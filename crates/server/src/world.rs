//! Shared server state: storage, settings, the online players and the loaded rooms.
//!
//! Locking rules (docs/DESIGN.md): never hold a lock across `.await`; take a player's lock
//! before a room's; never hold two players' or two rooms' locks at once.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

use anyhow::Result;
use parking_lot::{Mutex, RwLock};
use rusbbo_storage::{RoomCategory, Store};

use crate::game::achievements::AchievementManager;
use crate::game::catalogue::CatalogueManager;
use crate::game::item::ItemManager;
use crate::game::pathfinder::Position;
use crate::game::room::model::RoomModel;
use crate::game::room::walkways::{self, WalkwayEntrance};
use crate::game::room::{Room, RoomRef};
use crate::game::wordfilter::WordFilter;
use crate::session::Session;
use crate::settings::Settings;
use crate::settings_defaults::{DEFAULTS, Value};

/// The first key of the games' arenas, which are rooms with no row (see `game::games`); room ids
/// stay below it.
pub const ARENA_KEY_BASE: u32 = 2_000_000_000;

pub struct World {
    pub store: Store,
    pub settings: RwLock<Settings>,
    /// Item definitions (Java: `ItemManager`).
    pub items: ItemManager,
    /// Catalogue pages and items (Java: `CatalogueManager`).
    pub catalogue: CatalogueManager,
    /// Java: `AchievementManager`, loaded at startup.
    pub achievements: AchievementManager,
    /// Effect lengths in seconds by effect id (Java: `EffectsManager`, from `settings_effects`).
    pub effect_durations: std::collections::BTreeMap<i32, i32>,
    /// Logged-in sessions (Java: `PlayerManager.players`).
    players: Mutex<Players>,
    /// Loaded rooms (Java: `RoomManager.roomMap`). Never lock a room while holding this.
    rooms: Mutex<HashMap<u32, RoomRef>>,
    /// Room models by `model_id` (Java: `RoomModelManager`), loaded at startup.
    room_models: RwLock<Arc<HashMap<String, Arc<RoomModel>>>>,
    /// Navigator categories by id (Java: `NavigatorManager`), loaded at startup.
    room_categories: RwLock<Arc<HashMap<u32, RoomCategory>>>,
    /// Java: `WalkwaysManager`, loaded at startup.
    pub walkways: Vec<WalkwayEntrance>,
    /// Java: `WordfilterManager`, loaded at startup.
    pub wordfilter: WordFilter,
    /// Java: `CallForHelpManager`.
    pub cfh: crate::game::moderation::cfh::CallForHelpManager,
    /// Java: `InfobusManager`.
    pub infobus: crate::game::infobus::InfobusManager,
    /// Java: `GameManager` (BattleBall, SnowStorm).
    pub games: crate::game::games::GameManager,
    /// Java: `EventsManager`, loaded at startup.
    pub events: crate::game::events::EventsManager,
    /// Java: `ChatManager`'s queue of room chat for `room_chatlogs`.
    pub chat_log: crate::game::chatlog::ChatLogQueue,
    /// Java: `AdManager`.
    pub ads: crate::game::ads::AdManager,
    /// Java: `EcotronManager`.
    pub ecotron: crate::game::ecotron::EcotronManager,
    /// The games' arenas (Java keeps them out of `RoomManager`), by their keys.
    arenas: Mutex<HashMap<u32, RoomRef>>,
    next_arena_key: std::sync::atomic::AtomicU32,
    /// Set once the world is in its `Arc`, so room tasks can hold on to it.
    self_ref: OnceLock<Weak<World>>,
}

fn load_room_models(store: &Store) -> Result<HashMap<String, Arc<RoomModel>>> {
    Ok(store
        .room_models()?
        .iter()
        .map(|m| (m.model_id.clone(), Arc::new(RoomModel::parse(m))))
        .collect())
}

fn load_room_categories(store: &Store) -> Result<HashMap<u32, RoomCategory>> {
    Ok(store
        .room_categories()?
        .into_iter()
        .map(|c| (c.id, c))
        .collect())
}

impl World {
    pub fn new(store: Store) -> Result<Self> {
        let settings = Settings::new(load_settings(&store)?);
        crate::game::commands::init_group_permissions(&settings);
        let room_models = RwLock::new(Arc::new(load_room_models(&store)?));
        let room_categories = RwLock::new(Arc::new(load_room_categories(&store)?));
        let walkways = walkways::parse(&store.walkways()?);
        let wordfilter = WordFilter::new(store.wordfilter()?);
        let items = ItemManager::load(&store)?;
        let catalogue = CatalogueManager::load(&store, &items)?;
        let achievements = AchievementManager::new(store.achievements()?);
        let effect_durations = store.effect_durations()?;
        let games = crate::game::games::GameManager::new(&store);
        let events = crate::game::events::EventsManager::load(&store);
        Ok(Self {
            store,
            settings: RwLock::new(settings),
            items,
            catalogue,
            achievements,
            effect_durations,
            players: Mutex::new(Players::default()),
            rooms: Mutex::new(HashMap::new()),
            room_models,
            room_categories,
            walkways,
            wordfilter,
            cfh: Default::default(),
            infobus: Default::default(),
            games,
            events,
            chat_log: Default::default(),
            ads: Default::default(),
            ecotron: Default::default(),
            arenas: Mutex::new(HashMap::new()),
            next_arena_key: std::sync::atomic::AtomicU32::new(ARENA_KEY_BASE),
            self_ref: OnceLock::new(),
        })
    }

