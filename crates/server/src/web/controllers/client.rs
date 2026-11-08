//! Java: `controllers/site/ClientController`: the pages that open the game client.

use super::{account::logout_session, xss_clear};
use crate::web::{Conn, Resp, WebResult, data};

/// `/client`.
pub fn client(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/login_popup");
        return Ok(());
    }
    let query = conn
        .uri
        .split_once('?')
        .map(|(_, q)| format!("?{q}"))
        .unwrap_or_default();
    conn.redirect(&format!("/shockwave_client{query}"));
    Ok(())
}

/// What `/shockwave_client`, `/flash_client` and `/beta_client` share up to the template: the
/// checks, room creation and forwarding. `None` when the request was answered already.
struct Launch {
    tpl: crate::web::tpl::Tpl,
    user: rusbbo_storage::User,
    forward_room: bool,
    forward_type: i32,
    forward_id: i32,
}

fn launch(
    conn: &mut Conn,
    view: &str,
    template_first: bool,
) -> Result<Option<Launch>, crate::web::WebError> {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/login_popup");
        return Ok(None);
    }
    let mut tpl = if template_first {
        Some(conn.template(view))
    } else {
        None
    };
    let uri = conn.uri.clone();
    conn.session().set("clientRequest", uri);
    if conn.session().boolean("clientAuthenticate") {
        conn.redirect("/account/reauthenticate");
        return Ok(None);
    }
    let tpl = match tpl.take() {
        Some(t) => t,
        None => conn.template(view),
    };
    let Some(mut user) = conn.world().store.user(conn.user_id())? else {
        logout_session(conn);
        conn.redirect("/");
        return Ok(None);
    };
    if data::active_ban(conn.world(), &user).is_some() {
        conn.redirect("/account/banned");
        return Ok(None);
    }
    let (mut forward_room, mut forward_type, mut forward_id) = (false, -1, -1);
    let create = conn.get.string("createRoom");
    if conn.get.contains("createRoom")
        && !create.is_empty()
        && create.chars().all(|c| c.is_ascii_digit())
    {
        let room_type: i32 = create.parse().unwrap_or(-1);
        let set_gift = if !data::can_select_room(conn.world(), &user) {
            data::statistic(conn.world(), user.id, "newbie_room_layout") == 0
                && (0..=5).contains(&room_type)
        } else {
            data::select_room(conn.world(), user.id, room_type)
        };
        if set_gift {
            let now = crate::web::beans::now();
            data::set_statistic(
                conn.world(),
                user.id,
                "newbie_room_layout",
                (room_type + 1) as i64,
            );
            data::set_statistic(conn.world(), user.id, "newbie_gift", 1);
            data::set_statistic(conn.world(), user.id, "newbie_gift_time", now + 24 * 3600);
        }
        if let Some(u) = conn.world().store.user(conn.user_id())? {
            user = u;
        }
        forward_room = true;
        forward_type = 2;
        forward_id = user.selected_room_id;
    }
    if conn.get.contains("forwardId") {
        forward_room = true;
        if let (Some(id), Some(ty)) = (conn.get.int("roomId"), conn.get.int("forwardId")) {
            forward_id = id;
            forward_type = ty;
        } else if let Some(id) = conn.get.int("roomId") {
            forward_id = id;
        }
    }
    Ok(Some(Launch {
        tpl,
        user,
        forward_room,
        forward_type,
        forward_id,
    }))
}

/// The SSO ticket the client logs in with: a new one when `reset.sso.after.login` is on or the
/// user has none.
fn sso_ticket(conn: &Conn, user: &rusbbo_storage::User) -> String {
    let current = user.sso_ticket.clone().unwrap_or_default();
    if data::cfg_bool(conn.world(), "reset.sso.after.login") || current.trim().is_empty() {
        let ticket = uuid::Uuid::new_v4().to_string();
        let t = ticket.clone();
        let _ = conn
            .world()
            .store
            .update_user(user.id, |u| u.sso_ticket = Some(t));
        ticket
    } else {
        current
    }
}

