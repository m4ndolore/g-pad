//! The vault tab — Vellum's markdown notes, read on paper.
//!
//! Vellum ships two read-only device routes built for exactly this surface:
//! `/api/device/v1/notes` lists one shelf — the prefix's subfolders and the
//! notes that live directly in it (bounded, `shelf` alongside so the pad can
//! say what it left off) — and `/api/device/v1/note`
//! returns one body with the frontmatter already stripped. The pad polls the
//! list the way the bridge polls the hub — notes change at human speed, so
//! the cadence is minutes, not seconds — and fetches a body only when a row
//! is ticked. The writer just asked for that page and is waiting for it;
//! nothing else needs the radio.
//!
//! Markdown is lightened, not rendered: heading hashes go, list markers
//! become bullets, emphasis and links unwrap to their text. One face, one
//! color — v1 is about reading, not typesetting.

use ab_glyph::FontRef;

use crate::brief::{json_field, split_objects};
use crate::page::{self, BODY_PX, LINE_H, TITLE_LINE_H};

/// One row on the vault tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteMeta {
    pub path: String,
    pub title: String,
    /// Unix epoch, milliseconds — what the vault's stat said.
    pub mtime_ms: i64,
}

/// One folder row: a subfolder of the current prefix that holds at least one
/// readable note (Vellum never lists an empty one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirMeta {
    pub path: String,
    pub name: String,
    pub count: usize,
}

/// What the vault listing holds right now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vault {
    /// Where in the vault this listing stands. Empty at the root. Survives
    /// the drawer closing — reopening the tab returns where the reader was.
    pub prefix: String,
    /// The prefix's own subfolders, name-sorted — navigation reads like a
    /// shelf, one step at a time.
    pub dirs: Vec<DirMeta>,
    /// The notes that live directly in the prefix — deeper ones stay behind
    /// their folder rows, which is the whole point of walking shelves.
    pub notes: Vec<NoteMeta>,
    /// The server's count of this shelf's notes — the listing itself is
    /// bounded, so `shelf - notes.len()` never made it to the pad at all.
    pub shelf: usize,
    /// True when these are the last notes we held rather than a fresh poll.
    pub stale: bool,
}

/// One step up from a prefix: `raw/AI` → `raw`, `raw` → the root.
pub fn parent(prefix: &str) -> String {
    match prefix.rsplit_once('/') {
        Some((up, _)) => up.to_string(),
        None => String::new(),
    }
}

/// Does this note live directly on the prefix's shelf? A vault that still
/// lists recursively (an older Vellum) floods the drawer with every file
/// under the walk — the pad keeps only the current shelf, and deeper notes
/// stay behind their folder rows.
pub fn on_shelf(prefix: &str, path: &str) -> bool {
    let rest = if prefix.is_empty() {
        path
    } else {
        match path.strip_prefix(prefix).and_then(|r| r.strip_prefix('/')) {
            Some(r) => r,
            None => return false,
        }
    };
    !rest.is_empty() && !rest.contains('/')
}

/// One fetched note, ready to lay out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Note {
    pub path: String,
    pub title: String,
    pub text: String,
}

// ---- configuration ----------------------------------------------------

/// The vault's base URL, e.g. `https://vellum.example.com`. Unset = the
/// vault tab reports itself unconfigured and no thread ever starts.
pub fn base() -> Option<String> {
    std::env::var("RIDDLE_VELLUM_BASE").ok().filter(|s| !s.trim().is_empty())
}

fn token() -> String {
    std::env::var("RIDDLE_VELLUM_TOKEN").unwrap_or_default()
}

pub fn configured() -> bool {
    base().is_some()
}

/// The device listing endpoint, scoped to a folder when `prefix` names one.
/// The base is configuration; the paths are Vellum's device contract.
pub fn notes_url(base: &str, prefix: &str) -> String {
    let mut url = format!("{}/api/device/v1/notes?limit=50", base.trim_end_matches('/'));
    if !prefix.is_empty() {
        url.push_str("&prefix=");
        url.push_str(&urlencode(prefix));
    }
    url
}

