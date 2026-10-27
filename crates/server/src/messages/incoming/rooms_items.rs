//! Java: `messages/incoming/rooms/items` (placing, moving, picking up and using furniture).
//!
//! Handlers that touch the hand lock the player first and the room second (the lock order).

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use tracing::error;

use super::latin1;
use crate::game::now;
use crate::game::pathfinder::Position;
use crate::game::room::furniture::blocks_interaction;
use crate::game::room::items::{Behaviour, Item, is_numeric};
use crate::game::room::mapping::is_valid_tile;
use crate::game::room::trade::show_trade_ban_alert;
use crate::game::room::{RoomRef, current_room};
use crate::messages::Malformed;
use crate::messages::outgoing::{alerts, inventory as out_inventory, rooms_items as out};
use crate::session::Session;
use crate::world::World;

/// Who is acting, read from the player (Java: `player.getDetails()` and `hasFuse`).
struct Actor {
    user_id: u32,
    is_mod: bool,
}

fn actor(session: &Session) -> Option<Actor> {
    let p = session.player.lock();
    Some(Actor {
        user_id: p.user_id()?,
        is_mod: p.has_fuse("fuse_mod"),
    })
}

fn room_of(world: &World, session: &Session) -> Option<RoomRef> {
    current_room(world, session)
}

/// Java: `Integer.parseInt`, which takes an optional sign and digits.
fn parse_int(value: Option<&str>) -> Option<i32> {
    value?.parse().ok()
}

/// Java: `PLACESTUFF`: `"<id> <x> <y> <rotation>"` for floor items, `"<id> <wall position>"` for
/// wall items.
pub fn place_stuff(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let mut p = session.player.lock();
    let Some(user) = p.details.clone().filter(|_| p.logged_in) else {
        return Ok(());
    };
    let is_mod = p.has_fuse("fuse_mod");
    let is_mute = p.has_fuse("fuse_mute");
    let mut r = room.lock();
    if !r.has_rights(user.id, true) && !is_mod {
        return Ok(());
    }
    let content = latin1(reader.string().ok_or(Malformed)?);
    let data: Vec<&str> = content.split(' ').collect();
    if !is_numeric(data[0]) {
        return Ok(());
    }
    let Some(item_id) = parse_int(Some(data[0])) else {
        return Ok(());
    };
    let Some(owned) = p
        .inventory
        .item(item_id)
        .filter(|i| i.is_visible())
        .cloned()
    else {
        return Ok(());
    };
    let is_owner = r.data.owner_id == user.id;
    if !is_mute && !is_owner {
        if !owned.definition.record.is_tradable {
            return Ok(());
        }
        if user.trade_ban_expiration > 0 && user.trade_ban_expiration > now() {
            session.send(alerts::alert(&show_trade_ban_alert(
                user.trade_ban_expiration,
            )));
            return Ok(());
        }
    }
    // TODO(trade): a redeemable placed in someone else's room from the owner's IP trade-bans the
    // placer (RoomTradeManager.addTradeBan).
    if !is_owner {
        let owner_banned = world
            .store
            .user(r.data.owner_id)
            .ok()
            .flatten()
            .is_some_and(|o| o.trade_ban_expiration > 0 && o.trade_ban_expiration > now());
        if owner_banned {
            session.send(alerts::alert("The room owner is trade banned."));
            return Ok(());
        }
        // TODO(transactions): TransactionDao.createTransaction ("Placed item ... into ...'s room").
    }

    let mut item = Item::from_owned(owned);
    if item.has(Behaviour::WallItem) {
        let Some(wall_position) = content.get(data[0].len() + 1..) else {
            return Ok(()); // Java's substring throws.
        };
        if item.has(Behaviour::PostIt) {
            // TODO(post-its): a new sticky item is created from the pad, whose count goes down.
            return Ok(());
        }
        if let Some(flat) = item.flat.as_mut() {
            flat.record.wall_position = wall_position.to_string();
        }
    } else {
        let (Some(x), Some(y), Some(mut rotation)) = (
            parse_int(data.get(1).copied()),
            parse_int(data.get(2).copied()),
            parse_int(data.get(3).copied()),
        ) else {
            return Ok(()); // Java's parseInt throws.
        };
        if item.has(Behaviour::RedirectRotation0) {
            rotation = 0;
        }
        if item.has(Behaviour::RedirectRotation2) {
            rotation = 2;
        }
        if item.has(Behaviour::RedirectRotation4) {
            rotation = 4;
        }
        if !item.allowed_rotations().contains(&rotation) {
            let Some(&first) = item.allowed_rotations().first() else {
                return Ok(()); // Java's get(0) throws.
            };
            rotation = first;
        }
        if !r.is_valid_move(world, &item, x, y, rotation) {
            return Ok(());
        }
        if r.mapping.tile(Position::new(x, y)).is_some() {
            item.position.x = x;
            item.position.y = y;
            item.position.set_rotation(rotation);
        }
    }
    if r.has_too_many(&item) {
        return Ok(());
    }
    r.add_flat_item(world, item);
    p.inventory.take_item(item_id);
    // TODO(flash): Flash clients get the hand again (getView("new")).
    if let Some(idx) = r.entity_index(session.id) {
        r.reset_room_timer(world, idx);
    }
    Ok(())
}

