//! Drawing the SYSTEM page: header tabs, one section at a time, the hit map.
//!
//! Drawing is pure: main gathers a `View` once per frame, this module paints
//! it, and the regions it painted replace `page.hits` — so a tap can only
//! land on a row that is actually on the screen.

use ab_glyph::FontRef;

use crate::fb::{SCREEN_H, SCREEN_W};
use crate::preferences::{self, Mode, Preferences};
use crate::presets::Preset;
use crate::script;
use crate::surface::{Surface, BLACK, WHITE};
use crate::ui::{full_text, render_text, BLUE, LABEL_PX, PAD, TITLE_PX};

use super::{device, wifi, Act, Hits, Page, Section};

/// Rows start here and stack ROW_H apart until the notice line.
pub const ROWS_Y: i32 = 300;
pub const ROW_H: i32 = 110;
/// A row's value column; a value is clipped before the stepper boxes.
pub const VALUE_X: i32 = 700;
/// The minus box; the plus box sits STEP_GAP to its right.
pub const STEP_X: i32 = 1180;
pub const STEP_W: i32 = 80;
const STEP_GAP: i32 = 110;
const STEP_OUTLINE: i32 = 3;
/// One line of outcome sits here; rows never reach it.
pub const NOTICE_Y: i32 = 1700;
/// RGB565 vermilion. docs/ux-vignelli.md reserves it for destructive
/// confirmation; an armed REBOOT or POWER OFF row is its only use.
pub const VERMILION: u16 = 0xE206;

const CLOSE_W: i32 = 120;
const CLOSE_H: i32 = 105;
const TITLE_Y: usize = 110;
const TABS_Y: i32 = 180;
const TABS_H: i32 = 80;
const TAB_LABEL_Y: usize = 200;
const TAB_INSET: usize = 18;
const UNDERLINE_Y: usize = 244;
const RULE_Y: usize = 262;
/// Where LABEL_PX text sits inside a ROW_H row so it reads centred.
const TEXT_DY: i32 = 39;
const FOOTER_Y: usize = SCREEN_H - 70;
/// Liberation Sans has no U+25AE (▮); a bar of signal is a pipe.
const SIGNAL_BAR: &str = "|";

/// What the page shows. Gathered once per draw by main from the environment,
/// the preferences, the presets and the device, so drawing reads no state.
pub struct View {
    pub presets: Vec<Preset>,
    pub active_preset: Option<usize>,
    pub base: String,
    pub model: String,
    pub ask_model: String,
    pub reasoning: String,
    pub max_tokens: u32,
    pub key_set: bool,
    pub overrides_count: usize,
    pub palm_ms: u64,
    pub tutor_model: String,
    pub dwell_ms: u64,
    pub facts: device::Facts,
}

impl View {
    #[cfg(test)]
    pub fn sample() -> Self {
        Self {
            presets: crate::presets::parse(include_str!("../../settings.schema.json")),
            active_preset: Some(0),
            base: "https://vellum.mergecombinator.com/v1".into(),
            model: "vellum-capture".into(),
            ask_model: "vellum-ask".into(),
            reasoning: String::new(),
            max_tokens: 2000,
            key_set: true,
            overrides_count: 2,
            palm_ms: 500,
            tutor_model: String::new(),
            dwell_ms: 3000,
            facts: device::Facts {
                battery: "BATTERY: 82% DISCHARGING".into(),
                storage: "STORAGE: 4.1 GB FREE OF 6.5 GB".into(),
                clock: "14:05".into(),
                os: "OS: 3.27.3".into(),
                build: "2c22530".into(),
                hub: "HUB: OK 12S AGO".into(),
            },
        }
    }
}

