//! Java: `messages/outgoing/navigator`.
//!
//! Text from the database is written in ISO-8859-1 like Java's `NettyResponse.writeString`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;
use rusbbo_storage::RoomCategory;

use crate::game::navigator::{PUBLIC_ROOM_OFFSET, RoomEntry, Viewer};

/// A sub category of a `NAVNODEINFO`, with the visitor counts Java reads while composing.
pub struct SubCategory<'a> {
    pub category: &'a RoomCategory,
    pub current_visitors: i32,
    pub max_visitors: i32,
}

use super::catalogue::latin1;

/// The owner name shown to `viewer`: `-` unless it is their room, the owner shows their name or
/// they may see all owners.
fn visible_owner(room: &RoomEntry, viewer: &Viewer) -> Vec<u8> {
    if room.owner_id == viewer.user_id || room.show_owner_name || viewer.sees_all_owners() {
        latin1(&room.owner_name)
    } else {
        b"-".to_vec()
    }
}

/// Java: `NAVNODEINFO`. `None` when Java's composer would throw (a public room description such
/// as `abc/x` whose door part is not a number): the client gets nothing.
pub fn nav_node_info(
    viewer: &Viewer,
    parent: &RoomCategory,
    rooms: &[RoomEntry],
    hide_full: bool,
    sub_categories: &[SubCategory],
    current_visitors: i32,
    max_visitors: i32,
) -> Option<Message> {
    let mut body = ServerBody::new()
        .bool(hide_full)
        .int(parent.id as i32)
        .int(if parent.public_spaces { 0 } else { 2 })
        .string(&latin1(&parent.name))
        .int(current_visitors)
        .int(max_visitors)
        .int(parent.parent_id);
    if !parent.public_spaces {
        body = body.int(rooms.len() as i32);
    }
    for room in rooms {
        if room.is_public() {
            // The description may carry the door after a slash: `description/door`.
            let (description, door) = match room.description.split_once('/') {
                Some((text, rest)) => {
                    // Java: split("/")[1], parsed with Integer.parseInt.
                    let part = rest.split('/').next().unwrap_or("");
                    (text, part.parse::<i32>().ok()?)
                }
                None => (room.description.as_str(), 0),
            };
            body = body
                .int(room.id as i32 + PUBLIC_ROOM_OFFSET)
                .int(1)
                .string(&latin1(&room.name))
                .int(room.total_visitors_now)
                .int(room.total_visitors_max)
                .int(room.category)
                .string(&latin1(description))
                .int(room.id as i32)
                .int(door)
                .string(&latin1(&room.ccts))
                .int(0)
                .int(1);
        } else {
            body = body
                .int(room.id as i32)
                .string(&latin1(&room.name))
                .string(&visible_owner(room, viewer))
                .text(room.access_type_name())
                .int(room.visitors_now)
                .int(room.visitors_max)
                .string(&latin1(&room.description));
        }
    }
    for sub in sub_categories {
        body = body
            .int(sub.category.id as i32)
            .int(0)
            .string(&latin1(&sub.category.name))
            .int(sub.current_visitors)
            .int(sub.max_visitors)
            .int(parent.id as i32);
    }
    Some(Message::new(220, body.build()))
}

/// Java: `USERFLATCATS`.
pub fn user_flat_cats(categories: &[&RoomCategory]) -> Message {
    let mut body = ServerBody::new().int(categories.len() as i32);
    for c in categories {
        body = body.int(c.id as i32).string(&latin1(&c.name));
    }
    Message::new(221, body.build())
}

/// Java: `NOFLATS`.
pub fn no_flats() -> Message {
    Message::new(58, Vec::new())
}

/// Java: `NOFLATSFORUSER`.
pub fn no_flats_for_user(username: &str) -> Message {
    Message::new(57, ServerBody::new().string(&latin1(username)).build())
}

/// Java: `FLAT_RESULTS`: the owner's name is always shown (these are the viewer's own rooms).
pub fn flat_results(rooms: &[RoomEntry]) -> Message {
    let mut body = ServerBody::new().int(rooms.len() as i32);
    for room in rooms {
        body = body
            .int(room.id as i32)
            .string(&latin1(&room.name))
            .string(&latin1(&room.owner_name))
            .text(room.access_type_name())
            .int(room.visitors_now)
            .int(room.visitors_max)
            .string(&latin1(&room.description));
    }
    Message::new(16, body.build())
}

/// Java: `SEARCH_FLAT_RESULTS`.
pub fn search_flat_results(rooms: &[RoomEntry], viewer: &Viewer) -> Message {
    flat_list(55, rooms, viewer)
}

/// Java: `RECOMMENDED_ROOM_LIST`: the same layout as `SEARCH_FLAT_RESULTS`.
pub fn recommended_room_list(rooms: &[RoomEntry], viewer: &Viewer) -> Message {
    flat_list(351, rooms, viewer)
}

fn flat_list(header: u16, rooms: &[RoomEntry], viewer: &Viewer) -> Message {
    let mut body = ServerBody::new().int(rooms.len() as i32);
    for room in rooms {
        body = body
            .int(room.id as i32)
            .string(&latin1(&room.name))
            .string(&visible_owner(room, viewer))
            .text(room.access_type_name())
            .int(room.visitors_now)
            .int(room.visitors_max)
            .string(&latin1(&room.description));
    }
    Message::new(header, body.build())
}

