//! The play pages — where Learn mode stops drilling and starts giggling.
//!
//! Three games, all riding the same anchored-mark vocabulary as the
//! worksheets. Its voice is a sentence in the reply hand:
//!
//! - **Critter**: one picture, two artists. The child doodles, marks DONE,
//!   and the pad takes a drawing turn: the model authors a few pen strokes
//!   (`D x,y x,y …` polylines over the crop it was shown) that ATTACH to the
//!   child's lines and add one small part — a hat, a tail, a balloon — never
//!   a finished picture, because the child always draws next. The pad
//!   guarantees the turn connects (strokes slide onto the ink if the model's
//!   aim drifts) and decorates by itself when a reply carries no strokes.
//! - **Guess**: reverse Pictionary. The child draws; the pad guesses, starting
//!   sensible and getting sillier, then asks for one more detail.
//! - **Story**: a choose-your-own-adventure. Each beat ends in three choice
//!   boxes; a mark in one picks the path. The child's doodles star in it.

use ab_glyph::FontRef;

use crate::fb::{BBox, SCREEN_H, SCREEN_W};
use crate::script;
use crate::surface::{Surface, BLACK, WHITE};

use super::sheet;

const W: usize = SCREEN_W;
const H: usize = SCREEN_H;
const MARGIN: usize = W * 8 / 100;

/// Nanoseconds of the wall clock — entropy enough for a doodle game.
fn now_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(7)
}

/// Which game a play page carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Game {
    /// `round` counts the pad's drawing turns on this shared picture, so a
    /// later instruction can tell the model some strokes are its own.
    Critter { round: u8 },
    Guess,
    Story {
        /// (choice the child made, beat the pad told) — oldest first.
        log: Vec<(String, String)>,
        /// The labels of the three boxes currently on the page.
        choices: Vec<String>,
        /// A choice ticked but not yet answered by the model.
        pending: Option<String>,
    },
}

impl Game {
    pub fn story() -> Self {
        Game::Story { log: Vec::new(), choices: Vec::new(), pending: None }
    }

    /// A fresh critter page: a blank shared picture, no turns taken yet.
    pub fn critter() -> Self {
        Game::Critter { round: 0 }
    }

    /// The rotation NEW deals: critter → guess → story → critter…
    pub fn nth(i: usize) -> Self {
        match i % 3 {
            0 => Game::critter(),
            1 => Game::Guess,
            _ => Game::story(),
        }
    }

    pub fn prompt(&self) -> &'static str {
        match self {
            Game::Critter { round: 0, .. } => "DRAW ANYTHING. I DARE YOU.",
            Game::Critter { .. } => "ADD MORE, THEN MARK DONE",
            Game::Guess => "DRAW SOMETHING. I WILL GUESS IT.",
            Game::Story { log, .. } if log.is_empty() => "DRAW YOUR HERO, THEN MARK DONE",
            Game::Story { .. } => "MARK A BOX TO CHOOSE",
        }
    }

    /// The per-turn instruction that turns the oracle into a playmate.
    /// `choice` is the just-ticked story box, if any.
    pub fn instruction(&self, choice: Option<&str>) -> String {
        let base = "PLAY MODE. You are playing with a child aged four to eight. \
                    Be funny, warm, and gentle — playground silly, never mean or scary. \
                    Never mention pictures, images, or cameras. ";
        match self {
            Game::Critter { round } => {
                format!(
                    "{base}You and the child are drawing ONE picture together, taking \
                     turns with the same pen. The page shows the drawing so far{prior}. \
                     Take your turn: add ONE small simple part that grows the child's \
                     drawing — a hat, a tail, a leg, a balloon on a string, a friend \
                     peeking in. Reply with one caption line, then one to three D lines, \
                     and nothing else. \
                     The caption: at most eight very simple words about the part you add. \
                     Each D line is ONE pen stroke: the letter D, then two to eight \
                     points x,y as whole numbers 0-100 (0,0 top-left, 100,100 \
                     bottom-right of the page); the pad draws straight pen lines between \
                     the points in order. Your first point must sit ON the child's lines \
                     so your part connects to the drawing. Keep your part smaller than \
                     the child's drawing, and NEVER finish or redraw the whole picture — \
                     the child draws next. Example:\n\
                     A tiny hat! So fancy.\n\
                     D 45,30 50,22 55,30\n\
                     D 43,30 57,30",
                    prior = if *round > 0 {
                        " (some of its strokes are yours from your earlier turns)"
                    } else {
                        ""
                    }
                )
            }
            Game::Guess => format!(
                "{base}The child drew something and you must guess what it is. Give two short \
                 guesses in one sentence, first sensible then silly, and end by asking for one \
                 more detail so you can guess again. At most fifteen very simple words."
            ),
            Game::Story { log, .. } => {
                let mut s = format!(
                    "{base}You are telling a choose-your-own-adventure story, one beat at a time. "
                );
                if log.is_empty() {
                    s.push_str(
                        "Start a funny little adventure starring whatever the child drew on the \
                         page — describe it kindly, whatever it looks like. If the page is empty, \
                         invent a silly hero. ",
                    );
                } else if let Some(c) = choice {
                    s.push_str(&format!("The child chose: \"{c}\". Continue from there. "));
                }
                s.push_str(
                    "Write the next beat in at most two short sentences a five-year-old \
                     understands. Then, on separate lines, exactly three choices numbered \
                     1. 2. 3., each at most two words in capital letters.",
                );
                s
            }
        }
    }
}

