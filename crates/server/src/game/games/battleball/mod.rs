//! BattleBall (Java: `game/games/battleball`: `BattleBallGame`, `BattleBallMap`,
//! `BattleBallTile`, `BattleBallTask`, the objects and events, and `utils/FloodFill`,
//! `TileUtil`, `ScoreReference`). The power ups are in [`powerups`].
//!
//! The arena's tiles start grey; a player who jumps on one colours it in their team's colour
//! (touched, clicked, pressed, sealed), stealing another team's colour; sealing a tile fills the
//! tiles it closes off. Each colouring is worth points to the player and their team. The arena's
//! task (`BattleBallTask`, every 500 ms) moves the players a tile and sends `GAMESTATUS`.

pub mod powerups;

use std::collections::{HashMap, HashSet, VecDeque};

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

use super::game::{Game, GameKind};
use super::player::PlayerState;
use super::{GameState, Games, Op};
use crate::game::pathfinder::{Position, walk_direction};
use crate::game::room::Room;
use crate::game::room::entity::StatusType;
use crate::game::room::mapping::is_valid_tile;
use crate::game::room::model::RoomModel;
use crate::world::World;

pub use powerups::{PowerType, PowerUp};

/// Java: `BattleBallColourState`.
pub mod colour {
    pub const DISABLED: i32 = -2;
    pub const DEFAULT: i32 = -1;
}

/// Java: `BattleBallTileState`.
pub mod state {
    pub const DEFAULT: i32 = 0;
    pub const TOUCHED: i32 = 1;
    pub const CLICKED: i32 = 2;
    pub const PRESSED: i32 = 3;
    pub const SEALED: i32 = 4;
}

/// Java: `BattleBallGame.MAX_POWERS_ACTIVE`.
pub const MAX_POWERS_ACTIVE: usize = 2;

/// Java: `BattleBallMap`: which tiles of an arena take part in the game.
pub struct TileMap {
    pub map_id: i32,
    size_y: usize,
    /// Indexed `x * size_y + y`.
    tiles: Vec<bool>,
    size_x: usize,
}

impl TileMap {
    /// Java: `BattleBallMap.parse`.
    pub fn parse(map_id: i32, tile_map: &str) -> Self {
        // Java: replace("|", "\r").split("\r") drops the empty rows at the end.
        let lines: Vec<&str> = tile_map.trim_end_matches('|').split('|').collect();
        let size_y = lines.len();
        let size_x = lines.first().map_or(0, |l| l.len());
        let mut tiles = vec![false; size_x * size_y];
        for (y, line) in lines.iter().enumerate() {
            let bytes = line.as_bytes();
            for x in 0..size_x {
                let mut game_tile = bytes.get(x) == Some(&b'1');
                // Java: "Temporary fix for the two tiles on Sky Peak".
                if map_id == 1 && x == 24 && (y == 17 || y == 18) {
                    game_tile = true;
                }
                tiles[x * size_y + y] = game_tile;
            }
        }
        Self {
            map_id,
            size_y,
            tiles,
            size_x,
        }
    }

    /// Java: `isGameTile` (which throws outside the map; outside is no game tile here).
    pub fn is_game_tile(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x as usize >= self.size_x || y as usize >= self.size_y {
            return false;
        }
        self.tiles[x as usize * self.size_y + y as usize]
    }
}

impl super::GameData {
    /// Java: `GameManager.getBattleballTileMap`.
    pub fn battleball_map(&self, map_id: i32) -> Option<&TileMap> {
        self.battleball_maps.iter().find(|m| m.map_id == map_id)
    }
}

/// Java: `ScoreReference`: points a tile gives a team, scored by a user.
#[derive(Clone, Debug)]
pub struct ScoreRef {
    pub score: i32,
    pub team: i32,
    pub by: u32,
}

/// Java: `BattleBallTile`.
#[derive(Clone, Debug)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub z: f64,
    pub colour: i32,
    pub state: i32,
    pub spawn_occupied: bool,
    pub points: Vec<ScoreRef>,
}

/// An object of the game (Java: `GameObject` subclasses in `objects`): a player (`PlayerObject`),
/// a power up on the floor (`PowerObject`) or a pin of the box of pins (`PinObject`).
#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    Player(u64),
    Power(i32),
    Pin { id: i32, x: i32, y: i32, z: f64 },
}

/// An update object (Java: `PlayerUpdateObject`, `PowerUpUpdateObject`).
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// By serial; its id is the player's user id (Java).
    Player(u64, u32),
    Power(i32),
}

