//! What Learn mode asks: graded exercises in the Singapore Math spirit —
//! concrete (count the dots), pictorial (number bonds, ten-frames, arrays),
//! abstract (equations) — plus handwriting practice on ruled lines.
//!
//! Generation is local and deterministic. The oracle is only ever asked to
//! READ a child's answer, never to invent the curriculum: a misread word costs
//! a retry, but a hallucinated lesson would cost the child's trust in the page.

/// One exercise, fully determined before it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Part-whole number bond: two parts joined to a whole, one circle blank.
    Bond { whole: u32, parts: [u32; 2], blank: Blank },
    /// A ten-frame with `shown` dots. `make_ten` asks for the missing count
    /// to complete ten; otherwise the child counts the dots.
    TenFrame { shown: u32, make_ten: bool },
    /// A bare equation with the answer blank: the abstract stage.
    Equation { a: u32, op: Op, b: u32 },
    /// Multiplication as equal rows of dots.
    Array { rows: u32, cols: u32 },
    /// Division as sharing: `total` dots dealt into `groups` equal rows.
    Share { total: u32, groups: u32 },
    /// Handwriting: trace the dashed word, then write it again below.
    Trace { word: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blank {
    Whole,
    Part(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

impl Op {
    pub fn glyph(self) -> char {
        match self {
            Op::Add => '+',
            Op::Sub => '-',
            Op::Mul => 'x',
            Op::Div => '/',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub kind: Kind,
    /// The printed instruction line at the top of the sheet.
    pub prompt: &'static str,
}

impl Problem {
    /// What should appear in the blank, as the child would write it.
    pub fn expected(&self) -> String {
        match &self.kind {
            Kind::Bond { whole, parts, blank } => match blank {
                Blank::Whole => whole.to_string(),
                Blank::Part(i) => parts[*i].to_string(),
            },
            Kind::TenFrame { shown, make_ten } => {
                if *make_ten { (10 - shown).to_string() } else { shown.to_string() }
            }
            Kind::Equation { a, op, b } => match op {
                Op::Add => (a + b).to_string(),
                Op::Sub => (a - b).to_string(),
                Op::Mul => (a * b).to_string(),
                Op::Div => (a / b).to_string(),
            },
            Kind::Array { rows, cols } => (rows * cols).to_string(),
            Kind::Share { total, groups } => (total / groups).to_string(),
            Kind::Trace { word } => (*word).to_string(),
        }
    }

    /// The exercise described for the tutor, so the oracle can judge the ink
    /// without ever being shown the whole sheet.
    pub fn brief(&self) -> String {
        let expected = self.expected();
        match &self.kind {
            Kind::Bond { whole, parts, blank } => match blank {
                Blank::Whole => format!(
                    "a number bond with parts {} and {}; the child wrote the whole (correct answer {expected})",
                    parts[0], parts[1]
                ),
                Blank::Part(i) => format!(
                    "a number bond with whole {whole} and one part {}; the child wrote the missing part (correct answer {expected})",
                    parts[1 - i]
                ),
            },
            Kind::TenFrame { shown, make_ten } => {
                if *make_ten {
                    format!("a ten-frame showing {shown} dots; the child wrote how many more make ten (correct answer {expected})")
                } else {
                    format!("a ten-frame; the child counted its dots and wrote the count (correct answer {expected})")
                }
            }
            Kind::Equation { a, op, b } => {
                format!("the equation {a} {} {b} = ___; the child wrote the answer (correct answer {expected})", op.glyph())
            }
            Kind::Array { rows, cols } => format!(
                "an array of {rows} rows of {cols} dots; the child wrote how many dots in all (correct answer {expected})"
            ),
            Kind::Share { total, groups } => format!(
                "{total} dots shared into {groups} equal groups; the child wrote how many in each group (correct answer {expected})"
            ),
            Kind::Trace { word } => format!(
                "handwriting practice for the word \"{word}\": dashed guide letters to trace on the upper lines, then the same word written freehand on the lower lines. \
                 Judge only the child's own thick continuous pen strokes; the thin dashed skeleton letters are the printed guide. \
                 YES if the freehand copy is readable as \"{word}\", ALMOST if shaky but recognizable, NO if missing or unreadable"
            ),
        }
    }
}

/// A tiny deterministic generator — no dependency, stable across builds.
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 >> 8
    }
    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + self.next() % (hi - lo + 1)
    }
}

/// Adaptive difficulty: three right in a row steps up, two wrong steps down.
/// Small on purpose — the interesting reporting problem is deferred; this is
/// just enough tailoring that a bored child gets harder pages and a stuck
/// child gets easier ones without anyone touching a setting.
pub struct Ladder {
    pub level: u8,
    streak: i32,
}

pub const MIN_LEVEL: u8 = 1;
pub const MAX_LEVEL: u8 = 4;

impl Ladder {
    pub fn new(level: u8) -> Self {
        Self { level: level.clamp(MIN_LEVEL, MAX_LEVEL), streak: 0 }
    }

    /// Record a marked answer; the level moves at +3 / -2.
    pub fn record(&mut self, correct: bool) {
        if correct {
            self.streak = (self.streak.max(0)) + 1;
            if self.streak >= 3 && self.level < MAX_LEVEL {
                self.level += 1;
                self.streak = 0;
            }
        } else {
            self.streak = (self.streak.min(0)) - 1;
            if self.streak <= -2 && self.level > MIN_LEVEL {
                self.level -= 1;
                self.streak = 0;
            }
        }
    }

    /// Consecutive correct answers, for the header's streak dots.
    pub fn streak(&self) -> u32 {
        self.streak.max(0) as u32
    }
}

/// Words by level: single letters, then CVC words, then sight words, then
/// longer words. Everything stays short enough to trace at a large size.
const TRACE_L1: &[&str] = &["a", "b", "c", "d", "e", "m", "o", "s", "t"];
const TRACE_L2: &[&str] = &["cat", "dog", "sun", "hat", "pig", "bed", "cup", "fox"];
const TRACE_L3: &[&str] = &["the", "and", "you", "see", "play", "look", "here"];
const TRACE_L4: &[&str] = &["jump", "ship", "rain", "star", "frog", "moon", "tree"];

/// What the menu narrows practice to: the full mix (default), math only, or
/// handwriting only. A topic changes which activities rotate, never how any
/// one activity is generated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Topic {
    #[default]
    Mix,
    Math,
    Writing,
}

/// The activity rotation per level and topic. Rotating (rather than sampling)
/// guarantees variety: a math page is always followed by something different.
fn rotation(level: u8, topic: Topic) -> &'static [Activity] {
    use Activity::*;
    match (topic, level) {
        (Topic::Writing, _) => &[Trace],
        (Topic::Math, 1) => &[Count, Bond],
        (Topic::Math, 2) => &[Bond, MakeTen, Equation],
        (Topic::Math, 3) => &[Bond, Equation, MakeTen],
        (Topic::Math, _) => &[Array, Equation, Share],
        (Topic::Mix, 1) => &[Count, Bond, Trace],
        (Topic::Mix, 2) => &[Bond, MakeTen, Trace, Equation],
        (Topic::Mix, 3) => &[Bond, Equation, Trace, MakeTen],
        (Topic::Mix, _) => &[Array, Equation, Share, Trace],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activity {
    Count,
    Bond,
    MakeTen,
    Equation,
    Array,
    Share,
    Trace,
}

/// Generate the `rot`-th problem at `level`, within `topic`'s rotation.
pub fn generate(level: u8, topic: Topic, rot: usize, rng: &mut Rng) -> Problem {
    let acts = rotation(level.clamp(MIN_LEVEL, MAX_LEVEL), topic);
    match acts[rot % acts.len()] {
        Activity::Count => {
            let shown = rng.range(1, 5);
            Problem { kind: Kind::TenFrame { shown, make_ten: false }, prompt: "HOW MANY DOTS?" }
        }
        Activity::MakeTen => {
            let shown = rng.range(3, 9);
            Problem { kind: Kind::TenFrame { shown, make_ten: true }, prompt: "HOW MANY MORE MAKE 10?" }
        }
        Activity::Bond => {
            let max_whole = match level {
                1 => 5,
                2 => 10,
                _ => 20,
            };
            let whole = rng.range(3, max_whole);
            let p0 = rng.range(1, whole - 1);
            let parts = [p0, whole - p0];
            let blank = match rng.range(0, 2) {
                0 => Blank::Whole,
                1 => Blank::Part(0),
                _ => Blank::Part(1),
            };
            Problem { kind: Kind::Bond { whole, parts, blank }, prompt: "WRITE THE MISSING NUMBER" }
        }
        Activity::Equation => {
            let (a, op, b) = if level >= 4 {
                if rng.range(0, 1) == 0 {
                    let a = rng.range(2, 5);
                    (a, Op::Mul, rng.range(2, 6))
                } else {
                    let b = rng.range(2, 5);
                    let q = rng.range(2, 5);
                    (b * q, Op::Div, b)
                }
            } else {
                let max = if level <= 2 { 10 } else { 20 };
                if rng.range(0, 1) == 0 {
                    let a = rng.range(1, max - 1);
                    (a, Op::Add, rng.range(1, max - a))
                } else {
                    let a = rng.range(2, max);
                    (a, Op::Sub, rng.range(1, a - 1))
                }
            };
            Problem { kind: Kind::Equation { a, op, b }, prompt: "WRITE THE ANSWER" }
        }
        Activity::Array => {
            let rows = rng.range(2, 4);
            let cols = rng.range(2, 6);
            Problem { kind: Kind::Array { rows, cols }, prompt: "HOW MANY DOTS IN ALL?" }
        }
        Activity::Share => {
            let groups = rng.range(2, 4);
            let each = rng.range(2, 5);
            Problem { kind: Kind::Share { total: groups * each, groups }, prompt: "HOW MANY IN EACH GROUP?" }
        }
        Activity::Trace => {
            let words = match level {
                1 => TRACE_L1,
                2 => TRACE_L2,
                3 => TRACE_L3,
                _ => TRACE_L4,
            };
            let word = words[rng.range(0, words.len() as u32 - 1) as usize];
            Problem { kind: Kind::Trace { word }, prompt: "TRACE IT, THEN WRITE IT" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bond_parts_always_sum_to_the_whole() {
        let mut rng = Rng::new(7);
        for level in 1..=4 {
            for rot in 0..40 {
                if let Kind::Bond { whole, parts, .. } = generate(level, Topic::Mix, rot, &mut rng).kind {
                    assert_eq!(parts[0] + parts[1], whole);
                    assert!(parts[0] >= 1 && parts[1] >= 1, "a zero part teaches nothing");
                }
            }
        }
    }

    #[test]
    fn level_one_stays_within_five_and_level_two_within_ten() {
        let mut rng = Rng::new(3);
        for rot in 0..60 {
            if let Kind::Bond { whole, .. } = generate(1, Topic::Mix, rot, &mut rng).kind {
                assert!(whole <= 5, "level 1 whole {whole} > 5");
            }
            if let Kind::Bond { whole, .. } = generate(2, Topic::Mix, rot, &mut rng).kind {
                assert!(whole <= 10, "level 2 whole {whole} > 10");
            }
        }
    }

    #[test]
    fn subtraction_never_goes_negative_and_division_is_exact() {
        let mut rng = Rng::new(11);
        for level in 1..=4 {
            for rot in 0..80 {
                match generate(level, Topic::Mix, rot, &mut rng).kind {
                    Kind::Equation { a, op: Op::Sub, b } => assert!(a > b),
                    Kind::Equation { a, op: Op::Div, b } => assert_eq!(a % b, 0),
                    Kind::Share { total, groups } => assert_eq!(total % groups, 0),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn expected_answers_are_what_a_child_would_write() {
        let p = Problem { kind: Kind::Bond { whole: 7, parts: [3, 4], blank: Blank::Part(1) }, prompt: "" };
        assert_eq!(p.expected(), "4");
        let p = Problem { kind: Kind::TenFrame { shown: 6, make_ten: true }, prompt: "" };
        assert_eq!(p.expected(), "4");
        let p = Problem { kind: Kind::Array { rows: 3, cols: 4 }, prompt: "" };
        assert_eq!(p.expected(), "12");
        let p = Problem { kind: Kind::Trace { word: "cat" }, prompt: "" };
        assert_eq!(p.expected(), "cat");
    }

    #[test]
    fn the_ladder_steps_up_at_three_and_down_at_two() {
        let mut l = Ladder::new(2);
        l.record(true);
        l.record(true);
        assert_eq!(l.level, 2);
        l.record(true);
        assert_eq!(l.level, 3, "three in a row steps up");
        l.record(false);
        assert_eq!(l.level, 3);
        l.record(false);
        assert_eq!(l.level, 2, "two misses step down");
        // The floor and ceiling hold.
        let mut l = Ladder::new(1);
        l.record(false);
        l.record(false);
        l.record(false);
        assert_eq!(l.level, 1);
        let mut l = Ladder::new(4);
        for _ in 0..9 {
            l.record(true);
        }
        assert_eq!(l.level, 4);
    }

    #[test]
    fn every_level_rotates_through_more_than_one_activity() {
        let mut rng = Rng::new(5);
        for level in 1..=4 {
            let kinds: Vec<_> = (0..4).map(|rot| {
                std::mem::discriminant(&generate(level, Topic::Mix, rot, &mut rng).kind)
            }).collect();
            let first = kinds[0];
            assert!(kinds.iter().any(|k| *k != first), "level {level} repeats one activity");
        }
    }

    #[test]
    fn topics_narrow_the_rotation() {
        let mut rng = Rng::new(5);
        for level in 1..=4 {
            for rot in 0..8 {
                let math = generate(level, Topic::Math, rot, &mut rng).kind;
                assert!(!matches!(math, Kind::Trace { .. }), "math topic dealt {math:?}");
                let writing = generate(level, Topic::Writing, rot, &mut rng).kind;
                assert!(matches!(writing, Kind::Trace { .. }), "writing topic dealt {writing:?}");
            }
        }
    }

    #[test]
    fn briefs_carry_the_expected_answer_for_the_tutor() {
        let mut rng = Rng::new(9);
        for level in 1..=4 {
            for rot in 0..8 {
                let p = generate(level, Topic::Mix, rot, &mut rng);
                assert!(
                    p.brief().contains(&p.expected()),
                    "brief must tell the tutor the correct answer: {}",
                    p.brief()
                );
            }
        }
    }
}
