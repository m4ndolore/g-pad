//! Drawing the Learn worksheet, and the hit map that comes back from drawing.
//!
//! The one genuinely new piece of machinery the anthink design asks for: the
//! rendered regions of the current page with what each one means, built as a
//! RETURNED VALUE of drawing so a region can never drift from what was painted
//! (docs/anthink-interaction.md). A child's mark is then a bbox lookup, never
//! shape recognition.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

use crate::fb::{BBox, SCREEN_H, SCREEN_W};
use crate::script;
use crate::surface::{Surface, BLACK, FADED, WHITE};

use super::problems::{Blank, Kind, Op, Set};

const W: usize = SCREEN_W;
const H: usize = SCREEN_H;
const MARGIN: usize = W * 8 / 100;

/// Where the tutor's spoken feedback is written. Everything above belongs to
/// the exercise; everything below to the action boxes.
pub fn feedback_y() -> i32 {
    (H * 70 / 100) as i32
}

pub fn feedback_region() -> BBox {
    let mut b = BBox::empty();
    b.add(0, feedback_y() - 10, 0);
    b.add(W as i32 - 1, box_top() - 12, 0);
    b
}

pub fn box_top() -> i32 {
    (H - H * 8 / 100 - 40) as i32
}

/// What a mark can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Answer,
    Done,
    New,
    /// The MENU footer box: opens the topic-and-game picker.
    Menu,
    /// A choice box — a story path, or a picker entry — by index.
    Choice(usize),
}

/// The rendered regions of the current sheet.
#[derive(Clone, Debug)]
pub struct HitMap {
    /// The graded blank: on a practice sheet, the LAST question's box. The
    /// oracle marks only this one (the multi-question MVP); a mark and the
    /// verdict both anchor here.
    pub answer: BBox,
    pub done: BBox,
    pub new: BBox,
    /// The MENU footer box; empty only on the menu page itself.
    pub menu: BBox,
    /// Choice boxes (story paths, menu entries); empty elsewhere.
    pub choices: Vec<BBox>,
    /// Every answer box on the sheet, in question order (the last one is
    /// `answer`). DONE requires ink in each before the page commits.
    pub blanks: Vec<BBox>,
}

impl HitMap {
    /// Which element a point (a stroke's centroid) landed on.
    pub fn hit(&self, x: i32, y: i32) -> Option<Target> {
        let inside = |b: &BBox| !b.is_empty() && x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1;
        if inside(&self.done) {
            return Some(Target::Done);
        }
        if inside(&self.new) {
            return Some(Target::New);
        }
        if inside(&self.menu) {
            return Some(Target::Menu);
        }
        for (i, c) in self.choices.iter().enumerate() {
            if inside(c) {
                return Some(Target::Choice(i));
            }
        }
        if inside(&self.answer) {
            return Some(Target::Answer);
        }
        None
    }
}

/// Draw only the page footer — the NEW, MENU, and DONE decision boxes — and
/// return a hit map with everything else empty. The play pages start from
/// this and declare their own regions.
pub fn draw_footer(surf: &mut Surface, ui_font: &FontRef) -> HitMap {
    let done = draw_action_box(surf, ui_font, ActionBox::Done);
    let new = draw_action_box(surf, ui_font, ActionBox::New);
    let menu = draw_action_box(surf, ui_font, ActionBox::Menu);
    HitMap { answer: BBox::empty(), done, new, menu, choices: Vec::new(), blanks: Vec::new() }
}

/// A question's slice of the exercise area, top to bottom.
#[derive(Clone, Copy)]
struct Band {
    y0: i32,
    y1: i32,
}

impl Band {
    fn h(self) -> i32 {
        self.y1 - self.y0
    }
    fn mid(self) -> i32 {
        (self.y0 + self.y1) / 2
    }
}

/// Draw the whole sheet for `set` onto a white page and return its hit map.
/// One question keeps the classic full-page figure; two to four stack in
/// numbered bands, each with its own blank — the LAST blank is the graded
/// `answer`. `level`, `streak`, and `score` feed the header; they are the
/// child's only visible progress chrome.
pub fn draw(surf: &mut Surface, ui_font: &FontRef, set: &Set, level: u8, streak: u32, score: u32) -> HitMap {
    surf.fill_rect(0, 0, W, H, WHITE);

    // Header: what this page is, the session score, and the streak dots.
    // Quiet, top corners — the score takes the free top-center and only once
    // it exists, so the first page opens as calm as ever.
    print(surf, ui_font, &format!("LEARN · LEVEL {level}"), 32.0, MARGIN, 40, BLACK);
    if score > 0 {
        let txt = format!("{score}");
        let px = 40.0;
        let r = 15;
        let tw = tight_width(ui_font, &txt, px);
        let left = (W as i32 - (2 * r + 14 + tw)) / 2;
        draw_star(surf, left + r, 56, r);
        print_tight_at(surf, ui_font, &txt, px, left + 2 * r + 14, 56, BLACK);
    }
    let dots = streak.min(5) as i32;
    for i in 0..dots {
        surf.stamp((W - MARGIN) as i32 - 20 - i * 44, 56, 11, BLACK);
    }

    // The instruction line, printed large enough for an early reader.
    print_centered(surf, ui_font, set.prompt, 46.0, (W / 2) as i32, (H * 13 / 100) as i32, BLACK);

    let n = set.items.len().max(1) as i32;
    let (top, bottom) = ((H * 18 / 100) as i32, feedback_y() - 24);
    let mut blanks = Vec::new();
    for (i, problem) in set.items.iter().enumerate() {
        let band = Band {
            y0: top + (bottom - top) * i as i32 / n,
            y1: top + (bottom - top) * (i as i32 + 1) / n,
        };
        if n > 1 {
            // The question number, out in the margin where figures never go.
            print_tight_centered(surf, ui_font, &format!("{}", i + 1), 38.0, (MARGIN / 2) as i32, band.mid(), BLACK);
        }
        let blank = match &problem.kind {
            // The full-page figures: always alone on their sheet.
            Kind::Bond { parts, blank, whole } => draw_bond(surf, ui_font, *whole, *parts, *blank),
            Kind::TenFrame { shown, make_ten } => draw_ten_frame(surf, ui_font, *shown, *make_ten),
            Kind::Array { rows, cols } => draw_array(surf, ui_font, *rows, *cols),
            Kind::Share { total, groups } => draw_share(surf, ui_font, *total, *groups),
            Kind::Trace { word } => draw_trace(surf, ui_font, word),
            // The band figures: scale to their slice of the page.
            Kind::Equation { a, op, b } => draw_equation(surf, ui_font, *a, *op, *b, band),
            Kind::Bar { whole, parts, blank } => draw_bar(surf, ui_font, *whole, *parts, *blank, band),
            Kind::NumberLine { from, delta, top } => draw_number_line(surf, ui_font, *from, *delta, *top, band),
            Kind::PlaceValue { tens, ones } => draw_place_value(surf, ui_font, *tens, *ones, band),
            Kind::HundredWindow { center } => draw_hundred_window(surf, ui_font, *center, band),
            Kind::Compare { left, right } => draw_compare(surf, ui_font, *left, *right, band),
        };
        blanks.push(blank);
    }

    let done = draw_action_box(surf, ui_font, ActionBox::Done);
    let new = draw_action_box(surf, ui_font, ActionBox::New);
    let menu = draw_action_box(surf, ui_font, ActionBox::Menu);
    let answer = blanks.last().copied().unwrap_or_else(BBox::empty);
    HitMap { answer, done, new, menu, choices: Vec::new(), blanks }
}

