//! Java: `controllers/site/RegisterController`, `util/RegisterUtil`, `util/Captcha`.

use super::{MACHINE_ID_COOKIE, remove_html_tags};
use crate::web::{Conn, Resp, WebResult, beans, captcha, data};

/// Java: `ValidationUtil.isValidEmail`.
pub fn valid_email_syntax(email: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$").unwrap()
    });
    !email.is_empty() && re.is_match(email)
}

/// Java: `RegisterUtil.isValidEmail`: well formed and not used by another account.
pub fn valid_email(conn: &Conn, email: &str) -> bool {
    valid_email_syntax(email)
        && !conn
            .world()
            .store
            .users()
            .unwrap_or_default()
            .iter()
            .any(|u| u.email == email)
}

/// Java: `NameCheckController.hasAllowedCharacters`.
pub fn allowed_characters(s: &str, allowed: &str) -> bool {
    s.chars().all(|c| allowed.contains(c))
}

/// Java: `RegisterUtil.getNameErrorCode` (0 when the name is free and allowed).
pub fn name_error(conn: &Conn, username: &str) -> i32 {
    let lower = username.to_lowercase();
    if crate::web::text::filter_sentence(conn.world(), username) != username {
        6
    } else if !allowed_characters(&lower, "1234567890qwertyuiopasdfghjklzxcvbnm-+=?!@:.,$") {
        5
    } else if ["admin", "mod", "staff", "moderator", "vip"].contains(&lower.as_str())
        || [
            "admin-", "admin=", "mod-", "mod=", "bot-", "bot=", "vip=", "vip-",
        ]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        4
    } else if username.encode_utf16().count() > 24 {
        3
    } else if username.is_empty() {
        2
    } else if conn
        .world()
        .store
        .user_by_name(username)
        .ok()
        .flatten()
        .is_some()
    {
        1
    } else {
        0
    }
}

