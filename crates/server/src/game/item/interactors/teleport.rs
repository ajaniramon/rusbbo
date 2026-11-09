//! Teleporters (Java: `game/item/interactors/types/TeleportInteractor` and the teleporter arrival
//! in `RoomEntityManager.silentlyEnterRoom`).
//!
//! A teleporter leads to its partner (`items_teleporter_links`). Using it walks the user in, then
//! scheduler timers play the effects and either warp the user onto the partner in the same room,
//! or send them `ROOMFORWARD` to the partner's room, where `GOTOFLAT` puts them on the partner
//! (see [`arrive`]). Which teleporter the user is travelling to is kept on the player
//! (`Player::authenticate_teleporter`), because it outlives the room.
//!
//! Timers take the player's lock and the room's lock one after the other, never together.

use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::game::pathfinder::Position;
use crate::game::room::mapping::is_valid_tile;
use crate::game::room::{Room, RoomHandle};
use crate::game::scheduler::schedule;
use crate::messages::outgoing::{messenger as out_messenger, rooms_user as out_user};
use crate::session::Session;
use crate::world::World;

pub const TELEPORTER_CLOSE: &str = "0";
pub const TELEPORTER_OPEN: &str = "1";
pub const TELEPORTER_EFFECTS: &str = "2";

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// Whether the player is on the way to a teleporter (Java:
/// `getAuthenticateTelporterId() != -1`).
fn is_teleporting(session: &Session) -> bool {
    session.player.lock().authenticate_teleporter.is_some()
}

/// Java: `setAuthenticateTelporterId(-1)` and `setAuthenticateTelporterRoomId(-1)`.
fn clear(session: &Session) {
    session.player.lock().authenticate_teleporter = None;
}

/// Whether the player is (still) in `room_id`.
fn in_room(session: &Session, room_id: u32) -> bool {
    session.player.lock().room_id == Some(room_id)
}

/// Runs `task` on the room behind `room`, if it is still there.
fn with_room(room: &Weak<RoomHandle>, task: impl FnOnce(&mut Room)) {
    let Some(room) = room.upgrade() else {
        return;
    };
    let mut r = room.lock();
    task(&mut r);
}

/// Sets the state of the item `virtual_id` in `r`, if it is still there.
fn set_status(r: &mut Room, virtual_id: i32, data: &str) {
    if let Some(idx) = r.mapping.item_index(virtual_id) {
        r.set_item_status(idx, data);
    }
}

/// Makes the user of `session` walk to the square in front of the item at `idx`.
fn walk_out(r: &mut Room, session_id: u64, item: Option<usize>) {
    let (Some(e), Some(idx)) = (r.entity_index(session_id), item) else {
        return;
    };
    let front = r.mapping.items[idx].position.square_in_front();
    r.walk_to(e, front.x, front.y);
}

/// Java: `TeleportInteractor.onInteract`. `teleporting` is whether the player is already on the
/// way somewhere. Returns the teleporter (database id, room id) the player now travels to, for
/// Java: `TeleportRoomHireInteractor.onInteract` (`ROOM_HIRE`): like a teleporter, the user walks
/// in (open, then the effects a second later) and half a second after that leaves (`LOGOUT` to the
/// room, closed) for the Hire-A-Room room. That room is built in memory with id 0 and never
/// registered (Java: `item.setTemporaryRoom`), so the `ROOMFORWARD` to room 0 leads nowhere: the
/// client's `TRYFLAT 0` finds no room, and the user stays where they were, unable to walk (as in
/// Java). Nothing of the temporary room is observable, so it is not built here.
pub fn room_hire_interact(world: &World, session: &Arc<Session>, r: &mut Room, idx: usize) {
    let Some(e) = r.entity_index(session.id) else {
        return;
    };
    let item = &r.mapping.items[idx];
    let (item_position, virtual_id) = (item.position, item.virtual_id);
    let front = item_position.square_in_front();
    let user_position = r.entities[e].position;
    if front != user_position && item_position != user_position {
        r.walk_to(e, front.x, front.y);
        return;
    }
    r.set_item_status(idx, TELEPORTER_OPEN);
    r.walk_to(e, item_position.x, item_position.y);
    r.entities[e].walking_allowed = false;
    r.schedule(world, ms(1000), move |_, r| {
        set_status(r, virtual_id, TELEPORTER_EFFECTS)
    });
    let s = session.clone();
    r.schedule(world, ms(1500), move |_, r| {
        if let Some(e) = r.entity_index(s.id) {
            r.send(out_user::logout(r.entities[e].instance_id));
        }
        set_status(r, virtual_id, TELEPORTER_CLOSE);
        s.send(crate::messages::outgoing::messenger::room_forward(false, 0));
    });
}

