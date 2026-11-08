//! Item definitions and virtual ids (Java: `game/item/ItemManager.java`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use anyhow::Result;
use rusbbo_storage::Store;

use super::Item;
use super::base::ItemDefinition;

pub struct ItemManager {
    definitions: HashMap<u32, Arc<ItemDefinition>>,
    virtual_id_counter: AtomicI32,
}

impl ItemManager {
    /// Java: `ItemDao.getItemDefinitions`.
    pub fn load(store: &Store) -> Result<Self> {
        Ok(Self::new(store.item_definitions()?))
    }

    pub fn new(records: Vec<rusbbo_storage::ItemDefinition>) -> Self {
        let definitions = records
            .into_iter()
            .filter_map(|r| ItemDefinition::new(r).map(|d| (d.record.id, Arc::new(d))))
            .collect();
        Self {
            definitions,
            virtual_id_counter: AtomicI32::new(0),
        }
    }

    pub fn definition(&self, id: i32) -> Option<Arc<ItemDefinition>> {
        u32::try_from(id)
            .ok()
            .and_then(|id| self.definitions.get(&id).cloned())
    }

    /// Java: `getDefinitionBySprite`. The sprites the server looks up (`present_gen*`) are
    /// unique, so the map's order does not matter.
    pub fn definition_by_sprite(&self, sprite: &str) -> Option<Arc<ItemDefinition>> {
        self.definitions
            .values()
            .find(|d| d.sprite() == sprite)
            .cloned()
    }

    /// Java: `Item.assignVirtualId`.
    pub fn next_virtual_id(&self) -> i32 {
        self.virtual_id_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Wraps a stored row as a live item with a fresh virtual id; `None` if its definition is
    /// missing (Java would fail on the null definition).
    pub fn make_item(&self, record: rusbbo_storage::Item) -> Option<Item> {
        let definition = self.definition(record.definition_id as i32)?;
        Some(Item {
            record,
            definition,
            virtual_id: self.next_virtual_id(),
        })
    }
}
