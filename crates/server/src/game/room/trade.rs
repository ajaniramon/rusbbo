//! Trading between two users in a room (Java: `game/room/managers/RoomTradeManager` and the
//! trade fields of `RoomPlayer`).
//!
//! The state of a trade lives on the two [`RoomEntity`]s, behind the room's lock. The offered
//! items are snapshots; the real items stay in the owners' hands (flagged `is_trading`) until the
//! trade completes. Whatever needs a player's lock (the hands) is left to run after the room's
//! lock is released, as [`Deferred`] actions: never two player locks at once, and each hand is
//! locked on its own.

use std::sync::Arc;

use tracing::error;

use crate::game::item::Item;
use crate::game::now;
use crate::game::room::entity::{RoomEntity, StatusType};
use crate::game::room::{Deferred, Room};
use crate::messages::outgoing::{inventory as out_inventory, trade as out};
use crate::session::Session;
use crate::world::World;

/// Java: the trade fields of `RoomPlayer`.
#[derive(Default)]
pub struct TradeState {
    /// Session id of the partner (Java: `tradePartner`).
    pub partner: Option<u64>,
    /// Java: `tradeItems`, snapshots of the offered items.
    pub items: Vec<Item>,
    pub confirmed: bool,
    pub can_confirm: bool,
    pub accept: bool,
}

/// One side of a finished or cancelled trade, for the work on its hand.
pub struct TradeSide {
    pub session: Arc<Session>,
    pub user_id: u32,
    /// Row ids of the items it offered.
    pub items: Vec<u64>,
}

/// Java: `RoomTradeManager.showTradeBanAlert`.
pub fn show_trade_ban_alert(expiration: i64) -> String {
    let uptime = (expiration - now()) * 1000;
    let days = uptime / (1000 * 60 * 60 * 24);
    let hours = (uptime - days * (1000 * 60 * 60 * 24)) / (1000 * 60 * 60);
    format!(
        "You are temporarily banned from trading.<br>The ban will expire in {days} day(s) and \
         {hours} hours(s)"
    )
}

impl RoomEntity {
    /// Java: `RoomTradeManager.reset`: forgets the trade and its status. Returns the old state.
    fn reset_trade(&mut self) -> TradeState {
        self.statuses.remove(StatusType::Trade);
        self.needs_update = true;
        std::mem::take(&mut self.trade)
    }

    fn trade_side(&self, items: &[Item]) -> TradeSide {
        TradeSide {
            session: self.session.clone(),
            user_id: self.details.user_id,
            items: items.iter().map(|i| i.record.id).collect(),
        }
    }
}

impl Room {
    /// The entity the user at `idx` trades with.
    pub fn trade_partner(&self, idx: usize) -> Option<usize> {
        let partner = self.entities[idx].trade.partner?;
        self.entity_index(partner)
    }

    /// Whether both traders have trading enabled; the trade ignores everything else if not.
    pub fn trade_enabled(&self, idx: usize, partner: usize) -> bool {
        self.entities[idx].details.trade_enabled && self.entities[partner].details.trade_enabled
    }

    /// Java: `TRADE_OPEN` once the checks passed. Both get `TRADEOPEN`; the trade starts unless
    /// both have trading disabled.
    pub fn trade_open(&mut self, idx: usize, partner: usize) {
        // Java closes both first: neither has a partner, so this only resets them.
        self.close_trade(idx);
        self.close_trade(partner);
        let (a, b) = (&self.entities[idx], &self.entities[partner]);
        let (a_id, a_on) = (a.details.user_id, a.details.trade_enabled);
        let (b_id, b_on) = (b.details.user_id, b.details.trade_enabled);
        a.session.send(out::trade_open(a_id, a_on, b_id, b_on));
        b.session.send(out::trade_open(b_id, b_on, a_id, a_on));
        if !a_on && !b_on {
            return;
        }
        let (a_sid, b_sid) = (a.session.id, b.session.id);
        for (i, other) in [(idx, b_sid), (partner, a_sid)] {
            let e = &mut self.entities[i];
            e.statuses.set(StatusType::Trade, "");
            e.needs_update = true;
            e.trade.partner = Some(other);
        }
    }

    /// Java: `RoomTradeManager.refreshWindow`: `TRADE_ITEMS` to the user at `idx`.
    fn refresh_window(&self, idx: usize, partner: usize) {
        let (a, b) = (&self.entities[idx], &self.entities[partner]);
        a.session.send(out::trade_items(
            a.details.user_id,
            &a.trade.items,
            b.details.user_id,
            &b.trade.items,
        ));
    }

    fn refresh_windows(&self, idx: usize, partner: usize) {
        self.refresh_window(idx, partner);
        self.refresh_window(partner, idx);
    }

    /// Java: `TRADE_ADDITEM` once the item passed its checks: puts it in the offer and resets
    /// the accepts. The caller flags the item in the hand.
    pub fn trade_add_item(&mut self, idx: usize, partner: usize, item: Item) {
        self.entities[idx].trade.accept = false;
        self.entities[partner].trade.accept = false;
        self.entities[idx].trade.items.push(item);
        self.refresh_windows(idx, partner);
    }

    /// Java: `TRADE_REMOVE_ITEM` after the checks: takes the item out of the offer, and tells both
    /// sides about each accept it undoes.
    pub fn trade_remove_item(&mut self, idx: usize, partner: usize, record_id: u64) {
        self.entities[idx]
            .trade
            .items
            .retain(|i| i.record.id != record_id);
        for who in [idx, partner] {
            if self.entities[who].trade.accept {
                self.entities[who].trade.accept = false;
                let id = self.entities[who].details.user_id;
                self.entities[idx]
                    .session
                    .send(out::trade_accept(id, false));
                self.entities[partner]
                    .session
                    .send(out::trade_accept(id, false));
            }
        }
        self.refresh_windows(idx, partner);
    }

