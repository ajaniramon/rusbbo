//! Java: `ClubController`, `HabboClubHabblet`, `CreditsController`.

use chrono::{Datelike, Local, TimeZone};
use rusbbo_pebble::{Map, Value};

use super::xss_clear;
use crate::game::club::choice_data;
use crate::web::beans::{has_club, now};
use crate::web::tpl::Tpl;
use crate::web::{Conn, WebResult, data, jdate};

/// Java: `ClubSubscription.giftOrder` (the club sofa comes first on the website).
const GIFT_ORDER: [&str; 22] = [
    "hc_tv",
    "hcamme",
    "hc_crtn",
    "mocchamaster",
    "hc_crpt",
    "edicehc",
    "hc_wall_lamp",
    "doorD",
    "deal_hcrollers",
    "hcsohva",
    "hc_bkshlf",
    "hc_lmp",
    "hc_trll",
    "hc_tbl",
    "hc_machine",
    "hc_chr",
    "hc_rntgn",
    "hc_dsk",
    "hc_djset",
    "hc_lmpst",
    "hc_frplc",
    "hc_btlr",
];

fn club_days(conn: &Conn, tpl: &mut Tpl) -> WebResult {
    if !conn.is_authenticated() {
        return Ok(());
    }
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Ok(());
    };
    if has_club(&user) {
        tpl.set("hcDays", (user.club_expiration - now()) / 86_400);
        let days = data::statistic(conn.world(), user.id, "club_member_time") / 86_400;
        tpl.set("hcSinceMonths", if days > 0 { days / 31 } else { 0 });
    }
    Ok(())
}

fn choices(tpl: &mut Tpl) {
    for i in 1..=3 {
        let (credits, days) = choice_data(i);
        tpl.set(&format!("clubChoiceCredits{i}"), credits);
        tpl.set(&format!("clubChoiceDays{i}"), days);
    }
}

/// `/credits/club`.
pub fn club(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    conn.session().set("page", "credits");
    render_club(conn)
}

/// `/club` (Java: `ProfileController.club`).
pub fn profile_club(conn: &mut Conn) -> WebResult {
    conn.session().set("page", "me");
    render_club(conn)
}

/// Java: `ClubController.renderclub`.
fn render_club(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    let mut tpl = conn.template("club");
    if conn.is_authenticated()
        && let Some(user) = conn.world().store.user(conn.user_id())?
    {
        super::me::count_member_days(conn.world(), &user);
    }
    choices(&mut tpl);
    club_days(conn, &mut tpl)?;
    let month = if conn.session().contains("lastClubGiftMonth") {
        conn.session().int("lastClubGiftMonth")
    } else {
        1
    };
    append_gift_data(conn, &mut tpl, month);
    conn.render(tpl)
}

/// `/habblet/ajax/habboclub_gift`.
pub fn club_gift(conn: &mut Conn) -> WebResult {
    let numeric = |s: String| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !conn.post.contains("month")
        || !conn.post.contains("catalogpage")
        || !numeric(conn.post.string("month"))
        || !numeric(conn.post.string("catalogpage"))
    {
        conn.send_html("");
        return Ok(());
    }
    let month = conn.post.int("month").unwrap_or(1);
    let mut tpl = conn.template("habblet/habboclubgift");
    append_gift_data(conn, &mut tpl, month);
    conn.render(tpl)
}

/// Java: `ClubController.appendgiftdata`.
fn append_gift_data(conn: &mut Conn, tpl: &mut Tpl, month: i32) {
    xss_clear(conn);
    let order: Vec<&str> = std::iter::once("club_sofa").chain(GIFT_ORDER).collect();
    let mut position = month - 1;
    if position >= order.len() as i32 {
        position = 0;
    }
    let sprite = usize::try_from(position)
        .ok()
        .and_then(|p| order.get(p))
        .copied()
        .unwrap_or(order[0]);
    let page_group = match month {
        5..=8 => 1,
        9..=12 => 2,
        13..=16 => 3,
        17..=20 => 4,
        21..=23 => 5,
        _ => 0,
    };
    let pages: Vec<i32> = match page_group {
        0 => vec![1, 2, 3, 4, 5],
        1 => vec![5, 6, 7, 8, 9],
        2 => vec![9, 10, 11, 12, 13],
        3 => vec![13, 14, 15, 16, 17],
        4 => vec![17, 18, 19, 20, 21],
        _ => vec![19, 20, 21, 22, 23],
    };
    tpl.set("pages", Value::list(pages));
    tpl.set("currentPage", month);
    tpl.set("lastPage", order.len());
    let item = conn
        .world()
        .catalogue
        .item_by_sale_code(sprite)
        .and_then(|c| {
            c.definition
                .clone()
                .or_else(|| c.packages().first().and_then(|p| p.definition.clone()))
        });
    tpl.set(
        "item",
        item.map(|d| {
            Value::from(
                Map::new()
                    .with("id", d.record.id)
                    .with("name", d.record.name.as_str())
                    .with("sprite", d.record.sprite.as_str()),
            )
        }),
    );
    conn.session().set("lastClubGiftMonth", month);
}

