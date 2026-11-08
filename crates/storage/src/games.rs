//! The game tables (Java: `GameDao`): `games_maps` (the BattleBall arenas' heightmaps and tile
//! maps), `games_player_spawns`, `games_ranks` and `games_played_history`.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

/// Keyed by `games_maps.id`.
const GAMES_MAPS: TableDefinition<u32, &[u8]> = TableDefinition::new("games_maps");
/// Keyed by the row's position in the table: Java reads `SELECT *` (no key, insertion order) and
/// uses the first free spawn in that order.
const GAMES_SPAWNS: TableDefinition<u32, &[u8]> = TableDefinition::new("games_player_spawns");
/// Keyed by `games_ranks.id`.
const GAMES_RANKS: TableDefinition<u32, &[u8]> = TableDefinition::new("games_ranks");
/// Keyed by insertion order (the table has no key; Java orders by `played_at`, see
/// [`Store::last_played_games`]).
const GAMES_HISTORY: TableDefinition<u64, &[u8]> = TableDefinition::new("games_played_history");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(GAMES_MAPS)?;
    tx.open_table(GAMES_SPAWNS)?;
    tx.open_table(GAMES_RANKS)?;
    tx.open_table(GAMES_HISTORY)?;
    Ok(())
}

/// A row of `games_maps`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameMap {
    pub id: u32,
    /// `battleball` or `snowstorm`.
    pub game_type: String,
    pub map_id: i32,
    /// Rows separated by `|`.
    pub heightmap: String,
    /// Rows separated by `|`: `1` for a tile of the game.
    pub tile_map: String,
}

/// A row of `games_player_spawns`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSpawn {
    /// `battleball` or `snowstorm`.
    pub game_type: String,
    pub map_id: i32,
    pub team_id: i32,
    pub x: i32,
    pub y: i32,
    pub rotation: i32,
}

/// A row of `games_ranks`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRank {
    pub id: u32,
    /// `battleball` or `snowstorm`.
    pub game_type: String,
    pub title: String,
    pub min_points: i32,
    pub max_points: i32,
}

/// A row of `games_played_history`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayedGame {
    /// A random UUID.
    pub id: String,
    pub game_name: String,
    /// The creator's user id.
    pub game_creator: u32,
    /// `BATTLEBALL` or `SNOWSTORM` (Java's `GameType.name()`).
    pub game_type: String,
    pub map_id: i32,
    pub winning_team: i32,
    pub winning_team_score: i32,
    pub extra_data: String,
    /// Java's `GameHistoryData` as Gson writes it: `{"teamCount":..,"players":[..]}`.
    pub team_data: String,
    /// Unix seconds.
    pub played_at: i64,
}

impl Store {
    /// Java: `GameDao.getGameMaps` / `getBattleballTileMaps` (`SELECT * FROM games_maps`).
    pub fn game_maps(&self) -> Result<Vec<GameMap>> {
        self.read_all(GAMES_MAPS)
    }

    /// Java: `GameDao.getGameSpawns`, in table order.
    pub fn game_spawns(&self) -> Result<Vec<GameSpawn>> {
        self.read_all(GAMES_SPAWNS)
    }

    /// Java: `GameDao.getRanks`.
    pub fn game_ranks(&self) -> Result<Vec<GameRank>> {
        self.read_all(GAMES_RANKS)
    }

    /// Java: `GameDao.getLastPlayedGames` without the join: the games of `game_type`, latest
    /// first (`ORDER BY played_at DESC LIMIT limit`; rows of the same second come latest first,
    /// as the `(game_type, played_at)` index is read backwards).
    pub fn last_played_games(&self, game_type: &str, limit: usize) -> Result<Vec<PlayedGame>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(GAMES_HISTORY)?;
        let mut rows: Vec<(u64, PlayedGame)> = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            let row: PlayedGame = decode(value.value())?;
            if row.game_type == game_type {
                rows.push((key.value(), row));
            }
        }
        rows.sort_by_key(|a| std::cmp::Reverse((a.1.played_at, a.0)));
        Ok(rows.into_iter().take(limit).map(|(_, r)| r).collect())
    }

    /// Java: `GameDao.saveTeamHistory`.
    pub fn add_played_game(&self, row: &PlayedGame) -> Result<()> {
        let mut w = self.write()?;
        w.push_played_game(row)?;
        w.commit()
    }

    fn read_all<K: redb::Key + 'static, T: serde::de::DeserializeOwned>(
        &self,
        def: TableDefinition<K, &[u8]>,
    ) -> Result<Vec<T>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(def)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }
}

impl Writer {
    pub fn put_game_map(&mut self, map: &GameMap) -> Result<()> {
        let bytes = encode(map)?;
        self.tx
            .open_table(GAMES_MAPS)?
            .insert(map.id, bytes.as_slice())?;
        Ok(())
    }

    /// Appends a spawn after the ones stored.
    pub fn push_game_spawn(&mut self, spawn: &GameSpawn) -> Result<()> {
        let bytes = encode(spawn)?;
        let mut table = self.tx.open_table(GAMES_SPAWNS)?;
        let next = table.last()?.map_or(0, |(k, _)| k.value() + 1);
        table.insert(next, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_game_rank(&mut self, rank: &GameRank) -> Result<()> {
        let bytes = encode(rank)?;
        self.tx
            .open_table(GAMES_RANKS)?
            .insert(rank.id, bytes.as_slice())?;
        Ok(())
    }

    /// Appends a played game after the ones stored.
    pub fn push_played_game(&mut self, row: &PlayedGame) -> Result<()> {
        let bytes = encode(row)?;
        let mut table = self.tx.open_table(GAMES_HISTORY)?;
        let next = table.last()?.map_or(0, |(k, _)| k.value() + 1);
        table.insert(next, bytes.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns_keep_their_order_and_history_is_latest_first() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        for x in [5, 3, 9] {
            w.push_game_spawn(&GameSpawn {
                game_type: "battleball".into(),
                map_id: 1,
                x,
                ..Default::default()
            })
            .unwrap();
        }
        w.commit().unwrap();
        let xs: Vec<i32> = store.game_spawns().unwrap().iter().map(|s| s.x).collect();
        assert_eq!(xs, [5, 3, 9]);

        let game = |name: &str, game_type: &str, played_at| PlayedGame {
            game_name: name.into(),
            game_type: game_type.into(),
            played_at,
            ..Default::default()
        };
        store
            .add_played_game(&game("old", "BATTLEBALL", 10))
            .unwrap();
        store
            .add_played_game(&game("snow", "SNOWSTORM", 30))
            .unwrap();
        store
            .add_played_game(&game("same1", "BATTLEBALL", 20))
            .unwrap();
        store
            .add_played_game(&game("same2", "BATTLEBALL", 20))
            .unwrap();
        let names: Vec<String> = store
            .last_played_games("BATTLEBALL", 15)
            .unwrap()
            .into_iter()
            .map(|g| g.game_name)
            .collect();
        assert_eq!(names, ["same2", "same1", "old"]);
        assert_eq!(store.last_played_games("BATTLEBALL", 1).unwrap().len(), 1);
    }
}
