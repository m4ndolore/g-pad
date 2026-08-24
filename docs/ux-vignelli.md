# g-pad — UX system

## Intent

The product is a writing instrument first, not a chatbot window. The blank
page is the primary interface. Controls appear only when the writer asks for
them, and every consequential action has a visible, reversible command.

Semantics: a private paper pad that can answer back.

Appropriateness: e-ink rewards quiet composition, explicit timing, and a small
number of high-confidence gestures.

Timelessness: the system should still feel obvious when the model, corpus, or
display backend changes.

## The two modes

### Stealth

- The page is completely blank at rest.
- Pen strokes remain indefinitely until the writer draws the send rule.
- No automatic send by default; idle-send is an opt-in setting.
- A reply starts at the top writing line and remains until the writer
  dismisses it or starts a new turn.
- Pen eraser remains the only always-available tool affordance.

### Guided

- A four-column, eight-module control strip appears after a deliberate
  top-edge swipe or corner tap; it retracts after inactivity.
- Controls are text-plus-pictogram, flush-left, and ordered:
  `SEND` · `ERASE` · `NEW PAGE` · `RECALL` · `SLEEP`.
- The strip never occupies the writing field. It is a wayfinding surface, not
  a permanent toolbar.
- `SEND` commits the current page. `NEW PAGE` asks for confirmation before
  discarding unsent ink. `DISMISS` is present while a reply is visible.
- Mode selection lives in the strip under `SETTINGS`: `STEALTH` or `GUIDED`.

## Conversation drawer

The canvas is not replaced by a chat screen. Conversation history is a
secondary drawer, recalled with a deliberate edge swipe from the left margin
or `HISTORY` in the Guided strip.

- Closed state: no tab, icon, or persistent chrome; the paper stays untouched.
- Open state: a half-page drawer. HISTORY opens the current sitting as a
  message thread. `←` or the HISTORY tab returns to a conversation selector
  (sittings split after six hours of silence).
- The thread is a chronological log: `YOU` then `PAD`, flush-left, newest at
  the bottom, wrapped in full. Handwriting stays on the canvas.
- A selected turn offers `REPLAY ON PAGE`, which restores that turn's writing
  and reply to the canvas. It is a view operation, not a new oracle request.
- The drawer closes by the same edge swipe, a close control, or pen contact on
  the canvas. It never auto-closes while being read.

This is intentionally familiar to anyone who has used a messaging service or
Claude Code: a durable, inspectable log with clear speaker roles, but without
turning the pad itself into a chat app.

## Corpus explorer

`CORPUS` lives beside `HISTORY`, but opens a distinct read-only surface. It
answers “what can the pad currently see?” without exposing implementation details
on the writing page.

- Overview: number of stored turns, oldest/newest dates, memory capacity, and
  whether memory is enabled.
- Entries: the same chronological records as the conversation drawer,
  searchable by a single plain-language query.
- Context preview: the exact recent dialogue and catalog excerpt that would be
  sent with the next request, clearly labeled as **model context**.
- Knowledge sources: any configured corpus/provider, its last update time,
  and a plain-language description of what is and is not included.
- No editing or silent deletion. Destructive actions (`FORGET THIS`,
  `CLEAR CORPUS`) require a second confirmation in vermilion.

The first implementation can expose the existing local memory store (up to
400 turns, transcript, reply, and saved strokes). A later corpus backend can
plug into the same explorer without changing the canvas interaction.

## Page geometry

The rM2 canvas is 1404×1872. Use a 4×8 grid with a 72 px outer margin and
18 px gutters. The writing field is the quiet center; the control strip uses
the outer modules only. Reply text uses a fixed left edge aligned to the
writing margin, never a centered fallback.

Reply placement: start at the top writing line. The drink has already taken
the writer's ink; the reply uses the page, not the leftover strip under the
entry. Clear the consumed writing region before the first stroke so the
answer is not written through a ghost. It must never appear to jump to the
middle of the page.

Parked alternative: keep the drunk entry on the page and start the reply on
the first grid line below it (`reply_below_writing`), falling back to the
top writing line only when that leftover strip is too short. Use this if we
want both hands visible together.

## Timing and state

| State | Visible behavior | Exit |
| --- | --- | --- |
| Writing | Ink only | `SEND`, or optional idle-send |
| Thinking | Small fixed marker in the control margin | Reply, error, or cancel |
| Replying | Reply writes in one anchored region | Complete |
| Reply shown | Reply stays; no automatic dissolve | `DISMISS`, new writing, or `NEW PAGE` |
| Help/settings | Framed panel on the grid | Pen tap or explicit close |

There is no silent transition from writing to reply in the default mode. The
only automatic behavior in Stealth is ink rendering and safe persistence.

## Visual language

- UI face: a production-safe grotesque (Helvetica metrics; Liberation Sans as
  fallback), flush-left.
- Reply face: the expressive handwriting face remains reserved for the pad's
  voice.
- Two UI sizes only: 32 px label, 64 px section title; 2 px major rules and
  1 px minor rules.
- Black on paper is the default. Signal blue identifies the active mode in
  Guided panels; vermilion is reserved for destructive confirmation.
- No decorative icons, gradients, shadows, or persistent chrome.

## Implementation sequence

1. Make explicit underline-send the default and expose idle-send as a setting.
2. Replace reply centering and timed dissolve with anchored placement and
   explicit `DISMISS`.
3. Add the retractable Guided strip and mode persistence.
4. Add the hidden conversation drawer backed by `MemoryStore::recent_dialogue`.
5. Add the read-only corpus/context explorer backed by `MemoryStore::catalog`.
6. Rename/remove the duplicate AppLoad entry after the new bundle is verified.
7. Treat corpus quality as a separate oracle/data project.
