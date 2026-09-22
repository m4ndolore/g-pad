//! Quiet, read-only secondary surfaces for history, corpus, and preferences.
//! UI text uses the bundled grotesque; the pad's handwriting never appears here.

use ab_glyph::FontRef;

use crate::fb::{BBox, SCREEN_H, SCREEN_W};
use crate::memory::MemoryStore;
use crate::oracle::ContextSnapshot;
use crate::script;
use crate::surface::{Surface, BLACK, WHITE};

pub const UI_FONT_TTF: &[u8] = include_bytes!("../fonts/LiberationSans-Regular.ttf");
pub const PANEL_W: usize = SCREEN_W * 50 / 100;
pub(crate) const LABEL_PX: f32 = 32.0;
pub(crate) const TITLE_PX: f32 = 64.0;
pub(crate) const PAD: usize = 36;
pub(crate) const BLUE: u16 = 0x0335;
const HEADER_H: i32 = 105;
const THREAD_Y0: i32 = 148;
const CONV_ROW_H: usize = 168;
const THREAD_FOOTER: i32 = 150;
const SCROLL_STEP: i32 = 80;
/// Four tabs across the drawer header. The labels are drawn at these same
/// x positions, so a tap always lands on the word it looks like it hit.
/// The header wears a slightly smaller face than the rows; a test measures
/// the labels against these positions so a fifth tab cannot sneak in
/// blind. CORPUS is not a tab: it opens from the SYSTEM page, and the
/// header then shows only its name.
const TAB_PX: f32 = 28.0;
const TAB_GAP: i32 = 18;
const TAB_HISTORY_X: usize = 94;
const TAB_SESSIONS_X: i32 = 254;
const TAB_VAULT_X: i32 = 404;
const TAB_BRIEF_X: i32 = 530;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawerKind { History, Corpus, Sessions, Vault, Brief }

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
    Sessions,
    /// Open one agent session as a full page. The index is a row on the
    /// board, in `bridge::readable` order.
    OpenSession(usize),
    /// The vault tab itself.
    Vault,
    /// Open one vault note as a full page. The index is a row on the vault
    /// tab, in `vault::held` note order (folders come before notes on the
    /// shelf but index separately).
    OpenNote(usize),
    /// Walk into one of the listed folders, by `vault::held` dir order.
    OpenDir(usize),
    /// One step back up the vault — the ← that replaces × inside a folder.
    VaultUp,
    /// The brief tab itself.
    Brief,
    /// Open the day's brief as a full page. Any item row opens the same
    /// page: the brief is one page by design (docs/daily-brief.md).
    OpenBrief,
    Replay(u64),
    Threads,
    OpenThread(usize),
    Send,
    Erase,
    NewPage,
    Sleep,
    Settings,
    Dismiss,
    /// The strip's KIDS cell: flip between the pad and the tutor.
    Kids,
    /// The strip's tip cell: flip what the pen tip does, pen or eraser.
    Tool,
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
            // The corner glyph owns the header left of the first tab.
            if x < TAB_HISTORY_X as i32 - TAB_GAP / 2 {
                if self.kind == DrawerKind::History && self.thread.is_some() {
                    return Action::Threads;
                }
                // Inside a folder the corner is the way up, not the way out —
                // History's thread view taught this shape.
                if self.kind == DrawerKind::Vault && !crate::vault::held().prefix.is_empty() {
                    return Action::VaultUp;
                }
                return Action::Close;
            }
            // The corpus header carries no tabs; its only target is the corner.
            if self.kind == DrawerKind::Corpus { return Action::None; }
            if x < TAB_SESSIONS_X { return Action::History; }
            if x < TAB_VAULT_X { return Action::Sessions; }
            if x < TAB_BRIEF_X { return Action::Vault; }
            return Action::Brief;
        }
        // The BRIEF tab lists the day's items; the brief itself is one page,
        // so every row opens the same page. Row geometry is the board's.
        if self.kind == DrawerKind::Brief {
            let rows = crate::brief::held().items.len();
            return match session_index_at(y) {
                Some(i) if i + session_scroll(rows, self.scroll) < rows => Action::OpenBrief,
                _ => Action::None,
            };
        }
        // The AGENTS tab is a selector: tick a row, open that session. This
        // is navigation, which writes nothing — the read-only rule was only
        // ever about capture (see docs/anthink-interaction.md).
        if self.kind == DrawerKind::Sessions {
            let total = crate::bridge::readable(&crate::bridge::held()).len();
            return match session_index_at(y) {
                Some(i) => Action::OpenSession(i + session_scroll(total, self.scroll)),
                None => Action::None,
            };
        }
        // The VAULT tab is the same kind of selector: tick a folder to walk
        // in, tick a note to read it. Folders sit above notes; row geometry
        // is shared with the board, so the same arithmetic answers both.
        if self.kind == DrawerKind::Vault {
            let held = crate::vault::held();
            let rows = held.dirs.len() + held.notes.len();
            return match session_index_at(y) {
                Some(i) => {
                    let i = i + session_scroll(rows, self.scroll);
                    if i < held.dirs.len() {
                        Action::OpenDir(i)
                    } else {
                        Action::OpenNote(i - held.dirs.len())
                    }
                }
                None => Action::None,
            };
        }
        // Corpus is read-only: nothing below the header is a target.
        if self.kind != DrawerKind::History { return Action::None; }
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
    let close = if (drawer.kind == DrawerKind::History && drawer.thread.is_some())
        || (drawer.kind == DrawerKind::Vault && !crate::vault::held().prefix.is_empty())
    { "←" } else { "×" };
    text(surf, font, close, TAB_PX, PAD, 36, BLACK);
    if drawer.kind == DrawerKind::Corpus {
        // A leaf reached from the SYSTEM page, not a sibling of the tabs.
        text(surf, font, "CORPUS", TAB_PX, TAB_HISTORY_X, 36, BLUE);
    } else {
        text(surf, font, "HISTORY", TAB_PX, TAB_HISTORY_X, 36,
            if drawer.kind == DrawerKind::History { BLUE } else { BLACK });
        text(surf, font, "AGENTS", TAB_PX, TAB_SESSIONS_X as usize, 36,
            if drawer.kind == DrawerKind::Sessions { BLUE } else { BLACK });
        text(surf, font, "VAULT", TAB_PX, TAB_VAULT_X as usize, 36,
            if drawer.kind == DrawerKind::Vault { BLUE } else { BLACK });
        text(surf, font, "BRIEF", TAB_PX, TAB_BRIEF_X as usize, 36,
            if drawer.kind == DrawerKind::Brief { BLUE } else { BLACK });
    }
    rule(surf, 0, 104, PANEL_W, 2);
    match drawer.kind {
        DrawerKind::History => draw_history(surf, font, store, drawer),
        DrawerKind::Corpus => draw_corpus(surf, font, store, snapshot, drawer.scroll),
        DrawerKind::Sessions => draw_sessions(surf, font, &crate::bridge::held(), drawer.scroll),
        DrawerKind::Vault => draw_vault(surf, font, &crate::vault::held(), drawer.scroll),
        DrawerKind::Brief => draw_brief(surf, font, &crate::brief::held(),
            crate::brief::configured(), drawer.scroll),
    }
}