/// Repaint the decision boxes: called after an absorbed mark's white-out,
/// which may have clipped a box edge or label.
pub fn refresh_boxes(surf: &mut Surface, ui_font: &FontRef) {
    let _ = draw_action_box(surf, ui_font, ActionBox::Done);
    let _ = draw_action_box(surf, ui_font, ActionBox::New);
    let _ = draw_action_box(surf, ui_font, ActionBox::Menu);
}

// ---- the figures ----------------------------------------------------------

fn draw_bond(surf: &mut Surface, font: &FontRef, whole: u32, parts: [u32; 2], blank: Blank) -> BBox {
    let r = (W * 9 / 100) as i32;
    let cx = (W / 2) as i32;
    let (wy, py) = ((H * 30 / 100) as i32, (H * 52 / 100) as i32);
    let dx = (W * 18 / 100) as i32;
    let centers = [(cx, wy), (cx - dx, py), (cx + dx, py)]; // whole, part 0, part 1

    // Connectors first so the rings sit on top of their ends.
    for &(px, py2) in &centers[1..] {
        line(surf, cx, wy, px, py2, 4, BLACK);
    }

    let blank_i = match blank {
        Blank::Whole => 0,
        Blank::Part(i) => 1 + i,
    };
    let values = [whole, parts[0], parts[1]];
    let mut answer = BBox::empty();
    for (i, &(x, y)) in centers.iter().enumerate() {
        // Blot the connector out of the circle's interior.
        surf.stamp(x, y, r - 3, WHITE);
        ring(surf, x, y, r, if i == blank_i { 9 } else { 5 }, BLACK);
        if i == blank_i {
            answer.add(x - r + 12, y - r + 12, 0);
            answer.add(x + r - 12, y + r - 12, 0);
        } else {
            print_tight_centered(surf, font, &values[i].to_string(), 130.0, x, y, BLACK);
        }
    }
    answer
}

fn draw_ten_frame(surf: &mut Surface, font: &FontRef, shown: u32, make_ten: bool) -> BBox {
    let cell = (W * 9 / 100) as i32;
    let x0 = (W as i32 - 5 * cell) / 2;
    let y0 = (H * 22 / 100) as i32;
    for row in 0..=2 {
        line(surf, x0, y0 + row * cell, x0 + 5 * cell, y0 + row * cell, 2, BLACK);
    }
    for col in 0..=5 {
        line(surf, x0 + col * cell, y0, x0 + col * cell, y0 + 2 * cell, 2, BLACK);
    }
    for i in 0..shown.min(10) as i32 {
        let (row, col) = (i / 5, i % 5);
        surf.stamp(x0 + col * cell + cell / 2, y0 + row * cell + cell / 2, cell * 30 / 100, BLACK);
    }

    let eq_y = y0 + 2 * cell + (H * 12 / 100) as i32;
    if make_ten {
        equation_with_box(surf, font, &format!("{shown} + "), " = 10", eq_y)
    } else {
        answer_box_centered(surf, (W / 2) as i32, eq_y)
    }
}

fn draw_equation(surf: &mut Surface, font: &FontRef, a: u32, op: Op, b: u32, band: Band) -> BBox {
    equation_with_box_sized(surf, font, &format!("{a} {} {b} = ", op.glyph()), "", band.mid(), band_sizes(band))
}

/// Print, box, and box sizes that fit a band: the full-page sizes, shrunk
/// only when the slice is too short for them.
fn band_sizes(band: Band) -> (f32, i32, i32) {
    let h = band.h();
    let (bw, bh) = box_size();
    let px = (120.0f32).min(h as f32 * 0.42).max(56.0);
    (px, bw.min(h * 78 / 100), bh.min(h * 70 / 100))
}

/// Two numbers with an empty box between: the child writes <, =, or >.
fn draw_compare(surf: &mut Surface, font: &FontRef, left: u32, right: u32, band: Band) -> BBox {
    equation_with_box_sized(surf, font, &format!("{left} "), &format!(" {right}"), band.mid(), band_sizes(band))
}

/// The part-whole bar model: one bar split proportionally into two segments,
/// the whole bracketed beneath. The blank number's place holds the box.
fn draw_bar(surf: &mut Surface, font: &FontRef, whole: u32, parts: [u32; 2], blank: Blank, band: Band) -> BBox {
    let h = band.h();
    let (x0, x1) = (MARGIN as i32, (W - MARGIN) as i32);
    let bar_h = (h * 32 / 100).min(150);
    let bar_y = band.y0 + h * 8 / 100;
    let split = x0 + (x1 - x0) * parts[0] as i32 / whole.max(1) as i32;
    rect_outline(surf, x0, bar_y, x1 - x0, bar_h, 5, BLACK);
    line(surf, split, bar_y, split, bar_y + bar_h, 3, BLACK);

    let label_px = (bar_h as f32 * 0.55).min(80.0);
    let box_side = (bar_h - 24).min(130);
    let mut answer = BBox::empty();
    let seg_mid = [(x0 + split) / 2, (split + x1) / 2];
    for (i, &cx) in seg_mid.iter().enumerate() {
        if blank == Blank::Part(i) {
            answer = answer_box(surf, cx - box_side / 2, bar_y + (bar_h - box_side) / 2, box_side, box_side);
        } else {
            print_tight_centered(surf, font, &parts[i].to_string(), label_px, cx, bar_y + bar_h / 2, BLACK);
        }
    }

    // The whole: a bracket under the full bar, its number (or box) beneath.
    let brk_y = bar_y + bar_h + h * 8 / 100;
    line(surf, x0, brk_y, x1, brk_y, 3, BLACK);
    line(surf, x0, brk_y - 14, x0, brk_y, 3, BLACK);
    line(surf, x1, brk_y - 14, x1, brk_y, 3, BLACK);
    let cx = (x0 + x1) / 2;
    let whole_y = brk_y + h * 10 / 100 + box_side / 2;
    if blank == Blank::Whole {
        answer = answer_box(surf, cx - box_side / 2, whole_y - box_side / 2, box_side, box_side);
    } else {
        print_tight_centered(surf, font, &whole.to_string(), label_px, cx, whole_y, BLACK);
    }
    answer
}

