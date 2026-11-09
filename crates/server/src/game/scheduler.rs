//! The 1 s game tick (Java: `game/GameScheduler.java`): the player peaks, sleep and AFK in
//! rooms, daily credits and the queue of credits to hand out.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use tracing::error;

use crate::game::now;
use crate::messages::outgoing;
use crate::session::Session;
use crate::world::World;

pub async fn run(world: Arc<World>) {
    startup_time();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        crate::game::contain_panic(format_args!("game tick"), || tick(&world));
    }
}

/// Java: `GameScheduler.tickRate`, incremented at the end of every tick.
static TICK_RATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Java: `Havana.getStartupTime`: when the game tick started.
static STARTUP: OnceLock<i64> = OnceLock::new();

/// Java: `Havana.getStartupTime`.
pub fn startup_time() -> i64 {
    *STARTUP.get_or_init(now)
}

/// Java: `GameScheduler.creditsHandoutQueue`: credits for players, handed out on the next tick
/// (the daily credits, and a game's rewards: `GameFinishTask`).
static CREDITS_QUEUE: Mutex<Vec<(Arc<Session>, i32)>> = Mutex::new(Vec::new());

/// Java: `GameScheduler.pixelsHandoutQueue`.
static PIXELS_QUEUE: Mutex<Vec<(Arc<Session>, i32)>> = Mutex::new(Vec::new());

/// Java: `TimeUnit.valueOf(unit).toSeconds(amount)` for the time units the settings use.
pub fn time_unit_seconds(unit: &str, amount: i64) -> i64 {
    amount
        * match unit {
            "SECONDS" => 1,
            "MINUTES" => 60,
            "HOURS" => 3600,
            "DAYS" => 86_400,
            _ => 0,
        }
}

/// The seconds between two pixel handouts (`pixels.received.*`).
pub fn pixels_interval(world: &World) -> i64 {
    let settings = world.settings.read();
    time_unit_seconds(
        settings.string("pixels.received.timeunit"),
        settings.int("pixels.received.interval"),
    )
}

/// Java: `GameScheduler`'s pixels for one player in a room: 15 when the interval passed (and
/// the room visit has tries left), queued once.
fn pixels_handout(session: &Arc<Session>, interval: i64) {
    let mut p = session.player.lock();
    if !p.logged_in || now() <= p.last_pixels_time || p.room_id.is_none() {
        return;
    }
    let tries = p.pixel_tries;
    p.pixel_tries -= 1;
    if tries <= 0 {
        return;
    }
    let mut queue = PIXELS_QUEUE.lock();
    if queue.iter().any(|(s, _)| s.id == session.id) {
        return;
    }
    queue.push((session.clone(), 15));
    p.last_pixels_time = now() + interval;
}

/// Java: the pixels part of the handout (`CurrencyDao.increasePixels`, then
/// `ActivityPointNotification` with `PIXELS_RECEIVED`).
fn hand_out_pixels(world: &World) {
    let queued = std::mem::take(&mut *PIXELS_QUEUE.lock());
    for (session, pixels) in queued {
        let mut p = session.player.lock();
        let Some(user) = p.details.as_mut() else {
            continue;
        };
        crate::game::currency::increase_pixels(&world.store, user, pixels);
        session.send(outgoing::user::activity_points(
            user.pixels,
            outgoing::user::PointAlert::Received,
        ));
    }
}

/// Java: `ItemManager.checkExpiredRentals`: rented items past their time leave the room or the
/// hand and are deleted (each one takes a virtual id, as Java makes an `Item` for it).
fn check_expired_rentals(world: &World) {
    let t = now();
    let expired: Vec<rusbbo_storage::Item> = match world.store.all_items() {
        Ok(items) => items
            .into_iter()
            .filter(|i| i.expire_time != -1 && i.expire_time < t)
            .collect(),
        Err(e) => {
            error!("listing rentals: {e}");
            return;
        }
    };
    for item in &expired {
        world.items.next_virtual_id();
        if item.room_id > 0 {
            if let Some(room) = world.loaded_room(item.room_id as u32) {
                let deferred = {
                    let mut r = room.lock();
                    if let Some(idx) = r
                        .mapping
                        .items
                        .iter()
                        .position(|i| i.flat.as_ref().is_some_and(|f| f.record.id == item.id))
                    {
                        r.remove_flat_item(world, idx);
                    }
                    r.take_deferred()
                };
                crate::game::room::run_deferred(world, deferred);
            }
        } else if let Some(owner) = item.user_id.and_then(|id| world.player_by_id(id)) {
            owner.player.lock().inventory.take_record(item.id);
        }
    }
    if !expired.is_empty() {
        let ids: Vec<u64> = expired.iter().map(|i| i.id).collect();
        if let Err(e) = world.store.delete_items(&ids) {
            error!("deleting rentals: {e}");
        }
    }
}

/// Java: `GameScheduler.queuePlayerCredits`.
pub fn queue_credits(session: &Arc<Session>, credits: i32) {
    CREDITS_QUEUE.lock().push((session.clone(), credits));
}

