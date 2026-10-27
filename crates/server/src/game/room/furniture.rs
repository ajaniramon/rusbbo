//! Flat furniture in a loaded room (Java: `RoomMapping.addItem/moveItem/pickupItem/removeItem`,
//! `resetExtraData` and `handleItemAdjustment`, `RoomItemManager`, `Item.isValidMove`,
//! `Item.updateEntities` and `DefaultInteractor`).
//!
//! Items are saved as soon as they change; Java queues `Item.save()`/`delete()` on the
//! `GameScheduler` and flushes them every few seconds. Nothing on the wire depends on the delay
//! while the room stays loaded, and a saved item that was deleted meanwhile stays deleted
//! ([`havana_storage::Store::update_item`]), as Java's `UPDATE` does.

use tracing::error;

use crate::game::pathfinder::Position;
use crate::game::room::Room;
use crate::game::room::items::{Behaviour, Interaction, Item, is_numeric};
use crate::game::room::mapping::RoomMapping;
use crate::messages::outgoing::{rooms as out_rooms, rooms_items as out};
use crate::world::World;

/// Java: `GameConfiguration.getInteger("stack.height.limit")`.
fn stack_height_limit(world: &World) -> f64 {
    world.settings.read().int("stack.height.limit") as f64
}

/// Java: `RoomMapping.resetExtraData(item, roomLoad)`: puts transient states back. Returns
/// whether the item changed and needs saving.
pub fn reset_extra_data(item: &mut Item, _room_load: bool) -> bool {
    // TODO(dice): DICE sets requiresUpdate to false (the HC dice defaults are commented out in
    // Java too).
    if !item.has(Behaviour::WallItem) {
        let rotations = item.allowed_rotations();
        if !rotations.is_empty() && !rotations.contains(&item.position.rotation()) {
            let first = rotations[0];
            item.position.set_rotation(first);
            return true;
        }
    }
    use Interaction::*;
    let interaction = item.interaction;
    let resets = [
        (interaction == Lert, "0"),
        (interaction == IdolVoteChair, "0"),
        (interaction == IdolScoreboard, "-1"),
        (item.has(Behaviour::OneWayGate), "0"),
        (interaction == Fortune, "8"), // FortuneInteractor.FORTUNE_OFF
        (interaction == VendingMachine, "0"),
        (interaction == LoveRandomizer, "0"),
        (item.has(Behaviour::Teleporter), "0"), // TELEPORTER_CLOSE
    ];
    for (applies, value) in resets {
        if applies && item.custom_data != value {
            item.custom_data = value.to_string();
            return true;
        }
    }
    // TODO(rollers): isCurrentRollBlocked; TODO(items): requiresUpdate of the wheel of fortune,
    // vending machines and love randomizers.
    false
}

impl Room {
    /// Java: `ItemDao.getRoomItems(roomData)` (each row gets a fresh virtual id) followed by
    /// `RoomItemManager.resetItemStates`.
    pub(super) fn load_flat_items(&mut self, world: &World) {
        let rows = match world.store.room_items(self.id() as i32) {
            Ok(rows) => rows,
            Err(e) => {
                error!(room = self.id(), "loading room items: {e}");
                return;
            }
        };
        for row in rows {
            if let Some(owned) = world.items.make_item(row) {
                self.mapping.items.push(Item::from_owned(owned));
            }
        }
        self.reset_item_states(world);
    }

    /// Java: `RoomItemManager.resetItemStates`.
    pub(super) fn reset_item_states(&mut self, world: &World) {
        // TODO(games): game arenas and custom rooms keep their states.
        for i in 0..self.mapping.items.len() {
            if self.mapping.items[i].flat.is_some()
                && reset_extra_data(&mut self.mapping.items[i], true)
            {
                self.save_item(world, i);
            }
        }
    }

    /// Java: `Item.save()` (and `ItemDao.updateItemOwnership`, which writes a subset of the same
    /// row).
    pub fn save_item(&mut self, world: &World, idx: usize) {
        let item = &mut self.mapping.items[idx];
        item.sync_record();
        save_record(world, item);
    }

    /// Java: `Item.isValidMove(item, room, x, y, rotation)`; see [`is_valid_move`].
    pub fn is_valid_move(&self, world: &World, item: &Item, x: i32, y: i32, rotation: i32) -> bool {
        is_valid_move(
            &self.mapping,
            item,
            (x, y, rotation),
            stack_height_limit(world),
        )
    }

