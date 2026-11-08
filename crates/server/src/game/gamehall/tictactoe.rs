//! Java: `GameTicTacToe`: five in a row on a 23 x 24 board (the client's "TicTacToe"), O against
//! X. One player is enough to start; a second one joins the running game.

use super::{Aborted, Command, GameInstance, GamePlayer, arg, arg_char, java_parse_int};
use crate::game::room::Room;

const NUM_IN_ROW: usize = 5;
const MAX_WIDTH: usize = 23;
const MAX_LENGTH: usize = 24;

/// Java: `GameToken`.
#[derive(Clone, Copy)]
struct Token {
    token: char,
    winning: char,
    moves: i32,
}

pub struct TicTacToe {
    tokens: [Token; 2],
    /// Java: `playerSides`, a `HashMap<Player, Character>` (each side is held at most once, so
    /// its order never shows).
    sides: Vec<(GamePlayer, char)>,
    finished: bool,
    /// Java: `gameMap[X][Y]`; `None` once the game stopped.
    map: Option<Vec<[char; MAX_LENGTH]>>,
    next_turn: Option<GamePlayer>,
}

impl TicTacToe {
    /// Java: `gameStart`.
    pub fn start(game: &GameInstance) -> Self {
        let mut t = Self {
            tokens: fresh_tokens(),
            sides: Vec::new(),
            finished: false,
            map: None,
            next_turn: None,
        };
        t.restart_map(&game.players);
        t
    }

    /// Java: `gameStop`.
    pub fn stop(&mut self) {
        self.sides.clear();
        self.map = None;
    }

    /// Java: `restartMap`.
    fn restart_map(&mut self, players: &[GamePlayer]) {
        self.tokens = fresh_tokens();
        if let Some(first) = players.first() {
            self.next_turn = Some(first.clone());
        }
        self.finished = false;
        self.map = Some(vec![['0'; MAX_LENGTH]; MAX_WIDTH]);
    }

    fn side_of(&self, player: &GamePlayer) -> Option<char> {
        self.sides
            .iter()
            .find(|(p, _)| p == player)
            .map(|(_, s)| *s)
    }

    /// Java: `playerSides.put`.
    fn set_side(&mut self, player: &GamePlayer, side: char) {
        match self.sides.iter_mut().find(|(p, _)| p == player) {
            Some(entry) => entry.1 = side,
            None => self.sides.push((player.clone(), side)),
        }
    }

    /// Java: `getPlayerBySide`.
    fn player_by_side(&self, side: char) -> Option<&GamePlayer> {
        self.sides.iter().find(|(_, s)| *s == side).map(|(p, _)| p)
    }

    /// Java: `getToken`.
    fn token(&mut self, side: char) -> Option<&mut Token> {
        self.tokens.iter_mut().find(|t| t.token == side)
    }

    /// Java: `getCurrentlyPlaying`; `None` where Java catches its exception and returns
    /// `new String[0]` (the player whose turn it is has no side).
    fn currently_playing(&self) -> Option<[String; 2]> {
        let mut names = [String::new(), String::new()];
        if let Some(next) = &self.next_turn {
            let side = self.side_of(next)?;
            names[0] = format!("{} {}", side.to_ascii_uppercase(), next.name);
        }
        Some(names)
    }

    /// Java: `swapTurns`.
    fn swap_turns(&mut self, player: &GamePlayer, players: &[GamePlayer]) {
        let mut next = None;
        if self.next_turn.as_ref() == Some(player) {
            for p in players {
                if p != player {
                    next = Some(p.clone());
                }
            }
        }
        self.next_turn = next;
    }

    /// Java: `broadcastMap`.
    fn broadcast_map(&self, game: &GameInstance) -> Result<(), Aborted> {
        let map = self.map.as_ref().ok_or(Aborted)?;
        let mut board = String::with_capacity(MAX_WIDTH * (MAX_LENGTH + 1));
        for row in map {
            for &c in row {
                board.push(if c == '0' { ' ' } else { c });
            }
            board.push(' ');
        }
        if let Some(names) = self.currently_playing() {
            game.send_item_msg(&["BOARDDATA", &names[0], "", &board]);
        }
        Ok(())
    }