/// A number-line walk: the sum printed with its box above, the line below
/// with a labeled tick per number and one arc per unit hop.
fn draw_number_line(surf: &mut Surface, font: &FontRef, from: u32, delta: i32, top: u32, band: Band) -> BBox {
    let h = band.h();
    let (sign, n) = if delta >= 0 { ('+', delta) } else { ('-', -delta) };
    let (_, bw, bh) = band_sizes(band);
    let eq_px = (h as f32 * 0.28).min(100.0);
    let answer = equation_with_box_sized(
        surf,
        font,
        &format!("{from} {sign} {n} = "),
        "",
        band.y0 + h * 28 / 100,
        (eq_px, (bw * 3 / 4).max(110), (bh * 3 / 4).max(110)),
    );

    let (x0, x1) = (MARGIN as i32, (W - MARGIN) as i32);
    let line_y = band.y1 - h * 18 / 100;
    line(surf, x0, line_y, x1, line_y, 3, BLACK);
    let step = (x1 - x0) / top as i32;
    for t in 0..=top as i32 {
        let x = x0 + t * step;
        line(surf, x, line_y - 12, x, line_y + 12, 2, BLACK);
        print_tight_centered(surf, font, &t.to_string(), 26.0, x, line_y + 40, BLACK);
    }
    // One arc per unit hop, from `from` toward the landing number.
    let hop_h = (h * 14 / 100).min(64);
    let dir = if delta >= 0 { 1 } else { -1 };
    for k in 0..n {
        let a = x0 + (from as i32 + dir * k) * step;
        let b = a + dir * step;
        arc(surf, a, b, line_y - 6, hop_h);
    }
    answer
}

/// An arch from `xa` to `xb` rising `rise` above `y`, drawn as a polyline.
fn arc(surf: &mut Surface, xa: i32, xb: i32, y: i32, rise: i32) {
    let steps = 10;
    let mut prev = (xa, y);
    for s in 1..=steps {
        let t = s as f32 / steps as f32;
        let x = xa as f32 + (xb - xa) as f32 * t;
        let ang = std::f32::consts::PI * t;
        let py = y as f32 - ang.sin() * rise as f32;
        let cur = (x as i32, py as i32);
        surf.brush_line(prev.0, prev.1, cur.0, cur.1, 3, BLACK);
        prev = cur;
    }
}

/// The place-value chart: TENS and ONES columns of discs, the number box to
/// the right. Ten-discs are rings labeled 10; one-discs are dots.
fn draw_place_value(surf: &mut Surface, font: &FontRef, tens: u32, ones: u32, band: Band) -> BBox {
    let h = band.h();
    let cw = ((W as i32 - 2 * MARGIN as i32) * 22 / 100).min(280);
    let ch = (h * 78 / 100).min(560);
    let box_side = (h * 34 / 100).min(150);
    let total_w = 2 * cw + 60 + box_side + 60;
    let x0 = (W as i32 - total_w) / 2;
    let y0 = band.y0 + (h - ch) / 2;

    rect_outline(surf, x0, y0, 2 * cw, ch, 4, BLACK);
    line(surf, x0 + cw, y0, x0 + cw, y0 + ch, 3, BLACK);
    let head_h = (ch * 16 / 100).min(70);
    line(surf, x0, y0 + head_h, x0 + 2 * cw, y0 + head_h, 2, BLACK);
    print_tight_centered(surf, font, "TENS", 30.0, x0 + cw / 2, y0 + head_h / 2, BLACK);
    print_tight_centered(surf, font, "ONES", 30.0, x0 + cw + cw / 2, y0 + head_h / 2, BLACK);

    // Discs, three to a row inside their column; up to nine of a kind means
    // three rows, so the pitch also fits the column's height.
    let disc = |surf: &mut Surface, font: &FontRef, col_x: i32, i: i32, ten: bool| {
        let (row, col) = (i / 3, i % 3);
        let gap = (cw / 3).min((ch - head_h) / 3);
        let cx = col_x + gap / 2 + col * gap;
        let cy = y0 + head_h + gap / 2 + row * gap;
        if ten {
            ring(surf, cx, cy, gap * 36 / 100, 4, BLACK);
            print_tight_centered(surf, font, "10", (gap as f32 * 0.32).max(20.0), cx, cy, BLACK);
        } else {
            surf.stamp(cx, cy, gap * 18 / 100, BLACK);
        }
    };
    for i in 0..tens.min(9) as i32 {
        disc(surf, font, x0, i, true);
    }
    for i in 0..ones.min(9) as i32 {
        disc(surf, font, x0 + cw, i, false);
    }

    let eq_x = x0 + 2 * cw + 30;
    let cy = y0 + ch / 2;
    print_tight_at(surf, font, "=", (box_side as f32 * 0.5).max(40.0), eq_x, cy, BLACK);
    answer_box(surf, eq_x + 60, cy - box_side / 2, box_side, box_side)
}

/// The hundred-chart window: a 3×3 grid holding `center`'s cross neighbors —
/// one less, one more, ten less, ten more — with the middle cell blank.
fn draw_hundred_window(surf: &mut Surface, font: &FontRef, center: u32, band: Band) -> BBox {
    let h = band.h();
    let cell = (h * 28 / 100).min(140);
    let x0 = (W as i32 - 3 * cell) / 2;
    let y0 = band.y0 + (h - 3 * cell) / 2;
    for i in 0..=3 {
        line(surf, x0, y0 + i * cell, x0 + 3 * cell, y0 + i * cell, 2, BLACK);
        line(surf, x0 + i * cell, y0, x0 + i * cell, y0 + 3 * cell, 2, BLACK);
    }
    let c = center as i32;
    let num_px = (cell as f32 * 0.42).max(30.0);
    for (row, col, v) in [(0, 1, c - 10), (1, 0, c - 1), (1, 2, c + 1), (2, 1, c + 10)] {
        let (cx, cy) = (x0 + col * cell + cell / 2, y0 + row * cell + cell / 2);
        print_tight_centered(surf, font, &v.to_string(), num_px, cx, cy, BLACK);
    }
    // The middle cell is the blank: a bold outline says "write here".
    answer_box(surf, x0 + cell, y0 + cell, cell, cell)
}

fn draw_array(surf: &mut Surface, font: &FontRef, rows: u32, cols: u32) -> BBox {
    let gap = (W * 7 / 100) as i32;
    let x0 = (W as i32 - (cols as i32 - 1) * gap) / 2;
    let y0 = (H * 24 / 100) as i32;
    for r in 0..rows as i32 {
        for c in 0..cols as i32 {
            surf.stamp(x0 + c * gap, y0 + r * gap, gap * 22 / 100, BLACK);
        }
    }
    let eq_y = y0 + rows as i32 * gap + (H * 8 / 100) as i32;
    equation_with_box(surf, font, &format!("{rows} x {cols} = "), "", eq_y)
}

