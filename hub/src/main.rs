//! anthink-hub — the homelab side of Anthink's Agent mode.
//!
//! Serves Claude Code sessions to the pad in the shape
//! `bridge::parse_sessions` already reads, and carries the pad's marks back
//! into the panes that can act on them. Design: see
//! `docs/plans/2026-08-30-anthink-hub-design.md`.
//!
//!     GET  /sessions               the board, across every client
//!     POST /sessions/<id>/nudge    {"mark":"tick"|"strike"|"text","text":"…"}
//!
//! Runs on the machine the pad can reach; other clients are reached from
//! here over ssh (`hub.toml`). Session ids are `client:uuid`, so a mark
//! routes back to the machine it came from. A client that cannot be reached
//! shows its last-known sessions as `stale` — state is never invented.
//!
//! The pad never learns tmux or ssh exists.
//!
//! A nudge types into a real shell, so the hub is not open: without
//! `HUB_TOKEN` it binds loopback only, and with one it requires
//! `Authorization: Bearer` on every request (the pad sends
//! `RIDDLE_BRIDGE_TOKEN`). Requests that arrive under a DNS name are
//! refused — a browser on the LAN can be lured to `evil.example` rebound
//! to this address, but it cannot send a bare IP in `Host`. `HUB_HOST`
//! allowlists a real name if one is ever needed.

mod config;
mod tmux;
mod transcript;

use std::collections::HashMap;
use std::io::Read as _;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use config::{Access, Client};
use serde_json::{json, Value};
use transcript::SessionData;

/// Sessions older than this are history, not a board.
const FRESH_WINDOW: Duration = Duration::from_secs(48 * 3600);
/// The window in `find -mmin` units, kept in lockstep with FRESH_WINDOW.
const FRESH_MMIN: &str = "-2880";
/// The pad pages backward through a session now, so it can read more than
/// one screen — but a board poll every 20s is not the place for a whole
/// transcript. Two dozen turns covers a sitting.
const MAX_TURNS: usize = 24;
const MAX_ARTIFACTS: usize = 5;

/// (mtime, len): the fingerprint that decides whether a transcript re-parses.
type Fingerprint = (SystemTime, u64);

struct Hub {
    clients: Vec<Client>,
    tz_offset_hours: i64,
    /// Parse cache keyed by `client:path`; a transcript that has not changed
    /// is not re-fetched or re-parsed.
    cache: Mutex<HashMap<String, (Fingerprint, SessionData)>>,
    /// The last sessions each client answered with, served as `stale` when
    /// the client stops answering. A stale board that says so beats a hole.
    last_good: Mutex<HashMap<String, Vec<SessionData>>>,
}

/// What every request must pass before the hub will even route it.
struct Gate {
    /// The bearer token, when one is configured. None means the hub bound
    /// loopback and trusts the machine it lives on.
    token: Option<String>,
    /// Hostnames allowed in `Host` beyond IP literals and localhost.
    hosts: Vec<String>,
}

