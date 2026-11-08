//! `users_transactions`: the audit log of purchases, gifts, trades and placements (Java:
//! `TransactionDao`). The game server only writes it; the website and housekeeping read it.
//!
//! The MariaDB table has no primary key; here each row gets an auto-increment id, in insertion
//! order, and a `(user_id, id)` index answers the per-user queries
//! (`TransactionDao.getTransactions`, the housekeeping user page).

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const TRANSACTIONS: TableDefinition<u64, &[u8]> = TableDefinition::new("users_transactions");
const TRANSACTIONS_BY_USER: TableDefinition<(u32, u64), ()> =
    TableDefinition::new("users_transactions_by_user");
const TRANSACTIONS_META: TableDefinition<&str, u64> = TableDefinition::new("transactions_meta");
const AUTO_INCREMENT: &str = "transactions_auto_increment";

/// A row of `users_transactions`. `item_id` is a comma-separated list of item ids, as in Java;
/// `created_at` is Unix seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub user_id: u32,
    pub item_id: String,
    pub catalogue_id: String,
    pub amount: i32,
    pub description: String,
    pub credit_cost: i32,
    pub pixel_cost: i32,
    pub created_at: i64,
    pub is_visible: bool,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(TRANSACTIONS)?;
    tx.open_table(TRANSACTIONS_BY_USER)?;
    tx.open_table(TRANSACTIONS_META)?;
    Ok(())
}

impl Store {
    /// [`Writer::insert_transaction`] in a transaction of its own.
    pub fn insert_transaction(&self, transaction: &Transaction) -> Result<u64> {
        let mut w = self.write()?;
        let id = w.insert_transaction(transaction)?;
        w.commit()?;
        Ok(id)
    }

    /// The transactions of a user, oldest first.
    pub fn transactions(&self, user_id: u32) -> Result<Vec<Transaction>> {
        let tx = self.db.begin_read()?;
        let rows = tx.open_table(TRANSACTIONS)?;
        let mut out = Vec::new();
        for entry in tx
            .open_table(TRANSACTIONS_BY_USER)?
            .range((user_id, 0)..=(user_id, u64::MAX))?
        {
            let id = entry?.0.value().1;
            if let Some(bytes) = rows.get(id)? {
                out.push(decode(bytes.value())?);
            }
        }
        Ok(out)
    }

    /// Every transaction, in id order (the housekeeping panel's reports).
    pub fn all_transactions(&self) -> Result<Vec<(u64, Transaction)>> {
        let tx = self.db.begin_read()?;
        let mut out = Vec::new();
        for entry in tx.open_table(TRANSACTIONS)?.iter()? {
            let (k, v) = entry?;
            out.push((k.value(), decode(v.value())?));
        }
        Ok(out)
    }

    /// How many transactions are stored, for every user.
    pub fn transaction_count(&self) -> Result<u64> {
        use redb::ReadableTableMetadata;
        let tx = self.db.begin_read()?;
        Ok(tx.open_table(TRANSACTIONS)?.len()?)
    }
}

impl Writer {
    /// Java: `TransactionDao.createTransaction`. Returns the row's id.
    pub fn insert_transaction(&mut self, transaction: &Transaction) -> Result<u64> {
        let id = {
            let mut meta = self.tx.open_table(TRANSACTIONS_META)?;
            let id = meta.get(AUTO_INCREMENT)?.map_or(1, |v| v.value());
            meta.insert(AUTO_INCREMENT, id + 1)?;
            id
        };
        let bytes = encode(transaction)?;
        self.tx
            .open_table(TRANSACTIONS)?
            .insert(id, bytes.as_slice())?;
        self.tx
            .open_table(TRANSACTIONS_BY_USER)?
            .insert((transaction.user_id, id), ())?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_by_user_in_insertion_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let row = |user_id, description: &str| Transaction {
            user_id,
            description: description.into(),
            ..Default::default()
        };
        let mut w = store.write().unwrap();
        w.insert_transaction(&row(2, "b1")).unwrap();
        w.insert_transaction(&row(1, "a1")).unwrap();
        w.insert_transaction(&row(2, "b2")).unwrap();
        w.commit().unwrap();
        let descriptions = |id| -> Vec<String> {
            store
                .transactions(id)
                .unwrap()
                .into_iter()
                .map(|t| t.description)
                .collect()
        };
        assert_eq!(descriptions(2), ["b1", "b2"]);
        assert_eq!(descriptions(1), ["a1"]);
        assert_eq!(store.transaction_count().unwrap(), 3);
    }
}
