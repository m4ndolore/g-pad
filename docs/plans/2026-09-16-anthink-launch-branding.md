# Anthink Launch Branding Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** g-pad boots into an Anthink splash card, owns the boot escape window, draws matching power-off and restart cards, and installs the same cards as the OS's shutdown images.

**Architecture:** A new `src/splash.rs` renders three card variants with the sleep card's drawing helpers (made `pub(crate)` in `gesture.rs`). `run()` in `main.rs` draws the boot card as its first frame and runs a 3s power-key watch from `power.rs`. A `--render-cards` flag writes the cards as grayscale PNGs; the installer copies two of them over `/usr/share/remarkable/`. Design: `docs/plans/2026-09-16-anthink-launch-branding-design.md`.

**Tech Stack:** Rust (ab_glyph, png), bash on the tablet (bash 5.2, busybox coreutils, no `timeout`), systemd units. Host tests via `cargo test`; device build via `./build-takeover-zig.sh` + `DEVICE=rm2 ./scripts/make-bundle.sh`.

**Branch:** `worktree-quill-3-28`, worktree `/Users/paulgarcia/Dev/g-pad/.claude/worktrees/quill-3-28`. Commit messages follow the repo style: a lowercase `feat:`/`fix:`/`docs:` prefix and a sentence that says what the user gets.

**Constants you will use:** `SCREEN_W = 1404`, `SCREEN_H = 1872` (`src/fb.rs`). Colors `WHITE`, `BLACK`, `FADED` (`src/surface.rs`). Test surfaces are built like the sleep-card test in `src/gesture.rs` (`sleep_page_renders_and_restores`): a `Vec<u8>` of `w*h*4` bytes wrapped in `Surface::new(ptr, len, w, h, w*4, PixFmt::Rgb32)`.

---

### Task 1: Share the sleep card's drawing helpers

**Files:**
- Modify: `src/gesture.rs` (fns `mark_gray`, `blit_gray`, `blit_left`, `frame`, `blit_centered`)

**Step 1: Make the helpers crate-visible and add a colored `blit_left`**

Change the five signatures from `fn` to `pub(crate) fn`. Replace `blit_left` with a colored core plus the black wrapper:

```rust
pub(crate) fn blit_left(
    surf: &mut Surface,
    font: &FontRef,
    text: &str,
    px_size: f32,
    tracking: usize,
    x0: usize,
    y: usize,
) {
    blit_left_in(surf, font, text, px_size, tracking, x0, y, BLACK);
}

/// `blit_left` in any ink: the boot card's hatch line is FADED gray.
#[allow(clippy::too_many_arguments)]
pub(crate) fn blit_left_in(
    surf: &mut Surface,
    font: &FontRef,
    text: &str,
    px_size: f32,
    tracking: usize,
    x0: usize,
    y: usize,
    color: u16,
) {
    // A space rasterizes to a zero-width mask, so carry the word gap
    // explicitly or tracked lines run their words together.
    let space = (px_size * 0.32) as usize;
    let mut x = x0;
    for c in text.chars() {
        if c == ' ' {
            x += space + tracking;
            continue;
        }
        let g = script::rasterize_line(font, &c.to_string(), px_size);
        for row in 0..g.height {
            for col in 0..g.width {
                if g.mask[row * g.width + col] {
                    surf.put_px((x + col) as i32, (y + row) as i32, color);
                }
            }
        }
        x += g.width + tracking;
    }
}
```

**Step 2: Run the existing tests**

Run: `cargo test gesture`
Expected: all gesture tests pass (`brand_mark_keeps_its_grays`, `sleep_page_renders_and_restores`, …).

**Step 3: Commit**

```bash
git add src/gesture.rs
git commit -m "refactor: the sleep card's drawing helpers are shared, and a line can be drawn in any ink"
```

---

### Task 2: The card renderer

**Files:**
- Create: `src/splash.rs`
- Modify: `src/main.rs:41` (add `mod splash;` after `mod script;`)

**Step 1: Write the failing tests**

Create `src/splash.rs` with only the test module for now:

