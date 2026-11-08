//! The `soundmachine_*` tables (Java: `SongMachineDao`, `JukeboxDao`): songs made in a sound
//! machine, the sound sets inserted into a machine, a machine's or jukebox's playlist, and the
//! song discs burnt from songs.
//!
//! Only `soundmachine_songs` has a key. The other three tables have none in MariaDB and keep
//! their rows in insertion order (the hidden row id), which is also the order Java reads them in
//! through their indexes. Here each of their rows gets a number from one counter
//! (`row_seq`), and the tables are keyed by (the column Java looks them up by, that number).

use redb::{ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, Writer, decode, encode};

const SONGS: TableDefinition<u32, &[u8]> = TableDefinition::new("soundmachine_songs");
/// `soundmachine_songs` by `item_id` (`idx_item_id`): (item id, song id) -> ().
const SONGS_BY_ITEM: TableDefinition<(i64, u32), ()> =
    TableDefinition::new("soundmachine_songs_by_item");
/// `soundmachine_tracks`: (soundmachine id, row) -> track.
const TRACKS: TableDefinition<(i64, u64), &[u8]> = TableDefinition::new("soundmachine_tracks");
/// `soundmachine_playlists`: (item id, row) -> entry.
const PLAYLISTS: TableDefinition<(i64, u64), &[u8]> =
    TableDefinition::new("soundmachine_playlists");
/// `soundmachine_disks`: row -> disc.
const DISKS: TableDefinition<u64, &[u8]> = TableDefinition::new("soundmachine_disks");
/// `soundmachine_disks` by `item_id` (`idx_item_id`): (item id, row) -> ().
const DISKS_BY_ITEM: TableDefinition<(i64, u64), ()> =
    TableDefinition::new("soundmachine_disks_by_item");
/// `soundmachine_disks` by `soundmachine_id` (`idx_machine_slot`): (soundmachine id, row) -> ().
const DISKS_BY_MACHINE: TableDefinition<(i64, u64), ()> =
    TableDefinition::new("soundmachine_disks_by_machine");
const META: TableDefinition<&str, u64> = TableDefinition::new("soundmachine_meta");
/// The next `soundmachine_songs.id` (its `AUTO_INCREMENT`).
const SONGS_AUTO_INCREMENT: &str = "songs_auto_increment";
/// The next row number of the keyless tables.
const ROW_SEQ: &str = "row_seq";

pub(crate) fn create_tables(tx: &WriteTransaction) -> Result<()> {
    tx.open_table(SONGS)?;
    tx.open_table(SONGS_BY_ITEM)?;
    tx.open_table(TRACKS)?;
    tx.open_table(PLAYLISTS)?;
    tx.open_table(DISKS)?;
    tx.open_table(DISKS_BY_ITEM)?;
    tx.open_table(DISKS_BY_MACHINE)?;
    let mut meta = tx.open_table(META)?;
    for counter in [SONGS_AUTO_INCREMENT, ROW_SEQ] {
        if meta.get(counter)?.is_none() {
            meta.insert(counter, 1)?;
        }
    }
    Ok(())
}

/// A row of `soundmachine_songs` (Java: `Song`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub id: u32,
    pub user_id: i32,
    pub title: String,
    /// The sound machine the song was made in; -1 once deleted (Java keeps the row for the
    /// discs burnt from it).
    pub item_id: i64,
    /// In seconds (`ItemManager.calculateSongLength`).
    pub length: i32,
    pub data: String,
    pub burnt: bool,
}

/// A row of `soundmachine_tracks`: a sound set inserted into a machine's slot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoundMachineTrack {
    pub soundmachine_id: i64,
    /// The sound set number (`sound_set_<n>`).
    pub track_id: i32,
    pub slot_id: i32,
}

/// A row of `soundmachine_playlists` (Java: `SongPlaylist`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaylistEntry {
    /// The sound machine or jukebox.
    pub item_id: i64,
    pub song_id: i32,
    pub slot_id: i32,
}

