//! Java (Aeyra's panel): `HousekeepingToolsController`.

use rusbbo_pebble::Value;
use rusbbo_storage::{ChatLog, HkAudit, RoomEntryBadge, RoomVisit, User};

use super::hk_users::row;
use crate::web::beans::now;
use crate::web::hk::{self, Pager, row_date};
use crate::web::{Conn, WebResult};
use crate::world::World;

fn v(x: impl Into<Value>) -> Value {
    x.into()
}

fn name_of(world: &World, id: u32) -> Option<String> {
    world.store.user(id).ok().flatten().map(|u| u.username)
}

/// `/{hk}/hotel/alert`.
pub fn hotel_alert(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("hotel/alert")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if hk::is_post(conn) {
        let message = hk::str(conn, "message");
        let staff = hk::staff(conn);
        if message.is_empty() {
            hk::flash(conn, "danger", "Escribe el mensaje.");
        } else if message.encode_utf16().count() > 500 {
            hk::flash(
                conn,
                "danger",
                "El mensaje es demasiado largo (m\u{e1}ximo 500 caracteres).",
            );
        } else {
            let sender = if hk::flag(conn, "anonymous") {
                "Staff".to_string()
            } else {
                staff.map(|s| s.username).unwrap_or_default()
            };
            crate::rcon::send(
                &world,
                "hotel_alert",
                &[("sender", sender), ("message", message.clone())],
            );
            let online = world.store.users()?.iter().filter(|u| u.is_online).count();
            hk::audit(conn, "hotel_alert", "hotel", 0, "", &message);
            hk::flash(
                conn,
                "success",
                &format!(
                    "Alerta enviada a todo el hotel ({online} conectado{}).",
                    if online == 1 { "" } else { "s" }
                ),
            );
        }
        conn.redirect(&hk::path("/hotel/alert"));
        return Ok(());
    }
    let mut tpl = hk::template(conn, "housekeeping/hotel_alert");
    let mut online: Vec<User> = world
        .store
        .users()?
        .into_iter()
        .filter(|u| u.is_online)
        .collect();
    online.sort_by_key(|u| hk::collation_key(&u.username));
    tpl.set(
        "online",
        Value::list(
            online
                .iter()
                .map(|u| {
                    row(vec![
                        ("id", v(u.id)),
                        ("username", v(u.username.as_str())),
                        ("figure", v(u.figure.as_str())),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut history: Vec<HkAudit> = world.store.records_where::<HkAudit>(|h| {
        matches!(
            h.action.as_str(),
            "hotel_alert" | "room_alert" | "user_alert"
        )
    })?;
    history.sort_by_key(|h| std::cmp::Reverse(h.created_at));
    tpl.set(
        "history",
        Value::list(
            history
                .iter()
                .take(20)
                .map(|h| {
                    row(vec![
                        ("created_at", v(row_date(h.created_at))),
                        ("details", v(h.details.as_str())),
                        (
                            "staff_name",
                            v(name_of(&world, h.staff_id).unwrap_or_default()),
                        ),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// A day in Madrid time (`yyyy-MM-dd`) as the Unix second it starts at.
fn madrid_day_start(day: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let local = d.and_hms_opt(0, 0, 0)?.and_utc().timestamp();
    // The offset at that moment (the day never starts inside a DST change in Madrid).
    let guess = local - 3600;
    let offset = hk::madrid(guess, "%z");
    let hours: i64 = offset.get(1..3).and_then(|h| h.parse().ok()).unwrap_or(1);
    Some(local - hours * 3600)
}

/// `/{hk}/chatlogs`.
pub fn chatlogs(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("chatlog")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let user = hk::str(conn, "user");
    let text = hk::str(conn, "text");
    let mut room_id = hk::num(conn, "room", 0);
    let mut from = hk::str(conn, "from");
    let mut to = hk::str(conn, "to");
    let around = hk::num(conn, "around", 0);
    let all = world.store.all_chat_logs()?;
    let rooms = world.store.all_rooms()?;
    let room_name = |id: u32| {
        rooms
            .iter()
            .find(|r| r.id == id)
            .map_or(String::new(), |r| r.name.clone())
    };
    let users = world.store.users()?;
    let user_of = |id: u32| users.iter().find(|u| u.id == id);
    let mut lines: Vec<&ChatLog>;
    if around > 0 {
        match all.iter().find(|c| c.id as i64 == around) {
            Some(pivot) => {
                room_id = pivot.room_id as i64;
                let ts = pivot.timestamp;
                lines = all
                    .iter()
                    .filter(|c| {
                        c.room_id == pivot.room_id
                            && c.timestamp >= ts - 600
                            && c.timestamp <= ts + 600
                    })
                    .collect();
            }
            None => lines = all.iter().collect(),
        }
        lines.sort_by_key(|c| (c.timestamp, c.id));
    } else {
        let parse = |s: &str| {
            chrono::NaiveDate::parse_from_str(
                if s.is_empty() { "2000-01-01" } else { s },
                "%Y-%m-%d",
            )
            .is_ok()
        };
        if !parse(&from) || !parse(&to) {
            from.clear();
            to.clear();
        }
        let from_ts = madrid_day_start(&from);
        let to_ts = madrid_day_start(&to).map(|t| t + 86400);
        lines = all
            .iter()
            .filter(|c| {
                (user.is_empty()
                    || user_of(c.user_id).is_some_and(|u| u.username.eq_ignore_ascii_case(&user)))
                    && (room_id <= 0 || c.room_id as i64 == room_id)
                    && (text.is_empty() || hk::like(&c.message, &text))
                    && from_ts.is_none_or(|f| c.timestamp >= f)
                    && to_ts.is_none_or(|t| c.timestamp < t)
            })
            .collect();
        lines.sort_by_key(|c| std::cmp::Reverse(c.id));
    }
    let pager = Pager::new(conn, &hk::path("/chatlogs"), lines.len() as i64, 100);
    let rows: Vec<Value> = pager
        .slice(&lines)
        .iter()
        .map(|c| {
            let u = user_of(c.user_id);
            row(vec![
                ("id", v(c.id)),
                ("user_id", v(c.user_id)),
                (
                    "username",
                    v(u.map_or("?".to_string(), |u| u.username.clone())),
                ),
                ("figure", v(u.map_or(String::new(), |u| u.figure.clone()))),
                ("room_id", v(c.room_id)),
                ("room_name", v(room_name(c.room_id))),
                ("chat_type", v(c.chat_type)),
                ("message", v(c.message.as_str())),
                ("said_at", v(row_date(c.timestamp))),
            ])
        })
        .collect();
    let mut tpl = hk::template(conn, "housekeeping/chatlogs");
    tpl.set("lines", Value::list(rows));
    tpl.set("pager", pager.value());
    tpl.set("user", user);
    tpl.set("text", text);
    tpl.set(
        "room",
        if room_id > 0 {
            room_id.to_string()
        } else {
            String::new()
        },
    );
    let room_row = (room_id > 0)
        .then(|| rooms.iter().find(|r| r.id as i64 == room_id))
        .flatten()
        .map(|r| row(vec![("id", v(r.id)), ("name", v(r.name.as_str()))]));
    tpl.set("roomName", room_row);
    tpl.set("from", from);
    tpl.set("to", to);
    tpl.set("around", around);
    let day = now() - 86400;
    let mut counts: Vec<(u32, i64)> = Vec::new();
    for c in all.iter().filter(|c| c.timestamp >= day) {
        match counts.iter_mut().find(|x| x.0 == c.room_id) {
            Some(x) => x.1 += 1,
            None => counts.push((c.room_id, 1)),
        }
    }
    counts.sort_by_key(|x| std::cmp::Reverse(x.1));
    tpl.set(
        "activeRooms",
        Value::list(
            counts
                .iter()
                .take(8)
                .map(|(r, n)| {
                    row(vec![
                        ("room_id", v(*r)),
                        ("room_name", v(room_name(*r))),
                        ("lines_count", v(*n)),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/audit`.
pub fn audit(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("audit")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let q = hk::str(conn, "q");
    let entries = hk::audit_log(&world, None, Some(&q));
    let pager = Pager::new(conn, &hk::path("/audit"), entries.len() as i64, 50);
    let mut tpl = hk::template(conn, "housekeeping/audit");
    tpl.set(
        "entries",
        Value::list(pager.slice(&entries.iter().map(hk::AuditRow::value).collect::<Vec<_>>())),
    );
    tpl.set("pager", pager.value());
    tpl.set("q", q);
    hk::render(conn, tpl)
}

/// The online users whose last visit is `room_id`.
fn occupants(world: &World, room_id: u32) -> Vec<User> {
    let visits = world.store.records::<RoomVisit>().unwrap_or_default();
    world
        .store
        .users()
        .unwrap_or_default()
        .into_iter()
        .filter(|u| u.is_online)
        .filter(|u| {
            visits
                .iter()
                .filter(|v| v.user_id == u.id)
                .max_by_key(|v| v.visited_at)
                .is_some_and(|v| v.room_id == room_id)
        })
        .collect()
}

/// `/{hk}/rooms/view`.
pub fn room_view(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("rooms/view")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = hk::num(conn, "id", 0);
    let room = u32::try_from(id)
        .ok()
        .and_then(|id| world.store.room(id).ok().flatten());
    let Some(room) = room else {
        hk::flash(conn, "danger", "Esa sala no existe.");
        conn.redirect(&hk::path("/rooms"));
        return Ok(());
    };
    let rid = room.id;
    let category = world
        .store
        .room_categories()?
        .into_iter()
        .find(|c| c.id as i64 == room.category as i64)
        .map(|c| c.name)
        .unwrap_or_default();
    let mut tpl = hk::template(conn, "housekeeping/room_view");
    // `SELECT r.*` (without the password) and the joined names.
    tpl.set(
        "room",
        row(vec![
            ("id", v(room.id)),
            ("owner_id", v(room.owner_id.to_string())),
            ("category", v(room.category)),
            ("name", v(room.name.as_str())),
            ("description", v(room.description.as_str())),
            ("model", v(room.model.as_str())),
            ("ccts", v(room.ccts.clone().unwrap_or_default())),
            ("wallpaper", v(room.wallpaper)),
            ("floor", v(room.floor)),
            ("landscape", v(room.landscape.as_str())),
            ("showname", v(room.showname)),
            ("superusers", v(room.superusers)),
            ("accesstype", v(room.accesstype)),
            ("visitors_now", v(room.visitors_now)),
            ("visitors_max", v(room.visitors_max)),
            ("rating", v(room.rating)),
            ("icon_data", v(room.icon_data.as_str())),
            ("group_id", v(room.group_id)),
            ("is_hidden", v(room.is_hidden)),
            ("created_at", v(row_date(room.created_at))),
            ("updated_at", v(row_date(room.updated_at))),
            (
                "owner_name",
                v(name_of(&world, room.owner_id).unwrap_or_default()),
            ),
            ("category_name", v(category)),
            ("is_hidden_room", v(room.is_hidden)),
        ]),
    );
    let mut people = occupants(&world, rid);
    people.sort_by_key(|u| hk::collation_key(&u.username));
    tpl.set(
        "occupants",
        Value::list(
            people
                .iter()
                .map(|u| {
                    row(vec![
                        ("id", v(u.id)),
                        ("username", v(u.username.as_str())),
                        ("figure", v(u.figure.as_str())),
                        ("motto", v(u.motto.as_str())),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let items = world.store.room_items(rid as i32)?;
    tpl.set("itemCount", items.len());
    let mut stacks: Vec<(u32, String, String, i64, Vec<String>)> = Vec::new();
    for i in &items {
        let owner = i
            .user_id
            .and_then(|u| name_of(&world, u))
            .unwrap_or_else(|| "?".into());
        match stacks.iter_mut().find(|s| s.0 == i.definition_id) {
            Some(s) => {
                s.3 += 1;
                if !s.4.contains(&owner) {
                    s.4.push(owner);
                }
            }
            None => {
                let d = world.items.definition(i.definition_id as i32);
                let (sprite, name) = d
                    .as_ref()
                    .map(|d| (d.record.sprite.clone(), d.record.name.clone()))
                    .unwrap_or_default();
                stacks.push((i.definition_id, sprite, name, 1, vec![owner]));
            }
        }
    }
    stacks.sort_by_key(|s| std::cmp::Reverse(s.3));
    tpl.set(
        "items",
        Value::list(
            stacks
                .iter()
                .take(50)
                .map(|(_, sprite, name, n, owners)| {
                    row(vec![
                        ("sprite", v(sprite.as_str())),
                        ("name", v(name.as_str())),
                        ("amount", v(*n)),
                        ("owners", v(owners.join(", "))),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut chat: Vec<ChatLog> = world
        .store
        .all_chat_logs()?
        .into_iter()
        .filter(|c| c.room_id == rid)
        .collect();
    chat.sort_by_key(|c| std::cmp::Reverse(c.id));
    tpl.set(
        "chat",
        Value::list(
            chat.iter()
                .take(80)
                .map(|c| {
                    row(vec![
                        ("id", v(c.id)),
                        ("user_id", v(c.user_id)),
                        (
                            "username",
                            v(name_of(&world, c.user_id).unwrap_or_else(|| "?".into())),
                        ),
                        ("message", v(c.message.as_str())),
                        ("chat_type", v(c.chat_type)),
                        ("said_at", v(row_date(c.timestamp))),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut visits: Vec<RoomVisit> = world
        .store
        .records_where::<RoomVisit>(|rv| rv.room_id == rid)?
        .into_iter()
        .filter(|rv| name_of(&world, rv.user_id).is_some())
        .collect();
    visits.sort_by_key(|rv| std::cmp::Reverse(rv.visited_at));
    tpl.set(
        "visits",
        Value::list(
            visits
                .iter()
                .take(20)
                .map(|rv| {
                    row(vec![
                        ("id", v(rv.user_id)),
                        (
                            "username",
                            v(name_of(&world, rv.user_id).unwrap_or_default()),
                        ),
                        ("visited_at", v(row_date(rv.visited_at))),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut rights: Vec<(u32, String)> = world
        .store
        .room_rights(rid)?
        .into_iter()
        .map(|u| (u, name_of(&world, u).unwrap_or_default()))
        .collect();
    rights.sort_by_key(|(_, n)| n.to_lowercase());
    tpl.set(
        "rights",
        Value::list(
            rights
                .iter()
                .map(|(u, n)| {
                    rusbbo_pebble::Map::new()
                        .with("userId", *u)
                        .with("username", n.as_str())
                        .with("user_id", *u)
                        .into_value()
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut bans = world.store.room_bans(rid)?;
    bans.sort_by_key(|(_, e)| std::cmp::Reverse(*e));
    tpl.set(
        "bans",
        Value::list(
            bans.iter()
                .map(|(u, e)| {
                    rusbbo_pebble::Map::new()
                        .with("userId", *u)
                        .with("user_id", *u)
                        .with("expireAt", *e)
                        .with("expire_at", *e)
                        .with("username", name_of(&world, *u).unwrap_or_default())
                        .into_value()
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut badges: Vec<String> = world
        .store
        .records_where::<RoomEntryBadge>(|b| b.room_id == rid)?
        .into_iter()
        .map(|b| b.badge)
        .collect();
    badges.sort_by_key(|b| b.to_lowercase());
    tpl.set("entryBadges", Value::list(badges));
    tpl.set(
        "audit",
        Value::list(
            hk::audit_log(&world, Some(("room", rid as i64)), None)
                .iter()
                .take(15)
                .map(hk::AuditRow::value)
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/rooms/action`.
pub fn room_action(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "rooms/view") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = hk::num(conn, "id", 0);
    let room = u32::try_from(id)
        .ok()
        .and_then(|id| world.store.room(id).ok().flatten());
    let kind = hk::str(conn, "do");
    let Some(room) = room else {
        hk::flash(conn, "danger", "Esa sala no existe.");
        conn.redirect(&hk::path("/rooms"));
        return Ok(());
    };
    match kind.as_str() {
        "alert" => {
            let message = hk::str(conn, "message");
            if message.is_empty() {
                hk::flash(conn, "danger", "Escribe el mensaje.");
            } else {
                crate::rcon::send(
                    &world,
                    "room_alert",
                    &[
                        ("roomId", room.id.to_string()),
                        ("message", message.clone()),
                    ],
                );
                hk::audit(
                    conn,
                    "room_alert",
                    "room",
                    room.id as i64,
                    &room.name,
                    &message,
                );
                hk::flash(
                    conn,
                    "success",
                    &format!("Alerta enviada a la sala {}.", room.name),
                );
            }
        }
        "kick_all" => {
            let staff = hk::staff(conn)
                .ok_or_else(|| crate::web::WebError("NullPointerException".into()))?;
            let mut kicked = 0;
            for u in occupants(&world, room.id) {
                if u.id != staff.id && u.rank < staff.rank {
                    crate::rcon::send(&world, "disconnect", &[("userId", u.id.to_string())]);
                    kicked += 1;
                }
            }
            hk::audit(
                conn,
                "room_kick",
                "room",
                room.id as i64,
                &room.name,
                &format!("{kicked} usuario(s) desconectados"),
            );
            hk::flash(
                conn,
                "success",
                &format!(
                    "{kicked} usuario(s) desconectados del hotel (estaban en {}).",
                    room.name
                ),
            );
        }
        _ => hk::flash(conn, "danger", "Acci\u{f3}n desconocida."),
    }
    conn.redirect(&hk::path(&format!("/rooms/view?id={}", room.id)));
    Ok(())
}
