//! Java: `messages/flash/outgoing`: what only the Flash client is sent.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use crate::game::navigator::flash::{FlashRoom, icon_ints};

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

fn icon(body: ServerBody, room: &FlashRoom, item_try: bool) -> ServerBody {
    icon_ints(&room.icon_data, item_try)
        .into_iter()
        .fold(body, |b, v| b.int(v))
}

fn tags(body: ServerBody, room: &FlashRoom) -> ServerBody {
    room.tags
        .iter()
        .fold(body.int(room.tags.len() as i32), |b, t| b.text(t))
}

/// An event shown in `FLASH_FLAT_RESULTS`: its name, category and start time (`hh:mm AM`).
pub struct FlatEvent {
    pub name: String,
    pub category_id: i32,
    pub started: String,
}

/// Java: `FLASH_FLAT_RESULTS` (451). `None` where Java throws (a room without its category).
pub fn flat_results(
    rooms: &[FlashRoom],
    event_of: impl Fn(u32) -> Option<FlatEvent>,
) -> Option<Message> {
    let mut body = ServerBody::new().int(0).text("").int(rooms.len() as i32);
    for room in rooms {
        let e = &room.entry;
        let trading = room.allow_trading?;
        body = body.int(e.id as i32);
        body = match event_of(e.id) {
            Some(event) => body
                .bool(true)
                .text(&event.name)
                .text(&e.owner_name)
                .int(e.access_type)
                .int(e.visitors_now)
                .int(e.visitors_max)
                .text(&e.description)
                .int(1)
                .bool(trading)
                .int(e.rating)
                .int(event.category_id)
                .text(&event.started),
            None => body
                .bool(false)
                .text(&e.name)
                .text(&e.owner_name)
                .int(e.access_type)
                .int(e.visitors_now)
                .int(e.visitors_max)
                .text(&e.description)
                .int(1)
                .bool(trading)
                .int(e.rating)
                .int(e.category)
                .text(""),
        };
        body = tags(body, room);
        body = icon(body, room, true);
    }
    Some(Message::new(451, body.build()))
}

/// Java: `FLASH_SEARCHRESULTS` (451).
pub fn search_results(rooms: &[FlashRoom], query: &str) -> Option<Message> {
    let mut body = ServerBody::new()
        .int(1)
        .int(9)
        .text(query)
        .int(rooms.len() as i32);
    for room in rooms {
        let e = &room.entry;
        let trading = room.allow_trading?;
        body = body
            .int(e.id as i32)
            .bool(false)
            .text(&e.name)
            .text(&e.owner_name)
            .int(e.access_type)
            .int(e.visitors_now)
            .int(e.visitors_max)
            .text(&e.description)
            .int(1)
            .bool(trading)
            .int(e.rating)
            .int(0)
            .text("");
        body = tags(body, room).int(0).int(0).int(0);
    }
    Some(Message::new(451, body.build()))
}

/// Java: `FLASH_GET_GUEST_ROOM_RESULT` (451).
pub fn guest_room_result(force_display: i32, mode: i32, rooms: &[FlashRoom]) -> Option<Message> {
    let mut body = ServerBody::new()
        .int(force_display)
        .int(mode)
        .text("")
        .int(rooms.len() as i32);
    for room in rooms {
        let e = &room.entry;
        let trading = room.allow_trading?;
        body = body
            .int(e.id as i32)
            .bool(false)
            .text(&e.name)
            .text(&e.owner_name)
            .int(e.access_type)
            .int(e.visitors_now)
            .int(e.visitors_max)
            .text(&e.description)
            .int(0)
            .bool(trading)
            .int(0);
        body = tags(body, room);
        body = icon(body, room, true);
    }
    Some(Message::new(451, body.build()))
}

/// A public room with its navigator style, for `FLASH_FRONTPAGERESULT`.
pub struct FrontpageRoom<'a> {
    pub room: &'a FlashRoom,
    pub description: String,
    pub thumbnail_url: String,
    pub thumbnail_layout: i32,
}

/// Java: `FLASH_FRONTPAGERESULT` (450): the public rooms.
pub fn frontpage_result(rooms: &[FrontpageRoom]) -> Message {
    let mut body = ServerBody::new()
        .int(1)
        .text("Public Rooms")
        .int(rooms.len() as i32);
    for r in rooms {
        let e = &r.room.entry;
        body = body
            .text(&e.name)
            .text(&r.description)
            .int(r.thumbnail_layout)
            .text(&e.name)
            .text(&r.thumbnail_url)
            .int(e.total_visitors_now)
            .int(3)
            .text(&r.thumbnail_url)
            .int(e.id as i32)
            .int(0)
            .text(&e.ccts)
            .int(e.total_visitors_max)
            .int(e.id as i32);
    }
    Message::new(450, body.build())
}

