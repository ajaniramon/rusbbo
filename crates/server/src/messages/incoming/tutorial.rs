//! Java: `messages/incoming/tutorial`: the tutorial and the guide invitations.

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use super::latin1;
use crate::game::achievements::{self, AchievementType};
use crate::game::guides::{self, MAX_SIMULTANEOUS_GUIDING};
use crate::game::now;
use crate::game::room::items::is_numeric;
use crate::game::statistics::stat;
use crate::messages::Malformed;
use crate::messages::outgoing::alerts::alert;
use crate::messages::outgoing::guides as out;
use crate::session::Session;
use crate::world::World;

const IS_GUIDABLE: &str = "is_guidable";

/// Java: `MSG_GET_TUTORS_AVAILABLE`: a beginner opens the guide request.
pub fn get_tutors_available(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if p.statistics.int_value(IS_GUIDABLE) != 1 || !p.guides.can_use_tutorial {
        guides::try_clear_tutorial(world, &mut p);
        return Ok(());
    }
    p.guides.is_guidable = true;
    p.guides.block_tutorial = true;
    achievements::try_progress(world, session, &mut p, AchievementType::Graduate);
    session.send(out::tutors_available(1));
    Ok(())
}

/// Java: `MSG_INVITE_TUTORS`: the beginner waits for a guide (`guide.search.timeout.minutes`).
pub fn invite_tutors(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if !p.guides.is_guidable || p.guides.waiting_for_guide {
        return Ok(());
    }
    session.send(out::invitation_sent());
    let minutes = world.settings.read().int("guide.search.timeout.minutes");
    // Java keeps the time in an int.
    p.guides.started_waiting_time = i64::from((now() + minutes * 60) as i32);
    p.guides.waiting_for_guide = true;
    Ok(())
}

/// Java: `MSG_ACCEPT_TUTOR_INVITATION`: the guide goes to the beginner's room.
pub fn accept_tutor_invitation(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    {
        let p = session.player.lock();
        if !p.guides.is_guide || p.guides.invites.is_empty() {
            return Ok(());
        }
    }
    let data = latin1(reader.string().ok_or(Malformed)?);
    if !is_numeric(&data) {
        return Ok(());
    }
    // Java: `Integer.parseInt` throws above an int.
    let Ok(user_id) = data.parse::<i32>() else {
        return Ok(());
    };
    let user_id = user_id as u32;
    if !session.player.lock().guides.invites.contains(&user_id) {
        return Ok(());
    }
    let newb = world.player_by_id(user_id);
    let newb_room = newb.as_ref().and_then(|n| {
        let room_id = n.player.lock().room_id?;
        let room = world.loaded_room(room_id)?;
        let owned = room.lock().is_owner(user_id);
        owned.then_some(room)
    });
    let (Some(newb), Some(room)) = (newb, newb_room) else {
        session.send(out::invite_follow_failed());
        return Ok(());
    };
    let own_room = {
        let mut p = session.player.lock();
        p.guides.invites.retain(|i| *i != user_id);
        p.guides.invited_by = user_id;
        p.room_id
    };
    if own_room != Some(room.id) {
        session.player.lock().authenticate_id = Some(room.id);
        crate::game::moderation::forward(world, session, &room);
    } else {
        guides::tutor_enter_room(world, session, &newb);
    }
    Ok(())
}

/// Java: `MSG_REJECT_TUTOR_INVITATION`.
pub fn reject_tutor_invitation(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    {
        let p = session.player.lock();
        if !p.guides.is_guide || p.guides.invites.is_empty() {
            return Ok(());
        }
    }
    let data = latin1(reader.string().ok_or(Malformed)?);
    if !is_numeric(&data) {
        return Ok(());
    }
    let Ok(user_id) = data.parse::<i32>() else {
        return Ok(());
    };
    let user_id = user_id as u32;
    if !session.player.lock().guides.invites.contains(&user_id) {
        return Ok(());
    }
    let in_room = world
        .player_by_id(user_id)
        .is_some_and(|n| n.player.lock().room_id.is_some());
    if !in_room {
        return Ok(());
    }
    session
        .player
        .lock()
        .guides
        .invites
        .retain(|i| *i != user_id);
    session.send(out::invite_cancelled());
    Ok(())
}

/// Java: `MSG_CANCEL_TUTOR_INVITATIONS`: the beginner stops waiting.
pub fn cancel_tutor_invitations(
    _: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if !p.guides.is_guidable || !p.guides.waiting_for_guide {
        return Ok(());
    }
    p.guides.waiting_for_guide = false;
    p.guides.is_guidable = false;
    p.guides.invited.clear();
    Ok(())
}

/// Java: `MSG_WAIT_FOR_TUTOR_INVITATIONS`: a guide is ready for beginners.
pub fn wait_for_tutor_invitations(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if !p.guides.is_guide || p.guides.waiting_for_invitations {
        return Ok(());
    }
    if guides::is_disabled(world) {
        session.send(out::enable_tutor_service_status(3));
        return Ok(());
    }
    if p.messenger
        .as_ref()
        .is_some_and(|m| m.is_friends_limit_reached())
    {
        session.send(out::enable_tutor_service_status(2));
        return Ok(());
    }
    if p.guides.guiding.len() >= MAX_SIMULTANEOUS_GUIDING {
        session.send(out::enable_tutor_service_status(4));
        return Ok(());
    }
    p.guides.waiting_for_invitations = true;
    Ok(())
}

/// Java: `MSG_CANCEL_WAIT_FOR_TUTOR_INVITATIONS`.
pub fn cancel_wait_for_tutor_invitations(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let guide_id = {
        let mut p = session.player.lock();
        if !p.guides.is_guide {
            return Ok(());
        }
        p.guides.waiting_for_invitations = false;
        p.guides.invites.clear();
        p.user_id()
    };
    let Some(guide_id) = guide_id else {
        return Ok(());
    };
    for other in world.players() {
        other
            .player
            .lock()
            .guides
            .invited
            .retain(|i| *i != guide_id);
    }
    Ok(())
}

/// Java: `RESET_TUTORIAL`.
pub fn reset_tutorial(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if !p.logged_in {
        return Ok(());
    }
    if p.guides.is_guide {
        session.send(alert("You cannot restart the tutorial while as a guide."));
        return Ok(());
    }
    p.statistics
        .set_long_value(&world.store, stat::HAS_TUTORIAL, 1);
    if !p.badges.has_badge("ACH_Student1") {
        p.statistics.set_long_value(&world.store, IS_GUIDABLE, 1);
    }
    session.send(alert(
        "You may now do the tutorial again, please relog for it to take effect.",
    ));
    Ok(())
}

/// Java: `MSG_REMOVE_ACCOUNT_HELP_TEXT`: the tutorial's steps (closing it twice ends it).
pub fn remove_account_help_text(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if !p.guides.has_tutorial {
        return Ok(());
    }
    if p.guides.block_tutorial {
        p.guides.block_tutorial = false;
        p.guides.cancel_tutorial = true;
        return Ok(());
    }
    reader.int().ok_or(Malformed)?;
    if p.guides.cancel_tutorial {
        p.guides.cancel_tutorial = false;
        guides::try_clear_tutorial(world, &mut p);
        p.statistics.set_long_value(&world.store, IS_GUIDABLE, 0);
        achievements::try_progress(world, session, &mut p, AchievementType::Graduate);
        return Ok(());
    }
    if !p.guides.can_use_tutorial {
        p.guides.can_use_tutorial = true;
    }
    Ok(())
}
