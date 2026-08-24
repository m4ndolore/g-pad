# The daily brief

**Status:** Design. Tier 1 only — see "Not this build".

The pad reads as well as it writes. The first reading surface is a **daily
brief**: one page, one day, no navigation, no article bodies, no fetching of
arbitrary web pages.

## Why a brief and not a reader

The flywheel this pad serves is *bring a problem, find a solution* — and it
breaks if no solution arrives, the solution is wrong, or it doesn't arrive in
time. A capture surface only ever runs half of that loop. The brief is the
return path: the thing that comes back the other way, on the same paper you
wrote on.

That argues for the *smallest* possible reader. A pad that renders a feed is
a worse RSS client. A pad that shows the five things that matter today is
something a laptop is bad at.

**The reader's job is selection, not rendering.** The rendering is easy; the
brain already does the hard part.

## Source

`https://api.mergecombinator.com/api/intel/feed` returns clean JSON:

```json
{"articles":[{"id","source","title","excerpt","url","date","tags"}],
 "total": 67, "sources": {...}, "attribution": "..."}
```

No HTML extraction, no read-later pipeline, no arbitrary page fetching. The
acquisition problem is already solved upstream.

Two facts about that feed, current as of 2026-08-24:

- Relevance is scored server-side with per-source thresholds, tunable via
  `INTEL_THRESHOLD_<SOURCE>` without a redeploy. Roughly 15 relevant articles
  a day survive from ~140 raw.
- `api/intel/briefing` exists but scrapes an external site and currently
  returns `{"briefing": null}`. **The brief must not depend on it.** Treat a
  briefing as an optional header when present, never as the page.

Fifteen a day is still too many for one e-ink page. The brief shows a bounded
number — a handful — and says how many it left behind. Silent truncation reads
as "that was everything" when it wasn't.

## The page

One page. Date, an optional briefing line, then the selected items:

```text
24 August 2026                                    9 more

DIU Unveils Bridge Program
Defense Innovation Unit · today
The unit is standing up a program to move companies from
prototype to production without a new competition…

Germany opens new drone security centre
Irregulars · today
Germany has opened a new drone security centre in
Saxony-Anhalt to help counter the growing threat…
```

Rules:

- **Fits the screen.** No scrolling on the first page. If it doesn't fit, fewer
  items — never a smaller face.
- **Source and date are always visible.** Provenance is a product feature here
  the same way it is in the corpus.
- **No bodies.** Title, source, excerpt. The excerpt is what upstream gave us;
  the pad does not summarise it again.
- **An item with no excerpt is not shown.** A card with a headline and no body
  reads as broken, and on paper you cannot scroll past it. (This is why the
  feed now drops Hacker News items whose only excerpt is the word
  "Comments".)
- **Say what was left out.** The count of unshown items is on the page.

## Annotation — Tier 2, designed for now, built later

Reading and annotating are the same gesture. Marking an item with the pen
produces a prompt whose `context` is that article:

```yaml
context:
  type: document
  ref: intel/2026-08-24#diu-bridge-program
```

This is why `context` is already in the capture record: the reader is not a
detour from the capture architecture, it is a second door into it. A brief you
mark up becomes a capture with the brief as context — the same Note/Discuss
fork, against a document instead of a blank page.

Nothing about Tier 2 changes the record. That is the test of whether the shell
in `capture-record.md` was drawn correctly.

## Implementation

The pad already has everything this needs:

| Need | Exists |
|---|---|
| Text layout | `ui::wrapped()` over `ab_glyph`, Liberation Sans |
| A read-only full-height surface | `ui::Drawer`, with scroll and saved-region restore |
| Page-sized regions | `surface::copy_rect` / `paste_rect` |
| Display update | `display::update` with per-region waveforms |

So the brief is a third `DrawerKind` beside `History` and `Corpus`, plus a
fetch and a small layout pass. It is deliberately not a new app.

Fetching happens on the same HTTP path the oracle already uses. A brief that
cannot be fetched shows the last one it had, with its date — a stale brief
that says it is stale beats an empty page.

## Not this build

- Arbitrary web articles or a read-later pipeline. This was the expensive half
  of "reading on the pad" and it buys the least.
- Full article bodies.
- A general RSS client, feed management, or per-source configuration on device.
- Signal thread rendering. That is Wingman's surface and a different shape;
  revisit after Tier 2 works.
- Offline fetching. The brief is fetched when connected. Disconnected, the pad
  shows the last brief it holds.

## Open

- **How many items.** Depends on the face and the screen; settle it on
  hardware rather than in a doc.
- **Selection.** Newest-first is the honest default until there is a ranking
  worth trusting. Anything cleverer should come from the brain, not the pad.
- **Whether the brief is a page or a drawer.** A drawer is half-width and
  already built; a full page reads better. This is the first thing to try both
  ways on the tablet.
- **Does long-form read well through takeover at all?** The open question this
  build exists to answer. Page-turn latency and font rendering at 1404×1872
  are a spike, not a design decision.
