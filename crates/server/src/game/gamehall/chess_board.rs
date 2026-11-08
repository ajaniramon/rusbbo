//! The chess rules Java takes from chesslib 1.1.1 (`Board`, `MoveGenerator`), as `GameChess`
//! uses them: legal moves (castling, en passant, promotions), `doMove` with its castle-right
//! updates, `isDraw` (repetition, insufficient material, stalemate, the 50-move rule) and
//! `isMated`. Squares are numbered as chesslib's `Square` enum: A1 = 0, B1 = 1, ..., H8 = 63.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    White,
    Black,
}

impl Side {
    pub fn flip(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    /// The rank a pawn of this side promotes on, and the direction it moves.
    fn pawn_dir(self) -> i32 {
        match self {
            Self::White => 1,
            Self::Black => -1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceType {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Piece {
    pub side: Side,
    pub kind: PieceType,
}

/// Java: `CastleRight`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CastleRight {
    None,
    KingSide,
    QueenSide,
    Both,
}

impl CastleRight {
    fn king_side(self) -> bool {
        matches!(self, Self::KingSide | Self::Both)
    }

    fn queen_side(self) -> bool {
        matches!(self, Self::QueenSide | Self::Both)
    }
}

/// Java: `Move`: from, to and the promotion piece (if any).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Move {
    pub from: u8,
    pub to: u8,
    pub promotion: Option<PieceType>,
}

/// What chesslib's `hashCode` covers (pieces, side to move, both castle rights, the en passant
/// square), compared as a whole instead of hashed.
#[derive(Clone, PartialEq, Eq)]
struct Key {
    squares: [Option<Piece>; 64],
    side: Side,
    castle: [CastleRight; 2],
    en_passant: Option<u8>,
}

#[derive(Clone)]
pub struct Board {
    squares: [Option<Piece>; 64],
    side: Side,
    castle: [CastleRight; 2],
    /// Java: `enPassant`: the square behind a pawn that just moved two squares.
    en_passant: Option<u8>,
    /// Java: `enPassantTarget`: that pawn's square, only when an enemy pawn stands beside it.
    en_passant_target: Option<u8>,
    half_moves: i32,
    /// Java: `updateHistory`. Havana's `new Board()` is `Board(new GameContext(), false)`, so it
    /// is off and the history stays empty: `isDraw` never finds a repetition
    /// (`traces/gamehall-chess-draw.json`).
    update_history: bool,
    /// Java: `history`, one key per move made (the start position is not in it).
    history: Vec<Key>,
}

fn file(sq: u8) -> i32 {
    (sq % 8) as i32
}

fn rank(sq: u8) -> i32 {
    (sq / 8) as i32
}

fn square(file: i32, rank: i32) -> Option<u8> {
    ((0..8).contains(&file) && (0..8).contains(&rank)).then(|| (rank * 8 + file) as u8)
}

/// Java: `Square.valueOf(name.toUpperCase())`: `Some(None)` is `Square.NONE`, `None` an unknown
/// name (Java throws).
pub fn parse_square(name: &str) -> Option<Option<u8>> {
    let upper = name.to_uppercase();
    if upper == "NONE" {
        return Some(None);
    }
    let b = upper.as_bytes();
    if b.len() != 2 || !(b'A'..=b'H').contains(&b[0]) || !(b'1'..=b'8').contains(&b[1]) {
        return None;
    }
    Some(square((b[0] - b'A') as i32, (b[1] - b'1') as i32))
}

/// The square's name in lower case (Java: `square.value().toLowerCase()`).
pub fn square_name(sq: u8) -> String {
    format!("{}{}", (b'a' + sq % 8) as char, (b'1' + sq / 8) as char)
}

const KNIGHT: [(i32, i32); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];
const KING: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];
const ROOK_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

impl Default for Board {
    /// Java: `new Board()`: the start position.
    fn default() -> Self {
        use PieceType::*;
        let mut squares = [None; 64];
        let back = [Rook, Knight, Bishop, Queen, King, Bishop, Knight, Rook];
        for f in 0..8 {
            squares[f] = Some(Piece {
                side: Side::White,
                kind: back[f],
            });
            squares[8 + f] = Some(Piece {
                side: Side::White,
                kind: Pawn,
            });
            squares[48 + f] = Some(Piece {
                side: Side::Black,
                kind: Pawn,
            });
            squares[56 + f] = Some(Piece {
                side: Side::Black,
                kind: back[f],
            });
        }
        Self {
            squares,
            side: Side::White,
            castle: [CastleRight::Both; 2],
            en_passant: None,
            en_passant_target: None,
            half_moves: 0,
            update_history: false,
            history: Vec::new(),
        }
    }
}

impl Board {
    pub fn piece(&self, sq: u8) -> Option<Piece> {
        self.squares[sq as usize]
    }

    pub fn side_to_move(&self) -> Side {
        self.side
    }

    fn key(&self) -> Key {
        Key {
            squares: self.squares,
            side: self.side,
            castle: self.castle,
            en_passant: self.en_passant,
        }
    }

    fn king_square(&self, side: Side) -> Option<u8> {
        (0..64u8).find(|&sq| {
            self.squares[sq as usize]
                == Some(Piece {
                    side,
                    kind: PieceType::King,
                })
        })
    }

    /// Whether `by` attacks `sq` on this board.
    fn attacked(&self, sq: u8, by: Side) -> bool {
        let (f, r) = (file(sq), rank(sq));
        let has = |s: Option<u8>, kinds: &[PieceType]| {
            s.and_then(|s| self.squares[s as usize])
                .is_some_and(|p| p.side == by && kinds.contains(&p.kind))
        };
        // Pawns of `by` attack from one rank behind (from `by`'s point of view).
        let back = -by.pawn_dir();
        if has(square(f - 1, r + back), &[PieceType::Pawn])
            || has(square(f + 1, r + back), &[PieceType::Pawn])
        {
            return true;
        }
        if KNIGHT
            .iter()
            .any(|(df, dr)| has(square(f + df, r + dr), &[PieceType::Knight]))
        {
            return true;
        }
        if KING
            .iter()
            .any(|(df, dr)| has(square(f + df, r + dr), &[PieceType::King]))
        {
            return true;
        }
        let slide = |dirs: &[(i32, i32)], kinds: &[PieceType]| {
            dirs.iter().any(|(df, dr)| {
                let (mut cf, mut cr) = (f + df, r + dr);
                while let Some(s) = square(cf, cr) {
                    if let Some(p) = self.squares[s as usize] {
                        return p.side == by && kinds.contains(&p.kind);
                    }
                    cf += df;
                    cr += dr;
                }
                false
            })
        };
        slide(&ROOK_DIRS, &[PieceType::Rook, PieceType::Queen])
            || slide(&BISHOP_DIRS, &[PieceType::Bishop, PieceType::Queen])
    }

    /// Java: `isKingAttacked`.
    pub fn is_king_attacked(&self) -> bool {
        self.king_square(self.side)
            .is_some_and(|k| self.attacked(k, self.side.flip()))
    }

    fn push_pawn_move(moves: &mut Vec<Move>, side: Side, from: u8, to: u8) {
        let last = if side == Side::White { 7 } else { 0 };
        if rank(to) == last {
            for kind in [
                PieceType::Queen,
                PieceType::Rook,
                PieceType::Bishop,
                PieceType::Knight,
            ] {
                moves.push(Move {
                    from,
                    to,
                    promotion: Some(kind),
                });
            }
        } else {
            moves.push(Move {
                from,
                to,
                promotion: None,
            });
        }
    }

    fn pseudo_legal_moves(&self) -> Vec<Move> {
        let side = self.side;
        let mut moves = Vec::new();
        let plain = |from: u8, to: u8| Move {
            from,
            to,
            promotion: None,
        };
        for from in 0..64u8 {
            let Some(p) = self.squares[from as usize].filter(|p| p.side == side) else {
                continue;
            };
            let (f, r) = (file(from), rank(from));
            let free = |s: u8| self.squares[s as usize].is_none();
            let enemy = |s: u8| self.squares[s as usize].is_some_and(|q| q.side != side);
            match p.kind {
                PieceType::Pawn => {
                    let dir = side.pawn_dir();
                    if let Some(one) = square(f, r + dir).filter(|&s| free(s)) {
                        Self::push_pawn_move(&mut moves, side, from, one);
                        let start = if side == Side::White { 1 } else { 6 };
                        if r == start {
                            if let Some(two) = square(f, r + 2 * dir).filter(|&s| free(s)) {
                                moves.push(plain(from, two));
                            }
                        }
                    }
                    for df in [-1, 1] {
                        let Some(to) = square(f + df, r + dir) else {
                            continue;
                        };
                        if enemy(to) {
                            Self::push_pawn_move(&mut moves, side, from, to);
                        } else if let Some(target) = self.en_passant_target {
                            // En passant: beside the pawn that just moved two squares, onto the
                            // square behind it.
                            if rank(target) == r && file(target) == f + df && free(to) {
                                moves.push(plain(from, to));
                            }
                        }
                    }
                }
                PieceType::Knight | PieceType::King => {
                    let steps = if p.kind == PieceType::Knight {
                        &KNIGHT
                    } else {
                        &KING
                    };
                    for (df, dr) in steps {
                        if let Some(to) = square(f + df, r + dr).filter(|&s| free(s) || enemy(s)) {
                            moves.push(plain(from, to));
                        }
                    }
                }
                PieceType::Bishop | PieceType::Rook | PieceType::Queen => {
                    let dirs: Vec<(i32, i32)> = match p.kind {
                        PieceType::Bishop => BISHOP_DIRS.to_vec(),
                        PieceType::Rook => ROOK_DIRS.to_vec(),
                        _ => [ROOK_DIRS, BISHOP_DIRS].concat(),
                    };
                    for (df, dr) in dirs {
                        let (mut cf, mut cr) = (f + df, r + dr);
                        while let Some(to) = square(cf, cr) {
                            if free(to) {
                                moves.push(plain(from, to));
                            } else {
                                if enemy(to) {
                                    moves.push(plain(from, to));
                                }
                                break;
                            }
                            cf += df;
                            cr += dr;
                        }
                    }
                }
            }
        }
        // Java: MoveGenerator.generateCastleMoves.
        if !self.is_king_attacked() {
            let base = if side == Side::White { 0 } else { 56 };
            let right = self.castle[side.index()];
            let free = |files: &[u8]| {
                files
                    .iter()
                    .all(|f| self.squares[(base + f) as usize].is_none())
            };
            let safe = |files: &[u8]| files.iter().all(|f| !self.attacked(base + f, side.flip()));
            if right.king_side() && free(&[5, 6]) && safe(&[5, 6]) {
                moves.push(plain(base + 4, base + 6));
            }
            if right.queen_side() && free(&[1, 2, 3]) && safe(&[2, 3]) {
                moves.push(plain(base + 4, base + 2));
            }
        }
        moves
    }

    /// Java: `MoveGenerator.generateLegalMoves`.
    pub fn legal_moves(&self) -> Vec<Move> {
        let side = self.side;
        self.pseudo_legal_moves()
            .into_iter()
            .filter(|m| {
                let mut after = self.clone();
                after.apply(*m);
                after
                    .king_square(side)
                    .is_none_or(|k| !after.attacked(k, side.flip()))
            })
            .collect()
    }

    /// Moves the pieces of `m` (with the castling rook, the pawn taken en passant and the
    /// promotion); returns the piece taken.
    fn apply(&mut self, m: Move) -> Option<Piece> {
        let piece = self.squares[m.from as usize]?;
        let mut captured = self.squares[m.to as usize];
        if piece.kind == PieceType::Pawn
            && captured.is_none()
            && file(m.from) != file(m.to)
            && Some(m.to) == self.en_passant
        {
            if let Some(target) = self.en_passant_target {
                captured = self.squares[target as usize].take();
            }
        }
        if piece.kind == PieceType::King && (file(m.from) - file(m.to)).abs() == 2 {
            let base = m.from - file(m.from) as u8;
            let (rook_from, rook_to) = if file(m.to) == 6 {
                (base + 7, base + 5)
            } else {
                (base, base + 3)
            };
            self.squares[rook_to as usize] = self.squares[rook_from as usize].take();
        }
        self.squares[m.from as usize] = None;
        self.squares[m.to as usize] = Some(match m.promotion {
            Some(kind) => Piece {
                side: piece.side,
                kind,
            },
            None => piece,
        });
        captured
    }

    /// Java's corner rule for castle rights (a rook leaving or taken on `sq`): the king-side
    /// corner leaves only the queen side of Both and clears anything else, and the other way
    /// round.
    fn corner_rule(&mut self, side: Side, sq: u8) {
        let right = self.castle[side.index()];
        if right == CastleRight::None {
            return;
        }
        let base = if side == Side::White { 0 } else { 56 };
        if sq == base + 7 {
            self.castle[side.index()] = if right == CastleRight::Both {
                CastleRight::QueenSide
            } else {
                CastleRight::None
            };
        } else if sq == base {
            self.castle[side.index()] = if right == CastleRight::Both {
                CastleRight::KingSide
            } else {
                CastleRight::None
            };
        }
    }

    /// Java: `Board.doMove(move, true)` of a legal move.
    pub fn do_move(&mut self, m: Move) {
        let side = self.side;
        let Some(piece) = self.squares[m.from as usize] else {
            return;
        };
        if piece.kind == PieceType::King {
            self.castle[side.index()] = CastleRight::None;
        } else if piece.kind == PieceType::Rook {
            self.corner_rule(side, m.from);
        }
        let captured = self.apply(m);
        if captured.is_some_and(|c| c.kind == PieceType::Rook) {
            self.corner_rule(side.flip(), m.to);
        }
        if captured.is_none() {
            self.half_moves += 1;
        } else {
            self.half_moves = 0;
        }
        self.en_passant = None;
        self.en_passant_target = None;
        if piece.kind == PieceType::Pawn {
            if (rank(m.to) - rank(m.from)).abs() == 2 {
                let beside = [
                    square(file(m.to) - 1, rank(m.to)),
                    square(file(m.to) + 1, rank(m.to)),
                ];
                let enemy_pawn = Piece {
                    side: side.flip(),
                    kind: PieceType::Pawn,
                };
                if beside
                    .iter()
                    .flatten()
                    .any(|&s| self.squares[s as usize] == Some(enemy_pawn))
                {
                    self.en_passant_target = Some(m.to);
                }
                self.en_passant = square(file(m.to), rank(m.to) - side.pawn_dir());
            }
            self.half_moves = 0;
        }
        self.side = side.flip();
        if self.update_history {
            self.history.push(self.key());
        }
    }

    /// Java: `Board.isPromoRank(side, move)`.
    pub fn is_promo_rank(side: Side, to: Option<u8>) -> bool {
        let last = if side == Side::White { 7 } else { 0 };
        to.is_some_and(|t| rank(t) == last)
    }

    /// Java: `isInsufficientMaterial` (chesslib 1.1.1's rules).
    fn is_insufficient_material(&self) -> bool {
        let pieces: Vec<Piece> = self.squares.iter().flatten().copied().collect();
        let count_of = |side: Option<Side>, kind: Option<PieceType>| {
            pieces
                .iter()
                .filter(|p| side.is_none_or(|s| p.side == s) && kind.is_none_or(|k| p.kind == k))
                .count()
        };
        if count_of(None, Some(PieceType::Pawn)) != 0 {
            return false;
        }
        if count_of(None, Some(PieceType::Queen)) + count_of(None, Some(PieceType::Rook)) != 0 {
            return false;
        }
        match pieces.len() {
            4 => {
                (count_of(Some(Side::White), None) > 1 && count_of(Some(Side::Black), None) > 1)
                    || count_of(Some(Side::White), Some(PieceType::Knight)) == 2
                    || count_of(Some(Side::Black), Some(PieceType::Knight)) == 2
            }
            n => n < 4,
        }
    }

    /// Java: `isStaleMate`.
    pub fn is_stale_mate(&self) -> bool {
        !self.is_king_attacked() && self.legal_moves().is_empty()
    }

    /// Java: `isMated`.
    pub fn is_mated(&self) -> bool {
        self.is_king_attacked() && self.legal_moves().is_empty()
    }

    /// Java: `isDraw`: threefold repetition (within the half-move counter), insufficient
    /// material, stalemate, then the 50-move rule.
    pub fn is_draw(&self) -> bool {
        let n = self.history.len();
        if n >= 4 {
            let last = &self.history[n - 1];
            let i = (n - 1).min(self.half_moves.max(0) as usize);
            let mut repetitions = 0;
            let mut x = 2;
            while x <= i {
                if self.history[n - x - 1] == *last {
                    repetitions += 1;
                    if repetitions >= 2 {
                        return true;
                    }
                }
                x += 2;
            }
        }
        if self.is_insufficient_material() {
            return true;
        }
        if self.is_stale_mate() {
            return true;
        }
        self.half_moves >= 100
    }

    #[cfg(test)]
    pub fn empty(side: Side) -> Self {
        Self {
            squares: [None; 64],
            side,
            castle: [CastleRight::None; 2],
            en_passant: None,
            en_passant_target: None,
            half_moves: 0,
            update_history: false,
            history: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn put(&mut self, name: &str, side: Side, kind: PieceType) {
        let sq = parse_square(name).flatten().unwrap();
        self.squares[sq as usize] = Some(Piece { side, kind });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(board: &mut Board, from: &str, to: &str) -> bool {
        let m = Move {
            from: parse_square(from).flatten().unwrap(),
            to: parse_square(to).flatten().unwrap(),
            promotion: None,
        };
        if board.legal_moves().contains(&m) {
            board.do_move(m);
            true
        } else {
            false
        }
    }

    /// Perft from the start position (well-known counts).
    fn perft(board: &Board, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        board
            .legal_moves()
            .into_iter()
            .map(|m| {
                let mut b = board.clone();
                b.do_move(m);
                perft(&b, depth - 1)
            })
            .sum()
    }

    #[test]
    fn start_position_perft() {
        let b = Board::default();
        assert_eq!(perft(&b, 1), 20);
        assert_eq!(perft(&b, 2), 400);
        assert_eq!(perft(&b, 3), 8902);
    }

    #[test]
    fn fools_mate() {
        let mut b = Board::default();
        assert!(mv(&mut b, "f2", "f3"));
        assert!(mv(&mut b, "e7", "e5"));
        assert!(mv(&mut b, "g2", "g4"));
        assert!(mv(&mut b, "d8", "h4"));
        assert!(b.is_mated());
        assert!(!b.is_draw());
    }

    #[test]
    fn illegal_moves_are_refused() {
        let mut b = Board::default();
        assert!(!mv(&mut b, "e2", "e5"));
        assert!(!mv(&mut b, "e7", "e5"), "not black's turn");
        assert!(!mv(&mut b, "b1", "d2"), "own pawn there");
    }

    #[test]
    fn castling_and_en_passant() {
        let mut b = Board::default();
        for (f, t) in [
            ("e2", "e4"),
            ("a7", "a6"),
            ("g1", "f3"),
            ("a6", "a5"),
            ("f1", "c4"),
            ("a5", "a4"),
        ] {
            assert!(mv(&mut b, f, t), "{f}{t}");
        }
        assert!(mv(&mut b, "e1", "g1"));
        assert_eq!(b.piece(5).map(|p| p.kind), Some(PieceType::Rook));
        assert_eq!(b.piece(6).map(|p| p.kind), Some(PieceType::King));
        // A rook leaving its corner with only the other side's right left clears it (chesslib).
        let mut r = Board::default();
        for (f, t) in [("a2", "a4"), ("h7", "h6"), ("a1", "a3"), ("h6", "h5")] {
            assert!(mv(&mut r, f, t));
        }
        assert_eq!(r.castle[0], CastleRight::KingSide);
        assert!(mv(&mut r, "a3", "a1"));
        assert!(mv(&mut r, "h5", "h4"));
        assert!(mv(&mut r, "a1", "a2"));
        assert_eq!(r.castle[0], CastleRight::None);
    }

    #[test]
    fn en_passant_capture() {
        let mut b = Board::default();
        for (f, t) in [("e2", "e4"), ("a7", "a6"), ("e4", "e5"), ("d7", "d5")] {
            assert!(mv(&mut b, f, t));
        }
        assert!(mv(&mut b, "e5", "d6"));
        assert_eq!(b.piece(parse_square("d5").flatten().unwrap()), None);
    }

    #[test]
    fn repetition_and_material_draws() {
        let moves = [("g1", "f3"), ("g8", "f6"), ("f3", "g1"), ("f6", "g8")];
        // Havana's board keeps no history: never a repetition.
        let mut b = Board::default();
        for (f, t) in moves.iter().cycle().take(16) {
            assert!(mv(&mut b, f, t));
            assert!(!b.is_draw());
        }
        // With chesslib's history on (`Board(context, true)`).
        let mut b = Board {
            update_history: true,
            ..Board::default()
        };
        for (n, (f, t)) in moves.iter().cycle().take(9).enumerate() {
            assert!(!b.is_draw());
            assert!(mv(&mut b, f, t));
            // chesslib does not count the start position (it is not in the history): the first
            // position seen three times is the one after Nf3, on the ninth move.
            assert_eq!(b.is_draw(), n == 8, "move {n}");
        }

        let mut k = Board::empty(Side::White);
        k.put("e1", Side::White, PieceType::King);
        k.put("e8", Side::Black, PieceType::King);
        k.put("c1", Side::White, PieceType::Bishop);
        assert!(k.is_draw());
        k.put("a1", Side::White, PieceType::Rook);
        assert!(!k.is_draw());
    }

    #[test]
    fn promotion_rank() {
        assert!(Board::is_promo_rank(
            Side::White,
            parse_square("a8").flatten()
        ));
        assert!(!Board::is_promo_rank(
            Side::White,
            parse_square("a1").flatten()
        ));
        assert!(!Board::is_promo_rank(Side::Black, None));
    }
}
