//! `rooms_events` (Java: `EventsDao`): the room events hosts run from the navigator.
//!
//! Layout: `room_id -> RoomEvent`, as the MariaDB table's primary key: one event per room.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const EVENTS: TableDefinition<u32, &[u8]> = TableDefinition::new("rooms_events");

/// A row of `rooms_events`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomEvent {
    pub room_id: u32,
    pub user_id: u32,
    pub category_id: i32,
    pub name: String,
    pub description: String,
    /// Unix seconds.
    pub expire_time: i64,
    /// Comma-separated (only Flash clients set tags).
    pub tags: String,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(EVENTS)?;
    Ok(())
}

impl Store {
    /// Java: `EventsDao.getEvents`: every stored event, by room id.
    pub fn room_events(&self) -> Result<Vec<RoomEvent>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(EVENTS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// Java: `EventsDao.addEvent` and `EventsDao.save`.
    pub fn put_room_event(&self, event: &RoomEvent) -> Result<()> {
        let mut w = self.write()?;
        w.put_room_event(event)?;
        w.commit()
    }

    /// Java: `EventsDao.removeEvents`: the events of these rooms.
    pub fn remove_room_events(&self, room_ids: &[u32]) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(EVENTS)?;
            for id in room_ids {
                table.remove(*id)?;
            }
        }
        w.commit()
    }

    /// Java: `EventsDao.removeExpiredEvents` (`expire_time < now`). Returns how many went.
    pub fn remove_expired_room_events(&self, now: i64) -> Result<usize> {
        let expired: Vec<u32> = self
            .room_events()?
            .into_iter()
            .filter(|e| e.expire_time < now)
            .map(|e| e.room_id)
            .collect();
        self.remove_room_events(&expired)?;
        Ok(expired.len())
    }
}

impl Writer {
    /// Imports, adds or replaces the event of a room.
    pub fn put_room_event(&mut self, event: &RoomEvent) -> Result<()> {
        self.tx
            .open_table(EVENTS)?
            .insert(event.room_id, encode(event)?.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(room_id: u32, expire_time: i64) -> RoomEvent {
        RoomEvent {
            room_id,
            user_id: 1,
            category_id: 3,
            name: "Party".into(),
            expire_time,
            ..Default::default()
        }
    }

    #[test]
    fn events_round_trip_and_expire() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        store.put_room_event(&event(1001, 50)).unwrap();
        store.put_room_event(&event(1000, 100)).unwrap();
        let mut renamed = event(1000, 100);
        renamed.name = "Renamed".into();
        store.put_room_event(&renamed).unwrap();
        assert_eq!(
            store.room_events().unwrap(),
            [renamed.clone(), event(1001, 50)]
        );
        assert_eq!(store.remove_expired_room_events(50).unwrap(), 0);
        assert_eq!(store.remove_expired_room_events(51).unwrap(), 1);
        assert_eq!(store.room_events().unwrap(), [renamed]);
        store.remove_room_events(&[1000, 7]).unwrap();
        assert!(store.room_events().unwrap().is_empty());
    }
}
