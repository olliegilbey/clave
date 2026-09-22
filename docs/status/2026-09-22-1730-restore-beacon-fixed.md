# Task Pickup

You are picking up from an agent that fixed the Alt+c flap on a restored fleet in the model, test-first, through two review rounds and one red drive, and is now waiting for the box drive (run 28) to prove the shipped shape. The human launches; you kill the sandbox at 6c and read the verdict.

## Orientation

- Branch `fix/bar-separator-column`, ahead of `origin/main`, nothing pushed, no PR. Gates green at `6cbc67e`; `scripts/remote-qa.sh` has an UNCOMMITTED fix (see below). | **Checked** | `git status -sb`, `git log --oneline origin/main..HEAD`
- The fix, final shape: `note_restore_steal` in `model.rs` (the `clave-visited` entry) arms `restore_steal_pending` when the arriving beacon names a tab an owed row holds, or a tab no row holds while an owed open is in flight; that ROW alone leaves `restore_reanchor_owed`. Any other beacon disarms it (the human walked). `apply_tabs` emits the re-anchor on `restore_steal_pending` and clears it on emit. No frame touches it. | **Checked** | `crates/clave-bar/src/model.rs` `note_restore_steal`, fields `restore_reanchor_owed` / `restore_steal_pending`, `apply_tabs` `restore_home`
- Two shapes refuted on the way, both recorded in FOOTGUNS: a rule derived from `restore_sent` alone (review: unbounded lifetime, drags the beacon off a restored tab the human walked to); a drain of every owed row on emit (box run 26: tab 3's steal answered after tab 4's open went out, tab 4's steal unanswered, three asks for two presses). | **Checked** | `docs/FOOTGUNS.md` "A claim armed by an event and spent by the next frame"
- Tests that pin it: `the_reanchor_claim_outlives_a_bound_snapshot_that_beats_the_birth_announce` (devbox order), `a_finished_restore_does_not_drag_the_beacon_off_a_tab_the_human_walked_to` (lifetime), `answering_one_steal_leaves_the_next_open_owed` (box order), `a_walk_to_a_live_tab_during_an_open_is_not_the_newborns_steal`, `a_beacon_on_a_tab_the_restore_did_not_build_is_a_walk`. `cargo mutants -F "note_restore_steal|fn restore_effects"`: 7 caught, 0 missed. | **Checked** | this session
- Drive check: phase 6c, after `relaunch_checks`, presses `clave-toggle` twice in the tab the restore left focused with NO anchor, asserts 2 asks, both `tab=Some(<standing>)`, askers non-empty, painters == askers. Green on box run 24 and Mac run 25 (on the FIRST, unbounded shape). Red on box run 26 (the drain-all shape). Run 27 died in preflight: the launch raced the stage. | **Checked** | `scripts/qa-drive.sh` "post-relaunch"; `docs/dev/QA-DRIVE.md` run ledger
- Run 27's cause: `remote-qa.sh qa` printed the launch line BEFORE the detached remote stage ran; the human launched at 17:15:22 into a half-staged sandbox and the seed at 17:15:30 deleted launch.kdl. Fixed, uncommitted: the stage now runs attached and the launch line prints after it, the drive alone is detached. | **Checked** | `scripts/remote-qa.sh` `qa)` case; box `~/.local/state/clave-dev/state/clave.log` (launch ts 1790093722, seed ts 1790093730)
- Reviews done: blind Opus adversarial review over the beacon fix (1 blocker taken, the lifetime; the rest taken or declined, all in `/tmp/clave-pr-separator.md`); CodeRabbit over the beacon commits, 0 findings. The final shape (`6cbc67e`) has NOT been re-reviewed. | **Checked** | `/tmp/clave-pr-separator.md`
- Box: sandbox `clave-test` staged for run 28 at `6cbc67e` plus the script fix, drive detached and waiting up to 3600 s. The human launches; `/tmp/watch-box-drive.sh` is the poll script. Kill only via `./scripts/remote-qa.sh kill`. The devbox still carries the DIAGNOSTIC 0.5.2 bar in its live install (rollback in the 1214 file). | **Checked** | `./scripts/remote-qa.sh qa-log 20`
- Mac: no sandbox up. Run 25 was green on the first shape; the final shape has not been driven on the Mac. | **Checked** | the human confirmed the eyeballs, the sandbox was killed

**Method.** Every claim came from a trace or a test that was red first. Extend a log line before a theory. `remote-qa.sh` and `ct.sh` only; never a bare `zellij` on the box.

**Proof.** Run 28 on the box: twelve phases green, 6c's four post-relaunch lines green, the trace showing one `swap-width` per press from the standing tab's instance and its `painted` ~30 ms later. Then a Mac drive of the same commit (two launches), or at minimum a Mac launch with Alt+c in the first tab and a fresh tab.

## Task Overview

Ship the Alt+c fix: box run 28 green, Mac drive green, commit the script fix, refresh the PR body, ask for the go on push, PR, and v0.5.3.

## Reference Docs

- `docs/status/2026-09-22-1550-restore-beacon.md` — the diagnosis and the trace that named the seam (its "nothing is fixed" is dated).
- `docs/status/2026-09-22-1214-devbox-width-flap.md` — the width fix, the release steps, the devbox diagnostic-bar rollback.
- `/tmp/clave-pr-separator.md` — the PR body, filled except for runs 26-28; ephemeral, copy it into the repo if the session ends.
- `docs/dev/QA-DRIVE.md` — run ledger (add runs 26-28), the 6c row, the remote-drive section.

## Current State

Uncommitted: `scripts/remote-qa.sh` (the stage/launch-line reorder) and this file. Box drive waiting for the human's launch.

## What's Working

The model fix under all five tests and mutants. The 6c check catches the defect (proved by run 26 going red on a wrong shape). The remote loop, now with the stage attached.

## What success looks like

Run 28 and the Mac drive green on `6cbc67e`; PR opened with the dossier; v0.5.3 cut; devbox diagnostic bar rolled back.

## Important Discoveries

- Opens are paced one per bound snapshot, but a re-anchor for steal N can emit after open N+1 is sent. Any drain must be per row.
- The Mac cannot show box-order races: its opens land slower than the re-anchor. Drive the box for anything about restore ordering.
- A launch line printed before the stage finishes is a launch into a half-staged sandbox. The fix in `remote-qa.sh` is the record.

## Next Steps

1. Run 28: the human launches; at "needs a SECOND launch" run `./scripts/remote-qa.sh kill`, ask for the relaunch, read `drive-log` for the summary and the four post-relaunch lines. If red, read the bar trace (`./scripts/remote-qa.sh log 600 | grep clave-bar:`) around the press timestamps before touching code.
2. Commit `scripts/remote-qa.sh` (fix(qa): the remote stage runs attached, the launch line follows it; cite run 27) and this file. Add runs 26-28 to the QA-DRIVE ledger.
3. Mac drive on the same commit: `nohup just qa qa-fleet 3600 > /tmp/clave-local-qa.log 2>&1 &`, the human launches, kill `clave-test-restore-cwd` at 6c, relaunch, read the summary.
4. Refresh the PR body (runs 26-28, the final shape, a note that the final shape had no second adversarial pass), then ask for the go: push, PR, v0.5.3 per the 1214 file, devbox rollback.

Last exchange: the human said "its up" for run 27, which died in preflight from the stage race; the box was restaged with the fixed script and is waiting for the launch.

## Context for the Work

- Launch line, Mac terminal outside zellij: `ssh -t devbox 'cd ~/code/clave-qa && just launch'`. Local: `cd <this worktree> && just launch`.
- The human asked that the agent kill the sandbox each time; that exemption stands for both sandboxes this conversation launched.
- Commits end with `Claude-Session: https://claude.ai/code/session_01VxbdvjsGZN9x4hYyGgdXHL`.

## Restart Hint

Check `./scripts/remote-qa.sh qa-log 20` first: if run 28 has started, follow its phase; if it has finished, read the summary. Then step 2.

## Suggested Skills

`catch-up`; `superpowers:test-driven-development` if anything goes red in the model; `superpowers:requesting-code-review` before the PR.
