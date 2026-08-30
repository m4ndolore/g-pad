# Merging `anthink-interaction-design`

Verified by test-merging in a scratch worktree, building, and running the
tests. Two of the three hazards below compile cleanly when resolved wrongly,
so they are worth reading before resolving anything.

## Hazard 1 — `help.rs` is a modify/delete, and the obvious resolution loses work

This branch deletes `src/help.rs` (replaced by `src/gesture.rs`).
`worktree-sleep-fix` **modifies** it, adding Merge Combinator's branded sleep
page. Git reports a modify/delete conflict, and the obvious resolution — "I
deleted it, so `git rm src/help.rs`" — silently drops the branding. The build
stays green and the tests pass.

**Already handled.** `gesture.rs` absorbed the branded page in `b5eff5b`:
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

## Verified result

Merging this branch, then `worktree-sleep-fix`, then `origin/main`, resolving
as above: **80 tests pass, 0 warnings.** The branded sleep page renders with
its two mark strokes on distinct ink levels.

## Order

Any order works given the absorb, because the absorb is what removed the
silent-loss path. The earlier concern that "sleep-fix then anthink loses the
page" no longer applies: the page now lives in `gesture.rs` on this branch.

Do not merge to `main` without coordinating — merges are being sequenced
elsewhere.
