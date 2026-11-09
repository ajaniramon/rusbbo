//! The staff commands (Java: `game/commands/registered/{admin,groups,moderation}` and the staff
//! ones at the top level: `SetConfigCommand`, `TalkCommand`, `ShutdownCommand`).

use std::sync::Arc;

use rusbbo_storage::{BanType, User};
use tracing::error;

use super::{RANK_COMMUNITY_MANAGER, has_permission_for};
use crate::game::moderation::{self, LogAction};
use crate::game::room::{Room, current_room, run_deferred};
use crate::game::statistics::stat;
use crate::game::{format_decimal, long_date, now};
use crate::messages::incoming::latin1;
use crate::messages::outgoing::alerts::alert;
use crate::messages::outgoing::moderation::moderator_alert;
use crate::messages::outgoing::rooms_user::{self as out, ChatType};
use crate::session::Session;
use crate::world::World;

/// Java: `StringUtil.filterInput(String.join(" ", args), true)`.
fn joined(args: &[Vec<u8>], skip: usize) -> String {
    let parts: Vec<String> = args.iter().skip(skip).map(|a| latin1(a)).collect();
    crate::game::messenger::filter_input(&parts.join(" "), true)
}

fn arg(args: &[Vec<u8>], i: usize) -> Option<String> {
    args.get(i).map(|a| latin1(a))
}

/// Java: `StringUtil.isNumeric(s) ? Integer.parseInt(s) : 0`; `None` where `parseInt` throws
/// (a number too big for an int), which ends the command.
fn minutes_arg(s: &str) -> Option<i32> {
    if crate::game::room::items::is_numeric(s) {
        s.parse().ok()
    } else {
        Some(0)
    }
}

fn details(session: &Session) -> Option<User> {
    session.player.lock().details.clone()
}

/// Whispers `text` to the command's sender from their own avatar (Java: `player.send(new
/// CHAT_MESSAGE(WHISPER, instanceId, text, 0))`).
fn whisper_self(world: &World, session: &Arc<Session>, text: &str) {
    let Some(room) = current_room(world, session) else {
        return;
    };
    let instance_id = {
        let r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return;
        };
        r.entities[idx].instance_id
    };
    session.send(out::chat_message(
        ChatType::Whisper,
        instance_id,
        &rusbbo_protocol::body::latin1(text),
        0,
    ));
}

/// The command's sender must be in a room (Java: `getRoomUser().getRoom() == null` returns).
fn in_a_room(world: &World, session: &Session) -> bool {
    current_room(world, session).is_some()
}

/// Java: `PlayerDao.getDetails(name)`: the stored user, online or not.
fn stored_user(world: &World, name: &str) -> Option<User> {
    world.store.user_by_name(name).unwrap_or_else(|e| {
        error!("reading user {name:?}: {e}");
        None
    })
}

/// Runs `f` on the sender's room and entity, then the room's deferred work.
fn with_room<R>(
    world: &World,
    session: &Arc<Session>,
    f: impl FnOnce(&mut Room, usize) -> R,
) -> Option<R> {
    let room = current_room(world, session)?;
    let (result, deferred) = {
        let mut r = room.lock();
        let idx = r.entity_index(session.id)?;
        let result = f(&mut r, idx);
        (result, r.take_deferred())
    };
    run_deferred(world, deferred);
    Some(result)
}

/// Java: `CoordsCommand`.
pub fn coords(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    let Some(text) = with_room(world, session, |r, idx| {
        let p = r.entities[idx].position;
        format!(
            "Your coordinates:<br>Room ID: {}<br><br>X: {}<br>Y: {}<br>Z: {}<br><br>Head \
             rotation: {}<br>Body rotation: {}<br>",
            r.id(),
            p.x,
            p.y,
            format_decimal(p.z),
            p.head_rotation,
            p.body_rotation
        )
    }) else {
        return;
    };
    session.send(alert(&text));
}

/// Java: `HotelAlertCommand`: an alert to everyone online, signed by the sender.
pub fn hotel_alert(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let Some(me) = details(session) else {
        return;
    };
    let text = format!("{}<br><br>- {}", joined(args, 0), me.username);
    for player in world.players() {
        player.send(alert(&text));
    }
}

