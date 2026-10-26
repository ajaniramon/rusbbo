//! Item tables: definitions and the items users own.
//!
//! Records mirror the MariaDB columns `ItemDao` reads (`items_definitions`, `items`). Item ids
//! follow the table's `AUTO_INCREMENT` counter, which the importer copies, so a purchase gets the
//! id the Java server's `INSERT INTO items` would have produced.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const ITEMS_DEFINITIONS: TableDefinition<u32, &[u8]> = TableDefinition::new("items_definitions");
const ITEMS: TableDefinition<u64, &[u8]> = TableDefinition::new("items");
/// Secondary index: (owner, item id) -> (). Rows with a NULL `user_id` have no entry.
const ITEMS_BY_USER: TableDefinition<(u32, u64), ()> = TableDefinition::new("items_by_user");
/// Counters of this area; for now only `items_auto_increment`.
const ITEMS_META: TableDefinition<&str, u64> = TableDefinition::new("items_meta");
const AUTO_INCREMENT: &str = "items_auto_increment";

/// A row of `items_definitions`. `behaviour` and `interactor` are kept as text, parsed by the
/// server (Java: `ItemDefinition` constructor).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemDefinition {
    pub id: u32,
    pub sprite: String,
    pub name: String,
    pub description: String,
    pub sprite_id: i32,
    pub length: i32,
    pub width: i32,
    pub top_height: f64,
    /// Column `max_status` is a varchar; Java reads it with `getInt`.
    pub max_status: i32,
    pub behaviour: String,
    pub interactor: String,
    pub is_tradable: bool,
    pub is_recyclable: bool,
    pub drink_ids: Option<String>,
    pub rental_time: i32,
    pub allowed_rotations: String,
    pub heights: Option<String>,
}

/// A row of `items`. Datetimes are Unix seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub order_id: i32,
    pub user_id: Option<u32>,
    pub room_id: i32,
    pub definition_id: u32,
    /// Columns `x`, `y`, `z` are varchars; Java reads them with `getInt`/`getDouble`.
    pub x: i32,
    pub y: i32,
    pub z: f64,
    pub wall_position: String,
    pub rotation: i32,
    pub custom_data: String,
    pub is_hidden: bool,
    pub is_trading: bool,
    pub expire_time: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Item {
    /// A row with the column defaults of `items`, as an `INSERT` naming only a few columns gets.
    /// The id is 0 until [`Writer::insert_item`] assigns one.
    pub fn with_defaults(user_id: u32, definition_id: u32, custom_data: String, now: i64) -> Self {
        Item {
            id: 0,
            order_id: -1,
            user_id: Some(user_id),
            room_id: 0,
            definition_id,
            x: 0,
            y: 0,
            z: 0.0,
            wall_position: String::new(),
            rotation: 0,
            custom_data,
            is_hidden: false,
            is_trading: false,
            expire_time: -1,
            created_at: now,
            updated_at: now,
        }
    }
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(ITEMS_DEFINITIONS)?;
    tx.open_table(ITEMS)?;
    tx.open_table(ITEMS_BY_USER)?;
    let mut meta = tx.open_table(ITEMS_META)?;
    if meta.get(AUTO_INCREMENT)?.is_none() {
        meta.insert(AUTO_INCREMENT, 1)?;
    }
    Ok(())
}

