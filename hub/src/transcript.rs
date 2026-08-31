//! Claude Code transcript JSONL → what the pad can read.
//!
//! Content comes from here; state never does. A transcript can say what was
//! said and what was changed, but whether the session is running or waiting
//! on a human is only knowable from the live pane — see `tmux`.
//!
//! The evidence rule from `docs/claude-bridge.md` is enforced at extraction:
//! artifacts are references pulled from tool traffic (a sha, a file path),
//! never sentences from the model's prose.

use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    pub speaker: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Artifact {
    pub reference: String,
    pub label: String,
}

/// Everything a transcript can honestly report about one session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionData {
    pub id: String,
    pub title: String,
    /// The working directory the session runs in — the key that matches a
    /// session to its tmux pane.
    pub cwd: String,
    /// ISO timestamp of the last entry, as the transcript wrote it.
    pub updated_iso: String,
    pub turns: Vec<Turn>,
    pub artifacts: Vec<Artifact>,
}

/// Parse a whole transcript. Tolerant line by line: a line that is not JSON,
/// or is a shape this reader does not know, contributes nothing rather than
/// failing the file.
pub fn parse(id: &str, text: &str) -> SessionData {
    let mut s = SessionData { id: id.to_string(), ..Default::default() };
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(cwd) = v.get("cwd").and_then(Value::as_str) {
            s.cwd = cwd.to_string();
        }
        if let Some(ts) = v.get("timestamp").and_then(Value::as_str) {
            s.updated_iso = ts.to_string();
        }
        if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        // Sidechains are a subagent's conversation, not the session's.
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(msg) = v.get("message") else { continue };
        match msg.get("role").and_then(Value::as_str) {
            Some("user") => read_user(&v, msg, &mut s),
            Some("assistant") => read_assistant(msg, &mut s),
            _ => {}
        }
    }
    if s.title.is_empty() {
        // No human spoke plainly (a skill launch, a probe). The project's
        // name reads; a UUID does not.
        let dir = s.cwd.rsplit('/').next().unwrap_or("");
        s.title = if dir.is_empty() { s.id.clone() } else { dir.to_string() };
    }
    s
}

fn read_user(line: &Value, msg: &Value, s: &mut SessionData) {
    match msg.get("content") {
        // A plain string is the human typing.
        Some(Value::String(text)) => push_human(text, s),
        Some(Value::Array(blocks)) => {
            for b in blocks {
                match b.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(Value::as_str) {
                            push_human(t, s);
                        }
                    }
                    // Tool results are the world answering. Not prose — but
                    // commit lines inside them are evidence worth extracting.
                    Some("tool_result") => scan_commits(&b.to_string(), s),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    let _ = line;
}

fn push_human(text: &str, s: &mut SessionData) {
    let text = text.trim();
    // Harness scaffolding (command wrappers, reminders) is not the human.
    if text.is_empty() || text.starts_with('<') {
        return;
    }
    if s.title.is_empty() {
        s.title = first_line_capped(text, 72);
    }
    s.turns.push(Turn { speaker: "you".to_string(), text: text.to_string() });
}

fn read_assistant(msg: &Value, s: &mut SessionData) {
    let Some(Value::Array(blocks)) = msg.get("content") else { return };
    let mut prose = String::new();
    for b in blocks {
        match b.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = b.get("text").and_then(Value::as_str) {
                    if !prose.is_empty() {
                        prose.push('\n');
                    }
                    prose.push_str(t.trim());
                }
            }
            Some("tool_use") => read_tool_use(b, s),
            _ => {}
        }
    }
    if prose.trim().is_empty() {
        return;
    }
    // A reply split across blocks is one turn, and consecutive replies with
    // no human between them read better folded than as a stutter of turns.
    if let Some(last) = s.turns.last_mut() {
        if last.speaker == "claude" {
            last.text.push('\n');
            last.text.push_str(&prose);
            return;
        }
    }
    s.turns.push(Turn { speaker: "claude".to_string(), text: prose });
}

/// A file the agent wrote is a fact with an address. The path is the
/// reference; which tool touched it is the label.
fn read_tool_use(b: &Value, s: &mut SessionData) {
    let Some(name) = b.get("name").and_then(Value::as_str) else { return };
    if !matches!(name, "Write" | "Edit" | "NotebookEdit") {
        return;
    }
    let Some(path) = b.pointer("/input/file_path").and_then(Value::as_str) else { return };
    if s.artifacts.iter().any(|a| a.reference == path) {
        return;
    }
    s.artifacts.push(Artifact { reference: path.to_string(), label: "edited".to_string() });
}

/// Find `[branch abc1234] subject` commit lines in tool-result text.
///
/// The sha is the artifact; the subject is its label. Hand-rolled so the hub
/// does not grow a regex dependency for one pattern.
fn scan_commits(text: &str, s: &mut SessionData) {
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find(']') else { break };
        let inside = &rest[..close];
        let after = &rest[close + 1..];
        if let Some(sha) = commit_sha(inside) {
            if !s.artifacts.iter().any(|a| a.reference == sha) {
                let label = first_line_capped(after.trim_start(), 60);
                // Tool results arrive JSON-escaped; a subject that still
                // carries escapes reads as noise, so keep it plain.
                let label = label.split('\\').next().unwrap_or("").trim().to_string();
                s.artifacts.push(Artifact { reference: sha, label });
            }
        }
    }
}

