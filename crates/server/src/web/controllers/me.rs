//! Java: `AccountController.me`, the logged-in front page, and what it needs
//! (`BeginnerGiftManager`, `FriendsFeed`, `ClubSubscription.countMemberDays`).

use std::collections::BTreeMap;

use rusbbo_pebble::{Map, Value};

use super::minimail::{Outcome, append_messages};
use super::{MACHINE_ID_COOKIE, xss_clear};
use crate::web::beans::{has_club, now};
use crate::web::{Conn, WebResult, data, jdate, model};

pub fn me(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("me");
    let world = conn.web.world.clone();
    let Some(user) = world.store.user(conn.user_id())? else {
        {
            let mut s = conn.session();
            s.delete("user.id");
            s.delete("authenticated");
        }
        conn.redirect("/");
        return Ok(());
    };
    if data::active_ban(&world, &user).is_some() {
        conn.redirect("/account/banned");
        return Ok(());
    }
    {
        let mut s = conn.session();
        s.set("page", "me");
        s.delete("captcha.invalid");
    }
    let now = now();
    if has_club(&user) {
        tpl.set("hcDays", (user.club_expiration - now) / 86_400);
    }

    let snapshot = conn.web.watchdog.snapshot();
    let mut articles = if user.rank > 1 {
        snapshot.news_staff.clone()
    } else {
        snapshot.news.clone()
    };
    while articles.len() < 5 {
        articles.push(model::no_news());
    }
    for (i, a) in articles.iter().enumerate() {
        tpl.set(&format!("article{}", i + 1), model::article(&world, a));
    }

    let alerts = data::alerts(&world, user.id);
    if world.store.statistics(user.id)?.is_none() {
        world
            .store
            .put_statistics(user.id, &rusbbo_storage::UserStatistics::new_row())?;
    }
    let stat = |c: &str| data::statistic(&world, user.id, c);
    tpl.set("newbieRoomLayout", stat("newbie_room_layout"));
    tpl.set("newbieNextGift", stat("newbie_gift"));
    if stat("newbie_room_layout") > 0 && stat("newbie_gift") > 0 {
        let mut seconds = stat("newbie_gift_time") - now;
        if beginner_gift_progress(&world, &user) {
            seconds = stat("newbie_gift_time") - now;
        }
        tpl.set("newbieNextGift", stat("newbie_gift"));
        tpl.set("newbieGiftSeconds", seconds.max(0));
    }

    if has_club(&user) {
        if alerts.iter().any(|a| a.alert_type == "HC_EXPIRED") {
            data::delete_alerts(&world, user.id, "HC_EXPIRED");
        }
    } else if !alerts.iter().any(|a| a.alert_type == "HC_EXPIRED") && user.club_subscribed > 0 {
        data::create_alert(&world, user.id, "HC_EXPIRED", "");
    }
    if user.selected_room_id == -1 && stat("newbie_room_layout") != -1 {
        data::set_statistic(&world, user.id, "newbie_room_layout", -1);
    }

    let join_md = jdate::format(user.created_at, "MM/dd");
    let today_md = jdate::format(now, "MM/dd");
    if join_md.eq_ignore_ascii_case(&today_md)
        && !jdate::format(user.created_at, "MM/dd/yyyy")
            .eq_ignore_ascii_case(&jdate::format(now, "MM/dd/yyy"))
    {
        let years = birthday_years(user.created_at, now);
        tpl.set("hasBirthday", true);
        tpl.set("birthdayAge", years);
        let y = years.to_string();
        let prefix = if y.ends_with('1') {
            "st"
        } else if y.ends_with('2') {
            "nd"
        } else if y.ends_with('3') {
            "rd"
        } else {
            "th"
        };
        tpl.set("birthdayPrefix", prefix);
    } else {
        tpl.set("hasBirthday", false);
    }

    tpl.set("tags", Value::list(data::user_tags(&world, user.id)));
    tpl.set("lastOnline", jdate::friendly(user.last_online));
    tpl.set("tagRandomQuestion", random_question());
    let events: Vec<Value> = snapshot
        .events
        .iter()
        .map(|e| model::event(&world, e))
        .collect();
    tpl.set("events", Value::list(events));
    tpl.set(
        "groups",
        Value::list(
            data::joined_groups(&world, user.id)
                .iter()
                .map(|g| model::group(&world, g))
                .collect::<Vec<_>>(),
        ),
    );
    let shown_alerts: Vec<Value> = data::alerts(&world, user.id)
        .iter()
        .filter(|a| !a.is_disabled)
        .map(model::alert)
        .collect();
    tpl.set("alerts", Value::list(shown_alerts));
    let groups_value = |staff: bool| {
        Value::list(
            data::recommended_groups(&world, staff)
                .iter()
                .map(|g| model::group(&world, g))
                .collect::<Vec<_>>(),
        )
    };
    tpl.set("recommendedGroups", groups_value(false));
    tpl.set("staffPickGroups", groups_value(true));
    tpl.set(
        "staffPickRooms",
        Value::list(
            data::recommended_rooms(&world, true)
                .iter()
                .map(|r| model::room(&world, r))
                .collect::<Vec<_>>(),
        ),
    );

    // FriendsFeed.createFriendsOnline
    let friends: Vec<Value> = data::online_friends(&world, user.id)
        .iter()
        .map(|(u, cat)| model::messenger_user(u, *cat))
        .collect();
    tpl.set("feedFriendsOnline", Value::list(friends));
    tpl.set(
        "feedFriendRequests",
        world.store.messenger_requests(user.id)?.len(),
    );
    conn.session().delete("friendsOnlineRequest");

    if !append_messages(conn, &mut tpl, true, Outcome::None) {
        return Ok(());
    }

    let (pending_count, pending_groups) = pending_members(&world, user.id);
    tpl.set("pendingMembers", pending_count);
    tpl.set("pendingGroups", pending_groups);
    let (posts_count, posts_groups) = new_group_posts(&world, user.id, user.last_online);
    tpl.set("newPostsAmount", posts_count);
    tpl.set("newPosts", posts_groups);
    tpl.set("unreadGuestbookMessages", stat("guestbook_unread_messages"));
    conn.render(tpl)?;

    count_member_days(&world, &user);
    let ip = conn.ip.clone();
    if world.store.latest_ip(user.id)?.as_deref() != Some(ip.as_str()) {
        world.store.log_ip(user.id, &ip, now)?;
    }
    let cookie_ok = conn.cookie(MACHINE_ID_COOKIE) == Some(user.machine_id.as_str());
    if !cookie_ok && !user.machine_id.trim().is_empty() {
        conn.set_cookie(
            MACHINE_ID_COOKIE,
            &user.machine_id.replace('#', ""),
            Some(2 * 86_400),
        );
    }
    Ok(())
}

