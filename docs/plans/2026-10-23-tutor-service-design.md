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

---

# Eng review (2026-10-23)

Target: `docs/plans/2026-10-23-tutor-service-design.md` (this file, as merged in
PR #24 at 7332036). Reviewer: /plan-eng-review. The design above is the plan
baseline; amendments below apply only where an answer approved them.

## Scope record

feature answers: D1 local backend = Defer; D2 lesson pictures = Keep in phase 1;
D3 routes editor = Defer editor, keep read-only usage view; structure: D4 = B
Smaller arrangement; accepted scope: phase 1 = chat, generate → verify →
revise, four templates, generated pictures with line-art conversion, renderer,
device API, router with `cli` and `cloud` backends, read-only per-route usage
view, pad LESSONS deck with HINT, tick boxes and outbox; `tutor/` is one Node
package with modules `router/`, `lessons/` (pipeline, templates, renderer),
`device/` (pull, grade with key lookup), `app/` (API plus Vite web served as
static), `db`; pad side `src/learn/assigned.rs` plus hooks; pending remedies:
R1 onward (Scope Challenge and section findings).

## Factual corrections (no behavior change)

- C1. Tunnel and DNS. Plan line 62 says to add an ingress line to the
  `editorial-mergecombinator` tunnel and that gauntlet has no `cert.pem`. The
  running tunnel loads a Nix-generated config
  (`cloudflared-mergecombinator.service`, ExecStart
  `--config /nix/store/…-cloudflared-mergecombinator.yml`); its ingress map is
  declared in gauntlet `~/Dev/dotfiles/flake.nix:339`
  (`"vellum.mergecombinator.com" = "http://127.0.0.1:8787";`).
  `~/.cloudflared/cert.pem` now exists on gauntlet. Deployment is therefore a
  `tutor.nix` Home Manager module beside `vellum.nix` (which packages
  `vellum-server` at `vellum.nix:47`) plus one ingress entry in `flake.nix`,
  applied with the dotfiles switch. Probe: ssh gauntlet, 2026-10-23.
- C2. Tick boxes exist. Plan lines 177-180 list tick boxes as a new target.
  They shipped in `0f3eef3` ("tick-box questions marked on the tablet") and
  `c2a1e39`; `src/learn/sheet.rs:180` "Tick boxes, on a page answered by
  ticking: marked on the tablet." The LESSONS deck reuses that code; only
  loading the key from `regions.json` is new.
- C3. Vellum precedent. Plan line 58 says Vellum uses the same split with a
  per-device token. Vellum has one shared token: `server/security.mjs:8`
  `const deviceToken = env.VELLUM_DEVICE_TOKEN || "";`, and the Access bypass
  is configured at the edge by path (`security.mjs:46`). A new hostname needs
  its own Access application with a bypass policy for `/device/v1/*`.
- C4. Prose on the pad. Plan line 205 says the pad prints prose in Pad mode.
  The Pad page draws replies as animated strokes (`src/main.rs:4208`
  `plan_reply`); typeset prose lives in the reading tabs (`src/page.rs:1-4`).
  The adult stamp layout would reuse `page.rs`.

## Decision ledger

### R1: How many answer boxes an assigned page grades
Finding: S1, P1, confidence 9/10, plan line 181-182, reviewer plan-eng-review.
Plan baseline: "On DONE, each answer box's ink is cropped as today and posted
to `POST /device/v1/grade`" (plan line 181-182); Read and answer has 1-3
questions per page (plan line 150).
Runtime evidence: today only the last box is graded. `src/main.rs:2254`
"Only the graded (last) blank travels."; `src/learn/sheet.rs:54-57`
"The graded blank: on a practice sheet, the LAST question's box. The oracle
marks only this one (the multi-question MVP)". `HitMap.answer` is one `BBox`
and the stamp anchors there.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R1 graded boxes per assigned page | plan: every box; code: last box only | every handwriting box posted and stamped at its own box; page counts YES only when every box is YES; per-box attempt rows | last box only, as practice today; templates put at most one handwritten question per page |
| R2+ other findings | pending | pending | pending |
Question D5:
D5 — Should an assigned page grade every handwritten answer box, or only the last one as practice does today?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; plan line 181 assumes every box is graded "as today", but today the pad grades only the last box.
ELI10: A Read and answer page can have up to three questions. The plan says the pad sends each answer box to the grader "as today". Today the pad sends only the last box and stamps one verdict; earlier boxes just need ink. So grading every box is new pad work: one grade call per box, a stamp beside each box, and a rule for when the page counts as right.
Stakes if we pick wrong: either a child answers three questions and only one is checked, or phase 1 grows by per-box stamping on the pad.
Recommendation: A) Grade every box because a lesson page with ungraded answers misleads the child and leaves holes in the attempt records phase 2 reads.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Grade every box (recommended)
  ✅ Every question the child answers gets a verdict and an attempt row for phase 2 progress
  ✅ Templates can keep 1-3 questions per passage, as the design intends
  ❌ New pad work: per-box stamp anchors and an all-YES page rule (human: ~1 day / CC: ~30 min)
B) Last box only
  ✅ Reuses today's one-verdict sheet code unchanged
  ✅ No change to practice-sheet stamping, so no regression risk there
  ❌ Earlier questions on a page go unchecked, or each passage needs one page per question
Net: A finishes the job the plan describes; B keeps the pad simple by cutting what a page can ask.
Header: Graded boxes
Options:
A) Grade every box (recommended)
Each handwriting box is posted to /device/v1/grade and stamped at its own box; the page counts YES only when every box is YES; one attempt row per box. Pad work: per-box stamp anchors (human: ~1 day / CC: ~30 min).
B) Last box only
Keep today's contract: only the last box is graded and stamped. Templates allow at most one handwritten question per page; extra questions become extra pages.

State: approved
Actual answer: A) Grade every box (D5, 2026-10-23)
Accepted scope: each handwriting box on an assigned page is posted to
`/device/v1/grade` and stamped at its own box; the page counts YES only when
every box is YES; one attempt row per box; pad gains per-box stamp anchors.
History: none

### R2: How the renderer gets a working Chromium on gauntlet
Finding: S2, P1, confidence 8/10, plan line 159-161, reviewer plan-eng-review.
Plan baseline: "Playwright drives headless Chromium at 1404×1872" (plan line 159); no install method named.
Runtime evidence: gauntlet is NixOS 26.11 (`/etc/os-release`), with no
`chromium` on PATH and no `~/.cache/ms-playwright` (probe 2026-10-23). The
NixOS wiki states browsers fetched by `playwright install` fail there and
recommends nixpkgs `playwright-driver.browsers` via `PLAYWRIGHT_BROWSERS_PATH`,
with the npm Playwright version pinned to match (wiki.nixos.org/wiki/Playwright;
discourse.nixos.org/t/66267). Version drift fails at launch with "browser
revision not found".
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R2 Chromium source | unspecified | nixpkgs `playwright-driver.browsers` in `tutor.nix`, `PLAYWRIGHT_BROWSERS_PATH` set, npm `playwright-core` pinned to the nixpkgs version, startup self-test render that fails the service loudly | nixpkgs `chromium` passed as `executablePath` to `playwright-core`, no version pin, startup self-test render |
| R1 graded boxes | approved D5 A | every box | every box |
| R3+ | pending | pending | pending |
Question D6:
D6 — How should the tutor's page renderer get a Chromium that runs on gauntlet?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; plan line 159 uses Playwright with headless Chromium but gauntlet is NixOS with no browser installed.
ELI10: Gauntlet runs NixOS. The browser Playwright normally downloads does not start there because its libraries are not where it expects. NixOS packages a matching browser set, and the npm library version has to match it exactly or launch fails. The other route points Playwright at NixOS's own Chromium build, which works but Playwright only promises compatibility with its bundled browser revision.
Stakes if we pick wrong: every lesson preview and pad page fails to render after a nixpkgs update, and the parent sees a broken Create.
Recommendation: A) Pinned playwright-driver because it is the documented NixOS path, and a startup self-test catches drift on deploy instead of at Create.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Pinned playwright-driver (recommended)
  ✅ The documented NixOS route; browser revision matches the library Playwright tests against
  ✅ The self-test render at service start fails the deploy, not a parent's lesson
  ❌ npm playwright-core must be bumped in step with nixpkgs updates (human: ~1 hr per bump / CC: ~5 min)
B) nixpkgs chromium via executablePath
  ✅ No version coupling to manage between npm and nixpkgs
  ✅ Uses the same Chromium the rest of the system already updates
  ❌ Playwright does not promise behavior on an unbundled revision; layout drift shows up as region mismatches
Net: A trades a pin to maintain for a supported browser; B trades the pin for unsupported drift.
Header: Chromium source
Options:
A) Pinned playwright-driver (recommended)
tutor.nix uses nixpkgs playwright-driver.browsers and sets PLAYWRIGHT_BROWSERS_PATH; npm playwright-core is pinned to that version; the service renders a fixed test page at start and exits non-zero if launch or measurement fails.
B) nixpkgs chromium via executablePath
playwright-core launches nixpkgs chromium through executablePath with no version pin; the same startup self-test render runs.

State: approved
Actual answer: A) Pinned playwright-driver (D6, 2026-10-23)
Accepted scope: `tutor.nix` uses nixpkgs `playwright-driver.browsers` and
sets `PLAYWRIGHT_BROWSERS_PATH`; npm `playwright-core` is pinned to that
version; the service renders a fixed test page at start and exits non-zero if
launch or measurement fails.
History: none

### R3: Which pad player a lesson belongs to
Finding: S3, P2, confidence 9/10, plan line 115-118 and 168-174, reviewer plan-eng-review.
Plan baseline: server `learner` has kind, name, age band, reading level,
interests and NOTES.md (plan line 115-117); the pad deck deals assigned
pages "before generated ones" (plan line 173). No link between the two.
Runtime evidence: the pad has a roster of up to four players keyed by name,
`src/learn/players.rs:19` `pub const MAX_PLAYERS: usize = 4;`, and
`players.rs:3-4` "Players are animals (FOX, OWL, BEAR) unless
`RIDDLE_LEARN_PLAYERS` names them — nothing about the children is stored or
shown." Stars, level and trophies are per player.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R3 lesson-to-player link | none | `learner.player` holds a pad roster name (FOX); device API sends only that alias; pad deals a lesson only while that player is active; attempts and stars go to that player; child names stay on the server | no link; lessons deal to whoever is active; attempts record the active player's alias |
| R1, R2 | approved D5 A, D6 A | unchanged | unchanged |
| R4+ | pending | pending | pending |
Question D7:
D7 — How should a lesson made for one child reach that child on a pad shared by several players?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; the plan's server learners have no link to the pad's FOX/OWL/BEAR player roster.
ELI10: The pad already has up to four players, each with their own level and stars, named after animals so no child's name is stored on the tablet. The web app will know each child's real name, age and interests. The plan never says how a lesson for one child finds that child on the pad, or whose stars it earns. Linking each learner to a pad player name keeps lessons with the right child and keeps real names off the pad.
Stakes if we pick wrong: a sibling gets a lesson written for another child's reading level, and stars land on the wrong player.
Recommendation: A) Link learner to player because it keeps lessons at the right reading level and holds the pad's no-names rule.
Completeness: A=10/10, B=5/10
Pros / cons:
A) Link learner to player (recommended)
  ✅ Each lesson is dealt only to the player it was written for, at that child's level
  ✅ The device API carries only the animal alias, so the pad still stores no child names
  ❌ The parent must pick a player for each learner, and renaming a roster entry needs the same rename in the web app
B) Deal to whoever is active
  ✅ No mapping to set up or keep in sync
  ✅ Works with a one-child household as is
  ❌ With two children, either can receive the other's lesson and the stars follow whoever held the pen
Net: A costs one dropdown per learner; B breaks as soon as a second child uses the pad.
Header: Lesson owner
Options:
A) Link learner to player (recommended)
Each server learner stores a pad player name (FOX, or a RIDDLE_LEARN_PLAYERS name). Pulls send only that alias; the pad deals a lesson only while that player is active; attempts and stars go to that player. Child names never reach the pad.
B) Deal to whoever is active
No learner-to-player link. Assigned lessons deal to the active player; each attempt records that player's alias.

