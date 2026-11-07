//! Shared server state: storage, settings, the online players and the loaded rooms.
//!
//! Locking rules (docs/DESIGN.md): never hold a lock across `.await`; take a player's lock
//! before a room's; never hold two players' or two rooms' locks at once.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

use anyhow::Result;
use havana_storage::{RoomCategory, Store};
use parking_lot::{Mutex, RwLock};

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
    pub room_models: HashMap<String, Arc<RoomModel>>,
    /// Navigator categories by id (Java: `NavigatorManager`), loaded at startup.
    pub room_categories: HashMap<u32, RoomCategory>,
    /// Java: `WalkwaysManager`, loaded at startup.
    pub walkways: Vec<WalkwayEntrance>,
    /// Java: `WordfilterManager`, loaded at startup.
    pub wordfilter: WordFilter,
    /// Set once the world is in its `Arc`, so room tasks can hold on to it.
    self_ref: OnceLock<Weak<World>>,
}

impl World {
    pub fn new(store: Store) -> Result<Self> {
        let settings = Settings::new(load_settings(&store)?);
        let room_models = store
            .room_models()?
            .iter()
            .map(|m| (m.model_id.clone(), Arc::new(RoomModel::parse(m))))
            .collect();
        let room_categories = store
            .room_categories()?
            .into_iter()
            .map(|c| (c.id, c))
            .collect();
        let walkways = walkways::parse(&store.walkways()?);
        let wordfilter = WordFilter::new(store.wordfilter()?);
        let items = ItemManager::load(&store)?;
        let catalogue = CatalogueManager::load(&store, &items)?;
        let achievements = AchievementManager::new(store.achievements()?);
        let effect_durations = store.effect_durations()?;
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
            self_ref: OnceLock::new(),
        })
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

    /// A loaded room (Java: `RoomManager.getRoomById` when the room is in the map).
    pub fn loaded_room(&self, id: u32) -> Option<RoomRef> {
        self.rooms.lock().get(&id).cloned()
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
        if let Some(room) = self.loaded_room(id) {
            let r = room.lock();
            return Some((r.data.visitors_now, r.data.visitors_max));
        }
        let data = self.store.room(id).ok().flatten()?;
        Some((data.visitors_now, data.visitors_max))
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

/// The player registry: sessions by connection id, and which session each user is logged in on.
#[derive(Default)]
struct Players {
    sessions: HashMap<u64, Arc<Session>>,
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
    w.commit()?;
    Ok(store.settings()?)
}
