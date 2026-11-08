//! Java: `controllers/site/ProfileController` (account settings) and `FriendManagementController`.

use rusbbo_pebble::{Map, Value};

use super::account::logout_session;
use super::xss_clear;
use crate::web::tpl::Tpl;
use crate::web::{Conn, Resp, WebResult, data, model};

fn numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn random_number() -> i64 {
    use rand::RngExt;
    rand::rng().random_range(0..i32::MAX as i64)
}

/// Java: `HtmlUtil.createFigureLink`.
fn figure_link(conn: &Conn, figure: &str) -> String {
    format!(
        "{}/habbo-imaging/avatarimage?figure={figure}&size=s&direction=4&head_direction=4&crr=0&gesture=sml&frame=1",
        data::cfg_string(conn.world(), "site.path")
    )
}

/// `/profile?tab=n`.
pub fn profile(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    conn.session().set("page", "me");
    let mut tab = 0;
    let value = conn.get.string("tab");
    if conn.get.contains("tab") && numeric(&value) {
        tab = value.parse().unwrap_or(0);
    }
    let mut tpl = match tab {
        2 => {
            let mut t = conn.template("profile/change_preferences");
            preferences(conn, &mut t)?;
            t
        }
        3 => conn.template("profile/change_email"),
        4 => conn.template("profile/change_password"),
        5 => {
            let mut t = conn.template("profile/friend_management");
            friend_management(conn, &mut t, 30, 1, -1, None)?;
            t
        }
        6 => {
            let mut t = conn.template("profile/change_trade_settings");
            t.set(
                "canUseTrade",
                !data::cfg_bool(conn.world(), "trade.email.verification")
                    || data::is_activated(conn.world()),
            );
            t
        }
        _ => {
            // Java: profile_flash, which renders the page itself (and the outer render renders it
            // again over the same response).
            let mut t = conn.template("profile/change_looks");
            change_looks(conn, &mut t)?;
            // profile_flash renders and then clears these, so the page Java finally sends shows
            // neither the "saved" notice nor the alert.
            let mut s = conn.session();
            s.delete("settings.saved.successfully");
            s.delete("alertMessage");
            s.delete("alertColour");
            drop(s);
            t
        }
    };
    tpl.set("settingsSavedAlert", false);
    tpl.set("accountActivated", data::is_activated(conn.world()));
    if conn.session().contains("settings.saved.successfully") {
        tpl.set("settingsSavedAlert", true);
    }
    tpl.set("randomNumber", random_number());
    conn.render(tpl)?;
    let mut s = conn.session();
    s.delete("settings.saved.successfully");
    s.delete("alertMessage");
    s.delete("alertColour");
    Ok(())
}

/// Java: `ProfileController.changelooks`: the wardrobe and whether the figure needs club.
fn change_looks(conn: &mut Conn, tpl: &mut Tpl) -> WebResult {
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Ok(());
    };
    let wardrobe = conn
        .world()
        .store
        .records_where::<rusbbo_storage::Wardrobe>(|w| w.user_id == user.id)
        .unwrap_or_default();
    for slot in 1..=5 {
        tpl.set(&format!("wardrobe{slot}"), false);
    }
    for slot in 1..=5 {
        if let Some(w) = wardrobe.iter().find(|w| w.slot_id == slot) {
            tpl.set(&format!("wardrobe{slot}"), true);
            tpl.set(&format!("wardrobeUrl{slot}"), figure_link(conn, &w.figure));
            tpl.set(&format!("wardrobeFigure{slot}"), w.figure.as_str());
            tpl.set(&format!("wardrobeSex{slot}"), w.sex.as_str());
        }
    }
    let needs_club = !crate::web::beans::has_club(&user)
        && conn
            .web
            .figures
            .validate_code(&user.figure, &user.sex, false)
            == 6;
    tpl.set("figureHasClub", needs_club);
    Ok(())
}

