# Anthink — the pen interaction model

Anthink is the app. Three modes live inside it: **Agent** (drive Claude Code),
**Pad** (the g-pad teammate over the connected brain), and **Learn** (the kids'
tutor). One binary, one gesture vocabulary, one grid. A mode changes what the
page *means*, never what the pen *does*.

This document is about the pen. It answers one question: with no keyboard, no
mouse, and a screen that repaints in a quarter second, how do you drive an
agent?

## The premise: mark, don't manipulate

A Kanban board is a mouse idea. Drag-and-drop needs a cursor, a hover state,
and a fast display — the pad has none of those. Dragging a card across e-ink
costs a full repaint per frame and lands you somewhere approximate.

A pen is not a pointer. A pen is a *marking* instrument. On paper you do not
drag a task to a new column; you strike it out, you tick it, you circle the one
that matters and put an arrow to what it needs. The verbs are annotation verbs.

So Anthink renders state and the writer marks it up. The agent proposes; the
page shows the proposal; the pen answers on top of it. This is how people have
always reviewed work on paper, and it is the only interaction that is *faster*
on a tablet than on a laptop — which is the only reason to build this at all.

## The cost that shapes everything

`help::looks_like_question_mark` is ~85 lines of hand-tuned geometry to
recognize a single glyph drawn anywhere on a blank page: normalize the stroke
direction, measure the arc, rule out a "7", constrain the descender. That is
the true price of free-floating shape recognition, and it buys exactly one
symbol.

Worse, it is ambiguous by nature. A tick and a hastily written "v" are the same
ink. A circle and a written "o" are the same ink. Any vocabulary of free-floating
shapes competes with handwriting for the same strokes, and the competition has
no clean resolution.

**Anchoring dissolves the problem.** In review-first use the page is not blank —
Anthink drew it, so Anthink knows the bounding box of every element on it. A
mark is then read as *where it landed* plus one cheap global property:

| Property | Test | Cost |
|---|---|---|
| Which element | `BBox` containment of the stroke's centroid | trivial |
| Affirm vs. negate | net stroke direction, or self-intersection | ~10 lines |
| Scope | does the bbox span one row or many | trivial |

An 85-line problem becomes a lookup. And the ambiguity vanishes: ink inside a
decision box is a command, ink in the margin is a note. The same "v" means
different things in different places, exactly as it does on paper, and the
writer never has to draw carefully.

The rule: **free-floating shapes are reserved for the few global commands that
must work on a blank page. Everything else is anchored.**

## The primitives

Three, and no more. Each is a distinct motor action, so they are hard to confuse
with one another and hard to produce by accident.

### 1. The tick — affirm

A mark inside a decision box that is not a horizontal stroke. Approve, accept,
yes, keep. Drawn as a check, a slash, a scribble, an "x" — Anthink does not
care about the form, only that the writer deliberately marked *that box*.

Forgiving on purpose: the writer is agreeing, and agreement should not require
penmanship.

### 2. The strike — negate

A horizontal stroke through rendered content. Reject, skip, drop, undo. This is
the one shape distinguished by form rather than only position, because it is
worth distinguishing: `looks_like_send_rule` already recognizes exactly this
geometry (wide, flat, `w >= h * 4`), and it is a gesture nobody produces by
accident when writing.

Its scope is its bbox. Strike one row, drop one item. Strike through five rows,
drop five. Scope is expressed by the length of the stroke, which is the natural
thing and needs no modifier.

### 3. The arrow — direct

A stroke ending in a reversal, from one element toward another or into the
margin. Send this there. Assign, route, reorder, "use this one."

`looks_like_ask_arrow` already detects the arrowhead (the final quarter doubles
back and deviates vertically). Anthink reads the tail's origin and the head's
destination as two `BBox` lookups. The arrow is the only primitive with two
operands, which is why it earns its complexity.

### Composition

The three compose without extra vocabulary:

