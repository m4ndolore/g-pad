//! The Claude bridge — agent sessions as a reading surface.
//!
//! The second reader after the daily brief, and built the same way: measure
//! until the next thing would not fit, then say how much was left behind.
//!
//! The pad has no keyboard and should not grow one. This surface reads; it is
//! not a terminal and not a chat client. Responding is a full capture cycle
//! and ships separately — see `docs/claude-bridge.md`.
//!
//! The one thing this module is careful about is what counts as evidence.
//! Agent prose is commentary; a commit sha is a fact the repository can be
//! asked about on its own. `capture-record.md` splits them, so the types here
//! split them too — a `Turn` holds prose, an `Artifact` holds a reference.
//! The pad never mints artifacts; it renders what the bridge reports.

use ab_glyph::FontRef;

use crate::brief::{json_field, split_objects};
use crate::page::{self, BODY_PX, LINE_H, TITLE_LINE_H};

/// Something outside the model that another system can verify: a commit, a
/// path, an exit status.
///
/// Evidence by reference, never by description. `"fixed the parser"` is prose
/// and belongs in a `Turn`; `a1b2c3d` is a reference and belongs here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// The verifiable reference itself — a sha, a path, a status.
    pub reference: String,
    /// What it is, in the agent's words. Never the evidence, only the label.
    pub label: String,
}

/// One exchange in a session. Prose, and prose only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    /// "you" or the agent's name. Rendered as provenance, like the brief's source.
    pub speaker: String,
    pub text: String,
}

/// A session the bridge is reporting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
    /// running | waiting | done — shown next to the time, never interpreted here.
    pub state: String,
    /// Clock time of the last turn, e.g. "14:02".
    pub updated: String,
    /// The working directory the session runs in. Which project an agent is
    /// touching says more at a glance than its last sentence does.
    pub cwd: String,
    pub turns: Vec<Turn>,
    pub artifacts: Vec<Artifact>,
}

impl Session {
    /// A session with nothing to read is not shown. The brief's rule about
    /// excerpt-less items: a header with no exchange under it reads as broken,
    /// and on paper you cannot scroll past it.
    pub fn is_readable(&self) -> bool {
        self.turns.iter().any(|t| !t.text.trim().is_empty())
    }
}

/// What the bridge holds right now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bridge {
    pub sessions: Vec<Session>,
    /// True when these are the last sessions we held rather than a fresh poll.
    pub stale: bool,
}

/// The gap under each turn. Page geometry is shared (see `page`); this is the
/// bridge's own rhythm.
const TURN_GAP: usize = 26;

/// Agent turns run long. They are no longer cut — a summary the reader chose
/// must be readable to its end — but a turn is split into chunks of this many
/// lines so a page boundary can fall inside it.
const TURN_CHUNK_LINES: usize = 6;
/// Artifacts are references, not reading. A page that ends in a wall of shas
/// has stopped being a page.
const MAX_ARTIFACTS: usize = 3;

/// A laid-out block: wrapped lines and the height they occupy. Usually one
/// whole turn; a long turn is split into chunks, and only the first chunk
/// carries the speaker — the rest read as the same block continuing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnLayout {
    /// Empty on a continuation chunk: no speaker row is drawn.
    pub speaker: String,
    pub lines: Vec<String>,
    pub height: usize,
}

/// One page of a session, measured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageLayout {
    pub title_lines: Vec<String>,
    pub meta: String,
    /// The project directory the session works in, on its own line — a path
    /// is an identifier, so it is never uppercased and never crammed into the
    /// meta row. Empty when the hub sent no cwd.
    pub place: String,
    /// Newest-last, so the page reads downward like the conversation did.
    pub turns: Vec<TurnLayout>,
    /// Which page this is. 0 is the newest; the swipe pages backward. The
    /// *displayed* number runs the other way — the newest page is page
    /// `pages`, so the conversation reads 1..n like the transcript it is.
    pub page: usize,
    /// How many pages the session runs to.
    pub pages: usize,
    pub artifacts: Vec<Artifact>,
    pub artifacts_omitted: usize,
}

