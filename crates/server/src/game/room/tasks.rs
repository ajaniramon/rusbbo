//! The room tick (Java: `game/room/tasks/EntityTask` every 500 ms and `StatusTask` every 1 s,
//! both scheduled by `RoomTaskManager.startTasks` with no initial delay), and the room part of
//! the 1 s game tick (Java: `GameScheduler`'s sleep and AFK checks).
//!
//! Both room tasks run in one tokio task: every 500 ms the entity pass runs, and every other
//! time (starting with the first) the status pass runs right after it. The phase matters: the
//! traces end steps on quiet time, so statuses must go out on the same 500 ms grid, counted from
//! the moment the room was initialised, as in Java. The first run happens inside
//! `Room::try_initialise` (see there), so the task starts with the second one.

use std::sync::Weak;
use std::time::Duration;

use havana_protocol::Message;

use crate::game::now;
use crate::game::pathfinder::{Position, walk_direction};
use crate::game::room::entity::{RoomEntity, StatusType};
use crate::game::room::mapping::is_valid_tile;
use crate::game::room::{Room, RoomHandle, run_deferred};
use crate::messages::outgoing::rooms_user as out_user;
use crate::world::World;

/// Java: `EntityTask` period.
pub const ENTITY_TICK: Duration = Duration::from_millis(500);

pub async fn run(world: Weak<World>, room: Weak<RoomHandle>) {
    let start = tokio::time::Instant::now() + ENTITY_TICK;
    let mut interval = tokio::time::interval_at(start, ENTITY_TICK);
    let mut count: u64 = 1;
    loop {
        interval.tick().await;
        let (Some(world), Some(room)) = (world.upgrade(), room.upgrade()) else {
            return;
        };
        let deferred = {
            let mut r = room.lock();
            r.entity_tick(&world);
            if count % 2 == 0 {
                r.status_tick();
            }
            r.take_deferred()
        };
        run_deferred(&world, deferred);
        count += 1;
    }
}

/// An entity that left the room while the entity pass processed it. Java keeps iterating its
/// snapshot, sees `needsUpdate` set by `reset()` and includes it in the statuses it sends, with
/// instance id -1; so does this port.
enum Updated {
    Present(i32),
    Left(Box<RoomEntity>),
}

impl Room {
    /// Java: `EntityTask.run`.
    pub fn entity_tick(&mut self, world: &World) {
        if let Some(msg) = self.entity_pass(world) {
            self.send(msg);
        }
    }

    /// The entity pass of [`Room::entity_tick`]: moves everyone a step and returns the
    /// `USER_STATUSES` of those who changed, without sending it.
    pub fn entity_pass(&mut self, world: &World) -> Option<Message> {
        if self.entities.is_empty() {
            return None;
        }
        let moonwalk = world.settings.read().bool("april.fools");
        let snapshot: Vec<i32> = self.entities.iter().map(|e| e.instance_id).collect();
        let mut updated = Vec::new();
        for instance_id in snapshot {
            let Some(idx) = self.entity_by_instance(instance_id) else {
                continue;
            };
            self.entities[idx].handle_spam_ticks();
            match self.process_entity(world, idx, moonwalk) {
                Some(mut left) => {
                    if left.needs_update {
                        left.needs_update = false;
                        updated.push(Updated::Left(Box::new(left)));
                    }
                }
                None => {
                    if let Some(idx) = self.entity_by_instance(instance_id) {
                        if self.entities[idx].needs_update {
                            self.entities[idx].needs_update = false;
                            updated.push(Updated::Present(instance_id));
                        }
                    }
                }
            }
        }
        // TODO(items): EntityTask.queueAfterLoop (avatar effects re-sent after sitting).
        if updated.is_empty() {
            return None;
        }
        let entities: Vec<&RoomEntity> = updated
            .iter()
            .filter_map(|u| match u {
                Updated::Present(id) => self.entity_by_instance(*id).map(|i| &self.entities[i]),
                Updated::Left(e) => Some(&**e),
            })
            .collect();
        Some(out_user::user_statuses(entities, &self.model.name))
    }

