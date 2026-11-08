//! Beds (Java: `BedInteractor`): walking to any tile of a bed leads to a pillow (see
//! `Room::walk_to`), and stopping there lies down. Toggling a bed is the default interactor.

use crate::game::format_decimal;
use crate::game::pathfinder::Position;
use crate::game::room::Room;
use crate::game::room::entity::StatusType;
use crate::game::room::items::Item;
use crate::game::room::mapping::is_valid_tile;

/// Java: `BedInteractor.getValidPillowTiles`: the bed's own tile and the one next to it along
/// the head (for rotation 0 to the east, for 2 to the south; otherwise (-1, -1)).
pub fn valid_pillow_tiles(item: &Item) -> [Position; 2] {
    let p = item.position;
    let second = match p.rotation() {
        0 => Position::new(p.x + 1, p.y),
        2 => Position::new(p.x, p.y + 1),
        _ => Position::new(-1, -1),
    };
    [Position::new(p.x, p.y), second]
}

/// Java: `BedInteractor.isValidPillowTile`.
pub fn is_valid_pillow_tile(item: &Item, position: Position) -> bool {
    position == item.position || valid_pillow_tiles(item).contains(&position)
}

/// Java: `BedInteractor.convertToPillow`: the pillow in the same column (rotation 0) or row.
pub fn convert_to_pillow(position: Position, item: &Item) -> Position {
    let mut destination = position;
    if !is_valid_pillow_tile(item, position) {
        let tile = valid_pillow_tiles(item)[0];
        if item.position.rotation() == 0 {
            destination.y = tile.y;
        } else {
            destination.x = tile.x;
        }
    }
    destination
}

impl Room {
    /// Java: `BedInteractor.onEntityStop`: on (or moved to) a pillow, the entity lies down with
    /// the bed's rotation, stops carrying and dancing.
    pub(crate) fn bed_stop(&mut self, idx: usize, item: usize) {
        let mut destination = self.entities[idx].position;
        let bed = &self.mapping.items[item];
        if !is_valid_pillow_tile(bed, destination) {
            destination = convert_to_pillow(destination, bed);
        }
        if is_valid_pillow_tile(bed, destination) {
            let walker = self.walker(idx);
            if !is_valid_tile(&self.mapping, Some(&walker), destination) {
                return;
            }
            let (top_height, rotation) = (bed.top_height, bed.position.rotation());
            self.warp_silently(idx, destination);
            self.stop_carrying(idx);
            self.dance(idx, 0);
            let e = &mut self.entities[idx];
            e.position.set_rotation(rotation);
            e.statuses.set(StatusType::Lay, format_decimal(top_height));
        }
        self.entities[idx].needs_update = true;
        self.queue_effect(idx);
    }

    /// Java: `RoomEntity.warp(position, false, false)`: moves the entity on the map without
    /// telling anyone.
    pub(crate) fn warp_silently(&mut self, idx: usize, position: Position) {
        let (old, instance_id, next) = {
            let e = &self.entities[idx];
            (e.position, e.instance_id, e.next_position)
        };
        self.mapping.remove_entity(old, instance_id);
        if let Some(next) = next {
            self.mapping.remove_entity(next, instance_id);
        }
        let e = &mut self.entities[idx];
        e.position.x = position.x;
        e.position.y = position.y;
        self.update_new_height(idx);
        self.mapping.add_entity(position, instance_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::room::items::tests::{definition, flat_item};

    fn bed_polyfon(rotation: i32) -> Item {
        let def = definition(
            (17, "bed_polyfon", 0),
            (2, 3, 1.0),
            0,
            "can_lay_on_top",
            "bed",
            "0,2",
        );
        flat_item(1, def, (6, 8, rotation))
    }

    #[test]
    fn pillows() {
        // traces/flat-bed.json: a double bed at (6,8) facing north covers x 6-7, y 8-10.
        let bed = bed_polyfon(0);
        assert_eq!(
            valid_pillow_tiles(&bed),
            [Position::new(6, 8), Position::new(7, 8)]
        );
        assert!(is_valid_pillow_tile(&bed, Position::new(7, 8)));
        assert!(!is_valid_pillow_tile(&bed, Position::new(7, 10)));
        assert_eq!(
            convert_to_pillow(Position::new(7, 10), &bed),
            Position::new(7, 8)
        );
        assert_eq!(
            convert_to_pillow(Position::new(6, 9), &bed),
            Position::new(6, 8)
        );
        // Facing east, the pillow is the first column.
        let bed = bed_polyfon(2);
        assert_eq!(
            convert_to_pillow(Position::new(8, 9), &bed),
            Position::new(6, 9)
        );
        assert!(is_valid_pillow_tile(&bed, Position::new(6, 9)));
        // Other rotations only have the bed's own tile.
        let bed = bed_polyfon(4);
        assert_eq!(valid_pillow_tiles(&bed)[1], Position::new(-1, -1));
    }
}
