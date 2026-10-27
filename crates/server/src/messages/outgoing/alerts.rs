//! Java: `messages/outgoing/alerts`.

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

/// Java: `ALERT`.
pub fn alert(message: &[u8]) -> Message {
    Message::new(139, ServerBody::new().string(message).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_is_a_string() {
        assert_eq!(alert(b"hi").body, b"hi\x02");
    }
}