    /// Java: `EntityTask.processEntity`. Returns the entity if it left the room.
    fn process_entity(&mut self, world: &World, idx: usize, moonwalk: bool) -> Option<RoomEntity> {
        if !self.entities[idx].is_walking {
            return None;
        }
        if let Some(next) = self.entities[idx].next_position {
            let e = &mut self.entities[idx];
            e.position.x = next.x;
            e.position.y = next.y;
            self.update_new_height(idx);
            // TODO(items): the current item's onEntityStep trigger.
        }

        let Some(next) = self.entities[idx].path.pop_front() else {
            let left = self.stop_walking(world, idx);
            if let Some(mut left) = left {
                // Java sets needsUpdate after stopWalking, on the (reset) entity.
                left.needs_update = true;
                return Some(left);
            }
            self.entities[idx].needs_update = true;
            return None;
        };

        // The tile became invalid since the path was made: stop, then walk again.
        let walker = self.walker(idx);
        if !is_valid_tile(&self.mapping, Some(&walker), next) || self.mapping.tile(next).is_none() {
            self.entities[idx].path.clear();
            if let Some(left) = self.process_entity(world, idx, moonwalk) {
                return Some(left);
            }
            if let Some(goal) = self.entities[idx].goal {
                self.walk_to(idx, goal.x, goal.y);
            }
            return None;
        }

        let (position, instance_id) = {
            let e = &self.entities[idx];
            (e.position, e.instance_id)
        };
        self.mapping.remove_entity(position, instance_id);
        self.mapping.add_entity(next, instance_id);
        let next_tile = self.mapping.tile(next).expect("checked above");
        let new_height = self.mapping.walking_height(next_tile);
        let old_height = position.z;

        let model = self.model.name.as_str();
        if model.starts_with("pool_") || model == "md_a" {
            let min_difference = if model == "md_a" { 2.0 } else { 3.0 };
            let e = &mut self.entities[idx];
            if new_height > old_height && (new_height - old_height).abs() >= min_difference {
                if e.statuses.contains(StatusType::Swim) {
                    e.statuses.remove(StatusType::Swim);
                    e.statuses.remove(StatusType::Dance);
                }
                e.statuses.remove(StatusType::CarryDrink);
                e.statuses.remove(StatusType::CarryItem);
            }
            if new_height < old_height && (old_height - new_height).abs() >= min_difference {
                if !e.statuses.contains(StatusType::Swim) {
                    e.statuses.set(StatusType::Swim, "");
                    e.statuses.remove(StatusType::Dance);
                }
                e.statuses.remove(StatusType::CarryDrink);
                e.statuses.remove(StatusType::CarryItem);
            }
        }

        let e = &mut self.entities[idx];
        if e.last_item_interaction.is_some() {
            // TODO(items): the last item's onEntityLeave trigger.
            e.last_item_interaction = None;
        }
        e.statuses.remove(StatusType::Lay);
        e.statuses.remove(StatusType::Sit);
        let rotation = if moonwalk {
            walk_direction(next.x, next.y, position.x, position.y)
        } else {
            walk_direction(position.x, position.y, next.x, next.y)
        };
        e.position.set_rotation(rotation);
        e.statuses.set(
            StatusType::Move,
            format!(
                "{},{},{}",
                next.x,
                next.y,
                crate::game::format_decimal(new_height)
            ),
        );
        e.next_position = Some(next);
        e.needs_update = true;
        None
    }

    /// Java: `StatusTask.run`.
    pub fn status_tick(&mut self) {
        let t = now();
        for idx in 0..self.entities.len() {
            // Java: processHeadRotation.
            let e = &mut self.entities[idx];
            if e.look_timer != -1 && t > e.look_timer {
                e.look_timer = -1;
                e.position.head_rotation = e.position.body_rotation;
                e.needs_update = true;
            }
            self.process_pool_queue(idx);
            // Java: processChatBubble.
            let e = &mut self.entities[idx];
            if e.chat_bubble_timer != -1 && t > e.chat_bubble_timer {
                e.is_typing = false;
                e.chat_bubble_timer = -1;
                let msg = out_user::typing_status(e.instance_id, false);
                self.send(msg);
            }
            let e = &mut self.entities[idx];
            if e.statuses.tick() {
                e.needs_update = true;
            }
        }
    }

    /// Java: `StatusTask.processPoolQueue`: the Lido diving queue walks people forward.
    fn process_pool_queue(&mut self, idx: usize) {
        let e = &self.entities[idx];
        if e.details.tickets == 0 || e.details.pool_figure.is_empty() {
            return;
        }
        if self.model.name != "pool_b" || e.is_walking {
            return;
        }
        let Some(item) = self.current_item(idx) else {
            return;
        };
        if item.sprite.contains("queue_tile2") {
            let front: Position = item.position.square_in_front();
            self.walk_to(idx, front.x, front.y);
        }
    }

    /// Java: `GameScheduler`'s per-player checks: fall asleep after `sleep.timer.seconds` and
    /// leave after `afk.timer.seconds` without talking or waving.
    pub fn afk_tick(&mut self, world: &World) {
        let t = now();
        let snapshot: Vec<i32> = self.entities.iter().map(|e| e.instance_id).collect();
        for instance_id in snapshot {
            let Some(idx) = self.entity_by_instance(instance_id) else {
                continue;
            };
            if t > self.entities[idx].sleep_timer && !self.entities[idx].is_sleeping {
                // TODO(carry): stopCarrying.
                self.sleep(idx, true);
            }
            if t > self.entities[idx].afk_timer {
                self.kick(world, idx, true, false);
            }
        }
    }
}
