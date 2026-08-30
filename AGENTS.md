# Agent Instructions

<!-- BEGIN: agent-stack-routing (managed) -->
## Agent Stack Routing

Four tool stacks are installed. Each owns one activity. Do not mix them for the
same activity — pick the owner below and stay in that lane.

**ECC is the lead.** It owns the pipeline and decides what runs next. The other
stacks are invoked at specific steps.

| Activity | Owner | Entry point |
|---|---|---|
| Feature/defect execution | **ECC** (lead) | `/ecc:orch-add-feature`, `/ecc:orch-fix-defect`, `/ecc:orch-build-mvp` |
| Planning & specs | **superpowers** | `brainstorm` → `write-plan` → `execute-plan` |
| Code review | **ECC** | `/ecc:code-review` (auto-routes to the language reviewer) |
| Build/type errors | **ECC** | `/ecc:build-fix` |
| Browser QA, visual/design review | **gstack** | `/qa`, `/browse`, `/design-review` |
| Ship & deploy & post-deploy watch | **gstack** | `/ship` → `/land-and-deploy` → `/canary` |
| Requirements traceability | **RTMX** | `rtmx status`, `rtmx backlog`, `rtmx health` |

### Rules

1. **ECC leads.** Its `orch-*` pipeline is gated Research → Plan → TDD → Review →
   Commit with two human gates. Start there for non-trivial work.
2. **Cross-stack dispatch is manual.** ECC cannot invoke gstack skills and vice
   versa. When the pipeline reaches a browser or deploy step, invoke the gstack
   skill directly, then return to the ECC pipeline.
3. **Never run two stacks for one activity.** Three systems each offer plan and
   review; the table above is the tiebreak. Contradictory workflow guidance
   mid-task is the failure mode this prevents.
4. **gstack needs its daemon.** `/qa`, `/canary`, `/browse`, and `/design-review`
   require Bun and a built browse binary. If they fail, build it:
   `cd ~/.claude/skills/gstack/browse && bun install && bun run build`
5. **gstack ships daily.** Run `/gstack-upgrade` periodically or drift far behind.
6. **Non-web repos skip the gstack lane** for shipping — plain git or ECC is fine
   where there is no browser or deploy target.

### Notes

- This file (`AGENTS.md`) is the portable source of truth, read by Claude Code,
  Codex, Cursor, and other agent harnesses. `CLAUDE.md` should point here rather
  than restate this block.
- ECC exports to 14 agent harnesses and is the most portable of the three; prefer
  it when working outside Claude Code.
<!-- END: agent-stack-routing (managed) -->
