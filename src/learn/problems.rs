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
    /// Chained number bonds with several blanks, filled one at a time in an
    /// order where each blank is the only unknown left in some bond.
    BondChain(Chain),
    /// The make-ten strategy for adding: `a + b` with a bond under `b`
    /// splitting it into `10 - a` and the rest. The child writes the sum.
    BridgeTen { a: u32, b: u32 },
    /// A comparison bar model: a long bar over a short one, the gap
    /// bracketed. The child writes how many more the long bar has.
    CompareBars { big: u32, small: u32 },
    /// Skip counting: four terms of `start, start + step, …` and a box for
    /// the fifth.
    SkipCount { start: u32, step: u32 },
    /// Number writing: trace the dashed numeral, then write it again below.
    TraceNumber { number: u32 },
    /// Two-digit adding or taking away in column form: the numbers stacked
    /// under T and O headings, one answer box beneath both columns.
    Column { a: u32, op: Op, b: u32 },
    /// Multiplication as equal groups: `groups` rings of `each` dots.
    Groups { groups: u32, each: u32 },
    /// Division as the inverse of a fact: `a x ☐ = product`.
    MissingFactor { a: u32, product: u32 },
    /// One big shape: the child writes how many sides, or corners, it has.
    ShapeSides { shape: Shape, corners: bool },
    /// A scatter of mixed shapes on a 4×3 grid of cells, each `(shape,
    /// cell)`; the child writes how many of `target` there are.
    CountShapes { target: Shape, scene: Vec<(Shape, u8)> },
    /// A repeating row of shapes, `shown` long, then an empty box: the child
    /// draws the shape that comes next.
    ShapePattern { cycle: Vec<Shape>, shown: u32 },
    /// An empty frame: the child draws the named shape.
    DrawShape { shape: Shape },
    /// One big shape and three name boxes: the child ticks its name. Marked
    /// on the tablet, never by the tutor.
    NameShape { shape: Shape, options: [Shape; 3] },
    /// Three shapes in three boxes: the child ticks the `target`. Marked on
    /// the tablet.
    FindShape { target: Shape, options: [Shape; 3] },
}

/// The flat shapes Learn draws and asks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Circle,
    Triangle,
    Square,
    Rectangle,
    Pentagon,
    Hexagon,
}

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Shape::Circle => "CIRCLE",
            Shape::Triangle => "TRIANGLE",
            Shape::Square => "SQUARE",
            Shape::Rectangle => "RECTANGLE",
            Shape::Pentagon => "PENTAGON",
            Shape::Hexagon => "HEXAGON",
        }
    }

    /// Sides, and so corners: a circle has none of either.
    pub fn sides(self) -> u32 {
        match self {
            Shape::Circle => 0,
            Shape::Triangle => 3,
            Shape::Square | Shape::Rectangle => 4,
            Shape::Pentagon => 5,
            Shape::Hexagon => 6,
        }
    }

    fn count_prompt(self) -> &'static str {
        match self {
            Shape::Circle => "HOW MANY CIRCLES?",
            Shape::Triangle => "HOW MANY TRIANGLES?",
            Shape::Square => "HOW MANY SQUARES?",
            Shape::Rectangle => "HOW MANY RECTANGLES?",
            Shape::Pentagon => "HOW MANY PENTAGONS?",
            Shape::Hexagon => "HOW MANY HEXAGONS?",
        }
    }

    fn find_prompt(self) -> &'static str {
        match self {
            Shape::Circle => "MARK THE CIRCLE",
            Shape::Triangle => "MARK THE TRIANGLE",
            Shape::Square => "MARK THE SQUARE",
            Shape::Rectangle => "MARK THE RECTANGLE",
            Shape::Pentagon => "MARK THE PENTAGON",
            Shape::Hexagon => "MARK THE HEXAGON",
        }
    }

    fn draw_prompt(self) -> &'static str {
        match self {
            Shape::Circle => "DRAW A CIRCLE",
            Shape::Triangle => "DRAW A TRIANGLE",
            Shape::Square => "DRAW A SQUARE",
            Shape::Rectangle => "DRAW A RECTANGLE",
            Shape::Pentagon => "DRAW A PENTAGON",
            Shape::Hexagon => "DRAW A HEXAGON",
        }
    }
}

/// Shapes by level: the four every young child meets, then the pentagon
/// and hexagon from level 3.
fn shapes_for(level: u8) -> &'static [Shape] {
    use Shape::*;
    if level <= 2 {
        &[Circle, Triangle, Square, Rectangle]
    } else {
        &[Circle, Triangle, Square, Rectangle, Pentagon, Hexagon]
    }
}

/// A shape and three distinct options holding it, shuffled. Square and
/// rectangle never share a question before level 3: telling them apart is
/// its own lesson.
fn shape_options(level: u8, rng: &mut Rng) -> (Shape, [Shape; 3]) {
    let pool = shapes_for(level);
    let pick = |rng: &mut Rng| pool[rng.range(0, pool.len() as u32 - 1) as usize];
    let target = pick(rng);
    let clash = |a: Shape, b: Shape| {
        a == b || (level <= 2 && matches!((a, b), (Shape::Square, Shape::Rectangle) | (Shape::Rectangle, Shape::Square)))
    };
    let mut options = vec![target];
    for step in 0..pool.len() * 4 {
        if options.len() == 3 {
            break;
        }
        // Random first, then walk the pool so a short one always fills.
        let s = if step < pool.len() * 2 { pick(rng) } else { pool[step % pool.len()] };
        if !options.iter().any(|&o| clash(o, s)) {
            options.push(s);
        }
    }
    let mut options = [options[0], options[1], options[2]];
    for i in (1..3).rev() {
        options.swap(i, rng.range(0, i as u32) as usize);
    }
    (target, options)
}

/// Cells in a shape scene: four across, three down.
pub const SCENE_CELLS: u8 = 12;

/// Number bonds linked into a chain: a part of one bond is the whole of the
/// next. `order` lists the blank circles in the order they can be solved, so
/// each is the only unknown in some bond once the ones before it are filled;
/// `step` is the blank the child is on. Earlier blanks print as solved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    pub values: Vec<u32>,
    /// Each bond as (whole, [part, part]), indices into `values`; a whole
    /// always comes before its parts.
    pub bonds: Vec<(usize, [usize; 2])>,
    /// Each circle's place: its row from the top, and x in thousandths of the
    /// page width.
    pub at: Vec<(u8, u16)>,
    pub order: Vec<usize>,
    pub step: usize,
}

