//! BattleBall's power ups (Java: `BattleBallPowerUp`, `powerups/*Handle`, `utils/PowerUpUtil`).
//! They appear at random (`ThreadLocalRandom`), so the traces play without them (no power ups
//! allowed, or Barebones Classic, map 5, which never has any); the unit tests cover them.

use std::time::Duration;

use rand::RngExt;
use rand::seq::SliceRandom;

use super::{
    Event, Object, Tick, Update, bb_of, bb_of_mut, check_fill, colour, colour_team, new_points,
    state, undo_tile,
};
use crate::game::games::game::Game;
use crate::game::games::player::PlayerState;
use crate::game::games::{GameState, Games, Op};
use crate::game::pathfinder::{Position, walk_direction};
use crate::game::room::Room;
use crate::game::room::mapping::is_valid_tile;
use crate::world::World;

/// Java: `ThreadLocalRandom.current().nextInt(from, to)`.
pub(crate) fn random(from: i32, to: i32) -> i32 {
    if to <= from {
        return from;
    }
    rand::rng().random_range(from..to)
}

/// Java: `Math.random()`.
pub(crate) fn random_f64() -> f64 {
    rand::rng().random::<f64>()
}

/// Java: `BattleBallPowerType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerType {
    LightBulb = 1,
    Spring = 2,
    Flashlight = 3,
    Cannon = 4,
    BoxOfPins = 5,
    Harlequin = 6,
    Bomb = 7,
    Drill = 8,
    QuestionMark = 9,
}

impl PowerType {
    /// Java: `getById`.
    pub fn by_id(id: i32) -> Option<Self> {
        Some(match id {
            1 => Self::LightBulb,
            2 => Self::Spring,
            3 => Self::Flashlight,
            4 => Self::Cannon,
            5 => Self::BoxOfPins,
            6 => Self::Harlequin,
            7 => Self::Bomb,
            8 => Self::Drill,
            9 => Self::QuestionMark,
            _ => return None,
        })
    }
}

/// Java: `BattleBallPowerUp`.
#[derive(Clone, Debug)]
pub struct PowerUp {
    pub id: i32,
    /// The tile it appeared on.
    pub x: i32,
    pub y: i32,
    pub z: f64,
    pub time_to_despawn: i32,
    /// The serial of the player holding it.
    pub holder: Option<u64>,
    pub power_type: PowerType,
}

impl PowerUp {
    /// Java: `new BattleBallPowerUp(id, game, tile)`: 20-30 s on the floor, one of the allowed
    /// types at random.
    pub fn new(id: i32, bb: &super::BattleBall, tile: usize) -> Self {
        let t = &bb.tiles[tile];
        let allowed = &bb.allowed_power_ups;
        let pick = allowed[random(0, allowed.len() as i32) as usize];
        Self {
            id,
            x: t.x,
            y: t.y,
            z: t.z,
            time_to_despawn: random(20, 31),
            holder: None,
            // Java: getById of an id that is no power type gives null (and throws later).
            power_type: PowerType::by_id(pick).unwrap_or(PowerType::Bomb),
        }
    }
}

/// The game player of the player in the arena with `session_id` (Java:
/// `getRoomUser().getGamePlayer()`).
fn game_player_of(game: &Game, session_id: u64) -> Option<u64> {
    game.players
        .iter()
        .rev()
        .find(|p| p.session.id == session_id && p.game_id == game.id)
        .map(|p| p.serial)
}

fn entity_of(game: &Game, room: &Room, serial: u64) -> Option<usize> {
    let session_id = game.player(serial)?.session.id;
    room.entity_index(session_id)
}

fn position_of(game: &Game, room: &Room, serial: u64) -> Position {
    entity_of(game, room, serial).map_or_else(Position::default, |i| room.entities[i].position)
}

/// Java: `BattleBallTile.getPlayers`: the game players standing on a tile.
fn players_on(game: &Game, room: &Room, p: Position) -> Vec<u64> {
    let Some(tile) = room.mapping.tile(p) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &instance_id in &tile.entities {
        let Some(e) = room.entity_by_instance(instance_id) else {
            continue;
        };
        let e = &room.entities[e];
        if !e.is_player() || e.position != p {
            continue;
        }
        if let Some(serial) = game_player_of(game, e.session.id) {
            if !out.contains(&serial) {
                out.push(serial);
            }
        }
    }
    out
}

