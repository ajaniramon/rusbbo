//! Java: `messages/incoming/rooms/user` (walking, talking, waving, typing, leaving).

use std::sync::Arc;

use havana_protocol::body::ClientReader;

use crate::game::now;
use crate::game::room::{self, current_room, filter_input, is_blank, run_deferred};
use crate::messages::Malformed;
use crate::messages::outgoing::rooms_user::{self as out, ChatType};
use crate::session::Session;
use crate::world::World;

/// Java: `QUIT`. Shockwave clients get no `HOTEL_VIEW`. TODO(flash): Flash clients do.
pub fn quit(world: &World, session: &Arc<Session>, _: &mut ClientReader) -> Result<(), Malformed> {
    {
        let mut p = session.player.lock();
        if p.room_id.is_none() {
            return Ok(());
        }
        p.authenticate_id = None;
    }
    // TODO(items): clear the teleporter authentication.
    room::leave_current_room(world, session, false);
    Ok(())
}

/// Java: `WALK`.
pub fn walk(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return Ok(());
    };
    if !r.entities[idx].walking_allowed {
        return Ok(());
    }
    let contents = reader.remaining();
    let goal_x = reader.int().ok_or(Malformed)?;
    let goal_y = reader.int().ok_or(Malformed)?;

    if r.model.name == "md_a" {
        // Older CCTs send the Wobble Squabble queue as fixed bodies.
        if contents == b"@U@S" || contents == b"@U@G" {
            if r.current_item(idx)
                .is_some_and(|i| i.sprite == "wsQueueTile")
            {
                return Ok(());
            }
            if contents == b"@U@S" {
                r.walk_to(idx, 21, 19);
            } else {
                r.walk_to(idx, 21, 7);
            }
        }
    }
    if r.model.name == "park_a" && goal_x == 28 && goal_y == 4 {
        return Ok(());
    }
    r.walk_to(idx, goal_x, goal_y);
    Ok(())
}

/// Java: `CHAT`.
pub fn chat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    talk(world, session, reader, ChatType::Chat)
}

/// Java: `SHOUT`.
pub fn shout(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    talk(world, session, reader, ChatType::Shout)
}

/// What `CHAT` and `SHOUT` share.
fn talk(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
    kind: ChatType,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let original = reader.string().ok_or(Malformed)?;
    // TODO(wordfilter): WordfilterManager.filterMandatorySentence and the bannable words.
    let message = filter_input(original);
    if is_blank(&message) {
        return Ok(());
    }
    // TODO(commands): ":command" messages. TODO(moderation): muted users (users_mutes).
    let deferred = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return Ok(());
        };
        r.entities[idx].is_typing = false;
        let msg = out::typing_status(r.entities[idx].instance_id, false);
        r.send(msg);
        if !message.is_empty() {
            r.talk(world, idx, &message, kind);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `WAVE`.
pub fn wave(world: &World, session: &Arc<Session>, _: &mut ClientReader) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    if let Some(idx) = r.entity_index(session.id) {
        r.wave(idx);
        r.reset_room_timer(world, idx);
    }
    Ok(())
}

/// Java: `USER_START_TYPING`.
pub fn start_typing(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let timeout = world.settings.read().int("talk.bubble.timeout.seconds");
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return Ok(());
    };
    // TODO(games): not in game arenas.
    let e = &mut r.entities[idx];
    if e.is_typing {
        return Ok(());
    }
    if timeout > 0 {
        e.chat_bubble_timer = now() + timeout;
    }
    e.is_typing = true;
    let msg = out::typing_status(e.instance_id, true);
    r.send(msg);
    Ok(())
}

/// Java: `USER_CANCEL_TYPING`.
pub fn cancel_typing(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return Ok(());
    };
    let e = &mut r.entities[idx];
    if !e.is_typing {
        return Ok(());
    }
    e.chat_bubble_timer = -1;
    e.is_typing = false;
    let msg = out::typing_status(e.instance_id, false);
    r.send(msg);
    Ok(())
}
