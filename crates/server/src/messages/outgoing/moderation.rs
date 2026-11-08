//! Java: `messages/outgoing/moderation`: the call for help (CRY) and moderator alerts.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

/// Java: `MODERATOR_ALERT`.
pub fn moderator_alert(message: &str) -> Message {
    Message::new(161, ServerBody::new().text(message).text("").build())
}

/// The room part of a call, as Java reads it from the caller's room when composing.
pub struct CallRoom {
    pub name: String,
    pub is_public: bool,
    pub ccts: String,
    pub id: u32,
    pub owner_name: String,
}

/// What `CALL_FOR_HELP` and `CFH_ACK` show of a call.
pub struct CallView<'a> {
    pub cry_id: i32,
    pub category: i32,
    /// Java: `getFormattedRequestTime`.
    pub time: &'a str,
    pub caller_id: u32,
    pub message: &'a str,
}

/// Java: `CALL_FOR_HELP`, to every moderator.
pub fn call_for_help(cfh: &CallView<'_>, room: &CallRoom) -> Message {
    let caller = cfh.caller_id.to_string();
    let body = ServerBody::new()
        .text(&cfh.cry_id.to_string())
        .int(cfh.category)
        .text(cfh.time)
        .text(&caller)
        .text(cfh.message)
        .text(&caller)
        .text(&room.name);
    let body = if room.is_public {
        body.int(0)
            .text(&room.ccts)
            .int(room.id as i32 + crate::game::navigator::PUBLIC_ROOM_OFFSET)
            .int(room.id as i32)
    } else {
        body.int(1)
            .text(&room.name)
            .int(room.id as i32)
            .text(&room.owner_name)
    };
    Message::new(148, body.build())
}

/// Java: `PICKED_CRY`.
pub fn picked_cry(cry_id: i32, picked_up_by: u32) -> Message {
    Message::new(
        149,
        ServerBody::new()
            .text(&cry_id.to_string())
            .text(&picked_up_by.to_string())
            .build(),
    )
}

/// Java: `DELETE_CRY`.
pub fn delete_cry(cry_id: i32) -> Message {
    Message::new(273, ServerBody::new().text(&cry_id.to_string()).build())
}

/// Java: `CRY_REPLY`: a moderator's answer to the caller.
pub fn cry_reply(message: &str) -> Message {
    Message::new(274, ServerBody::new().text(message).build())
}

/// Java: `CFH_ACK`: the caller's open call, if any.
pub fn cfh_ack(call: Option<&CallView<'_>>) -> Message {
    let body = ServerBody::new().bool(call.is_some());
    let body = match call {
        Some(c) => body
            .text(&c.cry_id.to_string())
            .text(c.time)
            .text(c.message),
        None => body,
    };
    Message::new(319, body.build())
}

/// Java: `CRY_RECEIVED`.
pub fn cry_received() -> Message {
    Message::new(321, ServerBody::new().text("H").build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_bodies() {
        assert_eq!(moderator_alert("hi").body, b"hi\x02\x02");
        assert_eq!(cry_received().body, b"H\x02");
        assert_eq!(delete_cry(3).body, b"3\x02");
        assert_eq!(picked_cry(0, 21).body, b"0\x0221\x02");
        assert_eq!(cfh_ack(None).body, b"H");
    }

    #[test]
    fn call_bodies() {
        let call = CallView {
            cry_id: 0,
            category: 2,
            time: "22:15 7/11/2026",
            caller_id: 1,
            message: "Someone is being rude",
        };
        let public = CallRoom {
            name: "The Ballroom".into(),
            is_public: true,
            ccts: "hh_room_ballroom".into(),
            id: 7,
            owner_name: String::new(),
        };
        // Java CALL_FOR_HELP: ids as text, the category as an int, then the public room part:
        // 0, the CCTs, the id with the public offset, the id.
        let expected = ServerBody::new()
            .text("0")
            .int(2)
            .text("22:15 7/11/2026")
            .text("1")
            .text("Someone is being rude")
            .text("1")
            .text("The Ballroom")
            .int(0)
            .text("hh_room_ballroom")
            .int(7 + crate::game::navigator::PUBLIC_ROOM_OFFSET)
            .int(7)
            .build();
        assert_eq!(call_for_help(&call, &public).body, expected);
        assert!(expected.starts_with(b"0\x02J22:15 7/11/2026\x021\x02"));
        let flat = CallRoom {
            name: "bobba place".into(),
            is_public: false,
            ccts: String::new(),
            id: 1000,
            owner_name: "tracebot1".into(),
        };
        let body = call_for_help(&call, &flat).body;
        let tail = ServerBody::new()
            .text("bobba place")
            .int(1)
            .text("bobba place")
            .int(1000)
            .text("tracebot1")
            .build();
        assert!(body.ends_with(&tail));
        assert_eq!(
            cfh_ack(Some(&call)).body,
            b"I0\x0222:15 7/11/2026\x02Someone is being rude\x02"
        );
    }
}
