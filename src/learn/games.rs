//! The play pages — where Learn mode stops drilling and starts giggling.
//!
//! Three games, all riding the same anchored-mark vocabulary as the
//! worksheets. The pad cannot fetch pictures, but it can carry them: the
//! critter game ships a deck of real grayscale character art (like the sleep
//! page's brand mark), and the model's turn is to pick a card and place it.
//! Its voice is a sentence in the reply hand:
//!
//! - **Critter**: the child doodles, marks DONE, and the pad plays one
//!   picture card from a themed deck — a character lands next to the doodle,
//!   never on top of it (the child's ink is composited darken-only, so it
//!   can never be painted away). The model picks the card, the spot, and
//!   the joke; the pad guarantees the art.
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

/// A card theme: every card played onto one doodle stays in one world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Heelers,
    StarWars,
}

/// One picture card: real grayscale art shipped in the binary.
pub struct Card {
    /// The word the model plays it with.
    pub key: &'static str,
    /// One line telling the model what the picture shows.
    pub blurb: &'static str,
    /// 8-bit grayscale PNG (see scripts/make-learn-art.sh).
    pub png: &'static [u8],
}

pub const HEELERS: &[Card] = &[
    Card {
        key: "BLUEY",
        blurb: "a blue heeler pup waving hello",
        png: include_bytes!("../../assets/learn/bluey.png"),
    },
    Card {
        key: "BINGO",
        blurb: "her little sister cheering with both paws up",
        png: include_bytes!("../../assets/learn/bingo.png"),
    },
];

pub const STAR_WARS: &[Card] = &[
    Card {
        key: "R2D2",
        blurb: "a brave little dome-headed droid",
        png: include_bytes!("../../assets/learn/r2d2.png"),
    },
    Card {
        key: "BB8",
        blurb: "a round rolling droid with a wobbly head",
        png: include_bytes!("../../assets/learn/bb8.png"),
    },
    Card {
        key: "GROGU",
        blurb: "a tiny green youngling with huge ears",
        png: include_bytes!("../../assets/learn/grogu.png"),
    },
    Card {
        key: "VADER",
        blurb: "the deep-breathing dark helmet, secretly a softie",
        png: include_bytes!("../../assets/learn/vader.png"),
    },
    Card {
        key: "XWING",
        blurb: "a four-winged starfighter zooming past",
        png: include_bytes!("../../assets/learn/xwing.png"),
    },
];

impl Theme {
    pub fn deck(self) -> &'static [Card] {
        match self {
            Theme::Heelers => HEELERS,
            Theme::StarWars => STAR_WARS,
        }
    }

    /// A coin flip on the wall clock: which world this doodle joins.
    pub fn random() -> Self {
        if now_seed() & 1 == 0 { Theme::Heelers } else { Theme::StarWars }
    }
}

/// Nanoseconds of the wall clock — entropy enough for a card game.
fn now_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(7)
}