fn draw_share(surf: &mut Surface, font: &FontRef, total: u32, groups: u32) -> BBox {
    let each = (total / groups.max(1)).max(1);
    let gap = (W * 7 / 100) as i32;
    let x0 = (W as i32 - (each as i32 - 1) * gap) / 2;
    let y0 = (H * 24 / 100) as i32;
    for g in 0..groups as i32 {
        for i in 0..each as i32 {
            surf.stamp(x0 + i * gap, y0 + g * gap, gap * 22 / 100, BLACK);
        }
    }
    let eq_y = y0 + groups as i32 * gap + (H * 8 / 100) as i32;
    equation_with_box(surf, font, &format!("{total} / {groups} = "), "", eq_y)
}

/// Handwriting practice: dashed skeleton letters on the upper rule group to
/// trace, an empty rule group below to write on. The guide is drawn from the
/// same rasterize→thin→trace pipeline the pad's own hand uses.
///
/// The rules are the classic equal-spaced trio — top line, dashed midline at
/// exactly half the writing height, heavy baseline — and the glyphs are
/// sized FROM the midline: the template is scaled so the font's true
/// x-height lands on the dashed line, because "small letters stop at the
/// middle" is the lesson. Tall letters then rise above the midline toward
/// the top line without quite touching it — the print-font compromise every
/// paper worksheet set in a regular font makes.
fn draw_trace(surf: &mut Surface, font: &FontRef, word: &str) -> BBox {
    let gap = (H * 55 / 1000) as i32; // half the writing height
    let base1 = (H * 34 / 100) as i32;
    let base2 = (H * 56 / 100) as i32;

    // Fit: x-height → gap (the midline), capped by the content width.
    let probe = 100.0f32;
    let x_h = tight_text_height(font, "x", probe).max(1) as f32;
    let mut px = probe * gap as f32 / x_h;
    let max_w = (W - 2 * MARGIN) as f32;
    let w = script::measure(font, word, px);
    if w > max_w {
        px *= max_w / w;
    }

    for &base in &[base1, base2] {
        rule_group(surf, base, 2 * gap, gap);
    }

    let scaled = font.as_scaled(PxScale::from(px));
    let ascent = scaled.ascent().ceil() as i32;
    let mut raster = script::rasterize_line(font, word, px);
    script::thin(&mut raster);
    let x0 = ((W as i32 - raster.width as i32) / 2).max(MARGIN as i32);
    let y0 = base1 - ascent; // glyph baseline onto the writing baseline
    for stroke in script::trace(&raster) {
        for (i, &(sx, sy)) in stroke.iter().enumerate() {
            if (i / 7) % 2 == 0 {
                surf.stamp(x0 + sx, y0 + sy, 2, BLACK);
            }
        }
    }

    let mut answer = BBox::empty();
    answer.add(MARGIN as i32, base1 - 2 * gap - 20, 0);
    answer.add((W - MARGIN) as i32, base2 + gap + 30, 0);
    answer
}

/// One handwriting rule group: top line at `top_h` above the heavy
/// baseline, dashed midline at `mid_off` above it (half the writing
/// height — where the small letters stop).
fn rule_group(surf: &mut Surface, baseline: i32, top_h: i32, mid_off: i32) {
    let (x0, x1) = (MARGIN as i32, (W - MARGIN) as i32);
    line(surf, x0, baseline - top_h, x1, baseline - top_h, 1, FADED);
    dashed_hline(surf, x0, x1, baseline - mid_off, 2, FADED);
    line(surf, x0, baseline, x1, baseline, 3, BLACK);
}

/// Height of the inked pixels of `text` at `px` — true glyph metrics, not em.
fn tight_text_height(font: &FontRef, text: &str, px: f32) -> i32 {
    let raster = script::rasterize_line(font, text, px);
    tight_bounds(&raster).map(|(_, _, _, h)| h).unwrap_or(0)
}

// ---- shared pieces --------------------------------------------------------

/// `lead` + answer box + `tail`, tight-centered as one row at the full-page
/// size. Returns the box's interior as the answer region.
fn equation_with_box(surf: &mut Surface, font: &FontRef, lead: &str, tail: &str, cy: i32) -> BBox {
    let (bw, bh) = box_size();
    equation_with_box_sized(surf, font, lead, tail, cy, (120.0, bw, bh))
}

/// The same row at any size: `px` for the print, `bw`×`bh` for the box.
fn equation_with_box_sized(
    surf: &mut Surface,
    font: &FontRef,
    lead: &str,
    tail: &str,
    cy: i32,
    (px, bw, bh): (f32, i32, i32),
) -> BBox {
    let gap = 24i32;
    let lead_w = tight_width(font, lead, px);
    let tail_w = tight_width(font, tail, px);
    let total = lead_w + gap + bw + if tail_w > 0 { gap + tail_w } else { 0 };
    let mut x = (W as i32 - total) / 2;
    if !lead.trim().is_empty() {
        print_tight_at(surf, font, lead, px, x, cy, BLACK);
    }
    x += lead_w + gap;
    let b = answer_box(surf, x, cy - bh / 2, bw, bh);
    x += bw + gap;
    if !tail.trim().is_empty() {
        print_tight_at(surf, font, tail, px, x, cy, BLACK);
    }
    b
}

fn box_size() -> (i32, i32) {
    ((W * 16 / 100) as i32, (W * 14 / 100) as i32)
}

fn answer_box_centered(surf: &mut Surface, cx: i32, cy: i32) -> BBox {
    let (bw, bh) = box_size();
    answer_box(surf, cx - bw / 2, cy - bh / 2, bw, bh)
}