```rust
//! The Anthink cards: the boot splash, and the power-off and restart sheets.
//! Same grammar as the Merge Combinator sleep card in `gesture.rs` — one
//! hairline frame, one left spine, set type over rules, one line in the
//! pad's own hand — so the two read as one system.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::{SCREEN_H, SCREEN_W};
    use crate::surface::{PixFmt, Surface, BLACK};
    use ab_glyph::FontRef;

    fn fonts() -> (FontRef<'static>, FontRef<'static>) {
        let hand = FontRef::try_from_slice(include_bytes!("../fonts/DancingScript.ttf")).unwrap();
        let ui = FontRef::try_from_slice(crate::ui::UI_FONT_TTF).unwrap();
        (hand, ui)
    }

    fn dark(surf: &Surface, x0: usize, y0: usize, w: usize, h: usize) -> usize {
        let mut n = 0;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if surf.luma(x as i32, y as i32) < 128 {
                    n += 1;
                }
            }
        }
        n
    }

    fn render(card: Card) -> (Vec<u8>, usize) {
        let (w, h) = (SCREEN_W, SCREEN_H);
        let mut buf = vec![0u8; w * h * 4];
        let ptr = buf.as_mut_ptr();
        let mut surf = Surface::new(ptr, buf.len(), w, h, w * 4, PixFmt::Rgb32);
        surf.fill_rect(300, 1000, 400, 300, BLACK); // stale ink the card must cover
        let (hand, ui) = fonts();
        draw(&mut surf, &hand, &ui, card);
        let frame_px = dark(&surf, 56, 56, SCREEN_W - 112, 2);
        (buf, frame_px)
    }

    fn surface_of(buf: &mut Vec<u8>) -> Surface {
        let ptr = buf.as_mut_ptr();
        Surface::new(ptr, buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, PixFmt::Rgb32)
    }

    #[test]
    fn boot_card_has_the_frame_and_the_wordmark_on_the_spine() {
        let (mut buf, frame_px) = render(Card::Boot);
        assert_eq!(frame_px, (SCREEN_W - 112) * 2, "top frame edge is not a solid 2px line");
        let surf = surface_of(&mut buf);
        // Nothing but the frame lives left of the spine.
        assert_eq!(dark(&surf, 60, 100, 88, SCREEN_H - 200), 0, "ink left of the x=150 spine");
        // The wordmark band carries real ink inside the column.
        assert!(dark(&surf, 150, WORDMARK_Y, SCREEN_W - 300, 160) > 20_000, "wordmark band looks empty");
        // The stale ink under the card is gone (this band sits between the
        // venture line and the human line, where the card draws nothing).
        assert_eq!(dark(&surf, 300, 1000, 400, 300), 0, "stale ink shows through the card");
    }

    #[test]
    fn only_the_boot_card_shows_the_hatch_line() {
        let (x, y, w, h) = HATCH;
        for (card, expect) in [(Card::Boot, true), (Card::PowerOff, false), (Card::Restart, false)] {
            let (mut buf, _) = render(card);
            let surf = surface_of(&mut buf);
            let ink = dark(&surf, x, y, w, h);
            assert_eq!(ink > 200, expect, "{card:?}: hatch ink {ink}");
        }
    }

    #[test]
    fn erase_hatch_leaves_only_paper_in_its_rect() {
        let (mut buf, _) = render(Card::Boot);
        let mut surf = surface_of(&mut buf);
        erase_hatch(&mut surf);
        let (x, y, w, h) = HATCH;
        assert_eq!(dark(&surf, x, y, w, h), 0);
        // The human line above it survives.
        assert!(dark(&surf, 0, SCREEN_H - 210, SCREEN_W, 80) > 500, "human line erased with the hatch");
    }

    #[test]
    fn the_mark_keeps_its_grays_after_shrinking() {
        let (mw, mh, gray) = crate::gesture::mark_gray().unwrap();
        let (sw, sh, small) = shrink3(mw, mh, &gray);
        assert_eq!((sw, sh), (mw / 3, mh / 3));
        let mut buckets = std::collections::BTreeSet::new();
        for &g in &small {
            buckets.insert(g / 16);
        }
        assert!(buckets.len() >= 8, "shrunk mark has only {} ink levels", buckets.len());
    }
}
```

