//! The SYSTEM page: config, Wi-Fi, device facts, power — without leaving the
//! pad. Model only; drawing is `draw`, Wi-Fi shelling is `wifi`, device
//! reads are `device`. See docs/plans/2026-09-15-system-page-design.md.

pub mod device;
pub mod draw;
pub mod wifi;

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::fb::BBox;
use crate::preferences::Mode;

/// The page opens on ORACLE.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Oracle,
    Input,
    Learn,
    Wifi,
    Device,
    Power,
}

impl Section {
    /// Header order, left to right.
    pub const ALL: [Section; 6] = [
        Section::Oracle,
        Section::Input,
        Section::Learn,
        Section::Wifi,
        Section::Device,
        Section::Power,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Oracle => "ORACLE",
            Section::Input => "INPUT",
            Section::Learn => "LEARN",
            Section::Wifi => "WI-FI",
            Section::Device => "DEVICE",
            Section::Power => "POWER",
        }
    }
}

/// Everything a tap on the page can mean. Steppers carry their direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Tab(Section),
    Close,
    SetMode(Mode),
    ToggleIdle,
    StepIdle(i8),
    StepPalm(i8),
    Preset(usize),
    StepModel(i8),
    StepAskModel(i8),
    StepReasoning(i8),
    StepMaxTokens(i8),
    ResetOverrides,
    ToggleLearn,
    StepTutorModel(i8),
    StepDwell(i8),
    WifiSelect(u32),
    WifiRescan,
    /// The next page of the Wi-Fi list, or back to its top from the last.
    WifiMore,
    Sleep,
    Leave,
    Reboot,
    PowerOff,
}

impl Act {
    pub fn is_destructive(self) -> bool {
        matches!(self, Act::Reboot | Act::PowerOff)
    }
}

/// The regions drawing painted, in paint order. The first containing region wins.
#[derive(Clone, Debug, Default)]
pub struct Hits {
    regions: Vec<(Act, BBox)>,
}

impl Hits {
    pub fn push(&mut self, act: Act, x: i32, y: i32, w: i32, h: i32) {
        let mut b = BBox::empty();
        b.add(x, y, 0);
        b.add(x + w - 1, y + h - 1, 0);
        self.regions.push((act, b));
    }

    pub fn at(&self, x: i32, y: i32) -> Option<Act> {
        self.regions
            .iter()
            .find(|(_, b)| x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1)
            .map(|(a, _)| *a)
    }

    #[cfg(test)]
    pub fn region(&self, act: Act) -> Option<BBox> {
        self.regions.iter().find(|(a, _)| *a == act).map(|(_, b)| *b)
    }

    #[cfg(test)]
    pub fn regions(&self) -> impl Iterator<Item = &(Act, BBox)> {
        self.regions.iter()
    }
}

/// Move one place through a fixed list of values; the ends clamp. A current
/// value not in the list (an env string nobody typed from this menu) snaps to
/// its nearest neighbour by value in the asked direction — the smallest value
/// above it or the largest below — and to the list's last or first value when
/// there is none. Lists need not be sorted for that to be deterministic.
pub fn step<T: Copy + PartialOrd>(values: &[T], current: T, dir: i8) -> T {
    let (Some(first), Some(last)) = (values.first(), values.last()) else {
        return current;
    };
    if let Some(i) = values.iter().position(|v| *v == current) {
        let j = if dir > 0 { (i + 1).min(values.len() - 1) } else { i.saturating_sub(1) };
        return values[j];
    }
    if dir > 0 {
        values
            .iter()
            .filter(|v| **v > current)
            .fold(None, |best: Option<T>, v| match best {
                Some(b) if b <= *v => Some(b),
                _ => Some(*v),
            })
            .unwrap_or(*last)
    } else {
        values
            .iter()
            .filter(|v| **v < current)
            .fold(None, |best: Option<T>, v| match best {
                Some(b) if b >= *v => Some(b),
                _ => Some(*v),
            })
            .unwrap_or(*first)
    }
}

/// Arm-then-confirm for destructive rows: the first tap arms, a second tap
/// on the same row within the window acts, anything else re-arms or lapses.
pub const ARM_WINDOW: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default)]
pub struct Arm {
    armed: Option<(Act, Instant)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Armed(Act),
    Confirmed(Act),
}

impl Arm {
    pub fn tap(&mut self, act: Act, now: Instant) -> Outcome {
        match self.armed {
            Some((armed, at)) if armed == act && now.saturating_duration_since(at) <= ARM_WINDOW => {
                self.armed = None;
                Outcome::Confirmed(act)
            }
            _ => {
                self.armed = Some((act, now));
                Outcome::Armed(act)
            }
        }
    }

    pub fn armed(&self) -> Option<Act> {
        self.armed.map(|(act, _)| act)
    }

    /// True when an armed row just lapsed (caller repaints it).
    pub fn disarm_if_lapsed(&mut self, now: Instant) -> bool {
        match self.armed {
            Some((_, at)) if now.saturating_duration_since(at) > ARM_WINDOW => {
                self.armed = None;
                true
            }
            _ => false,
        }
    }

