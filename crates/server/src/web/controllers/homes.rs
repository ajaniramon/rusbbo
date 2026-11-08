//! Java: `controllers/homes/*`: `HomesController`, `StoreController`, `NoteEditorController`,
//! `WidgetController` and the widget controllers (`GuestbookController`, `RateController`,
//! `BadgesController`, `TraxController`, `FriendsWidgetController`, `MemberWidgetController`).

use std::sync::Arc;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{GroupEditSession, GuestbookEntry, Sticker, User};

use super::xss_clear;
use crate::web::beans::{has_club, now, player_details};
use crate::web::homes::{self, BACKGROUND, GROUP_WIDGET, HOME_WIDGET, NOTE, STICKER};
use crate::web::tpl::Tpl;
use crate::web::{Conn, Resp, WebError, WebResult, data, jdate, model, text};
use crate::world::World;

/// `getInt` on a parameter: Java's `NumberFormatException` is the error page.
pub(super) fn int_param(v: Option<i32>) -> Result<i64, WebError> {
    v.map(i64::from)
        .ok_or_else(|| WebError("NumberFormatException".into()))
}

/// `getInt` inside Java's `try { } catch (Exception ex) { }`: the default on a bad number.
pub(super) fn int_or(v: Option<i32>, default: i64) -> i64 {
    v.map(i64::from).unwrap_or(default)
}

/// Java: `String.substring(0, n)` when the string is longer (UTF-16 units).
pub(super) fn java_truncate(s: &str, n: usize) -> String {
    if s.encode_utf16().count() <= n {
        return s.to_string();
    }
    String::from_utf16_lossy(&s.encode_utf16().take(n).collect::<Vec<u16>>())
}

/// Java's `StringUtil.isNumeric`.
pub(super) fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_numeric())
}

/// `if (!session.getBoolean("authenticated")) { redirect("/"); return; }`.
pub(super) fn authenticated(conn: &mut Conn) -> bool {
    if conn.is_authenticated() {
        true
    } else {
        conn.redirect("/");
        false
    }
}

/// `PlayerDao.getDetails(session user.id)`.
pub(super) fn viewer(conn: &Conn) -> Option<User> {
    conn.world().store.user(conn.user_id()).ok().flatten()
}

/// `tpl.get("playerDetails") == null`: Java logs the session out and goes home.
fn logged_out(conn: &mut Conn) -> Option<User> {
    let user = if conn.is_authenticated() {
        viewer(conn)
    } else {
        None
    };
    if user.is_none() {
        {
            let mut s = conn.session();
            s.delete("user.id");
            s.delete("authenticated");
        }
        conn.redirect("/");
    }
    user
}

pub(super) fn not_found(conn: &mut Conn) {
    let r = crate::web::server::not_found(conn);
    conn.send(r);
}

pub(super) fn x_json(conn: &mut Conn, json: String) {
    conn.extra_headers.push(("X-JSON".into(), json));
}

pub(super) fn world(conn: &Conn) -> Arc<World> {
    conn.web.world.clone()
}

fn nulls(n: usize) -> Value {
    Value::list(vec![Value::Null; n])
}

/// The template a widget renders with (Java: `Widget.template`).
fn widget_template(conn: &mut Conn, w: &Sticker) -> Tpl {
    let world = world(conn);
    let data = homes::product_data(&world, w).to_lowercase();
    let view = match data.as_str() {
        "groupinfowidget" => "homes/widget/group_info_widget",
        "guestbookwidget" => "homes/widget/guestbook_widget",
        "stickienote" => "homes/widget/note",
        "memberwidget" => "homes/widget/member_widget",
        "traxplayerwidget" => "homes/widget/trax_player_widget",
        "profilewidget" => "homes/widget/profile_widget",
        "roomswidget" => "homes/widget/rooms_widget",
        "highscoreswidget" => "homes/widget/highscores_widget",
        "badgeswidget" => "homes/widget/badges_widget",
        "friendswidget" => "homes/widget/friends_widget",
        "groupswidget" => "homes/widget/groups_widget",
        "ratingwidget" => "homes/widget/rating_widget",
        _ => "homes/widget/sticker",
    };
    let mut tpl = conn.template(view);
    let (home_edit, group_edit) = {
        let s = conn.session();
        (
            s.contains("homeEditSession"),
            s.contains("groupEditSession"),
        )
    };
    tpl.set("editMode", home_edit || group_edit);
    tpl.set("sticker", homes::widget(&world, w, 1));
    if home_edit && let Some(user) = viewer(conn) {
        tpl.set("user", player_details(&world, &user));
        tpl.set("canAddFriend", false);
        if data == "profilewidget" {
            profile_badge(&world, &user, &mut tpl, false);
        }
    }
    tpl
}

/// The badge and favourite group the profile widget shows. The page shows the first worn badge
/// by slot; `Widget.template` the first badge of all (its list is copied before the filter).
fn profile_badge(world: &Arc<World>, user: &User, tpl: &mut Tpl, worn_only: bool) {
    let mut badges = world.store.badges(user.id).unwrap_or_default();
    if worn_only {
        badges.retain(|b| b.equipped);
        badges.sort_by_key(|b| b.slot_id);
    }
    tpl.set("hasBadge", !badges.is_empty());
    if let Some(b) = badges.first() {
        tpl.set("badgeCode", b.code.as_str());
    }
    tpl.set("hasFavouriteGroup", false);
    if user.favourite_group > 0
        && let Some(g) = world
            .store
            .record::<rusbbo_storage::Group>(user.favourite_group as u32)
            .ok()
            .flatten()
    {
        tpl.set("hasFavouriteGroup", true);
        tpl.set("group", model::group(world, &g));
    }
}

/// Java: `GroupEditDao.hasSession`.
pub fn group_edit_session(world: &World, user_id: u32, group_id: u32) -> bool {
    let now = now();
    world
        .store
        .records_where::<GroupEditSession>(|s| {
            s.user_id == user_id && s.group_id == group_id && s.expire > now
        })
        .is_ok_and(|v| !v.is_empty())
}

