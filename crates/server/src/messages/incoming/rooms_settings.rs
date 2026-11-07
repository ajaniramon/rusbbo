//! Java: `messages/incoming/rooms/settings` (the room kiosk, the room settings dialogs and
//! deleting a room).
//!
//! Like Java's `RoomManager.getRoomById`, these work on the loaded room when there is one and on
//! a copy read from storage otherwise ([`World::room_by_id`]); changes are written back with
//! [`save`] (Java: `RoomDao.save`).

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use tracing::error;

use super::latin1;
use crate::game::now;
use crate::game::room::{RoomRef, filter_input, run_deferred};
use crate::messages::Malformed;
use crate::messages::outgoing::alerts;
use crate::messages::outgoing::rooms_settings::{self as out, FlatInfo};
use crate::session::Session;
use crate::world::World;

/// Model letters anyone may pick in the room kiosk; the others need
/// `fuse_use_special_room_layouts`.
const KIOSK_MODELS: [&str; 12] = ["a", "b", "c", "d", "e", "f", "i", "j", "k", "l", "m", "n"];

/// Java: `RoomDao.save`: the columns the settings dialogs change.
pub fn save(world: &World, data: &havana_storage::Room) {
    if let Err(e) = world.store.update_room(data.id, |row| {
        row.category = data.category;
        row.name = data.name.clone();
        row.description = data.description.clone();
        row.showname = data.showname;
        row.superusers = data.superusers;
        row.accesstype = data.accesstype;
        row.password = data.password.clone();
        row.visitors_max = data.visitors_max;
    }) {
        error!(room = data.id, "saving room: {e}");
    }
}

/// Java's door mode names (`RoomData.getAccessType`) to their ids; anything else is open.
fn access_type_id(name: &str) -> i32 {
    match name {
        "closed" => 1,
        "password" => 2,
        _ => 0,
    }
}

/// The logged-in user id of the session.
fn user_id(session: &Session) -> Option<u32> {
    session.player.lock().user_id()
}

/// The room a client-sent id names, loaded or read from storage.
pub fn room_by_client_id(world: &World, id: i32) -> Option<RoomRef> {
    u32::try_from(id).ok().and_then(|id| world.room_by_id(id))
}

/// Java: `CREATEFLAT`: the room kiosk. Answers `GOTO_FLAT` and lets the creator in.
pub fn create_flat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    // TODO(wordfilter): WordfilterManager.filterSentence.
    let name = latin1(&filter_input(reader.string().ok_or(Malformed)?));
    let model = latin1(reader.string().ok_or(Malformed)?);
    let access = if reader.remaining().is_empty() {
        String::new()
    } else {
        latin1(reader.string().ok_or(Malformed)?)
    };
    let show_name = reader.remaining().is_empty() || reader.bool().ok_or(Malformed)?;
    let (owner_id, special_layouts) = {
        let p = session.player.lock();
        let Some(id) = p.user_id() else {
            return Ok(());
        };
        (id, p.has_fuse("fuse_use_special_room_layouts"))
    };

    if name.replace(' ', "").is_empty() {
        session.send(alerts::alert(&world.text("roomatic_givename")));
        return Ok(());
    }
    let Some(model_type) = model.strip_prefix("model_") else {
        return Ok(());
    };
    if !world.room_models.contains_key(&model) {
        return Ok(());
    }
    if !KIOSK_MODELS.contains(&model_type) && !special_layouts {
        return Ok(());
    }

    // NavigatorDao.createRoom: the INSERT sets these columns, the rest are the table defaults.
    let t = now();
    let room = havana_storage::Room {
        owner_id,
        category: 2,
        name: name.clone(),
        description: String::new(),
        model,
        ccts: Some(String::new()),
        landscape: "0".into(),
        showname: show_name,
        accesstype: access_type_id(&access),
        password: Some(String::new()),
        visitors_max: 25,
        icon_data: "0|0|".into(),
        created_at: t,
        updated_at: t,
        ..Default::default()
    };
    let room_id = match world.store.create_room(&room) {
        Ok(id) => id,
        Err(e) => {
            error!(owner = owner_id, "creating room: {e}");
            return Ok(());
        }
    };
    session.player.lock().authenticate_id = Some(room_id);
    session.send(out::goto_flat(room_id, &name));
    Ok(())
}

/// Java: `SETFLATINFO`: description, password, "everyone has rights" and the visitor limit.
pub fn set_flat_info(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(room) = room_by_client_id(world, room_id) else {
        return Ok(());
    };
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    if !room.lock().is_owner(user_id) {
        return Ok(());
    }
    // TODO(wordfilter): WordfilterManager.filterSentence on the description.
    let description = latin1(&filter_input(reader.string().ok_or(Malformed)?));
    let password = latin1(&filter_input(reader.string().ok_or(Malformed)?));
    let all_super_users = reader.int().ok_or(Malformed)?;
    let max_visitors = if reader.remaining().is_empty() {
        None
    } else {
        let max = reader.int().ok_or(Malformed)?;
        Some(if (10..=50).contains(&max) { max } else { 25 })
    };
    let data = {
        let mut r = room.lock();
        r.data.description = description;
        r.data.password = Some(password);
        r.data.superusers = all_super_users == 1;
        if let Some(max) = max_visitors {
            r.data.visitors_max = max;
        }
        r.data.clone()
    };
    save(world, &data);
    Ok(())
}

