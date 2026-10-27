//! Rooms area: `rooms`, `rooms_models`, `rooms_categories`, `public_items` and
//! `public_roomwalkways` (Java: `RoomDao`, `RoomModelDao`, `NavigatorDao` categories,
//! `PublicRoomsDao`).
//!
//! Records keep the MariaDB columns the Java server reads. Nullable integer columns are stored as
//! the value JDBC `getInt` returns for them (0 for NULL); nullable text columns stay `Option`.

use std::cmp::Reverse;

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const ROOMS: TableDefinition<u32, &[u8]> = TableDefinition::new("rooms");
const ROOM_MODELS: TableDefinition<u32, &[u8]> = TableDefinition::new("rooms_models");
const ROOM_CATEGORIES: TableDefinition<u32, &[u8]> = TableDefinition::new("rooms_categories");
const PUBLIC_ITEMS: TableDefinition<u32, &[u8]> = TableDefinition::new("public_items");
/// `rooms` by `(owner_id, id)`: Java's `owner_id` index (`RoomDao.getRoomsByUserId`, search by owner).
const ROOMS_BY_OWNER: TableDefinition<(u32, u32), ()> = TableDefinition::new("rooms_by_owner");
/// `rooms` by `(category, id)`: Java's `idx_category_visitors` (`NavigatorDao.getRecentRooms`,
/// `getRoomsByCategory`, `getRoomCountByCategory`).
const ROOMS_BY_CATEGORY: TableDefinition<(i32, u32), ()> =
    TableDefinition::new("rooms_by_category");
/// `public_items` by `(room_model, id)`: the Java DAO only ever queries by model.
const PUBLIC_ITEMS_BY_MODEL: TableDefinition<(&str, u32), ()> =
    TableDefinition::new("public_items_by_model");
/// `public_roomwalkways` has no key in MariaDB; rows are kept in import order.
const WALKWAYS: TableDefinition<u32, &[u8]> = TableDefinition::new("public_roomwalkways");

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(ROOMS)?;
    tx.open_table(ROOMS_BY_OWNER)?;
    tx.open_table(ROOMS_BY_CATEGORY)?;
    tx.open_table(ROOM_MODELS)?;
    tx.open_table(ROOM_CATEGORIES)?;
    tx.open_table(PUBLIC_ITEMS)?;
    tx.open_table(PUBLIC_ITEMS_BY_MODEL)?;
    tx.open_table(WALKWAYS)?;
    Ok(())
}

/// A row of `rooms`. Datetimes are Unix seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: u32,
    /// 0 for public rooms.
    pub owner_id: u32,
    pub category: i32,
    pub name: String,
    pub description: String,
    /// `rooms_models.model_id`.
    pub model: String,
    pub ccts: Option<String>,
    pub wallpaper: i32,
    pub floor: i32,
    pub landscape: String,
    pub showname: bool,
    pub superusers: bool,
    pub accesstype: i32,
    pub password: Option<String>,
    pub visitors_now: i32,
    pub visitors_max: i32,
    pub rating: i32,
    pub icon_data: String,
    pub group_id: i32,
    pub is_hidden: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A row of `rooms_models`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoomModel {
    pub id: u32,
    pub model_id: String,
    pub model_name: String,
    pub door_x: i32,
    pub door_y: i32,
    pub door_z: f64,
    pub door_dir: i32,
    /// Rows separated by `|`, as stored.
    pub heightmap: String,
    pub trigger_class: String,
}

/// A row of `rooms_categories`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomCategory {
    pub id: u32,
    pub order_id: i32,
    pub parent_id: i32,
    pub is_node: bool,
    pub name: String,
    pub public_spaces: bool,
    pub allow_trading: bool,
    pub minrole_access: i32,
    pub minrole_setflatcat: i32,
    pub club_only: bool,
    pub is_top_priority: bool,
}

