//! Java: `messages/incoming/events`: room events from the navigator's events tab. Flash clients
//! also send up to two tags with an event.

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use super::latin1;
use crate::game::events::{self, Event};
use crate::game::room::{RoomRef, current_room};
use crate::messages::Malformed;
use crate::messages::outgoing::events as out;
use crate::session::Session;
use crate::world::World;

/// The room the player is in and whether they own it.
fn own_room(world: &World, session: &Session) -> Option<(RoomRef, bool)> {
    let user_id = session.player.lock().user_id()?;
    let room = current_room(world, session)?;
    let is_owner = room.lock().is_owner(user_id);
    Some((room, is_owner))
}

/// Java: `WordfilterManager.filterSentence(StringUtil.filterInput(text, true))`.
fn event_text(world: &World, text: &[u8]) -> String {
    let filtered = crate::game::room::filter_input(text);
    let settings = world.settings.read();
    latin1(&world.wordfilter.filter_sentence(&settings, &filtered))
}

/// Java: `room.send(new ROOMEEVENT_INFO(event))`. Hold no lock.
fn send_event_info(world: &World, room: &RoomRef, event: Option<&Event>) {
    let view = match event {
        Some(event) => match events::view(world, event) {
            Some(v) => Some(v),
            None => return,
        },
        None => None,
    };
    room.lock()
        .send_with(|flash| out::room_event_info(view.as_ref(), flash));
}

/// Java: the tags a Flash client sends with an event (two at most read).
fn flash_tags(
    session: &Session,
    reader: &mut ClientReader,
) -> Result<Option<Vec<String>>, Malformed> {
    if !session.is_flash() {
        return Ok(None);
    }
    let amount = reader.int().ok_or(Malformed)?;
    let mut tags = Vec::new();
    for i in 0..amount {
        if i > 1 {
            break;
        }
        let tag = crate::web::text::filter_input(&latin1(reader.string().ok_or(Malformed)?), true);
        tags.push(tag.replace(',', " "));
    }
    Ok(Some(tags))
}

/// Java: `CAN_CREATE_ROOMEVENT`.
pub fn can_create_room_event(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let can = events::can_create_event(world, session);
    session.send(out::room_event_permission(can));
    Ok(())
}

/// Java: `CREATE_ROOMEVENT`: types 1 to 11; the room sees the new event.
pub fn create_room_event(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if !events::can_create_event(world, session) {
        return Ok(());
    }
    let category_id = reader.int().ok_or(Malformed)?;
    if !(1..=11).contains(&category_id) {
        return Ok(());
    }
    let name = event_text(world, reader.string().ok_or(Malformed)?);
    let description = event_text(world, reader.string().ok_or(Malformed)?);
    // Java: `new Event(.., String.join(",", tags))` splits them again, so a Shockwave event
    // (no tags) has one empty tag.
    let tags = events::java_split(&flash_tags(session, reader)?.unwrap_or_default().join(","));
    let Some(user_id) = session.player.lock().user_id() else {
        return Ok(());
    };
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let event = world.events.create(
        world,
        Event {
            room_id: room.id,
            user_id,
            category_id,
            name,
            description,
            expire: 0,
            tags,
        },
    );
    send_event_info(world, &room, Some(&event));
    Ok(())
}

/// Java: `EDIT_ROOMEVENT`: the owner changes the type, name and description; the room sees it.
pub fn edit_room_event(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((room, true)) = own_room(world, session) else {
        return Ok(());
    };
    let Some(mut event) = world.events.event_by_room(room.id) else {
        return Ok(());
    };
    let category_id = reader.int().ok_or(Malformed)?;
    if !(1..=11).contains(&category_id) {
        return Ok(());
    }
    event.name = event_text(world, reader.string().ok_or(Malformed)?);
    event.description = event_text(world, reader.string().ok_or(Malformed)?);
    event.category_id = category_id;
    if let Some(tags) = flash_tags(session, reader)? {
        event.tags = tags;
    }
    world.events.update(world, &event);
    send_event_info(world, &room, Some(&event));
    Ok(())
}

/// Java: `QUIT_ROOMEVENT`: the owner ends the event; the room is told there is none.
pub fn quit_room_event(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some((room, true)) = own_room(world, session) else {
        return Ok(());
    };
    if world.events.event_by_room(room.id).is_none() {
        return Ok(());
    }
    world.events.remove(&world.store, room.id);
    send_event_info(world, &room, None);
    Ok(())
}

/// Java: `GET_ROOMEVENT_TYPE_COUNT`: `events.category.count`.
pub fn get_room_event_type_count(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let count = world.settings.read().int("events.category.count") as i32;
    session.send(out::room_event_types(count));
    Ok(())
}

/// Java: `GET_ROOMEVENTS_BY_TYPE`: types 0 (every event, the busiest first) to 11.
pub fn get_room_events_by_type(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let category_id = reader.int().ok_or(Malformed)?;
    if !(0..=11).contains(&category_id) {
        return Ok(());
    }
    let list = events::events_of_type(world, category_id);
    // Java's composer throws on an event whose hoster does not exist: nothing is sent.
    let views: Option<Vec<_>> = list.iter().map(|e| events::view(world, e)).collect();
    if let Some(views) = views {
        session.send(out::room_event_list(category_id, &views));
    }
    Ok(())
}
