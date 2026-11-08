//! Rooftop Rumble's Wobble Squabble (Java: `game/games/wobblesquabble` and the interactors in
//! `game/item/interactors/types/wobblesquabble`).
//!
//! The bridge of `md_a` is column 21, at height 8. A player with a ticket who stops on a join
//! tile (`wsJoinQueue`, 21,19 and 21,7, in the water) is warped onto the bridge (the tile its
//! program names), and every queue tile (`wsQueueTile`) walks them on one tile. The second player
//! to stop on a start tile (`wsTileStart`, 21,15 and 21,11) while the other one stands on the
//! other starts a game: `PT_PREPARE`, then the game task (Java's `WobbleGameTask`) runs 3 s later
//! and every 200 ms after that. While the game exists, nobody joins the bridge or moves along the
//! queue.
//!
//! Java's `RoomEntityManager.leaveRoom` ends the game with a tie when a player leaves, but it
//! asks `isPlaying` after `RoomEntity.reset` cleared the player's room, so it never does: the game
//! goes on without them until someone falls or it times out, and the one who left still pays a
//! ticket at the end. This port does the same.

use std::sync::Arc;
use std::time::Duration;

use rand::RngExt;
use tracing::error;

use super::entity::StatusType;
use super::items::Interaction;
use super::{Room, run_deferred};
use crate::game::pathfinder::Position;
use crate::game::statistics::stat;
use crate::messages::outgoing::wobble_squabble::{self as out, PlayerStatus};
use crate::messages::outgoing::{alerts, rooms_pool};
use crate::session::Session;
use crate::world::World;

/// Java: `WobbleSquabbleManager.WS_GAME_TICKET_COST`.
pub const TICKET_COST: i32 = 1;
const BALANCE_POINTS: i32 = 35;
const HIT_POINTS: i32 = 13;
const HIT_BALANCE_POINTS: i32 = 10;
const TIMEOUT_SECS: i64 = 60;
/// Java: `WobbleSquabbleGame.LEFT_X/LEFT_Y`: whoever starts there is player 0.
const LEFT: (i32, i32) = (21, 15);
/// Java: `scheduleTask(name, wsGame, 3 s, 200 ms)`.
const START_DELAY: Duration = Duration::from_secs(3);
const TICK: Duration = Duration::from_millis(200);
/// Java: the players fall for 1.5 s before `PT_END`.
const END_DELAY: Duration = Duration::from_millis(1500);
/// Java: `RoomTradeManager.TRADE_BAN_IP_HISTORY_LIMIT`.
const IP_HISTORY_LIMIT: usize = 20;

fn not_enough_tickets() -> String {
    format!("You need at least {TICKET_COST} ticket(s) to play Wobble Squabble!")
}

/// Java: `WobbleSquabbleMove`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    None = 0,
    BalanceLeft = 1,
    BalanceRight = 2,
    HitLeft = 3,
    HitRight = 4,
    WalkForward = 5,
    WalkBackward = 6,
    Rebalance = 7,
}

impl Move {
    /// Java: `WobbleSquabbleMove.getMove(id)`.
    pub fn from_id(id: i32) -> Option<Move> {
        Some(match id {
            0 => Move::None,
            1 => Move::BalanceLeft,
            2 => Move::BalanceRight,
            3 => Move::HitLeft,
            4 => Move::HitRight,
            5 => Move::WalkForward,
            6 => Move::WalkBackward,
            7 => Move::Rebalance,
            _ => return None,
        })
    }
}

/// Java: `WobbleSquabblePlayer`.
pub struct WsPlayer {
    /// The player (Java keeps the `Player`): their session and user id.
    session: Arc<Session>,
    user_id: u32,
    position: i32,
    balance: i32,
    rebalanced: bool,
    hit: bool,
    requires_update: bool,
    mv: Move,
}

impl WsPlayer {
    fn new(session: Arc<Session>, user_id: u32) -> Self {
        WsPlayer {
            session,
            user_id,
            position: 0,
            balance: 0,
            rebalanced: false,
            hit: false,
            requires_update: false,
            mv: Move::None,
        }
    }

