//! The 1 s game tick (Java: `game/GameScheduler.java`). Only the daily credits handout so far.

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
        tick(&world);
    }
}

fn tick(world: &World) {
    let daily_credits = world.settings.read().int("daily.credits.amount") as i32;
    // TODO(afk, pixels, catalogue index refresh, ...): the other per-tick jobs in GameScheduler.
    for session in world.players() {
        let mut p = session.player.lock();
        if !p.logged_in || !p.credits_eligible || now() <= p.next_handout || daily_credits <= 0 {
            continue;
        }
        p.credits_eligible = false;
        let Some(user) = p.details.as_mut() else {
            continue;
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