    /// Java: `hasGameFinished`: the first five in a row (across, down, then both diagonals),
    /// with Java's bounds (the diagonals stop one column short).
    fn winner(&self) -> Option<(char, Vec<(usize, usize)>)> {
        let map = self.map.as_ref()?;
        let directions: [(i64, i64, usize); 4] = [
            (0, 1, MAX_LENGTH),
            (1, 0, MAX_WIDTH),
            (1, 1, MAX_WIDTH),
            (-1, 1, MAX_WIDTH),
        ];
        for (di, dj, limit) in directions {
            for i in 0..MAX_WIDTH {
                for j in 0..MAX_LENGTH {
                    let letter = map[i][j];
                    if letter == '0' {
                        continue;
                    }
                    let mut coords = Vec::new();
                    for k in 0..NUM_IN_ROW as i64 {
                        let x = i as i64 + di * k;
                        let y = j as i64 + dj * k;
                        if x < 0 {
                            continue;
                        }
                        // Java checks x and y against the same limit except across, where only
                        // y is checked (x is the row itself).
                        let out = match (di, dj) {
                            (0, 1) => y >= limit as i64,
                            (1, 0) => x >= limit as i64,
                            _ => x >= limit as i64 || y >= limit as i64,
                        };
                        if out {
                            continue;
                        }
                        let c = map[x as usize][y as usize];
                        if c != '0' && c == letter {
                            coords.push((x as usize, y as usize));
                            if coords.len() >= NUM_IN_ROW {
                                return Some((letter, coords));
                            }
                        } else {
                            coords.clear();
                        }
                    }
                }
            }
        }
        None
    }

    /// Java: `announceWinningSide`.
    fn announce(
        &mut self,
        room: &Room,
        game: &GameInstance,
        letter: char,
        coords: &[(usize, usize)],
    ) -> Result<(), Aborted> {
        let Some(token) = self
            .tokens
            .iter()
            .rev()
            .find(|t| t.token == letter)
            .copied()
        else {
            return Ok(());
        };
        if let Some(map) = self.map.as_mut() {
            for &(x, y) in coords {
                map[x][y] = token.winning;
            }
        }
        self.broadcast_map(game)?;
        let Some(winner) = self.player_by_side(token.token) else {
            return Ok(());
        };
        let text = format!("{} has won the game in {} moves", winner.name, token.moves);
        room.game_chat(&game.players, &text);
        Ok(())
    }
}

fn fresh_tokens() -> [Token; 2] {
    [
        Token {
            token: 'O',
            winning: 'q',
            moves: 0,
        },
        Token {
            token: 'X',
            winning: '+',
            moves: 0,
        },
    ]
}

/// Java: `GameTicTacToe.handleCommand`.
pub fn handle(
    room: &mut Room,
    game: &mut GameInstance,
    cmd: &Command<'_>,
    command: &str,
    args: &[&str],
) -> Result<(), Aborted> {
    let player = cmd.player;
    if command == "CLOSE" {
        room.leave_game(game, &player.session);
        return Ok(());
    }
    let mut state = match std::mem::replace(&mut game.state, super::GameState::Idle) {
        super::GameState::TicTacToe(t) => t,
        other => {
            game.state = other;
            return Ok(());
        }
    };
    let result = command_on(room, game, &mut state, cmd, command, args);
    game.state = super::GameState::TicTacToe(state);
    result
}