/// The edit check every editing action starts with: the group being edited (`Some(group)`) or
/// the home (`None`); `Err` when the session may not edit (Java answers "").
fn edit_target(conn: &mut Conn) -> Result<Option<i64>, ()> {
    let user_id = conn.user_id();
    let (group, home) = {
        let s = conn.session();
        (
            s.contains("groupEditSession")
                .then(|| s.int("groupEditSession") as i64),
            s.contains("homeEditSession"),
        )
    };
    match group {
        Some(group_id) => {
            let world = conn.world();
            let exists = u32::try_from(group_id).ok().and_then(|id| {
                world
                    .store
                    .record::<rusbbo_storage::Group>(id)
                    .ok()
                    .flatten()
            });
            match exists {
                Some(g) if group_edit_session(world, user_id, g.id) => Ok(Some(group_id)),
                _ => Err(()),
            }
        }
        None if home => Ok(None),
        None => Err(()),
    }
}

fn find_widget(conn: &Conn, target: Option<i64>, id: i64) -> Option<Sticker> {
    match target {
        Some(group) => homes::group_widget(conn.world(), id, group),
        None => homes::home_widget(conn.world(), conn.user_id(), id),
    }
}

// ---- HomesController ----

/// `/home/*`, `/home/*/id`.
pub fn home(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.session().contains("authenticated") {
        return Ok(());
    }
    let world = world(conn);
    let mut tpl = conn.template("home");
    let viewer = if conn.is_authenticated() {
        viewer(conn)
    } else {
        None
    };
    let mut username = Some(conn.wildcard(0).to_string());
    if conn.route.ends_with("/id")
        && let Ok(id) = conn.wildcard(0).parse::<i32>()
    {
        username = u32::try_from(id)
            .ok()
            .and_then(|id| world.store.user(id).ok().flatten())
            .map(|u| u.username);
    }
    let Some(user) = username.and_then(|n| world.store.user_by_name(&n).ok().flatten()) else {
        not_found(conn);
        return Ok(());
    };
    if viewer.as_ref().is_none_or(|v| v.rank < 5)
        && (!user.profile_visible || data::active_ban(&world, &user).is_some())
    {
        not_found(conn);
        return Ok(());
    }
    let stored = homes::home(&world, user.id);
    let default_widgets = stored.is_none();
    let home = stored.unwrap_or(rusbbo_storage::Home {
        id: user.id,
        background: "bg_pattern_abstract2".into(),
    });
    let mut can_add_friend = false;
    let mut user_id: i64 = -1;
    let mut session_time: i64 = -1;
    if conn.is_authenticated() {
        user_id = conn.user_id() as i64;
        session_time = homes::edit_session(&world, user_id as u32).unwrap_or(-1);
        if session_time != -1 && user_id == user.id as i64 {
            let mut s = conn.session();
            s.delete("groupEditSession");
            s.set("homeEditSession", user.id as i64);
        }
        if user_id != user.id as i64
            && !world
                .store
                .messenger_friend_exists(user_id as u32, user.id)
                .unwrap_or(false)
        {
            can_add_friend = true;
        }
    }
    let widgets = if default_widgets {
        homes::default_widgets(&world, user.id)
    } else {
        homes::home_widgets(&world, user.id, Some(true))
    };
    let guestbook = homes::home_widgets(&world, user.id, None)
        .into_iter()
        .find(|w| homes::product_data(&world, w).eq_ignore_ascii_case("guestbookwidget"));
    conn.session().set("page", "me");
    tpl.set("user", player_details(&world, &user));
    tpl.set("tags", Value::list(data::user_tags(&world, user.id)));
    tpl.set("editMode", session_time != -1 && user_id == user.id as i64);
    tpl.set(
        "stickers",
        Value::list(
            widgets
                .iter()
                .map(|w| homes::widget(&world, w, 1))
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set("homeBannerAd", homes::random_ad());
    tpl.set("home", homes::home_value(&home));
    tpl.set("canAddFriend", can_add_friend);
    tpl.set(
        "guestbookSetting",
        guestbook.as_ref().map_or("public", homes::guestbook_state),
    );
    tpl.set("stickerLimit", homes::sticker_limit(has_club(&user)));
    tpl.set("tagCloud", Value::list(Vec::<Value>::new()));
    profile_badge(&world, &user, &mut tpl, true);
    if conn.is_authenticated() && user.id as i64 == user_id {
        data::set_statistic(&world, user.id, "guestbook_unread_messages", 0);
    }
    conn.render(tpl)
}

/// The type a request names (`stickers`, `backgrounds`, `notes`).
fn type_id(kind: &str) -> i32 {
    match kind.to_lowercase().as_str() {
        "stickers" => STICKER,
        "backgrounds" => BACKGROUND,
        "notes" => NOTE,
        _ => 0,
    }
}

fn stacked(world: &Arc<World>, list: &[Sticker]) -> Value {
    Value::list(
        homes::stack(list)
            .iter()
            .map(|(w, n)| homes::widget(world, w, *n))
            .collect::<Vec<_>>(),
    )
}

fn categories_of(tpl: &mut Tpl, world: &World, rank: u8) -> Vec<rusbbo_storage::StickerCategory> {
    let (stickers, backgrounds) = homes::categories(world, rank);
    let list = |v: &[rusbbo_storage::StickerCategory]| {
        Value::list(v.iter().map(homes::category_value).collect::<Vec<_>>())
    };
    tpl.set("stickerCategories", list(&stickers));
    tpl.set("backgroundCategories", list(&backgrounds));
    stickers.into_iter().chain(backgrounds).collect()
}

fn css_or_null(p: &rusbbo_storage::StickerProduct) -> String {
    homes::css_class(p).unwrap_or_else(|| "null".into())
}

/// `/myhabbo/store/inventory`.
pub fn inventory(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let mut tpl = conn.template("homes/inventory/inventory");
    let Some(user) = logged_out(conn) else {
        return Ok(());
    };
    let world = world(conn);
    categories_of(&mut tpl, &world, user.rank);
    let list = homes::inventory_widgets(&world, user.id, Some(STICKER));
    let json = match list
        .first()
        .and_then(|w| homes::product(&world, w.sticker_id))
    {
        Some(p) => format!(
            "[[\"Inventory\",\"Web Store\"],[\"{}\",\"{}\",\"{}\",\"Stickers\",null,1]]",
            css_or_null(&p),
            p.data,
            p.name
        ),
        None => "[[\"Inventory\",\"Web Store\"],[\"\",\"\",\"\",\"Stickers\",null,1]]".into(),
    };
    x_json(conn, json);
    tpl.set("emptyBoxes", nulls(homes::empty_boxes(list.len(), 4.0)));
    tpl.set("widgets", stacked(&world, &list));
    conn.render(tpl)
}

/// `/myhabbo/store/inventory_items`.
pub fn inventory_items(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let kind = conn.post.string("type");
    if !kind.eq_ignore_ascii_case("widgets") {
        let list = homes::inventory_widgets(&world, conn.user_id(), Some(type_id(&kind)));
        let mut tpl = conn.template("homes/inventory/inventory_items");
        tpl.set("emptyBoxes", nulls(homes::empty_boxes(list.len(), 4.0)));
        tpl.set("widgets", stacked(&world, &list));
        tpl.set("widgetMode", false);
        return conn.render(tpl);
    }
    let group = {
        let s = conn.session();
        s.contains("groupEditSession")
            .then(|| s.int("groupEditSession"))
    };
    let list: Vec<Sticker> = match group {
        Some(g) => homes::group_widgets(&world, g.max(0) as u32, None)
            .into_iter()
            .filter(|w| homes::product_kind(&world, w) == GROUP_WIDGET)
            .collect(),
        None => homes::home_widgets(&world, conn.user_id(), None)
            .into_iter()
            .filter(|w| homes::product_kind(&world, w) == HOME_WIDGET)
            .filter(|w| !homes::product_data(&world, w).eq_ignore_ascii_case("profilewidget"))
            .collect(),
    };
    let mut tpl = conn.template("homes/inventory/inventory_items");
    tpl.set("widgetMode", true);
    tpl.set(
        "widgets",
        Value::list(
            list.iter()
                .map(|w| homes::widget(&world, w, 1))
                .collect::<Vec<_>>(),
        ),
    );
    conn.render(tpl)
}

/// `/myhabbo/store/inventory_preview`.
pub fn inventory_preview(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let item_id = int_param(conn.post.int("itemId"))?;
    let kind = conn.post.string("type");
    let group = {
        let s = conn.session();
        s.contains("groupEditSession")
            .then(|| s.int("groupEditSession"))
    };
    let mut type_id = type_id(&kind);
    if kind.eq_ignore_ascii_case("widgets") {
        type_id = if group.is_some() {
            GROUP_WIDGET
        } else {
            HOME_WIDGET
        };
    }
    let widget = match type_id {
        GROUP_WIDGET => homes::group_widgets(&world, group.unwrap_or(0).max(0) as u32, None),
        HOME_WIDGET => homes::home_widgets(&world, conn.user_id(), None),
        t => homes::inventory_widgets(&world, conn.user_id(), Some(t)),
    }
    .into_iter()
    .find(|w| w.id as i64 == item_id);
    let product = widget
        .as_ref()
        .and_then(|w| homes::product(&world, w.sticker_id));
    let json = match (&product, type_id) {
        (Some(p), STICKER) => format!(
            "[\"{}\",\"{}\",\"{}\",\"Sticker\",null,1]",
            css_or_null(p),
            p.data,
            p.name
        ),
        (Some(p), BACKGROUND) => format!(
            "[\"{}\",\"b_{}\",\"{}\",\"Background\",null,1]",
            css_or_null(p),
            p.data,
            p.name
        ),
        (Some(_), NOTE) => {
            "[\"commodity_stickienote_pre\",null,\"Notes\",\"WebCommodity\",null,1]".into()
        }
        (Some(p), GROUP_WIDGET | HOME_WIDGET) => {
            format!("[\"{}\",null,\"\",\"Widget\",\"true\",1]", css_or_null(p))
        }
        _ => "[\"\",\"\",\"\",\"Sticker\",null,1]".into(),
    };
    x_json(conn, json);
    let tpl = conn.template("homes/inventory/inventory_preview");
    conn.render(tpl)
}

/// The id of `/myhabbo/startSession/*` and `/myhabbo/cancel/*` when it is the user's own.
fn own_target(conn: &mut Conn) -> Option<u32> {
    let matched = conn.wildcard(0).to_string();
    if !is_numeric(&matched) {
        conn.redirect("/me");
        return None;
    }
    match matched.parse::<i32>() {
        Ok(id) if id as i64 == conn.user_id() as i64 => Some(id as u32),
        Ok(_) => {
            conn.redirect("/me");
            None
        }
        Err(_) => None,
    }
}

fn redirect_home(conn: &mut Conn) {
    let name = viewer(conn).map(|u| u.username).unwrap_or_default();
    conn.redirect(&format!("/home/{name}"));
}

/// `/myhabbo/startSession/*`.
pub fn start_editing_session(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let user_id = conn.user_id();
    let Some(target) = own_target(conn) else {
        if conn.response.is_none() {
            return Err(WebError("NumberFormatException".into()));
        }
        return Ok(());
    };
    let world = world(conn);
    if homes::home(&world, target).is_none() {
        homes::create_home(&world, target)?;
    }
    if homes::edit_session(&world, user_id).is_none() {
        homes::create_edit_session(&world, user_id)?;
        let mut s = conn.session();
        s.set("homeEditSession", user_id as i64);
        s.delete("groupEditSession");
    }
    redirect_home(conn);
    Ok(())
}

/// `/myhabbo/cancel/*`.
pub fn cancel_editing_session(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let user_id = conn.user_id();
    if own_target(conn).is_none() {
        if conn.response.is_none() {
            return Err(WebError("NumberFormatException".into()));
        }
        return Ok(());
    }
    let world = world(conn);
    if homes::edit_session(&world, user_id).is_some() {
        homes::delete_edit_session(&world, user_id)?;
        let mut s = conn.session();
        s.delete("homeEditSession");
        s.delete("groupEditSession");
    }
    redirect_home(conn);
    Ok(())
}

/// One "id:x,y,z" entry of a save.
pub(super) fn parse_position(entry: &str) -> Option<(i64, i64, i64, i64)> {
    let id: i32 = entry.split(':').next()?.parse().ok()?;
    let coords = entry.replace(&format!("{id}:"), "");
    let mut parts = coords.split(',');
    let x: i32 = parts.next()?.parse().ok()?;
    let y: i32 = parts.next()?.parse().ok()?;
    let z: i32 = parts.next()?.parse().ok()?;
    Some((id as i64, x as i64, y as i64, z as i64))
}

/// `/myhabbo/save`.
pub fn save(conn: &mut Conn) -> WebResult {
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
    if homes::edit_session(&world, user.id).is_none() {
        conn.send_html("");
        return Ok(());
    }
    let home = homes::home(&world, user.id);
    let mut placed = homes::home_widgets(&world, user.id, Some(true));
    // Java runs all of this in one try block: the first bad value ends it.
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
            let widget = homes::inventory_widgets(&world, user.id, None)
                .into_iter()
                .find(|w| w.id as i64 == background as i64);
            if let Some(w) = widget {
                let mut home = home.clone().ok_or(false)?;
                home.background = homes::product_data(&world, &w);
                // HomesDao.saveBackground: an UPDATE of the row.
                world.store.put_record(&home).map_err(|_| false)?;
            }
        }
        for (i, key) in ["stickers", "widgets", "stickienotes"].iter().enumerate() {
            if !conn.post.contains(key) {
                continue;
            }
            let value = conn.post.string(key);
            let entries: Vec<&str> = java_split(&value, '/');
            if i == 0 && entries.len() as i64 >= homes::sticker_limit(has_club(&user)) {
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
    let site_path = data::cfg_string(&world, "site.path");
    conn.send_html(format!(
        "<script language=\"JavaScript\" type=\"text/javascript\">\nwaitAndGo('{site_path}/home/{}');\n</script>\n",
        user.username
    ));
    homes::delete_edit_session(&world, user.id)?;
    conn.session().delete("homeEditSession");
    Ok(())
}

/// Java's `String.split`: trailing empty strings dropped.
pub(super) fn java_split(s: &str, sep: char) -> Vec<&str> {
    let mut v: Vec<&str> = s.split(sep).collect();
    while v.len() > 1 && v.last() == Some(&"") {
        v.pop();
    }
    if v == [""] && !s.is_empty() {
        v.clear();
    }
    v
}

/// `/myhabbo/tag/list`.
pub fn tag_list(conn: &mut Conn) -> WebResult {
    let user_id = conn.session().int("user.id");
    if user_id < 1 {
        conn.send_html("");
        return Ok(());
    }
    let account = int_param(conn.post.int("accountId"))?;
    let mut tpl = conn.template("homes/widget/habblet/taglist");
    let world = world(conn);
    let Some(user) = (if conn.is_authenticated() {
        viewer(conn)
    } else {
        None
    }) else {
        conn.send_html("");
        return Ok(());
    };
    let tags = u32::try_from(account)
        .map(|id| data::user_tags(&world, id))
        .unwrap_or_default();
    tpl.set("tags", Value::list(tags));
    tpl.set("user", player_details(&world, &user));
    conn.render(tpl)
}

// ---- StoreController ----

fn products_of(world: &World, category: u32) -> Vec<rusbbo_storage::StickerProduct> {
    homes::catalogue(world)
        .into_iter()
        .filter(|p| p.category_id == category)
        .collect()
}

fn product_list(v: &[rusbbo_storage::StickerProduct]) -> Value {
    Value::list(v.iter().map(homes::product_value).collect::<Vec<_>>())
}

/// `/myhabbo/store/main`.
pub fn store_main(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let mut tpl = conn.template("homes/store/main");
    let Some(user) = logged_out(conn) else {
        return Ok(());
    };
    let world = world(conn);
    let (stickers, backgrounds) = homes::categories(&world, user.rank);
    categories_of(&mut tpl, &world, user.rank);
    let category = stickers.first().or(backgrounds.first()).map(|c| c.id);
    let products = category.map(|c| products_of(&world, c)).unwrap_or_default();
    let empty = homes::empty_boxes(products.len(), 5.0);
    let product = products.first();
    let json = match product {
        Some(p) => format!(
            "[[\"Inventory\",\"Web Store\"],[{{\"itemCount\":{},\"previewCssClass\":\"{}\",\"titleKey\":\"\"}}]]",
            p.amount,
            css_or_null(p)
        ),
        None => "[[\"Inventory\",\"Web Store\"],[{\"itemCount\":0,\"titleKey\":\"\"}]]".into(),
    };
    x_json(conn, json);
    tpl.set("products", product_list(&products));
    tpl.set("product", product.map(homes::product_value));
    tpl.set("emptyBoxes", nulls(empty));
    conn.render(tpl)
}

/// `/myhabbo/store/items`.
pub fn store_items(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let mut tpl = conn.template("homes/store/items");
    if logged_out(conn).is_none() {
        return Ok(());
    }
    let world = world(conn);
    let sub = int_param(conn.post.int("subCategoryId"))?;
    let category = u32::try_from(sub).ok().and_then(|id| {
        world
            .store
            .record::<rusbbo_storage::StickerCategory>(id)
            .ok()
            .flatten()
    });
    let Some(category) = category else {
        not_found(conn);
        return Ok(());
    };
    let products = products_of(&world, category.id);
    tpl.set("emptyProducts", homes::empty_boxes(products.len(), 5.0));
    tpl.set("products", product_list(&products));
    conn.render(tpl)
}

fn catalogue_product(
    conn: &Conn,
    key: &str,
) -> Result<Option<rusbbo_storage::StickerProduct>, WebError> {
    let id = int_param(conn.post.int(key))?;
    Ok(u32::try_from(id)
        .ok()
        .and_then(|id| homes::product(conn.world(), id)))
}

/// `/myhabbo/store/preview`.
pub fn store_preview(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Some(p) = catalogue_product(conn, "productId")? else {
        not_found(conn);
        return Ok(());
    };
    match homes::kind(&p) {
        STICKER | NOTE => x_json(
            conn,
            format!(
                "[{{\"itemCount\":{},\"previewCssClass\":\"{}\",\"titleKey\":\"{}\"}}]",
                p.amount,
                css_or_null(&p),
                p.name
            ),
        ),
        BACKGROUND => x_json(
            conn,
            format!(
                "[{{\"bgCssClass\":\"b_{}\",\"itemCount\":{},\"previewCssClass\":\"{}\",\"titleKey\":\"{}\"}}]",
                p.data,
                p.amount,
                css_or_null(&p),
                p.name
            ),
        ),
        _ => {}
    }
    let mut tpl = conn.template("homes/store/preview");
    tpl.set("product", homes::product_value(&p));
    conn.render(tpl)
}

/// `/myhabbo/store/purchase_confirm`.
pub fn purchase_confirm(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Some(p) = catalogue_product(conn, "productId")? else {
        return Ok(());
    };
    let mut tpl = conn.template("homes/store/purchase_confirm");
    let credits = viewer(conn).map(|u| u.credits).unwrap_or(0);
    tpl.set("product", homes::product_value(&p));
    tpl.set("noCredits", credits < p.price);
    conn.render(tpl)
}

/// `/myhabbo/store/background_warning`.
pub fn background_warning(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let tpl = conn.template("homes/store/background_warning");
    conn.render(tpl)
}

/// `/myhabbo/store/purchase_stickers`, `purchase_backgrounds`, `purchase_stickie_notes`.
pub fn purchase(conn: &mut Conn, kind: i32) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Some(p) = catalogue_product(conn, "selectedId")? else {
        not_found(conn);
        return Ok(());
    };
    if homes::kind(&p) != kind {
        not_found(conn);
        return Ok(());
    }
    let world = world(conn);
    let Some(mut user) = viewer(conn) else {
        return Err(WebError("no user".into()));
    };
    if user.credits < p.price {
        conn.send_html("");
        return Ok(());
    }
    let mut w = world.store.write()?;
    for _ in 0..p.amount {
        homes::purchase_widget(&mut w, user.id, (0, 0, 0), 0, p.id, "", 0, false)?;
    }
    w.commit()?;
    crate::game::currency::decrease_credits(&world.store, &mut user, p.price);
    crate::rcon::send(
        &world,
        "refresh_credits",
        &[("userId", user.id.to_string())],
    );
    conn.send_html("OK");
    Ok(())
}

// ---- NoteEditorController ----

/// `/myhabbo/noteeditor/editor`.
pub fn note_editor(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let note = java_truncate(&conn.post.string("noteText"), 500);
    let skin = int_param(conn.post.int("skin"))?;
    let mut tpl = conn.template("homes/editor/noteeditor");
    if skin > 0 && skin < 9 {
        tpl.set(&format!("skin{skin}Selected"), " selected");
    }
    tpl.set("noteText", note);
    conn.render(tpl)
}

/// `/myhabbo/noteeditor/preview`.
pub fn note_preview(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let note = text::bbcode(
        &world,
        &text::html_escape(&text::bbcode_normalise(&conn.post.string("noteText"))),
        false,
    );
    let skin = int_param(conn.post.int("skin"))?;
    let mut tpl = conn.template("homes/editor/preview");
    tpl.set("skin", homes::skin(skin as i32));
    tpl.set("noteText", java_truncate(&note, 500));
    conn.render(tpl)
}

fn pair(key: &str, value: String) -> Value {
    Map::new()
        .with("key", key)
        .with("value", value.clone())
        .with("left", key)
        .with("right", value)
        .into()
}

/// `/myhabbo/linktool/search`.
pub fn link_search(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let query = conn.get.string("query");
    let scope = int_param(conn.get.int("scope"))?;
    let mut found: Vec<Value> = Vec::new();
    let kind = match scope {
        1 => {
            let mut users: Vec<User> = world
                .store
                .messenger_search(&query)?
                .into_iter()
                .filter_map(|id| world.store.user(id).ok().flatten())
                .collect();
            users.sort_by(|a, b| a.username.cmp(&b.username));
            for u in users.iter().take(10) {
                found.push(pair(&u.username, u.id.to_string()));
            }
            "habbo"
        }
        2 => {
            let rooms = if query.trim().is_empty() {
                Vec::new()
            } else {
                world.store.search_rooms(&query, -1, 30)?
            };
            for r in rooms.iter().take(10) {
                found.push(pair(&r.name, r.id.to_string()));
            }
            "room"
        }
        _ => {
            for g in data::query_search_groups(&world, &query).iter().take(10) {
                found.push(pair(&g.name, g.id.to_string()));
            }
            "group"
        }
    };
    let mut tpl = conn.template("homes/editor/search");
    tpl.set("querySearch", Value::list(found));
    tpl.set("type", kind);
    conn.render(tpl)
}

/// `/myhabbo/noteeditor/place`.
pub fn note_place(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let user_id = conn.user_id();
    let skin = int_param(conn.post.int("skin"))?;
    let note = java_truncate(&conn.post.string("noteText"), 500);
    let Ok(target) = edit_target(conn) else {
        conn.send_html("");
        return Ok(());
    };
    let world = world(conn);
    let Some(mut w) = homes::inventory_widgets(&world, user_id, Some(NOTE))
        .into_iter()
        .next()
    else {
        return Err(WebError("IndexOutOfBoundsException".into()));
    };
    w.x = "20".into();
    w.y = "30".into();
    w.z = "1".into();
    if let Some(group) = target {
        w.group_id = group as i32;
    }
    w.text = note;
    w.skin_id = skin as i32;
    w.is_placed = true;
    homes::save(&world, &w)?;
    x_json(conn, w.id.to_string());
    let tpl = widget_template(conn, &w);
    conn.render(tpl)
}

/// The skin a user may give a note or widget (Java: club skins for club members, 9 for staff).
fn allowed_skin(conn: &Conn, skin: i64) -> i32 {
    let user = viewer(conn);
    let club = user.as_ref().is_some_and(has_club);
    let rank = user.as_ref().map_or(0, |u| u.rank);
    if (skin == 7 || skin == 8) && !club {
        return 1;
    }
    if skin == 9 && rank < 5 {
        return 1;
    }
    skin as i32
}

/// `/myhabbo/stickie/edit` and `/myhabbo/widget/edit`.
fn edit_skin(conn: &mut Conn, id_key: &str, skin_key: &str, stickie: bool) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Ok(target) = edit_target(conn) else {
        conn.send_html("");
        return Ok(());
    };
    let id = int_param(conn.post.int(id_key))?;
    let skin = int_param(conn.post.int(skin_key))?;
    let Some(mut w) = find_widget(conn, target, id) else {
        conn.send_html("");
        return Ok(());
    };
    let mut tpl = widget_template(conn, &w);
    w.skin_id = allowed_skin(conn, skin);
    let world = world(conn);
    homes::save(&world, &w)?;
    let product = homes::product(&world, w.sticker_id);
    if stickie {
        x_json(
            conn,
            format!(
                "{{\"id\":\"{}\",\"cssClass\":\"n_skin_{}\",\"type\":\"stickie\"}}",
                w.id,
                homes::skin(w.skin_id)
            ),
        );
    } else if product
        .as_ref()
        .is_some_and(|p| p.widget_type == -1 || p.widget_type == 1)
    {
        x_json(
            conn,
            format!(
                "{{\"id\":\"{}\",\"cssClass\":\"w_skin_{}\",\"type\":\"widget\"}}",
                w.id,
                homes::skin(w.skin_id)
            ),
        );
    }
    tpl.set("sticker", homes::widget(&world, &w, 1));
    conn.render(tpl)
}

pub fn stickie_edit(conn: &mut Conn) -> WebResult {
    edit_skin(conn, "stickieId", "skinId", true)
}

pub fn widget_edit(conn: &mut Conn) -> WebResult {
    edit_skin(conn, "widgetId", "skinId", false)
}

/// `/myhabbo/stickie/delete`.
pub fn stickie_delete(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let user_id = conn.user_id();
    let Ok(target) = edit_target(conn) else {
        conn.send_html("");
        return Ok(());
    };
    let id = int_param(conn.post.int("stickieId"))?;
    let world = world(conn);
    let doomed = homes::sticker(&world, id).filter(|s| match target {
        Some(group) => s.group_id as i64 == group,
        None => s.group_id == 0 && s.is_placed && s.user_id == user_id,
    });
    if let Some(s) = doomed {
        world.store.delete_record::<Sticker>(s.id)?;
    }
    conn.send_html("SUCCESS");
    Ok(())
}

// ---- WidgetController ----

fn z_index(conn: &Conn) -> Result<i64, WebError> {
    let z = int_param(conn.post.int("zindex"))?;
    Ok(if !(0..=100).contains(&z) { 0 } else { z })
}

/// `/myhabbo/sticker/place_sticker`.
pub fn place_sticker(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Ok(target) = edit_target(conn) else {
        conn.send_html("");
        return Ok(());
    };
    let id = int_param(conn.post.int("selectedStickerId"))?;
    let z = z_index(conn)?;
    let world = world(conn);
    let Some(mut w) = homes::inventory_widget(&world, conn.user_id(), id) else {
        return Err(WebError("NullPointerException".into()));
    };
    w.x = "20".into();
    w.y = "30".into();
    w.z = z.to_string();
    if let Some(group) = target {
        w.group_id = group as i32;
    }
    w.is_placed = true;
    homes::save(&world, &w)?;
    x_json(conn, format!("[\"{}\"]", w.id));
    let tpl = widget_template(conn, &w);
    conn.render(tpl)
}

/// `/myhabbo/widget/add`.
pub fn place_widget(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Ok(target) = edit_target(conn) else {
        conn.send_html("");
        return Ok(());
    };
    let id = int_param(conn.post.int("widgetId"))?;
    let z = z_index(conn)?;
    let world = world(conn);
    let Some(mut w) = find_widget(conn, target, id) else {
        return Err(WebError("NullPointerException".into()));
    };
    w.x = "10".into();
    w.y = "10".into();
    w.z = z.to_string();
    if let Some(group) = target {
        w.group_id = group as i32;
    }
    w.is_placed = true;
    homes::save(&world, &w)?;
    x_json(conn, format!("[\"{}\"]", w.id));
    let mut tpl = widget_template(conn, &w);
    if target.is_some() {
        let group = world
            .store
            .record::<rusbbo_storage::Group>(w.group_id.max(0) as u32)
            .ok()
            .flatten()
            .map(|g| model::group(&world, &g));
        tpl.set("group", group);
    }
    conn.render(tpl)
}

/// `/myhabbo/sticker/remove_sticker` and `/myhabbo/widget/delete`.
fn remove(conn: &mut Conn, key: &str, widget: bool) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let Ok(target) = edit_target(conn) else {
        conn.send_html("");
        return Ok(());
    };
    let id = int_param(conn.post.int(key))?;
    let world = world(conn);
    let Some(mut w) = find_widget(conn, target, id) else {
        conn.send_html("");
        return Ok(());
    };
    if widget {
        let data = homes::product_data(&world, &w).to_lowercase();
        if data == "groupinfowidget" || data == "profilewidget" {
            conn.send_html("");
            return Ok(());
        }
    }
    w.x = "0".into();
    w.y = "0".into();
    w.z = "0".into();
    match (widget, target) {
        (false, _) => w.group_id = 0,
        (true, Some(group)) => w.group_id = group as i32,
        (true, None) => {}
    }
    w.is_placed = false;
    homes::save(&world, &w)?;
    conn.send_html("SUCCESS");
    Ok(())
}