State: approved
Actual answer: A) Link learner to player (D7, 2026-10-23)
Accepted scope: each server learner stores a pad player name (FOX, or a
`RIDDLE_LEARN_PLAYERS` name); pulls send only that alias; the pad deals a
lesson only while that player is active; attempts and stars go to that
player; child names never reach the pad.
History: none

### R4: Device token model
Finding: S4, P2, confidence 8/10, plan line 57-58, reviewer plan-eng-review.
Plan baseline: "`/device/v1` is the pad API. It bypasses Access and requires a
per-device bearer token." (plan line 57-58)
Runtime evidence: Vellum, cited as the precedent, uses one shared token:
`~/Dev/vellum/server/security.mjs:8` `const deviceToken = env.VELLUM_DEVICE_TOKEN || "";`
compared with `crypto.timingSafeEqual` (`security.mjs:58-66`). The pad reads
tokens from `oracle.env` beside the binary (`src/main.rs:110`). One rM2 in the
household.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R4 device token | per-device (plan), none built | one shared `TUTOR_DEVICE_TOKEN` in the service env and `RIDDLE_TUTOR_TOKEN` in `oracle.env`, constant-time compare as in Vellum; rotate by changing both | per-device tokens stored hashed in SQLite, issued and revoked from the web app, each attempt tagged with its device |
| R1-R3 | approved D5-D7 A | unchanged | unchanged |
| R5+ | pending | pending | pending |
Question D8:
D8 — Should the pad API use one shared device token, as Vellum does, or a token per device?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; plan line 58 asks for per-device bearer tokens and cites Vellum, which uses one shared token.
ELI10: The pad proves it is allowed to pull lessons and send ink by sending a secret token. Vellum uses one secret shared by every device. Per-device tokens let you cut off one lost tablet without touching the others, but they need a token table, an issue screen and a revoke button. The household has one rM2 today.
Stakes if we pick wrong: build token management nobody uses yet, or have to rotate the one shared secret on every device if a tablet is lost.
Recommendation: A) One shared token because there is one pad, Vellum's pattern is proven here, and moving to per-device later only adds a table behind the same header.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) One shared token (recommended)
  ✅ Same pattern and config style as Vellum and the hub, nothing new to learn
  ✅ No token table, issue screen or revoke flow in phase 1
  ❌ A lost tablet means rotating the secret everywhere it is used
B) Token per device
  ✅ Revoke one tablet without touching others, and attempts record which device sent them
  ✅ Ready for a second pad or a family member's device
  ❌ Adds a hashed token table, an issue screen and revoke handling (human: ~1 day / CC: ~30 min)
Net: per-device tokens solve a second-device problem the household does not have yet.
Header: Device token
Options:
A) One shared token (recommended)
TUTOR_DEVICE_TOKEN on the server and RIDDLE_TUTOR_TOKEN in the pad's oracle.env, constant-time compare as in Vellum. Rotate by changing both.
B) Token per device
Per-device tokens stored hashed in SQLite, issued and revoked from the web app; each attempt records its device.

State: approved
Actual answer: A) One shared token (D8, 2026-10-23)
Accepted scope: `TUTOR_DEVICE_TOKEN` on the server and `RIDDLE_TUTOR_TOKEN`
in the pad's `oracle.env`, constant-time compare as in Vellum; rotate by
changing both. Per-device tokens are not built.
History: none

### R5: Where lessons and the outbox live on the pad
Finding: S5, P3, confidence 8/10, plan line 170-171 and 193, reviewer plan-eng-review.
Plan baseline: "Pages download into `lessons/<id>/` beside the binary"
(plan line 170-171); offline ink goes to `outbox/` (plan line 193).
Runtime evidence: the binary lives in `/home/root/xovi/exthome/appload/g-pad`
(`scripts/install.sh:32`); all mutable pad data goes to
`/home/root/g-pad-data`, overridable by `RIDDLE_DATA_DIR`
(`src/oracle.rs:24` `const DATA_DIR: &str = "/home/root/g-pad-data";`,
`src/learn/players.rs:63-66`), and uninstall names both directories
(`scripts/install.sh:136-137`).
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R5 lesson + outbox location | beside the binary (plan) | `$RIDDLE_DATA_DIR/lessons/<id>/`, `lessons/done/` and `$RIDDLE_DATA_DIR/tutor-outbox/` | `lessons/` and `outbox/` inside the AppLoad app dir beside the binary |
| R1-R4 | approved D5-D8 A | unchanged | unchanged |
| R6+ | pending | pending | pending |
Question D9:
D9 — Where should downloaded lessons and unsent ink live on the pad?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; plan line 170 puts lessons beside the binary, while every other piece of pad data lives in /home/root/g-pad-data.
ELI10: The app's program files sit in one folder that the installer copies over on every update. Everything the pad saves for itself (players, settings, memories) goes in a separate data folder that tests can redirect with RIDDLE_DATA_DIR. The plan puts lessons and the offline ink queue next to the program instead. Following the data-folder rule keeps one place to back up, wipe or point tests at.
Stakes if we pick wrong: lessons and unsent answers sit in the folder the installer overwrites, and tests cannot redirect them like the rest of the pad's data.
Recommendation: A) Data folder because it follows the pad's one rule for mutable data and lets --lesson-test run against a temp dir.
Completeness: A=10/10, B=6/10
Pros / cons:
A) Data folder (recommended)
  ✅ Same home as players, preferences and memories; RIDDLE_DATA_DIR redirects it for tests
  ✅ Survives a reinstall that replaces the app folder
  ❌ None beyond a one-line path change in the plan
B) Beside the binary
  ✅ Matches the plan text as written
  ✅ Install's cp -Rf does not wipe the app folder today, so files survive updates
  ❌ A second data location to document, back up and clean, outside the RIDDLE_DATA_DIR test redirect
Net: A follows the existing convention at no cost.
Header: Lesson storage
Options:
A) Data folder (recommended)
Lessons in $RIDDLE_DATA_DIR/lessons/<id>/ with finished ones in lessons/done/; unsent ink in $RIDDLE_DATA_DIR/tutor-outbox/. Default /home/root/g-pad-data.
B) Beside the binary
lessons/ and outbox/ inside /home/root/xovi/exthome/appload/g-pad, as the plan says.

State: approved
Actual answer: A) Data folder (D9, 2026-10-23)
Accepted scope: lessons in `$RIDDLE_DATA_DIR/lessons/<id>/`, finished ones in
`lessons/done/`; unsent ink in `$RIDDLE_DATA_DIR/tutor-outbox/`; default
`/home/root/g-pad-data`.
History: none

## Section 1: Architecture

### R6: Exact lockdown for CLI model calls
Finding: A1, P2, confidence 7/10, plan line 103-105, reviewer plan-eng-review.
Plan baseline: "Each call runs in an empty temporary directory with no tools
beyond reading the attached images, using each CLI's own allowlist or sandbox
flags." (plan line 103-105)
Runtime evidence: the CLIs run as `m4ndolore` on gauntlet with credentials in
`~/.claude/.credentials.json` and `~/.codex/auth.json` and Vellum secrets in
`~/Dev/vellum/.env.local` (probe 2026-10-23). An empty working directory does
not stop a tool from reading `~`. Local `claude --help`: `--restricted`
"removes the built-in tools … and ignores user, project and local settings …
Also confines the file tools to the working directories"; `--strict-mcp-config`
skips MCP servers. `codex exec --help`: `-s read-only`, `--ephemeral`,
`--ignore-user-config`, `-i <FILE>`. Codex's read-only sandbox limits writes
and commands, not reads (unverified on gauntlet).
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R6 CLI lockdown | "allowlist or sandbox flags", unspecified | pinned argv per CLI: `claude -p --restricted --strict-mcp-config --no-session-persistence --tools Read --json-schema <task>` in a fresh temp dir holding only the images; `codex exec -s read-only --ephemeral --ignore-user-config --skip-git-repo-check -C <tmp> -i <img> --output-schema <task>`; env stripped to PATH and HOME; a unit test asserts the exact argv | same flags, plus the service unit sets `ProtectSystem=strict`, `PrivateTmp`, and `InaccessiblePaths` for `~/Dev`, `~/.ssh` and `~/.cloudflared`, so even a CLI that ignores its flags cannot read those |
| R1-R5 | approved D5-D9 A | unchanged | unchanged |
| R7+ | pending | pending | pending |
Question D10:
D10 — How tightly should the tutor lock down the CLI model calls it runs on gauntlet?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; plan line 103 says CLI calls get "no tools beyond reading the attached images" but names no flags, and an empty temp dir does not stop a tool from reading your home folder.
ELI10: The tutor will run claude and codex as programs on gauntlet, logged in as you. Your home folder there holds their login files, Vellum's secrets and your ssh keys. The plan wants each call to see only the images, but a temporary folder alone does not enforce that: a model allowed to read files can read anything your account can. Pinning the exact flags (restricted mode, read-only sandbox) closes most of it; adding systemd's file blocks makes the secret folders invisible even if a flag is wrong.
Stakes if we pick wrong: a prompt that tricks a model into reading files could put a secret into lesson text the parent sees, or into the call log.
Recommendation: B) Flags plus systemd blocks because flags depend on each CLI keeping its promises, and the systemd lines cost a few lines in tutor.nix.
Completeness: A=8/10, B=10/10
Pros / cons:
A) Pinned flags only
  ✅ Exact argv per CLI, checked by a unit test so a refactor cannot drop a flag
  ✅ No service hardening to debug when a CLI needs a path it did not need before
  ❌ Relies on each CLI's restricted mode; codex's read-only sandbox still allows reads
B) Flags plus systemd blocks (recommended)
  ✅ Same pinned argv and test, plus ~/Dev, ~/.ssh and ~/.cloudflared are invisible to the service
  ✅ Holds even if a future CLI release changes what restricted mode means
  ❌ A CLI update that needs a newly blocked path fails until tutor.nix is adjusted (human: ~2 hr / CC: ~15 min, plus hardware check)
Net: A trusts each CLI's sandbox; B adds an OS-level wall for a few lines of config.
Header: CLI lockdown
Options:
A) Pinned flags only
claude -p --restricted --strict-mcp-config --no-session-persistence --tools Read --json-schema, and codex exec -s read-only --ephemeral --ignore-user-config -C <tmp> -i <img> --output-schema; env stripped to PATH and HOME; a unit test asserts each argv.
B) Flags plus systemd blocks (recommended)
The same pinned argv and test, and the tutor unit sets ProtectSystem=strict, PrivateTmp and InaccessiblePaths for ~/Dev, ~/.ssh and ~/.cloudflared.

State: approved
Actual answer: B) Flags plus systemd blocks (D10, 2026-10-23)
Accepted scope: pinned argv per CLI (`claude -p --restricted
--strict-mcp-config --no-session-persistence --tools Read --json-schema`;
`codex exec -s read-only --ephemeral --ignore-user-config
--skip-git-repo-check -C <tmp> -i <img> --output-schema`), env stripped to
PATH and HOME, a unit test asserting each argv; the tutor unit in `tutor.nix`
sets `ProtectSystem=strict`, `PrivateTmp` and `InaccessiblePaths` for `~/Dev`,
`~/.ssh` and `~/.cloudflared`, checked on gauntlet.
History: none