    /// Java: `RoomItemManager.hasTooMany`. TODO(sound, moodlight, idol): the one sound machine,
    /// moodlight and idol scoreboard per room (`refreshRoomItems`), and the post-it, roller and
    /// pet nest limits with their `CANNOT_PLACE_STUFF_FROM_STRIP` errors.
    pub fn has_too_many(&self, _item: &Item) -> bool {
        false
    }

    /// Java: `RoomMapping.addItem`: puts a hand item into the room and tells everyone.
    pub fn add_flat_item(&mut self, world: &World, mut item: Item) {
        let room_id = self.id() as i32;
        let owner = self.data.owner_id;
        if let Some(flat) = item.flat.as_mut() {
            flat.record.room_id = room_id;
            flat.record.user_id = Some(owner);
            flat.record.order_id = -1;
        }
        // TODO(rollers): rolling data. TODO(items): lastPlacedTime.
        reset_extra_data(&mut item, false);
        let wall = item.has(Behaviour::WallItem);
        if item.has(Behaviour::Teleporter) {
            item.custom_data = "0".into(); // TELEPORTER_CLOSE
        }
        self.mapping.items.push(item);
        let idx = self.mapping.items.len() - 1;
        if wall {
            self.send(out::place_wallitem(&self.mapping.items[idx]));
            // TODO(moodlight): a room dimmer becomes the room's moodlight.
        } else {
            self.handle_item_adjustment(world, idx, false);
            self.mapping.add_item_tiles(idx);
            self.send_map();
            self.send(out::place_flooritem(&self.mapping.items[idx]));
        }
        let tiles = self.mapping.items[idx].affected_tiles();
        if !wall {
            self.update_entities(&[], &tiles);
        }
        self.save_item(world, idx);
        // TODO(items): onItemPlaced of the pet, totem and teleporter triggers.
    }

    /// Java: `RoomMapping.moveItem`.
    pub fn move_flat_item(&mut self, world: &World, idx: usize, x: i32, y: i32, rotation: i32) {
        let old = self.mapping.items[idx].position;
        let is_rotation = old == Position::new(x, y) && old.rotation() != rotation;
        let old_tiles = self.mapping.items[idx].affected_tiles();
        self.mapping.remove_item_tiles(idx);
        {
            let item = &mut self.mapping.items[idx];
            item.position.x = x;
            item.position.y = y;
            item.position.set_rotation(rotation);
            // TODO(rollers): rolling data. TODO(items): lastPlacedTime.
            reset_extra_data(item, false);
        }
        let wall = self.mapping.items[idx].has(Behaviour::WallItem);
        if !wall {
            self.handle_item_adjustment(world, idx, is_rotation);
            self.mapping.add_item_tiles(idx);
            self.send_map();
            self.send(out::move_flooritem(&self.mapping.items[idx]));
        }
        // TODO(teleporters): moving a teleporter resets the users teleporting with it.
        if !wall {
            let tiles = self.mapping.items[idx].affected_tiles();
            self.update_entities(&old_tiles, &tiles);
        }
        self.save_item(world, idx);
        // TODO(items): onItemMoved of the totem and pet triggers.
    }

    /// Java: `RoomMapping.pickupItem` (whose triggers do nothing for the ported interactors)
    /// and `removeItem`. Returns the item as it goes back to the hand.
    pub fn remove_flat_item(&mut self, world: &World, idx: usize) -> Item {
        // TODO(items): onItemPickup of the pet and totem triggers.
        let wall = self.mapping.items[idx].has(Behaviour::WallItem);
        if !wall {
            self.mapping.remove_item_tiles(idx);
        }
        let mut item = self.mapping.take_item(idx);
        if wall {
            self.send(out::remove_wallitem(&item));
        } else {
            self.send_map();
            self.send(out::remove_flooritem(&item));
        }
        if item.has(Behaviour::Roomdimmer) {
            if item.custom_data.is_empty() {
                item.custom_data = crate::game::item::DEFAULT_ROOMDIMMER_CUSTOM_DATA.into();
            }
            if item.custom_data.starts_with('2') {
                item.custom_data.replace_range(..1, "1");
            }
            // TODO(moodlight): the room loses its moodlight.
        }
        // TODO(totems): a totem head loses its colour.
        reset_extra_data(&mut item, false);
        if !wall {
            self.update_entities(&[], &item.affected_tiles());
        }
        item.position = Position::new(0, 0);
        if let Some(flat) = item.flat.as_mut() {
            flat.record.room_id = 0;
        }
        item.sync_record();
        save_record(world, &item);
        item
    }

