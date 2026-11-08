//! Java: `messages/incoming/rooms/user/IIM`: a command to the game-hall game at the player's
//! table (`game::gamehall`).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use super::latin1;
use crate::game::room::{current_room, run_deferred};
use crate::messages::Malformed;
use crate::session::Session;
use crate::world::World;

/// Java: `IIM`: the raw body is `<game id> <command> <args>`, for the game of the item the
/// player stands on. (Java reads it in the JVM's default charset; game commands are ASCII.)
pub fn iim(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let contents = latin1(reader.remaining());
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let deferred = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return Ok(());
        };
        let position = r.entities[idx].position;
        let Some(item) = r.mapping.tile(position).and_then(|t| t.highest_item) else {
            return Ok(());
        };
        r.game_command(world, session, item, &contents);
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}
