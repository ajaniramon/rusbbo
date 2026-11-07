//! `rooms_bots` (Java: `BotDao.getBotData`): the NPCs of the public rooms (the Welcome Lounge's
//! Harry, the café's Regina...), added to a room on its first entry.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

/// Keyed by the row id.
const ROOM_BOTS: TableDefinition<u32, &[u8]> = TableDefinition::new("rooms_bots");
/// `rooms_bots` by `(room_id, id)`: Java only ever asks for the bots of one room.
const ROOM_BOTS_BY_ROOM: TableDefinition<(u32, u32), ()> =
    TableDefinition::new("rooms_bots_by_room");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(ROOM_BOTS)?;
    tx.open_table(ROOM_BOTS_BY_ROOM)?;
    Ok(())
}

/// A row of `rooms_bots`, as stored; `BotData` in the server parses it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomBot {
    pub id: u32,
    pub room_id: u32,
    pub name: String,
    pub mission: String,
    pub x: i32,
    pub y: i32,
    /// `"head,body"` rotations.
    pub start_look: String,
    pub figure: String,
    pub figure_flash: String,
    /// Space separated `x,y` tiles the bot wanders over.
    pub walkspace: String,
    /// `|` separated sentences, each optionally `text#SHOUT` (or another chat type).
    pub speech: String,
    pub response: String,
    pub unrecognised_response: String,
    /// Comma separated drinks the bot hands out.
    pub hand_items: String,
}

impl Store {
    /// Java: `SELECT * FROM rooms_bots WHERE room_id = ?`, in id order.
    pub fn room_bots(&self, room_id: u32) -> Result<Vec<RoomBot>> {
        let tx = self.db.begin_read()?;
        let bots = tx.open_table(ROOM_BOTS)?;
        let mut out = Vec::new();
        for entry in tx
            .open_table(ROOM_BOTS_BY_ROOM)?
            .range((room_id, 0)..=(room_id, u32::MAX))?
        {
            let id = entry?.0.value().1;
            if let Some(bytes) = bots.get(id)? {
                out.push(decode(bytes.value())?);
            }
        }
        Ok(out)
    }
}

impl Writer {
    /// Inserts or replaces a bot and keeps the by-room index in step.
    pub fn put_room_bot(&mut self, bot: &RoomBot) -> Result<()> {
        let old: Option<RoomBot> = {
            let table = self.tx.open_table(ROOM_BOTS)?;
            let row = table.get(bot.id)?;
            row.map(|v| decode(v.value())).transpose()?
        };
        {
            let mut index = self.tx.open_table(ROOM_BOTS_BY_ROOM)?;
            if let Some(old) = old {
                index.remove((old.room_id, old.id))?;
            }
            index.insert((bot.room_id, bot.id), ())?;
        }
        let bytes = encode(bot)?;
        self.tx
            .open_table(ROOM_BOTS)?
            .insert(bot.id, bytes.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bots_by_room_in_id_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        for (id, room_id, name) in [(9, 1, "Sadie"), (3, 1, "Dave"), (4, 2, "Harry")] {
            w.put_room_bot(&RoomBot {
                id,
                room_id,
                name: name.into(),
                ..Default::default()
            })
            .unwrap();
        }
        // Moving a bot to another room moves its index entry.
        w.put_room_bot(&RoomBot {
            id: 4,
            room_id: 3,
            name: "Harry".into(),
            ..Default::default()
        })
        .unwrap();
        w.commit().unwrap();
        let names = |room| -> Vec<String> {
            store
                .room_bots(room)
                .unwrap()
                .into_iter()
                .map(|b| b.name)
                .collect()
        };
        assert_eq!(names(1), ["Dave", "Sadie"]);
        assert!(names(2).is_empty());
        assert_eq!(names(3), ["Harry"]);
    }
}
