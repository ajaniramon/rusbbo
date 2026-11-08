//! Java: `messages/outgoing/user/badges`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;
use rusbbo_storage::Badge;

/// Java: `AVAILABLE_BADGES`: every badge, then the worn ones with their slots.
pub fn available_badges(badges: &[Badge], equipped: &[&Badge]) -> Message {
    let mut body = ServerBody::new().int(badges.len() as i32);
    for badge in badges {
        body = body.text(&badge.code);
    }
    body = body.int(equipped.len() as i32);
    for badge in equipped {
        body = body.int(badge.slot_id).text(&badge.code);
    }
    Message::new(229, body.build())
}

/// Java: `USERBADGE` for a Shockwave client: the user id is a string.
pub fn user_badge(user_id: u32, equipped: &[&Badge]) -> Message {
    let mut body = ServerBody::new()
        .text(&user_id.to_string())
        .int(equipped.len() as i32);
    for badge in equipped {
        body = body.int(badge.slot_id).text(&badge.code);
    }
    Message::new(228, body.build())
}

/// Java: `ACHIEVEMENT_NOTIFICATION`.
pub fn achievement_notification(
    badge_code: &str,
    badge_remove: Option<&str>,
    level: i32,
) -> Message {
    Message::new(
        437,
        ServerBody::new()
            .int(0)
            .int(level)
            .text(badge_code)
            .text(badge_remove.unwrap_or(""))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn badge(code: &str, equipped: bool, slot_id: i32) -> Badge {
        Badge {
            code: code.into(),
            equipped,
            slot_id,
        }
    }

    #[test]
    fn bodies_match_user_badges_traces() {
        let all = [
            badge("TB1", false, 0),
            badge("TB2", true, 3),
            badge("TB3", false, 0),
            badge("TB4", false, 0),
        ];
        let worn = [&all[1]];
        assert_eq!(
            available_badges(&all, &worn).body,
            b"PATB1\x02TB2\x02TB3\x02TB4\x02IKTB2\x02"
        );
        let (tb1, tb4) = (badge("TB1", true, 1), badge("TB4", true, 4));
        assert_eq!(
            user_badge(3, &[&tb1, &tb4]).body,
            b"3\x02JITB1\x02PATB4\x02"
        );
        assert_eq!(user_badge(3, &[]).body, b"3\x02H");
        assert_eq!(
            achievement_notification("ACH_TraderPass1", None, 1).body,
            b"HIACH_TraderPass1\x02\x02"
        );
    }
}
