//! SnowStorm (Java: `game/games/snowstorm`): the engine of a SnowStorm game, which the shared game
//! core ([`super`]) drives through its seam.
//!
//! # How it is driven
//!
//! A SnowStorm game's [`super::GameKind::SnowStorm`] is a [`SnowStormGame`], made in the lobby
//! (`lobby::create_game`, Java's `SnowStormLobbyTrigger.createGame`, which refuses a map that did
//! not load). [`arena`] has the engine's arms of the seam:
//!
//! - start (`Game.initialise`, `assignSpawnPoints`): the arena model `snowwar_arena_0` from the
//!   map's raw heightmap, "SnowStorm Arena", the length from the choice (or
//!   `snowstorm.game.lifetime.seconds`), the machine objects, and the spawns (random, as in
//!   Java), which give each game player its object id and spawn;
//! - the arena's 300 ms task ([`arena`]'s `snowstorm_arena_tick`): the engine's roster is brought
//!   up to date from the game's teams and the room, then [`SnowStormGame::tick`] runs the timers
//!   due and the turn, whose `SNOWSTORM_GAMESTATUS` goes to everyone in the room (Java's
//!   `SnowStormGameTask`, which, unlike BattleBall's, goes on after the end); the room's first
//!   run starts a new turn task (Java's `loadGameTasks`);
//! - `GAMEEVENT` ([`SnowStormGame::handle_event`]: walk, throw at a player, throw at a tile, make
//!   a snowball), `G_HMAP` (the map's parsed heightmap, `FULLGAMESTATUS`, the map's objects) and
//!   `REQUESTFULLGAMESTATUS`;
//! - `FULLGAMESTATUS`, `GAMERESET`, leaving (the avatar object goes) and the end (each active
//!   player's score is its SnowStorm score, then the teams are summed); everything else (teams,
//!   timers, tickets, history, statistics) is the core's.
//!
//! # Clock
//!
//! Every time is milliseconds since the Unix epoch (Java: `System.currentTimeMillis`). Java runs
//! the arena on `GameScheduler` threads (the 300 ms turn task, one task per flying snowball, two
//! per snowball machine in use, one per timed activity state). Here those are timers inside the
//! engine ([`timers`]), run in due order (each at its own due time) by
//! [`SnowStormGame::advance`] before every event and every turn, so the result does not depend
//! on when the core calls in. The turn itself is the arena task's ([`SnowStormGame::tick`]).
//!
//! The arena room's model is the raw `|`-separated heightmap (what room users stand on), while
//! the client gets the parsed one, where every tile with an item is `x`.
//!
//! # Java quirks kept
//!
//! Players never block each other's walk, and the walk is greedy (it can loop forever); the
//! arrival rotation is always 0; attacks with trajectory 3 land at once; a quick throw from 22
//! tiles or more is announced but never lands; the activity timer counts from twice the state's
//! length; every arrival at a machine starts two more machine timers; snowball and object ids
//! share one counter that a restart does not reset. Each is commented where it happens.
//!
//! # Java classes
//!
//! | Java | Rust |
//! |---|---|
//! | `SnowStormGame` | [`game::SnowStormGame`] |
//! | `SnowStormMapsManager` | [`maps`] (+ [`heightmaps`]) |
//! | `SnowStormTurn` | `tasks::TurnTask::turns` |
//! | `mapping/SnowStormItem`, `SnowStormTile`, `SnowStormMap` | [`mapping`] |
//! | `mapping/SnowStormPathfinder` | [`pathfinder`] (`getOppositionPlayer`: unused in Java, `TODO(snowstorm)`) |
//! | `objects/SnowStormAvatarObject`, `SnowStormMachineObject`, `SnowballObject` | [`objects`] |
//! | `events/*` | [`events::TurnEvent`] |
//! | `tasks/SnowStormGameTask`, `SnowballMovementTask` | [`tasks`] (+ [`timers`]) |
//! | `util/SnowStormAttributes`, `SnowStormActivityState` | [`attributes`] |
//! | `util/SnowStormEvent`, `SnowStormMessage`, `messages/*` | [`messages`] |
//! | `util/SnowStormFuture` | `tasks::FutureEvent` |
//! | `util/SnowStormSpawn` | [`mapping::SnowStormSpawn`] |
//! | `SNOWSTORM_GAMESTATUS`, SnowStorm parts of `FULLGAMESTATUS`/`GAMEINSTANCE`/`INSTANCELIST`, `OBJECTS_WORLD(String)` | [`outgoing`] |

pub mod arena;
pub mod attributes;
pub mod events;
pub mod game;
pub mod heightmaps;
pub mod mapping;
pub mod maps;
pub mod messages;
pub mod objects;
pub mod outgoing;
pub mod pathfinder;
pub mod tasks;
pub mod timers;

#[cfg(test)]
mod tests;

pub use game::SnowStormGame;

use crate::game::pathfinder::Position;

/// Java: `SnowStormGame.MAX_QUICK_THROW_DISTANCE`.
pub const MAX_QUICK_THROW_DISTANCE: i32 = 22;
/// Java: `SnowStormGameTask.maxGameTurns`.
pub const TURNS: usize = 5;
/// Java: both throws refuse a throw within 300 ms of the last.
pub const THROW_COOLDOWN_MS: i64 = 300;
/// Java: `generateSpawn`'s spawn when the map has no clusters (or nothing fits).
pub const SPAWN_FALLBACK: Position = Position {
    x: 15,
    y: 18,
    z: 0.0,
    head_rotation: 0,
    body_rotation: 0,
};

/// Java: `SnowStormLobbyTrigger.createGame`'s answer to a map that did not load.
pub const MAP_UNAVAILABLE_ALERT: &str =
    "This SnowStorm arena is not available right now, please pick another one.";
/// Java: `convertToGameCoordinate` (world units are 1/3200 of a tile; integer division).
pub fn to_game_coordinate(n: i32) -> i32 {
    n / 3200
}

/// Java: `convertToWorldCoordinate` (wrapping like Java's `int`).
pub fn to_world_coordinate(n: i32) -> i32 {
    n.wrapping_mul(3200)
}

/// Java: `Position.getDistanceSquared` with Java's `int` overflow (a far-off walk goal from the
/// client wraps to a negative square, whose `(int) Math.sqrt` is 0). The shared
/// `Position::distance_squared` would panic on such input in debug builds.
pub fn java_distance(a: Position, b: Position) -> i32 {
    let dx = a.x.wrapping_sub(b.x);
    let dy = a.y.wrapping_sub(b.y);
    let squared = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    f64::from(squared).sqrt() as i32
}
