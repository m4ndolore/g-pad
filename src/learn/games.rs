//! The play pages — where Learn mode stops drilling and starts giggling.
//!
//! Three games, all riding the same anchored-mark vocabulary as the
//! worksheets, and all shaped by one hardware truth: the pad cannot fetch
//! pictures, but it can draw geometry and it can write. So the pad's "turns"
//! are decorations built from primitives and anchored to the child's own
//! ink, and its voice is a sentence in the reply hand:
//!
//! - **Critter**: the child doodles, marks DONE, and the pad adds exactly one
//!   decoration (googly eyes, stick legs, a party hat…) and names the
//!   creature. Turn-taking drawing where the pad's wit carries the humor and
//!   its geometry stays honest.
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

/// Which game a play page carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Game {
    /// `round` counts the pad's decorating turns on this doodle.
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

    /// The rotation NEW deals: critter → guess → story → critter…
    pub fn nth(i: usize) -> Self {
        match i % 3 {
            0 => Game::Critter { round: 0 },
            1 => Game::Guess,
            _ => Game::story(),
        }
    }

    pub fn prompt(&self) -> &'static str {
        match self {
            Game::Critter { round: 0 } => "DRAW ANYTHING. I DARE YOU.",
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
            Game::Critter { round } => format!(
                "{base}The page shows the child's doodle{prior}. Take a REAL drawing turn: \
                 add one generous new thing that makes it funnier — big wings, a tiny friend, \
                 a wobbly house, a sun with a face, a skateboard, whatever fits the doodle. \
                 Reply with EXACTLY this shape: \
                 first line: one funny caption of at most eight very simple words that names \
                 the creature or reacts to it. \
                 Then 4 to 10 more lines, each one pen stroke: the letter D, a space, then \
                 2 to 12 points as x,y pairs separated by spaces, like `D 10,80 30,60 50,80`. \
                 Coordinates run 0-100 across the whole picture: 0,0 is top-left, 100,100 \
                 bottom-right. Strokes are drawn in your order with a child's marker pen. \
                 Draw BIG — your addition should be about as large as the doodle itself — \
                 and make it touch the doodle so it belongs to it. Surprise the child: \
                 not just a hat every time. Write nothing else.",
                prior = if *round > 0 { " (some parts you added on earlier turns)" } else { "" }
            ),
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

// ---- the pad's turn: decorations ------------------------------------------

/// The decoration menu. Every entry is drawable geometry anchored to a bbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Deco {
    Eyes,
    Legs,
    Arms,
    Hat,
    Antenna,
    Tail,
    Rays,
    Crown,
    Mustache,
    Bubble(String),
}

/// The pad's drawing turn, parsed: strokes the model drew itself (in 0–100
/// picture coordinates), or a menu decoration when it fell back to the old
/// vocabulary, and the caption either way.
pub struct CritterTurn {
    /// Model-drawn pen strokes: polylines in 0–100 of the picture it saw.
    pub strokes: Vec<Vec<(f32, f32)>>,
    /// The menu fallback, when the reply led with a decoration word instead.
    pub deco: Option<Deco>,
    pub caption: String,
}

/// Parse a critter reply. `D x,y x,y …` lines are the model's own pen
/// strokes; every other non-empty line joins the caption. A reply with no
/// stroke lines degrades to the old menu-word protocol, and an unknown lead
/// means caption only — the game never stalls on a chatty model.
pub fn parse_critter_turn(reply: &str) -> CritterTurn {
    let mut strokes: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut caption_lines: Vec<&str> = Vec::new();
    for line in reply.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let is_stroke = (t.starts_with("D ") || t.starts_with("d "))
            && t[2..].trim_start().starts_with(|c: char| c.is_ascii_digit());
        if !is_stroke {
            caption_lines.push(t);
            continue;
        }
        let mut pts: Vec<(f32, f32)> = Vec::new();
        for pair in t[2..].split_whitespace() {
            let Some((xs, ys)) = pair.split_once(',') else { continue };
            let (Ok(x), Ok(y)) = (xs.trim().parse::<f32>(), ys.trim().parse::<f32>()) else {
                continue;
            };
            if x.is_finite() && y.is_finite() {
                pts.push((x.clamp(0.0, 100.0), y.clamp(0.0, 100.0)));
            }
            if pts.len() >= 12 {
                break;
            }
        }
        if pts.len() >= 2 {
            strokes.push(pts);
        }
    }
    strokes.truncate(10);
    if !strokes.is_empty() {
        return CritterTurn { strokes, deco: None, caption: caption_lines.join(" ") };
    }
    let (deco, caption) = parse_critter(reply);
    CritterTurn { strokes: Vec::new(), deco, caption }
}

