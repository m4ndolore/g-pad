//! The SELECT tip: the pen draws a lasso around finished strokes; a press
//! inside the selection drags it, and the DELETE chip above it removes it.
//!
//! Everything this module shows on the page is temporary and must come off
//! without a trace: the lasso's dots and the selection's dashed outline are
//! drawn by inverting pixels, so inverting them again restores the page
//! exactly, and the chip keeps the pixels it covers. Nothing here outlives
//! the tip: `drop` takes it all down, and the caller drops before anything
//! else paints over the page (overlays, page turns, a commit).

use ab_glyph::FontRef;

use crate::fb::{BBox, SCREEN_H, SCREEN_W};
use crate::ink::Ink;
use crate::surface::{Surface, BLACK, WHITE};

/// Lasso dots land at least this far apart.
const DOT_STEP: i32 = 8;
/// The outline stands this far off the strokes.
const PAD: i32 = 14;
const DASH: i32 = 16;
const DASH_GAP: i32 = 10;
const DASH_T: i32 = 3;
/// A drag shorter than this is a tap: nothing moves.
const MOVE_MIN: i32 = 5;
/// The drag preview moves in steps this big, to spare the panel.
const DRAG_STEP: i32 = 6;
const CHIP_W: usize = 170;
const CHIP_H: usize = 58;
/// Nothing temporary goes above the corner row.
const TOP: i32 = crate::ui::CORNER as i32 + 4;

#[derive(Default)]
pub struct Selector {
    lasso: Option<Lasso>,
    picked: Option<Picked>,
    /// The pen is down on a press this module owns.
    down: bool,
}

struct Lasso {
    path: Vec<(i32, i32)>,
    dots: Vec<(i32, i32)>,
}

struct Picked {
    strokes: Vec<usize>,
    /// The strokes' painted extent.
    bounds: BBox,
    /// Where the outline is showing, relative to `bounds`.
    shown: (i32, i32),
    chip: Option<Chip>,
    /// Pen-down point of a drag in progress.
    grab: Option<(i32, i32)>,
}

struct Chip {
    x: usize,
    y: usize,
    under: Vec<u8>,
}

impl Selector {
    /// True while anything of the selection is on the page.
    pub fn active(&self) -> bool {
        self.lasso.is_some() || self.picked.is_some()
    }

    /// One writing sample of the select tip. Returns the dirty rect.
    pub fn pen(&mut self, surf: &mut Surface, ink: &mut Ink, x: i32, y: i32) -> BBox {
        if !self.down {
            self.down = true;
            return self.press(surf, ink, x, y);
        }
        if let Some(lasso) = self.lasso.as_mut() {
            return lasso.extend(surf, x, y);
        }
        match self.picked.as_mut() {
            Some(p) if p.grab.is_some() => p.drag_to(surf, x, y),
            _ => BBox::empty(),
        }
    }

    /// The pen is down on a press this module owns.
    pub fn pressing(&self) -> bool {
        self.down
    }

    fn press(&mut self, surf: &mut Surface, ink: &mut Ink, x: i32, y: i32) -> BBox {
        if let Some(p) = self.picked.as_mut() {
            if p.chip.as_ref().is_some_and(|c| c.hit(x, y)) {
                let mut dirty = p.hide(surf);
                let gone = ink.remove(surf, &p.strokes);
                union(&mut dirty, &gone);
                self.picked = None;
                return dirty;
            }
            if p.grabbable(x, y) {
                let dirty = p.hide_chip(surf);
                p.grab = Some((x, y));
                return dirty;
            }
        }
        let mut dirty = self.drop(surf);
        let (lasso, d) = Lasso::start(surf, x, y);
        union(&mut dirty, &d);
        self.lasso = Some(lasso);
        dirty
    }

