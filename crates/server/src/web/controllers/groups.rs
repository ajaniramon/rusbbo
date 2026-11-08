//! Java: `controllers/groups/*`: `GroupController`, `GroupHabbletController`,
//! `GroupMemberController`, `GroupFavouriteController`, `GroupTagController` and the forum
//! controllers (`GroupDiscussionsController`, `DiscussionController`,
//! `DiscussionActionsController`, `DiscussionPreviewController`).

use std::sync::Arc;

use rusbbo_pebble::Value;
use rusbbo_storage::{ForumReply, Group, GroupEditSession, GroupMember, Sticker, User};

use super::homes::{
    authenticated, group_edit_session, int_or, int_param, is_numeric, java_split, java_truncate,
    not_found, parse_position, viewer, world, x_json,
};
use super::tags::{add_tag, group_tags, remove_tags, valid_tag};
use super::{remove_html_tags, xss_clear};
use crate::web::beans::now;
use crate::web::homes::{self, GROUP_WIDGET};
use crate::web::tpl::Tpl;
use crate::web::{Conn, Resp, WebError, WebResult, data, forum, jdate, model, text};
use crate::world::World;

fn npe() -> WebError {
    WebError("NullPointerException".into())
}

/// Java: `GroupDao.getGroup`.
fn group(world: &World, id: i64) -> Option<Group> {
    u32::try_from(id)
        .ok()
        .and_then(|id| world.store.record::<Group>(id).ok().flatten())
}

/// Java: `GroupDao.getGroupByAlias` (`alias = ?`, ignoring case as the collation does).
fn group_by_alias(world: &World, alias: &str) -> Option<Group> {
    let alias = alias.to_lowercase();
    world
        .store
        .records_where::<Group>(|g| g.alias.as_ref().is_some_and(|a| a.to_lowercase() == alias))
        .ok()?
        .into_iter()
        .next()
}

/// Java: `Group.getMember`: the owner, or a member who is not pending.
fn member(world: &World, g: &Group, user_id: i64) -> Option<GroupMember> {
    data::group_member(world, g.id, g.owner_id, user_id).filter(|m| !m.is_pending)
}

/// Java: `Group.getPendingMember` (`Err` where Java throws: no row at all).
fn pending_member(world: &World, g: &Group, user_id: i64) -> Result<Option<GroupMember>, WebError> {
    if g.owner_id as i64 == user_id {
        return Ok(data::group_member(world, g.id, g.owner_id, user_id));
    }
    let m = data::membership(world, g.id, user_id).ok_or_else(npe)?;
    Ok(m.is_pending.then_some(m))
}

