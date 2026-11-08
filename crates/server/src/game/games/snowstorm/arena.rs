//! Where the engine meets the game core: the seam's SnowStorm arms (`game.rs`), the arena task,
//! `GAMEEVENT`, `G_HMAP` and `REQUESTFULLGAMESTATUS`.

use std::sync::Arc;

use havana_protocol::Message;
use havana_protocol::body::ClientReader;
use rand::RngExt;

use super::game::{Output, PlayerInfo, RosterEntry, SnowStormGame};
use super::outgoing;
use crate::game::games::game::preparing_seconds;
use crate::game::games::{Game, GameKind, GameManager, GameType, Games};
use crate::game::pathfinder::Position;
use crate::game::room::Room;
use crate::game::room::model::RoomModel;
use crate::session::Session;
use crate::world::World;

/// The engine's clock (Java: `System.currentTimeMillis`).
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Java: `SnowStormGame.initialise`'s `new RoomModel("snowwar_arena_0", "snowwar_arena_0", 0, 0,
/// 0, 0, getHeightMap(mapId), null)`.
pub fn arena_model(map_id: i32) -> Arc<RoomModel> {
    Arc::new(RoomModel::parse(&havana_storage::RoomModel {
        model_id: "snowwar_arena_0".into(),
        model_name: "snowwar_arena_0".into(),
        door_x: 0,
        door_y: 0,
        door_z: 0.0,
        door_dir: 0,
        heightmap: super::maps::raw_heightmap(map_id)
            .trim_end_matches('|')
            .to_string(),
        ..Default::default()
    }))
}

/// The game's team members, team by team, for the engine (`in_room`: in `room`).
fn roster(game: &Game, room: Option<&Room>) -> Vec<RosterEntry> {
    game.teams
        .iter()
        .flat_map(|t| t.players.iter())
        .filter_map(|s| game.player(*s))
        .map(|p| RosterEntry {
            info: PlayerInfo {
                user_id: p.user_id,
                team_id: p.team_id,
                name: p.name.clone(),
                motto: p.motto.clone(),
                figure: p.figure.clone(),
                sex: p.sex.clone(),
            },
            in_game: p.in_game,
            in_room: room.is_some_and(|r| r.entity_index(p.session.id).is_some()),
        })
        .collect()
}

/// Brings the engine's roster up to date and returns it.
fn synced<'a>(game: &'a mut Game, room: Option<&Room>) -> Option<&'a mut SnowStormGame> {
    let roster = roster(game, room);
    match &mut game.kind {
        GameKind::SnowStorm(engine) => {
            engine.sync_roster(roster);
            Some(engine)
        }
        GameKind::BattleBall(_) => None,
    }
}

/// Java: `SnowStormGame.assignSpawnPoints`: the engine places everyone; each game player gets
/// its object id and spawn (and a score of 0).
pub(crate) fn assign_spawn_points(game: &mut Game) {
    let Some(engine) = synced(game, None) else {
        return;
    };
    let mut rng = rand::rng();
    let spawns = engine.assign_spawn_points(&mut |n| rng.random_range(0..n), now_ms());
    let members: Vec<u64> = game.total_serials();
    for s in spawns {
        let serial = members
            .iter()
            .copied()
            .find(|&serial| game.player(serial).is_some_and(|p| p.user_id == s.user_id));
        if let Some(gp) = serial.and_then(|serial| game.player_mut(serial)) {
            gp.object_id = s.object_id;
            gp.spawn = Position::new(s.spawn.x, s.spawn.y);
            gp.assigned_spawn = true;
            gp.score = 0;
        }
    }
}

/// Java: `SnowStormGame.finishGame`'s override: each active player's score is its SnowStorm
/// score, before the teams are summed.
pub(crate) fn set_final_scores(game: &mut Game) {
    let Some(engine) = synced(game, None) else {
        return;
    };
    let scores = engine.final_scores();
    for serial in game.active_serials() {
        let Some(gp) = game.player_mut(serial) else {
            continue;
        };
        if let Some((_, score)) = scores.iter().find(|(u, _)| *u == gp.user_id) {
            gp.score = *score;
        }
    }
}

