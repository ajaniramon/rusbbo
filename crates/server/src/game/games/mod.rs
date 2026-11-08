//! The lobby games, BattleBall and SnowStorm (Java: `game/games`: `Game`, `GameManager`,
//! `player/`, `history/`, `tasks/GameFinishTask`, the lobby triggers `BattleballLobbyTrigger` and
//! `SnowStormLobbyTrigger`, and `battleball/`).
//!
//! # How it fits together
//!
//! A game is made in its lobby (`bb_lobby_1`, `snowwar_lobby_1`: room models whose trigger is
//! `battleball_lobby_trigger` / `snowstorm_lobby_trigger`, see [`lobby`]), waits for players, and
//! on `STARTGAME` gets an arena: a [`Room`](crate::game::room::Room) of its own (Java's
//! `new Room()` with `isGameArena`), which is not in the room registry but in the world's arena
//! registry under a key of its own ([`crate::world::World::register_arena`]); the client is sent
//! there with `GAMELOCATION` and enters it with `ROOM_DIRECTORY` and room id -1. The arena has no
//! entity or status task: its game type's task runs instead ([`Games::arena_tick`]). At the end
//! the players go back to the lobby, and the finished game is stored (`games_played_history`) and
//! listed there.
//!
//! All games live in one [`GameManager`] behind one lock ([`Games`]). Lock order: player, then
//! room, then games; never a player's lock while the games are locked. Every change runs in
//! [`GameManager::run`] with an [`Op`]: what it sends goes to an outbox that is flushed before
//! the games' lock is released, and what needs a player's lock or another room ([`Op::defer`])
//! runs afterwards (with the room passed to `run`, after that room's lock too). `GAMEINSTANCE`
//! is composed when the outbox is flushed, from the game as the operation left it: Java encodes
//! its composers lazily, on the receiver's event loop, so a handler that sends it twice (a
//! spectator who joins a team: `traces/battleball-lobby.json`) shows the end state twice. One
//! that was sent while the game had another state shows that state instead (a restart sends it
//! while the game is still over, and its new state comes a little later:
//! `traces/battleball-restart.json`); an ended game's shows nothing but its state.
//!
//! A [`GamePlayer`](player::GamePlayer) belongs to its game (Java's `GamePlayer`, shared by the
//! game's lists and the player's `RoomPlayer.gamePlayer`); which one is the player's current one
//! is [`Games::current`] (by connection id). Game players and games are found by a serial and a
//! uid that never change (Java compares objects; a game's id changes when it restarts).
//!
//! # The seam: what a game type provides
//!
//! The game-type specific parts (Java's abstract `Game` with `BattleBallGame` and
//! `SnowStormGame`) are the variants of [`GameKind`] and every `match` on it. Grep
//! `TODO(snowstorm)` for the arms SnowStorm still lacks. A game type provides:
//!
//! - its state: a [`GameKind`] variant (BattleBall keeps its tiles, power ups, objects and event
//!   queues there; per-player state is either on [`GamePlayer`](player::GamePlayer), as Java
//!   keeps BattleBall's `playerState`, or in the variant keyed by the game player's serial), and
//!   how a game of it is made from the lobby's parameters (`lobby::create_game`, Java's
//!   `createGame` of the lobby trigger);
//! - in `game.rs`, `impl Game` after "The game type seam": `has_enough_players` and
//!   `can_timer_continue` (Java's abstract methods), `arena_setup` (Java's `initialise`: the
//!   arena's model, the room's name and the game's length; `None` refuses to start),
//!   `build_map`, `assign_spawn_points_hook`, the hooks `on_start`, `game_prepare_hook`,
//!   `game_prepare_tick_hook`, `game_started_hook`, `game_tick_hook` and `remove_player_object`,
//!   the extra fields of `GAMEINSTANCE`/`INSTANCELIST` (`length_choice`, `game_length`,
//!   `power_ups`), `history_extra_data` (`games_played_history.extra_data`), the scores
//!   (`calculate_team_points`, `calculate_player_score`, for `GAMEEND` and the history),
//!   `full_game_status` (`FULLGAMESTATUS`) and `game_reset` (`GAMERESET`);
//! - the arena's task: [`Games::arena_tick`] (in `battleball/mod.rs`; Java's `BattleBallTask`
//!   every 500 ms, `SnowStormGameTask` every 300 ms: [`GameType::arena_task_period`]);
//! - the messages: `GAMEEVENT` (`messages/incoming/games.rs`, `game_event`: BattleBall's moves and
//!   power ups, SnowStorm's `SnowStormMessageHandler`), `REQUESTFULLGAMESTATUS`, and `G_HMAP`'s
//!   heightmap and items of the arena (`messages/incoming/rooms.rs`).
//!
//! Everything else (teams, observers, spectators, the timers, the start, end and restart,
//! leaving, the tickets, the history, the statistics) is shared and needs nothing from the
//! game type. BattleBall is in [`battleball`]. SnowStorm stops at the lobby here (its engine lives
//! in `game/games/snowstorm/`, to be wired in): a SnowStorm game can be made, listed, joined and
//! left, but `STARTGAME` does not start it yet.

