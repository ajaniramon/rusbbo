//! Rooms (Java: `game/room`: `Room`, `RoomManager`, `managers/RoomEntityManager`, and the walking
//! and talking parts of `entities/RoomEntity`).
//!
//! Each loaded room is a [`RoomHandle`]: a `Mutex<Room>` in the world's registry. The entities
//! (users in the room) live inside the room, so the room tick only takes the room's lock. Changes
//! to a player's own state that a room operation causes (leaving through the door, walkways) are
//! queued as [`Deferred`] actions and run by the caller after it released the room's lock, so the
//! lock order "player before room, never two rooms" holds.
//!
//! Private rooms ("flats") keep their rights list and votes here too (Java: `Room.rights`,
//! `Room.votes`), loaded when the room is initialised together with their furniture (see
//! [`furniture`]). TODO(bots): `rooms_bots` NPCs; rooms that have them simply show none.

pub mod entity;
pub mod furniture;
pub mod items;
pub mod mapping;
pub mod model;
pub mod tasks;
pub mod walkways;

use std::sync::{Arc, Weak};

use havana_protocol::Message;
use parking_lot::{Mutex, MutexGuard};
use tracing::{error, warn};

use crate::game::now;
use crate::game::pathfinder::{Position, Walker, make_path};
use crate::game::room::entity::{EntityDetails, RoomEntity, StatusType};
use crate::game::room::items::{Behaviour, Interaction};
use crate::game::room::mapping::{RoomMapping, is_valid_tile};
use crate::game::room::model::RoomModel;
use crate::messages::outgoing::rooms_user::{self, ChatType};
use crate::messages::outgoing::{
    rooms as out_rooms, rooms_moderation as out_moderation, rooms_user as out_user,
};
use crate::session::Session;
use crate::world::World;

/// A loaded room in the registry.
pub struct RoomHandle {
    pub id: u32,
    room: Mutex<Room>,
}

impl RoomHandle {
    pub fn lock(&self) -> MutexGuard<'_, Room> {
        self.room.lock()
    }
}

pub type RoomRef = Arc<RoomHandle>;

/// Work a room operation leaves for after the room's lock is released.
pub enum Deferred {
    /// The user was taken out of `room_id` from inside the room (door, kick): forget the room on
    /// the player.
    Left { session: Arc<Session>, room_id: u32 },
    /// The user stopped on a walkway: move them into the target room.
    Walkway {
        session: Arc<Session>,
        target_room_id: u32,
        destination: Option<Position>,
    },
    /// The user was kicked: forget which rooms they may enter (Java: `RoomPlayer.kick` resets
    /// `authenticateId`).
    ClearAuthentication { session: Arc<Session> },
}

/// Java: `VoteData`, a stored vote with what `hasVoted` compares.
pub struct Vote {
    pub user_id: u32,
    #[allow(dead_code)] // TODO(votes): RATEFLAT sums them into the rating.
    pub vote: i32,
    /// The voter's machine id (Java's `getRatings` joins `users`).
    pub machine_id: String,
}

/// Java: `Room` with its `RoomData`, entity list and managers.
pub struct Room {
    pub data: havana_storage::Room,
    pub model: Arc<RoomModel>,
    /// Collision map and the room's items (`mapping.items` is Java's `Room.items`).
    pub mapping: RoomMapping,
    /// Users with rights, besides the owner (Java: `Room.rights`), loaded on initialisation.
    pub rights: Vec<u32>,
    /// Java: `Room.votes`, loaded on initialisation.
    pub votes: Vec<Vote>,
    /// The statuses the first room tick composed when the room was initialised, sent once the
    /// entry messages are out (see [`Room::try_initialise`]).
    first_tick: Option<Message>,
    /// In entry order (Java: `CopyOnWriteArrayList<Entity>`).
    pub entities: Vec<RoomEntity>,
    /// Java: `isActive`, set by the first `G_STAT`, cleared on dispose.
    pub active: bool,
    /// Java: `RoomEntityManager.counter`, the next instance id.
    counter: i32,
    /// The tick task (Java: `RoomTaskManager`'s EntityTask and StatusTask).
    task: Option<tokio::task::AbortHandle>,
    /// Bumped on every dispose request, so only the latest one runs.
    dispose_generation: u64,
    self_ref: Weak<RoomHandle>,
    deferred: Vec<Deferred>,
}

