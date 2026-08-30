# Shared page geometry

`src/page.rs` holds the arithmetic every bounded reading surface needs. It
exists because the second surface retyped the first one's constants, and the
third would have retyped them again.

## What is shared, and why only this

| Shared | Why |
|---|---|
| `PAD`, `TITLE_PX`, `BODY_PX`, `LINE_H`, `TITLE_LINE_H`, `HEADER_H`, `FOOTER_H` | Two surfaces open side by side should not look like two products. |
| `content_width()`, `limit(reserved)` | The page is the page. `reserved` is room claimed by something pinned. |
| `wrap_capped`, `title_lines`, `meta_line` | Every surface wraps to the column and caps; every surface has a `lead · trailer` meta row. |
| `fit(blocks, …) -> (taken, omitted)` | The measure-until-it-does-not-fit loop, and the count of what was left out. |
| `footer(parts)`, `counted(n, …)` | "Say what was left out" is a rule for all of them. |

What stays with each surface is what it *means*: which blocks are worth
showing, how many lines one is allowed, and what the footer counts. The brief
caps excerpts at three lines because it is a list; the bridge allows six
because it shows one session. Those are editorial decisions, not geometry, and
sharing them would be wrong.

## `Fill` — the one difference worth a parameter

The two surfaces disagree about which end of a sequence wins, and both are
right:

- `Fill::Front` — the brief. The first items are the ones worth reading.
- `Fill::Back` — the bridge. The *last* exchange is worth reading, so turns are
  taken from the end and then put back in order, because the page still reads
  downward.

This is the only behavioral difference between the two fit loops, so it is the
only thing the shared one takes as an argument.

## Merge state against main

Verified by test-merging this branch with `origin/main` at `f671bd5` (the
commit that landed the bridge) in a scratch worktree:

- **The merge is clean apart from two doc comments** in `brief.rs`. Main and
  this branch independently arrived at a byte-identical
  `split_objects(json, array_key)` and `json_field` — convergence, not
  conflict. Main's wording is the better of the two and has been adopted here,
  so the conflict is now gone.
- **The merged tree builds and passes 77 tests** — this branch's 61 plus main's
  bridge tests, green together.
- `bridge.rs` on main still carries its own copy of the geometry constants, so
  the adoption below is exactly as applicable post-merge as it was before.

## Adopting this in `bridge.rs`

`src/bridge.rs` (now on `main`, commit `f671bd5`) predates this module and
carries its own copy. Adopting it is deletion, not rework:

Delete from `bridge.rs`, and import from `page`:

```rust
const PAD: usize = 44;          // page::PAD
const TITLE_PX: f32 = 38.0;     // page::TITLE_PX
const BODY_PX: f32 = 28.0;      // page::BODY_PX
const LINE_H: usize = 38;       // page::LINE_H
const TITLE_LINE_H: usize = 46; // page::TITLE_LINE_H
const HEADER_H: usize = 110;    // page::HEADER_H
const FOOTER_H: usize = 70;     // page::FOOTER_H
const MAX_TITLE_LINES: usize = 2;
pub fn content_width() -> usize { … }
```

Keep, because they are the bridge's own editorial rules: `TURN_GAP`,
`MAX_TURN_LINES`, `MAX_ARTIFACTS`.

Then in `layout_session`:

```rust
let title_lines = page::title_lines(font, &session.title);
let meta = page::meta_line(&session.state, &session.updated);

let artifact_room = artifacts.len() * page::LINE_H;
let y = page::HEADER_H + title_lines.len() * page::TITLE_LINE_H + page::LINE_H;

let readable: Vec<&Turn> = session.turns.iter()
    .filter(|t| !t.text.trim().is_empty()).collect();
let measured: Vec<TurnLayout> = readable.iter().map(|t| layout_turn(font, t)).collect();
let (turns, turns_omitted) =
    page::fit(measured, y, page::limit(artifact_room), page::Fill::Back, |t| t.height);
```

and `footer_label` becomes the shared joiner:

```rust
page::footer(&[
    page::counted(remaining, "more session", "more sessions"),
    if layout.turns_omitted > 0 { format!("{} earlier", layout.turns_omitted) } else { String::new() },
    page::counted(layout.artifacts_omitted, "more changed", "more changed"),
    if stale { "not refreshed".into() } else { String::new() },
])
```

The bridge's existing tests are the acceptance criteria — particularly
`artifacts_claim_their_room_before_prose`, which is exactly what `limit(reserved)`
encodes, and `the_page_keeps_the_most_recent_turns`, which is `Fill::Back`.
If those pass unchanged, the adoption is correct.

## The rule going forward

A fourth reading surface writes no geometry. If it needs a constant that is not
here, the question is whether the other surfaces want it too — and usually they
do.