fn command_on(
    room: &mut Room,
    game: &GameInstance,
    t: &mut TicTacToe,
    cmd: &Command<'_>,
    command: &str,
    args: &[&str],
) -> Result<(), Aborted> {
    let player = cmd.player;
    match command {
        "CHOOSETYPE" => {
            let side = arg_char(args, 0)?;
            if t.token(side).is_none() {
                return Ok(());
            }
            if t.player_by_side(side).is_some() {
                player.send(game.item_msg(&["TYPERESERVED"]));
                return Ok(());
            }
            t.set_side(player, side);
            player.send(game.item_msg(&[&format!("SELECTTYPE {side}")]));
            let other = t.tokens.iter().find(|o| o.token != side).map(|o| o.token);
            if let Some(other) = other {
                if let Some(p) = game.players.iter().find(|p| *p != player) {
                    p.send(game.item_msg(&[&format!("SELECTTYPE {other}")]));
                    t.set_side(p, other);
                }
            }
            // Java indexes the empty array it gets back from getCurrentlyPlaying: throws.
            let names = t.currently_playing().ok_or(Aborted)?;
            game.send_item_msg(&["OPPONENTS", &names[0], &names[1]]);
        }
        "RESTART" => {
            t.restart_map(&game.players);
            t.broadcast_map(game)?;
        }
        "SETSECTOR" => {
            if game.players.len() < game.kind.min_players() {
                return Ok(());
            }
            let Some(own) = t.side_of(player) else {
                return Ok(());
            };
            if t.next_turn.as_ref() != Some(player) || t.finished {
                player.send(game.item_msg(&["TYPERESERVED"]));
                return Ok(());
            }
            if own != arg_char(args, 0)? {
                return Ok(());
            }
            let y = java_parse_int(arg(args, 1)?)?;
            let x = java_parse_int(arg(args, 2)?)?;
            if x >= MAX_WIDTH as i32 || y >= MAX_LENGTH as i32 || x < 0 || y < 0 {
                return Ok(());
            }
            let Some(map) = t.map.as_mut() else {
                return Ok(());
            };
            let (x, y) = (x as usize, y as usize);
            if map[x][y] != '0' {
                player.send(game.item_msg(&["TYPERESERVED"]));
                return Ok(());
            }
            let token = t.token(own).ok_or(Aborted)?;
            token.moves += 1;
            let letter = token.token;
            if let Some(map) = t.map.as_mut() {
                map[x][y] = letter;
            }
            match t.winner() {
                Some((letter, coords)) => {
                    t.finished = true;
                    t.announce(room, game, letter, &coords)?;
                }
                None => t.swap_turns(player, &game.players),
            }
            room.reset_room_timer(cmd.world, cmd.idx);
            t.broadcast_map(game)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_with(cells: &[(usize, usize, char)]) -> TicTacToe {
        let mut t = TicTacToe {
            tokens: fresh_tokens(),
            sides: Vec::new(),
            finished: false,
            map: Some(vec![['0'; MAX_LENGTH]; MAX_WIDTH]),
            next_turn: None,
        };
        for &(x, y, c) in cells {
            t.map.as_mut().unwrap()[x][y] = c;
        }
        t
    }

    #[test]
    fn five_across_and_down() {
        let row: Vec<_> = (0..5).map(|y| (3, y + 2, 'O')).collect();
        let (c, coords) = board_with(&row).winner().unwrap();
        assert_eq!(c, 'O');
        assert_eq!(coords, (0..5).map(|y| (3, y + 2)).collect::<Vec<_>>());
        let col: Vec<_> = (0..5).map(|x| (x + 18, 23, 'X')).collect();
        assert_eq!(board_with(&col).winner().unwrap().0, 'X');
        let four: Vec<_> = (0..4).map(|x| (x, 0, 'X')).collect();
        assert!(board_with(&four).winner().is_none());
    }

    #[test]
    fn diagonals_stop_short_of_the_last_column() {
        // Java's diagonal checks compare the column with MAX_WIDTH (23): a diagonal ending in
        // column 23 never wins.
        let diag: Vec<_> = (0..5).map(|k| (k, 19 + k, 'O')).collect();
        assert!(board_with(&diag).winner().is_none());
        let diag: Vec<_> = (0..5).map(|k| (k, 18 + k, 'O')).collect();
        assert!(board_with(&diag).winner().is_some());
        let anti: Vec<_> = (0..5).map(|k| (10 - k, 2 + k, 'X')).collect();
        let (_, coords) = board_with(&anti).winner().unwrap();
        // Found from its top end: x 10 down to 6.
        assert_eq!(coords[0], (10, 2));
    }
}
