//! Per-user tables of the user package (Java: `BadgeDao`, `UsersMutesDao`, `EffectDao`,
//! `PlayerStatisticsDao`, `AchievementDao`).
//!
//! Layout: the Java tables are only ever read by `user_id` and written back as a whole, so each
//! per-user table here is `user_id -> Vec<record>`. The order of the vector is the order MariaDB
//! returns rows without an `ORDER BY` (insertion order), which the protocol exposes (badge lists).
//! - `users_badges`, `users_mutes`, `users_effects`, `users_achievements`: one vector per user.
//! - `users_statistics`: `user_id -> UserStatistics`.
//! - `settings_effects`: `effect_id -> duration_seconds`; `achievements`: `id -> Achievement`
//!   (static data, loaded at startup).

use std::collections::BTreeMap;

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const BADGES: TableDefinition<u32, &[u8]> = TableDefinition::new("users_badges");
const MUTES: TableDefinition<u32, &[u8]> = TableDefinition::new("users_mutes");
const EFFECTS: TableDefinition<u32, &[u8]> = TableDefinition::new("users_effects");
const EFFECT_DURATIONS: TableDefinition<i32, i32> = TableDefinition::new("settings_effects");
const STATISTICS: TableDefinition<u32, &[u8]> = TableDefinition::new("users_statistics");
const ACHIEVEMENTS: TableDefinition<u32, &[u8]> = TableDefinition::new("achievements");
const USER_ACHIEVEMENTS: TableDefinition<u32, &[u8]> = TableDefinition::new("users_achievements");

/// A row of `users_badges`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    pub code: String,
    pub equipped: bool,
    pub slot_id: i32,
}

/// A row of `users_effects`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub id: u32,
    pub effect_id: i32,
    pub expiry_date: i64,
    pub activated: bool,
}

/// The integer columns of `users_statistics` by column name (Java: `PlayerStatistic`). Every
/// column is present, so "set a statistic that is not in the map" (a no-op in Java) never occurs
/// for a known column.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserStatistics {
    pub values: BTreeMap<String, i64>,
}

/// The integer columns the importer copies; the other columns (`club_gift_due`, activation and
/// password recovery codes) are not used yet.
pub const STATISTIC_COLUMNS: &[&str] = &[
    "days_logged_in_row",
    "guestbook_unread_messages",
    "online_time",
    "battleball_score_month",
    "battleball_score_all_time",
    "snowstorm_score_month",
    "snowstorm_score_all_time",
    "wobble_squabble_score_month",
    "wobble_squabble_score_all_time",
    "xp_earned_month",
    "xp_all_time",
    "battleball_games_won",
    "snowstorm_games_won",
    "wobble_squabble_games_won",
    "guided_by",
    "has_tutorial",
    "players_guided",
    "newbie_room_layout",
    "newbie_gift",
    "newbie_gift_time",
    "gifts_due",
    "club_member_time",
    "club_member_time_updated",
    "is_guidable",
    "mute_expires_at",
];

impl UserStatistics {
    /// A fresh row, as `INSERT INTO users_statistics (user_id, ...)` leaves it: every column at
    /// its default (`has_tutorial` defaults to 1).
    pub fn new_row() -> Self {
        let mut values: BTreeMap<String, i64> = STATISTIC_COLUMNS
            .iter()
            .map(|c| (c.to_string(), 0))
            .collect();
        values.insert("has_tutorial".into(), 1);
        Self { values }
    }
}

/// A row of `achievements` (disabled rows are not imported).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Achievement {
    pub id: u32,
    pub name: String,
    pub level: i32,
    pub reward_pixels: i32,
    pub progress_needed: i32,
}

/// A row of `users_achievements`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAchievement {
    pub achievement_id: u32,
    pub progress: i32,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(BADGES)?;
    tx.open_table(MUTES)?;
    tx.open_table(EFFECTS)?;
    tx.open_table(EFFECT_DURATIONS)?;
    tx.open_table(STATISTICS)?;
    tx.open_table(ACHIEVEMENTS)?;
    tx.open_table(USER_ACHIEVEMENTS)?;
    Ok(())
}

