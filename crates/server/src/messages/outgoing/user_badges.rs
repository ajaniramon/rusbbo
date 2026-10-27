//! Java: `messages/outgoing/user/badges`.

use havana_protocol::Message;
use havana_protocol::body::ServerBody;
use havana_storage::Badge;

/// Java: `AVAILABLE_BADGES`: every badge, then the worn ones with their slots.
pub fn available_badges(badges: &[Badge], equipped: &[&Badge]) -> Message {
    let mut body = ServerBody::new().int(badges.len() as i32);
    for badge in badges {
        body = body.string(badge.code.as_bytes());
    }
    body = body.int(equipped.len() as i32);
    for badge in equipped {
        body = body.int(badge.slot_id).string(badge.code.as_bytes());
    }
    Message::new(229, body.build())
}

/// Java: `USERBADGE` for a Shockwave client: the user id is a string.
pub fn user_badge(user_id: u32, equipped: &[&Badge]) -> Message {
    let mut body = ServerBody::new()
        .string(user_id.to_string().as_bytes())
        .int(equipped.len() as i32);
    for badge in equipped {
        body = body.int(badge.slot_id).string(badge.code.as_bytes());
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
            .string(badge_code.as_bytes())
            .string(badge_remove.unwrap_or("").as_bytes())
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
