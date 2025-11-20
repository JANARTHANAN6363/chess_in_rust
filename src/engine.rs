// Features:
// - 0x88 board representation
// - Move generation (legal-ish: handles captures, promotions, castling basics, en passant)
// - Alpha-beta search with iterative deepening
// - Transposition table with Zobrist hashing
// - Simple evaluation (material + piece-square tables)
// - Terminal UI: ASCII board, play vs engine or engine vs engine, make moves via UCI-like input

use crate::transposition::{NodeType, PackedMove, ProbeResult, TranspositionTable};
use crate::zobrist::Zobrist;
// use std::cmp::max;
use std::fmt;
use std::io::{self, Write};
use std::time::{Duration, Instant};

// =====================
// 0x88 Board Utilities
// =====================
pub type Sq = usize; // 0..127 but we'll use 0..128 with 0x88 tests
const BOARD_SIZE: usize = 128; // using 0x88

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Piece {
    Empty,
    WP,
    WN,
    WB,
    WR,
    WQ,
    WK,
    BP,
    BN,
    BB,
    BR,
    BQ,
    BK,
}

impl Piece {
    pub fn from_char(c: char) -> Piece {
        match c {
            'P' => Piece::WP,
            'N' => Piece::WN,
            'B' => Piece::WB,
            'R' => Piece::WR,
            'Q' => Piece::WQ,
            'K' => Piece::WK,
            'p' => Piece::BP,
            'n' => Piece::BN,
            'b' => Piece::BB,
            'r' => Piece::BR,
            'q' => Piece::BQ,
            'k' => Piece::BK,
            _ => Piece::Empty,
        }
    }
    pub fn to_char(self) -> char {
        match self {
            Piece::WP => 'P',
            Piece::WN => 'N',
            Piece::WB => 'B',
            Piece::WR => 'R',
            Piece::WQ => 'Q',
            Piece::WK => 'K',
            Piece::BP => 'p',
            Piece::BN => 'n',
            Piece::BB => 'b',
            Piece::BR => 'r',
            Piece::BQ => 'q',
            Piece::BK => 'k',
            Piece::Empty => '.',
        }
    }
    pub fn is_white(self) -> bool {
        match self {
            Piece::WP | Piece::WN | Piece::WB | Piece::WR | Piece::WQ | Piece::WK => true,
            _ => false,
        }
    }
    pub fn is_black(self) -> bool {
        match self {
            Piece::BP | Piece::BN | Piece::BB | Piece::BR | Piece::BQ | Piece::BK => true,
            _ => false,
        }
    }
    pub fn is_empty(self) -> bool {
        self == Piece::Empty
    }
}

// 0x88 helpers
fn on_board(s: Sq) -> bool {
    (s & 0x88) == 0
}
fn sq(rank: i32, file: i32) -> Sq {
    ((rank << 4) | file) as usize
}

// Convert 0x88 square to human algebraic like e2
fn sq_to_alg(s: Sq) -> String {
    let r = (s >> 4) as i32;
    let f = (s & 15) as i32;
    if r < 0 || r > 7 || f < 0 || f > 7 {
        return String::from("??");
    }
    let file = (b'a' + f as u8) as char;
    let rank = (1 + r).to_string();
    format!("{}{}", file, rank)
}

fn alg_to_sq(s: &str) -> Option<Sq> {
    let s = s.trim();
    if s.len() < 2 {
        return None;
    }
    let bytes = s.as_bytes();
    let f = (bytes[0] as char).to_ascii_lowercase();
    let rch = bytes[1] as char;
    if !(('a'..='h').contains(&f)) {
        return None;
    }
    if !(('1'..='8').contains(&rch)) {
        return None;
    }
    let file = (f as u8 - b'a') as i32;
    let rank = (rch as u8 - b'1') as i32;
    Some(sq(rank, file))
}

// =====================
// Board State
// =====================
#[derive(Clone)]
pub struct Board {
    pub cells: [Piece; BOARD_SIZE],
    pub side_white: bool, // true if white to move
    pub castling: u8,     // bits: 1 white K, 2 white Q, 4 black k, 8 black q
    pub ep: Option<Sq>,   // en passant square
    pub halfmove_clock: u32,
    pub fullmove: u32,
    history: Vec<Undo>,
    redo_stack: Vec<Undo>, // ADD THIS: stores undone moves
}

#[derive(Clone, Copy)]
struct Undo {
    mv_from: Sq,
    mv_to: Sq,
    moved_piece: Piece, // NEW: Store what piece was moved
    captured: Piece,
    prev_castling: u8,
    prev_ep: Option<Sq>,
    prev_halfmove: u32,
    promotion: Option<Piece>, // NEW: Store if there was a promotion
}

impl Board {
    fn empty() -> Board {
        Board {
            cells: [Piece::Empty; BOARD_SIZE],
            side_white: true,
            castling: 0,
            ep: None,
            halfmove_clock: 0,
            fullmove: 1,
            history: Vec::new(),
            redo_stack: Vec::new(), // for redo
        }
    }

