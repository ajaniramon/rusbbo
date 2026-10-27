//! `wordfilter` (Java: `WordfilterDao.getBadWords`): the words the chat filter replaces or bans for.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

/// Keyed by the row id, so a scan returns the rows in the order of `SELECT * FROM wordfilter`.
const WORDFILTER: TableDefinition<u32, &[u8]> = TableDefinition::new("wordfilter");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(WORDFILTER)?;
    Ok(())
}

/// A row of `wordfilter`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordfilterWord {
    pub id: u32,
    pub word: String,
    pub is_bannable: bool,
    pub is_filterable: bool,
}

impl Store {
    /// Every word, in row order.
    pub fn wordfilter(&self) -> Result<Vec<WordfilterWord>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(WORDFILTER)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            out.push(decode(value.value())?);
        }
        Ok(out)
    }
}

impl Writer {
    pub fn put_wordfilter_word(&mut self, word: &WordfilterWord) -> Result<()> {
        let bytes = encode(word)?;
        self.tx
            .open_table(WORDFILTER)?
            .insert(word.id, bytes.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_come_back_in_id_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        for (id, word) in [(2, "b"), (1, "a")] {
            w.put_wordfilter_word(&WordfilterWord {
                id,
                word: word.into(),
                is_bannable: false,
                is_filterable: true,
            })
            .unwrap();
        }
        w.commit().unwrap();
        let words: Vec<String> = store
            .wordfilter()
            .unwrap()
            .into_iter()
            .map(|w| w.word)
            .collect();
        assert_eq!(words, ["a", "b"]);
    }
}
