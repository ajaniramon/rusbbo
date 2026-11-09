//! Java: `messages/flash/incoming`: the Flash client's navigator, room settings, wardrobe and
//! moderation tool.

use std::cmp::Reverse;
use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;
use rusbbo_storage::Room;
use tracing::error;

use super::latin1;
use crate::game::navigator::flash::{FlashRoom, flash_rooms, follow_redirect};
use crate::game::navigator::{self, RoomEntry};
use crate::game::room::current_room;
use crate::messages::Malformed;
use crate::messages::outgoing::flash as out;
use crate::session::Session;
use crate::world::World;

/// Java: `RoomManager.replaceQueryRooms`, `sortRooms` and `ratingSantiyCheck` on a query.
fn listed(world: &World, rows: Vec<Room>, sort: bool) -> Vec<FlashRoom> {
    let mut entries = navigator::room_entries(world, rows);
    if sort {
        navigator::sort_rooms(&mut entries);
    }
    navigator::rating_sanity_check(world, &entries);
    flash_rooms(world, entries)
}

/// `FLASH_FLAT_RESULTS` with no events shown.
fn send_flat_results(session: &Session, rooms: &[FlashRoom]) {
    if let Some(msg) = out::flat_results(rooms, |_| None) {
        session.send(msg);
    }
}

fn user_id(session: &Session) -> Option<u32> {
    session.player.lock().user_id()
}

/// Java: `NavigatorDao.getRopularRooms(30, false)`: occupied guest rooms, most visitors first.
fn popular_rooms(world: &World) -> Vec<Room> {
    let mut rooms: Vec<Room> = world
        .store
        .all_rooms()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.owner_id > 0 && r.visitors_now > 0)
        .collect();
    rooms.sort_by_key(|r| (Reverse(r.visitors_now), Reverse(r.rating)));
    rooms.truncate(30);
    rooms
}

/// Java: `RoomDao.getHighestRatedRooms(30, 0)`.
fn highest_rated_rooms(world: &World) -> Vec<Room> {
    let mut rooms: Vec<Room> = world
        .store
        .all_rooms()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.owner_id > 0)
        .collect();
    rooms.sort_by_key(|r| Reverse(r.rating));
    rooms.truncate(30);
    rooms
}

/// Java: `NavigatorDao.getRecentlyVisited(30, user)`: the user's visits to guest rooms whose
/// owner exists, the latest first.
fn recently_visited(world: &World, user_id: u32) -> Vec<Room> {
    let mut visits = world
        .store
        .records_where::<rusbbo_storage::RoomVisit>(|v| v.user_id == user_id)
        .unwrap_or_default();
    visits.sort_by_key(|v| Reverse(v.visited_at));
    visits
        .into_iter()
        .filter_map(|v| world.store.room(v.room_id).ok().flatten())
        .filter(|r| r.owner_id > 0 && world.store.user(r.owner_id).ok().flatten().is_some())
        .take(30)
        .collect()
}

/// Java: `NavigatorDao.getFriendRooms(30, friends)`: the friends' rooms, most visitors first.
fn friend_rooms(world: &World, friends: &[u32]) -> Vec<Room> {
    if friends.is_empty() {
        return Vec::new();
    }
    let mut rooms: Vec<Room> = world
        .store
        .all_rooms()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| friends.contains(&r.owner_id))
        .filter(|r| world.store.user(r.owner_id).ok().flatten().is_some())
        .collect();
    rooms.sort_by_key(|r| (Reverse(r.visitors_now), Reverse(r.rating)));
    rooms.truncate(30);
    rooms
}

/// The friend ids of the player's messenger.
fn friend_ids(session: &Session) -> Vec<u32> {
    session
        .player
        .lock()
        .messenger
        .as_ref()
        .map(|m| m.friends.keys().copied().collect())
        .unwrap_or_default()
}

