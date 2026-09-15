# SYSTEM Page Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A full-page SYSTEM surface inside g-pad that changes oracle/input/learn config live, shows and drives Wi-Fi, reports device facts, and offers sleep/reboot/power-off — so the writer no longer leaves the pad for ssh or the stock UI.

**Architecture:** The process environment stays the config bus. A g-pad-owned `overrides` file is replayed into the environment at boot (after `oracle.env`) and rewritten on every in-app change; the oracle client is re-spawned from the environment, and the three boot-captured tunables are re-read. The page is a new `State::System` that replaces `State::Settings`; every draw function returns the hit map it painted (the `learn::sheet::HitMap` pattern). Wi-Fi shells to `wpa_cli` on a worker thread that reports back over an `mpsc` channel.

**Tech Stack:** Rust 2021, no new crates. Existing helpers: `ui::render_text`, `surface::Surface`, `fb::BBox`, `bridge::split_objects`/`json_field`, `preferences` file format. Cross-build: `./build-takeover-zig.sh` then `DEVICE=rm2 ./scripts/make-bundle.sh`. Design: `docs/plans/2026-09-15-system-page-design.md`.

**Conventions for every task:** run `cargo test --quiet` (host build, no features) before committing; `cargo clippy --quiet` must be clean (Dev/CLAUDE.md: linters pass before commit, no TODOs, delete replaced code). Commit messages follow the repo's voice (`feat: the pad learns to …`). Every commit ends with the attribution lines from the session's system reminder.

---

### Task 1: Build hash embedded at compile time

**Files:**
- Modify: `build.rs`
- Modify: `src/main.rs:1-60` (a `pub const BUILD: &str`)

**Step 1: Write the failing test** — append to `src/main.rs` inside `mod ux_tests` (line ~3228):

```rust
#[test]
fn the_build_hash_is_short_and_never_empty() {
    assert!(!crate::BUILD.is_empty());
    assert!(crate::BUILD.len() <= 12, "{}", crate::BUILD);
}
```

**Step 2: Run** `cargo test --quiet the_build_hash` — Expected: FAIL, `cannot find value BUILD`.

**Step 3: Implement.** In `build.rs` `main()`, before the `if` block:

```rust
// The running build's identity, shown on the SYSTEM page. `git` is absent
// on some build hosts and inside vendored snapshots, so "dev" is a value,
// not an error.
let hash = std::process::Command::new("git")
    .args(["rev-parse", "--short", "HEAD"])
    .output()
    .ok()
    .filter(|o| o.status.success())
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "dev".to_string());
println!("cargo:rustc-env=GPAD_BUILD={hash}");
println!("cargo:rerun-if-changed=.git/HEAD");
println!("cargo:rerun-if-changed=.git/refs/heads");
```

[As built: the two literal `.git/…` rerun lines were replaced by
`git rev-parse --git-path` resolution of HEAD, refs/heads and packed-refs,
watching only paths that exist. In a worktree `.git` is a file, so the
literal paths never matched.]

In `src/main.rs` after the `mod` declarations (line ~42):

```rust
/// Short git hash of this build, or "dev" when git was unavailable.
pub const BUILD: &str = env!("GPAD_BUILD");
```

**Step 4: Run** `cargo test --quiet the_build_hash` — Expected: PASS.

**Step 5: Commit** — `git add build.rs src/main.rs && git commit -m "feat: the binary knows its own git hash"`

---

### Task 2: Bridge records when the hub last answered

**Files:**
- Modify: `src/bridge.rs:229-250` (`held`, `replace`, `mark_stale`)

**Step 1: Failing test** — in `src/bridge.rs` tests module:

```rust
#[test]
fn a_fresh_poll_stamps_the_hub_as_reachable() {
    replace(Bridge { sessions: vec![], stale: false });
    assert!(last_ok_age().is_some_and(|d| d.as_secs() < 5));
    mark_stale();
    // Staleness keeps the timestamp: the age keeps growing, it is not lost.
    assert!(last_ok_age().is_some());
}
```

**Step 2: Run** `cargo test --quiet a_fresh_poll_stamps` — Expected: FAIL, `last_ok_age` not found.

**Step 3: Implement.** Next to the `HELD` static add:

```rust
static LAST_OK: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// How long ago the hub last answered a poll; `None` before it ever has.
pub fn last_ok_age() -> Option<std::time::Duration> {
    LAST_OK.lock().ok().and_then(|g| g.map(|t| t.elapsed()))
}
```

In `replace`, after storing the bridge: `if let Ok(mut g) = LAST_OK.lock() { *g = Some(std::time::Instant::now()); }`.

**Step 4: Run** `cargo test --quiet bridge` — Expected: PASS.

**Step 5: Commit** — `git commit -am "feat: the bridge remembers when the hub last answered"`

---

### Task 3: The overrides file

**Files:**
- Create: `src/overrides.rs`
- Modify: `src/main.rs:41` (add `mod overrides;`), `src/main.rs` startup (replay before `Preferences::load()` at line 682)

Format: `key=value` lines, same as `preferences`. Path `$RIDDLE_DATA_DIR/overrides` (default `/home/root/g-pad-data/overrides`). The module remembers each overridden key's pre-override value so RESET can put `oracle.env`'s value back without a restart.

**Step 1: Failing tests** — `src/overrides.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_order_skips_junk_and_takes_the_last_duplicate() {
        let got = parse("RIDDLE_PALM_MS=750\n# comment\nnot a pair\nRIDDLE_OPENAI_MODEL=a\nRIDDLE_OPENAI_MODEL=b\n =x\n");
        assert_eq!(got, vec![
            ("RIDDLE_PALM_MS".to_string(), "750".to_string()),
            ("RIDDLE_OPENAI_MODEL".to_string(), "b".to_string()),
        ]);
    }

    #[test]
    fn serialize_round_trips() {
        let pairs = vec![("A".to_string(), "1".to_string()), ("B".to_string(), "two words".to_string())];
        assert_eq!(parse(&serialize(&pairs)), pairs);
    }

    #[test]
    fn a_set_beats_the_environment_and_reset_restores_it() {
        let dir = std::env::temp_dir().join(format!("gpad-ovr-{}", std::process::id()));
        std::env::set_var("RIDDLE_DATA_DIR", &dir);
        std::env::set_var("RIDDLE_TEST_KEY", "from-env");
        let mut o = Overrides::load();
        o.set("RIDDLE_TEST_KEY", "from-page");
        assert_eq!(std::env::var("RIDDLE_TEST_KEY").unwrap(), "from-page");
        assert_eq!(parse(&std::fs::read_to_string(dir.join("overrides")).unwrap()),
            vec![("RIDDLE_TEST_KEY".to_string(), "from-page".to_string())]);
        o.reset();
        assert_eq!(std::env::var("RIDDLE_TEST_KEY").unwrap(), "from-env");
        assert!(!dir.join("overrides").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unset_removes_one_key_and_restores_its_original() {
        std::env::set_var("RIDDLE_DATA_DIR", std::env::temp_dir().join("gpad-ovr-unset"));
        std::env::remove_var("RIDDLE_UNSET_KEY");
        let mut o = Overrides::load();
        o.set("RIDDLE_UNSET_KEY", "v");
        o.unset("RIDDLE_UNSET_KEY");
        assert!(std::env::var("RIDDLE_UNSET_KEY").is_err());
        assert!(o.get("RIDDLE_UNSET_KEY").is_none());
    }
}
```

(Env-var tests share process state: keep each test on its own key names, as above.)

**Step 2: Run** `cargo test --quiet overrides` — Expected: FAIL to compile.

**Step 3: Implement** `src/overrides.rs`:

