//! Java: `controllers/site/AccountController` and `util/SessionUtil`.

use super::{REAUTHENTICATE_TIME, REMEMBER_TOKEN, REMEMBER_TOKEN_AGE, remove_html_tags, xss_clear};
use crate::web::{Conn, WebResult, beans};

/// Java: `SessionUtil.login`. On success the session is logged in (and a banned user is sent to
/// `/account/banned`).
pub fn login(conn: &mut Conn, username: &str, password: &str, delete_auth: bool) -> bool {
    let user = if username.trim().is_empty() || password.trim().is_empty() {
        None
    } else {
        crate::game::player::check_login(conn.world(), username, password)
    };
    let Some(user) = user else {
        let mut s = conn.session();
        s.set("alertMessage", "Incorrect username or password\n");
        if delete_auth {
            s.delete("user.id");
            s.delete("authenticated");
        }
        return false;
    };
    {
        let mut s = conn.session();
        s.set("authenticated", true);
        s.delete("authenticatedHousekeeping");
        s.set("captcha.invalid", false);
        s.set("user.id", user.id.to_string());
        s.set("clientAuthenticate", false);
        s.set(
            "lastRequest",
            (beans::now() + REAUTHENTICATE_TIME).to_string(),
        );
    }
    if conn.post.string("_login_remember_me") == "true" {
        let token = uuid::Uuid::new_v4().to_string();
        // Java passes "now + 31 days" as the cookie's max age.
        conn.set_cookie(
            REMEMBER_TOKEN,
            &token,
            Some(beans::now() + REMEMBER_TOKEN_AGE),
        );
        let t = token.clone();
        let _ = conn
            .world()
            .store
            .update_user(user.id, |u| u.remember_token = Some(t));
    } else {
        conn.set_cookie(REMEMBER_TOKEN, "", Some(0));
    }
    conn.set_cookie("vote_stamp", "", Some(0));
    if crate::web::data::active_ban(conn.world(), &user).is_some() {
        conn.redirect("/account/banned");
    }
    true
}

/// Java: `SessionUtil.logout`.
pub fn logout_session(conn: &mut Conn) {
    if conn.cookie(REMEMBER_TOKEN).is_some() {
        conn.set_cookie(REMEMBER_TOKEN, "", Some(0));
        let id = conn.user_id();
        let _ = conn
            .world()
            .store
            .update_user(id, |u| u.remember_token = None);
    }
    let mut s = conn.session();
    for k in [
        "user.id",
        "authenticated",
        "authenticatedHousekeeping",
        "minimailLabel",
        "lastBrowsedPage",
    ] {
        s.delete(k);
    }
}

/// `/account/submit`.
pub fn submit(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    let username = remove_html_tags(&conn.post.string("username"));
    let password = remove_html_tags(&conn.post.string("password"));
    if login(conn, &username, &password, true) {
        conn.redirect("/security_check");
        return Ok(());
    }
    let remember_me = conn.post.string("_login_remember_me") == "true";
    let mut tpl = conn.template("account/submit");
    tpl.set("rememberMe", if remember_me { "true" } else { "false" });
    tpl.set("username", username);
    conn.render(tpl)
}

/// `/security_check`.
pub fn security_check(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("/security_check");
    let redirect = conn
        .session()
        .string("lastBrowsedPage")
        .unwrap_or_else(|| "/me".into());
    tpl.set("redirectPath", redirect);
    conn.render(tpl)
}

/// `/account/logout`.
pub fn logout(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    logout_session(conn);
    conn.session().set("page", "logout");
    let tpl = conn.template("account/logout");
    conn.render(tpl)
}

/// `/login_popup`, `/account/login`.
pub fn login_popup(conn: &mut Conn) -> WebResult {
    conn.session().set("page", "login_popup");
    let tpl = conn.template("account/login");
    conn.render(tpl)?;
    conn.session().delete("alertMessage");
    Ok(())
}

/// `/welcome`: the room selection page after registering.
pub fn welcome(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let tpl = conn.template("welcome");
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Err(crate::web::WebError("welcome: no user".into()));
    };
    if crate::web::data::active_ban(conn.world(), &user).is_some() {
        conn.redirect("/account/banned");
        return Ok(());
    }
    if !crate::web::data::can_select_room(conn.world(), &user) {
        conn.redirect("/me");
        return Ok(());
    }
    conn.session().set("page", "welcome");
    conn.render(tpl)
}

/// `/account/reauthenticate`: the password again before the client opens.
pub fn reauthenticate(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    if !conn.post.is_empty() {
        let name = conn
            .world()
            .store
            .user(conn.user_id())?
            .map(|u| u.username)
            .unwrap_or_default();
        let password = conn.post.string("password");
        if login(conn, &name, &password, false) {
            let target = conn
                .session()
                .string("clientRequest")
                .unwrap_or_else(|| "/me".into());
            conn.redirect(&target);
            return Ok(());
        }
    }
    conn.session().set("page", "reauthenticate");
    let tpl = conn.template("account/reauthenticate");
    conn.render(tpl)?;
    conn.session().delete("alertMessage");
    Ok(())
}

/// `/account/banned`.
pub fn banned(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    conn.session().delete("lastBrowsedPage");
    let mut tpl = conn.template("account/banned");
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Err(crate::web::WebError("banned: no user".into()));
    };
    let Some(ban) = crate::web::data::active_ban(conn.world(), &user) else {
        conn.redirect("/me");
        return Ok(());
    };
    conn.session().set("page", "banned");
    let site_name = crate::web::data::cfg_string(conn.world(), "site.name");
    tpl.set(
        "bannedMsg",
        format!(
            "You have been banned from {site_name}. The reason for the ban is \"{}\". The ban will expire at {}.",
            ban.message,
            crate::web::jdate::format(ban.banned_until, crate::web::jdate::LONG_DATE)
        ),
    );
    conn.render(tpl)?;
    if conn.cookie(super::MACHINE_ID_COOKIE) != Some(user.machine_id.as_str())
        && !user.machine_id.trim().is_empty()
    {
        conn.set_cookie(
            super::MACHINE_ID_COOKIE,
            &user.machine_id.replace('#', ""),
            Some(2 * 86_400),
        );
    }
    logout_session(conn);
    Ok(())
}