/// Java: the rooms the player's online friends are in (guest rooms only, each once).
fn rooms_of_friends_online(world: &World, session: &Session) -> Vec<Room> {
    let friends: Vec<(u32, bool)> = session
        .player
        .lock()
        .messenger
        .as_ref()
        .map(|m| {
            m.friends
                .values()
                .map(|f| (f.user_id, f.online_status_visible))
                .collect()
        })
        .unwrap_or_default();
    let mut rooms: Vec<Room> = Vec::new();
    for (friend, visible) in friends {
        if !visible {
            continue;
        }
        let Some(online) = world.player_by_id(friend) else {
            continue;
        };
        let Some(room_id) = online.player.lock().room_id else {
            continue;
        };
        let Some(room) = world.loaded_room(room_id) else {
            continue;
        };
        let data = room.lock().data.clone();
        if data.owner_id > 0 && !rooms.iter().any(|r| r.id == data.id) {
            rooms.push(data);
        }
    }
    rooms
}

/// Java: `FLASH_PUBLICROOMS`: the public rooms with a navigator style.
pub fn public_rooms(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let rows = world.store.rooms_by_owner(0).unwrap_or_default();
    let mut entries: Vec<RoomEntry> = navigator::room_entries(world, rows)
        .into_iter()
        .filter(|r| !r.is_hidden)
        .collect();
    let styles = world
        .store
        .records::<rusbbo_storage::NavigatorStyle>()
        .unwrap_or_default();
    entries.retain(|r| styles.iter().any(|s| s.id == r.id));
    navigator::sort_rooms(&mut entries);
    let rooms = flash_rooms(world, entries);
    let listed: Vec<out::FrontpageRoom> = rooms
        .iter()
        .filter_map(|room| {
            let style = styles.iter().find(|s| s.id == room.entry.id)?;
            Some(out::FrontpageRoom {
                room,
                description: style.description.clone(),
                thumbnail_url: style.thumbnail_url.clone(),
                thumbnail_layout: style.thumbnail_layout,
            })
        })
        .collect();
    session.send(out::frontpage_result(&listed));
    Ok(())
}

/// Java: `FLASH_GETGUESTROOMS`: one of the navigator's lists by mode.
pub fn get_guest_rooms(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let force_display = reader.int().ok_or(Malformed)?;
    let mode = reader.int().ok_or(Malformed)?;
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    let rows: Vec<Room> = match mode {
        1 => navigator::replace_query_rooms(world, popular_rooms(world)),
        2 => navigator::replace_query_rooms(world, highest_rated_rooms(world)),
        5 => navigator::replace_query_rooms(
            world,
            world.store.rooms_by_owner(user_id).unwrap_or_default(),
        ),
        6 => navigator::favourite_rooms(world, user_id)
            .into_iter()
            .filter(|r| r.owner_id > 0)
            .collect(),
        7 => navigator::replace_query_rooms(world, recently_visited(world, user_id)),
        4 => rooms_of_friends_online(world, session),
        3 => navigator::replace_query_rooms(world, friend_rooms(world, &friend_ids(session))),
        _ => Vec::new(),
    };
    let rooms = listed(world, rows, true);
    if let Some(msg) = out::guest_room_result(force_display, mode, &rooms) {
        session.send(msg);
    }
    Ok(())
}

/// Java: `FLASH_POPULARTAGS`: the 50 most used room tags (a `HashMap`, in its order).
pub fn popular_tags(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut counts: Vec<(String, i32)> = Vec::new();
    for t in world
        .store
        .records_where::<rusbbo_storage::Tag>(|t| t.room_id > 0)
        .unwrap_or_default()
    {
        match counts
            .iter_mut()
            .find(|(tag, _)| tag.to_lowercase() == t.tag.to_lowercase())
        {
            Some(entry) => entry.1 += 1,
            None => counts.push((t.tag, 1)),
        }
    }
    counts.sort_by_key(|(_, n)| Reverse(*n));
    counts.truncate(50);
    let ordered = crate::web::data::java_hashmap_order(counts);
    session.send(out::popular_tags(&ordered));
    Ok(())
}

