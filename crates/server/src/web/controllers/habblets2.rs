//! Java: `MinimailController` (the actions), `QuickmenuController`, `ProxyHabblet`,
//! `UpdateMottoController`, `EventController`, `NavigationComponent`, `InviteController`,
//! `VerifyController`, `AdvertisementController`.

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::Minimail;

use super::minimail::{Outcome, append_messages};
use super::{remove_html_tags, xss_clear};
use crate::web::beans::{has_club, now, player_details};
use crate::web::{Conn, Resp, WebResult, data, model, text};

fn minimail_page(conn: &mut Conn, outcome: Outcome) -> WebResult {
    let mut tpl = conn.template("habblet/minimail/minimail_messages");
    if append_messages(conn, &mut tpl, false, outcome) {
        conn.render(tpl)?;
    }
    Ok(())
}

/// `/minimail/loadMessages`.
pub fn load_messages(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    minimail_page(conn, Outcome::None)
}

/// The user's friend ids in Java's `HashMap<Integer, MessengerUser>` order (ascending for the
/// ids a hotel has, as each id hashes to itself).
fn friend_ids(conn: &Conn, user_id: u32) -> Vec<u32> {
    let mut ids: Vec<u32> = conn
        .world()
        .store
        .messenger_friends(user_id)
        .unwrap_or_default()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let mut capacity = 16usize;
    while ids.len() as f64 > capacity as f64 * 0.75 {
        capacity *= 2;
    }
    ids.sort_by_key(|id| ((*id as usize ^ (*id as usize >> 16)) & (capacity - 1), *id));
    ids
}

/// `/minimail/recipients`.
pub fn recipients(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        return Ok(());
    }
    let ids = friend_ids(conn, conn.user_id());
    let list: Vec<String> = ids
        .iter()
        .filter_map(|id| conn.world().store.user(*id).ok().flatten())
        .map(|u| format!("{{\"id\":{},\"name\":\"{}\"}}", u.id, u.username))
        .collect();
    conn.send_html(format!("/*-secure-\n[{}]\n */", list.join(",")));
    Ok(())
}

/// `/minimail/preview`.
pub fn preview(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let body = text::bbcode(
        conn.world(),
        &text::html_escape(&conn.post.string("body")),
        false,
    );
    conn.send_html(body);
    Ok(())
}

fn new_mail(
    target: u32,
    to: u32,
    sender: u32,
    subject: &str,
    message: &str,
    conversation: u32,
) -> Minimail {
    Minimail {
        target_id: target,
        to_id: to,
        sender_id: sender,
        subject: subject.into(),
        message: message.into(),
        conversation_id: conversation,
        date_sent: now(),
        ..Default::default()
    }
}

/// `/minimail/sendMessage`.
pub fn send_message(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let message = conn.post.string("body");
    let user_id = conn.user_id();
    let mute = data::statistic(conn.world(), user_id, "mute_expires_at");
    let muted = mute > 0 && mute > now();
    let mut created: Vec<Minimail> = Vec::new();
    if !muted {
        let clean = text::filter_sentence(conn.world(), &message) == message;
        if conn.post.contains("recipientIds") {
            let subject = conn.post.string("subject");
            let friends = friend_ids(conn, user_id);
            for part in conn.post.string("recipientIds").split(',') {
                if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let to: u32 = part.parse()?;
                if !friends.contains(&to) {
                    continue;
                }
                if clean {
                    created.push(new_mail(user_id, to, user_id, &subject, &message, 0));
                    created.push(new_mail(to, to, user_id, &subject, &message, 0));
                }
            }
        } else if conn.post.contains("messageId") {
            let id = conn.post.int("messageId").unwrap_or(0).max(0) as u32;
            if let Some(mut original) = get_message(conn, id, user_id) {
                original.conversation_id = id;
                conn.world().store.put_record(&original)?;
                if clean {
                    let subject = format!("Re: {}", original.subject);
                    created.push(new_mail(
                        user_id,
                        original.sender_id,
                        user_id,
                        &subject,
                        &message,
                        id,
                    ));
                    created.push(new_mail(
                        original.sender_id,
                        original.sender_id,
                        user_id,
                        &subject,
                        &message,
                        id,
                    ));
                }
            }
        }
        let mut w = conn.world().store.write()?;
        for mut m in created {
            w.insert_record(&mut m)?;
        }
        w.commit()?;
    }
    minimail_page(conn, Outcome::Sent { muted })
}

