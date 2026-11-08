//! Tests of the social features the traces cannot reach: the favourites limit with public
//! rooms only, votes matched by IP address and recounted, a friend who does not allow following,
//! the chat log queue, and room events.

use rusbbo_protocol::body::ClientBody;
use rusbbo_storage::{Room, RoomVote, User};

use crate::game::messenger::Messenger;
use crate::game::navigator::{self, RoomEntry};
use crate::game::room::{self, entity::EntityDetails};
use crate::messages::incoming::{events, messenger, navigator as nav, rooms_user};
use crate::testkit::*;

fn room_body(room_type: i32, id: i32) -> ClientBody {
    ClientBody::new().int(room_type).int(id)
}

fn enter(f: &Fixture, client: &Client) {
    let room_ref = f.world.room_by_id(ROOM).unwrap();
    room::enter_room(&f.world, &client.session, room_ref, None, true);
}

#[test]
fn favourites_stop_at_30_and_list_newest_room_ids_first() {
    let f = Fixture::new();
    let mut w = f.store().write().unwrap();
    for id in 1..=31 {
        w.put_room(&Room {
            id,
            name: format!("Public {id}"),
            model: "model_a".into(),
            ..Default::default()
        })
        .unwrap();
    }
    w.commit().unwrap();
    let alice = f.client(ALICE);
    for id in 1..=30 {
        alice.send(&f.world, nav::add_favourite_room, room_body(1, 1000 + id));
    }
    alice.send(&f.world, nav::add_favourite_room, room_body(0, ROOM as i32));
    assert_eq!(f.store().room_favourites(ALICE).unwrap().len(), 30);
    assert!(!f.store().room_favourites(ALICE).unwrap().contains(&ROOM));

    alice.send(&f.world, nav::del_favourite_room, room_body(1, 1005));
    alice.send(&f.world, nav::add_favourite_room, room_body(0, ROOM as i32));
    alice.send(&f.world, nav::add_favourite_room, room_body(0, ROOM as i32));
    alice.send(&f.world, nav::add_favourite_room, room_body(0, 4321));
    let ids: Vec<u32> = navigator::favourite_rooms(&f.world, ALICE)
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids.len(), 30);
    assert_eq!(ids[..3], [ROOM, 30, 29]);
    assert!(!ids.contains(&5));
}

#[test]
fn votes_match_ip_addresses_and_are_recounted() {
    let f = Fixture::new();
    f.store()
        .put_user(&User {
            id: 9,
            username: "carol".into(),
            ..Default::default()
        })
        .unwrap();
    f.store().log_ip(BOB, "10.0.0.1", 1).unwrap();
    f.store().log_ip(9, "10.0.0.1", 2).unwrap();
    let bob = f.client(BOB);
    enter(&f, &bob);
    bob.send(&f.world, rooms_user::rate_flat, ClientBody::new().int(1));
    let room_ref = f.world.loaded_room(ROOM).unwrap();
    {
        let r = room_ref.lock();
        assert_eq!(r.data.rating, 1);
        let carol = EntityDetails {
            user_id: 9,
            ..Default::default()
        };
        assert!(r.has_voted(&f.world, &carol), "same IP address as bob");
        let stranger = EntityDetails {
            user_id: 10,
            ..Default::default()
        };
        assert!(!r.has_voted(&f.world, &stranger));
    }
    assert_eq!(f.store().room(ROOM).unwrap().unwrap().rating, 1);
    assert_eq!(room::counted_votes(&f.world, ROOM).len(), 1);

    // A vote of someone with no logged IP address does not count (Java's INNER JOIN).
    let mut w = f.store().write().unwrap();
    w.put_room_vote(&RoomVote {
        user_id: ALICE,
        room_id: ROOM,
        vote: 1,
        expire_time: i64::MAX,
    })
    .unwrap();
    // Bob's vote expires.
    w.put_room_vote(&RoomVote {
        user_id: BOB,
        room_id: ROOM,
        vote: 1,
        expire_time: 1,
    })
    .unwrap();
    w.commit().unwrap();
    assert!(room::counted_votes(&f.world, ROOM).is_empty());

    let entry = |rating| RoomEntry {
        id: ROOM,
        owner_id: ALICE,
        rating,
        ..Default::default()
    };
    // Someone in the room: not recounted.
    navigator::rating_sanity_check(
        &f.world,
        &[RoomEntry {
            visitors_now: 1,
            ..entry(1)
        }],
    );
    assert_eq!(f.store().room(ROOM).unwrap().unwrap().rating, 1);
    // An unrated room ends the check before the next one (Java's `return`).
    navigator::rating_sanity_check(&f.world, &[entry(0), entry(1)]);
    assert_eq!(f.store().room(ROOM).unwrap().unwrap().rating, 1);
    navigator::rating_sanity_check(&f.world, &[entry(1)]);
    assert_eq!(f.store().room(ROOM).unwrap().unwrap().rating, 0);
    assert_eq!(
        f.store().room_votes(ROOM, 0).unwrap().len(),
        1,
        "the expired vote is gone"
    );
}

