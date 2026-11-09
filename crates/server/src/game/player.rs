//! Per-connection player state and the login/logout flow (Java: `game/player/Player.java`).

use std::sync::Arc;

use rusbbo_storage::{Effect, User, UserAchievement};
use tracing::{error, info};

use crate::game::badges::BadgeManager;
use crate::game::inventory::Inventory;
use crate::game::messenger::{Messenger, manager as messenger_manager};
use crate::game::statistics::{PlayerStatistics, stat};
use crate::game::{club, fuserights, moderation, now, today_short};
use crate::messages::outgoing;
use crate::session::Session;
use crate::world::World;

#[derive(Default)]
pub struct Player {
    /// The user row, once logged in.
    pub details: Option<User>,
    /// The hand (Java: `Player.getInventory`), loaded at login.
    pub inventory: Inventory,
    pub logged_in: bool,
    pub time_connected: i64,
    /// Token sent in `CRYPTO_PARAMETERS`; the key exchange will need it.
    pub crypto_token: Option<String>,
    /// Java: `Player.getDiffieHellman()`: one key pair per connection, made at the first
    /// `GENERATEKEY`.
    pub dh: Option<rusbbo_protocol::crypto::DiffieHellman>,
    /// Machine id from `UNIQUEID`, or a generated one.
    pub client_machine_id: String,
    pub save_machine_id: bool,
    /// Daily credits are due on the next game tick after `next_handout`.
    pub credits_eligible: bool,
    pub next_handout: i64,
    /// The room the player is in (Java: `getRoomUser().getRoom()`). The presence itself (position,
    /// statuses) lives in the room, see `game::room`.
    pub room_id: Option<u32>,
    /// The private room the player may enter with `GOTOFLAT` (Java:
    /// `RoomPlayer.authenticateId`, -1 for `None`). Set by `CREATEFLAT`, `TRYFLAT` and
    /// `LETUSERIN`, cleared on entry, `QUIT` and kick.
    pub authenticate_id: Option<u32>,
    /// The teleporter the player is travelling to and its room (Java:
    /// `RoomPlayer.authenticateTelporterId` / `authenticateTelporterRoomId`, -1 for `None`). Set
    /// by `TeleportInteractor`, used by the arrival in `GOTOFLAT`.
    pub authenticate_teleporter: Option<(u64, i32)>,
    /// Java: `RoomPlayer.lastGiftTime`, when the player last bought a gift.
    pub last_gift_time: i64,
    /// Loaded at login (Java: `Player.messenger`).
    pub messenger: Option<Messenger>,
    /// The room of the friend last followed with `FOLLOW_FRIEND`, until `NAVIGATE` forwards to
    /// it again (Java: `Messenger.followed`).
    pub followed_room: Option<crate::game::room::RoomRef>,
    /// Java: `Player.badgeManager`, loaded at login.
    pub badges: BadgeManager,
    /// Java: `Player.statisticManager`, loaded at login.
    pub statistics: PlayerStatistics,
    /// Java: `Player.achievementManager`'s rows, loaded at login.
    pub achievements: Vec<UserAchievement>,
    /// Java: `Player.effects`, loaded at login.
    pub effects: Vec<Effect>,
    /// Java: `Player.processLoginSteps`: the first `GETAVAILABLEBADGES` after login sends the
    /// badge list and processes the achievements; later ones do nothing.
    pub process_login_steps: bool,
    /// Java: `PlayerDetails.previousRespectDay`: the respect day as stored before this login.
    pub previous_respect_day: String,
    /// Java: `NettyPlayerNetwork.getIpAddress`, set when the connection opens.
    pub ip_address: String,
    /// Java: `RoomEntity.packetQueueAfterRoomLeave`: what the Infobus's end sends again when the
    /// player is back in the park (`InfobusParkTrigger`).
    pub packet_queue_after_room_leave: Vec<rusbbo_protocol::Message>,
    /// Java: `Player.joinedGroups` (owned and joined groups), loaded at login.
    pub joined_groups: Vec<rusbbo_storage::Group>,
    /// Java: `PlayerDetails.groupMember`, the favourite group's membership once looked up.
    pub group_member: Option<rusbbo_storage::GroupMember>,
    /// Java: `Player.guideManager`.
    pub guides: crate::game::guides::PlayerGuides,
    /// Java: `Player.lastGift`, the last club gift given this session.
    pub last_gift: Option<String>,
    /// Java: `PlayerDetails.lastPixelsTime`: no pixels before then.
    pub last_pixels_time: i64,
    /// Java: `RoomEntity.pixelAvailableTick`, reset at each room entry.
    pub pixel_tries: i64,
}