/// Java: `MinimailDao.getMessage(id, user)`: a message the user received or sent.
fn get_message(conn: &Conn, id: u32, user_id: u32) -> Option<Minimail> {
    conn.world()
        .store
        .record::<Minimail>(id)
        .ok()
        .flatten()
        .filter(|m| m.target_id == user_id || m.sender_id == user_id)
}

/// `/minimail/loadMessage`.
pub fn load_message(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let user_id = conn.user_id();
    let Some(id) = conn.get.int("messageId") else {
        conn.send_html("1");
        return Ok(());
    };
    if id <= 0 {
        conn.send_html("2");
        return Ok(());
    }
    let Some(mut m) = get_message(conn, id as u32, user_id) else {
        conn.send_html("");
        return Ok(());
    };
    // Java: getString("minimailLabel").equalsIgnoreCase throws without a label.
    let label = conn
        .session()
        .string("minimailLabel")
        .ok_or_else(|| crate::web::WebError("no minimailLabel".into()))?;
    if (label.eq_ignore_ascii_case("conversation") || label.eq_ignore_ascii_case("sent"))
        && user_id != m.target_id
        && user_id != m.sender_id
    {
        conn.send_html("");
        return Ok(());
    }
    if !m.is_read {
        m.is_read = true;
        conn.world().store.put_record(&m)?;
    }
    let mut tpl = conn.template("habblet/minimail/minimail_load_message");
    tpl.set("minimailLabel", label);
    tpl.set("minimailMessage", model::minimail(&conn.web.world, &m));
    conn.render(tpl)
}

/// `/minimail/deleteMessage`: to the trash, or for good from the trash.
pub fn delete_message(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let user_id = conn.user_id();
    let Some(id) = conn.post.int("messageId") else {
        conn.send_html("");
        return Ok(());
    };
    let Some(mut m) = (id > 0)
        .then(|| get_message(conn, id as u32, user_id))
        .flatten()
    else {
        conn.send_html("");
        return Ok(());
    };
    if !m.is_trash {
        m.is_trash = true;
    } else {
        // MinimailDao.deleteMessage: `WHERE id = ? AND target_id = ?` (the message's own target).
        m.is_deleted = true;
    }
    conn.world().store.put_record(&m)?;
    minimail_page(conn, Outcome::Deleted)
}

/// `/minimail/undeleteMessage`.
pub fn undelete_message(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let user_id = conn.user_id();
    let Some(id) = conn.post.int("messageId") else {
        conn.send_html("");
        return Ok(());
    };
    let Some(mut m) = (id > 0)
        .then(|| get_message(conn, id as u32, user_id))
        .flatten()
    else {
        conn.send_html("");
        return Ok(());
    };
    m.is_trash = false;
    conn.world().store.put_record(&m)?;
    minimail_page(conn, Outcome::Undeleted)
}

/// `/minimail/emptyTrash`.
pub fn empty_trash(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let user_id = conn.user_id();
    for mut m in conn
        .world()
        .store
        .records_where::<Minimail>(|m| m.is_trash && m.target_id == user_id)?
    {
        m.is_deleted = true;
        conn.world().store.put_record(&m)?;
    }
    minimail_page(conn, Outcome::TrashEmptied)
}

/// `/quickmenu/groups`.
pub fn quickmenu_groups(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let world = conn.web.world.clone();
    let groups: Vec<Value> = data::joined_groups(&world, conn.user_id())
        .iter()
        .map(|g| model::group(&world, g))
        .collect();
    let mut tpl = conn.template("quickmenu/groups");
    tpl.set("groups", Value::list(groups));
    conn.render(tpl)
}

/// `/quickmenu/friends_all`: ten online and ten offline friends, last seen first.
pub fn quickmenu_friends(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let user_id = conn.user_id();
    let mut friends: Vec<(rusbbo_storage::User, i32)> = friend_ids(conn, user_id)
        .into_iter()
        .filter_map(|id| conn.world().store.user(id).ok().flatten())
        .map(|u| {
            let cat = conn
                .world()
                .store
                .messenger_friends(user_id)
                .unwrap_or_default()
                .into_iter()
                .find(|(f, _)| *f == u.id)
                .map(|(_, c)| c)
                .unwrap_or(0);
            (u, cat)
        })
        .collect();
    friends.sort_by_key(|(u, _)| std::cmp::Reverse(u.last_online));
    let pick = |online: bool| {
        Value::list(
            friends
                .iter()
                .filter(|(u, _)| u.is_online == online)
                .take(10)
                .map(|(u, c)| model::messenger_user(u, *c))
                .collect::<Vec<_>>(),
        )
    };
    let (on, off) = (pick(true), pick(false));
    let mut tpl = conn.template("quickmenu/friends_all");
    tpl.set("onlineFriends", on);
    tpl.set("offlineFriends", off);
    conn.render(tpl)
}

