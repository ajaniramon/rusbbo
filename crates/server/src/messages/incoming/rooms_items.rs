//! Java: `messages/incoming/rooms/items` (placing, moving, picking up and using furniture).
//!
//! Handlers that touch the hand lock the player first and the room second (the lock order).

use std::sync::Arc;
use std::time::Duration;

use havana_protocol::body::ClientReader;
use tracing::error;

use super::latin1;
use crate::game::catalogue::CatalogueManager;
use crate::game::item::interactors::teleport;
use crate::game::now;
use crate::game::pathfinder::Position;
use crate::game::player::Player;
use crate::game::room::furniture::blocks_interaction;
use crate::game::room::items::{Behaviour, Interaction, Item, is_numeric};
use crate::game::room::mapping::is_valid_tile;
use crate::game::room::trade::show_trade_ban_alert;
use crate::game::room::{Room, RoomRef, current_room};
use crate::messages::Malformed;
use crate::messages::outgoing::{
    alerts, catalogue as out_catalogue, inventory as out_inventory, rooms_items as out,
};
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
            place_sticky(world, &mut p, &mut r, item_id, wall_position);
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
    if r.has_too_many(world, session, p.has_club(), &item) {
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

/// Java: `ENTER_ONEWAY_DOOR`: from the square in front of a one-way door, open it, walk through to
/// the square behind and close it again a second later (on a scheduler timer).
pub fn enter_oneway_door(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let item_id = reader.int().ok_or(Malformed)?;
    let mut r = room.lock();
    let Some(idx) = r.mapping.item_index(item_id) else {
        return Ok(());
    };
    if !r.mapping.items[idx].has(Behaviour::OneWayGate) {
        return Ok(());
    }
    let Some(e) = r.entity_index(session.id) else {
        return Ok(());
    };
    let door = r.mapping.items[idx].position;
    if door.square_in_front() != r.entities[e].position {
        return Ok(());
    }
    let destination = door.square_behind();
    let walker = r.walker(e);
    if !is_valid_tile(&r.mapping, Some(&walker), destination) {
        return Ok(());
    }
    r.send(out::change_status(item_id, 1));
    r.mapping.items[idx].custom_data = "1".into();
    r.walk_to(e, destination.x, destination.y);
    if !r.entities[e].is_walking {
        return Ok(());
    }
    r.entities[e].enable_walking_on_stop = true;
    r.schedule(world, Duration::from_secs(1), move |_, r| {
        r.send(out::change_status(item_id, 0));
        if let Some(idx) = r.mapping.item_index(item_id) {
            r.mapping.items[idx].custom_data = "0".into();
        }
    });
    Ok(())
}

/// Java: the post-it part of `PLACESTUFF`: a new yellow sticky owned by the room owner goes on
/// the wall, and the pad (`pad_id`, its custom data the stickies left) loses one; at 0 it is
/// deleted.
fn place_sticky(world: &World, p: &mut Player, r: &mut Room, pad_id: i32, wall_position: &str) {
    let Some(pad) = p.inventory.item(pad_id) else {
        return;
    };
    let mut draft = havana_storage::Item::with_defaults(
        r.data.owner_id,
        pad.definition.record.id,
        "FFFF33".into(),
        now(),
    );
    draft.room_id = r.id() as i32;
    let created = world.store.write().and_then(|mut w| {
        let created = w.insert_item(&draft, now())?;
        w.commit()?;
        Ok(created)
    });
    let mut sticky = match created {
        Ok(created) => match world.items.make_item(created) {
            Some(sticky) => sticky,
            None => return,
        },
        Err(e) => {
            error!("creating a sticky: {e}");
            return;
        }
    };
    sticky.record.wall_position = wall_position.to_string();
    r.add_flat_item(world, Item::from_owned(sticky));

    let Some(pad) = p.inventory.item_mut(pad_id) else {
        return;
    };
    if !is_numeric(&pad.record.custom_data) {
        pad.record.custom_data = "1".into();
    }
    let total = pad.record.custom_data.parse::<i64>().unwrap_or(1) - 1;
    if total <= 0 {
        let id = pad.record.id;
        p.inventory.take_item(pad_id);
        if let Err(e) = world.store.delete_items(&[id]) {
            error!(item = id, "deleting a sticky pad: {e}");
        }
    } else {
        pad.record.custom_data = total.to_string();
        if let Err(e) = world.store.update_item(&pad.record) {
            error!(item = pad.record.id, "saving a sticky pad: {e}");
        }
    }
}

/// Java: `G_IDATA`: a wall item's data; for a post-it its colour and text.
pub fn g_idata(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let item_id = reader.int().ok_or(Malformed)?;
    let r = room.lock();
    let Some(item) = r.mapping.item_index(item_id).map(|i| &r.mapping.items[i]) else {
        return Ok(());
    };
    if item.has(Behaviour::PostIt) {
        // Java's substring(0, 6) throws on shorter data.
        let Some(colour) = item.custom_data.get(..6) else {
            return Ok(());
        };
        let text = item.custom_data.get(6..).unwrap_or("");
        session.send(out::idata(item, colour, text));
    } else {
        session.send(out::idata(item, "", ""));
    }
    Ok(())
}

/// The post-it colours `SETITEMDATA` accepts.
const POST_IT_COLOURS: [&str; 5] = ["FFFFFF", "FFFF33", "FF9CFF", "9CFF9C", "9CCEFF"];

/// Java: `StringUtil.filterInput(input, false)`: like [`crate::game::room::filter_input`] but
/// keeping carriage returns. TODO(settings): `normalise.input.strings`.
fn filter_input_keep_newlines(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            '\u{1}' | '\u{2}' | '\t' | '\n' | '\u{c}' => ' ',
            c => c,
        })
        .collect()
}

