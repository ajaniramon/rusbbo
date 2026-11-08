//! Java: `messages/incoming/infobus` (the rest is `game::infobus`).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use crate::game::infobus;
use crate::game::room::{current_room, run_deferred};
use crate::messages::Malformed;
use crate::session::Session;
use crate::world::World;

/// Java: `TRYBUS`: in a public room, towards the bus's door if it is open.
pub fn try_bus(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let deferred = {
        let mut r = room.lock();
        if !r.is_public() {
            return Ok(());
        }
        if let Some(idx) = r.entity_index(session.id) {
            infobus::try_bus(world, &mut r, idx);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `CHANGEWORLD`: in a public room, towards 11,2 (the bus's exit).
pub fn change_world(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let deferred = {
        let mut r = room.lock();
        if !r.is_public() {
            return Ok(());
        }
        if let Some(idx) = r.entity_index(session.id) {
            r.walk_to_player(world, idx, 11, 2);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `VOTE`: an answer to the Infobus's poll, from inside the bus.
pub fn vote(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(user_id) = session.player.lock().user_id() else {
        return Ok(());
    };
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    if room.model_name != "park_b" || world.infobus.current_poll().is_none() {
        return Ok(());
    }
    let choice = reader.int().ok_or(Malformed)?;
    infobus::vote(world, user_id, choice);
    Ok(())
}