fn click_link(world: &Arc<World>, g: &Group) -> String {
    match model::group(world, g) {
        Value::Map(m) => m
            .get("generateClickLink")
            .map(Value::render)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn set_games_page(conn: &mut Conn, g: &Group) {
    if let Some(alias) = &g.alias
        && [
            "battleball_rebound",
            "lido",
            "snow_storm",
            "wobble_squabble",
        ]
        .iter()
        .any(|a| alias.eq_ignore_ascii_case(a))
    {
        conn.session().set("page", "games");
    }
}

/// Java: `GroupEditDao.getSession`.
fn edit_expiry(world: &World, user_id: u32, group_id: u32) -> Option<i64> {
    let now = now();
    world
        .store
        .records_where::<GroupEditSession>(|s| {
            s.user_id == user_id && s.group_id == group_id && s.expire > now
        })
        .ok()?
        .into_iter()
        .next()
        .map(|s| s.expire)
}

/// Java: `GroupEditDao.delete`.
fn delete_edit_sessions(world: &World, user_id: u32, group_id: u32) -> anyhow::Result<()> {
    for s in world
        .store
        .records_where::<GroupEditSession>(|s| s.user_id == user_id && s.group_id == group_id)?
    {
        world.store.delete_record::<GroupEditSession>(s.id)?;
    }
    Ok(())
}

fn member_value(world: &Arc<World>, m: Option<GroupMember>) -> Value {
    m.map(|m| model::group_member_bean(world, m))
        .unwrap_or(Value::Null)
}

// ---- GroupController ----

/// `/groups/*`, `/groups/*/id`.
pub fn view_group(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.session().contains("authenticated") {
        return Ok(());
    }
    conn.session().set("page", "community");
    let world = world(conn);
    let matched = conn.wildcard(0).to_string();
    let by_id = conn.route.ends_with("/id");
    let mut found = None;
    if is_numeric(&matched) && by_id {
        let Some(g) = group(&world, matched.parse::<i64>().unwrap_or(-1)) else {
            not_found(conn);
            return Ok(());
        };
        let alias = g.alias.clone().unwrap_or_default();
        if !alias.trim().is_empty() {
            conn.redirect(&format!("/groups/{alias}"));
            return Ok(());
        }
        found = Some(g);
    } else if !by_id {
        found = group_by_alias(&world, &matched);
    }
    let Some(g) = found else {
        not_found(conn);
        return Ok(());
    };
    let mut session_time = -1;
    if conn.is_authenticated() {
        session_time = edit_expiry(&world, conn.user_id(), g.id).unwrap_or(-1);
    }
    if session_time != -1 {
        let mut s = conn.session();
        s.delete("homeEditSession");
        s.set("groupEditSession", g.id as i64);
    }
    set_games_page(conn, &g);
    let mut tpl = conn.template("groups");
    tpl.set("editMode", session_time != -1);
    tpl.set("group", model::group(&world, &g));
    let placed = homes::group_widgets(&world, g.id, Some(true));
    tpl.set(
        "stickers",
        Value::list(
            placed
                .iter()
                .map(|w| homes::widget(&world, w, 1))
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set("tags", Value::list(group_tags(&world, g.id)));
    let guestbook = homes::group_widgets(&world, g.id, None)
        .into_iter()
        .find(|w| homes::product_data(&world, w).eq_ignore_ascii_case("guestbookwidget"))
        .ok_or_else(|| WebError("NoSuchElementException".into()))?;
    tpl.set("guestbookSetting", homes::guestbook_state(&guestbook));
    tpl.set("stickerLimit", homes::sticker_limit(true));
    if session_time != -1 {
        tpl.set("expireMinutes", (session_time - now()) / 60);
    }
    if g.room_id > 0
        && let Some(room) = world.store.room(g.room_id).ok().flatten()
    {
        tpl.set("room", model::room(&world, &room));
    }
    tpl.set("hasMember", false);
    if conn.is_authenticated()
        && let Some(m) = member(&world, &g, conn.user_id() as i64)
    {
        tpl.set("hasMember", true);
        tpl.set("groupMember", model::group_member_bean(&world, m));
    }
    conn.render(tpl)
}

/// `/groups/actions/startEditingSession/*`.
pub fn start_editing_session(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let user_id = conn.user_id();
    let matched = conn.wildcard(0).to_string();
    let found = if is_numeric(&matched) {
        group(&world, matched.parse::<i64>().unwrap_or(-1))
    } else {
        None
    };
    let Some(g) = found else {
        not_found(conn);
        return Ok(());
    };
    let is_admin = member(&world, &g, user_id as i64).is_some_and(|m| m.member_rank >= 2);
    if is_admin && !group_edit_session(&world, user_id, g.id) {
        delete_edit_sessions(&world, user_id, g.id)?;
        let mut s = GroupEditSession {
            user_id,
            group_id: g.id,
            expire: now() + 30 * 60,
            ..Default::default()
        };
        world.store.insert_record(&mut s)?;
        let mut s = conn.session();
        s.delete("homeEditSession");
        s.set("groupEditSession", g.id as i64);
    }
    conn.redirect(&click_link(&world, &g));
    Ok(())
}

/// `/groups/actions/cancelEditingSession`.
pub fn cancel_editing_session(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    if !conn.session().contains("groupEditSession") {
        conn.redirect("/");
        return Ok(());
    }
    let world = world(conn);
    let user_id = conn.user_id();
    let group_id = conn.session().int("groupEditSession");
    if group_id >= 0 && group_edit_session(&world, user_id, group_id as u32) {
        delete_edit_sessions(&world, user_id, group_id as u32)?;
        let mut s = conn.session();
        s.delete("homeEditSession");
        s.delete("groupEditSession");
    }
    let g = group(&world, group_id as i64).ok_or_else(npe)?;
    conn.redirect(&click_link(&world, &g));
    Ok(())
}

/// `/groups/actions/saveEditingSession`.
pub fn save_editing_session(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Some(user) = viewer(conn) else {
        {
            let mut s = conn.session();
            s.delete("user.id");
            s.delete("authenticated");
        }
        conn.redirect("/");
        return Ok(());
    };
    let world = world(conn);
    let group_id = conn.session().int("groupEditSession");
    let Some(mut g) = group(&world, group_id as i64) else {
        conn.send_html("");
        return Ok(());
    };
    if !group_edit_session(&world, user.id, g.id) {
        conn.send_html("");
        return Ok(());
    }
    let mut placed = homes::group_widgets(&world, g.id, Some(true));
    let outcome = (|| -> Result<(), bool> {
        if conn.post.contains("background") {
            let background: i32 = conn
                .post
                .string("background")
                .split(':')
                .next()
                .unwrap_or("")
                .parse()
                .map_err(|_| false)?;
            if let Some(w) = homes::inventory_widgets(&world, user.id, None)
                .into_iter()
                .find(|w| w.id as i64 == background as i64)
            {
                g.background = homes::product_data(&world, &w);
                world.store.put_record(&g).map_err(|_| false)?;
            }
        }
        for (i, key) in ["stickers", "widgets", "stickienotes"].iter().enumerate() {
            if !conn.post.contains(key) {
                continue;
            }
            let value = conn.post.string(key);
            let entries = java_split(&value, '/');
            if i == 0 && entries.len() as i64 >= homes::sticker_limit(true) {
                return Err(true);
            }
            for entry in entries {
                let (id, x, y, z) = parse_position(entry).ok_or(false)?;
                if let Some(w) = placed.iter_mut().find(|w| w.id as i64 == id) {
                    w.x = x.to_string();
                    w.y = y.to_string();
                    w.z = z.to_string();
                    homes::save(&world, w).map_err(|_| false)?;
                }
            }
        }
        Ok(())
    })();
    if outcome == Err(true) {
        conn.send_html("");
        return Ok(());
    }
    delete_edit_sessions(&world, user.id, g.id)?;
    {
        let mut s = conn.session();
        s.delete("homeEditSession");
        s.delete("groupEditSession");
    }
    conn.send_html(format!(
        "<script language=\"JavaScript\" type=\"text/javascript\">\nwaitAndGo('{}');\n</script>",
        click_link(&world, &g)
    ));
    Ok(())
}

/// `/myhabbo/groups/groupinfo`.
pub fn group_info(conn: &mut Conn) -> WebResult {
    let id = int_param(conn.post.int("groupId"))?;
    let world = world(conn);
    let Some(g) = group(&world, id) else {
        conn.send_html("");
        return Ok(());
    };
    let mut tpl = conn.template("homes/widget/habblet/groupinfo");
    tpl.set("group", model::group(&world, &g));
    conn.render(tpl)
}

// ---- GroupHabbletController ----

fn group_cost(world: &World) -> i64 {
    data::cfg_int(world, "group.purchase.cost")
}

/// `HtmlUtil.removeHtmlTags(StringUtil.filterInput(s, true))`.
fn clean_input(s: &str) -> String {
    remove_html_tags(&text::filter_input(s, true))
}

/// `/groups/actions/group_create_form`.
pub fn group_create_form(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let world = world(conn);
    let user = viewer(conn).ok_or_else(npe)?;
    let cost = group_cost(&world);
    if (user.credits as i64) < cost {
        let tpl = conn.template("groups/habblet/purchase_result_error");
        return conn.render(tpl);
    }
    let mut tpl = conn.template("groups/habblet/group_create_form");
    tpl.set("groupCost", cost);
    conn.render(tpl)
}

/// `/groups/actions/purchase_confirmation`.
pub fn purchase_confirmation(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let world = world(conn);
    let name = clean_input(&conn.post.string("name"));
    let mut tpl = conn.template("groups/habblet/purchase_confirmation");
    tpl.set("groupName", name);
    tpl.set("groupCost", group_cost(&world));
    conn.render(tpl)
}

/// `/groups/actions/purchase_ajax`.
pub fn purchase_ajax(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let world = world(conn);
    let mut tpl = conn.template("groups/habblet/purchase_ajax");
    let mut user = viewer(conn).ok_or_else(npe)?;
    let cost = group_cost(&world);
    if (user.credits as i64) < cost {
        conn.send_html("");
        return Ok(());
    }
    crate::game::currency::decrease_credits(&world.store, &mut user, cost as i32);
    crate::rcon::send(
        &world,
        "refresh_credits",
        &[("userId", user.id.to_string())],
    );
    let name = clean_input(&conn.post.string("name"));
    let description = clean_input(&conn.post.string("description"));
    let mut g = Group {
        name: name.clone(),
        description,
        owner_id: user.id,
        badge: "b0503Xs09114s05013s05015".into(),
        background: "bg_colour_08".into(),
        created_at: now(),
        ..Default::default()
    };
    let group_id = world.store.insert_record(&mut g)?;
    let product = |data: &str| {
        homes::product_by_data(&world, data, GROUP_WIDGET)
            .map(|p| p.id)
            .ok_or_else(npe)
    };
    let widgets = [
        ((40, 34, 6), "guestbookwidget", true),
        ((433, 40, 3), "groupinfowidget", true),
        ((0, 0, 0), "memberwidget", false),
        ((0, 0, 0), "traxplayerwidget", false),
    ];
    for (pos, data, placed) in widgets {
        let id = product(data)?;
        let mut w = world.store.write()?;
        homes::purchase_widget(&mut w, 0, pos, 1, id, "", group_id as i32, placed)?;
        w.commit()?;
    }
    tpl.set("groupName", name);
    tpl.set("groupId", group_id);
    tpl.set("deductedCredits", user.credits);
    conn.render(tpl)
}

/// The group a settings action names, when the user owns it.
fn owned_group(conn: &Conn) -> Result<Option<Group>, WebError> {
    let id = int_param(conn.post.int("groupId"))?;
    Ok(group(conn.world(), id).filter(|g| g.owner_id == conn.user_id()))
}

/// `/groups/actions/group_settings`.
pub fn group_settings(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let id = int_param(conn.post.int("groupId"))?;
    let Some(g) = group(&world, id) else {
        conn.send_html("");
        return Ok(());
    };
    let user_id = conn.user_id();
    if g.owner_id != user_id {
        conn.send_html("");
        return Ok(());
    }
    let mut tpl = conn.template("groups/habblet/group_settings");
    tpl.set("group", model::group(&world, &g));
    let checked = " checked=\"checked\"";
    tpl.set(&format!("selected{}GroupType", g.group_type), checked);
    tpl.set(&format!("selected{}ForumType", g.forum_type), checked);
    tpl.set(
        &format!("selected{}ForumPermissionType", g.forum_permission),
        checked,
    );
    tpl.set(
        "charactersLeft",
        (255 - g.description.encode_utf16().count() as i64).to_string(),
    );
    let rooms: Vec<Value> = world
        .store
        .rooms_by_owner(user_id)?
        .iter()
        .filter(|r| r.group_id == 0 || r.group_id == g.id as i32)
        .map(|r| model::room(&world, r))
        .collect();
    tpl.set("rooms", Value::list(rooms));
    conn.render(tpl)
}

/// `/groups/actions/check_group_url`.
pub fn check_group_url(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let mut tpl = conn.template("groups/habblet/check_group_url");
    tpl.set("url", text::html_escape(&conn.post.string("url")));
    conn.render(tpl)
}

/// Java: `RoomDao.saveGroupId(roomId, groupId)`, whose parameters are swapped: room
/// `group_id`'s group becomes `room_id`.
fn save_room_group(world: &World, room_id: i64, group_id: i64) -> anyhow::Result<()> {
    if let Ok(id) = u32::try_from(group_id) {
        world
            .store
            .update_room(id, |room| room.group_id = room_id as i32)?;
    }
    Ok(())
}

/// `/groups/actions/update_group_settings`.
pub fn update_group_settings(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let user_id = conn.user_id();
    let Some(mut g) = owned_group(conn)? else {
        return Ok(());
    };
    let name = java_truncate(&remove_html_tags(&conn.post.string("name")), 30);
    let description = java_truncate(&remove_html_tags(&conn.post.string("description")), 255);
    let url: String = conn
        .post
        .string("url")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(30)
        .collect();
    let mut group_type = int_param(conn.post.int("type"))?;
    let mut forum_type = int_param(conn.post.int("forumType"))?;
    let mut permission = int_param(conn.post.int("newTopicPermission"))?;
    let mut room_id = if conn.post.string("roomId").is_empty() {
        0
    } else {
        int_or(conn.post.int("roomId"), 0)
    };
    if !(0..=3).contains(&group_type) {
        group_type = 0;
    }
    if room_id < 0 {
        room_id = 0;
    }
    if !(0..=1).contains(&forum_type) {
        forum_type = 0;
    }
    if !(0..=2).contains(&permission) {
        permission = 0;
    }
    g.name = name;
    g.description = description;
    if g.group_type != 3 {
        g.group_type = group_type as i32;
    }
    g.forum_type = forum_type as i32;
    g.forum_permission = permission as i32;
    if g.alias.as_ref().is_none_or(|a| a.trim().is_empty()) {
        g.alias = None;
        if !url.trim().is_empty() && group_by_alias(&world, &url).is_none() {
            g.alias = Some(url);
        }
    }
    save_room_group(&world, g.room_id as i64, 0)?;
    if room_id > 0 {
        match u32::try_from(room_id)
            .ok()
            .and_then(|id| world.store.room(id).ok().flatten())
        {
            Some(room) if room.owner_id == user_id => {
                save_room_group(&world, room_id, g.id as i64)?;
            }
            _ => room_id = 0,
        }
    }
    g.room_id = room_id as u32;
    world.store.put_record(&g)?;
    crate::rcon::send(&world, "refreshgroup", &[("groupId", g.id.to_string())]);
    let mut tpl = conn.template("groups/habblet/update_group_settings");
    tpl.set("group", model::group(&world, &g));
    tpl.set("message", "Editing group settings successful");
    conn.render(tpl)
}

/// `/groups/actions/show_badge_editor`.
pub fn show_badge_editor(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let Some(g) = owned_group(conn)? else {
        return Ok(());
    };
    let mut tpl = conn.template("groups/habblet/show_badge_editor");
    tpl.set("group", model::group(&world, &g));
    conn.render(tpl)
}

/// `/groups/actions/update_group_badge`.
pub fn update_group_badge(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let badge = remove_html_tags(&conn.post.string("code"));
    let Some(mut g) = owned_group(conn)? else {
        return Ok(());
    };
    g.badge = badge.chars().filter(char::is_ascii_alphanumeric).collect();
    world.store.put_record(&g)?;
    crate::rcon::send(&world, "refreshgroup", &[("groupId", g.id.to_string())]);
    conn.redirect(&click_link(&world, &g));
    Ok(())
}

/// `/groups/actions/confirm_delete_group`.
pub fn confirm_delete_group(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let Some(g) = owned_group(conn)? else {
        return Ok(());
    };
    let mut tpl = conn.template("groups/habblet/confirm_delete_group");
    tpl.set("group", model::group(&world, &g));
    conn.render(tpl)
}

/// `/groups/actions/delete_group`.
pub fn delete_group(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let Some(g) = owned_group(conn)? else {
        return Ok(());
    };
    let gid = g.id as i32;
    for s in world
        .store
        .records_where::<Sticker>(|s| s.group_id == gid)?
    {
        if s.user_id == 0 {
            world.store.delete_record::<Sticker>(s.id)?;
        } else {
            world.store.put_record(&Sticker { group_id: 0, ..s })?;
        }
    }
    for m in world
        .store
        .records_where::<GroupMember>(|m| m.group_id == g.id)?
    {
        world.store.delete_record::<GroupMember>(m.id)?;
    }
    for u in world.store.users()? {
        if u.favourite_group == gid {
            world.store.update_user(u.id, |u| u.favourite_group = 0)?;
        }
    }
    world.store.delete_record::<Group>(g.id)?;
    crate::rcon::send(&world, "groupdeleted", &[("groupId", g.id.to_string())]);
    let tpl = conn.template("groups/habblet/delete_group");
    conn.render(tpl)
}

// ---- GroupMemberController ----

/// Java: `GroupMemberDao.addMember`.
fn add_member(world: &World, user_id: u32, group_id: u32, pending: bool) -> anyhow::Result<()> {
    let mut m = GroupMember {
        user_id,
        group_id,
        member_rank: 1,
        is_pending: pending,
        created_at: now(),
        ..Default::default()
    };
    world.store.insert_record(&mut m)?;
    Ok(())
}

/// Java: `GroupMemberDao.deleteMember`.
fn delete_member(world: &World, user_id: u32, group_id: u32) -> anyhow::Result<()> {
    for m in world
        .store
        .records_where::<GroupMember>(|m| m.user_id == user_id && m.group_id == group_id)?
    {
        world.store.delete_record::<GroupMember>(m.id)?;
    }
    Ok(())
}

/// Java: `GroupMemberDao.updateMember`.
fn update_member(world: &World, user_id: u32, group_id: u32, rank: i32) -> anyhow::Result<()> {
    for mut m in world
        .store
        .records_where::<GroupMember>(|m| m.user_id == user_id && m.group_id == group_id)?
    {
        m.member_rank = rank;
        m.is_pending = false;
        world.store.put_record(&m)?;
    }
    Ok(())
}

fn save_favourite(world: &World, user_id: u32, group_id: i32) -> anyhow::Result<()> {
    world
        .store
        .update_user(user_id, |u| u.favourite_group = group_id)?;
    Ok(())
}

fn refresh_group_perms(world: &World, user_id: u32) {
    crate::rcon::send(
        world,
        "refreshgroupperms",
        &[("userId", user_id.to_string())],
    );
}

/// `/groups/actions/join`.
pub fn join(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let id = int_param(conn.post.int("groupId"))?;
    let Some(g) = group(&world, id) else {
        conn.send_html("");
        return Ok(());
    };
    let user_id = conn.user_id();
    let user = viewer(conn);
    let uid = user_id as i64;
    if user.is_none()
        || data::is_member(&world, g.id, g.owner_id, uid)
        || data::is_pending_member(&world, g.id, g.owner_id, uid)
        || g.group_type == 2
    {
        conn.send_html("");
        return Ok(());
    }
    let staff = user.as_ref().is_some_and(|u| u.rank >= 5);
    if g.group_type == 0 || g.group_type == 3 || staff {
        let tpl = conn.template("groups/member/member_added");
        conn.render(tpl)?;
    }
    if g.group_type == 1 {
        let tpl = conn.template("groups/member/member_added_request");
        conn.render(tpl)?;
    }
    add_member(&world, user_id, g.id, !staff && g.group_type == 1)?;
    Ok(())
}

/// `/groups/actions/confirm_leave`.
pub fn confirm_leave(conn: &mut Conn) -> WebResult {
    let tpl = conn.template("groups/member/confirm_leave");
    conn.render(tpl)
}

/// `/groups/actions/leave`.
pub fn leave(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let id = int_param(conn.post.int("groupId"))?;
    let Some(g) = group(&world, id) else {
        conn.send_html("");
        return Ok(());
    };
    let user_id = conn.user_id();
    if data::is_member(&world, g.id, g.owner_id, user_id as i64) {
        delete_member(&world, user_id, g.id)?;
        let favourite = world
            .store
            .user(user_id)?
            .is_some_and(|u| u.favourite_group == g.id as i32);
        if favourite {
            save_favourite(&world, user_id, 0)?;
            refresh_group_perms(&world, user_id);
        }
    }
    let mut tpl = conn.template("groups/member/leave");
    tpl.set("groupId", id);
    conn.render(tpl)
}

/// `/groups/actions/memberlist`.
pub fn member_list(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let id = int_param(conn.post.int("groupId"))?;
    let user_id = conn.user_id();
    let Some(g) = group(&world, id).filter(|g| {
        data::group_member(&world, g.id, g.owner_id, user_id as i64)
            .is_some_and(|m| m.member_rank >= 2)
    }) else {
        conn.send_html("");
        return Ok(());
    };
    let page = int_or(conn.post.int("pageNumber"), 1).max(1);
    let pending = data::count_members(&world, g.id, true);
    let members = data::count_members(&world, g.id, false);
    let limit = 12;
    let is_pending = conn.post.boolean("pending");
    let start = ((page - 1) * limit) as usize;
    let list: Vec<Value> = world
        .store
        .records_where::<GroupMember>(|m| m.group_id == g.id && m.is_pending == is_pending)?
        .into_iter()
        .filter(|m| world.store.user(m.user_id).ok().flatten().is_some())
        .skip(start)
        .take(limit as usize)
        .map(|m| model::group_member_bean(&world, m))
        .collect();
    let count = if is_pending { pending } else { members };
    let pages = if count > 0 {
        (count + limit - 1) / limit
    } else {
        1
    };
    let self_member = member_value(&world, member(&world, &g, user_id as i64));
    x_json(
        conn,
        format!(
            "{{\"pending\":\"Pending members ({pending})\",\"members\":\"Members ({members})\"}}"
        ),
    );
    let mut tpl = conn.template("groups/memberlist");
    tpl.set("pages", pages);
    tpl.set("memberList", Value::list(list));
    tpl.set("currentPage", page);
    tpl.set("selfMember", self_member);
    conn.render(tpl)
}

fn confirm_targets(conn: &mut Conn, view: &str) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let n = conn.post.array("targetIds").len();
    let mut tpl = conn.template(view);
    tpl.set("targetIds", n);
    conn.render(tpl)
}

pub fn confirm_revoke_rights(conn: &mut Conn) -> WebResult {
    confirm_targets(conn, "groups/member/confirm_revoke_rights")
}

pub fn confirm_give_rights(conn: &mut Conn) -> WebResult {
    confirm_targets(conn, "groups/member/confirm_give_rights")
}

pub fn confirm_remove(conn: &mut Conn) -> WebResult {
    confirm_targets(conn, "groups/member/confirm_remove")
}

/// The member actions' check: the acting user's rank in the group (`Err` where Java throws on a
/// non-member).
fn actor_rank(conn: &Conn) -> Result<Option<(Group, i32)>, WebError> {
    let id = int_param(conn.post.int("groupId"))?;
    let world = conn.world();
    let Some(g) = group(world, id) else {
        return Ok(None);
    };
    let m = member(world, &g, conn.user_id() as i64).ok_or_else(npe)?;
    Ok(Some((g, m.member_rank)))
}

fn target_ids(conn: &Conn) -> Vec<i64> {
    conn.post
        .array("targetIds")
        .iter()
        .filter(|s| is_numeric(s))
        .filter_map(|s| s.parse::<i32>().ok().map(i64::from))
        .collect()
}

/// `/groups/actions/give_rights` (`rank` 2) and `revoke_rights` (1).
fn set_rights(conn: &mut Conn, rank: i32) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Some((g, _)) = actor_rank(conn)?.filter(|(_, r)| *r == 3) else {
        conn.send_html("");
        return Ok(());
    };
    let world = world(conn);
    for id in target_ids(conn) {
        let Some(m) = member(&world, &g, id).filter(|m| m.member_rank != 3) else {
            continue;
        };
        update_member(&world, m.user_id, m.group_id, rank)?;
    }
    conn.send_html("OK");
    Ok(())
}

pub fn give_rights(conn: &mut Conn) -> WebResult {
    set_rights(conn, 2)
}

pub fn revoke_rights(conn: &mut Conn) -> WebResult {
    set_rights(conn, 1)
}

/// `/groups/actions/remove`.
pub fn remove(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Some((g, _)) = actor_rank(conn)?.filter(|(_, r)| *r >= 2) else {
        conn.send_html("");
        return Ok(());
    };
    let world = world(conn);
    let actor = conn.user_id();
    for id in target_ids(conn) {
        let Some(m) = member(&world, &g, id).filter(|m| m.member_rank < 2) else {
            continue;
        };
        delete_member(&world, m.user_id, m.group_id)?;
        if let Some(u) = world.store.user(m.user_id)?
            && u.favourite_group == g.id as i32
        {
            save_favourite(&world, m.user_id, 0)?;
            if u.is_online {
                // Java refreshes the acting user, not the removed one.
                refresh_group_perms(&world, actor);
            }
        }
    }
    conn.send_html("OK");
    Ok(())
}

fn group_name_page(conn: &mut Conn, view: &str) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let id = int_param(conn.post.int("groupId"))?;
    let world = world(conn);
    let Some(g) = group(&world, id) else {
        conn.send_html("");
        return Ok(());
    };
    let mut tpl = conn.template(view);
    tpl.set("groupName", g.name);
    conn.render(tpl)
}

pub fn confirm_accept(conn: &mut Conn) -> WebResult {
    group_name_page(conn, "groups/member/confirm_accept")
}

/// `/groups/actions/accept` and `decline`.
fn answer_requests(conn: &mut Conn, accept: bool) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Some((g, _)) = actor_rank(conn)?.filter(|(_, r)| *r >= 2) else {
        conn.send_html("");
        return Ok(());
    };
    let world = world(conn);
    for id in target_ids(conn) {
        let Some(m) = pending_member(&world, &g, id)?.filter(|m| m.member_rank < 2) else {
            continue;
        };
        if accept {
            update_member(&world, m.user_id, m.group_id, 1)?;
        } else {
            delete_member(&world, m.user_id, m.group_id)?;
        }
    }
    conn.send_html("OK");
    Ok(())
}