/// A row of `soundmachine_disks` (Java: `BurnedDisk`): a `song_disk` item and the song on it,
/// with the jukebox and slot it is loaded into (0 and 0 while it is in the hand).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SongDisk {
    pub item_id: i64,
    pub soundmachine_id: i64,
    pub slot_id: i32,
    pub song_id: i32,
    /// Unix seconds.
    pub burned_at: i64,
}

impl Store {
    /// Java: `SongMachineDao.getSong`.
    /// Java: `SongMachineDao.getSongUserList`: the songs a user made.
    pub fn songs_by_user(&self, user_id: u32) -> Result<Vec<Song>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(SONGS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let song: Song = decode(entry?.1.value())?;
            if song.user_id == user_id as i32 {
                out.push(song);
            }
        }
        Ok(out)
    }

    pub fn song(&self, id: u32) -> Result<Option<Song>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(SONGS)?;
        table.get(id)?.map(|v| decode(v.value())).transpose()
    }

    /// Java: `SongMachineDao.getSongList`: the songs of a machine, by id.
    pub fn machine_songs(&self, item_id: i64) -> Result<Vec<Song>> {
        let tx = self.db.begin_read()?;
        let songs = tx.open_table(SONGS)?;
        let mut out = Vec::new();
        for entry in tx
            .open_table(SONGS_BY_ITEM)?
            .range((item_id, 0)..=(item_id, u32::MAX))?
        {
            let id = entry?.0.value().1;
            if let Some(bytes) = songs.get(id)? {
                out.push(decode(bytes.value())?);
            }
        }
        Ok(out)
    }

    /// Java: `SongMachineDao.addSong`. Returns the new song with its id.
    pub fn insert_song(&self, song: Song) -> Result<Song> {
        let mut w = self.write()?;
        let song = w.insert_song(song)?;
        w.commit()?;
        Ok(song)
    }

    /// Java: `SongMachineDao.saveSong`, `deleteSong` (`item_id = -1`) and `JukeboxDao.setBurned`:
    /// an `UPDATE` of one song, so nothing happens without the row.
    pub fn update_song(&self, id: u32, change: impl FnOnce(&mut Song)) -> Result<()> {
        let mut w = self.write()?;
        let row: Option<Song> = {
            let table = w.tx.open_table(SONGS)?;
            let row = table.get(id)?;
            row.map(|v| decode(v.value())).transpose()?
        };
        if let Some(mut song) = row {
            change(&mut song);
            song.id = id;
            w.put_song(&song)?;
        }
        w.commit()
    }

    /// Java: `SongMachineDao.getTracks`' rows, in table order (Java puts them in a map by slot,
    /// so a later row for a slot wins).
    pub fn machine_tracks(&self, soundmachine_id: i64) -> Result<Vec<SoundMachineTrack>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(TRACKS)?;
        let mut out = Vec::new();
        for entry in table.range((soundmachine_id, 0)..=(soundmachine_id, u64::MAX))? {
            out.push(decode(entry?.1.value())?);
        }
        Ok(out)
    }

    /// Java: `SongMachineDao.addTrack`.
    pub fn add_track(&self, soundmachine_id: i64, track_id: i32, slot_id: i32) -> Result<()> {
        let mut w = self.write()?;
        w.push_track(&SoundMachineTrack {
            soundmachine_id,
            track_id,
            slot_id,
        })?;
        w.commit()
    }

    /// Java: `SongMachineDao.removeTrack`: every row of that machine and slot.
    pub fn remove_track(&self, soundmachine_id: i64, slot_id: i32) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(TRACKS)?;
            let mut doomed = Vec::new();
            for entry in table.range((soundmachine_id, 0)..=(soundmachine_id, u64::MAX))? {
                let (key, value) = entry?;
                let track: SoundMachineTrack = decode(value.value())?;
                if track.slot_id == slot_id {
                    doomed.push(key.value());
                }
            }
            for key in doomed {
                table.remove(key)?;
            }
        }
        w.commit()
    }

    /// Java: `SongMachineDao.getSongPlaylist`'s rows: `ORDER BY slot_id`, ties in table order.
    pub fn playlist(&self, item_id: i64) -> Result<Vec<PlaylistEntry>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(PLAYLISTS)?;
        let mut out: Vec<PlaylistEntry> = Vec::new();
        for entry in table.range((item_id, 0)..=(item_id, u64::MAX))? {
            out.push(decode(entry?.1.value())?);
        }
        out.sort_by_key(|e| e.slot_id);
        Ok(out)
    }

    /// Java: `SongMachineDao.addPlaylist`.
    pub fn add_playlist_entry(&self, item_id: i64, song_id: i32, slot_id: i32) -> Result<()> {
        let mut w = self.write()?;
        w.push_playlist_entry(&PlaylistEntry {
            item_id,
            song_id,
            slot_id,
        })?;
        w.commit()
    }

    /// Java: `SongMachineDao.removePlaylistSong`: `DELETE ... LIMIT 1`, the first such row.
    pub fn remove_playlist_song(&self, song_id: i32, item_id: i64) -> Result<()> {
        self.remove_playlist_rows(item_id, |e| e.song_id == song_id, true)
    }

    /// Java: `SongMachineDao.clearPlaylist`.
    pub fn clear_playlist(&self, item_id: i64) -> Result<()> {
        self.remove_playlist_rows(item_id, |_| true, false)
    }

    fn remove_playlist_rows(
        &self,
        item_id: i64,
        matches: impl Fn(&PlaylistEntry) -> bool,
        only_first: bool,
    ) -> Result<()> {
        let w = self.write()?;
        {
            let mut table = w.tx.open_table(PLAYLISTS)?;
            let mut doomed = Vec::new();
            for entry in table.range((item_id, 0)..=(item_id, u64::MAX))? {
                let (key, value) = entry?;
                if matches(&decode(value.value())?) {
                    doomed.push(key.value());
                    if only_first {
                        break;
                    }
                }
            }
            for key in doomed {
                table.remove(key)?;
            }
        }
        w.commit()
    }

    /// Java: `JukeboxDao.getDisks`: the discs loaded into a jukebox, by slot (the order of
    /// `idx_machine_slot`).
    pub fn machine_disks(&self, soundmachine_id: i64) -> Result<Vec<SongDisk>> {
        let tx = self.db.begin_read()?;
        let disks = tx.open_table(DISKS)?;
        let mut out: Vec<SongDisk> = Vec::new();
        for entry in tx
            .open_table(DISKS_BY_MACHINE)?
            .range((soundmachine_id, 0)..=(soundmachine_id, u64::MAX))?
        {
            let row = entry?.0.value().1;
            if let Some(bytes) = disks.get(row)? {
                out.push(decode(bytes.value())?);
            }
        }
        out.sort_by_key(|d| d.slot_id);
        Ok(out)
    }

    /// Java: `JukeboxDao.getDisk(soundmachineId, slotId)`: the first disc in that slot.
    pub fn disk_in_slot(&self, soundmachine_id: i64, slot_id: i32) -> Result<Option<SongDisk>> {
        Ok(self
            .machine_disks(soundmachine_id)?
            .into_iter()
            .find(|d| d.slot_id == slot_id))
    }

    /// Java: `JukeboxDao.getSongIdByItem`'s row: the first disc row of that item.
    pub fn disk_by_item(&self, item_id: i64) -> Result<Option<SongDisk>> {
        let tx = self.db.begin_read()?;
        let disks = tx.open_table(DISKS)?;
        let index = tx.open_table(DISKS_BY_ITEM)?;
        let first = index
            .range((item_id, 0)..=(item_id, u64::MAX))?
            .next()
            .transpose()?
            .map(|(k, _)| k.value().1);
        match first {
            Some(row) => disks.get(row)?.map(|v| decode(v.value())).transpose(),
            None => Ok(None),
        }
    }

    /// Java: `JukeboxDao.addDisk`.
    pub fn add_disk(&self, disk: &SongDisk) -> Result<()> {
        let mut w = self.write()?;
        w.push_disk(disk)?;
        w.commit()
    }

    /// Java: `JukeboxDao.editDisk`: every disc row of the item moves to that jukebox and slot.
    pub fn move_disk(&self, item_id: i64, soundmachine_id: i64, slot_id: i32) -> Result<()> {
        let mut w = self.write()?;
        let rows: Vec<u64> = {
            let index = w.tx.open_table(DISKS_BY_ITEM)?;
            index
                .range((item_id, 0)..=(item_id, u64::MAX))?
                .map(|e| e.map(|(k, _)| k.value().1))
                .collect::<std::result::Result<_, _>>()?
        };
        for row in rows {
            let old: Option<SongDisk> = {
                let table = w.tx.open_table(DISKS)?;
                let value = table.get(row)?;
                value.map(|v| decode(v.value())).transpose()?
            };
            let Some(mut disk) = old else {
                continue;
            };
            w.tx.open_table(DISKS_BY_MACHINE)?
                .remove((disk.soundmachine_id, row))?;
            disk.soundmachine_id = soundmachine_id;
            disk.slot_id = slot_id;
            w.put_disk_row(row, &disk)?;
        }
        w.commit()
    }
}

