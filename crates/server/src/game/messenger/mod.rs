//! The console (Java: `game/messenger`): friends, friend requests and their limits.
//!
//! A [`Messenger`] lives in the [`Player`](crate::game::player::Player) of every logged-in user,
//! loaded at login like the Java `new Messenger(player)`. For offline users the actions that
//! Java runs on `MessengerManager.getMessengerData` build a transient one from the store.
//! Cross-player work is in [`manager`], which never holds two player locks at once.

pub mod java_map;
pub mod manager;

use rusbbo_storage::{MessengerCategory, Store, User};
use tracing::error;

use crate::game::now;
use crate::settings::Settings;

/// Java: `StringUtil.filterInput(input, filterNewline)` without the `normalise.input.strings`
/// setting (off by default) and the "∂∫åæ" special case.
/// TODO(parity): Unicode normalisation when `normalise.input.strings` is on.
pub fn filter_input(input: &str, filter_newline: bool) -> String {
    input
        .chars()
        .map(|c| match c {
            '\u{1}' | '\u{2}' | '\t' | '\n' | '\u{c}' => ' ',
            '\r' if filter_newline => ' ',
            c => c,
        })
        .collect()
}

/// Java: `MessengerUser`, the snapshot of a user that sits in a friends list or request list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessengerUser {
    pub user_id: u32,
    pub username: String,
    pub figure: String,
    /// Always `"M"` or `"F"`.
    pub sex: String,
    pub motto: String,
    pub last_online: i64,
    pub allow_stalking: bool,
    pub category_id: i32,
    pub online_status_visible: bool,
}

impl MessengerUser {
    /// Java: `new MessengerUser(PlayerDetails)` / the DAO constructors.
    pub fn from_user(user: &User, category_id: i32) -> Self {
        Self {
            user_id: user.id,
            username: filter_input(&user.username, true),
            figure: filter_input(&user.figure, true),
            sex: if user.sex.eq_ignore_ascii_case("f") {
                "F"
            } else {
                "M"
            }
            .to_string(),
            motto: filter_input(&user.motto, true),
            last_online: user.last_online,
            allow_stalking: user.allow_stalking,
            category_id,
            online_status_visible: user.online_status_visible,
        }
    }
}

/// A [`MessengerUser`] with the live data Java's `MessengerUser.serialise` overlays on it, ready
/// to be written by a composer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendView {
    pub user_id: u32,
    pub username: String,
    pub male: bool,
    pub online: bool,
    /// Java: `MessengerUser.canFollowFriend` ([`manager::can_follow_friend`]).
    pub can_follow: bool,
    /// Empty when the friend is offline.
    pub figure: String,
    pub category_id: i32,
    pub motto: String,
    pub last_online: i64,
}

/// Java: `MessengerErrorType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessengerErrorType {
    TargetFriendListFull,
    TargetDoesNotAccept,
    FriendRequestNotFound,
    FriendlistFull,
    ConcurrencyError,
}

impl MessengerErrorType {
    pub fn code(self) -> i32 {
        match self {
            Self::TargetFriendListFull => 2,
            Self::TargetDoesNotAccept => 3,
            Self::FriendRequestNotFound => 4,
            Self::FriendlistFull => 39,
            Self::ConcurrencyError => 42,
        }
    }
}

/// Java: `MessengerError`. `MessengerErrorReason` is never set by any Java caller, so it is not
/// ported; `BUDDYREMOVE_ERROR` (37) is defined but unused there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessengerError {
    pub causer: String,
    pub kind: MessengerErrorType,
}

impl MessengerError {
    pub fn new(kind: MessengerErrorType, causer: &str) -> Self {
        Self {
            causer: causer.to_string(),
            kind,
        }
    }
}

/// Java: `Messenger`.
#[derive(Debug)]
pub struct Messenger {
    /// This user as the others see them (Java: `Messenger.user`).
    pub user: MessengerUser,
    /// Java: `HashMap<Integer, MessengerUser>`s, iterated in Java's order.
    pub friends: java_map::JavaIntMap<MessengerUser>,
    pub requests: java_map::JavaIntMap<MessengerUser>,
    pub categories: Vec<MessengerCategory>,
    /// Friends whose status changed, drained into the next `FRIENDS_UPDATE`.
    friends_update: Vec<MessengerUser>,
    friends_limit: i64,
    allows_friend_requests: bool,
}

