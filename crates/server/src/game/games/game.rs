//! Java: `game/games/Game.java`: a game's teams, players, observers and spectators, its life
//! (waiting, started, ended, restarted) and its 1 s timers, and the game-type seam (see the
//! module docs of [`super`]).

use std::sync::Arc;
use std::time::Duration;

use rusbbo_protocol::Message;
use tracing::error;

use super::battleball::BattleBall;
use super::history::{self, FinishWork};
use super::player::{GamePlayer, PlayerState, Team};
use super::snowstorm::{self, SnowStormGame};
use super::{GameState, GameType, Games, Op};
use crate::game::room::model::RoomModel;
use crate::game::room::{Room, RoomRef};
use crate::messages::outgoing::games::{self as out, InstanceView};
use crate::session::Session;
use crate::world::World;

/// The game-type specific state of a game (Java's subclasses of `Game`).
pub enum GameKind {
    BattleBall(Box<BattleBall>),
    /// The arena engine (`game/games/snowstorm/`): its map, players, objects, turns and timers.
    SnowStorm(Box<SnowStormGame>),
}

/// Java: the game's `FutureRunnable`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerKind {
    /// `preparingTimerRunnable`.
    Prepare,
    /// The preparing countdown after a restart (Java's second `preparingTimerRunnable`).
    RestartPrepare,
    /// `gameTimerRunnable`.
    Game,
    /// `restartRunnable`.
    Restart,
}

struct Timer {
    kind: TimerKind,
    generation: u64,
    handle: Option<tokio::task::AbortHandle>,
}

/// Java: `Game`.
pub struct Game {
    /// Never changes, unlike `id` (Java compares the objects).
    pub uid: u64,
    pub id: i32,
    pub map_id: i32,
    pub team_amount: i32,
    pub creator_name: String,
    pub creator_id: u32,
    pub game_type: GameType,
    pub state: GameState,
    pub name: String,
    /// Every game player of this game (Java's `GamePlayer` objects the game's lists refer to).
    pub players: Vec<GamePlayer>,
    pub teams: Vec<Team>,
    /// Java: `observers` (players in the lobby looking at the game).
    pub observers: Vec<Arc<Session>>,
    /// Java: `spectators`, by serial.
    pub spectators: Vec<u64>,
    pub preparing_seconds_left: i32,
    pub total_seconds_left: i32,
    pub restart_countdown: i64,
    /// Java: `objectId`, the last object id given out.
    pub object_id: i32,
    pub deleted: bool,
    pub game_started: bool,
    pub game_finished: bool,
    regenerate_map_interval: i64,
    regenerate_map_enabled: bool,
    /// The arena (Java: `room`), from the first start.
    pub room: Option<RoomRef>,
    pub model: Option<Arc<RoomModel>>,
    timers: Vec<Timer>,
    next_generation: u64,
    pub kind: GameKind,
}

