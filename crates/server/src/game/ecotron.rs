//! Java: `EcotronManager`: the recycler's rewards (`recycler_rewards`, read once and again on
//! RCON `refreshecotron`).

use std::sync::Arc;

use parking_lot::Mutex;
use rusbbo_storage::{RecyclerReward, Store};

#[derive(Default)]
pub struct EcotronManager {
    rewards: Mutex<Option<Arc<Vec<RecyclerReward>>>>,
}

/// `ItemDao.getEcotronItems`: `ORDER BY order_id ASC`.
fn load(store: &Store) -> Vec<RecyclerReward> {
    let mut rewards = store.records::<RecyclerReward>().unwrap_or_default();
    rewards.sort_by_key(|r| r.order_id);
    rewards
}

impl EcotronManager {
    pub fn rewards(&self, store: &Store) -> Arc<Vec<RecyclerReward>> {
        self.rewards
            .lock()
            .get_or_insert_with(|| Arc::new(load(store)))
            .clone()
    }

    /// `EcotronManager.reset`.
    pub fn reset(&self, store: &Store) {
        *self.rewards.lock() = Some(Arc::new(load(store)));
    }

    /// `getRewardsByChance`.
    pub fn rewards_by_chance(&self, store: &Store, chance: i32) -> Vec<RecyclerReward> {
        self.rewards(store)
            .iter()
            .filter(|r| r.chance == chance)
            .cloned()
            .collect()
    }
}

/// The odds of each prize level, from the rarest (`appendRewards`; level 1 is chance 0).
pub fn level_chance(level: i32) -> i32 {
    match level {
        5 => 2000,
        4 => 200,
        3 => 40,
        2 => 5,
        _ => 0,
    }
}
