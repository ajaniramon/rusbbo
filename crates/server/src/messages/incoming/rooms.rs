//! Java: `messages/incoming/rooms` (entering a room and loading it).

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use tracing::warn;

use crate::game::fuserights;
use crate::game::room::items::Behaviour;
use crate::game::room::{self, current_room};
use crate::messages::Malformed;
use crate::messages::outgoing::rooms::{self as out, ConnectError};
use crate::messages::outgoing::rooms_user as out_user;
use crate::session::Session;
use crate::world::World;

/// Java: `ROOM_DIRECTORY`: enter a public room.
pub fn room_directory(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    reader.int().ok_or(Malformed)?;
    let room_id = reader.int().ok_or(Malformed)?;
    // TODO(games): room id -1 while entering a game arena.
    let Some(room) = u32::try_from(room_id)
        .ok()
        .and_then(|id| world.room_by_id(id))
    else {
        return Ok(());
    };
    let (category, is_public, visitors_now, visitors_max) = {
        let r = room.lock();
        (
            r.data.category,
            r.is_public(),
            r.data.visitors_now,
            r.data.visitors_max,
        )
    };
    let Some(category) = u32::try_from(category)
        .ok()
        .and_then(|c| world.room_categories.get(&c))
    else {
        // Java throws a NullPointerException here: the request is dropped.
        warn!(room = room.id, category, "room has no navigator category");
        return Ok(());
    };
    let (rank, has_club) = {
        let p = session.player.lock();
        (p.details.as_ref().map_or(0, |d| d.rank), p.has_club())
    };
    if category.club_only && !has_club {
        room::leave_current_room(world, session, false);
        session.send(out::cant_connect(ConnectError::ClubOnly));
        return Ok(());
    }
    if is_public {
        let (mut total_now, mut total_max) = (visitors_now, visitors_max);
        for child in world.child_rooms(room.id, is_public) {
            if let Some((now, max)) = world.room_visitors(child) {
                total_now += now;
                total_max += max;
            }
        }
        if total_now >= total_max
            && !fuserights::for_player(rank, has_club).contains(&"fuse_enter_full_rooms")
        {
            session.send(out::cant_connect(ConnectError::RoomFull));
            return Ok(());
        }
    }
    session.send(out::open_connection());
    session.send(out::room_url());
    if is_public {
        room::enter_room(world, session, room, None, true);
    }
    Ok(())
}

/// Java: `G_HMAP`. TODO(games): SnowStorm maps and the game status. TODO(flash): FLOOR_MAP.
pub fn g_hmap(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let msg = out::heightmap(&room.lock().model.heightmap);
    session.send(msg);
    Ok(())
}

/// Java: `G_USRS`: the public room furniture and the room's floor items.
pub fn g_usrs(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let r = room.lock();
    let items = &r.mapping.items;
    session.send(out::objects_world(items.iter().filter(|i| {
        !i.has(Behaviour::Invisible)
            && !i.has(Behaviour::PrivateFurniture)
            && i.has(Behaviour::PublicSpaceObject)
    })));
    if r.model.name == "park_a" {
        session.send(out::active_objects_park_a());
    } else {
        session.send(out::active_objects(items.iter().filter(|i| {
            !i.has(Behaviour::PublicSpaceObject) && !i.has(Behaviour::WallItem)
        })));
    }
    // TODO(messenger): sendStatusUpdate.
    Ok(())
}

/// Java: `G_OBJS`: two literal messages.
pub fn g_objs(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    if current_room(world, session).is_none() {
        return Ok(());
    }
    session.send(out::group_badges_empty());
    session.send(out::alias_toggle());
    Ok(())
}

/// Java: `G_STAT`: who is in the room, and the user's own arrival for the others.
pub fn g_stat(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    // TODO(games): spectators and game players.
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    r.try_room_entry();
    let model = r.model.name.clone();
    session.send(out_user::user_objects(&r.entities, &model));
    if let Some(idx) = r.entity_index(session.id) {
        let msg = out_user::user_objects([&r.entities[idx]], &model);
        r.send_except(msg, session.id);
    }
    session.send(out_user::user_statuses(&r.entities, &model));
    // TODO(effects): USER_AVATAR_EFFECT for the user and the others. TODO(groups): memberships.
    for e in &r.entities {
        if e.is_dancing() {
            session.send(out_user::user_dance(e.instance_id, e.dance_id));
        }
        if e.is_sleeping {
            session.send(out_user::user_sleep(e.instance_id, true));
        }
    }
    // TODO(pool): SHOWPROGRAM for items with a program value. TODO(items): dice and wheel updates.
    // TODO(groups): GROUP_BADGES of the user's favourite group. TODO(badges): rooms_entry_badges.
    // TODO(inventory): drop hand items that are in the room.
    Ok(())
}