pub fn remove_sticker(conn: &mut Conn) -> WebResult {
    remove(conn, "stickerId", false)
}

pub fn remove_widget(conn: &mut Conn) -> WebResult {
    remove(conn, "widgetId", true)
}

// ---- GuestbookController ----

/// `/myhabbo/guestbook/preview`.
pub fn guestbook_preview(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let message = text::bbcode(
        &world,
        &text::html_escape(&text::bbcode_normalise(&conn.post.string("message"))),
        false,
    );
    let mut tpl = conn.template("homes/widget/guestbook/preview");
    let author = viewer(conn).map(|u| player_details(&world, &u));
    tpl.set("message", java_truncate(&message, 200));
    tpl.set("author", author);
    tpl.set("formattedDate", jdate::friendly(now()));
    conn.render(tpl)
}

/// A placed guestbook widget by id.
fn guestbook_widget(world: &World, id: i64) -> Option<Sticker> {
    homes::sticker(world, id)
        .filter(|w| homes::product_data(world, w).to_lowercase() == "guestbookwidget")
}

/// `/myhabbo/guestbook/add`.
pub fn guestbook_add(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let id = int_param(conn.post.int("widgetId"))?;
    let Some(w) = guestbook_widget(&world, id) else {
        conn.send_html("");
        return Ok(());
    };
    let message = java_truncate(&conn.post.string("message"), 200);
    let user_id = conn.user_id();
    if !w.is_placed || !homes::posting_allowed(&world, &w, user_id as i64) {
        conn.send_html("");
        return Ok(());
    }
    let (mut home_id, mut group_id) = (0, 0);
    match homes::product_kind(&world, &w) {
        GROUP_WIDGET => group_id = w.group_id.max(0) as u32,
        HOME_WIDGET => {
            home_id = w.user_id;
            if home_id != user_id {
                let unread = data::statistic(&world, home_id, "guestbook_unread_messages");
                data::set_statistic(&world, home_id, "guestbook_unread_messages", unread + 1);
            }
        }
        _ => {}
    }
    let mut entry = GuestbookEntry {
        user_id,
        home_id,
        group_id,
        message: message.clone(),
        created_at: now(),
        ..Default::default()
    };
    let mut value = homes::guestbook_entry(&world, &entry);
    if text::filter_sentence(&world, &message) == message {
        world.store.insert_record(&mut entry)?;
        // GuestbookDao.create reads the generated key as "id", which MariaDB does not name so:
        // the entry it returns has id -1 and creation time -1.
        if let Value::Map(m) = &value {
            let mut m = (**m).clone();
            m.insert("id", -1);
            m.insert("creationDate", jdate::friendly(-1));
            value = m.into();
        }
    } else {
        use rand::RngExt;
        entry.id = rand::rng().random_range(0..i32::MAX as u32);
        value = homes::guestbook_entry(&world, &entry);
    }
    let mut tpl = conn.template("homes/widget/guestbook/add");
    tpl.set("entry", value);
    tpl.set("sticker", homes::widget(&world, &w, 1));
    tpl.set(
        "canDeleteEntries",
        homes::can_delete_entries(&world, &w, user_id as i64),
    );
    conn.render(tpl)
}

