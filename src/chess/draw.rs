//! The chess page: a header naming the puzzle, the board from the solver's
//! side, a status line, and a row of buttons. Pieces are DejaVu Sans chess
//! glyphs (fonts/ChessGlyphs.ttf, a subset): a white piece is the filled
//! silhouette in white under the outline glyph in black, so it reads on
//! either square colour.

use ab_glyph::FontRef;

use super::{file, rank, Category, Square, Trainer};
use crate::fb::{BBox, SCREEN_H, SCREEN_W};
use crate::script;
use crate::surface::{Surface, BLACK, WHITE};
use crate::ui::{full_text, BLUE, LABEL_PX};

pub const CHESS_FONT_TTF: &[u8] = include_bytes!("../../fonts/ChessGlyphs.ttf");

const SQ: usize = 160;
const BOARD_X: usize = (SCREEN_W - 8 * SQ) / 2;
const BOARD_Y: usize = 130;
const STATUS_Y: usize = BOARD_Y + 8 * SQ + 40;
const BUTTON_Y: usize = STATUS_Y + 110;
const BUTTON_H: usize = 120;
const BUTTON_GAP: usize = 16;
const DARK: u16 = 0xB5B6;
const PIECE_PX: f32 = 150.0;

/// What a tap on the chess page means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Square(Square),
    Kind(Category),
    Hint,
    Next,
    Exit,
}

const BUTTONS: [Hit; 6] = [
    Hit::Kind(Category::Tactic),
    Hit::Kind(Category::Endgame),
    Hit::Kind(Category::Opening),
    Hit::Hint,
    Hit::Next,
    Hit::Exit,
];

fn button_w() -> usize {
    (SCREEN_W - 2 * BOARD_X - (BUTTONS.len() - 1) * BUTTON_GAP) / BUTTONS.len()
}

fn button_x(i: usize) -> usize {
    BOARD_X + i * (button_w() + BUTTON_GAP)
}

/// The square's top-left corner on screen.
fn square_xy(sq: Square, flipped: bool) -> (usize, usize) {
    let (col, row) = if flipped { (7 - file(sq), rank(sq)) } else { (file(sq), 7 - rank(sq)) };
    (BOARD_X + col as usize * SQ, BOARD_Y + row as usize * SQ)
}

pub fn hit(x: i32, y: i32, flipped: bool) -> Option<Hit> {
    let (bx, by) = (BOARD_X as i32, BOARD_Y as i32);
    let side = (8 * SQ) as i32;
    if x >= bx && x < bx + side && y >= by && y < by + side {
        let col = ((x - bx) / SQ as i32) as u8;
        let row = ((y - by) / SQ as i32) as u8;
        let (f, r) = if flipped { (7 - col, row) } else { (col, 7 - row) };
        return Some(Hit::Square(r * 8 + f));
    }
    if y >= BUTTON_Y as i32 && y < (BUTTON_Y + BUTTON_H) as i32 {
        return BUTTONS.iter().enumerate().find_map(|(i, &b)| {
            let x0 = button_x(i) as i32;
            (x >= x0 && x < x0 + button_w() as i32).then_some(b)
        });
    }
    None
}

/// The region a board change repaints: the board and the status line.
pub fn board_region() -> BBox {
    let mut b = BBox::empty();
    b.add(0, BOARD_Y as i32 - 10, 0);
    b.add(SCREEN_W as i32 - 1, (BUTTON_Y - 10) as i32, 0);
    b
}

pub fn draw(surf: &mut Surface, ui_font: &FontRef, pieces: &FontRef, t: &Trainer) {
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    let header = format!(
        "CHESS · {} · {} · {}",
        t.category.label(),
        t.puzzle.label.to_uppercase(),
        t.puzzle.rating
    );
    full_text(surf, ui_font, &header, LABEL_PX, BOARD_X, 40, BLACK);
    draw_board(surf, ui_font, pieces, t);
    for (i, &b) in BUTTONS.iter().enumerate() {
        let (label, on) = match b {
            Hit::Kind(c) => (c.label(), c == t.category),
            Hit::Hint => ("HINT", false),
            Hit::Next => ("NEXT", t.solved),
            Hit::Exit => ("EXIT", false),
            Hit::Square(_) => unreachable!("not a button"),
        };
        button(surf, ui_font, button_x(i), label, on);
    }
}

