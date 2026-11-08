//! Room events (Java: `game/events`: `EventsManager`, `Event`): a flat's owner announces what is
//! on in their room, listed in the navigator's events tab by type.
//!
//! The list lives in [`EventsManager`] behind its own lock, a leaf: nothing else is locked while
//! it is held, so it may be used with or without a room or player lock.

use parking_lot::Mutex;
use rusbbo_storage::{RoomEvent, Store};
use tracing::error;

use crate::game::now;
use crate::messages::outgoing::events::{self as out, EventView};
use crate::session::Session;
use crate::world::World;

/// Java: `Event`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub room_id: u32,
    pub user_id: u32,
    pub category_id: i32,
    pub name: String,
    pub description: String,
    /// Unix seconds (Java: `expire`).
    pub expire: i64,
    /// Java: `tags`, `tags.split(",")`.
    pub tags: Vec<String>,
}

impl Event {
    fn from_row(row: RoomEvent) -> Self {
        Self {
            room_id: row.room_id,
            user_id: row.user_id,
            category_id: row.category_id,
            name: row.name,
            description: row.description,
            expire: row.expire_time,
            tags: java_split(&row.tags),
        }
    }

    fn row(&self) -> RoomEvent {
        RoomEvent {
            room_id: self.room_id,
            user_id: self.user_id,
            category_id: self.category_id,
            name: self.name.clone(),
            description: self.description.clone(),
            expire_time: self.expire,
            tags: self.tags.join(","),
        }
    }

    /// Java: `Event.isExpired`.
    pub fn is_expired(&self) -> bool {
        now() > self.expire
    }
}

/// Java's `String.split(",")`: trailing empty strings go, but `""` is `[""]`.
fn java_split(s: &str) -> Vec<String> {
    let mut parts: Vec<String> = s.split(',').map(str::to_string).collect();
    while parts.len() > 1 && parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    if parts.iter().all(String::is_empty) && !s.is_empty() {
        parts.clear();
    }
    parts
}

/// Java: `EventsManager.getEventLifetime`: `events.expiry.minutes` in seconds.
pub fn event_lifetime(world: &World) -> i64 {
    world.settings.read().int("events.expiry.minutes") * 60
}

/// Java: `EventsManager`.
#[derive(Default)]
pub struct EventsManager {
    events: Mutex<Vec<Event>>,
}

impl EventsManager {
    /// Java: `new EventsManager()`: the expired rows are deleted, the others loaded.
    pub fn load(store: &Store) -> Self {
        let manager = Self::default();
        manager.reload(store);
        manager
    }

    /// Java: `EventsManager.reset` (`:reload events`).
    pub fn reload(&self, store: &Store) {
        if let Err(e) = store.remove_expired_room_events(now()) {
            error!("removing expired events: {e}");
        }
        let rows = store.room_events().unwrap_or_else(|e| {
            error!("loading events: {e}");
            Vec::new()
        });
        *self.events.lock() = rows.into_iter().map(Event::from_row).collect();
        self.remove_expired(store);
    }

    /// Java: `EventsManager.createEvent`.
    pub fn create(&self, world: &World, mut event: Event) -> Event {
        event.expire = now() + event_lifetime(world);
        if let Err(e) = world.store.put_room_event(&event.row()) {
            error!(room = event.room_id, "saving an event: {e}");
        }
        self.events.lock().push(event.clone());
        event
    }

    /// Java: `EventsDao.save` after `EDIT_ROOMEVENT` changed the event in place.
    pub fn update(&self, world: &World, event: &Event) {
        for e in self.events.lock().iter_mut() {
            if e.room_id == event.room_id && !e.is_expired() {
                *e = event.clone();
                break;
            }
        }
        if let Err(e) = world.store.put_room_event(&event.row()) {
            error!(room = event.room_id, "saving an event: {e}");
        }
    }

    /// Java: `getEventByRoomId`: the room's event that has not expired.
    pub fn event_by_room(&self, room_id: u32) -> Option<Event> {
        self.events
            .lock()
            .iter()
            .find(|e| e.room_id == room_id && !e.is_expired())
            .cloned()
    }

