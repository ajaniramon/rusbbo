//! Hotel bans, mutes and the moderation tool (Java: `game/ban`, `game/moderation`,
//! `WordfilterManager.performBan`, `Player.isMuted`, `PlayerManager.showMutedAlert`).
//!
//! IP bans are stored and imported but, as in Java (`PlayerDetails.isBanned` and
//! `BanManager.disconnectBanAccounts` have the IP check commented out), never refuse a login.

pub mod actions;
pub mod cfh;

use std::sync::Arc;

use rusbbo_storage::{AuditLogEntry, Ban, BanType, User};
use tracing::{error, info};

use crate::game::now;
use crate::game::player::Player;
use crate::game::statistics::stat;
use crate::messages::outgoing;
use crate::session::Session;
use crate::world::World;

/// Twenty years of 365 days (Java: `TimeUnit.DAYS.toSeconds(365) * 20`).
pub const TWENTY_YEARS: i64 = 365 * 86_400 * 20;

/// Java: `PlayerDetails.isBanned`: a user id ban, then a ban on the stored machine id.
pub fn active_ban(world: &World, user_id: u32, machine_id: &str) -> Option<Ban> {
    let now = now();
    let check = |t: BanType, value: &str| {
        world.store.active_ban(t, value, now).unwrap_or_else(|e| {
            error!("reading bans: {e}");
            None
        })
    };
    check(BanType::UserId, &user_id.to_string()).or_else(|| check(BanType::MachineId, machine_id))
}

/// Java: `BanDao.addBan`.
pub fn add_ban(
    world: &World,
    ban_type: BanType,
    value: &str,
    banned_until: i64,
    message: &str,
    banned_by: i32,
) {
    let ban = Ban {
        ban_type,
        banned_value: value.to_string(),
        message: message.to_string(),
        banned_until,
        banned_at: now(),
        banned_by,
        is_active: true,
    };
    if let Err(e) = world.store.add_ban(&ban) {
        error!("saving ban: {e}");
    }
}

/// Java: `BanDao.removeBan`.
pub fn remove_ban(world: &World, ban_type: BanType, value: &str) {
    if let Err(e) = world.store.remove_ban(ban_type, value) {
        error!("removing ban: {e}");
    }
}

/// What `BanManager.disconnectBanAccounts` matches on.
#[derive(Default)]
pub struct BanCriteria {
    pub user_id: Option<u32>,
    pub machine_id: Option<String>,
}

/// Java: `BanManager.disconnectBanAccounts`: closes the first online player that matches (Java
/// breaks out of the loop after one), without `HOTEL_LOGOUT`.
pub fn disconnect_ban_accounts(world: &World, criteria: &BanCriteria) {
    for session in world.players() {
        let (user_id, machine_id) = {
            let p = session.player.lock();
            let Some(d) = p.details.as_ref() else {
                continue;
            };
            (d.id, d.machine_id.clone())
        };
        let by_id = criteria.user_id == Some(user_id);
        let by_machine = criteria
            .machine_id
            .as_deref()
            .is_some_and(|m| !machine_id.is_empty() && m == machine_id);
        if by_id || by_machine {
            session.close();
            break;
        }
    }
}

/// Java: `WordfilterManager.performBan`: a 20-year ban by the "spam filter" (`banned_by` -1)
/// and the connection closed.
pub fn perform_wordfilter_ban(world: &World, user_id: u32) {
    info!(user = user_id, "banned by the word filter");
    add_ban(
        world,
        BanType::UserId,
        &user_id.to_string(),
        now() + TWENTY_YEARS,
        "Banned for breaking the Habbo Way",
        -1,
    );
    disconnect_ban_accounts(
        world,
        &BanCriteria {
            user_id: Some(user_id),
            machine_id: None,
        },
    );
}

/// Java: `Player.isMuted`: `MUTE_EXPIRES_AT` is in the future.
pub fn is_muted(p: &Player) -> bool {
    let expires = p.statistics.long_value(stat::MUTE_EXPIRES_AT);
    expires > 0 && expires > now()
}

/// Java: `PlayerManager.showMutedAlert`.
pub fn send_muted_alert(session: &Session, p: &Player) {
    let uptime = (p.statistics.long_value(stat::MUTE_EXPIRES_AT) - now()) * 1000;
    let days = uptime / (1000 * 60 * 60 * 24);
    let hours = (uptime - days * (1000 * 60 * 60 * 24)) / (1000 * 60 * 60);
    session.send(outgoing::alerts::alert(&format!(
        "You are currently muted. Your mute will expire in {days} day(s) and {hours} hours(s)"
    )));
}

/// The muted check of `CHAT`, `SHOUT`, `WHISPER` and `MESSENGER_SENDMSG`: sends the alert and
/// returns true when the player is muted.
pub fn check_muted(session: &Session) -> bool {
    let p = session.player.lock();
    if is_muted(&p) {
        send_muted_alert(session, &p);
        true
    } else {
        false
    }
}

