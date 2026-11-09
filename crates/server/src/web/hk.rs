//! The housekeeping panel's shared parts (Java: Aeyra's `controllers/housekeeping/Hk`,
//! `HkPager`, `HkDao`, `HousekeepingManager` and `HousekeepingBinder`).

use std::sync::Arc;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{HkAudit, User};

use super::routes_table::HOUSEKEEPING_PATH;
use super::tpl::Tpl;
use super::{Conn, beans};
use crate::world::World;

/// Java: `SessionUtil.LOGGED_IN_HOUSKEEPING`.
pub const LOGGED_IN: &str = "authenticatedHousekeeping";
/// Java: `Hk.TOKEN_SESSION_KEY` / `TOKEN_FIELD`.
pub const TOKEN_SESSION_KEY: &str = "hk.csrf";
pub const TOKEN_FIELD: &str = "hk_token";

/// Java: `HousekeepingManager.loadPermissions`: the lowest rank for each permission.
fn permission_rank(permission: &str) -> Option<u8> {
    Some(match permission {
        "root/login"
        | "transaction/lookup"
        | "marketplace/log_check"
        | "marketplace/user_log"
        | "bans"
        | "articles/create"
        | "articles/edit_own"
        | "articles/delete_own"
        | "hotel/alert"
        | "chatlog"
        | "rooms/view" => 5,
        "room_badges"
        | "infobus"
        | "infobus/delete_own"
        | "catalogue/edit_frontpage"
        | "badges"
        | "audit" => 6,
        "user/search"
        | "user/edit"
        | "user/create"
        | "articles/edit_any"
        | "articles/delete_any"
        | "room_ads"
        | "configuration"
        | "infobus/delete_any"
        | "catalogue/manage"
        | "item_definitions/manage"
        | "vouchers/manage"
        | "wordfilter/manage"
        | "recycler/manage"
        | "room_categories/manage"
        | "room_models/manage"
        | "rooms/manage"
        | "groups/manage"
        | "user/imitate"
        | "user/matches" => 8,
        _ => return None,
    })
}

/// Java: `Hk.can`.
pub fn can(staff: Option<&User>, permission: &str) -> bool {
    staff.is_some_and(|s| permission_rank(permission).is_some_and(|r| s.rank >= r))
}

/// Java: `Hk.path`.
pub fn path(suffix: &str) -> String {
    format!("/{HOUSEKEEPING_PATH}{suffix}")
}

/// Java: `HousekeepingBinder.rankLabel`.
pub fn rank_label(value: &Value) -> String {
    let id = match value {
        Value::Int(i) => *i,
        Value::Float(f) => *f as i64,
        v => match v.render().parse::<i32>() {
            Ok(i) => i as i64,
            Err(_) => return v.render(),
        },
    };
    match id {
        0 => "Sin rango".into(),
        1 => "Usuario".into(),
        2 => "Gu\u{ed}a".into(),
        3 => "Hobba".into(),
        4 => "Superhobba".into(),
        5 => "Moderador".into(),
        6 => "Community manager".into(),
        7 => "Hotel manager".into(),
        8 => "Administrador".into(),
        n => format!("Rango {n}"),
    }
}

/// Java: `HousekeepingBinder.actionLabel`.
pub fn action_label(code: &str) -> String {
    match code {
        "hk_login" => "Entrada al panel",
        "user_edit" => "Editar usuario",
        "user_kick" | "kick_user" => "Expulsar",
        "user_alert" | "alert_user" => "Alerta a usuario",
        "user_credits" => "Cr\u{e9}ditos",
        "user_pixels" => "P\u{ed}xeles",
        "user_club" => "HC",
        "user_ban" | "ban_user" => "Baneo",
        "user_unban" => "Desbaneo",
        "user_create" => "Crear usuario",
        "user_imitate" => "Entrar como",
        "badge_grant" => "Dar placa",
        "badge_remove" => "Quitar placa",
        "badge_update" => "Editar placa",
        "hotel_alert" => "Alerta al hotel",
        "room_alert" => "Alerta a sala",
        "room_kick" => "Vaciar sala",
        "room_edit" => "Editar sala",
        "room_delete" => "Borrar sala",
        "room_hide" => "Ocultar sala",
        "room_unhide" => "Mostrar sala",
        "group_delete" => "Borrar grupo",
        "config_save" => "Configuraci\u{f3}n",
        "article_delete" => "Borrar noticia",
        "catalogue_frontpage" => "Portada cat\u{e1}logo",
        other => other,
    }
    .to_string()
}

