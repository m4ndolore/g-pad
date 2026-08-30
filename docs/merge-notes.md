# Merging `anthink-interaction-design` — LANDED

**This branch landed on `main` as `ae0479b` on 2026-08-30.** The hazards below
are kept as the record of what the merge involved and how it was resolved;
they are history, not pending work.

Verified by test-merging in a scratch worktree, building, and running the
tests. Two of the three hazards below compile cleanly when resolved wrongly,
so they are worth reading before resolving anything.

## Hazard 1 — `help.rs` is a modify/delete, and the obvious resolution loses work

This branch deletes `src/help.rs` (replaced by `src/gesture.rs`).
`worktree-sleep-fix` **modifies** it, adding Merge Combinator's branded sleep
page. Git reports a modify/delete conflict, and the obvious resolution — "I
deleted it, so `git rm src/help.rs`" — silently drops the branding. The build
stays green and the tests pass.

**Already handled**, and note the direction has since flipped: `sleep-fix` has
landed, so `help.rs` is now modified on `main` and deleted here. Same shape,
but `git rm src/help.rs` is now the *right* resolution — the very resolution
that would have destroyed the branding before the absorb. Anyone working from
the older warning would now do the wrong thing.

`gesture.rs` absorbed the branded page in `b5eff5b`:
`assets/mc-mark.png`, `mark_gray`, `blit_gray`, `blit_left`, the composed
page, and both guard tests. `show_sleep` is now `(surf, font, ui_font)`.

So the correct resolution *is* `git rm src/help.rs` — but only because the
absorb already happened. Verify with `cargo test brand_mark_keeps_its_grays`
after merging: if that test is missing or failing, the branding was lost.

## Hazard 2 — this branch's `main.rs` drops `mod bridge;`

This branch forked before the bridge landed on main (`f671bd5`) and rewrote
the `mod` block. Taking this side of the `main.rs` conflict removes
`mod bridge;` while `src/bridge.rs` and its callers in `ui.rs` remain.

This one fails loudly (`cannot find bridge in crate`), so it cannot ship — but
it will look alarming. The fix is one line: re-add `mod bridge;`.

## Hazard 3 — the `main.rs` conflict hunk is the sleep call site

```
<<<<<<< HEAD
    let saved = gesture::show_sleep(&mut surf, &font);          // pre-absorb
=======
    let saved = help::show_sleep(&mut surf, &font, &ui_font);   // sleep-fix
>>>>>>>
```

Neither side is right post-absorb. The answer is
`gesture::show_sleep(&mut surf, &font, &ui_font)` — this branch's module, the
sleep page's signature. `ui_font` is already in scope.

Everything else in `main.rs` auto-merges, including the suspend fix from
`8b902ba`, which touches `main.rs` and `power.rs` and is independent of all of
the above.

## Deliberate removals — not conflicts, but decisions

Two things on `main` do not exist on this branch. Neither is an oversight, and
neither will produce a conflict: `help.rs` is deleted wholesale here, so its
contents vanish quietly. Anyone resolving from these notes should know they are
ratifying a decision, not discovering a bug.

### `looks_like_question_mark` and the `?` gesture guide

`main`'s `help.rs` has `looks_like_question_mark` (:52) and the guide modal it
opened, `show` (:193) with `Help::dismiss` (:232). This branch has neither, and
the README no longer lists "Draw a large `?` → Gesture guide".

**Removed on the user's explicit instruction**, in two steps:

1. *"looks like question mark never worked. It should be cut completely."* The
   recognizer was not merely inert — at two of its three call sites a suspected
   `?` set `send_mode = Some(CommitMode::Capture)`, force-committing the page.
   A recognizer nobody could trigger deliberately was wired to the pad's most
   consequential action.
2. Cutting it orphaned the guide, because the `?` was its only entry point —
   there is no `HELP` control and no `Action::Help`, so the panel had been
   unreachable in practice all along. Asked whether to delete it, re-enter it
   from the control strip, or leave it dark, the user chose **delete**.

Removing the panel also took `help.rs`'s off-spec 88/54/40 type scale and one
of the four competing `PAD` values — two findings from the UX audit resolved as
a side effect.

**If the guide is ever wanted back**, it should not return as a free-floating
shape. `docs/anthink-interaction.md` argues the general case: free-floating
recognition competes with handwriting for the same strokes and does not degrade
into doing nothing — it degrades into doing something else. A `GUIDE` entry in
the control strip is one `Action` variant and cannot misfire.

### Everything else

`help.rs`'s remaining contents are accounted for: the two working recognizers
(`looks_like_send_rule`, `looks_like_ask_arrow`) and the branded sleep page all
live in `gesture.rs`. Nothing else was dropped.

## Verified result

Re-verified against `main` at `cd688a0`, after `sleep-fix` landed. Merging this
branch with `origin/main` and resolving purely from these notes — `git rm
src/help.rs`, take `gesture::show_sleep(&mut surf, &font, &ui_font)`, re-add
`mod bridge;` — gives **80 tests passing** with the branded sleep page intact
(`brand_mark_keeps_its_grays` green).

Note the merged tree reports **26 dead-code warnings**, all from `bridge.rs`.
That is not merge damage: `bridge.rs` is unwired on `main` already, in exactly
the state `brief.rs` was before it got a module-level `#[allow(dead_code)]`.
This branch is at 0 warnings on its own. The two obvious follow-ups, in order:

1. Give `bridge.rs` the same annotation `brief.rs` has, with the same reason —
   it is a designed surface waiting on a call site.
2. Adopt `page.rs` in `bridge.rs`, which deletes most of those warnings
   outright rather than silencing them. See `docs/page-geometry.md`; it is a
   delete-only change, and `bridge.rs`'s own tests are the acceptance
   criteria.

## Dry run, resolved and verified

Full rehearsal against `main` at `cd688a0`, resolving only from this document:

```
git merge origin/main
git rm src/help.rs                                   # Hazard 1
# main.rs: keep gesture::show_sleep(&mut surf, &font, &ui_font)   # Hazard 3
# main.rs: keep `mod bridge;`                                     # Hazard 2
```

Resulting tree: `f8cea82`. Checks run on it:

| Check | Result |
|---|---|
| `cargo test` | 80 passed, 0 failed |
| `scripts/ci/check-no-vendor-blob.sh` | clean |
| `assets/mc-mark.png` | present, 15530 bytes |
| `brand_mark_keeps_its_grays` | present and passing |
| `MARK_PNG` / `mark_gray` / `blit_gray` / `blit_left` | all present in `gesture.rs` |
| `src/help.rs` | gone, no dangling `help::` references |
| `page.rs`, `gesture.rs`, `bridge.rs`, `brief.rs` | all present and compiling together |
| Rendered sleep page | inspected; mark strokes on distinct ink levels |

A quiet merge is not proof nothing was dropped, so the path checks above matter
as much as the test count. If a future rehearsal produces a different tree,
something moved and the difference is worth understanding before landing.

## Order

Any order works given the absorb, because the absorb is what removed the
silent-loss path. The earlier concern that "sleep-fix then anthink loses the
page" no longer applies: the page now lives in `gesture.rs` on this branch.

Landed as `ae0479b`, resolved exactly as documented above, plus one follow-up
commit adopting `page.rs` in `bridge.rs` (which is why `main` is at zero
warnings rather than the 26 this document anticipated).