/// Paint the whole page for `page.section` and replace `page.hits` with the
/// regions painted.
pub fn draw(surf: &mut Surface, font: &FontRef, page: &mut Page, view: &View, prefs: Preferences) {
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    let mut hits = Hits::default();
    header(surf, font, page.section, &mut hits);
    let mut rows = Rows { y: ROWS_Y, hits: &mut hits, armed: page.arm.armed() };
    match page.section {
        Section::Oracle => oracle(surf, font, &mut rows, view),
        Section::Input => input(surf, font, &mut rows, view, prefs),
        Section::Learn => learn(surf, font, &mut rows, view, prefs),
        Section::Wifi => wifi_section(surf, font, &mut rows, &page.wifi),
        Section::Device => device_section(surf, font, &mut rows, &view.facts),
        Section::Power => power(surf, font, &mut rows),
    }
    if let Some(notice) = &page.notice {
        full_text(surf, font, notice, LABEL_PX, PAD, NOTICE_Y as usize, BLACK);
    }
    full_text(surf, font, &format!("G-PAD {}", view.facts.build), LABEL_PX, PAD, FOOTER_Y, BLACK);
    page.hits = hits;
}

/// The close corner, the title, six tabs with the active one underlined in
/// blue, and the rule the rows hang from.
fn header(surf: &mut Surface, font: &FontRef, active: Section, hits: &mut Hits) {
    full_text(surf, font, "×", LABEL_PX, PAD, 36, BLACK);
    hits.push(Act::Close, 0, 0, CLOSE_W, CLOSE_H);
    full_text(surf, font, "SYSTEM", TITLE_PX, PAD, TITLE_Y, BLACK);
    let tab_w = SCREEN_W / 6;
    for (i, section) in Section::ALL.into_iter().enumerate() {
        let x = i * tab_w;
        let color = if section == active { BLUE } else { BLACK };
        full_text(surf, font, section.label(), LABEL_PX, x + TAB_INSET, TAB_LABEL_Y, color);
        if section == active {
            let w = script::rasterize_line(font, section.label(), LABEL_PX).width;
            surf.fill_rect(x + TAB_INSET, UNDERLINE_Y, w, 4, BLUE);
        }
        hits.push(Act::Tab(section), x as i32, TABS_Y, tab_w as i32, TABS_H);
    }
    surf.fill_rect(PAD, RULE_Y, SCREEN_W - 2 * PAD, 2, BLACK);
}

/// A cursor down the page. Every emitter checks `room()` first and paints
/// nothing once the next row would cross the notice line, so a long list
/// is cut rather than drawn over the notice and the footer.
struct Rows<'a> {
    y: i32,
    hits: &'a mut Hits,
    armed: Option<Act>,
}

impl Rows<'_> {
    fn room(&self) -> bool {
        self.y + ROW_H <= NOTICE_Y
    }

    fn text(&self, surf: &mut Surface, font: &FontRef, s: &str, x: i32, color: u16, limit_x: i32) {
        render_text(surf, font, s, LABEL_PX, x as usize, (self.y + TEXT_DY) as usize, color, limit_x as usize);
    }

    /// Information only: one line at the label column, no act.
    fn line(&mut self, surf: &mut Surface, font: &FontRef, s: &str) {
        if !self.room() {
            return;
        }
        self.text(surf, font, s, PAD as i32, BLACK, SCREEN_W as i32);
        self.y += ROW_H;
    }

    /// Label and value; the value is blue when `active`. With an act the
    /// whole row is a tap target, without one it is inert.
    fn row(&mut self, surf: &mut Surface, font: &FontRef, label: &str, value: &str, active: bool, act: Option<Act>) {
        if !self.room() {
            return;
        }
        self.text(surf, font, label, PAD as i32, BLACK, VALUE_X);
        self.text(surf, font, value, VALUE_X, if active { BLUE } else { BLACK }, SCREEN_W as i32);
        if let Some(act) = act {
            self.hits.push(act, 0, self.y, SCREEN_W as i32, ROW_H);
        }
        self.y += ROW_H;
    }

    /// Label, value, and a − and + box; each box is a target the full row tall.
    fn stepper(&mut self, surf: &mut Surface, font: &FontRef, label: &str, value: &str, act: fn(i8) -> Act) {
        if !self.room() {
            return;
        }
        self.text(surf, font, label, PAD as i32, BLACK, VALUE_X);
        self.text(surf, font, value, VALUE_X, BLACK, STEP_X - PAD as i32);
        for (dx, dir, glyph) in [(0, -1, "−"), (STEP_GAP, 1, "+")] {
            let x = STEP_X + dx;
            outline(surf, x, self.y + (ROW_H - STEP_W) / 2, STEP_W, STEP_W, STEP_OUTLINE, BLACK);
            let w = script::rasterize_line(font, glyph, LABEL_PX).width as i32;
            self.text(surf, font, glyph, x + (STEP_W - w) / 2, BLACK, SCREEN_W as i32);
            self.hits.push(act(dir), x, self.y, STEP_W, ROW_H);
        }
        self.y += ROW_H;
    }

    /// A destructive row: plain until its first tap arms it, then filled in
    /// vermilion with the label asking for the second tap.
    fn armed_row(&mut self, surf: &mut Surface, font: &FontRef, label: &str, act: Act) {
        if !self.room() {
            return;
        }
        if self.armed == Some(act) {
            surf.fill_rect(0, self.y as usize, SCREEN_W, ROW_H as usize, VERMILION);
            self.text(surf, font, &format!("{label} — TAP AGAIN"), PAD as i32, WHITE, SCREEN_W as i32);
        } else {
            self.text(surf, font, label, PAD as i32, BLACK, SCREEN_W as i32);
        }
        self.hits.push(act, 0, self.y, SCREEN_W as i32, ROW_H);
        self.y += ROW_H;
    }
}

