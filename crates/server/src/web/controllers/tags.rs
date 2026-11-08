//! Java: `controllers/site/TagController` and the tag helpers of `StringUtil` and `TagDao`.

use std::sync::Arc;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::Tag;

use super::{remove_html_tags, xss_clear};
use crate::web::{Conn, WebResult, beans, data, model, text};
use crate::world::World;

/// MariaDB's accent- and case-insensitive collation, roughly: lowercase comparison.
fn same_tag(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Java: `TagDao.hasTag`.
pub fn has_tag(world: &World, user_id: u32, room_id: u32, group_id: u32, tag: &str) -> bool {
    world
        .store
        .records_where::<Tag>(|t| {
            t.user_id == user_id
                && t.room_id == room_id
                && t.group_id == group_id
                && same_tag(&t.tag, tag)
        })
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Java: `StringUtil.normalizeSpace`.
pub fn normalize_space(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Java: `StringUtil.isValidTag`: the tag as stored, or `None`.
pub fn valid_tag(
    world: &World,
    tag: &str,
    user_id: u32,
    room_id: u32,
    group_id: u32,
) -> Option<String> {
    let format = remove_html_tags(&normalize_space(&text::filter_input(tag, false)))
        .replace(',', "")
        .to_lowercase();
    let len = tag.encode_utf16().count();
    if len <= 1
        || tag.trim().is_empty()
        || len > 20
        || has_tag(world, user_id, room_id, group_id, tag)
    {
        return None;
    }
    Some(format)
}

/// Java: `StringUtil.addTag` (with its two spelling fixes).
pub fn add_tag(
    world: &World,
    tag: &str,
    user_id: u32,
    room_id: u32,
    group_id: u32,
) -> rusbbo_storage::Result<()> {
    let mut tag = tag.to_string();
    let mut check = false;
    if tag.eq_ignore_ascii_case("br") || tag.eq_ignore_ascii_case("brasil") {
        tag = "brazil".into();
        check = true;
    }
    if tag.eq_ignore_ascii_case("spanish") || tag.eq_ignore_ascii_case("es") {
        tag = "espa\u{f1}ol".into();
        check = true;
    }
    if check && has_tag(world, user_id, room_id, group_id, &tag) {
        return Ok(());
    }
    let mut t = Tag {
        user_id,
        room_id,
        group_id,
        tag,
        created_at: beans::now(),
        ..Default::default()
    };
    world.store.insert_record(&mut t)?;
    Ok(())
}

/// Java: `TagDao.removeTag` / `removeTags` (`tag` `None`: all of them).
pub fn remove_tags(
    world: &World,
    user_id: u32,
    room_id: u32,
    group_id: u32,
    tag: Option<&str>,
) -> rusbbo_storage::Result<()> {
    let gone = world.store.records_where::<Tag>(|t| {
        t.user_id == user_id
            && t.room_id == room_id
            && t.group_id == group_id
            && tag.is_none_or(|x| same_tag(&t.tag, x))
    })?;
    for t in gone {
        world.store.delete_record::<Tag>(t.id)?;
    }
    Ok(())
}

/// Java: `TagDao.getGroupTags`.
pub fn group_tags(world: &World, group_id: u32) -> Vec<String> {
    world
        .store
        .records_where::<Tag>(|t| t.group_id == group_id)
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.tag)
        .collect()
}

/// Java: `HabboTag`.
fn habbo_tag(world: &Arc<World>, t: &Tag) -> Value {
    let (w1, w2) = (world.clone(), world.clone());
    let (user, group) = (t.user_id, t.group_id);
    Map::new()
        .with("tag", t.tag.as_str())
        .with("roomId", t.room_id)
        .with("userId", user)
        .with("groupId", group)
        .with_fn("getUserData", move |_| {
            Ok(if user > 0 {
                w1.store
                    .user(user)
                    .ok()
                    .flatten()
                    .map(|u| beans::player_details(&w1, &u))
                    .into()
            } else {
                Value::Null
            })
        })
        .with_fn("getGroupData", move |_| {
            Ok(if group > 0 {
                w2.store
                    .record::<rusbbo_storage::Group>(group)
                    .ok()
                    .flatten()
                    .map(|g| model::group(&w2, &g))
                    .into()
            } else {
                Value::Null
            })
        })
        .with(
            "tagList",
            if group > 0 {
                Value::list(group_tags(world, group))
            } else if user > 0 {
                Value::list(data::user_tags(world, user))
            } else {
                Value::list(Vec::<String>::new())
            },
        )
        .into()
}

fn pagination_flags(tpl: &mut crate::web::tpl::Tpl) {
    for k in [
        "showOlder",
        "showOldest",
        "showNewer",
        "showNewest",
        "showFirst",
        "showLast",
    ] {
        tpl.set(k, false);
    }
    tpl.set("showFirstPage", 1);
}

/// `/tag`.
pub fn tag(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    // Java: getInt throws on a non-number (the error page); missing is 0, then page 1.
    let _page = conn
        .get
        .int("pageNumber")
        .ok_or_else(|| crate::web::WebError("pageNumber".into()))?;
    let mut tpl = conn.template("tag");
    tpl.set("tagList", Value::list(Vec::<Value>::new()));
    tpl.set("pageId", 1);
    tpl.set("totalCount", 0);
    tpl.set("tagCloud", super::site::tag_cloud(conn, 10));
    tpl.set("tagSearchAdd", "");
    pagination_flags(&mut tpl);
    conn.session().set("page", "community");
    conn.render(tpl)
}

/// `/tag/*`.
pub fn search(conn: &mut Conn) -> WebResult {
    let page = conn.get.int("pageNumber").unwrap_or(1) as i64;
    let tag = if conn.get.contains("tag") {
        conn.get.string("tag")
    } else {
        conn.wildcard(0).to_string()
    };
    respond(conn, tag, page, "tag")
}

/// `/habblet/ajax/tagsearch`.
pub fn tag_search(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    let tag = conn.post.string("tag");
    respond(conn, tag, 0, "base/tag_search")
}

fn respond(conn: &mut Conn, tag: String, mut page: i64, view: &str) -> WebResult {
    xss_clear(conn);
    let tag = crate::web::http::decode_component(&tag);
    let world = conn.web.world.clone();
    let found: Vec<Tag> = if tag.trim().is_empty() {
        Vec::new()
    } else {
        world.store.records_where::<Tag>(|t| {
            same_tag(&t.tag, &tag) && (t.user_id > 0 || t.group_id > 0) && t.room_id == 0
        })?
    };
    let pages: Vec<&[Tag]> = found.chunks(5).collect();
    let has = |p: i64| p >= 0 && (p as usize) < pages.len();
    if !has(page - 1) {
        page = 1;
    }
    let tag = normalize_space(&remove_html_tags(&tag));
    let mut tpl = conn.template(view);
    tpl.set("tagSearchAdd", "");
    if conn.is_authenticated() && valid_tag(&world, &tag, conn.user_id(), 0, 0).is_some() {
        tpl.set("tagSearchAdd", tag.as_str());
    }
    pagination_flags(&mut tpl);
    let code = page - 1;
    if has(code - 1) {
        tpl.set("showOlder", true);
    }
    if has(code - 2) {
        tpl.set("showOldest", true);
    }
    if has(code + 1) {
        tpl.set("showNewer", true);
    }
    if has(code + 2) {
        tpl.set("showNewest", true);
    }
    if has(code + 3) {
        tpl.set("showLast", true);
        tpl.set("showLastPage", pages.len());
    }
    if has(code - 3) {
        tpl.set("showFirst", true);
        tpl.set("showFirstPage", 1);
    }
    let list = if has(page - 1) {
        Value::list(
            pages[(page - 1) as usize]
                .iter()
                .map(|t| habbo_tag(&world, t))
                .collect::<Vec<_>>(),
        )
    } else {
        Value::Null
    };
    tpl.set("tagList", list);
    let mut all = Map::new();
    for (i, p) in pages.iter().enumerate() {
        all.insert(
            i.to_string(),
            Value::list(p.iter().map(|t| habbo_tag(&world, t)).collect::<Vec<_>>()),
        );
    }
    tpl.set("totalTagUsers", all);
    tpl.set("tag", tag.as_str());
    tpl.set("pageId", page);
    tpl.set("totalCount", found.len());
    tpl.set("tagCloud", super::site::tag_cloud(conn, 10));
    tpl.set("lastPage", found.len());
    conn.session().set("page", "community");
    conn.render(tpl)
}

/// `/myhabbo/tag/add`.
pub fn add(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    let user_id = conn.session().int("user.id");
    if user_id < 1 {
        return Ok(());
    }
    let user_id = user_id as u32;
    let world = conn.web.world.clone();
    if data::user_tags(&world, user_id).len() as i64 >= data::cfg_int(&world, "max.tags.users") {
        conn.send_html("taglimit");
        return Ok(());
    }
    let Some(tag) = valid_tag(&world, &conn.post.string("tagName"), user_id, 0, 0) else {
        conn.send_html("invalidtag");
        return Ok(());
    };
    if text::filter_sentence(&world, &tag) == tag {
        add_tag(&world, &tag, user_id, 0, 0)?;
    }
    conn.send_html("valid");
    crate::rcon::send(&world, "refresh_tags", &[("userId", user_id.to_string())]);
    Ok(())
}

/// `/myhabbo/tag/remove`.
pub fn remove(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    let user_id = conn.session().int("user.id");
    if user_id < 1 {
        conn.send_html("");
        return Ok(());
    }
    let mut tpl = conn.template("homes/widget/habblet/taglist");
    let world = conn.web.world.clone();
    let Some(user) = world.store.user(user_id as u32)? else {
        conn.send_html("");
        return Ok(());
    };
    let tag = conn.post.string("tagName");
    remove_tags(&world, user.id, 0, 0, Some(&tag))?;
    tpl.set("tags", Value::list(data::user_tags(&world, user.id)));
    tpl.set("user", beans::player_details(&world, &user));
    conn.render(tpl)?;
    crate::rcon::send(&world, "refresh_tags", &[("userId", user.id.to_string())]);
    Ok(())
}

/// `/habblet/mytagslist`.
pub fn my_tag_list(conn: &mut Conn) -> WebResult {
    let user_id = conn.session().int("user.id");
    if user_id < 1 {
        return Ok(());
    }
    let mut tpl = conn.template("habblet/myTagList");
    tpl.set(
        "tags",
        Value::list(data::user_tags(conn.world(), user_id as u32)),
    );
    tpl.set("tagRandomQuestion", super::me::random_question());
    conn.render(tpl)
}

/// Java: `TagDao.countTag`.
fn count_tag(world: &World, tag: &str) -> usize {
    world
        .store
        .records_where::<Tag>(|t| same_tag(&t.tag, tag))
        .map(|v| v.len())
        .unwrap_or(0)
}

/// `/habblet/ajax/tagfight`.
pub fn tag_fight(conn: &mut Conn) -> WebResult {
    let first = remove_html_tags(&conn.post.string("tag1"));
    let second = remove_html_tags(&conn.post.string("tag2"));
    let (a, b) = (
        count_tag(conn.world(), &first),
        count_tag(conn.world(), &second),
    );
    let (image, result) = match b.cmp(&a) {
        std::cmp::Ordering::Greater => (1, "The winner is:"),
        std::cmp::Ordering::Less => (2, "The winner is:"),
        std::cmp::Ordering::Equal => (0, "Tie!"),
    };
    let mut tpl = conn.template("habblet/tagFightResult");
    tpl.set("result", result);
    tpl.set("resultTag1", first);
    tpl.set("resultTag2", second);
    tpl.set("resultHits1", a);
    tpl.set("resultHits2", b);
    tpl.set("tagFightImage", image);
    conn.render(tpl)
}

/// `/habblet/ajax/tagmatch`.
pub fn tag_match(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("habblet/tagMatch");
    let user_id = conn.user_id();
    let friend = conn
        .world()
        .store
        .user_by_name(&conn.post.string("friendName"))?;
    let is_friend = friend.is_some_and(|f| {
        conn.world()
            .store
            .messenger_friend_exists(user_id, f.id)
            .unwrap_or(false)
    });
    tpl.set(
        "errorMsg",
        if is_friend {
            ""
        } else {
            "Friend not found. Are you sure that they really exist?"
        },
    );
    conn.render(tpl)
}

/// `/myhabbo/tag/remove_all_tags`.
pub fn remove_all(conn: &mut Conn) -> WebResult {
    let user_id = conn.session().int("user.id");
    if user_id < 1 {
        conn.send_html("Please login to remove all your tags.");
        return Ok(());
    }
    let tags = data::user_tags(conn.world(), user_id as u32);
    remove_tags(conn.world(), user_id as u32, 0, 0, None)?;
    conn.send_html(format!(
        "All tags removed!<br><br>The tags removed: {}",
        tags.join(", ")
    ));
    Ok(())
}
