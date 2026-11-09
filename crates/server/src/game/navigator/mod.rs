//! The navigator (Java: `game/navigator`: `NavigatorManager`, `NavigatorCategory`, plus the room
//! list helpers of `RoomManager`: `replaceQueryRooms`, `sortRooms`, `ratingSantiyCheck`).
//!
//! The categories live in `World::room_categories`. A list of rooms is built the way Java does it:
//! a DAO query gives stored rows, and every row whose room is loaded is replaced by the loaded
//! room's live data. Each loaded room is locked on its own and only long enough to copy its row,
//! and never while the registry lock is held (see `World::room_visitors`).
//!
//! `RoomData.getVisitorsNow/Max` add the players and capacity of started BattleBall and SnowStorm
//! games to their lobby rooms ([`game_visitors`]).

use std::collections::HashMap;

use rusbbo_storage::{Room, RoomCategory};

use crate::game::fuserights;
use crate::world::World;

/// Java: `RoomManager.PUBLIC_ROOM_OFFSET`: public rooms are addressed as `room id + 1000` in
/// `NAVNODEINFO` and in `NAVIGATE` when following a user into a public room.
pub const PUBLIC_ROOM_OFFSET: i32 = 1000;

/// Java: `RoomDao.SHOCKWAVE_SEARCH_LIMIT`.
pub const SEARCH_LIMIT: usize = 100;
/// Java: the limit `NAVIGATE` passes to `NavigatorDao.getRecentRooms`.
pub const RECENT_ROOMS_LIMIT: usize = 30;

/// The part of the player a navigator list depends on.
#[derive(Clone, Debug)]
pub struct Viewer {
    pub user_id: u32,
    pub name: String,
    pub rank: u8,
    pub has_club: bool,
}

impl Viewer {
    /// Java: `player.hasFuse(Fuseright.SEE_ALL_ROOMOWNERS)`.
    pub fn sees_all_owners(&self) -> bool {
        fuserights::has_fuse(self.rank, self.has_club, "fuse_see_all_roomowners")
    }
}

/// What the composers need to know of a room: Java's `Room` + `RoomData` after
/// `replaceQueryRooms`, copied so no lock is held while composing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomEntry {
    pub id: u32,
    pub owner_id: u32,
    /// `users.username` of the owner, empty when there is none (Java: `RoomDao.fill`).
    pub owner_name: String,
    pub name: String,
    pub description: String,
    pub access_type: i32,
    pub category: i32,
    pub ccts: String,
    pub show_owner_name: bool,
    pub is_hidden: bool,
    pub rating: i32,
    /// Java: `RoomData.getVisitorsNow`.
    pub visitors_now: i32,
    /// Java: `RoomData.getVisitorsMax`.
    pub visitors_max: i32,
    /// Java: `RoomData.getTotalVisitorsNow`: this room plus the rooms behind its walkways.
    pub total_visitors_now: i32,
    /// Java: `RoomData.getTotalVisitorsMax`.
    pub total_visitors_max: i32,
}

impl RoomEntry {
    /// Java: `Room.isPublicRoom`.
    pub fn is_public(&self) -> bool {
        self.owner_id == 0
    }

    /// Java: `RoomData.getAccessType`.
    pub fn access_type_name(&self) -> &'static str {
        match self.access_type {
            2 => "password",
            1 => "closed",
            _ => "open",
        }
    }
}

/// Java: `NavigatorManager.categoryMap.values()`: a `HashMap<Integer, _>` filled in
/// `order_id` order. Its iteration order (bucket by `hash & (capacity - 1)`, insertion order inside
/// a bucket) decides how categories with the same `order_id` are listed.
pub fn categories_in_map_order(world: &World) -> Vec<RoomCategory> {
    let mut list: Vec<RoomCategory> = world.room_categories().values().cloned().collect();
    list.sort_by_key(|c| (c.order_id, c.id)); // the insertion order: ORDER BY order_id ASC
    let mut capacity = 16usize;
    while list.len() * 4 > capacity * 3 {
        capacity *= 2;
    }
    let bucket = |id: u32| ((id ^ (id >> 16)) as usize) & (capacity - 1);
    list.sort_by_key(|c| bucket(c.id)); // stable: keeps the insertion order inside a bucket
    list
}

