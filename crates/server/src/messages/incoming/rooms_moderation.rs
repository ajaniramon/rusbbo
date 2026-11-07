//! Java: `messages/incoming/rooms/moderation`: rights, kicking and answering the doorbell.
//!
//! Shockwave clients send the target's bare name as the whole body of `ASSIGNRIGHTS`,
//! `REMOVERIGHTS` and `KICK` (Java: `reader.contents()`). TODO(flash): Flash sends user ids.
//! TODO(moderation): BANUSER.
//!
//! Lock order: the sender's player lock, then the target's, are each taken and released before
//! the room is locked; while the room is locked no player is locked.

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use tracing::error;

use super::latin1;
use super::rooms_settings::room_by_client_id;
use crate::game::room::{RoomRef, current_room, run_deferred};
use crate::messages::Malformed;
use crate::messages::outgoing::{alerts, rooms as out_rooms, rooms_user as out_user};
use crate::session::Session;
use crate::world::World;

/// The sender's room, user id, and whether they control any room (`fuse_any_room_controller`).
fn sender(world: &World, session: &Session) -> Option<(RoomRef, u32, bool)> {
    let room = current_room(world, session)?;
    let p = session.player.lock();
    Some((room, p.user_id()?, p.has_fuse("fuse_any_room_controller")))
}

/// The online player called `name` if they are in `room`, with their user id.
fn target_in_room(world: &World, name: &str, room: &RoomRef) -> Option<(Arc<Session>, u32)> {
    let target = world.player_by_name(name)?;
    let user_id = {
        let p = target.player.lock();
        if p.room_id != Some(room.id) {
            return None;
        }
        p.user_id()?
    };
    Some((target, user_id))
}

/// Java: `ASSIGNRIGHTS`: the target gets `YOUARECONTROLLER`, and `flatctrl` on the next tick.
pub fn assign_rights(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((room, user_id, any_controller)) = sender(world, session) else {
        return Ok(());
    };
    if !room.lock().is_owner(user_id) && !any_controller {
        return Ok(());
    }
    let name = latin1(reader.remaining());
    let Some((target, target_id)) = target_in_room(world, &name, &room) else {
        return Ok(());
    };
    let mut r = room.lock();
    if r.rights.contains(&target_id) {
        return Ok(());
    }
    // Saved first, under the room's lock: the rights in memory are always ones the store has
    // (the target may have left meanwhile; the rights are given all the same, as in Java).
    if let Err(e) = world.store.add_room_rights(room.id, target_id) {
        error!(room = room.id, user = target_id, "adding rights: {e}");
        return Ok(());
    }
    r.rights.push(target_id);
    if let Some(idx) = r.entity_index(target.id) {
        r.refresh_rights(idx, true);
        r.entities[idx].needs_update = true;
    }
    Ok(())
}

/// Java: `REMOVERIGHTS`. Java refreshes the target's rights before taking them away, so the
/// target is told it is still a controller and keeps `flatctrl` (see docs/flats-notes.md); and a
/// target who is online but in another room keeps the rights.
pub fn remove_rights(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((room, user_id, any_controller)) = sender(world, session) else {
        return Ok(());
    };
    if !room.lock().is_owner(user_id) && !any_controller {
        return Ok(());
    }
    // Java: PlayerDao.getId(name): any user, online or not.
    let name = latin1(reader.remaining());
    let Some(target_id) = world.store.user_by_name(&name).ok().flatten().map(|u| u.id) else {
        return Ok(());
    };
    if !room.lock().rights.contains(&target_id) {
        return Ok(());
    }
    let target = world.player_by_id(target_id);
    if let Some(target) = &target {
        if target.player.lock().room_id != Some(room.id) {
            return Ok(());
        }
    }
    {
        let mut r = room.lock();
        if let Some(idx) = target.and_then(|t| r.entity_index(t.id)) {
            r.refresh_rights(idx, true);
        }
        r.rights.retain(|id| *id != target_id);
    }
    if let Err(e) = world.store.remove_room_rights(room.id, target_id) {
        error!(room = room.id, user = target_id, "removing rights: {e}");
    }
    // TODO(flash): FLASH_EDITDATA.
    Ok(())
}

/// Java: `REMOVEALLRIGHTS`.
pub fn remove_all_rights(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(room) = room_by_client_id(world, room_id) else {
        return Ok(());
    };
    let (user_id, any_controller) = {
        let p = session.player.lock();
        let Some(id) = p.user_id() else {
            return Ok(());
        };
        (id, p.has_fuse("fuse_any_room_controller"))
    };
    {
        let mut r = room.lock();
        if !r.is_owner(user_id) && !any_controller {
            return Ok(());
        }
        r.rights.clear();
        for idx in 0..r.entities.len() {
            r.refresh_rights(idx, true);
        }
    }
    if let Err(e) = world.store.delete_room_rights(room.id) {
        error!(room = room.id, "removing all rights: {e}");
    }
    // TODO(flash): FLASH_EDITDATA.
    Ok(())
}

/// Java: `KICK`: the target is sent to the hotel view and walks out through the door.
pub fn kick(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let (user_id, can_kick) = {
        let p = session.player.lock();
        let Some(id) = p.user_id() else {
            return Ok(());
        };
        (id, p.has_fuse("fuse_kick"))
    };
    let name = latin1(reader.remaining());
    let Some((target, target_id)) = target_in_room(world, &name, &room) else {
        return Ok(());
    };
    if target_id == user_id {
        return Ok(());
    }
    let target_can_kick = target.player.lock().has_fuse("fuse_kick");
    let deferred = {
        let mut r = room.lock();
        if target_can_kick || r.is_owner(target_id) || (!r.has_rights(user_id, true) && !can_kick) {
            drop(r);
            session.send(alerts::alert(&world.text("modtool_rankerror")));
            return Ok(());
        }
        let Some(idx) = r.entity_index(target.id) else {
            return Ok(());
        };
        target.send(out_user::hotel_view());
        r.kick(world, idx, false, true);
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `LETUSERIN`: someone with rights answers the doorbell.
pub fn let_user_in(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((room, user_id, any_controller)) = sender(world, session) else {
        return Ok(());
    };
    if !room.lock().has_rights(user_id, true) && !any_controller {
        return Ok(());
    }
    let username = latin1(reader.string().ok_or(Malformed)?);
    let can_enter = reader.bool().ok_or(Malformed)?;
    let Some(entering) = world.player_by_name(&username) else {
        return Ok(());
    };
    let mut p = entering.player.lock();
    if p.authenticate_id == Some(room.id) {
        return Ok(());
    }
    if can_enter {
        p.authenticate_id = Some(room.id);
        entering.send(out_rooms::flat_letin());
    } else {
        entering.send(out_rooms::flat_not_allowed_to_enter());
    }
    Ok(())
}