impl Game {
    /// Java: `Game(id, mapId, gameType, name, teamAmount, gameCreator)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        world: &World,
        uid: u64,
        id: i32,
        map_id: i32,
        game_type: GameType,
        name: String,
        team_amount: i32,
        creator: (u32, String),
        kind: GameKind,
    ) -> Self {
        let settings = world.settings.read();
        Self {
            uid,
            id,
            map_id,
            team_amount,
            creator_id: creator.0,
            creator_name: creator.1,
            game_type,
            state: GameState::Waiting,
            name,
            players: Vec::new(),
            teams: (0..team_amount)
                .map(|id| Team {
                    id,
                    ..Default::default()
                })
                .collect(),
            observers: Vec::new(),
            spectators: Vec::new(),
            preparing_seconds_left: 0,
            total_seconds_left: 0,
            restart_countdown: 0,
            object_id: 0,
            deleted: false,
            game_started: false,
            game_finished: false,
            regenerate_map_enabled: settings.bool("regenerate.map.enabled"),
            regenerate_map_interval: settings.int("regenerate.map.interval"),
            room: None,
            model: None,
            timers: Vec::new(),
            next_generation: 0,
            kind,
        }
    }

    pub fn player(&self, serial: u64) -> Option<&GamePlayer> {
        self.players.iter().find(|p| p.serial == serial)
    }

    pub fn player_mut(&mut self, serial: u64) -> Option<&mut GamePlayer> {
        self.players.iter_mut().find(|p| p.serial == serial)
    }

    /// Java: `getActivePlayers`: the team members still in the game, team by team.
    pub fn active_players(&self) -> impl Iterator<Item = &GamePlayer> + '_ {
        self.teams.iter().flat_map(move |t| {
            t.players
                .iter()
                .filter_map(move |s| self.player(*s))
                .filter(|p| p.in_game)
        })
    }

    pub fn active_serials(&self) -> Vec<u64> {
        self.active_players().map(|p| p.serial).collect()
    }

    /// Java: `getTotalPlayers`: every team member.
    pub fn total_serials(&self) -> Vec<u64> {
        self.teams.iter().flat_map(|t| t.players.clone()).collect()
    }

    /// Java: `GameTeam.getActivePlayers().size()`.
    pub fn team_active(&self, team: usize) -> usize {
        self.teams[team]
            .players
            .iter()
            .filter_map(|s| self.player(*s))
            .filter(|p| p.in_game)
            .count()
    }

    /// Java: `Game.send`: to the players in the game and the spectators.
    pub fn send(&self, op: &mut Op, msg: Message) {
        for s in self.recipients() {
            op.send(&s, msg.clone());
        }
    }

    /// Java: `send(new GAMEINSTANCE(this))`.
    pub fn send_instance(&self, op: &mut Op) {
        for s in self.recipients() {
            op.send_instance(&s, self.uid, self.state);
        }
    }

    /// Java: `sendObservers`.
    pub fn send_observers(&self, op: &mut Op, msg: Message) {
        for s in &self.observers {
            op.send(s, msg.clone());
        }
    }

    pub fn send_observers_instance(&self, op: &mut Op) {
        for s in &self.observers {
            op.send_instance(s, self.uid, self.state);
        }
    }

    fn recipients(&self) -> Vec<Arc<Session>> {
        let mut out: Vec<Arc<Session>> = self.active_players().map(|p| p.session.clone()).collect();
        out.extend(
            self.spectators
                .iter()
                .filter_map(|s| self.player(*s))
                .map(|p| p.session.clone()),
        );
        out
    }

    /// Java: the per-team maximum of `canSwitchTeam` and `getMaxPlayers`.
    fn max_per_team(&self) -> usize {
        match self.team_amount {
            1 => 10,
            2 => 5,
            3 => 3,
            4 => 2,
            _ => 0,
        }
    }

    /// Java: `canSwitchTeam`.
    pub fn can_switch_team(&self, team_id: i32) -> bool {
        if team_id < 0 || team_id as usize >= self.teams.len() {
            return false;
        }
        self.team_active(team_id as usize) < self.max_per_team()
    }

    /// Java: `getMaxPlayers`.
    pub fn max_players(&self) -> i32 {
        self.max_per_team() as i32 * self.team_amount
    }

    /// Java: `canGameStart`.
    pub fn can_game_start(&self, world: &World) -> bool {
        if self.game_type == GameType::SnowStorm && self.team_amount == 1 {
            return self.active_players().count() > 1;
        }
        let active_teams = (0..self.teams.len())
            .filter(|&t| self.team_active(t) > 0)
            .count() as i64;
        active_teams >= minimum_active_teams(world, self.game_type)
    }

    /// Java: `createObjectId`.
    pub fn create_object_id(&mut self) -> i32 {
        self.object_id += 1;
        self.object_id
    }

    /// Java: `getTicketCost`.
    pub fn ticket_cost(&self, world: &World) -> i32 {
        ticket_cost(world, self.game_type)
    }

    /// Java: `new GAMEINSTANCE(this)`.
    pub fn instance_message(&self) -> Message {
        out::game_instance(&self.instance_view())
    }

    fn instance_view(&self) -> InstanceView {
        match self.state {
            GameState::Waiting => InstanceView::Waiting {
                id: self.id,
                name: self.name.clone(),
                creator_id: self.creator_id,
                creator_name: self.creator_name.clone(),
                length_choice: self.length_choice(),
                map_id: self.map_id,
                spectators: self.spectators.len() as i32,
                teams: self
                    .teams
                    .iter()
                    .map(|t| {
                        t.players
                            .iter()
                            .filter_map(|s| self.player(*s))
                            .map(|p| (p.session.instance_id(), p.name.clone()))
                            .collect()
                    })
                    .collect(),
                power_ups: self.power_ups(),
            },
            GameState::Started => InstanceView::Started {
                id: self.id,
                name: self.name.clone(),
                creator_name: self.creator_name.clone(),
                length_choice: self.length_choice(),
                map_id: self.map_id,
                teams: self
                    .teams
                    .iter()
                    .map(|t| {
                        t.players
                            .iter()
                            .filter_map(|s| self.player(*s))
                            .filter(|p| p.in_game)
                            .map(|p| p.name.clone())
                            .collect()
                    })
                    .collect(),
                power_ups: self.power_ups(),
            },
            GameState::Ended => InstanceView::Ended,
        }
    }

    /// Java: `INSTANCELIST`'s entry for this game.
    pub fn listed(&self) -> out::ListedGame {
        out::ListedGame {
            id: self.id,
            name: self.name.clone(),
            creator_id: self.creator_id,
            creator_name: self.creator_name.clone(),
            game_length: self.game_length(),
            map_id: self.map_id,
        }
    }

    /// Spawns a timer of this game (Java: `scheduleAtFixedRate(runnable, delay, 1 s)`),
    /// replacing one of the same kind.
    pub(crate) fn schedule_timer(&mut self, world: &World, kind: TimerKind, delay: Duration) {
        self.cancel_timer(kind);
        self.next_generation += 1;
        let generation = self.next_generation;
        let handle = match (world.arc(), tokio::runtime::Handle::try_current()) {
            (Some(world), Ok(rt)) => {
                let world = Arc::downgrade(&world);
                let uid = self.uid;
                let task = rt.spawn(async move {
                    let start = tokio::time::Instant::now() + delay;
                    let mut interval = tokio::time::interval_at(start, Duration::from_secs(1));
                    loop {
                        interval.tick().await;
                        let Some(world) = world.upgrade() else {
                            return;
                        };
                        let keep =
                            crate::game::contain_panic(format_args!("game timer {kind:?}"), || {
                                timer_tick(&world, uid, kind, generation)
                            });
                        if keep != Some(true) {
                            return;
                        }
                    }
                });
                Some(task.abort_handle())
            }
            _ => None,
        };
        self.timers.push(Timer {
            kind,
            generation,
            handle,
        });
    }

    /// Java: `cancelFuture` of one timer.
    pub(crate) fn cancel_timer(&mut self, kind: TimerKind) {
        self.timers.retain(|t| {
            if t.kind == kind {
                if let Some(h) = &t.handle {
                    h.abort();
                }
                false
            } else {
                true
            }
        });
    }

    /// Java: `cancelTimers`.
    pub(crate) fn cancel_timers(&mut self) {
        for t in self.timers.drain(..) {
            if let Some(h) = t.handle {
                h.abort();
            }
        }
    }

    fn timer_is(&self, kind: TimerKind, generation: u64) -> bool {
        self.timers
            .iter()
            .any(|t| t.kind == kind && t.generation == generation)
    }
}

