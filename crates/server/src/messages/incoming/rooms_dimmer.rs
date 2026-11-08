//! Java: `messages/incoming/rooms/dimmer`: the room dimmer ("moodlight") and its three presets
//! (`items_moodlight_presets`), whose current one is the dimmer's custom data
//! (`<1 off|2 on>,<preset>,<background state>,<#colour>,<strength>`).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;
use tracing::error;

use super::latin1;
use crate::game::room::{Room, current_room};
use crate::messages::Malformed;
use crate::messages::outgoing::rooms_dimmer as out;
use crate::messages::outgoing::rooms_user::{ChatType, chat_message};
use crate::session::Session;
use crate::world::World;

/// The colours of the client's palette; anything else is a scripted dimmer, which Java refuses
/// (`roomdimmer.scripting.allowed` is commented out).
const PRESET_COLOURS: [&str; 7] = [
    "#74F5F5", "#0053F7", "#E759DE", "#EA4532", "#F2F851", "#82F349", "#000000",
];

/// The sender's user id and `fuse_any_room_controller`.
fn sender(session: &Session) -> Option<(u32, bool)> {
    let p = session.player.lock();
    Some((p.user_id()?, p.has_fuse("fuse_any_room_controller")))
}

/// The dimmer's row id (Java: `getDatabaseId`).
fn dimmer_id(r: &Room, idx: usize) -> u64 {
    r.mapping.items[idx].database_id as u64
}

/// Java: `if (!MoodlightDao.containsPreset(id)) MoodlightDao.createPresets(id)`.
fn ensure_presets(world: &World, item_id: u64) {
    let exists = world
        .store
        .moodlight_presets(item_id)
        .ok()
        .flatten()
        .is_some();
    if !exists {
        if let Err(e) = world.store.create_moodlight_presets(item_id) {
            error!(item = item_id, "creating moodlight presets: {e}");
        }
    }
}

fn presets(world: &World, item_id: u64) -> Option<rusbbo_storage::MoodlightPresets> {
    world.store.moodlight_presets(item_id).unwrap_or_else(|e| {
        error!(item = item_id, "loading moodlight presets: {e}");
        None
    })
}

/// Java: cancelling the `RainbowTask` because the owner chose their own setting, with a
/// whisper to them.
fn stop_rainbow(r: &mut Room, session: &Session) {
    if !r.cancel_rainbow() {
        return;
    }
    if let Some(idx) = r.entity_index(session.id) {
        session.send(chat_message(
            ChatType::Whisper,
            r.entities[idx].instance_id,
            b"Rainbow room dimmer cycle has stopped",
            0,
        ));
    }
}

/// Java: `MSG_ROOMDIMMER_GET_PRESETS`: the presets of the room's dimmer, created on the owner's
/// first look. Anyone in the room gets them once they exist (the client asks on entering).
pub fn get_presets(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let Some((user_id, any_controller)) = sender(session) else {
        return Ok(());
    };
    let r = room.lock();
    let Some(idx) = r.moodlight() else {
        return Ok(());
    };
    let item_id = dimmer_id(&r, idx);
    if r.is_owner(user_id) || any_controller {
        ensure_presets(world, item_id);
    }
    let Some(row) = presets(world, item_id) else {
        return Ok(());
    };
    if let Some(msg) = out::moodlight_presets(row.current_preset, &row.presets) {
        session.send(msg);
    }
    Ok(())
}

/// Java: `MSG_ROOMDIMMER_SET_PRESET`: preset id, background state, colour and strength. The
/// preset is saved, made current and the dimmer switched on with it.
pub fn set_preset(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let Some((user_id, any_controller)) = sender(session) else {
        return Ok(());
    };
    let mut r = room.lock();
    if !r.is_owner(user_id) && !any_controller {
        return Ok(());
    }
    let Some(idx) = r.moodlight() else {
        return Ok(());
    };
    let item_id = dimmer_id(&r, idx);
    ensure_presets(world, item_id);
    let preset_id = reader.int().ok_or(Malformed)?;
    let background_state = reader.int().ok_or(Malformed)?;
    let colour = latin1(reader.string().ok_or(Malformed)?);
    let strength = reader.int().ok_or(Malformed)?;
    if !PRESET_COLOURS.contains(&colour.as_str()) {
        return Ok(());
    }
    stop_rainbow(&mut r, session);
    let Some(mut row) = presets(world, item_id) else {
        return Ok(());
    };
    // Java's presets.set(presetId - 1, ...) throws for any other preset.
    let Some(slot) = usize::try_from(preset_id - 1)
        .ok()
        .and_then(|i| row.presets.get_mut(i))
    else {
        return Ok(());
    };
    *slot = format!("{background_state},{colour},{strength}");
    r.set_item_status(
        idx,
        &format!("2,{preset_id},{background_state},{colour},{strength}"),
    );
    r.save_item(world, idx);
    if let Err(e) = world
        .store
        .update_moodlight_presets(item_id, preset_id, &row.presets)
    {
        error!(item = item_id, "saving moodlight presets: {e}");
    }
    Ok(())
}

/// Java: `MSG_ROOMDIMMER_CHANGE_STATE`: switches the dimmer on (with the current preset) or
/// off.
pub fn change_state(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let Some((user_id, any_controller)) = sender(session) else {
        return Ok(());
    };
    let mut r = room.lock();
    if !r.is_owner(user_id) && !any_controller {
        return Ok(());
    }
    let Some(idx) = r.moodlight() else {
        return Ok(());
    };
    let item_id = dimmer_id(&r, idx);
    ensure_presets(world, item_id);
    stop_rainbow(&mut r, session);
    let Some(row) = presets(world, item_id) else {
        return Ok(());
    };
    // Java: charAt(0) and presets.get(currentPreset - 1), either of which may throw.
    let Some(first) = r.mapping.items[idx].custom_data.chars().next() else {
        return Ok(());
    };
    let Some(preset) = usize::try_from(row.current_preset - 1)
        .ok()
        .and_then(|i| row.presets.get(i))
    else {
        return Ok(());
    };
    let enabled = first != '2';
    let data = format!(
        "{},{},{preset}",
        if enabled { "2" } else { "1" },
        row.current_preset
    );
    r.set_item_status(idx, &data);
    r.save_item(world, idx);
    Ok(())
}
