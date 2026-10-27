//! A player's badges (Java: `game/badges/BadgeManager.java`). Loaded at login; every change is
//! written to the store as the whole list (the Java tables are only read by user).

use havana_storage::{Badge, Store};
use tracing::error;

use crate::messages::outgoing::user_badges as out;
use crate::session::Session;

#[derive(Default)]
pub struct BadgeManager {
    user_id: u32,
    badges: Vec<Badge>,
    /// Java: `badgesToSave`, flushed by `saveQueuedBadges`.
    dirty: bool,
}

impl BadgeManager {
    /// Java: `loadBadges`.
    pub fn load(store: &Store, user_id: u32) -> Self {
        let badges = store.badges(user_id).unwrap_or_else(|e| {
            error!(user = user_id, "loading badges: {e}");
            Vec::new()
        });
        Self {
            user_id,
            badges,
            dirty: false,
        }
    }

    pub fn badges(&self) -> &[Badge] {
        &self.badges
    }

    /// Java: `getEquippedBadges`, in list order (not slot order).
    pub fn equipped(&self) -> Vec<&Badge> {
        self.badges.iter().filter(|b| b.equipped).collect()
    }

    /// Java: `hasBadge`: case-insensitive.
    pub fn has_badge(&self, code: &str) -> bool {
        self.find(code).is_some()
    }

    fn find(&self, code: &str) -> Option<usize> {
        let code = code.to_lowercase();
        self.badges
            .iter()
            .position(|b| b.code.to_lowercase() == code)
    }

    /// Java: `refreshBadges`.
    pub fn refresh(&self, session: &Session) {
        session.send(out::available_badges(&self.badges, &self.equipped()));
    }

    /// Java: `changeBadge`: sets the worn flag and slot of a badge the player has. Saved by
    /// [`BadgeManager::save_queued`].
    pub fn change_badge(&mut self, code: &str, equipped: bool, slot_id: i32) {
        let Some(i) = self.find(code) else { return };
        self.badges[i].equipped = equipped;
        self.badges[i].slot_id = slot_id;
        self.dirty = true;
    }

    /// Java: `tryAddBadge`: gives the badge, taking `remove` away, and tells the client. Returns
    /// whether the badge was new.
    pub fn try_add_badge(
        &mut self,
        store: &Store,
        session: &Session,
        code: &str,
        remove: Option<&str>,
        level: i32,
    ) -> bool {
        if self.has_badge(code) {
            return false;
        }
        if let Some(remove) = remove {
            self.remove_badge(store, remove);
        }
        self.badges.push(Badge {
            code: code.to_string(),
            equipped: false,
            slot_id: 0,
        });
        self.save(store);
        session.send(out::achievement_notification(code, remove, level));
        true
    }

    /// Java: `removeBadge`.
    pub fn remove_badge(&mut self, store: &Store, code: &str) {
        let Some(i) = self.find(code) else { return };
        self.badges.remove(i);
        self.save(store);
    }

    /// Java: `saveQueuedBadges`.
    pub fn save_queued(&mut self, store: &Store) {
        if self.dirty {
            self.save(store);
        }
    }

    fn save(&mut self, store: &Store) {
        self.dirty = false;
        if let Err(e) = store.put_badges(self.user_id, &self.badges) {
            error!(user = self.user_id, "saving badges: {e}");
        }
    }
}