/// `/quickmenu/rooms`.
pub fn quickmenu_rooms(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let world = conn.web.world.clone();
    let rooms: Vec<Value> = world
        .store
        .rooms_by_owner(conn.user_id())?
        .iter()
        .map(|r| model::room(&world, r))
        .collect();
    let mut tpl = conn.template("quickmenu/rooms");
    tpl.set("rooms", Value::list(rooms));
    conn.render(tpl)
}

/// Java: `CommunityDao.getHotGroups`: groups created in the last month, by members who joined
/// in the last month (most first).
fn hot_groups(conn: &Conn, limit: usize, offset: usize) -> Value {
    use chrono::{Local, TimeZone};
    let world = conn.web.world.clone();
    let today = Local::now().date_naive();
    let start_day = today
        .checked_sub_months(chrono::Months::new(1))
        .unwrap_or(today);
    let at = |d: chrono::NaiveDate| {
        Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .map(|t| t.timestamp())
            .unwrap_or(0)
    };
    let (from, to) = (at(start_day), at(today));
    let members = world
        .store
        .records::<rusbbo_storage::GroupMember>()
        .unwrap_or_default();
    let mut groups: Vec<(rusbbo_storage::Group, usize)> = world
        .store
        .records_where::<rusbbo_storage::Group>(|g| g.created_at >= from && g.created_at <= to)
        .unwrap_or_default()
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|g| {
            let n = members
                .iter()
                .filter(|m| m.group_id == g.id && m.created_at >= from && m.created_at <= to)
                .count();
            (g, n)
        })
        .collect();
    // TODO(groups): Java sorts a HashMap keyed by Group (identity hashes): ties come out in no
    // fixed order; here they keep id order.
    groups.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    Value::list(
        groups
            .iter()
            .map(|(g, _)| model::group(&world, g))
            .collect::<Vec<_>>(),
    )
}

/// `/habblet/proxy?hid=`.
pub fn more_info(conn: &mut Conn) -> WebResult {
    if !conn.get.contains("hid") {
        conn.send_html("");
        return Ok(());
    }
    let world = conn.web.world.clone();
    let hid = conn.get.string("hid");
    let rooms = |list: Vec<rusbbo_storage::Room>| {
        Value::list(
            list.iter()
                .map(|r| model::room(&world, r))
                .collect::<Vec<_>>(),
        )
    };
    match hid.as_str() {
        "h21" => {
            let mut tpl = conn.template("habblet/staff_pick_rooms");
            tpl.set(
                "rooms",
                rooms(conn.web.watchdog.snapshot().staff_pick_rooms.clone()),
            );
            conn.render(tpl)
        }
        "h122" => {
            let limit = data::cfg_int(&world, "hot.groups.community.limit").max(0) as usize;
            let mut tpl = conn.template("habblet/community_hot_groups");
            tpl.set("hotGroups", hot_groups(conn, limit, 0));
            tpl.set("hotHiddenGroups", hot_groups(conn, limit, limit));
            conn.render(tpl)
        }
        "h120" => {
            let mut tpl = conn.template("habblet/showMoreRooms");
            tpl.set(
                "highestRatedRooms",
                rooms(data::highest_rated_rooms(&world, 5, 0)),
            );
            tpl.set(
                "highestHiddenRatedRooms",
                rooms(data::highest_rated_rooms(&world, 5, 5)),
            );
            conn.render(tpl)
        }
        "h24" => {
            let mut tpl = conn.template("habblet/tagList");
            tpl.set("tagCloud", super::site::tag_cloud(conn, 20));
            conn.render(tpl)
        }
        "groups" => {
            let limit = data::cfg_int(&world, "hot.groups.limit").max(0) as usize;
            let mut tpl = conn.template("habblet/hot_groups");
            tpl.set("groups", hot_groups(conn, limit, 0));
            conn.render(tpl)
        }
        _ => {
            conn.send_html("");
            Ok(())
        }
    }
}

