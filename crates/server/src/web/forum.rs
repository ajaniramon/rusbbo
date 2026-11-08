//! Group forums: Havana-Web's `GroupDiscussionDao`, `ReplyDao`, `DiscussionTopic` and
//! `DiscussionReply`.

use std::sync::Arc;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{ForumRead, ForumReply, ForumThread, User};

use super::{beans, data, jdate, model, text};
use crate::world::World;

/// Java: `GroupDiscussionDao.MAX_UNREAD_DAYS`.
const MAX_UNREAD_DAYS: i64 = 7;

/// `DATEDIFF(NOW(), created_at) > 7`: calendar days in the server's time zone.
fn older_than_unread_days(created_at: i64) -> bool {
    use chrono::{Local, TimeZone};
    let Some(then) = Local.timestamp_opt(created_at, 0).single() else {
        return false;
    };
    (Local::now().date_naive() - then.date_naive()).num_days() > MAX_UNREAD_DAYS
}

/// Java's `has_read` subquery: `COUNT(*) FROM cms_forums_read_replies WHERE (reply_id = ? AND
/// user_id = ?) OR (DATEDIFF(NOW(), created_at) > 7)`. The second half does not look at the
/// row, so an old reply counts as read only when someone has read anything.
fn has_read(reads: &[ForumRead], reply: &ForumReply, user_id: u32) -> bool {
    let old = older_than_unread_days(reply.created_at);
    reads
        .iter()
        .any(|r| (r.reply_id == reply.id && r.user_id == user_id) || old)
}

fn reads(world: &World) -> Vec<ForumRead> {
    world.store.records::<ForumRead>().unwrap_or_default()
}

fn user(world: &World, id: u32) -> Option<User> {
    world.store.user(id).ok().flatten()
}

/// The replies of a thread whose poster exists (`INNER JOIN users`), oldest first.
fn thread_replies(world: &World, thread_id: u32) -> Vec<ForumReply> {
    let mut v = world
        .store
        .records_where::<ForumReply>(|r| r.thread_id == thread_id)
        .unwrap_or_default();
    v.retain(|r| user(world, r.poster_id).is_some());
    v.sort_by_key(|r| (r.created_at, r.id));
    v
}

/// A topic as `GroupDiscussionDao.fill` reads it, with the reply it was joined with.
fn topic_value(
    world: &Arc<World>,
    t: &ForumThread,
    last: &ForumReply,
    user_id: u32,
    reads: &[ForumRead],
) -> Option<Value> {
    let creator = user(world, t.poster_id)?;
    let replier = user(world, last.poster_id)?;
    let reply_count = world
        .store
        .records_where::<ForumReply>(|r| r.thread_id == t.id)
        .map(|v| v.len() as i64)
        .unwrap_or(0);
    let read = user_id > 0 && has_read(reads, last, user_id);
    Some(model::discussion_topic(
        world,
        t,
        reply_count,
        last.created_at,
        creator.id,
        &creator.username,
        &replier.username,
        read,
    ))
}

/// Java: `GroupDiscussionDao.getDiscussions`: each topic with its newest reply (`MAX(id)`),
/// stickied first, then by that reply's time.
pub fn discussions(
    world: &Arc<World>,
    group_id: u32,
    page: i64,
    per_page: i64,
    user_id: u32,
) -> Vec<Value> {
    let reads = reads(world);
    let mut rows: Vec<(ForumThread, ForumReply)> = world
        .store
        .records_where::<ForumThread>(|t| t.group_id == group_id)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| {
            let last = world
                .store
                .records_where::<ForumReply>(|r| r.thread_id == t.id)
                .ok()?
                .into_iter()
                .max_by_key(|r| r.id)?;
            Some((t, last))
        })
        .filter(|(t, last)| {
            user(world, t.poster_id).is_some() && user(world, last.poster_id).is_some()
        })
        .collect();
    rows.sort_by(|(a, ra), (b, rb)| {
        b.is_stickied
            .cmp(&a.is_stickied)
            .then(rb.created_at.cmp(&ra.created_at))
    });
    let start = ((page - 1) * per_page).max(0) as usize;
    rows.iter()
        .skip(start)
        .take(per_page.max(0) as usize)
        .filter_map(|(t, last)| topic_value(world, t, last, user_id, &reads))
        .collect()
}

/// Java: `GroupDiscussionDao.countDiscussions`: always 0 (see [`first_reply`]).
pub fn count_discussions(_world: &World, _group_id: u32) -> i64 {
    0
}

/// Java: `GroupDiscussionDao.getDiscussion`: the topic joined with its newest reply by time.
pub fn discussion(
    world: &Arc<World>,
    group_id: i64,
    id: i64,
    user_id: u32,
) -> Option<(ForumThread, Value)> {
    let t = u32::try_from(id)
        .ok()
        .and_then(|id| world.store.record::<ForumThread>(id).ok().flatten())
        .filter(|t| t.group_id as i64 == group_id)?;
    let v = discussion_value(world, &t, user_id)?;
    Some((t, v))
}

