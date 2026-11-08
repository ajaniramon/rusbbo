//! The room's collision map: tiles, their furniture and who stands on them (Java:
//! `game/room/mapping/RoomMapping`, `RoomTile`).
//!
//! Entities are referred to by their instance id, which is unique within a room.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::game::pathfinder::{Position, Walker};
use crate::game::room::items::{Behaviour, Interaction, Item};
use crate::game::room::model::RoomModel;

/// Pool tiles nobody blocks (Java: `RoomTile.ignorePoolTiles`).
const IGNORE_POOL_TILES: [(i32, i32); 18] = [
    (20, 28),
    (19, 28),
    (17, 21),
    (17, 20),
    (31, 10),
    (31, 9),
    (19, 19),
    (18, 19),
    (11, 11),
    (10, 11),
    (21, 28),
    (22, 28),
    (16, 22),
    (16, 23),
    (30, 11),
    (30, 12),
    (12, 11),
    (13, 12),
];

/// Java: `RoomTile`.
#[derive(Clone, Debug)]
pub struct RoomTile {
    pub position: Position,
    /// Entities that block the tile.
    pub entities: Vec<i32>,
    /// Entities on the tiles around the door, which never block.
    pub non_blocking: Vec<i32>,
    /// Indexes into [`RoomMapping::items`], in the order Java's `ConcurrentHashMap<Integer, Item>`
    /// iterates them: by `virtualId & 15`, then insertion order.
    pub items: Vec<usize>,
    pub tile_height: f64,
    pub default_height: f64,
    pub highest_item: Option<usize>,
    #[allow(dead_code)] // Java sets it but nothing reads `isChairTile`.
    pub chair_tile: bool,
}

impl RoomTile {
    /// Java: `containsEntity`.
    pub fn contains_entity(&self, instance_id: i32) -> bool {
        self.entities.contains(&instance_id) || self.non_blocking.contains(&instance_id)
    }

    fn remove_entity(&mut self, instance_id: i32) {
        self.entities.retain(|e| *e != instance_id);
        self.non_blocking.retain(|e| *e != instance_id);
    }
}

/// Java: `RoomMapping`, plus the room's items it indexes (Java keeps those on `Room`).
pub struct RoomMapping {
    pub model: Arc<RoomModel>,
    pub items: Vec<Item>,
    pub is_public: bool,
    /// Walkway arrival tiles of this room (Java: `WalkwaysManager.getDestination`).
    pub walkway_destinations: Vec<Position>,
    /// `park_a` only: whether the Infobus's door is open (Java: `InfobusManager.isDoorOpen`).
    /// While it is closed nobody may walk onto the door tile.
    pub bus_door: Option<Arc<AtomicBool>>,
    /// Indexed `x * size_y + y`; empty until the first [`RoomMapping::regenerate`].
    tiles: Vec<RoomTile>,
}

