//! Tests of the multi-step writes (purchases, gifts, presents, club, trades) and of the data
//! integrity fixes: each multi-step change is one transaction, so a failure (here an injected
//! commit failure, `Store::fail_next_commits`, or a step that fails midway) leaves both the store
//! and the players' state in memory as they were.

use std::sync::Arc;

use havana_protocol::body::ClientBody;

use crate::game::room;
use crate::game::room::trade::{TradeSide, run_finished};
use crate::messages::incoming::{catalogue, club, rooms_items, rooms_moderation, rooms_settings};
use crate::testkit::*;

fn grpc_body(item: i32, extra: &str) -> ClientBody {
    ClientBody::new()
        .int(0)
        .int(item)
        .string(extra.as_bytes())
        .bool(false)
}

fn gift_body(item: i32, extra: &str, to: &str, note: &str) -> ClientBody {
    ClientBody::new()
        .int(0)
        .int(item)
        .string(extra.as_bytes())
        .bool(true)
        .string(to.as_bytes())
        .string(note.as_bytes())
}

#[test]
fn a_purchase_is_one_transaction() {
    let f = Fixture::new();
    let mut alice = f.client(ALICE);
    let before = f.snapshot();

    // A teleporter is two items, two links and a charge: a failed commit keeps none of them.
    f.store().fail_next_commits(1);
    alice.send(&f.world, catalogue::grpc, grpc_body(TELEPORTER, ""));
    assert_eq!(f.snapshot(), before);
    assert!(alice.hand().is_empty());
    assert_eq!(alice.credits(), 100);
    assert!(
        alice.headers().is_empty(),
        "nothing is said about a purchase that failed"
    );

    alice.send(&f.world, catalogue::grpc, grpc_body(TELEPORTER, ""));
    let items = f.store().inventory_items(ALICE).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(
        f.store().teleporter_link(items[0].id).unwrap(),
        Some(items[1].id)
    );
    assert_eq!(alice.hand().len(), 2);
    assert_eq!(alice.credits(), 95);
    assert_eq!(f.store().user(ALICE).unwrap().unwrap().credits, 95);
    assert!(!alice.headers().is_empty());
    let rows = f.store().transactions(ALICE).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].description, "Teleport Door purchase");
    assert_eq!(rows[0].credit_cost, 5);
    assert_eq!(rows[0].catalogue_id, TELEPORTER.to_string());
    assert_eq!(
        rows[0].item_id,
        items[0].id.to_string(),
        "the bought item, not its partner"
    );
}

#[test]
fn a_charge_that_fails_takes_the_items_back() {
    let f = Fixture::new();
    let alice = f.client(ALICE);
    // A player whose row is gone (user 99 has none): the item is written first, then the
    // charge fails because there is nobody to charge. Nothing of it may stay.
    alice.session.player.lock().details.as_mut().unwrap().id = 99;
    let before = f.snapshot();
    alice.send(&f.world, catalogue::grpc, grpc_body(CHAIR, ""));
    assert!(f.store().inventory_items(99).unwrap().is_empty());
    assert_eq!(f.snapshot(), before);
    assert!(alice.hand().is_empty());
    // With the row back, the same purchase goes through.
    let mut row = f.store().user(ALICE).unwrap().unwrap();
    row.id = 99;
    row.username = "alice2".into();
    f.store().put_user(&row).unwrap();
    alice.send(&f.world, catalogue::grpc, grpc_body(CHAIR, ""));
    assert_eq!(f.store().inventory_items(99).unwrap().len(), 1);
    assert_eq!(f.store().user(99).unwrap().unwrap().credits, 97);
}

#[test]
fn a_pet_purchase_writes_the_nest_and_the_pet_together() {
    let f = Fixture::new();
    let alice = f.client(ALICE);
    let before = f.snapshot();
    f.store().fail_next_commits(1);
    alice.send(&f.world, catalogue::grpc, grpc_body(DOG, "Rex\n1\nffffff"));
    assert_eq!(f.snapshot(), before);
    alice.send(&f.world, catalogue::grpc, grpc_body(DOG, "Rex\n1\nffffff"));
    let nest = &f.store().inventory_items(ALICE).unwrap()[0];
    let pet = f.store().pet_by_item(nest.id).unwrap().unwrap();
    assert_eq!(
        (pet.name.as_str(), pet.race, pet.colour.as_str()),
        ("Rex", 1, "ffffff")
    );
}

