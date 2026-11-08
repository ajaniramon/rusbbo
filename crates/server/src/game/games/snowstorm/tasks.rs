//! The timed work (Java: `snowstorm/tasks/SnowStormGameTask`, `SnowballMovementTask`,
//! `SnowStormTurn`, `util/SnowStormFuture`, and the machine runnables of
//! `SnowStormGameTask.trySnowballMachine`).

use std::collections::VecDeque;

use super::events::TurnEvent;
use super::game::{Output, SnowStormGame};
use super::objects::{ObjectRef, Snowball, Trajectory};
use super::outgoing::snowstorm_gamestatus;
use super::timers::{MACHINE_ANIMATION_MS, MACHINE_RESTOCK_MS, TimerKind};
use super::{MAX_QUICK_THROW_DISTANCE, TURN_MS, TURNS, java_distance};
use crate::game::pathfinder::{Position, walk_direction};

/// Java: `SnowStormFuture`: an event queued `frames_future` turns ahead.
#[derive(Clone, Debug)]
pub struct FutureEvent {
    pub frames_future: i32,
    pub sub_turn: usize,
    pub event: TurnEvent,
}

/// Java: the turn list of `SnowStormGameTask` (5 `SnowStormTurn`s, rebuilt after every send)
/// and its future events.
#[derive(Clone, Debug)]
pub struct TurnTask {
    pub turns: Vec<Vec<TurnEvent>>,
    pub futures: Vec<FutureEvent>,
    pub generation: u64,
}

impl TurnTask {
    pub fn new(generation: u64) -> Self {
        Self {
            turns: vec![Vec::new(); TURNS],
            futures: Vec::new(),
            generation,
        }
    }

    pub fn reset_turns(&mut self) {
        self.turns = vec![Vec::new(); TURNS];
    }

    /// Java: `sendQueue`.
    pub fn send_queue(&mut self, frames_future: i32, sub_turn: usize, event: TurnEvent) {
        if frames_future == 0 {
            if let Some(turn) = sub_turn.checked_sub(1).and_then(|i| self.turns.get_mut(i)) {
                turn.push(event);
            }
            return;
        }
        self.futures.push(FutureEvent {
            frames_future,
            sub_turn,
            event,
        });
    }
}

/// A snowball in the air (Java: `SnowballMovementTask`'s state).
#[derive(Clone, Debug)]
pub struct Flight {
    pub snowball: Snowball,
    pub path: VecDeque<Position>,
    pub thrown_time: i64,
    pub first_position: Position,
    pub last_position: Position,
}

impl SnowStormGame {
    /// Java: `RoomTaskManager.loadGameTasks` scheduling `UpdateTask` (a new
    /// `SnowStormGameTask`, delay 0, every 300 ms). Like Aeyra's guard, a running task is
    /// kept.
    pub fn start_turns(&mut self, now: i64) {
        if self.turns.is_some() {
            return;
        }
        self.turn_generation += 1;
        let generation = self.turn_generation;
        self.turns = Some(TurnTask::new(generation));
        self.timers.schedule(now, TimerKind::Turn { generation });
    }

    /// Java: `RoomTaskManager.stopTasks` (the arena room emptied): the task and the events it
    /// had queued are gone. Snowballs, machines and state resets keep running (they are on
    /// Java's `GameScheduler`, not the room's tasks).
    pub fn stop_turns(&mut self) {
        self.turns = None;
    }

    /// Whether the arena has its turn task (Java: `getUpdateTask() != null`).
    pub fn turns_running(&self) -> bool {
        self.turns.is_some()
    }

    /// Java: `getUpdateTask().getExecutingTurns()`: the turn being built.
    pub fn executing_turns(&self) -> Option<&[Vec<TurnEvent>]> {
        self.turns.as_ref().map(|t| t.turns.as_slice())
    }

    /// When the next timer is due (the core should call [`Self::advance`] then).
    pub fn next_deadline(&self) -> Option<i64> {
        self.timers.next_deadline()
    }

    /// Runs every timer due at or before `now`.
    pub fn advance(&mut self, now: i64) -> Vec<Output> {
        let mut out = Vec::new();
        while let Some((due, kind)) = self.timers.pop_due(now) {
            self.run_timer(due, kind, &mut out);
        }
        out
    }

    fn run_timer(&mut self, due: i64, kind: TimerKind, out: &mut Vec<Output>) {
        match kind {
            TimerKind::Turn { generation } => {
                if self.turns.as_ref().map(|t| t.generation) != Some(generation) {
                    return;
                }
                self.run_turn(due, out);
                self.timers.schedule(due + TURN_MS, kind);
            }
            TimerKind::ActivityReset {
                user_id,
                token,
                then,
            } => self.run_activity_reset(user_id, token, then, due),
            TimerKind::SnowballStep { snowball, period } => {
                if self.snowball_step(snowball, due) {
                    self.timers.schedule(due + period, kind);
                }
            }
            TimerKind::MachineAnimation { machine, .. } => {
                if self.machine_animation(machine) {
                    self.timers.schedule(due + MACHINE_ANIMATION_MS, kind);
                }
            }
            TimerKind::MachineRestock { machine, user_id } => {
                if self.machine_restock(machine, user_id) {
                    self.timers.schedule(due + MACHINE_RESTOCK_MS, kind);
                }
            }
        }
    }