/// The UTC offset of Europe/Madrid at `ts` (Java: `HkDao.DISPLAY_ZONE`): CEST from the last
/// Sunday of March to the last Sunday of October, at 01:00 UTC.
fn madrid_offset(ts: i64) -> i64 {
    use chrono::{Datelike, NaiveDate, TimeZone, Utc, Weekday};
    let Some(t) = Utc.timestamp_opt(ts, 0).single() else {
        return 3600;
    };
    let last_sunday = |month: u32| {
        let mut d = NaiveDate::from_ymd_opt(t.year(), month + 1, 1).unwrap_or_default()
            - chrono::Duration::days(1);
        while d.weekday() != Weekday::Sun {
            d -= chrono::Duration::days(1);
        }
        d.and_hms_opt(1, 0, 0)
            .unwrap_or_default()
            .and_utc()
            .timestamp()
    };
    if ts >= last_sunday(3) && ts < last_sunday(10) {
        7200
    } else {
        3600
    }
}

/// `ts` formatted in Madrid time with a `chrono` pattern.
pub fn madrid(ts: i64, pattern: &str) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_opt(ts + madrid_offset(ts), 0)
        .single()
        .map(|t| t.format(pattern).to_string())
        .unwrap_or_default()
}

/// Java: `Hk.date` ("dd/MM/yyyy HH:mm").
pub fn date(ts: i64) -> String {
    madrid(ts, "%d/%m/%Y %H:%M")
}

/// How `HkDao.rows` shows a `DATETIME` column ("yyyy-MM-dd HH:mm"; "" for none).
pub fn row_date(ts: i64) -> String {
    madrid(ts, "%Y-%m-%d %H:%M")
}

/// Java: `Hk.staff`: the logged-in staff member.
pub fn staff(conn: &Conn) -> Option<User> {
    if !conn.session().contains("user.id") {
        return None;
    }
    conn.world().store.user(conn.user_id()).ok().flatten()
}

/// Java: `Hk.token`.
pub fn token(conn: &Conn) -> String {
    let existing = conn
        .session()
        .string(TOKEN_SESSION_KEY)
        .filter(|t| !t.trim().is_empty());
    match existing {
        Some(t) => t,
        None => {
            use rand::RngExt;
            let bytes: [u8; 24] = rand::rng().random();
            let t: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            conn.session().set(TOKEN_SESSION_KEY, t.clone());
            t
        }
    }
}

fn valid_token(conn: &Conn) -> bool {
    let expected = conn.session().string(TOKEN_SESSION_KEY);
    let supplied = conn
        .post
        .contains(TOKEN_FIELD)
        .then(|| conn.post.string(TOKEN_FIELD));
    matches!((expected, supplied), (Some(e), Some(s)) if !e.trim().is_empty() && e == s)
}

pub fn is_post(conn: &Conn) -> bool {
    conn.method == "POST"
}

/// Java: `Hk.flash`.
pub fn flash(conn: &Conn, colour: &str, message: &str) {
    // The messages are written in Spanish; `web::i18n` translates them for an English panel.
    let message = super::i18n::tr(conn.hk_lang().0, message);
    let mut s = conn.session();
    s.set("alertColour", colour);
    s.set("alertMessage", message);
}

/// Java: `Hk.back`: the `return` parameter or the referer when it is a panel page.
pub fn back(conn: &Conn) -> String {
    let base = path("");
    let target = if conn.post.contains("return") {
        Some(conn.post.string("return"))
    } else if conn.get.contains("return") {
        Some(conn.get.string("return"))
    } else {
        None
    };
    if let Some(t) = target
        && t.starts_with(&base)
        && !t.contains("//")
        && !t.contains('\n')
    {
        return t;
    }
    if let Some(referer) = conn.header("Referer")
        && let Some(i) = referer.find(&base)
    {
        let local = &referer[i..];
        if !local.contains("//") {
            return local.to_string();
        }
    }
    base
}

