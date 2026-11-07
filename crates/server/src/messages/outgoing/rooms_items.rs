//! Java: `messages/outgoing/rooms/items` (furniture placed, moved, removed and toggled).

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

use super::catalogue::latin1;
use super::rooms::{floor_item, wall_item};
use crate::game::format_decimal;
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
        ServerBody::new().text(&item.virtual_id.to_string()).build(),
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
        ServerBody::new().text(&item.virtual_id.to_string()).build(),
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
                .text(&item.virtual_id.to_string())
                .string(&latin1(&item.custom_data))
                .build(),
        )
    }
}

/// Java: `CHANGESTATUS`: a one-way door opening (1) or closing (0).
pub fn change_status(item_id: i32, status: i32) -> Message {
    Message::new(312, ServerBody::new().int(item_id).int(status).build())
}

/// Java: `IDATA`: a post-it's colour and text, or any other wall item's custom data.
pub fn idata(item: &Item, colour: &str, text: &str) -> Message {
    let body = ServerBody::new().text(&item.virtual_id.to_string());
    let body = if item.has(Behaviour::PostIt) {
        body.raw(&latin1(colour)).raw(b" ").string(&latin1(text))
    } else {
        body.string(&latin1(&format!(
            "{} {}",
            item.virtual_id, item.custom_data
        )))
    };
    Message::new(48, body.build())
}

/// One item or user moved by a roller (Java: `RollingData`): its id and its heights before and
/// after the roll.
pub struct Slide {
    pub id: i32,
    pub from_z: f64,
    pub to_z: f64,
}

/// Java: `SLIDEOBJECTBUNDLE`: what one roller moves, with the user as `entity` (its `to_z` is the
/// display height).
pub fn slide_object_bundle(roller: &Item, items: &[Slide], entity: Option<&Slide>) -> Message {
    let front = roller.position.square_in_front();
    let mut body = ServerBody::new()
        .int(roller.position.x)
        .int(roller.position.y)
        .int(front.x)
        .int(front.y)
        .int(items.len() as i32);
    for item in items {
        body = body
            .int(item.id)
            .text(&format_decimal(item.from_z))
            .text(&format_decimal(item.to_z));
    }
    body = body
        .int(roller.virtual_id)
        .int(if entity.is_some() { 2 } else { 0 });
    if let Some(e) = entity {
        body = body
            .int(e.id)
            .text(&format_decimal(e.from_z))
            .text(&format_decimal(e.to_z));
    }
    Message::new(230, body.build())
}

/// Java: `DICE_VALUE`: the dice's id and its value; -1 starts the rolling animation (and 0 too,
/// for a client that comes in while it rolls).
pub fn dice_value(item_id: i32, value: i32) -> Message {
    Message::new(90, ServerBody::new().int(item_id).int(value).build())
}

/// Java: `CANNOT_PLACE_STUFF_FROM_STRIP`: why an item cannot be placed (a per-room limit).
pub fn cannot_place_stuff_from_strip(reason: i32) -> Message {
    Message::new(516, ServerBody::new().int(reason).build())
}

/// Java: `JUDGE_GUI_STATUS`: an idol judge's panel: 0 closed, 1 waiting, 2 voting for the
/// performer `user_id`.
pub fn judge_gui_status(status: i32, user_id: i32) -> Message {
    let body = ServerBody::new().int(status);
    let body = if status == 2 { body.int(user_id) } else { body };
    Message::new(490, body.build())
}

/// Java: `OPEN_PERFORMER_GUI`: the discs an idol performer may sing (virtual id, song name).
pub fn open_performer_gui(discs: &[(i32, String)]) -> Message {
    let mut body = ServerBody::new().int(discs.len() as i32);
    for (id, name) in discs {
        body = body.int(*id).string(&latin1(name));
    }
    Message::new(491, body.build())
}

/// Java: the anonymous composer 492 of `IdolScoreboardInteractor.onEntityLeave`: closes the
/// performer's panel.
pub fn close_performer_gui() -> Message {
    Message::new(492, ServerBody::new().build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::room::items::tests::{
        bench_armas, chair_plasto, flat_item, lamp_armas, torch,
    };
    use crate::messages::outgoing::rooms;

    /// Bodies from the interactor traces (`flat-gates`, `flat-postit`, `flat-roller`,
    /// `flat-teleport`).
    #[test]
    fn interactor_bodies_match_traces() {
        use crate::game::room::items::tests::{door, post_it, queue_tile1};

        let msg = change_status(1, 1);
        assert_eq!((msg.header, msg.body.as_slice()), (312, &b"II"[..]));
        assert_eq!(change_status(1, 0).body, b"IH");

        let mut sticky = flat_item(2, post_it(), (0, 0, 0));
        sticky.flat.as_mut().unwrap().record.wall_position = ":w=6,1 l=12,40 r".into();
        sticky.custom_data = "FFFF33".into();
        let msg = idata(&sticky, "FFFF33", "");
        assert_eq!(
            (msg.header, msg.body.as_slice()),
            (48, &b"2\x02FFFF33 \x02"[..])
        );
        assert_eq!(
            idata(&sticky, "9CCEFF", "Remember the milk").body,
            b"2\x029CCEFF Remember the milk\x02"
        );
        sticky.custom_data = "9CCEFFRemember the milk".into();
        let msg = stuff_data_update(&sticky);
        assert_eq!(
            (msg.header, msg.body.as_slice()),
            (85, &b"2\x02QX:w=6,1 l=12,40 r\x029CCEFF\x02"[..])
        );
        assert_eq!(
            place_wallitem(&sticky).body,
            b"2\x02QX:w=6,1 l=12,40 r\x029CCEFF\x02"
        );

        let roller = flat_item(1, queue_tile1(), (6, 6, 4));
        let user = Slide {
            id: 0,
            from_z: 0.45,
            to_z: 0.0,
        };
        let msg = slide_object_bundle(&roller, &[], Some(&user));
        assert_eq!(
            (msg.header, msg.body.as_slice()),
            (230, &b"RARARASAHIJH0.45\x020.0\x02"[..])
        );
        let chair = Slide {
            id: 2,
            from_z: 0.45,
            to_z: 0.0,
        };
        assert_eq!(
            slide_object_bundle(&roller, &[chair], None).body,
            b"RARARASAIJ0.45\x020.0\x02IH"
        );
        assert_eq!(place_flooritem(&roller).body, b"IYtARARAPA0.0\x02J0\x02M");

        let mut teleporter = flat_item(2, door(), (5, 9, 2));
        assert_eq!(
            place_flooritem(&teleporter).body,
            b"J[EBQAQBJ0.0\x02H0\x02M"
        );
        teleporter.custom_data = "2".into();
        let msg = stuff_data_update(&teleporter);
        assert_eq!((msg.header, msg.body.as_slice()), (88, &b"2\x022\x02"[..]));
    }

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
