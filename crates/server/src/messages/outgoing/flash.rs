//! Java: `messages/flash/outgoing`: what only the Flash client is sent.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

/// Java: `HOME_ROOM`.
pub fn home_room(room_id: i32) -> Message {
    Message::new(455, ServerBody::new().int(room_id).build())
}

/// Java: `FLASH_FAVOURITES`: the limit and the favourite rooms' ids.
pub fn favourites(limit: i32, room_ids: &[i32]) -> Message {
    let mut body = ServerBody::new().int(limit).int(room_ids.len() as i32);
    for id in room_ids {
        body = body.int(*id);
    }
    Message::new(458, body.build())
}

/// Java: `FLASH_FAVOURITE_STATUS`.
pub fn favourite_status(room_id: i32, added: bool) -> Message {
    Message::new(459, ServerBody::new().int(room_id).bool(added).build())
}

/// Java: `FLASH_MODTOOL`: the moderation tool's (fixed) presets and permissions.
pub fn modtool() -> Message {
    let ones = |body: ServerBody, n: usize| (0..n).fold(body, |b, _| b.int(1));
    let mut body = ServerBody::new().int(-1).int(1).text("test").int(0).int(14);
    body = ones(body, 6);
    body = body.int(1).text("room preset 1");
    body = ones(body, 24).text("test");
    body = ones(body, 24).text("test");
    body = ones(body, 24).text("test");
    Message::new(531, body.build())
}
