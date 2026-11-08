//! What Havana-Web's content objects (news, alerts, friends, groups, minimail) look like to the
//! templates.

use std::sync::Arc;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::User;

use super::beans::now;
use super::{data, jdate, text};
use crate::world::World;

/// Java: `NewsArticle` (`id` 0 is the "No news" placeholder).
pub fn article(world: &Arc<World>, a: &rusbbo_storage::Article) -> Value {
    let static_path = data::cfg_string(world, "static.content.path");
    let live_top_story = if !a.topstory_override.is_empty() {
        a.topstory_override.clone()
    } else {
        format!("{static_path}/c_images/Top_Story_Images/{}", a.topstory)
    };
    let all = world
        .store
        .records::<rusbbo_storage::ArticleCategory>()
        .unwrap_or_default();
    let categories: Vec<Value> = a
        .categories
        .iter()
        .filter_map(|id| all.iter().find(|c| c.id == *id))
        .map(news_category)
        .collect();
    let author = if !a.author_override.is_empty() {
        a.author_override.clone()
    } else {
        world
            .store
            .user(a.author_id)
            .ok()
            .flatten()
            .map(|u| u.username)
            .unwrap_or_default()
    };
    let ids = a.categories.clone();
    let w = world.clone();
    let story = a.full_story.clone();
    Map::new()
        .with("id", a.id)
        .with("title", a.title.as_str())
        .with("authorId", a.author_id)
        .with("author", author)
        .with("authorOverride", a.author_override.as_str())
        .with("shortStory", a.short_story.as_str())
        .with("fullStory", a.full_story.as_str())
        .with("timestamp", a.created_at)
        .with(
            "date",
            jdate::format(a.created_at, "EEE dd MMM, yyyy").replace('.', ""),
        )
        .with("topStory", a.topstory.as_str())
        .with("topstoryOverride", a.topstory_override.as_str())
        .with("liveTopStory", live_top_story)
        .with("articleImage", a.article_image.as_str())
        .with("published", a.is_published)
        .with("futurePublished", a.is_future_published)
        .with("views", a.views)
        .with("url", article_url(a.id, &a.title))
        .with("categories", Value::list(categories))
        .with_fn("hasCategory", move |args| {
            let id = args.first().and_then(Value::as_i64).unwrap_or(-1);
            Ok(Value::Bool(ids.iter().any(|c| *c as i64 == id)))
        })
        .with_fn("escapedStory", move |_| {
            Ok(Value::from(text::format_news_story(&w, &story)))
        })
        .into()
}

/// Java: `NewsCategory`.
pub fn news_category(c: &rusbbo_storage::ArticleCategory) -> Value {
    Map::new()
        .with("id", c.id)
        .with("label", c.label.as_str())
        .with("index", c.category_index.as_str())
        .into()
}

/// Java: `NewsArticle.getUrl`.
pub fn article_url(id: u32, title: &str) -> String {
    if id == 0 {
        return "0-no-news".into();
    }
    let mut t = title.to_string();
    for c in [
        "!", "'", "@", "&", "*", "%", "[", "]", "#", "=", "\"", ":", ">", "<", ",", ".", "+", "-",
        "_", "/", "?", "\\",
    ] {
        t = t.replace(c, "");
    }
    format!("{id}-{}", t.replace(' ', "-").to_lowercase())
}

/// The placeholder `/me` shows when there are fewer than five articles.
pub fn no_news() -> rusbbo_storage::Article {
    rusbbo_storage::Article {
        id: 0,
        title: "No news".into(),
        created_at: now(),
        topstory: "attention_topstory.png".into(),
        is_published: true,
        ..Default::default()
    }
}

/// Java: `AccountAlert`.
pub fn alert(a: &rusbbo_storage::Alert) -> Value {
    Map::new()
        .with("id", a.id)
        .with("userId", a.user_id)
        .with("alertType", a.alert_type.as_str())
        .with("message", a.message.as_str())
        .with("disabled", a.is_disabled)
        .with("createdAt", a.created_at)
        .into()
}

/// Java: `MessengerUser`.
pub fn messenger_user(u: &User, category_id: i32) -> Value {
    let last_online = u.last_online;
    Map::new()
        .with("userId", u.id)
        .with("username", u.username.as_str())
        .with("figure", u.figure.as_str())
        .with(
            "sex",
            if u.sex.eq_ignore_ascii_case("f") {
                "F"
            } else {
                "M"
            },
        )
        .with("motto", u.motto.as_str())
        .with("lastOnline", last_online)
        .with("categoryId", category_id)
        .with("online", u.is_online)
        .with(
            "formattedLastOnline",
            jdate::format(last_online, "dd/MM/yyyy hh:mm a")
                .replace("am", "AM")
                .replace("pm", "PM")
                .replace('.', ""),
        )
        .with_fn("getFormatLastOnline", move |args| {
            let f = args.first().map(Value::render).unwrap_or_default();
            Ok(Value::from(jdate::format(last_online, &f)))
        })
        .into()
}

