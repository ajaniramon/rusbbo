//! `infobus_polls` and `infobus_polls_answers` (Java: `InfobusDao`): the Infobus polls the
//! website's housekeeping creates, and the users' answers.

use std::collections::BTreeMap;

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

/// Keyed by the poll id.
const INFOBUS_POLLS: TableDefinition<u32, &[u8]> = TableDefinition::new("infobus_polls");
/// `(poll_id, user_id) -> answer` (the answer's index). Java's table has no key, but only ever
/// stores one answer per user and poll (`VOTE` checks `hasAnswer` first).
const INFOBUS_ANSWERS: TableDefinition<(u32, u32), i32> =
    TableDefinition::new("infobus_polls_answers");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(INFOBUS_POLLS)?;
    tx.open_table(INFOBUS_ANSWERS)?;
    Ok(())
}

/// A row of `infobus_polls`, with its `poll_data` JSON (Java: `InfobusPollData`) parsed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfobusPoll {
    pub id: u32,
    pub initiated_by: u32,
    pub question: String,
    pub answers: Vec<String>,
    /// Unix seconds.
    pub created_at: i64,
}

impl Store {
    /// Java: `InfobusDao.get`.
    pub fn infobus_poll(&self, id: u32) -> Result<Option<InfobusPoll>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(INFOBUS_POLLS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Java: `InfobusDao.hasAnswer`.
    pub fn has_infobus_answer(&self, poll_id: u32, user_id: u32) -> Result<bool> {
        let tx = self.db.begin_read()?;
        Ok(tx
            .open_table(INFOBUS_ANSWERS)?
            .get((poll_id, user_id))?
            .is_some())
    }

    /// Java: `InfobusDao.getAnswers`: the number of answers per answer index.
    pub fn infobus_answers(&self, poll_id: u32) -> Result<BTreeMap<i32, i32>> {
        let tx = self.db.begin_read()?;
        let mut out = BTreeMap::new();
        for entry in tx
            .open_table(INFOBUS_ANSWERS)?
            .range((poll_id, 0)..=(poll_id, u32::MAX))?
        {
            *out.entry(entry?.1.value()).or_insert(0) += 1;
        }
        Ok(out)
    }

    /// Java: `InfobusDao.getInfobusPolls`: every poll, newest first.
    pub fn infobus_polls(&self) -> Result<Vec<InfobusPoll>> {
        let tx = self.db.begin_read()?;
        let mut out: Vec<InfobusPoll> = Vec::new();
        for entry in tx.open_table(INFOBUS_POLLS)?.iter()? {
            out.push(decode(entry?.1.value())?);
        }
        out.sort_by_key(|p| std::cmp::Reverse(p.created_at));
        Ok(out)
    }

    /// Java: `InfobusDao.createInfobusPoll`: the new poll's id (`AUTO_INCREMENT`).
    pub fn insert_infobus_poll(&self, poll: &InfobusPoll) -> Result<u32> {
        let mut w = self.write()?;
        let last = {
            let table = w.tx.open_table(INFOBUS_POLLS)?;
            table.last()?.map(|(k, _)| k.value()).unwrap_or(0)
        };
        let id = w.take_next_id("infobus_polls", last)?;
        w.put_infobus_poll(&InfobusPoll { id, ..poll.clone() })?;
        w.commit()?;
        Ok(id)
    }

    /// Java: `InfobusDao.delete`.
    pub fn delete_infobus_poll(&self, id: u32) -> Result<()> {
        let w = self.write()?;
        w.tx.open_table(INFOBUS_POLLS)?.remove(id)?;
        w.commit()
    }

    /// Java: `InfobusDao.clearAnswers`.
    pub fn clear_infobus_answers(&self, poll_id: u32) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(INFOBUS_ANSWERS)?;
            let keys: Vec<(u32, u32)> = table
                .range((poll_id, 0)..=(poll_id, u32::MAX))?
                .map(|e| e.map(|(k, _)| k.value()))
                .collect::<std::result::Result<_, _>>()?;
            for k in keys {
                table.remove(k)?;
            }
        }
        w.commit()
    }

    /// Java: `InfobusDao.addAnswer`.
    pub fn add_infobus_answer(&self, poll_id: u32, answer: i32, user_id: u32) -> Result<()> {
        let mut w = self.write()?;
        w.put_infobus_answer(poll_id, answer, user_id)?;
        w.commit()
    }
}

impl Writer {
    pub fn put_infobus_poll(&mut self, poll: &InfobusPoll) -> Result<()> {
        let bytes = encode(poll)?;
        self.tx
            .open_table(INFOBUS_POLLS)?
            .insert(poll.id, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_infobus_answer(&mut self, poll_id: u32, answer: i32, user_id: u32) -> Result<()> {
        self.tx
            .open_table(INFOBUS_ANSWERS)?
            .insert((poll_id, user_id), answer)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polls_and_answers() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let poll = InfobusPoll {
            id: 3,
            initiated_by: 21,
            question: "Which?".into(),
            answers: vec!["A".into(), "B".into()],
            created_at: 5,
        };
        let mut w = store.write().unwrap();
        w.put_infobus_poll(&poll).unwrap();
        w.commit().unwrap();
        assert_eq!(store.infobus_poll(3).unwrap(), Some(poll));
        assert_eq!(store.infobus_poll(4).unwrap(), None);
        assert!(!store.has_infobus_answer(3, 1).unwrap());
        store.add_infobus_answer(3, 1, 1).unwrap();
        store.add_infobus_answer(3, 1, 2).unwrap();
        store.add_infobus_answer(3, 0, 5).unwrap();
        store.add_infobus_answer(4, 0, 5).unwrap();
        assert!(store.has_infobus_answer(3, 1).unwrap());
        assert_eq!(
            store
                .infobus_answers(3)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            [(0, 1), (1, 2)]
        );
    }
}