impl Room {
    /// Java: `RoomDao.getRoomById`. `None` if the room or its model does not exist.
    pub fn load(world: &World, id: u32) -> Option<RoomRef> {
        let data = match world.store.room(id) {
            Ok(Some(data)) => data,
            Ok(None) => return None,
            Err(e) => {
                error!(room = id, "loading room: {e}");
                return None;
            }
        };
        let Some(model) = world.room_models.get(&data.model).cloned() else {
            warn!(room = id, model = %data.model, "room has no model");
            return None;
        };
        let is_public = data.owner_id == 0;
        let mut mapping = RoomMapping::new(model.clone(), is_public);
        mapping.walkway_destinations = world
            .walkways
            .iter()
            .filter(|w| w.room_id == id)
            .map(|w| w.destination.unwrap_or_else(|| model.door_location()))
            .collect();
        Some(Arc::new_cyclic(|weak| RoomHandle {
            id,
            room: Mutex::new(Room {
                data,
                model,
                mapping,
                rights: Vec::new(),
                votes: Vec::new(),
                first_tick: None,
                entities: Vec::new(),
                active: false,
                counter: 0,
                task: None,
                dispose_generation: 0,
                self_ref: weak.clone(),
                deferred: Vec::new(),
            }),
        }))
    }

    pub fn id(&self) -> u32 {
        self.data.id
    }

    /// Java: `isPublicRoom`.
    pub fn is_public(&self) -> bool {
        self.data.owner_id == 0
    }

    pub fn take_deferred(&mut self) -> Vec<Deferred> {
        std::mem::take(&mut self.deferred)
    }

    /// Java: `Room.send`: to every player in the room.
    pub fn send(&self, msg: Message) {
        for e in &self.entities {
            e.session.send(msg.clone());
        }
    }

    /// Java: `Room.send(composer, exclude)`.
    pub fn send_except(&self, msg: Message, except: u64) {
        for e in self.entities.iter().filter(|e| e.session.id != except) {
            e.session.send(msg.clone());
        }
    }

    pub fn entity_index(&self, session_id: u64) -> Option<usize> {
        self.entities
            .iter()
            .position(|e| e.session.id == session_id)
    }

    pub fn entity_by_instance(&self, instance_id: i32) -> Option<usize> {
        self.entities
            .iter()
            .position(|e| e.instance_id == instance_id)
    }

    fn players(&self) -> i32 {
        self.entities.len() as i32
    }

    pub fn walker(&self, idx: usize) -> Walker {
        let e = &self.entities[idx];
        Walker {
            instance_id: e.instance_id,
            position: e.position,
            swimming: e.statuses.contains(StatusType::Swim),
            has_pool_figure: !e.details.pool_figure.is_empty(),
        }
    }

    fn save_visitors(&self, world: &World) {
        let visitors = self.players();
        if let Err(e) = world
            .store
            .update_room(self.id(), |r| r.visitors_now = visitors)
        {
            error!(room = self.id(), "saving visitors: {e}");
        }
    }

    /// Java: `RoomEntityManager.silentlyEnterRoom` (from where the entity is added) and, when
    /// `send_entry`, `enterRoom`'s messages to the user.
    fn add_entity(
        &mut self,
        world: &World,
        session: &Arc<Session>,
        details: EntityDetails,
        destination: Option<Position>,
    ) {
        let instance_id = self.counter;
        self.counter += 1;
        let mut entity = RoomEntity::new(session.clone(), details, instance_id);
        entity.position = destination.unwrap_or_else(|| self.model.door_location());
        {
            let settings = world.settings.read();
            entity.reset_timers(
                settings.int("afk.timer.seconds"),
                settings.int("sleep.timer.seconds"),
            );
        }
        // TODO(games): game players enter at their spawn with their object id as instance id.
        self.entities.push(entity);
        self.data.visitors_now = self.players();

        self.try_initialise(world);
        // TODO(effects): Abigail.Ryan's ghost effect. TODO(items): teleporter arrival.
        // TODO(messenger): sendStatusUpdate so friends see the room.
        self.save_visitors(world);
    }

    /// Java: `RoomEntityManager.tryInitialiseRoom`: loads the furniture (and, for private rooms,
    /// the rights and votes), builds the map and (re)starts the tick while the room is not
    /// active yet.
    ///
    /// Java's tasks start with no delay on the scheduler thread, so their first run reads the
    /// room right after the newcomer was added, before `enterRoom` goes on with `refreshRights`
    /// (the first `USER_STATUSES` of a freshly loaded flat has no `flatctrl`); its message is
    /// written after the entry messages because those are written from the event loop the
    /// connection belongs to. That first tick runs here, its statuses are kept in `first_tick`
    /// and sent by [`enter_room`], and the task starts with the second tick.
    fn try_initialise(&mut self, world: &World) {
        if self.active {
            return;
        }
        // TODO(games): game arenas and custom rooms keep their items.
        self.mapping.items.clear();
        self.rights.clear();
        self.votes.clear();
        if self.is_public() {
            match world.store.public_items(&self.model.id) {
                Ok(rows) => self.mapping.items = items::public_items(&self.model.id, &rows),
                Err(e) => error!(room = self.id(), "loading public items: {e}"),
            }
        } else {
            self.load_rights_and_votes(world);
        }
        self.load_flat_items(world);
        let entities: Vec<(i32, Position)> = self
            .entities
            .iter()
            .map(|e| (e.instance_id, e.position))
            .collect();
        self.mapping.regenerate(&entities);
        self.first_tick = self.entity_pass(world);
        self.status_tick();
        self.start_tasks(world);
    }

