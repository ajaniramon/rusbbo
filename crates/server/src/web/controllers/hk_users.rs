//! Java (Aeyra's panel): `HousekeepingUsersController`.

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{Ban, BanType, Tag, User};

use crate::web::beans::now;
use crate::web::hk::{self, Pager, row_date};
use crate::web::{Conn, WebResult, jdate};
use crate::world::World;

/// A row of `HkDao.rows`.
pub(super) fn row(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k, v);
    }
    m.into()
}

fn v(x: impl Into<Value>) -> Value {
    x.into()
}

fn banned_now(bans: &[Ban], user_id: u32, now: i64) -> bool {
    bans.iter().any(|b| {
        b.ban_type == BanType::UserId
            && b.banned_value == user_id.to_string()
            && b.is_active
            && b.banned_until > now
    })
}

/// `USER_COLUMNS`.
fn user_row(u: &User, now: i64, extra: Vec<(&str, Value)>) -> Value {
    let mut pairs = vec![
        ("id", v(u.id)),
        ("username", v(u.username.as_str())),
        ("figure", v(u.figure.as_str())),
        ("sex", v(u.sex.as_str())),
        ("motto", v(u.motto.as_str())),
        ("email", v(u.email.as_str())),
        ("credits", v(u.credits)),
        ("pixels", v(u.pixels)),
        ("tickets", v(u.tickets)),
        ("film", v(u.film)),
        ("rank", v(u.rank)),
        ("created_at", v(row_date(u.created_at))),
        ("last_online", v(row_date(u.last_online))),
        ("is_online", v(u.is_online)),
        ("club_subscribed", v(u.club_subscribed)),
        ("club_expiration", v(u.club_expiration)),
        ("is_club", v(u.club_expiration > now)),
        (
            "club_until",
            // FROM_UNIXTIME gives NULL past 2038 (shown as "").
            v(
                if u.club_expiration > 0 && u.club_expiration <= i32::MAX as i64 {
                    jdate::format(u.club_expiration, "yyyy-MM-dd HH:mm:ss")
                } else {
                    String::new()
                },
            ),
        ),
        ("machine_id", v(u.machine_id.as_str())),
        ("home_room", v(u.home_room)),
        ("is_trade_enabled", v(u.trade_enabled)),
        ("respect_points", v(u.respect_points)),
        ("favourite_group", v(u.favourite_group)),
    ];
    pairs.extend(extra);
    row(pairs)
}

/// `/{hk}/users`, `/{hk}/users/search`.
pub fn search(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("user/search")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let now = now();
    let query = hk::str(conn, "q");
    let mut filter = hk::str(conn, "filter");
    let sort = hk::str(conn, "sort");
    let bans = world.store.bans()?;
    let id_query =
        (query.len() <= 9 && !query.is_empty() && query.chars().all(|c| c.is_ascii_digit()))
            .then(|| query.parse::<u32>().ok())
            .flatten();
    let mut users: Vec<User> = world
        .store
        .users()?
        .into_iter()
        .filter(|u| !u.username.is_empty())
        .filter(|u| {
            query.is_empty()
                || hk::like(&u.username, &query)
                || hk::like(&u.email, &query)
                || hk::like(&u.motto, &query)
                || id_query == Some(u.id)
        })
        .collect();
    match filter.as_str() {
        "online" => users.retain(|u| u.is_online),
        "staff" => users.retain(|u| u.rank >= 2),
        "club" => users.retain(|u| u.club_expiration > now),
        "banned" => users.retain(|u| banned_now(&bans, u.id, now)),
        "nocredits" => users.retain(|u| u.credits == 0),
        _ => filter.clear(),
    }
    match sort.as_str() {
        "name" => users.sort_by_key(|u| u.username.to_lowercase()),
        "credits" => users.sort_by_key(|u| std::cmp::Reverse(u.credits)),
        "last_online" => users.sort_by_key(|u| {
            (
                std::cmp::Reverse(u.is_online),
                std::cmp::Reverse(u.last_online),
            )
        }),
        "id" => users.sort_by_key(|u| u.id),
        _ => users.sort_by_key(|u| std::cmp::Reverse(u.created_at)),
    }
    let pager = Pager::new(conn, &hk::path("/users"), users.len() as i64, 25);
    let rows: Vec<Value> = pager
        .slice(&users)
        .iter()
        .map(|u| user_row(u, now, vec![("is_banned", v(banned_now(&bans, u.id, now)))]))
        .collect();
    let mut tpl = hk::template(conn, "housekeeping/users_search");
    tpl.set("players", Value::list(rows));
    tpl.set("pager", pager.value());
    tpl.set("q", query);
    tpl.set("filter", filter);
    tpl.set("sort", sort);
    hk::render(conn, tpl)
}