/// Java: `PlayerManager.getPlayerData(name)`: the online player's details, else the stored user.
pub fn player_data(world: &World, name: &str) -> Option<User> {
    if let Some(session) = world.player_by_name(name) {
        if let Some(d) = session.player.lock().details.clone() {
            return Some(d);
        }
    }
    world.store.user_by_name(name).unwrap_or_else(|e| {
        error!("reading user {name:?}: {e}");
        None
    })
}

/// The kinds of `housekeeping_audit_log` rows (Java: `ModerationActionType`, lower case).
#[derive(Clone, Copy)]
pub enum LogAction {
    AlertUser,
    KickUser,
    RoomAlert,
    RoomKick,
}

impl LogAction {
    fn name(self) -> &'static str {
        match self {
            LogAction::AlertUser => "alert_user",
            LogAction::KickUser => "kick_user",
            LogAction::RoomAlert => "room_alert",
            LogAction::RoomKick => "room_kick",
        }
    }
}

/// Java: `ModerationDao.addLog`. Java binds the target id to `user_id` and the moderator to
/// `target_id` (the parameters are swapped in the insert); kept as is.
pub fn add_log(
    world: &World,
    action: LogAction,
    user_id: u32,
    target_id: i32,
    message: &str,
    notes: &str,
) {
    let entry = AuditLogEntry {
        action: action.name().to_string(),
        user_id: target_id,
        target_id: user_id as i32,
        message: message.to_string(),
        extra_notes: notes.to_string(),
        created_at: now(),
    };
    if let Err(e) = world.store.add_audit_log(&entry) {
        error!("saving audit log: {e}");
    }
}

/// Java: `ModeratorBanUserAction.ban`, shared by the moderation tool and `:ban`. Returns the
/// text of the alert for the moderator.
pub fn ban_user(
    world: &World,
    banner: &User,
    alert_message: &str,
    name: &str,
    ban_seconds: i64,
    ban_machine_id: bool,
) -> String {
    let Some(target) = player_data(world, name) else {
        return format!("Could not find user: {name}");
    };
    if target.id == banner.id {
        return "Cannot ban yourself".to_string();
    }
    if active_ban(world, target.id, &target.machine_id).is_some() {
        return "User is already banned!".to_string();
    }
    let joined = crate::game::commands::online_group_ids(world, target.id);
    if crate::game::commands::has_permission_for(target.rank, "ban", &joined) {
        return "Cannot ban a user who has permission to ban".to_string();
    }
    let until = now() + ban_seconds;
    let banned_by = banner.id as i32;
    add_ban(
        world,
        BanType::UserId,
        &target.id.to_string(),
        until,
        alert_message,
        banned_by,
    );
    let mut criteria = BanCriteria {
        user_id: Some(target.id),
        machine_id: None,
    };
    if ban_machine_id {
        add_ban(
            world,
            BanType::MachineId,
            &target.machine_id,
            until,
            alert_message,
            banned_by,
        );
        criteria.machine_id = Some(target.machine_id.clone());
    }
    // TODO(bans): Java's IP ban here is commented out; `banIp` is read and ignored.
    if let Some(session) = world.player_by_id(target.id) {
        session.close();
    }
    disconnect_ban_accounts(world, &criteria);
    format!("The user {} has been banned.", target.username)
}

/// Sends `msg` to everyone online with `fuse` (Java: `CallForHelpManager.sendToModerators`).
pub fn send_to_fuse(world: &World, fuse: &str, msg: &rusbbo_protocol::Message) {
    for session in world.players() {
        if session.player.lock().has_fuse(fuse) {
            session.send(msg.clone());
        }
    }
}

/// Kicks `session` from the room it is in, as `RoomEntity.kick(false, true)`, then sends the
/// hotel view (the moderation tool's kicks). Returns false when the player is in no room (Java
/// throws there and the action ends).
pub fn kick_to_hotel_view(world: &World, session: &Arc<Session>) -> bool {
    let Some(room) = crate::game::room::current_room(world, session) else {
        return false;
    };
    let deferred = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return false;
        };
        r.kick(world, idx, false, true);
        r.take_deferred()
    };
    crate::game::room::run_deferred(world, deferred);
    session.send(outgoing::rooms_user::hotel_view());
    true
}

/// Java: `Room.forward(player, false)`: `ROOMFORWARD`, with the public room id offset for
/// Shockwave; a game's arena sends to its lobby. TODO(publics): hidden public rooms
/// (`followRedirect`) are not redirected.
pub fn forward(world: &World, session: &Session, room: &crate::game::room::RoomRef) {
    let (is_public, id, arena) = {
        let r = room.lock();
        (
            r.is_public(),
            r.data.id as i32,
            r.arena.as_ref().map(|a| a.game_type),
        )
    };
    if let Some(game_type) = arena {
        if let Some(lobby) = world.games.lobby_room_id(world, game_type) {
            forward_to(session, true, lobby);
        }
        return;
    }
    forward_to(session, is_public, id as u32);
}

/// `ROOMFORWARD` to a room by id (the public room offset added for Shockwave).
pub fn forward_to(session: &Session, is_public: bool, id: u32) {
    let id = id as i32;
    let id = if is_public {
        id + crate::game::navigator::PUBLIC_ROOM_OFFSET
    } else {
        id
    };
    session.send(outgoing::messenger::room_forward(is_public, id));
}