#[test]
fn a_gift_is_one_transaction() {
    let f = Fixture::new();
    let mut alice = f.client(ALICE);
    let bob = f.client(BOB);
    let before = f.snapshot();

    f.store().fail_next_commits(1);
    alice.send(&f.world, catalogue::grpc, gift_body(CHAIR, "", "bob", "hi"));
    assert_eq!(f.snapshot(), before);
    assert!(bob.hand().is_empty());
    assert_eq!(alice.credits(), 100);
    assert!(alice.headers().is_empty());
    assert_eq!(
        alice.session.player.lock().last_gift_time,
        0,
        "a failed gift does not count for the one-a-minute limit"
    );

    alice.send(&f.world, catalogue::grpc, gift_body(CHAIR, "", "bob", "hi"));
    let presents = f.store().inventory_items(BOB).unwrap();
    assert_eq!(presents.len(), 1);
    assert!(
        presents[0]
            .custom_data
            .starts_with(&format!("{CHAIR}|alice|hi||"))
    );
    assert_eq!(bob.hand(), [presents[0].id]);
    assert_eq!(alice.credits(), 97);
    let mine: Vec<String> = f
        .store()
        .transactions(ALICE)
        .unwrap()
        .into_iter()
        .map(|t| t.description)
        .collect();
    assert_eq!(
        mine,
        [
            "Chair purchase",
            "Gift purchase from alice for bob - Chair purchase"
        ]
    );
    assert_eq!(f.store().transactions(BOB).unwrap().len(), 1);
}

#[test]
fn a_pet_gift_carries_the_pet_until_it_is_opened() {
    let f = Fixture::new();
    let alice = f.client(ALICE);
    alice.send(
        &f.world,
        catalogue::grpc,
        gift_body(DOG, "Rex|\n1\nffffff", "alice", ""),
    );
    let present = &f.store().inventory_items(ALICE).unwrap()[0];
    let fields: Vec<&str> = present.custom_data.split('|').collect();
    assert_eq!(
        fields[..4],
        [DOG.to_string().as_str(), "alice", "", "Rex\n1\nffffff"]
    );
    assert!(
        f.store().pet_by_item(present.id).unwrap().is_none(),
        "Java creates the pet when the present is opened"
    );
}

/// Alice in her room with a present (holding `extra`, a gift from Bob) placed in it. Returns
/// the present's virtual id.
fn present_in_room(f: &Fixture, alice: &Client, item: i32, extra: &str) -> (u64, i32) {
    let present = f.add_item(ALICE, 20, &format!("{item}|bob||{extra}|0"), ROOM);
    let room_ref = f.world.room_by_id(ROOM).unwrap();
    room::enter_room(&f.world, &alice.session, room_ref.clone(), None, true);
    let virtual_id = room_ref
        .lock()
        .mapping
        .items
        .iter()
        .find(|i| i.flat.as_ref().is_some_and(|fl| fl.record.id == present.id))
        .unwrap()
        .virtual_id;
    (present.id, virtual_id)
}

#[test]
fn opening_a_pet_present_is_one_transaction() {
    let f = Fixture::new();
    let mut alice = f.client(ALICE);
    let (present, virtual_id) = present_in_room(&f, &alice, DOG, "Rex\n1\nffffff");
    alice.headers();
    let before = f.snapshot();

    f.store().fail_next_commits(1);
    let open = || ClientBody::new().int(virtual_id);
    alice.send(&f.world, rooms_items::present_open, open());
    assert_eq!(f.snapshot(), before);
    assert!(
        f.store().item(present).unwrap().is_some(),
        "the present is still there"
    );
    assert!(alice.hand().is_empty());
    assert!(alice.headers().is_empty());

    alice.send(&f.world, rooms_items::present_open, open());
    assert!(f.store().item(present).unwrap().is_none());
    let nest = f.store().inventory_items(ALICE).unwrap();
    assert_eq!(nest.len(), 1);
    assert_eq!(nest[0].definition_id, DOG_DEFINITION);
    let pet = f.store().pet_by_item(nest[0].id).unwrap().unwrap();
    assert_eq!(pet.name, "Rex");
    assert_eq!(alice.hand(), [nest[0].id]);
}

