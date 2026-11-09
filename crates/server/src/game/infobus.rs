//! The Infobus (Java: `game/infobus/InfobusManager` and the model triggers `InfobusParkTrigger`
//! and `InfobusPollTrigger`).
//!
//! The bus stands in the park (`park_a`, The Park); its door tile (28,4) is a walkway into the
//! bus (`park_b`, The Infobus). While the door is closed nobody may walk onto it, and `TRYBUS`
//! says so. Staff open and close it, ask a poll and end the event through the website's
//! housekeeping, which sends the RCON messages `infobusdoorstatus`, `infobuspoll` and
//! `infobusendevent` (see `crate::rcon`); the polls themselves (`infobus_polls`) are made there
//! too. A poll's question goes to everyone in the bus who has not answered it; 30 s later the bus
//! sees the results, and from then on every new answer shows them again.
//!
//! `InfobusPollTrigger` does nothing (its body is commented out in Java), and neither trigger
//! does anything when a user leaves.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use rusbbo_storage::InfobusPoll;
use tracing::error;

use crate::game::pathfinder::Position;
use crate::game::room::{Room, RoomRef, run_deferred};
use crate::messages::outgoing::infobus as out;
use crate::world::World;

/// Java: `InfobusManager.getDoorX/getDoorY`.
const DOOR: (i32, i32) = (28, 4);
/// Java: `startPolling`'s timer before the results.
const POLL_TIME: Duration = Duration::from_secs(30);
const CLOSED_TEXT: &str =
    "The Infobus is closed, there is no event right now. Please check back later.";
const ENDED_TEXT: &str =
    "The Infobus event has now ended. Please check the site for updates in future.";

/// The bus's door tile in the park.
pub fn door_position() -> Position {
    Position::new(DOOR.0, DOOR.1)
}

/// Java: `InfobusManager`'s state. Never held while a room or a player is locked.
#[derive(Default)]
pub struct InfobusManager {
    /// Java: `isDoorOpen`; the park's room mapping reads it too.
    pub door_open: Arc<AtomicBool>,
    state: Mutex<PollState>,
}

#[derive(Default)]
struct PollState {
    /// Java: `canUpdateResults`: the results were shown, so every answer shows them again.
    can_update_results: bool,
    current_poll: Option<InfobusPoll>,
}

impl InfobusManager {
    pub fn is_door_open(&self) -> bool {
        self.door_open.load(Ordering::Relaxed)
    }

    /// Java: `getCurrentPoll`.
    pub fn current_poll(&self) -> Option<InfobusPoll> {
        self.state.lock().current_poll.clone()
    }

    pub fn can_update_results(&self) -> bool {
        self.state.lock().can_update_results
    }
}

/// Java: `RoomManager.getRoomByModel`: a loaded room of that model.
fn room_by_model(world: &World, model: &str) -> Option<RoomRef> {
    world
        .loaded_rooms()
        .into_iter()
        .find(|r| r.model_name == model)
}

/// Java: `InfobusManager.updateDoorStatus`: the park sees the door open or close.
pub fn update_door_status(world: &World, open: bool) {
    world.infobus.door_open.store(open, Ordering::Relaxed);
    if let Some(park) = room_by_model(world, "park_a") {
        park.lock().send(out::bus_door(open));
    }
}

/// Java: `InfobusManager.stopEvent`: everyone in the bus is told and walks out of it (they hear
/// it again in the park), the door closes and the poll is over.
pub fn stop_event(world: &World) {
    if let Some(bus) = room_by_model(world, "park_b") {
        let sessions: Vec<_> = {
            let r = bus.lock();
            r.entities
                .iter()
                .filter(|e| e.is_player())
                .map(|e| e.session.clone())
                .collect()
        };
        for session in sessions {
            let msg = out::cannot_enter_bus(ENDED_TEXT);
            session.send(msg.clone());
            session
                .player
                .lock()
                .packet_queue_after_room_leave
                .push(msg);
            let deferred = {
                let mut r = bus.lock();
                if let Some(idx) = r.entity_index(session.id) {
                    r.kick(world, idx, false, true);
                    // Java: `setBeingKicked(false)` right after, so they take the bus's walkway
                    // back to the park instead of leaving to the hotel view.
                    if let Some(idx) = r.entity_index(session.id) {
                        r.entities[idx].being_kicked = false;
                    }
                }
                r.take_deferred()
            };
            run_deferred(world, deferred);
        }
    }
    update_door_status(world, false);
    world.infobus.state.lock().current_poll = None;
}

/// Java: `InfobusManager.startPolling`: the poll's question for everyone in the bus who has not
/// answered it, and its results 30 s later.
pub fn start_polling(world: &World, poll_id: u32) {
    let poll = match world.store.infobus_poll(poll_id) {
        Ok(poll) => poll,
        Err(e) => {
            error!(poll = poll_id, "loading the infobus poll: {e}");
            None
        }
    };
    {
        let mut state = world.infobus.state.lock();
        state.can_update_results = false;
        state.current_poll = poll.clone();
    }
    let Some(poll) = poll else {
        return;
    };
    let Some(bus) = room_by_model(world, "park_b") else {
        return;
    };
    {
        let r = bus.lock();
        for e in r.entities.iter().filter(|e| e.is_player()) {
            if !world
                .store
                .has_infobus_answer(poll.id, e.details.user_id)
                .unwrap_or(false)
            {
                e.session
                    .send(out::poll_question(&poll.question, &poll.answers));
            }
        }
    }
    // Java's timer reads the current poll when it fires: none (the event ended) shows nothing.
    crate::game::scheduler::schedule(world, POLL_TIME, |world| {
        if let Some(poll) = world.infobus.current_poll() {
            show_poll_results(world, poll.id);
        }
    });
}

