//! The arena (Java: `snowstorm/mapping/SnowStormItem`, `SnowStormTile`, `SnowStormMap`, and
//! `snowstorm/util/SnowStormSpawn`).

use super::objects::Trajectory;
use crate::game::pathfinder::Position;

/// One arena object of `arena_<id>.dat` (or a snowball machine from `_snowmachines.dat`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnowStormItem {
    pub item_id: String,
    pub item_name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub rotation: i32,
    /// What it stops: 0 nothing but walking, 1 quick throws, 2 or more short throws too.
    pub height: i32,
}

impl SnowStormItem {
    /// Java: `isSnowballMachine` (only the left tile of a machine; the other two are
    /// `snowball_machine_hidden`).
    pub fn is_snowball_machine(&self) -> bool {
        self.item_name.eq_ignore_ascii_case("snowball_machine")
    }
}

/// A spawn cluster of `arena_<id>_spawn_clusters.dat` (Java: `SnowStormSpawn`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnowStormSpawn {
    pub x: i32,
    pub y: i32,
    pub radius: i32,
    /// Spawns closer than this (`Position.getDistanceSquared`, a truncated distance) to an
    /// already placed player are refused for the first 150 attempts.
    pub min_distance: i32,
}

/// Java: `SnowStormTile`.
#[derive(Clone, Debug)]
pub struct SnowStormTile {
    blocked: bool,
    /// The first item with the greatest z (Java sorts the tile's items by z, stably, and
    /// keeps the first of the highest).
    highest: Option<usize>,
}

impl SnowStormTile {
    /// Java: `isWalkable`: not an `x` of the heightmap and no item at all on it.
    pub fn is_walkable(&self) -> bool {
        !self.blocked && self.highest.is_none()
    }
}

/// Java: `SnowStormMap`.
#[derive(Clone, Debug)]
pub struct SnowStormMap {
    items: Vec<SnowStormItem>,
    spawn_clusters: Vec<SnowStormSpawn>,
    /// The raw `arena_<id>.dat` (what `OBJECTS_WORLD` is built from).
    compiled_items: String,
    /// After parsing: `0` for a walkable tile, `x` otherwise, each row ended by `\r` (what the
    /// arena's `HEIGHTMAP` sends).
    heightmap: String,
    size_x: i32,
    size_y: i32,
    /// `[x * size_y + y]`.
    tiles: Vec<SnowStormTile>,
}

impl SnowStormMap {
    /// Java: the constructor and `parseHeightMap`. `heightmap` is the `|`-separated model of
    /// `SnowStormMapsManager.getHeightMap`. The width is the first row's length; a shorter row
    /// makes Java's `charAt` throw (the map then fails to load), here an error.
    pub fn new(
        map_id: i32,
        compiled_items: String,
        items: Vec<SnowStormItem>,
        heightmap: &str,
        spawn_clusters: Vec<SnowStormSpawn>,
    ) -> Result<Self, String> {
        let lines = java_split(heightmap, '|');
        if lines.is_empty() {
            return Err(format!("map {map_id}: empty heightmap"));
        }
        let size_y = lines.len() as i32;
        let size_x = lines[0].len() as i32;
        let mut tiles = Vec::with_capacity((size_x * size_y) as usize);
        let mut processed = vec![String::with_capacity(size_x as usize + 1); size_y as usize];

        for x in 0..size_x {
            for y in 0..size_y {
                let line = lines[y as usize].as_bytes();
                let ch = *line.get(x as usize).ok_or_else(|| {
                    format!("map {map_id}: row {y} is shorter than the first row ({size_x})")
                })?;
                let mut on_tile: Vec<usize> = (0..items.len())
                    .filter(|&i| items[i].x == x && items[i].y == y)
                    .collect();
                on_tile.sort_by_key(|&i| items[i].z);
                let mut highest: Option<usize> = None;
                for &i in &on_tile {
                    if highest.is_none_or(|h| items[i].z > items[h].z) {
                        highest = Some(i);
                    }
                }
                tiles.push(SnowStormTile {
                    blocked: ch.eq_ignore_ascii_case(&b'x'),
                    highest,
                });
            }
        }

        for (y, row) in processed.iter_mut().enumerate() {
            for x in 0..size_x {
                let tile = &tiles[(x * size_y) as usize + y];
                row.push(if tile.is_walkable() { '0' } else { 'x' });
            }
            row.push('\r');
        }

        Ok(Self {
            items,
            spawn_clusters,
            compiled_items,
            heightmap: processed.concat(),
            size_x,
            size_y,
            tiles,
        })
    }

    /// Java: `getTile`: `None` off the map.
    pub fn tile(&self, position: Position) -> Option<&SnowStormTile> {
        let (x, y) = (position.x, position.y);
        if x < 0 || y < 0 || x >= self.size_x || y >= self.size_y {
            return None;
        }
        self.tiles.get((x * self.size_y + y) as usize)
    }

    /// Java: `SnowStormTile.getHighestItem`.
    pub fn highest_item(&self, tile: &SnowStormTile) -> Option<&SnowStormItem> {
        tile.highest.map(|i| &self.items[i])
    }

    /// Java: `SnowStormTile.isHeightBlocking`. `None` is a trajectory id Java does not know
    /// (an attack with trajectory 3), which nothing blocks.
    pub fn is_height_blocking(&self, tile: &SnowStormTile, trajectory: Option<Trajectory>) -> bool {
        let Some(item) = self.highest_item(tile) else {
            return false;
        };
        match trajectory {
            Some(Trajectory::Long) | None => false,
            Some(Trajectory::Short) => item.height > 1,
            Some(Trajectory::Quick) => item.height > 0,
        }
    }

    pub fn items(&self) -> &[SnowStormItem] {
        &self.items
    }

    pub fn spawn_clusters(&self) -> &[SnowStormSpawn] {
        &self.spawn_clusters
    }

    pub fn compiled_items(&self) -> &str {
        &self.compiled_items
    }

    /// Java: `getHeightMap` (after parsing).
    pub fn heightmap(&self) -> &str {
        &self.heightmap
    }

    #[cfg(test)]
    pub fn size_x(&self) -> i32 {
        self.size_x
    }

    #[cfg(test)]
    pub fn size_y(&self) -> i32 {
        self.size_y
    }
}

/// Java's `String.split` with a one-character literal pattern: trailing empty strings are
/// dropped, and a string without the separator is returned whole (so `""` gives `[""]`).
pub fn java_split(s: &str, sep: char) -> Vec<&str> {
    if !s.contains(sep) {
        return vec![s];
    }
    let mut parts: Vec<&str> = s.split(sep).collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}
