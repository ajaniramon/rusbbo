//! Java: `messages/outgoing/rooms/user`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

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
            .text(&e.details.name)
            .text(&motto)
            .text(&e.details.figure)
            .int(e.instance_id)
            .int(e.position.x)
            .int(e.position.y)
            .text(&format_decimal(e.position.z))
            .int(e.position.rotation())
            .int(e.type_id());
        if !e.is_player() {
            continue;
        }
        body = body
            .text(&e.details.sex.to_uppercase())
            // Java: XP_EARNED_MONTH, -1 when it is 0.
            .int(if e.details.xp_month == 0 {
                -1
            } else {
                e.details.xp_month
            })
            // TODO(groups): favourite group id and member rank.
            .int(-1)
            .int(-1);
        let pool_figure = if pool && !e.details.pool_figure.is_empty() {
            e.details.pool_figure.as_str()
        } else {
            ""
        };
        body = body.text(pool_figure);
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
            .text(&format_decimal(e.position.z))
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
                    body = body.text(&format_decimal(7.0));
                } else {
                    body = body.raw(&rusbbo_protocol::body::latin1(&status.value));
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
    Message::new(29, ServerBody::new().text(&instance_id.to_string()).build())
}

/// Java: `HOTEL_VIEW`.
pub fn hotel_view() -> Message {
    Message::new(18, vec![])
}

/// Java: `TAG_LIST`: a user's tags.
pub fn tag_list(user_id: u32, tags: &[&str]) -> Message {
    let body = tags.iter().fold(
        ServerBody::new().int(user_id as i32).int(tags.len() as i32),
        |b, tag| b.text(tag),
    );
    Message::new(350, body.build())
}

/// Java: `GROUP_MEMBERSHIP_UPDATE`: a user's favourite group and their rank in it (-1 for none).
pub fn group_membership_update(instance_id: i32, group_id: i32, rank_id: i32) -> Message {
    Message::new(
        310,
        ServerBody::new()
            .int(instance_id)
            .int(group_id)
            .int(rank_id)
            .build(),
    )
}

/// Java: `FIGURE_CHANGE`: a user's look and motto changed (also after `:givebadge` and a
/// `:teleport` warp).
pub fn figure_change(instance_id: i32, figure: &str, sex: &str, motto: &str) -> Message {
    Message::new(
        266,
        ServerBody::new()
            .int(instance_id)
            .text(figure)
            .text(sex)
            .text(motto)
            .build(),
    )
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
    fn social_composers_match_traces() {
        // traces/social-actions.json, chat-commands.json, social-dance-carry.json
        let whisper = chat_message(ChatType::Whisper, 0, b"psst, over here", 0);
        assert_eq!(
            (whisper.header, whisper.body),
            (25, b"Hpsst, over here\x02H".to_vec())
        );
        assert_eq!(
            chat_message(ChatType::Chat, 0, b":shutdown now", 4).body,
            b"H:shutdown now\x02PA"
        );
        assert_eq!(user_dance(0, 3).body, b"HK");
        assert_eq!(user_sleep(1, true).body, b"II");
        assert_eq!(user_sleep(1, false).body, b"IH");
        assert_eq!(typing_status(0, false).body, b"HH");
    }

    #[test]
    fn carrying_status_matches_trace() {
        let mut e = entity(0, 12, 8, 4);
        e.statuses.set_timed(
            StatusType::CarryDrink,
            "5",
            300,
            Some(StatusType::UseItem),
            12,
            1,
        );
        assert_eq!(
            user_statuses([&e], "ballroom").body,
            b"IHPCPB0.0\x02PAPA/carryd 5/\x02"
        );
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
    fn statuses_match_flat_traces() {
        // traces/flat-create.json: the owner in the door of model_a, then walking.
        let mut e = entity(0, 3, 5, 2);
        e.statuses.set(StatusType::FlatControl, "useradmin");
        assert_eq!(
            user_statuses([&e], "model_a").body,
            b"IHKQA0.0\x02JJ/flatctrl useradmin/\x02"
        );
        e.position.set_rotation(3);
        e.statuses.set(StatusType::Move, "4,6,0.0");
        assert_eq!(
            user_statuses([&e], "model_a").body,
            b"IHKQA0.0\x02KK/flatctrl useradmin/mv 4,6,0.0/\x02"
        );
        // traces/flat-visit.json: rights given to a visitor (empty flatctrl value).
        let mut e = entity(1, 8, 8, 3);
        e.statuses.set(StatusType::FlatControl, "");
        assert_eq!(
            user_statuses([&e], "model_a").body,
            b"IIPBPB0.0\x02KK/flatctrl/\x02"
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