pub fn accept(conn: &mut Conn) -> WebResult {
    answer_requests(conn, true)
}

pub fn decline(conn: &mut Conn) -> WebResult {
    answer_requests(conn, false)
}

/// `/groups/actions/confirm_decline`.
pub fn confirm_decline(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let ids = conn.post.string("targetIds");
    let n = java_split(&ids, ',').len().max(1);
    let mut tpl = conn.template("groups/member/confirm_decline");
    tpl.set("targetIds", n);
    conn.render(tpl)
}

// ---- GroupFavouriteController ----

pub fn confirm_select_favourite(conn: &mut Conn) -> WebResult {
    group_name_page(conn, "groups/favourite/confirm_select_favourite")
}

pub fn confirm_deselect_favourite(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let tpl = conn.template("groups/favourite/confirm_deselect_favourite");
    conn.render(tpl)
}

fn favourite(conn: &mut Conn, select: bool) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let id = int_param(conn.post.int("groupId"))?;
    let world = world(conn);
    let user_id = conn.user_id();
    let Some(g) =
        group(&world, id).filter(|g| data::is_member(&world, g.id, g.owner_id, user_id as i64))
    else {
        conn.send_html("");
        return Ok(());
    };
    save_favourite(&world, user_id, if select { g.id as i32 } else { 0 })?;
    refresh_group_perms(&world, user_id);
    conn.send_html("OK");
    Ok(())
}