```rust
//! In-app config that beats `oracle.env` without editing it. Plain
//! `key=value` lines in `$RIDDLE_DATA_DIR/overrides`, replayed into the
//! process environment at boot and rewritten by the SYSTEM page. The
//! environment stays the config bus: everything that reads
//! `std::env::var("RIDDLE_…")` keeps working unchanged.

use std::collections::HashMap;
use std::path::PathBuf;

pub struct Overrides {
    pairs: Vec<(String, String)>,
    /// What the environment held before each key was overridden, so RESET
    /// can put `oracle.env`'s value back without a restart.
    original: HashMap<String, Option<String>>,
}

impl Overrides {
    /// Read the file and apply it to the environment.
    pub fn load() -> Self {
        let pairs = std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or_default();
        let mut o = Self { pairs: Vec::new(), original: HashMap::new() };
        for (k, v) in pairs { o.apply(&k, &v); }
        o
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Override one key: environment now, file for next boot.
    pub fn set(&mut self, key: &str, value: &str) {
        self.apply(key, value);
        self.save();
    }

    /// Drop one override and restore what the environment had.
    pub fn unset(&mut self, key: &str) {
        self.pairs.retain(|(k, _)| k != key);
        self.restore(key);
        self.save();
    }

    /// Drop every override; `oracle.env` wins again.
    pub fn reset(&mut self) {
        let keys: Vec<String> = self.pairs.drain(..).map(|(k, _)| k).collect();
        for k in keys { self.restore(&k); }
        let _ = std::fs::remove_file(path());
    }

    fn apply(&mut self, key: &str, value: &str) {
        self.original.entry(key.to_string()).or_insert_with(|| std::env::var(key).ok());
        std::env::set_var(key, value);
        self.pairs.retain(|(k, _)| k != key);
        self.pairs.push((key.to_string(), value.to_string()));
    }

    fn restore(&mut self, key: &str) {
        match self.original.remove(key).flatten() {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); }
        if let Err(e) = std::fs::write(&p, serialize(&self.pairs)) {
            eprintln!("g-pad: overrides not saved: {e}");
        }
    }
}

pub fn parse(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if k.is_empty() { continue; }
        out.retain(|(ek, _)| ek != k);
        out.push((k.to_string(), v.to_string()));
    }
    out
}

pub fn serialize(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
}

fn path() -> PathBuf {
    std::env::var("RIDDLE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/root/g-pad-data"))
        .join("overrides")
}
```

In `src/main.rs`: add `mod overrides;` (alphabetical, after `mod oracle;`), and in `run()` immediately before line 682 `let mut prefs = preferences::Preferences::load();` add:

```rust
// In-app overrides beat oracle.env from here on; everything below reads env.
let mut overrides = overrides::Overrides::load();
```

**Step 4: Run** `cargo test --quiet overrides` — Expected: 4 PASS. `cargo clippy --quiet` clean (the unused `overrides` variable warning is fine until Task 9; silence with `let _ = &overrides;` only if clippy errors).

**Step 5: Commit** — `git add src/overrides.rs src/main.rs && git commit -m "feat: an overrides file the pad writes itself, replayed over oracle.env"`

---

### Task 4: Presets read from settings.schema.json

**Files:**
- Modify: `settings.schema.json` (add Vellum preset first; add `"models": [...]` to every preset)
- Modify: `src/bridge.rs` (`split_objects` and `json_field` become `pub(crate)`)
  [As built: the helpers live in `src/brief.rs` and were already `pub(crate)`;
  `bridge.rs` was untouched by this task.]
- Create: `src/presets.rs`
- Modify: `src/main.rs:41` (`mod presets;`)

**Step 1: Edit `settings.schema.json`** — the `presets` array becomes:

```json
"presets": [
  { "name": "Vellum", "models": ["vellum-capture", "vellum-ask", "vellum-tutor"],
    "values": { "RIDDLE_OPENAI_BASE": "https://vellum.mergecombinator.com/v1", "RIDDLE_OPENAI_MODEL": "vellum-capture", "RIDDLE_OPENAI_REASONING": "" } },
  { "name": "Gemini", "models": ["gemini-3.5-flash", "gemini-3.5-pro"],
    "values": { "RIDDLE_OPENAI_BASE": "https://generativelanguage.googleapis.com/v1beta/openai", "RIDDLE_OPENAI_MODEL": "gemini-3.5-flash", "RIDDLE_OPENAI_REASONING": "low" } },
  { "name": "OpenAI", "models": ["gpt-4o-mini", "gpt-4o"],
    "values": { "RIDDLE_OPENAI_BASE": "https://api.openai.com/v1", "RIDDLE_OPENAI_MODEL": "gpt-4o-mini", "RIDDLE_OPENAI_REASONING": "" } },
  { "name": "OpenRouter", "models": ["openai/gpt-4o-mini", "anthropic/claude-sonnet-5"],
    "values": { "RIDDLE_OPENAI_BASE": "https://openrouter.ai/api/v1", "RIDDLE_OPENAI_MODEL": "openai/gpt-4o-mini", "RIDDLE_OPENAI_REASONING": "" } }
]
```

Keep the other keys of the file untouched. Confirm with `python3 -c "import json;json.load(open('settings.schema.json'))"`.

**Step 2: Failing tests** — `src/presets.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"title":"t","presets":[
      {"name":"Vellum","models":["vellum-capture","vellum-ask"],"values":{"RIDDLE_OPENAI_BASE":"https://v/v1","RIDDLE_OPENAI_MODEL":"vellum-capture","RIDDLE_OPENAI_REASONING":""}},
      {"name":"OpenAI","models":["gpt-4o-mini"],"values":{"RIDDLE_OPENAI_BASE":"https://api.openai.com/v1","RIDDLE_OPENAI_MODEL":"gpt-4o-mini","RIDDLE_OPENAI_REASONING":"low"}}
    ]}"#;

    #[test]
    fn presets_parse_names_models_and_values() {
        let p = parse(SAMPLE);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].name, "Vellum");
        assert_eq!(p[0].models, vec!["vellum-capture", "vellum-ask"]);
        assert_eq!(p[0].base, "https://v/v1");
        assert_eq!(p[1].reasoning, "low");
        assert_eq!(p[0].reasoning, "");
    }

    #[test]
    fn the_active_preset_is_the_one_whose_base_matches() {
        let p = parse(SAMPLE);
        assert_eq!(active(&p, Some("https://v/v1/")), Some(0));
        assert_eq!(active(&p, Some("https://elsewhere")), None);
        assert_eq!(active(&p, None), None);
    }

    #[test]
    fn the_shipped_schema_parses_and_leads_with_vellum() {
        let p = parse(include_str!("../settings.schema.json"));
        assert!(p.len() >= 4);
        assert_eq!(p[0].name, "Vellum");
        assert!(p.iter().all(|x| !x.models.is_empty()));
    }
}
```

**Step 3: Run** `cargo test --quiet presets` — Expected: FAIL to compile.

**Step 4: Implement.** In `src/bridge.rs` change `fn split_objects` and `fn json_field` to `pub(crate) fn` (find them with `grep -n "fn split_objects\|fn json_field" src/bridge.rs`). Then `src/presets.rs`:

```rust
//! Oracle presets from settings.schema.json — the file the bundle already
//! ships for the off-device editor. The SYSTEM page reads the same file, so
//! a preset added over ssh needs no rebuild. Loaded from
//! `$RIDDLE_PRESETS_FILE`, else `settings.schema.json` beside the binary,
//! else the copy compiled in.

use crate::bridge::{json_field, split_objects};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    pub models: Vec<String>,
    pub base: String,
    pub model: String,
    pub reasoning: String,
}

pub fn load() -> Vec<Preset> {
    let from_env = std::env::var("RIDDLE_PRESETS_FILE").ok().map(std::path::PathBuf::from);
    let beside = std::env::current_exe().ok()
        .and_then(|p| p.parent().map(|d| d.join("settings.schema.json")));
    for path in [from_env, beside].into_iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let p = parse(&text);
            if !p.is_empty() { return p; }
        }
    }
    parse(include_str!("../settings.schema.json"))
}

pub fn parse(json: &str) -> Vec<Preset> {
    split_objects(json, "presets").into_iter().filter_map(|block| {
        let name = json_field(&block, "name")?;
        Some(Preset {
            name,
            models: json_string_array(&block, "models"),
            base: json_field(&block, "RIDDLE_OPENAI_BASE").unwrap_or_default(),
            model: json_field(&block, "RIDDLE_OPENAI_MODEL").unwrap_or_default(),
            reasoning: json_field(&block, "RIDDLE_OPENAI_REASONING").unwrap_or_default(),
        })
    }).collect()
}

/// Which preset the environment's base URL belongs to, trailing slash ignored.
pub fn active(presets: &[Preset], base: Option<&str>) -> Option<usize> {
    let base = base?.trim_end_matches('/');
    presets.iter().position(|p| p.base.trim_end_matches('/') == base)
}

/// `"key": ["a", "b"]` → the strings. Enough for a list of model names.
fn json_string_array(block: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let Some(i) = block.find(&needle) else { return Vec::new() };
    let rest = &block[i + needle.len()..];
    let Some(open) = rest.find('[') else { return Vec::new() };
    let Some(close) = rest[open..].find(']') else { return Vec::new() };
    rest[open + 1..open + close]
        .split(',')
        .filter_map(|s| s.trim().strip_prefix('"')?.strip_suffix('"').map(str::to_string))
        .collect()
}
```

