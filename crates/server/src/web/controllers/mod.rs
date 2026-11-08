//! Havana-Web's controllers (`org.alexdev.http.controllers`), one module per Java package/class
//! group. [`handle`] maps a route's Java handler name to its port.

mod account;
mod api;
mod client;
mod habblets;
mod me;
mod minimail;
mod recovery;
mod register;
mod site;

use super::routes_table::HOUSEKEEPING_PATH;
use super::tpl::Tpl;
use super::{Conn, WebResult, beans};

/// Java: `BaseController.handleRoute`, run for every request before its route.
pub fn base(conn: &mut Conn) -> WebResult {
    let route = conn.route.clone();
    if conn.request_handled
        && conn.world().settings.read().bool("maintenance")
        && !route.starts_with("/api")
        && !route.starts_with("/maintenance")
        && !route.starts_with(&format!("/{HOUSEKEEPING_PATH}"))
    {
        conn.redirect("/maintenance");
        return Ok(());
    }
    if !route.starts_with("/api")
        && let Some(proto) = conn.header("x-forwarded-proto")
        && let Some(host) = conn.header("host")
        && proto.eq_ignore_ascii_case("http")
    {
        let mut target = format!("https://{host}");
        if !conn.uri.starts_with('/') {
            target.push('/');
        }
        target.push_str(&conn.uri.clone());
        conn.moved_permanently(&target);
        return Ok(());
    }
    if conn.request_handled {
        if conn.is_authenticated() {
            authenticated_route(conn);
        } else {
            check_remember_cookie(conn);
        }
    }
    Ok(())
}

/// `BaseController.handleAuthenticatedRoute`.
fn authenticated_route(conn: &mut Conn) {
    let now = beans::now();
    let route = conn.route.clone();
    let mut s = conn.session();
    if route == "/client" {
        s.set("lastRequest", (now + REAUTHENTICATE_TIME).to_string());
    }
    if s.contains("lastRequest") {
        if now > s.long("lastRequest") {
            s.set("clientAuthenticate", true);
        }
    } else {
        s.set("clientAuthenticate", false);
    }
}

/// Java: `SessionUtil.REAUTHENTICATE_TIME` (30 minutes).
pub const REAUTHENTICATE_TIME: i64 = 30 * 60;
pub const REMEMBER_TOKEN: &str = "remember_token";
pub const REMEMBER_TOKEN_AGE: i64 = 31 * 24 * 3600;
pub const MACHINE_ID_COOKIE: &str = "SECURITY_KEY";

/// Java: `SessionUtil.checkCookie`: the "remember me" token logs the session in.
fn check_remember_cookie(conn: &mut Conn) {
    let Some(token) = conn.cookie(REMEMBER_TOKEN).map(str::to_string) else {
        return;
    };
    if token.trim().is_empty() {
        return;
    }
    let user = conn.world().store.users().ok().and_then(|users| {
        users
            .into_iter()
            .find(|u| u.remember_token.as_deref() == Some(token.as_str()))
    });
    match user {
        Some(u) if u.id > 0 => {
            {
                let mut s = conn.session();
                s.set("authenticated", true);
                s.set("captcha.invalid", false);
                s.set("user.id", u.id as i64);
            }
            if conn.uri == "/home" || conn.uri == "/index" || conn.uri == "/" {
                conn.redirect("/me");
            }
        }
        _ => {
            {
                let mut s = conn.session();
                s.delete("user.id");
                s.delete("authenticated");
            }
            conn.set_cookie(REMEMBER_TOKEN, "", Some(0));
        }
    }
}

/// Java: `XSSUtil.clear`.
pub fn xss_clear(conn: &mut Conn) {
    let mut s = conn.session();
    if s.contains("xssKey") || s.contains("xssSeed") || s.contains("xssRequested") {
        s.delete("xssSeed");
        s.delete("xssKey");
        s.delete("xssRequested");
    }
}

/// Java: `HtmlUtil.removeHtmlTags`.
pub fn remove_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        match rest[i..].find('>') {
            Some(j) => rest = &rest[i + j + 1..],
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Java: `HousekeepingBinder`. TODO(web): housekeeping (phase 5).
pub fn housekeeping_binder(_conn: &mut Conn, _tpl: &mut Tpl) {}

/// Runs the controller for a route's Java handler; `false` when it is not ported yet.
pub fn handle(conn: &mut Conn, handler: &str) -> Result<bool, super::WebError> {
    match handler {
        "HomepageController::homepage" => site::homepage(conn)?,
        "HomepageController::maintenance" => site::maintenance(conn)?,
        "AccountController::submit" => account::submit(conn)?,
        "AccountController::securityCheck" => account::security_check(conn)?,
        "AccountController::me" => me::me(conn)?,
        "AccountController::welcome" => account::welcome(conn)?,
        "AccountController::reauthenticate" => account::reauthenticate(conn)?,
        "AccountController::banned" => account::banned(conn)?,
        "RegisterController::register" => register::register(conn)?,
        "RegisterController::registerCancelled" => register::cancelled(conn)?,
        "RegisterController::captcha" => register::captcha_image(conn)?,
        "RecoveryController::forgot" => recovery::forgot(conn)?,
        "RecoveryController::recovery" => recovery::recovery(conn)?,
        "RecoveryController::activate" => recovery::activate(conn)?,
        "RoomSelectionController::confirm" => habblets::room_selection_confirm(conn)?,
        "RoomSelectionController::create" => habblets::room_selection_create(conn)?,
        "RoomSelectionController::hide" => habblets::room_selection_hide(conn)?,
        "FeedController::removeFeedItem" => habblets::remove_feed_item(conn)?,
        "FeedController::nextgift" => habblets::next_gift(conn)?,
        "FeedController::giftqueueHide" => habblets::gift_queue_hide(conn)?,
        "NameCheckController::namecheck" => habblets::name_check(conn)?,
        "ClientController::shockwaveclient" => client::shockwave_client(conn)?,
        "ClientController::flashClient" => client::flash_client(conn)?,
        "ClientController::betaClient" => client::beta_client(conn)?,
        "ClientController::clientInstallShockwave" => client::install_shockwave(conn)?,
        "ClientController::updateHabboCount" => client::update_habbo_count(conn)?,
        "ClientController::blank" => client::blank(conn)?,
        "ClientController::client_error" => client::client_error(conn)?,
        "ClientController::client_connection_failed" => client::connection_failed(conn)?,
        "TicketController::get" => api::ticket(conn)?,
        "TicketController::login" => api::login(conn)?,
        "AccountController::logout" => account::logout(conn)?,
        "AccountController::login_popup" => account::login_popup(conn)?,
        "ClientController::client" => client::client(conn)?,
        _ => return Ok(false),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags() {
        assert_eq!(remove_html_tags("a<b>c</b>d<e"), "acd<e");
    }
}