pub fn select_favourite(conn: &mut Conn) -> WebResult {
    favourite(conn, true)
}

pub fn deselect_favourite(conn: &mut Conn) -> WebResult {
    favourite(conn, false)
}

// ---- GroupTagController ----

fn tag_group(conn: &mut Conn, owner_only: bool) -> Result<Option<Group>, WebError> {
    let user_id = conn.session().int("user.id");
    if user_id < 1 {
        conn.send_html("");
        return Ok(None);
    }
    let id = int_param(conn.post.int("groupId"))?;
    match group(conn.world(), id) {
        Some(g) if !owner_only || g.owner_id as i64 == user_id as i64 => Ok(Some(g)),
        _ => {
            conn.send_html("");
            Ok(None)
        }
    }
}

/// `/groups/actions/add_tag`.
pub fn add_group_tag(conn: &mut Conn) -> WebResult {
    let Some(g) = tag_group(conn, true)? else {
        return Ok(());
    };
    let world = world(conn);
    if group_tags(&world, g.id).len() as i64 >= data::cfg_int(&world, "max.tags.groups") {
        conn.send_html("taglimit");
        return Ok(());
    }
    let Some(tag) = valid_tag(&world, &conn.post.string("tagName"), 0, 0, g.id) else {
        conn.send_html("invalidtag");
        return Ok(());
    };
    if text::filter_sentence(&world, &tag) == tag {
        add_tag(&world, &tag, 0, 0, g.id)?;
    }
    conn.send_html("valid");
    Ok(())
}