/// `/myhabbo/guestbook/remove`.
pub fn guestbook_remove(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let (entry_id, widget_id) = match (conn.post.int("entryId"), conn.post.int("widgetId")) {
        (Some(e), Some(w)) => (e as i64, w as i64),
        (Some(e), None) => (e as i64, -1),
        _ => (-1, -1),
    };
    let user_id = conn.user_id();
    let Some(w) = guestbook_widget(&world, widget_id).filter(|w| w.is_placed) else {
        conn.send_html("");
        return Ok(());
    };
    let entry = u32::try_from(entry_id)
        .ok()
        .and_then(|id| world.store.record::<GuestbookEntry>(id).ok().flatten());
    let Some(entry) = entry
        .filter(|e| homes::can_delete_entries(&world, &w, user_id as i64) || e.user_id == user_id)
    else {
        conn.send_html("");
        return Ok(());
    };
    let (home_id, group_id) = match homes::product_kind(&world, &w) {
        GROUP_WIDGET => (0, w.group_id.max(0) as u32),
        HOME_WIDGET => (w.user_id, 0),
        _ => (0, 0),
    };
    if entry.home_id == home_id && entry.group_id == group_id {
        world.store.delete_record::<GuestbookEntry>(entry.id)?;
    }
    let mut tpl = conn.template("homes/widget/guestbook_widget");
    let edit = {
        let s = conn.session();
        s.contains("homeEditSession") || s.contains("groupEditSession")
    };
    tpl.set("editMode", edit);
    tpl.set("sticker", homes::widget(&world, &w, 1));
    conn.render(tpl)
}

