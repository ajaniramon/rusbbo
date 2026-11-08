//! `room_chatlogs` (Java: `RoomDao.saveChatLog`, `RoomDao.getModChatlog`, `LogDao.deleteChatLogs`):
//! what was said in rooms, for the moderators.
//!
//! Layout: `id -> ChatLog` (the MariaDB `AUTO_INCREMENT` id), plus `room_chatlogs_by_time:
//! (timestamp, id) -> ()` for the age-based purge and `room_chatlogs_by_room: (room_id, timestamp,
//! id) -> ()` for a room's latest lines.

use redb::{ReadableTable, ReadableTableMetadata, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const CHATLOGS: TableDefinition<u32, &[u8]> = TableDefinition::new("room_chatlogs");
const BY_TIME: TableDefinition<(i64, u32), ()> = TableDefinition::new("room_chatlogs_by_time");
const BY_ROOM: TableDefinition<(u32, i64, u32), ()> = TableDefinition::new("room_chatlogs_by_room");

/// A row of `room_chatlogs`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatLog {
    pub id: u32,
    pub user_id: u32,
    pub room_id: u32,
    /// Unix seconds.
    pub timestamp: i64,
    /// 0 chat, 1 shout, 2 whisper.
    pub chat_type: u8,
    pub message: String,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(CHATLOGS)?;
    tx.open_table(BY_TIME)?;
    tx.open_table(BY_ROOM)?;
    Ok(())
}

impl Store {
    /// Java: `RoomDao.saveChatLog`: the queued lines in one transaction, each with the next id
    /// (`id` is ignored).
    pub fn save_chat_logs(&self, logs: &[ChatLog]) -> Result<()> {
        if logs.is_empty() {
            return Ok(());
        }
        let mut w = self.write()?;
        let first = {
            let table = w.tx.open_table(CHATLOGS)?;
            table.last()?.map_or(1, |(id, _)| id.value() + 1)
        };
        for (id, log) in (first..).zip(logs) {
            w.put_chat_log(&ChatLog { id, ..log.clone() })?;
        }
        w.commit()
    }

    /// Java: `RoomDao.getModChatlog` (`ORDER BY timestamp DESC LIMIT 150`) without the joins:
    /// the latest `limit` lines of a room, newest first.
    pub fn room_chat_logs(&self, room_id: u32, limit: usize) -> Result<Vec<ChatLog>> {
        let tx = self.db.begin_read()?;
        let index = tx.open_table(BY_ROOM)?;
        let table = tx.open_table(CHATLOGS)?;
        let mut out = Vec::new();
        for entry in index
            .range((room_id, i64::MIN, 0)..=(room_id, i64::MAX, u32::MAX))?
            .rev()
        {
            if out.len() == limit {
                break;
            }
            let id = entry?.0.value().2;
            if let Some(bytes) = table.get(id)? {
                out.push(decode(bytes.value())?);
            }
        }
        Ok(out)
    }

    /// Every chat line, in id order (the housekeeping panel).
    pub fn all_chat_logs(&self) -> Result<Vec<ChatLog>> {
        let tx = self.db.begin_read()?;
        let mut out = Vec::new();
        for entry in tx.open_table(CHATLOGS)?.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    pub fn chat_log_count(&self) -> Result<u64> {
        let tx = self.db.begin_read()?;
        Ok(tx.open_table(CHATLOGS)?.len()?)
    }

    /// Java: `LogDao.deleteChatLogs`: the lines older than `cutoff` (`timestamp < cutoff`).
    /// Returns how many went.
    pub fn delete_chat_logs_before(&self, cutoff: i64) -> Result<usize> {
        let w = self.write()?;
        let old: Vec<(i64, u32)> = {
            let index = w.tx.open_table(BY_TIME)?;
            index
                .range(..(cutoff, 0))?
                .map(|e| e.map(|(k, _)| k.value()))
                .collect::<std::result::Result<_, _>>()?
        };
        {
            let mut table = w.tx.open_table(CHATLOGS)?;
            let mut by_time = w.tx.open_table(BY_TIME)?;
            let mut by_room = w.tx.open_table(BY_ROOM)?;
            for (timestamp, id) in &old {
                if let Some(bytes) = table.remove(*id)? {
                    let log: ChatLog = decode(bytes.value())?;
                    by_room.remove((log.room_id, log.timestamp, log.id))?;
                }
                by_time.remove((*timestamp, *id))?;
            }
        }
        w.commit()?;
        Ok(old.len())
    }
}

impl Writer {
    /// Imports or adds one `room_chatlogs` row with its id.
    pub fn put_chat_log(&mut self, log: &ChatLog) -> Result<()> {
        let old: Option<ChatLog> = {
            let table = self.tx.open_table(CHATLOGS)?;
            table.get(log.id)?.map(|v| decode(v.value())).transpose()?
        };
        if let Some(old) = old {
            self.tx
                .open_table(BY_TIME)?
                .remove((old.timestamp, old.id))?;
            self.tx
                .open_table(BY_ROOM)?
                .remove((old.room_id, old.timestamp, old.id))?;
        }
        self.tx
            .open_table(CHATLOGS)?
            .insert(log.id, encode(log)?.as_slice())?;
        self.tx
            .open_table(BY_TIME)?
            .insert((log.timestamp, log.id), ())?;
        self.tx
            .open_table(BY_ROOM)?
            .insert((log.room_id, log.timestamp, log.id), ())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(room_id: u32, timestamp: i64, message: &str) -> ChatLog {
        ChatLog {
            user_id: 1,
            room_id,
            timestamp,
            message: message.into(),
            ..Default::default()
        }
    }

    #[test]
    fn saving_reading_and_purging() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        store.save_chat_logs(&[]).unwrap();
        store
            .save_chat_logs(&[
                line(1000, 10, "a"),
                line(1001, 11, "b"),
                line(1000, 12, "c"),
            ])
            .unwrap();
        store.save_chat_logs(&[line(1000, 13, "d")]).unwrap();
        let ids: Vec<u32> = store
            .room_chat_logs(1000, 150)
            .unwrap()
            .iter()
            .map(|l| l.id)
            .collect();
        assert_eq!(ids, [4, 3, 1]);
        assert_eq!(store.room_chat_logs(1000, 2).unwrap()[1].message, "c");
        assert_eq!(store.delete_chat_logs_before(12).unwrap(), 2);
        assert_eq!(store.chat_log_count().unwrap(), 2);
        let left: Vec<String> = store
            .room_chat_logs(1000, 150)
            .unwrap()
            .into_iter()
            .map(|l| l.message)
            .collect();
        assert_eq!(left, ["d", "c"]);
        assert!(store.room_chat_logs(1001, 150).unwrap().is_empty());
        // The next id carries on after the highest one.
        store.save_chat_logs(&[line(1001, 20, "e")]).unwrap();
        assert_eq!(store.room_chat_logs(1001, 1).unwrap()[0].id, 5);
    }
}