/// Lay out one turn as one or more blocks. The speaker row rides only the
/// first chunk and the gap under the turn only the last, so mid-turn chunks
/// read as one continuous block — a page boundary may fall between them.
pub fn layout_turn(font: &FontRef, turn: &Turn) -> Vec<TurnLayout> {
    let lines = crate::script::wrap(font, &turn.text, BODY_PX, page::content_width() as f32);
    let total = lines.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < total {
        let end = (i + TURN_CHUNK_LINES).min(total);
        let first = i == 0;
        let last = end == total;
        let chunk: Vec<String> = lines[i..end].to_vec();
        let height = if first { LINE_H } else { 0 } // speaker row
            + chunk.len() * LINE_H
            + if last { TURN_GAP } else { 0 };
        out.push(TurnLayout {
            speaker: if first { turn.speaker.clone() } else { String::new() },
            lines: chunk,
            height,
        });
        i = end;
    }
    out
}

/// Lay out the newest page of a session — what a tap on the board shows.
pub fn layout_session(font: &FontRef, session: &Session) -> PageLayout {
    layout_session_page(font, session, 0, 0)
}

/// Lay out one page of a session, leaving `extra` pixels above the footer
/// untouched — the room the decision box claims when the session is waiting
/// on a human. `want_page` is clamped to what exists; 0 is the newest.
pub fn layout_session_page(font: &FontRef, session: &Session, extra: usize, want_page: usize) -> PageLayout {
    let title_lines = page::title_lines(font, &session.title);
    let meta = page::meta_line(&session.state, &session.updated);
    // The repo the agent works in, handy on every page of the session. The
    // full-width page affords a long tail; the board's rows keep a short one.
    let place = page::tail(&session.cwd, 60);

    let artifacts: Vec<Artifact> = session.artifacts.iter().take(MAX_ARTIFACTS).cloned().collect();
    let artifacts_omitted = session.artifacts.len().saturating_sub(artifacts.len());

    let place_room = if place.is_empty() { 0 } else { LINE_H };
    let y = page::HEADER_H + title_lines.len() * TITLE_LINE_H + LINE_H + place_room;
    // Artifacts are pinned: they are the evidence on the page, so they claim
    // their room before prose does — on every page.
    let artifact_room = artifacts.len() * LINE_H;

    let blocks: Vec<TurnLayout> = session
        .turns
        .iter()
        .filter(|t| !t.text.trim().is_empty())
        .flat_map(|t| layout_turn(font, t))
        .collect();
    let ranges = page::paginate(&blocks, y, page::limit(artifact_room + extra), |t| t.height);
    let pages = ranges.len();
    let page = want_page.min(pages - 1);
    let turns = blocks[ranges[page].clone()].to_vec();

    PageLayout { title_lines, meta, place, turns, page, pages, artifacts, artifacts_omitted }
}

/// The footer line. Empty when everything fit on one page — no need to say
/// "page 1 of 1".
///
/// `remaining` is sessions not shown, which the caller knows and the layout
/// does not.
pub fn footer_label(layout: &PageLayout, remaining: usize, stale: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if layout.pages > 1 {
        // Pages are numbered in reading order: the transcript starts on
        // page 1, and the newest page — where the reader lands — is page n.
        // Internally 0 is still the newest (see PageLayout::page).
        let mut p = format!("page {} of {}", layout.pages - layout.page, layout.pages);
        if layout.page == 0 {
            // The first hardware read found content cut off with no visible
            // way onward — the page must teach the gesture, not assume it.
            p.push_str(" · swipe down for earlier");
        }
        parts.push(p);
    }
    if remaining > 0 {
        parts.push(format!(
            "{remaining} more session{}",
            if remaining == 1 { "" } else { "s" }
        ));
    }
    if layout.artifacts_omitted > 0 {
        parts.push(format!("{} more changed", layout.artifacts_omitted));
    }
    if stale {
        parts.push("not refreshed".to_string());
    }
    parts.join(" · ")
}

/// Sessions worth putting on the page, in the order the bridge gave them.
pub fn readable(bridge: &Bridge) -> Vec<&Session> {
    bridge.sessions.iter().filter(|s| s.is_readable()).collect()
}

