//! The Anthink cards: the boot splash, and the power-off and restart sheets.
//! Same grammar as the Merge Combinator sleep card in `gesture.rs` — one
//! hairline frame, one left spine, set type over rules, one line in the
//! pad's own hand — so the two read as one system.

use ab_glyph::FontRef;

use crate::fb::{SCREEN_H, SCREEN_W};
use crate::gesture::{blit_centered, blit_gray, blit_left, blit_left_in, frame, mark_gray};
use crate::surface::{PixFmt, Surface, BLACK, FADED, WHITE};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    /// First frame after the panel is ours; holds through the escape window.
    Boot,
    /// Drawn before `systemctl poweroff`; the OS's poweroff.png is its twin.
    PowerOff,
    /// Drawn before `systemctl reboot`; the OS's rebooting.png is its twin.
    Restart,
}

impl Card {
    fn line(self) -> &'static str {
        match self {
            Card::Boot => "Getting the pen ready.",
            Card::PowerOff => "Powering off.",
            Card::Restart => "Restarting.",
        }
    }
}

/// Left spine every block hangs from, as on the sleep card.
const ML: usize = 150;
/// Where the wordmark band starts; the tests look for ink here.
pub const WORDMARK_Y: usize = (SCREEN_H - 1030) / 2 - 60 + 62 + 82;
/// The escape-hatch line's rect (x, y, w, h): drawn on the boot card only,
/// erased with one partial update once the window closes.
pub const HATCH: (usize, usize, usize, usize) = (ML, SCREEN_H - 120, SCREEN_W - 2 * ML, 40);
const HATCH_TEXT: &str = "POWER BUTTON NOW: REMARKABLE HOME";

/// Box-average a grayscale image down 3×. Averaging (not sampling) keeps
/// the mark's mid-gray stroke a mid-gray at a third of the size.
pub(crate) fn shrink3(w: usize, h: usize, gray: &[u8]) -> (usize, usize, Vec<u8>) {
    let (sw, sh) = (w / 3, h / 3);
    let mut out = Vec::with_capacity(sw * sh);
    for y in 0..sh {
        for x in 0..sw {
            let mut sum = 0u32;
            for dy in 0..3 {
                for dx in 0..3 {
                    sum += gray[(y * 3 + dy) * w + x * 3 + dx] as u32;
                }
            }
            out.push((sum / 9) as u8);
        }
    }
    (sw, sh, out)
}

/// Draw `card` over the whole surface. The caller pushes it with a full
/// refresh; nothing on it changes afterwards except the hatch erase.
pub fn draw(surf: &mut Surface, hand: &FontRef, ui_font: &FontRef, card: Card) {
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    frame(surf, 56, 56, SCREEN_W - 112, SCREEN_H - 112, 2);
    let rule_w = SCREEN_W - ML * 2;

    let mut y = (SCREEN_H - 1030) / 2 - 60;
    blit_left(surf, ui_font, "ANTHINK SLATE", 32.0, 9, ML, y);

    y += 62;
    surf.fill_rect(ML, y, rule_w, 1, BLACK);

    // The wordmark is typographic — one word, one weight. It must not read
    // as a copy of the Merge / COMBINATOR pairing on the sleep card.
    y += 82;
    debug_assert_eq!(y, WORDMARK_Y);
    blit_left(surf, ui_font, "ANTHINK", 150.0, 4, ML, y);

    y += 190;
    surf.fill_rect(ML, y, rule_w, 1, BLACK);

    // The parent brand, once: the venture line with the mark hung against
    // the right margin at a third of its sleep-card size.
    y += 25;
    let mut line_y = y + 40;
    if let Some((mw, mh, gray)) = mark_gray() {
        let (sw, sh, small) = shrink3(mw, mh, &gray);
        let mx = SCREEN_W - ML - sw;
        blit_gray(surf, mx, y, sw, sh, &small);
        line_y = y + sh.saturating_sub(23) / 2;
    }
    blit_left(surf, ui_font, "A MERGE COMBINATOR VENTURE.", 32.0, 7, ML, line_y);

    blit_centered(surf, hand, card.line(), 50.0, 0, SCREEN_W, SCREEN_H - 210);

    if card == Card::Boot {
        let (hx, hy, _, _) = HATCH;
        blit_left_in(surf, ui_font, HATCH_TEXT, 22.0, 6, hx, hy + 8, FADED);
    }
}

/// Paper over the hatch line once the escape window has closed.
pub fn erase_hatch(surf: &mut Surface) {
    let (x, y, w, h) = HATCH;
    surf.fill_rect(x, y, w, h, WHITE);
}

