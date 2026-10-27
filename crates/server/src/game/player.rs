//! Per-connection player state and the login/logout flow (Java: `game/player/Player.java`).

use std::sync::Arc;

use havana_storage::{Effect, User, UserAchievement};
use tracing::{error, info};

use crate::game::badges::BadgeManager;
use crate::game::inventory::Inventory;
use crate::game::java_set::JavaStringSet;
use crate::game::messenger::{Messenger, manager as messenger_manager};
use crate::game::statistics::{PlayerStatistics, stat};
use crate::game::{club, fuserights, now, today_short};
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
    /// Machine id from `UNIQUEID`, or a generated one.
    pub client_machine_id: String,
    pub save_machine_id: bool,
    /// Daily credits are due on the next game tick after `next_handout`.
    pub credits_eligible: bool,
    pub next_handout: i64,
    /// The room the player is in (Java: `getRoomUser().getRoom()`). The presence itself (position,
    /// statuses) lives in the room, see `game::room`.
    pub room_id: Option<u32>,
    /// Loaded at login (Java: `Player.messenger`).
    pub messenger: Option<Messenger>,
    /// Java: `Player.badgeManager`, loaded at login.
    pub badges: BadgeManager,
    /// Java: `Player.statisticManager`, loaded at login.
    pub statistics: PlayerStatistics,
    /// Java: `Player.achievementManager`'s rows, loaded at login.
    pub achievements: Vec<UserAchievement>,
    /// Java: `Player.effects`, loaded at login.
    pub effects: Vec<Effect>,
    /// Java: `Player.ignoredList`, a `HashSet`; filled by the first `GET_IGNORE_LIST`.
    pub ignored: JavaStringSet,
    /// Java: `Player.processLoginSteps`: the first `GETAVAILABLEBADGES` after login sends the
    /// badge list and processes the achievements; later ones do nothing.
    pub process_login_steps: bool,
    /// Java: `PlayerDetails.previousRespectDay`: the respect day as stored before this login.
    pub previous_respect_day: String,
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

/// Logs in `user` on `session` (Java: `Player.login`).
pub fn login(world: &World, session: &Arc<Session>, mut user: User) {
    // Kill other sessions of the same user before registering this one.
    for other in world.sessions_of(user.id) {
        if other.id != session.id {
            kick(world, &other);
        }
    }

    let settings = world.settings.read();
    let mut p = session.player.lock();
    p.logged_in = true;
    p.time_connected = now();
    world.add_player(session);

    user.last_online = now();
    user.is_online = true;
    let save_last_online = user.username != NO_LAST_ONLINE_USER;
    if settings.bool("reset.sso.after.login") {
        user.sso_ticket = None;
    }

    p.messenger = Some(Messenger::load(&world.store, &settings, &user));
    p.process_login_steps = true;
    p.previous_respect_day = user.respect_day.clone();

    // TODO(bans): kick banned users once the bans tables are ported (Java: details.isBanned()).

    let machine_id_valid = user.machine_id.len() == 33 && user.machine_id.starts_with('#');
    if !machine_id_valid {
        user.machine_id = p.client_machine_id.clone();
        p.save_machine_id = true;
    }
    if p.save_machine_id {
        session.send(outgoing::handshake::unique_id(&p.client_machine_id));
    }

    // TODO(ip-log): users_ip_logs.

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

    p.inventory = Inventory::load(&world.store, &world.items, user.id);

    p.statistics = PlayerStatistics::load(&world.store, user.id);
    p.badges = BadgeManager::load(&world.store, user.id);
    p.achievements = world.store.user_achievements(user.id).unwrap_or_default();
    // TODO(groups): the joined groups are loaded here in Java.

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
        |id| world.effect_durations.get(&id).copied().unwrap_or(0),
        now(),
    ));

    if settings.bool("welcome.message.enabled") {
        let text = settings
            .string("welcome.message.content")
            .replace("%username%", &user.username);
        session.send(outgoing::alerts::alert(text.as_bytes()));
    }
    // TODO(alerts): the maintenance notice (INFO_HOTEL_CLOSING).
    // TODO(alerts): the other placeholders of StringUtil.replaceAlertMessage.

    // TODO(guides): a guide has no tutorial and starts guiding; nobody is a guide yet.
    if settings.bool("tutorial.enabled") && p.statistics.int_value(stat::HAS_TUTORIAL) == 1 {
        session.send(outgoing::user::help_items(&[1, 2, 3, 4, 5, 6, 7, 8]));
    }

    info!(user = user.id, name = %user.username, "logged in");
    p.details = Some(user);
    club::count_member_days(world, &mut p);
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
    // TODO(games): stop observing and leave the game.
    let mut p = session.player.lock();
    if !p.logged_in {
        return;
    }
    p.logged_in = false;
    world.remove_player(session);
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
