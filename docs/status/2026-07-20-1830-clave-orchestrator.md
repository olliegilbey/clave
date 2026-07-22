# Status — clave orchestrator (Task 9 CLOSED · issues #1/#2 SHIPPED · fleet-coordination phase begins)

_2026-07-20 18:30 · repo github.com/olliegilbey/clave · branch `main` · HEAD `6feda56` · tree CLEAN (untracked: docs/status/, .claude/ settings only)_

Predecessor: @docs/status/2026-07-18-1240-clave-orchestrator.md (Alt+c bug
framing + C1–C7 history; everything it lists as open is now DONE).

## Task Overview

Build **clave** (Zellij sidebar orchestrator: wasm `clave-bar` + `clave`
CLI). This session CLOSED Task 9 (validation C1–C10 all PASS), shipped the
stable/dev split, and moved the project into **issue-driven fleet
coordination**: work now lives in public GitHub issues; the coordinating
session specs briefs and dispatches implementer/reviewer subagents, then
personally supplements with session-held context. YOUR job as the next
coordinator: run that loop for the remaining issues (see Next Steps).

## Reference Docs

- `docs/superpowers/specs/2026-07-20-stable-dev-split-design.md` — the
  LOCKED design (whole file, it's short). §2 release mechanics (now
  implemented), §3 workflow, §6 issue seed list.
- `CONTRIBUTING.md`, `docs/dev/TESTING.md`, root `CLAUDE.md` — NEW, written
  this session for zero-context agents/humans. TESTING.md is the live-
  validation SOP (interaction contract, observability map, instrumentation
  recipe). Read CLAUDE.md first; it points at the rest. These now carry
  most operating knowledge old status files used to carry.
- `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` — lore ledger; C8
  section now holds the fixed-pane root cause, parity-desync family, and
  final verdicts.
- GitHub: labels (bar/cli/harness/docs/upstream-watch), milestone v0.1.0,
  issues #1–#11 (#1 `1c5da76`, #2 `6feda56` — close on next push).

## Current State