    /// Java: `RoomMapping.sendMap`.
    fn send_map(&self) {
        self.send(out_rooms::heightmap_update(
            &self.mapping.heightmap_update(),
        ));
    }

    /// Java: `RoomMapping.handleItemAdjustment`: a moved or placed floor item lands on top of
    /// what is on its tile.
    fn handle_item_adjustment(&mut self, world: &World, idx: usize, is_rotation: bool) {
        let position = self.mapping.items[idx].position;
        let Some(tile) = self.mapping.tile(position) else {
            return;
        };
        if !is_rotation {
            let mut height = tile.tile_height;
            if let Some(highest) = tile.highest_item.map(|i| &self.mapping.items[i]) {
                if highest.database_id == self.mapping.items[idx].database_id {
                    height -= highest.total_height();
                    let default = self.model.tile_height(position.x, position.y);
                    if height < default {
                        height = default;
                    }
                }
            }
            // TODO(rollers): items placed on rolling items sit on the roller.
            self.mapping.items[idx].position.z = height;
        }
        let limit = stack_height_limit(world);
        let item = &mut self.mapping.items[idx];
        if item.position.z > limit {
            item.position.z = limit;
        }
    }

    /// Java: `Item.updateEntities(oldPosition)`: everybody on the item's old and new tiles gets
    /// their status worked out again, sent at once. Like Java, someone on both is updated twice.
    fn update_entities(&mut self, old_tiles: &[Position], tiles: &[Position]) {
        let mut instances = Vec::new();
        for p in old_tiles.iter().chain(tiles) {
            instances.extend(self.mapping.entire_entities(*p));
        }
        for instance_id in instances {
            if let Some(idx) = self.entity_by_instance(instance_id) {
                self.invoke_item(idx, true);
            }
        }
    }

    /// Java: `InteractionType.getTrigger().onInteract(player, room, item, status)` for the
    /// ported interactors: `DefaultInteractor` (also what `ChairInteractor` and `BedInteractor`
    /// do). TODO(items): teleporters, vending machines, lert, scoreboards, fortune wheel, pets,
    /// pool, games, totems, idol, step light, love randomizer and multi height interactors.
    pub fn interact(&mut self, world: &World, idx: usize) {
        match self.mapping.items[idx].interaction {
            Interaction::Default | Interaction::Chair | Interaction::Bed => {
                self.default_interact(world, idx)
            }
            _ => {}
        }
    }

    /// Java: `DefaultInteractor.onInteract`: cycles the custom data through
    /// `0..max_status - 1`.
    fn default_interact(&mut self, world: &World, idx: usize) {
        let item = &self.mapping.items[idx];
        if item.has(Behaviour::Gate)
            && item.is_gate_open()
            && item
                .affected_tiles()
                .first()
                .is_some_and(|p| !self.mapping.entire_entities(*p).is_empty())
        {
            return; // Can't close a gate with someone on it.
        }
        if blocks_interaction(item) {
            return;
        }
        let max_status = item.max_status();
        if max_status <= 0 {
            return;
        }
        let current = if is_numeric(&item.custom_data) {
            item.custom_data.parse::<i32>().unwrap_or(0)
        } else {
            0
        };
        let mut next = current + 1;
        if next >= max_status {
            next = 0;
        }
        self.mapping.items[idx].custom_data = next.to_string();
        self.send(out::stuff_data_update(&self.mapping.items[idx]));
        self.save_item(world, idx);
    }

    /// Java: `DELETEFLAT`'s `item.delete()` for each room item. They stay in the loaded room
    /// until it is disposed, as in Java.
    pub fn delete_flat_items(&self, world: &World) {
        let ids: Vec<u64> = self
            .mapping
            .items
            .iter()
            .filter_map(|i| i.flat.as_ref().map(|f| f.record.id))
            .collect();
        if let Err(e) = world.store.delete_items(&ids) {
            error!(room = self.id(), "deleting room items: {e}");
        }
    }
}

/// The behaviours `USEFURNITURE`, `USEWALLITEM` and `DefaultInteractor` refuse to toggle
/// ("Prevent dice rigging, scripting trophies, post-its, etc.").
pub fn blocks_interaction(item: &Item) -> bool {
    [
        Behaviour::Roomdimmer,
        Behaviour::Dice,
        Behaviour::PrizeTrophy,
        Behaviour::PostIt,
        Behaviour::Roller,
        Behaviour::WheelOfFortune,
        Behaviour::SoundMachineSampleSet,
    ]
    .iter()
    .any(|b| item.has(*b))
}