// ---- the pad's turn: drawing strokes --------------------------------------

/// How much of a turn the pad will honor: enough for a hat or a balloon,
/// never enough to bury the child's picture under a mural.
const MAX_STROKES: usize = 4;
const MAX_POINTS: usize = 16;

/// The model's drawing turn, parsed: pen strokes in the 0–100 space of the
/// picture it saw, plus the caption. No strokes means the model only
/// chatted — the pad then attaches a little decoration by itself, so a turn
/// always lands and the game never stalls.
pub struct DrawTurn {
    /// Polylines in 0–100 page coordinates, each at least two points.
    pub strokes: Vec<Vec<(f32, f32)>>,
    pub caption: String,
}

/// Parse a critter reply. Lines leading with D are pen strokes — their
/// numbers pair up as x,y points — and every other non-empty line joins the
/// caption. A D line short of two points is dropped, not fatal; a DRAW word
/// with no coordinates ("Draw with me!") stays caption.
pub fn parse_draw_turn(reply: &str) -> DrawTurn {
    let mut turn = DrawTurn { strokes: Vec::new(), caption: String::new() };
    let mut caption_lines: Vec<&str> = Vec::new();
    for line in reply.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let lead: String = t
            .split_whitespace()
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect::<String>()
            .to_ascii_uppercase();
        let mut pts: Vec<(f32, f32)> = line_numbers(t)
            .chunks_exact(2)
            .map(|p| (p[0].clamp(0.0, 100.0), p[1].clamp(0.0, 100.0)))
            .collect();
        pts.truncate(MAX_POINTS);
        let stroke = matches!(lead.as_str(), "D" | "DRAW") && pts.len() >= 2;
        if stroke && turn.strokes.len() < MAX_STROKES {
            turn.strokes.push(pts);
        } else if lead != "D" {
            caption_lines.push(t);
        }
    }
    turn.caption = caption_lines.join(" ");
    turn
}

/// Every number in a line, in order, however it was punctuated. A digit
/// glued to a letter is part of a word (BB8, D2), never a coordinate.
fn line_numbers(t: &str) -> Vec<f32> {
    let mut nums: Vec<f32> = Vec::new();
    let mut cur = String::new();
    let mut prev = ' ';
    for c in t.chars().chain(std::iter::once(' ')) {
        let extend = !cur.is_empty() && (c.is_ascii_digit() || c == '.');
        let start = cur.is_empty() && !prev.is_alphabetic() && (c.is_ascii_digit() || c == '-');
        if extend || start {
            cur.push(c);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse::<f32>() {
                if n.is_finite() {
                    nums.push(n);
                }
            }
            cur.clear();
        }
        prev = c;
    }
    nums
}

