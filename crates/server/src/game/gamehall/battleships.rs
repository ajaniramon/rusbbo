//! Java: `GameBattleShip` (with `utils/GameShip*`): ten ships each on a 13 x 12 grid, then turns
//! of shots; a miss passes the turn two seconds later.

use std::collections::BTreeMap;
use std::time::Duration;

use super::{Aborted, Command, GameInstance, GamePlayer, GameState, arg, java_parse_int};
use crate::game::room::Room;

/// Java: `GameShipType`: id, how many each player has, length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShipType {
    AircraftCarrier,
    Battleship,
    Cruiser,
    Destroyer,
}

impl ShipType {
    fn by_id(id: i32) -> Option<Self> {
        Some(match id {
            5 => Self::AircraftCarrier,
            4 => Self::Battleship,
            3 => Self::Cruiser,
            2 => Self::Destroyer,
            _ => return None,
        })
    }

    fn max_allowed(self) -> usize {
        match self {
            Self::AircraftCarrier => 1,
            Self::Battleship => 2,
            Self::Cruiser => 3,
            Self::Destroyer => 4,
        }
    }

    fn length(self) -> i32 {
        match self {
            Self::AircraftCarrier => 5,
            Self::Battleship => 4,
            Self::Cruiser => 3,
            Self::Destroyer => 2,
        }
    }
}

/// Java: `GameShipMoveResult`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MoveResult {
    Hit,
    Miss,
}

impl MoveResult {
    fn symbol(self) -> char {
        match self {
            Self::Hit => 'X',
            Self::Miss => 'O',
        }
    }
}

/// Java: `GameShip`.
struct Ship {
    kind: ShipType,
    x: i32,
    y: i32,
    player: i32,
    horizontal: bool,
}

impl Ship {
    fn squares(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (0..self.kind.length()).map(|i| {
            if self.horizontal {
                (self.x + i, self.y)
            } else {
                (self.x, self.y + i)
            }
        })
    }
}

/// Java: `GameShipMove`.
struct ShipMove {
    x: i32,
    y: i32,
    result: MoveResult,
    /// Index into [`BattleShips::ships`].
    ship: Option<usize>,
}

/// The game's state (Java's fields, as `gameStart` and `gameStop` set them).
#[derive(Default)]
pub struct BattleShips {
    /// Java: `shipsPlaced`, a `HashMap` keyed by the ship objects; here in placing order (Java's
    /// order only shows when two ships of a player overlap).
    ships: Vec<Ship>,
    /// Java: `playerListMap`: the shots of each player number.
    moves: BTreeMap<i32, Vec<ShipMove>>,
    /// Java: `players`, a `Player[2]` filled by the first ship each places.
    players: [Option<GamePlayer>; 2],
    next_turn: i32,
    is_turn_used: bool,
    game_started: bool,
    #[allow(dead_code)] // Java sets it but nothing reads it.
    game_ended: bool,
}

fn opposite_num(player: i32) -> i32 {
    if player == 0 { 1 } else { 0 }
}

impl BattleShips {
    /// Java: `getPlayerNum`: the player's slot, or -1.
    fn player_num(&self, player: Option<&GamePlayer>) -> i32 {
        self.players
            .iter()
            .position(|p| p.as_ref() == player)
            .map_or(-1, |i| i as i32)
    }

    /// Java: `getOppositePlayer`: the first slot that is not `player` (an empty slot counts).
    fn opposite_player(&self, player: Option<&GamePlayer>) -> Option<GamePlayer> {
        self.players
            .iter()
            .find(|p| p.as_ref() != player)
            .and_then(Clone::clone)
    }

    /// Java: `countShips(shipType, player)` (-1: any player).
    fn count_ships(&self, kind: Option<ShipType>, player: i32) -> usize {
        self.ships
            .iter()
            .filter(|s| kind.is_none_or(|k| s.kind == k))
            .filter(|s| player == -1 || s.player == player)
            .count()
    }