If `split_objects`'s signature differs from `(json: &str, key: &str) -> Vec<String>`, adapt the call, not the helper. Add `mod presets;` to `src/main.rs`.

**Step 5: Run** `cargo test --quiet presets` — Expected: 3 PASS.

**Step 6: Commit** — `git add settings.schema.json src/presets.rs src/bridge.rs src/main.rs && git commit -m "feat: presets come from the schema the bundle already ships, Vellum first"`

---

### Task 5: SYSTEM page model — sections, actions, steppers, arm-then-confirm, hit map

[As built: the page is constructed with `Page::default()`, not `Page::new()`;
the same applies where Tasks 8 and 9 write `Page::new()`.]

**Files:**
- Create: `src/system/mod.rs`
- Modify: `src/main.rs:41` (`mod system;`)

**Step 1: Failing tests** — `src/system/mod.rs` tests:

```rust
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
        assert_eq!(step(&v, 333, 1), 500);   // nearest above
        assert_eq!(step(&v, 333, -1), 250);  // nearest below
    }

    #[test]
    fn arming_needs_a_second_tap_on_the_same_row_within_the_window() {
        let t0 = Instant::now();
        let mut arm = Arm::default();
        assert_eq!(arm.tap(Act::Reboot, t0), Outcome::Armed(Act::Reboot));
        assert_eq!(arm.tap(Act::PowerOff, t0 + Duration::from_secs(1)), Outcome::Armed(Act::PowerOff));
        assert_eq!(arm.tap(Act::PowerOff, t0 + Duration::from_secs(3)), Outcome::Confirmed(Act::PowerOff));
        assert_eq!(arm.tap(Act::Reboot, t0 + Duration::from_secs(20)), Outcome::Armed(Act::Reboot));
        assert_eq!(arm.tap(Act::Reboot, t0 + Duration::from_secs(26)), Outcome::Armed(Act::Reboot)); // window lapsed
        assert!(arm.disarm_if_lapsed(t0 + Duration::from_secs(40)));
        assert_eq!(arm.armed(), None);
    }

    #[test]
    fn the_hit_map_answers_the_first_region_that_contains_the_point() {
        let mut hits = Hits::default();
        hits.push(Act::Tab(Section::Oracle), 0, 150, 200, 60);
        hits.push(Act::StepPalm(1), 1200, 400, 90, 90);
        assert_eq!(hits.at(10, 160), Some(Act::Tab(Section::Oracle)));
        assert_eq!(hits.at(1250, 450), Some(Act::StepPalm(1)));
        assert_eq!(hits.at(700, 900), None);
    }

    #[test]
    fn sections_cycle_in_header_order() {
        assert_eq!(Section::ALL.len(), 6);
        assert_eq!(Section::ALL[0], Section::Oracle);
        assert_eq!(Section::ALL[5], Section::Power);
        assert_eq!(Section::Wifi.label(), "WI-FI");
    }
}
```

**Step 2: Run** `cargo test --quiet system::` — Expected: FAIL to compile.

**Step 3: Implement** `src/system/mod.rs`:

```rust
//! The SYSTEM page: config, Wi-Fi, device facts, power — without leaving the
//! pad. Model only; drawing is `draw`, Wi-Fi shelling is `wifi`, device
//! reads are `device`. See docs/plans/2026-09-15-system-page-design.md.

pub mod device;
pub mod draw;
pub mod wifi;

use std::time::{Duration, Instant};

use crate::fb::BBox;
use crate::preferences::Mode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section { Oracle, Input, Learn, Wifi, Device, Power }

impl Section {
    pub const ALL: [Section; 6] = [Section::Oracle, Section::Input, Section::Learn, Section::Wifi, Section::Device, Section::Power];
    pub fn label(self) -> &'static str {
        match self {
            Section::Oracle => "ORACLE", Section::Input => "INPUT", Section::Learn => "LEARN",
            Section::Wifi => "WI-FI", Section::Device => "DEVICE", Section::Power => "POWER",
        }
    }
}

/// Everything a tap on the page can mean. Steppers carry their direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Tab(Section), Close,
    SetMode(Mode), ToggleIdle, StepIdle(i8), StepPalm(i8),
    Preset(usize), StepModel(i8), StepAskModel(i8), StepReasoning(i8), StepMaxTokens(i8), ResetOverrides,
    ToggleLearn, StepTutorModel(i8), StepDwell(i8),
    WifiSelect(usize), WifiRescan,
    Sleep, Leave, Reboot, PowerOff,
}

impl Act {
    pub fn is_destructive(self) -> bool { matches!(self, Act::Reboot | Act::PowerOff) }
}

/// The regions drawing painted, in paint order. The first containing region wins.
#[derive(Clone, Debug, Default)]
pub struct Hits { regions: Vec<(Act, BBox)> }

impl Hits {
    pub fn push(&mut self, act: Act, x: i32, y: i32, w: i32, h: i32) {
        let mut b = BBox::empty(); b.add(x, y, 0); b.add(x + w - 1, y + h - 1, 0);
        self.regions.push((act, b));
    }
    pub fn at(&self, x: i32, y: i32) -> Option<Act> {
        self.regions.iter()
            .find(|(_, b)| x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1)
            .map(|(a, _)| *a)
    }
    pub fn region(&self, act: Act) -> Option<BBox> {
        self.regions.iter().find(|(a, _)| *a == act).map(|(_, b)| *b)
    }
}

/// Move through a fixed list of values. An unknown current value snaps to
/// the nearest neighbour in the asked direction; the ends clamp.
pub fn step<T: Copy + PartialOrd>(values: &[T], current: T, dir: i8) -> T {
    if values.is_empty() { return current; }
    if let Some(i) = values.iter().position(|v| *v == current) {
        let j = (i as i64 + dir as i64).clamp(0, values.len() as i64 - 1) as usize;
        return values[j];
    }
    if dir > 0 { values.iter().copied().find(|v| *v > current).unwrap_or(values[values.len() - 1]) }
    else { values.iter().rev().copied().find(|v| *v < current).unwrap_or(values[0]) }
}

/// Arm-then-confirm for destructive rows: the first tap arms, a second tap
/// on the same row within the window acts, anything else re-arms or lapses.
pub const ARM_WINDOW: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default)]
pub struct Arm { armed: Option<(Act, Instant)> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome { Armed(Act), Confirmed(Act) }

impl Arm {
    pub fn tap(&mut self, act: Act, now: Instant) -> Outcome {
        match self.armed {
            Some((a, t)) if a == act && now.duration_since(t) <= ARM_WINDOW => {
                self.armed = None; Outcome::Confirmed(act)
            }
            _ => { self.armed = Some((act, now)); Outcome::Armed(act) }
        }
    }
    pub fn armed(&self) -> Option<Act> { self.armed.map(|(a, _)| a) }
    /// True when an armed row just lapsed (caller repaints it).
    pub fn disarm_if_lapsed(&mut self, now: Instant) -> bool {
        match self.armed {
            Some((_, t)) if now.duration_since(t) > ARM_WINDOW => { self.armed = None; true }
            _ => false,
        }
    }
    pub fn clear(&mut self) { self.armed = None; }
}

/// Stepper value lists. Properties of a hand and a budget, not of the code.
pub const PALM_MS: [u64; 6] = [0, 250, 500, 750, 1000, 1500];
pub const IDLE_MS: [u64; 4] = [1500, 2800, 4000, 6000];
pub const DWELL_MS: [u64; 4] = [0, 3000, 5000, 8000];
pub const MAX_TOKENS: [u32; 5] = [800, 1200, 2000, 4000, 8000];
pub const REASONING: [&str; 4] = ["", "low", "medium", "high"];

/// The page's live state. Drawing reads it; taps mutate it.
pub struct Page {
    pub section: Section,
    pub hits: Hits,
    pub arm: Arm,
    pub wifi: wifi::View,
    /// One line of outcome at the foot of the section (an error, "SAVED", …).
    pub notice: Option<String>,
}

impl Page {
    pub fn new() -> Self {
        Self { section: Section::Oracle, hits: Hits::default(), arm: Arm::default(), wifi: wifi::View::default(), notice: None }
    }
}
```

Create empty stubs so it compiles: `src/system/device.rs` (empty file with a `//!` line), `src/system/wifi.rs` containing `#[derive(Clone, Debug, Default)] pub struct View;`, `src/system/draw.rs` (a `//!` line). Add `mod system;` to `src/main.rs`.