impl Update {
    /// Java: `GameObject.getId`, what `addObjectToQueue` replaces by.
    fn id(&self) -> i64 {
        match self {
            Self::Player(_, user_id) => i64::from(*user_id),
            Self::Power(id) => i64::from(*id),
        }
    }
}

/// Java: `GameEvent` subclasses.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `PlayerMoveEvent`: the player's instance id and the tile it moves to.
    PlayerMove { serial: u64, x: i32, y: i32 },
    /// `PowerUpSpawnEvent`.
    PowerSpawn(i32),
    /// `PinSpawnEvent`.
    PinSpawn { id: i32, x: i32, y: i32, z: f64 },
    /// `DespawnObjectEvent`.
    Despawn(i32),
    /// `AcquirePowerUpEvent`, with the power's type.
    Acquire { serial: u64, power: i32, ty: i32 },
    /// `ActivatePowerUpEvent`, with the power's type.
    Activate { serial: u64, power: i32, ty: i32 },
}

/// Java: `BattleBallGame`'s state.
pub struct BattleBall {
    size_x: i32,
    size_y: i32,
    /// Java: `battleballTiles`, indexed `x * size_y + y`.
    pub tiles: Vec<Tile>,
    /// Java: `tiles`: the indexes of the game's tiles, row by row.
    pub game_tiles: Vec<usize>,
    pub update_tiles_queue: Vec<usize>,
    pub fill_tiles_queue: Vec<usize>,
    pub allowed_power_ups: Vec<i32>,
    pub active_powers: Vec<PowerUp>,
    /// Java: `storedPowers` (a map by game player; a list per player), by serial.
    pub stored_powers: Vec<(u64, Vec<PowerUp>)>,
    spawned_initial_powers: bool,
    /// Java: `Game.objects`.
    pub objects: Vec<Object>,
    /// Java: `Game.eventsQueue`.
    pub events_queue: Vec<Event>,
    /// Java: `Game.objectsQueue`.
    pub objects_queue: Vec<Update>,
}

impl BattleBall {
    /// Java: `new BattleBallGame(...)`: with two or more power ups allowed, the question mark
    /// comes too.
    pub fn new(mut allowed_power_ups: Vec<i32>) -> Self {
        if allowed_power_ups.len() >= 2 {
            allowed_power_ups.push(PowerType::QuestionMark as i32);
        }
        Self {
            size_x: 0,
            size_y: 0,
            tiles: Vec::new(),
            game_tiles: Vec::new(),
            update_tiles_queue: Vec::new(),
            fill_tiles_queue: Vec::new(),
            allowed_power_ups,
            active_powers: Vec::new(),
            stored_powers: Vec::new(),
            spawned_initial_powers: false,
            objects: Vec::new(),
            events_queue: Vec::new(),
            objects_queue: Vec::new(),
        }
    }

