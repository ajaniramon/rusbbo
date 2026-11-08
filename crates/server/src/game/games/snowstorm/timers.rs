//! The engine's clock-driven work: what Java schedules on `GameScheduler` (the 300 ms turn
//! task, each snowball's flight, the snowball machines and the activity-state resets).
//!
//! Every timer has a due time on the engine's clock and runs, in due order (ties in the order
//! they were scheduled), when the core calls `SnowStormGame::advance` with a time at or past
//! it; a timer runs with its own due time as "now", so the outcome does not depend on how
//! often the core polls. Fixed-rate timers re-arm at `due + period`, like
//! `scheduleAtFixedRate` (which also catches up on missed runs).

use std::collections::BTreeMap;

/// What a timer does when due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerKind {
    /// Java: `SnowStormGameTask.run`, every 300 ms while the arena room has its tasks.
    Turn { generation: u64 },
    /// Java: the reset scheduled by `SnowStormAttributes.setActivityState`.
    ActivityReset {
        user_id: u32,
        token: u64,
        then: Option<ResetAction>,
    },
    /// Java: `SnowballMovementTask`, at `(timeToLive * 100) / path.size()` ms.
    SnowballStep { snowball: i32, period: i64 },
    /// Java: `snowMachineAnimation` (fixed delay, 3 s).
    MachineAnimation { machine: usize, user_id: u32 },
    /// Java: `snowMachineRestock` (fixed rate, 1 s).
    MachineRestock { machine: usize, user_id: u32 },
}

/// The runnable passed to `setActivityState`, run after the state went back to normal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetAction {
    /// Java: `stunPlayerHandler`'s: invincible for 3.6 s.
    AfterStun,
    /// Java: `SnowstormCreateSnowballMessage`'s: one more snowball.
    AfterCreate,
}

pub const MACHINE_ANIMATION_MS: i64 = 3000;
pub const MACHINE_RESTOCK_MS: i64 = 1000;

#[derive(Clone, Debug, Default)]
pub struct Timers {
    queue: BTreeMap<(i64, u64), TimerKind>,
    seq: u64,
}

impl Timers {
    pub fn schedule(&mut self, due: i64, kind: TimerKind) {
        self.seq += 1;
        self.queue.insert((due, self.seq), kind);
    }

    /// The first timer due at or before `now`.
    pub fn pop_due(&mut self, now: i64) -> Option<(i64, TimerKind)> {
        let entry = self.queue.first_entry()?;
        if entry.key().0 > now {
            return None;
        }
        let ((due, _), kind) = entry.remove_entry();
        Some((due, kind))
    }

    pub fn next_deadline(&self) -> Option<i64> {
        self.queue.keys().next().map(|&(due, _)| due)
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (i64, &TimerKind)> {
        self.queue.iter().map(|(&(due, _), kind)| (due, kind))
    }
}