/// Java: `MOVESTUFF`: id, x, y and rotation of a floor item.
pub fn move_stuff(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let Some(actor) = actor(session) else {
        return Ok(());
    };
    let mut r = room.lock();
    if !r.has_rights(actor.user_id, true) && !actor.is_mod {
        return Ok(());
    }
    let item_id = reader.int().ok_or(Malformed)?;
    let Some(idx) = r.mapping.item_index(item_id) else {
        return Ok(());
    };
    if r.mapping.items[idx].has(Behaviour::WallItem) {
        return Ok(());
    }
    // TODO(dice, items): items waiting for an update (requiresUpdate) cannot be moved.
    let x = reader.int().ok_or(Malformed)?;
    let y = reader.int().ok_or(Malformed)?;
    let rotation = reader.int().ok_or(Malformed)?;
    let old = r.mapping.items[idx].position;
    // TODO(rollers): rolling items cannot be rotated.
    let unchanged = old.x == x && old.y == y && old.rotation() == rotation;
    if unchanged || !r.is_valid_move(world, &r.mapping.items[idx], x, y, rotation) {
        // Sent even though nothing moved, "otherwise the client will be confused".
        session.send(out::move_flooritem(&r.mapping.items[idx]));
        return Ok(());
    }
    if !r.mapping.items[idx].allowed_rotations().contains(&rotation) {
        return Ok(());
    }
    r.move_flat_item(world, idx, x, y, rotation);
    if let Some(e) = r.entity_index(session.id) {
        r.reset_room_timer(world, e);
    }
    Ok(())
}

/// Java: `ADDSTRIPITEM`: picks an item up into the hand. Two ints, the first is ignored.
pub fn add_strip_item(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let mut p = session.player.lock();
    let Some(user_id) = p.user_id() else {
        return Ok(());
    };
    let any_controller = p.has_fuse("fuse_any_room_controller");
    let owned = {
        let mut r = room.lock();
        if !r.is_owner(user_id) && !any_controller {
            return Ok(());
        }
        reader.int().ok_or(Malformed)?;
        let item_id = reader.int().ok_or(Malformed)?;
        let Some(idx) = r.mapping.item_index(item_id) else {
            return Ok(());
        };
        let item = &mut r.mapping.items[idx];
        if item.has(Behaviour::PostIt) {
            return Ok(()); // The client does not allow picking up post-its either.
        }
        // Java: setOwnerId + ItemDao.updateItemOwnership; the removal below saves the row.
        if let Some(flat) = item.flat.as_mut() {
            flat.record.user_id = Some(user_id);
        }
        r.remove_flat_item(world, idx).to_owned()
    };
    let Some(owned) = owned else {
        error!(user = user_id, "picked up an item that is not furniture");
        return Ok(());
    };
    p.inventory.add_item(owned);
    let hand = p.inventory.get_view(&world.store, "update");
    session.send(out_inventory::inventory(&hand));
    Ok(())
}

/// Java: `USEFURNITURE`: toggles a floor item. The status argument is read and ignored by the
/// ported interactors.
pub fn use_furniture(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let Some(actor) = actor(session) else {
        return Ok(());
    };
    let item_id = reader.int().ok_or(Malformed)?;
    let _status = reader.int().ok_or(Malformed)?;
    let mut r = room.lock();
    let Some(idx) = r.mapping.item_index(item_id) else {
        return Ok(());
    };
    let item = &r.mapping.items[idx];
    if blocks_interaction(item) {
        return Ok(());
    }
    if item.has(Behaviour::RequiresRightsForInteraction)
        && !r.has_rights(actor.user_id, true)
        && !actor.is_mod
    {
        return Ok(());
    }
    if item.has(Behaviour::RequiresTouchingForInteraction) {
        let Some(e) = r.entity_index(session.id) else {
            return Ok(());
        };
        let user_position = r.entities[e].position;
        if item.position.distance_squared(&user_position) > 1 {
            let front = item.position.square_in_front();
            let walker = r.walker(e);
            if is_valid_tile(&r.mapping, Some(&walker), front) {
                r.walk_to_player(world, e, front.x, front.y);
            }
            // TODO(items): otherwise Java walks to RoomTile.getNextAvailablePosition.
            return Ok(());
        }
    }
    // TODO(commands): the item debug alert (":debugfurniture").
    r.interact(world, idx);
    Ok(())
}

/// Java: `USEWALLITEM`: toggles a wall item.
pub fn use_wall_item(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let Some(actor) = actor(session) else {
        return Ok(());
    };
    let item_id = reader.int().ok_or(Malformed)?;
    let mut r = room.lock();
    let Some(idx) = r.mapping.item_index(item_id) else {
        return Ok(());
    };
    let item = &r.mapping.items[idx];
    if !item.has(Behaviour::WallItem) || blocks_interaction(item) {
        return Ok(());
    }
    if item.has(Behaviour::RequiresRightsForInteraction)
        && !r.has_rights(actor.user_id, true)
        && !actor.is_mod
    {
        return Ok(());
    }
    r.interact(world, idx);
    Ok(())
}