/// What the pad is currently holding.
///
/// Sessions arrive from a poll, not from the draw path, so the drawer reads
/// the last thing we were told rather than taking it as an argument. A bridge
/// that cannot be reached keeps its previous contents and marks them stale: a
/// stale page that says so beats an empty one.
static HELD: std::sync::Mutex<Option<Bridge>> = std::sync::Mutex::new(None);

/// The sessions to draw. Empty and non-stale before the first poll.
pub fn held() -> Bridge {
    HELD.lock().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// Take a fresh poll. Nothing else in the tree writes this.
pub fn replace(bridge: Bridge) {
    if let Ok(mut g) = HELD.lock() {
        *g = Some(bridge);
    }
}

/// Mark what we hold as stale after a failed poll, keeping the contents.
pub fn mark_stale() {
    if let Ok(mut g) = HELD.lock() {
        let b = g.get_or_insert_with(Bridge::default);
        b.stale = true;
    }
}

/// `http://laptop:9707` → the board endpoint. The base is configuration; the
/// paths are the bridge's contract with the hub.
pub fn sessions_url(base: &str) -> String {
    format!("{}/sessions", base.trim_end_matches('/'))
}

/// Where a mark on session `id` is POSTed.
pub fn nudge_url(base: &str, id: &str) -> String {
    format!("{}/sessions/{id}/nudge", base.trim_end_matches('/'))
}

/// Start polling the hub, if one is configured. Without RIDDLE_BRIDGE_URL the
/// thread never starts and Agent mode stays dormant — the pad loses nothing.
///
/// The pad polls; there is no streaming. A failed poll marks what is held as
/// stale rather than clearing it: a stale page that says so beats an empty
/// one. See `docs/claude-bridge.md`.
pub fn spawn_poll() {
    let Ok(base) = std::env::var("RIDDLE_BRIDGE_URL") else { return };
    let every = std::env::var("RIDDLE_BRIDGE_POLL_S")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20)
        .max(5);
    eprintln!("g-pad: bridge polling {base} every {every}s");
    std::thread::spawn(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(10))
            .build();
        let url = sessions_url(&base);
        loop {
            match agent.get(&url).call().ok().and_then(|r| r.into_string().ok()) {
                Some(body) => replace(Bridge { sessions: parse_sessions(&body), stale: false }),
                None => mark_stale(),
            }
            std::thread::sleep(std::time::Duration::from_secs(every));
        }
    });
}

/// Carry one mark to the hub. Synchronous and short-fused: the hub is one
/// LAN hop away, and the page redraw right after this reports the outcome.
///
/// The pad only ever says what the writer did — `tick`, `strike`, or words.
/// What keystrokes that becomes is the hub's business (see the design doc);
/// swapping the hub's transport never touches this.
pub fn post_nudge(id: &str, mark: &str, text: Option<&str>) -> Result<(), String> {
    let base = std::env::var("RIDDLE_BRIDGE_URL").map_err(|_| "no hub configured".to_string())?;
    let body = match text {
        Some(t) => format!(r#"{{"mark":"{mark}","text":"{}"}}"#, escape_json(t)),
        None => format!(r#"{{"mark":"{mark}"}}"#),
    };
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(5)).build();
    match agent
        .post(&nudge_url(&base, id))
        .set("Content-Type", "application/json")
        .send_string(&body)
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, r)) => {
            let detail = r.into_string().unwrap_or_default();
            Err(format!("hub {code}: {}", detail.trim()))
        }
        Err(e) => Err(format!("hub unreachable: {e}")),
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// Parse the bridge payload.
///
/// Deliberately tolerant, like the feed parser: a bridge that adds fields must
/// not break the pad. Turns and artifacts are read from nested arrays, and a
/// session with neither still parses — `is_readable` decides whether it is
/// shown, not this.
pub fn parse_sessions(json: &str) -> Vec<Session> {
    let mut out = Vec::new();
    for block in split_objects(json, "sessions") {
        let title = json_field(&block, "title").unwrap_or_default();
        if title.trim().is_empty() {
            continue;
        }
        out.push(Session {
            id: json_field(&block, "id").unwrap_or_default(),
            title,
            state: json_field(&block, "state").unwrap_or_default(),
            updated: json_field(&block, "updated").unwrap_or_default(),
            cwd: json_field(&block, "cwd").unwrap_or_default(),
            turns: parse_turns(&block),
            artifacts: parse_artifacts(&block),
        });
    }
    out
}

