//! Client → server handlers, registered by id as in `MessageHandler.java`.

pub mod badges;
pub mod catalogue;
pub mod club;
pub mod effects;
pub mod handshake;
pub mod infobus;
pub mod inventory;
pub mod jukebox;
pub mod messenger;
pub mod moderation;
pub mod navigator;
pub mod pets;
pub mod rooms;
pub mod rooms_dimmer;
pub mod rooms_idol;
pub mod rooms_items;
pub mod rooms_moderation;
pub mod rooms_pool;
pub mod rooms_settings;
pub mod rooms_user;
pub mod songs;
pub mod trade;
pub mod user;
pub mod wobble_squabble;

use super::Handler;

/// Client text as Java reads it (ISO-8859-1: one char per byte).
pub(crate) use havana_protocol::body::from_latin1 as latin1;

/// The handler for a message id, or `None` for ids nobody handles yet.
pub fn handler(header: u16) -> Option<Handler> {
    Some(match header {
        206 => handshake::init_crypto,
        2002 => handshake::generate_key,
        204 | 415 => handshake::sso,
        813 => handshake::unique_id,
        1170 => handshake::version_check,
        1817 => handshake::get_session_parameters,
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
        264 => navigator::recommended_rooms,
        42 => pets::approve_pet_name,
        128 => pets::get_pet_stat,
        151 => navigator::get_user_flat_cats,
        16 => navigator::own_rooms,
        17 => navigator::search_rooms,
        154 => navigator::get_space_node_users,
        // registerRoomPackets
        57 | 391 => rooms::try_flat,
        182 => rooms::get_interest,
        394 => rooms::get_floormap,
        59 => rooms::goto_flat,
        2 => rooms::room_directory,
        126 => rooms::get_room_ad,
        60 => rooms::g_hmap,
        62 => rooms::g_objs,
        61 => rooms::g_usrs,
        64 => rooms::g_stat,
        63 => rooms::g_items,
        66 => rooms::flat_prop_by_item,
        341 => rooms_dimmer::get_presets,
        342 => rooms_dimmer::set_preset,
        343 => rooms_dimmer::change_state,
        // registerItemPackets
        90 => rooms_items::place_stuff,
        73 => rooms_items::move_stuff,
        67 => rooms_items::add_strip_item,
        392 => rooms_items::use_furniture,
        393 => rooms_items::use_wall_item,
        232 => rooms_items::enter_oneway_door,
        83 => rooms_items::g_idata,
        84 => rooms_items::set_item_data,
        85 => rooms_items::remove_item,
        78 => rooms_items::present_open,
        183 => rooms_items::convert_furni_to_credits,
        76 => rooms_items::throw_dice,
        77 => rooms_items::dice_off,
        247 => rooms_items::spin_wheel_of_fortune,
        314 => rooms_items::set_random_state,
        410 => rooms_idol::start_performance,
        411 => rooms_idol::vote_performance,
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
        56 => rooms_user::whisper,
        79 => rooms_user::look_to,
        93 => rooms_user::dance,
        88 => rooms_user::stop,
        80 => rooms_user::carry_drink,
        87 => rooms_user::carry_item,
        115 => rooms_user::go_away,
        100 => catalogue::grpc,
        101 => catalogue::get_catalog_index,
        102 => catalogue::get_catalogue_page,
        65 | 404 => inventory::get_strip,
        // registerTradePackets
        71 => trade::trade_open,
        72 => trade::trade_add_item,
        70 => trade::trade_close,
        69 => trade::trade_accept,
        68 | 403 => trade::trade_unaccept,
        402 => trade::trade_confirm_accept,
        405 => trade::trade_remove_item,
        // registerSongPackets
        244 | 246 => songs::get_song_list,
        239 => songs::new_song,
        219 => songs::insert_sound_package,
        220 => songs::eject_sound_package,
        240 => songs::save_song_new,
        243 => songs::update_play_list,
        221 => songs::get_song_info,
        245 => songs::get_play_list,
        248 => songs::delete_song,
        241 => songs::edit_song,
        242 => songs::save_song_edit,
        254 => songs::burn_song,
        // registerJukeboxPackets
        258 => jukebox::get_jukebox_discs,
        259 => jukebox::get_user_song_discs,
        255 => jukebox::add_jukebox_disc,
        256 => jukebox::remove_jukebox_disc,
        257 => jukebox::jukebox_playlist_add,
        260 => jukebox::reset_jukebox,
        // registerModerationPackets
        200 => moderation::moderator_action,
        237 => moderation::request_cfh,
        86 => moderation::submit_cfh,
        48 => moderation::pick_call_for_help,
        199 => moderation::message_to_caller,
        198 => moderation::change_call_category,
        238 => moderation::delete_cry,
        323 => moderation::follow_cry_for_help,
        320 => rooms_moderation::ban_user,
        4 | 756 => handshake::try_login,
        // registerRoomPoolPackets
        116 => rooms_pool::swimsuit,
        108 => rooms_pool::close_uimakoppi,
        104 => rooms_pool::sign,
        105 => rooms_pool::btcks,
        106 => rooms_pool::dive,
        107 => rooms_pool::splash_position,
        // registerInfobus
        111 => infobus::change_world,
        112 => infobus::vote,
        113 => infobus::try_bus,
        // registerWobbleSquabblePackets
        114 => wobble_squabble::ptm,
        // registerEffects
        372 => effects::use_avatar_effect,
        373 => effects::activate_avatar_effect,
        _ => return None,
    })
}
