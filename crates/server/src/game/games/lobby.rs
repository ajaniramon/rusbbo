//! The game lobbies (Java: `GameLobbyTrigger`, `BattleballLobbyTrigger`,
//! `SnowStormLobbyTrigger`): what a player sees on entering one, the instance list, and making a
//! game from the lobby's parameters.

use std::collections::HashMap;
use std::sync::Arc;

use rusbbo_protocol::Message;

use super::battleball::BattleBall;
use super::game::{Game, GameKind};
use super::player::{GamePlayer, PlayerDetails};
use super::{GameType, Games, Op, points_of};
use crate::game::room::Room;
use crate::messages::outgoing::games::{self as out, PlayerInfo};
use crate::session::Session;
use crate::world::World;

/// A value of `GAMEPARAMETERVALUES` (Java: an `Integer` or a `String` in the map).
#[derive(Clone, Debug)]
pub enum Param {
    Int(i32),
    Text(String),
}

/// Java's casts of the parameters, which throw on a missing or mistyped one: `None` is that
/// exception.
fn int(params: &HashMap<String, Param>, name: &str) -> Option<i32> {
    match params.get(name)? {
        Param::Int(v) => Some(*v),
        Param::Text(_) => None,
    }
}

fn text<'a>(params: &'a HashMap<String, Param>, name: &str) -> Option<&'a str> {
    match params.get(name)? {
        Param::Text(v) => Some(v),
        Param::Int(_) => None,
    }
}

/// Java: `<type>.create.game.enabled`.
pub fn create_enabled(world: &World, game_type: GameType) -> bool {
    world
        .settings
        .read()
        .bool(&format!("{}.create.game.enabled", game_type.key()))
}

/// Java: `GameLobbyTrigger.getInstanceList`.
pub fn instance_list(games: &Games, game_type: GameType) -> Message {
    let created: Vec<out::ListedGame> = games
        .games_of_type(game_type)
        .filter(|g| g.state == super::GameState::Waiting)
        .map(Game::listed)
        .collect();
    let started: Vec<out::ListedGame> = games
        .games_of_type(game_type)
        .filter(|g| g.state == super::GameState::Started)
        .map(Game::listed)
        .collect();
    let finished: Vec<out::ListedGame> = games
        .last_played(game_type)
        .iter()
        .map(super::GameHistory::listed)
        .collect();
    out::instance_list(&created, &started, &finished)
}