    /// Java: `TRADE_ACCEPT` after the checks.
    pub fn trade_accept(&mut self, idx: usize, partner: usize) {
        self.entities[idx].trade.accept = true;
        let id = self.entities[idx].details.user_id;
        self.entities[idx].session.send(out::trade_accept(id, true));
        self.entities[partner]
            .session
            .send(out::trade_accept(id, true));
        if self.entities[idx].trade.accept && self.entities[partner].trade.accept {
            self.entities[idx].session.send(out::trade_confirm());
            self.entities[partner].session.send(out::trade_confirm());
            for who in [idx, partner] {
                let t = &mut self.entities[who].trade;
                t.can_confirm = true;
                t.accept = false;
            }
        }
    }

    /// Java: `TRADE_UNACCEPT` after the checks: takes back the accept, or closes the trade once
    /// both are confirming.
    pub fn trade_unaccept(&mut self, idx: usize, partner: usize) {
        let t = &self.entities[idx].trade;
        if !t.can_confirm && t.accept {
            self.entities[idx].trade.accept = false;
            let id = self.entities[idx].details.user_id;
            self.entities[idx]
                .session
                .send(out::trade_accept(id, false));
            self.entities[partner]
                .session
                .send(out::trade_accept(id, false));
        } else {
            self.close_trade(idx);
        }
    }

    /// Java: `TRADE_CONFIRM_ACCEPT` after the checks. When both confirmed the trade completes:
    /// both get `TRADE_COMPLETED` now, the items change hands after the room's lock is released.
    pub fn trade_confirm(&mut self, idx: usize, partner: usize) {
        self.entities[idx].trade.confirmed = true;
        if !(self.entities[idx].trade.confirmed && self.entities[partner].trade.confirmed) {
            return;
        }
        self.entities[idx].session.send(out::trade_completed());
        self.entities[partner].session.send(out::trade_completed());
        // Java: `RoomTradeManager.finish`.
        let a = self.entities[idx].reset_trade();
        let b = self.entities[partner].reset_trade();
        self.deferred.push(Deferred::TradeFinished {
            sides: [
                self.entities[idx].trade_side(&a.items),
                self.entities[partner].trade_side(&b.items),
            ],
        });
    }

    /// Java: `RoomTradeManager.close(entity, false)` for an entity of the room.
    pub fn close_trade(&mut self, idx: usize) {
        let e = &mut self.entities[idx];
        let trade = e.reset_trade();
        let (session, user_id) = (e.session.clone(), e.details.user_id);
        self.close_trade_of(&session, user_id, trade);
    }

    /// Java: `RoomTradeManager.close` for a user who is not in the entity list any more (it just
    /// left, see `Room::remove_entity`), whose `trade` the caller already took.
    pub fn close_trade_of(&mut self, session: &Arc<Session>, user_id: u32, trade: TradeState) {
        let Some(partner) = trade.partner.and_then(|sid| self.entity_index(sid)) else {
            return;
        };
        // Both hands are sent before the offered items leave the trade (Java's order), so the
        // items on offer are missing from them.
        session.send(out::trade_close(user_id));
        self.entities[partner]
            .session
            .send(out::trade_close(user_id));
        let theirs = self.entities[partner].reset_trade();
        let sides = [
            TradeSide {
                session: session.clone(),
                user_id,
                items: trade.items.iter().map(|i| i.record.id).collect(),
            },
            self.entities[partner].trade_side(&theirs.items),
        ];
        self.deferred.push(Deferred::TradeClosed { sides });
    }
}

/// Runs the hand side of a cancelled trade: each hand is sent while the offered items are still
/// flagged (Java's `getView("new")` before `reset`), then the items are released.
pub fn run_closed(world: &World, sides: [TradeSide; 2]) {
    for side in sides {
        let mut p = side.session.player.lock();
        let hand = p.inventory.get_view(&world.store, "new");
        side.session.send(out_inventory::inventory(&hand));
        p.inventory.set_trading(&world.store, &side.items, false);
    }
}

/// Runs the hand side of a completed trade: the offered items change hands (Java:
/// `RoomTradeManager.addItems`), then both hands are sent.
///
/// TODO(transactions): `TransactionDao.createTransaction` ("Traded ... from/to ...").
pub fn run_finished(world: &World, sides: [TradeSide; 2]) {
    // Take everything out first, one hand at a time, so no two player locks are held.
    let mut taken: Vec<Vec<Item>> = sides
        .iter()
        .map(|side| {
            let mut p = side.session.player.lock();
            // Recycled, used or otherwise gone since it was offered: never hand over a ghost.
            side.items
                .iter()
                .filter_map(|&id| p.inventory.take_record(id))
                .collect()
        })
        .collect();
    let from_second = taken.pop().unwrap_or_default();
    let from_first = taken.pop().unwrap_or_default();
    for (side, items) in sides.iter().zip([from_second, from_first]) {
        let mut p = side.session.player.lock();
        for mut item in items {
            item.record.user_id = Some(side.user_id);
            item.record.room_id = 0;
            item.record.is_trading = false;
            if let Err(e) = world.store.update_item(&item.record) {
                error!(item = item.record.id, "saving traded item: {e}");
            }
            p.inventory.add_item(item);
        }
        let hand = p.inventory.get_view(&world.store, "new");
        side.session.send(out_inventory::inventory(&hand));
    }
}