pub mod battleball;
pub mod game;
pub mod history;
pub mod lobby;
pub mod player;

use std::collections::HashMap;
use std::sync::Arc;

use havana_protocol::Message;
use havana_storage::Store;
use parking_lot::Mutex;
use tracing::error;

use crate::game::pathfinder::Position;
use crate::game::room::Room;
use crate::game::room::model::RoomModel;
use crate::session::Session;
use crate::world::World;

pub use game::{Game, GameKind};
pub use history::GameHistory;

/// Java: `GameType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameType {
    BattleBall,
    SnowStorm,
    WobbleSquabble,
}

impl GameType {
    /// Java's `GameType.values()`.
    pub const ALL: [GameType; 3] = [Self::BattleBall, Self::SnowStorm, Self::WobbleSquabble];

    /// Java: `name()`, as stored in `games_played_history.game_type`.
    pub fn name(self) -> &'static str {
        match self {
            Self::BattleBall => "BATTLEBALL",
            Self::SnowStorm => "SNOWSTORM",
            Self::WobbleSquabble => "WOBBLE_SQUABBLE",
        }
    }

    /// Java: `name().toLowerCase()`, the prefix of the settings (`battleball.ticket.charge`...)
    /// and the `type` column of the other game tables.
    pub fn key(self) -> &'static str {
        match self {
            Self::BattleBall => "battleball",
            Self::SnowStorm => "snowstorm",
            Self::WobbleSquabble => "wobble_squabble",
        }
    }

    /// Java: `getLobbyModel`.
    pub fn lobby_model(self) -> &'static str {
        match self {
            Self::BattleBall => "bb_lobby_1",
            Self::SnowStorm => "snowwar_lobby_1",
            Self::WobbleSquabble => "md_a",
        }
    }

    /// The game lobby of a room model's trigger (Java: `getRoomTrigger() instanceof
    /// GameLobbyTrigger`).
    pub fn of_lobby_trigger(trigger: &str) -> Option<GameType> {
        match trigger {
            "battleball_lobby_trigger" => Some(Self::BattleBall),
            "snowstorm_lobby_trigger" => Some(Self::SnowStorm),
            _ => None,
        }
    }

    /// The period of the arena's task (Java: `RoomTaskManager.loadGameTasks`).
    pub fn arena_task_period(self) -> std::time::Duration {
        match self {
            Self::SnowStorm => std::time::Duration::from_millis(300),
            _ => std::time::Duration::from_millis(500),
        }
    }
}

/// Java: `GameState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Waiting = 0,
    Started = 1,
    Ended = 2,
}

