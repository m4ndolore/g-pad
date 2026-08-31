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

use super::problems::{Blank, Kind, Op, Problem};

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

fn box_top() -> i32 {
    (H - H * 8 / 100 - 40) as i32
}

/// What a mark can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Answer,
    Done,
    New,
}

/// The rendered regions of the current sheet.
#[derive(Clone, Copy, Debug)]
pub struct HitMap {
    pub answer: BBox,
    pub done: BBox,
    pub new: BBox,
}

impl HitMap {
    /// Which element a point (a stroke's centroid) landed on.
    pub fn hit(&self, x: i32, y: i32) -> Option<Target> {
        let inside = |b: &BBox| !b.is_empty() && x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1;
        if inside(&self.done) {
            Some(Target::Done)
        } else if inside(&self.new) {
            Some(Target::New)
        } else if inside(&self.answer) {
            Some(Target::Answer)
        } else {
            None
        }
    }
}

/// Draw the whole sheet for `problem` onto a white page and return its hit
/// map. `level` and `streak` feed the header; they are the child's only
/// visible progress chrome.
pub fn draw(surf: &mut Surface, ui_font: &FontRef, problem: &Problem, level: u8, streak: u32) -> HitMap {
    surf.fill_rect(0, 0, W, H, WHITE);

    // Header: what this page is, and the streak dots. Quiet, top corners.
    print(surf, ui_font, &format!("LEARN · LEVEL {level}"), 32.0, MARGIN, 40, BLACK);
    let dots = streak.min(5) as i32;
    for i in 0..dots {
        surf.stamp((W - MARGIN) as i32 - 20 - i * 44, 56, 11, BLACK);
    }

    // The instruction line, printed large enough for an early reader.
    print_centered(surf, ui_font, problem.prompt, 46.0, (W / 2) as i32, (H * 13 / 100) as i32, BLACK);

    let answer = match &problem.kind {
        Kind::Bond { parts, blank, whole } => draw_bond(surf, ui_font, *whole, *parts, *blank),
        Kind::TenFrame { shown, make_ten } => draw_ten_frame(surf, ui_font, *shown, *make_ten),
        Kind::Equation { a, op, b } => draw_equation(surf, ui_font, *a, *op, *b),
        Kind::Array { rows, cols } => draw_array(surf, ui_font, *rows, *cols),
        Kind::Share { total, groups } => draw_share(surf, ui_font, *total, *groups),
        Kind::Trace { word } => draw_trace(surf, ui_font, word),
    };

    let done = draw_action_box(surf, ui_font, false);
    let new = draw_action_box(surf, ui_font, true);
    HitMap { answer, done, new }
}

