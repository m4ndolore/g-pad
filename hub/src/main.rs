//! anthink-hub — the homelab side of Anthink's Agent mode.
//!
//! Serves Claude Code sessions to the pad in the shape
//! `bridge::parse_sessions` already reads, and carries the pad's marks back
//! into the panes that can act on them. Design: see
//! `docs/plans/2026-08-30-anthink-hub-design.md`.
//!
//!     GET  /sessions               the board
//!     POST /sessions/<id>/nudge    {"mark":"tick"|"strike"|"text","text":"…"}
//!
//! Runs on the machine the pad can reach. The pad never learns tmux exists.

mod transcript;
mod tmux;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};
use transcript::SessionData;

/// Sessions older than this are history, not a board.
const FRESH_WINDOW: Duration = Duration::from_secs(48 * 3600);
/// The pad's page caps what it shows; sending more than this is waste.
const MAX_TURNS: usize = 8;
const MAX_ARTIFACTS: usize = 5;

/// (mtime, len): the fingerprint that decides whether a transcript re-parses.
type Fingerprint = (SystemTime, u64);

struct Hub {
    projects_root: PathBuf,
    tz_offset_hours: i64,
    /// Parse cache keyed by transcript path; a file that has not changed is
    /// not re-read.
    cache: Mutex<HashMap<PathBuf, (Fingerprint, SessionData)>>,
}