const NEWS_HABBLET: &str = "<div class=\"habblet-container \">\t\t\n\t\n\t<div id=\"news-habblet-container\">\n\t\n\t\t<div class=\"title\">\n\t\t\n\t\t\t<div class=\"habblet-close\"></div>\n\t\t\t\n\t\t\t<div>The shit you don't even wanna know!</div>\n\t\t\t\n\t\t</div>\n\t\t\n\t\t<div class=\"content-container\">\n\t\t\n\t\t\t<div id=\"news-articles\">\n\t\t\t\n\t\t\t\t<ul id=\"news-articlelist\" class=\"articlelist\" style=\"display: none\">\n\n\t\t\t\t</ul>\n\t\t\t\t\n\t\t\t</div>\n\t\t\t\n\t\t</div>\n\t\t\n\t\t<div class=\"news-footer\"></div>\n\t\n\t</div>\n\n\t<script type=\"text/javascript\">    \n\t\tL10N.put(\"news.promo.readmore\", \"Read more\").put(\"news.promo.close\", \"Close article\");\n\t\tNews.init(false);\n\t</script>\n\n</div>\n";

/// `/habblet/cproxy`: the client's minimail (and news) habblet.
pub fn cproxy(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    if conn.get.contains("habbletKey") && conn.get.string("habbletKey").eq_ignore_ascii_case("news")
    {
        conn.send_html(NEWS_HABBLET);
        return Ok(());
    }
    let mut tpl = conn.template("habblet/minimail");
    conn.session().set("minimailLabel", "inbox");
    if !append_messages(conn, &mut tpl, true, Outcome::None) {
        return Ok(());
    }
    tpl.set("minimailClient", true);
    conn.render(tpl)
}

/// Java: `XSSUtil.verifyKey`.
fn xss_verify(conn: &mut Conn, route: &str) -> bool {
    let (key, seed, requested) = {
        let s = conn.session();
        (
            s.contains("xssKey").then(|| s.int("xssKey")),
            s.contains("xssSeed").then(|| s.int("xssSeed")),
            s.string("xssRequested"),
        )
    };
    let ok = match (key, seed, requested) {
        (Some(key), Some(seed), Some(r)) => {
            r.eq_ignore_ascii_case(route) && super::club::java_random_next_int(seed as i64) == key
        }
        _ => false,
    };
    xss_clear(conn);
    ok
}

/// `/habblet/ajax/clear_hand`: deletes the furni in the hand (from `/credits`).
pub fn clear_hand(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    if !xss_verify(conn, "/credits") {
        conn.send_html("Failed to securely verify request");
        return Ok(());
    }
    let user_id = conn.user_id();
    conn.world().store.delete_hand_items(user_id)?;
    crate::rcon::send(
        conn.world(),
        "refresh_hand",
        &[("userId", user_id.to_string())],
    );
    conn.send_html("");
    Ok(())
}

/// `/habblet/ajax/token_generate`.
pub fn token_generate(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let token = format!("token-{}", uuid::Uuid::new_v4());
    conn.session().set("authenticationToken", token.as_str());
    conn.send_html(token);
    Ok(())
}

const CROCO: &str = "<script>document.getElementById(\"habbo-plate\").innerHTML = \"<img src='";

/// `/habblet/ajax/updatemotto` (once a second at most).
pub fn update_motto(conn: &mut Conn) -> WebResult {
    let user_id = conn.session().int("user.id");
    if user_id < 1 {
        conn.send_html("");
        return Ok(());
    }
    let Some(user) = conn.world().store.user(user_id as u32)? else {
        return Err(crate::web::WebError("no user".into()));
    };
    let site = data::cfg_string(conn.world(), "site.path");
    let croco = format!(
        "{CROCO}{site}/web-gallery/images/sticker_croco.gif' style='margin-top: 57px'>\";</script>"
    );
    if conn.session().contains("lastMottoUpdate") {
        let last = conn.session().long("lastMottoUpdate");
        if now() < last {
            conn.send_html(format!("{croco}{}", text::html_escape(&user.motto)));
            return Ok(());
        }
    }
    conn.session().set("lastMottoUpdate", now().to_string());
    let mut motto =
        text::filter_sentence(conn.world(), &remove_html_tags(&conn.post.string("motto")));
    if motto.encode_utf16().count() > 100 {
        motto = motto.chars().take(100).collect();
    }
    let mut response = String::new();
    if motto.replace(' ', "").is_empty() {
        response = "Click to enter your motto/ status".into();
        motto = String::new();
    } else if motto.to_lowercase() == "crikey" {
        response = croco;
    }
    if user.motto != motto {
        let m = motto.clone();
        conn.world().store.update_user(user.id, |u| u.motto = m)?;
        if user.is_online || conn.world().player_by_id(user.id).is_some() {
            crate::rcon::send(
                conn.world(),
                "refresh_looks",
                &[("userId", user.id.to_string())],
            );
        }
    }
    conn.send_html(format!("{response}{}", text::html_escape(&motto)));
    Ok(())
}

