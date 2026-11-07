//! Interactors that only play with an item's state (Java: `LertInteractor`,
//! `ScoreboardInteractor`, `StepLightInteractor`, `MultiHeightInteractor`).

use std::time::Duration;

use super::numeric_or;
use crate::game::room::Room;
use crate::world::World;

/// Java: `ScoreboardInteractor`'s new state for `status` (0 on/off, 1 down, 2 up), or `None`
/// when it is the default interactor's turn (2) or nothing changes.
pub fn scoreboard_next(custom_data: &str, status: i32, max_status: i32) -> Option<String> {
    match status {
        1 if max_status > 0 => {
            let next = numeric_or(custom_data, 0) - 1;
            Some(if next < 0 { 99 } else { next }.to_string())
        }
        0 if crate::game::room::items::is_numeric(custom_data) => Some("x".into()),
        0 => Some("0".into()),
        _ => None,
    }
}

impl Room {
    /// Java: `LertInteractor.onInteract`: the hockey light flashes (0, 1, 0).
    pub(crate) fn lert_interact(&mut self, world: &World, idx: usize) {
        self.set_item_status(idx, "0");
        self.set_item_status(idx, "1");
        self.set_item_status_saved(world, idx, "0");
    }

    /// Java: `ScoreboardInteractor.onInteract`: users with rights count down (status 1), up (2,
    /// the default interactor) or switch it off and on (0).
    pub(crate) fn scoreboard_interact(
        &mut self,
        world: &World,
        entity: usize,
        idx: usize,
        status: i32,
    ) {
        if !self.has_rights_or_mod(entity) {
            return;
        }
        if status == 2 {
            self.default_interact(world, idx);
        }
        let item = &self.mapping.items[idx];
        if let Some(next) = scoreboard_next(&item.custom_data, status, item.max_status()) {
            self.set_item_status(idx, &next);
        }
        self.save_item(world, idx);
    }

    /// Java: `StepLightInteractor.onEntityStep`: lights up (1) for half a second.
    pub(crate) fn step_light_step(&mut self, idx: usize) {
        let virtual_id = self.mapping.items[idx].virtual_id;
        self.set_item_status(idx, "1");
        self.schedule_later(Duration::from_millis(500), move |world, r| {
            if let Some(idx) = r.mapping.item_index(virtual_id) {
                r.set_item_status_saved(world, idx, "0");
            }
        });
    }

    /// Java: `MultiHeightInteractor.onInteract`: with nothing on top, the default interactor
    /// changes the height, and whoever stands on it moves with it.
    pub(crate) fn multi_height_interact(&mut self, world: &World, idx: usize) {
        if self.item_above(idx).is_some() {
            return;
        }
        self.default_interact(world, idx);
        let position = self.mapping.items[idx].position;
        self.mapping.readd_item_at(position, idx);
        let tiles = self.mapping.items[idx].affected_tiles();
        self.update_entities(&[], &tiles, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoreboard_states() {
        // traces/flat-vending.json: down from 0 wraps to 99; 0 switches between a number and x.
        assert_eq!(scoreboard_next("2", 1, 100).as_deref(), Some("1"));
        assert_eq!(scoreboard_next("0", 1, 100).as_deref(), Some("99"));
        assert_eq!(scoreboard_next("x", 1, 100).as_deref(), Some("99"));
        assert_eq!(scoreboard_next("1", 1, 0), None);
        assert_eq!(scoreboard_next("99", 0, 100).as_deref(), Some("x"));
        assert_eq!(scoreboard_next("x", 0, 100).as_deref(), Some("0"));
        assert_eq!(scoreboard_next("5", 2, 100), None);
        assert_eq!(scoreboard_next("5", -1, 100), None);
    }
}