/// Java: `FLASH_FLATINFO` (454), for a Flash client (not the beta one). `owner_name` is the
/// owner's name or `-` when it is hidden. `None` where Java throws.
pub fn flat_info(
    room: &FlashRoom,
    owner_name: &str,
    override_lock: bool,
    is_loading: bool,
    check_entry: bool,
) -> Option<Message> {
    let e = &room.entry;
    let trading = room.allow_trading?;
    let mut body = ServerBody::new()
        .bool(is_loading)
        .int(e.id as i32)
        .bool(check_entry)
        .text(&e.name)
        .text(owner_name)
        .int(if override_lock { 0 } else { e.access_type })
        .int(e.visitors_now)
        .int(e.visitors_max)
        .text(&e.description)
        .bool(true)
        .bool(trading)
        .int(e.rating)
        .int(e.category)
        .text("");
    body = tags(body, room);
    body = icon(body, room, false);
    Some(Message::new(454, body.bool(true).build()))
}

/// Java: `FLASH_PUBLICROOMCASTS` (453).
pub fn public_room_casts(room_id: i32, ccts: &str) -> Message {
    Message::new(
        453,
        ServerBody::new()
            .int(room_id)
            .text(ccts)
            .int(room_id)
            .build(),
    )
}

/// Java: `FLASH_ROOMENTRYINFO` (471): a public room's description, or a flat's id and whether
/// the player owns it.
pub fn room_entry_info(public: bool, room_id: i32, description: &str, owner: bool) -> Message {
    let body = ServerBody::new().bool(!public);
    let body = if public {
        body.text(description).bool(false)
    } else {
        body.int(room_id).bool(owner)
    };
    Message::new(471, body.build())
}

/// Java: `FLASH_EDITDATA` (465): the owner's room settings, with the users who have rights.
pub fn edit_data(room: &FlashRoom, rights: &[(u32, String)]) -> Message {
    let e = &room.entry;
    let mut body = ServerBody::new()
        .int(e.id as i32)
        .text(&e.name)
        .text(&e.description)
        .int(e.access_type)
        .int(e.category)
        .int(e.visitors_max)
        .int(e.visitors_max)
        .bool(false)
        .bool(false)
        .bool(false);
    body = tags(body, room).int(rights.len() as i32);
    for (id, name) in rights {
        body = body.int(*id as i32).text(name);
    }
    Message::new(465, body.int(rights.len() as i32).build())
}

/// Java: `FLASH_MODTOOL_ROOMDATA` (538).
pub fn modtool_room_data(
    room: &FlashRoom,
    owner_in_room: bool,
    event: Option<(&str, &str, &[String])>,
) -> Message {
    let e = &room.entry;
    let mut body = ServerBody::new()
        .int(e.id as i32)
        .int(e.total_visitors_now)
        .bool(owner_in_room)
        .int(e.owner_id as i32)
        .text(&e.owner_name)
        .int(e.id as i32)
        .text(&e.name)
        .text(&e.description);
    body = tags(body, room).bool(event.is_some());
    if let Some((name, description, event_tags)) = event {
        body = body
            .text(name)
            .text(description)
            .int(event_tags.len() as i32);
        for t in event_tags {
            body = body.text(t);
        }
    }
    Message::new(538, body.build())
}

/// A chat line for `FLASH_MODTOOL_CHATLOG`: hour, minute, user id, name and message.
pub struct ModChatLine {
    pub hour: i32,
    pub minute: i32,
    pub user_id: u32,
    pub username: String,
    pub message: String,
}

/// Java: `FLASH_MODTOOL_CHATLOG` (535).
pub fn modtool_chatlog(public: bool, room_id: i32, name: &str, lines: &[ModChatLine]) -> Message {
    let mut body = ServerBody::new()
        .bool(public)
        .int(room_id)
        .text(name)
        .int(lines.len() as i32);
    for l in lines {
        body = body
            .int(l.hour)
            .int(l.minute)
            .int(l.user_id as i32)
            .text(&l.username)
            .text(&l.message);
    }
    Message::new(535, body.build())
}

/// Java: `WARDROBE` (267): the saved looks.
pub fn wardrobe(slots: &[(i32, String, String)]) -> Message {
    let mut body = ServerBody::new().bool(false).int(slots.len() as i32);
    for (slot, figure, sex) in slots {
        body = body.int(*slot).text(figure).text(sex);
    }
    Message::new(267, body.build())
}

/// The room id the client is told after a change of its settings or icon (Java's anonymous
/// composers 467 and 456).
pub fn room_updated(header: u16, room_id: i32) -> Message {
    Message::new(header, ServerBody::new().int(room_id).build())
}

/// Java: `FLASH_CANCREATEROOM`'s answer (512): never too many rooms, 25 at most.
pub fn can_create_room() -> Message {
    Message::new(512, ServerBody::new().bool(false).int(25).build())
}

/// Java: `FLASH_POPULARTAGS`' answer (452): room tags with their counts.
pub fn popular_tags(tags: &[(String, i32)]) -> Message {
    let mut body = ServerBody::new().int(tags.len() as i32);
    for (tag, count) in tags {
        body = body.text(tag).int(*count);
    }
    Message::new(452, body.build())
}