/// `/habblet/ajax/load_events`.
pub fn load_events(conn: &mut Conn) -> WebResult {
    if !conn.post.contains("eventTypeId") {
        conn.send_html("");
        return Ok(());
    }
    let filter = conn
        .post
        .int("eventTypeId")
        .ok_or_else(|| crate::web::WebError("eventTypeId".into()))?;
    let world = conn.web.world.clone();
    let events: Vec<Value> = conn
        .web
        .watchdog
        .snapshot()
        .events
        .iter()
        .filter(|e| e.category_id == filter)
        .map(|e| model::event(&world, e))
        .collect();
    let mut tpl = conn.template("habblet/load_events");
    tpl.set("events", Value::list(events));
    conn.render(tpl)
}

/// `/components/roomNavigation`.
pub fn navigation(conn: &mut Conn) -> WebResult {
    conn.send_html("");
    Ok(())
}

/// `/habblet/ajax/mgmgetinvitelink`.
pub fn invite_link(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let tpl = conn.template("habblet/invite_referralLink");
    conn.render(tpl)
}

/// A user's friends and requests (Java: `Messenger`), for the invite habblets.
fn messenger_value(conn: &Conn, user_id: u32) -> Value {
    let friends = friend_ids(conn, user_id);
    let requests = conn
        .world()
        .store
        .messenger_requests(user_id)
        .unwrap_or_default();
    let (f2, r2) = (friends.clone(), requests);
    Map::new()
        .with_fn("hasFriend", move |a| {
            Ok(Value::Bool(
                a.first()
                    .and_then(Value::as_i64)
                    .is_some_and(|id| f2.contains(&(id as u32))),
            ))
        })
        .with_fn("hasRequest", move |a| {
            Ok(Value::Bool(
                a.first()
                    .and_then(Value::as_i64)
                    .is_some_and(|id| r2.contains(&(id as u32))),
            ))
        })
        .into()
}

/// `/habblet/habbosearchcontent`.
pub fn search_content(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("habblet/invite_searchContent");
    let user_id = conn.user_id();
    let query = conn.post.string("searchString");
    let page = if conn.post.contains("pageNumber") {
        conn.post
            .int("pageNumber")
            .ok_or_else(|| crate::web::WebError("pageNumber".into()))?
            - 1
    } else {
        0
    } as i64;
    let mut found: Vec<rusbbo_storage::User> = conn
        .world()
        .store
        .messenger_search(&query)?
        .into_iter()
        .filter(|id| *id != user_id)
        .filter_map(|id| conn.world().store.user(id).ok().flatten())
        .collect();
    found.sort_by(|a, b| a.username.cmp(&b.username));
    let pages: Vec<&[rusbbo_storage::User]> = found.chunks(5).collect();
    let has = |p: i64| p >= 0 && (p as usize) < pages.len();
    let world = conn.web.world.clone();
    let results: Vec<Value> = if has(page) {
        pages[page as usize]
            .iter()
            .map(|u| player_details(&world, u))
            .collect()
    } else {
        Vec::new()
    };
    let previous = if has(page - 1) { page } else { -1 };
    let next = if has(page + 1) { page + 2 } else { -1 };
    tpl.set("searchResults", Value::list(results));
    tpl.set("currentPage", page + 1);
    tpl.set("totalPages", pages.len());
    tpl.set("previousPageId", previous);
    tpl.set("nextPageId", next);
    tpl.set("messenger", messenger_value(conn, user_id));
    conn.render(tpl)
}

/// `/habblet/ajax/confirmAddFriend`.
pub fn confirm_add_friend(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("habblet/invite_confirmAddFriend");
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        conn.send_html("");
        return Ok(());
    };
    tpl.set("username", user.username);
    conn.render(tpl)
}

