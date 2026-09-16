# Anthink launch branding — design

Date: 2026-09-16. Base branch: `worktree-quill-3-28` (what the rM2 runs on OS 3.28).

## Decisions

- **The boot splash is Anthink. The sleep card stays Merge Combinator.** Two
  faces for two moments: the pad announces itself as the Anthink Slate when it
  starts, and shows the Merge Combinator mission card when it sleeps. The
  sleep card in `src/gesture.rs` does not change.
- **Branding reaches as far back as the OS lets it.** The bootloader logo
  lives in flash and cannot change. `starting.png` is dead on OS 3.28 (no
  binary references it), so it is left alone. `poweroff.png` and
  `rebooting.png` are drawn by `/usr/bin/remarkable-shutdown` after xochitl
  and g-pad have let go of the panel, so they are replaced with Anthink cards.
  `suspended.png` is xochitl's own sleep image and never shows under g-pad.
- **xochitl still starts at boot.** Suppressing it would remove the brief
  stock-UI flash, but after an OS update xochitl is what marks the new root
  partition good; if it never runs the bootloader falls back. Not worth it.
- **The escape window moves into g-pad, with a script fallback.** The old
  window in `g-pad-boot.sh` ran on the frozen stock frame with a background
  `dd` (OS 3.28 removed `timeout`; main's copy still uses it and is dead
  there). g-pad already grabs the power key, so the window can sit under the
  card; the script keeps a window of its own for a binary that will not start.
- **SYSTEM page footer** becomes `ANTHINK SLATE · A MERGE COMBINATOR VENTURE ·
  <build>`.

## The cards

One renderer (`src/splash.rs`) draws three variants onto any `Surface`, using
the sleep card's grammar: white page, 2px hairline frame inset 56px, left
spine at x=150, 1px rules, Liberation Sans for set type, and the reply hand for
the one human line (Liberation Sans by default; cursive only when
`RIDDLE_FONT_FILE` opts in), exactly as the sleep card does.

Top to bottom (y from `(SCREEN_H - 1030) / 2 - 60`):

| element | type | notes |
|---|---|---|
| `ANTHINK SLATE` | 32px, tracking 9, flush left | eyebrow |
| rule | 1px | |
| `ANTHINK` | 150px, tracking 4 | typographic wordmark; no logo asset exists |
| rule | 1px | |
| MC mark | grayscale PNG shrunk 3× (206×121) | right-hung against the right margin |
| `A MERGE COMBINATOR VENTURE.` | 32px, tracking 7 | vertically centered on the mark |
| human line | 50px reply hand, centered, y = SCREEN_H - 210 | varies by card |
| hatch line | 22px, tracking 6, FADED gray, at (150, SCREEN_H - 120) | boot card only |

Human line per card: Boot `Getting the pen ready.`, PowerOff `Powering off.`,
Restart `Restarting.`. Hatch line: `POWER BUTTON NOW: REMARKABLE HOME`, shown
only during the escape window and erased with one partial update afterwards.

The card is drawn once with a full refresh. Nothing on it changes while it
is up except the hatch erase.

## Launch routine

From power-on: bootloader logo → stock home UI while xochitl settles →
**Anthink boot card** → writing page.

1. `g-pad-boot.sh` stops xochitl as today. It then runs `g-pad --version`;
   if the binary cannot even do that, it holds the old 3s window on the frozen
   stock frame using bash's `read -t 3 -N 1 < /dev/input/event0` (bash 5.2
   is on the tablet; `timeout` is not). One byte, not sixteen: `read` drops
   NUL bytes and a power event is mostly NULs, and event0 carries only the
   power key. Otherwise it hands over immediately.
2. g-pad draws the boot card as its first frame, replacing the blank white
   fill, right after the display and the power device open.
3. g-pad watches the grabbed power device for 3 seconds. A press means
   "stock UI this boot": log it, terminate the display, return from `run()`.
   The unit's `ExecStopPost` starts xochitl as it does for any exit.
4. The hatch line is erased. Init continues (memory, preferences, oracle
   warm-up). The page is drawn as today.
5. `POWER OFF` / `REBOOT` on the SYSTEM page draw the matching card with a
   full refresh before calling `systemctl`, and skip the page redraw on
   success so the card stays up until the unit is stopped.

A binary that hangs inside display init still has no hatch. That gap exists
today and stays documented (`touch /home/root/g-pad-boot-off` over ssh, or
hold power to force off).

## OS images, installer, doctor, unit

- `g-pad --render-cards [DIR]` draws the three cards offscreen and writes
  `boot.png`, `poweroff.png`, `rebooting.png` as 1404×1872 8-bit grayscale
  PNGs, the stock files' exact format. Runs on the laptop's host build with
  `--features rm2`, which selects the rM2 panel size.
- `scripts/install-boot-rm2.sh` renders the cards, backs up the stock
  `poweroff.png` and `rebooting.png` once to `/home/root/g-pad-stock-images/`,
  then copies the branded ones over `/usr/share/remarkable/`. `restart-crashed.png`
  is a symlink to `rebooting.png`. Re-running is a no-op when the files match.
- `scripts/rm2-doctor.sh` notes when the live images equal the backups
  ("stock images restored by an OS update, rerun install-boot") or when no
  backup exists (images never installed).
- `scripts/g-pad-takeover.service` gains
  `Before=remarkable-shutdown.service remarkable-reboot.service` so g-pad is
  stopped (and the panel lock released) before the OS draws its image, and
  its `ExecStopPost` skips restarting xochitl while the system is stopping.

## Failure rules

Branding never blocks boot. A card draw error logs and falls through to the
page. A missing power device skips the window with a log line. The installer
backs up whatever file is present before overwriting.

## Testing

Host tests: card renderer (frame at the inset, wordmark ink inside the spine
column, hatch only on Boot, mark keeps ≥ 8 gray levels); `--render-cards`
round-trip (dimensions and color type); `escape_window` over a fake press
source (press → true early, silence → false after the window); shellcheck on
the boot script plus a host bash test of the fallback path with a fake
failing binary.

Hardware, once: cold boot shows the card; power press inside the window
lands on stock; POWER OFF and REBOOT show the card and the OS follows with
the same image; the doctor flags a reverted image.