/// Where one note body is fetched. The path rides in the query string, so it
/// is percent-encoded — vault paths carry slashes and spaces.
pub fn note_url(base: &str, path: &str) -> String {
    format!("{}/api/device/v1/note?path={}", base.trim_end_matches('/'), urlencode(path))
}

/// Percent-encode a query value. Unreserved characters pass; everything else
/// is encoded byte-wise, which is always correct even if sometimes verbose.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---- the held listing --------------------------------------------------

/// The listing arrives from a poll, not from the draw path, so the drawer
/// reads the last thing we were told. A vault that cannot be reached keeps
/// its previous contents and marks them stale — the bridge's rule: a stale
/// page that says so beats an empty one.
static HELD: std::sync::Mutex<Option<Vault>> = std::sync::Mutex::new(None);

/// The notes to draw. Empty and non-stale before the first poll.
pub fn held() -> Vault {
    HELD.lock().ok().and_then(|g| g.clone()).unwrap_or_default()
}

pub fn replace(vault: Vault) {
    if let Ok(mut g) = HELD.lock() {
        *g = Some(vault);
    }
}

pub fn mark_stale() {
    if let Ok(mut g) = HELD.lock() {
        let v = g.get_or_insert_with(Vault::default);
        v.stale = true;
    }
}

/// Fetch one listing and hold it, but only if the reader is still standing
/// in that folder — a poll finishing after the reader walked elsewhere must
/// not yank them back.
fn fetch_listing(agent: &ureq::Agent, base: &str, bearer: &str, prefix: &str) -> bool {
    let got = agent
        .get(&notes_url(base, prefix))
        .set("Authorization", bearer)
        .call()
        .ok()
        .and_then(|r| r.into_string().ok());
    match got {
        Some(body) => {
            let (dirs, mut notes, shelf) = parse_listing(&body);
            // Only this shelf's notes reach the drawer — an older vault
            // lists recursively, and the filter is what keeps the folders
            // meaning something.
            notes.retain(|n| on_shelf(prefix, &n.path));
            let shelf = shelf.unwrap_or(notes.len());
            if let Ok(mut g) = HELD.lock() {
                let current = g.as_ref().map(|v| v.prefix.clone()).unwrap_or_default();
                if current == prefix {
                    *g = Some(Vault { prefix: prefix.to_string(), dirs, notes, shelf, stale: false });
                }
            }
            true
        }
        None => {
            mark_stale();
            false
        }
    }
}

/// Start polling the vault, if one is configured. Notes change at human
/// speed: the default cadence is five minutes, floored at thirty seconds.
/// Each cycle refreshes the folder the reader is standing in.
pub fn spawn_poll() {
    let Some(base) = base() else { return };
    let every = std::env::var("RIDDLE_VELLUM_POLL_S")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(300)
        .max(30);
    eprintln!("g-pad: vault polling {base} every {every}s");
    let bearer = format!("Bearer {}", token());
    std::thread::spawn(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(15))
            .build();
        loop {
            let prefix = held().prefix;
            fetch_listing(&agent, &base, &bearer, &prefix);
            std::thread::sleep(std::time::Duration::from_secs(every));
        }
    });
}

/// Walk to a folder: fetch its listing and stand there. Synchronous and
/// short-fused like `fetch_note` — the reader just ticked the folder row and
/// the pause is the shelf loading. On failure the current listing stays,
/// marked stale, and the reader has not moved.
pub fn browse(prefix: &str) -> Result<(), String> {
    let base = base().ok_or_else(|| "no vault configured".to_string())?;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let bearer = format!("Bearer {}", token());
    // Stand in the folder first, so the guard inside `fetch_listing` sees
    // this fetch as current — a slow poll landing later is the stale one.
    let before = held();
    if let Ok(mut g) = HELD.lock() {
        g.get_or_insert_with(Vault::default).prefix = prefix.to_string();
    }
    if fetch_listing(&agent, &base, &bearer, prefix) {
        Ok(())
    } else {
        // Step back where we stood: a failed walk must not leave the reader
        // in a folder whose shelf never arrived.
        if let Ok(mut g) = HELD.lock() {
            if g.as_ref().is_some_and(|v| v.prefix == prefix) {
                *g = Some(Vault { stale: true, ..before });
            }
        }
        Err("vault unreachable".to_string())
    }
}

