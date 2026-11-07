//! Positions, rotations and the walking pathfinder (Java: `game/pathfinder`).
//!
//! The pathfinder is ported line for line, including its quirks (it searches from the goal back
//! to the walker, keeps a FIFO open list rather than a priority queue, and adds the squared
//! distance truncated to an int as heuristic), because the paths it picks are visible on the wire.

use std::collections::VecDeque;

use crate::game::room::items::Behaviour;
use crate::game::room::mapping::{RoomMapping, is_valid_diagonal_tile, is_valid_tile};

/// A tile position with height and rotations (Java: `Position`). Equality only compares x and y,
/// like `Position.equals`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Position {
    pub x: i32,
    pub y: i32,
    pub z: f64,
    pub head_rotation: i32,
    pub body_rotation: i32,
}

impl PartialEq for Position {
    fn eq(&self, other: &Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}

impl Position {
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            ..Default::default()
        }
    }

    /// Java: `getRotation` (the body rotation).
    pub fn rotation(&self) -> i32 {
        self.body_rotation
    }

    /// Java: `setRotation`: sets head and body.
    pub fn set_rotation(&mut self, rotation: i32) {
        self.head_rotation = rotation;
        self.body_rotation = rotation;
    }

    /// Java: `getDistanceSquared`, which despite its name is the distance truncated to an int.
    pub fn distance_squared(&self, other: &Position) -> i32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        ((dx * dx + dy * dy) as f64).sqrt() as i32
    }

    /// Java: `getSquareInFront`.
    pub fn square_in_front(&self) -> Position {
        let mut p = *self;
        match self.body_rotation {
            0 => p.y -= 1,
            1 => {
                p.x += 1;
                p.y -= 1
            }
            2 => p.x += 1,
            3 => {
                p.x += 1;
                p.y += 1
            }
            4 => p.y += 1,
            5 => {
                p.x -= 1;
                p.y += 1
            }
            6 => p.x -= 1,
            7 => {
                p.x -= 1;
                p.y -= 1
            }
            _ => {}
        }
        p
    }

    /// Java: `getSquareBehind`.
    pub fn square_behind(&self) -> Position {
        let mut p = *self;
        let (dx, dy) = match self.body_rotation {
            0 => (0, 1),
            1 => (-1, 1),
            2 => (-1, 0),
            3 => (-1, -1),
            4 => (0, -1),
            5 => (1, -1),
            6 => (1, 0),
            7 => (1, 1),
            _ => (0, 0),
        };
        p.x += dx;
        p.y += dy;
        p
    }

    /// Java: `getSquareLeft`.
    pub fn square_left(&self) -> Position {
        let mut p = *self;
        let (dx, dy) = match self.body_rotation {
            0 => (-1, 0),
            1 => (-1, -1),
            2 => (0, -1),
            3 => (1, -1),
            4 => (1, 0),
            5 => (1, 1),
            6 => (0, 1),
            7 => (-1, 1),
            _ => (0, 0),
        };
        p.x += dx;
        p.y += dy;
        p
    }

    /// Java: `getSquareRight`.
    pub fn square_right(&self) -> Position {
        let mut p = *self;
        let (dx, dy) = match self.body_rotation {
            0 => (1, 0),
            1 => (1, 1),
            2 => (0, 1),
            3 => (-1, 1),
            4 => (-1, 0),
            5 => (-1, -1),
            6 => (0, -1),
            7 => (1, -1),
            _ => (0, 0),
        };
        p.x += dx;
        p.y += dy;
        p
    }
}

/// Java: `Rotation.calculateWalkDirection`.
pub fn walk_direction(x: i32, y: i32, to_x: i32, to_y: i32) -> i32 {
    use std::cmp::Ordering::*;
    match (x.cmp(&to_x), y.cmp(&to_y)) {
        (Equal, Less) => 4,
        (Equal, _) => 0,
        (Greater, Equal) => 6,
        (Greater, Less) => 5,
        (Greater, Greater) => 7,
        (Less, Equal) => 2,
        (Less, Less) => 3,
        (Less, Greater) => 1,
    }
}