fn save_record(world: &World, item: &Item) {
    let Some(flat) = item.flat.as_ref() else {
        return;
    };
    if let Err(e) = world.store.update_item(&flat.record) {
        error!(item = flat.record.id, "saving item: {e}");
    }
}

/// Java: `Item.isValidMove(item, room, x, y, rotation)`. `item` may be in the room already
/// (moving) or not (placing from the hand); `limit` is `stack.height.limit`.
pub fn is_valid_move(
    map: &RoomMapping,
    item: &Item,
    (x, y, rotation): (i32, i32, i32),
    limit: f64,
) -> bool {
    if map.tile(Position::new(x, y)).is_none() {
        return false;
    }
    // TODO(rollers): the rolling data's next and previous positions also count.
    let is_rotation = item.position.rotation() != rotation && Position::new(x, y) == item.position;
    if is_rotation && item.length <= 1 && item.width <= 1 {
        return true;
    }
    let target = Position::new(x, y);
    for p in item.affected_tiles_at(x, y, rotation) {
        let Some(tile) = map.tile(p) else {
            return false;
        };
        if map.walking_height(tile) + item.positive_top_height() > limit {
            return false;
        }
        if !is_rotation && !map.entire_entities(p).is_empty() {
            return false;
        }
        if let Some(highest) = tile.highest_item.map(|i| &map.items[i]) {
            if highest.virtual_id != item.virtual_id && !can_place_on_top(item, highest, target) {
                return false;
            }
        }
        for other in tile.items.iter().map(|&i| &map.items[i]) {
            if other.virtual_id == item.virtual_id {
                continue;
            }
            if !can_place_on_top(item, other, target) {
                return false;
            }
            if other.has(Behaviour::Roller) {
                if item.has(Behaviour::Roller) {
                    return false;
                }
                if (item.length > 1 || item.width > 1) && item.has(Behaviour::CanLayOnTop) {
                    return false;
                }
            }
        }
    }
    true
}