/// Fetch one note body. Synchronous and short-fused, like `post_nudge`: the
/// writer just ticked the row and is waiting for this exact page, so the
/// pause is the page loading, not the pad freezing.
pub fn fetch_note(path: &str) -> Result<Note, String> {
    let base = base().ok_or_else(|| "no vault configured".to_string())?;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    match agent
        .get(&note_url(&base, path))
        .set("Authorization", &format!("Bearer {}", token()))
        .call()
    {
        Ok(r) => {
            let body = r.into_string().map_err(|e| format!("vault read failed: {e}"))?;
            Ok(parse_note(&body))
        }
        Err(ureq::Error::Status(code, r)) => {
            let detail = r.into_string().unwrap_or_default();
            Err(format!("vault {code}: {}", detail.trim()))
        }
        Err(e) => Err(format!("vault unreachable: {e}")),
    }
}

// ---- annotating a note --------------------------------------------------

/// What the writer's ink on this printed note is doing right now. The flow
/// is the decision box's, pointed at the vault: marks raise the box, a tick
/// sends them, Vellum drafts a revision and holds it, and only a second
/// tick applies it — the same manners as approving an agent's action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Annot {
    /// No marks: the pen has only been reading along.
    #[default]
    Clean,
    /// Ink on the page, not yet sent.
    Marked,
    /// Vellum drafted a revision from the marks and holds it, waiting.
    Proposed { id: String, summary: String },
}

pub fn annotate_url(base: &str) -> String {
    format!("{}/api/device/v1/annotate", base.trim_end_matches('/'))
}

pub fn decision_url(base: &str, id: &str) -> String {
    format!("{}/api/device/v1/annotate/{}/decision", base.trim_end_matches('/'), urlencode(id))
}

/// A vault error body is `{"error": "..."}`; say the message, not the JSON.
fn error_detail(code: u16, body: &str) -> String {
    let msg = json_field(body, "error").unwrap_or_else(|| body.trim().to_string());
    format!("vault {code}: {msg}")
}

/// Send the marked-up page and get back the proposal Vellum now holds:
/// `(id, summary)`. Synchronous like `fetch_note`, but the pause is longer —
/// a model is reading ink — so the box says so before this is called.
pub fn propose(path: &str, png_path: &str) -> Result<(String, String), String> {
    let base = base().ok_or_else(|| "no vault configured".to_string())?;
    let png = std::fs::read(png_path).map_err(|e| format!("no page image: {e}"))?;
    let body = format!(
        "{{\"path\":\"{}\",\"mimeType\":\"image/png\",\"imageBase64\":\"{}\",\"requestId\":\"annotate-{}\"}}",
        crate::bridge::escape_json(path),
        crate::oracle::base64(&png),
        now_ms(),
    );
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(180))
        .build();
    match agent
        .post(&annotate_url(&base))
        .set("Authorization", &format!("Bearer {}", token()))
        .set("Content-Type", "application/json")
        .send_string(&body)
    {
        Ok(r) => {
            let text = r.into_string().map_err(|e| format!("vault read failed: {e}"))?;
            let id = json_field(&text, "id").unwrap_or_default();
            if id.is_empty() {
                return Err("vault sent no proposal".to_string());
            }
            Ok((id, json_field(&text, "summary").unwrap_or_default()))
        }
        Err(ureq::Error::Status(code, r)) => {
            let detail = r.into_string().unwrap_or_default();
            Err(error_detail(code, &detail))
        }
        Err(e) => Err(format!("vault unreachable: {e}")),
    }
}