/// Java: `RoomAlertCommand`.
pub fn room_alert(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let Some(me) = details(session) else {
        return;
    };
    let text = joined(args, 0);
    let Some(players) = with_room(world, session, |r, _| {
        r.entities
            .iter()
            .filter(|e| e.is_player())
            .map(|e| e.session.clone())
            .collect::<Vec<_>>()
    }) else {
        return;
    };
    for target in players {
        target.send(moderator_alert(&text));
    }
    moderation::add_log(world, LogAction::RoomAlert, me.id, -1, &text, "");
}

/// Java: `RoomMuteCommand`.
pub fn room_mute(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    with_room(world, session, |r, idx| {
        r.is_room_muted = !r.is_room_muted;
        let text = format!(
            "The room is now {}",
            if r.is_room_muted { "muted" } else { "unmuted" }
        );
        session.send(out::chat_message(
            ChatType::Whisper,
            r.entities[idx].instance_id,
            text.as_bytes(),
            0,
        ));
    });
}

/// Java: `BanCommand`: `:ban user minutes reason...`, through `ModeratorBanUserAction.ban` with
/// the machine id.
pub fn ban(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let Some(me) = details(session) else {
        return;
    };
    let (Some(name), Some(minutes)) = (arg(args, 0), arg(args, 1)) else {
        return;
    };
    let Some(minutes) = minutes_arg(&minutes) else {
        return;
    };
    let reason = joined(args, 2);
    let response = moderation::ban_user(world, &me, &reason, &name, i64::from(minutes) * 60, true);
    session.send(alert(&response));
}

/// Java: `UnbanCommand`: lifts the user id, machine id and IP bans, and closes the user's
/// connection if they are online.
pub fn unban(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    // Java declares no argument and reads args[0]: without one it throws and nothing happens.
    let Some(name) = arg(args, 0) else {
        return;
    };
    let Some(target) = moderation::player_data(world, &name) else {
        session.send(alert(&format!("Could not find user: {name}")));
        return;
    };
    if !target.machine_id.is_empty() {
        moderation::remove_ban(world, BanType::MachineId, &target.machine_id);
    }
    let online = world.player_by_name(&name);
    let ip = match &online {
        Some(s) => Some(s.player.lock().ip_address.clone()),
        None => world.store.latest_ip(target.id).unwrap_or_default(),
    };
    // Java: ValidationUtil.isValidIpAddress.
    let ip = ip.filter(|ip| ip.parse::<std::net::IpAddr>().is_ok());
    if let Some(ip) = ip {
        moderation::remove_ban(world, BanType::IpAddress, &ip);
    }
    moderation::remove_ban(world, BanType::UserId, &target.id.to_string());
    if let Some(s) = online {
        s.close();
    }
    session.send(alert(&format!(
        "Player {} is successfully unbanned.",
        target.username
    )));
}

/// Java: `TradeBanCommand`.
pub fn trade_ban(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let (Some(name), Some(minutes)) = (arg(args, 0), arg(args, 1)) else {
        return;
    };
    let Some(minutes) = minutes_arg(&minutes) else {
        return;
    };
    // Java: (int) TimeUnit.MINUTES.toSeconds(minutes).
    let seconds = if minutes > 0 {
        (i64::from(minutes) * 60) as i32
    } else {
        0
    };
    let Some(target) = moderation::player_data(world, &name) else {
        session.send(alert(&format!("Could not find user: {name}")));
        return;
    };
    let joined = super::online_group_ids(world, target.id);
    if has_permission_for(target.rank, "ban", &joined) {
        session.send(alert("Cannot ban a user who has permission to trade ban"));
        return;
    }
    let expiration = now() + i64::from(seconds);
    if let Err(e) = world
        .store
        .update_user(target.id, |u| u.trade_ban_expiration = expiration)
    {
        error!(user = target.id, "saving trade ban: {e}");
    }
    if let Some(online) = world.player_by_id(target.id) {
        if let Some(d) = online.player.lock().details.as_mut() {
            d.trade_ban_expiration = expiration;
        }
        // The room keeps a copy of the details made at entry.
        if let Some(room) = current_room(world, &online) {
            let mut r = room.lock();
            if let Some(idx) = r.entity_index(online.id) {
                r.entities[idx].details.trade_ban_expiration = expiration;
            }
        }
    }
    session.send(alert(&format!(
        "Player ({}) trade ban time set to: {}",
        target.username,
        long_date(expiration)
    )));
}