impl Player {
    pub fn user_id(&self) -> Option<u32> {
        self.details
            .as_ref()
            .filter(|_| self.logged_in)
            .map(|d| d.id)
    }

    pub fn has_club(&self) -> bool {
        self.details.as_ref().is_some_and(club::has_club)
    }

    /// Java: `Player.hasFuse`.
    pub fn has_fuse(&self, name: &str) -> bool {
        let rank = self.details.as_ref().map_or(0, |d| d.rank);
        fuserights::has_fuse(rank, self.has_club(), name)
    }
}

/// Java's special case for one account, kept for parity.
const NO_LAST_ONLINE_USER: &str = "Abigail.Ryan";

/// Sends `HOTEL_LOGOUT`, closes the connection and logs the player out (Java: `kickFromServer`).
pub fn kick(world: &World, session: &Arc<Session>) {
    session.send(outgoing::handshake::hotel_logout(
        outgoing::handshake::LogoutReason::Disconnect,
    ));
    session.close();
    dispose(world, session);
}

/// Java: `PlayerDao.login`: the user with that name (`username = ?`, case-insensitive like the
/// MariaDB collation) whose stored Argon2 hash matches the password. Used by `TRY_LOGIN` and the
/// ticket API.
pub fn check_login(world: &World, username: &str, password: &str) -> Option<User> {
    let user = world.store.user_by_name(username).unwrap_or_else(|e| {
        error!("login lookup: {e}");
        None
    })?;
    crate::game::password::matches(&user.password, password).then_some(user)
}

