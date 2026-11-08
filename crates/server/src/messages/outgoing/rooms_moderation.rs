//! Java: `messages/outgoing/rooms/moderation` (what a user may do in a private room).

use rusbbo_protocol::Message;

/// Java: `YOUAROWNER`.
pub fn you_are_owner() -> Message {
    Message::new(47, Vec::new())
}

/// Java: `YOUARECONTROLLER`.
pub fn you_are_controller() -> Message {
    Message::new(42, Vec::new())
}

/// Java: `YOUNOTCONTROLLER`.
pub fn you_not_controller() -> Message {
    Message::new(43, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_match_flat_traces() {
        // traces/flat-create.json and flat-visit.json: empty bodies.
        assert_eq!(you_are_owner().header, 47);
        assert_eq!(you_are_controller().header, 42);
        assert_eq!(you_not_controller().header, 43);
        assert!(you_are_owner().body.is_empty());
    }
}