/// Java: `Hk.guard`.
pub fn guard(conn: &mut Conn, permission: Option<&str>) -> bool {
    if !conn.session().boolean(LOGGED_IN) {
        conn.redirect(&path(""));
        return false;
    }
    let staff = staff(conn);
    if staff.is_none() {
        conn.session().set(LOGGED_IN, false);
        conn.redirect(&path(""));
        return false;
    }
    if let Some(p) = permission
        && !can(staff.as_ref(), p)
    {
        flash(
            conn,
            "danger",
            "No tienes permiso para acceder a esa secci\u{f3}n.",
        );
        conn.redirect(&path(""));
        return false;
    }
    if is_post(conn) && !valid_token(conn) {
        flash(
            conn,
            "danger",
            "La sesi\u{f3}n del formulario ha caducado. Vuelve a intentarlo.",
        );
        let to = back(conn);
        conn.redirect(&to);
        return false;
    }
    true
}

/// Java: `Hk.guardAction`: a guarded page that only takes POSTs.
pub fn guard_action(conn: &mut Conn, permission: &str) -> bool {
    if !guard(conn, Some(permission)) {
        return false;
    }
    if !is_post(conn) {
        flash(
            conn,
            "warning",
            "Esa acci\u{f3}n solo se puede ejecutar desde su bot\u{f3}n en el panel.",
        );
        let to = back(conn);
        conn.redirect(&to);
        return false;
    }
    true
}

/// Java: `Hk.template`.
pub fn template(conn: &mut Conn, view: &str) -> Tpl {
    let mut tpl = conn.template(view);
    tpl.set("housekeepingManager", Map::new());
    tpl
}

/// Java: `Hk.render`: the page, then the flash message is gone.
pub fn render(conn: &mut Conn, tpl: Tpl) -> super::WebResult {
    conn.render(tpl)?;
    conn.session().delete("alertMessage");
    Ok(())
}

/// Java: `Hk.str`: a POST or GET parameter, trimmed.
pub fn str(conn: &Conn, key: &str) -> String {
    if conn.post.contains(key) {
        return conn.post.string(key).trim().to_string();
    }
    if conn.get.contains(key) {
        return conn.get.string(key).trim().to_string();
    }
    String::new()
}

/// Java: `Hk.num`.
pub fn num(conn: &Conn, key: &str, fallback: i64) -> i64 {
    let v = str(conn, key);
    if v.is_empty() {
        return fallback;
    }
    v.parse::<i32>().map_or(fallback, i64::from)
}

/// Java: `Hk.numOrNull`.
pub fn num_or_null(conn: &Conn, key: &str) -> Option<i64> {
    str(conn, key).parse::<i32>().ok().map(i64::from)
}

/// Java: `Hk.flag`.
pub fn flag(conn: &Conn, key: &str) -> bool {
    let v = str(conn, key);
    v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("true") || v == "1"
}