/// Java: `TileUtil.isValidGameTile`.
fn is_valid_game_tile(
    game: &Game,
    room: &Room,
    serial: u64,
    p: Position,
    check_entities: bool,
) -> bool {
    if bb_of(game).tile_index(p.x, p.y).is_none() {
        return false;
    }
    let walker = if check_entities {
        entity_of(game, room, serial).map(|i| room.walker(i))
    } else {
        None
    };
    is_valid_tile(&room.mapping, walker.as_ref(), p)
}

/// Java: `Position.getCircle`.
fn circle(center: Position, radius: i32) -> Vec<Position> {
    let mut out = Vec::new();
    for x in -radius..=radius {
        for y in -radius..=radius {
            let mut p = center;
            p.x += x;
            p.y += y;
            if center.distance_squared(&p) <= radius {
                out.push(p);
            }
        }
    }
    out
}

/// Runs `task` on the game `uid` after `delay`, with its arena locked (Java:
/// `GameScheduler.schedule` with a lambda that holds the game).
pub(crate) fn schedule_game(
    world: &World,
    uid: u64,
    delay: Duration,
    task: impl FnOnce(&mut Games, usize, &mut Op) + Send + 'static,
) {
    crate::game::scheduler::schedule(world, delay, move |world| {
        let room = world.games.arena_of(uid);
        let run = |games: &mut Games, op: &mut Op| {
            if let Some(gi) = games.game_index(uid) {
                task(games, gi, op);
            }
        };
        match room {
            Some(room) => {
                let deferred = {
                    let mut r = room.lock();
                    world.games.run(world, Some(&mut r), run);
                    r.take_deferred()
                };
                crate::game::room::run_deferred(world, deferred);
            }
            None => world.games.run(world, None, run),
        }
    });
}

/// Java: `PowerUpUtil.stunPlayer`: the player stops and cannot walk for 5 s.
fn stun_player(world: &World, game: &mut Game, room: &mut Room, serial: u64, how: PlayerState) {
    if let Some(i) = entity_of(game, room, serial) {
        room.stop_walking_entity(world, i);
        if let Some(i) = entity_of(game, room, serial) {
            room.entities[i].walking_allowed = false;
        }
    }
    room.regenerate_map();
    let user_id = game.player(serial).map_or(0, |p| p.user_id);
    if let Some(gp) = game.player_mut(serial) {
        gp.player_state = Some(how);
    }
    bb_of_mut(game).queue_object(Update::Player(serial, user_id));
    schedule_game(
        world,
        game.uid,
        Duration::from_secs(5),
        move |games, gi, op| {
            let game = &mut games.games[gi];
            if game.state != GameState::Ended {
                let session_id = game.player(serial).map(|p| p.session.id);
                if let (Some(room), Some(session_id)) = (op.room.as_deref_mut(), session_id) {
                    if let Some(i) = room.entity_index(session_id) {
                        room.entities[i].walking_allowed = true;
                    }
                }
            }
            if let Some(gp) = game.player_mut(serial) {
                gp.player_state = Some(PlayerState::Normal);
            }
            if let super::super::GameKind::BattleBall(bb) = &mut game.kind {
                bb.queue_object(Update::Player(serial, user_id));
            }
        },
    );
}

