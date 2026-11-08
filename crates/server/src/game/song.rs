//! Songs made in a sound machine and the discs burnt from them (Java: `game/song`,
//! `SongCache`, `JukeboxManager`, `ItemManager.calculateSongLength` and
//! `Inventory.getSoundsets`).
//!
//! Java caches songs by id (`SongCache`); every write goes through the cache's `invalidate`, so
//! reading the store each time gives the same answers.

use std::collections::BTreeMap;

use rusbbo_storage::{PlaylistEntry, Song, SongDisk, Store};
use tracing::error;

use crate::game::item::Item;
use crate::game::item::base::ItemBehaviour;

/// Java's `String.split(String.valueOf(sep))` for a separator that is not a regex
/// metacharacter: no trailing empty strings, and an input without the separator is itself.
pub fn java_split(text: &str, sep: char) -> Vec<&str> {
    if !text.contains(sep) {
        return vec![text];
    }
    let mut parts: Vec<&str> = text.split(sep).collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
}

/// Java: `Integer.parseInt`.
pub fn parse_int(text: &str) -> Option<i32> {
    text.parse().ok()
}

/// Java: `ItemManager.calculateSongLength`: the longest of the four tracks of trax data
/// (`1:sample,length;...:2:...:3:...:4:...:`), two seconds per length unit. 0 when the data
/// does not parse (Java catches the exception).
pub fn calculate_song_length(song: &str) -> i32 {
    let Some((last, _)) = song.char_indices().last() else {
        return 0; // substring(0, -1) throws.
    };
    let data = song[..last]
        .replace(":4:", "|")
        .replace(":3:", "|")
        .replace(":2:", "|")
        .replace("1:", "");
    let tracks = java_split(&data, '|');
    if tracks.len() < 4 {
        return 0;
    }
    let mut song_length = 0i32;
    for track in &tracks[..4] {
        let mut track_length = 0i32;
        for sample in java_split(track, ';') {
            let Some(seconds) = java_split(sample, ',').get(1).and_then(|s| parse_int(s)) else {
                return 0;
            };
            track_length = track_length.wrapping_add(seconds.wrapping_mul(2));
        }
        if track_length > song_length {
            song_length = track_length;
        }
    }
    song_length
}

/// Java: `SongCache.get`: the song, if `id` is one.
pub fn song(store: &Store, id: i32) -> Option<Song> {
    let id = u32::try_from(id).ok()?;
    match store.song(id) {
        Ok(song) => song,
        Err(e) => {
            error!(song = id, "loading song: {e}");
            None
        }
    }
}

/// Java: `SongMachineDao.getTracks`: slot -> sound set. Java's `HashMap` of small integer keys
/// lists the slots in ascending order (the slots are 1-4).
pub fn tracks(store: &Store, machine_id: i64) -> BTreeMap<i32, i32> {
    let rows = store.machine_tracks(machine_id).unwrap_or_else(|e| {
        error!(machine = machine_id, "loading sound machine tracks: {e}");
        Vec::new()
    });
    rows.into_iter().map(|t| (t.slot_id, t.track_id)).collect()
}

/// Java: `SongMachineDao.getSongList`.
pub fn machine_songs(store: &Store, machine_id: i64) -> Vec<Song> {
    store.machine_songs(machine_id).unwrap_or_else(|e| {
        error!(machine = machine_id, "loading songs: {e}");
        Vec::new()
    })
}

/// Java: `SongMachineDao.getSongPlaylist`: the playlist rows, each with its song (`None` when
/// the song row is missing; Java then fails composing `SONG_PLAYLIST`).
pub fn playlist(store: &Store, machine_id: i64) -> Vec<(PlaylistEntry, Option<Song>)> {
    let rows = store.playlist(machine_id).unwrap_or_else(|e| {
        error!(machine = machine_id, "loading playlist: {e}");
        Vec::new()
    });
    rows.into_iter()
        .map(|entry| {
            let song = song(store, entry.song_id);
            (entry, song)
        })
        .collect()
}

/// Java: `JukeboxManager.getDisks`: the discs loaded into a jukebox whose song exists.
pub fn jukebox_disks(store: &Store, machine_id: i64) -> Vec<(SongDisk, Song)> {
    let rows = store.machine_disks(machine_id).unwrap_or_else(|e| {
        error!(machine = machine_id, "loading jukebox discs: {e}");
        Vec::new()
    });
    rows.into_iter()
        .filter_map(|disk| {
            let song = song(store, disk.song_id)?;
            Some((disk, song))
        })
        .collect()
}

/// The number of a sound set (`sound_set_<n>`): Java's
/// `Integer.parseInt(sprite.split("_")[2])`, `None` where that throws.
pub fn sound_set_number(item: &Item) -> Option<i32> {
    java_split(item.definition.sprite(), '_')
        .get(2)
        .and_then(|n| parse_int(n))
}

/// Java: `Inventory.getSoundsets`: the numbers of the sound sets in the hand, in hand order.
/// `None` where Java throws (a sound set sprite without a number).
pub fn hand_sound_sets(items: &[Item]) -> Option<Vec<i32>> {
    items
        .iter()
        .filter(|i| i.is_visible() && i.has_behaviour(ItemBehaviour::SoundMachineSampleSet))
        .map(sound_set_number)
        .collect()
}

/// The user's name for a song's author (Java: `PlayerDao.getName`; `null`, written as an empty
/// string, for an unknown user).
pub fn author_name(store: &Store, user_id: i32) -> Option<String> {
    let id = u32::try_from(user_id).ok()?;
    store.user(id).ok().flatten().map(|u| u.username)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_length_of_trax_data() {
        // Track 1: (4 + 4) * 2 = 16 s, the longest.
        assert_eq!(calculate_song_length("1:1,4;2,4:2:3,2:3:0,1:4:0,1:"), 16);
        assert_eq!(calculate_song_length("1:1,6;2,2:2:3,2:3:0,1:4:0,1:"), 16);
        assert_eq!(calculate_song_length("1:0,1:2:0,1:3:0,1:4:5,9:"), 18);
        // Anything that does not parse is 0.
        for bad in [
            "",
            ":",
            "garbage",
            "1:1,4:2:3,2:3:0,1:",
            "1:1,x:2:1,1:3:1,1:4:1,1:",
        ] {
            assert_eq!(calculate_song_length(bad), 0, "{bad:?}");
        }
    }

    #[test]
    fn split_like_java() {
        assert_eq!(java_split("a,,b,,", ','), ["a", "", "b"]);
        assert_eq!(java_split("", ','), [""]);
        assert!(java_split(",,", ',').is_empty());
        assert_eq!(java_split(",a", ','), ["", "a"]);
    }
}
