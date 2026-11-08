//! Dice and the wheel of fortune (Java: `THROW_DICE`, `DICE_OFF`, `SPIN_WHEEL_OF_FORTUNE`,
//! `room/tasks/DiceTask` and `FortuneTask`). Both pick their result at random after a delay;
//! meanwhile the item has `requires_update` set and ignores everything.

use std::time::Duration;

use rand::RngExt;

use crate::game::room::Room;
use crate::game::room::items::Behaviour;
use crate::messages::outgoing::rooms_items as out;
use crate::world::World;

/// Java: `DiceTask`: 1-6, or 1-8 for the bottle.
pub fn dice_result(sprite: &str, rng: &mut impl rand::Rng) -> i32 {
    let max = if sprite == "bottle" { 8 } else { 6 };
    rng.random_range(1..=max)
}

/// Java: `FortuneTask`: 1-10.
pub fn wheel_result(rng: &mut impl rand::Rng) -> i32 {
    rng.random_range(1..=10)
}

impl Room {
    /// Java: `THROW_DICE` once the item is found: the player at `entity` throws the dice at
    /// `idx`. Only next to it, and not while it rolls; the result comes 2 s later.
    pub fn throw_dice(&mut self, world: &World, entity: usize, idx: usize) {
        let item = &self.mapping.items[idx];
        if !item.has(Behaviour::Dice) || item.requires_update {
            return;
        }
        if item
            .position
            .distance_squared(&self.entities[entity].position)
            > 1
        {
            return;
        }
        let virtual_id = item.virtual_id;
        // Java resets the room timer here too: someone rolling dice for others in a casino is
        // not idle.
        self.reset_room_timer(world, entity);
        self.send(out::dice_value(virtual_id, -1));
        self.mapping.items[idx].requires_update = true;
        self.schedule(world, Duration::from_secs(2), move |world, r| {
            let Some(idx) = r.mapping.item_index(virtual_id) else {
                return;
            };
            if !r.mapping.items[idx].requires_update {
                return;
            }
            let value = dice_result(&r.mapping.items[idx].sprite, &mut rand::rng());
            r.send(out::dice_value(virtual_id, value));
            r.set_item_status(idx, &value.to_string());
            r.mapping.items[idx].requires_update = false;
            r.save_item(world, idx);
        });
    }

    /// Java: `DICE_OFF` once the item is found: turns a dice off (0), from next to it.
    pub fn dice_off(&mut self, world: &World, entity: usize, idx: usize) {
        let item = &self.mapping.items[idx];
        if !item.has(Behaviour::Dice) {
            return;
        }
        if self.entities[entity]
            .position
            .distance_squared(&item.position)
            > 1
        {
            return;
        }
        if item.requires_update {
            return;
        }
        let virtual_id = item.virtual_id;
        self.set_item_status_saved(world, idx, "0");
        self.send(out::dice_value(virtual_id, 0));
    }

    /// Java: `SPIN_WHEEL_OF_FORTUNE` once the rights are checked: the wheel at `idx` spins (-1)
    /// and stops on its result 4.25 s later.
    pub fn spin_wheel(&mut self, world: &World, idx: usize) {
        let item = &self.mapping.items[idx];
        if !item.has(Behaviour::WheelOfFortune) || item.requires_update {
            return;
        }
        let virtual_id = item.virtual_id;
        self.set_item_status(idx, "-1");
        self.mapping.items[idx].requires_update = true;
        self.schedule(world, Duration::from_millis(4250), move |world, r| {
            let Some(idx) = r.mapping.item_index(virtual_id) else {
                return;
            };
            if !r.mapping.items[idx].requires_update {
                return;
            }
            let value = wheel_result(&mut rand::rng());
            r.set_item_status(idx, &value.to_string());
            r.mapping.items[idx].requires_update = false;
            r.save_item(world, idx);
        });
    }

    /// Java: `G_STAT`'s loop over the room's items for a user who just came in: a pool item's
    /// program (`SHOWPROGRAM`), a spinning wheel's state again (it would not spin otherwise) and
    /// a rolling dice's animation.
    pub fn busy_item_updates(&self) -> Vec<havana_protocol::Message> {
        let mut messages = Vec::new();
        for (idx, item) in self.mapping.items.iter().enumerate() {
            messages.extend(self.program_of(idx));
            if item.has(Behaviour::Invisible) || !item.requires_update {
                continue;
            }
            if item.has(Behaviour::WheelOfFortune) {
                messages.push(out::stuff_data_update(item));
            }
            if item.has(Behaviour::Dice) {
                messages.push(out::dice_value(item.virtual_id, 0));
            }
        }
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_stay_in_range() {
        let mut rng = rand::rng();
        let mut seen = [false; 11];
        for _ in 0..2000 {
            let d = dice_result("edice", &mut rng);
            assert!((1..=6).contains(&d));
            let b = dice_result("bottle", &mut rng);
            assert!((1..=8).contains(&b));
            let w = wheel_result(&mut rng);
            assert!((1..=10).contains(&w));
            seen[w as usize] = true;
        }
        assert!(seen[1..].iter().all(|s| *s), "every wheel result comes up");
    }

    #[test]
    fn dice_value_body() {
        let msg = out::dice_value(1, -1);
        assert_eq!((msg.header, msg.body.as_slice()), (90, &b"IM"[..]));
        let msg = out::dice_value(2, 0);
        assert_eq!(msg.body, b"JH");
    }
}
