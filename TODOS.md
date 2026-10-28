# TODOS

## Tutor

### Routes editor in the web app

**What:** A settings page that edits `routes.toml` with validation, a safe file write and a live router reload.

**Why:** Lets a parent without CLIs switch every chain to `cloud` from the browser, without ssh or a restart.

**Context:** The design (`docs/plans/2026-10-23-tutor-service-design.md`, line 96-97) made routes editable from the web app. The eng review deferred the editor (D3) and kept a read-only per-route usage view. Phase 1 reads `routes.toml` at start from the dotfiles-managed config. Start from the router's config loader and the usage view page. Edits must keep the D27 rule: every route declares a model family, and verify skips the generator's family.

**Effort:** M
**Priority:** P3
**Depends on:** Tutor phase 1 router and usage view; D27 family tags.

### `local` model backend

**What:** Add the `local` route kind (an OpenAI-compatible endpoint on localhost, such as Ollama) to the tutor router.

**Why:** Free, offline calls for trivial tasks such as splitting a word into sounds.

**Context:** Design line 80 and 98-99 list `local`; the eng review deferred it (D1) because no phase 1 task routes to it. The `cloud` adapter already speaks the OpenAI-compatible protocol, so this is likely a config entry plus a family tag (D27) and a health check. Never route `grade` to it.

**Effort:** S
**Priority:** P3
**Depends on:** Tutor phase 1 router.

### grok and hermes CLI adapters

**What:** `cli` adapters for `grok` and `hermes` on gauntlet.

**Why:** Spreads chat, generate and verify load across every subscription.

**Context:** Design line 78 names four CLIs; the eng review shipped only `claude` and `codex` (D11). On gauntlet, `grok --help` shows "Grok Build TUI" with no headless or image flag in its first page of help; `hermes --help` was not read. Each adapter needs a verified headless mode, image input, schema output and lockdown equal to D10 (pinned argv with a unit test, systemd blocks), plus a model family tag (D27).

**Effort:** M
**Priority:** P4
**Depends on:** D10 lockdown pattern; D27 family tags.

## Completed
