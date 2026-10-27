//! Client → server handlers, registered by id as in `MessageHandler.java`.

pub mod handshake;
pub mod navigator;
pub mod rooms;
pub mod rooms_user;
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
        8 => user::get_credits,
        // registerNavigatorPackets
        150 => navigator::navigate,
        151 => navigator::get_user_flat_cats,
        16 => navigator::own_rooms,
        17 => navigator::search_rooms,
        154 => navigator::get_space_node_users,
        // registerRoomPackets
        2 => rooms::room_directory,
        60 => rooms::g_hmap,
        62 => rooms::g_objs,
        61 => rooms::g_usrs,
        64 => rooms::g_stat,
        // registerRoomUserPackets
        53 => rooms_user::quit,
        75 => rooms_user::walk,
        52 => rooms_user::chat,
        55 => rooms_user::shout,
        317 => rooms_user::start_typing,
        318 => rooms_user::cancel_typing,
        94 => rooms_user::wave,
        _ => return None,
    })
}