/// Parse a critter reply: leading menu word (BUBBLE takes the next word as
/// its text), rest is the caption. An unknown lead means no decoration —
/// the caption still lands, the game never stalls on a chatty model.
pub fn parse_critter(reply: &str) -> (Option<Deco>, String) {
    let trimmed = reply.trim();
    let mut words = trimmed.split_whitespace();
    let Some(first) = words.next() else { return (None, String::new()) };
    let key: String = first
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    let strip = |n: usize| -> String {
        trimmed
            .split_whitespace()
            .skip(n)
            .collect::<Vec<_>>()
            .join(" ")
            .trim_start_matches(|c: char| matches!(c, '.' | ',' | ':' | ';' | '!' | '-' | '—'))
            .trim()
            .to_string()
    };
    let deco = match key.as_str() {
        "EYES" => Some(Deco::Eyes),
        "LEGS" => Some(Deco::Legs),
        "ARMS" => Some(Deco::Arms),
        "HAT" => Some(Deco::Hat),
        "ANTENNA" => Some(Deco::Antenna),
        "TAIL" => Some(Deco::Tail),
        "RAYS" => Some(Deco::Rays),
        "CROWN" => Some(Deco::Crown),
        "MUSTACHE" => Some(Deco::Mustache),
        "BUBBLE" => {
            let word: String = words
                .next()
                .unwrap_or("WOW")
                .chars()
                .filter(|c| c.is_alphanumeric())
                .take(8)
                .collect::<String>()
                .to_ascii_uppercase();
            return (Some(Deco::Bubble(word)), strip(2));
        }
        _ => return (None, trimmed.to_string()),
    };
    (deco, strip(1))
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

// ---- drawing the pad's turn -----------------------------------------------

/// Draw `deco` anchored to the child's ink bbox. Returns the dirty region.
/// Everything is brush strokes, so the pad's additions look penned, not
/// printed — it is taking a turn, not stamping a sticker.
pub fn draw_deco(surf: &mut Surface, ink: &BBox, deco: &Deco, ui_font: &FontRef) -> BBox {
    if ink.is_empty() {
        return BBox::empty();
    }
    let (bx, by, bw, bh) = ink.rect();
    let (cx, top, bottom) = (bx + bw / 2, by, by + bh);
    let s = bw.max(bh).clamp(120, 700);
    let r = 4;
    let mut dirty = BBox::empty();
    let mark = |d: &mut BBox, x: i32, y: i32| d.add(x, y, 24);

    match deco {
        Deco::Eyes => {
            let er = (s * 8 / 100).max(16);
            for dx in [-er * 2, er * 2] {
                let (ex, ey) = (cx + dx, top + bh / 4);
                // A white pad under each eye so it sits ON the doodle.
                surf.stamp(ex, ey, er, WHITE);
                ring(surf, ex, ey, er, 3);
                surf.stamp(ex + er / 3, ey + er / 4, er / 3, BLACK); // pupil, slightly unhinged
                mark(&mut dirty, ex, ey);
            }
        }
        Deco::Legs => {
            let len = (s / 4).max(60);
            for dx in [-bw / 4, bw / 4] {
                let x = cx + dx;
                surf.brush_line(x, bottom, x, bottom + len, r, BLACK);
                surf.brush_line(x, bottom + len, x + 30, bottom + len, r, BLACK); // a foot
                mark(&mut dirty, x, bottom + len);
            }
            mark(&mut dirty, cx, bottom);
        }
        Deco::Arms => {
            let len = (s / 4).max(60);
            let y = top + bh / 2;
            surf.brush_line(bx, y, bx - len, y - len, r, BLACK);
            surf.brush_line(bx + bw, y, bx + bw + len, y - len, r, BLACK);
            mark(&mut dirty, bx - len, y - len);
            mark(&mut dirty, bx + bw + len, y - len);
        }
        Deco::Hat => {
            // A party cone with a pompom: never not funny.
            let hw = (s / 5).max(50);
            let hh = (s / 3).max(70);
            surf.brush_line(cx - hw, top, cx, top - hh, r, BLACK);
            surf.brush_line(cx + hw, top, cx, top - hh, r, BLACK);
            surf.brush_line(cx - hw, top, cx + hw, top, r, BLACK);
            surf.stamp(cx, top - hh, 12, BLACK);
            mark(&mut dirty, cx, top - hh);
            mark(&mut dirty, cx - hw, top);
            mark(&mut dirty, cx + hw, top);
        }
        Deco::Antenna => {
            let len = (s / 3).max(80);
            for dx in [-bw / 5, bw / 5] {
                let (x0, x1) = (cx + dx, cx + dx * 2);
                surf.brush_line(x0, top, x1, top - len, 3, BLACK);
                ring(surf, x1, top - len, 14, 4);
                mark(&mut dirty, x1, top - len);
            }
            mark(&mut dirty, cx, top);
        }
        Deco::Tail => {
            // A curly pig tail off the right side.
            let (ox, oy) = (bx + bw, top + bh / 2);
            let mut prev = (ox, oy);
            for i in 1..=40 {
                let t = i as f32 * 0.35;
                let rad = 8.0 + t * 4.0;
                let x = ox + (t * 9.0) as i32 + (rad * t.cos()) as i32;
                let y = oy + (rad * t.sin()) as i32;
                surf.brush_line(prev.0, prev.1, x, y, 3, BLACK);
                prev = (x, y);
                mark(&mut dirty, x, y);
            }
        }
        Deco::Rays => {
            let cr = (bw.max(bh) / 2) + 30;
            let (ccx, ccy) = (cx, top + bh / 2);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (dx, dy) = (a.cos(), a.sin());
                let (x0, y0) = (ccx + (dx * cr as f32) as i32, ccy + (dy * cr as f32) as i32);
                let (x1, y1) = (ccx + (dx * (cr + 70) as f32) as i32, ccy + (dy * (cr + 70) as f32) as i32);
                surf.brush_line(x0, y0, x1, y1, r, BLACK);
                mark(&mut dirty, x1, y1);
            }
        }
        Deco::Crown => {
            let hw = (bw / 2).min(s / 2).max(60);
            let hh = (s / 4).max(50);
            let base = top - 10;
            let mut prev = (cx - hw, base);
            for i in 0..=6 {
                let x = cx - hw + hw * i / 3;
                let y = if i % 2 == 1 { base - hh } else { base };
                surf.brush_line(prev.0, prev.1, x, y, r, BLACK);
                prev = (x, y);
            }
            surf.brush_line(cx - hw, base, cx + hw, base, r, BLACK);
            mark(&mut dirty, cx - hw, base - hh);
            mark(&mut dirty, cx + hw, base);
        }
        Deco::Mustache => {
            // Two proud curls just below center.
            let y = top + bh * 3 / 5;
            for side in [-1i32, 1] {
                let mut prev = (cx, y);
                for i in 1..=24 {
                    let t = i as f32 / 24.0;
                    let x = cx + side * (t * (s as f32 / 4.0)) as i32;
                    let yy = y - ((t * 3.0).sin() * 24.0 * t) as i32;
                    surf.brush_line(prev.0, prev.1, x, yy, r, BLACK);
                    prev = (x, yy);
                    mark(&mut dirty, x, yy);
                }
            }
        }
        Deco::Bubble(word) => {
            let raster = script::rasterize_line(ui_font, word, 44.0);
            let (tw, th) = (raster.width as i32, raster.height as i32);
            let (bw2, bh2) = (tw + 60, th + 30);
            // Top-right of the doodle, pulled back on-screen if needed.
            let x = (bx + bw - 20).min(W as i32 - bw2 - 20).max(20);
            let y = (top - bh2 - 60).max(20);
            surf.fill_rect(x as usize, y as usize, bw2 as usize, bh2 as usize, WHITE);
            rect_outline(surf, x, y, bw2, bh2, 4);
            for row in 0..raster.height {
                for col in 0..raster.width {
                    if raster.mask[row * raster.width + col] {
                        surf.put_px(x + 30 + col as i32, y + 15 + row as i32, BLACK);
                    }
                }
            }
            // The tail points at the creature's face, not through its hat.
            let (fx, fy) = (bx + bw * 3 / 4, top + bh / 4);
            surf.brush_line(x + bw2 / 4, y + bh2, fx, fy, 3, BLACK);
            mark(&mut dirty, x, y);
            mark(&mut dirty, x + bw2, y + bh2);
            mark(&mut dirty, fx, fy);
        }
    }
    dirty
}

