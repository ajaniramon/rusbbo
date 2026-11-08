//! Java: `messages/incoming/rooms/user` (walking, talking, waving, typing, leaving).

use std::sync::Arc;

use havana_protocol::body::ClientReader;

use crate::game::commands;
use crate::game::now;
use crate::game::room::entity::StatusType;
use crate::game::room::items::Interaction;
use crate::game::room::{self, Receiver, Room, current_room, filter_input, is_blank, run_deferred};
use crate::messages::Malformed;
use crate::messages::incoming::latin1;
use crate::messages::outgoing::rooms_user::{self as out, ChatType};
use crate::messages::outgoing::user as out_user;
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
        p.authenticate_teleporter = None;
    }
    room::leave_current_room(world, session, false);
    Ok(())
}

/// Java: `SET_SOUND_SETTING`.
pub fn set_sound_setting(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let enabled = reader.bool().ok_or(Malformed)?;
    let mut p = session.player.lock();
    if let Some(details) = p.details.as_mut() {
        details.sound_enabled = enabled;
        if let Err(e) = world
            .store
            .update_user(details.id, |u| u.sound_enabled = enabled)
        {
            tracing::error!(user = details.id, "saving sound setting: {e}");
        }
    }
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
                r.walk_to_player(world, idx, 21, 19);
            } else {
                r.walk_to_player(world, idx, 21, 7);
            }
        }
    }
    if r.model.name == "park_a" && goal_x == 28 && goal_y == 4 {
        return Ok(());
    }
    if r.entities[idx].has_item_debug {
        let goal = crate::game::pathfinder::Position::new(goal_x, goal_y);
        if let Some(item) = r.mapping.highest_item(goal) {
            session.send(crate::messages::outgoing::alerts::alert(
                &item.debug_alert(),
            ));
            return Ok(());
        }
    }
    r.walk_to_player(world, idx, goal_x, goal_y);
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

/// What the chat handlers need to know about the speaker.
struct Speaker {
    rank: u8,
    created_at: i64,
    filter: bool,
    name: String,
}

fn speaker(session: &Session) -> Option<Speaker> {
    let p = session.player.lock();
    let d = p.details.as_ref()?;
    Some(Speaker {
        rank: d.rank,
        created_at: d.created_at,
        filter: d.wordfilter_enabled,
        name: d.username.clone(),
    })
}

/// Java: `WordfilterManager.filterMandatorySentence(StringUtil.filterInput(text, true))`.
fn mandatory_filtered(world: &World, text: &[u8]) -> Vec<u8> {
    let settings = world.settings.read();
    world
        .wordfilter
        .filter_mandatory(&settings, &filter_input(text))
}

/// Java: `WordfilterManager.hasBannableSentence`: only users who joined at most 30 minutes ago.
pub(crate) fn is_bannable(world: &World, created_at: i64, sentence: &[u8]) -> bool {
    let minutes_since_joined = (now() - created_at).div_euclid(60);
    if minutes_since_joined > 30 {
        return false;
    }
    let settings = world.settings.read();
    world.wordfilter.has_bannable_sentence(&settings, sentence)
}

/// `hasBannableSentence` then `performBan`: true when the speaker was banned (and the message
/// must go nowhere).
fn banned_for(world: &World, session: &Session, speaker: &Speaker, sentence: &[u8]) -> bool {
    if !is_bannable(world, speaker.created_at, sentence) {
        return false;
    }
    // The player lock must be released first: the ban locks every online player.
    let user_id = session.player.lock().user_id();
    if let Some(user_id) = user_id {
        crate::game::moderation::perform_wordfilter_ban(world, user_id);
    }
    true
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
    let Some(speaker) = speaker(session) else {
        return Ok(());
    };
    let message = mandatory_filtered(world, original);
    if is_blank(&message) {
        return Ok(());
    }
    if banned_for(world, session, &speaker, original) {
        return Ok(());
    }
    if commands::has_command(speaker.rank, original) {
        commands::invoke_command(world, session, original);
        return Ok(());
    }
    if crate::game::moderation::check_muted(session) {
        return Ok(());
    }
    let deferred = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return Ok(());
        };
        r.entities[idx].is_typing = false;
        let msg = out::typing_status(r.entities[idx].instance_id, false);
        r.send(msg);
        if !message.is_empty() {
            r.talk(world, idx, &message, kind, None);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `WHISPER`: `"<username> <message>"` to the speaker and the named user, wherever they are.
pub fn whisper(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let contents = reader.string().ok_or(Malformed)?;
    // Java: indexOf(" ") is -1 without a space and substring throws.
    let space = contents.iter().position(|&c| c == b' ').ok_or(Malformed)?;
    let username = latin1(&contents[..space]);
    let text = filter_input(&contents[space + 1..]);
    let message = mandatory_filtered(world, &text);
    if is_blank(&message) {
        return Ok(());
    }
    let Some(speaker) = speaker(session) else {
        return Ok(());
    };
    if banned_for(world, session, &speaker, &text) {
        return Ok(());
    }
    if crate::game::moderation::check_muted(session) {
        return Ok(());
    }
    let mut receivers = vec![Receiver {
        session: session.clone(),
        filter: speaker.filter,
    }];
    if let Some(target) = world.player_by_name(&username) {
        if !target.ignored.lock().contains(speaker.name.as_bytes()) {
            let filter = target
                .player
                .lock()
                .details
                .as_ref()
                .is_some_and(|d| d.wordfilter_enabled);
            receivers.push(Receiver {
                session: target,
                filter,
            });
        }
    }
    let deferred = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return Ok(());
        };
        r.talk(world, idx, &message, ChatType::Whisper, Some(receivers));
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `LOOKTO`. Shockwave sends `"x y"` as the raw body; otherwise two ints.
pub fn look_to(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let contents = reader.remaining();
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return Ok(());
    };
    // Not on Wobble Squabble tiles.
    if r.current_item(idx).is_some_and(|i| {
        matches!(
            i.interaction,
            Interaction::WsJoinQueue | Interaction::WsQueueTile | Interaction::WsTileStart
        )
    }) {
        return Ok(());
    }
    let (x, y) = if contents.contains(&b' ') {
        // Java: Integer.parseInt on the first two parts; a bad number ends the handler.
        let text = latin1(contents);
        let mut parts = text.split(' ');
        let x = parts.next().and_then(|s| s.parse().ok()).ok_or(Malformed)?;
        let y = parts.next().and_then(|s| s.parse().ok()).ok_or(Malformed)?;
        (x, y)
    } else {
        (
            reader.int().ok_or(Malformed)?,
            reader.int().ok_or(Malformed)?,
        )
    };
    r.look_to(world, idx, x, y);
    Ok(())
}

