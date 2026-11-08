//! Java: `messages/incoming/jukebox`: the song discs loaded into the room's jukebox (10 slots)
//! and its playlist. A loaded disc stays the user's item, hidden from the hand.

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use havana_storage::SongDisk;
use tracing::error;

use super::songs::{machine, playlist_message, send_hand};
use crate::game::item::base::ItemBehaviour;
use crate::game::player::Player;
use crate::game::room::{Room, current_room};
use crate::game::song;
use crate::messages::Malformed;
use crate::messages::outgoing::jukebox as out;
use crate::messages::outgoing::songs as out_songs;
use crate::session::Session;
use crate::world::World;

/// Java: `room.hasRights(id) || player.hasFuse(MOD)`, which every jukebox handler but
/// `JUKEBOX_PLAYLIST_ADD` asks.
fn has_rights(p: &Player, r: &Room) -> bool {
    p.user_id().is_some_and(|id| r.has_rights(id, true)) || p.has_fuse("fuse_mod")
}

/// Java: `JUKEBOX_DISCS(JukeboxManager.getDisks(machine))`, `None` where it fails to compose.
fn discs_message(world: &World, machine_id: i64) -> Option<havana_protocol::Message> {
    let discs: Vec<_> = song::jukebox_disks(&world.store, machine_id)
        .into_iter()
        .map(|(disk, song)| {
            let author = song::author_name(&world.store, song.user_id);
            (disk, song, author)
        })
        .collect();
    out::jukebox_discs(&discs)
}

/// Java: `USER_SONG_DISKS` of the visible song discs in the hand, the body of
/// `GET_USER_SONG_DISCS`.
fn send_hand_discs(session: &Session, p: &Player) {
    let discs: Vec<(i32, &str)> = p
        .inventory
        .items()
        .iter()
        .filter(|i| i.is_visible() && i.has_behaviour(ItemBehaviour::SongDisk))
        .map(|i| (i.virtual_id, i.record.custom_data.as_str()))
        .collect();
    session.send(out::user_song_disks(&discs));
}

/// Runs `f` with the player and the room when the room has a sound machine or jukebox and the
/// player has rights there. Locks the player, then the room.
fn with_jukebox(
    world: &World,
    session: &Arc<Session>,
    f: impl FnOnce(&mut Player, &mut Room, i64),
) {
    let Some(room) = current_room(world, session) else {
        return;
    };
    let mut p = session.player.lock();
    let mut r = room.lock();
    let Some(machine) = machine(&r) else {
        return;
    };
    if !has_rights(&p, &r) {
        return;
    }
    f(&mut p, &mut r, machine.id);
}

/// Java: `GET_JUKEBOX_DISCS`: the discs loaded into the jukebox. The client asks on entering a
/// room with one.
pub fn get_jukebox_discs(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    with_jukebox(world, session, |_, _, machine_id| {
        if let Some(msg) = discs_message(world, machine_id) {
            session.send(msg);
        }
    });
    Ok(())
}

/// Java: `GET_USER_SONG_DISCS`: the song discs in the hand.
pub fn get_user_song_discs(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    with_jukebox(world, session, |p, _, _| send_hand_discs(session, p));
    Ok(())
}

/// Java: `ADD_JUKEBOX_DISC`: a disc from the hand (virtual id) into a free slot (1-10).
pub fn add_jukebox_disc(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut result = Ok(());
    with_jukebox(world, session, |p, r, machine_id| {
        let (Some(item_id), Some(slot_id)) = (reader.int(), reader.int()) else {
            result = Err(Malformed);
            return;
        };
        let Some(index) = p
            .inventory
            .items()
            .iter()
            .position(|i| i.virtual_id == item_id && i.is_visible())
        else {
            return;
        };
        let disk_id = p.inventory.items()[index].record.id as i64;
        // Checked before the disc leaves the hand (Java: "otherwise a bad request makes it
        // vanish").
        let song_id = match world.store.disk_by_item(disk_id) {
            Ok(disk) => disk.map_or(-1, |d| d.song_id),
            Err(e) => {
                error!(item = disk_id, "loading a song disc: {e}");
                -1
            }
        };
        if song::song(&world.store, song_id).is_none() {
            return;
        }
        if !(1..=10).contains(&slot_id) {
            return;
        }
        if world
            .store
            .disk_in_slot(machine_id, slot_id)
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }
        let item = &mut p.inventory.items_mut()[index];
        item.record.is_hidden = true;
        if let Err(e) = world.store.update_item(&item.record) {
            error!(item = item.record.id, "saving a song disc: {e}");
        }
        send_hand(world, session, p);
        if let Err(e) = world.store.move_disk(disk_id, machine_id, slot_id) {
            error!(item = disk_id, "loading a song disc: {e}");
        }
        if let Some(msg) = discs_message(world, machine_id) {
            r.send(msg);
        }
        send_hand_discs(session, p);
    });
    result
}

