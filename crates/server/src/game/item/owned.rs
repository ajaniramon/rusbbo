//! An item owned by a user (Java: `game/item/Item.java`). Only the inventory side so far.

use std::sync::Arc;

use super::base::{ItemBehaviour, ItemDefinition};

/// Custom data of a new room dimmer (Java: `Item.DEFAULT_ROOMDIMMER_CUSTOM_DATA`).
pub const DEFAULT_ROOMDIMMER_CUSTOM_DATA: &str = "1,1,1,#000000,255";

#[derive(Clone, Debug)]
pub struct Item {
    /// The `items` row.
    pub record: rusbbo_storage::Item,
    pub definition: Arc<ItemDefinition>,
    /// The id the client sees (Java: `Item.getVirtualId`, from `ItemManager`'s counter).
    pub virtual_id: i32,
}

impl Item {
    pub fn has_behaviour(&self, behaviour: ItemBehaviour) -> bool {
        self.definition.has_behaviour(behaviour)
    }

    /// Java: `Item.isVisible`.
    pub fn is_visible(&self) -> bool {
        !self.record.is_trading && !self.record.is_hidden
    }
}