/// `/register`.
pub fn register(conn: &mut Conn) -> WebResult {
    if conn.is_authenticated() {
        conn.redirect("/me");
        return Ok(());
    }
    let world = conn.web.world.clone();
    let max = data::cfg_int(&world, "max.connections.per.ip");
    let ip = conn.ip.clone();
    if world.store.users_with_ip(&ip)? as i64 >= max {
        conn.session().set(
            "alertMessage",
            "You already have enough accounts registered",
        );
        conn.redirect("/");
        return Ok(());
    }
    if let Some(machine) = conn.cookie(MACHINE_ID_COOKIE).map(str::to_string) {
        let id = format!("#{machine}");
        let count = world
            .store
            .users()?
            .iter()
            .filter(|u| u.machine_id == id)
            .count() as i64;
        if count >= max {
            conn.session().set(
                "alertMessage",
                "You already have enough accounts registered",
            );
            conn.redirect("/");
            return Ok(());
        }
    }
    if data::cfg_bool(&world, "registration.disabled") {
        let tpl = conn.template("register_disabled");
        return conn.render(tpl);
    }
    let referral = if conn.get.contains("referral") {
        conn.get.int("referral").unwrap_or(0)
    } else {
        0
    };
    if referral > 0 {
        conn.session().set("referral", referral);
    }

    if conn.post.len() > 3 {
        for field in [
            "bean.avatarName",
            "bean.captchaResponse",
            "retypedPassword",
            "bean.email",
        ] {
            if conn.post.contains(field) && conn.post.string(field).trim().is_empty() {
                conn.session().set("captcha.invalid", false);
                conn.redirect("/register?errorCode=blank_fields");
                break;
            }
        }
        let mut username = String::new();
        let mut email = String::new();
        if conn.post.contains("registerUsername") {
            username = remove_html_tags(
                &conn
                    .session()
                    .string("registerUsername")
                    .unwrap_or_default(),
            );
        }
        if conn.post.contains("registerEmail") {
            email = remove_html_tags(&conn.session().string("registerEmail").unwrap_or_default());
        }
        if conn.post.contains("bean.avatarName") {
            username = remove_html_tags(&conn.post.string("bean.avatarName"));
        }
        if conn.post.contains("bean.email") {
            email = remove_html_tags(&conn.post.string("bean.email"));
        }
        if conn.post.len() > 10 {
            let password = remove_html_tags(&conn.post.string("retypedPassword"));
            let day = remove_html_tags(&conn.post.string("bean.day"));
            let month = remove_html_tags(&conn.post.string("bean.month"));
            let year = remove_html_tags(&conn.post.string("bean.year"));
            let (figure, gender) = if conn.post.contains("randomFigure") {
                let temp = remove_html_tags(&conn.post.string("randomFigure"));
                // Java: substring(2) and substring(0, 1) throw on short input.
                if temp.chars().count() < 2 {
                    return Err(crate::web::WebError("randomFigure too short".into()));
                }
                (
                    temp.chars().skip(2).collect::<String>(),
                    temp.chars().take(1).collect::<String>(),
                )
            } else {
                (
                    remove_html_tags(&conn.post.string("bean.figure")),
                    remove_html_tags(&conn.post.string("bean.gender")),
                )
            };
            {
                let mut s = conn.session();
                s.set("registerUsername", username.as_str());
                s.set("registerPassword", password.as_str());
                s.set("registerShowPassword", "*".repeat(password.chars().count()));
                s.set("registerFigure", figure.as_str());
                s.set("registerGender", gender.as_str());
                s.set("registerEmail", email.as_str());
                s.set("registerDay", day);
                s.set("registerMonth", month);
                s.set("registerYear", year);
            }
            if !conn.web.figures.validate(&figure, &gender, false) {
                conn.redirect("/register?error=bad_look");
                return Ok(());
            }
            if name_error(conn, &username) != 0 {
                conn.redirect("/register?error=bad_username");
                return Ok(());
            }
            if !valid_email(conn, &email) {
                conn.session().set("email.invalid", true);
                conn.redirect("/register?error=bad_email");
                return Ok(());
            }
        }
        let response = remove_html_tags(&conn.post.string("bean.captchaResponse"));
        if Some(response) != conn.session().string("captcha-text") {
            conn.session().set("captcha.invalid", true);
            conn.redirect("/register?error=bad_captcha");
            return Ok(());
        }
        if conn.post.contains("bean.email") {
            let email = remove_html_tags(&conn.post.string("bean.email"));
            conn.session().set("registerEmail", email);
        }
        let email = conn.session().string("registerEmail").unwrap_or_default();
        if !valid_email(conn, &email) {
            conn.session().set("email.invalid", true);
            conn.redirect("/register?error=bad_email");
            return Ok(());
        }
        let (name, password, figure, gender) = {
            let s = conn.session();
            (
                s.string("registerUsername").unwrap_or_default(),
                s.string("registerPassword").unwrap_or_default(),
                s.string("registerFigure").unwrap_or_default(),
                s.string("registerGender").unwrap_or_default(),
            )
        };
        let hashed = crate::game::password::hash(&password)?;
        let user_id = new_user(&world, &name, &hashed, &figure, &gender, &email)?;
        world
            .store
            .put_statistics(user_id, &rusbbo_storage::UserStatistics::new_row())?;
        if world.store.latest_ip(user_id)?.as_deref() != Some(ip.as_str()) {
            world.store.log_ip(user_id, &ip, beans::now())?;
        }
        let referral = conn.session().int("referral");
        if referral > 0 {
            let mut r = rusbbo_storage::Referral {
                user_id: referral as u32,
                referred_id: user_id,
                ..Default::default()
            };
            world.store.insert_record(&mut r)?;
        }
        {
            let mut s = conn.session();
            s.delete("referral");
            s.delete("captcha.invalid");
            s.set("user.id", user_id as i64);
            s.set("authenticated", true);
        }
        conn.redirect("/welcome");
        return Ok(());
    }

    let mut tpl = conn.template("register");
    let (captcha_invalid, email_invalid) = {
        let s = conn.session();
        (s.boolean("captcha.invalid"), s.boolean("email.invalid"))
    };
    if captcha_invalid || email_invalid {
        if captcha_invalid {
            tpl.set("registerCaptchaInvalid", true);
        }
        if email_invalid {
            tpl.set("registerEmailInvalid", true);
        }
        let s = conn.session();
        tpl.set("registerUsername", s.string("registerUsername"));
        // Java: getString("registerPassword").replaceAll(...) throws when it is missing.
        let Some(password) = s.string("registerPassword") else {
            drop(s);
            return Err(crate::web::WebError("registerPassword missing".into()));
        };
        tpl.set("registerShowPassword", "*".repeat(password.chars().count()));
        for k in [
            "registerFigure",
            "registerGender",
            "registerEmail",
            "registerDay",
            "registerMonth",
            "registerYear",
        ] {
            tpl.set(k, s.string(k));
        }
    }
    {
        use rand::RngExt;
        tpl.set("randomNum", rand::rng().random_range(0..10000));
    }
    for (k, g) in [
        ("randomFemaleFigure1", "F"),
        ("randomFemaleFigure2", "F"),
        ("randomFemaleFigure3", "F"),
        ("randomMaleFigure1", "M"),
        ("randomMaleFigure2", "M"),
        ("randomMaleFigure3", "M"),
    ] {
        tpl.set(k, conn.web.figures.random(g, false));
    }
    tpl.set("referral", conn.session().int("referral"));
    conn.render(tpl)
}

/// Java: `RegisterDao.newUser`: the new row with the table's defaults.
fn new_user(
    world: &crate::world::World,
    name: &str,
    password: &str,
    figure: &str,
    gender: &str,
    email: &str,
) -> anyhow::Result<u32> {
    let mut w = world.store.write()?;
    let id = w.next_user_id()?;
    let now = beans::now();
    let user = rusbbo_storage::User {
        id,
        username: name.to_string(),
        password: password.to_string(),
        figure: figure.to_string(),
        sex: gender.to_string(),
        email: email.to_string(),
        created_at: now,
        updated_at: now,
        last_online: now,
        ..rusbbo_storage::User::table_defaults()
    };
    w.put_user(&user)?;
    w.commit()?;
    Ok(id)
}

/// `/register/cancel`.
pub fn cancelled(conn: &mut Conn) -> WebResult {
    {
        let mut s = conn.session();
        s.delete("referral");
        s.delete("captcha.invalid");
    }
    conn.redirect("/");
    Ok(())
}

/// `/captcha.jpg` (a PNG, as in Java).
pub fn captcha_image(conn: &mut Conn) -> WebResult {
    let text = captcha::generate_text(7);
    conn.session().set("captcha-text", text.as_str());
    conn.send(Resp::typed(200, "image/png", captcha::image(&text)));
    Ok(())
}