/// Write the three cards to `dir` as 1404×1872 8-bit grayscale PNGs — the
/// format of the images in /usr/share/remarkable, so poweroff.png and
/// rebooting.png can replace the OS's own. Returns the paths written.
pub fn render_to(dir: &str, hand: &FontRef, ui_font: &FontRef) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    let mut buf = vec![0xFFu8; SCREEN_W * SCREEN_H * 4];
    let ptr = buf.as_mut_ptr();
    let mut surf = Surface::new(ptr, buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
    let mut out = Vec::new();
    for (card, name) in [
        (Card::Boot, "boot.png"),
        (Card::PowerOff, "poweroff.png"),
        (Card::Restart, "rebooting.png"),
    ] {
        draw(&mut surf, hand, ui_font, card);
        let path = format!("{dir}/{name}");
        crate::dump_page(&surf, &path)?;
        out.push(path);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> (FontRef<'static>, FontRef<'static>) {
        let hand = FontRef::try_from_slice(include_bytes!("../fonts/DancingScript.ttf")).unwrap();
        let ui = FontRef::try_from_slice(crate::ui::UI_FONT_TTF).unwrap();
        (hand, ui)
    }

    fn dark(surf: &Surface, x0: usize, y0: usize, w: usize, h: usize) -> usize {
        let mut n = 0;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if surf.luma(x as i32, y as i32) < 128 {
                    n += 1;
                }
            }
        }
        n
    }

    fn surface_of(buf: &mut [u8]) -> Surface {
        let ptr = buf.as_mut_ptr();
        Surface::new(ptr, buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32)
    }

    /// Render `card` over a page that already carries ink, so the test can
    /// tell "drew the card" from "left the old page alone".
    fn render(card: Card) -> Vec<u8> {
        let mut buf = vec![0u8; SCREEN_W * SCREEN_H * 4];
        let mut surf = surface_of(&mut buf);
        surf.fill_rect(300, 1000, 400, 300, BLACK);
        let (hand, ui) = fonts();
        draw(&mut surf, &hand, &ui, card);
        buf
    }

    #[test]
    fn boot_card_has_the_frame_and_the_wordmark_on_the_spine() {
        let mut buf = render(Card::Boot);
        let surf = surface_of(&mut buf);
        assert_eq!(dark(&surf, 56, 56, SCREEN_W - 112, 2), (SCREEN_W - 112) * 2, "top frame edge is not a solid 2px line");
        // Nothing but the frame lives left of the spine.
        assert_eq!(dark(&surf, 60, 100, 88, SCREEN_H - 200), 0, "ink left of the x=150 spine");
        // The wordmark band carries real ink inside the column.
        let ink = dark(&surf, ML, WORDMARK_Y, SCREEN_W - 2 * ML, 160);
        // A real render carries ~17k; the floor says "not empty", no more.
        assert!(ink > 14_000, "wordmark band looks empty: {ink} dark px");
        // The stale ink under the card is gone (this band sits between the
        // venture line and the human line, where the card draws nothing).
        assert_eq!(dark(&surf, 300, 1000, 400, 300), 0, "stale ink shows through the card");

        let path = std::env::temp_dir().join("g-pad-boot-card.png");
        crate::dump_page(&surf, path.to_str().unwrap()).unwrap();
        eprintln!("boot card: {}", path.display());
    }

    #[test]
    fn only_the_boot_card_shows_the_hatch_line() {
        let (x, y, w, h) = HATCH;
        for (card, expect) in [(Card::Boot, true), (Card::PowerOff, false), (Card::Restart, false)] {
            let mut buf = render(card);
            let surf = surface_of(&mut buf);
            let ink = dark(&surf, x, y, w, h);
            assert_eq!(ink > 200, expect, "{card:?}: hatch ink {ink}");
        }
    }

    #[test]
    fn erase_hatch_leaves_only_paper_in_its_rect() {
        let mut buf = render(Card::Boot);
        let mut surf = surface_of(&mut buf);
        erase_hatch(&mut surf);
        let (x, y, w, h) = HATCH;
        assert_eq!(dark(&surf, x, y, w, h), 0);
        // The human line above it survives.
        let ink = dark(&surf, 0, SCREEN_H - 210, SCREEN_W, 80);
        assert!(ink > 500, "human line erased with the hatch: {ink} dark px");
    }

    #[test]
    fn the_mark_keeps_its_grays_after_shrinking() {
        let (mw, mh, gray) = mark_gray().unwrap();
        let (sw, sh, small) = shrink3(mw, mh, &gray);
        assert_eq!((sw, sh), (mw / 3, mh / 3));
        let mut buckets = std::collections::BTreeSet::new();
        for &g in &small {
            buckets.insert(g / 16);
        }
        assert!(buckets.len() >= 8, "shrunk mark has only {} ink levels", buckets.len());
    }

    #[test]
    fn render_to_writes_the_os_image_format() {
        let dir = std::env::temp_dir().join(format!("g-pad-cards-{}", std::process::id()));
        let (hand, ui) = fonts();
        let written = render_to(dir.to_str().unwrap(), &hand, &ui).unwrap();
        assert_eq!(written.len(), 3);
        for name in ["boot.png", "poweroff.png", "rebooting.png"] {
            let path = dir.join(name);
            let dec = png::Decoder::new(std::fs::File::open(&path).unwrap());
            let reader = dec.read_info().unwrap();
            let info = reader.info();
            assert_eq!((info.width, info.height), (SCREEN_W as u32, SCREEN_H as u32), "{name}");
            assert_eq!(info.color_type, png::ColorType::Grayscale, "{name}");
            assert_eq!(info.bit_depth, png::BitDepth::Eight, "{name}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