/// Java: `ProfileController.preferences`.
fn preferences(conn: &mut Conn, tpl: &mut Tpl) -> WebResult {
    let Some(u) = conn.world().store.user(conn.user_id())? else {
        return Ok(());
    };
    let checked = "checked=\"checked\"";
    for k in [
        "onlineStatusEnabled",
        "onlineStatusDisabled",
        "followFriendEnabled",
        "followFriendDisabled",
        "profileVisibleEnabled",
        "profileVisibleDisabled",
        "allowFriendRequests",
        "wordFilterSetting",
    ] {
        tpl.set(k, "");
    }
    tpl.set(
        if u.online_status_visible {
            "onlineStatusEnabled"
        } else {
            "onlineStatusDisabled"
        },
        checked,
    );
    tpl.set(
        if u.allow_stalking {
            "followFriendEnabled"
        } else {
            "followFriendDisabled"
        },
        checked,
    );
    tpl.set(
        if u.profile_visible {
            "profileVisibleEnabled"
        } else {
            "profileVisibleDisabled"
        },
        checked,
    );
    tpl.set(
        "allowFriendRequests",
        if u.allow_friend_requests {
            "checked=\"true\""
        } else {
            ""
        },
    );
    tpl.set(
        "wordFilterSetting",
        if u.wordfilter_enabled {
            ""
        } else {
            "checked=\"true\""
        },
    );
    Ok(())
}

fn alert(conn: &mut Conn, message: &str, colour: &str) {
    let mut s = conn.session();
    s.set("alertMessage", message);
    s.set("alertColour", colour);
}

fn password_ok(conn: &Conn, name: &str, password: &str) -> bool {
    crate::game::player::check_login(conn.world(), name, password).is_some()
}

/// `/profile/passwordupdate`.
pub fn password_update(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("profile/change_password");
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Ok(());
    };
    let mut logout = false;
    let (current, new, confirm, captcha) = (
        conn.post.string("currentpassword"),
        conn.post.string("newpassword"),
        conn.post.string("newpasswordconfirm"),
        conn.post.string("captcha"),
    );
    if current.trim().is_empty()
        || new.trim().is_empty()
        || confirm.trim().is_empty()
        || captcha.trim().is_empty()
    {
        alert(conn, "Please enter all fields", "red");
    } else if !password_ok(conn, &user.username, &current) {
        alert(conn, "Your current password is invalid", "red");
    } else if new.encode_utf16().count() < 6 {
        alert(conn, "Password is too short, 6 characters minimum", "red");
    } else if new != confirm {
        alert(conn, "The passwords don't match", "red");
    } else {
        // Java: getString("captcha-text").equals(captcha) throws without a captcha.
        let Some(expected) = conn.session().string("captcha-text") else {
            return Err(crate::web::WebError(
                "no captcha-text in the session".into(),
            ));
        };
        if expected != captcha {
            alert(
                conn,
                "The security code was invalid, please try again.",
                "red",
            );
        } else {
            alert(
                conn,
                "Your password has been changed successfully. You will need to login again.",
                "green",
            );
            let hashed = crate::game::password::hash(&new)?;
            conn.world()
                .store
                .update_user(user.id, |u| u.password = hashed)?;
            logout = true;
        }
    }
    tpl.set("randomNumber", random_number());
    conn.render(tpl)?;
    {
        let mut s = conn.session();
        s.delete("alertMessage");
        s.delete("alertColour");
        s.delete("captcha-text");
    }
    if logout {
        logout_session(conn);
    }
    Ok(())
}