/// Java: `GameRank`.
#[derive(Clone, Debug)]
pub struct Rank {
    pub game_type: GameType,
    pub title: String,
    pub min_points: i32,
    pub max_points: i32,
}

/// Java: `GameSpawn` (a position with the rotation as its z, see `GameDao.getGameSpawns`).
#[derive(Clone, Debug)]
pub struct Spawn {
    pub game_type: GameType,
    pub map_id: i32,
    pub team_id: i32,
    pub x: i32,
    pub y: i32,
    pub rotation: i32,
}

/// The game data loaded at startup (Java: `GameManager`'s lists).
#[derive(Default)]
pub struct GameData {
    pub ranks: Vec<Rank>,
    /// `bb_arena_<map>` / `ss_arena_<map>` (Java: `GameDao.getGameMaps`).
    pub models: Vec<Arc<RoomModel>>,
    pub spawns: Vec<Spawn>,
    /// Java: `GameDao.getBattleballTileMaps`.
    pub battleball_maps: Vec<battleball::TileMap>,
}

impl GameData {
    pub fn load(store: &Store) -> Self {
        let parse_type = |t: &str| match t {
            "battleball" => Some(GameType::BattleBall),
            "snowstorm" => Some(GameType::SnowStorm),
            _ => None,
        };
        let mut data = GameData::default();
        match store.game_ranks() {
            Ok(rows) => {
                data.ranks = rows
                    .into_iter()
                    .filter_map(|r| {
                        Some(Rank {
                            game_type: parse_type(&r.game_type)?,
                            title: r.title,
                            min_points: r.min_points,
                            max_points: r.max_points,
                        })
                    })
                    .collect();
            }
            Err(e) => error!("loading games_ranks: {e}"),
        }
        match store.game_maps() {
            Ok(rows) => {
                for row in rows {
                    let prefix = if row.game_type == "battleball" {
                        "bb"
                    } else {
                        "ss"
                    };
                    let name = format!("{prefix}_arena_{}", row.map_id);
                    data.models.push(Arc::new(arena_model(
                        &name,
                        &row.heightmap,
                        i32::MAX,
                        i32::MAX,
                    )));
                    if row.game_type == "battleball" {
                        data.battleball_maps
                            .push(battleball::TileMap::parse(row.map_id, &row.tile_map));
                    }
                }
            }
            Err(e) => error!("loading games_maps: {e}"),
        }
        match store.game_spawns() {
            Ok(rows) => {
                data.spawns = rows
                    .into_iter()
                    .filter_map(|s| {
                        Some(Spawn {
                            game_type: parse_type(&s.game_type)?,
                            map_id: s.map_id,
                            team_id: s.team_id,
                            x: s.x,
                            y: s.y,
                            rotation: s.rotation,
                        })
                    })
                    .collect();
            }
            Err(e) => error!("loading games_player_spawns: {e}"),
        }
        data
    }

    /// Java: `GameManager.getModel`.
    pub fn model(&self, game_type: GameType, map_id: i32) -> Option<Arc<RoomModel>> {
        let prefix = if game_type == GameType::BattleBall {
            "bb"
        } else {
            "ss"
        };
        let name = format!("{prefix}_arena_{map_id}");
        self.models.iter().find(|m| m.name == name).cloned()
    }

    /// Java: `GameManager.getGameSpawns`, in table order.
    pub fn spawns(&self, game_type: GameType, map_id: i32, team_id: i32) -> Vec<&Spawn> {
        self.spawns
            .iter()
            .filter(|s| s.game_type == game_type && s.map_id == map_id && s.team_id == team_id)
            .collect()
    }

    /// Java: `GameManager.getRankByPoints`.
    pub fn rank_by_points(&self, game_type: GameType, points: i32) -> Option<&Rank> {
        self.ranks
            .iter()
            .filter(|r| r.game_type == game_type)
            .find(|r| points >= r.min_points && (r.max_points == 0 || points <= r.max_points))
    }
}