/// the caller to set on the player once the room's lock is released.
pub fn on_interact(
    world: &World,
    session: &Arc<Session>,
    r: &mut Room,
    idx: usize,
    teleporting: bool,
) -> Option<(u64, i32)> {
    if teleporting {
        return None;
    }
    let e = r.entity_index(session.id)?;
    let item = &r.mapping.items[idx];
    let (item_position, virtual_id) = (item.position, item.virtual_id);
    let database_id = item.flat.as_ref()?.record.id;
    let front = item_position.square_in_front();
    let user_position = r.entities[e].position;
    if front != user_position && item_position != user_position {
        r.walk_to(e, front.x, front.y);
        return None;
    }

    let pair_id = world.store.teleporter_link(database_id).ok().flatten();
    let target = pair_id.and_then(|id| world.store.item(id).ok().flatten());

    r.set_item_status(idx, TELEPORTER_OPEN);
    r.walk_to(e, item_position.x, item_position.y);
    r.entities[e].walking_allowed = false;

    let room_id = r.id();
    // Java: `targetTeleporter.getRoom()`, the loaded room or the one in the database.
    let target = target.filter(|t| {
        u32::try_from(t.room_id).is_ok_and(|id| id == room_id || world.room_by_id(id).is_some())
    });
    let Some(target) = target else {
        // Broken link: walk in, then back out.
        let session_id = session.id;
        r.schedule(world, ms(1000), move |_, r| {
            let item = r.mapping.item_index(virtual_id);
            walk_out(r, session_id, item);
        });
        r.schedule(world, ms(2000), move |_, r| {
            set_status(r, virtual_id, TELEPORTER_CLOSE)
        });
        r.schedule(world, ms(2500), move |_, r| {
            if let Some(e) = r.entity_index(session_id) {
                r.entities[e].walking_allowed = true;
            }
        });
        return None;
    };
    let pair = (target.id, target.room_id);
    let same_room = target.room_id == room_id as i32;
    let room = r.handle().map(|h| Arc::downgrade(&h)).unwrap_or_default();

    // Stuck inside the teleporter: go straight on.
    let walker = r.walker(e);
    if item_position == user_position && !is_valid_tile(&r.mapping, Some(&walker), front) {
        r.set_item_status(idx, TELEPORTER_EFFECTS);
        stuck_timers(world, session, room, virtual_id, pair, same_room);
        return Some(pair);
    }

    let (s, rm) = (session.clone(), room.clone());
    schedule(world, ms(1000), move |_| {
        if !is_teleporting(&s) {
            return;
        }
        with_room(&rm, |r| set_status(r, virtual_id, TELEPORTER_EFFECTS));
    });
    let (s, rm) = (session.clone(), room.clone());
    schedule(world, ms(1500), move |_| {
        if !is_teleporting(&s) {
            return;
        }
        with_room(&rm, |r| {
            if let Some(e) = r.entity_index(s.id) {
                r.send(out_user::logout(r.entities[e].instance_id));
            }
            set_status(r, virtual_id, TELEPORTER_CLOSE);
            if same_room {
                warp_to_pair(r, s.id, pair.0);
            }
        });
        if !same_room {
            forward(&s, pair.1);
        }
    });
    if same_room {
        let (s, rm) = (session.clone(), room.clone());
        schedule(world, ms(3000), move |_| {
            if !in_room(&s, room_id) {
                clear(&s);
                return;
            }
            with_room(&rm, |r| {
                let Some(pair_idx) = r.item_by_database_id(pair.0) else {
                    return;
                };
                r.set_item_status(pair_idx, TELEPORTER_OPEN);
                walk_out(r, s.id, Some(pair_idx));
            });
        });
        let (s, rm) = (session.clone(), room);
        schedule(world, ms(4000), move |_| {
            if !in_room(&s, room_id) {
                clear(&s);
                return;
            }
            clear(&s);
            with_room(&rm, |r| {
                if let Some(pair_idx) = r.item_by_database_id(pair.0) {
                    r.set_item_status(pair_idx, TELEPORTER_CLOSE);
                }
                if let Some(e) = r.entity_index(s.id) {
                    r.entities[e].walking_allowed = true;
                }
            });
        });
    }
    Some(pair)
}