    /// The pen lifted. A lasso becomes a selection (or nothing, if it took
    /// no strokes); a drag puts its strokes down.
    pub fn pen_up(&mut self, surf: &mut Surface, ink: &mut Ink, font: &FontRef) -> BBox {
        self.down = false;
        if let Some(lasso) = self.lasso.take() {
            let (path, mut dirty) = lasso.finish(surf);
            let strokes = ink.strokes_in(&path);
            if !strokes.is_empty() {
                let (picked, d) = Picked::show(surf, font, strokes, ink);
                union(&mut dirty, &d);
                self.picked = Some(picked);
            }
            return dirty;
        }
        if let Some(mut p) = self.picked.take() {
            if p.grab.take().is_some() {
                let (dx, dy) = p.shown;
                let mut dirty = p.hide(surf);
                if dx.abs() >= MOVE_MIN || dy.abs() >= MOVE_MIN {
                    let d = ink.shift(surf, &p.strokes, dx, dy);
                    union(&mut dirty, &d);
                }
                let (picked, d) = Picked::show(surf, font, p.strokes, ink);
                union(&mut dirty, &d);
                self.picked = Some(picked);
                return dirty;
            }
            self.picked = Some(p);
        }
        BBox::empty()
    }

    /// Take everything down. Safe to call when nothing is up. A press in
    /// progress stays owned here, so the rest of it is not ink.
    pub fn drop(&mut self, surf: &mut Surface) -> BBox {
        let mut dirty = BBox::empty();
        if let Some(lasso) = self.lasso.take() {
            dirty = lasso.finish(surf).1;
        }
        if let Some(mut p) = self.picked.take() {
            let d = p.hide(surf);
            union(&mut dirty, &d);
        }
        dirty
    }
}

impl Lasso {
    fn start(surf: &mut Surface, x: i32, y: i32) -> (Self, BBox) {
        let mut l = Lasso { path: Vec::new(), dots: Vec::new() };
        let d = l.extend(surf, x, y);
        (l, d)
    }

    fn extend(&mut self, surf: &mut Surface, x: i32, y: i32) -> BBox {
        self.path.push((x, y));
        let far = self.dots.last().is_none_or(|&(lx, ly)| (x - lx).abs().max((y - ly).abs()) >= DOT_STEP);
        if !far || y < TOP {
            return BBox::empty();
        }
        self.dots.push((x, y));
        invert_dot(surf, x, y)
    }

    /// Take the dots off the page and hand back the path.
    fn finish(self, surf: &mut Surface) -> (Vec<(i32, i32)>, BBox) {
        let mut dirty = BBox::empty();
        for &(x, y) in self.dots.iter().rev() {
            let d = invert_dot(surf, x, y);
            union(&mut dirty, &d);
        }
        (self.path, dirty)
    }
}

impl Picked {
    fn show(surf: &mut Surface, font: &FontRef, strokes: Vec<usize>, ink: &Ink) -> (Self, BBox) {
        let bounds = ink.bounds(&strokes);
        let mut p = Picked { strokes, bounds, shown: (0, 0), chip: None, grab: None };
        let mut dirty = outline(surf, p.frame());
        let chip = Chip::show(surf, font, p.frame());
        union(&mut dirty, &chip.region());
        p.chip = Some(chip);
        (p, dirty)
    }

    /// The outline's rectangle at its shown offset, kept on the page.
    fn frame(&self) -> BBox {
        let (dx, dy) = self.shown;
        BBox {
            x0: (self.bounds.x0 - PAD + dx).max(0),
            y0: (self.bounds.y0 - PAD + dy).max(TOP),
            x1: (self.bounds.x1 + PAD + dx).min(SCREEN_W as i32 - 1),
            y1: (self.bounds.y1 + PAD + dy).min(SCREEN_H as i32 - 1),
        }
    }

    fn grabbable(&self, x: i32, y: i32) -> bool {
        let f = self.frame();
        x >= f.x0 && x <= f.x1 && y >= f.y0 && y <= f.y1
    }

    /// Move the outline with the pen, clamped so the strokes stay on the
    /// page.
    fn drag_to(&mut self, surf: &mut Surface, x: i32, y: i32) -> BBox {
        let Some((gx, gy)) = self.grab else { return BBox::empty() };
        let b = self.bounds;
        let dx = (x - gx).clamp(-b.x0, SCREEN_W as i32 - 1 - b.x1);
        let dy = (y - gy).clamp(TOP - b.y0, SCREEN_H as i32 - 1 - b.y1);
        if (dx - self.shown.0).abs() < DRAG_STEP && (dy - self.shown.1).abs() < DRAG_STEP {
            return BBox::empty();
        }
        let mut dirty = outline(surf, self.frame());
        self.shown = (dx, dy);
        let d = outline(surf, self.frame());
        union(&mut dirty, &d);
        dirty
    }