    pub fn from_fen(fen: &str) -> Board {
        let mut b = Board::empty();
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.is_empty() {
            return b;
        }

        // board
        let ranks: Vec<&str> = parts[0].split('/').collect();
        for (r, rank_str) in ranks.iter().enumerate() {
            let rank = 7 - r as i32;
            let mut file = 0i32;
            for ch in rank_str.chars() {
                if ch.is_digit(10) {
                    file += ch.to_digit(10).unwrap() as i32;
                } else {
                    let sqi = sq(rank, file);
                    b.cells[sqi] = Piece::from_char(ch);
                    file += 1;
                }
            }
        }
        // side
        if parts.len() > 1 {
            b.side_white = parts[1] == "w"
        }
        // castling
        b.castling = 0;
        if parts.len() > 2 {
            let c = parts[2];
            if c.contains('K') {
                b.castling |= 1
            }
            if c.contains('Q') {
                b.castling |= 2
            }
            if c.contains('k') {
                b.castling |= 4
            }
            if c.contains('q') {
                b.castling |= 8
            }
        }
        // ep
        b.ep = None;
        if parts.len() > 3 {
            if parts[3] != "-" {
                b.ep = alg_to_sq(parts[3])
            }
        }
        if parts.len() > 4 {
            b.halfmove_clock = parts[4].parse().unwrap_or(0)
        }
        if parts.len() > 5 {
            b.fullmove = parts[5].parse().unwrap_or(1)
        }
        b
    }
    #[allow(dead_code)]
    fn to_fen(&self) -> String {
        let mut s = String::new();
        for r in (0..8).rev() {
            let mut empty = 0;
            for f in 0..8 {
                let p = self.cells[sq(r, f)];
                if p.is_empty() {
                    empty += 1;
                } else {
                    if empty > 0 {
                        s.push_str(&empty.to_string());
                        empty = 0;
                    }
                    s.push(p.to_char());
                }
            }
            if empty > 0 {
                s.push_str(&empty.to_string());
            }
            if r > 0 {
                s.push('/')
            }
        }
        s.push(' ');
        s.push(if self.side_white { 'w' } else { 'b' });
        s.push(' ');
        let mut cast = String::new();
        if self.castling & 1 != 0 {
            cast.push('K')
        }
        if self.castling & 2 != 0 {
            cast.push('Q')
        }
        if self.castling & 4 != 0 {
            cast.push('k')
        }
        if self.castling & 8 != 0 {
            cast.push('q')
        }
        if cast.is_empty() {
            cast.push('-')
        }
        s.push_str(&cast);
        s.push(' ');
        if let Some(e) = self.ep {
            s.push_str(&sq_to_alg(e));
        } else {
            s.push('-')
        }
        s.push(' ');
        s.push_str(&self.halfmove_clock.to_string());
        s.push(' ');
        s.push_str(&self.fullmove.to_string());
        s
    }
    fn print_board(&self) {
        println!("  +------------------------+");
        for r in (0..8).rev() {
            print!("{} |", r + 1);
            for f in 0..8 {
                let p = self.cells[sq(r, f)];
                print!(" {}", p.to_char());
            }
            println!(" |");
        }
        println!("  +------------------------+");
        println!("    a b c d e f g h");
        println!(
            "Side: {}  Castling: {}{}{}{}  EP: {}  Halfmove: {}  Fullmove: {}",
            if self.side_white { "White" } else { "Black" },
            if self.castling & 1 != 0 { "K" } else { "-" },
            if self.castling & 2 != 0 { "Q" } else { "-" },
            if self.castling & 4 != 0 { "k" } else { "-" },
            if self.castling & 8 != 0 { "q" } else { "-" },
            match self.ep {
                Some(s) => sq_to_alg(s),
                None => String::from("-"),
            },
            self.halfmove_clock,
            self.fullmove
        );
    }
    fn piece_at(&self, s: Sq) -> Piece {
        self.cells[s]
    }
    #[allow(dead_code)]
    fn set_piece(&mut self, s: Sq, p: Piece) {
        self.cells[s] = p
    }
    fn find_king(&self, white: bool) -> Option<Sq> {
        for r in 0..8 {
            for f in 0..8 {
                let s = sq(r, f);
                let p = self.cells[s];
                if (white && p == Piece::WK) || (!white && p == Piece::BK) {
                    return Some(s);
                }
            }
        }
        None
    }
    // Make a move (no validation here) and save undo
    pub fn make_move(&mut self, from: Sq, to: Sq, promotion: Option<Piece>) {
        let captured = self.cells[to];
        let prev_cast = self.castling;
        let prev_ep = self.ep;
        let prev_half = self.halfmove_clock;
        let moved_piece = self.cells[from]; // STORE the moving piece

        // handle special: en passant capture
        let mut _actual_captured = captured;
        if let Some(ep_sq) = self.ep {
            // pawn moved to ep square capturing pawn
            if self.cells[from] == Piece::WP && to == ep_sq && (from >> 4) == 4 {
                // white ep capture
                let cap_sq = to - 16;
                _actual_captured = self.cells[cap_sq];
                self.cells[cap_sq] = Piece::Empty;
            } else if self.cells[from] == Piece::BP && to == ep_sq && (from >> 4) == 3 {
                // black ep capture
                let cap_sq = to + 16;
                _actual_captured = self.cells[cap_sq];
                self.cells[cap_sq] = Piece::Empty;
            }
        }

        // move piece
        let mut moving = self.cells[from];
        self.cells[from] = Piece::Empty;

        // promotions
        if let Some(prom) = promotion {
            moving = prom;
        }
        self.cells[to] = moving;

        // update castling rights if king or rook moved/captured
        match from {
            f if f == sq(0, 4) && self.cells[to] == Piece::BK => {}
            _ => {}
        }

        // crude castling update
        if moved_piece == Piece::WK {
            self.castling &= !(1 | 2);
        }
        if moved_piece == Piece::BK {
            self.castling &= !(4 | 8);
        }

        // if rook captured or moved
        if from == sq(0, 0) || to == sq(0, 0) {
            self.castling &= !2;
        }
        if from == sq(0, 7) || to == sq(0, 7) {
            self.castling &= !1;
        }
        if from == sq(7, 0) || to == sq(7, 0) {
            self.castling &= !8;
        }
        if from == sq(7, 7) || to == sq(7, 7) {
            self.castling &= !4;
        }

        // handle castling move proper: move rook
        // white castling
        if moved_piece == Piece::WK && from == sq(0, 4) && to == sq(0, 6) {
            // white kingside
            self.cells[sq(0, 7)] = Piece::Empty;
            self.cells[sq(0, 5)] = Piece::WR;
        } else if moved_piece == Piece::WK && from == sq(0, 4) && to == sq(0, 2) {
            // white queenside
            self.cells[sq(0, 0)] = Piece::Empty;
            self.cells[sq(0, 3)] = Piece::WR;
        }

        // black castling
        if moved_piece == Piece::BK && from == sq(7, 4) && to == sq(7, 6) {
            self.cells[sq(7, 7)] = Piece::Empty;
            self.cells[sq(7, 5)] = Piece::BR;
        } else if moved_piece == Piece::BK && from == sq(7, 4) && to == sq(7, 2) {
            self.cells[sq(7, 0)] = Piece::Empty;
            self.cells[sq(7, 3)] = Piece::BR;
        }

        // update en passant target
        self.ep = None;
        if moved_piece == Piece::WP && (to >> 4) - (from >> 4) == 2 {
            self.ep = Some(from + 16);
        }
        if moved_piece == Piece::BP && (from >> 4) - (to >> 4) == 2 {
            self.ep = Some(from - 16);
        }

        // halfmove clock
        if moved_piece == Piece::WP || moved_piece == Piece::BP || !_actual_captured.is_empty() {
            self.halfmove_clock = 0
        } else {
            self.halfmove_clock += 1
        }

        if !self.side_white {
            self.fullmove += 1
        }

        // flip side
        self.side_white = !self.side_white;

        // record undo with ALL necessary information
        self.history.push(Undo {
            mv_from: from,
            mv_to: to,
            moved_piece, // STORE original piece
            captured: _actual_captured,
            prev_castling: prev_cast,
            prev_ep: prev_ep,
            prev_halfmove: prev_half,
            promotion, // STORE promotion
        });

        // Clear redo stack when new move is made
        self.redo_stack.clear();
    }