const GUESTBOOK_TOGGLE: &str = "var el = $(\"guestbook-type\");\nif (el) {\n\tif (el.hasClassName(\"public\")) {\n\t\tel.className = \"private\";\n\t\tnew Effect.Pulsate(el,\n\t\t\t{ duration: 1.0, afterFinish : function() { Element.setOpacity(el, 1); } }\n\t\t);\t\t\t\t\t\t\n\t} else {\t\t\t\t\t\t\n\t\tnew Effect.Pulsate(el,\n\t\t\t{ duration: 1.0, afterFinish : function() { Element.setOpacity(el, 0); el.className = \"public\"; } }\n\t\t);\t\t\t\t\t\t\n\t}\n}";

/// `/myhabbo/guestbook/configure`.
pub fn guestbook_configure(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let user_id = conn.user_id();
    let id = int_param(conn.post.int("widgetId"))?;
    let Some(mut w) = guestbook_widget(&world, id).filter(|w| w.is_placed) else {
        conn.send_html("");
        return Ok(());
    };
    let owner = match homes::product_kind(&world, &w) {
        GROUP_WIDGET => homes::group_owner(&world, w.group_id.max(0) as u32),
        HOME_WIDGET => w.user_id,
        _ => 0,
    };
    if owner != user_id {
        conn.send_html("");
        return Ok(());
    }
    w.extra_data = if homes::guestbook_state(&w) == "private" {
        "public".into()
    } else {
        "private".into()
    };
    homes::save(&world, &w)?;
    conn.send(Resp::typed(
        200,
        "text/javascript",
        GUESTBOOK_TOGGLE.as_bytes().to_vec(),
    ));
    Ok(())
}

