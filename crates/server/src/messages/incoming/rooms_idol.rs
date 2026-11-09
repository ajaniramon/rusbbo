//! Java: `messages/incoming/rooms/idol` (the American Idol performance, see
//! `game::item::interactors::idol`).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use crate::game::room::current_room;
use crate::messages::Malformed;
use crate::session::Session;
use crate::world::World;

/// Java: `START_PERFORMANCE` (VL64 id of a song disc in the hand): from the performer. The disc's
/// song (`JukeboxDao.getSongIdByItem`, `SongCache.get`) plays; any other id plays nothing.
pub fn start_performance(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    if room.lock().idol.performer.map(|(s, _)| s) != Some(session.id) {
        return Ok(());
    }
    let disc = reader.int().ok_or(Malformed)?;
    // The hand is read before the room is locked (player before room).
    let disc_item = session
        .player
        .lock()
        .inventory
        .item(disc)
        .map(|i| i.record.id as i64);
    let song = disc_item
        .and_then(|item_id| world.store.disk_by_item(item_id).ok().flatten())
        .and_then(|d| u32::try_from(d.song_id).ok())
        .filter(|&id| world.store.song(id).ok().flatten().is_some());
    room.lock().start_performance(session.id, song);
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
    let deferred = r.take_deferred();
    drop(r);
    crate::game::room::run_deferred(world, deferred);
    Ok(())
}
