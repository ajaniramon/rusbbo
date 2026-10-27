//! Who may do what in a private room: `rooms_rights`, `rooms_bans` and `users_room_votes`
//! (Java: `RoomRightsDao`, `RoomBanDao`, `RoomVoteDao`).
//!
//! Layout, from the `WHERE` clauses of those DAOs (all of them query by room first):
//! - `rooms_rights`: `(room_id, user_id) -> ()`. The MariaDB table has no key; the Java server
//!   only ever adds a row for a user who has none (`ASSIGNRIGHTS` checks the loaded list first).
//! - `rooms_bans`: `(room_id, user_id) -> expire_at`. Java deletes the old row before inserting.
//! - `users_room_votes`: `(room_id, user_id) -> RoomVote`. Java's `getRatings` groups by user.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const RIGHTS: TableDefinition<(u32, u32), ()> = TableDefinition::new("rooms_rights");
const BANS: TableDefinition<(u32, u32), i64> = TableDefinition::new("rooms_bans");
const VOTES: TableDefinition<(u32, u32), &[u8]> = TableDefinition::new("users_room_votes");

/// A row of `users_room_votes`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomVote {
    pub user_id: u32,
    pub room_id: u32,
    pub vote: i32,
    /// Unix seconds.
    pub expire_time: i64,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(RIGHTS)?;
    tx.open_table(BANS)?;
    tx.open_table(VOTES)?;
    Ok(())
}

impl Store {
    /// Java: `RoomRightsDao.getRoomRights`: the users with rights in a room, by user id.
    pub fn room_rights(&self, room_id: u32) -> Result<Vec<u32>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(RIGHTS)?;
        let mut out = Vec::new();
        for entry in table.range((room_id, 0)..=(room_id, u32::MAX))? {
            out.push(entry?.0.value().1);
        }
        Ok(out)
    }

    /// Java: `RoomRightsDao.addRights`.
    pub fn add_room_rights(&self, room_id: u32, user_id: u32) -> Result<()> {
        let mut w = self.write()?;
        w.put_room_rights(room_id, user_id)?;
        w.commit()
    }

    /// Java: `RoomRightsDao.removeRights`.
    pub fn remove_room_rights(&self, room_id: u32, user_id: u32) -> Result<()> {
        let w = self.write()?;
        w.tx.open_table(RIGHTS)?.remove((room_id, user_id))?;
        w.commit()
    }

    /// Java: `RoomRightsDao.deleteRoomRights`: everybody's rights in a room.
    pub fn delete_room_rights(&self, room_id: u32) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(RIGHTS)?;
            let users: Vec<u32> = table
                .range((room_id, 0)..=(room_id, u32::MAX))?
                .map(|e| e.map(|(k, _)| k.value().1))
                .collect::<std::result::Result<_, _>>()?;
            for user_id in users {
                table.remove((room_id, user_id))?;
            }
        }
        w.commit()
    }

    /// Java: `RoomBanDao.hasBan`: a ban of `user_id` from `room_id` that expires after `now`.
    pub fn has_room_ban(&self, room_id: u32, user_id: u32, now: i64) -> Result<bool> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(BANS)?;
        Ok(table
            .get((room_id, user_id))?
            .is_some_and(|expire_at| expire_at.value() > now))
    }

    /// Java: `RoomVoteDao.getRatings`: the votes of a room that expire after `now`, by user id.
    /// TODO(ip-log): Java joins `users_ip_logs` (a vote only counts if its user has logged IPs)
    /// and keeps the voter's IPs; that table is not stored yet, so the join is not applied.
    pub fn room_votes(&self, room_id: u32, now: i64) -> Result<Vec<RoomVote>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(VOTES)?;
        let mut out = Vec::new();
        for entry in table.range((room_id, 0)..=(room_id, u32::MAX))? {
            let vote: RoomVote = decode(entry?.1.value())?;
            if vote.expire_time > now {
                out.push(vote);
            }
        }
        Ok(out)
    }
}

impl Writer {
    /// Imports or adds one `rooms_rights` row.
    pub fn put_room_rights(&mut self, room_id: u32, user_id: u32) -> Result<()> {
        self.tx.open_table(RIGHTS)?.insert((room_id, user_id), ())?;
        Ok(())
    }

    /// Imports one `rooms_bans` row; of several rows for the same pair the latest expiry wins.
    pub fn put_room_ban(&mut self, room_id: u32, user_id: u32, expire_at: i64) -> Result<()> {
        let mut table = self.tx.open_table(BANS)?;
        let current = table.get((room_id, user_id))?.map(|v| v.value());
        if current.is_none_or(|c| expire_at > c) {
            table.insert((room_id, user_id), expire_at)?;
        }
        Ok(())
    }

    /// Imports one `users_room_votes` row.
    pub fn put_room_vote(&mut self, vote: &RoomVote) -> Result<()> {
        self.tx
            .open_table(VOTES)?
            .insert((vote.room_id, vote.user_id), encode(vote)?.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rights_bans_and_votes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        store.add_room_rights(1000, 2).unwrap();
        store.add_room_rights(1000, 1).unwrap();
        store.add_room_rights(1001, 3).unwrap();
        assert_eq!(store.room_rights(1000).unwrap(), [1, 2]);
        store.remove_room_rights(1000, 2).unwrap();
        assert_eq!(store.room_rights(1000).unwrap(), [1]);
        store.delete_room_rights(1000).unwrap();
        assert!(store.room_rights(1000).unwrap().is_empty());
        assert_eq!(store.room_rights(1001).unwrap(), [3]);

        let mut w = store.write().unwrap();
        w.put_room_ban(1000, 2, 50).unwrap();
        w.put_room_ban(1000, 2, 40).unwrap();
        w.put_room_vote(&RoomVote {
            user_id: 2,
            room_id: 1000,
            vote: 1,
            expire_time: 100,
        })
        .unwrap();
        w.commit().unwrap();
        assert!(store.has_room_ban(1000, 2, 49).unwrap());
        assert!(!store.has_room_ban(1000, 2, 50).unwrap());
        assert!(!store.has_room_ban(1000, 3, 0).unwrap());
        assert_eq!(store.room_votes(1000, 99).unwrap().len(), 1);
        assert!(store.room_votes(1000, 100).unwrap().is_empty());
    }
}