/// Java: `Group`.
pub fn group(world: &Arc<World>, g: &rusbbo_storage::Group) -> Value {
    let id = g.id;
    let owner = g.owner_id;
    let site_path = data::cfg_string(world, "site.path");
    let alias = g.alias.clone().unwrap_or_default();
    let click = if !alias.trim().is_empty() {
        format!("{site_path}/groups/{alias}")
    } else {
        format!("{site_path}/groups/{id}/id")
    };
    let (w1, w2, w3, w4, w5, w6) = (
        world.clone(),
        world.clone(),
        world.clone(),
        world.clone(),
        world.clone(),
        world.clone(),
    );
    fn arg_id(args: &[Value]) -> i64 {
        args.first().and_then(Value::as_i64).unwrap_or(-1)
    }
    Map::new()
        .with("id", id)
        .with("name", text::filter_sentence(world, &g.name))
        .with("description", text::filter_sentence(world, &g.description))
        .with("ownerId", owner)
        .with("roomId", g.room_id)
        .with(
            "badge",
            g.badge
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>(),
        )
        .with("recommended", g.recommended != 0)
        .with("background", g.background.as_str())
        .with("views", g.views)
        .with("topics", g.topics)
        .with("groupType", g.group_type)
        .with("forumType", forum_type(g.forum_type))
        .with("forumPermission", forum_permission(g.forum_permission))
        .with("alias", alias)
        .with("createdDate", jdate::format(g.created_at, "MMM dd, YYYY"))
        .with("generateClickLink", click)
        .with_fn("getMemberCount", move |args| {
            let pending = args.first().is_some_and(|v| matches!(v, Value::Bool(true)));
            Ok(Value::from(data::count_members(&w1, id, pending)))
        })
        .with_fn("getMember", move |args| {
            Ok(data::group_member(&w2, id, owner, arg_id(args))
                .map(group_member)
                .into())
        })
        .with_fn("isMember", move |args| {
            Ok(Value::Bool(data::is_member(&w3, id, owner, arg_id(args))))
        })
        .with_fn("isPendingMember", move |args| {
            Ok(Value::Bool(data::is_pending_member(
                &w4,
                id,
                owner,
                arg_id(args),
            )))
        })
        .with_fn("hasAdministrator", move |args| {
            Ok(Value::Bool(
                data::group_member(&w5, id, owner, arg_id(args))
                    .is_some_and(|m| m.member_rank >= 2),
            ))
        })
        .with_fn("isFavourite", move |args| {
            let user = arg_id(args).max(0) as u32;
            Ok(Value::Bool(
                w6.store
                    .user(user)
                    .ok()
                    .flatten()
                    .is_some_and(|u| u.favourite_group == id as i32),
            ))
        })
        .into()
}

fn enum_value(id: i32, name: &'static str) -> Value {
    Map::new()
        .with("id", id)
        .with("name", name)
        .with_fn("toString", move |_| Ok(Value::str(name)))
        .into()
}

/// Java: `GroupForumType`.
pub fn forum_type(id: i32) -> Value {
    enum_value(id, if id == 1 { "PRIVATE" } else { "PUBLIC" })
}

/// Java: `GroupPermissionType`.
pub fn forum_permission(id: i32) -> Value {
    enum_value(
        id,
        match id {
            1 => "MEMBER_ONLY",
            2 => "ADMIN_ONLY",
            _ => "EVERYONE",
        },
    )
}

/// Java: `GroupMember`.
pub fn group_member(m: rusbbo_storage::GroupMember) -> Value {
    let rank = match m.member_rank {
        3 => "OWNER",
        2 => "ADMINISTRATOR",
        _ => "MEMBER",
    };
    Map::new()
        .with("userId", m.user_id)
        .with("groupId", m.group_id)
        .with("pending", m.is_pending)
        .with(
            "memberRank",
            Map::new()
                .with("rankId", m.member_rank)
                .with("name", rank)
                .with_fn("toString", move |_| Ok(Value::str(rank))),
        )
        .into()
}

/// Java: `Event` (with the room it is held in).
pub fn event(world: &Arc<World>, e: &rusbbo_storage::RoomEvent) -> Value {
    let lifetime = crate::game::events::event_lifetime(world);
    let room = world.store.room(e.room_id).ok().flatten();
    let room_data = Map::new()
        .with("id", e.room_id)
        .with(
            "visitorsNow",
            room.as_ref().map(|r| r.visitors_now as i64).unwrap_or(0),
        )
        .with(
            "visitorsMax",
            room.as_ref().map(|r| r.visitors_max as i64).unwrap_or(0),
        )
        .with(
            "name",
            room.as_ref().map(|r| r.name.clone()).unwrap_or_default(),
        );
    let host = world.store.user(e.user_id).ok().flatten();
    Map::new()
        .with("roomId", e.room_id)
        .with("userId", e.user_id)
        .with("categoryId", e.category_id)
        .with("name", e.name.as_str())
        .with("description", e.description.as_str())
        .with("expireTime", e.expire_time)
        .with("friendlyDate", jdate::friendly(e.expire_time - lifetime))
        .with("roomData", room_data)
        .with(
            "userInfo",
            host.map(|u| super::beans::player_details(world, &u)),
        )
        .into()
}