/// Draw the model's strokes onto the page as real pen lines. `frame` is the
/// rectangle its 0–100 coordinates cover (the crop it was shown); `clip` is
/// the open canvas the pen must stay inside; `ink` is the child's actual pen
/// work. The pad guarantees the turn CONNECTS: if the nearest stroke point
/// still misses the ink, all strokes slide together until it touches — the
/// model's shape survives, its aim is forgiven. The pen matches the child's
/// own width, so the pad's part belongs to the picture. Returns the dirty
/// region.
pub fn play_strokes(
    surf: &mut Surface,
    frame: &BBox,
    clip: &BBox,
    ink: &[Vec<(i32, i32, i32)>],
    strokes: &[Vec<(f32, f32)>],
) -> BBox {
    if frame.is_empty() || clip.is_empty() || strokes.is_empty() {
        return BBox::empty();
    }
    let (fx, fy, fw, fh) = frame.rect();
    let mut lines: Vec<Vec<(i32, i32)>> = strokes
        .iter()
        .map(|s| {
            s.iter()
                .map(|&(nx, ny)| {
                    (fx + (nx / 100.0 * fw as f32) as i32, fy + (ny / 100.0 * fh as f32) as i32)
                })
                .collect()
        })
        .collect();

    let mut radii: Vec<i32> = ink.iter().flatten().map(|&(_, _, r)| r).collect();
    radii.sort_unstable();
    let pen = radii.get(radii.len() / 2).copied().unwrap_or(4).clamp(2, 6);

    // Attachment: the closest (stroke point, ink point) pair decides. Close
    // enough already means the model aimed true; otherwise the whole turn
    // slides as one piece, so a hat stays a hat while it lands on the head.
    let mut best: Option<(i64, (i32, i32), (i32, i32))> = None;
    for l in &lines {
        for &(x, y) in l {
            for st in ink {
                for &(ix, iy, _) in st.iter().step_by(3) {
                    let d = (x - ix) as i64 * (x - ix) as i64 + (y - iy) as i64 * (y - iy) as i64;
                    if best.is_none_or(|(bd, _, _)| d < bd) {
                        best = Some((d, (x, y), (ix, iy)));
                    }
                }
            }
        }
    }
    if let Some((d, (sx, sy), (ix, iy))) = best {
        let touch = (pen as i64) * 3;
        if d > touch * touch {
            let (dx, dy) = (ix - sx, iy - sy);
            for l in &mut lines {
                for p in l.iter_mut() {
                    *p = (p.0 + dx, p.1 + dy);
                }
            }
        }
    }

    let mut dirty = BBox::empty();
    for l in &lines {
        let mut prev: Option<(i32, i32)> = None;
        for &(x, y) in l {
            let x = x.clamp(clip.x0 + pen, (clip.x1 - pen).max(clip.x0 + pen));
            let y = y.clamp(clip.y0 + pen, (clip.y1 - pen).max(clip.y0 + pen));
            if let Some((px, py)) = prev {
                surf.brush_line(px, py, x, y, pen, BLACK);
            }
            dirty.add(x, y, pen);
            prev = Some((x, y));
        }
    }
    dirty
}

