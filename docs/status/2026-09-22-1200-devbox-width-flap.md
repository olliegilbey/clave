# Task Pickup

You are picking up a session that cut v0.5.2 (the anchored-transcript
restore, #267, plus the collapse-step fix #268), installed it on the Mac and
the devbox, and then measured the devbox width flap with a diagnostic bar.
The cause is measured and cited. The fix is WRITTEN on branch `fix/bar-separator-column` (off main): `RowHeight::mode_at` in clave-types, three call sites in model.rs, one new test, four tests repointed from 47 to 46, FOOTGUNS entry. Gates were being run at handoff time; not committed.

## Orientation

- v0.5.2 is tagged, released and installed on both machines. The Mac fleet is fine. | **Checked** | `gh release view v0.5.2`; `ls ~/.local/share/clave/bin/`
- The devbox runs a DIAGNOSTIC 0.5.2 bar: the release bar plus `eprintln!` lines on every width ask. The release file is beside it as `.orig`. Rollback: `ssh devbox 'cp ~/.local/share/clave/clave-bar-v0.5.2.wasm.orig ~/.local/share/clave/clave-bar-v0.5.2.wasm'` after the fleet is down (`box down`). The sandbox refuses this copy for the agent; the human runs it. | **Open** | `ssh devbox 'ls -la ~/.local/share/clave/clave-bar-v0.5.2.wasm*'`
- The flap, measured 2026-09-22 11:19 on the devbox log: every bar is born at 48 and wants 48, but the host paints it at **47** (and 15 for 16). The model compares painted width to the target EXACTLY, so it asks for a swap it does not need; the swap walks the tab through the collapsed geometry, and the walk budget resets on every focus change, which the restore causes six times in three seconds. | **Checked** | `ssh devbox 'grep "DIAG ask" /tmp/zellij-1000/zellij-log/zellij.log'` (16 asks, all `cols=47` or `cols=15`)
- Why 47: with `pane_frames false`, zellij reserves one column on any tiled pane that is not at the viewport's right edge, for the separator line, and it does so for borderless panes too. Same rule in 0.44.3 and 0.45.1 (`zellij-server/src/panes/tiled_panes/mod.rs`, `pane_content_offset` and the last `else` arm of `set_pane_frames`; copies in `/tmp/zj/tiled-v0.44.3.rs:556-612` and `/tmp/zj/tiled-v0.45.1.rs:48-70,610-700`). The devbox has `pane_frames false`, and since #262 (v0.5.1) that setting passes through into clave's generated config. So the flap began with v0.5.1, not with zellij 0.45.1. The Mac has frames on and never lost the column. | **Checked** | `ssh devbox 'grep pane_frames ~/.local/share/clave/config.kdl'`
- The fix (written, see above): the bar must judge which mode a painted width IS by the nearest declared width, allowing the one separator column, at all three comparison sites in `crates/clave-bar/src/model.rs`: the birth guess (`width_effects`, ~3653), the at-target check (~3690), and `widths_at` (~2980). One helper, one constant for the separator column with the zellij citation, red-first tests: painted 47 wanting expanded asks nothing; painted 15 wanting collapsed asks nothing; birth guess at 47 is expanded; `widths_at(15)` while awaiting hydration is COLLAPSED; a toggle from 47 still asks. | **Open** | `sed -n 3639,3712p crates/clave-bar/src/model.rs`
- The branch `diag/width-flap` in this worktree holds the diagnostic `eprintln!` lines only (8 lines, model.rs and bar main.rs). They must NOT ship. The fix goes on a fresh branch off `origin/main`. | **Checked** | `git diff --stat v0.5.2`
- Second defect seen on the devbox, not clave's: the first restored tab refuses with "Session … is running as a background session". Claude Code 2.1.278's daemon holds that conversation as a background session; clave's spawn runs `--resume` and Claude refuses. clave should attach (`claude attach <id>`) when the live session is held by the daemon. Not started. | **Open** | `ssh devbox 'ps -eo args | grep [b]g-pty-host'`
- Third gap, from the Mac: a session whose restore stalled, where the human opened one tab by hand, overwrites the recorded live set with that one tab. The guard in `store::clear_session_order` covers a session that bound NOTHING only. Not started. | **Open** | `sed -n 985,1015p crates/clave/src/store.rs`
- Fourth, the Mac: a trackpad scroll over the bar panics the plugin inside `register_plugin!` (bar main.rs:142, "NO PAYLOAD"). Unmeasured; likely a mouse event the 0.44.3 tile crate cannot decode from a 0.45.1 host. Not started. | **Open** | screenshot in this conversation only

## Next Steps

1. On `fix/bar-separator-column`: `just gates` green, then commit (`fix(bar): a painted width one short of the target is at the target`), `just mutants` over the diff, TESTING.md row.
2. FOOTGUNS entry: the separator column, with the zellij citation and the 2026-09-22 measurement. UBIQUITOUS_LANGUAGE if a term is minted. TESTING.md row.
3. Opus subagent review, then `coderabbit review --committed --base main --agent`, fix findings, PR from the template, the human's go.
4. Cut v0.5.3, install on both machines (devbox: `box down`, restore the `.orig` bar first, then the installer and `~/.cargo/bin/clave setup` over ssh, then `box`).
5. Then the background-session attach, the live-set gap, the scroll panic, in that order.

## Context for the Work

- The human launches and kills every session. On the devbox he types `box` (launch) and `box down` (kill); the `remote` session is his ssh shell and stays. The agent may not overwrite files under the devbox's `~/.local/share/clave/`; print the command.
- Bring log lines, not theories. The diagnostic bar is how this was settled.
