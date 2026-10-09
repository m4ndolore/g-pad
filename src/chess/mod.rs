//! Chess trainer: Lichess puzzles in three kinds (tactics, endgames,
//! openings), solved by tapping a piece and then its square.
//!
//! Nothing here judges legality. A puzzle's solution is a fixed list of moves,
//! so a move is right when it is the next one on the list, and the opponent's
//! replies come from the same list. The board only has to apply moves
//! faithfully: castling moves the rook, en passant removes the pawn behind,
//! and a promotion takes the piece the solution names.

pub mod draw;

/// The puzzle set, picked by scripts/chess-puzzles.py from the Lichess
/// puzzle database (CC0).
const PUZZLES: &str = include_str!("../../assets/chess/puzzles.txt");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    Tactic,
    Endgame,
    Opening,
}

impl Category {
    pub const ALL: [Category; 3] = [Category::Tactic, Category::Endgame, Category::Opening];

    pub fn label(self) -> &'static str {
        match self {
            Category::Tactic => "TACTICS",
            Category::Endgame => "ENDGAMES",
            Category::Opening => "OPENINGS",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "tactic" => Some(Category::Tactic),
            "endgame" => Some(Category::Endgame),
            "opening" => Some(Category::Opening),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Puzzle {
    pub category: Category,
    pub rating: u32,
    /// The motif, the endgame type, or the opening family.
    pub label: String,
    pub fen: String,
    /// The opponent's setup move, then the solution alternating sides.
    pub moves: Vec<Move>,
}

/// Every puzzle in the set; lines that do not parse are skipped.
pub fn puzzles() -> Vec<Puzzle> {
    PUZZLES.lines().filter_map(parse_puzzle).collect()
}

fn parse_puzzle(line: &str) -> Option<Puzzle> {
    let mut f = line.split('|');
    let category = Category::parse(f.next()?)?;
    let rating = f.next()?.parse().ok()?;
    let label = f.next()?.to_string();
    let fen = f.next()?.to_string();
    let moves: Vec<Move> = f.next()?.split_whitespace().map(Move::parse).collect::<Option<_>>()?;
    Board::from_fen(&fen)?;
    (moves.len() >= 2).then_some(Puzzle { category, rating, label, fen, moves })
}

/// A square, 0 = a1 through 63 = h8.
pub type Square = u8;

pub fn file(sq: Square) -> u8 {
    sq % 8
}

pub fn rank(sq: Square) -> u8 {
    sq / 8
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: Square,
    pub to: Square,
    /// The promotion piece as a lowercase letter, when there is one.
    pub promo: Option<u8>,
}

impl Move {
    /// A UCI move such as `e2e4` or `e7e8q`.
    pub fn parse(s: &str) -> Option<Move> {
        let b = s.as_bytes();
        if !(4..=5).contains(&b.len()) {
            return None;
        }
        let sq = |f: u8, r: u8| -> Option<Square> {
            ((b'a'..=b'h').contains(&f) && (b'1'..=b'8').contains(&r)).then(|| (r - b'1') * 8 + (f - b'a'))
        };
        let promo = match b.get(4) {
            Some(&p) if b"qrbn".contains(&p) => Some(p),
            Some(_) => return None,
            None => None,
        };
        Some(Move { from: sq(b[0], b[1])?, to: sq(b[2], b[3])?, promo })
    }
}

/// Pieces as FEN letters: uppercase white, lowercase black, 0 for empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub squares: [u8; 64],
    pub white_to_move: bool,
}

impl Board {
    pub fn from_fen(fen: &str) -> Option<Board> {
        let mut parts = fen.split_whitespace();
        let placement = parts.next()?;
        let side = parts.next()?;
        let mut squares = [0u8; 64];
        let rows: Vec<&str> = placement.split('/').collect();
        if rows.len() != 8 {
            return None;
        }
        for (i, row) in rows.iter().enumerate() {
            let r = 7 - i as u8;
            let mut f = 0u8;
            for c in row.bytes() {
                if c.is_ascii_digit() {
                    f += c - b'0';
                } else if b"pnbrqkPNBRQK".contains(&c) {
                    if f > 7 {
                        return None;
                    }
                    squares[(r * 8 + f) as usize] = c;
                    f += 1;
                } else {
                    return None;
                }
            }
            if f != 8 {
                return None;
            }
        }
        let white_to_move = match side {
            "w" => true,
            "b" => false,
            _ => return None,
        };
        Some(Board { squares, white_to_move })
    }