impl Store {
    /// Java: `ItemDao.getItemDefinitions`.
    pub fn item_definitions(&self) -> Result<Vec<ItemDefinition>> {
        let tx = self.db.begin_read()?;
        let mut out = Vec::new();
        for entry in tx.open_table(ITEMS_DEFINITIONS)?.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// Java: `ItemDao.getInventory`: `WHERE user_id = ? AND room_id = 0 ORDER BY order_id ASC`
    /// (ties by id, the order of the `idx_user_room_order` index).
    pub fn inventory_items(&self, user_id: u32) -> Result<Vec<Item>> {
        let tx = self.db.begin_read()?;
        let items = tx.open_table(ITEMS)?;
        let mut out: Vec<Item> = Vec::new();
        for entry in tx
            .open_table(ITEMS_BY_USER)?
            .range((user_id, 0)..=(user_id, u64::MAX))?
        {
            let id = entry?.0.value().1;
            if let Some(bytes) = items.get(id)? {
                let item: Item = decode(bytes.value())?;
                if item.room_id == 0 {
                    out.push(item);
                }
            }
        }
        out.sort_by_key(|i| (i.order_id, i.id));
        Ok(out)
    }

    /// Java: `ItemDao.getItem`.
    pub fn item(&self, id: u64) -> Result<Option<Item>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(ITEMS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Replaces an existing item (Java: `ItemDao.saveItem`).
    pub fn put_item(&self, item: &Item) -> Result<()> {
        let mut w = self.write()?;
        w.put_item(item)?;
        w.commit()
    }
}

impl Writer {
    pub fn put_item_definition(&mut self, definition: &ItemDefinition) -> Result<()> {
        let bytes = encode(definition)?;
        self.tx
            .open_table(ITEMS_DEFINITIONS)?
            .insert(definition.id, bytes.as_slice())?;
        Ok(())
    }

    fn item_in_tx(&mut self, id: u64) -> Result<Option<Item>> {
        let table = self.tx.open_table(ITEMS)?;
        let row = table.get(id)?;
        row.map(|v| decode(v.value())).transpose()
    }

    /// Inserts or replaces an item and keeps the owner index in step. The item must have an id.
    pub fn put_item(&mut self, item: &Item) -> Result<()> {
        let old: Option<Item> = self.item_in_tx(item.id)?;
        {
            let mut by_user = self.tx.open_table(ITEMS_BY_USER)?;
            if let Some(owner) = old.as_ref().and_then(|o| o.user_id) {
                by_user.remove((owner, item.id))?;
            }
            if let Some(owner) = item.user_id {
                by_user.insert((owner, item.id), ())?;
            }
        }
        let bytes = encode(item)?;
        self.tx
            .open_table(ITEMS)?
            .insert(item.id, bytes.as_slice())?;
        Ok(())
    }

    /// Inserts a new item under the next auto-increment id and returns it (Java:
    /// `ItemDao.newItem`, which reads `getGeneratedKeys`). Like the Java `INSERT`, only
    /// `user_id`, `definition_id`, `custom_data` and `expire_time` come from `item`; the other
    /// columns take their defaults.
    pub fn insert_item(&mut self, item: &Item, now: i64) -> Result<Item> {
        let id = self.next_item_id()?;
        let mut created = Item::with_defaults(
            item.user_id.unwrap_or(0),
            item.definition_id,
            item.custom_data.clone(),
            now,
        );
        created.id = id;
        created.user_id = item.user_id;
        created.expire_time = item.expire_time;
        self.put_item(&created)?;
        Ok(created)
    }

    fn next_item_id(&mut self) -> Result<u64> {
        let mut meta = self.tx.open_table(ITEMS_META)?;
        let id = meta.get(AUTO_INCREMENT)?.map(|v| v.value()).unwrap_or(1);
        meta.insert(AUTO_INCREMENT, id + 1)?;
        Ok(id)
    }

    /// Sets the next id `insert_item` hands out (the importer copies the table's counter).
    pub fn set_item_auto_increment(&mut self, next: u64) -> Result<()> {
        self.tx
            .open_table(ITEMS_META)?
            .insert(AUTO_INCREMENT, next)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        (dir, store)
    }

    #[test]
    fn insert_uses_auto_increment_and_indexes_by_owner() {
        let (_dir, store) = temp_store();
        let mut w = store.write().unwrap();
        w.set_item_auto_increment(40).unwrap();
        let draft = Item::with_defaults(7, 550, "0".into(), 1000);
        let a = w.insert_item(&draft, 1000).unwrap();
        let b = w.insert_item(&draft, 1000).unwrap();
        let other = w
            .insert_item(&Item::with_defaults(8, 550, "0".into(), 1000), 1000)
            .unwrap();
        w.commit().unwrap();
        assert_eq!((a.id, b.id, other.id), (40, 41, 42));

        let mine: Vec<u64> = store
            .inventory_items(7)
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(mine, [40, 41]);

        // A placed item (room_id != 0) is not in the inventory.
        let mut placed = b.clone();
        placed.room_id = 3;
        store.put_item(&placed).unwrap();
        assert_eq!(store.inventory_items(7).unwrap().len(), 1);
    }

    #[test]
    fn inventory_is_ordered_by_order_id() {
        let (_dir, store) = temp_store();
        let mut w = store.write().unwrap();
        for (id, order) in [(1u64, 2), (2, 0), (3, 1)] {
            let mut item = Item::with_defaults(1, 1, "0".into(), 0);
            item.id = id;
            item.order_id = order;
            w.put_item(&item).unwrap();
        }
        w.commit().unwrap();
        let ids: Vec<u64> = store
            .inventory_items(1)
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(ids, [2, 3, 1]);
    }
}
