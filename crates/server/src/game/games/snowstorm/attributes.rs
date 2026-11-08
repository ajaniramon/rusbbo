//! A player's SnowStorm state (Java: `snowstorm/util/SnowStormAttributes` and
//! `SnowStormActivityState`).
//!
//! Java keeps this on `GamePlayer.getSnowStormAttributes()`; here the engine keeps it on its own
//! [`super::SnowStormPlayer`]. The timed state changes (`setActivityState`, which Java schedules
//! on `GameScheduler`) live on the engine (`SnowStormGame::set_activity_state`) because they
//! need its clock.

use crate::game::pathfinder::Position;

/// Java: `SnowStormActivityState` (`stateId`, `timer` in client frames of 60 ms).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ActivityState {
    InvincibleAfterStun,
    Stunned,
    Creating,
    #[default]
    Normal,
}

impl ActivityState {
    /// What the avatar object sends (creating shares 0 with normal).
    pub fn state_id(self) -> i32 {
        match self {
            Self::InvincibleAfterStun => 3,
            Self::Stunned => 2,
            Self::Creating | Self::Normal => 0,
        }
    }

    pub fn timer(self) -> i32 {
        match self {
            Self::InvincibleAfterStun => 60,
            Self::Stunned => 125,
            Self::Creating => 20,
            Self::Normal => 0,
        }
    }

    /// Java: `getTimeInMS`: `(timer / 5) * 300` (stunned 7500 ms, invincible 3600 ms, creating
    /// 1200 ms).
    pub fn time_in_ms(self) -> i64 {
        let timer = self.timer();
        if timer > 0 {
            i64::from((timer / 5) * 300)
        } else {
            0
        }
    }
}

/// Java: `SnowStormAttributes`. Times are milliseconds on the engine's clock (Unix epoch
/// milliseconds, like Java's `System.currentTimeMillis`, so `last_throw` and
/// `immunity_expiry` starting at 0 are in the past).
#[derive(Clone, Debug, Default)]
pub struct SnowStormAttributes {
    pub walking: bool,
    /// Java leaves it `null` until `assignSpawnPoints`.
    pub current_position: Position,
    pub walk_goal: Option<Position>,
    pub next_goal: Option<Position>,
    /// The world coordinates the client asked to walk to (sent back in every move event).
    pub goal_world_coordinates: Option<[i32; 2]>,
    pub snowballs: i32,
    pub health: i32,
    pub score: i32,
    pub rotation: i32,
    pub last_throw: i64,
    pub immunity_expiry: i64,
    pub activity_state: ActivityState,
    pub(super) state_time: i64,
    /// Java: `stateCounter` (Aeyra): a newer state invalidates the pending reset of the last.
    pub(super) state_counter: u64,
}

impl SnowStormAttributes {
    /// Java: `isWalkable` (also gates throwing and making snowballs).
    pub fn is_walkable(&self) -> bool {
        matches!(
            self.activity_state,
            ActivityState::Normal | ActivityState::InvincibleAfterStun
        )
    }

    /// Java: `isDamageable`.
    pub fn is_damageable(&self, now: i64) -> bool {
        now > self.immunity_expiry && self.activity_state == ActivityState::Normal
    }

    /// Java: `getActivityTimer`. `stateTime` is already "set time + duration", and the expiry
    /// adds the duration again, so this counts down from twice the state's length (in frames:
    /// `remaining / 300 * 5`).
    pub fn activity_timer(&self, now: i64) -> i32 {
        let expire = self.state_time + self.activity_state.time_in_ms();
        if !(now > self.state_time || self.activity_state == ActivityState::Normal) {
            let remaining = (expire - now) as i32;
            remaining / 300 * 5
        } else {
            0
        }
    }
}
