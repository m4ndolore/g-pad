//! Learn mode — the kids' tutor surface of anthink.
//!
//! A challenge renders, the child answers in their own hand, and anchored
//! marks carry the commands: a mark in DONE commits the answer region to the
//! oracle for marking, a mark in NEW deals a fresh page. Free-floating shape
//! recognition is never used; every consequential mark is a bbox lookup
//! against the hit map the sheet returned when it was drawn.
//!
//! The pedagogy is Singapore Math's concrete → pictorial → abstract ladder:
//! counting dots, then number bonds and ten-frames, then bare equations, then
//! arrays and sharing — plus handwriting practice on ruled lines. Problems
//! are generated locally; the oracle only ever READS the child's ink.

pub mod problems;
pub mod sheet;
pub mod verdict;

use ab_glyph::FontRef;

use crate::surface::Surface;

pub use sheet::{HitMap, Target};
pub use verdict::Verdict;

pub struct Session {
    ladder: problems::Ladder,
    rng: problems::Rng,
    rot: usize,
    pub problem: problems::Problem,
    pub hits: HitMap,
}

impl Session {
    pub fn start() -> Self {
        let level: u8 = std::env::var("RIDDLE_LEARN_LEVEL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        Self::start_at(level, now_seed())
    }

    pub fn start_at(level: u8, seed: u32) -> Self {
        let mut rng = problems::Rng::new(seed);
        let ladder = problems::Ladder::new(level);
        let problem = problems::generate(ladder.level, 0, &mut rng);
        Self { ladder, rng, rot: 0, problem, hits: empty_hits() }
    }

    /// Draw the current sheet and remember its hit map.
    pub fn draw(&mut self, surf: &mut Surface, ui_font: &FontRef) {
        self.hits = sheet::draw(surf, ui_font, &self.problem, self.ladder.level, self.ladder.streak());
    }

    pub fn level(&self) -> u8 {
        self.ladder.level
    }

    /// Deal the next problem (call `draw` afterwards).
    pub fn next(&mut self) {
        self.rot += 1;
        self.problem = problems::generate(self.ladder.level, self.rot, &mut self.rng);
    }

    /// A marked answer came back; move the ladder.
    pub fn record(&mut self, v: Verdict) {
        if let Some(correct) = v.counts_as_correct() {
            self.ladder.record(correct);
        }
    }

    /// The per-turn instruction that turns the pad's oracle into a tutor for
    /// exactly this exercise. Travels as user text, so it works unchanged on
    /// both oracle backends and never disturbs the pad's persona.
    pub fn instruction(&self) -> String {
        format!(
            "LEARN MODE. You are a warm, playful tutor for a child aged four to eight. \
             The page shows only the child's pen work for this exercise: {brief}. \
             Decide whether the child's writing shows the correct answer. \
             Your reply MUST start with exactly one word: YES if it is right, \
             ALMOST if it is nearly right (a reversed digit, shaky but readable letters), \
             NO if it is wrong, missing, or unreadable. \
             Then one short cheerful sentence for the child — at most twelve very simple words. \
             After NO, add a tiny hint but never the answer. \
             After YES, you may add one fun follow-up thought about the same numbers or word. \
             Never mention pictures, images, or cameras.",
            brief = self.problem.brief()
        )
    }
}

fn now_seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0x5EED)
}

fn empty_hits() -> HitMap {
    use crate::fb::BBox;
    HitMap { answer: BBox::empty(), done: BBox::empty(), new: BBox::empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_deals_a_new_problem_each_time() {
        let mut s = Session::start_at(2, 99);
        let first = s.problem.clone();
        s.next();
        assert_ne!(std::mem::discriminant(&first.kind), std::mem::discriminant(&s.problem.kind),
            "the rotation must change activity between consecutive pages");
    }

    #[test]
    fn the_instruction_names_the_protocol_and_the_answer() {
        let s = Session::start_at(1, 7);
        let i = s.instruction();
        assert!(i.contains("YES"));
        assert!(i.contains("ALMOST"));
        assert!(i.contains(&s.problem.expected()));
    }

    #[test]
    fn recording_yes_answers_eventually_raises_the_level() {
        let mut s = Session::start_at(1, 5);
        for _ in 0..3 {
            s.record(Verdict::Yes);
        }
        assert_eq!(s.ladder.level, 2);
        s.record(Verdict::Unknown); // must not move anything
        assert_eq!(s.ladder.level, 2);
    }
}