/// A row of `public_items`: the furniture of a public room model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PublicItem {
    pub id: u32,
    pub room_model: String,
    pub sprite: Option<String>,
    pub x: i32,
    pub y: i32,
    pub z: f64,
    pub rotation: i32,
    pub top_height: f64,
    pub length: i32,
    pub width: i32,
    /// Comma-separated `ItemBehaviour` names, lower case.
    pub behaviour: String,
    pub current_program: Option<String>,
    /// `x,y,z,rotation`.
    pub teleport_to: Option<String>,
    /// `x,y,z,rotation`.
    pub swim_to: Option<String>,
}

/// A row of `public_roomwalkways`: tiles of one public room that lead into another.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Walkway {
    pub room_id: i32,
    pub to_id: i32,
    /// Space-separated `x,y` pairs.
    pub coords_map: Option<String>,
    /// `x,y,z,rotation` in the target room, or `None` for its door.
    pub door_position: Option<String>,
}

fn all<T: serde::de::DeserializeOwned>(
    store: &Store,
    def: TableDefinition<u32, &[u8]>,
) -> Result<Vec<T>> {
    let tx = store.db.begin_read()?;
    let table = tx.open_table(def)?;
    let mut out = Vec::new();
    for entry in table.iter()? {
        let (_, v) = entry?;
        out.push(decode(v.value())?);
    }
    Ok(out)
}

