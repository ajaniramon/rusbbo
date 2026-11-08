//! Java: `messages/outgoing/handshake` and `HOTEL_LOGOUT`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

/// Java `MessageComposer` ids; `MessageComposer`s with no body still get the 0x01 terminator.
pub fn hello() -> Message {
    Message::new(0, vec![])
}

pub fn crypto_parameters(token: &str) -> Message {
    Message::new(277, ServerBody::new().text(token).bool(true).build())
}

/// Java: `SECRET_KEY`. The body is the server's public Diffie-Hellman key in decimal, with no
/// string terminator, as `response.write(String)` writes it.
pub fn secret_key(public_key: &str) -> Message {
    Message::new(1, ServerBody::new().raw(public_key.as_bytes()).build())
}

/// Java: `UniqueIDMessageEvent`.
pub fn unique_id(machine_id: &str) -> Message {
    Message::new(439, ServerBody::new().text(machine_id).build())
}

pub fn rights(fuserights: &[&str]) -> Message {
    let body = fuserights.iter().fold(ServerBody::new(), |b, f| b.text(f));
    Message::new(2, body.build())
}

pub fn login() -> Message {
    Message::new(3, vec![])
}

#[derive(Clone, Copy)]
pub enum LogoutReason {
    Disconnect = -1,
    #[allow(dead_code)]
    LoggedOut = 1,
    #[allow(dead_code)]
    Concurrent = 2,
    #[allow(dead_code)]
    Timeout = 3,
}

pub fn hotel_logout(reason: LogoutReason) -> Message {
    Message::new(287, ServerBody::new().int(reason as i32).build())
}

/// Java: `SESSION_PARAMETERS` (`SessionParamType` id, then the value: an int when it starts with a
/// digit, otherwise a string). Java fills a `HashMap` keyed by the enum, whose iteration order
/// follows identity hash codes and changes between JVM runs; the client reads the pairs by id, so
/// the order does not matter and this sends them by id.
pub fn session_parameters(vouchers: bool, profile_editing: bool, tutorial: bool) -> Message {
    enum Value {
        Int(i32),
        Text(&'static str),
    }
    use Value::{Int, Text};
    let params = [
        (1, Int(vouchers as i32)),        // VOUCHER_ENABLED
        (2, Int(0)),                      // REGISTER_REQUIRE_PARENT_EMAIL
        (3, Int(0)),                      // REGISTER_SEND_PARENT_EMAIL
        (4, Int(0)),                      // ALLOW_DIRECT_MAIL
        (5, Text("yyyy-MM-dd")),          // DATE_FORMAT
        (6, Int(0)),                      // PARTNER_INTEGRATION_ENABLED
        (7, Int(profile_editing as i32)), // ALLOW_PROFILE_EDITING
        (8, Text("")),                    // TRACKING_HEADER
        (9, Int(tutorial as i32)),        // TUTORIAL_ENABLED
    ];
    let mut body = ServerBody::new().int(params.len() as i32);
    for (id, value) in params {
        body = body.int(id);
        body = match value {
            Int(v) => body.int(v),
            Text(s) => body.text(s),
        };
    }
    Message::new(257, body.build())
}

/// Java: `INFO_HOTEL_CLOSING`: the hotel closes in that many minutes (the maintenance shutdown).
pub fn info_hotel_closing(minutes: i32) -> Message {
    Message::new(291, ServerBody::new().int(minutes).build())
}

/// Java: `INFO_HOTEL_CLOSED`: the hotel closes (sent to everyone on shutdown, with the time it
/// closed and `disconnect = false`, by `PlayerManager.dispose`).
pub fn info_hotel_closed(hour: u32, minute: u32, disconnect: bool) -> Message {
    Message::new(
        292,
        ServerBody::new()
            .int(hour as i32)
            .int(minute as i32)
            .bool(disconnect)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_match_traces() {
        assert_eq!(crypto_parameters("123").body, b"123\x02I");
        assert_eq!(hotel_logout(LogoutReason::Disconnect).body, b"M");
        // 9 pairs; VL64: H = 0, I = 1, J = 2 ... (vouchers on, profile editing off, tutorial on)
        assert_eq!(
            session_parameters(true, false, true).body,
            b"QBIIJHKHPAHQAyyyy-MM-dd\x02RAHSAHPB\x02QBI"
        );
        assert_eq!(
            rights(&["fuse_login", "fuse_trade"]).body,
            b"fuse_login\x02fuse_trade\x02"
        );
    }
}
