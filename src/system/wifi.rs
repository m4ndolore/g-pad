//! Wi-Fi over `nmcli`. reMarkable OS 3.28 handed the radio to NetworkManager
//! (wpa_supplicant runs D-Bus-only, without the control socket wpa_cli
//! needs), and `power::wifi_heal` leans on the same tool after resume. Every
//! call runs on a worker thread and reports over a channel; the pad loop
//! never waits on the radio.
//!
//! Terse output (`-t`) separates fields with `:` and escapes a `:` or `\`
//! inside a value as `\:` and `\\`; `fields` undoes that.

use std::collections::HashSet;
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

/// The current connection, from `dev show` plus the in-use row of the scan
/// list for its signal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub ssid: Option<String>,
    pub connected: bool,
    pub ip: Option<String>,
    pub rssi: Option<i32>,
}

/// One saved Wi-Fi connection from `con show`. `ssid` is the connection's
/// name, which is what nmcli activates; `id` is the row's place in the list
/// the page was handed, the handle a tap carries back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub id: u32,
    pub ssid: String,
    pub current: bool,
    pub disabled: bool,
}

/// One network in range, strongest access point only; `saved_id` when it is
/// a saved connection's name; `secured` when joining it needs a password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub ssid: String,
    pub rssi: i32,
    pub saved_id: Option<u32>,
    pub secured: bool,
}

/// One row of the scan list as nmcli prints it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heard {
    pub in_use: bool,
    pub ssid: String,
    pub signal: u8,
    pub security: String,
}

/// What the page shows. `busy` names the command in flight, if any.
///
/// The saved and in-range rows are one list that rarely fits the page.
/// `offset` is the first entry drawn; drawing sets `next_offset` to where a
/// MORE tap should move it (or 0 from the last page), None when the whole
/// list is on screen. A fresh list starts at the top again.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub status: Status,
    pub saved: Vec<Saved>,
    pub seen: Vec<Seen>,
    pub busy: Option<&'static str>,
    pub error: Option<String>,
    pub offset: usize,
    pub next_offset: Option<usize>,
}

impl View {
    /// Claim the worker for `label`, clearing the last error. False while
    /// another command is still in flight; the caller then spawns nothing.
    pub fn begin(&mut self, label: &'static str) -> bool {
        if self.busy.is_some() {
            return false;
        }
        self.busy = Some(label);
        self.error = None;
        true
    }

    /// The join for the saved row a tap named, or None when the list moved
    /// on since the page painted it.
    pub fn select(&self, id: u32) -> Option<Cmd> {
        self.saved
            .iter()
            .find(|s| s.id == id)
            .map(|s| Cmd::Select(s.ssid.clone()))
    }

    /// The in-range row a tap named that is not saved: its name and whether
    /// it needs a password. None for a saved row or a stale index.
    pub fn new_network(&self, index: u32) -> Option<(String, bool)> {
        self.seen
            .get(index as usize)
            .filter(|s| s.saved_id.is_none())
            .map(|s| (s.ssid.clone(), s.secured))
    }
}

/// What the page asks of the worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Refresh,
    Scan,
    Select(String),
    /// Join a network the tablet has no connection for; NetworkManager
    /// saves one on success. `password` is None for an open network.
    Join { ssid: String, password: Option<String> },
}

/// What the worker sends back; `Failed` carries nmcli's first line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Status(Status),
    Saved(Vec<Saved>),
    Seen(Vec<Seen>),
    Failed(String),
}

const IFACE: &str = "wlan0";
/// A rescan is asynchronous; the list answers from the previous scan until
/// the new one lands, which takes a few seconds.
const SCAN_SETTLE: Duration = Duration::from_secs(4);
/// How long `con up` may take before it is reported as a failure.
const JOIN_WAIT_SECS: &str = "10";

const DEV_SHOW: &[&str] = &[
    "-t",
    "-f",
    "GENERAL.STATE,GENERAL.CONNECTION,IP4.ADDRESS",
    "dev",
    "show",
    IFACE,
];
const WIFI_LIST: &[&str] = &[
    "-t",
    "-f",
    "IN-USE,SSID,SIGNAL,SECURITY",
    "dev",
    "wifi",
    "list",
    "--rescan",
    "no",
];
const CON_SHOW: &[&str] = &["-t", "-f", "NAME,TYPE,ACTIVE,AUTOCONNECT", "con", "show"];