**Step 4: Run** `cargo test --quiet system::` — Expected: 4 PASS.

**Step 5: Commit** — `git add src/system src/main.rs && git commit -m "feat: the SYSTEM page's model — sections, acts, steppers, arm-then-confirm, a returned hit map"`

---

### Task 6: Wi-Fi — wpa_cli parsers and the worker

[As built: `View::begin` (the page claims the worker once until it reports)
lives in this file, and Step 4 expects 8 tests, not 4. Task 9's text below
was amended in place during this task to match.]

**Files:**
- Modify: `src/system/wifi.rs`

**Step 1: Failing tests** (fixtures are real wpa_cli shapes; the rM2 prints the same):

```rust
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
    fn rssi_reads_as_bars() {
        assert_eq!(bars(-40), 4); assert_eq!(bars(-60), 3); assert_eq!(bars(-70), 2); assert_eq!(bars(-80), 1); assert_eq!(bars(-95), 0);
    }
}
```

**Step 2: Run** `cargo test --quiet wifi` — Expected: FAIL to compile.

**Step 3: Implement** `src/system/wifi.rs`:

```rust
//! Wi-Fi over `wpa_cli -i wlan0`, the tool `power::wifi_heal` already
//! trusts after resume. Every call runs on a worker thread and reports over
//! a channel; the pad loop never waits on the radio.

use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status { pub ssid: Option<String>, pub connected: bool, pub ip: Option<String>, pub rssi: Option<i32> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved { pub id: u32, pub ssid: String, pub current: bool, pub disabled: bool }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen { pub ssid: String, pub rssi: i32, pub saved_id: Option<u32> }

/// What the page shows. `busy` names the command in flight, if any.
#[derive(Clone, Debug, Default)]
pub struct View { pub status: Status, pub saved: Vec<Saved>, pub seen: Vec<Seen>, pub busy: Option<&'static str>, pub error: Option<String> }

pub enum Cmd { Refresh, Scan, Select(u32) }

pub enum Event { Status(Status), Saved(Vec<Saved>), Seen(Vec<Seen>), Failed(String) }

pub fn spawn(cmd: Cmd, tx: Sender<Event>) {
    std::thread::spawn(move || {
        let send = |e: Event| { let _ = tx.send(e); };
        match cmd {
            Cmd::Refresh => refresh(&send),
            Cmd::Scan => {
                if let Err(e) = wpa(&["scan"]) { return send(Event::Failed(e)); }
                std::thread::sleep(std::time::Duration::from_secs(4));
                match (wpa(&["scan_results"]), wpa(&["list_networks"])) {
                    (Ok(res), Ok(list)) => send(Event::Seen(parse_scan_results(&res, &parse_list_networks(&list)))),
                    (Err(e), _) | (_, Err(e)) => send(Event::Failed(e)),
                }
            }
            Cmd::Select(id) => {
                // select_network alone starts the join, but it disables every
                // other network. Re-enabling them at once would let the
                // supplicant roam straight back to a stronger one, so wait
                // for the join first, then restore roaming, best-effort.
                if let Err(e) = wpa(&["select_network", &id.to_string()]) { return send(Event::Failed(e)); }
                wait_for_join();
                let _ = wpa(&["enable_network", "all"]);
                refresh(&send);
            }
        }
    });
}

/// How long a join may take before roaming is restored regardless.
const JOIN_WAIT: Duration = Duration::from_secs(10);
/// How often the join is checked for COMPLETED.
const JOIN_POLL: Duration = Duration::from_secs(1);

/// Poll `status` until `wpa_state=COMPLETED` or `JOIN_WAIT` has passed.
fn wait_for_join() {
    let deadline = Instant::now() + JOIN_WAIT;
    loop {
        std::thread::sleep(JOIN_POLL);
        let joined = wpa(&["status"]).map(|s| parse_status(&s, None).connected).unwrap_or(false);
        if joined || Instant::now() >= deadline { return; }
    }
}

fn refresh(send: &dyn Fn(Event)) {
    match wpa(&["status"]) {
        Ok(status) => send(Event::Status(parse_status(&status, wpa(&["signal_poll"]).ok().as_deref()))),
        Err(e) => return send(Event::Failed(e)),
    }
    match wpa(&["list_networks"]) {
        Ok(list) => send(Event::Saved(parse_list_networks(&list))),
        Err(e) => send(Event::Failed(e)),
    }
}

/// Run one wpa_cli command; the error is its first line, for the status row.
fn wpa(args: &[&str]) -> Result<String, String> {
    let out = Command::new("wpa_cli").args(["-i", "wlan0"]).args(args).output()
        .map_err(|e| format!("wpa_cli: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() || text.trim_start().starts_with("FAIL") {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().chain(text.lines()).next().unwrap_or("wpa_cli failed").to_string());
    }
    Ok(text)
}

pub fn parse_status(status: &str, signal: Option<&str>) -> Status {
    let field = |k: &str| status.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')).map(|v| v.trim().to_string()));
    let connected = field("wpa_state").as_deref() == Some("COMPLETED");
    let rssi = signal.and_then(|s| s.lines().find_map(|l| l.strip_prefix("RSSI=")?.trim().parse().ok()));
    Status { ssid: field("ssid").filter(|s| !s.is_empty()), connected, ip: field("ip_address"), rssi }
}

pub fn parse_list_networks(text: &str) -> Vec<Saved> {
    text.lines().skip(1).filter_map(|l| {
        let mut f = l.split('\t');
        let id = f.next()?.trim().parse().ok()?;
        let ssid = f.next()?.trim().to_string();
        let _bssid = f.next();
        let flags = f.next().unwrap_or("");
        Some(Saved { id, ssid, current: flags.contains("[CURRENT]"), disabled: flags.contains("[DISABLED]") })
    }).collect()
}

pub fn parse_scan_results(text: &str, saved: &[Saved]) -> Vec<Seen> {
    let mut seen: Vec<Seen> = text.lines().skip(1).filter_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 5 { return None; }
        let ssid = f[4].trim();
        if ssid.is_empty() { return None; }
        let rssi = f[2].trim().parse().ok()?;
        Some(Seen { ssid: ssid.to_string(), rssi, saved_id: saved.iter().find(|s| s.ssid == ssid).map(|s| s.id) })
    }).collect();
    seen.sort_by_key(|s| -s.rssi);
    seen.dedup_by(|a, b| a.ssid == b.ssid);
    seen
}

/// Signal as 0–4 bars, the way a status line reads it.
pub fn bars(rssi: i32) -> u8 {
    match rssi { r if r >= -50 => 4, r if r >= -65 => 3, r if r >= -75 => 2, r if r >= -85 => 1, _ => 0 }
}
```

**Step 4: Run** `cargo test --quiet wifi` — Expected: 8 PASS.

**Step 5: Commit** — `git add src/system/wifi.rs && git commit -m "feat: the pad reads and drives wpa_cli off the draw loop"`

---

### Task 7: Device facts

**Files:**
- Modify: `src/system/device.rs`

**Step 1: Failing tests**:

```rust
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
}
```

**Step 2: Run** `cargo test --quiet device` — Expected: FAIL to compile.

**Step 3: Implement** `src/system/device.rs`:

