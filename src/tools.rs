//! The writing kit: what the pen tip does (write, erase, or select) and,
//! when it writes, with what: a pen kind and a size, the way the stock
//! tablet's tool bar offers them. The tool menu under the corner button sets
//! all three; the strip's tip cell and the SYSTEM page's PEN TIP row still
//! flip between pen and eraser.
//!
//! A brush paints one of three ways. Solid ink covers the paper. Pencil is
//! a grain: a fixed per-pixel pattern, so a stroke repainted after a move
//! comes back pixel for pixel. The highlighter only lightens paper that is
//! lighter than itself, so it never covers the writing under it.

use crate::pen::MAX_PRESSURE;
use crate::surface::{Surface, BLACK};

/// What the pen tip does. The marker's hardware eraser end always erases,
/// whatever this says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tip {
    #[default]
    Pen,
    Eraser,
    /// Lasso finished strokes, then drag them or delete them.
    Select,
}

impl Tip {
    /// The strip's and SYSTEM page's flip: pen and eraser trade places, and
    /// a select tip goes back to writing.
    pub fn flipped(self) -> Self {
        match self {
            Tip::Pen => Tip::Eraser,
            Tip::Eraser | Tip::Select => Tip::Pen,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tip::Pen => "PEN",
            Tip::Eraser => "ERASER",
            Tip::Select => "SELECT",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PenKind {
    /// One width whatever the pressure.
    Fineliner,
    /// Pressure widens the line. The pad's pen before there was a menu.
    #[default]
    Ballpoint,
    /// Broad and nearly even.
    Marker,
    /// Graphite grain.
    Pencil,
    /// A light band that stays under the ink it crosses.
    Highlighter,
}

impl PenKind {
    pub const ALL: [PenKind; 5] =
        [PenKind::Fineliner, PenKind::Ballpoint, PenKind::Marker, PenKind::Pencil, PenKind::Highlighter];

    pub fn label(self) -> &'static str {
        match self {
            PenKind::Fineliner => "FINELINER",
            PenKind::Ballpoint => "BALLPOINT",
            PenKind::Marker => "MARKER",
            PenKind::Pencil => "PENCIL",
            PenKind::Highlighter => "HIGHLIGHTER",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Size {
    Fine,
    #[default]
    Medium,
    Bold,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Fine, Size::Medium, Size::Bold];

    pub fn label(self) -> &'static str {
        match self {
            Size::Fine => "FINE",
            Size::Medium => "MEDIUM",
            Size::Bold => "BOLD",
        }
    }
}

/// Graphite: a dark gray, laid down on five pixels in eight.
const PENCIL: u16 = 0x39E7;
const PENCIL_GRAIN: u32 = 5;
/// Highlighter: a light gray, luma about 200 of 255.
const HIGHLIGHT: u16 = 0xCE59;
const HIGHLIGHT_LUMA: u8 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Brush {
    pub kind: PenKind,
    pub size: Size,
}

impl Brush {
    /// Radius in pixels for a pen pressure of 0..=`MAX_PRESSURE`. Ballpoint
    /// at medium is `2 + p * 3 / MAX`, the pad's line before the menu.
    pub fn radius(self, pressure: i32) -> i32 {
        use PenKind::*;
        use Size::*;
        let (base, swing) = match (self.kind, self.size) {
            (Fineliner, Fine) => (1, 0),
            (Fineliner, Medium) => (2, 0),
            (Fineliner, Bold) => (4, 0),
            (Ballpoint, Fine) => (1, 2),
            (Ballpoint, Medium) => (2, 3),
            (Ballpoint, Bold) => (3, 5),
            (Marker, Fine) => (5, 1),
            (Marker, Medium) => (7, 2),
            (Marker, Bold) => (10, 3),
            (Pencil, Fine) => (1, 1),
            (Pencil, Medium) => (2, 2),
            (Pencil, Bold) => (3, 3),
            (Highlighter, Fine) => (9, 0),
            (Highlighter, Medium) => (13, 0),
            (Highlighter, Bold) => (18, 0),
        };
        base + pressure.clamp(0, MAX_PRESSURE) * swing / MAX_PRESSURE
    }

    /// Lay one piece of a stroke: a dab at `to` when the stroke starts
    /// there, else a line from the previous point. Solid ink goes through
    /// `Surface::brush_line`, so the default pen draws exactly as before.
    pub fn segment(self, surf: &mut Surface, from: Option<(i32, i32, i32)>, to: (i32, i32, i32)) {
        let (x, y, r) = to;
        match (self.kind, from) {
            (PenKind::Pencil | PenKind::Highlighter, Some((px, py, pr))) => {
                let r = r.min(pr + 1);
                let steps = (x - px).abs().max((y - py).abs()).max(1);
                for i in 0..=steps {
                    self.dab(surf, px + (x - px) * i / steps, py + (y - py) * i / steps, r);
                }
            }
            (PenKind::Pencil | PenKind::Highlighter, None) => self.dab(surf, x, y, r),
            (_, Some((px, py, pr))) => surf.brush_line(px, py, x, y, r.min(pr + 1), BLACK),
            (_, None) => surf.stamp(x, y, r, BLACK),
        }
    }

    fn dab(self, surf: &mut Surface, cx: i32, cy: i32, r: i32) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy > r * r {
                    continue;
                }
                let (x, y) = (cx + dx, cy + dy);
                match self.kind {
                    PenKind::Pencil if grain(x, y) % 8 < PENCIL_GRAIN => surf.put_px(x, y, PENCIL),
                    PenKind::Highlighter if surf.luma(x, y) > HIGHLIGHT_LUMA => surf.put_px(x, y, HIGHLIGHT),
                    _ => {}
                }
            }
        }
    }
}