/// Java: `getTicketCost(gameType)` (`<type>.ticket.charge`).
pub fn ticket_cost(world: &World, game_type: GameType) -> i32 {
    world
        .settings
        .read()
        .int(&format!("{}.ticket.charge", game_type.key())) as i32
}

/// `<type>.start.minimum.active.teams`.
fn minimum_active_teams(world: &World, game_type: GameType) -> i64 {
    world
        .settings
        .read()
        .int(&format!("{}.start.minimum.active.teams", game_type.key()))
}

fn setting_seconds(world: &World, game_type: GameType, what: &str) -> i64 {
    world
        .settings
        .read()
        .int(&format!("{}.{what}", game_type.key()))
}

/// Java: `GameManager.getPreparingSeconds`.
pub fn preparing_seconds(world: &World, game_type: GameType) -> i32 {
    setting_seconds(world, game_type, "preparing.game.seconds") as i32
}

/// Java: `GameManager.getRestartSeconds`.
pub fn restart_seconds(world: &World, game_type: GameType) -> i32 {
    setting_seconds(world, game_type, "restart.game.seconds") as i32
}

/// Java: `GameManager.getLifetimeSeconds`.
pub fn lifetime_seconds(world: &World, game_type: GameType) -> i32 {
    setting_seconds(world, game_type, "game.lifetime.seconds") as i32
}

/// One run of a game timer: locks the arena (if any), then the games. Returns whether the timer
/// goes on.
fn timer_tick(world: &World, uid: u64, kind: TimerKind, generation: u64) -> bool {
    let room = {
        let Some(room) = world.games.arena_of_timer(uid, kind, generation) else {
            return false;
        };
        room
    };
    let run = |games: &mut Games, op: &mut Op| -> bool {
        let Some(gi) = games.game_index(uid) else {
            return false;
        };
        if !games.games[gi].timer_is(kind, generation) {
            return false;
        }
        games.on_timer(gi, kind, op)
    };
    match room {
        Some(room) => {
            let deferred = {
                let mut r = room.lock();
                let keep = world.games.run(world, Some(&mut r), run);
                (keep, r.take_deferred())
            };
            crate::game::room::run_deferred(world, deferred.1);
            deferred.0
        }
        None => world.games.run(world, None, run),
    }
}

impl super::GameManager {
    /// For a timer: `None` when the timer is gone, else the game's arena.
    fn arena_of_timer(
        &self,
        uid: u64,
        kind: TimerKind,
        generation: u64,
    ) -> Option<Option<RoomRef>> {
        let games = self.state.lock();
        let game = games.game_by_uid(uid)?;
        game.timer_is(kind, generation).then(|| game.room.clone())
    }
}

impl Games {
    /// One second of a timer. Returns whether it goes on.
    fn on_timer(&mut self, gi: usize, kind: TimerKind, op: &mut Op) -> bool {
        match kind {
            TimerKind::Prepare => {
                // Java: preparingTimerRunnable of startGame.
                self.game_prepare_tick(gi, op);
                if self.games[gi].active_players().next().is_none() {
                    let uid = self.games[gi].uid;
                    self.games[gi].cancel_timer(kind);
                    self.kill_spectators(gi, op);
                    self.remove_game(uid);
                    return false;
                }
                let left = self.games[gi].preparing_seconds_left;
                self.games[gi].preparing_seconds_left -= 1;
                if left == 0 {
                    self.games[gi].cancel_timer(kind);
                    self.begin_game(gi, op);
                    return false;
                }
                true
            }
            TimerKind::RestartPrepare => {
                if !self.games[gi].has_enough_players() {
                    self.games[gi].cancel_timer(kind);
                    return false;
                }
                self.game_prepare_tick(gi, op);
                let left = self.games[gi].preparing_seconds_left;
                self.games[gi].preparing_seconds_left -= 1;
                if left == 0 {
                    self.games[gi].cancel_timer(kind);
                    self.begin_game(gi, op);
                    return false;
                }
                true
            }
            TimerKind::Game => {
                self.game_tick(gi, op);
                let game = &mut self.games[gi];
                if game.regenerate_map_enabled
                    && game.regenerate_map_interval != 0
                    && i64::from(game.total_seconds_left) % game.regenerate_map_interval == 0
                {
                    if let Some(room) = op.room.as_deref_mut() {
                        room.regenerate_map();
                    }
                }
                game.total_seconds_left -= 1;
                if game.total_seconds_left <= 0
                    || !game.can_timer_continue()
                    || game.active_players().next().is_none()
                {
                    game.cancel_timer(kind);
                    self.finish_game(gi, op);
                    return false;
                }
                true
            }
            TimerKind::Restart => {
                let game = &self.games[gi];
                if game.state != GameState::Ended {
                    self.games[gi].cancel_timer(kind);
                    return false;
                }
                if !game.has_enough_players() {
                    self.games[gi].cancel_timer(kind);
                    // Aeyra: nobody left in the finished game, remove it.
                    if self.games[gi].active_players().next().is_none() {
                        let uid = self.games[gi].uid;
                        self.remove_game(uid);
                        if let Some(gi) = self.graveyard_index(uid) {
                            self.kill_spectators_of_removed(gi, op);
                        }
                    }
                    return false;
                }
                let any_afk = game.active_players().any(|p| !p.clicked_restart);
                self.games[gi].restart_countdown -= 1;
                if self.games[gi].restart_countdown == 0 {
                    self.games[gi].cancel_timer(kind);
                    if any_afk {
                        self.trigger_restart(gi, op);
                    }
                    return false;
                }
                true
            }
        }
    }