impl RoomMapping {
    pub fn new(model: Arc<RoomModel>, is_public: bool) -> Self {
        Self {
            model,
            items: Vec::new(),
            is_public,
            walkway_destinations: Vec::new(),
            bus_door: None,
            tiles: Vec::new(),
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if self.tiles.is_empty()
            || x < 0
            || y < 0
            || x >= self.model.size_x
            || y >= self.model.size_y
            || !self.model.is_open(x, y)
        {
            return None;
        }
        Some((x * self.model.size_y + y) as usize)
    }

    /// Java: `getTile`: `None` outside the map, on closed tiles and before the map exists.
    pub fn tile(&self, p: Position) -> Option<&RoomTile> {
        self.index(p.x, p.y).map(|i| &self.tiles[i])
    }

    /// Java: `regenerateCollisionMap`. `entities` are the instance ids and positions of who is
    /// in the room.
    pub fn regenerate(&mut self, entities: &[(i32, Position)]) {
        let model = self.model.clone();
        self.tiles.clear();
        for x in 0..model.size_x {
            for y in 0..model.size_y {
                let height = model.tile_height(x, y);
                self.tiles.push(RoomTile {
                    position: Position::new(x, y),
                    entities: Vec::new(),
                    non_blocking: Vec::new(),
                    items: Vec::new(),
                    tile_height: height,
                    default_height: height,
                    highest_item: None,
                    chair_tile: false,
                });
            }
        }
        // Java sorts a copy by z (a stable sort) and adds each item to the tiles it covers.
        let mut order: Vec<usize> = (0..self.items.len()).collect();
        order.sort_by(|a, b| {
            self.items[*a]
                .position
                .z
                .total_cmp(&self.items[*b].position.z)
        });
        for i in order {
            if self.items[i].has(Behaviour::WallItem) || self.tile(self.items[i].position).is_none()
            {
                continue;
            }
            for p in self.items[i].affected_tiles() {
                let Some(t) = self.index(p.x, p.y) else {
                    continue;
                };
                self.add_item(t, i);
                if self.items[i].has(Behaviour::PublicSpaceObject) {
                    self.setup_redirections(i);
                }
            }
        }
        for (id, position) in entities {
            self.add_entity(*position, *id);
        }
        // Java's refreshRoomItems (the room's sound machine, moodlight and idol scoreboard) is
        // looked up when needed instead (`Room::has_too_many`, `interactors::idol`).
    }

    /// Java: `PoolHandler.setupRedirections`: a changing booth also owns the tile it is entered
    /// from, for standing in it and for the curtain.
    fn setup_redirections(&mut self, item: usize) {
        if self.items[item].sprite != "poolBooth" {
            return;
        }
        let p = self.items[item].position;
        let entrance = match (p.x, p.y) {
            (17, 11) => (18, 11),
            (17, 9) => (18, 9),
            (8, 1) => (8, 0),
            (9, 1) => (9, 0),
            _ => return,
        };
        // Java: `getTile(x, y).setHighestItem(item)`, the tile's height unchanged.
        if let Some(t) = self.index(entrance.0, entrance.1) {
            self.tiles[t].highest_item = Some(item);
        }
    }

    /// Java: `RoomTile.addItem`.
    fn add_item(&mut self, tile: usize, item: usize) {
        let bucket = |i: usize| self.items[i].virtual_id & 15;
        let b = bucket(item);
        let pos = self.tiles[tile]
            .items
            .iter()
            .position(|other| bucket(*other) > b)
            .unwrap_or(self.tiles[tile].items.len());
        self.tiles[tile].items.insert(pos, item);
        if self.items[item].total_height() < self.tiles[tile].tile_height {
            return;
        }
        self.reset_highest_item(tile);
    }

    /// The room item with this virtual id (Java: `RoomItemManager.getById`).
    pub fn item_index(&self, virtual_id: i32) -> Option<usize> {
        self.items.iter().position(|i| i.virtual_id == virtual_id)
    }

    /// Adds `item` to every tile it covers (the loop in Java's `RoomMapping.addItem` and
    /// `moveItem`).
    pub fn add_item_tiles(&mut self, item: usize) {
        for p in self.items[item].affected_tiles() {
            if let Some(t) = self.index(p.x, p.y) {
                self.add_item(t, item);
            }
        }
    }

    /// Takes `item` off the tiles it covers (Java: `RoomTile.removeItem` for each affected tile).
    pub fn remove_item_tiles(&mut self, item: usize) {
        for p in self.items[item].affected_tiles() {
            let Some(t) = self.index(p.x, p.y) else {
                continue;
            };
            self.tiles[t].items.retain(|i| *i != item);
            if self.tiles[t].highest_item == Some(item) {
                self.reset_highest_item(t);
            }
        }
    }

    /// Java: `getTile(p).removeItem(item)` then `getTile(p).addItem(item)`: puts the item back
    /// on that one tile, working the tile's height out again (a multi-height item changed).
    pub fn readd_item_at(&mut self, p: Position, item: usize) {
        let Some(t) = self.index(p.x, p.y) else {
            return;
        };
        self.tiles[t].items.retain(|i| *i != item);
        if self.tiles[t].highest_item == Some(item) {
            self.reset_highest_item(t);
        }
        self.add_item(t, item);
    }

    /// Removes the item at `item` from the room's list once it is off its tiles (see
    /// [`RoomMapping::remove_item_tiles`]), keeping the tiles' indexes valid.
    pub fn take_item(&mut self, item: usize) -> Item {
        let removed = self.items.remove(item);
        let shift = |i: &mut usize| {
            if *i > item {
                *i -= 1;
            }
        };
        for tile in &mut self.tiles {
            tile.items.retain(|i| *i != item);
            tile.items.iter_mut().for_each(shift);
            if tile.highest_item == Some(item) {
                tile.highest_item = None;
            }
            if let Some(h) = tile.highest_item.as_mut() {
                shift(h);
            }
        }
        removed
    }

    /// Java: `RoomTile.getEntireEntities` (blocking ones first) of the tile at `p`.
    pub fn entire_entities(&self, p: Position) -> Vec<i32> {
        self.tile(p).map_or_else(Vec::new, |t| {
            t.entities
                .iter()
                .chain(t.non_blocking.iter())
                .copied()
                .collect()
        })
    }

    /// Java: `HEIGHTMAP_UPDATE`'s map: each open tile of the model's heightmap as its walking
    /// height rounded (0 outside 0..=8), `x` for the rest, every row followed by `\r`.
    pub fn heightmap_update(&self) -> String {
        let mut out = String::new();
        for (y, line) in self.model.heightmap.split('\r').enumerate() {
            if y as i32 >= self.model.size_y {
                break;
            }
            let chars: Vec<char> = line.chars().collect();
            for x in 0..self.model.size_x {
                let c = chars.get(x as usize).copied().unwrap_or('x');
                if c.is_ascii_digit() {
                    // Java skips the character when the tile is missing.
                    let Some(tile) = self.tile(Position::new(x, y as i32)) else {
                        continue;
                    };
                    // Java: (int) Math.round(walkingHeight).
                    let mut height = (self.walking_height(tile) + 0.5).floor() as i64;
                    if !(0..=8).contains(&height) {
                        height = 0;
                    }
                    out.push_str(&height.to_string());
                } else {
                    out.push('x');
                }
            }
            out.push('\r');
        }
        out
    }

    /// Java: `RoomTile.resetHighestItem`.
    fn reset_highest_item(&mut self, tile: usize) {
        let t = &mut self.tiles[tile];
        t.highest_item = None;
        t.tile_height = t.default_height;
        let mut chair = None;
        for &i in &t.items {
            let item = &self.items[i];
            if item.has(Behaviour::CanSitOnTop)
                || item.has(Behaviour::Teleporter)
                || item.has(Behaviour::DoorTeleporter)
            {
                chair = Some(i);
            }
            let height = item.total_height();
            if height < t.tile_height {
                continue;
            }
            t.highest_item = Some(i);
            t.tile_height = height;
        }
        if let Some(chair) = chair {
            // Java compares database ids, which are all 0 for public items, so this never
            // applies to them.
            let chair_item = &self.items[chair];
            let covered = t.items.iter().any(|&i| {
                self.items[i].database_id != chair_item.database_id
                    && self.items[i].position.z > chair_item.position.z
            });
            if covered {
                t.chair_tile = true;
                t.highest_item = Some(chair);
                t.tile_height = chair_item.total_height();
            }
        }
    }

    /// Java: `RoomTile.addEntity`.
    pub fn add_entity(&mut self, p: Position, instance_id: i32) {
        let Some(t) = self.index(p.x, p.y) else {
            return;
        };
        let here = Position::new(p.x, p.y);
        let door = self.model.door_location();
        if here == door {
            return;
        }
        if self.is_public {
            if self.walkway_destinations.contains(&here) {
                return;
            }
            let name = &self.model.name;
            if (name == "pool_a" || name == "pool_b" || name == "md_a")
                && IGNORE_POOL_TILES.contains(&(here.x, here.y))
            {
                return;
            }
            if self.tiles[t]
                .highest_item
                .is_some_and(|i| self.items[i].interaction == Interaction::WsJoinQueue)
            {
                return;
            }
        }
        // TODO(games): game arenas skip the door-front rule.
        let front = door.square_in_front();
        if here == front || here == front.square_left() || here == front.square_right() {
            self.tiles[t].non_blocking.push(instance_id);
            return;
        }
        self.tiles[t].entities.push(instance_id);
    }

    /// Java: `RoomTile.removeEntity`.
    pub fn remove_entity(&mut self, p: Position, instance_id: i32) {
        if let Some(t) = self.index(p.x, p.y) {
            self.tiles[t].remove_entity(instance_id);
        }
    }

    /// Java: `RoomTile.getWalkingHeight`.
    pub fn walking_height(&self, tile: &RoomTile) -> f64 {
        let mut height = tile.tile_height;
        if let Some(i) = tile.highest_item {
            let item = &self.items[i];
            if item.has(Behaviour::CanSitOnTop) || item.has(Behaviour::CanLayOnTop) {
                height -= item.positive_top_height();
            }
        }
        height
    }

    /// The highest item of the tile at `p` (Java: `getTile(p).getHighestItem()`).
    pub fn highest_item(&self, p: Position) -> Option<&Item> {
        self.tile(p)
            .and_then(|t| t.highest_item)
            .map(|i| &self.items[i])
    }
}

/// Java: `RoomTile.isValidTile`; `walker` is the entity, or `None` for Java's `null`.
pub fn is_valid_tile(map: &RoomMapping, walker: Option<&Walker>, p: Position) -> bool {
    let Some(tile) = map.tile(p) else {
        return false;
    };
    let highest = tile.highest_item.map(|i| &map.items[i]);
    if let Some(walker) = walker {
        if let Some(door) = &map.bus_door {
            if !door.load(Ordering::Relaxed) && p == crate::game::infobus::door_position() {
                return false;
            }
        }
        if let Some(item) = highest {
            if item.sprite == "poolExit" && item.position == Position::new(19, 19) {
                return walker.swimming;
            }
            // Pets may walk onto their own nest.
            if walker.pet_nest.is_some()
                && item.flat.as_ref().map(|f| f.record.id) == walker.pet_nest
            {
                return true;
            }
        }
    }
    if !tile.entities.is_empty() {
        if highest.is_some_and(|i| i.has(Behaviour::Teleporter)) {
            return true;
        }
        return walker.is_none_or(|w| tile.contains_entity(w.instance_id));
    }
    let walkable = highest.is_none_or(|i| i.is_walkable(walker.is_some(), tile.position));
    if !walkable {
        return match walker {
            Some(w) => highest.is_some_and(|i| i.position == w.position),
            None => false,
        };
    }
    true
}

/// Java: `RoomTile.isValidDiagonalTile`.
pub fn is_valid_diagonal_tile(map: &RoomMapping, walker: Option<&Walker>, p: Position) -> bool {
    let Some(tile) = map.tile(p) else {
        return false;
    };
    if !tile.entities.is_empty() {
        return walker.is_none_or(|w| tile.contains_entity(w.instance_id));
    }
    if let Some(i) = tile.highest_item {
        let item = &map.items[i];
        if item.has(Behaviour::CanSitOnTop) || !item.is_walkable(walker.is_some(), p) {
            return false;
        }
    }
    true
}
