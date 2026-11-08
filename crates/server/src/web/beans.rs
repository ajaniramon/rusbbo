//! What Havana-Web's Java objects look like to the templates.

use std::sync::Arc;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::User;

use super::Web;
use crate::world::World;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Java: `PlayerRank` names by rank id.
pub const RANKS: [&str; 9] = [
    "RANKLESS",
    "NORMAL",
    "GUIDE",
    "HOBBA",
    "SUPERHOBBA",
    "MODERATOR",
    "COMMUNITY_MANAGER",
    "HOTEL_MANAGER",
    "ADMINISTRATOR",
];

/// A `PlayerRank` value: prints as its name, `getRankId()`.
pub fn rank(id: u8) -> Value {
    let name = RANKS.get(id as usize).copied().unwrap_or("RANKLESS");
    Map::new()
        .with("rankId", id)
        .with("name", name)
        .with_fn("toString", move |_| Ok(Value::str(name)))
        .into()
}

pub fn has_club(u: &User) -> bool {
    u.club_expiration != 0 && now() < u.club_expiration
}

/// Java: `PlayerDetails` (the getters the templates use, under their bean names).
pub fn player_details(world: &World, u: &User) -> Value {
    let online = world.player_by_id(u.id).is_some() || u.is_online;
    Map::new()
        .with("id", u.id)
        .with("name", u.username.as_str())
        .with("figure", u.figure.as_str())
        .with("poolFigure", u.pool_figure.as_str())
        .with("pixels", u.pixels)
        .with("credits", u.credits)
        .with("motto", u.motto.as_str())
        .with("sex", u.sex.as_str())
        .with("tickets", u.tickets)
        .with("film", u.film)
        .with("rank", rank(u.rank))
        .with("lastOnline", u.last_online)
        .with("firstClubSubscription", u.club_subscribed)
        .with("clubExpiration", u.club_expiration)
        .with("clubSubscription", has_club(u))
        .with("soundSetting", u.sound_enabled)
        .with("allowFriendRequests", u.allow_friend_requests)
        .with("ssoTicket", u.sso_ticket.clone().unwrap_or_default())
        .with("machineId", u.machine_id.as_str())
        .with("selectedRoom", u.selected_room_id > 0)
        .with("selectedRoomId", u.selected_room_id)
        .with("dailyRespectPoints", u.daily_respect_points)
        .with("respectDay", u.respect_day.as_str())
        .with("respectPoints", u.respect_points)
        .with("email", u.email.as_str())
        .with("online", online)
        .with("joinDate", u.created_at)
        .with("totemEffectExpiry", u.totem_effect_expiry)
        .with("respectGiven", u.respect_given)
        .with("onlineStatusVisible", u.online_status_visible)
        .with("profileVisible", u.profile_visible)
        .with("wordFilterEnabled", u.wordfilter_enabled)
        .with("tradeEnabled", u.trade_enabled)
        .with("tradeBanExpiration", u.trade_ban_expiration)
        .with("tradeBanned", u.trade_ban_expiration > now())
        .with("favouriteGroupId", u.favourite_group)
        .into()
}

/// Java: `SiteBinder` (`site`).
pub fn site(web: &Web) -> Value {
    let s = web.world.settings.read();
    let get = |k: &str| s.string(k).to_string();
    let static_path = get("static.content.path");
    let online = web.world.players_online();
    Map::new()
        .with("siteName", get("site.name"))
        .with("sitePath", get("site.path"))
        .with("staticContentPath", static_path.as_str())
        .with("furniImagerPath", format!("{static_path}/c_images/furni"))
        .with("emailStaticPath", get("email.static.content.path"))
        .with(
            "emailHotelName",
            get("site.path")
                .replace("https://", "")
                .replace("http://", "")
                .replace('/', "")
                .to_uppercase(),
        )
        .with("loaderGameIp", get("loader.game.ip"))
        .with("loaderGamePort", get("loader.game.port"))
        .with("loaderMusIp", get("loader.mus.ip"))
        .with("loaderMusPort", get("loader.mus.port"))
        .with("loaderDcr", get("loader.dcr"))
        .with("loaderVariables", get("loader.external.variables"))
        .with("loaderTexts", get("loader.external.texts"))
        .with("loaderFlashBase", get("loader.flash.base"))
        .with("loaderFlashSwf", get("loader.flash.swf"))
        .with("loaderFlashTexts", get("loader.flash.external.texts"))
        .with(
            "loaderFlashVariables",
            get("loader.flash.external.variables"),
        )
        .with("loaderFlashBetaBase", get("loader.flash.beta.base"))
        .with("loaderFlashBetaSwf", get("loader.flash.beta.swf"))
        .with(
            "loaderFlashBetaTexts",
            get("loader.flash.beta.external.texts"),
        )
        .with(
            "loaderFlashBetaVariables",
            get("loader.flash.beta.external.variables"),
        )
        .with("serverOnline", true)
        .with("usersOnline", online)
        .with(
            "formattedUsersOnline",
            rusbbo_pebble::number_format(online as f64, true, "#,##0"),
        )
        .with("visits", web.world.store.web_last_visits().unwrap_or(0))
        .with("housekeepingPath", super::routes_table::HOUSEKEEPING_PATH)
        .into()
}

/// Java: `GameConfiguration` (`gameConfig`): the `settings` table.
pub fn game_config(world: &World) -> Value {
    let settings: Arc<std::collections::BTreeMap<String, String>> =
        Arc::new(world.settings.read().all().clone());
    let (a, b, c, d) = (
        settings.clone(),
        settings.clone(),
        settings.clone(),
        settings,
    );
    let arg = |args: &[Value]| args.first().map(Value::render).unwrap_or_default();
    Map::new()
        .with_fn("getString", move |args| {
            Ok(Value::from(a.get(&arg(args)).cloned().unwrap_or_default()))
        })
        .with_fn("getInteger", move |args| {
            Ok(Value::Int(
                b.get(&arg(args))
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .unwrap_or(0),
            ))
        })
        .with_fn("getLong", move |args| {
            Ok(Value::Int(
                c.get(&arg(args))
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .unwrap_or(0),
            ))
        })
        .with_fn("getBoolean", move |args| {
            Ok(Value::Bool(
                d.get(&arg(args))
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("true")),
            ))
        })
        .into()
}