/// Java: `DANCE`.
pub fn dance(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let club_dance = session.player.lock().has_fuse("fuse_use_club_dance");
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return Ok(());
    };
    let s = &r.entities[idx].statuses;
    if s.contains(StatusType::Swim) || s.contains(StatusType::Sit) || s.contains(StatusType::Lay) {
        return Ok(()); // No dancing while sitting or laying.
    }
    let dance_id = reader.int().ok_or(Malformed)?;
    match dance_id {
        0 => r.dance(idx, 0),
        1 => r.dance(idx, 1),
        2..=4 => {
            if !club_dance {
                return Ok(());
            }
            r.dance(idx, dance_id);
        }
        _ => {}
    }
    r.stop_carrying(idx);
    r.reset_room_timer(world, idx);
    Ok(())
}

/// Java: `STOP`: the raw body says what to stop.
pub fn stop(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let what = reader.remaining();
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return Ok(());
    };
    if what == b"Dance" {
        r.dance(idx, 0);
    }
    if what == b"CarryItem" {
        r.stop_carrying(idx);
    }
    r.reset_room_timer(world, idx);
    Ok(())
}

/// Java: `CARRYDRINK`.
pub fn carry_drink(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    carry(world, session, reader, true)
}

/// Java: `CARRYITEM`.
pub fn carry_item(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    carry(world, session, reader, false)
}

/// What `CARRYDRINK` and `CARRYITEM` share; they differ in how they refresh the user and in
/// `CARRYITEM` carrying even id 0.
fn carry(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
    drink: bool,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let carry_id = reader.int().ok_or(Malformed)?;
    // Java: RoomPlayer.canCarry needs the hand for the camera (carry id 20).
    let (has_camera, film) = {
        let p = session.player.lock();
        (
            p.inventory.has_sprite("camera"),
            p.details.as_ref().map_or(0, |d| d.film),
        )
    };
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return Ok(());
    };
    // Flats would let users carry what an adjacent vending machine hands out, but that part of
    // `canCarry` is commented out in Java (the machines hand out drinks themselves).
    let can_carry = |r: &Room| {
        if carry_id == 20 {
            has_camera
        } else {
            r.is_public()
        }
    };
    if carry_id > 0 && !can_carry(&r) {
        return Ok(());
    }
    if carry_id == 20 {
        r.stop_carrying(idx);
        let e = &mut r.entities[idx];
        e.statuses.set(StatusType::CarryItem, "20");
        e.needs_update = true;
        session.send(out_user::film(film));
        return Ok(());
    }
    if carry_id == 0 && r.entities[idx].statuses.contains(StatusType::CarryItem) {
        let e = &mut r.entities[idx];
        e.statuses.remove(StatusType::CarryItem);
        e.needs_update = true;
        let instance_id = e.instance_id;
        if drink && r.entities[idx].statuses.contains(StatusType::Sit) {
            r.refresh_user(idx);
        } else {
            // Hacky fix to remove "Carrying: camera".
            r.send(out::user_dance(instance_id, 1));
            r.send(out::user_dance(instance_id, 0));
        }
    }
    if !drink || carry_id > 0 {
        r.carry_item(world, idx, carry_id, None);
    } else if r.is_carrying(idx) {
        r.stop_carrying(idx);
    }
    r.reset_room_timer(world, idx);
    Ok(())
}

/// Java: `GOAWAY`: walk to the door and leave.
pub fn go_away(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let deferred = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return Ok(());
        };
        if !r.entities[idx].walking_allowed {
            return Ok(());
        }
        if r.is_public() {
            let door = r.model.door_location();
            if world.walkway_at(&r, door).is_some() {
                return Ok(());
            }
        }
        r.kick(world, idx, true, false);
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