    pub fn undo_move(&mut self) {
        if let Some(u) = self.history.pop() {
            // Save current state for redo BEFORE we modify anything
            let redo_entry = Undo {
                mv_from: u.mv_from,
                mv_to: u.mv_to,
                moved_piece: u.moved_piece,
                captured: u.captured,
                prev_castling: self.castling,       // Current castling
                prev_ep: self.ep,                   // Current EP
                prev_halfmove: self.halfmove_clock, // Current halfmove
                promotion: u.promotion,
            };
            self.redo_stack.push(redo_entry);

            // Now undo the move
            // 1. Flip side back
            self.side_white = !self.side_white;

            // 2. Restore fullmove counter
            if self.side_white {
                // If we're back to white's turn, decrement fullmove
                self.fullmove = self.fullmove.saturating_sub(1).max(1);
            }

            // 3. Get the piece that's currently on the destination square
            let _piece_on_to = self.cells[u.mv_to];

            // 4. Restore the original piece to source square
            // If there was a promotion, restore the original pawn
            let original_piece = if u.promotion.is_some() {
                u.moved_piece // This is the pawn before promotion
            } else {
                u.moved_piece
            };
            self.cells[u.mv_from] = original_piece;

            // 5. Restore captured piece (or empty square)
            self.cells[u.mv_to] = u.captured;

            // 6. Handle en passant capture undo
            if let Some(ep_sq) = u.prev_ep {
                if u.moved_piece == Piece::WP && u.mv_to == ep_sq && (u.mv_from >> 4) == 4 {
                    // White en passant - restore black pawn
                    let cap_sq = u.mv_to - 16;
                    self.cells[cap_sq] = Piece::BP;
                    self.cells[u.mv_to] = Piece::Empty;
                } else if u.moved_piece == Piece::BP && u.mv_to == ep_sq && (u.mv_from >> 4) == 3 {
                    // Black en passant - restore white pawn
                    let cap_sq = u.mv_to + 16;
                    self.cells[cap_sq] = Piece::WP;
                    self.cells[u.mv_to] = Piece::Empty;
                }
            }

            // 7. Undo castling rook move
            if u.moved_piece == Piece::WK && u.mv_from == sq(0, 4) {
                if u.mv_to == sq(0, 6) {
                    // White kingside
                    self.cells[sq(0, 5)] = Piece::Empty;
                    self.cells[sq(0, 7)] = Piece::WR;
                } else if u.mv_to == sq(0, 2) {
                    // White queenside
                    self.cells[sq(0, 3)] = Piece::Empty;
                    self.cells[sq(0, 0)] = Piece::WR;
                }
            } else if u.moved_piece == Piece::BK && u.mv_from == sq(7, 4) {
                if u.mv_to == sq(7, 6) {
                    // Black kingside
                    self.cells[sq(7, 5)] = Piece::Empty;
                    self.cells[sq(7, 7)] = Piece::BR;
                } else if u.mv_to == sq(7, 2) {
                    // Black queenside
                    self.cells[sq(7, 3)] = Piece::Empty;
                    self.cells[sq(7, 0)] = Piece::BR;
                }
            }

            // 8. Restore previous state
            self.castling = u.prev_castling;
            self.ep = u.prev_ep;
            self.halfmove_clock = u.prev_halfmove;
        }
    }