/// Java: `BattleBallPowerUp.checkPowerUp`: walking onto a power up picks it up.
pub(crate) fn check_power_up(game: &mut Game, tile: usize, serial: u64, tick: &mut Tick) {
    let bb = bb_of_mut(game);
    let (x, y) = (bb.tiles[tile].x, bb.tiles[tile].y);
    let Some(index) = bb.active_powers.iter().position(|p| p.x == x && p.y == y) else {
        return;
    };
    let mut power = bb.active_powers.remove(index);
    if power.power_type == PowerType::QuestionMark {
        let mut choices: Vec<i32> = bb.allowed_power_ups.clone();
        if let Some(i) = choices
            .iter()
            .position(|&p| p == PowerType::QuestionMark as i32)
        {
            choices.remove(i);
        }
        if choices.is_empty() {
            choices.push(PowerType::Bomb as i32);
        }
        let pick = choices[random(0, choices.len() as i32) as usize];
        power.power_type = PowerType::by_id(pick).unwrap_or(PowerType::Bomb);
    }
    bb.objects.retain(|o| *o != Object::Power(power.id));
    power.time_to_despawn = 15;
    power.holder = Some(serial);
    let (id, ty) = (power.id, power.power_type as i32);
    let stored = bb.stored_of_mut(serial);
    stored.clear();
    stored.push(power);
    tick.events.push(Event::Acquire {
        serial,
        power: id,
        ty,
    });
    tick.objects.push(Update::Power(id));
}

/// Java: `NailBoxHandle.checkNailTile`: a pin under the player breaks their ball.
pub(crate) fn check_nail_tile(
    world: &World,
    game: &mut Game,
    serial: u64,
    room: &mut Room,
) -> bool {
    let p = position_of(game, room, serial);
    let bb = bb_of_mut(game);
    let Some(i) = bb
        .objects
        .iter()
        .position(|o| matches!(o, Object::Pin { x, y, .. } if *x == p.x && *y == p.y))
    else {
        return false;
    };
    let Object::Pin { id, .. } = bb.objects.remove(i) else {
        unreachable!("a pin");
    };
    bb.events_queue.push(Event::Despawn(id));
    stun_player(world, game, room, serial, PlayerState::BallBroken);
    true
}

/// Java: `BattleBallPowerUp.hasUsedPower`: the spring seals every tile jumped on; the drill
/// cleans them.
pub(crate) fn has_used_power(
    game: &mut Game,
    tile: usize,
    serial: u64,
    updates: &mut Vec<usize>,
    fills: &mut Vec<usize>,
) -> bool {
    if bb_of(game).tiles[tile].colour == colour::DISABLED {
        return false;
    }
    match game.player_state(serial) {
        PlayerState::HighJumps => {
            if bb_of(game).tiles[tile].state == state::SEALED {
                return true;
            }
            let team = colour_team(game, serial);
            let t = &mut bb_of_mut(game).tiles[tile];
            t.points.clear();
            t.colour = team;
            t.state = state::SEALED;
            new_points(game, tile, serial, state::SEALED, team);
            check_fill(game, tile, serial, fills);
            updates.push(tile);
            true
        }
        PlayerState::CleaningTiles => {
            if undo_tile(&mut bb_of_mut(game).tiles[tile]) {
                updates.push(tile);
            }
            true
        }
        _ => false,
    }
}

/// Java: `GAMEEVENT` 4: the player uses the power up `power_id` they hold.
pub fn use_stored_power(game: &mut Game, serial: u64, power_id: i32, room: &mut Room, op: &mut Op) {
    let bb = bb_of_mut(game);
    let Some(stored) = bb.stored_powers.iter_mut().find(|(s, _)| *s == serial) else {
        return;
    };
    let Some(i) = stored.1.iter().position(|p| p.id == power_id) else {
        return;
    };
    let power = stored.1.remove(i);
    bb.events_queue.push(Event::Activate {
        serial,
        power: power.id,
        ty: power.power_type as i32,
    });
    let world = op.world;
    match power.power_type {
        PowerType::BoxOfPins => nail_box(world, game, serial, room),
        PowerType::Flashlight => torch(game, serial, room),
        PowerType::LightBulb => lightbulb(game, serial, room),
        PowerType::Drill => timed_state(world, game, serial, PlayerState::CleaningTiles),
        PowerType::Spring => timed_state(world, game, serial, PlayerState::HighJumps),
        PowerType::Harlequin => harlequin(world, game, serial),
        PowerType::Cannon => cannon(world, game, serial, room),
        PowerType::Bomb => bomb(world, game, serial, room),
        PowerType::QuestionMark => {}
    }
}