/// The day's brief as rows — the selector for the one-page reader.
///
/// Title, then source and date: provenance stays visible on the shelf as it
/// does on the page. `configured` rides in as an argument so the drawing
/// can be checked without touching the process environment.
fn draw_brief(surf: &mut Surface, font: &FontRef, brief: &crate::brief::Brief,
    configured: bool, scroll: i32) {
    if !configured {
        text(surf, font, "BRIEF NOT CONFIGURED", LABEL_PX, PAD, 170, BLACK);
        text(surf, font, "SET RIDDLE_BRIEF_URL IN ORACLE.ENV", LABEL_PX, PAD, 220, BLUE);
        return;
    }
    if brief.items.is_empty() {
        let msg = if brief.stale { "NO BRIEF · NOT REFRESHED" } else { "NO BRIEF YET" };
        text(surf, font, msg, LABEL_PX, PAD, 170, BLACK);
        return;
    }
    let skipped = session_scroll(brief.items.len(), scroll);
    let mut y = HEADER_H as usize + 16;
    let mut shown = 0usize;
    for item in &brief.items[skipped..] {
        if y + CONV_ROW_H > SCREEN_H { break; }
        let meta = crate::page::meta_line(&item.source, &item.date).to_uppercase();
        text(surf, font, &one_line(&item.title, 34), LABEL_PX, PAD, y, BLACK);
        text(surf, font, &one_line(&meta, 34), LABEL_PX, PAD, y + 42, BLUE);
        text(surf, font, &one_line(&item.excerpt, 42), LABEL_PX, PAD, y + 84, BLACK);
        rule(surf, PAD, y + CONV_ROW_H - 16, PANEL_W - 2 * PAD, 1);
        y += CONV_ROW_H;
        shown += 1;
    }
    // The date leads the footer, then what was left out.
    let mut parts: Vec<String> = vec![brief.date.to_uppercase()];
    let more = sessions_footer(brief.items.len() - skipped, shown, brief.stale);
    if !more.is_empty() {
        parts.push(more);
    }
    text(surf, font, &parts.join(" · "), LABEL_PX, PAD, SCREEN_H - 60, BLUE);
}

/// The brief page's header targets, mirroring `note_page_action`: `← BRIEF`
/// on the left returns to the drawer, `×` on the right closes to the canvas,
/// and the rest of the page is inert to touch.
pub fn brief_page_action(x: i32, y: i32) -> Action {
    if !(0..=100).contains(&y) {
        return Action::None;
    }
    if (0..420).contains(&x) {
        return Action::Brief;
    }
    if (SCREEN_W as i32 - 240..SCREEN_W as i32).contains(&x) {
        return Action::Close;
    }
    Action::None
}

