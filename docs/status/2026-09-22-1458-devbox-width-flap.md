# Task Pickup

You are picking up this work session from a prior agent that landed the frames-off width fix on `fix/bar-separator-column`, built the REMOTE QA loop (the drive running on the devbox over ssh), drove it green once (run 23), and then measured a new defect on the box: after a restore, Alt+c in the first tab asks for the collapse three times and the tab never swaps. The human is at the box and can launch; everything else is yours.

## Orientation

- Branch `fix/bar-separator-column`, 13 commits ahead of `origin/main`, tree clean, `just gates` green (host 446, bar 347, types 34). Nothing pushed; no PR. | **Checked** | `git log --oneline origin/main..HEAD; git status --short`
- The width fix (`RowHeight::mode_at`, one separator column of tolerance) is in the model AND the card renderer (`card.rs` `is_expanded`); the blind review found the renderer copy. Reviews done: Opus blind lane, CodeRabbit ×2 (0 findings). | **Checked** | `docs/status/2026-09-22-1214-devbox-width-flap.md` "Review round"
- The remote loop: `scripts/remote-qa.sh` {sync,stage,qa,drive,qa-log,qa-running,log,drive-log,instance,kill}; `just remote-qa|remote-sandbox|remote-log|remote-drive-log|remote-kill`. The box checkout is `devbox:~/code/clave-qa`, a plain repo fed by `git push HEAD:main` (updateInstead). Its sandbox is `clave-test` at `~/.local/state/clave-dev` (a MAIN checkout to `dev instance`). | **Checked** | `./scripts/remote-qa.sh instance`
- On Linux zellij's sockets are under `$XDG_RUNTIME_DIR/zellij/contract_version_1/`, the log under `/tmp/zellij-1000/zellij-log/zellij.log`. `ct.sh` follows that rule now. | **Checked** | `ssh devbox 'ps -eo args | grep "[z]ellij --server"'`
- The human's launch line must run from a Mac terminal that is NOT a zellij pane: `ssh -t devbox 'cd ~/code/clave-qa && just launch'`. From a clave tab, zellij 0.45 shows a nesting dialog. | **Checked** | this session, 14:08
- The agent's sandbox allows: ssh file reads, `git push` to the box, remote `cargo`/`just`, killing the box sandbox by name, `ct.sh` against the box SANDBOX. It refuses `scp`, and any `zellij` against the box's live `clave`/`remote` sessions. `pkill -f` over ssh must use a `[q]a-drive` pattern or it kills its own shell. | **Checked** | the denials this session
- The shipped bar logs `clave-bar: swap-width backwards=<b> cols=<n>` once per width ask (main.rs `render`). lib.sh `swap_ask_count_since` counts them per LINE for sandbox instances; phase 1 measures, phase 4 asserts zero. | **Checked** | `bash scripts/qa/lib-selftest.sh`
- Run 23 on the box (frames off): twelve phases green, 224 checks; launch 0 asks; ring walk 0 asks; burst one ask per press, cols alternating 47/15 (swaps LAND in a fresh `clave open` tab). | **Inherited** | cost two launches by the human; `./scripts/remote-qa.sh drive-log 40`
- After run 23's relaunch, Alt+c in the FIRST (baked, restored) tab: bar id 5 asks `backwards=true cols=47` three times, 2.5s apart, twice over (14:40:50, 14:45:58), never lands, rests on `WALK_ASK_CAP`. The human sees "collapses randomly, doesn't respond, hides for a fraction of a second". The dump shows every tab's bar declared `size=48` and session-level swap layouts `clave_expanded`/`clave_collapsed`. | **Open** | which seam refuses the swap. Candidate: the baked tab's pane set does not fit the swap layouts' pane count, so zellij applies nothing (or applies and reverts). Settle with the extended log line below.
- Both servers' bars beacon in lockstep (nine ids on `beacon 2`, `beacon 8` at 14:44:09-11); ids 1-5 collide across the box's `clave` and `clave-test` servers; the `DIAG ask` lines are the LIVE fleet's diagnostic bar. | **Open** | whether a nav in one fleet really reaches the other (`zellij pipe` target), or ids merely collide. Attribute by build tag only.
- The box's live fleet still runs the DIAGNOSTIC 0.5.2 bar (`.orig` beside it); the box's `clave-test` sandbox is still UP. | **Checked** | `ssh devbox 'zellij list-sessions -n'`; rollback per the 1214 file
- A local frames-off sandbox is staged at `clave-test-restore-cwd` (config from `/tmp/clave-frames-off.kdl`), never launched, built BEFORE the swap-width log line. Restage before use. | **Checked** | `./target/release/clave dev instance --field session`

**Method.** Read the box's zellij log for `swap-width` lines with instance id and `cols` before believing any description of a flap. Push, restage and drive on the box through `scripts/remote-qa.sh` only; never a bare `zellij` against the box. Write the handoff line the moment a measurement lands; the log is user-global and ids collide.

**Proof.** `just gates` green locally; `./scripts/remote-qa.sh qa qa-fleet 3600` then the human's launch, and `qa-log` ends in twelve PASS lines with phase 4's "the two walks made no width ask" at 0.

## Task Overview

Make v0.5.2's restore hold on both machines and close the devbox regressions, with the QA drive covering each seam locally AND over ssh (the human's standing requirement, verbatim in the 1214 file). Now concretely: find why a restored/baked tab on the box refuses the bar's width swap after Alt+c, fix it test-first, drive it green remotely, then cut v0.5.3.

