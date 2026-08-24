//! The pad's hand: rasterize reply text in Dancing Script, thin it to
//! single-pixel pen paths (Zhang-Suen), trace them into ordered strokes, and
//! yield them for stroke-by-stroke animation.

use ab_glyph::{Font, FontRef, Glyph, PxScale, ScaleFont};

pub struct Line {
    pub width: usize,
    pub height: usize,
    /// Bit mask of inked pixels, row-major.
    pub mask: Vec<bool>,
}

/// Rasterize one line of text at `px` height into a boolean mask.
pub fn rasterize_line(font: &FontRef, text: &str, px: f32) -> Line {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut caret = 0.0f32;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            caret += scaled.kern(p, id);
        }
        let mut g = id.with_scale(PxScale::from(px));
        g.position = ab_glyph::point(caret, scaled.ascent());
        caret += scaled.h_advance(id);
        glyphs.push(g);
        prev = Some(id);
    }
    let width = (caret.ceil() as usize + 4).max(1);
    let height = (scaled.height().ceil() as usize + 4).max(1);
    let mut mask = vec![false; width * height];
    for g in glyphs {
        if let Some(outline) = font.outline_glyph(g) {
            let bounds = outline.px_bounds();
            outline.draw(|x, y, cov| {
                if cov > 0.5 {
                    let px_x = bounds.min.x as i32 + x as i32;
                    let px_y = bounds.min.y as i32 + y as i32;
                    if px_x >= 0 && px_y >= 0 && (px_x as usize) < width && (px_y as usize) < height {
                        mask[px_y as usize * width + px_x as usize] = true;
                    }
                }
            });
        }
    }
    Line { width, height, mask }
}

/// Measure the advance width of text at `px` without rasterizing.
pub fn measure(font: &FontRef, text: &str, px: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut caret = 0.0f32;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            caret += scaled.kern(p, id);
        }
        caret += scaled.h_advance(id);
        prev = Some(id);
    }
    caret
}

/// Zhang-Suen thinning: reduce the mask to 1px-wide skeleton lines.
pub fn thin(line: &mut Line) {
    let (w, h) = (line.width, line.height);
    let idx = |x: usize, y: usize| y * w + x;
    loop {
        let mut changed = false;
        for phase in 0..2 {
            let mut to_clear = Vec::new();
            for y in 1..h.saturating_sub(1) {
                for x in 1..w.saturating_sub(1) {
                    if !line.mask[idx(x, y)] {
                        continue;
                    }
                    let p = [
                        line.mask[idx(x, y - 1)],     // p2 N
                        line.mask[idx(x + 1, y - 1)], // p3 NE
                        line.mask[idx(x + 1, y)],     // p4 E
                        line.mask[idx(x + 1, y + 1)], // p5 SE
                        line.mask[idx(x, y + 1)],     // p6 S
                        line.mask[idx(x - 1, y + 1)], // p7 SW
                        line.mask[idx(x - 1, y)],     // p8 W
                        line.mask[idx(x - 1, y - 1)], // p9 NW
                    ];
                    let b: u32 = p.iter().map(|&v| v as u32).sum();
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let mut a = 0;
                    for i in 0..8 {
                        if !p[i] && p[(i + 1) % 8] {
                            a += 1;
                        }
                    }
                    if a != 1 {
                        continue;
                    }
                    let (c1, c2) = if phase == 0 {
                        (!(p[0] && p[2] && p[4]), !(p[2] && p[4] && p[6]))
                    } else {
                        (!(p[0] && p[2] && p[6]), !(p[0] && p[4] && p[6]))
                    };
                    if c1 && c2 {
                        to_clear.push(idx(x, y));
                    }
                }
            }
            if !to_clear.is_empty() {
                changed = true;
                for i in to_clear {
                    line.mask[i] = false;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Trace the skeleton into polyline strokes, ordered left-to-right so the
/// animation writes like a hand.
pub fn trace(line: &Line) -> Vec<Vec<(i32, i32)>> {
    let (w, h) = (line.width, line.height);
    let at = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && line.mask[y as usize * w + x as usize]
    };
    let neighbors = |x: i32, y: i32| -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                if (dx != 0 || dy != 0) && at(x + dx, y + dy) {
                    out.push((x + dx, y + dy));
                }
            }
        }
        out
    };

    let mut visited = vec![false; w * h];
    let vis = |v: &mut Vec<bool>, x: i32, y: i32| {
        v[y as usize * w + x as usize] = true;
    };
    let seen = |v: &Vec<bool>, x: i32, y: i32| -> bool { v[y as usize * w + x as usize] };

    // Endpoints first (degree 1), then any remaining pixels (loops).
    let mut starts: Vec<(i32, i32)> = Vec::new();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if at(x, y) && neighbors(x, y).len() == 1 {
                starts.push((x, y));
            }
        }
    }
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if at(x, y) {
                starts.push((x, y));
            }
        }
    }

    let mut strokes: Vec<Vec<(i32, i32)>> = Vec::new();
    for (sx, sy) in starts {
        if seen(&visited, sx, sy) {
            continue;
        }
        let mut path = vec![(sx, sy)];
        vis(&mut visited, sx, sy);
        let (mut cx, mut cy) = (sx, sy);
        loop {
            let next = neighbors(cx, cy)
                .into_iter()
                .find(|&(nx, ny)| !seen(&visited, nx, ny));
            match next {
                Some((nx, ny)) => {
                    vis(&mut visited, nx, ny);
                    path.push((nx, ny));
                    cx = nx;
                    cy = ny;
                }
                None => break,
            }
        }
        if path.len() >= 3 {
            strokes.push(path);
        }
    }
    strokes.sort_by_key(|s| s.iter().map(|&(x, _)| x).min().unwrap_or(0));
    strokes
}

/// Word-wrap `text` to lines that fit `max_px` at scale `px`.
pub fn wrap(font: &FontRef, text: &str, px: f32, max_px: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.lines() {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if measure(font, &cand, px) <= max_px || cur.is_empty() {
                cur = cand;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_produces_strokes() {
        let font = FontRef::try_from_slice(include_bytes!("../fonts/DancingScript.ttf")).unwrap();
        let mut line = rasterize_line(&font, "Yes, Harry?", 96.0);
        assert!(line.width > 100 && line.height > 50);
        let inked_before: usize = line.mask.iter().filter(|&&v| v).count();
        thin(&mut line);
        let inked_after: usize = line.mask.iter().filter(|&&v| v).count();
        assert!(inked_after * 3 < inked_before, "thinning should slim the glyphs: {inked_before} -> {inked_after}");
        let strokes = trace(&line);
        assert!(!strokes.is_empty());
        let total: usize = strokes.iter().map(|s| s.len()).sum();
        println!("strokes={} total_points={} ({}x{})", strokes.len(), total, line.width, line.height);
        assert!(total > 200, "expected a decent path length, got {total}");
        // Wrap sanity.
        let lines = wrap(&font, "Do you know anything about the Chamber of Secrets?", 96.0, 1380.0);
        assert!(lines.len() >= 2);
    }
}