/// Java: `Rotation.calculateHumanDirection`.
#[allow(dead_code)] // TODO(rooms): LOOKTO and body turns use it.
pub fn human_direction(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    if x1 > x2 && y1 > y2 {
        7
    } else if x1 < x2 && y1 < y2 {
        3
    } else if x1 > x2 && y1 < y2 {
        5
    } else if x1 < x2 && y1 > y2 {
        1
    } else if x1 > x2 {
        6
    } else if x1 < x2 {
        2
    } else if y1 < y2 {
        4
    } else {
        0
    }
}

/// Java: `AffectedTile.getAffectedTiles`: the tiles a `length` x `width` item covers.
pub fn affected_tiles(
    mut length: i32,
    mut width: i32,
    x: i32,
    y: i32,
    rotation: i32,
) -> Vec<Position> {
    if length != width && (rotation == 0 || rotation == 4) {
        std::mem::swap(&mut length, &mut width);
    }
    let mut points = Vec::new();
    for nx in x..x + width {
        for ny in y..y + length {
            points.push(Position::new(nx, ny));
        }
    }
    points
}

pub const MAX_DROP_HEIGHT: f64 = 3.0;
pub const MAX_LIFT_HEIGHT: f64 = 1.5;

/// Java: `Pathfinder.DIAGONAL_MOVE_POINTS`, in its order (the order changes the paths found).
pub const DIAGONAL_MOVE_POINTS: [(i32, i32); 8] = [
    (0, -1),
    (0, 1),
    (1, 0),
    (-1, 0),
    (1, -1),
    (-1, 1),
    (1, 1),
    (-1, -1),
];

/// What the pathfinder needs to know about the walker (Java reads it from the `Entity`).
#[derive(Clone, Copy, Debug)]
pub struct Walker {
    pub instance_id: i32,
    pub position: Position,
    pub swimming: bool,
    pub has_pool_figure: bool,
    /// A pet's nest item (database id), which it may walk onto (Java: `RoomTile.isValidTile`).
    pub pet_nest: Option<u64>,
}

/// Sprites whose tiles skip the height checks (Java: the `*ItemHeightExempt` flags).
fn height_exempt(sprite: &str, other: Option<&str>, has_teleporter: bool) -> bool {
    has_teleporter
        || sprite == "wsJoinQueue"
        || sprite == "wsQueueTile"
        || (sprite == "poolEnter" && other == Some("poolExit"))
        || (sprite == "poolExit" && other == Some("poolEnter"))
        || sprite == "poolLift"
        || sprite == "queue_tile2"
}