/// The timers of a user stuck inside a teleporter (the first branch of Java's
/// `TeleportInteractor.onInteract` after the link checks).
fn stuck_timers(
    world: &World,
    session: &Arc<Session>,
    room: Weak<RoomHandle>,
    virtual_id: i32,
    pair: (u64, i32),
    same_room: bool,
) {
    let (s, rm) = (session.clone(), room.clone());
    schedule(world, ms(1000), move |_| {
        if !is_teleporting(&s) {
            return;
        }
        with_room(&rm, |r| {
            set_status(r, virtual_id, TELEPORTER_CLOSE);
            if let Some(e) = r.entity_index(s.id) {
                r.send(out_user::logout(r.entities[e].instance_id));
            }
        });
    });
    let (s, rm) = (session.clone(), room.clone());
    schedule(world, ms(2000), move |_| {
        if !is_teleporting(&s) {
            return;
        }
        if same_room {
            with_room(&rm, |r| {
                warp_to_pair(r, s.id, pair.0);
            });
        } else {
            forward(&s, pair.1);
        }
    });
    if !same_room {
        return;
    }
    let (s, rm) = (session.clone(), room.clone());
    schedule(world, ms(3000), move |_| {
        if !is_teleporting(&s) {
            return;
        }
        with_room(&rm, |r| {
            let Some(pair_idx) = r.item_by_database_id(pair.0) else {
                return;
            };
            r.set_item_status(pair_idx, TELEPORTER_OPEN);
            walk_out(r, s.id, Some(pair_idx));
            if let Some(e) = r.entity_index(s.id) {
                r.entities[e].walking_allowed = true;
            }
        });
    });
    let s = session.clone();
    schedule(world, ms(4000), move |_| {
        if !is_teleporting(&s) {
            return;
        }
        clear(&s);
        with_room(&room, |r| {
            if let Some(pair_idx) = r.item_by_database_id(pair.0) {
                r.set_item_status(pair_idx, TELEPORTER_CLOSE);
            }
        });
    });
}

/// Java: `roomUser.warp(pairedTeleporter.getPosition(), true, true)` followed by the partner's
/// effects.
fn warp_to_pair(r: &mut Room, session_id: u64, pair_id: u64) {
    let Some(pair_idx) = r.item_by_database_id(pair_id) else {
        return;
    };
    if let Some(e) = r.entity_index(session_id) {
        let position: Position = r.mapping.items[pair_idx].position;
        r.warp(e, position);
    }
    r.set_item_status(pair_idx, TELEPORTER_EFFECTS);
}

/// Java: `roomUser.setAuthenticateId(room)` and `pairedTeleporter.getRoom().forward(player,
/// false)`: the client follows with `TRYFLAT` and `GOTOFLAT`. TODO(publics): teleporters in
/// public rooms (the public room id offset and redirections of `Room.forward`).
fn forward(session: &Session, room_id: i32) {
    let Ok(id) = u32::try_from(room_id) else {
        return;
    };
    session.player.lock().authenticate_id = Some(id);
    session.send(out_messenger::room_forward(false, room_id));
}

/// Java: the teleporter arrival in `RoomEntityManager.silentlyEnterRoom`: the user who came
/// through a teleporter appears on its partner instead of the door, which shows the effects,
/// opens to let them out and closes. Called with the room locked, right after the user joined.
pub fn arrive(world: &World, r: &mut Room, session_id: u64, teleporter_id: u64) {
    let (Some(idx), Some(e)) = (
        r.item_by_database_id(teleporter_id),
        r.entity_index(session_id),
    ) else {
        return;
    };
    let teleporter = &r.mapping.items[idx];
    let (virtual_id, position) = (teleporter.virtual_id, teleporter.position);
    r.entities[e].walking_allowed = false;
    r.entities[e].position = position;
    r.schedule(world, Duration::ZERO, move |_, r| {
        set_status(r, virtual_id, TELEPORTER_EFFECTS)
    });
    r.schedule(world, ms(1000), move |_, r| {
        let idx = r.mapping.item_index(virtual_id);
        if let Some(idx) = idx {
            r.set_item_status(idx, TELEPORTER_OPEN);
        }
        walk_out(r, session_id, idx);
    });
    r.schedule(world, ms(1500), move |_, r| {
        set_status(r, virtual_id, TELEPORTER_CLOSE);
        if let Some(e) = r.entity_index(session_id) {
            r.entities[e].walking_allowed = true;
        }
    });
}