    /// Java: `isBalancing`.
    fn is_balancing(&self) -> bool {
        self.balance > -100 && self.balance < 100
    }

    /// Java: `getScore`.
    fn score(&self) -> i32 {
        if !self.is_balancing() {
            0
        } else if self.balance >= 0 {
            100 - self.balance
        } else {
            100 + self.balance
        }
    }

    /// Java: `resetActions`.
    fn reset_actions(&mut self) {
        self.mv = Move::None;
        self.requires_update = false;
        self.hit = false;
    }

    fn status(&self) -> PlayerStatus {
        PlayerStatus {
            position: self.position,
            balance: self.balance,
            move_id: self.mv as i32,
            hit: self.hit,
        }
    }
}

/// Java: `WobbleSquabbleGame`, while its task is scheduled in the room.
pub struct WobbleGame {
    /// By order: player 0 started on 21,15.
    players: [WsPlayer; 2],
    started: bool,
    ended: bool,
    started_at: i64,
    task: Option<tokio::task::AbortHandle>,
}

impl Drop for WobbleGame {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl WobbleGame {
    fn player_by_id(&mut self, user_id: u32) -> Option<&mut WsPlayer> {
        self.players.iter_mut().find(|p| p.user_id == user_id)
    }
}

/// Java's `Integer.parseInt` of each comma-separated part of an item's program.
fn program_numbers(program: &str) -> Option<Vec<i32>> {
    program.split(',').map(|p| p.parse().ok()).collect()
}

/// One player's move in a game tick (Java: `WobbleSquabbleGame.updatePlayer`).
fn update_player(players: &mut [WsPlayer; 2], order: usize, rng: &mut impl rand::Rng) {
    let other = 1 - order;
    let distance = (players[order].position - players[other].position).abs();
    match players[order].mv {
        Move::BalanceLeft => players[order].balance -= BALANCE_POINTS + rng.random_range(0..10),
        Move::BalanceRight => players[order].balance += BALANCE_POINTS + rng.random_range(0..10),
        Move::HitLeft => {
            if distance <= 2 {
                players[other].hit = true;
                players[other].balance += HIT_POINTS + rng.random_range(0..10);
            } else {
                players[order].balance += HIT_BALANCE_POINTS + rng.random_range(0..10);
            }
        }
        Move::HitRight => {
            if distance <= 2 {
                players[other].hit = true;
                players[other].balance -= HIT_POINTS + rng.random_range(0..10);
            } else {
                players[order].balance -= HIT_BALANCE_POINTS + rng.random_range(0..10);
            }
        }
        // Java: forward is -1 for both players.
        Move::WalkForward | Move::WalkBackward => {
            let step = if players[order].mv == Move::WalkForward {
                -1
            } else {
                1
            };
            let position = players[order].position + step;
            if (-3..=4).contains(&position) && position != players[other].position {
                players[order].position = position;
            }
        }
        Move::Rebalance => {
            if !players[order].rebalanced {
                players[order].rebalanced = true;
                players[order].balance = 0;
            }
        }
        Move::None => {}
    }
}

impl Room {
    /// The world for code reached from a trigger with or without one at hand.
    fn wobble_world(&self, world: Option<&World>) -> Option<Arc<World>> {
        match world {
            Some(world) => world.arc(),
            None => self.world(),
        }
    }

    /// Java: `RoomPlayer.walkTo` for a player (`RoomEntity.walkTo` for anything else).
    fn walk_entity_to(&mut self, world: Option<&World>, idx: usize, to: Position) {
        let world = self.wobble_world(world);
        match world {
            Some(world) if self.entities[idx].is_player() => {
                self.walk_to_player(&world, idx, to.x, to.y);
            }
            _ => {
                self.walk_to(idx, to.x, to.y);
            }
        }
    }

