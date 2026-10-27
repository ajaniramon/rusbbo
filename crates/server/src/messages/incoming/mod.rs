//! Client → server handlers, registered by id as in `MessageHandler.java`.

pub mod badges;
pub mod catalogue;
pub mod club;
pub mod handshake;
pub mod inventory;
pub mod messenger;
pub mod navigator;
pub mod rooms;
pub mod rooms_moderation;
pub mod rooms_settings;
pub mod rooms_user;
pub mod user;

use super::Handler;

/// Client text as Java reads it (ISO-8859-1: one char per byte).
pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// The handler for a message id, or `None` for ids nobody handles yet.
pub fn handler(header: u16) -> Option<Handler> {
    Some(match header {
        206 => handshake::init_crypto,
        2002 => handshake::generate_key,
        204 | 415 => handshake::sso,
        813 => handshake::unique_id,
        1170 => handshake::version_check,
        7 => user::get_info,
        228 => user::get_account_preferences,
        196 => user::pong,
        370 => user::get_possible_achievements,
        360 => user::get_ignore_list,
        319 => user::ignore_user,
        322 => user::unignore_user,
        26 => club::get_club,
        190 => club::subscribe_club,
        157 => badges::get_available_badges,
        158 => badges::set_badge,
        159 => badges::get_selected_badges,
        229 => rooms_user::set_sound_setting,
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
        // registerNavigatorPackets
        150 => navigator::navigate,
        151 => navigator::get_user_flat_cats,
        16 => navigator::own_rooms,
        17 => navigator::search_rooms,
        154 => navigator::get_space_node_users,
        // registerRoomPackets
        57 | 391 => rooms::try_flat,
        59 => rooms::goto_flat,
        2 => rooms::room_directory,
        126 => rooms::get_room_ad,
        60 => rooms::g_hmap,
        62 => rooms::g_objs,
        61 => rooms::g_usrs,
        64 => rooms::g_stat,
        63 => rooms::g_items,
        98 => rooms_moderation::let_user_in,
        // registerRoomSettingsPackets
        21 => rooms_settings::get_flat_info,
        29 => rooms_settings::create_flat,
        25 => rooms_settings::set_flat_info,
        24 => rooms_settings::update_flat,
        153 => rooms_settings::set_flat_cat,
        152 => rooms_settings::get_flat_cat,
        23 => rooms_settings::delete_flat,
        // registerRoomModerationPackets
        95 => rooms_moderation::kick,
        96 => rooms_moderation::assign_rights,
        97 => rooms_moderation::remove_rights,
        155 => rooms_moderation::remove_all_rights,
        // registerRoomUserPackets
        53 => rooms_user::quit,
        75 => rooms_user::walk,
        52 => rooms_user::chat,
        55 => rooms_user::shout,
        317 => rooms_user::start_typing,
        318 => rooms_user::cancel_typing,
        94 => rooms_user::wave,
        100 => catalogue::grpc,
        101 => catalogue::get_catalog_index,
        102 => catalogue::get_catalogue_page,
        65 | 404 => inventory::get_strip,
        _ => return None,
    })
}