    /// Java: `Game.getTile`.
    pub fn tile_index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.size_x || y >= self.size_y {
            return None;
        }
        Some((x * self.size_y + y) as usize)
    }

    /// Java: `buildMap` (with `Game.initialise`'s clearing of the objects and events).
    pub fn build_map(&mut self, model: &RoomModel, map: Option<&TileMap>) {
        self.objects.clear();
        self.size_x = model.size_x;
        self.size_y = model.size_y;
        self.tiles.clear();
        self.game_tiles.clear();
        for x in 0..model.size_x {
            for y in 0..model.size_y {
                self.tiles.push(Tile {
                    x,
                    y,
                    z: model.tile_height(x, y),
                    colour: colour::DISABLED,
                    state: state::DEFAULT,
                    spawn_occupied: false,
                    points: Vec::new(),
                });
            }
        }
        for y in 0..model.size_y {
            for x in 0..model.size_x {
                let i = (x * model.size_y + y) as usize;
                if !model.is_open(x, y) || !map.is_some_and(|m| m.is_game_tile(x, y)) {
                    continue;
                }
                self.tiles[i].colour = colour::DEFAULT;
                self.game_tiles.push(i);
            }
        }
    }

    pub fn clear_queues(&mut self) {
        self.update_tiles_queue.clear();
        self.fill_tiles_queue.clear();
        self.events_queue.clear();
        self.objects_queue.clear();
    }

    /// Java: `BattleBallGame.gamePrepare`: the old powers go.
    pub fn game_prepare(&mut self) {
        for p in &self.active_powers {
            self.events_queue.push(Event::Despawn(p.id));
        }
        self.spawned_initial_powers = false;
        self.stored_powers.clear();
        self.active_powers.clear();
    }

    /// Java: `gamePrepareTick`: up to two power ups on the first tick.
    pub fn game_prepare_tick(&mut self, map_id: i32, object_id: &mut i32) {
        if !self.spawned_initial_powers {
            let initial = powerups::random(0, MAX_POWERS_ACTIVE as i32 + 1);
            for _ in 0..initial {
                self.check_spawn_power(false, map_id, object_id, None);
            }
            self.spawned_initial_powers = true;
        }
    }

    /// Java: `gameTick`.
    pub fn game_tick(&mut self, map_id: i32, object_id: &mut i32, room: Option<&Room>) {
        self.check_expire_power(map_id);
        self.check_spawn_power(true, map_id, object_id, room);
        self.check_stored_expire_power();
    }

    /// Java: `checkSpawnPower`.
    fn check_spawn_power(
        &mut self,
        do_percent_check: bool,
        map_id: i32,
        object_id: &mut i32,
        room: Option<&Room>,
    ) {
        if self.allowed_power_ups.is_empty() || map_id == 5 {
            return;
        }
        if self.active_powers.len() >= MAX_POWERS_ACTIVE {
            return;
        }
        // Java: `!(Math.random() < 0.06)`.
        if do_percent_check && powerups::random_f64() >= 0.06 {
            return;
        }
        let Some(tile) = self.random_tile(room) else {
            return;
        };
        *object_id += 1;
        let power = PowerUp::new(*object_id, self, tile);
        self.events_queue.push(Event::PowerSpawn(power.id));
        self.objects.push(Object::Power(power.id));
        self.active_powers.push(power);
    }

    /// Java: `checkExpirePower`.
    fn check_expire_power(&mut self, map_id: i32) {
        if self.allowed_power_ups.is_empty() || map_id == 5 {
            return;
        }
        let mut i = 0;
        while i < self.active_powers.len() {
            if expire_power(&mut self.active_powers[i], &mut self.events_queue) {
                let id = self.active_powers.remove(i).id;
                self.objects.retain(|o| *o != Object::Power(id));
            } else {
                i += 1;
            }
        }
    }

    /// Java: `checkStoredExpirePower`.
    fn check_stored_expire_power(&mut self) {
        for (_, powers) in &mut self.stored_powers {
            powers.retain_mut(|p| !expire_power(p, &mut self.events_queue));
        }
    }

    /// Java: `getRandomTile`: a game tile nobody stands on and without a power up.
    fn random_tile(&self, room: Option<&Room>) -> Option<usize> {
        let free = |i: &usize| {
            let t = &self.tiles[*i];
            let empty = room.is_none_or(|r| {
                r.mapping
                    .tile(Position::new(t.x, t.y))
                    .is_none_or(|rt| rt.entities.is_empty())
            });
            empty && !self.active_powers.iter().any(|p| p.x == t.x && p.y == t.y)
        };
        // Java picks coordinates at random until one fits; picking among the fitting ones gives
        // the same distribution without the unbounded retries.
        let candidates: Vec<usize> = self.game_tiles.iter().copied().filter(free).collect();
        if candidates.is_empty() {
            return None;
        }
        Some(candidates[powerups::random(0, candidates.len() as i32) as usize])
    }

    /// Java: `canTimerContinue`: a game tile that is not sealed.
    pub fn can_timer_continue(&self) -> bool {
        self.tiles
            .iter()
            .any(|t| t.colour != colour::DISABLED && t.state != state::SEALED)
    }

    pub fn remove_player_object(&mut self, serial: u64) {
        self.objects.retain(|o| *o != Object::Player(serial));
    }

    /// Java: `GameTeam.calculateScore`.
    pub fn team_points(&self, team: i32) -> i32 {
        self.game_tiles
            .iter()
            .flat_map(|&i| &self.tiles[i].points)
            .filter(|r| r.team == team)
            .map(|r| r.score)
            .sum()
    }

    /// Java: `GamePlayer.calculateScore`.
    pub fn player_score(&self, user_id: u32) -> i32 {
        self.game_tiles
            .iter()
            .flat_map(|&i| &self.tiles[i].points)
            .filter(|r| r.by == user_id)
            .map(|r| r.score)
            .sum()
    }

    /// Java: `addObjectToQueue`.
    pub fn queue_object(&mut self, update: Update) {
        let id = update.id();
        self.objects_queue.retain(|o| o.id() != id);
        self.objects_queue.push(update);
    }

    /// Java: `addPlayerMove`.
    pub fn queue_player_move(&mut self, serial: u64, x: i32, y: i32) {
        self.events_queue
            .retain(|e| !matches!(e, Event::PlayerMove { serial: s, .. } if *s == serial));
        self.events_queue.push(Event::PlayerMove { serial, x, y });
    }

    pub fn power(&self, id: i32) -> Option<&PowerUp> {
        self.active_powers.iter().find(|p| p.id == id).or_else(|| {
            self.stored_powers
                .iter()
                .flat_map(|(_, list)| list)
                .find(|p| p.id == id)
        })
    }

    pub fn stored_of(&self, serial: u64) -> Option<&Vec<PowerUp>> {
        self.stored_powers
            .iter()
            .find(|(s, _)| *s == serial)
            .map(|(_, l)| l)
    }

    fn stored_of_mut(&mut self, serial: u64) -> &mut Vec<PowerUp> {
        if let Some(i) = self.stored_powers.iter().position(|(s, _)| *s == serial) {
            return &mut self.stored_powers[i].1;
        }
        self.stored_powers.push((serial, Vec::new()));
        &mut self.stored_powers.last_mut().expect("pushed").1
    }

    /// Java: `FULLGAMESTATUS` for BattleBall.
    pub fn full_game_status(
        &self,
        game: &Game,
        world: &World,
        arena: Option<&Room>,
        positions: &HashMap<u64, Position>,
    ) -> Message {
        let view = View {
            game,
            bb: self,
            arena,
            positions,
        };
        let mut body = ServerBody::new()
            .int(GameState::Started as i32)
            .int(game.preparing_seconds_left)
            .int(super::game::preparing_seconds(world, game.game_type))
            .int(self.objects.len() as i32);
        for o in &self.objects {
            body = view.full_object(body, o);
        }
        body = body.int(self.size_y).int(self.size_x);
        for y in 0..self.size_y {
            for x in 0..self.size_x {
                body = match self.tile_index(x, y) {
                    Some(i) => body.int(self.tiles[i].colour).int(self.tiles[i].state),
                    None => body.int(-1).int(0),
                };
            }
        }
        Message::new(243, body.int(1).int(0).build())
    }
}