fn outline(surf: &mut Surface, x: i32, y: i32, w: i32, h: i32, t: i32, color: u16) {
    let (x, y, w, h, t) = (x as usize, y as usize, w as usize, h as usize, t as usize);
    surf.fill_rect(x, y, w, t, color);
    surf.fill_rect(x, y + h - t, w, t, color);
    surf.fill_rect(x, y, t, h, color);
    surf.fill_rect(x + w - t, y, t, h, color);
}

fn dot(on: bool) -> &'static str {
    if on { "●" } else { "" }
}

fn on_off(on: bool) -> &'static str {
    if on { "ON" } else { "OFF" }
}

fn ms(v: u64) -> String {
    format!("{v} MS")
}

/// An env string shown as a word: empty means `when_empty`.
fn word_or(value: &str, when_empty: &str) -> String {
    if value.is_empty() { when_empty.to_string() } else { value.to_uppercase() }
}

fn oracle(surf: &mut Surface, font: &FontRef, rows: &mut Rows, view: &View) {
    rows.line(surf, font, "PRESET");
    for (i, preset) in view.presets.iter().enumerate() {
        let on = view.active_preset == Some(i);
        rows.row(surf, font, &preset.name.to_uppercase(), dot(on), on, Some(Act::Preset(i)));
    }
    if view.active_preset.is_none() {
        rows.row(surf, font, "CUSTOM", &view.base, true, None);
    }
    rows.stepper(surf, font, "MODEL", &view.model, Act::StepModel);
    rows.stepper(surf, font, "ASK MODEL", &view.ask_model, Act::StepAskModel);
    rows.stepper(surf, font, "REASONING", &word_or(&view.reasoning, "OFF"), Act::StepReasoning);
    rows.stepper(surf, font, "MAX TOKENS", &view.max_tokens.to_string(), Act::StepMaxTokens);
    rows.row(surf, font, "API KEY", if view.key_set { "SET" } else { "MISSING" }, view.key_set, None);
    rows.row(surf, font, "RESET OVERRIDES", &format!("{} SET", view.overrides_count), false, Some(Act::ResetOverrides));
}

fn input(surf: &mut Surface, font: &FontRef, rows: &mut Rows, view: &View, prefs: Preferences) {
    for (mode, label) in [(Mode::Stealth, "STEALTH"), (Mode::Guided, "GUIDED")] {
        let on = prefs.mode == mode;
        rows.row(surf, font, label, dot(on), on, Some(Act::SetMode(mode)));
    }
    let idle = prefs.idle_send_ms != 0;
    rows.row(surf, font, "IDLE-SEND", on_off(idle), idle, Some(Act::ToggleIdle));
    if idle {
        rows.stepper(surf, font, "IDLE DELAY", &ms(prefs.idle_send_ms), Act::StepIdle);
    }
    rows.stepper(surf, font, "PALM HOLDOFF", &ms(view.palm_ms), Act::StepPalm);
}