impl Chain {
    /// The circle being filled now.
    pub fn blank(&self) -> usize {
        self.order[self.step]
    }

    pub fn is_solved(&self, i: usize) -> bool {
        self.order[..self.step].contains(&i)
    }

    /// Move to the next blank; false when the one just filled was the last.
    pub fn advance(&mut self) -> bool {
        if self.step + 1 < self.order.len() {
            self.step += 1;
            true
        } else {
            false
        }
    }

    /// The order `blanks` can be solved in, bond by bond, with `true` when
    /// some step leans on an earlier answer. None when some blank is never
    /// the only unknown in a bond.
    fn solve_order(bonds: &[(usize, [usize; 2])], blanks: &[usize]) -> Option<(Vec<usize>, bool)> {
        let mut unknown: Vec<usize> = blanks.to_vec();
        let mut order: Vec<usize> = Vec::new();
        let mut linked = false;
        while !unknown.is_empty() {
            let (next, bond) = bonds.iter().find_map(|&(w, [a, b])| {
                let open: Vec<usize> = [w, a, b].into_iter().filter(|i| unknown.contains(i)).collect();
                (open.len() == 1).then(|| (open[0], [w, a, b]))
            })?;
            linked |= bond.iter().any(|i| order.contains(i));
            unknown.retain(|&i| i != next);
            order.push(next);
        }
        Some((order, linked))
    }
}

type ChainShape = (&'static [(u8, u16)], &'static [(usize, [usize; 2])]);

/// Chain shapes: circle places and bonds, drawn for a right-hand split and
/// mirrored for a left-hand one. Splits go inward so circles stay on the page.
const CHAIN_TWO: ChainShape = (
    &[(0, 500), (1, 280), (1, 720), (2, 600), (2, 840)],
    &[(0, [1, 2]), (2, [3, 4])],
);
const CHAIN_TREE: ChainShape = (
    &[(0, 500), (1, 280), (1, 720), (2, 160), (2, 400), (2, 600), (2, 840)],
    &[(0, [1, 2]), (1, [3, 4]), (2, [5, 6])],
);
const CHAIN_LADDER: ChainShape = (
    &[(0, 500), (1, 280), (1, 720), (2, 600), (2, 840), (3, 480), (3, 720)],
    &[(0, [1, 2]), (2, [3, 4]), (3, [5, 6])],
);
/// Four bonds for four blanks: n bonds are n equations, so a chain never
/// has more solvable blanks than bonds.
const CHAIN_TREE_PLUS: ChainShape = (
    &[(0, 500), (1, 280), (1, 720), (2, 160), (2, 400), (2, 600), (2, 840), (3, 300), (3, 500)],
    &[(0, [1, 2]), (1, [3, 4]), (2, [5, 6]), (4, [7, 8])],
);
const CHAIN_LADDER_PLUS: ChainShape = (
    &[(0, 500), (1, 280), (1, 720), (2, 600), (2, 840), (3, 480), (3, 720), (2, 160), (2, 400)],
    &[(0, [1, 2]), (1, [7, 8]), (2, [3, 4]), (3, [5, 6])],
);

/// A chain sized by level: two bonds and two blanks to 10, then three bonds
/// and three blanks to 20, then four of each to 50.
fn chain(level: u8, rng: &mut Rng) -> Chain {
    let either = |rng: &mut Rng, a: ChainShape, b: ChainShape| if rng.range(0, 1) == 0 { a } else { b };
    let (shape, blanks, max_whole) = match level {
        0..=2 => (CHAIN_TWO, 2, 10),
        3 => (either(rng, CHAIN_TREE, CHAIN_LADDER), 3, 20),
        _ => (either(rng, CHAIN_TREE_PLUS, CHAIN_LADDER_PLUS), 4, 50),
    };
    let (places, bonds) = shape;
    let mirror = rng.range(0, 1) == 1;
    let at: Vec<(u8, u16)> = places.iter().map(|&(r, x)| (r, if mirror { 1000 - x } else { x })).collect();
    let n = places.len();

    // The least each circle can hold: 1 for a leaf, the sum of its parts'
    // least for a circle that splits, so every circle gets a number.
    let mut least = vec![1u32; n];
    for &(w, [a, b]) in bonds.iter().rev() {
        least[w] = least[a] + least[b];
    }
    let mut values = vec![0u32; n];
    values[0] = rng.range(least[0].max(4), max_whole);
    for &(w, [a, b]) in bonds {
        values[a] = rng.range(least[a], values[w] - least[b]);
        values[b] = values[w] - values[a];
    }

    // Blanks: random picks until one set solves in order and at least one
    // step needs an earlier answer, which is what makes it a chain.
    for _ in 0..500 {
        let mut set: Vec<usize> = Vec::new();
        while set.len() < blanks {
            let i = rng.range(0, n as u32 - 1) as usize;
            if !set.contains(&i) {
                set.push(i);
            }
        }
        if let Some((order, true)) = Chain::solve_order(bonds, &set) {
            return Chain { values, bonds: bonds.to_vec(), at, order, step: 0 };
        }
    }
    // One part of each bond, solved bottom-up, always chains; the random
    // search above finds a set long before this.
    let set: Vec<usize> = bonds.iter().take(blanks).map(|&(_, [a, _])| a).collect();
    let (order, _) = Chain::solve_order(bonds, &set).expect("one part per bond solves");
    Chain { values, bonds: bonds.to_vec(), at, order, step: 0 }
}

