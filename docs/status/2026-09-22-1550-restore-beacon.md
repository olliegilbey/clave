# Task Pickup

You are picking up from an agent that found, by measurement on the devbox, why Alt+c "flaps" in a restored fleet: after the restore, the replicated focus beacon rests on the LAST tab the restore opened, while the human stands on the FIRST. The bar in the last tab believes it is focused and asks for the swap; zellij applies every ask to the tab that is really focused (the first), whose own bar believes it is unfocused and stays silent. Nothing is fixed yet. The human is at the box and can launch.

## Orientation

- Branch `fix/bar-separator-column`, 19 commits ahead of `origin/main`, tree clean after this file, `just gates` green. Nothing pushed, no PR. | **Checked** | `git log --oneline origin/main..HEAD`
- The trace that names the seam (box log, launch 15:46:35, presses 15:47:04-08): bar 4 (tab 3) `swap-width backwards=true cols=47 tab=Some(3) active=Some(3)`; 2 ms later bar 1 (tab 0) `painted cols=16 was=Some(47)`; bar 4's cooldown asks twice more, bar 1 paints 48 then 47; bar 4 rests `capped`. Bar 1 logs `width-deaf cols=47 reason=unfocused tab=Some(0) active=Some(0)` throughout. Dump at +8 s: tab 0 `focus=true`, all four restored panes `start_suspended true`. | **Checked** | `ssh devbox 'grep "clave-bar:" /tmp/zellij-1000/zellij-log/zellij.log | grep 15:47:0'`; `/tmp/launch-dump.txt` on the box
- `own_tab_focused()` is `own_tab() == current_tab`, and `current_tab` is the BEACON (`set_beacon`, the only writer, `model.rs` ~1046), fed by `clave-visited` pipes. It is not zellij's active flag. A bar whose frames say `active=Some(own)` can still read "unfocused". | **Checked** | `crates/clave-bar/src/model.rs:3285`, `:1046`
- Why the beacon is wrong: each deferred restore open runs `zellij action new-tab --layout` (born focused, zellij rule), the newborn bar announces its birth (`None → own`, ungated, `apply_tabs` ~2400), then `open.rs` returns focus with `go-to-tab-by-id` (~225) and announces nothing. The reanchor claim built for this (`restore_reanchor_owed`, #261's third trigger, `apply_tabs` ~2385-2415) is spent early: the focus-return frame reaches tab 0's bar while the beacon still names tab 0 and `opening` is empty between sequential opens, so the claim is dropped before the last newborn's announce arrives. No later frame re-derives it. | **Inferred from code + trace** | the spend branch is `if self.opening.is_empty() { self.restore_reanchor_owed = false }`; confirm with a log line on the spend before fixing
- Refuted this session, by measurement: pane count of the restored tab (2, same as fresh); the plugin's client id (every bar loads under client 1); the tab's swap layouts (a CLI `previous-swap-layout` lands on the refusing tab both ways). Do not revisit. | **Checked** | log lines at 15:09, 15:22, 15:14
- A hot-reload of all bars (`ct.sh start-or-reload-plugin "file:$d/clave-bar.wasm" -c clave_binary=clave,row_height=card`, ONE `-c` with commas, or zellij starts a stray second bar) reset the beacons and every tab then toggled correctly, one ask per press, landing in ~10 ms. The defect is beacon state, not tab state. | **Checked** | trace 15:33-15:35
- Shipped instruments on the branch: `swap-width` line carries tab/active/panes/client; the cooldown's asks are logged (`source=cooldown`, they were silent before and the QA counter now counts them too); `painted cols=N was=M` on every width change; `width-deaf cols= reason=` once per (width, reason) via `BarModel::width_deaf_reason`. Noise to tidy: `reason=owed` prints in the same render as the ask. | **Checked** | `crates/clave-bar/src/main.rs` render and Timer arms
- Probes: `scripts/qa/width-probe.sh <frag> <go-to-tab idx> <since>` (two toggles, samples one tab's bar width every 150 ms); `scripts/qa/width-probe-cli.sh` (two CLI swaps). Run on the box as `~/code/clave-qa/scripts/qa/width-probe.sh restored-a 2 HH:MM:SS`. | **Checked** | this session
- Box state: sandbox `clave-test` is UP from the 15:46 launch, stale. Kill it by name (`./scripts/remote-qa.sh kill`) before restaging. The box checkout is at this branch's head minus this file. The live `clave` and `remote` sessions are untouched. | **Checked** | `ssh devbox 'zellij list-sessions -n'`

**Method.** Every claim above came from a log line or a dump; theories cost the human three launches before the instruments caught it. Extend a log line before you extend a theory. Use `remote-qa.sh` and `ct.sh` only; never a bare `zellij` on the box.

**Proof.** `just gates`; then on the box: kill, `stage relaunch-restore`, the human's launch, Alt+c ×4 in the FIRST tab: one `swap-width` line per press from bar 1 (tab 0), each followed by bar 1's own `painted` line, no `reason=unfocused` from bar 1, no asks from bar 4. Then the same in a fresh tab. Then `qa qa-fleet` green with phase 4 at zero asks.

## Task Overview

Make Alt+c hold on a restored fleet on both machines: the beacon must name the tab the human stands on when the restore finishes. Fix test-first in `model.rs` (the model is pure; the trace gives the exact event order to replay in a test), add a drive check that presses the toggle in the FIRST tab after 6c's relaunch and asserts the swap landed on THAT tab, drive green remotely, then v0.5.3.

## Reference Docs

- `docs/status/2026-09-22-1458-devbox-width-flap.md` and the 1214 file — the width fix, the remote loop, the four other regressions, rollback of the box's diagnostic bar.
- `crates/clave-bar/src/model.rs`: `apply_tabs` ~2330-2425 (birth announce, reanchor gate, the early spend), `set_beacon` ~1032-1051, `width_effects` ~3657, `width_deaf_reason` after it, `restore_effects` ~2000.
- `crates/clave/src/open.rs` ~214-240 (the silent focus return).
- `docs/FOOTGUNS.md` ~58-66: TabUpdate reaches only the active tab; `is_active_instance` is not a visibility gate; the beacon is the only trustworthy signal.

## Current State

All committed. Sandbox on the box up and stale.

## What's Working

Every swap the bars ask for lands where zellij's focus is. The fresh-tab path, the reload path, the width tolerance, the remote loop, the probes.

## What success looks like

The proof above, on the box and locally, with the drive asserting it.

## Important Discoveries

- Candidate fixes, weakest to strongest; decide after reading `apply_tabs` with the trace beside it: (a) `open.rs` pipes `clave-visited` for `restore_to` after the focus return — host-side, one line, but the newborn's birth announce is asynchronous and can still land after it; (b) keep `restore_reanchor_owed` alive until the restore is COMPLETE (the owner knows the deferred list), and re-run the reanchor when a `clave-visited` pipe moves the beacon off a tab whose fresh frame says it is active; (c) a restored HELD newborn does not announce its birth at all, since the host takes focus back at once — simplest, KISS-preferred, but check `run_held_effect` (#261 starts the agent on a beacon MOVE) and the 2026-09-17 note in the spend branch before choosing.
- The cooldown re-asks whenever the paint has not arrived within 0.15-0.2 s. When the ask went to another tab, that is a guaranteed extra walk step per press. Any fix must leave one ask per press in the trace.

## Next Steps

1. Read `apply_tabs` ~2385-2415 with the 15:47 trace; add a log line where `restore_reanchor_owed` is spent; relaunch once to confirm the early spend (or replay the event order in a model test and skip the launch).
2. Fix (test-first, `model.rs`), gates, `just mutants` over the change.
3. Drive check: after phase 6c's relaunch, `pipe --name clave-toggle -- 1` in the first tab and assert the first tab's bar logged the ask AND the paint.
4. `remote-qa.sh kill`, `qa qa-fleet 3600`, the human's launch; then the local drive.
5. Ask for the go: push, PR from `/tmp/clave-pr-separator.md` (refresh), v0.5.3 per the 1214 file, box diagnostic-bar rollback.

Last exchange: the human launched, waited, pressed Alt+c four times in the first tab, and said "done. still flapping." The trace above is that run.

## Context for the Work

- Launch line, Mac terminal outside zellij: `ssh -t devbox 'cd ~/code/clave-qa && just launch'`. Say "outside zellij" every time.
- He has launched four times today for this; bring the fix and the drive check in one launch.
- Remote surfaces wait for his go. Commits end with `Claude-Session: https://claude.ai/code/session_01VxbdvjsGZN9x4hYyGgdXHL`.

## Restart Hint

Tree clean, gates green. Start at step 1 with the trace open.

## Suggested Skills

`catch-up`; `superpowers:test-driven-development` for the fix; `superpowers:requesting-code-review` before the PR.