/// Java: `Pathfinder.isValidStep`.
pub fn is_valid_step(
    map: &RoomMapping,
    walker: &Walker,
    current: Position,
    tmp: Position,
    is_final_move: bool,
) -> bool {
    if !is_valid_tile(map, Some(walker), Position::new(current.x, current.y)) {
        return false;
    }
    if !is_valid_tile(map, Some(walker), Position::new(tmp.x, tmp.y)) {
        return false;
    }
    let (Some(from_tile), Some(to_tile)) = (map.tile(current), map.tile(tmp)) else {
        return false;
    };
    let old_height = map.walking_height(from_tile);
    let new_height = map.walking_height(to_tile);
    let from_item = from_tile.highest_item.map(|i| &map.items[i]);
    let to_item = to_tile.highest_item.map(|i| &map.items[i]);
    let from_sprite = from_item.map(|i| i.sprite.as_str());
    let to_sprite = to_item.map(|i| i.sprite.as_str());

    let from_exempt = from_item
        .is_some_and(|i| height_exempt(&i.sprite, to_sprite, i.has(Behaviour::Teleporter)));
    let to_exempt = to_item
        .is_some_and(|i| height_exempt(&i.sprite, from_sprite, i.has(Behaviour::Teleporter)));
    let exempt = from_exempt || to_exempt;

    // The search runs from the goal back to the walker, so Java checks both directions; together
    // the four checks reject any step of more than MAX_LIFT_HEIGHT either way.
    if !exempt {
        // toTile.isHeightUpwards(fromTile)
        if new_height < old_height && (new_height - old_height).abs() > MAX_LIFT_HEIGHT {
            return false;
        }
        // toTile.isHeightDrop(fromTile)
        if new_height > old_height && (old_height - new_height).abs() > MAX_DROP_HEIGHT {
            return false;
        }
        // fromTile.isHeightUpwards(toTile)
        if old_height < new_height && (new_height - old_height).abs() > MAX_LIFT_HEIGHT {
            return false;
        }
        // fromTile.isHeightDrop(toTile)
        if old_height > new_height && (old_height - new_height).abs() > MAX_DROP_HEIGHT {
            return false;
        }
    }

    if !pool_tile_status(map, walker, from_item, to_item) {
        return false;
    }

    // Java: no diagonal checks in the Sun Terrace, nor between pool triggers.
    let can_walk_diagonal = !map.model.name.starts_with("sun_terrace")
        && from_sprite != Some("poolExit")
        && from_sprite != Some("poolEnter")
        && to_sprite != Some("poolExit")
        && to_sprite != Some("poolEnter");
    if can_walk_diagonal && current.x != tmp.x && current.y != tmp.y {
        let first = is_valid_diagonal_tile(map, Some(walker), Position::new(tmp.x, current.y));
        let second = is_valid_diagonal_tile(map, Some(walker), Position::new(current.x, tmp.y));
        if !first && !second {
            return false;
        }
    }

    // Avoid walking into furniture unless it's the last step.
    let door = map.model.door_location();
    if current != door && tmp != door {
        if let Some(to_item) = to_item {
            return if is_final_move {
                to_item.is_walkable(true, tmp)
                    || to_item.affected_tiles().iter().any(|p| {
                        map.tile(*p)
                            .is_some_and(|t| t.contains_entity(walker.instance_id))
                    })
            } else {
                to_item.has(Behaviour::CanStandOnTop)
                    || to_item.has(Behaviour::DoorTeleporter)
                    || to_item.is_gate_open()
            };
        }
    }
    true
}

/// Java: `PoolInteractor.getTileStatus` (only does anything in pool rooms).
fn pool_tile_status(
    map: &RoomMapping,
    walker: &Walker,
    from_item: Option<&crate::game::room::items::Item>,
    to_item: Option<&crate::game::room::items::Item>,
) -> bool {
    let name = &map.model.name;
    if !(name.starts_with("pool_") || name == "md_a") {
        return true;
    }
    if let Some(from) = from_item {
        if from.sprite == "poolEnter" || from.sprite == "poolExit" {
            return walker.has_pool_figure;
        }
    }
    if let Some(to) = to_item {
        if to.sprite == "poolEnter" || to.sprite == "poolExit" {
            return walker.has_pool_figure;
        }
        // The swim checks below are unreachable in Java too (the branch above returned).
        if walker.swimming && to.sprite == "poolEnter" {
            return false;
        }
        if !walker.swimming && to.sprite == "poolExit" {
            return false;
        }
        // TODO(flash): Flash clients may not cut the queue nor use booths and lifts this way.
        if to.sprite == "poolBooth" || to.sprite == "poolLift" {
            return to.current_program_value != "close";
        }
    }
    true
}

struct Node {
    position: Position,
    next: Option<usize>,
    cost: i32,
    in_open: bool,
    in_closed: bool,
}

/// Java: `Pathfinder.makePath`: the tiles from (excluding) `start` to (including) `end`, or
/// empty if there is no path.
pub fn make_path(
    map: &RoomMapping,
    walker: &Walker,
    start: Position,
    end: Position,
) -> VecDeque<Position> {
    let mut squares = VecDeque::new();
    let (nodes, found) = make_path_reversed(map, walker, end, start);
    if let Some(mut node) = found {
        while let Some(next) = nodes[node].next {
            squares.push_back(nodes[next].position);
            node = next;
        }
    }
    squares
}

