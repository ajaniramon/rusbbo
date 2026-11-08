//! Shared data helpers for the controllers (the Java DAOs the website uses).

use std::collections::HashMap;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::Tag;

use crate::world::World;

/// Java (web process): `GameConfiguration.getString(key)`: the setting, else the key itself.
pub fn cfg_string(world: &World, key: &str) -> String {
    let s = world.settings.read();
    if s.contains(key) {
        s.string(key).to_string()
    } else {
        key.to_string()
    }
}

/// Java: `GameConfiguration.getBoolean`: `true` (any case) or `1`.
pub fn cfg_bool(world: &World, key: &str) -> bool {
    let s = world.settings.read();
    let v = s.string(key);
    v.eq_ignore_ascii_case("true") || v == "1"
}

/// Java: `GameConfiguration.getInteger`.
pub fn cfg_int(world: &World, key: &str) -> i64 {
    world.settings.read().int(key)
}

/// A Havana `Pair` (`getKey`/`getLeft`, `getValue`/`getRight`).
pub fn pair(left: impl Into<Value>, right: impl Into<Value>) -> Value {
    let (l, r) = (left.into(), right.into());
    Map::new()
        .with("key", l.clone())
        .with("left", l)
        .with("value", r.clone())
        .with("right", r)
        .into()
}

/// Java: `TagDao.getPopularTags(n)`: `SELECT tag, COUNT(*) ... GROUP BY tag ORDER BY quantity DESC
/// LIMIT n`. Ties keep MariaDB's group order (the tag's collation order).
pub fn popular_tags(world: &World, n: usize) -> Vec<(String, i64)> {
    let tags = world.store.records::<Tag>().unwrap_or_default();
    let mut counts: HashMap<String, (String, i64)> = HashMap::new();
    for t in tags {
        let e = counts
            .entry(t.tag.to_lowercase())
            .or_insert_with(|| (t.tag.clone(), 0));
        e.1 += 1;
    }
    let mut v: Vec<(String, (String, i64))> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.1.cmp(&a.1.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter()
        .take(n)
        .map(|(_, (tag, c))| (tag, c))
        .collect()
}

/// Java: `TagDao.getUserTags`.
pub fn user_tags(world: &World, user_id: u32) -> Vec<String> {
    world
        .store
        .records_where::<Tag>(|t| t.user_id == user_id && t.room_id == 0 && t.group_id == 0)
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.tag)
        .collect()
}

/// Java: `WebSettingsConfigWriter`'s defaults, written into `settings` when the website starts
/// (`GameConfiguration.getInstance(new WebSettingsConfigWriter())`).
pub const WEB_DEFAULTS: &[(&str, &str)] = &[
    ("site.name", "Habbo"),
    ("site.path", "http://localhost"),
    ("static.content.path", "http://localhost"),
    ("hotel.check.online", "true"),
    ("loader.game.ip", "127.0.0.1"),
    ("loader.game.port", "12321"),
    ("loader.mus.ip", "127.0.0.1"),
    ("loader.mus.port", "12322"),
    ("loader.dcr", "http://localhost/dcr/v31/habbo.dcr?"),
    (
        "loader.external.variables",
        "http://localhost/dcr/v31/gamedata/external_variables.txt?",
    ),
    (
        "loader.external.texts",
        "http://localhost/dcr/v31/gamedata/external_texts.txt?",
    ),
    (
        "loader.flash.base",
        "http://localhost/gordon/RELEASE39-22643-22891-200911110035_07c3a2a30713fd5bea8a8caf07e33438/",
    ),
    (
        "loader.flash.swf",
        "http://localhost/gordon/RELEASE39-22643-22891-200911110035_07c3a2a30713fd5bea8a8caf07e33438/Habbo.swf",
    ),
    (
        "loader.flash.external.texts",
        "http://localhost/flash/gamedata/external_flash_texts.txt",
    ),
    (
        "loader.flash.external.variables",
        "http://localhost/flash/gamedata/external_variables.txt",
    ),
    ("registration.disabled", "false"),
    ("collectables.page", "51"),
    ("group.purchase.cost", "20"),
    ("group.default.badge", "b0503Xs09114s05013s05015"),
    ("hot.groups.community.limit", "8"),
    ("hot.groups.limit", "10"),
    ("discussions.per.page", "10"),
    ("discussions.replies.per.page", "10"),
    (
        "alerts.gift.message",
        "A new gift has arrived. This time you received a %item_name%.",
    ),
    ("homepage.template.file", "index_v32"),
    ("free.month.hc.registration", "true"),
    ("max.tags.users", "8"),
    ("max.tags.groups", "20"),
    ("trade.email.verification", "false"),
    ("email.smtp.enable", "false"),
    ("email.static.content.path", "http://localhost"),
    ("email.smtp.host", ""),
    ("email.smtp.port", "465"),
    ("email.smtp.login.username", ""),
    ("email.smtp.login.password", ""),
    ("email.smtp.from.email", ""),
    ("email.smtp.from.name", ""),
    ("maintenance", "false"),
];

/// Writes the missing web defaults and reloads the settings.
pub fn apply_web_defaults(world: &World) -> rusbbo_storage::Result<()> {
    let existing = world.store.settings()?;
    let mut w = world.store.write()?;
    for (k, v) in WEB_DEFAULTS {
        if !existing.contains_key(*k) {
            w.put_setting(k, v)?;
        }
    }
    w.commit()?;
    world.reload_settings();
    Ok(())
}

/// Java: `PlayerDetails.isBanned`.
pub fn active_ban(world: &World, user: &rusbbo_storage::User) -> Option<rusbbo_storage::Ban> {
    crate::game::moderation::active_ban(world, user.id, &user.machine_id)
}

/// Java: `PlayerDetails.canSelectRoom`: no room picked yet.
pub fn can_select_room(_world: &World, user: &rusbbo_storage::User) -> bool {
    user.selected_room_id == 0
}

/// A `users_statistics` column (Java: `PlayerStatisticsDao.getStatisticLong`).
pub fn statistic(world: &World, user_id: u32, column: &str) -> i64 {
    world
        .store
        .statistics(user_id)
        .ok()
        .flatten()
        .and_then(|s| s.values.get(column).copied())
        .unwrap_or(0)
}

/// Java: `PlayerStatisticsDao.updateStatistic`.
pub fn set_statistic(world: &World, user_id: u32, column: &str, value: i64) {
    if let Err(e) = world.store.update_statistic(user_id, column, value) {
        tracing::error!("web: statistic {column}: {e}");
    }
}

/// Java: `RoomSelectionHandler.selectRoom`: creates the user's pre-decorated room. TODO(web): the
/// room templates (phase 1, with the room selection habblet).
pub fn select_room(_world: &World, _user_id: u32, _room_type: i32) -> bool {
    false
}
