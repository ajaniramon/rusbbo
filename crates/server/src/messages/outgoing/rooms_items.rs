//! Java: `messages/outgoing/rooms/items` (furniture placed, moved, removed and toggled).

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

use super::catalogue::latin1;
use super::rooms::{floor_item, wall_item};
use crate::game::room::items::{Behaviour, Item};

/// Java: `PLACE_FLOORITEM`.
pub fn place_flooritem(item: &Item) -> Message {
    Message::new(93, floor_item(ServerBody::new(), item).build())
}

/// Java: `MOVE_FLOORITEM`, also the answer to a `MOVESTUFF` that changes nothing or is invalid.
pub fn move_flooritem(item: &Item) -> Message {
    Message::new(95, floor_item(ServerBody::new(), item).build())
}

/// Java: `REMOVE_FLOORITEM`: the virtual id as text.
pub fn remove_flooritem(item: &Item) -> Message {
    Message::new(
        94,
        ServerBody::new()
            .string(item.virtual_id.to_string().as_bytes())
            .build(),
    )
}

/// Java: `PLACE_WALLITEM`.
pub fn place_wallitem(item: &Item) -> Message {
    Message::new(83, wall_item(ServerBody::new(), item).build())
}

/// Java: `REMOVE_WALLITEM`: the virtual id as text.
pub fn remove_wallitem(item: &Item) -> Message {
    Message::new(
        84,
        ServerBody::new()
            .string(item.virtual_id.to_string().as_bytes())
            .build(),
    )
}

/// Java: `STUFFDATAUPDATE`, whose header depends on the item: 85 with the whole wall item, 88
/// with a floor item's id and custom data.
pub fn stuff_data_update(item: &Item) -> Message {
    if item.has(Behaviour::WallItem) {
        Message::new(85, wall_item(ServerBody::new(), item).build())
    } else {
        Message::new(
            88,
            ServerBody::new()
                .string(item.virtual_id.to_string().as_bytes())
                .string(&latin1(&item.custom_data))
                .build(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::room::items::tests::{
        bench_armas, chair_plasto, flat_item, lamp_armas, torch,
    };
    use crate::messages::outgoing::rooms;

    /// Bodies from `traces/flat-furni.json` and `flat-visit.json`.
    #[test]
    fn furniture_bodies_match_traces() {
        let lamp = flat_item(2, lamp_armas(), (6, 5, 0));
        assert_eq!(place_flooritem(&lamp).body, b"JZRARAQAH0.0\x02H0\x02M");
        assert_eq!(place_flooritem(&lamp).header, 93);
        let chair = flat_item(1, chair_plasto(), (7, 3, 4));
        assert_eq!(place_flooritem(&chair).body, b"I[\\ASAKPA0.0\x02H0\x02M");
        let sofa = flat_item(3, bench_armas(), (9, 10, 4));
        assert_eq!(move_flooritem(&sofa).body, b"K[PAQBRBPA0.0\x02H0\x02M");
        assert_eq!(move_flooritem(&sofa).header, 95);
        assert_eq!(
            rooms::active_objects([&lamp]).body,
            b"IJZRARAQAH0.0\x02H0\x02M"
        );
        assert_eq!(remove_flooritem(&chair).body, b"1\x02");
        assert_eq!(remove_flooritem(&chair).header, 94);

        let mut torch = flat_item(4, torch(), (0, 0, 0));
        torch.flat.as_mut().unwrap().record.wall_position = ":w=6,1 l=8,32 r".into();
        assert_eq!(
            place_wallitem(&torch).body,
            b"4\x02[@A:w=6,1 l=8,32 r\x020\x02"
        );
        assert_eq!(place_wallitem(&torch).header, 83);
        assert_eq!(
            rooms::items([&torch]).body,
            b"I4\x02[@A:w=6,1 l=8,32 r\x020\x02"
        );
        assert_eq!(remove_wallitem(&torch).body, b"4\x02");
        assert_eq!(remove_wallitem(&torch).header, 84);

        let mut lamp_on = lamp.clone();
        lamp_on.custom_data = "1".into();
        let msg = stuff_data_update(&lamp_on);
        assert_eq!((msg.header, msg.body.as_slice()), (88, &b"2\x021\x02"[..]));
        torch.custom_data = "1".into();
        let msg = stuff_data_update(&torch);
        assert_eq!(
            (msg.header, msg.body.as_slice()),
            (85, &b"4\x02[@A:w=6,1 l=8,32 r\x021\x02"[..])
        );
    }
}