/// Settle a proposal: `apply` writes the revision into the vault (Vellum
/// banks what stood and reindexes, so every reader of the brain sees the
/// change), `discard` walks away. Ok(true) means the note changed.
pub fn decide(id: &str, decision: &str) -> Result<bool, String> {
    let base = base().ok_or_else(|| "no vault configured".to_string())?;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    match agent
        .post(&decision_url(&base, id))
        .set("Authorization", &format!("Bearer {}", token()))
        .set("Content-Type", "application/json")
        .send_string(&format!("{{\"decision\":\"{}\"}}", crate::bridge::escape_json(decision)))
    {
        Ok(r) => {
            let text = r.into_string().map_err(|e| format!("vault read failed: {e}"))?;
            Ok(json_bool(&text, "applied").unwrap_or(false))
        }
        Err(ureq::Error::Status(code, r)) => {
            let detail = r.into_string().unwrap_or_default();
            Err(error_detail(code, &detail))
        }
        Err(e) => Err(format!("vault unreachable: {e}")),
    }
}

// ---- parsing -----------------------------------------------------------

/// Parse the listing payload. Tolerant like every parser on the pad: a vault
/// that adds fields must not break the reader, a note missing a path is
/// unopenable and skipped, and an old vault sending no `dirs` still lists —
/// there are simply no folders to walk. `shelf` is the server's count of the
/// prefix's own notes before the bound; an old vault sends none.
pub fn parse_listing(json: &str) -> (Vec<DirMeta>, Vec<NoteMeta>, Option<usize>) {
    let mut dirs = Vec::new();
    for block in split_objects(json, "dirs") {
        let path = json_field(&block, "path").unwrap_or_default();
        if path.trim().is_empty() {
            continue;
        }
        let name = match json_field(&block, "name").filter(|n| !n.trim().is_empty()) {
            Some(n) => n,
            None => path.rsplit('/').next().unwrap_or(&path).to_string(),
        };
        let count = json_number(&block, "count").map(|n| n.max(0) as usize).unwrap_or(0);
        dirs.push(DirMeta { path, name, count });
    }
    let mut notes = Vec::new();
    for block in split_objects(json, "notes") {
        let path = json_field(&block, "path").unwrap_or_default();
        if path.trim().is_empty() {
            continue;
        }
        let title = match json_field(&block, "title").filter(|t| !t.trim().is_empty()) {
            Some(t) => t,
            None => path.rsplit('/').next().unwrap_or(&path).to_string(),
        };
        notes.push(NoteMeta { path, title, mtime_ms: json_number(&block, "mtime").unwrap_or(0) });
    }
    // `shelf` sits outside both arrays; their objects carry no such key.
    let shelf = json_number(json, "shelf").map(|n| n.max(0) as usize);
    (dirs, notes, shelf)
}

/// Parse one note body. The text keeps its newlines — `json_field` flattens
/// them for single-line rows, which is exactly wrong for a page of prose.
pub fn parse_note(json: &str) -> Note {
    Note {
        path: json_field(json, "path").unwrap_or_default(),
        title: json_field(json, "title").unwrap_or_default(),
        text: json_text(json, "text").unwrap_or_default(),
    }
}

/// Read one string field, preserving line structure.
fn json_text(block: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let at = block.find(&needle)? + needle.len();
    let rest = block[at..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push_str("  "),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                }
                Some(other) => out.push(other),
                None => break,
            },
            _ => out.push(c),
        }
    }
    Some(out)
}

/// Read one boolean field. `applied` is a bare true/false, which
/// `json_field` cannot see.
fn json_bool(block: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{key}\"");
    let at = block.find(&needle)? + needle.len();
    let rest = block[at..].trim_start().strip_prefix(':')?.trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Read one integer field. `mtime` and `shelf` are numbers, which
/// `json_field` cannot see.
fn json_number(block: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\"");
    let at = block.find(&needle)? + needle.len();
    let rest = block[at..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    digits.parse().ok()
}

