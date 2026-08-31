# The Claude bridge

**Status:** Design. Tier 1 only — reading. See "Not this build".

The pad reads agent sessions. Flip through what Claude Code is doing, read the
last exchange, and — later — answer it in your own hand.

This is the second reading surface after the daily brief, and it is built the
same way: a bounded page, no navigation into arbitrary content, no rendering
the pad cannot finish. The brief reads a feed a brain selected. This reads a
set of sessions an agent is running.

## Why a session reader and not a chat client

The pad has no keyboard, and it should not grow one. Every input path in this
tree is the same: pen strokes, a ruled line, a vision model that reads the ink.
That is not a limitation to route around — it is the reason the device is worth
carrying. A chat client on e-ink is a worse laptop.

So the bridge is not "Claude Code on the tablet". It is **paper that an agent
writes to and you write back on**. Latency is a round trip through
transcription, and the design has to want that rather than hide it. A surface
that pretends to be a terminal will fight the hardware on every frame; one that
behaves like correspondence will not.

That framing also sets the scope. Reading is most of the value and nearly free
— the layout engine already exists. Responding is a full capture cycle, which
is a different build with different risks. They ship separately.

## Prompts, responses, and the thing in between

`capture-record.md` draws a hard line:

> **Prompts are evidence. Responses are not.** A brain that cites its own past
> answers manufactures corroboration, and that compounds.

An agent session does not sit cleanly on either side of that line, and getting
this wrong is the failure that would not show up for months.

Claude Code produces two different kinds of output:

| What | Example | Standing |
|---|---|---|
| **Prose** | "I think the drawer should be full-width." | Response. Commentary. Never evidence. |
| **Artifact** | A diff, a commit sha, a command's exit status | **Fact about the world.** |

A model's opinion about the code is commentary no matter how confident it
sounds. But a commit that exists, exists — the repository is the witness, not
the model. Collapsing those two into one `kind` either poisons the corpus with
self-citation or throws away the only agent output worth keeping.

So the record gains a third kind:

```yaml
kind: artifact          # prompt | response | artifact
```

Rules:

- **`artifact` is evidence, but only by reference.** The record stores what the
  artifact *is* and where it lives — a commit sha, a path, an exit code — never
  the agent's description of it. `"fixed the parser"` is prose. `a1b2c3d` is an
  artifact. If a claim cannot be named by a reference something else can check,
  it is prose.
- **Agent prose is `kind: response`,** with the same standing as an oracle
  reply. It rides along as conversation context and never ranks as retrieval
  evidence.
- **Provenance is not optional.** An artifact record carries the session it came
  from and the agent that produced it. Evidence whose origin cannot be named is
  not evidence.
- **The pad never mints artifacts.** It renders what the bridge reports. A pad
  that decides for itself that something was accomplished is the ungrounded
  confidence failure mode wearing a friendly UI.

The asymmetry from the capture record survives intact: what happened is
evidence, what something says about what happened is not.

## Annotation is the same door

Marking up a session is the brief's Tier 2 gesture against a different
document:

```yaml
context:
  type: document
  ref: sessions/<session-id>#<turn>
```

No new shape. This is the second time `context` has absorbed a reading surface
without reshaping the record, which is the test the capture record set itself.

## The page

One session per page. Header, the last exchange, a footer that says what was
left out.

```text
g-pad · claude-bridge                          3 more sessions

Direct Claude Code from the pad
running · 14:02

  Baseline is green — 58 passed. Writing the design doc
  first, since it settles the evidence question that
  governs the code…

a1b2c3d  docs: the Claude bridge
```

Rules, inherited from the brief because the constraints are identical:

- **Fits the screen.** No scrolling on the first page. If it does not fit,
  fewer turns — never a smaller face.
- **Provenance always visible.** Session name and state, the same way the brief
  always shows source and date.
- **Say what was left out.** Unshown sessions are counted on the page.
- **A session with nothing to read is not shown.** The brief's rule about
  excerpt-less items; a session header with no exchange under it reads as
  broken.
- **Artifacts render as references.** A sha, a path, a status — set apart from
  prose, because the record distinguishes them and the page should too.

Sessions are longer than brief excerpts, which makes this the harder test of
the question the brief posed: does long-form read at all through takeover?

## Implementation

Same answer as the brief. Everything needed exists:

| Need | Exists |
|---|---|
| Text layout | `script::wrap` over `ab_glyph` |
| Measure-until-it-fits paging | `brief::layout_page`, same shape |
| A read-only surface | `ui::Drawer`, with saved-region restore |
| Display update | `display::update` |

`bridge.rs` is a layout and parse module with no display coupling, exactly as
`brief.rs` is. The drawer gains a `Sessions` kind. Fetch rides the HTTP path
the oracle already uses.

A bridge that cannot be reached shows the last sessions it held, with their
age. A stale page that says it is stale beats an empty one.

## Not this build

- **Responding.** Tier 2. It is a full capture cycle — ink, transcription, a
  post back to the session — and it should not ship until reading is known to
  work on hardware.
- **A keyboard, or any typed input.** Not now, not later. See above.
- **Streaming.** The pad polls. A live-updating e-ink page is a waveform
  problem, not a feature.
- **Orchestrating g-stack.** The least defined branch and the one whose scope
  grows without bound. Nothing here is blocked on it.
- **Arbitrary tool output.** Long command logs are not reading material. The
  bridge shows the exchange and names artifacts; it is not a terminal.
- **Approving tool calls from the pad.** Consequential actions should not hang
  off a transcription round trip.

## Open

- **What a "session" is on the page** — one per page, or a selector like
  HISTORY's conversation list. Wants hardware time.
- **Poll interval.** Frequent enough to be current, rare enough not to churn
  the display or the battery.
- **How much of a turn to show.** The brief capped excerpts at three lines;
  agent turns are longer and the right cap is a hardware question.
- **Whether the drawer's half-width panel is enough,** or whether reading wants
  the full page. The brief left the same question open; the answer should be
  the same for both.

## Sources

- `capture-record.md` — the record this extends, and the evidence rule the
  third kind had to be reconciled with.
- `daily-brief.md` — the first reading surface. The layout discipline and the
  "say what was left out" rule are inherited wholesale.
