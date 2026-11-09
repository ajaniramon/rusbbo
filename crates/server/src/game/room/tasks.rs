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

use rusbbo_protocol::Message;

use crate::game::item::roller;
use crate::game::now;
use crate::game::pathfinder::{Position, walk_direction};
use crate::game::room::entity::{RoomEntity, StatusType};
use crate::game::room::items::Behaviour;
use crate::game::room::mapping::is_valid_tile;
use crate::game::room::{Room, RoomHandle, run_deferred};
use crate::messages::outgoing::rooms_items::{self as out_items, Slide};
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
        let tick = count;
        crate::game::contain_panic(format_args!("room tick"), || {
            let deferred = {
                let mut r = room.lock();
                r.entity_tick(&world);
                if tick % 2 == 0 {
                    // Java: StatusTask runs every pet's PetTask.
                    r.pet_tick(&world);
                    r.status_tick();
                    r.bot_tick(&world);
                }
                r.take_deferred()
            };
            run_deferred(&world, deferred);
        });
        count += 1;
    }
}

/// Java: the arena's game task (`BattleBallTask` every 500 ms, `SnowStormGameTask` every 300 ms),
/// instead of the room's entity and status tasks; the first run is right away (Java schedules it
/// with no delay), once the entry that started it released the room's lock.
pub async fn run_arena(world: Weak<World>, room: Weak<RoomHandle>, period: Duration) {
    let mut interval = tokio::time::interval(period);
    let mut first = true;
    loop {
        interval.tick().await;
        let (Some(world), Some(room)) = (world.upgrade(), room.upgrade()) else {
            return;
        };
        let is_first = std::mem::replace(&mut first, false);
        crate::game::contain_panic(format_args!("arena tick"), || {
            let deferred = {
                let mut r = room.lock();
                let Some(uid) = r.arena.as_ref().map(|a| a.game_uid) else {
                    return;
                };
                world.games.run(&world, Some(&mut r), |games, op| {
                    let room = op.room.take();
                    if let Some(room) = room {
                        games.arena_tick(uid, room, is_first, op);
                        op.room = Some(room);
                    }
                });
                r.take_deferred()
            };
            run_deferred(&world, deferred);
        });
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
        if let Some(msgs) = self.entity_pass(world) {
            self.send_pair(msgs);
        }
        // Java: handleMessageQueue, the messages queued for after the loop.
        for msg in std::mem::take(&mut self.queue_after_loop) {
            self.send(msg);
        }
    }

    /// The entity pass of [`Room::entity_tick`]: moves everyone a step and returns the
    /// `USER_STATUSES` of those who changed, without sending it.
    pub fn entity_pass(&mut self, world: &World) -> Option<[Message; 2]> {
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
        Some([
            out_user::user_statuses(entities.iter().copied(), &self.model.name, false),
            out_user::user_statuses(entities, &self.model.name, true),
        ])
    }

    /// Java: `EntityTask.processEntity`. Returns the entity if it left the room.
    fn process_entity(&mut self, world: &World, idx: usize, moonwalk: bool) -> Option<RoomEntity> {
        if !self.entities[idx].is_walking {
            return None;
        }
        // Java's local `position` is the entity's position object, moved onto the next tile
        // here. A trigger that warps the entity (a pool ladder) replaces that object, and the
        // rest of the step still reads the old one: it gives the rotation and the old height.
        let mut stepped = self.entities[idx].position;
        if let Some(next) = self.entities[idx].next_position {
            let e = &mut self.entities[idx];
            let old = e.position;
            e.position.x = next.x;
            e.position.y = next.y;
            self.update_new_height(idx);
            stepped = self.entities[idx].position;
            let instance_id = self.entities[idx].instance_id;
            self.on_entity_step(idx, old);
            // A trigger that stops the walk could, in principle, have taken the entity out.
            if self.entity_by_instance(instance_id) != Some(idx) {
                return None;
            }
        }

        let Some(next) = self.entities[idx].path.pop_front() else {
            let left = self.stop_walking(world, idx);
            if let Some(mut left) = left {
                // Java sets needsUpdate after stopWalking, on the (reset) entity.
                left.needs_update = true;
                return Some(left);
            }
            // Java: an entity that took a walkway is already in the other room, so this room's
            // statuses do not reach it. TODO(walkways): Java sends the others here its status as
            // it is in the new room.
            let e = &mut self.entities[idx];
            e.needs_update = !e.leaving_by_walkway;
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
        let position = stepped;
        let old_height = position.z;

        let model = self.model.name.as_str();
        if super::pool::is_pool_model(model) {
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
        if let Some((_, interaction)) = e.last_item_interaction.take() {
            // Java passes the item the entity stands on, while walking.
            let (session, is_player) = (e.session.clone(), e.is_player());
            let current = self
                .mapping
                .tile(self.entities[idx].position)
                .and_then(|t| t.highest_item);
            self.on_entity_leave(interaction, current, &session, is_player, true);
        }
        let e = &mut self.entities[idx];
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
        let world = self.world();
        for idx in 0..self.entities.len() {
            // Java: processHeadRotation.
            let e = &mut self.entities[idx];
            if e.look_timer != -1 && t > e.look_timer {
                e.look_timer = -1;
                e.position.head_rotation = e.position.body_rotation;
                e.needs_update = true;
            }
            self.process_pool_queue(world.as_deref(), idx);
            // Java: processChatBubble.
            let e = &mut self.entities[idx];
            if e.chat_bubble_timer != -1 && t > e.chat_bubble_timer {
                e.is_typing = false;
                e.chat_bubble_timer = -1;
                let msg = out_user::typing_status(e.instance_id, false);
                self.send(msg);
            }
            let e = &mut self.entities[idx];
            let (changed, refresh_user) = e.statuses.tick();
            if changed {
                e.needs_update = true;
            }
            if refresh_user {
                self.refresh_user(idx);
            }
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
            if !self.entities[idx].is_player() {
                continue;
            }
            if t > self.entities[idx].sleep_timer && !self.entities[idx].is_sleeping {
                self.stop_carrying(idx);
                self.sleep(idx, true);
            }
            if t > self.entities[idx].afk_timer {
                self.kick(world, idx, true, false);
            }
        }
    }
}

/// One roller's share of a roll (Java: `RollerEntry`): the items and the user it moves.
struct RollerEntry {
    roller: usize,
    items: Vec<usize>,
    entity: Option<i32>,
}

/// Java: `RollerTask`, every `roller.tick.default` ms in private rooms, starting 250 ms after
/// the room's tasks: each roller moves the items on it, and the first user on it, one square on
/// (`SLIDEOBJECTBUNDLE`); `RollerCompleteTask` finishes the roll 800 ms later.
pub async fn run_rollers(world: Weak<World>, room: Weak<RoomHandle>) {
    let period = match world.upgrade() {
        Some(w) => w.settings.read().int("roller.tick.default").max(1) as u64,
        None => return,
    };
    let period = Duration::from_millis(period);
    let start = tokio::time::Instant::now() + Duration::from_millis(250);
    let mut interval = tokio::time::interval_at(start, period);
    loop {
        interval.tick().await;
        let (Some(world), Some(room)) = (world.upgrade(), room.upgrade()) else {
            return;
        };
        crate::game::contain_panic(format_args!("roller tick"), || {
            room.lock().roller_tick(&world)
        });
    }
}

impl Room {
    /// Rebuilds the collision map from the items and who stands where (Java:
    /// `RoomMapping.regenerateCollisionMap`).
    pub fn regenerate_map(&mut self) {
        let entities: Vec<(i32, Position)> = self
            .entities
            .iter()
            .map(|e| (e.instance_id, e.position))
            .collect();
        self.mapping.regenerate(&entities);
    }

    /// Java: `RollerTask.run`.
    pub fn roller_tick(&mut self, world: &World) {
        if !self.mapping.items.iter().any(|i| i.has(Behaviour::Roller)) {
            return;
        }
        self.regenerate_map();
        let stack_limit = world.settings.read().int("stack.height.limit");
        // In insertion order, like Java's LinkedHashMaps.
        let mut items_rolling: Vec<(usize, Position)> = Vec::new();
        let mut entities_rolling: Vec<(i32, usize, Position)> = Vec::new();
        let mut entries: Vec<RollerEntry> = Vec::new();

        for roller in 0..self.mapping.items.len() {
            let roller_position = self.mapping.items[roller].position;
            let Some(roller_tile) = self.mapping.tile(roller_position) else {
                continue;
            };
            if !self.mapping.items[roller].has(Behaviour::Roller) {
                continue;
            }
            let tile_items = roller::items_by_height(self, roller_tile);
            let tile_entities = self.mapping.entire_entities(roller_position);
            let mut entry = RollerEntry {
                roller,
                items: Vec::new(),
                entity: None,
            };
            for item in tile_items {
                let it = &self.mapping.items[item];
                if it.position != roller_position
                    || it.has(Behaviour::Roller)
                    || items_rolling.iter().any(|(i, _)| *i == item)
                {
                    continue;
                }
                if let Some(next) = roller::item_can_roll(self, item, roller, stack_limit) {
                    items_rolling.push((item, next));
                    entry.items.push(item);
                }
            }
            if let Some(&instance_id) = tile_entities.first() {
                if entities_rolling.iter().any(|(i, _, _)| *i == instance_id) {
                    continue; // Java's `continue` skips this roller's entry too.
                }
                if let Some(e) = self.entity_by_instance(instance_id) {
                    if let Some(next) = roller::entity_can_roll(self, e, roller, stack_limit) {
                        entities_rolling.push((instance_id, roller, next));
                        entry.entity = Some(instance_id);
                    }
                }
            }
            if entry.entity.is_some() || !entry.items.is_empty() {
                entries.push(entry);
            }
        }

        for &(instance_id, roller, next) in &entities_rolling {
            if let Some(e) = self.entity_by_instance(instance_id) {
                roller::entity_do_roll(self, e, roller, next);
            }
        }
        for &(item, next) in &items_rolling {
            if !self.mapping.items[item].current_roll_blocked {
                roller::item_do_roll(self, item, next);
            }
            self.save_item(world, item);
        }
        for entry in &entries {
            let slides: Vec<Slide> = entry
                .items
                .iter()
                .map(|&i| &self.mapping.items[i])
                .filter(|i| !i.current_roll_blocked)
                .filter_map(|i| {
                    i.rolling.map(|r| Slide {
                        id: i.virtual_id,
                        from_z: r.from.z,
                        to_z: r.next.z,
                    })
                })
                .collect();
            let entity = entry
                .entity
                .and_then(|id| self.entity_by_instance(id))
                .and_then(|e| {
                    let e = &self.entities[e];
                    e.rolling.map(|r| Slide {
                        id: e.instance_id,
                        from_z: r.from.z,
                        to_z: r.display_height,
                    })
                });
            let msg = out_items::slide_object_bundle(
                &self.mapping.items[entry.roller],
                &slides,
                entity.as_ref(),
            );
            self.send(msg);
        }
        if items_rolling.is_empty() && entities_rolling.is_empty() {
            return;
        }
        self.regenerate_map();
        let items: Vec<i32> = items_rolling
            .iter()
            .map(|(i, _)| self.mapping.items[*i].virtual_id)
            .collect();
        let entities: Vec<i32> = entities_rolling.iter().map(|(i, _, _)| *i).collect();
        self.schedule(world, Duration::from_millis(800), move |_, r| {
            r.roller_complete(&items, &entities)
        });
    }

    /// Java: `RollerCompleteTask.run`: the rolled items may roll again; the rolled users get
    /// their status worked out on the square they arrived at.
    fn roller_complete(&mut self, items: &[i32], entities: &[i32]) {
        for &id in items {
            if let Some(i) = self.mapping.item_index(id) {
                let item = &mut self.mapping.items[i];
                if item.rolling.is_none() {
                    continue;
                }
                item.current_roll_blocked = false;
                item.rolling = None;
            }
        }
        for &id in entities {
            let Some(e) = self.entity_by_instance(id) else {
                continue;
            };
            if self.entities[e].rolling.is_none() {
                continue;
            }
            self.invoke_item(None, e, true, None);
            self.entities[e].rolling = None;
        }
    }
}