/// `/credits/club/tryout`.
pub fn club_tryout(conn: &mut Conn) -> WebResult {
    let mut tpl = conn.template("club_tryout");
    choices(&mut tpl);
    if conn.is_authenticated() {
        club_days(conn, &mut tpl)?;
        if let Some(user) = conn.world().store.user(conn.user_id())? {
            tpl.set("figure", user.figure.as_str());
            tpl.set("sex", user.sex.as_str());
        }
    }
    conn.session().set("page", "credits");
    conn.render(tpl)
}

/// `/habboclub/habboclub_confirm`.
pub fn confirm(conn: &mut Conn) -> WebResult {
    let option: i32 = conn.post.string("optionNumber").parse().unwrap_or(1);
    if !(0..=4).contains(&option) {
        return Ok(());
    }
    let (credits, days) = choice_data(option);
    let mut tpl = conn.template("habblet/habboClubConfirm");
    tpl.set("clubCredits", credits);
    tpl.set("clubDays", days);
    tpl.set("clubType", option);
    conn.render(tpl)
}

/// `/habboclub/habboclub_subscribe`.
pub fn subscribe(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        return Ok(());
    }
    let option: i32 = conn.post.string("optionNumber").parse().unwrap_or(1);
    let (credits, _) = choice_data(option);
    let mut tpl = conn.template("habblet/habboClubSubscribe");
    let world = conn.web.world.clone();
    let Some(mut user) = world.store.user(conn.user_id())? else {
        return Ok(());
    };
    if user.credits < credits {
        tpl.set(
            "subscribeMsg",
            "You don't have enough credits to complete the subscription purchase.",
        );
    } else {
        tpl.set(
            "subscribeMsg",
            format!(
                "Congratulations! You have successfully subscribed to {} Club.",
                data::cfg_string(&world, "site.name")
            ),
        );
        let first_time = user.club_subscribed == 0;
        crate::game::club::subscribe_club_user(&world, &mut user, option);
        let gift_seconds = club_gift_seconds(&world);
        data::set_statistic(
            &world,
            user.id,
            "club_member_time_updated",
            now() + gift_seconds,
        );
        if user.is_online || world.player_by_id(user.id).is_some() {
            crate::rcon::send(&world, "refresh_club", &[("userId", user.id.to_string())]);
            if first_time {
                crate::rcon::send(&world, "refresh_hand", &[("userId", user.id.to_string())]);
            }
        }
    }
    conn.render(tpl)
}

/// Java: `ClubSubscription.getClubGiftSeconds`: `club.gift.interval` in `club.gift.timeunit`.
fn club_gift_seconds(world: &crate::world::World) -> i64 {
    let interval = data::cfg_int(world, "club.gift.interval");
    let unit = match data::cfg_string(world, "club.gift.timeunit").as_str() {
        "SECONDS" => 1,
        "MINUTES" => 60,
        "HOURS" => 3_600,
        _ => 86_400,
    };
    interval * unit
}

/// `/habblet/ajax/habboclub_enddate`.
pub fn end_date(conn: &mut Conn) -> WebResult {
    let mut tpl = conn.template("habblet/habboClubEnddate");
    if conn.is_authenticated()
        && let Some(user) = conn.world().store.user(conn.user_id())?
        && has_club(&user)
    {
        tpl.set("hcDays", (user.club_expiration - now()) / 86_400);
    }
    conn.render(tpl)
}

/// `/habboclub/habboclub_reminder_remove`.
pub fn reminder_remove(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        return Ok(());
    }
    let id = conn.user_id();
    for mut a in conn
        .world()
        .store
        .records_where::<rusbbo_storage::Alert>(|a| {
            a.user_id == id && a.alert_type == "HC_EXPIRED"
        })?
    {
        a.is_disabled = true;
        conn.world().store.put_record(&a)?;
    }
    conn.send_html("");
    Ok(())
}

