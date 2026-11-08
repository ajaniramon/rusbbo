//! The Space Cafe's lights and flipboard (Java: `SpaceCafeTrigger` and `tasks/SpaceCafeTask`).
//!
//! The room's first entry starts a task every 500 ms (Java's `SpaceCafeTask`, no initial delay).
//! Every 5 s each of the three lamps (`lmpa`/`liga`, `lmpb`/`ligb`, `lmpc`/`ligc`) takes a random
//! colour, and the room sees those that changed. Every 60 s the flipboard turns its boards over
//! (`flipflop0`-`flipflop8`), one tick apart, until the next still board.

use std::time::Duration;

use rand::RngExt;

use super::Room;
use crate::messages::outgoing::rooms_pool::show_program;

/// Java: `SpaceCafeTask.FLIPBOARD_TIME` and `LIGHT_TIME`, in seconds.
const FLIPBOARD_TIME: i64 = 60;
const LIGHT_TIME: i64 = 5;
/// Java: `FLIPBOARD_ORDER`.
const FLIPBOARD_ORDER: [usize; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
/// Java: the lamps' colours.
const COLOURS: [i32; 3] = [4, 1, 2];
const TICK: Duration = Duration::from_millis(500);

/// Java: `SpaceCafeTask.convertToInterval`: seconds in 500 ms ticks.
fn ticks(seconds: i64) -> i64 {
    seconds * 2
}

/// Java: `SpaceCafeTask`, while it is scheduled in the room.
pub struct SpaceCafe {
    task: Option<tokio::task::AbortHandle>,
    stage_index: usize,
    time_to_next_stage: i64,
    time_to_next_light: i64,
    /// The lamps' colours, 0 until the first change.
    colours: [i32; 3],
}

impl SpaceCafe {
    fn new() -> Self {
        SpaceCafe {
            task: None,
            stage_index: 2,
            time_to_next_stage: ticks(FLIPBOARD_TIME),
            time_to_next_light: ticks(LIGHT_TIME),
            colours: [0; 3],
        }
    }
}

impl Drop for SpaceCafe {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Room {
    /// Java: `SpaceCafeTrigger.onRoomEntry`: the room's first entry by a player starts the task
    /// (replacing one that runs, as `scheduleTask` does).
    pub(super) fn space_cafe_room_entry(&mut self, idx: usize, first_entry: bool) {
        if !first_entry || !self.entities[idx].is_player() {
            return;
        }
        let mut cafe = SpaceCafe::new();
        if let (Some(_), Ok(rt)) = (self.world(), tokio::runtime::Handle::try_current()) {
            let room = self.self_ref.clone();
            let handle = rt.spawn(async move {
                let mut interval = tokio::time::interval(TICK);
                loop {
                    interval.tick().await;
                    let Some(room) = room.upgrade() else {
                        return;
                    };
                    crate::game::contain_panic(format_args!("space cafe"), || {
                        room.lock().space_cafe_tick();
                    });
                }
            });
            cafe.task = Some(handle.abort_handle());
        }
        self.space_cafe = Some(cafe);
    }

    /// Java: `SpaceCafeTask.run`.
    pub(crate) fn space_cafe_tick(&mut self) {
        let Some(cafe) = self.space_cafe.as_mut() else {
            return;
        };
        if cafe.time_to_next_stage > 0 {
            cafe.time_to_next_stage -= 1;
        }
        if cafe.time_to_next_stage == 0 {
            self.flip_boards();
        }
        let Some(cafe) = self.space_cafe.as_mut() else {
            return;
        };
        if cafe.time_to_next_light > 0 {
            cafe.time_to_next_light -= 1;
        }
        if cafe.time_to_next_light == 0 {
            self.change_lights();
        }
    }

    /// Java: `processLightColor`.
    fn change_lights(&mut self) {
        let Some(cafe) = self.space_cafe.as_mut() else {
            return;
        };
        let mut rng = rand::rng();
        let picked: [i32; 3] = std::array::from_fn(|_| COLOURS[rng.random_range(0..COLOURS.len())]);
        let mut messages = Vec::new();
        for (i, (lamp, light)) in [("lmpa", "liga"), ("lmpb", "ligb"), ("lmpc", "ligc")]
            .into_iter()
            .enumerate()
        {
            if picked[i] != cafe.colours[i] {
                cafe.colours[i] = picked[i];
                let colour = picked[i].to_string();
                messages.push(show_program(&[lamp, "ufol", &colour]));
                messages.push(show_program(&[light, "litecol", &colour]));
            }
        }
        cafe.time_to_next_light = ticks(LIGHT_TIME);
        for msg in messages {
            self.send(msg);
        }
    }

    /// Java: `processBoardFlip`.
    fn flip_boards(&mut self) {
        let Some(cafe) = self.space_cafe.as_mut() else {
            return;
        };
        cafe.stage_index += 1;
        if cafe.stage_index >= FLIPBOARD_ORDER.len() {
            cafe.stage_index = 0;
        }
        let previous = cafe.stage_index;
        let next = if previous + 1 >= FLIPBOARD_ORDER.len() {
            0
        } else {
            previous + 1
        };
        let still = matches!((previous, next), (2, 3) | (5, 6) | (8, 0));
        cafe.time_to_next_stage = if still { ticks(FLIPBOARD_TIME) } else { 1 };
        let hide = format!("flipflop{}", FLIPBOARD_ORDER[previous]);
        let show = format!("flipflop{}", FLIPBOARD_ORDER[next]);
        self.send(show_program(&[&hide, "visible", "0"]));
        self.send(show_program(&[&show, "visible", "1"]));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::game::pathfinder::Position;
    use crate::game::room::model::RoomModel;

    /// The bodies of the frames sent (a frame is a 2-byte header, the body and ``).
    fn bodies(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::session::Outgoing>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(crate::session::Outgoing::Frame(f)) = rx.try_recv() {
            out.push(String::from_utf8_lossy(&f[2..f.len() - 1]).into_owned());
        }
        out
    }

    #[test]
    fn lights_then_flipboard() {
        let model = Arc::new(RoomModel::parse(&rusbbo_storage::RoomModel {
            model_id: "space_cafe".into(),
            model_name: "space_cafe".into(),
            heightmap: "000|000".into(),
            trigger_class: "space_cafe_trigger".into(),
            ..Default::default()
        }));
        let mut room = Room::for_test(0, model, Vec::new());
        let (idx, mut rx) = room.add_test_user(Position::new(1, 1));
        room.space_cafe_room_entry(idx, true);
        // Ten ticks (4.5 s after the first): all six lights, since they start at no colour.
        for _ in 0..9 {
            room.space_cafe_tick();
        }
        assert!(bodies(&mut rx).is_empty());
        room.space_cafe_tick();
        let lights = bodies(&mut rx);
        assert_eq!(lights.len(), 6);
        assert!(lights[0].starts_with("lmpa ufol ") && lights[1].starts_with("liga litecol "));
        assert!(lights[5].starts_with("ligc litecol "));
        // The flipboard at the 120th tick, then 4-5 and 5-6 one tick apart, then still.
        for _ in 10..119 {
            room.space_cafe_tick();
        }
        let flips: Vec<String> = bodies(&mut rx)
            .into_iter()
            .filter(|b| b.starts_with("flipflop"))
            .collect();
        assert!(flips.is_empty());
        room.space_cafe_tick();
        room.space_cafe_tick();
        room.space_cafe_tick();
        room.space_cafe_tick();
        let flips: Vec<String> = bodies(&mut rx)
            .into_iter()
            .filter(|b| b.starts_with("flipflop"))
            .collect();
        assert_eq!(
            flips,
            [
                "flipflop3 visible 0",
                "flipflop4 visible 1",
                "flipflop4 visible 0",
                "flipflop5 visible 1",
                "flipflop5 visible 0",
                "flipflop6 visible 1",
            ]
        );
    }
}