    fn graveyard_index(&self, uid: u64) -> Option<usize> {
        self.graveyard.iter().position(|g| g.uid == uid)
    }

    /// Java: `killSpectators` of a game removed in this operation.
    fn kill_spectators_of_removed(&mut self, index: usize, op: &mut Op) {
        let game = &mut self.graveyard[index];
        let spectators: Vec<u64> = std::mem::take(&mut game.spectators);
        let targets: Vec<(Arc<Session>, Option<RoomRef>)> = spectators
            .iter()
            .filter_map(|s| game.player(*s))
            .map(|p| (p.session.clone(), game.room.clone()))
            .collect();
        let lobby = game.game_type;
        for (session, room) in targets {
            send_to_lobby(op, session, room, lobby);
        }
    }

    /// Java: `movePlayer(gamePlayer, fromTeamId, toTeamId)`.
    pub fn move_player(&mut self, gi: usize, serial: u64, from: i32, to: i32, op: &mut Op) {
        let game = &mut self.games[gi];
        let waiting_or_ended = matches!(game.state, GameState::Waiting | GameState::Ended);
        if from != -1 {
            if waiting_or_ended {
                if let Some(team) = game.teams.get_mut(from as usize) {
                    team.players.retain(|s| *s != serial);
                }
            }
            if let Some(gp) = game.player_mut(serial) {
                gp.in_game = false;
            }
        }
        let mut clear_current = None;
        if to != -1 {
            if let Some(team) = game.teams.get_mut(to as usize) {
                if !team.players.contains(&serial) {
                    team.players.push(serial);
                }
            }
            if let Some(gp) = game.player_mut(serial) {
                gp.team_id = to;
                gp.in_game = true;
            }
        } else {
            let team_id = game.player(serial).map_or(-1, |p| p.team_id);
            if waiting_or_ended {
                if let Some(team) = game.teams.get_mut(team_id as usize) {
                    team.players.retain(|s| *s != serial);
                }
            } else if let Some(gp) = game.player_mut(serial) {
                // Not taken out of the team: "0" score at the end.
                gp.in_game = false;
            }
            clear_current = game.player(serial).map(|p| p.session.id);
        }
        if let Some(session_id) = clear_current {
            self.set_current(session_id, None);
        }
        let game = &self.games[gi];
        game.send_instance(op);
        game.send_observers_instance(op);
    }

    /// Java: `leaveGame(gamePlayer)`.
    pub fn leave_game(&mut self, gi: usize, serial: u64, op: &mut Op) {
        let game = &mut self.games[gi];
        let Some(gp) = game.player(serial) else {
            return;
        };
        let session = gp.session.clone();
        let team_id = gp.team_id;
        let is_spectator = game.spectators.contains(&serial);
        game.spectators.retain(|s| *s != serial);
        game.remove_player_object(serial);
        self.set_current(session.id, None);
        let id = self.games[gi].id;
        op.send(&session, out::game_deleted(id));
        if !is_spectator {
            self.move_player(gi, serial, team_id, -1, op);
        } else {
            let game = &self.games[gi];
            game.send_instance(op);
            game.send_observers_instance(op);
        }
        let game = &self.games[gi];
        let creator_left =
            game.state == GameState::Waiting && game.creator_id == session_user(game, serial);
        if !game.deleted && (creator_left || game.active_players().next().is_none()) {
            let uid = game.uid;
            self.games[gi].deleted = true;
            self.games[gi].cancel_timers();
            let game = &self.games[gi];
            game.send(op, out::game_deleted(id));
            game.send_observers(op, out::game_deleted(id));
            let members: Vec<(u64, Option<u64>)> = game
                .total_serials()
                .into_iter()
                .map(|s| (s, game.player(s).map(|p| p.session.id)))
                .collect();
            for (s, session_id) in members {
                if let Some(session_id) = session_id {
                    if self.is_current(session_id, s) {
                        self.set_current(session_id, None);
                    }
                }
                if let Some(p) = self.games[gi].player_mut(s) {
                    p.in_game = false;
                    p.game_id = -1;
                }
            }
            // Java: stopObservingGame finds no game any more (it was removed): only the id goes.
            let observers: Vec<u64> = self.games[gi].observers.iter().map(|s| s.id).collect();
            for o in observers {
                self.forget_observing(o);
            }
            self.games[gi].observers.clear();
            self.kill_spectators(gi, op);
            let spectators: Vec<u64> = std::mem::take(&mut self.games[gi].spectators);
            for s in spectators {
                if let Some(session_id) = self.games[gi].player(s).map(|p| p.session.id) {
                    self.set_current(session_id, None);
                }
            }
            self.remove_game(uid);
            if let Some(p) = self
                .graveyard
                .iter_mut()
                .find(|g| g.uid == uid)
                .and_then(|g| g.player_mut(serial))
            {
                p.game_id = -1;
            }
            return;
        }
        if let Some(p) = self.games[gi].player_mut(serial) {
            p.game_id = -1;
        }
    }