/// Terms shown before the box on a skip-counting row.
pub const SKIP_SHOWN: u32 = 4;

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
            Kind::BondChain(c) => c.values[c.blank()].to_string(),
            Kind::BridgeTen { a, b } => (a + b).to_string(),
            Kind::CompareBars { big, small } => (big - small).to_string(),
            Kind::SkipCount { start, step } => (start + SKIP_SHOWN * step).to_string(),
            Kind::TraceNumber { number } => number.to_string(),
            Kind::Column { a, op, b } => match op {
                Op::Sub => (a - b).to_string(),
                _ => (a + b).to_string(),
            },
            Kind::Groups { groups, each } => (groups * each).to_string(),
            Kind::MissingFactor { a, product } => (product / a).to_string(),
            Kind::ShapeSides { shape, .. } => shape.sides().to_string(),
            Kind::CountShapes { target, scene } => scene.iter().filter(|(s, _)| s == target).count().to_string(),
            Kind::ShapePattern { cycle, shown } => cycle[*shown as usize % cycle.len()].name().to_string(),
            Kind::DrawShape { shape } => shape.name().to_string(),
            Kind::NameShape { shape, .. } => shape.name().to_string(),
            Kind::FindShape { target, .. } => target.name().to_string(),
        }
    }

    /// For a problem answered by ticking one of its boxes: the right box.
    /// None for everything the child writes or draws.
    pub fn choice_answer(&self) -> Option<usize> {
        match &self.kind {
            Kind::NameShape { shape, options } => options.iter().position(|o| o == shape),
            Kind::FindShape { target, options } => options.iter().position(|o| o == target),
            _ => None,
        }
    }

    /// The grown-up's line after a wrong tick: how to look again, never the
    /// answer.
    pub fn choice_hint(&self) -> &'static str {
        match &self.kind {
            Kind::NameShape { .. } => "Count its sides together, then look again.",
            Kind::FindShape { .. } => "Count the sides on each one.",
            _ => "Look at each box again.",
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
            Kind::BondChain(c) => {
                let links: Vec<String> = c.bonds.iter()
                    .map(|&(w, [a, b])| format!("{} splits into {} and {}", c.values[w], c.values[a], c.values[b]))
                    .collect();
                format!(
                    "chained number bonds ({}), filled one empty circle at a time; the child wrote the number for blank {} of {} (correct answer {expected})",
                    links.join(", "), c.step + 1, c.order.len()
                )
            }
            Kind::BridgeTen { a, b } => format!(
                "the sum {a} + {b} = ___ with {b} split into {} and {} to make ten; the child wrote the sum (correct answer {expected})",
                10 - a, b - (10 - a)
            ),
            Kind::CompareBars { big, small } => format!(
                "a comparison bar model: a bar of {big} over a bar of {small}; the child wrote how many more the longer bar has (correct answer {expected})"
            ),
            Kind::SkipCount { start, step } => format!(
                "skip counting by {step}s from {start}, four numbers shown; the child wrote the next number (correct answer {expected})"
            ),
            Kind::TraceNumber { number } => format!(
                "handwriting practice for the number {number}: the child traced it over guide digits on the upper lines, then wrote it freehand on the lower lines. \
                 Only the child's own pen strokes are shown — the printed guides are not. \
                 YES if the lower freehand copy reads as {number}, ALMOST if a digit is written backwards or shaky but recognizable, NO if missing or unreadable"
            ),
            Kind::Column { a, op, b } => format!(
                "the sum {a} {} {b} written in tens and ones columns, one number above the other; the child wrote the answer under the line (correct answer {expected})",
                op.glyph()
            ),
            Kind::Groups { groups, each } => format!(
                "{groups} equal groups of {each} dots with the equation {groups} x {each} = ___; the child wrote how many dots in all (correct answer {expected})"
            ),
            Kind::MissingFactor { a, product } => format!(
                "the equation {a} x ___ = {product}; the child wrote the missing number (correct answer {expected})"
            ),
            Kind::ShapeSides { shape, corners } => format!(
                "a large {}; the child wrote how many {} it has (correct answer {expected})",
                shape.name().to_lowercase(),
                if *corners { "corners" } else { "sides" }
            ),
            Kind::CountShapes { target, scene } => {
                let mut tally: Vec<String> = Vec::new();
                for s in [Shape::Circle, Shape::Triangle, Shape::Square, Shape::Rectangle, Shape::Pentagon, Shape::Hexagon] {
                    let n = scene.iter().filter(|(k, _)| *k == s).count();
                    if n > 0 {
                        tally.push(format!("{n} {}", s.name().to_lowercase()));
                    }
                }
                format!(
                    "a picture of scattered shapes ({}); the child wrote how many {}s there are (correct answer {expected})",
                    tally.join(", "),
                    target.name().to_lowercase()
                )
            }
            Kind::ShapePattern { cycle, shown } => {
                let row: Vec<&str> = (0..*shown as usize).map(|i| cycle[i % cycle.len()].name()).collect();
                format!(
                    "a repeating pattern of shapes ({}), then an empty box; the child drew the shape that comes next. \
                     The page shows only the child's drawing. YES if it is recognizably a {lower} (wobbly lines and rounded corners are fine \
                     for a young child), ALMOST if nearly (a {lower} left open, or one side too many), NO if a different shape or empty \
                     (correct answer {expected})",
                    row.join(", ").to_lowercase(),
                    lower = expected.to_lowercase()
                )
            }
            Kind::NameShape { .. } | Kind::FindShape { .. } => format!(
                "a tick-box question marked on the tablet (correct answer {expected})"
            ),
            Kind::DrawShape { shape } => format!(
                "the child was asked to draw a {lower} in an empty frame. The page shows only the child's drawing. \
                 YES if it is recognizably a {lower} (wobbly lines and rounded corners are fine for a young child), \
                 ALMOST if nearly (a {lower} left open, or one side too many), NO if a different shape or empty \
                 (correct answer {expected})",
                lower = shape.name().to_lowercase()
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
pub const MAX_LEVEL: u8 = 5;

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
    /// Number writing only: the WRITE NUMBERS entry.
    Numbers,
    /// Shapes only: the SHAPES entry.
    Shapes,
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
        (Topic::Numbers, _) => &[WriteNumbers],
        (Topic::Shapes, 1) => &[FindShape, DrawShape, CountShapes, NameShape, ShapePattern],
        (Topic::Shapes, 2) => &[NameShape, DrawShape, CountShapes, FindShape, ShapeSides, ShapePattern],
        (Topic::Shapes, _) => &[ShapeSides, NameShape, CountShapes, ShapePattern, FindShape, DrawShape],
        (Topic::Math, 1) => &[Count, Bond, Compare],
        (Topic::Math, 2) => &[Bond, MakeTen, Equation, BondChain, Compare, NumberLine, Bar, BridgeTen],
        (Topic::Math, 3) => &[Bond, ColumnAdd, Equation, BondChain, NumberLine, Bar, BridgeTen, ColumnSub,
            MakeTen, PlaceValue, CompareBars, SkipCount],
        (Topic::Math, 4) => &[Array, Groups, TimesFacts, ColumnAdd, Equation, BondChain, Share, ColumnSub,
            PlaceValue, HundredWindow, DivideFacts, Bar, CompareBars, SkipCount],
        (Topic::Math, _) => &[ColumnAdd, TimesFacts, MissingFactor, ColumnSub, Groups, DivideFacts, BondChain,
            Bar, CompareBars, HundredWindow, SkipCount],
        (Topic::Mix, 1) => &[Count, Bond, WriteNumbers, FindShape, Trace, Compare, DrawShape, CountShapes],
        (Topic::Mix, 2) => &[Bond, MakeTen, WriteNumbers, Equation, CountShapes, Trace, BondChain, NameShape,
            Compare, NumberLine, ShapePattern, Bar, BridgeTen],
        (Topic::Mix, 3) => &[Bond, ColumnAdd, Equation, ShapeSides, Trace, BondChain, WriteNumbers, NumberLine,
            Bar, BridgeTen, ColumnSub, ShapePattern, PlaceValue, CompareBars, SkipCount],
        (Topic::Mix, 4) => &[Array, ColumnAdd, Trace, Groups, TimesFacts, BondChain, ShapeSides, ColumnSub,
            PlaceValue, Share, HundredWindow, DivideFacts, CountShapes, Bar, CompareBars, SkipCount],
        (Topic::Mix, _) => &[ColumnAdd, TimesFacts, Trace, ColumnSub, MissingFactor, ShapeSides, Groups,
            DivideFacts, BondChain, Bar, CountShapes, CompareBars, HundredWindow, SkipCount],
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
    BondChain,
    BridgeTen,
    CompareBars,
    SkipCount,
    WriteNumbers,
    ColumnAdd,
    ColumnSub,
    Groups,
    TimesFacts,
    MissingFactor,
    DivideFacts,
    ShapeSides,
    CountShapes,
    ShapePattern,
    DrawShape,
    NameShape,
    FindShape,
}

/// The skills picker's sections, each a strand of the curriculum, in box
/// order. `Trace` stays out: handwriting is already the WRITING topic.
pub const SKILL_GROUPS: &[(&str, &[Activity])] = &[
    (
        "COUNTING & NUMBERS",
        &[Activity::Count, Activity::Compare, Activity::PlaceValue, Activity::HundredWindow, Activity::SkipCount],
    ),
    (
        "ADDING & TAKING AWAY",
        &[
            Activity::Bond,
            Activity::BondChain,
            Activity::MakeTen,
            Activity::BridgeTen,
            Activity::Equation,
            Activity::NumberLine,
            Activity::ColumnAdd,
            Activity::ColumnSub,
        ],
    ),
    ("BAR MODELS", &[Activity::Bar, Activity::CompareBars]),
    (
        "TIMES & SHARING",
        &[
            Activity::Array,
            Activity::Groups,
            Activity::TimesFacts,
            Activity::Share,
            Activity::MissingFactor,
            Activity::DivideFacts,
        ],
    ),
    (
        "SHAPES",
        &[
            Activity::NameShape,
            Activity::FindShape,
            Activity::ShapeSides,
            Activity::CountShapes,
            Activity::ShapePattern,
            Activity::DrawShape,
        ],
    ),
];

/// Every math activity, in the skills picker's box order.
pub fn math_skills() -> Vec<Activity> {
    SKILL_GROUPS.iter().flat_map(|(_, acts)| acts.iter().copied()).collect()
}

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
            BondChain => &[BondChain],
            BridgeTen => &[BridgeTen],
            CompareBars => &[CompareBars],
            SkipCount => &[SkipCount],
            WriteNumbers => &[WriteNumbers],
            ColumnAdd => &[ColumnAdd],
            ColumnSub => &[ColumnSub],
            Groups => &[Groups],
            TimesFacts => &[TimesFacts],
            MissingFactor => &[MissingFactor],
            DivideFacts => &[DivideFacts],
            ShapeSides => &[ShapeSides],
            CountShapes => &[CountShapes],
            ShapePattern => &[ShapePattern],
            DrawShape => &[DrawShape],
            NameShape => &[NameShape],
            FindShape => &[FindShape],
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
            BondChain => "BOND CHAINS",
            BridgeTen => "ADD BY MAKING TEN",
            CompareBars => "HOW MANY MORE",
            SkipCount => "SKIP COUNTING",
            WriteNumbers => "WRITE NUMBERS",
            ColumnAdd => "BIG SUMS",
            ColumnSub => "BIG TAKE-AWAYS",
            Groups => "EQUAL GROUPS",
            TimesFacts => "TIMES FACTS",
            MissingFactor => "MISSING NUMBER",
            DivideFacts => "DIVIDE",
            ShapeSides => "SIDES & CORNERS",
            CountShapes => "COUNT SHAPES",
            ShapePattern => "SHAPE PATTERNS",
            DrawShape => "DRAW SHAPES",
            NameShape => "NAME THE SHAPE",
            FindShape => "FIND THE SHAPE",
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
        Equation | Compare | TimesFacts | DivideFacts | MissingFactor => rng.range(3, 4) as usize,
        PlaceValue | SkipCount => rng.range(2, 3) as usize,
        NumberLine | Bar | HundredWindow | CompareBars | ColumnAdd | ColumnSub => 2,
        _ => 1,
    }
}

