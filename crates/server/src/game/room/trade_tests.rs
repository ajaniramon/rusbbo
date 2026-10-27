//! Unit tests of the trade state (see [`super::trade`]).

use std::sync::Arc;

use havana_protocol::Message;
use havana_protocol::frame::encode_server;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::entity::{EntityDetails, RoomEntity, StatusType};
use super::model::RoomModel;
use super::trade::{TradeSide, run_closed, run_finished};
use super::{Deferred, Room};
use crate::messages::outgoing::rooms_user as out_user;
use crate::messages::outgoing::trade as out;
use crate::messages::outgoing::trade::tests::item;
use crate::session::{Outgoing, Session};
use crate::world::World;

fn world() -> (tempfile::TempDir, World) {
    let dir = tempfile::tempdir().unwrap();
    let store = havana_storage::Store::create(&dir.path().join("t.redb")).unwrap();
    (dir, World::new(store).unwrap())
}

fn room() -> Room {
    let model = Arc::new(RoomModel::parse(&havana_storage::RoomModel {
        model_id: "m".into(),
        model_name: "m".into(),
        door_x: 0,
        door_y: 0,
        heightmap: "000|000|000".into(),
        ..Default::default()
    }));
    Room {
        data: havana_storage::Room {
            id: 1000,
            ..Default::default()
        },
        mapping: super::mapping::RoomMapping::new(model.clone(), false),
        model,
        rights: Vec::new(),
        votes: Vec::new(),
        first_tick: None,
        entities: Vec::new(),
        active: true,
        counter: 0,
        task: None,
        dispose_generation: 0,
        self_ref: std::sync::Weak::new(),
        deferred: Vec::new(),
    }
}

struct User {
    session: Arc<Session>,
    rx: UnboundedReceiver<Outgoing>,
}

impl User {
    /// The frames queued since the last call.
    fn frames(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(Outgoing::Frame(f)) = self.rx.try_recv() {
            out.push(f);
        }
        out
    }
}

fn join(room: &mut Room, id: u32, trade_enabled: bool) -> (usize, User) {
    let (tx, rx) = unbounded_channel();
    let session = Arc::new(Session::new(u64::from(id), tx));
    let details = EntityDetails {
        user_id: id,
        name: format!("bot{id}"),
        trade_enabled,
        ..Default::default()
    };
    let instance = room.counter;
    room.counter += 1;
    room.entities
        .push(RoomEntity::new(session.clone(), details, instance));
    (room.entities.len() - 1, User { session, rx })
}

fn enc(messages: &[Message]) -> Vec<Vec<u8>> {
    messages.iter().map(encode_server).collect()
}

#[test]
fn open_starts_the_trade_if_one_side_has_trading_on() {
    let mut r = room();
    let (a, mut ua) = join(&mut r, 1, false);
    let (e, mut ue) = join(&mut r, 5, true);
    r.trade_open(a, e);
    assert_eq!(ua.frames(), enc(&[out::trade_open(1, false, 5, true)]));
    assert_eq!(ue.frames(), enc(&[out::trade_open(5, true, 1, false)]));
    assert_eq!(r.trade_partner(a), Some(e));
    assert_eq!(r.trade_partner(e), Some(a));
    assert!(r.entities[a].statuses.contains(StatusType::Trade));
    // One side disabled: the handlers ignore accept, add and confirm.
    assert!(!r.trade_enabled(a, e));
}

#[test]
fn open_with_both_disabled_sends_only_tradeopen() {
    let mut r = room();
    let (a, mut ua) = join(&mut r, 1, false);
    let (b, _ub) = join(&mut r, 2, false);
    r.trade_open(a, b);
    assert_eq!(ua.frames(), enc(&[out::trade_open(1, false, 2, false)]));
    assert_eq!(r.trade_partner(a), None);
    assert!(!r.entities[a].statuses.contains(StatusType::Trade));
}