/// Which game a play page carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Game {
    /// `round` counts the pad's card turns on this doodle; `last` remembers
    /// the card just played so the next turn deals a different one.
    Critter { round: u8, theme: Theme, last: Option<&'static str> },
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

    /// A fresh critter page: theme flipped on the clock, no cards played yet.
    pub fn critter() -> Self {
        Game::Critter { round: 0, theme: Theme::random(), last: None }
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
            Game::Critter { round, theme, last } => {
                let hand = hand_line(*theme, *last);
                let example = theme
                    .deck()
                    .iter()
                    .find(|c| Some(c.key) != *last)
                    .map(|c| c.key)
                    .unwrap_or("R2D2");
                format!(
                    "{base}The page shows the child's doodle{prior}. You are playing picture \
                     cards with the child: you pick one card and it lands on the page as a \
                     real drawing. Your hand: {hand}. \
                     Pick the ONE card that makes the doodle funniest, as if it just walked \
                     into the child's picture. Reply with EXACTLY two lines and nothing else. \
                     Line 1: one funny caption of at most eight very simple words tying your \
                     card to the doodle. \
                     Line 2: PLAY <CARD> AT x,y SIZE s — <CARD> is the card word from your \
                     hand; x,y is the CENTER of a big EMPTY spot (whole numbers 0-100, 0,0 \
                     top-left, 100,100 bottom-right) chosen so the card lands NEXT TO the \
                     child's drawing, never on top of it; s is how wide the card lands as a \
                     share of the page, 30 to 55. Example: PLAY {example} AT 72,30 SIZE 42",
                    prior = if *round > 0 { " (a card you played is already on it)" } else { "" }
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

// ---- the pad's turn: playing a card ---------------------------------------

/// The dealt hand, written for the model: every theme card but the one just
/// played. Two-card themes therefore alternate; bigger decks read as a menu.
fn hand_line(theme: Theme, last: Option<&str>) -> String {
    theme
        .deck()
        .iter()
        .filter(|c| Some(c.key) != last)
        .map(|c| format!("{} ({})", c.key, c.blurb))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The model's card turn, parsed: which card it named (if any), where it
/// wants it, how big, and the caption.
pub struct CardTurn {
    /// The card word from the PLAY line, uppercased, unvalidated.
    pub key: Option<String>,
    /// Landing center in 0–100 of the picture it saw.
    pub center: Option<(f32, f32)>,
    /// Landing width as a share of the picture, 0–100.
    pub size: Option<f32>,
    pub caption: String,
}

/// Parse a critter reply. The line leading with PLAY names the card and its
/// landing spot; every other non-empty line joins the caption. Any missing
/// piece stays None — the game never stalls on a chatty model, because the
/// pad can always deal a random card into empty space itself.
pub fn parse_card_turn(reply: &str) -> CardTurn {
    let mut turn = CardTurn { key: None, center: None, size: None, caption: String::new() };
    let mut caption_lines: Vec<&str> = Vec::new();
    for line in reply.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let mut words = t.split_whitespace();
        let lead: String = words
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect::<String>()
            .to_ascii_uppercase();
        if lead != "PLAY" || turn.key.is_some() {
            caption_lines.push(t);
            continue;
        }
        // The card: the first word that isn't protocol furniture.
        for w in words {
            let word: String =
                w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_uppercase();
            if word.is_empty() || word.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if !matches!(word.as_str(), "AT" | "SIZE") {
                turn.key = Some(word);
                break;
            }
        }
        // The numbers, in order, however they were punctuated: two or more
        // read as x,y (+ size); a lone number can only be the size.
        let mut nums: Vec<f32> = Vec::new();
        let mut cur = String::new();
        let mut prev = ' ';
        for c in t.chars().chain(std::iter::once(' ')) {
            // A digit glued to a letter is part of a name (BB8, R2D2),
            // never a coordinate.
            let extend = !cur.is_empty() && (c.is_ascii_digit() || c == '.');
            let start = cur.is_empty()
                && !prev.is_alphabetic()
                && (c.is_ascii_digit() || c == '-');
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
        if nums.len() >= 2 {
            turn.center = Some((nums[0].clamp(0.0, 100.0), nums[1].clamp(0.0, 100.0)));
        }
        match nums.len() {
            1 => turn.size = Some(nums[0].clamp(25.0, 60.0)),
            n if n >= 3 => turn.size = Some(nums[n - 1].clamp(25.0, 60.0)),
            _ => {}
        }
    }
    turn.caption = caption_lines.join(" ");
    turn
}

/// Turn the model's card word into a real card from this doodle's theme.
/// An unknown or missing word falls back to chance — any theme card but the
/// one just played — so a chatty model still plays a proper turn.
pub fn resolve_card(theme: Theme, last: Option<&str>, want: Option<&str>) -> &'static Card {
    let deck = theme.deck();
    if let Some(w) = want {
        if let Some(c) = deck.iter().find(|c| c.key == w) {
            return c;
        }
    }
    let fresh: Vec<&'static Card> = deck.iter().filter(|c| Some(c.key) != last).collect();
    fresh[(now_seed() as usize) % fresh.len()]
}

/// Decode a card's art: (width, height, gray bytes). None rather than a
/// panic — a bad asset must never end the game.
fn card_gray(card: &Card) -> Option<(usize, usize, Vec<u8>)> {
    let dec = png::Decoder::new(card.png);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Grayscale || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some((info.width as usize, info.height as usize, buf))
}

/// Play `card` onto the page. `frame` is the rectangle the model's 0–100
/// coordinates cover (the crop it was shown); `clip` is the open canvas the
/// art must stay inside; `ink` is the child's drawing, which the card dodges
/// and — because the art is composited darken-only — can never erase.
/// Returns the dirty region (empty if the asset is unreadable).
pub fn play_card(
    surf: &mut Surface,
    frame: &BBox,
    clip: &BBox,
    ink: &BBox,
    card: &Card,
    center: Option<(f32, f32)>,
    size: Option<f32>,
) -> BBox {
    if frame.is_empty() || clip.is_empty() {
        return BBox::empty();
    }
    let Some((sw, sh, gray)) = card_gray(card) else {
        return BBox::empty();
    };
    let (fx, fy, fw, fh) = frame.rect();
    let (_, _, cw, ch) = clip.rect();

    // The landing box: model's size as a share of the frame, kept generous
    // but on-canvas, aspect true to the art.
    let pct = size.unwrap_or(42.0).clamp(25.0, 60.0);
    let mut w = (fw as f32 * pct / 100.0) as i32;
    w = w.clamp(220.min(cw), (cw * 3 / 5).max(220)).min(cw);
    let mut h = w * sh as i32 / sw as i32;
    if h > ch * 3 / 5 {
        h = ch * 3 / 5;
        w = h * sw as i32 / sh as i32;
    }

    // The landing spot: the model's pick, or the open corner farthest from
    // the child's ink when it named none.
    let (mut cx, mut cy) = match center {
        Some((nx, ny)) => (fx + (nx / 100.0 * fw as f32) as i32, fy + (ny / 100.0 * fh as f32) as i32),
        None => farthest_corner(clip, ink),
    };
    // On-canvas, whole card visible.
    cx = cx.clamp(clip.x0 + w / 2, (clip.x1 - w / 2).max(clip.x0 + w / 2));
    cy = cy.clamp(clip.y0 + h / 2, (clip.y1 - h / 2).max(clip.y0 + h / 2));

    // The child's drawing is locked: if the card would squat on it, slide the
    // card sideways toward the roomier half before it lands.
    if !ink.is_empty() {
        let overlap_w = ((cx + w / 2).min(ink.x1) - (cx - w / 2).max(ink.x0)).max(0);
        let overlap_h = ((cy + h / 2).min(ink.y1) - (cy - h / 2).max(ink.y0)).max(0);
        if overlap_w * overlap_h > w * h / 4 {
            let room_left = ink.x0 - clip.x0;
            let room_right = clip.x1 - ink.x1;
            cx = if room_right >= room_left {
                (ink.x1 + w / 2 + 20).min(clip.x1 - w / 2)
            } else {
                (ink.x0 - w / 2 - 20).max(clip.x0 + w / 2)
            };
        }
    }

    // Land it: bilinear from the asset, darken-only onto the page, so the
    // art keeps its 16 grays and the child's ink shows through everywhere.
    let (x0, y0) = (cx - w / 2, cy - h / 2);
    for row in 0..h {
        let sy = ((row as f32 + 0.5) * sh as f32 / h as f32 - 0.5).max(0.0);
        let (syi, syf) = (sy as usize, sy.fract());
        let sy1 = (syi + 1).min(sh - 1);
        for col in 0..w {
            let sx = ((col as f32 + 0.5) * sw as f32 / w as f32 - 0.5).max(0.0);
            let (sxi, sxf) = (sx as usize, sx.fract());
            let sx1 = (sxi + 1).min(sw - 1);
            let g00 = gray[syi * sw + sxi] as f32;
            let g01 = gray[syi * sw + sx1] as f32;
            let g10 = gray[sy1 * sw + sxi] as f32;
            let g11 = gray[sy1 * sw + sx1] as f32;
            let g = (g00 * (1.0 - sxf) + g01 * sxf) * (1.0 - syf)
                + (g10 * (1.0 - sxf) + g11 * sxf) * syf;
            let g = g as u8;
            if g >= 248 {
                continue; // paper stays paper
            }
            let (x, y) = (x0 + col, y0 + row);
            let under = surf.luma(x, y);
            let g = g.min(under);
            let q = (g as u16) >> 3;
            surf.put_px(x, y, (q << 11) | (((g as u16) >> 2) << 5) | q);
        }
    }
    let mut dirty = BBox::empty();
    dirty.add(x0, y0, 4);
    dirty.add(x0 + w, y0 + h, 4);
    dirty
}

/// The canvas corner with the most daylight between it and the ink.
fn farthest_corner(clip: &BBox, ink: &BBox) -> (i32, i32) {
    let (ix, iy) = if ink.is_empty() {
        ((clip.x0 + clip.x1) / 2, (clip.y0 + clip.y1) / 2)
    } else {
        ((ink.x0 + ink.x1) / 2, (ink.y0 + ink.y1) / 2)
    };
    let inset_x = (clip.x1 - clip.x0) / 4;
    let inset_y = (clip.y1 - clip.y0) / 4;
    [
        (clip.x0 + inset_x, clip.y0 + inset_y),
        (clip.x1 - inset_x, clip.y0 + inset_y),
        (clip.x0 + inset_x, clip.y1 - inset_y),
        (clip.x1 - inset_x, clip.y1 - inset_y),
    ]
    .into_iter()
    .max_by_key(|(x, y)| {
        let (dx, dy) = ((x - ix) as i64, (y - iy) as i64);
        dx * dx + dy * dy
    })
    .unwrap()
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

    #[test]
    fn card_replies_parse_into_card_spot_size_and_caption() {
        let turn = parse_card_turn("A droid moved into your house!\nPLAY R2D2 AT 72,30 SIZE 45");
        assert_eq!(turn.key.as_deref(), Some("R2D2"));
        assert_eq!(turn.center, Some((72.0, 30.0)));
        assert_eq!(turn.size, Some(45.0));
        assert_eq!(turn.caption, "A droid moved into your house!");

        // Punctuation, case, and chatter are tamed, not fatal.
        let turn = parse_card_turn("Bluey wants a turn!\nplay bluey at (20, 60) size 50 please");
        assert_eq!(turn.key.as_deref(), Some("BLUEY"));
        assert_eq!(turn.center, Some((20.0, 60.0)));
        assert_eq!(turn.size, Some(50.0));

        // Missing pieces stay missing; the caption still lands.
        let turn = parse_card_turn("PLAY GROGU");
        assert_eq!(turn.key.as_deref(), Some("GROGU"));
        assert!(turn.center.is_none() && turn.size.is_none());

        // No PLAY line at all: caption only, the pad deals for itself.
        let turn = parse_card_turn("What a magnificent potato!");
        assert!(turn.key.is_none());
        assert_eq!(turn.caption, "What a magnificent potato!");

        // Out-of-range numbers are clamped to the page and the size band.
        let turn = parse_card_turn("Zoom!\nPLAY XWING AT 150,-20 SIZE 99");
        assert_eq!(turn.center, Some((100.0, 0.0)));
        assert_eq!(turn.size, Some(60.0));
    }

    #[test]
    fn cards_resolve_from_the_theme_with_a_fresh_fallback() {
        // A named card in the theme is honored.
        let c = resolve_card(Theme::StarWars, None, Some("VADER"));
        assert_eq!(c.key, "VADER");
        // A card from the wrong world, or nonsense, falls back into theme…
        let c = resolve_card(Theme::Heelers, None, Some("VADER"));
        assert!(HEELERS.iter().any(|h| h.key == c.key));
        // …and the fallback never repeats the card just played.
        for _ in 0..12 {
            let c = resolve_card(Theme::Heelers, Some("BLUEY"), None);
            assert_eq!(c.key, "BINGO");
        }
    }

    #[test]
    fn the_dealt_hand_skips_the_card_just_played() {
        let h = hand_line(Theme::Heelers, Some("BLUEY"));
        assert!(h.contains("BINGO") && !h.contains("BLUEY"));
        let h = hand_line(Theme::StarWars, None);
        for c in STAR_WARS {
            assert!(h.contains(c.key), "{} missing from the hand", c.key);
        }
    }

    #[test]
    fn every_card_decodes_as_8_bit_grayscale_art_with_real_tone() {
        for card in HEELERS.iter().chain(STAR_WARS) {
            let (w, h, gray) = card_gray(card)
                .unwrap_or_else(|| panic!("{} must decode as 8-bit grayscale", card.key));
            assert!(w >= 400 && h >= 400, "{} too small: {w}x{h}", card.key);
            // Continuous tone must survive to the GC16 panel: at least 8
            // distinct ink levels, with real black and a genuine mid-gray.
            let mut seen = [false; 16];
            for &g in &gray {
                seen[(g >> 4) as usize] = true;
            }
            let levels = seen.iter().filter(|&&s| s).count();
            assert!(levels >= 8, "{} has only {levels} distinct ink levels", card.key);
            assert!(seen[0] || seen[1], "{} carries no near-black stroke", card.key);
            assert!(
                seen[5] || seen[6] || seen[7] || seen[8],
                "{} carries no genuine mid-gray",
                card.key
            );
        }
    }

    #[test]
    fn a_played_card_lands_in_frame_with_tone_and_never_erases_ink() {
        let (_buf, mut surf) = page();
        let frame = BBox { x0: 112, y0: 224, x1: 1292, y1: 1548 };
        let clip = frame.clone();
        // The child's ink: a black blot on the left half.
        let mut ink = BBox::empty();
        for y in 700..900 {
            for x in 300..500 {
                surf.put_px(x, y, BLACK);
            }
        }
        ink.add(300, 700, 0);
        ink.add(500, 900, 0);
        let card = &STAR_WARS[0];
        let dirty = play_card(&mut surf, &frame, &clip, &ink, card, Some((75.0, 40.0)), Some(45.0));
        assert!(!dirty.is_empty());
        assert!(dirty.x0 >= clip.x0 - 8 && dirty.x1 <= clip.x1 + 8);
        assert!(dirty.y0 >= clip.y0 - 8 && dirty.y1 <= clip.y1 + 8);
        // Real grayscale landed, not a silhouette.
        let mut seen = [false; 16];
        for y in dirty.y0..dirty.y1 {
            for x in dirty.x0..dirty.x1 {
                seen[(surf.luma(x, y) >> 4) as usize] = true;
            }
        }
        let levels = seen.iter().filter(|&&s| s).count();
        assert!(levels >= 6, "card landed with only {levels} ink levels");
        // The child's ink is locked: still black everywhere it was.
        for y in 700..900 {
            for x in 300..500 {
                assert!(surf.luma(x, y) < 32, "ink erased at {x},{y}");
            }
        }
    }

    #[test]
    fn a_card_aimed_at_the_ink_slides_off_it() {
        let (_buf, mut surf) = page();
        let frame = BBox { x0: 112, y0: 224, x1: 1292, y1: 1548 };
        let mut ink = BBox::empty();
        ink.add(300, 700, 0);
        ink.add(650, 1100, 0);
        // The model aims dead at the doodle; the card must land mostly off it.
        let dirty =
            play_card(&mut surf, &frame, &frame, &ink, &STAR_WARS[0], Some((30.0, 55.0)), Some(40.0));
        assert!(!dirty.is_empty());
        let overlap_w = (dirty.x1.min(ink.x1) - dirty.x0.max(ink.x0)).max(0);
        let overlap_h = (dirty.y1.min(ink.y1) - dirty.y0.max(ink.y0)).max(0);
        let area = (dirty.x1 - dirty.x0) * (dirty.y1 - dirty.y0);
        assert!(
            overlap_w * overlap_h <= area / 3,
            "card still squats on the doodle: {}x{} of {}",
            overlap_w,
            overlap_h,
            area
        );
    }

    #[test]
    fn an_unsent_frame_plays_no_card() {
        let (_buf, mut surf) = page();
        let clip = BBox { x0: 112, y0: 224, x1: 1292, y1: 1548 };
        let dirty =
            play_card(&mut surf, &BBox::empty(), &clip, &BBox::empty(), &HEELERS[0], None, None);
        assert!(dirty.is_empty());
    }

    #[test]
    fn play_pages_declare_canvas_and_story_choice_targets() {
        let font = ui_font();
        let (_buf, mut surf) = page();
        let critter = Game::Critter { round: 0, theme: Theme::StarWars, last: None };
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
        assert!(matches!(Game::nth(0), Game::Critter { round: 0, last: None, .. }));
        assert_eq!(Game::nth(1), Game::Guess);
        assert!(matches!(Game::nth(2), Game::Story { .. }));
        assert!(matches!(Game::nth(3), Game::Critter { round: 0, last: None, .. }));
    }

    #[test]
    fn instructions_carry_the_protocol_each_parser_expects() {
        let c = Game::Critter { round: 0, theme: Theme::StarWars, last: Some("R2D2") }
            .instruction(None);
        assert!(c.contains("PLAY") && c.contains("0-100"), "critter must teach the card protocol");
        assert!(c.contains("GROGU (") && !c.contains("R2D2 ("), "the hand must skip the last card");
        assert!(!c.contains("BLUEY"), "the hand must stay in its theme");
        let s = Game::story().instruction(None);
        assert!(s.contains("1. 2. 3."));
        let mid = Game::Story { log: vec![("RUN".into(), "b".into())], choices: vec![], pending: None };
        assert!(mid.instruction(Some("SING LOUDLY")).contains("SING LOUDLY"));
    }
}