/// Java: `expirePower`.
fn expire_power(power: &mut PowerUp, events: &mut Vec<Event>) -> bool {
    if power.time_to_despawn > 0 {
        power.time_to_despawn -= 1;
        if power.time_to_despawn != 0 {
            return false;
        }
    }
    events.push(Event::Despawn(power.id));
    true
}

/// What serialising the objects reads: the players' positions in the arena (or, for those not
/// in it, from `positions`).
struct View<'a> {
    game: &'a Game,
    bb: &'a BattleBall,
    arena: Option<&'a Room>,
    positions: &'a HashMap<u64, Position>,
}

impl View<'_> {
    fn position(&self, serial: u64) -> Position {
        let Some(gp) = self.game.player(serial) else {
            return Position::default();
        };
        if let Some(room) = self.arena {
            if let Some(idx) = room.entity_index(gp.session.id) {
                return room.entities[idx].position;
            }
        }
        self.positions.get(&serial).copied().unwrap_or_default()
    }

    fn instance_id(&self, serial: u64) -> i32 {
        self.game
            .player(serial)
            .map_or(-1, |gp| gp.session.instance_id())
    }

    fn object_id(&self, serial: u64) -> i32 {
        self.game.player(serial).map_or(-1, |p| p.object_id)
    }

    /// Java: `getColouringForOpponentId`.
    fn colouring_for(&self, serial: u64) -> i32 {
        self.game
            .player(serial)
            .and_then(|p| p.harlequin)
            .map_or(-1, |h| self.object_id(h))
    }

    /// `PlayerUpdateObject.serialiseObject`.
    fn player_update(&self, body: ServerBody, serial: u64) -> ServerBody {
        let p = self.position(serial);
        body.int(self.object_id(serial))
            .int(p.x)
            .int(p.y)
            .int(p.z as i32)
            .int(p.rotation())
            .int(self.game.player(serial).map_or(0, |gp| gp.state_id()))
            .int(self.colouring_for(serial))
    }

    /// `PowerUpUpdateObject.serialiseObject`.
    fn power_update(&self, body: ServerBody, id: i32) -> ServerBody {
        match self.bb.power(id) {
            Some(power) => body
                .int(power.id)
                .int(power.time_to_despawn)
                .int(power.holder.map_or(-1, |h| self.object_id(h))),
            None => body.int(id).int(0).int(-1),
        }
    }

    /// `PowerObject.serialiseObject`.
    fn power_object(&self, body: ServerBody, id: i32) -> ServerBody {
        match self.bb.power(id) {
            Some(power) => body
                .int(power.id)
                .int(power.time_to_despawn)
                .int(power.holder.map_or(-1, |h| self.object_id(h)))
                .int(power.power_type as i32)
                .int(power.x)
                .int(power.y)
                .int(power.z as i32),
            None => body.int(id).int(0).int(-1).int(0).int(0).int(0).int(0),
        }
    }

    /// An object of `objects` with its type, as `FULLGAMESTATUS` writes it.
    fn full_object(&self, body: ServerBody, object: &Object) -> ServerBody {
        match object {
            Object::Player(serial) => {
                let Some(gp) = self.game.player(*serial) else {
                    return body;
                };
                let body = self.player_update(body.int(0), *serial);
                body.text(&gp.name)
                    .text(&gp.motto)
                    .text(&gp.figure)
                    .text(&gp.sex)
                    .int(gp.team_id)
                    .int(gp.object_id)
            }
            Object::Power(id) => self.power_object(body.int(1), *id),
            Object::Pin { id, x, y, z } => body.int(2).int(*id).int(*x).int(*y).int(*z as i32),
        }
    }

    fn update(&self, body: ServerBody, update: &Update) -> ServerBody {
        match update {
            Update::Player(serial, _) => self.player_update(body.int(0), *serial),
            Update::Power(id) => self.power_update(body.int(1), *id),
        }
    }

    fn event(&self, body: ServerBody, event: &Event) -> ServerBody {
        match event {
            Event::PlayerMove { serial, x, y } => {
                body.int(2).int(self.instance_id(*serial)).int(*x).int(*y)
            }
            Event::PowerSpawn(id) => self.power_object(body.int(0).int(1), *id),
            Event::PinSpawn { id, x, y, z } => {
                body.int(0).int(2).int(*id).int(*x).int(*y).int(*z as i32)
            }
            Event::Despawn(id) => body.int(1).int(*id),
            Event::Acquire { serial, power, ty } => body
                .int(3)
                .int(self.object_id(*serial))
                .int(*power)
                .int(*ty),
            Event::Activate { serial, power, ty } => {
                let rotation = self.position(*serial).rotation();
                body.int(5)
                    .int(self.object_id(*serial))
                    .int(*power)
                    .int(rotation)
                    .int(*ty)
            }
        }
    }
}