#[test]
fn accept_confirm_and_swap() {
    let mut r = room();
    let (e, mut ue) = join(&mut r, 5, true);
    let (f, mut uf) = join(&mut r, 6, true);
    r.trade_open(e, f);
    ue.frames();
    uf.frames();

    let chair = item(1, 371, "can_sit_on_top", "0");
    r.trade_add_item(e, f, chair.clone());
    assert_eq!(
        ue.frames(),
        enc(&[out::trade_items(5, std::slice::from_ref(&chair), 6, &[])])
    );
    assert_eq!(
        uf.frames(),
        enc(&[out::trade_items(6, &[], 5, std::slice::from_ref(&chair))])
    );

    // An accept is taken back by TRADE_UNACCEPT, and by a new offer without a message.
    r.trade_accept(e, f);
    assert!(r.entities[e].trade.accept);
    r.trade_unaccept(e, f);
    assert!(!r.entities[e].trade.accept);
    assert_eq!(
        ue.frames(),
        enc(&[out::trade_accept(5, true), out::trade_accept(5, false)])
    );
    uf.frames();
    r.trade_accept(e, f);
    ue.frames();
    uf.frames();
    r.trade_add_item(e, f, item(2, 330, "can_sit_on_top", "0"));
    assert!(!r.entities[e].trade.accept);
    r.trade_remove_item(e, f, 2);
    assert_eq!(r.entities[e].trade.items.len(), 1);
    ue.frames();
    uf.frames();

    // Both accepted: confirmation starts and the accepts reset.
    r.trade_accept(f, e);
    r.trade_accept(e, f);
    assert!(r.entities[e].trade.can_confirm && r.entities[f].trade.can_confirm);
    assert!(!r.entities[e].trade.accept && !r.entities[f].trade.accept);
    assert_eq!(
        uf.frames(),
        enc(&[
            out::trade_accept(6, true),
            out::trade_accept(5, true),
            out::trade_confirm()
        ])
    );
    ue.frames();

    // Completed once both confirmed; the items move after the lock is released.
    r.trade_confirm(e, f);
    assert!(r.deferred.is_empty());
    r.trade_confirm(f, e);
    assert_eq!(ue.frames(), enc(&[out::trade_completed()]));
    assert!(matches!(
        r.deferred.as_slice(),
        [Deferred::TradeFinished { .. }]
    ));
    assert_eq!(r.trade_partner(e), None);
    assert!(!r.entities[e].statuses.contains(StatusType::Trade));
    assert!(r.entities[f].needs_update);
}

#[test]
fn removing_an_item_undoes_the_accepts() {
    let mut r = room();
    let (e, mut ue) = join(&mut r, 5, true);
    let (f, mut uf) = join(&mut r, 6, true);
    r.trade_open(e, f);
    r.trade_add_item(e, f, item(1, 371, "can_sit_on_top", "0"));
    r.trade_accept(f, e);
    ue.frames();
    uf.frames();
    r.trade_remove_item(e, f, 1);
    assert_eq!(
        ue.frames(),
        enc(&[
            out::trade_accept(6, false),
            out::trade_items(5, &[], 6, &[])
        ])
    );
    assert!(!r.entities[f].trade.accept);
}

#[test]
fn unaccept_while_confirming_closes() {
    let mut r = room();
    let (e, mut ue) = join(&mut r, 5, true);
    let (f, mut uf) = join(&mut r, 6, true);
    r.trade_open(e, f);
    r.trade_accept(e, f);
    r.trade_accept(f, e);
    ue.frames();
    uf.frames();
    r.trade_unaccept(e, f);
    assert_eq!(ue.frames(), enc(&[out::trade_close(5)]));
    assert_eq!(uf.frames(), enc(&[out::trade_close(5)]));
    assert!(matches!(
        r.deferred.as_slice(),
        [Deferred::TradeClosed { .. }]
    ));
}

