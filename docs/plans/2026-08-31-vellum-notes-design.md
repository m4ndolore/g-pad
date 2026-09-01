# The Notes tab — reading the vault on the pad

2026-08-31. Reading works on this tablet; the next thing worth reading is the
vault. Vellum already ships two read-only device routes built for exactly this
(`worktree-device-notes`, deployed on gauntlet, verified through the public
tunnel):

    GET /api/device/v1/notes?prefix=&limit=   -> { notes: [{path, title, mtime}], total }
    GET /api/device/v1/note?path=             -> { path, title, text }

The list walks recursively, newest first, capped at 50, `total` alongside so
the pad can say what it left off. The body arrives with frontmatter already
stripped. Both routes bypass Cloudflare Access and are gated by the bearer
token at the origin.

## What the pad has, what it lacks

- `ureq` + rustls + webpki-roots: an arbitrary public HTTPS endpoint works
  today, no dependency changes.
- The Sessions pattern — poll thread, mutex-held snapshot, drawer draws with
  no I/O, stale flag instead of an empty list — is the proven shape for the
  list. But its detail view is free only because the poll payload carries full
  bodies; note bodies are unbounded, so the body needs its own fetch.
- The turn page (`State::SessionPage`) is a *single* page: `page::fit` takes
  what fits and the footer counts the rest, unreachable. `Gesture::Page` is
  inert there. A vault note needs the pad's first real page-turning reader.
- No markdown handling exists anywhere; `## Heading` would render its hashes.

## Decisions

**List by poll, body on tick.** A new `src/notes.rs` mirrors
`bridge::spawn_poll`: fetch the list on startup and every
`RIDDLE_VELLUM_POLL_S` (default 300 — notes change at human speed, not agent
speed), into a `static` mutex, stale flag on failure. Ticking a row fetches
the body synchronously with tight timeouts (connect 5s, read 10s). The writer
just asked for that page and is waiting for it; `post_nudge` already blocks
the loop for 5s on the same reasoning. On failure the drawer stays and the
footer says the fetch failed — never a blank page.

**Config:** `RIDDLE_VELLUM_BASE` (e.g. `https://vellum.mergecombinator.com`)
and `RIDDLE_VELLUM_TOKEN` in `oracle.env`. Unset = the Notes tab never
appears and the pad loses nothing, exactly like the bridge.

**Drawer:** fourth `DrawerKind::Notes`. The three tab x-thresholds re-space
to four; the `tap()` fallthrough that returns Sessions for "anything to the
right" becomes a real threshold. Rows: title, then path · age. Footer: "N
more" from `total`, "not refreshed" when stale.

**The note page:** new `State::NotePage { title, pages, page: usize, saved,
return_to }`. Lines are wrapped once through `page.rs` and chunked into
pages of `lines_per_page`; `Gesture::Page(delta)` turns; footer "i / n".
A finger tap puts the note away and the canvas returns (same as the turn
page); the drawer swipe also closes. Pen ink lands and stays, a note on a
note — the turn page taught us wiping ink reads as breakage.

**Markdown, lightly.** A `md_plain()` pass feeds the wrapper: heading hashes
dropped (text kept), `-`/`*` list markers become `•`, `**bold**`,
`*italic*`, `` `code` `` unwrapped to their text, `[text](url)` keeps the
text, fence lines dropped (content kept). Tested like `brief.rs`'s scanner.
No styled rendering: the pad has one regular face, and v1 is about reading,
not typesetting.

**Navigation, v1:** the flat newest-first 50 the endpoint was designed to
serve ("the most recent work has to be the part that survives the bound").
Folder drill-down via `prefix` is possible but honest folder rows need a
directory listing on the vellum side — a second repo and a gauntlet deploy.
Follow-up, not now.

## Non-goals

- No search, no write path, no folder tree (v1).
- No styled markdown rendering — plain text discipline.
- No caching of note bodies on the pad; the vault is the store.