/// `Period.between(joined, today).getYears()`.
fn birthday_years(joined: i64, now: i64) -> i64 {
    use chrono::{Datelike, Local, TimeZone};
    let (Some(a), Some(b)) = (
        Local.timestamp_opt(joined, 0).single(),
        Local.timestamp_opt(now, 0).single(),
    ) else {
        return 0;
    };
    let mut years = (b.year() - a.year()) as i64;
    if (b.month(), b.day()) < (a.month(), a.day()) {
        years -= 1;
    }
    years
}

pub fn random_question() -> String {
    use rand::RngExt;
    let q = crate::web::tag_questions::TAG_QUESTIONS;
    q[rand::rng().random_range(0..q.len())].to_string()
}

/// Java: `BeginnerGiftManager.progress`: the next beginner gift, when it is due.
pub fn beginner_gift_progress(world: &crate::world::World, user: &rusbbo_storage::User) -> bool {
    let stat = |c: &str| data::statistic(world, user.id, c);
    let (layout, gift) = (stat("newbie_room_layout"), stat("newbie_gift"));
    if !(layout > 0 && gift > 0) || gift > 2 || stat("newbie_gift_time") > now() {
        return false;
    }
    let sprite = match gift {
        1 => format!("noob_stool*{layout}"),
        2 => "noob_plant".to_string(),
        _ => return false,
    };
    let Some(definition) = world.items.definition_by_sprite(&sprite) else {
        return false;
    };
    let label = data::cfg_string(world, "alerts.gift.message")
        .replace("%item_name%", &definition.record.name);
    data::create_alert(world, user.id, "PRESENT", &label);
    // Java: ItemManager.createGift(id, name, sprite, label, ""): the sprite is the sale code.
    let created = (|| -> anyhow::Result<()> {
        let mut w = world.store.write()?;
        world.catalogue.create_gift_in(
            &mut w,
            &world.items,
            user.id,
            &user.username,
            &definition.record.sprite,
            &label,
            "",
            now(),
        )?;
        w.commit()?;
        Ok(())
    })();
    if let Err(e) = created {
        tracing::error!("web: beginner gift: {e}");
    }
    if gift + 1 < 3 {
        data::set_statistic(world, user.id, "newbie_gift", gift + 1);
        data::set_statistic(world, user.id, "newbie_gift_time", now() + 86_400);
    } else {
        data::set_statistic(world, user.id, "newbie_gift", 3);
        data::set_statistic(world, user.id, "newbie_gift_time", 0);
    }
    true
}