/// Java: `InviteController.createFriendRequestResponse`.
fn friend_request(conn: &mut Conn, account_id: i32) -> Result<String, crate::web::WebError> {
    let me = conn.user_id();
    let world = conn.web.world.clone();
    let limit = |u: &rusbbo_storage::User| {
        let s = world.settings.read();
        if has_club(u) {
            s.int("messenger.max.friends.club")
        } else {
            s.int("messenger.max.friends.nonclub")
        }
    };
    // Java: MessengerManager.getMessengerData of an unknown user throws (the error page).
    let target = (account_id > 0)
        .then(|| world.store.user(account_id as u32).ok().flatten())
        .flatten()
        .ok_or_else(|| crate::web::WebError(format!("no user {account_id}")))?;
    if target.username.eq_ignore_ascii_case("Abigail.Ryan") {
        return Ok("There was an error finding the user for the friend request.".into());
    }
    let Some(callee) = world.store.user(me)? else {
        return Err(crate::web::WebError("no user".into()));
    };
    let my_friends = world.store.messenger_friends(me)?.len() as i64;
    let their_friends = world.store.messenger_friends(target.id)?;
    Ok(if my_friends >= limit(&callee) {
        "Your friends list is full.".into()
    } else if their_friends.iter().any(|(id, _)| *id == me) {
        "This person is already your friend".into()
    } else if world.store.messenger_requests(target.id)?.contains(&me) {
        "There is already a friend request for this user.".into()
    } else if their_friends.len() as i64 >= limit(&target) {
        "This user's friend list is full.".into()
    } else if !target.allow_friend_requests {
        "This user does not accept friend requests at the moment.".into()
    } else if account_id == me as i32 {
        "There was an error processing your request.".into()
    } else {
        world.store.messenger_new_request(me, target.id)?;
        crate::rcon::send(
            &world,
            "friendrequest",
            &[
                ("userId", me.to_string()),
                ("friendId", account_id.to_string()),
            ],
        );
        "Friend request has been sent successfully.".into()
    })
}

/// `/habblet/ajax/addFriend`.
pub fn add_friend(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let id = conn.post.int("accountId").unwrap_or(0);
    let mut tpl = conn.template("habblet/invite_addFriend");
    let message = friend_request(conn, id)?;
    tpl.set("message", message);
    conn.render(tpl)
}

/// `/myhabbo/friends/add`.
pub fn add(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let id = conn.post.int("accountId").unwrap_or(0);
    let message = friend_request(conn, id)?;
    conn.send(Resp::typed(
        200,
        "application/x-javascript",
        format!("Dialog.showInfoDialog(\"add-friend-messages\", \"{message}\", \"OK\");")
            .into_bytes(),
    ));
    Ok(())
}

/// `/api/verify/get/*`.
pub fn verify_get(conn: &mut Conn) -> WebResult {
    let code = conn.wildcard(0).to_string();
    if code.trim().is_empty() {
        conn.send_html("error: INVALID");
        return Ok(());
    }
    let found = conn
        .world()
        .store
        .records_where::<rusbbo_storage::VerifyCode>(|v| v.code == code)?;
    match found
        .first()
        .and_then(|v| conn.world().store.user(v.id).ok().flatten())
    {
        Some(u) => conn.send_html(u.username),
        None => conn.send_html("error: INVALID"),
    }
    Ok(())
}

/// `/api/verify/clear/*`.
pub fn verify_clear(conn: &mut Conn) -> WebResult {
    let code = conn.wildcard(0).to_string();
    if code.trim().is_empty() {
        conn.send_html("error: INVALID");
        return Ok(());
    }
    if let Some(v) = conn
        .world()
        .store
        .records_where::<rusbbo_storage::VerifyCode>(|v| v.code == code)?
        .first()
    {
        conn.world()
            .store
            .delete_record::<rusbbo_storage::VerifyCode>(v.id)?;
    }
    conn.send_html("SUCCESS");
    Ok(())
}

fn advertisement(conn: &mut Conn, image: bool) -> WebResult {
    if !conn.get.contains("ad") {
        conn.send_html("");
        return Ok(());
    }
    let id = conn
        .get
        .int("ad")
        .ok_or_else(|| crate::web::WebError("ad".into()))?;
    match (id > 0)
        .then(|| {
            conn.world()
                .store
                .record::<rusbbo_storage::RoomAd>(id as u32)
                .ok()
                .flatten()
        })
        .flatten()
    {
        Some(ad) => {
            let target = if image {
                ad.image
            } else {
                ad.url.unwrap_or_else(|| "null".into())
            };
            conn.redirect(&target);
        }
        None => conn.send_html(""),
    }
    Ok(())
}

/// `/api/advertisement/get_img`.
pub fn ad_image(conn: &mut Conn) -> WebResult {
    advertisement(conn, true)
}

/// `/api/advertisement/get_url`.
pub fn ad_url(conn: &mut Conn) -> WebResult {
    advertisement(conn, false)
}