### R7: Which CLIs the phase 1 router supports
Finding: A2, P3, confidence 7/10, plan line 78, reviewer plan-eng-review.
Plan baseline: `cli` backend runs "`claude -p`, `codex exec`, `grok`,
`hermes`. Prompt on stdin, images as file paths" (plan line 78). Default
chains use only `cli:claude` and `cli:codex` (plan line 86-89).
Runtime evidence: all four are on gauntlet's PATH (probe 2026-10-23).
`claude` and `codex` have headless, schema and image flags (R6). `grok --help`
on gauntlet: "Grok Build TUI … [PROMPT] Initial prompt for the interactive
session", with no headless or image flag in the first page of help; `hermes`
help was not read. Their lockdown flags under R6 are unverified.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R7 CLI adapters in phase 1 | four named | `claude` and `codex` adapters only, each with its pinned argv test; `grok` and `hermes` added later once a headless image mode and R6-grade lockdown are verified | all four adapters, each needing its own verified headless, image and lockdown flags before merge |
| R6 lockdown | approved D10 B | applies to claude, codex | applies to all four |
| R8+ | pending | pending | pending |
Question D11:
D11 — Should the phase 1 router ship adapters for grok and hermes, or only claude and codex?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; plan line 78 names four CLIs, but the default routes use only claude and codex, and grok's help shows an interactive TUI.
ELI10: Each CLI needs its own adapter: how to send a prompt, attach an image, force JSON output and lock down tools. claude and codex document all four. grok's help describes an interactive program, and hermes was not checked. No default route uses either. Shipping them now means proving each one's lockdown flags before merge.
Stakes if we pick wrong: two adapters with unverified lockdown, or a later small add when you want grok in a chain.
Recommendation: A) claude and codex only because no default chain uses the others and their lockdown cannot be verified today.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) claude and codex only (recommended)
  ✅ Both adapters have documented headless, image, schema and lockdown flags
  ✅ Two argv tests instead of four, matching what the default chains call
  ❌ Using grok or hermes in a chain waits for its own adapter
B) All four adapters
  ✅ Every subscription you hold can absorb load from day one
  ✅ Matches the plan's backend table as written
  ❌ grok and hermes need verified headless image and lockdown flags first (human: ~1 day / CC: ~45 min, plus probing)
Net: ship the two adapters the routes use; add others when a route needs them.
Header: CLI adapters
Options:
A) claude and codex only (recommended)
Phase 1 cli backend has claude and codex adapters with pinned argv tests. grok and hermes are added later once a headless image mode and R6 lockdown are verified.
B) All four adapters
Phase 1 ships claude, codex, grok and hermes adapters; each needs verified headless, image and lockdown flags before merge.

State: approved
Actual answer: A) claude and codex only (D11, 2026-10-23)
Accepted scope: phase 1 `cli` backend has `claude` and `codex` adapters with
pinned argv tests; `grok` and `hermes` are added later once a headless image
mode and R6 lockdown are verified.
History: none

### R8: Retried grades must not record twice
Finding: A3, P2, confidence 8/10, plan line 123 and 193, reviewer plan-eng-review.
Plan baseline: `attempt` rows hold page, region, ink, verdict, route, hints,
timestamp (plan line 122-123); offline ink "drains on the next successful
pull" (plan line 193); a grader timeout yields `Verdict::Unknown` (plan line 194).
Runtime evidence: the pad's chat call reads with a 90 s per-read timeout
(`src/oracle.rs:558-559`) inside a 120 s app limit (`src/main.rs:75`
`const ORACLE_PATIENCE: Duration = Duration::from_secs(120);`). A grade that
the server finishes after the pad gave up is followed by a resend from the
outbox, so the server sees the same ink twice. No attempt identifier exists
in the plan.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R8 attempt identity | none | pad assigns each box submission an `attempt_id` (lesson, page, region, player, ink hash); `POST /device/v1/grade` upserts on it and returns the stored verdict for a repeat without calling a model; outbox files are named by it | no identity; every post is graded and stored; duplicates are tolerated |
| R1-R7 | approved | unchanged | unchanged |
| R9+ | pending | pending | pending |
Question D12:
D12 — Should each graded answer carry an ID so a resend from the outbox is not graded and counted twice?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; the plan's outbox resends ink after a failure, but nothing stops the server from storing and grading the same answer twice.
ELI10: If the pad sends an answer, waits, and gives up just before the server replies, it keeps the ink and sends it again later. Without an ID, the server grades it twice: two attempt rows, two model calls, and possibly two different verdicts for the same answer. With an ID, the server notices the repeat and hands back the verdict it already has.
Stakes if we pick wrong: phase 2 progress counts double attempts, and a resend can contradict the stamp the child already saw.
Recommendation: A) Attempt ID because it makes resend safe and costs one field and one unique index.
Completeness: A=10/10, B=4/10
Pros / cons:
A) Attempt ID (recommended)
  ✅ A resend returns the stored verdict without another model call or row
  ✅ Outbox files are named by the ID, so a half-written retry cannot duplicate
  ❌ One more field in the pad payload and a unique index in SQLite (human: ~2 hr / CC: ~10 min)
B) No ID
  ✅ Simplest request shape
  ✅ Nothing to keep in sync between pad and server
  ❌ Duplicate attempts and possibly conflicting verdicts for one answer
Net: one field buys safe retries and clean phase 2 history.
Header: Attempt ID
Options:
A) Attempt ID (recommended)
The pad gives each box submission an attempt_id from lesson, page, region, player and ink hash. /device/v1/grade upserts on it and returns the stored verdict for a repeat without a model call; outbox files are named by it.
B) No ID
Every post is graded and stored; duplicates are tolerated.

State: approved
Actual answer: A) Attempt ID (D12, 2026-10-23)
Accepted scope: the pad gives each box submission an `attempt_id` from
lesson, page, region, player and ink hash; `/device/v1/grade` upserts on it
and returns the stored verdict for a repeat without a model call; outbox files
are named by it.
History: none

### R9: Who draws the footer and HINT on a lesson page
Finding: A4, P2, confidence 8/10, plan line 175-179, reviewer plan-eng-review.
Plan baseline: "The PNG is drawn full-screen; `regions.json` becomes the
`HitMap`. DONE, NEW, MENU and answer boxes behave as they do today." HINT is a
new target (plan line 175-179). The plan does not say whether the server
paints the footer into the PNG or the pad paints it.
Runtime evidence: `src/learn/sheet.rs:3-5` the hit map is "built as a
RETURNED VALUE of drawing so a region can never drift from what was painted".
`sheet.rs:108` `pub fn draw_footer(surf, ui_font) -> HitMap` paints NEW, MENU,
DONE at `box_top()`, each 22% wide and 8% tall (`sheet.rs:995-1012`).
`gesture.rs:76` `blit_gray` draws a gray bitmap and skips pixels ≥ 250,
leaving them paper white.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R9 footer ownership | unspecified | templates leave the footer band blank; the pad blits the PNG, then calls `draw_footer` plus a new HINT box, so those targets come from drawing; `regions.json` carries only answer and tick regions plus the body rectangle; the pad refuses a lesson whose regions fall outside the body; the web preview overlays the same footer | the server paints NEW, MENU, DONE and HINT into the PNG and lists them in `regions.json`; the pad builds the whole `HitMap` from JSON |
| R1-R8 | approved | unchanged | unchanged |
| R10+ | pending | pending | pending |
Question D13:
D13 — Should the pad or the server draw the DONE, NEW, MENU and HINT boxes on a lesson page?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; the plan has lesson pages arrive as a full-screen PNG plus regions.json, but does not say who draws the command boxes at the foot of the page.
ELI10: Every worksheet ends with NEW, MENU and DONE boxes the child marks to act. The pad's rule is that a tap target exists only because the pad drew it, so a box can never be where the picture is not. If the server paints those boxes into the PNG, the pad has to trust the JSON for its most important buttons. If the pad paints them on top of a blank band the template leaves, the buttons look and work exactly like every other page.
Stakes if we pick wrong: a template change or layout drift moves the DONE box off its picture, and the child cannot finish a page.
Recommendation: A) Pad draws the footer because it keeps the pad's drawn-target rule for commands and makes lesson pages look like every other sheet.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Pad draws footer (recommended)
  ✅ Command boxes come from draw_footer, identical to practice pages, never from JSON
  ✅ The pad rejects a lesson whose answer regions overlap the footer band, catching template bugs before a child sees them
  ❌ Templates must reserve the band, and the band height becomes a shared constant between server and pad
B) Server draws footer
  ✅ The preview in the web app shows the exact final page, footer included
  ✅ The pad's lesson code only blits and loads JSON
  ❌ The pad's most important targets come from data, against the rule in sheet.rs:3-5
Net: A keeps commands owned by the pad; B gives a pixel-exact preview at the cost of trusting JSON for DONE.
Header: Footer owner
Options:
A) Pad draws footer (recommended)
Templates leave the footer band blank. The pad blits the PNG, then draw_footer plus a new HINT box; regions.json carries only answer and tick regions and the body rectangle; lessons with regions outside the body are refused. The web preview overlays the same footer.
B) Server draws footer
The server paints NEW, MENU, DONE and HINT into the PNG and lists them in regions.json; the pad builds the whole HitMap from JSON.

State: approved
Actual answer: A) Pad draws footer (D13, 2026-10-23)
Accepted scope: templates leave the footer band blank; the pad blits the PNG,
then calls `draw_footer` plus a new HINT box; `regions.json` carries only
answer and tick regions and the body rectangle; lessons with regions outside
the body are refused; the web preview overlays the same footer.
History: none

### R10: Chat must not queue behind background generation
Finding: A5, P2, confidence 7/10, plan line 86-88 and 106, reviewer plan-eng-review.
Plan baseline: `chat` → `cli:claude`, `generate` → `cli:claude` → `cli:codex`,
`verify` → `cli:codex` (plan line 86-88); "at most two CLI runs at once"
(plan line 106).
Runtime evidence: with two lessons generating, or one generating while
another verifies, both slots are busy and a parent's chat message waits for a
5-30 s CLI run to finish (plan line 78 latency), then its own 5-30 s run.
Gauntlet has 16 cores and 30 GB RAM (probe 2026-10-23), so the cap is about
subscription rate limits, not hardware.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R10 CLI slot policy | two shared slots, FIFO | two slots plus one reserved for `chat` (three CLI processes at most); background tasks never take the chat slot; a test holds both background slots and asserts chat still starts | two shared slots with priority: chat jumps the queue but waits for a running call to finish |
| R1-R9 | approved | unchanged | unchanged |
| R11+ | pending | pending | pending |
Question D14:
D14 — How should the router keep the parent's chat responsive while lessons generate in the background?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; plan line 106 caps CLI runs at two, and chat, generate and verify all use CLIs.
ELI10: Chat, lesson writing and lesson checking all run as CLI programs, and the plan allows two at once. If two lessons are being written, a parent typing in chat waits for one of them to finish (up to half a minute) before their own reply even starts. Reserving a slot for chat means a reply always starts right away; priority alone only moves chat to the front of the line.
Stakes if we pick wrong: chat feels broken exactly when the parent is most active, right after pressing Create.
Recommendation: A) Reserved chat slot because chat is the only CLI task a person is waiting on, and one extra process costs nothing on gauntlet.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Reserved chat slot (recommended)
  ✅ A chat reply starts immediately however many lessons are in flight
  ✅ Background throughput stays at two runs, unchanged from the plan
  ❌ Up to three CLI processes at once, which uses more of the subscription rate limit
B) Priority queue only
  ✅ Keeps the plan's hard cap of two CLI processes
  ✅ Chat still goes ahead of queued background work
  ❌ Chat waits up to one full generation run (5-30 s) when both slots are busy
Net: A spends one process on the only latency-sensitive CLI task; B keeps the cap and accepts a wait.
Header: Chat slot
Options:
A) Reserved chat slot (recommended)
Two background CLI slots plus one reserved for chat (three processes at most). Background tasks never take the chat slot; a test holds both background slots and asserts chat still starts.
B) Priority queue only
Two shared slots; chat jumps the queue but waits for a running call to finish.

State: approved
Actual answer: A) Reserved chat slot (D14, 2026-10-23)
Accepted scope: two background CLI slots plus one reserved for chat (three
processes at most); background tasks never take the chat slot; a test holds
both background slots and asserts chat still starts.
History: none

## Section 2: Code quality

