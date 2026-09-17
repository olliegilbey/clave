# Task Pickup

You are picking up `worktree-live-set-restore` (PR #261) after the defect the
QA drive found was FIXED and PROVED live. The branch is ready to merge on the
evidence. A swarm review was running when this was written; its report is the
only open item.

## Orientation

- **The defect is fixed, and the fix is one line removed.** | `SessionEnd`
  cleared `tab_id` as well as `pane_id`, so a row lost its tab while the tab
  was still on screen, and `clear_session_order` builds the restore set from
  `tab_id`. | **Checked** — `crates/clave/src/hook.rs`, the `SessionEnd` arm of
  `apply_hook_pane`. The rationale is written at the site.
- **`pane_id` says where the process runs; `tab_id` says where the row
  lives.** | That is the whole model change. The event that means a tab is
  gone is `clave prune-tabs`, untouched. | **Checked** — the row is left
  holding a tab and no pane, the same state a RESTORED row already sits in,
  so the bar renders and re-adopts it with no new code.
- **Run 16 (2026-09-16) is the first ALL TWELVE phases green.** | Five bound
  before the quit, five back, the minted row among them, the closed tab
  absent. | **Checked** — recorded in `docs/dev/QA-DRIVE.md`.
- **Phase 6c cost four live runs to make honest, and that is the story.** |
  Run 13 proved the fix and went red on an assertion phase 4 had invalidated.
  Runs 14 and 15 reached the relaunch measuring a fleet with no witness in it.
  | **Checked** — both traps are in `docs/FOOTGUNS.md` under "Process and
  tooling".
- **The one visible trade, and the maintainer has NOT eyeballed it.** | A tab
  whose claude exited now keeps that agent's row instead of reverting to a
  terminal row. `agent_in_tab` joins on the store bind, so this follows from
  the fix. | **Open** — he was asked twice and has not answered. Ask again
  before merge.

**Method.** Gates before every commit. The drive is the acceptance test and
costs two maintainer launches; `bash scripts/qa/lib-selftest.sh` is under a
second and covers every reading the drive makes.

## Current State

Tree CLEAN. **7 commits ahead of origin, NOT PUSHED.** Gates green.

- `3dc2271` the fix, with the store-level regression test that was red first
- `4e07faa` a stale line in the QA doc
- `a9331f0` phase 6c closes its own tab instead of trusting phase 3's
- `6fa9f68` + `0c2210a` the close must spare the minted row, and assert it
- `a2b03c2` the pick moves into `qa/lib.sh` under the selftest
- `4952c50` run 16 on the record

## What's Working

- **The product fix is proved twice**: red-first store test, and run 16 live.
- **Phase 6c now asserts its own witness** — "the row whose agent really ran
  is in the set to restore". That check is what would have turned runs 14 and
  15 red. Do not weaken it.
- **`close_candidate_tab` in `scripts/qa/lib.sh`**, covered offline for all
  four cases. `script_hygiene.rs` fails the build if it moves back inline.

## Next Steps

1. **Read the swarm review report.** Ten blind lanes plus an opus verifier,
   launched 2026-09-16 on `main...HEAD` with the PR body in scope. If the
   session died before the report landed, re-run it: the lane briefs are
   adapted from `~/code/corti/olympus/.claude/skills/swarm-review/SKILL.md`.
2. **Act on blockers and majors.** Minors are the maintainer's call.
3. **The PR body is stale.** It was written before these seven commits and
   describes neither the defect nor the fix. Rewrite it before the merge.
4. **Ask him for the eyeball check** on the exited-agent row.
5. **Then push, PR, merge — all on his go.**

Where work stopped: the swarm review was in flight. Before that he said
"fix the check and let's drive it again", then "launched it", and run 16 went
all green.

## Context for the Work

Guardrails unchanged and still binding: do not touch his live session; he
launches and kills every session (the one exemption is the sandbox you asked
for, once its drive is done); hooks only through `scripts/ct.sh --hook`;
remote surfaces wait for his go; `just gates` green before any commit;
ASD-STE100 in everything written to him; he does not read code, so give him
the decision and not the mechanism.

Harness friction: compound shell commands with runtime variables get refused
by the worktree-isolation classifier — split them, or use the Edit tool.

## Restart Hint

Tree clean, nothing mid-refactor, no sandbox running (run 16's was left up;
kill it by name if it is still there). The branch is mergeable the moment the
review is read and the PR body is rewritten.