/// The blank the child writes in: a bold outlined box, interior returned.
fn answer_box(surf: &mut Surface, x: i32, y: i32, w: i32, h: i32) -> BBox {
    rect_outline(surf, x, y, w, h, 7, BLACK);
    let mut b = BBox::empty();
    b.add(x + 10, y + 10, 0);
    b.add(x + w - 10, y + h - 10, 0);
    b
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionBox {
    New,
    Menu,
    Done,
}

/// NEW (left), MENU (center), DONE (right) at the foot of every sheet: the
/// anchored decision boxes. Any mark inside one is a command — form never
/// matters.
fn draw_action_box(surf: &mut Surface, font: &FontRef, which: ActionBox) -> BBox {
    let bw = (W * 22 / 100) as i32;
    let bh = (H * 8 / 100) as i32;
    let y = box_top();
    let x = match which {
        ActionBox::New => MARGIN as i32,
        ActionBox::Menu => (W as i32 - bw) / 2,
        ActionBox::Done => (W - MARGIN) as i32 - bw,
    };
    rect_outline(surf, x, y, bw, bh, 4, BLACK);
    let label = match which {
        ActionBox::New => "NEW",
        ActionBox::Menu => "MENU",
        ActionBox::Done => "DONE",
    };
    print_tight_centered(surf, font, label, 42.0, x + bw / 2, y + bh / 2, BLACK);
    let mut b = BBox::empty();
    b.add(x, y, 0);
    b.add(x + bw, y + bh, 0);
    b
}

// ---- the menu page --------------------------------------------------------

/// The picker's entries, in choice-index order. `Session::choose_menu`
/// interprets the index, so the two must move together: 0 math, 1 writing,
/// 2 the full mix, 3 the skills page, 4–6 the games — and after the named
/// entries, four LEVEL boxes at indices 7–10.
pub const MENU_ITEMS: &[&str] = &[
    "MATH",
    "WRITING",
    "SURPRISE MIX",
    "MATH SKILLS",
    "DOODLE CRITTER",
    "GUESSING GAME",
    "STORY TIME",
];

/// How many leading MENU_ITEMS are practice topics (the rest are games).
const MENU_PRACTICE: usize = 4;

/// The levels the menu's LEVEL row offers, boxes 7–10.
const MENU_LEVELS: u8 = 4;

/// Draw the topic-and-game picker and return its hit map: every entry is a
/// choice box, and there is deliberately no DONE, NEW, or MENU here — a mark
/// in an entry is the only thing this page understands. `level` fills in the
/// LEVEL row's current box so the child can see where the ladder stands.
pub fn draw_menu(surf: &mut Surface, ui_font: &FontRef, level: u8) -> HitMap {
    surf.fill_rect(0, 0, W, H, WHITE);
    print(surf, ui_font, "LEARN · MENU", 32.0, MARGIN, 40, BLACK);
    print_centered(surf, ui_font, "MARK A BOX TO CHOOSE", 46.0, (W / 2) as i32, (H * 8 / 100) as i32, BLACK);

    let gap_x = (W * 4 / 100) as i32;
    let bw = ((W - 2 * MARGIN) as i32 - gap_x) / 2;
    let bh = (H * 8 / 100) as i32;
    let row_h = bh + (H * 25 / 1000) as i32;
    let mut choices = Vec::new();

    let section = |surf: &mut Surface, label: &str, y: i32, items: &[&str], base: usize, choices: &mut Vec<BBox>| {
        print(surf, ui_font, label, 30.0, MARGIN, y as usize, BLACK);
        let top = y + 50;
        for (i, item) in items.iter().enumerate() {
            let (row, col) = ((i / 2) as i32, (i % 2) as i32);
            let x = MARGIN as i32 + col * (bw + gap_x);
            let by = top + row * row_h;
            rect_outline(surf, x, by, bw, bh, 4, BLACK);
            let px = fit_px(ui_font, item, 40.0, (bw - 40) as f32);
            print_tight_centered(surf, ui_font, item, px, x + bw / 2, by + bh / 2, BLACK);
            let mut b = BBox::empty();
            b.add(x, by, 0);
            b.add(x + bw, by + bh, 0);
            debug_assert_eq!(choices.len(), base + i);
            choices.push(b);
        }
        top + items.len().div_ceil(2) as i32 * row_h
    };

    let after_practice = section(
        surf,
        "PRACTICE",
        (H * 14 / 100) as i32,
        &MENU_ITEMS[..MENU_PRACTICE],
        0,
        &mut choices,
    );
    let after_play = section(
        surf,
        "PLAY",
        after_practice + (H * 4 / 100) as i32,
        &MENU_ITEMS[MENU_PRACTICE..],
        MENU_PRACTICE,
        &mut choices,
    );

    // The LEVEL row: four boxes across, the current level filled solid so it
    // reads at a glance. Marking one re-seats the ladder and keeps the menu
    // open; the indices continue after the named entries.
    let ly = after_play + (H * 4 / 100) as i32;
    print(surf, ui_font, "LEVEL", 30.0, MARGIN, ly as usize, BLACK);
    let top = ly + 50;
    let lw = ((W - 2 * MARGIN) as i32 - 3 * gap_x) / 4;
    for k in 0..MENU_LEVELS {
        let x = MARGIN as i32 + k as i32 * (lw + gap_x);
        let (ink, paper) = if k + 1 == level { (WHITE, BLACK) } else { (BLACK, WHITE) };
        if paper == BLACK {
            surf.fill_rect(x.max(0) as usize, top.max(0) as usize, lw as usize, bh as usize, BLACK);
        } else {
            rect_outline(surf, x, top, lw, bh, 4, BLACK);
        }
        print_tight_centered(surf, ui_font, &format!("{}", k + 1), 52.0, x + lw / 2, top + bh / 2, ink);
        let mut b = BBox::empty();
        b.add(x, top, 0);
        b.add(x + lw, top + bh, 0);
        choices.push(b);
    }

    HitMap {
        answer: BBox::empty(),
        done: BBox::empty(),
        new: BBox::empty(),
        menu: BBox::empty(),
        choices,
        blanks: Vec::new(),
    }
}

/// Draw the skills picker — every math activity as its own box — and return
/// its hit map. Reached from the menu's MATH SKILLS entry; the indices mirror
/// `problems::MATH_SKILLS`.
pub fn draw_skills(surf: &mut Surface, ui_font: &FontRef) -> HitMap {
    surf.fill_rect(0, 0, W, H, WHITE);
    print(surf, ui_font, "LEARN · SKILLS", 32.0, MARGIN, 40, BLACK);
    print_centered(surf, ui_font, "MARK A SKILL TO PRACTICE", 46.0, (W / 2) as i32, (H * 8 / 100) as i32, BLACK);

    let gap_x = (W * 4 / 100) as i32;
    let bw = ((W - 2 * MARGIN) as i32 - gap_x) / 2;
    let bh = (H * 8 / 100) as i32;
    let row_h = bh + (H * 25 / 1000) as i32;
    let top = (H * 14 / 100) as i32;
    let mut choices = Vec::new();
    for (i, act) in super::problems::MATH_SKILLS.iter().enumerate() {
        let (row, col) = ((i / 2) as i32, (i % 2) as i32);
        let x = MARGIN as i32 + col * (bw + gap_x);
        let by = top + row * row_h;
        rect_outline(surf, x, by, bw, bh, 4, BLACK);
        let label = act.label();
        let px = fit_px(ui_font, label, 40.0, (bw - 40) as f32);
        print_tight_centered(surf, ui_font, label, px, x + bw / 2, by + bh / 2, BLACK);
        let mut b = BBox::empty();
        b.add(x, by, 0);
        b.add(x + bw, by + bh, 0);
        choices.push(b);
    }

    HitMap {
        answer: BBox::empty(),
        done: BBox::empty(),
        new: BBox::empty(),
        menu: BBox::empty(),
        choices,
        blanks: Vec::new(),
    }
}

fn fit_px(font: &FontRef, text: &str, px: f32, max_w: f32) -> f32 {
    let w = script::measure(font, text, px);
    if w > max_w {
        px * max_w / w
    } else {
        px
    }
}

// ---- primitives -----------------------------------------------------------

fn line(surf: &mut Surface, x0: i32, y0: i32, x1: i32, y1: i32, r: i32, c: u16) {
    surf.brush_line(x0, y0, x1, y1, r, c);
}

fn dashed_hline(surf: &mut Surface, x0: i32, x1: i32, y: i32, r: i32, c: u16) {
    let (dash, gap) = (22, 16);
    let mut x = x0;
    while x < x1 {
        let e = (x + dash).min(x1);
        surf.brush_line(x, y, e, y, r, c);
        x = e + gap;
    }
}

/// An outlined circle: every pixel whose distance to the center falls inside
/// the band `[r - thick, r]`.
fn ring(surf: &mut Surface, cx: i32, cy: i32, r: i32, thick: i32, c: u16) {
    let (lo2, hi2) = ((r - thick) * (r - thick), r * r);
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            if d2 >= lo2 && d2 <= hi2 {
                surf.put_px(cx + dx, cy + dy, c);
            }
        }
    }
}

