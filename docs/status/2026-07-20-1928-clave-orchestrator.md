# Status — clave orchestrator (#3 SHIPPED · #10 COMPLETE signature-ready · afk-autonomous session)

_2026-07-20 19:28 · repo github.com/olliegilbey/clave · main HEAD `29ba832` ·
work-in-flight on worktree branch `worktree-issue-10-kdl-guardrail`_

Predecessor: @docs/status/2026-07-20-1830-clave-orchestrator.md (coordination
loop + backlog ranking; still accurate).

## What happened this session

1. **Pushed `main`** (`7fa1caf..6feda56`, then `..29ba832`) — issues #1/#2
   auto-closed.
2. **Issue #3 SHIPPED + CLOSED**: `29ba832` adds `.github/workflows/ci.yml`
   (required checks `test` = cargo test --workspace, `wasm-build` =
   clave-bar → wasm32-wasip1; first run green) and `.coderabbit.yaml`
   (assertive, path filters, why-comment + wasm-safety path instructions).
   Branch protection live on `main`: PRs required, both checks strict
   (up-to-date branch), 0 approvals (solo maintainer), enforce_admins ON,
   conversation resolution required, no force-push/delete. fmt/clippy NOT
   CI-gated yet — joins with #9 (tree carries 4 parked lints + fmt drift).
   **CHECK: is the CodeRabbit GitHub app installed on the repo?** The yaml
   is inert without it; installing is an OAuth flow only the maintainer
   can do.