    /// The room models by name (Java: `RoomModelManager`).
    pub fn room_models(&self) -> Arc<HashMap<String, Arc<RoomModel>>> {
        self.room_models.read().clone()
    }

    /// The navigator's categories by id (Java: `NavigatorManager`).
    pub fn room_categories(&self) -> Arc<HashMap<u32, RoomCategory>> {
        self.room_categories.read().clone()
    }

    /// Java: `RoomModelManager.reset` (RCON `refreshroommodels`): loaded rooms keep the model
    /// they were loaded with.
    pub fn reload_room_models(&self) -> Result<()> {
        *self.room_models.write() = Arc::new(load_room_models(&self.store)?);
        Ok(())
    }

    /// Java: `NavigatorManager.reset` (RCON `refreshnavigator`).
    pub fn reload_room_categories(&self) -> Result<()> {
        *self.room_categories.write() = Arc::new(load_room_categories(&self.store)?);
        Ok(())
    }

    /// Wraps the world in its `Arc` and remembers it for [`World::arc`].
    pub fn into_arc(self) -> Arc<World> {
        let world = Arc::new(self);
        let _ = world.self_ref.set(Arc::downgrade(&world));
        world
    }

    /// The world's own `Arc`, for spawning tasks from synchronous code.
    pub fn arc(&self) -> Option<Arc<World>> {
        self.self_ref.get().and_then(Weak::upgrade)
    }

    /// A loaded room (Java: `RoomManager.getRoomById` when the room is in the map), or a game's
    /// arena by its key.
    pub fn loaded_room(&self, id: u32) -> Option<RoomRef> {
        if id >= ARENA_KEY_BASE {
            return self.arenas.lock().get(&id).cloned();
        }
        self.rooms.lock().get(&id).cloned()
    }