    fn hide_chip(&mut self, surf: &mut Surface) -> BBox {
        match self.chip.take() {
            Some(c) => c.hide(surf),
            None => BBox::empty(),
        }
    }

    fn hide(&mut self, surf: &mut Surface) -> BBox {
        let mut dirty = self.hide_chip(surf);
        let d = outline(surf, self.frame());
        union(&mut dirty, &d);
        dirty
    }
}

impl Chip {
    /// DELETE, just above the outline's right end, or just below it when
    /// the selection sits too near the top.
    fn show(surf: &mut Surface, font: &FontRef, frame: BBox) -> Self {
        let (w, h) = (CHIP_W as i32, CHIP_H as i32);
        let x = (frame.x1 - w).clamp(0, SCREEN_W as i32 - w);
        let above = frame.y0 - 10 - h;
        let y = if above >= TOP { above } else { (frame.y1 + 10).min(SCREEN_H as i32 - h) };
        let (x, y) = (x as usize, y as usize);
        let under = surf.copy_rect(x, y, CHIP_W, CHIP_H);
        surf.fill_rect(x, y, CHIP_W, CHIP_H, WHITE);
        for (rx, ry, rw, rh) in [(x, y, CHIP_W, 3), (x, y + CHIP_H - 3, CHIP_W, 3), (x, y, 3, CHIP_H),
            (x + CHIP_W - 3, y, 3, CHIP_H)]
        {
            surf.fill_rect(rx, ry, rw, rh, BLACK);
        }
        crate::ui::full_text(surf, font, "DELETE", 28.0, x + 26, y + 14, BLACK);
        Chip { x, y, under }
    }

    fn region(&self) -> BBox {
        BBox { x0: self.x as i32, y0: self.y as i32, x1: (self.x + CHIP_W) as i32 - 1, y1: (self.y + CHIP_H) as i32 - 1 }
    }

    fn hit(&self, x: i32, y: i32) -> bool {
        let r = self.region();
        x >= r.x0 && x <= r.x1 && y >= r.y0 && y <= r.y1
    }

    fn hide(self, surf: &mut Surface) -> BBox {
        surf.paste_rect(self.x, self.y, CHIP_W, CHIP_H, &self.under);
        self.region()
    }
}

/// Flip a 3×3 dot. Flipping it again puts the page back.
fn invert_dot(surf: &mut Surface, x: i32, y: i32) -> BBox {
    let (x0, y0) = ((x - 1).max(0), (y - 1).max(0));
    surf.invert_rect(x0 as usize, y0 as usize, 3, 3);
    BBox { x0, y0, x1: x0 + 2, y1: y0 + 2 }
}

/// Flip a dashed rectangle along `f`'s edges. The sides never share a
/// pixel, so a second call restores every pixel the first one changed.
fn outline(surf: &mut Surface, f: BBox) -> BBox {
    if f.is_empty() {
        return f;
    }
    let mut x = f.x0;
    while x <= f.x1 {
        let w = DASH.min(f.x1 - x + 1) as usize;
        surf.invert_rect(x as usize, f.y0 as usize, w, DASH_T as usize);
        surf.invert_rect(x as usize, (f.y1 - DASH_T + 1).max(f.y0 + DASH_T) as usize, w, DASH_T as usize);
        x += DASH + DASH_GAP;
    }
    let mut y = f.y0 + DASH_T;
    while y <= f.y1 - DASH_T {
        let h = DASH.min(f.y1 - DASH_T - y + 1) as usize;
        surf.invert_rect(f.x0 as usize, y as usize, DASH_T as usize, h);
        surf.invert_rect((f.x1 - DASH_T + 1).max(f.x0 + DASH_T) as usize, y as usize, DASH_T as usize, h);
        y += DASH + DASH_GAP;
    }
    f
}