```rust
//! Read-only device facts for the DEVICE section. Every reader degrades to
//! a word rather than an error: the page must never go blank.

use std::time::Duration;

pub struct Facts { pub battery: String, pub storage: String, pub clock: String, pub os: String, pub build: String, pub hub: String }

pub fn gather() -> Facts {
    Facts {
        battery: battery(),
        storage: storage(),
        clock: clock_hhmm(unix_now(), tz_offset()),
        os: os_version(),
        build: crate::BUILD.to_string(),
        hub: hub_line(crate::bridge::last_ok_age()),
    }
}

fn battery() -> String {
    let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else { return "BATTERY: UNKNOWN".into() };
    for e in dir.flatten() {
        let p = e.path();
        let kind = std::fs::read_to_string(p.join("type")).unwrap_or_default();
        if kind.trim() != "Battery" { continue; }
        let cap = std::fs::read_to_string(p.join("capacity")).unwrap_or_default();
        let status = std::fs::read_to_string(p.join("status")).unwrap_or_default();
        return format!("BATTERY: {}% {}", cap.trim(), status.trim().to_uppercase());
    }
    "BATTERY: UNKNOWN".into()
}

fn storage() -> String {
    let Ok(path) = std::ffi::CString::new("/home") else { return "STORAGE: UNKNOWN".into() };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 { return "STORAGE: UNKNOWN".into() }
    let free = st.f_bavail as u64 * st.f_frsize as u64;
    let total = st.f_blocks as u64 * st.f_frsize as u64;
    format!("STORAGE: {} FREE OF {}", human(free), human(total))
}

pub fn human(bytes: u64) -> String {
    const GB: f64 = 1_073_741_824.0; const MB: f64 = 1_048_576.0;
    // [As built: units are decimal, GB = 1_000_000_000 and MB = 1_000_000.
    // The tests above were right; these constants were wrong.]
    let b = bytes as f64;
    if b >= GB { format!("{:.1} GB", b / GB) } else if b >= MB { format!("{:.1} MB", b / MB) } else { format!("{bytes} B") }
}

fn os_version() -> String {
    let text = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let v = text.lines().find_map(|l| l.strip_prefix("VERSION_ID=")).map(|v| v.trim_matches('"').to_string());
    format!("OS: {}", v.unwrap_or_else(|| "UNKNOWN".into()))
}

fn unix_now() -> i64 { unsafe { libc::time(std::ptr::null_mut()) } }

fn tz_offset() -> i64 { std::env::var("RIDDLE_TZ_OFFSET").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0) }

pub fn clock_hhmm(unix: i64, offset_h: i64) -> String {
    let local = (unix + offset_h * 3600).rem_euclid(86_400);
    format!("{:02}:{:02}", local / 3600, (local % 3600) / 60)
}

pub fn hub_line(age: Option<Duration>) -> String {
    match age {
        None => "HUB: NEVER REACHED".into(),
        Some(d) if d.as_secs() <= 90 => format!("HUB: OK {}S AGO", d.as_secs()),
        Some(d) => format!("HUB: UNREACHABLE {}M", d.as_secs() / 60),
    }
}
```

**Step 4: Run** `cargo test --quiet device` — Expected: 3 PASS. `cargo clippy --quiet` clean.

**Step 5: Commit** — `git add src/system/device.rs && git commit -m "feat: battery, storage, clock, OS, build and hub reachability, read without leaving the pad"`

---

### Task 8: Drawing the page

[As built: `Rows::room()` bounds every row to `NOTICE_Y`, so a long list is
cut at the notice line; RESCAN precedes the saved list for that reason; the
ORACLE section has a CUSTOM row showing the base when no preset matches; the
page is `Page::default()`.]

**Files:**
- Modify: `src/ui.rs:14-19` (`PAD`, `LABEL_PX`, `TITLE_PX`, `BLUE` become `pub(crate)`; `full_text` and `render_text` become `pub(crate) fn`)
- Modify: `src/system/draw.rs`

Layout (1404×1872): `×` at (PAD, 36); title "SYSTEM" at y 110 (TITLE_PX); section labels in one row at y 200, each `SCREEN_W/6` wide, active one BLUE with a 4px underline; rows from y 300, `ROW_H = 110`; label at x PAD, value at x 700, stepper boxes 80×80 at x 1180 (−) and 1290 (+); a `SAVED`/error notice at y 1700; the running build in the footer corner. Vermilion is RGB565 `0xE206`; on e-ink it renders dark, and the inverted row is the visible cue.

**Step 1: Failing test** — `src/system/draw.rs` tests (a Surface is built the way `ui::tests` does: a `vec![0xff; SCREEN_W*SCREEN_H*4]` with `Surface::new(ptr, len, SCREEN_W, SCREEN_H, SCREEN_W*4, PixFmt::Rgba)` — copy the exact constructor from `src/ui.rs` tests):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{Act, Page, Section};
    use crate::preferences::Preferences;

    fn page_with(section: Section) -> Page { let mut p = Page::new(); p.section = section; p }

    #[test]
    fn every_section_paints_its_tabs_and_close_and_stays_on_screen() {
        let font = ab_glyph::FontRef::try_from_slice(crate::ui::UI_FONT_TTF).unwrap();
        let mut bytes = vec![0xffu8; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, crate::surface::PixFmt::Rgba);
        for s in Section::ALL {
            let mut page = page_with(s);
            let view = View::sample();
            draw(&mut surf, &font, &mut page, &view, Preferences::default());
            assert_eq!(page.hits.at(20, 50), Some(Act::Close), "{s:?}");
            for t in Section::ALL { assert!(page.hits.region(Act::Tab(t)).is_some(), "{s:?} lacks tab {t:?}"); }
            // No region may spill past the panel.
            for (_, b) in page.hits.regions() { assert!(b.x1 < SCREEN_W as i32 && b.y1 < SCREEN_H as i32); }
        }
    }

    #[test]
    fn the_input_section_offers_the_palm_stepper_and_power_offers_both_destructive_rows() {
        let font = ab_glyph::FontRef::try_from_slice(crate::ui::UI_FONT_TTF).unwrap();
        let mut bytes = vec![0xffu8; SCREEN_W * SCREEN_H * 4];
        let ptr = bytes.as_mut_ptr();
        let mut surf = Surface::new(ptr, bytes.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, crate::surface::PixFmt::Rgba);
        let mut page = page_with(Section::Input);
        draw(&mut surf, &font, &mut page, &View::sample(), Preferences::default());
        assert!(page.hits.region(Act::StepPalm(-1)).is_some() && page.hits.region(Act::StepPalm(1)).is_some());
        let mut page = page_with(Section::Power);
        draw(&mut surf, &font, &mut page, &View::sample(), Preferences::default());
        assert!(page.hits.region(Act::Reboot).is_some() && page.hits.region(Act::PowerOff).is_some());
    }
}
```

Add `pub fn regions(&self) -> impl Iterator<Item = &(Act, BBox)>` to `Hits` in `mod.rs`.

**Step 2: Run** `cargo test --quiet system::draw` — Expected: FAIL to compile.

**Step 3: Implement** `src/system/draw.rs`. `View` is the snapshot drawing reads (gathered by main from env, presets, device, prefs) so drawing stays pure:

```rust
//! Paint the SYSTEM page and return what was painted. Drawing owns the hit
//! map: a tap target exists only where a row was actually drawn.

use ab_glyph::FontRef;

use crate::fb::{SCREEN_H, SCREEN_W};
use crate::preferences::{Mode, Page as PrefPage, Preferences};
use crate::presets::Preset;
use crate::surface::{Surface, BLACK, WHITE};
use crate::system::{device, wifi, Act, Hits, Page, Section};
use crate::ui::{full_text, LABEL_PX, PAD, TITLE_PX, BLUE};

pub const VERMILION: u16 = 0xE206;
const ROW_H: i32 = 110;
const ROWS_Y: i32 = 300;
const VALUE_X: usize = 700;
const STEP_X: i32 = 1180;
const STEP_W: i32 = 80;
const NOTICE_Y: usize = 1700;

/// What the page shows, read once per draw by main.
pub struct View {
    pub presets: Vec<Preset>,
    pub active_preset: Option<usize>,
    pub base: String, pub model: String, pub ask_model: String, pub reasoning: String,
    pub max_tokens: u32, pub key_set: bool, pub overrides_count: usize,
    pub palm_ms: u64, pub tutor_model: String, pub dwell_ms: u64,
    pub facts: device::Facts,
}

impl View {
    #[cfg(test)]
    pub fn sample() -> Self {
        View { presets: crate::presets::parse(include_str!("../../settings.schema.json")), active_preset: Some(0),
            base: "https://v/v1".into(), model: "vellum-capture".into(), ask_model: "vellum-ask".into(), reasoning: String::new(),
            max_tokens: 1200, key_set: true, overrides_count: 0, palm_ms: 500, tutor_model: "vellum-tutor".into(), dwell_ms: 5000,
            facts: device::Facts { battery: "BATTERY: 80% DISCHARGING".into(), storage: "STORAGE: 3.1 GB FREE OF 6.7 GB".into(),
                clock: "12:00".into(), os: "OS: 3.27.3".into(), build: "abc1234".into(), hub: "HUB: OK 5S AGO".into() } }
    }
}

