//! SnowStorm (Java: `game/games/snowstorm`), as an engine the shared game core drives.
//!
//! Phase 1 of the port: everything SnowStorm-specific, standalone and unit-tested. Nothing here
//! touches rooms, sessions or the handler registry; the core (`Game`, `GameManager`,
//! `GamePlayer`, teams, lobbies, history, `messages/incoming/games/*`) owns one
//! [`SnowStormGame`] per SnowStorm game and calls it at the points listed below.
//!
//! # Clock
//!
//! Every time is milliseconds since the Unix epoch (Java: `System.currentTimeMillis`). Java runs
//! the arena on `GameScheduler` threads (the 300 ms turn task, one task per flying snowball, two
//! per snowball machine in use, one per timed activity state). Here those are timers inside the
//! engine ([`timers`]): the core calls [`SnowStormGame::advance`]`(now)` whenever
//! [`SnowStormGame::next_deadline`] is reached (and the event handler advances on its own), and
//! forwards the returned [`Output`]s. Each timer runs at its own due time, so results do not
//! depend on the polling rate. Randomness (spawns) comes in as Java's `nextInt(bound)`.
//!
//! # What the core must provide and call
//!
//! Lobby (`SnowStormLobbyTrigger`, `INITIATECREATEGAME`):
//! - `GAMEPARAMETERS` for SnowStorm: `fieldType` (default 1, 1..7), `numTeams` (2, 2..4),
//!   `gameLengthChoice` (1, 1..3), `name`. Validate a creation with [`check_create`] (Java's
//!   order: map id, map loaded, teams, name; the length is not checked), then
//!   [`SnowStormGame::new`]`(map_id, teams, length_choice, &maps::instance())` and add the
//!   creator to team 0.
//! - Lobby entry: the generic `LOUNGEINFO`/`GAMEPLAYERINFO`/points, plus
//!   [`ALPHA_ALERT`] when `snowstorm.alpha.alert` is set. `GAMEPLAYERINFO` shows the
//!   `snowstorm_score_all_time` statistic.
//!
//! Players: [`SnowStormGame::add_player`] whenever Java's `movePlayer` puts a player on a team
//! (same order: appended to the team), [`SnowStormGame::remove_player`] on `leaveGame`/kick,
//! [`SnowStormGame::set_in_game`] with `GamePlayer.setInGame`, and
//! [`SnowStormGame::set_in_room`] when the player enters or leaves the arena room.
//!
//! Start (`Game.startGame`/`restartGame`):
//! 1. The core's `Game.initialise(lifetime, "SnowStorm Arena", model)`, with
//!    [`SnowStormGame::lifetime_seconds`]`(snowstorm.game.lifetime.seconds)` and the room model
//!    `snowwar_arena_0` (door 0,0,0, rotation 0) built from [`maps::raw_heightmap`]`(map_id)`;
//!    then [`SnowStormGame::initialise`] (machine objects).
//! 2. The core's `regenerateCollisionMap`, then [`SnowStormGame::assign_spawn_points`]; copy
//!    each [`SpawnAssignment`] onto the `GamePlayer` (object id, spawn, score 0; on a restart
//!    the room user's instance id and position too).
//! 3. `gamePrepare` charges tickets (`snowstorm.ticket.charge`) like BattleBall does.
//! 4. Start/canStart: [`SnowStormGame::can_start_one_team`] is the one-team rule (two players;
//!    `STARTGAME` answers `ALERT(`[`START_ALERT_ONE_TEAM`]`)` instead of `STARTFAILED`); restart
//!    uses [`SnowStormGame::has_enough_players`]. `gameTick` does nothing and the timer may
//!    always continue (the game ends on time or when nobody is left).
//!
//! Arena room (Java: `RoomTaskManager.loadGameTasks`/`stopTasks`):
//! [`SnowStormGame::start_turns`] when the room starts its tasks (a running task is kept) and
//! [`SnowStormGame::stop_turns`] when it stops them. While running, every 300 ms the engine
//! returns [`Output::SpamTick`] for each game player in the room (Java:
//! `RoomPlayer.handleSpamTicks`; SnowStorm also doubles the chat spam window,
//! `chatSpamTicks = 16`) and [`Output::Broadcast`] of `SNOWSTORM_GAMESTATUS` for every player in
//! the room.
//!
//! Requests:
//! - `GAMEEVENT` (171): Java's generic checks, then [`SnowStormGame::handle_event`] (event types
//!   0 walk, 1 throw at a player, 2 throw at a tile, 3 make a snowball).
//! - `G_HMAP` in the arena for a game player: [`outgoing::heightmap`], then
//!   [`SnowStormGame::full_game_status`], then [`outgoing::objects_world`].
//! - `REQUESTFULLGAMESTATUS` (173): [`SnowStormGame::full_game_status`] (SnowStorm games only).
//! - Arena entry (generic `RoomEntityManager.enterRoom`): an entering game player's instance
//!   id is its `GamePlayer.objectId`, i.e. its [`SpawnAssignment::object_id`].
//!
//! Outgoing composers the core writes, with the SnowStorm parts from here:
//! - `GAMEINSTANCE` (waiting/started): [`SnowStormGame::write_length_choice`] after the
//!   creator's name; finished: [`outgoing::write_finished_length_choice`] with the history's
//!   extra data. `INSTANCELIST` writes [`SnowStormGame::game_length`] there (finished games:
//!   [`length_choice_seconds`] of the extra data, 0 if none).
//! - `FULLGAMESTATUS`: [`SnowStormGame::full_game_status`] (the whole message).
//! - `GAMESTATUS`: the SnowStorm overload is never constructed in Java (SnowStorm sends
//!   `SNOWSTORM_GAMESTATUS` under the same header 244), so nothing to port.
//! - `GAMEEND`: generic; the players' scores are their `GamePlayer` scores, which
//!   [`SnowStormGame::final_scores`] sets at the end.
//!
//! End (`finishGame`): set each `GamePlayer.score` from [`SnowStormGame::final_scores`], then
//! the generic finish (team points are the sum of their players' scores; the history's extra
//! data is the length choice; statistics `snowstorm_score_month`, `snowstorm_score_all_time`,
//! `snowstorm_games_won`; `snowstorm.increase.points`, `snowstorm.restart.game.seconds`).
//!
//! `:reload` calls [`maps::reload`].
//!
//! # Core touch points for phase 2
//!
//! The Java files outside `game/games/snowstorm` that mention SnowStorm, and what each needs:
//! - `game/games/Game.java` (`initialise`, `startGame`, `restartGame`, `finishGame`,
//!   `canGameStart`, `leaveGame`, `createObjectId`) and `GamePlayer`/`GameTeam`: the calls
//!   above; `GameTeam.calculateScore` sums player scores for SnowStorm.
//! - `game/room/managers/RoomTaskManager.java`: `start_turns`/`stop_turns` and a timer that
//!   calls `advance` at `next_deadline` (per arena room, next to `game/room/tasks.rs`).
//! - `game/room/managers/RoomEntityManager.java` (arena entry: instance id = object id,
//!   walking blocked until `GAMESTART`) and `ROOM_DIRECTORY` with room id -1.
//! - `game/room/models/triggers/SnowStormLobbyTrigger.java` (model `snowwar_lobby_1`, public
//!   room 67 "Snowstorm Lobby"): `check_create`, `ALPHA_ALERT`, `onRoomLeave` stops observing.
//! - `messages/incoming/games/GAMEEVENT.java`, `REQUESTFULLGAMESTATUS.java`,
//!   `INITIATECREATEGAME.java` (the parameter list), `STARTGAME.java` (the one-team alert),
//!   `REQUEST_GAME_LOBBY.java` (`setLastLobbyRedirection(SNOWSTORM)`).
//! - `messages/incoming/rooms/G_HMAP.java`: heightmap, full status, objects (above).
//! - `messages/outgoing/games/FULLGAMESTATUS`, `GAMEINSTANCE`, `INSTANCELIST`,
//!   `GAMEPLAYERINFO` (`snowstorm_score_all_time`), `GAMEEND`.
//! - `game/room/entities/RoomPlayer.java`: chat spam ticks are 16 in a SnowStorm game.
//! - `game/games/history/GameHistory.java` (extra data = length choice, `getGameLength`),
//!   `game/games/tasks/GameFinishTask.java` and `GameManager.java` (SnowStorm statistics),
//!   `dao/mysql/HighscoreDao.java` (SnowStorm high scores), `AchievementGamePlayed`
//!   (counts `snowstorm_games_won`).
//! - `game/room/RoomData.java`: the SnowStorm lobby's visitor count adds the first started
//!   SnowStorm game's active players and spectators, and its maximum every started game's
//!   maximum players.
//! - `game/messenger/MessengerUser.java`: a friend in the `snowwar` model is not followable.
//! - `Havana.java` loads the maps at startup ([`maps::instance`]);
//!   `ReloadCommand` resets them ([`maps::reload`]).
//! - Settings (`GameConfigWriter`): `snowstorm.create.game.enabled` (true),
//!   `snowstorm.start.minimum.active.teams` (2), `snowstorm.preparing.game.seconds` (10),
//!   `snowstorm.game.lifetime.seconds` (0), `snowstorm.restart.game.seconds` (30),
//!   `snowstorm.ticket.charge` (2), `snowstorm.increase.points` (true),
//!   `snowstorm.alpha.alert`.
//!
//! The arena room's model is the raw `|`-separated heightmap (what room users walk on, through
//! the core's normal room mapping), while the client gets the parsed one, where every tile with
//! an item is `x`.
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