/// Java: `InfobusManager.showPollResults`: the bus sees the votes of the poll so far.
pub fn show_poll_results(world: &World, poll_id: u32) {
    let Ok(Some(poll)) = world.store.infobus_poll(poll_id) else {
        return;
    };
    world.infobus.state.lock().can_update_results = true;
    let Some(bus) = room_by_model(world, "park_b") else {
        return;
    };
    let votes = world.store.infobus_answers(poll.id).unwrap_or_default();
    let total = votes.values().sum();
    let msg = out::vote_results(&poll.question, &poll.answers, &votes, total);
    bus.lock().send(msg);
}

/// Java: `VOTE`: the user's answer (1 = the first) to the current poll, once.
pub fn vote(world: &World, user_id: u32, choice: i32) {
    let Some(poll) = world.infobus.current_poll() else {
        return;
    };
    if choice <= 0 || choice as usize > poll.answers.len() {
        return;
    }
    match world.store.has_infobus_answer(poll.id, user_id) {
        Ok(false) => {}
        Ok(true) => return,
        Err(e) => {
            error!(poll = poll.id, "reading infobus answers: {e}");
            return;
        }
    }
    if let Err(e) = world.store.add_infobus_answer(poll.id, choice - 1, user_id) {
        error!(
            poll = poll.id,
            user = user_id,
            "saving an infobus answer: {e}"
        );
        return;
    }
    if world.infobus.can_update_results() {
        show_poll_results(world, poll.id);
    }
}

/// Java: `InfobusParkTrigger.onRoomEntry` (`G_STAT`): the door's state. What the bus queued for
/// the player is sent by `G_STAT` right after (see `messages::incoming::rooms::g_stat`).
impl Room {
    pub(crate) fn infobus_park_room_entry(&mut self, world: &World, idx: usize) {
        let e = &self.entities[idx];
        if e.is_player() {
            e.session.send(out::bus_door(world.infobus.is_door_open()));
        }
    }
}

/// Java: `TRYBUS`: towards the bus's door, if it is open.
pub fn try_bus(world: &World, room: &mut Room, idx: usize) {
    if !world.infobus.is_door_open() {
        room.entities[idx]
            .session
            .send(out::cannot_enter_bus(CLOSED_TEXT));
        return;
    }
    let door = door_position();
    room.walk_to_player(world, idx, door.x, door.y);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::pathfinder::Walker;
    use crate::game::room::mapping::{RoomMapping, is_valid_tile};
    use crate::game::room::model::RoomModel;

    #[test]
    fn the_closed_door_tile_is_not_walkable() {
        let model = Arc::new(RoomModel::parse(&rusbbo_storage::RoomModel {
            model_id: "park_a".into(),
            model_name: "park_a".into(),
            door_x: 2,
            door_y: 8,
            heightmap: vec!["0".repeat(30); 10].join("|"),
            trigger_class: "infobus_park".into(),
            ..Default::default()
        }));
        let mut map = RoomMapping::new(model, true);
        let door = Arc::new(AtomicBool::new(false));
        map.bus_door = Some(door.clone());
        map.regenerate(&[]);
        let walker = Walker {
            flash: false,
            pet_nest: None,
            instance_id: 0,
            position: Position::new(27, 5),
            swimming: false,
            has_pool_figure: false,
        };
        assert!(!is_valid_tile(&map, Some(&walker), door_position()));
        assert!(is_valid_tile(&map, None, door_position()));
        assert!(is_valid_tile(&map, Some(&walker), Position::new(28, 5)));
        door.store(true, Ordering::Relaxed);
        assert!(is_valid_tile(&map, Some(&walker), door_position()));
    }

    #[test]
    fn votes_count_once_and_only_for_the_current_poll() {
        let dir = tempfile::tempdir().unwrap();
        let store = rusbbo_storage::Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        w.put_infobus_poll(&InfobusPoll {
            id: 1,
            question: "Q?".into(),
            answers: vec!["A".into(), "B".into(), "C".into()],
            ..Default::default()
        })
        .unwrap();
        w.commit().unwrap();
        let world = World::new(store).unwrap().into_arc();
        vote(&world, 5, 1);
        assert!(
            world.store.infobus_answers(1).unwrap().is_empty(),
            "no poll yet"
        );

        start_polling(&world, 1);
        assert_eq!(world.infobus.current_poll().map(|p| p.id), Some(1));
        for choice in [0, 4, 2, 3] {
            vote(&world, 5, choice);
        }
        vote(&world, 6, 3);
        let votes = world.store.infobus_answers(1).unwrap();
        assert_eq!(votes.into_iter().collect::<Vec<_>>(), [(1, 1), (2, 1)]);
        assert!(!world.infobus.can_update_results());
        show_poll_results(&world, 1);
        assert!(world.infobus.can_update_results());

        // A poll that does not exist ends the current one; so does the end of the event.
        start_polling(&world, 2);
        assert!(world.infobus.current_poll().is_none());
        start_polling(&world, 1);
        assert!(!world.infobus.can_update_results());
        stop_event(&world);
        assert!(world.infobus.current_poll().is_none() && !world.infobus.is_door_open());
    }
}
