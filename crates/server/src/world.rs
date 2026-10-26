//! Shared server state: storage, settings and the online players.
//!
//! Locking rules (docs/DESIGN.md): never hold a lock across `.await`; take a player's lock
//! before a room's; never hold two players' or two rooms' locks at once.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use havana_storage::Store;
use parking_lot::{Mutex, RwLock};

use crate::session::Session;
use crate::settings::Settings;
use crate::settings_defaults::{DEFAULTS, Value};

pub struct World {
    pub store: Store,
    pub settings: RwLock<Settings>,
    /// Logged-in sessions by connection id (Java: `PlayerManager.players`).
    players: Mutex<HashMap<u64, Arc<Session>>>,
}

impl World {
    pub fn new(store: Store) -> Result<Self> {
        let settings = Settings::new(load_settings(&store)?);
        Ok(Self {
            store,
            settings: RwLock::new(settings),
            players: Mutex::new(HashMap::new()),
        })
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