fn union(into: &mut BBox, other: &BBox) {
    if !other.is_empty() {
        into.add(other.x0, other.y0, 0);
        into.add(other.x1, other.y1, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::PixFmt;

    fn page() -> (Vec<u8>, Surface) {
        let mut buf = vec![0xFFu8; SCREEN_W * SCREEN_H * 4];
        let ptr = buf.as_mut_ptr();
        let s = Surface::new(ptr, buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        (buf, s)
    }

    fn font() -> FontRef<'static> {
        FontRef::try_from_slice(crate::ui::UI_FONT_TTF).unwrap()
    }

    fn word(s: &mut Surface, ink: &mut Ink, y: i32) {
        for x in (400..=600).step_by(5) {
            ink.pen_point(s, x, y, 3);
        }
        ink.pen_up();
    }

    fn snapshot(s: &Surface) -> Vec<u8> {
        s.copy_rect(0, 0, SCREEN_W, SCREEN_H)
    }

    fn lasso(sel: &mut Selector, s: &mut Surface, ink: &mut Ink, f: &FontRef, pts: &[(i32, i32)]) {
        for &(x, y) in pts {
            sel.pen(s, ink, x, y);
        }
        sel.pen_up(s, ink, f);
    }

    fn ring(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for x in (x0..=x1).step_by(4) { v.push((x, y0)); }
        for y in (y0..=y1).step_by(4) { v.push((x1, y)); }
        for x in (x0..=x1).rev().step_by(4) { v.push((x, y1)); }
        for y in (y0..=y1).rev().step_by(4) { v.push((x0, y)); }
        v
    }

    #[test]
    fn a_lasso_and_a_drop_leave_the_page_exactly_as_it_was() {
        let (_buf, mut s) = page();
        let f = font();
        let mut ink = Ink::new();
        word(&mut s, &mut ink, 500);
        let before = snapshot(&s);
        let mut sel = Selector::default();
        lasso(&mut sel, &mut s, &mut ink, &f, &ring(350, 450, 650, 550));
        assert!(sel.active(), "the lasso took the word");
        assert_ne!(snapshot(&s), before, "the selection shows");
        sel.drop(&mut s);
        assert!(!sel.active());
        assert_eq!(snapshot(&s), before, "dropping must restore every pixel");
    }

    #[test]
    fn an_empty_lasso_selects_nothing_and_leaves_no_trace() {
        let (_buf, mut s) = page();
        let f = font();
        let mut ink = Ink::new();
        word(&mut s, &mut ink, 500);
        let before = snapshot(&s);
        let mut sel = Selector::default();
        lasso(&mut sel, &mut s, &mut ink, &f, &ring(900, 1200, 1100, 1400));
        assert!(!sel.active());
        assert_eq!(snapshot(&s), before);
    }

    #[test]
    fn dragging_inside_the_selection_moves_the_strokes() {
        let (_buf, mut s) = page();
        let f = font();
        let mut ink = Ink::new();
        word(&mut s, &mut ink, 500);
        word(&mut s, &mut ink, 900);
        let mut sel = Selector::default();
        lasso(&mut sel, &mut s, &mut ink, &f, &ring(350, 450, 650, 550));
        for step in 0..=20 {
            sel.pen(&mut s, &mut ink, 500, 500 + step * 10);
        }
        sel.pen_up(&mut s, &mut ink, &f);
        let b = ink.bounds(&[0]);
        assert!(b.y0 > 680 && b.y1 < 720, "the word moved down 200: {b:?}");
        sel.drop(&mut s);
        for x in 400..=600 {
            assert_eq!(s.luma(x, 500), 255, "the old place is paper at x={x}");
        }
        assert!(s.luma(500, 700) < 128, "the word is at its new place");
        assert!(s.luma(500, 900) < 128, "the other word is untouched");
    }

    #[test]
    fn the_delete_chip_removes_the_selection() {
        let (_buf, mut s) = page();
        let f = font();
        let mut ink = Ink::new();
        word(&mut s, &mut ink, 500);
        word(&mut s, &mut ink, 900);
        let mut sel = Selector::default();
        lasso(&mut sel, &mut s, &mut ink, &f, &ring(350, 450, 650, 550));
        let chip = sel.picked.as_ref().and_then(|p| p.chip.as_ref()).map(Chip::region).unwrap();
        sel.pen(&mut s, &mut ink, (chip.x0 + chip.x1) / 2, (chip.y0 + chip.y1) / 2);
        sel.pen_up(&mut s, &mut ink, &f);
        assert!(!sel.active());
        assert_eq!(ink.stroke_list().len(), 1);
        for y in 80..SCREEN_H as i32 {
            for x in 0..SCREEN_W as i32 {
                if s.luma(x, y) < 255 {
                    assert!((880..=920).contains(&y), "a stray pixel at ({x}, {y}) after delete");
                }
            }
        }
    }
}