/// Java: `new RoomModel(name, name, doorX, doorY, Double.MAX_VALUE, 0, heightmap, null)` as
/// `GameDao.getGameMaps` builds the arenas' models. Java's `split("|")` drops the empty row a
/// trailing `|` leaves.
pub fn arena_model(name: &str, heightmap: &str, door_x: i32, door_y: i32) -> RoomModel {
    RoomModel::parse(&havana_storage::RoomModel {
        model_id: name.to_string(),
        model_name: name.to_string(),
        door_x,
        door_y,
        door_z: f64::MAX,
        door_dir: 0,
        heightmap: heightmap.trim_end_matches('|').to_string(),
        ..Default::default()
    })
}

/// Java: `GameManager`: the static data, and every game behind one lock.
pub struct GameManager {
    pub data: GameData,
    state: Mutex<Games>,
    /// Java: `RoomManager.getRoomByModel(lobbyModel)`'s room id, looked up once.
    lobby_rooms: Mutex<HashMap<GameType, Option<u32>>>,
}

/// Something a game operation sends: a message, or a `GAMEINSTANCE` composed when the outbox
/// is flushed.
pub(crate) enum Out {
    Message(Message),
    /// A game's uid, and its state when it was sent.
    Instance(u64, GameState),
}

/// One game operation's context (see the module docs).
pub struct Op<'a> {
    pub world: &'a World,
    /// The arena room, when the caller holds its lock (every arena operation does).
    pub room: Option<&'a mut Room>,
    out: Vec<(Arc<Session>, Out)>,
    deferred: Vec<Task>,
}

/// Work deferred until every lock is released.
type Task = Box<dyn FnOnce(&World) + Send>;

impl Op<'_> {
    /// Java: `player.send(composer)`.
    pub fn send(&mut self, session: &Arc<Session>, msg: Message) {
        self.out.push((session.clone(), Out::Message(msg)));
    }

    /// Java: `player.send(new GAMEINSTANCE(game))`, composed when the outbox is flushed (see the
    /// module docs); `state` is the game's state now.
    pub fn send_instance(&mut self, session: &Arc<Session>, game_uid: u64, state: GameState) {
        self.out
            .push((session.clone(), Out::Instance(game_uid, state)));
    }

    /// Runs `task` once every lock is released.
    pub fn defer(&mut self, task: impl FnOnce(&World) + Send + 'static) {
        self.deferred.push(Box::new(task));
    }
}

impl GameManager {
    pub fn new(store: &Store) -> Self {
        let mut games = Games::default();
        games.refresh_played_games(store);
        Self {
            data: GameData::load(store),
            state: Mutex::new(games),
            lobby_rooms: Mutex::new(HashMap::new()),
        }
    }

    /// Runs a game operation (see the module docs). `room` is the room whose lock the caller
    /// holds, if any; what the operation defers then runs after that room's lock.
    pub fn run<R>(
        &self,
        world: &World,
        room: Option<&mut Room>,
        f: impl FnOnce(&mut Games, &mut Op) -> R,
    ) -> R {
        let mut op = Op {
            world,
            room,
            out: Vec::new(),
            deferred: Vec::new(),
        };
        let result = {
            let mut games = self.state.lock();
            let result = f(&mut games, &mut op);
            let out = std::mem::take(&mut op.out);
            for (session, out) in out {
                let msg = match out {
                    Out::Message(msg) => msg,
                    Out::Instance(uid, state) => match games.any_game_by_uid(uid) {
                        Some(game) if game.state == state || state != GameState::Ended => {
                            game.instance_message()
                        }
                        Some(_) => crate::messages::outgoing::games::game_instance(
                            &crate::messages::outgoing::games::InstanceView::Ended,
                        ),
                        None => continue,
                    },
                };
                session.send(msg);
            }
            games.graveyard.clear();
            result
        };
        let deferred = std::mem::take(&mut op.deferred);
        match op.room.take() {
            Some(room) => {
                for task in deferred {
                    room.defer(task);
                }
            }
            None => {
                for task in deferred {
                    task(world);
                }
            }
        }
        result
    }

