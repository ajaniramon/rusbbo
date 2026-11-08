//! Shared data helpers for the controllers (the Java DAOs the website uses).

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

/// Java's `String.hashCode`.
pub fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// The order a Java `HashMap<String, _>` iterates its keys, given insertion order (default
/// capacity 16, load factor 0.75, resized by doubling; ties within a bucket keep insertion order).
pub fn java_hashmap_order<T>(items: Vec<(String, T)>) -> Vec<(String, T)> {
    let mut capacity = 16usize;
    while items.len() as f64 > capacity as f64 * 0.75 {
        capacity *= 2;
    }
    let bucket = |k: &str| {
        let h = java_string_hash(k) as u32;
        ((h ^ (h >> 16)) as usize) & (capacity - 1)
    };
    let mut indexed: Vec<(usize, usize, (String, T))> = items
        .into_iter()
        .enumerate()
        .map(|(i, kv)| (bucket(&kv.0), i, kv))
        .collect();
    indexed.sort_by_key(|(b, i, _)| (*b, *i));
    indexed.into_iter().map(|(_, _, kv)| kv).collect()
}

/// Java: `TagDao.getPopularTags(n)`: the `n` most used tags with their font size (10, 12, 14 or
/// 20 px), shuffled.
pub fn popular_tags(world: &World, n: usize) -> Vec<(String, i64)> {
    use rand::seq::SliceRandom;
    let tags = world.store.records::<Tag>().unwrap_or_default();
    // SELECT tag, COUNT(*) GROUP BY tag ORDER BY quantity DESC LIMIT n (collation: case-insensitive).
    let mut groups: Vec<(String, i64)> = Vec::new();
    for t in tags {
        match groups
            .iter_mut()
            .find(|(k, _)| k.to_lowercase() == t.tag.to_lowercase())
        {
            Some(g) => g.1 += 1,
            None => groups.push((t.tag.clone(), 1)),
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.1));
    groups.truncate(n);
    let total: i64 = groups.iter().map(|g| g.1).sum();
    let mut list = java_hashmap_order(groups);
    list.sort_by_key(|g| g.1);
    let fonts = [10, 12, 14, 20];
    let mut out = Vec::new();
    if !list.is_empty() && total > 0 {
        let mut counter = list.len();
        let bits = list.len() / 4;
        for (tag, _) in list {
            let mut weight = 0;
            if counter == bits {
                weight = 3;
            }
            if counter == bits * 3 {
                weight = 2;
            }
            if counter == bits * 2 {
                weight = 1;
            }
            out.push((tag, fonts[weight]));
            counter -= 1;
        }
    }
    out.shuffle(&mut rand::rng());
    out
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

/// Java: `RoomSelectionHandler.selectRoom`: creates the user's pre-decorated room (`model_s`,
/// its wallpaper and floor, a stool and a window in the room, a table in the hand).
pub fn select_room(world: &World, user_id: u32, room_type: i32) -> bool {
    match try_select_room(world, user_id, room_type) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("web: room selection: {e}");
            false
        }
    }
}