/// Java: `GAMEPLAYERINFO(type, players)`.
pub fn player_info(
    world: &World,
    game_type: GameType,
    sessions: &[(i32, Arc<Session>)],
) -> Message {
    let players: Vec<PlayerInfo> = sessions
        .iter()
        .map(|(instance_id, session)| {
            let points = points_of(session, game_type);
            PlayerInfo {
                instance_id: *instance_id,
                points,
                rank: world
                    .games
                    .data
                    .rank_by_points(game_type, points)
                    .map(|r| r.title.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();
    out::game_player_info(&players)
}

impl Room {
    /// Java: the lobby triggers' `onRoomEntry`: `LOUNGEINFO`, everyone's points and ranks, and
    /// the newcomer's to the room.
    pub(crate) fn game_lobby_entry(&mut self, world: &World, game_type: GameType, idx: usize) {
        if !self.entities[idx].is_player() || !create_enabled(world, game_type) {
            return;
        }
        let session = self.entities[idx].session.clone();
        session.send(out::lounge_info());
        let players: Vec<(i32, Arc<Session>)> = self
            .entities
            .iter()
            .filter(|e| e.is_player())
            .map(|e| (e.instance_id, e.session.clone()))
            .collect();
        session.send(player_info(world, game_type, &players));
        let me = [(self.entities[idx].instance_id, session.clone())];
        self.send(player_info(world, game_type, &me));
        if game_type == GameType::SnowStorm && world.settings.read().bool("snowstorm.alpha.alert") {
            session.send(crate::messages::outgoing::alerts::alert(
                "Welcome to SnowStorm testing!<br><br>The game is almost complete but may have a few quirks to figure out. Please report any issues (aside from desync) to Alex.<br><br>Thanks to the following for making it possible:<br>  - Sefhriloff<br>  - Ascii",
            ));
        }
    }

    /// Java: the lobby triggers' `onRoomLeave`: a player stops observing a game.
    pub(crate) fn game_lobby_leave(&mut self, world: &World, session_id: u64) {
        world.games.run(world, Some(self), |games, _| {
            if games.observing(session_id) != -1 {
                games.stop_observing(session_id);
            }
        });
    }
}

/// How `createGame` ended.
pub enum Created {
    /// Made, or refused for a bad value: the room gets the instance list.
    Done,
    /// Java threw (a missing or mistyped parameter): nothing more happens.
    Failed,
}

/// Java: the lobby triggers' `createGame`.
pub fn create_game(
    games: &mut Games,
    op: &mut Op,
    game_type: GameType,
    session: &Arc<Session>,
    creator: PlayerDetails,
    params: &HashMap<String, Param>,
) -> Created {
    let world = op.world;
    let Some(map_id) = int(params, "fieldType") else {
        return Created::Failed;
    };
    let kind = match game_type {
        GameType::BattleBall => {
            if !(1..=5).contains(&map_id) {
                return Created::Done;
            }
            let Some(teams) = int(params, "numTeams") else {
                return Created::Failed;
            };
            if !(2..=4).contains(&teams) {
                return Created::Done;
            }
            let Some(name) = text(params, "name") else {
                return Created::Failed;
            };
            if name.is_empty() {
                return Created::Done;
            }
            let Some(power_ups) = text(params, "allowedPowerups") else {
                return Created::Failed;
            };
            let mut allowed = Vec::new();
            for p in power_ups.split(',') {
                if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                    // Java: Integer.parseInt, whose NumberFormatException aborts the handler.
                    let Ok(v) = p.parse::<i32>() else {
                        return Created::Failed;
                    };
                    allowed.push(v);
                }
            }
            (
                teams,
                name.to_string(),
                GameKind::BattleBall(Box::new(BattleBall::new(allowed))),
            )
        }
        GameType::SnowStorm => {
            if !(1..=7).contains(&map_id) {
                return Created::Done;
            }
            // Java: an arena that did not load from tools/snowstorm_maps.
            let maps = super::snowstorm::maps::instance();
            if maps.get(map_id).is_none() {
                op.send(
                    session,
                    crate::messages::outgoing::alerts::alert(
                        super::snowstorm::MAP_UNAVAILABLE_ALERT,
                    ),
                );
                return Created::Done;
            }
            let Some(teams) = int(params, "numTeams") else {
                return Created::Failed;
            };
            if !(1..=4).contains(&teams) {
                return Created::Done;
            }
            let Some(name) = text(params, "name") else {
                return Created::Failed;
            };
            if name.is_empty() {
                return Created::Done;
            }
            let Some(length_choice) = int(params, "gameLengthChoice") else {
                return Created::Failed;
            };
            (
                teams,
                name.to_string(),
                GameKind::SnowStorm(Box::new(super::snowstorm::SnowStormGame::new(
                    map_id,
                    teams,
                    length_choice,
                    &maps,
                ))),
            )
        }
        GameType::WobbleSquabble => return Created::Done,
    };
    let (teams, name, kind) = kind;
    let id = games.create_id();
    let uid = games.new_uid();
    let game = Game::new(
        world,
        uid,
        id,
        map_id,
        game_type,
        name,
        teams,
        (creator.user_id, creator.name.clone()),
        kind,
    );
    let serial = games.new_serial();
    let mut gp = GamePlayer::new(serial, session.clone(), creator);
    gp.game_id = id;
    gp.team_id = 0;
    let mut game = game;
    game.players.push(gp);
    games.set_current(session.id, Some(serial));
    // Java moves the creator into team 0 before the game is in the manager's list.
    games.games.push(game);
    let gi = games.games.len() - 1;
    games.move_player(gi, serial, -1, 0, op);
    Created::Done
}
