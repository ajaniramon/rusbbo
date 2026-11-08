//! The engine (Java: `SnowStormGame`, the SnowStorm half of `GamePlayer` and the per-arena
//! `SnowStormGameTask` state).

use std::collections::BTreeMap;
use std::sync::Arc;

use havana_protocol::Message;
use tracing::error;

use super::attributes::{ActivityState, SnowStormAttributes};
use super::events::TurnEvent;
use super::mapping::SnowStormMap;
use super::maps::SnowStormMaps;
use super::objects::{ObjectRef, SnowStormMachine, Snowball};
use super::tasks::{Flight, TurnTask};
use super::timers::{ResetAction, TimerKind, Timers};
use super::{SPAWN_FALLBACK, java_distance};
use crate::game::pathfinder::{Position, walk_direction};

/// Who a player is, as the core's `GamePlayer`/`Player` knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerInfo {
    pub user_id: u32,
    /// 0-based, below the game's team amount.
    pub team_id: i32,
    pub name: String,
    pub motto: String,
    pub figure: String,
    pub sex: String,
}

/// One player of the game (Java: a `GamePlayer` of one of the game's teams).
#[derive(Clone, Debug)]
pub struct SnowStormPlayer {
    pub info: PlayerInfo,
    /// Java: `GamePlayer.isInGame` (`getActivePlayers` is the players with it set).
    pub in_game: bool,
    /// Whether the player's room is the arena (the turn task only moves those).
    pub in_room: bool,
    /// Java: `GamePlayer.getObjectId` (assigned by `assignSpawnPoints`; -1 before).
    pub object_id: i32,
    /// Java: `GamePlayer.getSpawnPosition`.
    pub spawn: Position,
    pub assigned_spawn: bool,
    pub attrs: SnowStormAttributes,
}

/// What `assignSpawnPoints` decided for one player; the core copies it onto its `GamePlayer`
/// (object id, score 0) and, on a restart, onto the room user (instance id = object id,
/// position = spawn).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnAssignment {
    pub user_id: u32,
    pub object_id: i32,
    pub spawn: Position,
    pub rotation: i32,
}

/// What the engine asks the core to do, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    /// Java: `player.getRoomUser().handleSpamTicks()`, once per turn for every game player in
    /// the arena room (before the player is moved).
    SpamTick(u32),
    /// Send to every player in the arena room (Java:
    /// `room.getEntityManager().getPlayers()`, spectators included).
    Broadcast(Message),
}

/// Java: `SnowStormGame` (plus the turn task the arena room runs for it).
#[derive(Debug)]
pub struct SnowStormGame {
    pub map_id: i32,
    pub team_amount: i32,
    /// Java: `gameLengthChoice` (1: 2 minutes, 2: 3 minutes, 3: 5 minutes).
    pub length_choice: i32,
    pub(super) map: Option<Arc<SnowStormMap>>,
    /// In Java's iteration order: by team, then in the order they joined the team.
    pub(super) players: Vec<SnowStormPlayer>,
    pub(super) objects: Vec<ObjectRef>,
    /// Every machine ever created; `objects` refers to the current game's.
    pub(super) machines: Vec<SnowStormMachine>,
    pub(super) flights: BTreeMap<i32, Flight>,
    /// Java: `Game.objectId` (machines, avatars and snowballs share it; never reset).
    object_counter: i32,
    /// Java: the room's `UpdateTask`, `None` while the arena room has no tasks.
    pub(super) turns: Option<TurnTask>,
    pub(super) turn_generation: u64,
    pub(super) timers: Timers,
}

impl SnowStormGame {
    /// Java: `new SnowStormGame(...)`. The map is looked up once (see [`super::maps::reload`]).
    pub fn new(map_id: i32, team_amount: i32, length_choice: i32, maps: &SnowStormMaps) -> Self {
        Self {
            map_id,
            team_amount,
            length_choice,
            map: maps.get(map_id),
            players: Vec::new(),
            objects: Vec::new(),
            machines: Vec::new(),
            flights: BTreeMap::new(),
            object_counter: 0,
            turns: None,
            turn_generation: 0,
            timers: Timers::default(),
        }
    }

    pub fn map(&self) -> Option<&SnowStormMap> {
        self.map.as_deref()
    }

    pub fn players(&self) -> &[SnowStormPlayer] {
        &self.players
    }

    pub fn player(&self, user_id: u32) -> Option<&SnowStormPlayer> {
        self.players.iter().find(|p| p.info.user_id == user_id)
    }

    pub fn player_mut(&mut self, user_id: u32) -> Option<&mut SnowStormPlayer> {
        self.players.iter_mut().find(|p| p.info.user_id == user_id)
    }

    pub(super) fn index_of(&self, user_id: u32) -> Option<usize> {
        self.players.iter().position(|p| p.info.user_id == user_id)
    }

