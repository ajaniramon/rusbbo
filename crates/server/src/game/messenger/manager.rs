//! Messenger actions that touch more than one player (Java: `Messenger`'s methods that reach
//! other players through `PlayerManager`, and `MessengerManager`).
//!
//! Locking: every function here takes at most one player lock at a time and releases it before
//! it looks at or sends to another player. Callers must not hold a player lock.

use std::sync::Arc;

use rusbbo_storage::{MessengerCategory, User};
use tracing::error;

use super::{FriendView, Messenger, MessengerUser};
use crate::messages::outgoing::messenger as out;
use crate::session::Session;
use crate::world::World;

/// Java: `MessengerManager.getMessengerData`: runs `f` on the live messenger of an online user,
/// or on a transient one loaded from the store. `None` when the user does not exist.
pub fn with_messenger<R>(
    world: &World,
    user_id: u32,
    f: impl FnOnce(&mut Messenger) -> R,
) -> Option<R> {
    if let Some(session) = world.player_by_id(user_id) {
        if let Some(m) = session.player.lock().messenger.as_mut() {
            return Some(f(m));
        }
    }
    let user = world.store.user(user_id).ok().flatten()?;
    let mut transient = Messenger::load(&world.store, &world.settings.read(), &user);
    Some(f(&mut transient))
}

/// The details of an online user, if logged in (Java: `PlayerManager.getPlayerById(..).getDetails()`).
pub fn online_details(world: &World, user_id: u32) -> Option<User> {
    let session = world.player_by_id(user_id)?;
    let p = session.player.lock();
    p.details.clone()
}

/// The room an online user is in (Java: `getPlayerById(id).getRoomUser().getRoom()`).
pub fn room_of(world: &World, user_id: u32) -> Option<crate::game::room::RoomRef> {
    let session = world.player_by_id(user_id)?;
    let room_id = session.player.lock().room_id;
    room_id.and_then(|id| world.loaded_room(id))
}

/// Java: `MessengerUser.canFollowFriend`: the friend is connected (whatever their online status
/// setting) and in a room that is not a game arena. A Flash viewer is also refused public rooms
/// without a navigator style (a room behind a walkway counts as the one it leads from).
pub fn can_follow_friend(world: &World, user_id: u32, viewer_flash: bool) -> bool {
    let Some(room) = room_of(world, user_id) else {
        return false;
    };
    if viewer_flash {
        let mut room_id = room.id;
        let redirect = crate::game::navigator::flash::follow_redirect(world, room.id);
        if redirect != 0 {
            room_id = redirect;
        }
        let public = room.lock().is_public();
        let styled = world
            .store
            .record::<rusbbo_storage::NavigatorStyle>(room_id)
            .ok()
            .flatten()
            .is_some();
        if public && !styled {
            return false;
        }
    }
    !room.model_name.starts_with("bb_") && room.model_name != "snowwar"
}

/// Java: `MessengerUser.serialise`: overlays the live data of an online friend and resolves the
/// online flag (`PlayerManager.isPlayerOnline`, which honours `online_status_visible`).
pub fn resolve(world: &World, friend: &MessengerUser, viewer_flash: bool) -> FriendView {
    let live = online_details(world, friend.user_id);
    let (figure, last_online, sex, motto) = match &live {
        Some(d) => (
            d.figure.clone(),
            d.last_online,
            d.sex.clone(),
            d.motto.clone(),
        ),
        None => (
            friend.figure.clone(),
            friend.last_online,
            friend.sex.clone(),
            friend.motto.clone(),
        ),
    };
    let online = live.as_ref().is_some_and(|d| d.online_status_visible);
    let can_follow = can_follow_friend(world, friend.user_id, viewer_flash);
    FriendView {
        user_id: friend.user_id,
        username: friend.username.clone(),
        male: sex.eq_ignore_ascii_case("m"),
        online,
        can_follow,
        figure: if online { figure } else { String::new() },
        category_id: friend.category_id,
        motto,
        last_online,
    }
}

/// Resolves a list of friends. Takes no lock of the caller's.
pub fn resolve_all(
    world: &World,
    friends: &[MessengerUser],
    viewer_flash: bool,
) -> Vec<FriendView> {
    friends
        .iter()
        .map(|f| resolve(world, f, viewer_flash))
        .collect()
}

