//! Room dimmer presets and the sound machine tables (Java: `MoodlightDao`, `SongMachineDao`,
//! `JukeboxDao`): one function per table.

use anyhow::{Context, Result};
use havana_storage::{
    MoodlightPresets, PlaylistEntry, Song, SongDisk, SoundMachineTrack, Store, Writer,
};
use mysql::Row;
use mysql::prelude::Queryable;

use crate::{auto_increment, col};

/// Returns the number of rows copied.
pub fn import_sound(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let mut w = store.write()?;
    let count = import_moodlight_presets(conn, &mut w)?
        + import_songs(conn, &mut w)?
        + import_tracks(conn, &mut w)?
        + import_playlists(conn, &mut w)?
        + import_disks(conn, &mut w)?;
    w.commit()?;
    Ok(count)
}

/// `items_moodlight_presets`.
fn import_moodlight_presets(conn: &mut mysql::PooledConn, w: &mut Writer) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM items_moodlight_presets")?;
    for row in &rows {
        w.put_moodlight_presets(&MoodlightPresets {
            item_id: col(row, "item_id")?,
            current_preset: col(row, "current_preset")?,
            presets: [
                col(row, "preset_1")?,
                col(row, "preset_2")?,
                col(row, "preset_3")?,
            ],
        })?;
    }
    Ok(rows.len())
}

/// `soundmachine_songs`, with its `AUTO_INCREMENT`.
fn import_songs(conn: &mut mysql::PooledConn, w: &mut Writer) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM soundmachine_songs ORDER BY id")?;
    for row in &rows {
        w.put_song(&Song {
            id: col(row, "id")?,
            user_id: col(row, "user_id")?,
            title: col(row, "title")?,
            item_id: col(row, "item_id")?,
            length: col(row, "length")?,
            data: col(row, "data")?,
            burnt: col::<i32>(row, "burnt")? != 0,
        })?;
    }
    if let Some(next) = auto_increment(conn, "soundmachine_songs")? {
        w.set_song_auto_increment(next)?;
    }
    Ok(rows.len())
}

// The three keyless tables are read without an ORDER BY: MariaDB returns them in insertion
// order, which is the order they keep here.

/// `soundmachine_tracks`.
fn import_tracks(conn: &mut mysql::PooledConn, w: &mut Writer) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM soundmachine_tracks")?;
    for row in &rows {
        w.push_track(&SoundMachineTrack {
            soundmachine_id: col(row, "soundmachine_id")?,
            track_id: col(row, "track_id")?,
            slot_id: col(row, "slot_id")?,
        })?;
    }
    Ok(rows.len())
}

/// `soundmachine_playlists`.
fn import_playlists(conn: &mut mysql::PooledConn, w: &mut Writer) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM soundmachine_playlists")?;
    for row in &rows {
        w.push_playlist_entry(&PlaylistEntry {
            item_id: col(row, "item_id")?,
            song_id: col(row, "song_id")?,
            slot_id: col(row, "slot_id")?,
        })?;
    }
    Ok(rows.len())
}

/// `soundmachine_disks`.
fn import_disks(conn: &mut mysql::PooledConn, w: &mut Writer) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM soundmachine_disks")?;
    for row in &rows {
        w.push_disk(&SongDisk {
            item_id: col(row, "item_id")?,
            soundmachine_id: col(row, "soundmachine_id")?,
            slot_id: col(row, "slot_id")?,
            song_id: col(row, "song_id")?,
            burned_at: col(row, "burned_at").context("soundmachine_disks.burned_at")?,
        })?;
    }
    Ok(rows.len())
}