/// How old a note is, said the way a person would. The drawer row has room
/// for two or three characters, not a timestamp.
pub fn age(mtime_ms: i64, now_ms: i64) -> String {
    let mins = (now_ms.saturating_sub(mtime_ms) / 60_000).max(0);
    match mins {
        0..=99 => format!("{mins}m"),
        100..=2879 => format!("{}h", mins / 60),
        _ => format!("{}d", mins / (60 * 24)),
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---- markdown, lightened ----------------------------------------------

/// One paragraph of a lightened note: prose, a bullet, or a heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Para {
    pub text: String,
    pub heading: bool,
}

/// Lighten markdown into paragraphs the page can wrap.
///
/// Not a renderer: headings lose their hashes and gain a face, list markers
/// become bullets, emphasis and links unwrap to their text, fence lines
/// vanish (their contents stay — code reads fine in the one face we have).
/// Consecutive prose lines join into one paragraph, the way markdown means
/// them; a list item or heading always stands alone.
pub fn md_paras(text: &str) -> Vec<Para> {
    let mut out: Vec<Para> = Vec::new();
    let mut prose = String::new();
    let mut in_fence = false;
    let flush = |prose: &mut String, out: &mut Vec<Para>| {
        if !prose.trim().is_empty() {
            out.push(Para { text: std::mem::take(prose).trim().to_string(), heading: false });
        } else {
            prose.clear();
        }
    };
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut prose, &mut out);
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            // Code keeps its lines: joining them would destroy it.
            flush(&mut prose, &mut out);
            if !line.trim().is_empty() {
                out.push(Para { text: line.to_string(), heading: false });
            }
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut prose, &mut out);
            continue;
        }
        if let Some(rest) = heading_text(trimmed) {
            flush(&mut prose, &mut out);
            out.push(Para { text: strip_inline(rest), heading: true });
            continue;
        }
        if let Some(rest) = bullet_text(trimmed) {
            flush(&mut prose, &mut out);
            out.push(Para { text: format!("• {}", strip_inline(rest)), heading: false });
            continue;
        }
        let cleaned = strip_inline(trimmed.strip_prefix("> ").unwrap_or(trimmed));
        if !prose.is_empty() {
            prose.push(' ');
        }
        prose.push_str(&cleaned);
    }
    flush(&mut prose, &mut out);
    out
}

/// `## Heading` → `Heading`; anything up to six hashes and a space.
fn heading_text(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) {
        line[hashes..].strip_prefix(' ').map(str::trim)
    } else {
        None
    }
}

/// `- item`, `* item`, `+ item` → `item`. Numbered lists keep their numbers —
/// they carry meaning a bullet would erase.
fn bullet_text(line: &str) -> Option<&str> {
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(m) {
            return Some(rest.trim_start());
        }
    }
    None
}