- **All committed+signed through `6feda56`**; test gate 104 green
  (`cargo test --workspace`), clippy = only the 4 parked lints (issue #9).
- Task 9 validation: C1–C10 verdicts written (`a4ab4d0`). Root-caused &
  fixed en route: Alt+c dead in fresh sessions = zellij refuses resizes on
  fixed-size panes → percent panes + birth-armed seek (`0f69a13`).
- Fugu whole-range review ran (7 findings); quick wins shipped
  (`ac0c917`); mediums became issues #4/#5/#6.
- Release mechanics live in code (`1c5da76`): versioned wasm+CLI installs,
  release gate, binary-path-aware generation, merge_hooks
  replace-on-version-change, `--version`, `just dev-install`/`release`
  (`install` retired), reset preserves `data/`.
- **NOT yet done**: no push to GitHub since issue creation; no v0.1.0 tag;
  no `just release` run ever (stable install still pre-split layout — the
  real session runs old-wasm/old-config fine); sandbox NOT refreshed since
  the generation format changed.

## Important Discoveries

1. **The coordination loop that worked** (user wants it continued): write
   task brief as a scratchpad FILE (complete requirements, explore-first
   instruction, hard constraints, report-file path + status contract) →
   dispatch opus implementer(s), parallel only when file-sets are disjoint
   → reviewer subagent (sonnet small/mechanical, opus design-heavy) with
   brief+report+diff-package paths → coordinator adjudicates findings,
   dispatches ONE fixer for the batch, personally verifies fixer diffs →
   coordinator does a PERSONAL supplement pass adding session-only context
   → stage, user signs. Subagent hard constraints that mattered: never
   install/write outside repo (live env reads install paths!), no zellij
   commands, no commits, name files another agent owns as off-limits.
2. **Subagents misread task-constraints as doc content**: the docs agent
   almost encoded my per-task "never install to ~/.cargo/bin" as a
   standing rule (spec says dev-install owns that path). It flagged
   rather than picked — because the brief told it to flag contradictions.
   Always include that instruction.
3. **Stale-diagnostics trap**: IDE diagnostics arriving mid-agent-work
   showed compile errors while the agent's final state was green — verify
   with `cargo test --workspace` yourself before reacting.
4. **Hook-slot policy (user-ratified)**: single shared hook entry;
   releases pin it to the versioned binary; dev scenario/setup borrow it;
   next release heals. Documented in CONTRIBUTING.md.
5. **Reviewer minor left OPEN by choice**: release gate doesn't check
   HEAD-is-on-main (clean tagged feature branch would pass) — branch
   protection (issue #3) closes it at the GitHub layer.
6. Old lore still binding: zellij log shared across sessions (filter date
   AND build tag); `zellij action` on absent session blocks forever;
   1Password signing — stage, user signs; hot-reload resets plugin state.

## Next Steps

1. **Push to GitHub** (user's call/command) — auto-closes #1/#2.
2. **Issue #3**: PR workflow switch — branch protection on main (require
   PR + checks), CodeRabbit config. Mostly `gh api` + config file; small.
   Then **cut v0.1.0**: tag → `just release` (first-ever run — watch it
   live; it regenerates the user's real stable config to versioned paths)
   → user relaunches daily `clave` when ready.
3. **Sandbox refresh before any live validation**: `just dev-install`,
   then user reseeds+relaunches (config generation format changed).
4. Then the backlog issues, subagent-driven, roughly: #10 (KDL real-parser
   test guardrail — recommended BEFORE more bar work), #5 (snapshot
   collapsed flag — closes parity-desync family), #4 (drift re-arm), #6
   (tab_id verify+prune), #9 (parked lints sweep), #7/#8/#11 as user
   directs. Post-#3, all of these go via feature-branch PRs, not
   direct-to-main.

**Where work stopped — verbatim last exchange:**
> **User:** "yep" (approving the two commits)
>
> **Claude:** "Both landed, signed: `1c5da76` (release mechanics, issue
> #1) and `6feda56` (docs trio, issue #2). … **Issue #3** (PR workflow
> switch) is next and is mostly GitHub-side: branch protection on `main`
> and CodeRabbit config. It's also the natural moment to cut **v0.1.0** …
> Before the next live round: `just dev-install` and a sandbox relaunch."
> Then user invoked /handoff "for the next coordinating agent to know
> what to do and how to spec and inform the subagents for everything to
> tackle next."

## Context to Preserve

- **User prefs**: extremely concise; explain while doing; dense
  why-comments; conventional commits ending
  `Claude-Session: <session URL>` on its own line; ask before commits
  (he signs via 1Password — retry when he says unlocked); ask before
  architecture decisions (his instincts have been right repeatedly); he
  drives ALL live zellij input, agents read observability (now codified
  in TESTING.md — follow it). Subagent models: opus implementers for
  design-bearing work, sonnet for mechanical, fable only whole-branch.
- **Meta-goal**: clave is becoming the user's daily driver AND the tool
  he manages agent fleets with — coordination quality is the product.
  Docs must serve zero-context humans+agents ("beautiful and cohesive").
- **Zellij CLI safety** (memory file + TESTING.md): Claude's shell is
  INSIDE his `main` zellij session. Only sanctioned: env-scoped
  hot-reload/dump-layout/status against `clave-test`, read-only
  list-sessions. Session lifecycle is ALWAYS his — print commands.
- **Env**: real session `clave` still on pre-split artifacts until he
  relaunches post-release. Sandbox `clave-test` state unknown-stale after
  this session's format changes — treat as needs-reseed. Vendored zellij
  source paths + zellij-server-from-crates.io trick are in TESTING.md.
- Scratchpad briefs/reports from this session die with it — the PATTERN
  is what carries (Discovery 1), not the files.

## Restart Hint

Tree clean, all signed, tests green. Safe to /clear. Start at Next Step 1
(push), then #3 + v0.1.0 cut. Read root CLAUDE.md before anything else.
