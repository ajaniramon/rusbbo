//! Java: `messages/outgoing/games` (the BattleBall and SnowStorm lobbies and arenas, see
//! `game::games`). The game-type parts of `GAMESTATUS`, `FULLGAMESTATUS` and `GAMERESET` are
//! built by the game types themselves (`game::games::battleball`).

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

/// Java: `LOUNGEINFO`.
pub fn lounge_info() -> Message {
    Message::new(231, ServerBody::new().int(0).build())
}

/// One player of `GAMEPLAYERINFO`: instance id, points and rank title.
pub struct PlayerInfo {
    pub instance_id: i32,
    pub points: i32,
    pub rank: String,
}

/// Java: `GAMEPLAYERINFO`.
pub fn game_player_info(players: &[PlayerInfo]) -> Message {
    let mut body = ServerBody::new().int(players.len() as i32);
    for p in players {
        body = body
            .int(p.instance_id)
            .text(&p.points.to_string())
            .text(&p.rank);
    }
    Message::new(250, body.build())
}

/// A game of `INSTANCELIST`. `game_length` is written for SnowStorm games only.
pub struct ListedGame {
    pub id: i32,
    pub name: String,
    pub creator_id: u32,
    pub creator_name: String,
    pub game_length: Option<i32>,
    pub map_id: i32,
}

/// Java: `INSTANCELIST`: the waiting games (with the creator's id), the started ones and the
/// finished ones (Java's `GameHistory`, no creator id).
pub fn instance_list(
    created: &[ListedGame],
    started: &[ListedGame],
    finished: &[ListedGame],
) -> Message {
    let mut body = ServerBody::new().int(created.len() as i32);
    for g in created {
        body = body
            .int(g.id)
            .text(&g.name)
            .int(g.creator_id as i32)
            .text(&g.creator_name);
        if let Some(length) = g.game_length {
            body = body.int(length);
        }
        body = body.int(g.map_id);
    }
    for list in [started, finished] {
        body = body.int(list.len() as i32);
        for g in list {
            body = body.int(g.id).text(&g.name).text(&g.creator_name);
            if let Some(length) = g.game_length {
                body = body.int(length);
            }
            body = body.int(g.map_id);
        }
    }
    Message::new(232, body.build())
}

/// What `GAMEINSTANCE` shows of a game.
pub enum InstanceView {
    /// A waiting game: its teams with each player's instance id (in their room) and name.
    Waiting {
        id: i32,
        name: String,
        creator_id: u32,
        creator_name: String,
        length_choice: Option<i32>,
        map_id: i32,
        spectators: i32,
        teams: Vec<Vec<(i32, String)>>,
        power_ups: Option<String>,
    },
    /// A started game: the names of the players still in it.
    Started {
        id: i32,
        name: String,
        creator_name: String,
        length_choice: Option<i32>,
        map_id: i32,
        teams: Vec<Vec<String>>,
        power_ups: Option<String>,
    },
    /// A game that ended but is still live (Java writes only its state).
    Ended,
    /// A finished game (Java's `GameHistory`): names and scores per team, and each team's total.
    Finished {
        id: i32,
        name: String,
        creator_name: String,
        length_choice: Option<i32>,
        map_id: i32,
        teams: Vec<Vec<(String, i32)>>,
        power_ups: Option<String>,
    },
}

/// Java: `GAMEINSTANCE`.
pub fn game_instance(view: &InstanceView) -> Message {
    let body = match view {
        InstanceView::Waiting {
            id,
            name,
            creator_id,
            creator_name,
            length_choice,
            map_id,
            spectators,
            teams,
            power_ups,
        } => {
            let mut body = ServerBody::new()
                .int(0)
                .int(*id)
                .text(name)
                .int(*creator_id as i32)
                .text(creator_name);
            if let Some(choice) = length_choice {
                body = body.int(*choice);
            }
            body = body.int(*map_id).int(*spectators).int(teams.len() as i32);
            for team in teams {
                body = body.int(team.len() as i32);
                for (instance_id, name) in team {
                    body = body.int(*instance_id).text(name);
                }
            }
            if let Some(power_ups) = power_ups {
                body = body.text(power_ups);
            }
            body
        }
        InstanceView::Started {
            id,
            name,
            creator_name,
            length_choice,
            map_id,
            teams,
            power_ups,
        } => {
            let mut body = ServerBody::new()
                .int(1)
                .int(*id)
                .text(name)
                .text(creator_name);
            if let Some(choice) = length_choice {
                body = body.int(*choice);
            }
            body = body.int(*map_id).int(teams.len() as i32);
            for team in teams {
                body = body.int(team.len() as i32);
                for name in team {
                    body = body.text(name);
                }
            }
            if let Some(power_ups) = power_ups {
                body = body.text(power_ups);
            }
            body
        }
        InstanceView::Ended => ServerBody::new().int(2),
        InstanceView::Finished {
            id,
            name,
            creator_name,
            length_choice,
            map_id,
            teams,
            power_ups,
        } => {
            let mut body = ServerBody::new()
                .int(2)
                .int(*id)
                .text(name)
                .text(creator_name);
            if let Some(choice) = length_choice {
                body = body.int(*choice);
            }
            body = body.int(*map_id).int(teams.len() as i32);
            for team in teams {
                body = body.int(team.len() as i32);
                for (name, score) in team {
                    body = body.text(name).int(*score);
                }
                body = body.int(team.iter().map(|(_, s)| s).sum());
            }
            if let Some(power_ups) = power_ups {
                body = body.text(power_ups);
            }
            body
        }
    };
    Message::new(233, body.build())
}

/// Java: `GameParameter`.
pub struct GameParameter {
    pub name: &'static str,
    pub editable: bool,
    pub default_value: &'static str,
    /// `(min, max)`, for the numeric ones.
    pub min_max: Option<(i32, i32)>,
}

