//! Unit tests of the interactors on a room without tasks (the random outcomes, the timers and the
//! cases the traces do not reach).

use std::sync::Arc;

use rusbbo_protocol::Message;
use rusbbo_protocol::frame::encode_server;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::game::item::base::ItemDefinition;
use crate::game::item::roller::tests::model_a;
use crate::game::pathfinder::Position;
use crate::game::room::Room;
use crate::game::room::entity::StatusType;
use crate::game::room::items::Item;
use crate::game::room::items::tests::{definition, flat_item, queue_tile1};
use crate::messages::outgoing::rooms_items as out;
use crate::session::{Outgoing, Session};
use crate::world::World;

fn world() -> (tempfile::TempDir, World) {
    let dir = tempfile::tempdir().unwrap();
    let store = rusbbo_storage::Store::create(&dir.path().join("t.redb")).unwrap();
    (dir, World::new(store).unwrap())
}

/// The frames queued since the last call.
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

/// A flat of user 1 on model_a with `items`.
fn flat(items: Vec<Item>) -> Room {
    Room::for_test(1, model_a(), items)
}

/// Adds user `user_id` at `at`.
fn join(r: &mut Room, user_id: u32, at: Position) -> (usize, UnboundedReceiver<Outgoing>) {
    let (e, rx) = r.add_test_user(at);
    r.entities[e].details.user_id = user_id;
    (e, rx)
}

fn def(
    sprite: &str,
    size: (i32, i32, f64),
    max_status: i32,
    behaviour: &str,
    interactor: &str,
) -> Arc<ItemDefinition> {
    definition(
        (1, sprite, 1),
        size,
        max_status,
        behaviour,
        interactor,
        "0,2,4,6",
    )
}

#[test]
fn dice_only_next_to_it_and_not_while_rolling() {
    let (_dir, world) = world();
    let dice = def(
        "edice",
        (1, 1, 0.0),
        101,
        "solid,requires_touching_for_interaction,dice",
        "default",
    );
    let mut r = flat(vec![flat_item(1, dice, (6, 5, 0))]);
    let (far, mut rx) = join(&mut r, 1, Position::new(3, 5));
    r.throw_dice(&world, far, 0);
    assert!(frames(&mut rx).is_empty());
    assert!(!r.mapping.items[0].requires_update);

    r.entities[far].position = Position::new(5, 4);
    r.throw_dice(&world, far, 0);
    assert_eq!(frames(&mut rx), enc(&[out::dice_value(1, -1)]));
    assert!(r.mapping.items[0].requires_update);
    // A visitor coming in now sees it roll.
    assert_eq!(enc(&r.busy_item_updates()), enc(&[out::dice_value(1, 0)]));
    // Rolling: neither a second throw nor DICE_OFF.
    r.throw_dice(&world, far, 0);
    r.dice_off(&world, far, 0);
    assert!(frames(&mut rx).is_empty());
    // Moved or picked up, it stops rolling (resetExtraData).
    crate::game::room::furniture::reset_extra_data(&mut r.mapping.items[0], false);
    assert!(!r.mapping.items[0].requires_update);
    r.dice_off(&world, far, 0);
    let mut off = r.mapping.items[0].clone();
    off.custom_data = "0".into();
    assert_eq!(
        frames(&mut rx),
        enc(&[out::stuff_data_update(&off), out::dice_value(1, 0)])
    );
}

#[test]
fn wheel_spins_once() {
    let (_dir, world) = world();
    let wheel = def(
        "habbowheel",
        (1, 1, 0.0),
        33,
        "wall_item,wheel_of_fortune,requires_rights_for_interaction",
        "default",
    );
    let mut r = flat(vec![flat_item(1, wheel, (0, 0, 0))]);
    let (_, mut rx) = join(&mut r, 1, Position::new(3, 5));
    r.spin_wheel(&world, 0);
    assert_eq!(r.mapping.items[0].custom_data, "-1");
    assert!(r.mapping.items[0].requires_update);
    assert_eq!(frames(&mut rx).len(), 1);
    r.spin_wheel(&world, 0);
    assert!(frames(&mut rx).is_empty());
    assert_eq!(
        enc(&r.busy_item_updates()),
        enc(&[out::stuff_data_update(&r.mapping.items[0])])
    );
}