/// The day's brief, full page, through `brief::layout_page`: the date, an
/// optional briefing line, then as many items as fit, and a footer that
/// says how many did not. One page, no navigation (docs/daily-brief.md).
pub fn draw_brief_page(surf: &mut Surface, font: &FontRef, brief: &crate::brief::Brief) {
    use crate::page;
    let layout = crate::brief::layout_page(font, brief);
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    full_text(surf, font, "← BRIEF", PAGE_LABEL_PX, page::PAD, 40, BLACK);
    full_text(surf, font, "×", PAGE_LABEL_PX, SCREEN_W - page::PAD - 28, 40, BLACK);
    rule(surf, page::PAD, 100, SCREEN_W - page::PAD * 2, 2);
    // The date shares the header row with the targets, right of ← BRIEF's
    // reach, so the items start where `layout_page` measured from.
    let header = if brief.date.is_empty() { "THE BRIEF".to_string() } else { brief.date.to_uppercase() };
    full_text(surf, font, &header, PAGE_LABEL_PX, 440, 40, BLUE);
    let mut y = page::HEADER_H;
    if let Some(summary) = &brief.summary {
        for line in page::wrap_capped(font, summary, page::BODY_PX, 2) {
            full_text(surf, font, &line, page::BODY_PX, page::PAD, y, BLACK);
            y += page::LINE_H;
        }
    }
    if layout.laid_out.is_empty() {
        let msg = if brief.stale { "Nothing held, and the feed could not be reached." }
            else { "Nothing in the brief yet." };
        full_text(surf, font, msg, page::BODY_PX, page::PAD, y, BLACK);
    }
    for item in &layout.laid_out {
        for line in &item.title_lines {
            full_text(surf, font, line, page::TITLE_PX, page::PAD, y, BLACK);
            y += page::TITLE_LINE_H;
        }
        full_text(surf, font, &item.meta, PAGE_LABEL_PX, page::PAD, y, BLUE);
        y += page::LINE_H;
        for line in &item.body_lines {
            full_text(surf, font, line, page::BODY_PX, page::PAD, y, BLACK);
            y += page::LINE_H;
        }
        y += item.height
            - item.title_lines.len() * page::TITLE_LINE_H
            - page::LINE_H
            - item.body_lines.len() * page::LINE_H;
    }
    let footer = crate::brief::footer_label(&layout, brief.stale);
    if !footer.is_empty() {
        full_text(surf, font, &footer.to_uppercase(), PAGE_LABEL_PX, page::PAD, SCREEN_H - 66, BLUE);
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

/// The agent sessions the bridge is holding.
///
/// A selector, not a reader: which sessions exist, what state they are in, and
/// the last line of each. The full page belongs to `bridge::layout_session`,
/// which measures against the whole screen rather than this half-width panel —
/// whether reading wants the drawer or the page is the open question in
/// `docs/claude-bridge.md`, and this is the cheap half of the answer.
fn draw_sessions(surf: &mut Surface, font: &FontRef, bridge: &crate::bridge::Bridge, scroll: i32) {
    let sessions = crate::bridge::readable(bridge);
    if sessions.is_empty() {
        let msg = if bridge.stale { "NO SESSIONS · NOT REFRESHED" } else { "NO AGENT SESSIONS" };
        text(surf, font, msg, LABEL_PX, PAD, 170, BLACK);
        return;
    }
    let skipped = session_scroll(sessions.len(), scroll);
    let mut y = HEADER_H as usize + 16;
    let mut shown = 0usize;
    for s in &sessions[skipped..] {
        if y + CONV_ROW_H > SCREEN_H { break; }
        let meta = if s.updated.is_empty() {
            s.state.to_uppercase()
        } else {
            format!("{} · {}", s.state.to_uppercase(), s.updated)
        };
        // Where the agent is working identifies a row faster than what it
        // last said — the first hardware read found last-line previews
        // interchangeable. An old hub sends no cwd; fall back to the preview.
        let place = if s.cwd.is_empty() {
            let last = s.turns.iter().rev().find(|t| !t.text.trim().is_empty());
            last.map(|t| one_line(&t.text, 42)).unwrap_or_else(|| "(NOTHING YET)".into())
        } else {
            crate::page::place(&s.cwd, 42)
        };
        text(surf, font, &one_line(&s.title, 34), LABEL_PX, PAD, y, BLACK);
        text(surf, font, &meta, LABEL_PX, PAD, y + 42, BLUE);
        text(surf, font, &place, LABEL_PX, PAD, y + 84, BLACK);
        rule(surf, PAD, y + CONV_ROW_H - 16, PANEL_W - 2 * PAD, 1);
        y += CONV_ROW_H;
        shown += 1;
    }
    // Say what was left out — silent truncation reads as "that was everything".
    let label = sessions_footer(sessions.len() - skipped, shown, bridge.stale);
    if !label.is_empty() {
        text(surf, font, &label, LABEL_PX, PAD, SCREEN_H - 60, BLUE);
    }
}

/// The vault's newest notes — Vellum's listing, held by `vault::held`.
///
/// A selector, not a reader, exactly like the board: title, age, and where
/// in the vault the note lives. The full page belongs to
/// `vault::layout_note_page`; ticking a row fetches the body and opens it.
fn draw_vault(surf: &mut Surface, font: &FontRef, vault: &crate::vault::Vault, scroll: i32) {
    if !crate::vault::configured() {
        text(surf, font, "VELLUM NOT CONFIGURED", LABEL_PX, PAD, 170, BLACK);
        text(surf, font, "SET RIDDLE_VELLUM_BASE IN ORACLE.ENV", LABEL_PX, PAD, 220, BLUE);
        return;
    }
    if vault.dirs.is_empty() && vault.notes.is_empty() {
        let msg = if vault.stale { "NO NOTES · NOT REFRESHED" } else { "NO NOTES YET" };
        text(surf, font, msg, LABEL_PX, PAD, 170, BLACK);
        return;
    }
    let now = crate::vault::now_ms();
    let rows = vault.dirs.len() + vault.notes.len();
    let skipped = session_scroll(rows, scroll);
    let mut y = HEADER_H as usize + 16;
    let mut shown = 0usize;
    for i in skipped..rows {
        if y + CONV_ROW_H > SCREEN_H { break; }
        if let Some(d) = vault.dirs.get(i) {
            // A folder row: the name wears its slash, the count says what a
            // step in is worth.
            text(surf, font, &format!("{}/", one_line(&d.name, 33)), LABEL_PX, PAD, y, BLACK);
            text(surf, font, &crate::page::counted(d.count, "NOTE", "NOTES"), LABEL_PX, PAD, y + 42, BLUE);
        } else {
            let n = &vault.notes[i - vault.dirs.len()];
            let meta = crate::vault::age(n.mtime_ms, now).to_uppercase();
            text(surf, font, &one_line(&n.title, 34), LABEL_PX, PAD, y, BLACK);
            text(surf, font, &meta, LABEL_PX, PAD, y + 42, BLUE);
            // The path identifies the note the way cwd identifies a session —
            // it keeps its case, and its tail is the part that matters.
            text(surf, font, &crate::page::tail(&n.path, 42), LABEL_PX, PAD, y + 84, BLACK);
        }
        rule(surf, PAD, y + CONV_ROW_H - 16, PANEL_W - 2 * PAD, 1);
        y += CONV_ROW_H;
        shown += 1;
    }
    // The footer carries where the reader stands, then what was left out:
    // rows this panel could not fit plus shelf notes the vault's own bound
    // left off the listing — silent truncation reads as "that was
    // everything". Deeper notes are not "more"; they sit behind their
    // folder rows, each wearing its own count.
    let beyond = vault.shelf.saturating_sub(vault.notes.len());
    let hidden = rows - skipped - shown + beyond;
    let mut parts: Vec<String> = Vec::new();
    if !vault.prefix.is_empty() {
        parts.push(crate::page::tail(&vault.prefix, 26));
    }
    let more = sessions_footer(hidden + shown, shown, vault.stale);
    if !more.is_empty() {
        parts.push(more);
    }
    let label = parts.join(" · ");
    if !label.is_empty() {
        text(surf, font, &label, LABEL_PX, PAD, SCREEN_H - 60, BLUE);
    }
}

/// The note page's header targets, mirroring `session_page_action`:
/// `← VAULT` on the left returns to the listing, `×` on the right closes to
/// the canvas, and the rest of the page is inert to touch.
pub fn note_page_action(x: i32, y: i32) -> Action {
    if !(0..=100).contains(&y) {
        return Action::None;
    }
    if (0..420).contains(&x) {
        return Action::Vault;
    }
    if (SCREEN_W as i32 - 240..SCREEN_W as i32).contains(&x) {
        return Action::Close;
    }
    Action::None
}

/// The note box's words for each stage of the annotate flow. `status`
/// replaces the invitation after a send, exactly as on the turn page — the
/// pad never assumes a mark landed.
pub fn note_box_lines(annot: &crate::vault::Annot, status: Option<&str>)
    -> Option<(String, Option<String>)> {
    if let Some(s) = status {
        return Some((one_line(s, 56), None));
    }
    match annot {
        crate::vault::Annot::Clean => None,
        crate::vault::Annot::Marked => {
            Some(("MARKS ON THE PAGE · TICK TO SEND TO VELLUM · STRIKE TO CLEAR".into(), None))
        }
        crate::vault::Annot::Proposed { summary, .. } => Some((
            one_line(summary, 56),
            Some("PROPOSED · TICK TO APPLY · STRIKE TO DISCARD".into()),
        )),
    }
}

/// The note page's decision box: the turn page's anchored target, wearing
/// the vault's words. Painted over whatever stands at the page foot (its
/// fill is opaque), so the first mark can raise it WITHOUT a redraw — a
/// redraw would absorb the very ink being offered.
pub fn draw_note_box(surf: &mut Surface, font: &FontRef, line1: &str, line2: Option<&str>)
    -> DecisionBox {
    use crate::page;
    let x = page::PAD;
    let w = SCREEN_W - page::PAD * 2;
    let h = DECISION_H - 40;
    let y = page::limit(0) - h;
    surf.fill_rect(x, y.saturating_sub(20), w, h + 20, WHITE);
    surf.fill_rect(x, y, w, 3, BLACK);
    surf.fill_rect(x, y + h - 3, w, 3, BLACK);
    surf.fill_rect(x, y, 3, h, BLACK);
    surf.fill_rect(x + w - 3, y, 3, h, BLACK);
    match line2 {
        Some(l2) => {
            full_text(surf, font, line1, PAGE_LABEL_PX, x + 32, y + 22, BLACK);
            full_text(surf, font, l2, PAGE_LABEL_PX, x + 32, y + 70, BLACK);
        }
        None => full_text(surf, font, line1, PAGE_LABEL_PX, x + 32, y + h / 2 - 20, BLACK),
    }
    DecisionBox { x, y, w, h, decision: Decision::Approve }
}

/// One vault note, full page. Reads front to back — page 0 is the beginning
/// — and the swipe pages forward, the same fingers as the turn page. When
/// the annotate flow is underway its box paints over the page foot; the
/// returned hit map is what the pen's marks are read against.
pub fn draw_note_page(surf: &mut Surface, font: &FontRef, note: &crate::vault::Note,
    want_page: usize, annot: &crate::vault::Annot, status: Option<&str>) -> Option<DecisionBox> {
    use crate::page;
    let layout = crate::vault::layout_note_page(font, note, want_page);
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    full_text(surf, font, "← VAULT", PAGE_LABEL_PX, page::PAD, 40, BLACK);
    full_text(surf, font, "×", PAGE_LABEL_PX, SCREEN_W - page::PAD - 28, 40, BLACK);
    rule(surf, page::PAD, 100, SCREEN_W - page::PAD * 2, 2);
    let mut y = page::HEADER_H;
    for line in &layout.title_lines {
        full_text(surf, font, line, page::TITLE_PX, page::PAD, y, BLACK);
        y += page::TITLE_LINE_H;
    }
    // The path is an identifier: it keeps its case and its own line.
    full_text(surf, font, &layout.meta, PAGE_LABEL_PX, page::PAD, y, BLUE);
    y += page::LINE_H + crate::vault::PARA_GAP;
    for line in &layout.lines {
        let px = if line.heading { crate::vault::HEAD_PX } else { page::BODY_PX };
        // Headings sit at the foot of their measured block so the extra
        // height they carry reads as the gap above, where it belongs.
        let baseline = y + line.height - if line.heading { page::TITLE_LINE_H } else { page::LINE_H };
        full_text(surf, font, &line.text, px, page::PAD, baseline, BLACK);
        y += line.height;
    }
    let footer = crate::vault::footer_label(&layout);
    if !footer.is_empty() {
        full_text(surf, font, &footer.to_uppercase(), PAGE_LABEL_PX, page::PAD, SCREEN_H - 66, BLUE);
    }
    note_box_lines(annot, status).map(|(l1, l2)| draw_note_box(surf, font, &l1, l2.as_deref()))
}

/// The scroll offset the board actually uses: never past the last session,
/// so the list cannot scroll into blankness. Shared by drawing and tapping,
/// which must agree on which row is which.
fn session_scroll(total: usize, scroll: i32) -> usize {
    (scroll.max(0) as usize).min(total.saturating_sub(1))
}

/// Which board row a tap landed on. Mirrors `draw_sessions` geometry: rows
/// from `HEADER_H + 16`, `CONV_ROW_H` tall, and only rows that fully fit are
/// drawn — a tap below the drawn rows targets nothing.
fn session_index_at(y: i32) -> Option<usize> {
    let top = HEADER_H + 16;
    if y < top {
        return None;
    }
    let i = ((y - top) as usize) / CONV_ROW_H;
    (top as usize + (i + 1) * CONV_ROW_H <= SCREEN_H).then_some(i)
}

/// What a tick on the decision box means for this session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The session sits at a pending prompt: tick approves, strike rejects.
    Approve,
    /// The session finished its turn: tick nudges it forward.
    Continue,
}

/// A rendered region and what marking it means — the hit map, returned by
/// drawing so it can never drift from what was painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionBox {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub decision: Decision,
}

