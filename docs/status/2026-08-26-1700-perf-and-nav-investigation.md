# 2026-08-26 — fleet lag, spawn hang, nav death: three defects, all captured live

Investigation session, no code changes (two FOOTGUNS.md entries added). Fleet ran
v0.2.1 wasm throughout; #225/#227 merged but unreleased. Implementation deferred
until in-flight work lands on main, then a v0.2.2 cut.

## 1. Spawn hang + all-TERM flash: newborn probe storm

Alt+a → new hangs for seconds while the new tab's bar renders every row TERM.
Captured at 09:44:38 (tab 21, instance 25): the newborn bar hydrates agents only
after `clave snapshot` returns, and until then `probe_targets` (model.rs:1781)
qualifies EVERY tab — `agent_in_tab` is None for all of them — so the visible
newborn fires 2 blocking OS queries × N tabs, serialized on the plugin-exec
thread that must also deliver its own hydration result. Five full probe passes
in 3.1s, one `GetPaneCwd` timeout (100ms each, zellij_exports.rs:4308). The
probing delays the hydration that would shrink the target list; self-healing
once the snapshot sneaks through and claude registers (~9s in the incident).

Steady-state variant: a pane whose queries time out while `running: true` stays
latched and is re-probed forever (plugin 19 vs pane 29 for 4+ hours on 08-25;
pane 48 still doing it on 08-26). #189 closed only the exited-pane door.

**Fix (agreed direction):** gate probes on hydration (`awaiting_hydration`
already exists, cleared in `apply_snapshot` model.rs:1321) + per-pane failure
cooldown so a timed-out query can't requalify on the next manifest. Both
model-side, unit-testable.

## 2. Nav death (alt+up/down dead until mouse click): close-stranded beacon

Captured 16:53–16:55. Timeline:
- 16:53:41 "Bye from plugin 30" (tab close) → instances 25, 5, 24 each log
  BIND STALLED pairs (tab/pane frames disagree — the close race).
- 16:53:49 last successful visited announce.
- 90s of keybind presses refused silently (footgun 108 discriminator: no focus
  change, no log line — payload landed, `nav_executor` election refused).
- 16:55:21 mouse click emits the ungated announce, re-seeds the beacon, nav
  returns instantly.

This is the #162 family (beacon stranded by tab close) in a variant that
survives #162's reanchor-debt fix: no bar paid the re-anchor for 90 seconds.
Payloads aren't logged so the beacon's named tab is unproven, but the
close→stall→refusal→click-cure chain is complete. Needs a model-level repro:
close the beacon-holding (or adjacent) tab under frame incoherence, assert the
next delivered frame on ANY bar re-anchors.

## 3. Unbounded hook push: still spinning in the field

16:50:01: "Client sent over 1000 consecutive unknown messages … logging client
out" — footgun 112's orphaned `zellij pipe`, 7th occurrence since 08-21
(5× on 08-21, 1× 08-24, 1× 08-26). `push_snapshot`'s pipe still has no
timeout. Bound it (process-level timeout on the `zellij pipe` child).

## Ruled out / noise calibrated

- `Action CliPipe did not complete within 1s timeout` is logged INSTANTLY for
  every CLI pipe on zellij 0.44.3 (route.rs:1562 drops the completion sender;
  :74 folds RecvError into the timeout arm). 3,182 in the log, 2 per pipe
  (payload + #45 blank twin). Not a stall. Now in FOOTGUNS.md.
- Memory/kernel_task episode was machine-level, not clave: 18GB machine at
  ~46GB logical (15.4/16GB swap, 19.5B decompressions). Biggest holders: 15
  rust-analyzers (one per claude session — no supported cap exists, only
  `claude plugin disable rust-analyzer-lsp@claude-plugins-official`; see
  claude-plugins-official#3417), Zen browser (4.8GB/56 procs, incl. one
  runaway Notion tab at 42h53m CPU), a live call (avconferenced+VTEncoder).
  Clave's own processes trivial (zellij server 15MB, 0.8% CPU). But the fleet
  pattern (N × claude × rust-analyzer) is the structural multiplier, and swap
  thrash amplifies defect 1 and 2's latencies.

## Implementation order for the round

1. Probe gate + failure cooldown (smallest, kills the spawn hang).
2. Bounded push_snapshot pipe (small, kills the spinner).
3. Beacon re-anchor after close (needs the model repro first).
4. Cut v0.2.2 so #225/#227 + these reach the daily driver; cold restart per
   footgun 140.
