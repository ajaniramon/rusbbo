//! Java: `messages/incoming/songs`: the sound machine's song editor (sound sets in the machine's
//! four slots, songs saved, edited and deleted), the playlist of a sound machine or jukebox, and
//! burning a song to a disc.
//!
//! The room's sound machine is [`Room::sound_machine`]: a sound machine or a jukebox, whichever
//! is placed. Handlers that touch the hand lock the player first and the room second.

use std::sync::Arc;

use chrono::Datelike;
use havana_protocol::body::ClientReader;
use havana_storage::Song;
use tracing::error;

use super::latin1;
use crate::game::currency;
use crate::game::item::base::ItemBehaviour;
use crate::game::now;
use crate::game::player::Player;
use crate::game::room::items::Behaviour;
use crate::game::room::{Room, RoomRef, current_room, filter_input};
use crate::game::song::{self, calculate_song_length, hand_sound_sets, parse_int};
use crate::messages::Malformed;
use crate::messages::outgoing::{inventory as out_inventory, songs as out, user as out_user};
use crate::session::Session;
use crate::world::World;

/// The room's sound machine or jukebox (Java: `getSoundMachine()`).
pub(super) struct Machine {
    pub idx: usize,
    /// Java: `getDatabaseId()`.
    pub id: i64,
}

pub(super) fn machine(r: &Room) -> Option<Machine> {
    let idx = r.sound_machine()?;
    Some(Machine {
        idx,
        id: r.mapping.items[idx].database_id,
    })
}

/// Java: `!room.isOwner(id) && !player.hasFuse(ANY_ROOM_CONTROLLER)`.
fn may_edit(p: &Player, r: &Room) -> bool {
    p.user_id().is_some_and(|id| r.is_owner(id)) || p.has_fuse("fuse_any_room_controller")
}

/// Java: `getTimerManager().resetRoomTimer()`: "we don't want a user to get kicked when making
/// cool beats".
fn reset_timer(world: &World, r: &mut Room, session: &Session) {
    if let Some(idx) = r.entity_index(session.id) {
        r.reset_room_timer(world, idx);
    }
}

/// Java: `StringUtil.filterInput(reader.readString(), true)`.
fn read_text(reader: &mut ClientReader) -> Result<String, Malformed> {
    Ok(latin1(&filter_input(reader.string().ok_or(Malformed)?)))
}

/// Java: `SOUND_PACKAGES(SongMachineDao.getTracks(machine))`.
fn send_sound_packages(world: &World, session: &Session, machine: &Machine) {
    session.send(out::sound_packages(&song::tracks(&world.store, machine.id)));
}

/// Java: `USER_SOUND_PACKAGES(inventory.getSoundsets())`. `false` where Java throws.
fn send_hand_sound_sets(session: &Session, p: &Player) -> bool {
    let Some(sets) = hand_sound_sets(p.inventory.items()) else {
        return false;
    };
    session.send(out::user_sound_packages(&sets));
    true
}

/// Java: `getView("new")`: the hand, opened on its first page.
pub(super) fn send_hand(world: &World, session: &Session, p: &mut Player) {
    let hand = p.inventory.get_view(&world.store, "new");
    session.send(out_inventory::inventory(&hand));
}

/// Java: `SONG_PLAYLIST(SongMachineDao.getSongPlaylist(machine))`, `None` where it fails to
/// compose.
pub(super) fn playlist_message(world: &World, machine_id: i64) -> Option<havana_protocol::Message> {
    let songs: Vec<(Option<Song>, Option<String>)> = song::playlist(&world.store, machine_id)
        .into_iter()
        .map(|(_, song)| {
            let author = song
                .as_ref()
                .and_then(|s| song::author_name(&world.store, s.user_id));
            (song, author)
        })
        .collect();
    out::song_playlist(&songs)
}

/// The room's sound machine for the song editor: the player's room, the owner (or a room
/// controller), a sound machine placed. Locks the player, then the room.
fn with_editor(
    world: &World,
    session: &Arc<Session>,
    f: impl FnOnce(&mut Player, &mut Room, Machine),
) {
    let Some(room) = current_room(world, session) else {
        return;
    };
    let mut p = session.player.lock();
    let mut r = room.lock();
    if !may_edit(&p, &r) {
        return;
    }
    let Some(machine) = machine(&r) else {
        return;
    };
    reset_timer(world, &mut r, session);
    f(&mut p, &mut r, machine);
}