fn group_tag_list(conn: &mut Conn, g: &Group) -> WebResult {
    let world = world(conn);
    let mut tpl = conn.template("groups/habblet/listgrouptags");
    tpl.set("tags", Value::list(group_tags(&world, g.id)));
    tpl.set("group", model::group(&world, g));
    conn.render(tpl)
}

/// `/groups/actions/remove_tag`.
pub fn remove_group_tag(conn: &mut Conn) -> WebResult {
    let Some(g) = tag_group(conn, true)? else {
        return Ok(());
    };
    let world = world(conn);
    remove_tags(&world, 0, 0, g.id, Some(&conn.post.string("tagName")))?;
    group_tag_list(conn, &g)
}

/// `/groups/actions/list_tags`.
pub fn list_group_tag(conn: &mut Conn) -> WebResult {
    let Some(g) = tag_group(conn, false)? else {
        return Ok(());
    };
    group_tag_list(conn, &g)
}

// ---- The forums ----

fn page_links(tpl: &mut Tpl, page: i64, pages: i64) {
    for i in 1..=3 {
        tpl.set(
            &format!("previousPage{i}"),
            if page - i >= 1 { page - i } else { -1 },
        );
    }
    for i in 1..=3 {
        let n = page + i;
        tpl.set(
            &format!("nextPage{i}"),
            if n > 1 && n <= pages { n } else { -1 },
        );
    }
    tpl.set("currentPage", page);
    tpl.set("pages", pages);
}

fn session_user(conn: &Conn) -> u32 {
    conn.session().int("user.id").max(0) as u32
}