#[test]
fn a_pet_present_with_a_bad_pet_is_used_up() {
    // Java: the opener's name is checked when the pet is created; a pet named after the opener
    // buys nothing, and the present is gone all the same.
    let f = Fixture::new();
    let alice = f.client(ALICE);
    let (present, virtual_id) = present_in_room(&f, &alice, DOG, "alice\n1\nffffff");
    alice.send(
        &f.world,
        rooms_items::present_open,
        ClientBody::new().int(virtual_id),
    );
    assert!(f.store().item(present).unwrap().is_none());
    assert!(f.store().inventory_items(ALICE).unwrap().is_empty());
}

#[test]
fn a_club_subscription_is_one_transaction() {
    let f = Fixture::new();
    let mut alice = f.client(ALICE);
    let before = f.snapshot();
    let buy = || ClientBody::new().string(b"").int(1);

    f.store().fail_next_commits(1);
    alice.send(&f.world, club::subscribe_club, buy());
    assert_eq!(f.snapshot(), before);
    {
        let p = alice.session.player.lock();
        let details = p.details.as_ref().unwrap();
        assert_eq!((details.credits, details.club_expiration), (100, 0));
    }
    assert!(alice.headers().is_empty());

    alice.send(&f.world, club::subscribe_club, buy());
    let stored = f.store().user(ALICE).unwrap().unwrap();
    assert_eq!(stored.credits, 75);
    assert!(stored.club_expiration > 0);
    assert_eq!(alice.credits(), 75);
    let stats = f.store().statistics(ALICE).unwrap().unwrap();
    assert_eq!(stats.values["gifts_due"], 1);
    assert_eq!(
        f.store().transactions(ALICE).unwrap()[0].description,
        "Habbo Club purchase"
    );
}

fn trade_sides(alice: &Client, bob: &Client, a: u64, b: u64) -> [TradeSide; 2] {
    [
        TradeSide {
            session: alice.session.clone(),
            user_id: ALICE,
            items: vec![a],
        },
        TradeSide {
            session: bob.session.clone(),
            user_id: BOB,
            items: vec![b],
        },
    ]
}

#[test]
fn a_trade_is_one_transaction() {
    let f = Fixture::new();
    let a = f.add_item(ALICE, 10, "0", 0);
    let b = f.add_item(BOB, 11, "0", 0);
    let mut alice = f.client(ALICE);
    let mut bob = f.client(BOB);
    for (client, id) in [(&alice, a.id), (&bob, b.id)] {
        client
            .session
            .player
            .lock()
            .inventory
            .set_trading(f.store(), &[id], true);
    }
    let before = f.snapshot();

    f.store().fail_next_commits(1);
    run_finished(&f.world, trade_sides(&alice, &bob, a.id, b.id));
    // Nothing changed hands: the store says so, the hands say so, and the items are out of
    // the trade window again.
    assert_eq!(f.store().item(a.id).unwrap().unwrap().user_id, Some(ALICE));
    assert_eq!(f.store().item(b.id).unwrap().unwrap().user_id, Some(BOB));
    assert_eq!(f.store().transaction_count().unwrap(), 0);
    assert_eq!(alice.hand(), [a.id]);
    assert_eq!(bob.hand(), [b.id]);
    assert!(!f.store().item(a.id).unwrap().unwrap().is_trading);
    assert_ne!(f.snapshot(), before, "only the trade flags were released");
    alice.headers();
    bob.headers();

    run_finished(&f.world, trade_sides(&alice, &bob, a.id, b.id));
    assert_eq!(f.store().item(a.id).unwrap().unwrap().user_id, Some(BOB));
    assert_eq!(f.store().item(b.id).unwrap().unwrap().user_id, Some(ALICE));
    assert_eq!(alice.hand(), [b.id]);
    assert_eq!(bob.hand(), [a.id]);
    let descriptions = |id| -> Vec<String> {
        f.store()
            .transactions(id)
            .unwrap()
            .into_iter()
            .map(|t| t.description)
            .collect()
    };
    assert_eq!(
        descriptions(ALICE),
        ["Traded Teleport Door from bob", "Traded Chair to bob"]
    );
    assert_eq!(
        descriptions(BOB),
        ["Traded Teleport Door to alice", "Traded Chair from alice"]
    );
    assert!(!alice.headers().is_empty());
}