fn room_and_machine(world: &World, session: &Session) -> Option<(RoomRef, Machine)> {
    let room = current_room(world, session)?;
    let machine = machine(&room.lock())?;
    Some((room, machine))
}

/// Java: `NEW_SONG`: the machine's sound sets and the hand's, to start a song.
pub fn new_song(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    with_editor(world, session, |p, _, machine| {
        send_sound_packages(world, session, &machine);
        send_hand_sound_sets(session, p);
    });
    Ok(())
}

/// Java: `INSERT_SOUND_PACKAGE`: a sound set from the hand into the machine's first free slot
/// (1-4); the set leaves the hand (hidden) while it is in the machine.
pub fn insert_sound_package(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut result = Ok(());
    with_editor(world, session, |p, _, machine| {
        let tracks = song::tracks(&world.store, machine.id);
        let Some(sound_set) = reader.int() else {
            result = Err(Malformed);
            return;
        };
        let mut slot = 1;
        while tracks.contains_key(&slot) {
            slot += 1;
        }
        if slot >= 5 {
            return;
        }
        let mut found = None;
        for (i, item) in p.inventory.items().iter().enumerate() {
            if item.has_behaviour(ItemBehaviour::SoundMachineSampleSet) && item.is_visible() {
                // Java's parseInt throws here for a sprite without a number.
                let Some(number) = song::sound_set_number(item) else {
                    return;
                };
                if number == sound_set {
                    found = Some(i);
                    break;
                }
            }
        }
        let Some(i) = found else {
            return;
        };
        let item = &mut p.inventory.items_mut()[i];
        item.record.is_hidden = true;
        if let Err(e) = world.store.update_item(&item.record) {
            error!(item = item.record.id, "saving a sound set: {e}");
        }
        send_hand(world, session, p);
        if let Err(e) = world.store.add_track(machine.id, sound_set, slot) {
            error!(machine = machine.id, "adding a sound set: {e}");
        }
        send_sound_packages(world, session, &machine);
        send_hand_sound_sets(session, p);
    });
    result
}

/// Java: `EJECT_SOUND_PACKAGE`: the sound set in a slot goes back to the hand (the first hidden
/// one of that number).
pub fn eject_sound_package(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut result = Ok(());
    with_editor(world, session, |p, _, machine| {
        let Some(slot) = reader.int() else {
            result = Err(Malformed);
            return;
        };
        let tracks = song::tracks(&world.store, machine.id);
        let Some(&sound_set) = tracks.get(&slot) else {
            return;
        };
        if let Err(e) = world.store.remove_track(machine.id, slot) {
            error!(machine = machine.id, "removing a sound set: {e}");
        }
        let mut found = None;
        for item in p.inventory.items() {
            if item.is_visible() || !item.has_behaviour(ItemBehaviour::SoundMachineSampleSet) {
                continue;
            }
            // Java: parseInt(sprite.replace("sound_set_", "")), which may throw.
            let Some(number) = parse_int(&item.definition.sprite().replace("sound_set_", ""))
            else {
                return;
            };
            if number == sound_set {
                found = Some(item.clone());
                break;
            }
        }
        let Some(mut item) = found else {
            return;
        };
        item.record.is_hidden = false;
        if let Err(e) = world.store.update_item(&item.record) {
            error!(item = item.record.id, "saving a sound set: {e}");
        }
        p.inventory.add_item(item); // Back at the start.
        send_hand(world, session, p);
        send_sound_packages(world, session, &machine);
        send_hand_sound_sets(session, p);
    });
    result
}

/// Java: `SAVE_SONG_NEW`: title and trax data; a new song of this machine.
pub fn save_song_new(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut result = Ok(());
    with_editor(world, session, |p, r, machine| {
        let (title, data) = match (read_text(reader), read_text(reader)) {
            (Ok(title), Ok(data)) => (title, data),
            _ => {
                result = Err(Malformed);
                return;
            }
        };
        let Some(user_id) = p.user_id() else {
            return;
        };
        let new = Song {
            user_id: user_id as i32,
            item_id: machine.id,
            length: calculate_song_length(&data),
            title: title.clone(),
            data,
            ..Default::default()
        };
        if let Err(e) = world.store.insert_song(new) {
            error!(machine = machine.id, "saving a new song: {e}");
            return;
        }
        send_sound_packages(world, session, &machine);
        let virtual_id = r.mapping.items[machine.idx].virtual_id;
        session.send(out::song_new(virtual_id, &title));
    });
    result
}

