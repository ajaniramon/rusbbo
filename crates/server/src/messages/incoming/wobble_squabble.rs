//! Java: `messages/incoming/wobblesquabble` (the room side is `game::room::wobble_squabble`).

use std::sync::Arc;

use havana_protocol::body::ClientReader;

use crate::game::room::current_room;
use crate::messages::Malformed;
use crate::session::Session;
use crate::world::World;

/// Java: `PTM`: a Wobble Squabble player's move for the next game tick.
pub fn ptm(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(user_id) = session.player.lock().details.as_ref().map(|d| d.id) else {
        return Ok(());
    };
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    // Java: isPlaying, then the move is read.
    if !r.is_playing_wobble_squabble(user_id) {
        return Ok(());
    }
    let move_id = reader.int().ok_or(Malformed)?;
    r.wobble_move(user_id, move_id);
    Ok(())
}
