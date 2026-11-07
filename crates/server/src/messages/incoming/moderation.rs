//! Java: `messages/incoming/moderation`: the call for help (CRY) and the moderation tool.

use std::sync::Arc;

use havana_protocol::body::ClientReader;

use super::latin1;
use crate::game::messenger::filter_input;
use crate::game::moderation::actions::{self, ActionContext};
use crate::game::moderation::cfh::RECEIVE_CALLS_FOR_HELP;
use crate::game::room::current_room;
use crate::messages::Malformed;
use crate::messages::outgoing::moderation as out;
use crate::session::Session;
use crate::world::World;

/// Java: `MODERATORACTION`.
pub fn moderator_action(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let target_type = reader.int().ok_or(Malformed)?;
    let action_type = reader.int().ok_or(Malformed)?;
    let alert_message = latin1(reader.string().ok_or(Malformed)?);
    let notes = latin1(reader.string().ok_or(Malformed)?);
    let ctx = ActionContext {
        world,
        session,
        alert_message: &alert_message,
        notes: &notes,
    };
    actions::perform(&ctx, target_type, action_type, reader)
}

fn user_id(session: &Session) -> Option<u32> {
    session.player.lock().user_id()
}

fn can_receive_calls(session: &Session) -> bool {
    session.player.lock().has_fuse(RECEIVE_CALLS_FOR_HELP)
}

/// Java: `Integer.parseInt(reader.readString())`: a call id sent as text.
fn call_id(reader: &mut ClientReader) -> Result<i32, Malformed> {
    latin1(reader.string().ok_or(Malformed)?)
        .parse()
        .map_err(|_| Malformed)
}

/// Java: `REQUEST_CFH`: the caller's open call, if any.
pub fn request_cfh(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(id) = user_id(session) else {
        return Ok(());
    };
    let call = world.cfh.pending_call(id);
    let msg = match &call {
        Some(c) => c.with_view(|v| out::cfh_ack(Some(v))),
        None => out::cfh_ack(None),
    };
    session.send(msg);
    Ok(())
}

/// Java: `SUBMIT_CFH`: one open call per user, from inside a room.
pub fn submit_cfh(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let message = filter_input(&latin1(reader.string().ok_or(Malformed)?), false);
    if message.is_empty() {
        return Ok(());
    }
    let Some(id) = user_id(session) else {
        return Ok(());
    };
    if world.cfh.pending_call(id).is_some() {
        return Ok(());
    }
    world.cfh.submit_call(world, id, room, message);
    session.send(out::cry_received());
    Ok(())
}

/// Java: `PICK_CALLFORHELP`: a moderator takes a call, and may close it at once.
pub fn pick_call_for_help(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if !can_receive_calls(session) {
        return Ok(());
    }
    let id = call_id(reader)?;
    let block = reader.bool().ok_or(Malformed)?;
    if world.cfh.get_call(id).is_none() {
        return Ok(());
    }
    let Some(moderator) = user_id(session) else {
        return Ok(());
    };
    world.cfh.pick_up(world, id, moderator);
    if block {
        world.cfh.delete_call(world, id);
    }
    Ok(())
}

/// Java: `MESSAGETOCALLER`: the moderator's answer goes to the caller, and the call closes.
pub fn message_to_caller(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if !can_receive_calls(session) {
        return Ok(());
    }
    let id = call_id(reader)?;
    let message = latin1(reader.string().ok_or(Malformed)?);
    let Some(call) = world.cfh.get_call(id) else {
        return Ok(());
    };
    let Some(caller) = world.player_by_id(call.caller_id) else {
        return Ok(());
    };
    caller.send(out::cry_reply(&message));
    world.cfh.delete_call(world, id);
    Ok(())
}

/// Java: `CHANGECALLCATEGORY`.
pub fn change_call_category(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if !can_receive_calls(session) {
        return Ok(());
    }
    let id = call_id(reader)?;
    let category = reader.int().ok_or(Malformed)?;
    // Java passes a missing call on and throws inside changeCategory: nothing is sent.
    world.cfh.change_category(world, id, category);
    Ok(())
}

/// Java: `DELETE_CRY`: the caller withdraws their open call.
pub fn delete_cry(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(id) = user_id(session) else {
        return Ok(());
    };
    let Some(call) = world.cfh.pending_call(id) else {
        return Ok(());
    };
    world.cfh.delete_call(world, call.cry_id);
    session.send(out::cfh_ack(None));
    Ok(())
}

/// Java: `FOLLOW_CRYFORHELP`: the moderator goes to the caller's room, and the call closes.
pub fn follow_cry_for_help(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if !can_receive_calls(session) {
        return Ok(());
    }
    let id = call_id(reader)?;
    let Some(call) = world.cfh.get_call(id) else {
        return Ok(());
    };
    crate::game::moderation::forward(session, &call.room);
    world.cfh.delete_call(world, id);
    Ok(())
}