    /// Java: `Game.movePlayer(gamePlayer, from, to)` onto a team: the player goes to the end of
    /// `info.team_id`'s list (a player already in the game changes team and keeps its state).
    pub fn add_player(&mut self, info: PlayerInfo) {
        let existing = self.index_of(info.user_id).map(|i| self.players.remove(i));
        let at = self
            .players
            .iter()
            .rposition(|p| p.info.team_id <= info.team_id)
            .map_or(0, |i| i + 1);
        let player = match existing {
            Some(mut p) => {
                p.info = info;
                p
            }
            None => SnowStormPlayer {
                info,
                in_game: true,
                in_room: false,
                object_id: -1,
                spawn: Position::default(),
                assigned_spawn: false,
                attrs: SnowStormAttributes::default(),
            },
        };
        self.players.insert(at, player);
    }

    /// Java: `Game.leaveGame` (`objects.remove(gamePlayer.getGameObject())` and the move out of
    /// the team). Snowballs it threw still land; it scores nothing for them.
    pub fn remove_player(&mut self, user_id: u32) {
        self.players.retain(|p| p.info.user_id != user_id);
        self.objects.retain(|o| *o != ObjectRef::Avatar(user_id));
    }

    /// Java: `GamePlayer.setInGame`.
    pub fn set_in_game(&mut self, user_id: u32, in_game: bool) {
        if let Some(p) = self.player_mut(user_id) {
            p.in_game = in_game;
        }
    }

    /// Whether the player's room is the arena room (Java checks
    /// `player.getRoomUser().getRoom() == this.room` each turn).
    pub fn set_in_room(&mut self, user_id: u32, in_room: bool) {
        if let Some(p) = self.player_mut(user_id) {
            p.in_room = in_room;
        }
    }

    pub(super) fn active(&self) -> impl Iterator<Item = (usize, &SnowStormPlayer)> {
        self.players.iter().enumerate().filter(|(_, p)| p.in_game)
    }

    /// Java: `createObjectId` / `getObjectId().incrementAndGet()`.
    pub(super) fn create_object_id(&mut self) -> i32 {
        self.object_counter += 1;
        self.object_counter
    }

    /// Java: `isOppositionPlayer(a, b)`: anybody else in a one-team game, else another team.
    pub fn is_opposition(&self, a: (u32, i32), b: (u32, i32)) -> bool {
        if a.0 == b.0 {
            return false;
        }
        if self.team_amount == 1 {
            return true;
        }
        a.1 != b.1
    }

    /// Java: `SnowStormGame.initialise` after `Game.initialise` cleared the objects: one machine
    /// object (empty) per `snowball_machine` item of the map. The core's part (room, model,
    /// timers) is described in the module docs.
    pub fn initialise(&mut self) {
        self.objects.clear();
        let Some(map) = self.map.clone() else {
            error!(
                "SnowStorm map {} is not loaded (missing tools/snowstorm_maps/arena_{}.dat?)",
                self.map_id, self.map_id
            );
            return;
        };
        for item in map.items() {
            if item.is_snowball_machine() {
                let object_id = self.create_object_id();
                self.machines.push(SnowStormMachine {
                    object_id,
                    x: item.x,
                    y: item.y,
                    snowballs: 0,
                });
                self.objects
                    .push(ObjectRef::Machine(self.machines.len() - 1));
            }
        }
    }

    /// Java: `assignSpawnPoints` (after the core's `regenerateCollisionMap`): every player of
    /// every team gets a spawn, a random rotation (0 to 6), full health, 5 snowballs, a new
    /// object id and an avatar object. `next_int(n)` is Java's `ThreadLocalRandom.nextInt(n)`
    /// (`0..n`).
    pub fn assign_spawn_points(
        &mut self,
        next_int: &mut impl FnMut(i32) -> i32,
        now: i64,
    ) -> Vec<SpawnAssignment> {
        for p in &mut self.players {
            p.assigned_spawn = false;
        }
        let mut assigned = Vec::with_capacity(self.players.len());
        for i in 0..self.players.len() {
            let spawn = self.generate_spawn(i, next_int);
            let rotation = next_int(7);
            self.players[i].spawn = spawn;
            self.players[i].assigned_spawn = true;
            self.set_activity_state(i, ActivityState::Normal, None, now);
            let object_id = self.create_object_id();
            let p = &mut self.players[i];
            let a = &mut p.attrs;
            a.rotation = rotation;
            a.walking = false;
            a.current_position = spawn;
            a.walk_goal = None;
            a.next_goal = None;
            a.immunity_expiry = 0;
            a.score = 0;
            a.snowballs = 5;
            a.health = 4;
            a.goal_world_coordinates = None;
            p.object_id = object_id;
            assigned.push(SpawnAssignment {
                user_id: p.info.user_id,
                object_id,
                spawn,
                rotation,
            });
            let user_id = p.info.user_id;
            self.objects.push(ObjectRef::Avatar(user_id));
        }
        assigned
    }

