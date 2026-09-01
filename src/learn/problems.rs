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
    /// Part-whole bar model: one bar split into two segments, the whole
    /// bracketed beneath. One of the three numbers is the blank.
    Bar { whole: u32, parts: [u32; 2], blank: Blank },
    /// A number-line walk: start at `from`, take `delta.abs()` unit hops
    /// (right when positive), on a line labeled 0..=`top`.
    NumberLine { from: u32, delta: i32, top: u32 },
    /// A place-value chart: `tens` ten-discs and `ones` one-dots; the child
    /// writes the number they make.
    PlaceValue { tens: u32, ones: u32 },
    /// A hundred-chart window: the 3×3 neighborhood of `center` with the
    /// middle cell blank.
    HundredWindow { center: u32 },
    /// Two numbers with an empty box between: the child writes <, =, or >.
    Compare { left: u32, right: u32 },
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
            Kind::Bar { whole, parts, blank } => match blank {
                Blank::Whole => whole.to_string(),
                Blank::Part(i) => parts[*i].to_string(),
            },
            Kind::NumberLine { from, delta, .. } => (*from as i32 + delta).to_string(),
            Kind::PlaceValue { tens, ones } => (tens * 10 + ones).to_string(),
            Kind::HundredWindow { center } => center.to_string(),
            Kind::Compare { left, right } => match left.cmp(right) {
                std::cmp::Ordering::Less => "<".to_string(),
                std::cmp::Ordering::Equal => "=".to_string(),
                std::cmp::Ordering::Greater => ">".to_string(),
            },
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
                "handwriting practice for the word \"{word}\": the child traced it over guide letters on the upper lines, then wrote it freehand on the lower lines. \
                 Only the child's own pen strokes are shown — the printed guides are not. \
                 YES if the lower freehand copy is readable as \"{word}\", ALMOST if shaky but recognizable, NO if missing or unreadable"
            ),
            Kind::Bar { whole, parts, blank } => match blank {
                Blank::Whole => format!(
                    "a bar model with parts {} and {}; the child wrote the whole bar's total (correct answer {expected})",
                    parts[0], parts[1]
                ),
                Blank::Part(i) => format!(
                    "a bar model with whole {whole} and one part {}; the child wrote the missing part (correct answer {expected})",
                    parts[1 - i]
                ),
            },
            Kind::NumberLine { from, delta, .. } => {
                let (sign, n) = if *delta >= 0 { ('+', *delta) } else { ('-', -delta) };
                format!(
                    "the equation {from} {sign} {n} = ___ shown as hops on a number line; the child wrote the landing number (correct answer {expected})"
                )
            }
            Kind::PlaceValue { tens, ones } => format!(
                "a place-value chart with {tens} tens and {ones} ones; the child wrote the number they make (correct answer {expected})"
            ),
            Kind::HundredWindow { .. } => format!(
                "a hundred-chart window around a blank middle cell; the child wrote the missing middle number (correct answer {expected})"
            ),
            Kind::Compare { left, right } => format!(
                "the numbers {left} and {right} with an empty box between them; the child wrote a comparison symbol, one of < or = or > (correct answer {expected})"
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

/// What the menu narrows practice to: the full mix (default), math only,
/// handwriting only, or one math skill chosen by name. A topic changes which
/// activities rotate, never how any one activity is generated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Topic {
    #[default]
    Mix,
    Math,
    Writing,
    /// One activity, dealt every page: the skills picker's latch.
    Skill(Activity),
}

/// The activity rotation per level and topic. Rotating (rather than sampling)
/// guarantees variety: a math page is always followed by something different.
fn rotation(level: u8, topic: Topic) -> &'static [Activity] {
    use Activity::*;
    match (topic, level) {
        (Topic::Skill(a), _) => a.alone(),
        (Topic::Writing, _) => &[Trace],
        (Topic::Math, 1) => &[Count, Bond, Compare],
        (Topic::Math, 2) => &[Bond, MakeTen, Equation, Compare, NumberLine, Bar],
        (Topic::Math, 3) => &[Bond, Equation, NumberLine, Bar, MakeTen, PlaceValue],
        (Topic::Math, _) => &[Array, Equation, Share, PlaceValue, HundredWindow, Bar],
        (Topic::Mix, 1) => &[Count, Bond, Trace, Compare],
        (Topic::Mix, 2) => &[Bond, MakeTen, Trace, Equation, Compare, NumberLine, Bar],
        (Topic::Mix, 3) => &[Bond, Equation, Trace, NumberLine, Bar, PlaceValue],
        (Topic::Mix, _) => &[Array, Equation, Trace, PlaceValue, HundredWindow, Bar],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Count,
    Bond,
    MakeTen,
    Equation,
    Array,
    Share,
    Trace,
    Bar,
    NumberLine,
    PlaceValue,
    HundredWindow,
    Compare,
}

