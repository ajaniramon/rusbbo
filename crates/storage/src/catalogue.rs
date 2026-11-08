//! Catalogue tables: pages, items, packages and sale badges.
//!
//! Records mirror the MariaDB columns that `CatalogueDao` reads (`catalogue_pages`,
//! `catalogue_items`, `catalogue_packages`, `catalogue_sale_badges`). Nullable columns are `Option`
//! so the server can apply JDBC's null handling itself. The catalogue is static game data: the
//! server loads all of it at startup, so the readers return whole tables in the order the Java
//! queries use.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const CATALOGUE_PAGES: TableDefinition<u32, &[u8]> = TableDefinition::new("catalogue_pages");
const CATALOGUE_ITEMS: TableDefinition<u32, &[u8]> = TableDefinition::new("catalogue_items");
const CATALOGUE_PACKAGES: TableDefinition<u32, &[u8]> = TableDefinition::new("catalogue_packages");
/// `catalogue_sale_badges` has no key in MariaDB: keyed by import order.
const CATALOGUE_SALE_BADGES: TableDefinition<u32, &[u8]> =
    TableDefinition::new("catalogue_sale_badges");

/// A row of `catalogue_pages`. `images` and `texts` are the JSON string lists of the columns,
/// parsed by the importer (Java: `StringUtil.GSON.fromJson`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CataloguePage {
    pub id: u32,
    pub old_id: i32,
    pub parent_id: i32,
    pub order_id: i32,
    pub min_role: i32,
    pub is_navigatable: bool,
    pub is_club_only: bool,
    pub name: String,
    pub icon: i32,
    pub colour: i32,
    pub layout: String,
    pub images: Vec<String>,
    pub texts: Vec<String>,
    pub seasonal_start: Option<String>,
    pub seasonal_length: i32,
    /// The `images` and `texts` columns as stored, for the housekeeping panel (which shows
    /// them as they are).
    #[serde(default)]
    pub images_json: String,
    #[serde(default)]
    pub texts_json: String,
}

/// A row of `catalogue_items`. `page_id` is the comma separated page list, verbatim.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueItem {
    pub id: u32,
    pub sale_code: Option<String>,
    pub page_id: Option<String>,
    pub order_id: i32,
    pub price_coins: i32,
    pub price_pixels: i32,
    pub seasonal_coins: i32,
    pub seasonal_pixels: i32,
    pub hidden: bool,
    pub amount: i32,
    pub definition_id: Option<i32>,
    /// Column `item_specialspriteid`.
    pub item_special_sprite_id: String,
    pub is_package: bool,
    pub active_at: Option<String>,
}

/// A row of `catalogue_packages`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CataloguePackage {
    pub id: u32,
    /// Column `salecode`.
    pub sale_code: Option<String>,
    pub definition_id: Option<i32>,
    pub special_sprite_id: Option<i32>,
    pub amount: Option<i32>,
}

/// A row of `catalogue_sale_badges`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueSaleBadge {
    pub sale_code: String,
    pub badge_code: String,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(CATALOGUE_PAGES)?;
    tx.open_table(CATALOGUE_ITEMS)?;
    tx.open_table(CATALOGUE_PACKAGES)?;
    tx.open_table(CATALOGUE_SALE_BADGES)?;
    Ok(())
}

impl Store {
    /// Java: `CatalogueDao.getPages` (`ORDER BY order_id ASC`; ties by id, as InnoDB returns them).
    pub fn catalogue_pages(&self) -> Result<Vec<CataloguePage>> {
        let tx = self.db.begin_read()?;
        let mut pages: Vec<CataloguePage> = Vec::new();
        for entry in tx.open_table(CATALOGUE_PAGES)?.iter()? {
            pages.push(decode(entry?.1.value())?);
        }
        pages.sort_by_key(|p| (p.order_id, p.id));
        Ok(pages)
    }

    /// Java: `CatalogueDao.getItems` (`ORDER BY order_id ASC`; ties by id).
    pub fn catalogue_items(&self) -> Result<Vec<CatalogueItem>> {
        let tx = self.db.begin_read()?;
        let mut items: Vec<CatalogueItem> = Vec::new();
        for entry in tx.open_table(CATALOGUE_ITEMS)?.iter()? {
            items.push(decode(entry?.1.value())?);
        }
        items.sort_by_key(|i| (i.order_id, i.id));
        Ok(items)
    }