fn tick(world: &World) {
    let tick_rate = TICK_RATE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if tick_rate % 10 == 0 {
        check_player_peak(world);
    }
    // Java checks sleep and AFK per player in a room; here per room, one room lock at a time.
    for room in world.loaded_rooms().into_iter().chain(world.arenas()) {
        let deferred = {
            let mut r = room.lock();
            r.afk_tick(world);
            r.take_deferred()
        };
        crate::game::room::run_deferred(world, deferred);
    }

    let daily_credits = world.settings.read().int("daily.credits.amount") as i32;
    let interval = pixels_interval(world);
    for session in world.players() {
        daily_handout(&session, daily_credits);
        pixels_handout(&session, interval);
        crate::game::effects::expire(world, &session);
    }
    // Java: "Resend the catalogue index every 15 minutes to clear page cache".
    if tick_rate % 900 == 0 {
        for session in world.players() {
            crate::messages::incoming::catalogue::send_catalog_index(world, &session);
        }
    }
    hand_out_credits(world);
    hand_out_pixels(world);
    // Java: the club gifts that came due, every minute.
    if tick_rate % 60 == 0
        && let Err(e) = world
            .store
            .increment_club_gifts(now(), crate::game::club::club_gift_seconds(world))
    {
        error!("club gifts: {e}");
    }

    // Java: "Purge expired rows" once a day (the first tick included).
    if tick_rate % 86_400 == 0 {
        world.events.remove_expired(&world.store);
    }
    // Java: "Check expired rentals every 60 seconds".
    if tick_rate % 60 == 0 {
        check_expired_rentals(world);
    }
    // Delete expired CFH's every 60 seconds (after the handouts, as in Java).
    if tick_rate % 60 == 0 {
        world.cfh.purge_expired(world);
    }
    // Java: "Save chat messages every 60 seconds".
    if tick_rate % 60 == 0 {
        world.chat_log.save(world);
    }
    // Java: "Check xp expiry once every day", every 5 s.
    if tick_rate % 5 == 0 {
        crate::game::games::check_xp_expiry(world);
    }
    crate::game::catalogue::collectables::check_expiries(world);
}

/// Java: `GameScheduler`'s daily credits for one player: queued (unless they already wait in
/// the queue), and no more until the next handout.
fn daily_handout(session: &Arc<Session>, daily_credits: i32) {
    {
        let mut p = session.player.lock();
        if !p.logged_in || !p.credits_eligible || now() <= p.next_handout || daily_credits <= 0 {
            return;
        }
        if CREDITS_QUEUE.lock().iter().any(|(s, _)| s.id == session.id) {
            return;
        }
        p.credits_eligible = false;
    }
    queue_credits(session, daily_credits);
}

/// Java: the end of `GameScheduler.run`: the queued credits are added (`CurrencyDao
/// .increaseCredits`, keyed by player, so a player queued twice gets the last amount once), then
/// every entry turns the daily credits off in the store (`updateEligibleCredits`) and sends the
/// balance.
fn hand_out_credits(world: &World) {
    let queued = std::mem::take(&mut *CREDITS_QUEUE.lock());
    if queued.is_empty() {
        return;
    }
    let mut amounts: Vec<(Arc<Session>, i32)> = Vec::new();
    for (session, credits) in &queued {
        match amounts.iter_mut().find(|(s, _)| s.id == session.id) {
            Some(entry) => entry.1 = *credits,
            None => amounts.push((session.clone(), *credits)),
        }
    }
    for (session, credits) in &amounts {
        let mut p = session.player.lock();
        let Some(user) = p.details.as_mut() else {
            continue;
        };
        match world.store.update_user(user.id, |u| u.credits += credits) {
            Ok(Some(stored)) => user.credits = stored.credits,
            Ok(None) => {}
            Err(e) => error!(user = user.id, "handing out credits: {e}"),
        }
    }
    for (session, _) in &queued {
        let mut p = session.player.lock();
        let Some(user) = p.details.as_mut() else {
            continue;
        };
        if let Err(e) = world
            .store
            .update_user(user.id, |u| u.daily_coins_enabled = false)
        {
            error!(user = user.id, "saving daily_coins_enabled: {e}");
        }
        user.daily_coins_enabled = false;
        session.send(outgoing::user::credit_balance(user.credits));
    }
}

/// Java: `PlayerManager.checkPlayerPeak`, every 10 ticks: a new day starts the daily peak again
/// from the players online; a higher count raises either peak. Kept in the settings
/// (`players.daily.peak`, `players.daily.peak.date`, `players.all.time.peak`).
fn check_player_peak(world: &World) {
    let online = world.players_online() as i64;
    let today = crate::game::today_short();
    let (date, daily, all_time) = {
        let settings = world.settings.read();
        (
            settings.string("players.daily.peak.date").to_string(),
            settings.int("players.daily.peak"),
            settings.int("players.all.time.peak"),
        )
    };
    let save = |key: &str, value: &str| {
        if let Err(e) = world.store.put_setting(key, value) {
            error!("saving {key}: {e}");
        }
        world.settings.write().set(key, value);
    };
    if date != today {
        save("players.daily.peak", &online.to_string());
        save("players.daily.peak.date", &today);
    } else if online > daily {
        save("players.daily.peak", &online.to_string());
    }
    if online > all_time {
        save("players.all.time.peak", &online.to_string());
    }
}

/// Java: `PlayerManager.getDailyPlayerPeak` and `getAllTimePlayerPeak`.
pub fn player_peaks(world: &World) -> (i64, i64) {
    let settings = world.settings.read();
    (
        settings.int("players.daily.peak"),
        settings.int("players.all.time.peak"),
    )
}

/// Java: `GameScheduler.getInstance().getService().schedule(task, delay)`: runs `task` once after
/// `delay` on the runtime. The task takes its locks itself, briefly, following the lock order.
/// Does nothing outside a runtime (unit tests).
pub fn schedule(world: &World, delay: Duration, task: impl FnOnce(&World) + Send + 'static) {
    let (Some(world), Ok(rt)) = (world.arc(), tokio::runtime::Handle::try_current()) else {
        return;
    };
    let world = Arc::downgrade(&world);
    rt.spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if let Some(world) = world.upgrade() {
            crate::game::contain_panic(format_args!("scheduled task"), || task(&world));
        }
    });
}
