//! Wi-Fi over `wpa_cli -i wlan0`, the tool `power::wifi_heal` already
//! trusts after resume. Every call runs on a worker thread and reports over
//! a channel; the pad loop never waits on the radio.

use std::collections::HashSet;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::Duration;

/// The current connection, from `status` plus `signal_poll`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub ssid: Option<String>,
    pub connected: bool,
    pub ip: Option<String>,
    pub rssi: Option<i32>,
}

/// One row of `list_networks`: a network wpa_supplicant already knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub id: u32,
    pub ssid: String,
    pub current: bool,
    pub disabled: bool,
}

/// One network in range, strongest BSSID only; `saved_id` when it is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub ssid: String,
    pub rssi: i32,
    pub saved_id: Option<u32>,
}

/// What the page shows. `busy` names the command in flight, if any.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub status: Status,
    pub saved: Vec<Saved>,
    pub seen: Vec<Seen>,
    pub busy: Option<&'static str>,
    pub error: Option<String>,
}

/// What the page asks of the worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Refresh,
    Scan,
    Select(u32),
}

/// What the worker sends back; `Failed` carries wpa_cli's first line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Status(Status),
    Saved(Vec<Saved>),
    Seen(Vec<Seen>),
    Failed(String),
}

/// A scan needs a few seconds before `scan_results` has the answers.
const SCAN_SETTLE: Duration = Duration::from_secs(4);
/// Association plus DHCP, before `status` reads the new network.
const SELECT_SETTLE: Duration = Duration::from_secs(5);

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
                if let Err(e) = wpa(&["scan"]) {
                    return send(Event::Failed(e));
                }
                std::thread::sleep(SCAN_SETTLE);
                match (wpa(&["scan_results"]), wpa(&["list_networks"])) {
                    (Ok(res), Ok(list)) => send(Event::Seen(parse_scan_results(
                        &res,
                        &parse_list_networks(&list),
                    ))),
                    (Err(e), _) | (_, Err(e)) => send(Event::Failed(e)),
                }
            }
            Cmd::Select(id) => {
                // select_network disables every other network; enable them
                // again right after so roaming keeps working.
                let id = id.to_string();
                for args in [
                    vec!["select_network", id.as_str()],
                    vec!["enable_network", "all"],
                    vec!["reassociate"],
                ] {
                    if let Err(e) = wpa(&args) {
                        return send(Event::Failed(e));
                    }
                }
                std::thread::sleep(SELECT_SETTLE);
                refresh(&send);
            }
        }
    });
}

/// Status then the saved list. `signal_poll` fails while disconnected, so
/// its absence is not an error.
fn refresh(send: &dyn Fn(Event)) {
    match wpa(&["status"]) {
        Ok(status) => send(Event::Status(parse_status(
            &status,
            wpa(&["signal_poll"]).ok().as_deref(),
        ))),
        Err(e) => return send(Event::Failed(e)),
    }
    match wpa(&["list_networks"]) {
        Ok(list) => send(Event::Saved(parse_list_networks(&list))),
        Err(e) => send(Event::Failed(e)),
    }
}

/// Run one wpa_cli command; the error is its first line, for the status row.
fn wpa(args: &[&str]) -> Result<String, String> {
    let out = Command::new("wpa_cli")
        .args(["-i", "wlan0"])
        .args(args)
        .output()
        .map_err(|e| format!("wpa_cli: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() || text.trim_start().starts_with("FAIL") {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err
            .lines()
            .chain(text.lines())
            .next()
            .unwrap_or("wpa_cli failed")
            .to_string());
    }
    Ok(text)
}

/// The value of the first `key=value` line, if any.
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.trim())
}

/// `status` output plus, when connected, `signal_poll` for the RSSI.
pub fn parse_status(status: &str, signal: Option<&str>) -> Status {
    let nonempty = |v: &str| (!v.is_empty()).then(|| v.to_string());
    Status {
        ssid: field(status, "ssid").and_then(nonempty),
        connected: field(status, "wpa_state") == Some("COMPLETED"),
        ip: field(status, "ip_address").and_then(nonempty),
        rssi: signal
            .and_then(|s| field(s, "RSSI"))
            .and_then(|v| v.parse().ok()),
    }
}

/// `list_networks` output: a header, then `id \t ssid \t bssid \t flags`.
pub fn parse_list_networks(text: &str) -> Vec<Saved> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let id = cols.next()?.trim().parse().ok()?;
            let ssid = cols.next()?.to_string();
            let flags = cols.nth(1).unwrap_or("");
            Some(Saved {
                id,
                ssid,
                current: flags.contains("[CURRENT]"),
                disabled: flags.contains("[DISABLED]"),
            })
        })
        .collect()
}