    /// Java: `generateSpawn` (Aeyra's iterative version): up to 200 random tiles of random
    /// clusters; the first walkable one is the fallback, and one far enough from every placed
    /// active player wins (any walkable one after 150 attempts).
    fn generate_spawn(&self, i: usize, next_int: &mut impl FnMut(i32) -> i32) -> Position {
        let Some(map) = self.map.as_deref() else {
            return SPAWN_FALLBACK;
        };
        let clusters = map.spawn_clusters();
        if clusters.is_empty() {
            return SPAWN_FALLBACK;
        }
        let mut fallback = None;
        for attempt in 0..200 {
            let spawn = clusters[next_int(clusters.len() as i32) as usize];
            let circle = circle(Position::new(spawn.x, spawn.y), spawn.radius);
            if circle.is_empty() {
                continue;
            }
            let candidate = circle[next_int(circle.len() as i32) as usize];
            if !map.tile(candidate).is_some_and(|t| t.is_walkable()) {
                continue;
            }
            fallback.get_or_insert(candidate);
            let too_close = self.active().any(|(j, other)| {
                j != i
                    && other.assigned_spawn
                    && java_distance(other.spawn, candidate) < spawn.min_distance
            });
            if too_close && attempt < 150 {
                continue;
            }
            return candidate;
        }
        fallback.unwrap_or(SPAWN_FALLBACK)
    }

    /// Java: `SnowStormAttributes.setActivityState(state, runnable)`: a state other than normal
    /// goes back to normal after its time (unless a newer state replaced it), then runs `then`.
    pub(super) fn set_activity_state(
        &mut self,
        i: usize,
        state: ActivityState,
        then: Option<ResetAction>,
        now: i64,
    ) {
        let p = &mut self.players[i];
        let a = &mut p.attrs;
        a.activity_state = state;
        a.state_time = now + state.time_in_ms();
        a.state_counter += 1;
        if state != ActivityState::Normal {
            let kind = TimerKind::ActivityReset {
                user_id: p.info.user_id,
                token: a.state_counter,
                then,
            };
            self.timers.schedule(now + state.time_in_ms(), kind);
        }
    }

    pub(super) fn run_activity_reset(
        &mut self,
        user_id: u32,
        token: u64,
        then: Option<ResetAction>,
        now: i64,
    ) {
        let Some(i) = self.index_of(user_id) else {
            return;
        };
        if self.players[i].attrs.state_counter != token {
            return;
        }
        self.players[i].attrs.activity_state = ActivityState::Normal;
        match then {
            Some(ResetAction::AfterStun) => {
                self.set_activity_state(i, ActivityState::InvincibleAfterStun, None, now);
                self.players[i].attrs.immunity_expiry =
                    now + ActivityState::InvincibleAfterStun.time_in_ms();
            }
            Some(ResetAction::AfterCreate) => {
                let a = &mut self.players[i].attrs;
                if a.is_walkable() && a.health != 0 {
                    a.snowballs += 1;
                }
            }
            None => {}
        }
    }

    /// Java: `handleSnowballLanding`: the first damageable active opponent of the thrower on
    /// (or stepping onto) the landing tile is hit: +1 for the thrower, -1 health, and stunned
    /// at 0. A miss only deletes the ball when it was blocked.
    pub(super) fn handle_snowball_landing(
        &mut self,
        snowball: &Snowball,
        delete_after_hit: bool,
        now: i64,
    ) {
        if self.turns.is_none() {
            return; // Arena already disposed
        }
        let Some(map) = self.map.as_deref() else {
            return;
        };
        let landed = snowball.target();
        if map.tile(landed).is_none() {
            return;
        }
        let thrower = (snowball.thrower, self.thrower_team(snowball));
        let hit = self
            .active()
            .find(|(_, p)| {
                self.is_opposition((p.info.user_id, p.info.team_id), thrower)
                    && (p.attrs.current_position == landed || p.attrs.next_goal == Some(landed))
                    && p.attrs.is_damageable(now)
            })
            .map(|(i, _)| i);

        match hit {
            Some(i) if self.players[i].attrs.health > 0 => {
                let thrower_object_id = self.thrower_object_id(snowball);
                if let Some(t) = self.player_mut(snowball.thrower) {
                    t.attrs.score += 1;
                }
                let direction =
                    walk_direction(snowball.from_x, snowball.from_y, landed.x, landed.y);
                self.send_queue(
                    0,
                    1,
                    TurnEvent::Hit {
                        thrower_id: thrower_object_id,
                        target_id: self.players[i].object_id,
                        direction,
                    },
                );
                if delete_after_hit {
                    self.send_queue(
                        0,
                        1,
                        TurnEvent::DeleteObject {
                            object_id: snowball.object_id,
                        },
                    );
                }
                self.players[i].attrs.health -= 1;
                if self.players[i].attrs.health == 0 {
                    self.stun_player(snowball, i, landed, now);
                }
            }
            _ => {
                if snowball.blocked {
                    self.send_queue(
                        0,
                        1,
                        TurnEvent::DeleteObject {
                            object_id: snowball.object_id,
                        },
                    );
                }
            }
        }
    }

