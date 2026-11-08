//! Flat furniture in a loaded room (Java: `RoomMapping.addItem/moveItem/pickupItem/removeItem`,
//! `resetExtraData` and `handleItemAdjustment`, `RoomItemManager`, `Item.isValidMove`,
//! `Item.updateEntities` and `DefaultInteractor`).
//!
//! Items are saved as soon as they change; Java queues `Item.save()`/`delete()` on the
//! `GameScheduler` and flushes them every few seconds. Nothing on the wire depends on the delay
//! while the room stays loaded, and a saved item that was deleted meanwhile stays deleted
//! ([`rusbbo_storage::Store::update_item`]), as Java's `UPDATE` does.

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
    // The HC dice defaults that follow in Java are commented out there too.
    if item.has(Behaviour::Dice) {
        item.requires_update = false;
    }
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
    // (applies, state, whether Java also clears `requiresUpdate` when the state was right)
    let resets = [
        (interaction == Lert, "0", false),
        (interaction == IdolVoteChair, "0", false),
        (interaction == IdolScoreboard, "-1", false),
        (item.has(Behaviour::OneWayGate), "0", false),
        (interaction == Fortune, "8", true), // FortuneInteractor.FORTUNE_OFF
        (interaction == VendingMachine, "0", true),
        (interaction == LoveRandomizer, "0", true),
        (item.has(Behaviour::Teleporter), "0", false), // TELEPORTER_CLOSE
    ];
    for (applies, value, clears) in resets {
        if !applies {
            continue;
        }
        if item.custom_data != value {
            item.custom_data = value.to_string();
            return true;
        }
        if clears {
            item.requires_update = false;
        }
    }
    if item.current_roll_blocked {
        item.current_roll_blocked = false;
    }
    false
}

