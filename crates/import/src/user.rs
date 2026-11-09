//! Badges, mutes, effects, statistics and achievements (Java: `BadgeDao`, `UsersMutesDao`,
//! `EffectDao`, `PlayerStatisticsDao`, `AchievementDao`).

use std::collections::BTreeMap;

use anyhow::Result;
use mysql::Row;
use mysql::prelude::Queryable;
use rusbbo_storage::{
    Achievement, Badge, Effect, STATISTIC_COLUMNS, Store, UserAchievement, UserStatistics,
};

use crate::col;

/// Returns the number of rows copied.
pub fn import_user_data(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let mut count = 0;

    // Rows keep the order the table returns them in, which is the order the protocol shows.
    let mut badges: BTreeMap<u32, Vec<Badge>> = BTreeMap::new();
    let rows: Vec<Row> = conn.query("SELECT * FROM users_badges")?;
    for row in &rows {
        badges.entry(col(row, "user_id")?).or_default().push(Badge {
            code: col::<String>(row, "badge")?.trim_end().to_string(),
            equipped: col::<i32>(row, "equipped")? != 0,
            slot_id: col(row, "slot_id")?,
        });
    }
    for (user_id, list) in &badges {
        store.put_badges(*user_id, list)?;
    }
    count += rows.len();

    let mut mutes: BTreeMap<u32, Vec<i64>> = BTreeMap::new();
    let rows: Vec<(u32, i64)> = conn.query("SELECT user_id, muted_id FROM users_mutes")?;
    for (user_id, muted) in &rows {
        mutes.entry(*user_id).or_default().push(*muted);
    }
    for (user_id, list) in &mutes {
        store.put_mutes(*user_id, list)?;
    }
    count += rows.len();

    let mut effects: BTreeMap<u32, Vec<Effect>> = BTreeMap::new();
    let rows: Vec<Row> = conn.query("SELECT * FROM users_effects ORDER BY id")?;
    for row in &rows {
        effects
            .entry(col(row, "user_id")?)
            .or_default()
            .push(Effect {
                id: col(row, "id")?,
                effect_id: col(row, "effect_id")?,
                expiry_date: col(row, "expiry_date")?,
                activated: col::<i32>(row, "activated")? != 0,
            });
    }
    for (user_id, list) in &effects {
        store.put_effects(*user_id, list)?;
    }
    count += rows.len();

    let durations: Vec<(i32, i32)> =
        conn.query("SELECT effect_id, duration_seconds FROM settings_effects")?;
    store.put_effect_durations(&durations.iter().copied().collect())?;
    count += durations.len();

    let rows: Vec<Row> = conn.query(
        "SELECT *, UNIX_TIMESTAMP(club_gift_due) AS club_gift_due_ts FROM users_statistics",
    )?;
    for row in &rows {
        let mut stats = UserStatistics::default();
        for column in STATISTIC_COLUMNS {
            let value: Option<i64> = col(row, column)?;
            stats.values.insert(column.to_string(), value.unwrap_or(0));
        }
        // A datetime: Unix seconds, absent while NULL.
        let due: Option<i64> = col(row, "club_gift_due_ts")?;
        if let Some(due) = due {
            stats.values.insert("club_gift_due".into(), due);
        }
        store.put_statistics(col(row, "user_id")?, &stats)?;
    }
    count += rows.len();

    let rows: Vec<Row> = conn.query("SELECT * FROM achievements WHERE disabled = 0 ORDER BY id")?;
    let achievements: Vec<Achievement> = rows
        .iter()
        .map(|row| {
            Ok(Achievement {
                id: col(row, "id")?,
                name: col(row, "achievement")?,
                level: col(row, "level")?,
                reward_pixels: col(row, "reward_pixels")?,
                progress_needed: col(row, "progress_needed")?,
            })
        })
        .collect::<Result<_>>()?;
    store.put_achievements(&achievements)?;
    count += achievements.len();

    let mut user_achievements: BTreeMap<u32, Vec<UserAchievement>> = BTreeMap::new();
    let rows: Vec<Row> = conn.query("SELECT * FROM users_achievements")?;
    for row in &rows {
        user_achievements
            .entry(col(row, "user_id")?)
            .or_default()
            .push(UserAchievement {
                achievement_id: col(row, "achievement_id")?,
                progress: col(row, "progress")?,
            });
    }
    for (user_id, list) in &user_achievements {
        store.put_user_achievements(*user_id, list)?;
    }
    count += rows.len();
    Ok(count)
}