Implementation notes that approved answers already require (no question):
- Q1. A lesson page needs its own `Page::Lesson` variant in `src/learn/mod.rs`
  rather than a `Set`: `src/learn/problems.rs:970` `pub prompt: &'static str`
  cannot hold runtime text, and `Session::next()` (`mod.rs:384-414`) is where
  "assigned pages first" plugs in, filtered by the active player (R3). Per-box
  stamps (R1) anchor on the existing `HitMap.blanks`.
- Q2. The grade route replies in the pad's existing text protocol
  (`src/learn/verdict.rs:1-3`); the server converts the model's JSON verdict to
  that text so `verdict::parse` runs unchanged (plan line 183-184).
- Q3. The lessons pull thread follows `src/vault.rs:215-235` `spawn_poll`
  (thread, `access::builder()`, bounded timeout). `brief.rs:309` and
  `bridge.rs:280` repeat the pattern with different payloads and cadences; no
  shared helper is proposed, since the three differ in contract.

### R11: How an open-canvas page answers without a verdict
Finding: Q4, P2, confidence 8/10, plan line 153, reviewer plan-eng-review.
Plan baseline: Draw and label is graded by "Model; encouragement only, no
verdict" (plan line 153); the grade reply "uses the existing protocol
(verdict word, cheer, hint)" (plan line 183).
Runtime evidence: the protocol has only YES, ALMOST and NO
(`src/learn/verdict.rs:65-70`). Anything else is Unknown: `verdict.rs:7-8`
"the text still shows, but no mark is drawn and the ladder holds still", and
the whole text becomes the grown-up hint with an empty cheer
(`verdict.rs:71-73`). An encouragement reply would therefore show no cheer to
the child.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R11 no-verdict reply | not in protocol | new leading word `SEEN` in `verdict.rs`: no mark, ladder holds, cheer shown to the child, attempt stored as ungraded; new rows in the existing parse test table | reuse Unknown: server sends the encouragement as plain text; it prints in the hint line with no cheer and no mark |
| R1-R10 | approved | unchanged | unchanged |
| R12+ | pending | pending | pending |
Question D15:
D15 — How should the pad show encouragement on a Draw and label page, which has no right answer?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; plan line 153 says drawings get encouragement only, but the pad's reply protocol knows only YES, ALMOST and NO.
ELI10: The pad reads every grading reply by its first word: YES, ALMOST or NO decides the mark and the level, then a few cheer words go to the child. A drawing has no right answer, so it needs a reply that says "looked at it, nice work" without a mark. Today any other first word is treated as unreadable: no mark, and the text shows only in the small grown-up line, so the child sees no cheer.
Stakes if we pick wrong: a child finishes a drawing and the page shows them nothing, or a drawing accidentally moves their level.
Recommendation: A) New SEEN word because it gives the child their cheer while keeping the rule that only YES, ALMOST and NO move the ladder.
Completeness: A=10/10, B=5/10
Pros / cons:
A) New SEEN word (recommended)
  ✅ The child gets a cheer on every drawing; no mark is drawn and the ladder does not move
  ✅ Attempts are stored as ungraded, so phase 2 progress does not count drawings as right or wrong
  ❌ Changes verdict.rs, shared with practice pages; needs regression rows in its test table (human: ~2 hr / CC: ~10 min)
B) Reuse Unknown
  ✅ No pad protocol change
  ✅ The no-mark, no-ladder rule already holds for Unknown
  ❌ Encouragement prints only in the grown-up hint line; the child sees no cheer
Net: one protocol word gives the child the feedback the template promises.
Header: Drawing reply
Options:
A) New SEEN word (recommended)
verdict.rs gains a SEEN leading word: no mark, ladder holds, cheer shown to the child; the attempt is stored as ungraded. The existing parse test table gains SEEN rows and keeps every current row passing.
B) Reuse Unknown
The server sends encouragement as plain text; it prints in the hint line with no cheer and no mark.

State: approved
Actual answer: A) New SEEN word (D15, 2026-10-23)
Accepted scope: `verdict.rs` gains a `SEEN` leading word: no mark, ladder
holds, cheer shown to the child; the attempt is stored as ungraded; the
existing parse test table gains SEEN rows and keeps every current row passing.
History: none

## Section 3: Tests

Framework: pad uses inline Rust `#[test]` modules (`cargo test`, 321 passed,
0 failed at 7332036, run 2026-10-23, 63 s). `tutor/` has no framework yet.
CI (`.github/workflows/no-vendor-blob.yml`) runs no tests.

```
CODE PATHS (proposed)                                   USER FLOWS
[+] tutor/router                                        [+] Parent creates a lesson
  ├── run(task) chain walk                                ├── [GAP] [→E2E] chat → Create → verify → preview → Send to pad
  │   ├── [GAP] cli timeout → next route                  ├── [GAP] verify fails twice → "needs your eyes" with reasons
  │   ├── [GAP] cli exit non-zero → next route            ├── [GAP] inline edit re-renders the page and regions
  │   ├── [GAP] invalid JSON / schema miss → next route   └── [GAP] chat while two lessons generate (R10)
  │   └── [GAP] whole chain fails → task error surfaced [+] Child works a lesson on the pad
  ├── [GAP] argv per CLI is exactly the pinned set (R6)   ├── [GAP] [→E2E] LESSONS entry → page → ink 3 boxes → DONE → 3 stamps
  └── [GAP] chat slot free while 2 background run (R10)   ├── [GAP] HINT prints hint 1, 2, 3, then nothing new
[+] tutor/lessons                                         ├── [GAP] tick box graded on the pad, no network
  ├── generate → verify → revise → needs-your-eyes        ├── [GAP] drawing → SEEN cheer, no mark (R11)
  │   ├── [GAP] [→EVAL] generated content meets schema    └── [GAP] other player active → lesson not dealt (R3)
  │   ├── [GAP] [→EVAL] verify catches wrong key / leaky hint [+] Failure states the child sees
  │   └── [GAP] one revision, then needs-your-eyes        ├── [GAP] offline DONE → "SAVED! WE'LL CHECK IT SOON", next page
  ├── renderer                                            ├── [GAP] grader timeout → no mark, ladder still
  │   ├── [GAP] regions measured from DOM per slot        ├── [GAP] partial download → lesson not listed
  │   ├── [GAP] no region inside footer band (R9)         └── [GAP] token rejected → local deck, SYSTEM "TUTOR: UNAUTHORIZED"
  │   ├── [GAP] startup self-test render (R2)
  │   └── [GAP] picture → line art, 16-level gray
[+] tutor/device
  ├── [GAP] bearer check, 401 on bad token (R4)
  ├── [GAP] pull since=<rev> returns only that player's lessons (R3)
  └── [GAP] grade upsert on attempt_id, repeat returns stored verdict (R8)
[+] pad src/learn/assigned.rs + hooks
  ├── [GAP] lesson listed only when every file present
  ├── [GAP] regions.json → blanks/choices; refuse out-of-body regions (R9)
  ├── [GAP] outbox write, drain on next pull, named by attempt_id
  ├── [★★★ TESTED] verdict::parse YES/ALMOST/NO/Unknown — verdict.rs tests
  │   └── [GAP] SEEN rows (R11)
  └── [★★★ TESTED] practice stamp beside the answer box — sheet.rs:1812, :1836

COVERAGE: 2/37 paths tested (existing pad code only)  |  GAPS: 35 (2 E2E, 2 eval)
```

### R12: Regression contract for shared pad code
Finding: T1, P1 (CRITICAL regression risk), confidence 9/10, R1 and R11 accepted scope, reviewer plan-eng-review.
Plan baseline: R1 (D5 A) adds per-box stamps; R11 (D15 A) adds `SEEN` to
`verdict.rs`; both touch code that practice sheets use today.
Runtime evidence: practice behavior is pinned by `sheet.rs:1812`
`marks_land_next_to_the_answer_region`, `sheet.rs:1829`
`the_feedback_strip_sits_between_figure_and_boxes`, `sheet.rs:1836`
`feedback_stamps_inside_the_strip_and_a_hint_adds_ink`, `verdict.rs` tests
(`verdict_words_are_read_through_punctuation_and_case` and 5 more) and
`mod.rs:692` `recording_yes_answers_eventually_raises_the_level`; 321 passing.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R12 regression contract | none stated | practice and play pages keep today's behavior exactly: only the last blank is graded and stamped, ladder moves only on YES/ALMOST/NO, Unknown draws nothing; every existing test passes unedited; new tests add a practice-sheet assertion that earlier blanks get no stamp and that `SEEN` never moves the ladder | same preserved behavior, enforced by the existing tests only; new tests cover lesson pages alone |
| R1-R11 | approved | unchanged | unchanged |
| R13+ | pending | pending | pending |
Question D16:
D16 — What must stay exactly the same on practice pages while the pad learns per-box stamps and the SEEN word?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; approved D5 (grade every box) and D15 (SEEN) change sheet.rs and verdict.rs, which today's math practice uses.
ELI10: The lesson work changes two pieces of code that every math page already uses: where the right/wrong mark is drawn, and how the tutor's reply is read. Math pages must keep working exactly as now: one graded box, the mark next to it, the level moving only on YES, ALMOST or NO. The existing tests pin most of that. Adding two explicit tests closes the gaps nobody has pinned yet: earlier boxes on a math page get no stamp, and SEEN can never change a level.
Stakes if we pick wrong: a math page starts stamping every box or a drawing nudges a child's level, and nothing fails until a child sees it.
Recommendation: A) Contract plus two tests because the two new behaviors are exactly the ones the current tests do not pin.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Contract plus two tests (recommended)
  ✅ Existing 321 tests must pass unedited, so no one can quietly rewrite an assertion
  ✅ Two new tests pin the exact behaviors the change could break: earlier blanks unstamped on practice, SEEN never moves the ladder
  ❌ Two more tests to write (human: ~1 hr / CC: ~5 min)
B) Existing tests only
  ✅ No new pad tests beyond the lesson-page ones
  ✅ The existing suite already pins stamp placement and verdict words
  ❌ Nothing pins "earlier blanks stay unstamped" or "SEEN never moves the ladder"
Net: the contract is the same either way; A adds the two tests that enforce it.
Header: Pad regression
Options:
A) Contract plus two tests (recommended)
Practice and play pages keep today's behavior: last blank graded and stamped only; ladder moves only on YES/ALMOST/NO; Unknown draws nothing. All existing tests pass unedited. New tests: a practice sheet leaves earlier blanks unstamped; SEEN never moves the ladder.
B) Existing tests only
Same preserved behavior, enforced only by the existing suite; new tests cover lesson pages alone.

State: approved
Actual answer: A) Contract plus two tests (D16, 2026-10-23)
Accepted scope: practice and play pages keep today's behavior (last blank
graded and stamped only; ladder moves only on YES/ALMOST/NO; Unknown draws
nothing); all existing tests pass unedited; new tests: a practice sheet leaves
earlier blanks unstamped, and SEEN never moves the ladder.
History: none

