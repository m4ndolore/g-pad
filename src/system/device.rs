//! Read-only device facts for the DEVICE section. Every reader degrades to
//! a word rather than an error: the page must never go blank.
//!
//! Each fact is a thin reader over a pure formatter that takes the raw
//! strings, so the formatting is tested without a tablet under it.

use std::time::Duration;

/// One line per row of the DEVICE section, already worded for the page.
pub struct Facts {
    pub battery: String,
    pub storage: String,
    pub clock: String,
    pub os: String,
    pub build: String,
    pub hub: String,
}

pub fn gather() -> Facts {
    Facts {
        battery: battery(),
        storage: storage(),
        clock: clock_hhmm(unix_now(), tz_offset()),
        os: os_line(&std::fs::read_to_string("/etc/os-release").unwrap_or_default()),
        build: crate::BUILD.to_string(),
        hub: hub_line(crate::bridge::last_ok_age()),
    }
}

/// Walk /sys/class/power_supply and hand every supply's `type`, `capacity`
/// and `status` to the formatter. A supply that cannot be read is an empty
/// string, not an error: the formatter decides what that means.
fn battery() -> String {
    let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else {
        return battery_from(&[]);
    };
    let read = |p: &std::path::Path, name: &str| std::fs::read_to_string(p.join(name)).unwrap_or_default();
    let supplies: Vec<(String, String, String)> = dir
        .flatten()
        .map(|e| {
            let p = e.path();
            (read(&p, "type"), read(&p, "capacity"), read(&p, "status"))
        })
        .collect();
    let borrowed: Vec<(&str, &str, &str)> =
        supplies.iter().map(|(t, c, s)| (t.as_str(), c.as_str(), s.as_str())).collect();
    battery_from(&borrowed)
}

/// The first supply of type `Battery` speaks; anything else (mains only,
/// nothing at all) reads UNKNOWN. Missing readings on a real battery show
/// as `?` and UNKNOWN so the row still says a battery exists.
pub fn battery_from(entries: &[(&str, &str, &str)]) -> String {
    let Some((_, capacity, status)) = entries.iter().find(|(kind, _, _)| kind.trim() == "Battery") else {
        return "BATTERY: UNKNOWN".into();
    };
    let capacity = capacity.trim();
    let capacity = if capacity.is_empty() { "?" } else { capacity };
    let status = status.trim();
    let status = if status.is_empty() { "UNKNOWN".to_string() } else { status.to_uppercase() };
    format!("BATTERY: {capacity}% {status}")
}

/// Free and total space on /home, where notes and the vault live. The
/// statvfs fields are u32 on the armv7 target and u64 on the host; the
/// casts widen on one and are no-ops on the other.
fn storage() -> String {
    let Ok(path) = std::ffi::CString::new("/home") else {
        return "STORAGE: UNKNOWN".into();
    };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 {
        return "STORAGE: UNKNOWN".into();
    }
    let free = st.f_bavail as u64 * st.f_frsize as u64;
    let total = st.f_blocks as u64 * st.f_frsize as u64;
    format!("STORAGE: {} FREE OF {}", human(free), human(total))
}

const MB: u64 = 1_000_000;
const GB: u64 = 1_000_000_000;

/// Bytes in the largest decimal unit that reaches one, to one place — the
/// figure the box and the store quote, not the binary one.
pub fn human(bytes: u64) -> String {
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// VERSION_ID from /etc/os-release, quotes stripped.
pub fn os_line(os_release: &str) -> String {
    os_release
        .lines()
        .find_map(|l| l.strip_prefix("VERSION_ID="))
        .map(|v| v.trim().trim_matches('"'))
        .filter(|v| !v.is_empty())
        .map_or_else(|| "OS: UNKNOWN".into(), |v| format!("OS: {v}"))
}

fn unix_now() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) }
}

/// RIDDLE_TZ_OFFSET in hours from UTC, the same knob the diary uses.
fn tz_offset() -> i64 {
    std::env::var("RIDDLE_TZ_OFFSET")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map_or(0, |h| h as i64)
}

/// Wall-clock HH:MM for a unix time shifted by whole hours; wraps at midnight.
pub fn clock_hhmm(unix: i64, offset_h: i64) -> String {
    let local = (unix + offset_h * 3600).rem_euclid(86_400);
    format!("{:02}:{:02}", local / 3600, (local % 3600) / 60)
}

/// The hub polls every few seconds; a reply older than this is a gap, not lag.
const HUB_FRESH: Duration = Duration::from_secs(90);

/// How recently the hub answered, as a word when it never has.
pub fn hub_line(age: Option<Duration>) -> String {
    match age {
        None => "HUB: NEVER REACHED".into(),
        Some(d) if d <= HUB_FRESH => format!("HUB: OK {}S AGO", d.as_secs()),
        Some(d) => format!("HUB: UNREACHABLE {}M", d.as_secs() / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_applies_the_offset_and_wraps() {
        assert_eq!(clock_hhmm(0, 0), "00:00");
        assert_eq!(clock_hhmm(3600 * 23 + 60 * 5, -4), "19:05");
        assert_eq!(clock_hhmm(60 * 30, -1), "23:30");
    }

    #[test]
    fn bytes_read_in_the_nearest_unit() {
        assert_eq!(human(900), "900 B");
        assert_eq!(human(1_500_000), "1.5 MB");
        assert_eq!(human(2_147_483_648), "2.1 GB");
    }

    #[test]
    fn hub_age_reads_as_a_word_or_a_time() {
        use std::time::Duration;
        assert_eq!(hub_line(None), "HUB: NEVER REACHED");
        assert_eq!(hub_line(Some(Duration::from_secs(12))), "HUB: OK 12S AGO");
        assert_eq!(hub_line(Some(Duration::from_secs(400))), "HUB: UNREACHABLE 6M");
    }

    #[test]
    fn the_battery_is_the_first_supply_of_its_kind_or_a_word() {
        assert_eq!(battery_from(&[]), "BATTERY: UNKNOWN");
        assert_eq!(battery_from(&[("Mains\n", "", "")]), "BATTERY: UNKNOWN");
        assert_eq!(
            battery_from(&[("Mains\n", "", ""), ("Battery\n", "80\n", "Discharging\n")]),
            "BATTERY: 80% DISCHARGING"
        );
        // A battery whose readings are missing still names itself.
        assert_eq!(battery_from(&[("Battery", "", "")]), "BATTERY: ?% UNKNOWN");
    }

    #[test]
    fn the_os_version_comes_from_os_release_or_is_a_word() {
        assert_eq!(os_line("NAME=x\nVERSION_ID=\"3.27.3\"\n"), "OS: 3.27.3");
        assert_eq!(os_line("NAME=x\nVERSION_ID=3.27.3\n"), "OS: 3.27.3");
        assert_eq!(os_line("NAME=x\n"), "OS: UNKNOWN");
        assert_eq!(os_line(""), "OS: UNKNOWN");
    }
}