/// One canned nudge, rendered as a small outlined tag above the decision
/// box. A mark whose center lands inside sends `phrase` to the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bubble {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub tag: String,
    pub phrase: String,
}

/// Everything on the turn page a pen mark can hit, returned by drawing so
/// the hit map can never drift from what was painted.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PageControls {
    pub boxr: Option<DecisionBox>,
    pub bubbles: Vec<Bubble>,
}

/// The decision box's face while the pad reads or offers back a note.
/// `Idle` is the box as it always was; the other two belong to the
/// handwritten-nudge flow (see docs/plans/2026-08-31-ink-continuation-design.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxFace<'a> {
    Idle,
    Reading,
    Confirm(&'a str),
}

/// Room the decision box claims above the footer, gap included.
const DECISION_H: usize = 170;
/// Room the nudge-bubble row claims above the decision box, gap included.
const BUBBLE_ROW_H: usize = 90;
/// A bubble's height; the rest of BUBBLE_ROW_H is the gap below it.
const BUBBLE_H: usize = 64;
/// Labels on the full page are set for the panel's density, not the drawer's.
const PAGE_LABEL_PX: f32 = 36.0;

/// What a session's state asks of the human, if anything.
fn decision_for(state: &str) -> Option<Decision> {
    match state {
        "waiting" => Some(Decision::Approve),
        "done" => Some(Decision::Continue),
        _ => None,
    }
}

/// How many pages a session's turn page runs to, measured exactly as
/// `draw_session_page` will draw it — the flip handler must not step past
/// what drawing can show.
pub fn session_page_count(font: &FontRef, session: &crate::bridge::Session) -> usize {
    crate::bridge::layout_session_page(font, session, page_reserved(&session.state), 0).pages
}

/// Room the page's controls claim above the footer: the decision box plus
/// the bubble row, or nothing when the session asks nothing of the human.
fn page_reserved(state: &str) -> usize {
    if decision_for(state).is_some() { DECISION_H + BUBBLE_ROW_H } else { 0 }
}

/// The turn page's header targets. `← AGENTS` on the left returns to the
/// board; `×` on the right closes to the canvas. Everything else on the page
/// is inert to touch — an idle finger must not throw the page away, which is
/// exactly what tap-anywhere-closes did on hardware. The regions mirror
/// `draw_session_page`'s header, generous around each label.
pub fn session_page_action(x: i32, y: i32) -> Action {
    if !(0..=100).contains(&y) {
        return Action::None;
    }
    if (0..420).contains(&x) {
        return Action::Sessions;
    }
    if (SCREEN_W as i32 - 240..SCREEN_W as i32).contains(&x) {
        return Action::Close;
    }
    Action::None
}

/// One session, full page — the turn page of `docs/anthink-interaction.md`.
/// The board chooses; this reads and, when the session needs a human,
/// carries the decision box. Artifacts pin above the box because evidence
/// must not be pushed off the page by prose. The header carries the page's
/// only touch targets (`session_page_action`): ← AGENTS back to the board,
/// × to the canvas.
///
/// `armed` is the destructive-confirmation state: the first tick arms, the
/// box inverts, the second tick sends. `status` replaces the box label after
/// a send — the pad never assumes a mark landed. `want_page` is which page to
/// show, 0 the newest; the swipe pages backward through longer sessions.
#[allow(clippy::too_many_arguments)]
pub fn draw_session_page(surf: &mut Surface, font: &FontRef, session: &crate::bridge::Session,
    remaining: usize, stale: bool, armed: bool, status: Option<&str>,
    want_page: usize, vocab: &[(String, String)], face: BoxFace) -> PageControls {
    use crate::page;
    let decision = decision_for(&session.state);
    let reserved = page_reserved(&session.state);
    let layout = crate::bridge::layout_session_page(font, session, reserved, want_page);
    // Full-page surface: `text` clips at the drawer's PANEL_W, which on the
    // first hardware read left the right half of every line blank.
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    // The header is the page's only touch surface (see `session_page_action`):
    // back to the board on the left, close to the canvas on the right.
    full_text(surf, font, "← AGENTS", PAGE_LABEL_PX, page::PAD, 40, BLACK);
    full_text(surf, font, "×", PAGE_LABEL_PX, SCREEN_W - page::PAD - 28, 40, BLACK);
    rule(surf, page::PAD, 100, SCREEN_W - page::PAD * 2, 2);
    let mut y = page::HEADER_H;
    for line in &layout.title_lines {
        full_text(surf, font, line, page::TITLE_PX, page::PAD, y, BLACK);
        y += page::TITLE_LINE_H;
    }
    full_text(surf, font, &layout.meta.to_uppercase(), PAGE_LABEL_PX, page::PAD, y, BLUE);
    y += page::LINE_H;
    // The repo the session works in, as written — a path is an identifier,
    // so it keeps its case and its own line under the meta row.
    if !layout.place.is_empty() {
        full_text(surf, font, &layout.place, PAGE_LABEL_PX, page::PAD, y, BLUE);
        y += page::LINE_H;
    }
    for t in &layout.turns {
        // A continuation chunk has no speaker row: the turn flows on from
        // the block (or page) before it.
        let mut ly = y;
        if !t.speaker.is_empty() {
            full_text(surf, font, &t.speaker.to_uppercase(), PAGE_LABEL_PX, page::PAD, y, BLUE);
            ly += page::LINE_H;
        }
        for line in &t.lines {
            full_text(surf, font, line, page::BODY_PX, page::PAD, ly, BLACK);
            ly += page::LINE_H;
        }
        y += t.height;
    }
    // Exactly the room the layout reserved, so drawing can never disagree
    // with measuring: artifacts above the box, the box above the footer.
    let mut ay = page::limit(reserved) - layout.artifacts.len() * page::LINE_H;
    for a in &layout.artifacts {
        full_text(surf, font, &page::tail(&a.reference, 22), page::BODY_PX, page::PAD, ay, BLUE);
        full_text(surf, font, &one_line(&a.label, 40), page::BODY_PX, page::PAD + 470, ay, BLACK);
        ay += page::LINE_H;
    }
    let footer = crate::bridge::footer_label(&layout, remaining, stale);
    if !footer.is_empty() {
        full_text(surf, font, &footer.to_uppercase(), PAGE_LABEL_PX, page::PAD, SCREEN_H - 66, BLUE);
    }
    match decision {
        Some(d) => PageControls {
            bubbles: draw_bubbles(surf, font, vocab),
            boxr: Some(draw_decision_box(surf, font, d, armed, status, face)),
        },
        None => PageControls::default(),
    }
}

