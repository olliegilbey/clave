# QA-DRIVE.md — the automated regression drive (ratified 2026-08-12)

_Design 2026-08-12, built on [qa/BREAKAGE-INVENTORY.md](qa/BREAKAGE-INVENTORY.md)
(101 classes), ratified in the 2026-08-12 grill (spec: #182). One idea: every
escape has lived at a seam — process, env, event ordering, screen — so the
drive tests seams, not logic. Unit tests keep owning the model; this drive
owns everything they structurally cannot reach._

## Shape

One script, `scripts/qa-drive.sh <scenario>`, driven **by an agent** against
the per-worktree sandbox (`clave dev instance`), every zellij touch through
`scripts/ct.sh` (fail-closed — Z14). The human launches; the agent stages,
drives, joins, and reports; two eyeball checkpoints go to the human; once
both are in, the agent may kill the sandbox it asked for (TESTING.md's
lifecycle exemption) or print the pair and leave it to the human.
This is the drive loop (TESTING.md) made executable — its nine steps are the
skeleton, phases below are the flesh.

**One command runs the whole loop:** `just qa <scenario>` stages, prints the
launch line, blocks until the session appears, then drives. Launching is still
the human's and nothing in it starts a session — what it removes is the round
trips, which used to be three messages wide (stage, ask, be told, drive) and
every one of them a chance to drive something that is not the sandbox.

### The isolation contract

A drive shell runs INSIDE the maintainer's fleet, so it inherits his
`ZELLIJ_SESSION_NAME` — and anything that reads it aims there. Three
mechanisms, because on 2026-09-11 a convention alone let a drive hang his
live session (FOOTGUNS #281):

1. **The drive scrubs the ambient identity once, before its first phase.**
   `ZELLIJ`/`ZELLIJ_PANE_ID` unset, `ZELLIJ_SESSION_NAME` re-pointed at the
   sandbox, store dirs exported. Every child inherits the sandbox whether or
   not it was routed through `ct.sh`. A tripwire refuses the run if the
   identity is not the sandbox's at that point.
2. **`clave hook` refuses a push whose target does not own the store it
   wrote** (`hook.rs::aim_push`), in both directions, logging `push-refused`.
   The env is the caller's claim; the store is the fact. This is the layer
   that holds for callers that do not exist yet.
3. **`crates/clave/tests/script_hygiene.rs` fails the build** if any line in
   the drive fires a hook outside `ct.sh --hook`, or if the scrub stops
   preceding the first phase.

Phase 6b then asserts zero refusals across the run. That count is the only
evidence of isolation obtainable without touching the session it is proving
was not touched — looking IS touching, and nothing goes that way, not even a
read.

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
| 0 | Preflight | nothing | build tag on the loaded tail; the launch-baked ROW GEOMETRY (one `row_height` across launch.kdl and config.kdl, equal to the store's; exactly two declared bar widths; the card's ratified 16/48 when that is the mode) — a stale stage renders one geometry into another's pane; config+launch coherence (`clave_versions`/`clave_unversioned` from the runbook, scripted); permission cache seeded both key forms; no orphan `zellij pipe` processes | V5, V11, V12, V14, K7, P7 |
| 1 | Baseline join | `dev status` + guarded dump | row and dormant counts match the scenario seed; the eager-launch row's `tab_id` BOUND and its resumed identity == `live_session` (the #178 resume face); measured viewport geometry recorded in the log; store↔layout join printed with unresolvables MARKED, not filtered; store `seq` recorded | Z10, P9's resume face, drive-loop step 4 |
| 2 | **Bind ladder (mixed paths)** | ~6 binds through mixed paths: dormant wakes via nav pipes (`{"row":N}` pick + commit) plus ≥1 scripted create | after EACH bind, within a bounded wait: `tab_id` bound in store (bind is the proxy for row class); dormant count decremented in the shared store (`dev status` — per-instance snapshots are not observable from outside); EOF-twin delta recorded (unattributable in the shared log — see Delivery accounting); seek-trace resting width == target only when the build carries the seek instrumentation, else an honest NOTE (the shipped bar has no emitter; model belief, NOT pane truth — the eyeball stays the oracle). First discriminator: is the bind budget spent-and-never-refilled after bind 2 (#178's fleet signature)? | **P9 (#178)**, **B22 (#181 detection)**, P10, P11, P14, R1 |
| 3 | Tab churn | close a NON-last tab; close the HIGHEST tab then create one; re-join after each | nav answers with exactly one focus change; no `bind-evict` in evlog; no stale binds; recycled id carries no inherited stamp | B14, B15 (#55), Z15, P4, P12/P13 |
| 4 | Ring walk | pick into the dormant block, walk both directions, wrap; Alt+Enter one commit | single executor (one focus change per press, never two); walk stays in-block; commit opens exactly one tab | P1 (#162), P2, P16, K8 — becomes single-ring on #179 |
| 5 | Collapse burst | 12× toggle with pauses, then 5× rapid, then 1 more | store writes per press ≤ 2; every paced press lands its store flip within a bounded wait; the rapid burst settles at parity (per-instance snapshots are not observable from outside — the store flag plus the phase-5 eyeball stand in for them); bar still answers press 18; then the WIDTH probe — the bar's pane size read from the layout dump in both settled modes, asserted only if the two readings DIFFER (a number that moves with the toggle is the applied swap position; one that does not is the declared layout, and asserting on it would be a tautology — the eyeball stays the oracle then) | B6–B9, B10/B11, P5, the #232 band collision |
| 5b | **Card cells (hook side)** | five real `clave hook` events against the sandbox store: a permission `Notification`, `UserPromptSubmit`, two `Stop`s (one with a `turn_duration` tail carrying `pendingBackgroundAgentCount`, one silent), `SessionEnd` | `wants` arrives from the notification's own words and leaves with the next prompt; the subagent mark rises on the closing record, HOLDS on a silent tail, and clears on `SessionEnd`; status follows each event; ≤1 store write per event, counted AFTER a PR-cache warm-up (`seq` is not a per-event counter — a stale cache makes the hook spawn `pr-sync`, a second writer landing off-schedule; FOOTGUNS). Fires every event through `ct.sh --hook`, never `clave hook` directly, and appends only to a `c85c` scenario transcript, guarded | #232's card cells — the seam two of its defects lived at |
| 6 | Quiescence | idle 60s | evlog and store `seq` flat; zellij log flat after the mark for sandbox-attributable lines only (the shared log is never globally flat with a live maintainer fleet — see Delivery accounting) | P17, B19/B20, drive step 6 |
| 6b | **Isolation witness** | nothing (reads this run's own evidence) | zero `push-refused` events across the run — no push was aimed at a bar that does not own this store; the ambient zellij identity is STILL the sandbox's at the END of the run, not just at the start; the inherited session's name appears nowhere as a push target | FOOTGUNS #281, the 2026-09-11 incident |
| 7 | Teardown | nothing | prints the kill pair (the agent may run it once both eyeballs are in) | drive step 9 |

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
records the geometry each run actually got. S17 (#180 → #226) is BACK IN the
drive: the adoption path shipped (PR #227, loop drive-validated 2026-08-24),
so its assertion — registration within 10s of an out-of-band resume — is
expected green. One trap when driving it: re-export the sandbox shim in the
pane's shell before launching claude by hand, or the hook silently measures
the stable binary (FOOTGUNS, 2026-08-24).

## Agent protocol (the runbook for a drive session)

1. `just sandbox qa-fleet` (per-worktree instance), take the log mark.
2. Hand the launch line to the human; wait.
3. `scripts/qa-drive.sh qa-fleet` — the full spine, phases 0–7 (with 5b and 6b in between); stop on
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
   **A re-run needs a fresh stage, not just a re-invocation.** The drive is
   not idempotent by design: phase 2 rung 1 is a scripted `clave add`, so
   every run MINTS a row and leaves tabs behind it. Phase 1's seed-count
   check therefore only holds against a freshly seeded store, and a second
   run against the same session goes red on the previous run's own residue —
   which reads as a finding and is not one. The cycle is the full one each
   time: kill, `just sandbox <scenario>`, the maintainer launches, drive.
   (Re-seeding alone is not a shortcut: `clave dev scenario` calls
   `run_setup`, so it regenerates config.kdl — the #44 hazard that refuses a
   live session in the first place.)
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