/// Java: `SET_HOME_ROOM`: one of the player's own guest rooms, or none.
pub fn set_home_room(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut room_id = reader.int().ok_or(Malformed)?;
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    if room_id > 0 {
        let Some(room) = world.store.room(room_id as u32).ok().flatten() else {
            return Ok(());
        };
        if room.owner_id == 0 || room.owner_id != user_id {
            return Ok(());
        }
    } else {
        room_id = 0;
    }
    if let Err(e) = world.store.update_user(user_id, |u| u.home_room = room_id) {
        error!(user = user_id, "saving the home room: {e}");
    }
    if let Some(d) = session.player.lock().details.as_mut() {
        d.home_room = room_id;
    }
    session.send(out::home_room(room_id));
    Ok(())
}

/// The navigator view of one room (Java: `RoomManager.getRoomById`, loaded or stored).
fn flash_room(world: &World, room_id: u32) -> Option<FlashRoom> {
    let data = match world.loaded_room(room_id) {
        Some(room) => room.lock().data.clone(),
        None => world.store.room(room_id).ok().flatten()?,
    };
    let entries = navigator::room_entries(world, vec![data]);
    flash_rooms(world, entries).into_iter().next()
}

/// Java: `FLASH_ROOM_INFO`: a room's details before entering (`FLASH_FLATINFO`).
pub fn room_info(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(room) = u32::try_from(room_id)
        .ok()
        .and_then(|id| flash_room(world, id))
    else {
        return Ok(());
    };
    let is_loading = reader.bool().ok_or(Malformed)?;
    let check_entry = reader.bool().ok_or(Malformed)?;
    let (user_id, authenticated, see_owners) = {
        let p = session.player.lock();
        (
            p.user_id().unwrap_or(0),
            p.authenticate_id == Some(room.entry.id),
            p.has_fuse("fuse_see_all_roomowners"),
        )
    };
    let owner = room.entry.owner_id == user_id;
    let owner_name = if owner || room.entry.show_owner_name || see_owners {
        room.entry.owner_name.clone()
    } else {
        "-".to_string()
    };
    if let Some(msg) = out::flat_info(
        &room,
        &owner_name,
        owner || authenticated,
        is_loading,
        check_entry,
    ) {
        session.send(msg);
    }
    Ok(())
}

/// Java: `FLASH_ROOMICONDATA`: the owner draws the room's navigator icon.
pub fn room_icon_data(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    let Some(room) = u32::try_from(room_id)
        .ok()
        .and_then(|id| flash_room(world, id))
    else {
        return Ok(());
    };
    if room.entry.owner_id != user_id {
        return Ok(());
    }
    let background = reader.int().ok_or(Malformed)?;
    let top_layer = reader.int().ok_or(Malformed)?;
    let amount = reader.int().ok_or(Malformed)?;
    // Java: a HashMap<Integer, Integer> (small keys: in key order).
    let mut items: std::collections::BTreeMap<i32, i32> = std::collections::BTreeMap::new();
    for _ in 0..amount {
        let position = reader.int().ok_or(Malformed)?;
        let item = reader.int().ok_or(Malformed)?;
        if !(0..=10).contains(&position) || !(1..=27).contains(&item) {
            return Ok(());
        }
        if items.contains_key(&position) {
            return Ok(());
        }
        items.insert(position, item);
    }
    if !(1..=24).contains(&background) || !(0..=11).contains(&top_layer) {
        return Ok(());
    }
    let listed: Vec<String> = items.iter().map(|(k, v)| format!("{k},{v}")).collect();
    let icon = format!("{background}|{top_layer}|{}", listed.join(" "));
    let id = room.entry.id;
    if let Some(loaded) = world.loaded_room(id) {
        loaded.lock().data.icon_data = icon.clone();
    }
    if let Err(e) = world.store.update_room(id, |r| r.icon_data = icon) {
        error!(room = id, "saving the room icon: {e}");
    }
    session.send(out::room_updated(467, room_id));
    session.send(out::room_updated(456, room_id));
    Ok(())
}