impl Store {
    pub fn room(&self, id: u32) -> Result<Option<Room>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(ROOMS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// The rooms of one owner in id order; owner 0 is the public rooms (Java:
    /// `RoomDao.getRoomsByUserId`).
    pub fn rooms_by_owner(&self, owner_id: u32) -> Result<Vec<Room>> {
        let tx = self.db.begin_read()?;
        let index = tx.open_table(ROOMS_BY_OWNER)?;
        let rooms = tx.open_table(ROOMS)?;
        let mut out = Vec::new();
        for entry in index.range((owner_id, 0)..=(owner_id, u32::MAX))? {
            let (key, _) = entry?;
            if let Some(v) = rooms.get(key.value().1)? {
                out.push(decode(v.value())?);
            }
        }
        Ok(out)
    }

    /// Every room of a category in id order (Java: `NavigatorDao.getRoomsByCategory`).
    pub fn rooms_by_category(&self, category: i32) -> Result<Vec<Room>> {
        let tx = self.db.begin_read()?;
        let index = tx.open_table(ROOMS_BY_CATEGORY)?;
        let rooms = tx.open_table(ROOMS)?;
        let mut out = Vec::new();
        for entry in index.range((category, 0)..=(category, u32::MAX))? {
            let (key, _) = entry?;
            if let Some(v) = rooms.get(key.value().1)? {
                out.push(decode(v.value())?);
            }
        }
        Ok(out)
    }

    /// Java: `NavigatorDao.getRoomCountByCategory`.
    pub fn room_count_by_category(&self, category: i32) -> Result<usize> {
        let tx = self.db.begin_read()?;
        let index = tx.open_table(ROOMS_BY_CATEGORY)?;
        let mut count = 0;
        for entry in index.range((category, 0)..=(category, u32::MAX))? {
            entry?;
            count += 1;
        }
        Ok(count)
    }

    /// Java: `NavigatorDao.getRecentRooms`: private rooms of a category, most visited first, then
    /// best rated. Ties come out by descending id, which is what MariaDB's backwards scan of
    /// `idx_category_visitors` (category, visitors_now, rating, primary key) returns.
    pub fn recent_rooms(&self, category: i32, limit: usize) -> Result<Vec<Room>> {
        let mut rooms = self.rooms_by_category(category)?;
        rooms.retain(|r| r.owner_id > 0);
        rooms.reverse();
        rooms.sort_by_key(|r| (Reverse(r.visitors_now), Reverse(r.rating)));
        rooms.truncate(limit);
        Ok(rooms)
    }

    /// Java: `RoomDao.searchRooms`. The SQL is
    /// `... FROM rooms INNER JOIN users ON rooms.owner_id = users.id WHERE <owner_id = ? AND |
    /// users.username LIKE ? OR> rooms.name LIKE ? ORDER BY visitors_now DESC, rating DESC LIMIT ?`
    /// with `%query%` as the patterns. `owner` is the id from the `owner:` tag, or `<= 0` when there
    /// is none (then the query also matches the owner's name, and `OR` has no parentheses, as in
    /// Java). A blank query without an owner (`-1`) finds nothing. Rooms are scanned: `LIKE
    /// '%..%'` cannot use an index in MariaDB either. Ties come out by ascending id.
    pub fn search_rooms(&self, query: &str, owner: i32, limit: usize) -> Result<Vec<Room>> {
        if query.trim().is_empty() && owner == -1 {
            return Ok(Vec::new());
        }
        let pattern = format!("%{query}%");
        let mut found = Vec::new();
        for room in all::<Room>(self, ROOMS)? {
            // INNER JOIN users: rooms whose owner row is missing (public rooms) never match.
            let Some(user) = self.user(room.owner_id)? else {
                continue;
            };
            let name_matches = like(&pattern, &room.name);
            let hit = if owner > 0 {
                room.owner_id == owner as u32 && name_matches
            } else {
                like(&pattern, &user.username) || name_matches
            };
            if hit {
                found.push(room);
            }
        }
        found.sort_by_key(|r| (Reverse(r.visitors_now), Reverse(r.rating)));
        found.truncate(limit);
        Ok(found)
    }

    /// Reads, changes and writes back one room in a single transaction.
    pub fn update_room(&self, id: u32, change: impl FnOnce(&mut Room)) -> Result<Option<Room>> {
        let mut w = self.write()?;
        let Some(mut room) = w.room(id)? else {
            return Ok(None);
        };
        change(&mut room);
        w.put_room(&room)?;
        w.commit()?;
        Ok(Some(room))
    }

    /// Every room model, by id (Java: `RoomModelDao.getModels`).
    pub fn room_models(&self) -> Result<Vec<RoomModel>> {
        all(self, ROOM_MODELS)
    }

    /// Every navigator category, by id.
    pub fn room_categories(&self) -> Result<Vec<RoomCategory>> {
        all(self, ROOM_CATEGORIES)
    }

    /// Every walkway, in import order (Java: `PublicRoomsDao.getWalkways`).
    pub fn walkways(&self) -> Result<Vec<Walkway>> {
        all(self, WALKWAYS)
    }

    /// The furniture of a public room model, by id (Java: `PublicRoomsDao.getPublicItemData`).
    pub fn public_items(&self, room_model: &str) -> Result<Vec<PublicItem>> {
        let tx = self.db.begin_read()?;
        let index = tx.open_table(PUBLIC_ITEMS_BY_MODEL)?;
        let items = tx.open_table(PUBLIC_ITEMS)?;
        let mut out = Vec::new();
        for entry in index.range((room_model, 0)..=(room_model, u32::MAX))? {
            let (key, _) = entry?;
            let (_, id) = key.value();
            if let Some(v) = items.get(id)? {
                out.push(decode(v.value())?);
            }
        }
        Ok(out)
    }
}

impl Writer {
    pub fn room(&mut self, id: u32) -> Result<Option<Room>> {
        let table = self.tx.open_table(ROOMS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Inserts or replaces a room and keeps the by-owner and by-category indexes in step.
    pub fn put_room(&mut self, room: &Room) -> Result<()> {
        let old = self.room(room.id)?;
        if old.as_ref().map(|o| o.owner_id) != Some(room.owner_id) {
            let mut index = self.tx.open_table(ROOMS_BY_OWNER)?;
            if let Some(old) = &old {
                index.remove((old.owner_id, old.id))?;
            }
            index.insert((room.owner_id, room.id), ())?;
        }
        if old.as_ref().map(|o| o.category) != Some(room.category) {
            let mut index = self.tx.open_table(ROOMS_BY_CATEGORY)?;
            if let Some(old) = &old {
                index.remove((old.category, old.id))?;
            }
            index.insert((room.category, room.id), ())?;
        }
        let bytes = encode(room)?;
        self.tx
            .open_table(ROOMS)?
            .insert(room.id, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_room_model(&mut self, model: &RoomModel) -> Result<()> {
        let bytes = encode(model)?;
        self.tx
            .open_table(ROOM_MODELS)?
            .insert(model.id, bytes.as_slice())?;
        Ok(())
    }

    pub fn put_room_category(&mut self, category: &RoomCategory) -> Result<()> {
        let bytes = encode(category)?;
        self.tx
            .open_table(ROOM_CATEGORIES)?
            .insert(category.id, bytes.as_slice())?;
        Ok(())
    }

    /// Inserts or replaces a public item and keeps the by-model index in step.
    pub fn put_public_item(&mut self, item: &PublicItem) -> Result<()> {
        let old: Option<PublicItem> = {
            let table = self.tx.open_table(PUBLIC_ITEMS)?;
            table.get(item.id)?.map(|v| decode(v.value())).transpose()?
        };
        {
            let mut index = self.tx.open_table(PUBLIC_ITEMS_BY_MODEL)?;
            if let Some(old) = &old {
                index.remove((old.room_model.as_str(), old.id))?;
            }
            index.insert((item.room_model.as_str(), item.id), ())?;
        }
        let bytes = encode(item)?;
        self.tx
            .open_table(PUBLIC_ITEMS)?
            .insert(item.id, bytes.as_slice())?;
        Ok(())
    }

    /// Appends a walkway after the existing ones.
    pub fn push_walkway(&mut self, walkway: &Walkway) -> Result<()> {
        let mut table = self.tx.open_table(WALKWAYS)?;
        let next = table.last()?.map(|(k, _)| k.value() + 1).unwrap_or(0);
        let bytes = encode(walkway)?;
        table.insert(next, bytes.as_slice())?;
        Ok(())
    }
}

/// SQL `LIKE` as MariaDB evaluates it with the `utf8mb4_uca1400_ai_ci` collation of the Java
/// schema: `%` and `_` are wildcards, `\` escapes the next character, and the comparison ignores
/// case. TODO(collation): accent insensitivity (`e` = `é`) is not reproduced.
pub(crate) fn like(pattern: &str, text: &str) -> bool {
    enum Tok {
        Any,
        One,
        Lit(char),
    }
    fn fold(s: &str) -> Vec<char> {
        s.chars().flat_map(char::to_lowercase).collect()
    }
    let mut toks = Vec::new();
    let mut chars = fold(pattern).into_iter();
    while let Some(c) = chars.next() {
        toks.push(match c {
            '%' => Tok::Any,
            '_' => Tok::One,
            '\\' => Tok::Lit(chars.next().unwrap_or('\\')),
            c => Tok::Lit(c),
        });
    }
    let text = fold(text);
    // Backtracking match that remembers the last `%`.
    let (mut t, mut p) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match toks.get(p) {
            Some(Tok::Any) => {
                star = Some((p, t));
                p += 1;
            }
            Some(Tok::One) => {
                p += 1;
                t += 1;
            }
            Some(Tok::Lit(c)) if *c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    toks[p..].iter().all(|t| matches!(t, Tok::Any))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_items_and_walkways() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        let mut w = store.write().unwrap();
        w.put_room(&Room {
            id: 7,
            model: "ballroom".into(),
            visitors_max: 50,
            ..Default::default()
        })
        .unwrap();
        for (id, model) in [(3, "ballroom"), (1, "ballroom"), (2, "pub_a")] {
            w.put_public_item(&PublicItem {
                id,
                room_model: model.into(),
                ..Default::default()
            })
            .unwrap();
        }
        w.push_walkway(&Walkway {
            room_id: 45,
            to_id: 69,
            ..Default::default()
        })
        .unwrap();
        w.push_walkway(&Walkway {
            room_id: 69,
            to_id: 45,
            ..Default::default()
        })
        .unwrap();
        w.commit().unwrap();

        let ids: Vec<u32> = store
            .public_items("ballroom")
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(ids, [1, 3]);
        assert_eq!(store.walkways().unwrap()[1].room_id, 69);

        store.update_room(7, |r| r.visitors_now = 2).unwrap();
        assert_eq!(store.room(7).unwrap().unwrap().visitors_now, 2);
        assert!(store.room(8).unwrap().is_none());

        // Moving an item to another model moves its index entry.
        let mut w = store.write().unwrap();
        w.put_public_item(&PublicItem {
            id: 1,
            room_model: "pub_a".into(),
            ..Default::default()
        })
        .unwrap();
        w.commit().unwrap();
        assert_eq!(store.public_items("ballroom").unwrap().len(), 1);
        assert_eq!(store.public_items("pub_a").unwrap().len(), 2);
    }

    #[test]
    fn room_indexes_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        for (id, name) in [(1, "alice"), (2, "bob")] {
            store
                .put_user(&crate::User {
                    id,
                    username: name.into(),
                    ..Default::default()
                })
                .unwrap();
        }
        let mut w = store.write().unwrap();
        for (id, owner, category, name, now, rating) in [
            (1, 0, 3, "Lounge", 0, 0),
            (10, 1, 112, "Alice Place", 2, 0),
            (11, 1, 112, "Chill Zone", 5, 1),
            (12, 2, 112, "Bob's lounge", 5, 3),
            (13, 2, 114, "100% fun", 0, 0),
        ] {
            w.put_room(&Room {
                id,
                owner_id: owner,
                category,
                name: name.into(),
                visitors_now: now,
                rating,
                ..Default::default()
            })
            .unwrap();
        }
        w.commit().unwrap();
        let ids = |rooms: Vec<Room>| rooms.iter().map(|r| r.id).collect::<Vec<_>>();

        assert_eq!(ids(store.rooms_by_owner(0).unwrap()), [1]);
        assert_eq!(ids(store.rooms_by_owner(1).unwrap()), [10, 11]);
        assert_eq!(store.room_count_by_category(112).unwrap(), 3);
        // Most visitors, then best rating.
        assert_eq!(ids(store.recent_rooms(112, 30).unwrap()), [12, 11, 10]);
        assert_eq!(ids(store.recent_rooms(112, 2).unwrap()), [12, 11]);

        // Without an owner the query matches room names and owner names.
        assert_eq!(ids(store.search_rooms("lounge", -1, 100).unwrap()), [12]);
        assert_eq!(ids(store.search_rooms("ALICE", -1, 100).unwrap()), [11, 10]);
        // With an owner both conditions must hold.
        assert_eq!(ids(store.search_rooms("", 2, 100).unwrap()), [12, 13]);
        assert!(store.search_rooms("zone", 2, 100).unwrap().is_empty());
        assert!(store.search_rooms("", -1, 100).unwrap().is_empty());
        // An unknown owner (0) with no text is `LIKE '%%'`: every room that has an owner.
        assert_eq!(store.search_rooms("", 0, 100).unwrap().len(), 4);
        // Wildcards of the query are live, as in SQL.
        assert_eq!(ids(store.search_rooms("100%", -1, 100).unwrap()), [13]);

        // Changing owner and category moves the index entries.
        store
            .update_room(11, |r| {
                r.owner_id = 2;
                r.category = 114;
            })
            .unwrap();
        assert_eq!(ids(store.rooms_by_owner(1).unwrap()), [10]);
        assert_eq!(ids(store.rooms_by_owner(2).unwrap()), [11, 12, 13]);
        assert_eq!(store.room_count_by_category(112).unwrap(), 2);
        assert_eq!(store.room_count_by_category(114).unwrap(), 2);
    }

    #[test]
    fn like_matching() {
        assert!(like("%lou%", "Lounge"));
        assert!(like("l_unge", "LOUNGE"));
        assert!(!like("lounge", "lounges"));
        assert!(like("%", ""));
        assert!(like("a%b%c", "a-b-b-c"));
        assert!(!like("a%b%c", "a-b-b"));
        assert!(like("100\\%", "100%"));
        assert!(!like("100\\%", "1000"));
    }
}