/// Java: `ClubSubscription.countMemberDays`.
pub fn count_member_days(world: &crate::world::World, user: &rusbbo_storage::User) {
    if !has_club(user) {
        return;
    }
    let updated = data::statistic(world, user.id, "club_member_time_updated");
    if updated > 0 {
        let time = data::statistic(world, user.id, "club_member_time");
        data::set_statistic(world, user.id, "club_member_time", time + now() - updated);
    }
    data::set_statistic(world, user.id, "club_member_time_updated", now());
}

/// The groups a user owns or administers (Java: the first query of `getPendingMembers` and
/// `getNewGroupMessages`), as id -> name.
fn managed_groups(
    world: &crate::world::World,
    user_id: u32,
    ranks: &[i32],
    include_pending: bool,
) -> BTreeMap<u32, String> {
    let groups = world
        .store
        .records::<rusbbo_storage::Group>()
        .unwrap_or_default();
    let mut out = BTreeMap::new();
    for g in groups.iter().filter(|g| g.owner_id == user_id) {
        out.insert(g.id, g.name.clone());
    }
    for m in world
        .store
        .records_where::<rusbbo_storage::GroupMember>(|m| {
            m.user_id == user_id
                && ranks.contains(&m.member_rank)
                && (include_pending || !m.is_pending)
        })
        .unwrap_or_default()
    {
        if let Some(g) = groups.iter().find(|g| g.id == m.group_id) {
            out.insert(g.id, g.name.clone());
        }
    }
    out
}

/// Java: `GroupMemberDao.getPendingMembers`: groups with membership requests the user can answer.
/// The count is the number of such groups (`GROUP BY group_id`).
fn pending_members(world: &crate::world::World, user_id: u32) -> (i64, Value) {
    let groups = managed_groups(world, user_id, &[2, 3], true);
    let mut map = Map::new();
    let mut count = 0;
    for (id, name) in &groups {
        if data::count_members(world, *id, true) > 0 {
            map.insert(id.to_string(), name.as_str());
            count += 1;
        }
    }
    (count, map.into())
}

/// Java: `GroupDiscussionDao.getNewGroupMessages`: groups with replies from the last days the
/// user has not read.
fn new_group_posts(world: &crate::world::World, user_id: u32, _last_online: i64) -> (i64, Value) {
    let groups = managed_groups(world, user_id, &[1, 2, 3], false);
    let mut map = Map::new();
    let mut count = 0;
    if groups.is_empty() {
        return (0, map.into());
    }
    let threads = world
        .store
        .records::<rusbbo_storage::ForumThread>()
        .unwrap_or_default();
    let replies = world
        .store
        .records::<rusbbo_storage::ForumReply>()
        .unwrap_or_default();
    let reads = world
        .store
        .records_where::<rusbbo_storage::ForumRead>(|r| r.user_id == user_id)
        .unwrap_or_default();
    let cutoff = now() - MAX_UNREAD_DAYS * 86_400;
    for (gid, name) in &groups {
        let unread = threads.iter().filter(|t| t.group_id == *gid).any(|t| {
            let thread_replies: Vec<_> = replies.iter().filter(|r| r.thread_id == t.id).collect();
            let last = thread_replies.iter().map(|r| r.id).max();
            let last_read = last.is_some_and(|l| reads.iter().any(|r| r.reply_id == l));
            !last_read && thread_replies.iter().any(|r| r.created_at >= cutoff)
        });
        if unread {
            map.insert(gid.to_string(), name.as_str());
            count += 1;
        }
    }
    (count, map.into())
}

/// Java: `GroupDiscussionDao.MAX_UNREAD_DAYS`.
const MAX_UNREAD_DAYS: i64 = 7;