/// Java: `BattleBallGame.assignSpawnPoints`.
pub fn assign_spawn_points(game: &mut Game, world: &World) {
    let map_id = game.map_id;
    let model = game.model.clone();
    for t in 0..game.teams.len() {
        let team_id = game.teams[t].id;
        let spawns: Vec<(i32, i32, i32)> = world
            .games
            .data
            .spawns(game.game_type, map_id, team_id)
            .iter()
            .map(|s| (s.x, s.y, s.rotation))
            .collect();
        for serial in game.teams[t].players.clone() {
            let GameKind::BattleBall(bb) = &mut game.kind else {
                return;
            };
            let mut spawn = None;
            for &(x, y, rotation) in &spawns {
                // Java indexes the tile array directly (and throws outside it).
                let Some(i) = bb.tile_index(x, y) else {
                    continue;
                };
                if !bb.tiles[i].spawn_occupied {
                    let z = model.as_ref().map_or(0.0, |m| m.tile_height(x, y));
                    spawn = Some(super::spawn_position(x, y, z, rotation));
                    break;
                }
            }
            let Some(spawn) = spawn else {
                continue;
            };
            let Some(tile) = bb.tile_index(spawn.x, spawn.y) else {
                continue;
            };
            // Java: `getObjects().add(new PlayerObject(p))`.
            bb.objects.push(Object::Player(serial));
            bb.tiles[tile].spawn_occupied = true;
            if bb.tiles[tile].colour != colour::DISABLED {
                bb.tiles[tile].colour = team_id;
                bb.tiles[tile].state = if map_id == 5 {
                    state::TOUCHED
                } else {
                    state::CLICKED
                };
            }
            let needs_id = game.player(serial).is_some_and(|p| p.object_id == -1);
            let object_id = if needs_id {
                game.create_object_id()
            } else {
                game.player(serial).map_or(-1, |p| p.object_id)
            };
            // TODO(games): Java also stops the player's walk wherever they are
            // (`setWalking(false)`, `setNextPosition(null)`); a restart does it in the arena
            // (`Room::set_game_position`), a first start leaves the lobby walk alone.
            let gp = game.player_mut(serial).expect("team member");
            gp.player_state = Some(PlayerState::Normal);
            gp.harlequin = None;
            gp.object_id = object_id;
            gp.spawn = spawn;
        }
    }
}