fn parse_turns(block: &str) -> Vec<Turn> {
    split_objects(block, "turns")
        .into_iter()
        .filter_map(|t| {
            let text = json_field(&t, "text").unwrap_or_default();
            if text.trim().is_empty() {
                return None;
            }
            Some(Turn { speaker: json_field(&t, "speaker").unwrap_or_default(), text })
        })
        .collect()
}

/// An artifact with no reference is prose wearing evidence's clothes. Drop it.
fn parse_artifacts(block: &str) -> Vec<Artifact> {
    split_objects(block, "artifacts")
        .into_iter()
        .filter_map(|a| {
            let reference = json_field(&a, "ref").unwrap_or_default();
            if reference.trim().is_empty() {
                return None;
            }
            Some(Artifact { reference, label: json_field(&a, "label").unwrap_or_default() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UI_FONT_TTF;

    fn font() -> FontRef<'static> {
        FontRef::try_from_slice(UI_FONT_TTF).unwrap()
    }

    fn turn(speaker: &str, text: &str) -> Turn {
        Turn { speaker: speaker.to_string(), text: text.to_string() }
    }

    fn session(turns: Vec<Turn>) -> Session {
        Session {
            id: "s1".to_string(),
            title: "Direct Claude Code from the pad".to_string(),
            state: "running".to_string(),
            updated: "14:02".to_string(),
            cwd: String::new(),
            turns,
            artifacts: Vec::new(),
        }
    }

    const LIVE: &str = r#"{"sessions":[
        {"id":"s1","title":"Wire the bridge","state":"running","updated":"14:02",
         "cwd":"/Users/p/Dev/g-pad",
         "turns":[{"speaker":"you","text":"read the docs first"},
                  {"speaker":"claude","text":"Baseline is green — 58 passed."}],
         "artifacts":[{"ref":"a1b2c3d","label":"docs: the Claude bridge"}]},
        {"id":"s2","title":"Nothing to read","state":"waiting","updated":"09:10",
         "turns":[],"artifacts":[]}
    ],"total":2}"#;

    #[test]
    fn parses_the_bridge_shape() {
        let s = parse_sessions(LIVE);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title, "Wire the bridge");
        assert_eq!(s[0].state, "running");
        assert_eq!(s[0].turns.len(), 2);
        assert_eq!(s[0].turns[1].speaker, "claude");
        assert_eq!(s[0].artifacts[0].reference, "a1b2c3d");
        assert_eq!(s[0].cwd, "/Users/p/Dev/g-pad");
        assert_eq!(s[1].cwd, "", "a hub that sends no cwd still parses");
    }

    #[test]
    fn a_session_with_no_exchange_is_not_shown() {
        let bridge = Bridge { sessions: parse_sessions(LIVE), stale: false };
        let shown = readable(&bridge);
        assert_eq!(shown.len(), 1, "the empty session must not reach the page");
        assert_eq!(shown[0].id, "s1");
    }

    #[test]
    fn an_artifact_without_a_reference_is_dropped() {
        // Evidence by reference, never by description: a label alone is prose.
        let json = r#"{"sessions":[{"title":"t","turns":[{"speaker":"c","text":"hi"}],
            "artifacts":[{"label":"fixed the parser"},{"ref":"9f8e7d","label":"real"}]}]}"#;
        let s = parse_sessions(json);
        assert_eq!(s[0].artifacts.len(), 1);
        assert_eq!(s[0].artifacts[0].reference, "9f8e7d");
    }

    #[test]
    fn decodes_escapes_and_keeps_multibyte_intact() {
        let s = parse_sessions(LIVE);
        assert_eq!(s[0].turns[1].text, "Baseline is green — 58 passed.");
    }

    #[test]
    fn tolerates_junk_and_missing_fields() {
        assert!(parse_sessions("").is_empty());
        assert!(parse_sessions("{\"sessions\":[]}").is_empty());
        assert!(parse_sessions("not json at all").is_empty());
        // A session with no title is not a session.
        assert!(parse_sessions(r#"{"sessions":[{"state":"running"}]}"#).is_empty());
    }

    #[test]
    fn the_page_keeps_the_most_recent_turns() {
        let f = font();
        let many: Vec<Turn> = (0..40)
            .map(|i| turn("claude", &format!("turn number {i} with enough words to wrap a line or two")))
            .collect();
        let layout = layout_session(&f, &session(many));
        assert!(!layout.turns.is_empty());
        assert!(layout.pages > 1, "40 turns cannot fit one page");
        // The last turn is the one worth reading, and it must be last on the page.
        let last = layout.turns.last().unwrap();
        assert!(last.lines[0].contains("turn number 39"));
    }

    #[test]
    fn a_long_turn_is_chunked_and_readable_to_its_end() {
        let f = font();
        // One turn far taller than a page — the overnight-summary shape that
        // was unreadable on the first hardware read.
        let long = "a sentence with enough words to certainly wrap onto the next line. ".repeat(60);
        let s = session(vec![turn("claude", &long)]);
        let chunks = layout_turn(&f, &s.turns[0]);
        assert!(chunks.len() > 1, "a long turn must split into chunks");
        assert_eq!(chunks[0].speaker, "claude");
        assert!(chunks[1..].iter().all(|c| c.speaker.is_empty()),
            "only the first chunk carries the speaker");
        // Nothing is cut: every wrapped line appears on exactly one page.
        let total_lines: usize = chunks.iter().map(|c| c.lines.len()).sum();
        let layout0 = layout_session(&f, &s);
        assert!(layout0.pages > 1);
        let mut seen = 0usize;
        for p in 0..layout0.pages {
            let l = layout_session_page(&f, &s, 0, p);
            seen += l.turns.iter().map(|t| t.lines.len()).sum::<usize>();
        }
        assert_eq!(seen, total_lines, "paging must reach every line of the turn");
    }

    #[test]
    fn a_page_request_past_the_end_clamps_to_the_oldest() {
        let f = font();
        let many: Vec<Turn> = (0..40)
            .map(|i| turn("claude", &format!("turn {i} with enough text to occupy a line or two here")))
            .collect();
        let s = session(many);
        let last = layout_session_page(&f, &s, 0, usize::MAX);
        assert_eq!(last.page, last.pages - 1);
        // The oldest page starts at the first turn.
        assert!(last.turns[0].lines[0].contains("turn 0"));
    }

    #[test]
    fn pages_are_numbered_in_reading_order_and_the_landing_page_is_last() {
        let f = font();
        let many: Vec<Turn> = (0..40)
            .map(|i| turn("claude", &format!("turn {i} with enough text to occupy a line or two here")))
            .collect();
        let s = session(many);
        // The reader lands on the newest content, which is the *last* page of
        // the conversation — page n of n, with earlier pages counting down.
        let landing = layout_session_page(&f, &s, 0, 0);
        let n = landing.pages;
        let label = footer_label(&landing, 0, false);
        assert!(label.starts_with(&format!("page {n} of {n}")), "got {label:?}");
        assert!(label.contains("swipe down for earlier"));
        let back_one = layout_session_page(&f, &s, 0, 1);
        let label = footer_label(&back_one, 0, false);
        assert!(label.starts_with(&format!("page {} of {n}", n - 1)), "got {label:?}");
        assert!(!label.contains("swipe"), "the hint earned its keep on the landing page");
        // The oldest page is page 1.
        let oldest = layout_session_page(&f, &s, 0, n - 1);
        assert!(footer_label(&oldest, 0, false).starts_with(&format!("page 1 of {n}")));
    }

    #[test]
    fn the_project_directory_gets_its_own_line_never_the_meta_row() {
        let f = font();
        let mut s = session(vec![turn("you", "hi")]);
        s.cwd = "/Users/p/Dev/g-pad".to_string();
        let layout = layout_session(&f, &s);
        assert_eq!(layout.place, "/Users/p/Dev/g-pad");
        assert_eq!(layout.meta, "running · 14:02", "the path must not ride the meta row");
        // No cwd, no line — and no room claimed for one.
        let bare = layout_session(&f, &session(vec![turn("you", "hi")]));
        assert!(bare.place.is_empty());
    }

    #[test]
    fn a_long_project_path_keeps_its_tail() {
        let f = font();
        let mut s = session(vec![turn("you", "hi")]);
        s.cwd = format!("/very{}/Dev/g-pad", "/deep".repeat(20));
        let layout = layout_session(&f, &s);
        assert!(layout.place.starts_with('…'));
        assert!(layout.place.ends_with("Dev/g-pad"));
    }

    #[test]
    fn a_page_stops_before_the_footer() {
        let f = font();
        let many: Vec<Turn> = (0..40)
            .map(|i| turn("claude", &format!("turn {i} with enough text to occupy several lines of the page")))
            .collect();
        let layout = layout_session(&f, &session(many));
        let used: usize = page::HEADER_H
            + layout.title_lines.len() * TITLE_LINE_H
            + LINE_H
            + layout.turns.iter().map(|t| t.height).sum::<usize>();
        assert!(used <= page::limit(0), "used {used} px");
    }

    #[test]
    fn everything_fitting_leaves_no_footer() {
        let f = font();
        let layout = layout_session(&f, &session(vec![turn("you", "short"), turn("claude", "also short")]));
        assert_eq!(layout.pages, 1);
        assert_eq!(footer_label(&layout, 0, false), "");
    }

    #[test]
    fn urls_compose_from_a_base_with_or_without_slash() {
        assert_eq!(sessions_url("http://h:9707"), "http://h:9707/sessions");
        assert_eq!(sessions_url("http://h:9707/"), "http://h:9707/sessions");
        assert_eq!(nudge_url("http://h:9707", "s1"), "http://h:9707/sessions/s1/nudge");
    }

    #[test]
    fn a_failed_first_poll_still_reads_as_stale() {
        // Serialized with the shared HELD state: reset, fail, inspect.
        if let Ok(mut g) = HELD.lock() {
            *g = None;
        }
        mark_stale();
        assert!(held().stale, "a pad that never heard from the hub must say so");
        assert!(held().sessions.is_empty());
    }

    #[test]
    fn the_footer_counts_what_was_left_out() {
        let f = font();
        let layout = layout_session(&f, &session(vec![turn("you", "short")]));
        assert_eq!(footer_label(&layout, 3, false), "3 more sessions");
        assert_eq!(footer_label(&layout, 1, false), "1 more session");
        assert_eq!(footer_label(&layout, 0, true), "not refreshed");
        assert_eq!(footer_label(&layout, 2, true), "2 more sessions · not refreshed");
    }

    #[test]
    fn artifacts_are_capped_and_the_rest_are_counted() {
        let f = font();
        let mut s = session(vec![turn("you", "short")]);
        s.artifacts = (0..6)
            .map(|i| Artifact { reference: format!("sha{i}"), label: "change".to_string() })
            .collect();
        let layout = layout_session(&f, &s);
        assert_eq!(layout.artifacts.len(), MAX_ARTIFACTS);
        assert_eq!(layout.artifacts_omitted, 3);
        assert!(footer_label(&layout, 0, false).contains("3 more changed"));
    }

    #[test]
    fn artifacts_claim_their_room_before_prose() {
        let f = font();
        let many: Vec<Turn> = (0..40)
            .map(|i| turn("claude", &format!("turn {i} with enough text to occupy several lines here")))
            .collect();
        let bare = layout_session(&f, &session(many.clone()));
        let mut with = session(many);
        with.artifacts = (0..MAX_ARTIFACTS)
            .map(|i| Artifact { reference: format!("sha{i}"), label: "change".to_string() })
            .collect();
        let with = layout_session(&f, &with);
        assert!(with.turns.len() <= bare.turns.len(),
            "evidence must not be pushed off the page by prose");
    }

    #[test]
    fn long_titles_are_capped() {
        let f = font();
        let mut s = session(vec![turn("you", "hi")]);
        s.title = "a very long session title ".repeat(20);
        let layout = layout_session(&f, &s);
        assert!(layout.title_lines.len() <= page::MAX_TITLE_LINES);
    }

    #[test]
    fn meta_survives_a_missing_time() {
        let f = font();
        let mut s = session(vec![turn("you", "hi")]);
        s.updated = String::new();
        assert_eq!(layout_session(&f, &s).meta, "running");
    }
}
