//! Java: `messages/incoming/rooms/idol` (the American Idol performance, see
//! `game::item::interactors::idol`).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use crate::game::room::current_room;
use crate::messages::Malformed;
use crate::session::Session;
use crate::world::World;

/// Java: `START_PERFORMANCE` (VL64 id of the disc to sing, TODO(sound)): from the performer.
pub fn start_performance(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    if r.idol.performer.map(|(s, _)| s) != Some(session.id) {
        return Ok(());
    }
    let _disc = reader.int().ok_or(Malformed)?;
    r.start_performance(session.id);
    Ok(())
}

/// Java: `VOTE_PERFORMANCE` (VL64 boolean): a judge on a vote chair votes.
pub fn vote_performance(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    let Some(e) = r.entity_index(session.id) else {
        return Ok(());
    };
    // The chair and the scoreboard are checked again in `vote_performance`, before the vote
    // is read, as in Java.
    let on_chair = r
        .current_item(e)
        .is_some_and(|i| i.interaction == crate::game::room::items::Interaction::IdolVoteChair);
    if !on_chair || !r.has_idol_scoreboard() {
        return Ok(());
    }
    let positive = reader.bool().ok_or(Malformed)?;
    r.vote_performance(e, positive);
    Ok(())
}