impl Writer {
    fn next(&mut self, counter: &str) -> Result<u64> {
        let mut meta = self.tx.open_table(META)?;
        let next = meta.get(counter)?.map_or(1, |v| v.value());
        meta.insert(counter, next + 1)?;
        Ok(next)
    }

    /// Inserts or replaces a song and keeps the by-item index in step (import, and updates).
    pub fn put_song(&mut self, song: &Song) -> Result<()> {
        let old: Option<Song> = {
            let table = self.tx.open_table(SONGS)?;
            let row = table.get(song.id)?;
            row.map(|v| decode(v.value())).transpose()?
        };
        {
            let mut index = self.tx.open_table(SONGS_BY_ITEM)?;
            if let Some(old) = old {
                index.remove((old.item_id, old.id))?;
            }
            index.insert((song.item_id, song.id), ())?;
        }
        let bytes = encode(song)?;
        self.tx
            .open_table(SONGS)?
            .insert(song.id, bytes.as_slice())?;
        let mut meta = self.tx.open_table(META)?;
        let next = meta.get(SONGS_AUTO_INCREMENT)?.map_or(1, |v| v.value());
        if u64::from(song.id) >= next {
            meta.insert(SONGS_AUTO_INCREMENT, u64::from(song.id) + 1)?;
        }
        Ok(())
    }