impl Gate {
    fn from_env() -> Gate {
        Gate {
            token: std::env::var("HUB_TOKEN")
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty()),
            hosts: std::env::var("HUB_HOST")
                .map(|h| {
                    h.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// None when the request may proceed; otherwise why it may not.
    fn deny(&self, host: Option<&str>, auth: Option<&str>) -> Option<(u16, &'static str)> {
        // A browser always sends the name it navigated to, so a DNS name
        // here is the rebinding tell. Non-browser clients send the IP they
        // dialed, or nothing.
        if let Some(h) = host {
            if !host_is_safe(h, &self.hosts) {
                return Some((403, "host not recognized"));
            }
        }
        if let Some(token) = &self.token {
            let sent = auth.and_then(|a| a.strip_prefix("Bearer "));
            if !sent.is_some_and(|s| ct_eq(s.trim().as_bytes(), token.as_bytes())) {
                return Some((401, "missing or wrong token"));
            }
        }
        None
    }
}

/// An acceptable `Host`: an IP literal, localhost, or an allowlisted name —
/// with or without a port. `[::1]:9707` keeps its brackets off the IP.
fn host_is_safe(host: &str, extra: &[String]) -> bool {
    let bare = match host.strip_prefix('[') {
        // Bracketed IPv6: the authority is what the brackets hold.
        Some(rest) => match rest.split_once(']') {
            Some((ip, _)) => ip,
            None => return false,
        },
        None => host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host),
    };
    bare.parse::<std::net::IpAddr>().is_ok()
        || bare.eq_ignore_ascii_case("localhost")
        || extra.iter().any(|e| e.eq_ignore_ascii_case(bare))
}

/// Byte comparison that spends the same time on every wrong guess.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn main() {
    let port = std::env::var("HUB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9707u16);
    let gate = Gate::from_env();
    // Loopback by default either way: the pad reaches the hub through the
    // reverse ssh tunnel (scripts/hub-tunnel.sh), so nothing on the LAN
    // needs this port. With a token, HUB_BIND widens it deliberately;
    // without one, a hub that can type into shells refuses to listen
    // beyond the machine it trusts at all.
    let bind = match &gate.token {
        Some(_) => std::env::var("HUB_BIND").unwrap_or_else(|_| "127.0.0.1".to_string()),
        None => {
            eprintln!("anthink-hub: no HUB_TOKEN — loopback only; set one to gate requests");
            "127.0.0.1".to_string()
        }
    };
    let default_projects = format!("{}/.claude/projects", home());
    let clients = config::load(&default_projects);
    for c in &clients {
        let how = match &c.access {
            Access::Local => "local".to_string(),
            Access::Ssh(d) => format!("ssh {d}"),
        };
        eprintln!(
            "anthink-hub: client {} ({how}) projects {}",
            c.name, c.projects
        );
    }
    let hub = Hub {
        clients,
        tz_offset_hours: local_offset_hours(),
        cache: Mutex::new(HashMap::new()),
        last_good: Mutex::new(HashMap::new()),
    };

    let server = match tiny_http::Server::http((bind.as_str(), port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("anthink-hub: cannot bind {bind}:{port}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("anthink-hub: serving on {bind}:{port}");

    // One thread per request: a stalled client must not block the pad's next
    // poll. The Hub is all Mutex-guarded state, so sharing it is safe.
    let hub = std::sync::Arc::new(hub);
    let gate = std::sync::Arc::new(gate);
    for mut request in server.incoming_requests() {
        let hub = hub.clone();
        let gate = gate.clone();
        std::thread::spawn(move || {
            let header = |name: &'static str| {
                request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv(name))
                    .map(|h| h.value.as_str().to_string())
            };
            if let Some((code, why)) = gate.deny(
                header("Host").as_deref(),
                header("Authorization").as_deref(),
            ) {
                let _ = request.respond(err_json(code, why));
                return;
            }
            let method = request.method().to_string();
            let url = request.url().to_string();
            let response = match (method.as_str(), url.as_str()) {
                ("GET", "/sessions") => ok_json(hub.board()),
                ("POST", _) => match nudge_target(&url) {
                    Some(id) => {
                        // A nudge body is tiny; anything larger is not a nudge.
                        let mut body = String::new();
                        let _ = request
                            .as_reader()
                            .take(16 * 1024)
                            .read_to_string(&mut body);
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
        });
    }
}

/// `/sessions/<id>/nudge` → the id, or None for any other path.
fn nudge_target(url: &str) -> Option<String> {
    let rest = url.strip_prefix("/sessions/")?;
    let id = rest.strip_suffix("/nudge")?;
    (!id.is_empty() && !id.contains('/')).then(|| id.to_string())
}

impl Hub {
    /// The board: every fresh session on every client, the neediest first.
    fn board(&self) -> Value {
        let mut rows: Vec<(u8, String, Value)> = Vec::new();
        for client in &self.clients {
            match self.fresh_sessions(client) {
                Some(sessions) => {
                    if let Ok(mut lg) = self.last_good.lock() {
                        lg.insert(client.name.clone(), sessions.clone());
                    }
                    let panes = classified_panes(&client.access);
                    for s in sessions {
                        let state = pane_for(&panes, &s.cwd)
                            .map(|(_, st)| st.word())
                            .unwrap_or("done");
                        let rank = match state {
                            "waiting" => 0,
                            "running" => 1,
                            _ => 2,
                        };
                        rows.push((rank, s.updated_iso.clone(), self.row(client, &s, state)));
                    }
                }
                // Unreachable: the last thing it said, marked as such.
                None => {
                    let held = self
                        .last_good
                        .lock()
                        .ok()
                        .and_then(|lg| lg.get(&client.name).cloned())
                        .unwrap_or_default();
                    for s in held {
                        rows.push((2, s.updated_iso.clone(), self.row(client, &s, "stale")));
                    }
                }
            }
        }
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
                let t = v
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if t.is_empty() {
                    return Err("a text nudge needs text".to_string());
                }
                tmux::Nudge::Text(t.to_string())
            }
            _ => return Err(format!("unknown mark {mark:?}")),
        };
        let (client, sid) = self.route(id)?;
        let session = self
            .fresh_sessions(client)
            .ok_or_else(|| format!("client {} unreachable", client.name))?
            .into_iter()
            .find(|s| s.id == sid)
            .ok_or_else(|| format!("no fresh session {sid} on {}", client.name))?;
        let panes = classified_panes(&client.access);
        let (pane, _) = pane_for(&panes, &session.cwd)
            .ok_or_else(|| format!("no pane is running claude in {}", session.cwd))?;
        tmux::nudge(&client.access, pane, &n)
    }

    /// `client:uuid` → the client and the bare session id. A bare id (an old
    /// pad) routes to the first client.
    fn route(&self, id: &str) -> Result<(&Client, String), String> {
        match id.split_once(':') {
            Some((name, sid)) => self
                .clients
                .iter()
                .find(|c| c.name == name)
                .map(|c| (c, sid.to_string()))
                .ok_or_else(|| format!("unknown client {name}")),
            None => self
                .clients
                .first()
                .map(|c| (c, id.to_string()))
                .ok_or_else(|| "no clients configured".to_string()),
        }
    }

    /// Every transcript touched inside the fresh window, parsed (from cache
    /// where the file has not changed). `None` means the client could not be
    /// asked — which is different from "no sessions".
    fn fresh_sessions(&self, client: &Client) -> Option<Vec<SessionData>> {
        let listing = match &client.access {
            Access::Local => list_local(&client.projects),
            Access::Ssh(_) => list_remote(client)?,
        };
        let mut out = Vec::new();
        for (path, fingerprint) in listing {
            if let Some(s) = self.parse_cached(client, &path, fingerprint) {
                // A session nobody spoke in (a bare boot, a probe) is not a
                // board row.
                if !s.turns.is_empty() {
                    out.push(s);
                }
            }
        }
        Some(out)
    }

    fn parse_cached(
        &self,
        client: &Client,
        path: &str,
        fingerprint: Fingerprint,
    ) -> Option<SessionData> {
        let key = format!("{}:{path}", client.name);
        if let Ok(cache) = self.cache.lock() {
            if let Some((fp, s)) = cache.get(&key) {
                if *fp == fingerprint {
                    return Some(s.clone());
                }
            }
        }
        let id = path.rsplit('/').next()?.strip_suffix(".jsonl")?.to_string();
        let text = match &client.access {
            Access::Local => std::fs::read_to_string(path).ok()?,
            Access::Ssh(_) => {
                let out = tmux::run(&client.access, &["cat", path])?;
                if !out.status.success() {
                    return None;
                }
                String::from_utf8_lossy(&out.stdout).into_owned()
            }
        };
        let s = transcript::parse(&id, &text);
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(key, (fingerprint, s.clone()));
        }
        Some(s)
    }

    fn row(&self, client: &Client, s: &SessionData, state: &str) -> Value {
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
            "id": format!("{}:{}", client.name, s.id),
            "title": s.title,
            "state": state,
            "updated": transcript::clock(&s.updated_iso, self.tz_offset_hours),
            // Where the session works. The pad shows it on the board — which
            // project an agent is in beats its last sentence at a glance.
            "cwd": s.cwd,
            "turns": turns,
            "artifacts": artifacts,
        })
    }
}

/// Fresh transcripts on this machine: path and fingerprint, cheap to walk.
fn list_local(projects: &str) -> Vec<(String, Fingerprint)> {
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(projects) else {
        return out;
    };
    let now = SystemTime::now();
    for project in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = f.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            if now
                .duration_since(mtime)
                .map(|d| d > FRESH_WINDOW)
                .unwrap_or(true)
            {
                continue;
            }
            if let Some(p) = path.to_str() {
                out.push((p.to_string(), (mtime, meta.len())));
            }
        }
    }
    out
}