/// Java: `GAMERESET` for BattleBall: the players' new positions.
pub fn game_reset(game: &Game, world: &World, players: &[u64], arena: Option<&Room>) -> Message {
    let mut body = ServerBody::new()
        .int(super::game::preparing_seconds(world, game.game_type))
        .int(players.len() as i32);
    for &serial in players {
        let Some(gp) = game.player(serial) else {
            continue;
        };
        let position = arena
            .and_then(|r| {
                r.entity_index(gp.session.id)
                    .map(|i| r.entities[i].position)
            })
            .unwrap_or(gp.spawn);
        body = body
            .int(gp.object_id)
            .int(position.x)
            .int(position.y)
            .int(position.rotation());
    }
    Message::new(249, body.build())
}

/// The tiles and messages a tick collects (Java: `BattleBallTask.run`'s lists).
#[derive(Default)]
pub(crate) struct Tick {
    pub objects: Vec<Update>,
    pub events: Vec<Event>,
    pub updates: Vec<usize>,
    pub fills: Vec<usize>,
}

impl Games {
    /// Java: `BattleBallTask.run` for the arena `room` of the game `uid`.
    pub fn arena_tick(&mut self, uid: u64, room: &mut Room, op: &mut Op) {
        let Some(gi) = self.game_index(uid) else {
            return;
        };
        let game = &mut self.games[gi];
        if game.active_players().next().is_none() || game.state == GameState::Ended {
            return;
        }
        let GameKind::BattleBall(bb) = &mut game.kind else {
            // TODO(snowstorm): SnowStormGameTask.
            return;
        };
        let mut tick = Tick {
            events: std::mem::take(&mut bb.events_queue),
            objects: std::mem::take(&mut bb.objects_queue),
            updates: std::mem::take(&mut bb.update_tiles_queue),
            fills: std::mem::take(&mut bb.fill_tiles_queue),
        };
        game.calculate_team_points();
        let world = op.world;
        for serial in game.active_serials() {
            let session_id = game.player(serial).map(|p| p.session.id).unwrap_or(0);
            let Some(idx) = room.entity_index(session_id) else {
                continue;
            };
            if matches!(
                game.player_state(serial),
                PlayerState::ClimbingIntoCannon | PlayerState::FlyingThroughAir
            ) {
                continue;
            }
            if let GameKind::BattleBall(bb) = &game.kind {
                if let Some(powers) = bb.stored_of(serial) {
                    for p in powers {
                        tick.objects.push(Update::Power(p.id));
                    }
                }
            }
            room.entities[idx].handle_spam_ticks();
            process_entity(world, game, serial, room, &mut tick);
            let user_id = game.player(serial).map_or(0, |p| p.user_id);
            tick.objects.push(Update::Player(serial, user_id));
        }
        let msg = game_status(game, room, &tick);
        game.send(op, msg);
    }
}

/// Java: `GAMESTATUS` for BattleBall.
fn game_status(game: &Game, room: &Room, tick: &Tick) -> Message {
    let GameKind::BattleBall(bb) = &game.kind else {
        return Message::new(244, Vec::new());
    };
    let empty = HashMap::new();
    let view = View {
        game,
        bb,
        arena: Some(room),
        positions: &empty,
    };
    let mut body = ServerBody::new().int(tick.objects.len() as i32);
    for o in &tick.objects {
        body = view.update(body, o);
    }
    for list in [&tick.updates, &tick.fills] {
        body = body.int(list.len() as i32);
        for &i in list {
            let t = &bb.tiles[i];
            body = body.int(t.x).int(t.y).int(t.colour).int(t.state);
        }
    }
    body = body.int(game.teams.len() as i32);
    for team in &game.teams {
        body = body.int(team.points);
    }
    body = body.int(1).int(tick.events.len() as i32);
    for e in &tick.events {
        body = view.event(body, e);
    }
    Message::new(244, body.build())
}