/// Java: `Pathfinder.makePathReversed`. Nodes live in an arena; `map` holds arena indexes.
fn make_path_reversed(
    map: &RoomMapping,
    walker: &Walker,
    start: Position,
    end: Position,
) -> (Vec<Node>, Option<usize>) {
    let size_x = map.model.size_x;
    let size_y = map.model.size_y;
    let mut nodes: Vec<Node> = Vec::new();
    let mut grid: Vec<Option<usize>> = vec![None; (size_x * size_y).max(0) as usize];
    let in_grid = |p: &Position| p.x >= 0 && p.y >= 0 && p.x < size_x && p.y < size_y;
    let cell = |p: &Position| (p.x * size_y + p.y) as usize;

    if !in_grid(&start) {
        return (nodes, None);
    }
    nodes.push(Node {
        position: start,
        next: None,
        cost: 0,
        in_open: false,
        in_closed: false,
    });
    grid[cell(&start)] = Some(0);
    let mut open: VecDeque<usize> = VecDeque::from([0]);

    while let Some(current) = open.pop_front() {
        nodes[current].in_closed = true;
        let cur_pos = nodes[current].position;
        for (dx, dy) in DIAGONAL_MOVE_POINTS {
            let tmp = Position {
                x: cur_pos.x + dx,
                y: cur_pos.y + dy,
                z: cur_pos.z,
                ..Default::default()
            };
            let is_final_move = tmp.x == end.x && tmp.y == end.y;
            let from = Position {
                x: cur_pos.x,
                y: cur_pos.y,
                z: cur_pos.z,
                ..Default::default()
            };
            if !is_valid_step(map, walker, from, tmp, is_final_move) || !in_grid(&tmp) {
                continue;
            }
            let node = match grid[cell(&tmp)] {
                Some(n) => n,
                None => {
                    nodes.push(Node {
                        position: tmp,
                        next: None,
                        cost: i32::MAX,
                        in_open: false,
                        in_closed: false,
                    });
                    let n = nodes.len() - 1;
                    grid[cell(&tmp)] = Some(n);
                    n
                }
            };
            if nodes[node].in_closed {
                continue;
            }
            let mut diff = 0;
            if cur_pos.x != nodes[node].position.x {
                diff += 2;
            }
            if cur_pos.y != nodes[node].position.y {
                diff += 2;
            }
            let cost = nodes[current]
                .cost
                .wrapping_add(diff)
                .wrapping_add(nodes[node].position.distance_squared(&end));
            if cost < nodes[node].cost {
                nodes[node].cost = cost;
                nodes[node].next = Some(current);
            }
            if !nodes[node].in_open {
                if nodes[node].position.x == end.x && nodes[node].position.y == end.y {
                    nodes[node].next = Some(current);
                    return (nodes, Some(node));
                }
                nodes[node].in_open = true;
                open.push_back(node);
            }
        }
    }
    (nodes, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations() {
        assert_eq!(walk_direction(13, 6, 12, 7), 5);
        assert_eq!(walk_direction(11, 8, 11, 9), 4);
        assert_eq!(walk_direction(1, 1, 1, 0), 0);
        assert_eq!(walk_direction(1, 1, 2, 0), 1);
        assert_eq!(human_direction(0, 0, 1, 1), 3);
        assert_eq!(human_direction(0, 0, 0, 0), 0);
    }

    #[test]
    fn distance_is_truncated() {
        let a = Position::new(0, 0);
        assert_eq!(a.distance_squared(&Position::new(3, 4)), 5);
        assert_eq!(a.distance_squared(&Position::new(1, 1)), 1);
        assert_eq!(a.distance_squared(&Position::new(2, 3)), 3);
    }

    #[test]
    fn affected_tiles_swap_on_rotation() {
        assert_eq!(affected_tiles(2, 1, 5, 5, 0).len(), 2);
        assert_eq!(affected_tiles(2, 1, 5, 5, 0)[1], Position::new(6, 5));
        assert_eq!(affected_tiles(2, 1, 5, 5, 2)[1], Position::new(5, 6));
    }
}