/// Run `cmd` on its own thread; the results arrive on `tx`. A dropped
/// receiver is not an error — the page may have closed meanwhile.
pub fn spawn(cmd: Cmd, tx: Sender<Event>) {
    std::thread::spawn(move || {
        let send = |e: Event| {
            let _ = tx.send(e);
        };
        match cmd {
            Cmd::Refresh => refresh(&send),
            Cmd::Scan => {
                // A rescan right after another is refused ("not allowed
                // immediately following previous scan"); the list still
                // answers from that one, so only other failures count.
                if let Err(e) = nm(&["dev", "wifi", "rescan"]) {
                    if !e.contains("not allowed") {
                        return send(Event::Failed(e));
                    }
                }
                std::thread::sleep(SCAN_SETTLE);
                match (nm(WIFI_LIST), nm(CON_SHOW)) {
                    (Ok(list), Ok(cons)) => send(Event::Seen(seen_from(
                        &parse_wifi_list(&list),
                        &parse_con_show(&cons),
                    ))),
                    (Err(e), _) | (_, Err(e)) => send(Event::Failed(e)),
                }
            }
            Cmd::Select(name) => {
                // NetworkManager owns roaming: activating a saved connection
                // is one call that waits for the join or says why it failed.
                if let Err(e) = nm(&["--wait", JOIN_WAIT_SECS, "con", "up", "id", &name]) {
                    return send(Event::Failed(e));
                }
                refresh(&send);
            }
            Cmd::Join { ssid, password } => {
                // The password rides on nmcli's command line, briefly visible
                // to a process listing on the tablet — the same trust
                // boundary as oracle.env on a single-user device.
                let mut args = vec!["--wait", JOIN_WAIT_SECS, "dev", "wifi", "connect", &ssid];
                if let Some(p) = &password {
                    args.extend(["password", p.as_str()]);
                }
                if let Err(e) = nm(&args) {
                    return send(Event::Failed(e));
                }
                refresh(&send);
            }
        }
    });
}

/// Status then the saved list. The scan list only supplies the signal, so
/// its absence is not an error.
fn refresh(send: &dyn Fn(Event)) {
    let heard = nm(WIFI_LIST).map(|t| parse_wifi_list(&t)).unwrap_or_default();
    match nm(DEV_SHOW) {
        Ok(dev) => send(Event::Status(status_from(&dev, &heard))),
        Err(e) => return send(Event::Failed(e)),
    }
    match nm(CON_SHOW) {
        Ok(cons) => send(Event::Saved(parse_con_show(&cons))),
        Err(e) => send(Event::Failed(e)),
    }
}

/// Run one nmcli command; the error is its first line, for the status row.
fn nm(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nmcli")
        .args(args)
        .output()
        .map_err(|e| format!("nmcli: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        return Err(first_line(&String::from_utf8_lossy(&out.stderr), &text));
    }
    Ok(text)
}

/// The first non-blank line of stderr, else of stdout, else a stand-in:
/// one line the status row can show.
fn first_line(stderr: &str, stdout: &str) -> String {
    stderr
        .lines()
        .chain(stdout.lines())
        .find(|l| !l.trim().is_empty())
        .unwrap_or("nmcli failed")
        .to_string()
}

/// Split one terse line into its fields, undoing nmcli's `\:` and `\\`.
pub fn fields(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    out.last_mut().expect("one field").push(escaped);
                }
            }
            ':' => out.push(String::new()),
            c => out.last_mut().expect("one field").push(c),
        }
    }
    out
}

/// NetworkManager reports strength as a percentage it derives as
/// 2 × (dBm + 100); the inverse gives the RSSI the bars are read from.
pub fn pct_to_rssi(pct: u8) -> i32 {
    i32::from(pct) / 2 - 100
}

/// `dev show` output (`KEY:value` lines) plus the scan list for the signal
/// of the in-use row. State `100 (connected)` is the only connected state.
pub fn status_from(dev_show: &str, heard: &[Heard]) -> Status {
    let value = |key: &str| {
        dev_show
            .lines()
            .map(fields)
            .find(|f| f.first().map(String::as_str) == Some(key))
            .and_then(|f| f.get(1).cloned())
    };
    let nonempty = |v: String| (!v.is_empty() && v != "--").then_some(v);
    Status {
        ssid: value("GENERAL.CONNECTION").and_then(nonempty),
        connected: value("GENERAL.STATE").is_some_and(|s| s.trim_start().starts_with("100")),
        ip: value("IP4.ADDRESS[1]")
            .and_then(nonempty)
            .map(|v| v.split('/').next().unwrap_or_default().to_string()),
        rssi: heard.iter().find(|h| h.in_use).map(|h| pct_to_rssi(h.signal)),
    }
}

