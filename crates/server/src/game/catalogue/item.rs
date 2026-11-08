//! Java: `game/catalogue/CatalogueItem.java` and `CataloguePackage.java`.

use std::sync::Arc;

use crate::game::item::ItemManager;
use crate::game::item::base::ItemDefinition;

#[derive(Clone, Debug)]
pub struct CataloguePackage {
    pub sale_code: String,
    #[allow(dead_code)] // Java keeps it too; nothing reads it (the transaction log uses names)
    pub definition_id: i32,
    pub special_sprite_id: String,
    pub amount: i32,
    pub definition: Option<Arc<ItemDefinition>>,
}

#[derive(Clone, Debug)]
pub struct CatalogueItem {
    pub id: i32,
    pub sale_code: String,
    pub pages: Vec<i32>,
    pub order_id: i32,
    pub price_coins: i32,
    pub price_pixels: i32,
    pub seasonal_coins: i32,
    pub seasonal_pixels: i32,
    pub amount: i32,
    pub hidden: bool,
    pub definition: Option<Arc<ItemDefinition>>,
    pub item_special_id: String,
    pub is_package: bool,
    packages: Vec<CataloguePackage>,
    pub active_at: Option<String>,
}

impl CatalogueItem {
    /// Java: the `CatalogueItem` constructor. `None` when `page_id` is NULL or not a list of
    /// numbers (Java throws while loading and the rest of the table is lost; here only the row).
    pub fn new(r: havana_storage::CatalogueItem, items: &ItemManager) -> Option<Self> {
        let page_id = r.page_id.as_deref()?;
        // `"".split(",")` has one element, so an empty list is the array `[0]`.
        let pages = if page_id.is_empty() {
            vec![0]
        } else {
            page_id
                .split(',')
                .map(|p| p.parse().ok())
                .collect::<Option<Vec<i32>>>()?
        };
        let definition_id = r.definition_id.unwrap_or(0);
        let definition = items.definition(definition_id);
        if definition.is_none() && !r.is_package {
            tracing::warn!(
                "Item {} ({}) has an invalid definition id: {definition_id}",
                r.id,
                r.sale_code.as_deref().unwrap_or("")
            );
        }
        Some(Self {
            id: r.id as i32,
            sale_code: r.sale_code.unwrap_or_default(),
            pages,
            order_id: r.order_id,
            price_coins: r.price_coins,
            price_pixels: r.price_pixels,
            seasonal_coins: r.seasonal_coins,
            seasonal_pixels: r.seasonal_pixels,
            amount: r.amount,
            hidden: r.hidden,
            definition,
            item_special_id: r.item_special_sprite_id,
            is_package: r.is_package,
            packages: Vec::new(),
            active_at: r.active_at,
        })
    }

    pub fn has_page(&self, page_id: i32) -> bool {
        self.pages.contains(&page_id)
    }

    /// Java: `getPackages`. A plain item is a package of one: itself.
    pub fn packages(&self) -> Vec<CataloguePackage> {
        if self.is_package {
            return self.packages.clone();
        }
        vec![CataloguePackage {
            sale_code: self.sale_code.clone(),
            definition_id: self.definition.as_ref().map_or(0, |d| d.id()),
            special_sprite_id: self.item_special_id.clone(),
            amount: self.amount,
            definition: self.definition.clone(),
        }]
    }

    /// The packages stored for a package item (Java: `CatalogueManager.loadPackages`).
    pub fn add_package(&mut self, package: CataloguePackage) {
        self.packages.push(package);
    }

    /// Java: `isActive`: the item is on sale today (`active_at` is `MM-dd`).
    pub fn is_active(&self) -> bool {
        self.active_at.as_deref().is_some_and(|d| {
            d.eq_ignore_ascii_case(&chrono::Local::now().format("%m-%d").to_string())
        })
    }
}