/// Sets `MUTE_EXPIRES_AT` in the store and on the online player.
fn set_mute_expiration(world: &World, user_id: u32, expiration: i64) {
    if let Err(e) = world
        .store
        .update_statistic(user_id, stat::MUTE_EXPIRES_AT, expiration)
    {
        error!(user = user_id, "saving mute: {e}");
    }
    if let Some(online) = world.player_by_id(user_id) {
        online.player.lock().statistics.set_long_value(
            &world.store,
            stat::MUTE_EXPIRES_AT,
            expiration,
        );
    }
}

/// Java: `MuteCommand`.
pub fn mute(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let (Some(name), Some(minutes)) = (arg(args, 0), arg(args, 1)) else {
        return;
    };
    let Some(minutes) = minutes_arg(&minutes) else {
        return;
    };
    let seconds = if minutes > 0 {
        (i64::from(minutes) * 60) as i32
    } else {
        0
    };
    let Some(target) = moderation::player_data(world, &name) else {
        session.send(alert(&format!("Could not find user: {name}")));
        return;
    };
    let joined = super::online_group_ids(world, target.id);
    if has_permission_for(target.rank, "mute", &joined) {
        session.send(alert("Cannot mute a user who has permission to mute"));
        return;
    }
    let expiration = now() + i64::from(seconds);
    set_mute_expiration(world, target.id, expiration);
    session.send(alert(&format!(
        "Player ({}) mute expiration date set to: {}",
        target.username,
        long_date(expiration)
    )));
}

/// Java: `UnmuteCommand`.
pub fn unmute(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let Some(name) = arg(args, 0) else {
        return;
    };
    let Some(target) = moderation::player_data(world, &name) else {
        session.send(alert(&format!("Could not find user: {name}")));
        return;
    };
    set_mute_expiration(world, target.id, 0);
    session.send(alert(&format!(
        "Player ({}) unmute expiration date removed",
        target.username
    )));
}

/// Java: `DisconnectUserCommand`: only users of a lower rank.
pub fn disconnect(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let Some(me) = details(session) else {
        return;
    };
    let Some(name) = arg(args, 0) else {
        return;
    };
    let Some(target) = world.player_by_name(&name) else {
        session.send(alert(&format!("Could not find user: {name}")));
        return;
    };
    let Some(target_details) = details(&target) else {
        return;
    };
    if target_details.rank >= me.rank {
        session.send(alert("Insufficient perms to ban user"));
        return;
    }
    crate::game::player::kick(world, &target);
    whisper_self(
        world,
        session,
        &format!("{} disconnected", target_details.username),
    );
}

/// Java: `CheckCreditsCommand`: the online user's balance.
pub fn check_credits(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let Some(name) = arg(args, 0) else {
        return;
    };
    let target = world.player_by_name(&name).and_then(|s| details(&s));
    let text = match target {
        Some(t) => format!("{} has {} credits", t.username, t.credits),
        None => format!("Could not find user: {name}"),
    };
    whisper_self(world, session, &text);
}

/// Java: `GiveCreditsCommand` and `RemoveCreditsCommand`. The change is made on the stored row
/// (`PlayerDao.getDetails`); an online target is sent `CREDIT_BALANCE` with the balance it has in
/// memory, which Java does not update either.
fn change_credits(world: &World, session: &Arc<Session>, args: &[Vec<u8>], add: bool) {
    if !in_a_room(world, session) {
        return;
    }
    let Some(name) = arg(args, 0) else {
        return;
    };
    let Some(mut target) = stored_user(world, &name) else {
        whisper_self(world, session, &format!("Could not find user: {name}"));
        return;
    };
    let amount_text = arg(args, 1).unwrap_or_default();
    if !crate::game::room::items::is_numeric(&amount_text) {
        whisper_self(world, session, "Credit amount not provided");
        return;
    }
    let Ok(amount) = amount_text.parse::<i32>() else {
        return;
    };
    if add {
        if let Err(e) = world
            .store
            .update_user(target.id, |u| u.credits = u.credits.wrapping_add(amount))
        {
            error!(user = target.id, "adding credits: {e}");
        }
    } else {
        crate::game::currency::decrease_credits(&world.store, &mut target, amount);
    }
    if let Some(online) = world.player_by_id(target.id) {
        let credits = online.player.lock().details.as_ref().map(|d| d.credits);
        if let Some(credits) = credits {
            online.send(crate::messages::outgoing::user::credit_balance(credits));
        }
    }
    let text = if add {
        format!("{amount} credits added to user {}", target.username)
    } else {
        format!("{amount} credits removed from user {}", target.username)
    };
    whisper_self(world, session, &text);
}

