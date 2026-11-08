//! Java: `messages/outgoing/jukebox` (the discs in a jukebox and in the hand).

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;
use rusbbo_storage::{Song, SongDisk};

use crate::game::song::calculate_song_length;

/// Java: `JUKEBOX_DISCS`: the jukebox's 10 slots and the discs loaded, each with its song and
/// the song's author. `None` when an author is unknown (Java's composer throws on the missing
/// player).
pub fn jukebox_discs(disks: &[(SongDisk, Song, Option<String>)]) -> Option<Message> {
    let mut body = ServerBody::new().int(10).int(disks.len() as i32);
    for (disk, song, author) in disks {
        body = body
            .int(disk.slot_id)
            .int(song.id as i32)
            .int(calculate_song_length(&song.data) / 2)
            .text(&song.title)
            .text(author.as_deref()?);
    }
    Some(Message::new(334, body.build()))
}

/// Java: `USER_SONG_DISKS`: the discs in the hand, by virtual id, with the song title from the
/// disc's custom data (`name\nday\nmonth\nyear\nlength\ntitle`).
pub fn user_song_disks(disks: &[(i32, &str)]) -> Message {
    let mut body = ServerBody::new().int(disks.len() as i32);
    for (virtual_id, custom_data) in disks {
        let title = custom_data.split('\n').nth(5).unwrap_or("");
        body = body.int(*virtual_id).text(title);
    }
    Message::new(333, body.build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_titles_come_from_the_custom_data() {
        let msg = user_song_disks(&[(7, "tracebot1\n7\n10\n2026\n16\nFirst"), (8, "x\n1")]);
        assert_eq!(msg.body, b"JSAFirst\x02PB\x02");
    }
}
