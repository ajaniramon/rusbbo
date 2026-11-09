//! Java: `messages/incoming/messenger`. `MESSENGER_GETMESSAGES` (191) is not ported: Java leaves
//! it unregistered.

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;
use tracing::error;

use crate::game::messenger::manager::{
    add_friend, add_request, can_follow_friend, categories_of, online_details, remove_friend,
    resolve_all, room_of, send_friends_update, with_messenger,
};
use crate::game::messenger::{MessengerError, MessengerErrorType, filter_input};
use crate::game::{format_local, now};
use crate::messages::Malformed;
use crate::messages::outgoing::messenger::{self as out, SearchEntry};
use crate::session::Session;
use crate::world::World;

/// The user whose console must stay hidden (Java hard-codes this name in several places).
const HIDDEN_USER: &str = "Abigail.Ryan";

use super::latin1;

/// A client-sent user id; negative ids cannot match anybody.
fn user_id(v: i32) -> Option<u32> {
    u32::try_from(v).ok()
}

/// The id of the logged-in user of this session, if its messenger is loaded.
fn me(session: &Session) -> Option<u32> {
    let p = session.player.lock();
    p.messenger
        .as_ref()
        .filter(|_| p.logged_in)
        .map(|m| m.user.user_id)
}

/// Java: `MESSENGERINIT`.
pub fn messenger_init(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let (my_id, limit, categories, friends) = {
        let p = session.player.lock();
        let Some(m) = p.messenger.as_ref().filter(|_| p.logged_in) else {
            return Ok(());
        };
        (
            m.user.user_id,
            m.friends_limit(),
            m.categories.clone(),
            m.friends.values().cloned().collect::<Vec<_>>(),
        )
    };
    let (normal, club) = {
        let s = world.settings.read();
        (
            s.int("messenger.max.friends.nonclub") as i32,
            s.int("messenger.max.friends.club") as i32,
        )
    };
    let views = resolve_all(world, &friends, session.is_flash());
    session.send(out::messenger_init(
        limit,
        normal,
        club,
        &categories,
        &views,
    ));

    // Messages friends left while this player was offline, marked read once delivered.
    let unread = world.store.messenger_unread(my_id).unwrap_or_else(|e| {
        error!(user = my_id, "loading offline messages: {e}");
        Vec::new()
    });
    for message in &unread {
        let when = format_local(message.date, "%d/%m %H:%M");
        session.send(out::messenger_msg(
            message.sender_id,
            &format!("[{when}] {}", message.body),
        ));
    }
    let ids: Vec<u32> = unread.iter().map(|m| m.id).collect();
    if let Err(e) = world.store.messenger_mark_all_read(my_id, &ids) {
        error!(user = my_id, "marking messages read: {e}");
    }
    Ok(())
}

/// Java: `FINDUSER`.
pub fn find_user(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let query = latin1(reader.string().ok_or(Malformed)?).to_lowercase();
    let Some(my_id) = me(session) else {
        return Ok(());
    };
    let ids = world.store.messenger_search(&query).unwrap_or_else(|e| {
        error!("user search: {e}");
        Vec::new()
    });
    let mut friends = Vec::new();
    let mut others = Vec::new();
    for id in ids {
        if id == my_id {
            continue;
        }
        let is_friend = {
            let p = session.player.lock();
            p.messenger.as_ref().is_some_and(|m| m.has_friend(id))
        };
        let live = online_details(world, id);
        let Some(details) = live.clone().or_else(|| world.store.user(id).ok().flatten()) else {
            continue;
        };
        if !is_friend && details.username == HIDDEN_USER {
            continue;
        }
        // Java: `isOnline && player.getRoomUser().getRoom() != null`, then the room's name.
        let online = live.as_ref().is_some_and(|d| d.online_status_visible);
        let room_name = online
            .then(|| room_of(world, id))
            .flatten()
            .map(|room| room.lock().data.name.clone());
        let entry = SearchEntry {
            user_id: id,
            username: details.username.clone(),
            motto: details.motto.clone(),
            online,
            room_name,
            male: details.sex.eq_ignore_ascii_case("m"),
            figure: details.figure.clone(),
            last_online: details.last_online,
        };
        if is_friend {
            friends.push(entry);
        } else {
            others.push(entry);
        }
    }
    session.send(out::messenger_search(&friends, &others));
    Ok(())
}