    /// Java: `getShipPlaced(x, y, player)`: the opponent's ship on that square.
    fn ship_at(&self, x: i32, y: i32, player: i32) -> Option<usize> {
        let opponent = opposite_num(player);
        self.ships
            .iter()
            .enumerate()
            .filter(|(_, s)| s.player == opponent)
            .find(|(_, s)| s.squares().any(|sq| sq == (x, y)))
            .map(|(i, _)| i)
    }

    /// Java: `GameShip.getHits`: squares of the ship whose first shot hit. Java fails when the
    /// other player has no shot list.
    fn hits(&self, ship: usize) -> Result<i32, Aborted> {
        let ship = &self.ships[ship];
        let shots = self.moves.get(&opposite_num(ship.player)).ok_or(Aborted)?;
        Ok(ship
            .squares()
            .filter(|&(x, y)| {
                shots
                    .iter()
                    .find(|m| m.x == x && m.y == y)
                    .is_some_and(|m| m.result == MoveResult::Hit)
            })
            .count() as i32)
    }

    fn is_destroyed(&self, ship: usize) -> Result<bool, Aborted> {
        Ok(self.hits(ship)? == self.ships[ship].kind.length())
    }

    fn is_hit_twice(&self, ship: usize) -> Result<bool, Aborted> {
        let hits = self.hits(ship)?;
        Ok(hits >= 2 && hits != self.ships[ship].kind.length())
    }

