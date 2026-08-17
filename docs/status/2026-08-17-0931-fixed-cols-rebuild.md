# Status — width machine rebuilt on fixed columns; live drive owed on relaunch

_Follows @docs/status/2026-08-16-0300-width-stillness-fix.md. Worktree
`qa-182-drive-slice-1`, branch `push-197-fixes` (mirrored to
`origin/redesign-181-width`; last push at 59b5e71 — the two new commits
below are NOT pushed yet)._

## Task Overview

Drive checklist 10a–10c on the #197 branch. The drive instead found the root
cause of the whole width-regression family, and Ollie approved a redesign,
which is now BUILT and COMMITTED (cc6089d, 7b7f97e), gates green. What remains
is the live validation and the merge train.

## Current State

Tree is CLEAN. Two commits on top of 59b5e71:

- `7b7f97e` — FOOTGUNS + C6-ledger entries for the finding.
- `cc6089d` — the rebuild (−1372/+526 across 14 files): model.rs width
  machine, main.rs plumbing, setup.rs/add.rs/open.rs/main.rs generators and
  CLI, clave-types constants, kdl_guardrail.rs, checklist §10.

**The finding (verified live, instrumented):** zellij computes
`active_swap_layout_name` only for tabs with ≥2 SELECTABLE panes
(`zellij-server-0.44.3/src/tab/mod.rs:1042` — "no layout for single pane").
Every clave tab (unselectable bar + one workspace pane) reads `None` forever,
so #197's reported-truth machine was blind in the entire product: one switch
per mode flip, cycle walked one step per press, every third toggle lost, drag
snap-back arm unreachable. Proof: a `new-pane` split made names appear next
frame and the machine self-heal.

**The design (Ollie's, arrived at via his "battery never desyncs" push):**
both geometries are FIXED COLUMN counts (`size=54` / `size=30`, bare KDL
numbers) in all three generators. The machine
(`crates/clave-bar/src/model.rs` `width_effects`) is one equality: painted
`cols` vs `clave_types::target_cols_for(mode)`; mismatch → one `SwapWidth`
per paint; spin guard `frozen: Option<(cols, want, u8)>` caps at
`FROZEN_ASK_CAP = 3` consecutive asks while the width is frozen (cycle is 3
long), unbounded while it moves, reset by width or mode change. Fixed panes
also make the bar border UN-DRAGGABLE (zellij refuses resizes touching a
fixed pane, both sides) — the 2026-08-15 snap-back ruling enforced at the
source. Deleted: TabGeometry/name reading, stillness gate, settled_w,
snap_asked/swap_asked, both drag arms, `--display-cols`/percent plumbing
end-to-end (incl. `terminal_size` dep, `BAR_BIRTH_PERCENT`,
`REFERENCE_VIEWPORT_COLS`).

## What's Working

- `just gates` green on the committed tree: 173 clave-bar + 229 clave tests,
  wasm builds, clippy clean.
- The new width tests (model.rs, `a_toggle_asks_on_the_first_paint…` through
  `snapshot_heals…`) encode the design: no-move landings re-ask, frozen cap,
  guard resets, stale-paint flap converges, peeks, cold starts owe zero
  switches. Copy their shape for new cases.
- The percent-era mandate is reversed knowingly: layout APPLICATION applies
  fixed sizes exactly (C8 proved fixed births); only the deleted resize
  engine refused them. D34's lattice problem was resize-only.
- `scripts/ct.sh pipe --name clave-toggle -- 'x'` = exactly one press (blank
  twin dropped); widths read via `ct.sh dump-layout` (will show `size=54` /
  `size=30` under the new build). ct.sh wraps `zellij --session … action` —
  bounded, no stdin streaming.
- Instrumentation recipe works and was stripped post-use: `width_debug()` on
  the model + eprintln in render/pipe/TabUpdate, rebuild wasm,
  `ct.sh start-or-reload-plugin "file:$SB_DATA/clave-bar.wasm" -c
  clave_binary=clave` (the `-c` is load-bearing, FOOTGUNS), read
  `/var/folders/dd/kvqk7tfx70l6pmbmt512sm340000gn/T/zellij-501/zellij-log/zellij.log`.

## Important Discoveries

- Idle sessions starve render-dependent logic (FOOTGUNS 62) — the stillness
  gate starved in the sandbox; the new machine asks on the FIRST mismatching
  paint precisely for this.
- A `rename-tab` to the same name fires a TabUpdate (used as a no-op event
  probe).
- Sandbox staging REFUSES while its session lives (config re-key would spawn
  a second bar, #44) — kill first, and the kill is Ollie's.
- A plugin reload can NOT change a session's swap layouts — fixed-cols
  validation requires a fresh launch.

## Next Steps

1. **FIRST ACTION, before anything else: Ollie has ALREADY KILLED the
   sandbox session.** Run `just sandbox` (staging is the agent's; it will
   now succeed) and hand him the launch command it prints, verbatim — he is
   waiting to run it. Everything else comes after he says it's up.
2. **Drive checklist §10 (rewritten this session)**: 10a cold start (born at
   30, NO switch at all now), 10b six slow + six fast toggles via ct.sh
   pipes (expect: every press moves, no lost third press — dump-layout
   between presses), 10c = Ollie mouse-drags the border: must be IMMOBILE;
   record whether zellij flashes "FIXED!" and how it looks (the one open
   unknown). Then `Alt+c` still moves in one press.
3. Push to `origin/redesign-181-width`, then the merge train for #197
   (update-branch first).
4. Still queued: #190 (Alt+f), PR #200 review (qa-drive slice 2), v0.1.3
   tag + release, worktree cleanup.

Where work stopped — staging refused (expected, session live):

> FAILED: the clave-test-qa-182-d-1d57 session is live, and regenerating
> config.kdl under it would re-key its keybinds … Kill it first, in a
> non-zellij terminal:
>   zellij kill-session clave-test-qa-182-d-1d57 && zellij delete-session --force clave-test-qa-182-d-1d57

Ollie's approval of the design, verbatim:

> Okay, yes, that's the kiss mechanism, and is the straightforward build.
> Let's go with that.

And the insight that led there, verbatim:

> How do we decide when the battery is an icon, versus showing it's numeric
> token count? If that toggles just fine, which it does, can't we use that
> logic everywhere as the is_collapsed vs is_expanded understanding?

## Context to Preserve

- Ollie rejected the intermediate proposal (remembered/settled widths behind
  a stillness gate) as "too overkill, nondeterministic, and far from a KISS
  design" — do not resurrect learned widths; constants only.
- Accepted trades, stated to Ollie: border permanently non-draggable; on a
  narrow SSH window the expanded bar still takes 54 cols (v0.1.2 feel). A
  window that cannot produce the target gets 3 asks then rests ("wherever
  cols stop changing", round-20 ruling).
- The old sandbox session (if still up at read time) runs the PERCENT build
  with the name-blind machine — its behavior is known-broken; don't debug it.
- Sandbox session name `clave-test-qa-182-d-1d57`; root
  `~/.local/state/clave-dev-qa-182-d-1d57`; declared widths now equal cols
  (54/30), not percents (28%/16%).

## Restart Hint

Tree clean, gates green, commits unpushed, sandbox session already killed by
Ollie. Resume = run `just sandbox`, print him the launch command, wait for
"it's up", then drive §10 via ct.sh.
