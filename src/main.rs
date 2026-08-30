//! g-pad — an e-ink writing pad for the reMarkable 2.
//!
//! Write on the page with the pen. Rule a line beneath the entry and the pad
//! reads your ink; an answer writes itself in a flowing hand and remains.
//!
//! Two display backends (picked at runtime): windowed via qtfb/AppLoad when
//! QTFB_KEY is set, or full takeover via the vendor engine (quill) when
//! built with --features takeover and launched with xochitl stopped.

mod ask;
mod brief;
mod bridge;
mod display;
mod evdev;
mod fb;
mod help;
mod ink;
mod memory;
mod oracle;
mod pen;
mod power;
mod preferences;
mod qtfb;
#[cfg(all(feature = "rm2", not(feature = "takeover")))]
mod rm2fb;
mod script;
mod surface;
mod touch;
mod ui;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ab_glyph::FontRef;

use fb::{BBox, SCREEN_H, SCREEN_W};
use oracle::Event;
use surface::{Surface, BLACK, FADED, WHITE};

const FONT_TTF: &[u8] = include_bytes!("../fonts/DancingScript.ttf");
const PNG_PATH: &str = "/tmp/g-pad-page.png";

/// How long the diary waits on a silent oracle before giving up on the turn.
/// Generous: thinking models can lead with a long silence.
const ORACLE_PATIENCE: Duration = Duration::from_secs(120);
const REPLY_PX: f32 = 96.0;
const MARGIN_X: i32 = 120;
const THINK_X: i32 = 48;
const THINK_Y: i32 = 120;

const USAGE: &str = "\
g-pad — an e-ink writing pad

usage:
  g-pad                       open the pad (windowed when AppLoad sets
                              QTFB_KEY, otherwise takeover via libquill)
  g-pad --oracle-test [PNG]   run one oracle turn against PNG (default
                              /tmp/g-pad-page.png) and print the streamed
                              reply; verifies key + endpoint + model
  g-pad --version             print the version

configuration lives in oracle.env next to the binary — see
oracle.env.example for every RIDDLE_* variable.
";

type OracleRx = mpsc::Receiver<Result<Event, String>>;

/// Millisecond duration from the environment, with a default.
fn env_ms(name: &str, default: u64) -> Duration {
    Duration::from_millis(
        std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default),
    )
}

enum State {
    Listening { last_pen: Option<Instant> },
    Drinking { stage: u32, next: Instant, region: BBox, rx: OracleRx },
    /// `wrote` is where the user's (drunk) ink was: that ghost is cleared
    /// before the reply, which always starts at the top writing line.
    Thinking { rx: OracleRx, pulse: Instant, blot_on: bool, since: Instant, wrote: BBox },
    Replying { plan: WritePlan, next: Instant, rx: Option<OracleRx> },
    /// A completed reply stays until an explicit dismissal or new turn.
    /// `more` is leftover reply text that did not fit this page.
    Lingering { region: BBox, more: String },
    FadingReply { stage: u32, next: Instant, region: BBox },
    /// The guide panel. `panel: None` = dismissed, waiting for pen-up so the
    /// dismissing touch doesn't leave a mark on the page.
    Help { panel: Option<help::Help>, until: Instant },
    /// A remembered page rising through the paper: date, the writer's own
    /// past ink, Tom's old reply — all in faded ink. `saved` is today's page.
    Conjuring { plan: ConjurePlan, next: Instant, saved: Vec<u8> },
    /// The conjured memory rests on the page. Pen contact (or time) dissolves
    /// it and today's page returns. `saved: None` = dismissed, waiting pen-up.
    MemoryShown { saved: Option<Vec<u8>>, until: Instant, region: BBox },
    Drawer { panel: Option<ui::Drawer>, return_to: Box<State> },
    #[allow(dead_code)]
    ExpandedConversation { panel: Option<ui::Drawer>, return_to: Box<State> },
    Settings { saved: Option<Vec<u8>>, return_to: Box<State> },
}

#[derive(Clone, Copy)]
enum CommitMode {
    Capture,
    Ask,
}

/// A memory being rewritten onto the page: pre-positioned strokes with their
/// original radii, drawn in faded ink.
struct ConjurePlan {
    strokes: Vec<Vec<(i32, i32, i32)>>,
    stroke_i: usize,
    point_i: usize,
    region: BBox,
}

struct WritePlan {
    strokes: Vec<Vec<(i32, i32)>>,
    stroke_i: usize,
    point_i: usize,
    region: BBox,
    /// Where the next streamed chunk's first line starts.
    next_y: i32,
    /// Lines that did not fit below `next_y`; shown on the next page.
    leftover: String,
    /// Lines actually laid out on this page, for going back.
    shown: String,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // Diagnostic: run one oracle turn and print the streamed chunks.
        // Lets you verify your endpoint + key + model before ever launching
        // the diary. No display needed.
        Some("--oracle-test") => {
            let png = args.get(2).map(String::as_str).unwrap_or(PNG_PATH);
            std::process::exit(oracle_test(png));
        }
        // Diagnostic: run the Path B raw->PNG conversion alone (consuming the
        // raw, like startup does) so a fresh capture can be checked without
        // launching the diary. No display needed.
        Some("--ask-test") => {
            let raw = args.get(2).map(String::as_str).unwrap_or("/tmp/xochitl-screen.raw");
            std::process::exit(match ask::prepare(raw, "/tmp/riddle-ask.png") {
                Some(()) => {
                    println!("ok: /tmp/riddle-ask.png");
                    0
                }
                None => 1,
            });
        }
        // Diagnostic: print the newest stock-notes page render Track B would
        // ask about (RIDDLE_XOCHITL_DIR / RIDDLE_ASK_MAX_AGE honored), then
        // exit. Verifies the finder against the real store.
        Some("--xochitl-page") => {
            std::process::exit(match ask::newest_xochitl_page() {
                Some(p) => {
                    println!("{p}");
                    0
                }
                None => {
                    eprintln!("no fresh xochitl page found");
                    1
                }
            });
        }
        Some("--version" | "-V") => {
            println!("riddle {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--help" | "-h") => {
            print!("{USAGE}");
            return;
        }
        Some(flag) if flag.starts_with('-') => {
            eprintln!("g-pad: unknown flag {flag}\n");
            eprint!("{USAGE}");
            std::process::exit(2);
        }
        _ => {}
    }
    if let Err(e) = run() {
        eprintln!("g-pad: fatal: {e}");
        std::process::exit(1);
    }
}

fn oracle_test(png: &str) -> i32 {
    let store = memory::MemoryStore::open();
    let o = match oracle::Oracle::spawn(store.is_some()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("oracle spawn failed: {e}");
            return 1;
        }
    };
    let ctx = build_ctx(&store);
    let (tx, rx) = mpsc::channel();
    let t0 = Instant::now();
    o.ask(png, &ctx, tx);
    let mut got = String::new();
    loop {
        match rx.recv() {
            Ok(Ok(Event::Ink(chunk))) => {
                if got.is_empty() {
                    eprintln!("first chunk +{}ms", t0.elapsed().as_millis());
                }
                print!("{chunk} ");
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
                got.push_str(&chunk);
            }
            Ok(Ok(Event::Show(id))) => {
                println!("[would conjure memory {id} — {}]", memory::spoken_date(id));
                got.push_str("(show)");
            }
            Ok(Ok(Event::Transcript(t))) => eprintln!("\n[transcript] {t}"),
            Ok(Err(e)) => {
                eprintln!("\noracle error: {e}");
                return 1;
            }
            Err(_) => break, // disconnected = reply complete
        }
    }
    println!("\n--- reply complete ({}ms, {} chars) ---", t0.elapsed().as_millis(), got.len());
    if got.trim().is_empty() { 1 } else { 0 }
}