/// Logs in `user` on `session` (Java: `Player.login`).
pub fn login(world: &World, session: &Arc<Session>, mut user: User) {
    // Kill other sessions of the same user (Java: `disconnectSession`, then `addPlayer`); the
    // registry swaps them atomically, so of two racing logins only the later one stays.
    for other in world.register_login(session, user.id) {
        kick(world, &other);
    }

    let settings = world.settings.read();
    let mut p = session.player.lock();
    if session.is_closing() {
        // A racing login of the same user replaced this one (and its kick found nothing to log
        // out yet): stop here, so the closed connection never counts as logged in.
        world.remove_player(session);
        return;
    }
    p.logged_in = true;
    p.time_connected = now();

    user.last_online = now();
    user.is_online = true;
    let save_last_online = user.username != NO_LAST_ONLINE_USER;
    if settings.bool("reset.sso.after.login") {
        user.sso_ticket = None;
    }

    p.messenger = Some(Messenger::load(&world.store, &settings, &user));
    p.process_login_steps = true;
    p.previous_respect_day = user.respect_day.clone();

    // Java: `details.isBanned()` (user id, then the stored machine id) kicks the user. What
    // Java saved before this point (last online, the reset SSO ticket) is saved first.
    if let Some(ban) = moderation::active_ban(world, user.id, &user.machine_id) {
        info!(
            user = user.id,
            until = ban.banned_until,
            "banned user logged in: kicked"
        );
        if let Err(e) = world.store.update_user(user.id, |stored| {
            if save_last_online {
                stored.last_online = user.last_online;
                stored.is_online = true;
            }
            stored.sso_ticket = user.sso_ticket.clone();
        }) {
            error!(user = user.id, "saving login: {e}");
        }
        let online = world.players_online().to_string();
        if let Err(e) = world.store.put_setting("players.online", &online) {
            error!("saving players.online: {e}");
        }
        p.details = Some(user);
        drop(p);
        drop(settings);
        kick(world, session);
        return;
    }

    let machine_id_valid = user.machine_id.len() == 33 && user.machine_id.starts_with('#');
    if !machine_id_valid {
        user.machine_id = p.client_machine_id.clone();
        p.save_machine_id = true;
    }
    if p.save_machine_id {
        session.send(outgoing::handshake::unique_id(&p.client_machine_id));
    }

    // Java: the connection's IP is logged when it differs from the latest one.
    let latest_ip = world.store.latest_ip(user.id).unwrap_or_default();
    if latest_ip.as_deref() != Some(p.ip_address.as_str()) {
        if let Err(e) = world.store.log_ip(user.id, &p.ip_address, now()) {
            error!(user = user.id, "logging the IP address: {e}");
        }
    }

    // Java: EffectDao.removeEffects drops used-up effects, then the rest are loaded.
    let mut effects = world.store.effects(user.id).unwrap_or_default();
    let before = effects.len();
    effects.retain(|e| !(e.activated && e.expiry_date < now()));
    if effects.len() != before {
        if let Err(e) = world.store.put_effects(user.id, &effects) {
            error!(user = user.id, "removing used-up effects: {e}");
        }
    }
    p.effects = effects;

    p.credits_eligible = user.daily_coins_enabled;
    let today = today_short();
    if user.respect_day != today {
        user.respect_day = today;
        user.daily_respect_points = 3;
        p.credits_eligible = true;
        user.daily_coins_enabled = true;
    }

    // Java: PlayerDetails.resetNextHandout.
    p.next_handout = if settings.int("daily.credits.amount") > 0 {
        0
    } else {
        let unit = match settings.string("credits.scheduler.timeunit") {
            "SECONDS" => 1,
            "HOURS" => 3600,
            "DAYS" => 86_400,
            _ => 60,
        };
        now() + unit * settings.int("credits.scheduler.interval")
    };

    // Java: the next pixels are an interval away.
    p.last_pixels_time = now() + crate::game::scheduler::pixels_interval(world);
    p.inventory = Inventory::load(&world.store, &world.items, user.id);
    p.inventory.flash = session.is_flash();

    p.statistics = PlayerStatistics::load(&world.store, user.id);
    // For the game lobbies' GAMEPLAYERINFO (see `Session::game_points`).
    session.set_game_points(
        false,
        p.statistics.int_value(stat::BATTLEBALL_POINTS_ALL_TIME),
    );
    session.set_game_points(
        true,
        p.statistics.int_value(stat::SNOWSTORM_POINTS_ALL_TIME),
    );
    p.badges = BadgeManager::load(&world.store, user.id);
    p.achievements = world.store.user_achievements(user.id).unwrap_or_default();
    // Java: `refreshJoinedGroups` (the details are not on the player yet).
    p.joined_groups = crate::web::data::joined_groups(world, user.id);

    if let Err(e) = world.store.update_user(user.id, |stored| {
        if save_last_online {
            stored.last_online = user.last_online;
            stored.is_online = true;
        }
        stored.sso_ticket = user.sso_ticket.clone();
        stored.machine_id = user.machine_id.clone();
        stored.respect_day = user.respect_day.clone();
        stored.daily_respect_points = user.daily_respect_points;
        stored.daily_coins_enabled = user.daily_coins_enabled;
    }) {
        error!(user = user.id, "saving login: {e}");
    }
    let online = world.players_online().to_string();
    if let Err(e) = world.store.put_setting("players.online", &online) {
        error!("saving players.online: {e}");
    }

    session.send(outgoing::handshake::rights(&fuserights::for_player(
        user.rank,
        club::has_club(&user),
    )));
    session.send(outgoing::handshake::login());
    session.send(outgoing::user::avatar_effects(
        &p.effects,
        |id| crate::game::effects::duration(world, id),
        now(),
        session.is_flash(),
    ));
    // Java: a Flash client is told its home room (one that no longer exists is forgotten).
    if session.is_flash() {
        let mut home = user.home_room;
        if home != 0 && !crate::game::navigator::room_exists(world, home as u32) {
            home = 0;
            if let Err(e) = world.store.update_user(user.id, |u| u.home_room = 0) {
                error!(user = user.id, "saving the home room: {e}");
            }
            user.home_room = 0;
        }
        session.send(outgoing::flash::home_room(home));
    }

    if settings.bool("welcome.message.enabled") {
        let text = replace_alert_message(
            settings.string("welcome.message.content"),
            &user.username,
            session.is_flash(),
        );
        session.send(outgoing::alerts::alert(&text));
    }
    crate::game::maintenance::tell_closing(session);

    // Java: the guide checks (the first `GuideManager.getInstance()` starts its invite task).
    crate::game::guides::start(world);
    p.guides.is_guide = crate::game::guides::is_guide_user(world, &p.joined_groups, &user);
    if settings.bool("tutorial.enabled") {
        if p.guides.is_guide {
            p.guides.has_tutorial = false;
            p.guides.guiding = crate::game::guides::guided_by(world, user.id);
        } else {
            p.guides.has_tutorial = p.statistics.int_value(stat::HAS_TUTORIAL) == 1;
        }
        if p.guides.has_tutorial {
            session.send(outgoing::user::help_items(&[1, 2, 3, 4, 5, 6, 7, 8]));
        }
    }
    // Java: the guide admins' GLK and the eXperts' XXX badges go when the membership did.
    let membership = |p: &Player, setting: &str| {
        let group_id = settings.int(setting) as i32;
        crate::game::groups::joined_group(p, group_id)
            .map(|g| crate::web::data::group_member(world, g.id, g.owner_id, i64::from(user.id)))
    };
    if p.badges.has_badge("GLK")
        && let Some(member) = membership(&p, "guides.group.id")
        && !member.is_some_and(|m| m.member_rank == 2 || m.member_rank == 3)
    {
        p.badges.remove_badge(&world.store, "GLK");
    }
    if p.badges.has_badge("XXX")
        && let Some(None) = membership(&p, "habbo.experts.group.id")
    {
        p.badges.remove_badge(&world.store, "XXX");
    }

    info!(user = user.id, name = %user.username, "logged in");
    let (user_id, rank) = (user.id, user.rank);
    p.details = Some(user);
    club::count_member_days(world, &mut p);
    if session.is_flash() {
        let favourites: Vec<i32> = crate::game::navigator::favourite_rooms(world, user_id)
            .into_iter()
            .filter(|r| r.owner_id > 0)
            .map(|r| r.id as i32)
            .collect();
        session.send(outgoing::flash::favourites(
            crate::game::navigator::MAX_FAVOURITES as i32,
            &favourites,
        ));
        if rank >= crate::game::commands::RANK_MODERATOR {
            session.send(outgoing::flash::modtool());
        }
    }
    drop(p);
    drop(settings);
    // Tell the online friends (Java: Messenger.sendStatusUpdate). Locks other players, so ours
    // must be released first.
    messenger_manager::send_status_update(world, session);
}