/// Java: `SpringHandle` and `VacuumHandle`: the state for 10 s.
fn timed_state(world: &World, game: &mut Game, serial: u64, how: PlayerState) {
    let user_id = game.player(serial).map_or(0, |p| p.user_id);
    if let Some(gp) = game.player_mut(serial) {
        gp.player_state = Some(how);
    }
    bb_of_mut(game).queue_object(Update::Player(serial, user_id));
    schedule_game(
        world,
        game.uid,
        Duration::from_secs(10),
        move |games, gi, _| {
            let game = &mut games.games[gi];
            if game.state == GameState::Ended || game.player_state(serial) != how {
                return;
            }
            if let Some(gp) = game.player_mut(serial) {
                gp.player_state = Some(PlayerState::Normal);
            }
            bb_of_mut(game).queue_object(Update::Player(serial, user_id));
        },
    );
}

/// Java: `HarlequinHandle`: the other teams' players colour for this player for 10 s.
fn harlequin(world: &World, game: &mut Game, serial: u64) {
    let team = game.player(serial).map_or(-1, |p| p.team_id);
    let mut affected = Vec::new();
    for other in game.active_serials() {
        let p = game.player(other).expect("active");
        if p.harlequin.is_some() || p.team_id == team {
            continue;
        }
        if game.player_state(other) != PlayerState::Normal {
            continue;
        }
        let user_id = p.user_id;
        let p = game.player_mut(other).expect("active");
        p.player_state = Some(PlayerState::ColoringForOpponent);
        p.harlequin = Some(serial);
        bb_of_mut(game).queue_object(Update::Player(other, user_id));
        affected.push((other, user_id));
    }
    schedule_game(
        world,
        game.uid,
        Duration::from_secs(10),
        move |games, gi, _| {
            let game = &mut games.games[gi];
            if game.state == GameState::Ended {
                return;
            }
            for (other, user_id) in affected {
                if let Some(p) = game.player_mut(other) {
                    p.player_state = Some(PlayerState::Normal);
                    p.harlequin = None;
                }
                bb_of_mut(game).queue_object(Update::Player(other, user_id));
            }
        },
    );
}

/// Java: `LightbulbHandle`: the tiles within 5 go one step towards the team's colour.
fn lightbulb(game: &mut Game, serial: u64, room: &Room) {
    let team = colour_team(game, serial);
    let center = position_of(game, room, serial);
    for p in circle(center, 5) {
        colour_step(game, serial, team, p);
    }
}

/// Java: `TorchHandle`: the tiles in a line in front of the player.
fn torch(game: &mut Game, serial: u64, room: &Room) {
    let team = colour_team(game, serial);
    let mut next = position_of(game, room, serial);
    let mut tiles = Vec::new();
    while is_valid_game_tile(game, room, serial, next, false) {
        next = next.square_in_front();
        if !is_valid_game_tile(game, room, serial, next, false) {
            break;
        }
        tiles.push(next);
    }
    for p in tiles {
        colour_step(game, serial, team, p);
    }
}

/// The colouring of the light bulb and the torch: a default tile counts as touched (three hits
/// instead of four), another team's tile becomes clicked.
fn colour_step(game: &mut Game, serial: u64, team: i32, p: Position) {
    let Some(i) = bb_of(game).tile_index(p.x, p.y) else {
        return;
    };
    let tile = &bb_of(game).tiles[i];
    if tile.colour == colour::DISABLED || tile.state == state::SEALED {
        return;
    }
    let mut current = tile.state;
    if current == state::DEFAULT {
        current = state::TOUCHED;
    }
    let new_state = if tile.colour != team {
        state::CLICKED
    } else {
        current + 1
    };
    new_points(game, i, serial, new_state, team);
    let bb = bb_of_mut(game);
    bb.tiles[i].colour = team;
    bb.tiles[i].state = new_state;
    let mut fills = Vec::new();
    if new_state == state::SEALED {
        check_fill(game, i, serial, &mut fills);
    }
    let bb = bb_of_mut(game);
    bb.fill_tiles_queue.extend(fills);
    bb.update_tiles_queue.push(i);
}

