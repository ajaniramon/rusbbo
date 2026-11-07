//! Embedded storage for the Havana Rust server: one redb file.
//!
//! All access goes through [`Store`] (or a [`Writer`] for multi-record transactions), which keeps
//! the secondary index tables in step with the primary ones. See `docs/DESIGN.md`.
//!
//! Adding an area (rooms, catalogue...): put its records, table definitions and its own
//! `impl Store` / `impl Writer` blocks in a module of its own (`rooms.rs`), with a
//! `create_tables(&WriteTransaction)` called from `Store::create_tables`, so areas can be added in
//! parallel without touching each other's code. Bump [`SCHEMA_VERSION`] only for changes to
//! existing tables; new tables only need `create_tables`.

mod bots;
mod catalogue;
mod items;
mod messenger;
mod moderation;
mod pets;
mod records;
mod room_rights;
mod rooms;
mod texts;
mod user;
mod wordfilter;

use std::collections::BTreeMap;
use std::path::Path;

use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition, WriteTransaction};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use bots::RoomBot;
pub use catalogue::{CatalogueItem, CataloguePackage, CataloguePage, CatalogueSaleBadge};
pub use items::{Item, ItemDefinition};
pub use messenger::{MessengerCategory, MessengerMessage, SEARCH_LIMIT};
pub use moderation::{AuditLogEntry, Ban, BanType, IpLog};
pub use pets::Pet;
pub use records::User;
pub use room_rights::RoomVote;
pub use rooms::{PublicItem, Room, RoomCategory, RoomModel, Walkway};
pub use user::{Achievement, Badge, Effect, STATISTIC_COLUMNS, UserAchievement, UserStatistics};
pub use wordfilter::WordfilterWord;

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
        store.create_tables()?;
        store.check_version()?;
        Ok(store)
    }

    /// Creates every table that does not exist yet (and the schema version of a new file). New
    /// tables need no version bump: a file written by an older build gets them, empty, when it is
    /// opened.
    fn create_tables(&self) -> Result<()> {
        let tx = self.db.begin_write()?;
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
            bots::create_tables(&tx)?;
            rooms::create_tables(&tx)?;
            catalogue::create_tables(&tx)?;
            items::create_tables(&tx)?;
            messenger::create_tables(&tx)?;
            moderation::create_tables(&tx)?;
            pets::create_tables(&tx)?;
            room_rights::create_tables(&tx)?;
            texts::create_tables(&tx)?;
            user::create_tables(&tx)?;
            wordfilter::create_tables(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Opens an existing database file.
    pub fn open(path: &Path) -> Result<Self> {
        let store = Self {
            db: Database::open(path)?,
        };
        store.check_version()?;
        store.create_tables()?;
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
        let cleared = w.clear_sso_tickets()?;
        w.commit()?;
        Ok(cleared)
    }

    /// Everything `Havana.java` resets on startup, in one transaction: SSO tickets, the
    /// `players.online` setting, `rooms.visitors_now` (`RoomDao.resetVisitors`),
    /// `items.is_trading` (`ItemDao.resetTradeStates`) and `users.is_online`
    /// (`PlayerDao.resetOnline`). After a crash these still describe the previous run: an item
    /// left `is_trading` would be hidden from its owner's hand for good.
    pub fn startup_reset(&self) -> Result<StartupReset> {
        let mut w = self.write()?;
        let reset = StartupReset {
            sso_tickets: w.clear_sso_tickets()?,
            online_users: w.reset_online()?,
            room_visitors: w.reset_visitors()?,
            trading_items: w.reset_trade_states()?,
        };
        w.put_setting("players.online", "0")?;
        w.commit()?;
        Ok(reset)
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

/// What [`Store::startup_reset`] changed: how many records of each kind.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StartupReset {
    pub sso_tickets: usize,
    pub online_users: usize,
    pub room_visitors: usize,
    pub trading_items: usize,
}

/// A write transaction over several records.
pub struct Writer {
    tx: WriteTransaction,
}

impl Writer {
    fn clear_sso_tickets(&mut self) -> Result<usize> {
        let ids: Vec<u32> = {
            let index = self.tx.open_table(USERS_BY_SSO)?;
            index
                .iter()?
                .map(|e| e.map(|(_, id)| id.value()))
                .collect::<std::result::Result<_, _>>()?
        };
        for id in &ids {
            if let Some(mut user) = self.user(*id)? {
                user.sso_ticket = None;
                self.put_user(&user)?;
            }
        }
        Ok(ids.len())
    }

    /// Java: `PlayerDao.resetOnline`. `is_online` is in no index, so rows are rewritten in place.
    fn reset_online(&mut self) -> Result<usize> {
        let mut table = self.tx.open_table(USERS)?;
        let mut changed = Vec::new();
        for entry in table.iter()? {
            let (id, bytes) = entry?;
            let mut user: User = decode(bytes.value())?;
            if user.is_online {
                user.is_online = false;
                changed.push((id.value(), encode(&user)?));
            }
        }
        for (id, bytes) in &changed {
            table.insert(*id, bytes.as_slice())?;
        }
        Ok(changed.len())
    }

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
    fn startup_reset_clears_the_previous_run() {
        let (_dir, store) = temp_store();
        store
            .put_user(&User {
                is_online: true,
                ..user(1, "a", Some("x"))
            })
            .unwrap();
        store.put_user(&user(2, "b", None)).unwrap();
        let mut w = store.write().unwrap();
        w.put_room(&Room {
            id: 7,
            owner_id: 1,
            visitors_now: 3,
            ..Default::default()
        })
        .unwrap();
        w.put_room(&Room {
            id: 8,
            owner_id: 2,
            ..Default::default()
        })
        .unwrap();
        w.put_item(&Item {
            id: 5,
            user_id: Some(1),
            is_trading: true,
            ..Default::default()
        })
        .unwrap();
        w.put_setting("players.online", "4").unwrap();
        w.commit().unwrap();

        let reset = store.startup_reset().unwrap();
        assert_eq!(
            reset,
            StartupReset {
                sso_tickets: 1,
                online_users: 1,
                room_visitors: 1,
                trading_items: 1,
            }
        );
        assert!(!store.user(1).unwrap().unwrap().is_online);
        assert_eq!(store.user(1).unwrap().unwrap().sso_ticket, None);
        assert_eq!(store.room(7).unwrap().unwrap().visitors_now, 0);
        let item = store.item(5).unwrap().unwrap();
        assert!(!item.is_trading);
        // The owner index still finds the rewritten item.
        assert_eq!(store.inventory_items(1).unwrap(), vec![item]);
        assert_eq!(store.settings().unwrap()["players.online"], "0");
        assert_eq!(store.startup_reset().unwrap(), StartupReset::default());
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
