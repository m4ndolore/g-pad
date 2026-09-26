//! User ink: capture pen strokes, render them, dissolve them, rasterize them
//! for the oracle.

use crate::fb::BBox;
use crate::surface::{Surface, BLACK, WHITE};
use crate::tools::{Brush, PenKind};

pub struct Ink {
    /// Finished strokes as point lists (x, y, radius).
    strokes: Vec<Vec<(i32, i32, i32)>>,
    /// The brush each finished stroke was drawn with, index for index.
    brushes: Vec<Brush>,
    current: Vec<(i32, i32, i32)>,
    current_brush: Brush,
    last_erase: Option<(i32, i32)>,
    pub bbox: BBox,
}

impl Ink {
    pub fn new() -> Self {
        Self {
            strokes: Vec::new(),
            brushes: Vec::new(),
            current: Vec::new(),
            current_brush: Brush::default(),
            last_erase: None,
            bbox: BBox::empty(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.current.is_empty()
    }

    /// Finished strokes (the current in-flight stroke is not included).
    pub fn stroke_list(&self) -> &[Vec<(i32, i32, i32)>] {
        &self.strokes
    }

    pub fn clear(&mut self) {
        self.strokes.clear();
        self.brushes.clear();
        self.current.clear();
        self.last_erase = None;
        self.bbox = BBox::empty();
    }

    /// Remove the most recent finished stroke (an absorbed gesture) and return
    /// its bounding box. The ink bbox is rebuilt from what remains.
    pub fn pop_stroke(&mut self) -> Option<BBox> {
        let s = self.strokes.pop()?;
        self.brushes.pop();
        let gone = stroke_bounds(&s);
        self.rebuild_bbox();
        Some(gone)
    }

    fn rebuild_bbox(&mut self) {
        self.bbox = BBox::empty();
        for st in self.strokes.iter().chain(std::iter::once(&self.current)) {
            for &(x, y, r) in st {
                self.bbox.add(x, y, r + 2);
            }
        }
    }

    /// Pen touched down or moved while down, with brush radius already
    /// resolved by the caller: the default ballpoint. Returns the dirty rect
    /// of what was drawn.
    pub fn pen_point(&mut self, surf: &mut Surface, x: i32, y: i32, r: i32) -> BBox {
        self.pen_point_with(surf, x, y, r, Brush::default())
    }

    /// `pen_point` with a chosen brush. The brush is taken at the stroke's
    /// first point and holds until pen-up.
    pub fn pen_point_with(&mut self, surf: &mut Surface, x: i32, y: i32, r: i32, brush: Brush) -> BBox {
        if self.current.is_empty() {
            self.current_brush = brush;
        }
        let mut dirty = BBox::empty();
        let prev = self.current.last().copied();
        self.current_brush.segment(surf, prev, (x, y, r));
        if let Some((px, py, pr)) = prev {
            dirty.add(px, py, pr + 2);
        }
        dirty.add(x, y, r + 2);
        self.current.push((x, y, r));
        self.bbox.add(x, y, r + 2);
        dirty
    }

    /// Eraser tip: brush white over the page AND drop the stored points it
    /// covers, so the stroke model stays true to the visible page. Without
    /// this, erased ink would still be remembered and re-conjured, and an
    /// erased "?" would still summon the guide.
    pub fn erase_point(&mut self, surf: &mut Surface, x: i32, y: i32, r: i32) -> BBox {
        let mut dirty = BBox::empty();
        if let Some((px, py)) = self.last_erase {
            surf.brush_line(px, py, x, y, r, WHITE);
            dirty.add(px, py, r + 2);
        } else {
            surf.stamp(x, y, r, WHITE);
        }
        dirty.add(x, y, r + 2);
        self.forget_near(x, y, r);
        self.last_erase = Some((x, y));
        dirty
    }

    /// Remove committed stroke points within `r` of (x, y); split strokes that
    /// are erased through the middle, and recompute the ink bbox.
    fn forget_near(&mut self, x: i32, y: i32, r: i32) {
        let r2 = (r + 2) * (r + 2);
        let mut kept: Vec<Vec<(i32, i32, i32)>> = Vec::new();
        let mut kept_brushes: Vec<Brush> = Vec::new();
        for (stroke, brush) in self.strokes.drain(..).zip(self.brushes.drain(..)) {
            let mut seg: Vec<(i32, i32, i32)> = Vec::new();
            for p in stroke {
                let (dx, dy) = (p.0 - x, p.1 - y);
                if dx * dx + dy * dy <= r2 {
                    if !seg.is_empty() {
                        kept.push(std::mem::take(&mut seg));
                        kept_brushes.push(brush);
                    }
                } else {
                    seg.push(p);
                }
            }
            if !seg.is_empty() {
                kept.push(seg);
                kept_brushes.push(brush);
            }
        }
        self.strokes = kept;
        self.brushes = kept_brushes;
        self.rebuild_bbox();
    }

    pub fn pen_up(&mut self) {
        if !self.current.is_empty() {
            self.strokes.push(std::mem::take(&mut self.current));
            self.brushes.push(self.current_brush);
        }
        self.last_erase = None;
    }

    /// The finished strokes a lasso encloses: those with at least half their
    /// points inside the polygon, so a loop drawn a little tight still takes
    /// a word whose tails poke out.
    pub fn strokes_in(&self, lasso: &[(i32, i32)]) -> Vec<usize> {
        if lasso.len() < 3 {
            return Vec::new();
        }
        self.strokes
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                let inside = s.iter().filter(|&&(x, y, _)| point_in_polygon(x, y, lasso)).count();
                !s.is_empty() && inside * 2 >= s.len()
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The painted extent of strokes `idx`.
    pub fn bounds(&self, idx: &[usize]) -> BBox {
        let mut b = BBox::empty();
        for s in idx.iter().filter_map(|&i| self.strokes.get(i)) {
            let sb = stroke_bounds(s);
            if !sb.is_empty() {
                b.add(sb.x0, sb.y0, 0);
                b.add(sb.x1, sb.y1, 0);
            }
        }
        b
    }

    /// Lift strokes `idx` off the page and put them down `(dx, dy)` away.
    /// The lifted ground is whited out, and every stroke touching either
    /// place is repainted in order, so ink the move uncovered comes back.
    /// Returns the dirty rect.
    pub fn shift(&mut self, surf: &mut Surface, idx: &[usize], dx: i32, dy: i32) -> BBox {
        let mut dirty = self.lift(surf, idx);
        for &i in idx {
            if let Some(s) = self.strokes.get_mut(i) {
                for p in s.iter_mut() {
                    p.0 += dx;
                    p.1 += dy;
                }
            }
        }
        let moved = self.bounds(idx);
        if !moved.is_empty() {
            dirty.add(moved.x0, moved.y0, 0);
            dirty.add(moved.x1, moved.y1, 0);
        }
        self.repaint_within(surf, &dirty);
        self.rebuild_bbox();
        dirty
    }

    /// Take strokes `idx` off the page and out of the model. Returns the
    /// dirty rect.
    pub fn remove(&mut self, surf: &mut Surface, idx: &[usize]) -> BBox {
        let dirty = self.lift(surf, idx);
        let mut sorted = idx.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        for &i in sorted.iter().rev() {
            if i < self.strokes.len() {
                self.strokes.remove(i);
                self.brushes.remove(i);
            }
        }
        self.repaint_within(surf, &dirty);
        self.rebuild_bbox();
        dirty
    }

    /// White out strokes `idx` where they lie; returns the area whitened.
    fn lift(&self, surf: &mut Surface, idx: &[usize]) -> BBox {
        let mut gone = BBox::empty();
        for s in idx.iter().filter_map(|&i| self.strokes.get(i)) {
            let mut prev: Option<(i32, i32, i32)> = None;
            for &(x, y, r) in s {
                match prev {
                    Some((px, py, pr)) => surf.brush_line(px, py, x, y, r.max(pr) + 1, WHITE),
                    None => surf.stamp(x, y, r + 1, WHITE),
                }
                gone.add(x, y, r + 3);
                prev = Some((x, y, r));
            }
        }
        gone
    }

    /// Repaint, in order, every finished stroke that reaches into `region`.
    fn repaint_within(&self, surf: &mut Surface, region: &BBox) {
        if region.is_empty() {
            return;
        }
        for (s, brush) in self.strokes.iter().zip(&self.brushes) {
            let b = stroke_bounds(s);
            let touches = !b.is_empty()
                && b.x0 <= region.x1 && b.x1 >= region.x0 && b.y0 <= region.y1 && b.y1 >= region.y0;
            if !touches {
                continue;
            }
            let mut prev = None;
            for &p in s {
                brush.segment(surf, prev, p);
                prev = Some(p);
            }
        }
    }

    /// True if any finished stroke has a point inside `region` — the "did the
    /// child actually write in the blank" test, immune to printed page ink.
    pub fn has_ink_in(&self, region: &BBox) -> bool {
        !region.is_empty()
            && self.strokes.iter().flatten().any(|&(x, y, _)| {
                x >= region.x0 && x <= region.x1 && y >= region.y0 && y <= region.y1
            })
    }

    /// Centroid of the most recent finished stroke: where an anchored mark
    /// landed. `None` when there is no finished stroke.
    pub fn last_stroke_centroid(&self) -> Option<(i32, i32)> {
        let s = self.strokes.last()?;
        if s.is_empty() {
            return None;
        }
        let n = s.len() as i64;
        let (sx, sy) = s.iter().fold((0i64, 0i64), |(ax, ay), &(x, y, _)| (ax + x as i64, ay + y as i64));
        Some(((sx / n) as i32, (sy / n) as i32))
    }

    /// True when the most recent finished stroke is a deliberate tap: a dot,
    /// not writing. The span test (not point count) is what matters — the pen
    /// samples fast, so even a quick tap can report a dozen points, but they
    /// all land within a few pixels of each other.
    pub fn last_stroke_is_tap(&self) -> bool {
        let Some(s) = self.strokes.last() else { return false };
        let mut b = BBox::empty();
        for &(x, y, _) in s {
            b.add(x, y, 0);
        }
        !b.is_empty() && (b.x1 - b.x0) <= 14 && (b.y1 - b.y0) <= 14
    }

    /// True when the most recent finished stroke sits at least `margin`
    /// pixels away from every other stroke on the page (trivially true when
    /// it is the only one). A tap this far from the child's writing is a
    /// deliberate command, not a dot on an 'i'.
    pub fn last_stroke_clear_of_rest(&self, margin: i32) -> bool {
        let Some((cx, cy)) = self.last_stroke_centroid() else { return false };
        let mut rest = BBox::empty();
        for st in &self.strokes[..self.strokes.len() - 1] {
            for &(x, y, r) in st {
                rest.add(x, y, r);
            }
        }
        rest.is_empty()
            || cx < rest.x0 - margin
            || cx > rest.x1 + margin
            || cy < rest.y0 - margin
            || cy > rest.y1 + margin
    }

    /// The region the oracle should see for an answer blank: the declared
    /// region grown to hold every finished stroke that comes within `margin`
    /// of it. A child's digit rarely respects the printed box — a stroke
    /// leaning over its edge must never be cropped mid-glyph — while marks
    /// far away (the DONE tick, stray doodles) stay excluded.
    pub fn crop_for(&self, region: &BBox, margin: i32) -> BBox {
        let mut out = *region;
        if region.is_empty() {
            return out;
        }
        for stroke in &self.strokes {
            let mut b = BBox::empty();
            for &(x, y, r) in stroke {
                b.add(x, y, r + 2);
            }
            if !b.is_empty()
                && b.x0 <= region.x1 + margin
                && b.x1 >= region.x0 - margin
                && b.y0 <= region.y1 + margin
                && b.y1 >= region.y0 - margin
            {
                out.add(b.x0, b.y0, 0);
                out.add(b.x1, b.y1, 0);
            }
        }
        out
    }

    /// Rasterize the ink region to a grayscale PNG for the oracle.
    /// Crops to the ink bounding box and box-downscales so the long side stays
    /// ≤ 800px (at least 2x): the model reads handwriting fine at that scale,
    /// and image pixels are the dominant vision-token / latency cost.
    pub fn to_png(&self, surf: &Surface, path: &str) -> std::io::Result<()> {
        region_png(surf, self.bbox, path).map(|_| ())
    }

    /// The child's pen work near an answer blank, alone on white paper: every
    /// stroke within `margin` of `region` is redrawn on a clean canvas and
    /// written as the oracle's PNG. The printed sheet — box borders, bond
    /// rings, dashed guides — never reaches the tutor, so a digit tangled
    /// with the furniture is still read as just a digit.
    pub fn ink_png(&self, region: &BBox, margin: i32, path: &str) -> std::io::Result<()> {
        let crop = self.crop_for(region, margin);
        if crop.is_empty() {
            return Err(std::io::Error::other("no ink"));
        }
        let (w, h) = ((crop.x1 - crop.x0 + 1) as usize + 40, (crop.y1 - crop.y0 + 1) as usize + 40);
        let mut buf = vec![0xFFu8; w * h * 4];
        let ptr = buf.as_mut_ptr();
        let mut tmp = Surface::new(ptr, buf.len(), w, h, w * 4, crate::surface::PixFmt::Rgb32);
        let (ox, oy) = (crop.x0 - 20, crop.y0 - 20);
        // A highlighter band is emphasis, not writing: the tutor reads the
        // strokes alone, and a band would read as a thick black bar.
        for (stroke, _) in self.strokes.iter().zip(&self.brushes)
            .filter(|(_, b)| b.kind != PenKind::Highlighter)
        {
            let mut b = BBox::empty();
            for &(x, y, r) in stroke {
                b.add(x, y, r + 2);
            }
            let near = !b.is_empty()
                && b.x0 <= region.x1 + margin
                && b.x1 >= region.x0 - margin
                && b.y0 <= region.y1 + margin
                && b.y1 >= region.y0 - margin;
            if !near {
                continue;
            }
            let mut prev: Option<(i32, i32, i32)> = None;
            for &(x, y, r) in stroke {
                let (tx, ty) = (x - ox, y - oy);
                match prev {
                    Some((px, py, pr)) => tmp.brush_line(px, py, tx, ty, r.min(pr + 1), BLACK),
                    None => tmp.stamp(tx, ty, r, BLACK),
                }
                prev = Some((tx, ty, r));
            }
        }
        region_png(&tmp, BBox { x0: 20, y0: 20, x1: w as i32 - 21, y1: h as i32 - 21 }, path).map(|_| ())
    }
}

fn stroke_bounds(s: &[(i32, i32, i32)]) -> BBox {
    let mut b = BBox::empty();
    for &(x, y, r) in s {
        b.add(x, y, r + 2);
    }
    b
}

/// Even-odd ray cast: is (x, y) inside the closed polygon `poly`?
fn point_in_polygon(x: i32, y: i32, poly: &[(i32, i32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let ((xi, yi), (xj, yj)) = (poly[i], poly[j]);
        if (yi > y) != (yj > y) {
            let cross = xi as i64 + (xj - xi) as i64 * (y - yi) as i64 / (yj - yi) as i64;
            if (x as i64) < cross {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Rasterize any page region to the oracle's grayscale PNG. Learn mode sends
/// only the region the sheet declared as the answer, never the whole page.
/// Returns the page rectangle the image actually covers (crop plus margin,
/// clamped) — the frame a model's picture coordinates refer to.
pub fn region_png(surf: &Surface, region: BBox, path: &str) -> std::io::Result<BBox> {
    {
        if region.is_empty() {
            return Err(std::io::Error::other("no ink"));
        }
        let (bx, by, bw, bh) = region.rect();
        let x0 = (bx - 20).max(0) as usize;
        let y0 = (by - 20).max(0) as usize;
        let x1 = ((bx + bw + 20) as usize).min(surf.w);
        let y1 = ((by + bh + 20) as usize).min(surf.h);
        let f = ((x1 - x0).max(y1 - y0)).div_ceil(800).max(2);
        let (w, h) = ((x1 - x0) / f, (y1 - y0) / f);

        let mut gray = vec![0u8; w * h];
        for oy in 0..h {
            for ox in 0..w {
                let mut acc = 0u32;
                for sy in 0..f {
                    for sx in 0..f {
                        acc += surf.luma((x0 + ox * f + sx) as i32, (y0 + oy * f + sy) as i32) as u32;
                    }
                }
                gray[oy * w + ox] = (acc / (f * f) as u32) as u8;
            }
        }

        let file = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        // Fast deflate: encode time matters more than a few KB on the tablet.
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&gray)
            .map_err(std::io::Error::other)?;
        // The rect the pixels actually cover: what was cropped, downscale
        // truncation included, in page coordinates.
        Ok(BBox {
            x0: x0 as i32,
            y0: y0 as i32,
            x1: (x0 + w * f) as i32 - 1,
            y1: (y0 + h * f) as i32 - 1,
        })
    }
}

/// Deterministic per-pixel hash for the dissolve pattern.
#[inline]
fn px_hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E3779B1) ^ (y as u32).wrapping_mul(0x85EBCA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2AE35);
    h ^ (h >> 16)
}

/// One pass of the "diary drinks the ink" effect: erase the pixels whose hash
/// falls in this stage. After `stages` passes the region is clean white.
pub fn dissolve_pass(surf: &mut Surface, region: BBox, stage: u32, stages: u32) {
    if region.is_empty() {
        return;
    }
    for y in region.y0..=region.y1 {
        for x in region.x0..=region.x1 {
            if surf.luma(x, y) < 250 && px_hash(x, y) % stages <= stage {
                surf.put_px(x, y, WHITE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::PixFmt;

    fn surf() -> (Vec<u8>, Surface) {
        let mut buf = vec![0xFFu8; 400 * 400 * 4];
        let ptr = buf.as_mut_ptr();
        let s = Surface::new(ptr, buf.len(), 400, 400, 400 * 4, PixFmt::Rgb32);
        (buf, s)
    }

    #[test]
    fn erase_forgets_covered_points_and_splits_strokes() {
        let (_buf, mut s) = surf();
        let mut ink = Ink::new();
        // A horizontal stroke across the page.
        for x in (20..=200).step_by(10) {
            ink.pen_point(&mut s, x, 100, 3);
        }
        ink.pen_up();
        assert_eq!(ink.stroke_list().len(), 1);
        let before: usize = ink.stroke_list().iter().map(|s| s.len()).sum();

        // Erase through the middle: the stroke splits, points vanish.
        ink.erase_point(&mut s, 110, 100, 20);
        let after: usize = ink.stroke_list().iter().map(|s| s.len()).sum();
        assert!(after < before, "erase kept every point ({after} of {before})");
        assert_eq!(ink.stroke_list().len(), 2, "middle-erase should split the stroke");
        // No surviving point lies under the eraser.
        for st in ink.stroke_list() {
            for &(x, y, _) in st {
                assert!((x - 110).pow(2) + (y - 100).pow(2) > 22 * 22);
            }
        }
    }

    #[test]
    fn the_answer_crop_follows_ink_that_leans_over_the_box() {
        let (_buf, mut s) = surf();
        let mut ink = Ink::new();
        let region = BBox { x0: 100, y0: 100, x1: 200, y1: 200 };
        // A digit written too big: one stroke starts inside the box and
        // runs well past its bottom edge.
        for y in (150..=260).step_by(10) {
            ink.pen_point(&mut s, 150, y, 3);
        }
        ink.pen_up();
        // The DONE tick, far away in its own box: never part of the answer.
        for i in 0..5 {
            ink.pen_point(&mut s, 40 + i, 380, 3);
        }
        ink.pen_up();

        let crop = ink.crop_for(&region, 60);
        assert!(crop.y1 >= 260, "the overflowing stroke must stay whole (y1={})", crop.y1);
        assert!(crop.x0 >= 90 && crop.x1 <= 210, "the crop must not swallow the page");
        assert!(crop.y0 <= 100 && crop.x0 <= 100, "the declared region is always included");
        assert!(!(crop.x1 >= 380 || crop.y1 >= 370), "a far-off tick must be excluded");

        // An empty region stays empty — DONE with no blank sends nothing.
        let empty = ink.crop_for(&BBox::empty(), 60);
        assert!(empty.is_empty());
    }

    #[test]
    fn ink_png_carries_the_strokes_and_none_of_the_page() {
        let (_buf, mut s) = surf();
        // Printed furniture: a black box on the page the tutor must not see.
        for x in 100..=200 {
            s.put_px(x, 100, BLACK);
            s.put_px(x, 200, BLACK);
        }
        let mut ink = Ink::new();
        // The child's digit: a stroke inside and past the box.
        for y in (120..=260).step_by(10) {
            ink.pen_point(&mut s, 150, y, 3);
        }
        ink.pen_up();

        let region = BBox { x0: 100, y0: 100, x1: 200, y1: 200 };
        let path = std::env::temp_dir().join("g-pad-ink-png-test.png");
        let path = path.to_str().unwrap();
        ink.ink_png(&region, 60, path).unwrap();

        let dec = png::Decoder::new(std::fs::File::open(path).unwrap());
        let mut reader = dec.read_info().unwrap();
        let mut img = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut img).unwrap();
        let (w, h) = (info.width as usize, info.height as usize);
        let dark: usize = img[..w * h].iter().filter(|&&p| p < 128).count();
        assert!(dark > 20, "the stroke must be on the canvas ({dark} dark px)");
        // The page's printed box would put a solid dark row near the top; the
        // ink-only canvas has none — count dark pixels on each row: no row may
        // be mostly dark the way a printed rule would be.
        for y in 0..h {
            let row_dark = img[y * w..(y + 1) * w].iter().filter(|&&p| p < 128).count();
            assert!(row_dark < w / 2, "row {y} looks like printed furniture");
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_dot_is_a_tap_and_a_stroke_is_not() {
        let (_buf, mut s) = surf();
        let mut ink = Ink::new();
        // A quick tap: many samples, all within a few pixels.
        for i in 0..10 {
            ink.pen_point(&mut s, 100 + i % 3, 100 + i % 2, 3);
        }
        ink.pen_up();
        assert!(ink.last_stroke_is_tap());
        // Writing a digit spans far more than a dot.
        for y in (100..160).step_by(5) {
            ink.pen_point(&mut s, 200, y, 3);
        }
        ink.pen_up();
        assert!(!ink.last_stroke_is_tap());
        ink.clear();
        assert!(!ink.last_stroke_is_tap(), "no stroke, no tap");
    }

    #[test]
    fn a_tap_clear_of_the_writing_is_a_command_and_a_dot_on_an_i_is_ink() {
        let (_buf, mut s) = surf();
        let mut ink = Ink::new();
        assert!(!ink.last_stroke_clear_of_rest(100), "no stroke, no command");
        // A word's stem near (100, 100)...
        for y in (80..120).step_by(5) {
            ink.pen_point(&mut s, 100, y, 3);
        }
        ink.pen_up();
        assert!(ink.last_stroke_clear_of_rest(100), "the only stroke is trivially clear");
        // ...its dot lands close by: ink, not a command.
        ink.pen_point(&mut s, 102, 70, 3);
        ink.pen_up();
        assert!(!ink.last_stroke_clear_of_rest(100));
        // A tap across the page is clear of it all.
        ink.pen_point(&mut s, 350, 350, 3);
        ink.pen_up();
        assert!(ink.last_stroke_clear_of_rest(100));
    }

    fn dark(s: &Surface, x0: i32, y0: i32, x1: i32, y1: i32) -> usize {
        let mut n = 0;
        for y in y0..=y1 {
            for x in x0..=x1 {
                if s.luma(x, y) < 128 {
                    n += 1;
                }
            }
        }
        n
    }

    /// Two short horizontal strokes, one at y=100 and one at y=300.
    fn two_strokes(s: &mut Surface) -> Ink {
        let mut ink = Ink::new();
        for x in (50..=150).step_by(5) {
            ink.pen_point(s, x, 100, 3);
        }
        ink.pen_up();
        for x in (50..=150).step_by(5) {
            ink.pen_point(s, x, 300, 3);
        }
        ink.pen_up();
        ink
    }

    #[test]
    fn a_lasso_takes_the_strokes_it_encloses_and_leaves_the_rest() {
        let (_buf, mut s) = surf();
        let ink = two_strokes(&mut s);
        let lasso = [(30, 70), (180, 70), (180, 130), (30, 130)];
        assert_eq!(ink.strokes_in(&lasso), vec![0]);
        // Tails poking out of a tight loop still count.
        let tight = [(70, 80), (180, 80), (180, 120), (70, 120)];
        assert_eq!(ink.strokes_in(&tight), vec![0]);
        assert!(ink.strokes_in(&[(0, 0), (10, 10)]).is_empty(), "two points enclose nothing");
    }

    #[test]
    fn moving_a_stroke_repaints_it_and_clears_where_it_was() {
        let (_buf, mut s) = surf();
        let mut ink = two_strokes(&mut s);
        ink.shift(&mut s, &[0], 100, 100);
        assert_eq!(dark(&s, 40, 90, 160, 110), 0, "the old place is paper again");
        assert!(dark(&s, 150, 190, 250, 210) > 100, "the stroke landed");
        assert!(dark(&s, 50, 290, 150, 310) > 100, "the other stroke is untouched");
        let b = ink.bounds(&[0]);
        assert!(b.x0 >= 140 && b.y0 >= 190, "the model moved with the pixels: {b:?}");
        assert!(ink.bbox.y1 >= 300, "the page bbox still holds both strokes");
    }

    #[test]
    fn a_move_across_another_stroke_leaves_it_whole() {
        let (_buf, mut s) = surf();
        let mut ink = two_strokes(&mut s);
        // A vertical stroke crossing the top one, then moved away.
        for y in (60..=140).step_by(5) {
            ink.pen_point(&mut s, 100, y, 3);
        }
        ink.pen_up();
        ink.shift(&mut s, &[2], 150, 0);
        assert!(dark(&s, 95, 97, 105, 103) > 20, "the crossing point of the top stroke came back");
    }

    #[test]
    fn deleting_strokes_removes_pixels_and_model_together() {
        let (_buf, mut s) = surf();
        let mut ink = two_strokes(&mut s);
        ink.remove(&mut s, &[1]);
        assert_eq!(ink.stroke_list().len(), 1);
        assert_eq!(dark(&s, 40, 290, 160, 310), 0);
        assert!(dark(&s, 50, 90, 150, 110) > 100);
        ink.remove(&mut s, &[0]);
        assert!(ink.is_empty() && ink.bbox.is_empty());
    }

    #[test]
    fn brushes_follow_their_strokes_through_an_erase() {
        let (_buf, mut s) = surf();
        let mut ink = Ink::new();
        let hl = Brush { kind: PenKind::Highlighter, size: crate::tools::Size::Fine };
        for x in (20..=200).step_by(10) {
            ink.pen_point_with(&mut s, x, 100, 9, hl);
        }
        ink.pen_up();
        for x in (20..=200).step_by(10) {
            ink.pen_point(&mut s, x, 300, 3);
        }
        ink.pen_up();
        ink.erase_point(&mut s, 110, 100, 20);
        assert_eq!(ink.stroke_list().len(), 3);
        assert_eq!(ink.brushes.len(), 3);
        assert!(ink.brushes[..2].iter().all(|b| b.kind == PenKind::Highlighter));
        assert_eq!(ink.brushes[2], Brush::default());
    }

    #[test]
    fn erasing_everything_empties_the_ink() {
        let (_buf, mut s) = surf();
        let mut ink = Ink::new();
        ink.pen_point(&mut s, 100, 100, 3);
        ink.pen_point(&mut s, 104, 100, 3);
        ink.pen_up();
        assert!(!ink.is_empty());
        ink.erase_point(&mut s, 102, 100, 30);
        assert!(ink.stroke_list().is_empty());
        assert!(ink.bbox.is_empty());
    }
}