fn try_select_room(world: &World, user_id: u32, room_type: i32) -> anyhow::Result<bool> {
    let Some(user) = world.store.user(user_id)? else {
        return Ok(false);
    };
    let (stool_x, stool_y, stool_rot, floor, wallpaper) = match room_type {
        0 => (1, 6, 2, 601, 1501),
        1 => (3, 6, 4, 0, 607),
        2 => (2, 2, 4, 301, 1901),
        3 => (1, 2, 2, 110, 1801),
        4 => (3, 6, 0, 104, 503),
        5 => (3, 6, 0, 107, 804),
        _ => return Ok(false),
    };
    let now = super::beans::now();
    // NavigatorDao.createRoom(user, "<name>'s Room", "model_s", showName = true, access 0).
    let room = rusbbo_storage::Room {
        owner_id: user_id,
        category: 2,
        name: format!("{}'s Room", user.username),
        model: "model_s".into(),
        ccts: Some(String::new()),
        landscape: "0".into(),
        showname: true,
        accesstype: 0,
        password: Some(String::new()),
        visitors_max: 25,
        icon_data: "0|0|".into(),
        created_at: now,
        updated_at: now,
        ..Default::default()
    };
    let room_id = world.store.create_room(&room)?;
    let description = format!("{} has entered the building", user.username);
    if world
        .store
        .update_room(room_id, |r| {
            r.wallpaper = wallpaper;
            r.floor = floor;
            r.description = description;
        })?
        .is_none()
    {
        return Ok(false);
    }
    let def = |sprite: &str| {
        world
            .items
            .definition_by_sprite(sprite)
            .map(|d| d.record.id)
            .ok_or_else(|| anyhow::anyhow!("no {sprite}"))
    };
    let mut w = world.store.write()?;
    let mut stool = w.insert_item(
        &rusbbo_storage::Item::with_defaults(
            user_id,
            def(&format!("noob_stool*{}", room_type + 1))?,
            String::new(),
            now,
        ),
        now,
    )?;
    stool.x = stool_x;
    stool.y = stool_y;
    stool.rotation = stool_rot;
    stool.room_id = room_id as i32;
    w.put_item(&stool)?;
    w.insert_item(
        &rusbbo_storage::Item::with_defaults(
            user_id,
            def(&format!("noob_table*{}", room_type + 1))?,
            String::new(),
            now,
        ),
        now,
    )?;
    let mut window = w.insert_item(
        &rusbbo_storage::Item::with_defaults(
            user_id,
            def("noob_window_double")?,
            String::new(),
            now,
        ),
        now,
    )?;
    window.wall_position = ":w=3,0 l=13,71 r".into();
    window.room_id = room_id as i32;
    w.put_item(&window)?;
    w.update_user(user_id, |u| u.selected_room_id = room_id as i32)?;
    w.commit()?;
    Ok(true)
}

/// Java: `GroupMemberDao.countMembers`.
pub fn count_members(world: &World, group_id: u32, pending: bool) -> i64 {
    world
        .store
        .records_where::<rusbbo_storage::GroupMember>(|m| {
            m.group_id == group_id && m.is_pending == pending
        })
        .map(|v| v.len() as i64)
        .unwrap_or(0)
}

/// Java: `GroupMemberDao.getMember` (the row, pending or not).
pub fn membership(
    world: &World,
    group_id: u32,
    user_id: i64,
) -> Option<rusbbo_storage::GroupMember> {
    if user_id < 0 {
        return None;
    }
    world
        .store
        .records_where::<rusbbo_storage::GroupMember>(|m| {
            m.group_id == group_id && m.user_id as i64 == user_id
        })
        .ok()?
        .pop()
}

/// Java: `Group.getMember`: the owner (rank 3), or a member who is not pending.
pub fn group_member(
    world: &World,
    group_id: u32,
    owner_id: u32,
    user_id: i64,
) -> Option<rusbbo_storage::GroupMember> {
    if owner_id as i64 == user_id {
        return Some(rusbbo_storage::GroupMember {
            user_id: owner_id,
            group_id,
            member_rank: 3,
            ..Default::default()
        });
    }
    membership(world, group_id, user_id).filter(|m| !m.is_pending)
}

pub fn is_member(world: &World, group_id: u32, owner_id: u32, user_id: i64) -> bool {
    owner_id as i64 == user_id
        || membership(world, group_id, user_id).is_some_and(|m| !m.is_pending)
}

pub fn is_pending_member(world: &World, group_id: u32, owner_id: u32, user_id: i64) -> bool {
    owner_id as i64 != user_id && membership(world, group_id, user_id).is_some_and(|m| m.is_pending)
}

/// Java: `GroupDao.getJoinedGroups`: owned and joined groups, most members first.
pub fn joined_groups(world: &World, user_id: u32) -> Vec<rusbbo_storage::Group> {
    let groups = world
        .store
        .records::<rusbbo_storage::Group>()
        .unwrap_or_default();
    let memberships = world
        .store
        .records_where::<rusbbo_storage::GroupMember>(|m| m.user_id == user_id && !m.is_pending)
        .unwrap_or_default();
    let mut out: Vec<rusbbo_storage::Group> = groups
        .iter()
        .filter(|g| g.owner_id == user_id)
        .cloned()
        .collect();
    for m in memberships {
        if let Some(g) = groups.iter().find(|g| g.id == m.group_id)
            && !out.iter().any(|o| o.id == g.id)
        {
            out.push(g.clone());
        }
    }
    // Stable sort, like the Java stream's sorted().
    out.sort_by_key(|g| std::cmp::Reverse(count_members(world, g.id, false)));
    out
}