    /// Java: `isGameOver(player)`: every ship of `player` sunk (none counts as all).
    fn is_game_over(&self, player: i32) -> Result<bool, Aborted> {
        for (i, ship) in self.ships.iter().enumerate() {
            if ship.player == player && !self.is_destroyed(i)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Java: `generateHitGrid(player)`: the shots at `player`'s ships, 12 rows of 13.
    fn hit_grid(&self, player: i32) -> Result<String, Aborted> {
        let shots = self.moves.get(&opposite_num(player)).ok_or(Aborted)?;
        let mut grid = String::with_capacity(13 * 12);
        for y in 0..12 {
            for x in 0..13 {
                match shots.iter().find(|m| m.x == x && m.y == y) {
                    None => grid.push('-'),
                    Some(m) => {
                        let sunk = match m.ship {
                            Some(s) => self.is_destroyed(s)?,
                            None => false,
                        };
                        grid.push(if sunk { 'S' } else { m.result.symbol() });
                    }
                }
            }
        }
        Ok(grid)
    }

    /// Java: `sendMarkedMap`.
    fn send_marked_map(&self, game: &GameInstance) -> Result<(), Aborted> {
        for p in &self.players {
            let Some(opponent) = self.opposite_player(p.as_ref()) else {
                return Ok(());
            };
            let p = p.as_ref().ok_or(Aborted)?;
            let own = self.player_num(Some(p));
            let other = self.player_num(Some(&opponent));
            let (first, second) = if own == 0 {
                (self.hit_grid(own)?, self.hit_grid(other)?)
            } else {
                (self.hit_grid(other)?, self.hit_grid(own)?)
            };
            p.send(game.item_msg(&["SITUATION", "", &first, "", &second]));
        }
        Ok(())
    }

    /// Java: `hasEveryoneFinished`.
    fn has_everyone_finished(&self) -> bool {
        self.count_ships(None, 0) == 10 && self.count_ships(None, 1) == 10
    }

    /// Java: `rotateTurn`.
    fn rotate_turn(&mut self, game: &GameInstance) {
        self.next_turn = opposite_num(self.next_turn);
        game.send_item_msg(&["TURN", &self.next_turn.to_string()]);
        self.is_turn_used = false;
    }
}

/// Java: `GameBattleShip.handleCommand`.
pub fn handle(
    room: &mut Room,
    game: &mut GameInstance,
    cmd: &Command<'_>,
    command: &str,
    args: &[&str],
) -> Result<(), Aborted> {
    if command == "CLOSE" {
        room.leave_game(game, &cmd.player.session);
        return Ok(());
    }
    let GameState::BattleShips(mut state) = std::mem::replace(&mut game.state, GameState::Idle)
    else {
        return Ok(());
    };
    let result = command_on(room, game, &mut state, cmd, command, args);
    game.state = GameState::BattleShips(state);
    result
}

fn command_on(
    room: &mut Room,
    game: &GameInstance,
    b: &mut BattleShips,
    cmd: &Command<'_>,
    command: &str,
    args: &[&str],
) -> Result<(), Aborted> {
    let player = cmd.player;
    match command {
        "PLACESHIP" => {
            if b.game_started {
                return Ok(());
            }
            let ship_id = java_parse_int(arg(args, 0)?)?;
            let start_x = java_parse_int(arg(args, 1)?)?;
            let start_y = java_parse_int(arg(args, 2)?)?;
            let _end_x = java_parse_int(arg(args, 3)?)?;
            let end_y = java_parse_int(arg(args, 4)?)?;
            let Some(kind) = ShipType::by_id(ship_id) else {
                return Ok(());
            };
            if b.player_num(Some(player)) == -1 {
                if b.players[0].is_none() {
                    b.players[0] = Some(player.clone());
                } else if b.players[1].is_none() {
                    b.players[1] = Some(player.clone());
                }
            }
            let num = b.player_num(Some(player));
            if b.count_ships(Some(kind), num) >= kind.max_allowed() {
                return Ok(());
            }
            b.moves.entry(num).or_default();
            b.ships.push(Ship {
                kind,
                x: start_x,
                y: start_y,
                player: num,
                horizontal: start_y == end_y,
            });
            if b.has_everyone_finished() {
                let mut data = Vec::new();
                for p in &b.players {
                    // Java: players[i].getDetails() throws on an empty slot.
                    let p = p.as_ref().ok_or(Aborted)?;
                    data.push(format!("{} {}", b.player_num(Some(p)), p.name));
                }
                game.send_item_msg(&["OPPONENTS", &data.join("\r")]);
                b.rotate_turn(game);
                b.game_started = true;
            }
        }
        "SHOOT" => {
            let num = b.player_num(Some(player));
            if b.next_turn != num || b.is_turn_used {
                return Ok(());
            }
            let x = java_parse_int(arg(args, 0)?)?;
            let y = java_parse_int(arg(args, 1)?)?;
            let ship = b.ship_at(x, y, num);
            let result = if ship.is_some() {
                MoveResult::Hit
            } else {
                MoveResult::Miss
            };
            let Some(opponent) = b.opposite_player(Some(player)) else {
                return Ok(());
            };
            b.moves
                .get_mut(&num)
                .ok_or(Aborted)?
                .push(ShipMove { x, y, result, ship });
            b.send_marked_map(game)?;
            let what = match ship {
                Some(s) if b.is_destroyed(s)? => "SINK",
                Some(s) if b.is_hit_twice(s)? => "HITTWICE",
                Some(_) => "HIT",
                None => "MISS",
            };
            game.send_item_msg(&[what]);
            if result == MoveResult::Miss {
                b.is_turn_used = true;
                let index = cmd.index;
                room.schedule(cmd.world, Duration::from_secs(2), move |_, room| {
                    room.with_game_instance(index, |_, game| {
                        let state = std::mem::replace(&mut game.state, GameState::Idle);
                        if let GameState::BattleShips(mut b) = state {
                            b.rotate_turn(game);
                            game.state = GameState::BattleShips(b);
                        } else {
                            game.state = state;
                        }
                    });
                });
            } else {
                b.is_turn_used = false;
                game.send_item_msg(&["TURN", &b.next_turn.to_string()]);
            }
            if b.is_game_over(b.player_num(Some(&opponent)))? {
                b.game_ended = true;
                game.send_item_msg(&["GAMEEND", &player.name]);
                game.send_item_msg(&["GAMEOVER"]);
                b.is_turn_used = true;
            }
            room.reset_room_timer(cmd.world, cmd.idx);
        }
        _ => {}
    }
    Ok(())
}
