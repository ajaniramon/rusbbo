//! Java: `messages/outgoing/rooms` (room entry and furniture), plus `CANTCONNECT` which room
//! entry sends.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use super::catalogue::latin1;
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
            .text(model_name)
            .int(room_id as i32)
            .build(),
    )
}

/// Java: `FLATPROPERTY`.
pub fn flat_property(property: &str, value: &str) -> Message {
    Message::new(46, ServerBody::new().text(property).text(value).build())
}

/// Java: `UPDATE_VOTES`; -1 makes the client show the vote buttons.
pub fn update_votes(rating: i32) -> Message {
    Message::new(345, ServerBody::new().int(rating).build())
}

/// Java: `HEIGHTMAP`: the model's rows, each followed by `\r`, written raw.
pub fn heightmap(heightmap: &str) -> Message {
    Message::new(31, heightmap.as_bytes().to_vec())
}

/// Java: `FLOOR_MAP`: the model's rows like `HEIGHTMAP`, with the door tile replaced by the door
/// height as a whole number (`(int) door.z`), written raw.
pub fn floor_map(model: &crate::game::room::model::RoomModel) -> Message {
    let mut body = Vec::with_capacity(model.heightmap.len());
    for (y, row) in model
        .heightmap
        .split('\r')
        .take(model.size_y as usize)
        .enumerate()
    {
        for (x, tile) in row.chars().enumerate() {
            if x as i32 == model.door_x && y as i32 == model.door_y {
                body.extend((model.door_z as i32).to_string().bytes());
            } else {
                body.extend(latin1(&tile.to_string()));
            }
        }
        body.push(b'\r');
    }
    Message::new(470, body)
}

/// Java: `HEIGHTMAP_UPDATE`: the walking heights of the open tiles (see
/// `RoomMapping::heightmap_update`), sent before furniture is placed, moved or picked up.
pub fn heightmap_update(map: &str) -> Message {
    Message::new(219, ServerBody::new().text(map).build())
}