pub fn give_credits(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    change_credits(world, session, args, true);
}

pub fn remove_credits(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    change_credits(world, session, args, false);
}

/// Badges no command may give or take (Java: `GL`/`ACH_` prefixes and `Z64`).
fn is_protected_badge(badge: &str) -> bool {
    badge.starts_with("GL") || badge.starts_with("ACH_") || badge.eq_ignore_ascii_case("Z64")
}

/// Java: `GiveBadgeCommand`.
pub fn give_badge(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let (Some(name), Some(badge)) = (arg(args, 0), arg(args, 1)) else {
        return;
    };
    let Some(target) = stored_user(world, &name) else {
        session.send(alert(&format!("Could not find user: {name}")));
        return;
    };
    if is_protected_badge(&badge) {
        return;
    }
    let rank_badges = world.store.rank_badges().unwrap_or_default();
    let owned_text = format!("User {} already owns this badge.", target.username);
    let rank_text = format!(
        "This badge belongs to a certain rank. If you would like to give {} this badge, increase \
         their rank.",
        target.username
    );
    let added_text = format!("Badge {badge} added to user {}", target.username);
    match world.player_by_name(&name) {
        None => {
            let mut badges = crate::game::badges::BadgeManager::load(&world.store, target.id);
            if badges.has_badge(&badge) {
                whisper_self(world, session, &owned_text);
                return;
            }
            if rank_badges.contains(&badge) {
                whisper_self(world, session, &rank_text);
                return;
            }
            badges.add_badge_offline(&world.store, &badge);
            whisper_self(world, session, &added_text);
        }
        Some(online) => {
            {
                let mut p = online.player.lock();
                if p.badges.has_badge(&badge) {
                    drop(p);
                    whisper_self(world, session, &owned_text);
                    return;
                }
                if rank_badges.contains(&badge) {
                    drop(p);
                    whisper_self(world, session, &rank_text);
                    return;
                }
                p.badges
                    .try_add_badge(&world.store, &online, &badge, None, 0);
            }
            if let Some(room) = current_room(world, &online) {
                let r = room.lock();
                if let Some(idx) = r.entity_index(online.id) {
                    r.send(crate::messages::outgoing::rooms_user::figure_change(
                        r.entities[idx].instance_id,
                        &target.figure,
                        &target.sex,
                        &target.motto,
                    ));
                }
            }
            whisper_self(world, session, &added_text);
        }
    }
}

/// Java: `RemoveBadgeCommand`: on the stored badges (an online user's loaded list keeps it until
/// they log in again, as in Java).
pub fn remove_badge(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let Some(name) = arg(args, 0) else {
        return;
    };
    let Some(target) = stored_user(world, &name) else {
        whisper_self(world, session, &format!("Could not find user: {name}"));
        return;
    };
    let Some(badge) = arg(args, 1) else {
        whisper_self(world, session, "Badge code not provided");
        return;
    };
    if is_protected_badge(&badge) {
        return;
    }
    let mut badges = crate::game::badges::BadgeManager::load(&world.store, target.id);
    if !badges.has_badge(&badge) {
        whisper_self(
            world,
            session,
            &format!("User {} does not have this badge.", target.username),
        );
        return;
    }
    badges.remove_badge(&world.store, &badge);
    whisper_self(
        world,
        session,
        &format!("Badge {badge} removed from user {}", target.username),
    );
}