    /// Java: `isHostingEvent` (expired events count until they are purged).
    pub fn is_hosting(&self, user_id: u32) -> bool {
        self.events.lock().iter().any(|e| e.user_id == user_id)
    }

    /// Java: `getEventList`: the events that have not expired.
    pub fn event_list(&self) -> Vec<Event> {
        self.events
            .lock()
            .iter()
            .filter(|e| !e.is_expired())
            .cloned()
            .collect()
    }

    /// Java: `removeEvent`: every event of the room.
    pub fn remove(&self, store: &Store, room_id: u32) {
        self.events.lock().retain(|e| e.room_id != room_id);
        if let Err(e) = store.remove_room_events(&[room_id]) {
            error!(room = room_id, "removing an event: {e}");
        }
    }

    /// Java: `removeExpiredEvents`.
    pub fn remove_expired(&self, store: &Store) {
        let expired: Vec<u32> = {
            let mut events = self.events.lock();
            let expired = events
                .iter()
                .filter(|e| e.is_expired())
                .map(|e| e.room_id)
                .collect();
            events.retain(|e| !e.is_expired());
            expired
        };
        if let Err(e) = store.remove_room_events(&expired) {
            error!("removing expired events: {e}");
        }
    }
}

/// Java: `EventsManager.getEvents(categoryId)`: type 0 is every event, the one with the most
/// players first (a stable sort, as `List.sort`); other types in list order.
pub fn events_of_type(world: &World, category_id: i32) -> Vec<Event> {
    let events = world.events.event_list();
    if category_id != 0 {
        return events
            .into_iter()
            .filter(|e| e.category_id == category_id)
            .collect();
    }
    // Java: `getPlayersInEvent`, the players of the room if it is loaded.
    let mut counted: Vec<(i32, Event)> = events
        .into_iter()
        .map(|e| {
            let players = world
                .loaded_room(e.room_id)
                .map_or(0, |room| room.lock().data.visitors_now);
            (players, e)
        })
        .collect();
    counted.sort_by_key(|(players, _)| std::cmp::Reverse(*players));
    counted.into_iter().map(|(_, e)| e).collect()
}

/// Java: `EventsManager.canCreateEvent`: the owner of the flat the player is in, hosting no event
/// and in a room without one.
pub fn can_create_event(world: &World, session: &Session) -> bool {
    let Some(user_id) = session.player.lock().user_id() else {
        return false;
    };
    let Some(room) = crate::game::room::current_room(world, session) else {
        return false;
    };
    let (is_public, is_owner) = {
        let r = room.lock();
        (r.is_public(), r.is_owner(user_id))
    };
    if is_public || !is_owner {
        return false;
    }
    !world.events.is_hosting(user_id) && world.events.event_by_room(room.id).is_none()
}

/// Java: `Event.getEventHoster().getName()` (`PlayerManager.getPlayerData`: the online player,
/// else the stored user). Takes a player lock: hold no room lock.
pub fn hoster_name(world: &World, user_id: u32) -> Option<String> {
    crate::game::messenger::manager::online_details(world, user_id)
        .or_else(|| world.store.user(user_id).ok().flatten())
        .map(|u| u.username)
}

/// What the composers show of `event`; `None` when its hoster does not exist (Java's composer
/// throws on the `null` details, and nothing is sent). Hold no room lock.
pub fn view<'a>(world: &World, event: &'a Event) -> Option<EventView<'a>> {
    Some(EventView {
        event,
        hoster_name: hoster_name(world, event.user_id)?,
        started: event.expire - event_lifetime(world),
    })
}

/// Java: `new ROOMEEVENT_INFO(getEventByRoomId(roomId))`. `None` when the composer would throw
/// (see [`view`]). Hold no room lock.
pub fn event_info(world: &World, room_id: u32) -> Option<rusbbo_protocol::Message> {
    match world.events.event_by_room(room_id) {
        Some(event) => view(world, &event).map(|v| out::room_event_info(Some(&v))),
        None => Some(out::room_event_info(None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_split_like_java() {
        assert_eq!(java_split(""), [""]);
        assert_eq!(java_split("a,b"), ["a", "b"]);
        assert_eq!(java_split("a,,"), ["a"]);
        assert!(java_split(",").is_empty());
    }
}