    /// The game's arena room, if it has one.
    pub fn arena_of(&self, game_uid: u64) -> Option<crate::game::room::RoomRef> {
        let games = self.state.lock();
        games.game_by_uid(game_uid).and_then(|g| g.room.clone())
    }

    /// Java: `RoomManager.getRoomByModel(gameType.getLobbyModel())`'s id.
    pub fn lobby_room_id(&self, world: &World, game_type: GameType) -> Option<u32> {
        let mut cache = self.lobby_rooms.lock();
        *cache.entry(game_type).or_insert_with(|| {
            let model = game_type.lobby_model();
            match world.store.rooms_by_owner(0) {
                Ok(rooms) => rooms
                    .into_iter()
                    .find(|r| {
                        world
                            .room_models
                            .get(&r.model)
                            .is_some_and(|m| m.name == model || m.id == model)
                    })
                    .map(|r| r.id),
                Err(e) => {
                    error!("finding the {model} lobby: {e}");
                    None
                }
            }
        })
    }

    /// Java: `RoomData.getVisitorsNow/getVisitorsMax` of a lobby: the first started game's
    /// players and spectators (Java's `findFirst`), and the capacity of every started game.
    pub fn lobby_visitors(&self, trigger: &str) -> (i32, i32) {
        let Some(game_type) = GameType::of_lobby_trigger(trigger) else {
            return (0, 0);
        };
        let games = self.state.lock();
        let started: Vec<&Game> = games
            .games
            .iter()
            .filter(|g| g.game_type == game_type && g.state == GameState::Started)
            .collect();
        let now = started.first().map_or(0, |g| {
            g.active_players().count() as i32 + g.spectators.len() as i32
        });
        let max = started.iter().map(|g| g.max_players()).sum();
        (now, max)
    }

    /// Whether the player's game is SnowStorm, whose ticks are twice as fast (Java:
    /// `RoomPlayer.talk`'s spam ticks).
    pub fn in_snowstorm_game(&self, session_id: u64) -> bool {
        let games = self.state.lock();
        games
            .current_game(session_id)
            .is_some_and(|(gi, _)| games.games[gi].game_type == GameType::SnowStorm)
    }

    /// Java: `silentlyEnterRoom`'s check: the object id of a player on the way to a game's
    /// arena (their instance id there).
    pub fn entering_object_id(&self, session_id: u64) -> Option<i32> {
        let games = self.state.lock();
        let (gi, pi) = games.current(session_id)?;
        let gp = &games.games[gi].players[pi];
        gp.entering_game.then_some(gp.object_id)
    }

    /// Java: `setEnteringGame(false)` once the player entered a room.
    pub fn entered_game(&self, session_id: u64) {
        let mut games = self.state.lock();
        if let Some((gi, pi)) = games.current(session_id) {
            games.games[gi].players[pi].entering_game = false;
        }
    }

    /// Java: `Player.dispose`'s part for the games: stop observing, leave the game.
    pub fn player_disposed(&self, world: &World, session_id: u64) {
        self.run(world, None, |games, op| {
            if games.observing(session_id) != -1 {
                games.stop_observing(session_id);
            }
            if let Some((target, Some(pi))) = games.current_game(session_id) {
                let serial = games.games[target].players[pi].serial;
                games.leave_game(target, serial, op);
            }
            games.forget_session(session_id);
        });
    }

