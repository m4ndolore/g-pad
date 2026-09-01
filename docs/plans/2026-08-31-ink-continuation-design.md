# Ink Continuation — freeform and canned nudges from the turn page

Date: 2026-08-31
Status: implemented (see "As built" at the end for review-driven refinements)

## Problem

The turn page can approve or reject a pending action (tick / strike) and can
poke a done session with a literal "continue". That is the whole vocabulary.
A session that finished its turn, or one that asked a real question, cannot be
*directed* from the pad — the reviewer has to walk back to a keyboard.

The transport already exists end to end: the hub accepts
`{"mark":"text","text":"…"}` and types it into the pane (`hub/src/tmux.rs`,
`Nudge::Text`), and the pad already posts text nudges (`bridge::post_nudge` —
the done-state tick sends the word "continue" this way today). The oracle
(`src/oracle.rs`) already reads handwriting from framebuffer snapshots — that
is the riddle heritage. Only the middle is missing: turn-page ink that misses
the decision box is kept as a dead-end note that nothing ever reads.

## The input model

One rule resolves all ambiguity: **ink on the page changes what a tick
means.**

- No note ink → the decision box behaves exactly as today (approve a pending
  action; continue a done session).
- Note ink present → a tick in the box means "send my words": snapshot the
  note, transcribe it via the oracle, show the transcription in the box as
  `SEND: "…"`, and require a second tick to send (strike discards and
  re-absorbs the ink).

This works identically for `done` sessions (freeform continuation) and
`waiting` sessions (answering the agent's actual question instead of merely
approving option 1).

### Nudge bubbles

A single row of small rounded-rect tags above the decision box —
`CONTINUE` `SHIP IT` `RUN TESTS` `EXPLAIN` — each sized to its text. A
deliberate mark whose center lands inside a bubble sends that bubble's phrase
immediately as a text nudge: one mark, no arming, because these are
pre-vetted phrases, not destructive approvals. Each tag maps to a fuller
canned sentence (`EXPLAIN` → "explain what you're blocked on and what you'd
do next"); the tag→phrase table lives in preferences. Bubbles render only
when the session can accept text — never on the picker.

## Transcription mechanics

The oracle gains a **transcriber call** beside the diary persona: same
`ask`-style plumbing and backend selection, different prompt — "Return
exactly the words written on the page, nothing else. If any word is
illegible, return it as `?`. No commentary." The pad crops the framebuffer to
the note's bounding box (plus margin) and writes the PNG, as the diary flow
does. The reply is collected whole, never streamed — a partial instruction
must never be sent.

While the oracle thinks, the box shows `READING YOUR INK…` with the diary's
pulse treatment. Outcomes:

- **Clean** → `SEND: "…"` with tick/strike affordance. Long text is
  truncated visually but sent in full.
- **Contains `?`** → gaps shown verbatim (`SEND: "run the ? again"`). Tick
  still sends — the agent copes with a dropped word better than a full
  rewrite — or strike, rewrite the bad word, tick again; the re-tick
  re-snapshots the whole note.
- **Oracle unreachable / timed out** (diary timeout) →
  `INK NOT READ · TRY AGAIN OR USE A BUBBLE`. The note ink stays on the page;
  bubbles remain live as the offline fallback.

Deliberate scope cuts: no on-device transcription editing (the correction
loop is strike–rewrite–retick), and the `SEND:` confirmation times out back
to normal after ~60s so a stale confirmation cannot be ticked days later.

## Wiring

The hub does not change. All work is pad-side, four touchpoints:

- **`oracle.rs`** — `transcribe(png_path)`: transcriber prompt, full-reply
  collection, same backend selection as `ask`.
- **`ui.rs`** — `draw_session_page` renders the bubble row and returns its
  hit-rects alongside the decision box; the box learns three new faces
  (`READING YOUR INK…`, `SEND: "…"`, `INK NOT READ…`).
- **`main.rs`** — `SessionPage` gains a note sub-state:
  `Idle` / `Transcribing { rx }` / `Confirm { text, since }`.
  `session_page_mark` checks bubbles first (center-in-bubble → send phrase at
  once), then the box (ink present → snapshot note bbox, enter
  `Transcribing`; confirm state → send or discard). While transcribing or
  confirming, bubbles go inert so nothing double-fires. The note bbox is the
  union of stroke bboxes that hit neither box nor bubble.
- **`preferences.rs`** — the tag→phrase table with the four defaults.

## Testing

House pattern throughout: state-transition tests drive `session_page_mark`
with a fake oracle channel (the `rx` is an mpsc receiver, so tests feed
canned transcriptions — no network); `classify_mark` grows bubble-hit cases
from synthetic strokes; hub tests stand untouched. Final verification is a
hardware read on the rM2, as the turn page received.

## As built

The review pass sharpened four points; the code follows these, not the
sections above, where they differ:

- **The one rule is symmetric.** Ink on the page flips *both* marks: a tick
  sends the words, a strike clears them (`NOTE CLEARED`). Rejecting a
  pending action with a strike requires a page clean of note ink — so a
  reject can never silently coexist with unspoken words. The
  strike-on-`SEND:` offer still declines without clearing, which is the
  fix-one-word loop.
- **Notes are per page.** Each stroke is tagged with the turn page it was
  written on; flips hide other pages' strokes rather than smearing them over
  new text, and a tick transcribes and sends only the visible page's words.
- **Bubbles block on a pending note.** A bubble marked while the visible
  page holds note ink does not fire; the box explains
  (`NOTE ON PAGE · TICK SENDS IT · STRIKE CLEARS IT`). Bubbles are also
  inert while reading or offering.
- **The memory protocol is tolerated, not fought.** With the pad's memory
  on, both oracle backends carry the diary's ⁂-postscript protocol in their
  system prompt (Pi bakes it at spawn; it cannot be suppressed per turn). A
  model may follow it instead of the transcribe instruction, so the
  collector keeps the ⁂ transcription as a fallback used when the prose
  reply is empty. The confirm-before-send face remains the guard either way.
- The mark semantics live in a pure decision table (`mark_act`), tested
  exhaustively without a screen or network; `session_page_mark` only carries
  out its verdicts.