/// `/shockwave_client`.
pub fn shockwave_client(conn: &mut Conn) -> WebResult {
    let Some(Launch {
        mut tpl,
        user,
        forward_room,
        forward_type,
        forward_id,
    }) = launch(conn, "client", false)?
    else {
        return Ok(());
    };
    if conn.get.contains("shortcut") && conn.get.string("shortcut") == "roomomatic" {
        tpl.set("shortcut", "shortcut.id=1;");
    }
    let ticket = sso_ticket(conn, &user);
    tpl.set("ssoTicket", ticket);
    tpl.set("forwardRoom", forward_room);
    if forward_room {
        let (t, i) = (forward_type, forward_id);
        tpl.set(
            "forward",
            format!(
                "<param name=\"sw9\" value=\"forward.type={t};forward.id={i};processlog.url=\">"
            ),
        );
        tpl.set(
            "forwardSub",
            format!("sw9=\"forward.type={t};forward.id={i};processlog.url=\""),
        );
        tpl.set(
            "forwardScript",
            format!("<param name=\\\"sw9\\\" value=\\\"forward.type={t};forward.id={i};processlog.url=\\\">"),
        );
        tpl.set(
            "forwardSubScript",
            format!("sw9=\\\"forward.type={t};forward.id={i};processlog.url=\\\""),
        );
    }
    conn.render(tpl)
}

/// `/flash_client` (and `/beta_client` with its own view).
fn flash_like(conn: &mut Conn, view: &str) -> WebResult {
    let Some(Launch {
        mut tpl,
        user,
        forward_room,
        forward_type,
        forward_id,
    }) = launch(conn, view, true)?
    else {
        return Ok(());
    };
    tpl.set("forwardRoom", forward_room);
    tpl.set("forwardId", forward_id);
    tpl.set("forwardType", forward_type);
    let ticket = sso_ticket(conn, &user);
    tpl.set("ssoTicket", ticket);
    conn.render(tpl)
}

pub fn flash_client(conn: &mut Conn) -> WebResult {
    flash_like(conn, "client_flash")
}

pub fn beta_client(conn: &mut Conn) -> WebResult {
    flash_like(conn, "client_beta")
}

/// `/client_popup/install_shockwave`.
pub fn install_shockwave(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/login_popup");
        return Ok(());
    }
    let tpl = conn.template("client_install_shockwave");
    if let Some(u) = conn.world().store.user(conn.user_id())?
        && data::active_ban(conn.world(), &u).is_some()
    {
        conn.redirect("/account/banned");
        return Ok(());
    }
    conn.render(tpl)
}

/// `/components/updateHabboCount`.
pub fn update_habbo_count(conn: &mut Conn) -> WebResult {
    let online = conn.world().players_online();
    let mut r = Resp::html(200, "");
    let count = rusbbo_pebble::number_format(online as f64, true, "#,##0");
    r.headers.push((
        "X-JSON".into(),
        format!("{{\"habboCountText\":\"{count} members online\"}}"),
    ));
    conn.send(r);
    Ok(())
}

/// `/clientlog/update`, `/cacheCheck`.
pub fn blank(conn: &mut Conn) -> WebResult {
    conn.send_html("");
    Ok(())
}

/// `/client_error`, `/client_connection_failed`.
fn error_page(conn: &mut Conn, view: &str) -> WebResult {
    xss_clear(conn);
    let mut tpl = conn.template(view);
    if conn.is_authenticated()
        && let Some(u) = conn.world().store.user(conn.user_id())?
        && data::active_ban(conn.world(), &u).is_some()
    {
        conn.redirect("/account/banned");
        return Ok(());
    }
    if conn.get.contains("error_id") {
        tpl.set("errorId", conn.get.string("error_id"));
    }
    conn.render(tpl)
}

pub fn client_error(conn: &mut Conn) -> WebResult {
    error_page(conn, "client_error")
}

pub fn connection_failed(conn: &mut Conn) -> WebResult {
    error_page(conn, "client_connection_failed")
}