/// `/profile/emailupdate`. The new address is only saved after a sent activation mail (Java),
/// so without SMTP the address never changes.
pub fn email_update(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Ok(());
    };
    let (password, email, captcha) = (
        conn.post.string("password"),
        conn.post.string("email"),
        conn.post.string("captcha"),
    );
    if password.trim().is_empty() || captcha.trim().is_empty() {
        alert(conn, "Please enter all fields", "red");
    } else if !password_ok(conn, &user.username, &password) {
        alert(conn, "Your current password is invalid", "red");
    } else if !super::register::valid_email_syntax(&email) {
        alert(conn, "The email you entered is invalid", "red");
    } else {
        let Some(expected) = conn.session().string("captcha-text") else {
            return Err(crate::web::WebError(
                "no captcha-text in the session".into(),
            ));
        };
        if expected != captcha {
            alert(
                conn,
                "The security code was invalid, please try again.",
                "red",
            );
        } else {
            alert(conn, "Your email has been changed successfully.", "green");
            // TODO(email): with SMTP on, Java mails an activation code and only then saves the
            // address (and turns trading off with trade.email.verification).
        }
    }
    conn.redirect("/profile?tab=3");
    conn.session().delete("captcha-text");
    Ok(())
}

/// `/profile/characterupdate`.
pub fn character_update(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    for field in ["figureData", "newGender"] {
        if !conn.post.contains(field) || conn.post.array(field).is_empty() {
            conn.redirect("/profile");
            return Ok(());
        }
    }
    let (figure, gender) = (
        conn.post.string("figureData"),
        conn.post.string("newGender"),
    );
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Ok(());
    };
    if conn
        .web
        .figures
        .validate_code(&figure, &gender, crate::web::beans::has_club(&user))
        > 0
    {
        conn.redirect("/profile");
        return Ok(());
    }
    // Java: charAt(0) throws on an empty gender.
    let first = gender
        .chars()
        .next()
        .ok_or_else(|| crate::web::WebError("empty gender".into()))?;
    if first != 'M' && first != 'F' {
        conn.redirect("/profile");
        return Ok(());
    }
    conn.world().store.update_user(user.id, |u| {
        u.figure = figure.clone();
        u.sex = gender.clone();
    })?;
    if user.is_online || conn.world().player_by_id(user.id).is_some() {
        crate::rcon::send(
            conn.world(),
            "refresh_looks",
            &[("userId", user.id.to_string())],
        );
    }
    conn.session().set("settings.saved.successfully", "");
    conn.redirect("/profile");
    Ok(())
}

/// `/profile/profile.action`.
pub fn action(conn: &mut Conn) -> WebResult {
    conn.redirect("/profile");
    Ok(())
}

/// `/profile/profileupdate`.
pub fn profile_update(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut motto = conn.post.string("motto");
    let visible = conn.post.string("visibility") == "EVERYONE";
    let online = conn.post.string("showOnlineStatus") == "true";
    let wordfilter = conn.post.string("wordFilterSetting") != "false";
    let requests = conn.post.string("allowFriendRequests") == "true";
    let stalking = conn.post.string("followFriendSetting") == "true";
    if motto.encode_utf16().count() > 32 {
        motto = motto.chars().take(32).collect();
    }
    conn.world().store.update_user(conn.user_id(), |u| {
        u.motto = motto;
        u.profile_visible = visible;
        u.online_status_visible = online;
        u.wordfilter_enabled = wordfilter;
        u.allow_friend_requests = requests;
        u.allow_stalking = stalking;
    })?;
    conn.session().set("settings.saved.successfully", "true");
    conn.redirect("/profile?tab=2");
    Ok(())
}

/// `/profile/securitysettingupdate`: the trade setting.
pub fn security_update(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Ok(());
    };
    let password = conn.post.string("password");
    if password.is_empty() {
        alert(conn, "You did not enter a password", "red");
    } else if !password_ok(conn, &user.username, &password) {
        alert(conn, "Your current password is invalid", "red");
    } else if data::cfg_bool(conn.world(), "trade.email.verification")
        && !data::is_activated(conn.world())
    {
        alert(
            conn,
            "You must verify your email before enabling trade.",
            "red",
        );
    } else if data::has_trade_pass(conn.world(), user.id, &user.email) {
        alert(conn, "This email is already used for a trade pass.", "red");
    } else {
        alert(conn, "Security settings updated successfully", "green");
        let on = conn.post.string("tradingsetting") == "true";
        conn.world()
            .store
            .update_user(user.id, |u| u.trade_enabled = on)?;
        crate::rcon::send(
            conn.world(),
            "refreshtrade",
            &[
                ("userId", user.id.to_string()),
                ("tradeEnabled", if on { "1" } else { "0" }.to_string()),
            ],
        );
    }
    conn.redirect("/profile?tab=6");
    Ok(())
}

