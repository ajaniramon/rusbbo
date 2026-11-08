//! Java: `messages/incoming/games`: the game lobbies' and arenas' messages (see `game::games`).

use std::collections::HashMap;
use std::sync::Arc;

use rand::RngExt;
use rusbbo_protocol::body::ClientReader;

use crate::game::games::game::ticket_cost;
use crate::game::games::lobby::{self, Created, Param};
use crate::game::games::player::{GamePlayer, PlayerDetails};
use crate::game::games::{GameKind, GameState, GameType, battleball};
use crate::game::room::{self, RoomRef, current_room, filter_input};
use crate::messages::Malformed;
use crate::messages::outgoing::alerts;
use crate::messages::outgoing::games::{self as out, CreateFailed, GameParameter, JoinFailed};
use crate::session::Session;
use crate::world::World;

/// The player's room when its model is a game lobby (Java: `getRoomTrigger() instanceof
/// GameLobbyTrigger`).
fn lobby_of(world: &World, session: &Session) -> Option<(RoomRef, GameType)> {
    let room = current_room(world, session)?;
    let game_type = GameType::of_lobby_trigger(&room.lock().model.trigger)?;
    Some((room, game_type))
}

/// The player's details a game player copies, and their tickets.
fn details_of(session: &Session) -> Option<(PlayerDetails, i32)> {
    let p = session.player.lock();
    let user = p.details.as_ref().filter(|_| p.logged_in)?;
    Some((
        PlayerDetails {
            user_id: user.id,
            name: user.username.clone(),
            motto: user.motto.clone(),
            figure: user.figure.clone(),
            sex: user.sex.clone(),
        },
        user.tickets,
    ))
}

/// Message 159: Java registers both `GETSELECTEDBADGES` and `GETINSTANCELIST` for it, and runs
/// each on its own copy of the body.
pub fn selected_badges_and_instance_list(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let body = reader.remaining();
    let badges = super::badges::get_selected_badges(world, session, &mut ClientReader::new(body));
    let list = get_instance_list(world, session, &mut ClientReader::new(body));
    badges.and(list)
}

/// Java: `GETINSTANCELIST`.
pub fn get_instance_list(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((_, game_type)) = lobby_of(world, session) else {
        return Ok(());
    };
    if !lobby::create_enabled(world, game_type) {
        return Ok(());
    }
    let msg = world.games.run(world, None, |games, _| {
        lobby::instance_list(games, game_type)
    });
    session.send(msg);
    Ok(())
}

/// Java: `OBSERVEINSTANCE`: a game's details, and its updates from then on (a finished game's
/// once).
pub fn observe_instance(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((_, game_type)) = lobby_of(world, session) else {
        return Ok(());
    };
    let game_id = reader.int().ok_or(Malformed)?;
    world.games.run(world, None, |games, op| {
        if let Some(gi) = games.game_by_id(game_id) {
            if games.games[gi].state != GameState::Ended {
                op.send_instance(session, games.games[gi].uid, games.games[gi].state);
                games.start_observing(session, game_id);
                return;
            }
        }
        if let Some(finished) = games.finished_game(game_type, game_id) {
            let msg = finished.instance_message(op.world);
            op.send(session, msg);
        }
    });
    Ok(())
}

/// Java: `UNOBSERVEINSTANCE`.
pub fn unobserve_instance(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    world.games.run(world, None, |games, _| {
        if games.observing(session.id) != -1 {
            games.stop_observing(session.id);
        }
    });
    Ok(())
}

/// Java: `INITIATECREATEGAME`: the parameters of a new game, if the player has the tickets.
pub fn initiate_create_game(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((_, game_type)) = lobby_of(world, session) else {
        return Ok(());
    };
    let parameters: &[GameParameter] = match game_type {
        GameType::BattleBall => &[
            GameParameter {
                name: "fieldType",
                editable: true,
                default_value: "1",
                min_max: Some((1, 5)),
            },
            GameParameter {
                name: "numTeams",
                editable: true,
                default_value: "2",
                min_max: Some((2, 4)),
            },
            GameParameter {
                name: "allowedPowerups",
                editable: true,
                default_value: "1,2,3,4,5,6,7,8",
                min_max: None,
            },
            GameParameter {
                name: "name",
                editable: true,
                default_value: "",
                min_max: None,
            },
        ],
        _ => &[
            GameParameter {
                name: "fieldType",
                editable: true,
                default_value: "1",
                min_max: Some((1, 7)),
            },
            GameParameter {
                name: "numTeams",
                editable: true,
                default_value: "2",
                min_max: Some((2, 4)),
            },
            GameParameter {
                name: "gameLengthChoice",
                editable: true,
                default_value: "1",
                min_max: Some((1, 3)),
            },
            GameParameter {
                name: "name",
                editable: true,
                default_value: "",
                min_max: None,
            },
        ],
    };
    let Some((_, tickets)) = details_of(session) else {
        return Ok(());
    };
    if tickets < ticket_cost(world, game_type) {
        session.send(out::create_failed(CreateFailed::TicketsNeeded));
        return Ok(());
    }
    session.send(out::game_parameters(parameters));
    Ok(())
}