/// Java: `SETFLATCAT`: the room's navigator category.
pub fn set_flat_cat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let mut category_id = reader.int().ok_or(Malformed)?;
    let Some(category) = u32::try_from(category_id)
        .ok()
        .and_then(|id| world.room_categories.get(&id))
    else {
        return Ok(());
    };
    let (user_id, rank) = {
        let p = session.player.lock();
        let Some(id) = p.user_id() else {
            return Ok(());
        };
        (id, p.details.as_ref().map_or(0, |d| d.rank))
    };
    if category.minrole_setflatcat > i32::from(rank) {
        return Ok(());
    }
    if category.is_node || category.public_spaces {
        category_id = 2;
    }
    let Some(room) = room_by_client_id(world, room_id) else {
        return Ok(());
    };
    let data = {
        let mut r = room.lock();
        if !r.is_owner(user_id) {
            return Ok(());
        }
        r.data.category = category_id;
        r.data.clone()
    };
    save(world, &data);
    Ok(())
}

/// Java: `GETFLATCAT`.
pub fn get_flat_cat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(room) = room_by_client_id(world, room_id) else {
        return Ok(());
    };
    let msg = {
        let r = room.lock();
        out::flat_cat(r.id(), r.data.category)
    };
    session.send(msg);
    Ok(())
}

/// Java: `GETFLATINFO`: the navigator's room info and the settings dialog.
pub fn get_flat_info(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(room) = room_by_client_id(world, room_id) else {
        return Ok(());
    };
    let (user_id, sees_all_owners, override_lock) = {
        let p = session.player.lock();
        let Some(id) = p.user_id() else {
            return Ok(());
        };
        (
            id,
            p.has_fuse("fuse_see_all_roomowners"),
            p.authenticate_id == Some(room.id),
        )
    };
    let data = room.lock().data.clone();
    let model_name = world
        .room_models
        .get(&data.model)
        .map(|m| m.name.clone())
        .unwrap_or_default();
    let Some(category) = u32::try_from(data.category)
        .ok()
        .and_then(|id| world.room_categories.get(&id))
    else {
        // Java throws a NullPointerException (`getCategory().hasAllowTrading()`).
        return Ok(());
    };
    let owner_name = if data.owner_id == user_id || data.showname || sees_all_owners {
        // Java's RoomDao reads the owner's name with the room (LEFT JOIN users).
        world
            .store
            .user(data.owner_id)
            .ok()
            .flatten()
            .map(|u| u.username)
            .unwrap_or_default()
    } else {
        "-".to_string()
    };
    session.send(out::flat_info(&FlatInfo {
        super_users: data.superusers,
        access_type: if override_lock { 0 } else { data.accesstype },
        room_id: data.id,
        owner_name: &owner_name,
        model_name: &model_name,
        name: &data.name,
        description: &data.description,
        show_owner_name: data.showname,
        allow_trading: category.allow_trading,
        visitors_now: data.visitors_now,
        visitors_max: data.visitors_max,
    }));
    Ok(())
}

/// Java: `UPDATEFLAT`: name, door mode and whether the owner's name shows.
pub fn update_flat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(room) = room_by_client_id(world, room_id) else {
        return Ok(());
    };
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    if !room.lock().is_owner(user_id) {
        return Ok(());
    }
    // TODO(wordfilter): WordfilterManager.filterSentence.
    let name = latin1(&filter_input(reader.string().ok_or(Malformed)?));
    let access = latin1(reader.string().ok_or(Malformed)?);
    let show_owner = reader.bool().ok_or(Malformed)?;
    let data = {
        let mut r = room.lock();
        r.data.name = name;
        r.data.accesstype = access_type_id(&access);
        r.data.showname = show_owner;
        r.data.clone()
    };
    save(world, &data);
    Ok(())
}

/// Java: `DELETEFLAT`.
pub fn delete_flat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Some(user_id) = user_id(session) else {
        return Ok(());
    };
    delete(world, room_id, user_id);
    // TODO(flash): Flash clients get their room list again (FLASH_USERFLATS).
    Ok(())
}

/// Java: `DELETEFLAT.delete`: everybody inside goes to the hotel view, the room is unloaded
/// after the dispose delay and its row is deleted. Until the room is unloaded it still answers
/// `GETFLATINFO` and `TRYFLAT`, as in Java.
pub fn delete(world: &World, room_id: i32, user_id: u32) {
    let Some(room) = room_by_client_id(world, room_id) else {
        return;
    };
    let deferred = {
        let mut r = room.lock();
        if !r.is_owner(user_id) {
            return;
        }
        // Java: `item.delete()` for each room item; they are not returned to the hand.
        r.delete_flat_items(world);
        let sessions: Vec<Arc<Session>> = r
            .entities
            .iter()
            .filter(|e| e.is_player())
            .map(|e| e.session.clone())
            .collect();
        for s in &sessions {
            r.remove_user(world, s, true);
        }
        r.dispose_when_empty(world);
        r.take_deferred()
    };
    run_deferred(world, deferred);
    // TODO(tags): TagDao.removeTags. TODO(groups): GroupDao.deleteHomeRoom.
    if let Err(e) = world.store.delete_room(room.id) {
        error!(room = room.id, "deleting room: {e}");
    }
}