**Step 2: Run to verify they fail**

Run: `cargo test splash`
Expected: compile errors, `Card`, `draw`, `HATCH`, `WORDMARK_Y`, `erase_hatch`, `shrink3` not found.

**Step 3: Write the implementation** (above the test module in `src/splash.rs`)

```rust
use ab_glyph::FontRef;

use crate::fb::{SCREEN_H, SCREEN_W};
use crate::gesture::{blit_centered, blit_gray, blit_left, blit_left_in, frame, mark_gray};
use crate::surface::{Surface, BLACK, FADED, WHITE};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    /// First frame after the panel is ours; holds through the escape window.
    Boot,
    /// Drawn before `systemctl poweroff`; the OS's poweroff.png is its twin.
    PowerOff,
    /// Drawn before `systemctl reboot`; the OS's rebooting.png is its twin.
    Restart,
}

impl Card {
    fn line(self) -> &'static str {
        match self {
            Card::Boot => "Getting the pen ready.",
            Card::PowerOff => "Powering off.",
            Card::Restart => "Restarting.",
        }
    }
}

/// Left spine every block hangs from, as on the sleep card.
const ML: usize = 150;
/// Where the wordmark band starts; the tests look for ink here.
pub const WORDMARK_Y: usize = (SCREEN_H - 1030) / 2 - 60 + 62 + 82;
/// The escape-hatch line's rect (x, y, w, h): drawn on the boot card only,
/// erased with one partial update once the window closes.
pub const HATCH: (usize, usize, usize, usize) = (ML, SCREEN_H - 120, SCREEN_W - 2 * ML, 40);
const HATCH_TEXT: &str = "POWER BUTTON NOW: REMARKABLE HOME";

/// Box-average a grayscale image down 3×. Averaging (not sampling) keeps
/// the mark's mid-gray stroke a mid-gray at a third of the size.
pub(crate) fn shrink3(w: usize, h: usize, gray: &[u8]) -> (usize, usize, Vec<u8>) {
    let (sw, sh) = (w / 3, h / 3);
    let mut out = Vec::with_capacity(sw * sh);
    for y in 0..sh {
        for x in 0..sw {
            let mut sum = 0u32;
            for dy in 0..3 {
                for dx in 0..3 {
                    sum += gray[(y * 3 + dy) * w + x * 3 + dx] as u32;
                }
            }
            out.push((sum / 9) as u8);
        }
    }
    (sw, sh, out)
}

/// Draw `card` over the whole surface. Caller pushes it with a full refresh.
pub fn draw(surf: &mut Surface, hand: &FontRef, ui_font: &FontRef, card: Card) {
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    frame(surf, 56, 56, SCREEN_W - 112, SCREEN_H - 112, 2);
    let rule_w = SCREEN_W - ML * 2;

    let mut y = (SCREEN_H - 1030) / 2 - 60;
    blit_left(surf, ui_font, "ANTHINK SLATE", 32.0, 9, ML, y);

    y += 62;
    surf.fill_rect(ML, y, rule_w, 1, BLACK);

    // The wordmark is typographic — one word, one weight. It must not read
    // as a copy of the Merge / COMBINATOR pairing on the sleep card.
    y += 82;
    debug_assert_eq!(y, WORDMARK_Y);
    blit_left(surf, ui_font, "ANTHINK", 150.0, 4, ML, y);

    y += 190;
    surf.fill_rect(ML, y, rule_w, 1, BLACK);

    // The parent brand, once: the venture line with the mark hung against
    // the right margin at a third of its sleep-card size.
    y += 25;
    let mut line_y = y + 40;
    if let Some((mw, mh, gray)) = mark_gray() {
        let (sw, sh, small) = shrink3(mw, mh, &gray);
        let mx = SCREEN_W - ML - sw;
        blit_gray(surf, mx, y, sw, sh, &small);
        line_y = y + (sh.saturating_sub(23)) / 2;
    }
    blit_left(surf, ui_font, "A MERGE COMBINATOR VENTURE.", 32.0, 7, ML, line_y);

    blit_centered(surf, hand, card.line(), 50.0, 0, SCREEN_W, SCREEN_H - 210);

    if card == Card::Boot {
        let (hx, hy, _, _) = HATCH;
        blit_left_in(surf, ui_font, HATCH_TEXT, 22.0, 6, hx, hy + 8, FADED);
    }
}

/// Paper over the hatch line once the escape window has closed.
pub fn erase_hatch(surf: &mut Surface) {
    let (x, y, w, h) = HATCH;
    surf.fill_rect(x, y, w, h, WHITE);
}
```

