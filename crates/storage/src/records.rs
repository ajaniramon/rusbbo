//! Record types stored in the database. Changing a struct's fields means bumping
//! [`crate::SCHEMA_VERSION`] and adding a migration: postcard encodes fields by position.

use serde::{Deserialize, Serialize};

/// A row of the Java `users` table. Datetimes are Unix seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: u32,
    pub username: String,
    /// Password hash as written by the Java web (spring-security), kept verbatim.
    pub password: String,
    pub figure: String,
    pub pool_figure: String,
    pub sex: String,
    pub motto: String,
    pub email: String,
    pub credits: i32,
    pub pixels: i32,
    pub tickets: i32,
    pub film: i32,
    pub rank: u8,
    pub last_online: i64,
    pub remember_token: Option<String>,
    pub is_online: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub sso_ticket: Option<String>,
    pub machine_id: String,
    pub club_subscribed: i64,
    pub club_expiration: i64,
    pub club_gift_due: i64,
    pub allow_stalking: bool,
    pub allow_friend_requests: bool,
    pub online_status_visible: bool,
    pub profile_visible: bool,
    pub wordfilter_enabled: bool,
    pub trade_enabled: bool,
    pub trade_ban_expiration: i64,
    pub sound_enabled: bool,
    pub selected_room_id: i32,
    pub tutorial_finished: bool,
    pub daily_coins_enabled: bool,
    pub daily_respect_points: i32,
    pub respect_points: i32,
    /// `dd-MM-yyyy` of the last day respect points were reset, as the Java server stores it.
    pub respect_day: String,
    pub respect_given: i32,
    pub totem_effect_expiry: i64,
    pub favourite_group: i32,
    pub home_room: i32,
    pub has_flash_warning: bool,
}

impl User {
    /// A new `users` row with the columns' defaults (credits 50, rank 1, the privacy and sound
    /// switches on, 3 daily respect points, the Flash warning on).
    pub fn table_defaults() -> Self {
        User {
            sex: "M".into(),
            credits: 50,
            rank: 1,
            allow_stalking: true,
            allow_friend_requests: true,
            online_status_visible: true,
            profile_visible: true,
            wordfilter_enabled: true,
            sound_enabled: true,
            daily_respect_points: 3,
            has_flash_warning: true,
            ..Default::default()
        }
    }
}