/// The pad's own little flourish, for a reply that carried no strokes: a
/// decoration in 0–100 frame coordinates anchored to the top of the child's
/// ink, so `play_strokes` lands it attached. Chance picks a party hat, a
/// balloon on a string, or a little flag.
pub fn deco_strokes(frame: &BBox, ink: &BBox) -> Vec<Vec<(f32, f32)>> {
    if frame.is_empty() {
        return Vec::new();
    }
    let (fx, fy, fw, fh) = frame.rect();
    let (ax, ay) = if ink.is_empty() {
        (50.0, 50.0)
    } else {
        (
            (((ink.x0 + ink.x1) / 2 - fx) as f32 / fw as f32 * 100.0).clamp(0.0, 100.0),
            ((ink.y0 - fy) as f32 / fh as f32 * 100.0).clamp(0.0, 100.0),
        )
    };
    // Unit shapes anchored at (0, 0), growing upward.
    let shapes: [&[&[(f32, f32)]]; 3] = [
        // A party hat sitting right on the doodle's head.
        &[&[(-5.0, 0.0), (0.0, -9.0), (5.0, 0.0)], &[(-6.0, 0.0), (6.0, 0.0)]],
        // A balloon on a string.
        &[
            &[(0.0, 0.0), (0.7, -5.0), (0.0, -9.0)],
            &[
                (0.0, -9.0),
                (-3.0, -11.5),
                (-3.5, -15.5),
                (0.0, -19.0),
                (3.5, -15.5),
                (3.0, -11.5),
                (0.0, -9.0),
            ],
        ],
        // A little flag planted on top.
        &[&[(0.0, 0.0), (0.0, -14.0)], &[(0.0, -14.0), (8.0, -11.5), (0.0, -9.0)]],
    ];
    let shape = shapes[now_seed() as usize % shapes.len()];
    let s = 1.4f32;
    shape
        .iter()
        .map(|line| {
            line.iter()
                .map(|&(ux, uy)| {
                    ((ax + ux * s).clamp(0.0, 100.0), (ay + uy * s).clamp(0.0, 100.0))
                })
                .collect()
        })
        .collect()
}

/// Parse a story reply into (beat text, three choices). Choices are the last
/// lines starting `1.`/`2.`/`3.`; missing ones degrade to safe defaults so a
/// beat can never render without a way onward.
pub fn parse_story(reply: &str) -> (String, Vec<String>) {
    let mut beat = Vec::new();
    let mut choices: Vec<String> = Vec::new();
    for line in reply.lines() {
        let t = line.trim();
        let numbered = t.len() > 2
            && t.starts_with(|c: char| ('1'..='3').contains(&c))
            && (t.as_bytes()[1] == b'.' || t.as_bytes()[1] == b')');
        if numbered {
            let label: String = t[2..].trim().trim_end_matches(['.', '!']).to_string();
            if !label.is_empty() {
                choices.push(shorten(&label, 16));
            }
        } else if !t.is_empty() {
            beat.push(t);
        }
    }
    let defaults = ["GO ON", "LOOK AROUND", "GO HOME"];
    while choices.len() < 3 {
        choices.push(defaults[choices.len()].to_string());
    }
    choices.truncate(3);
    (beat.join(" "), choices)
}

fn shorten(s: &str, max: usize) -> String {
    let mut out = String::new();
    for w in s.split_whitespace() {
        if !out.is_empty() {
            if out.len() + 1 + w.len() > max {
                break;
            }
            out.push(' ');
        } else if w.len() > max {
            out.push_str(&w[..max]);
            break;
        }
        out.push_str(w);
    }
    out.to_uppercase()
}

fn rect_outline(surf: &mut Surface, x: i32, y: i32, w: i32, h: i32, t: i32) {
    let t = t.max(1) as usize;
    let (x, y, w, h) = (x.max(0) as usize, y.max(0) as usize, w.max(0) as usize, h.max(0) as usize);
    surf.fill_rect(x, y, w, t, BLACK);
    surf.fill_rect(x, y + h - t, w, t, BLACK);
    surf.fill_rect(x, y, t, h, BLACK);
    surf.fill_rect(x + w - t, y, t, h, BLACK);
}

// ---- the play sheets -------------------------------------------------------

/// Where story beats write themselves.
pub fn story_text_y() -> i32 {
    (H * 10 / 100) as i32
}