/// Java: `MESSENGER_REQUESTBUDDY`.
pub fn request_buddy(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let username = latin1(reader.string().ok_or(Malformed)?);
    let Some(my_id) = me(session) else {
        return Ok(());
    };
    let target = world
        .store
        .user_by_name(&username)
        .ok()
        .flatten()
        .filter(|_| !username.eq_ignore_ascii_case(HIDDEN_USER));
    let Some(target) = target else {
        // External texts: "There was an error finding the user for the friend request".
        session.send(out::messenger_error(&MessengerError::new(
            MessengerErrorType::FriendRequestNotFound,
            "",
        )));
        return Ok(());
    };

    let (callee, limit_reached, my_name) = {
        let p = session.player.lock();
        let (Some(m), Some(d)) = (p.messenger.as_ref(), p.details.as_ref()) else {
            return Ok(());
        };
        (
            m.user.clone(),
            m.is_friends_limit_reached(),
            d.username.clone(),
        )
    };
    // Java compares the lowercased input with the name as stored, so this only matches names
    // that are all lowercase; kept as is.
    if username.to_lowercase() == my_name {
        return Ok(());
    }
    let send_error = |kind| {
        session.send(out::messenger_error(&MessengerError::new(kind, "")));
    };
    if limit_reached {
        send_error(MessengerErrorType::FriendlistFull);
        return Ok(());
    }
    let Some((has_friend, has_request, target_full, allows)) =
        with_messenger(world, target.id, |m| {
            (
                m.has_friend(my_id),
                m.has_request(my_id),
                m.is_friends_limit_reached(),
                m.allows_friend_requests(),
            )
        })
    else {
        return Ok(());
    };
    if has_friend || has_request {
        return Ok(());
    }
    if target_full {
        send_error(MessengerErrorType::TargetFriendListFull);
        return Ok(());
    }
    if !allows {
        send_error(MessengerErrorType::TargetDoesNotAccept);
        return Ok(());
    }
    add_request(world, &callee, target.id);
    Ok(())
}

/// Java: `MESSENGER_GETREQUESTS`.
pub fn get_requests(
    _: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let p = session.player.lock();
    if let Some(m) = p.messenger.as_ref() {
        session.send(out::friend_requests(m.requests.values()));
    }
    Ok(())
}

/// Java: `MESSENGER_ACCEPTBUDDY`.
pub fn accept_buddy(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut errors = Vec::new();
    let amount = reader.int().ok_or(Malformed)?;
    for _ in 0..amount {
        let id = reader.int().ok_or(Malformed)?;
        let request = user_id(id).and_then(|id| {
            let p = session.player.lock();
            p.messenger
                .as_ref()
                .and_then(|m| m.requests.get(&id).cloned())
        });
        let Some(new_buddy) = request else {
            errors.push(MessengerError::new(
                MessengerErrorType::FriendRequestNotFound,
                "",
            ));
            continue;
        };
        let Some(buddy_full) =
            with_messenger(world, new_buddy.user_id, |m| m.is_friends_limit_reached())
        else {
            error!(
                user = new_buddy.user_id,
                "friend request from a missing user"
            );
            continue;
        };
        let my_limit_reached = {
            let p = session.player.lock();
            p.messenger
                .as_ref()
                .is_some_and(|m| m.is_friends_limit_reached())
        };
        if my_limit_reached {
            errors.push(MessengerError::new(
                MessengerErrorType::FriendlistFull,
                &new_buddy.username,
            ));
            continue;
        }
        if buddy_full {
            errors.push(MessengerError::new(
                MessengerErrorType::TargetFriendListFull,
                &new_buddy.username,
            ));
            continue;
        }
        // Java has the `allow_friend_requests` check of the requester commented out here.
        add_friend(world, session, &new_buddy);
    }
    session.send(out::buddy_request_result(&errors));
    Ok(())
}

