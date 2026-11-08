//! `games_maps`, `games_player_spawns`, `games_ranks` and `games_played_history` (Java: `GameDao`).

use anyhow::Result;
use havana_storage::{GameMap, GameRank, GameSpawn, PlayedGame, Store};
use mysql::Row;
use mysql::prelude::Queryable;

use crate::col;

/// Returns the number of rows copied.
pub fn import_games(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    // `map_id` is an ENUM of digits: read it as text.
    let maps: Vec<Row> = conn.query(
        "SELECT id, game_type, CAST(map_id AS CHAR) AS map_id, heightmap, tile_map \
         FROM games_maps ORDER BY id",
    )?;
    // No key: Java reads the rows in table order, which is the order the spawns are tried in.
    let spawns: Vec<Row> =
        conn.query("SELECT type, map_id, team_id, x, y, rotation FROM games_player_spawns")?;
    let ranks: Vec<Row> =
        conn.query("SELECT id, type, title, min_points, max_points FROM games_ranks ORDER BY id")?;
    let history: Vec<Row> = conn.query(
        "SELECT id, game_name, game_creator, game_type, map_id, winning_team, \
         winning_team_score, extra_data, team_data, \
         CAST(UNIX_TIMESTAMP(played_at) AS SIGNED) AS played_ts FROM games_played_history \
         ORDER BY played_at",
    )?;
    let mut w = store.write()?;
    for row in &maps {
        let map_id: String = col(row, "map_id")?;
        w.put_game_map(&GameMap {
            id: col(row, "id")?,
            game_type: col(row, "game_type")?,
            map_id: map_id.parse().unwrap_or(0),
            heightmap: col(row, "heightmap")?,
            tile_map: col(row, "tile_map")?,
        })?;
    }
    for row in &spawns {
        w.push_game_spawn(&GameSpawn {
            game_type: col(row, "type")?,
            map_id: col(row, "map_id")?,
            team_id: col(row, "team_id")?,
            x: col(row, "x")?,
            y: col(row, "y")?,
            rotation: col(row, "rotation")?,
        })?;
    }
    for row in &ranks {
        w.put_game_rank(&GameRank {
            id: col(row, "id")?,
            game_type: col(row, "type")?,
            title: col(row, "title")?,
            min_points: col(row, "min_points")?,
            max_points: col(row, "max_points")?,
        })?;
    }
    for row in &history {
        let played_at: Option<i64> = col(row, "played_ts")?;
        w.push_played_game(&PlayedGame {
            id: col(row, "id")?,
            game_name: col(row, "game_name")?,
            game_creator: col(row, "game_creator")?,
            game_type: col(row, "game_type")?,
            map_id: col(row, "map_id")?,
            winning_team: col(row, "winning_team")?,
            winning_team_score: col(row, "winning_team_score")?,
            extra_data: col(row, "extra_data")?,
            team_data: col(row, "team_data")?,
            played_at: played_at.unwrap_or(0),
        })?;
    }
    w.commit()?;
    Ok(maps.len() + spawns.len() + ranks.len() + history.len())
}