/// The `DiscussionTopic` of `t` as it is in memory (Java changes the object and shows it).
pub fn discussion_value(world: &Arc<World>, t: &ForumThread, user_id: u32) -> Option<Value> {
    user(world, t.poster_id)?;
    let last = world
        .store
        .records_where::<ForumReply>(|r| r.thread_id == t.id)
        .ok()?
        .into_iter()
        .filter(|r| user(world, r.poster_id).is_some())
        .max_by_key(|r| (r.created_at, std::cmp::Reverse(r.id)))?;
    topic_value(world, t, &last, user_id, &reads(world))
}

/// The forum badges of a user: the first worn badge by slot and their favourite group's badge.
fn display_badges(world: &World, u: &User) -> (Option<String>, Option<String>) {
    let mut worn: Vec<_> = world
        .store
        .badges(u.id)
        .unwrap_or_default()
        .into_iter()
        .filter(|b| b.equipped)
        .collect();
    worn.sort_by_key(|b| b.slot_id);
    let group = u32::try_from(u.favourite_group)
        .ok()
        .and_then(|id| {
            world
                .store
                .record::<rusbbo_storage::Group>(id)
                .ok()
                .flatten()
        })
        .map(|g| g.badge);
    (worn.into_iter().next().map(|b| b.code), group)
}

/// Java: `GroupDiscussionDao.getDisplayBadges`.
pub fn user_display_badges(world: &World, user_id: u32) -> (Option<String>, Option<String>) {
    user(world, user_id)
        .map(|u| display_badges(world, &u))
        .unwrap_or((None, None))
}

/// Java: `GroupDiscussionDao.countUserReplies`.
pub fn count_user_replies(world: &World, user_id: u32) -> i64 {
    world
        .store
        .records_where::<ForumReply>(|r| r.poster_id == user_id)
        .map(|v| v.len() as i64)
        .unwrap_or(0)
}

fn date(ts: i64, f: &str) -> String {
    jdate::format(ts, f)
        .replace("am", "AM")
        .replace("pm", "PM")
        .replace('.', "")
}

/// Java: `DiscussionReply`.
pub fn reply_value(world: &Arc<World>, r: &ForumReply, user_id: u32, reads: &[ForumRead]) -> Value {
    let poster = user(world, r.poster_id);
    let (badge, group_badge) = poster
        .as_ref()
        .map(|u| display_badges(world, u))
        .unwrap_or((None, None));
    let (created, modified) = (r.created_at, r.modified_at);
    let read = user_id > 0 && has_read(reads, r, user_id);
    Map::new()
        .with("id", r.id)
        .with("userId", r.poster_id)
        .with("message", text::filter_sentence(world, &r.message))
        .with(
            "formattedMessage",
            text::bbcode(
                world,
                &text::html_escape(&text::bbcode_normalise(&text::filter_sentence(
                    world, &r.message,
                ))),
                false,
            ),
        )
        .with("figure", poster.as_ref().map(|u| u.figure.clone()))
        .with("username", poster.as_ref().map(|u| u.username.clone()))
        .with("online", poster.as_ref().is_some_and(|u| u.is_online))
        .with("equippedBadge", badge.clone())
        .with("hasBadge", badge.is_some())
        .with("groupId", poster.as_ref().map_or(0, |u| u.favourite_group))
        .with("hasGroupBadge", group_badge.is_some())
        .with("groupBadge", group_badge)
        .with("forumMessages", count_user_replies(world, r.poster_id))
        .with("edited", r.is_edited)
        .with("deleted", r.is_deleted)
        .with("createdAt", created)
        .with("modifiedAt", modified)
        .with("new", !read)
        .with_fn("getCreatedDate", move |a| {
            Ok(Value::from(date(
                created,
                &a.first().map(Value::render).unwrap_or_default(),
            )))
        })
        .with_fn("getEditedDate", move |a| {
            Ok(Value::from(date(
                modified,
                &a.first().map(Value::render).unwrap_or_default(),
            )))
        })
        .into()
}

/// Java: `GroupDiscussionDao.getReplies`: (the rows, their values), oldest first.
pub fn replies(
    world: &Arc<World>,
    thread_id: u32,
    page: i64,
    per_page: i64,
    user_id: u32,
) -> Vec<(ForumReply, Value)> {
    let reads = reads(world);
    let start = ((page - 1) * per_page).max(0) as usize;
    thread_replies(world, thread_id)
        .into_iter()
        .skip(start)
        .take(per_page.max(0) as usize)
        .map(|r| {
            let v = reply_value(world, &r, user_id, &reads);
            (r, v)
        })
        .collect()
}