    /// A new song under the next id (`AUTO_INCREMENT`).
    pub fn insert_song(&mut self, mut song: Song) -> Result<Song> {
        let id = self
            .tx
            .open_table(META)?
            .get(SONGS_AUTO_INCREMENT)?
            .map_or(1, |v| v.value());
        song.id = u32::try_from(id).unwrap_or(u32::MAX);
        self.put_song(&song)?;
        Ok(song)
    }

    /// Carries over `soundmachine_songs`' `AUTO_INCREMENT` (import).
    pub fn set_song_auto_increment(&mut self, next: u64) -> Result<()> {
        let mut meta = self.tx.open_table(META)?;
        let current = meta.get(SONGS_AUTO_INCREMENT)?.map_or(1, |v| v.value());
        if next > current {
            meta.insert(SONGS_AUTO_INCREMENT, next)?;
        }
        Ok(())
    }

    /// Appends a `soundmachine_tracks` row.
    pub fn push_track(&mut self, track: &SoundMachineTrack) -> Result<()> {
        let row = self.next(ROW_SEQ)?;
        let bytes = encode(track)?;
        self.tx
            .open_table(TRACKS)?
            .insert((track.soundmachine_id, row), bytes.as_slice())?;
        Ok(())
    }

    /// Appends a `soundmachine_playlists` row.
    pub fn push_playlist_entry(&mut self, entry: &PlaylistEntry) -> Result<()> {
        let row = self.next(ROW_SEQ)?;
        let bytes = encode(entry)?;
        self.tx
            .open_table(PLAYLISTS)?
            .insert((entry.item_id, row), bytes.as_slice())?;
        Ok(())
    }