/// Unwrap inline markers to their text: `**b**`, `__b__`, `` `c` ``,
/// `[text](url)` and `![alt](url)`. Single `*`/`_` stay — stripping those
/// mangles paths, globs, and arithmetic more often than it fixes emphasis.
fn strip_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            continue;
        }
        if c == '_' && i + 1 < chars.len() && chars[i + 1] == '_' {
            i += 2;
            continue;
        }
        if c == '`' {
            i += 1;
            continue;
        }
        if c == '!' && chars.get(i + 1) == Some(&'[') {
            i += 1; // the '[' branch below handles the rest
            continue;
        }
        if c == '[' {
            // [text](url) — keep the text, drop the address.
            if let Some(close) = find_from(&chars, i + 1, ']') {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(paren) = find_from(&chars, close + 2, ')') {
                        out.extend(&chars[i + 1..close]);
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn find_from(chars: &[char], start: usize, needle: char) -> Option<usize> {
    chars[start..].iter().position(|c| *c == needle).map(|p| start + p)
}

// ---- layout ------------------------------------------------------------

/// The gap above a paragraph, and the extra breath above a heading. Public
/// because drawing leaves the same gap under the meta row, so measuring and
/// painting agree on where the body starts.
pub const PARA_GAP: usize = 22;
const HEAD_GAP: usize = 18;
/// A heading line inside a note — set between the body and the page title.
pub const HEAD_PX: f32 = 46.0;

/// A laid-out line of the note. Pagination works on lines, so a page
/// boundary can fall anywhere and nothing is ever cut mid-thought worse
/// than mid-paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineLayout {
    pub text: String,
    pub heading: bool,
    pub height: usize,
}

/// One page of a note, measured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoteLayout {
    pub title_lines: Vec<String>,
    /// The path, tailed — where in the vault this page lives.
    pub meta: String,
    pub lines: Vec<LineLayout>,
    /// Which page this is, 0 the first — a note reads front to back.
    pub page: usize,
    pub pages: usize,
}

fn note_lines(font: &FontRef, note: &Note) -> Vec<LineLayout> {
    let mut paras = md_paras(&note.text);
    // Vellum titles a note by its first heading; rendering that heading again
    // under the title would read as a stutter.
    if paras.first().is_some_and(|p| p.heading && p.text == note.title) {
        paras.remove(0);
    }
    let mut lines: Vec<LineLayout> = Vec::new();
    for (pi, para) in paras.iter().enumerate() {
        let (px, line_h) = if para.heading { (HEAD_PX, TITLE_LINE_H) } else { (BODY_PX, LINE_H) };
        let wrapped = crate::script::wrap(font, &para.text, px, page::content_width() as f32);
        for (i, text) in wrapped.into_iter().enumerate() {
            // The paragraph gap rides above its first line (headings breathe a
            // little more), so a line's text always sits at the foot of its
            // measured block and drawing never has to guess where a gap went.
            let gap_above = if i == 0 && pi > 0 {
                PARA_GAP + if para.heading { HEAD_GAP } else { 0 }
            } else {
                0
            };
            lines.push(LineLayout { text, heading: para.heading, height: gap_above + line_h });
        }
    }
    lines
}

/// Lay out one page of a note. `want_page` is clamped to what exists.
pub fn layout_note_page(font: &FontRef, note: &Note, want_page: usize) -> NoteLayout {
    let title_lines = page::title_lines(font, &note.title);
    let meta = page::tail(&note.path, 60);
    let y = page::HEADER_H + title_lines.len() * TITLE_LINE_H + LINE_H + PARA_GAP;
    let lines = note_lines(font, note);
    let ranges = page::paginate_forward(&lines, y, page::limit(0), |l| l.height);
    let pages = ranges.len();
    let page = want_page.min(pages - 1);
    let shown = lines[ranges[page].clone()].to_vec();
    NoteLayout { title_lines, meta, lines: shown, page, pages }
}

/// How many pages a note runs to, measured exactly as drawing will.
pub fn note_page_count(font: &FontRef, note: &Note) -> usize {
    layout_note_page(font, note, 0).pages
}

/// The reader's footer. Empty on a one-page note.
pub fn footer_label(layout: &NoteLayout) -> String {
    if layout.pages <= 1 {
        return String::new();
    }
    let mut label = format!("page {} of {}", layout.page + 1, layout.pages);
    if layout.page == 0 {
        // The page must teach the gesture, not assume it (the turn page's
        // hardware lesson).
        label.push_str(" · swipe up for more");
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UI_FONT_TTF;

    fn font() -> FontRef<'static> {
        FontRef::try_from_slice(UI_FONT_TTF).unwrap()
    }

    #[test]
    fn parses_the_device_listing_shape() {
        let json = r#"{"notes":[
          {"path":"raw/remarkable/2026/08/30/capture.md","title":"A handwritten question","mtime":1756500000000},
          {"path":"projects/anthink.md","title":"Anthink","mtime":1756400000000}
        ],"shelf":137,"dirs":[
          {"path":"raw/Apple Notes","name":"Apple Notes","count":236},
          {"path":"raw/AI","name":"AI","count":14}
        ]}"#;
        let (dirs, notes, shelf) = parse_listing(json);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].path, "raw/remarkable/2026/08/30/capture.md");
        assert_eq!(notes[0].title, "A handwritten question");
        assert_eq!(notes[0].mtime_ms, 1756500000000);
        assert_eq!(shelf, Some(137));
        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0], DirMeta { path: "raw/Apple Notes".into(), name: "Apple Notes".into(), count: 236 });
        assert_eq!(dirs[1].count, 14);
    }

    #[test]
    fn a_pathless_note_is_skipped_and_a_titleless_one_shows_its_filename() {
        let json = r#"{"notes":[
          {"title":"orphan","mtime":1},
          {"path":"deep/dir/untitled.md","title":"","mtime":2}
        ],"total":2}"#;
        let (dirs, notes, _) = parse_listing(json);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "untitled.md");
        // An old vault sends no dirs at all: the shelf is simply flat.
        assert!(dirs.is_empty());
    }

    #[test]
    fn tolerates_junk() {
        assert_eq!(parse_listing(""), (Vec::new(), Vec::new(), None));
        assert_eq!(parse_listing("{}"), (Vec::new(), Vec::new(), None));
        let (dirs, notes, shelf) = parse_listing(r#"{"notes":[],"shelf":0,"dirs":[]}"#);
        assert!(dirs.is_empty());
        assert!(notes.is_empty());
        assert_eq!(shelf, Some(0));
    }

    #[test]
    fn only_the_shelf_survives_a_recursive_listing() {
        // At the root, only rootless paths stay.
        assert!(on_shelf("", "inbox.md"));
        assert!(!on_shelf("", "raw/deep/capture.md"));
        // Inside a folder, only its own files stay — not its subfolders'.
        assert!(on_shelf("raw", "raw/Hood and DLA.md"));
        assert!(!on_shelf("raw", "raw/AI/agents.md"));
        // A path outside the prefix (or the prefix itself) never lists.
        assert!(!on_shelf("raw", "projects/anthink.md"));
        assert!(!on_shelf("raw", "rawhide.md"));
        assert!(!on_shelf("raw", "raw"));
    }

    #[test]
    fn a_prefix_scopes_the_listing_url_and_a_parent_walks_up() {
        assert_eq!(notes_url("https://v.example.com", ""), "https://v.example.com/api/device/v1/notes?limit=50");
        assert_eq!(
            notes_url("https://v.example.com", "raw/Apple Notes"),
            "https://v.example.com/api/device/v1/notes?limit=50&prefix=raw/Apple%20Notes"
        );
        assert_eq!(parent("raw/Apple Notes"), "raw");
        assert_eq!(parent("raw"), "");
        assert_eq!(parent(""), "");
    }

    #[test]
    fn annotate_urls_and_replies_hold_their_shape() {
        assert_eq!(
            annotate_url("https://v.example.com/"),
            "https://v.example.com/api/device/v1/annotate"
        );
        assert_eq!(
            decision_url("https://v.example.com", "abc 123"),
            "https://v.example.com/api/device/v1/annotate/abc%20123/decision"
        );
        assert_eq!(json_bool(r#"{"applied":true,"path":"a.md"}"#, "applied"), Some(true));
        assert_eq!(json_bool(r#"{"applied":false}"#, "applied"), Some(false));
        assert_eq!(json_bool(r#"{"path":"a.md"}"#, "applied"), None);
        // A vault error body speaks its message, not its JSON.
        assert_eq!(
            error_detail(409, r#"{"error":"the note changed since the markup was read"}"#),
            "vault 409: the note changed since the markup was read"
        );
        assert_eq!(error_detail(500, "gateway fell over"), "vault 500: gateway fell over");
    }

    #[test]
    fn a_note_body_keeps_its_newlines() {
        let json = r##"{"path":"a.md","title":"A","text":"# A\n\nline one\nline two"}"##;
        let note = parse_note(json);
        assert_eq!(note.text, "# A\n\nline one\nline two");
        assert_eq!(note.title, "A");
    }

    #[test]
    fn the_note_url_encodes_the_path() {
        let url = note_url("https://v.example.com/", "raw/a note (draft).md");
        assert_eq!(
            url,
            "https://v.example.com/api/device/v1/note?path=raw/a%20note%20%28draft%29.md"
        );
    }

    #[test]
    fn ages_read_the_way_a_person_says_them() {
        let now = 10_000 * 60_000;
        assert_eq!(age(now - 5 * 60_000, now), "5m");
        assert_eq!(age(now - 3 * 60 * 60_000, now), "3h");
        assert_eq!(age(now - 50 * 60 * 60_000, now), "2d");
        assert_eq!(age(now + 60_000, now), "0m");
    }

    #[test]
    fn markdown_is_lightened_not_rendered() {
        let paras = md_paras(
            "## Findings\n\nThe **fix** worked, see [the PR](https://x/pr/1).\n- first `item`\n- second\n\n1. numbered stays",
        );
        assert_eq!(paras[0], Para { text: "Findings".into(), heading: true });
        assert_eq!(paras[1].text, "The fix worked, see the PR.");
        assert_eq!(paras[2].text, "• first item");
        assert_eq!(paras[3].text, "• second");
        assert_eq!(paras[4].text, "1. numbered stays");
    }

    #[test]
    fn prose_lines_join_but_code_lines_do_not() {
        let paras = md_paras("one line\nsame paragraph\n\n```\nlet a = 1;\nlet b = 2;\n```");
        assert_eq!(paras[0].text, "one line same paragraph");
        assert_eq!(paras[1].text, "let a = 1;");
        assert_eq!(paras[2].text, "let b = 2;");
    }

    #[test]
    fn images_lose_their_bang_and_quotes_lose_their_marker() {
        let paras = md_paras("![a chart](https://x/c.png)\n\n> quoted words");
        assert_eq!(paras[0].text, "a chart");
        assert_eq!(paras[1].text, "quoted words");
    }

    #[test]
    fn single_stars_survive() {
        assert_eq!(strip_inline("glob *.md and a*b"), "glob *.md and a*b");
    }

    #[test]
    fn a_long_note_pages_forward_and_covers_everything() {
        let f = font();
        // Every wrapped line carries the paragraph's number, so no two lines
        // of the note are identical and positions can be compared by value.
        let body: String = (0..60)
            .map(|i| format!("Paragraph {i} carries its marker {i} through every wrapped line {i} \
                so each line {i} of this paragraph {i} stays unique {i} on paper {i} always {i}.\n\n"))
            .collect();
        let note = Note { path: "long.md".into(), title: "A long note".into(), text: body };
        let pages = note_page_count(&f, &note);
        assert!(pages > 1, "60 paragraphs cannot be one page");
        let mut seen = 0usize;
        for p in 0..pages {
            let layout = layout_note_page(&f, &note, p);
            assert_eq!(layout.page, p);
            assert_eq!(layout.pages, pages);
            assert!(!layout.lines.is_empty(), "page {p} is blank");
            seen += layout.lines.len();
        }
        assert_eq!(seen, note_lines(&f, &note).len(), "every line lands on exactly one page");
        // Page 1 starts where page 0 left off — reading front to back.
        let p0_last = layout_note_page(&f, &note, 0).lines.last().cloned().unwrap();
        let all = note_lines(&f, &note);
        let p1_first = layout_note_page(&f, &note, 1).lines.first().cloned().unwrap();
        let i0 = all.iter().position(|l| *l == p0_last).unwrap();
        assert_eq!(all[i0 + 1], p1_first);
    }

    #[test]
    fn the_first_heading_does_not_stutter_under_the_title() {
        let f = font();
        let note = Note {
            path: "a.md".into(),
            title: "Findings".into(),
            text: "# Findings\n\nbody text".into(),
        };
        let layout = layout_note_page(&f, &note, 0);
        assert!(layout.lines.iter().all(|l| l.text != "Findings"));
        assert!(layout.lines.iter().any(|l| l.text == "body text"));
    }

    #[test]
    fn the_footer_teaches_the_gesture_only_on_the_first_page() {
        let layout = NoteLayout { pages: 3, page: 0, ..Default::default() };
        assert_eq!(footer_label(&layout), "page 1 of 3 · swipe up for more");
        let layout = NoteLayout { pages: 3, page: 2, ..Default::default() };
        assert_eq!(footer_label(&layout), "page 3 of 3");
        let layout = NoteLayout { pages: 1, page: 0, ..Default::default() };
        assert_eq!(footer_label(&layout), "");
    }

    #[test]
    fn an_empty_note_still_lays_out_one_page() {
        let f = font();
        let note = Note { path: "e.md".into(), title: "Empty".into(), text: String::new() };
        let layout = layout_note_page(&f, &note, 0);
        assert_eq!(layout.pages, 1);
        assert!(layout.lines.is_empty());
    }
}
