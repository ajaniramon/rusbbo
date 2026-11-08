//! The arena maps (Java: `SnowStormMapsManager`).
//!
//! Java reads arenas 1 to 7 at startup (and on `:reload`) from `tools/snowstorm_maps` in its
//! working directory: `arena_<id>.dat` (the objects, one per `\r`-separated line:
//! `id sprite x y z rotation`), optional `arena_<id>_snowmachines.dat` (`x y` per line, each a
//! 3-tile machine) and optional `arena_<id>_spawn_clusters.dat` (`x y radius minDistance`,
//! `|`-separated). The heightmaps are hard-coded in Java ([`super::heightmaps`]).
//!
//! [`SnowStormMaps::load_dir`] does exactly that; [`SnowStormMaps::embedded`] parses the same
//! files from the repository's `tools/snowstorm_maps`, compiled into the binary, so the server
//! works without a data directory. [`instance`] is the process-wide manager: the directory when
//! it exists, else the embedded copy.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::error;

use super::heightmaps;
use super::mapping::{SnowStormItem, SnowStormMap, SnowStormSpawn, java_split};

/// Java: `SnowStormMapsManager` parses ids 1 to 7.
pub const MAP_IDS: std::ops::RangeInclusive<i32> = 1..=7;

/// Where Java looks for the arena files (relative to the working directory).
pub const DEFAULT_DIR: &str = "tools/snowstorm_maps";

/// Java: `getItemHeight`. `block_small` is commented out in Java, so it is 0.
pub fn item_height(sprite: &str) -> i32 {
    match sprite {
        "sw_tree1" | "sw_tree2" | "sw_tree3" | "sw_tree4" | "block_basic3" | "obst_snowman"
        | "block_arch1" | "block_arch3" | "block_arch1b" | "block_arch3b" => 3,
        "block_basic2" | "block_ice2" => 2,
        "block_basic" | "obst_duck" | "sw_fence" | "block_ice" => 1,
        _ => 0,
    }
}

/// Java: `getHeightMap`: the arena model, rows separated by `|`. Arenas 4, 5, 6 (and any other
/// id) share the default.
pub fn raw_heightmap(map_id: i32) -> &'static str {
    match map_id {
        1 => heightmaps::ARENA_1,
        2 => heightmaps::ARENA_2,
        3 => heightmaps::ARENA_3,
        7 => heightmaps::ARENA_7,
        _ => heightmaps::DEFAULT,
    }
}

/// Java: `parseMap` from the three file contents. Java only catches `IOException`, so a
/// malformed line (a `NumberFormatException` or a missing column) escapes the manager's
/// constructor and no arena loads at all; here only that arena is refused.
pub fn parse_map(
    map_id: i32,
    map_data: &str,
    snowmachines: Option<&str>,
    spawn_clusters: Option<&str>,
) -> Result<SnowStormMap, String> {
    let int = |s: Option<&&str>, what: &str| -> Result<i32, String> {
        let s = s.ok_or_else(|| format!("map {map_id}: missing {what}"))?;
        s.parse::<i32>()
            .map_err(|_| format!("map {map_id}: bad {what} {s:?}"))
    };

    let mut items = Vec::new();
    for line in java_split(map_data, '\r') {
        let data = java_split(line, ' ');
        let name = data
            .get(1)
            .ok_or_else(|| format!("map {map_id}: bad item line {line:?}"))?;
        items.push(SnowStormItem {
            item_id: data[0].to_string(),
            item_name: name.to_string(),
            x: int(data.get(2), "x")?,
            y: int(data.get(3), "y")?,
            z: int(data.get(4), "z")?,
            rotation: int(data.get(5), "rotation")?,
            height: item_height(name),
        });
    }

    if let Some(machines) = snowmachines {
        for line in java_split(machines, '\r') {
            let data = java_split(line, ' ');
            let x = int(data.first(), "machine x")?;
            let y = int(data.get(1), "machine y")?;
            for (dx, name) in [
                (0, "snowball_machine"),
                (1, "snowball_machine_hidden"),
                (2, "snowball_machine_hidden"),
            ] {
                items.push(SnowStormItem {
                    item_id: String::new(),
                    item_name: name.to_string(),
                    x: x + dx,
                    y,
                    z: 0,
                    rotation: 0,
                    height: 1,
                });
            }
        }
    }

    let mut spawns = Vec::new();
    if let Some(clusters) = spawn_clusters {
        for cluster in java_split(clusters, '|') {
            let data = java_split(cluster, ' ');
            spawns.push(SnowStormSpawn {
                x: int(data.first(), "spawn x")?,
                y: int(data.get(1), "spawn y")?,
                radius: int(data.get(2), "spawn radius")?,
                min_distance: int(data.get(3), "spawn min distance")?,
            });
        }
    }

    SnowStormMap::new(
        map_id,
        map_data.to_string(),
        items,
        raw_heightmap(map_id),
        spawns,
    )
}

