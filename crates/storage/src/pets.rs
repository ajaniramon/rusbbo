//! `items_pets` (Java: `PetDao`): a pet belongs to its nest item (a `PET_NEST` furni bought from
//! the catalogue with a name, race and colour) and comes into the room with it.

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

/// Keyed by the pet id.
const PETS: TableDefinition<u32, &[u8]> = TableDefinition::new("items_pets");
/// `items_pets` by `item_id` (the nest): Java only ever looks a pet up by its item.
const PETS_BY_ITEM: TableDefinition<u64, u32> = TableDefinition::new("items_pets_by_item");
const PETS_META: TableDefinition<&str, u32> = TableDefinition::new("items_pets_meta");
const AUTO_INCREMENT: &str = "auto_increment";

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(PETS)?;
    tx.open_table(PETS_BY_ITEM)?;
    let mut meta = tx.open_table(PETS_META)?;
    if meta.get(AUTO_INCREMENT)?.is_none() {
        meta.insert(AUTO_INCREMENT, 1)?;
    }
    Ok(())
}

/// A row of `items_pets` (Java: `PetDetails`). Times are Unix seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pet {
    pub id: u32,
    /// The nest item.
    pub item_id: u64,
    pub name: String,
    /// `"0"` dog, `"1"` cat, `"2"` crocodile.
    pub pet_type: String,
    pub race: i32,
    pub colour: String,
    pub nature_positive: i32,
    pub nature_negative: i32,
    pub friendship: f32,
    pub born: i64,
    pub last_kip: i64,
    pub last_eat: i64,
    pub last_drink: i64,
    pub last_playtoy: i64,
    pub last_playuser: i64,
    pub x: i32,
    pub y: i32,
    pub rotation: i32,
}

impl Store {
    /// Java: `PetDao.getPetDetails(itemId)`.
    pub fn pet_by_item(&self, item_id: u64) -> Result<Option<Pet>> {
        let tx = self.db.begin_read()?;
        let Some(id) = tx
            .open_table(PETS_BY_ITEM)?
            .get(item_id)?
            .map(|v| v.value())
        else {
            return Ok(None);
        };
        let table = tx.open_table(PETS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Java: `PetDao.saveCoordinates`.
    pub fn save_pet_coordinates(&self, id: u32, x: i32, y: i32, rotation: i32) -> Result<()> {
        self.update_pet(id, |p| {
            p.x = x;
            p.y = y;
            p.rotation = rotation;
        })
    }

    /// Java: `PetDao.saveDetails`, which writes the five "last" times (not the friendship).
    pub fn save_pet_details(&self, pet: &Pet) -> Result<()> {
        self.update_pet(pet.id, |p| {
            p.last_kip = pet.last_kip;
            p.last_eat = pet.last_eat;
            p.last_drink = pet.last_drink;
            p.last_playtoy = pet.last_playtoy;
            p.last_playuser = pet.last_playuser;
        })
    }

    fn update_pet(&self, id: u32, change: impl FnOnce(&mut Pet)) -> Result<()> {
        let mut w = self.write()?;
        let row: Option<Pet> = {
            let table = w.tx.open_table(PETS)?;
            let row = table.get(id)?;
            row.map(|v| decode(v.value())).transpose()?
        };
        if let Some(mut pet) = row {
            change(&mut pet);
            w.put_pet(&pet)?;
        }
        w.commit()
    }
}

impl Writer {
    /// Inserts or replaces a pet and keeps the by-item index in step (import, and updates).
    pub fn put_pet(&mut self, pet: &Pet) -> Result<()> {
        let old: Option<Pet> = {
            let table = self.tx.open_table(PETS)?;
            let row = table.get(pet.id)?;
            row.map(|v| decode(v.value())).transpose()?
        };
        {
            let mut index = self.tx.open_table(PETS_BY_ITEM)?;
            if let Some(old) = old {
                index.remove(old.item_id)?;
            }
            index.insert(pet.item_id, pet.id)?;
        }
        let bytes = encode(pet)?;
        self.tx.open_table(PETS)?.insert(pet.id, bytes.as_slice())?;
        let mut meta = self.tx.open_table(PETS_META)?;
        let next = meta.get(AUTO_INCREMENT)?.map_or(1, |v| v.value());
        if pet.id >= next {
            meta.insert(AUTO_INCREMENT, pet.id + 1)?;
        }
        Ok(())
    }

    /// Java: `PetDao.createPet`: a new row under the next id; returns it.
    pub fn insert_pet(&mut self, mut pet: Pet) -> Result<Pet> {
        let id = self
            .tx
            .open_table(PETS_META)?
            .get(AUTO_INCREMENT)?
            .map_or(1, |v| v.value());
        pet.id = id;
        self.put_pet(&pet)?;
        Ok(pet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pets_by_item_and_updates() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        w.put_pet(&Pet {
            id: 4,
            item_id: 87,
            name: "Lacoste".into(),
            ..Default::default()
        })
        .unwrap();
        let mita = w
            .insert_pet(Pet {
                item_id: 127,
                name: "Mita".into(),
                ..Default::default()
            })
            .unwrap();
        w.commit().unwrap();
        assert_eq!(mita.id, 5, "after the imported ids");
        assert_eq!(store.pet_by_item(87).unwrap().unwrap().name, "Lacoste");
        assert!(store.pet_by_item(88).unwrap().is_none());

        store.save_pet_coordinates(4, 6, 7, 2).unwrap();
        let mut lacoste = store.pet_by_item(87).unwrap().unwrap();
        assert_eq!((lacoste.x, lacoste.y, lacoste.rotation), (6, 7, 2));
        lacoste.last_eat = 99;
        lacoste.friendship = 0.5;
        store.save_pet_details(&lacoste).unwrap();
        let saved = store.pet_by_item(87).unwrap().unwrap();
        assert_eq!(saved.last_eat, 99);
        assert_eq!(
            saved.friendship, 0.0,
            "saveDetails does not write the friendship"
        );
    }
}