/// Java: `HkDao.audit` (via `Hk.audit`).
pub fn audit(
    conn: &Conn,
    action: &str,
    target_type: &str,
    target_id: i64,
    target_name: &str,
    details: &str,
) {
    let staff_id = staff(conn).map_or(0, |u| u.id);
    audit_as(
        conn.world(),
        staff_id,
        action,
        target_type,
        target_id,
        target_name,
        details,
        &conn.ip,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn audit_as(
    world: &World,
    staff_id: u32,
    action: &str,
    target_type: &str,
    target_id: i64,
    target_name: &str,
    details: &str,
    ip: &str,
) {
    let cut = |s: &str, n: usize| {
        String::from_utf16_lossy(&s.encode_utf16().take(n).collect::<Vec<u16>>())
    };
    let mut row = HkAudit {
        created_at: beans::now(),
        staff_id,
        action: action.into(),
        target_type: target_type.into(),
        target_id: target_id as i32,
        target_name: cut(target_name, 100),
        details: cut(details, 2000),
        ip_address: cut(ip, 64),
        ..Default::default()
    };
    if let Err(e) = world.store.insert_record(&mut row) {
        tracing::error!("hk audit: {e}");
    }
}

/// One entry of `HkDao.auditLog` (the panel's own log and the game's `housekeeping_audit_log`).
pub struct AuditRow {
    pub created_at: i64,
    pub staff_id: i64,
    pub action: String,
    pub target_type: String,
    pub target_id: i64,
    pub target_name: String,
    pub details: String,
    pub staff_name: String,
}

impl AuditRow {
    /// [`AuditRow::value`] with the details (texts the port writes in Spanish) in `lang`.
    pub fn value_in(&self, lang: super::i18n::Lang) -> Value {
        self.map(&super::i18n::tr(lang, &self.details))
    }

    pub fn value(&self) -> Value {
        self.map(&self.details)
    }

    fn map(&self, details: &str) -> Value {
        Map::new()
            .with("created_at", row_date(self.created_at))
            .with("staff_id", self.staff_id)
            .with("action", self.action.as_str())
            .with("target_type", self.target_type.as_str())
            .with("target_id", self.target_id)
            .with("target_name", self.target_name.as_str())
            .with("details", details)
            .with("staff_name", self.staff_name.as_str())
            .into()
    }
}

/// A sort key that orders strings as MariaDB's `utf8mb4_uca1400_ai_ci` collation does for the
/// characters the panel lists (DUCET order: spaces and punctuation, then symbols, digits and
/// letters; case and accents ignored).
pub fn collation_key(s: &str) -> Vec<u32> {
    const PUNCTUATION: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    s.chars()
        .map(|c| {
            if let Some(i) = PUNCTUATION.find(c) {
                return 0x100 + i as u32;
            }
            if c.is_ascii_digit() {
                return 0x200 + c as u32;
            }
            let base = fold_accent(c.to_lowercase().next().unwrap_or(c));
            if base.is_ascii_lowercase() {
                0x300 + base as u32
            } else {
                0x1000 + base as u32
            }
        })
        .collect()
}

fn fold_accent(c: char) -> char {
    match c {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        c => c,
    }
}

/// `LIKE '%x%'` with `HkDao.like`'s escapes, ignoring case as the collation does.
pub fn like(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Java: `HkDao.auditLog` / `auditCount` without the paging: newest first.
pub fn audit_log(
    world: &World,
    target: Option<(&str, i64)>,
    filter: Option<&str>,
) -> Vec<AuditRow> {
    let name = |id: i64| {
        u32::try_from(id)
            .ok()
            .and_then(|id| world.store.user(id).ok().flatten())
            .map(|u| u.username)
            .unwrap_or_default()
    };
    let mut rows: Vec<AuditRow> = world
        .store
        .records::<HkAudit>()
        .unwrap_or_default()
        .into_iter()
        .map(|h| AuditRow {
            created_at: h.created_at,
            staff_id: h.staff_id as i64,
            action: h.action,
            target_type: h.target_type,
            target_id: h.target_id as i64,
            target_name: h.target_name,
            details: h.details,
            staff_name: String::new(),
        })
        .collect();
    // Newest first within a second too (the sort below is by second and stable): each log
    // reversed, the panel's before the game's.
    rows.reverse();
    for o in world.store.audit_log().unwrap_or_default().into_iter().rev() {
        rows.push(AuditRow {
            created_at: o.created_at,
            staff_id: o.user_id as i64,
            action: o.action,
            target_type: "user".into(),
            target_id: o.target_id as i64,
            target_name: name(o.target_id as i64),
            details: format!("{} {}", o.message, o.extra_notes)
                .trim()
                .to_string(),
            staff_name: String::new(),
        });
    }
    for r in rows.iter_mut() {
        r.staff_name = name(r.staff_id);
    }
    rows.retain(|r| {
        target.is_none_or(|(t, id)| r.target_type == t && r.target_id == id)
            && filter.is_none_or(|f| {
                f.is_empty()
                    || like(&r.staff_name, f)
                    || like(&r.target_name, f)
                    || like(&r.action, f)
            })
    });
    rows.sort_by_key(|r| std::cmp::Reverse(r.created_at));
    rows
}

/// Java: `HkPager`.
pub struct Pager {
    pub page: i64,
    pub size: i64,
    pub total: i64,
    base_url: String,
    base_query: String,
}

impl Pager {
    pub fn new(conn: &Conn, base_url: &str, total: i64, size: i64) -> Self {
        let pages = ((total + size - 1) / size).max(1);
        let page = num(conn, "p", 1).min(pages).max(1);
        let mut query = String::new();
        // duckHTTPD keeps the query in a HashMap: its order.
        let pairs: Vec<(String, String)> = conn
            .get
            .keys()
            .map(|k| (k.clone(), conn.get.string(k)))
            .collect();
        for (k, v) in super::data::java_hashmap_order(pairs) {
            if k == "p" || v.is_empty() {
                continue;
            }
            query.push_str(&format!("{}={}&", url_encode(&k), url_encode(&v)));
        }
        Pager {
            page,
            size,
            total,
            base_url: base_url.into(),
            base_query: query,
        }
    }

    pub fn pages(&self) -> i64 {
        ((self.total + self.size - 1) / self.size).max(1)
    }

    pub fn offset(&self) -> i64 {
        (self.page - 1) * self.size
    }

    pub fn slice<T: Clone>(&self, all: &[T]) -> Vec<T> {
        let from = (self.offset().max(0) as usize).min(all.len());
        let to = (from + self.size as usize).min(all.len());
        all[from..to].to_vec()
    }

    fn url(&self, target: i64) -> String {
        format!("{}?{}p={target}", self.base_url, self.base_query)
    }

    pub fn value(&self) -> Value {
        let pages = self.pages();
        let mut window: Vec<i64> = Vec::new();
        for i in 1..=pages {
            if i == 1 || i == pages || (i - self.page).abs() <= 2 {
                window.push(i);
            } else if window.last().is_some_and(|l| *l != 0) {
                window.push(0);
            }
        }
        let base = self.url(0);
        let base = base[..base.len() - 1].to_string();
        Map::new()
            .with("page", self.page)
            .with("size", self.size)
            .with("total", self.total)
            .with("pages", pages)
            .with("offset", self.offset())
            .with("hasPrev", self.page > 1)
            .with("hasNext", self.page < pages)
            .with(
                "from",
                if self.total == 0 {
                    0
                } else {
                    self.offset() + 1
                },
            )
            .with("to", self.total.min(self.offset() + self.size))
            .with("prevUrl", self.url(self.page - 1))
            .with("nextUrl", self.url(self.page + 1))
            .with("window", Value::list(window))
            .with_fn("url", move |a| {
                Ok(Value::from(format!(
                    "{base}{}",
                    a.first().map(Value::render).unwrap_or_default()
                )))
            })
            .into()
    }
}

/// Java's `URLEncoder.encode(s, UTF_8)`.
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Java: `HousekeepingBinder`: the `hk` variable of every panel page.
pub fn binder(conn: &mut Conn, tpl: &mut Tpl) {
    let world: Arc<World> = conn.web.world.clone();
    let logged_in = conn.session().boolean(LOGGED_IN);
    let token = token(conn);
    let route = conn.route.clone();
    let staff: Option<User> = if tpl.get("playerDetails").is_some() || logged_in {
        staff(conn)
    } else {
        None
    };
    let base = path("");
    let lang = conn.hk_lang().0;
    let rank_name = staff
        .as_ref()
        .map(|s| super::i18n::tr(lang, &rank_label(&Value::Int(s.rank as i64))))
        .unwrap_or_default();
    let staff_for_can = staff.clone();
    let on_route = route.clone();
    let on_base = base.clone();
    let users_online = conn.web.watchdog.snapshot().users_online;
    tpl.set(
        "hk",
        Map::new()
            .with("token", token)
            .with("path", base)
            .with("route", route)
            .with("loggedIn", logged_in)
            .with(
                "staff",
                staff.as_ref().map(|s| beans::player_details(&world, s)),
            )
            .with_fn("can", move |a| {
                let p = a.first().map(Value::render).unwrap_or_default();
                Ok(Value::Bool(can(staff_for_can.as_ref(), &p)))
            })
            .with_fn("on", move |a| {
                let section = a.first().map(Value::render).unwrap_or_default();
                if section.is_empty() || section == "/" {
                    return Ok(Value::Bool(
                        on_route == on_base || on_route == format!("{on_base}/"),
                    ));
                }
                let full = format!("{on_base}{section}");
                Ok(Value::Bool(
                    on_route == full || on_route.starts_with(&format!("{full}/")),
                ))
            })
            .with("serverOnline", true)
            .with("usersOnline", users_online)
            .with("rankName", rank_name)
            .with_fn("actionLabel", move |a| {
                Ok(Value::from(super::i18n::tr(
                    lang,
                    &action_label(&a.first().map(Value::render).unwrap_or_default()),
                )))
            })
            .with_fn("rankLabel", move |a| {
                Ok(Value::from(super::i18n::tr(
                    lang,
                    &rank_label(a.first().unwrap_or(&Value::Null)),
                )))
            }),
    );
}