/// Fresh transcripts on a remote client, one `find` over ssh. `None` when
/// the client cannot be reached; changed files are fetched by the parse
/// cache, so a quiet fleet costs one round trip per client per poll.
fn list_remote(client: &Client) -> Option<Vec<(String, Fingerprint)>> {
    let out = tmux::run(
        &client.access,
        &[
            "find",
            &client.projects,
            "-name",
            "*.jsonl",
            "-mmin",
            FRESH_MMIN,
            "-printf",
            "%T@\\t%s\\t%p\\n",
        ],
    )?;
    if !out.status.success() {
        return None;
    }
    Some(parse_find_listing(&String::from_utf8_lossy(&out.stdout)))
}

/// What `find -printf '%T@\t%s\t%p\n'` emits → paths and fingerprints.
fn parse_find_listing(text: &str) -> Vec<(String, Fingerprint)> {
    let mut listing = Vec::new();
    for line in text.lines() {
        let mut f = line.split('\t');
        let (Some(t), Some(size), Some(path)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let Some(secs) = t.split('.').next().and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let Ok(size) = size.parse::<u64>() else {
            continue;
        };
        let mtime = UNIX_EPOCH + Duration::from_secs(secs);
        listing.push((path.to_string(), (mtime, size)));
    }
    listing
}

/// Every claude pane on one client with its state read once. One capture per
/// pane per request, not per session sharing it. Unreachable reads as no
/// panes — the sessions listing is the reachability check.
fn classified_panes(access: &Access) -> Vec<(tmux::Pane, tmux::PaneState)> {
    tmux::claude_panes(access)
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let st = tmux::classify(&tmux::capture(access, &p.id));
            (p, st)
        })
        .collect()
}