/// Paint the whole page, replacing `page.hits`.
pub fn draw(surf: &mut Surface, font: &FontRef, page: &mut Page, view: &View, prefs: Preferences) {
    let mut hits = Hits::default();
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    full_text(surf, font, "×", LABEL_PX, PAD, 36, BLACK);
    hits.push(Act::Close, 0, 0, 120, 105);
    full_text(surf, font, "SYSTEM", TITLE_PX, PAD, 110, BLACK);
    let tab_w = SCREEN_W as i32 / Section::ALL.len() as i32;
    for (i, s) in Section::ALL.iter().enumerate() {
        let x = i as i32 * tab_w;
        let active = *s == page.section;
        full_text(surf, font, s.label(), LABEL_PX, x as usize + 24, 200, if active { BLUE } else { BLACK });
        if active { surf.fill_rect(x as usize + 24, 244, tab_w as usize - 48, 4, BLUE); }
        hits.push(Act::Tab(*s), x, 180, tab_w, 80);
    }
    surf.fill_rect(PAD, 262, SCREEN_W - 2 * PAD, 2, BLACK);
    let mut r = Rows { y: ROWS_Y, hits: &mut hits, page_arm: page.arm.armed() };
    match page.section {
        Section::Oracle => oracle(surf, font, &mut r, view),
        Section::Input => input(surf, font, &mut r, view, prefs),
        Section::Learn => learn(surf, font, &mut r, view, prefs),
        Section::Wifi => wifi_rows(surf, font, &mut r, &page.wifi),
        Section::Device => device_rows(surf, font, &mut r, &view.facts),
        Section::Power => power(surf, font, &mut r),
    }
    if let Some(n) = &page.notice { full_text(surf, font, n, LABEL_PX, PAD, NOTICE_Y, BLACK); }
    full_text(surf, font, &format!("G-PAD {}", view.facts.build), LABEL_PX, PAD, SCREEN_H - 70, BLACK);
    page.hits = hits;
}

struct Rows<'a> { y: i32, hits: &'a mut Hits, page_arm: Option<Act> }

impl Rows<'_> {
    /// A full-width tappable row: label left, value at VALUE_X, one act.
    fn row(&mut self, surf: &mut Surface, font: &FontRef, label: &str, value: &str, act: Option<Act>, color: u16) {
        full_text(surf, font, label, LABEL_PX, PAD, self.y as usize + 34, color);
        full_text(surf, font, value, LABEL_PX, VALUE_X, self.y as usize + 34, if act.is_some() { BLUE } else { color });
        if let Some(a) = act { self.hits.push(a, 0, self.y, SCREEN_W as i32, ROW_H); }
        self.y += ROW_H;
    }
    /// Label, value, and −/+ boxes; `mk` builds the act for a direction.
    fn stepper(&mut self, surf: &mut Surface, font: &FontRef, label: &str, value: &str, mk: fn(i8) -> Act) {
        full_text(surf, font, label, LABEL_PX, PAD, self.y as usize + 34, BLACK);
        full_text(surf, font, value, LABEL_PX, VALUE_X, self.y as usize + 34, BLACK);
        for (dx, sym, dir) in [(0, "−", -1i8), (STEP_W + 30, "+", 1)] {
            let x = STEP_X + dx;
            outline(surf, x, self.y + 15, STEP_W, STEP_W, 3, BLACK);
            full_text(surf, font, sym, LABEL_PX, x as usize + 28, self.y as usize + 34, BLACK);
            self.hits.push(mk(dir), x, self.y, STEP_W, ROW_H);
        }
        self.y += ROW_H;
    }
    /// A destructive row: inverted in vermilion while armed.
    fn armed_row(&mut self, surf: &mut Surface, font: &FontRef, label: &str, act: Act) {
        let armed = self.page_arm == Some(act);
        if armed {
            surf.fill_rect(0, self.y as usize, SCREEN_W, ROW_H as usize, VERMILION);
            full_text(surf, font, &format!("{label} — TAP AGAIN"), LABEL_PX, PAD, self.y as usize + 34, WHITE);
        } else {
            full_text(surf, font, label, LABEL_PX, PAD, self.y as usize + 34, BLACK);
        }
        self.hits.push(act, 0, self.y, SCREEN_W as i32, ROW_H);
        self.y += ROW_H;
    }
    fn line(&mut self, surf: &mut Surface, font: &FontRef, s: &str) {
        full_text(surf, font, s, LABEL_PX, PAD, self.y as usize + 34, BLACK);
        self.y += ROW_H * 3 / 4;
    }
}

fn outline(surf: &mut Surface, x: i32, y: i32, w: i32, h: i32, t: i32, c: u16) {
    let (x, y, w, h, t) = (x as usize, y as usize, w as usize, h as usize, t as usize);
    surf.fill_rect(x, y, w, t, c); surf.fill_rect(x, y + h - t, w, t, c);
    surf.fill_rect(x, y, t, h, c); surf.fill_rect(x + w - t, y, t, h, c);
}

fn oracle(surf: &mut Surface, font: &FontRef, r: &mut Rows, v: &View) {
    r.line(surf, font, "PRESET");
    for (i, p) in v.presets.iter().enumerate() {
        let on = v.active_preset == Some(i);
        r.row(surf, font, &p.name.to_uppercase(), if on { "●" } else { "" }, Some(Act::Preset(i)), if on { BLUE } else { BLACK });
    }
    r.stepper(surf, font, "MODEL", &v.model, Act::StepModel);
    r.stepper(surf, font, "ASK MODEL", &v.ask_model, Act::StepAskModel);
    r.stepper(surf, font, "REASONING", if v.reasoning.is_empty() { "OFF" } else { &v.reasoning }, Act::StepReasoning);
    r.stepper(surf, font, "MAX TOKENS", &v.max_tokens.to_string(), Act::StepMaxTokens);
    r.row(surf, font, "API KEY", if v.key_set { "SET" } else { "MISSING" }, None, BLACK);
    r.row(surf, font, "RESET OVERRIDES", &format!("{} SET", v.overrides_count), Some(Act::ResetOverrides), BLACK);
}

fn input(surf: &mut Surface, font: &FontRef, r: &mut Rows, v: &View, prefs: Preferences) {
    r.row(surf, font, "STEALTH", if prefs.mode == Mode::Stealth { "●" } else { "" }, Some(Act::SetMode(Mode::Stealth)), BLACK);
    r.row(surf, font, "GUIDED", if prefs.mode == Mode::Guided { "●" } else { "" }, Some(Act::SetMode(Mode::Guided)), BLACK);
    r.row(surf, font, "IDLE-SEND", if prefs.idle_send_ms == 0 { "OFF" } else { "ON" }, Some(Act::ToggleIdle), BLACK);
    if prefs.idle_send_ms > 0 { r.stepper(surf, font, "IDLE DELAY", &format!("{} MS", prefs.idle_send_ms), Act::StepIdle); }
    r.stepper(surf, font, "PALM HOLDOFF", &format!("{} MS", v.palm_ms), Act::StepPalm);
}

fn learn(surf: &mut Surface, font: &FontRef, r: &mut Rows, v: &View, prefs: Preferences) {
    let on = prefs.page == PrefPage::Learn;
    r.row(surf, font, "KIDS LEARN MODE", if on { "ON" } else { "OFF" }, Some(Act::ToggleLearn), BLACK);
    r.stepper(surf, font, "TUTOR MODEL", if v.tutor_model.is_empty() { "DEFAULT" } else { &v.tutor_model }, Act::StepTutorModel);
    r.stepper(surf, font, "NEXT-PAGE DWELL", if v.dwell_ms == 0 { "OFF" } else { &format!("{} MS", v.dwell_ms) }, Act::StepDwell);
    r.line(surf, font, "LEVEL AND SKILLS LIVE ON THE LEARN MENU");
}

fn wifi_rows(surf: &mut Surface, font: &FontRef, r: &mut Rows, w: &wifi::View) {
    let s = &w.status;
    let head = match (&s.ssid, s.connected) {
        (Some(ssid), true) => format!("{} · {} · {}", ssid.to_uppercase(), s.ip.clone().unwrap_or_default(), "▮".repeat(s.rssi.map(wifi::bars).unwrap_or(0) as usize)),
        (Some(ssid), false) => format!("{} · CONNECTING", ssid.to_uppercase()),
        _ => "DISCONNECTED".into(),
    };
    r.line(surf, font, &head);
    if let Some(b) = w.busy { r.line(surf, font, &format!("{b}…")); }
    if let Some(e) = &w.error { r.line(surf, font, &e.to_uppercase()); }
    r.line(surf, font, "SAVED");
    for n in &w.saved {
        r.row(surf, font, &n.ssid.to_uppercase(), if n.current { "●" } else if n.disabled { "DISABLED" } else { "" }, Some(Act::WifiSelect(n.id)), BLACK);
    }
    r.row(surf, font, "RESCAN", "", Some(Act::WifiRescan), BLACK);
    if !w.seen.is_empty() {
        r.line(surf, font, "IN RANGE");
        for n in &w.seen {
            let act = n.saved_id.map(Act::WifiSelect);
            r.row(surf, font, &n.ssid.to_uppercase(), &"▮".repeat(wifi::bars(n.rssi) as usize), act, BLACK);
        }
        r.line(surf, font, "NEW NETWORKS ARE ADDED OVER SSH");
    }
}

