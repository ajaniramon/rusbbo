//! A user's presence in a room: position, path, statuses and timers (Java:
//! `game/room/entities/RoomEntity`, `RoomPlayer`, `RoomUserStatus`, `RoomTimerManager`,
//! `enums/StatusType`).
//!
//! It lives inside the `Room` (behind the room's lock), not on the player, so the room tick never
//! needs a player's lock.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::game::now;
use crate::game::pathfinder::Position;
use crate::game::room::trade::TradeState;
use crate::session::Session;

/// Java: `StatusType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // TODO(rooms): most are set by handlers not ported yet (carry, dance, trade...).
pub enum StatusType {
    Move,
    Sit,
    Lay,
    FlatControl,
    Swim,
    Talk,
    CarryItem,
    CarryDrink,
    UseItem,
    UseDrink,
    JoinGame,
    Trade,
    Dance,
    Dead,
    Jump,
    PetSleep,
    Eat,
    Smile,
    Play,
    Sign,
}

impl StatusType {
    pub fn code(self) -> &'static str {
        match self {
            Self::Move => "mv",
            Self::Sit => "sit",
            Self::Lay => "lay",
            Self::FlatControl => "flatctrl",
            Self::Swim => "swim",
            Self::Talk => "talk",
            Self::CarryItem => "cri",
            Self::CarryDrink => "carryd",
            Self::UseItem => "usei",
            Self::UseDrink => "drink",
            Self::JoinGame => "joingame",
            Self::Trade => "trd",
            Self::Dance => "dance",
            Self::Dead => "ded",
            Self::Jump => "jmp",
            Self::PetSleep => "slp",
            Self::Eat => "eat",
            Self::Smile => "sml",
            Self::Play => "pla",
            Self::Sign => "sign",
        }
    }
}

/// Java: `RoomUserStatus`. Timed statuses (drinks) swap `key` and `action` back and forth.
#[derive(Clone, Debug)]
pub struct RoomUserStatus {
    /// The map key it was stored under; Java removes by the current key, which can differ.
    map_key: &'static str,
    pub key: StatusType,
    pub value: String,
    pub action: Option<StatusType>,
    pub sec_action_switch: i32,
    pub sec_switch_lifetime: i32,
    pub lifetime_countdown: i32,
    pub action_countdown: i32,
    pub action_switch_countdown: i32,
}

impl RoomUserStatus {
    pub fn swap_key_action(&mut self) {
        if let Some(action) = self.action {
            self.action = Some(self.key);
            self.key = action;
        }
    }
}

/// Java's `String.hashCode`.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// The bin a key falls in in a default-sized `ConcurrentHashMap` (16 bins; statuses never reach
/// the resize threshold of 12).
fn bin(key: &str) -> i32 {
    let h = java_hash(key);
    ((h ^ ((h as u32) >> 16) as i32) & 0x7fff_ffff) & 15
}

/// The statuses of an entity (Java: `ConcurrentHashMap<String, RoomUserStatus>`). They are
/// listed in the map's iteration order, because `USER_STATUSES` writes them in that order.
#[derive(Clone, Debug, Default)]
pub struct StatusMap {
    /// Insertion order.
    entries: Vec<RoomUserStatus>,
}

impl StatusMap {
    pub fn contains(&self, status: StatusType) -> bool {
        self.entries.iter().any(|e| e.map_key == status.code())
    }