#[test]
fn leaving_cancels_the_trade_and_frees_the_partner() {
    let (_dir, world) = world();
    let mut r = room();
    let (e, mut ue) = join(&mut r, 5, true);
    let (f, mut uf) = join(&mut r, 6, true);
    r.trade_open(e, f);
    r.trade_add_item(e, f, item(1, 371, "can_sit_on_top", "0"));
    ue.frames();
    uf.frames();

    let left = r.remove_entity(&world, ue.session.id, false).unwrap();
    // The partner sees LOGOUT, then TRADE_CLOSE with the leaver's id; the leaver gets it too.
    assert_eq!(
        uf.frames(),
        enc(&[out_user::logout(0), out::trade_close(5)])
    );
    assert_eq!(ue.frames(), enc(&[out::trade_close(5)]));
    let f = r.entity_index(uf.session.id).unwrap();
    assert_eq!(r.trade_partner(f), None);
    assert!(!r.entities[f].statuses.contains(StatusType::Trade));
    assert!(r.entities[f].needs_update);
    assert!(left.trade.items.is_empty() && left.trade.partner.is_none());
    let Some(Deferred::TradeClosed { sides }) = r.deferred.first() else {
        panic!("the hands are not refreshed");
    };
    assert_eq!(sides[0].user_id, 5);
    assert_eq!(sides[1].user_id, 6);
}

#[test]
fn leaving_without_a_trade_sends_nothing_more() {
    let (_dir, world) = world();
    let mut r = room();
    let (_e, mut ue) = join(&mut r, 5, true);
    let (_f, mut uf) = join(&mut r, 6, true);
    r.remove_entity(&world, ue.session.id, false);
    assert_eq!(uf.frames(), enc(&[out_user::logout(0)]));
    assert!(ue.frames().is_empty());
    assert!(r.deferred.is_empty());
}

#[test]
fn hands_cancel_and_swap() {
    let (_dir, world) = world();
    let mut r = room();
    let (_e, mut ue) = join(&mut r, 5, true);
    let (_f, uf) = join(&mut r, 6, true);
    let mut chair = item(1, 371, "can_sit_on_top", "0");
    chair.record.user_id = Some(5);
    let mut bench = item(3, 323, "can_sit_on_top", "0");
    bench.record.user_id = Some(6);
    for (u, i) in [(&ue, &chair), (&uf, &bench)] {
        let mut p = u.session.player.lock();
        p.inventory.add_item(i.clone());
        p.inventory.set_trading(&world.store, &[i.record.id], true);
    }
    let side = |u: &User, id: u32, items: &[u64]| TradeSide {
        session: u.session.clone(),
        user_id: id,
        items: items.to_vec(),
    };

    // Cancelled: the hand goes out without the offered item, which is released after.
    run_closed(&world, [side(&ue, 5, &[1]), side(&uf, 6, &[3])]);
    let sent = ue.frames();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0],
        encode_server(&Message::new(140, b"HH".to_vec())),
        "the offered chair is not in the hand"
    );
    for (u, id) in [(&ue, 1), (&uf, 3)] {
        let p = u.session.player.lock();
        assert!(!p.inventory.item(id).unwrap().record.is_trading);
    }

    // Completed: they swap and own each other's item.
    for (u, i) in [(&ue, &chair), (&uf, &bench)] {
        u.session
            .player
            .lock()
            .inventory
            .set_trading(&world.store, &[i.record.id], true);
    }
    run_finished(&world, [side(&ue, 5, &[1]), side(&uf, 6, &[3])]);
    let p5 = ue.session.player.lock();
    let p6 = uf.session.player.lock();
    assert!(p5.inventory.item(1).is_none() && p6.inventory.item(3).is_none());
    let got = p5.inventory.item(3).unwrap();
    assert_eq!(got.record.user_id, Some(5));
    assert!(!got.record.is_trading);
    assert_eq!(p6.inventory.item(1).unwrap().record.user_id, Some(6));
}