fn device_rows(surf: &mut Surface, font: &FontRef, r: &mut Rows, f: &device::Facts) {
    for s in [&f.battery, &f.storage, &format!("CLOCK: {}", f.clock), &f.os, &format!("BUILD: {}", f.build), &f.hub] {
        r.line(surf, font, s);
    }
}

fn power(surf: &mut Surface, font: &FontRef, r: &mut Rows) {
    r.row(surf, font, "SLEEP", "", Some(Act::Sleep), BLACK);
    r.row(surf, font, "LEAVE TO STOCK UI", "OR HOLD FIVE FINGERS", Some(Act::Leave), BLACK);
    r.armed_row(surf, font, "REBOOT", Act::Reboot);
    r.armed_row(surf, font, "POWER OFF", Act::PowerOff);
}
```

If the font lacks `●`, `▮`, `−`, substitute `*`, `|`, `-` — check by rendering once in the test (`script::rasterize_line(...).width > 0`). Keep whatever the Liberation face draws.

**Step 4: Run** `cargo test --quiet system` — Expected: all PASS. `cargo clippy --quiet` clean.

**Step 5: Commit** — `git add src/ui.rs src/system && git commit -m "feat: the SYSTEM page draws six sections and hands back every tap target it painted"`

---

### Task 9: Wire the page into the pad loop (replaces Settings)

[This task's text was amended in place during Task 6. As built: the Wi-Fi
channel lives on `Page` (`wifi_tx`/`wifi_rx`), not in `main`; `State::System`
holds `saved: Vec<u8>`, not `Option<Vec<u8>>`; `env_u32` was added beside
`env_u64` in f8d07ed, the review follow-up to this task; the page is
`Page::default()`.]

**Files:**
- Modify: `src/main.rs` — the sites below
- Modify: `src/ui.rs` — delete `draw_settings` (946-977) and `settings_action` (979-993); delete `Action::SetMode/ToggleIdle/ToggleLearn/Quit` only if nothing else uses them after this task (`grep -n` first; `apply_control` at 2764-2767 still matches them — keep the variants, delete the two functions).

This is the one task without a new unit test: the loop is exercised by the existing `ux_tests` and by hardware in Task 11. Do it in this order, running `cargo build --quiet` after each numbered step so the compiler walks you to every site.

1. **State.** `src/main.rs:125` — replace `Settings { saved: Option<Vec<u8>>, return_to: Box<State> }` with
   ```rust
   /// The SYSTEM page, full-screen. `saved` is the canvas underneath;
   /// `None` after Close, waiting for pen-up like the drawer.
   System { page: Box<system::Page>, saved: Option<Vec<u8>>, return_to: Box<State> },
   ```
   Replace every `State::Settings { .. }` in match patterns (lines 870, 1240, 1303, 1407, 2130, 2246) with `State::System { .. }`.

2. **Long-lived values that must be re-readable.** At line 660 `let palm_holdoff` → `let mut palm_holdoff`; line 704 `let oracle` → `let mut oracle`; line 756 `let learn_next_dwell` → `let mut learn_next_dwell`; line 780 `let learn_model` → `let mut learn_model`. Add, after line 787:
   ```rust
   let presets = presets::load();
   let (wifi_tx, wifi_rx) = mpsc::channel::<system::wifi::Event>();
   ```

3. **A helper that builds the View** (free fn near `close_overlay`):
   ```rust
   fn system_view(presets: &[presets::Preset], overrides: &overrides::Overrides) -> system::draw::View {
       let env = |k: &str| std::env::var(k).unwrap_or_default();
       system::draw::View {
           presets: presets.to_vec(),
           active_preset: presets::active(presets, std::env::var("RIDDLE_OPENAI_BASE").ok().as_deref()),
           base: env("RIDDLE_OPENAI_BASE"), model: env("RIDDLE_OPENAI_MODEL"), ask_model: env("RIDDLE_OPENAI_ASK_MODEL"),
           reasoning: env("RIDDLE_OPENAI_REASONING"),
           max_tokens: env("RIDDLE_OPENAI_MAX_TOKENS").parse().unwrap_or(2000),
           key_set: std::env::var("RIDDLE_OPENAI_KEY").is_ok(),
           overrides_count: overrides.len(),
           palm_ms: env("RIDDLE_PALM_MS").parse().unwrap_or(500),
           tutor_model: env("RIDDLE_LEARN_MODEL"), dwell_ms: env("RIDDLE_LEARN_NEXT_MS").parse().unwrap_or(5000),
           facts: system::device::gather(),
       }
   }
   ```
   Add `pub fn len(&self) -> usize { self.pairs.len() }` to `Overrides`.

4. **Opening.** Two sites draw the panel: line 905-909 (Stealth edge swipe) and `apply_control` 2755-2762 (`Action::Settings`). Both become:
   ```rust
   let old = std::mem::replace(&mut state, State::Listening { last_pen: None });
   let saved = surf.copy_rect(0, 0, SCREEN_W, SCREEN_H);
   let mut page = Box::new(system::Page::new());
   system::draw::draw(&mut surf, &ui_font, &mut page, &system_view(&presets, &overrides), prefs);
   disp.update(0, 0, SCREEN_W as i32, SCREEN_H as i32, false);
   state = State::System { page, saved: Some(saved), return_to: Box::new(old) };
   ```
   `apply_control` needs `presets: &[presets::Preset], overrides: &overrides::Overrides` added to its parameters (and the call at line ~1009 updated). Inside `apply_control` use `state`/`surf` as the references it already has.

5. **Closing.** `close_overlay` line 2246: `State::System { saved: Some(bytes), return_to, .. } => { surf.paste_rect(0, 0, SCREEN_W, SCREEN_H, &bytes); disp.update(0, 0, SCREEN_W as i32, SCREEN_H as i32, false); *state = *return_to; }`. Restore at line 2130 mirrors the old Settings arm with the new fields.

6. **Taps.** Replace the whole `else if matches!(state, State::Settings { .. }) { … }` block (1017-1063) with a call `system_tap(x, y, …)` implemented as a free fn taking `&mut state, &mut surf, &disp, &ui_font, &mut prefs, &mut idle_commit, &mut overrides, &presets, &mut oracle, &store, &mut palm_holdoff, &mut learn_next_dwell, &mut learn_model, &wifi_tx, &mut sleep_requested, &mut learn_session, &mut user_ink, &mut drawer_selection, &mut drawer_scroll` and returning `enum After { Stay, Closed, Leave }`. The body:
   ```rust
   let State::System { page, .. } = state else { return After::Stay };
   let Some(act) = page.hits.at(x, y) else { return After::Stay };
   let now = Instant::now();
   if page.arm.armed().is_some() && !act.is_destructive() { page.arm.clear(); }
   let mut reopen_oracle = false;
   let mut reread = false;
   page.notice = None;
   match act {
       Act::Tab(s) => { page.section = s; if s == Section::Wifi && page.wifi.begin("READING") { system::wifi::spawn(Cmd::Refresh, wifi_tx.clone()); } }
       Act::Close => { close_overlay(...); return After::Closed; }
       Act::SetMode(m) => { prefs.mode = m; let _ = prefs.save(); }
       Act::ToggleIdle => { prefs.idle_send_ms = if prefs.idle_send_ms == 0 { 2800 } else { 0 }; *idle_commit = Duration::from_millis(prefs.idle_send_ms); let _ = prefs.save(); }
       Act::StepIdle(d) => { prefs.idle_send_ms = step(&IDLE_MS, prefs.idle_send_ms, d); *idle_commit = …; let _ = prefs.save(); }
       Act::StepPalm(d) => { let cur = env_u64("RIDDLE_PALM_MS", 500); overrides.set("RIDDLE_PALM_MS", &step(&PALM_MS, cur, d).to_string()); reread = true; }
       Act::Preset(i) => { if let Some(p) = presets.get(i) { overrides.set("RIDDLE_OPENAI_BASE", &p.base); overrides.set("RIDDLE_OPENAI_MODEL", &p.model); overrides.set("RIDDLE_OPENAI_REASONING", &p.reasoning); reopen_oracle = true; } }
       Act::StepModel(d) => { step_in_preset("RIDDLE_OPENAI_MODEL", d, …); reopen_oracle = true; }
       Act::StepAskModel(d) => { step_in_preset("RIDDLE_OPENAI_ASK_MODEL", d, …); }
       Act::StepReasoning(d) => { let cur = env("RIDDLE_OPENAI_REASONING"); let next = step(&REASONING, cur.as_str(), d); overrides.set("RIDDLE_OPENAI_REASONING", next); reopen_oracle = true; }
       Act::StepMaxTokens(d) => { … MAX_TOKENS …; reopen_oracle = true; }
       Act::ResetOverrides => { overrides.reset(); reopen_oracle = true; reread = true; }
       Act::ToggleLearn => { /* move the existing ToggleLearn body here verbatim (1025-1049); it closes the overlay and returns After::Closed */ }
       Act::StepTutorModel(d) => { step_in_preset("RIDDLE_LEARN_MODEL", d, …); reread = true; }
       Act::StepDwell(d) => { … DWELL_MS → RIDDLE_LEARN_NEXT_MS …; reread = true; }
       Act::WifiSelect(id) => { if page.wifi.begin("JOINING") { system::wifi::spawn(Cmd::Select(id), wifi_tx.clone()); } }
       Act::WifiRescan => { if page.wifi.begin("SCANNING") { system::wifi::spawn(Cmd::Scan, wifi_tx.clone()); } }
       Act::Sleep => { *sleep_requested = true; close_overlay(...); return After::Closed; }
       Act::Leave => { eprintln!("g-pad: leave from system"); return After::Leave; }
       Act::Reboot | Act::PowerOff => match page.arm.tap(act, now) {
           Outcome::Armed(_) => {}
           Outcome::Confirmed(a) => {
               let verb = if a == Act::Reboot { "reboot" } else { "poweroff" };
               eprintln!("g-pad: {verb} from system");
               if let Err(e) = std::process::Command::new("systemctl").arg(verb).status() { page.notice = Some(format!("{verb} failed: {e}").to_uppercase()); }
           }
       },
   }
   if reopen_oracle {
       match oracle::Oracle::spawn(store.is_some()) {
           Ok(o) => { *oracle = Some(o); page.notice = Some("ORACLE READY".into()); }
           Err(e) => { page.notice = Some(format!("ORACLE UNCHANGED: {e}").to_uppercase()); }
       }
   }
   if reread {
       *palm_holdoff = env_ms("RIDDLE_PALM_MS", 500);
       *learn_next_dwell = learn_dwell_from_env();   // extract lines 756-763 into this fn
       *learn_model = learn_model_from_env();        // extract lines 780-782 into this fn
   }
   system::draw::draw(surf, ui_font, page, &system_view(presets, overrides), *prefs);
   disp.update(0, 0, SCREEN_W as i32, SCREEN_H as i32, false);
   After::Stay
   ```
   `step_in_preset(key, dir, presets, overrides)` steps a model name through the active preset's `models` list (first entry when the current name is not in the list); an empty list leaves the value alone. Where the old block did `break 'pad` on Quit, the caller now does `if matches!(after, After::Leave) { break 'pad; }` and `continue`s on `Closed`. Put `learn_dwell_from_env`/`learn_model_from_env` next to `env_ms` (line 93) and use them at the two original sites too — no duplicated parsing.

7. **Wi-Fi events and arm lapses.** In the loop, right after the touch drain (line ~875), add:
   ```rust
   if let State::System { page, .. } = &mut state {
       let mut changed = false;
       while let Ok(ev) = wifi_rx.try_recv() {
           changed = true;
           match ev {
               system::wifi::Event::Status(s) => { page.wifi.status = s; }
               system::wifi::Event::Saved(v) => { page.wifi.saved = v; page.wifi.busy = None; }
               system::wifi::Event::Seen(v) => { page.wifi.seen = v; page.wifi.busy = None; }
               system::wifi::Event::Failed(e) => { page.wifi.error = Some(e); page.wifi.busy = None; }
           }
       }
       if page.arm.disarm_if_lapsed(Instant::now()) { changed = true; }
       if changed && page.section == system::Section::Wifi || changed && page.section == system::Section::Power {
           system::draw::draw(&mut surf, &ui_font, page, &system_view(&presets, &overrides), prefs);
           disp.update(0, 0, SCREEN_W as i32, SCREEN_H as i32, false);
       }
   }
   ```
   (Wi-Fi events while another section is open just update the model; the next draw shows them.)

8. **Delete** `ui::draw_settings` and `ui::settings_action` and their doc comments. `cargo build --quiet` must be warning-free; `cargo clippy --quiet` clean.

9. **Run** `cargo test --quiet` — Expected: every existing test still passes (201 + the new ones).

10. **Commit** — `git add -A src && git commit -m "feat: SETTINGS becomes SYSTEM — config, Wi-Fi, device and power live on the page, oracle re-spawns without a restart"`

---

### Task 10: Docs and the example env

**Files:**
- Modify: `oracle.env.example` (a comment block under the palm section)
- Modify: `docs/anthink-interaction.md` §Switching modes (one paragraph)
- Modify: `docs/rm2-setup.md` (an "In-app SYSTEM page" subsection)

**Step 1:** `oracle.env.example`, after the palm block:

```
# --- in-app overrides ---
# The SYSTEM page (edge swipe in Stealth, SETTINGS in Guided) writes
# /home/root/g-pad-data/overrides. Keys there beat this file until RESET
# OVERRIDES on the page removes them. Presets come from settings.schema.json
# beside the binary (RIDDLE_PRESETS_FILE points elsewhere). The API key is
# never written by the page.
# RIDDLE_PRESETS_FILE=/home/root/xovi/exthome/appload/g-pad/settings.schema.json
```

**Step 2:** `docs/anthink-interaction.md` after line 174, one paragraph: the SYSTEM page is the one settled surface; what it holds; that overrides beat `oracle.env`; that reboot/power off are the first vermilion confirmations.

**Step 3:** `docs/rm2-setup.md`: a short subsection listing what no longer needs ssh (preset/model/reasoning/tokens, palm ms, tutor model, dwell, Wi-Fi reconnect among saved networks, reboot/power off) and what still does (a new API key, a new Wi-Fi password, new presets).

**Step 4: Commit** — `git add oracle.env.example docs && git commit -m "docs: the SYSTEM page, and what still needs ssh"`

---

### Task 11: Cross-build, deploy, verify on hardware

[Build check gates: `cargo zigbuild --target armv7-unknown-linux-gnueabihf`
and `cargo-zigbuild check --target armv7-unknown-linux-gnueabihf --features
takeover,rm2`.]

**Files:** none new. Requires the tablet awake on USB (`/usr/bin/ssh rm2 true`).

1. `export PATH="$HOME/.cargo/bin:$PATH"; cargo test --quiet && ./build-takeover-zig.sh && DEVICE=rm2 ./scripts/make-bundle.sh` — note the new `md5 -q dist/rm2-takeover/g-pad/g-pad`.
2. Stage: `/usr/bin/scp -O dist/rm2-takeover/g-pad/g-pad rm2:/home/root/xovi/exthome/appload/g-pad/g-pad.new` and the same for `g-pad-learn`; also `scp -O settings.schema.json` to both dirs (the presets file beside the binary).
3. Swap and restart with the detached script from the 2026-09-11 deploy (`/tmp/g-pad-deploy.sh` on the tablet; recreate it from memory note `g-pad-restart-procedure` if gone): `mv g-pad g-pad.prev-<date>; mv g-pad.new g-pad`, then stop → settle → `systemctl stop rm2fb.service` → start. NEVER `systemctl restart`.
4. Verify: `journalctl -u g-pad-takeover.service -b | grep -c "rm2fb update failed"` is 0, the tail says "the pad is open", `md5sum /proc/$(pidof g-pad)/exe` matches the bundle.
5. On the tablet with a finger: edge-swipe (Stealth) → SYSTEM opens full page; tap each tab; INPUT → PALM + twice → `cat /home/root/g-pad-data/overrides` shows `RIDDLE_PALM_MS=1000`; ORACLE → tap Vellum → journal shows a fresh `g-pad: http oracle base=…` line; WI-FI → status line names the network, RESCAN lists neighbours within ~5 s; DEVICE → battery and `HUB: OK …` (tunnel up) ; POWER → REBOOT once inverts, second tap within 5 s reboots (do this last; the pad autostarts at boot). Write outcomes into the memory note for this feature.
6. Commit nothing new unless a fix was needed; then merge per the finishing-a-development-branch skill.