/// `/profile/wardrobeStore`.
pub fn wardrobe_store(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    if !numeric(&conn.post.string("slot")) {
        conn.redirect("/");
        return Ok(());
    }
    let user_id = conn.user_id();
    let slot: i32 = conn.post.string("slot").parse()?;
    let figure = crate::web::text::filter_input(&conn.post.string("figure"), true);
    let mut sex = crate::web::text::filter_input(&conn.post.string("gender"), true);
    if sex.trim().is_empty() {
        sex = "M".into();
    }
    let Some(user) = conn.world().store.user(user_id)? else {
        return Ok(());
    };
    if !conn
        .web
        .figures
        .validate(&figure, &sex, crate::web::beans::has_club(&user))
        || !(1..=5).contains(&slot)
    {
        conn.redirect("/");
        return Ok(());
    }
    let existing = conn
        .world()
        .store
        .records_where::<rusbbo_storage::Wardrobe>(|w| w.user_id == user_id && w.slot_id == slot)?;
    match existing.into_iter().next() {
        Some(mut w) => {
            w.figure = figure.clone();
            w.sex = sex.to_uppercase();
            conn.world().store.put_record(&w)?;
        }
        None => {
            let mut w = rusbbo_storage::Wardrobe {
                user_id,
                slot_id: slot,
                sex: sex.to_uppercase(),
                figure: figure.clone(),
                ..Default::default()
            };
            conn.world().store.insert_record(&mut w)?;
        }
    }
    let json = format!(
        "{{\"slot\":\"{slot}\",\"u\":\"{}\",\"f\":\"{figure}\",\"g\":77}}",
        figure_link(conn, &figure)
    );
    conn.send(Resp::typed(200, "application/json", json.into_bytes()));
    Ok(())
}

/// `/profile/verify`.
pub fn verify(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    conn.session().set("page", "me");
    let mut tpl = conn.template("profile/verify_email");
    tpl.set("accountActivated", data::is_activated(conn.world()));
    conn.render(tpl)?;
    let mut s = conn.session();
    s.delete("alertMessage");
    s.delete("alertColour");
    Ok(())
}

/// `/profile/send_email`.
pub fn send_email(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    if data::is_activated(conn.world()) {
        alert(conn, "Your email is already activated", "red");
        conn.redirect("/profile/verify");
        return Ok(());
    }
    // TODO(email): a new activation code and its mail.
    conn.redirect("/profile/verify");
    Ok(())
}

