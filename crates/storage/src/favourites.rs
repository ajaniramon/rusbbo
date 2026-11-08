//! `users_room_favourites` (Java: `RoomFavouritesDao`): the navigator's favourite rooms.
//!
//! Layout: `(user_id, room_id) -> rows`. The MariaDB table has no key, so an imported hotel may
//! hold the same pair more than once; the count keeps those rows (Java lists each of them).
//! Java's `SELECT room_id ... WHERE user_id = ?` reads Aeyra's `(user_id, room_id)` index, so its
//! rows come in room id order (`traces/navigator-favourites.json`), as this table's keys do.

use redb::{ReadableTable, TableDefinition, WriteTransaction};

use crate::{Result, Store, Writer};

const FAVOURITES: TableDefinition<(u32, u32), u32> = TableDefinition::new("users_room_favourites");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(FAVOURITES)?;
    Ok(())
}

impl Store {
    /// Java: `RoomFavouritesDao.getFavouriteRooms(userId, false)` before the `INNER JOIN rooms`:
    /// the user's favourite room ids, by room id, a pair stored twice listed twice.
    pub fn room_favourites(&self, user_id: u32) -> Result<Vec<u32>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(FAVOURITES)?;
        let mut out = Vec::new();
        for entry in table.range((user_id, 0)..=(user_id, u32::MAX))? {
            let (key, rows) = entry?;
            for _ in 0..rows.value() {
                out.push(key.value().1);
            }
        }
        Ok(out)
    }

    /// Java: `RoomFavouritesDao.addFavouriteRoom`.
    pub fn add_room_favourite(&self, user_id: u32, room_id: u32) -> Result<()> {
        let mut w = self.write()?;
        w.put_room_favourite(user_id, room_id)?;
        w.commit()
    }

    /// Java: `RoomFavouritesDao.removeFavouriteRoom`: every row of the pair.
    pub fn remove_room_favourite(&self, user_id: u32, room_id: u32) -> Result<()> {
        let w = self.write()?;
        w.tx.open_table(FAVOURITES)?.remove((user_id, room_id))?;
        w.commit()
    }
}

impl Writer {
    /// Imports or adds one `users_room_favourites` row.
    pub fn put_room_favourite(&mut self, user_id: u32, room_id: u32) -> Result<()> {
        let mut table = self.tx.open_table(FAVOURITES)?;
        let rows = table.get((user_id, room_id))?.map_or(0, |v| v.value());
        table.insert((user_id, room_id), rows + 1)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favourites_by_room_id_with_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        store.add_room_favourite(1, 1001).unwrap();
        store.add_room_favourite(1, 62).unwrap();
        store.add_room_favourite(1, 1000).unwrap();
        store.add_room_favourite(2, 5).unwrap();
        assert_eq!(store.room_favourites(1).unwrap(), [62, 1000, 1001]);
        let mut w = store.write().unwrap();
        w.put_room_favourite(1, 62).unwrap();
        w.commit().unwrap();
        assert_eq!(store.room_favourites(1).unwrap(), [62, 62, 1000, 1001]);
        store.remove_room_favourite(1, 62).unwrap();
        store.remove_room_favourite(1, 4321).unwrap();
        assert_eq!(store.room_favourites(1).unwrap(), [1000, 1001]);
        assert_eq!(store.room_favourites(2).unwrap(), [5]);
    }
}