    /// Java: `getStatus(status).getValue()`.
    pub fn value(&self, status: StatusType) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.map_key == status.code())
            .map(|e| e.value.as_str())
    }

    pub fn remove(&mut self, status: StatusType) {
        self.remove_code(status.code());
    }

    fn remove_code(&mut self, code: &str) {
        self.entries.retain(|e| e.map_key != code);
    }

    /// Java: `setStatus(status, value)`.
    pub fn set(&mut self, status: StatusType, value: impl Into<String>) {
        self.set_timed(status, value, -1, None, -1, -1);
    }

    /// Java: `setStatus(status, value, secLifetime, action, secActionSwitch, secSwitchLifetime)`.
    pub fn set_timed(
        &mut self,
        status: StatusType,
        value: impl Into<String>,
        sec_lifetime: i32,
        action: Option<StatusType>,
        sec_action_switch: i32,
        sec_switch_lifetime: i32,
    ) {
        self.remove(status);
        self.entries.push(RoomUserStatus {
            map_key: status.code(),
            key: status,
            value: value.into(),
            action,
            sec_action_switch,
            sec_switch_lifetime,
            lifetime_countdown: sec_lifetime,
            action_countdown: sec_action_switch,
            action_switch_countdown: -1,
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The statuses in Java's iteration order: by bin, then insertion order within a bin.
    pub fn iter(&self) -> impl Iterator<Item = &RoomUserStatus> {
        let mut order: Vec<&RoomUserStatus> = self.entries.iter().collect();
        order.sort_by_key(|e| bin(e.map_key));
        order.into_iter()
    }

    /// Java: `StatusTask.processEntity`'s countdown loop. Returns whether anything changed and
    /// whether a carried drink ran out, which refreshes the user.
    pub fn tick(&mut self) -> (bool, bool) {
        let mut changed = false;
        let mut to_remove = Vec::new();
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by_key(|&i| bin(self.entries[i].map_key));
        for i in order {
            let rus = &mut self.entries[i];
            if rus.action_switch_countdown > 0 {
                rus.action_switch_countdown -= 1;
            } else if rus.action_switch_countdown == 0 {
                rus.action_switch_countdown = -1;
                rus.action_countdown = rus.sec_action_switch;
                rus.swap_key_action();
                changed = true;
            }
            if rus.action_countdown > 0 {
                rus.action_countdown -= 1;
            } else if rus.action_countdown == 0 {
                rus.action_countdown = -1;
                rus.action_switch_countdown = rus.sec_switch_lifetime;
                rus.swap_key_action();
                changed = true;
            }
            if rus.lifetime_countdown > 0 {
                rus.lifetime_countdown -= 1;
            } else if rus.lifetime_countdown == 0 {
                rus.lifetime_countdown = -1;
                to_remove.push(rus.key.code());
                changed = true;
            }
        }
        // Java removes by the current key, not the one the status was stored under.
        let mut refresh_user = false;
        for code in to_remove {
            self.remove_code(code);
            if code == StatusType::CarryDrink.code() {
                refresh_user = true;
            }
        }
        (changed, refresh_user)
    }
}

/// What a room shows of a user, copied from the player at entry (Java reads `PlayerDetails`).
#[derive(Clone, Debug, Default)]
pub struct EntityDetails {
    pub user_id: u32,
    pub name: String,
    pub motto: String,
    pub figure: String,
    pub sex: String,
    pub pool_figure: String,
    pub tickets: i32,
    /// Join date (Unix seconds).
    pub created_at: i64,
    /// For the fuserights the room checks (`Player.hasFuse`).
    pub rank: u8,
    pub club_expiration: i64,
    /// Java: `PlayerDetails.getMachineId`, for `Room.hasVoted`.
    pub machine_id: String,
    /// Java: `PlayerDetails.isTradeEnabled`. TODO(rcon): changes while the player is in a room.
    pub trade_enabled: bool,
    /// Java: `PlayerDetails.getTradeBanExpiration`.
    pub trade_ban_expiration: i64,
    /// Java: `PlayerDetails.isWordFilterEnabled`, for the chat a receiver gets.
    pub wordfilter_enabled: bool,
}

impl EntityDetails {
    /// Java: `Player.hasFuse`, from the rank and club membership copied at entry.
    pub fn has_fuse(&self, name: &str) -> bool {
        crate::game::fuserights::has_fuse(self.rank, self.club_expiration > now(), name)
    }
}

/// Java: `EntityType`: what is behind a room entity.
pub enum EntityKind {
    Player,
    /// An NPC of a public room (`rooms_bots`).
    Bot(Box<crate::game::bot::BotState>),
    /// A pet, while its nest is in the room (`items_pets`).
    Pet(Box<crate::game::pet::PetState>),
}

/// Java: `RoomEntity` + `RoomPlayer` + `RoomTimerManager`. Bots (and, later, pets) are entities
/// too: they have a detached `session` (see [`Session::detached`]) that nothing reads, so code that
/// sends to everyone in the room needs no special case, and `kind` says what they are.
pub struct RoomEntity {
    pub session: Arc<Session>,
    pub kind: EntityKind,
    pub details: EntityDetails,
    pub instance_id: i32,
    pub position: Position,
    pub goal: Option<Position>,
    pub next_position: Option<Position>,
    pub path: VecDeque<Position>,
    pub statuses: StatusMap,
    pub is_walking: bool,
    pub walking_allowed: bool,
    pub being_kicked: bool,
    pub needs_update: bool,
    pub enable_walking_on_stop: bool,
    /// Java: `rollingData`, while a roller moves the user (until `RollerCompleteTask`).
    pub rolling: Option<crate::game::item::roller::RollingData>,
    pub dance_id: i32,
    pub effect_id: i32,
    pub is_sleeping: bool,
    pub is_typing: bool,
    /// Virtual id of the item whose trigger last ran on this entity.
    pub last_item_interaction: Option<i32>,
    /// Recent messages, for the chat log's duplicate check.
    pub chat_messages: Vec<Vec<u8>>,
    pub chat_spam_count: i32,
    pub chat_spam_ticks: i32,
    pub mute_time: i64,
    pub look_timer: i64,
    pub afk_timer: i64,
    pub sleep_timer: i64,
    pub chat_bubble_timer: i64,
    /// Java: `RoomPlayer.walkHistory`, the last goals asked for.
    pub walk_history: Vec<Position>,
    /// Java: `RoomPlayer`'s trade fields.
    pub trade: TradeState,
}

impl EntityDetails {
    /// Java: `PlayerDetails.isTradeBanned`.
    pub fn is_trade_banned(&self) -> bool {
        self.trade_ban_expiration > 0 && self.trade_ban_expiration > now()
    }
}

impl RoomEntity {
    /// A fresh entity, as after Java's `RoomEntity.reset` + `RoomPlayer.reset` (the trade reset in
    /// there sets `needsUpdate`, which is why everyone's status goes out on the next tick).
    pub fn new(session: Arc<Session>, details: EntityDetails, instance_id: i32) -> Self {
        Self {
            session,
            kind: EntityKind::Player,
            details,
            instance_id,
            position: Position::default(),
            goal: None,
            next_position: None,
            path: VecDeque::new(),
            statuses: StatusMap::default(),
            is_walking: false,
            walking_allowed: true,
            being_kicked: false,
            needs_update: true,
            enable_walking_on_stop: false,
            rolling: None,
            dance_id: 0,
            effect_id: 0,
            is_sleeping: false,
            is_typing: false,
            last_item_interaction: None,
            chat_messages: Vec::new(),
            // TODO(rooms): Java keeps the spam counters on the player across rooms.
            chat_spam_count: 0,
            chat_spam_ticks: 16,
            mute_time: 0,
            look_timer: -1,
            afk_timer: 0,
            sleep_timer: 0,
            chat_bubble_timer: -1,
            trade: TradeState::default(),
            walk_history: Vec::new(),
        }
    }

    /// Java: `entity.getType() == EntityType.PLAYER`.
    pub fn is_player(&self) -> bool {
        matches!(self.kind, EntityKind::Player)
    }

    /// Java: `EntityType.getTypeId`, as `USER_OBJECTS` writes it.
    pub fn type_id(&self) -> i32 {
        match self.kind {
            EntityKind::Player => 1,
            EntityKind::Pet(_) => 2,
            EntityKind::Bot(_) => 3,
        }
    }

    pub fn bot(&self) -> Option<&crate::game::bot::BotState> {
        match &self.kind {
            EntityKind::Bot(b) => Some(b),
            _ => None,
        }
    }

    pub fn pet(&self) -> Option<&crate::game::pet::PetState> {
        match &self.kind {
            EntityKind::Pet(p) => Some(p),
            _ => None,
        }
    }

    pub fn pet_mut(&mut self) -> Option<&mut crate::game::pet::PetState> {
        match &mut self.kind {
            EntityKind::Pet(p) => Some(p),
            _ => None,
        }
    }

    pub fn bot_mut(&mut self) -> Option<&mut crate::game::bot::BotState> {
        match &mut self.kind {
            EntityKind::Bot(b) => Some(b),
            _ => None,
        }
    }

    pub fn is_dancing(&self) -> bool {
        self.dance_id > 0
    }

    pub fn is_using_effect(&self) -> bool {
        self.effect_id > 0
    }

    /// Java: `RoomTimerManager.resetRoomTimer()` (without the wake-up, see `Room::reset_room_timer`).
    pub fn reset_timers(&mut self, afk_seconds: i64, sleep_seconds: i64) {
        self.afk_timer = now() + afk_seconds;
        self.sleep_timer = now() + sleep_seconds;
    }

    /// Java: `RoomPlayer.handleSpamTicks`, once per entity tick.
    pub fn handle_spam_ticks(&mut self) {
        if self.chat_spam_ticks >= 0 {
            self.chat_spam_ticks -= 1;
            if self.chat_spam_ticks == -1 {
                self.chat_spam_count = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_string_hash() {
        assert_eq!(java_hash("mv"), 3497);
        assert_eq!(
            java_hash("flatctrl"),
            "flatctrl"
                .bytes()
                .fold(0i32, |h, c| { h.wrapping_mul(31).wrapping_add(c as i32) })
        );
    }

    #[test]
    fn statuses_iterate_like_concurrent_hash_map() {
        let mut map = StatusMap::default();
        map.set(StatusType::Move, "1,2,0.0");
        map.set(StatusType::Sit, "1.0");
        map.set(StatusType::FlatControl, "useradmin");
        let order: Vec<&str> = map.iter().map(|s| s.key.code()).collect();
        let mut expected = vec!["mv", "sit", "flatctrl"];
        expected.sort_by_key(|k| bin(k));
        assert_eq!(order, expected);
        map.remove(StatusType::Sit);
        assert!(!map.contains(StatusType::Sit));
        assert!(map.contains(StatusType::Move));
    }

    #[test]
    fn timed_status_expires() {
        let mut map = StatusMap::default();
        map.set_timed(StatusType::Sign, "3", 1, None, -1, -1);
        assert_eq!(map.tick(), (false, false));
        assert_eq!(map.tick(), (true, false));
        assert!(!map.contains(StatusType::Sign));
    }
}
