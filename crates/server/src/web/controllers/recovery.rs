//! Java: `controllers/site/RecoveryController`: password recovery and account activation by
//! e-mail. Every page needs `email.smtp.enable`; without it each one sends the visitor to the front
//! page.

use crate::web::{Conn, WebResult, data, email};

fn email_off(conn: &mut Conn) -> bool {
    if data::cfg_bool(conn.world(), "email.smtp.enable") {
        return false;
    }
    conn.redirect("/");
    true
}

/// Codes compare as the `utf8mb4_uca1400_ai_ci` columns do (case ignored).
fn same_code(stored: Option<&str>, given: &str) -> bool {
    stored.is_some_and(|s| s.to_lowercase() == given.to_lowercase())
}

/// Java: `EmailDao.getDetailsByEmail`: some user has this address.
fn email_exists(conn: &Conn, email: &str) -> bool {
    let key = fold(email);
    conn.world()
        .store
        .users()
        .unwrap_or_default()
        .iter()
        .any(|u| fold(&u.email) == key)
}

/// `username = ? AND email = ?` under the collation: case and accents ignored.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(rusbbo_storage::collation_fold)
        .collect()
}

/// `/account/password/forgot`.
pub fn forgot(conn: &mut Conn) -> WebResult {
    if email_off(conn) {
        return Ok(());
    }
    let mut tpl = conn.template("account/email/account_forgot");
    if !conn.post.is_empty() {
        if conn.post.contains("actionList") {
            let email = conn.post.string("ownerEmailAddress");
            if !conn.post.contains("ownerEmailAddress")
                || !super::register::valid_email_syntax(&email)
                || !email_exists(conn, &email)
            {
                tpl.set("invalidForgetName", true);
                return conn.render(tpl);
            }
            let tpl = conn.template("account/email/sent");
            return conn.render(tpl);
        }
        if conn.post.contains("actionForgot") {
            let username = conn.post.string("forgottenpw-username");
            let email = conn.post.string("forgottenpw-email");
            if !conn.post.contains("forgottenpw-username") || username.trim().is_empty() {
                tpl.set("invalidForgetPassword", true);
                return conn.render(tpl);
            }
            if !conn.post.contains("forgottenpw-email") || email.trim().is_empty() {
                tpl.set("invalidForgetPassword", true);
                return conn.render(tpl);
            }
            // Java: EmailDao.getDetails(username, email).
            let details = conn
                .world()
                .store
                .user_by_name(&username)?
                .filter(|u| fold(&u.email) == fold(&email));
            let Some(details) = details.filter(|_| super::register::valid_email_syntax(&email))
            else {
                tpl.set("invalidForgetPassword", true);
                return conn.render(tpl);
            };
            let code = uuid::Uuid::new_v4().to_string();
            let world = conn.web.world.clone();
            let mut codes = email::codes(&world, details.id);
            codes.forgot_password_code = Some(code.clone());
            codes.forgot_recovery_requested_time = Some(crate::web::beans::now());
            email::put_codes(&world, &codes)?;
            let html = email::render_recovery(conn.web, details.id, &details.username, &code)?;
            email::send(conn, &email, "Password recovery at Classic Habbo", html);
            let tpl = conn.template("account/email/sent");
            return conn.render(tpl);
        }
    }
    conn.session().set("page", "recover");
    conn.render(tpl)
}

/// Sets the session's alert, as the controllers do.
fn alert(conn: &mut Conn, message: &str, colour: &str) {
    let mut s = conn.session();
    s.set("alertMessage", message);
    s.set("alertColour", colour);
}

/// `/account/password/recovery`: the page a recovery mail links to (`?id=..&code=..`), which posts
/// the new password.
pub fn recovery(conn: &mut Conn) -> WebResult {
    if email_off(conn) {
        return Ok(());
    }
    // Java: get().getInt("id") in a try (0 otherwise); get().getString("code") is null if absent.
    let mut user_id = conn.get.int("id").unwrap_or(0);
    let mut code = conn.get.contains("code").then(|| conn.get.string("code"));
    let mut tpl = conn.template("account/email/account_recovery");
    if conn.post.contains("user_id") && conn.post.contains("recovery_code") {
        // Java: post().getInt, which throws on anything else.
        user_id = conn
            .post
            .int("user_id")
            .ok_or_else(|| crate::web::WebError("user_id is not a number".into()))?;
        code = Some(conn.post.string("recovery_code"));
    }
    let world = conn.web.world.clone();
    let valid = user_id != 0
        && code.as_deref().is_some_and(|c| {
            u32::try_from(user_id).is_ok_and(|id| {
                same_code(email::codes(&world, id).forgot_password_code.as_deref(), c)
            })
        });
    if !valid {
        alert(conn, "The recovery code was invalid", "red");
    } else {
        let id = user_id as u32;
        if conn.post.contains("password") && conn.post.contains("confirmpassword") {
            let password = conn.post.string("password");
            let confirm = conn.post.string("confirmpassword");
            if confirm != password {
                alert(conn, "The passwords don't match", "red");
            } else if confirm.chars().count() < 6 {
                alert(conn, "Password is too short, 6 characters minimum", "red");
            } else {
                alert(
                    conn,
                    "Your password has been changed successfully.",
                    "green",
                );
                let hashed = crate::game::password::hash(&confirm)?;
                world.store.update_user(id, |u| u.password = hashed)?;
                let mut codes = email::codes(&world, id);
                codes.forgot_password_code = None;
                codes.forgot_recovery_requested_time = None;
                email::put_codes(&world, &codes)?;
            }
        }
        tpl.set("recoveryCode", code.unwrap_or_default());
        tpl.set("userId", user_id);
    }
    conn.render(tpl)?;
    let mut s = conn.session();
    s.delete("alertMessage");
    s.delete("alertColour");
    Ok(())
}

/// `/account/activate`: the link of an activation mail (`?id=..&code=..`).
pub fn activate(conn: &mut Conn) -> WebResult {
    if email_off(conn) {
        return Ok(());
    }
    let user_id = conn.get.int("id").unwrap_or(0);
    let code = conn.get.contains("code").then(|| conn.get.string("code"));
    let mut tpl = conn.template("account/email/account_activated");
    tpl.set("verifySuccess", true);
    let world = conn.web.world.clone();
    match (u32::try_from(user_id).ok().filter(|&id| id != 0), code) {
        (Some(id), Some(code)) => {
            let mut codes = email::codes(&world, id);
            if same_code(codes.activation_code.as_deref(), &code) {
                // Java: EmailDao.activate (activation_code = NULL).
                codes.activation_code = None;
                email::put_codes(&world, &codes)?;
            } else {
                tpl.set("verifySuccess", false);
            }
        }
        _ => tpl.set("verifySuccess", false),
    }
    conn.render(tpl)
}