// ---- RateController ----

fn rate_template(conn: &mut Conn, w: &Sticker) -> WebResult {
    let world = world(conn);
    let mut tpl = conn.template("homes/widget/habblet/rate");
    tpl.set("sticker", homes::widget(&world, w, 1));
    conn.render(tpl)
}

/// `/myhabbo/rating/rate`.
pub fn rate(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let user_id = conn.user_id();
    let (widget_id, rating) = match (conn.get.int("ratingId"), conn.get.int("givenRate")) {
        (Some(w), Some(r)) => (w as i64, r as i64),
        (Some(w), None) => (w as i64, -1),
        _ => (-1, -1),
    };
    if !(1..=5).contains(&rating) {
        conn.send_html("");
        return Ok(());
    }
    let world = world(conn);
    let Some(w) = homes::sticker(&world, widget_id) else {
        conn.send_html("");
        return Ok(());
    };
    let home_id = w.user_id;
    if home_id == user_id || homes::has_rated(&world, user_id as i64, home_id) {
        conn.send_html("");
        return Ok(());
    }
    homes::rate(&world, user_id, home_id, rating as i32)?;
    rate_template(conn, &w)
}

/// `/myhabbo/rating/reset_ratings`.
pub fn reset_rating(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let user_id = conn.user_id();
    let widget_id = int_param(conn.get.int("ratingId"))?;
    let world = world(conn);
    let Some(w) = homes::sticker(&world, widget_id) else {
        conn.send_html("");
        return Ok(());
    };
    if w.user_id != user_id {
        conn.send_html("");
        return Ok(());
    }
    homes::delete_ratings(&world, w.user_id)?;
    rate_template(conn, &w)
}

