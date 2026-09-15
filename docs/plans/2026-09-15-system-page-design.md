# SYSTEM page: in-app config, Wi-Fi, device facts, power — design

Date: 2026-09-15. Status: designed, not yet built.

## The ask

Better menu controls so the writer leaves g-pad less often. Two things force
them out today, in the user's own ranking:

1. **Reaching stock reMarkable features** — Wi-Fi (status, pick a network,
   reconnect), device facts (battery, storage, time), and power control
   beyond the power button's suspend.
2. **Changing config** — model, endpoint, tokens, palm holdoff, URLs. All of
   it lives in `oracle.env`, which is sourced once by `g-pad-takeover.sh`,
   so every change is an ssh session plus a stop→settle→start.

Input rule chosen for config: **presets and steppers only**. No keyboard, no
handwriting transcription of values. Secrets and new URLs still arrive over
ssh; every routine change happens on the page.

## Approaches considered

- **A SYSTEM tab in the drawer.** Reuses the tab/scroll scaffolding, but the
  drawer is half-width and a reading surface; network names and stepper rows
  need the full column. Rejected.
- **A persistent status strip** (battery + Wi-Fi always visible, tap to
  expand). Fastest glance, but `docs/anthink-interaction.md` §Switching modes
  rules out persistent chrome, and it steals canvas height. Rejected.
- **A full-page SYSTEM surface** grown from today's half-width Settings
  panel, reached exactly as Settings is (top/bottom edge swipe in Stealth,
  the SETTINGS cell of the control strip in Guided). No new chrome, room for
  lists and steppers. **Chosen.**

## The page

`State::System { section, saved, return_to }` replaces `State::Settings`.
The header carries six section labels tapped like drawer tabs; one section
is drawn at a time:

| Section | Rows |
|---|---|
| ORACLE | PRESET picker (Vellum / Gemini / OpenAI / OpenRouter, or CUSTOM showing the base) · MODEL stepper (per-preset list) · ASK MODEL stepper · REASONING stepper · MAX TOKENS stepper · KEY: SET / MISSING (never the value) · RESET OVERRIDES |
| INPUT | STEALTH / GUIDED · IDLE-SEND on/off + delay stepper · PALM HOLDOFF stepper (0, 250, 500, 750, 1000, 1500 ms) |
| LEARN | KIDS LEARN MODE on/off · TUTOR MODEL picker · NEXT-PAGE DWELL stepper |
| WI-FI | status line (SSID · IP · signal, or DISCONNECTED) · RESCAN · saved networks (tap = select, wait for the join, re-enable the rest) · in-range list after a rescan, saved ones marked, unsaved rows inert with a one-line "add over ssh" note |
| DEVICE | battery % + charging · free space on /home · clock (configured offset) · OS version · running build (short git hash, compile-time) · HUB: last poll age or UNREACHABLE |
| POWER | SLEEP (existing suspend + sleep card) · LEAVE TO STOCK UI (existing quit) · REBOOT · POWER OFF |

LEVEL and the skills picker stay on the Learn menu; that page is the
configuration surface for Learn (`2026-08-31-learn-config-scoring-design.md`).
One owner per setting.

Close: corner ×, leftward swipe, or the page's DONE box. Five-finger hold
still exits everywhere.

**Hit map is a returned value of drawing.** Each `draw_system_*` returns the
regions it painted (the `learn::sheet::HitMap` pattern), so tap targets
cannot drift from the pixels. Steppers are minus/plus boxes around the
value; a tap repaints the section (rows can appear or vanish, so the page
redraws whole with the non-flashing partial update).

**Destructive rows** (REBOOT, POWER OFF) use the turn page's arm-then-confirm:
the first tap inverts the row in vermilion, a second tap within five seconds
acts, any other tap or the timeout disarms. `docs/ux-vignelli.md` reserves
vermilion for destructive confirmation; this is its first use.

**Palm rejection** is suppressed while SYSTEM is open, as for every overlay
today, so a hovering marker does not fight finger taps.

## Config becomes live without a restart

Every config value is read from the process environment, and the oracle
client is built by one call (`oracle::Oracle::spawn`) that reads it. So the
environment stays the config bus:

1. `g-pad-takeover.sh` sources `oracle.env` as today.
2. At startup the app reads `/home/root/g-pad-data/overrides` (plain
   `key=value`, same format as `preferences`) and `set_var`s each line —
   **overrides beat `oracle.env`** without touching it.
3. On a change from the page: `set_var`, save the file, and re-spawn the
   oracle. `palm_holdoff`, idle-send and the learn dwell are re-read after
   any SYSTEM change instead of being captured once at boot.
4. RESET OVERRIDES deletes the file and re-spawns from `oracle.env` alone.

`set_var` at runtime is accepted as is: a review found no background thread
calling `env::var` after spawn (bridge, oracle and vault read the environment
on the calling thread at spawn), and the residual libc `getenv` race — the
glibc resolver on the vault poll thread during a `setenv` that grows
`environ` — is taken on for now rather than restructuring how config is passed.

Presets come from `settings.schema.json`, already shipped in the bundle and
consumed by nothing on-device. It gains a **Vellum** preset matching the
tablet's current `oracle.env`, and each preset gains a short `models` list
for the MODEL stepper. `RIDDLE_OPENAI_KEY` is never written by the page.

## Wi-Fi, device, power mechanics

- Wi-Fi drives `wpa_cli -i wlan0` (`status`, `signal_poll`, `list_networks`,
  `scan` + `scan_results`, `select_network N`, wait for COMPLETED, then
  `enable_network all`) — the tool `power::wifi_heal` already uses after
  resume. Every call runs on a worker thread and reports through the event
  loop like the oracle stream; a slow scan never blocks the draw loop. Rows
  repaint in place as results land.
- Battery and charging from `/sys/class/power_supply/*/{capacity,status}`;
  storage from `statvfs("/home")`; OS version from `/etc/os-release`.
- The running build hash is embedded at compile time (`option_env!` fed by
  `build-takeover-zig.sh`).
- HUB reads the bridge poller's last-success instant; no new network call.
- REBOOT / POWER OFF shell to `systemctl reboot` / `systemctl poweroff`.

## Error handling

The page never goes blank. Unparsable override lines are skipped and logged.
If the oracle fails to re-spawn, the old client stays and ORACLE's status line
shows the error until the next successful change. A wpa_cli failure prints
its first line in the WI-FI status row. A failed systemctl disarms the row
and shows the error.

## Tests

Pure parts, no hardware: overrides parse / serialize / replay precedence over
the environment; preset application; stepper cycling at both ends; every
`draw_system_*` hit map against its painted rows; the wpa_cli parsers
(`status`, `list_networks`, `scan_results`) over captured fixture strings;
the arm-then-confirm state machine with a fake clock.

## Hardware verification (open at design time)

The tablet was asleep while this was designed. Confirm before building the
WI-FI section: whether `wpa_cli` or `connmanctl` is the right tool on OS
3.27.3; the exact battery sysfs path; scan latency.