/// Java: `Messenger.sendStatusUpdate`: tells the online friends that this user came online or went
/// offline, or entered or left a room. Called after login and logout, at the end of entering and
/// leaving a room, and on `G_USRS`, as in Java; with no lock held.
pub fn send_status_update(world: &World, session: &Arc<Session>) {
    let (me, friend_ids) = {
        let p = session.player.lock();
        let Some(m) = p.messenger.as_ref() else {
            return;
        };
        (
            m.user.user_id,
            m.friends.keys().copied().collect::<Vec<_>>(),
        )
    };

    // Java `getOnlineFriends`: friends who are online and also have us in their list.
    let mut online_friends = Vec::new();
    for id in friend_ids {
        let Some(friend) = world.player_by_id(id) else {
            continue;
        };
        {
            let mut p = friend.player.lock();
            let Some(m) = p.messenger.as_mut() else {
                continue;
            };
            let Some(me_as_friend) = m.friends.get(&me).cloned() else {
                continue;
            };
            m.queue_friend_update(me_as_friend);
        }
        online_friends.push(friend);
    }

    if !world
        .settings
        .read()
        .bool("messenger.enable.official.update.speed")
    {
        for friend in &online_friends {
            send_friends_update(world, friend);
        }
    }
}

/// Java: `new FRIENDS_UPDATE(player, messenger)`: drains the queued updates and sends them.
pub fn send_friends_update(world: &World, session: &Arc<Session>) {
    let (categories, updates) = {
        let mut p = session.player.lock();
        let Some(m) = p.messenger.as_mut() else {
            return;
        };
        (m.categories.clone(), m.drain_friend_updates())
    };
    let views = resolve_all(world, &updates, session.is_flash());
    session.send(out::friends_update(&categories, &views));
}

/// Java: `Messenger.addRequest` on the target's messenger.
pub fn add_request(world: &World, requester: &MessengerUser, target_id: u32) {
    if let Err(e) = world
        .store
        .messenger_new_request(requester.user_id, target_id)
    {
        error!("saving friend request: {e}");
    }
    let Some(target) = world.player_by_id(target_id) else {
        return;
    };
    {
        let mut p = target.player.lock();
        if let Some(m) = p.messenger.as_mut() {
            m.requests.insert(requester.user_id, requester.clone());
        }
    }
    target.send(out::friend_request(requester));
}

/// Java: `Messenger.addFriend` on the accepting player's messenger.
pub fn add_friend(world: &World, session: &Arc<Session>, new_buddy: &MessengerUser) {
    let (me_as_buddy, categories) = {
        let mut p = session.player.lock();
        let Some(m) = p.messenger.as_mut() else {
            return;
        };
        if m.has_friend(new_buddy.user_id) {
            return;
        }
        let me = m.user.user_id;
        let store = &world.store;
        let saved = store.messenger_make_friends(me, new_buddy.user_id);
        if let Err(e) = saved {
            error!("saving friendship: {e}");
        }
        m.requests.remove(&new_buddy.user_id);
        m.friends.insert(new_buddy.user_id, new_buddy.clone());
        (m.user.clone(), m.categories.clone())
    };

    // The accepting player gets the buddy as the database has it now.
    if let Ok(Some(details)) = world.store.user(new_buddy.user_id) {
        let fresh = MessengerUser::from_user(&details, 0);
        session.send(out::add_buddy(
            &categories,
            &resolve(world, &fresh, session.is_flash()),
        ));
    }

    // And the requester, if online, gets the accepting player.
    if let Some(friend) = world.player_by_id(new_buddy.user_id) {
        let friend_categories = {
            let mut p = friend.player.lock();
            p.messenger.as_mut().map(|m| {
                m.friends.insert(me_as_buddy.user_id, me_as_buddy.clone());
                m.categories.clone()
            })
        };
        if let Some(cats) = friend_categories {
            friend.send(out::add_buddy(
                &cats,
                &resolve(world, &me_as_buddy, friend.is_flash()),
            ));
        }
    }
}

/// Java: `Messenger.removeFriend`, on a live or transient messenger.
pub fn remove_friend(world: &World, owner: &mut Messenger, friend_id: u32) {
    owner.friends.remove(&friend_id);
    let owner_id = owner.user.user_id;
    if let Err(e) = world
        .store
        .messenger_remove_friend(owner_id, friend_id)
        .and_then(|_| world.store.messenger_remove_friend(friend_id, owner_id))
    {
        error!("removing friend: {e}");
    }
}

/// Categories of an online player, for composers that need them.
pub fn categories_of(session: &Session) -> Vec<MessengerCategory> {
    session
        .player
        .lock()
        .messenger
        .as_ref()
        .map(|m| m.categories.clone())
        .unwrap_or_default()
}