    /// Java: `RoomRightsDao.getRoomRights` and `RoomVoteDao.getRatings`.
    fn load_rights_and_votes(&mut self, world: &World) {
        match world.store.room_rights(self.id()) {
            Ok(rights) => self.rights = rights,
            Err(e) => error!(room = self.id(), "loading rights: {e}"),
        }
        match world.store.room_votes(self.id(), now()) {
            Ok(votes) => {
                self.votes = votes
                    .into_iter()
                    .filter_map(|v| {
                        // INNER JOIN users: votes of deleted users do not count.
                        let user = world.store.user(v.user_id).ok().flatten()?;
                        Some(Vote {
                            user_id: v.user_id,
                            vote: v.vote,
                            machine_id: user.machine_id,
                        })
                    })
                    .collect();
            }
            Err(e) => error!(room = self.id(), "loading votes: {e}"),
        }
    }

    /// Java: `RoomTaskManager.startTasks`: the 500 ms entity task and 1 s status task, both
    /// started now; their first run already happened in [`Room::try_initialise`], so the task
    /// runs from the second one, 500 ms later. Restarting replaces the running ones, as Java's
    /// `scheduleTask` does.
    fn start_tasks(&mut self, world: &World) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let Some(world) = world.arc() else {
            return; // Unit tests build rooms without a running world.
        };
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        // TODO(flats): RollerTask for private rooms.
        let handle = rt.spawn(tasks::run(Arc::downgrade(&world), self.self_ref.clone()));
        self.task = Some(handle.abort_handle());
    }

    fn stop_tasks(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    /// Java: `RoomEntityManager.tryRoomEntry` (`G_STAT`). Returns whether it was the first entry.
    pub fn try_room_entry(&mut self) -> bool {
        let was_active = self.active;
        self.active = true;
        // TODO(triggers): model trigger onRoomEntry. TODO(bots): BotManager.addBots on first entry.
        !was_active
    }

    /// Java: `RoomEntityManager.leaveRoom`. Removes the entity and returns it as Java's `reset`
    /// leaves it (instance id -1, no statuses, `needsUpdate` set).
    fn remove_entity(
        &mut self,
        world: &World,
        session_id: u64,
        hotel_view: bool,
    ) -> Option<RoomEntity> {
        let idx = self.entity_index(session_id)?;
        let mut e = self.entities.remove(idx);
        // TODO(items): the current item's onEntityLeave trigger. TODO(triggers): onRoomLeave.
        self.mapping.remove_entity(e.position, e.instance_id);
        if let Some(next) = e.next_position {
            self.mapping.remove_entity(next, e.instance_id);
        }
        self.data.visitors_now = self.players();
        self.send(out_user::logout(e.instance_id));
        self.try_dispose(world);

        // Java: RoomEntity.reset() + RoomPlayer.reset().
        e.statuses.clear();
        e.path.clear();
        e.next_position = None;
        e.goal = None;
        e.is_walking = false;
        e.dance_id = 0;
        e.is_sleeping = false;
        e.walking_allowed = true;
        e.being_kicked = false;
        e.instance_id = -1;
        e.is_typing = false;
        e.needs_update = true;

        if hotel_view {
            e.session.send(out_user::hotel_view());
        }
        // TODO(games): Wobble Squabble and game leave. TODO(messenger): status update.
        self.save_visitors(world);
        Some(e)
    }

    /// Java: `Room.tryDispose`: once the last player left, unload the room after
    /// `room.dispose.timer.seconds` unless someone came back.
    fn try_dispose(&mut self, world: &World) {
        if self.players() > 0 {
            return;
        }
        self.dispose_generation += 1;
        let generation = self.dispose_generation;
        let seconds = {
            let settings = world.settings.read();
            if settings.bool("room.dispose.timer.enabled") {
                settings.int("room.dispose.timer.seconds").max(0) as u64
            } else {
                0
            }
        };
        let (Some(world), Ok(rt)) = (world.arc(), tokio::runtime::Handle::try_current()) else {
            return;
        };
        let room = self.self_ref.clone();
        let world = Arc::downgrade(&world);
        rt.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
            let (Some(world), Some(room)) = (world.upgrade(), room.upgrade()) else {
                return;
            };
            let mut r = room.lock();
            if r.dispose_generation != generation || r.players() > 0 {
                return;
            }
            // TODO(events): remove the room's event. TODO(pets): save pet coordinates.
            r.reset_item_states(&world);
            r.counter = 0;
            r.active = false;
            r.stop_tasks();
            r.mapping.items.clear();
            r.rights.clear();
            r.votes.clear();
            r.entities.clear();
            world.unregister_room(&room);
        });
    }

    /// Java: `RoomEntity.updateNewHeight`.
    fn update_new_height(&mut self, idx: usize) {
        let e = &mut self.entities[idx];
        let Some(tile) = self.mapping.tile(e.position) else {
            return;
        };
        let height = self.mapping.walking_height(tile);
        if height != e.position.z {
            e.position.z = height;
            e.needs_update = true;
        }
    }

    /// Java: `RoomEntity.walkTo`.
    pub fn walk_to(&mut self, idx: usize, x: i32, y: i32) -> bool {
        if self.is_redirected(idx, x, y) {
            return false;
        }
        if let Some(next) = self.entities[idx].next_position {
            let e = &mut self.entities[idx];
            e.position.x = next.x;
            e.position.y = next.y;
            self.update_new_height(idx);
            // TODO(items): the current item's onEntityStep trigger.
        }
        if self.mapping.tile(Position::new(x, y)).is_none() {
            return false;
        }
        let goal = Position::new(x, y);
        self.entities[idx].goal = Some(goal);
        // TODO(commands): teleporting users warp instead of walking.
        let walker = self.walker(idx);
        if !is_valid_tile(&self.mapping, Some(&walker), goal) {
            return false;
        }
        // TODO(items): beds redirect the goal to the pillow (BedInteractor).
        let path = make_path(&self.mapping, &walker, walker.position, goal);
        if path.is_empty() {
            return false;
        }
        let e = &mut self.entities[idx];
        e.path = path;
        e.is_walking = true;
        true
    }

    /// Java: `PublicRoomRedirection.isRedirected`.
    fn is_redirected(&mut self, idx: usize, x: i32, y: i32) -> bool {
        let position = self.entities[idx].position;
        match self.model.name.as_str() {
            "sun_terrace" => {
                let Some(goal_z) = self
                    .mapping
                    .tile(Position::new(x, y))
                    .map(|t| t.tile_height)
                else {
                    return false;
                };
                // Java: `!(currentZ >= 8)`, heights are never NaN.
                if position.z < 8.0 && goal_z >= 8.0 && position.x != 4 && position.y != 18 {
                    return true;
                }
                x == 4 && y == 18 && position.x != 6 && position.y != 21
            }
            "star_lounge" if x == 36 && y == 27 => {
                self.walk_to(idx, 37, 28);
                true
            }
            _ => false,
        }
    }

    /// Java: `RoomEntity.stopWalking`. Returns the entity if it left the room.
    fn stop_walking(&mut self, world: &World, idx: usize) -> Option<RoomEntity> {
        let room_id = self.id();
        let e = &mut self.entities[idx];
        e.path.clear();
        e.is_walking = false;
        e.needs_update = true;
        e.next_position = None;
        e.statuses.remove(StatusType::Move);
        if e.enable_walking_on_stop {
            e.enable_walking_on_stop = false;
            e.walking_allowed = true;
        }
        let position = e.position;
        let being_kicked = e.being_kicked;
        let session = e.session.clone();

        if !being_kicked {
            if let Some(w) = world.walkway_at(self, position) {
                // Java enters the target room right here; it runs once the room lock is released.
                self.deferred.push(Deferred::Walkway {
                    session,
                    target_room_id: w.target_room_id,
                    destination: w.destination,
                });
                return None;
            }
        }
        let door = self.model.door_location();
        let mut leave = being_kicked;
        if door == position {
            leave = true;
            if self
                .mapping
                .highest_item(position)
                .is_some_and(|i| i.has(Behaviour::Teleporter))
            {
                leave = false;
            }
        }
        if self.is_public() && world.walkway_at(self, door).is_some() {
            leave = false;
        }
        if leave || being_kicked {
            let left = self.remove_entity(world, session.id, true);
            self.deferred.push(Deferred::Left { session, room_id });
            return left;
        }
        self.invoke_item(idx, false);
        None
    }

    /// Java: `RoomEntity.invokeItem`: runs the trigger of the item the entity stopped on.
    fn invoke_item(&mut self, idx: usize, instant_update: bool) {
        let position = self.entities[idx].position;
        let Some(tile) = self.mapping.tile(position) else {
            return;
        };
        let height = self.mapping.walking_height(tile);
        let item = tile.highest_item;
        self.entities[idx].position.z = height;

        let (can_sit, can_lay) = item.map_or((false, false), |i| {
            let item = &self.mapping.items[i];
            (
                item.has(Behaviour::CanSitOnTop),
                item.has(Behaviour::CanLayOnTop),
            )
        });
        if item.is_none() || !can_sit || !can_lay {
            let e = &mut self.entities[idx];
            // TODO(items): rollers ("isRolling") keep the sit.
            if e.statuses.contains(StatusType::Sit) || e.statuses.contains(StatusType::Lay) {
                e.statuses.remove(StatusType::Sit);
                e.statuses.remove(StatusType::Lay);
            }
            if item.is_none() && e.last_item_interaction.is_some() {
                // TODO(items): the last item's onEntityLeave trigger.
                e.last_item_interaction = None;
            }
        }
        if let Some(i) = item {
            if self.mapping.items[i].interaction == Interaction::Chair {
                self.chair_stop(idx, i);
            }
            // TODO(items, pool, games): the other interaction triggers' onEntityStop.
            self.entities[idx].last_item_interaction = Some(self.mapping.items[i].virtual_id);
        }
        self.update_new_height(idx);
        if instant_update {
            let msg = out_user::user_statuses([&self.entities[idx]], &self.model.name);
            self.send(msg);
        } else {
            self.entities[idx].needs_update = true;
        }
    }

    /// Java: `ChairInteractor.onEntityStop`.
    fn chair_stop(&mut self, idx: usize, item: usize) {
        let (top_height, rotation) = {
            let item = &self.mapping.items[item];
            (item.top_height, item.position.rotation())
        };
        self.entities[idx].position.set_rotation(rotation);
        self.dance(idx, 0);
        let e = &mut self.entities[idx];
        e.statuses
            .set(StatusType::Sit, crate::game::format_decimal(top_height));
        e.needs_update = true;
        // TODO(items): rolling keeps the head rotation. TODO(effects): re-send the effect.
    }

    /// Java: `RoomEntity.dance`.
    pub fn dance(&mut self, idx: usize, dance_id: i32) {
        let pool = self.model.name.starts_with("pool_") || self.model.name == "md_a";
        let e = &mut self.entities[idx];
        e.dance_id = dance_id;
        let msg = out_user::user_dance(e.instance_id, dance_id);
        if pool && !e.details.pool_figure.is_empty() && dance_id > 0 {
            e.statuses.set(StatusType::Dance, dance_id.to_string());
        }
        if dance_id == 0 && e.statuses.contains(StatusType::Dance) {
            e.statuses.remove(StatusType::Dance);
            e.needs_update = true;
        }
        self.send(msg);
    }

    /// Java: `RoomEntity.wave`.
    pub fn wave(&mut self, idx: usize) {
        if self.entities[idx].is_using_effect() {
            return;
        }
        self.dance(idx, 0);
        self.send(out_user::user_wave(self.entities[idx].instance_id));
    }

    /// Java: `RoomEntity.sleep`.
    pub fn sleep(&mut self, idx: usize, sleeping: bool) {
        self.entities[idx].is_sleeping = sleeping;
        self.send(out_user::user_sleep(
            self.entities[idx].instance_id,
            sleeping,
        ));
    }

    /// Java: `RoomTimerManager.resetRoomTimer`.
    pub fn reset_room_timer(&mut self, world: &World, idx: usize) {
        let (afk, sleep) = {
            let settings = world.settings.read();
            (
                settings.int("afk.timer.seconds"),
                settings.int("sleep.timer.seconds"),
            )
        };
        self.entities[idx].reset_timers(afk, sleep);
        if self.entities[idx].is_sleeping {
            self.sleep(idx, false);
        }
    }

    /// Java: `RoomEntity.look(towards)`: the head stays as it is; the look timer starts and the
    /// status is re-sent.
    fn look(&mut self, idx: usize) {
        let e = &self.entities[idx];
        if e.is_walking {
            return;
        }
        if self
            .mapping
            .highest_item(e.position)
            .is_some_and(|i| i.has(Behaviour::NoHeadTurn))
        {
            return;
        }
        let e = &mut self.entities[idx];
        e.look_timer = now() + 6;
        e.needs_update = true;
    }

    /// Java: `RoomPlayer.talk` and `RoomEntity.talk(message, type)`.
    pub fn talk(&mut self, world: &World, idx: usize, message: &[u8], kind: ChatType) {
        if message.ends_with(b"o/") {
            self.wave(idx);
            if message == b"o/" {
                return;
            }
        }
        let e = &mut self.entities[idx];
        e.chat_spam_count += 1;
        if e.chat_spam_ticks == -1 {
            // TODO(games): twice as long in SnowStorm.
            e.chat_spam_ticks = 8;
        }
        if e.chat_spam_count >= 6 {
            e.mute_time = now() + 30;
        }
        if e.mute_time > now() {
            return;
        }
        // TODO(moderation): muted rooms ("Shh! The room is currently muted...").
        let message: &[u8] = if e.details.name == "Abigail.Ryan" {
            b""
        } else {
            message
        };
        // TODO(messenger): skip receivers that ignore the talker.
        self.talk_to_all(world, idx, message, kind);
        self.entities[idx].mute_time = 0;
    }

    /// Java: `RoomEntity.talk(message, type, receivers)` with every player as receiver.
    fn talk_to_all(&mut self, world: &World, idx: usize, message: &[u8], kind: ChatType) {
        let spam_count = world.settings.read().int("chat.spam.count");
        let e = &mut self.entities[idx];
        let save = !e
            .chat_messages
            .iter()
            .any(|m| latin1_eq_ignore_case(m, message));
        e.chat_messages.push(message.to_vec());
        if e.chat_messages.len() as i64 > spam_count {
            e.chat_messages.remove(0);
        }
        let talker = e.position;
        let instance_id = e.instance_id;

        if kind != ChatType::Whisper {
            for other in 0..self.entities.len() {
                if other == idx || self.entities[other].is_sleeping {
                    continue;
                }
                if kind == ChatType::Chat
                    && talker.distance_squared(&self.entities[other].position) > 14
                {
                    continue;
                }
                self.look(other);
            }
        }
        // TODO(wordfilter): users with the filter on get WordfilterManager.filterSentence.
        let msg = rooms_user::chat_message(kind, instance_id, message, chat_gesture(message));
        self.send(msg);
        // TODO(bots, pets): handleSpeech.
        if save {
            // TODO(chatlog): ChatManager.queue (room_chatlogs).
            self.reset_room_timer(world, idx);
        }
    }

    /// Java: `RoomPlayer.kick(allowWalking, isBeingKicked)`: walk to the door, or leave now, and
    /// forget the room authentications.
    pub fn kick(&mut self, world: &World, idx: usize, allow_walking: bool, being_kicked: bool) {
        let door = self.model.door_location();
        let room_id = self.id();
        let session = self.entities[idx].session.clone();
        self.deferred.push(Deferred::ClearAuthentication {
            session: session.clone(),
        });
        if self.entities[idx].position == door {
            self.remove_entity(world, session.id, true);
            self.deferred.push(Deferred::Left { session, room_id });
            return;
        }
        self.walk_to(idx, door.x, door.y);
        let e = &mut self.entities[idx];
        e.walking_allowed = allow_walking;
        e.being_kicked = being_kicked;
        if !e.is_walking {
            self.remove_entity(world, session.id, true);
            self.deferred.push(Deferred::Left { session, room_id });
        }
    }

    /// Java: `Room.isOwner`.
    pub fn is_owner(&self, user_id: u32) -> bool {
        self.data.owner_id == user_id
    }

    /// Java: `Room.hasRights(userId, includeSuperUsers)`: the owner, everyone when the room
    /// gives everybody rights ("superusers") and `include_super_users`, and the rights list.
    pub fn has_rights(&self, user_id: u32, include_super_users: bool) -> bool {
        if self.is_public() {
            return false;
        }
        if self.is_owner(user_id) {
            return true;
        }
        if include_super_users && self.data.superusers {
            return true;
        }
        self.rights.contains(&user_id)
    }

    /// Java: `Room.hasVoted`. Users who joined the hotel less than an hour ago count as having
    /// voted, and so does anyone whose user id or machine id voted. TODO(ip-log): Java also
    /// matches the voters' IP history (`users_ip_logs`).
    pub fn has_voted(&self, details: &EntityDetails) -> bool {
        let mins_since_joined = (now() - details.created_at) / 60;
        if mins_since_joined <= 60 {
            return true;
        }
        if self.votes.iter().any(|v| v.user_id == details.user_id) {
            return true;
        }
        self.votes.iter().any(|v| {
            !is_blank(v.machine_id.as_bytes())
                && !is_blank(details.machine_id.as_bytes())
                && v.machine_id.to_lowercase() == details.machine_id.to_lowercase()
        })
    }

    /// Java: `Room.refreshRights(player, sendStatus)`: tells the user what they may do and sets
    /// their `flatctrl` status (`useradmin` for the owner). With `send_status` the status goes
    /// out on the next tick.
    pub fn refresh_rights(&mut self, idx: usize, send_status: bool) {
        let e = &self.entities[idx];
        let user_id = e.details.user_id;
        let mut value = "";
        if self.is_owner(user_id) || e.details.has_fuse("fuse_any_room_controller") {
            e.session.send(out_moderation::you_are_owner());
            value = "useradmin";
        }
        let has_rights = self.has_rights(user_id, true);
        let is_mod = e.details.has_fuse("fuse_mod");
        if has_rights || is_mod {
            e.session.send(out_moderation::you_are_controller());
        } else {
            e.session.send(out_moderation::you_not_controller());
        }
        let flat_control = has_rights || self.is_owner(user_id) || is_mod;
        let e = &mut self.entities[idx];
        e.statuses.remove(StatusType::FlatControl);
        if flat_control {
            e.statuses.set(StatusType::FlatControl, value);
        }
        if send_status {
            e.needs_update = true;
        }
    }

    /// Java: `leaveRoom(entity, hotelView)` for a user taken out of the room from outside their
    /// own session (`DELETEFLAT`): the player forgets the room once the room lock is released.
    pub fn remove_user(&mut self, world: &World, session: &Arc<Session>, hotel_view: bool) {
        if self.remove_entity(world, session.id, hotel_view).is_some() {
            self.deferred.push(Deferred::Left {
                session: session.clone(),
                room_id: self.id(),
            });
        }
    }

    /// Java: `Room.tryDispose`, called again by `DELETEFLAT` after everybody left.
    pub fn dispose_when_empty(&mut self, world: &World) {
        self.try_dispose(world);
    }

    /// Java's `getRoomUser().getCurrentItem()`.
    pub fn current_item(&self, idx: usize) -> Option<&items::Item> {
        self.mapping.highest_item(self.entities[idx].position)
    }
}