/// Java: `RecommendedDao.getRecommendedGroups(staffPick)`.
pub fn recommended_groups(world: &World, staff_pick: bool) -> Vec<rusbbo_storage::Group> {
    let picks = world
        .store
        .records_where::<rusbbo_storage::Recommended>(|r| {
            r.kind == "GROUP" && r.is_staff_pick == staff_pick
        })
        .unwrap_or_default();
    world
        .store
        .records_where::<rusbbo_storage::Group>(|g| picks.iter().any(|p| p.recommended_id == g.id))
        .unwrap_or_default()
}

/// Java: `RecommendedDao.getRecommendedRooms(staffPick)`.
pub fn recommended_rooms(world: &World, staff_pick: bool) -> Vec<rusbbo_storage::Room> {
    let picks = world
        .store
        .records_where::<rusbbo_storage::Recommended>(|r| {
            r.kind == "ROOM" && r.is_staff_pick == staff_pick
        })
        .unwrap_or_default();
    let mut ids: Vec<u32> = picks.iter().map(|p| p.recommended_id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| world.store.room(id).ok().flatten())
        .collect()
}

/// Java: `NewsDao.getTop(ALL, limit, includeUnpublished, [], 0)`: newest first.
pub fn top_news(
    world: &World,
    limit: usize,
    include_unpublished: bool,
) -> Vec<rusbbo_storage::Article> {
    let mut v = world
        .store
        .records_where::<rusbbo_storage::Article>(|a| include_unpublished || a.is_published)
        .unwrap_or_default();
    v.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    v.truncate(limit);
    v
}

/// Java: `AlertsDao.getAlerts`: newest first.
pub fn alerts(world: &World, user_id: u32) -> Vec<rusbbo_storage::Alert> {
    let mut v = world
        .store
        .records_where::<rusbbo_storage::Alert>(|a| a.user_id == user_id)
        .unwrap_or_default();
    v.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    v
}

/// Java: `AlertsDao.createAlert`.
pub fn create_alert(world: &World, user_id: u32, alert_type: &str, message: &str) {
    let mut a = rusbbo_storage::Alert {
        user_id,
        alert_type: alert_type.into(),
        message: message.into(),
        created_at: super::beans::now(),
        ..Default::default()
    };
    if let Err(e) = world.store.insert_record(&mut a) {
        tracing::error!("web: alert: {e}");
    }
}

/// Java: `AlertsDao.deleteAlerts`.
pub fn delete_alerts(world: &World, user_id: u32, alert_type: &str) {
    for a in world
        .store
        .records_where::<rusbbo_storage::Alert>(|a| {
            a.user_id == user_id && a.alert_type == alert_type
        })
        .unwrap_or_default()
    {
        let _ = world.store.delete_record::<rusbbo_storage::Alert>(a.id);
    }
}

/// Java: `AlertsDao.getOnlineFriends`: friends whose `users.is_online` is set.
pub fn online_friends(world: &World, user_id: u32) -> Vec<(rusbbo_storage::User, i32)> {
    world
        .store
        .messenger_friends(user_id)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(id, cat)| world.store.user(id).ok().flatten().map(|u| (u, cat)))
        .filter(|(u, _)| u.is_online)
        .collect()
}

/// Java: `RoomDao.getRecommendedRooms(limit, offset)` (`WebCache.popularRooms`): open user rooms,
/// busiest then best rated.
pub fn popular_rooms(world: &World, limit: usize, offset: usize) -> Vec<rusbbo_storage::Room> {
    world
        .store
        .recommended_rooms(limit, offset)
        .unwrap_or_default()
}

/// Java: `PlayerDao.getRecentlyActiveHabbos(30 days, 500)` shuffled, `count` of them
/// (`WebCache.randomHabbos`).
pub fn random_habbos(world: &World, count: usize) -> Vec<rusbbo_storage::User> {
    use rand::seq::SliceRandom;
    const BLACKLIST: &str = "hd-180-1.hr-100-61.ch-210-66.lg-270-82.sh-290-80";
    let since = super::beans::now() - 30 * 86_400;
    let mut users: Vec<rusbbo_storage::User> = world
        .store
        .users()
        .unwrap_or_default()
        .into_iter()
        .filter(|u| u.figure != BLACKLIST && u.last_online > since)
        .collect();
    users.sort_by_key(|u| std::cmp::Reverse(u.last_online));
    users.truncate(500);
    users.shuffle(&mut rand::rng());
    users.truncate(count);
    users
}