/// Java: `XSSUtil.createKey`.
fn xss_create_key(conn: &mut Conn, route: &str) {
    use rand::RngExt;
    xss_clear(conn);
    let seed: i32 = rand::rng().random();
    let key = java_random_next_int(seed as i64);
    let mut s = conn.session();
    s.set("xssKey", key as i64);
    s.set("xssSeed", seed as i64);
    s.set("xssRequested", route);
}

/// `new java.util.Random(seed).nextInt()`.
pub fn java_random_next_int(seed: i64) -> i32 {
    let mut s = (seed ^ 0x5DEECE66D) & ((1 << 48) - 1);
    s = (s.wrapping_mul(0x5DEECE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
    (s >> 16) as i32
}

/// `/credits`.
pub fn credits(conn: &mut Conn) -> WebResult {
    xss_create_key(conn, "/credits");
    let tpl = conn.template("credits");
    conn.session().set("page", "credits");
    conn.render(tpl)
}

/// `/credits/history`: the user's transactions month by month (staff may look at anyone's).
pub fn history(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("credits_history");
    let Some(user) = conn.world().store.user(conn.user_id())? else {
        return Ok(());
    };
    let mut user_id = user.id;
    let view_all = user.rank >= 5;
    if view_all
        && conn.get.contains("userId")
        && let Some(id) = conn.get.int("userId")
    {
        user_id = id.max(0) as u32;
    }
    let today = Local::now().date_naive();
    let mut current = today;
    if conn.get.contains("period")
        && let Ok(d) = chrono::NaiveDate::parse_from_str(&conn.get.string("period"), "%Y-%m-%d")
        && Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
            .earliest()
            .is_some_and(|t| t.timestamp() > 0)
    {
        current = d;
    }
    let shift = |d: chrono::NaiveDate, months: i32| {
        let total = d.year() * 12 + d.month0() as i32 + months;
        let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
        let last = (1..=31)
            .rev()
            .find(|day| chrono::NaiveDate::from_ymd_opt(y, m, *day).is_some())
            .unwrap_or(28);
        chrono::NaiveDate::from_ymd_opt(y, m, d.day().min(last)).unwrap()
    };
    let (previous, future) = (shift(current, -1), shift(current, 1));
    let month_name = |d: chrono::NaiveDate| {
        let t = Local
            .from_local_datetime(&d.and_hms_opt(12, 0, 0).unwrap())
            .earliest()
            .map(|t| t.timestamp())
            .unwrap_or(0);
        jdate::format(t, "MMMM")
    };
    let start = Local
        .with_ymd_and_hms(current.year(), current.month(), 1, 0, 0, 0)
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or(0);
    let next = shift(
        chrono::NaiveDate::from_ymd_opt(current.year(), current.month(), 1).unwrap(),
        1,
    );
    let end = Local
        .with_ymd_and_hms(next.year(), next.month(), 1, 0, 0, 0)
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or(0);
    let mut rows: Vec<rusbbo_storage::Transaction> = conn
        .world()
        .store
        .transactions(user_id)?
        .into_iter()
        .filter(|t| t.created_at >= start && t.created_at < end && (t.is_visible || view_all))
        .collect();
    rows.sort_by_key(|t| std::cmp::Reverse(t.created_at));
    tpl.set(
        "canGoNext",
        current.month() != today.month() || current.year() != today.year(),
    );
    tpl.set("previousYear", previous.year());
    tpl.set("previousMonth", month_name(previous));
    tpl.set("previousNumericalMonth", previous.month());
    tpl.set("futureYear", future.year());
    tpl.set("futureMonth", month_name(future));
    tpl.set("futureNumericalMonth", future.month());
    conn.session().set("page", "credits");
    tpl.set("currentYear", current.year());
    tpl.set("currentMonth", month_name(current));
    tpl.set(
        "transactions",
        Value::list(
            rows.iter()
                .map(|t| {
                    let item_id = t
                        .item_id
                        .split(',')
                        .next()
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(0);
                    Value::from(
                        Map::new()
                            .with("itemId", item_id)
                            .with("description", t.description.as_str())
                            .with("costCoins", t.credit_cost)
                            .with("costPixels", t.pixel_cost)
                            .with("amount", t.amount)
                            .with(
                                "formattedDate",
                                jdate::format(t.created_at, "yyyy-MM-dd HH:mm a")
                                    .replace("am", "AM")
                                    .replace("pm", "PM")
                                    .replace('.', ""),
                            ),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    conn.render(tpl)
}
