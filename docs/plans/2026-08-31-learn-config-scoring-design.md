# Learn mode: skill picker, level row, session score — design

Date: 2026-08-31. Status: implemented alongside this document.

## The ask

Three expansions to Learn (kids) mode, gamifying math only for now:

1. Math methods individually configurable — practice one method by name.
2. The level adjustable by hand (the automatic walk-up stays).
3. A scoring system within the current session.

## Approaches considered

- **Multi-select toggles** for the method mix (checkbox grid + a START box).
  Rejected for now: two-phase interaction is foreign to the one-mark-one-act
  page, and "practice bar models" was the actual want. A latched single
  skill covers it with the existing sticky-focus machinery.
- **Persistent score** across boots. Rejected: the ask says "within the
  current session", and a daily-reset score keeps the game low-stakes.
- **Env-var configuration** (`RIDDLE_LEARN_SKILL=`). Rejected: the tablet is
  handed to a child; the menu page is the configuration surface.

## What was built

**Skills picker** (`Page::Skills`, `sheet::draw_skills`): the menu's new
MATH SKILLS entry opens a second picker listing every math activity
(`problems::MATH_SKILLS`, labels from `Activity::label()`). A mark latches
`Topic::Skill(activity)` — a one-element rotation — sticky until the menu
says otherwise, never interrupted by earned play pages (only `Topic::Mix`
earns treats). Both pickers route marks through `Session::choose_menu`;
`is_menu()` now covers both pages, so the main loop needed almost no change.

**Level row** (`draw_menu` + `choose_menu` 7..=10): four numbered boxes under
the menu's sections, the current level filled solid. A mark re-seats
`Ladder::new(level)` — streak cleared, auto-adaptation continues — and stays
on the menu so a topic can still be picked. The old four math-bucket entries
(which conflated topic and level) collapse into one MATH entry.

**Session score** (`Session::{score, last_award}`): a correct answer scores
`10 × level + 5 × streak.min(4)`, settled before the ladder moves so a
level-up never inflates its own answer. ALMOST/NO score 0; Unknown moves
nothing. Rendering: a star + total in the practice header's top center (only
once nonzero), and a `+N` toast beside the cheer's right star on YES
(`draw_feedback` gained an `award` parameter). Resets every boot; play pages
neither earn nor show points.

## Deferred

- Multi-select method mix (toggle boxes) if single-skill latching proves too
  narrow in practice.
- Score persistence / daily bests — the "interesting reporting problem" the
  Ladder comment already defers.
- Milestone celebrations (every 100 points) — watch whether the +N toast is
  enough of a carrot first.