// ---- BadgesController ----

/// `/myhabbo/badgelist/badgepaging`.
pub fn badge_paging(conn: &mut Conn) -> WebResult {
    let widget_id = int_param(conn.post.int("widgetId"))?;
    let mut page = int_param(conn.post.int("pageNumber"))?;
    let world = world(conn);
    let Some(w) = homes::sticker(&world, widget_id) else {
        conn.send_html("");
        return Ok(());
    };
    let pages = homes::badge_pages(&world, w.user_id);
    let size = pages.len() as i64;
    let mut list: Value = pages.first().map_or(Value::Null, |p| homes::badge_list(p));
    let mut show_last = true;
    if page > size {
        page = size;
    }
    if page >= size {
        show_last = false;
    }
    if page <= 0 {
        page = 1;
    }
    if let Some(p) = pages.get((page - 1) as usize) {
        list = homes::badge_list(p);
    }
    let mut tpl = conn.template("homes/widget/habblet/badgepaging");
    tpl.set("sticker", homes::widget(&world, &w, 1));
    tpl.set("pages", size);
    tpl.set("showLast", show_last);
    tpl.set("badgeList", list);
    tpl.set("currentPage", page);
    conn.render(tpl)
}

// ---- TraxController ----

/// `/myhabbo/traxplayer/select_song`.
pub fn select_song(conn: &mut Conn) -> WebResult {
    if !authenticated(conn) {
        return Ok(());
    }
    let world = world(conn);
    let Some(user) = viewer(conn) else {
        return Err(WebError("NullPointerException".into()));
    };
    let widget_id = int_or(conn.post.int("widgetId"), -1);
    let song_id = int_or(conn.post.int("songId"), -1);
    let Some(mut w) = homes::sticker(&world, widget_id)
        .filter(|w| homes::product_data(&world, w).to_lowercase() == "traxplayerwidget")
    else {
        conn.send_html("");
        return Ok(());
    };
    let can_select = match homes::product_kind(&world, &w) {
        GROUP_WIDGET => homes::group_owner(&world, w.group_id.max(0) as u32) == user.id,
        HOME_WIDGET => w.user_id == user.id,
        _ => false,
    };
    if !can_select {
        conn.send_html("");
        return Ok(());
    }
    let list = homes::songs(&world, &w).unwrap_or_default();
    let song = i32::try_from(song_id)
        .ok()
        .and_then(|id| crate::game::song::song(&world.store, id));
    w.extra_data = match song {
        Some(s) if song_id != 0 && list.iter().any(|x| x.id == s.id) => s.id.to_string(),
        _ => String::new(),
    };
    homes::save(&world, &w)?;
    let mut tpl = conn.template("homes/widget/habblet/trax_song");
    tpl.set("sticker", homes::widget(&world, &w, 1));
    conn.render(tpl)
}