#[test]
fn the_hand_never_brings_back_a_deleted_row() {
    let f = Fixture::new();
    let a = f.add_item(ALICE, 10, "0", 0);
    let b = f.add_item(ALICE, 10, "0", 0);
    let alice = f.client(ALICE);
    // Gone from the store (traded, recycled...) while this copy of the hand still has it.
    f.store().delete_items(&[b.id]).unwrap();
    // A new item first renumbers every other one: `b` would be written back.
    let c = f.add_item(ALICE, 10, "0", 0);
    {
        let mut p = alice.session.player.lock();
        let item = f.world.items.make_item(c.clone()).unwrap();
        p.inventory.add_item(item);
        p.inventory.get_view(f.store(), "new");
    }
    assert!(f.store().item(b.id).unwrap().is_none());
    assert_eq!(f.store().item(a.id).unwrap().unwrap().order_id, 1);
}

fn entities(room_ref: &crate::game::room::RoomRef) -> usize {
    room_ref
        .lock()
        .entities
        .iter()
        .filter(|e| e.is_player())
        .count()
}

#[test]
fn nobody_enters_a_deleted_room() {
    let f = Fixture::new();
    let alice = f.client(ALICE);
    let bob = f.client(BOB);
    let room_ref = f.world.room_by_id(ROOM).unwrap();
    room::enter_room(&f.world, &bob.session, room_ref.clone(), None, true);
    assert_eq!(entities(&room_ref), 1);

    rooms_settings::delete(&f.world, ROOM as i32, ALICE);
    assert_eq!(entities(&room_ref), 0);
    // Still registered (until the dispose delay), still answering, but closed for entry.
    let again = f.world.room_by_id(ROOM).unwrap();
    assert!(Arc::ptr_eq(&again, &room_ref));
    room::enter_room(&f.world, &bob.session, again, None, true);
    assert_eq!(entities(&room_ref), 0);
    assert_eq!(bob.session.player.lock().room_id, None);
    drop(alice);
}

#[test]
fn rights_are_saved_before_they_are_given() {
    let f = Fixture::new();
    let alice = f.client(ALICE);
    let bob = f.client(BOB);
    let room_ref = f.world.room_by_id(ROOM).unwrap();
    room::enter_room(&f.world, &alice.session, room_ref.clone(), None, true);
    // Bob is on his way in: the room is his, but he is not an entity of it yet.
    bob.session.player.lock().room_id = Some(ROOM);

    f.store().fail_next_commits(1);
    alice.send(
        &f.world,
        rooms_moderation::assign_rights,
        ClientBody::new().raw(b"bob"),
    );
    assert!(
        room_ref.lock().rights.is_empty(),
        "not given when not saved"
    );
    assert!(f.store().room_rights(ROOM).unwrap().is_empty());

    alice.send(
        &f.world,
        rooms_moderation::assign_rights,
        ClientBody::new().raw(b"bob"),
    );
    assert_eq!(room_ref.lock().rights, [BOB]);
    assert_eq!(f.store().room_rights(ROOM).unwrap(), [BOB]);
}

#[test]
fn racing_logins_of_one_user_leave_one_session() {
    let f = Fixture::new();
    for round in 0..20u64 {
        let sessions: Vec<_> = (0..2)
            .map(|i| {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                std::mem::forget(rx);
                Arc::new(crate::session::Session::new(1000 + round * 2 + i, tx))
            })
            .collect();
        let user = f.store().user(ALICE).unwrap().unwrap();
        std::thread::scope(|s| {
            for session in &sessions {
                let user = user.clone();
                let world = &f.world;
                s.spawn(move || crate::game::player::login(world, session, user));
            }
        });
        let live: Vec<_> = sessions
            .iter()
            .filter(|s| !s.is_closing() && s.player.lock().logged_in)
            .collect();
        assert_eq!(live.len(), 1, "round {round}");
        let online = f.world.players();
        assert_eq!(online.len(), 1, "round {round}");
        assert_eq!(online[0].id, live[0].id);
        // Log the survivor out for the next round.
        crate::game::player::dispose(&f.world, live[0]);
    }
}