    pub fn redo_move(&mut self) {
        if let Some(u) = self.redo_stack.pop() {
            // Get current state
            let moved_piece = self.cells[u.mv_from];
            let captured = self.cells[u.mv_to];
            let prev_cast = self.castling;
            let prev_ep = self.ep;
            let prev_half = self.halfmove_clock;

            // Store for future undo
            self.history.push(Undo {
                mv_from: u.mv_from,
                mv_to: u.mv_to,
                moved_piece,
                captured,
                prev_castling: prev_cast,
                prev_ep: prev_ep,
                prev_halfmove: prev_half,
                promotion: u.promotion,
            });

            // Handle en passant capture in redo
            let mut _actual_captured = captured;
            if let Some(ep_sq) = self.ep {
                if moved_piece == Piece::WP && u.mv_to == ep_sq && (u.mv_from >> 4) == 4 {
                    let cap_sq = u.mv_to - 16;
                    _actual_captured = self.cells[cap_sq];
                    self.cells[cap_sq] = Piece::Empty;
                } else if moved_piece == Piece::BP && u.mv_to == ep_sq && (u.mv_from >> 4) == 3 {
                    let cap_sq = u.mv_to + 16;
                    _actual_captured = self.cells[cap_sq];
                    self.cells[cap_sq] = Piece::Empty;
                }
            }

            // Move the piece
            let mut moving = self.cells[u.mv_from];
            self.cells[u.mv_from] = Piece::Empty;

            // Handle promotion
            if let Some(prom) = u.promotion {
                moving = prom;
            }
            self.cells[u.mv_to] = moving;

            // Restore castling rights from redo entry
            self.castling = u.prev_castling;

            // Handle castling rook move
            if moved_piece == Piece::WK && u.mv_from == sq(0, 4) {
                if u.mv_to == sq(0, 6) {
                    self.cells[sq(0, 7)] = Piece::Empty;
                    self.cells[sq(0, 5)] = Piece::WR;
                } else if u.mv_to == sq(0, 2) {
                    self.cells[sq(0, 0)] = Piece::Empty;
                    self.cells[sq(0, 3)] = Piece::WR;
                }
            } else if moved_piece == Piece::BK && u.mv_from == sq(7, 4) {
                if u.mv_to == sq(7, 6) {
                    self.cells[sq(7, 7)] = Piece::Empty;
                    self.cells[sq(7, 5)] = Piece::BR;
                } else if u.mv_to == sq(7, 2) {
                    self.cells[sq(7, 0)] = Piece::Empty;
                    self.cells[sq(7, 3)] = Piece::BR;
                }
            }

            // Restore en passant from redo entry
            self.ep = u.prev_ep;

            // Restore halfmove clock from redo entry
            self.halfmove_clock = u.prev_halfmove;

            // Flip side
            self.side_white = !self.side_white;

            // Update fullmove
            if !self.side_white {
                self.fullmove += 1;
            }
        }
    }

    // Make a clone and play move, used by search
    fn make_move_clone(&self, from: Sq, to: Sq, promotion: Option<Piece>) -> Board {
        // associated functions are those in impl or trait definitions
        let mut b = self.clone();
        b.make_move(from, to, promotion);
        b
    }
}

// =====================
// Move Representation
// =====================
#[derive(Clone, Copy, Debug)]
pub struct Move {
    pub from: Sq,
    pub to: Sq,
    pub promotion: Option<Piece>,
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(p) = self.promotion {
            write!(
                f,
                "{}{}{}",
                sq_to_alg(self.from),
                sq_to_alg(self.to),
                p.to_char()
            )
        } else {
            write!(f, "{}{}", sq_to_alg(self.from), sq_to_alg(self.to))
        }
    }
}

// =====================
// Move Generation
// =====================
// knight jumps and king deltas
const KNIGHT_DELTAS: [i32; 8] = [33, 31, 18, 14, -33, -31, -18, -14];
const KING_DELTAS: [i32; 8] = [16, 1, -16, -1, 17, 15, -15, -17];
const ROOK_DELTAS: [i32; 4] = [16, 1, -16, -1];
const BISHOP_DELTAS: [i32; 4] = [17, 15, -17, -15];

pub fn gen_moves(board: &Board, moves: &mut Vec<Move>) {
    moves.clear();
    let white = board.side_white;
    for r in 0..8 {
        for f in 0..8 {
            let s = sq(r, f);
            let p = board.piece_at(s);
            if p.is_empty() {
                continue;
            }
            if white && !p.is_white() {
                continue;
            }
            if !white && !p.is_black() {
                continue;
            }
            match p {
                Piece::WP => gen_pawn_moves(board, s, true, moves),
                Piece::BP => gen_pawn_moves(board, s, false, moves),
                Piece::WN | Piece::BN => gen_leaper_moves(board, s, &KNIGHT_DELTAS, moves),
                Piece::WB | Piece::BB => gen_slider_moves(board, s, &BISHOP_DELTAS, moves),
                Piece::WR | Piece::BR => gen_slider_moves(board, s, &ROOK_DELTAS, moves),
                Piece::WQ | Piece::BQ => {
                    gen_slider_moves(board, s, &ROOK_DELTAS, moves);
                    gen_slider_moves(board, s, &BISHOP_DELTAS, moves);
                }
                Piece::WK | Piece::BK => gen_leaper_moves(board, s, &KING_DELTAS, moves),
                _ => {}
            }
        }
    }
    // add promotion handling is inside pawn function
    // castling: naive check
    gen_castling(board, moves);
    // filter illegal by checking king in check after move
    let mut legal = Vec::new();
    for &m in moves.iter() {
        let b2 = board.make_move_clone(m.from, m.to, m.promotion);
        if !is_king_attacked(&b2, !board.side_white) {
            legal.push(m);
        }
    }
    *moves = legal;
}

