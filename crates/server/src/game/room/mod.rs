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
//! [`furniture`]).

pub mod actions;
mod bots;
pub mod entity;
pub mod furniture;
pub mod items;
pub mod mapping;
pub mod model;
mod pets;
pub mod pool;
#[cfg(test)]
mod pool_tests;
pub mod sound;
mod space_cafe;
pub mod tasks;
pub mod trade;
#[cfg(test)]
mod trade_tests;
pub mod walkways;
pub mod wobble_squabble;

use std::sync::{Arc, Weak};

use parking_lot::{Mutex, MutexGuard};
use rusbbo_protocol::Message;
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
    /// The model's name, readable without the room's lock (a loaded room never changes model).
    pub model_name: String,
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
        /// The room the walkway is in.
        from_room_id: u32,
        session: Arc<Session>,
        target_room_id: u32,
        destination: Option<Position>,
    },
    /// The user was kicked: forget which rooms they may enter (Java: `RoomPlayer.kick` resets
    /// `authenticateId`).
    ClearAuthentication { session: Arc<Session> },
    /// The user no longer travels to a teleporter (Java: `setAuthenticateTelporterId(-1)`).
    StopTeleporting { session: Arc<Session> },
    /// A trade was cancelled: send both hands, then release the offered items.
    TradeClosed { sides: [trade::TradeSide; 2] },
    /// A trade completed: the offered items change hands, then both hands are sent.
    TradeFinished { sides: [trade::TradeSide; 2] },
    /// Anything else that has to wait for the room's lock to be released (it may lock a player).
    Run(Box<dyn FnOnce(&World) + Send>),
}

/// Someone a chat message is sent to (Java: the `List<Player>` of `RoomEntity.talk`).
pub struct Receiver {
    pub session: Arc<Session>,
    /// Java: `isWordFilterEnabled`.
    pub filter: bool,
}

/// Java: `VoteData`, a stored vote with what `hasVoted` compares.
pub struct Vote {
    pub user_id: u32,
    pub vote: i32,
    /// The voter's IP addresses (Java's `getRatings` joins `users_ip_logs`; `addVote` takes the
    /// voter's latest 20). Never empty: no addresses is `[""]`, Java's `"".split(",")`.
    pub ip_addresses: Vec<String>,
    /// The voter's machine id (Java's `getRatings` joins `users`).
    pub machine_id: String,
}

/// Java: `RoomTradeManager.TRADE_BAN_IP_HISTORY_LIMIT`, the IP addresses `hasVoted` and
/// `addVote` read.
const VOTE_IP_HISTORY_LIMIT: usize = 20;

/// Java: `RoomVoteDao.vote`'s expiry: 60 days.
const VOTE_EXPIRE_SECONDS: i64 = 60 * 24 * 60 * 60;

/// Java: `RoomVoteDao.getRatings`: the room's votes that have not expired, of voters who exist
/// (`INNER JOIN users`) and have logged an IP address (`INNER JOIN users_ip_logs`).
pub fn counted_votes(world: &World, room_id: u32) -> Vec<Vote> {
    let votes = match world.store.room_votes(room_id, now()) {
        Ok(votes) => votes,
        Err(e) => {
            error!(room = room_id, "loading votes: {e}");
            return Vec::new();
        }
    };
    votes
        .into_iter()
        .filter_map(|v| {
            let user = world.store.user(v.user_id).ok().flatten()?;
            let ip_addresses = world.store.ip_addresses(v.user_id, usize::MAX).ok()?;
            if ip_addresses.is_empty() {
                return None;
            }
            Some(Vote {
                user_id: v.user_id,
                vote: v.vote,
                ip_addresses,
                machine_id: user.machine_id,
            })
        })
        .collect()
}

/// Java: `Room` with its `RoomData`, entity list and managers.
pub struct Room {
    pub data: rusbbo_storage::Room,
    pub model: Arc<RoomModel>,
    /// Collision map and the room's items (`mapping.items` is Java's `Room.items`).
    pub mapping: RoomMapping,
    /// Users with rights, besides the owner (Java: `Room.rights`), loaded on initialisation.
    pub rights: Vec<u32>,
    /// Java: `Room.votes`, loaded on initialisation.
    pub votes: Vec<Vote>,
    /// The statuses the first room tick composed when the room was initialised, sent once the
    /// entry messages are out (see [`Room::try_initialise`]).
    first_tick: Option<[Message; 2]>,
    /// In entry order (Java: `CopyOnWriteArrayList<Entity>`).
    pub entities: Vec<RoomEntity>,
    /// Java: `isActive`, set by the first `G_STAT`, cleared on dispose.
    pub active: bool,
    /// Java: `RoomEntityManager.counter`, the next instance id.
    counter: i32,
    /// The tick task (Java: `RoomTaskManager`'s EntityTask and StatusTask).
    task: Option<tokio::task::AbortHandle>,
    /// Java: `RoomTaskManager`'s RollerTask, private rooms only.
    roller_task: Option<tokio::task::AbortHandle>,
    /// Java: `RoomTaskManager`'s RainbowTask (`:rainbow`), while it cycles the moodlight.
    rainbow_task: Option<tokio::task::AbortHandle>,
    /// Java: `RoomTaskManager`'s BotCommandTask (`:bot`), from the first party bot on.
    bot_party_task: Option<tokio::task::AbortHandle>,
    /// Bumped on every dispose request, so only the latest one runs.
    dispose_generation: u64,
    /// Java: the room's `BotTask` is scheduled (the room has bots).
    bots_active: bool,
    /// Java: `RoomData.isRoomMuted` (`:roommute`); not saved, gone when the room unloads.
    pub is_room_muted: bool,
    /// Java: `BotTask.lastSpeech`: the bot and sentence said last, never said twice in a row.
    last_bot_speech: Option<(i32, usize)>,
    self_ref: Weak<RoomHandle>,
    deferred: Vec<Deferred>,
    /// The world the room was loaded in, for item timers started where no `&World` is at hand.
    pub(crate) world: Weak<World>,
    /// Java: `RoomIdolManager`.
    pub idol: crate::game::item::interactors::idol::IdolState,
    /// The game-hall tables of the room (Java: the game triggers' `GamehallGame` instances).
    pub(crate) gamehall: crate::game::gamehall::GameHalls,
    /// Java: the `DivingCamera` task of Lido B (`DivingDeckTrigger`), from its first `G_STAT`.
    camera: Option<pool::DivingCamera>,
    /// Java: Rooftop Rumble's `WobbleGameTask`, while a game of Wobble Squabble is on.
    wobble: Option<wobble_squabble::WobbleGame>,
    /// Java: the Space Cafe's `SpaceCafeTask`, from the room's first entry.
    space_cafe: Option<space_cafe::SpaceCafe>,
    /// Java: `EntityTask.queueAfterLoop`: sent to the room after the next entity tick's statuses.
    queue_after_loop: Vec<Message>,
    /// Set by `DELETEFLAT`: the row is gone and the room only waits to be unloaded. It still
    /// answers `GETFLATINFO` and `TRYFLAT` until then, as in Java, but nobody may enter it again
    /// (Java lets them, and whatever they place in it is lost with the room).
    pub deleted: bool,
    /// Set for a game's arena (Java: `isGameArena`, see `game::games`).
    pub arena: Option<Arena>,
    /// The last `remove_entity` stopped where Java's `leaveRoom` throws (`pool_room_leave`):
    /// the caller keeps the player's room.
    pub leave_threw: bool,
}

