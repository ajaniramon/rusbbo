//! Java: `game/games/player`: `GamePlayer` and `GameTeam`.

use std::sync::Arc;

use crate::game::pathfinder::Position;
use crate::session::Session;

/// Java: `BattleBallPlayerState` (the client knows all of them; some no power up sets).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlayerState {
    #[default]
    Normal = 0,
    Stunned = 1,
    TurboBoost = 2,
    HighJumps = 3,
    CleaningTiles = 4,
    ColoringForOpponent = 5,
    ClimbingIntoCannon = 6,
    FlyingThroughAir = 7,
    BallBroken = 8,
}

/// Java: `GamePlayer`. It belongs to its game ([`super::Game::players`]); the game's teams and
/// spectators refer to it by `serial`.
pub struct GamePlayer {
    /// Never changes (Java compares the objects).
    pub serial: u64,
    pub session: Arc<Session>,
    pub user_id: u32,
    /// The player's details when the game player was made (Java reads them live; they only
    /// change with a figure change, which Java keeps from the client during a started game).
    pub name: String,
    pub motto: String,
    pub figure: String,
    pub sex: String,
    pub object_id: i32,
    pub game_id: i32,
    pub team_id: i32,
    /// Java: `getSpawnPosition`.
    pub spawn: Position,
    pub entering_game: bool,
    pub spectator: bool,
    pub in_game: bool,
    pub clicked_restart: bool,
    /// BattleBall's player state (Java keeps it on the `GamePlayer`). `None` until a spawn is
    /// assigned (Java's `null`).
    pub player_state: Option<PlayerState>,
    /// Java: `harlequinPlayer` (BattleBall's harlequin power up): the serial of the player this
    /// one colours for.
    pub harlequin: Option<u64>,
    /// Java: `assignedSpawn` (SnowStorm's spawns; the engine keeps the one its spawn search
    /// reads, this one is only set).
    #[allow(dead_code)]
    pub assigned_spawn: bool,
    pub score: i32,
    pub xp: i32,
}

/// The parts of a player's details a game player copies.
#[derive(Clone, Default)]
pub struct PlayerDetails {
    pub user_id: u32,
    pub name: String,
    pub motto: String,
    pub figure: String,
    pub sex: String,
}

impl GamePlayer {
    /// Java: `new GamePlayer(player)`.
    pub fn new(serial: u64, session: Arc<Session>, details: PlayerDetails) -> Self {
        Self {
            serial,
            session,
            user_id: details.user_id,
            name: details.name,
            motto: details.motto,
            figure: details.figure,
            sex: details.sex,
            object_id: -1,
            game_id: -1,
            team_id: -1,
            spawn: Position::default(),
            entering_game: false,
            spectator: false,
            in_game: false,
            clicked_restart: false,
            player_state: None,
            harlequin: None,
            assigned_spawn: false,
            score: 0,
            xp: 0,
        }
    }

    /// Java: `setXp`, never negative.
    pub fn set_xp(&mut self, xp: i32) {
        self.xp = xp.max(0);
    }

    /// Java: `getPlayerState().getStateId()`.
    pub fn state_id(&self) -> i32 {
        self.player_state.unwrap_or_default() as i32
    }
}

/// Java: `GameTeam`.
#[derive(Default)]
pub struct Team {
    pub id: i32,
    /// Java: `playerList`: the serials of its game players, in order.
    pub players: Vec<u64>,
    /// Java: `points`, set by `calculateScore`.
    pub points: i32,
}
