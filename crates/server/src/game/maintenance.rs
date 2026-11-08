//! The maintenance shutdown (Java: `PlayerManager.planMaintenance`, `cancelMaintenance`,
//! `isMaintenance` and `getMaintenanceAt`): `:shutdown <minutes>` closes the hotel after that
//! long, and while the countdown runs the players are told when they log in, buy something or
//! open a trade (`INFO_HOTEL_CLOSING`).

use std::sync::OnceLock;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::messages::outgoing::alerts::alert;
use crate::messages::outgoing::handshake::info_hotel_closing;
use crate::world::World;

struct State {
    /// Java: `isMaintenanceShutdown`.
    active: bool,
    /// Java: `maintenanceAt`: the minutes the countdown was planned with (not what is left).
    minutes: i64,
    /// Java: `shutdownTimeout`; `None` until the first plan (Java's field is null then).
    timeout: Option<tokio::task::AbortHandle>,
}

static STATE: Mutex<State> = Mutex::new(State {
    active: false,
    minutes: 0,
    timeout: None,
});

fn shutdown_notify() -> &'static Notify {
    static NOTIFY: OnceLock<Notify> = OnceLock::new();
    NOTIFY.get_or_init(Notify::new)
}

/// Resolves when a planned maintenance shutdown is due (Java: its task's `System.exit(0)`,
/// which runs the shutdown hook); `main` then shuts down as on SIGTERM.
pub async fn shutdown_requested() {
    shutdown_notify().notified().await;
}

/// Java: `PlayerManager.planMaintenance`: (re)starts the countdown and tells everyone online.
/// `None` where Java throws before anything happens (`Duration.toMillis` overflows).
pub fn plan(world: &World, minutes: i64) -> Option<()> {
    let millis = minutes.checked_mul(60_000)?;
    let delay = Duration::from_millis(u64::try_from(millis).unwrap_or(0));
    let mut state = STATE.lock();
    if let Some(timeout) = state.timeout.take() {
        timeout.abort();
    }
    if let Ok(rt) = tokio::runtime::Handle::try_current() {
        let task = rt.spawn(async move {
            tokio::time::sleep(delay).await;
            shutdown_notify().notify_one();
        });
        state.timeout = Some(task.abort_handle());
    }
    state.active = true;
    state.minutes = minutes;
    drop(state);
    // Java: `Math.toIntExact` in the composer, which fails to encode a count beyond an int.
    if let Ok(minutes) = i32::try_from(minutes) {
        for session in world.players() {
            session.send(info_hotel_closing(minutes));
        }
    }
    Some(())
}

/// Java: `PlayerManager.cancelMaintenance`: stops the countdown and tells everyone. `false`
/// where Java throws (no countdown was ever planned: `shutdownTimeout` is null).
pub fn cancel(world: &World) -> bool {
    {
        let mut state = STATE.lock();
        let Some(timeout) = &state.timeout else {
            return false;
        };
        timeout.abort();
        state.active = false;
    }
    let text = world.text("maintenance_cancelled");
    for session in world.players() {
        session.send(alert(&text));
    }
    true
}

/// Java: `if (isMaintenance()) send(new INFO_HOTEL_CLOSING(getMaintenanceAt()))`: whether the
/// hotel is closing, after telling `session` so.
pub fn tell_closing(session: &crate::session::Session) -> bool {
    let minutes = {
        let state = STATE.lock();
        if !state.active {
            return false;
        }
        state.minutes
    };
    if let Ok(minutes) = i32::try_from(minutes) {
        session.send(info_hotel_closing(minutes));
    }
    true
}