/// Java: `Item.canPlaceOnTop(item, tileItem, targetTile)`.
fn can_place_on_top(item: &Item, other: &Item, target: Position) -> bool {
    if other.has(Behaviour::CanNotStackOnTop) {
        return false;
    }
    if item.has(Behaviour::Roller)
        && other.has(Behaviour::CanStackOnTop)
        && !other.has(Behaviour::PlaceRollerOnTop)
        && other.top_height >= 0.1
    {
        return false;
    }
    // TODO(rollers): an item that is rolling can be placed on.
    if (other.has(Behaviour::Solid) || other.has(Behaviour::SolidSingleTile))
        && !other.has(Behaviour::CanStackOnTop)
    {
        return false;
    }
    if other.has(Behaviour::SolidSingleTile) && other.has(Behaviour::CanStackOnTop) {
        return other.position == target;
    }
    if other.has(Behaviour::OneWayGate)
        || other.has(Behaviour::CanSitOnTop)
        || other.has(Behaviour::CanLayOnTop)
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::game::pathfinder::{Walker, make_path};
    use crate::game::room::items::tests::{bench_armas, chair_plasto, flat_item, lamp_armas};
    use crate::game::room::mapping::is_valid_tile;
    use crate::game::room::model::RoomModel;

    const MODEL_A: &str = "xxxxxxxxxxxx|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxx00000000|xxxxxxxxxxxx|xxxxxxxxxxxx";

    /// Room 1000 of `traces/flat-furni.json` once the chair, the lamp and the moved sofa are in.
    fn furni_flat(entities: &[(i32, Position)]) -> RoomMapping {
        let model = Arc::new(RoomModel::parse(&havana_storage::RoomModel {
            model_id: "model_a".into(),
            model_name: "model_a".into(),
            door_x: 3,
            door_y: 5,
            door_dir: 2,
            heightmap: MODEL_A.into(),
            ..Default::default()
        }));
        let mut map = RoomMapping::new(model, false);
        map.items = vec![
            flat_item(2, lamp_armas(), (6, 5, 0)),
            flat_item(1, chair_plasto(), (7, 3, 4)),
            flat_item(3, bench_armas(), (9, 10, 4)),
        ];
        map.regenerate(entities);
        map
    }

    #[test]
    fn placement_rules() {
        let map = furni_flat(&[]);
        let sofa = map.items[2].clone();
        // traces/flat-furni.json: the sofa cannot go onto the solid lamp.
        assert!(!is_valid_move(&map, &sofa, (6, 5, 4), 8.0));
        assert!(!is_valid_move(&map, &sofa, (5, 5, 4), 8.0));
        assert!(is_valid_move(&map, &sofa, (9, 10, 2), 8.0));
        // Nothing goes on a chair, and nothing outside the open tiles.
        let lamp = map.items[0].clone();
        assert!(!is_valid_move(&map, &lamp, (7, 3, 0), 8.0));
        assert!(!is_valid_move(&map, &lamp, (2, 5, 0), 8.0));
        // Rotating a one-tile item in place is always fine.
        assert!(is_valid_move(&map, &map.items[1], (7, 3, 2), 8.0));
        // Above the stack height limit.
        assert!(!is_valid_move(&map, &lamp, (4, 4, 0), -1.0));
        // Someone standing there.
        let map = furni_flat(&[(0, Position::new(8, 8))]);
        assert!(!is_valid_move(&map, &lamp, (8, 8, 0), 8.0));
        assert!(is_valid_move(&map, &lamp, (8, 9, 0), 8.0));
    }

    #[test]
    fn walking_goes_around_solid_furniture() {
        let map = furni_flat(&[]);
        let walker = Walker {
            instance_id: 0,
            position: Position::new(3, 5),
            swimming: false,
            has_pool_figure: false,
        };
        // traces/flat-furni.json: onto the lamp there is no path; past it the path bends.
        assert!(!is_valid_tile(&map, Some(&walker), Position::new(6, 5)));
        let path: Vec<(i32, i32)> = make_path(&map, &walker, walker.position, Position::new(8, 5))
            .iter()
            .map(|p| (p.x, p.y))
            .collect();
        assert_eq!(path, [(4, 6), (5, 6), (6, 6), (7, 5), (8, 5)]);
        // The chair is the highest item of its tile and sits at 0.
        let tile = map.tile(Position::new(7, 3)).unwrap();
        assert_eq!(tile.tile_height, 1.0);
        assert_eq!(map.walking_height(tile), 0.0);
    }

    #[test]
    fn heightmap_update_matches_trace() {
        let map = furni_flat(&[]);
        let mut expected = String::from("xxxxxxxxxxxx\r");
        for _ in 0..13 {
            expected.push_str("xxxx00000000\r");
        }
        expected.push_str("xxxxxxxxxxxx\rxxxxxxxxxxxx\r");
        assert_eq!(map.heightmap_update(), expected);
        let msg = out_rooms::heightmap_update(&map.heightmap_update());
        assert_eq!(msg.header, 219);
        assert!(msg.body.ends_with(b"\r\x02"));
    }

    #[test]
    fn removing_an_item_keeps_the_tiles_consistent() {
        let mut map = furni_flat(&[]);
        let lamp = map.item_index(2).unwrap();
        map.remove_item_tiles(lamp);
        let removed = map.take_item(lamp);
        assert_eq!(removed.virtual_id, 2);
        assert!(map.highest_item(Position::new(6, 5)).is_none());
        assert_eq!(map.highest_item(Position::new(7, 3)).unwrap().virtual_id, 1);
        assert_eq!(
            map.highest_item(Position::new(10, 10)).unwrap().virtual_id,
            3
        );

        // Moving the sofa: off its old tiles, onto the new ones.
        let sofa = map.item_index(3).unwrap();
        map.remove_item_tiles(sofa);
        map.items[sofa].position = Position::new(5, 8);
        map.items[sofa].position.set_rotation(2);
        map.add_item_tiles(sofa);
        assert!(map.highest_item(Position::new(9, 10)).is_none());
        assert_eq!(map.highest_item(Position::new(5, 9)).unwrap().virtual_id, 3);
    }

    #[test]
    fn extra_data_resets() {
        let mut lamp = flat_item(2, lamp_armas(), (6, 5, 4));
        assert!(reset_extra_data(&mut lamp, false));
        assert_eq!(lamp.position.rotation(), 0);
        assert!(!reset_extra_data(&mut lamp, false));
        let mut chair = flat_item(1, chair_plasto(), (7, 3, 4));
        assert!(!reset_extra_data(&mut chair, true));
    }
}
