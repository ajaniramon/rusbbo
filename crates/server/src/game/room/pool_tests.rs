//! Unit tests of the Habbo Lido (`pool.rs`) and of avatar effects in a room, on rooms without
//! tasks: what the traces cannot reach (no tickets, the totem's and a purchase's stored effect,
//! effects expiring) and the pieces the traces build on.

use std::sync::Arc;

use rusbbo_protocol::Message;
use rusbbo_protocol::frame::encode_server;
use tokio::sync::mpsc::UnboundedReceiver;

use super::Room;
use super::entity::StatusType;
use super::items::{Interaction, Item, public_items};
use super::model::RoomModel;
use crate::game::pathfinder::Position;
use crate::messages::outgoing::{effects as out_effects, rooms_pool as out};
use crate::session::{Outgoing, Session};
use crate::world::World;

fn world() -> (tempfile::TempDir, Arc<World>) {
    let dir = tempfile::tempdir().unwrap();
    let store = rusbbo_storage::Store::create(&dir.path().join("t.redb")).unwrap();
    (dir, World::new(store).unwrap().into_arc())
}

fn frames(rx: &mut UnboundedReceiver<Outgoing>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Ok(Outgoing::Frame(f)) = rx.try_recv() {
        out.push(f);
    }
    out
}

fn enc(messages: &[Message]) -> Vec<Vec<u8>> {
    messages.iter().map(encode_server).collect()
}

/// A 30x30 public room of `model` at height 7 with water (height 0) from y = 15 down.
fn pool_room(
    world: &Arc<World>,
    model: &str,
    trigger: &str,
    rows: &[rusbbo_storage::PublicItem],
) -> Room {
    let line = |c: char| std::iter::repeat_n(c, 30).collect::<String>();
    let heightmap: Vec<String> = (0..30)
        .map(|y| if y < 15 { line('7') } else { line('0') })
        .collect();
    let model = Arc::new(RoomModel::parse(&rusbbo_storage::RoomModel {
        model_id: model.into(),
        model_name: model.into(),
        door_x: 2,
        door_y: 2,
        door_z: 7.0,
        heightmap: heightmap.join("|"),
        trigger_class: trigger.into(),
        ..Default::default()
    }));
    let items: Vec<Item> = public_items(&model.id, rows, &|_| None);
    let mut r = Room::for_test(0, model, items);
    r.world = Arc::downgrade(world);
    r
}

fn row(
    sprite: &str,
    (x, y, z): (i32, i32, f64),
    rotation: i32,
    program: &str,
) -> rusbbo_storage::PublicItem {
    rusbbo_storage::PublicItem {
        sprite: Some(sprite.into()),
        x,
        y,
        z,
        rotation,
        top_height: 0.001,
        length: 1,
        width: 1,
        behaviour: "can_stand_on_top,invisible".into(),
        current_program: Some(program.into()),
        ..Default::default()
    }
}

fn swimmer(r: &mut Room, at: Position, tickets: i32) -> (usize, UnboundedReceiver<Outgoing>) {
    let (e, rx) = r.add_test_user(at);
    let d = &mut r.entities[e].details;
    d.user_id = 1;
    d.name = "swimmer".into();
    d.pool_figure = "ch=s02/238,238,238".into();
    d.tickets = tickets;
    (e, rx)
}

#[test]
fn ladder_targets_are_space_separated() {
    let mut ladder = row("poolEnter", (17, 21, 7.0), 0, "Splash0");
    ladder.teleport_to = Some("17 22 3 4".into());
    ladder.swim_to = Some("17 23 3 4".into());
    let items = public_items("pool_b", &[ladder], &|_| None);
    assert_eq!(items[0].interaction, Interaction::PoolLadder);
    let to = items[0].teleport_to.unwrap();
    assert_eq!((to.x, to.y, to.z, to.body_rotation), (17, 22, 3.0, 4));
    assert_eq!(items[0].swim_to.unwrap(), Position::new(17, 23));
}

#[test]
fn a_booth_closes_on_its_user_and_opens_when_they_leave() {
    let (_dir, world) = world();
    let rows = [
        row("poolBooth", (17, 11, 7.0), 0, "curtains1"),
        row("poolBooth", (17, 9, 7.0), 0, "curtains2"),
    ];
    let mut r = pool_room(&world, "pool_a", "habbo_lido_trigger", &rows);
    // The booth also owns the tile it is entered from.
    assert_eq!(
        r.mapping
            .highest_item(Position::new(18, 11))
            .unwrap()
            .sprite,
        "poolBooth"
    );
    assert_eq!(r.mapping.items[0].current_program_value, "open");
    let program = r.program_of(0).unwrap();
    assert_eq!(program.body, b"curtains1 open");

    let (e, mut rx) = swimmer(&mut r, Position::new(17, 11), 10);
    r.invoke_item_for_test(e);
    assert!(!r.entities[e].walking_allowed);
    assert_eq!(
        frames(&mut rx),
        enc(&[
            out::open_uimakoppi(),
            out::show_program(&["curtains1", "close"])
        ])
    );

    r.exit_booth(&world, e);
    assert!(r.entities[e].walking_allowed);
    assert!(r.entities[e].is_walking);
    assert_eq!(r.entities[e].goal, Some(Position::new(19, 11)));
    assert_eq!(
        frames(&mut rx),
        enc(&[out::show_program(&["curtains1", "open"])])
    );
}