/// Java: `FLASH_CANCREATEROOM`.
pub fn can_create_room(
    _: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    session.send(out::can_create_room());
    Ok(())
}

/// Java: `FLASH_GETPUBLICROOMDATA`: a public room's casts (a hidden one answers with the room
/// whose walkway leads to it).
pub fn get_public_room_data(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = if reader.remaining().is_empty() {
        // Java: `getRoom().getId()` throws outside a room.
        let Some(room) = current_room(world, session) else {
            return Ok(());
        };
        room.id as i32
    } else {
        reader.int().ok_or(Malformed)?
    };
    let Some(room) = u32::try_from(room_id)
        .ok()
        .and_then(|id| flash_room(world, id))
    else {
        return Ok(());
    };
    let mut answer = room_id;
    if room.entry.is_hidden {
        answer = follow_redirect(world, room.entry.id) as i32;
    }
    session.send(out::public_room_casts(answer, &room.entry.ccts));
    Ok(())
}

/// Java: `FLASH_ROOMEDITDATA`: the owner's settings window.
pub fn room_edit_data(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    let Some(room) = u32::try_from(room_id)
        .ok()
        .and_then(|id| flash_room(world, id))
    else {
        return Ok(());
    };
    if room.entry.owner_id != user_id {
        return Ok(());
    }
    // Java: a room that is not loaded has no rights list yet.
    let rights: Vec<u32> = world
        .loaded_room(room.entry.id)
        .map(|r| r.lock().rights.clone())
        .unwrap_or_default();
    let named: Vec<(u32, String)> = rights
        .into_iter()
        .map(|id| {
            let name = world
                .store
                .user(id)
                .ok()
                .flatten()
                .map(|u| u.username)
                .unwrap_or_default();
            (id, name)
        })
        .collect();
    session.send(out::edit_data(&room, &named));
    Ok(())
}

/// Java: `FLASH_ROOMSAVEDATA`: the owner saves the room's settings and tags.
pub fn room_save_data(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    use crate::web::text::filter_input;
    let room_id = reader.int().ok_or(Malformed)?;
    let (user_id, rank) = {
        let p = session.player.lock();
        let Some(d) = p.details.as_ref() else {
            return Ok(());
        };
        (d.id, d.rank)
    };
    let Some(room) = u32::try_from(room_id)
        .ok()
        .and_then(|id| flash_room(world, id))
    else {
        return Ok(());
    };
    if room.entry.owner_id != user_id {
        return Ok(());
    }
    let text = |r: &mut ClientReader| -> Result<String, Malformed> {
        Ok(filter_input(&latin1(r.string().ok_or(Malformed)?), true))
    };
    let name = text(reader)?;
    let description = text(reader)?;
    let mut state = reader.int().ok_or(Malformed)?;
    let password = text(reader)?;
    let mut max_users = reader.int().ok_or(Malformed)?;
    if name.trim().is_empty() {
        return Ok(());
    }
    reader.bool().ok_or(Malformed)?;
    reader.bool().ok_or(Malformed)?;
    reader.bool().ok_or(Malformed)?;
    let category_id = reader.int().ok_or(Malformed)?;
    let tag_count = reader.int().ok_or(Malformed)?;
    if !(10..=50).contains(&max_users) {
        max_users = 25;
    }
    let categories = world.room_categories();
    let fallback = || categories.get(&2);
    let mut category = u32::try_from(category_id)
        .ok()
        .and_then(|c| categories.get(&c))
        .or_else(fallback);
    if category.is_some_and(|c| c.minrole_setflatcat > i32::from(rank)) {
        category = fallback();
    }
    if category.is_some_and(|c| c.is_node || c.public_spaces) {
        category = fallback();
    }
    // Java: without category 2 `getId()` throws here.
    let Some(category) = category else {
        return Ok(());
    };
    let category_id = category.id as i32;
    if !(0..=2).contains(&state) {
        state = 0;
    }
    let id = room.entry.id;
    // Java: TagDao.removeTags(0, roomId, 0), then the new tags.
    let old = world
        .store
        .records_where::<rusbbo_storage::Tag>(|t| {
            t.user_id == 0 && t.room_id == id && t.group_id == 0
        })
        .unwrap_or_default();
    for t in old {
        let _ = world.store.delete_record::<rusbbo_storage::Tag>(t.id);
    }
    if tag_count > 0 && tag_count < 3 {
        for _ in 0..tag_count {
            let raw = text(reader)?;
            if let Some(tag) = crate::web::controllers::tags::valid_tag(world, &raw, 0, id, 0) {
                let _ = crate::web::controllers::tags::add_tag(world, &tag, 0, id, 0);
            }
        }
    }
    let change = |r: &mut Room| {
        r.name = name.clone();
        r.description = description.clone();
        r.accesstype = state;
        r.password = Some(password.clone());
        r.visitors_max = max_users;
        r.category = category_id;
    };
    if let Some(loaded) = world.loaded_room(id) {
        change(&mut loaded.lock().data);
    }
    if let Err(e) = world.store.update_room(id, change) {
        error!(room = id, "saving the room settings: {e}");
    }
    session.send(out::room_updated(467, room_id));
    session.send(out::room_updated(456, room_id));
    Ok(())
}