/// Logs the player out after a disconnect or kick (Java: `Player.dispose`). Safe to call twice.
pub fn dispose(world: &World, session: &Arc<Session>) {
    // Java leaves the room first; done before taking the player's lock for the rest.
    if session.player.lock().logged_in {
        crate::game::room::leave_current_room(world, session, false);
    }
    // Java: then the games: stop observing, leave the game.
    world.games.player_disposed(world, session.id);
    let mut p = session.player.lock();
    if !p.logged_in {
        return;
    }
    p.logged_in = false;
    world.remove_player(session);
    club::count_member_days(world, &mut p);
    let logged_in = (now() - p.time_connected) as i32;
    p.statistics
        .increment_value(&world.store, stat::ONLINE_TIME, logged_in);
    crate::game::guides::try_progress(world, session, &mut p);
    crate::game::guides::check_guiding_friends(world, &p);
    let Some(user) = p.details.as_mut() else {
        return;
    };
    user.last_online = now();
    user.is_online = false;
    if user.username != NO_LAST_ONLINE_USER {
        let last_online = user.last_online;
        if let Err(e) = world.store.update_user(user.id, |u| {
            u.last_online = last_online;
            u.is_online = false;
        }) {
            error!(user = user.id, "saving logout: {e}");
        }
    }
    let online = world.players_online().to_string();
    if let Err(e) = world.store.put_setting("players.online", &online) {
        error!("saving players.online: {e}");
    }
    info!(user = user.id, "logged out");
    drop(p);
    messenger_manager::send_status_update(world, session);
}

/// Java: `StringUtil.replaceAlertMessage`: line breaks become `<br>` (`\r` for a Flash client,
/// every `<br>` included) and `%username%` the player's name.
pub fn replace_alert_message(message: &str, username: &str, flash: bool) -> String {
    let mut text = message
        .replace("\r\n", "<br>")
        .replace(['\r', '\n'], "<br>");
    if flash {
        text = text.replace("<br>", "\r");
    }
    text.replace("%username%", username)
}

#[cfg(test)]
mod alert_tests {
    use super::replace_alert_message;

    #[test]
    fn alert_message_like_java() {
        let m = "Hello\r\n%username%,\rwelcome\nback<br>%username%";
        assert_eq!(
            replace_alert_message(m, "a", false),
            "Hello<br>a,<br>welcome<br>back<br>a"
        );
        assert_eq!(
            replace_alert_message(m, "a", true),
            "Hello\ra,\rwelcome\rback\ra"
        );
    }
}