/// Draw a play page and return its hit map. Doodle games get the whole open
/// middle as the answer region; the story page reserves the top for the beat
/// and puts three choice boxes above DONE/NEW.
pub fn draw(surf: &mut Surface, ui_font: &FontRef, game: &Game) -> sheet::HitMap {
    surf.fill_rect(0, 0, W, H, WHITE);
    print(surf, ui_font, "PLAY", 32.0, MARGIN, 40, BLACK);
    print_centered(surf, ui_font, game.prompt(), 46.0, (W / 2) as i32, (H * 6 / 100) as i32);

    let mut map = sheet::draw_footer(surf, ui_font);
    match game {
        Game::Critter { .. } | Game::Guess => {
            let mut answer = BBox::empty();
            answer.add(MARGIN as i32, (H * 12 / 100) as i32, 0);
            answer.add((W - MARGIN) as i32, sheet::feedback_y() - 30, 0);
            map.answer = answer;
        }
        Game::Story { choices, .. } => {
            let mut answer = BBox::empty();
            answer.add(MARGIN as i32, (H * 42 / 100) as i32, 0);
            answer.add((W - MARGIN) as i32, choice_row_y() - 30, 0);
            map.answer = answer;
            map.choices = draw_choices(surf, ui_font, choices);
        }
    }
    map
}

fn choice_row_y() -> i32 {
    sheet::box_top() - (H * 8 / 100) as i32 - 30
}

/// Three boxes across, labeled. Empty labels (first beat not told yet) draw
/// nothing and return no targets.
pub fn draw_choices(surf: &mut Surface, ui_font: &FontRef, labels: &[String]) -> Vec<BBox> {
    if labels.is_empty() {
        return Vec::new();
    }
    let bh = (H * 7 / 100) as i32;
    let gap = 30i32;
    let total_w = (W - 2 * MARGIN) as i32;
    let bw = (total_w - 2 * gap) / 3;
    let y = choice_row_y();
    let mut out = Vec::new();
    for (i, label) in labels.iter().take(3).enumerate() {
        let x = MARGIN as i32 + i as i32 * (bw + gap);
        // Clear underneath so a redraw never shows the last beat's labels.
        surf.fill_rect(x as usize, y as usize, bw as usize, bh as usize, WHITE);
        rect_outline(surf, x, y, bw, bh, 4);
        let px = fit_px(ui_font, label, 38.0, (bw - 30) as f32);
        let raster = script::rasterize_line(ui_font, label, px);
        let (tx, ty) = (x + (bw - raster.width as i32) / 2, y + (bh - raster.height as i32) / 2);
        for row in 0..raster.height {
            for col in 0..raster.width {
                if raster.mask[row * raster.width + col] {
                    surf.put_px(tx + col as i32, ty + row as i32, BLACK);
                }
            }
        }
        let mut b = BBox::empty();
        b.add(x, y, 0);
        b.add(x + bw, y + bh, 0);
        out.push(b);
    }
    out
}

fn fit_px(font: &FontRef, text: &str, px: f32, max_w: f32) -> f32 {
    let w = script::measure(font, text, px);
    if w > max_w {
        px * max_w / w
    } else {
        px
    }
}

fn print(surf: &mut Surface, font: &FontRef, text: &str, px: f32, x: usize, y: usize, _c: u16) {
    let raster = script::rasterize_line(font, text, px);
    for row in 0..raster.height {
        for col in 0..raster.width {
            if raster.mask[row * raster.width + col] {
                surf.put_px((x + col) as i32, (y + row) as i32, BLACK);
            }
        }
    }
}

