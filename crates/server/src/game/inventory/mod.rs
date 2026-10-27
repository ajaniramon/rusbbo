//! A player's hand (Java: `game/inventory/Inventory.java`).

use havana_storage::Store;
use tracing::error;

use crate::game::item::{Item, ItemManager};

const MAX_ITEMS_PER_PAGE: usize = 9;

/// What `INVENTORY` shows: strip slots with their items, and how many items the hand holds.
pub struct HandView<'a> {
    pub casts: Vec<(i32, &'a Item)>,
    pub displayed_count: usize,
}

#[derive(Default)]
pub struct Inventory {
    items: Vec<Item>,
    /// Indices into `items` of the items the hand shows, in strip order.
    displayed: Vec<usize>,
    hand_strip_page_index: i32,
}

impl Inventory {
    /// Java: `new Inventory(player)`, i.e. `reload`.
    pub fn load(store: &Store, manager: &ItemManager, user_id: u32) -> Self {
        let mut inventory = Self::default();
        inventory.reload(store, manager, user_id);
        inventory
    }

    /// Java: `Inventory.reload`.
    pub fn reload(&mut self, store: &Store, manager: &ItemManager, user_id: u32) {
        self.hand_strip_page_index = 0;
        self.items = match store.inventory_items(user_id) {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|r| manager.make_item(r))
                .collect(),
            Err(e) => {
                error!(user = user_id, "loading inventory: {e}");
                Vec::new()
            }
        };
        // TODO(rooms): drop the items that are currently placed in the player's room.
        self.refresh_pagination(store);
    }

    /// Java: `refreshPagination`. The hand shows the items in order and renumbers `order_id` to
    /// match, saving the items it changed.
    fn refresh_pagination(&mut self, store: &Store) {
        self.displayed.clear();
        let mut order_id = 0;
        for (index, item) in self.items.iter_mut().enumerate() {
            if !item.is_visible() {
                continue;
            }
            // Java also skips the items in the player's trade window; those have `is_trading`
            // set, which `is_visible` covers.
            if item.record.order_id != order_id {
                item.record.order_id = order_id;
                if let Err(e) = store.put_item(&item.record) {
                    error!(item = item.record.id, "saving item order: {e}");
                }
            }
            order_id += 1;
            self.displayed.push(index);
        }
    }

    fn page_count(&self) -> i32 {
        self.displayed.len().div_ceil(MAX_ITEMS_PER_PAGE) as i32
    }

    /// Java: `changeView`.
    fn change_view(&mut self, view: &str) {
        match view {
            "new" => self.hand_strip_page_index = 0,
            "next" => self.hand_strip_page_index += 1,
            "prev" => self.hand_strip_page_index -= 1,
            "last" => self.hand_strip_page_index = self.page_count() - 1,
            "current" => {
                self.hand_strip_page_index = self.hand_strip_page_index.min(self.page_count() - 1)
            }
            _ => {}
        }
        if !(0..self.page_count()).contains(&self.hand_strip_page_index) {
            self.hand_strip_page_index = 0;
        }
    }

    /// Java: `getView`: moves to `view` (`new`, `next`, `prev`, `last`, `current`) and returns what
    /// the client is sent.
    ///
    /// TODO(flash): the Flash client gets every displayed item, not only the current page.
    pub fn get_view(&mut self, store: &Store, view: &str) -> HandView<'_> {
        self.refresh_pagination(store);
        self.change_view(view);
        let first = self.hand_strip_page_index as usize * MAX_ITEMS_PER_PAGE;
        let casts = self
            .displayed
            .iter()
            .skip(first)
            .take(MAX_ITEMS_PER_PAGE)
            .enumerate()
            .map(|(i, &index)| ((first + i) as i32, &self.items[index]))
            .collect();
        HandView {
            casts,
            displayed_count: self.displayed.len(),
        }
    }

    /// Java: `getItems().stream().anyMatch(sprite.equalsIgnoreCase(..))`.
    pub fn has_sprite(&self, sprite: &str) -> bool {
        self.items
            .iter()
            .any(|i| i.definition.sprite().eq_ignore_ascii_case(sprite))
    }

    /// Java: `getItem(itemId)`: by virtual id.
    pub fn item(&self, virtual_id: i32) -> Option<&Item> {
        self.items.iter().find(|i| i.virtual_id == virtual_id)
    }

    /// Java: `ItemDao.updateTradeStates(items, in_trade)` for the items of the hand with these
    /// row ids: puts them in or takes them out of a trade window.
    pub fn set_trading(&mut self, store: &Store, record_ids: &[u64], trading: bool) {
        for item in self
            .items
            .iter_mut()
            .filter(|i| record_ids.contains(&i.record.id))
        {
            item.record.is_trading = trading;
            if let Err(e) = store.update_item(&item.record) {
                error!(item = item.record.id, "saving trade state: {e}");
            }
        }
    }

    /// Takes an item out of the hand by its row id (a trade moves items between hands).
    pub fn take_record(&mut self, record_id: u64) -> Option<Item> {
        let index = self.items.iter().position(|i| i.record.id == record_id)?;
        Some(self.items.remove(index))
    }

    /// [`Inventory::item`], to change it in place.
    pub fn item_mut(&mut self, virtual_id: i32) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.virtual_id == virtual_id)
    }

    /// Java: `getItemByDatabaseId`.
    pub fn item_by_database_id(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.record.id == id)
    }

    /// Java: `getItems().remove(item)`, by virtual id. The strip is rebuilt by the next
    /// [`Inventory::get_view`].
    pub fn take_item(&mut self, virtual_id: i32) -> Option<Item> {
        let index = self.items.iter().position(|i| i.virtual_id == virtual_id)?;
        Some(self.items.remove(index))
    }

    /// Java: `addItem`: the newest item goes first.
    pub fn add_item(&mut self, item: Item) {
        self.items.retain(|i| i.record.id != item.record.id);
        self.items.insert(0, item);
    }
}
