//! The 1 s game tick (Java: `game/GameScheduler.java`): sleep and AFK in rooms, daily credits.

use std::sync::Arc;
use std::time::Duration;

use tracing::error;

use crate::game::now;
use crate::messages::outgoing;
use crate::world::World;

pub async fn run(world: Arc<World>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        crate::game::contain_panic(format_args!("game tick"), || tick(&world));
    }
}

/// Java: `GameScheduler.tickRate`, incremented at the end of every tick.
static TICK_RATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn tick(world: &World) {
    let tick_rate = TICK_RATE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Java checks sleep and AFK per player in a room; here per room, one room lock at a time.
    for room in world.loaded_rooms() {
        let deferred = {
            let mut r = room.lock();
            r.afk_tick(world);
            r.take_deferred()
        };
        crate::game::room::run_deferred(world, deferred);
    }

    let daily_credits = world.settings.read().int("daily.credits.amount") as i32;
    // TODO(afk, pixels, catalogue index refresh, ...): the other per-tick jobs in GameScheduler.
    for session in world.players() {
        daily_handout(world, &session, daily_credits);
        crate::game::effects::expire(world, &session);
    }

    // Delete expired CFH's every 60 seconds (after the handouts, as in Java).
    if tick_rate % 60 == 0 {
        world.cfh.purge_expired(world);
    }
}

/// Java: `GameScheduler`'s daily credits for one player.
fn daily_handout(world: &World, session: &crate::session::Session, daily_credits: i32) {
    {
        let mut p = session.player.lock();
        if !p.logged_in || !p.credits_eligible || now() <= p.next_handout || daily_credits <= 0 {
            return;
        }
        p.credits_eligible = false;
        let Some(user) = p.details.as_mut() else {
            return;
        };
        // Java queues the handout and drains the queue later in the same tick: one write per user.
        match world.store.update_user(user.id, |u| {
            u.credits += daily_credits;
            u.daily_coins_enabled = false;
        }) {
            Ok(Some(stored)) => {
                user.credits = stored.credits;
                user.daily_coins_enabled = false;
                session.send(outgoing::user::credit_balance(user.credits));
            }
            Ok(None) => {}
            Err(e) => error!(user = user.id, "daily credits: {e}"),
        }
    }
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