fn gen_leaper_moves(board: &Board, s: Sq, deltas: &[i32], moves: &mut Vec<Move>) {
    let side_white = board.side_white;
    for &d in deltas.iter() {
        let ns = (s as i32 + d) as usize;
        if !on_board(ns) {
            continue;
        }
        let p = board.piece_at(ns);
        if p.is_empty() || (side_white && p.is_black()) || (!side_white && p.is_white()) {
            moves.push(Move {
                from: s,
                to: ns,
                promotion: None,
            });
        }
    }
}

fn gen_slider_moves(board: &Board, s: Sq, deltas: &[i32], moves: &mut Vec<Move>) {
    let side_white = board.side_white;
    for &d in deltas.iter() {
        let mut ns = s as i32 + d;
        while ns >= 0 && on_board(ns as usize) {
            let nsu = ns as usize;
            let p = board.piece_at(nsu);
            if p.is_empty() {
                moves.push(Move {
                    from: s,
                    to: nsu,
                    promotion: None,
                });
            } else {
                if (side_white && p.is_black()) || (!side_white && p.is_white()) {
                    moves.push(Move {
                        from: s,
                        to: nsu,
                        promotion: None,
                    });
                }
                break;
            }
            ns += d;
        }
    }
}

fn gen_pawn_moves(board: &Board, s: Sq, white: bool, moves: &mut Vec<Move>) {
    let dir = if white { 16 } else { -16 };
    let start_rank = if white { 1 } else { 6 };
    let r = (s >> 4) as i32;
    let one = (s as i32 + dir) as usize;
    // single push
    if on_board(one) && board.piece_at(one).is_empty() {
        // promotion?
        if (white && (one >> 4) == 7) || (!white && (one >> 4) == 0) {
            // promote to Q, R, B, N
            moves.push(Move {
                from: s,
                to: one,
                promotion: Some(if white { Piece::WQ } else { Piece::BQ }),
            });
            moves.push(Move {
                from: s,
                to: one,
                promotion: Some(if white { Piece::WR } else { Piece::BR }),
            });
            moves.push(Move {
                from: s,
                to: one,
                promotion: Some(if white { Piece::WB } else { Piece::BB }),
            });
            moves.push(Move {
                from: s,
                to: one,
                promotion: Some(if white { Piece::WN } else { Piece::BN }),
            });
        } else {
            moves.push(Move {
                from: s,
                to: one,
                promotion: None,
            });
            // double push
            if r == start_rank {
                let two = (s as i32 + dir * 2) as usize;
                if on_board(two) && board.piece_at(two).is_empty() {
                    moves.push(Move {
                        from: s,
                        to: two,
                        promotion: None,
                    });
                }
            }
        }
    }
    // captures
    for cap_dir in [dir + 1, dir - 1].iter() {
        let ns = (s as i32 + cap_dir) as i32;
        if ns < 0 {
            continue;
        }
        let nsu = ns as usize;
        if !on_board(nsu) {
            continue;
        }
        let p = board.piece_at(nsu);
        if !p.is_empty() && ((white && p.is_black()) || (!white && p.is_white())) {
            if (white && (nsu >> 4) == 7) || (!white && (nsu >> 4) == 0) {
                moves.push(Move {
                    from: s,
                    to: nsu,
                    promotion: Some(if white { Piece::WQ } else { Piece::BQ }),
                });
                moves.push(Move {
                    from: s,
                    to: nsu,
                    promotion: Some(if white { Piece::WR } else { Piece::BR }),
                });
                moves.push(Move {
                    from: s,
                    to: nsu,
                    promotion: Some(if white { Piece::WB } else { Piece::BB }),
                });
                moves.push(Move {
                    from: s,
                    to: nsu,
                    promotion: Some(if white { Piece::WN } else { Piece::BN }),
                });
            } else {
                moves.push(Move {
                    from: s,
                    to: nsu,
                    promotion: None,
                });
            }
        }
    }
    // en passant
    if let Some(ep) = board.ep {
        if (ep == (s + 15) || ep == (s + 17) || ep == (s - 15) || ep == (s - 17)) && on_board(ep) {
            // ensure correct rank relation
            // playable: pawn diagonally behind ep square
            if white {
                if (ep >> 4) == 5
                    && (s >> 4) == 4
                    && ((ep & 15) as i32 - (s & 15) as i32).abs() == 1
                {
                    moves.push(Move {
                        from: s,
                        to: ep,
                        promotion: None,
                    });
                }
            } else {
                if (ep >> 4) == 2
                    && (s >> 4) == 3
                    && ((ep & 15) as i32 - (s & 15) as i32).abs() == 1
                {
                    moves.push(Move {
                        from: s,
                        to: ep,
                        promotion: None,
                    });
                }
            }
        }
    }
}

