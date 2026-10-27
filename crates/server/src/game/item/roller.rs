//! Rollers (Java: `game/item/roller`: `RollingData`, `ItemRollingAnalysis`,
//! `EntityRollingAnalysis`). The task that runs them is `RollerTask` in `game::room::tasks`.
//!
//! Every roll first checks whether the item or user may move one square on (`*_can_roll`, which
//! also records the [`RollingData`] as Java does, so later checks of the same roll see it), then
//! moves it (`*_do_roll`).

use crate::game::pathfinder::Position;
use crate::game::room::Room;
use crate::game::room::entity::StatusType;
use crate::game::room::items::Behaviour;
use crate::game::room::mapping::{RoomTile, is_valid_tile};

/// Java: `RollingData`: where a roll takes an item or user.
#[derive(Clone, Copy, Debug)]
pub struct RollingData {
    /// Virtual id of the roller.
    pub roller: i32,
    pub from: Position,
    pub next: Position,
    /// The height the client is shown for a rolling user.
    pub display_height: f64,
}

impl RollingData {
    fn new(roller: i32, from: Position, next: Position) -> Self {
        Self {
            roller,
            from,
            next,
            display_height: 0.0,
        }
    }
}

/// Java: `RoomTile.getItems()`: the tile's items sorted by height (a stable sort).
pub fn items_by_height(room: &Room, tile: &RoomTile) -> Vec<usize> {
    let mut items = tile.items.clone();
    items.sort_by(|a, b| {
        room.mapping.items[*a]
            .position
            .z
            .total_cmp(&room.mapping.items[*b].position.z)
    });
    items
}

/// Java: `RoomTile.hasWalkableFurni(entity)`.
fn has_walkable_furni(room: &Room, tile: &RoomTile, with_entity: bool) -> bool {
    tile.highest_item
        .is_none_or(|i| room.mapping.items[i].is_walkable(with_entity, tile.position))
}

/// Java: `RoomTile.getItemsAbove(item)`.
fn has_items_above(room: &Room, tile: &RoomTile, item: usize) -> bool {
    let base = &room.mapping.items[item];
    tile.items.iter().any(|&i| {
        let other = &room.mapping.items[i];
        other.database_id != base.database_id && other.position.z >= base.position.z
    })
}

/// The checks both analyses make against the other users and the other rolling items.
fn front_is_taken(room: &Room, roller: usize, front: Position) -> bool {
    for e in &room.entities {
        if e.next_position == Some(front) {
            return true;
        }
        if e.is_walking {
            continue;
        }
        if e.rolling.is_some_and(|r| r.next == front) {
            return true;
        }
        if e.position == front {
            return true;
        }
    }
    let roller_position = room.mapping.items[roller].position;
    room.mapping.items.iter().any(|i| {
        !i.has(Behaviour::WallItem)
            && i.position != roller_position
            && i.rolling.is_some_and(|r| r.next == front)
    })
}

/// The roller among the items of `tile` (the last one in height order), if any.
fn front_roller(room: &Room, items: &[usize]) -> Option<usize> {
    items
        .iter()
        .copied()
        .filter(|&i| room.mapping.items[i].has(Behaviour::Roller))
        .last()
}

/// Java: `ItemRollingAnalysis.canRoll(item, roller, room)`.
pub fn item_can_roll(room: &mut Room, item: usize, roller: usize, stack_limit: i64) -> Option<Position> {
    let roller_item = &room.mapping.items[roller];
    room.mapping.tile(roller_item.position)?;
    let it = &room.mapping.items[item];
    if it.virtual_id == roller_item.virtual_id || it.position.z < roller_item.position.z {
        return None;
    }
    let front = roller_item.position.square_in_front();
    let front_tile = room.mapping.tile(front)?;
    if front_is_taken(room, roller, front) {
        return None;
    }
    let (item_position, item_db) = (it.position, it.database_id);
    let roller_z = roller_item.position.z;
    let mut next_height = item_position.z;
    let mut subtract_roller_height = true;
    let mut block = Vec::new();

    if let Some(highest_next) = front_tile.highest_item {
        let front_items = items_by_height(room, front_tile);
        if let Some(fr) = front_roller(room, &front_items) {
            subtract_roller_height = false;
            let front_roller_item = &room.mapping.items[fr];
            if front_roller_item.position.z != roller_z
                && (front_roller_item.position.z - roller_z).abs() > 0.1
            {
                return None;
            }
            for &fi in &front_items {
                let front_item = &room.mapping.items[fi];
                if front_item.position.z < front_roller_item.position.z
                    || front_item.database_id == item_db
                {
                    continue;
                }
                if front_item.has(Behaviour::Roller)
                    && front_roller_item.position.square_in_front() == item_position
                    && (has_items_above(room, front_tile, fr)
                        || !room.mapping.entire_entities(front).is_empty())
                {
                    return None;
                }
            }
            let highest = &room.mapping.items[highest_next];
            if !highest.has(Behaviour::Roller) && highest.database_id != item_db {
                let item_tile_empty = room.mapping.entire_entities(item_position).is_empty();
                if highest.has(Behaviour::CanStackOnTop) && item_tile_empty {
                    if let Some(t) = room.mapping.tile(front_roller_item.position) {
                        block = t.items.clone();
                    }
                    next_height = (highest.position.z + highest.top_height) + item_position.z
                        - front_roller_item.top_height;
                } else {
                    return None;
                }
            }
        } else if !is_valid_tile(&room.mapping, None, front) {
            return None;
        }
    }
    for i in block {
        room.mapping.items[i].current_roll_blocked = true;
    }
    if subtract_roller_height {
        next_height -= room.mapping.items[roller].top_height;
    }
    if next_height > stack_limit as f64 {
        next_height = stack_limit as f64;
    }
    let mut next = Position::new(front.x, front.y);
    next.z = next_height;
    let roller_id = room.mapping.items[roller].virtual_id;
    room.mapping.items[item].rolling = Some(RollingData::new(roller_id, item_position, next));
    Some(next)
}