    /// Java: `stunPlayerHandler`: +5 for the thrower; the victim loses its snowballs, gets its
    /// health back and is stunned for 7.5 s, then invincible for 3.6 s.
    fn stun_player(&mut self, snowball: &Snowball, i: usize, landed: Position, now: i64) {
        let direction = 45 * walk_direction(snowball.from_x, snowball.from_y, landed.x, landed.y);
        let thrower_id = self.thrower_object_id(snowball);
        self.send_queue(
            0,
            1,
            TurnEvent::Stun {
                stunned_id: self.players[i].object_id,
                thrower_id,
                direction,
            },
        );
        if let Some(t) = self.player_mut(snowball.thrower) {
            t.attrs.score += 5;
        }
        let a = &mut self.players[i].attrs;
        a.snowballs = 0;
        a.health = 4;
        self.set_activity_state(i, ActivityState::Stunned, Some(ResetAction::AfterStun), now);
    }

    pub(super) fn thrower_team(&self, snowball: &Snowball) -> i32 {
        self.player(snowball.thrower)
            .map_or(snowball.thrower_team, |p| p.info.team_id)
    }

    fn thrower_object_id(&self, snowball: &Snowball) -> i32 {
        self.player(snowball.thrower)
            .map_or(snowball.thrower_object_id, |p| p.object_id)
    }

    /// Java: `SnowStormGameTask.sendQueue`: `frames_future` 0 adds to sub-turn `sub_turn`
    /// (1-based; out of range is dropped) of the turn being built, otherwise it waits that many
    /// turns. Java's call sites throw a `NullPointerException` when the arena has no task; the
    /// event is dropped here.
    pub(super) fn send_queue(&mut self, frames_future: i32, sub_turn: usize, event: TurnEvent) {
        if let Some(turns) = self.turns.as_mut() {
            turns.send_queue(frames_future, sub_turn, event);
        }
    }

    /// Java: `finishGame`'s override: each active player's `GamePlayer.score` becomes its
    /// SnowStorm score (the core then sums the teams and runs `Game.finishGame`).
    pub fn final_scores(&self) -> Vec<(u32, i32)> {
        self.active()
            .map(|(_, p)| (p.info.user_id, p.attrs.score))
            .collect()
    }

    /// Java: `hasEnoughPlayers` (whether a finished game may restart): somebody still active.
    pub fn has_enough_players(&self) -> bool {
        self.active().next().is_some()
    }

    /// Java: `Game.canGameStart`'s SnowStorm branch for one team: at least two active players.
    /// For more teams the core's generic rule applies (`snowstorm.start.minimum.active.teams`
    /// teams with an active player).
    pub fn can_start_one_team(&self) -> Option<bool> {
        (self.team_amount == 1).then(|| self.active().count() > 1)
    }

    /// Java: `getGameLength`: the chosen length while waiting or ended, else the seconds left.
    pub fn game_length(&self, started: bool, total_seconds_left: i32) -> i32 {
        if !started {
            if let Some(seconds) = length_choice_seconds(self.length_choice) {
                return seconds;
            }
        }
        total_seconds_left
    }

    /// Java: the lifetime `SnowStormGame.initialise` passes to `Game.initialise`: the length
    /// choice, unless `snowstorm.game.lifetime.seconds` is positive.
    pub fn lifetime_seconds(&self, configured_lifetime: i32) -> i32 {
        if configured_lifetime > 0 {
            return configured_lifetime;
        }
        length_choice_seconds(self.length_choice).unwrap_or(0)
    }
}

/// Java: the `gameLengthChoice` table (also `GameHistory.getGameLength`, which gives 0 for
/// anything else).
pub fn length_choice_seconds(choice: i32) -> Option<i32> {
    match choice {
        1 => Some(120),
        2 => Some(180),
        3 => Some(300),
        _ => None,
    }
}

/// Java: `Position.getCircle(radius)`: the tiles at a (truncated) distance of at most `radius`.
pub fn circle(center: Position, radius: i32) -> Vec<Position> {
    let mut tiles = Vec::new();
    for x in -radius..=radius {
        for y in -radius..=radius {
            let b = Position::new(center.x + x, center.y + y);
            if java_distance(center, b) <= radius {
                tiles.push(b);
            }
        }
    }
    tiles
}