fn gen_castling(board: &Board, moves: &mut Vec<Move>) {
    // naive: ensure squares empty and not attacked
    if board.side_white {
        if board.castling & 1 != 0 {
            // white king side: e1->g1 squares f1 g1 empty
            if board.piece_at(sq(0, 5)).is_empty() && board.piece_at(sq(0, 6)).is_empty() {
                moves.push(Move {
                    from: sq(0, 4),
                    to: sq(0, 6),
                    promotion: None,
                });
            }
        }
        if board.castling & 2 != 0 {
            if board.piece_at(sq(0, 3)).is_empty()
                && board.piece_at(sq(0, 2)).is_empty()
                && board.piece_at(sq(0, 1)).is_empty()
            {
                moves.push(Move {
                    from: sq(0, 4),
                    to: sq(0, 2),
                    promotion: None,
                });
            }
        }
    } else {
        if board.castling & 4 != 0 {
            if board.piece_at(sq(7, 5)).is_empty() && board.piece_at(sq(7, 6)).is_empty() {
                moves.push(Move {
                    from: sq(7, 4),
                    to: sq(7, 6),
                    promotion: None,
                });
            }
        }
        if board.castling & 8 != 0 {
            if board.piece_at(sq(7, 3)).is_empty()
                && board.piece_at(sq(7, 2)).is_empty()
                && board.piece_at(sq(7, 1)).is_empty()
            {
                moves.push(Move {
                    from: sq(7, 4),
                    to: sq(7, 2),
                    promotion: None,
                });
            }
        }
    }
}

// =====================
// Attack Detection
// =====================
fn is_square_attacked(board: &Board, s: Sq, by_white: bool) -> bool {
    // pawns
    if by_white {
        let attacks = [s as i32 - 17, s as i32 - 15];
        for &a in attacks.iter() {
            if a >= 0 && on_board(a as usize) {
                if board.piece_at(a as usize) == Piece::WP {
                    return true;
                }
            }
        }
    } else {
        let attacks = [s as i32 + 17, s as i32 + 15];
        for &a in attacks.iter() {
            if a >= 0 && on_board(a as usize) {
                if board.piece_at(a as usize) == Piece::BP {
                    return true;
                }
            }
        }
    }
    // knights
    for &d in KNIGHT_DELTAS.iter() {
        let a = s as i32 + d;
        if a >= 0 && on_board(a as usize) {
            let p = board.piece_at(a as usize);
            if (by_white && p == Piece::WN) || (!by_white && p == Piece::BN) {
                return true;
            }
        }
    }
    // sliders
    for &d in ROOK_DELTAS.iter() {
        let mut a = s as i32 + d;
        while a >= 0 && on_board(a as usize) {
            let p = board.piece_at(a as usize);
            if !p.is_empty() {
                if (by_white && (p == Piece::WR || p == Piece::WQ))
                    || (!by_white && (p == Piece::BR || p == Piece::BQ))
                {
                    return true;
                }
                break;
            }
            a += d;
        }
    }
    for &d in BISHOP_DELTAS.iter() {
        let mut a = s as i32 + d;
        while a >= 0 && on_board(a as usize) {
            let p = board.piece_at(a as usize);
            if !p.is_empty() {
                if (by_white && (p == Piece::WB || p == Piece::WQ))
                    || (!by_white && (p == Piece::BB || p == Piece::BQ))
                {
                    return true;
                }
                break;
            }
            a += d;
        }
    }
    // king
    for &d in KING_DELTAS.iter() {
        let a = s as i32 + d;
        if a >= 0 && on_board(a as usize) {
            let p = board.piece_at(a as usize);
            if (by_white && p == Piece::WK) || (!by_white && p == Piece::BK) {
                return true;
            }
        }
    }
    false
}

pub fn is_king_attacked(board: &Board, white_king: bool) -> bool {
    if let Some(kpos) = board.find_king(white_king) {
        is_square_attacked(board, kpos, !white_king)
    } else {
        true
    }
}

// =====================
// Evaluation
// =====================
fn eval(board: &Board) -> i32 {
    // material values
    let mut score = 0i32;
    for r in 0..8 {
        for f in 0..8 {
            let s = sq(r, f);
            let p = board.piece_at(s);
            score += match p {
                Piece::WP => 100,
                Piece::WN => 320,
                Piece::WB => 330,
                Piece::WR => 500,
                Piece::WQ => 900,
                Piece::WK => 20000,
                Piece::BP => -100,
                Piece::BN => -320,
                Piece::BB => -330,
                Piece::BR => -500,
                Piece::BQ => -900,
                Piece::BK => -20000,
                _ => 0,
            };
        }
    }
    // side to move
    if !board.side_white {
        score = -score
    }
    score
}

// =====================
// Search
// =====================

struct SearchInfo {
    nodes: u64,
    start: Instant,
    time_limit: Option<Duration>,
    // best_move: Option<Move>,
    pub tt: TranspositionTable,
    pub zob: Zobrist,
}

fn piece_to_promo_id(p: Option<Piece>) -> u8 {
    match p {
        None => 0,
        Some(Piece::WQ) | Some(Piece::BQ) => 1,
        Some(Piece::WR) | Some(Piece::BR) => 2,
        Some(Piece::WB) | Some(Piece::BB) => 3,
        Some(Piece::WN) | Some(Piece::BN) => 4,
        _ => 0,
    }
}

fn piece_from_promo_id(id: u8) -> Option<Piece> {
    match id {
        1 => Some(Piece::WQ),
        2 => Some(Piece::WR),
        3 => Some(Piece::WB),
        4 => Some(Piece::WN),
        _ => None,
    }
}