    /// Java: `WobbleSquabbleJoinQueue.onEntityStop`: a player with a ticket is warped onto the
    /// bridge (the tile and rotation in the item's program), unless someone stands there, and
    /// walks on.
    pub(super) fn ws_join_queue_stop(&mut self, world: Option<&World>, idx: usize, item: usize) {
        if !self.entities[idx].is_player() || self.wobble.is_some() {
            return;
        }
        if self.entities[idx].details.tickets < TICKET_COST {
            let msg = alerts::alert(&not_enough_tickets());
            self.entities[idx].session.send(msg);
            return;
        }
        let Some(numbers) = program_numbers(&self.mapping.items[item].current_program) else {
            return;
        };
        let [x, y, rotation, ..] = numbers[..] else {
            return;
        };
        let mut destination = Position::new(x, y);
        destination.set_rotation(rotation);
        match self.mapping.tile(destination) {
            Some(tile) if tile.entities.is_empty() => {}
            _ => return,
        }
        let e = &mut self.entities[idx];
        e.is_teleporting = false;
        e.statuses.remove(StatusType::Swim);
        self.warp_to(idx, destination, true, false);
        // Java then calls the queue tile's trigger itself (the warp already did).
        self.ws_queue_tile_stop(world, idx);
    }

    /// Java: `BTCKS` in `md_a`: the current item's `onEntityStop` when it is a join tile.
    pub fn wobble_join_after_buying(&mut self, world: &World, idx: usize) {
        let Some(item) = self
            .mapping
            .tile(self.entities[idx].position)
            .and_then(|t| t.highest_item)
            .filter(|&i| self.mapping.items[i].interaction == Interaction::WsJoinQueue)
        else {
            return;
        };
        self.ws_join_queue_stop(Some(world), idx, item);
    }

    /// Java: `WobbleSquabbleQueueTile.onEntityStop`: on to the tile in front.
    pub(super) fn ws_queue_tile_stop(&mut self, world: Option<&World>, idx: usize) {
        if !self.entities[idx].is_player() || self.wobble.is_some() {
            return;
        }
        self.entities[idx].is_teleporting = false;
        let front = self.entities[idx].position.square_in_front();
        self.walk_entity_to(world, idx, front);
    }

    /// Java: `WobbleSquabbleTileStart.onEntityStop`: with someone on the other start tile (the
    /// item's program), the game is prepared, unless one of the two has no ticket: they are
    /// thrown off the bridge.
    pub(super) fn ws_tile_start_stop(&mut self, idx: usize, item: usize) {
        if !self.entities[idx].is_player() || self.wobble.is_some() {
            return;
        }
        let Some(numbers) = program_numbers(&self.mapping.items[item].current_program) else {
            return;
        };
        let [x, y, ..] = numbers[..] else {
            return;
        };
        let Some(&other_instance) = self
            .mapping
            .tile(Position::new(x, y))
            .and_then(|t| t.entities.first())
        else {
            return;
        };
        self.entities[idx].is_teleporting = false;
        let Some(other) = self
            .entity_by_instance(other_instance)
            .filter(|&o| self.entities[o].is_player())
        else {
            return; // Java casts the entity to a Player and throws.
        };
        // Java: `new WobbleSquabbleGame(entity, other)`.
        let (first, second) = if self.entities[idx].position == Position::new(LEFT.0, LEFT.1) {
            (idx, other)
        } else {
            (other, idx)
        };
        for player in [first, second] {
            if self.entities[player].details.tickets < TICKET_COST {
                let msg = alerts::alert(&not_enough_tickets());
                self.entities[player].session.send(msg);
                let side = if rand::rng().random_bool(0.5) { -1 } else { 1 };
                let at = self.entities[player].position;
                let mut position = Position::new(at.x + side, at.y);
                position.set_rotation(at.rotation());
                self.entities[player].statuses.set(StatusType::Swim, "");
                self.warp_to(player, position, true, false);
                return;
            }
        }
        let players = [first, second].map(|i| {
            let e = &self.entities[i];
            WsPlayer::new(e.session.clone(), e.details.user_id)
        });
        self.entities[first].walking_allowed = false;
        self.entities[second].walking_allowed = false;
        let msg = out::pt_prepare(
            self.entities[first].instance_id,
            self.entities[second].instance_id,
        );
        self.wobble = Some(WobbleGame {
            players,
            started: false,
            ended: false,
            started_at: 0,
            task: self.start_wobble_task(),
        });
        self.send(msg);
    }