    pub fn at(&self, sq: Square) -> u8 {
        self.squares[sq as usize]
    }

    /// Whether the piece on `sq` belongs to the side to move.
    pub fn is_own(&self, sq: Square) -> bool {
        let p = self.at(sq);
        p != 0 && p.is_ascii_uppercase() == self.white_to_move
    }

    pub fn apply(&mut self, m: Move) {
        let piece = self.at(m.from);
        let target = self.at(m.to);
        let kind = piece.to_ascii_lowercase();
        self.squares[m.from as usize] = 0;
        // Castling: the king moves two files, the rook jumps over it.
        if kind == b'k' && file(m.from).abs_diff(file(m.to)) == 2 {
            let r = rank(m.from);
            let (rook_from, rook_to) = if file(m.to) == 6 { (r * 8 + 7, r * 8 + 5) } else { (r * 8, r * 8 + 3) };
            self.squares[rook_to as usize] = self.at(rook_from);
            self.squares[rook_from as usize] = 0;
        }
        // En passant: a pawn moving diagonally onto an empty square takes
        // the pawn beside it.
        if kind == b'p' && file(m.from) != file(m.to) && target == 0 {
            self.squares[(rank(m.from) * 8 + file(m.to)) as usize] = 0;
        }
        self.squares[m.to as usize] = match m.promo {
            Some(p) if piece.is_ascii_uppercase() => p.to_ascii_uppercase(),
            Some(p) => p,
            None => piece,
        };
        self.white_to_move = !self.white_to_move;
    }
}

/// What a tap on the board did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    /// Nothing changed.
    None,
    /// A piece was picked up, or put down.
    Selected,
    /// The right move: the opponent's reply follows after a pause.
    Right,
    /// The last move of the solution.
    Solved,
    /// Not the move; the piece is put down.
    Wrong,
}

/// The rating the next puzzle is picked near moves this much per result.
pub const RATING_STEP: u32 = 40;
pub const RATING_MIN: u32 = 1400;
pub const RATING_MAX: u32 = 2300;

/// One puzzle in play, and the player's place in it.
pub struct Trainer {
    pub category: Category,
    /// Where puzzles are picked from: up after a clean solve, down after a miss.
    pub target: u32,
    pub puzzle: Puzzle,
    pub board: Board,
    /// Index into `puzzle.moves` of the move expected next.
    pub ply: usize,
    pub selected: Option<Square>,
    pub last: Option<Move>,
    /// The solver plays black, so the board is drawn from black's side.
    pub flipped: bool,
    pub missed: bool,
    pub hint: bool,
    pub solved: bool,
    pub status: String,
    all: Vec<Puzzle>,
    seed: u32,
}

impl Trainer {
    pub fn new(category: Category, target: u32, seed: u32) -> Option<Trainer> {
        let all = puzzles();
        let first = pick(&all, category, target, seed)?.clone();
        let mut t = Trainer {
            category,
            target: target.clamp(RATING_MIN, RATING_MAX),
            board: Board::from_fen(&first.fen)?,
            puzzle: first,
            ply: 0,
            selected: None,
            last: None,
            flipped: false,
            missed: false,
            hint: false,
            solved: false,
            status: String::new(),
            all,
            seed,
        };
        t.start();
        Some(t)
    }