#[test]
fn vending_machine_from_the_front_or_the_side() {
    let (_dir, world) = world();
    let sink = Arc::new(
        ItemDefinition::new(rusbbo_storage::ItemDefinition {
            id: 85,
            sprite: "sink".into(),
            length: 1,
            width: 1,
            max_status: 2,
            behaviour: "solid,requires_touching_for_interaction,custom_data_true_false".into(),
            interactor: "vending_machine".into(),
            allowed_rotations: "2,4".into(),
            drink_ids: Some("18".into()),
            ..Default::default()
        })
        .unwrap(),
    );
    // traces/flat-vending.json: the sink at (8,3) facing south, its front at (8,4).
    let mut item = flat_item(1, sink, (8, 3, 4));
    item.flat.as_mut().unwrap().record.custom_data = "0".into();
    let mut r = flat(vec![item]);
    let (e, mut rx) = join(&mut r, 1, Position::new(3, 5));
    // Far: walks to the front.
    r.vending_interact(&world, e, 0);
    assert_eq!(r.entities[e].goal, Some(Position::new(8, 4)));
    // From the side: east of it, facing north, the machine on the left.
    r.entities[e].path.clear();
    r.entities[e].is_walking = false;
    r.entities[e].position = Position::new(9, 3);
    r.entities[e].position.set_rotation(0);
    assert_eq!(r.entities[e].position.square_left(), Position::new(8, 3));
    frames(&mut rx);
    r.vending_interact(&world, e, 0);
    // Turned west to it, the machine opens.
    assert_eq!(r.entities[e].position.rotation(), 6);
    assert_eq!(r.mapping.items[0].custom_data, "1");
    // Sitting users keep their rotation.
    r.entities[e].position = Position::new(8, 4);
    r.entities[e].position.set_rotation(2);
    r.entities[e].statuses.set(StatusType::Sit, "1.0");
    r.vending_interact(&world, e, 0);
    assert_eq!(r.entities[e].position.rotation(), 2);
}

#[test]
fn scoreboard_needs_rights() {
    let (_dir, world) = world();
    let score = def(
        "hockey_score",
        (1, 1, 0.0),
        100,
        "solid,requires_touching_for_interaction,custom_data_numeric_state",
        "scoreboard",
    );
    let mut r = flat(vec![flat_item(1, score, (7, 4, 4))]);
    let (visitor, _rx) = join(&mut r, 2, Position::new(6, 4));
    let (owner, _rx2) = join(&mut r, 1, Position::new(7, 5));
    r.on_interact(&world, visitor, 0, 2);
    assert_eq!(r.mapping.items[0].custom_data, "0");
    r.on_interact(&world, owner, 0, 2);
    r.on_interact(&world, owner, 0, 2);
    assert_eq!(r.mapping.items[0].custom_data, "2");
    r.on_interact(&world, owner, 0, 1);
    assert_eq!(r.mapping.items[0].custom_data, "1");
    r.on_interact(&world, owner, 0, 0);
    assert_eq!(r.mapping.items[0].custom_data, "x");
    r.on_interact(&world, owner, 0, 1);
    assert_eq!(r.mapping.items[0].custom_data, "99");
}

#[test]
fn lert_flashes() {
    let (_dir, world) = world();
    let light = def(
        "hockey_light",
        (1, 1, 0.0),
        2,
        "solid,requires_touching_for_interaction,custom_data_true_false",
        "lert",
    );
    let mut r = flat(vec![flat_item(1, light, (9, 4, 0))]);
    let (e, mut rx) = join(&mut r, 1, Position::new(8, 4));
    r.on_interact(&world, e, 0, 0);
    let states: Vec<Vec<u8>> = ["0", "1", "0"]
        .iter()
        .map(|s| {
            let mut item = r.mapping.items[0].clone();
            item.custom_data = s.to_string();
            encode_server(&out::stuff_data_update(&item))
        })
        .collect();
    assert_eq!(frames(&mut rx), states);
}