    /// Java: `ROOM_DIRECTORY` with room -1: `None` when the player is not on the way to a game's
    /// arena; `Some(None)` when that game (or its arena) is gone, which forgets the game player;
    /// else the arena and the player's spawn.
    pub fn arena_entry(
        &self,
        session_id: u64,
    ) -> Option<Option<(crate::game::room::RoomRef, Position)>> {
        let mut games = self.state.lock();
        let (gi, pi) = games.current(session_id)?;
        if !games.games[gi].players[pi].entering_game {
            return None;
        }
        let spawn = games.games[gi].players[pi].spawn;
        let arena = match games.current_game(session_id) {
            Some((target, _)) => games.games[target].room.clone(),
            None => None,
        };
        match arena {
            Some(arena) => Some(Some((arena, spawn))),
            None => {
                games.games[gi].players[pi].entering_game = false;
                games.set_current(session_id, None);
                Some(None)
            }
        }
    }

    /// Java: `G_HMAP`'s `FULLGAMESTATUS` for a player in their game's arena. The positions of
    /// the game's players are read from their rooms, one room at a time (Java reads each
    /// player's room user wherever they are, still in the lobby or already in the arena).
    pub fn full_game_status_for(&self, world: &World, session: &Session) -> Option<Message> {
        let (uid, players) = {
            let games = self.state.lock();
            let (gi, _) = games.current_game(session.id)?;
            let game = &games.games[gi];
            let arena = game.room.as_ref()?.id;
            if session.room_key() != Some(arena) {
                return None;
            }
            let players: Vec<(u64, Arc<Session>)> = game
                .players
                .iter()
                .map(|p| (p.serial, p.session.clone()))
                .collect();
            (game.uid, players)
        };
        let mut positions = HashMap::new();
        for (serial, s) in players {
            let Some(room) = s.room_key().and_then(|key| world.loaded_room(key)) else {
                continue;
            };
            let r = room.lock();
            if let Some(idx) = r.entity_index(s.id) {
                positions.insert(serial, r.entities[idx].position);
            }
        }
        let games = self.state.lock();
        let game = games.game_by_uid(uid)?;
        Some(game.full_game_status(world, None, &positions))
    }

    /// Java: `G_STAT`'s `GAMESTART` for a spectator: the seconds left of a game that is on.
    pub fn started_seconds_left(&self, session_id: u64) -> Option<i32> {
        let games = self.state.lock();
        let (gi, _) = games.current_game(session_id)?;
        let game = &games.games[gi];
        game.game_started.then_some(game.total_seconds_left)
    }

    /// The player's current game player: (spectator, in game, entering the game).
    pub fn player_flags(&self, session_id: u64) -> Option<(bool, bool, bool)> {
        let games = self.state.lock();
        let (gi, pi) = games.current(session_id)?;
        let gp = &games.games[gi].players[pi];
        Some((gp.spectator, gp.in_game, gp.entering_game))
    }
}

/// Java: `GameManager`'s mutable state.
#[derive(Default)]
pub struct Games {
    /// Java: `gameCounter`.
    pub(crate) counter: i32,
    /// Java: `finishedGameCounter`.
    pub(crate) finished_counter: i32,
    /// Java: `GameManager.games`, in order.
    pub games: Vec<Game>,
    /// Java: `RoomPlayer.gamePlayer`, by connection id: the serial of the player's game player.
    current: HashMap<u64, u64>,
    /// Java: `RoomPlayer.observingGameId`, by connection id.
    observing: HashMap<u64, i32>,
    /// Java: `RoomPlayer.lastLobbyRedirection`, by connection id.
    last_lobby: HashMap<u64, GameType>,
    /// Java: `lastPlayedGames`.
    history: HashMap<GameType, Vec<GameHistory>>,
    next_serial: u64,
    next_uid: u64,
    /// Games removed during the current operation, kept until its outbox is flushed.
    graveyard: Vec<Game>,
}

impl Games {
    /// Java: `GameManager.refreshPlayedGames`: the last 15 games of each type, with ids from the
    /// game counter.
    pub fn refresh_played_games(&mut self, store: &Store) {
        self.history.clear();
        for game_type in GameType::ALL {
            let mut list = history::last_played_games(store, game_type);
            for game in &mut list {
                game.id = self.counter;
                self.counter += 1;
            }
            self.history.insert(game_type, list);
        }
    }