## Reference Docs

- `docs/status/2026-09-22-1214-devbox-width-flap.md` — the whole thread: the flap's cause, the review round, the remote loop, the four other regressions (nav skipping a dead tab, restored rows without pane, daemon-held spawn, live-set loss), rollback commands. Read it in full; it is the ledger this file extends.
- `docs/dev/QA-DRIVE.md` "The remote drive" and run 23 in the ledger.
- `docs/FOOTGUNS.md` entries added today: frameless pane paints one short; ssh-shell `zellij action` lands on `remote`; worktree `.git` is a file; Linux socket dir.
- `crates/clave-bar/src/model.rs` `width_effects` (~3655-3712) and `own_tab_focused` (3270); `crates/clave-bar/src/main.rs` `render` (~1206-1240, the swap-width log line) and the `SwapWidth` arm (~419).
- `crates/clave/src/setup.rs` 1400-1612 `launch_session`: how the first tab is baked and restored rows deferred — the tab that refuses the swap is born here.

## Current State

All committed. Today's commits on the branch, oldest first: the width fix (6bf32e6), docs, the doc-comment move, the renderer fix + hardening (f2aeaca), status, the remote loop + width assertion (fe5a3fd), detached run (e6dc84a), Linux socket fix (b8f1ba4), run 23 docs (25af2e7), and three status updates (3db9e55, ac04ea7, this). `/tmp/clave-pr-separator.md` holds a drafted PR body (dossier template, review lanes filled) for when the human says push.

## What's Working

- The remote loop end to end: push, stage, detached drive, log reads, kill. Copy its shape for any future remote target (`CLAVE_QA_HOST`).
- The width assertion is a real detector: it read 0 on a healthy frames-off fleet and the fixture self-test pins per-line attribution.
- `mode_at` and `is_expanded` are the two places the painted width is judged; a compile-time assert keeps the pairs more than a separator apart. Do not add a third comparison.
- The relaunch phase (6c) passed on the box: five rows back, same uuids, four bound before their agent ran.

## What success looks like

Alt+c in every tab of a restored fleet on the box collapses and expands with one swap per press; the drive proves it (a phase that presses Alt+c in the FIRST tab after a relaunch, not only in the commit's fresh tab); v0.5.3 cut and installed on both machines; the four remaining regressions closed the same way.

## Important Discoveries

- The old flap (asks for the width the pane already has, on every focus change) is gone on the box with this branch: zero asks at launch and across the walk. What remains is different: a legitimate ask that the TAB does not honour. The drive's burst passed because it stood in tab 7, a fresh `clave open` tab; the human stood in the baked first tab. That gap is the drive's blind spot to close.
- The bar logs only when it asks, so "hides for a fraction of a second" (a swap applied and undone, or a pane rebuilt) is invisible in the log. The next log line must carry `tab=<own_tab> active=<active_tab_id> panes=<pane count of own tab>` so one round names the seam.
- Tried and abandoned: rsync of the worktree (`.git` is a file); an attached ssh drive (dies with the 10-minute tool cap); `pkill -f` without the bracket trick (kills the ssh shell).

## Next Steps

1. Extend the `swap-width` line in main.rs `render` with own tab, active tab and own-tab pane count (model already has `own_tab()`, `active_tab_id()`, `panes`). Gates.
2. `./scripts/remote-qa.sh kill`, then `./scripts/remote-qa.sh qa relaunch-restore 3600` (one launch gives a restored fleet). Hand the launch line. Ask the human to press Alt+c twice in the first tab, then in a fresh tab. Read `just remote-log 60`.
3. Fix at the seam the numbers name, test-first in model.rs or setup.rs; add a drive check that presses the toggle in the FIRST tab after 6c's relaunch and asserts the swap landed (cols moved).
4. Mutation run: revert the two fix commits on a temp branch, keep the log line, push to the box, watch phase 4 go red. One launch.
5. Ask the human for the go: push, PR from `/tmp/clave-pr-separator.md` (refresh the test counts and lanes), then v0.5.3 per the 1214 file's step 2, including the box rollback of the diagnostic bar.
6. Then the four regressions in the 1214 file's order.

Last exchange: the human sent a screenshot standing in `qa-fleet-quiet` (first tab, bar expanded) and said "tried again, still flapping. The bar collapses randomly, or doesn't respond to alt+c, or hides completely for a fraction of a second." The agent measured the same three unlanded asks and handed off.

## Context for the Work

- He launches; hand him the exact line and say it must be a Mac terminal outside zellij. He was annoyed that "any terminal" was wrong: be explicit.
- Never touch the box's `clave` or `remote` sessions, not even a read. The sandbox is `clave-test`; kill it by name only.
- Remote surfaces (push, PR, tag, issue) wait for his go each time.
- Bring log lines, not theories; he pushed back on unmeasured theories three times on this thread.
- Commits end with `Claude-Session: https://claude.ai/code/session_01VxbdvjsGZN9x4hYyGgdXHL`; PR bodies end with that URL bare.

## Restart Hint

Tree clean, gates green, box sandbox `clave-test` up and stale; kill it, then step 1.

## Suggested Skills

`catch-up` to resume; `superpowers:systematic-debugging` for the refused swap; `superpowers:test-driven-development` for the fix; `superpowers:requesting-code-review` before the PR.
