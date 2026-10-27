//! Shared server state: storage, settings, the online players and the loaded rooms.
//!
//! Locking rules (docs/DESIGN.md): never hold a lock across `.await`; take a player's lock
//! before a room's; never hold two players' or two rooms' locks at once.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

use anyhow::Result;
use havana_storage::{RoomCategory, Store};
use parking_lot::{Mutex, RwLock};

use crate::game::catalogue::CatalogueManager;
use crate::game::item::ItemManager;
use crate::game::pathfinder::Position;
use crate::game::room::model::RoomModel;
use crate::game::room::walkways::{self, WalkwayEntrance};
use crate::game::room::{Room, RoomRef};
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
    /// Logged-in sessions by connection id (Java: `PlayerManager.players`).
    players: Mutex<HashMap<u64, Arc<Session>>>,
    /// Loaded rooms (Java: `RoomManager.roomMap`). Never lock a room while holding this.
    rooms: Mutex<HashMap<u32, RoomRef>>,
    /// Room models by `model_id` (Java: `RoomModelManager`), loaded at startup.
    pub room_models: HashMap<String, Arc<RoomModel>>,
    /// Navigator categories by id (Java: `NavigatorManager`), loaded at startup.
    pub room_categories: HashMap<u32, RoomCategory>,
    /// Java: `WalkwaysManager`, loaded at startup.
    pub walkways: Vec<WalkwayEntrance>,
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
        let items = ItemManager::load(&store)?;
        let catalogue = CatalogueManager::load(&store, &items)?;
        Ok(Self {
            store,
            settings: RwLock::new(settings),
            items,
            catalogue,
            players: Mutex::new(HashMap::new()),
            rooms: Mutex::new(HashMap::new()),
            room_models,
            room_categories,
            walkways,
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

    pub fn add_player(&self, session: &Arc<Session>) {
        self.players.lock().insert(session.id, session.clone());
    }

    pub fn remove_player(&self, session: &Session) {
        self.players.lock().remove(&session.id);
    }

    /// Snapshot of the online players, so callers can lock each one without holding the registry.
    pub fn players(&self) -> Vec<Arc<Session>> {
        self.players.lock().values().cloned().collect()
    }

    pub fn players_online(&self) -> usize {
        self.players.lock().len()
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

    /// Sessions logged in as `user_id`.
    pub fn sessions_of(&self, user_id: u32) -> Vec<Arc<Session>> {
        self.players()
            .into_iter()
            .filter(|s| s.player.lock().user_id() == Some(user_id))
            .collect()
    }
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
