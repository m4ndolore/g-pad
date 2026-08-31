//! The writing canvas's page stack.
//!
//! The pad used to hold exactly one page: NEW PAGE wiped it, and whatever was
//! there was gone. Now the canvas is a small notebook — flipping back parks
//! the current sheet (pixels and ink together) and restores an earlier one,
//! flipping forward returns, and a fresh sheet goes on the end. Pages live in
//! memory only: they are scratch paper for the session, not the corpus. What
//! deserves remembering is committed with the rule gesture, as ever.
//!
//! A parked page is its full framebuffer snapshot plus its [`Ink`], so a
//! restored page can be written on, erased, and committed exactly as if it
//! had never been left.

use crate::ink::Ink;
use crate::surface::{Surface, WHITE};

/// A full-screen snapshot costs w*h*bytes-per-pixel (~5 MB on the rM2), so
/// the notebook is bounded. Sixteen sheets of scratch is a generous session.
pub const MAX_PAGES: usize = 16;

struct Page {
    snapshot: Vec<u8>,
    ink: Ink,
}

/// Pages behind and ahead of the sheet currently on the panel. The current
/// sheet itself lives on the live surface and in the live `Ink`; the notebook
/// only holds parked pages.
pub struct Notebook {
    back: Vec<Page>,
    forward: Vec<Page>,
}

impl Notebook {
    pub fn new() -> Self {
        Self { back: Vec::new(), forward: Vec::new() }
    }

    fn count(&self) -> usize {
        self.back.len() + self.forward.len() + 1
    }

    /// 1-based position of the current page, for the flip banner.
    pub fn position(&self) -> (usize, usize) {
        (self.back.len() + 1, self.count())
    }

    fn park(surf: &Surface, ink: &mut Ink) -> Page {
        Page {
            snapshot: surf.copy_rect(0, 0, surf.w, surf.h),
            ink: std::mem::replace(ink, Ink::new()),
        }
    }

    fn restore(page: Page, surf: &mut Surface, ink: &mut Ink) {
        surf.paste_rect(0, 0, surf.w, surf.h, &page.snapshot);
        *ink = page.ink;
    }

    /// Flip to the previous page. False at the front cover.
    pub fn prev(&mut self, surf: &mut Surface, ink: &mut Ink) -> bool {
        let Some(page) = self.back.pop() else { return false };
        self.forward.push(Self::park(surf, ink));
        Self::restore(page, surf, ink);
        true
    }

    /// Flip to the next page. At the last page a fresh sheet is added, but
    /// only when the current one carries ink — flipping past the end of an
    /// empty notebook must not mint blank pages — and never past [`MAX_PAGES`].
    pub fn next(&mut self, surf: &mut Surface, ink: &mut Ink) -> bool {
        if let Some(page) = self.forward.pop() {
            self.back.push(Self::park(surf, ink));
            Self::restore(page, surf, ink);
            return true;
        }
        if ink.is_empty() || self.count() >= MAX_PAGES {
            return false;
        }
        self.fresh(surf, ink);
        true
    }

    /// Park the current page and present a blank sheet. False when the
    /// current page is already blank (there is nothing to move past) or the
    /// notebook is full.
    pub fn new_page(&mut self, surf: &mut Surface, ink: &mut Ink) -> bool {
        if ink.is_empty() || self.count() >= MAX_PAGES {
            return false;
        }
        self.fresh(surf, ink);
        true
    }

    fn fresh(&mut self, surf: &mut Surface, ink: &mut Ink) {
        self.back.push(Self::park(surf, ink));
        surf.fill_rect(0, 0, surf.w, surf.h, WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::PixFmt;

    fn surf(buf: &mut Vec<u8>) -> Surface {
        let ptr = buf.as_mut_ptr();
        Surface::new(ptr, buf.len(), 64, 64, 64 * 4, PixFmt::Rgb32)
    }

    fn write_dot(s: &mut Surface, ink: &mut Ink, x: i32, y: i32) {
        ink.pen_point(s, x, y, 3);
        ink.pen_point(s, x + 2, y, 3);
        ink.pen_up();
    }

    #[test]
    fn flipping_back_and_forward_restores_pixels_and_ink() {
        let mut buf = vec![0xFFu8; 64 * 64 * 4];
        let mut s = surf(&mut buf);
        let mut nb = Notebook::new();
        let mut ink = Ink::new();

        write_dot(&mut s, &mut ink, 10, 10);
        assert!(nb.new_page(&mut s, &mut ink));
        assert!(ink.is_empty(), "a fresh sheet starts with no ink");
        assert!(s.luma(10, 10) > 200, "a fresh sheet starts white");
        write_dot(&mut s, &mut ink, 30, 30);

        assert!(nb.prev(&mut s, &mut ink));
        assert!(s.luma(10, 10) < 100, "page one's ink came back");
        assert!(s.luma(30, 30) > 200, "page two's ink is not on page one");
        assert_eq!(ink.stroke_list().len(), 1);
        assert_eq!(nb.position(), (1, 2));

        assert!(nb.next(&mut s, &mut ink));
        assert!(s.luma(30, 30) < 100, "page two's ink came back");
        assert_eq!(nb.position(), (2, 2));
    }

    #[test]
    fn the_covers_do_not_flip() {
        let mut buf = vec![0xFFu8; 64 * 64 * 4];
        let mut s = surf(&mut buf);
        let mut nb = Notebook::new();
        let mut ink = Ink::new();

        assert!(!nb.prev(&mut s, &mut ink), "no page behind the first");
        assert!(!nb.next(&mut s, &mut ink), "an empty page must not mint a new sheet");
        assert!(!nb.new_page(&mut s, &mut ink), "NEW PAGE on a blank sheet is a no-op");

        write_dot(&mut s, &mut ink, 10, 10);
        assert!(nb.next(&mut s, &mut ink), "an inked last page flips onto a fresh sheet");
        assert_eq!(nb.position(), (2, 2));
    }

    #[test]
    fn the_notebook_is_bounded() {
        let mut buf = vec![0xFFu8; 64 * 64 * 4];
        let mut s = surf(&mut buf);
        let mut nb = Notebook::new();
        let mut ink = Ink::new();

        for i in 0..MAX_PAGES + 4 {
            write_dot(&mut s, &mut ink, 10 + i as i32, 10);
            nb.new_page(&mut s, &mut ink);
        }
        assert_eq!(nb.position().1, MAX_PAGES);
        assert!(!ink.is_empty(), "the refused sheet keeps its ink");
        assert!(!nb.new_page(&mut s, &mut ink), "the notebook must stop at its cap");
    }
}