/// Repaint the two decision boxes: called after an absorbed mark's white-out,
/// which may have clipped a box edge or label.
pub fn refresh_boxes(surf: &mut Surface, ui_font: &FontRef) {
    let _ = draw_action_box(surf, ui_font, false);
    let _ = draw_action_box(surf, ui_font, true);
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

fn draw_equation(surf: &mut Surface, font: &FontRef, a: u32, op: Op, b: u32) -> BBox {
    let eq_y = (H * 38 / 100) as i32;
    equation_with_box(surf, font, &format!("{a} {} {b} = ", op.glyph()), "", eq_y)
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
/// The rules are sized FROM the font, not the other way round: the template
/// is scaled so a 'd' ascender spans the full writing height, and the dashed
/// midline is placed at the font's true x-height — so the guide letters
/// actually touch the lines the child is told to reach.
fn draw_trace(surf: &mut Surface, font: &FontRef, word: &str) -> BBox {
    let gap = (H * 55 / 1000) as i32; // half the writing height
    let base1 = (H * 34 / 100) as i32;
    let base2 = (H * 56 / 100) as i32;

    // Fit: ascender span → 2*gap, capped by the content width.
    let probe = 100.0f32;
    let asc_h = tight_text_height(font, "d", probe).max(1) as f32;
    let mut px = probe * (2 * gap) as f32 / asc_h;
    let max_w = (W - 2 * MARGIN) as f32;
    let w = script::measure(font, word, px);
    if w > max_w {
        px *= max_w / w;
    }
    let mid_off = tight_text_height(font, "x", px); // true x-height at px

    for &base in &[base1, base2] {
        rule_group(surf, base, 2 * gap, mid_off);
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

/// One handwriting rule group: ascender line at `top_h` above the heavy
/// baseline, dashed midline at `mid_off` above it (the letters' x-height).
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

/// `lead` + answer box + `tail`, tight-centered as one row. Returns the box's
/// interior as the answer region.
fn equation_with_box(surf: &mut Surface, font: &FontRef, lead: &str, tail: &str, cy: i32) -> BBox {
    const PX: f32 = 120.0;
    let (bw, bh) = box_size();
    let gap = 24i32;
    let lead_w = tight_width(font, lead, PX);
    let tail_w = tight_width(font, tail, PX);
    let total = lead_w + gap + bw + if tail_w > 0 { gap + tail_w } else { 0 };
    let mut x = (W as i32 - total) / 2;
    if !lead.trim().is_empty() {
        print_tight_at(surf, font, lead, PX, x, cy, BLACK);
    }
    x += lead_w + gap;
    let b = answer_box(surf, x, cy - bh / 2, bw, bh);
    x += bw + gap;
    if !tail.trim().is_empty() {
        print_tight_at(surf, font, tail, PX, x, cy, BLACK);
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

/// DONE (right) and NEW (left) at the foot of every sheet: the two anchored
/// decision boxes. Any mark inside one is a command — form never matters.
fn draw_action_box(surf: &mut Surface, font: &FontRef, is_new: bool) -> BBox {
    let bw = (W * 22 / 100) as i32;
    let bh = (H * 8 / 100) as i32;
    let y = box_top();
    let x = if is_new { MARGIN as i32 } else { (W - MARGIN) as i32 - bw };
    rect_outline(surf, x, y, bw, bh, 4, BLACK);
    let label = if is_new { "NEW" } else { "DONE" };
    print_tight_centered(surf, font, label, 42.0, x + bw / 2, y + bh / 2, BLACK);
    let mut b = BBox::empty();
    b.add(x, y, 0);
    b.add(x + bw, y + bh, 0);
    b
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
    use crate::learn::problems::{generate, Rng};
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
            for rot in 0..6 {
                let p = generate(level, rot, &mut rng);
                let map = draw(&mut surf, &font, &p, level, 2);
                for b in [&map.answer, &map.done, &map.new] {
                    assert!(!b.is_empty(), "empty region for {:?}", p.kind);
                    assert!(b.x0 >= 0 && b.y0 >= 0 && b.x1 < W as i32 && b.y1 < H as i32);
                }
                // The answer region never overlaps a decision box: a mark must
                // be unambiguous.
                assert!(map.answer.y1 < map.done.y0, "{:?}", p.kind);
                assert!(map.answer.y1 < map.new.y0, "{:?}", p.kind);
                assert!(map.done.x0 > map.new.x1);
            }
        }
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
        };
        assert_eq!(map.hit(250, 250), Some(Target::Answer));
        assert_eq!(map.hit(1000, 1750), Some(Target::Done));
        assert_eq!(map.hit(200, 1750), Some(Target::New));
        assert_eq!(map.hit(700, 900), None);
    }

    #[test]
    fn a_bond_sheet_paints_ink_and_leaves_the_blank_blank() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let p = crate::learn::problems::Problem {
            kind: Kind::Bond { whole: 7, parts: [3, 4], blank: Blank::Part(1) },
            prompt: "WRITE THE MISSING NUMBER",
        };
        let map = draw(&mut surf, &font, &p, 2, 0);
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
        let map = draw(&mut surf, &font, &p, 1, 0);
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
        let map = draw(&mut surf, &font, &p, 2, 0);
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
}