fn negamax(
    board: &mut Board,
    depth: i32,
    ply: i32,
    alpha: i32,
    beta: i32,
    info: &mut SearchInfo,
) -> i32 {
    if let Some(limit) = info.time_limit {
        if info.start.elapsed() >= limit {
            return 0;
        }
    }
    info.nodes += 1;

    // TT probe
    let key = info.zob.hash_board(board);
    let mut tt_move: Option<Move> = None;
    match info.tt.probe(key, depth, alpha, beta) {
        ProbeResult::Usable(score, _best) => {
            return score;
        }
        ProbeResult::Found(entry) => {
            if entry.best != PackedMove::none() {
                let (from, to, promo) = entry.best.unpack();
                tt_move = Some(Move {
                    from,
                    to,
                    promotion: piece_from_promo_id(promo),
                });
            }
        }
        ProbeResult::Miss => {}
    }

    if depth == 0 {
        return quiescence(board, alpha, beta, info);
    }
    let mut a = alpha;
    let mut moves = Vec::new();
    gen_moves(board, &mut moves);
    if moves.is_empty() {
        // checkmate or stalemate
        if is_king_attacked(board, board.side_white) {
            return -100000 + ((100 - depth) as i32);
        } else {
            return 0;
        }
    }

    // ordering: TT move first, then captures
    if let Some(tm) = tt_move {
        if let Some(pos) = moves
            .iter()
            .position(|m| m.from == tm.from && m.to == tm.to)
        {
            let pv = moves.remove(pos);
            moves.insert(0, pv);
        }
    }
    moves.sort_by_key(|m| {
        if board.piece_at(m.to).is_empty() {
            0
        } else {
            1
        }
    });

    let mut best = -999999;
    let mut best_move_here: Option<Move> = None;

    for m in moves {
        board.make_move(m.from, m.to, m.promotion);
        let val = -negamax(board, depth - 1, ply + 1, -beta, -a, info);
        board.undo_move();

        if val > best {
            best = val;
            best_move_here = Some(m);
            // if depth == info.tt.probe(key, depth, alpha, beta).depth_for_root() {
            //     info.best_move = Some(m);
        }

        if val > a {
            a = val;
        }
        if a >= beta {
            break;
        }
    }

    // Store in TT
    let node_type = if best <= alpha {
        NodeType::UpperBound
    } else if best >= beta {
        NodeType::LowerBound
    } else {
        NodeType::Exact
    };

    let best_packed = if let Some(bm) = best_move_here {
        let promo_id = piece_to_promo_id(bm.promotion);
        Some((bm.from, bm.to, promo_id))
    } else {
        None
    };

    info.tt.store(key, depth, best, node_type, best_packed);

    best
}

fn quiescence(board: &mut Board, alpha: i32, beta: i32, info: &mut SearchInfo) -> i32 {
    if let Some(limit) = info.time_limit {
        if info.start.elapsed() >= limit {
            return 0;
        }
    }
    info.nodes += 1;
    let stand = eval(board);
    if stand >= beta {
        return beta;
    }
    let mut a = alpha;
    if stand > a {
        a = stand
    }
    // generate captures
    let mut moves = Vec::new();
    gen_moves(board, &mut moves);
    moves.retain(|m| !board.piece_at(m.to).is_empty());
    for m in moves {
        board.make_move(m.from, m.to, m.promotion);
        let score = -quiescence(board, -beta, -a, info);
        board.undo_move();
        if score >= beta {
            return beta;
        }
        if score > a {
            a = score
        }
    }
    a
}

fn search_root(board: &mut Board, max_depth: i32, time_limit_ms: Option<u64>) -> Option<Move> {
    let mut info = SearchInfo {
        nodes: 0,
        start: Instant::now(),
        time_limit: time_limit_ms.map(|ms| Duration::from_millis(ms)),
        // best_move: None,
        tt: TranspositionTable::new_buckets(1 << 18),
        zob: Zobrist::new(),
    };

    let mut best_move_overall = None;

    // Generate root moves once
    let mut root_moves = Vec::new();
    gen_moves(board, &mut root_moves);

    if root_moves.is_empty() {
        println!("No legal moves available!");
        return None;
    }

    println!("Root has {} legal moves", root_moves.len());

    for depth in 1..=max_depth {
        info.tt.new_search();

        let mut alpha = -1000000;
        let beta = 1000000;
        let mut best_score = -999999;
        let mut best_move_this_depth = None;

        // Search each root move
        for m in &root_moves {
            board.make_move(m.from, m.to, m.promotion);
            let val = -negamax(board, depth - 1, 1, -beta, -alpha, &mut info);
            board.undo_move();

            if val > best_score {
                best_score = val;
                best_move_this_depth = Some(*m);
            }

            if val > alpha {
                alpha = val;
            }

            // Check time limit
            if let Some(limit) = info.time_limit {
                if info.start.elapsed() >= limit {
                    println!("Time limit reached at depth {}!", depth);
                    if best_move_overall.is_some() {
                        return best_move_overall; // Return last complete depth
                    } else if best_move_this_depth.is_some() {
                        return best_move_this_depth; // Return incomplete depth if nothing else
                    }
                    return None;
                }
            }
        }

        // Update overall best move after completing this depth
        if let Some(m) = best_move_this_depth {
            best_move_overall = Some(m);
            println!(
                "depth={} score={} nodes={} move={} {}",
                depth,
                best_score,
                info.nodes,
                m,
                info.tt.stats()
            );
        } else {
            println!(
                "depth={} score={} nodes={} (no move found) {}",
                depth,
                best_score,
                info.nodes,
                info.tt.stats()
            );
        }
    }

    best_move_overall
}

