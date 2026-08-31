//! Pen-gesture recognition, and the sleep card.
//!
//! The sleep card is Merge Combinator's identity set for e-ink, absorbed from
//! `help.rs` when that module was retired (see `docs/anthink-interaction.md`).
//! It needs both faces: the grotesque for the wordmark and mission lines, the
//! pad's own hand for the one functional line.
//!
//! Recognition is local geometry — no oracle — so a gesture works with no
//! network. Every recognizer here is deliberate and hard to produce by
//! accident: the cost of a false positive is a committed page.

use crate::fb::{SCREEN_H, SCREEN_W};
use crate::script;
use crate::surface::{Surface, BLACK, WHITE};
use ab_glyph::FontRef;

/// The deliberate "send" gesture: a long, flat, horizontal stroke — a rule
/// drawn under the words, like signing off a diary entry. `min_w` is supplied
/// by the caller (proportional to how wide the written text is).
pub fn looks_like_send_rule(stroke: &[(i32, i32, i32)], min_w: i32) -> bool {
    if stroke.len() < 12 {
        return false;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for &(x, y, _) in stroke {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    // Wide enough, roughly flat, and much wider than tall.
    w >= min_w && h <= 110 && w >= h * 4
}

/// An underline finished as a right-pointing arrow selects the alternate
/// oracle model (Vellum Ask). The shaft still has to meet the deliberate-send
/// width; the final quarter must double back and deviate vertically.
pub fn looks_like_ask_arrow(stroke: &[(i32, i32, i32)], min_w: i32) -> bool {
    if !looks_like_send_rule(stroke, min_w) || stroke.len() < 16 {
        return false;
    }
    let x0 = stroke.iter().map(|p| p.0).min().unwrap_or(0);
    let x1 = stroke.iter().map(|p| p.0).max().unwrap_or(0);
    let width = x1 - x0;
    let tip_i = stroke.iter().enumerate().max_by_key(|(_, p)| p.0).map(|(i, _)| i).unwrap_or(0);
    let tail = &stroke[tip_i..];
    if tail.len() < 4 || tip_i < stroke.len() / 2 {
        return false;
    }
    let tip_y = stroke[tip_i].1;
    let doubled_back = tail.iter().any(|p| p.0 < x1 - width / 12);
    let vertical_head = tail.iter().any(|p| (p.1 - tip_y).abs() > 28);
    doubled_back && vertical_head
}

const MARK_PNG: &[u8] = include_bytes!("../assets/mc-mark.png");

/// Decode the mark once: (width, height, gray bytes). Returns None rather than
/// panicking — a bad asset must never keep the pad from sleeping.
fn mark_gray() -> Option<(usize, usize, Vec<u8>)> {
    let dec = png::Decoder::new(MARK_PNG);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Grayscale || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some((info.width as usize, info.height as usize, buf))
}

/// Paint 8-bit gray art onto the surface. The panel's GC16 waveform resolves
/// 16 levels, so the mark's antialiased curves survive as real tone rather
/// than a threshold-crushed silhouette.
fn blit_gray(surf: &mut Surface, x0: usize, y0: usize, w: usize, h: usize, gray: &[u8]) {
    for row in 0..h {
        for col in 0..w {
            let g = gray[row * w + col];
            if g >= 250 {
                continue; // leave paper white
            }
            let q = (g as u16) >> 3; // 8-bit -> 5 bits per channel
            let px = (q << 11) | (((g as u16) >> 2) << 5) | q;
            surf.put_px((x0 + col) as i32, (y0 + row) as i32, px);
        }
    }
}

/// Letter-spaced type from a fixed left edge, the way the site sets its
/// eyebrow lines and logotype. `tracking` is extra px between glyphs.
fn blit_left(
    surf: &mut Surface,
    font: &FontRef,
    text: &str,
    px_size: f32,
    tracking: usize,
    x0: usize,
    y: usize,
) {
    // A space rasterizes to a zero-width mask, so carry the word gap
    // explicitly or tracked lines run their words together.
    let space = (px_size * 0.32) as usize;
    let mut x = x0;
    for c in text.chars() {
        if c == ' ' {
            x += space + tracking;
            continue;
        }
        let g = script::rasterize_line(font, &c.to_string(), px_size);
        for row in 0..g.height {
            for col in 0..g.width {
                if g.mask[row * g.width + col] {
                    surf.put_px((x + col) as i32, (y + row) as i32, BLACK);
                }
            }
        }
        x += g.width + tracking;
    }
}

/// Replace the page with the full-screen sleep card; returns the saved page
/// pixels so waking can restore them exactly.
///
/// Merge Combinator's identity, set for e-ink: the wordmark's weight contrast
/// ("Merge" light over "COMBINATOR" heavy), the letter-spaced mission lines,
/// hairline rules, and the brand mark as real grayscale art.
pub fn show_sleep(surf: &mut Surface, font: &FontRef, ui_font: &FontRef) -> Vec<u8> {
    let saved = surf.copy_rect(0, 0, SCREEN_W, SCREEN_H);
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);

    // A single hairline border. The old double frame fought the wordmark for
    // attention; the brand leads with type, not chrome.
    frame(surf, 56, 56, SCREEN_W - 112, SCREEN_H - 112, 2);

    // Flush-left, like the site: the wordmark hangs off one margin rather than
    // floating. Everything shares this left edge so the page has one spine.
    let ml = 150usize;
    let rule_w = SCREEN_W - ml * 2;

    // Eyebrow. The composed block runs ~1030px; sitting it slightly above true
    // center leaves the wake line room to breathe without stranding the page.
    let mut y = (SCREEN_H - 1030) / 2 - 60;
    blit_left(surf, ui_font, "JOIN THE MISSION.", 32.0, 9, ml, y);

    y += 62;
    surf.fill_rect(ml, y, rule_w, 1, BLACK);

    // Wordmark: light "Merge" over heavy "COMBINATOR", as on the site. The
    // weight contrast IS the logotype, so keep both on the same left edge.
    y += 82;
    blit_left(surf, ui_font, "Merge", 104.0, 0, ml, y);
    y += 112;
    blit_left(surf, ui_font, "COMBINATOR", 112.0, 3, ml, y);

    // The mark, hung to the right of the wordmark's block, as on the site.
    y += 150;
    if let Some((mw, mh, gray)) = mark_gray() {
        let mx = SCREEN_W.saturating_sub(ml + mw).max(ml);
        if mx + mw <= SCREEN_W && y + mh < SCREEN_H {
            blit_gray(surf, mx, y, mw, mh, &gray);
            y += mh;
        }
    }

    y += 44;
    surf.fill_rect(ml, y, rule_w, 1, BLACK);

    y += 52;
    blit_left(surf, ui_font, "BUILD WHAT WARFIGHTERS NEED.", 32.0, 7, ml, y);

    // The one functional line, in the pad's own hand, sitting on the bottom
    // margin so the page reads as a composed sheet rather than a top-heavy one.
    blit_centered(
        surf,
        font,
        "Press the button to wake it.",
        50.0,
        0,
        SCREEN_W,
        SCREEN_H - 210,
    );
    saved
}

pub fn restore_sleep(surf: &mut Surface, saved: &[u8]) {
    surf.paste_rect(0, 0, SCREEN_W, SCREEN_H, saved);
}

fn frame(surf: &mut Surface, x: usize, y: usize, w: usize, h: usize, t: usize) {
    surf.fill_rect(x, y, w, t, BLACK);
    surf.fill_rect(x, y + h - t, w, t, BLACK);
    surf.fill_rect(x, y, t, h, BLACK);
    surf.fill_rect(x + w - t, y, t, h, BLACK);
}

/// Centered because the sleep card is lapidary text — the one case the UX
/// system allows it. Everything else on the pad is flush-left.
fn blit_centered(surf: &mut Surface, font: &FontRef, text: &str, px_size: f32, panel_x: usize, panel_w: usize, y: usize) {
    let line = script::rasterize_line(font, text, px_size);
    let x = panel_x + panel_w.saturating_sub(line.width) / 2;
    for row in 0..line.height {
        for col in 0..line.width {
            if line.mask[row * line.width + col] {
                surf.put_px((x + col) as i32, (y + row) as i32, BLACK);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_capture_rule_from_ask_arrow() {
        let rule: Vec<(i32, i32, i32)> = (0..=40).map(|i| (100 + i * 10, 300, 3)).collect();
        let mut arrow = rule.clone();
        arrow.extend((1..=8).map(|i| (500 - i * 8, 300 - i * 6, 3)));
        assert!(looks_like_send_rule(&rule, 300));
        assert!(!looks_like_ask_arrow(&rule, 300));
        assert!(looks_like_ask_arrow(&arrow, 300));
    }

    /// The brand mark must reach the panel as continuous tone. The rM2's GC16
    /// waveform resolves 16 gray levels, and the red and blue strokes are only
    /// told apart by landing on different ones — if this collapses to pure
    /// black and white, the two gestures merge into one blob.
    #[test]
    fn brand_mark_keeps_its_grays() {
        let (mw, mh, gray) = mark_gray().expect("mc-mark.png must decode as 8-bit grayscale");
        assert!(mw > 0 && mh > 0);
        assert!(mw <= SCREEN_W, "mark {mw}px is wider than the {SCREEN_W}px panel");

        let mut buckets = std::collections::BTreeSet::new();
        for &g in &gray {
            buckets.insert(g / 16);
        }
        assert!(
            buckets.len() >= 8,
            "mark has only {} distinct ink levels; it has been flattened toward 1-bit",
            buckets.len()
        );

        // Both strokes present: near-black (red) AND a genuine mid-gray (blue).
        assert!(gray.iter().any(|&g| g < 60), "no dark stroke in the mark");
        assert!(
            gray.iter().any(|&g| (90..=190).contains(&g)),
            "no mid-gray stroke — red and blue collapsed onto one ink level"
        );
    }

    #[test]
    fn sleep_page_renders_and_restores() {
        let (w, h) = (SCREEN_W, SCREEN_H);
        let mut buf = vec![0xFFu8; w * h * 4];
        let ptr = buf.as_mut_ptr();
        let mut surf = Surface::new(ptr, buf.len(), w, h, w * 4, crate::surface::PixFmt::Rgb32);
        let font = FontRef::try_from_slice(include_bytes!("../fonts/DancingScript.ttf")).unwrap();
        let ui_font = FontRef::try_from_slice(crate::ui::UI_FONT_TTF).unwrap();

        surf.fill_rect(300, 300, 400, 400, BLACK);
        let before = surf.copy_rect(0, 0, w, h);

        let saved = show_sleep(&mut surf, &font, &ui_font);
        let mut black = 0usize;
        for y in 0..h {
            for x in 0..w {
                if surf.luma(x as i32, y as i32) < 128 {
                    black += 1;
                }
            }
        }
        assert!(black > 10_000, "sleep page looks empty: {black} dark px");

        let out = std::env::temp_dir().join("g-pad-sleep-page.png");
        let mut gray = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                gray[y * w + x] = surf.luma(x as i32, y as i32);
            }
        }
        let file = std::fs::File::create(&out).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&gray).unwrap();
        eprintln!("sleep snapshot: {}", out.display());

        restore_sleep(&mut surf, &saved);
        assert_eq!(before, surf.copy_rect(0, 0, w, h), "sleep restore is not exact");
    }
}