    pub fn clear(&mut self) {
        self.armed = None;
    }
}

/// Stepper value lists. Properties of a hand and a budget, not of the code.
pub const PALM_MS: [u64; 6] = [0, 250, 500, 750, 1000, 1500];
pub const IDLE_MS: [u64; 4] = [1500, 2800, 4000, 6000];
pub const DWELL_MS: [u64; 4] = [0, 3000, 5000, 8000];
pub const MAX_TOKENS: [u32; 5] = [800, 1200, 2000, 4000, 8000];
pub const REASONING: [&str; 4] = ["", "low", "medium", "high"];

/// The page's live state. Drawing reads it; taps mutate it. `Page::default()`
/// opens on ORACLE with nothing armed, nothing hit, nothing to say.
pub struct Page {
    pub section: Section,
    pub hits: Hits,
    pub arm: Arm,
    pub wifi: wifi::View,
    /// The Wi-Fi worker reports here. One channel per opening: a worker
    /// still running when the page closed sends to a dropped receiver,
    /// which `wifi::spawn` ignores, so a stale report never reaches the
    /// next opening.
    pub wifi_tx: Sender<wifi::Event>,
    pub wifi_rx: Receiver<wifi::Event>,
    /// One line of outcome at the foot of the section (an error, "SAVED", …).
    pub notice: Option<String>,
}

impl Default for Page {
    fn default() -> Self {
        let (wifi_tx, wifi_rx) = mpsc::channel();
        Self {
            section: Section::default(),
            hits: Hits::default(),
            arm: Arm::default(),
            wifi: wifi::View::default(),
            wifi_tx,
            wifi_rx,
            notice: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_stepper_clamps_at_both_ends_and_recovers_an_unknown_value() {
        let v = [0u64, 250, 500, 750];
        assert_eq!(step(&v, 500, 1), 750);
        assert_eq!(step(&v, 750, 1), 750);
        assert_eq!(step(&v, 0, -1), 0);
        assert_eq!(step(&v, 333, 1), 500); // nearest above
        assert_eq!(step(&v, 333, -1), 250); // nearest below
    }

    #[test]
    fn a_string_stepper_clamps_and_snaps_an_unknown_env_value() {
        assert_eq!(step(&REASONING, "", -1), "");
        assert_eq!(step(&REASONING, "", 1), "low");
        assert_eq!(step(&REASONING, "high", 1), "high");
        assert_eq!(step(&REASONING, "medium", -1), "low");
        // Unknown strings snap by lexicographic order: "medium" is the
        // largest value below "weird"; nothing sits above it, so the last.
        assert_eq!(step(&REASONING, "weird", -1), "medium");
        assert_eq!(step(&REASONING, "weird", 1), "high");
    }

    #[test]
    fn arming_needs_a_second_tap_on_the_same_row_within_the_window() {
        let t0 = Instant::now();
        let mut arm = Arm::default();
        assert_eq!(arm.tap(Act::Reboot, t0), Outcome::Armed(Act::Reboot));
        assert_eq!(arm.tap(Act::PowerOff, t0 + Duration::from_secs(1)), Outcome::Armed(Act::PowerOff));
        assert_eq!(arm.tap(Act::PowerOff, t0 + Duration::from_secs(3)), Outcome::Confirmed(Act::PowerOff));
        assert_eq!(arm.armed(), None);
        assert_eq!(arm.tap(Act::Reboot, t0 + Duration::from_secs(20)), Outcome::Armed(Act::Reboot));
        assert_eq!(arm.tap(Act::Reboot, t0 + Duration::from_secs(26)), Outcome::Armed(Act::Reboot)); // window lapsed
        assert!(!arm.disarm_if_lapsed(t0 + Duration::from_secs(28)));
        assert_eq!(arm.armed(), Some(Act::Reboot));
        assert!(arm.disarm_if_lapsed(t0 + Duration::from_secs(40)));
        assert_eq!(arm.armed(), None);
        assert!(!arm.disarm_if_lapsed(t0 + Duration::from_secs(50)));
    }

    #[test]
    fn the_hit_map_answers_the_first_region_that_contains_the_point() {
        let mut hits = Hits::default();
        hits.push(Act::Tab(Section::Oracle), 0, 150, 200, 60);
        hits.push(Act::StepPalm(1), 1200, 400, 90, 90);
        assert_eq!(hits.at(10, 160), Some(Act::Tab(Section::Oracle)));
        assert_eq!(hits.at(1250, 450), Some(Act::StepPalm(1)));
        assert_eq!(hits.at(700, 900), None);
        assert_eq!(hits.region(Act::StepPalm(1)).map(|b| b.rect()), Some((1200, 400, 90, 90)));
        assert!(hits.region(Act::Close).is_none());
    }

    #[test]
    fn sections_cycle_in_header_order() {
        assert_eq!(Section::ALL.len(), 6);
        assert_eq!(Section::ALL[0], Section::Oracle);
        assert_eq!(Section::ALL[5], Section::Power);
        assert_eq!(Section::Wifi.label(), "WI-FI");
    }
}
