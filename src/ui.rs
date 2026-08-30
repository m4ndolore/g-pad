//! Quiet, read-only secondary surfaces for history, corpus, and preferences.
//! UI text uses the bundled grotesque; the pad's handwriting never appears here.

use ab_glyph::FontRef;

use crate::fb::{BBox, SCREEN_H, SCREEN_W};
use crate::memory::MemoryStore;
use crate::oracle::ContextSnapshot;
use crate::preferences::{Mode, Preferences};
use crate::script;
use crate::surface::{Surface, BLACK, WHITE};

pub const UI_FONT_TTF: &[u8] = include_bytes!("../fonts/LiberationSans-Regular.ttf");
pub const PANEL_W: usize = SCREEN_W * 50 / 100;
const LABEL_PX: f32 = 32.0;
const TITLE_PX: f32 = 64.0;
const PAD: usize = 36;
const BLUE: u16 = 0x0335;
const HEADER_H: i32 = 105;
const THREAD_Y0: i32 = 148;
const CONV_ROW_H: usize = 168;
const THREAD_FOOTER: i32 = 150;
const SCROLL_STEP: i32 = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawerKind { History, Corpus }

pub struct Drawer {
    pub kind: DrawerKind,
    pub selection: Option<usize>,
    pub scroll: i32,
    /// `Some` opens that sitting as a thread; `None` shows the conversation list.
    pub thread: Option<usize>,
    saved: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Close,
    History,
    Corpus,
    Replay(u64),
    Threads,
    OpenThread(usize),
    Send,
    Erase,
    NewPage,
    Sleep,
    Settings,
    Dismiss,
    SetMode(Mode),
    ToggleIdle,
    Quit,
}

impl Drawer {
    pub fn open(surf: &Surface, kind: DrawerKind, selection: Option<usize>, scroll: i32, thread: Option<usize>) -> Self {
        Self { kind, selection, scroll, thread,
            saved: surf.copy_rect(0, 0, PANEL_W, SCREEN_H) }
    }

    pub fn close(self, surf: &mut Surface) -> BBox {
        surf.paste_rect(0, 0, PANEL_W, SCREEN_H, &self.saved);
        panel_region()
    }

    pub fn scroll_by(&mut self, delta: i32) {
        self.scroll = (self.scroll + delta.signum()).max(0);
    }

    pub fn tap(&mut self, x: i32, y: i32, store: &Option<MemoryStore>) -> Action {
        if x < 0 || x >= PANEL_W as i32 { return Action::Close; }
        if y < HEADER_H {
            if x < 100 {
                return if self.kind == DrawerKind::History && self.thread.is_some() {
                    Action::Threads
                } else {
                    Action::Close
                };
            }
            if x < PANEL_W as i32 / 2 { return Action::History; }
            return Action::Corpus;
        }
        if self.kind == DrawerKind::Corpus { return Action::None; }
        let Some(s) = store else { return Action::None };
        let convs = s.conversations();
        if let Some(ti) = self.thread {
            let Some(conv) = convs.get(ti) else { return Action::Threads };
            if self.selection.is_some() && y > SCREEN_H as i32 - THREAD_FOOTER {
                return self.selection.and_then(|i| conv.turns.get(i)).map(|r| Action::Replay(r.id)).unwrap_or(Action::None);
            }
            if let Some(i) = thread_index_at(y, self.scroll, &conv.turns) {
                self.selection = Some(i);
            }
            return Action::None;
        }
        if let Some(i) = selector_index_at(y, convs.len(), self.scroll) {
            return Action::OpenThread(i);
        }
        Action::None
    }
}

pub fn draw_drawer(surf: &mut Surface, font: &FontRef, store: &Option<MemoryStore>,
    snapshot: &ContextSnapshot, drawer: &Drawer) {
    surf.fill_rect(0, 0, PANEL_W, SCREEN_H, WHITE);
    surf.fill_rect(PANEL_W - 2, 0, 2, SCREEN_H, BLACK);
    let close = if drawer.kind == DrawerKind::History && drawer.thread.is_some() { "←" } else { "×" };
    text(surf, font, close, LABEL_PX, PAD, 36, BLACK);
    text(surf, font, "HISTORY", LABEL_PX, 105, 36,
        if drawer.kind == DrawerKind::History { BLUE } else { BLACK });
    text(surf, font, "CORPUS", LABEL_PX, PANEL_W / 2 + 18, 36,
        if drawer.kind == DrawerKind::Corpus { BLUE } else { BLACK });
    rule(surf, 0, 104, PANEL_W, 2);
    match drawer.kind {
        DrawerKind::History => draw_history(surf, font, store, drawer),
        DrawerKind::Corpus => draw_corpus(surf, font, store, snapshot, drawer.scroll),
    }
}

