//! Rooms area: `rooms`, `rooms_models`, `rooms_categories`, `public_items` and
//! `public_roomwalkways` (Java: `RoomDao`, `RoomModelDao`, `NavigatorDao` categories,
//! `PublicRoomsDao`).
//!
//! Records keep the MariaDB columns the Java server reads. Nullable integer columns are stored as
//! the value JDBC `getInt` returns for them (0 for NULL); nullable text columns stay `Option`.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const ROOMS: TableDefinition<u32, &[u8]> = TableDefinition::new("rooms");
const ROOM_MODELS: TableDefinition<u32, &[u8]> = TableDefinition::new("rooms_models");
const ROOM_CATEGORIES: TableDefinition<u32, &[u8]> = TableDefinition::new("rooms_categories");
const PUBLIC_ITEMS: TableDefinition<u32, &[u8]> = TableDefinition::new("public_items");
/// `public_items` by `(room_model, id)`: the Java DAO only ever queries by model.
const PUBLIC_ITEMS_BY_MODEL: TableDefinition<(&str, u32), ()> =
    TableDefinition::new("public_items_by_model");
/// `public_roomwalkways` has no key in MariaDB; rows are kept in import order.
const WALKWAYS: TableDefinition<u32, &[u8]> = TableDefinition::new("public_roomwalkways");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(ROOMS)?;
    tx.open_table(ROOM_MODELS)?;
    tx.open_table(ROOM_CATEGORIES)?;
    tx.open_table(PUBLIC_ITEMS)?;
    tx.open_table(PUBLIC_ITEMS_BY_MODEL)?;
    tx.open_table(WALKWAYS)?;
    Ok(())
}

/// A row of `rooms`. Datetimes are Unix seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: u32,
    /// 0 for public rooms.
    pub owner_id: u32,
    pub category: i32,
    pub name: String,
    pub description: String,
    /// `rooms_models.model_id`.
    pub model: String,
    pub ccts: Option<String>,
    pub wallpaper: i32,
    pub floor: i32,
    pub landscape: String,
    pub showname: bool,
    pub superusers: bool,
    pub accesstype: i32,
    pub password: Option<String>,
    pub visitors_now: i32,
    pub visitors_max: i32,
    pub rating: i32,
    pub icon_data: String,
    pub group_id: i32,
    pub is_hidden: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A row of `rooms_models`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoomModel {
    pub id: u32,
    pub model_id: String,
    pub model_name: String,
    pub door_x: i32,
    pub door_y: i32,
    pub door_z: f64,
    pub door_dir: i32,
    /// Rows separated by `|`, as stored.
    pub heightmap: String,
    pub trigger_class: String,
}

/// A row of `rooms_categories`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomCategory {
    pub id: u32,
    pub order_id: i32,
    pub parent_id: i32,
    pub is_node: bool,
    pub name: String,
    pub public_spaces: bool,
    pub allow_trading: bool,
    pub minrole_access: i32,
    pub minrole_setflatcat: i32,
    pub club_only: bool,
    pub is_top_priority: bool,
}

/// A row of `public_items`: the furniture of a public room model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PublicItem {
    pub id: u32,
    pub room_model: String,
    pub sprite: Option<String>,
    pub x: i32,
    pub y: i32,
    pub z: f64,
    pub rotation: i32,
    pub top_height: f64,
    pub length: i32,
    pub width: i32,
    /// Comma-separated `ItemBehaviour` names, lower case.
    pub behaviour: String,
    pub current_program: Option<String>,
    /// `x,y,z,rotation`.
    pub teleport_to: Option<String>,
    /// `x,y,z,rotation`.
    pub swim_to: Option<String>,
}

/// A row of `public_roomwalkways`: tiles of one public room that lead into another.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Walkway {
    pub room_id: i32,
    pub to_id: i32,
    /// Space-separated `x,y` pairs.
    pub coords_map: Option<String>,
    /// `x,y,z,rotation` in the target room, or `None` for its door.
    pub door_position: Option<String>,
}

fn all<T: serde::de::DeserializeOwned>(
    store: &Store,
    def: TableDefinition<u32, &[u8]>,
) -> Result<Vec<T>> {
    let tx = store.db.begin_read()?;
    let table = tx.open_table(def)?;
    let mut out = Vec::new();
    for entry in table.iter()? {
        let (_, v) = entry?;
        out.push(decode(v.value())?);
    }
    Ok(out)
}