/// Java: `FLASH_POPULARROOMS`: a category's busiest rooms, or (`-1`) the busiest of all.
pub fn popular_rooms_list(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let value = latin1(reader.string().ok_or(Malformed)?);
    let mut mode = -1;
    let mut rows: Vec<Room> = Vec::new();
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        // Java: `Integer.parseInt` throws above an int.
        let Ok(parsed) = value.parse::<i32>() else {
            return Ok(());
        };
        mode = parsed;
        if u32::try_from(mode)
            .ok()
            .is_some_and(|c| world.room_categories().contains_key(&c))
        {
            rows.extend(navigator::replace_query_rooms(
                world,
                world.store.recent_rooms(mode, 30).unwrap_or_default(),
            ));
        }
    }
    if mode == -1 {
        rows.extend(navigator::replace_query_rooms(world, popular_rooms(world)));
    }
    let rooms = listed(world, rows, true);
    send_flat_results(session, &rooms);
    Ok(())
}

/// Java: `FLASH_HIGHESTVOTEDROOMS`.
pub fn highest_voted_rooms(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let rows = navigator::replace_query_rooms(world, highest_rated_rooms(world));
    send_flat_results(session, &listed(world, rows, true));
    Ok(())
}

/// Java: `FLASH_ROOMS_FRIENDS_OWN`: the friends' rooms.
pub fn rooms_friends_own(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let rows = navigator::replace_query_rooms(world, friend_rooms(world, &friend_ids(session)));
    send_flat_results(session, &listed(world, rows, true));
    Ok(())
}

/// Java: `FLASH_FRIENDS_IN_ROOM`: the rooms the online friends are in.
pub fn friends_in_room(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let rows = rooms_of_friends_online(world, session);
    send_flat_results(session, &listed(world, rows, true));
    Ok(())
}

/// Java: `FLASH_USERFLATS`: the player's own rooms.
pub fn user_flats(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    let rows = navigator::replace_query_rooms(
        world,
        world.store.rooms_by_owner(user_id).unwrap_or_default(),
    );
    send_flat_results(session, &listed(world, rows, true));
    Ok(())
}

/// Java: `FLASH_FAVOURITE_ROOMS`: the favourite guest rooms.
pub fn favourite_rooms(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    let rows: Vec<Room> = navigator::favourite_rooms(world, user_id)
        .into_iter()
        .filter(|r| r.owner_id > 0)
        .collect();
    send_flat_results(session, &listed(world, rows, true));
    Ok(())
}

