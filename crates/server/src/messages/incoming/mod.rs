//! Client → server handlers, registered by id as in `MessageHandler.java`.

pub mod handshake;
pub mod messenger;
pub mod user;

use super::Handler;

/// The handler for a message id, or `None` for ids nobody handles yet.
pub fn handler(header: u16) -> Option<Handler> {
    Some(match header {
        206 => handshake::init_crypto,
        2002 => handshake::generate_key,
        204 | 415 => handshake::sso,
        813 => handshake::unique_id,
        1170 => handshake::version_check,
        7 => user::get_info,
        12 => messenger::messenger_init,
        15 => messenger::friendlist_update,
        32 => messenger::mark_read,
        33 => messenger::send_message,
        37 => messenger::accept_buddy,
        38 => messenger::decline_buddy,
        39 => messenger::request_buddy,
        40 => messenger::remove_buddy,
        41 => messenger::find_user,
        233 => messenger::get_requests,
        8 => user::get_credits,
        _ => return None,
    })
}
