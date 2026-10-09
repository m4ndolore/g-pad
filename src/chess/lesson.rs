//! Kids' chess lessons, generated: a few pieces on an otherwise empty board
//! and exactly one right move. MOVES asks which piece can reach the star;
//! CAPTURE asks for the one black piece within reach. A lesson is about how
//! a piece moves, so check never matters and kings are just pieces here.

use super::{file, rank, Board, Category, Move, Puzzle, Square};

/// The pieces a lesson teaches, as white's FEN letters.
const PIECES: [u8; 6] = [b'N', b'B', b'R', b'Q', b'K', b'P'];
/// The black pieces a capture lesson offers up. No king: taking one is not
/// a thing a child should learn as a move.
const PREY: [u8; 5] = [b'p', b'n', b'b', b'r', b'q'];

/// Tries before a lesson falls back to a fixed one. Each try succeeds often
/// enough that the fallback is never seen in practice.
const TRIES: usize = 500;

/// Whether the piece on `from` can move to `to`, ignoring check. Pawns move
/// up the board for white and down for black, two squares from their first
/// rank, and capture one square diagonally forward.
pub fn reaches(b: &Board, from: Square, to: Square) -> bool {
    let p = b.at(from);
    if p == 0 || from == to {
        return false;
    }
    let white = p.is_ascii_uppercase();
    let t = b.at(to);
    if t != 0 && t.is_ascii_uppercase() == white {
        return false;
    }
    let df = file(to) as i32 - file(from) as i32;
    let dr = rank(to) as i32 - rank(from) as i32;
    match p.to_ascii_lowercase() {
        b'n' => matches!((df.abs(), dr.abs()), (1, 2) | (2, 1)),
        b'k' => df.abs().max(dr.abs()) == 1,
        b'r' => (df == 0 || dr == 0) && clear(b, from, df, dr),
        b'b' => df.abs() == dr.abs() && clear(b, from, df, dr),
        b'q' => (df == 0 || dr == 0 || df.abs() == dr.abs()) && clear(b, from, df, dr),
        _ => {
            let dir = if white { 1 } else { -1 };
            let home = if white { 1 } else { 6 };
            if t != 0 {
                df.abs() == 1 && dr == dir
            } else {
                df == 0 && (dr == dir || (dr == 2 * dir && rank(from) == home && clear(b, from, df, dr)))
            }
        }
    }
}

/// Every square strictly between `from` and `from + (df, dr)` is empty.
fn clear(b: &Board, from: Square, df: i32, dr: i32) -> bool {
    let (sf, sr) = (df.signum(), dr.signum());
    (1..df.abs().max(dr.abs())).all(|i| {
        let sq = (rank(from) as i32 + sr * i) * 8 + file(from) as i32 + sf * i;
        b.at(sq as Square) == 0
    })
}

pub fn piece_name(p: u8) -> &'static str {
    match p.to_ascii_lowercase() {
        b'k' => "KING",
        b'q' => "QUEEN",
        b'r' => "ROOK",
        b'b' => "BISHOP",
        b'n' => "KNIGHT",
        _ => "PAWN",
    }
}

/// A fresh MOVES or CAPTURE lesson from `seed`.
pub fn generate(category: Category, seed: u32) -> Puzzle {
    let mut rng = Rng(seed ^ 0x9E37_79B9);
    (0..TRIES)
        .find_map(|_| match category {
            Category::Capture => capture(&mut rng),
            _ => moves(&mut rng),
        })
        .unwrap_or_else(|| fallback(category))
}

/// Three white pieces of different kinds and a star on an empty square
/// exactly one of them can reach.
fn moves(rng: &mut Rng) -> Option<Puzzle> {
    let mut b = empty_board();
    let mut kinds = PIECES.to_vec();
    let mut own = Vec::new();
    for _ in 0..3 {
        let p = kinds.swap_remove(rng.below(kinds.len()));
        own.push(place(&mut b, p, rng));
    }
    let stars: Vec<Square> = (0..64)
        .filter(|&s| b.at(s) == 0 && own.iter().filter(|&&f| reaches(&b, f, s)).count() == 1)
        .collect();
    let goal = *stars.get(rng.below(stars.len().max(1)))?;
    let from = *own.iter().find(|&&f| reaches(&b, f, goal))?;
    Some(lesson(Category::Moves, b, from, goal, Some(goal)))
}

/// Two white pieces and three black ones, and exactly one capture between
/// them.
fn capture(rng: &mut Rng) -> Option<Puzzle> {
    let mut b = empty_board();
    let mut kinds = PIECES.to_vec();
    let own: Vec<Square> = (0..2)
        .map(|_| {
            let p = kinds.swap_remove(rng.below(kinds.len()));
            place(&mut b, p, rng)
        })
        .collect();
    let prey: Vec<Square> = (0..3).map(|_| place(&mut b, PREY[rng.below(PREY.len())], rng)).collect();
    let mut takes = own.iter().flat_map(|&f| prey.iter().map(move |&t| (f, t))).filter(|&(f, t)| reaches(&b, f, t));
    let (from, to) = takes.next()?;
    takes.next().is_none().then(|| lesson(Category::Capture, b, from, to, None))
}