/// Java: `GroupDiscussionDao.getReply`.
pub fn reply(world: &World, thread_id: u32, id: i64) -> Option<ForumReply> {
    let r = u32::try_from(id)
        .ok()
        .and_then(|id| world.store.record::<ForumReply>(id).ok().flatten())
        .filter(|r| r.thread_id == thread_id)?;
    user(world, r.poster_id)?;
    Some(r)
}

/// Java: `GroupDiscussionDao.getLatestReply`.
pub fn latest_reply(world: &World, user_id: u32) -> Option<ForumReply> {
    user(world, user_id)?;
    world
        .store
        .records_where::<ForumReply>(|r| r.poster_id == user_id)
        .ok()?
        .into_iter()
        .max_by_key(|r| (r.created_at, std::cmp::Reverse(r.id)))
}

/// Java: `GroupDiscussionDao.getFirstReply`, `countReplies` and `countDiscussions` call
/// `executeUpdate()` on their `SELECT` before running it; the driver refuses, the DAO swallows
/// the exception and returns 0. So the first post is a reply like the others ("RE: ...") and
/// every topic and topic list has one page.
pub fn first_reply(_world: &World, _thread_id: u32) -> u32 {
    0
}

pub fn count_replies(_world: &World, _thread_id: u32) -> i64 {
    0
}

/// Java: `ReplyDao.read` (`INSERT IGNORE` into a table without a key: a second read adds a
/// second row).
pub fn mark_read(world: &World, user_id: u32, reply_ids: &[u32]) -> anyhow::Result<()> {
    let mut w = world.store.write()?;
    for id in reply_ids {
        let mut row = ForumRead {
            user_id,
            reply_id: *id,
            ..Default::default()
        };
        w.insert_record(&mut row)?;
    }
    w.commit()?;
    Ok(())
}

/// Java: `GroupDiscussionDao.createDiscussion`.
pub fn create_discussion(
    world: &World,
    group_id: u32,
    poster_id: u32,
    title: &str,
) -> anyhow::Result<u32> {
    let now = beans::now();
    let mut t = ForumThread {
        topic_title: title.into(),
        poster_id,
        is_open: true,
        group_id,
        created_at: now,
        modified_at: now,
        ..Default::default()
    };
    Ok(world.store.insert_record(&mut t)?)
}

/// Java: `GroupDiscussionDao.createReplies`.
pub fn create_reply(
    world: &World,
    thread_id: u32,
    poster_id: u32,
    message: &str,
) -> anyhow::Result<()> {
    let now = beans::now();
    let mut r = ForumReply {
        thread_id,
        message: message.into(),
        poster_id,
        created_at: now,
        modified_at: now,
        ..Default::default()
    };
    world.store.insert_record(&mut r)?;
    Ok(())
}

/// `ON UPDATE current_timestamp()`: `modified_at` moves only when a value changes.
pub fn save_reply(world: &World, r: &ForumReply) -> anyhow::Result<()> {
    if let Some(old) = world.store.record::<ForumReply>(r.id)? {
        let mut r = r.clone();
        if old.message != r.message
            || old.is_edited != r.is_edited
            || old.is_deleted != r.is_deleted
        {
            r.modified_at = beans::now();
        }
        world.store.put_record(&r)?;
    }
    Ok(())
}

pub fn save_thread(world: &World, t: &ForumThread) -> anyhow::Result<()> {
    if let Some(old) = world.store.record::<ForumThread>(t.id)? {
        let mut t = t.clone();
        if old.topic_title != t.topic_title
            || old.is_open != t.is_open
            || old.is_stickied != t.is_stickied
            || old.views != t.views
        {
            t.modified_at = beans::now();
        }
        world.store.put_record(&t)?;
    }
    Ok(())
}

/// Java: `GroupDiscussionDao.deleteDiscussion`, whose first statement has its parameters
/// swapped: the topic row goes only when its id is the group's and its group the topic's; the
/// replies of `topic_id` always go.
pub fn delete_discussion(world: &World, group_id: i64, topic_id: i64) -> anyhow::Result<()> {
    if let Ok(id) = u32::try_from(group_id)
        && let Some(t) = world.store.record::<ForumThread>(id)?
        && t.group_id as i64 == topic_id
    {
        world.store.delete_record::<ForumThread>(t.id)?;
    }
    for r in world
        .store
        .records_where::<ForumReply>(|r| r.thread_id as i64 == topic_id)?
    {
        world.store.delete_record::<ForumReply>(r.id)?;
    }
    Ok(())
}

/// Java: `GroupDiscussionDao.incrementViews`.
pub fn increment_views(world: &World, id: u32) -> anyhow::Result<()> {
    if let Some(mut t) = world.store.record::<ForumThread>(id)? {
        t.views += 1;
        save_thread(world, &t)?;
    }
    Ok(())
}

/// The forum settings: (`discussions.per.page`, `discussions.replies.per.page`).
pub fn per_page(world: &World) -> (i64, i64) {
    (
        data::cfg_int(world, "discussions.per.page"),
        data::cfg_int(world, "discussions.replies.per.page"),
    )
}