/// Java: `NavigatorManager.getCategoriesByParentId`.
pub fn categories_by_parent(world: &World, parent_id: i32, rank: u8) -> Vec<RoomCategory> {
    let hide_empty = world
        .settings
        .read()
        .bool("navigator.hide.empty.public.categories");
    let mut categories: Vec<RoomCategory> = categories_in_map_order(world)
        .into_iter()
        .filter(|c| !(hide_empty && c.public_spaces && room_count(world, c) == 0))
        .filter(|c| c.minrole_access <= i32::from(rank))
        .filter(|c| c.parent_id == parent_id)
        .collect();
    categories.sort_by_key(|c| c.order_id);
    categories
}

/// Java: `NavigatorCategory.getRoomCount`: the stored rooms of a public category, the loaded
/// rooms of a guest category.
pub fn room_count(world: &World, category: &RoomCategory) -> usize {
    if category.public_spaces {
        world
            .store
            .room_count_by_category(category.id as i32)
            .unwrap_or(0)
    } else {
        loaded_rooms_in(world, category.id as i32).len()
    }
}

/// Java: `NavigatorCategory.getCurrentVisitors` and `getMaxVisitors`, as `(now, max)`.
pub fn category_visitors(world: &World, category: &RoomCategory) -> (i32, i32) {
    let rooms = if category.public_spaces {
        let rows = world
            .store
            .rooms_by_category(category.id as i32)
            .unwrap_or_default();
        replace_query_rooms(world, rows)
    } else {
        // Java walks every loaded room and keeps those of the category.
        loaded_rooms_in(world, category.id as i32)
    };
    rooms
        .iter()
        .filter(|r| r.category == category.id as i32)
        .fold((0, 0), |(now, max), r| {
            (now + r.visitors_now, max + r.visitors_max)
        })
}

/// The loaded rooms of a category (Java: `RoomManager.getRooms()` filtered by category), one room
/// lock at a time.
fn loaded_rooms_in(world: &World, category: i32) -> Vec<Room> {
    world
        .loaded_rooms()
        .into_iter()
        .map(|room| room.lock().data.clone())
        .filter(|data| data.category == category)
        .collect()
}

/// Java: `RoomManager.replaceQueryRooms`: stored rows, with the live data of the rooms that are
/// loaded in place of their row.
pub fn replace_query_rooms(world: &World, rows: Vec<Room>) -> Vec<Room> {
    rows.into_iter()
        .map(|row| match world.loaded_room(row.id) {
            Some(room) => room.lock().data.clone(),
            None => row,
        })
        .map(|mut data| {
            let (now, max) = game_visitors(world, &data);
            data.visitors_now += now;
            data.visitors_max += max;
            data
        })
        .collect()
}

/// Java: what `RoomData.getVisitorsNow/getVisitorsMax` add for a game lobby: the players of its
/// started games (see `GameManager::lobby_visitors`). Takes the games' lock: hold no room lock.
pub fn game_visitors(world: &World, data: &Room) -> (i32, i32) {
    match world.room_models().get(&data.model) {
        Some(model) => world.games.lobby_visitors(&model.trigger),
        None => (0, 0),
    }
}

/// `replaceQueryRooms` plus everything a composer reads from the rooms: owner names (the
/// `LEFT JOIN users` of the DAO queries) and the walkway totals.
pub fn room_entries(world: &World, rows: Vec<Room>) -> Vec<RoomEntry> {
    let mut owner_names: HashMap<u32, String> = HashMap::new();
    let show_hidden = world.settings.read().bool("navigator.show.hidden.rooms");
    replace_query_rooms(world, rows)
        .into_iter()
        .map(|data| {
            let owner_name = owner_names
                .entry(data.owner_id)
                .or_insert_with(|| {
                    world
                        .store
                        .user(data.owner_id)
                        .ok()
                        .flatten()
                        .map(|u| u.username)
                        .unwrap_or_default()
                })
                .clone();
            let (mut total_now, mut total_max) = (data.visitors_now, data.visitors_max);
            for child in world.child_rooms(data.id, data.owner_id == 0) {
                if let Some((now, max)) = world.room_visitors(child) {
                    total_now += now;
                    total_max += max;
                }
            }
            RoomEntry {
                id: data.id,
                owner_id: data.owner_id,
                owner_name,
                name: data.name,
                description: data.description,
                access_type: data.accesstype,
                category: data.category,
                ccts: data.ccts.unwrap_or_default(),
                show_owner_name: data.showname,
                // Java: RoomData.isNavigatorHide.
                is_hidden: data.is_hidden && !show_hidden,
                rating: data.rating,
                visitors_now: data.visitors_now,
                visitors_max: data.visitors_max,
                total_visitors_now: total_now,
                total_visitors_max: total_max,
            }
        })
        .collect()
}

