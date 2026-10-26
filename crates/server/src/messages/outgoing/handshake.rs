//! Java: `messages/outgoing/handshake` and `HOTEL_LOGOUT`.

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

/// Java `MessageComposer` ids; `MessageComposer`s with no body still get the 0x01 terminator.
pub fn hello() -> Message {
    Message::new(0, vec![])
}

pub fn crypto_parameters(token: &str) -> Message {
    Message::new(
        277,
        ServerBody::new()
            .string(token.as_bytes())
            .bool(true)
            .build(),
    )
}

/// Java: `SECRET_KEY`. The body is the server's public Diffie-Hellman key in decimal, with no
/// string terminator, as `response.write(String)` writes it.
pub fn secret_key(public_key: &str) -> Message {
    Message::new(1, ServerBody::new().raw(public_key.as_bytes()).build())
}

/// Java: `UniqueIDMessageEvent`.
pub fn unique_id(machine_id: &str) -> Message {
    Message::new(439, ServerBody::new().string(machine_id.as_bytes()).build())
}

pub fn rights(fuserights: &[&str]) -> Message {
    let body = fuserights
        .iter()
        .fold(ServerBody::new(), |b, f| b.string(f.as_bytes()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_match_traces() {
        assert_eq!(crypto_parameters("123").body, b"123\x02I");
        assert_eq!(hotel_logout(LogoutReason::Disconnect).body, b"M");
        assert_eq!(
            rights(&["fuse_login", "fuse_trade"]).body,
            b"fuse_login\x02fuse_trade\x02"
        );
    }
}