/// Java: `BattleBallTask.processEntity`.
fn process_entity(world: &World, game: &mut Game, serial: u64, room: &mut Room, tick: &mut Tick) {
    let Some(session_id) = game.player(serial).map(|p| p.session.id) else {
        return;
    };
    let Some(idx) = room.entity_index(session_id) else {
        return;
    };
    if !room.entities[idx].is_walking {
        return;
    }
    let goal = room.entities[idx].goal;
    let instance_id = room.entities[idx].instance_id;
    if let Some(next) = room.entities[idx].next_position {
        let e = &mut room.entities[idx];
        e.position.x = next.x;
        e.position.y = next.y;
        room.update_new_height(idx);
        let alone = room
            .mapping
            .tile(next)
            .is_some_and(|t| t.entities.iter().all(|&i| i == instance_id));
        if alone {
            interact(world, game, serial, next.x, next.y, room, tick);
        }
    }
    // The tile's trigger may have stunned the player (stopping the walk).
    let Some(idx) = room.entity_index(session_id) else {
        return;
    };
    let position = room.entities[idx].position;
    if let Some(next) = room.entities[idx].path.pop_front() {
        let walker = room.walker(idx);
        let tile_exists = room.mapping.tile(next).is_some();
        if !tile_exists
            || (!is_valid_tile(&room.mapping, Some(&walker), next) && Some(next) != goal)
        {
            room.entities[idx].path.clear();
            if let Some(goal) = goal {
                room.walk_to_player(world, idx, goal.x, goal.y);
            }
            process_entity(world, game, serial, room, tick);
            return;
        }
        room.mapping.remove_entity(position, instance_id);
        room.mapping.add_entity(next, instance_id);
        let height = room
            .mapping
            .tile(next)
            .map_or(0.0, |t| room.mapping.walking_height(t));
        let e = &mut room.entities[idx];
        e.statuses.remove(StatusType::Lay);
        e.statuses.remove(StatusType::Sit);
        e.position
            .set_rotation(walk_direction(position.x, position.y, next.x, next.y));
        e.statuses.set(
            StatusType::Move,
            format!(
                "{},{},{}",
                next.x,
                next.y,
                crate::game::format_decimal(height)
            ),
        );
        e.next_position = Some(next);
        tick.events.push(Event::PlayerMove {
            serial,
            x: next.x,
            y: next.y,
        });
    } else {
        room.stop_walking_entity(world, idx);
        let Some(idx) = room.entity_index(session_id) else {
            return;
        };
        let position = room.entities[idx].position;
        let behind = position.square_behind();
        let crowded = room
            .mapping
            .tile(position)
            .is_some_and(|t| t.entities.iter().any(|&i| i != instance_id));
        if crowded && room.mapping.tile(behind).is_some() {
            room.mapping.remove_entity(position, instance_id);
            room.mapping.add_entity(behind, instance_id);
            room.entities[idx].position = behind;
        }
    }
    if let Some(idx) = room.entity_index(session_id) {
        room.entities[idx].needs_update = true;
    }
}

/// Java: `BattleBallTile.interact`.
fn interact(
    world: &World,
    game: &mut Game,
    serial: u64,
    x: i32,
    y: i32,
    room: &mut Room,
    tick: &mut Tick,
) {
    let Some(i) = bb_of(game).tile_index(x, y) else {
        return;
    };
    powerups::check_power_up(game, i, serial, tick);
    if powerups::check_nail_tile(world, game, serial, room) {
        return;
    }
    if powerups::has_used_power(game, i, serial, &mut tick.updates, &mut tick.fills) {
        return;
    }
    change_state(game, i, serial, &mut tick.updates, &mut tick.fills);
}

pub(crate) fn bb_of(game: &Game) -> &BattleBall {
    match &game.kind {
        GameKind::BattleBall(bb) => bb,
        GameKind::SnowStorm(_) => unreachable!("a BattleBall game"),
    }
}

pub(crate) fn bb_of_mut(game: &mut Game) -> &mut BattleBall {
    match &mut game.kind {
        GameKind::BattleBall(bb) => bb,
        GameKind::SnowStorm(_) => unreachable!("a BattleBall game"),
    }
}

/// Java: `GamePlayer.getTeam().getId()`: the team the player colours for (the harlequin's when
/// one is on).
pub(crate) fn colour_team(game: &Game, serial: u64) -> i32 {
    let Some(gp) = game.player(serial) else {
        return -1;
    };
    match gp.harlequin.and_then(|h| game.player(h)) {
        Some(h) => h.team_id,
        None => gp.team_id,
    }
}

/// The user the points are scored by (Java: the harlequin's when one is on).
fn scorer(game: &Game, serial: u64) -> u32 {
    let Some(gp) = game.player(serial) else {
        return 0;
    };
    match gp.harlequin.and_then(|h| game.player(h)) {
        Some(h) => h.user_id,
        None => gp.user_id,
    }
}