#[test]
fn fortune_teller_and_love_randomizer() {
    let (_dir, world) = world();
    let fortune = def("fortune", (1, 1, 0.0), 9, "solid", "fortune");
    let randomizer = def(
        "val_randomizer",
        (4, 1, 1.4),
        33,
        "can_sit_on_top,requires_rights_for_interaction",
        "love_randomizer",
    );
    let mut r = flat(vec![
        flat_item(1, fortune, (8, 6, 0)),
        flat_item(2, randomizer, (5, 10, 2)),
    ]);
    for i in 0..2 {
        crate::game::room::furniture::reset_extra_data(&mut r.mapping.items[i], false);
    }
    assert_eq!(r.mapping.items[0].custom_data, "8");
    let (e, _rx) = join(&mut r, 2, Position::new(6, 4));
    r.on_interact(&world, e, 0, 0);
    assert_eq!(r.mapping.items[0].custom_data, "-1");
    // traces/flat-vending.json: the user at (6,4) turns south-east to it.
    assert_eq!(r.entities[e].position.rotation(), 3);
    assert!(r.mapping.items[0].requires_update);
    r.on_interact(&world, e, 0, 0);
    assert_eq!(r.mapping.items[0].custom_data, "-1");
    // Sitting, only the head turns.
    r.mapping.items[0].requires_update = false;
    r.mapping.items[0].custom_data = "3".into();
    r.entities[e].statuses.set(StatusType::Sit, "1.0");
    r.entities[e].position.set_rotation(2);
    r.on_interact(&world, e, 0, 0);
    assert_eq!(r.mapping.items[0].custom_data, "8");
    assert_eq!(r.entities[e].position.body_rotation, 2);
    assert_eq!(r.entities[e].position.head_rotation, 3);

    r.on_interact(&world, e, 1, 0);
    assert_eq!(r.mapping.items[1].custom_data, "0");
    assert!(r.mapping.items[1].requires_update);
    // Placed again, the spin is forgotten.
    crate::game::room::furniture::reset_extra_data(&mut r.mapping.items[1], false);
    assert!(!r.mapping.items[1].requires_update);
    // It is a seat.
    let (s, _rx) = join(&mut r, 3, Position::new(5, 11));
    r.invoke_item_for_test(s);
    assert_eq!(r.entities[s].statuses.value(StatusType::Sit), Some("1.4"));
}

#[test]
fn beds_lay_on_the_pillow() {
    let bed = def("bed_polyfon", (2, 3, 1.0), 0, "can_lay_on_top", "bed");
    let mut r = flat(vec![flat_item(1, bed, (6, 8, 0))]);
    let (e, _rx) = join(&mut r, 1, Position::new(7, 10));
    r.entities[e].dance_id = 1;
    r.invoke_item_for_test(e);
    let user = &r.entities[e];
    assert_eq!(user.position, Position::new(7, 8));
    assert_eq!(user.position.rotation(), 0);
    assert_eq!(user.statuses.value(StatusType::Lay), Some("1.0"));
    assert_eq!(user.dance_id, 0);
    assert_eq!(
        r.mapping.entire_entities(Position::new(7, 8)),
        [user.instance_id]
    );
    assert!(r.mapping.entire_entities(Position::new(7, 10)).is_empty());
    // The pillow is taken: the second user stays at the foot, standing.
    let (f, _rx) = join(&mut r, 2, Position::new(7, 9));
    r.invoke_item_for_test(f);
    assert_eq!(r.entities[f].position, Position::new(7, 9));
    assert!(!r.entities[f].statuses.contains(StatusType::Lay));
    // Walking to the foot goes to the pillow of that column.
    let (g, _rx) = join(&mut r, 3, Position::new(4, 9));
    assert!(r.walk_to(g, 6, 10));
    assert_eq!(r.entities[g].goal, Some(Position::new(6, 8)));
}