impl Store {
    fn read_record<T: DeserializeOwned>(
        &self,
        table: TableDefinition<u32, &[u8]>,
        user_id: u32,
    ) -> Result<Option<T>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(table)?;
        table.get(user_id)?.map(|v| decode(v.value())).transpose()
    }

    fn write_record<T: Serialize>(
        &self,
        table: TableDefinition<u32, &[u8]>,
        user_id: u32,
        value: &T,
    ) -> Result<()> {
        let bytes = encode(value)?;
        let w = self.write()?;
        w.tx.open_table(table)?.insert(user_id, bytes.as_slice())?;
        w.commit()
    }

    /// Java: `BadgeDao.getBadges`.
    pub fn badges(&self, user_id: u32) -> Result<Vec<Badge>> {
        Ok(self.read_record(BADGES, user_id)?.unwrap_or_default())
    }

    /// Replaces the badges of a user (covers `newBadge`, `removeBadge` and `saveBadgeChanges`).
    pub fn put_badges(&self, user_id: u32, badges: &[Badge]) -> Result<()> {
        self.write_record(BADGES, user_id, &badges.to_vec())
    }

    /// Java: `UsersMutesDao.getMutedUsers`: the muted ids whose user exists, in insertion order.
    pub fn muted_users(&self, user_id: u32) -> Result<Vec<u32>> {
        let mut out = Vec::new();
        for id in self.mute_rows(user_id)? {
            if let Ok(id) = u32::try_from(id) {
                if self.user(id)?.is_some() {
                    out.push(id);
                }
            }
        }
        Ok(out)
    }

    /// Stores the raw mute rows of a user, including ids of users that do not exist (the Java
    /// table has no foreign key on `muted_id`; `-1` is stored for an unknown name).
    pub fn put_mutes(&self, user_id: u32, muted: &[i64]) -> Result<()> {
        self.write_record(MUTES, user_id, &muted.to_vec())
    }

    /// The raw mute rows of a user.
    pub fn mute_rows(&self, user_id: u32) -> Result<Vec<i64>> {
        Ok(self.read_record(MUTES, user_id)?.unwrap_or_default())
    }

    /// Java: `EffectDao.getEffects`.
    pub fn effects(&self, user_id: u32) -> Result<Vec<Effect>> {
        Ok(self.read_record(EFFECTS, user_id)?.unwrap_or_default())
    }

    pub fn put_effects(&self, user_id: u32, effects: &[Effect]) -> Result<()> {
        self.write_record(EFFECTS, user_id, &effects.to_vec())
    }

    /// Java: `EffectDao.getEffectTimes`.
    pub fn effect_durations(&self) -> Result<BTreeMap<i32, i32>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(EFFECT_DURATIONS)?;
        let mut out = BTreeMap::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            out.insert(k.value(), v.value());
        }
        Ok(out)
    }

    pub fn put_effect_durations(&self, durations: &BTreeMap<i32, i32>) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(EFFECT_DURATIONS)?;
            for (k, v) in durations {
                table.insert(*k, *v)?;
            }
        }
        w.commit()
    }

    /// Java: `PlayerStatisticsDao.getStatistics`.
    pub fn statistics(&self, user_id: u32) -> Result<Option<UserStatistics>> {
        self.read_record(STATISTICS, user_id)
    }

    pub fn put_statistics(&self, user_id: u32, stats: &UserStatistics) -> Result<()> {
        self.write_record(STATISTICS, user_id, stats)
    }

    /// Java: `PlayerStatisticsDao.updateStatistic`: `UPDATE users_statistics SET column = value`,
    /// one column, so it never overwrites the others. Does nothing without a row.
    pub fn update_statistic(&self, user_id: u32, column: &str, value: i64) -> Result<()> {
        let mut w = self.write()?;
        w.update_statistic(user_id, column, value)?;
        w.commit()
    }

    /// Java: `AchievementDao.getAchievements`, ordered by id.
    pub fn achievements(&self) -> Result<Vec<Achievement>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(ACHIEVEMENTS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    pub fn put_achievements(&self, achievements: &[Achievement]) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(ACHIEVEMENTS)?;
            for a in achievements {
                let bytes = encode(a)?;
                table.insert(a.id, bytes.as_slice())?;
            }
        }
        w.commit()
    }

    /// Java: `AchievementDao.getUserAchievements`.
    pub fn user_achievements(&self, user_id: u32) -> Result<Vec<UserAchievement>> {
        Ok(self
            .read_record(USER_ACHIEVEMENTS, user_id)?
            .unwrap_or_default())
    }

    pub fn put_user_achievements(&self, user_id: u32, rows: &[UserAchievement]) -> Result<()> {
        self.write_record(USER_ACHIEVEMENTS, user_id, &rows.to_vec())
    }
}

impl Writer {
    /// [`Store::update_statistic`] inside this transaction.
    pub fn update_statistic(&mut self, user_id: u32, column: &str, value: i64) -> Result<()> {
        let mut table = self.tx.open_table(STATISTICS)?;
        let current: Option<UserStatistics> =
            table.get(user_id)?.map(|v| decode(v.value())).transpose()?;
        if let Some(mut stats) = current {
            stats.values.insert(column.to_string(), value);
            let bytes = encode(&stats)?;
            table.insert(user_id, bytes.as_slice())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_user_vectors_round_trip_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        assert!(store.badges(1).unwrap().is_empty());
        let badges = vec![
            Badge {
                code: "B".into(),
                equipped: false,
                slot_id: 0,
            },
            Badge {
                code: "A".into(),
                equipped: true,
                slot_id: 3,
            },
        ];
        store.put_badges(1, &badges).unwrap();
        assert_eq!(store.badges(1).unwrap(), badges);

        // Mutes of users that do not exist are skipped when listing, but kept as rows.
        store
            .put_user(&crate::User {
                id: 2,
                username: "two".into(),
                ..Default::default()
            })
            .unwrap();
        store.put_mutes(1, &[2, 999, -1]).unwrap();
        assert_eq!(store.muted_users(1).unwrap(), vec![2]);
        assert_eq!(store.mute_rows(1).unwrap(), vec![2, 999, -1]);

        assert_eq!(UserStatistics::new_row().values["has_tutorial"], 1);
    }
}