/// Java: `BattleBallTile.changeState`.
fn change_state(
    game: &mut Game,
    i: usize,
    serial: u64,
    updates: &mut Vec<usize>,
    fills: &mut Vec<usize>,
) {
    let team = colour_team(game, serial);
    let map_id = game.map_id;
    let tile = &bb_of(game).tiles[i];
    if tile.colour == colour::DISABLED || tile.state == state::SEALED {
        return;
    }
    let (new_state, new_colour) = if tile.colour == team {
        (tile.state + 1, tile.colour)
    } else if map_id == 5 {
        // Barebones classic takes four hits.
        (state::TOUCHED, team)
    } else {
        (state::CLICKED, team)
    };
    new_points(game, i, serial, new_state, new_colour);
    let tile = &mut bb_of_mut(game).tiles[i];
    tile.colour = new_colour;
    tile.state = new_state;
    if new_state == state::SEALED {
        check_fill(game, i, serial, fills);
    }
    updates.push(i);
}

/// Java: `BattleBallTile.getNewPoints`.
pub(crate) fn new_points(game: &mut Game, i: usize, serial: u64, new_state: i32, new_colour: i32) {
    let team = colour_team(game, serial);
    let by = scorer(game, serial);
    let tile = &mut bb_of_mut(game).tiles[i];
    let mut points = match new_state {
        state::TOUCHED => 2,
        state::CLICKED => 6,
        state::PRESSED => 10,
        state::SEALED if tile.colour == new_colour => 14,
        _ => -1,
    };
    // Stealing the tile is worth more.
    if tile.colour != new_colour && tile.colour != colour::DEFAULT {
        points += 2;
    }
    if tile.colour != new_colour {
        tile.points.retain(|r| r.team == team);
    }
    if points != -1 {
        tile.points.push(ScoreRef {
            score: points,
            team,
            by,
        });
    }
}

/// Java: `BattleBallTile.checkFill`: the tiles a sealed tile closes off are sealed too.
pub(crate) fn check_fill(game: &mut Game, i: usize, serial: u64, fills: &mut Vec<usize>) {
    // Java: FloodFill uses the player's own team id.
    let team = game.player(serial).map_or(-1, |p| p.team_id);
    let bb = bb_of_mut(game);
    let (x, y, colour) = (bb.tiles[i].x, bb.tiles[i].y, bb.tiles[i].colour);
    for (nx, ny) in neighbours(x, y) {
        let Some(n) = bb.tile_index(nx, ny) else {
            continue;
        };
        if bb.tiles[n].state == state::SEALED || bb.tiles[n].colour == colour::DISABLED {
            continue;
        }
        let fill = flood_fill(bb, team, n);
        for f in fill {
            if bb.tiles[f].state == state::SEALED {
                continue;
            }
            bb.tiles[f].colour = colour;
            bb.tiles[f].state = state::SEALED;
            fills.push(f);
        }
    }
}

/// Java: `Pathfinder.MOVE_POINTS` around a tile.
fn neighbours(x: i32, y: i32) -> [(i32, i32); 4] {
    [(x, y - 1), (x + 1, y), (x, y + 1), (x - 1, y)]
}

/// Java: `FloodFill.getFill`: the area around `start` that is not the team's sealed tiles, if it
/// is closed (it touches neither the map's edge nor a disabled tile); empty otherwise. Java
/// returns a `HashSet`, so the order the tiles are filled in is not defined there; here it is
/// the order they are found in.
fn flood_fill(bb: &BattleBall, team: i32, start: usize) -> Vec<usize> {
    let mut closed: Vec<usize> = Vec::new();
    let mut closed_set: HashSet<usize> = HashSet::new();
    let mut stack: VecDeque<usize> = VecDeque::from([start]);
    while let Some(tile) = stack.pop_back() {
        let (x, y) = (bb.tiles[tile].x, bb.tiles[tile].y);
        for (nx, ny) in neighbours(x, y) {
            let Some(n) = bb.tile_index(nx, ny) else {
                return Vec::new();
            };
            let t = &bb.tiles[n];
            if t.colour == colour::DISABLED {
                return Vec::new();
            }
            if (t.colour != team || t.state != state::SEALED)
                && !closed_set.contains(&n)
                && !stack.contains(&n)
            {
                stack.push_front(n);
            }
        }
        if closed_set.insert(tile) {
            closed.push(tile);
        }
    }
    closed
}

/// Java: `TileUtil.undoTileAttributes`.
pub(crate) fn undo_tile(tile: &mut Tile) -> bool {
    if tile.colour == colour::DEFAULT || tile.state == state::DEFAULT {
        return false;
    }
    tile.points.clear();
    tile.colour = colour::DEFAULT;
    tile.state = state::DEFAULT;
    true
}

#[cfg(test)]
mod tests;