/// Java: `FLASH_ROOMS_RECENTLY_VISTED` (not sorted again).
pub fn rooms_recently_visited(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    let rows = navigator::replace_query_rooms(world, recently_visited(world, user_id));
    send_flat_results(session, &listed(world, rows, false));
    Ok(())
}

/// Java: `FLASH_SEARCH`: rooms by name or owner (`owner:name` narrows to one owner).
pub fn search(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut query = latin1(reader.string().ok_or(Malformed)?);
    let mut owner = -1;
    if query.contains("owner:") {
        let tag = query.split(' ').find(|s| s.starts_with("owner:"));
        if let Some(tag) = tag {
            let name = tag.replacen("owner:", "", 1);
            owner = world
                .store
                .user_by_name(&name)
                .ok()
                .flatten()
                .map_or(-1, |u| u.id as i32);
        }
        query = query
            .split(' ')
            .filter(|s| !s.to_lowercase().starts_with("owner:"))
            .collect::<Vec<_>>()
            .join(",");
    }
    let rows = navigator::replace_query_rooms(
        world,
        world
            .store
            .search_rooms(&query, owner, navigator::FLASH_SEARCH_LIMIT)
            .unwrap_or_default(),
    );
    let rooms = listed(world, rows, true);
    if let Some(msg) = out::search_results(&rooms, &query) {
        session.send(msg);
    }
    Ok(())
}

/// Java: `FLASH_SEARCH_TAGS`: rooms with a tag starting like the search (30 at most).
pub fn search_tags(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let tag = latin1(reader.string().ok_or(Malformed)?);
    let prefix = tag.to_lowercase();
    let mut rows: Vec<Room> = Vec::new();
    for t in world
        .store
        .records::<rusbbo_storage::Tag>()
        .unwrap_or_default()
    {
        if rows.len() >= 30 {
            break;
        }
        if !t.tag.to_lowercase().starts_with(&prefix) || t.room_id == 0 {
            continue;
        }
        // INNER JOIN rooms and the owner (one row per matching tag).
        let Some(room) = world.store.room(t.room_id).ok().flatten() else {
            continue;
        };
        if world.store.user(room.owner_id).ok().flatten().is_none() {
            continue;
        }
        rows.push(room);
    }
    let rows = navigator::replace_query_rooms(world, rows);
    let rooms = listed(world, rows, true);
    if let Some(msg) = out::search_results(&rooms, &tag) {
        session.send(msg);
    }
    Ok(())
}

/// Java: `FLASH_EVENTROOMS`: the rooms with an event of a category (`-1`: every event), the
/// events shown.
pub fn event_rooms(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let value = latin1(reader.string().ok_or(Malformed)?);
    // Java: `Integer.parseInt` throws on anything else.
    let Ok(mode) = value.parse::<i32>() else {
        return Ok(());
    };
    let events = if mode == -1 {
        world.events.event_list()
    } else {
        crate::game::events::events_of_type(world, mode)
    };
    let mut rows: Vec<Room> = Vec::new();
    for e in &events {
        // Java: a missing room is a null in the list, and sorting it throws.
        let Some(room) = world
            .loaded_room(e.room_id)
            .map(|r| r.lock().data.clone())
            .or_else(|| world.store.room(e.room_id).ok().flatten())
        else {
            return Ok(());
        };
        rows.push(room);
    }
    let mut entries = navigator::room_entries(world, rows);
    navigator::sort_rooms(&mut entries);
    navigator::rating_sanity_check(world, &entries);
    let rooms = flash_rooms(world, entries);
    let lifetime = crate::game::events::event_lifetime(world);
    let msg = out::flat_results(&rooms, |room_id| {
        let event = world.events.event_by_room(room_id)?;
        let started = crate::game::format_local(event.expire - lifetime, "%I:%M %p")
            .replace('.', "")
            .to_uppercase();
        Some(out::FlatEvent {
            name: event.name,
            category_id: event.category_id,
            started,
        })
    });
    if let Some(msg) = msg {
        session.send(msg);
    }
    Ok(())
}