/// Java: `GAMEPARAMETERVALUES`: makes a game (leaving the one the player is in first).
pub fn game_parameter_values(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((room, game_type)) = lobby_of(world, session) else {
        return Ok(());
    };
    let count = reader.int().ok_or(Malformed)?;
    let mut params = HashMap::new();
    for _ in 0..count {
        let name = super::latin1(reader.string().ok_or(Malformed)?);
        let is_text = reader.bool().ok_or(Malformed)?;
        let value = if is_text {
            Param::Text(super::latin1(&filter_input(
                reader.string().ok_or(Malformed)?,
            )))
        } else {
            Param::Int(reader.int().ok_or(Malformed)?)
        };
        params.insert(name, value);
    }
    let Some((details, tickets)) = details_of(session) else {
        return Ok(());
    };
    let mut r = room.lock();
    world.games.run(world, Some(&mut r), |games, op| {
        // Aeyra: leave the game the player is still in first.
        if let Some((gi, pi)) = games.current(session.id) {
            let serial = games.games[gi].players[pi].serial;
            if let Some((target, Some(_))) = games.current_game(session.id) {
                games.leave_game(target, serial, op);
            }
            games.set_current(session.id, None);
        }
        if tickets < ticket_cost(op.world, game_type) {
            return;
        }
        match lobby::create_game(games, op, game_type, session, details, &params) {
            Created::Failed => {}
            Created::Done => {
                let msg = lobby::instance_list(games, game_type);
                let sessions: Vec<Arc<Session>> = op
                    .room
                    .as_deref()
                    .map(|r| {
                        r.entities
                            .iter()
                            .filter(|e| e.is_player())
                            .map(|e| e.session.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for s in sessions {
                    op.send(&s, msg.clone());
                }
            }
        }
    });
    let deferred = r.take_deferred();
    drop(r);
    room::run_deferred(world, deferred);
    Ok(())
}

/// Java: `INITIATEJOINGAME`: joins a waiting game's team (or changes team).
pub fn initiate_join_game(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if current_room(world, session).is_none() {
        return Ok(());
    }
    let instance_id = reader.int().ok_or(Malformed)?;
    let team_id = reader.int().ok_or(Malformed)?;
    if lobby_of(world, session).is_none() {
        return Ok(());
    }
    let Some((details, tickets)) = details_of(session) else {
        return Ok(());
    };
    world.games.run(world, None, |games, op| {
        let Some(gi) = games.game_by_id(instance_id) else {
            return;
        };
        let game = &games.games[gi];
        if game.state != GameState::Waiting {
            return;
        }
        if tickets < game.ticket_cost(op.world) {
            op.send(session, out::join_failed(JoinFailed::TicketsNeeded, None));
            return;
        }
        if team_id < 0 || team_id >= game.team_amount {
            return;
        }
        if !game.can_switch_team(team_id) {
            op.send(
                session,
                out::join_failed(JoinFailed::TeamsFull, Some("join")),
            );
            return;
        }
        let game_uid = game.uid;
        // A spectator, or a player of another game, leaves it first.
        if let Some((cgi, cpi)) = games.current(session.id) {
            let gp = &games.games[cgi].players[cpi];
            if gp.spectator || gp.game_id != instance_id {
                let serial = gp.serial;
                // Java's leaveGame on the game the game player's id names. TODO(games): a
                // spectator whose game restarted (a stale id) names no game or another one.
                if let Some((target, Some(_))) = games.current_game(session.id) {
                    games.leave_game(target, serial, op);
                }
                games.set_current(session.id, None);
                if games.game_by_id(instance_id).is_none() {
                    return;
                }
            }
        }
        let Some(gi) = games.game_index(game_uid) else {
            return;
        };
        let serial = match games.current(session.id) {
            Some((cgi, cpi)) if cgi == gi => games.games[cgi].players[cpi].serial,
            _ => {
                let serial = games.new_serial();
                let mut gp = GamePlayer::new(serial, session.clone(), details);
                gp.game_id = games.games[gi].id;
                gp.team_id = team_id;
                games.games[gi].players.push(gp);
                games.set_current(session.id, Some(serial));
                serial
            }
        };
        let game = &mut games.games[gi];
        game.observers.retain(|s| s.id != session.id);
        game.spectators.retain(|s| *s != serial);
        let from = game.player(serial).map_or(-1, |p| p.team_id);
        games.move_player(gi, serial, from, team_id, op);
    });
    Ok(())
}

/// Java: `LEAVEGAME`.
pub fn leave_game(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    if lobby_of(world, session).is_none() {
        return Ok(());
    }
    world.games.run(world, None, |games, op| {
        if let Some((target, Some(pi))) = games.current_game(session.id) {
            let serial = games.games[target].players[pi].serial;
            games.leave_game(target, serial, op);
        }
    });
    Ok(())
}

/// Java: `KICKPLAYER`: the creator takes a player out of the waiting game.
pub fn kick_player(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if lobby_of(world, session).is_none() {
        return Ok(());
    }
    let Some((details, _)) = details_of(session) else {
        return Ok(());
    };
    let mut result = Ok(());
    world.games.run(world, None, |games, op| {
        let Some((gi, pi)) = games.current(session.id) else {
            return;
        };
        let game_id = games.games[gi].players[pi].game_id;
        let Some(gi) = games.game_by_id(game_id) else {
            return;
        };
        if games.games[gi].creator_id != details.user_id {
            return;
        }
        let Some(instance_id) = reader.int() else {
            result = Err(Malformed);
            return;
        };
        let game = &games.games[gi];
        let target = game
            .active_players()
            .find(|p| p.session.instance_id() == instance_id)
            .map(|p| (p.serial, p.session.clone()));
        let Some((serial, kicked)) = target else {
            return;
        };
        games.leave_game(gi, serial, op);
        op.send(&kicked, out::create_failed(CreateFailed::Kicked));
    });
    result
}

/// Java: `WATCHGAME`: spectate a game (sent to its arena if it has started).
pub fn watch_game(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if lobby_of(world, session).is_none() {
        return Ok(());
    }
    let Some((details, _)) = details_of(session) else {
        return Ok(());
    };
    let mut result = Ok(());
    world.games.run(world, None, |games, op| {
        if games.current(session.id).is_some() {
            return;
        }
        let Some(game_id) = reader.int() else {
            result = Err(Malformed);
            return;
        };
        let Some(gi) = games.game_by_id(game_id) else {
            return;
        };
        let serial = games.new_serial();
        let mut gp = GamePlayer::new(serial, session.clone(), details);
        gp.game_id = game_id;
        gp.spectator = true;
        games.set_current(session.id, Some(serial));
        let game = &mut games.games[gi];
        game.players.push(gp);
        game.spectators.push(serial);
        if game.state == GameState::Started {
            games.send_spectator_to_arena(gi, serial, op);
        } else {
            game.send_instance(op);
            game.send_observers_instance(op);
        }
    });
    result
}

/// Java: `STARTGAME`: the creator starts the waiting game.
pub fn start_game(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    if lobby_of(world, session).is_none() {
        return Ok(());
    }
    let Some((details, _)) = details_of(session) else {
        return Ok(());
    };
    world.games.run(world, None, |games, op| {
        let Some((gi, pi)) = games.current(session.id) else {
            return;
        };
        let game_id = games.games[gi].players[pi].game_id;
        let Some(gi) = games.game_by_id(game_id) else {
            return;
        };
        let game = &games.games[gi];
        if game.state != GameState::Waiting || game.creator_id != details.user_id {
            return;
        }
        if !game.can_game_start(op.world) {
            if game.game_type == GameType::SnowStorm && game.team_amount == 1 {
                op.send(
                    session,
                    alerts::alert("There needs to be at least two players to start this match"),
                );
            } else {
                op.send(session, out::start_failed_minimum_teams());
            }
            return;
        }
        games.start_game(gi, op);
    });
    Ok(())
}

/// Java: `GAMEEVENT`: a move or a power up in the arena (BattleBall), or SnowStorm's events.
pub fn game_event(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return Ok(());
    };
    if !r.entities[idx].walking_allowed {
        return Ok(());
    }
    let result = world.games.run(world, Some(&mut r), |games, op| {
        let Some((gi, Some(pi))) = games.current_game(session.id) else {
            return Ok(());
        };
        let game = &games.games[gi];
        let serial = game.players[pi].serial;
        if game.state != GameState::Started || !game.players[pi].in_game {
            return Ok(());
        }
        let event = reader.int().ok_or(Malformed)?;
        if matches!(game.kind, GameKind::SnowStorm(_)) {
            // Java: SnowStormMessageHandler.handleMessage(event, reader, game, player).
            games.snowstorm_event(gi, serial, event, reader);
            return Ok(());
        }
        match event {
            2 => {
                let Some(room) = op.room.as_deref_mut() else {
                    return Ok(());
                };
                let Some(idx) = room.entity_index(session.id) else {
                    return Ok(());
                };
                if !room.entities[idx].walking_allowed {
                    return Ok(());
                }
                let x = reader.int().ok_or(Malformed)?;
                let y = reader.int().ok_or(Malformed)?;
                room.walk_to_player(op.world, idx, x, y);
            }
            4 => {
                let power = reader.int().ok_or(Malformed)?;
                let game = &mut games.games[gi];
                let Some(room) = op.room.take() else {
                    return Ok(());
                };
                battleball::powerups::use_stored_power(game, serial, power, room, op);
                op.room = Some(room);
            }
            _ => {}
        }
        Ok(())
    });
    let deferred = r.take_deferred();
    drop(r);
    room::run_deferred(world, deferred);
    result
}

/// Java: `GAMERESTART`: "play again" on the score board; the game restarts when everyone in it
/// has clicked it.
pub fn game_restart(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    world.games.run(world, Some(&mut r), |games, op| {
        let Some((gi, Some(pi))) = games.current_game(session.id) else {
            return;
        };
        let game = &mut games.games[gi];
        if !game.game_finished || game.players[pi].clicked_restart {
            return;
        }
        game.players[pi].clicked_restart = true;
        let game = &games.games[gi];
        game.send(op, out::player_rejoined(session.instance_id()));
        if game.active_players().any(|p| !p.clicked_restart) {
            return;
        }
        // Everyone clicked it.
        let players = game.active_serials();
        let game = &mut games.games[gi];
        for &s in &players {
            if let Some(p) = game.player_mut(s) {
                p.clicked_restart = false;
            }
        }
        let minimum = op.world.settings.read().int(&format!(
            "{}.start.minimum.active.teams",
            game.game_type.key()
        ));
        let mut leaving = Vec::new();
        if players.len() as i64 >= minimum {
            games.restart_game(gi, &players, op);
        } else {
            leaving.extend(players);
        }
        let game = &games.games[gi];
        leaving.extend(game.spectators.iter().copied());
        let targets: Vec<Arc<Session>> = leaving
            .iter()
            .filter_map(|s| game.player(*s))
            .map(|p| p.session.clone())
            .collect();
        let (arena, game_type) = (game.room.clone(), game.game_type);
        for s in targets {
            crate::game::games::game::send_to_lobby(op, s, arena.clone(), game_type);
        }
    });
    let deferred = r.take_deferred();
    drop(r);
    room::run_deferred(world, deferred);
    Ok(())
}

/// Java: `REQUEST_GAME_LOBBY`: to a game lobby, the BattleBall or SnowStorm one at random the
/// first time, then the other one each time.
pub fn request_game_lobby(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    if !session.player.lock().logged_in {
        return Ok(());
    }
    let target = world.games.run(world, None, |games, _| {
        if games.current(session.id).is_some() {
            return None;
        }
        let next = match games.last_lobby(session.id) {
            None => {
                if rand::rng().random::<bool>() {
                    GameType::BattleBall
                } else {
                    GameType::SnowStorm
                }
            }
            Some(GameType::BattleBall) => GameType::SnowStorm,
            Some(_) => GameType::BattleBall,
        };
        games.set_last_lobby(session.id, next);
        Some(next)
    });
    if let Some(game_type) = target {
        if let Some(lobby) = world.games.lobby_room_id(world, game_type) {
            crate::game::moderation::forward_to(session, true, lobby);
        }
    }
    Ok(())
}

/// Java: `REQUESTFULLGAMESTATUS`: SnowStorm's full status.
pub fn request_full_game_status(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    if current_room(world, session).is_none() {
        return Ok(());
    }
    // Java throws for a game that never started (its preparing countdown is null); BattleBall
    // games get nothing.
    if let Some(status) = world.games.snowstorm_full_status(world, session) {
        session.send(status);
    }
    Ok(())
}
