//! Java: `messages/incoming/trade`.
//!
//! The state lives in the room, so most handlers only lock the room. `TRADE_ADDITEM` also needs
//! the hand: it locks the player first and the room second (the lock order).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use crate::game::room::trade::show_trade_ban_alert;
use crate::game::room::{Room, current_room, run_deferred};
use crate::messages::Malformed;
use crate::messages::outgoing::{alerts, trade as out};
use crate::session::Session;
use crate::world::World;

/// The room, the user's entity and the partner's entity, if the user is in a trade.
fn in_trade(r: &Room, session: &Session) -> Option<(usize, usize)> {
    let idx = r.entity_index(session.id)?;
    Some((idx, r.trade_partner(idx)?))
}

/// Runs `action` on a user who is in a trade with trading enabled on both sides (the guard most
/// handlers start with), then what the room left to do.
fn with_trade(world: &World, session: &Arc<Session>, action: impl FnOnce(&mut Room, usize, usize)) {
    let Some(room) = current_room(world, session) else {
        return;
    };
    let deferred = {
        let mut r = room.lock();
        let Some((idx, partner)) = in_trade(&r, session) else {
            return;
        };
        if !r.trade_enabled(idx, partner) {
            return;
        }
        action(&mut r, idx, partner);
        r.take_deferred()
    };
    run_deferred(world, deferred);
}

/// Java: `TRADE_OPEN`: the instance id of the user to trade with.
pub fn trade_open(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let deferred = {
        let mut r = room.lock();
        if !allows_trading(world, &r) {
            return Ok(());
        }
        let Some(idx) = r.entity_index(session.id) else {
            return Ok(());
        };
        if r.entities[idx].trade.partner.is_some() {
            return Ok(());
        }
        let details = &r.entities[idx].details;
        if details.is_trade_banned() {
            session.send(alerts::alert(&show_trade_ban_alert(
                details.trade_ban_expiration,
            )));
            return Ok(());
        }
        let instance_id = reader.int().ok_or(Malformed)?;
        // Java casts the entity to a Player: with a bot or a pet it throws and nothing happens.
        let Some(partner) = r
            .entity_by_instance(instance_id)
            .filter(|&p| r.entities[p].is_player())
        else {
            return Ok(());
        };
        // Java lets a user open a trade with themselves, which leaves them in a broken state.
        if partner == idx {
            return Ok(());
        }
        if r.entities[partner].details.is_trade_banned() {
            session.send(alerts::alert("You cannot trade with this user"));
            return Ok(());
        }
        if r.entities[partner].trade.partner.is_some() {
            session.send(out::trade_already_open());
            return Ok(());
        }
        if crate::game::maintenance::tell_closing(session) {
            return Ok(());
        }
        r.trade_open(idx, partner);
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `room.getCategory().hasAllowTrading()`; a room without a category throws in Java and the
/// request is dropped.
fn allows_trading(world: &World, r: &Room) -> bool {
    u32::try_from(r.data.category)
        .ok()
        .and_then(|id| world.room_categories().get(&id).cloned())
        .is_some_and(|c| c.allow_trading)
}

/// Java: `TRADE_ADDITEM`: the id of an item of the hand.
pub fn trade_add_item(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut p = session.player.lock();
    let deferred = {
        let mut r = room.lock();
        let Some((idx, partner)) = in_trade(&r, session) else {
            return Ok(());
        };
        if !r.trade_enabled(idx, partner) {
            return Ok(());
        }
        let (mine, theirs) = (&r.entities[idx].trade, &r.entities[partner].trade);
        if mine.confirmed && theirs.confirmed {
            return Ok(());
        }
        if mine.accept && theirs.accept {
            return Ok(());
        }
        // Offers are locked once both sides accepted and the trade is waiting for confirmation.
        if mine.can_confirm || theirs.can_confirm {
            return Ok(());
        }
        let item_id = reader.int().ok_or(Malformed)?;
        // Not in the hand, hidden, or already in a trade window (the offered ones are flagged).
        let Some(item) = p
            .inventory
            .item(item_id)
            .filter(|i| i.is_visible())
            .cloned()
        else {
            return Ok(());
        };
        if !item.definition.record.is_tradable {
            session.send(out::item_not_tradable());
            return Ok(());
        }
        p.inventory
            .set_trading(&world.store, &[item.record.id], true);
        r.trade_add_item(idx, partner, item);
        r.take_deferred()
    };
    drop(p);
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `TRADE_REMOVE_ITEM`: the id of an item of the hand.
pub fn trade_remove_item(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut p = session.player.lock();
    let mut r = room.lock();
    let Some((idx, partner)) = in_trade(&r, session) else {
        return Ok(());
    };
    if !r.trade_enabled(idx, partner) {
        return Ok(());
    }
    let (mine, theirs) = (&r.entities[idx].trade, &r.entities[partner].trade);
    if (mine.confirmed && theirs.confirmed)
        || (mine.accept && theirs.accept)
        || mine.can_confirm
        || theirs.can_confirm
    {
        return Ok(());
    }
    let item_id = reader.int().ok_or(Malformed)?;
    let Some(item) = p.inventory.item(item_id) else {
        return Ok(());
    };
    if !item.definition.record.is_tradable {
        session.send(out::item_not_tradable());
        return Ok(());
    }
    let record_id = item.record.id;
    p.inventory.set_trading(&world.store, &[record_id], false);
    r.trade_remove_item(idx, partner, record_id);
    Ok(())
}

/// Java: `TRADE_ACCEPT`.
pub fn trade_accept(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    with_trade(world, session, |r, idx, partner| {
        r.trade_accept(idx, partner)
    });
    Ok(())
}

/// Java: `TRADE_UNACCEPT` (ids 68 and 403).
pub fn trade_unaccept(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    with_trade(world, session, |r, idx, partner| {
        r.trade_unaccept(idx, partner)
    });
    Ok(())
}

/// Java: `TRADE_CONFIRM_ACCEPT`.
pub fn trade_confirm_accept(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    with_trade(world, session, |r, idx, partner| {
        if r.entities[idx].trade.can_confirm && r.entities[partner].trade.can_confirm {
            r.trade_confirm(idx, partner);
        }
    });
    Ok(())
}

/// Java: `TRADE_CLOSE`: works even when trading is disabled.
pub fn trade_close(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let deferred = {
        let mut r = room.lock();
        let Some((idx, _)) = in_trade(&r, session) else {
            return Ok(());
        };
        r.close_trade(idx);
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}