    /// Java: `GameManager.createId`.
    pub fn create_id(&mut self) -> i32 {
        self.counter += 1;
        self.counter
    }

    pub(crate) fn new_serial(&mut self) -> u64 {
        self.next_serial += 1;
        self.next_serial
    }

    pub(crate) fn new_uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    /// Java: `GameManager.getGameById`.
    pub fn game_by_id(&self, id: i32) -> Option<usize> {
        self.games.iter().position(|g| g.id == id)
    }

    pub fn game_by_uid(&self, uid: u64) -> Option<&Game> {
        self.games.iter().find(|g| g.uid == uid)
    }

    pub fn game_index(&self, uid: u64) -> Option<usize> {
        self.games.iter().position(|g| g.uid == uid)
    }

    /// A game of this operation, also one removed during it.
    fn any_game_by_uid(&self, uid: u64) -> Option<&Game> {
        self.game_by_uid(uid)
            .or_else(|| self.graveyard.iter().find(|g| g.uid == uid))
    }

    /// Where a game player is: its game's index and its index in the game's players.
    pub fn find_player(&self, serial: u64) -> Option<(usize, usize)> {
        self.games.iter().enumerate().find_map(|(gi, g)| {
            g.players
                .iter()
                .position(|p| p.serial == serial)
                .map(|pi| (gi, pi))
        })
    }

    /// Java: `getRoomUser().getGamePlayer()`.
    pub fn current(&self, session_id: u64) -> Option<(usize, usize)> {
        let serial = *self.current.get(&session_id)?;
        self.find_player(serial)
    }

    /// Java: `getRoomUser().setGamePlayer(gamePlayer)`.
    pub fn set_current(&mut self, session_id: u64, serial: Option<u64>) {
        match serial {
            Some(serial) => {
                self.current.insert(session_id, serial);
            }
            None => {
                self.current.remove(&session_id);
            }
        }
    }

    /// Whether `serial` is the player's current game player (Java:
    /// `getRoomUser().getGamePlayer() == gamePlayer`).
    pub fn is_current(&self, session_id: u64, serial: u64) -> bool {
        self.current.get(&session_id) == Some(&serial)
    }

    /// Java: `getGamePlayer().getGame()`: the game the player's current game player's game id
    /// names (not always the game that game player belongs to: a spectator's id goes stale when
    /// the game restarts with a new id). Returns that game's index and, when the game player
    /// belongs to it, its index there.
    pub fn current_game(&self, session_id: u64) -> Option<(usize, Option<usize>)> {
        let (gi, pi) = self.current(session_id)?;
        let game_id = self.games[gi].players[pi].game_id;
        let target = self.game_by_id(game_id)?;
        Some((target, (target == gi).then_some(pi)))
    }

    /// Java: `RoomPlayer.getObservingGameId`.
    pub fn observing(&self, session_id: u64) -> i32 {
        self.observing.get(&session_id).copied().unwrap_or(-1)
    }

    /// Java: `RoomPlayer.startObservingGame`.
    pub fn start_observing(&mut self, session: &Arc<Session>, game_id: i32) {
        if self.observing(session.id) != -1 {
            self.stop_observing(session.id);
        }
        self.observing.insert(session.id, game_id);
        if let Some(gi) = self.game_by_id(game_id) {
            self.games[gi].observers.push(session.clone());
        }
    }

    /// Java: `RoomPlayer.stopObservingGame`.
    pub fn stop_observing(&mut self, session_id: u64) {
        let Some(game_id) = self.observing.remove(&session_id) else {
            return;
        };
        if let Some(gi) = self.game_by_id(game_id) {
            let observers = &mut self.games[gi].observers;
            if let Some(i) = observers.iter().position(|s| s.id == session_id) {
                observers.remove(i);
            }
        }
    }

    /// Java: `RoomPlayer.reset()`'s `observingGameId = -1` (without leaving the observers).
    pub fn forget_observing(&mut self, session_id: u64) {
        self.observing.remove(&session_id);
    }

