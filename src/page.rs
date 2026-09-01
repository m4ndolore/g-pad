//! Shared geometry for the pad's bounded reading surfaces.
//!
//! Every reading surface — the daily brief, the Claude bridge, whatever comes
//! next — obeys the same discipline: one page, no scrolling on the first
//! screen, fewer blocks rather than a smaller face, and a footer that says what
//! was left out. That discipline is arithmetic, and it lives here so a third
//! surface inherits it instead of retyping it.
//!
//! What stays with each surface is what it *means*: which blocks are worth
//! showing, how many lines one is allowed, and what the footer counts. Only
//! the measuring is shared.

use crate::fb::{SCREEN_H, SCREEN_W};
use crate::script;
use ab_glyph::FontRef;

/// Outer margin. The reading surfaces share one page geometry so two open
/// side by side do not look like two different products.
pub const PAD: usize = 48;
/// Sized for the panel, not a laptop: ~226 dpi means 40px is roughly 12.5pt
/// on paper. The first hardware read (2026-08-31) found the old 28px body
/// unreadably small.
pub const TITLE_PX: f32 = 56.0;
pub const BODY_PX: f32 = 40.0;
/// One body line, and one title line. Titles are set larger, so they lead.
pub const LINE_H: usize = 54;
pub const TITLE_LINE_H: usize = 68;
/// Room reserved at the top for the surface's own header, and at the foot for
/// the "what was left out" line.
pub const HEADER_H: usize = 120;
pub const FOOTER_H: usize = 84;
/// A title may run to two lines. Beyond that it is not a title.
pub const MAX_TITLE_LINES: usize = 2;

pub fn content_width() -> usize {
    SCREEN_W - PAD * 2
}

/// The last y a block may occupy. `reserved` is room already claimed by
/// something pinned — the bridge pins artifacts, because evidence must not be
/// pushed off the page by prose.
pub fn limit(reserved: usize) -> usize {
    (SCREEN_H - FOOTER_H).saturating_sub(reserved)
}

/// Wrap to the content column, capped. The cap is the surface's call: a list
/// of items wants short excerpts, a single session can afford long turns.
pub fn wrap_capped(font: &FontRef, text: &str, px: f32, max_lines: usize) -> Vec<String> {
    script::wrap(font, text, px, content_width() as f32)
        .into_iter()
        .take(max_lines)
        .collect()
}

/// A title, wrapped and capped at [`MAX_TITLE_LINES`].
pub fn title_lines(font: &FontRef, title: &str) -> Vec<String> {
    wrap_capped(font, title, TITLE_PX, MAX_TITLE_LINES)
}

/// `"{lead} · {trailer}"`, or just `lead` when there is no trailer. The meta
/// row under a title: source and date on the brief, state and time on the
/// bridge.
pub fn meta_line(lead: &str, trailer: &str) -> String {
    if trailer.is_empty() {
        lead.to_string()
    } else {
        format!("{lead} · {trailer}")
    }
}

/// Which end of the sequence wins when not everything fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    /// Take from the front: the first items are the ones worth reading.
    Front,
    /// Take from the back, then restore order — the last exchange is the one
    /// worth reading, but the page still reads downward.
    Back,
}

/// Fit measured blocks onto the page, and report how many did not make it.
///
/// `height` measures a block. Blocks are taken until the next one would cross
/// `limit`; the count of those left over is returned with them, because every
/// surface has to say what it left out.
pub fn fit<T>(blocks: Vec<T>, start_y: usize, limit: usize, fill: Fill, height: impl Fn(&T) -> usize) -> (Vec<T>, usize) {
    let total = blocks.len();
    let mut y = start_y;
    let mut taken = Vec::new();
    let ordered: Box<dyn Iterator<Item = T>> = match fill {
        Fill::Front => Box::new(blocks.into_iter()),
        Fill::Back => Box::new(blocks.into_iter().rev()),
    };
    for block in ordered {
        let h = height(&block);
        if y + h > limit {
            break;
        }
        y += h;
        taken.push(block);
    }
    if fill == Fill::Back {
        taken.reverse();
    }
    let omitted = total.saturating_sub(taken.len());
    (taken, omitted)
}

