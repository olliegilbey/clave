# Status — width regressions diagnosed and fixed; landed as 8324175; one live drive owed

_Follows @docs/status/2026-08-15-1400-v013-release-drive.md. Worktree
`qa-182-drive-slice-1`, branch `push-197-fixes` (local pointer pushed to
`origin/redesign-181-width`)._

## What happened overnight

Ollie live-drove the #197 branch (tip 5a79b24, which included this session's
first snap-back cut) and hit two regressions: the bar's text/width thrashed
("couldn't decide where it should be") and a dragged border did not return.
He went to bed instructing: proceed alone, test everything, check v0.1.2.

## Root causes — both found, one fix

1. **The thrash was the first snap-back cut's own race** (commit 5a79b24):
   it adopted the FIRST agreed-name frame's width as the geometry's truth,
   but a toggle's pane resize lands renders AFTER its TabUpdate, so it
   adopted the OLD width, read the real width as a drag, and spent switches
   on an UNDAMAGED tab — which ADVANCE the cycle. Reproduced in a unit test
   (`a_toggles_late_pane_resize_is_not_a_drag`) that failed before the fix.
2. **"Drag doesn't return" was the same wedge**, not a false mechanism:
   verified LIVE on the sandbox (bar mangled to 5%; one clave-toggle pipe
   landed it exactly on the declared 16% collapsed percent; second pipe back
   to 28% expanded). The damaged-tab re-apply is real — D40/D41's owed live
   run is CLOSED. zellij-server 0.44.3 source (fetched to /tmp, not vendored)
   confirms: `swap_layouts.rs` skips the advance when damaged; the layout
   applier reassigns every pane's position and size.

## The fix (commit 8324175, pushed to origin/redesign-181-width)

v0.1.2's drift-confirmation gate, ported (sonnet archaeology of the old seek:
it never acted on a width seen once; that's why stable never thrashed and
snapped on release). In `snap_effects`: a reading counts only when STILL
(same width, two consecutive renders). Adopt a still width per geometry
(refusing the other geometry's width — the transitional poison is
structurally unreachable); a still deviation spends one switch; a spent-on
width still standing two more renders is CONCEDED (window-resize face,
self-healing — no stale memory). Fields: `still`, `settled_w: [Option; 2]`,
`snap_asked` in model.rs.

**Verification:** 177 clave-bar + 231 clave tests green; `just gates` green;
scoped mutants on `snap_effects`: 17/17 caught, 1 unviable. Checklist 10c and
LEDGER D41 amended to the stillness design (snap on release; held drag stays
quiet; window resize = one dead ask then concede).

## Blocked / owed

- ~~1Password-blocked commit~~ — LANDED as 8324175 and pushed (the signing
  wrapper's contract, for next time: a plain `git commit` that times out
  mints a one-shot fallback token; the retry must run with the
  `GIT_*_NAME=Claude GIT_*_EMAIL=noreply@anthropic.com` identity, one mint
  per commit).
- **The LIVE sandbox session (if still up) runs the thrashy 5a79b24 build.**
  The FIXED wasm and CLI are already staged on disk (data dir +
  `target/release/clave`), so a plain kill + relaunch with the printed
  `dev launch` command picks up the fix — no restage needed. Kill is
  Ollie's.
- **Live drive owed:** checklist 10a–10c on the fixed build (six slow Alt+c;
  drag-release snaps back on its own; held drag stays quiet). Then the
  scripted phase 0–2 drive, then merge train for #197 (update-branch first).
- Still open from yesterday: #190 (Ollie's Alt+f), PR #200 review/merge
  (qa-drive slice 2), v0.1.3 tag + release, worktree cleanup.

## Context to preserve

- Sandbox toggles ARE scriptable now (blank-twin discard landed): one
  `scripts/ct.sh pipe --name clave-toggle -- 'x'` = one press. Widths
  readable via `scripts/ct.sh dump-layout` pane size percents (good as a
  delta oracle; declared percents: expanded 28%, collapsed 16% at this
  window).
- zellij-server source lives at /tmp/zellij-server-0.44.3 (re-fetch:
  crates.io download URL); worth vendoring a note in FOOTGUNS if used again.