Add `mod splash;` to `src/main.rs` next to `mod script;`.

**Step 4: Run the tests**

Run: `cargo test splash`
Expected: 4 passed. If `boot_card_has_the_frame...` fails on the spine check because a glyph's left side bearing pokes below x=150 by a pixel or two, widen the exclusion start to `x=60, w=86`. If the wordmark band count is under 20 000, print the count and lower the threshold to what a real render gives minus 20%; the point is "not empty", not a magic number.

**Step 5: Eyeball it once**

Run: `cargo test splash -- --nocapture` is not enough to see it; write a snapshot the way the sleep test does. Add to the first test, before the asserts:

```rust
let path = std::env::temp_dir().join("g-pad-boot-card.png");
crate::dump_page(&surface_of(&mut buf), path.to_str().unwrap()).unwrap();
eprintln!("boot card: {}", path.display());
```

`dump_page` must be `pub(crate) fn dump_page` in `src/main.rs` (it is private today; change the one line). Open the PNG and check the composition against the design table. Then leave the snapshot line in place; the sleep test does the same.

**Step 6: Commit**

```bash
git add src/splash.rs src/main.rs
git commit -m "feat: the Anthink card — boot splash, power-off and restart sheets, one renderer"
```

---

### Task 3: The escape window in `power.rs`

**Files:**
- Modify: `src/power.rs` (after `impl Drop for PowerButton`)