/// Java: `EntityRollingAnalysis.canRoll(entity, roller, room)`.
pub fn entity_can_roll(room: &mut Room, e: usize, roller: usize, stack_limit: i64) -> Option<Position> {
    let entity = &room.entities[e];
    let position = entity.position;
    let tile = room.mapping.tile(position)?;
    let roller_item = &room.mapping.items[roller];
    if entity.is_walking || position.z < roller_item.position.z || position != roller_item.position
    {
        return None;
    }
    if !has_walkable_furni(room, tile, true) {
        return None;
    }
    let front = roller_item.position.square_in_front();
    let front_tile = room.mapping.tile(front)?;
    if !has_walkable_furni(room, front_tile, true) || front_is_taken(room, roller, front) {
        return None;
    }
    let roller_z = roller_item.position.z;
    let current = tile.highest_item;
    let mut next_height = position.z;
    let mut subtract_roller_height = true;

    if front_tile.highest_item.is_some() {
        let front_items = items_by_height(room, front_tile);
        if let Some(fr) = front_roller(room, &front_items) {
            subtract_roller_height = false;
            let front_roller_item = &room.mapping.items[fr];
            if front_roller_item.position.z != roller_z
                && (front_roller_item.position.z - roller_z).abs() > 0.1
            {
                return None;
            }
            for &fi in &front_items {
                let front_item = &room.mapping.items[fi];
                if front_item.position.z < front_roller_item.position.z {
                    continue;
                }
                if current.is_some_and(|c| room.mapping.items[c].virtual_id == front_item.virtual_id)
                {
                    continue;
                }
                if !front_item.has(Behaviour::Roller) {
                    return None;
                }
                if front_roller_item.position.square_in_front() == position
                    && (has_items_above(room, front_tile, fr)
                        || !room.mapping.entire_entities(front).is_empty())
                {
                    return None;
                }
            }
        } else {
            let walker = room.walker(e);
            if !is_valid_tile(&room.mapping, Some(&walker), front) {
                return None;
            }
        }
    }
    if subtract_roller_height {
        next_height -= room.mapping.items[roller].top_height;
    }
    // If the user can roll but what they stand on cannot, nobody rolls.
    if let Some(c) = current.filter(|&c| !room.mapping.items[c].has(Behaviour::Roller)) {
        item_can_roll(room, c, roller, stack_limit)?;
    }
    let mut next = Position::new(front.x, front.y);
    next.z = next_height;
    let roller_id = room.mapping.items[roller].virtual_id;
    room.entities[e].rolling = Some(RollingData::new(roller_id, position, next));
    Some(next)
}

/// Java: `ItemRollingAnalysis.doRoll`: the item moves; the map is rebuilt by the caller.
pub fn item_do_roll(room: &mut Room, item: usize, next: Position) {
    let p = &mut room.mapping.items[item].position;
    p.x = next.x;
    p.y = next.y;
    p.z = next.z;
}

/// Java: `EntityRollingAnalysis.doRoll`: the user moves (keeping their rotation) and the height
/// the client is shown is worked out.
pub fn entity_do_roll(room: &mut Room, e: usize, roller: usize, mut next: Position) {
    let from = room.entities[e].position;
    let (roller_z, roller_top) = {
        let r = &room.mapping.items[roller];
        (r.position.z, r.top_height)
    };
    let current = room.mapping.highest_item(from);
    if current.is_some_and(|c| c.has(Behaviour::Roller))
        && (from.z - roller_z).abs() >= 0.1
        && room
            .mapping
            .highest_item(next)
            .is_some_and(|i| i.has(Behaviour::Roller))
    {
        next.z = roller_z + roller_top;
    }
    let mut display = next.z;
    let entity = &room.entities[e];
    let sitting_on_chair = current.is_some_and(|c| c.has(Behaviour::CanSitOnTop));
    if !sitting_on_chair && entity.statuses.contains(StatusType::Sit) {
        display -= 0.5; // Java: isSittingOnGround.
    }
    if let Some(sit) = entity
        .statuses
        .iter()
        .find(|s| s.key == StatusType::Sit)
        .and_then(|s| s.value.parse::<f64>().ok())
    {
        if sit > 1.0 {
            display += sit - 1.0;
        }
    }
    let instance_id = entity.instance_id;
    let entity = &mut room.entities[e];
    if let Some(rolling) = entity.rolling.as_mut() {
        rolling.display_height = display;
    }
    entity.position.x = next.x;
    entity.position.y = next.y;
    entity.position.z = next.z;
    room.mapping.remove_entity(from, instance_id);
    room.mapping.add_entity(next, instance_id);
}