/// `GroupDiscussionsController.render`.
fn discussions_page(conn: &mut Conn, tpl: &mut Tpl, g: &Group, page: i64) {
    let world = world(conn);
    let public = g.forum_type == 0;
    let mut can_view = public;
    tpl.set("hasMember", false);
    tpl.set("canPostForum", false);
    if conn.is_authenticated() {
        let m = member(&world, g, conn.user_id() as i64);
        let can_post = match g.forum_permission {
            0 => true,
            2 => m.as_ref().is_some_and(|m| m.member_rank >= 2),
            _ => m.is_some(),
        };
        tpl.set("canPostForum", can_post);
        if let Some(m) = m {
            tpl.set("hasMember", true);
            tpl.set("groupMember", model::group_member_bean(&world, m));
            can_view = true;
        }
    }
    tpl.set("canViewForum", can_view);
    let page = page.max(1);
    let (per_page, _) = forum::per_page(&world);
    let mut topics = Vec::new();
    let mut pages = 1;
    if can_view {
        let count = forum::count_discussions(&world, g.id);
        pages = if count > 0 {
            (count + per_page - 1) / per_page
        } else {
            1
        };
        topics = forum::discussions(&world, g.id, page, per_page, session_user(conn));
    }
    page_links(tpl, page, pages);
    tpl.set("discussionTopics", Value::list(topics));
}

/// `/groups/*/discussions`, `/groups/*/id/discussions`.
pub fn view_discussions(conn: &mut Conn) -> WebResult {
    discussions(conn, false)
}

/// `/groups/*/discussions/page/*`, `/groups/*/id/discussions/page/*`.
pub fn view_discussions_page(conn: &mut Conn) -> WebResult {
    discussions(conn, true)
}

fn discussions(conn: &mut Conn, paged: bool) -> WebResult {
    xss_clear(conn);
    conn.session().set("page", "community");
    let world = world(conn);
    let route = conn.route.clone();
    let matched = conn.wildcard(0).to_string();
    let (by_id, by_alias) = if paged {
        (
            route.contains("/id/discussions"),
            route.contains("/discussions") && !route.contains("id/"),
        )
    } else {
        (
            route.ends_with("/id/discussions"),
            route.ends_with("/discussions"),
        )
    };
    let found = if is_numeric(&matched) && by_id {
        let Some(g) = group(&world, matched.parse::<i64>().unwrap_or(-1)) else {
            not_found(conn);
            return Ok(());
        };
        let alias = g.alias.clone().unwrap_or_default();
        if !alias.trim().is_empty() {
            conn.redirect(&format!("/groups/{alias}/discussions"));
            return Ok(());
        }
        Some(g)
    } else if by_alias {
        group_by_alias(&world, &matched)
    } else {
        None
    };
    let Some(g) = found else {
        not_found(conn);
        return Ok(());
    };
    let page = if paged {
        conn.wildcard(1).parse::<i32>().map_or(1, i64::from)
    } else {
        1
    };
    set_games_page(conn, &g);
    let mut tpl = conn.template("groups/view_discussions");
    tpl.set("group", model::group(&world, &g));
    discussions_page(conn, &mut tpl, &g, page);
    conn.render(tpl)
}

/// `DiscussionController.appendpagedata`.
fn topic_page(
    conn: &mut Conn,
    tpl: &mut Tpl,
    g: &Group,
    thread: &rusbbo_storage::ForumThread,
    topic: Value,
    page: i64,
) -> WebResult {
    let world = world(conn);
    let logged_in = conn.is_authenticated();
    let mut topic_admin = logged_in && viewer(conn).is_some_and(|u| u.rank >= 5);
    tpl.set("group", model::group(&world, g));
    tpl.set("hasMember", false);
    tpl.set("hasMessage", false);
    let public = g.forum_type == 0;
    let mut can_view = public;
    let mut can_reply = false;
    let user_id = session_user(conn);
    if logged_in {
        let m = member(&world, g, user_id as i64);
        can_view = public || m.is_some();
        can_reply = public || m.is_some();
        if let Some(m) = m {
            tpl.set("hasMember", true);
            if !topic_admin {
                topic_admin = m.member_rank >= 2;
            }
            tpl.set("groupMember", model::group_member_bean(&world, m));
        }
    }
    if !thread.is_open {
        can_reply = false;
    }
    tpl.set("canViewForum", can_view);
    tpl.set("canReplyForum", can_reply);
    if !can_view {
        tpl.set("hasMessage", true);
        tpl.set("message", "View forums denied. Please check that you are logged in and have the appropriate rights to view the forums. If you are logged in and still can't view the forums, the group may be private. If so, you need to join the group in order to view the forums. ");
        return Ok(());
    }
    tpl.set("firstReply", forum::first_reply(&world, thread.id));
    let (_, per_page) = forum::per_page(&world);
    let count = forum::count_replies(&world, thread.id);
    let pages = if count > 0 {
        (count + per_page - 1) / per_page
    } else {
        1
    };
    let replies = forum::replies(&world, thread.id, page, per_page, user_id);
    if user_id > 0 {
        let unread: Vec<u32> = replies
            .iter()
            .filter(|(_, v)| match v {
                Value::Map(m) => matches!(m.get("new"), Some(Value::Bool(true))),
                _ => false,
            })
            .map(|(r, _)| r.id)
            .collect();
        if !unread.is_empty() {
            forum::mark_read(&world, user_id, &unread)?;
        }
    }
    page_links(tpl, page, pages);
    tpl.set(
        "replyList",
        Value::list(replies.into_iter().map(|(_, v)| v).collect::<Vec<_>>()),
    );
    tpl.set("discussionId", thread.id);
    tpl.set("discussionTopic", topic);
    tpl.set("hasTopicAdmin", topic_admin);
    Ok(())
}

/// `/groups/*/discussions/*/id[/page/*]` and the `/id/discussions` forms.
pub fn view_discussion(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    conn.session().set("page", "community");
    let world = world(conn);
    let matched = conn.wildcard(0).to_string();
    let discussion_id = conn.wildcard(1).parse::<i32>().map_or(0, i64::from);
    let (page, has_page) = match conn.wildcard(2).parse::<i32>() {
        Ok(p) => (p as i64, true),
        Err(_) => (1, false),
    };
    let g = if is_numeric(&matched) && conn.route.contains("/id/discussions") {
        let Some(g) = group(&world, matched.parse::<i64>().unwrap_or(-1)) else {
            not_found(conn);
            return Ok(());
        };
        let alias = g.alias.clone().unwrap_or_default();
        if !alias.trim().is_empty() {
            let suffix = if has_page {
                format!("/page/{page}")
            } else {
                String::new()
            };
            conn.redirect(&format!(
                "/groups/{alias}/discussions/{discussion_id}/id{suffix}"
            ));
            return Ok(());
        }
        Some(g)
    } else {
        group_by_alias(&world, &matched)
    };
    let Some(g) = g else {
        not_found(conn);
        return Ok(());
    };
    let Some((thread, topic)) =
        forum::discussion(&world, g.id as i64, discussion_id, session_user(conn))
    else {
        not_found(conn);
        return Ok(());
    };
    let mut tpl = conn.template("groups/discussion");
    let page = page.max(1);
    let key = format!("hasViewedDiscussion{discussion_id}");
    if !conn.session().contains(&key) {
        conn.session().set(&key, true);
        forum::increment_views(&world, thread.id)?;
    }
    topic_page(conn, &mut tpl, &g, &thread, topic, page)?;
    conn.render(tpl)
}