/// Java: `TeleportCommand`: walking warps instead.
pub fn teleport(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    let Some(on) = with_room(world, session, |r, idx| {
        let e = &mut r.entities[idx];
        e.is_teleporting = !e.is_teleporting;
        e.is_teleporting
    }) else {
        return;
    };
    session.send(alert(&format!("Teleport mode is set to: {on}")));
}

/// Java: `ItemDebugCommand`.
pub fn item_debug(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    with_room(world, session, |r, idx| {
        let e = &mut r.entities[idx];
        session.send(out::chat_message(
            ChatType::Chat,
            e.instance_id,
            format!("Item debug has been set to {}", !e.has_item_debug).as_bytes(),
            0,
        ));
        e.has_item_debug = !e.has_item_debug;
    });
}

/// Java: `UnacceptableCommand`.
pub fn unacceptable(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    const UNACCEPTABLE: &str = "Unacceptable to hotel management";
    let Some(target) = arg(args, 0) else {
        return;
    };
    let target = target.to_lowercase();
    let Some((data, text)) = with_room(world, session, |r, _| {
        let text = match target.as_str() {
            "name" => {
                r.data.name = UNACCEPTABLE.into();
                "Set the room name to unacceptable"
            }
            "desc" | "description" => {
                r.data.description = UNACCEPTABLE.into();
                "Set the room description to unacceptable"
            }
            "both" => {
                r.data.name = UNACCEPTABLE.into();
                r.data.description = UNACCEPTABLE.into();
                "Set both the room name and description to unacceptable"
            }
            _ => return None,
        };
        Some((r.data.clone(), text))
    })
    .flatten() else {
        return;
    };
    crate::messages::incoming::rooms_settings::save(world, &data);
    session.send(alert(text));
}

/// Java: `SetConfigCommand`.
pub fn set_config(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let (Some(setting), Some(value)) = (arg(args, 0), arg(args, 1)) else {
        return;
    };
    let old = {
        let settings = world.settings.read();
        if !settings.contains(&setting) {
            drop(settings);
            session.send(alert(&format!("The setting \"{setting}\" doesn't exist!")));
            return;
        }
        settings.string(&setting).to_string()
    };
    if let Err(e) = world.store.put_setting(&setting, &value) {
        error!("saving setting {setting}: {e}");
    }
    world.reload_settings();
    session.send(alert(&format!(
        "The setting \"{setting}\" value has been updated from \"{old}\" to \"{value}\""
    )));
}

/// Java: `ReloadCommand`. The Rust server reads most of this data from its database at startup
/// and nothing else writes it while it runs, so reading it again changes nothing; what Java does
/// besides is done: `catalogue`/`shop`/`items` regenerates the sender's room's collision map and
/// sends its heightmap, and every player gets the catalogue index again; `games` rebuilds the
/// game manager ([`crate::game::games::Games::reset`]); the word filter, the room events, the
/// SnowStorm maps and the settings are read again.
pub fn reload(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let Some(room) = current_room(world, session) else {
        return;
    };
    let Some(component) = arg(args, 0) else {
        return;
    };
    let c = component.to_lowercase();
    let mut name = None;
    if matches!(c.as_str(), "catalogue" | "shop" | "items") {
        {
            let mut r = room.lock();
            r.regenerate_map();
            r.send_map();
        }
        for player in world.players() {
            crate::messages::incoming::catalogue::send_catalog_index(world, &player);
        }
        name = Some("Catalogue and item definitions");
    }
    if c == "wordfilter" {
        match world.store.wordfilter() {
            Ok(words) => world.wordfilter.reload(words),
            Err(e) => error!("reloading the word filter: {e}"),
        }
        name = Some("Wordfilter");
    }
    let simple = [
        ("commands", "Commands"),
        ("badgebuy", "Badge sale rewards"),
        ("ads", "Advertisements"),
        ("navigator", "Navigator"),
        ("texts", "Texts"),
        ("games", "Games"),
        ("events", "Events"),
        ("gamemaps", "game maps"),
        ("achievements", "Achievements"),
        ("ach", "Achievements"),
        ("models", "Room models"),
        ("collectables", "Collectables"),
        ("versions", "Furni settings"),
    ];
    for (key, component_name) in simple {
        if c == key {
            name = Some(component_name);
        }
    }
    if c == "games" {
        world.games.reset(world);
    }
    if c == "gamemaps" {
        crate::game::games::snowstorm::maps::reload();
    }
    if c == "events" {
        world.events.reload(&world.store);
    }
    if c == "settings" || c == "config" {
        world.reload_settings();
        name = Some("Game settings");
    }
    let text = match name {
        Some(n) => format!("{n} have been reloaded."),
        None => "You did not specify which component to reload!<br>You may reload either the \
                 catalogue/shop/items, advertisements, events, commands,<br>navigator, \
                 collectables, models, texts, plugins, wordfitler, games, badgebuy,<br>rewards, \
                 versions or settings."
            .to_string(),
    };
    session.send(alert(&text));
}