/// Java's `String.equalsIgnoreCase` on ISO-8859-1 text.
fn latin1_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    fn fold(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c + 32,
            0xC0..=0xDE if c != 0xD7 => c + 32,
            _ => c,
        }
    }
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| fold(*x) == fold(*y))
}

/// Java: `RoomEntity.getChatGesture`.
fn chat_gesture(message: &[u8]) -> i32 {
    let has = |s: &str| message.windows(s.len()).any(|w| w == s.as_bytes());
    if [":)", ":-)", ":p", ":d", ":D", ";)", ";-)"]
        .iter()
        .any(|s| has(s))
    {
        1 // sml
    } else if [":s", ":(", ":-(", ":'("].iter().any(|s| has(s)) {
        4 // sad
    } else if [":o", ":O"].iter().any(|s| has(s)) {
        3 // srp
    } else if [":@", ">:("].iter().any(|s| has(s)) {
        2 // agr
    } else {
        0
    }
}

/// Java: `StringUtil.filterInput(input, true)` on ISO-8859-1 bytes. TODO(settings):
/// `normalise.input.strings` (NFD normalisation) is not applied.
pub fn filter_input(input: &[u8]) -> Vec<u8> {
    input
        .iter()
        .map(|&c| {
            if matches!(c, 1 | 2 | 9 | 10 | 12 | 13) {
                b' '
            } else {
                c
            }
        })
        .collect()
}