#[test]
fn the_queue_sends_back_those_without_tickets_or_swimwear() {
    let (_dir, world) = world();
    let rows = [row("queue_tile2", (5, 5, 7.0), 0, "")];
    let mut r = pool_room(&world, "pool_b", "diving_deck_trigger", &rows);
    let (e, mut rx) = swimmer(&mut r, Position::new(5, 5), 0);
    r.entities[e].is_walking = true;
    let mut old = Position::new(5, 6);
    old.z = 7.0;
    r.on_entity_step(e, old);
    let p = r.entities[e].position;
    assert_eq!((p.x, p.y, p.body_rotation, p.head_rotation), (5, 6, 2, 2));
    assert!(!r.entities[e].is_walking);
    assert_eq!(frames(&mut rx), enc(&[out::no_tickets()]));

    r.entities[e].details.tickets = 3;
    r.entities[e].details.pool_figure.clear();
    r.entities[e].position = Position::new(5, 5);
    r.on_entity_step(e, old);
    let msgs = frames(&mut rx);
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].windows(9).any(|w| w == b"Necesitas"));
}

#[test]
fn the_lift_takes_a_ticket_and_the_splash_lands_in_the_water() {
    let (_dir, world) = world();
    let rows = [
        row("poolLift", (26, 4, 7.0), 0, "door"),
        row("queue_tile2", (26, 5, 7.0), 0, ""),
    ];
    let mut r = pool_room(&world, "pool_b", "diving_deck_trigger", &rows);
    let (e, mut rx) = swimmer(&mut r, Position::new(26, 5), 1);
    // Standing on the queue: the status tick walks the swimmer on to the lift.
    r.process_pool_queue(Some(&world), e);
    assert_eq!(r.entities[e].goal, Some(Position::new(26, 4)));

    r.entities[e].position = Position::new(26, 4);
    r.invoke_item_for_test(e);
    assert!(r.entities[e].is_diving);
    assert_eq!(r.entities[e].details.tickets, 0);
    assert_eq!(
        frames(&mut rx),
        enc(&[
            out::show_program(&["door", "close"]),
            out::ticket_balance(0),
            out::jumpingplace_ok(),
        ])
    );
    // Nothing is taken from an empty balance.
    r.invoke_item_for_test(e);
    assert_eq!(r.entities[e].details.tickets, 0);
    frames(&mut rx);

    r.dive(e, b"dive");
    assert_eq!(frames(&mut rx), enc(&[out::jumpdata(0, b"dive")]));

    r.splash_position(&world, e);
    let p = r.entities[e].position;
    assert_eq!((p.x, p.y, p.z), (23, 19, 0.0));
    assert!(r.entities[e].statuses.contains(StatusType::Swim));
    assert!(!r.entities[e].is_diving && !r.entities[e].walking_allowed);
    assert_eq!(r.entities[e].goal, Some(Position::new(20, 19)));
    assert_eq!(r.mapping.items[0].current_program_value, "open");
    // No longer diving: DIVE is ignored.
    frames(&mut rx);
    r.dive(e, b"dive");
    assert!(frames(&mut rx).is_empty());
}

#[test]
fn signs_vote_from_zero_to_seven() {
    let (_dir, world) = world();
    let mut r = pool_room(&world, "pool_b", "diving_deck_trigger", &[]);
    let (e, _rx) = swimmer(&mut r, Position::new(5, 5), 1);
    r.hold_sign(&world, e, "9");
    assert_eq!(r.entities[e].lido_vote, 0);
    assert_eq!(r.entities[e].statuses.value(StatusType::Sign), Some("9"));
    r.hold_sign(&world, e, "x");
    r.hold_sign(&world, e, "99999999999");
    assert_eq!(r.entities[e].statuses.value(StatusType::Sign), Some("9"));
    r.hold_sign(&world, e, "7");
    assert_eq!(r.entities[e].lido_vote, 10);
    r.hold_sign(&world, e, "0");
    assert_eq!(r.entities[e].lido_vote, 3);
}