/// The page's instruction line, agreeing in number with its questions.
fn page_prompt(act: Activity, n: usize) -> &'static str {
    use Activity::*;
    let many = n > 1;
    match act {
        Equation | TimesFacts | DivideFacts => {
            if many { "WRITE THE ANSWERS" } else { "WRITE THE ANSWER" }
        }
        MissingFactor => {
            if many { "WRITE THE MISSING NUMBERS" } else { "WRITE THE MISSING NUMBER" }
        }
        ColumnAdd => "ADD THE ONES FIRST",
        ColumnSub => "TAKE AWAY THE ONES FIRST",
        Groups => "HOW MANY IN ALL?",
        WriteNumbers => "TRACE IT, THEN WRITE IT",
        // One to a page: each problem names its own shape or question.
        ShapeSides => "HOW MANY SIDES?",
        CountShapes => "HOW MANY?",
        ShapePattern => "DRAW WHAT COMES NEXT",
        DrawShape => "DRAW THE SHAPE",
        NameShape => "WHAT SHAPE IS THIS?",
        FindShape => "MARK THE SHAPE",
        Compare => "WRITE <, =, OR >",
        NumberLine => "COUNT THE HOPS",
        BondChain => "FILL THE CIRCLES, ONE AT A TIME",
        Bar | Bond => {
            if many { "WRITE THE MISSING NUMBERS" } else { "WRITE THE MISSING NUMBER" }
        }
        BridgeTen => "MAKE TEN, THEN ADD",
        CompareBars => "HOW MANY MORE?",
        SkipCount => "WHAT COMES NEXT?",
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
    let items: Vec<Problem> = (0..n).map(|_| problem_for(act, level, rng)).collect();
    // A page of one says its own problem's line ("HOW MANY TRIANGLES?"); a
    // worksheet row says the activity's.
    let prompt = if n == 1 { items[0].prompt } else { page_prompt(act, n) };
    Set { items, prompt }
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
        Activity::BondChain => Problem {
            kind: Kind::BondChain(chain(level, rng)),
            prompt: "FILL THE CIRCLES, ONE AT A TIME",
        },
        Activity::BridgeTen => {
            // `b` must be bigger than what `a` needs to reach ten, so the
            // sum crosses ten and the bond has a nonzero rest.
            let a = if level <= 2 { rng.range(8, 9) } else { rng.range(6, 9) };
            let b = rng.range(11 - a, 9);
            Problem { kind: Kind::BridgeTen { a, b }, prompt: "MAKE TEN, THEN ADD" }
        }
        Activity::CompareBars => {
            let top = if level <= 2 { 10 } else { 20 };
            let big = rng.range(3, top);
            let small = rng.range(1, big - 1);
            Problem { kind: Kind::CompareBars { big, small }, prompt: "HOW MANY MORE?" }
        }
        Activity::SkipCount => {
            let step = if level <= 3 {
                [2, 5, 10][rng.range(0, 2) as usize]
            } else {
                [2, 3, 4, 5, 10][rng.range(0, 4) as usize]
            };
            // Start on a multiple of the step, low enough that the answer
            // stays under 100. A step above 19 would underflow this bound.
            let k = rng.range(0, (99 / step - SKIP_SHOWN).min(6));
            Problem { kind: Kind::SkipCount { start: k * step, step }, prompt: "WHAT COMES NEXT?" }
        }
        Activity::WriteNumbers => {
            // Digits first, then the teens and twenty, then any two digits.
            let number = match level {
                1 => rng.range(0, 9),
                2 => rng.range(10, 20),
                _ => rng.range(21, 99),
            };
            Problem { kind: Kind::TraceNumber { number }, prompt: "TRACE IT, THEN WRITE IT" }
        }
        Activity::ColumnAdd => {
            let (a, b) = column_add(level, rng);
            Problem { kind: Kind::Column { a, op: Op::Add, b }, prompt: "ADD THE ONES FIRST" }
        }
        Activity::ColumnSub => {
            let (a, b) = column_sub(level, rng);
            Problem { kind: Kind::Column { a, op: Op::Sub, b }, prompt: "TAKE AWAY THE ONES FIRST" }
        }
        Activity::Groups => {
            let each = times_table(level, rng);
            let groups = rng.range(2, 5);
            Problem { kind: Kind::Groups { groups, each }, prompt: "HOW MANY IN ALL?" }
        }
        Activity::TimesFacts => {
            // Either order: 3 x 5 and 5 x 3 are the same fact, and the
            // child should meet both.
            let t = times_table(level, rng);
            let n = rng.range(1, 10);
            let (a, b) = if rng.range(0, 1) == 0 { (t, n) } else { (n, t) };
            Problem { kind: Kind::Equation { a, op: Op::Mul, b }, prompt: "WRITE THE ANSWER" }
        }
        Activity::DivideFacts => {
            let t = times_table(level, rng);
            let n = rng.range(2, 10);
            Problem { kind: Kind::Equation { a: t * n, op: Op::Div, b: t }, prompt: "WRITE THE ANSWER" }
        }
        Activity::MissingFactor => {
            let t = times_table(level, rng);
            let n = rng.range(2, 10);
            Problem { kind: Kind::MissingFactor { a: t, product: t * n }, prompt: "WRITE THE MISSING NUMBER" }
        }
        Activity::ShapeSides => {
            // A circle (none at all) is the best trick question, but only
            // once the child has met sides on shapes that have them.
            let pool = shapes_for(level);
            let pool = if level <= 2 { &pool[1..] } else { pool };
            let shape = pool[rng.range(0, pool.len() as u32 - 1) as usize];
            let corners = rng.range(0, 1) == 1;
            let prompt = if corners { "HOW MANY CORNERS?" } else { "HOW MANY SIDES?" };
            Problem { kind: Kind::ShapeSides { shape, corners }, prompt }
        }
        Activity::CountShapes => {
            let pool = shapes_for(level);
            let target = pool[rng.range(0, pool.len() as u32 - 1) as usize];
            let want = if level <= 2 { rng.range(2, 5) } else { rng.range(3, 7) } as usize;
            let others = rng.range(3, 5) as usize;
            // Shuffle the cells, then deal: targets first, distractors after.
            let mut cells: Vec<u8> = (0..SCENE_CELLS).collect();
            for i in (1..cells.len()).rev() {
                cells.swap(i, rng.range(0, i as u32) as usize);
            }
            let mut scene: Vec<(Shape, u8)> = cells[..want].iter().map(|&c| (target, c)).collect();
            for &c in &cells[want..want + others] {
                let mut s = pool[rng.range(0, pool.len() as u32 - 1) as usize];
                if s == target {
                    s = pool[(pool.iter().position(|&p| p == target).unwrap_or(0) + 1) % pool.len()];
                }
                // A square among rectangles (or the reverse) is a fair
                // distractor only once the child can tell them apart.
                if level <= 2 && matches!((s, target), (Shape::Square, Shape::Rectangle) | (Shape::Rectangle, Shape::Square)) {
                    s = Shape::Circle;
                }
                scene.push((s, c));
            }
            scene.sort_by_key(|&(_, c)| c);
            Problem { kind: Kind::CountShapes { target, scene }, prompt: target.count_prompt() }
        }
        Activity::ShapePattern => {
            // Patterns use the shapes a child can draw: circle, triangle,
            // square. Never square beside rectangle: a wobbly drawing of one
            // reads as the other.
            use Shape::*;
            let patterns: &[&[Shape]] = if level <= 1 {
                &[&[Circle, Square], &[Triangle, Circle], &[Square, Triangle]]
            } else {
                &[
                    &[Circle, Square],
                    &[Triangle, Circle],
                    &[Circle, Circle, Triangle],
                    &[Square, Triangle, Triangle],
                    &[Circle, Triangle, Square],
                    &[Triangle, Square, Circle],
                ]
            };
            let cycle = patterns[rng.range(0, patterns.len() as u32 - 1) as usize].to_vec();
            // At least one whole repeat and one more shape, at most seven.
            let shown = rng.range(cycle.len() as u32 + 1, 7);
            Problem { kind: Kind::ShapePattern { cycle, shown }, prompt: "DRAW WHAT COMES NEXT" }
        }
        Activity::NameShape => {
            let (shape, options) = shape_options(level, rng);
            Problem { kind: Kind::NameShape { shape, options }, prompt: "WHAT SHAPE IS THIS?" }
        }
        Activity::FindShape => {
            let (target, options) = shape_options(level, rng);
            Problem { kind: Kind::FindShape { target, options }, prompt: target.find_prompt() }
        }
        Activity::DrawShape => {
            use Shape::*;
            let pool: &[Shape] = if level <= 2 { &[Circle, Triangle, Square, Rectangle] } else { &[Triangle, Square, Rectangle, Pentagon, Hexagon] };
            let shape = pool[rng.range(0, pool.len() as u32 - 1) as usize];
            Problem { kind: Kind::DrawShape { shape }, prompt: shape.draw_prompt() }
        }
    }
}

