//! Java: `messages/outgoing/songs` (the sound machine's song editor and playlists).
//!
//! TODO(idol): `START_PLAYING_SONG` and `STOP_PLAYING_SONG` belong to the idol performances.

use std::collections::BTreeMap;

use havana_protocol::Message;
use havana_protocol::body::ServerBody;
use havana_storage::Song;

use crate::game::song::calculate_song_length;

/// Java: `SONG_INFO`.
pub fn song_info(song: &Song) -> Message {
    Message::new(
        300,
        ServerBody::new()
            .int(song.id as i32)
            .text(&song.title)
            .text(&song.data)
            .build(),
    )
}

/// Java: `SONG_LIST`: the machine's songs, their length halved.
pub fn song_list(songs: &[Song]) -> Message {
    let mut body = ServerBody::new().int(songs.len() as i32);
    for song in songs {
        body = body
            .int(song.id as i32)
            .int(calculate_song_length(&song.data) / 2)
            .text(&song.title)
            .bool(song.burnt);
    }
    Message::new(322, body.build())
}

/// Java: `SONG_LOCKED`: the song was burnt to a disc and cannot be overwritten.
pub fn song_locked() -> Message {
    Message::new(336, Vec::new())
}

/// Java: `SONG_NEW`: the machine's virtual id and the new song's title.
pub fn song_new(item_id: i32, title: &str) -> Message {
    Message::new(331, ServerBody::new().int(item_id).text(title).build())
}

/// Java: `SONG_PLAYLIST`: each song with its author's name. `None` when a song is missing,
/// where Java's composer throws and nothing is sent.
pub fn song_playlist(songs: &[(Option<Song>, Option<String>)]) -> Option<Message> {
    let mut body = ServerBody::new().int(0).int(songs.len() as i32);
    for (song, author) in songs {
        let song = song.as_ref()?;
        body = body
            .int(song.id as i32)
            .int(calculate_song_length(&song.data) / 2)
            .text(&song.title)
            .text(author.as_deref().unwrap_or(""));
    }
    Some(Message::new(323, body.build()))
}

/// Java: `SONG_UPDATE`.
pub fn song_update() -> Message {
    Message::new(339, Vec::new())
}

/// Java: `SOUND_PACKAGES`: the machine's slots (4) and the sound set in each, with its nine
/// sample ids.
pub fn sound_packages(tracks: &BTreeMap<i32, i32>) -> Message {
    let mut body = ServerBody::new().int(4).int(tracks.len() as i32);
    for (&slot, &sound_set) in tracks {
        body = body.int(slot).int(sound_set).int(9);
        let first = sound_set.wrapping_mul(9).wrapping_sub(8);
        for j in 0..9 {
            body = body.int(first.wrapping_add(j));
        }
    }
    Message::new(301, body.build())
}

/// Java: `USER_SOUND_PACKAGES`: the sound sets in the hand.
pub fn user_sound_packages(sound_sets: &[i32]) -> Message {
    let mut body = ServerBody::new().int(sound_sets.len() as i32);
    for &set in sound_sets {
        body = body.int(set);
    }
    Message::new(302, body.build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_packages_lists_nine_samples_per_set() {
        let tracks = BTreeMap::from([(1, 2)]);
        let msg = sound_packages(&tracks);
        assert_eq!(msg.header, 301);
        // 4 slots, 1 set: slot 1, set 2, 9 samples 10..=18.
        let mut expected = ServerBody::new().int(4).int(1).int(1).int(2).int(9);
        for j in 10..=18 {
            expected = expected.int(j);
        }
        assert_eq!(msg.body, expected.build());
    }

    #[test]
    fn playlist_needs_every_song() {
        let song = Song {
            id: 1,
            title: "First".into(),
            data: "1:1,4;2,4:2:3,2:3:0,1:4:0,1:".into(),
            ..Default::default()
        };
        let msg = song_playlist(&[(Some(song.clone()), Some("tracebot1".into()))]).unwrap();
        assert_eq!(msg.body, b"HIIPBFirst\x02tracebot1\x02");
        assert!(song_playlist(&[(Some(song), None), (None, None)]).is_none());
    }
}
