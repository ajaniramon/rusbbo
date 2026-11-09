//! `items_photos`: the camera's photos (Java: `PhotoDao`), keyed by the photo item's id. A photo
//! hidden by the housekeeping (RCON `clear_photo`) stays with `is_active` false.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const PHOTOS: TableDefinition<u64, &[u8]> = TableDefinition::new("items_photos");

/// A row of `items_photos`. `timestamp` is Unix seconds; `data` the image the client sent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Photo {
    pub photo_id: u64,
    pub user_id: i64,
    pub timestamp: i64,
    pub data: Vec<u8>,
    pub checksum: i32,
    pub is_active: bool,
}

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(PHOTOS)?;
    Ok(())
}

impl Store {
    /// Java: `PhotoDao.addPhoto`, in a transaction of its own.
    pub fn put_photo(&self, photo: &Photo) -> Result<()> {
        let mut w = self.write()?;
        w.put_photo(photo)?;
        w.commit()
    }

    /// Java: `PhotoDao.getPhoto`: the photo of an item, if it is still active.
    pub fn active_photo(&self, photo_id: u64) -> Result<Option<Photo>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(PHOTOS)?;
        let Some(bytes) = table.get(photo_id)? else {
            return Ok(None);
        };
        let photo: Photo = decode(bytes.value())?;
        Ok(photo.is_active.then_some(photo))
    }

    /// Java: `PhotoDao.deleteItem`: hides the photo (`is_active = 0`).
    pub fn hide_photo(&self, photo_id: u64) -> Result<()> {
        let mut w = self.write()?;
        let photo = {
            let table = w.tx.open_table(PHOTOS)?;
            let bytes = table.get(photo_id)?;
            match bytes {
                Some(b) => Some(decode::<Photo>(b.value())?),
                None => None,
            }
        };
        if let Some(mut photo) = photo {
            photo.is_active = false;
            w.put_photo(&photo)?;
        }
        w.commit()
    }

    /// The ids of every stored photo (active or not), for the housekeeping's count.
    pub fn photo_ids(&self) -> Result<Vec<u64>> {
        let tx = self.db.begin_read()?;
        let mut out = Vec::new();
        for entry in tx.open_table(PHOTOS)?.iter()? {
            out.push(entry?.0.value());
        }
        Ok(out)
    }
}

impl Writer {
    pub fn put_photo(&mut self, photo: &Photo) -> Result<()> {
        let bytes = encode(photo)?;
        self.tx
            .open_table(PHOTOS)?
            .insert(photo.photo_id, bytes.as_slice())?;
        Ok(())
    }
}
