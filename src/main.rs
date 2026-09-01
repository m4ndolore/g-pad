//! g-pad — an e-ink writing pad for the reMarkable 2.
//!
//! Write on the page with the pen. Rule a line beneath the entry and the pad
//! reads your ink; an answer writes itself in a flowing hand and remains.
//!
//! Two display backends (picked at runtime): windowed via qtfb/AppLoad when
//! QTFB_KEY is set, or full takeover via the vendor engine (quill) when
//! built with --features takeover and launched with xochitl stopped.

mod ask;
// Laid out and tested, but not yet wired to a DrawerKind: the brief is a
// reading surface waiting on a call site, not dead code. See
// docs/daily-brief.md. Its JSON scanner is shared with the Claude bridge.
#[allow(dead_code)]
mod brief;
// Agent mode: the AGENTS tab is the board, a tapped row opens the full turn
// page, and a poll thread feeds both when RIDDLE_BRIDGE_URL names a hub. See
// docs/claude-bridge.md and docs/plans/2026-08-30-anthink-hub-design.md.
#[allow(dead_code)]
mod bridge;
mod display;
mod evdev;
mod fb;
mod gesture;
mod ink;
mod learn;
mod memory;
mod notebook;
mod page;
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

// Replies default to the same legible sans the chrome uses; the cursive
// hand survives only as an opt-in via RIDDLE_FONT_FILE.
const FONT_TTF: &[u8] = include_bytes!("../fonts/LiberationSans-Regular.ttf");
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
  g-pad --learn-sheets [DIR]  render sample Learn-mode worksheets for every
                              level into DIR (default /tmp/learn-sheets) as
                              PNGs; no display or oracle needed
  g-pad --learn-test [ANS]    one Learn-mode tutor round trip with a simulated
                              child answer (default: the correct one); prints
                              the verdict; verifies key + endpoint + model
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
    /// Learn mode: the child marked DONE and the tutor is reading the answer
    /// region. `got` accumulates the streamed reply until it can be read as a
    /// verdict; the child's ink stays on the page throughout.
    LearnMarking { rx: OracleRx, pulse: Instant, blot_on: bool, since: Instant, got: String },
    FadingReply { stage: u32, next: Instant, region: BBox },
    /// The guide panel. `panel: None` = dismissed, waiting for pen-up so the
    /// dismissing touch doesn't leave a mark on the page.
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
    /// One agent session read full-page (the turn page). `saved` is the whole
    /// canvas underneath. Touch acts only on named targets: ← AGENTS returns
    /// to the board, × (or the leftward swipe) closes to the canvas, the
    /// vertical swipe flips pages, and the rest of the page ignores fingers —
    /// the pen alone commands. `boxr` is the hit map drawing returned;
    /// `armed` is the destructive-confirmation state (first tick arms, second
    /// sends); `status` is the last nudge's outcome; `page` is which page is
    /// open — 0 the newest, the swipe pages backward.
    SessionPage {
        session: bridge::Session,
        remaining: usize,
        stale: bool,
        armed: bool,
        status: Option<String>,
        boxr: Option<ui::DecisionBox>,
        page: usize,
        saved: Vec<u8>,
        return_to: Box<State>,
    },
}

/// What a pen stroke on the turn page meant, once anchored to the hit map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageMark {
    Tick,
    Strike,
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
        // Diagnostic: render sample Learn worksheets to PNGs so the sheets can
        // be reviewed (and shown to a parent) without a tablet in hand.
        Some("--learn-sheets") => {
            let dir = args.get(2).map(String::as_str).unwrap_or("/tmp/learn-sheets");
            std::process::exit(learn_sheets(dir));
        }
        // Diagnostic: one full tutor round trip with a simulated child answer
        // — draws a number bond, writes ANSWER into the blank in the reply
        // hand, sends the answer region, prints the verdict. Verifies the
        // whole Learn pipeline short of a pen. Needs oracle credentials.
        Some("--learn-test") => {
            let answer = args.get(2).map(String::as_str);
            std::process::exit(learn_test(answer));
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

/// Render four sample sheets per level into `dir`: the Learn-mode preview.
fn learn_sheets(dir: &str) -> i32 {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("g-pad: cannot create {dir}: {e}");
        return 1;
    }
    let Ok(ui_font) = FontRef::try_from_slice(ui::UI_FONT_TTF) else {
        eprintln!("g-pad: bundled UI font unreadable");
        return 1;
    };
    let mut buf = vec![0xFFu8; SCREEN_W * SCREEN_H * 4];
    let ptr = buf.as_mut_ptr();
    let mut surf = Surface::new(ptr, buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, surface::PixFmt::Rgb32);
    for level in 1..=4u8 {
        let mut session = learn::Session::start_at(level, 41 + level as u32);
        for page in 0..4 {
            session.draw(&mut surf, &ui_font);
            let path = format!("{dir}/learn-L{level}-{page}.png");
            if let Err(e) = dump_page(&surf, &path) {
                eprintln!("g-pad: write {path}: {e}");
                return 1;
            }
            println!("{path}");
            session.next();
        }
    }
    // The menu: the picker every page's MENU box deals.
    let mut session = learn::Session::start_at(1, 99);
    session.open_menu();
    session.draw(&mut surf, &ui_font);
    let path = format!("{dir}/learn-menu.png");
    if let Err(e) = dump_page(&surf, &path) {
        eprintln!("g-pad: write {path}: {e}");
        return 1;
    }
    println!("{path}");
    // The play pages: each game's opening sheet, plus a story mid-beat.
    let mut session = learn::Session::start_at(1, 99);
    let pages: [(&str, learn::Page); 4] = [
        ("critter", learn::Page::Play(learn::games::Game::Critter { round: 0 })),
        ("guess", learn::Page::Play(learn::games::Game::Guess)),
        ("story-start", learn::Page::Play(learn::games::Game::story())),
        (
            "story-beat",
            learn::Page::Play(learn::games::Game::Story {
                log: vec![(String::new(), "A brave sock puppet set out at dawn.".into())],
                choices: vec!["EAT IT".into(), "RUN AWAY".into(), "ASK NICELY".into()],
                pending: None,
            }),
        ),
    ];
    for (name, page) in pages {
        session.page = page;
        session.draw(&mut surf, &ui_font);
        let path = format!("{dir}/play-{name}.png");
        if let Err(e) = dump_page(&surf, &path) {
            eprintln!("g-pad: write {path}: {e}");
            return 1;
        }
        println!("{path}");
    }
    // One critter mid-game, so the pad's turns can be judged: a child-ish
    // blob wearing three of the pad's decorations.
    session.page = learn::Page::Play(learn::games::Game::Critter { round: 3 });
    session.draw(&mut surf, &ui_font);
    let (cx, cy, r0) = (SCREEN_W as i32 / 2, SCREEN_H as i32 * 2 / 5, 220.0f32);
    let mut prev: Option<(i32, i32)> = None;
    for i in 0..=72 {
        let a = i as f32 * std::f32::consts::TAU / 72.0;
        let r = r0 + (a * 5.0).sin() * 26.0; // a wobbly hand-drawn circle
        let (x, y) = (cx + (r * a.cos()) as i32, cy + (r * a.sin()) as i32);
        if let Some((px, py)) = prev {
            surf.brush_line(px, py, x, y, 4, BLACK);
        }
        prev = Some((x, y));
    }
    let mut blob = BBox::empty();
    blob.add(cx - 250, cy - 250, 0);
    blob.add(cx + 250, cy + 250, 0);
    for deco in [
        learn::games::Deco::Eyes,
        learn::games::Deco::Hat,
        learn::games::Deco::Legs,
        learn::games::Deco::Bubble("MOO".into()),
    ] {
        learn::games::draw_deco(&mut surf, &blob, &deco, &ui_font);
    }
    let path = format!("{dir}/play-critter-demo.png");
    if let Err(e) = dump_page(&surf, &path) {
        eprintln!("g-pad: write {path}: {e}");
        return 1;
    }
    println!("{path}");
    // The three verdict stamps a child actually sees, on a real sheet.
    let mut session = learn::Session::start_at(2, 7);
    let samples: [(&str, &str, &str, bool); 3] = [
        ("yes", "GREAT JOB!", "", true),
        ("almost", "SO CLOSE!", "Your 3 is facing the wrong way.", false),
        ("no", "TRY AGAIN!", "Count the dots one by one.", false),
    ];
    for (name, cheer, hint, starred) in samples {
        session.draw(&mut surf, &ui_font);
        let answer = session.hits.answer;
        if starred {
            learn::sheet::draw_check(&mut surf, &answer);
        } else {
            learn::sheet::draw_look_again(&mut surf, &answer);
        }
        learn::sheet::draw_feedback(&mut surf, &ui_font, cheer, hint, starred);
        let path = format!("{dir}/learn-verdict-{name}.png");
        if let Err(e) = dump_page(&surf, &path) {
            eprintln!("g-pad: write {path}: {e}");
            return 1;
        }
        println!("{path}");
        session.next();
    }
    0
}

