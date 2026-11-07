//! Java: `messages/outgoing/alerts`.

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

use super::catalogue::latin1;

/// Java: `ALERT`: a message box.
pub fn alert(message: &str) -> Message {
    Message::new(139, ServerBody::new().string(&latin1(message)).build())
}

/// Java: `LOCALISED_ERROR(errorCode)`.
pub fn localised_error(error_code: i32) -> Message {
    Message::new(33, ServerBody::new().int(error_code).build())
}

/// Java: `LOCALISED_ERROR(externalTextEntry)`: the text, raw.
pub fn localised_error_text(text: &str) -> Message {
    Message::new(33, ServerBody::new().raw(&latin1(text)).build())
}

/// Java: `NO_USER_FOUND`: the name, raw.
pub fn no_user_found(username: &str) -> Message {
    Message::new(76, ServerBody::new().raw(&latin1(username)).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_match_traces() {
        // traces/flat-visit.json: wrong room password.
        assert_eq!(localised_error(-100002).body, b"fhFF");
        assert_eq!(alert("hi").body, b"hi\x02");
        // traces/flat-present.json: a gift for an unknown user.
        let msg = no_user_found("nobody123");
        assert_eq!((msg.header, msg.body.as_slice()), (76, &b"nobody123"[..]));
    }
}
