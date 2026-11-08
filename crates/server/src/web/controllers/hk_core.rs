//! Java (Aeyra's panel): `HousekeepingController` (login, logout, the dashboard),
//! `HousekeepingCommandsController`, `HousekeepingBansController`,
//! `HousekeepingTransactionsController` and `HousekeepingConfigController`.

use std::collections::BTreeMap;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{BanType, User};

use crate::web::beans::now;
use crate::web::hk::{self, Pager, row_date};
use crate::web::{Conn, WebResult};
use crate::world::World;

fn user_name(world: &World, id: i64) -> Option<String> {
    u32::try_from(id)
        .ok()
        .and_then(|id| world.store.user(id).ok().flatten())
        .map(|u| u.username)
}

/// `WebCache.setting`: "0" for a missing or blank value.
fn setting(world: &World, key: &str) -> String {
    let s = world.settings.read();
    let v = s.string(key).trim().to_string();
    if v.is_empty() { "0".into() } else { v }
}

/// `/{hk}` (and the login page when the panel session is not open).
pub fn dashboard(conn: &mut Conn) -> WebResult {
    if !conn.session().boolean(hk::LOGGED_IN) {
        let tpl = conn.template("housekeeping/login");
        return hk::render(conn, tpl);
    }
    if !hk::guard(conn, Some("root/login")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let now = now();
    let users: Vec<User> = world.store.users()?;
    let rooms = world.store.all_rooms()?;
    let items = world.store.all_items()?;
    let transactions = world.store.all_transactions()?;
    let chat = world.store.all_chat_logs()?;
    let day = now - 86400;
    let week = now - 7 * 86400;
    let count = |f: &dyn Fn(&User) -> bool| users.iter().filter(|u| f(u)).count() as i64;
    let item_ids: std::collections::HashSet<u64> = items.iter().map(|i| i.id).collect();
    let bans = world.store.bans()?;
    let stats = Map::new()
        .with("users", users.len())
        .with("usersToday", count(&|u| u.created_at >= day))
        .with("usersWeek", count(&|u| u.created_at >= week))
        .with("online", count(&|u| u.is_online))
        .with(
            "activeToday",
            count(&|u| u.last_online >= day || u.is_online),
        )
        .with("rooms", rooms.iter().filter(|r| r.owner_id != 0).count())
        .with(
            "publicRooms",
            rooms.iter().filter(|r| r.owner_id == 0).count(),
        )
        .with(
            "occupiedRooms",
            rooms.iter().filter(|r| r.visitors_now > 0).count(),
        )
        .with("roomItems", items.iter().filter(|i| i.room_id > 0).count())
        .with(
            "inventoryItems",
            items.iter().filter(|i| i.room_id == 0).count(),
        )
        .with(
            "groups",
            world.store.records::<rusbbo_storage::Group>()?.len(),
        )
        .with(
            "pets",
            world
                .store
                .all_pets()?
                .iter()
                .filter(|p| item_ids.contains(&p.item_id))
                .count(),
        )
        // TODO(photos): the port does not store photos (items_photos) yet.
        .with("photos", 0)
        .with(
            "credits",
            users.iter().map(|u| u.credits as i64).sum::<i64>(),
        )
        .with("pixels", users.iter().map(|u| u.pixels as i64).sum::<i64>())
        .with(
            "tickets",
            users.iter().map(|u| u.tickets as i64).sum::<i64>(),
        )
        .with("clubMembers", count(&|u| u.club_expiration > now))
        .with(
            "spentWeek",
            transactions
                .iter()
                .filter(|(_, t)| t.created_at >= week)
                .map(|(_, t)| t.credit_cost as i64)
                .sum::<i64>(),
        )
        .with(
            "pixelsSpentWeek",
            transactions
                .iter()
                .filter(|(_, t)| t.created_at >= week)
                .map(|(_, t)| t.pixel_cost as i64)
                .sum::<i64>(),
        )
        .with(
            "purchasesToday",
            transactions
                .iter()
                .filter(|(_, t)| t.created_at >= day)
                .count(),
        )
        .with(
            "chatToday",
            chat.iter().filter(|c| c.timestamp >= day).count(),
        )
        .with(
            "activeBans",
            bans.iter()
                .filter(|b| b.is_active && b.banned_until > now)
                .count(),
        )
        .with("serverOnline", true)
        .with("playersOnline", setting(&world, "players.online"))
        .with("dailyPeak", setting(&world, "players.daily.peak"))
        .with("allTimePeak", setting(&world, "players.all.time.peak"))
        .with(
            "maintenance",
            setting(&world, "maintenance").eq_ignore_ascii_case("true"),
        )
        .with(
            "registrationDisabled",
            setting(&world, "registration.disabled").eq_ignore_ascii_case("true"),
        );
    let mut tpl = hk::template(conn, "housekeeping/dashboard");
    tpl.set("stats", stats);
    // Online users with the occupied room they visited last.
    let visits = world
        .store
        .records::<rusbbo_storage::RoomVisit>()
        .unwrap_or_default();
    let mut online: Vec<&User> = users.iter().filter(|u| u.is_online).collect();
    online.sort_by_key(|u| u.username.to_lowercase());
    let online_rows: Vec<Value> = online
        .iter()
        .take(100)
        .map(|u| {
            let room = visits
                .iter()
                .filter(|v| v.user_id == u.id)
                .filter_map(|v| {
                    rooms
                        .iter()
                        .find(|r| r.id == v.room_id && r.visitors_now > 0)
                        .map(|r| (v.visited_at, r))
                })
                .max_by_key(|(t, _)| *t)
                .map(|(_, r)| r);
            Value::from(
                Map::new()
                    .with("id", u.id)
                    .with("username", u.username.as_str())
                    .with("figure", u.figure.as_str())
                    .with("motto", u.motto.as_str())
                    .with("rank", u.rank)
                    .with("credits", u.credits)
                    .with("pixels", u.pixels)
                    .with("room_id", room.map_or(0, |r| r.id))
                    .with("room_name", room.map_or(String::new(), |r| r.name.clone())),
            )
        })
        .collect();
    tpl.set("onlineUsers", Value::list(online_rows));
    let mut active: Vec<&rusbbo_storage::Room> =
        rooms.iter().filter(|r| r.visitors_now > 0).collect();
    active.sort_by_key(|r| std::cmp::Reverse(r.visitors_now));
    tpl.set(
        "activeRooms",
        Value::list(
            active
                .iter()
                .take(20)
                .map(|r| {
                    Value::from(
                        Map::new()
                            .with("id", r.id)
                            .with("name", r.name.as_str())
                            .with("model", r.model.as_str())
                            .with("visitors_now", r.visitors_now)
                            .with("visitors_max", r.visitors_max)
                            .with("owner_id", r.owner_id.to_string())
                            .with(
                                "owner_name",
                                user_name(&world, r.owner_id as i64).unwrap_or_default(),
                            ),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut recent: Vec<&User> = users.iter().collect();
    recent.sort_by_key(|u| std::cmp::Reverse(u.created_at));
    tpl.set(
        "recentUsers",
        Value::list(
            recent
                .iter()
                .take(8)
                .map(|u| {
                    Value::from(
                        Map::new()
                            .with("id", u.id)
                            .with("username", u.username.as_str())
                            .with("figure", u.figure.as_str())
                            .with("rank", u.rank)
                            .with("credits", u.credits)
                            .with("created_at", row_date(u.created_at))
                            .with("last_online", row_date(u.last_online))
                            .with("is_online", u.is_online),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut recent_tx: Vec<&(u64, rusbbo_storage::Transaction)> = transactions.iter().collect();
    recent_tx.sort_by_key(|(_, t)| std::cmp::Reverse(t.created_at));
    tpl.set(
        "recentTransactions",
        Value::list(
            recent_tx
                .iter()
                .take(10)
                .map(|(_, t)| {
                    Value::from(
                        Map::new()
                            .with("user_id", t.user_id)
                            .with(
                                "username",
                                user_name(&world, t.user_id as i64).unwrap_or_else(|| "?".into()),
                            )
                            .with("description", t.description.as_str())
                            .with("amount", t.amount)
                            .with("credit_cost", t.credit_cost)
                            .with("pixel_cost", t.pixel_cost)
                            .with("created_at", row_date(t.created_at)),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut recent_chat: Vec<&rusbbo_storage::ChatLog> = chat.iter().collect();
    recent_chat.sort_by_key(|c| std::cmp::Reverse(c.id));
    tpl.set(
        "recentChat",
        Value::list(
            recent_chat
                .iter()
                .take(10)
                .map(|c| {
                    Value::from(
                        Map::new()
                            .with("user_id", c.user_id)
                            .with(
                                "username",
                                user_name(&world, c.user_id as i64).unwrap_or_else(|| "?".into()),
                            )
                            .with("room_id", c.room_id)
                            .with(
                                "room_name",
                                rooms
                                    .iter()
                                    .find(|r| r.id == c.room_id)
                                    .map_or(String::new(), |r| r.name.clone()),
                            )
                            .with("message", c.message.as_str())
                            .with("said_at", row_date(c.timestamp)),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set(
        "recentActions",
        Value::list(
            hk::audit_log(&world, None, None)
                .iter()
                .take(8)
                .map(hk::AuditRow::value)
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/login`.
pub fn login(conn: &mut Conn) -> WebResult {
    if !hk::is_post(conn) {
        conn.redirect(&hk::path(""));
        return Ok(());
    }
    let username = conn.post.string("hkusername").trim().to_string();
    let password = conn.post.string("hkpassword");
    if username.is_empty() || password.is_empty() {
        hk::flash(conn, "danger", "Introduce tu usuario y tu contrase\u{f1}a.");
        conn.redirect(&hk::path(""));
        return Ok(());
    }
    let Some(user) = crate::game::player::check_login(conn.world(), &username, &password) else {
        hk::flash(conn, "danger", "Usuario o contrase\u{f1}a incorrectos.");
        conn.redirect(&hk::path(""));
        return Ok(());
    };
    if !hk::can(Some(&user), "root/login") {
        hk::flash(
            conn,
            "warning",
            "Tu cuenta no tiene acceso al panel de administraci\u{f3}n.",
        );
        conn.redirect(&hk::path(""));
        return Ok(());
    }
    {
        let mut s = conn.session();
        s.set(hk::LOGGED_IN, true);
        s.set("user.id", user.id.to_string());
        s.delete(hk::TOKEN_SESSION_KEY);
    }
    hk::audit_as(
        conn.world(),
        user.id,
        "hk_login",
        "user",
        user.id as i64,
        &user.username,
        "Inicio de sesi\u{f3}n en el panel",
        &conn.ip.clone(),
    );
    conn.redirect(&hk::path(""));
    Ok(())
}

/// `/{hk}/logout`.
pub fn logout(conn: &mut Conn) -> WebResult {
    if conn.session().boolean(hk::LOGGED_IN) {
        hk::flash(conn, "success", "Sesi\u{f3}n cerrada.");
        let mut s = conn.session();
        s.set(hk::LOGGED_IN, false);
        s.delete(hk::TOKEN_SESSION_KEY);
    }
    conn.redirect(&hk::path(""));
    Ok(())
}

/// `/{hk}/bans/ban`: find a user to ban.
pub fn command_ban(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("bans")) {
        return Ok(());
    }
    let name = hk::str(conn, "username");
    match conn.world().store.user_by_name(&name)? {
        Some(u) if u.id > 0 => {
            conn.redirect(&hk::path(&format!(
                "/users/view?id={}&tab=moderation",
                u.id
            )));
        }
        _ => {
            hk::flash(conn, "danger", "Ese usuario no existe.");
            conn.redirect(&hk::path("/bans"));
        }
    }
    Ok(())
}

/// `/{hk}/bans`.
pub fn bans(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("bans")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let now = now();
    let view = if hk::str(conn, "view") == "history" {
        "history"
    } else {
        "active"
    };
    let all = world.store.bans()?;
    let active = |b: &rusbbo_storage::Ban| b.is_active && b.banned_until > now;
    let mut list: Vec<&rusbbo_storage::Ban> = all
        .iter()
        .filter(|b| active(b) == (view == "active"))
        .collect();
    list.sort_by_key(|b| std::cmp::Reverse(b.banned_at));
    let pager = Pager::new(conn, &hk::path("/bans"), list.len() as i64, 30);
    let users = world.store.users()?;
    let rows: Vec<Value> = pager
        .slice(&list)
        .iter()
        .map(|b| {
            let user = (b.ban_type == BanType::UserId)
                .then(|| b.banned_value.trim().parse::<u32>().ok())
                .flatten()
                .and_then(|id| users.iter().find(|u| u.id == id));
            let machine_users = if b.ban_type == BanType::MachineId {
                let mut names: Vec<&str> = users
                    .iter()
                    .filter(|u| u.machine_id == b.banned_value)
                    .map(|u| u.username.as_str())
                    .collect();
                names.sort_by_key(|n| n.to_lowercase());
                names.join(", ")
            } else {
                String::new()
            };
            Value::from(
                Map::new()
                    .with("ban_type", b.ban_type.name())
                    .with("banned_value", b.banned_value.as_str())
                    .with("message", b.message.as_str())
                    .with("banned_at", row_date(b.banned_at))
                    .with("banned_until", row_date(b.banned_until))
                    .with("is_active", active(b))
                    .with(
                        "banned_by",
                        user_name(&world, b.banned_by as i64).unwrap_or_default(),
                    )
                    .with("user_id", user.map_or(0, |u| u.id))
                    .with(
                        "username",
                        user.map_or(String::new(), |u| u.username.clone()),
                    )
                    .with("figure", user.map_or(String::new(), |u| u.figure.clone()))
                    .with("machine_users", machine_users),
            )
        })
        .collect();
    let mut tpl = hk::template(conn, "housekeeping/users_bans");
    tpl.set("view", view);
    tpl.set("pager", pager.value());
    tpl.set("activeCount", all.iter().filter(|b| active(b)).count());
    tpl.set("bans", Value::list(rows));
    hk::render(conn, tpl)
}

/// `/{hk}/bans/unban`.
pub fn unban(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "bans") {
        return Ok(());
    }
    let kind = hk::str(conn, "type");
    let value = hk::str(conn, "value");
    let Some(ban_type) = BanType::from_name(&kind) else {
        hk::flash(conn, "danger", "Tipo de baneo no v\u{e1}lido.");
        let to = hk::back(conn);
        conn.redirect(&to);
        return Ok(());
    };
    conn.world().store.remove_ban(ban_type, &value)?;
    let user = ban_type == BanType::UserId;
    let name = if user {
        user_name(conn.world(), value.trim().parse::<i64>().unwrap_or(-1)).unwrap_or(value.clone())
    } else {
        value.clone()
    };
    hk::audit(
        conn,
        "user_unban",
        if user { "user" } else { "machine" },
        if user {
            value.parse::<i32>().unwrap_or(0) as i64
        } else {
            0
        },
        &name,
        &format!("{} {value}", ban_type.name()),
    );
    hk::flash(conn, "success", &format!("Baneo retirado ({name})."));
    let to = hk::back(conn);
    conn.redirect(&to);
    Ok(())
}

/// `/{hk}/transaction/lookup`.
pub fn transactions(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("transaction/lookup")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut user = hk::str(conn, "user");
    if user.is_empty() {
        user = hk::str(conn, "searchQuery");
    }
    let text = hk::str(conn, "text");
    let item = hk::str(conn, "item");
    let days = hk::num(conn, "days", 30);
    let now = now();
    let mut list: Vec<(u64, rusbbo_storage::Transaction)> = world
        .store
        .all_transactions()?
        .into_iter()
        .filter(|(_, t)| {
            let name = user_name(&world, t.user_id as i64);
            (user.is_empty()
                || name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&user)))
                && (text.is_empty() || hk::like(&t.description, &text))
                && (item.is_empty() || t.item_id == item || t.item_id.split(',').any(|x| x == item))
                && (days <= 0 || t.created_at >= now - days * 86400)
        })
        .collect();
    list.sort_by_key(|(_, t)| std::cmp::Reverse(t.created_at));
    let pager = Pager::new(
        conn,
        &hk::path("/transaction/lookup"),
        list.len() as i64,
        40,
    );
    let rows: Vec<Value> = pager
        .slice(&list)
        .iter()
        .map(|(_, t)| {
            Value::from(
                Map::new()
                    .with("user_id", t.user_id)
                    .with(
                        "username",
                        user_name(&world, t.user_id as i64).unwrap_or_else(|| "?".into()),
                    )
                    .with("item_id", t.item_id.as_str())
                    .with("catalogue_id", t.catalogue_id.as_str())
                    .with("amount", t.amount)
                    .with("description", t.description.as_str())
                    .with("credit_cost", t.credit_cost)
                    .with("pixel_cost", t.pixel_cost)
                    .with("created_at", row_date(t.created_at))
                    .with("is_visible", t.is_visible),
            )
        })
        .collect();
    let totals = Map::new()
        .with(
            "credits",
            list.iter().map(|(_, t)| t.credit_cost as i64).sum::<i64>(),
        )
        .with(
            "pixels",
            list.iter().map(|(_, t)| t.pixel_cost as i64).sum::<i64>(),
        )
        .with("cnt", list.len());
    let mut tpl = hk::template(conn, "housekeeping/transaction_lookup");
    tpl.set("transactions", Value::list(rows));
    tpl.set("totals", totals);
    tpl.set("pager", pager.value());
    tpl.set("user", user);
    tpl.set("text", text);
    tpl.set("item", item);
    tpl.set("days", days);
    hk::render(conn, tpl)
}

/// `/{hk}/transaction/item_lookup`.
pub fn item_lookup(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("transaction/lookup")) {
        return Ok(());
    }
    let id = hk::num(conn, "id", 0);
    conn.redirect(&hk::path(&format!("/transaction/lookup?days=0&item={id}")));
    Ok(())
}

const IMPORTANT: [(&str, &str); 15] = [
    (
        "maintenance",
        "Modo mantenimiento de la web (true/false). El panel sigue accesible.",
    ),
    (
        "registration.disabled",
        "Cerrar el registro de cuentas nuevas (true/false).",
    ),
    ("site.name", "Nombre del hotel que se muestra en la web."),
    (
        "welcome.message.enabled",
        "Mostrar el mensaje de bienvenida al entrar (true/false).",
    ),
    (
        "welcome.message.content",
        "Mensaje de bienvenida (%username% = nombre).",
    ),
    (
        "credits.scheduler.amount",
        "Cr\u{e9}ditos que reciben los conectados en cada reparto.",
    ),
    (
        "credits.scheduler.interval",
        "Cada cu\u{e1}nto se reparten cr\u{e9}ditos (ver unidad).",
    ),
    (
        "credits.scheduler.timeunit",
        "Unidad del reparto de cr\u{e9}ditos (MINUTES, HOURS...).",
    ),
    ("daily.credits.amount", "Cr\u{e9}ditos diarios por jugar."),
    (
        "pixels.received.interval",
        "Cada cu\u{e1}nto se reciben p\u{ed}xeles.",
    ),
    (
        "wordfitler.enabled",
        "Activar el filtro de palabras (sic: la clave est\u{e1} mal escrita en Havana).",
    ),
    (
        "wordfilter.word.replacement",
        "Texto que sustituye a las palabras filtradas.",
    ),
    ("vouchers.enabled", "Permitir canjear vales."),
    (
        "afk.timer.seconds",
        "Segundos hasta que un usuario inactivo se queda AFK.",
    ),
    (
        "max.connections.per.ip",
        "Conexiones m\u{e1}ximas por IP (los agentes comparten IP).",
    ),
];

fn is_secret(key: &str) -> bool {
    let l = key.to_lowercase();
    ["password", "secret", "apikey", "api.key", "token"]
        .iter()
        .any(|w| l.contains(w))
}

fn abbreviate(v: &str) -> String {
    let units: Vec<u16> = v.encode_utf16().collect();
    if units.len() > 40 {
        format!("{}\u{2026}", String::from_utf16_lossy(&units[..40]))
    } else {
        v.to_string()
    }
}

/// `/{hk}/configurations`.
pub fn configurations(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("configuration")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let current: BTreeMap<String, String> = world.settings.read().all().clone();
    if hk::is_post(conn) {
        let mut changed: BTreeMap<String, String> = BTreeMap::new();
        for key in conn.post.keys() {
            let Some(name) = key.strip_prefix("cfg:") else {
                continue;
            };
            let value = conn.post.string(key);
            let Some(old) = current.get(name) else {
                continue;
            };
            if is_secret(name) && value.is_empty() {
                continue;
            }
            if &value != old {
                changed.insert(name.to_string(), value);
            }
        }
        if changed.is_empty() {
            hk::flash(conn, "info", "No hab\u{ed}a cambios que guardar.");
        } else {
            for (k, v) in &changed {
                world.store.put_setting(k, v)?;
                world.settings.write().set(k, v);
            }
            crate::rcon::send(&world, "refreshcataloguefrontpage", &[]);
            let names: Vec<String> = changed
                .iter()
                .map(|(k, v)| {
                    if is_secret(k) {
                        k.clone()
                    } else {
                        format!("{k}={}", abbreviate(v))
                    }
                })
                .collect();
            hk::audit(conn, "config_save", "config", 0, "", &names.join("; "));
            let keys: Vec<&str> = changed.keys().map(String::as_str).collect();
            hk::flash(
                conn,
                "success",
                &format!(
                    "Guardado {} ajuste(s): {}. Web y servidor de juego recargados.",
                    changed.len(),
                    keys.join(", ")
                ),
            );
        }
        let q = hk::str(conn, "q");
        let suffix = if q.is_empty() {
            String::new()
        } else {
            format!("?q={q}")
        };
        conn.redirect(&format!("{}{suffix}", hk::path("/configurations")));
        return Ok(());
    }
    let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut rows_by_key: BTreeMap<String, Value> = BTreeMap::new();
    for (key, value) in &current {
        let help = IMPORTANT
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, h)| h);
        let row: Value = Map::new()
            .with("key", key.as_str())
            .with("value", if is_secret(key) { "" } else { value.as_str() })
            .with("secret", is_secret(key))
            .with("hasValue", !value.is_empty())
            .with(
                "bool",
                value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false"),
            )
            .with("long", value.encode_utf16().count() > 70)
            .with("help", help)
            .with(
                "important",
                !help.is_empty() || IMPORTANT.iter().any(|(k, _)| k == key),
            )
            .into();
        let group = key.split_once('.').map_or("general", |(g, _)| g);
        groups
            .entry(group.to_string())
            .or_default()
            .push(row.clone());
        rows_by_key.insert(key.clone(), row);
    }
    let important: Vec<Value> = IMPORTANT
        .iter()
        .filter_map(|(k, _)| rows_by_key.get(*k).cloned())
        .collect();
    let mut groups_value = Map::new();
    for (g, rows) in groups {
        groups_value.insert(g, Value::list(rows));
    }
    let mut tpl = hk::template(conn, "housekeeping/configurations");
    tpl.set("groups", groups_value);
    tpl.set("important", Value::list(important));
    tpl.set("total", current.len());
    tpl.set("q", hk::str(conn, "q"));
    hk::render(conn, tpl)
}