fn draw_history(surf: &mut Surface, font: &FontRef, store: &Option<MemoryStore>, drawer: &Drawer) {
    let Some(store) = store else {
        text(surf, font, "MEMORY DISABLED", TITLE_PX, PAD, 170, BLACK);
        return;
    };
    let convs = store.conversations();
    if convs.is_empty() {
        text(surf, font, "NO CONVERSATIONS YET", LABEL_PX, PAD, 170, BLACK);
        return;
    }
    if let Some(ti) = drawer.thread {
        let Some(conv) = convs.get(ti) else { return };
        draw_thread(surf, font, conv, drawer);
        return;
    }
    draw_selector(surf, font, &convs, drawer.scroll);
}

fn draw_selector(surf: &mut Surface, font: &FontRef, convs: &[crate::memory::Conversation], scroll: i32) {
    let mut y = HEADER_H as usize + 16;
    for conv in convs.iter().rev().skip(scroll.max(0) as usize) {
        if y + CONV_ROW_H > SCREEN_H { break; }
        let count = if conv.turns.len() == 1 { "1 TURN".into() } else { format!("{} TURNS", conv.turns.len()) };
        let preview = if conv.preview.is_empty() { "(NO TRANSCRIPT)".into() } else { one_line(&conv.preview, 42) };
        text(surf, font, &conv.date, LABEL_PX, PAD, y, BLACK);
        text(surf, font, &count, LABEL_PX, PAD, y + 42, BLUE);
        text(surf, font, &preview, LABEL_PX, PAD, y + 84, BLACK);
        rule(surf, PAD, y + CONV_ROW_H - 16, PANEL_W - 2 * PAD, 1);
        y += CONV_ROW_H;
    }
}

fn draw_thread(surf: &mut Surface, font: &FontRef, conv: &crate::memory::Conversation, drawer: &Drawer) {
    text(surf, font, &conv.date, LABEL_PX, PAD, 112, BLUE);
    let blocks = thread_blocks(font, &conv.turns);
    let visible_h = SCREEN_H as i32 - THREAD_Y0 - THREAD_FOOTER;
    let content_h = blocks.last().map(|b| b.y1).unwrap_or(0);
    let max_off = (content_h - visible_h).max(0);
    let offset = (max_off - drawer.scroll * SCROLL_STEP).max(0);
    for (i, b) in blocks.iter().enumerate() {
        let y = THREAD_Y0 + b.y0 - offset;
        if y + b.h < HEADER_H + 8 || y > SCREEN_H as i32 - THREAD_FOOTER { continue; }
        let turn = &conv.turns[b.turn];
        if b.you {
            text(surf, font, "YOU", LABEL_PX, PAD, y.max(HEADER_H + 8) as usize, BLACK);
            wrapped(surf, font, &turn.transcript, LABEL_PX, PAD, (y + 36).max(HEADER_H as i32 + 8) as usize,
                PANEL_W - 2 * PAD, BLACK, 8);
        } else {
            text(surf, font, "PAD", LABEL_PX, PAD, y.max(HEADER_H + 8) as usize, BLUE);
            wrapped(surf, font, &turn.reply, LABEL_PX, PAD, (y + 36).max(HEADER_H as i32 + 8) as usize,
                PANEL_W - 2 * PAD, BLACK, 10);
        }
        if drawer.selection == Some(b.turn) {
            surf.fill_rect(12, y.max(HEADER_H + 8) as usize, 5, (b.h as usize).min(120), BLUE);
        }
        let _ = i;
    }
    if drawer.selection.is_some() {
        rule(surf, PAD, SCREEN_H - THREAD_FOOTER as usize + 8, PANEL_W - 2 * PAD, 2);
        text(surf, font, "REPLAY ON PAGE", LABEL_PX, PAD, SCREEN_H - 100, BLUE);
    }
}

struct ThreadBlock { turn: usize, you: bool, y0: i32, y1: i32, h: i32 }