fn with_messenger(f: &Fixture, client: &Client, id: u32) {
    let user = f.store().user(id).unwrap().unwrap();
    let m = Messenger::load(f.store(), &f.world.settings.read(), &user);
    client.session.player.lock().messenger = Some(m);
}

#[test]
fn following_a_friend_who_does_not_allow_it() {
    let f = Fixture::new();
    f.store().messenger_make_friends(ALICE, BOB).unwrap();
    f.store()
        .update_user(BOB, |u| u.allow_stalking = false)
        .unwrap();
    let mut alice = f.client(ALICE);
    let bob = f.client(BOB);
    with_messenger(&f, &alice, ALICE);
    with_messenger(&f, &bob, BOB);
    enter(&f, &bob);
    alice.headers();
    alice.send(
        &f.world,
        messenger::follow_friend,
        ClientBody::new().int(BOB as i32),
    );
    assert_eq!(alice.headers(), [349], "FOLLOW_ERROR 3");
    assert!(alice.session.player.lock().followed_room.is_none());

    bob.session
        .player
        .lock()
        .details
        .as_mut()
        .unwrap()
        .allow_stalking = true;
    alice.send(
        &f.world,
        messenger::follow_friend,
        ClientBody::new().int(BOB as i32),
    );
    assert_eq!(alice.headers(), [286], "ROOMFORWARD");
    assert!(alice.session.player.lock().followed_room.is_some());
}

#[test]
fn room_chat_is_queued_and_saved() {
    let f = Fixture::new();
    let alice = f.client(ALICE);
    enter(&f, &alice);
    let chat = |text: &str| ClientBody::new().string(text.as_bytes());
    alice.send(&f.world, rooms_user::chat, chat("hola"));
    alice.send(&f.world, rooms_user::shout, chat("HOLA"));
    alice.send(&f.world, rooms_user::chat, chat("hola"));
    let queued = f.world.chat_log.queued();
    let lines: Vec<(u32, u32, u8, &str)> = queued
        .iter()
        .map(|l| (l.user_id, l.room_id, l.chat_type, l.message.as_str()))
        .collect();
    // The same line again (ignoring case, among the last ones) is not saved.
    assert_eq!(lines, [(ALICE, ROOM, 0, "hola")]);
    f.world.chat_log.save(&f.world);
    assert!(f.world.chat_log.queued().is_empty());
    assert_eq!(f.store().room_chat_logs(ROOM, 150).unwrap().len(), 1);
}

#[test]
fn room_events_for_the_owner_only() {
    let f = Fixture::new();
    let alice = f.client(ALICE);
    let bob = f.client(BOB);
    enter(&f, &alice);
    enter(&f, &bob);
    let body = |category: i32, name: &str| {
        ClientBody::new()
            .int(category)
            .string(name.as_bytes())
            .string(b"desc")
    };
    bob.send(&f.world, events::create_room_event, body(1, "Bob's"));
    assert!(f.world.events.event_by_room(ROOM).is_none());
    alice.send(&f.world, events::create_room_event, body(2, "Party"));
    let event = f.world.events.event_by_room(ROOM).unwrap();
    assert_eq!((event.user_id, event.category_id), (ALICE, 2));
    assert_eq!(f.store().room_events().unwrap().len(), 1);
    assert!(f.world.events.is_hosting(ALICE));

    bob.send(&f.world, events::edit_room_event, body(3, "Hijacked"));
    bob.send(&f.world, events::quit_room_event, ClientBody::new());
    assert_eq!(f.world.events.event_by_room(ROOM).unwrap().name, "Party");
    alice.send(&f.world, events::edit_room_event, body(3, "Renamed"));
    assert_eq!(f.store().room_events().unwrap()[0].name, "Renamed");
    alice.send(&f.world, events::quit_room_event, ClientBody::new());
    assert!(f.world.events.event_by_room(ROOM).is_none());
    assert!(f.store().room_events().unwrap().is_empty());
}