/// Java: `MESSENGER_DECLINEBUDDY`.
pub fn decline_buddy(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let decline_all = reader.bool().ok_or(Malformed)?;
    let Some(my_id) = me(session) else {
        return Ok(());
    };
    if decline_all {
        if let Err(e) = world.store.messenger_remove_all_requests(my_id) {
            error!("declining requests: {e}");
        }
        if let Some(m) = session.player.lock().messenger.as_mut() {
            m.requests.clear();
        }
        return Ok(());
    }
    let amount = reader.int().ok_or(Malformed)?;
    for _ in 0..amount {
        let id = reader.int().ok_or(Malformed)?;
        let Some(id) = user_id(id) else { continue };
        let has_request = session
            .player
            .lock()
            .messenger
            .as_ref()
            .is_some_and(|m| m.has_request(id));
        if !has_request {
            continue;
        }
        if let Err(e) = world.store.messenger_remove_request(id, my_id) {
            error!("declining request: {e}");
        }
        if let Some(m) = session.player.lock().messenger.as_mut() {
            m.requests.remove(&id);
        }
    }
    Ok(())
}

/// Java: `MESSENGER_REMOVEBUDDY`.
pub fn remove_buddy(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let size = reader.int().ok_or(Malformed)?;
    let Some(my_id) = me(session) else {
        return Ok(());
    };
    for _ in 0..size {
        let friend_id = user_id(reader.int().ok_or(Malformed)?);
        let removed = friend_id.and_then(|fid| {
            let mut p = session.player.lock();
            let m = p.messenger.as_mut()?;
            m.has_friend(fid).then_some(())?;
            remove_friend(world, m, fid);
            Some((fid, m.categories.clone()))
        });
        let Some((fid, my_categories)) = removed else {
            // The client asked to remove somebody who is not a friend.
            session.send(out::messenger_error(&MessengerError::new(
                MessengerErrorType::ConcurrencyError,
                "",
            )));
            return Ok(());
        };
        // Remove ourselves from the friend's list, online or not.
        with_messenger(world, fid, |m| remove_friend(world, m, my_id));
        if let Some(friend) = world.player_by_id(fid) {
            friend.send(out::remove_buddy(&categories_of(&friend), my_id));
        }
        session.send(out::remove_buddy(&my_categories, fid));
    }
    Ok(())
}

/// Java: `MESSENGER_SENDMSG`: the mandatory word filter for everyone, the automatic ban for a
/// bannable sentence, the muted alert, and the receiver's own word filter. The stored message is
/// the original text.
pub fn send_message(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let to = reader.int().ok_or(Malformed)?;
    let original_bytes = reader.string().ok_or(Malformed)?;
    let original = latin1(original_bytes);
    let message = {
        let settings = world.settings.read();
        let filtered = filter_input(&original, false);
        latin1(
            &world
                .wordfilter
                .filter_mandatory(&settings, &rusbbo_protocol::body::latin1(&filtered)),
        )
    };
    if message.trim().is_empty() {
        return Ok(());
    }
    let Some(my_id) = me(session) else {
        return Ok(());
    };
    let created_at = session
        .player
        .lock()
        .details
        .as_ref()
        .map_or(0, |d| d.created_at);
    if super::rooms_user::is_bannable(world, created_at, original_bytes) {
        crate::game::moderation::perform_wordfilter_ban(world, my_id);
        return Ok(());
    }
    if crate::game::moderation::check_muted(session) {
        return Ok(());
    }
    let friend_id = user_id(to).filter(|id| {
        session
            .player
            .lock()
            .messenger
            .as_ref()
            .is_some_and(|m| m.has_friend(*id))
    });
    let Some(friend_id) = friend_id else {
        session.send(out::instant_message_error(6, to));
        return Ok(());
    };
    let message_id = match world
        .store
        .messenger_new_message(my_id, friend_id, &original, now())
    {
        Ok(id) => id,
        Err(e) => {
            error!("saving message: {e}");
            return Ok(());
        }
    };
    // Offline: the message stays unread and is delivered at the receiver's next MESSENGERINIT.
    let Some(friend) = world.player_by_id(friend_id) else {
        return Ok(());
    };
    let filter = friend
        .player
        .lock()
        .details
        .as_ref()
        .is_some_and(|d| d.wordfilter_enabled);
    let message = if filter {
        let settings = world.settings.read();
        latin1(
            &world
                .wordfilter
                .filter_sentence(&settings, &rusbbo_protocol::body::latin1(&message)),
        )
    } else {
        message
    };
    friend.send(out::messenger_msg(my_id, &message));
    if let Err(e) = world.store.messenger_mark_read(message_id) {
        error!("marking message read: {e}");
    }
    Ok(())
}