/// Java: `NailBoxHandle.handle`: 8-15 pins around the tile six in front; whoever stands on one
/// is dizzy, and the pins go 12-16 s later.
fn nail_box(world: &World, game: &mut Game, serial: u64, room: &mut Room) {
    let Some(i) = entity_of(game, room, serial) else {
        return;
    };
    let me = room.entities[i].position;
    let next_position = room.entities[i].next_position;
    let mut target = me;
    for _ in 0..6 {
        target = target.square_in_front();
    }
    let max_pins = random(8, 16) as usize;
    let mut positions = circle(target, 3);
    positions.shuffle(&mut rand::rng());
    let mut pins = Vec::new();
    let mut dizzy = Vec::new();
    let mut selected = 0;
    for mut c in positions {
        if c == me {
            continue;
        }
        let Some(t) = bb_of(game).tile_index(c.x, c.y) else {
            continue;
        };
        if room.mapping.tile(c).is_none() || Some(c) == next_position {
            continue;
        }
        c.z = bb_of(game).tiles[t].z;
        if selected < max_pins && rand::rng().random::<bool>() {
            let id = game.create_object_id();
            bb_of_mut(game).events_queue.push(Event::PinSpawn {
                id,
                x: c.x,
                y: c.y,
                z: c.z,
            });
            pins.push(Object::Pin {
                id,
                x: c.x,
                y: c.y,
                z: c.z,
            });
            selected += 1;
            for s in players_on_tile_entities(game, room, c) {
                if game.player(s).is_some_and(|p| !p.spectator) {
                    dizzy.push(s);
                }
            }
        }
    }
    bb_of_mut(game).objects.extend(pins.iter().cloned());
    for s in dizzy {
        stun_player(world, game, room, s, PlayerState::BallBroken);
    }
    for pin in pins {
        let Object::Pin { id, .. } = pin else {
            continue;
        };
        let delay = Duration::from_secs(random(12, 17) as u64);
        schedule_game(world, game.uid, delay, move |games, gi, _| {
            let bb = bb_of_mut(&mut games.games[gi]);
            bb.events_queue.push(Event::Despawn(id));
            bb.objects
                .retain(|o| !matches!(o, Object::Pin { id: p, .. } if *p == id));
        });
    }
}

/// The game players on a room tile (Java: `getTile(pos).getEntities()`, players only).
fn players_on_tile_entities(game: &Game, room: &Room, p: Position) -> Vec<u64> {
    let Some(tile) = room.mapping.tile(p) else {
        return Vec::new();
    };
    tile.entities
        .iter()
        .filter_map(|&i| room.entity_by_instance(i))
        .filter(|&e| room.entities[e].is_player())
        .filter_map(|e| game_player_of(game, room.entities[e].session.id))
        .collect()
}

/// Java: `BombHandle`: the tiles within 3 are cleaned, everyone within 7 is pushed back and
/// stunned, the bomber too.
fn bomb(world: &World, game: &mut Game, serial: u64, room: &mut Room) {
    let Some(i) = entity_of(game, room, serial) else {
        return;
    };
    room.stop_walking_entity(world, i);
    let Some(i) = entity_of(game, room, serial) else {
        return;
    };
    room.entities[i].walking_allowed = false;
    let center = room.entities[i].position;
    for p in circle(center, 3) {
        if room.mapping.tile(p).is_none() || !is_valid_tile(&room.mapping, None, p) {
            continue;
        }
        let bb = bb_of_mut(game);
        if let Some(t) = bb.tile_index(p.x, p.y) {
            if undo_tile(&mut bb.tiles[t]) {
                bb.update_tiles_queue.push(t);
            }
        }
    }
    let mut stunned = Vec::new();
    for p in circle(center, 7) {
        if room.mapping.tile(p).is_none() || !is_valid_tile(&room.mapping, None, p) {
            continue;
        }
        stunned.extend(players_on(game, room, p));
    }
    if !stunned.contains(&serial) {
        stunned.push(serial);
    }
    for s in stunned {
        if let Some(e) = entity_of(game, room, s) {
            room.stop_walking_entity(world, e);
            if let Some(e) = entity_of(game, room, s) {
                room.entities[e].walking_allowed = false;
            }
        }
        if s != serial {
            let from = position_of(game, room, s);
            let towards = position_of(game, room, serial);
            let mut push = from;
            push.set_rotation(walk_direction(from.x, from.y, towards.x, towards.y));
            let push = push.square_behind();
            if is_valid_game_tile(game, room, s, push, true) {
                if let Some(e) = entity_of(game, room, s) {
                    room.warp_to(e, push, false, false);
                }
            }
        }
        stun_player(world, game, room, s, PlayerState::Stunned);
    }
}