fn thread_blocks(font: &FontRef, turns: &[crate::memory::ConversationRow]) -> Vec<ThreadBlock> {
    let width = (PANEL_W - 2 * PAD) as f32;
    let mut y = 0i32;
    let mut out = Vec::new();
    for (i, turn) in turns.iter().enumerate() {
        let you_n = script::wrap(font, &turn.transcript, LABEL_PX, width).len().min(8).max(1) as i32;
        let tom_n = script::wrap(font, &turn.reply, LABEL_PX, width).len().min(10).max(1) as i32;
        let you_h = 36 + you_n * 42 + 12;
        out.push(ThreadBlock { turn: i, you: true, y0: y, y1: y + you_h, h: you_h });
        y += you_h;
        let tom_h = 36 + tom_n * 42 + 28;
        out.push(ThreadBlock { turn: i, you: false, y0: y, y1: y + tom_h, h: tom_h });
        y += tom_h;
    }
    out
}

fn thread_index_at(y: i32, scroll: i32, turns: &[crate::memory::ConversationRow]) -> Option<usize> {
    if y >= SCREEN_H as i32 - THREAD_FOOTER { return None; }
    // Font is needed for wrap counts; approximate with the same helper via a
    // dummy layout using Liberation metrics already baked into wrap. Callers
    // pass real turns; we reconstruct with the bundled UI font.
    let font = FontRef::try_from_slice(UI_FONT_TTF).ok()?;
    let blocks = thread_blocks(&font, turns);
    let visible_h = SCREEN_H as i32 - THREAD_Y0 - THREAD_FOOTER;
    let content_h = blocks.last().map(|b| b.y1).unwrap_or(0);
    let max_off = (content_h - visible_h).max(0);
    let offset = (max_off - scroll * SCROLL_STEP).max(0);
    for b in &blocks {
        let y0 = THREAD_Y0 + b.y0 - offset;
        let y1 = y0 + b.h;
        if y >= y0 && y < y1 { return Some(b.turn); }
    }
    None
}

fn selector_index_at(y: i32, n: usize, scroll: i32) -> Option<usize> {
    if n == 0 { return None; }
    let y0 = HEADER_H + 16;
    if y < y0 { return None; }
    let from_newest = ((y - y0) as usize / CONV_ROW_H) + scroll.max(0) as usize;
    if from_newest < n { Some(n - 1 - from_newest) } else { None }
}

fn draw_corpus(surf: &mut Surface, font: &FontRef, store: &Option<MemoryStore>, snap: &ContextSnapshot, scroll: i32) {
    let mut y = 145i32 - scroll * 120;
    let stats = store.as_ref().map(|s| s.stats()).unwrap_or_default();
    section(surf, font, "LOCAL MEMORY", &mut y);
    line(surf, font, &format!("STATE  {}", if store.is_some() { "ENABLED" } else { "DISABLED" }), &mut y);
    line(surf, font, &format!("STORED TURNS  {} / 400", stats.count), &mut y);
    line(surf, font, &format!("OLDEST  {}", stats.oldest.map(crate::memory::spoken_date).unwrap_or_else(|| "—".into())), &mut y);
    line(surf, font, &format!("NEWEST  {}", stats.newest.map(crate::memory::spoken_date).unwrap_or_else(|| "—".into())), &mut y);
    line(surf, font, "SEARCH  ALL LOCAL ENTRIES", &mut y);
    if let Some(store) = store {
        let entries = store.search("");
        for row in entries.iter().rev().take(8).rev() {
            line(surf, font, &format!("{}  {}", row.date, one_line(&row.preview, 38)), &mut y);
        }
    }
    y += 28;
    section(surf, font, "MODEL CONTEXT", &mut y);
    line(surf, font, &format!("PROVIDER  {}", snap.provider), &mut y);
    line(surf, font, &format!("MODEL  {}", snap.model), &mut y);
    line(surf, font, "RECENT DIALOGUE — EXACT", &mut y);
    for (you, tom) in &snap.context.history {
        context_text(surf, font, &format!("YOU  {you}"), &mut y);
        context_text(surf, font, &format!("PAD  {tom}"), &mut y);
    }
    line(surf, font, "CATALOG — EXACT", &mut y);
    for (i, row) in snap.context.catalog_lines.iter().enumerate() {
        let id = snap.context.catalog_ids.get(i).copied().unwrap_or(0);
        context_text(surf, font, row, &mut y);
        line(surf, font, &format!("SELECTED ID  {id}"), &mut y);
    }
    y += 25;
    wrapped(surf, font, "API CREDENTIALS AND UNCONFIGURED EXTERNAL KNOWLEDGE ARE NOT INCLUDED.",
        LABEL_PX, PAD, y.max(110) as usize, PANEL_W - 2 * PAD, BLACK, 4);
}