/// The loaded arenas (Java: `SnowStormMapsManager`). A map that is missing or failed to parse
/// is absent, which the lobby refuses (`SnowStormLobbyTrigger.createGame`).
#[derive(Clone, Debug, Default)]
pub struct SnowStormMaps {
    maps: BTreeMap<i32, Arc<SnowStormMap>>,
}

impl SnowStormMaps {
    /// Java: the constructor: every arena from `dir` (Java: `tools/snowstorm_maps`).
    pub fn load_dir(dir: &Path) -> Self {
        let mut maps = BTreeMap::new();
        for map_id in MAP_IDS {
            let path = dir.join(format!("arena_{map_id}.dat"));
            let Ok(data) = std::fs::read_to_string(&path) else {
                error!(
                    "SnowStorm arena {map_id} not loaded, file not found: {}",
                    path.display()
                );
                continue;
            };
            let read = |suffix: &str| {
                std::fs::read_to_string(dir.join(format!("arena_{map_id}_{suffix}.dat"))).ok()
            };
            let machines = read("snowmachines");
            let spawns = read("spawn_clusters");
            match parse_map(map_id, &data, machines.as_deref(), spawns.as_deref()) {
                Ok(map) => {
                    maps.insert(map_id, Arc::new(map));
                }
                Err(e) => error!("Error when parsing SnowStorm map {map_id}: {e}"),
            }
        }
        Self { maps }
    }

    /// The repository's `tools/snowstorm_maps`, compiled in.
    pub fn embedded() -> Self {
        let mut maps = BTreeMap::new();
        for (map_id, data, machines, spawns) in EMBEDDED {
            match parse_map(map_id, data, machines, spawns) {
                Ok(map) => {
                    maps.insert(map_id, Arc::new(map));
                }
                Err(e) => error!("Error when parsing SnowStorm map {map_id}: {e}"),
            }
        }
        Self { maps }
    }

    /// Java: `getMap`.
    pub fn get(&self, map_id: i32) -> Option<Arc<SnowStormMap>> {
        self.maps.get(&map_id).cloned()
    }
}

macro_rules! arena_file {
    ($name:literal) => {
        include_str!(concat!("../../../../../../../tools/snowstorm_maps/", $name))
    };
}

type EmbeddedArena = (
    i32,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
);

const EMBEDDED: [EmbeddedArena; 7] = [
    (
        1,
        arena_file!("arena_1.dat"),
        Some(arena_file!("arena_1_snowmachines.dat")),
        Some(arena_file!("arena_1_spawn_clusters.dat")),
    ),
    (
        2,
        arena_file!("arena_2.dat"),
        Some(arena_file!("arena_2_snowmachines.dat")),
        Some(arena_file!("arena_2_spawn_clusters.dat")),
    ),
    (
        3,
        arena_file!("arena_3.dat"),
        Some(arena_file!("arena_3_snowmachines.dat")),
        Some(arena_file!("arena_3_spawn_clusters.dat")),
    ),
    (
        4,
        arena_file!("arena_4.dat"),
        None,
        Some(arena_file!("arena_4_spawn_clusters.dat")),
    ),
    (
        5,
        arena_file!("arena_5.dat"),
        None,
        Some(arena_file!("arena_5_spawn_clusters.dat")),
    ),
    (
        6,
        arena_file!("arena_6.dat"),
        None,
        Some(arena_file!("arena_6_spawn_clusters.dat")),
    ),
    (
        7,
        arena_file!("arena_7.dat"),
        None,
        Some(arena_file!("arena_7_spawn_clusters.dat")),
    ),
];

static INSTANCE: RwLock<Option<Arc<SnowStormMaps>>> = RwLock::new(None);

fn load_default() -> SnowStormMaps {
    let dir = Path::new(DEFAULT_DIR);
    if dir.is_dir() {
        SnowStormMaps::load_dir(dir)
    } else {
        SnowStormMaps::embedded()
    }
}

/// Java: `SnowStormMapsManager.getInstance()`.
pub fn instance() -> Arc<SnowStormMaps> {
    if let Some(maps) = INSTANCE.read().as_ref() {
        return maps.clone();
    }
    let mut slot = INSTANCE.write();
    slot.get_or_insert_with(|| Arc::new(load_default())).clone()
}

/// Java: `SnowStormMapsManager.reset()` (`:reload`). Games already created keep the map they
/// were created with (Java re-reads the manager on every `getMap`, so a running game would
/// switch to the reloaded arena mid-game).
pub fn reload() {
    *INSTANCE.write() = Some(Arc::new(load_default()));
}