    /// Java: `killSpectators`: every spectator back to the lobby.
    fn kill_spectators(&mut self, gi: usize, op: &mut Op) {
        let game = &self.games[gi];
        let targets: Vec<Arc<Session>> = game
            .spectators
            .iter()
            .filter_map(|s| game.player(*s))
            .map(|p| p.session.clone())
            .collect();
        let (room, game_type) = (game.room.clone(), game.game_type);
        for session in targets {
            send_to_lobby(op, session, room.clone(), game_type);
        }
        self.games[gi].spectators.clear();
    }

    /// Java: `sendSpectatorToArena`.
    pub fn send_spectator_to_arena(&mut self, gi: usize, serial: u64, op: &mut Op) {
        let game = &self.games[gi];
        let Some(gp) = game.player(serial) else {
            return;
        };
        if !gp.spectator {
            return;
        }
        let session = gp.session.clone();
        let arena_key = game.room.as_ref().map(|r| r.id);
        // Java: `getRoomUser().getRoom() != this.room`.
        if session.room_key() == arena_key && arena_key.is_some() {
            return;
        }
        op.send(&session, out::game_location());
        if let Some(gp) = self.games[gi].player_mut(serial) {
            gp.entering_game = true;
        }
        self.games[gi].observers.retain(|s| s.id != session.id);
    }

    /// Java: `startGame` (with `BattleBallGame.startGame`'s queue clearing).
    pub fn start_game(&mut self, gi: usize, op: &mut Op) {
        if !self.initialise(gi, op) {
            return;
        }
        self.assign_spawn_points(gi, op);
        let game = &mut self.games[gi];
        for serial in game.active_serials() {
            if let Some(p) = game.player_mut(serial) {
                p.entering_game = true;
            }
        }
        game.send(op, out::game_location());
        for serial in self.games[gi].spectators.clone() {
            self.send_spectator_to_arena(gi, serial, op);
        }
        let world = op.world;
        self.games[gi].schedule_timer(world, TimerKind::Prepare, Duration::ZERO);
        let game = &self.games[gi];
        game.send_observers_instance(op);
        self.game_prepare(gi, op);
        self.games[gi].on_start();
    }

    /// Java: `initialise()`: started, the timers' seconds, the arena and its map. Returns false
    /// when the game type has no arena for the map.
    fn initialise(&mut self, gi: usize, op: &mut Op) -> bool {
        let world = op.world;
        let Some((model, room_name, lifetime)) = self.games[gi].arena_setup(world) else {
            return false;
        };
        let game = &mut self.games[gi];
        game.state = GameState::Started;
        game.preparing_seconds_left = preparing_seconds(world, game.game_type);
        game.total_seconds_left = lifetime;
        game.model = Some(model.clone());
        match &game.room {
            None => {
                game.room = Some(Room::new_arena(
                    world,
                    model.clone(),
                    room_name,
                    game.uid,
                    game.game_type,
                ));
            }
            Some(_) => {
                if let Some(room) = op.room.as_deref_mut() {
                    room.regenerate_map();
                }
            }
        }
        game.build_map(world);
        true
    }

    /// Java: `beginGame`, when the preparing countdown is over.
    fn begin_game(&mut self, gi: usize, op: &mut Op) {
        let game = &mut self.games[gi];
        game.game_started = true;
        let sessions: Vec<u64> = game.active_players().map(|p| p.session.id).collect();
        if let Some(room) = op.room.as_deref_mut() {
            for session_id in sessions {
                if let Some(idx) = room.entity_index(session_id) {
                    room.entities[idx].walking_allowed = true;
                    room.entities[idx].is_teleporting = false;
                }
            }
            room.regenerate_map();
        }
        let game = &self.games[gi];
        game.send(op, out::game_start(game.total_seconds_left));
        let world = op.world;
        self.games[gi].schedule_timer(world, TimerKind::Game, Duration::ZERO);
        self.games[gi].game_started_hook();
    }

    /// Java: `finishGame`: the scores, the history and the statistics, then `GAMEEND` (after
    /// the statistics, which may send achievements; they need the players' locks, so the end is
    /// sent from a second operation once they are done).
    pub fn finish_game(&mut self, gi: usize, op: &mut Op) {
        let game = &mut self.games[gi];
        game.game_started = false;
        game.game_finished = true;
        game.state = GameState::Ended;
        let uid = game.uid;
        if game.active_players().next().is_none() {
            game.deleted = true;
            game.cancel_timers();
            let id = game.id;
            game.send_observers(op, out::game_deleted(id));
            self.kill_spectators(gi, op);
            self.remove_game(uid);
            return;
        }
        let sessions: Vec<u64> = game.active_players().map(|p| p.session.id).collect();
        if let Some(room) = op.room.as_deref_mut() {
            for session_id in sessions {
                if let Some(idx) = room.entity_index(session_id) {
                    room.entities[idx].walking_allowed = false;
                    room.entities[idx].is_teleporting = false;
                }
            }
        }
        let work = history::finish(op.world, self, gi);
        op.defer(move |world| history::complete(world, uid, work));
    }