    /// Set up the current puzzle and play the opponent's setup move.
    fn start(&mut self) {
        self.board = Board::from_fen(&self.puzzle.fen).expect("puzzles parse at load");
        let setup = self.puzzle.moves[0];
        self.board.apply(setup);
        self.last = Some(setup);
        self.ply = 1;
        self.flipped = !self.board.white_to_move;
        self.selected = None;
        self.missed = false;
        self.hint = false;
        self.solved = false;
        self.status = format!("{} TO MOVE", if self.board.white_to_move { "WHITE" } else { "BLACK" });
    }

    /// Deal the next puzzle in `category`, near the target rating.
    pub fn next(&mut self, category: Category) {
        self.category = category;
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        if let Some(p) = pick(&self.all, category, self.target, self.seed) {
            self.puzzle = p.clone();
        }
        self.start();
    }

    /// The move the solver should play now.
    pub fn expected(&self) -> Option<Move> {
        (!self.solved).then(|| self.puzzle.moves.get(self.ply).copied()).flatten()
    }

    /// Show the square the right piece stands on. A hinted puzzle counts as
    /// missed for the rating.
    pub fn show_hint(&mut self) {
        if self.expected().is_some() {
            if !self.missed {
                self.missed = true;
                self.target = self.target.saturating_sub(RATING_STEP).max(RATING_MIN);
            }
            self.hint = true;
            self.selected = None;
            self.status = "MOVE THE MARKED PIECE".into();
        }
    }

    pub fn tap(&mut self, sq: Square) -> Tap {
        let Some(want) = self.expected() else { return Tap::None };
        match self.selected {
            None if self.board.is_own(sq) => {
                self.selected = Some(sq);
                Tap::Selected
            }
            None => Tap::None,
            Some(from) if from == sq => {
                self.selected = None;
                Tap::Selected
            }
            Some(_) if self.board.is_own(sq) => {
                self.selected = Some(sq);
                Tap::Selected
            }
            Some(from) => {
                self.selected = None;
                if from == want.from && sq == want.to {
                    self.board.apply(want);
                    self.last = Some(want);
                    self.hint = false;
                    self.ply += 1;
                    if self.ply >= self.puzzle.moves.len() {
                        self.finish();
                        Tap::Solved
                    } else {
                        self.status = "RIGHT".into();
                        Tap::Right
                    }
                } else {
                    if !self.missed {
                        self.missed = true;
                        self.target = self.target.saturating_sub(RATING_STEP).max(RATING_MIN);
                    }
                    self.status = "NOT THIS ONE. TRY AGAIN".into();
                    Tap::Wrong
                }
            }
        }
    }

    /// Play the opponent's reply after a right move.
    pub fn reply(&mut self) {
        if let Some(m) = self.expected() {
            self.board.apply(m);
            self.last = Some(m);
            self.ply += 1;
            self.status = "YOUR MOVE".into();
        }
    }

    fn finish(&mut self) {
        self.solved = true;
        if self.missed {
            self.status = "SOLVED. TAP NEXT".into();
        } else {
            self.target = (self.target + RATING_STEP).min(RATING_MAX);
            self.status = "SOLVED CLEANLY. TAP NEXT".into();
        }
    }
}

