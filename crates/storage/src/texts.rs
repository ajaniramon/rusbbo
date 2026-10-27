//! `external_texts` (Java: `SettingsDao.getTexts`, read into `TextsManager`): `entry -> text`.
//! The MariaDB table has no unique key; Java's map keeps the last row of an entry, and so does
//! the importer.

use redb::{TableDefinition, WriteTransaction};

use crate::{Result, Store, Writer};

const TEXTS: TableDefinition<&str, &str> = TableDefinition::new("external_texts");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(TEXTS)?;
    Ok(())
}

impl Store {
    /// The text of an entry, if there is one.
    pub fn external_text(&self, entry: &str) -> Result<Option<String>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(TEXTS)?;
        Ok(table.get(entry)?.map(|v| v.value().to_string()))
    }
}

impl Writer {
    pub fn put_external_text(&mut self, entry: &str, text: &str) -> Result<()> {
        self.tx.open_table(TEXTS)?.insert(entry, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_row_wins() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        w.put_external_text("a", "one").unwrap();
        w.put_external_text("a", "two").unwrap();
        w.commit().unwrap();
        assert_eq!(store.external_text("a").unwrap().as_deref(), Some("two"));
        assert_eq!(store.external_text("b").unwrap(), None);
    }
}