/// Java: `MESSENGER_MARKREAD`.
pub fn mark_read(
    world: &World,
    _: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let id = reader.int().ok_or(Malformed)?;
    if let Some(id) = user_id(id) {
        if let Err(e) = world.store.messenger_mark_read(id) {
            error!("marking message read: {e}");
        }
    }
    Ok(())
}

/// Java: `FRIENDLIST_UPDATE`.
pub fn friendlist_update(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    send_friends_update(world, session);
    Ok(())
}

fn has_friend(session: &Session, id: Option<u32>) -> bool {
    let Some(id) = id else {
        return false;
    };
    let p = session.player.lock();
    p.messenger.as_ref().is_some_and(|m| m.has_friend(id))
}

/// Java: `FOLLOW_FRIEND`: `FOLLOW_ERROR` (0 not a friend, 1 offline, 2 in no room or in a game,
/// 3 does not allow following), or `ROOMFORWARD` to the friend's room, remembered so that the
/// navigator can forward again (`NAVIGATE`'s `wasFollow`).
pub fn follow_friend(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let friend_id = user_id(reader.int().ok_or(Malformed)?);
    if me(session).is_none() {
        return Ok(());
    }
    if !has_friend(session, friend_id) {
        session.send(out::follow_error(0));
        return Ok(());
    }
    let Some(friend) = friend_id.and_then(|id| world.player_by_id(id)) else {
        session.send(out::follow_error(1));
        return Ok(());
    };
    let friend_id = friend_id.unwrap_or_default();
    let room = room_of(world, friend_id);
    let Some(room) = room.filter(|_| can_follow_friend(world, friend_id, session.is_flash()))
    else {
        session.send(out::follow_error(2));
        return Ok(());
    };
    let allows_stalking = friend
        .player
        .lock()
        .details
        .as_ref()
        .is_some_and(|d| d.allow_stalking);
    if !allows_stalking {
        session.send(out::follow_error(3));
        return Ok(());
    }
    session.player.lock().followed_room = Some(room.clone());
    crate::game::moderation::forward(world, session, &room);
    Ok(())
}

/// Java: `INVITE_FRIEND`: from a room, invites friends to it. A user who is not a friend gets
/// `INVITATION_ERROR` and ends the list (the ids after it are not read, as in Java); an offline
/// friend gets one too and is skipped.
pub fn invite_friend(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(my_id) = me(session) else {
        return Ok(());
    };
    if session.player.lock().room_id.is_none() {
        return Ok(());
    }
    let users = reader.int().ok_or(Malformed)?;
    let mut friends = Vec::new();
    for _ in 0..users.max(0) {
        let id = user_id(reader.int().ok_or(Malformed)?);
        if !has_friend(session, id) {
            session.send(out::invitation_error());
            break;
        }
        match id.and_then(|id| world.player_by_id(id)) {
            Some(friend) => friends.push(friend),
            None => session.send(out::invitation_error()),
        }
    }
    let message = filter_input(&latin1(reader.string().ok_or(Malformed)?), false);
    for friend in friends {
        friend.send(out::instant_message_invitation(my_id, &message));
    }
    Ok(())
}