/// The times table a fact comes from, in Singapore's Primary 2 order: 2, 5,
/// and 10 first, then 3 and 4 join them at level 5.
fn times_table(level: u8, rng: &mut Rng) -> u32 {
    if level <= 4 {
        [2, 5, 10][rng.range(0, 2) as usize]
    } else {
        [2, 3, 4, 5, 10][rng.range(0, 4) as usize]
    }
}

/// Draws a pair for `keep` up to a few hundred times, then takes the
/// fallback. Every condition here is met by a large share of draws, so the
/// fallback only guards against a pathological seed.
fn draw_pair(rng: &mut Rng, mut draw: impl FnMut(&mut Rng) -> (u32, u32), keep: impl Fn(u32, u32) -> bool,
    fallback: (u32, u32)) -> (u32, u32) {
    for _ in 0..400 {
        let (a, b) = draw(rng);
        if keep(a, b) {
            return (a, b);
        }
    }
    fallback
}

/// A column sum at `level`: two digits plus one at level 3, two plus two at
/// level 4, both with no new ten made; at level 5 the ones always make a new
/// ten (regrouping). The sum stays under 100.
fn column_add(level: u8, rng: &mut Rng) -> (u32, u32) {
    let regroup = level >= 5;
    let keep = move |a: u32, b: u32| a + b <= 99 && (a % 10 + b % 10 >= 10) == regroup;
    if level <= 3 {
        draw_pair(rng, |r| (r.range(11, 89), r.range(1, 9)), keep, (23, 4))
    } else {
        draw_pair(rng, |r| (r.range(11, 79), r.range(11, 79)), keep, if regroup { (38, 47) } else { (23, 14) })
    }
}

