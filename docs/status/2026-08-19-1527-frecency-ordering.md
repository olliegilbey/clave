You are the driver of a live end-to-end validation session for clave's new frecency ordering feature, with a strong understanding of the clave store→snapshot→bar pipeline, the zellij sandbox drive SOP, and adversarial QA instincts — you verify by evidence, never by expectation.

## Task Overview

The frecency-ordering feature is fully implemented, reviewed, and pushed as **PR #218** (branch `worktree-frecency-ordering`, HEAD `7cf4f5c`, base `main`@3fc27ca). Feature issue: #216. Follow-up bug issue (recency flakiness/terminal commitments — OUT of scope here): #217.

Remaining work: **drive the sandbox end-to-end** across all ordering permutations with mocked buckets/decays, explore additional checks beyond the scripted matrix, post the evidence to PR #218 (numbered live steps, sandbox-first, per TESTING.md ~line 113), fix anything the drive surfaces, and leave the branch in a beautiful merged-ready state.

**Your literal first message to Ollie must be the launch command** (Current State below) — he runs it in a new terminal outside zellij; you drive only after he confirms launch.

## Reference Docs

- `docs/superpowers/plans/2026-08-19-frecency-e2e-drive.md` — THE drive plan: staging/seeding steps, the 8-permutation matrix, teardown, reporting. Read whole (short). Explore beyond it where instinct says.
- `docs/dev/TESTING.md` lines ~722-895 — the sandbox drive loop SOP (build-tag proof from log TAIL, baseline-before-provoke, re-join after every provocation, 60s quiescence with frozen seq+evlog, forced tab-id reuse, report-the-unexercised, hand back teardown). Follow it exactly; each step exists because skipping it produced a confident wrong result.
- `docs/superpowers/specs/2026-08-19-frecency-ordering.md` — the binding design (score formula, zero→ordinal fallback, opener inheritance, 7-day GC).
- `docs/dev/qa/` + `docs/dev/LIVE-INTERACTION-CHECKLIST.md` — additional live checks worth folding in.
- FOOTGUNS.md — grep before debugging anything odd (zellij log is USER-GLOBAL; TabUpdate reaches only the active tab's instance).

## Current State

- Branch pushed; PR #218 open; workspace green: 488 tests, all four gates, mutation runs clean (bar 24 mutants, host 18 — all killed/pinned). SDD ledger deleted after final review "Ready to merge" (git history is the record).
- Sandbox **staged and seeded**, awaiting launch. Untracked (commit if useful): `docs/superpowers/plans/2026-08-19-frecency-e2e-drive.md`, `scripts/frecency-drive-seed.sh` (idempotent re-seeder; re-run after any `just sandbox` re-stage, since staging rewrites the store).
- **Launch command for Ollie (give verbatim, first message):**

```
CLAVE_SESSION=clave-test-frecency-735d \
CLAVE_STATE_DIR=/Users/olliegilbey/.local/state/clave-dev-frecency-735d/state \
CLAVE_DATA_DIR=/Users/olliegilbey/.local/state/clave-dev-frecency-735d/data \
PATH="/Users/olliegilbey/.local/state/clave-dev-frecency-735d/shim:$PATH" \
"/Users/olliegilbey/code/clave/.claude/worktrees/frecency-ordering/target/release/clave" dev launch
```

- Seeded store (`…clave-dev-frecency-735d/state/agents.json`, seq=100, today was 20684 at seeding — re-derive as `date +%s / 86400`):
  - `…c85c…0001` "invested": buckets {t-6:8, t-3:8, t-1:8, t:2} ≈ 7125 millipoints, ord 0
  - `…c85c…0002` "one-off today": {t:1} = 1000 millipoints, ord 99 (newest ordinal)
  - `…c85c…0003` "dormant giant": {t-6:30} ≈ 468 millipoints, ord 0
- Expected orders — frecency24h: invested > one-off > giant · recency: one-off top · dial 999h: giant top (~27k) · dial 1h: ≈recency. Three modes, three visibly distinct orders.

## What's Working

- The whole feature is verified green and review-hardened; build on it, don't re-litigate it. Zero-score rows fall back to ordinal order by design — an unbucketed store ordering "like before" is a PASS, not a bug.
- Sandbox commands are safe when prefixed with the sandbox env (`CLAVE_STATE_DIR=… clave order …` via the shim'd binary); `zellij action` against `clave-test-frecency-735d` (`ZELLIJ_SESSION_NAME=…`) is yours to run freely.
- `clave order` bare prints Rust Debug format (e.g. `Frecency { half_life_hours: 24 }`) — sanctioned, not a defect.
- Seeded rows have NO real Claude processes: permutation 5 (prompt banks a point) must simulate via the hook path (`clave hook` UserPromptSubmit with CLAVE_AGENT_UUID + sandbox state dir) and be labeled as such in the report.

## Important Discoveries

- The worktree isolation guard refuses compound Bash commands with redirects to paths outside the worktree — write helper scripts inside the worktree (like `scripts/frecency-drive-seed.sh`) and `bash` them.
- Staging (`just sandbox`) REWRITES the sandbox store — always re-seed after re-staging.
- Retention window is today-6..=today (strict `>` in `bump_bucket`) — ruled during implementation; the t-6 bucket survives, worth 0.5^6.
- Inherited copies are exact and unpruned at seed time; adjacency = identical score + position tiebreak (opener above, newborn directly below).
- Bar diagnostics in the IDE lag reality — trust `cargo check`/tests, not stale E0063s.

## Next Steps

1. Give Ollie the launch command (verbatim above). Wait for his confirmation.
2. Build-tag proof: `grep 'clave-bar: loaded' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -5` — TAIL must carry this build's tag (from `git rev-parse --short HEAD`); stop if not.
3. Baseline: store JSON + `zellij action` pane/tab truth, joined, before provoking.
4. Run the 8-permutation matrix from the drive plan; re-join after every provocation; add exploratory checks (e.g. `clave order frecency 0` → clamps to 1h; collapse-mode rendering; C's dormant-block position under each mode).
5. Quiescence 60s (seq + evlog frozen), forced tab-id reuse round.
6. Post numbered evidence to PR #218 (comment), including anything NOT exercised, labeled in those words.
7. Print the teardown kill pair for Ollie (TESTING.md § tear the sandbox down).
8. Fix anything surfaced (small fixes on this branch, re-run gates, push).

Last exchange — Ollie: "Make sure that you can use the testing and qa system to check the full flow and drive it in a sandbox to test all the permutations and orderings, mocking the buckets and decays. You can set up a sandbox that I'll launch for you to drive. Need to test end to end with this."

## Context for the Work

- **Never touch Ollie's live clave session** — no bare `zellij` commands; the sandbox env prefix is mandatory on every drive command (it fails OPEN onto his fleet otherwise). Launching/killing sessions is his; print commands for him.
- Decision ledger (grilled + ratified): frecency default (24h half-life dial via `clave order`), user-turn commitments only (Stop/finish never reorders), 7-day retention-as-GC (not a rule), opener = max-tab_order proxy (ratified over true-focus plumbing), exact-copy inheritance with position-tiebreak adjacency, zero→ordinal fallback, terminals earn commitments only in follow-up #217.
- Ollie's report style: lead with outcome, plain sentences, no unglossed symbols, decision-not-mechanism; background any CI waits and report between them.
- PR evidence convention: numbered live steps, sandbox-first, each with what was observed (TESTING.md ~113).

## Restart Hint

Tests green, branch pushed; two untracked drive artifacts in the worktree — commit them to the branch as drive evidence infra (no stash needed). Safe to start immediately with the launch command.

## Suggested Skills

- `superpowers:verification-before-completion` — before claiming the drive/PR done.
- `superpowers:systematic-debugging` — the moment any drive step surprises (after grepping FOOTGUNS.md).
- `mattpocock-skills:diagnosing-bugs` — if a permutation fails and needs a root cause.
- `handoff` — if the drive outlives the context window again.