/// Java: `GAMEPARAMETERS`.
pub fn game_parameters(parameters: &[GameParameter]) -> Message {
    let mut body = ServerBody::new().int(parameters.len() as i32);
    for p in parameters {
        body = body
            .text(p.name)
            .bool(p.min_max.is_none())
            .int(if p.editable { 2 } else { 0 });
        match p.min_max {
            Some((min, max)) => {
                // Java: Integer.parseInt of the default.
                body = body.int(p.default_value.parse().unwrap_or(0));
                if min != -1 {
                    body = body.bool(true).int(min);
                }
                if max != -1 {
                    body = body.bool(true).int(max);
                }
            }
            None => body = body.text(p.default_value).int(0),
        }
    }
    Message::new(235, body.build())
}

/// Java: `CREATEFAILED.FailedReason`.
#[derive(Clone, Copy)]
pub enum CreateFailed {
    Kicked = 6,
    TicketsNeeded = 2,
}

/// Java: `CREATEFAILED`.
pub fn create_failed(reason: CreateFailed) -> Message {
    Message::new(236, ServerBody::new().int(reason as i32).build())
}

/// Java: `GAMEDELETED`.
pub fn game_deleted(game_id: i32) -> Message {
    Message::new(237, ServerBody::new().int(game_id).build())
}

/// Java: `JOINFAILED.FailedReason`.
#[derive(Clone, Copy)]
pub enum JoinFailed {
    TicketsNeeded = 2,
    TeamsFull = 0,
}

/// Java: `JOINFAILED`.
pub fn join_failed(reason: JoinFailed, key: Option<&str>) -> Message {
    let mut body = ServerBody::new().int(reason as i32);
    if let Some(key) = key {
        body = body.text(key);
    }
    Message::new(239, body.build())
}

/// Java: `GAMELOCATION`: the client goes to the arena (`ROOM_DIRECTORY` with room -1).
pub fn game_location() -> Message {
    Message::new(241, ServerBody::new().int(-1).build())
}

/// Java: `STARTFAILED` with `MINIMUM_TEAMS_REQUIRED`.
pub fn start_failed_minimum_teams() -> Message {
    Message::new(242, ServerBody::new().int(8).build())
}

/// Java: `PLAYERREJOINED`.
pub fn player_rejoined(instance_id: i32) -> Message {
    Message::new(245, ServerBody::new().int(instance_id).build())
}

/// Java: `GAMESTART`: the game's length in seconds.
pub fn game_start(seconds: i32) -> Message {
    Message::new(247, ServerBody::new().int(seconds).build())
}

/// One team of `GAMEEND`: its players (object id, name, score) and points.
pub struct EndTeam {
    pub players: Vec<(i32, String, i32)>,
    pub points: i32,
}

/// Java: `GAMEEND`: the seconds until the restart, then each team (its points only when it has
/// players).
pub fn game_end(restart_seconds: i32, teams: &[EndTeam]) -> Message {
    let mut body = ServerBody::new()
        .int(restart_seconds)
        .int(teams.len() as i32);
    for team in teams {
        body = body.int(team.players.len() as i32);
        if !team.players.is_empty() {
            for (object_id, name, score) in &team.players {
                body = body.int(*object_id).text(name).int(*score);
            }
            body = body.int(team.points);
        }
    }
    Message::new(248, body.build())
}

/// Java: `YOUARESPECTATOR`.
pub fn you_are_spectator() -> Message {
    Message::new(254, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_match_the_traces() {
        // traces/battleball-lobby.json
        assert_eq!(lounge_info().body, b"H");
        let info = PlayerInfo {
            instance_id: 0,
            points: 0,
            rank: "Beginner".into(),
        };
        assert_eq!(game_player_info(&[info]).body, b"IH0\x02Beginner\x02");
        assert_eq!(instance_list(&[], &[], &[]).body, b"HHH");
        let game = ListedGame {
            id: 1,
            name: "Trace match".into(),
            creator_id: 1,
            creator_name: "tracebot1".into(),
            game_length: None,
            map_id: 1,
        };
        assert_eq!(
            instance_list(&[game], &[], &[]).body,
            b"IITrace match\x02Itracebot1\x02IHH"
        );
        let view = InstanceView::Waiting {
            id: 1,
            name: "Trace match".into(),
            creator_id: 1,
            creator_name: "tracebot1".into(),
            length_choice: None,
            map_id: 1,
            spectators: 0,
            teams: vec![vec![(0, "tracebot1".into())], vec![]],
            power_ups: Some("1,2,3,4,5,6,7,8,9".into()),
        };
        assert_eq!(
            game_instance(&view).body,
            b"HITrace match\x02Itracebot1\x02IHJIHtracebot1\x02H1,2,3,4,5,6,7,8,9\x02"
        );
        assert_eq!(game_instance(&InstanceView::Ended).body, b"J");
        assert_eq!(create_failed(CreateFailed::Kicked).body, b"RA");
        assert_eq!(
            join_failed(JoinFailed::TeamsFull, Some("join")).body,
            b"Hjoin\x02"
        );
        assert_eq!(start_failed_minimum_teams().body, b"PB");
        assert_eq!(game_location().body, b"M");
        // traces/battleball-game.json
        let teams = [
            EndTeam {
                players: vec![(1, "tracebot1".into(), 24)],
                points: 24,
            },
            EndTeam {
                players: vec![(2, "tracebot2".into(), 4)],
                points: 4,
            },
        ];
        assert_eq!(
            game_end(30, &teams).body,
            b"RGJIItracebot1\x02PFPFIJtracebot2\x02PAPA"
        );
    }
}
