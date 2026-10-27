//! Per-connection player state and the login/logout flow (Java: `game/player/Player.java`).

use std::sync::Arc;

use havana_storage::User;
use tracing::{error, info};

use crate::game::inventory::Inventory;
use crate::game::messenger::{Messenger, manager as messenger_manager};
use crate::game::{fuserights, now, today_short};
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
    /// The private room the player may enter with `GOTOFLAT` (Java:
    /// `RoomPlayer.authenticateId`, -1 for `None`). Set by `CREATEFLAT`, `TRYFLAT` and
    /// `LETUSERIN`, cleared on entry, `QUIT` and kick.
    pub authenticate_id: Option<u32>,
    /// Loaded at login (Java: `Player.messenger`).
    pub messenger: Option<Messenger>,
}

impl Player {
    pub fn user_id(&self) -> Option<u32> {
        self.details
            .as_ref()
            .filter(|_| self.logged_in)
            .map(|d| d.id)
    }

    pub fn has_club(&self) -> bool {
        self.details
            .as_ref()
            .is_some_and(|d| d.club_expiration > now())
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

    // TODO(bans): kick banned users once the bans tables are ported (Java: details.isBanned()).

    let machine_id_valid = user.machine_id.len() == 33 && user.machine_id.starts_with('#');
    if !machine_id_valid {
        user.machine_id = p.client_machine_id.clone();
        p.save_machine_id = true;
    }
    if p.save_machine_id {
        session.send(outgoing::handshake::unique_id(&p.client_machine_id));
    }

    // TODO(ip-log): users_ip_logs. TODO(effects): users_effects (expired ones are removed here).

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

    // TODO(statistics, badges, achievements, groups): loaded here in Java.

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
        p.has_club(),
    )));
    session.send(outgoing::handshake::login());
    session.send(outgoing::user::avatar_effects());

    // TODO(alerts): welcome message (welcome.message.enabled), maintenance notice.
    // TODO(guides/tutorial): HELP_ITEMS when the
    // users_statistics.has_tutorial flag is set and tutorial.enabled is on.
    // TODO(club): member days, badges, gifts.

    info!(user = user.id, name = %user.username, "logged in");
    p.details = Some(user);
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