fn rect_outline(surf: &mut Surface, x: i32, y: i32, w: i32, h: i32, thick: i32, c: u16) {
    let t = thick.max(1) as usize;
    let (x, y, w, h) = (x.max(0) as usize, y.max(0) as usize, w.max(0) as usize, h.max(0) as usize);
    surf.fill_rect(x, y, w, t, c);
    surf.fill_rect(x, y + h - t, w, t, c);
    surf.fill_rect(x, y, t, h, c);
    surf.fill_rect(x + w - t, y, t, h, c);
}

fn print(surf: &mut Surface, font: &FontRef, text: &str, px: f32, x: usize, y: usize, c: u16) {
    let raster = script::rasterize_line(font, text, px);
    blit(surf, &raster, x as i32, y as i32, c);
}

fn print_centered(surf: &mut Surface, font: &FontRef, text: &str, px: f32, cx: i32, y: i32, c: u16) {
    let raster = script::rasterize_line(font, text, px);
    blit(surf, &raster, cx - raster.width as i32 / 2, y, c);
}

/// Draw with the mask's INK (not its em box) centered on (cx, cy) — the only
/// way digits sit visually centered inside a circle.
fn print_tight_centered(surf: &mut Surface, font: &FontRef, text: &str, px: f32, cx: i32, cy: i32, c: u16) {
    let raster = script::rasterize_line(font, text, px);
    if let Some((tx0, ty0, tw, th)) = tight_bounds(&raster) {
        blit(surf, &raster, cx - tx0 - tw / 2, cy - ty0 - th / 2, c);
    }
}

/// Draw with the ink's left edge at `x`, vertically ink-centered on `cy`.
fn print_tight_at(surf: &mut Surface, font: &FontRef, text: &str, px: f32, x: i32, cy: i32, c: u16) {
    let raster = script::rasterize_line(font, text, px);
    if let Some((tx0, ty0, _, th)) = tight_bounds(&raster) {
        blit(surf, &raster, x - tx0, cy - ty0 - th / 2, c);
    }
}

fn tight_width(font: &FontRef, text: &str, px: f32) -> i32 {
    if text.trim().is_empty() {
        return 0;
    }
    let raster = script::rasterize_line(font, text, px);
    tight_bounds(&raster).map(|(_, _, w, _)| w).unwrap_or(0)
}