    /// Java: `RoomTaskManager.scheduleTask("WobbleGameTask", game, 3000, 200, MILLISECONDS)`.
    fn start_wobble_task(&self) -> Option<tokio::task::AbortHandle> {
        let world = self.world()?;
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let world = Arc::downgrade(&world);
        let room = self.self_ref.clone();
        let handle = rt.spawn(async move {
            let start = tokio::time::Instant::now() + START_DELAY;
            let mut interval = tokio::time::interval_at(start, TICK);
            loop {
                interval.tick().await;
                let (Some(world), Some(room)) = (world.upgrade(), room.upgrade()) else {
                    return;
                };
                crate::game::contain_panic(format_args!("wobble squabble"), || {
                    let deferred = {
                        let mut r = room.lock();
                        r.wobble_tick(&world);
                        r.take_deferred()
                    };
                    run_deferred(&world, deferred);
                });
            }
        });
        Some(handle.abort_handle())
    }

    /// The instance id of a game's player (Java: `getRoomUser().getInstanceId()`): theirs in
    /// this room, or in the room they went to since, or -1 outside a room.
    fn ws_instance_id(&self, session: &Session) -> i32 {
        match self.entity_index(session.id) {
            Some(i) => self.entities[i].instance_id,
            None => session.instance_id(),
        }
    }

    /// Java: `WobbleSquabbleGame.run`.
    pub(crate) fn wobble_tick(&mut self, world: &World) {
        let Some(game) = self.wobble.as_ref() else {
            return;
        };
        if game.ended {
            return;
        }
        if !game.started {
            let sessions = [&game.players[0], &game.players[1]].map(|p| p.session.clone());
            let msg = out::pt_start(
                self.ws_instance_id(&sessions[0]),
                self.ws_instance_id(&sessions[1]),
            );
            self.send(msg);
            let game = self.wobble.as_mut().expect("checked above");
            game.started = true;
            game.started_at = crate::game::now();
            game.players[0].position = -3;
            game.players[1].position = 4;
            return;
        }
        let game = self.wobble.as_mut().expect("checked above");
        if game.players.iter().any(|p| p.requires_update) {
            let mut rng = rand::rng();
            update_player(&mut game.players, 0, &mut rng);
            update_player(&mut game.players, 1, &mut rng);
            let msg = out::pt_status([game.players[0].status(), game.players[1].status()]);
            for p in &mut game.players {
                p.reset_actions();
            }
            self.send(msg);
        }
        let game = self.wobble.as_ref().expect("checked above");
        if crate::game::now() > game.started_at + TIMEOUT_SECS {
            self.send(out::pt_timeout());
            self.end_wobble_game(world, -1);
            return;
        }
        // Java checks player 0, then player 1: if both fell, player 0 wins.
        let mut winner = -1;
        if !game.players[0].is_balancing() {
            winner = 1;
        }
        if !game.players[1].is_balancing() {
            winner = 0;
        }
        if winner != -1 {
            self.end_wobble_game(world, winner);
        }
    }

    /// Java: `WobbleSquabbleGame.endGame`: the statistics (only for players with different IP
    /// addresses), `PT_WIN`, and 1.5 s later `PT_END` and the players off the game.
    fn end_wobble_game(&mut self, world: &World, winner: i32) {
        let Some(game) = self.wobble.as_mut() else {
            return;
        };
        game.ended = true;
        let players: [(Arc<Session>, i32); 2] =
            [&game.players[0], &game.players[1]].map(|p| (p.session.clone(), p.score()));
        self.defer(move |world| save_statistics(world, &players, winner));
        self.send(out::pt_win(winner));
        self.schedule(world, END_DELAY, move |world, r| {
            r.finish_wobble_game(world, winner)
        });
    }

    /// The second half of Java's `endGame`, after the fall.
    fn finish_wobble_game(&mut self, world: &World, winner: i32) {
        self.send(out::pt_end());
        if winner == -1 {
            self.wobble_remove_player(0, true);
            self.wobble_remove_player(1, true);
        } else {
            self.wobble_remove_player(winner as usize, false);
            self.wobble_remove_player(1 - winner as usize, true);
        }
        self.wobble = None;
        self.move_queued_users(world);
    }