impl Store {
    pub fn room(&self, id: u32) -> Result<Option<Room>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(ROOMS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Reads, changes and writes back one room in a single transaction.
    pub fn update_room(&self, id: u32, change: impl FnOnce(&mut Room)) -> Result<Option<Room>> {
        let mut w = self.write()?;
        let Some(mut room) = w.room(id)? else {
            return Ok(None);
        };
        change(&mut room);
        w.put_room(&room)?;
        w.commit()?;
        Ok(Some(room))
    }

    /// Every room model, by id (Java: `RoomModelDao.getModels`).
    pub fn room_models(&self) -> Result<Vec<RoomModel>> {
        all(self, ROOM_MODELS)
    }

    /// Every navigator category, by id.
    pub fn room_categories(&self) -> Result<Vec<RoomCategory>> {
        all(self, ROOM_CATEGORIES)
    }

    /// Every walkway, in import order (Java: `PublicRoomsDao.getWalkways`).
    pub fn walkways(&self) -> Result<Vec<Walkway>> {
        all(self, WALKWAYS)
    }

    /// The furniture of a public room model, by id (Java: `PublicRoomsDao.getPublicItemData`).
    pub fn public_items(&self, room_model: &str) -> Result<Vec<PublicItem>> {
        let tx = self.db.begin_read()?;
        let index = tx.open_table(PUBLIC_ITEMS_BY_MODEL)?;
        let items = tx.open_table(PUBLIC_ITEMS)?;
        let mut out = Vec::new();
        for entry in index.range((room_model, 0)..=(room_model, u32::MAX))? {
            let (key, _) = entry?;
            let (_, id) = key.value();
            if let Some(v) = items.get(id)? {
                out.push(decode(v.value())?);
            }
        }
        Ok(out)
    }
}

impl Writer {
    pub fn room(&mut self, id: u32) -> Result<Option<Room>> {
        let table = self.tx.open_table(ROOMS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    pub fn put_room(&mut self, room: &Room) -> Result<()> {
        let bytes = encode(room)?;
        self.tx
            .open_table(ROOMS)?
            .insert(room.id, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_room_model(&mut self, model: &RoomModel) -> Result<()> {
        let bytes = encode(model)?;
        self.tx
            .open_table(ROOM_MODELS)?
            .insert(model.id, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_room_category(&mut self, category: &RoomCategory) -> Result<()> {
        let bytes = encode(category)?;
        self.tx
            .open_table(ROOM_CATEGORIES)?
            .insert(category.id, bytes.as_slice())?;
        Ok(())
    }

    /// Inserts or replaces a public item and keeps the by-model index in step.
    pub fn put_public_item(&mut self, item: &PublicItem) -> Result<()> {
        let old: Option<PublicItem> = {
            let table = self.tx.open_table(PUBLIC_ITEMS)?;
            table.get(item.id)?.map(|v| decode(v.value())).transpose()?
        };
        {
            let mut index = self.tx.open_table(PUBLIC_ITEMS_BY_MODEL)?;
            if let Some(old) = &old {
                index.remove((old.room_model.as_str(), old.id))?;
            }
            index.insert((item.room_model.as_str(), item.id), ())?;
        }
        let bytes = encode(item)?;
        self.tx
            .open_table(PUBLIC_ITEMS)?
            .insert(item.id, bytes.as_slice())?;
        Ok(())
    }

    /// Appends a walkway after the existing ones.
    pub fn push_walkway(&mut self, walkway: &Walkway) -> Result<()> {
        let mut table = self.tx.open_table(WALKWAYS)?;
        let next = table.last()?.map(|(k, _)| k.value() + 1).unwrap_or(0);
        let bytes = encode(walkway)?;
        table.insert(next, bytes.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_items_and_walkways() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        w.put_room(&Room {
            id: 7,
            model: "ballroom".into(),
            visitors_max: 50,
            ..Default::default()
        })
        .unwrap();
        for (id, model) in [(3, "ballroom"), (1, "ballroom"), (2, "pub_a")] {
            w.put_public_item(&PublicItem {
                id,
                room_model: model.into(),
                ..Default::default()
            })
            .unwrap();
        }
        w.push_walkway(&Walkway {
            room_id: 45,
            to_id: 69,
            ..Default::default()
        })
        .unwrap();
        w.push_walkway(&Walkway {
            room_id: 69,
            to_id: 45,
            ..Default::default()
        })
        .unwrap();
        w.commit().unwrap();

        let ids: Vec<u32> = store
            .public_items("ballroom")
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(ids, [1, 3]);
        assert_eq!(store.walkways().unwrap()[1].room_id, 69);

        store.update_room(7, |r| r.visitors_now = 2).unwrap();
        assert_eq!(store.room(7).unwrap().unwrap().visitors_now, 2);
        assert!(store.room(8).unwrap().is_none());

        // Moving an item to another model moves its index entry.
        let mut w = store.write().unwrap();
        w.put_public_item(&PublicItem {
            id: 1,
            room_model: "pub_a".into(),
            ..Default::default()
        })
        .unwrap();
        w.commit().unwrap();
        assert_eq!(store.public_items("ballroom").unwrap().len(), 1);
        assert_eq!(store.public_items("pub_a").unwrap().len(), 2);
    }
}