/// Java: `UPDATE_PLAY_LIST`: the playlist, replaced by up to 5 song ids, for the room.
pub fn update_play_list(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let p = session.player.lock();
    let mut r = room.lock();
    if !may_edit(&p, &r) {
        return Ok(());
    }
    let Some(machine) = machine(&r) else {
        return Ok(());
    };
    let amount = reader.int().ok_or(Malformed)?;
    if amount >= 6 {
        return Ok(());
    }
    reset_timer(world, &mut r, session);
    if let Err(e) = world.store.clear_playlist(machine.id) {
        error!(machine = machine.id, "clearing the playlist: {e}");
    }
    for i in 0..amount {
        let song_id = reader.int().ok_or(Malformed)?;
        if let Err(e) = world.store.add_playlist_entry(machine.id, song_id, i) {
            error!(machine = machine.id, "adding to the playlist: {e}");
        }
    }
    if let Some(msg) = playlist_message(world, machine.id) {
        r.send(msg);
    }
    Ok(())
}

/// Java: `GET_SONG_INFO`: a song's title and data (nothing for an unknown song: Java fails to
/// compose it).
pub fn get_song_info(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if room_and_machine(world, session).is_none() {
        return Ok(());
    }
    let song_id = reader.int().ok_or(Malformed)?;
    if let Some(song) = song::song(&world.store, song_id) {
        session.send(out::song_info(&song));
    }
    Ok(())
}

/// Java: `GET_PLAY_LIST`: the playlist of the room's sound machine or jukebox. The client asks
/// on entering a room that has one.
pub fn get_play_list(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((_, machine)) = room_and_machine(world, session) else {
        return Ok(());
    };
    if let Some(msg) = playlist_message(world, machine.id) {
        session.send(msg);
    }
    Ok(())
}

/// Java: `GET_SONG_LIST`: the songs made in the room's machine.
pub fn get_song_list(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((_, machine)) = room_and_machine(world, session) else {
        return Ok(());
    };
    session.send(out::song_list(&song::machine_songs(
        &world.store,
        machine.id,
    )));
    Ok(())
}

/// The song `song_id` if it was made by the player (Java: `song.getUserId() != id` returns).
fn own_song(world: &World, p: &Player, song_id: i32) -> Option<Song> {
    let song = song::song(&world.store, song_id)?;
    (p.user_id().map(|id| id as i32) == Some(song.user_id)).then_some(song)
}

/// Java: `DELETE_SONG`: the song leaves the machine (its row stays for the discs burnt from
/// it) and the playlist.
pub fn delete_song(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut result = Ok(());
    with_editor(world, session, |p, r, machine| {
        let Some(song_id) = reader.int() else {
            result = Err(Malformed);
            return;
        };
        let Some(song) = own_song(world, p, song_id) else {
            return;
        };
        if let Err(e) = world.store.update_song(song.id, |s| s.item_id = -1) {
            error!(song = song.id, "deleting a song: {e}");
        }
        if let Err(e) = world.store.remove_playlist_song(song_id, machine.id) {
            error!(song = song.id, "removing a song from the playlist: {e}");
        }
        if r.mapping.items[machine.idx].has(Behaviour::SoundMachine) {
            if let Some(msg) = playlist_message(world, machine.id) {
                session.send(msg);
            }
        }
    });
    result
}

/// Java: `EDIT_SONG`: a song to edit (`SONG_LOCKED` first when it was burnt) with the machine's
/// and the hand's sound sets.
pub fn edit_song(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut result = Ok(());
    with_editor(world, session, |p, _, machine| {
        let Some(song_id) = reader.int() else {
            result = Err(Malformed);
            return;
        };
        let Some(song) = own_song(world, p, song_id) else {
            return;
        };
        if song.burnt {
            session.send(out::song_locked());
        }
        session.send(out::song_info(&song));
        send_sound_packages(world, session, &machine);
        send_hand_sound_sets(session, p);
    });
    result
}