/// Deterministic per-pixel hash for the pencil's grain.
#[inline]
fn grain(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x27D4EB2D) ^ (y as u32).wrapping_mul(0x165667B1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EBCA77);
    h ^ (h >> 13)
}

/// Everything the tool menu sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Kit {
    pub tip: Tip,
    pub brush: Brush,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::PixFmt;

    fn page() -> (Vec<u8>, Surface) {
        let mut buf = vec![0xFFu8; 200 * 200 * 4];
        let ptr = buf.as_mut_ptr();
        let s = Surface::new(ptr, buf.len(), 200, 200, 200 * 4, PixFmt::Rgb32);
        (buf, s)
    }

    #[test]
    fn the_default_pen_is_the_old_pen() {
        let b = Brush::default();
        for p in [0, 1000, 2048, 3000, MAX_PRESSURE] {
            assert_eq!(b.radius(p), 2 + p * 3 / MAX_PRESSURE);
        }
        assert_eq!(Kit::default().tip, Tip::Pen);
    }

    #[test]
    fn sizes_grow_and_fineliners_ignore_pressure() {
        for kind in PenKind::ALL {
            let r = |size| Brush { kind, size }.radius(MAX_PRESSURE / 2);
            assert!(r(Size::Fine) < r(Size::Medium) && r(Size::Medium) < r(Size::Bold), "{kind:?}");
        }
        let f = Brush { kind: PenKind::Fineliner, size: Size::Medium };
        assert_eq!(f.radius(0), f.radius(MAX_PRESSURE));
    }

    #[test]
    fn the_highlighter_never_covers_ink() {
        let (_buf, mut s) = page();
        s.fill_rect(90, 0, 20, 200, BLACK);
        let h = Brush { kind: PenKind::Highlighter, size: Size::Bold };
        h.segment(&mut s, Some((10, 100, 18)), (190, 100, 18));
        assert_eq!(s.luma(100, 100), 0, "the ink under the band stays black");
        let paper = s.luma(40, 100);
        assert!(paper < 255 && paper > 150, "the band is a light gray, got {paper}");
    }

    #[test]
    fn pencil_grain_repaints_identically() {
        let (_a, mut s1) = page();
        let (_b, mut s2) = page();
        let p = Brush { kind: PenKind::Pencil, size: Size::Bold };
        for s in [&mut s1, &mut s2] {
            p.segment(s, None, (50, 50, 3));
            p.segment(s, Some((50, 50, 3)), (150, 120, 3));
        }
        let mut marked = 0;
        for y in 0..200 {
            for x in 0..200 {
                assert_eq!(s1.luma(x, y), s2.luma(x, y));
                if s1.luma(x, y) < 255 {
                    marked += 1;
                }
            }
        }
        assert!(marked > 100, "the pencil drew nothing");
    }

    #[test]
    fn the_flip_trades_pen_and_eraser_and_select_goes_back_to_writing() {
        assert_eq!(Tip::Pen.flipped(), Tip::Eraser);
        assert_eq!(Tip::Eraser.flipped(), Tip::Pen);
        assert_eq!(Tip::Select.flipped(), Tip::Pen);
    }
}
