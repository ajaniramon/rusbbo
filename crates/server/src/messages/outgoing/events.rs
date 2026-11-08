//! Java: `messages/outgoing/events`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use super::catalogue::latin1;
use crate::game::events::Event;
use crate::game::long_date;

/// What a composer shows of an event: the event with its hoster's name and its start date
/// (Java: `getEventHoster().getName()`, `getStartedDate()`, read while composing).
pub struct EventView<'a> {
    pub event: &'a Event,
    pub hoster_name: String,
    /// Unix seconds: the expiry minus `events.expiry.minutes`.
    pub started: i64,
}

/// Java: `ROOMEEVENT_INFO` (Shockwave: no tags). `None` is the room without an event.
pub fn room_event_info(event: Option<&EventView>) -> Message {
    let Some(view) = event else {
        return Message::new(370, ServerBody::new().string(b"-1").build());
    };
    let e = view.event;
    let body = ServerBody::new()
        .text(&e.user_id.to_string())
        .string(&latin1(&view.hoster_name))
        .text(&e.room_id.to_string())
        .int(e.category_id)
        .string(&latin1(&e.name))
        .string(&latin1(&e.description))
        .text(&long_date(view.started));
    Message::new(370, body.build())
}

/// Java: `ROOMEVENT_LIST`.
pub fn room_event_list(type_id: i32, events: &[EventView]) -> Message {
    let mut body = ServerBody::new().int(type_id).int(events.len() as i32);
    for view in events {
        let e = view.event;
        body = body
            .text(&e.room_id.to_string())
            .string(&latin1(&view.hoster_name))
            .string(&latin1(&e.name))
            .string(&latin1(&e.description))
            .text(&long_date(view.started));
    }
    Message::new(369, body.build())
}

/// Java: `ROOMEVENT_PERMISSION`.
pub fn room_event_permission(can_create: bool) -> Message {
    Message::new(367, ServerBody::new().bool(can_create).build())
}

/// Java: `ROOMEVENT_TYPES`.
pub fn room_event_types(count: i32) -> Message {
    Message::new(368, ServerBody::new().int(count).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// traces/flat-events.json (the date is the clock's, and masked there).
    #[test]
    fn events_match_trace() {
        let event = Event {
            room_id: 1000,
            user_id: 1,
            category_id: 3,
            name: "bobba's party".into(),
            description: "Everyone welcome".into(),
            expire: 0,
            tags: vec![String::new()],
        };
        let view = EventView {
            event: &event,
            hoster_name: "tracebot1".into(),
            started: 1_794_162_087,
        };
        let date = long_date(view.started);
        let info = room_event_info(Some(&view));
        assert_eq!(info.header, 370);
        assert_eq!(
            info.body,
            format!("1\x02tracebot1\x021000\x02Kbobba's party\x02Everyone welcome\x02{date}\x02")
                .into_bytes()
        );
        let list = room_event_list(3, std::slice::from_ref(&view));
        assert_eq!(
            (list.header, list.body),
            (
                369,
                format!("KI1000\x02tracebot1\x02bobba's party\x02Everyone welcome\x02{date}\x02")
                    .into_bytes()
            )
        );
        assert_eq!(room_event_list(5, &[]).body, b"QAH");
        assert_eq!(room_event_permission(true).body, b"I");
        assert_eq!(room_event_types(11).body, b"SB");
    }
}