/// A rook that can only reach the star, for the lesson that never comes.
fn fallback(category: Category) -> Puzzle {
    let mut b = empty_board();
    b.squares[0] = b'R';
    let to = if category == Category::Capture {
        b.squares[56] = b'p';
        56
    } else {
        32
    };
    let goal = (category != Category::Capture).then_some(to);
    lesson(category, b, 0, to, goal)
}

fn lesson(category: Category, start: Board, from: Square, to: Square, goal: Option<Square>) -> Puzzle {
    Puzzle {
        category,
        rating: 0,
        label: piece_name(start.at(from)).to_string(),
        start,
        moves: vec![Move { from, to, promo: None }],
        setup: false,
        goal,
    }
}

fn empty_board() -> Board {
    Board { squares: [0; 64], white_to_move: true }
}

/// Put `p` on a random empty square. Pawns stay off the first and last two
/// ranks, so no lesson move ever promotes.
fn place(b: &mut Board, p: u8, rng: &mut Rng) -> Square {
    loop {
        let sq = rng.below(64) as Square;
        let pawn_ok = !p.eq_ignore_ascii_case(&b'p') || (1..=5).contains(&rank(sq));
        if b.at(sq) == 0 && pawn_ok {
            b.squares[sq as usize] = p;
            return sq;
        }
    }
}

struct Rng(u32);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as usize % n.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chess::{Tap, Trainer};

    fn sq(s: &str) -> Square {
        Move::parse(&format!("{s}a1")).unwrap().from
    }

    fn board(fen: &str) -> Board {
        Board::from_fen(fen).unwrap()
    }

    #[test]
    fn pieces_move_the_way_the_rules_say() {
        let b = board("8/8/8/8/8/8/8/1N6 w - - 0 1");
        assert!(reaches(&b, sq("b1"), sq("c3")));
        assert!(reaches(&b, sq("b1"), sq("d2")));
        assert!(!reaches(&b, sq("b1"), sq("b3")));

        let b = board("8/8/8/8/8/8/1P6/R7 w - - 0 1");
        assert!(reaches(&b, sq("a1"), sq("a8")), "an open file");
        assert!(reaches(&b, sq("a1"), sq("h1")), "an open rank");
        assert!(!reaches(&b, sq("a1"), sq("b2")), "a rook never goes diagonally");
        assert!(reaches(&b, sq("b2"), sq("b4")), "a pawn steps two from home");
        assert!(!reaches(&b, sq("b2"), sq("b5")));

        let b = board("8/8/8/3p4/8/1B6/8/8 w - - 0 1");
        assert!(reaches(&b, sq("b3"), sq("d5")), "a bishop takes along its diagonal");
        assert!(!reaches(&b, sq("b3"), sq("e6")), "but not through the piece it meets");

        let b = board("8/8/8/8/3p4/2P1P3/8/8 w - - 0 1");
        assert!(reaches(&b, sq("c3"), sq("d4")), "pawns take diagonally");
        assert!(!reaches(&b, sq("c3"), sq("c5")), "and step two only from home");
        assert!(reaches(&b, sq("d4"), sq("c3")), "black pawns take down the board");
        assert!(!reaches(&b, sq("c3"), sq("e3")), "nobody takes their own side");
    }

    #[test]
    fn every_lesson_has_exactly_one_right_move() {
        for seed in 0..400 {
            let p = generate(Category::Moves, seed);
            let b = &p.start;
            let own: Vec<Square> = (0..64).filter(|&s| b.is_own(s)).collect();
            let goal = p.goal.expect("a moves lesson marks a star");
            assert_eq!(b.at(goal), 0, "the star sits on an empty square");
            assert_eq!(own.iter().filter(|&&f| reaches(b, f, goal)).count(), 1, "seed {seed}: {p:?}");
            assert_eq!(p.moves, vec![Move { from: p.moves[0].from, to: goal, promo: None }]);

            let p = generate(Category::Capture, seed);
            let b = &p.start;
            let takes: Vec<(Square, Square)> = (0..64)
                .filter(|&f| b.is_own(f))
                .flat_map(|f| (0..64).filter(move |&t| b.at(t) != 0 && !b.is_own(t)).map(move |t| (f, t)))
                .filter(|&(f, t)| reaches(b, f, t))
                .collect();
            assert_eq!(takes.len(), 1, "seed {seed}: {p:?}");
            assert_eq!((p.moves[0].from, p.moves[0].to), takes[0]);
            assert!(p.goal.is_none());
        }
    }

    #[test]
    fn a_lesson_plays_through_the_trainer() {
        let mut t = Trainer::kids(Category::Moves, 3).unwrap();
        assert!(!t.flipped, "lessons are played from white's side");
        assert!(t.last.is_none(), "no setup move before a lesson");
        let want = t.expected().unwrap();
        assert_eq!(t.tap(want.from), Tap::Selected);
        assert_eq!(t.tap(want.to), Tap::Solved);
        assert_eq!(t.status, "YES! GREAT MOVE!");
        t.next(Category::Capture);
        assert_eq!(t.puzzle.category, Category::Capture);
        let want = t.expected().unwrap();
        t.tap(want.from);
        assert_eq!(t.tap(want.to), Tap::Solved);
    }

    #[test]
    fn the_fallback_is_a_lesson_too() {
        for c in [Category::Moves, Category::Capture] {
            let p = fallback(c);
            assert!(reaches(&p.start, p.moves[0].from, p.moves[0].to));
        }
    }
}
