//! Java: `messages/outgoing/rooms` (room entry and furniture), plus `ROOMEEVENT_INFO` and
//! `CANTCONNECT` which room entry sends.

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

use crate::game::room::items::{Behaviour, Item};

/// Java: `OPEN_CONNECTION`.
pub fn open_connection() -> Message {
    Message::new(19, vec![])
}

/// Java: `ROOM_URL`.
pub fn room_url() -> Message {
    Message::new(166, ServerBody::new().string(b"/client/").build())
}

/// Java: `ROOM_READY`.
pub fn room_ready(room_id: u32, model_name: &str) -> Message {
    Message::new(
        69,
        ServerBody::new()
            .string(model_name.as_bytes())
            .int(room_id as i32)
            .build(),
    )
}

/// Java: `FLATPROPERTY`.
pub fn flat_property(property: &str, value: &str) -> Message {
    Message::new(
        46,
        ServerBody::new()
            .string(property.as_bytes())
            .string(value.as_bytes())
            .build(),
    )
}

/// Java: `UPDATE_VOTES`; -1 makes the client show the vote buttons.
pub fn update_votes(rating: i32) -> Message {
    Message::new(345, ServerBody::new().int(rating).build())
}

/// Java: `ROOMEEVENT_INFO` with no event. TODO(events): rooms_events.
pub fn room_event_info_none() -> Message {
    Message::new(370, ServerBody::new().string(b"-1").build())
}

/// Java: `HEIGHTMAP`: the model's rows, each followed by `\r`, written raw.
pub fn heightmap(heightmap: &str) -> Message {
    Message::new(31, heightmap.as_bytes().to_vec())
}

/// Java: `Item.serialise` for public space objects.
fn public_item(body: ServerBody, item: &Item) -> ServerBody {
    let has_dimensions = item.has(Behaviour::ExtraParameter);
    let body = body
        .int(has_dimensions as i32)
        .string(item.custom_data.as_bytes())
        .string(item.sprite.as_bytes())
        .int(item.position.x)
        .int(item.position.y)
        .int(item.position.z as i32);
    if has_dimensions {
        body.int(item.length).int(item.width)
    } else {
        body.int(item.position.rotation())
    }
}

/// Java: `OBJECTS_WORLD(room.getItemManager().getPublicItems())`.
pub fn objects_world<'a>(items: impl IntoIterator<Item = &'a Item>) -> Message {
    let items: Vec<&Item> = items.into_iter().collect();
    let body = items
        .iter()
        .fold(ServerBody::new().int(items.len() as i32), |b, item| {
            public_item(b, item)
        });
    Message::new(30, body.build())
}

/// Java: `Item.serialise` for floor items. Public rooms only have the pool queue tiles here,
/// whose sprite id is -1 so the sprite name follows. TODO(items): flat furniture (sprite ids,
/// rollers, presents, trophies, eco boxes, rentals).
fn floor_item(body: ServerBody, item: &Item) -> ServerBody {
    let body = body
        .int(item.virtual_id)
        .int(-1)
        .int(item.position.x)
        .int(item.position.y)
        .int(item.position.rotation())
        .string(crate::game::format_decimal(item.position.z).as_bytes())
        .int(0)
        .string(item.custom_data.as_bytes())
        .int(-1);
    body.string(item.sprite.as_bytes())
}

/// Java: `ACTIVE_OBJECTS(room)`: the floor items that are not public space objects.
pub fn active_objects<'a>(items: impl IntoIterator<Item = &'a Item>) -> Message {
    let items: Vec<&Item> = items.into_iter().collect();
    let body = items
        .iter()
        .fold(ServerBody::new().int(items.len() as i32), |b, item| {
            floor_item(b, item)
        });
    Message::new(32, body.build())
}

/// The literal `ACTIVE_OBJECTS` Java sends in `park_a` instead (the infobus queue tiles).
pub fn active_objects_park_a() -> Message {
    const BODY: &str = "SGSBMRDPBPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}JMPGRAH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}SAMPFSAJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}QBMRFSAPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}SFMSERBJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}SCMRFPBPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}REMPGQBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}PGMPFRBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}PCMPEPBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}QGMRFRBJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}QDMRDQBPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}RFMRERBJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}PFMSDRBJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}PDMPGPBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}RGMSFRBJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}RAMRESAPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}RBMPGSAH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}SDMREQBPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}QEMRFQBPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}RCMPFPBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}KMRDSAPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}PAMPESAJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}PBMQFSAJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}IMPGQAH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}SEMRDRBJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}QCMREPBPA0.0\u{2}I2\u{2}Mqueue_tile2\u{2}SGMPGRBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}QAMQESAJ0.0\u{2}I2\u{2}Mqueue_tile2\u{2}QFMPERBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}RDMPEQBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}PEMPFQBH0.0\u{2}I2\u{2}Mqueue_tile2\u{2}";
    Message::new(32, BODY.as_bytes().to_vec())
}

/// The literal `DuHH` Java's `G_OBJS` sends (`GROUP_BADGES` with no badges).
pub fn group_badges_empty() -> Message {
    Message::new(309, b"HH".to_vec())
}

/// The literal `DiHH` Java's `G_OBJS` sends (`ALIAS_TOGGLE`).
pub fn alias_toggle() -> Message {
    Message::new(297, b"HH".to_vec())
}

/// Java: `CANTCONNECT.ConnectError`.
#[derive(Clone, Copy)]
pub enum ConnectError {
    RoomFull = 1,
    /// `QUEUE_ERROR` with the club-only reason `c`.
    ClubOnly = 3,
}

/// Java: `CANTCONNECT`.
pub fn cant_connect(error: ConnectError) -> Message {
    let body = ServerBody::new().int(error as i32);
    let body = match error {
        ConnectError::ClubOnly => body.string(b"c"),
        ConnectError::RoomFull => body,
    };
    Message::new(224, body.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::pathfinder::Position;
    use crate::game::room::items::Interaction;

    #[test]
    fn entry_bodies_match_public_room_trace() {
        assert_eq!(room_url().body, b"/client/\x02");
        assert_eq!(room_ready(7, "ballroom").body, b"ballroom\x02SA");
        assert_eq!(
            flat_property("landscape", "0.0").body,
            b"landscape\x020.0\x02"
        );
        assert_eq!(update_votes(-1).body, b"M");
        assert_eq!(room_event_info_none().body, b"-1\x02");
        assert_eq!(active_objects(std::iter::empty()).body, b"H");
        assert_eq!(heightmap("xx0\r00\r").body, b"xx0\r00\r");
        assert_eq!(cant_connect(ConnectError::ClubOnly).body, b"Kc\x02");
    }

    #[test]
    fn public_items_serialise_like_trace() {
        let mut position = Position::new(4, 0);
        position.z = 4.0;
        position.set_rotation(4);
        let item = Item {
            virtual_id: 0,
            database_id: 0,
            custom_data: "r697".into(),
            sprite: "broom_bench1".into(),
            top_height: 1.0,
            length: 1,
            width: 1,
            behaviours: vec![Behaviour::CanSitOnTop, Behaviour::PublicSpaceObject],
            interaction: Interaction::Chair,
            position,
            current_program: String::new(),
            current_program_value: String::new(),
            teleport_to: None,
            swim_to: None,
        };
        // From traces/public-room.json: "QNH" + "r697\x02broom_bench1\x02PAHPAPA" ...
        assert_eq!(
            objects_world([&item]).body,
            b"IHr697\x02broom_bench1\x02PAHPAPA"
        );
    }
}
