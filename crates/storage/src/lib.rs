//! Embedded storage for the Havana Rust server: one redb file.
//!
//! All access goes through [`Store`] (or a [`Writer`] for multi-record transactions), which keeps
//! the secondary index tables in step with the primary ones. See `docs/DESIGN.md`.
//!
//! Adding an area (rooms, catalogue...): put its records, table definitions and its own
//! `impl Store` / `impl Writer` blocks in a module of its own (`rooms.rs`), with a
//! `create_tables(&WriteTransaction)` called from [`Store::create`], so areas can be added in
//! parallel without touching each other's code. Bump [`SCHEMA_VERSION`] only for changes to
//! existing tables; new tables only need `create_tables`.

mod records;
mod rooms;

use std::collections::BTreeMap;
use std::path::Path;

use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition, WriteTransaction};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use records::User;
pub use rooms::{PublicItem, Room, RoomCategory, RoomModel, Walkway};

/// Bump when a record layout or table changes, and add a migration in [`Store::open`].
pub const SCHEMA_VERSION: u32 = 1;

const META: TableDefinition<&str, u32> = TableDefinition::new("meta");
const USERS: TableDefinition<u32, &[u8]> = TableDefinition::new("users");
const USERS_BY_NAME: TableDefinition<&str, u32> = TableDefinition::new("users_by_name");
const USERS_BY_SSO: TableDefinition<&str, u32> = TableDefinition::new("users_by_sso");
const SETTINGS: TableDefinition<&str, &str> = TableDefinition::new("settings");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Boxed: redb's error type is large and would bloat every `Result`.
    #[error("database: {0}")]
    Db(Box<redb::Error>),
    #[error("record encoding: {0}")]
    Encoding(#[from] postcard::Error),
    #[error("schema version {found} in file, this build expects {SCHEMA_VERSION}")]
    SchemaVersion { found: u32 },
    #[error("username {0:?} is already taken")]
    DuplicateName(String),
}

macro_rules! db_error {
    ($($t:ty),*) => {$(impl From<$t> for Error {
        fn from(e: $t) -> Self { Error::Db(Box::new(e.into())) }
    })*};
}
db_error!(
    redb::Error,
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError
);

pub type Result<T> = std::result::Result<T, Error>;

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(postcard::to_allocvec(value)?)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    Ok(postcard::from_bytes(bytes)?)
}

/// Usernames are unique case-insensitively, like the MariaDB collation the Java server relies on.
fn name_key(name: &str) -> String {
    name.to_lowercase()
}

pub struct Store {
    db: Database,
}

impl Store {
    /// Creates the database file, or opens it if it exists, and sets up the tables.
    pub fn create(path: &Path) -> Result<Self> {
        let db = Database::create(path)?;
        let store = Self { db };
        let tx = store.db.begin_write()?;
        {
            let mut meta = tx.open_table(META)?;
            if meta.get("schema_version")?.is_none() {
                meta.insert("schema_version", SCHEMA_VERSION)?;
            }
            tx.open_table(USERS)?;
            tx.open_table(USERS_BY_NAME)?;
            tx.open_table(USERS_BY_SSO)?;
            tx.open_table(SETTINGS)?;
            // Area modules: one `create_tables(&tx)?;` call each.
            rooms::create_tables(&tx)?;
        }
        tx.commit()?;
        store.check_version()?;
        Ok(store)
    }

    /// Opens an existing database file.
    pub fn open(path: &Path) -> Result<Self> {
        let store = Self {
            db: Database::open(path)?,
        };
        store.check_version()?;
        Ok(store)
    }

    fn check_version(&self) -> Result<()> {
        let tx = self.db.begin_read()?;
        let found = tx
            .open_table(META)?
            .get("schema_version")?
            .map(|v| v.value())
            .unwrap_or(0);
        if found != SCHEMA_VERSION {
            return Err(Error::SchemaVersion { found });
        }
        Ok(())
    }

    /// Starts a write transaction; nothing is visible to readers until [`Writer::commit`].
    pub fn write(&self) -> Result<Writer> {
        Ok(Writer {
            tx: self.db.begin_write()?,
        })
    }