/// `/trax/song/*`.
pub fn get_song(conn: &mut Conn) -> WebResult {
    let id = conn.wildcard(0).to_string();
    if !is_numeric(&id) {
        conn.send_html("");
        return Ok(());
    }
    let world = world(conn);
    let song = id
        .parse::<i32>()
        .ok()
        .and_then(|id| crate::game::song::song(&world.store, id));
    let Some(song) = song.filter(|s| !s.data.is_empty()) else {
        conn.send_html("");
        return Ok(());
    };
    let units: Vec<u16> = song.data.encode_utf16().collect();
    let data = String::from_utf16_lossy(&units[..units.len() - 1]);
    let tracks = data
        .replace(":4:", "&track4=")
        .replace(":3:", "&track3=")
        .replace(":2:", "&track2=")
        .replace("1:", "&track1=");
    let author = u32::try_from(song.user_id)
        .ok()
        .and_then(|id| world.store.user(id).ok().flatten())
        .map_or_else(|| "null".to_string(), |u| u.username);
    conn.send_html(format!(
        "status=0&name={}&author={author}{tracks}",
        song.title
    ));
    Ok(())
}

// ---- FriendsWidgetController, MemberWidgetController ----

fn paging_params(conn: &Conn) -> Result<(i64, i64, String), WebError> {
    let widget_id = int_param(conn.post.int("widgetId"))?;
    let page = int_or(conn.post.int("pageNumber"), 1).max(1);
    Ok((widget_id, page, conn.post.string("searchString")))
}

/// `/myhabbo/avatarlist/friendsearchpaging`.
pub fn friend_search_paging(conn: &mut Conn) -> WebResult {
    let (widget_id, page, search) = paging_params(conn)?;
    let world = world(conn);
    let Some(w) = homes::sticker(&world, widget_id) else {
        conn.send_html("");
        return Ok(());
    };
    let pages = homes::friends_pages(&world, w.user_id, &search);
    let list = homes::friends_page(&world, w.user_id, &search, page);
    let mut tpl = conn.template("homes/widget/habblet/friendsearchpaging");
    tpl.set("sticker", homes::widget(&world, &w, 1));
    tpl.set("pages", pages);
    tpl.set("friends", homes::friends_amount(&world, w.user_id));
    tpl.set("friendsList", Value::list(list));
    tpl.set("currentPage", page);
    conn.render(tpl)
}

/// `/myhabbo/avatarlist/avatarinfo`.
pub fn avatar_info(conn: &mut Conn) -> WebResult {
    let id = int_param(conn.post.int("anAccountId"))?;
    let world = world(conn);
    let Some(user) = u32::try_from(id)
        .ok()
        .and_then(|id| world.store.user(id).ok().flatten())
    else {
        conn.send_html("");
        return Ok(());
    };
    let mut tpl = conn.template("homes/widget/habblet/avatarinfo");
    tpl.set("avatar", player_details(&world, &user));
    conn.render(tpl)
}

/// `/myhabbo/avatarlist/membersearchpaging`.
pub fn member_search_paging(conn: &mut Conn) -> WebResult {
    let (widget_id, page, search) = paging_params(conn)?;
    let world = world(conn);
    let Some(w) = homes::sticker(&world, widget_id) else {
        conn.send_html("");
        return Ok(());
    };
    let group_id = w.group_id.max(0) as u32;
    let members = data::count_members(&world, group_id, false) + 1;
    let mut tpl = conn.template("homes/widget/habblet/membersearchpaging");
    tpl.set("sticker", homes::widget(&world, &w, 1));
    tpl.set("pages", if members > 0 { (members + 31) / 32 } else { 1 });
    tpl.set("members", members);
    tpl.set(
        "membersList",
        Value::list(homes::members_page(&world, group_id, &search, page, false)),
    );
    tpl.set("currentPage", page);
    let group = world
        .store
        .record::<rusbbo_storage::Group>(group_id)
        .ok()
        .flatten()
        .map(|g| model::group(&world, &g));
    tpl.set("group", group);
    conn.render(tpl)
}
