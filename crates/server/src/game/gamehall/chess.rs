//! Java: `GameChess` (on chesslib, see `chess_board`). Two players; white moves first.
//!
//! The moves are checked by `traces/gamehall-chess-mate.json` and `gamehall-chess-draw.json`
//! (the oracle is built with chesslib), and by unit tests.

use super::chess_board::{Board, Move, PieceType, Side, parse_square, square_name};
use super::{Aborted, Command, GameInstance, GamePlayer, GameState, arg, arg_char};
use crate::game::room::Room;

pub struct Chess {
    /// Java: `gameTokens`, always `w` and `b`.
    tokens: [char; 2],
    /// Java: `playerSides`.
    sides: Vec<(GamePlayer, char)>,
    finished: bool,
    /// `None` once the game stopped (Java sets it to null).
    board: Option<Board>,
    next_turn: Option<GamePlayer>,
}

impl Chess {
    /// Java: `gameStart`: no sides yet, so nobody's turn.
    pub fn start() -> Self {
        Self {
            tokens: ['w', 'b'],
            sides: Vec::new(),
            finished: false,
            board: Some(Board::default()),
            next_turn: None,
        }
    }

    /// Java: `gameStop`.
    pub fn stop(&mut self) {
        self.sides.clear();
        self.board = None;
    }

    fn side_of(&self, player: &GamePlayer) -> Option<char> {
        self.sides
            .iter()
            .find(|(p, _)| p == player)
            .map(|(_, s)| *s)
    }

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

    /// Java: `restartMap`.
    fn restart_map(&mut self, players: &[GamePlayer]) {
        self.tokens = ['w', 'b'];
        if !players.is_empty() {
            self.next_turn = self.player_by_side('w').cloned();
        }
        self.finished = false;
        self.board = Some(Board::default());
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

    /// Java: `broadcastMap`: `PIECEDATA` with whose turn it is and every piece, A1 to H8.
    fn broadcast_map(&self, game: &GameInstance) -> Result<(), Aborted> {
        let board = self.board.as_ref().ok_or(Aborted)?;
        let mut data = String::new();
        for sq in 0..64u8 {
            let Some(piece) = board.piece(sq) else {
                continue;
            };
            data.push(if piece.side == Side::Black { 'B' } else { 'W' });
            data.push_str(piece_code(piece.kind));
            data.push_str(&square_name(sq));
            data.push(' ');
        }
        let mut names = [String::new(), String::new()];
        if let Some(next) = &self.next_turn {
            // Java: playerSides.get(nextTurn).getToken() throws without a side.
            let side = self.side_of(next).ok_or(Aborted)?;
            names[0] = format!("{} {}", side.to_ascii_uppercase(), next.name);
        }
        game.send_item_msg(&["PIECEDATA", &names[0], &names[1], &data]);
        Ok(())
    }
}

/// Java: `getChessPiece`.
fn piece_code(kind: PieceType) -> &'static str {
    match kind {
        PieceType::Bishop => "cr",
        PieceType::Knight => "hr",
        PieceType::King => "kg",
        PieceType::Queen => "qu",
        PieceType::Rook => "tw",
        PieceType::Pawn => "sd",
    }
}

/// Java: `GameChess.handleCommand`.
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
    let GameState::Chess(mut state) = std::mem::replace(&mut game.state, GameState::Idle) else {
        return Ok(());
    };
    let result = command_on(room, game, &mut state, cmd, command, args);
    game.state = GameState::Chess(state);
    result
}

fn command_on(
    room: &mut Room,
    game: &GameInstance,
    c: &mut Chess,
    cmd: &Command<'_>,
    command: &str,
    args: &[&str],
) -> Result<(), Aborted> {
    let player = cmd.player;
    match command {
        "CHOOSETYPE" => {
            let side = arg_char(args, 0)?;
            if !c.tokens.contains(&side) {
                return Ok(());
            }
            if c.player_by_side(side).is_some() || c.finished {
                player.send(game.item_msg(&["TYPERESERVED"]));
                return Ok(());
            }
            player.send(game.item_msg(&[&format!("SELECTTYPE {side}")]));
            c.set_side(player, side);
            if let Some(other) = c.tokens.iter().copied().find(|&t| t != side) {
                if let Some(p) = game.players.iter().find(|p| *p != player) {
                    p.send(game.item_msg(&[&format!("SELECTTYPE {other}")]));
                    c.set_side(p, other);
                }
            }
            c.restart_map(&game.players);
            c.broadcast_map(game)?;
        }
        "MOVEPIECE" => {
            if c.next_turn.as_ref() != Some(player) || c.finished {
                player.send(game.item_msg(&["TYPERESERVED"]));
                c.broadcast_map(game)?;
                return Ok(());
            }
            if game.players.len() < game.kind.min_players() || c.side_of(player).is_none() {
                c.broadcast_map(game)?;
                return Ok(());
            }
            let from = parse_square(arg(args, 0)?).ok_or(Aborted)?;
            let to = parse_square(arg(args, 1)?).ok_or(Aborted)?;
            if from == to {
                return Ok(());
            }
            let board = c.board.as_mut().ok_or(Aborted)?;
            let side = board.side_to_move();
            let legal = board.legal_moves();
            let chosen = match (from, to) {
                (Some(from), Some(to)) => {
                    let plain = Move {
                        from,
                        to,
                        promotion: None,
                    };
                    let queen = Move {
                        promotion: Some(PieceType::Queen),
                        ..plain
                    };
                    if legal.contains(&plain) {
                        Some(plain)
                    } else if Board::is_promo_rank(side, Some(to)) && legal.contains(&queen) {
                        Some(queen)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(m) = chosen {
                board.do_move(m);
                let ending = if board.is_draw() {
                    Some("The chess game has ended in a draw".to_string())
                } else if board.is_stale_mate() {
                    Some("The chess game has encountered a stalemate".to_string())
                } else if board.is_mated() {
                    Some(format!("{} has won the chess game", player.name))
                } else {
                    None
                };
                if let Some(text) = ending {
                    c.finished = true;
                    room.game_chat(&game.players, &text);
                    c.broadcast_map(game)?;
                    return Ok(());
                }
                c.swap_turns(player, &game.players);
            }
            room.reset_room_timer(cmd.world, cmd.idx);
            c.broadcast_map(game)?;
        }
        "RESTART" => {
            c.restart_map(&game.players);
            c.broadcast_map(game)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piece_data_of_the_start_position() {
        let c = Chess::start();
        let board = c.board.as_ref().unwrap();
        let mut data = String::new();
        for sq in 0..64u8 {
            if let Some(p) = board.piece(sq) {
                data.push(if p.side == Side::Black { 'B' } else { 'W' });
                data.push_str(piece_code(p.kind));
                data.push_str(&square_name(sq));
                data.push(' ');
            }
        }
        assert!(data.starts_with("Wtwa1 Whrb1 Wcrc1 Wqud1 Wkge1 Wcrf1 Whrg1 Wtwh1 Wsda2 "));
        assert!(data.ends_with("Bsdh7 Btwa8 Bhrb8 Bcrc8 Bqud8 Bkge8 Bcrf8 Bhrg8 Btwh8 "));
    }
}