fn main() {
    let port = std::env::var("HUB_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(9707u16);
    let projects_root = std::env::var("HUB_PROJECTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".claude/projects"));
    let hub = Hub {
        projects_root,
        tz_offset_hours: local_offset_hours(),
        cache: Mutex::new(HashMap::new()),
    };

    let server = match tiny_http::Server::http(("0.0.0.0", port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("anthink-hub: cannot bind port {port}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("anthink-hub: serving {} on :{port}", hub.projects_root.display());

    for mut request in server.incoming_requests() {
        let method = request.method().to_string();
        let url = request.url().to_string();
        let response = match (method.as_str(), url.as_str()) {
            ("GET", "/sessions") => ok_json(hub.board()),
            ("POST", _) => match nudge_target(&url) {
                Some(id) => {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    match hub.nudge(&id, &body) {
                        Ok(()) => ok_json(json!({"ok": true})),
                        Err(e) => err_json(409, &e),
                    }
                }
                None => err_json(404, "unknown path"),
            },
            _ => err_json(404, "unknown path"),
        };
        let _ = request.respond(response);
    }
}

/// `/sessions/<id>/nudge` → the id, or None for any other path.
fn nudge_target(url: &str) -> Option<String> {
    let rest = url.strip_prefix("/sessions/")?;
    let id = rest.strip_suffix("/nudge")?;
    (!id.is_empty() && !id.contains('/')).then(|| id.to_string())
}

impl Hub {
    /// The board: every fresh session, the neediest first.
    fn board(&self) -> Value {
        let mut sessions = self.fresh_sessions();
        let panes = tmux::claude_panes();
        // One capture per distinct pane, not per session sharing it.
        let mut pane_states: HashMap<String, tmux::PaneState> = HashMap::new();
        let mut rows: Vec<(u8, String, Value)> = sessions
            .drain(..)
            .map(|s| {
                let state = pane_for(&panes, &s.cwd)
                    .map(|p| {
                        *pane_states
                            .entry(p.id.clone())
                            .or_insert_with(|| tmux::classify(&tmux::capture(&p.id)))
                    })
                    .map(tmux::PaneState::word)
                    .unwrap_or("done");
                let rank = match state {
                    "waiting" => 0,
                    "running" => 1,
                    _ => 2,
                };
                (rank, s.updated_iso.clone(), row(&s, state, self.tz_offset_hours))
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        let total = rows.len();
        json!({ "sessions": rows.into_iter().map(|r| r.2).collect::<Vec<_>>(), "total": total })
    }

    fn nudge(&self, id: &str, body: &str) -> Result<(), String> {
        let v: Value = serde_json::from_str(body).map_err(|_| "body is not JSON".to_string())?;
        let mark = v.get("mark").and_then(Value::as_str).unwrap_or_default();
        let n = match mark {
            "tick" => tmux::Nudge::Tick,
            "strike" => tmux::Nudge::Strike,
            "text" => {
                let t = v.get("text").and_then(Value::as_str).unwrap_or_default().trim();
                if t.is_empty() {
                    return Err("a text nudge needs text".to_string());
                }
                tmux::Nudge::Text(t.to_string())
            }
            _ => return Err(format!("unknown mark {mark:?}")),
        };
        let session = self
            .fresh_sessions()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("no fresh session {id}"))?;
        let panes = tmux::claude_panes();
        let pane = pane_for(&panes, &session.cwd)
            .ok_or_else(|| format!("no pane is running claude in {}", session.cwd))?;
        tmux::nudge(pane, &n)
    }

    /// Every transcript touched inside the fresh window, parsed (from cache
    /// where the file has not changed).
    fn fresh_sessions(&self) -> Vec<SessionData> {
        let mut out = Vec::new();
        let Ok(projects) = std::fs::read_dir(&self.projects_root) else { return out };
        let now = SystemTime::now();
        for project in projects.flatten() {
            let Ok(files) = std::fs::read_dir(project.path()) else { continue };
            for f in files.flatten() {
                let path = f.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(meta) = f.metadata() else { continue };
                let Ok(mtime) = meta.modified() else { continue };
                if now.duration_since(mtime).map(|d| d > FRESH_WINDOW).unwrap_or(true) {
                    continue;
                }
                if let Some(s) = self.parse_cached(&path, (mtime, meta.len())) {
                    // A session nobody spoke in (a bare boot, a probe) is not
                    // a board row.
                    if !s.turns.is_empty() {
                        out.push(s);
                    }
                }
            }
        }
        out
    }

    fn parse_cached(&self, path: &Path, fingerprint: Fingerprint) -> Option<SessionData> {
        if let Ok(cache) = self.cache.lock() {
            if let Some((fp, s)) = cache.get(path) {
                if *fp == fingerprint {
                    return Some(s.clone());
                }
            }
        }
        let id = path.file_stem()?.to_str()?.to_string();
        let text = std::fs::read_to_string(path).ok()?;
        let s = transcript::parse(&id, &text);
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(path.to_path_buf(), (fingerprint, s.clone()));
        }
        Some(s)
    }
}

/// The pane whose working directory is the session's. The picker shares a
/// directory with real work, so a session that also matches a non-picker
/// pane must get the real one — callers get the first match after panes are
/// filtered here; picker refusal happens at nudge time.
fn pane_for<'a>(panes: &'a [tmux::Pane], cwd: &str) -> Option<&'a tmux::Pane> {
    if cwd.is_empty() {
        return None;
    }
    panes.iter().find(|p| p.path == cwd)
}

fn row(s: &SessionData, state: &str, tz: i64) -> Value {
    let turns: Vec<Value> = s
        .turns
        .iter()
        .rev()
        .take(MAX_TURNS)
        .rev()
        .map(|t| json!({"speaker": t.speaker, "text": t.text}))
        .collect();
    let artifacts: Vec<Value> = s
        .artifacts
        .iter()
        .rev()
        .take(MAX_ARTIFACTS)
        .rev()
        .map(|a| json!({"ref": a.reference, "label": a.label}))
        .collect();
    json!({
        "id": s.id,
        "title": s.title,
        "state": state,
        "updated": transcript::clock(&s.updated_iso, tz),
        "turns": turns,
        "artifacts": artifacts,
    })
}

fn ok_json(v: Value) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    json_response(200, v)
}

fn err_json(code: u16, msg: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    json_response(code, json!({"error": msg}))
}

fn json_response(code: u16, v: Value) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let body = v.to_string().into_bytes();
    tiny_http::Response::from_data(body)
        .with_status_code(code)
        .with_header("Content-Type: application/json".parse::<tiny_http::Header>().unwrap())
}

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

/// The machine's UTC offset, read once at startup from `date +%z`.
fn local_offset_hours() -> i64 {
    let out = std::process::Command::new("date").arg("+%z").output();
    let Ok(out) = out else { return 0 };
    let z = String::from_utf8_lossy(&out.stdout);
    let z = z.trim();
    if z.len() < 3 {
        return 0;
    }
    z[..3].parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nudge_paths_parse_and_reject() {
        assert_eq!(nudge_target("/sessions/abc-123/nudge").as_deref(), Some("abc-123"));
        assert_eq!(nudge_target("/sessions//nudge"), None);
        assert_eq!(nudge_target("/sessions/a/b/nudge"), None);
        assert_eq!(nudge_target("/other"), None);
    }

    #[test]
    fn rows_carry_the_bridge_shape() {
        let s = SessionData {
            id: "s1".into(),
            title: "Wire the bridge".into(),
            cwd: "/x".into(),
            updated_iso: "2026-08-30T18:04:00.000Z".into(),
            turns: vec![transcript::Turn { speaker: "you".into(), text: "go".into() }],
            artifacts: vec![transcript::Artifact { reference: "a1b2c3d".into(), label: "l".into() }],
        };
        let r = row(&s, "waiting", -10);
        assert_eq!(r["state"], "waiting");
        assert_eq!(r["updated"], "08:04");
        assert_eq!(r["turns"][0]["speaker"], "you");
        assert_eq!(r["artifacts"][0]["ref"], "a1b2c3d");
    }

    #[test]
    fn only_the_last_turns_ride_and_order_survives() {
        let turns: Vec<transcript::Turn> = (0..20)
            .map(|i| transcript::Turn { speaker: "claude".into(), text: format!("t{i}") })
            .collect();
        let s = SessionData { turns, ..Default::default() };
        let r = row(&s, "running", 0);
        let sent = r["turns"].as_array().unwrap();
        assert_eq!(sent.len(), MAX_TURNS);
        assert_eq!(sent[0]["text"], "t12");
        assert_eq!(sent[MAX_TURNS - 1]["text"], "t19");
    }
}