    pub fn user(&self, id: u32) -> Result<Option<User>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(USERS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    pub fn user_by_name(&self, name: &str) -> Result<Option<User>> {
        let id = {
            let tx = self.db.begin_read()?;
            tx.open_table(USERS_BY_NAME)?
                .get(name_key(name).as_str())?
                .map(|v| v.value())
        };
        id.map(|id| self.user(id)).transpose().map(Option::flatten)
    }

    pub fn user_by_sso(&self, ticket: &str) -> Result<Option<User>> {
        let id = {
            let tx = self.db.begin_read()?;
            tx.open_table(USERS_BY_SSO)?.get(ticket)?.map(|v| v.value())
        };
        id.map(|id| self.user(id)).transpose().map(Option::flatten)
    }

    pub fn user_count(&self) -> Result<u64> {
        let tx = self.db.begin_read()?;
        Ok(tx.open_table(USERS)?.len()?)
    }

    /// Reads, changes and writes back one user in a single transaction.
    pub fn update_user(&self, id: u32, change: impl FnOnce(&mut User)) -> Result<Option<User>> {
        let mut w = self.write()?;
        let Some(mut user) = w.user(id)? else {
            return Ok(None);
        };
        change(&mut user);
        w.put_user(&user)?;
        w.commit()?;
        Ok(Some(user))
    }

    pub fn put_user(&self, user: &User) -> Result<()> {
        let mut w = self.write()?;
        w.put_user(user)?;
        w.commit()
    }

    /// Removes every SSO ticket (the Java server does this on startup). Returns how many were set.
    pub fn clear_sso_tickets(&self) -> Result<usize> {
        let mut w = self.write()?;
        let ids: Vec<u32> = {
            let index = w.tx.open_table(USERS_BY_SSO)?;
            index
                .iter()?
                .map(|e| e.map(|(_, id)| id.value()))
                .collect::<std::result::Result<_, _>>()?
        };
        for id in &ids {
            if let Some(mut user) = w.user(*id)? {
                user.sso_ticket = None;
                w.put_user(&user)?;
            }
        }
        w.commit()?;
        Ok(ids.len())
    }

    pub fn settings(&self) -> Result<BTreeMap<String, String>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(SETTINGS)?;
        let mut out = BTreeMap::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            out.insert(k.value().to_string(), v.value().to_string());
        }
        Ok(out)
    }

    pub fn put_setting(&self, name: &str, value: &str) -> Result<()> {
        let mut w = self.write()?;
        w.put_setting(name, value)?;
        w.commit()
    }
}

/// A write transaction over several records.
pub struct Writer {
    tx: WriteTransaction,
}

impl Writer {
    pub fn user(&mut self, id: u32) -> Result<Option<User>> {
        let table = self.tx.open_table(USERS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Inserts or replaces a user and updates the name and SSO indexes.
    pub fn put_user(&mut self, user: &User) -> Result<()> {
        let old: Option<User> = self.user(user.id)?;
        {
            let mut by_name = self.tx.open_table(USERS_BY_NAME)?;
            let key = name_key(&user.username);
            if let Some(other) = by_name.get(key.as_str())?.map(|v| v.value()) {
                if other != user.id {
                    return Err(Error::DuplicateName(user.username.clone()));
                }
            }
            if let Some(old) = &old {
                if name_key(&old.username) != key {
                    by_name.remove(name_key(&old.username).as_str())?;
                }
            }
            by_name.insert(key.as_str(), user.id)?;
        }
        {
            let mut by_sso = self.tx.open_table(USERS_BY_SSO)?;
            if let Some(ticket) = old.as_ref().and_then(|o| o.sso_ticket.as_deref()) {
                by_sso.remove(ticket)?;
            }
            if let Some(ticket) = user.sso_ticket.as_deref() {
                by_sso.insert(ticket, user.id)?;
            }
        }
        let bytes = encode(user)?;
        self.tx
            .open_table(USERS)?
            .insert(user.id, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_setting(&mut self, name: &str, value: &str) -> Result<()> {
        self.tx.open_table(SETTINGS)?.insert(name, value)?;
        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u32, name: &str, sso: Option<&str>) -> User {
        User {
            id,
            username: name.into(),
            sso_ticket: sso.map(Into::into),
            credits: 50,
            ..Default::default()
        }
    }

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        (dir, store)
    }

    #[test]
    fn users_and_indexes() {
        let (_dir, store) = temp_store();
        store.put_user(&user(1, "Alice", Some("t1"))).unwrap();
        store.put_user(&user(2, "bob", None)).unwrap();

        assert_eq!(store.user(1).unwrap().unwrap().username, "Alice");
        assert_eq!(store.user_by_name("alice").unwrap().unwrap().id, 1);
        assert_eq!(store.user_by_sso("t1").unwrap().unwrap().id, 1);
        assert!(store.user_by_sso("t2").unwrap().is_none());
        assert_eq!(store.user_count().unwrap(), 2);

        // Renaming and changing the ticket move the index entries.
        store
            .update_user(1, |u| {
                u.username = "Alicia".into();
                u.sso_ticket = Some("t9".into());
            })
            .unwrap();
        assert!(store.user_by_name("alice").unwrap().is_none());
        assert_eq!(store.user_by_name("ALICIA").unwrap().unwrap().id, 1);
        assert!(store.user_by_sso("t1").unwrap().is_none());
        assert_eq!(store.user_by_sso("t9").unwrap().unwrap().id, 1);

        assert!(matches!(
            store.put_user(&user(3, "BOB", None)),
            Err(Error::DuplicateName(_))
        ));
    }

    #[test]
    fn clear_tickets() {
        let (_dir, store) = temp_store();
        store.put_user(&user(1, "a", Some("x"))).unwrap();
        store.put_user(&user(2, "b", Some("y"))).unwrap();
        assert_eq!(store.clear_sso_tickets().unwrap(), 2);
        assert!(store.user_by_sso("x").unwrap().is_none());
        assert_eq!(store.user(2).unwrap().unwrap().sso_ticket, None);
    }

    #[test]
    fn settings_round_trip_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.redb");
        {
            let store = Store::create(&path).unwrap();
            store.put_setting("daily.credits.amount", "120").unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .settings()
                .unwrap()
                .get("daily.credits.amount")
                .map(String::as_str),
            Some("120")
        );
    }
}
