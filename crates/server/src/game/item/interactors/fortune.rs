//! The fortune teller and the love randomizer (Java: `FortuneInteractor`,
//! `LoveRandomizerInteractor`): both show a random state after a delay. The randomizer is also a
//! seat (its `onEntityStop` is the chair's, see `Room::invoke_item`).

use std::time::Duration;

use rand::RngExt;

use super::numeric_or;
use crate::game::pathfinder::walk_direction;
use crate::game::room::Room;
use crate::game::room::actions::head_rotation;
use crate::game::room::entity::StatusType;
use crate::world::World;

/// Java: `FortuneInteractor.FORTUNE_OFF`.
pub const FORTUNE_OFF: i32 = 8;
/// Java: `FortuneInteractor.FORTUNE_NO_STATE`.
pub const FORTUNE_NO_STATE: i32 = -1;

/// Java: the fortune teller's next state: off turns it on (no state yet), anything else off.
pub fn fortune_next(custom_data: &str) -> i32 {
    if numeric_or(custom_data, FORTUNE_OFF) == FORTUNE_OFF {
        FORTUNE_NO_STATE
    } else {
        FORTUNE_OFF
    }
}

impl Room {
    /// Java: `FortuneInteractor.onInteract`: the user turns to the fortune teller (only the head
    /// when sitting); turned on, it shows one of 0-7 after 1.5 s.
    pub(crate) fn fortune_interact(&mut self, world: &World, entity: usize, idx: usize) {
        let item = &self.mapping.items[idx];
        if item.requires_update {
            return;
        }
        let next = fortune_next(&item.custom_data);
        let (towards, virtual_id) = (item.position, item.virtual_id);
        self.set_item_status_saved(world, idx, &next.to_string());

        let e = &mut self.entities[entity];
        let current = e.position;
        if e.statuses.contains(StatusType::Sit) {
            e.position.head_rotation = head_rotation(current.rotation(), &current, &towards);
        } else {
            e.position
                .set_rotation(walk_direction(current.x, current.y, towards.x, towards.y));
        }
        e.needs_update = true;

        if next == FORTUNE_NO_STATE {
            self.mapping.items[idx].requires_update = true;
            self.schedule(world, Duration::from_millis(1500), move |world, r| {
                let Some(idx) = r.mapping.item_index(virtual_id) else {
                    return;
                };
                let value = rand::rng().random_range(0..8);
                r.set_item_status(idx, &value.to_string());
                r.mapping.items[idx].requires_update = false;
                r.save_item(world, idx);
            });
        }
    }

    /// Java: `LoveRandomizerInteractor.onInteract`: spins (0) and shows one of 1-4 after 4.25 s.
    pub(crate) fn love_randomizer_interact(&mut self, world: &World, idx: usize) {
        if self.mapping.items[idx].requires_update {
            return;
        }
        let virtual_id = self.mapping.items[idx].virtual_id;
        self.set_item_status(idx, "0");
        self.mapping.items[idx].requires_update = true;
        self.schedule(world, Duration::from_millis(4250), move |world, r| {
            let Some(idx) = r.mapping.item_index(virtual_id) else {
                return;
            };
            if !r.mapping.items[idx].requires_update {
                return;
            }
            let value = rand::rng().random_range(1..5);
            r.set_item_status(idx, &value.to_string());
            r.save_item(world, idx);
            r.mapping.items[idx].requires_update = false;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fortune_states() {
        // Placed, it is off (8); on, it has no state (-1) until it shows 0-7; then off again.
        assert_eq!(fortune_next("8"), FORTUNE_NO_STATE);
        assert_eq!(fortune_next(""), FORTUNE_NO_STATE);
        assert_eq!(fortune_next("-1"), FORTUNE_NO_STATE);
        assert_eq!(fortune_next("3"), FORTUNE_OFF);
        assert_eq!(fortune_next("0"), FORTUNE_OFF);
    }
}