/// One tutor round trip against a simulated child: draw a bond sheet, write
/// `answer` (default: the correct one) into the blank in the reply hand, send
/// the answer region with the tutor instruction, print verdict + feedback.
fn learn_test(answer: Option<&str>) -> i32 {
    let Ok(ui_font) = FontRef::try_from_slice(ui::UI_FONT_TTF) else {
        eprintln!("g-pad: bundled UI font unreadable");
        return 1;
    };
    let Ok(hand) = FontRef::try_from_slice(FONT_TTF) else {
        eprintln!("g-pad: bundled hand font unreadable");
        return 1;
    };
    let mut buf = vec![0xFFu8; SCREEN_W * SCREEN_H * 4];
    let ptr = buf.as_mut_ptr();
    let mut surf = Surface::new(ptr, buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, surface::PixFmt::Rgb32);
    let mut session = learn::Session::start_at(2, 7);
    session.draw(&mut surf, &ui_font);
    let problem = session.as_practice().expect("start_at deals a practice page").clone();
    let expected = problem.expected();
    let written = answer.unwrap_or(&expected);
    println!("exercise: {}", problem.brief());
    println!("child writes: {written}");

    // "Handwrite" the answer into the blank, tracked as real user ink so the
    // round trip exercises the live ink-only crop path.
    let b = session.hits.answer;
    let mut sim_ink = ink::Ink::new();
    let mut raster = script::rasterize_line(&hand, written, 110.0);
    script::thin(&mut raster);
    let (cx, cy) = ((b.x0 + b.x1) / 2 - raster.width as i32 / 2, (b.y0 + b.y1) / 2 - raster.height as i32 / 2);
    for stroke in script::trace(&raster) {
        for &(sx, sy) in stroke.iter() {
            sim_ink.pen_point(&mut surf, cx + sx, cy + sy, 3);
        }
        sim_ink.pen_up();
    }

    let png = "/tmp/g-pad-learn-test.png";
    if let Err(e) = sim_ink.ink_png(&b, 80, png) {
        eprintln!("g-pad: region png failed: {e}");
        return 1;
    }
    let o = match oracle::Oracle::spawn(false) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("oracle spawn failed: {e}");
            return 1;
        }
    };
    let ctx = oracle::TurnContext { instruction: Some(session.instruction()), ..Default::default() };
    let (tx, rx) = mpsc::channel();
    // Same model choice as the live tutor: never the pad's capture sink.
    let learn_model: Option<String> = std::env::var("RIDDLE_LEARN_MODEL")
        .ok()
        .or_else(|| std::env::var("RIDDLE_OPENAI_ASK_MODEL").ok());
    o.ask_with_model(png, &ctx, tx, learn_model.as_deref());
    let mut got = String::new();
    loop {
        match rx.recv() {
            Ok(Ok(Event::Ink(chunk))) => {
                if !got.is_empty() {
                    got.push(' ');
                }
                got.push_str(&chunk);
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                eprintln!("oracle error: {e}");
                return 1;
            }
            Err(_) => break,
        }
    }
    let (verdict, fb) = learn::verdict::parse(got.trim());
    println!("verdict: {verdict:?}");
    println!("cheer: {}", fb.cheer);
    println!("hint: {}", fb.hint);
    i32::from(verdict == learn::Verdict::Unknown)
}