    /// The end of `finishGame` once the statistics are saved: `GAMEEND`, the restart countdown
    /// and the finished game for the observers.
    pub(crate) fn end_game(&mut self, uid: u64, work: &FinishWork, op: &mut Op) {
        let Some(gi) = self.game_index(uid) else {
            return;
        };
        let world = op.world;
        let game = &self.games[gi];
        let restart = restart_seconds(world, game.game_type);
        let teams: Vec<out::EndTeam> = game
            .teams
            .iter()
            .map(|t| out::EndTeam {
                players: t
                    .players
                    .iter()
                    .filter_map(|s| game.player(*s))
                    .map(|p| (p.object_id, p.name.clone(), p.score))
                    .collect(),
                points: t.points,
            })
            .collect();
        game.send(op, out::game_end(restart, &teams));
        let game = &mut self.games[gi];
        game.restart_countdown = i64::from(restart);
        game.schedule_timer(world, TimerKind::Restart, Duration::ZERO);
        let msg = work.history.instance_message(world);
        let game = &mut self.games[gi];
        game.send_observers(op, msg);
        game.observers.clear();
    }

    /// Java: `triggerRestart`: those who clicked "play again" play the next game, the others
    /// (and the spectators) go back to the lobby.
    pub fn trigger_restart(&mut self, gi: usize, op: &mut Op) {
        let game = &mut self.games[gi];
        let mut players = Vec::new();
        let mut afk = Vec::new();
        for serial in game.active_serials() {
            let gp = game.player_mut(serial).expect("active");
            if !gp.clicked_restart {
                afk.push(serial);
            } else {
                gp.clicked_restart = false;
                players.push(serial);
            }
        }
        let world = op.world;
        if players.len() as i64 >= minimum_active_teams(world, game.game_type) {
            self.restart_game(gi, &players, op);
        } else {
            afk.extend(players);
        }
        let game = &self.games[gi];
        afk.extend(game.spectators.iter().copied());
        let targets: Vec<Arc<Session>> = afk
            .iter()
            .filter_map(|s| game.player(*s))
            .map(|p| p.session.clone())
            .collect();
        let (room, game_type) = (game.room.clone(), game.game_type);
        for session in targets {
            send_to_lobby(op, session, room.clone(), game_type);
        }
    }

    /// Java: `restartGame(players)`.
    pub fn restart_game(&mut self, gi: usize, players: &[u64], op: &mut Op) {
        let game = &mut self.games[gi];
        game.cancel_timer(TimerKind::Restart);
        game.cancel_timer(TimerKind::Prepare);
        game.cancel_timer(TimerKind::RestartPrepare);
        game.cancel_timer(TimerKind::Game);
        for team in &mut game.teams {
            team.players.clear();
        }
        for &serial in players {
            let team_id = self.games[gi].player(serial).map_or(-1, |p| p.team_id);
            self.move_player(gi, serial, -1, team_id, op);
            let session_id = self.games[gi].player(serial).map(|p| p.session.id);
            if let (Some(room), Some(session_id)) = (op.room.as_deref_mut(), session_id) {
                if let Some(idx) = room.entity_index(session_id) {
                    room.entities[idx].walking_allowed = false;
                    room.entities[idx].is_teleporting = false;
                }
            }
        }
        // Java: reassignGameId (getAndIncrement, unlike createId).
        self.games[gi].id = self.counter;
        self.counter += 1;
        if !self.initialise(gi, op) {
            return;
        }
        self.assign_spawn_points(gi, op);
        let game = &mut self.games[gi];
        let id = game.id;
        let active: Vec<(u64, i32, crate::game::pathfinder::Position)> = game
            .active_players()
            .map(|p| (p.session.id, p.object_id, p.spawn))
            .collect();
        for serial in game.active_serials() {
            if let Some(p) = game.player_mut(serial) {
                p.game_id = id;
            }
        }
        if let Some(room) = op.room.as_deref_mut() {
            for (session_id, object_id, spawn) in active {
                room.set_game_position(session_id, object_id, spawn);
            }
        }
        let world = op.world;
        let game = &self.games[gi];
        let reset = game.game_reset(world, players, op.room.as_deref());
        game.send(op, reset);
        let status = game.full_game_status(world, op.room.as_deref(), &Default::default());
        game.send(op, status);
        game.send_observers(op, out::game_deleted(id));
        let game = &mut self.games[gi];
        game.schedule_timer(world, TimerKind::RestartPrepare, Duration::from_secs(1));
        self.game_prepare(gi, op);
    }

    /// Java: `gamePrepare` (the base part, then the game type's).
    fn game_prepare(&mut self, gi: usize, op: &mut Op) {
        let game = &mut self.games[gi];
        for serial in game.active_serials() {
            if let Some(p) = game.player_mut(serial) {
                p.set_xp(0);
                p.score = 0;
            }
        }
        game.game_prepare_hook();
        // Java: chargeTickets, in both game types' gamePrepare.
        let cost = game.ticket_cost(op.world);
        if cost > 0 {
            let sessions: Vec<Arc<Session>> =
                game.active_players().map(|p| p.session.clone()).collect();
            op.defer(move |world| {
                for session in sessions {
                    charge_tickets(world, &session, cost);
                }
            });
        }
    }