    /// Java: `WobbleSquabbleGame.removePlayer`: walking again, a ticket less and, when thrown,
    /// into the water on the side they leaned to.
    fn wobble_remove_player(&mut self, order: usize, thrown: bool) {
        let Some(game) = self.wobble.as_ref() else {
            return;
        };
        let player = &game.players[order];
        let (session, balance) = (player.session.clone(), player.balance);
        let Some(idx) = self.entity_index(session.id) else {
            // Gone from the room: Java does the same wherever they are now.
            self.defer(move |world| remove_elsewhere(world, &session, thrown, balance));
            return;
        };
        self.entities[idx].walking_allowed = true;
        let world = self.world();
        let tickets = self.decrease_tickets(world.as_deref(), idx);
        session.send(rooms_pool::ticket_balance(tickets));
        if thrown {
            self.entities[idx].statuses.set(StatusType::Swim, "");
            let at = self.entities[idx].position;
            let side = if balance < 0 { -1 } else { 1 };
            let mut position = Position::new(at.x + side, at.y);
            position.set_rotation(at.rotation());
            self.warp_to(idx, position, true, false);
        }
    }

    /// Java: `WobbleSquabbleGame.moveQueuedUsers`: everyone on a queue tile walks on a tile.
    fn move_queued_users(&mut self, world: &World) {
        let snapshot: Vec<i32> = self
            .entities
            .iter()
            .filter(|e| e.is_player())
            .map(|e| e.instance_id)
            .collect();
        for instance_id in snapshot {
            let Some(idx) = self.entity_by_instance(instance_id) else {
                continue;
            };
            if self
                .current_item(idx)
                .is_none_or(|i| i.interaction != Interaction::WsQueueTile)
            {
                continue;
            }
            let front = self.entities[idx].position.square_in_front();
            self.walk_to_player(world, idx, front.x, front.y);
        }
    }

    /// Java: `PTM` once `isPlaying` holds: the player's move for the next game tick. Moves 0-8
    /// pass Java's range check; 8 is no move.
    pub fn wobble_move(&mut self, user_id: u32, move_id: i32) {
        let Some(player) = self.wobble.as_mut().and_then(|g| g.player_by_id(user_id)) else {
            return;
        };
        if !(0..=8).contains(&move_id) {
            return;
        }
        let Some(mv) = Move::from_id(move_id) else {
            return;
        };
        player.mv = mv;
        player.requires_update = true;
    }