/// Java: `ADD_FAVORITE_ROOM.MAX_FAVOURITES`.
pub const MAX_FAVOURITES: usize = 30;

/// Java: `RoomManager.getRoomById(id) != null`: a loaded room or a stored one.
pub fn room_exists(world: &World, id: u32) -> bool {
    world.loaded_room(id).is_some() || world.store.room(id).ok().flatten().is_some()
}

/// Java: `RoomManager.getFavouriteRooms(userId, false)`: the user's favourite rooms that exist,
/// the newest favourite first (the stored order, by room id, reversed), as stored rows (a loaded
/// room's live row).
pub fn favourite_rooms(world: &World, user_id: u32) -> Vec<Room> {
    let mut ids = world.store.room_favourites(user_id).unwrap_or_else(|e| {
        tracing::error!(user = user_id, "loading favourite rooms: {e}");
        Vec::new()
    });
    ids.reverse();
    ids.into_iter()
        .filter_map(|id| match world.loaded_room(id) {
            Some(room) => Some(room.lock().data.clone()),
            None => world.store.room(id).ok().flatten(),
        })
        .collect()
}

/// Java: `RoomManager.sortRooms`: most visitors (walkway totals included) first, then best rated.
/// The sort is stable, like `List.sort`, so ties keep the order of the query.
pub fn sort_rooms(rooms: &mut [RoomEntry]) {
    rooms.sort_by_key(|r| std::cmp::Reverse((r.total_visitors_now, r.rating)));
}

/// Java: `RoomManager.ratingSantiyCheck`: for the flats nobody is in, the expired votes are
/// deleted and the rating is counted again; a changed rating is stored (only in the row, as in
/// Java: a loaded room keeps showing the one it has, and this list was composed before). The
/// first room with no rating ends the check (Java's `return` where `continue` was meant).
pub fn rating_sanity_check(world: &World, rooms: &[RoomEntry]) {
    for room in rooms {
        if room.is_public() || room.visitors_now > 0 {
            continue;
        }
        if room.rating <= 0 {
            return;
        }
        let now = crate::game::now();
        if let Err(e) = world.store.remove_expired_room_votes(room.id, now) {
            tracing::error!(room = room.id, "removing expired votes: {e}");
        }
        let rating = crate::game::room::counted_votes(world, room.id)
            .iter()
            .map(|v| v.vote)
            .sum::<i32>()
            .max(0);
        if rating != room.rating {
            if let Err(e) = world.store.update_room(room.id, |r| r.rating = rating) {
                tracing::error!(room = room.id, "saving the rating: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, total_now: i32, rating: i32) -> RoomEntry {
        RoomEntry {
            id,
            total_visitors_now: total_now,
            rating,
            ..Default::default()
        }
    }

    #[test]
    fn rooms_sort_by_visitors_then_rating_descending_and_stably() {
        let mut rooms = vec![
            entry(1, 0, 0),
            entry(2, 3, 1),
            entry(3, 0, 5),
            entry(4, 3, 4),
            entry(5, 0, 0),
        ];
        sort_rooms(&mut rooms);
        let ids: Vec<u32> = rooms.iter().map(|r| r.id).collect();
        assert_eq!(ids, [4, 2, 3, 1, 5]);
    }

    #[test]
    fn access_type_names() {
        let mut room = entry(1, 0, 0);
        assert_eq!(room.access_type_name(), "open");
        room.access_type = 1;
        assert_eq!(room.access_type_name(), "closed");
        room.access_type = 2;
        assert_eq!(room.access_type_name(), "password");
    }
}