/// Java: `SETITEMDATA`: writes a post-it, `"<colour> <text>"`. Without rights the text may only
/// grow; only the owner may change the colour.
pub fn set_item_data(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let (user_id, is_mod, any_controller) = {
        let p = session.player.lock();
        let Some(user_id) = p.user_id() else {
            return Ok(());
        };
        (
            user_id,
            p.has_fuse("fuse_mod"),
            p.has_fuse("fuse_any_room_controller"),
        )
    };
    let item_id = reader.int().ok_or(Malformed)?;
    let contents = latin1(reader.string().ok_or(Malformed)?);
    // Java's substring(0, 6) and substring(7) throw on shorter contents.
    let (Some(colour), Some(message)) = (contents.get(..6), contents.get(7..)) else {
        return Err(Malformed);
    };
    let mut message = filter_input_keep_newlines(message);
    if !POST_IT_COLOURS.contains(&colour) {
        return Ok(());
    }
    let mut r = room.lock();
    let Some(idx) = r.mapping.item_index(item_id) else {
        return Ok(());
    };
    let item = &r.mapping.items[idx];
    if !item.has(Behaviour::PostIt) {
        return Ok(());
    }
    let (old_colour, old_text) = match (item.custom_data.get(..6), item.custom_data.get(6..)) {
        (Some(colour), Some(text)) => (colour.to_string(), text.to_string()),
        _ => ("FFFFFF".to_string(), String::new()),
    };
    if !r.has_rights(user_id, true)
        && !is_mod
        && !old_text.is_empty()
        && !message.starts_with(&old_text)
    {
        return Ok(());
    }
    if !r.is_owner(user_id) && !any_controller && old_colour != colour {
        return Ok(());
    }
    if message.chars().count() > 684 {
        message = message.chars().take(684).collect();
    }
    r.mapping.items[idx].custom_data = format!("{colour}{message}");
    let msg = out::stuff_data_update(&r.mapping.items[idx]);
    r.send(msg);
    r.save_item(world, idx);
    Ok(())
}