/// Every math activity, in the skills picker's box order. `Trace` stays out:
/// handwriting is already the WRITING topic.
pub const MATH_SKILLS: &[Activity] = &[
    Activity::Count,
    Activity::Bond,
    Activity::MakeTen,
    Activity::Equation,
    Activity::Array,
    Activity::Share,
    Activity::Bar,
    Activity::NumberLine,
    Activity::PlaceValue,
    Activity::HundredWindow,
    Activity::Compare,
];

impl Activity {
    /// A rotation of just this activity, for the skills latch.
    fn alone(self) -> &'static [Activity] {
        use Activity::*;
        match self {
            Count => &[Count],
            Bond => &[Bond],
            MakeTen => &[MakeTen],
            Equation => &[Equation],
            Array => &[Array],
            Share => &[Share],
            Trace => &[Trace],
            Bar => &[Bar],
            NumberLine => &[NumberLine],
            PlaceValue => &[PlaceValue],
            HundredWindow => &[HundredWindow],
            Compare => &[Compare],
        }
    }

    /// The skills picker's label: named for what the child does, not for the
    /// pedagogy ("TENS & ONES", never "place-value chart").
    pub fn label(self) -> &'static str {
        use Activity::*;
        match self {
            Count => "COUNT DOTS",
            Bond => "NUMBER BONDS",
            MakeTen => "MAKE TEN",
            Equation => "NUMBER SENTENCES",
            Array => "TIMES TABLES",
            Share => "SHARING",
            Trace => "WRITING",
            Bar => "BAR MODELS",
            NumberLine => "NUMBER LINE",
            PlaceValue => "TENS & ONES",
            HundredWindow => "HUNDRED CHART",
            Compare => "BIGGER OR SMALLER",
        }
    }
}

/// A dealt page: one to four problems of a single activity, like a paper
/// worksheet's row of sums. Dot-heavy figures and handwriting keep the page
/// to themselves; the compact kinds deal two to four to a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Set {
    /// Never empty. The LAST item is the graded one: the MVP marks only the
    /// final answer — right moves on, wrong flags the page for a grown-up.
    pub items: Vec<Problem>,
    /// The printed instruction line for the whole page.
    pub prompt: &'static str,
}

impl Set {
    /// The one problem the oracle is asked to mark: the last on the page.
    pub fn graded(&self) -> &Problem {
        self.items.last().expect("a set is never empty")
    }
}

/// How many of `act` share a page.
fn per_page(act: Activity, rng: &mut Rng) -> usize {
    use Activity::*;
    match act {
        Equation | Compare => rng.range(3, 4) as usize,
        PlaceValue => rng.range(2, 3) as usize,
        NumberLine | Bar | HundredWindow => 2,
        _ => 1,
    }
}

/// The page's instruction line, agreeing in number with its questions.
fn page_prompt(act: Activity, n: usize) -> &'static str {
    use Activity::*;
    let many = n > 1;
    match act {
        Equation => {
            if many { "WRITE THE ANSWERS" } else { "WRITE THE ANSWER" }
        }
        Compare => "WRITE <, =, OR >",
        NumberLine => "COUNT THE HOPS",
        Bar | Bond => {
            if many { "WRITE THE MISSING NUMBERS" } else { "WRITE THE MISSING NUMBER" }
        }
        PlaceValue => {
            if many { "WRITE THE NUMBERS" } else { "WRITE THE NUMBER" }
        }
        HundredWindow => {
            if many { "FILL THE MIDDLE BOXES" } else { "FILL THE MIDDLE BOX" }
        }
        Count => "HOW MANY DOTS?",
        MakeTen => "HOW MANY MORE MAKE 10?",
        Array => "HOW MANY DOTS IN ALL?",
        Share => "HOW MANY IN EACH GROUP?",
        Trace => "TRACE IT, THEN WRITE IT",
    }
}

/// Generate the `rot`-th page at `level`, within `topic`'s rotation.
pub fn generate_set(level: u8, topic: Topic, rot: usize, rng: &mut Rng) -> Set {
    let acts = rotation(level.clamp(MIN_LEVEL, MAX_LEVEL), topic);
    let act = acts[rot % acts.len()];
    let n = per_page(act, rng);
    let items = (0..n).map(|_| problem_for(act, level, rng)).collect();
    Set { items, prompt: page_prompt(act, n) }
}