/// Java: `NODESPACEUSERS`.
pub fn node_space_users(names: &[String]) -> Message {
    let mut body = ServerBody::new().int(-1).int(names.len() as i32);
    for name in names {
        body = body.string(&latin1(name));
    }
    Message::new(223, body.build())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category(id: u32, name: &str, public_spaces: bool, parent_id: i32) -> RoomCategory {
        RoomCategory {
            id,
            name: name.into(),
            public_spaces,
            parent_id,
            ..Default::default()
        }
    }

    fn viewer() -> Viewer {
        Viewer {
            user_id: 1,
            name: "tracebot1".into(),
            rank: 1,
            has_club: false,
        }
    }

    const GUEST_CATEGORIES: [(u32, &str); 9] = [
        (112, "Restaurant, Bar & Night Club Rooms"),
        (113, "Trade floor"),
        (114, "Chill, Chat & Discussion Rooms"),
        (115, "Hair Salons & Modelling Rooms"),
        (116, "Maze & Theme Park Rooms"),
        (117, "Gaming & Race Rooms"),
        (118, "Help Centre Rooms"),
        (120, "Miscellaneous"),
        (121, "Flower Power Puzzle"),
    ];

    #[test]
    fn user_flat_cats_matches_trace() {
        let mut cats = vec![category(2, "No category", false, 0)];
        cats.extend(
            GUEST_CATEGORIES
                .iter()
                .map(|(id, n)| category(*id, n, false, 4)),
        );
        let refs: Vec<&RoomCategory> = cats.iter().collect();
        assert_eq!(
            user_flat_cats(&refs).body,
            b"RBJNo category\x02P\\Restaurant, Bar & Night Club Rooms\x02Q\\Trade floor\x02R\\Chill, Chat & Discussion Rooms\x02S\\Hair Salons & Modelling Rooms\x02P]Maze & Theme Park Rooms\x02Q]Gaming & Race Rooms\x02R]Help Centre Rooms\x02P^Miscellaneous\x02Q^Flower Power Puzzle\x02"
        );
    }

    #[test]
    fn nav_node_info_of_guest_rooms_matches_trace() {
        let parent = category(4, "Guest Rooms", false, 0);
        let subs: Vec<RoomCategory> = GUEST_CATEGORIES
            .iter()
            .map(|(id, n)| category(*id, n, false, 4))
            .collect();
        let subs: Vec<SubCategory> = subs
            .iter()
            .map(|category| SubCategory {
                category,
                current_visitors: 0,
                max_visitors: 0,
            })
            .collect();
        let msg = nav_node_info(&viewer(), &parent, &[], true, &subs, 0, 0).unwrap();
        assert_eq!(msg.header, 220);
        assert_eq!(
            msg.body,
            b"IPAJGuest Rooms\x02HHHHP\\HRestaurant, Bar & Night Club Rooms\x02HHPAQ\\HTrade floor\x02HHPAR\\HChill, Chat & Discussion Rooms\x02HHPAS\\HHair Salons & Modelling Rooms\x02HHPAP]HMaze & Theme Park Rooms\x02HHPAQ]HGaming & Race Rooms\x02HHPAR]HHelp Centre Rooms\x02HHPAP^HMiscellaneous\x02HHPAQ^HFlower Power Puzzle\x02HHPA"
        );
    }

    #[test]
    fn nav_node_info_of_public_rooms_matches_trace() {
        let parent = category(5, "Entertainment", true, 3);
        let room = RoomEntry {
            id: 2,
            name: "Theatredome".into(),
            description: "theatredrome".into(),
            category: 5,
            ccts: "hh_room_theater".into(),
            total_visitors_now: 0,
            total_visitors_max: 100,
            visitors_max: 100,
            ..Default::default()
        };
        let msg = nav_node_info(&viewer(), &parent, &[room], false, &[], 0, 465).unwrap();
        // Public rooms are addressed as id + 1000, and have no room count.
        assert_eq!(
            msg.body,
            b"HQAHEntertainment\x02HYtAKZzCITheatredome\x02HPYQAtheatredrome\x02JHhh_room_theater\x02HI".to_vec()
                .to_vec()
        );
    }

    #[test]
    fn owner_is_hidden_from_others() {
        let room = RoomEntry {
            id: 9,
            owner_id: 2,
            owner_name: "bob".into(),
            name: "R".into(),
            ..Default::default()
        };
        let msg = search_flat_results(std::slice::from_ref(&room), &viewer());
        // count 1, room 9, "R", owner "-", "open", 0 now, 0 max, no description.
        assert_eq!(msg.body, b"IQBR\x02-\x02open\x02HH\x02".as_slice());
        let own = Viewer {
            user_id: 2,
            ..viewer()
        };
        assert!(
            search_flat_results(&[room], &own)
                .body
                .windows(4)
                .any(|w| w == b"bob\x02")
        );
    }

    #[test]
    fn empty_results_match_trace() {
        assert_eq!(no_flats().body, b"");
        assert_eq!(no_flats_for_user("tracebot1").body, b"tracebot1\x02");
    }
}