/// The pane whose working directory is the session's. The picker shares a
/// directory with real work, so it can never represent a session: only
/// non-picker panes match, and a session whose sole pane is the picker gets
/// none (its state reads as done, and a nudge to it is refused).
fn pane_for<'a>(
    panes: &'a [(tmux::Pane, tmux::PaneState)],
    cwd: &str,
) -> Option<&'a (tmux::Pane, tmux::PaneState)> {
    if cwd.is_empty() {
        return None;
    }
    panes
        .iter()
        .find(|(p, st)| p.path == cwd && *st != tmux::PaneState::Picker)
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
        .with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap(),
        )
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
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

    fn hub_with(clients: Vec<Client>) -> Hub {
        Hub {
            clients,
            tz_offset_hours: -10,
            cache: Mutex::new(HashMap::new()),
            last_good: Mutex::new(HashMap::new()),
        }
    }

    fn client(name: &str) -> Client {
        Client {
            name: name.into(),
            access: Access::Local,
            projects: "/nonexistent".into(),
        }
    }

    #[test]
    fn nudge_paths_parse_and_reject() {
        assert_eq!(
            nudge_target("/sessions/laptop:abc-123/nudge").as_deref(),
            Some("laptop:abc-123")
        );
        assert_eq!(nudge_target("/sessions//nudge"), None);
        assert_eq!(nudge_target("/sessions/a/b/nudge"), None);
        assert_eq!(nudge_target("/other"), None);
    }

    #[test]
    fn ids_route_to_their_client_and_bare_ids_to_the_first() {
        let hub = hub_with(vec![client("laptop"), client("gauntlet")]);
        let (c, sid) = hub.route("gauntlet:abc").unwrap();
        assert_eq!(c.name, "gauntlet");
        assert_eq!(sid, "abc");
        let (c, sid) = hub.route("abc").unwrap();
        assert_eq!(c.name, "laptop");
        assert_eq!(sid, "abc");
        assert!(hub.route("nobody:abc").is_err());
    }

    #[test]
    fn rows_carry_the_bridge_shape_with_prefixed_ids() {
        let hub = hub_with(vec![client("laptop")]);
        let s = SessionData {
            id: "s1".into(),
            title: "Wire the bridge".into(),
            cwd: "/x".into(),
            updated_iso: "2026-08-30T18:04:00.000Z".into(),
            turns: vec![transcript::Turn {
                speaker: "you".into(),
                text: "go".into(),
            }],
            artifacts: vec![transcript::Artifact {
                reference: "a1b2c3d".into(),
                label: "l".into(),
            }],
        };
        let r = hub.row(&hub.clients[0], &s, "waiting");
        assert_eq!(r["id"], "laptop:s1");
        assert_eq!(r["state"], "waiting");
        assert_eq!(r["updated"], "08:04");
        assert_eq!(r["cwd"], "/x");
        assert_eq!(r["turns"][0]["speaker"], "you");
        assert_eq!(r["artifacts"][0]["ref"], "a1b2c3d");
    }

    #[test]
    fn only_the_last_turns_ride_and_order_survives() {
        let hub = hub_with(vec![client("laptop")]);
        let turns: Vec<transcript::Turn> = (0..30)
            .map(|i| transcript::Turn {
                speaker: "claude".into(),
                text: format!("t{i}"),
            })
            .collect();
        let s = SessionData {
            turns,
            ..Default::default()
        };
        let r = hub.row(&hub.clients[0], &s, "running");
        let sent = r["turns"].as_array().unwrap();
        assert_eq!(sent.len(), MAX_TURNS);
        assert_eq!(sent[0]["text"], "t6");
        assert_eq!(sent[MAX_TURNS - 1]["text"], "t29");
    }

    #[test]
    fn hosts_pass_as_ips_and_fail_as_names() {
        let none: &[String] = &[];
        for ok in [
            "192.168.1.76:9707",
            "192.168.1.76",
            "127.0.0.1:9707",
            "localhost:9707",
            "LOCALHOST",
            "[::1]:9707",
            "[fe80::1]",
        ] {
            assert!(host_is_safe(ok, none), "{ok} should pass");
        }
        for bad in ["evil.example", "evil.example:9707", "hub.lan:9707", "[::1"] {
            assert!(!host_is_safe(bad, none), "{bad} should fail");
        }
        let extra = vec!["hub.lan".to_string()];
        assert!(host_is_safe("hub.lan:9707", &extra));
        assert!(host_is_safe("HUB.LAN", &extra));
        assert!(!host_is_safe("evil.example", &extra));
    }

    #[test]
    fn the_gate_requires_the_exact_token_and_refuses_dns_hosts() {
        let gate = Gate {
            token: Some("s3cret".into()),
            hosts: vec![],
        };
        // The pad: IP host, right token.
        assert!(gate
            .deny(Some("192.168.1.76:9707"), Some("Bearer s3cret"))
            .is_none());
        // No token, wrong token, wrong scheme: 401.
        assert_eq!(gate.deny(Some("192.168.1.76:9707"), None).unwrap().0, 401);
        assert_eq!(
            gate.deny(Some("192.168.1.76:9707"), Some("Bearer nope"))
                .unwrap()
                .0,
            401
        );
        assert_eq!(
            gate.deny(Some("192.168.1.76:9707"), Some("s3cret"))
                .unwrap()
                .0,
            401
        );
        // A rebound browser sends its DNS name: 403 before auth is weighed.
        assert_eq!(
            gate.deny(Some("evil.example"), Some("Bearer s3cret"))
                .unwrap()
                .0,
            403
        );
        // A raw client with no Host still needs the token.
        assert!(gate.deny(None, Some("Bearer s3cret")).is_none());
        // Loopback-only hub (no token): host rule still holds, auth does not.
        let open = Gate {
            token: None,
            hosts: vec![],
        };
        assert!(open.deny(Some("127.0.0.1:9707"), None).is_none());
        assert_eq!(open.deny(Some("evil.example"), None).unwrap().0, 403);
    }

    #[test]
    fn token_comparison_is_exact() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"a"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn remote_listings_parse_find_output_and_skip_junk() {
        let rows = parse_find_listing(
            "1756600000.1234\t2048\t/root/.claude/projects/-x/s1.jsonl\nnot a line\n\t\t\n",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "/root/.claude/projects/-x/s1.jsonl");
        assert_eq!(rows[0].1 .1, 2048);
    }
}