/// Java: `FriendManagementController.friendmanagement`.
pub fn friend_management(
    conn: &mut Conn,
    tpl: &mut Tpl,
    limit: i64,
    page: i64,
    category: i64,
    search: Option<&str>,
) -> WebResult {
    xss_clear(conn);
    let user_id = conn.user_id();
    let world = conn.web.world.clone();
    let mut all: Vec<(rusbbo_storage::User, i32)> = world
        .store
        .messenger_friends(user_id)?
        .into_iter()
        .filter_map(|(id, cat)| world.store.user(id).ok().flatten().map(|u| (u, cat)))
        .filter(|(u, _)| {
            search.is_none_or(|q| u.username.to_lowercase().starts_with(&q.to_lowercase()))
        })
        .collect();
    let count = all.len() as i64;
    // ORDER BY last_online DESC LIMIT (page - 1) * limit, limit; then sorted again (stable).
    all.sort_by_key(|(u, _)| std::cmp::Reverse(u.last_online));
    let start = ((page - 1).max(0) * limit) as usize;
    let mut friends: Vec<(rusbbo_storage::User, i32)> = all
        .into_iter()
        .skip(start)
        .take(limit.max(0) as usize)
        .collect();
    let pages = if count > 0 {
        (count + limit - 1) / limit
    } else {
        1
    }
    .max(1);
    let mut categories = world.store.messenger_categories(user_id)?;
    categories.sort_by_key(|c| c.id);
    for (u, cat) in friends.iter_mut() {
        if !categories.iter().any(|c| c.id as i32 == *cat) {
            *cat = 0;
            let mut w = world.store.write()?;
            w.messenger_set_friend_category(user_id, u.id, 0)?;
            w.commit()?;
        }
    }
    if category > -1 {
        friends.retain(|(_, c)| *c as i64 == category);
    }
    tpl.set(
        "friends",
        Value::list(
            friends
                .iter()
                .map(|(u, c)| model::messenger_user(u, *c))
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set("categories", categories_value(&categories));
    tpl.set("currentPage", page);
    tpl.set("pageLimit", limit);
    tpl.set("firstPage", if page >= 2 { 1 } else { -1 });
    tpl.set("previousPage", if page > 1 { page - 1 } else { -1 });
    tpl.set("nextPage", if pages > page { page + 1 } else { -1 });
    tpl.set("lastPage", if pages >= page + 2 { pages } else { -1 });
    Ok(())
}

fn categories_value(categories: &[rusbbo_storage::MessengerCategory]) -> Value {
    Value::list(
        categories
            .iter()
            .map(|c| {
                Value::from(
                    Map::new()
                        .with("id", c.id)
                        .with("userId", c.user_id)
                        .with("name", c.name.as_str()),
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn category_widget(conn: &mut Conn) -> WebResult {
    let user_id = conn.user_id();
    crate::rcon::send(
        conn.world(),
        "refreshmessengercategories",
        &[("userId", user_id.to_string())],
    );
    let mut tpl = conn.template("profile/profile_widgets/friend_category_widget");
    let mut categories = conn.world().store.messenger_categories(user_id)?;
    categories.sort_by_key(|c| c.id);
    tpl.set("categories", categories_value(&categories));
    conn.render(tpl)
}

/// `/friendmanagement/ajax/editCategory`.
pub fn edit_category(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let name = conn.post.string("name");
    let id = conn
        .post
        .int("categoryId")
        .ok_or_else(|| crate::web::WebError("categoryId".into()))?;
    if !name.trim().is_empty() {
        let mut w = conn.world().store.write()?;
        w.messenger_rename_category(id.max(0) as u32, conn.user_id(), &name)?;
        w.commit()?;
    }
    category_widget(conn)
}

/// `/friendmanagement/ajax/createcategory`.
pub fn create_category(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut name = conn.post.string("name");
    if !name.trim().is_empty() {
        if name.encode_utf16().count() > 50 {
            name = name.chars().take(50).collect();
        }
        let mut w = conn.world().store.write()?;
        w.messenger_add_category(conn.user_id(), &name)?;
        w.commit()?;
    }
    category_widget(conn)
}

/// `/friendmanagement/ajax/deletecategory`.
pub fn delete_category(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let id = conn
        .post
        .int("categoryId")
        .ok_or_else(|| crate::web::WebError("categoryId".into()))?;
    let user_id = conn.user_id();
    {
        let mut w = conn.world().store.write()?;
        w.messenger_delete_category(id.max(0) as u32, user_id)?;
        w.commit()?;
    }
    crate::rcon::send(
        conn.world(),
        "refreshmessengercategories",
        &[("userId", user_id.to_string())],
    );
    let mut tpl = conn.template("profile/profile_widgets/friend_category_widget");
    let mut categories = conn.world().store.messenger_categories(user_id)?;
    categories.sort_by_key(|c| c.id);
    {
        let mut w = conn.world().store.write()?;
        w.messenger_reset_friend_categories(user_id, id)?;
        w.commit()?;
    }
    tpl.set("categories", categories_value(&categories));
    conn.render(tpl)
}

/// `/friendmanagement/ajax/viewcategory`.
pub fn view_category(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let int = |q: &crate::web::http::Query, k: &str| {
        q.int(k)
            .ok_or_else(|| crate::web::WebError(format!("{k} is not a number")))
    };
    let mut page = if conn.get.contains("pageNumber") {
        int(&conn.get, "pageNumber")?
    } else {
        1
    } as i64;
    let mut size = int(&conn.get, "pageSize")? as i64;
    let category = if conn.get.contains("categoryId") {
        int(&conn.get, "categoryId")?
    } else {
        -1
    } as i64;
    let mut search = conn
        .get
        .contains("searchString")
        .then(|| conn.get.string("searchString"));
    if conn.get.is_empty() {
        size = int(&conn.post, "pageSize")? as i64;
        search = conn
            .post
            .contains("searchString")
            .then(|| conn.post.string("searchString"));
    }
    if size > 100 || size <= 0 {
        size = 30;
    }
    if page <= 0 {
        page = 1;
    }
    let mut tpl = conn.template("profile/profile_widgets/friend_view_category");
    friend_management(conn, &mut tpl, size, page, category, search.as_deref())?;
    conn.render(tpl)
}

/// `/friendmanagement/ajax/updatecategoryoptions`.
pub fn update_category_options(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("profile/profile_widgets/friend_category_options");
    let categories = conn.world().store.messenger_categories(conn.user_id())?;
    tpl.set("categories", categories_value(&categories));
    conn.render(tpl)
}

/// `/friendmanagement/ajax/movefriends`.
pub fn move_friends(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut size = conn
        .post
        .int("pageSize")
        .ok_or_else(|| crate::web::WebError("pageSize".into()))? as i64;
    let category = if conn.post.contains("moveCategoryId") {
        conn.post
            .int("moveCategoryId")
            .ok_or_else(|| crate::web::WebError("moveCategoryId".into()))?
    } else {
        -1
    };
    if size > 100 || size <= 0 {
        size = 30;
    }
    let mut tpl = conn.template("profile/profile_widgets/friend_view_category");
    let user_id = conn.user_id();
    if conn.post.contains("friendList[]") {
        let mut w = conn.world().store.write()?;
        for v in conn.post.array("friendList[]") {
            if let Ok(id) = v.parse::<u32>() {
                w.messenger_set_friend_category(user_id, id, category)?;
            }
        }
        w.commit()?;
    }
    crate::rcon::send(
        conn.world(),
        "refreshmessengercategories",
        &[("userId", user_id.to_string())],
    );
    friend_management(conn, &mut tpl, size, 1, category as i64, None)?;
    conn.render(tpl)
}

/// `/friendmanagement/ajax/deletefriends`.
pub fn delete_friends(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("profile/profile_widgets/friend_view_category");
    let user_id = conn.user_id();
    let mut ids: Vec<u32> = Vec::new();
    if conn.post.contains("friendList[]") {
        for v in conn.post.array("friendList[]") {
            // Java: Integer.parseInt throws on junk.
            ids.push(v.parse()?);
        }
    }
    if conn.post.contains("friendId") {
        ids.push(
            conn.post
                .int("friendId")
                .ok_or_else(|| crate::web::WebError("friendId".into()))?
                .max(0) as u32,
        );
    }
    for id in ids {
        conn.world().store.messenger_remove_friend(user_id, id)?;
        conn.world().store.messenger_remove_friend(id, user_id)?;
    }
    crate::rcon::send(
        conn.world(),
        "refreshmessengercategories",
        &[("userId", user_id.to_string())],
    );
    friend_management(conn, &mut tpl, 30, 1, -1, None)?;
    conn.render(tpl)
}