/// `dev wifi list` rows: `IN-USE:SSID:SIGNAL:SECURITY`. Hidden networks and
/// rows that do not parse are dropped.
pub fn parse_wifi_list(text: &str) -> Vec<Heard> {
    text.lines()
        .filter_map(|line| {
            let f = fields(line);
            if f.len() < 3 || f[1].is_empty() {
                return None;
            }
            Some(Heard {
                in_use: f[0].trim() == "*",
                ssid: f[1].clone(),
                signal: f[2].trim().parse().ok()?,
                security: f.get(3).map(|s| s.trim().to_string()).unwrap_or_default(),
            })
        })
        .collect()
}

/// `con show` rows: `NAME:TYPE:ACTIVE:AUTOCONNECT`. Wi-Fi connections only,
/// numbered by their place in the list; one that will not autoconnect shows
/// as disabled.
pub fn parse_con_show(text: &str) -> Vec<Saved> {
    text.lines()
        .map(fields)
        .filter(|f| f.len() >= 4 && f[1] == "802-11-wireless")
        .enumerate()
        .map(|(i, f)| Saved {
            id: i as u32,
            ssid: f[0].clone(),
            current: f[2] == "yes",
            disabled: f[3] == "no",
        })
        .collect()
}

/// The scan list for the page: strongest first, one row per SSID (a
/// dual-band router answers once per access point), saved networks pointed
/// back at their row.
pub fn seen_from(heard: &[Heard], saved: &[Saved]) -> Vec<Seen> {
    let mut seen: Vec<Seen> = heard
        .iter()
        .map(|h| Seen {
            ssid: h.ssid.clone(),
            rssi: pct_to_rssi(h.signal),
            saved_id: saved.iter().find(|s| s.ssid == h.ssid).map(|s| s.id),
            secured: !h.security.is_empty() && h.security != "--",
        })
        .collect();
    seen.sort_by_key(|s| std::cmp::Reverse(s.rssi));
    let mut named = HashSet::new();
    seen.retain(|s| named.insert(s.ssid.clone()));
    seen
}

/// Signal as 0–4 bars, the way a status line reads it.
pub fn bars(rssi: i32) -> u8 {
    match rssi {
        r if r >= -50 => 4,
        r if r >= -65 => 3,
        r if r >= -75 => 2,
        r if r >= -85 => 1,
        _ => 0,
    }
}