/// `/discussions/actions/newtopic`.
pub fn new_topic(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !authenticated(conn) {
        return Ok(());
    }
    let tpl = conn.template("groups/discussions/newpost");
    conn.render(tpl)
}

fn replies_message(conn: &mut Conn, message: &str) -> WebResult {
    let mut tpl = conn.template("groups/discussion_replies");
    tpl.set("hasMessage", true);
    tpl.set("message", message);
    conn.render(tpl)
}

fn captcha_error(conn: &mut Conn) {
    conn.session().delete("captcha-text");
    let mut r = Resp::html(200, "");
    r.headers
        .push(("X-JSON".into(), "{\"captchaError\":\"true\"}".into()));
    conn.send(r);
}

/// `/discussions/actions/savetopic`.
pub fn save_topic(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !authenticated(conn) {
        return Ok(());
    }
    let captcha = conn.post.string("captcha");
    let message = conn.post.string("message");
    let mut title = conn.post.string("topicName");
    let group_id = int_or(conn.post.int("groupId"), 0);
    if title.trim().is_empty() || message.trim().is_empty() {
        return replies_message(conn, "Please supply a valid message.");
    }
    let expected = conn.session().string("captcha-text").ok_or_else(npe)?;
    if expected != captcha {
        captcha_error(conn);
        return Ok(());
    }
    let world = world(conn);
    let user_id = conn.user_id();
    if let Some(latest) = forum::latest_reply(&world, user_id)
        && latest.message.starts_with(&message)
    {
        return replies_message(conn, "Do not spam the forums");
    }
    let Some(g) = group(&world, group_id) else {
        conn.redirect("/");
        return Ok(());
    };
    if g.forum_type == 1 || g.forum_permission == 1 || g.forum_permission == 2 {
        let Some(m) = member(&world, &g, user_id as i64) else {
            conn.redirect("/");
            return Ok(());
        };
        if g.forum_permission == 2 && m.member_rank < 2 {
            conn.redirect("/");
            return Ok(());
        }
    }
    title = java_truncate(&title, 32);
    let topic_id = forum::create_discussion(&world, g.id, user_id, &title)?;
    forum::create_reply(&world, topic_id, user_id, &message)?;
    conn.session().delete("captcha-text");
    conn.send_html(format!(
        "{}/discussions/{topic_id}/id",
        click_link(&world, &g)
    ));
    Ok(())
}

/// `/discussions/actions/ping_session`.
pub fn ping_session(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let mut r = Resp::html(200, "");
    r.headers
        .push(("X-JSON".into(), "{\"privilegeLevel\":\"1\"}".into()));
    conn.send(r);
    Ok(())
}

fn post_ids(conn: &Conn) -> (i64, i64) {
    match (conn.post.int("groupId"), conn.post.int("topicId")) {
        (Some(g), Some(t)) => (g as i64, t as i64),
        (Some(g), None) => (g as i64, 0),
        _ => (0, 0),
    }
}

/// `/discussions/actions/opentopicsettings`.
pub fn open_topic_settings(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let group_id = int_or(conn.post.int("groupId"), 0);
    let topic_id = int_or(conn.post.int("topicId"), 0);
    let world = world(conn);
    let Some((_, topic)) = forum::discussion(&world, group_id, topic_id, session_user(conn)) else {
        conn.redirect("/");
        return Ok(());
    };
    let mut tpl = conn.template("groups/discussions/opentopicsettings");
    tpl.set("topic", topic);
    conn.render(tpl)
}

/// `/discussions/actions/confirm_delete_topic`.
pub fn confirm_delete_topic(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let tpl = conn.template("groups/discussions/confirm_delete_topic");
    conn.render(tpl)
}

/// Whether the user may manage a topic or post by someone else: staff, or a group admin.
fn may_moderate(world: &World, g: &Group, user: Option<&User>) -> bool {
    let Some(user) = user else {
        return false;
    };
    user.rank >= 5 || member(world, g, user.id as i64).is_some_and(|m| m.member_rank >= 2)
}

/// `/discussions/actions/deletetopic`.
pub fn delete_topic(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let group_id = int_or(conn.post.int("groupId"), 0);
    let topic_id = int_or(conn.post.int("topicId"), 0);
    let world = world(conn);
    let Some((thread, _)) = forum::discussion(&world, group_id, topic_id, session_user(conn))
    else {
        conn.redirect("/");
        return Ok(());
    };
    let Some(g) = group(&world, group_id) else {
        conn.redirect("/");
        return Ok(());
    };
    let user = viewer(conn);
    if thread.poster_id != conn.user_id() {
        if user.is_none() {
            return Err(npe());
        }
        if !may_moderate(&world, &g, user.as_ref()) {
            return Ok(());
        }
    }
    forum::delete_discussion(&world, group_id, topic_id)?;
    conn.send_html("SUCCESS");
    Ok(())
}

/// `/discussions/actions/savetopicsettings`.
pub fn save_topic_settings(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let (group_id, topic_id) = post_ids(conn);
    let world = world(conn);
    let Some((mut thread, _)) = forum::discussion(&world, group_id, topic_id, session_user(conn))
    else {
        conn.redirect("/");
        return Ok(());
    };
    let Some(g) = group(&world, group_id) else {
        conn.redirect("/");
        return Ok(());
    };
    let mut tpl = conn.template("groups/discussion_replies");
    let user = viewer(conn);
    if thread.poster_id != conn.user_id() && !may_moderate(&world, &g, user.as_ref()) {
        return Ok(());
    }
    let page = int_or(conn.post.int("page"), 1).max(1);
    let title = java_truncate(&conn.post.string("topicName"), 32);
    // Java sets the fields one by one inside a try: a bad number stops it there, and the page
    // shows the object as it was left.
    if let Some(closed) = conn.post.int("topicClosed") {
        thread.is_open = closed == 0;
        if let Some(sticky) = conn.post.int("topicSticky") {
            thread.is_stickied = sticky == 1;
            thread.topic_title = title;
            forum::save_thread(&world, &thread)?;
        }
    }
    let topic = forum::discussion_value(&world, &thread, session_user(conn)).ok_or_else(npe)?;
    topic_page(conn, &mut tpl, &g, &thread, topic, page)?;
    conn.render(tpl)
}