/// What an arena room knows of its game (Java: `RoomData.game`, `getGameLobby`).
pub struct Arena {
    /// The game's uid (`game::games::Game::uid`).
    pub game_uid: u64,
    pub game_type: crate::game::games::GameType,
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
        let Some(model) = world.room_models().get(&data.model).cloned() else {
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
        if model.name == "park_a" {
            mapping.bus_door = Some(world.infobus.door_open.clone());
        }
        Some(Arc::new_cyclic(|weak| RoomHandle {
            id,
            model_name: model.name.clone(),
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
                roller_task: None,
                rainbow_task: None,
                bot_party_task: None,
                dispose_generation: 0,
                leave_threw: false,
                bots_active: false,
                is_room_muted: false,
                last_bot_speech: None,
                self_ref: weak.clone(),
                deferred: Vec::new(),
                world: world.arc().as_ref().map(Arc::downgrade).unwrap_or_default(),
                idol: Default::default(),
                gamehall: Default::default(),
                camera: None,
                wobble: None,
                space_cafe: None,
                queue_after_loop: Vec::new(),
                deleted: false,
                arena: None,
            }),
        }))
    }

    /// Java: the `new Room()` of `Game.initialise`: a game's arena, with no row, items or
    /// visitors of its own, under an arena key of the world (not the room registry).
    pub fn new_arena(
        world: &World,
        model: Arc<RoomModel>,
        name: &str,
        game_uid: u64,
        game_type: crate::game::games::GameType,
    ) -> RoomRef {
        let key = world.new_arena_key();
        // Java: RoomData.fill(0, name, ""): no owner (a public room), no landscape (written
        // as "null").
        let data = rusbbo_storage::Room {
            id: key,
            name: name.to_string(),
            model: model.id.clone(),
            landscape: "null".into(),
            ..Default::default()
        };
        let mut mapping = RoomMapping::new(model.clone(), true);
        mapping.is_game_arena = true;
        mapping.regenerate(&[]);
        Arc::new_cyclic(|weak| RoomHandle {
            id: key,
            model_name: model.name.clone(),
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
                roller_task: None,
                rainbow_task: None,
                bot_party_task: None,
                dispose_generation: 0,
                leave_threw: false,
                bots_active: false,
                is_room_muted: false,
                last_bot_speech: None,
                self_ref: weak.clone(),
                deferred: Vec::new(),
                world: world.arc().as_ref().map(Arc::downgrade).unwrap_or_default(),
                idol: Default::default(),
                gamehall: Default::default(),
                camera: None,
                wobble: None,
                space_cafe: None,
                queue_after_loop: Vec::new(),
                deleted: false,
                arena: Some(Arena {
                    game_uid,
                    game_type,
                }),
            }),
        })
    }

    /// Java: `isGameArena`.
    pub fn is_game_arena(&self) -> bool {
        self.arena.is_some()
    }

    /// The room id the client is told (an arena's is 0, Java's `new Room()`).
    pub fn wire_id(&self) -> u32 {
        if self.is_game_arena() { 0 } else { self.id() }
    }

    pub fn id(&self) -> u32 {
        self.data.id
    }

    /// Java: `isPublicRoom`.
    pub fn is_public(&self) -> bool {
        self.data.owner_id == 0
    }

    /// Queues `task` to run once the room's lock is released ([`Deferred::Run`]).
    pub(crate) fn defer(&mut self, task: impl FnOnce(&World) + Send + 'static) {
        self.deferred.push(Deferred::Run(Box::new(task)));
    }

    pub fn take_deferred(&mut self) -> Vec<Deferred> {
        std::mem::take(&mut self.deferred)
    }

    /// A scheduler timer on this room (Java: `GameScheduler...schedule` with a lambda that uses
    /// the room): after `delay`, locks the room, runs `task` and then what it deferred. Nothing
    /// runs if the room was unloaded meanwhile. The task must not lock a player.
    pub fn schedule(
        &self,
        world: &World,
        delay: std::time::Duration,
        task: impl FnOnce(&World, &mut Room) + Send + 'static,
    ) {
        let room = self.self_ref.clone();
        crate::game::scheduler::schedule(world, delay, move |world| {
            let Some(room) = room.upgrade() else {
                return;
            };
            let deferred = {
                let mut r = room.lock();
                task(world, &mut r);
                r.take_deferred()
            };
            run_deferred(world, deferred);
        });
    }

    /// The registry handle of this room, while it exists.
    pub fn handle(&self) -> Option<RoomRef> {
        self.self_ref.upgrade()
    }

    /// Java: `Room.send`: to every player in the room.
    pub fn send(&self, msg: Message) {
        for e in self.entities.iter().filter(|e| e.is_player()) {
            e.session.send(msg.clone());
        }
    }

    /// `send` of a message Java composes for each player (a `PlayerMessageComposer`): built once
    /// for the Shockwave clients and once for the Flash ones.
    pub fn send_with(&self, compose: impl Fn(bool) -> Message) {
        self.send_except_with(compose, u64::MAX);
    }

    /// `send` of a message already built for Shockwave (`[0]`) and Flash (`[1]`) clients.
    pub fn send_pair(&self, msgs: [Message; 2]) {
        for e in self.entities.iter().filter(|e| e.is_player()) {
            e.session
                .send(msgs[usize::from(e.session.is_flash())].clone());
        }
    }

    /// [`Room::send_with`] to everyone but `except`.
    pub fn send_except_with(&self, compose: impl Fn(bool) -> Message, except: u64) {
        let mut built: [Option<Message>; 2] = [None, None];
        for e in self
            .entities
            .iter()
            .filter(|e| e.is_player() && e.session.id != except)
        {
            let flash = e.session.is_flash();
            let msg = built[usize::from(flash)].get_or_insert_with(|| compose(flash));
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

    /// Java: `getPlayers().size()`: bots and pets do not count.
    fn players(&self) -> i32 {
        self.entities.iter().filter(|e| e.is_player()).count() as i32
    }

    pub fn walker(&self, idx: usize) -> Walker {
        let e = &self.entities[idx];
        Walker {
            pet_nest: e.pet().map(|p| p.pet.item_id),
            instance_id: e.instance_id,
            position: e.position,
            swimming: e.statuses.contains(StatusType::Swim),
            has_pool_figure: !e.details.pool_figure.is_empty(),
            flash: e.is_player() && e.session.is_flash(),
        }
    }

    fn save_visitors(&self, world: &World) {
        if self.is_game_arena() {
            // Java's RoomDao.saveVisitors(0, ...) changes no row.
            return;
        }
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
        // Java keeps the effect on the player's `RoomEntity` from room to room, and the rest of
        // what its `reset` leaves.
        entity.effect_id = session.effect_id();
        if entity.is_player() {
            let carried = *session.carried.lock();
            entity.chat_spam_count = carried.chat_spam_count;
            entity.chat_spam_ticks = carried.chat_spam_ticks;
            entity.mute_time = carried.mute_time;
            entity.is_teleporting = carried.is_teleporting;
            entity.has_item_debug = carried.has_item_debug;
        }
        {
            let settings = world.settings.read();
            entity.reset_timers(
                settings.int("afk.timer.seconds"),
                settings.int("sleep.timer.seconds"),
            );
        }
        // Java: a game player on the way to the arena gets their object id as instance id and
        // may not walk until the game begins.
        let entering = world.games.entering_object_id(session.id);
        if let Some(object_id) = entering {
            entity.instance_id = object_id;
            entity.walking_allowed = false;
        }
        session.set_room_presence(Some((self.id(), entity.instance_id)));
        self.entities.push(entity);
        self.data.visitors_now = self.players();

        self.try_initialise(world);
        // Java: `tryInitialiseRoom` gives Abigail.Ryan the ghost effect. The teleporter arrival
        // is in `enter_room`.
        if let Some(idx) = self.entity_index(session.id) {
            if self.entities[idx].details.name == "Abigail.Ryan" {
                self.use_effect(idx, crate::game::effects::GHOST_EFFECT);
            }
        }
        if entering.is_some() {
            world.games.entered_game(session.id);
        }
        // Java's `sendStatusUpdate` comes at the end of `enter_room`, once the room lock is free.
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
        if self.is_game_arena() {
            // Java: an arena keeps its (no) items and runs its game's task instead of the room's
            // (started once, `RoomTaskManager.loadGameTasks`). Java's custom rooms (the
            // Hire-A-Room room) would keep their items too, but nobody can enter one (see
            // `teleport::room_hire_interact`).
            self.regenerate_map();
            self.start_arena_task(world);
            return;
        }
        self.mapping.items.clear();
        self.rights.clear();
        self.votes.clear();
        if self.is_public() {
            match world.store.public_items(&self.model.id) {
                Ok(rows) => {
                    let definition = |sprite: &str| world.items.definition_by_sprite(sprite);
                    self.mapping.items = items::public_items(&self.model.id, &rows, &definition);
                    self.show_initial_programs();
                }
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
        self.votes = counted_votes(world, self.id());
    }

    /// Java: `RoomTaskManager.startTasks`: the 500 ms entity task and 1 s status task, both
    /// started now; their first run already happened in [`Room::try_initialise`], so the task
    /// runs from the second one, 500 ms later. Restarting replaces the running ones, as Java's
    /// `scheduleTask` does.
    fn start_tasks(&mut self, world: &World) {
        self.stop_tasks();
        let Some(world) = world.arc() else {
            return; // Unit tests build rooms without a running world.
        };
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let handle = rt.spawn(tasks::run(Arc::downgrade(&world), self.self_ref.clone()));
        self.task = Some(handle.abort_handle());
        // Only private rooms run rollers.
        if !self.is_public() {
            let handle = rt.spawn(tasks::run_rollers(
                Arc::downgrade(&world),
                self.self_ref.clone(),
            ));
            self.roller_task = Some(handle.abort_handle());
        }
    }

    /// Java: `RoomTaskManager.loadGameTasks`: the game type's task, unless it runs already.
    fn start_arena_task(&mut self, world: &World) {
        if self.task.is_some() {
            return;
        }
        let Some(arena) = &self.arena else {
            return;
        };
        let period = arena.game_type.arena_task_period();
        let (Some(world), Ok(rt)) = (world.arc(), tokio::runtime::Handle::try_current()) else {
            return;
        };
        let handle = rt.spawn(tasks::run_arena(
            Arc::downgrade(&world),
            self.self_ref.clone(),
            period,
        ));
        self.task = Some(handle.abort_handle());
    }

    fn stop_tasks(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(task) = self.roller_task.take() {
            task.abort();
        }
        if let Some(task) = self.rainbow_task.take() {
            task.abort();
        }
        if let Some(task) = self.bot_party_task.take() {
            task.abort();
        }
    }

    /// Java: `RoomEntityManager.tryRoomEntry` (`G_STAT`) for the player of `session_id`.
    /// Returns whether it was the first entry.
    pub fn try_room_entry(&mut self, world: &World, session_id: u64) -> bool {
        let was_active = self.active;
        self.active = true;
        // The model trigger's onRoomEntry.
        if let Some(idx) = self.entity_index(session_id) {
            match self.model.trigger.as_str() {
                "battleball_lobby_trigger" => {
                    self.game_lobby_entry(world, crate::game::games::GameType::BattleBall, idx);
                }
                "snowstorm_lobby_trigger" => {
                    self.game_lobby_entry(world, crate::game::games::GameType::SnowStorm, idx);
                }
                "habbo_lido_trigger" | "diving_deck_trigger" | "rooftop_rumble_trigger" => {
                    self.pool_room_entry(world, idx);
                }
                "space_cafe_trigger" => self.space_cafe_room_entry(idx, !was_active),
                "infobus_park" => self.infobus_park_room_entry(world, idx),
                _ => {}
            }
        }
        if !was_active {
            if self.model.trigger == "flat_trigger" {
                self.load_pets(world);
            }
            self.add_bots(world);
        }
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
        if e.is_player() {
            *e.session.carried.lock() = crate::session::CarriedRoomState {
                chat_spam_count: e.chat_spam_count,
                chat_spam_ticks: e.chat_spam_ticks,
                mute_time: e.mute_time,
                is_teleporting: e.is_teleporting,
                has_item_debug: e.has_item_debug,
            };
        }
        // Java: the current item's onEntityLeave trigger, then the model's onRoomLeave.
        let current = self.mapping.tile(e.position).and_then(|t| t.highest_item);
        if let Some(item) = current {
            let session = e.session.clone();
            let interaction = self.mapping.items[item].interaction;
            self.on_entity_leave(
                interaction,
                Some(item),
                &session,
                e.is_player(),
                e.is_walking,
            );
        }
        if e.is_player() {
            // Only the cameras' triggers (Lido B, Rooftop Rumble) and the game lobbies' do
            // something; the flat, Habbo Lido, Infobus and Space Cafe ones do nothing.
            if self.pool_room_leave(e.session.id) {
                // Java's NullPointerException: no LOGOUT, no hotel view, no reset (the room user
                // keeps the room), and the entity stays on its tile.
                self.leave_threw = true;
                return None;
            }
            if crate::game::games::GameType::of_lobby_trigger(&self.model.trigger).is_some() {
                self.game_lobby_leave(world, e.session.id);
            }
        }
        self.mapping.remove_entity(e.position, e.instance_id);
        if let Some(next) = e.next_position {
            self.mapping.remove_entity(next, e.instance_id);
        }
        self.data.visitors_now = self.players();
        self.send(out_user::logout(e.instance_id));
        self.try_dispose(world);
        // Java: RoomPlayer.reset() closes the trade window.
        let trade = std::mem::take(&mut e.trade);
        self.close_trade_of(&e.session.clone(), e.details.user_id, trade);

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
        e.rolling = None;
        e.instance_id = -1;
        e.is_typing = false;
        e.needs_update = true;
        e.session.set_room_presence(None);

        if hotel_view {
            e.session.send(out_user::hotel_view());
        }
        // Java ends a game of Wobble Squabble here, but never does (see `wobble_squabble`).
        // Java: a player who leaves a room while in a game (not on the way to its arena) leaves
        // the game. The friends' status update follows once the player forgot the room
        // (`leave_current_room`, `Deferred::Left`).
        if e.is_player() {
            let session_id = e.session.id;
            world.games.run(world, Some(self), |games, op| {
                games.left_room(session_id, op)
            });
        }
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
            // Java: the room's event ends with it.
            if world.events.event_by_room(r.id()).is_some() {
                world.events.remove(&world.store, r.id());
            }
            r.save_pet_coordinates(&world);
            r.reset_item_states(&world);
            r.counter = 0;
            r.active = false;
            r.stop_tasks();
            r.mapping.items.clear();
            r.rights.clear();
            r.votes.clear();
            r.entities.clear();
            r.bots_active = false;
            r.last_bot_speech = None;
            r.camera = None;
            r.wobble = None;
            r.space_cafe = None;
            r.queue_after_loop.clear();
            if r.is_game_arena() {
                world.unregister_arena(&room);
            } else {
                world.unregister_room(&room);
            }
        });
    }

    /// Java: `RoomEntity.updateNewHeight`.
    pub(crate) fn update_new_height(&mut self, idx: usize) {
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

    /// Java: `RoomPlayer.walkTo`: walking somewhere new (not among the last
    /// `walk.spam.count` goals) resets the idle timers, which also wakes a sleeper.
    pub fn walk_to_player(&mut self, world: &World, idx: usize, x: i32, y: i32) -> bool {
        let goal = Position::new(x, y);
        let limit = world.settings.read().int("walk.spam.count");
        let e = &mut self.entities[idx];
        let unidle = !e.walk_history.contains(&goal);
        e.walk_history.push(goal);
        if e.walk_history.len() as i64 > limit {
            e.walk_history.remove(0);
        }
        let walking = self.walk_to(idx, x, y);
        if walking && unidle {
            self.reset_room_timer(world, idx);
        }
        walking
    }

    /// Java: `RoomEntity.walkTo`.
    pub fn walk_to(&mut self, idx: usize, x: i32, y: i32) -> bool {
        if self.is_redirected(idx, x, y) {
            return false;
        }
        if let Some(next) = self.entities[idx].next_position {
            let e = &mut self.entities[idx];
            let old = e.position;
            e.position.x = next.x;
            e.position.y = next.y;
            self.update_new_height(idx);
            let instance_id = self.entities[idx].instance_id;
            self.on_entity_step(idx, old);
            if self.entity_by_instance(instance_id) != Some(idx) {
                return false;
            }
        }
        if self.mapping.tile(Position::new(x, y)).is_none() {
            return false;
        }
        let goal = Position::new(x, y);
        self.entities[idx].goal = Some(goal);
        // Java: `:teleport` mode warps the user to the goal.
        if self.entities[idx].is_teleporting {
            // Unlike a teleporter arrival, no status follows on the next tick
            // (traces/staff-commands.json), unless the height changed (Java's `updateNewHeight`
            // from the goal's height 0: traces/wobble-squabble-tickets.json).
            self.warp_to(idx, goal, true, true);
            let e = &self.entities[idx];
            let msg = rooms_user::figure_change(
                e.instance_id,
                &e.details.figure,
                &e.details.sex,
                &e.details.motto,
            );
            self.send(msg);
            return true;
        }
        let walker = self.walker(idx);
        if !is_valid_tile(&self.mapping, Some(&walker), goal) {
            return false;
        }
        // Java: a bed leads to its pillow (BedInteractor).
        if let Some(bed) = self
            .mapping
            .highest_item(goal)
            .filter(|i| i.has(Behaviour::CanLayOnTop))
        {
            use crate::game::item::interactors::bed;
            if !bed::is_valid_pillow_tile(bed, goal) {
                let destination = bed::convert_to_pillow(goal, bed);
                self.walk_to(idx, destination.x, destination.y);
                return true;
            }
        }
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

    /// Java: `RoomEntity.stopWalking`, for callers outside the room's tick (the games).
    pub(crate) fn stop_walking_entity(&mut self, world: &World, idx: usize) {
        let _ = self.stop_walking(world, idx);
    }

    /// Java: `restartGame`'s `setInstanceId(objectId)` and `setPosition(spawn)` for a player in
    /// this arena, after `assignSpawnPoints`' `setWalking(false)` and `setNextPosition(null)`
    /// (the path stays, unused until the next walk).
    pub(crate) fn set_game_position(&mut self, session_id: u64, object_id: i32, spawn: Position) {
        let Some(idx) = self.entity_index(session_id) else {
            return;
        };
        let (old_id, old_position, next) = {
            let e = &self.entities[idx];
            (e.instance_id, e.position, e.next_position)
        };
        self.mapping.remove_entity(old_position, old_id);
        if let Some(next) = next {
            self.mapping.remove_entity(next, old_id);
        }
        let key = self.data.id;
        let e = &mut self.entities[idx];
        e.is_walking = false;
        e.next_position = None;
        e.instance_id = object_id;
        e.position = spawn;
        e.session.set_room_presence(Some((key, object_id)));
        self.mapping.add_entity(spawn, object_id);
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

        // Java: walkways and the door only apply to players (`EntityType.PLAYER`).
        if !e.is_player() {
            self.invoke_item(Some(world), idx, false, None);
            self.bot_stopped(world, idx);
            return None;
        }
        if !being_kicked {
            if let Some(w) = world.walkway_at(self, position) {
                // Java enters the target room right here; it runs once the room lock is released.
                // The user is no longer in this room when the tick sends its statuses.
                self.entities[idx].leaving_by_walkway = true;
                self.deferred.push(Deferred::Walkway {
                    from_room_id: self.id(),
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
        self.invoke_item(Some(world), idx, false, None);
        None
    }

    /// Java: `invokeItem(null, false)` right after an entry (the RCON trade setting).
    pub(crate) fn invoke_item_after_entry(&mut self, world: &World, idx: usize) {
        self.invoke_item(Some(world), idx, false, None);
    }

    /// Java: `RoomEntity.invokeItem`: runs the trigger of the item the entity stopped on.
    /// `old_position` is Java's `oldPosition`: standing there again is a rotation.
    fn invoke_item(
        &mut self,
        world: Option<&World>,
        idx: usize,
        instant_update: bool,
        old_position: Option<Position>,
    ) {
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
            if e.rolling.is_none()
                && (e.statuses.contains(StatusType::Sit) || e.statuses.contains(StatusType::Lay))
            {
                e.statuses.remove(StatusType::Sit);
                e.statuses.remove(StatusType::Lay);
            }
            if item.is_none() {
                if let Some((virtual_id, interaction)) = e.last_item_interaction.take() {
                    let (session, is_player, walking) =
                        (e.session.clone(), e.is_player(), e.is_walking);
                    let last = self.mapping.item_index(virtual_id);
                    self.on_entity_leave(interaction, last, &session, is_player, walking);
                }
            }
        }
        if let Some(i) = item {
            let is_rotation = old_position == Some(self.entities[idx].position);
            match self.mapping.items[i].interaction {
                Interaction::Chair | Interaction::LoveRandomizer => self.chair_stop(idx, i),
                Interaction::Bed => self.bed_stop(idx, i),
                Interaction::IdolVoteChair => self.idol_chair_stop(idx, i, is_rotation),
                Interaction::IdolScoreboard => self.idol_scoreboard_stop(idx, i, is_rotation),
                Interaction::GameTicTacToe
                | Interaction::GameChess
                | Interaction::GameBattleships
                | Interaction::GamePoker => self.game_trigger_stop(idx, i),
                _ => {}
            }
            self.pet_stopped_on(world, idx, i);
            self.pool_stop(world, idx, i);
            match self.mapping.items[i].interaction {
                Interaction::WsJoinQueue => self.ws_join_queue_stop(world, idx, i),
                Interaction::WsQueueTile => self.ws_queue_tile_stop(world, idx),
                Interaction::WsTileStart => self.ws_tile_start_stop(idx, i),
                _ => {}
            }
            let item = &self.mapping.items[i];
            self.entities[idx].last_item_interaction = Some((item.virtual_id, item.interaction));
        }
        self.update_new_height(idx);
        if instant_update {
            self.send_with(|f| out_user::user_statuses([&self.entities[idx]], &self.model.name, f));
        } else {
            self.entities[idx].needs_update = true;
        }
    }

    /// Java: `ChairInteractor.onEntityStop`.
    pub(crate) fn chair_stop(&mut self, idx: usize, item: usize) {
        let (top_height, rotation) = {
            let item = &self.mapping.items[item];
            (item.top_height, item.position.rotation())
        };
        let is_rolling = self.entities[idx].rolling.is_some();
        let head_rotation = self.entities[idx].position.head_rotation;
        self.entities[idx].position.set_rotation(rotation);
        self.dance(idx, 0);
        let e = &mut self.entities[idx];
        e.statuses
            .set(StatusType::Sit, crate::game::format_decimal(top_height));
        e.needs_update = true;
        // A user rolled onto a chair keeps looking where they looked.
        if is_rolling && e.look_timer > -1 {
            e.position.head_rotation = head_rotation;
        }
        self.queue_effect(idx);
    }

    /// Java: `RoomEntity.dance`.
    pub fn dance(&mut self, idx: usize, dance_id: i32) {
        let pool = pool::is_pool_model(&self.model.name);
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
        let afk = world.settings.read().int("afk.timer.seconds");
        self.reset_room_timer_to(Some(world), idx, afk);
    }

    /// Java: `RoomTimerManager.resetRoomTimer(afkTimer)`: `afk` seconds until the user is sent
    /// out for idling. Without `world`, the one the room was loaded in.
    pub(crate) fn reset_room_timer_to(&mut self, world: Option<&World>, idx: usize, afk: i64) {
        let loaded = if world.is_none() { self.world() } else { None };
        let Some(world) = world.or(loaded.as_deref()) else {
            return;
        };
        let sleep = world.settings.read().int("sleep.timer.seconds");
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

    /// Java: `RoomPlayer.talk` and `RoomEntity.talk(message, type)`. `whisper` holds the explicit
    /// receivers of a whisper (`talk(message, type, receivers)`); chat and shout go to everyone
    /// in the room who does not ignore the talker.
    pub fn talk(
        &mut self,
        world: &World,
        idx: usize,
        message: &[u8],
        kind: ChatType,
        whisper: Option<Vec<Receiver>>,
    ) {
        if message.ends_with(b"o/") {
            self.wave(idx);
            if message == b"o/" {
                return;
            }
        }
        let e = &mut self.entities[idx];
        e.chat_spam_count += 1;
        if e.chat_spam_ticks == -1 {
            e.chat_spam_ticks = 8;
            // Java: game ticks go twice as fast in SnowStorm.
            if e.is_player() && world.games.in_snowstorm_game(e.session.id) {
                e.chat_spam_ticks = 16;
            }
        }
        if e.chat_spam_count >= 6 {
            e.mute_time = now() + 30;
        }
        if e.mute_time > now() {
            return;
        }
        // Java: only players with the `roommute` command may talk in a muted room.
        if self.is_room_muted
            && self.entities[idx].is_player()
            && !crate::game::commands::has_permission_for(
                self.entities[idx].details.rank,
                "roommute",
                &self.entities[idx].details.groups,
            )
        {
            let e = &self.entities[idx];
            e.session.send(rooms_user::chat_message(
                ChatType::Whisper,
                e.instance_id,
                b"Shh! The room is currently muted by the moderators",
                0,
            ));
            self.reset_room_timer(world, idx);
            return;
        }
        let (message, receivers): (&[u8], Vec<Receiver>) = match whisper {
            Some(receivers) => (message, receivers),
            None => {
                let name = self.entities[idx].details.name.clone();
                let receivers = self
                    .entities
                    .iter()
                    .filter(|o| {
                        o.is_player() && !o.session.ignored.lock().contains(name.as_bytes())
                    })
                    .map(|o| Receiver {
                        session: o.session.clone(),
                        filter: o.details.wordfilter_enabled,
                    })
                    .collect();
                let message = if name == "Abigail.Ryan" {
                    &b""[..]
                } else {
                    message
                };
                (message, receivers)
            }
        };
        self.talk_to(world, idx, message, kind, receivers);
        self.entities[idx].mute_time = 0;
    }

    /// Java: `RoomEntity.talk(message, type, receivers)`.
    fn talk_to(
        &mut self,
        world: &World,
        idx: usize,
        message: &[u8],
        kind: ChatType,
        receivers: Vec<Receiver>,
    ) {
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

        let talker_session = self.entities[idx].session.id;
        if kind != ChatType::Whisper {
            // Everyone who hears it, except the talker, turns their head.
            for receiver in &receivers {
                if receiver.session.id == talker_session {
                    continue;
                }
                let Some(other) = self.entity_index(receiver.session.id) else {
                    continue;
                };
                if self.entities[other].is_sleeping {
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
        let gesture = chat_gesture(message);
        let filtered = {
            let settings = world.settings.read();
            receivers
                .iter()
                .any(|r| r.filter)
                .then(|| world.wordfilter.filter_sentence(&settings, message))
        };
        for receiver in receivers {
            let text = match (&filtered, receiver.filter) {
                (Some(filtered), true) => filtered.as_slice(),
                _ => message,
            };
            receiver
                .session
                .send(rooms_user::chat_message(kind, instance_id, text, gesture));
        }
        if self.entities[idx].is_player() && kind != ChatType::Whisper {
            self.bots_hear(world, idx, message);
            self.pets_hear(world, idx, message);
        }
        if save && self.entities[idx].is_player() {
            let user_id = self.entities[idx].details.user_id;
            world.chat_log.queue(user_id, self.wire_id(), message, kind);
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
        self.walk_to_player(world, idx, door.x, door.y);
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
    /// voted, and so does anyone whose user id, machine id or one of whose latest 20 IP
    /// addresses voted.
    pub fn has_voted(&self, world: &World, details: &EntityDetails) -> bool {
        let mins_since_joined = (now() - details.created_at) / 60;
        if mins_since_joined <= 60 {
            return true;
        }
        if self.votes.iter().any(|v| v.user_id == details.user_id) {
            return true;
        }
        if self.votes.iter().any(|v| {
            !is_blank(v.machine_id.as_bytes())
                && !is_blank(details.machine_id.as_bytes())
                && v.machine_id.to_lowercase() == details.machine_id.to_lowercase()
        }) {
            return true;
        }
        let history = world
            .store
            .ip_addresses(details.user_id, VOTE_IP_HISTORY_LIMIT)
            .unwrap_or_default();
        // Java: StringUtil.hasValue both ways, which is any address in common.
        self.votes
            .iter()
            .any(|v| v.ip_addresses.iter().any(|ip| history.contains(ip)))
    }

    /// Java: `Room.addVote`: the vote counts at once, the rating (never below 0) is stored, the
    /// owner and everyone in the room who has voted get it, and the vote is stored for 60 days.
    pub fn add_vote(&mut self, world: &World, answer: i32, details: &EntityDetails) {
        let history = world
            .store
            .ip_addresses(details.user_id, VOTE_IP_HISTORY_LIMIT)
            .unwrap_or_default();
        self.votes.push(Vote {
            user_id: details.user_id,
            vote: answer,
            // Java: String.join(",", history), split again by VoteData.
            ip_addresses: if history.is_empty() {
                vec![String::new()]
            } else {
                history
            },
            machine_id: details.machine_id.clone(),
        });
        let rating = self.votes.iter().map(|v| v.vote).sum::<i32>().max(0);
        self.data.rating = rating;
        if let Err(e) = world.store.update_room(self.id(), |r| r.rating = rating) {
            error!(room = self.id(), "saving the rating: {e}");
        }
        for e in self.entities.iter().filter(|e| e.is_player()) {
            if self.has_voted(world, &e.details) || self.is_owner(e.details.user_id) {
                e.session.send(out_rooms::update_votes(rating));
            }
        }
        let vote = rusbbo_storage::RoomVote {
            user_id: details.user_id,
            room_id: self.id(),
            vote: answer,
            expire_time: now() + VOTE_EXPIRE_SECONDS,
        };
        if let Err(e) = world.store.add_room_vote(&vote) {
            error!(room = self.id(), "saving a vote: {e}");
        }
    }

    /// Java: `Room.refreshRights(player, sendStatus)`: tells the user what they may do and sets
    /// their `flatctrl` status (`useradmin` for the owner). With `send_status` the status goes
    /// out on the next tick.
    pub fn refresh_rights(&mut self, idx: usize, send_status: bool) {
        let e = &self.entities[idx];
        if !e.is_player() {
            return;
        }
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

    /// Java: `RoomEntity.warp(position, instantUpdate, sendUserObject)` with both set: the user
    /// jumps to `position` (with its height and rotation), the room sees `USER_OBJECTS` and
    /// `USER_STATUSES`, then the item there is worked out again with its own status update.
    pub fn warp(&mut self, idx: usize, position: Position) {
        self.warp_to(idx, position, true, true);
        // traces/flat-teleport.json: after a warp the next room tick sends the user's status once
        // more. The Java code path that sets `needsUpdate` here was not found; this reproduces
        // what the oracle sends.
        self.entities[idx].needs_update = true;
    }

    /// Java: `RoomEntity.warp(position, instantUpdate, sendUserObject)`: the user jumps to
    /// `position`; with `instant_update` the room sees the user's status (and, with
    /// `send_user_object`, `USER_OBJECTS` first) and the item there is worked out again.
    pub(crate) fn warp_to(
        &mut self,
        idx: usize,
        position: Position,
        instant_update: bool,
        send_user_object: bool,
    ) {
        let (old, instance_id, next) = {
            let e = &self.entities[idx];
            (e.position, e.instance_id, e.next_position)
        };
        let had_tile = self.mapping.tile(old).is_some();
        self.mapping.remove_entity(old, instance_id);
        if let Some(next) = next {
            self.mapping.remove_entity(next, instance_id);
        }
        self.entities[idx].position = position;
        self.update_new_height(idx);
        self.mapping.add_entity(position, instance_id);
        if !instant_update {
            return;
        }
        let model = self.model.name.clone();
        if send_user_object {
            self.send_with(|f| out_user::user_objects([&self.entities[idx]], &model, f));
        }
        self.send_with(|f| out_user::user_statuses([&self.entities[idx]], &model, f));
        if had_tile {
            self.invoke_item(None, idx, true, Some(old));
        }
    }

    /// Java: `RoomEntity.useEffect`: a carried drink goes, then the room sees the effect.
    pub fn use_effect(&mut self, idx: usize, effect_id: i32) {
        if self.is_carrying(idx) {
            self.stop_carrying(idx);
        }
        let e = &mut self.entities[idx];
        e.effect_id = effect_id;
        e.session.set_effect_id(effect_id);
        let msg = crate::messages::outgoing::effects::user_avatar_effect(e.instance_id, effect_id);
        self.send(msg);
    }

    /// Java: `entityTask.getQueueAfterLoop().add(USER_AVATAR_EFFECT)` after sitting or lying
    /// down: the effect is shown again once the next tick's statuses are out.
    pub(crate) fn queue_effect(&mut self, idx: usize) {
        let e = &self.entities[idx];
        if e.is_using_effect() {
            let msg =
                crate::messages::outgoing::effects::user_avatar_effect(e.instance_id, e.effect_id);
            self.queue_after_loop.push(msg);
        }
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

/// Java: `StringUtil.filterInput(input, true)` on ISO-8859-1 bytes. With
/// `normalise.input.strings` on, Java's NFD splits an accented letter into its base letter and a
/// combining mark, which ISO-8859-1 cannot carry: the client gets `?` for it (`é` is `e?`).
pub fn filter_input(input: &[u8]) -> Vec<u8> {
    let filtered = input.iter().map(|&c| {
        if matches!(c, 1 | 2 | 9 | 10 | 12 | 13) {
            b' '
        } else {
            c
        }
    });
    if !crate::settings::normalise_input() {
        return filtered.collect();
    }
    let mut out = Vec::with_capacity(input.len());
    for c in filtered {
        match nfd_base(c) {
            Some(base) => {
                out.push(base);
                out.push(b'?');
            }
            None => out.push(c),
        }
    }
    out
}

/// The base letter of an ISO-8859-1 letter that NFD decomposes (its canonical decomposition is
/// that letter and one combining mark).
fn nfd_base(c: u8) -> Option<u8> {
    Some(match c {
        0xC0..=0xC5 => b'A',
        0xC7 => b'C',
        0xC8..=0xCB => b'E',
        0xCC..=0xCF => b'I',
        0xD1 => b'N',
        0xD2..=0xD6 => b'O',
        0xD9..=0xDC => b'U',
        0xDD => b'Y',
        0xE0..=0xE5 => b'a',
        0xE7 => b'c',
        0xE8..=0xEB => b'e',
        0xEC..=0xEF => b'i',
        0xF1 => b'n',
        0xF2..=0xF6 => b'o',
        0xF9..=0xFC => b'u',
        0xFD | 0xFF => b'y',
        _ => return None,
    })
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
                {
                    let mut p = session.player.lock();
                    if p.room_id == Some(room_id) {
                        p.room_id = None;
                    }
                }
                // Java: the end of `RoomEntityManager.leaveRoom`.
                crate::game::messenger::manager::send_status_update(world, &session);
            }
            Deferred::Walkway {
                from_room_id,
                session,
                target_room_id,
                destination,
            } => {
                let target = world
                    .loaded_room(target_room_id)
                    .or_else(|| Room::load(world, target_room_id));
                if let Some(target) = target {
                    enter_room_with(world, &session, target.clone(), destination, true, true);
                    // Java: the old room's task set `needsUpdate` after the walkway and its loop
                    // sends the walker's status, as it is in the new room, to the users left
                    // behind (traces/walkway-others.json); the new room does not.
                    let msgs = {
                        let t = target.lock();
                        t.entity_index(session.id).map(|i| {
                            let e = &t.entities[i];
                            [
                                out_user::user_statuses(std::iter::once(e), &t.model.name, false),
                                out_user::user_statuses(std::iter::once(e), &t.model.name, true),
                            ]
                        })
                    };
                    if let (Some(msgs), Some(old)) = (msgs, world.loaded_room(from_room_id)) {
                        old.lock().send_pair(msgs);
                    }
                }
            }
            Deferred::TradeClosed { sides } => trade::run_closed(world, sides),
            Deferred::TradeFinished { sides } => trade::run_finished(world, sides),
            Deferred::Run(task) => task(world),
            Deferred::StopTeleporting { session } => {
                session.player.lock().authenticate_teleporter = None;
            }
            Deferred::ClearAuthentication { session } => {
                let mut p = session.player.lock();
                p.authenticate_id = None;
                p.authenticate_teleporter = None;
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
    let (deferred, threw) = {
        let mut r = room.lock();
        r.remove_entity(world, session.id, hotel_view);
        (r.take_deferred(), std::mem::take(&mut r.leave_threw))
    };
    run_deferred(world, deferred);
    if threw {
        // Java's room user was never reset: it still names the room.
        session.player.lock().room_id = Some(room.id);
        return;
    }
    // Java: the end of `RoomEntityManager.leaveRoom`: friends' consoles stop showing the room.
    crate::game::messenger::manager::send_status_update(world, session);
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
    enter_room_with(world, session, room, destination, send_entry, false);
}

/// [`enter_room`]; `from_room_task` for an entry that Java runs on the old room's task thread
/// (a walkway): the new room's first statuses then go out before the entry messages
/// (`traces/pool-ladders.json`), as both are written from threads other than the connection's
/// event loop and the room task wins.
fn enter_room_with(
    world: &World,
    session: &Arc<Session>,
    room: RoomRef,
    destination: Option<Position>,
    send_entry: bool,
    from_room_task: bool,
) {
    let room = world.register_room(room);
    // A deleted room waiting to be unloaded is as good as gone (Java's `room == null`).
    if room.lock().deleted {
        return;
    }
    leave_current_room(world, session, false);
    let details = {
        let mut p = session.player.lock();
        if !p.logged_in || p.details.is_none() {
            return;
        }
        let group = crate::game::groups::shown_membership(world, &mut p);
        let Some(user) = p.details.as_ref() else {
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
            trade_enabled: user.trade_enabled,
            trade_ban_expiration: user.trade_ban_expiration,
            wordfilter_enabled: user.wordfilter_enabled,
            xp_month: p
                .statistics
                .int_value(crate::game::statistics::stat::XP_EARNED_MONTH),
            favourite_group: user.favourite_group,
            group,
            groups: crate::game::commands::joined_group_ids(&p),
        };
        p.room_id = Some(room.id);
        // Java: `RoomEntity.reset` sets the pixel tries for the room visit.
        p.pixel_tries = world
            .settings
            .read()
            .int("pixels.max.tries.single.room.instance");
        p.authenticate_id = None;
        // Java resets both teleporter authentications after the arrival.
        let teleporter = p.authenticate_teleporter.take();
        (details, teleporter)
    };
    let (details, teleporter) = details;
    // Java: `ROOMEEVENT_INFO(getEventByRoomId(...))`, composed before the room's lock is taken
    // (the hoster's name may need a player's lock).
    let event_info = if send_entry {
        crate::game::events::event_info(world, room.id, session.is_flash())
    } else {
        None
    };
    let deferred = {
        let mut r = room.lock();
        if r.deleted {
            // Deleted since the check above: do not join it after all.
            drop(r);
            let mut p = session.player.lock();
            if p.room_id == Some(room.id) {
                p.room_id = None;
            }
            drop(p);
            session.send(out_user::hotel_view());
            return;
        }
        r.add_entity(world, session, details, destination);
        if from_room_task {
            if let Some(msgs) = r.first_tick.take() {
                r.send_pair(msgs);
            } else if let Some(idx) = r.entity_index(session.id) {
                // A room that was already running: Java's old room sets `needsUpdate` after the
                // walkway (`EntityTask.processEntity` after `stopWalking`) and its loop takes it,
                // so this room never sends the newcomer's status (traces/infobus-door.json): the
                // old room sends it to its own users instead (`Deferred::Walkway`).
                r.entities[idx].needs_update = false;
            }
        }
        if let Some((teleporter_id, _)) = teleporter {
            crate::game::item::interactors::teleport::arrive(
                world,
                &mut r,
                session.id,
                teleporter_id,
            );
        }
        if send_entry {
            session.send(out_rooms::room_ready(r.wire_id(), &r.model.name));
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
                let voted = r.is_owner(details.user_id) || r.has_voted(world, details);
                session.send(out_rooms::update_votes(if voted {
                    r.data.rating
                } else {
                    -1
                }));
            }
            if let Some(msg) = &event_info {
                session.send(msg.clone());
            }
        }
        if let Some(msgs) = r.first_tick.take() {
            r.send_pair(msgs);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
    // Java: the end of `RoomEntityManager.enterRoom`: "let friends know I entered this room".
    crate::game::messenger::manager::send_status_update(world, session);
}

#[cfg(test)]
impl Room {
    /// A loaded room for unit tests: no world, no tasks.
    pub(crate) fn for_test(owner_id: u32, model: Arc<RoomModel>, items: Vec<items::Item>) -> Room {
        let data = rusbbo_storage::Room {
            id: 1000,
            owner_id,
            ..Default::default()
        };
        let mut mapping = RoomMapping::new(model.clone(), owner_id == 0);
        mapping.items = items;
        let mut room = Room {
            data,
            model,
            mapping,
            rights: Vec::new(),
            votes: Vec::new(),
            first_tick: None,
            entities: Vec::new(),
            active: true,
            counter: 0,
            task: None,
            roller_task: None,
            rainbow_task: None,
            bot_party_task: None,
            dispose_generation: 0,
            leave_threw: false,
            bots_active: false,
            is_room_muted: false,
            last_bot_speech: None,
            self_ref: Weak::new(),
            deferred: Vec::new(),
            world: Weak::new(),
            idol: Default::default(),
            gamehall: Default::default(),
            camera: None,
            wobble: None,
            space_cafe: None,
            queue_after_loop: Vec::new(),
            deleted: false,
            arena: None,
        };
        room.regenerate_map();
        room
    }

    /// Java: `invokeItem(null, false)` for the entity at `idx`.
    pub(crate) fn invoke_item_for_test(&mut self, idx: usize) {
        self.invoke_item(None, idx, false, None);
    }

    /// Puts a user at `position` and returns its index and the receiving end of its messages.
    pub(crate) fn add_test_user(
        &mut self,
        position: Position,
    ) -> (
        usize,
        tokio::sync::mpsc::UnboundedReceiver<crate::session::Outgoing>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let session = Arc::new(Session::new(self.counter as u64 + 1, tx));
        let mut e = RoomEntity::new(session, EntityDetails::default(), self.counter);
        self.counter += 1;
        e.position = position;
        self.mapping.add_entity(position, e.instance_id);
        self.entities.push(e);
        (self.entities.len() - 1, rx)
    }
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
        let model = Arc::new(RoomModel::parse(&rusbbo_storage::RoomModel {
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
            flash: false,
            pet_nest: None,
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