3. **Issue #10 item 1 (KDL real-parser guardrail) implemented, reviewed,
   signature-ready** in worktree
   `.claude/worktrees/issue-10-kdl-guardrail` (branch
   `worktree-issue-10-kdl-guardrail`, based on `29ba832`). UNCOMMITTED —
   maintainer signs. Diff: NEW `crates/clave/tests/kdl_guardrail.rs`
   (6 tests), `crates/clave/Cargo.toml` dev-deps
   (`zellij-utils = "=0.44.3"`, `kdl = "=4.7.1"`, `clave-types` path —
   exact pins why-commented as the #10-item-5 tripwire seed), Cargo.lock
   +2 edge lines (no version drift). 110 tests green
   (`cargo test --workspace`), wasm build unaffected (verified).

## How it was built (the loop, it worked again)

Explore agent mapped the generation surface → brief FILE with hard
constraints + report contract → opus implementer (TDD, real regression
demo'd + reverted) → sonnet reviewer independently re-verified everything
(parser entry points against vendored source, lockfile, wasm build) →
verdict approve, 1 NIT (fmt) → coordinator personal pass: fixed the NIT
(rustfmt on the new file only) AND caught a real weakness the reviewer
missed — the synthetic missing-`;` sensitivity case was being rejected for
an unrelated reason (`Invalid mode: 'bind'`); replaced with a
differential mutation of the REAL config_kdl output (unmutated parses,
one `};`→`}` strip must fail). That differential also empirically
confirms the Task 9 C1 trailing-`;` comment in setup.rs.

NOTE for the loop ledger: subagents could not write report files to the
scratchpad (harness blocks non-repo writes for them) — reports came back
as final-text and the coordinator persisted them. Adjust future briefs:
tell the agent to return the report as text directly.

## The guardrail's coverage

Every KDL generator: config_kdl (both binary forms), layout_kdl,
launch_layout_kdl (both branches), merge_permissions_kdl (both branches,
kdl-crate well-formedness — zellij exposes no PermissionCache string
entry), add::tab_layout (through real sanitize_label/validate_cwd), plus
the non-vacuity rejection pair. permissions caveat and =pin rationale are
why-commented in the test header and Cargo.toml.

## SESSION CONTINUED — issue #10 is now COMPLETE (all 4 proposals)

After the item-1 handoff above, the same loop ran twice more. Worktree
branch `worktree-issue-10-kdl-guardrail` now carries, ALL UNCOMMITTED,
124 tests green (`cargo test --workspace`), wasm build verified clean:

- **Items 2+3** (opus implementer → sonnet reviewer APPROVE, both
  sensitivity mutations independently reproduced by the reviewer):
  `crates/clave-bar/src/model.rs` +550 lines all inside `mod tests` —
  `SimZellij` + `drive()` convergence harness (coarse step, floor
  refusal, one-render latency for the in-flight guard, Toggle/Jump
  interrupts), 6 harness example tests, 7 proptests (128 cases each,
  0.01s) covering the convergence contract (a)-(d), focus-never-reorders
  §6.6, rows determinism/recency, seq gate, timeline wholesale-replace,
  nav closure + cursor_gen, classify_timer. `crates/clave-bar/Cargo.toml`
  gains `[dev-dependencies] proptest = "1"` (why-commented; verified
  absent from the wasm --target build). Reviewer NITs (accepted, no
  action): prop 1 generates an unreachable (collapsed=false,
  peeking=true) combo harmlessly; prop 7 pins classify_timer's decision
  table rather than independently deriving it.
- **Item 4** (pin tripwire) — COORDINATOR-AUTHORED, the least-reviewed
  piece of the branch, look here first: NEW
  `crates/clave/tests/zellij_pin_tripwire.rs` parses the workspace
  Cargo.lock and asserts every zellij-family crate resolves to exactly
  0.44.3 (PINNED_ZELLIJ). Rationale in its header: `=` dev-dep pins do
  NOT fail on a zellij-tile bump — cargo holds two zellij-utils versions
  and the kdl_guardrail would silently validate against the wrong
  parser. Fires with re-audit instructions (call-site list seeded from
  the ledger — MAINTAINER: vet that list). TDD'd: mutated pin → fails
  with guidance; reverted → green.
- Loop lesson added to the ledger-of-record (this file): briefs must say
  "return report as final text" — subagent scratchpad writes are blocked.
  rustfmt here is `--edition 2024` (a brief said 2021; implementer
  correctly flagged instead of failing).

## SECOND CONTINUATION (2026-07-21 early) — fugu review ran, fixes applied

- **Fugu review** (user-requested pre-PR): 3 blind model reviewers + opus
  verifier. Verdict: "go with minor changes", zero blockers, work called
  "unusually careful". Gemini CLI lane was ADDED to the global
  `~/.claude/workflows/fugu-review.js` (user-ratified promotion) along
  with dynamic lane-count wording. CAVEAT: in the actual run the CLI
  lanes (coderabbit/codex/gemini) did NOT execute — the Workflow args
  were passed as a JSON STRING, so `cli_reviewers` never parsed
  (lesson: pass Workflow args as a real object). Gemini lane exercised
  standalone afterwards instead.
- **Fixes applied per the report** (user pre-authorized "act on whatever
  you think is correct"): (1) permissions.kdl non-vacuity guard —
  brace-truncated merged doc must fail kdl parse; (2) tripwire now also
  audits the `kdl` crate (PINNED_KDL=4.7.1, exactly-one-version
  assertion); (3) classify_timer property de-tautologized → two
  spec-phrased partial contracts (7a short-elapsed-always-dwell, 7b
  pending-peek-owns-long-expiries) + a fixed boundary test for the
  late-dwell rescue; (4) trailing-`;` differential comment now names the
  Alt+a Run bind as the mutated node; (5) nav-closure docstring narrowed
  to what it proves (executor pinned; progression = live-only); (6)
  resolver="3" isolation note on the zellij-utils dev-dep. Also fixed 2
  clippy lints the new test code had introduced (field_reassign,
  needless_range_loop) — clippy is back to only the pre-existing parked
  production sites (#9).
- **Declined from the report**: workspace-level zellij-tile exact-pin
  (dependency policy — maintainer's call, revisit with #9); REFUTED
  lockfile-scanner nit (no action).
- **Final state: 126 tests green** (`cargo test --workspace`), wasm build
  clean, tree ready for signature.

## Next steps

1. **User: sign + PR the worktree branch** — closes #10 entirely; first
   PR under the new protection (CI green = live test of #3). Suggested:
   branch rename `test/guardrails-issue-10`, commit
   `test(clave): #10 guardrails — KDL real-parser validation, width-seek
   convergence harness, model proptests, zellij pin tripwire` +
   Claude-Session trailer. Diff: 2 new test files, model.rs tests-module
   append, 2 Cargo.tomls dev-deps, Cargo.lock mechanical.
2. Deferred for the maintainer (need live/you): v0.1.0 cut + first
   `just release` (watch live), sandbox reseed (`just dev-install` +
   relaunch), then #5/#4/#6 (bar work, architecture calls — NOT touched
   autonomously on purpose). CodeRabbit app: user confirmed installed.
3. Consider: workspace zellij-tile `=0.44.3` exact pin (fugu nit) when
   doing #9.
4. Gemini fugu lane: works mechanically but the 1Password-injected
   GEMINI_API_KEY is free-tier (daily quota died mid-review). The shell
   wrapper overrides OAuth — either add billing to the key or drop the
   wrapper and `gemini` login with the Google account (~1000 req/day
   free). Workflow degrades gracefully until then. Also hardened:
   fugu-review now JSON-parses stringified args so cli_reviewers can't
   silently drop (the bug that skipped CLI lanes in this session's run).

## THIRD CONTINUATION (2026-07-21, user asleep) — PR #12 live, #5 started

- **PR #12** (test/guardrails-issue-10 → main, closes #10): user signed
  `0e3b681`, pushed, PR opened. Required checks GREEN (test 1m03s,
  wasm-build 27s), MERGEABLE. CodeRabbit rate-limited (free-plan window);
  timer armed → re-tag `@coderabbitai review` → triage → fix → commit to
  the PR branch (user authorized; fall back to staged + `!` command if
  the classifier blocks the commit).
- **Issue #5 loop started** in worktree
  `.claude/worktrees/issue-5-collapsed-snapshot`, branch
  `feat/snapshot-collapsed-flag`, STACKED on test/guardrails-issue-10
  (both touch model.rs's test module — branching off main would
  guarantee a conflict; merge #12 first, then PR #5 targets main
  cleanly). Coordinator session stays cwd'd in the issue-10 worktree for
  PR-fix work; #5 subagents work by absolute path.
- **NEW standing constraint (memory: clave-must-work-over-ssh)**: clave
  must eventually work over SSH like zellij — design lens: nothing may
  assume CLI+plugin share a local desktop; host-side (store/pipes/data
  dirs with the zellij server) is fine. In #5 briefs and future work.

## FOURTH CONTINUATION (2026-07-21, user asleep) — #5 implemented solo

- **Claude monthly spend limit HIT mid-session** (first #5 implementer
  agent died on it; raise at claude.ai/settings/usage). Coordinator
  implemented #5 SOLO in consequence; a later reviewer probe launched OK
  — limit behavior appears intermittent/tier-dependent.
- **Issue #5 implemented** in worktree issue-5-collapsed-snapshot
  (branch feat/snapshot-collapsed-flag, stacked on the PR #12 commit),
  UNCOMMITTED, 132 tests green + wasm clean. Design as briefed:
  `clave-toggle` broadcast keeps instant flips; ACTIVE instance persists
  the ABSOLUTE mode via new hidden `clave collapse <bool>` →
  `store::apply_collapse` (change-gated seq-bump + push);
  `AgentSnapshot.collapsed` (serde default=false) rides every push;
  `apply_snapshot` heals ON CHANGE ONLY (re-arm seek, clear peeking) with
  the accepted old-flag transient why-commented. Tests: serde default,
  store dedupe, birth-while-collapsed, missed-pipe heal + no-rearm-when-
  synced, stale-gate, last-writer-wins proptest. Two pre-existing
  fixtures updated to collapsed:true (their snapshots now carry store
  truth — semantically faithful, flagged for review).
- **Known gap**: the executor gate in toggle_collapsed (plugin main.rs)
  is untestable on host (wasm bin, test=false) — same standing gap as
  MarkRead/Bind gating. Live validation needed post-sandbox-reseed:
  scenario = two-instance toggle, kill one pipe, watch heal.
- PR #12: CodeRabbit re-tagged in-window; ack received, review pending
  (monitor armed).
- **#5 reviewed + MAJOR fixed** (sonnet reviewer survived the spend
  limit): review verdict needs-fixes with one MAJOR — two rapid toggles'
  `clave collapse` subprocesses have no arrival-order guarantee; the
  change-gate can swallow the correct write and the store's push then
  overrides the user, STICKY. Fixed with a **pending-write ledger** in
  the model (also resolves the reviewer's Effect-shape NIT):
  `toggle()` now books `pending_collapse` and emits
  `Effect::PersistCollapse` (executed active-gated in run_effects like
  MarkRead/Bind); `apply_snapshot` settles the debt on a confirming
  flag, absorbs the FIRST contradiction (keeps user truth, re-asserts
  once), yields to the store on the second (`collapse_reasserted` —
  wrong-but-consistent beats a two-instance ping-pong). All pure model
  logic, host-tested: `toggle_emits_the_persist_effect...`,
  `out_of_order_write_is_reasserted_once_then_store_wins`, confirming-
  push-is-silent assertion, reworked spec-fold proptest. **134 tests
  green**, wasm clean, clippy = 4 parked only, new-code fmt clean.
  Remaining review NITs accepted as-is: proptest-fold mirroring
  (documented; example tests carry mutation burden), no CLI-level
  `clave collapse` test (matches Bind/Touch/Focus precedent).

## Standing context

Unchanged from predecessor: zellij CLI safety, signing flow, subagent
constraints, coordination-loop pattern. Task list in-session tracks the
same structure.