fn tight_bounds(raster: &script::Line) -> Option<(i32, i32, i32, i32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for y in 0..raster.height {
        for x in 0..raster.width {
            if raster.mask[y * raster.width + x] {
                x0 = x0.min(x as i32);
                y0 = y0.min(y as i32);
                x1 = x1.max(x as i32);
                y1 = y1.max(y as i32);
            }
        }
    }
    (x0 <= x1).then_some((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

fn blit(surf: &mut Surface, raster: &script::Line, x: i32, y: i32, c: u16) {
    for row in 0..raster.height {
        for col in 0..raster.width {
            if raster.mask[row * raster.width + col] {
                surf.put_px(x + col as i32, y + row as i32, c);
            }
        }
    }
}

// ---- marks ----------------------------------------------------------------

/// A drawn check beside the answer region: the sheet agrees with the child.
pub fn draw_check(surf: &mut Surface, answer: &BBox) -> BBox {
    let s = (W * 3 / 100) as i32;
    let cy = (answer.y0 + answer.y1) / 2;
    let cx = if answer.x1 + 4 * s < W as i32 { answer.x1 + 2 * s } else { answer.x0 - 3 * s };
    surf.brush_line(cx - s, cy, cx - s / 4, cy + s * 3 / 4, 5, BLACK);
    surf.brush_line(cx - s / 4, cy + s * 3 / 4, cx + s, cy - s * 3 / 4, 5, BLACK);
    let mut b = BBox::empty();
    b.add(cx, cy, 2 * s);
    b
}

/// The verdict, written for its two readers at once: the cheer huge and
/// centered for the child (stars flank it when `starred`, and the points it
/// scored ride beside the right star when `award` is nonzero), the hint in
/// small print beneath for the grown-up to read aloud. Printed instantly, not
/// handwritten — feedback should land like a teacher's stamp, and a child
/// should never wait through an animation to learn they were right.
pub fn draw_feedback(surf: &mut Surface, font: &FontRef, cheer: &str, hint: &str, starred: bool, award: u32) -> BBox {
    let cx = (W / 2) as i32;
    let cheer_cy = feedback_y() + (H * 4 / 100) as i32;
    if !cheer.is_empty() {
        let reserved = if starred { W * 16 / 100 } else { 0 } + if award > 0 { W * 12 / 100 } else { 0 };
        let max_w = (W - 2 * MARGIN - reserved) as f32;
        let mut px = 120.0;
        let wide = script::measure(font, cheer, px);
        if wide > max_w {
            px = (px * max_w / wide).max(56.0);
        }
        print_tight_centered(surf, font, cheer, px, cx, cheer_cy, BLACK);
        if starred {
            let r = (W * 5 / 200) as i32;
            let half = tight_width(font, cheer, px) / 2;
            draw_star(surf, cx - half - 2 * r, cheer_cy, r);
            draw_star(surf, cx + half + 2 * r, cheer_cy, r);
            if award > 0 {
                print_tight_at(surf, font, &format!("+{award}"), 52.0, cx + half + 4 * r, cheer_cy, BLACK);
            }
        }
    }
    if !hint.is_empty() {
        let hint_px = 40.0;
        let lines = script::wrap(font, hint, hint_px, (W - 2 * MARGIN) as f32);
        let mut y = cheer_cy + (H * 5 / 100) as i32;
        for line in lines.iter().take(3) {
            print_centered(surf, font, line, hint_px, cx, y, BLACK);
            y += (hint_px * 1.3) as i32;
        }
    }
    feedback_region()
}

/// A five-point star, outlined in the brush hand.
fn draw_star(surf: &mut Surface, cx: i32, cy: i32, r: i32) {
    let mut pts = [(0i32, 0i32); 10];
    for (i, p) in pts.iter_mut().enumerate() {
        let ang = std::f32::consts::PI * (i as f32) / 5.0 - std::f32::consts::FRAC_PI_2;
        let rad = if i % 2 == 0 { r as f32 } else { r as f32 * 0.42 };
        *p = (cx + (ang.cos() * rad) as i32, cy + (ang.sin() * rad) as i32);
    }
    for i in 0..10 {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % 10];
        surf.brush_line(x0, y0, x1, y1, 4, BLACK);
    }
}

/// A gentle underline beneath the answer region: look here again.
pub fn draw_look_again(surf: &mut Surface, answer: &BBox) -> BBox {
    let y = answer.y1 + 18;
    surf.brush_line(answer.x0, y, answer.x1, y, 3, BLACK);
    let mut b = BBox::empty();
    b.add(answer.x0, y, 8);
    b.add(answer.x1, y, 8);
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::problems::{generate_set, Rng, Set, Topic};
    use crate::surface::PixFmt;

    fn page() -> (Vec<u8>, Surface) {
        let mut buf = vec![0xFFu8; W * H * 4];
        let ptr = buf.as_mut_ptr();
        let s = Surface::new(ptr, buf.len(), W, H, W * 4, PixFmt::Rgb32);
        (buf, s)
    }

    fn ui_font() -> FontRef<'static> {
        FontRef::try_from_slice(crate::ui::UI_FONT_TTF).unwrap()
    }

    /// A one-question set around `p`, as the full-page kinds are dealt.
    fn solo(p: crate::learn::problems::Problem) -> Set {
        Set { prompt: p.prompt, items: vec![p] }
    }

    fn dark_in(surf: &Surface, b: &BBox) -> usize {
        let mut n = 0;
        for y in b.y0..=b.y1 {
            for x in b.x0..=b.x1 {
                if surf.luma(x, y) < 128 {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn every_sheet_returns_disjoint_regions_inside_the_page() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let mut rng = Rng::new(42);
        for level in 1..=4 {
            for rot in 0..8 {
                let set = generate_set(level, Topic::Mix, rot, &mut rng);
                let map = draw(&mut surf, &font, &set, level, 2, 45);
                for b in [&map.answer, &map.done, &map.new, &map.menu] {
                    assert!(!b.is_empty(), "empty region for {:?}", set.items);
                    assert!(b.x0 >= 0 && b.y0 >= 0 && b.x1 < W as i32 && b.y1 < H as i32);
                }
                // One blank per question, the last one graded.
                assert_eq!(map.blanks.len(), set.items.len());
                let last = map.blanks.last().unwrap();
                assert_eq!((last.x0, last.y0, last.x1, last.y1),
                    (map.answer.x0, map.answer.y0, map.answer.x1, map.answer.y1));
                for (i, b) in map.blanks.iter().enumerate() {
                    assert!(!b.is_empty(), "blank {i} empty for {:?}", set.items[i].kind);
                    assert!(b.x0 >= 0 && b.y0 >= 0 && b.x1 < W as i32 && b.y1 < H as i32,
                        "blank {i} off-page for {:?}", set.items[i].kind);
                    // Blanks never overlap the decision boxes...
                    assert!(b.y1 < map.done.y0, "{:?}", set.items[i].kind);
                    // ...or each other: every question's box is its own.
                    for other in &map.blanks[i + 1..] {
                        let apart = b.x1 < other.x0 || other.x1 < b.x0
                            || b.y1 < other.y0 || other.y1 < b.y0;
                        assert!(apart, "blanks overlap for {:?}", set.items[i].kind);
                    }
                }
                assert!(map.done.x0 > map.menu.x1 && map.menu.x0 > map.new.x1);
            }
        }
    }

    #[test]
    fn multi_question_sheets_deal_two_to_four_and_paint_every_figure() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let mut rng = Rng::new(13);
        let mut saw_multi = false;
        for level in 1..=4 {
            for rot in 0..8 {
                let set = generate_set(level, Topic::Mix, rot, &mut rng);
                assert!((1..=4).contains(&set.items.len()));
                // A page is one activity, like a worksheet's row of sums.
                let d = std::mem::discriminant(&set.items[0].kind);
                assert!(set.items.iter().all(|p| std::mem::discriminant(&p.kind) == d));
                if set.items.len() > 1 {
                    saw_multi = true;
                    let map = draw(&mut surf, &font, &set, level, 0, 0);
                    for (i, b) in map.blanks.iter().enumerate() {
                        // Each band painted its figure: ink lands near each blank.
                        let mut around = BBox::empty();
                        around.add(b.x0 - 260, b.y0 - 60, 0);
                        around.add(b.x1 + 260, b.y1 + 60, 0);
                        let mut inside = *b;
                        inside.x0 += 12; inside.y0 += 12; inside.x1 -= 12; inside.y1 -= 12;
                        assert!(dark_in(&surf, &around) > dark_in(&surf, &inside) + 50,
                            "question {i} painted nothing around its blank: {:?}", set.items[i].kind);
                    }
                }
            }
        }
        assert!(saw_multi, "the mix must deal some multi-question pages");
    }

    #[test]
    fn hits_resolve_by_containment_and_boxes_win() {
        let map = HitMap {
            answer: {
                let mut b = BBox::empty();
                b.add(100, 100, 0);
                b.add(400, 400, 0);
                b
            },
            done: {
                let mut b = BBox::empty();
                b.add(900, 1700, 0);
                b.add(1200, 1830, 0);
                b
            },
            new: {
                let mut b = BBox::empty();
                b.add(100, 1700, 0);
                b.add(400, 1830, 0);
                b
            },
            menu: {
                let mut b = BBox::empty();
                b.add(550, 1700, 0);
                b.add(850, 1830, 0);
                b
            },
            choices: Vec::new(),
            blanks: Vec::new(),
        };
        assert_eq!(map.hit(250, 250), Some(Target::Answer));
        assert_eq!(map.hit(1000, 1750), Some(Target::Done));
        assert_eq!(map.hit(200, 1750), Some(Target::New));
        assert_eq!(map.hit(700, 1750), Some(Target::Menu));
        assert_eq!(map.hit(700, 900), None);
    }

    #[test]
    fn the_menu_offers_every_item_as_a_disjoint_box_and_nothing_else() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let map = draw_menu(&mut surf, &font, 2);
        // The named entries, then the four LEVEL boxes.
        assert_eq!(map.choices.len(), MENU_ITEMS.len() + MENU_LEVELS as usize);
        assert!(map.done.is_empty() && map.new.is_empty() && map.menu.is_empty() && map.answer.is_empty());
        for (i, b) in map.choices.iter().enumerate() {
            assert!(!b.is_empty());
            assert!(b.x0 >= 0 && b.y0 >= 0 && b.x1 < W as i32 && b.y1 < H as i32);
            assert!(dark_in(&surf, b) > 100, "menu box {i} must be visibly drawn");
            for other in &map.choices[i + 1..] {
                let apart = b.x1 < other.x0 || other.x1 < b.x0 || b.y1 < other.y0 || other.y1 < b.y0;
                assert!(apart, "menu boxes {i} must not overlap");
            }
        }
        // Every box resolves to its own choice index.
        for (i, b) in map.choices.iter().enumerate() {
            let (cx, cy) = ((b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2);
            assert_eq!(map.hit(cx, cy), Some(Target::Choice(i)));
        }
        // The current level's box is filled solid; the others are outlines.
        let current = &map.choices[MENU_ITEMS.len() + 1]; // level 2
        let other = &map.choices[MENU_ITEMS.len()]; // level 1
        assert!(
            dark_in(&surf, current) > dark_in(&surf, other) * 3,
            "the current level must read at a glance"
        );
    }

    #[test]
    fn the_skills_page_offers_every_math_skill_as_a_disjoint_box() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let map = draw_skills(&mut surf, &font);
        assert_eq!(map.choices.len(), crate::learn::problems::MATH_SKILLS.len());
        assert!(map.done.is_empty() && map.new.is_empty() && map.menu.is_empty() && map.answer.is_empty());
        for (i, b) in map.choices.iter().enumerate() {
            assert!(!b.is_empty());
            assert!(b.x0 >= 0 && b.y0 >= 0 && b.x1 < W as i32 && b.y1 < H as i32);
            assert!(dark_in(&surf, b) > 100, "skill box {i} must be visibly drawn");
            for other in &map.choices[i + 1..] {
                let apart = b.x1 < other.x0 || other.x1 < b.x0 || b.y1 < other.y0 || other.y1 < b.y0;
                assert!(apart, "skill boxes {i} must not overlap");
            }
            let (cx, cy) = ((b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2);
            assert_eq!(map.hit(cx, cy), Some(Target::Choice(i)));
        }
    }

    #[test]
    fn the_score_shows_in_the_header_once_it_exists() {
        let font = ui_font();
        let mut rng = Rng::new(17);
        let set = generate_set(2, Topic::Math, 0, &mut rng);
        let mut strip = BBox::empty();
        strip.add((W / 3) as i32, 20, 0);
        strip.add((W * 2 / 3) as i32, 90, 0);
        let (_a, mut quiet) = page();
        draw(&mut quiet, &font, &set, 2, 0, 0);
        assert_eq!(dark_in(&quiet, &strip), 0, "a scoreless header stays calm");
        let (_b, mut scored) = page();
        draw(&mut scored, &font, &set, 2, 0, 145);
        assert!(dark_in(&scored, &strip) > 200, "the star and points must be visible");
    }

    #[test]
    fn a_bond_sheet_paints_ink_and_leaves_the_blank_blank() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let p = crate::learn::problems::Problem {
            kind: Kind::Bond { whole: 7, parts: [3, 4], blank: Blank::Part(1) },
            prompt: "WRITE THE MISSING NUMBER",
        };
        let map = draw(&mut surf, &font, &solo(p), 2, 0, 0);
        // The page is not blank...
        let mut full = BBox::empty();
        full.add(0, 0, 0);
        full.add(W as i32 - 1, H as i32 - 1, 0);
        assert!(dark_in(&surf, &full) > 5000, "the sheet painted almost nothing");
        // ...but the inside of the blank circle is: the child writes there.
        let mut core = BBox::empty();
        core.add((map.answer.x0 + map.answer.x1) / 2, (map.answer.y0 + map.answer.y1) / 2, 40);
        assert_eq!(dark_in(&surf, &core), 0, "the answer blank must be empty paper");
    }

    #[test]
    fn a_trace_sheet_draws_the_guide_inside_the_answer_region() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let p = crate::learn::problems::Problem {
            kind: Kind::Trace { word: "cat" },
            prompt: "TRACE IT, THEN WRITE IT",
        };
        let map = draw(&mut surf, &font, &solo(p), 1, 0, 0);
        assert!(dark_in(&surf, &map.answer) > 400, "the dashed template must be visible");
    }

    #[test]
    fn marks_land_next_to_the_answer_region() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let p = crate::learn::problems::Problem {
            kind: Kind::Equation { a: 4, op: Op::Add, b: 3 },
            prompt: "WRITE THE ANSWER",
        };
        let map = draw(&mut surf, &font, &solo(p), 2, 0, 0);
        let before = dark_in(&surf, &map.answer);
        let check = draw_check(&mut surf, &map.answer);
        assert!(!check.is_empty());
        assert_eq!(dark_in(&surf, &map.answer), before, "the check must not cover the child's blank");
        let under = draw_look_again(&mut surf, &map.answer);
        assert!(!under.is_empty());
    }

    #[test]
    fn the_feedback_strip_sits_between_figure_and_boxes() {
        let f = feedback_region();
        assert!(f.y0 > (H / 2) as i32);
        assert!(f.y1 < box_top());
    }

    #[test]
    fn feedback_stamps_inside_the_strip_and_a_hint_adds_ink() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        surf.fill_rect(0, 0, W, H, WHITE);
        let region = draw_feedback(&mut surf, &font, "GREAT JOB!", "", true, 0);
        let cheer_only = dark_in(&surf, &region);
        assert!(cheer_only > 400, "the big cheer and its stars must land in the strip");
        // Everything stays inside the strip: no ink above it or in the boxes.
        let mut whole = BBox::empty();
        whole.add(0, 0, 0);
        whole.add(W as i32 - 1, H as i32 - 1, 0);
        assert_eq!(dark_in(&surf, &whole), cheer_only, "feedback must not spill out of its region");

        let (_b2, mut surf2) = page();
        surf2.fill_rect(0, 0, W, H, WHITE);
        let r2 = draw_feedback(&mut surf2, &font, "SO CLOSE!", "Your 3 is facing the wrong way.", false, 0);
        assert!(dark_in(&surf2, &r2) > cheer_only / 2, "the hint must add readable ink");

        // An award prints beside the stars, and stays inside the strip too.
        let (_b3, mut surf3) = page();
        surf3.fill_rect(0, 0, W, H, WHITE);
        let r3 = draw_feedback(&mut surf3, &font, "GREAT JOB!", "", true, 15);
        let with_award = dark_in(&surf3, &r3);
        assert!(with_award > cheer_only + 100, "the +N toast must add visible ink");
        let mut whole3 = BBox::empty();
        whole3.add(0, 0, 0);
        whole3.add(W as i32 - 1, H as i32 - 1, 0);
        assert_eq!(dark_in(&surf3, &whole3), with_award, "the toast must not spill out of the strip");
    }
}
