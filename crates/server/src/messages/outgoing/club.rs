//! Java: `messages/outgoing/club`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

/// Java: `CLUB_INFO`.
pub fn club_info(
    remaining_days_this_month: i32,
    since_months: i32,
    prepaid_months: i32,
) -> Message {
    Message::new(
        7,
        ServerBody::new()
            .string(b"club_habbo")
            .int(remaining_days_this_month)
            .int(since_months)
            .int(prepaid_months)
            .int(1) // 2 would open the Habbo Club dialogue
            .build(),
    )
}

/// Java: `CLUB_GIFT`.
pub fn club_gift(gift_count: i32) -> Message {
    Message::new(280, ServerBody::new().int(gift_count).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_match_user_club_trace() {
        assert_eq!(club_info(0, 0, 0).body, b"club_habbo\x02HHHI");
        assert_eq!(club_info(31, 0, 0).body, b"club_habbo\x02SGHHI");
        assert_eq!(club_info(31, 0, 9).body, b"club_habbo\x02SGHQBI");
        assert_eq!(club_gift(0).body, b"H");
    }
}
