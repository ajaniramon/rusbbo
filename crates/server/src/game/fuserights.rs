//! Permissions (Java: `game/fuserights/Fuseright.java`, `FuserightsManager`, `PlayerRank`).
//! The table order is the enum order, which is the order `RIGHTS` lists them in.

/// `PlayerRank` ids.
pub mod rank {
    pub const NORMAL: u8 = 1;
    pub const GUIDE: u8 = 2;
    pub const HOBBA: u8 = 3;
    pub const SUPERHOBBA: u8 = 4;
    pub const MODERATOR: u8 = 5;
    pub const COMMUNITY_MANAGER: u8 = 6;
    pub const ADMINISTRATOR: u8 = 8;
}

/// Who gets a fuseright: everyone from a rank up, or club members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grant {
    Rank(u8),
    Club,
}

use Grant::{Club, Rank};
use rank::*;

pub const FUSERIGHTS: &[(&str, Grant)] = &[
    ("default", Rank(NORMAL)),
    ("fuse_login", Rank(NORMAL)),
    ("fuse_trade", Rank(NORMAL)),
    ("fuse_buy_credits", Rank(NORMAL)),
    ("fuse_buy_credits_fuse_login", Rank(NORMAL)),
    ("fuse_room_queue_default", Rank(NORMAL)),
    ("fuse_mute", Rank(HOBBA)),
    ("fuse_kick", Rank(HOBBA)),
    ("fuse_receive_calls_for_help", Rank(HOBBA)),
    ("fuse_remove_photos", Rank(SUPERHOBBA)),
    ("fuse_remove_stickies", Rank(SUPERHOBBA)),
    ("fuse_mod", Rank(MODERATOR)),
    ("fuse_moderator_access", Rank(MODERATOR)),
    ("fuse_chat_log", Rank(MODERATOR)),
    ("fuse_room_alert", Rank(MODERATOR)),
    ("fuse_room_kick", Rank(MODERATOR)),
    ("fuse_ignore_room_owner", Rank(MODERATOR)),
    ("fuse_enter_full_rooms", Rank(MODERATOR)),
    ("fuse_enter_locked_rooms", Rank(MODERATOR)),
    ("fuse_see_all_roomowners", Rank(MODERATOR)),
    ("fuse_search_users", Rank(MODERATOR)),
    ("fuse_ban", Rank(MODERATOR)),
    ("fuse_see_chat_log_link", Rank(MODERATOR)),
    ("fuse_cancel_roomevent", Rank(MODERATOR)),
    ("fuse_administrator_access", Rank(ADMINISTRATOR)),
    ("fuse_any_room_controller", Rank(ADMINISTRATOR)),
    ("fuse_pick_up_any_furni", Rank(ADMINISTRATOR)),
    ("fuse_see_flat_ids", Rank(ADMINISTRATOR)),
    ("fuse_credits", Rank(ADMINISTRATOR)),
    ("fuse_debug_window", Rank(ADMINISTRATOR)),
    ("fuse_priority_access", Club),
    ("fuse_use_special_room_layouts", Club),
    ("fuse_use_club_outfits", Club),
    ("fuse_use_club_outfits_default", Club),
    ("fuse_use_club_badge", Club),
    ("fuse_use_club_dance", Club),
    ("fuse_habbo_chooser", Club),
    ("fuse_furni_chooser", Club),
    ("fuse_extended_buddylist", Club),
    ("fuse_room_queue_club", Club),
    ("housekeeping_intra", Rank(HOBBA)),
    ("housekeeping_kick", Rank(ADMINISTRATOR)),
    ("fuse_housekeeping_alert", Rank(SUPERHOBBA)),
    ("housekeeping_discussion", Rank(GUIDE)),
    ("housekeeping_discussion_admin", Rank(SUPERHOBBA)),
    ("housekeeping_admin", Rank(ADMINISTRATOR)),
    ("housekeeping_admin_credits", Rank(ADMINISTRATOR)),
    ("housekeeping_admin_user_data", Rank(ADMINISTRATOR)),
    ("housekeeping_admin_payments", Rank(ADMINISTRATOR)),
    ("housekeeping_campaign", Rank(COMMUNITY_MANAGER)),
    ("housekeeping_campaign_ads", Rank(COMMUNITY_MANAGER)),
    ("housekeeping_george", Rank(MODERATOR)),
    ("housekeeping_admin_catalog", Rank(ADMINISTRATOR)),
    ("housekeeping_hobba", Rank(GUIDE)),
    ("housekeeping_hobba_newbietools", Rank(GUIDE)),
    ("housekeeping_hobba_moderatortools", Rank(HOBBA)),
    ("housekeeping_hobba_hobbatools", Rank(HOBBA)),
    ("housekeeping_hobba_admintools", Rank(ADMINISTRATOR)),
    ("housekeeping_hobba_supertools", Rank(SUPERHOBBA)),
    ("housekeeping_ban", Rank(MODERATOR)),
    ("housekeeping_megaban", Rank(ADMINISTRATOR)),
    ("housekeeping_superban", Rank(ADMINISTRATOR)),
];

/// What `RIGHTS` sends (Java: `Player.getFuserights`): the rank's rights, then the club ones,
/// keeping only names that start with `fuse_`.
pub fn for_player(rank: u8, has_club: bool) -> Vec<&'static str> {
    let ranked = FUSERIGHTS
        .iter()
        .filter(|(_, g)| matches!(g, Rank(min) if rank >= *min));
    let club = FUSERIGHTS.iter().filter(|(_, g)| has_club && *g == Club);
    ranked
        .chain(club)
        .map(|(name, _)| *name)
        .filter(|name| name.starts_with("fuse_"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_user_matches_login_trace() {
        assert_eq!(
            for_player(rank::NORMAL, false),
            [
                "fuse_login",
                "fuse_trade",
                "fuse_buy_credits",
                "fuse_buy_credits_fuse_login",
                "fuse_room_queue_default"
            ]
        );
    }

    #[test]
    fn club_rights_come_after_rank_rights() {
        let rights = for_player(rank::HOBBA, true);
        assert_eq!(rights.last(), Some(&"fuse_room_queue_club"));
        assert!(rights.contains(&"fuse_kick"));
        assert!(!rights.contains(&"housekeeping_intra"));
    }
}