    /// Java: `WobbleSquabbleManager.isPlaying`.
    pub fn is_playing_wobble_squabble(&self, user_id: u32) -> bool {
        self.wobble
            .as_ref()
            .is_some_and(|g| g.players.iter().any(|p| p.user_id == user_id))
    }
}

/// The statistics part of Java's `endGame`: games won (both, for a tie) and the scores, unless
/// the two share an IP address (now or in each other's recent history).
fn save_statistics(world: &World, players: &[(Arc<Session>, i32); 2], winner: i32) {
    let [first, second] = players.each_ref().map(|(s, _)| {
        let p = s.player.lock();
        (p.ip_address.clone(), p.details.as_ref().map(|d| d.id))
    });
    let (Some(first_id), Some(second_id)) = (first.1, second.1) else {
        return;
    };
    let history = |id: u32| {
        world
            .store
            .ip_addresses(id, IP_HISTORY_LIMIT)
            .unwrap_or_default()
    };
    if first.0 == second.0
        || history(first_id).contains(&second.0)
        || history(second_id).contains(&first.0)
    {
        return;
    }
    for (order, (session, score)) in players.iter().enumerate() {
        let mut p = session.player.lock();
        if winner == -1 || winner as usize == order {
            p.statistics
                .increment_value(&world.store, stat::WOBBLE_SQUABBLE_GAMES_WON, 1);
        }
        if *score > 0 {
            p.statistics.increment_value(
                &world.store,
                stat::WOBBLE_SQUABBLE_MONTHLY_SCORES,
                *score,
            );
            p.statistics.increment_value(
                &world.store,
                stat::WOBBLE_SQUABBLE_POINTS_ALL_TIME,
                *score,
            );
        }
    }
}

/// `CurrencyDao.decreaseTickets` and `TICKET_BALANCE` for a game's player who left the room.
/// [`Room::wobble_remove_player`] for a player who left the room mid-game (Java never takes them
/// out of the game): the ticket is still taken, and in the room they are in now they may walk
/// again and, when thrown, swim one tile to the side (outside a room Java's `warp` only moves the
/// position, which the next room entry resets).
fn remove_elsewhere(world: &World, session: &Arc<Session>, thrown: bool, balance: i32) {
    let (tickets, room_id) = {
        let mut p = session.player.lock();
        let room_id = p.room_id;
        let Some(details) = p.details.as_mut() else {
            return;
        };
        let amount = if details.tickets <= 0 { 0 } else { 1 };
        match world.store.update_user(details.id, |u| u.tickets -= amount) {
            Ok(Some(stored)) => details.tickets = stored.tickets,
            Ok(None) => details.tickets -= amount,
            Err(e) => error!(user = details.id, "decreasing tickets: {e}"),
        }
        (details.tickets, room_id)
    };
    session.send(rooms_pool::ticket_balance(tickets));
    let Some(room) = room_id.and_then(|id| world.loaded_room(id)) else {
        return;
    };
    let deferred = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return;
        };
        r.entities[idx].details.tickets = tickets;
        r.entities[idx].walking_allowed = true;
        if thrown {
            r.entities[idx].statuses.set(StatusType::Swim, "");
            let at = r.entities[idx].position;
            let side = if balance < 0 { -1 } else { 1 };
            let mut position = Position::new(at.x + side, at.y);
            position.set_rotation(at.rotation());
            r.warp_to(idx, position, true, false);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn players(a: i32, b: i32) -> [WsPlayer; 2] {
        let session = Session::detached;
        let mut p = [WsPlayer::new(session(), 1), WsPlayer::new(session(), 2)];
        p[0].position = a;
        p[1].position = b;
        p
    }

    #[test]
    fn moves() {
        let mut rng = rand::rng();
        let mut p = players(-3, 4);
        // Forward is -1 for both; the ends of the bridge and the opponent stop a step.
        p[0].mv = Move::WalkForward;
        p[1].mv = Move::WalkForward;
        update_player(&mut p, 0, &mut rng);
        update_player(&mut p, 1, &mut rng);
        assert_eq!((p[0].position, p[1].position), (-3, 3));
        p[1].position = -2;
        update_player(&mut p, 1, &mut rng);
        assert_eq!(p[1].position, -2);
        p[0].mv = Move::WalkBackward;
        update_player(&mut p, 0, &mut rng);
        assert_eq!(p[0].position, -3);

        // A hit within 2 tiles moves the other's balance; further away, one's own.
        p[1].mv = Move::HitLeft;
        update_player(&mut p, 1, &mut rng);
        assert!(p[0].hit && (13..23).contains(&p[0].balance));
        p[0].mv = Move::HitRight;
        p[0].balance = 0;
        p[1].position = 4;
        p[1].hit = false;
        update_player(&mut p, 0, &mut rng);
        assert!(!p[1].hit && (-19..=-10).contains(&p[0].balance));
        p[0].mv = Move::Rebalance;
        update_player(&mut p, 0, &mut rng);
        assert_eq!(p[0].balance, 0);
        p[0].balance = 50;
        update_player(&mut p, 0, &mut rng);
        assert_eq!(p[0].balance, 50, "only once");
    }

    #[test]
    fn balance_and_score() {
        let mut p = players(0, 0);
        p[0].balance = 99;
        assert!(p[0].is_balancing());
        assert_eq!(p[0].score(), 1);
        p[0].balance = -40;
        assert_eq!(p[0].score(), 60);
        p[0].balance = 0;
        assert_eq!(p[0].score(), 100);
        p[0].balance = -100;
        assert!(!p[0].is_balancing());
        assert_eq!(p[0].score(), 0);
        assert_eq!(Move::from_id(8), None);
        assert_eq!(Move::from_id(7), Some(Move::Rebalance));
        assert_eq!(program_numbers("21,18,0"), Some(vec![21, 18, 0]));
        assert_eq!(program_numbers(""), None);
    }

    use crate::game::room::items::{Item, public_items};
    use crate::game::room::model::RoomModel;
    use crate::session::Outgoing;
    use rusbbo_protocol::frame::encode_server;
    use tokio::sync::mpsc::UnboundedReceiver;

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

    /// Rooftop Rumble's bridge: column 21 at height 8 from y 8 to 18 in water at height 4, with
    /// its join, queue and start tiles.
    fn rooftop(world: &Arc<World>) -> Room {
        let heightmap: Vec<String> = (0..22)
            .map(|y| {
                (0..25)
                    .map(|x| {
                        if x == 21 && (8..=18).contains(&y) {
                            '8'
                        } else {
                            '4'
                        }
                    })
                    .collect()
            })
            .collect();
        let model = Arc::new(RoomModel::parse(&rusbbo_storage::RoomModel {
            model_id: "md_a".into(),
            model_name: "md_a".into(),
            door_x: 3,
            door_y: 4,
            heightmap: heightmap.join("|"),
            trigger_class: "rooftop_rumble_trigger".into(),
            ..Default::default()
        }));
        let row =
            |sprite: &str, (x, y, z): (i32, i32, f64), program: &str| rusbbo_storage::PublicItem {
                sprite: Some(sprite.into()),
                x,
                y,
                z,
                top_height: 0.002,
                length: 1,
                width: 1,
                behaviour: "invisible,can_stand_on_top".into(),
                current_program: Some(program.into()),
                ..Default::default()
            };
        let rows = [
            row("wsJoinQueue", (21, 19, 4.0), "21,18,0"),
            row("wsQueueTile", (21, 18, 8.0), "21,18"),
            row("wsQueueTile", (21, 17, 8.0), ""),
            row("wsQueueTile", (21, 16, 8.0), ""),
            row("wsTileStart", (21, 15, 8.0), "21,11"),
            row("wsTileStart", (21, 11, 8.0), "21,15"),
        ];
        let items: Vec<Item> = public_items("md_a", &rows, &|_| None);
        let mut r = Room::for_test(0, model, items);
        r.world = Arc::downgrade(world);
        r
    }

    fn player(
        r: &mut Room,
        (x, y): (i32, i32),
        user_id: u32,
        tickets: i32,
    ) -> (usize, UnboundedReceiver<Outgoing>) {
        let (e, rx) = r.add_test_user(Position::new(x, y));
        r.entities[e].details.user_id = user_id;
        r.entities[e].details.tickets = tickets;
        r.update_new_height(e);
        (e, rx)
    }

    #[test]
    fn the_join_tile_needs_a_ticket() {
        let (_dir, world) = world();
        let mut r = rooftop(&world);
        let (e, mut rx) = player(&mut r, (21, 19), 1, 0);
        r.invoke_item_for_test(e);
        assert_eq!(
            frames(&mut rx),
            [encode_server(&alerts::alert(
                "You need at least 1 ticket(s) to play Wobble Squabble!"
            ))]
        );
        assert_eq!(r.entities[e].position, Position::new(21, 19));

        // With one, onto the bridge, out of the water, walking on.
        r.entities[e].details.tickets = 1;
        r.entities[e].statuses.set(StatusType::Swim, "");
        r.invoke_item_for_test(e);
        let e = &r.entities[e];
        assert_eq!(e.position, Position::new(21, 18));
        assert!(e.position.z > 8.0 && e.position.z < 8.01, "on the bridge");
        assert!(!e.statuses.contains(StatusType::Swim));
        assert!(e.is_walking);
        assert_eq!(e.path.front().copied(), Some(Position::new(21, 17)));
    }

    #[test]
    fn a_game_from_prepare_to_the_fall() {
        let (_dir, world) = world();
        let mut r = rooftop(&world);
        let (first, mut rx) = player(&mut r, (21, 15), 1, 3);
        r.invoke_item_for_test(first);
        assert!(r.wobble.is_none(), "alone on the bridge");
        let (second, _rx2) = player(&mut r, (21, 11), 2, 3);
        r.invoke_item_for_test(second);
        assert!(r.wobble.is_some());
        assert!(!r.entities[first].walking_allowed && !r.entities[second].walking_allowed);
        assert!(frames(&mut rx).contains(&encode_server(&out::pt_prepare(0, 1))));
        // Nobody joins the bridge or moves along the queue meanwhile.
        let (late, _rx3) = player(&mut r, (21, 19), 3, 3);
        r.invoke_item_for_test(late);
        assert_eq!(r.entities[late].position, Position::new(21, 19));

        r.wobble_tick(&world);
        assert_eq!(frames(&mut rx), [encode_server(&out::pt_start(0, 1))]);
        r.wobble_move(1, 9);
        r.wobble_move(3, 1);
        r.wobble_tick(&world);
        assert!(frames(&mut rx).is_empty(), "no move, no status");
        for _ in 0..3 {
            r.wobble_move(1, 1);
            r.wobble_tick(&world);
        }
        let sent = frames(&mut rx);
        assert_eq!(sent.last(), Some(&encode_server(&out::pt_win(1))));
        assert!(r.wobble.as_ref().unwrap().ended);
        r.wobble_tick(&world);
        assert!(frames(&mut rx).is_empty(), "the game is over");

        r.finish_wobble_game(&world, 1);
        assert!(r.wobble.is_none());
        let sent = frames(&mut rx);
        assert_eq!(sent[0], encode_server(&out::pt_end()));
        assert_eq!(sent[1], encode_server(&rooms_pool::ticket_balance(2)));
        let loser = &r.entities[first];
        assert_eq!(
            loser.position,
            Position::new(20, 15),
            "thrown to the side it leaned to"
        );
        assert!(loser.statuses.contains(StatusType::Swim) && loser.walking_allowed);
        assert_eq!(r.entities[second].details.tickets, 2);
        assert!(r.entities[second].walking_allowed);
    }

    #[test]
    fn a_game_times_out_as_a_tie() {
        let (_dir, world) = world();
        let mut r = rooftop(&world);
        let (first, mut rx) = player(&mut r, (21, 15), 1, 3);
        let (second, _rx2) = player(&mut r, (21, 11), 2, 3);
        r.invoke_item_for_test(second);
        r.wobble_tick(&world);
        frames(&mut rx);
        r.wobble.as_mut().unwrap().started_at = crate::game::now() - TIMEOUT_SECS - 1;
        r.wobble_tick(&world);
        assert_eq!(
            frames(&mut rx),
            [
                encode_server(&out::pt_timeout()),
                encode_server(&out::pt_win(-1))
            ]
        );
        r.finish_wobble_game(&world, -1);
        // Both thrown, each to the side of their balance (0: the right).
        assert_eq!(r.entities[first].position, Position::new(22, 15));
        assert_eq!(r.entities[second].position, Position::new(22, 11));
        assert!(r.entities[second].statuses.contains(StatusType::Swim));
    }

    #[test]
    fn a_player_without_tickets_is_thrown_off() {
        let (_dir, world) = world();
        let mut r = rooftop(&world);
        let (first, _rx) = player(&mut r, (21, 15), 1, 1);
        let (second, mut rx) = player(&mut r, (21, 11), 2, 0);
        r.invoke_item_for_test(second);
        assert!(r.wobble.is_none());
        assert_eq!(
            frames(&mut rx)[0],
            encode_server(&alerts::alert(
                "You need at least 1 ticket(s) to play Wobble Squabble!"
            ))
        );
        let poor = &r.entities[second];
        assert!(poor.statuses.contains(StatusType::Swim));
        assert!(poor.position == Position::new(20, 11) || poor.position == Position::new(22, 11));
        assert!(r.entities[first].walking_allowed);
    }
}