**Step 1: Write the failing tests** (append to `src/power.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    struct Fake { presses_after: usize, calls: usize }
    impl PressSource for Fake {
        fn drain_pressed(&mut self) -> bool {
            self.calls += 1;
            self.calls > self.presses_after
        }
    }

    #[test]
    fn a_press_inside_the_window_escapes_early() {
        let mut dev = Fake { presses_after: 2, calls: 0 };
        let t0 = Instant::now();
        assert!(escape_window(&mut dev, Duration::from_secs(2)));
        assert!(t0.elapsed() < Duration::from_secs(1), "escape did not return on the press");
    }

    #[test]
    fn silence_runs_the_window_out_and_continues() {
        let mut dev = Fake { presses_after: usize::MAX, calls: 0 };
        let t0 = Instant::now();
        assert!(!escape_window(&mut dev, Duration::from_millis(120)));
        assert!(t0.elapsed() >= Duration::from_millis(120));
    }
}
```

**Step 2: Run to verify they fail**

Run: `cargo test power::tests`
Expected: compile error, `PressSource` and `escape_window` not found.

**Step 3: Implement**

Add `use std::time::{Duration, Instant};` to the imports, then:

```rust
/// Anything that can say "the power key was pressed since you last asked".
/// `PowerButton` is the real one; the tests use a fake.
pub trait PressSource {
    fn drain_pressed(&mut self) -> bool;
}

impl PressSource for PowerButton {
    fn drain_pressed(&mut self) -> bool {
        PowerButton::drain_pressed(self)
    }
}

/// The boot escape hatch: true if the power key is pressed within `window`.
/// Polls at 50ms; the key is grabbed, so nothing else sees the press.
pub fn escape_window<P: PressSource>(dev: &mut P, window: Duration) -> bool {
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        if dev.drain_pressed() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    dev.drain_pressed()
}
```

**Step 4: Run the tests**

Run: `cargo test power::tests`
Expected: 2 passed.

**Step 5: Commit**

```bash
git add src/power.rs
git commit -m "feat: a 3-second power-key watch the boot splash can hand the stock UI back from"
```

---

### Task 4: The boot card is the first frame

**Files:**
- Modify: `src/main.rs:726-729` (the "Blank page" block in `run()`)

**Step 1: Insert the card and the window before the blank page**

Replace

```rust
    // Blank page, with the corner button that opens the controls.
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    ui::draw_corner(&mut surf);
    disp.update_all(surf.w, surf.h);
```

with

```rust
    // The Anthink card is the first frame: it holds the panel through the
    // escape window and the rest of init, then the page replaces it.
    splash::draw(&mut surf, &font, &ui_font, splash::Card::Boot);
    disp.full_refresh(surf.w, surf.h);
    if let Some(ref mut pd) = power_dev {
        if power::escape_window(pd, Duration::from_secs(3)) {
            eprintln!("g-pad: power press in the escape window — stock UI this boot");
            disp.terminate();
            return Ok(());
        }
    } else if takeover {
        eprintln!("g-pad: no power button, so no escape window this boot");
    }
    splash::erase_hatch(&mut surf);
    let (hx, hy, hw, hh) = splash::HATCH;
    disp.update(hx as i32, hy as i32, hw as i32, hh as i32, false);

    // Blank page, with the corner button that opens the controls.
    surf.fill_rect(0, 0, SCREEN_W, SCREEN_H, WHITE);
    ui::draw_corner(&mut surf);
    disp.update_all(surf.w, surf.h);
```

`font` is the hand font already loaded at `src/main.rs:671`; `takeover` is the existing bool in scope. `disp.terminate()` is what the leave path calls at the end of `run()`; `ExecStopPost` in the unit then starts xochitl.

**Step 2: Build and test**

Run: `cargo build && cargo test`
Expected: builds; all tests pass (the count was 80+ before this work).

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: the pad wakes into the Anthink card and holds the escape window there"
```

---

### Task 5: `--render-cards`

**Files:**
- Modify: `src/splash.rs` (add `render_to`), `src/main.rs` (flag dispatch after `--learn-sheets`, and `USAGE`)

**Step 1: Write the failing test** (in `src/splash.rs` tests)

```rust
    #[test]
    fn render_to_writes_the_os_image_format() {
        let dir = std::env::temp_dir().join(format!("g-pad-cards-{}", std::process::id()));
        let (hand, ui) = fonts();
        let written = render_to(dir.to_str().unwrap(), &hand, &ui).unwrap();
        assert_eq!(written.len(), 3);
        for name in ["boot.png", "poweroff.png", "rebooting.png"] {
            let path = dir.join(name);
            let dec = png::Decoder::new(std::fs::File::open(&path).unwrap());
            let reader = dec.read_info().unwrap();
            let info = reader.info();
            assert_eq!((info.width, info.height), (SCREEN_W as u32, SCREEN_H as u32), "{name}");
            assert_eq!(info.color_type, png::ColorType::Grayscale, "{name}");
            assert_eq!(info.bit_depth, png::BitDepth::Eight, "{name}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
```

**Step 2: Run to verify it fails**

Run: `cargo test render_to_writes`
Expected: `render_to` not found.

**Step 3: Implement** (in `src/splash.rs`)

```rust
/// Write the three cards to `dir` as 1404×1872 8-bit grayscale PNGs — the
/// format of the images in /usr/share/remarkable, so poweroff.png and
/// rebooting.png can replace the OS's own. Returns the paths written.
pub fn render_to(dir: &str, hand: &FontRef, ui_font: &FontRef) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    let mut buf = vec![0xFFu8; SCREEN_W * SCREEN_H * 4];
    let ptr = buf.as_mut_ptr();
    let mut surf = Surface::new(ptr, buf.len(), SCREEN_W, SCREEN_H, SCREEN_W * 4, crate::surface::PixFmt::Rgb32);
    let mut out = Vec::new();
    for (card, name) in [(Card::Boot, "boot.png"), (Card::PowerOff, "poweroff.png"), (Card::Restart, "rebooting.png")] {
        draw(&mut surf, hand, ui_font, card);
        let path = format!("{dir}/{name}");
        crate::dump_page(&surf, &path)?;
        out.push(path);
    }
    Ok(out)
}
```

In `src/main.rs` `main()`, after the `--learn-sheets` arm:

```rust
        // Diagnostic: render the Anthink cards (boot, power-off, restart) as
        // the OS's own image format. install-boot-rm2.sh copies two of them
        // over /usr/share/remarkable. No display needed.
        Some("--render-cards") => {
            let dir = args.get(2).map(String::as_str).unwrap_or("/tmp/g-pad-cards");
            let Ok(ui_font) = FontRef::try_from_slice(ui::UI_FONT_TTF) else {
                eprintln!("g-pad: bundled UI font unreadable");
                std::process::exit(1);
            };
            let Ok(hand) = FontRef::try_from_slice(FONT_TTF) else {
                eprintln!("g-pad: bundled hand font unreadable");
                std::process::exit(1);
            };
            std::process::exit(match splash::render_to(dir, &hand, &ui_font) {
                Ok(paths) => {
                    for p in paths {
                        println!("{p}");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("g-pad: render cards: {e}");
                    1
                }
            });
        }
```

Add to `USAGE` after the `--learn-sheets` lines:

```
  g-pad --render-cards [DIR]  render the Anthink boot, power-off and restart
                              cards into DIR (default /tmp/g-pad-cards) as
                              the OS's grayscale PNG format; no display needed
```

**Step 4: Run the tests and the flag**

Run: `cargo test render_to_writes && cargo run --quiet -- --render-cards /tmp/g-pad-cards && file /tmp/g-pad-cards/*.png`
Expected: test passes; three paths printed; `file` reports `1404 x 1872, 8-bit grayscale`.

**Step 5: Commit**

```bash
git add src/splash.rs src/main.rs
git commit -m "feat: --render-cards writes the Anthink cards in the OS's own image format"
```

---

### Task 6: Power off and reboot show the card

**Files:**
- Modify: `src/main.rs:2354` (`system_tap` signature), `:1095` (its call site), the `Act::Reboot | Act::PowerOff` arm (~`:2531`)

**Step 1: Thread the hand font in**

Add `hand: &FontRef,` to `system_tap`'s parameters right after `ui_font: &FontRef,`. At the call site pass `&font,` right after `&ui_font,`.

**Step 2: Draw the card before `systemctl`, and keep it up on success**

Replace the `Outcome::Confirmed(confirmed)` body with:

```rust
            Outcome::Confirmed(confirmed) => {
                let (verb, card) = if confirmed == Act::Reboot {
                    ("reboot", splash::Card::Restart)
                } else {
                    ("poweroff", splash::Card::PowerOff)
                };
                eprintln!("g-pad: {verb} from system");
                // The card goes up first; the OS draws its twin once the
                // unit is stopped, so the panel never shows the page again.
                splash::draw(surf, hand, ui_font, card);
                disp.full_refresh(SCREEN_W, SCREEN_H);
                match std::process::Command::new("systemctl").arg(verb).status() {
                    Ok(s) if s.success() => return After::Stay,
                    Ok(s) => page.notice = Some(format!("{verb} failed: {s}").to_uppercase()),
                    Err(e) => page.notice = Some(format!("{verb} failed: {e}").to_uppercase()),
                }
            }
```

The failure arms fall through to the page redraw at the bottom of the function, so a failed call puts the SYSTEM page back with the notice.

**Step 3: Build and test**

Run: `cargo build && cargo test`
Expected: clean.

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: POWER OFF and REBOOT leave the Anthink card on the panel, not the page"
```

---

### Task 7: The SYSTEM page footer

**Files:**
- Modify: `src/system/draw.rs:122` (the footer `full_text` line)

**Step 1: Change the string**

```rust
    full_text(
        surf,
        font,
        &format!("ANTHINK SLATE  ·  A MERGE COMBINATOR VENTURE  ·  {}", view.facts.build),
        LABEL_PX,
        PAD,
        FOOTER_Y,
        BLACK,
    );
```

**Step 2: Run the draw tests**

Run: `cargo test system::draw`
Expected: pass.

**Step 3: Commit**

```bash
git add src/system/draw.rs
git commit -m "feat: the SYSTEM page signs itself ANTHINK SLATE, a Merge Combinator venture"
```

---

### Task 8: The boot script's fallback window

**Files:**
- Modify: `scripts/g-pad-boot.sh:19-31`

**Step 1: Replace the dead `timeout` window**

Replace everything from the `systemctl stop xochitl` line to the `exec` line with:

```bash
systemctl stop xochitl

# The escape window normally lives inside g-pad: it draws the Anthink card,
# then watches the power key for 3s. That needs a binary that starts. If it
# cannot even print its version, hold the window here on the frozen stock
# frame instead, with bash's own timed read (OS 3.28 has no `timeout`).
if ! "$HERE/g-pad" --version >/dev/null 2>&1; then
    echo "g-pad-boot: binary does not start; escape window on the stock frame"
    if IFS= read -r -t 3 -N 16 _ < /dev/input/event0; then
        echo "g-pad-boot: power press in the escape window — stock UI this boot"
        systemctl start xochitl
        exit 0
    fi
fi

exec bash "$HERE/g-pad-takeover.sh"
```

Update the header comment's first hatch bullet to: `press the power button within ~3s of the Anthink card appearing: this boot stays on the stock UI (if the binary is broken, the window is held on the frozen stock frame by this script alone)`.

**Step 2: Test the fallback on the laptop**

```bash
tmp=$(mktemp -d) && printf '#!/bin/sh\nexit 1\n' > "$tmp/g-pad" && chmod +x "$tmp/g-pad"
printf '#!/bin/sh\necho takeover-ran\n' > "$tmp/g-pad-takeover.sh"
sed -e 's#systemctl stop xochitl#true#' -e 's#systemctl start xochitl#echo stock-restarted#' \
    -e "s#/dev/input/event0#$tmp/keys#" scripts/g-pad-boot.sh > "$tmp/g-pad-boot.sh"
head -c 16 /dev/zero > "$tmp/keys"
bash "$tmp/g-pad-boot.sh"
```

Expected output ends with `stock-restarted` (16 bytes on the fake device count as a press). Then `: > "$tmp/keys"` and re-run: expected `takeover-ran` after about 3s (an empty file makes `read` hit EOF immediately on a regular file, so if it returns at once, test the delay with `mkfifo "$tmp/keys"` and a background `sleep 5 > "$tmp/keys" &`).

Run `shellcheck scripts/g-pad-boot.sh` if shellcheck is installed; fix anything it flags.

**Step 3: Commit**

```bash
git add scripts/g-pad-boot.sh
git commit -m "fix: the boot escape window works again on OS 3.28, and falls back to the script when the binary will not start"
```

---

### Task 9: Unit ordering, installer images, doctor check

**Files:**
- Modify: `scripts/g-pad-takeover.service`, `scripts/install-boot-rm2.sh`, `scripts/rm2-doctor.sh` (before the verdict section, after the bundle check)

**Step 1: The unit**

Under `After=xochitl.service` add:

```ini
# The OS draws poweroff.png / rebooting.png after everything that holds the
# panel has let go. Stop us before those units start, or they fail to lock
# the framebuffer and the shutdown screen is whatever we left.
Before=remarkable-shutdown.service remarkable-reboot.service
```

Replace the `ExecStopPost` line with:

```ini
ExecStopPost=/bin/sh -c '[ "$(systemctl is-system-running)" = stopping ] || systemctl start --no-block xochitl'
```

**Step 2: The installer** — append before the final `echo`:

```bash
# The OS's own power-off and reboot images become the Anthink cards, rendered
# by the host build so they match what the pad draws pixel for pixel. The
# stock files are kept once under /home/root, which survives OS updates;
# the images themselves do not (see the caveat above) — rerun after an update.
CARDS=dist/cards
cargo run --quiet --release -- --render-cards "$CARDS" >/dev/null
$SSH 'mkdir -p /home/root/g-pad-stock-images && for f in poweroff rebooting; do
    [ -e /home/root/g-pad-stock-images/$f.png ] || cp /usr/share/remarkable/$f.png /home/root/g-pad-stock-images/; done'
for f in poweroff rebooting; do
    $SSH "cat > /usr/share/remarkable/$f.png" < "$CARDS/$f.png"
done
```

Update the installer's header caveat to mention the images: `…drops the unit from /etc/systemd/system and restores the stock shutdown images — rerun this after an update.`

**Step 3: The doctor** — add after the bundle check:

```bash
say "Anthink shutdown images"
if rm_ssh 'test -e /home/root/g-pad-stock-images/poweroff.png'; then
    if rm_ssh 'cmp -s /usr/share/remarkable/poweroff.png /home/root/g-pad-stock-images/poweroff.png'; then
        bad "the OS shutdown images are back to stock — an OS update restored them"
        fix "./scripts/install-boot-rm2.sh   # re-renders and re-installs the Anthink cards"
    else
        ok "power-off and reboot images are the Anthink cards"
    fi
else
    warn "Anthink shutdown images were never installed (install-boot-rm2.sh does it)"
fi
```

Check `say`, `ok`, `bad`, `warn`, `fix` exist with those names at the top of the doctor (lines ~38-44); match whatever is there.

**Step 4: Lint**

Run: `shellcheck scripts/install-boot-rm2.sh scripts/rm2-doctor.sh` (if installed) and `systemd-analyze verify` is not available on macOS, so re-read the unit for typos.

**Step 5: Commit**

```bash
git add scripts/g-pad-takeover.service scripts/install-boot-rm2.sh scripts/rm2-doctor.sh
git commit -m "feat: the OS's shutdown screens become the Anthink cards, and the doctor notices when an update takes them back"
```

---

### Task 10: Docs

**Files:**
- Modify: `docs/rm2-setup.md` ("After a reMarkable OS update" list), `README.md` (one line near the top, after the naming paragraph)

**Step 1:** In the OS-update list add a bullet: `**The shutdown images revert to stock.** Rerun ./scripts/install-boot-rm2.sh; the doctor reports it.` In README add: `The pad boots into an Anthink card and sleeps on the Merge Combinator mission card; POWER OFF and REBOOT leave a matching card on the panel.`

**Step 2: Commit**

```bash
git add docs/rm2-setup.md README.md
git commit -m "docs: the boot card, the escape window, and the shutdown images after an OS update"
```

---

### Task 11: Build, deploy, hardware checks

No code. The tablet is at `root@192.168.1.78` (Wi-Fi) or `rm2` (USB). Use `/usr/bin/ssh` and `/usr/bin/scp -O`; put remote commands in a script file and run `bash -s < file`. Never `systemctl restart g-pad-takeover` — stop, wait for the panel to settle, then start.

1. `./build-takeover-zig.sh && DEVICE=rm2 ./scripts/make-bundle.sh`
2. `RM_HOST=192.168.1.78 ./scripts/install-boot-rm2.sh` (unit, boot script, images)
3. Copy the binary: `scp -O dist/rm2-takeover/g-pad/g-pad root@192.168.1.78:/home/root/xovi/exthome/appload/g-pad/g-pad.new` then on the tablet `mv g-pad.new g-pad` (avoids ETXTBSY), `systemctl daemon-reload`.
4. `systemctl stop g-pad-takeover`; wait 5s; `systemctl start g-pad-takeover`. Expect the Anthink card, the hatch line for 3s, then the page. Journal: `journalctl -u g-pad-takeover -b --no-pager | tail`.
5. Stop again, start, press power within 3s. Expect the stock UI and the journal line `power press in the escape window`.
6. Start g-pad. SYSTEM → POWER → REBOOT (tap twice to confirm). Expect the Restart card, then the OS's identical image, then a boot into the card. After boot check `journalctl -b -1 -u remarkable-reboot --no-pager` shows no "Failed to lock" line.
7. On the tablet: `cp /home/root/g-pad-stock-images/poweroff.png /usr/share/remarkable/poweroff.png`, run `./scripts/rm2-doctor.sh`, expect the "back to stock" finding, then rerun `install-boot-rm2.sh`.
8. Push the branch: `git push origin worktree-quill-3-28`.