/// A column take-away at `level`: one digit from two at level 3, two from
/// two at level 4, never borrowing; at level 5 the ones always borrow a ten.
/// The answer is never zero or less.
fn column_sub(level: u8, rng: &mut Rng) -> (u32, u32) {
    let regroup = level >= 5;
    let keep = move |a: u32, b: u32| b < a && (b % 10 > a % 10) == regroup;
    if level <= 3 {
        draw_pair(rng, |r| (r.range(11, 99), r.range(1, 9)), keep, (47, 5))
    } else {
        draw_pair(rng, |r| (r.range(21, 99), r.range(11, 89)), keep, if regroup { (52, 27) } else { (68, 25) })
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
        let mut l = Ladder::new(MAX_LEVEL);
        for _ in 0..9 {
            l.record(true);
        }
        assert_eq!(l.level, MAX_LEVEL);
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
        for act in math_skills() {
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
                            | (Kind::BondChain(_), Activity::BondChain)
                            | (Kind::BridgeTen { .. }, Activity::BridgeTen)
                            | (Kind::CompareBars { .. }, Activity::CompareBars)
                            | (Kind::SkipCount { .. }, Activity::SkipCount)
                            | (Kind::Column { op: Op::Add, .. }, Activity::ColumnAdd)
                            | (Kind::Column { op: Op::Sub, .. }, Activity::ColumnSub)
                            | (Kind::Groups { .. }, Activity::Groups)
                            | (Kind::Equation { op: Op::Mul, .. }, Activity::TimesFacts)
                            | (Kind::Equation { op: Op::Div, .. }, Activity::DivideFacts)
                            | (Kind::MissingFactor { .. }, Activity::MissingFactor)
                            | (Kind::ShapeSides { .. }, Activity::ShapeSides)
                            | (Kind::CountShapes { .. }, Activity::CountShapes)
                            | (Kind::ShapePattern { .. }, Activity::ShapePattern)
                            | (Kind::DrawShape { .. }, Activity::DrawShape)
                            | (Kind::NameShape { .. }, Activity::NameShape)
                            | (Kind::FindShape { .. }, Activity::FindShape)
                    );
                    assert!(same, "skill {act:?} at level {level} dealt {:?}", p.kind);
                }
            }
        }
    }

    #[test]
    fn every_math_skill_has_a_short_readable_label() {
        for act in math_skills() {
            let label = act.label();
            assert!(!label.is_empty() && label.len() <= 18, "label {label:?} won't fit its box");
            assert!(!matches!(act, Activity::Trace), "writing is a topic, not a math skill");
        }
        let skills = math_skills();
        for (i, a) in skills.iter().enumerate() {
            assert!(!skills[..i].contains(a), "{a:?} sits in two skill groups");
        }
        assert_eq!(skills.len(), 27, "every math activity has a box");
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
                        | Kind::Share { .. } | Kind::Trace { .. } | Kind::BondChain(_)
                        | Kind::BridgeTen { .. } | Kind::TraceNumber { .. } | Kind::Groups { .. }
                        | Kind::ShapeSides { .. } | Kind::CountShapes { .. } | Kind::ShapePattern { .. }
                        | Kind::DrawShape { .. } | Kind::NameShape { .. } | Kind::FindShape { .. });
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
    fn every_chain_bond_adds_up_and_its_blanks_solve_in_order() {
        let mut rng = Rng::new(43);
        for level in 1..=4u8 {
            for rot in 0..60 {
                let p = generate(level, Topic::Skill(Activity::BondChain), rot, &mut rng);
                let Kind::BondChain(c) = p.kind else { panic!("{:?}", p.kind) };
                for &(w, [a, b]) in &c.bonds {
                    assert_eq!(c.values[a] + c.values[b], c.values[w], "a bond that does not add up");
                }
                assert!(c.values.iter().all(|&v| v >= 1), "a zero circle teaches nothing");
                let (want, max) = match level { 1 | 2 => (2, 10), 3 => (3, 20), _ => (4, 50) };
                assert_eq!(c.order.len(), want, "blanks at level {level}");
                assert!(c.values[0] <= max);
                // Replay the order: each blank is the only unknown in a bond
                // when its turn comes, and some turn uses an earlier answer.
                let mut open = c.order.clone();
                let mut leaned = false;
                for (k, &next) in c.order.iter().enumerate() {
                    let bond = c.bonds.iter().find(|&&(w, [a, b])| {
                        let left: Vec<_> = [w, a, b].into_iter().filter(|i| open.contains(i)).collect();
                        left == [next]
                    });
                    let &(w, [a, b]) = bond.unwrap_or_else(|| panic!("blank {next} is not solvable at its turn in {c:?}"));
                    leaned |= [w, a, b].iter().any(|i| c.order[..k].contains(i));
                    open.retain(|&i| i != next);
                }
                assert!(leaned, "no step uses an earlier answer in {c:?}");
                assert!(c.at.iter().all(|&(_, x)| (100..=900).contains(&x)), "circles stay on the page");
            }
        }
    }

    #[test]
    fn a_chain_steps_through_its_blanks_then_stops() {
        let mut c = Chain {
            values: vec![9, 4, 5, 2, 3],
            bonds: vec![(0, [1, 2]), (2, [3, 4])],
            at: vec![(0, 500), (1, 280), (1, 720), (2, 600), (2, 840)],
            order: vec![3, 2],
            step: 0,
        };
        let p = Problem { kind: Kind::BondChain(c.clone()), prompt: "" };
        assert_eq!(p.expected(), "2");
        assert!(c.advance());
        assert!(c.is_solved(3) && !c.is_solved(2));
        assert_eq!(Problem { kind: Kind::BondChain(c.clone()), prompt: "" }.expected(), "5");
        assert!(!c.advance(), "the last blank ends the chain");
        assert_eq!(Chain::solve_order(&c.bonds, &[0, 1]), None, "a whole and its own part never solve alone");
    }

    #[test]
    fn making_ten_always_crosses_ten() {
        let mut rng = Rng::new(47);
        for level in 1..=4 {
            for rot in 0..40 {
                let p = generate(level, Topic::Skill(Activity::BridgeTen), rot, &mut rng);
                let Kind::BridgeTen { a, b } = p.kind else { panic!("{:?}", p.kind) };
                assert!(b > 10 - a, "{a} + {b} does not cross ten");
                assert!(a <= 9 && b <= 9, "both addends are single digits");
                assert_eq!(p.expected(), (a + b).to_string());
            }
        }
    }

    #[test]
    fn comparison_bars_and_skip_counts_stay_positive_and_under_a_hundred() {
        let mut rng = Rng::new(53);
        for level in 1..=4 {
            for rot in 0..60 {
                if let Kind::CompareBars { big, small } =
                    generate(level, Topic::Skill(Activity::CompareBars), rot, &mut rng).kind
                {
                    assert!(small >= 1 && big > small);
                    assert!(big <= if level <= 2 { 10 } else { 20 });
                }
                let p = generate(level, Topic::Skill(Activity::SkipCount), rot, &mut rng);
                let Kind::SkipCount { start, step } = p.kind else { panic!("{:?}", p.kind) };
                assert_eq!(start % step, 0, "counting starts on the step's own ladder");
                assert!(start + SKIP_SHOWN * step < 100, "{start} by {step}s runs past 99");
            }
        }
    }

    fn dealt(level: u8, act: Activity, n: usize) -> Vec<Kind> {
        let mut rng = Rng::new(17 + level as u32);
        (0..n).map(|_| problem_for(act, level, &mut rng).kind).collect()
    }

    #[test]
    fn column_sums_regroup_only_at_level_five() {
        for level in 3..=5u8 {
            for kind in dealt(level, Activity::ColumnAdd, 200) {
                let Kind::Column { a, op: Op::Add, b } = kind else { panic!("not a column sum: {kind:?}") };
                assert!(a + b <= 99, "{a} + {b} leaves two digits");
                assert!((10..=99).contains(&a), "{a} is not two digits");
                assert_eq!(a % 10 + b % 10 >= 10, level == 5, "regrouping wrong at level {level}: {a} + {b}");
                if level == 3 {
                    assert!(b <= 9, "level 3 adds one digit, got {b}");
                } else {
                    assert!(b >= 10, "level {level} adds two digits, got {b}");
                }
            }
            for kind in dealt(level, Activity::ColumnSub, 200) {
                let Kind::Column { a, op: Op::Sub, b } = kind else { panic!("not a column take-away: {kind:?}") };
                assert!(b < a, "{a} - {b} is not positive");
                assert_eq!(b % 10 > a % 10, level == 5, "borrowing wrong at level {level}: {a} - {b}");
            }
        }
    }

    #[test]
    fn times_facts_come_from_the_level_tables_and_division_is_exact() {
        for (level, tables) in [(4u8, vec![2u32, 5, 10]), (5, vec![2, 3, 4, 5, 10])] {
            for kind in dealt(level, Activity::TimesFacts, 200) {
                let Kind::Equation { a, op: Op::Mul, b } = kind else { panic!("{kind:?}") };
                assert!(tables.contains(&a) || tables.contains(&b), "{a} x {b} is off the level {level} tables");
                assert!(a.min(b) >= 1 && a.max(b) <= 10);
            }
            for kind in dealt(level, Activity::DivideFacts, 200) {
                let Kind::Equation { a, op: Op::Div, b } = kind else { panic!("{kind:?}") };
                assert!(tables.contains(&b) && a % b == 0 && a / b >= 2, "{a} / {b}");
            }
            for kind in dealt(level, Activity::MissingFactor, 200) {
                let Kind::MissingFactor { a, product } = kind else { panic!("{kind:?}") };
                assert!(tables.contains(&a) && product % a == 0 && product / a >= 2, "{a} x ? = {product}");
            }
            for kind in dealt(level, Activity::Groups, 200) {
                let Kind::Groups { groups, each } = kind else { panic!("{kind:?}") };
                assert!(tables.contains(&each) && (2..=5).contains(&groups));
            }
        }
        // The 3 and 4 tables wait for level 5.
        assert!(dealt(4, Activity::TimesFacts, 200).iter().all(|k| !matches!(
            k, Kind::Equation { a: 3 | 4, b: 3 | 4, .. }
        )));
    }

    #[test]
    fn number_writing_grows_from_digits_to_two_digit_numbers() {
        for (level, lo, hi) in [(1u8, 0u32, 9u32), (2, 10, 20), (3, 21, 99), (5, 21, 99)] {
            for kind in dealt(level, Activity::WriteNumbers, 100) {
                let Kind::TraceNumber { number } = kind else { panic!("{kind:?}") };
                assert!((lo..=hi).contains(&number), "level {level} wrote {number}");
            }
        }
        let p = Problem { kind: Kind::TraceNumber { number: 7 }, prompt: "" };
        assert_eq!(p.expected(), "7");
        assert!(p.brief().contains("backwards"), "a reversed digit must read as ALMOST");
    }

    #[test]
    fn the_numbers_topic_deals_only_number_writing() {
        let mut rng = Rng::new(9);
        for level in 1..=5 {
            for rot in 0..10 {
                let set = generate_set(level, Topic::Numbers, rot, &mut rng);
                assert!(set.items.iter().all(|p| matches!(p.kind, Kind::TraceNumber { .. })));
            }
        }
    }

    #[test]
    fn shape_answers_follow_the_shapes() {
        for level in 1..=MAX_LEVEL {
            for kind in dealt(level, Activity::ShapeSides, 100) {
                let Kind::ShapeSides { shape, .. } = kind else { panic!("{kind:?}") };
                assert!(shapes_for(level).contains(&shape));
                if level <= 2 {
                    assert_ne!(shape, Shape::Circle, "no trick circle before level 3");
                }
            }
            for kind in dealt(level, Activity::CountShapes, 100) {
                let Kind::CountShapes { target, scene } = &kind else { panic!("{kind:?}") };
                let mut cells: Vec<u8> = scene.iter().map(|&(_, c)| c).collect();
                cells.dedup();
                assert_eq!(cells.len(), scene.len(), "two shapes share a cell");
                assert!(cells.iter().all(|&c| c < SCENE_CELLS));
                let n = scene.iter().filter(|(s, _)| s == target).count();
                assert!(n >= 2 && n < scene.len(), "{n} targets in {scene:?}");
                assert!(scene.iter().all(|(s, _)| shapes_for(level).contains(s)));
                let p = Problem { kind: kind.clone(), prompt: "" };
                assert_eq!(p.expected(), n.to_string());
            }
            for kind in dealt(level, Activity::ShapePattern, 100) {
                let Kind::ShapePattern { cycle, shown } = &kind else { panic!("{kind:?}") };
                assert!(*shown as usize > cycle.len() && *shown <= 7, "{shown} shown of a {}-cycle", cycle.len());
                let has = |s: Shape| cycle.contains(&s);
                assert!(!(has(Shape::Square) && has(Shape::Rectangle)));
                assert!(cycle.iter().all(|s| matches!(s, Shape::Circle | Shape::Triangle | Shape::Square)));
            }
        }
        let p = Problem { kind: Kind::ShapePattern { cycle: vec![Shape::Circle, Shape::Square], shown: 5 }, prompt: "" };
        assert_eq!(p.expected(), "SQUARE");
        assert_eq!(Problem { kind: Kind::ShapeSides { shape: Shape::Hexagon, corners: true }, prompt: "" }.expected(), "6");
    }

    #[test]
    fn tick_box_questions_hold_their_answer_once() {
        for level in 1..=MAX_LEVEL {
            for act in [Activity::NameShape, Activity::FindShape] {
                for kind in dealt(level, act, 200) {
                    let p = Problem { kind: kind.clone(), prompt: "" };
                    let (Kind::NameShape { options, .. } | Kind::FindShape { options, .. }) = &kind else {
                        panic!("{kind:?}")
                    };
                    let right = p.choice_answer().expect("a tick-box question has a right box");
                    assert_eq!(options[right].name(), p.expected());
                    for i in 0..3 {
                        for j in i + 1..3 {
                            assert_ne!(options[i], options[j], "two boxes say the same: {options:?}");
                        }
                    }
                    if level <= 2 {
                        let has = |s| options.contains(&s);
                        assert!(!(has(Shape::Square) && has(Shape::Rectangle)), "{options:?} at level {level}");
                    }
                }
            }
        }
        // The right box moves around.
        let spots: std::collections::HashSet<usize> = dealt(3, Activity::NameShape, 60).iter()
            .filter_map(|k| Problem { kind: k.clone(), prompt: "" }.choice_answer())
            .collect();
        assert_eq!(spots.len(), 3);
        assert_eq!(Problem { kind: Kind::Equation { a: 1, op: Op::Add, b: 1 }, prompt: "" }.choice_answer(), None);
    }

    #[test]
    fn a_shape_page_says_its_own_question() {
        let mut rng = Rng::new(4);
        for rot in 0..12 {
            let set = generate_set(3, Topic::Shapes, rot, &mut rng);
            assert_eq!(set.items.len(), 1);
            assert_eq!(set.prompt, set.items[0].prompt);
            if let Kind::CountShapes { target, .. } = set.items[0].kind {
                assert!(set.prompt.contains(&target.name()[..4]), "{} for {target:?}", set.prompt);
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