/// Java: `RecoverAccountCommand`: the password becomes `changeme123`.
pub fn recover_account(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    if !in_a_room(world, session) {
        return;
    }
    let Some(name) = arg(args, 0) else {
        return;
    };
    let Some(target) = stored_user(world, &name) else {
        session.send(alert("User not found"));
        return;
    };
    session.send(alert(&format!(
        "{}'s password has been reset to: changeme123",
        target.username
    )));
    match crate::game::password::hash("changeme123") {
        Ok(hash) => {
            if let Err(e) = world.store.update_user(target.id, |u| u.password = hash) {
                error!(user = target.id, "saving password: {e}");
            }
        }
        Err(e) => error!("hashing password: {e}"),
    }
}

/// Java: `TalkCommand`: a hidden spotlight whose data makes the client speak the text.
pub fn talk(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let text = if args.is_empty() {
        String::new()
    } else {
        joined(args, 0)
    };
    let Some(definition) = world.items.definition_by_sprite("spotlight") else {
        return;
    };
    let record = rusbbo_storage::Item {
        definition_id: definition.record.id,
        x: 255,
        y: 255,
        z: -1.0,
        custom_data: format!("voiceSpeak(\"{text}\")"),
        expire_time: -1,
        ..Default::default()
    };
    let owned = crate::game::item::Item {
        virtual_id: 999,
        record,
        definition,
    };
    let item = crate::game::room::items::Item::from_owned(owned);
    with_room(world, session, |r, _| {
        r.send(crate::messages::outgoing::rooms_items::place_flooritem(
            &item,
        ));
    });
}

/// Java: `PacketTestCommand`: `{0}`..`{13}` become those characters, and the frame end is added.
pub fn packet(_: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let mut packet: Vec<u8> = args.join(&b' ');
    for i in 0..14u8 {
        let token = format!("{{{i}}}");
        packet = replace_bytes(&packet, token.as_bytes(), &[i]);
    }
    packet.push(1);
    session.send_raw(packet);
}

