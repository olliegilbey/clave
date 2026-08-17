# QA-DRIVE.md — the automated regression drive (ratified 2026-08-12)

_Design 2026-08-12, built on [qa/BREAKAGE-INVENTORY.md](qa/BREAKAGE-INVENTORY.md)
(101 classes), ratified in the 2026-08-12 grill (spec: #182). One idea: every
escape has lived at a seam — process, env, event ordering, screen — so the
drive tests seams, not logic. Unit tests keep owning the model; this drive
owns everything they structurally cannot reach._

## Shape

One script, `scripts/qa-drive.sh <scenario>`, driven **by an agent** against
the per-worktree sandbox (`clave dev instance`), every zellij touch through
`scripts/ct.sh` (fail-closed — Z14). The human launches and kills; the agent
stages, drives, joins, and reports; two eyeball checkpoints go to the human.
This is the drive loop (TESTING.md) made executable — its nine steps are the
skeleton, phases below are the flesh.

Non-goals, deliberately (KISS): no CI integration (that is #47's real-zellij
harness, later), no screenshot automation (width truth stays human — the
known-liar detectors), no automatic bisecting.

## Tracing spec

- **One drive log per run**: `<state-dir>/qa/drive-<ts>.log`. Every ct.sh
  call, every assertion, every measured value goes through `phase()` /
  `check()` helpers that prefix `[phase-name ts]`. Output is NEVER
  discarded (`>/dev/null` is the documented trap).
- **Log mark before launch** (runbook Step 3 mechanism), so every zellij-log
  read is "lines after the mark", filtered by build tag on the tail.
- **Assertions print what they measured**, pass or fail — "empty" is written
  as the word, so a silent failure and a clean pass never look alike.
- Delivery accounting: expected EOF-twin deltas are computed per phase
  (pipes-sent × live-instances) and RECORDED alongside the measured delta —
  never asserted. The zellij log is user-global and its truncated source
  column cannot attribute a `clave-bar: dropped` line to a session (the
  stable and sandbox wasm paths collapse to the same 25-char prefix), so a
  live maintainer fleet pollutes every delta. First red run proved it:
  rung 1 measured 10, all of it main-session traffic.

## The phase spine

Each phase names the inventory classes it regression-covers. A phase FAILS
loudly and stops the run; later phases assume earlier truth.

| # | Phase | Drives | Asserts | Covers |
|---|---|---|---|---|
| 0 | Preflight | nothing | build tag on the loaded tail; config+launch coherence (`clave_versions`/`clave_unversioned` from the runbook, scripted); permission cache seeded both key forms; no orphan `zellij pipe` processes | V5, V11, V12, V14, K7, P7 |
| 1 | Baseline join | `dev status` + guarded dump | row and dormant counts match the scenario seed; the eager-launch row's `tab_id` BOUND and its resumed identity == `live_session` (the #178 resume face); measured viewport geometry recorded in the log; store↔layout join printed with unresolvables MARKED, not filtered; store `seq` recorded | Z10, P9's resume face, drive-loop step 4 |
| 2 | **Bind ladder (mixed paths)** | ~6 binds through mixed paths: dormant wakes via nav pipes (`{"row":N}` pick + commit) plus ≥1 scripted create | after EACH bind, within a bounded wait: `tab_id` bound in store (bind is the proxy for row class); dormant count decremented in the shared store (`dev status` — per-instance snapshots are not observable from outside); EOF-twin delta recorded (unattributable in the shared log — see Delivery accounting); seek-trace resting width == target only when the build carries the seek instrumentation, else an honest NOTE (the shipped bar has no emitter; model belief, NOT pane truth — the eyeball stays the oracle). First discriminator: is the bind budget spent-and-never-refilled after bind 2 (#178's fleet signature)? | **P9 (#178)**, **B22 (#181 detection)**, P10, P11, P14, R1 |
| 3 | Tab churn | close a NON-last tab; close the HIGHEST tab then create one; re-join after each | nav answers with exactly one focus change; no `bind-evict` in evlog; no stale binds; recycled id carries no inherited stamp | B14, B15 (#55), Z15, P4, P12/P13 |
| 4 | Ring walk | pick into the dormant block, walk both directions, wrap; Alt+Enter one commit | single executor (one focus change per press, never two); walk stays in-block; commit opens exactly one tab | P1 (#162), P2, P16, K8 — becomes single-ring on #179 |
| 5 | Collapse burst | 12× toggle with pauses, then 5× rapid, then 1 more | store writes per press ≤ 2; every paced press lands its store flip within a bounded wait; the rapid burst settles at parity (per-instance snapshots are not observable from outside — the store flag plus the phase-5 eyeball stand in for them); bar still answers press 18 | B6–B9, B10/B11, P5 |
| 6 | Quiescence | idle 60s | evlog and store `seq` flat; zellij log flat after the mark for sandbox-attributable lines only (the shared log is never globally flat with a live maintainer fleet — see Delivery accounting) | P17, B19/B20, drive step 6 |
| 7 | Teardown | nothing | prints the kill pair for the human | drive step 9 |

**Eyeball checkpoints** (human, one message each): after phase 2 — one bar
per tab, woken rows show agent chips not terminal glyphs; after phase 5 —
every tab a strip (or every tab wide), no width outliers. These stay human
because every automated width/screen probe is a known liar.

**Instance counting and the #178 gap** (settled 2026-08-15, #186): the
sandbox fleet has the same topology as the real one — one bar per tab; every
tab-creating path bakes the bar in (`setup.rs` tab template, `add.rs`
one-shot layout). Never count instances via `list-panes` — the bar is
non-selectable and invisible to it (the lone plugin it does list is zellij's
own background `zellij:link`). The honest counter is fresh `clave-bar:
loaded` lines in the zellij log since the mark. Phase 2 therefore has the
right *structure* to catch #178's class but did not reproduce it because the
sandbox lacks the field's load-latency aggravator (no MCP servers or LSPs
slowing the newborn pane; the bar loads in under a second and the bind
lands) — a timing gap, not a coverage lie.

## Scenario requirement

Phases 2–4 need a fleet the current `c8-*` scenarios don't seed: **new
scenario `qa-fleet`** — 6 dormant rows (one worktree, one stale-cwd
[worktree-backed, per the shared-repo deletion caveat], one rotated
`live_session`, three plain). Seeded like all scenarios: real transcripts,
deterministic `c85c` uuids. The rotated row is FAITHFUL: a second minted
`c85c` uuid with a second real transcript on disk, so `resume_target`'s
jsonl-exists gate prefers the live session id instead of silently falling
through. No viewport-clip requirement — host window size is not
programmable, and viewport behaviour is the `tall` scenario's job; phase 1
records the geometry each run actually got. S17 (#180) stays OUT of the
drive until a re-adoption path exists — a release gate must not carry a
known-red.

## Agent protocol (the runbook for a drive session)

1. `just sandbox qa-fleet` (per-worktree instance), take the log mark.
2. Hand the launch line to the human; wait.
3. `scripts/qa-drive.sh qa-fleet` — the full spine, phases 0–7; stop on
   first failure.
   **Full 0–7 driven live green: run 4, 2026-08-17**, both eyeball
   checkpoints confirmed. Runs 1–3 each went red on one real finding (all
   fixed and recorded in FOOTGUNS.md); the script header's ledger records
   how each once-pending assumption settled. Still awaiting a first live
   run: the CONCURRENT burst shape (ledger (6) — runs 1–4 drove the burst
   serially because the CLI pipe blocks). A first red on a new shape is
   evidence to read, not a script to patch until it greens.
4. On failure: capture the drive log tail + the joins BEFORE any teardown,
   grep FOOTGUNS, then debug systematically. A failed phase is evidence,
   not an excuse to re-run until green.
5. Report the per-phase table with measured values; request the two
   eyeballs; hand back the kill pair.

## When it runs

- Before every release cut — the runbook's QA-drive gate, which sits after
  Part A and before the tag because it needs a maintainer-launched sandbox
  session and Part A is unattended by definition.
- After any change classed pipe-delivery / zellij-truth / spawn / identity
  in TESTING.md's risk taxonomy.
- On demand when a field sighting matches an inventory class.

## Build order (each lands separately, gates green)

1. `qa-fleet` scenario + phase 0–2 (**catches #178's class** — build this
   first, and it doubles as #178's reproduction harness). LANDED (#182).
2. Phases 3–4 (churn + ring). LANDED (#200).
3. Phases 5–7 (collapse + quiescence + the teardown hand-back). LANDED
   (2026-08-17, the pre-release drive). **Full 0–7 driven live green on run 4
   (2026-08-17) plus both eyeball checkpoints; runs 1–3 each went red on a
   real finding first (nav wedge, newborn-bind prune, jq `//` vs `false` —
   see FOOTGUNS).**
4. Runbook/TESTING integration line + retire the duplicated manual steps.
