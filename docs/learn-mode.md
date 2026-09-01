# Learn mode — the kids' tutor

The third anthink mode (`docs/anthink-interaction.md` names Agent, Pad,
Learn). A challenge renders, a child aged four to eight answers in their own
hand, and the pad marks it with warmth and a next step. This document records
what shipped, why it is shaped this way, and what is deliberately deferred.

## The premise, inherited

Learn mode is the first surface built end to end on the anchored-marks
vocabulary. Everything consequential is a bbox lookup against a hit map the
sheet returned when it was drawn; nothing is shape recognition:

- **The answer regions.** Every sheet declares one blank per question — the
  blank circle of a number bond, the outlined box after each equation, the
  ruled writing lines. A worksheet page carries up to four (see "Worksheet
  pages" below); the LAST one is the graded blank. Ink in a blank is the
  child's answer and stays on the page.
- **DONE** (bottom right). Any mark inside it — check, scribble, "x" —
  commits *only the graded blank* to the oracle for marking. The mark is
  absorbed, never sent.
- **NEW** (bottom left). Any mark inside it deals a fresh page.
- **MENU** (bottom center). Any mark inside it deals the picker: every
  topic and game as its own tick box (see "The menu" below).

A child cannot commit by accident: an empty blank answers locally ("Write
your answer first" — "Answer every box" on a worksheet page), and the
whole-page commit paths of Pad mode — the send
rule, idle-send, page flips, the finger-tap pen palette — are all disabled
while Learn is open. Fingers navigate nothing here; the pen does everything.

## The pedagogy

Singapore Math's concrete → pictorial → abstract ladder, four levels:

| Level | Activities |
|---|---|
| 1 | count the ten-frame dots (≤5), number bonds to 5, compare to 5 (<, =, >), trace single letters |
| 2 | bonds to 10, make-ten, + and − within 10, compare to 10, number-line hops, bar models to 10, trace CVC words |
| 3 | bonds to 20, + and − within 20, number lines to 20, bar models to 20, place value (teens), sight words |
| 4 | multiplication as dot arrays, division as sharing, ×/÷ equations, place value to 99, hundred-chart windows, bar models, longer words |

The full Singapore Math toolbox is on the page: part-whole **bar models**
(one bar split proportionally, the whole bracketed beneath, one number
blank), **number lines** with one arc per unit hop under the printed
equation, **place-value charts** (ten-discs and one-dots in labeled
columns), **hundred-chart windows** (the 3×3 neighborhood of a blank middle
cell), and **comparison boxes** (write <, =, or >).

Difficulty adapts locally: three right in a row steps up, two wrong steps
down, silently. Activities rotate rather than sample, so a math page is
always followed by something different.

## Worksheet pages

The compact kinds — equations, comparisons, number lines, bar models,
place-value charts, hundred-chart windows — deal like a paper worksheet: two
to four questions of one activity, stacked in numbered bands, each with its
own blank (`problems::Set`; the dot-heavy figures and handwriting keep the
page to themselves). DONE wants ink in *every* box before the page commits,
but the MVP grades **only the last question**: right moves on, wrong flags
that answer with the look-again underline and the grown-up reviews the rest
of the page. The graded blank's ink travels alone, cropped with a tight
catchment so a neighbor row's digit stays home.

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

## The menu

A mark in MENU deals the picker: two labeled sections of tick boxes,
**PRACTICE** (Counting, Add to 10, Add to 20, Times & Share, Writing,
Surprise Mix — the math rows also pin the starting level) and **PLAY**
(Doodle Critter, Guessing Game, Story Time). A mark in a box deals that page
at once, and the choice is *sticky*: a picked game deals itself again on
every NEW, a picked topic narrows the practice rotation (math only, or
tracing only) until the menu says otherwise. Surprise Mix restores the
default deck. The mapping between box order and meaning lives in one place
(`sheet::MENU_ITEMS` ↔ `Session::choose_menu`), and the picker page itself
has no DONE or NEW — a mark in an entry is the only thing it understands.

## The marking round trip

On a DONE mark, the answer region (only) is rasterized by `ink::region_png`
— same crop-and-downscale as the pad's page — and sent through the existing
oracle with a per-turn instruction (`TurnContext::instruction`, which rides
as user text and therefore works on both the HTTP and pi backends without
touching the persona).

The tutor's asks never use the pad's default model blindly: a capture sink
there (Vellum's `vellum-capture`) would archive the worksheet and answer
"Saved to Vellum" instead of marking it. Learn asks go to
`RIDDLE_LEARN_MODEL`, else `RIDDLE_OPENAI_ASK_MODEL`, else the base model —
on Vellum, `vellum-tutor` is the straight vision chat built for this.

The tutor's protocol is one leading word — **YES**, **ALMOST**, or **NO** —
then one to three capital **cheer words** on the same line, then (after
ALMOST or NO only) one short **hint line**. The verdict word is for the
sheet; the cheer is the child's channel; the hint is the grown-up's, read
aloud. The session assumes an adult nearby, but the child must get the
payload — right, almost, try again — with zero reading help:

- **YES** → a drawn check beside the answer, stars flanking the cheer, the
  ladder credits it, and — after a dwell long enough to enjoy the stamp
  (`RIDDLE_LEARN_NEXT_MS`, default 5000 ms, 0 disables) — the next page
  deals itself, so a child on a roll never taps NEW. While the praise is
  on show, a deliberate pen tap deals the next page at once — no waiting
  out the dwell. Writing cancels only the timed auto-deal (the child went
  back to the page); the tap stays live for a tap landing clear of their
  own ink, so a dot in a word never tears the page away but a deliberate
  tap in open space always turns it. Marking NEW works as everywhere.
- **ALMOST / NO** → a gentle underline ("look here again"), the ladder
  debits it. The hint suggests, never tells the answer.
- **Anything else** → `Verdict::Unknown`: the text still shows small, but
  no mark is drawn and the ladder holds still. A misread verdict must
  never move consequence — the same rule the marking vocabulary lives by.

The parser enforces the register the model is asked for: a "cheer" longer
than three words is demoted to the hint and a fixed cheer stands in
(GREAT JOB! / SO CLOSE! / TRY AGAIN!), so what the child reads alone is
always at most three capital words. Feedback is stamped in print — the
cheer huge and centered, the hint small beneath — in a reserved strip
above the decision boxes, wiped before each new reply so encouragement
never stacks into clutter. Practice verdicts skip the handwriting
animation entirely; a child should not wait to learn they were right.
Learn turns never enter the diary's memory.

## The play deck

Every couple of correct answers earns a **play page** — the treat that keeps
the worksheets moving (`RIDDLE_LEARN_PLAY` = `earned` (default) | `always` |
`never`). Treats interrupt only the default mix: a topic chosen from the
menu was asked for by name, and its pages keep coming until the menu says
otherwise. Three games rotate, all on the same marks, all shaped by one
hardware truth: the pad cannot fetch pictures, but it can draw geometry
anchored to the child's ink, and it can write.

- **Doodle critter** — turn-taking drawing. The child doodles anything and
  marks DONE; the model picks ONE decoration from a fixed menu (googly eyes,
  stick legs, arms, a party hat, antennae, a curly tail, sun rays, a crown, a
  mustache, a speech bubble with a word of its choosing), the pad draws it
  anchored to the ink bbox and writes a one-line christening ("His name is
  Kevin and he is late for soup."). Add more, mark again, it escalates. The
  model's choice travels as a leading menu word — the same
  parse-a-word-or-degrade-gracefully protocol as the verdicts, so a chatty
  model still lands its caption and the game never stalls.
- **The pad guesses** — reverse Pictionary. The child draws, marks DONE, and
  the pad guesses — starting sensible, getting sillier — then asks for one
  more detail. Pure prose; nothing to parse.
- **Story paths** — a choose-your-own-adventure. The child draws the hero
  first (an empty page just means the pad invents one); each beat writes
  itself and ends in three choice boxes, new anchored targets in the hit map.
  A mark in one picks the path; prior beats ride the oracle's existing
  history transport so the tale stays coherent. A model that forgets its
  choices degrades to GO ON / LOOK AROUND / GO HOME — a beat can never
  render without a way onward. The child can doodle each scene; the ink goes
  along with the next choice.

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