fn replace_bytes(text: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

/// Java: `GiftRoomCommand`: a gift of a catalogue item for every user in the room below
/// community manager.
pub fn gift_room(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let Some(me) = details(session) else {
        return;
    };
    // Java reads args[0] without declaring it: no argument throws.
    let Some(sale_code) = arg(args, 0) else {
        return;
    };
    let Some(catalogue_item) = world.catalogue.item_by_sale_code(&sale_code) else {
        talk_whisper(
            world,
            session,
            &format!("Catalogue item not found: {sale_code}"),
        );
        return;
    };
    let Some(room) = current_room(world, session) else {
        return;
    };
    let players: Vec<Arc<Session>> = {
        let r = room.lock();
        r.entities
            .iter()
            .filter(|e| e.is_player() && e.details.rank < RANK_COMMUNITY_MANAGER)
            .map(|e| e.session.clone())
            .collect()
    };
    for user in players {
        let Some(user_id) = user.player.lock().user_id() else {
            continue;
        };
        let item = (|| -> anyhow::Result<Option<crate::game::item::Item>> {
            let mut w = world.store.write()?;
            let row = world.catalogue.create_gift_in(
                &mut w,
                &world.items,
                user_id,
                &me.username,
                &catalogue_item.sale_code,
                "You have just received a gift from Classic Habbo",
                "",
                now(),
            )?;
            w.commit()?;
            Ok(world.items.make_item(row))
        })();
        match item {
            Ok(Some(item)) => {
                let mut p = user.player.lock();
                p.inventory.add_item(item);
                let hand = p.inventory.get_view(&world.store, "new");
                user.send(crate::messages::outgoing::inventory::inventory(&hand));
            }
            Ok(None) => {}
            Err(e) => error!("gift: {e}"),
        }
    }
    let name = catalogue_item
        .definition
        .as_ref()
        .map(|d| d.record.name.clone())
        .unwrap_or_default();
    talk_whisper(
        world,
        session,
        &format!("Everybody (except community managers or higher) has been gifted a {name}"),
    );
}

/// Java: `getRoomUser().talk(text, WHISPER, List.of(player))`: a whisper through the room's
/// talk (spam counter, room mute), to the sender only.
fn talk_whisper(world: &World, session: &Arc<Session>, text: &str) {
    let filter = session
        .player
        .lock()
        .details
        .as_ref()
        .is_some_and(|d| d.wordfilter_enabled);
    with_room(world, session, |r, idx| {
        let receivers = vec![crate::game::room::Receiver {
            session: session.clone(),
            filter,
        }];
        r.talk(
            world,
            idx,
            &rusbbo_protocol::body::latin1(text),
            ChatType::Whisper,
            Some(receivers),
        );
    });
}

/// Java: `HeadRotateCommand.HEAD_ROTATION`.
const HEAD_ROTATION: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// Java: `HeadRotateCommand`: the head turns once round, one step every 200 ms (7 steps).
pub fn head_rotate(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    use crate::game::pathfinder::walk_direction;
    let Some(room) = current_room(world, session) else {
        return;
    };
    let Some(current) = with_room(world, session, |r, idx| r.entities[idx].position) else {
        return;
    };
    let mut start = 0;
    for (dx, dy) in HEAD_ROTATION {
        if walk_direction(current.x, current.y, current.x + dx, current.y + dy)
            == current.head_rotation
        {
            break;
        }
        start += 1;
    }
    let (Some(world), Ok(rt)) = (world.arc(), tokio::runtime::Handle::try_current()) else {
        return;
    };
    let world = Arc::downgrade(&world);
    let session_id = session.id;
    rt.spawn(async move {
        let mut turns = 8;
        let mut next_move = start + 1;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
        loop {
            interval.tick().await;
            let Some(world) = world.upgrade() else {
                return;
            };
            if turns == 1 {
                return;
            }
            let mut r = room.lock();
            let Some(idx) = r.entity_index(session_id) else {
                return;
            };
            if r.entities[idx].is_walking {
                let body = r.entities[idx].position.body_rotation;
                r.entities[idx].position.head_rotation = body;
                return;
            }
            if next_move + 1 >= HEAD_ROTATION.len() {
                next_move = 0;
            }
            let (dx, dy) = HEAD_ROTATION[next_move];
            next_move += 1;
            r.entities[idx].position.head_rotation =
                walk_direction(current.x, current.y, current.x + dx, current.y + dy);
            let model = r.model.name.clone();
            let msg = out::user_statuses([&r.entities[idx]], &model);
            r.send(msg);
            turns -= 1;
            drop(r);
            drop(world);
        }
    });
}

/// Java: `CopyRoomCommand`: a new flat for the sender with the room's name plus " (2)", model,
/// owner-name setting, door mode and decoration, and a copy of every item except teleporters;
/// then the client is sent there (`ROOMFORWARD`).
pub fn copy_room(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    use crate::game::room::items::Behaviour;
    let Some(me) = details(session) else {
        return;
    };
    let Some(room) = current_room(world, session) else {
        return;
    };
    let (data, items) = {
        let r = room.lock();
        if r.is_public() {
            return;
        }
        let items: Vec<rusbbo_storage::Item> = r
            .mapping
            .items
            .iter()
            .filter(|i| !i.has(Behaviour::Teleporter))
            .filter_map(|i| {
                let flat = i.flat.as_ref()?;
                let mut copy = rusbbo_storage::Item {
                    user_id: Some(me.id),
                    definition_id: flat.record.definition_id,
                    custom_data: i.custom_data.clone(),
                    expire_time: -1,
                    ..Default::default()
                };
                if i.has(Behaviour::WallItem) {
                    copy.wall_position = flat.record.wall_position.clone();
                } else {
                    copy.x = i.position.x;
                    copy.y = i.position.y;
                    copy.z = i.position.z;
                    copy.rotation = i.position.rotation();
                }
                Some(copy)
            })
            .collect();
        (r.data.clone(), items)
    };
    // NavigatorDao.createRoom, as CREATEFLAT, then RoomDao.saveDecorations.
    let t = now();
    let new_room = rusbbo_storage::Room {
        owner_id: me.id,
        category: 2,
        name: format!("{} (2)", data.name),
        description: String::new(),
        model: data.model.clone(),
        ccts: Some(String::new()),
        wallpaper: data.wallpaper,
        floor: data.floor,
        landscape: data.landscape.clone(),
        showname: data.showname,
        accesstype: data.accesstype,
        password: Some(String::new()),
        visitors_max: 25,
        icon_data: "0|0|".into(),
        created_at: t,
        updated_at: t,
        ..Default::default()
    };
    let room_id = match world.store.create_room(&new_room) {
        Ok(id) => id,
        Err(e) => {
            error!(owner = me.id, "copying room: {e}");
            return;
        }
    };
    let saved = world.store.write().and_then(|mut w| {
        for item in &items {
            let mut created = w.insert_item(item, t)?;
            created.room_id = room_id as i32;
            created.wall_position = item.wall_position.clone();
            created.x = item.x;
            created.y = item.y;
            created.z = item.z;
            created.rotation = item.rotation;
            w.put_item(&created)?;
        }
        w.commit()
    });
    if let Err(e) = saved {
        error!(room = room_id, "copying items: {e}");
    }
    crate::game::moderation::forward_to(session, false, room_id);
}

/// Java: `ShutdownCommand`: `cancel`, `off` or `stop` stops the maintenance countdown;
/// otherwise the hotel closes in `<minutes>` (`shutdown.minutes` without one, or when it is not a
/// number). The whispers go to the sender's instance id in their room.
pub fn shutdown(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let whisper = |text: &str| {
        let instance_id = with_room(world, session, |r, idx| r.entities[idx].instance_id);
        // Java: `getRoomUser().getInstanceId()`, -1 outside a room.
        session.send(out::chat_message(
            ChatType::Whisper,
            instance_id.unwrap_or(-1),
            text.as_bytes(),
            0,
        ));
    };
    if let Some(first) = arg(args, 0) {
        if ["cancel", "off", "stop"]
            .iter()
            .any(|w| first.eq_ignore_ascii_case(w))
        {
            // Java throws (and whispers nothing) when nothing was ever planned.
            if crate::game::maintenance::cancel(world) {
                whisper("Cancelled shutdown");
            }
            return;
        }
    }
    let default = world.settings.read().int("shutdown.minutes");
    let minutes = match arg(args, 0) {
        // Java: `Long.parseLong`.
        Some(first) => match first.parse::<i64>() {
            Ok(minutes) => minutes,
            _ => {
                whisper(&format!(
                    "Failed to parse minutes provided to shutdown command, defaulting to \
                     {default} minute(s)"
                ));
                default
            }
        },
        None => default,
    };
    if crate::game::maintenance::plan(world, minutes).is_none() {
        return;
    }
    whisper(&format!("Shutting down in {minutes} minute(s)"));
}

/// Java: `BotCommand` (`:bot <amount>`, see [`Room::bot_party`]).
pub fn bot(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let Some(room) = current_room(world, session) else {
        return;
    };
    // Java: `Integer.parseInt`, which throws on anything else.
    let Some(amount) = arg(args, 0).and_then(|a| a.parse::<i32>().ok()) else {
        return;
    };
    let deferred = {
        let mut r = room.lock();
        r.bot_party(world, amount);
        r.take_deferred()
    };
    run_deferred(world, deferred);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_placeholders() {
        assert_eq!(replace_bytes(b"BKhi{2}", b"{2}", &[2]), b"BKhi\x02");
        assert_eq!(minutes_arg("10"), Some(10));
        assert_eq!(minutes_arg("ten"), Some(0));
        assert_eq!(minutes_arg("99999999999"), None);
        assert!(is_protected_badge("ACH_Login1"));
        assert!(is_protected_badge("z64"));
        assert!(!is_protected_badge("ADM"));
    }
}