    /// Appends a `soundmachine_disks` row.
    pub fn push_disk(&mut self, disk: &SongDisk) -> Result<()> {
        let row = self.next(ROW_SEQ)?;
        self.tx
            .open_table(DISKS_BY_ITEM)?
            .insert((disk.item_id, row), ())?;
        self.put_disk_row(row, disk)
    }

    fn put_disk_row(&mut self, row: u64, disk: &SongDisk) -> Result<()> {
        self.tx
            .open_table(DISKS_BY_MACHINE)?
            .insert((disk.soundmachine_id, row), ())?;
        let bytes = encode(disk)?;
        self.tx.open_table(DISKS)?.insert(row, bytes.as_slice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        (dir, store)
    }

    #[test]
    fn songs_by_machine_and_updates() {
        let (_dir, store) = store();
        let mut w = store.write().unwrap();
        w.put_song(&Song {
            id: 4,
            item_id: 10,
            title: "Old".into(),
            ..Default::default()
        })
        .unwrap();
        w.commit().unwrap();
        let new = store
            .insert_song(Song {
                item_id: 10,
                title: "New".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(new.id, 5, "after the imported ids");
        store
            .insert_song(Song {
                item_id: 11,
                ..Default::default()
            })
            .unwrap();
        let titles: Vec<String> = store
            .machine_songs(10)
            .unwrap()
            .into_iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(titles, ["Old", "New"]);

        store.update_song(4, |s| s.item_id = -1).unwrap();
        store.update_song(99, |s| s.burnt = true).unwrap();
        assert_eq!(store.machine_songs(10).unwrap().len(), 1);
        assert_eq!(store.song(4).unwrap().unwrap().item_id, -1);
        assert!(store.song(99).unwrap().is_none());
    }

    #[test]
    fn tracks_and_playlists() {
        let (_dir, store) = store();
        store.add_track(1, 7, 1).unwrap();
        store.add_track(1, 9, 2).unwrap();
        store.add_track(2, 3, 1).unwrap();
        store.remove_track(1, 1).unwrap();
        let tracks = store.machine_tracks(1).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!((tracks[0].track_id, tracks[0].slot_id), (9, 2));

        store.add_playlist_entry(1, 5, 2).unwrap();
        store.add_playlist_entry(1, 6, 1).unwrap();
        store.add_playlist_entry(1, 5, 3).unwrap();
        let songs = |store: &Store| -> Vec<(i32, i32)> {
            store
                .playlist(1)
                .unwrap()
                .iter()
                .map(|e| (e.song_id, e.slot_id))
                .collect()
        };
        assert_eq!(songs(&store), [(6, 1), (5, 2), (5, 3)]);
        // LIMIT 1: only the first row of the song goes.
        store.remove_playlist_song(5, 1).unwrap();
        assert_eq!(songs(&store), [(6, 1), (5, 3)]);
        store.clear_playlist(1).unwrap();
        assert!(store.playlist(1).unwrap().is_empty());
    }

    #[test]
    fn disks_move_between_hand_and_jukebox() {
        let (_dir, store) = store();
        store
            .add_disk(&SongDisk {
                item_id: 30,
                song_id: 2,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(store.disk_by_item(30).unwrap().unwrap().song_id, 2);
        assert!(store.disk_by_item(31).unwrap().is_none());
        assert!(store.machine_disks(8).unwrap().is_empty());

        store.move_disk(30, 8, 3).unwrap();
        assert_eq!(store.machine_disks(8).unwrap().len(), 1);
        assert_eq!(store.disk_in_slot(8, 3).unwrap().unwrap().item_id, 30);
        assert!(store.disk_in_slot(8, 1).unwrap().is_none());
        assert!(store.machine_disks(0).unwrap().is_empty());

        store.move_disk(30, 0, 0).unwrap();
        assert!(store.machine_disks(8).unwrap().is_empty());
        assert_eq!(store.disk_by_item(30).unwrap().unwrap().slot_id, 0);
    }
}