- Tick a box, then write beside it → approve *with* a note.
- Strike a line, then write above it → reject *and* replace.
- Arrow from a task to the margin, then write → reassign with instruction.

Nothing here needs a mode, a modifier, or a toolbar.

## Handwriting is the slow path, deliberately

Handwriting recognition is real, useful, and expensive — a round trip to a
vision model, seconds on e-ink. It must never sit on the fast path.

- **Marks are local and instant.** Geometry only, no network. Tick, strike, and
  arrow resolve in the app and repaint one bbox.
- **Words are committed and slow.** Handwriting goes to the model only when the
  writer commits it with the existing rule-underneath gesture, and only the
  region they ruled.

This is also the honest answer to "what if recognition is wrong": the marks that
carry consequence never touch recognition at all. A misread word costs an edit.
A misread approval costs a merged branch.

## The surfaces

### Agent mode — review-first

Two surfaces, and the pen means the same thing on both.

**The board.** Not columns of draggable cards — a flush-left list of sessions,
one row each: what it is, its state, how long it has been there. State is a word,
not a color: `RUNNING`, `WAITING`, `DONE`, `BLOCKED`. Waiting sessions sort to
the top, because those are the ones that need a human. Tick a row to open it.
Strike a row to stop that session.

The Kanban instinct is right about one thing — you want to see many pieces of
work at once and their states. It is wrong about the interaction. Columns are for
dragging; a list is for marking. Anthink keeps the overview and drops the drag.

**The turn.** One session's latest exchange, full width, with the decision the
agent is waiting on rendered as an explicit box at the foot of the page. Tick the
box to approve. Strike it to reject. Write below it to redirect, then rule to
commit. Long output pages with the finger swipe that already exists.

The decision box is the heart of the whole design. It gives the highest-frequency
action — approve or reject — a fixed, known, anchored target that requires no
recognition and no precision. It is the one thing a reviewer does hundreds of
times, and it should cost one stroke.

### Pad mode

The current g-pad surface, unchanged: blank page, write, rule underneath, reply
in a flowing hand. The primitives apply where they have meaning — strike to
dismiss a reply, arrow to send an excerpt into a note — but Pad mode's premise
stays the blank page.

### Learn mode

Deferred. Recording only what the vocabulary implies, so the design does not
have to be revisited later: a challenge renders, the child answers in their own
hand, and the tick/strike primitives serve self-marking and retry. The
interesting problem there is pedagogical (progress, tailoring, reporting), not
gestural, and it should be designed when it is actually next.

## Switching modes

Modes are a property of the *page*, not a place you travel to. The existing
top-edge swipe reveals the control strip; mode lives there, as it does today for
Stealth and Guided. Anthink remembers the last mode per boot.

No launcher, no home screen, no persistent chrome. The pad should open where the
writer left it.

## What this owes the existing system

The vocabulary is deliberately built from what is already proven in this tree:

- `looks_like_send_rule` — the strike, and the commit gesture.
- `looks_like_ask_arrow` — the arrow's head.
- `Ink::pop_stroke` — absorbing a mark so a command does not leave ink behind.
- `BBox` — the containment test every anchored mark depends on.
- `touch::Gesture` — fingers navigate (scroll, page, drawer); the pen commands.
  That channel separation is what lets marks stay unambiguous.

The one genuinely new piece of machinery is a **hit map**: the rendered regions
of the current page, with what each one means. Everything above is a lookup
against that map. It is the same idea as `control_action`'s fixed hit regions,
generalized from a fixed strip to whatever the page currently shows — and it
should be built as a returned value of drawing, so a region can never drift
from what was painted.

## Reconciling with the Claude bridge

`docs/claude-bridge.md` (branch `worktree-claude-bridge`) designs the reading
half of Agent mode and reaches the same conclusion from the other direction:

> A surface that pretends to be a terminal will fight the hardware on every
> frame; one that behaves like correspondence will not.

That is *mark, don't manipulate* arrived at from latency rather than from what
a pen is. The two documents agree on the shape of the thing. This one adds the
marking vocabulary; the bridge defines what is on the page to mark.