/// Java: `CannonHandle`: the player flies to the end of the line in front, sealing it; whoever
/// stands in the way is pushed aside and stunned.
fn cannon(world: &World, game: &mut Game, serial: u64, room: &mut Room) {
    let Some(i) = entity_of(game, room, serial) else {
        return;
    };
    room.stop_walking_entity(world, i);
    let Some(i) = entity_of(game, room, serial) else {
        return;
    };
    room.entities[i].walking_allowed = false;
    let first = room.entities[i].position;
    let rotation = first.rotation();
    let mut next = first;
    let mut tiles: Vec<Position> = Vec::new();
    while is_valid_game_tile(game, room, serial, next, false) {
        next = next.square_in_front();
        if !is_valid_game_tile(game, room, serial, next, false) {
            break;
        }
        tiles.push(next);
    }
    if tiles.is_empty() {
        tiles.push(first);
    }
    let in_the_way = tiles.clone();
    schedule_game(
        world,
        game.uid,
        Duration::from_millis(200),
        move |games, gi, op| {
            let Some(room) = op.room.as_deref_mut() else {
                return;
            };
            let world = op.world;
            let game = &mut games.games[gi];
            let mut stunned = Vec::new();
            for p in &in_the_way {
                for s in players_on(game, room, *p) {
                    if s != serial {
                        stunned.push((s, *p));
                    }
                }
            }
            for (s, from) in stunned {
                if let Some(e) = entity_of(game, room, s) {
                    if room.entities[e].is_walking {
                        room.stop_walking_entity(world, e);
                    }
                }
                let mut pushed_from = from;
                pushed_from.set_rotation(rotation);
                let set = [pushed_from.square_right(), pushed_from.square_left()]
                    .into_iter()
                    .find(|p| is_valid_game_tile(game, room, s, *p, true));
                if let Some(set) = set {
                    bb_of_mut(game).queue_player_move(s, set.x, set.y);
                }
                stun_player(world, game, room, s, PlayerState::Stunned);
                if let Some(mut set) = set {
                    set.set_rotation(position_of(game, room, s).rotation());
                    if let Some(e) = entity_of(game, room, s) {
                        room.warp_to(e, set, false, false);
                    }
                }
            }
        },
    );
    let team = colour_team(game, serial);
    for p in &tiles {
        let Some(t) = bb_of(game).tile_index(p.x, p.y) else {
            continue;
        };
        let tile = &bb_of(game).tiles[t];
        if tile.colour == colour::DISABLED || (tile.state == state::SEALED && tile.colour == team) {
            continue;
        }
        new_points(game, t, serial, state::SEALED, team);
        let bb = bb_of_mut(game);
        bb.tiles[t].colour = team;
        bb.tiles[t].state = state::SEALED;
        let mut fills = Vec::new();
        check_fill(game, t, serial, &mut fills);
        let bb = bb_of_mut(game);
        bb.fill_tiles_queue.extend(fills);
        bb.update_tiles_queue.push(t);
    }
    let mut last = *tiles.last().expect("one tile at least");
    last.set_rotation(rotation);
    let user_id = game.player(serial).map_or(0, |p| p.user_id);
    if let Some(gp) = game.player_mut(serial) {
        gp.player_state = Some(PlayerState::FlyingThroughAir);
    }
    let bb = bb_of_mut(game);
    bb.queue_object(Update::Player(serial, user_id));
    bb.queue_player_move(serial, last.x, last.y);
    schedule_game(
        world,
        game.uid,
        Duration::from_millis(800),
        move |games, gi, op| {
            let Some(room) = op.room.as_deref_mut() else {
                return;
            };
            let world = op.world;
            stun_player(
                world,
                &mut games.games[gi],
                room,
                serial,
                PlayerState::Stunned,
            );
        },
    );
    if let Some(e) = entity_of(game, room, serial) {
        room.warp_to(e, last, false, false);
    }
}