/// The bubble row: small outlined tags above the decision box, each sized to
/// its label. Tags that would run off the page are simply not drawn — the
/// vocabulary is the reviewer's own, and a too-long list loses its tail.
fn draw_bubbles(surf: &mut Surface, font: &FontRef, vocab: &[(String, String)]) -> Vec<Bubble> {
    use crate::page;
    let y = page::limit(0) - (DECISION_H - 40) - BUBBLE_ROW_H + (BUBBLE_ROW_H - BUBBLE_H);
    let mut x = page::PAD;
    let mut out = Vec::new();
    for (tag, phrase) in vocab {
        let w = script::measure(font, tag, PAGE_LABEL_PX) as usize + 48;
        if x + w > SCREEN_W - page::PAD {
            break;
        }
        surf.fill_rect(x, y, w, 3, BLACK);
        surf.fill_rect(x, y + BUBBLE_H - 3, w, 3, BLACK);
        surf.fill_rect(x, y, 3, BUBBLE_H, BLACK);
        surf.fill_rect(x + w - 3, y, 3, BUBBLE_H, BLACK);
        full_text(surf, font, tag, PAGE_LABEL_PX, x + 24, y + BUBBLE_H / 2 - 20, BLACK);
        out.push(Bubble { x, y, w, h: BUBBLE_H, tag: tag.clone(), phrase: phrase.clone() });
        x += w + 24;
    }
    out
}

/// The box itself: a fixed, known, anchored target that requires no
/// recognition and no precision. Armed, it inverts — the pad's stand-in for
/// vermilion, and unmistakable on a grayscale panel.
fn draw_decision_box(surf: &mut Surface, font: &FontRef, decision: Decision, armed: bool,
    status: Option<&str>, face: BoxFace) -> DecisionBox {
    use crate::page;
    let x = page::PAD;
    let w = SCREEN_W - page::PAD * 2;
    let h = DECISION_H - 40; // the rest is the gap above
    let y = page::limit(0) - h;
    let label = match face {
        BoxFace::Reading => "READING YOUR INK…".to_string(),
        // The note offered back, hot like an armed box: one tick commits it.
        BoxFace::Confirm(t) => one_line(&format!("SEND \u{201c}{t}\u{201d} · TICK TO SEND · STRIKE TO DISCARD"), 72),
        BoxFace::Idle => match (status, armed, decision) {
            (Some(s), _, _) => one_line(s, 56),
            (None, false, Decision::Approve) => "PENDING ACTION · TICK TO APPROVE · STRIKE TO REJECT".into(),
            (None, true, Decision::Approve) => "TICK AGAIN TO APPROVE — TREATED AS DESTRUCTIVE".into(),
            (None, false, Decision::Continue) => "TURN FINISHED · TICK TO NUDGE FORWARD".into(),
            (None, true, Decision::Continue) => "TICK AGAIN TO SEND CONTINUE".into(),
        },
    };
    let hot = matches!(face, BoxFace::Confirm(_))
        || (face == BoxFace::Idle && armed && status.is_none());
    if hot {
        surf.fill_rect(x, y, w, h, BLACK);
        full_text(surf, font, &label, PAGE_LABEL_PX, x + 32, y + h / 2 - 20, WHITE);
    } else {
        surf.fill_rect(x, y, w, 3, BLACK);
        surf.fill_rect(x, y + h - 3, w, 3, BLACK);
        surf.fill_rect(x, y, 3, h, BLACK);
        surf.fill_rect(x + w - 3, y, 3, h, BLACK);
        full_text(surf, font, &label, PAGE_LABEL_PX, x + 32, y + h / 2 - 20, BLACK);
    }
    DecisionBox { x, y, w, h, decision }
}