/// Java: `MinimailMessage` with its author and target.
pub fn minimail(world: &Arc<World>, m: &rusbbo_storage::Minimail) -> Value {
    let author = world
        .store
        .user(m.sender_id)
        .ok()
        .flatten()
        .map(|u| super::beans::player_details(world, &u));
    let target = world
        .store
        .user(m.to_id)
        .ok()
        .flatten()
        .map(|u| super::beans::player_details(world, &u));
    let filtered = text::filter_sentence(world, &m.message);
    Map::new()
        .with("id", m.id)
        .with("targetId", m.target_id)
        .with("toId", m.to_id)
        .with("senderId", m.sender_id)
        .with("read", m.is_read)
        .with("subject", m.subject.as_str())
        .with("message", filtered.as_str())
        .with(
            "formattedSubject",
            text::bbcode(world, &text::html_escape(&m.subject), false),
        )
        .with(
            "formattedMessage",
            text::bbcode(world, &text::html_escape(&filtered), false),
        )
        .with("dateSent", m.date_sent)
        .with("date", jdate::friendly(m.date_sent))
        .with(
            "isoDate",
            jdate::format(m.date_sent, "yyyy-MM-dd'T'HH:mm:ssZ"),
        )
        .with("conversationId", m.conversation_id)
        .with("trash", m.is_trash)
        .with("author", author)
        .with("target", target)
        .into()
}

/// Java: `Room` / `RoomData` (the room's getters; `getData()` is the same object).
pub fn room(world: &Arc<World>, r: &rusbbo_storage::Room) -> Value {
    let owner_name = if r.owner_id > 0 {
        world
            .store
            .user(r.owner_id)
            .ok()
            .flatten()
            .map(|u| u.username)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let access = match r.accesstype {
        1 => "CLOSED",
        2 => "PASSWORD",
        _ => "OPEN",
    };
    let m = Map::new()
        .with("id", r.id)
        .with("ownerId", r.owner_id)
        .with("ownerName", owner_name)
        .with("categoryId", r.category)
        .with("category", r.category)
        .with("name", r.name.as_str())
        .with("description", r.description.as_str())
        .with("model", r.model.as_str())
        .with("ccts", r.ccts.clone().unwrap_or_default())
        .with("wallpaper", r.wallpaper)
        .with("floor", r.floor)
        .with("landscape", r.landscape.as_str())
        .with("showOwnerName", r.showname)
        .with("superUsers", r.superusers)
        .with("accessType", r.accesstype)
        .with("accesstype", r.accesstype)
        .with("accessTypeName", access)
        .with("password", r.password.clone().unwrap_or_default())
        .with("visitorsNow", r.visitors_now)
        .with("visitorsMax", r.visitors_max)
        .with("rating", r.rating)
        .with("iconData", r.icon_data.as_str())
        .with("groupId", r.group_id)
        .with("hidden", r.is_hidden)
        .with("publicRoom", r.owner_id == 0);
    let data = m.clone();
    m.with("data", data).into()
}

/// Java: `DiscussionTopic`.
#[allow(clippy::too_many_arguments)]
pub fn discussion_topic(
    world: &Arc<World>,
    t: &rusbbo_storage::ForumThread,
    reply_count: i64,
    last_message_at: i64,
    creator_id: u32,
    creator_name: &str,
    last_reply_name: &str,
    has_read: bool,
) -> Value {
    let per_page = data::cfg_int(world, "discussions.replies.per.page").max(1);
    let pages = if reply_count > 0 {
        (reply_count + per_page - 1) / per_page
    } else {
        1
    };
    let mut recent = Vec::new();
    if reply_count > per_page {
        for i in 0..3 {
            let n = pages - i;
            if n > 1 {
                recent.push(n);
            }
        }
    }
    recent.sort_unstable();
    let (created, last) = (t.created_at, last_message_at);
    let date = |ts: i64, f: &str| {
        jdate::format(ts, f)
            .replace("am", "AM")
            .replace("pm", "PM")
            .replace('.', "")
    };
    Map::new()
        .with("id", t.id)
        .with("groupId", t.group_id)
        .with("creatorId", creator_id)
        .with("creatorName", creator_name)
        .with("lastReplyName", last_reply_name)
        .with("topicTitle", text::filter_sentence(world, &t.topic_title))
        .with("replyCount", reply_count)
        .with("replyPages", pages)
        .with("recentPages", Value::list(recent))
        .with("open", t.is_open)
        .with("stickied", t.is_stickied)
        .with("views", t.views)
        .with("createdAt", created)
        .with("lastMessageAt", last)
        .with("new", !has_read)
        .with_fn("getCreatedDate", move |args| {
            Ok(Value::from(date(
                created,
                &args.first().map(Value::render).unwrap_or_default(),
            )))
        })
        .with_fn("getLastMessage", move |args| {
            Ok(Value::from(date(
                last,
                &args.first().map(Value::render).unwrap_or_default(),
            )))
        })
        .into()
}