/// Java: `String.isBlank` (only whitespace) on ISO-8859-1 bytes.
pub fn is_blank(input: &[u8]) -> bool {
    input
        .iter()
        .all(|c| matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | 0x1C..=0x1F))
}

/// The room the player is in, if it is loaded.
pub fn current_room(world: &World, session: &Session) -> Option<RoomRef> {
    let room_id = session.player.lock().room_id?;
    world.loaded_room(room_id)
}

/// Runs what a room operation queued (see [`Deferred`]). Call with no lock held.
pub fn run_deferred(world: &World, deferred: Vec<Deferred>) {
    for action in deferred {
        match action {
            Deferred::Left { session, room_id } => {
                let mut p = session.player.lock();
                if p.room_id == Some(room_id) {
                    p.room_id = None;
                }
            }
            Deferred::Walkway {
                session,
                target_room_id,
                destination,
            } => {
                let target = world
                    .loaded_room(target_room_id)
                    .or_else(|| Room::load(world, target_room_id));
                if let Some(target) = target {
                    enter_room(world, &session, target, destination, true);
                }
            }
            Deferred::ClearAuthentication { session } => {
                // TODO(items): also the teleporter authentications.
                session.player.lock().authenticate_id = None;
            }
        }
    }
}

/// Leaves the room the player is in (Java: `leaveRoom(player, hotelView)` on the current room).
pub fn leave_current_room(world: &World, session: &Arc<Session>, hotel_view: bool) {
    let room_id = session.player.lock().room_id.take();
    let Some(room) = room_id.and_then(|id| world.loaded_room(id)) else {
        return;
    };
    let deferred = {
        let mut r = room.lock();
        r.remove_entity(world, session.id, hotel_view);
        r.take_deferred()
    };
    run_deferred(world, deferred);
}