fn learn(surf: &mut Surface, font: &FontRef, rows: &mut Rows, view: &View, prefs: Preferences) {
    let on = prefs.page == preferences::Page::Learn;
    rows.row(surf, font, "KIDS LEARN MODE", on_off(on), on, Some(Act::ToggleLearn));
    rows.stepper(surf, font, "TUTOR MODEL", &word_or(&view.tutor_model, "DEFAULT"), Act::StepTutorModel);
    let dwell = if view.dwell_ms == 0 { "OFF".to_string() } else { ms(view.dwell_ms) };
    rows.stepper(surf, font, "NEXT-PAGE DWELL", &dwell, Act::StepDwell);
    rows.line(surf, font, "LEVEL AND SKILLS LIVE ON THE LEARN MENU");
}

fn wifi_section(surf: &mut Surface, font: &FontRef, rows: &mut Rows, w: &wifi::View) {
    rows.line(surf, font, &wifi_head(&w.status));
    if let Some(busy) = w.busy {
        rows.line(surf, font, &format!("{busy}…"));
    }
    if let Some(error) = &w.error {
        rows.line(surf, font, &error.to_uppercase());
    }
    // RESCAN sits above the list so a long list of saved networks, which is
    // cut at the notice line, can never push it off the page.
    rows.row(surf, font, "RESCAN", "", false, Some(Act::WifiRescan));
    rows.line(surf, font, "SAVED");
    for s in &w.saved {
        let value = if s.current { "●" } else if s.disabled { "DISABLED" } else { "" };
        rows.row(surf, font, &s.ssid.to_uppercase(), value, s.current, Some(Act::WifiSelect(s.id)));
    }
    if w.seen.is_empty() {
        return;
    }
    rows.line(surf, font, "IN RANGE");
    for s in &w.seen {
        rows.row(surf, font, &s.ssid.to_uppercase(), &signal(s.rssi), false, s.saved_id.map(Act::WifiSelect));
    }
    rows.line(surf, font, "NEW NETWORKS ARE ADDED OVER SSH");
}

fn wifi_head(status: &wifi::Status) -> String {
    match (&status.ssid, status.connected) {
        (Some(ssid), true) => format!(
            "{} · {} · {}",
            ssid.to_uppercase(),
            status.ip.as_deref().unwrap_or("NO IP"),
            signal(status.rssi.unwrap_or(i32::MIN))
        ),
        (Some(ssid), false) => format!("{} · CONNECTING", ssid.to_uppercase()),
        (None, _) => "DISCONNECTED".to_string(),
    }
}

fn signal(rssi: i32) -> String {
    match wifi::bars(rssi) {
        0 => "NO SIGNAL".to_string(),
        n => SIGNAL_BAR.repeat(n as usize),
    }
}

fn device_section(surf: &mut Surface, font: &FontRef, rows: &mut Rows, facts: &device::Facts) {
    let clock = format!("CLOCK: {}", facts.clock);
    let build = format!("BUILD: {}", facts.build);
    for line in [&facts.battery, &facts.storage, &clock, &facts.os, &build, &facts.hub] {
        rows.line(surf, font, line);
    }
}