    /// Java: `RoomPlayer.lastLobbyRedirection`.
    pub fn last_lobby(&self, session_id: u64) -> Option<GameType> {
        self.last_lobby.get(&session_id).copied()
    }

    pub fn set_last_lobby(&mut self, session_id: u64, game_type: GameType) {
        self.last_lobby.insert(session_id, game_type);
    }

    /// Java: `GameManager.getGamesByType`.
    pub fn games_of_type(&self, game_type: GameType) -> impl Iterator<Item = &Game> {
        self.games.iter().filter(move |g| g.game_type == game_type)
    }

    /// Java: `GameManager.getLastPlayedGames`.
    pub fn last_played(&self, game_type: GameType) -> &[GameHistory] {
        self.history.get(&game_type).map_or(&[], Vec::as_slice)
    }

    /// Java: `GameManager.getFinishedGameById`.
    pub fn finished_game(&self, game_type: GameType, id: i32) -> Option<&GameHistory> {
        self.last_played(game_type).iter().find(|g| g.id == id)
    }

    /// Java: `GameManager.getGames().remove(game)`; the game is kept until the operation's
    /// outbox is flushed.
    pub(crate) fn remove_game(&mut self, uid: u64) {
        if let Some(gi) = self.game_index(uid) {
            let mut game = self.games.remove(gi);
            game.cancel_timers();
            self.graveyard.push(game);
        }
    }

    /// Java: the end of `RoomEntityManager.leaveRoom`: a player who leaves a room while in a
    /// game (and not on the way to its arena) leaves the game.
    pub fn left_room(&mut self, session_id: u64, op: &mut Op) {
        let Some((gi, pi)) = self.current(session_id) else {
            return;
        };
        if self.games[gi].players[pi].entering_game {
            return;
        }
        match self.current_game(session_id) {
            Some((target, Some(pi))) => {
                let serial = self.games[target].players[pi].serial;
                self.leave_game(target, serial, op);
            }
            // TODO(games): Java's leaveGame on another game the stale id names.
            Some((_, None)) => {}
            None => self.set_current(session_id, None),
        }
    }

    /// Forgets a connection that logged out.
    pub fn forget_session(&mut self, session_id: u64) {
        self.current.remove(&session_id);
        self.observing.remove(&session_id);
        self.last_lobby.remove(&session_id);
    }
}

/// Java: `GameManager.checkXpExpiry`: at the start of a month (`xp.monthly.expiry` is not this
/// `yyyy-MM`), the setting becomes this month and everyone's monthly game scores and xp go back
/// to 0 in the store (the players online keep theirs in memory, as in Java).
pub fn check_xp_expiry(world: &World) {
    let month = chrono::Local::now().format("%Y-%m").to_string();
    if world
        .settings
        .read()
        .string("xp.monthly.expiry")
        .eq_ignore_ascii_case(&month)
    {
        return;
    }
    if let Err(e) = world.store.put_setting("xp.monthly.expiry", &month) {
        error!("saving xp.monthly.expiry: {e}");
    }
    world.settings.write().set("xp.monthly.expiry", &month);
    if let Err(e) = world.store.reset_monthly_scores() {
        error!("resetting the monthly game scores: {e}");
    }
}

/// The points Java's `GAMEPLAYERINFO` and ranks read (`battleball_score_all_time`,
/// `snowstorm_score_all_time`), kept on the session so rooms can read them under their lock.
pub fn points_of(session: &Session, game_type: GameType) -> i32 {
    session.game_points(game_type == GameType::SnowStorm)
}

/// The spawn of a game player as a `Position` (Java's `GameSpawn` keeps the rotation as both).
pub fn spawn_position(x: i32, y: i32, z: f64, rotation: i32) -> Position {
    let mut p = Position::new(x, y);
    p.z = z;
    p.set_rotation(rotation);
    p
}