/// Java: `Item.serialise` for public space objects.
fn public_item(body: ServerBody, item: &Item) -> ServerBody {
    let has_dimensions = item.has(Behaviour::ExtraParameter);
    let body = body
        .int(has_dimensions as i32)
        .text(&item.custom_data)
        .text(&item.sprite)
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
/// whose sprite id is -1 so the sprite name follows.
pub(crate) fn floor_item(body: ServerBody, item: &Item) -> ServerBody {
    let body = body
        .int(item.virtual_id)
        .int(item.sprite_id())
        .int(item.position.x)
        .int(item.position.y)
        .int(item.position.rotation())
        .text(&crate::game::format_decimal(item.position.z))
        // Rollers need 2 to animate.
        .int(if item.has(Behaviour::Roller) { 2 } else { 0 });
    let data = if item.has(Behaviour::Present) {
        let parts: Vec<&str> = item.custom_data.split('|').collect();
        format!("!{}", parts.get(2).copied().unwrap_or(""))
    } else if item.has(Behaviour::EcoBox) {
        let created = item.flat.as_ref().map_or(0, |f| f.record.created_at);
        crate::game::format_local(created, "%d-%m-%Y")
    } else if item.has(Behaviour::Redeemable) {
        item.sprite.split('_').nth(1).unwrap_or("").to_string()
    } else if item.has(Behaviour::PrizeTrophy) {
        item.trophy_data.clone().unwrap_or_default()
    } else {
        item.custom_data.clone()
    };
    let expire = item.expire_time();
    let minutes = if expire > -1 {
        ((expire - crate::game::now()) / 60) as i32
    } else {
        -1
    };
    let body = body.string(&latin1(&data)).int(minutes);
    if item.sprite_id() < 0 {
        body.text(&item.sprite)
    } else {
        body
    }
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

/// Java: `ITEMS(room)`: the room's wall items (answer to `G_ITEMS`).
pub fn items<'a>(wall_items: impl IntoIterator<Item = &'a Item>) -> Message {
    let items: Vec<&Item> = wall_items.into_iter().collect();
    let body = items
        .iter()
        .fold(ServerBody::new().int(items.len() as i32), |b, item| {
            wall_item(b, item)
        });
    Message::new(45, body.build())
}

/// Java: `Item.serialise` for wall items: the id as text, the sprite id, the wall position and
/// the custom data (only the colour of a post-it).
pub(crate) fn wall_item(body: ServerBody, item: &Item) -> ServerBody {
    let data = if item.has(Behaviour::PostIt) {
        item.custom_data.chars().take(6).collect()
    } else {
        item.custom_data.clone()
    };
    body.text(&item.virtual_id.to_string())
        .int(item.sprite_id())
        .string(&latin1(item.wall_position()))
        .string(&latin1(&data))
}

/// Java: `ROOMAD`: a billboard image and its link, or an empty string for none.
pub fn room_ad(ad: Option<(&str, &str)>) -> Message {
    let body = match ad {
        Some((image, url)) => ServerBody::new().text(image).text(url),
        None => ServerBody::new().string(b""),
    };
    Message::new(208, body.build())
}

/// Java: `INTERSITIALDATA`: the ad shown while a room loads; empty strings for none.
pub fn intersitial_data(ad: Option<(&str, &str)>) -> Message {
    let (image, url) = ad.unwrap_or(("", ""));
    Message::new(258, ServerBody::new().text(image).text(url).build())
}

/// Java: `FLAT_LETIN`: the user may enter the private room (`GOTOFLAT`).
pub fn flat_letin() -> Message {
    Message::new(41, Vec::new())
}

/// Java: `DOORBELL_WAIT`: empty to the visitor at the door, with their name to the users with
/// rights inside.
pub fn doorbell_wait(username: &str) -> Message {
    Message::new(
        91,
        ServerBody::new()
            .string(&super::catalogue::latin1(username))
            .build(),
    )
}

/// Java: `FLATNOTALLOWEDTOENTER`: the doorbell was not answered, or answered with no.
pub fn flat_not_allowed_to_enter() -> Message {
    Message::new(131, Vec::new())
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
    Banned = 4,
}

/// Java: `CANTCONNECT`.
pub fn cant_connect(error: ConnectError) -> Message {
    let body = ServerBody::new().int(error as i32);
    let body = match error {
        ConnectError::ClubOnly => body.string(b"c"),
        ConnectError::RoomFull | ConnectError::Banned => body,
    };
    Message::new(224, body.build())
}

/// Java: `PETSTAT`.
pub fn pet_stat(
    instance_id: i32,
    age: i32,
    hunger: i32,
    thirst: i32,
    happiness: i32,
    nature_negative: i32,
    nature_positive: i32,
) -> Message {
    Message::new(
        210,
        ServerBody::new()
            .int(instance_id)
            .int(age)
            .int(hunger)
            .int(thirst)
            .int(happiness)
            .int(nature_negative)
            .int(nature_positive)
            .build(),
    )
}

/// Java: `NAMEAPPROVED`: 0 when a pet name is fine (see `pet::name_status`).
pub fn name_approved(status: i32) -> Message {
    Message::new(36, ServerBody::new().int(status).build())
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
        assert_eq!(
            crate::messages::outgoing::events::room_event_info(None).body,
            b"-1\x02"
        );
        assert_eq!(active_objects(std::iter::empty()).body, b"H");
        assert_eq!(heightmap("xx0\r00\r").body, b"xx0\r00\r");
        assert_eq!(cant_connect(ConnectError::ClubOnly).body, b"Kc\x02");
    }

    #[test]
    fn flat_entry_bodies_match_flat_traces() {
        // traces/flat-create.json and flat-visit.json.
        assert_eq!(room_ready(1000, "model_a").body, b"model_a\x02XzC");
        assert_eq!(flat_property("landscape", "0").body, b"landscape\x020\x02");
        assert_eq!(update_votes(0).body, b"H");
        assert_eq!(items(std::iter::empty()).body, b"H");
        assert_eq!(items(std::iter::empty()).header, 45);
        assert_eq!(room_ad(None).body, b"\x02");
        assert_eq!(room_ad(None).header, 208);
        assert_eq!(flat_letin().header, 41);
        assert!(flat_letin().body.is_empty());
        assert_eq!(doorbell_wait("tracebot2").body, b"tracebot2\x02");
        assert_eq!(doorbell_wait("").body, b"\x02");
        assert_eq!(flat_not_allowed_to_enter().header, 131);
        assert!(flat_not_allowed_to_enter().body.is_empty());
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
            flat: None,
            rolling: None,
            current_roll_blocked: false,
            requires_update: false,
            trophy_data: None,
        };
        // From traces/public-room.json: "QNH" + "r697\x02broom_bench1\x02PAHPAPA" ...
        assert_eq!(
            objects_world([&item]).body,
            b"IHr697\x02broom_bench1\x02PAHPAPA"
        );
    }
}