    fn game_prepare_tick(&mut self, gi: usize, op: &mut Op) {
        self.games[gi].game_prepare_tick_hook(op);
    }

    fn game_tick(&mut self, gi: usize, op: &mut Op) {
        self.games[gi].game_tick_hook(op);
    }

    fn assign_spawn_points(&mut self, gi: usize, op: &mut Op) {
        let world = op.world;
        let game = &mut self.games[gi];
        game.assign_spawn_points_hook(world);
    }
}

fn session_user(game: &Game, serial: u64) -> u32 {
    game.player(serial).map_or(0, |p| p.user_id)
}

/// Java: `sendToLobby(gamePlayer)`: out of the arena (which makes them leave the game) and a
/// `ROOMFORWARD` to the lobby, unless they are not in the arena any more.
pub(crate) fn send_to_lobby(
    op: &mut Op,
    session: Arc<Session>,
    arena: Option<RoomRef>,
    game_type: GameType,
) {
    op.defer(move |world| {
        let arena_key = arena.as_ref().map(|r| r.id);
        if arena_key.is_none() || session.room_key() != arena_key {
            return;
        }
        crate::game::room::leave_current_room(world, &session, false);
        if let Some(lobby) = world.games.lobby_room_id(world, game_type) {
            crate::game::moderation::forward_to(&session, true, lobby);
        }
    });
}

/// Java: `chargeTickets` for one player: up to the price, and `TICKET_BALANCE`.
fn charge_tickets(world: &World, session: &Arc<Session>, cost: i32) {
    let (tickets, room_id) = {
        let mut p = session.player.lock();
        let room_id = p.room_id;
        let Some(details) = p.details.as_mut() else {
            return;
        };
        let amount = cost.min(details.tickets);
        if amount > 0 {
            // Java: CurrencyDao.decreaseTickets (nothing when the player has none).
            match world.store.update_user(details.id, |u| u.tickets -= amount) {
                Ok(Some(stored)) => details.tickets = stored.tickets,
                Ok(None) => details.tickets -= amount,
                Err(e) => error!(user = details.id, "decreasing tickets: {e}"),
            }
        }
        (details.tickets, room_id)
    };
    if let Some(room) = room_id.and_then(|id| world.loaded_room(id)) {
        let mut r = room.lock();
        if let Some(idx) = r.entity_index(session.id) {
            r.entities[idx].details.tickets = tickets;
        }
    }
    session.send(crate::messages::outgoing::rooms_pool::ticket_balance(
        tickets,
    ));
}

/// The game type seam (see the module docs of [`super`]).
impl Game {
    /// Java: `hasEnoughPlayers`.
    pub fn has_enough_players(&self) -> bool {
        let active_teams = (0..self.teams.len())
            .filter(|&t| self.team_active(t) > 0)
            .count();
        match &self.kind {
            GameKind::BattleBall(_) => active_teams > 0,
            GameKind::SnowStorm(_) => {
                if self.team_amount == 1 {
                    self.active_players().next().is_some()
                } else {
                    active_teams > 0
                }
            }
        }
    }

    /// Java: `canTimerContinue`.
    pub fn can_timer_continue(&self) -> bool {
        match &self.kind {
            GameKind::BattleBall(bb) => bb.can_timer_continue(),
            // Java: SnowStormGame.canTimerContinue.
            GameKind::SnowStorm(_) => true,
        }
    }

    /// The arena of `initialise`: its model, the room's name and the game's length. `None` when
    /// the game type cannot start yet.
    fn arena_setup(&self, world: &World) -> Option<(Arc<RoomModel>, &'static str, i32)> {
        match &self.kind {
            GameKind::BattleBall(_) => {
                let model = match &self.model {
                    Some(m) => m.clone(),
                    None => world.games.data.model(self.game_type, self.map_id)?,
                };
                Some((
                    model,
                    "Battleball Arena",
                    lifetime_seconds(world, self.game_type),
                ))
            }
            // Java: SnowStormGame.initialise: the map's heightmap (`snowwar_arena_0`), the
            // length from the choice (2, 3 or 5 minutes, or `snowstorm.game.lifetime.seconds`
            // when above 0); the machines are `build_map`'s.
            GameKind::SnowStorm(s) => Some((
                snowstorm::arena::arena_model(self.map_id),
                "SnowStorm Arena",
                s.lifetime_seconds(lifetime_seconds(world, self.game_type)),
            )),
        }
    }

    /// Java: `buildMap`.
    fn build_map(&mut self, world: &World) {
        let Some(model) = self.model.clone() else {
            return;
        };
        let map_id = self.map_id;
        match &mut self.kind {
            GameKind::BattleBall(bb) => {
                bb.build_map(&model, world.games.data.battleball_map(map_id));
            }
            // Java: the machines of SnowStormGame.initialise.
            GameKind::SnowStorm(s) => s.initialise(),
        }
    }

    /// Java: `assignSpawnPoints`.
    fn assign_spawn_points_hook(&mut self, world: &World) {
        match self.kind {
            GameKind::BattleBall(_) => super::battleball::assign_spawn_points(self, world),
            GameKind::SnowStorm(_) => snowstorm::arena::assign_spawn_points(self),
        }
    }

    /// Java: `BattleBallGame.startGame`'s part after `super.startGame()`.
    fn on_start(&mut self) {
        if let GameKind::BattleBall(bb) = &mut self.kind {
            bb.clear_queues();
        }
    }