/// Generate the `rot`-th problem at `level`, within `topic`'s rotation.
#[cfg(test)]
pub fn generate(level: u8, topic: Topic, rot: usize, rng: &mut Rng) -> Problem {
    let acts = rotation(level.clamp(MIN_LEVEL, MAX_LEVEL), topic);
    problem_for(acts[rot % acts.len()], level, rng)
}

/// One problem of `act`, sized to `level`.
fn problem_for(act: Activity, level: u8, rng: &mut Rng) -> Problem {
    match act {
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
        Activity::Bar => {
            // Segments must stay wide enough to write in: each part at least
            // a sixth of the whole, so no sliver ever has to hold a box.
            let whole = if level <= 2 { rng.range(5, 10) } else { rng.range(8, 20) };
            let lo = whole.div_ceil(6);
            let p0 = rng.range(lo, whole - lo);
            let parts = [p0, whole - p0];
            let blank = match rng.range(0, 2) {
                0 => Blank::Whole,
                1 => Blank::Part(0),
                _ => Blank::Part(1),
            };
            Problem { kind: Kind::Bar { whole, parts, blank }, prompt: "WRITE THE MISSING NUMBER" }
        }
        Activity::NumberLine => {
            let top = if level <= 2 { 10 } else { 20 };
            let hops = rng.range(2, 4) as i32;
            if rng.range(0, 1) == 0 {
                let from = rng.range(1, top - hops as u32);
                Problem { kind: Kind::NumberLine { from, delta: hops, top }, prompt: "COUNT THE HOPS" }
            } else {
                let from = rng.range(hops as u32 + 1, top);
                Problem { kind: Kind::NumberLine { from, delta: -hops, top }, prompt: "COUNT THE HOPS" }
            }
        }
        Activity::PlaceValue => {
            let (tens, ones) = if level <= 3 {
                (1, rng.range(1, 9))
            } else {
                (rng.range(2, 9), rng.range(0, 9))
            };
            Problem { kind: Kind::PlaceValue { tens, ones }, prompt: "WRITE THE NUMBER" }
        }
        Activity::HundredWindow => {
            // Row and column both 2..=9 so every neighbor stays on the chart
            // and in the same row/column as a real hundred chart.
            let center = (rng.range(2, 9) - 1) * 10 + rng.range(2, 9);
            Problem { kind: Kind::HundredWindow { center }, prompt: "FILL THE MIDDLE BOX" }
        }
        Activity::Compare => {
            let top = match level {
                1 => 5,
                2 => 10,
                _ => 20,
            };
            // Equal pairs only by choice: consecutive LCG draws correlate
            // under small moduli, so an independent `right` came out equal
            // to `left` far too often. An offset-and-wrap never can.
            let left = rng.range(1, top);
            let right = if rng.range(0, 3) == 0 {
                left
            } else {
                let stepped = left + rng.range(1, top - 1);
                if stepped > top { stepped - top } else { stepped }
            };
            Problem { kind: Kind::Compare { left, right }, prompt: "WRITE <, =, OR >" }
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
    fn a_skill_topic_deals_only_its_own_activity_at_every_level() {
        let mut rng = Rng::new(13);
        for &act in MATH_SKILLS {
            for level in 1..=4 {
                for rot in 0..6 {
                    let p = generate(level, Topic::Skill(act), rot, &mut rng);
                    let same = matches!(
                        (&p.kind, act),
                        (Kind::TenFrame { make_ten: false, .. }, Activity::Count)
                            | (Kind::TenFrame { make_ten: true, .. }, Activity::MakeTen)
                            | (Kind::Bond { .. }, Activity::Bond)
                            | (Kind::Equation { .. }, Activity::Equation)
                            | (Kind::Array { .. }, Activity::Array)
                            | (Kind::Share { .. }, Activity::Share)
                            | (Kind::Bar { .. }, Activity::Bar)
                            | (Kind::NumberLine { .. }, Activity::NumberLine)
                            | (Kind::PlaceValue { .. }, Activity::PlaceValue)
                            | (Kind::HundredWindow { .. }, Activity::HundredWindow)
                            | (Kind::Compare { .. }, Activity::Compare)
                    );
                    assert!(same, "skill {act:?} at level {level} dealt {:?}", p.kind);
                }
            }
        }
    }

    #[test]
    fn every_math_skill_has_a_short_readable_label() {
        for &act in MATH_SKILLS {
            let label = act.label();
            assert!(!label.is_empty() && label.len() <= 18, "label {label:?} won't fit its box");
            assert!(!matches!(act, Activity::Trace), "writing is a topic, not a math skill");
        }
    }

    #[test]
    fn bar_models_sum_and_every_segment_can_hold_a_box() {
        let mut rng = Rng::new(21);
        for level in 2..=4 {
            for rot in 0..60 {
                if let Kind::Bar { whole, parts, .. } = generate(level, Topic::Math, rot, &mut rng).kind {
                    assert_eq!(parts[0] + parts[1], whole);
                    let lo = whole.div_ceil(6);
                    assert!(parts[0] >= lo && parts[1] >= lo, "a sliver segment cannot hold a box: {parts:?} of {whole}");
                    assert!(whole <= if level == 2 { 10 } else { 20 });
                }
            }
        }
    }

    #[test]
    fn number_line_walks_stay_on_the_line() {
        let mut rng = Rng::new(23);
        for level in 2..=4 {
            for rot in 0..60 {
                if let Kind::NumberLine { from, delta, top } = generate(level, Topic::Math, rot, &mut rng).kind {
                    assert_eq!(top, if level == 2 { 10 } else { 20 });
                    let land = from as i32 + delta;
                    assert!(land >= 1 && land <= top as i32, "walk falls off the line: {from} {delta:+}");
                    assert!((2..=4).contains(&delta.abs()), "hops must stay countable");
                    assert!(from >= 1 && from <= top);
                }
            }
        }
    }

    #[test]
    fn place_value_charts_read_as_real_two_digit_numbers() {
        let mut rng = Rng::new(29);
        for level in 3..=4 {
            for rot in 0..60 {
                if let Kind::PlaceValue { tens, ones } = generate(level, Topic::Math, rot, &mut rng).kind {
                    assert!((1..=9).contains(&tens) && ones <= 9);
                    if level == 3 {
                        assert_eq!(tens, 1, "level 3 stays in the teens");
                        assert!(ones >= 1, "10 alone teaches nothing about ones");
                    }
                }
            }
        }
    }

    #[test]
    fn hundred_windows_keep_every_neighbor_on_the_chart() {
        let mut rng = Rng::new(31);
        for rot in 0..60 {
            if let Kind::HundredWindow { center } = generate(4, Topic::Math, rot, &mut rng).kind {
                let (row, col) = ((center - 1) / 10 + 1, (center - 1) % 10 + 1);
                assert!((2..=9).contains(&row) && (2..=9).contains(&col),
                    "center {center} pushes a neighbor off the chart or across a row edge");
            }
        }
    }

    #[test]
    fn compare_expects_the_true_symbol_and_deals_equals_sometimes() {
        let p = Problem { kind: Kind::Compare { left: 7, right: 4 }, prompt: "" };
        assert_eq!(p.expected(), ">");
        let p = Problem { kind: Kind::Compare { left: 3, right: 3 }, prompt: "" };
        assert_eq!(p.expected(), "=");
        let p = Problem { kind: Kind::Compare { left: 2, right: 9 }, prompt: "" };
        assert_eq!(p.expected(), "<");
        let mut rng = Rng::new(37);
        let (mut saw_eq, mut saw_ne) = (false, false);
        for rot in 0..80 {
            if let Kind::Compare { left, right } = generate(1, Topic::Math, rot, &mut rng).kind {
                assert!((1..=5).contains(&left) && (1..=5).contains(&right), "level 1 compares within 1..=5");
                if left == right { saw_eq = true } else { saw_ne = true }
            }
        }
        assert!(saw_eq && saw_ne, "equals must appear, but not always");
    }

    #[test]
    fn sets_deal_worksheet_pages_and_keep_big_figures_alone() {
        let mut rng = Rng::new(41);
        for level in 1..=4 {
            for topic in [Topic::Mix, Topic::Math, Topic::Writing] {
                for rot in 0..10 {
                    let set = generate_set(level, topic, rot, &mut rng);
                    assert!((1..=4).contains(&set.items.len()));
                    let d = std::mem::discriminant(&set.items[0].kind);
                    assert!(set.items.iter().all(|p| std::mem::discriminant(&p.kind) == d),
                        "a page is one activity, like a worksheet");
                    let solo_only = matches!(set.items[0].kind,
                        Kind::Bond { .. } | Kind::TenFrame { .. } | Kind::Array { .. }
                        | Kind::Share { .. } | Kind::Trace { .. });
                    if solo_only {
                        assert_eq!(set.items.len(), 1, "{:?} needs the whole page", set.items[0].kind);
                    } else {
                        assert!(set.items.len() >= 2, "{:?} deals a worksheet row", set.items[0].kind);
                    }
                    assert!(!set.prompt.is_empty());
                    assert_eq!(set.graded().expected(), set.items.last().unwrap().expected());
                }
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