fn power(surf: &mut Surface, font: &FontRef, rows: &mut Rows) {
    rows.row(surf, font, "SLEEP", "", false, Some(Act::Sleep));
    rows.row(surf, font, "LEAVE TO STOCK UI", "OR HOLD FIVE FINGERS", false, Some(Act::Leave));
    rows.armed_row(surf, font, "REBOOT", Act::Reboot);
    rows.armed_row(surf, font, "POWER OFF", Act::PowerOff);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::{SCREEN_H, SCREEN_W};
    use crate::preferences::Preferences;
    use crate::surface::{PixFmt, Surface};
    use crate::system::{wifi, Act, Page, Section};
    use crate::ui::UI_FONT_TTF;
    use ab_glyph::FontRef;
    use std::time::Instant;

    fn canvas() -> Vec<u8> {
        vec![0xff; SCREEN_W * SCREEN_H * 4]
    }

    fn surface(bytes: &mut [u8]) -> Surface {
        let len = bytes.len();
        Surface::new(bytes.as_mut_ptr(), len, SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32)
    }

    fn font() -> FontRef<'static> {
        FontRef::try_from_slice(UI_FONT_TTF).unwrap()
    }

    fn page(section: Section) -> Page {
        Page { section, ..Page::default() }
    }

    #[test]
    fn every_section_paints_its_tabs_and_close_and_stays_on_screen() {
        let mut bytes = canvas();
        let mut surf = surface(&mut bytes);
        let font = font();
        let view = View::sample();
        for s in Section::ALL {
            let mut page = page(s);
            draw(&mut surf, &font, &mut page, &view, Preferences::default());
            assert_eq!(page.hits.at(20, 50), Some(Act::Close), "{s:?} has no close corner");
            for t in Section::ALL {
                assert!(page.hits.region(Act::Tab(t)).is_some(), "{s:?} lost the {t:?} tab");
            }
            for (act, b) in page.hits.regions() {
                assert!(!b.is_empty(), "{s:?}: {act:?} painted off-screen");
                assert!(b.y1 < NOTICE_Y as i32, "{s:?}: {act:?} crosses the notice line");
                assert!(b.x1 < SCREEN_W as i32, "{s:?}: {act:?} runs off the right edge");
            }
        }
    }

    #[test]
    fn the_input_section_offers_the_palm_stepper_and_power_offers_both_destructive_rows() {
        let mut bytes = canvas();
        let mut surf = surface(&mut bytes);
        let font = font();
        let view = View::sample();

        let mut input = page(Section::Input);
        draw(&mut surf, &font, &mut input, &view, Preferences::default());
        assert!(input.hits.region(Act::StepPalm(-1)).is_some());
        assert!(input.hits.region(Act::StepPalm(1)).is_some());
        assert!(input.hits.region(Act::StepIdle(1)).is_none(), "idle-send off hides its delay");
        let prefs = Preferences { idle_send_ms: 2800, ..Preferences::default() };
        draw(&mut surf, &font, &mut input, &view, prefs);
        assert!(input.hits.region(Act::StepIdle(1)).is_some(), "idle-send on shows its delay");
        assert!(input.hits.region(Act::StepIdle(-1)).is_some());

        let mut power = page(Section::Power);
        draw(&mut surf, &font, &mut power, &view, Preferences::default());
        assert!(power.hits.region(Act::Reboot).is_some());
        assert!(power.hits.region(Act::PowerOff).is_some());
        assert!(power.hits.region(Act::Sleep).is_some());
        assert!(power.hits.region(Act::Leave).is_some());
    }

    #[test]
    fn a_long_wifi_list_stops_before_the_notice_line() {
        let mut bytes = canvas();
        let mut surf = surface(&mut bytes);
        let font = font();
        let view = View::sample();
        let mut page = page(Section::Wifi);
        page.wifi = wifi::View {
            saved: (0..30)
                .map(|i| wifi::Saved { id: i, ssid: format!("net{i}"), current: i == 3, disabled: i == 5 })
                .collect(),
            ..wifi::View::default()
        };
        draw(&mut surf, &font, &mut page, &view, Preferences::default());
        for (act, b) in page.hits.regions() {
            assert!(b.y1 < NOTICE_Y as i32, "{act:?} reaches the notice line");
        }
        // Thirty saved rows do not fit; the list is cut where the notice
        // line begins. RESCAN sits above the list, so it is always painted.
        assert!(page.hits.region(Act::WifiRescan).is_some(), "RESCAN is reachable above a long list");
        assert!(page.hits.region(Act::WifiSelect(0)).is_some());
        assert!(page.hits.region(Act::WifiSelect(29)).is_none());
    }

    #[test]
    fn an_armed_power_row_repaints_in_vermilion() {
        let mut bytes = canvas();
        let mut surf = surface(&mut bytes);
        let font = font();
        let view = View::sample();
        let mut page = page(Section::Power);
        page.arm.tap(Act::Reboot, Instant::now());
        draw(&mut surf, &font, &mut page, &view, Preferences::default());
        let b = page.hits.region(Act::Reboot).unwrap();
        let (x, y) = (b.x1 - 40, b.y0 + 6);
        assert_ne!(surf.luma(x, y), 255, "an armed row is filled");
        page.arm.clear();
        draw(&mut surf, &font, &mut page, &view, Preferences::default());
        assert_eq!(surf.luma(x, y), 255, "a disarmed row is white again");
    }
}