/// Repaint the board and status line only.
pub fn draw_board(surf: &mut Surface, ui_font: &FontRef, pieces: &FontRef, t: &Trainer) {
    let r = board_region();
    surf.fill_rect(0, r.y0 as usize, SCREEN_W, (r.y1 - r.y0 + 1) as usize, WHITE);
    let hint_from = t.hint.then(|| t.expected().map(|m| m.from)).flatten();
    for sq in 0..64u8 {
        let (x, y) = square_xy(sq, t.flipped);
        let dark = (file(sq) + rank(sq)).is_multiple_of(2);
        surf.fill_rect(x, y, SQ, SQ, if dark { DARK } else { WHITE });
        if t.last.is_some_and(|m| m.from == sq || m.to == sq) {
            frame(surf, x, y, 6, BLACK);
        }
        if hint_from == Some(sq) {
            frame(surf, x, y, 12, BLUE);
        }
        if t.selected == Some(sq) {
            frame(surf, x, y, 14, BLACK);
        }
        let p = t.board.at(sq);
        if p != 0 {
            draw_piece(surf, pieces, p, x + SQ / 2, y + SQ / 2);
        }
    }
    // Coordinates in the corner squares, small, for players who think in them.
    for i in 0..8u8 {
        let f = if t.flipped { 7 - i } else { i };
        let (x, y) = square_xy(if t.flipped { f + 56 } else { f }, t.flipped);
        full_text(surf, ui_font, &((b'a' + f) as char).to_string(), 24.0, x + SQ - 22, y + SQ - 30, BLACK);
        let r = if t.flipped { 7 - i } else { i };
        let (x, y) = square_xy(r * 8 + if t.flipped { 7 } else { 0 }, t.flipped);
        full_text(surf, ui_font, &(r + 1).to_string(), 24.0, x + 6, y + 4, BLACK);
    }
    rect_outline(surf, BOARD_X - 3, BOARD_Y - 3, 8 * SQ + 6, 3);
    full_text(surf, ui_font, &t.status, LABEL_PX, BOARD_X, STATUS_Y, BLACK);
}

fn draw_piece(surf: &mut Surface, font: &FontRef, p: u8, cx: usize, cy: usize) {
    let idx = match p.to_ascii_lowercase() {
        b'k' => 0,
        b'q' => 1,
        b'r' => 2,
        b'b' => 3,
        b'n' => 4,
        _ => 5,
    };
    let outline = char::from_u32(0x2654 + idx).expect("chess glyph");
    let filled = char::from_u32(0x265A + idx).expect("chess glyph");
    if p.is_ascii_uppercase() {
        glyph(surf, font, filled, cx, cy, WHITE);
        glyph(surf, font, outline, cx, cy, BLACK);
    } else {
        glyph(surf, font, filled, cx, cy, BLACK);
    }
}

/// One glyph with its inked pixels centered on (cx, cy).
fn glyph(surf: &mut Surface, font: &FontRef, c: char, cx: usize, cy: usize, color: u16) {
    let line = script::rasterize_line(font, &c.to_string(), PIECE_PX);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..line.height {
        for x in 0..line.width {
            if line.mask[y * line.width + x] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 > x1 {
        return;
    }
    let ox = cx as i32 - (x0 + x1) as i32 / 2;
    let oy = cy as i32 - (y0 + y1) as i32 / 2;
    for y in y0..=y1 {
        for x in x0..=x1 {
            if line.mask[y * line.width + x] {
                surf.put_px(ox + x as i32, oy + y as i32, color);
            }
        }
    }
}

fn frame(surf: &mut Surface, x: usize, y: usize, t: usize, c: u16) {
    surf.fill_rect(x, y, SQ, t, c);
    surf.fill_rect(x, y + SQ - t, SQ, t, c);
    surf.fill_rect(x, y, t, SQ, c);
    surf.fill_rect(x + SQ - t, y, t, SQ, c);
}

fn rect_outline(surf: &mut Surface, x: usize, y: usize, w: usize, t: usize) {
    surf.fill_rect(x, y, w, t, BLACK);
    surf.fill_rect(x, y + w - t, w, t, BLACK);
    surf.fill_rect(x, y, t, w, BLACK);
    surf.fill_rect(x + w - t, y, t, w, BLACK);
}

fn button(surf: &mut Surface, font: &FontRef, x: usize, label: &str, on: bool) {
    let w = button_w();
    let (bg, fg) = if on { (BLACK, WHITE) } else { (WHITE, BLACK) };
    surf.fill_rect(x, BUTTON_Y, w, BUTTON_H, bg);
    rect_outline_wh(surf, x, BUTTON_Y, w, BUTTON_H, 3);
    let px = 30.0;
    let tw = script::measure(font, label, px) as usize;
    full_text(surf, font, label, px, x + w.saturating_sub(tw) / 2, BUTTON_Y + BUTTON_H / 2 - 18, fg);
}

fn rect_outline_wh(surf: &mut Surface, x: usize, y: usize, w: usize, h: usize, t: usize) {
    surf.fill_rect(x, y, w, t, BLACK);
    surf.fill_rect(x, y + h - t, w, t, BLACK);
    surf.fill_rect(x, y, t, h, BLACK);
    surf.fill_rect(x + w - t, y, t, h, BLACK);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_square_and_button_hits_back_to_itself() {
        for flipped in [false, true] {
            for sq in 0..64u8 {
                let (x, y) = square_xy(sq, flipped);
                assert_eq!(hit((x + SQ / 2) as i32, (y + SQ / 2) as i32, flipped), Some(Hit::Square(sq)));
            }
        }
        for (i, &b) in BUTTONS.iter().enumerate() {
            assert_eq!(hit((button_x(i) + 10) as i32, (BUTTON_Y + 10) as i32, false), Some(b));
        }
        const { assert!(BUTTON_Y + BUTTON_H < SCREEN_H, "buttons fit on the page") };
        assert!(button_x(BUTTONS.len() - 1) + button_w() <= SCREEN_W);
    }

    #[test]
    fn white_on_the_bottom_unless_black_solves() {
        assert!(square_xy(0, false).1 > square_xy(63, false).1, "a1 below h8");
        assert!(square_xy(0, true).1 < square_xy(63, true).1, "flipped: a1 on top");
    }
}
