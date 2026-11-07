//! A room's sound machine (or jukebox) and moodlight (Java: `RoomItemManager.getSoundMachine`,
//! `getMoodlight`, kept by `RoomMapping.refreshRoomItems`) and the `:rainbow` cycle of the
//! moodlight (Java: `RainbowTask`).
//!
//! Java recomputes both after every placement, move and removal and on every map regeneration;
//! here they are looked up when needed, which gives the same items.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use super::Room;
use super::items::Behaviour;
use crate::world::World;

/// Java: `RainbowTask.HEX_COLOURS`: 32 colours around the colour wheel.
static RAINBOW: LazyLock<Vec<String>> = LazyLock::new(|| {
    let frequency = 0.5;
    (0..32)
        .map(|i| {
            let i = f64::from(i);
            let red = (frequency * i).sin() * 127.0 + 128.0;
            let green = (frequency * i + 2.0).sin() * 127.0 + 128.0;
            let blue = (frequency * i + 4.0).sin() * 127.0 + 128.0;
            format!("#{:02x}{:02x}{:02x}", red as i32, green as i32, blue as i32)
        })
        .collect()
});

impl Room {
    /// Java: `getSoundMachine()`: the first floor item (on the map) that is a sound machine or
    /// a jukebox.
    pub fn sound_machine(&self) -> Option<usize> {
        self.mapping.items.iter().position(|i| {
            !i.has(Behaviour::WallItem)
                && self.mapping.tile(i.position).is_some()
                && (i.has(Behaviour::Jukebox) || i.has(Behaviour::SoundMachine))
        })
    }

    /// Java: `getMoodlight()`: the first wall item that is a room dimmer.
    pub fn moodlight(&self) -> Option<usize> {
        self.mapping
            .items
            .iter()
            .position(|i| i.has(Behaviour::WallItem) && i.has(Behaviour::Roomdimmer))
    }

    /// Java: `getTaskManager().hasTask("RainbowTask")`.
    pub fn has_rainbow(&self) -> bool {
        self.rainbow_task.is_some()
    }

    /// Java: `cancelTask("RainbowTask")` after `hasTask`: whether it was running.
    pub fn cancel_rainbow(&mut self) -> bool {
        match self.rainbow_task.take() {
            Some(task) => {
                task.abort();
                true
            }
            None => false,
        }
    }

    /// Java: `scheduleTask("RainbowTask", new RainbowTask(room), 0, seconds, SECONDS)`: the
    /// moodlight changes colour now (once the caller releases the room) and every `seconds`.
    pub fn start_rainbow(&mut self, world: &World, seconds: u64) {
        self.cancel_rainbow();
        let (Some(world), Ok(rt)) = (world.arc(), tokio::runtime::Handle::try_current()) else {
            return;
        };
        let world = Arc::downgrade(&world);
        let room = self.self_ref.clone();
        let handle = rt.spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(seconds));
            let mut colour_index = -1;
            loop {
                ticker.tick().await;
                let (Some(_world), Some(room)) = (world.upgrade(), room.upgrade()) else {
                    return;
                };
                let mut r = room.lock();
                // Cancelled while it waited for the lock.
                if r.rainbow_task.as_ref().map(|t| t.id()) != Some(tokio::task::id()) {
                    return;
                }
                if !r.rainbow_tick(&mut colour_index) {
                    r.rainbow_task = None;
                    return;
                }
            }
        });
        self.rainbow_task = Some(handle.abort_handle());
    }

    /// Java: `RainbowTask.run`: the next colour, shown but not saved. `false` when the room has
    /// no moodlight any more (the task cancels itself).
    fn rainbow_tick(&mut self, colour_index: &mut i32) -> bool {
        let Some(idx) = self.moodlight() else {
            return false;
        };
        *colour_index += 1;
        if *colour_index as usize >= RAINBOW.len() {
            *colour_index = 0;
        }
        let colour = &RAINBOW[*colour_index as usize];
        self.set_item_status(idx, &format!("2,1,1,{colour},211"));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rainbow_colours() {
        // traces/flat-moodlight.json: the first colour.
        assert_eq!(RAINBOW[0], "#80f31f");
        assert_eq!(RAINBOW.len(), 32);
        assert!(RAINBOW.iter().all(|c| c.len() == 7));
    }

    #[test]
    fn the_first_machine_and_dimmer_count() {
        use crate::game::item::roller::tests::model_a;
        use crate::game::room::items::tests::{definition, flat_item};

        let machine = || {
            definition(
                (232, "sound_machine", 1),
                (1, 1, 1.0),
                2,
                "solid,requires_rights_for_interaction,custom_data_numeric_on_off,sound_machine",
                "default",
                "0,2,4,6",
            )
        };
        let jukebox = definition(
            (352, "jukebox*1", 2),
            (1, 1, 1.0),
            2,
            "solid,jukebox,requires_rights_for_interaction",
            "default",
            "0,2,4,6",
        );
        let dimmer = || {
            definition(
                (351, "roomdimmer", 3),
                (0, 0, 0.0),
                2,
                "wall_item,roomdimmer,requires_rights_for_interaction",
                "default",
                "",
            )
        };
        let items = vec![
            flat_item(1, dimmer(), (0, 0, 0)),
            flat_item(2, machine(), (99, 99, 0)), // Off the map: not the room's machine.
            flat_item(3, jukebox, (6, 5, 0)),
            flat_item(4, machine(), (8, 5, 0)),
            flat_item(5, dimmer(), (0, 0, 0)),
        ];
        let mut room = Room::for_test(1, model_a(), items);
        assert_eq!(
            room.sound_machine()
                .map(|i| room.mapping.items[i].virtual_id),
            Some(3)
        );
        assert_eq!(
            room.moodlight().map(|i| room.mapping.items[i].virtual_id),
            Some(1)
        );
        room.mapping.items.remove(0);
        assert_eq!(
            room.moodlight().map(|i| room.mapping.items[i].virtual_id),
            Some(5)
        );
        let mut index = -1;
        assert!(room.rainbow_tick(&mut index));
        assert_eq!(room.mapping.items[3].custom_data, "2,1,1,#80f31f,211");
        room.mapping.items.remove(3);
        assert!(!room.rainbow_tick(&mut index), "no dimmer: the cycle ends");
    }
}