#[test]
fn per_room_limits() {
    let (_dir, world) = world();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let session = Session::new(1, tx);
    let mut items: Vec<Item> = (0..35)
        .map(|i| flat_item(i + 1, queue_tile1(), (4 + i % 8, 1 + i / 8, 2)))
        .collect();
    let roller = flat_item(100, queue_tile1(), (11, 13, 2));
    let r = flat(items.clone());
    assert!(r.has_too_many(&world, &session, false, &roller));
    assert_eq!(
        frames(&mut rx),
        enc(&[out::cannot_place_stuff_from_strip(22)])
    );
    // Club members may have 60.
    assert!(!r.has_too_many(&world, &session, true, &roller));

    let nest = def("pets0", (1, 1, 0.0), 0, "", "pet_nest");
    let nests: Vec<Item> = (0..3)
        .map(|i| flat_item(200 + i, nest.clone(), (4 + i, 10, 0)))
        .collect();
    let r = flat(nests.clone());
    assert!(r.has_too_many(&world, &session, false, &nests[0]));
    assert_eq!(
        frames(&mut rx),
        enc(&[out::cannot_place_stuff_from_strip(21)])
    );
    let r = flat(nests[..2].to_vec());
    assert!(!r.has_too_many(&world, &session, false, &nests[0]));

    let post_it = def("post.it", (0, 0, 0.0), 2, "wall_item,post_it", "default");
    let stickies: Vec<Item> = (0..25)
        .map(|i| flat_item(300 + i, post_it.clone(), (0, 0, 0)))
        .collect();
    let r = flat(stickies.clone());
    assert!(r.has_too_many(&world, &session, false, &stickies[0]));
    assert_eq!(
        frames(&mut rx),
        enc(&[out::cannot_place_stuff_from_strip(12)])
    );

    let hotspot = def(
        "ads_idol_hotspot",
        (1, 1, 0.0),
        2,
        "can_stand_on_top",
        "idol_scoreboard",
    );
    items.clear();
    items.push(flat_item(400, hotspot.clone(), (6, 5, 2)));
    let r = flat(items);
    assert!(r.has_too_many(&world, &session, false, &flat_item(401, hotspot, (7, 5, 2))));
    assert_eq!(frames(&mut rx).len(), 1);
    assert!(!r.has_too_many(&world, &session, false, &roller));
}

#[test]
fn totem_head_takes_the_legs_colour() {
    let (_dir, world) = world();
    let legs = def(
        "totem_leg",
        (1, 1, 1.3),
        12,
        "solid,can_stack_on_top",
        "totem_leg",
    );
    let head = def(
        "totem_head",
        (1, 1, 1.8),
        15,
        "solid,can_stack_on_top",
        "totem_head",
    );
    let mut l = flat_item(1, legs, (6, 6, 0));
    l.custom_data = "1".into(); // red human legs
    let mut h = flat_item(2, head.clone(), (6, 6, 0));
    h.custom_data = "1".into(); // a squid head
    h.position.z = 1.3;
    let mut r = flat(vec![l, h]);
    assert_eq!(r.item_below(1), Some(0));
    assert_eq!(r.item_above(0), Some(1));
    r.totem_head_placed(&world, 1);
    assert_eq!(r.mapping.items[1].custom_data, "8"); // red squid
    assert_eq!(r.mapping.items[0].custom_data, "0"); // grey legs
    // On the legs, neither toggles.
    r.totem_head_interact(&world, 1);
    r.totem_leg_interact(&world, 0);
    assert_eq!(
        (
            r.mapping.items[0].custom_data.as_str(),
            r.mapping.items[1].custom_data.as_str()
        ),
        ("0", "8")
    );
    // Moved off them, the red goes back to the legs.
    r.mapping.remove_item_tiles(1);
    r.mapping.items[1].position = Position::new(7, 6);
    r.mapping.add_item_tiles(1);
    r.totem_head_moved(&world, 1, false, Some(1));
    assert_eq!(r.mapping.items[1].custom_data, "1");
    assert_eq!(r.mapping.items[0].custom_data, "1");
    // A coloured head picked up loses its colour.
    let mut picked = flat_item(3, head, (0, 0, 0));
    picked.custom_data = "13".into();
    super::totem::head_picked_up(&mut picked);
    assert_eq!(picked.custom_data, "2");
}