// TODO(snowstorm): phase 2 wires this onto the game core; until then nothing outside the tests
// calls it.
#![allow(dead_code)]

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

#[allow(unused_imports)] // TODO(snowstorm): used once wired (phase 2).
pub use game::{
    Output, PlayerInfo, SnowStormGame, SnowStormPlayer, SpawnAssignment, length_choice_seconds,
};

use crate::game::pathfinder::Position;

/// Java: `SnowStormGame.MAX_QUICK_THROW_DISTANCE`.
pub const MAX_QUICK_THROW_DISTANCE: i32 = 22;
/// Java: the `UpdateTask` period.
pub const TURN_MS: i64 = 300;
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
/// Java: `STARTGAME` for a one-team SnowStorm game without two players.
pub const START_ALERT_ONE_TEAM: &str = "There needs to be at least two players to start this match";
/// Java: `SnowStormLobbyTrigger.onRoomEntry` when `snowstorm.alpha.alert` is set.
pub const ALPHA_ALERT: &str = "Welcome to SnowStorm testing!<br><br>The game is almost complete but may have a few quirks to figure out. Please report any issues (aside from desync) to Alex.<br><br>Thanks to the following for making it possible:<br>  - Sefhriloff<br>  - Ascii";

/// Why a creation is refused (Java: `SnowStormLobbyTrigger.createGame`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateRefusal {
    /// Nothing is sent.
    Silent,
    /// `ALERT(`[`MAP_UNAVAILABLE_ALERT`]`)`.
    MapUnavailable,
}

/// Java: `SnowStormLobbyTrigger.createGame`'s checks, in its order.
pub fn check_create(
    map_id: i32,
    teams: i32,
    name: &str,
    maps: &maps::SnowStormMaps,
) -> Result<(), CreateRefusal> {
    if !maps::MAP_IDS.contains(&map_id) {
        return Err(CreateRefusal::Silent);
    }
    if maps.get(map_id).is_none() {
        return Err(CreateRefusal::MapUnavailable);
    }
    if !(1..=4).contains(&teams) {
        return Err(CreateRefusal::Silent);
    }
    if name.is_empty() {
        return Err(CreateRefusal::Silent);
    }
    Ok(())
}

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