/// Java: `RoomEntityManager.enterRoom(player, destination)`: leaves the current room, joins
/// `room` at its door (or `destination`) and, with `send_entry`, tells the client the room is
/// ready.
pub fn enter_room(
    world: &World,
    session: &Arc<Session>,
    room: RoomRef,
    destination: Option<Position>,
    send_entry: bool,
) {
    leave_current_room(world, session, false);
    let room = world.register_room(room);
    let details = {
        let mut p = session.player.lock();
        let Some(user) = p.details.as_ref().filter(|_| p.logged_in) else {
            return;
        };
        let details = EntityDetails {
            user_id: user.id,
            name: user.username.clone(),
            motto: user.motto.clone(),
            figure: user.figure.clone(),
            sex: user.sex.clone(),
            pool_figure: user.pool_figure.clone(),
            tickets: user.tickets,
            created_at: user.created_at,
            rank: user.rank,
            club_expiration: user.club_expiration,
            machine_id: user.machine_id.clone(),
        };
        p.room_id = Some(room.id);
        p.authenticate_id = None;
        details
    };
    let deferred = {
        let mut r = room.lock();
        r.add_entity(world, session, details, destination);
        if send_entry {
            session.send(out_rooms::room_ready(r.id(), &r.model.name));
            session.send(out_rooms::flat_property("landscape", &r.data.landscape));
            if r.data.wallpaper > 0 {
                session.send(out_rooms::flat_property(
                    "wallpaper",
                    &r.data.wallpaper.to_string(),
                ));
            }
            if r.data.floor > 0 {
                session.send(out_rooms::flat_property("floor", &r.data.floor.to_string()));
            }
            if let Some(idx) = r.entity_index(session.id) {
                if !r.is_public() {
                    r.refresh_rights(idx, false);
                }
                // The owner never votes on their own room.
                let details = &r.entities[idx].details;
                let voted = r.is_owner(details.user_id) || r.has_voted(details);
                session.send(out_rooms::update_votes(if voted {
                    r.data.rating
                } else {
                    -1
                }));
            }
            session.send(out_rooms::room_event_info_none());
        }
        if let Some(msg) = r.first_tick.take() {
            r.send(msg);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::pathfinder::walk_direction;

    #[test]
    fn gestures() {
        assert_eq!(chat_gesture(b"hola desde el bot"), 0);
        assert_eq!(chat_gesture(b"hi :)"), 1);
        assert_eq!(chat_gesture(b">:("), 4); // ":(" is checked first, as in Java
        assert_eq!(chat_gesture(b"wow :O"), 3);
    }

    #[test]
    fn input_filters() {
        assert_eq!(filter_input(b"a\x01b\rc"), b"a b c");
        assert!(is_blank(b" \t"));
        assert!(!is_blank(b" a"));
        assert!(latin1_eq_ignore_case(b"HOLA \xc9", b"hola \xe9"));
    }

    #[test]
    fn walking_uses_the_trace_path() {
        let model = Arc::new(RoomModel::parse(&havana_storage::RoomModel {
            model_id: "ballroom".into(),
            model_name: "ballroom".into(),
            door_x: 13,
            door_y: 6,
            door_z: 0.0,
            door_dir: 4,
            heightmap: BALLROOM.into(),
            ..Default::default()
        }));
        let mut map = RoomMapping::new(model, true);
        map.regenerate(&[]);
        let walker = Walker {
            instance_id: 0,
            position: Position::new(13, 6),
            swimming: false,
            has_pool_figure: false,
        };
        // traces/public-room.json, WALK to 10,10: mv 12,7 / 11,8 / 11,9 / 10,10.
        let path: Vec<(i32, i32)> =
            make_path(&map, &walker, walker.position, Position::new(10, 10))
                .iter()
                .map(|p| (p.x, p.y))
                .collect();
        assert_eq!(path, [(12, 7), (11, 8), (11, 9), (10, 10)]);
        // traces/room-two-bots.json, WALK to 12,8: mv 12,7 / 12,8.
        let path: Vec<(i32, i32)> = make_path(&map, &walker, walker.position, Position::new(12, 8))
            .iter()
            .map(|p| (p.x, p.y))
            .collect();
        assert_eq!(path, [(12, 7), (12, 8)]);
        assert_eq!(walk_direction(13, 6, 12, 7), 5);
    }

    const BALLROOM: &str = "xxxx4444444444444444444|xxxx4444444444444444444|xxxx4444444444444444444|xxxx33x2222444442222x33|xxxx2222222x00xx2222222|xxxx22222220000x2222222|xxxx11x0000x000x0000x11|xxxx0000000000000000000|11100000000000000000000|11100000000000000000000|11100000000000000000000|xxxx0000000000000000000|22210000000000000000000|22210000000000000000000|22210000000000000000000|xxxx0000000000000000000|11100000000000000000000|11100000000000000000000|11100000000000000000000|xxxxx000x11111111x0000x|xxxxxx00x1111x111x000xx|xxxxxxx0x11111111x00xxx|xxxxxxxxx11111111x0xxxx|xxxxxxxxx11111111xxxxxx";
}