/// A puzzle of `category` near `target`: among the 40 nearest by rating, one
/// chosen by `seed`, so repeats are rare without tracking what was played.
fn pick(all: &[Puzzle], category: Category, target: u32, seed: u32) -> Option<&Puzzle> {
    let mut near: Vec<&Puzzle> = all.iter().filter(|p| p.category == category).collect();
    near.sort_by_key(|p| p.rating.abs_diff(target));
    near.truncate(40);
    let n = near.len();
    (n > 0).then(|| near[(seed as usize >> 4) % n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_puzzle_parses_and_each_kind_has_plenty() {
        let all = puzzles();
        assert_eq!(all.len(), PUZZLES.lines().count(), "a line failed to parse");
        for c in Category::ALL {
            assert!(all.iter().filter(|p| p.category == c).count() >= 300, "{c:?}");
        }
    }

    #[test]
    fn every_solution_moves_its_own_pieces() {
        for p in puzzles() {
            let mut b = Board::from_fen(&p.fen).unwrap();
            for m in &p.moves {
                assert!(b.is_own(m.from), "{} plays {m:?} from a square it does not own", p.fen);
                b.apply(*m);
            }
        }
    }

    #[test]
    fn castling_moves_the_rook_and_en_passant_takes_the_pawn() {
        let mut b = Board::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        b.apply(Move::parse("e1g1").unwrap());
        assert_eq!((b.at(6), b.at(5), b.at(7)), (b'K', b'R', 0));
        b.apply(Move::parse("e8c8").unwrap());
        assert_eq!((b.at(58), b.at(59), b.at(56)), (b'k', b'r', 0));

        let mut b = Board::from_fen("8/8/8/3pP3/8/8/8/4K2k w - d6 0 1").unwrap();
        b.apply(Move::parse("e5d6").unwrap());
        assert_eq!((b.at(43), b.at(35)), (b'P', 0), "the d5 pawn is taken");

        let mut b = Board::from_fen("8/4P3/8/8/8/8/8/4K2k w - - 0 1").unwrap();
        b.apply(Move::parse("e7e8n").unwrap());
        assert_eq!(b.at(60), b'N');
    }

    fn trainer_on(line: &str) -> Trainer {
        let p = parse_puzzle(line).unwrap();
        let mut t = Trainer::new(p.category, 1500, 1).unwrap();
        t.puzzle = p;
        t.start();
        t
    }

    #[test]
    fn a_puzzle_plays_through_with_replies_and_a_miss_lowers_the_target() {
        // Lichess 0000D: after Qd6 Rd8, Qxd8+ Bxd8 wins the rook.
        let line = "endgame|1529|endgame|5rk1/1p3ppp/pq3b2/8/8/1P1Q1N2/P4PPP/3R2K1 w - - 2 27|d3d6 f8d8 d6d8 f6d8";
        let mut t = trainer_on(line);
        // The setup move was white's, so black solves and the board flips.
        assert!(t.flipped);
        let sq = |s: &str| Move::parse(&format!("{s}a1")).unwrap().from;
        assert_eq!(t.tap(sq("a1")), Tap::None, "an empty square does nothing");
        assert_eq!(t.tap(sq("f8")), Tap::Selected);
        assert_eq!(t.tap(sq("e8")), Tap::Wrong);
        assert_eq!(t.target, 1500 - RATING_STEP);
        assert_eq!(t.tap(sq("f8")), Tap::Selected);
        assert_eq!(t.tap(sq("d8")), Tap::Right);
        t.reply();
        assert_eq!(t.board.at(sq("d8")), b'Q', "the queen took on d8");
        assert_eq!(t.tap(sq("f6")), Tap::Selected);
        assert_eq!(t.tap(sq("d8")), Tap::Solved);
        assert!(t.solved);
        assert_eq!(t.target, 1500 - RATING_STEP, "a missed solve does not raise the target");
    }

    #[test]
    fn a_clean_solve_raises_the_target_and_next_deals_the_chosen_kind() {
        let line = "endgame|1529|endgame|5rk1/1p3ppp/pq3b2/8/8/1P1Q1N2/P4PPP/3R2K1 w - - 2 27|d3d6 f8d8 d6d8 f6d8";
        let mut t = trainer_on(line);
        let sq = |s: &str| Move::parse(&format!("{s}a1")).unwrap().from;
        for (a, b) in [("f8", "d8"), ("f6", "d8")] {
            t.tap(sq(a));
            let r = t.tap(sq(b));
            if r == Tap::Right {
                t.reply();
            }
        }
        assert!(t.solved);
        assert_eq!(t.target, 1500 + RATING_STEP);
        t.next(Category::Opening);
        assert_eq!(t.puzzle.category, Category::Opening);
        assert!(!t.solved && t.ply == 1);
    }
}
