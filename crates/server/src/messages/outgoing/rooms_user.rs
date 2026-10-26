//! Java: `messages/outgoing/rooms/user`.

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

use crate::game::format_decimal;
use crate::game::room::entity::{RoomEntity, StatusType};

/// Java: `USER_OBJECTS`. `model_name` is the room's model (pool rooms show the swimwear).
pub fn user_objects<'a>(
    entities: impl IntoIterator<Item = &'a RoomEntity>,
    model_name: &str,
) -> Message {
    let entities: Vec<&RoomEntity> = entities.into_iter().collect();
    let pool = model_name.starts_with("pool_") || model_name == "md_a";
    let mut body = ServerBody::new().int(entities.len() as i32);
    for e in entities {
        let mut motto = e.details.motto.clone();
        motto.push(' ');
        body = body
            .int(e.details.user_id as i32)
            .string(e.details.name.as_bytes())
            .string(motto.as_bytes())
            .string(e.details.figure.as_bytes())
            .int(e.instance_id)
            .int(e.position.x)
            .int(e.position.y)
            .string(format_decimal(e.position.z).as_bytes())
            .int(e.position.rotation())
            .int(1) // EntityType.PLAYER
            .string(e.details.sex.to_uppercase().as_bytes())
            // TODO(statistics): XP_EARNED_MONTH; -1 when it is 0.
            .int(-1)
            // TODO(groups): favourite group id and member rank.
            .int(-1)
            .int(-1);
        let pool_figure = if pool && !e.details.pool_figure.is_empty() {
            e.details.pool_figure.as_bytes()
        } else {
            b""
        };
        body = body.string(pool_figure);
    }
    Message::new(28, body.build())
}

/// Java: `USER_STATUSES`. `model_name` is the room's model (Java special-cases `picnic`).
pub fn user_statuses<'a>(
    entities: impl IntoIterator<Item = &'a RoomEntity>,
    model_name: &str,
) -> Message {
    let entities: Vec<&RoomEntity> = entities.into_iter().collect();
    let mut body = ServerBody::new().int(entities.len() as i32);
    for e in entities {
        body = body
            .int(e.instance_id)
            .int(e.position.x)
            .int(e.position.y)
            .string(format_decimal(e.position.z).as_bytes())
            .int(e.position.head_rotation)
            .int(e.position.body_rotation)
            .raw(b"/");
        for status in e.statuses.iter() {
            body = body.raw(status.key.code().as_bytes());
            if !status.value.is_empty() {
                body = body.raw(b" ");
                if status.key == StatusType::Sit
                    && model_name == "picnic"
                    && e.position.x == 8
                    && e.position.y == 24
                {
                    // TODO(flash): Flash clients get 4.0 here.
                    body = body.string(format_decimal(7.0).as_bytes());
                } else {
                    body = body.raw(status.value.as_bytes());
                }
            }
            body = body.raw(b"/");
        }
        body = body.raw(&[2]);
    }
    Message::new(34, body.build())
}

/// Java: `CHAT_MESSAGE.ChatMessageType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatType {
    Chat = 24,
    #[allow(dead_code)] // TODO(rooms): WHISPER.
    Whisper = 25,
    Shout = 26,
}

/// Java: `CHAT_MESSAGE`.
pub fn chat_message(kind: ChatType, instance_id: i32, message: &[u8], gesture: i32) -> Message {
    Message::new(
        kind as u16,
        ServerBody::new()
            .int(instance_id)
            .string(message)
            .int(gesture)
            .build(),
    )
}

/// Java: `TYPING_STATUS`.
pub fn typing_status(instance_id: i32, typing: bool) -> Message {
    Message::new(361, ServerBody::new().int(instance_id).bool(typing).build())
}

/// Java: `USER_DANCE`.
pub fn user_dance(instance_id: i32, dance_id: i32) -> Message {
    Message::new(
        480,
        ServerBody::new().int(instance_id).int(dance_id).build(),
    )
}

/// Java: `USER_WAVE`.
pub fn user_wave(instance_id: i32) -> Message {
    Message::new(481, ServerBody::new().int(instance_id).build())
}

/// Java: `USER_SLEEP`.
pub fn user_sleep(instance_id: i32, sleeping: bool) -> Message {
    Message::new(
        486,
        ServerBody::new().int(instance_id).bool(sleeping).build(),
    )
}

/// Java: `LOGOUT`: the instance id as text.
pub fn logout(instance_id: i32) -> Message {
    Message::new(
        29,
        ServerBody::new()
            .string(instance_id.to_string().as_bytes())
            .build(),
    )
}

/// Java: `HOTEL_VIEW`.
pub fn hotel_view() -> Message {
    Message::new(18, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::pathfinder::Position;
    use crate::game::room::entity::EntityDetails;
    use crate::session::Session;
    use std::sync::Arc;

    fn entity(instance_id: i32, x: i32, y: i32, rotation: i32) -> RoomEntity {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let details = EntityDetails {
            user_id: 1,
            name: "tracebot1".into(),
            motto: "trace bot one".into(),
            figure: "hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61".into(),
            sex: "M".into(),
            ..Default::default()
        };
        let mut e = RoomEntity::new(Arc::new(Session::new(0, tx)), details, instance_id);
        e.position = Position::new(x, y);
        e.position.set_rotation(rotation);
        e
    }

    #[test]
    fn statuses_match_public_room_trace() {
        let e = entity(0, 13, 6, 4);
        assert_eq!(
            user_statuses([&e], "ballroom").body,
            b"IHQCRA0.0\x02PAPA/\x02"
        );
        let mut e = entity(0, 13, 6, 5);
        e.statuses.set(StatusType::Move, "12,7,0.0");
        assert_eq!(
            user_statuses([&e], "ballroom").body,
            b"IHQCRA0.0\x02QAQA/mv 12,7,0.0/\x02"
        );
    }

    #[test]
    fn objects_match_public_room_trace() {
        let e = entity(0, 13, 6, 4);
        assert_eq!(
            user_objects([&e], "ballroom").body,
            b"IItracebot1\x02trace bot one \x02hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61\x02HQCRA0.0\x02PAIM\x02MMM\x02"
        );
    }

    #[test]
    fn small_bodies_match_traces() {
        assert_eq!(
            chat_message(ChatType::Chat, 0, b"hola desde el bot", 0).body,
            b"Hhola desde el bot\x02H"
        );
        assert_eq!(chat_message(ChatType::Shout, 0, b"HOLA", 0).header, 26);
        assert_eq!(typing_status(1, false).body, b"IH");
        assert_eq!(user_dance(0, 0).body, b"HH");
        assert_eq!(user_wave(0).body, b"H");
        assert_eq!(logout(0).body, b"0\x02");
    }
}