impl Messenger {
    /// Java: `new Messenger(PlayerDetails)`. Friends and requests are joined with `users` like the
    /// DAO's `INNER JOIN`, so rows of deleted users disappear.
    ///
    /// Java keeps `HashMap`s, filled in the rows' order: [`java_map::JavaIntMap`] iterates them
    /// as Java does.
    pub fn load(store: &Store, settings: &Settings, user: &User) -> Self {
        let friends = match store.messenger_friends(user.id) {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|(id, category)| {
                    let u = store.user(id).ok().flatten()?;
                    Some((id, MessengerUser::from_user(&u, category)))
                })
                .collect(),
            Err(e) => {
                error!(user = user.id, "loading friends: {e}");
                java_map::JavaIntMap::default()
            }
        };
        let requests = match store.messenger_requests(user.id) {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|id| {
                    let u = store.user(id).ok().flatten()?;
                    Some((id, MessengerUser::from_user(&u, 0)))
                })
                .collect(),
            Err(e) => {
                error!(user = user.id, "loading friend requests: {e}");
                java_map::JavaIntMap::default()
            }
        };
        let categories = store.messenger_categories(user.id).unwrap_or_else(|e| {
            error!(user = user.id, "loading messenger categories: {e}");
            Vec::new()
        });
        // Java: rank <= 1 gets the configured limit, higher ranks are unlimited.
        let friends_limit = if user.rank <= 1 {
            let has_club = user.club_expiration != 0 && now() < user.club_expiration;
            settings.int(if has_club {
                "messenger.max.friends.club"
            } else {
                "messenger.max.friends.nonclub"
            })
        } else {
            i64::from(i32::MAX)
        };
        Self {
            user: MessengerUser::from_user(user, 0),
            friends,
            requests,
            categories,
            friends_update: Vec::new(),
            friends_limit,
            allows_friend_requests: user.allow_friend_requests,
        }
    }

    pub fn has_friend(&self, user_id: u32) -> bool {
        self.friends.contains_key(&user_id)
    }

    pub fn has_request(&self, user_id: u32) -> bool {
        self.requests.contains_key(&user_id)
    }

    /// Java: `isFriendsLimitReached`.
    pub fn is_friends_limit_reached(&self) -> bool {
        self.friends.len() as i64 >= self.friends_limit
    }

    /// Java: `getFriendsLimit`, as the client gets it in `MESSENGER_INIT`.
    pub fn friends_limit(&self) -> i32 {
        self.friends_limit
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }

    pub fn allows_friend_requests(&self) -> bool {
        self.allows_friend_requests
    }

    /// Java: `queueFriendUpdate`: replaces any earlier mention of the same friend.
    pub fn queue_friend_update(&mut self, friend: MessengerUser) {
        self.friends_update.retain(|f| f.user_id != friend.user_id);
        self.friends_update.push(friend);
    }

    /// Java: `getFriendsUpdate().drainTo`.
    pub fn drain_friend_updates(&mut self) -> Vec<MessengerUser> {
        std::mem::take(&mut self.friends_update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::create(&dir.path().join("t.redb")).unwrap();
        (dir, store)
    }

    fn user(id: u32, name: &str) -> User {
        User {
            id,
            username: name.into(),
            sex: "f".into(),
            ..Default::default()
        }
    }

    fn settings() -> Settings {
        Settings::new(
            [
                ("messenger.max.friends.nonclub", "2"),
                ("messenger.max.friends.club", "5"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        )
    }

    #[test]
    fn load_applies_limits_and_skips_missing_users() {
        let (_dir, store) = temp_store();
        for id in 1..=3 {
            store.put_user(&user(id, &format!("u{id}"))).unwrap();
        }
        store.messenger_add_friend(1, 2).unwrap();
        store.messenger_add_friend(1, 3).unwrap();
        store.messenger_add_friend(1, 99).unwrap(); // no such user
        store.messenger_new_request(3, 1).unwrap();

        let me = store.user(1).unwrap().unwrap();
        let m = Messenger::load(&store, &settings(), &me);
        assert_eq!(m.friends.keys().copied().collect::<Vec<_>>(), vec![2, 3]);
        assert_eq!(m.requests.keys().copied().collect::<Vec<_>>(), vec![3]);
        assert_eq!(m.friends_limit(), 2);
        assert!(m.is_friends_limit_reached());
        assert_eq!(m.user.sex, "F");

        let club = User {
            club_expiration: now() + 1000,
            ..me.clone()
        };
        assert_eq!(
            Messenger::load(&store, &settings(), &club).friends_limit(),
            5
        );
        let staff = User { rank: 5, ..me };
        assert_eq!(
            Messenger::load(&store, &settings(), &staff).friends_limit(),
            i32::MAX
        );
    }

    #[test]
    fn queued_updates_replace_earlier_ones() {
        let (_dir, store) = temp_store();
        store.put_user(&user(1, "a")).unwrap();
        let mut m = Messenger::load(&store, &settings(), &user(1, "a"));
        let mut f = MessengerUser::from_user(&user(2, "b"), 0);
        m.queue_friend_update(f.clone());
        f.motto = "new".into();
        m.queue_friend_update(f);
        let drained = m.drain_friend_updates();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].motto, "new");
        assert!(m.drain_friend_updates().is_empty());
    }

    #[test]
    fn filter_input_blanks_control_characters() {
        assert_eq!(filter_input("a\u{2}b\tc\rd", true), "a b c d");
        assert_eq!(filter_input("a\rb", false), "a\rb");
    }
}
