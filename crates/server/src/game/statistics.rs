//! A player's `users_statistics` row (Java: `game/player/statistics/PlayerStatisticManager.java`).
//! Values are loaded at login; every change is also written to the store, one column at a time.

use rusbbo_storage::{Store, UserStatistics};
use tracing::error;

/// Column names (Java: `PlayerStatistic`).
pub mod stat {
    pub const DAYS_LOGGED_IN_ROW: &str = "days_logged_in_row";
    pub const ONLINE_TIME: &str = "online_time";
    pub const HAS_TUTORIAL: &str = "has_tutorial";
    pub const PLAYERS_GUIDED: &str = "players_guided";
    pub const GIFTS_DUE: &str = "gifts_due";
    pub const CLUB_MEMBER_TIME: &str = "club_member_time";
    pub const CLUB_MEMBER_TIME_UPDATED: &str = "club_member_time_updated";
    pub const MUTE_EXPIRES_AT: &str = "mute_expires_at";
    pub const WOBBLE_SQUABBLE_GAMES_WON: &str = "wobble_squabble_games_won";
    pub const BATTLEBALL_GAMES_WON: &str = "battleball_games_won";
    pub const BATTLEBALL_MONTHLY_SCORES: &str = "battleball_score_month";
    pub const BATTLEBALL_POINTS_ALL_TIME: &str = "battleball_score_all_time";
    pub const SNOWSTORM_GAMES_WON: &str = "snowstorm_games_won";
    pub const SNOWSTORM_MONTHLY_SCORES: &str = "snowstorm_score_month";
    pub const SNOWSTORM_POINTS_ALL_TIME: &str = "snowstorm_score_all_time";
    pub const XP_EARNED_MONTH: &str = "xp_earned_month";
    pub const XP_ALL_TIME: &str = "xp_all_time";
    pub const WOBBLE_SQUABBLE_MONTHLY_SCORES: &str = "wobble_squabble_score_month";
    pub const WOBBLE_SQUABBLE_POINTS_ALL_TIME: &str = "wobble_squabble_score_all_time";
}

#[derive(Default)]
pub struct PlayerStatistics {
    user_id: u32,
    row: UserStatistics,
}

impl PlayerStatistics {
    /// Java: the part of `Player.login` that reads the row, inserting a default one when the user
    /// has none (`newStatistics`).
    pub fn load(store: &Store, user_id: u32) -> Self {
        let row = match store.statistics(user_id) {
            Ok(Some(row)) => row,
            Ok(None) => {
                let row = UserStatistics::new_row();
                if let Err(e) = store.put_statistics(user_id, &row) {
                    error!(user = user_id, "creating statistics: {e}");
                }
                row
            }
            Err(e) => {
                error!(user = user_id, "loading statistics: {e}");
                UserStatistics::default()
            }
        };
        Self { user_id, row }
    }

    /// Java: `getIntValue` (an `Integer.parseInt`, so values are `i32`).
    pub fn int_value(&self, column: &str) -> i32 {
        self.row.values.get(column).copied().unwrap_or(0) as i32
    }

    /// Java: `getLongValue`.
    pub fn long_value(&self, column: &str) -> i64 {
        self.row.values.get(column).copied().unwrap_or(0)
    }

    /// Java: `setLongValue`: nothing happens for a column the row does not have.
    pub fn set_long_value(&mut self, store: &Store, column: &str, value: i64) {
        let Some(slot) = self.row.values.get_mut(column) else {
            return;
        };
        *slot = value;
        if let Err(e) = store.update_statistic(self.user_id, column, value) {
            error!(user = self.user_id, "saving statistic {column}: {e}");
        }
    }

    /// Java: `incrementValue`.
    pub fn increment_value(&mut self, store: &Store, column: &str, by: i32) {
        let value = i64::from(self.int_value(column)) + i64::from(by);
        self.set_long_value(store, column, value);
    }
}