/// `scan_results` output: a header, then
/// `bssid \t freq \t signal \t flags \t ssid`. Hidden networks are dropped,
/// the list is strongest first, and an SSID heard on several BSSIDs keeps
/// only its strongest.
pub fn parse_scan_results(text: &str, saved: &[Saved]) -> Vec<Seen> {
    let mut seen: Vec<Seen> = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() < 5 || cols[4].is_empty() {
                return None;
            }
            let ssid = cols[4].to_string();
            Some(Seen {
                rssi: cols[2].trim().parse().ok()?,
                saved_id: saved.iter().find(|s| s.ssid == ssid).map(|s| s.id),
                ssid,
            })
        })
        .collect();
    seen.sort_by_key(|s| -s.rssi);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_ssid_state_ip_and_signal() {
        let status = "bssid=aa:bb:cc:dd:ee:ff\nfreq=2437\nssid=spaceship-321\nid=0\nmode=station\nwpa_state=COMPLETED\nip_address=192.168.1.230\n";
        let s = parse_status(status, Some("RSSI=-61\nLINKSPEED=72\nNOISE=9999\nFREQUENCY=2437\n"));
        assert_eq!(s, Status { ssid: Some("spaceship-321".into()), connected: true, ip: Some("192.168.1.230".into()), rssi: Some(-61) });
        let d = parse_status("wpa_state=DISCONNECTED\n", None);
        assert_eq!(d, Status { ssid: None, connected: false, ip: None, rssi: None });
    }

    #[test]
    fn saved_networks_keep_id_ssid_and_flags() {
        let text = "network id / ssid / bssid / flags\n0\tspaceship-321\tany\t[CURRENT]\n1\tiot\tany\t[DISABLED]\n2\tcafe\tany\t\n";
        let nets = parse_list_networks(text);
        assert_eq!(nets, vec![
            Saved { id: 0, ssid: "spaceship-321".into(), current: true, disabled: false },
            Saved { id: 1, ssid: "iot".into(), current: false, disabled: true },
            Saved { id: 2, ssid: "cafe".into(), current: false, disabled: false },
        ]);
    }

    #[test]
    fn scan_results_sort_strongest_first_and_mark_saved() {
        let text = "bssid / frequency / signal level / flags / ssid\naa:aa:aa:aa:aa:aa\t2437\t-71\t[WPA2-PSK-CCMP][ESS]\tiot\nbb:bb:bb:bb:bb:bb\t5180\t-52\t[WPA2-PSK-CCMP][ESS]\tspaceship-321\ncc:cc:cc:cc:cc:cc\t2412\t-80\t[ESS]\t\n";
        let saved = parse_list_networks("network id / ssid / bssid / flags\n0\tspaceship-321\tany\t[CURRENT]\n");
        let seen = parse_scan_results(text, &saved);
        assert_eq!(seen.len(), 2, "hidden SSIDs are dropped");
        assert_eq!(seen[0].ssid, "spaceship-321");
        assert_eq!(seen[0].rssi, -52);
        assert_eq!(seen[0].saved_id, Some(0));
        assert_eq!(seen[1].saved_id, None);
    }

    #[test]
    fn same_ssid_on_two_bands_keeps_the_strongest() {
        // A dual-band router answers once per BSSID; the list shows it once.
        // Sorted by signal the rows read S -49, C -55, S -58, C -70: the
        // duplicates are not neighbours, so a consecutive-only dedup would
        // keep all four. Only a seen-set filter passes this.
        let text = "bssid / frequency / signal level / flags / ssid\n\
            aa:aa:aa:aa:aa:aa\t2437\t-58\t[WPA2-PSK-CCMP][ESS]\tspaceship-321\n\
            dd:dd:dd:dd:dd:dd\t2462\t-55\t[WPA2-PSK-CCMP][ESS]\tcafe\n\
            bb:bb:bb:bb:bb:bb\t5180\t-49\t[WPA2-PSK-CCMP][ESS]\tspaceship-321\n\
            ee:ee:ee:ee:ee:ee\t5240\t-70\t[WPA2-PSK-CCMP][ESS]\tcafe\n";
        let seen = parse_scan_results(text, &[]);
        assert_eq!(seen, vec![
            Seen { ssid: "spaceship-321".into(), rssi: -49, saved_id: None },
            Seen { ssid: "cafe".into(), rssi: -55, saved_id: None },
        ]);
    }

    #[test]
    fn rssi_reads_as_bars() {
        assert_eq!(bars(-40), 4); assert_eq!(bars(-60), 3); assert_eq!(bars(-70), 2); assert_eq!(bars(-80), 1); assert_eq!(bars(-95), 0);
    }
}