/// Draw the model's own pen strokes. `frame` is the page rectangle its 0–100
/// coordinates cover (the crop that was sent as the picture); `clip` keeps
/// every point on the open canvas so a wild stroke never scribbles over the
/// decision boxes or the caption. Returns the dirty region.
pub fn draw_strokes(surf: &mut Surface, frame: &BBox, clip: &BBox, strokes: &[Vec<(f32, f32)>]) -> BBox {
    if frame.is_empty() || clip.is_empty() {
        return BBox::empty();
    }
    let (fx, fy, fw, fh) = frame.rect();
    let mut dirty = BBox::empty();
    for stroke in strokes {
        let mut prev: Option<(i32, i32)> = None;
        for &(nx, ny) in stroke {
            let x = (fx + (nx / 100.0 * fw as f32) as i32).clamp(clip.x0, clip.x1);
            let y = (fy + (ny / 100.0 * fh as f32) as i32).clamp(clip.y0, clip.y1);
            if let Some((px, py)) = prev {
                surf.brush_line(px, py, x, y, 4, BLACK);
            }
            dirty.add(x, y, 8);
            prev = Some((x, y));
        }
    }
    dirty
}

fn ring(surf: &mut Surface, cx: i32, cy: i32, radius: i32, thick: i32) {
    let (lo2, hi2) = ((radius - thick) * (radius - thick), radius * radius);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let d2 = dx * dx + dy * dy;
            if d2 >= lo2 && d2 <= hi2 {
                surf.put_px(cx + dx, cy + dy, BLACK);
            }
        }
    }
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

    #[test]
    fn critter_replies_parse_into_decoration_and_caption() {
        let (d, cap) = parse_critter("EYES! His name is Kevin and he is late for soup.");
        assert_eq!(d, Some(Deco::Eyes));
        assert_eq!(cap, "His name is Kevin and he is late for soup.");

        let (d, cap) = parse_critter("BUBBLE moo — this cow has opinions.");
        assert_eq!(d, Some(Deco::Bubble("MOO".into())));
        assert!(cap.starts_with("this cow"));

        // A chatty model that skips the menu still lands its caption.
        let (d, cap) = parse_critter("What a magnificent potato!");
        assert_eq!(d, None);
        assert_eq!(cap, "What a magnificent potato!");
    }

    #[test]
    fn a_drawing_turn_parses_strokes_and_caption() {
        let reply = "Sir Wigglebottom grew wings!\nD 10,80 30,60 50,80\nD 50,80 70,60 90,80\nD 45,20 55,20";
        let turn = parse_critter_turn(reply);
        assert_eq!(turn.strokes.len(), 3);
        assert_eq!(turn.strokes[0], vec![(10.0, 80.0), (30.0, 60.0), (50.0, 80.0)]);
        assert_eq!(turn.caption, "Sir Wigglebottom grew wings!");
        assert!(turn.deco.is_none(), "strokes win over the menu");

        // Out-of-range and junk points are tamed, not fatal.
        let turn = parse_critter_turn("Wow\nD 150,-20 50,50\nD nonsense\nD 1,1");
        assert_eq!(turn.strokes.len(), 1);
        assert_eq!(turn.strokes[0][0], (100.0, 0.0));

        // No stroke lines: the old menu protocol still works.
        let turn = parse_critter_turn("EYES! His name is Kevin.");
        assert!(turn.strokes.is_empty());
        assert_eq!(turn.deco, Some(Deco::Eyes));
        assert_eq!(turn.caption, "His name is Kevin.");

        // A chatty model with neither still lands its caption.
        let turn = parse_critter_turn("What a magnificent potato!");
        assert!(turn.strokes.is_empty() && turn.deco.is_none());
        assert_eq!(turn.caption, "What a magnificent potato!");
    }

    #[test]
    fn model_strokes_draw_scaled_into_the_frame_and_clipped_to_the_canvas() {
        let (_buf, mut surf) = page();
        let frame = BBox { x0: 200, y0: 400, x1: 1200, y1: 1400 };
        let clip = BBox { x0: 100, y0: 300, x1: 1300, y1: 1500 };
        let strokes = vec![vec![(0.0, 0.0), (100.0, 100.0)], vec![(50.0, 0.0), (50.0, 100.0)]];
        let dirty = draw_strokes(&mut surf, &frame, &clip, &strokes);
        assert!(!dirty.is_empty());
        // The diagonal spans the frame, so ink lands mid-frame.
        assert!(surf.luma(700, 900) < 128, "mid-frame stroke missing");
        // Points scale into the frame, never past the clip.
        assert!(dirty.x0 >= clip.x0 - 8 && dirty.x1 <= clip.x1 + 8);

        // A frame never sent (empty) draws nothing.
        assert!(draw_strokes(&mut surf, &BBox::empty(), &clip, &strokes).is_empty());
    }

    #[test]
    fn story_replies_parse_into_beat_and_exactly_three_choices() {
        let reply = "The potato knight reached a wobbly bridge.\nA troll asked for a password.\n1. Say please\n2. Sing loudly\n3. Wobble back";
        let (beat, choices) = parse_story(reply);
        assert!(beat.contains("wobbly bridge"));
        assert_eq!(choices, vec!["SAY PLEASE", "SING LOUDLY", "WOBBLE BACK"]);

        // A model that forgets choices still leaves the child a way onward.
        let (_, defaults) = parse_story("The end of the bridge. That's all I know.");
        assert_eq!(defaults.len(), 3);
        assert_eq!(defaults[0], "GO ON");
    }

    #[test]
    fn long_choice_labels_are_shortened_not_overflowed() {
        let reply = "Beat.\n1. Ask the enormous purple dinosaur very politely\n2. Run\n3. Hide";
        let (_, choices) = parse_story(reply);
        assert!(choices[0].len() <= 16, "{}", choices[0]);
    }

    #[test]
    fn every_decoration_draws_something_near_the_doodle() {
        let font = ui_font();
        let decos = [
            Deco::Eyes, Deco::Legs, Deco::Arms, Deco::Hat, Deco::Antenna,
            Deco::Tail, Deco::Rays, Deco::Crown, Deco::Mustache, Deco::Bubble("MOO".into()),
        ];
        for deco in decos {
            let (_buf, mut surf) = page();
            let mut ink = BBox::empty();
            ink.add(600, 900, 0);
            ink.add(900, 1200, 0);
            let dirty = draw_deco(&mut surf, &ink, &deco, &font);
            assert!(!dirty.is_empty(), "{deco:?} drew nothing");
            let mut dark = 0;
            for y in dirty.y0..=dirty.y1 {
                for x in dirty.x0..=dirty.x1 {
                    if surf.luma(x, y) < 128 {
                        dark += 1;
                    }
                }
            }
            assert!(dark > 30, "{deco:?} left almost no ink ({dark})");
        }
    }

    #[test]
    fn decorating_an_empty_page_is_a_no_op() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        assert!(draw_deco(&mut surf, &BBox::empty(), &Deco::Eyes, &font).is_empty());
    }

    #[test]
    fn play_pages_declare_canvas_and_story_choice_targets() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let map = draw(&mut surf, &font, &Game::Critter { round: 0 });
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
        assert_eq!(Game::nth(0), Game::Critter { round: 0 });
        assert_eq!(Game::nth(1), Game::Guess);
        assert!(matches!(Game::nth(2), Game::Story { .. }));
        assert_eq!(Game::nth(3), Game::Critter { round: 0 });
    }

    #[test]
    fn instructions_carry_the_protocol_each_parser_expects() {
        let c = Game::Critter { round: 0 }.instruction(None);
        assert!(c.contains("D 10,80") && c.contains("0-100"), "critter must teach the stroke protocol");
        let s = Game::story().instruction(None);
        assert!(s.contains("1. 2. 3."));
        let mid = Game::Story { log: vec![("RUN".into(), "b".into())], choices: vec![], pending: None };
        assert!(mid.instruction(Some("SING LOUDLY")).contains("SING LOUDLY"));
    }
}
