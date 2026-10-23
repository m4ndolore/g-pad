# Tutor service: generated lessons on e-ink

## Why

Learn mode marks a child's handwriting well, but everything it deals is
generated on the pad: math worksheets, tracing, games. A parent cannot ask
for "a four-page reading lesson about sharks at short-vowel level", and
nothing outside math reaches the page.

[Ari](https://github.com/nichochar/ari) shows the missing half: a tutor you
chat with that writes lessons, verifies them with a second model, and grades
handwritten work against a key. Ari reads paper through a camera, and about a
fifth of its grading prompt deals with photo trouble (overlapping shots,
glare, a photo of the prompt alone). The pad already removes that problem:
it knows which question the ink answers and sends only that box.

This design pairs the two. A parent chats with a tutor in a web app and
creates lessons there. The rM2 deals the lessons as e-ink pages and grades
the child's ink.

Scope is children first. Every place the design must later serve adults is
marked, and the data model carries the hooks from day one.

## Decisions

| Question | Decision |
|---|---|
| Where the server lives | New service in `tutor/` of this repo, deployed on gauntlet beside Vellum |
| v1 parent features | Chat and generated lessons. Progress, ink review and assignment come later |
| How lessons reach e-ink | The server renders each page to a PNG plus a regions JSON; the pad displays the bitmap |
| Stack | TypeScript on Node, Playwright for rendering, SQLite, a small Vite web app |
| Models | One router with `cli`, `cloud` and `local` backends, routed per task |

Local-only restrictions from earlier g-pad work do not apply here. Education
uses the cloud wherever it helps, preferring CLI sessions over metered APIs
to spend subscription allowance instead of API credit.

## Architecture

```
 parent (phone/laptop)            gauntlet: tutor service              rM2 (g-pad)
 ─────────────────────            ──────────────────────              ───────────
 web app  ── Access SSO ──►  /app   chat, lessons, learners
                             /api   ─┐
                                     ├─ lesson pipeline: generate → verify → revise
                                     ├─ page renderer: HTML → PNG + regions.json
                                     ├─ grader: ink PNG + answer key → verdict
                                     └─ model router: cloud | cli | local
                             /device/v1  ◄── bearer token ──  pull pages, post ink
                             SQLite + files (pages, ink)
```

The service has three surfaces:

- `/app` is the parent web UI, behind Cloudflare Access.
- `/api` serves that UI.
- `/device/v1` is the pad API. It bypasses Access and requires a per-device
  bearer token. Vellum uses the same split
  (`server/ai.mjs` handles device routes before the browser auth check).

Deployment: a user systemd service on gauntlet, with one ingress line
(`tutor.mergecombinator.com`, for example) added to the existing
`editorial-mergecombinator` Cloudflare tunnel. The DNS record goes in through
the Cloudflare dashboard, since gauntlet has no `cert.pem`.

The pad's local practice stays as it is: math generators, games, chess, the
adaptive ladder. Assigned lessons are an additional deck.

## Model router

Every model call goes through `run(task, input) → output`. Each task maps to
an ordered chain of routes; a route that fails, times out or returns output
that does not parse against the task's JSON schema passes the call to the
next route in the chain.

| Backend | How it runs | Cost | Latency |
|---|---|---|---|
| `cli` | Subprocess on gauntlet: `claude -p`, `codex exec`, `grok`, `hermes`. Prompt on stdin, images as file paths | Subscription allowance | 5–30 s, includes startup |
| `cloud` | Any OpenAI-compatible endpoint: OpenRouter, the Cloudflare AI Gateway Vellum uses, Anthropic direct | Per token | 1–4 s |
| `local` | OpenAI-compatible local server (Ollama, llama.cpp) | Free | Varies |

Default routes for this household:

| Task | Chain | Reason |
|---|---|---|
| `chat` | `cli:claude` → `cloud` | Long context; latency-tolerant |
| `generate` | `cli:claude` → `cli:codex` | Heavy reasoning; runs in the background |
| `verify` | `cli:codex` → `cloud` | A different model checks the first |
| `grade` | `cloud` (fast vision model) → `cli` | A child is waiting at the page |
| hints | none | Written with the lesson; dealt from the cache |

Grading is the one task routed to `cloud` first. CLI startup alone makes a
child wait too long, and grading calls are small. Generation, verification
and chat carry most of the token volume, and those run on subscriptions.

Configuration lives in `routes.toml` on the server and is editable from a
settings page in the web app. A user without CLIs sets every chain to
`cloud`; nothing else changes. `local` is for trivial tasks such as
splitting a word into sounds, never for grading.

CLI guardrails:

- Each call runs in an empty temporary directory with no tools beyond
  reading the attached images, using each CLI's own allowlist or sandbox
  flags.
- A hard timeout per task, and at most two CLI runs at once.
- Output is parsed against the task's schema by the service, not trusted.
- Every call is logged with task, route, duration and outcome, so the
  settings page shows what each subscription absorbed.

## Lessons

### Data model

- `learner`: `kind` (`child` | `adult`), name, age band, reading level,
  interests, and an editable `NOTES.md` (the counterpart of Ari's `USER.md`),
  included in every generation prompt.
- `lesson`: title, subject, learner, status
  (`draft → verifying → ready → assigned → done`), revision history.
- `page`: template ID, content JSON, answer key, three hints, rendered PNG,
  regions JSON.
- `attempt`: page, region, ink PNG, verdict, reply text, route used, hints
  used, timestamp. v1 writes these so phase 2 has history to read.

### From chat to page

1. **Chat.** The parent describes what they want. The tutor proposes an
   outline. Nothing is generated until the parent presses **Create**.
2. **Generate.** The model fills page content against the fixed templates
   below, not free HTML, and writes the answer key and three hints per
   question. Hints guide the next step and never state the answer.
3. **Verify.** A second model answers every question from the prompt alone,
   checks it against the key, checks that hints do not give the answer away,
   and checks the reading level against the learner. One revision round
   follows a failure. A second failure leaves the lesson in
   "needs your eyes" with the reasons shown.
4. **Preview.** The web app shows the exact PNGs the pad will display. Inline
   text edits re-render the page; larger changes go through chat.
5. **Assign.** **Send to pad** marks the lesson assigned; the pad picks it up
   on its next pull.

### Templates (v1)

Each template is an HTML/CSS file with named answer-region slots. After
layout the renderer measures each slot's bounding box in the DOM and writes
it to the regions JSON, so regions always match the drawn page.

| Template | Answer | Grading |
|---|---|---|
| Read and answer: a passage, 1–3 questions | Handwriting boxes | Vision model against the key |
| Pick one: 2–4 choices | Tick boxes | On the pad, against the key, no network |
| Word work: trace, fill the missing letter, match | Boxes or ticks | Mixed |
| Draw and label: a picture prompt | Open canvas | Model; encouragement only, no verdict |

Pictures, when a lesson needs them, come from a cloud image model. The
renderer converts them to line art suited to e-ink, and the verify step
checks them with the rest of the page.

Rendering: Playwright drives headless Chromium at 1404×1872, output in
grayscale. Gauntlet is a server with no WebGL content in these pages, so the
CPU renderer is acceptable there.

## Pad side

New module `src/learn/assigned.rs`, plus small hooks in `learn/mod.rs` and
`sheet.rs`.

- **Pull.** A background thread calls
  `GET /device/v1/lessons?since=<rev>` on Learn entry and every few minutes
  while Learn is open. Pages download into `lessons/<id>/` beside the binary
  and are listed only once every file is present.
- **Deck.** The menu shows a **LESSONS** entry when lessons are waiting.
  Assigned pages are dealt before generated ones in the default rotation. A
  finished lesson moves to `lessons/done/` and is not dealt again.
- **Page.** The PNG is drawn full-screen; `regions.json` becomes the
  `HitMap`. DONE, NEW, MENU and answer boxes behave as they do today. Two new
  targets:
  - **HINT** prints the next of the three stored hints and records its use.
  - **Tick boxes** are graded on the pad against a key shipped only for
    tick-box questions.
- **Grade.** On DONE, each answer box's ink is cropped as today and posted to
  `POST /device/v1/grade {lesson, page, region, png}`. The reply uses the
  existing protocol (verdict word, cheer, hint), so `verdict.rs` parses it
  and the stamp, dwell and auto-deal code is reused unchanged.

Answer keys for handwritten questions stay on the server; the grade route
looks them up.

### Failures

| Failure | Behavior |
|---|---|
| Offline at DONE | Ink saved to `outbox/`; the sheet prints "SAVED! WE'LL CHECK IT SOON" and deals the next page. The outbox drains on the next successful pull; the verdict shows in the web app |
| Grader timeout or unparseable reply | `Verdict::Unknown`: no mark, ladder untouched |
| Lesson download fails partway | Not listed; retried on the next pull |
| Device token rejected | Learn runs the local deck only; the SYSTEM page shows "tutor: unauthorized" |

## Scaling to adults

- `learner.kind` selects register and grading style. It does not fork the
  code.
- Children get YES / ALMOST / NO, at most three cheer words and a hint for
  the adult. Adults get Ari's register: per-criterion rubric scoring of
  visible steps, an observation and one next step. The pad already prints
  prose in Pad mode, so the adult stamp is a second layout.
- Adult lessons add a "worked problem" template (prompt, open work area,
  rubric) with KaTeX in the renderer.
- Adults may chat their way to multi-lesson curricula, as in Ari. For
  children the parent remains the author.

## Phases

| Phase | Ships |
|---|---|
| 1 | Service skeleton, router (`cli`, `cloud`, `local`), chat, generate → verify → revise, four templates, renderer, device API, pad LESSONS deck with HINT, tick boxes and outbox |
| 2 | Progress per skill from attempt records, ink review with verdict override, daily note (also written to the Vellum vault) |
| 3 | Parent sets today's plan and the pad follows it; spaced review drives the local deck as well |
| 4 | Adult learner kind, rubric grading, curricula |

## Testing

- **Server.** Router fallback tests with fake CLIs (scripts that sleep, exit
  non-zero or print invalid JSON); schema validation for every task;
  renderer tests that assert regions JSON by measurement, not pixel diffs.
- **Pad.** `g-pad --lesson-test <dir>` loads a cached lesson, writes
  simulated ink into each box, posts it and prints the verdicts, mirroring
  `--learn-test`. Unit tests for the tick-box grader and the outbox.
- **End to end.** One real lesson generated through `cli:claude`, previewed
  in the web app, pulled on the rM2, and graded from a handwritten answer on
  hardware.

## Out of scope for v1

- Progress views, daily notes, verdict overrides (phase 2).
- Parent-set plans and spaced review (phase 3).
- Adult learners (phase 4).
- Child-facing chat. The child's interface stays pen marks on the page.
