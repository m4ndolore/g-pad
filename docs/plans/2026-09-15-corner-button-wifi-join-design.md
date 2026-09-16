# Corner button, group bands, Wi-Fi join — design

**Date:** 2026-09-15. **Status:** agreed with the user in conversation; built on
branch worktree-quill-3-28 after the SYSTEM page shipped on OS 3.28.

## The ask

Three things from the first day of the SYSTEM page on hardware:

1. Surfaces open on finger or palm contact (the top-edge swipe). The user
   wants a reMarkable-style toolbar button: touch a target, the controls
   open.
2. Group heads (SAVED, IN RANGE, PRESET, MODE) now show their members with a
   rule and an indent; a gray head would make the grouping plainer still.
3. An in-range network cannot be joined. The page had no way to take a
   password, by the earlier "presets and steppers only" rule; the user wants
   to join new networks from the page.

## Decisions

- **Keep the Guided strip; add a corner button.** Not a new toolbar, no lasso
  or delete tools in this pass. The button shows in both modes and does what
  the top-edge swipe does per mode (Guided: the strip; Stealth: the SYSTEM
  page). All existing swipes stay. The user chose this over dropping the
  swipes and over a single-mode redesign.
- **Gray head bands.** A light gray band behind a group head's label, full
  width, replaces the minor rule; members stay indented. The one gray in
  the UI, used for structure only.
- **On-page tap keyboard for joins.** An unsaved in-range row opens a join
  sheet with a bottom-anchored keyboard; JOIN runs one `nmcli dev wifi
  connect <ssid> password <psk>` on the Wi-Fi worker. Open networks join on
  tap with no sheet. The password shows in clear. The keyboard is a pure
  model in its own module so a later field can reuse it; nothing else uses
  it now.

## The corner button

- Top-left, an 82×82 box (the strip's height), glyph `≡` in the label size,
  painted with the page and repainted after anything that repaints the page.
- Only `Gesture::Tap` inside the box acts; pen strokes there are ink.
- Guided: opens the strip (same path as `OpenControls`). While the strip is
  open the corner cell reads `×` and a tap there retracts it; the 12 s
  auto-retract stays. Stealth: opens the SYSTEM page.
- Known risk: a palm that lands and lifts without travelling is a Tap. The
  box is small and in a corner; if it still triggers, cap the corner tap's
  contact time. Shipped as its own commit so it can be backed out alone.

## The join sheet

- `Page.join: Option<Join>` — `Join { ssid, secured, text, shift, symbols }`.
  `Act::WifiNew(u32)` on an unsaved row starts it (index into `seen`);
  `Act::Key(char)`, `Act::KeyShift`, `Act::KeySymbols`, `Act::KeyBackspace`,
  `Act::JoinCancel`, `Act::JoinGo`.
- Drawing: the section header stays; under it the SSID, a field row with the
  text and a caret, then the keyboard from the bottom up: four rows of
  82 px keys across 1404 px (digits; QWERTY; ASDF with SHIFT; ZXCV with
  SYMBOLS, SPACE, BACKSPACE) and a last row CANCEL · JOIN. Keys are outlined
  like the steppers; SHIFT and SYMBOLS fill when on.
- `wifi::Cmd::Join { ssid, password: Option<String> }` → `nmcli --wait 15
  dev wifi connect <ssid> [password <psk>]`, then refresh. NetworkManager
  saves the connection, so it appears under SAVED afterwards.
- The password passes on nmcli's command line, briefly visible to a process
  listing on the tablet. Same trust boundary as oracle.env; accepted.

## Tests (host)

- Hit map: the corner box exists on a fresh page in both modes; reads as
  close while the strip is open.
- Keyboard model: typing, shift (one-shot), symbols page, backspace, and
  every painted key has a region.
- Join: an unsaved secured row yields the sheet, an unsaved open row yields
  `Cmd::Join` with no password, a saved row still yields `Cmd::Select`.
- Draw: a head band is painted where the rule was.

## On the tablet (the user's checks)

Tap the corner with a finger and with the side of a palm; join one new
network; look at the heads.