/// What the diary sends alongside the page: its memory of recent turns and
/// the catalog the oracle picks conjured pages from. Empty when memory is off.
fn build_ctx(store: &Option<memory::MemoryStore>) -> oracle::TurnContext {
    let turns: usize = std::env::var("RIDDLE_MEMORY_TURNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);
    oracle::context_snapshot(store, turns).context
}

fn run() -> std::io::Result<()> {
    // The reply hand: RIDDLE_FONT_FILE (any TTF/OTF next to the binary or an
    // absolute path), else the embedded Dancing Script. Loaded once and
    // leaked — one font per process lifetime.
    let font_bytes: &'static [u8] = match std::env::var("RIDDLE_FONT_FILE") {
        Ok(p) => match std::fs::read(&p) {
            Ok(b) => {
                eprintln!("g-pad: reply font {p}");
                Box::leak(b.into_boxed_slice())
            }
            Err(e) => {
                eprintln!("g-pad: font {p} unreadable ({e}); using Dancing Script");
                FONT_TTF
            }
        },
        Err(_) => FONT_TTF,
    };
    let font = FontRef::try_from_slice(font_bytes).map_err(std::io::Error::other)?;
    let ui_font = FontRef::try_from_slice(ui::UI_FONT_TTF).map_err(std::io::Error::other)?;

    let (disp, mut surf) = display::Display::open()?;
    // Anything that isn't the qtfb window owns the panel, raw input devices,
    // and power button: Quill on either tablet, or the legacy rm2fb fallback.
    let takeover = !matches!(disp, display::Display::Qtfb(_));
    eprintln!(
        "g-pad: display {} ({}x{} stride {})",
        if takeover { "quill/takeover" } else { "qtfb" },
        surf.w,
        surf.h,
        surf.stride
    );

    let mut pen_dev = match pen::PenDevice::open() {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("g-pad: raw pen unavailable ({e}), falling back to qtfb pen events");
            None
        }
    };
    // Takeover mode: touch is ours too; 5-finger tap = quit.
    let mut touch_dev = if takeover { touch::TouchDevice::open().ok() } else { None };
    // Takeover mode: the power button is ours too (sleep page + suspend).
    let mut power_dev = if takeover {
        power::PowerButton::open().map_err(|e| eprintln!("g-pad: no power button ({e})")).ok()
    } else {
        None
    };
    // Ignore power presses briefly after a wake: the waking press itself (and
    // key bounce) arrives on our grabbed fd and must not re-suspend.
    let mut power_grace = Instant::now();

    let sigterm = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&sigterm))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&sigterm))?;

    // Blank page.
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    disp.update_all(surf.w, surf.h);

    // The diary's memory (None = RIDDLE_MEMORY=off or the dir is unusable).
    let mut store = memory::MemoryStore::open();
    let mut prefs = preferences::Preferences::load();
    if let Some(ref s) = store {
        eprintln!("g-pad: memory holds {} pages", s.entries.len());
    }

    // Warm the oracle now: pi loads Node + extensions + codex auth ONCE here,
    // while you're still picking up the pen, so replies pay only model latency.
    let oracle = match oracle::Oracle::spawn(store.is_some()) {
        Ok(o) => {
            eprintln!("g-pad: oracle ready");
            Some(o)
        }
        Err(e) => {
            eprintln!("g-pad: oracle spawn failed: {e}");
            None
        }
    };

    let mut user_ink = ink::Ink::new();
    let mut state = State::Listening { last_pen: None };
    let mut pen_down = false;
    // The turn being remembered: strokes captured at commit, transcript and
    // reply accumulated as they stream, stored when the turn completes.
    let mut turn_id: u64 = 0;
    let mut turn_strokes: memory::Strokes = Vec::new();
    let mut turn_reply = String::new();
    let mut turn_transcript: Option<String> = None;
    let mut turn_failed = false;
    let mut reply_pages: Vec<String> = Vec::new();
    let mut reply_page: usize = 0;
    // Raw stylus contact, tracked in every state (the guide dismisses on it).
    // `stylus_on` is the level; `stylus_tapped` latches any contact seen this
    // loop iteration, so a tap that starts AND ends within one drain still
    // registers.
    let mut stylus_on = false;
    let mut stylus_tapped = false;
    let mut ink_dirty = BBox::empty();
    let mut last_flush = Instant::now();
    // Takeover swaps are cheap and synchronous; qtfb needs coalescing — but
    // the interval is the dominant tunable ink latency, so let users trade
    // CPU for feel (RIDDLE_FLUSH_MS).
    let flush_every =
        if takeover { Duration::from_millis(8) } else { env_ms("RIDDLE_FLUSH_MS", 12) };
    // Optional compatibility behavior: how long the pen rests before commit.
    // The saved preference wins over RIDDLE_IDLE_MS and defaults to disabled.
    let mut idle_commit = Duration::from_millis(prefs.idle_send_ms);
    // Reply pen width: thicker ink reads darker on fast e-ink waveforms.
    let reply_w: i32 = std::env::var("RIDDLE_REPLY_WIDTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    // Deliberate send: latched when the user draws the send rule.
    let mut send_mode: Option<CommitMode> = None;
    let mut drawer_selection: Option<usize> = None;
    let mut drawer_scroll = 0i32;
    let mut controls_saved: Option<Vec<u8>> = None;
    let mut controls_until: Option<Instant> = None;
    let mut sleep_requested = false;
    let mut queued_gestures: Vec<touch::Gesture> = Vec::new();
    let mut fallback_touch: Option<((i32, i32), (i32, i32))> = None;
    let mut control_pen_latched = false;

    // Reply draw speed: points drawn per animation frame. Higher = the answer
    // appears faster (fewer seconds of watching it scrawl). Was 26; the e-ink
    // coalesces the per-frame dirty rect into one update, so larger batches are
    // nearly free. RIDDLE_REPLY_POINTS overrides.
    let reply_points: usize = std::env::var("RIDDLE_REPLY_POINTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);

    // Startup ask — two sources for "what did you write in the stock notes app":
    //   Track A (RIDDLE_ASK_RAW): a live-screen framebuffer dump the launch
    //     script captured (parked — capture doesn't work on this OS build).
    //   Track B (RIDDLE_ASK_XOCHITL): the newest rendered xochitl page PNG —
    //     you write in stock notes at native latency, close the page, open The
    //     Diary. This is already a PNG, so it feeds the oracle directly.
    // Either way the answer writes itself onto the blank page while you watch,
    // and the exchange flows into the diary's memory like any other turn — the
    // oracle's transcription postscript covers the words; only the pen strokes
    // are absent (they were penned in xochitl, not here).
    let ask_png: Option<String> = std::env::var("RIDDLE_ASK_RAW")
        .ok()
        .and_then(|raw| ask::prepare(&raw, PNG_PATH).map(|_| PNG_PATH.to_string()))
        .or_else(|| {
            // A failed/absent capture falls through to Track B, never blocks it.
            std::env::var("RIDDLE_ASK_XOCHITL")
                .map(|v| v != "0" && v != "off")
                .unwrap_or(false)
                .then(ask::newest_xochitl_page)
                .flatten()
        });
    if let Some(png) = ask_png {
        if let Some(ref o) = oracle {
            turn_id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            turn_strokes = Vec::new();
            turn_reply.clear();
            turn_transcript = None;
            turn_failed = false;
            let ctx = build_ctx(&store);
            let (tx, rx) = mpsc::channel();
            o.ask(&png, &ctx, tx);
            eprintln!("g-pad: asking about the captured page");
            state = State::Thinking {
                rx,
                pulse: Instant::now(),
                blot_on: false,
                since: Instant::now(),
                wrote: BBox::empty(),
            };
        }
    }

    eprintln!("g-pad: the pad is open");

    loop {
        if sigterm.load(Ordering::Relaxed) {
            break;
        }
        let mut gestures = std::mem::take(&mut queued_gestures);
        if let Some(t) = touch_dev.as_mut() { gestures.extend(t.drain()); }
        if gestures.contains(&touch::Gesture::Quit) {
            eprintln!("g-pad: 5-finger quit");
            break;
        }

        // Touch belongs to overlays while they are visible. Edge gestures are
        // consumed here and never reach page navigation or page ink.
        for gesture in gestures {
            match gesture {
                touch::Gesture::OpenControls => {
                    if matches!(state, State::Listening { .. } | State::Lingering { .. }) {
                        if prefs.mode == preferences::Mode::Guided {
                            if controls_saved.is_none() {
                                controls_saved = Some(ui::draw_controls(&mut surf, &ui_font, matches!(state, State::Lingering { .. })));
                                disp.update(0, 0, SCREEN_W as i32, 82, false);
                            }
                            controls_until = Some(Instant::now() + Duration::from_secs(12));
                        } else {
                            let old = std::mem::replace(&mut state, State::Listening { last_pen: None });
                            let saved = ui::draw_settings(&mut surf, &ui_font, prefs);
                            disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
                            state = State::Settings { saved: Some(saved), return_to: Box::new(old) };
                        }
                    }
                }
                touch::Gesture::OpenDrawer if matches!(state, State::Listening { .. } | State::Lingering { .. }) => {
                    if let Some(saved) = controls_saved.take() { ui::restore_controls(&mut surf, &saved); }
                    let old = std::mem::replace(&mut state, State::Listening { last_pen: None });
                    let thread = store.as_ref().and_then(|s| s.conversations().len().checked_sub(1));
                    let panel = ui::Drawer::open(&surf, ui::DrawerKind::History, drawer_selection, 0, thread);
                    let snapshot = oracle::context_snapshot(&store, memory_turns());
                    ui::draw_drawer(&mut surf, &ui_font, &store, &snapshot, &panel);
                    disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
                    state = State::Drawer { panel: Some(panel), return_to: Box::new(old) };
                }
                touch::Gesture::CloseDrawer => {
                    close_overlay(&mut state, &mut surf, &disp, &mut drawer_selection, &mut drawer_scroll);
                }
                touch::Gesture::Page(delta) if matches!(state, State::Lingering { .. }) => {
                    let _ = step_reply_page(delta, &font, reply_w, &mut reply_pages, &mut reply_page,
                        &mut state, &mut surf, &disp);
                }
                touch::Gesture::Scroll(delta) | touch::Gesture::Page(delta) => {
                    let panel = match &mut state {
                        State::Drawer { panel: Some(p), .. } | State::ExpandedConversation { panel: Some(p), .. } => Some(p),
                        _ => None,
                    };
                    if let Some(panel) = panel {
                        panel.scroll_by(delta);
                        drawer_scroll = panel.scroll;
                        let snapshot = oracle::context_snapshot(&store, memory_turns());
                        ui::draw_drawer(&mut surf, &ui_font, &store, &snapshot, panel);
                        disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
                    }
                }
                touch::Gesture::Tap(x, y) => {
                    if matches!(state, State::Lingering { .. }) {
                        if step_reply_page(1, &font, reply_w, &mut reply_pages, &mut reply_page,
                            &mut state, &mut surf, &disp)
                        {
                            continue;
                        }
                    }
                    if controls_saved.is_some() {
                        let action = ui::control_action(x, y, matches!(state, State::Lingering { .. }));
                        if let Some(saved) = controls_saved.take() { ui::restore_controls(&mut surf, &saved); }
                        disp.update(0, 0, SCREEN_W as i32, 82, false);
                        apply_control(action, &mut state, &mut surf, &disp, &ui_font, &store,
                            &mut user_ink, &mut send_mode, &mut sleep_requested, &mut prefs,
                            &mut idle_commit, drawer_selection, drawer_scroll);
                    } else if matches!(state, State::Settings { .. }) {
                        let action = ui::settings_action(x, y);
                        match action {
                            ui::Action::SetMode(mode) => { prefs.mode = mode; let _ = prefs.save(); }
                            ui::Action::ToggleIdle => {
                                prefs.idle_send_ms = if prefs.idle_send_ms == 0 { 2800 } else { 0 };
                                idle_commit = Duration::from_millis(prefs.idle_send_ms); let _ = prefs.save();
                            }
                            ui::Action::Close => {
                                close_overlay(&mut state, &mut surf, &disp, &mut drawer_selection, &mut drawer_scroll);
                                continue;
                            }
                            ui::Action::Quit => {
                                eprintln!("g-pad: leave from settings");
                                break;
                            }
                            _ => {}
                        }
                        ui::draw_settings(&mut surf, &ui_font, prefs);
                        disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
                    } else {
                        let action = match &mut state {
                            State::Drawer { panel: Some(p), .. } | State::ExpandedConversation { panel: Some(p), .. } => p.tap(x, y, &store),
                            _ => ui::Action::None,
                        };
                        handle_drawer_action(action, &mut state, &mut surf, &disp, &ui_font, &font, &store,
                            &mut drawer_selection, &mut drawer_scroll);
                    }
                }
                _ => {}
            }
        }

        if controls_until.is_some_and(|t| Instant::now() >= t) {
            if let Some(saved) = controls_saved.take() {
                ui::restore_controls(&mut surf, &saved);
                disp.update(0, 0, SCREEN_W as i32, 82, false);
            }
            controls_until = None;
        }

        // ---- power button: sleep page, suspend, restore on wake ----
        if let Some(ref mut p) = power_dev {
            let pressed = p.drain_pressed();
            if (pressed || sleep_requested) && Instant::now() >= power_grace {
                sleep_requested = false;
                eprintln!("g-pad: sleeping (power button)");
                let saved = help::show_sleep(&mut surf, &font, &ui_font);
                disp.full_refresh(surf.w, surf.h);
                // Let the flashing refresh finish before the panel loses power.
                std::thread::sleep(Duration::from_millis(800));
                // Ask logind to suspend, then wait to be woken.
                //
                // `systemctl suspend` is ASYNCHRONOUS: it hands logind a D-Bus
                // request and returns within milliseconds, long before the
                // kernel has frozen anything (measured on-device: same-second
                // return, actual "PM: suspend entry" several seconds later).
                // So the success counter cannot be polled on a short deadline
                // to decide whether the suspend "took" — it only moves once we
                // are already awake again on the other side.
                //
                // Wait for the counter to advance with no deadline short
                // enough to race the teardown. When it moves we have slept AND
                // resumed, which is exactly the moment to redraw.
                let count0 = power::suspend_count();
                if p.grabbed {
                    // The takeover wrapper blocks ambient sleep with
                    // systemd-inhibit. This explicit user action is
                    // intentional, so bypass that inhibitor here.
                    let _ = std::process::Command::new("systemctl")
                        .args(["--check-inhibitors=no", "suspend"])
                        .status();
                }
                // Give logind room to complete teardown, sleep, and resume.
                // A press that never reaches suspend (something else vetoed
                // it) falls out after the timeout and simply redraws the page.
                let t0 = Instant::now();
                let mut slept = false;
                while t0.elapsed() < power::SUSPEND_WAIT {
                    std::thread::sleep(Duration::from_millis(400));
                    if power::suspend_count() > count0 {
                        slept = true;
                        break;
                    }
                    // A press while still awake means the user gave up on a
                    // suspend that is not coming; stop waiting and redraw.
                    // Re-check the counter first: the press that WAKES us
                    // arrives on this same grabbed fd, and must read as "we
                    // slept", not as a cancellation.
                    if p.drain_pressed() {
                        if power::suspend_count() > count0 {
                            slept = true;
                        } else {
                            eprintln!("g-pad: suspend did not take; press cancelled the wait");
                        }
                        break;
                    }
                }
                if !slept {
                    eprintln!("g-pad: suspend never happened; waking the page");
                }
                eprintln!("g-pad: waking");
                help::restore_sleep(&mut surf, &saved);
                disp.full_refresh(surf.w, surf.h);
                power::wifi_heal();
                // Discard input that queued while asleep — stale pen events
                // would otherwise replay as phantom ink on the restored page.
                if let Some(ref mut pd) = pen_dev {
                    let _ = pd.drain();
                }
                if let Some(ref mut td) = touch_dev {
                    let _ = td.drain_check_quit();
                }
                p.drain_pressed();
                power_grace = Instant::now() + Duration::from_secs(3);
            }
        }

        // ---- raw pen (preferred path) ----
        if let Some(ref mut pdev) = pen_dev {
            for s in pdev.drain() {
                // While the marker is in proximity, its user's palm may
                // touch the capacitive sensor.  Never let that contact
                // participate in touch gestures (especially five-finger
                // quit); the pen is the authoritative input device here.
                if s.proximity && !matches!(state,
                    State::Settings { .. } | State::Drawer { .. }
                    | State::ExpandedConversation { .. } | State::Help { .. })
                {
                    if let Some(ref mut td) = touch_dev {
                        td.suppress();
                    }
                }
                let writing = s.touching && s.pressure > 40;
                stylus_on = writing;
                stylus_tapped |= writing;
                if !writing {
                    control_pen_latched = false;
                    if pen_down {
                        pen_down = false;
                        user_ink.pen_up();
                        if let State::Listening { ref mut last_pen } = state {
                            *last_pen = Some(Instant::now());
                            if let Some(mode) = absorb_send_rule(&mut user_ink, &mut surf, &disp) {
                                send_mode = Some(mode);
                            } else if help::looks_like_question_mark(user_ink.stroke_list()) {
                                send_mode = Some(CommitMode::Capture);
                            }
                        }
                    }
                    continue;
                }
                if controls_saved.is_some() && s.y < 82 {
                    if !control_pen_latched {
                        queued_gestures.push(touch::Gesture::Tap(s.x, s.y));
                        control_pen_latched = true;
                    }
                    continue;
                }
                if matches!(state, State::Settings { .. } | State::Drawer { .. } | State::ExpandedConversation { .. }) {
                    if !control_pen_latched {
                        queued_gestures.push(touch::Gesture::Tap(s.x, s.y));
                        control_pen_latched = true;
                    }
                    continue;
                }
                match state {
                    State::Listening { ref mut last_pen } => {
                        pen_down = true;
                        let d = match s.tool {
                            pen::Tool::Pen => {
                                let r = 2 + s.pressure * 3 / pen::MAX_PRESSURE;
                                user_ink.pen_point(&mut surf, s.x, s.y, r)
                            }
                            pen::Tool::Eraser => user_ink.erase_point(&mut surf, s.x, s.y, 22),
                        };
                        if !d.is_empty() {
                            ink_dirty.add(d.x0, d.y0, 0);
                            ink_dirty.add(d.x1, d.y1, 0);
                        }
                        *last_pen = Some(Instant::now());
                    }
                    State::Lingering { region, ref more } => {
                        if more.is_empty() && reply_page + 1 >= reply_pages.len() {
                            state = State::FadingReply { stage: 0, next: Instant::now(), region };
                        } else if !control_pen_latched {
                            queued_gestures.push(touch::Gesture::Page(1));
                            control_pen_latched = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        // ---- window-system events (qtfb close detection + pen fallback) ----
        let events = match disp.pump() {
            Ok(v) => v,
            Err(_) => break, // qtfb window closed
        };
        for ev in events {
            if pen_dev.is_some() && matches!(ev.input_type,
                qtfb::INPUT_PEN_PRESS | qtfb::INPUT_PEN_UPDATE | qtfb::INPUT_PEN_RELEASE) {
                continue;
            }
            match ev.input_type {
                qtfb::INPUT_PEN_PRESS | qtfb::INPUT_PEN_UPDATE => {
                    stylus_on = true;
                    stylus_tapped = true;
                    if controls_saved.is_some() && ev.y < 82 {
                        if !control_pen_latched {
                            queued_gestures.push(touch::Gesture::Tap(ev.x, ev.y));
                            control_pen_latched = true;
                        }
                        continue;
                    }
                    if let State::Listening { ref mut last_pen } = state {
                        pen_down = true;
                        let r = 2 + ev.d.clamp(0, 100) / 45;
                        let d = user_ink.pen_point(&mut surf, ev.x, ev.y, r);
                        if !d.is_empty() {
                            ink_dirty.add(d.x0, d.y0, 0);
                            ink_dirty.add(d.x1, d.y1, 0);
                        }
                        *last_pen = Some(Instant::now());
                    } else if let State::Lingering { region, more } = &state {
                        if more.is_empty() && reply_page + 1 >= reply_pages.len() {
                            let region = *region;
                            state = State::FadingReply { stage: 0, next: Instant::now(), region };
                        } else if !control_pen_latched {
                            queued_gestures.push(touch::Gesture::Page(1));
                            control_pen_latched = true;
                        }
                    } else if matches!(state, State::Settings { .. } | State::Drawer { .. } | State::ExpandedConversation { .. }) {
                        if !control_pen_latched {
                            queued_gestures.push(touch::Gesture::Tap(ev.x, ev.y));
                            control_pen_latched = true;
                        }
                    }
                }
                qtfb::INPUT_PEN_RELEASE => {
                    stylus_on = false;
                    control_pen_latched = false;
                    if pen_down {
                        pen_down = false;
                        user_ink.pen_up();
                        if let State::Listening { ref mut last_pen } = state {
                            *last_pen = Some(Instant::now());
                            if let Some(mode) = absorb_send_rule(&mut user_ink, &mut surf, &disp) {
                                send_mode = Some(mode);
                            } else if help::looks_like_question_mark(user_ink.stroke_list()) {
                                send_mode = Some(CommitMode::Capture);
                            }
                        }
                    }
                }
                qtfb::INPUT_TOUCH_PRESS => fallback_touch = Some(((ev.x, ev.y), (ev.x, ev.y))),
                qtfb::INPUT_TOUCH_UPDATE => {
                    if let Some((_, ref mut last)) = fallback_touch { *last = (ev.x, ev.y); }
                }
                qtfb::INPUT_TOUCH_RELEASE => {
                    if let Some((start, last)) = fallback_touch.take() {
                        queued_gestures.push(touch::gesture_from_points(start, last));
                    }
                }
                _ => {}
            }
        }

        // ---- coalesced ink flush ----
        if !ink_dirty.is_empty() && last_flush.elapsed() >= flush_every {
            let (x, y, w, h) = ink_dirty.rect();
            disp.update(x, y, w, h, true);
            ink_dirty = BBox::empty();
            last_flush = Instant::now();
        }

        // ---- state machine ----
        state = match state {
            State::Listening { last_pen } => match last_pen {
                Some(t)
                    if !pen_down
                        && (send_mode.is_some()
                            || (!idle_commit.is_zero() && t.elapsed() >= idle_commit))
                        && !user_ink.is_empty() =>
                {
                    let commit_mode = send_mode.take().unwrap_or(CommitMode::Capture);
                    if region_all_white(&surf, user_ink.bbox) {
                        // Everything was erased before commit: nothing to
                        // commit (and no phantom "?" from erased strokes).
                        user_ink.clear();
                        State::Listening { last_pen: None }
                    } else if help::looks_like_question_mark(user_ink.stroke_list()) {
                        // Absorb the "?" and open the guide instead of asking.
                        let (qx, qy, qw, qh) = user_ink.bbox.rect();
                        surf.fill_rect(qx as usize, qy as usize, qw as usize, qh as usize, WHITE);
                        disp.update(qx, qy, qw, qh, false);
                        user_ink.clear();
                        let panel = help::show(&mut surf, &font, takeover);
                        let (px, py, pw, ph) = panel.region.rect();
                        disp.update(px, py, pw, ph, false);
                        eprintln!("g-pad: guide shown");
                        State::Help { panel: Some(panel), until: Instant::now() + Duration::from_secs(45) }
                    } else if oracle.is_none() {
                        // No spirit at all: don't eat ink that nothing will
                        // answer — leave the writing and put the reason below.
                        let y = prepare_reply_anchor(user_ink.bbox, &mut surf, &disp);
                        let plan = plan_reply(&font, &oracle_excuse("no oracle"), Some(y));
                        State::Replying { plan, next: Instant::now(), rx: None }
                    } else {
                        if let Err(e) = user_ink.to_png(&surf, PNG_PATH) {
                            eprintln!("g-pad: rasterize failed: {e}");
                        }
                        // Remember this page: strokes now (they're cleared
                        // after the drink), transcript/reply as they stream.
                        turn_id = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        turn_strokes = user_ink.stroke_list().to_vec();
                        turn_reply.clear();
                        turn_transcript = None;
                        turn_failed = false;
                        reply_pages.clear();
                        reply_page = 0;
                        // Ask NOW: the model streams while the diary drinks the
                        // ink, hiding most of the reply latency in the animation.
                        let (tx, rx) = mpsc::channel();
                        if let Some(ref o) = oracle {
                            let ask_model = match commit_mode {
                                CommitMode::Ask => std::env::var("RIDDLE_OPENAI_ASK_MODEL").ok(),
                                CommitMode::Capture => None,
                            };
                            o.ask_with_model(PNG_PATH, &build_ctx(&store), tx, ask_model.as_deref());
                        }
                        // Both backends read the page before ask() returns; the
                        // writer's words don't need to sit on disk afterwards.
                        if std::env::var_os("RIDDLE_KEEP_PAGE").is_none() {
                            let _ = std::fs::remove_file(PNG_PATH);
                        }
                        let region = user_ink.bbox;
                        State::Drinking { stage: 0, next: Instant::now(), region, rx }
                    }
                }
                _ => State::Listening { last_pen },
            },

            State::Drinking { stage, next, region, rx } => {
                const STAGES: u32 = 14;
                if Instant::now() >= next {
                    ink::dissolve_pass(&mut surf, region, stage, STAGES);
                    let (x, y, w, h) = region.rect();
                    disp.update(x, y, w, h, true);
                    if stage + 1 >= STAGES {
                        user_ink.clear();
                        State::Thinking { rx, pulse: Instant::now(), blot_on: false, since: Instant::now(), wrote: region }
                    } else {
                        State::Drinking { stage: stage + 1, next: Instant::now() + Duration::from_millis(70), region, rx }
                    }
                } else {
                    State::Drinking { stage, next, region, rx }
                }
            }

            State::Thinking { rx, pulse, blot_on, since, wrote } => match rx.try_recv() {
                Ok(result) => {
                    surf.fill_rect((THINK_X - 14) as usize, (THINK_Y - 14) as usize, 28, 28, WHITE);
                    disp.update(THINK_X - 14, THINK_Y - 14, 28, 28, true);
                    // First streamed event: start writing now; keep the
                    // receiver so the rest of the reply can append itself.
                    match result {
                        Ok(Event::Show(id)) => {
                            // An incantation: the rest of this turn is the
                            // conjured memory, not a reply. (rx drops here.)
                            match conjure(&font, &store, id, &mut surf, &disp) {
                                Some(st) => st,
                                None => {
                                    eprintln!("g-pad: memory {id} is missing");
                                    let y = prepare_reply_anchor(wrote, &mut surf, &disp);
                                    let plan = plan_reply(&font, &oracle_excuse("lost page"), Some(y));
                                    turn_failed = true;
                                    State::Replying { plan, next: Instant::now(), rx: None }
                                }
                            }
                        }
                        Ok(Event::Ink(text)) => {
                            turn_reply.push_str(&text);
                            let y = prepare_reply_anchor(wrote, &mut surf, &disp);
                            let plan = plan_reply(&font, &text, Some(y));
                            State::Replying { plan, next: Instant::now(), rx: Some(rx) }
                        }
                        Ok(Event::Transcript(t)) => {
                            // Transcript with no prose (model skipped the
                            // reply): remember the words, keep waiting.
                            turn_transcript = Some(t);
                            State::Thinking { rx, pulse, blot_on, since, wrote }
                        }
                        Err(e) => {
                            eprintln!("g-pad: oracle failed: {e}");
                            turn_failed = true;
                            let y = prepare_reply_anchor(wrote, &mut surf, &disp);
                            let plan = plan_reply(&font, &oracle_excuse(&e), Some(y));
                            State::Replying { plan, next: Instant::now(), rx: None }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if since.elapsed() >= ORACLE_PATIENCE {
                        // The oracle never answered (stalled stream, dead pi):
                        // stop pulsing and say so instead of thinking forever.
                        eprintln!("g-pad: oracle timed out after {}s", ORACLE_PATIENCE.as_secs());
                        surf.fill_rect((THINK_X - 14) as usize, (THINK_Y - 14) as usize, 28, 28, WHITE);
                        disp.update(THINK_X - 14, THINK_Y - 14, 28, 28, true);
                        let y = prepare_reply_anchor(wrote, &mut surf, &disp);
                        let plan = plan_reply(&font, &oracle_excuse("timed out"), Some(y));
                        State::Replying { plan, next: Instant::now(), rx: None }
                    } else if pulse.elapsed() >= Duration::from_millis(600) {
                        let (cx, cy) = (THINK_X, THINK_Y);
                        if blot_on {
                            surf.fill_rect(cx as usize - 14, cy as usize - 14, 28, 28, WHITE);
                        } else {
                            surf.stamp(cx, cy, 9, BLACK);
                        }
                        disp.update(cx - 14, cy - 14, 28, 28, true);
                        State::Thinking { rx, pulse: Instant::now(), blot_on: !blot_on, since, wrote }
                    } else {
                        State::Thinking { rx, pulse, blot_on, since, wrote }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => State::Listening { last_pen: None },
            },

            State::Replying { mut plan, next, mut rx } => {
                // More of the reply may still be streaming in: append each
                // new chunk below what is already planned, mid-animation.
                if let Some(ref r) = rx {
                    let drop_rx = match r.try_recv() {
                        Ok(Ok(Event::Ink(more))) => {
                            turn_reply.push_str(" ");
                            turn_reply.push_str(&more);
                            if !plan.leftover.is_empty() {
                                plan.leftover.push(' ');
                                plan.leftover.push_str(&more);
                            } else {
                                append_reply(&font, &mut plan, &more);
                            }
                            false
                        }
                        Ok(Ok(Event::Transcript(t))) => {
                            turn_transcript = Some(t);
                            false // the disconnect is still coming
                        }
                        Ok(Ok(Event::Show(_))) => {
                            eprintln!("g-pad: conjuring directive mid-reply ignored");
                            false
                        }
                        Ok(Err(e)) => {
                            eprintln!("g-pad: oracle failed mid-reply: {e}");
                            turn_failed = true;
                            true
                        }
                        Err(mpsc::TryRecvError::Disconnected) => true,
                        Err(mpsc::TryRecvError::Empty) => false,
                    };
                    if drop_rx {
                        rx = None;
                    }
                }
                if Instant::now() >= next {
                    let mut dirty = BBox::empty();
                    let mut budget = reply_points;
                    while budget > 0 && plan.stroke_i < plan.strokes.len() {
                        let stroke = &plan.strokes[plan.stroke_i];
                        if plan.point_i >= stroke.len() {
                            plan.stroke_i += 1;
                            plan.point_i = 0;
                            continue;
                        }
                        let (x, y) = stroke[plan.point_i];
                        if plan.point_i > 0 {
                            let (px, py) = stroke[plan.point_i - 1];
                            surf.brush_line(px, py, x, y, reply_w, BLACK);
                        } else {
                            surf.stamp(x, y, reply_w, BLACK);
                        }
                        dirty.add(x, y, reply_w + 2);
                        plan.point_i += 1;
                        budget -= 1;
                    }
                    if !dirty.is_empty() {
                        let (x, y, w, h) = dirty.rect();
                        disp.update(x, y, w, h, true);
                    }
                    if plan.stroke_i >= plan.strokes.len() && rx.is_none() {
                        // The turn is complete: the diary remembers it.
                        if !turn_failed && !turn_reply.is_empty() {
                            if let Some(ref mut s) = store {
                                s.append(
                                    turn_id,
                                    turn_transcript.as_deref().unwrap_or(""),
                                    turn_reply.trim(),
                                    &turn_strokes,
                                );
                            }
                        }
                        turn_strokes = Vec::new();
                        if !plan.shown.is_empty() {
                            reply_pages.push(plan.shown.clone());
                            reply_page = reply_pages.len() - 1;
                        }
                        let region = plan.region;
                        let more = plan.leftover;
                        State::Lingering { region, more }
                    } else {
                        State::Replying { plan, next: Instant::now() + Duration::from_millis(14), rx }
                    }
                } else {
                    State::Replying { plan, next, rx }
                }
            }

            State::Lingering { region, more } => State::Lingering { region, more },

            State::Help { panel, until } => match panel {
                Some(p) => {
                    if stylus_tapped || Instant::now() >= until {
                        let region = p.dismiss(&mut surf);
                        let (x, y, w, h) = region.rect();
                        disp.update(x, y, w, h, false);
                        eprintln!("g-pad: guide dismissed");
                        State::Help { panel: None, until }
                    } else {
                        State::Help { panel: Some(p), until }
                    }
                }
                // Dismissed: swallow the closing touch, listen again on pen-up.
                None if stylus_on => State::Help { panel: None, until },
                None => State::Listening { last_pen: None },
            },

            State::Conjuring { mut plan, next, saved } => {
                if stylus_tapped {
                    // The writer interrupts: today's page returns at once.
                    surf.paste_rect(0, 0, SCREEN_W, SCREEN_H, &saved);
                    disp.full_refresh(surf.w, surf.h);
                    State::MemoryShown { saved: None, until: Instant::now(), region: plan.region }
                } else if Instant::now() >= next {
                    // The memory pours back faster than Tom writes: it is
                    // remembered, not composed.
                    let mut dirty = BBox::empty();
                    let mut budget = 48;
                    while budget > 0 && plan.stroke_i < plan.strokes.len() {
                        let stroke = &plan.strokes[plan.stroke_i];
                        if plan.point_i >= stroke.len() {
                            plan.stroke_i += 1;
                            plan.point_i = 0;
                            continue;
                        }
                        let (x, y, r) = stroke[plan.point_i];
                        if plan.point_i > 0 {
                            let (px, py, pr) = stroke[plan.point_i - 1];
                            surf.brush_line(px, py, x, y, r.min(pr + 1), FADED);
                        } else {
                            surf.stamp(x, y, r, FADED);
                        }
                        dirty.add(x, y, r + 2);
                        plan.point_i += 1;
                        budget -= 1;
                    }
                    if !dirty.is_empty() {
                        let (x, y, w, h) = dirty.rect();
                        disp.update(x, y, w, h, true);
                    }
                    if plan.stroke_i >= plan.strokes.len() {
                        let region = plan.region;
                        State::MemoryShown {
                            saved: Some(saved),
                            until: Instant::now() + Duration::from_secs(120),
                            region,
                        }
                    } else {
                        State::Conjuring { plan, next: Instant::now() + Duration::from_millis(10), saved }
                    }
                } else {
                    State::Conjuring { plan, next, saved }
                }
            }

            State::MemoryShown { saved, until, region } => match saved {
                Some(s) => {
                    if stylus_tapped || Instant::now() >= until {
                        // The paper swallows its memory; today's page returns.
                        surf.paste_rect(0, 0, SCREEN_W, SCREEN_H, &s);
                        disp.full_refresh(surf.w, surf.h);
                        eprintln!("g-pad: memory dismissed");
                        State::MemoryShown { saved: None, until, region }
                    } else {
                        State::MemoryShown { saved: Some(s), until, region }
                    }
                }
                // Dismissed: swallow the closing touch, listen again on pen-up.
                None if stylus_on => State::MemoryShown { saved: None, until, region },
                None => State::Listening { last_pen: None },
            },

            State::Drawer { panel, return_to } => match panel {
                Some(p) => State::Drawer { panel: Some(p), return_to },
                None if stylus_on => State::Drawer { panel: None, return_to },
                None => *return_to,
            },
            State::ExpandedConversation { panel, return_to } => match panel {
                Some(p) => State::ExpandedConversation { panel: Some(p), return_to },
                None if stylus_on => State::ExpandedConversation { panel: None, return_to },
                None => *return_to,
            },
            State::Settings { saved, return_to } => match saved {
                Some(s) => State::Settings { saved: Some(s), return_to },
                None if stylus_on => State::Settings { saved: None, return_to },
                None => *return_to,
            },

            State::FadingReply { stage, next, region } => {
                const STAGES: u32 = 10;
                if Instant::now() >= next {
                    ink::dissolve_pass(&mut surf, region, stage, STAGES);
                    let (x, y, w, h) = region.rect();
                    disp.update(x, y, w, h, true);
                    if stage + 1 >= STAGES {
                        disp.full_refresh(surf.w, surf.h);
                        State::Listening { last_pen: None }
                    } else {
                        State::FadingReply { stage: stage + 1, next: Instant::now() + Duration::from_millis(80), region }
                    }
                } else {
                    State::FadingReply { stage, next, region }
                }
            }
        };

        stylus_tapped = false;
        std::thread::sleep(Duration::from_millis(2));
    }

    eprintln!("g-pad: the pad closes");
    disp.terminate();
    Ok(())
}

fn memory_turns() -> usize {
    std::env::var("RIDDLE_MEMORY_TURNS").ok().and_then(|v| v.parse().ok()).unwrap_or(6)
}

const TOP_WRITING_LINE: i32 = 144;
const GRID_LINE: i32 = 216;
/// Room a below-writing reply needs before we give up and use the top line.
const BELOW_WRITING_MIN_ROOM: i32 = 900;
/// Gap between the drunk ink and a below-writing reply.
const BELOW_WRITING_GAP: i32 = 48;

/// Replies always start at the top writing line so a normal-length answer
/// fits. The drink already took the writer's ink; erase that ghost locally
/// (no full-page flash) so Tom is not writing through it.
fn prepare_reply_anchor(wrote: BBox, surf: &mut Surface, disp: &display::Display) -> i32 {
    if !wrote.is_empty() {
        let (x, y, w, h) = wrote.rect();
        surf.fill_rect(x as usize, y as usize, w as usize, h as usize, WHITE);
        disp.update(x, y, w, h, true);
    }
    reply_anchor(wrote)
}

fn reply_anchor(_wrote: BBox) -> i32 {
    TOP_WRITING_LINE
}

/// Parked: first grid line below the writer's ink, or the top writing line if
/// a useful reply cannot fit. For a future mode where the entry and Tom's
/// reply share the page instead of the reply taking the whole sheet.
#[allow(dead_code)]
fn reply_below_writing(wrote: BBox) -> (i32, bool) {
    if wrote.is_empty() { return (TOP_WRITING_LINE, false); }
    let below = wrote.y1 + BELOW_WRITING_GAP;
    let anchored = ((below + GRID_LINE - 1) / GRID_LINE) * GRID_LINE;
    if anchored > SCREEN_H as i32 - BELOW_WRITING_MIN_ROOM {
        (TOP_WRITING_LINE, true)
    } else {
        (anchored.max(TOP_WRITING_LINE), false)
    }
}

fn close_overlay(state: &mut State, surf: &mut Surface, disp: &display::Display,
    selection: &mut Option<usize>, scroll: &mut i32) {
    let old = std::mem::replace(state, State::Listening { last_pen: None });
    match old {
        State::Drawer { panel: Some(p), return_to } | State::ExpandedConversation { panel: Some(p), return_to } => {
            *selection = p.selection; *scroll = p.scroll;
            let region = p.close(surf); let (x, y, w, h) = region.rect(); disp.update(x, y, w, h, false);
            *state = *return_to;
        }
        State::Settings { saved: Some(bytes), return_to } => {
            surf.paste_rect(0, 0, ui::PANEL_W, SCREEN_H, &bytes);
            disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false); *state = *return_to;
        }
        other => *state = other,
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_control(action: ui::Action, state: &mut State, surf: &mut Surface, disp: &display::Display,
    ui_font: &FontRef, store: &Option<memory::MemoryStore>, user_ink: &mut ink::Ink,
    send_mode: &mut Option<CommitMode>, sleep_requested: &mut bool,
    prefs: &mut preferences::Preferences, idle_commit: &mut Duration,
    selection: Option<usize>, scroll: i32) {
    match action {
        ui::Action::Send => *send_mode = Some(CommitMode::Capture),
        ui::Action::Erase | ui::Action::NewPage => {
            surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE); disp.full_refresh(surf.w, surf.h);
            user_ink.clear(); *state = State::Listening { last_pen: None };
        }
        ui::Action::Dismiss => {
            if let State::Lingering { region, .. } = state {
                let region = *region;
                *state = State::FadingReply { stage: 0, next: Instant::now(), region };
            }
        }
        ui::Action::History | ui::Action::Corpus | ui::Action::Sessions => {
            if matches!(state, State::Listening { .. } | State::Lingering { .. }) {
                let old = std::mem::replace(state, State::Listening { last_pen: None });
                let kind = match action {
                    ui::Action::History => ui::DrawerKind::History,
                    ui::Action::Sessions => ui::DrawerKind::Sessions,
                    _ => ui::DrawerKind::Corpus,
                };
                let thread = if kind == ui::DrawerKind::History {
                    store.as_ref().and_then(|s| s.conversations().len().checked_sub(1))
                } else { None };
                let panel = ui::Drawer::open(surf, kind, selection, scroll, thread);
                let snapshot = oracle::context_snapshot(store, memory_turns());
                ui::draw_drawer(surf, ui_font, store, &snapshot, &panel);
                disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
                *state = State::Drawer { panel: Some(panel), return_to: Box::new(old) };
            }
        }
        ui::Action::Settings => {
            if matches!(state, State::Listening { .. } | State::Lingering { .. }) {
                let old = std::mem::replace(state, State::Listening { last_pen: None });
                let saved = ui::draw_settings(surf, ui_font, *prefs);
                disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
                *state = State::Settings { saved: Some(saved), return_to: Box::new(old) };
            }
        }
        ui::Action::Sleep => *sleep_requested = true,
        ui::Action::SetMode(mode) => { prefs.mode = mode; let _ = prefs.save(); }
        ui::Action::ToggleIdle => {
            prefs.idle_send_ms = if prefs.idle_send_ms == 0 { 2800 } else { 0 };
            *idle_commit = Duration::from_millis(prefs.idle_send_ms); let _ = prefs.save();
        }
        _ => {}
    }
}

fn handle_drawer_action(action: ui::Action, state: &mut State, surf: &mut Surface,
    disp: &display::Display, ui_font: &FontRef, reply_font: &FontRef, store: &Option<memory::MemoryStore>,
    selection: &mut Option<usize>, scroll: &mut i32) {
    if action == ui::Action::Close {
        close_overlay(state, surf, disp, selection, scroll); return;
    }
    if let ui::Action::Replay(id) = action {
        close_overlay(state, surf, disp, selection, scroll);
        if let Some(next) = conjure(reply_font, store, id, surf, disp) { *state = next; }
        return;
    }
    let mut redraw = false;
    if let State::Drawer { panel: Some(p), .. } | State::ExpandedConversation { panel: Some(p), .. } = state {
        match action {
            ui::Action::History => {
                if p.kind == ui::DrawerKind::History && p.thread.is_some() {
                    p.thread = None; p.scroll = 0; p.selection = None;
                } else {
                    p.kind = ui::DrawerKind::History;
                    p.thread = store.as_ref().and_then(|s| s.conversations().len().checked_sub(1));
                    p.scroll = 0;
                }
                redraw = true;
            }
            ui::Action::Threads => { p.thread = None; p.scroll = 0; p.selection = None; redraw = true; }
            ui::Action::OpenThread(i) => { p.thread = Some(i); p.scroll = 0; p.selection = None; redraw = true; }
            ui::Action::Corpus => { p.kind = ui::DrawerKind::Corpus; p.scroll = 0; redraw = true; }
            ui::Action::Sessions => { p.kind = ui::DrawerKind::Sessions; p.scroll = 0; redraw = true; }
            ui::Action::None => redraw = true,
            _ => {}
        }
        *selection = p.selection; *scroll = p.scroll;
        if redraw {
            let snapshot = oracle::context_snapshot(store, memory_turns());
            ui::draw_drawer(surf, ui_font, store, &snapshot, p);
            disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
        }
    }
}

/// If the most recent stroke is the "send rule" (a long flat line ruled under
/// the words, like signing off a diary entry), absorb it — erase it from the
/// page and drop it from the ink — and report that the user asked to send.
/// The rule must span ~60% of the width of what's written (short note, short
/// rule), with an absolute floor so a stray dash under one word doesn't send.
fn absorb_send_rule(ink: &mut ink::Ink, surf: &mut Surface, disp: &display::Display) -> Option<CommitMode> {
    let strokes = ink.stroke_list();
    if strokes.len() < 2 {
        return None;
    }
    let mut text = BBox::empty();
    for s in &strokes[..strokes.len() - 1] {
        for &(x, y, r) in s {
            text.add(x, y, r);
        }
    }
    let text_w = (text.x1 - text.x0).max(0);
    let min_w = (text_w * 3 / 5).max(SCREEN_W as i32 * 3 / 20);
    let mode = strokes.last().and_then(|stroke| {
        if help::looks_like_ask_arrow(stroke, min_w) {
            Some(CommitMode::Ask)
        } else if help::looks_like_send_rule(stroke, min_w) {
            Some(CommitMode::Capture)
        } else {
            None
        }
    });
    if mode.is_none() {
        return None;
    }
    if let Some(gone) = ink.pop_stroke() {
        let (x, y, w, h) = gone.rect();
        surf.fill_rect(x.max(0) as usize, y.max(0) as usize, w as usize, h as usize, WHITE);
        disp.update(x, y, w, h, true);
        eprintln!("g-pad: {} gesture drawn", if matches!(mode, Some(CommitMode::Ask)) { "ask" } else { "capture" });
        return mode;
    }
    None
}

/// True if the region no longer holds any dark pixels (fully erased).
fn region_all_white(surf: &Surface, region: BBox) -> bool {
    if region.is_empty() {
        return true;
    }
    for y in region.y0..=region.y1 {
        for x in region.x0..=region.x1 {
            if surf.luma(x, y) < 200 {
                return false;
            }
        }
    }
    true
}

/// What the pad writes when the oracle cannot answer: short, specific enough
/// to act on. The raw error still goes to stderr.
fn oracle_excuse(e: &str) -> String {
    if e.contains("no oracle") {
        "The pad is quiet: it found no oracle. \
         Put an API key in oracle.env, then open me again."
            .into()
    } else if e.starts_with("http 401") || e.starts_with("http 403") {
        "The oracle refused the pad's key. Check RIDDLE_OPENAI_KEY in oracle.env.".into()
    } else if e.starts_with("http ") {
        let code = e.split(':').next().unwrap_or("an error");
        format!("The oracle rejected the request ({code}). Check the model and endpoint in oracle.env.")
    } else if e.contains("request failed") || e.contains("timed out") {
        "The pad cannot reach its oracle. Is the tablet connected to Wi-Fi?".into()
    } else if e.contains("empty reply") {
        "The pad read your words but said nothing. Write again.".into()
    } else {
        "The ink blurred before it could answer. Write again.".into()
    }
}

/// Summon a remembered page: snapshot today's page, clear the paper, and plan
/// the memory's rewriting — the date in a small hand, the writer's own strokes
/// exactly as they were penned, Tom's old reply beneath — all in faded ink.
fn conjure(
    font: &FontRef,
    store: &Option<memory::MemoryStore>,
    id: u64,
    surf: &mut Surface,
    disp: &display::Display,
) -> Option<State> {
    let s = store.as_ref()?;
    let entry = s.get(id)?.clone();
    let strokes = s.strokes(id).unwrap_or_default();
    eprintln!("g-pad: conjuring memory {id} ({})", memory::spoken_date(id));

    let saved = surf.copy_rect(0, 0, SCREEN_W, SCREEN_H);
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    disp.update_all(surf.w, surf.h);

    let mut all: Vec<Vec<(i32, i32, i32)>> = Vec::new();
    let mut region = BBox::empty();

    // The date, small and centered near the top, like a diary heading.
    let date = memory::spoken_date(entry.id);
    let mut raster = script::rasterize_line(font, &date, 54.0);
    script::thin(&mut raster);
    let x0 = (SCREEN_W as i32 - raster.width as i32) / 2;
    let mut ink_bottom = 64;
    for stroke in script::trace(&raster) {
        let mapped: Vec<(i32, i32, i32)> =
            stroke.iter().map(|&(sx, sy)| (x0 + sx, 64 + sy, 1)).collect();
        for &(x, y, r) in &mapped {
            region.add(x, y, r + 2);
            ink_bottom = ink_bottom.max(y);
        }
        all.push(mapped);
    }

    // The writer's own hand, exactly as it was penned.
    for stroke in &strokes {
        for &(x, y, r) in stroke {
            region.add(x, y, r + 2);
            ink_bottom = ink_bottom.max(y);
        }
        all.push(stroke.clone());
    }

    // Tom's old reply, below.
    if !entry.reply.is_empty() {
        let y = (ink_bottom + 130).min(SCREEN_H as i32 - 400);
        let reply = plan_reply(font, &entry.reply, Some(y));
        for stroke in reply.strokes {
            let mapped: Vec<(i32, i32, i32)> = stroke.iter().map(|&(x, y)| (x, y, 2)).collect();
            for &(x, y, r) in &mapped {
                region.add(x, y, r + 2);
            }
            all.push(mapped);
        }
    }

    Some(State::Conjuring {
        plan: ConjurePlan { strokes: all, stroke_i: 0, point_i: 0, region },
        next: Instant::now(),
        saved,
    })
}

/// Lay out reply text and produce screen-space strokes. `y_start` continues a
/// streamed reply below its previous chunk; None places the first chunk.
fn plan_reply(font: &FontRef, text: &str, y_start: Option<i32>) -> WritePlan {
    let max_w = (SCREEN_W as i32 - 2 * MARGIN_X) as f32;
    let lines = script::wrap(font, text, REPLY_PX, max_w);
    let line_h = (REPLY_PX * 1.25) as i32;
    let mut y = y_start.unwrap_or(TOP_WRITING_LINE);
    let mut strokes = Vec::new();
    let mut region = BBox::empty();
    let mut seed = 0x1234u32;
    let mut jitter = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        ((seed >> 16) % 7) as i32 - 3
    };

    let y_limit = SCREEN_H as i32 - line_h;
    let mut leftover = String::new();
    let mut shown_lines: Vec<String> = Vec::new();
    for (idx, line_text) in lines.iter().enumerate() {
        if y > y_limit {
            leftover = lines[idx..].join(" ");
            break;
        }
        shown_lines.push(line_text.clone());
        let mut raster = script::rasterize_line(font, line_text, REPLY_PX);
        script::thin(&mut raster);
        let line_strokes = script::trace(&raster);
        let x0 = MARGIN_X;
        let wobble = jitter();
        for s in line_strokes {
            let mapped: Vec<(i32, i32)> = s.iter().map(|&(sx, sy)| (x0 + sx, y + sy + wobble)).collect();
            for &(x, yy) in &mapped {
                region.add(x, yy, 5);
            }
            strokes.push(mapped);
        }
        y += line_h;
    }

    WritePlan { strokes, stroke_i: 0, point_i: 0, region, next_y: y, leftover,
        shown: shown_lines.join(" ") }
}

/// Splice a streamed continuation chunk into a running write animation.
fn append_reply(font: &FontRef, plan: &mut WritePlan, more: &str) {
    let cont = plan_reply(font, more, Some(plan.next_y));
    if !cont.leftover.is_empty() {
        if !plan.leftover.is_empty() { plan.leftover.push(' '); }
        plan.leftover.push_str(&cont.leftover);
    }
    if !cont.shown.is_empty() {
        if !plan.shown.is_empty() { plan.shown.push(' '); }
        plan.shown.push_str(&cont.shown);
    }
    if cont.strokes.is_empty() {
        return;
    }
    plan.region.add(cont.region.x0, cont.region.y0, 0);
    plan.region.add(cont.region.x1, cont.region.y1, 0);
    plan.strokes.extend(cont.strokes);
    plan.next_y = cont.next_y;
}

fn continue_reply(font: &FontRef, leftover: String, state: &mut State, surf: &mut Surface, disp: &display::Display) {
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    disp.full_refresh(surf.w, surf.h);
    let plan = plan_reply(font, leftover.trim(), Some(TOP_WRITING_LINE));
    *state = State::Replying { plan, next: Instant::now(), rx: None };
}

fn paint_reply_page(font: &FontRef, text: &str, reply_w: i32, surf: &mut Surface, disp: &display::Display) -> BBox {
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    disp.full_refresh(surf.w, surf.h);
    let plan = plan_reply(font, text, Some(TOP_WRITING_LINE));
    for stroke in &plan.strokes {
        for (i, &(x, y)) in stroke.iter().enumerate() {
            if i == 0 {
                surf.stamp(x, y, reply_w, BLACK);
            } else {
                let (px, py) = stroke[i - 1];
                surf.brush_line(px, py, x, y, reply_w, BLACK);
            }
        }
    }
    if !plan.region.is_empty() {
        let (x, y, w, h) = plan.region.rect();
        disp.update(x, y, w, h, true);
    }
    plan.region
}

fn step_reply_page(dir: i32, font: &FontRef, reply_w: i32, pages: &mut Vec<String>, index: &mut usize,
    state: &mut State, surf: &mut Surface, disp: &display::Display) -> bool
{
    let more = match state {
        State::Lingering { more, .. } => more.clone(),
        _ => return false,
    };
    if dir > 0 {
        if *index + 1 < pages.len() {
            *index += 1;
            let region = paint_reply_page(font, &pages[*index], reply_w, surf, disp);
            *state = State::Lingering { region, more };
            return true;
        }
        if !more.is_empty() {
            continue_reply(font, more, state, surf, disp);
            return true;
        }
        return false;
    }
    if dir < 0 && *index > 0 {
        *index -= 1;
        let region = paint_reply_page(font, &pages[*index], reply_w, surf, disp);
        *state = State::Lingering { region, more };
        return true;
    }
    false
}

#[cfg(test)]
mod ux_tests {
    use super::*;

    #[test]
    fn reply_starts_at_the_top_writing_line() {
        let mut wrote = BBox::empty(); wrote.add(100, 500, 0); wrote.add(300, 700, 0);
        assert_eq!(reply_anchor(wrote), TOP_WRITING_LINE);

        let mut low = BBox::empty(); low.add(100, SCREEN_H as i32 - 200, 0);
        assert_eq!(reply_anchor(low), TOP_WRITING_LINE);
        assert_eq!(reply_anchor(BBox::empty()), TOP_WRITING_LINE);
    }

    #[test]
    fn parked_below_writing_offset_keeps_entry_and_reply_on_the_page() {
        let mut wrote = BBox::empty(); wrote.add(100, 500, 0); wrote.add(300, 700, 0);
        let (y, refresh) = reply_below_writing(wrote);
        assert_eq!(y % GRID_LINE, 0);
        assert!(y > 700);
        assert!(!refresh);

        let mut low = BBox::empty(); low.add(100, SCREEN_H as i32 - 200, 0);
        assert_eq!(reply_below_writing(low), (TOP_WRITING_LINE, true));
        assert_eq!(reply_below_writing(BBox::empty()), (TOP_WRITING_LINE, false));
    }

    #[test]
    fn long_reply_keeps_unfitted_lines_for_the_next_page() {
        let font = FontRef::try_from_slice(FONT_TTF).unwrap();
        let text = "word ".repeat(400);
        let plan = plan_reply(&font, &text, Some(TOP_WRITING_LINE));
        assert!(!plan.leftover.is_empty(), "a long reply must leave leftover text");
        assert!(!plan.shown.is_empty());
        assert!(plan.next_y <= SCREEN_H as i32);
        let again = plan_reply(&font, &plan.leftover, Some(TOP_WRITING_LINE));
        assert!(!again.strokes.is_empty(), "leftover must be writable on a fresh page");
    }
}
