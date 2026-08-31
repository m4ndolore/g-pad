# Learn mode — the kids' tutor

The third anthink mode (`docs/anthink-interaction.md` names Agent, Pad,
Learn). A challenge renders, a child aged four to eight answers in their own
hand, and the pad marks it with warmth and a next step. This document records
what shipped, why it is shaped this way, and what is deliberately deferred.

## The premise, inherited

Learn mode is the first surface built end to end on the anchored-marks
vocabulary. Everything consequential is a bbox lookup against a hit map the
sheet returned when it was drawn; nothing is shape recognition:

- **The answer region.** Every sheet declares exactly one — the blank circle
  of a number bond, the outlined box after an equation, the ruled writing
  lines. Ink there is the child's answer and stays on the page.
- **DONE** (bottom right). Any mark inside it — check, scribble, "x" —
  commits *only the declared answer region* to the oracle for marking. The
  mark is absorbed, never sent.
- **NEW** (bottom left). Any mark inside it deals a fresh page.

A child cannot commit by accident: an empty blank answers locally ("Write
your answer first"), and the whole-page commit paths of Pad mode — the send
rule, idle-send, page flips, the finger-tap pen palette — are all disabled
while Learn is open. Fingers navigate nothing here; the pen does everything.

## The pedagogy

Singapore Math's concrete → pictorial → abstract ladder, four levels:

| Level | Activities |
|---|---|
| 1 | count the ten-frame dots (≤5), number bonds to 5, trace single letters |
| 2 | bonds to 10, make-ten on the ten-frame, trace CVC words, + within 10 |
| 3 | bonds to 20, + and − within 20, sight words, make-ten |
| 4 | multiplication as dot arrays, division as sharing, ×/÷ equations, longer words |

Difficulty adapts locally: three right in a row steps up, two wrong steps
down, silently. Activities rotate rather than sample, so a math page is
always followed by something different.

**Problems are generated locally** (`src/learn/problems.rs`, a seeded LCG, no
dependencies). The oracle is only ever asked to *read* the child's ink, never
to invent curriculum: a misread word costs a retry; a hallucinated lesson
would cost the child's trust in the page. Every generator is invariant-tested
(bond parts sum to the whole, subtraction never goes negative, division is
exact).

The handwriting sheets take this seriously in the other direction: the dashed
template letters come from the same rasterize → thin → trace pipeline as the
pad's own hand, scaled so a 'd' ascender spans the full writing height, with
the dashed midline placed at the font's *measured* x-height — the guide
letters genuinely touch the lines the child is told to reach.

## The marking round trip

On a DONE mark, the answer region (only) is rasterized by `ink::region_png`
— same crop-and-downscale as the pad's page — and sent through the existing
oracle with a per-turn instruction (`TurnContext::instruction`, which rides
as user text and therefore works on both the HTTP and pi backends without
touching the persona).

The tutor's protocol is one leading word — **YES**, **ALMOST**, or **NO** —
then one cheerful sentence of at most twelve simple words. The word is for
the sheet; the sentence is for the child:

- **YES** → a drawn check beside the answer, the ladder credits it.
- **ALMOST / NO** → a gentle underline ("look here again"), the ladder
  debits it. After NO the tutor gives a tiny hint, never the answer.
- **Anything else** → `Verdict::Unknown`: the feedback still writes itself,
  but no mark is drawn and the ladder holds still. A misread verdict must
  never move consequence — the same rule the marking vocabulary lives by.

Feedback writes itself in the reply hand in a reserved strip above the
decision boxes, wiped before each new reply so encouragement never stacks
into clutter. Learn turns never enter the diary's memory.

## Entry and exit

Settings gains a **KIDS LEARN MODE** row. The page choice persists
(`page=learn` in the preferences file) — Anthink remembers the last mode per
boot — and `RIDDLE_PAGE=learn` dedicates a boot to the tutor regardless, for
a tablet handed to a child. `RIDDLE_LEARN_LEVEL` seeds the starting level
(default 1). The control strip in Learn keeps ERASE (same sheet, clean) and
NEW PAGE (fresh problem); SEND and DISMISS do nothing.

## Verifying without a tablet

- `g-pad --learn-sheets [DIR]` renders four sample worksheets per level as
  full-page PNGs.
- `g-pad --learn-test [ANSWER]` runs one whole round trip against a
  simulated child: draws a bond sheet, handwrites ANSWER (default: the
  correct one) into the blank, sends the region, prints verdict and
  feedback. Exit 1 if the verdict came back unreadable.

## Deferred, deliberately

- **Progress reporting.** The ladder is the only memory, and it lives for
  one sitting. The interesting problem — mastery models, a parent's view,
  what the pad remembers of a child — is pedagogical and privacy-laden, and
  `docs/anthink-interaction.md:159` was right to defer it until it is next.
- **Local ink-vs-template scoring for tracing.** The skeleton to compare
  against already exists (`script::trace`); a Hausdorff/DTW score would give
  instant local feedback. Today the vision model judges legibility instead.
- **Eraser-proof sheet furniture.** The eraser whites the page, so a
  determined child can erase the printed circles. ERASE (or NEW) restores
  everything; sheet-aware erasing is not worth its complexity yet.
- **Multi-blank sheets.** The hit map carries one answer region; fact
  families and bar models want several. The `Target::Answer` variant is
  already shaped for it.
