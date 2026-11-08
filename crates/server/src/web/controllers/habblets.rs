//! Small habblets: Java `RoomSelectionController`, `FeedController`, `NameCheckController`.

use super::me::beginner_gift_progress;
use super::register::name_error;
use crate::web::{Conn, Resp, WebResult, beans, data};

/// `/habblet/ajax/roomselectionConfirm`.
pub fn room_selection_confirm(conn: &mut Conn) -> WebResult {
    let tpl = conn.template("habblet/roomselectionConfirm");
    conn.render(tpl)
}

/// `/habblet/ajax/roomselectionCreate`: Java checks the request and answers nothing (the room
/// is made by `/client?createRoom=`).
pub fn room_selection_create(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() || !conn.post.contains("roomType") {
        conn.send_html("");
        return Ok(());
    }
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Err(crate::web::WebError("no user".into()));
    };
    if !data::can_select_room(conn.world(), &user) {
        conn.send_html("");
        return Ok(());
    }
    // Integer.parseInt throws on junk.
    let _: i32 = conn.post.string("roomType").parse()?;
    conn.send_html("");
    Ok(())
}

/// `/habblet/ajax/roomselectionHide`.
pub fn room_selection_hide(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Err(crate::web::WebError("no user".into()));
    };
    if data::can_select_room(conn.world(), &user) {
        conn.world()
            .store
            .update_user(user.id, |u| u.selected_room_id = -1)?;
        data::set_statistic(conn.world(), user.id, "newbie_room_layout", -1);
    }
    conn.send_html("");
    Ok(())
}

/// `/habblet/ajax/removeFeedItem`: deletes the n-th alert (newest first).
pub fn remove_feed_item(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let index = conn.post.int("feedItemIndex").unwrap_or(-1);
    if index != -1 {
        let alerts = data::alerts(conn.world(), conn.user_id());
        if let Some(a) = usize::try_from(index).ok().and_then(|i| alerts.get(i)) {
            conn.world()
                .store
                .delete_record::<rusbbo_storage::Alert>(a.id)?;
        }
        conn.send_html("");
    }
    Ok(())
}

/// `/habblet/ajax/nextgift`.
pub fn next_gift(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let mut tpl = conn.template("habblet/nextgift");
    let world = conn.web.world.clone();
    let Some(user) = world.store.user(conn.user_id())? else {
        return Err(crate::web::WebError("no user".into()));
    };
    let stat = |c: &str| data::statistic(&world, user.id, c);
    tpl.set("newbieRoomLayout", stat("newbie_room_layout"));
    tpl.set("newbieNextGift", stat("newbie_gift"));
    if stat("newbie_room_layout") > 0 && stat("newbie_gift") > 0 {
        let now = beans::now();
        let mut seconds = stat("newbie_gift_time") - now;
        if beginner_gift_progress(&world, &user) {
            seconds = stat("newbie_gift_time") - now;
            tpl.set("newbieNextGift", stat("newbie_gift"));
        }
        tpl.set("newbieGiftSeconds", seconds.max(0));
    }
    conn.render(tpl)
}

/// `/habblet/ajax/giftqueueHide`.
pub fn gift_queue_hide(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.send_html("");
        return Ok(());
    }
    let id = conn.user_id();
    if data::statistic(conn.world(), id, "newbie_gift") == 3 {
        data::set_statistic(conn.world(), id, "newbie_gift", 4);
    }
    conn.send_html("");
    Ok(())
}

/// `/habblet/ajax/namecheck`.
pub fn name_check(conn: &mut Conn) -> WebResult {
    let name = conn.post.string("name");
    let message = match name_error(conn, &name) {
        6 => "This name is unacceptable to hotel management.",
        5 => "Your username is invalid or contains invalid characters.",
        4 => "This name is not allowed.",
        3 => "The name you have chosen is too long.",
        2 => "Please enter a username.",
        1 => "A user with this name already exists.",
        _ => "",
    };
    let mut r = Resp::html(200, "");
    r.headers.push((
        "X-JSON".into(),
        format!("{{\"registration_name\":\"{message}\"}}"),
    ));
    conn.send(r);
    Ok(())
}
