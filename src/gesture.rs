//! Pen-gesture recognition, and the sleep card.
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

/// Replace the page with the full-screen sleep card; returns the saved page
/// pixels so waking can restore them exactly.
pub fn show_sleep(surf: &mut Surface, font: &FontRef) -> Vec<u8> {
    let saved = surf.copy_rect(0, 0, SCREEN_W, SCREEN_H);
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    frame(surf, 48, 48, SCREEN_W - 96, SCREEN_H - 96, 4);
    frame(surf, 66, 66, SCREEN_W - 132, SCREEN_H - 132, 1);
    let y = SCREEN_H * 38 / 100;
    blit_centered(surf, font, "The pad sleeps.", 116.0, 0, SCREEN_W, y);
    blit_centered(surf, font, "Press the button to wake it.", 56.0, 0, SCREEN_W, y + 230);
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

    fn stroke(pts: &[(i32, i32)]) -> Vec<(i32, i32, i32)> {
        pts.iter().map(|&(x, y)| (x, y, 3)).collect()
    }

    #[test]
    fn distinguishes_capture_rule_from_ask_arrow() {
        let rule: Vec<(i32, i32, i32)> = (0..=40).map(|i| (100 + i * 10, 300, 3)).collect();
        let mut arrow = rule.clone();
        arrow.extend((1..=8).map(|i| (500 - i * 8, 300 - i * 6, 3)));
        assert!(looks_like_send_rule(&rule, 300));
        assert!(!looks_like_ask_arrow(&rule, 300));
        assert!(looks_like_ask_arrow(&arrow, 300));
    }

    #[test]
    #[test]
    fn sleep_page_renders_and_restores() {
        let (w, h) = (SCREEN_W, SCREEN_H);
        let mut buf = vec![0xFFu8; w * h * 4];
        let ptr = buf.as_mut_ptr();
        let mut surf = Surface::new(ptr, buf.len(), w, h, w * 4, crate::surface::PixFmt::Rgb32);
        let font = FontRef::try_from_slice(include_bytes!("../fonts/DancingScript.ttf")).unwrap();

        surf.fill_rect(300, 300, 400, 400, BLACK);
        let before = surf.copy_rect(0, 0, w, h);

        let saved = show_sleep(&mut surf, &font);
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