/// Java: `SAVE_SONG_EDIT`: song id, title and data. A burnt song is not overwritten: under the
/// same title the client is told it is locked, under another one it is saved as a new song.
pub fn save_song_edit(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut result = Ok(());
    with_editor(world, session, |p, r, machine| {
        let read = (|| -> Result<_, Malformed> {
            let song_id = reader.int().ok_or(Malformed)?;
            Ok((song_id, read_text(reader)?, read_text(reader)?))
        })();
        let Ok((song_id, title, data)) = read else {
            result = Err(Malformed);
            return;
        };
        let Some(song) = own_song(world, p, song_id) else {
            return;
        };
        let mut songs = song::machine_songs(&world.store, machine.id);
        let length = calculate_song_length(&data);
        if song.burnt {
            if songs
                .iter()
                .any(|s| s.id as i32 == song_id && s.title == title)
            {
                session.send(out::song_locked());
                return;
            }
            let copy = Song {
                user_id: song.user_id,
                item_id: machine.id,
                title,
                length,
                data,
                ..Default::default()
            };
            if let Err(e) = world.store.insert_song(copy) {
                error!(machine = machine.id, "saving a song copy: {e}");
            }
            songs = song::machine_songs(&world.store, machine.id);
        } else if let Err(e) = world.store.update_song(song.id, |s| {
            s.title = title;
            s.length = length;
            s.data = data;
        }) {
            error!(song = song.id, "saving a song: {e}");
        }
        // Java sends the list it read before saving.
        if let Some(saved) = song::song(&world.store, song_id) {
            session.send(out::song_info(&saved));
        }
        session.send(out::song_update());
        session.send(out::song_list(&songs));
        if let Some(msg) = playlist_message(world, machine.id) {
            r.send(msg);
        }
    });
    result
}

/// Java: `BURN_SONG`: a `song_disk` with the song for 1 credit, for anyone with rights in a room
/// with a sound machine or jukebox. Its custom data is
/// `name\nday\nmonth (0-11)\nyear\nlength\ntitle`.
pub fn burn_song(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut p = session.player.lock();
    let Some(user) = p.details.clone() else {
        return Ok(());
    };
    let is_mod = p.has_fuse("fuse_mod");
    {
        let r = room.lock();
        if machine(&r).is_none() {
            return Ok(());
        }
        if !r.has_rights(user.id, true) && !is_mod {
            return Ok(());
        }
    }
    if user.credits <= 0 {
        return Ok(());
    }
    let song_id = reader.int().ok_or(Malformed)?;
    let Some(song) = song::song(&world.store, song_id) else {
        return Ok(());
    };
    let Some(definition) = world.items.definition_by_sprite("song_disk") else {
        return Ok(()); // Java: a NullPointerException.
    };
    let today = chrono::Local::now();
    let custom_data = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        user.username,
        today.day(),
        today.month0(),
        today.year(),
        calculate_song_length(&song.data),
        song.title
    );
    let t = now();
    let draft =
        havana_storage::Item::with_defaults(user.id, definition.id() as u32, custom_data, t);
    let created = world.store.write().and_then(|mut w| {
        let created = w.insert_item(&draft, t)?;
        w.commit()?;
        Ok(created)
    });
    let created = match created {
        Ok(created) => created,
        Err(e) => {
            error!(user = user.id, "burning a song: {e}");
            return Ok(());
        }
    };
    let disk_id = created.id;
    let Some(disk) = world.items.make_item(created) else {
        return Ok(());
    };
    p.inventory.add_item(disk);
    send_hand(world, session, &mut p);
    let row = havana_storage::SongDisk {
        item_id: disk_id as i64,
        soundmachine_id: 0,
        slot_id: 0,
        song_id,
        burned_at: t,
    };
    if let Err(e) = world.store.add_disk(&row) {
        error!(item = disk_id, "saving a song disc: {e}");
    }
    if let Err(e) = world.store.update_song(song.id, |s| s.burnt = true) {
        error!(song = song.id, "marking a song burnt: {e}");
    }
    if let Some(details) = p.details.as_mut() {
        currency::decrease_credits(&world.store, details, 1);
        session.send(out_user::credit_balance(details.credits));
    }
    Ok(())
}