/// Write the whole page as an 8-bit grayscale PNG (full resolution).
fn dump_page(surf: &Surface, path: &str) -> std::io::Result<()> {
    let mut gray = vec![0u8; surf.w * surf.h];
    for y in 0..surf.h {
        for x in 0..surf.w {
            gray[y * surf.w + x] = surf.luma(x as i32, y as i32);
        }
    }
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), surf.w as u32, surf.h as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer.write_image_data(&gray).map_err(std::io::Error::other)
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
    // absolute path), else the embedded Liberation Sans. Loaded once and
    // leaked — one font per process lifetime.
    let font_bytes: &'static [u8] = match std::env::var("RIDDLE_FONT_FILE") {
        Ok(p) => match std::fs::read(&p) {
            Ok(b) => {
                eprintln!("g-pad: reply font {p}");
                Box::leak(b.into_boxed_slice())
            }
            Err(e) => {
                eprintln!("g-pad: font {p} unreadable ({e}); using Liberation Sans");
                FONT_TTF
            }
        },
        Err(_) => FONT_TTF,
    };
    let font = FontRef::try_from_slice(font_bytes).map_err(std::io::Error::other)?;
    let ui_font = FontRef::try_from_slice(ui::UI_FONT_TTF).map_err(std::io::Error::other)?;

    // Agent sessions arrive by poll, when a hub is configured. Dormant
    // without RIDDLE_BRIDGE_URL.
    bridge::spawn_poll();

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

    // Learn mode: the kids' tutor page. The pad remembers which page it was
    // left on (RIDDLE_PAGE=learn dedicates a boot). See docs/learn-mode.md.
    let mut learn_session: Option<learn::Session> = match prefs.page {
        preferences::Page::Learn => Some(learn::Session::start()),
        preferences::Page::Pad => None,
    };
    if let Some(ref mut session) = learn_session {
        session.draw(&mut surf, &ui_font);
        disp.update_all(surf.w, surf.h);
        eprintln!("g-pad: learn mode open (level {})", session.level());
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
    // Learn mode: latched when a stroke lands in a decision box.
    let mut learn_tick: Option<LearnTick> = None;
    // Learn mode: a YES verdict deals the next page by itself once the
    // feedback has been written and read — no second tap. The dwell leaves
    // time to enjoy the check; pen-down cancels (the child kept writing).
    // RIDDLE_LEARN_NEXT_MS tunes it; 0 turns auto-dealing off.
    let learn_next_dwell: Option<Duration> = match std::env::var("RIDDLE_LEARN_NEXT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(5000)
    {
        0 => None,
        ms => Some(Duration::from_millis(ms)),
    };
    let mut learn_advance_pending = false;
    let mut learn_auto_at: Option<Instant> = None;
    // While the praise is on show, a deliberate pen tap deals the next page at
    // once — no waiting out the dwell (and no dwell needed at all). It stays
    // armed until the page turns; writing cancels only the timed auto-deal,
    // and a tap must land clear of the child's own ink, so a dot in a word
    // never tears the page away but a clear tap always turns it.
    let mut learn_tap_advance = false;
    // The page rectangle the last play-page image covered, captured at send
    // time: the model's drawing turn comes back in 0–100 of this frame, and
    // the child may keep inking while the reply streams, so it cannot be
    // recomputed later.
    let mut learn_sent_frame = BBox::empty();
    // Learn asks must never go to a capture sink (the pad's default model may
    // be one, archiving pages instead of marking them): prefer the dedicated
    // learn model, else the ask model, else whatever the pad uses.
    let learn_model: Option<String> = std::env::var("RIDDLE_LEARN_MODEL")
        .ok()
        .or_else(|| std::env::var("RIDDLE_OPENAI_ASK_MODEL").ok());
    let mut drawer_selection: Option<usize> = None;
    let mut drawer_scroll = 0i32;
    let mut controls_saved: Option<Vec<u8>> = None;
    let mut controls_until: Option<Instant> = None;
    let mut sleep_requested = false;
    let mut queued_gestures: Vec<touch::Gesture> = Vec::new();
    let mut fallback_touch: Option<((i32, i32), (i32, i32))> = None;
    let mut control_pen_latched = false;
    // The canvas's page stack, and the flip banner that names where you are.
    let mut notebook = notebook::Notebook::new();
    let mut banner_saved: Option<Vec<u8>> = None;
    let mut banner_until: Option<Instant> = None;
    // What the pen tip does, chosen from the tap-summoned palette. The
    // marker's hardware eraser end always erases regardless of this.
    let mut selected_tool = pen::Tool::Pen;
    let mut palette: Option<ui::Palette> = None;
    let mut palette_until: Option<Instant> = None;

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
    if let (Some(png), true) = (ask_png, learn_session.is_none()) {
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

    'pad: loop {
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
                        if let Some(p) = palette.take() {
                            let (px, py, pw, ph) = p.close(&mut surf).rect();
                            disp.update(px, py, pw, ph, false);
                            palette_until = None;
                        }
                        // The banner lives inside the strip's saved region;
                        // interleaved save/restore patches would corrupt both.
                        if let Some(saved) = banner_saved.take() {
                            ui::restore_page_banner(&mut surf, &saved);
                            banner_until = None;
                        }
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
                // The drawer opens over the canvas AND over an open turn
                // page — reading one session must not wall off the rest of
                // the board. Whatever was open rides in `return_to`.
                touch::Gesture::OpenDrawer if matches!(state,
                    State::Listening { .. } | State::Lingering { .. } | State::SessionPage { .. }) => {
                    if let Some(p) = palette.take() {
                        let (px, py, pw, ph) = p.close(&mut surf).rect();
                        disp.update(px, py, pw, ph, false);
                        palette_until = None;
                    }
                    if let Some(saved) = controls_saved.take() { ui::restore_controls(&mut surf, &saved); }
                    open_drawer(&mut state, &mut surf, &disp, &ui_font, &store, drawer_selection);
                }
                touch::Gesture::CloseDrawer => {
                    close_overlay(&mut state, &mut surf, &disp, &mut drawer_selection, &mut drawer_scroll);
                }
                touch::Gesture::Page(delta) if matches!(state, State::Lingering { .. }) => {
                    let _ = step_reply_page(delta, &font, reply_w, &mut reply_pages, &mut reply_page,
                        &mut state, &mut surf, &disp);
                }
                // The turn page flips on the swipe alone (not the two-finger
                // scroll, which fires once per frame and would tear through
                // every page in one drag).
                touch::Gesture::Page(delta) if matches!(state, State::SessionPage { .. }) => {
                    session_page_flip(delta, &mut state, &mut surf, &disp, &ui_font);
                }
                // Flipping the writing canvas: forward through the notebook
                // (a fresh sheet past an inked last page), back to earlier
                // sheets. Overlays come down first so they are not parked
                // into the page snapshot.
                touch::Gesture::Page(delta)
                    if matches!(state, State::Listening { .. }) && !pen_down && learn_session.is_none() => {
                    let mut came_down = false;
                    if let Some(p) = palette.take() {
                        p.close(&mut surf);
                        palette_until = None;
                        came_down = true;
                    }
                    if let Some(saved) = controls_saved.take() {
                        ui::restore_controls(&mut surf, &saved);
                        controls_until = None;
                        came_down = true;
                    }
                    if let Some(saved) = banner_saved.take() {
                        ui::restore_page_banner(&mut surf, &saved);
                        banner_until = None;
                        came_down = true;
                    }
                    let flipped = if delta < 0 {
                        notebook.prev(&mut surf, &mut user_ink)
                    } else {
                        notebook.next(&mut surf, &mut user_ink)
                    };
                    if flipped {
                        send_mode = None;
                        state = State::Listening { last_pen: None };
                        let (current, total) = notebook.position();
                        eprintln!("g-pad: page {current} of {total}");
                        banner_saved = Some(ui::draw_page_banner(&mut surf, &ui_font, current, total));
                        banner_until = Some(Instant::now() + Duration::from_millis(1500));
                        disp.full_refresh(surf.w, surf.h);
                    } else if came_down {
                        // The flip was refused (a cover, or a full notebook)
                        // but overlays already left the surface; show that.
                        disp.update_all(surf.w, surf.h);
                    }
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
                            &mut user_ink, &mut notebook, &mut send_mode, &mut sleep_requested,
                            &mut prefs, &mut idle_commit, drawer_selection, drawer_scroll, &mut learn_session);
                        // A control action closes the praise moment: a NEW
                        // PAGE from the strip must not be followed by a stale
                        // auto-deal or tap-deal on the fresh page.
                        learn_advance_pending = false;
                        learn_auto_at = None;
                        learn_tap_advance = false;
                    } else if matches!(state, State::Settings { .. }) {
                        let action = ui::settings_action(x, y);
                        match action {
                            ui::Action::SetMode(mode) => { prefs.mode = mode; let _ = prefs.save(); }
                            ui::Action::ToggleIdle => {
                                prefs.idle_send_ms = if prefs.idle_send_ms == 0 { 2800 } else { 0 };
                                idle_commit = Duration::from_millis(prefs.idle_send_ms); let _ = prefs.save();
                            }
                            ui::Action::ToggleLearn => {
                                prefs.page = match prefs.page {
                                    preferences::Page::Learn => preferences::Page::Pad,
                                    preferences::Page::Pad => preferences::Page::Learn,
                                };
                                let _ = prefs.save();
                                // Land directly on the chosen page, clean.
                                close_overlay(&mut state, &mut surf, &disp, &mut drawer_selection, &mut drawer_scroll);
                                user_ink.clear();
                                learn_advance_pending = false;
                                learn_auto_at = None;
                                learn_tap_advance = false;
                                surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
                                learn_session = match prefs.page {
                                    preferences::Page::Learn => {
                                        let mut s = learn::Session::start();
                                        s.draw(&mut surf, &ui_font);
                                        Some(s)
                                    }
                                    preferences::Page::Pad => None,
                                };
                                disp.full_refresh(surf.w, surf.h);
                                state = State::Listening { last_pen: None };
                                continue;
                            }
                            ui::Action::Close => {
                                close_overlay(&mut state, &mut surf, &disp, &mut drawer_selection, &mut drawer_scroll);
                                continue;
                            }
                            ui::Action::Quit => {
                                // A bare break here only leaves the gesture
                                // loop — the pad shrugs and keeps running.
                                eprintln!("g-pad: leave from settings");
                                break 'pad;
                            }
                            _ => {}
                        }
                        ui::draw_settings(&mut surf, &ui_font, prefs);
                        disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
                    } else if matches!(state, State::SessionPage { .. }) {
                        // Specific targets, not a whole-page trigger — the
                        // first tap-anywhere-closes build read as breakage
                        // on hardware: an idle touch threw the page away.
                        // ← AGENTS returns to the board, × closes to the
                        // canvas, and the rest of the page is inert.
                        match ui::session_page_action(x, y) {
                            ui::Action::Sessions => {
                                open_drawer(&mut state, &mut surf, &disp, &ui_font, &store, drawer_selection);
                            }
                            ui::Action::Close => {
                                close_overlay(&mut state, &mut surf, &disp, &mut drawer_selection, &mut drawer_scroll);
                            }
                            _ => {}
                        }
                    } else if let Some(p) = palette.take() {
                        // A tap on a row picks that tool; anywhere else just
                        // puts the page back.
                        let choice = p.tap(x, y);
                        let (px, py, pw, ph) = p.close(&mut surf).rect();
                        disp.update(px, py, pw, ph, false);
                        palette_until = None;
                        if let Some(tool) = choice {
                            selected_tool = tool;
                            eprintln!("g-pad: pen tip is now the {tool:?}");
                        }
                    } else if matches!(state, State::Listening { .. }) && !pen_down && learn_session.is_none() {
                        // A bare finger tap on the open page summons the pen
                        // palette where the finger landed. Not in Learn mode:
                        // a child's stray finger must never grow chrome.
                        let p = ui::Palette::open(&mut surf, &ui_font, x, y, selected_tool);
                        let (px, py, pw, ph) = p.region().rect();
                        disp.update(px, py, pw, ph, false);
                        palette = Some(p);
                        palette_until = Some(Instant::now() + Duration::from_secs(10));
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
        if palette_until.is_some_and(|t| Instant::now() >= t) {
            if let Some(p) = palette.take() {
                let (px, py, pw, ph) = p.close(&mut surf).rect();
                disp.update(px, py, pw, ph, false);
            }
            palette_until = None;
        }
        if banner_until.is_some_and(|t| Instant::now() >= t) {
            if let Some(saved) = banner_saved.take() {
                ui::restore_page_banner(&mut surf, &saved);
                let (bx, by) = ui::banner_origin();
                disp.update(bx as i32, by as i32, ui::BANNER_W as i32, ui::BANNER_H as i32, false);
            }
            banner_until = None;
        }

        // ---- power button: sleep page, suspend, restore on wake ----
        if let Some(ref mut p) = power_dev {
            let pressed = p.drain_pressed();
            if (pressed || sleep_requested) && Instant::now() >= power_grace {
                sleep_requested = false;
                eprintln!("g-pad: sleeping (power button)");
                let saved = gesture::show_sleep(&mut surf, &font, &ui_font);
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
                gesture::restore_sleep(&mut surf, &saved);
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
                // The turn page is a pen surface like the canvas: while the
                // pen is near, the palm must not tap the page closed.
                if s.proximity && !matches!(state,
                    State::Settings { .. } | State::Drawer { .. }
                    | State::ExpandedConversation { .. })
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
                            if let Some(ref session) = learn_session {
                                // Anchored marks, never shape recognition: a
                                // stroke landing in a decision box is a command.
                                if let Some(tick) = learn_mark(&mut user_ink, session, &ui_font, &mut surf, &disp) {
                                    learn_tick = Some(tick);
                                } else if learn_tap_advance
                                    && user_ink.last_stroke_is_tap()
                                    && user_ink.last_stroke_clear_of_rest(100)
                                {
                                    // The praise is on show and the child
                                    // tapped clear of their own ink: deal the
                                    // next page now — the tap is a NEW in
                                    // spirit. A dot near the writing is just
                                    // ink (an 'i' keeps its dot); writing
                                    // cancels only the timed auto-deal, so a
                                    // clear tap afterwards still turns the
                                    // page.
                                    let _ = user_ink.pop_stroke();
                                    learn_tick = Some(LearnTick::New);
                                }
                            } else if let Some(mode) = absorb_send_rule(&mut user_ink, &mut surf, &disp) {
                                send_mode = Some(mode);
                            }
                        } else if matches!(state, State::SessionPage { .. }) {
                            session_page_mark(&mut state, &mut user_ink, &mut surf, &disp, &ui_font);
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
                if palette.as_ref().is_some_and(|p| p.contains(s.x, s.y)) {
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
                        // The hardware eraser end always erases; the tip does
                        // whatever the palette last chose.
                        let d = match (s.tool, selected_tool) {
                            (pen::Tool::Eraser, _) | (_, pen::Tool::Eraser) => {
                                user_ink.erase_point(&mut surf, s.x, s.y, 22)
                            }
                            _ => {
                                let r = 2 + s.pressure * 3 / pen::MAX_PRESSURE;
                                user_ink.pen_point(&mut surf, s.x, s.y, r)
                            }
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
                    // The turn page: the pen marks. Ink lands like anywhere
                    // else; pen-up reads it against the hit map and the
                    // redraw absorbs it.
                    State::SessionPage { .. } => {
                        pen_down = true;
                        if s.tool == pen::Tool::Pen {
                            let r = 2 + s.pressure * 3 / pen::MAX_PRESSURE;
                            let d = user_ink.pen_point(&mut surf, s.x, s.y, r);
                            if !d.is_empty() {
                                ink_dirty.add(d.x0, d.y0, 0);
                                ink_dirty.add(d.x1, d.y1, 0);
                            }
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
                    if palette.as_ref().is_some_and(|p| p.contains(ev.x, ev.y)) {
                        if !control_pen_latched {
                            queued_gestures.push(touch::Gesture::Tap(ev.x, ev.y));
                            control_pen_latched = true;
                        }
                        continue;
                    }
                    if let State::Listening { ref mut last_pen } = state {
                        pen_down = true;
                        let d = if selected_tool == pen::Tool::Eraser {
                            user_ink.erase_point(&mut surf, ev.x, ev.y, 22)
                        } else {
                            let r = 2 + ev.d.clamp(0, 100) / 45;
                            user_ink.pen_point(&mut surf, ev.x, ev.y, r)
                        };
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
                    } else if matches!(state, State::Settings { .. } | State::Drawer { .. }
                        | State::ExpandedConversation { .. } | State::SessionPage { .. }) {
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
                            if let Some(ref session) = learn_session {
                                // Anchored marks, never shape recognition: a
                                // stroke landing in a decision box is a command.
                                if let Some(tick) = learn_mark(&mut user_ink, session, &ui_font, &mut surf, &disp) {
                                    learn_tick = Some(tick);
                                } else if learn_tap_advance
                                    && user_ink.last_stroke_is_tap()
                                    && user_ink.last_stroke_clear_of_rest(100)
                                {
                                    // The praise is on show and the child
                                    // tapped clear of their own ink: deal the
                                    // next page now — the tap is a NEW in
                                    // spirit. A dot near the writing is just
                                    // ink (an 'i' keeps its dot); writing
                                    // cancels only the timed auto-deal, so a
                                    // clear tap afterwards still turns the
                                    // page.
                                    let _ = user_ink.pop_stroke();
                                    learn_tick = Some(LearnTick::New);
                                }
                            } else if let Some(mode) = absorb_send_rule(&mut user_ink, &mut surf, &disp) {
                                send_mode = Some(mode);
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

        // ---- learn mode: an absorbed decision-box mark acts here ----
        if let Some(tick) = learn_tick.take() {
            // The child acted first: a pending auto-dealt page yields.
            learn_auto_at = None;
            learn_advance_pending = false;
            learn_tap_advance = false;
            if matches!(state, State::Listening { .. }) {
                if let Some(ref mut session) = learn_session {
                    // Marks the page answers locally: NEW deals, MENU opens
                    // the picker, and a picker choice deals what it names.
                    let deal_locally = match tick {
                        LearnTick::New => {
                            session.next();
                            true
                        }
                        LearnTick::Menu => {
                            session.open_menu();
                            true
                        }
                        LearnTick::Choice(i) if session.is_menu() => session.choose_menu(i),
                        _ => false,
                    };
                    if deal_locally {
                        user_ink.clear();
                        session.draw(&mut surf, &ui_font);
                        disp.full_refresh(surf.w, surf.h);
                    } else {
                    // A story choice tick must name a box that exists on this
                    // page; registering it also latches the label for the ask.
                    let tick_valid = match tick {
                        LearnTick::Choice(i) => session.choose(i).is_some(),
                        _ => true,
                    };
                    if !tick_valid {
                        // A stale mark (no such box): nothing to do.
                    } else {
                    match tick {
                        LearnTick::New | LearnTick::Menu => unreachable!("dealt locally above"),
                        // A choice tick is a DONE with the chosen box latched.
                        LearnTick::Done | LearnTick::Choice(_) => {
                            clear_feedback(&mut surf, &disp);
                            if session.needs_ink() && !user_ink.has_ink_in(&session.hits.answer) {
                                // An empty blank needs no oracle to mark.
                                turn_failed = true;
                                let nudge = match session.page {
                                    learn::Page::Play(_) => "Draw something first, then mark DONE.",
                                    _ => "Write your answer first, then mark DONE.",
                                };
                                let plan = plan_reply(&font, nudge, Some(learn::sheet::feedback_y()));
                                state = State::Replying { plan, next: Instant::now(), rx: None };
                            } else if let Some(ref o) = oracle {
                                // Practice sends the child's pen work alone
                                // on white: a digit written too big or over
                                // the printed box must reach the tutor whole
                                // and untangled — cropped or furniture-merged
                                // glyphs read as NO every time. Play pages
                                // keep the page crop: their drawings live on
                                // the canvas the game printed.
                                let sent = if matches!(session.page, learn::Page::Practice(_)) {
                                    user_ink.ink_png(&session.hits.answer, 80, PNG_PATH)
                                } else {
                                    let crop = user_ink.crop_for(&session.hits.answer, 80);
                                    ink::region_png(&surf, crop, PNG_PATH)
                                        .map(|frame| learn_sent_frame = frame)
                                };
                                if let Err(e) = sent {
                                    eprintln!("g-pad: rasterize answer failed: {e}");
                                }
                                let ctx = oracle::TurnContext {
                                    instruction: Some(session.instruction()),
                                    // Story beats stay coherent across turns.
                                    history: session.story_history(),
                                    ..Default::default()
                                };
                                let (tx, rx) = mpsc::channel();
                                o.ask_with_model(PNG_PATH, &ctx, tx, learn_model.as_deref());
                                if std::env::var_os("RIDDLE_KEEP_PAGE").is_none() {
                                    let _ = std::fs::remove_file(PNG_PATH);
                                }
                                state = State::LearnMarking {
                                    rx,
                                    pulse: Instant::now(),
                                    blot_on: false,
                                    since: Instant::now(),
                                    got: String::new(),
                                };
                            } else {
                                turn_failed = true;
                                let plan = plan_reply(&font, &oracle_excuse("no oracle"),
                                    Some(learn::sheet::feedback_y()));
                                state = State::Replying { plan, next: Instant::now(), rx: None };
                            }
                        }
                    }
                    }
                    }
                }
            }
        }

        // ---- learn mode: a correct answer deals the next page by itself ----
        if let Some(at) = learn_auto_at {
            if pen_down {
                // The child went back to the page: let them.
                learn_auto_at = None;
            } else if at <= Instant::now() && matches!(state, State::Listening { .. }) {
                learn_auto_at = None;
                learn_tap_advance = false;
                if let Some(ref mut session) = learn_session {
                    session.next();
                    user_ink.clear();
                    session.draw(&mut surf, &ui_font);
                    disp.full_refresh(surf.w, surf.h);
                }
            }
        }

        // ---- state machine ----
        state = match state {
            State::Listening { last_pen } => match last_pen {
                Some(t)
                    if learn_session.is_none()
                        && !pen_down
                        && (send_mode.is_some()
                            || (!idle_commit.is_zero() && t.elapsed() >= idle_commit))
                        && !user_ink.is_empty() =>
                {
                    let commit_mode = send_mode.take().unwrap_or(CommitMode::Capture);
                    // Overlays must not outlive the page they were opened
                    // over: the drink and the reply repaint beneath them, and
                    // their saved patches would restore stale pixels.
                    if let Some(p) = palette.take() {
                        let (px, py, pw, ph) = p.close(&mut surf).rect();
                        disp.update(px, py, pw, ph, false);
                        palette_until = None;
                    }
                    if let Some(saved) = banner_saved.take() {
                        ui::restore_page_banner(&mut surf, &saved);
                        let (bx, by) = ui::banner_origin();
                        disp.update(bx as i32, by as i32, ui::BANNER_W as i32, ui::BANNER_H as i32, false);
                        banner_until = None;
                    }
                    if region_all_white(&surf, user_ink.bbox) {
                        // Everything was erased before commit: nothing to commit.
                        user_ink.clear();
                        State::Listening { last_pen: None }
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

            State::Lingering { region, more } => {
                if learn_session.is_some() {
                    // Learn feedback stays painted; the page listens again at
                    // once so the child can keep writing or retry. A YES
                    // verdict starts the dwell that deals the next page.
                    if learn_advance_pending {
                        learn_advance_pending = false;
                        learn_tap_advance = true;
                        learn_auto_at = learn_next_dwell.map(|d| Instant::now() + d);
                    }
                    State::Listening { last_pen: None }
                } else {
                    State::Lingering { region, more }
                }
            }

            State::LearnMarking { rx, pulse, blot_on, since, mut got } => {
                // The thinking blot pulses above the DONE box the child marked.
                let (bx, by) = learn_session
                    .as_ref()
                    .map(|s| ((s.hits.done.x0 + s.hits.done.x1) / 2, (s.hits.done.y0 - 40).max(20)))
                    .unwrap_or((THINK_X, THINK_Y));
                let clear_blot = |surf: &mut Surface, disp: &display::Display| {
                    surf.fill_rect((bx - 14).max(0) as usize, (by - 14).max(0) as usize, 28, 28, WHITE);
                    disp.update(bx - 14, by - 14, 28, 28, true);
                };
                match rx.try_recv() {
                    Ok(Ok(Event::Ink(text))) => {
                        if !got.is_empty() {
                            got.push(' ');
                        }
                        got.push_str(&text);
                        State::LearnMarking { rx, pulse, blot_on, since, got }
                    }
                    // Conjuring and transcription have no meaning on a worksheet.
                    Ok(Ok(_)) => State::LearnMarking { rx, pulse, blot_on, since, got },
                    Ok(Err(e)) => {
                        eprintln!("g-pad: tutor failed: {e}");
                        clear_blot(&mut surf, &disp);
                        turn_failed = true;
                        let plan = plan_reply(&font, &oracle_excuse(&e), Some(learn::sheet::feedback_y()));
                        State::Replying { plan, next: Instant::now(), rx: None }
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        if since.elapsed() >= ORACLE_PATIENCE {
                            eprintln!("g-pad: tutor timed out after {}s", ORACLE_PATIENCE.as_secs());
                            clear_blot(&mut surf, &disp);
                            turn_failed = true;
                            let plan = plan_reply(&font, &oracle_excuse("timed out"), Some(learn::sheet::feedback_y()));
                            State::Replying { plan, next: Instant::now(), rx: None }
                        } else if pulse.elapsed() >= Duration::from_millis(600) {
                            if blot_on {
                                surf.fill_rect((bx - 14).max(0) as usize, (by - 14).max(0) as usize, 28, 28, WHITE);
                            } else {
                                surf.stamp(bx, by, 9, BLACK);
                            }
                            disp.update(bx - 14, by - 14, 28, 28, true);
                            State::LearnMarking { rx, pulse: Instant::now(), blot_on: !blot_on, since, got }
                        } else {
                            State::LearnMarking { rx, pulse, blot_on, since, got }
                        }
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        clear_blot(&mut surf, &disp);
                        let mut text = String::new();
                        let mut text_y = learn::sheet::feedback_y();
                        // Practice verdicts are stamped in print, not written
                        // in the reply hand — no animation to sit through.
                        let mut stamped = false;
                        if let Some(ref mut session) = learn_session {
                            match learn_flavor(session) {
                                LearnFlavor::Practice => {
                                    let (verdict, fb) = learn::verdict::parse(got.trim());
                                    session.record(verdict);
                                    let answer = session.hits.answer;
                                    let mut mark_dirty = BBox::empty();
                                    match verdict {
                                        learn::Verdict::Yes => {
                                            // The winning answer stays on show, but
                                            // the tracked ink is dropped so a second
                                            // DONE cannot resubmit it (or a mixture).
                                            user_ink.clear();
                                            mark_dirty = learn::sheet::draw_check(&mut surf, &answer);
                                            // Once the praise is written, the next
                                            // page deals itself — no second tap.
                                            learn_advance_pending = true;
                                        }
                                        learn::Verdict::Almost | learn::Verdict::No => {
                                            // A clean retry: repaint the sheet so the
                                            // next attempt is written into an empty
                                            // blank, never on top of the last one —
                                            // the tutor must only ever be shown one
                                            // answer at a time.
                                            user_ink.clear();
                                            session.draw(&mut surf, &ui_font);
                                            learn::sheet::draw_look_again(&mut surf, &answer);
                                            disp.full_refresh(surf.w, surf.h);
                                        }
                                        // An unreadable verdict moves nothing: the ink
                                        // stays, and DONE can simply be marked again.
                                        learn::Verdict::Unknown => {}
                                    }
                                    if !mark_dirty.is_empty() {
                                        let (x, y, w, h) = mark_dirty.rect();
                                        disp.update(x, y, w, h, true);
                                    }
                                    // The child's channel is the marks and the
                                    // big cheer; the hint line is the grown-up's.
                                    let hint = if verdict == learn::Verdict::Unknown && fb.hint.trim().is_empty() {
                                        oracle_excuse("empty reply")
                                    } else {
                                        fb.hint.clone()
                                    };
                                    let region = learn::sheet::draw_feedback(
                                        &mut surf, &ui_font, &fb.cheer, &hint,
                                        verdict == learn::Verdict::Yes,
                                    );
                                    let (x, y, w, h) = region.rect();
                                    disp.update(x, y, w, h, true);
                                    stamped = true;
                                }
                                LearnFlavor::Critter => {
                                    // The pad's turn: its own pen strokes on
                                    // the doodle (menu decoration as the
                                    // fallback), one line of commentary.
                                    let turn = learn::games::parse_critter_turn(got.trim());
                                    let dirty = if !turn.strokes.is_empty() {
                                        learn::games::draw_strokes(
                                            &mut surf, &learn_sent_frame,
                                            &session.hits.answer, &turn.strokes,
                                        )
                                    } else if let Some(d) = turn.deco {
                                        learn::games::draw_deco(&mut surf, &user_ink.bbox, &d, &ui_font)
                                    } else {
                                        BBox::empty()
                                    };
                                    if !dirty.is_empty() {
                                        let (x, y, w, h) = dirty.rect();
                                        disp.update(x, y, w, h, true);
                                    }
                                    session.critter_turn();
                                    text = if turn.caption.trim().is_empty() {
                                        "There. Much better.".to_string()
                                    } else {
                                        turn.caption
                                    };
                                }
                                LearnFlavor::Guess => {
                                    text = if got.trim().is_empty() {
                                        "Hmm. Draw one more clue!".to_string()
                                    } else {
                                        got.trim().to_string()
                                    };
                                }
                                LearnFlavor::Story => {
                                    // A fresh scene: log the beat, offer the next
                                    // three doors, clear the stage for doodling.
                                    let (beat, choices) = learn::games::parse_story(got.trim());
                                    session.story_advance(beat.clone(), choices);
                                    user_ink.clear();
                                    session.draw(&mut surf, &ui_font);
                                    disp.full_refresh(surf.w, surf.h);
                                    text_y = learn::games::story_text_y();
                                    text = if beat.trim().is_empty() {
                                        "And then... hmm. Mark a box!".to_string()
                                    } else {
                                        beat
                                    };
                                }
                            }
                        }
                        // A Learn turn never enters the diary's memory.
                        turn_failed = true;
                        if stamped {
                            // Feedback is already on the page; listen again at
                            // once, and a YES starts the deal-next dwell.
                            if learn_advance_pending {
                                learn_advance_pending = false;
                                learn_tap_advance = true;
                                learn_auto_at = learn_next_dwell.map(|d| Instant::now() + d);
                            }
                            State::Listening { last_pen: None }
                        } else {
                            let plan = plan_reply(&font, &text, Some(text_y));
                            State::Replying { plan, next: Instant::now(), rx: None }
                        }
                    }
                }
            }


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
            // The turn page rests until the reader closes it.
            s @ State::SessionPage { .. } => s,

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

/// Open the drawer on the AGENTS board over whatever is on screen — the
/// canvas or an open turn page. What was open rides in `return_to`, so
/// closing the drawer puts it back. History and Corpus stay one tab-tap
/// away inside the drawer.
fn open_drawer(state: &mut State, surf: &mut Surface, disp: &display::Display,
    ui_font: &FontRef, store: &Option<memory::MemoryStore>, selection: Option<usize>) {
    let old = std::mem::replace(state, State::Listening { last_pen: None });
    let panel = ui::Drawer::open(surf, ui::DrawerKind::Sessions, selection, 0, None);
    let snapshot = oracle::context_snapshot(store, memory_turns());
    ui::draw_drawer(surf, ui_font, store, &snapshot, &panel);
    disp.update(0, 0, ui::PANEL_W as i32, SCREEN_H as i32, false);
    *state = State::Drawer { panel: Some(panel), return_to: Box::new(old) };
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
        State::SessionPage { saved, return_to, .. } => {
            surf.paste_rect(0, 0, SCREEN_W, SCREEN_H, &saved);
            disp.update(0, 0, SCREEN_W as i32, SCREEN_H as i32, false); *state = *return_to;
        }
        other => *state = other,
    }
}

/// Read the stroke just finished on the turn page against the hit map, act,
/// and redraw — the redraw absorbs the ink, so a command leaves no mark.
///
/// Approval is tiered by consequence, and until the hub reports tiers every
/// pending action counts as destructive: the first tick arms the box, the
/// second sends. Reject is always cheaper — a strike takes effect at once.
/// See docs/anthink-interaction.md.
fn session_page_mark(state: &mut State, ink: &mut ink::Ink, surf: &mut Surface,
    disp: &display::Display, ui_font: &FontRef) {
    let Some(stroke) = ink.stroke_list().last().cloned() else { return };
    let _ = ink.pop_stroke();
    let State::SessionPage { session, remaining, stale, armed, status, boxr, page, .. } = state else {
        return;
    };
    let mut sb = BBox::empty();
    for &(x, y, r) in &stroke {
        sb.add(x, y, r);
    }
    match boxr.as_ref().and_then(|b| classify_mark(&stroke, &sb, b)) {
        // A mark that hit nothing is a note. Its ink stays on the page (the
        // stroke is already absorbed from the data, so it can never be
        // committed) — wiping it instantly read as breakage on hardware.
        None => return,
        Some(PageMark::Strike) => {
            // Only a pending action has something to reject.
            if boxr.map(|b| b.decision) == Some(ui::Decision::Approve) {
                *status = Some(match bridge::post_nudge(&session.id, "strike", None) {
                    Ok(()) => "REJECTED · SENT".to_string(),
                    Err(e) => e.to_uppercase(),
                });
                *armed = false;
            }
        }
        Some(PageMark::Tick) if *armed => {
            let sent = match boxr.map(|b| b.decision) {
                Some(ui::Decision::Continue) => bridge::post_nudge(&session.id, "text", Some("continue")),
                _ => bridge::post_nudge(&session.id, "tick", None),
            };
            *status = Some(match sent {
                Ok(()) => "SENT".to_string(),
                Err(e) => e.to_uppercase(),
            });
            *armed = false;
        }
        Some(PageMark::Tick) => {
            *armed = true;
            *status = None;
        }
    }
    *boxr = ui::draw_session_page(surf, ui_font, session, *remaining, *stale, *armed,
        status.as_deref(), *page);
    disp.update(0, 0, SCREEN_W as i32, SCREEN_H as i32, false);
}

/// Flip the turn page: a downward swipe pages back to earlier turns, an
/// upward swipe returns toward the newest. The whole page redraws (the box
/// and its hit map ride along), and a flip past either end simply holds.
fn session_page_flip(delta: i32, state: &mut State, surf: &mut Surface,
    disp: &display::Display, ui_font: &FontRef) {
    let State::SessionPage { session, remaining, stale, armed, status, boxr, page, .. } = state else {
        return;
    };
    let pages = ui::session_page_count(ui_font, session);
    let want = if delta < 0 {
        (*page + 1).min(pages.saturating_sub(1))
    } else {
        page.saturating_sub(1)
    };
    if want == *page {
        return;
    }
    *page = want;
    *boxr = ui::draw_session_page(surf, ui_font, session, *remaining, *stale, *armed,
        status.as_deref(), *page);
    disp.update(0, 0, SCREEN_W as i32, SCREEN_H as i32, false);
}

/// Anchored reading: where the mark landed, plus one cheap global property.
/// A wide flat stroke through the box is the strike; any other deliberate
/// mark whose center is inside the box is the tick.
fn classify_mark(stroke: &[(i32, i32, i32)], sb: &BBox, b: &ui::DecisionBox) -> Option<PageMark> {
    let (bx0, by0) = (b.x as i32, b.y as i32);
    let (bx1, by1) = (bx0 + b.w as i32, by0 + b.h as i32);
    if sb.x0 >= bx1 || sb.x1 <= bx0 || sb.y0 >= by1 || sb.y1 <= by0 {
        return None;
    }
    if gesture::looks_like_send_rule(stroke, (b.w / 4) as i32) {
        return Some(PageMark::Strike);
    }
    let (cx, cy) = ((sb.x0 + sb.x1) / 2, (sb.y0 + sb.y1) / 2);
    (cx >= bx0 && cx < bx1 && cy >= by0 && cy < by1).then_some(PageMark::Tick)
}

#[allow(clippy::too_many_arguments)]
fn apply_control(action: ui::Action, state: &mut State, surf: &mut Surface, disp: &display::Display,
    ui_font: &FontRef, store: &Option<memory::MemoryStore>, user_ink: &mut ink::Ink,
    notebook: &mut notebook::Notebook, send_mode: &mut Option<CommitMode>, sleep_requested: &mut bool,
    prefs: &mut preferences::Preferences, idle_commit: &mut Duration,
    selection: Option<usize>, scroll: i32, learn: &mut Option<learn::Session>) {
    // Learn mode repurposes the strip: committing is the DONE box, so SEND and
    // DISMISS do nothing; ERASE re-deals the same sheet clean; NEW PAGE deals
    // a fresh problem. Everything else behaves as on the pad.
    if let Some(session) = learn.as_mut() {
        match action {
            ui::Action::Send | ui::Action::Dismiss => return,
            ui::Action::Erase | ui::Action::NewPage => {
                if action == ui::Action::NewPage {
                    session.next();
                }
                user_ink.clear();
                session.draw(surf, ui_font);
                disp.full_refresh(surf.w, surf.h);
                *state = State::Listening { last_pen: None };
                return;
            }
            _ => {}
        }
    }
    match action {
        ui::Action::Send => *send_mode = Some(CommitMode::Capture),
        ui::Action::Erase => {
            surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE); disp.full_refresh(surf.w, surf.h);
            user_ink.clear(); *state = State::Listening { last_pen: None };
        }
        ui::Action::NewPage => {
            // An inked sheet is parked in the notebook (flip back with a
            // swipe to return to it); anything else is simply wiped, which
            // is what NEW PAGE always did.
            if !notebook.new_page(surf, user_ink) {
                surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
                user_ink.clear();
            }
            disp.full_refresh(surf.w, surf.h);
            *state = State::Listening { last_pen: None };
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
    if let ui::Action::OpenSession(i) = action {
        // Snapshot the session before the drawer closes; a poll can land at
        // any moment and the page should show what the row showed.
        let held = crate::bridge::held();
        let list = crate::bridge::readable(&held);
        let Some(session) = list.get(i).map(|s| (*s).clone()) else { return };
        let remaining = list.len().saturating_sub(1);
        close_overlay(state, surf, disp, selection, scroll);
        // A session picked from a page's own drawer REPLACES that page
        // rather than stacking on it: the new page inherits the canvas
        // saved underneath, so × always closes to the canvas in one step
        // and hopping between sessions cannot pile up saved screens.
        let (saved, old) = match std::mem::replace(state, State::Listening { last_pen: None }) {
            State::SessionPage { saved, return_to, .. } => (saved, *return_to),
            other => (surf.copy_rect(0, 0, SCREEN_W, SCREEN_H), other),
        };
        let boxr = ui::draw_session_page(surf, ui_font, &session, remaining, held.stale, false, None, 0);
        disp.update(0, 0, SCREEN_W as i32, SCREEN_H as i32, false);
        *state = State::SessionPage {
            session, remaining, stale: held.stale, armed: false, status: None, boxr,
            page: 0, saved, return_to: Box::new(old),
        };
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

/// What kind of page a Learn reply resolves against — a copyable snapshot so
/// resolution can branch without holding a borrow of the session's page.
#[derive(Clone, Copy)]
enum LearnFlavor {
    Practice,
    Critter,
    Guess,
    Story,
}

fn learn_flavor(session: &learn::Session) -> LearnFlavor {
    match &session.page {
        learn::Page::Practice(_) => LearnFlavor::Practice,
        learn::Page::Play(learn::games::Game::Critter { .. }) => LearnFlavor::Critter,
        learn::Page::Play(learn::games::Game::Guess) => LearnFlavor::Guess,
        learn::Page::Play(learn::games::Game::Story { .. }) => LearnFlavor::Story,
        // Unreachable in marking: the menu page has no DONE box to send from.
        learn::Page::Menu => LearnFlavor::Practice,
    }
}

/// Which decision box a Learn-mode mark landed in.
#[derive(Clone, Copy)]
enum LearnTick {
    Done,
    New,
    /// The MENU footer box: open the topic-and-game picker.
    Menu,
    /// A choice box — a story path, or a picker entry — by index.
    Choice(usize),
}

/// An anchored Learn-mode mark: if the just-finished stroke's centroid landed
/// in a decision box, absorb the stroke (pop it, white the ink out, restore
/// the box under it) and report which box. Ink anywhere else is the child's
/// answer and stays on the page. Form never matters — a check, a scribble,
/// and an "x" all mean the same deliberate thing: "that box".
fn learn_mark(ink: &mut ink::Ink, session: &learn::Session, ui_font: &FontRef,
    surf: &mut Surface, disp: &display::Display) -> Option<LearnTick> {
    let (cx, cy) = ink.last_stroke_centroid()?;
    let tick = match session.hits.hit(cx, cy) {
        Some(learn::Target::Done) => LearnTick::Done,
        Some(learn::Target::New) => LearnTick::New,
        Some(learn::Target::Menu) => LearnTick::Menu,
        Some(learn::Target::Choice(i)) => LearnTick::Choice(i),
        _ => return None,
    };
    if let Some(gone) = ink.pop_stroke() {
        let (x, y, w, h) = gone.rect();
        surf.fill_rect(x.max(0) as usize, y.max(0) as usize, w as usize, h as usize, WHITE);
        if session.is_menu() {
            // The menu page's furniture is its choice grid; the chosen page
            // repaints in full right after, so only the grid needs restoring.
            let _ = learn::sheet::draw_menu(surf, ui_font);
        } else {
            learn::sheet::refresh_boxes(surf, ui_font);
            // A story page also has choice boxes under the mark; repaint them.
            if let learn::Page::Play(learn::games::Game::Story { choices, .. }) = &session.page {
                let _ = learn::games::draw_choices(surf, ui_font, choices);
            }
        }
        disp.update(x, y, w, h, true);
    }
    Some(tick)
}

/// White out the tutor's previous feedback line so replies never stack.
fn clear_feedback(surf: &mut Surface, disp: &display::Display) {
    let region = learn::sheet::feedback_region();
    let (x, y, w, h) = region.rect();
    surf.fill_rect(x.max(0) as usize, y.max(0) as usize, w as usize, h as usize, WHITE);
    disp.update(x, y, w, h, true);
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
        if gesture::looks_like_ask_arrow(stroke, min_w) {
            Some(CommitMode::Ask)
        } else if gesture::looks_like_send_rule(stroke, min_w) {
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

    fn decision_box() -> ui::DecisionBox {
        ui::DecisionBox { x: 44, y: 1600, w: 1316, h: 96, decision: ui::Decision::Approve }
    }

    fn stroke_between(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32, i32)> {
        (0..=20)
            .map(|i| (x0 + (x1 - x0) * i / 20, y0 + (y1 - y0) * i / 20, 2))
            .collect()
    }

    fn bbox_of(stroke: &[(i32, i32, i32)]) -> BBox {
        let mut b = BBox::empty();
        for &(x, y, r) in stroke {
            b.add(x, y, r);
        }
        b
    }

    #[test]
    fn a_check_in_the_box_is_a_tick_and_a_flat_stroke_is_a_strike() {
        let b = decision_box();
        let tick = stroke_between(600, 1630, 660, 1680);
        assert_eq!(classify_mark(&tick, &bbox_of(&tick), &b), Some(PageMark::Tick));
        let strike = stroke_between(200, 1650, 1100, 1655);
        assert_eq!(classify_mark(&strike, &bbox_of(&strike), &b), Some(PageMark::Strike));
    }

    #[test]
    fn ink_outside_the_box_is_a_note_never_a_command() {
        // The anchoring rule: the same "v" means different things in
        // different places. Outside the box it commands nothing.
        let b = decision_box();
        let margin = stroke_between(600, 400, 660, 450);
        assert_eq!(classify_mark(&margin, &bbox_of(&margin), &b), None);
        let flat_above = stroke_between(200, 900, 1100, 905);
        assert_eq!(classify_mark(&flat_above, &bbox_of(&flat_above), &b), None);
    }

    #[test]
    fn a_mark_straddling_the_box_edge_counts_only_if_centered_inside() {
        let b = decision_box();
        // Center above the top edge: overlaps, but not aimed at the box.
        let straddle = stroke_between(600, 1500, 620, 1620);
        assert_eq!(classify_mark(&straddle, &bbox_of(&straddle), &b), None);
        // Center inside: aimed.
        let aimed = stroke_between(600, 1590, 620, 1690);
        assert_eq!(classify_mark(&aimed, &bbox_of(&aimed), &b), Some(PageMark::Tick));
    }

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