/// Java: `REMOVE_JUKEBOX_DISC`: the disc in a slot back to the hand of whoever loaded it (only
/// they can take it out), and its song off the playlist.
pub fn remove_jukebox_disc(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut result = Ok(());
    with_jukebox(world, session, |p, r, machine_id| {
        let Some(slot_id) = reader.int() else {
            result = Err(Malformed);
            return;
        };
        let disk: Option<SongDisk> = world
            .store
            .disk_in_slot(machine_id, slot_id)
            .unwrap_or_else(|e| {
                error!(machine = machine_id, "loading a song disc: {e}");
                None
            });
        let Some(disk) = disk else {
            return;
        };
        let Some(mut item) = p
            .inventory
            .items()
            .iter()
            .find(|i| i.record.id as i64 == disk.item_id && !i.is_visible())
            .cloned()
        else {
            return;
        };
        item.record.is_hidden = false;
        if let Err(e) = world.store.update_item(&item.record) {
            error!(item = item.record.id, "saving a song disc: {e}");
        }
        p.inventory.add_item(item); // Back at the start.
        if let Err(e) = world.store.remove_playlist_song(disk.song_id, machine_id) {
            error!(
                machine = machine_id,
                "removing a song from the playlist: {e}"
            );
        }
        if let Err(e) = world.store.move_disk(disk.item_id, 0, 0) {
            error!(item = disk.item_id, "unloading a song disc: {e}");
        }
        send_hand(world, session, p);
        send_hand_discs(session, p);
        if let Some(msg) = playlist_message(world, machine_id) {
            r.send(msg);
        }
        if let Some(msg) = discs_message(world, machine_id) {
            r.send(msg);
        }
    });
    result
}

/// Java: `JUKEBOX_PLAYLIST_ADD`: a song of a loaded disc queued at the end of the playlist (or
/// moved there). Java's rights check is commented out: anyone in the room may queue.
pub fn jukebox_playlist_add(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let r = room.lock();
    let Some(machine) = machine(&r) else {
        return Ok(());
    };
    let song_id = reader.int().ok_or(Malformed)?;
    if let Err(e) = world.store.remove_playlist_song(song_id, machine.id) {
        error!(
            machine = machine.id,
            "removing a song from the playlist: {e}"
        );
    }
    let playlist = song::playlist(&world.store, machine.id);
    let loaded = song::jukebox_disks(&world.store, machine.id);
    if !loaded.iter().any(|(disk, _)| disk.song_id == song_id) {
        return Ok(());
    }
    let slot_id = playlist.iter().map(|(e, _)| e.slot_id).max().unwrap_or(0) + 1;
    if let Err(e) = world.store.add_playlist_entry(machine.id, song_id, slot_id) {
        error!(machine = machine.id, "adding to the playlist: {e}");
    }
    // Java sends the list it read with the new song appended.
    let songs: Vec<_> = playlist
        .into_iter()
        .map(|(_, song)| song)
        .chain(std::iter::once(song::song(&world.store, song_id)))
        .map(|song| {
            let author = song
                .as_ref()
                .and_then(|s| song::author_name(&world.store, s.user_id));
            (song, author)
        })
        .collect();
    if let Some(msg) = out_songs::song_playlist(&songs) {
        r.send(msg);
    }
    Ok(())
}

/// Java: `RESET_JUKEBOX`: the playlist is emptied.
pub fn reset_jukebox(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    with_jukebox(world, session, |_, r, machine_id| {
        if let Err(e) = world.store.clear_playlist(machine_id) {
            error!(machine = machine_id, "clearing the playlist: {e}");
        }
        if let Some(msg) = out_songs::song_playlist(&[]) {
            r.send(msg);
        }
    });
    Ok(())
}