/// Java: `REMOVEITEM`: the owner (or a moderator with the fuse for it) deletes a post-it or
/// photo from the wall.
pub fn remove_item(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = room_of(world, session) else {
        return Ok(());
    };
    let (user_id, remove_photos, remove_stickies) = {
        let p = session.player.lock();
        let Some(user_id) = p.user_id() else {
            return Ok(());
        };
        (
            user_id,
            p.has_fuse("fuse_remove_photos"),
            p.has_fuse("fuse_remove_stickies"),
        )
    };
    let item_id = reader.int().ok_or(Malformed)?;
    let mut r = room.lock();
    let Some(idx) = r.mapping.item_index(item_id) else {
        return Ok(());
    };
    let item = &r.mapping.items[idx];
    let allowed = r.is_owner(user_id)
        || (item.has(Behaviour::Photo) && remove_photos)
        || (item.has(Behaviour::PostIt) && remove_stickies);
    if !allowed {
        return Ok(());
    }
    // Java: the current item's onEntityLeave trigger, for the remover.
    if let Some(e) = r.entity_index(session.id) {
        if let Some(interaction) = r.current_item(e).map(|i| i.interaction) {
            let is_player = r.entities[e].is_player();
            r.on_entity_leave(interaction, session, is_player);
        }
    }
    let removed = r.remove_flat_item(world, idx);
    if let Some(flat) = removed.flat.as_ref() {
        if let Err(e) = world.store.delete_items(&[flat.record.id]) {
            error!(item = flat.record.id, "deleting an item: {e}");
        }
    }
    // TODO(flash): Flash clients get the hand again (getView("new")).
    if let Some(e) = r.entity_index(session.id) {
        r.reset_room_timer(world, e);
    }
    Ok(())
}

/// Java: `PRESENTOPEN`: the owner opens a present. Ordinary furniture keeps the present's item
/// (and virtual id) with the wrapped definition; the rest is bought anew and the present deleted.
/// TODO(ecotron): `ECO_BOX` boxes and their `FLAG:` prizes.
pub fn present_open(
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
    let mut r = room.lock();
    if !r.is_owner(user_id) && !any_controller {
        return Ok(());
    }
    let item_id = reader.int().ok_or(Malformed)?;
    let Some(idx) = r.mapping.item_index(item_id) else {
        return Ok(());
    };
    if !r.mapping.items[idx].has(Behaviour::Present) {
        return Ok(());
    }
    let data: Vec<String> = r.mapping.items[idx]
        .custom_data
        .split('|')
        .map(str::to_string)
        .collect();
    // Java indexes the array and parses the id; anything missing throws.
    let (Some(sale_code), Some(received_from), Some(extra_data)) = (
        data.first().and_then(|s| s.parse::<i32>().ok()),
        data.get(1),
        data.get(3),
    ) else {
        return Err(Malformed);
    };
    let timestamp = data
        .get(4)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(now);
    let _ = (received_from, timestamp); // TODO(trophies): the giver and date of a trophy.
    let Some(catalogue_item) = world.catalogue.item(sale_code).cloned() else {
        let present = r.remove_flat_item(world, idx);
        if let Some(flat) = present.flat.as_ref() {
            if let Err(e) = world.store.delete_items(&[flat.record.id]) {
                error!(item = flat.record.id, "deleting a present: {e}");
            }
        }
        return Ok(());
    };
    let reuse = match catalogue_item.definition.as_ref() {
        Some(def) if !catalogue_item.is_package => {
            !def.has_behaviour(Behaviour::PrizeTrophy)
                && !def.has_behaviour(Behaviour::Teleporter)
                && def.interaction_type != crate::game::item::base::InteractionType::PetNest
                && !def.has_behaviour(Behaviour::Roomdimmer)
                && !def.has_behaviour(Behaviour::Decoration)
                && !def.has_behaviour(Behaviour::PostIt)
                && !def.sprite().eq_ignore_ascii_case("film")
        }
        _ => false,
    };
    if reuse {
        let Some(def) = catalogue_item.definition.clone() else {
            return Ok(());
        };
        let Some(mut owned) = r.remove_flat_item(world, idx).to_owned() else {
            return Ok(());
        };
        owned.record.definition_id = def.record.id;
        owned.record.custom_data = extra_data.clone();
        owned.definition = def;
        if let Err(e) = world.store.update_item(&owned.record) {
            error!(item = owned.record.id, "saving an opened present: {e}");
        }
        session.send(out_catalogue::deliver_present(
            &owned.definition,
            &owned.record.custom_data,
        ));
        p.inventory.add_item(owned);
    } else {
        if CatalogueManager::unsupported(&catalogue_item).is_some() {
            return Ok(()); // Nothing this port can create; leave the present closed.
        }
        let bought = match world.catalogue.purchase(
            &world.store,
            &world.items,
            session,
            &mut p,
            &catalogue_item,
            extra_data,
            now(),
        ) {
            Ok(bought) => bought,
            Err(e) => {
                error!("opening a present: {e}");
                return Ok(());
            }
        };
        if let Some(first) = bought
            .first()
            .and_then(|id| p.inventory.item_by_database_id(*id))
        {
            session.send(out_catalogue::deliver_present(
                &first.definition,
                &first.record.custom_data,
            ));
            let hand = p.inventory.get_view(&world.store, "new");
            session.send(out_inventory::inventory(&hand));
        }
        let present = r.remove_flat_item(world, idx);
        if let Some(flat) = present.flat.as_ref() {
            if let Err(e) = world.store.delete_items(&[flat.record.id]) {
                error!(item = flat.record.id, "deleting a present: {e}");
            }
        }
        return Ok(());
    }
    let hand = p.inventory.get_view(&world.store, "new");
    session.send(out_inventory::inventory(&hand));
    Ok(())
}