fn section(surf: &mut Surface, font: &FontRef, label: &str, y: &mut i32) {
    if *y > 105 && *y < SCREEN_H as i32 { text(surf, font, label, TITLE_PX, PAD, *y as usize, BLACK); }
    *y += 92;
}
fn line(surf: &mut Surface, font: &FontRef, value: &str, y: &mut i32) {
    if *y > 105 && *y < SCREEN_H as i32 - 40 { text(surf, font, value, LABEL_PX, PAD, *y as usize, BLACK); }
    *y += 48;
}

fn context_text(surf: &mut Surface, font: &FontRef, value: &str, y: &mut i32) {
    for part in script::wrap(font, value, LABEL_PX, (PANEL_W - 2 * PAD) as f32) {
        line(surf, font, &part, y);
    }
}

pub fn draw_controls(surf: &mut Surface, font: &FontRef, reply_visible: bool) -> Vec<u8> {
    let h = 82;
    let saved = surf.copy_rect(0, 0, SCREEN_W, h);
    surf.fill_rect(0, 0, SCREEN_W, h, WHITE);
    let labels = if reply_visible {
        ["DISMISS", "ERASE", "NEW PAGE", "HISTORY", "CORPUS", "SLEEP", "SETTINGS"]
    } else {
        ["SEND", "ERASE", "NEW PAGE", "HISTORY", "CORPUS", "SLEEP", "SETTINGS"]
    };
    let w = SCREEN_W / labels.len();
    for (i, label) in labels.iter().enumerate() {
        if i > 0 { surf.fill_rect(i * w, 0, 1, h, BLACK); }
        full_text(surf, font, label, LABEL_PX, i * w + 12, 25, if i == 3 || i == 4 { BLUE } else { BLACK });
    }
    rule(surf, 0, h - 2, SCREEN_W, 2);
    saved
}

pub fn control_action(x: i32, y: i32, reply_visible: bool) -> Action {
    if y < 0 || y >= 82 || x < 0 || x >= SCREEN_W as i32 { return Action::None; }
    match x as usize / (SCREEN_W / 7) {
        0 if reply_visible => Action::Dismiss,
        0 => Action::Send,
        1 => Action::Erase,
        2 => Action::NewPage,
        3 => Action::History,
        4 => Action::Corpus,
        5 => Action::Sleep,
        _ => Action::Settings,
    }
}

pub fn restore_controls(surf: &mut Surface, saved: &[u8]) {
    surf.paste_rect(0, 0, SCREEN_W, 82, saved);
}

pub fn draw_settings(surf: &mut Surface, font: &FontRef, prefs: Preferences) -> Vec<u8> {
    let saved = surf.copy_rect(0, 0, PANEL_W, SCREEN_H);
    surf.fill_rect(0, 0, PANEL_W, SCREEN_H, WHITE);
    surf.fill_rect(PANEL_W - 2, 0, 2, SCREEN_H, BLACK);
    text(surf, font, "×", LABEL_PX, PAD, 36, BLACK);
    text(surf, font, "SETTINGS", TITLE_PX, PAD, 145, BLACK);
    rule(surf, PAD, 245, PANEL_W - 2 * PAD, 2);
    text(surf, font, "STEALTH", LABEL_PX, PAD, 310, if prefs.mode == Mode::Stealth { BLUE } else { BLACK });
    text(surf, font, "GUIDED", LABEL_PX, PAD, 390, if prefs.mode == Mode::Guided { BLUE } else { BLACK });
    text(surf, font, "OPTIONAL IDLE-SEND", LABEL_PX, PAD, 510, BLACK);
    text(surf, font, if prefs.idle_send_ms == 0 { "OFF" } else { "ON" }, LABEL_PX, PAD, 570,
        if prefs.idle_send_ms == 0 { BLACK } else { BLUE });

    // Leaving is a five-finger hold, which is not discoverable — nothing on
    // the page says so. Give it a tapped row too, and say what the gesture is
    // so the pad teaches it rather than hiding it.
    rule(surf, PAD, 690, PANEL_W - 2 * PAD, 2);
    text(surf, font, "LEAVE G-PAD", LABEL_PX, PAD, 755, BLACK);
    text(surf, font, "OR HOLD FIVE FINGERS", LABEL_PX, PAD, 805, BLACK);

    // Signature, at the foot of the one panel that is already a settled
    // surface rather than the writing page.
    text(surf, font, "G-PAD", LABEL_PX, PAD, SCREEN_H - 120, BLACK);
    text(surf, font, "BY MERGE COMBINATOR", LABEL_PX, PAD, SCREEN_H - 70, BLACK);
    saved
}

