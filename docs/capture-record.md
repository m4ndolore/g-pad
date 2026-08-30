# The capture record

**Status:** Draft shell. Expect this to change — it is written to be extended,
not to be right the first time.

This is the durable unit of a pen interaction: what the pad writes down, what
it sends, and what a brain gets back. Everything else in g-pad hangs off it —
sync, pruning, offline behavior, and the eventual retrieval swap.

## Why this document exists

Today the same interaction exists in three incompatible forms:

| Where | Shape |
|---|---|
| The pad's local store | `index.tsv` row + `<id>.strokes` file |
| The Vellum device gateway | a Markdown note with provenance frontmatter |
| The diary dialogue files | prose under a `## HH:MM` heading |

None can be reconciled with the others. That is not a theoretical problem: on
2026-08-23 the pad was asked to list the corpus and answered *"I don't have a
corpus to list… I can't inventory what I can't see"* — correct, because the
only corpus it had was its own local turn log. A shared record is what closes
that gap.

## Vocabulary

**Prompt** — what the writer put on the page. A note, a question, a drawing, an
insight, a blocker. It is the durable artifact and it has standalone value.

**Response** — what came back. Derived from the prompt; useful, but not
evidence.

**Conversation** — a sequence of prompts and responses with a persisting
subject. Maps to an Ask thread in anymouse; **Discuss** is the action that
promotes a prompt into one.

The asymmetry matters. A handwritten *"SigmaBlox transitioning to the Guild
app"* is a fact about the world. A reply about it is commentary. So:

> **Prompts are evidence. Responses are not.**

Responses are stored with their prompt and are available as conversation
context, but they must never rank as retrieval evidence. A brain that cites its
own past answers manufactures corroboration, and that compounds.

## Shape

```yaml
id: rm2-paul:1755950400:7f3a         # device : unix seconds : random
kind: prompt                          # prompt | response | artifact
conversation: null                    # id of the conversation, or null
captured_at: 2026-08-23T11:03:00Z
device_id: rm2-paul

# What the writer declared, and how we know.
category: blocker                     # a label from workspace config, or null
category_source: user-declared        # see "The hint" below
taxonomy_version: null                # config version the label came from

# The artifact.
text: "…transcription…"
strokes: .gpad/strokes/7f3a.strokes   # pen geometry, or null
source_image: .gpad/images/<sha>.png  # or null once aged out
content_sha256: <hash of the image>

# Where it came from, if not a blank page.
context:
  type: document                      # document | none
  ref: briefs/2026-08-23-daily.md

status: inbox
```

A response record carries the same envelope with `kind: response`, a
`conversation`, and provenance for how it was produced (model, retrieval
sources). It is never `kind: prompt`.

### Artifacts

An agent that edits files does not fit the prompt/response split. Its prose is
commentary like any other response, but the commit it made is a fact the
repository can be asked about independently. Filing both as `response` throws
away the only agent output worth keeping; filing both as evidence is exactly
the self-citation the rule above forbids.

So there is a third kind. An `artifact` record names something outside the
model that another system can verify — a commit sha, a file path, an exit
status — and carries the session and agent that produced it.

> **An artifact is evidence by reference, never by description.**

`"fixed the parser"` is prose and files as `response`. `a1b2c3d` is an
artifact. The test is whether the claim can be named by a reference something
else can check; if it cannot, it is prose. Nothing that only the model asserts
becomes evidence by being recorded next to something that did happen.

The pad never mints artifacts. It renders what a bridge reports. See
`claude-bridge.md`.

### Identity

`<device>:<unix-seconds>:<random>` — minted by the pad at commit time and never
reassigned. Unix seconds alone is not enough: two pages committed in the same
second collide.

This is what makes sync safe. The pad writes a page offline, uploads, the
network drops before the ack, the pad retries — the server must recognise the
retry as the same prompt rather than filing a second note. The gateway already
dedupes by request id; the pad must supply a stable one.