#[test]
fn arriving_in_the_water_swims() {
    let (_dir, world) = world();
    let mut r = pool_room(&world, "pool_a", "habbo_lido_trigger", &[]);
    let mut at = Position::new(20, 20);
    at.z = 1.0;
    let (e, _rx) = swimmer(&mut r, at, 1);
    let session = r.entities[e].session.id;
    r.try_room_entry(&world, session);
    assert!(r.entities[e].statuses.contains(StatusType::Swim));
}

#[test]
fn effects_stop_drinks_and_drinks_stop_effects() {
    let (_dir, world) = world();
    let mut r = pool_room(&world, "pool_a", "habbo_lido_trigger", &[]);
    let (e, mut rx) = swimmer(&mut r, Position::new(5, 5), 1);
    r.use_effect(e, 4);
    assert_eq!(r.entities[e].session.effect_id(), 4);
    assert_eq!(
        frames(&mut rx),
        enc(&[out_effects::user_avatar_effect(0, 4)])
    );
    r.carry_item(&world, e, 5, None);
    assert_eq!(r.entities[e].effect_id, 0);
    assert!(r.is_carrying(e));
    assert_eq!(
        frames(&mut rx)[0],
        encode_server(&out_effects::user_avatar_effect(0, 0))
    );
    r.use_effect(e, 4);
    assert!(!r.is_carrying(e));
    // Sitting shows the effect again after the next tick.
    r.sit_here(e);
    assert_eq!(r.queue_after_loop.len(), 1);
    r.entity_tick(&world);
    assert!(r.queue_after_loop.is_empty());
    let last = frames(&mut rx).pop().unwrap();
    assert_eq!(last, encode_server(&out_effects::user_avatar_effect(0, 4)));
}

fn logged_in(world: &World, user_id: u32) -> (Arc<Session>, UnboundedReceiver<Outgoing>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let session = Arc::new(Session::new(u64::from(user_id), tx));
    let user = rusbbo_storage::User {
        id: user_id,
        username: format!("user{user_id}"),
        ..Default::default()
    };
    world.store.put_user(&user).unwrap();
    {
        let mut p = session.player.lock();
        p.details = Some(user);
        p.logged_in = true;
    }
    (session, rx)
}

#[test]
fn activated_effects_expire() {
    let (_dir, world) = world();
    let (session, mut rx) = logged_in(&world, 7);
    let effect = |id, effect_id, expiry_date, activated| rusbbo_storage::Effect {
        id,
        effect_id,
        expiry_date,
        activated,
    };
    let stored = vec![
        effect(1, 2, 10, true),
        effect(2, 3, -1, false),
        effect(3, 9, -1, false),
    ];
    world.store.put_effects(7, &stored).unwrap();
    // The player has the first two in memory; the third was stored since (a purchase).
    session.player.lock().effects = stored[..2].to_vec();
    crate::game::effects::expire(&world, &session);
    assert_eq!(
        frames(&mut rx),
        enc(&[out_effects::avatar_effect_expired(2)])
    );
    assert_eq!(session.player.lock().effects, vec![effect(2, 3, -1, false)]);
    assert_eq!(
        world.store.effects(7).unwrap(),
        vec![effect(2, 3, -1, false), effect(3, 9, -1, false)]
    );

    // Activating starts the effect's time (two hours without a settings_effects row).
    {
        let mut p = session.player.lock();
        crate::game::effects::activate(&world, &session, &mut p, 3);
        assert!(p.effects[0].activated);
        assert!(p.effects[0].expiry_date >= crate::game::now() + 7199);
    }
    assert_eq!(frames(&mut rx).len(), 1);
    let stored = world.store.effects(7).unwrap();
    assert!(stored[0].activated && !stored[1].activated);
}

#[test]
fn bought_and_totem_effects_are_only_stored() {
    let (_dir, world) = world();
    let (session, mut rx) = logged_in(&world, 8);
    let mut w = world.store.write().unwrap();
    crate::game::effects::purchase_in(&mut w, 8, 5).unwrap();
    w.commit().unwrap();
    crate::game::effects::totem_effect(&world, &session, 23);
    crate::game::effects::totem_effect(&world, &session, 23);
    let stored: Vec<(u32, i32, bool)> = world
        .store
        .effects(8)
        .unwrap()
        .iter()
        .map(|e| (e.id, e.effect_id, e.activated))
        .collect();
    // Java's newEffect returns null: nothing reaches the player, and each use stores one more.
    assert_eq!(stored, vec![(1, 5, false), (2, 23, false), (3, 23, false)]);
    assert!(session.player.lock().effects.is_empty());
    assert!(frames(&mut rx).is_empty());

    // An owned totem effect (loaded at login) gives none.
    session.player.lock().effects = world.store.effects(8).unwrap();
    crate::game::effects::totem_effect(&world, &session, 23);
    assert_eq!(world.store.effects(8).unwrap().len(), 3);
}