// =====================
// CLI / Interaction
// =====================
fn print_help() {
    println!("Commands:");
    println!("  help                - this");
    println!("  fen <FEN>           - set position from FEN");
    println!("  board               - print board");
    println!("  go depth <n>        - engine thinks for n plies");
    println!("  go time <ms>        - engine thinks up to ms milliseconds");
    println!("  play                - play human vs engine (you play white)");
    println!("  move <e2e4>         - make a move in algebraic coords");
    println!("  undo                - undo last move");
    println!("  redo                - redo previously undone move");
    println!("  hash                - show Zobrist hash of current position");
    println!("  tt                  - show transposition table info");
    println!("  logout              - return to main menu"); // for logout
    println!("  quit                - exit");
}

pub fn ai_move(board: &mut Board, depth: i32, time_ms: Option<u64>) -> Option<Move> {
    search_root(board, depth, time_ms)
}

pub fn run() {
    println!("Rust Chess Engine — with Transposition Table + Zobrist. Type 'help' for commands.");
    let mut board = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    board.print_board();
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        let mut line = String::new();
        if stdin.read_line(&mut line).is_err() {
            break;
        }
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "help" => print_help(),
            "fen" => {
                if parts.len() > 1 {
                    let fen = parts[1..].join(" ");
                    board = Board::from_fen(&fen);
                    board.print_board();
                } else {
                    println!("usage: fen <FEN>")
                }
            }
            "board" => board.print_board(),
            "move" => {
                if parts.len() < 2 {
                    println!("usage: move e2e4");
                    continue;
                }
                let mv = parts[1];
                if mv.len() < 4 {
                    println!("bad move");
                    continue;
                }
                if let (Some(from), Some(to)) = (alg_to_sq(&mv[0..2]), alg_to_sq(&mv[2..4])) {
                    let promotion = if mv.len() >= 5 {
                        Some(Piece::from_char(mv.chars().nth(4).unwrap()))
                    } else {
                        None
                    };
                    board.make_move(from, to, promotion);
                    board.print_board();
                } else {
                    println!("bad squares")
                }
            }
            "undo" => {
                board.undo_move();
                board.print_board();
            }
            "redo" => {
                board.redo_move();
                board.print_board();
            }

            "go" => {
                if parts.len() < 3 {
                    println!("usage: go depth <n> | go time <ms>");
                    continue;
                }
                match parts[1] {
                    "depth" => {
                        if let Ok(d) = parts[2].parse::<i32>() {
                            let now = Instant::now();
                            if let Some(mv) = ai_move(&mut board, d, None) {
                                println!("engine -> {}", mv);
                                board.make_move(mv.from, mv.to, mv.promotion);
                                board.print_board();
                                println!("(took {:?})", now.elapsed());
                            } else {
                                println!("no move")
                            }
                        }
                    }
                    "time" => {
                        if let Ok(ms) = parts[2].parse::<u64>() {
                            let now = Instant::now();
                            if let Some(mv) = ai_move(&mut board, 6, Some(ms)) {
                                println!("engine -> {}", mv);
                                board.make_move(mv.from, mv.to, mv.promotion);
                                board.print_board();
                                println!("(took {:?})", now.elapsed());
                            } else {
                                println!("no move")
                            }
                        }
                    }
                    _ => println!("unknown go"),
                }
            }
            "play" => {
                println!("You are White. Enter moves like e2e4. Type 'resign' to stop.");
                board = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
                board.print_board();
                loop {
                    if board.side_white {
                        print!("white> ");
                        io::stdout().flush().unwrap();
                        let mut l = String::new();
                        if stdin.read_line(&mut l).is_err() {
                            break;
                        }
                        let t = l.trim();
                        if t == "resign" {
                            println!("You resigned.\n");
                            break;
                        }
                        if t.len() < 4 {
                            println!("bad move");
                            continue;
                        }
                        if let (Some(from), Some(to)) = (alg_to_sq(&t[0..2]), alg_to_sq(&t[2..4])) {
                            let promotion = if t.len() >= 5 {
                                Some(Piece::from_char(t.chars().nth(4).unwrap()))
                            } else {
                                None
                            };
                            board.make_move(from, to, promotion);
                            board.print_board();
                        } else {
                            println!("bad squares")
                        }
                    } else {
                        println!("Engine thinking...");
                        if let Some(mv) = ai_move(&mut board, 5, Some(500)) {
                            println!("engine -> {}", mv);
                            board.make_move(mv.from, mv.to, mv.promotion);
                            board.print_board();
                        } else {
                            println!("engine has no move");
                            break;
                        }
                    }
                }
            }
            "hash" => {
                let mut zob = Zobrist::new();
                let h = zob.hash_board(&board);
                println!("Zobrist hash = {}", h);
            }
            "tt" => {
                println!("TT stats: use 'go depth N' first to see per-depth stats");
                println!("(TT is created per search in current implementation)");
            }
            "logout" => {
                println!("Returning to main menu...");
                break; // Exit the engine loop, return to main menu
            }

            "quit" => break,
            _ => println!("unknown command, type 'help'"),
        }
    }
    println!("bye")
}

// WIP: For v0.2.0 (still testing, it's under serious development)
// Default branch changing from testing ==> main