    fn game_prepare_hook(&mut self) {
        if let GameKind::BattleBall(bb) = &mut self.kind {
            bb.game_prepare();
        }
    }

    fn game_prepare_tick_hook(&mut self, _op: &mut Op) {
        let map_id = self.map_id;
        if let GameKind::BattleBall(bb) = &mut self.kind {
            bb.game_prepare_tick(map_id, &mut self.object_id);
        }
    }

    fn game_started_hook(&mut self) {}

    fn game_tick_hook(&mut self, op: &mut Op) {
        let map_id = self.map_id;
        if let GameKind::BattleBall(bb) = &mut self.kind {
            bb.game_tick(map_id, &mut self.object_id, op.room.as_deref());
        }
    }

    /// Java: `objects.remove(gamePlayer.getGameObject())`.
    pub fn remove_player_object(&mut self, serial: u64) {
        let user_id = self.player(serial).map(|p| p.user_id);
        match &mut self.kind {
            GameKind::BattleBall(bb) => bb.remove_player_object(serial),
            GameKind::SnowStorm(s) => {
                if let Some(user_id) = user_id {
                    s.remove_avatar(user_id);
                }
            }
        }
    }

    /// The game length choice `GAMEINSTANCE` writes (SnowStorm).
    pub fn length_choice(&self) -> Option<i32> {
        match &self.kind {
            GameKind::SnowStorm(s) => Some(s.length_choice),
            GameKind::BattleBall(_) => None,
        }
    }

    /// The game length `INSTANCELIST` writes (Java: `SnowStormGame.getGameLength`).
    pub fn game_length(&self) -> Option<i32> {
        match &self.kind {
            GameKind::SnowStorm(s) => Some(
                if matches!(self.state, GameState::Waiting | GameState::Ended) {
                    match s.length_choice {
                        1 => 120,
                        2 => 180,
                        3 => 300,
                        // Java reads totalSecondsLeft, which is null before the first start
                        // (and throws); a real client only sends 1-3.
                        _ => self.total_seconds_left,
                    }
                } else {
                    self.total_seconds_left
                },
            ),
            GameKind::BattleBall(_) => None,
        }
    }

    /// The power ups `GAMEINSTANCE` writes (BattleBall).
    pub fn power_ups(&self) -> Option<String> {
        match &self.kind {
            GameKind::BattleBall(bb) => Some(join_ints(&bb.allowed_power_ups)),
            GameKind::SnowStorm(_) => None,
        }
    }

    /// `games_played_history.extra_data`.
    pub fn history_extra_data(&self) -> String {
        match &self.kind {
            GameKind::BattleBall(bb) => join_ints(&bb.allowed_power_ups),
            GameKind::SnowStorm(s) => s.length_choice.to_string(),
        }
    }

    /// Java: `GameTeam.calculateScore` for every team.
    pub fn calculate_team_points(&mut self) {
        match &self.kind {
            GameKind::BattleBall(bb) => {
                let points: Vec<i32> = self.teams.iter().map(|t| bb.team_points(t.id)).collect();
                for (team, points) in self.teams.iter_mut().zip(points) {
                    team.points = points;
                }
            }
            GameKind::SnowStorm(_) => {
                snowstorm::arena::set_final_scores(self);
                for t in 0..self.teams.len() {
                    let sum = self.teams[t]
                        .players
                        .iter()
                        .filter_map(|s| self.player(*s))
                        .map(|p| p.score)
                        .sum();
                    self.teams[t].points = sum;
                }
            }
        }
    }

    /// Java: `GamePlayer.calculateScore`.
    pub fn calculate_player_score(&mut self, serial: u64) {
        let Some(gp) = self.player(serial) else {
            return;
        };
        if !gp.in_game {
            let gp = self.player_mut(serial).expect("found");
            gp.score = 0;
            return;
        }
        let user_id = gp.user_id;
        if let GameKind::BattleBall(bb) = &self.kind {
            let score = bb.player_score(user_id);
            if let Some(gp) = self.player_mut(serial) {
                gp.score = score;
            }
        }
    }

    /// Java: `FULLGAMESTATUS`. `positions` has the room positions of players who are not in the
    /// arena (Java reads each player's room user wherever it is).
    pub fn full_game_status(
        &self,
        world: &World,
        arena: Option<&Room>,
        positions: &std::collections::HashMap<u64, crate::game::pathfinder::Position>,
    ) -> Message {
        match &self.kind {
            GameKind::BattleBall(bb) => bb.full_game_status(self, world, arena, positions),
            GameKind::SnowStorm(s) => snowstorm::arena::full_game_status(self, s, world),
        }
    }

    /// Java: `GAMERESET`.
    fn game_reset(&self, world: &World, players: &[u64], arena: Option<&Room>) -> Message {
        match &self.kind {
            GameKind::BattleBall(_) => super::battleball::game_reset(self, world, players, arena),
            GameKind::SnowStorm(s) => snowstorm::arena::game_reset(s, world),
        }
    }

    /// Java: `BattleBallPlayerState` of a game player (BattleBall only).
    pub fn player_state(&self, serial: u64) -> PlayerState {
        self.player(serial)
            .and_then(|p| p.player_state)
            .unwrap_or_default()
    }
}

/// Java: `String.join(",", ...)` of integers.
pub fn join_ints(values: &[i32]) -> String {
    values
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