    /// Java: `CatalogueDao.getPackages`, in id order.
    pub fn catalogue_packages(&self) -> Result<Vec<CataloguePackage>> {
        let tx = self.db.begin_read()?;
        let mut out = Vec::new();
        for entry in tx.open_table(CATALOGUE_PACKAGES)?.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// Java: `CatalogueDao.getBadgeRewards` (the rows; the server groups them by sale code).
    pub fn catalogue_sale_badges(&self) -> Result<Vec<CatalogueSaleBadge>> {
        let tx = self.db.begin_read()?;
        let mut out = Vec::new();
        for entry in tx.open_table(CATALOGUE_SALE_BADGES)?.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }
}

impl Writer {
    fn next_id(&mut self, table: TableDefinition<u32, &[u8]>, name: &str) -> Result<u32> {
        let last = {
            let t = self.tx.open_table(table)?;
            t.last()?.map(|(k, _)| k.value()).unwrap_or(0)
        };
        self.take_next_id(name, last)
    }

    /// The id the next `INSERT INTO catalogue_pages` gets (`AUTO_INCREMENT`).
    pub fn next_catalogue_page_id(&mut self) -> Result<u32> {
        self.next_id(CATALOGUE_PAGES, "catalogue_pages")
    }

    pub fn next_catalogue_item_id(&mut self) -> Result<u32> {
        self.next_id(CATALOGUE_ITEMS, "catalogue_items")
    }

    pub fn next_catalogue_package_id(&mut self) -> Result<u32> {
        self.next_id(CATALOGUE_PACKAGES, "catalogue_packages")
    }

    pub fn delete_catalogue_page(&mut self, id: u32) -> Result<()> {
        self.tx.open_table(CATALOGUE_PAGES)?.remove(id)?;
        Ok(())
    }

    pub fn delete_catalogue_item(&mut self, id: u32) -> Result<()> {
        self.tx.open_table(CATALOGUE_ITEMS)?.remove(id)?;
        Ok(())
    }

    pub fn delete_catalogue_package(&mut self, id: u32) -> Result<()> {
        self.tx.open_table(CATALOGUE_PACKAGES)?.remove(id)?;
        Ok(())
    }

    /// `INSERT INTO catalogue_sale_badges`, after the existing rows.
    pub fn add_catalogue_sale_badge(&mut self, badge: &CatalogueSaleBadge) -> Result<()> {
        let seq = {
            let t = self.tx.open_table(CATALOGUE_SALE_BADGES)?;
            t.last()?.map(|(k, _)| k.value() + 1).unwrap_or(0)
        };
        self.put_catalogue_sale_badge(seq, badge)
    }

    /// `DELETE FROM catalogue_sale_badges WHERE sale_code = ? AND badge_code = ?` (ignoring case,
    /// as the collation does).
    pub fn delete_catalogue_sale_badge(&mut self, sale_code: &str, badge_code: &str) -> Result<()> {
        let mut t = self.tx.open_table(CATALOGUE_SALE_BADGES)?;
        let mut gone = Vec::new();
        for entry in t.iter()? {
            let (k, v) = entry?;
            let b: CatalogueSaleBadge = decode(v.value())?;
            if b.sale_code.to_lowercase() == sale_code.to_lowercase()
                && b.badge_code.to_lowercase() == badge_code.to_lowercase()
            {
                gone.push(k.value());
            }
        }
        for k in gone {
            t.remove(k)?;
        }
        Ok(())
    }

    pub fn put_catalogue_page(&mut self, page: &CataloguePage) -> Result<()> {
        let bytes = encode(page)?;
        self.tx
            .open_table(CATALOGUE_PAGES)?
            .insert(page.id, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_catalogue_item(&mut self, item: &CatalogueItem) -> Result<()> {
        let bytes = encode(item)?;
        self.tx
            .open_table(CATALOGUE_ITEMS)?
            .insert(item.id, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_catalogue_package(&mut self, package: &CataloguePackage) -> Result<()> {
        let bytes = encode(package)?;
        self.tx
            .open_table(CATALOGUE_PACKAGES)?
            .insert(package.id, bytes.as_slice())?;
        Ok(())
    }

    /// Appends a sale badge row; `seq` is its position in the source table.
    pub fn put_catalogue_sale_badge(&mut self, seq: u32, badge: &CatalogueSaleBadge) -> Result<()> {
        let bytes = encode(badge)?;
        self.tx
            .open_table(CATALOGUE_SALE_BADGES)?
            .insert(seq, bytes.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_come_back_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        for (id, order) in [(1, 2), (2, 1), (3, 1)] {
            w.put_catalogue_page(&CataloguePage {
                id,
                order_id: order,
                ..Default::default()
            })
            .unwrap();
        }
        w.commit().unwrap();
        let ids: Vec<u32> = store
            .catalogue_pages()
            .unwrap()
            .iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, [2, 3, 1]);
    }
}