/// `main 1922056` → the sha, if the bracket really was a commit line.
fn commit_sha(inside: &str) -> Option<String> {
    let mut parts = inside.split(' ');
    let _branch = parts.next()?;
    let sha = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let ok = sha.len() >= 7 && sha.len() <= 40 && sha.chars().all(|c| c.is_ascii_hexdigit());
    ok.then(|| sha.to_string())
}

fn first_line_capped(text: &str, cap: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() <= cap {
        return line.to_string();
    }
    let cut: String = line.chars().take(cap.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// "2026-08-30T08:38:22.065Z" → "22:38" at the given offset from UTC.
///
/// The transcript writes UTC; the reviewer reads a wall clock. Date math
/// stays trivial because only the clock is shown, never the day.
pub fn clock(iso: &str, tz_offset_hours: i64) -> String {
    let Some(t) = iso.find('T') else { return String::new() };
    let hhmm = &iso[t + 1..];
    let (Some(h), Some(m)) = (hhmm.get(0..2), hhmm.get(3..5)) else { return String::new() };
    let (Ok(h), Ok(m)) = (h.parse::<i64>(), m.parse::<i64>()) else { return String::new() };
    let h = (h + tz_offset_hours).rem_euclid(24);
    format!("{h:02}:{m:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shapes below mirror real transcript lines captured 2026-08-30.
    const LINES: &str = r#"{"type":"last-prompt","sessionId":"abc"}
{"type":"user","message":{"role":"user","content":"fix the parser in the brief"},"cwd":"/Users/p/Dev/g-pad","timestamp":"2026-08-30T18:00:00.000Z","origin":{"kind":"human"}}
{"message":{"role":"assistant","content":[{"type":"thinking","thinking":"…"},{"type":"text","text":"Looking at brief.rs now."},{"type":"tool_use","name":"Edit","input":{"file_path":"src/brief.rs"}}]},"timestamp":"2026-08-30T18:01:00.000Z"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"[main a1b2c3d] fix: unhook the stall\n 1 file changed"}]},"timestamp":"2026-08-30T18:02:00.000Z"}
{"message":{"role":"assistant","content":[{"type":"text","text":"Committed."}]},"timestamp":"2026-08-30T18:03:00.000Z"}
{"message":{"role":"assistant","content":[{"type":"text","text":"Tests are green."}]},"timestamp":"2026-08-30T18:04:00.000Z"}"#;

    #[test]
    fn reads_turns_title_cwd_and_time() {
        let s = parse("abc", LINES);
        assert_eq!(s.title, "fix the parser in the brief");
        assert_eq!(s.cwd, "/Users/p/Dev/g-pad");
        assert_eq!(s.updated_iso, "2026-08-30T18:04:00.000Z");
        assert_eq!(s.turns[0].speaker, "you");
        assert_eq!(s.turns[1].speaker, "claude");
    }

    #[test]
    fn consecutive_agent_replies_fold_into_one_turn() {
        let s = parse("abc", LINES);
        assert_eq!(s.turns.len(), 2);
        assert!(s.turns[1].text.contains("Committed."));
        assert!(s.turns[1].text.contains("Tests are green."));
    }

    #[test]
    fn artifacts_are_references_never_prose() {
        let s = parse("abc", LINES);
        let refs: Vec<&str> = s.artifacts.iter().map(|a| a.reference.as_str()).collect();
        assert_eq!(refs, vec!["src/brief.rs", "a1b2c3d"]);
        assert_eq!(s.artifacts[1].label, "fix: unhook the stall");
    }

    #[test]
    fn scaffolding_and_sidechains_are_not_the_human() {
        let lines = r#"{"type":"user","message":{"role":"user","content":"<system-reminder>noise</system-reminder>"}}
{"type":"user","isSidechain":true,"message":{"role":"user","content":"subagent brief"}}
{"type":"user","message":{"role":"user","content":"the real ask"}}"#;
        let s = parse("x", lines);
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.title, "the real ask");
    }

    #[test]
    fn junk_lines_contribute_nothing() {
        let s = parse("x", "not json\n{\"half\":true}\n");
        assert!(s.turns.is_empty());
        assert_eq!(s.title, "x", "id stands in when no human spoke");
    }

    #[test]
    fn a_bracket_that_is_not_a_commit_is_ignored() {
        let mut s = SessionData::default();
        scan_commits("[info] done and [main deadbee] feat: real", &mut s);
        assert_eq!(s.artifacts.len(), 1);
        assert_eq!(s.artifacts[0].reference, "deadbee");
    }

    #[test]
    fn long_titles_are_capped() {
        let long = "a ".repeat(100);
        let s = parse("x", &format!(r#"{{"type":"user","message":{{"role":"user","content":"{long}"}}}}"#));
        assert!(s.title.chars().count() <= 72);
        assert!(s.title.ends_with('…'));
    }

    #[test]
    fn clock_shifts_utc_to_local() {
        assert_eq!(clock("2026-08-30T08:38:22.065Z", -10), "22:38");
        assert_eq!(clock("2026-08-30T18:04:00.000Z", -10), "08:04");
        assert_eq!(clock("garbage", -10), "");
    }
}