/// Java: `FULLGAMESTATUS` of a SnowStorm game.
pub(crate) fn full_game_status(game: &Game, engine: &SnowStormGame, world: &World) -> Message {
    engine.full_game_status(
        game.state as i32,
        game.preparing_seconds_left,
        preparing_seconds(world, GameType::SnowStorm),
        now_ms(),
    )
}

/// Java: `GAMERESET` of a SnowStorm game.
pub(crate) fn game_reset(engine: &SnowStormGame, world: &World) -> Message {
    engine.game_reset(preparing_seconds(world, GameType::SnowStorm), now_ms())
}

impl Games {
    /// Java: `SnowStormGameTask.run` for the arena `room` of the game `gi`. `first` is the first
    /// run of the room's task, which Java's `loadGameTasks` makes a new `SnowStormGameTask`
    /// (with nothing queued).
    pub(crate) fn snowstorm_arena_tick(&mut self, gi: usize, room: &mut Room, first: bool) {
        let game = &mut self.games[gi];
        let sessions: Vec<(u32, u64)> = game
            .total_serials()
            .into_iter()
            .filter_map(|s| game.player(s))
            .map(|p| (p.user_id, p.session.id))
            .collect();
        let Some(engine) = synced(game, Some(room)) else {
            return;
        };
        if first {
            engine.start_external_turns();
        }
        for output in engine.tick(now_ms()) {
            match output {
                Output::SpamTick(user_id) => {
                    let session = sessions.iter().find(|(u, _)| *u == user_id);
                    if let Some(idx) = session.and_then(|(_, s)| room.entity_index(*s)) {
                        room.entities[idx].handle_spam_ticks();
                    }
                }
                Output::Broadcast(msg) => room.send(msg),
            }
        }
    }

    /// Java: `SnowStormMessageHandler.handleMessage` for the game player `serial` of game `gi`.
    pub(crate) fn snowstorm_event(
        &mut self,
        gi: usize,
        serial: u64,
        event: i32,
        reader: &mut ClientReader,
    ) {
        let game = &mut self.games[gi];
        let Some(user_id) = game.player(serial).map(|p| p.user_id) else {
            return;
        };
        let Some(engine) = synced(game, None) else {
            return;
        };
        engine.handle_event(user_id, event, reader, true, now_ms());
    }
}

impl GameManager {
    /// Java: `G_HMAP` for a player of a SnowStorm game in its arena: the map's heightmap, the
    /// full status and the arena's objects. `None` when that is not the case (the room's own
    /// heightmap then); empty when the map is missing (Java throws).
    pub fn snowstorm_arena_view(&self, world: &World, session: &Session) -> Option<Vec<Message>> {
        let games = self.state.lock();
        let (gi, _) = games.current_game(session.id)?;
        let game = &games.games[gi];
        let GameKind::SnowStorm(engine) = &game.kind else {
            return None;
        };
        let arena = game.room.as_ref()?.id;
        if session.room_key() != Some(arena) {
            return None;
        }
        let Some(map) = engine.map() else {
            return Some(Vec::new());
        };
        let mut out = vec![
            outgoing::heightmap(map),
            full_game_status(game, engine, world),
        ];
        out.extend(outgoing::objects_world(map, outgoing::random_object_id));
        Some(out)
    }

    /// Java: `REQUESTFULLGAMESTATUS`: the full status of the player's SnowStorm game, once it
    /// was started (before, Java throws on its preparing countdown).
    pub fn snowstorm_full_status(&self, world: &World, session: &Session) -> Option<Message> {
        let games = self.state.lock();
        let (gi, pi) = games.current(session.id)?;
        let game_id = games.games[gi].players[pi].game_id;
        let game = &games.games[games.game_by_id(game_id)?];
        let GameKind::SnowStorm(engine) = &game.kind else {
            return None;
        };
        game.model.as_ref()?;
        Some(full_game_status(game, engine, world))
    }
}
