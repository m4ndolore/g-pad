# anthink-hub — the homelab side of Agent mode

**Status:** Design, validated 2026-08-30. Implements the missing half of
`docs/claude-bridge.md` and `docs/anthink-interaction.md`: nothing feeds
`bridge::replace`, and nothing on the homelab exports session state. This
sidecar is both.

## Placement

The rM2 reaches the laptop on LAN and nothing else, so the hub runs on the
laptop. Other homelab clients are reached *by the hub* over SSH, never by the
pad. One Rust binary, in this repo, sharing nothing with the pad build except
the JSON shape `bridge::parse_sessions` already accepts.

## Endpoints

| | |
|---|---|
| `GET /sessions` | The bridge payload: sessions across all clients, most-in-need-of-a-human first. |
| `POST /sessions/<id>/nudge` | Body `{"mark": "tick" \| "strike" \| "text", "text"?: "…"}`. |

## Where session truth comes from

Two sources, deliberately split:

- **Content** — title, last exchange, artifacts — from Claude Code's
  transcript JSONL under `~/.claude/projects/<slug>/`. Artifacts are
  references (shas, paths, exit codes) extracted from tool results, never the
  model's prose about them.
- **State** — `RUNNING` / `WAITING` / `DONE` — from `tmux capture-pane` on the
  pane running that session. A permission prompt or an idle prompt line is
  visible in the pane; that is ground truth, not self-report.

Classification is conservative: unrecognized pane content reads as `RUNNING`,
never `WAITING`. A false "needs you" spends the reviewer's attention; a false
"running" only delays.

## The nudge

The pad POSTs a mark; the hub owns what keys it becomes:

- **tick** → the keystroke that accepts the pending prompt.
- **strike** → escape / reject.
- **text** → typed into the pane, then Enter. This is the redirect: answer a
  question, unstick a session, say "keep going."

The pad never learns tmux exists. When a hooks-based approval path replaces
injection later, the pad does not change.

Until the hub classifies consequence tiers, every pending action reports as
destructive and the pad confirms in vermilion, per `anthink-interaction.md`.

## Failure honesty

- Hub unreachable → the pad shows held sessions with age (`bridge.stale`).
- A client's SSH failure marks that client's sessions stale; state is never
  invented.
- A failed nudge POST renders as a failure on the turn page. The pad never
  assumes a mark landed.

## Config

`hub.toml` next to the binary:

```toml
[[client]]
name = "laptop"          # local, no ssh
projects = "~/.claude/projects"

[[client]]
name = "gauntlet"
ssh = "root@gauntlet"
projects = "~/.claude/projects"
```

## Pad wiring (same sprint, separate commits)

1. Poll thread — `ureq` GET on `RIDDLE_BRIDGE_URL`, feeding
   `bridge::replace`. The pad polls; no streaming.
2. Turn page — tick a Sessions-drawer row opens `bridge::layout_session`
   full-page. Built and tested already; this adds the call site.
3. Decision box — anchored tick/strike at the foot of the turn page, POSTing
   the nudge. Ruled handwriting transcribes through the existing oracle path
   and POSTs as `text`.

## Testing

- Hub: fixture-driven unit tests for transcript parsing and pane
  classification, fixtures captured from live sessions.
- Pad: parse/layout/hit-map tests in the existing style; no hardware needed.
- End to end: hub + one real tmux Claude Code session; board → turn → nudge
  round trip. Hardware bundle last.

## Out of scope tonight

Consequence-tier classification at the source, the hooks-based approval
transport, Learn mode, streaming, any typed input on the pad.