/// Split measured blocks into pages, newest page first.
///
/// The single-page discipline stays: nothing scrolls, and the first screen
/// still shows the newest content. But a reader must be able to reach what
/// was left out — the first hardware read (2026-08-31) found a summary cut
/// off with no way to continue. So the leftovers become further pages, and
/// the swipe flips between them.
///
/// Page 0 holds the last blocks that fit between `start_y` and `limit`;
/// page 1 the blocks before those, and so on backward. Every block lands on
/// exactly one page — a block taller than the page itself gets a page alone
/// rather than vanishing.
pub fn paginate<T>(blocks: &[T], start_y: usize, limit: usize, height: impl Fn(&T) -> usize) -> Vec<std::ops::Range<usize>> {
    let mut pages = Vec::new();
    let mut end = blocks.len();
    while end > 0 {
        let mut y = start_y;
        let mut start = end;
        while start > 0 && y + height(&blocks[start - 1]) <= limit {
            y += height(&blocks[start - 1]);
            start -= 1;
        }
        if start == end {
            start = end - 1;
        }
        pages.push(start..end);
        end = start;
    }
    if pages.is_empty() {
        pages.push(0..0);
    }
    pages
}

/// Split measured blocks into pages, first page first.
///
/// The forward twin of [`paginate`], for surfaces that read front to back —
/// a vault note starts at its beginning, where a session starts at its end.
/// The same guarantees hold: every block lands on exactly one page, and a
/// block taller than the page gets a page alone rather than vanishing.
pub fn paginate_forward<T>(blocks: &[T], start_y: usize, limit: usize, height: impl Fn(&T) -> usize) -> Vec<std::ops::Range<usize>> {
    let mut pages = Vec::new();
    let mut start = 0;
    while start < blocks.len() {
        let mut y = start_y;
        let mut end = start;
        while end < blocks.len() && y + height(&blocks[end]) <= limit {
            y += height(&blocks[end]);
            end += 1;
        }
        if end == start {
            end = start + 1;
        }
        pages.push(start..end);
        start = end;
    }
    if pages.is_empty() {
        pages.push(0..0);
    }
    pages
}

/// The end of a reference is the part that identifies it — a path's file, a
/// sha, a project directory. Keep the tail, mark the cut.
pub fn tail(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let kept: String = s.chars().skip(n - (max - 1)).collect();
    format!("…{kept}")
}

/// Join footer fragments with `·`, dropping the empty ones. Empty when
/// everything fit — no need to say "0 more".
pub fn footer(parts: &[String]) -> String {
    parts.iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(" · ")
}