    /// Java: `SnowStormGameTask.run`.
    fn run_turn(&mut self, now: i64, out: &mut Vec<Output>) {
        if self.active().next().is_none() {
            return; // Nothing is sent (or reset) while nobody is in the game
        }

        for i in 0..self.players.len() {
            if self.players[i].in_room {
                out.push(Output::SpamTick(self.players[i].info.user_id));
                self.process_entity(i, now);
            }
        }

        let Some(task) = self.turns.as_mut() else {
            return;
        };
        let ready: Vec<FutureEvent> = task
            .futures
            .iter()
            .filter(|f| f.frames_future == 0)
            .cloned()
            .collect();
        for f in ready {
            task.send_queue(0, f.sub_turn, f.event);
        }
        out.push(Output::Broadcast(snowstorm_gamestatus(&task.turns)));
        task.reset_turns();
        task.futures.retain(|f| f.frames_future != 0);
        for f in &mut task.futures {
            if f.frames_future > 0 {
                f.frames_future -= 1;
            }
        }
    }

    /// Java: `processEntity`: one tile towards the walk goal per turn; on arrival a last move
    /// event, rotation 0 (Java computes the direction from the goal to itself) and the machine
    /// check.
    fn process_entity(&mut self, i: usize, now: i64) {
        if !self.players[i].attrs.walking {
            return;
        }
        let object_id = self.players[i].object_id;
        let [gx, gy] = self.players[i]
            .attrs
            .goal_world_coordinates
            .unwrap_or_default();
        let move_event = TurnEvent::AvatarMove {
            object_id,
            x: gx,
            y: gy,
        };

        let current = self.players[i].attrs.current_position;
        if let Some(goal) = self.players[i].attrs.walk_goal.filter(|&g| g == current) {
            self.send_queue(0, 1, move_event);
            let a = &mut self.players[i].attrs;
            a.rotation = walk_direction(current.x, current.y, goal.x, goal.y);
            a.next_goal = None;
            a.walking = false;
            self.try_snowball_machine(i, now);
            return;
        }

        match self.next_direction(i) {
            Some(next) => {
                let a = &mut self.players[i].attrs;
                a.rotation = walk_direction(current.x, current.y, next.x, next.y);
                a.current_position = next;
                a.next_goal = Some(next);
                self.send_queue(0, 1, move_event);
            }
            None => {
                let a = &mut self.players[i].attrs;
                a.next_goal = None;
                a.walking = false;
            }
        }
    }

    /// Java: `trySnowballMachine`: standing just below a machine's left tile starts its two
    /// runnables (both at once, the animation first). Every arrival starts a new pair; each
    /// stops on its own once nobody (walkable) stands there.
    fn try_snowball_machine(&mut self, i: usize, now: i64) {
        let current = self.players[i].attrs.current_position;
        let machine_tile = Position::new(current.x, current.y - 1);
        let Some(map) = self.map.as_deref() else {
            return;
        };
        let is_machine = map
            .tile(machine_tile)
            .and_then(|t| map.highest_item(t))
            .is_some_and(|item| item.is_snowball_machine());
        if !is_machine {
            return;
        }
        let machine = self
            .objects
            .iter()
            .filter_map(|o| match *o {
                ObjectRef::Machine(m) if self.machines[m].position() == machine_tile => Some(m),
                _ => None,
            })
            .next_back();
        let Some(machine) = machine else {
            return;
        };
        let user_id = self.players[i].info.user_id;
        self.timers
            .schedule(now, TimerKind::MachineAnimation { machine, user_id });
        self.timers
            .schedule(now, TimerKind::MachineRestock { machine, user_id });
    }

    /// Java: `SnowStormMachineObject.isPlayerCollectingSnowballs`: the first active player on
    /// the tile below the machine is walkable.
    fn is_player_collecting(&self, machine: usize) -> bool {
        let m = &self.machines[machine];
        let below = Position::new(m.x, m.y + 1);
        let Some(map) = self.map.as_deref() else {
            return false;
        };
        if map.tile(below).is_none() {
            return false;
        }
        self.active()
            .find(|(_, p)| p.attrs.current_position == below)
            .is_some_and(|(_, p)| p.attrs.is_walkable())
    }

    /// Java: `snowMachineAnimation`. Returns whether it keeps running.
    fn machine_animation(&mut self, machine: usize) -> bool {
        if !self.is_player_collecting(machine) {
            return false;
        }
        if self.machines[machine].snowballs < 5 {
            self.machines[machine].snowballs += 1;
            let machine_id = self.machines[machine].object_id;
            self.send_queue(0, 1, TurnEvent::MachineAddSnowball { machine_id });
        }
        true
    }