#[test]
fn step_light_and_multi_height() {
    let (_dir, world) = world();
    let step = def(
        "step_light",
        (1, 1, 0.0),
        2,
        "can_stand_on_top",
        "step_light",
    );
    let mut multi_def = rusbbo_storage::ItemDefinition {
        id: 2,
        sprite: "multi".into(),
        length: 1,
        width: 1,
        top_height: 0.0,
        max_status: 3,
        behaviour: "can_stand_on_top".into(),
        interactor: "multi_height".into(),
        allowed_rotations: "0".into(),
        ..Default::default()
    };
    multi_def.heights = Some("0.5,1,1.5".into());
    let multi = Arc::new(ItemDefinition::new(multi_def).unwrap());
    let mut r = flat(vec![
        flat_item(1, step, (6, 6, 0)),
        flat_item(2, multi, (8, 8, 0)),
    ]);
    let (e, _rx) = join(&mut r, 1, Position::new(6, 6));
    r.on_entity_step(e, Position::new(5, 6));
    assert_eq!(r.mapping.items[0].custom_data, "1");

    let (m, _rx) = join(&mut r, 2, Position::new(8, 8));
    assert_eq!(r.mapping.items[1].total_height(), 0.5);
    r.on_interact(&world, m, 1, 0);
    assert_eq!(r.mapping.items[1].custom_data, "1");
    assert_eq!(r.mapping.items[1].total_height(), 1.0);
    // The user on it rises with it.
    assert_eq!(r.entities[m].position.z, 1.0);
}

#[test]
fn idol_performer_and_judges() {
    let hotspot = def(
        "ads_idol_hotspot",
        (1, 1, 0.0),
        2,
        "can_stand_on_top,requires_rights_for_interaction,can_not_stack_on_top",
        "idol_scoreboard",
    );
    let chair = def(
        "ads_idol_voting_ch",
        (1, 1, 1.2),
        3,
        "can_sit_on_top,requires_rights_for_interaction",
        "idol_vote_chair",
    );
    let mut items = vec![
        flat_item(1, hotspot, (6, 5, 2)),
        flat_item(2, chair, (8, 8, 2)),
    ];
    for item in &mut items {
        crate::game::room::furniture::reset_extra_data(item, false);
    }
    let mut r = flat(items);
    let (judge, mut jrx) = join(&mut r, 2, Position::new(8, 8));
    r.invoke_item_for_test(judge);
    assert!(frames(&mut jrx).contains(&encode_server(&out::judge_gui_status(1, -1))));

    let (performer, mut prx) = join(&mut r, 1, Position::new(6, 5));
    r.invoke_item_for_test(performer);
    assert_eq!(frames(&mut prx), enc(&[out::open_performer_gui(&[])]));
    assert_eq!(r.idol.performer.map(|p| p.1), Some(1));

    frames(&mut jrx);
    let performer_session = r.entities[performer].session.id;
    r.start_performance(performer_session);
    let jf = frames(&mut jrx);
    assert_eq!(jf[0], encode_server(&out::judge_gui_status(2, 1)));
    assert_eq!(r.mapping.items[0].custom_data, "0");

    r.vote_performance(judge, false);
    // Down from 0 stays 0; the only judge voted, so it stops.
    assert_eq!(r.mapping.items[1].custom_data, "2");
    assert_eq!(r.mapping.items[0].custom_data, "-1");
    assert_eq!(r.idol.voted, [r.entities[judge].session.id]);
}
