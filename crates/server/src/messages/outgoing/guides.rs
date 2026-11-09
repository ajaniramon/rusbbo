//! Java: `messages/outgoing/guides` and `messages/outgoing/tutorial`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

/// Java: `INVITATION`: a beginner asks this guide for help.
pub fn invitation(user_id: u32, username: &str) -> Message {
    Message::new(
        355,
        ServerBody::new()
            .text(&user_id.to_string())
            .text(username)
            .build(),
    )
}

/// Java: `TUTORS_AVAILABLE`.
pub fn tutors_available(count: i32) -> Message {
    Message::new(356, ServerBody::new().int(count).build())
}

/// Java: `INVITING_COMPLETED`: whether a guide came.
pub fn inviting_completed(success: bool) -> Message {
    Message::new(357, ServerBody::new().bool(success).build())
}

/// Java: `INVITE_FOLLOW_FAILED`.
pub fn invite_follow_failed() -> Message {
    Message::new(359, Vec::new())
}

/// Java: `INVITE_CANCELLED`.
pub fn invite_cancelled() -> Message {
    Message::new(360, Vec::new())
}

/// Java: `INVITATION_SENT`.
pub fn invitation_sent() -> Message {
    Message::new(421, Vec::new())
}

/// Java: `GUIDE_FOUND`.
pub fn guide_found(user_id: u32) -> Message {
    Message::new(423, ServerBody::new().int(user_id as i32).build())
}

/// Java: `INIT_TUTOR_SERVICE_STATUS`.
pub fn init_tutor_service_status(status: i32) -> Message {
    Message::new(425, ServerBody::new().int(status).build())
}

/// Java: `ENABLE_TUTOR_SERVICE_STATUS`: 2 friend list full, 3 service disabled, 4 too many
/// beginners.
pub fn enable_tutor_service_status(status: i32) -> Message {
    Message::new(426, ServerBody::new().int(status).build())
}