fn print_centered(surf: &mut Surface, font: &FontRef, text: &str, px: f32, cx: i32, y: i32) {
    let raster = script::rasterize_line(font, text, px);
    let x = (cx - raster.width as i32 / 2).max(0);
    for row in 0..raster.height {
        for col in 0..raster.width {
            if raster.mask[row * raster.width + col] {
                surf.put_px(x + col as i32, y + row as i32, BLACK);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// A child-ish squiggle to draw against: a horizontal pen line.
    fn child_line(x0: i32, x1: i32, y: i32) -> Vec<Vec<(i32, i32, i32)>> {
        vec![(x0..=x1).step_by(4).map(|x| (x, y, 4)).collect()]
    }

    #[test]
    fn draw_replies_parse_into_strokes_and_caption() {
        let turn = parse_draw_turn("A tiny hat! So fancy.\nD 45,30 50,22 55,30\nD 43,30 57,30");
        assert_eq!(turn.caption, "A tiny hat! So fancy.");
        assert_eq!(turn.strokes.len(), 2);
        assert_eq!(turn.strokes[0], vec![(45.0, 30.0), (50.0, 22.0), (55.0, 30.0)]);
        assert_eq!(turn.strokes[1], vec![(43.0, 30.0), (57.0, 30.0)]);

        // Punctuation, case, and chatter are tamed, not fatal.
        let turn = parse_draw_turn("Wow!\ndraw (20, 60) (25, 50) please\nd: 10,10 12,12");
        assert_eq!(turn.strokes.len(), 2);
        assert_eq!(turn.strokes[0][0], (20.0, 60.0));

        // Out-of-range points are clamped to the page.
        let turn = parse_draw_turn("Zoom!\nD 150,-20 50,50");
        assert_eq!(turn.strokes[0], vec![(100.0, 0.0), (50.0, 50.0)]);

        // A D line short of two points is dropped; a wordy DRAW stays caption.
        let turn = parse_draw_turn("Draw with me!\nD 30,30");
        assert!(turn.strokes.is_empty());
        assert_eq!(turn.caption, "Draw with me!");

        // A mural is trimmed to a turn: stroke and point caps hold.
        let long = format!("Hi!\n{}", "D 1,1 2,2 3,3\n".repeat(9));
        assert_eq!(parse_draw_turn(&long).strokes.len(), MAX_STROKES);
        let dense: String = (0..40).map(|i| format!(" {i},{i}")).collect();
        let turn = parse_draw_turn(&format!("Hi!\nD{dense}"));
        assert_eq!(turn.strokes[0].len(), MAX_POINTS);
    }

    #[test]
    fn played_strokes_land_as_pen_ink_inside_the_canvas() {
        let (_buf, mut surf) = page();
        let frame = BBox { x0: 112, y0: 224, x1: 1292, y1: 1548 };
        let ink = child_line(400, 700, 800);
        // A stroke aimed straight across the child's line.
        let strokes = vec![vec![(20.0, 40.0), (40.0, 40.0)]];
        let dirty = play_strokes(&mut surf, &frame, &frame, &ink, &strokes);
        assert!(!dirty.is_empty());
        assert!(dirty.x0 >= frame.x0 - 8 && dirty.x1 <= frame.x1 + 8);
        assert!(dirty.y0 >= frame.y0 - 8 && dirty.y1 <= frame.y1 + 8);
        // Real pen ink landed somewhere in the dirty region.
        let mut black = 0;
        for y in dirty.y0..dirty.y1 {
            for x in dirty.x0..dirty.x1 {
                if surf.luma(x, y) < 32 {
                    black += 1;
                }
            }
        }
        assert!(black > 50, "only {black} inked pixels landed");
    }

    #[test]
    fn a_turn_that_misses_the_ink_slides_until_it_touches() {
        let (_buf, mut surf) = page();
        let frame = BBox { x0: 0, y0: 0, x1: 1000, y1: 1000 };
        // The child's line lives at y=800; the model draws far away, top-left.
        let ink = child_line(400, 700, 800);
        let strokes = vec![vec![(5.0, 5.0), (10.0, 5.0)], vec![(5.0, 5.0), (5.0, 10.0)]];
        let dirty = play_strokes(&mut surf, &frame, &frame, &ink, &strokes);
        // The whole turn slid onto the ink: its dirty box touches the line.
        assert!(
            dirty.x1 >= 390 && dirty.x0 <= 710 && dirty.y1 >= 780 && dirty.y0 <= 820,
            "turn landed at {:?}, not attached to the ink at y=800",
            dirty.rect()
        );
    }

    #[test]
    fn a_turn_aimed_true_stays_where_the_model_put_it() {
        let (_buf, mut surf) = page();
        let frame = BBox { x0: 0, y0: 0, x1: 1000, y1: 1000 };
        let ink = child_line(400, 700, 800);
        // First point right on the line: no sliding.
        let strokes = vec![vec![(50.0, 80.0), (55.0, 70.0), (60.0, 80.0)]];
        let dirty = play_strokes(&mut surf, &frame, &frame, &ink, &strokes);
        assert!(dirty.y0 <= 710 && dirty.y1 >= 790, "stroke moved: {:?}", dirty.rect());
    }

    #[test]
    fn a_chatty_reply_still_gets_a_decoration_attached() {
        let (_buf, mut surf) = page();
        let frame = BBox { x0: 0, y0: 0, x1: 1000, y1: 1000 };
        let mut ink_box = BBox::empty();
        ink_box.add(400, 500, 0);
        ink_box.add(700, 800, 0);
        let deco = deco_strokes(&frame, &ink_box);
        assert!(!deco.is_empty(), "the pad must always have a flourish");
        for s in &deco {
            assert!(s.len() >= 2);
        }
        let ink = child_line(400, 700, 500);
        let dirty = play_strokes(&mut surf, &frame, &frame, &ink, &deco);
        // The decoration lands touching the top of the doodle.
        assert!(!dirty.is_empty());
        assert!(dirty.y0 < 500 && dirty.y1 >= 480, "deco at {:?} not on top", dirty.rect());
    }

    #[test]
    fn an_unsent_frame_plays_no_strokes() {
        let (_buf, mut surf) = page();
        let strokes = vec![vec![(10.0, 10.0), (20.0, 20.0)]];
        let dirty = play_strokes(&mut surf, &BBox::empty(), &BBox::empty(), &[], &strokes);
        assert!(dirty.is_empty());
        assert!(deco_strokes(&BBox::empty(), &BBox::empty()).is_empty());
    }

    #[test]
    fn play_pages_declare_canvas_and_story_choice_targets() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let critter = Game::Critter { round: 0 };
        let map = draw(&mut surf, &font, &critter);
        assert!(!map.answer.is_empty());
        assert!(map.choices.is_empty());
        assert!(map.answer.y1 < map.done.y0);

        let story = Game::Story {
            log: vec![("".into(), "beat".into())],
            choices: vec!["RUN".into(), "HIDE".into(), "SING".into()],
            pending: None,
        };
        let map = draw(&mut surf, &font, &story);
        assert_eq!(map.choices.len(), 3);
        for c in &map.choices {
            assert!(c.y1 < map.done.y0, "choices must sit above the decision boxes");
            assert!(c.y0 > map.answer.y1, "choices must not overlap the doodle area");
        }
        // Choice boxes are distinct targets.
        assert!(map.choices[0].x1 < map.choices[1].x0);
        assert!(map.choices[1].x1 < map.choices[2].x0);
    }

    #[test]
    fn the_game_rotation_cycles_all_three() {
        assert!(matches!(Game::nth(0), Game::Critter { round: 0 }));
        assert_eq!(Game::nth(1), Game::Guess);
        assert!(matches!(Game::nth(2), Game::Story { .. }));
        assert!(matches!(Game::nth(3), Game::Critter { round: 0 }));
    }

    #[test]
    fn instructions_carry_the_protocol_each_parser_expects() {
        let c = Game::Critter { round: 0 }.instruction(None);
        assert!(c.contains("D ") && c.contains("0-100"), "critter must teach the stroke protocol");
        assert!(c.contains("NEVER finish"), "the turn must stay a turn, not a finished picture");
        assert!(c.contains("ON the child's lines"), "strokes must be told to connect");
        let later = Game::Critter { round: 2 }.instruction(None);
        assert!(later.contains("earlier turns"), "later rounds must own the pad's strokes");
        let s = Game::story().instruction(None);
        assert!(s.contains("1. 2. 3."));
        let mid = Game::Story { log: vec![("RUN".into(), "b".into())], choices: vec![], pending: None };
        assert!(mid.instruction(Some("SING LOUDLY")).contains("SING LOUDLY"));
    }
}
