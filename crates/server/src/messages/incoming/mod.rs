//! Client → server handlers, registered by id as in `MessageHandler.java`.

pub mod handshake;
pub mod user;

use super::Handler;

/// The handler for a message id, or `None` for ids nobody handles yet.
pub fn handler(header: u16) -> Option<Handler> {
    Some(match header {
        206 => handshake::init_crypto,
        204 | 415 => handshake::sso,
        813 => handshake::unique_id,
        1170 => handshake::version_check,
        7 => user::get_info,
        8 => user::get_credits,
        _ => return None,
    })
}