/// `g-pad --wifi-test`: one refresh and one scan through the worker,
/// printed, so the nmcli path can be checked over ssh without the page.
pub fn diagnostic() -> i32 {
    let mut failed = false;
    for (cmd, expect) in [(Cmd::Refresh, 2), (Cmd::Scan, 1)] {
        let (tx, rx) = mpsc::channel();
        spawn(cmd, tx);
        for _ in 0..expect {
            match rx.recv_timeout(Duration::from_secs(30)) {
                Ok(Event::Status(s)) => println!("status: {s:?}"),
                Ok(Event::Saved(v)) => {
                    println!("saved: {}", v.len());
                    for s in v {
                        let mark = if s.current { " *" } else { "" };
                        println!("  {} {}{mark}", s.id, s.ssid);
                    }
                }
                Ok(Event::Seen(v)) => {
                    println!("seen: {}", v.len());
                    for s in v {
                        let saved = s.saved_id.map(|i| format!(" (saved {i})")).unwrap_or_default();
                        println!("  {:>4} dBm {}{saved}", s.rssi, s.ssid);
                    }
                }
                Ok(Event::Failed(e)) => {
                    println!("failed: {e}");
                    failed = true;
                    break;
                }
                Err(_) => {
                    println!("timed out");
                    failed = true;
                    break;
                }
            }
        }
    }
    if failed {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terse_fields_split_on_colons_and_undo_escapes() {
        assert_eq!(fields(r"*:cafe\:bar:80:WPA2"), vec!["*", "cafe:bar", "80", "WPA2"]);
        assert_eq!(fields(r"a\\b:c"), vec![r"a\b", "c"]);
        assert_eq!(fields(""), vec![""]);
    }

    #[test]
    fn status_reads_state_connection_ip_and_signal() {
        let dev = "GENERAL.STATE:100 (connected)\nGENERAL.CONNECTION:spaceship-321\nIP4.ADDRESS[1]:192.168.1.78/24\n";
        let heard = parse_wifi_list("*:spaceship-321:78:WPA2\n :iot:95:WPA2\n");
        assert_eq!(
            status_from(dev, &heard),
            Status { ssid: Some("spaceship-321".into()), connected: true, ip: Some("192.168.1.78".into()), rssi: Some(-61) }
        );
        let down = "GENERAL.STATE:30 (disconnected)\nGENERAL.CONNECTION:--\n";
        assert_eq!(status_from(down, &[]), Status { ssid: None, connected: false, ip: None, rssi: None });
    }

    #[test]
    fn saved_connections_are_wifi_only_numbered_in_order_with_flags() {
        let text = "spaceship-321:802-11-wireless:yes:yes\nlo:loopback:yes:yes\nGuest@MITRE:802-11-wireless:no:no\nmerged199:802-11-wireless:no:yes\n";
        assert_eq!(parse_con_show(text), vec![
            Saved { id: 0, ssid: "spaceship-321".into(), current: true, disabled: false },
            Saved { id: 1, ssid: "Guest@MITRE".into(), current: false, disabled: true },
            Saved { id: 2, ssid: "merged199".into(), current: false, disabled: false },
        ]);
    }

    #[test]
    fn scan_rows_sort_strongest_first_dedupe_and_mark_saved() {
        // The rM2's own list: a dual-band router answers once per band, a
        // hidden network prints an empty SSID, and one row is garbage.
        let text = " :iot dont use me:95:WPA2\n :spaceship-321:70:WPA2\n ::79:WPA2\n :spaceship-321:84:WPA2\ngarbage\n :cafe:weak:WPA2\n :open cafe:60:\n";
        let saved = parse_con_show("spaceship-321:802-11-wireless:yes:yes\n");
        assert_eq!(seen_from(&parse_wifi_list(text), &saved), vec![
            Seen { ssid: "iot dont use me".into(), rssi: -53, saved_id: None, secured: true },
            Seen { ssid: "spaceship-321".into(), rssi: -58, saved_id: Some(0), secured: true },
            Seen { ssid: "open cafe".into(), rssi: -70, saved_id: None, secured: false },
        ]);
    }

    #[test]
    fn a_new_network_is_named_only_when_it_is_not_saved() {
        let saved = parse_con_show("home:802-11-wireless:yes:yes\n");
        let v = View {
            seen: seen_from(&parse_wifi_list("*:home:80:WPA2\n :cafe:60:\n :office:70:WPA3\n"), &saved),
            saved,
            ..View::default()
        };
        // Strongest first: home (80%), office (70%), cafe (60%).
        assert_eq!(v.new_network(0), None, "a saved row joins through select");
        assert_eq!(v.new_network(1), Some(("office".into(), true)));
        assert_eq!(v.new_network(2), Some(("cafe".into(), false)));
        assert_eq!(v.new_network(9), None);
    }

    #[test]
    fn percent_maps_back_to_dbm() {
        assert_eq!(pct_to_rssi(100), -50);
        assert_eq!(pct_to_rssi(70), -65);
        assert_eq!(pct_to_rssi(0), -100);
    }

    #[test]
    fn a_tap_selects_by_the_row_the_page_painted() {
        let v = View { saved: parse_con_show("home:802-11-wireless:yes:yes\ncafe:802-11-wireless:no:yes\n"), ..View::default() };
        assert_eq!(v.select(1), Some(Cmd::Select("cafe".into())));
        assert_eq!(v.select(7), None, "a row that is no longer there joins nothing");
    }

    #[test]
    fn first_line_prefers_stderr_then_stdout_then_a_stand_in() {
        assert_eq!(first_line("", "busy\n"), "busy");
        assert_eq!(
            first_line("Error: Connection activation failed: (7) Secrets were required\n", ""),
            "Error: Connection activation failed: (7) Secrets were required"
        );
        assert_eq!(first_line("", ""), "nmcli failed");
    }

    #[test]
    fn begin_claims_the_worker_once_until_it_finishes() {
        let mut v = View { error: Some("OLD".into()), ..View::default() };
        assert!(v.begin("SCANNING"));
        assert_eq!(v.busy, Some("SCANNING"));
        assert_eq!(v.error, None, "a fresh command clears the last error");
        assert!(!v.begin("JOINING"), "a second command waits its turn");
        assert_eq!(v.busy, Some("SCANNING"));
        v.busy = None;
        assert!(v.begin("JOINING"));
        assert_eq!(v.busy, Some("JOINING"));
    }

    #[test]
    fn rssi_reads_as_bars() {
        assert_eq!(bars(-40), 4);
        assert_eq!(bars(-60), 3);
        assert_eq!(bars(-70), 2);
        assert_eq!(bars(-80), 1);
        assert_eq!(bars(-95), 0);
    }
}
