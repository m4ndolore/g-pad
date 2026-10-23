# Learn mode: the Singapore progression, players, and the race

## The ask

The kids like games, competition, and reward. They need to practise writing
their numbers, two-digit adding and taking away, then basic multiplication
and division. Shapes are worth adding.

Learn mode already teaches Singapore Math through level 4: number bonds, bar
models, ten-frames, place value to 99, and multiplication as dot arrays. What
it lacks:

- number writing (only letters are traced),
- two-digit sums in the column form Primary 1 and 2 use, with and without
  regrouping,
- a times-table progression by fact family, with division taught as its
  inverse,
- shapes,
- rewards that outlast a boot, and anything to compete over.

## Decisions made without the grown-up

This was built overnight. These choices are recorded so they can be undone:

1. **A fifth level.** Level 4 stays as it is. Level 5 is where regrouping,
   the 3 and 4 times tables, missing factors, and the harder shapes live. The
   menu's LEVEL row gets a fifth box.
2. **Players are animals, not names.** FOX, OWL, and BEAR by default, so
   nothing about the children is stored or shown. `RIDDLE_LEARN_PLAYERS`
   (comma-separated, at most four) renames them.
3. **Each player keeps their own level and stars.** Switching player re-seats
   the ladder on that player's level. The file is
   `$RIDDLE_DATA_DIR/learn-players`, plain `key=value` lines, on the tablet
   only.
4. **Trophies are star milestones.** Every 100 lifetime stars is a trophy,
   and reaching a new level is a badge. They are drawn as geometry, since
   the pad cannot fetch pictures.
5. **The race is marked on the tablet.** Every race question is multiple
   choice: three tick boxes, one right. That makes it instant. An oracle round
   trip takes seconds, which would make "fastest" measure the network rather
   than the child.

## Number writing

`Kind::TraceNumber`: the same dashed-skeleton trace as letters, then the
number freehand on the lines below. Single digits 0 to 9 at level 1, the
teens and twenty at level 2, two-digit numbers after that. Digits are drawn
at cap height, so they rise to the top line, which is how numbers are taught.
A reversed digit is ALMOST, never NO. The tutor brief says so.

WRITE NUMBERS is its own PRACTICE entry, and the default mix deals it at
levels 1 to 3.

## Two-digit sums: the column form

`Kind::Column { a, op, b }`: the numbers stacked under a T and an O heading,
the operator to the left, a rule beneath, and one wide answer box under both
columns. The child writes the whole answer.

| Level | Adding | Taking away |
|---|---|---|
| 3 | two-digit + one-digit, no regrouping | two-digit − one-digit, no regrouping |
| 4 | two-digit + two-digit, no regrouping | two-digit − two-digit, no regrouping |
| 5 | with regrouping (the ones make a new ten) | with regrouping (borrow a ten) |

"No regrouping" is checked, not hoped for: the generator rejects a pair whose
ones column carries or borrows.

## Times tables and division

Singapore's Primary 2 order: 2, 5, and 10 first, then 3 and 4.

- `Kind::Groups { groups, each }`: equal groups, drawn as circles holding
  dots, with `groups x each = ☐`. This is the pictorial step between the
  array and the bare fact.
- **Times facts**: `Kind::Equation` with `Op::Mul`, the table chosen by level
  (2, 5, 10 at level 4; adding 3 and 4 at level 5).
- `Kind::MissingFactor { a, product }`: `a x ☐ = product`, division taught
  as the inverse of a fact the child knows. Level 5.
- **Division facts**: `Kind::Equation` with `Op::Div`, drawn from the same
  tables, so every quotient is exact.

## Shapes

Drawn locally as polygons and rings:

- `Kind::ShapeSides { shape, ask }`: one big shape. HOW MANY SIDES? or HOW
  MANY CORNERS? The child writes the number. A circle has 0 of each.
- `Kind::CountShapes { target, scene }`: a scatter of mixed shapes. HOW MANY
  TRIANGLES?
- `Kind::ShapePattern { cycle, shown }`: a row of shapes repeating, and an
  empty box. DRAW WHAT COMES NEXT. The tutor judges the drawing.
- `Kind::NameShape`: multiple choice. One shape drawn large, three tick boxes
  with names (TRIANGLE / SQUARE / CIRCLE). Marked on the tablet.

The shapes, by level: circle, triangle, square, and rectangle at levels 1 and
2, adding pentagon and hexagon at level 3 and up.

## Multiple choice, marked on the tablet

A practice problem can carry `options` with one correct index. The sheet
draws them as tick boxes in the hit map's `choices`. A mark in one decides
the verdict locally, and the main loop feeds the result (`YES` with a cheer,
or `NO`) through the same channel and the same code as a tutor reply. The
check, the cheer, the score, the ladder, and the next-page dwell all behave
exactly as they do for an oracle verdict.

## Players, stars, and trophies

The menu gains a PLAYER row: one box per player, the current one filled,
like the LEVEL row. Picking a player saves the current player and loads the
new one's level.

A correct answer's award (`10 × level` plus the streak bonus) is added to the
player's lifetime stars as well as the session score. The header shows the
player's animal and their stars.

**TROPHIES** (a GAMES entry) is the leaderboard: each player's stars, a cup
per 100, their highest level, and their best race. A YES that crosses a
hundred stamps NEW TROPHY! and the cheer gets bigger stars.

## The race

**SPEED RACE** (a GAMES entry): ten multiple-choice questions at the player's
level, mixing sums, facts, and shapes. The header counts them, 3 / 10. Each
question is marked the moment its box is ticked; a wrong tick shows TRY
AGAIN and stays on the question. At the end, the page shows the time and the
player's best. A faster time is NEW RECORD!, and a race finished with no
wrong ticks earns a bonus. The leaderboard keeps each player's best time.

Races never move the ladder, the same rule as play pages: the race is the
game, the ladder is the lesson.

## The menu, re-laid

The pickers move to three columns:

- **PRACTICE**: MATH, MATH SKILLS, WRITE LETTERS, WRITE NUMBERS, SHAPES,
  SURPRISE MIX
- **GAMES**: SPEED RACE, TROPHIES, CHESS, DOODLE CRITTER, GUESSING GAME,
  STORY TIME
- **PLAYER**: one box per player
- **LEVEL**: 1 to 5

The skills page gains BIG SUMS, BIG TAKE-AWAYS, EQUAL GROUPS, TIMES FACTS,
MISSING NUMBER, DIVIDE, and a SHAPES strand (SIDES & CORNERS, COUNT SHAPES,
SHAPE PATTERNS, NAME THE SHAPE).

## Order of work

Each step is its own PR, tested and rendered with `--learn-sheets`:

1. Level 5, number writing, column sums, times and division.
2. Shapes marked by the tutor: sides and corners, counting, patterns.
3. Multiple choice marked on the tablet, NAME THE SHAPE, FIND THE SHAPE.
4. Players, lifetime stars, trophies, the leaderboard, the three-column
   menu.
5. The race.