/// Java: `RoomTradeManager.showTradeBanAlert`.
pub(crate) fn trade_ban_alert(expiration: i64) -> String {
    let uptime = (expiration - now()) * 1000;
    let days = uptime / (1000 * 60 * 60 * 24);
    let hours = (uptime - days * (1000 * 60 * 60 * 24)) / (1000 * 60 * 60);
    format!(
        "You are temporarily banned from trading.<br>The ban will expire in {days} day(s) and \
         {hours} hours(s)"
    )
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
    if r.mapping.items[idx].requires_update {
        return Ok(()); // A dice rolling, a wheel spinning...
    }
    let x = reader.int().ok_or(Malformed)?;
    let y = reader.int().ok_or(Malformed)?;
    let rotation = reader.int().ok_or(Malformed)?;
    let old = r.mapping.items[idx].position;
    if old == Position::new(x, y)
        && old.rotation() != rotation
        && r.mapping.items[idx].rolling.is_some()
    {
        return Ok(()); // No rotating while rolling.
    }
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
    let deferred = r.take_deferred();
    drop(r);
    crate::game::room::run_deferred(world, deferred);
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

/// Java: `USEFURNITURE`: uses a floor item (its interactor's `onInteract` with the status the
/// client sends, which only the scoreboard reads).
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
    let status = reader.int().ok_or(Malformed)?;
    let teleporting = session.player.lock().authenticate_teleporter.is_some();
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
            walk_next_to(world, &mut r, e, idx, true);
            return Ok(());
        }
    }
    // TODO(commands): the item debug alert (":debugfurniture").
    use_item(world, session, r, idx, status, teleporting);
    Ok(())
}

/// Java: `USEFURNITURE`/`SET_RANDOM_STATE` walking a user who is not next to an item to the
/// square in front of it, or (when that is blocked) to the first free square around it. With
/// `check_tile` (`USEFURNITURE`), a missing item tile falls back to the front square.
fn walk_next_to(world: &World, r: &mut Room, entity: usize, idx: usize, check_tile: bool) {
    let position = r.mapping.items[idx].position;
    let mut next = Some(position.square_in_front());
    let walker = r.walker(entity);
    if !is_valid_tile(&r.mapping, Some(&walker), position.square_in_front()) {
        if r.mapping.tile(position).is_some() {
            next = r.next_available_position(entity, position);
        } else if !check_tile {
            return; // Java's getTile() is null there and throws.
        }
    }
    if let Some(next) = next {
        r.walk_to_player(world, entity, next.x, next.y);
    }
}

/// Java: `getInteractionType().getTrigger().onInteract(player, room, item, status)` for the
/// player of `session`; teleporters need the player too, once the room is released.
fn use_item(
    world: &World,
    session: &Arc<Session>,
    mut r: parking_lot::MutexGuard<'_, Room>,
    idx: usize,
    status: i32,
    teleporting: bool,
) {
    if r.mapping.items[idx].interaction == Interaction::Teleport {
        let travel = teleport::on_interact(world, session, &mut r, idx, teleporting);
        drop(r);
        if travel.is_some() {
            session.player.lock().authenticate_teleporter = travel;
        }
        return;
    }
    if let Some(e) = r.entity_index(session.id) {
        r.on_interact(world, e, idx, status);
    }
    let deferred = r.take_deferred();
    drop(r);
    crate::game::room::run_deferred(world, deferred);
}

/// Java: `USEWALLITEM`: uses a wall item (status -1).
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
    let teleporting = session.player.lock().authenticate_teleporter.is_some();
    let r = room.lock();
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
    use_item(world, session, r, idx, -1, teleporting);
    Ok(())
}