### R13: Test runner for tutor/
Finding: T2, P2, confidence 9/10, plan line 222-224, reviewer plan-eng-review.
Plan baseline: server tests are listed (router fallback with fake CLIs,
schema validation, renderer region measurement; plan line 222-224); no runner
named.
Runtime evidence: Vellum, the sibling Node service on gauntlet, uses Node's
built-in runner: `~/Dev/vellum/package.json` `"test": "node --import tsx --test tests/*.test.*"`.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R13 tutor test runner | none | `node:test` with `tsx`, same command shape as Vellum, no new test dependency | Vitest, sharing config with the Vite web build |
| R1-R12 | approved | unchanged | unchanged |
| R14+ | pending | pending | pending |
Question D17:
D17 — Which test runner should the tutor service use?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; the plan lists server tests but names no runner, and Vellum already runs its tests with Node's built-in runner.
ELI10: The tutor needs a tool to run its tests. Vellum, which runs beside it on gauntlet, uses the test runner built into Node, started through tsx so TypeScript works. Vitest is the other common choice and pairs with Vite, which the web app uses. Following Vellum means one way to run tests across both services and no extra package.
Stakes if we pick wrong: a second test setup to remember, or missing a Vitest feature (browser-like DOM tests) the web app might want later.
Recommendation: A) node:test like Vellum because it is the pattern already in use beside this service and needs no new dependency.
Note: options differ in kind, not coverage — no completeness score.
Pros / cons:
A) node:test like Vellum (recommended)
  ✅ Same command shape as Vellum: node --import tsx --test tests/*.test.*
  ✅ No new test dependency to pin or update in tutor.nix
  ❌ No built-in DOM environment if the web app later wants component tests
B) Vitest
  ✅ Shares config with the Vite web build and has watch mode and DOM environments
  ✅ Familiar assertion and mocking API
  ❌ A second test setup across the two gauntlet services, plus one more pinned dependency
Net: A matches the service next door; B buys web-component testing nobody has asked for yet.
Header: Test runner
Options:
A) node:test like Vellum (recommended)
tutor/ tests run with node --import tsx --test tests/*.test.*, the same shape as Vellum, with no new test dependency.
B) Vitest
tutor/ tests run with Vitest, configured alongside the Vite web build.

State: approved
Actual answer: A) node:test like Vellum (D17, 2026-10-23)
Accepted scope: `tutor/` tests run with `node --import tsx --test
tests/*.test.*`, the same shape as Vellum, with no new test dependency.
History: none

### R14: Run tests in CI
Finding: T3, P2, confidence 9/10, plan line 220-230, reviewer plan-eng-review.
Plan baseline: the Testing section lists server, pad and end-to-end tests
(plan line 220-230); nothing says where they run.
Runtime evidence: the only workflow, `.github/workflows/no-vendor-blob.yml`,
runs `./scripts/ci/check-no-vendor-blob.sh` and no tests; recent runs take
8-11 s (`gh run list`, 2026-10-23). `cargo test` passes 321 locally in 63 s.
The user's merge rule is "Merge once mergeable and CI green".
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R14 CI tests | none | new `.github/workflows/test.yml` on push and PR: `cargo test` on ubuntu-latest (host build, no `rm2` feature) and `tutor/` `npm ci && npm test`, with Playwright's Chromium installed by the pinned npm version for the renderer tests | no CI tests; run `cargo test` and `npm test` locally before each PR |
| R1-R13 | approved | unchanged | unchanged |
| R15+ | pending | pending | pending |
Question D18:
D18 — Should CI run the pad and tutor tests on every push and PR?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; CI today only checks for a vendor library and runs no tests, and your merge rule waits on CI green.
ELI10: GitHub runs one check on this repo today, and it only looks for a file that must never be committed. "CI green" therefore says nothing about whether the 321 pad tests or the new tutor tests pass. Adding a test job makes green mean the code works, and catches a broken test when you merge your own PR without a local run.
Stakes if we pick wrong: a PR merges green with failing tests, which is how the per-box stamping or SEEN regressions from D16 would slip through.
Recommendation: A) Add a test job because your merge rule trusts CI, and today CI does not test anything.
Completeness: A=10/10, B=5/10
Pros / cons:
A) Add a test job (recommended)
  ✅ CI green now means 321 pad tests plus the tutor suite passed
  ✅ Enforces the D16 regression contract on every PR, not just when someone remembers
  ❌ Each PR waits a few minutes for cargo and Chromium setup (human: ~2 hr / CC: ~15 min)
B) Local runs only
  ✅ No CI minutes or workflow to maintain
  ✅ Fast PR checks stay at about 10 s
  ❌ CI green keeps meaning only "no vendor blob", so a failing test can merge
Net: A makes the existing merge rule mean what it says.
Header: CI tests
Options:
A) Add a test job (recommended)
New .github/workflows/test.yml on push and PR: cargo test on ubuntu-latest (host build, no rm2 feature) and tutor/ npm ci && npm test, with Chromium installed by the pinned npm Playwright for renderer tests.
B) Local runs only
No CI tests; run cargo test and npm test locally before each PR.

State: approved
Actual answer: A) Add a test job (D18, 2026-10-23)
Accepted scope: new `.github/workflows/test.yml` on push and PR: `cargo test`
on ubuntu-latest (host build, no `rm2` feature) and `tutor/` `npm ci && npm
test`, with Chromium installed by the pinned npm Playwright for renderer tests.
History: none

### R15: Eval set for the model tasks
Finding: T4, P2, confidence 8/10, plan line 127-136 and 150-153, reviewer plan-eng-review.
Plan baseline: verify checks keys, hints and reading level (plan line
132-136); grade uses a vision model against the key (plan line 150); the only
model-quality check is one real end-to-end lesson (plan line 228-230).
Runtime evidence: unit tests with fake CLIs (plan line 222-223) prove routing,
not answer quality. Memory note "Learn digit misreads fixed": 4 of 6 earlier
rejections were crop problems found only by looking at real ink. No stored
handwriting fixtures exist in the repo (search 2026-10-23).
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R15 model-quality eval | one E2E run | `tutor/evals/`: ~20 ink PNGs captured with `--lesson-test` from real handwriting, each with its key and expected verdict, for `grade`; ~10 seeded lessons with planted faults (wrong key, hint that states the answer, passage above the reading level) for `verify`; `npm run eval` reports pass rate per route; run by hand before changing a prompt, schema or route, not in CI | no eval in phase 1; rely on the end-to-end run and parent review in the web app |
| R1-R14 | approved | unchanged | unchanged |
| R16+ | pending | pending | pending |
Question D19:
D19 — Should phase 1 include a small eval set that checks grading and verification quality?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; the plan tests routing with fake CLIs and runs one real lesson, but nothing measures whether grades and verify checks are right.
ELI10: The unit tests prove the plumbing: calls go to the right model and bad output falls through. They cannot tell you whether the grader reads a child's "7" as a 7, or whether verify catches a hint that gives away the answer. A small eval set is a folder of real handwriting and deliberately broken lessons with known right answers. You run it before changing a prompt or switching models and get a pass rate.
Stakes if we pick wrong: a prompt or model change starts marking right answers wrong, and you learn it from a child.
Recommendation: A) Small eval set because grading and verify are the parts a child and parent trust, and 30 cases are cheap to run by hand.
Completeness: A=10/10, B=5/10
Pros / cons:
A) Small eval set (recommended)
  ✅ A pass rate per route before any prompt, schema or model change
  ✅ Real ink fixtures catch crop and misread problems like the earlier digit misreads
  ❌ Capturing 20 handwriting samples takes a session on the pad, and each run spends model calls (human: ~1 day / CC: ~45 min plus capture)
B) No eval in phase 1
  ✅ Nothing to capture or maintain until the loop works end to end
  ✅ No model spend outside real use
  ❌ Grading quality is judged only by the one end-to-end run and by spotting bad stamps
Net: thirty fixtures turn "seems right" into a number you can compare across models.
Header: Model evals
Options:
A) Small eval set (recommended)
tutor/evals/ with ~20 real ink PNGs (key + expected verdict) for grade and ~10 seeded faulty lessons for verify; npm run eval prints a pass rate per route; run by hand before prompt, schema or route changes, not in CI.
B) No eval in phase 1
Rely on the end-to-end run and parent review in the web app.

State: approved
Actual answer: A) Small eval set (D19, 2026-10-23)
Accepted scope: `tutor/evals/` with ~20 real ink PNGs (key and expected
verdict) for `grade` and ~10 seeded faulty lessons for `verify`; `npm run
eval` prints a pass rate per route; run by hand before prompt, schema or route
changes, not in CI.
History: none

Test Plan Artifact: `~/.gstack/projects/m4ndolore-g-pad/paulgarcia-worktree-tutor-eng-review-eng-review-test-plan-20261023-114338.md`.

## Section 4: Performance

### R16: Grading a page's boxes one after another or together
Finding: P1, P2, confidence 7/10, plan line 89 and 181, reviewer plan-eng-review.
Plan baseline: `grade` → cloud fast vision model, 1-4 s (plan line 79, 89);
per-box posts approved in R1 (D5 A).
Runtime evidence: the pad's HTTP client is blocking `ureq` (`Cargo.toml:15`)
and today's oracle call runs on a worker thread that reports over an mpsc
channel (`src/oracle.rs:20-22`, `main.rs:2274` `ask_with_model(..., tx, ...)`).
Three boxes posted in sequence wait 3-12 s [estimated from the plan's 1-4 s;
not measured]; a cloud failure that falls back to `cli` adds 5-30 s per box.
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R16 per-page grade concurrency | unspecified | one worker thread per handwriting box (at most 3), posted together; each stamp draws as its reply arrives; the page result waits for all | one worker posts boxes in order, stamping each as it returns |
| R1 per-box posts | approved D5 A | unchanged | unchanged |
| R17+ | pending | pending | pending |
Question D20:
D20 — When a lesson page has three answer boxes, should the pad send them to the grader together or one after another?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; approved D5 posts every box separately, and each cloud grade takes 1-4 s by the plan's estimate.
ELI10: After the child presses DONE, each answer box goes to the grader. Sent one at a time, three boxes take three times as long, about 3 to 12 seconds, longer if the cloud model fails and a CLI takes over. Sent together, the wait is about as long as the slowest single box. The pad already grades on a background thread, so three threads is the same pattern three times.
Stakes if we pick wrong: a young child stares at a page for ten seconds after DONE, or the pad juggles three replies arriving in any order.
Recommendation: A) Send together because a child is waiting, and the replies already arrive on a channel the sheet drains.
Completeness: A=10/10, B=7/10
Pros / cons:
A) Send together (recommended)
  ✅ Wait after DONE is about one grade, not three
  ✅ Reuses the existing thread-plus-channel pattern from oracle.rs
  ❌ Stamps can appear out of order, and the cloud sees three requests at once
B) One after another
  ✅ Stamps appear top to bottom in order
  ✅ One request in flight at a time keeps the pad and the cloud quiet
  ❌ Waits add up: about 3-12 s for three boxes, much longer on CLI fallback
Net: A cuts the child's wait to one grade; B keeps stamps in order.
Header: Grade order
Options:
A) Send together (recommended)
One worker thread per handwriting box (at most 3) posts at once; each stamp draws as its reply arrives; the page result waits for every box.
B) One after another
A single worker posts the boxes in order and stamps each as it returns.

State: approved
Actual answer: A) Send together (D20, 2026-10-23)
Accepted scope: one worker thread per handwriting box (at most 3) posts at
once; each stamp draws as its reply arrives; the page result waits for every
box.
History: none

### R17: Renderer browser lifetime
Finding: P2, P3, confidence 6/10 (medium confidence, verify the launch cost on gauntlet), plan line 137-138 and 159, reviewer plan-eng-review.
Plan baseline: Playwright renders each page; inline edits re-render (plan line
137-138, 159). Startup self-test approved in R2 (D6 A).
Runtime evidence: no renderer exists. A headless Chromium launch commonly
costs on the order of a second and a few hundred MB [estimated, not measured
on gauntlet]; a four-page lesson plus edits means many renders. Gauntlet has
16 cores, 30 GB RAM, 23 GB available (probe 2026-10-23).
Comparison grid:
| Choice | Current | A | B |
|---|---|---|---|
| R17 browser lifetime | unspecified | one Chromium launched at service start (the R2 self-test instance), a fresh browser context per render, at most 2 renders at once, relaunch on disconnect with the self-test rerun | launch and close a browser for every render |
| R2 self-test | approved D6 A | unchanged | unchanged |
| R18+ | pending | pending | pending |
Question D21:
D21 — Should the renderer keep one browser running or start a new one for every page?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; every lesson page and every inline edit is rendered by headless Chromium, and the plan does not say how long a browser lives.
ELI10: Turning a lesson page into a picture needs a browser. Starting one takes time and memory each time. Keeping one running and opening a clean tab per page makes edits re-render quickly, but the service then has to notice if that browser crashes and start it again. Starting fresh each time is simpler and leaks nothing between pages, at the cost of startup time on every render.
Stakes if we pick wrong: inline edits in the preview feel slow, or a crashed browser leaves every render failing until restart.
Recommendation: A) One browser, fresh context because edits re-render often, and the self-test from D6 already gives a relaunch check.
Completeness: A=10/10, B=8/10
Pros / cons:
A) One browser, fresh context (recommended)
  ✅ Renders skip browser startup, so preview edits come back fast
  ✅ A fresh context per render keeps pages isolated; relaunch reruns the self-test
  ❌ Needs crash detection and relaunch logic, plus a cap of two renders at once
B) Launch per render
  ✅ Nothing to keep alive, nothing to relaunch
  ✅ Each render starts from a clean browser
  ❌ Every render, including each inline edit, pays browser startup time and memory
Net: A trades a relaunch path for fast edits; B trades speed for simplicity.
Header: Browser lifetime
Options:
A) One browser, fresh context (recommended)
Launch one Chromium at service start (the self-test instance), open a fresh browser context per render, at most 2 renders at once, relaunch and rerun the self-test on disconnect.
B) Launch per render
Launch and close a browser for every render.

State: approved
Actual answer: A) One browser, fresh context (D21, 2026-10-23)
Accepted scope: one Chromium launched at service start (the self-test
instance), a fresh browser context per render, at most 2 renders at once,
relaunch and rerun the self-test on disconnect.
History: none

## Outside voice (Codex, gpt-6-astra, completed 2026-10-23)

Six findings, all checked against the code by the parent reviewer:
X1 edits bypass verify (plan line 137); X2 per-box retry has no contract
(`src/main.rs:2643-2672`: YES runs `user_ink.clear()`, ALMOST/NO clear and
`session.draw` the whole sheet); X3 no completion sync (`main.rs:2139` NEW and
`main.rs:2307` auto-advance both call `session.next()`; tick-only pages post
nothing); X4 feedback band (`src/learn/sheet.rs:23` `feedback_y()` at 70% of
height, `main.rs:4043` `clear_feedback` whites it out); X5 tick boxes capped at
three (`src/learn/mod.rs:607` `if i >= 3 { return None; }`, one
`choice_answered` flag per page); X6 verifier can equal the generator on
fallback (plan line 87-88).

- C5 (correction from X4, refines R9's accepted scope, no behavior change):
  the "body rectangle" in R9 ends above the feedback strip, at
  `feedback_y() - 10` (`sheet.rs:23-30`). Templates leave both the feedback
  strip and the footer blank; the renderer test and the pad's refusal check
  use that bound; the web preview overlays both.

### R18: Edits after verification
Finding: X1, P1, confidence 9/10, plan line 137-138, reviewer Codex (confirmed by plan-eng-review).
Plan baseline: "Inline text edits re-render the page; larger changes go
through chat." (plan line 137-138); verify runs once before preview.
Runtime evidence: no code yet. Under the plan, editing a question's text
after verify leaves its key and hints unchecked, and a pad holding the old
page keeps grading against whatever key the server now holds; attempt IDs
(R8) carry no revision.
Comparison grid:
| Choice | Current | A | B | C | D |
|---|---|---|---|---|---|
| R18 edit after verify | re-render only | any edit re-runs verify for that page before the lesson can be (re)sent; once assigned, a lesson revision is immutable and an edit creates a new revision; pulls, keys and `attempt_id` carry the revision, and the grade route uses the key of the revision the pad holds | keep re-render only | investigate: no change approved; bounded to listing which edits change keys | defer this proposal; plan unchanged |
| R1-R17 | approved | unchanged | unchanged | unchanged | unchanged |
Question D22:
D22 — What should happen when the parent edits a lesson page after it has been verified?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; Codex found that plan line 137 lets an inline edit re-render a page without re-checking its answer key, and grades are not tied to a lesson version.
ELI10: Verify is the step where a second model makes sure every answer key and hint is right. The plan lets the parent fix a word on a page after that, and only re-draws the picture. If the edit changes the question, the key may now be wrong, and nobody checks. Worse, a pad that already downloaded the old page keeps sending answers that the server grades against the new key. Tying each lesson to a version and re-checking on edit closes both holes.
Stakes if we pick wrong: a child is told NO for a right answer because the key no longer matches the question on their screen.
Recommendation: A) Re-verify and version because a wrong key is the worst thing this service can show a child, and the version field is small.
Completeness: A=10/10, B=4/10, C=n/a, D=n/a
Pros / cons:
A) Re-verify and version (recommended)
  ✅ No page reaches the pad with a key that was not checked against its current text
  ✅ The pad always grades against the key of the version it shows, even offline
  ❌ Each edit waits for a verify run, and the schema gains a revision on lesson, page and attempt (human: ~1 day / CC: ~40 min)
B) Keep re-render only
  ✅ Edits show instantly in preview
  ✅ No revision bookkeeping
  ❌ Edited pages can carry unchecked keys, and cached pages are graded against the wrong one
C) Investigate first
  ✅ Bounds the work to listing which edits change keys before deciding
  ✅ Leaves all approved scope unchanged
  ❌ Leaves the hole open until a second decision
D) Defer this change
  ✅ Phase 1 scope stays as approved
  ✅ Can be added before any other parent uses the service
  ❌ Ships with the stale-key path
Net: A closes the one path to a wrong verdict the review found after verify.
Header: Edit safety
Options:
A) Re-verify and version (recommended)
Any edit re-runs verify for that page before the lesson can be sent or resent. An assigned lesson revision is immutable; an edit makes a new revision. Pulls, keys and attempt_id carry the revision, and grading uses the key of the revision the pad holds.
B) Keep re-render only
Edits re-render the page as the plan says; no re-verify, no revision.
C) Investigate first
No change approved now; list which edits change keys or hints, then decide.
D) Defer this change
Leave this proposal unresolved; plan unchanged for phase 1.

State: approved
Actual answer: A) Re-verify and version (D22, 2026-10-23)
Accepted scope: any edit re-runs verify for that page before the lesson can
be sent or resent; an assigned lesson revision is immutable and an edit makes
a new revision; pulls, keys and `attempt_id` carry the revision; grading uses
the key of the revision the pad holds.
History: none

### R19: Mixed results on one page
Finding: X2, P1, confidence 9/10, R1 and R16 accepted scope, reviewer Codex (confirmed by plan-eng-review).
Plan baseline: every box graded and stamped (D5 A), posted together with
stamps in arrival order (D20 A). Retry behavior not stated.
Runtime evidence: `src/main.rs:2643-2647` on YES "the tracked ink is dropped
so a second DONE cannot resubmit it" (`user_ink.clear()`); `main.rs:2665-2672`
on ALMOST/NO "A clean retry: repaint the sheet" (`user_ink.clear();
session.draw(...)`). Applied per box, the first NO would wipe every other
box's ink and stamps.
Comparison grid:
| Choice | Current | A | B | C | D |
|---|---|---|---|---|---|
| R19 per-box retry | unspecified; today's handlers clear the whole page | lesson pages keep per-box state: a YES box keeps its ink and check and is locked; an ALMOST/NO box is whited out alone, stamped, and reopened; an Unknown/timeout box keeps its ink unmarked; the next DONE resends only reopened or Unknown boxes; the page counts done when every box is YES; practice handlers unchanged (D16) | keep today's whole-page handlers on lesson pages | investigate: no change approved; bounded to a sketch of per-box state in `assigned.rs` | defer this proposal; plan unchanged |
| R1, R12, R16 | approved | unchanged | unchanged | unchanged | unchanged |
Question D23:
D23 — When some boxes on a lesson page come back right and others wrong, what should the pad keep and what should it reopen?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; Codex found that today's pad wipes and redraws the whole page on a wrong answer, which would erase the other boxes once each box is graded on its own.
ELI10: Today a math page has one graded box. A wrong answer wipes the page so the child writes fresh. With three graded boxes, wiping the page on the first NO would erase two answers that were right. The pad needs a rule per box: right answers stay with their check mark, a wrong one is cleared alone for another try, and an answer the grader could not read stays as written so the child can send it again. Only the unsettled boxes go back on the next DONE.
Stakes if we pick wrong: a child's correct answers vanish because a different question was wrong, and they have to rewrite the page.
Recommendation: A) Per-box retry because erasing right answers punishes the child for a different question, and math pages keep their handlers per D16.
Completeness: A=10/10, B=3/10, C=n/a, D=n/a
Pros / cons:
A) Per-box retry (recommended)
  ✅ Right answers stay on the page with their checks; only the wrong box reopens
  ✅ A resend carries only unsettled boxes, so no extra grading calls or rows
  ❌ New per-box state and partial repaint on lesson pages (human: ~1 day / CC: ~40 min), plus tests
B) Whole-page handlers
  ✅ Reuses today's code with no new state
  ✅ Behavior matches math pages exactly
  ❌ One wrong box erases every other answer on the page, including right ones
C) Investigate first
  ✅ Bounds the work to a sketch of per-box state before deciding
  ✅ Leaves approved scope unchanged
  ❌ D5 and D20 stay unimplementable until a second decision
D) Defer this change
  ✅ Phase 1 scope stays as approved
  ✅ Can be revisited with the first real lesson
  ❌ D5 and D20 have no defined retry behavior
Net: grading every box only works if one wrong box cannot erase the others.
Header: Per-box retry
Options:
A) Per-box retry (recommended)
Lesson pages keep per-box state: YES keeps ink and check and locks the box; ALMOST/NO whites out that box alone, stamps it and reopens it; Unknown or timeout keeps the ink unmarked. The next DONE resends only reopened or Unknown boxes. The page is done when every box is YES. Practice handlers unchanged.
B) Whole-page handlers
Lesson pages use today's handlers: any ALMOST/NO clears and redraws the whole page.
C) Investigate first
No change approved now; sketch per-box state in assigned.rs, then decide.
D) Defer this change
Leave this proposal unresolved; plan unchanged for phase 1.

State: approved
Actual answer: A) Per-box retry (D23, 2026-10-23)
Accepted scope: lesson pages keep per-box state: YES keeps ink and check and
locks the box; ALMOST/NO whites out that box alone, stamps it and reopens it;
Unknown or timeout keeps the ink unmarked; the next DONE resends only
reopened or Unknown boxes; the page is done when every box is YES; practice
handlers unchanged.
History: none

### R20: Page progress and lesson completion
Finding: X3, P1, confidence 8/10, plan line 168-174, reviewer Codex (confirmed by plan-eng-review).
Plan baseline: "A finished lesson moves to `lessons/done/` and is not dealt
again" (plan line 173-174); status `assigned → done` on the server (plan line
119); only pull and grade routes exist.
Runtime evidence: tick boxes are graded on the pad (C2) and post nothing;
`src/main.rs:2139` NEW and `main.rs:2307` auto-advance both call
`session.next()`, so the pad does not tell skip from finish; nothing records
which page a lesson is on across a restart.
Comparison grid:
| Choice | Current | A | B | C | D |
|---|---|---|---|---|---|
| R20 progress sync | none | pad keeps per-lesson progress in `$RIDDLE_DATA_DIR/lessons/<id>/progress` and resumes at the first unfinished page after restart; on leaving a lesson page it sends `POST /device/v1/progress {lesson, rev, page, player, outcome: done or skipped, tick results}` through the outbox; tick results become attempt rows; the server marks the lesson done when every page is done or skipped; NEW on a lesson page records skipped | none: the server infers completion from grade posts only | investigate: no change approved; bounded to listing which outcomes the server must know | defer this proposal; plan unchanged |
| R8, R9, R18, R19 | approved | unchanged | unchanged | unchanged | unchanged |
Question D24:
D24 — How should the server learn that a lesson page was finished or skipped?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; Codex found that tick-box pages send nothing to the server and the pad treats NEW and finishing the same way.
ELI10: The server only hears from the pad when a handwritten box is graded. A page of tick boxes is marked on the pad and never reported, so the server cannot know the child did it. NEW (skip) and finishing both just move on, so the pad itself cannot tell which happened. And if the pad restarts mid-lesson, nothing remembers which page the child was on. A small progress report per page, saved on the pad too, fixes all three.
Stakes if we pick wrong: lessons sit as "assigned" forever in the web app, tick answers never reach phase 2 history, and a restart starts the lesson over.
Recommendation: A) Progress report per page because it is the only way tick-box work and skips reach the server, and it reuses the outbox.
Completeness: A=10/10, B=4/10, C=n/a, D=n/a
Pros / cons:
A) Progress report per page (recommended)
  ✅ The web app shows done, skipped and in-progress lessons accurately
  ✅ Tick answers become attempt rows, and a restart resumes on the right page
  ❌ One more device route, a progress file per lesson, and outbox entries for it (human: ~1 day / CC: ~40 min)
B) Infer from grades
  ✅ No new route or file
  ✅ Handwritten-only lessons still complete
  ❌ Tick-only pages and skips are invisible, and a restart loses the child's place
C) Investigate first
  ✅ Bounds the work to listing what the server must know
  ✅ Leaves approved scope unchanged
  ❌ Lesson completion stays undefined until a second decision
D) Defer this change
  ✅ Phase 1 scope stays as approved
  ✅ Phase 2 progress work could own it
  ❌ The web app's lesson status cannot reach done in phase 1
Net: A gives the lesson lifecycle an end the server can see.
Header: Page progress
Options:
A) Progress report per page (recommended)
Pad saves per-lesson progress in $RIDDLE_DATA_DIR/lessons/<id>/progress and resumes there after restart. Leaving a lesson page sends POST /device/v1/progress {lesson, rev, page, player, outcome done or skipped, tick results} through the outbox; tick results become attempt rows; NEW records skipped; the server marks the lesson done when every page is done or skipped.
B) Infer from grades
No progress route; the server infers completion from grade posts only.
C) Investigate first
No change approved now; list which outcomes the server must know, then decide.
D) Defer this change
Leave this proposal unresolved; plan unchanged for phase 1.

State: approved
Actual answer: A) Progress report per page (D24, 2026-10-23)
Accepted scope: the pad saves per-lesson progress in
`$RIDDLE_DATA_DIR/lessons/<id>/progress` and resumes there after restart;
leaving a lesson page sends `POST /device/v1/progress {lesson, rev, page,
player, outcome done or skipped, tick results}` through the outbox; tick
results become attempt rows; NEW records skipped; the server marks the lesson
done when every page is done or skipped.
History: none

### R21: Tick-box questions on lesson pages
Finding: X5, P2, confidence 9/10, plan line 151-152, reviewer Codex (confirmed by plan-eng-review).
Plan baseline: Pick one has "2–4 choices" in tick boxes; Word work mixes
boxes and ticks (plan line 151-152). C2 said only loading the key is new.
Runtime evidence: `src/learn/mod.rs:601-608` `answer_choice` returns `None`
for `i >= 3`, and one `choice_answered` flag covers the page;
`sheet.rs:254` "Three equal tick boxes in a row". So the pad supports one
three-box tick question per page.
Comparison grid:
| Choice | Current | A | B | C | D |
|---|---|---|---|---|---|
| R21 lesson tick questions | pad: one question, exactly 3 boxes | lesson pages carry tick groups from `regions.json`, each with 2-4 boxes, its own key and its own answered state, several per page and mixed with handwriting boxes; practice tick pages keep `answer_choice` unchanged (D16) | phase 1 lesson templates use at most one tick question per page with exactly 3 choices, reusing `answer_choice`; Pick one and Word work are written to that limit | investigate: no change approved; bounded to sizing the group change | defer this proposal; plan unchanged |
| R9, R16, R19, R20 | approved | unchanged | unchanged | unchanged | unchanged |
Question D25:
D25 — Should lesson pages support the plan's 2-4 tick boxes and several tick questions per page, or keep today's one question with three boxes?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; Codex found that the pad accepts exactly three tick boxes and one tick question per page, while the plan's Pick one template promises 2-4 choices.
ELI10: Today's tick pages always show three boxes and mark one question. The lesson templates promise 2 to 4 choices, and Word work can mix ticks with written answers on one page. Supporting that means the pad keeps a small group per tick question: how many boxes, which is right, and whether it is answered. The other choice is to write the lesson templates to today's limit.
Stakes if we pick wrong: lesson pages with four choices or two tick questions silently ignore taps, or the templates are narrower than the design promises.
Recommendation: A) Tick groups because Word work needs mixed pages, and math pages keep their code per D16.
Completeness: A=10/10, B=6/10, C=n/a, D=n/a
Pros / cons:
A) Tick groups (recommended)
  ✅ Pick one gets 2-4 choices and Word work can mix ticks with written boxes
  ✅ Practice tick pages keep answer_choice exactly as today
  ❌ New group state on lesson pages, with its own tests (human: ~1 day / CC: ~30 min)
B) Templates to today's limit
  ✅ Reuses answer_choice with no pad change
  ✅ Nothing new to test on the pad for ticks
  ❌ Pick one always shows three choices, and Word work cannot mix ticks with written answers
C) Investigate first
  ✅ Bounds the work to sizing the group change
  ✅ Leaves approved scope unchanged
  ❌ Templates stay undefined until a second decision
D) Defer this change
  ✅ Phase 1 scope stays as approved
  ✅ Can follow the first lessons
  ❌ The plan's 2-4 choice promise stays unimplemented and undocumented
Net: A delivers the templates as designed; B narrows them to the code that exists.
Header: Tick groups
Options:
A) Tick groups (recommended)
Lesson pages read tick groups from regions.json: 2-4 boxes each, own key, own answered state, several per page, mixed with handwriting boxes. Practice tick pages keep answer_choice unchanged.
B) Templates to today's limit
Phase 1 lesson templates use at most one tick question per page with exactly 3 choices, reusing answer_choice.
C) Investigate first
No change approved now; size the group change, then decide.
D) Defer this change
Leave this proposal unresolved; plan unchanged for phase 1.

State: approved
Actual answer: A) Tick groups (D25, 2026-10-23)
Accepted scope: lesson pages read tick groups from `regions.json`: 2-4 boxes
each, own key, own answered state, several per page, mixed with handwriting
boxes; practice tick pages keep `answer_choice` unchanged.
History: none


### R22: Verifier must differ from the generator
Finding: X6, P2, confidence 9/10, plan line 87-88 and 132, reviewer Codex (confirmed by plan-eng-review).
Plan baseline: `generate` → `cli:claude` → `cli:codex`; `verify` →
`cli:codex` → `cloud`; "A different model checks the first" (plan line 87-88).
Runtime evidence: two self-check paths. (1) When `cli:claude` fails,
generation lands on `cli:codex` and verification starts on `cli:codex`: the
same model. (2) When `cli:codex` fails during verify, the `cloud` route runs;
Vellum's cloud default is an Anthropic model,
`~/Dev/vellum/server/ai.mjs:301` `process.env.VELLUM_MODEL || "anthropic/claude-haiku-4.5"`
(AI Gateway) and `ai.mjs:358` `"claude-opus-4-8"` (direct), so a Claude model
would check a Claude-written lesson. A rule that compares exact model ids
misses path 2. Investigation per D26 C, 2026-10-23.
Comparison grid:
| Choice | Current | A | B | C |
|---|---|---|---|---|
| R22 verifier independence | not enforced | every route in `routes.toml` declares a model family (anthropic, openai, google, xai…); each lesson stores the family that generated it; verify skips every route of that family; the default `cloud` verify route names a non-Anthropic model; with no other family left, the lesson goes to "needs your eyes" with "no independent verifier"; router tests cover both paths | each lesson stores the exact model id; verify skips only that id; same-family checks (Claude CLI then Claude Haiku) allowed | keep chains as written; same-model verification allowed on fallback |
| R6, R7, R10 router decisions | approved | unchanged | unchanged | unchanged |
Question D27:
D27 — With the investigation done, how strictly should the router keep the verifier independent of the model that wrote the lesson?
Project/branch/task: tutor eng review on worktree-tutor-eng-review; D26 asked to investigate, and the check found two self-verify paths: codex checking codex, and a Claude cloud model checking a Claude-written lesson.
ELI10: There are two ways the second opinion can come from the same place. If claude is down, codex writes and codex checks. If codex is down during the check, the cloud backup is a Claude model, because that is what Vellum's cloud route uses, so Claude checks Claude. Tracking only the exact model name catches the first case but not the second. Tracking the model family (Anthropic, OpenAI and so on) catches both, as long as the cloud backup for verify is a different family.
Stakes if we pick wrong: on a day one CLI is down, answer keys are checked by the same family that wrote them, and family-wide blind spots pass.
Recommendation: A) Different family because path 2 is a real default, and family tags cost one field per route.
Completeness: A=10/10, B=7/10, C=4/10
Pros / cons:
A) Different family (recommended)
  ✅ Catches both paths: codex checking codex and Claude Haiku checking Claude
  ✅ Picking a non-Anthropic cloud verify model keeps verification available when codex is down
  ❌ Each route needs a family tag, and the cloud verify model must be chosen outside Anthropic (human: ~3 hr / CC: ~20 min)
B) Different exact model
  ✅ Simple: compare one model id per lesson
  ✅ Catches the codex-checks-codex path
  ❌ Claude Haiku can still verify a Claude-written lesson
C) Keep chains as written
  ✅ No provenance or tags
  ✅ Lessons always finish verification when any model is up
  ❌ Both self-check paths stay open
Net: only the family rule keeps the plan's "different model" promise on both fallback paths.
Header: Verifier rule
Options:
A) Different family (recommended)
Every route in routes.toml declares a model family; each lesson stores its generator's family; verify skips every route of that family; the default cloud verify route is a non-Anthropic model; with no other family left, the lesson goes to needs your eyes with "no independent verifier". Router tests cover both paths.
B) Different exact model
Each lesson stores the exact model id; verify skips only that id. Same-family checks are allowed.
C) Keep chains as written
Verify chains run as configured; same-model verification is allowed on fallback.

State: approved
Actual answer: A) Different family (D27, 2026-10-23)
Accepted scope: every route in `routes.toml` declares a model family; each
lesson stores its generator's family; verify skips every route of that family;
the default cloud verify route is a non-Anthropic model; with no other family
left, the lesson goes to "needs your eyes" with "no independent verifier";
router tests cover both paths.
History: D26 (2026-10-23) offered A) skip same provider and model id, B) keep
chains, C) investigate, D) defer; actual answer C) Investigate first, bounded
to listing cloud model ids that could collide. The investigation found the
same-family path through Vellum's Anthropic cloud default, so the options were
rebuilt as D27.

## TODO dispositions

- D28 `local` backend: A) Add to TODOS.md (P3).
- D29 routes editor: A) Add to TODOS.md (P3, depends on D27 family tags).
- D30 grok and hermes adapters: A) Add to TODOS.md (P4, D10 lockdown precondition).

Approval readiness: PASS. Checked R1 (D5), R2 (D6), R3 (D7), R4 (D8),
R5 (D9), R6 (D10), R7 (D11), R8 (D12), R9 (D13), R10 (D14), R11 (D15),
R12 (D16), R13 (D17), R14 (D18), R15 (D19), R16 (D20), R17 (D21), R18 (D22),
R19 (D23), R20 (D24), R21 (D25), R22 (D27 after D26 C); scope D1-D4; TODOs
D28-D30. Corrections C1-C5 change no behavior. No pending choice.

## NOT in scope (phase 1)

- `local` backend: no task routes to it (D1; TODOS.md).
- Routes editor: read-only usage view instead (D3; TODOS.md).
- grok and hermes adapters: lockdown unverified (D11; TODOS.md).
- Per-device tokens: one pad; shared token like Vellum (D8).
- Progress views, daily notes, verdict overrides (plan phase 2).
- Parent-set plans and spaced review (plan phase 3); adult learners (phase 4).

## What already exists

| Need | Existing piece | Use |
|---|---|---|
| Device routes before Access, bearer compare | `~/Dev/vellum/server/ai.mjs:111-113`, `security.mjs:58-66` | copy the pattern (D8) |
| Nix service and tunnel ingress | gauntlet `~/Dev/dotfiles/vellum.nix:47`, `flake.nix:339` | add `tutor.nix` and one ingress entry (C1) |
| Verdict protocol | `src/learn/verdict.rs:54` `parse` | reuse; add `SEEN` (D15) |
| Ink crop | `src/ink.rs:351` `ink_png` | reuse per box (D5) |
| Footer targets | `src/learn/sheet.rs:108` `draw_footer` | reuse; add HINT (D13) |
| Tick boxes | `sheet.rs:180`, `mod.rs:601` `answer_choice` | practice unchanged; lesson tick groups new (D25) |
| Poll thread | `src/vault.rs:215-235` `spawn_poll` | pattern for the lessons pull (Q3) |
| Gray bitmap blit | `src/gesture.rs:76` `blit_gray` | full-screen lesson PNG |
| Data dir | `src/oracle.rs:24`, `RIDDLE_DATA_DIR` | lessons and outbox (D9) |
| Player roster | `src/learn/players.rs:19-24` | learner alias (D7) |
| SYSTEM status line | `src/system/device.rs:162-164` `hub_line` | TUTOR line |
| Node test runner | `~/Dev/vellum/package.json` `node --import tsx --test` | same shape (D17) |
| Single-shot test harness | `src/main.rs:727` `learn_test` | model for `--lesson-test` |

## Flow after the review

```
parent /app ──chat──► router[chat slot] ──Create──► generate(family F)
      ▲                                               │
      │ preview (+ footer and feedback overlay)       ▼
      │◄──────── render (1 Chromium, ctx/render) ◄── verify(family ≠ F) ─fail x2─► needs your eyes
      │ edit ──► re-verify page ──► new revision when assigned
      ▼
 Send to pad ──► lesson rev N for player alias (FOX)
                         │ GET /device/v1/lessons?since
                         ▼
 rM2: $RIDDLE_DATA_DIR/lessons/<id>/  (listed when complete; region bounds checked)
   page = blit PNG + draw_footer + HINT
   DONE ──► per box, concurrent: POST /device/v1/grade {rev, attempt_id}
              YES keep+lock │ ALMOST/NO clear box, reopen │ Unknown keep ink
   tick groups graded on pad
   leave page ──► POST /device/v1/progress {done|skipped, ticks} (outbox when offline)
```

## Failure modes

| Path | Realistic failure | Handling and test | Child or parent sees |
|---|---|---|---|
| Renderer | nixpkgs bump drifts from npm Playwright | startup self-test exits non-zero (D6); CI renderer tests | deploy fails, not Create |
| Router | CLI hangs | per-task timeout, next route; fake-CLI tests | slower reply, logged route |
| Verify | only same-family routes up | needs your eyes with reason (D27); router test | parent reviews the lesson |
| Device grade | pad times out after server graded | attempt_id upsert returns stored verdict (D12); test | one verdict, one row |
| Pad grade | one box NO, others YES | per-box retry (D23); pad unit test | right answers stay |
| Pad pull | partial download | listed only when complete; test | lesson appears next pull |
| Pad offline | DONE with no network | outbox by attempt_id, drains on pull; test | "SAVED! WE'LL CHECK IT SOON" |
| Edit | parent edits a verified page | re-verify, new revision (D22); test | stale key never used |
| Progress | tick-only page | progress route (D24); test | lesson reaches done |
| Auth | token rotated on one side | 401 → local deck, SYSTEM line; test | "TUTOR: UNAUTHORIZED" |

Critical gaps (no test, no handling, silent): none after the approved remedies.

## Worktree parallelization

| Step | Modules touched | Depends on |
|---|---|---|
| Contract: device JSON, regions.json schema, verdict words | `docs/`, `tutor/device`, `src/learn` | — |
| Service core: router, lessons, renderer, db, app | `tutor/` | contract |
| Pad lesson deck: assigned, sheet, verdict, main hooks | `src/learn/`, `src/main.rs`, `src/system/` | contract |
| Deploy: tutor.nix, ingress, Access app | gauntlet `~/Dev/dotfiles` | service core |
| CI job | `.github/workflows/` | service core, pad deck |
| Evals and end-to-end | `tutor/evals/`, hardware | all above |

Lane A: contract → service core → deploy (`tutor/`, dotfiles).
Lane B: pad lesson deck (`src/`), after the contract.
Execution order: write the contract first. Launch A and B. Merge both. Then
CI, evals and the hardware run. Conflict flag: `src/main.rs` is shared with
any other pad work in flight; keep pad changes in one lane.

## Implementation Tasks
Synthesized from this review's findings. Each task derives from a specific
finding above. Run with Claude Code or Codex; checkbox as you ship.
Effort ratios assumed: features ~30x, tests ~50x, deploy and architecture ~5x.

- [ ] **T1 (P1, human: ~2h / CC: ~20min)** — contract — Write the device and regions contract
  - Surfaced by: R3, R8, R9, R18, R20, R21, C5
  - Files: `docs/plans/` (contract section), `tutor/device/`, `src/learn/assigned.rs`
  - Verify: both sides parse the same fixture JSON in tests
- [ ] **T2 (P1, human: ~2d / CC: ~1h)** — tutor/router — claude and codex adapters with pinned argv, chat slot, family tags
  - Surfaced by: R6, R7, R10, R22
  - Files: `tutor/router/`, `tutor/tests/`
  - Verify: `npm test` argv, fallback, slot and family tests
- [ ] **T3 (P1, human: ~3d / CC: ~1.5h)** — tutor/lessons — pipeline, templates with blank footer and feedback bands, renderer with self-test and one browser, pictures
  - Surfaced by: R2, R9, R17, R18, C5, D2
  - Files: `tutor/lessons/`, `tutor/tests/`
  - Verify: region-measurement tests; self-test fails without Chromium
- [ ] **T4 (P1, human: ~2d / CC: ~1h)** — tutor/device — token, pull by alias and revision, grade upsert, progress route
  - Surfaced by: R3, R4, R8, R18, R20
  - Files: `tutor/device/`, `tutor/db`
  - Verify: `npm test` route tests
- [ ] **T5 (P1, human: ~3d / CC: ~1.5h)** — pad — Page::Lesson, pull thread, data-dir lessons and outbox, regions loader with bounds check, footer plus HINT, SYSTEM line
  - Surfaced by: R5, R9, Q1, Q3, C2
  - Files: `src/learn/assigned.rs`, `src/learn/mod.rs`, `src/learn/sheet.rs`, `src/main.rs`, `src/system/device.rs`
  - Verify: `cargo test`; `g-pad --lesson-test <dir>`
- [ ] **T6 (P1, human: ~2d / CC: ~1h)** — pad — concurrent per-box grading with per-box retry state; progress posts
  - Surfaced by: R1, R16, R19, R20
  - Files: `src/learn/assigned.rs`, `src/main.rs`
  - Verify: unit tests for mixed YES/NO/Unknown pages
- [ ] **T7 (P1, human: ~1h / CC: ~5min)** — pad — regression pins: practice earlier blanks unstamped; SEEN never moves the ladder; 321 tests unedited
  - Surfaced by: R12
  - Files: `src/learn/sheet.rs`, `src/learn/mod.rs`
  - Verify: `cargo test`
- [ ] **T8 (P2, human: ~1d / CC: ~30min)** — pad — tick groups with 2-4 boxes on lesson pages
  - Surfaced by: R21
  - Files: `src/learn/assigned.rs`
  - Verify: `cargo test`
- [ ] **T9 (P2, human: ~2h / CC: ~10min)** — pad — SEEN verdict word
  - Surfaced by: R11
  - Files: `src/learn/verdict.rs`
  - Verify: verdict parse table rows
- [ ] **T10 (P2, human: ~1d / CC: ~30min)** — deploy — `tutor.nix` with playwright-driver, systemd blocks, ingress entry, Access bypass for `/device/v1/*`
  - Surfaced by: C1, C3, R2, R6
  - Files: gauntlet `~/Dev/dotfiles/tutor.nix`, `flake.nix`
  - Verify: service starts, self-test passes, blocked paths unreadable from the unit
- [ ] **T11 (P2, human: ~2h / CC: ~15min)** — CI — `test.yml` running cargo test and tutor tests
  - Surfaced by: R13, R14
  - Files: `.github/workflows/test.yml`, `tutor/package.json`
  - Verify: green run on the PR
- [ ] **T12 (P2, human: ~1d / CC: ~45min plus capture)** — evals — 20 ink fixtures, 10 faulty lessons, `npm run eval`
  - Surfaced by: R15
  - Files: `tutor/evals/`
  - Verify: pass rate printed per route

## Unresolved decisions

None. Every choice D1-D30 has an actual answer.

## Completion summary

- Step 0: Scope Challenge — scope reduced per recommendation (D1 local deferred, D3 editor deferred; D4 smaller arrangement)
- Reality rows: 7/7 applicable rows emitted, 2 unverified premises
- Architecture Review: 5 issues found (R6-R10)
- Code Quality Review: 1 issue found (R11), plus 3 implementation notes
- Test Review: diagram produced, 35 gaps identified (4 decisions: R12-R15)
- Performance Review: 2 issues found (R16, R17)
- NOT in scope: written
- What already exists: written
- TODOS.md updates: 3 items proposed to user, 3 added
- Failure modes: 0 critical gaps flagged
- Unresolved decisions: 0 in this review
- Outside voice: codex (gpt-6-astra), completed; 6 findings, 1 correction (C5) and 5 decisions (R18-R22)
- Parallelization: 2 lanes, 2 parallel / 4 sequential steps
- Lake Score: 19/20 = answers picking a 10/10 option / answers scored for Completeness

Scope Challenge findings (reported separately): S1-S5 (R1-R5) and corrections C1-C4.

REALITY: premise-table finding 4 false, 7 partial, unverified=2 (Playwright on gauntlet, grok/hermes headless) plan:57-62
REALITY: already-done finding tick boxes shipped in 0f3eef3 src/learn/sheet.rs:180
REALITY: surface-check finding tunnel config is Nix-declared; grok has no verified headless image flag flake.nix:339
REALITY: binding-decisions pass plan Decisions table honored; review decisions D1-D30 recorded plan:26-32
REALITY: numbers finding CLI and cloud latencies are estimates; 321 tests measured; README.md:118 claims 260 plan:78-80
REALITY: deferred-asks pass no linked issue plan:1
REALITY: pr-count finding phase 1 spans this repo and gauntlet dotfiles, two PRs minimum plan:61-64

## Suppressed findings (appendix)

- (confidence 4/10) Lesson page PNG size: a gray PNG per page is likely
  100-300 KB, four pages per pull; no evidence of a problem.
- (confidence 4/10) `README.md:118` says "260 tests, under three seconds";
  the suite is 321 tests and reported 63 s on this Mac. Doc drift, outside
  this plan.
- (confidence 3/10) Codex read-only sandbox read scope on gauntlet is
  unverified; covered in practice by D10's systemd blocks.

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | `/plan-ceo-review` | Scope & strategy | 0 | — | — |
| Outside Review | codex (`codex exec`, plan-review phase) | Independent 2nd opinion | 1 | completed | 6 findings, all resolved (C5, R18-R22) |
| Eng Review | `/plan-eng-review` | Architecture & tests (required) | 1 | issues_open | 12 issues, 0 critical gaps |
| Design Review | `/plan-design-review` | UI/UX gaps | 0 | — | — |
| DX Review | `/plan-devex-review` | Developer experience gaps | 0 | — | — |

- **OUTSIDE COVERAGE:** codex, plan-review phase, completed 2026-10-24 on commit 7332036 (model gpt-6-astra); 6 findings (4 High, 2 Medium), every one verified against code and resolved.
- **CROSS-MODEL:** native review (Claude) and Codex overlapped on per-box grading (R1/X2) and tick boxes (C2/X5); Codex alone found edit-after-verify (X1), completion sync (X3), the feedback band (X4) and verifier independence (X6).
- **VERDICT:** No review CLEARED. Eng review recorded 12 issues, all with approved remedies (status issues_open means mapped work, not open questions); eng review required to clear after the remedies land in the plan or code.

NO UNRESOLVED DECISIONS