/// `"{n} {singular}"`, pluralized, or empty at zero.
pub fn counted(n: usize, singular: &str, plural: &str) -> String {
    match n {
        0 => String::new(),
        1 => format!("1 {singular}"),
        n => format!("{n} {plural}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_stops_before_the_footer() {
        let blocks = vec![100usize; 100];
        let (taken, omitted) = fit(blocks, HEADER_H, limit(0), Fill::Front, |b| *b);
        assert_eq!(taken.len() + omitted, 100);
        assert!(HEADER_H + taken.len() * 100 <= limit(0), "the page must not run into the footer");
        assert!(omitted > 0, "100 blocks of 100px cannot fit");
    }

    #[test]
    fn pinned_room_costs_the_page_blocks() {
        let plain = fit(vec![100usize; 100], HEADER_H, limit(0), Fill::Front, |b| *b).0.len();
        let pinned = fit(vec![100usize; 100], HEADER_H, limit(400), Fill::Front, |b| *b).0.len();
        assert!(pinned < plain, "reserving room must push prose off the page, not the reverse");
    }

    #[test]
    fn filling_from_the_back_keeps_the_last_blocks_in_order() {
        let (taken, omitted) = fit((0..100usize).collect(), HEADER_H, limit(0), Fill::Back, |_| 200);
        assert!(omitted > 0);
        // The newest block is last on the page, and the page reads downward.
        assert_eq!(taken.last().copied(), Some(99));
        let mut sorted = taken.clone();
        sorted.sort_unstable();
        assert_eq!(taken, sorted, "back-filled pages must still read in order");
    }

    #[test]
    fn everything_fitting_leaves_nothing_omitted() {
        let (taken, omitted) = fit(vec![10usize; 3], HEADER_H, limit(0), Fill::Front, |b| *b);
        assert_eq!(taken.len(), 3);
        assert_eq!(omitted, 0);
    }

    #[test]
    fn pages_cover_every_block_once_and_the_newest_page_is_first() {
        let blocks = vec![300usize; 20];
        let pages = paginate(&blocks, HEADER_H, limit(0), |b| *b);
        assert!(pages.len() > 1, "20 blocks of 300px cannot be one page");
        // Page 0 ends at the newest block; walking the pages backward covers
        // the whole sequence with no gaps and no overlaps.
        assert_eq!(pages[0].end, blocks.len());
        assert_eq!(pages.last().unwrap().start, 0);
        for w in pages.windows(2) {
            assert_eq!(w[1].end, w[0].start);
        }
        // Every page fits between start and limit.
        for p in &pages {
            let used: usize = HEADER_H + blocks[p.clone()].iter().sum::<usize>();
            assert!(used <= limit(0), "a page must not run into the footer");
        }
    }

    #[test]
    fn an_oversized_block_gets_a_page_alone_and_nothing_vanishes() {
        let blocks = vec![100, SCREEN_H * 2, 100];
        let pages = paginate(&blocks, HEADER_H, limit(0), |b| *b);
        let covered: usize = pages.iter().map(|p| p.len()).sum();
        assert_eq!(covered, 3, "every block must land on exactly one page");
        assert!(pages.iter().any(|p| p.len() == 1 && blocks[p.start] > SCREEN_H));
    }

    #[test]
    fn no_blocks_still_reads_as_one_empty_page() {
        let pages = paginate(&Vec::<usize>::new(), HEADER_H, limit(0), |b| *b);
        assert_eq!(pages, vec![0..0]);
    }

    #[test]
    fn forward_pages_cover_every_block_once_and_the_first_page_is_first() {
        let blocks = vec![300usize; 20];
        let pages = paginate_forward(&blocks, HEADER_H, limit(0), |b| *b);
        assert!(pages.len() > 1, "20 blocks of 300px cannot be one page");
        assert_eq!(pages[0].start, 0);
        assert_eq!(pages.last().unwrap().end, blocks.len());
        for w in pages.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        for p in &pages {
            let used: usize = HEADER_H + blocks[p.clone()].iter().sum::<usize>();
            assert!(used <= limit(0), "a page must not run into the footer");
        }
    }

    #[test]
    fn forward_oversized_block_gets_a_page_alone() {
        let blocks = vec![100, SCREEN_H * 2, 100];
        let pages = paginate_forward(&blocks, HEADER_H, limit(0), |b| *b);
        let covered: usize = pages.iter().map(|p| p.len()).sum();
        assert_eq!(covered, 3, "every block must land on exactly one page");
        assert!(pages.iter().any(|p| p.len() == 1 && blocks[p.start] > SCREEN_H));
    }

    #[test]
    fn forward_no_blocks_still_reads_as_one_empty_page() {
        let pages = paginate_forward(&Vec::<usize>::new(), HEADER_H, limit(0), |b| *b);
        assert_eq!(pages, vec![0..0]);
    }

    #[test]
    fn a_reference_keeps_its_tail_when_cut() {
        assert_eq!(tail("a1b2c3d", 22), "a1b2c3d");
        let cut = tail("/Users/p/Dev/g-pad/src/bridge.rs", 22);
        assert!(cut.starts_with('…'));
        assert!(cut.ends_with("bridge.rs"));
        assert_eq!(cut.chars().count(), 22);
    }

    #[test]
    fn the_footer_drops_empty_parts_and_joins_the_rest() {
        assert_eq!(footer(&[String::new(), String::new()]), "");
        assert_eq!(footer(&["2 more".into(), String::new(), "not refreshed".into()]), "2 more · not refreshed");
    }

    #[test]
    fn counted_pluralizes_and_vanishes_at_zero() {
        assert_eq!(counted(0, "session", "sessions"), "");
        assert_eq!(counted(1, "session", "sessions"), "1 session");
        assert_eq!(counted(4, "session", "sessions"), "4 sessions");
    }

    #[test]
    fn a_meta_row_survives_a_missing_trailer() {
        assert_eq!(meta_line("running", "14:02"), "running · 14:02");
        assert_eq!(meta_line("running", ""), "running");
    }
}