/// Java: `Item.trophyData`, worked out once the item is in a room.
fn engrave(world: &World, item: &mut Item) {
    if item.has(Behaviour::PrizeTrophy) && item.trophy_data.is_none() {
        item.trophy_data = Some(crate::game::item::trophy::display(world, &item.custom_data));
    }
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
                let mut item = Item::from_owned(owned);
                engrave(world, &mut item);
                self.mapping.items.push(item);
            }
        }
        self.reset_item_states(world);
    }

    /// Java: `RoomItemManager.resetItemStates`.
    pub(super) fn reset_item_states(&mut self, world: &World) {
        // Java: game arenas keep their states. TODO(custom-rooms): custom rooms too.
        if self.is_game_arena() {
            return;
        }
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

    /// Java: `RoomItemManager.hasTooMany`: one sound machine (or jukebox), room dimmer and idol
    /// scoreboard per room, at most 25 post-its, 35 rollers (60 with club) and 3 pet nests.
    /// Tells the player why. (A post-it pad never gets here: `PLACESTUFF` places stickies
    /// before it asks, in Java too.)
    pub fn has_too_many(
        &self,
        world: &World,
        session: &crate::session::Session,
        has_club: bool,
        item: &Item,
    ) -> bool {
        use crate::messages::outgoing::alerts;
        let items = &self.mapping.items;
        // Java: the `soundMachine`, `moodlight` and `idolScoreboard` that `refreshRoomItems`
        // keeps: the first floor item with the behaviour (on a tile), the first wall dimmer.
        let on_tile =
            |i: &&Item| !i.has(Behaviour::WallItem) && self.mapping.tile(i.position).is_some();
        let sound_machine = items
            .iter()
            .filter(on_tile)
            .any(|i| i.has(Behaviour::Jukebox) || i.has(Behaviour::SoundMachine));
        if sound_machine && (item.has(Behaviour::SoundMachine) || item.has(Behaviour::Jukebox)) {
            session.send(out::cannot_place_stuff_from_strip(23));
            return true;
        }
        let moodlight = items
            .iter()
            .any(|i| i.has(Behaviour::WallItem) && i.has(Behaviour::Roomdimmer));
        if moodlight && item.has(Behaviour::Roomdimmer) {
            session.send(alerts::alert(&world.text("roomdimmer_furni_limit")));
            return true;
        }
        let idol_scoreboard = items
            .iter()
            .filter(on_tile)
            .any(|i| i.interaction == Interaction::IdolScoreboard);
        if idol_scoreboard && item.interaction == Interaction::IdolScoreboard {
            session.send(alerts::alert(
                "You can only place one American Idol scoreboard per room",
            ));
            return true;
        }
        let roller_limit = if has_club { 60 } else { 35 };
        let count = |f: &dyn Fn(&Item) -> bool| items.iter().filter(|i| f(i)).count();
        let limits = [
            (Behaviour::PostIt, 25, 12),
            (Behaviour::Roller, roller_limit, 22),
        ];
        for (behaviour, limit, error) in limits {
            if count(&|i| i.has(behaviour)) >= limit && item.has(behaviour) {
                session.send(out::cannot_place_stuff_from_strip(error));
                return true;
            }
        }
        if count(&|i| i.interaction == Interaction::PetNest) >= 3
            && item.interaction == Interaction::PetNest
        {
            session.send(out::cannot_place_stuff_from_strip(21));
            return true;
        }
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
        item.rolling = None;
        // Java also sets lastPlacedTime, which only commented-out code in WALK reads.
        reset_extra_data(&mut item, false);
        engrave(world, &mut item);
        let wall = item.has(Behaviour::WallItem);
        if item.has(Behaviour::Teleporter) {
            item.custom_data = "0".into(); // TELEPORTER_CLOSE
        }
        self.mapping.items.push(item);
        let idx = self.mapping.items.len() - 1;
        if wall {
            // A room dimmer becomes the room's moodlight (`Room::moodlight`).
            self.send(out::place_wallitem(&self.mapping.items[idx]));
        } else {
            self.handle_item_adjustment(world, idx, false);
            self.mapping.add_item_tiles(idx);
            self.send_map();
            self.send(out::place_flooritem(&self.mapping.items[idx]));
        }
        let tiles = self.mapping.items[idx].affected_tiles();
        if !wall {
            self.update_entities(&[], &tiles, None);
        }
        self.save_item(world, idx);
        if !wall && self.mapping.items[idx].interaction == Interaction::TotemHead {
            self.totem_head_placed(world, idx);
        }
        if self.mapping.items[idx].interaction == super::items::Interaction::PetNest {
            self.pet_nest_placed(world, idx);
        }
        self.pet_item_placed(world, idx);
    }

    /// Java: `RoomMapping.moveItem`.
    pub fn move_flat_item(&mut self, world: &World, idx: usize, x: i32, y: i32, rotation: i32) {
        let old = self.mapping.items[idx].position;
        let is_rotation = old == Position::new(x, y) && old.rotation() != rotation;
        // Java: `getItemBelow`/`getItemAbove` before the move, for the totem triggers.
        let (below, above) = (self.item_below(idx), self.item_above(idx));
        let (below, above) = (
            below.map(|i| self.mapping.items[i].virtual_id),
            above.map(|i| self.mapping.items[i].virtual_id),
        );
        let old_tiles = self.mapping.items[idx].affected_tiles();
        self.mapping.remove_item_tiles(idx);
        {
            let item = &mut self.mapping.items[idx];
            item.position.x = x;
            item.position.y = y;
            item.position.set_rotation(rotation);
            item.rolling = None;
            reset_extra_data(item, false);
        }
        let wall = self.mapping.items[idx].has(Behaviour::WallItem);
        if !wall {
            self.handle_item_adjustment(world, idx, is_rotation);
            self.mapping.add_item_tiles(idx);
            self.send_map();
            self.send(out::move_flooritem(&self.mapping.items[idx]));
        }
        if !is_rotation && self.mapping.items[idx].has(Behaviour::Teleporter) {
            // Whoever stood in the teleporter stops teleporting. (Java also compares the
            // player's target teleporter, a database id, with this item's virtual id.)
            for e in 0..self.entities.len() {
                if self.entities[e].position == old {
                    self.entities[e].walking_allowed = true;
                    let session = self.entities[e].session.clone();
                    self.deferred
                        .push(super::Deferred::StopTeleporting { session });
                }
            }
        }
        if !wall {
            let tiles = self.mapping.items[idx].affected_tiles();
            self.update_entities(&old_tiles, &tiles, Some(old));
        }
        self.save_item(world, idx);
        let interaction = self.mapping.items[idx].interaction;
        match interaction {
            Interaction::TotemHead => self.totem_head_moved(world, idx, is_rotation, below),
            Interaction::TotemLeg => self.totem_leg_moved(world, above),
            _ => {}
        }
        self.pet_item_left(world, interaction, old);
    }

    /// Java: `RoomMapping.pickupItem` (whose triggers do nothing for the ported interactors)
    /// and `removeItem`. Returns the item as it goes back to the hand.
    pub fn remove_flat_item(&mut self, world: &World, idx: usize) -> Item {
        let interaction = self.mapping.items[idx].interaction;
        if interaction == super::items::Interaction::PetNest {
            self.pet_nest_picked_up(world, idx);
        }
        let at = self.mapping.items[idx].position;
        self.pet_item_left(world, interaction, at);
        self.take_flat_item(world, idx)
    }

    /// Java: `RoomMapping.removeItem`: takes the item out of the room, with no pickup trigger.
    pub fn take_flat_item(&mut self, world: &World, idx: usize) -> Item {
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
            // The room loses its moodlight (`Room::moodlight` finds the next one, if any).
        }
        if item.sprite == "totem_head" {
            crate::game::item::interactors::totem::head_picked_up(&mut item);
        }
        reset_extra_data(&mut item, false);
        if !wall {
            self.update_entities(&[], &item.affected_tiles(), None);
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
            // An item put down on something rolling lands on the roller.
            let rolling_on = tile
                .highest_item
                .and_then(|h| self.mapping.items[h].rolling)
                .and_then(|r| self.mapping.item_index(r.roller))
                .map(|r| {
                    let roller = &self.mapping.items[r];
                    roller.position.z + roller.positive_top_height()
                });
            self.mapping.items[idx].position.z = rolling_on.unwrap_or(height);
        }
        let limit = stack_height_limit(world);
        let item = &mut self.mapping.items[idx];
        if item.position.z > limit {
            item.position.z = limit;
        }
    }

    /// Java: `Item.updateEntities(oldPosition)`: everybody on the item's old and new tiles gets
    /// their status worked out again, sent at once. Like Java, someone on both is updated twice.
    pub(crate) fn update_entities(
        &mut self,
        old_tiles: &[Position],
        tiles: &[Position],
        old_position: Option<Position>,
    ) {
        let mut instances = Vec::new();
        for p in old_tiles.iter().chain(tiles) {
            instances.extend(self.mapping.entire_entities(*p));
        }
        for instance_id in instances {
            if let Some(idx) = self.entity_by_instance(instance_id) {
                self.invoke_item(None, idx, true, old_position);
            }
        }
    }

    /// Java: `InteractionType.getTrigger().onInteract(player, room, item, status)` of
    /// `DefaultInteractor` and of `ChairInteractor` and `BedInteractor`, which do the same. The
    /// other interactors are in `game::item::interactors` ([`Room::on_interact`]).
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
    pub(crate) fn default_interact(&mut self, world: &World, idx: usize) {
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

    /// The room item with this database id (Java: `RoomItemManager.getByDatabaseId`).
    pub fn item_by_database_id(&self, database_id: u64) -> Option<usize> {
        self.mapping
            .items
            .iter()
            .position(|i| i.flat.as_ref().is_some_and(|f| f.record.id == database_id))
    }

    /// Java: `item.setCustomData(data); item.updateStatus()`: the room sees the new state; it is
    /// not saved.
    pub fn set_item_status(&mut self, idx: usize, data: &str) {
        self.mapping.items[idx].custom_data = data.to_string();
        let msg = out::stuff_data_update(&self.mapping.items[idx]);
        self.send(msg);
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
    let target = Position::new(x, y);
    let is_rotation = item.position.rotation() != rotation
        && (target == item.position
            || item
                .rolling
                .is_some_and(|r| target == r.next || target == r.from));
    if is_rotation {
        if item.rolling.is_some() {
            return false; // No rotating while rolling.
        }
        if item.length <= 1 && item.width <= 1 {
            return true;
        }
    }
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
    if other.rolling.is_some() {
        return true; // Something rolling can be placed on.
    }
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
        let model = Arc::new(RoomModel::parse(&rusbbo_storage::RoomModel {
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
            pet_nest: None,
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

    /// traces/flat-gates.json: a gate at (6,3) and a one-way door at (6,9) facing east.
    #[test]
    fn gates_are_solid_while_closed() {
        use crate::game::item::roller::tests::model_a;
        use crate::game::room::items::tests::{exe_gate, one_way_door};

        let items = vec![
            flat_item(2, exe_gate(), (6, 3, 0)),
            flat_item(1, one_way_door(), (6, 9, 2)),
        ];
        let mut map = RoomMapping::new(model_a(), false);
        map.items = items;
        map.regenerate(&[]);
        let walker = |x, y| Walker {
            pet_nest: None,
            instance_id: 0,
            position: Position::new(x, y),
            swimming: false,
            has_pool_figure: false,
        };
        // Closed gate: no path onto it.
        assert!(!is_valid_tile(
            &map,
            Some(&walker(3, 5)),
            Position::new(6, 3)
        ));
        map.items[0].custom_data = "1".into();
        assert!(map.items[0].is_gate_open());
        assert!(is_valid_tile(
            &map,
            Some(&walker(3, 5)),
            Position::new(6, 3)
        ));
        let path: Vec<(i32, i32)> = make_path(
            &map,
            &walker(3, 5),
            Position::new(3, 5),
            Position::new(6, 3),
        )
        .iter()
        .map(|p| (p.x, p.y))
        .collect();
        assert_eq!(path, [(4, 4), (5, 3), (6, 3)]);

        // The one-way door is entered from (7,9) and left at (5,9).
        let door = map.items[1].position;
        assert_eq!(door.square_in_front(), Position::new(7, 9));
        assert_eq!(door.square_behind(), Position::new(5, 9));
        // Closed, the way back from behind goes around it.
        let from = walker(5, 9);
        assert!(!is_valid_tile(&map, Some(&from), door));
        let path: Vec<(i32, i32)> = make_path(&map, &from, from.position, Position::new(7, 9))
            .iter()
            .map(|p| (p.x, p.y))
            .collect();
        assert_eq!(path, [(6, 10), (7, 9)]);
        // Open, it can be walked through.
        map.items[1].custom_data = "1".into();
        let path: Vec<(i32, i32)> = make_path(
            &map,
            &walker(7, 9),
            Position::new(7, 9),
            door.square_behind(),
        )
        .iter()
        .map(|p| (p.x, p.y))
        .collect();
        assert_eq!(path, [(6, 9), (5, 9)]);
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