/// Java: `SET_RANDOM_STATE`: like `USEFURNITURE` with status -1, except that a teleporter is
/// used from anywhere (its interactor walks the user to it).
pub fn set_random_state(
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
    let teleporting = session.player.lock().authenticate_teleporter.is_some();
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
    if item.has(Behaviour::RequiresTouchingForInteraction) && !item.has(Behaviour::Teleporter) {
        let Some(e) = r.entity_index(session.id) else {
            return Ok(());
        };
        if item.position.distance_squared(&r.entities[e].position) > 1 {
            walk_next_to(world, &mut r, e, idx, false);
            return Ok(());
        }
    }
    use_item(world, session, r, idx, -1, teleporting);
    Ok(())
}

/// `room` locked, with the index of the session's entity in it.
fn in_room<'a>(
    session: &Session,
    room: &'a Option<RoomRef>,
) -> Option<(parking_lot::MutexGuard<'a, Room>, usize)> {
    let r = room.as_ref()?.lock();
    let e = r.entity_index(session.id)?;
    Some((r, e))
}

/// Java: `THROW_DICE` (VL64 item id).
pub fn throw_dice(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room = room_of(world, session);
    let Some((mut r, e)) = in_room(session, &room) else {
        return Ok(());
    };
    let item_id = reader.int().ok_or(Malformed)?;
    if let Some(idx) = r.mapping.item_index(item_id) {
        r.throw_dice(world, e, idx);
    }
    Ok(())
}

/// Java: `DICE_OFF` (VL64 item id).
pub fn dice_off(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room = room_of(world, session);
    let Some((mut r, e)) = in_room(session, &room) else {
        return Ok(());
    };
    let item_id = reader.int().ok_or(Malformed)?;
    if let Some(idx) = r.mapping.item_index(item_id) {
        r.dice_off(world, e, idx);
    }
    Ok(())
}

/// Java: `SPIN_WHEEL_OF_FORTUNE` (VL64 item id), for users with rights.
pub fn spin_wheel_of_fortune(
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
    if let Some(idx) = r.mapping.item_index(item_id) {
        r.spin_wheel(world, idx);
    }
    Ok(())
}

/// Java: `CONVERT_FURNI_TO_CREDITS` (VL64 item id): the owner exchanges a credit furni in the
/// room (`CF_<amount>_<name>`) for its credits.
/// TODO(transactions): the "Exchanged ... into ... credits" transaction.
pub fn convert_furni_to_credits(
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
    let mut r = room.lock();
    if !r.is_owner(user_id) && !any_controller {
        return Ok(());
    }
    let item_id = reader.int().ok_or(Malformed)?;
    if item_id < 0 {
        return Ok(());
    }
    let Some(idx) = r.mapping.item_index(item_id) else {
        return Ok(());
    };
    let item = &r.mapping.items[idx];
    let Some(database_id) = item.flat.as_ref().map(|f| f.record.id) else {
        return Ok(());
    };
    if !item.has(Behaviour::Redeemable) {
        return Ok(());
    }
    // Java: Integer.parseInt(sprite.split("_")[1]), which throws on anything else.
    let Some(amount) = item
        .sprite
        .split('_')
        .nth(1)
        .and_then(|a| a.parse::<i32>().ok())
    else {
        return Ok(());
    };
    // Java: ItemDao.redeemCreditItem, the item deleted and the credits added in one transaction.
    let redeemed = (|| -> anyhow::Result<Option<i32>> {
        let mut w = world.store.write()?;
        let Some(mut user) = w.user(user_id)? else {
            return Ok(None);
        };
        w.delete_item(database_id)?;
        user.credits += amount;
        w.put_user(&user)?;
        w.commit()?;
        Ok(Some(user.credits))
    })();
    let credits = match redeemed {
        Ok(Some(credits)) => credits,
        Ok(None) | Err(_) => {
            if let Err(e) = redeemed {
                error!(item = database_id, "redeeming a credit furni: {e}");
            }
            session.send(alerts::alert(
                "Unable to redeem furniture! Contact staff or support team.",
            ));
            return Ok(());
        }
    };
    r.remove_flat_item(world, idx);
    drop(r);
    if let Some(details) = p.details.as_mut() {
        details.credits = credits;
    }
    session.send(crate::messages::outgoing::user::credit_balance(credits));
    Ok(())
}