    /// A key for a new arena (see [`ARENA_KEY_BASE`]).
    pub fn new_arena_key(&self) -> u32 {
        self.next_arena_key
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Makes an arena reachable by its key while players are in it.
    pub fn register_arena(&self, room: &RoomRef) {
        self.arenas
            .lock()
            .entry(room.id)
            .or_insert_with(|| room.clone());
    }

    /// Forgets an arena nobody is in (its game may still hold it, and register it again).
    pub fn unregister_arena(&self, room: &RoomRef) {
        let mut arenas = self.arenas.lock();
        if arenas.get(&room.id).is_some_and(|r| Arc::ptr_eq(r, room)) {
            arenas.remove(&room.id);
        }
    }

    /// Every arena players can be in, so callers can lock them one at a time.
    pub fn arenas(&self) -> Vec<RoomRef> {
        self.arenas.lock().values().cloned().collect()
    }

    /// Every loaded room, so callers can lock them one at a time.
    pub fn loaded_rooms(&self) -> Vec<RoomRef> {
        self.rooms.lock().values().cloned().collect()
    }

    /// The loaded room, or the room read from storage without registering it (Java:
    /// `RoomManager.getRoomById`).
    pub fn room_by_id(&self, id: u32) -> Option<RoomRef> {
        self.loaded_room(id).or_else(|| Room::load(self, id))
    }

    /// Java: `RoomManager.addRoom`: registers `room` unless that id is loaded already, and
    /// returns the registered one.
    pub fn register_room(&self, room: RoomRef) -> RoomRef {
        self.rooms.lock().entry(room.id).or_insert(room).clone()
    }

    /// Java: `RoomManager.removeRoom`, if `room` is still the registered one.
    pub fn unregister_room(&self, room: &RoomRef) {
        let mut rooms = self.rooms.lock();
        if rooms.get(&room.id).is_some_and(|r| Arc::ptr_eq(r, room)) {
            rooms.remove(&room.id);
        }
    }

    /// Java: `WalkwaysManager.getWalkway(room, position)`.
    pub fn walkway_at(&self, room: &Room, position: Position) -> Option<&WalkwayEntrance> {
        if !room.is_public() {
            return None;
        }
        self.walkways
            .iter()
            .filter(|w| w.room_id == room.id())
            .find(|w| w.from_coords.contains(&position))
    }

    /// Visitors now and maximum of a room, read from the loaded room or from storage (Java:
    /// `getRoomById(id).getData()`). Locks that room; hold no other room's lock.
    pub fn room_visitors(&self, id: u32) -> Option<(i32, i32)> {
        let data = match self.loaded_room(id) {
            Some(room) => room.lock().data.clone(),
            None => self.store.room(id).ok().flatten()?,
        };
        let (now, max) = crate::game::navigator::game_visitors(self, &data);
        Some((data.visitors_now + now, data.visitors_max + max))
    }

    /// Java: `RoomManager.getChildRooms`: public rooms connected to `room_id` by walkways.
    pub fn child_rooms(&self, room_id: u32, is_public: bool) -> Vec<u32> {
        let mut list: Vec<u32> = Vec::new();
        if !is_public {
            return list;
        }
        self.sub_rooms(room_id, &mut list);
        // Java iterates the list while getSubRooms may append to it; by index here.
        let mut i = 0;
        while i < list.len() {
            let id = list[i];
            self.sub_rooms(id, &mut list);
            i += 1;
        }
        list
    }

    /// Java: `RoomManager.getSubRooms`.
    fn sub_rooms(&self, id: u32, list: &mut Vec<u32>) {
        for w in &self.walkways {
            if w.target_room_id == id
                && w.room_id != id
                && !list.contains(&w.room_id)
                && !list.contains(&w.target_room_id)
                && self.room_visitors(w.room_id).is_some()
            {
                list.push(w.room_id);
            }
        }
    }

    /// Registers `session` as the one session of `user_id` and returns the sessions it replaces,
    /// for the caller to kick (Java: `disconnectSession` then `addPlayer`). Both happen under the
    /// registry's lock, so two logins of the same user racing each other (the same ticket on
    /// two connections) never both stay registered: the later one replaces the earlier.
    pub fn register_login(&self, session: &Arc<Session>, user_id: u32) -> Vec<Arc<Session>> {
        let mut players = self.players.lock();
        let mut replaced = Vec::new();
        if let Some(previous) = players.by_user.insert(user_id, session.id) {
            if previous != session.id {
                replaced.extend(players.sessions.remove(&previous));
            }
        }
        players.sessions.insert(session.id, session.clone());
        replaced
    }

    pub fn remove_player(&self, session: &Session) {
        let mut players = self.players.lock();
        if players.sessions.remove(&session.id).is_some() {
            players.by_user.retain(|_, id| *id != session.id);
        }
    }

    /// Snapshot of the online players, so callers can lock each one without holding the registry.
    pub fn players(&self) -> Vec<Arc<Session>> {
        self.players.lock().sessions.values().cloned().collect()
    }

    pub fn players_online(&self) -> usize {
        self.players.lock().sessions.len()
    }

    /// The session logged in as `user_id` (Java: `PlayerManager.getPlayerById`). Locks each
    /// player in turn, so the caller must not hold a player lock.
    pub fn player_by_id(&self, user_id: u32) -> Option<Arc<Session>> {
        self.players()
            .into_iter()
            .find(|s| s.player.lock().user_id() == Some(user_id))
    }

    /// The session logged in under `name`, ignoring case (Java: `PlayerManager.getPlayerByName`).
    /// Locks each player in turn, so the caller must not hold a player lock.
    pub fn player_by_name(&self, name: &str) -> Option<Arc<Session>> {
        let name = name.to_lowercase();
        self.players().into_iter().find(|s| {
            let p = s.player.lock();
            p.logged_in
                && p.details
                    .as_ref()
                    .is_some_and(|d| d.username.to_lowercase() == name)
        })
    }

    /// Java: `TextsManager.getValue`: the external text of `key`, or the key itself.
    pub fn text(&self, key: &str) -> String {
        self.store
            .external_text(key)
            .ok()
            .flatten()
            .unwrap_or_else(|| key.to_string())
    }
}

impl World {
    /// Java: `GameConfiguration.reset`: reads the settings again (`:setconfig`, `:reload`).
    pub fn reload_settings(&self) {
        match load_settings(&self.store) {
            Ok(values) => *self.settings.write() = Settings::new(values),
            Err(e) => tracing::error!("reloading settings: {e}"),
        }
    }
}

/// The player registry: sessions by connection id, and which session each user is logged in on.
#[derive(Default)]
struct Players {
    /// By session id, so in connection order (Java: `PlayerManager.players`, a list each new
    /// connection is added to).
    sessions: std::collections::BTreeMap<u64, Arc<Session>>,
    by_user: HashMap<u32, u64>,
}

/// Reads `settings`, first writing every default that is missing (Java: `GameConfiguration`).
fn load_settings(store: &Store) -> Result<std::collections::BTreeMap<String, String>> {
    let existing = store.settings()?;
    let mut w = store.write()?;
    for (key, default) in DEFAULTS {
        if existing.contains_key(*key) {
            continue;
        }
        let value = match default {
            Value::Text(v) => v.to_string(),
            Value::TodayDayMonth => chrono::Local::now().format("%d-%m").to_string(),
            Value::Today => crate::game::today_short(),
        };
        w.put_setting(key, &value)?;
    }
    // GameConfigWriter: an empty `groups.ids.permission.<command>` for every staff command.
    for command in crate::game::commands::commands() {
        let key = format!("groups.ids.permission.{}", command.aliases[0]);
        if command.rank > 1 && !existing.contains_key(&key) {
            w.put_setting(&key, "")?;
        }
    }
    w.commit()?;
    Ok(store.settings()?)
}