fn post_page(conn: &Conn) -> (i64, i64, i64, i64) {
    let mut out = (0, 0, 0, 1);
    let keys = ["groupId", "topicId", "postId", "page"];
    for (i, key) in keys.iter().enumerate() {
        match conn.post.int(key) {
            Some(v) => match i {
                0 => out.0 = v as i64,
                1 => out.1 = v as i64,
                2 => out.2 = v as i64,
                _ => out.3 = v as i64,
            },
            None => break,
        }
    }
    out.3 = out.3.max(1);
    out
}

/// `/discussions/actions/updatepost`.
pub fn update_post(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let (group_id, topic_id, post_id, page) = post_page(conn);
    let world = world(conn);
    let Some((thread, topic)) = forum::discussion(&world, group_id, topic_id, session_user(conn))
        .filter(|(t, _)| t.is_open)
    else {
        conn.redirect("/");
        return Ok(());
    };
    let Some(mut reply) = forum::reply(&world, thread.id, post_id) else {
        conn.redirect("/");
        return Ok(());
    };
    let Some(g) = group(&world, group_id) else {
        conn.redirect("/");
        return Ok(());
    };
    // Java's check is inverted: a matching captcha is the error.
    let captcha = conn.session().string("captcha-text");
    if captcha
        .as_deref()
        .is_some_and(|c| !c.trim().is_empty() && c == conn.post.string("captcha"))
    {
        captcha_error(conn);
        return Ok(());
    }
    if reply.poster_id != conn.user_id() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("groups/discussion_replies");
    let staff = viewer(conn).is_some_and(|u| u.rank >= 5);
    reply.message = text::filter_sentence(&world, &conn.post.string("message"));
    if !staff {
        reply.is_edited = true;
    }
    forum::save_reply(&world, &reply)?;
    conn.session().delete("captcha-text");
    topic_page(conn, &mut tpl, &g, &thread, topic, page)?;
    conn.render(tpl)
}

/// `/discussions/actions/deletepost`.
pub fn delete_post(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let (group_id, topic_id, post_id, page) = post_page(conn);
    let world = world(conn);
    let Some((thread, topic)) = forum::discussion(&world, group_id, topic_id, session_user(conn))
        .filter(|(t, _)| t.is_open)
    else {
        conn.redirect("/?1");
        return Ok(());
    };
    let Some(mut reply) = forum::reply(&world, thread.id, post_id) else {
        conn.redirect("/?2");
        return Ok(());
    };
    let Some(g) = group(&world, group_id) else {
        conn.redirect("/?3");
        return Ok(());
    };
    let mut tpl = conn.template("groups/discussion_replies");
    let user = viewer(conn);
    let user_id = conn.user_id();
    if reply.poster_id != user_id && !may_moderate(&world, &g, user.as_ref()) {
        return Ok(());
    }
    let staff = user.as_ref().is_some_and(|u| u.rank >= 5);
    if reply.poster_id != user_id || staff {
        world.store.delete_record::<ForumReply>(reply.id)?;
    } else {
        reply.is_deleted = true;
        forum::save_reply(&world, &reply)?;
    }
    topic_page(conn, &mut tpl, &g, &thread, topic, page)?;
    conn.render(tpl)
}

/// `/discussions/actions/savepost`.
pub fn save_post(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let (group_id, topic_id) = post_ids(conn);
    let expected = conn.session().string("captcha-text").ok_or_else(npe)?;
    if expected != conn.post.string("captcha") {
        captcha_error(conn);
        return Ok(());
    }
    let message = conn.post.string("message");
    if message.trim().is_empty() {
        return replies_message(conn, "Please supply a valid message.");
    }
    let world = world(conn);
    let mut tpl = conn.template("groups/discussion_replies");
    let staff = viewer(conn).is_some_and(|u| u.rank >= 5);
    let Some((thread, topic)) = forum::discussion(&world, group_id, topic_id, session_user(conn))
        .filter(|(t, _)| t.is_open || staff)
    else {
        conn.redirect("/");
        return Ok(());
    };
    let user_id = conn.user_id();
    if let Some(latest) = forum::latest_reply(&world, user_id)
        && latest.message.starts_with(&message)
    {
        tpl.set("hasMessage", true);
        tpl.set("message", "Do not spam the forums");
        return conn.render(tpl);
    }
    let Some(g) = group(&world, group_id) else {
        conn.redirect("/");
        return Ok(());
    };
    if g.forum_type == 1 && member(&world, &g, user_id as i64).is_none() {
        conn.redirect("/");
        return Ok(());
    }
    conn.session().delete("captcha-text");
    forum::create_reply(&world, topic_id.max(0) as u32, user_id, &message)?;
    let (_, per_page) = forum::per_page(&world);
    let count = forum::count_replies(&world, thread.id);
    let pages = if count > 0 {
        (count + per_page - 1) / per_page
    } else {
        1
    };
    // Java shows the topic as it read it before the new reply.
    topic_page(conn, &mut tpl, &g, &thread, topic, pages)?;
    conn.render(tpl)
}

fn preview(
    conn: &mut Conn,
    view: &str,
    name_key: &str,
    name: String,
    message_key: &str,
) -> WebResult {
    let world = world(conn);
    let message = conn.post.string("message");
    let user_id = conn.user_id();
    let (badge, group_badge) = forum::user_display_badges(&world, user_id);
    let mut tpl = conn.template(view);
    let date = |f: &str| {
        jdate::format(now(), f)
            .replace("am", "AM")
            .replace("pm", "PM")
            .replace('.', "")
    };
    tpl.set(name_key, name);
    tpl.set(
        message_key,
        text::bbcode(
            &world,
            &text::html_escape(&text::bbcode_normalise(&message)),
            false,
        ),
    );
    tpl.set("previewDay", date("MMM dd, yyyy"));
    tpl.set("previewTime", date("h:mm a"));
    tpl.set("userReplies", forum::count_user_replies(&world, user_id));
    tpl.set("hasBadge", false);
    tpl.set("hasGroup", false);
    if let Some(b) = badge {
        tpl.set("hasBadge", true);
        tpl.set("badge", b);
    }
    if let Some(b) = group_badge {
        tpl.set("hasGroup", true);
        let favourite = viewer(conn).map_or(0, |u| u.favourite_group);
        tpl.set("groupId", favourite);
        tpl.set("groupBadge", b);
    }
    conn.render(tpl)
}

/// `/discussions/actions/previewtopic`.
pub fn preview_topic(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let name = conn.post.string("topicName");
    preview(
        conn,
        "groups/discussions/previewtopic",
        "topicName",
        name,
        "topicMessage",
    )
}

/// `/discussions/actions/previewpost`.
pub fn preview_post(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let topic_id = int_or(conn.post.int("topicId"), 0);
    let group_id = int_or(conn.post.int("groupId"), 0);
    let world = world(conn);
    let Some((thread, _)) = forum::discussion(&world, group_id, topic_id, session_user(conn))
    else {
        conn.redirect("/");
        return Ok(());
    };
    let name = format!("RE: {}", text::filter_sentence(&world, &thread.topic_title));
    preview(
        conn,
        "groups/discussions/previewpost",
        "postName",
        name,
        "postMessage",
    )
}
