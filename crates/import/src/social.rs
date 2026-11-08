//! `users_room_favourites`, `rooms_events` and `room_chatlogs` (Java: `RoomFavouritesDao`,
//! `EventsDao`, `RoomDao.saveChatLog`).

use anyhow::Result;
use mysql::Row;
use mysql::prelude::Queryable;
use rusbbo_storage::{ChatLog, RoomEvent, Store};

use crate::col;

/// Chat log rows per write transaction (Aeyra keeps a month of them).
const CHATLOG_BATCH: usize = 20_000;

/// Returns the number of rows copied.
pub fn import_social(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let mut w = store.write()?;
    let favourites: Vec<(u32, u32)> =
        conn.query("SELECT user_id, room_id FROM users_room_favourites")?;
    for (user_id, room_id) in &favourites {
        w.put_room_favourite(*user_id, *room_id)?;
    }
    let events: Vec<Row> = conn.query("SELECT * FROM rooms_events")?;
    for row in &events {
        w.put_room_event(&RoomEvent {
            room_id: col(row, "room_id")?,
            user_id: col(row, "user_id")?,
            category_id: col(row, "category_id")?,
            name: col(row, "name")?,
            description: col(row, "description")?,
            expire_time: col(row, "expire_time")?,
            tags: col(row, "tags")?,
        })?;
    }
    w.commit()?;

    let mut chatlogs = 0;
    let mut w = store.write()?;
    let rows = conn.query_iter(
        "SELECT id, user_id, room_id, timestamp, chat_type, message FROM room_chatlogs ORDER BY id",
    )?;
    for row in rows {
        let row = row?;
        w.put_chat_log(&ChatLog {
            id: col(&row, "id")?,
            user_id: col(&row, "user_id")?,
            room_id: col(&row, "room_id")?,
            timestamp: col(&row, "timestamp")?,
            chat_type: col(&row, "chat_type")?,
            message: col(&row, "message")?,
        })?;
        chatlogs += 1;
        if chatlogs % CHATLOG_BATCH == 0 {
            w.commit()?;
            w = store.write()?;
        }
    }
    w.commit()?;
    Ok(favourites.len() + events.len() + chatlogs)
}
