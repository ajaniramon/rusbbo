//! Java: `messages/outgoing/rooms/pool`, `rooms/items/SHOWPROGRAM` and the ticket messages of
//! `user/currencies`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::{ServerBody, latin1};

/// Java: `SHOWPROGRAM`: the arguments joined by spaces, raw (the pool's curtains, splashes, lift
/// and diving camera).
pub fn show_program(arguments: &[&str]) -> Message {
    Message::new(71, latin1(&arguments.join(" ")))
}

/// Java: `OPEN_UIMAKOPPI`: the changing booth's clothes window.
pub fn open_uimakoppi() -> Message {
    Message::new(96, Vec::new())
}

/// Java: `JUMPINGPLACE_OK`: the user is on the diving board.
pub fn jumpingplace_ok() -> Message {
    Message::new(125, Vec::new())
}

/// Java: `JUMPDATA`: the diver's instance id, `\r`, and the dive the client sent, raw.
pub fn jumpdata(instance_id: i32, diving_handle: &[u8]) -> Message {
    let mut body = format!("{instance_id}\r").into_bytes();
    body.extend_from_slice(diving_handle);
    Message::new(74, body)
}

/// Java: `NO_TICKETS`.
pub fn no_tickets() -> Message {
    Message::new(73, Vec::new())
}

/// Java: `TICKET_BALANCE`.
pub fn ticket_balance(tickets: i32) -> Message {
    Message::new(124, ServerBody::new().int(tickets).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        assert_eq!(
            show_program(&["curtains1", "close"]).body,
            b"curtains1 close"
        );
        assert_eq!(show_program(&["Splash0"]).body, b"Splash0");
        assert_eq!(jumpdata(3, b"abc").body, b"3\rabc");
        assert_eq!(ticket_balance(12).body, b"PC");
    }
}
