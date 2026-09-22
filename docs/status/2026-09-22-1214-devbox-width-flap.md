# Task Pickup

You are picking up this work session from a prior agent that cut and rolled out clave v0.5.2 (the anchored-transcript restore #267 and the collapse-step fix #268), then chased a fleet of regressions the human saw on the devbox after the cut. One of them, the width flap, is measured, cited, fixed and committed on a branch, not yet reviewed or pushed. Four more are named with evidence and not started. The human has also set a standing requirement: the automated QA drive must cover the seams these regressions crossed, locally and over ssh.

## Orientation

- The fix branch is `fix/bar-separator-column`, off `origin/main` (b403e81), three commits, tree clean, `just gates` green (446 host + 345 bar). Reviews not run. Not pushed. | **Checked** | `git log --oneline origin/main..HEAD; git status --short`
- The width flap's cause: with `pane_frames false`, zellij reserves one column of any tiled pane not at the viewport's right edge, borderless or not, for the separator line. The bar (left pane) paints at 47/15 for 48/16. The model compared exactly. Same zellij rule in 0.44.3 and 0.45.1. The devbox has frames off; the Mac has them on. It started with v0.5.1 (#262 passed `pane_frames` into clave's config), not with zellij 0.45.1. | **Inherited** | cost a diagnostic-bar launch on the devbox plus two source fetches. `/tmp/zj/tiled-v0.44.3.rs:556-612`, `/tmp/zj/tiled-v0.45.1.rs:48-70,610-700` (fetched copies; refetch from GitHub if gone). Log: `ssh devbox 'grep "DIAG ask" /tmp/zellij-1000/zellij-log/zellij.log'`
- The devbox still runs the DIAGNOSTIC 0.5.2 bar (release bar + `eprintln!` on every width ask). The release file sits beside it as `.orig`. The agent's sandbox refuses to overwrite files under the box's `~/.local/share/clave/`; the human runs the copy. | **Open** | `ssh devbox 'ls -la ~/.local/share/clave/clave-bar-v0.5.2.wasm*'`; rollback after `box down`: `ssh devbox 'cp ~/.local/share/clave/clave-bar-v0.5.2.wasm.orig ~/.local/share/clave/clave-bar-v0.5.2.wasm'`
- The diagnostic lines live only on branch `diag/width-flap` in this worktree (one WIP commit). They must not ship. | **Checked** | `git diff --stat v0.5.2 diag/width-flap -- crates`
- The human reaches the devbox with `box` (launches the fleet over ssh) and `box down` (kills it). The `remote` zellij session on the box is his ssh shell; never kill it. Both sessions run the same zellij 0.45.1 and only `clave` loads the bar. | **Checked** | `sed -n 644,690p ~/.aliases`; `ssh devbox 'zellij list-sessions'`
- Installing a release on the devbox: the installer puts the binary at `~/.cargo/bin/clave`, but the box's PATH puts the versioned launcher first. The upgrade only lands after `~/.cargo/bin/clave setup` (or one launch by full path), which rewrites config and launcher. Run it only while the fleet is down: zellij live-watches the config. | **Checked** | `ssh devbox 'ls -la ~/.local/share/clave/bin/'`
- `just release` runs from a worktree checkout of the tagged commit; the agent's sandbox allowed it once and refused it once. Tags are shared across worktrees. The gate refuses untracked paths outside `docs/` and `.claude/`; `.claude-worktrees/` is now ignored (#270). | **Checked** | `sed -n 270,300p crates/clave/src/release.rs`
- Claude Code 2.1.278 on the box has a daemon holding a conversation as a background session; `claude --resume <id>` on it prints "running as a background session" and exits. clave's spawn does not handle this. `claude attach <id>` gets it back with nothing lost. | **Checked** | `ssh devbox 'ps -eo args | grep [b]g-pty-host'`
- On the box, after the restore, Alt+Up/Down work inconsistently and skip the first (baked) tab, whose spawn died on the refusal above. The store has that row bound (tab 0, pane 14); two restored rows have a tab but no pane recorded. Hypothesis, unmeasured: nav walks rows it counts as live and a row whose pane exited does not count. | **Open** | `ssh devbox 'grep -E "nav landed|BIND STALLED" /tmp/zellij-1000/zellij-log/zellij.log | tail -20'`; store: `~/.local/state/clave/agents.json` on the box
- On the Mac, a session whose restore stalled and where the human opened one tab by hand overwrites the recorded live set with that one tab. The guard in the store covers a session that bound NOTHING only. | **Checked** | `sed -n 985,1015p crates/clave/src/store.rs`
- On the Mac, a trackpad scroll over the bar panics the plugin inside `register_plugin!` (bar main.rs:142, "NO PAYLOAD"). Likely a mouse event from a 0.45.1 host the 0.44.3 tile crate cannot decode. Unmeasured. | **Open** | reproduce in a sandbox, read `$TMPDIR/zellij-501/zellij-log/zellij.log`
- The "Action CliPipe did not complete within 1s timeout" and "dropped … pipe with empty payload" log lines are noise on every version. | **Inherited** | previous handoff, `docs/status/2026-09-21-1700-swap-width-fix.md`

**Method.** Put a diagnostic bar into the live fleet and read the log when a sandbox cannot reproduce; one launch settled what eight source diffs did not. Fetch the zellij server source from GitHub for the path a symptom names, both versions, and stop when they agree. Repoint an existing test one failing assertion at a time from its actual value.

**Proof.** `just gates` green in the worktree, and `cargo test -p clave-bar a_painted_width_one_short` passes while it fails with the exact comparison restored.

## The remote loop (added 14:35, same day)

- `just remote-qa <scenario>` pushes HEAD to `devbox:~/code/clave-qa` (a plain repo, `receive.denyCurrentBranch updateInstead`), stages there, and runs the same drive detached; `remote-qa.sh qa-log`, `just remote-log`, `just remote-drive-log` read it back; `just remote-kill` kills the box SANDBOX (`clave-test`) by name. | **Checked** | `sed -n 1,40p scripts/remote-qa.sh`; docs/dev/QA-DRIVE.md "The remote drive"
- Run 23: the first remote drive, all twelve phases green on the box, 224 checks, this branch's bar, frames off. The width assertion (phase 4, zero asks across the ring walk; `swap_ask_count_since` in lib.sh over the shipped `clave-bar: swap-width` line) passed. | **Checked** | `./scripts/remote-qa.sh drive-log 40`
- The human's launch line, EXPLICITLY from a Mac terminal window that is not a zellij pane: `ssh -t devbox 'cd ~/code/clave-qa && just launch'`. From a clave tab, zellij 0.45 shows a nesting dialog instead. | **Checked** | this session, 14:08
- The agent's sandbox permits: ssh reads of files, `git push` to the box, remote `cargo`/`just` runs, and killing the box sandbox by name. It refuses `scp` of binaries and any `zellij` command against the box's live sessions. Work through the wrapper. | **Checked** | the denials in this session
- NOT yet done: the mutation run (the drive against a bar WITHOUT the fix but WITH the swap-width log line, to watch phase 4 go red); the frames-off scenario needs no special fixture on the box, it is the box's own config. The other four regressions from the list below are untouched. | **Open** | —

## Task Overview

Make the v0.5.2 rollout hold on both machines, and close the regressions the devbox surfaced. The human's read: "the restore system is the main breakage; locally things feel good." His standing requirement (verbatim): "We need to know about where all these regressions are showing up so that our automated qa testing can pick them up, and be able to test both locally and over ssh. Even if we spin up a local ssh test environment as part of the automated qa system."

## Reference Docs

- `docs/status/2026-09-22-1200-devbox-width-flap.md` — the earlier handoff from this session; superseded by this file except its Next Steps 4-5 wording.
- `docs/status/2026-09-21-1700-swap-width-fix.md` — the collapse-step fix and the sandbox measurement loop (two-minute round), the CliPipe noise, the hot-reload trap on the box.
- `docs/status/2026-09-21-1200-anchored-transcript-restore.md` — the restore fix's design and review record.
- `docs/dev/QA-DRIVE.md` — the drive the new scenarios go into. `docs/dev/TESTING.md` — the risk taxonomy.
- `docs/FOOTGUNS.md` — new entry "A frameless pane paints one column short" (on the fix branch).
- `crates/clave-bar/src/model.rs` `width_effects` (~3639-3712) and the tests from `a_toggle_asks_on_the_first_paint…` (~7990) onward — the width machine and its contract.
- `crates/clave/src/setup.rs` 1400-1612 — `launch_session`, the restore bake and deferral; `crates/clave/src/store.rs` 985-1015 — the live-set rebuild.

## Current State

Committed on `fix/bar-separator-column`:
- `crates/clave-types/src/lib.rs`: `SEPARATOR_COLS`, `RowHeight::mode_at`.
- `crates/clave-bar/src/model.rs`: three call sites use `mode_at`; new test `a_painted_width_one_short_of_the_target_is_at_the_target`; four tests moved from 47 to `EXP_W - 2` as their impossible width.
- `docs/FOOTGUNS.md`: the entry. `docs/status/2026-09-22-1200-devbox-width-flap.md`: the earlier handoff.
- Review round (later session, same day): the blind Opus lane found the same exact threshold in the card and double-card renderers (`card.rs` `is_expanded`), which would have parked a frames-off bar at full width drawing the collapsed card. Fixed test-first (`the_card_one_column_under_expanded_is_the_expanded_card`, `the_double_card_one_column_under_expanded_keeps_its_branch`). Added a compile-time assert that each mode's two widths sit more than a separator column apart, a types-crate test pinning the tolerance as one-sided, and rewrote the two model.rs doc comments that still called the comparison an equality. CodeRabbit: zero findings on both passes. Declined: dropping `const` from `mode_at` (the assert above now needs it const) and splitting the new model.rs test (it reads as one scenario).

Merged to main today: #267 (restore), #269 (0.5.2 bump), #270 (gitignore). Tag v0.5.2 pushed, release published, installed on the Mac and the box.

## What's Working

- The anchored-transcript restore works on both machines: a six-tab devbox fleet came up in three seconds with two anchored rows resumed from their birth dir.
- The width fix is small and pure: one helper on `RowHeight`, three comparisons. The whole width machine contract is unchanged; the tests from `a_toggle_asks…` onward are the safety net and all pass.
- The diagnostic-bar loop on the devbox: build `clave-bar` with `eprintln!` lines, scp to `/tmp` on the box, the human copies it over the versioned bar after `box down`, `box`, read `/tmp/zellij-1000/zellij-log/zellij.log`. One round is two minutes.
- The review loop that held today: Opus subagent (blind, given file paths and questions), then `coderabbit review --committed --base main --agent`, then Codex on the PR. Every lane found a real defect on #267.
- The Mac fleet on 0.5.2 is good by the human's word.

## What success looks like

Both fleets restore cold and stay at their width, nav walks every tab, and a drive in `just qa` reproduces each of today's seams locally and over loopback ssh so the next regression is caught before a cut.

## Important Discoveries

- The devbox flap was never a zellij-version issue and never reproduced in sandboxes because every Mac sandbox has frames on. The "Mac flapped with frames on" note in the previous handoff is unexplained and may have been a different, since-fixed cause (#268); the Mac is fine on 0.5.2.
- `pane_frames false` costs the bar one column. The fix tolerates exactly one; two short is still "neither width" and the walk logic is untouched.
- A restore that stalls, followed by one hand-opened tab, is treated as a real session and shrinks the live set to that tab. That is why the Mac needed two relaunches after the 0.5.1 failures. Rows deferred but never bound should stay in the set.
- The launch bakes the first tab from the store's collapse flag at that moment; the devbox store said collapsed at launch and expanded a minute later (a toggle, or a re-assert) — not root-caused, and moot once the width fix lands.

## Next Steps

1. Reviews on `fix/bar-separator-column`: an Opus subagent briefed with `crates/clave-types/src/lib.rs` `mode_at`, the three model.rs sites and the question "can a one-column tolerance make the walk settle at the wrong mode?"; then CodeRabbit; fix; ask the human for the go; PR from `.github/PULL_REQUEST_TEMPLATE.md` via `--body-file`, body ending with the session URL.
2. Cut v0.5.3 (bump commit via PR as #269 was, tag, `just release` from the tagged checkout, launch, tag push last). Devbox: `box down`, restore the `.orig` bar, installer, `~/.cargo/bin/clave setup`, `box`.
3. The QA work the human asked for: a devbox-shaped sandbox (frames off in the sandbox config; six-plus tabs; nav after restore; one row whose spawn dies on purpose; one held by the daemon), runnable locally and against loopback ssh. Put each scenario at the seam its regression crossed.
4. Fix, in order: nav skipping a dead-pane tab; restored rows missing their pane registration; spawn attaching to a daemon-held session (`claude attach`); the live-set loss after a stalled restore; the scroll panic on the Mac.

Last exchange: the human said "Okay, so the Alt+up and Alt+down do somewhat work, but seriously inconsistent behaviour, and it skips over the first tab opened. I have a feeling the restore system is the main breakage. But locally, things feel good." The agent offered two starts, the review lanes on the width fix or the drive first, and the human had not chosen.

## Context for the Work

- The human launches and kills every session. On the box: `box` / `box down`. Never touch `remote`. Never run zellij against his live sessions; the log file is readable.
- The agent may not write under the box's `~/.local/share/clave/`; print the command. `clave setup` over ssh has precedent and was run this session.
- Remote surfaces (push, PR, tag, issue) wait for his go. He gave "push it and open the PR" per PR today; do not assume it carries over.
- Never `clave spawn` or `clave hook` by hand; `scripts/ct.sh --hook` only.
- He does not read the code. Give the decision, what it cost, why. Short sentences. Bring log lines, not theories; he pushed back on unmeasured theories twice on this thread.
- Commits end with `Claude-Session: https://claude.ai/code/session_01VxbdvjsGZN9x4hYyGgdXHL`; PR bodies end with that URL bare.

## Restart Hint

Tree clean on `fix/bar-separator-column`, gates green; start with the review lanes unless the human says drive first. The devbox still carries the diagnostic bar.

## Suggested Skills

`catch-up` to resume; `superpowers:systematic-debugging` for the nav and register defects; `superpowers:test-driven-development` for every fix; `superpowers:requesting-code-review` before the PR.
