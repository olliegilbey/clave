# Status — clave orchestrator (#3/#10 SHIPPED · PR #13 (#5) green awaiting merge · live-validation round next)

_2026-07-21 13:23 · repo github.com/olliegilbey/clave · main `0161d99` ·
PR #13 open, branch `feat/snapshot-collapsed-flag` HEAD `c7eb3d0`, checks GREEN_

Predecessor: @docs/status/2026-07-20-1928-clave-orchestrator.md — the full
session log of everything below (fugu review details, spend-limit incident,
coordination-loop lessons). Read it only if you need the WHY behind a
decision; this file is the working state.

## Task Overview

clave = Zellij fleet-orchestration sidebar (wasm `clave-bar` + `clave` CLI).
Work is issue-driven via public GitHub issues, feature-branch PRs into
protected `main` (checks `test` + `wasm-build` + conversation resolution;
0 approvals, solo maintainer signs everything via 1Password). The
coordinating session specs briefs, dispatches subagent implementers/
reviewers when spend allows, personally supplements, and NEVER commits
without the maintainer (he must approve; commits pop 1Password on his
side — running `git commit` from the session is fine once he's approved).

## Current State

- **Merged**: PR #12 → squash `0161d99` on main (issue #10: KDL real-parser
  guardrail, SimZellij convergence harness, 8 proptests, zellij+kdl pin
  tripwire — 126 tests at that point). Issues #1/#2/#3/#10 CLOSED.
  Squash-merge is the repo convention (ratified on #12).
- **PR #13 open + green, ready to merge** (closes #5): snapshot-carried
  collapsed flag. `3bac9cd` feature + `c7eb3d0` review fixes. Design:
  `clave-toggle` broadcast keeps instant flips; `model.toggle()` books a
  pending-write ledger + emits `Effect::PersistCollapse`; active instance
  runs hidden `clave collapse <bool>` (ArgAction::Set REQUIRED — see
  Discoveries); `store::apply_collapse` = absolute value, change-gated
  seq-bump + push; `apply_snapshot` heals on change only, settles/
  re-asserts the ledger (one retry, then store wins). 135 tests green.
- **Worktrees**: `.claude/worktrees/issue-10-kdl-guardrail` (session home,
  branch test/guardrails-issue-10 — merged, disposable) and
  `.claude/worktrees/issue-5-collapsed-snapshot` (branch = PR #13, tree
  clean). Both removable once #13 merges.
- Fugu workflow upgraded globally (`~/.claude/workflows/fugu-review.js`):
  gemini CLI lane added, lane-count wording dynamic, args JSON-string
  hardening. Gemini lane blocked on quota (see Discoveries).

## Important Discoveries

1. **clap-derive bool positional is a FLAG**: bare `collapsed: bool` in a
   subcommand became SetTrue — debug builds panic clap's debug_assert on
   EVERY parse; `clave collapse true` could never work. Caught only by
   CodeRabbit CLI + an end-to-end run (`CLAVE_STATE_DIR=<scratch> cargo
   run -p clave -- collapse true`). Fix: `#[arg(action =
   clap::ArgAction::Set)]` + parse test in clave main.rs. LESSON: any new
   CLI surface needs a `Cli::try_parse_from` test + one sandboxed
   end-to-end run; workspace tests don't touch the CLI layer.
2. **Out-of-order persist writes** (model reviewer MAJOR): two rapid
   toggles → two subprocesses, no arrival-order guarantee → change-gate
   swallows the correct write, store pushes stale truth, STICKY. Fixed by
   the pending-write ledger in model.rs (`pending_collapse` +
   `collapse_reasserted`, test
   `out_of_order_write_is_reasserted_once_then_store_wins`).
3. **CodeRabbit cloud free plan is rate-limit flaky** (~1 review/hour;
   three "finished"-but-empty responses on #12). The CLI
   (`coderabbit review --agent --type committed --base main`, run in the
   worktree) is the reliable path and found the clap bug. Declined its
   third finding (beacon-election executor gate) with reasoning on PR #13
   — revisit only if live C8 scenarios show duplicate-write noise.
4. **Spend limit / gemini quota**: Claude monthly spend limit hit once
   (subagent died mid-flight; later dispatches worked — intermittent).
   Gemini CLI works but the 1Password-injected GEMINI_API_KEY is
   free-tier (~20/day, exhausted); OAuth login (~1000/day) or billing —
   maintainer's call, wrapper overrides OAuth today.
5. **Workflow args must be a real JSON object** — a stringified JSON args
   silently dropped `cli_reviewers` (now hardened in the script, but
   don't regress the calling convention).
6. Subagents cannot write scratchpad report files — briefs must say
   "return the report as your final text response".
7. Stacked-branch pattern used for #13 (based on #12's commit because both
   touch model.rs's tests tail); after #12's squash-merge the re-target
   was `git checkout -B <branch> origin/main` + re-sync of the one file
   the squash changed. Worked cleanly; remember squash = new SHAs.

## Next Steps

1. **Merge PR #13** (maintainer's click or `gh pr merge 13 --squash
   --subject "feat(clave): snapshot-carried collapsed flag — close the
   parity-desync family (#13)"` + body `Closes #5` + Claude-Session
   trailer). Auto-closes #5. Then optionally clean both worktrees and
   delete merged branches.
2. **Live-validation round (maintainer at the terminal, TESTING.md SOP)**:
   sandbox reseed first (`just dev-install`, maintainer relaunches
   `clave-test` — config generation format changed since last seed). Then
   validate #5 live: two-instance session → Alt+c → verify store
   `collapsed` flips + all bars converge; kill/miss one pipe (reload one
   bar) → watch the snapshot heal it; double-Alt+c fast → no sticky
   desync. The executor gate (`is_active_instance`) is host-untestable —
   this live pass is its only verification.
3. **v0.1.0 cut**: tag → first-ever `just release` (WATCH IT LIVE — it
   regenerates the real stable config to versioned paths) → maintainer
   relaunches daily `clave`.
4. Backlog after that: #4 (width-seek drift re-arm), #6 (tab_id
   verify+prune), #9 (parked lints + fmt sweep — also add fmt/clippy to
   CI gate there; also consider workspace zellij-tile `=0.44.3` exact
   pin, fugu nit), #7/#8/#11 as directed.

**Where work stopped — verbatim:** PR #13 checks came back green
(`test pass 30s`, `wasm-build pass 26s`) right as the maintainer invoked
/handoff: "keep what's helpful for the next agent to continue." Merge of
#13 was proposed but NOT yet confirmed by the maintainer.

## Context to Preserve

- **User prefs (unchanged, binding)**: extremely concise; explain while
  doing; dense why-comments citing spec §/ledger; conventional commits +
  `Claude-Session: <session URL>` trailer on its own line; he signs via
  1Password (session may run `git commit` after his approval — the popup
  is his approval surface); ask before architecture decisions; he drives
  ALL live zellij input; session shell lives INSIDE his `main` zellij
  session (memory file) — no zellij commands except sanctioned sandbox
  ops; never install to stable surfaces; session lifecycle is his.
- **SSH constraint (memory: clave-must-work-over-ssh)**: clave must
  eventually work over SSH like zellij — reject designs assuming CLI and
  plugin share a local desktop; host-side store/pipes/subprocess = fine.
- **Review flow that works**: explore → brief file (constraints + report
  contract) → implementer → independent reviewer → coordinator
  adjudicates + personal pass → CodeRabbit CLI as extra lane on the
  committed branch. CodeRabbit cloud auto-review may work on future PRs
  (config is live) but don't block on it.
- **Verification bar**: `cargo test --workspace` (--workspace is
  load-bearing) AND `cargo build -p clave-bar --target wasm32-wasip1`;
  clippy must stay at the 4 pre-existing parked production lints (#9);
  rustfmt --edition 2024 on NEW code only — full-file rustfmt on model.rs
  /store.rs re-flows pre-existing drift and pollutes diffs (bitten once).

## Restart Hint

Everything committed and pushed; both worktrees clean; PR #13 green. Safe
to /clear. Resume: confirm #13 merge with the maintainer, then Next Step 2
(sandbox reseed + live validation) — that whole phase is
maintainer-at-the-terminal, TESTING.md is the SOP.