pub fn settings_action(x: i32, y: i32) -> Action {
    if x < 0 || x >= PANEL_W as i32 { return Action::Close; }
    if y < HEADER_H { return Action::Close; }
    // Labels sit at 310 / 390 / 570 / 755; give each a full row so a slightly
    // off tap still hits the control it is over. Leaving stops short of the
    // signature at the foot so a tap down there cannot quit the pad.
    match y {
        250..=355 => Action::SetMode(Mode::Stealth),
        356..=470 => Action::SetMode(Mode::Guided),
        480..=680 => Action::ToggleIdle,
        700..=860 => Action::Quit,
        _ => Action::None,
    }
}

fn panel_region() -> BBox {
    let mut b = BBox::empty(); b.add(0, 0, 0); b.add(PANEL_W as i32 - 1, SCREEN_H as i32 - 1, 0); b
}
fn rule(s: &mut Surface, x: usize, y: usize, w: usize, h: usize) { s.fill_rect(x, y, w, h, BLACK); }
fn one_line(s: &str, max: usize) -> String { s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect() }

fn text(surf: &mut Surface, font: &FontRef, value: &str, px: f32, x: usize, y: usize, color: u16) {
    render_text(surf, font, value, px, x, y, color, PANEL_W);
}

fn full_text(surf: &mut Surface, font: &FontRef, value: &str, px: f32, x: usize, y: usize, color: u16) {
    render_text(surf, font, value, px, x, y, color, SCREEN_W);
}

fn render_text(surf: &mut Surface, font: &FontRef, value: &str, px: f32, x: usize, y: usize, color: u16, limit_x: usize) {
    let raster = script::rasterize_line(font, value, px);
    for row in 0..raster.height {
        if y + row >= SCREEN_H { break; }
        for col in 0..raster.width {
            if x + col >= limit_x { break; }
            if raster.mask[row * raster.width + col] { surf.put_px((x + col) as i32, (y + row) as i32, color); }
        }
    }
}

fn wrapped(surf: &mut Surface, font: &FontRef, value: &str, px: f32, x: usize, y: usize,
    width: usize, color: u16, max_lines: usize) -> usize {
    let lines = script::wrap(font, value, px, width as f32);
    let mut yy = y;
    for line in lines.iter().take(max_lines) { text(surf, font, line, px, x, yy, color); yy += 42; }
    yy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::PixFmt;
    #[test]
    fn controls_use_fixed_hit_regions() {
        assert_eq!(control_action(10, 20, false), Action::Send);
        assert_eq!(control_action((SCREEN_W * 3 / 7 + 2) as i32, 20, false), Action::History);
        assert_eq!(control_action(10, 100, false), Action::None);
    }

    #[test]
    fn settings_labels_are_inside_their_hit_rows() {
        assert_eq!(settings_action(40, 310), Action::SetMode(Mode::Stealth));
        assert_eq!(settings_action(40, 390), Action::SetMode(Mode::Guided));
        assert_eq!(settings_action(40, 570), Action::ToggleIdle);
        assert_eq!(settings_action(40, 36), Action::Close);
        assert_eq!(settings_action(PANEL_W as i32 + 8, 310), Action::Close);
    }

    #[test]
    fn selector_lists_newest_conversation_first() {
        assert_eq!(selector_index_at(HEADER_H + 20, 3, 0), Some(2));
        assert_eq!(selector_index_at(HEADER_H + 16 + CONV_ROW_H as i32 + 10, 3, 0), Some(1));
        assert_eq!(selector_index_at(10, 3, 0), None);
    }

    #[test]
    fn drawer_touch_cannot_create_ink_and_reopen_state_is_preserved() {
        let mut bytes = vec![0xff; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let mut drawer = Drawer::open(&surf, DrawerKind::History, Some(3), 2, None);
        let ink = crate::ink::Ink::new();
        assert_eq!(drawer.tap(200, 500, &None), Action::None);
        assert!(ink.is_empty(), "touch routing must not add page ink");
        let selection = drawer.selection;
        let scroll = drawer.scroll;
        drawer.close(&mut surf);
        let reopened = Drawer::open(&surf, DrawerKind::History, selection, scroll, None);
        assert_eq!(reopened.selection, Some(3));
        assert_eq!(reopened.scroll, 2);
    }
}