/// Java: `assignableRanks`.
fn assignable_ranks(staff: &User) -> Vec<i64> {
    let max = if staff.rank >= 8 {
        8
    } else {
        staff.rank as i64 - 1
    };
    (1..=max).collect()
}

fn user_by_param(conn: &Conn) -> Option<User> {
    let mut id = hk::num(conn, "id", 0);
    let name = hk::str(conn, "name");
    if id == 0 && !name.is_empty() {
        id = conn
            .world()
            .store
            .user_by_name(&name)
            .ok()
            .flatten()
            .map_or(0, |u| u.id as i64);
    }
    u32::try_from(id)
        .ok()
        .and_then(|id| conn.world().store.user(id).ok().flatten())
}

fn name_of(world: &World, id: i64) -> String {
    u32::try_from(id)
        .ok()
        .and_then(|id| world.store.user(id).ok().flatten())
        .map(|u| u.username)
        .unwrap_or_default()
}

/// `/{hk}/users/view`.
pub fn view(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("user/search")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let now = now();
    let Some(user) = user_by_param(conn) else {
        hk::flash(conn, "danger", "Ese usuario no existe.");
        conn.redirect(&hk::path("/users"));
        return Ok(());
    };
    let Some(staff) = hk::staff(conn) else {
        return Err(crate::web::WebError("NullPointerException".into()));
    };
    let id = user.id;
    let me = staff.id == id;
    let mut tpl = hk::template(conn, "housekeeping/users_view");
    tpl.set("user", user_row(&user, now, vec![]));
    tpl.set("self", me);
    tpl.set("canManage", me || staff.rank > user.rank);
    tpl.set(
        "canEdit",
        hk::can(Some(&staff), "user/edit") && (me || staff.rank > user.rank),
    );
    tpl.set("ranks", Value::list(assignable_ranks(&staff)));
    let stats = world.store.statistics(id)?.map(|s| {
        let get = |k: &str| v(s.values.get(k).copied().unwrap_or(0));
        row(vec![
            ("online_time", get("online_time")),
            ("days_logged_in_row", get("days_logged_in_row")),
            ("xp_all_time", get("xp_all_time")),
            ("guided_by", get("guided_by")),
            ("players_guided", get("players_guided")),
            ("mute_expires_at", get("mute_expires_at")),
        ])
    });
    tpl.set("stats", stats);
    let mut badges = world.store.badges(id)?;
    badges.sort_by(|a, b| {
        b.equipped
            .cmp(&a.equipped)
            .then(a.code.to_lowercase().cmp(&b.code.to_lowercase()))
    });
    tpl.set(
        "badges",
        Value::list(
            badges
                .iter()
                .map(|b| {
                    row(vec![
                        ("badge", v(b.code.as_str())),
                        ("is_equipped", v(b.equipped)),
                        ("slot_id", v(b.slot_id)),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let items = world.store.all_items()?;
    let mut rooms = world.store.rooms_by_owner(id)?;
    rooms.sort_by_key(|r| r.id);
    tpl.set(
        "rooms",
        Value::list(
            rooms
                .iter()
                .map(|r| {
                    row(vec![
                        ("id", v(r.id)),
                        ("name", v(r.name.as_str())),
                        ("model", v(r.model.as_str())),
                        ("visitors_now", v(r.visitors_now)),
                        ("visitors_max", v(r.visitors_max)),
                        ("accesstype", v(r.accesstype)),
                        ("is_hidden", v(r.is_hidden)),
                        (
                            "item_count",
                            v(items.iter().filter(|i| i.room_id == r.id as i32).count()),
                        ),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let owned: Vec<&rusbbo_storage::Item> =
        items.iter().filter(|i| i.user_id == Some(id)).collect();
    let mut stacks: Vec<(u32, String, String, i64)> = Vec::new();
    for i in owned.iter().filter(|i| i.room_id == 0) {
        match stacks.iter_mut().find(|s| s.0 == i.definition_id) {
            Some(s) => s.3 += 1,
            None => {
                let d = world.items.definition(i.definition_id as i32);
                let (sprite, name) = d
                    .as_ref()
                    .map(|d| (d.record.sprite.clone(), d.record.name.clone()))
                    .unwrap_or_default();
                stacks.push((i.definition_id, sprite, name, 1));
            }
        }
    }
    stacks.sort_by(|a, b| {
        b.3.cmp(&a.3)
            .then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    tpl.set(
        "inventory",
        Value::list(
            stacks
                .iter()
                .take(60)
                .map(|(_, sprite, name, n)| {
                    row(vec![
                        ("sprite", v(sprite.as_str())),
                        ("name", v(name.as_str())),
                        ("amount", v(*n)),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let sum = |f: &dyn Fn(&rusbbo_storage::Item) -> bool| -> Value {
        if owned.is_empty() {
            v("")
        } else {
            v(owned.iter().filter(|i| f(i)).count())
        }
    };
    tpl.set(
        "inventoryTotals",
        row(vec![
            ("in_hand", sum(&|i| i.room_id == 0)),
            ("placed", sum(&|i| i.room_id > 0)),
            ("total", v(owned.len())),
        ]),
    );
    let mut friends: Vec<User> = world
        .store
        .messenger_friends(id)?
        .into_iter()
        .filter_map(|(f, _)| world.store.user(f).ok().flatten())
        .collect();
    friends.sort_by_key(|u| (std::cmp::Reverse(u.is_online), u.username.to_lowercase()));
    tpl.set(
        "friends",
        Value::list(
            friends
                .iter()
                .map(|u| {
                    row(vec![
                        ("id", v(u.id)),
                        ("username", v(u.username.as_str())),
                        ("figure", v(u.figure.as_str())),
                        ("is_online", v(u.is_online)),
                        ("last_online", v(row_date(u.last_online))),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let ip_logs = world.store.all_ip_logs()?;
    let mine: Vec<&rusbbo_storage::IpLog> = ip_logs
        .iter()
        .filter(|(u, _)| *u == id)
        .flat_map(|(_, l)| l.iter())
        .collect();
    let mut ips: Vec<(String, i64, i64, i64)> = Vec::new();
    for l in &mine {
        match ips.iter_mut().find(|x| x.0 == l.ip_address) {
            Some(x) => {
                x.1 += 1;
                x.2 = x.2.min(l.created_at);
                x.3 = x.3.max(l.created_at);
            }
            None => ips.push((l.ip_address.clone(), 1, l.created_at, l.created_at)),
        }
    }
    ips.sort_by_key(|x| std::cmp::Reverse(x.3));
    tpl.set(
        "ips",
        Value::list(
            ips.iter()
                .take(20)
                .map(|(ip, times, first, last)| {
                    let accounts = ip_logs
                        .iter()
                        .filter(|(_, l)| l.iter().any(|x| &x.ip_address == ip))
                        .count();
                    row(vec![
                        ("ip_address", v(ip.as_str())),
                        ("times", v(*times)),
                        ("first_seen", v(row_date(*first))),
                        ("last_seen", v(row_date(*last))),
                        ("accounts", v(accounts)),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let users = world.store.users()?;
    let machine = user.machine_id.clone();
    let mut same_machine: Vec<&User> = if machine.trim().is_empty() {
        Vec::new()
    } else {
        users
            .iter()
            .filter(|u| u.machine_id == machine && u.id != id)
            .collect()
    };
    same_machine.sort_by_key(|u| u.username.to_lowercase());
    tpl.set(
        "machineAccounts",
        Value::list(
            same_machine
                .iter()
                .map(|u| row(vec![("id", v(u.id)), ("username", v(u.username.as_str()))]))
                .collect::<Vec<_>>(),
        ),
    );
    let all_rooms = world.store.all_rooms()?;
    let room_name = |rid: u32| {
        all_rooms
            .iter()
            .find(|r| r.id == rid)
            .map_or(String::new(), |r| r.name.clone())
    };
    let mut chat: Vec<rusbbo_storage::ChatLog> = world
        .store
        .all_chat_logs()?
        .into_iter()
        .filter(|c| c.user_id == id)
        .collect();
    chat.sort_by_key(|c| std::cmp::Reverse(c.id));
    tpl.set(
        "chat",
        Value::list(
            chat.iter()
                .take(60)
                .map(|c| {
                    row(vec![
                        ("room_id", v(c.room_id)),
                        ("room_name", v(room_name(c.room_id))),
                        ("message", v(c.message.as_str())),
                        ("chat_type", v(c.chat_type)),
                        ("said_at", v(row_date(c.timestamp))),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut txs = world.store.transactions(id)?;
    txs.sort_by_key(|t| std::cmp::Reverse(t.created_at));
    tpl.set(
        "transactions",
        Value::list(
            txs.iter()
                .take(40)
                .map(|t| {
                    row(vec![
                        ("description", v(t.description.as_str())),
                        ("amount", v(t.amount)),
                        ("credit_cost", v(t.credit_cost)),
                        ("pixel_cost", v(t.pixel_cost)),
                        ("created_at", v(row_date(t.created_at))),
                        ("item_id", v(t.item_id.as_str())),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut bans: Vec<Ban> = world
        .store
        .bans()?
        .into_iter()
        .filter(|b| {
            (b.ban_type == BanType::UserId && b.banned_value == id.to_string())
                || (b.ban_type == BanType::MachineId
                    && b.banned_value == machine
                    && !machine.is_empty())
        })
        .collect();
    bans.sort_by_key(|b| std::cmp::Reverse(b.banned_at));
    let active = bans.iter().any(|b| b.is_active && b.banned_until > now);
    tpl.set(
        "bans",
        Value::list(
            bans.iter()
                .map(|b| {
                    row(vec![
                        ("ban_type", v(b.ban_type.name())),
                        ("banned_value", v(b.banned_value.as_str())),
                        ("message", v(b.message.as_str())),
                        ("banned_at", v(row_date(b.banned_at))),
                        ("banned_until", v(row_date(b.banned_until))),
                        ("is_active", v(b.is_active && b.banned_until > now)),
                        ("banned_by", v(name_of(&world, b.banned_by as i64))),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set("activeBan", active);
    let mut tags: Vec<String> = world
        .store
        .records_where::<Tag>(|t| t.user_id == id && t.room_id == 0 && t.group_id == 0)?
        .into_iter()
        .map(|t| t.tag)
        .collect();
    tags.sort_by_key(|t| t.to_lowercase());
    tpl.set(
        "tags",
        Value::list(
            tags.iter()
                .map(|t| row(vec![("tag", v(t.as_str()))]))
                .collect::<Vec<_>>(),
        ),
    );
    let mut visits: Vec<rusbbo_storage::RoomVisit> = world
        .store
        .records_where::<rusbbo_storage::RoomVisit>(|rv| rv.user_id == id)?
        .into_iter()
        .filter(|rv| all_rooms.iter().any(|r| r.id == rv.room_id))
        .collect();
    visits.sort_by_key(|rv| std::cmp::Reverse(rv.visited_at));
    let current = visits.iter().find_map(|rv| {
        all_rooms
            .iter()
            .find(|r| r.id == rv.room_id && r.visitors_now > 0)
            .map(|r| row(vec![("id", v(r.id)), ("name", v(r.name.as_str()))]))
    });
    tpl.set("currentRoom", current);
    tpl.set(
        "lastVisits",
        Value::list(
            visits
                .iter()
                .take(10)
                .map(|rv| {
                    row(vec![
                        ("id", v(rv.room_id)),
                        ("name", v(room_name(rv.room_id))),
                        ("visited_at", v(row_date(rv.visited_at))),
                    ])
                })
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set(
        "audit",
        Value::list(
            hk::audit_log(&world, Some(("user", id as i64)), None)
                .iter()
                .take(25)
                .map(hk::AuditRow::value)
                .collect::<Vec<_>>(),
        ),
    );
    let tab = hk::str(conn, "tab");
    tpl.set(
        "tab",
        if tab.is_empty() {
            "summary".into()
        } else {
            tab
        },
    );
    hk::render(conn, tpl)
}

/// `/{hk}/users/edit`.
pub fn edit(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("user/search")) {
        return Ok(());
    }
    let id = hk::num(conn, "id", 0);
    conn.redirect(&hk::path(&format!("/users/view?id={id}&tab=edit")));
    Ok(())
}

fn valid_figure(figure: &str) -> bool {
    let n = figure.chars().count();
    (5..=200).contains(&n)
        && figure
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

/// `/{hk}/users/save`.
pub fn save(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "user/edit") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = hk::num(conn, "id", 0);
    let back = format!("/users/view?id={id}&tab=edit");
    let target = u32::try_from(id)
        .ok()
        .and_then(|id| world.store.user(id).ok().flatten());
    let staff =
        hk::staff(conn).ok_or_else(|| crate::web::WebError("NullPointerException".into()))?;
    let Some(target) = target else {
        hk::flash(conn, "danger", "Ese usuario no existe.");
        conn.redirect(&hk::path("/users"));
        return Ok(());
    };
    let me = staff.id == target.id;
    if !me && staff.rank <= target.rank {
        hk::flash(
            conn,
            "danger",
            "No puedes editar a alguien con un rango igual o superior al tuyo.",
        );
        conn.redirect(&hk::path(&back));
        return Ok(());
    }
    let motto = hk::str(conn, "motto");
    let email = hk::str(conn, "email");
    let figure = hk::str(conn, "figure");
    let sex = if hk::str(conn, "sex").eq_ignore_ascii_case("F") {
        "F"
    } else {
        "M"
    };
    let credits = hk::num_or_null(conn, "credits");
    let pixels = hk::num_or_null(conn, "pixels");
    let tickets = hk::num_or_null(conn, "tickets");
    let film = hk::num_or_null(conn, "film");
    let rank = hk::num_or_null(conn, "rank");
    let password = conn.post.string("password");
    let bad_number = |n: Option<i64>| n.is_none_or(|n| n < 0);
    if motto.encode_utf16().count() > 100 {
        hk::flash(
            conn,
            "danger",
            "La misi\u{f3}n no puede tener m\u{e1}s de 100 caracteres.",
        );
    } else if bad_number(credits) || bad_number(pixels) || bad_number(tickets) || bad_number(film) {
        hk::flash(
            conn,
            "danger",
            "Cr\u{e9}ditos, p\u{ed}xeles, tickets y carretes tienen que ser n\u{fa}meros enteros positivos.",
        );
    } else if figure.is_empty() || !valid_figure(&figure) {
        hk::flash(
            conn,
            "danger",
            "El c\u{f3}digo de look no es v\u{e1}lido (formato hd-180-1.ch-210-66...).",
        );
    } else if rank.is_none_or(|r| !(1..=8).contains(&r)) {
        hk::flash(conn, "danger", "Rango no v\u{e1}lido.");
    } else if me && rank != Some(target.rank as i64) {
        hk::flash(conn, "danger", "No puedes cambiar tu propio rango.");
    } else if !me && !assignable_ranks(&staff).contains(&rank.unwrap_or(0)) {
        hk::flash(
            conn,
            "danger",
            "No puedes asignar un rango superior al tuyo.",
        );
    } else if !password.is_empty() && password.encode_utf16().count() < 6 {
        hk::flash(
            conn,
            "danger",
            "La contrase\u{f1}a nueva necesita al menos 6 caracteres.",
        );
    } else {
        let (credits, pixels, tickets, film, rank) = (
            credits.unwrap_or(0) as i32,
            pixels.unwrap_or(0) as i32,
            tickets.unwrap_or(0) as i32,
            film.unwrap_or(0) as i32,
            rank.unwrap_or(1) as u8,
        );
        let mut changes: Vec<String> = Vec::new();
        let online = target.is_online;
        if motto != target.motto {
            changes.push("misi\u{f3}n".into());
        }
        if email != target.email {
            changes.push("email".into());
        }
        if figure != target.figure || !sex.eq_ignore_ascii_case(&target.sex) {
            changes.push("look".into());
        }
        let arrow = |label: &str, old: i64, new: i64| format!("{label} {old} \u{2192} {new}");
        if credits != target.credits {
            changes.push(arrow(
                "cr\u{e9}ditos",
                target.credits as i64,
                credits as i64,
            ));
        }
        if pixels != target.pixels {
            changes.push(arrow("p\u{ed}xeles", target.pixels as i64, pixels as i64));
        }
        if tickets != target.tickets {
            changes.push(arrow("tickets", target.tickets as i64, tickets as i64));
        }
        if film != target.film {
            changes.push(arrow("carretes", target.film as i64, film as i64));
        }
        if rank != target.rank {
            changes.push(arrow("rango", target.rank as i64, rank as i64));
        }
        if !password.is_empty() {
            changes.push("contrase\u{f1}a".into());
        }
        let hashed = if password.is_empty() {
            None
        } else {
            Some(crate::game::password::hash(&password)?)
        };
        world.store.update_user(target.id, |u| {
            u.motto = motto.clone();
            u.email = email.clone();
            u.figure = figure.clone();
            u.sex = sex.into();
            u.credits = credits;
            u.pixels = pixels;
            u.tickets = tickets;
            u.film = film;
            u.rank = rank;
            if let Some(h) = &hashed {
                u.password = h.clone();
            }
        })?;
        let uid = [("userId", target.id.to_string())];
        crate::rcon::send(&world, "refresh_looks", &uid);
        crate::rcon::send(&world, "refresh_credits", &uid);
        let relog = online
            && (rank != target.rank
                || pixels != target.pixels
                || tickets != target.tickets
                || film != target.film);
        if relog && hk::flag(conn, "apply_now") {
            crate::rcon::send(&world, "disconnect", &uid);
        }
        let details = if changes.is_empty() {
            "Sin cambios".to_string()
        } else {
            changes.join(", ")
        };
        hk::audit(
            conn,
            "user_edit",
            "user",
            target.id as i64,
            &target.username,
            &details,
        );
        if changes.is_empty() {
            hk::flash(conn, "info", "No hab\u{ed}a nada que cambiar.");
        } else if relog && !hk::flag(conn, "apply_now") {
            hk::flash(
                conn,
                "warning",
                &format!(
                    "Guardado ({}). Misi\u{f3}n, look y cr\u{e9}ditos ya se ven en el juego; rango, p\u{ed}xeles y tickets se aplicar\u{e1}n cuando {} vuelva a entrar.",
                    changes.join(", "),
                    target.username
                ),
            );
        } else {
            hk::flash(
                conn,
                "success",
                &format!("Guardado: {}.", changes.join(", ")),
            );
        }
    }
    conn.redirect(&hk::path(&back));
    Ok(())
}

fn valid_badge(code: &str) -> bool {
    let n = code.chars().count();
    (1..=50).contains(&n)
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `/{hk}/users/action`.
pub fn action(conn: &mut Conn) -> WebResult {
    let kind = hk::str(conn, "do");
    let permission = match kind.as_str() {
        "ban" => "bans",
        "badge_add" | "badge_remove" => "badges",
        _ => "user/edit",
    };
    if !hk::guard_action(conn, permission) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let target = user_by_param(conn).filter(|u| u.id > 0);
    let staff =
        hk::staff(conn).ok_or_else(|| crate::web::WebError("NullPointerException".into()))?;
    let Some(target) = target else {
        hk::flash(conn, "danger", "Ese usuario no existe.");
        let to = hk::back(conn);
        conn.redirect(&to);
        return Ok(());
    };
    let me = staff.id == target.id;
    let back = hk::back(conn);
    let online = target.is_online;
    let name = target.username.clone();
    let id = target.id;
    let uid = [("userId", id.to_string())];
    if !me && staff.rank <= target.rank {
        hk::flash(
            conn,
            "danger",
            "No puedes actuar sobre alguien con un rango igual o superior al tuyo.",
        );
        conn.redirect(&back);
        return Ok(());
    }
    match kind.as_str() {
        "kick" => {
            if me {
                hk::flash(conn, "danger", "No te puedes expulsar a ti mismo.");
            } else {
                crate::rcon::send(&world, "disconnect", &uid);
                hk::audit(
                    conn,
                    "user_kick",
                    "user",
                    id as i64,
                    &name,
                    &hk::str(conn, "reason"),
                );
                if online {
                    hk::flash(
                        conn,
                        "success",
                        &format!("{name} ha sido desconectado del hotel."),
                    );
                } else {
                    hk::flash(conn, "info", &format!("{name} no estaba conectado."));
                }
            }
        }
        "alert" => {
            let message = hk::str(conn, "message");
            if message.is_empty() {
                hk::flash(conn, "danger", "Escribe el mensaje de la alerta.");
            } else {
                crate::rcon::send(
                    &world,
                    "user_alert",
                    &[("userId", id.to_string()), ("message", message.clone())],
                );
                hk::audit(conn, "user_alert", "user", id as i64, &name, &message);
                if online {
                    hk::flash(conn, "success", &format!("Alerta enviada a {name}."));
                } else {
                    hk::flash(
                        conn,
                        "warning",
                        &format!(
                            "{name} no est\u{e1} conectado; la alerta solo llega a usuarios dentro del hotel."
                        ),
                    );
                }
            }
        }
        "credits" | "pixels" => {
            let amount = hk::num_or_null(conn, "amount");
            match amount {
                Some(a) if a != 0 && a.abs() <= 10_000_000 => {
                    let credits = kind == "credits";
                    let updated = world.store.update_user(id, |u| {
                        if credits {
                            u.credits = (u.credits as i64 + a).max(0) as i32;
                        } else {
                            u.pixels = (u.pixels as i64 + a).max(0) as i32;
                        }
                    })?;
                    let now_value =
                        updated.map_or(0, |u| if credits { u.credits } else { u.pixels });
                    let sign = if a > 0 { "+" } else { "" };
                    if credits {
                        crate::rcon::send(&world, "refresh_credits", &uid);
                        hk::audit(
                            conn,
                            "user_credits",
                            "user",
                            id as i64,
                            &name,
                            &format!(
                                "{sign}{a} cr\u{e9}ditos (ahora {now_value}) {}",
                                hk::str(conn, "reason")
                            ),
                        );
                        hk::flash(
                            conn,
                            "success",
                            &format!(
                                "{}{} cr\u{e9}ditos a {name}. Saldo: {now_value}{}",
                                if a > 0 { "A\u{f1}adidos " } else { "Quitados " },
                                a.abs(),
                                if online {
                                    " (actualizado en el juego)."
                                } else {
                                    "."
                                }
                            ),
                        );
                    } else {
                        hk::audit(
                            conn,
                            "user_pixels",
                            "user",
                            id as i64,
                            &name,
                            &format!("{sign}{a} p\u{ed}xeles (ahora {now_value})"),
                        );
                        hk::flash(
                            conn,
                            if online { "warning" } else { "success" },
                            &format!(
                                "P\u{ed}xeles de {name}: {now_value}.{}",
                                if online {
                                    " El juego mostrar\u{e1} el saldo nuevo cuando vuelva a entrar."
                                } else {
                                    ""
                                }
                            ),
                        );
                    }
                }
                _ => hk::flash(
                    conn,
                    "danger",
                    "Indica una cantidad distinta de cero (puede ser negativa).",
                ),
            }
        }
        "club" => match hk::num_or_null(conn, "days") {
            Some(days) if days != 0 && days.abs() <= 3650 => {
                let now = now();
                let base = target.club_expiration.max(now);
                let mut expiration = base + days * 86400;
                if expiration <= now {
                    expiration = 0;
                }
                let first = if target.club_subscribed > 0 {
                    target.club_subscribed
                } else if expiration > 0 {
                    now
                } else {
                    0
                };
                world.store.update_user(id, |u| {
                    u.club_subscribed = first;
                    u.club_expiration = expiration;
                })?;
                crate::rcon::send(&world, "refresh_club", &uid);
                let sign = if days > 0 { "+" } else { "" };
                hk::audit(
                    conn,
                    "user_club",
                    "user",
                    id as i64,
                    &name,
                    &format!("{sign}{days} d\u{ed}as de HC"),
                );
                if expiration == 0 {
                    hk::flash(conn, "success", &format!("Se ha retirado el HC a {name}."));
                } else {
                    hk::flash(
                        conn,
                        "success",
                        &format!("{name} tiene HC hasta el {}.", hk::date(expiration)),
                    );
                }
            }
            _ => hk::flash(
                conn,
                "danger",
                "Indica los d\u{ed}as de HC (negativo para quitar).",
            ),
        },
        "club_remove" => {
            world.store.update_user(id, |u| u.club_expiration = 0)?;
            crate::rcon::send(&world, "refresh_club", &uid);
            hk::audit(conn, "user_club", "user", id as i64, &name, "HC retirado");
            hk::flash(conn, "success", &format!("Se ha retirado el HC a {name}."));
        }
        "badge_add" | "badge_remove" => {
            let add = kind == "badge_add";
            let code = if add {
                hk::str(conn, "badge").to_uppercase()
            } else {
                hk::str(conn, "badge")
            };
            let mut badges = world.store.badges(id)?;
            // `badge = ?` ignores case, as the collation does.
            let has = badges
                .iter()
                .any(|b| b.code.to_lowercase() == code.to_lowercase());
            let audit_game = |action: &str| {
                if let Err(e) = world.store.add_audit_log(&rusbbo_storage::AuditLogEntry {
                    action: action.into(),
                    user_id: staff.id as i32,
                    target_id: id as i32,
                    message: code.clone(),
                    extra_notes: "Desde el panel".into(),
                    created_at: now(),
                }) {
                    tracing::error!("hk badge audit: {e}");
                }
            };
            if add && !valid_badge(&code) {
                hk::flash(
                    conn,
                    "danger",
                    "El c\u{f3}digo de placa solo puede tener letras, n\u{fa}meros, - y _ (m\u{e1}x. 50).",
                );
            } else if add && has {
                hk::flash(
                    conn,
                    "warning",
                    &format!("{name} ya tiene la placa {code}."),
                );
            } else if add {
                badges.push(rusbbo_storage::Badge {
                    code: code.clone(),
                    equipped: false,
                    slot_id: 0,
                });
                world.store.put_badges(id, &badges)?;
                audit_game("badge_grant");
                hk::audit(conn, "badge_grant", "user", id as i64, &name, &code);
                hk::flash(
                    conn,
                    "success",
                    &format!(
                        "Placa {code} entregada a {name}.{}",
                        if online {
                            " La ver\u{e1} al volver a entrar."
                        } else {
                            ""
                        }
                    ),
                );
            } else if has {
                badges.retain(|b| b.code.to_lowercase() != code.to_lowercase());
                world.store.put_badges(id, &badges)?;
                audit_game("badge_remove");
                hk::audit(conn, "badge_remove", "user", id as i64, &name, &code);
                hk::flash(conn, "success", &format!("Placa {code} retirada a {name}."));
            } else {
                hk::flash(conn, "warning", "Ese usuario no tiene esa placa.");
            }
        }
        "ban" => {
            let reason = hk::str(conn, "reason");
            let mut hours = hk::num(conn, "hours", 24);
            if me {
                hk::flash(conn, "danger", "No te puedes banear a ti mismo.");
            } else if reason.is_empty() {
                hk::flash(
                    conn,
                    "danger",
                    "Escribe el motivo del baneo (lo ver\u{e1} el usuario).",
                );
            } else {
                let forever = 24 * 365 * 30;
                if hours <= 0 || hours > forever {
                    hours = forever;
                }
                let now = now();
                let until = now + hours * 3600;
                let ban = |kind: BanType, value: &str| Ban {
                    ban_type: kind,
                    banned_value: value.into(),
                    message: reason.clone(),
                    banned_until: until,
                    banned_at: now,
                    banned_by: staff.id as i32,
                    is_active: true,
                };
                world
                    .store
                    .add_ban(&ban(BanType::UserId, &id.to_string()))?;
                let mut detail = format!(
                    "{reason} ({})",
                    if hours >= forever {
                        "permanente".to_string()
                    } else {
                        format!("{hours} h")
                    }
                );
                if hk::flag(conn, "machine") && !target.machine_id.trim().is_empty() {
                    world
                        .store
                        .add_ban(&ban(BanType::MachineId, &target.machine_id))?;
                    detail.push_str(" + m\u{e1}quina");
                }
                crate::rcon::send(&world, "disconnect", &uid);
                hk::audit(conn, "user_ban", "user", id as i64, &name, &detail);
                hk::flash(
                    conn,
                    "success",
                    &format!("{name} ha sido baneado hasta el {}.", hk::date(until)),
                );
            }
        }
        _ => hk::flash(conn, "danger", "Acci\u{f3}n desconocida."),
    }
    conn.redirect(&back);
    Ok(())
}

/// `/{hk}/users/imitate/*`.
pub fn imitate(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "user/imitate") {
        return Ok(());
    }
    let name = if conn.matches.is_empty() {
        hk::str(conn, "name")
    } else {
        conn.wildcard(0).to_string()
    };
    let player = conn.world().store.user_by_name(&name)?;
    let staff =
        hk::staff(conn).ok_or_else(|| crate::web::WebError("NullPointerException".into()))?;
    let Some(player) = player else {
        hk::flash(conn, "danger", "Ese usuario no existe.");
        conn.redirect(&hk::path("/users"));
        return Ok(());
    };
    if player.id != staff.id && player.rank >= staff.rank {
        hk::flash(
            conn,
            "danger",
            "No puedes entrar como alguien con un rango igual o superior al tuyo.",
        );
        conn.redirect(&hk::path(&format!("/users/view?id={}", player.id)));
        return Ok(());
    }
    hk::audit(
        conn,
        "user_imitate",
        "user",
        player.id as i64,
        &player.username,
        "Entr\u{f3} en la web como este usuario",
    );
    {
        let mut s = conn.session();
        s.set("authenticated", true);
        s.set("captcha.invalid", false);
        s.set("user.id", player.id as i64);
        s.set("clientAuthenticate", false);
        s.set(hk::LOGGED_IN, false);
        s.set(
            "lastRequest",
            (now() + super::REAUTHENTICATE_TIME).to_string(),
        );
    }
    conn.redirect("/me");
    Ok(())
}

fn valid_username(name: &str) -> bool {
    let n = name.chars().count();
    (3..=20).contains(&n)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:=?!@-".contains(c))
}

/// `/{hk}/users/create`.
pub fn create(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("user/create")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/users_create");
    let default_figure = "hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61";
    tpl.set("defaultFigure", default_figure);
    if hk::is_post(conn) {
        let username = hk::str(conn, "username");
        let email = hk::str(conn, "email");
        let figure = hk::str(conn, "figure");
        let sex = if hk::str(conn, "sex").eq_ignore_ascii_case("F") {
            "F"
        } else {
            "M"
        };
        let password = conn.post.string("password");
        let credits = hk::num(conn, "credits", 0).max(0);
        let error = if !valid_username(&username) {
            Some(
                "El nombre debe tener entre 3 y 20 caracteres (letras, n\u{fa}meros y - _ . : = ? ! @).",
            )
        } else if world.store.user_by_name(&username)?.is_some() {
            Some("Ya existe un usuario con ese nombre.")
        } else if password.encode_utf16().count() < 6 {
            Some("La contrase\u{f1}a necesita al menos 6 caracteres.")
        } else if !figure.is_empty() && !valid_figure(&figure) {
            Some("El look no es v\u{e1}lido.")
        } else {
            None
        };
        if let Some(error) = error {
            hk::flash(conn, "danger", error);
            tpl.set(
                "form",
                row(vec![
                    ("username", v(username.as_str())),
                    ("email", v(email.as_str())),
                    ("figure", v(figure.as_str())),
                    ("sex", v(sex)),
                    ("credits", v(credits)),
                ]),
            );
        } else {
            let figure = if figure.is_empty() {
                default_figure.to_string()
            } else {
                figure
            };
            let hashed = crate::game::password::hash(&password)?;
            let id = super::register::new_user(&world, &username, &hashed, &figure, sex, &email)?;
            world
                .store
                .put_statistics(id, &rusbbo_storage::UserStatistics::new_row())?;
            if credits > 0 {
                world
                    .store
                    .update_user(id, |u| u.credits = credits as i32)?;
            }
            hk::audit(
                conn,
                "user_create",
                "user",
                id as i64,
                &username,
                "Cuenta creada desde el panel",
            );
            hk::flash(conn, "success", &format!("Usuario {username} creado."));
            conn.redirect(&hk::path(&format!("/users/view?id={id}")));
            return Ok(());
        }
    }
    hk::render(conn, tpl)
}
