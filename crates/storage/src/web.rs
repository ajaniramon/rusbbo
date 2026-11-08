//! The website's tables (Havana-Web's `cms_*`, `site_*`, `homes_*`, `groups_*` and friends).
//!
//! Most of them are small and read whole by the pages that show them, so they share one generic
//! shape: a [`Record`] type keyed by a `u32` id, stored with postcard. Queries filter a scan of
//! the table, which is what the Java DAOs' `SELECT ... WHERE` amounts to at these sizes.

use redb::{ReadableTable, ReadableTableMetadata, TableDefinition, WriteTransaction};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Result, Store, USERS, User, Writer, decode, encode};

/// A row of one of the website's tables.
pub trait Record: Serialize + DeserializeOwned + Clone {
    const TABLE: TableDefinition<'static, u32, &'static [u8]>;
    fn id(&self) -> u32;
    fn set_id(&mut self, id: u32);
}

/// Each `Record` table, for `create_tables`.
macro_rules! record_tables {
    ($($t:ty),* $(,)?) => {
        pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
            $( tx.open_table(<$t as Record>::TABLE)?; )*
            Ok(())
        }
    };
}

/// Declares a [`Record`] struct (with an `id: u32` field) and its table.
macro_rules! record {
    ($(#[$meta:meta])* $name:ident, $table:literal { $($(#[$fmeta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            pub id: u32,
            $($(#[$fmeta])* pub $field: $ty),*
        }
        impl Record for $name {
            const TABLE: TableDefinition<'static, u32, &'static [u8]> = TableDefinition::new($table);
            fn id(&self) -> u32 { self.id }
            fn set_id(&mut self, id: u32) { self.id = id; }
        }
    };
}

record!(
    /// `users_tags` (Java: `TagDao`): a tag on a user, a room or a group. The table has no key in
    /// MariaDB; ids here are the import order.
    Tag, "users_tags" {
        user_id: u32,
        room_id: u32,
        group_id: u32,
        tag: String,
        created_at: i64,
    }
);

record_tables!(Tag);

impl Store {
    pub fn record<R: Record>(&self, id: u32) -> Result<Option<R>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(R::TABLE)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Every row, by id.
    pub fn records<R: Record>(&self) -> Result<Vec<R>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(R::TABLE)?;
        let mut out = Vec::with_capacity(table.len()? as usize);
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// The rows `keep` accepts, by id.
    pub fn records_where<R: Record>(&self, keep: impl Fn(&R) -> bool) -> Result<Vec<R>> {
        Ok(self
            .records::<R>()?
            .into_iter()
            .filter(|r| keep(r))
            .collect())
    }

    pub fn put_record<R: Record>(&self, r: &R) -> Result<()> {
        let mut w = self.write()?;
        w.put_record(r)?;
        w.commit()
    }

    /// Inserts `r` with the next id (`AUTO_INCREMENT`); returns the id.
    pub fn insert_record<R: Record>(&self, r: &mut R) -> Result<u32> {
        let mut w = self.write()?;
        let id = w.insert_record(r)?;
        w.commit()?;
        Ok(id)
    }

    pub fn delete_record<R: Record>(&self, id: u32) -> Result<bool> {
        let mut w = self.write()?;
        let found = w.delete_record::<R>(id)?;
        w.commit()?;
        Ok(found)
    }

    /// Every user (for the website's searches and counts).
    pub fn users(&self) -> Result<Vec<User>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(USERS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// Java: `SiteDao.getLastVisits`: users online in the last 30 days.
    pub fn web_last_visits(&self) -> Result<i64> {
        let since = now() - 30 * 24 * 3600;
        Ok(self
            .users()?
            .iter()
            .filter(|u| u.last_online > since)
            .count() as i64)
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Writer {
    pub fn record<R: Record>(&mut self, id: u32) -> Result<Option<R>> {
        let table = self.tx.open_table(R::TABLE)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    pub fn put_record<R: Record>(&mut self, r: &R) -> Result<()> {
        let bytes = encode(r)?;
        self.tx
            .open_table(R::TABLE)?
            .insert(r.id(), bytes.as_slice())?;
        Ok(())
    }

    pub fn insert_record<R: Record>(&mut self, r: &mut R) -> Result<u32> {
        let id = {
            let table = self.tx.open_table(R::TABLE)?;
            table.last()?.map(|(k, _)| k.value()).unwrap_or(0) + 1
        };
        r.set_id(id);
        self.put_record(r)?;
        Ok(id)
    }

    pub fn delete_record<R: Record>(&mut self, id: u32) -> Result<bool> {
        Ok(self.tx.open_table(R::TABLE)?.remove(id)?.is_some())
    }
}