**Never eat ink.** Local ink is deleted only after the server has durably
acknowledged that id. An unacknowledged prompt stays on the tablet
indefinitely.

### The hint

The category is a **hint**, not an authoritative label, and its provenance is
part of the record:

| `category_source` | Meaning |
|---|---|
| `user-declared` | The writer chose it |
| `user-declined` | The writer was offered the choice and dismissed it |
| `inferred` | Derived downstream from content |
| `inferred-confirmed` | Suggested downstream, then approved by the writer |
| `absent` | The writer was never asked |

`user-declined` and `absent` are different facts and both are worth keeping —
"saw the choice, didn't care" is signal about the category set itself.

Anything derived from a hint is marked derivative. An inferred label must never
be indistinguishable from a declared one; that is the ungrounded-confidence
failure mode, wearing a friendly UI.

Categories come from workspace configuration, not from an enum in this
repository. The pad renders what it is given and binds a few to fast gestures.
An artist's categories are not an engineer's.

## Retention

Three assets with different cost profiles, so three policies:

| Asset | Policy | Why |
|---|---|---|
| Strokes | Keep | Kilobytes once decimated, never re-inferenced, and not reconstructible from anything else |
| Text | Keep | Small, and it is what retrieval actually runs over |
| Source image | Bounded | Needed once for transcription, then cold. Age out to cheap storage |

**Inference runs once per page, never per query.** Transcribe on capture; all
retrieval afterwards is text over text. This keeps cost linear in pages
written rather than in questions asked. Bulk re-transcription is a deliberate
decision, never a background job.

Strokes are the writer's actual pen geometry — decimated, but faithful enough
to render a page back in their own hand. Nothing else in the stack holds that.
Pruning them on a fixed count destroys the one asset that cannot be rebuilt.

## Offline

**Disconnected, not unplugged.** Offline the pad *always* captures and
*sometimes* answers.

Transcription needs vision inference, so a disconnected pad cannot read
handwriting. It therefore queues: strokes and image stored locally under the
minted id, transcription and response deferred until reconnect. Capture never
depends on the network.

On reconnect, queued prompts transcribe and get real responses. Offline
answering is out of scope for this tree.

## Reading

The pad also renders. The first reader is a **daily brief** — one page, one
day, no navigation, no article bodies, no fetching. The content already exists
upstream; the pad is an output surface for a selection a brain already made.

Reading and annotating are the same gesture. Marking up a brief produces a
prompt whose `context` is that document — the same record, entered from a
different door. That is why `context` is in the shell now: the reader should
slot in without reshaping anything.

The second reader is the **Claude bridge** — agent sessions on the same paper,
annotated through the same door. It needed one addition to this record (the
artifact kind above) and no reshaping otherwise, which is the first real test
of that claim. See `claude-bridge.md`.

## Open

Decisions that are deliberately not made here:

- **What bounds a conversation** — idle gap, page, explicit gesture, or
  subject. The local store currently splits sittings after six hours of
  silence; whether that is the right boundary wants hardware time.
- **Presenting configurable categories on e-ink.** A fixed few are a fast
  gesture; an arbitrary list is a menu, and a menu is friction on the one path
  that must stay frictionless.
- **Whether the pad's conversations are anymouse Ask threads directly**, or a
  vault-native form that maps onto them.
- **Response provenance detail** — how much of the retrieval trace to keep so
  that a disagreement between two surfaces can actually be explained.

## Sources

- `docs/REMARKABLE_DEVICE_DESIGN.md` in Vellum — the device protocol, gesture
  boundaries, and the "never eat ink" principle this inherits.
- anymouse `docs/taxonomy.md` — evidence tiers and the synthesis vocabulary a
  hint eventually feeds.
- anymouse `docs/plans/2026-07-07-capture-modes-and-drawer-design.md` — commit
  before classify; the fork is a bonus, never a gate.