    /// Java: `snowMachineRestock`: one ball from the machine to the player who started it.
    fn machine_restock(&mut self, machine: usize, user_id: u32) -> bool {
        if !self.is_player_collecting(machine) {
            return false;
        }
        let receiver = self
            .index_of(user_id)
            .filter(|&i| self.players[i].attrs.snowballs < 5);
        if let (true, Some(i)) = (self.machines[machine].snowballs > 0, receiver) {
            self.players[i].attrs.snowballs += 1;
            let event = TurnEvent::MachineMoveSnowballs {
                player_id: self.players[i].object_id,
                machine_id: self.machines[machine].object_id,
            };
            self.send_queue(0, 1, event);
            self.machines[machine].snowballs -= 1;
        }
        true
    }

    /// Java: `SnowballObject.getPath`.
    pub(super) fn snowball_path(&self, snowball: &Snowball) -> VecDeque<Position> {
        self.max_visibility(
            snowball,
            snowball.from_x,
            snowball.from_y,
            snowball.target_x,
            snowball.target_y,
        )
    }

    /// Java: `SnowballObject.scheduleMovementTask`: a flight stepping through its path over
    /// its time to live; a ball with no time to live (or an unknown trajectory) lands at once,
    /// without a delete event.
    pub(super) fn schedule_movement(&mut self, snowball: Snowball, now: i64) {
        let path = self.snowball_path(&snowball);
        let ttl = snowball.time_to_live();
        if ttl > 0 && !path.is_empty() {
            let period = i64::from(ttl * 100) / path.len() as i64;
            if period <= 0 {
                // Java's scheduleAtFixedRate throws (the handler dies; the ball never lands).
                return;
            }
            let from = snowball.from();
            let id = snowball.object_id;
            let flight = Flight {
                path: self.snowball_path(&snowball),
                thrown_time: now + i64::from(ttl) * 100,
                first_position: from,
                last_position: from,
                snowball,
            };
            self.flights.insert(id, flight);
            self.timers.schedule(
                now,
                TimerKind::SnowballStep {
                    snowball: id,
                    period,
                },
            );
        } else {
            self.handle_snowball_landing(&snowball, false, now);
        }
    }

    /// Java: `SnowballMovementTask.tick`. Returns whether the flight goes on.
    fn snowball_step(&mut self, id: i32, now: i64) -> bool {
        let Some(mut f) = self.flights.remove(&id) else {
            return false;
        };
        let trajectory = f.snowball.trajectory();
        if trajectory != Some(Trajectory::Long) {
            match f.path.pop_front() {
                Some(next) => {
                    f.last_position = next;
                    if self.is_blocked_tile(&f.snowball, next) {
                        f.snowball.target_x = next.x;
                        f.snowball.target_y = next.y;
                        f.snowball.blocked = true;
                        self.handle_snowball_landing(&f.snowball, true, now);
                        return false;
                    }
                }
                None => {
                    if matches!(trajectory, Some(Trajectory::Quick | Trajectory::Long))
                        && self.continue_quick_throw(&mut f, now)
                    {
                        f.path = self.snowball_path(&f.snowball);
                        self.flights.insert(id, f);
                        return true;
                    }
                    self.handle_snowball_landing(&f.snowball, true, now);
                    return false;
                }
            }
        }

        if now > f.thrown_time {
            if trajectory == Some(Trajectory::Quick) && self.continue_quick_throw(&mut f, now) {
                f.path = self.snowball_path(&f.snowball);
            } else {
                self.handle_snowball_landing(&f.snowball, true, now);
                return false;
            }
        }
        self.flights.insert(id, f);
        true
    }

    /// Java: `continueQuickThrowSnowball`: if the target still stands (damageable) on the
    /// straight line from the throw in its direction, closer than 22 to where it was thrown
    /// from, the ball flies on from where it is to the target's tile.
    fn continue_quick_throw(&self, f: &mut Flight, now: i64) -> bool {
        let Some(map) = self.map.as_deref() else {
            return false;
        };
        let Some(target_player) = f.snowball.target_player else {
            // Java: a NullPointerException, which ends the task without landing. Only an attack
            // (which always has a target) throws quick.
            return false;
        };
        let mut position = Position::new(f.snowball.from_x, f.snowball.from_y);
        position.set_rotation(f.snowball.direction);
        while map.tile(position).is_some() {
            let found = self.active().find(|(_, p)| {
                p.attrs.current_position == position
                    && p.info.user_id == target_player
                    && p.attrs.is_damageable(now)
            });
            if let Some((_, p)) = found {
                let target = p.attrs.current_position;
                if java_distance(target, f.first_position) >= MAX_QUICK_THROW_DISTANCE {
                    return false;
                }
                f.snowball.from_x = f.last_position.x;
                f.snowball.from_y = f.last_position.y;
                f.snowball.target_x = target.x;
                f.snowball.target_y = target.y;
                f.thrown_time = now + i64::from(f.snowball.time_to_live()) * 100;
                return true;
            }
            position = position.square_in_front();
        }
        false
    }
}