/// Java: `FLASH_GETWARDROBE`.
pub fn get_wardrobe(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    let slots: Vec<(i32, String, String)> = world
        .store
        .records_where::<rusbbo_storage::Wardrobe>(|w| w.user_id == user_id)
        .unwrap_or_default()
        .into_iter()
        .map(|w| (w.slot_id, w.figure, w.sex))
        .collect();
    session.send(out::wardrobe(&slots));
    Ok(())
}

/// Java: `FLASH_SAVEWARDROBE`: a look into one of the five slots.
pub fn save_wardrobe(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let slot = reader.int().ok_or(Malformed)?;
    let figure = latin1(reader.string().ok_or(Malformed)?);
    let sex = if reader.string().ok_or(Malformed)? == b"M" {
        "M"
    } else {
        "F"
    };
    if !(1..=5).contains(&slot) {
        return Ok(());
    }
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    let existing = world
        .store
        .records_where::<rusbbo_storage::Wardrobe>(|w| w.user_id == user_id && w.slot_id == slot)
        .unwrap_or_default();
    let result = match existing.into_iter().next() {
        Some(mut w) => {
            w.figure = figure;
            w.sex = sex.to_string();
            world.store.put_record(&w)
        }
        None => {
            let mut w = rusbbo_storage::Wardrobe {
                user_id,
                slot_id: slot,
                sex: sex.to_string(),
                figure,
                ..Default::default()
            };
            world.store.insert_record(&mut w).map(|_| ())
        }
    };
    if let Err(e) = result {
        error!(user = user_id, "saving the wardrobe: {e}");
    }
    Ok(())
}

/// Java: `FLASH_MODTOOL_ROOMINFO`: the room a moderator is in.
pub fn modtool_room_info(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let rank = session.player.lock().details.as_ref().map_or(0, |d| d.rank);
    if rank < crate::game::commands::RANK_MODERATOR {
        return Ok(());
    }
    let (id, owner_id, owner_in_room) = {
        let r = room.lock();
        let owner = r.data.owner_id;
        (
            r.id(),
            owner,
            r.entities
                .iter()
                .any(|e| e.is_player() && e.details.user_id == owner),
        )
    };
    let _ = owner_id;
    let Some(view) = flash_room(world, id) else {
        return Ok(());
    };
    let event = world.events.event_by_room(id);
    let event_ref = event
        .as_ref()
        .map(|e| (e.name.as_str(), e.description.as_str(), e.tags.as_slice()));
    session.send(out::modtool_room_data(&view, owner_in_room, event_ref));
    Ok(())
}

/// Java: `FLASH_MODTOOL_ROOM_CHATLOG`: the room's last 150 lines.
pub fn modtool_room_chatlog(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    use chrono::Timelike;
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let rank = session.player.lock().details.as_ref().map_or(0, |d| d.rank);
    if rank < crate::game::commands::RANK_MODERATOR {
        return Ok(());
    }
    let (id, public, name) = {
        let r = room.lock();
        (r.id(), r.is_public(), r.data.name.clone())
    };
    let mut logs: Vec<rusbbo_storage::ChatLog> = world
        .store
        .all_chat_logs()
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.room_id == id)
        .collect();
    logs.sort_by_key(|l| Reverse(l.timestamp));
    let mut lines = Vec::new();
    for l in logs {
        if lines.len() >= 150 {
            break;
        }
        // INNER JOIN users.
        let Some(user) = world.store.user(l.user_id).ok().flatten() else {
            continue;
        };
        let time = chrono::TimeZone::timestamp_opt(&chrono::Local, l.timestamp, 0).single();
        lines.push(out::ModChatLine {
            hour: time.map_or(0, |t| t.hour() as i32),
            minute: time.map_or(0, |t| t.minute() as i32),
            user_id: l.user_id,
            username: user.username,
            message: crate::web::text::filter_input(&l.message, true),
        });
    }
    session.send(out::modtool_chatlog(public, id as i32, &name, &lines));
    Ok(())
}