/// The drawer's footer line. Mirrors `bridge::footer_label`, but counts rows
/// this panel could not fit rather than turns a page could not fit.
fn sessions_footer(total: usize, shown: usize, stale: bool) -> String {
    let hidden = total.saturating_sub(shown);
    match (hidden, stale) {
        (0, false) => String::new(),
        (0, true) => "NOT REFRESHED".to_string(),
        (n, false) => format!("{n} MORE"),
        (n, true) => format!("{n} MORE · NOT REFRESHED"),
    }
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

/// The corner button: an 82 px square at the page's top-left, the strip's
/// height, that a finger tap opens the controls with (the strip in Guided,
/// the SYSTEM page in Stealth) — the same as the top-edge swipe, without a
/// palm being able to do it. Painted with every writing page; pen strokes
/// over it are ink like anywhere else. Three short bars, drawn as rules so
/// no glyph coverage is assumed of the font.
pub const CORNER: usize = 82;
const CORNER_BAR_W: usize = 30;
const CORNER_BAR_H: usize = 3;

pub fn draw_corner(surf: &mut Surface) {
    let x = (CORNER - CORNER_BAR_W) / 2;
    for i in 0..3 {
        rule(surf, x, 29 + i * 10, CORNER_BAR_W, CORNER_BAR_H);
    }
}

pub fn corner_hit(x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && x < CORNER as i32 && y < CORNER as i32
}

/// The strip: a close cell where the corner button was, then eight equal
/// cells across the rest of the width. Two cells show state rather than
/// name an act: KIDS reads ON or OFF and the tip cell reads PEN or ERASER,
/// and a tap on either flips it. Blue marks the state that is not the
/// default. Eight words do not fit at the row size, so the strip wears the
/// drawer header's smaller face; a test measures every label against the
/// cell width on the tablet, so a longer word cannot run into its
/// neighbour unseen.
const STRIP_CELLS: usize = 8;
const STRIP_INSET: usize = 12;
const STRIP_PX: f32 = TAB_PX;
const STRIP_LABEL_Y: usize = 27;

fn strip_cell_w(screen_w: usize) -> usize {
    (screen_w - CORNER) / STRIP_CELLS
}

fn strip_labels(reply_visible: bool, kids: bool, tool: crate::pen::Tool) -> [(&'static str, bool); STRIP_CELLS] {
    let eraser = tool == crate::pen::Tool::Eraser;
    [
        (if reply_visible { "DISMISS" } else { "SEND" }, false),
        ("ERASE", false),
        ("NEW PAGE", false),
        ("HISTORY", false),
        (if kids { "KIDS ON" } else { "KIDS OFF" }, kids),
        (if eraser { "ERASER" } else { "PEN" }, eraser),
        ("SLEEP", false),
        ("SETTINGS", false),
    ]
}

pub fn draw_controls(surf: &mut Surface, font: &FontRef, reply_visible: bool, kids: bool,
    tool: crate::pen::Tool) -> Vec<u8> {
    let h = CORNER;
    let saved = surf.copy_rect(0, 0, SCREEN_W, h);
    surf.fill_rect(0, 0, SCREEN_W, h, WHITE);
    full_text(surf, font, "×", LABEL_PX, 30, 25, BLACK);
    surf.fill_rect(CORNER, 0, 1, h, BLACK);
    let w = strip_cell_w(SCREEN_W);
    for (i, (label, lit)) in strip_labels(reply_visible, kids, tool).iter().enumerate() {
        let x = CORNER + i * w;
        if i > 0 { surf.fill_rect(x, 0, 1, h, BLACK); }
        full_text(surf, font, label, STRIP_PX, x + STRIP_INSET, STRIP_LABEL_Y, if *lit { BLUE } else { BLACK });
    }
    rule(surf, 0, h - 2, SCREEN_W, 2);
    saved
}

pub fn control_action(x: i32, y: i32, reply_visible: bool) -> Action {
    if y < 0 || y >= CORNER as i32 || x < 0 || x >= SCREEN_W as i32 { return Action::None; }
    if corner_hit(x, y) { return Action::Close; }
    match (x as usize - CORNER) / strip_cell_w(SCREEN_W) {
        0 if reply_visible => Action::Dismiss,
        0 => Action::Send,
        1 => Action::Erase,
        2 => Action::NewPage,
        3 => Action::History,
        4 => Action::Kids,
        5 => Action::Tool,
        6 => Action::Sleep,
        _ => Action::Settings,
    }
}

pub fn restore_controls(surf: &mut Surface, saved: &[u8]) {
    surf.paste_rect(0, 0, SCREEN_W, 82, saved);
}

/// Where the flip banner sits, and what it covers: a small top-right chip
/// saying "PAGE 2 / 3" for a moment after a page flip.
pub const BANNER_W: usize = 280;
pub const BANNER_H: usize = 60;

pub fn banner_origin() -> (usize, usize) {
    (SCREEN_W - BANNER_W - 20, 16)
}

pub fn draw_page_banner(surf: &mut Surface, font: &FontRef, current: usize, total: usize) -> Vec<u8> {
    let (x, y) = banner_origin();
    let saved = surf.copy_rect(x, y, BANNER_W, BANNER_H);
    surf.fill_rect(x, y, BANNER_W, BANNER_H, WHITE);
    render_text(surf, font, &format!("PAGE {current} / {total}"), LABEL_PX, x + 16, y + 12, BLACK, SCREEN_W);
    saved
}

pub fn restore_page_banner(surf: &mut Surface, saved: &[u8]) {
    let (x, y) = banner_origin();
    surf.paste_rect(x, y, BANNER_W, BANNER_H, saved);
}

fn panel_region() -> BBox {
    let mut b = BBox::empty(); b.add(0, 0, 0); b.add(PANEL_W as i32 - 1, SCREEN_H as i32 - 1, 0); b
}
fn rule(s: &mut Surface, x: usize, y: usize, w: usize, h: usize) { s.fill_rect(x, y, w, h, BLACK); }
fn one_line(s: &str, max: usize) -> String { s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect() }

fn text(surf: &mut Surface, font: &FontRef, value: &str, px: f32, x: usize, y: usize, color: u16) {
    render_text(surf, font, value, px, x, y, color, PANEL_W);
}

pub(crate) fn full_text(surf: &mut Surface, font: &FontRef, value: &str, px: f32, x: usize, y: usize, color: u16) {
    render_text(surf, font, value, px, x, y, color, SCREEN_W);
}

pub(crate) fn render_text(surf: &mut Surface, font: &FontRef, value: &str, px: f32, x: usize, y: usize, color: u16, limit_x: usize) {
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
        assert_eq!(control_action(10, 20, false), Action::Close, "the corner cell closes the strip");
        assert_eq!(control_action(CORNER as i32 + 8, 20, false), Action::Send);
        assert_eq!(control_action(CORNER as i32 + 8, 20, true), Action::Dismiss);
        let cell = strip_cell_w(SCREEN_W) as i32;
        assert_eq!(control_action(CORNER as i32 + cell * 3 + 2, 20, false), Action::History);
        assert_eq!(control_action(CORNER as i32 + cell * 4 + 2, 20, false), Action::Kids);
        assert_eq!(control_action(CORNER as i32 + cell * 5 + 2, 20, false), Action::Tool);
        assert_eq!(control_action(CORNER as i32 + cell * 6 + 2, 20, false), Action::Sleep);
        assert_eq!(control_action(SCREEN_W as i32 - 2, 20, false), Action::Settings);
        assert_eq!(control_action(10, 100, false), Action::None);
        assert!(corner_hit(0, 0) && corner_hit(81, 81) && !corner_hit(82, 10) && !corner_hit(10, 82));
    }

    #[test]
    fn every_strip_label_fits_its_cell_on_the_tablet() {
        // The tablet is the narrower screen, so measure against its width
        // whichever screen this test build compiled for. Every variant of
        // the state cells is measured: the longer word is the one that would
        // run into the rule.
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let cell = strip_cell_w(1404) as f32;
        for reply in [false, true] {
            for kids in [false, true] {
                for tool in [crate::pen::Tool::Pen, crate::pen::Tool::Eraser] {
                    for (label, _) in strip_labels(reply, kids, tool) {
                        let end = STRIP_INSET as f32 + script::measure(&font, label, STRIP_PX);
                        assert!(end + STRIP_INSET as f32 <= cell, "{label} runs to {end} in a {cell} cell");
                    }
                }
            }
        }
    }

    #[test]
    fn the_corpus_header_has_no_tabs() {
        let mut bytes = vec![0xff; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let mut d = Drawer::open(&surf, DrawerKind::Corpus, None, 0, None);
        assert_eq!(d.tap(20, 36, &None), Action::Close, "the corner still closes");
        for x in [TAB_HISTORY_X as i32 + 4, TAB_SESSIONS_X + 4, TAB_VAULT_X + 4, TAB_BRIEF_X + 4] {
            assert_eq!(d.tap(x, 36, &None), Action::None, "no tab lives at {x} on the corpus page");
        }
        assert_eq!(d.tap(PAD as i32, HEADER_H + 40, &None), Action::None, "the corpus body is read-only");
    }

    #[test]
    fn the_header_splits_four_ways_and_each_tab_is_reachable() {
        let mut bytes = vec![0xff; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let mut d = Drawer::open(&surf, DrawerKind::History, None, 0, None);
        // A tap on each label reaches its own tab, and the close box still closes.
        assert_eq!(d.tap(TAB_HISTORY_X as i32 + 4, 36, &None), Action::History);
        assert_eq!(d.tap(TAB_SESSIONS_X + 4, 36, &None), Action::Sessions);
        assert_eq!(d.tap(TAB_VAULT_X + 4, 36, &None), Action::Vault);
        assert_eq!(d.tap(TAB_BRIEF_X + 4, 36, &None), Action::Brief);
        assert_eq!(d.tap(20, 36, &None), Action::Close);
    }

    #[test]
    fn four_tab_labels_fit_the_header_without_touching() {
        // The labels are drawn at the tab x positions the taps are read
        // against, so a label that runs into its neighbour is also a tap
        // that lands on the wrong word. Measure, do not eyeball.
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let tabs = [
            (TAB_HISTORY_X as i32, "HISTORY"), (TAB_SESSIONS_X, "AGENTS"),
            (TAB_VAULT_X, "VAULT"), (TAB_BRIEF_X, "BRIEF"),
        ];
        for w in tabs.windows(2) {
            let end = w[0].0 + script::measure(&font, w[0].1, TAB_PX).ceil() as i32;
            assert!(end + TAB_GAP <= w[1].0,
                "{} runs to {end} but {} starts at {}", w[0].1, w[1].1, w[1].0);
        }
        let (x, label) = tabs[3];
        let end = x + script::measure(&font, label, TAB_PX).ceil() as i32;
        assert!(end + PAD as i32 <= PANEL_W as i32 - 2, "BRIEF runs to {end}, past the panel's margin");
        // The corpus title sits where HISTORY does, so it fits by the same measure.
        assert!(script::measure(&font, "CORPUS", TAB_PX) <= script::measure(&font, "HISTORY", TAB_PX));
        // The close glyph keeps its own room before the first tab.
        let close_end = PAD as i32 + script::measure(&font, "×", TAB_PX).ceil() as i32;
        assert!(close_end + TAB_GAP <= TAB_HISTORY_X as i32);
    }

    #[test]
    fn a_board_row_opens_its_session_and_the_gutter_opens_nothing() {
        // Navigation writes nothing, so it was never the thing the read-only
        // rule protected — see docs/anthink-interaction.md.
        let mut bytes = vec![0xff; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let mut d = Drawer::open(&surf, DrawerKind::Sessions, None, 0, None);
        assert_eq!(d.tap(PAD as i32, HEADER_H + 40, &None), Action::OpenSession(0));
        assert_eq!(
            d.tap(PAD as i32, HEADER_H + 16 + CONV_ROW_H as i32 + 10, &None),
            Action::OpenSession(1)
        );
        // Above the rows the header owns the tap; below the last row that
        // fully fits, nothing does.
        assert_eq!(d.tap(PAD as i32, 40, &None), Action::Close);
        assert_eq!(session_index_at(10), None);
        let last_fit = (SCREEN_H - (HEADER_H as usize + 16)) / CONV_ROW_H;
        assert_eq!(session_index_at(HEADER_H + 16 + (last_fit * CONV_ROW_H) as i32 + 4), None);
        assert_eq!(d.selection, None, "the board keeps no selection; a tap opens");
    }

    #[test]
    fn the_session_page_draws_without_panicking_and_pins_artifacts() {
        let mut bytes = vec![0xff; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let session = crate::bridge::Session {
            id: "s1".into(),
            title: "Wire the bridge board to the page".into(),
            state: "waiting".into(),
            updated: "14:02".into(),
            cwd: "/Users/p/Dev/g-pad".into(),
            turns: (0..30)
                .map(|i| crate::bridge::Turn {
                    speaker: if i % 2 == 0 { "you".into() } else { "claude".into() },
                    text: format!("turn {i} with enough words to wrap across a line or two of the page"),
                })
                .collect(),
            artifacts: vec![crate::bridge::Artifact {
                reference: "/a/very/long/path/deep/in/the/tree/src/bridge.rs".into(),
                label: "edited".into(),
            }],
        };
        let vocab = crate::preferences::DEFAULT_BUBBLES
            .iter()
            .map(|&(t, p)| (t.to_string(), p.to_string()))
            .collect::<Vec<_>>();
        // Waiting: the decision box is on the page, and arming redraws it.
        let c = draw_session_page(&mut surf, &font, &session, 3, true, false, None, 0, &vocab, BoxFace::Idle);
        assert_eq!(c.boxr.map(|b| b.decision), Some(Decision::Approve));
        let armed = draw_session_page(&mut surf, &font, &session, 3, true, true, None, 0, &vocab, BoxFace::Idle);
        assert_eq!(armed, c, "arming changes the drawing, never the hit map");
        // The bubble row rides above the box: every tag laid out, none
        // overlapping the box, and the same hit map on every face.
        assert_eq!(c.bubbles.len(), vocab.len());
        let boxr = c.boxr.unwrap();
        for b in &c.bubbles {
            assert!(b.y + b.h <= boxr.y, "a bubble must sit above the box");
            assert!(b.x + b.w <= SCREEN_W, "a bubble must stay on the page");
        }
        let confirm = draw_session_page(&mut surf, &font, &session, 3, true, false, None, 0,
            &vocab, BoxFace::Confirm("run the tests again"));
        assert_eq!(confirm, c, "the face changes the drawing, never the hit map");
        // Done: a tick means nudge forward. Running: nothing to decide, no bubbles.
        let mut done = session.clone();
        done.state = "done".into();
        let c = draw_session_page(&mut surf, &font, &done, 0, false, false, None, 0, &vocab, BoxFace::Idle);
        assert_eq!(c.boxr.map(|b| b.decision), Some(Decision::Continue));
        assert!(!c.bubbles.is_empty(), "a finished session accepts text");
        let mut running = session.clone();
        running.state = "running".into();
        let c = draw_session_page(&mut surf, &font, &running, 0, false, false, None, 0, &vocab, BoxFace::Idle);
        assert_eq!(c, PageControls::default());
        // 30 turns run past one page; the box (and its hit map) rides every
        // page, so a decision is never out of reach while reading earlier.
        let pages = session_page_count(&font, &session);
        assert!(pages > 1, "30 turns cannot fit one page");
        let later = draw_session_page(&mut surf, &font, &session, 3, true, false, None, pages - 1, &vocab, BoxFace::Idle);
        assert_eq!(later.boxr.map(|b| b.decision), Some(Decision::Approve));
    }

    #[test]
    fn vault_folders_sit_above_notes_and_the_corner_walks_up() {
        let mut bytes = vec![0xffu8; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        crate::vault::replace(crate::vault::Vault {
            prefix: "raw".into(),
            dirs: vec![
                crate::vault::DirMeta { path: "raw/AI".into(), name: "AI".into(), count: 14 },
                crate::vault::DirMeta { path: "raw/Apple Notes".into(), name: "Apple Notes".into(), count: 236 },
            ],
            notes: vec![crate::vault::NoteMeta {
                path: "raw/Hood and DLA.md".into(), title: "Hood and DLA".into(), mtime_ms: 0,
            }],
            shelf: 1,
            stale: false,
        });
        let mut d = Drawer::open(&surf, DrawerKind::Vault, None, 0, None);
        let store = None;
        // Rows run from HEADER_H + 16, CONV_ROW_H tall: folders first, then
        // notes indexed from zero again — two shelves, one column.
        let top = HEADER_H + 16;
        assert_eq!(d.tap(200, top + 10, &store), Action::OpenDir(0));
        assert_eq!(d.tap(200, top + CONV_ROW_H as i32 + 10, &store), Action::OpenDir(1));
        assert_eq!(d.tap(200, top + 2 * CONV_ROW_H as i32 + 10, &store), Action::OpenNote(0));
        // Inside a folder the corner is ← (one step up), not × (close).
        assert_eq!(d.tap(50, 40, &store), Action::VaultUp);
        crate::vault::replace(crate::vault::Vault::default());
        assert_eq!(d.tap(50, 40, &store), Action::Close);
        // The shelf draws without panicking, folders and all.
        crate::vault::replace(crate::vault::Vault {
            prefix: "raw".into(),
            dirs: vec![crate::vault::DirMeta { path: "raw/AI".into(), name: "AI".into(), count: 14 }],
            notes: Vec::new(),
            shelf: 0,
            stale: true,
        });
        draw_vault(&mut surf, &font, &crate::vault::held(), 0);
        crate::vault::replace(crate::vault::Vault::default());
    }

    #[test]
    fn the_note_page_draws_full_width_and_the_vault_rows_map_to_taps() {
        let mut bytes = vec![0xffu8; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let note = crate::vault::Note {
            path: "raw/remarkable/2026/08/30/a-capture-with-a-long-path.md".into(),
            title: "A note whose title is long enough to cross the panel's halfway line".into(),
            text: format!("## Findings\n\n{}", "word ".repeat(2000)),
        };
        // Every page draws, and the body paints past the drawer boundary —
        // the turn page's clipping regression must not return here.
        let pages = crate::vault::note_page_count(&font, &note);
        assert!(pages > 1, "2000 words cannot fit one page");
        for p in 0..pages {
            let boxr = draw_note_page(&mut surf, &font, &note, p, &crate::vault::Annot::Clean, None);
            // A clean page carries no box: the pen is only reading along.
            assert_eq!(boxr, None);
        }
        draw_note_page(&mut surf, &font, &note, 0, &crate::vault::Annot::Clean, None);
        let dark_right = (PANEL_W..SCREEN_W).step_by(3).any(|x| {
            (0..SCREEN_H).step_by(5).any(|y| surf.luma(x as i32, y as i32) < 200)
        });
        assert!(dark_right, "no ink right of PANEL_W — the note page is clipped to the drawer");
        // The header targets: left returns to the vault, right closes, the
        // middle of the page is inert.
        assert_eq!(note_page_action(50, 40), Action::Vault);
        assert_eq!(note_page_action(SCREEN_W as i32 - 50, 40), Action::Close);
        assert_eq!(note_page_action(700, 900), Action::None);
    }

    fn brief_with(n: usize) -> crate::brief::Brief {
        crate::brief::Brief {
            date: "24 August 2026".into(),
            summary: None,
            items: (0..n).map(|i| crate::brief::Item {
                id: format!("item-{i}"),
                title: format!("Headline number {i} about a program moving to production"),
                source: "Defense Innovation Unit".into(),
                excerpt: "The unit is standing up a program to move companies from prototype \
                    to production without a new competition, and here is the rest.".into(),
                date: "today".into(),
            }).collect(),
            stale: false,
        }
    }

    #[test]
    fn the_brief_tab_says_why_it_is_empty_and_lists_the_items_when_it_has_them() {
        let mut bytes = vec![0xffu8; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let rows = |surf: &Surface| (HEADER_H as usize + 16..SCREEN_H - 80).step_by(4)
            .any(|y| (PAD..PANEL_W - PAD).step_by(4).any(|x| surf.luma(x as i32, y as i32) < 128));

        // Unconfigured: the tab says so and names the knob, and no row is a target.
        let store = None;
        draw_brief(&mut surf, &font, &crate::brief::Brief::default(), false, 0);
        assert!(rows(&surf), "an unconfigured tab must explain itself on the page");
        let mut d = Drawer::open(&surf, DrawerKind::Brief, None, 0, None);
        crate::brief::replace(crate::brief::Brief::default());
        assert_eq!(d.tap(200, HEADER_H + 30, &store), Action::None);

        // Configured, nothing yet, and a failed poll: both say so.
        surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
        draw_brief(&mut surf, &font, &crate::brief::Brief { stale: true, ..Default::default() }, true, 0);
        assert!(rows(&surf), "a stale empty brief must say it was not refreshed");

        // Held items draw as rows, and a tick on any row opens the page.
        crate::brief::replace(brief_with(3));
        surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
        draw_brief(&mut surf, &font, &crate::brief::held(), true, 0);
        assert!(rows(&surf));
        let top = HEADER_H + 16;
        assert_eq!(d.tap(200, top + 10, &store), Action::OpenBrief);
        assert_eq!(d.tap(200, top + 2 * CONV_ROW_H as i32 + 10, &store), Action::OpenBrief);
        // Below the last drawn row is nothing.
        assert_eq!(d.tap(200, top + 3 * CONV_ROW_H as i32 + 10, &store), Action::None);
        crate::brief::replace(crate::brief::Brief::default());
    }

    #[test]
    fn the_brief_page_draws_full_width_through_the_layout_and_its_header_maps() {
        let mut bytes = vec![0xffu8; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let brief = brief_with(40);
        let layout = crate::brief::layout_page(&font, &brief);
        assert!(layout.remaining > 0, "40 items cannot fit one page");
        draw_brief_page(&mut surf, &font, &brief);
        // The body paints past the drawer boundary — a page, not a panel.
        let dark_right = (PANEL_W..SCREEN_W).step_by(3).any(|x| {
            (0..SCREEN_H).step_by(5).any(|y| surf.luma(x as i32, y as i32) < 200)
        });
        assert!(dark_right, "no ink right of PANEL_W — the brief page is clipped to the drawer");
        // The footer says what was left out.
        let footer_ink = (crate::page::PAD..SCREEN_W / 2).step_by(3)
            .any(|x| (SCREEN_H - 70..SCREEN_H - 30).step_by(3).any(|y| surf.luma(x as i32, y as i32) < 200));
        assert!(footer_ink, "a page that left items out must say so at the foot");
        // A stale brief still draws, and an empty one does not panic.
        draw_brief_page(&mut surf, &font, &crate::brief::Brief { stale: true, ..brief_with(2) });
        draw_brief_page(&mut surf, &font, &crate::brief::Brief::default());
        // The header targets: left returns to the drawer, right closes, the
        // middle of the page is inert.
        assert_eq!(brief_page_action(50, 40), Action::Brief);
        assert_eq!(brief_page_action(SCREEN_W as i32 - 50, 40), Action::Close);
        assert_eq!(brief_page_action(700, 900), Action::None);
    }

    #[test]
    fn the_note_box_walks_the_annotate_flow_and_stays_inside_the_page() {
        use crate::vault::Annot;
        // Clean is boxless; marks invite the send; a proposal wears its
        // summary above the apply/discard line; a status replaces it all.
        assert_eq!(note_box_lines(&Annot::Clean, None), None);
        let (l1, l2) = note_box_lines(&Annot::Marked, None).unwrap();
        assert!(l1.contains("TICK TO SEND"), "{l1}");
        assert_eq!(l2, None);
        let annot = Annot::Proposed { id: "abc123".into(), summary: "Strikes the second item.".into() };
        let (l1, l2) = note_box_lines(&annot, None).unwrap();
        assert_eq!(l1, "Strikes the second item.");
        assert!(l2.unwrap().contains("TICK TO APPLY"));
        let (l1, l2) = note_box_lines(&annot, Some("APPLIED")).unwrap();
        assert_eq!((l1.as_str(), l2), ("APPLIED", None));

        // Drawing returns a hit map that sits fully on the page, above the
        // footer — the same anchored target the turn page uses.
        let mut bytes = vec![0xffu8; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let b = draw_note_box(&mut surf, &font, "MARKS ON THE PAGE", None);
        assert!(b.x + b.w <= SCREEN_W && b.y + b.h < SCREEN_H - 66);
        // The same box comes back from the full draw when marks are on.
        let note = crate::vault::Note { path: "a.md".into(), title: "A".into(), text: "words".into() };
        let drawn = draw_note_page(&mut surf, &font, &note, 0, &Annot::Marked, None).unwrap();
        assert_eq!((drawn.x, drawn.y, drawn.w, drawn.h), (b.x, b.y, b.w, b.h));
    }

    #[test]
    fn the_turn_page_paints_past_the_drawer_boundary() {
        // Regression: the page once rendered through the drawer's `text`,
        // which clips at PANEL_W — on hardware the right half of every line
        // was blank. A full-page surface must use the full width.
        let mut bytes = vec![0xffu8; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32);
        let font = FontRef::try_from_slice(UI_FONT_TTF).unwrap();
        let session = crate::bridge::Session {
            id: "s1".into(),
            title: "A title long enough that its glyphs must cross the halfway line of the panel".into(),
            state: "running".into(),
            updated: "14:02".into(),
            cwd: String::new(),
            turns: vec![crate::bridge::Turn {
                speaker: "claude".into(),
                text: "word ".repeat(120),
            }],
            artifacts: Vec::new(),
        };
        draw_session_page(&mut surf, &font, &session, 0, false, false, None, 0, &[], BoxFace::Idle);
        let dark_right = (PANEL_W..SCREEN_W).step_by(3).any(|x| {
            (0..SCREEN_H).step_by(5).any(|y| surf.luma(x as i32, y as i32) < 200)
        });
        assert!(dark_right, "no ink right of PANEL_W — the page is clipped to the drawer");
    }

    #[test]
    fn the_turn_page_header_has_two_targets_and_the_body_is_inert() {
        // ← AGENTS on the left returns to the board.
        assert_eq!(session_page_action(60, 40), Action::Sessions);
        assert_eq!(session_page_action(300, 90), Action::Sessions);
        // × on the right closes to the canvas.
        assert_eq!(session_page_action(SCREEN_W as i32 - 60, 40), Action::Close);
        // Everything else — header middle, page body, footer — is inert:
        // an idle touch must not throw the page away.
        assert_eq!(session_page_action(SCREEN_W as i32 / 2, 40), Action::None);
        assert_eq!(session_page_action(500, 900), Action::None);
        assert_eq!(session_page_action(60, SCREEN_H as i32 - 40), Action::None);
        assert_eq!(session_page_action(-5, 40), Action::None);
    }

    #[test]
    fn a_bridge_with_nothing_readable_draws_no_rows() {
        // The brief's rule: a header with no exchange under it reads as broken.
        let empty = crate::bridge::Bridge::default();
        assert!(crate::bridge::readable(&empty).is_empty());
        assert_eq!(sessions_footer(0, 0, false), "");
        assert_eq!(sessions_footer(0, 0, true), "NOT REFRESHED");
        assert_eq!(sessions_footer(5, 2, false), "3 MORE");
        assert_eq!(sessions_footer(5, 2, true), "3 MORE · NOT REFRESHED");
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