The bridge branch has already built more of this than a reader would guess.
`ui::draw_sessions` ships the board: a session list in the drawer's half-width
panel, one row each with state, time, and the last readable line.
`bridge::layout_session` — the full-page reader, which takes turns from the end
and pins artifacts before prose — is written and tested but **never called**.
The turn page is not unbuilt. It is built and unwired.

Three things the bridge settles that this document adopts:

- **Reading ships before responding.** Marks are worthless without state to
  mark. The board exists; the turn page needs only to be reached.
- **Prompt / response / artifact.** Agent prose is commentary; a sha, a path,
  or an exit code is a fact about the world. The board shows *state*, and state
  is an artifact — `RUNNING`, `WAITING`, a commit — never the agent's prose
  about its own progress. This is why the board's rows are words and not the
  model's self-report.
- **The pad never mints artifacts.** A mark records a human decision. It must
  never be recorded as though the agent accomplished something.

### One open disagreement

The bridge lists under **Not this build**:

> **Approving tool calls from the pad.** Consequential actions should not hang
> off a transcription round trip.

The decision box is that feature, and the objection deserves a real answer
rather than a footnote.

The objection is right about *transcription*. Writing "yes, ship it" and having
a vision model read the cursive is a bad way to approve a consequential action:
the failure is silent, the confidence is unearned, and the cost lands on the
repository.

An anchored tick is not that. It is `BBox` containment plus one direction test
— local geometry, no model, no network. It is the same class of detection as
`looks_like_send_rule`, which this tree already trusts to commit a page to the
oracle. The risk the bridge names is a property of transcription, not of
approval, and the decision box does not transcribe.

Two constraints keep that honest, and they are part of the design rather than
concessions to it:

- **Destructive approvals are confirmed, in vermilion, per the UX system.** A
  tick that merges, deploys, or deletes renders a confirmation; a tick that
  advances a conversation does not. The line is consequence, not category.
- **Reject is always cheaper than approve.** A strike takes effect immediately;
  a consequential tick asks once. Getting it wrong should cost a redraw, never
  a rollback.

If hardware testing shows anchored marks misfire often enough to be dangerous,
the decision box loses to the bridge's position and approval moves off the pad.
That is a hardware question, and it should be answered on hardware rather than
in a document.

### Two questions the bridge left open, answered here

`docs/claude-bridge.md` closes with an Open list. Two of its items are exactly
what this document decides, so they are answered rather than re-asked:

- **"What a session is on the page — one per page, or a selector?"** Both, and
  they are the two surfaces above. The selector is the board; one-per-page is
  the turn. The bridge already shipped the first and wrote the second.
- **"Whether the drawer's half-width panel is enough, or whether reading wants
  the full page."** The board earns the drawer — it is a glance, and it should
  not displace what is on the canvas. The turn wants the full page: it is the
  thing being read, and `layout_session` already measures against `SCREEN_W`
  and `SCREEN_H` rather than `PANEL_W`. Half-width for choosing, full-page for
  reading.

### The test this contradicts, deliberately

`ui.rs::the_sessions_tab_is_read_only` asserts that a tap below the drawer
header returns `Action::None`, with the comment *"reading is not a capture
path."*

Opening a session from the board breaks that test, and it should — but as an
argued change, not a silent one. The rule the test protects is real: reading
must not become a capture path, because capture means ink, transcription, and
a post back to the agent. Opening a session is none of those. It is navigation
between two read-only surfaces, and it writes nothing.

The replacement invariant, which keeps the intent and drops the
over-restriction: **no tap on a reading surface may write.** A tap may change
what is displayed. Marks — which do carry intent — remain a separate channel
with their own confirmation rules.

### What stays out, and stays out here too

The bridge's exclusions this document does not touch: no keyboard ever, no
streaming (the pad polls), no arbitrary tool output, no orchestrating g-stack.
Nothing in the marking vocabulary needs any of them.