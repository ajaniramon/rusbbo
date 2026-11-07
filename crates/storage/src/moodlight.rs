//! `items_moodlight_presets` (Java: `MoodlightDao`): the three presets of a room dimmer, keyed by
//! the dimmer's item id. The row is created the first time the room owner opens the dimmer.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

/// Keyed by `item_id` (a `UNIQUE KEY` in MariaDB).
const PRESETS: TableDefinition<u64, &[u8]> = TableDefinition::new("items_moodlight_presets");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(PRESETS)?;
    Ok(())
}

/// The column default of `preset_1`..`preset_3`: background state, colour, strength.
pub const DEFAULT_MOODLIGHT_PRESET: &str = "1,#000000,255";

/// A row of `items_moodlight_presets`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoodlightPresets {
    pub item_id: u64,
    /// 1-based.
    pub current_preset: i32,
    /// `preset_1`, `preset_2`, `preset_3`.
    pub presets: [String; 3],
}

impl MoodlightPresets {
    /// The row `INSERT INTO items_moodlight_presets (item_id) VALUES (?)` creates.
    pub fn with_defaults(item_id: u64) -> Self {
        MoodlightPresets {
            item_id,
            current_preset: 1,
            presets: std::array::from_fn(|_| DEFAULT_MOODLIGHT_PRESET.to_string()),
        }
    }
}

impl Store {
    /// Java: `MoodlightDao.getPresets` (and `containsPreset`, which is `is_some`).
    pub fn moodlight_presets(&self, item_id: u64) -> Result<Option<MoodlightPresets>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(PRESETS)?;
        table.get(item_id)?.map(|v| decode(v.value())).transpose()
    }

    /// Java: `MoodlightDao.createPresets`: a row with the defaults. Like the `INSERT`, which
    /// fails on the unique key, an existing row is left alone.
    pub fn create_moodlight_presets(&self, item_id: u64) -> Result<()> {
        let mut w = self.write()?;
        let exists = w.tx.open_table(PRESETS)?.get(item_id)?.is_some();
        if !exists {
            w.put_moodlight_presets(&MoodlightPresets::with_defaults(item_id))?;
        }
        w.commit()
    }

    /// Java: `MoodlightDao.updatePresets`: an `UPDATE`, so nothing happens without a row.
    pub fn update_moodlight_presets(
        &self,
        item_id: u64,
        current_preset: i32,
        presets: &[String; 3],
    ) -> Result<()> {
        let mut w = self.write()?;
        let exists = w.tx.open_table(PRESETS)?.get(item_id)?.is_some();
        if exists {
            w.put_moodlight_presets(&MoodlightPresets {
                item_id,
                current_preset,
                presets: presets.clone(),
            })?;
        }
        w.commit()
    }
}

impl Writer {
    /// Inserts or replaces a dimmer's presets (import, and updates).
    pub fn put_moodlight_presets(&mut self, presets: &MoodlightPresets) -> Result<()> {
        let bytes = encode(presets)?;
        self.tx
            .open_table(PRESETS)?
            .insert(presets.item_id, bytes.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_created_once_and_updated_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        assert!(store.moodlight_presets(7).unwrap().is_none());
        // No row: the UPDATE changes nothing.
        let changed: [String; 3] = ["2,#74F5F5,128".into(), "1".into(), "1".into()];
        store.update_moodlight_presets(7, 2, &changed).unwrap();
        assert!(store.moodlight_presets(7).unwrap().is_none());

        store.create_moodlight_presets(7).unwrap();
        let row = store.moodlight_presets(7).unwrap().unwrap();
        assert_eq!(row, MoodlightPresets::with_defaults(7));
        assert_eq!(row.presets[2], "1,#000000,255");

        store.update_moodlight_presets(7, 2, &changed).unwrap();
        store.create_moodlight_presets(7).unwrap();
        let row = store.moodlight_presets(7).unwrap().unwrap();
        assert_eq!((row.current_preset, &row.presets), (2, &changed));
    }
}
