# QA-DRIVE.md — the automated regression drive (ratified 2026-08-12)

_Design 2026-08-12, built on [qa/BREAKAGE-INVENTORY.md](qa/BREAKAGE-INVENTORY.md)
(101 classes), ratified in the 2026-08-12 grill (spec: #182). One idea: every
escape has lived at a seam — process, env, event ordering, screen — so the
drive tests seams, not logic. Unit tests keep owning the model; this drive
owns everything they structurally cannot reach._

## Run ledger — the useful recent history

- **run 30, 2026-09-22 — box, phase 2 green after the add-order fix; 6c red
  again.** Three asks for two presses: the owner bar never emitted the
  re-anchor, so the beacon sat on the last tab built and the first tab's bar
  answered a press it did not own. Decision: the live-set restore is removed
  (commit `9670fbf`); 6c now verifies one eager tab and a dormant fleet.

- **runs 26 to 29, 2026-09-22 — four reds, three of them the drive's own,
  and a host race the box lost every time.** Run 26 (box) went red in 6c on
  the second shape of the beacon fix: three asks for two presses, because a
  drain of every owed row on emit answered tab 3's steal after tab 4's open
  had gone out (FOOTGUNS, "A claim armed by an event"). Run 27 (box) died
  in preflight: the launch line printed before the detached stage had run,
  and the seed deleted the launch.kdl eight seconds after the launch. The
  stage now runs attached and the launch line follows it. Runs 28 and 29
  (box) went red in phase 2 rung 1: `clave add` created the tab first and
  recorded the row after, the newborn's `clave spawn` registered its pane
  id into a store with no such row, and `pane_id` stayed null for the life
  of the row. The host log shows it in order: `spawn: Create` then `add:
  recorded`, same second. The Mac won that race in every run and never
  showed it. The record now precedes the open (`add.rs`
  `record_then_open`). The Mac drive of the same commit passed phase 2
  and went red in 5b on a fixture collision: a sibling sandbox's leftover
  transcript shares the scenario uuid, sorts first in the drive's glob, and
  had an unclosed launch from run 25 inside the six-hour bound. That one is
  the Mac's, not the code's; the box has no sibling sandboxes.

- **runs 24 and 25, 2026-09-22 — the Alt+c flap on a restored fleet, caught
  by a new 6c check, fixed, green on both machines.** Run 24 on the devbox
  (232 checks) and run 25 on the Mac (233 checks), 0 failures each. After
  the relaunch verdict, 6c now presses the toggle twice in the tab the
  restore left focused, with NO beacon anchor, and asserts one width ask per
  press from that tab's bar, painted by that bar. Before the fix the beacon
  ended every restore on the LAST tab built: the first tab's bar asked
  nothing, the last tab's bar asked, and zellij applied each ask to the
  first tab. Phase 5 never saw it because it pipes the beacon before it
  presses. The trace on both hosts: one `swap-width` line per press from
  the standing tab's instance, its `painted` line about 30 ms later, no
  other instance asking.

- **run 23, 2026-09-22 — the FIRST REMOTE DRIVE, on the devbox, twelve
  phases green, 224 checks, 0 failures.** `just remote-qa qa-fleet` from a
  Mac worktree; the box has `pane_frames false` and zellij 0.45.1, which no
  Mac sandbox has. The new width assertion held: zero asks at launch, zero
  across both ring-walk legs over six live tabs, one ask per press in the
  collapse burst (sixteen for eighteen presses, alternating direction, one
  instance). The first attempt died in phase 1 because `ct.sh` looked for
  the sockets under `/tmp`; on Linux they are under `$XDG_RUNTIME_DIR`
  (FOOTGUNS). The launch must come from a Mac terminal that is NOT a
  zellij pane: zellij 0.45 detects the outer session through the terminal
  and offers a nesting dialog instead of the fleet.

- **run 22, 2026-09-17 — TWELVE phases green, 237 checks, 0 failures.** The
  first fully green drive on `worktree-live-set-restore` (#261), and the first
  in which `restored rows were bound before their agent ran (tab, no pane)`
  passed: 4 of 4. Runs 19-21 found three separate live defects the unit suite
  could not see, all one shape — one sidebar runs per tab, and every guard had
  been tested against a single model. Run 19 killed the zellij server with
  "Too many open files". Run 21 measured 2 of 4 awake. The fix was to stop
  inferring which tab drives the restore and have the launch name it.
- **Two phase-6c runs timed out** waiting 30 minutes for the second launch,
  costing a pair of launches each. A standalone verdict script existed for
  exactly that: it ran 6c's verdict on its own against a sandbox still in the
  pre-quit state (deleted with the restore, 2026-09-22).
- **A fixture that stages nothing passes.** `dev scenario relaunch-restore`
  had been silently staging no restore at all since `bound_since_launch`
  landed. `clave dev scenario` now prints how many rows the next launch will
  bring back, and refuses a relaunch fixture whose answer is wrong.

## Shape

One script, `scripts/qa-drive.sh <scenario>` — plus the instrument it reads
with, `scripts/qa/lib.sh` — driven **by an agent** against the per-worktree
sandbox (`clave dev instance`), every zellij touch through
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
- **The instrument is a file of its own**: `scripts/qa/lib.sh` holds the
  tracing helpers, the assertions and every zellij-log reading; `qa-drive.sh`
  is the phases. The split is what makes the log parsing testable at all —
  `scripts/qa/lib-selftest.sh` runs it against a fixture log in under a
  second, gated by `crates/clave/tests/qa_lib.rs`, instead of needing a
  maintainer-launched session to find out whether a `sed` expression was
  right.
- **Read the log per BAR INSTANCE, not per line.** zellij stamps every plugin
  line with `[id: N]`, and until 2026-09-12 nothing used it: every reading was
  a count of lines, which cannot tell one busy instance from five quiet ones.
  That is exactly why the terminal-facts flicker was invisible — one bar of
  five had the facts and the line count said the pipeline worked. Ids are
  attributed to this sandbox by intersecting them with the instances that
  logged a build-tagged `loaded` line, because the log is user-global and most
  bar lines carry no tag.
- **Every run prints the instance ledger**: one row per sandbox bar, naming
  the capabilities its own log shows it exercising. Asserted nowhere — it is
  the first thing to read when a phase goes red, and it distinguishes a
  fleet-wide behaviour from one instance's.
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
| 5b | **Card cells (hook side)** | eight real `clave hook` events against the sandbox store: a permission `Notification`, `UserPromptSubmit`, five `Stop`s (on a tail carrying a fresh `Agent` launch, on a background-command notification, on the agent's own notification, on a launch stamped seven hours ago, and on a second fresh launch), then `SessionEnd` | `wants` arrives from the notification's own words and leaves with the next prompt; the subagent mark rises on a fan-out, ignores a background command finishing beside it, clears on the notification naming its own `tool-use-id`, is never raised by a launch past its age bound, and is forced down by `SessionEnd` **from a raised mark**; status follows each event; ≤1 store write per event, counted AFTER a PR-cache warm-up (`seq` is not a per-event counter — a stale cache makes the hook spawn `pr-sync`, a second writer landing off-schedule; FOOTGUNS). Fires every event through `ct.sh --hook`, never `clave hook` directly, and appends only to a `c85c` scenario transcript, guarded | #232's card cells — the seam three of its defects lived at |
| 5d | **The meter seam (statusLine side)** — SPECIFIED, NOT YET SCRIPTED | three `clave statusline` runs against the sandbox store, fed the captured payload shapes on stdin: one while the row is `Working`, one after a permission `Notification` has turned it red carrying the SAME token count, one carrying a HIGHER count | the red survives the repeated count and clears on the moved one (`statusline::apply_statusline`, 2026-09-14); the cleared row reads `Working` in the shared store within a bounded wait; ≤1 store write per run, counted after the PR-cache warm-up 5b already does. Runs with the drive's scrubbed identity like every other leg — `run_statusline` PUSHES a snapshot, so an unscrubbed run aims at the maintainer's fleet exactly as a hand-written `clave hook` does (FOOTGUNS #281) | the meter seam — phase 5b drives the HOOK side only, and the statusLine is the other half of the card's live cells |
| 5c | **Terminal facts (OS side)** | a real `sleep` typed into a plain shell tab, behind a shell allowlist, while that tab is focused; then focus moves to an agent tab with the command still running | leg A: at least one sandbox bar learns the change — the OS-facts pipeline (`get_pane_cwd`/`get_pane_running_command` → `apply_pane_facts` → the terminal row) delivers end to end, which nothing tested before. Leg B MEASURES whether a second instance learns it from another tab, the open question under the store-backed fix, and asserts nothing: the facts are per-instance today, so "every bar agrees" is not yet true and a drive must not go red on a known-open defect | the 2026-09-12 flicker (a terminal row with facts under one tab and none under another), #206, #239 |
| 6 | Quiescence | idle 60s | evlog and store `seq` flat; zellij log flat after the mark for sandbox-attributable lines only (the shared log is never globally flat with a live maintainer fleet — see Delivery accounting) | P17, B19/B20, drive step 6 |
| 6b | **Isolation witness** | nothing (reads this run's own evidence) | zero `push-refused` events across the run — no push was aimed at a bar that does not own this store; the ambient zellij identity is STILL the sandbox's at the END of the run, not just at the start; the inherited session's name appears nowhere as a push target | FOOTGUNS #281, the 2026-09-11 incident |
| 6c | **Relaunch (the second launch)** | quit the sandbox session, then ask the maintainer to `just launch` it again | exactly ONE row is bound after the relaunch — the most-recent row, the eager tab the launch bakes — and every other row is dormant: no `tab_id`, no `pane_id`, and no stale `Working` or `NeedsYou` carried over from the first session. Then the beacon leg: two `clave-toggle` presses in the eager tab, with NO anchor pipe first. Each press is ONE width ask, from that tab's bar, painted by that bar | the relaunch seam (TESTING.md's escape record); the one-eager-tab rule, commit `9670fbf` |
| 7 | Teardown | nothing | prints the kill pair (the agent may run it once both eyeballs are in) | drive step 9 |

**Why 5b could not catch the 2026-09-14 red-glyph defect.** Its event
sequence is `Notification` → `UserPromptSubmit` → `Stop` → `SessionEnd`, and
`UserPromptSubmit` clears `NeedsYou` on its own. The bug lives in the gap the
drive never visits: a permission answered with NO further hook, where only the
statusLine can speak. A phase that only drives the events clave listens to
cannot find a state the listened-to events cannot reach. Hence 5d.

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

1. **`just qa qa-fleet`** — one command for steps 1–3 below: it stages the
   per-worktree instance, prints the launch line, waits up to ten minutes for
   the human to run it, then drives the full spine the moment the session is
   up. Prefer this; the three-step form below is what it does, kept because a
   drive that needs an already-launched session still uses step 3 alone.
   1. `just sandbox qa-fleet` (per-worktree instance), take the log mark.
   2. Hand the launch line to the human (`cd <worktree> && just launch`);
      wait. The agent CANNOT do this: `clave dev launch` refuses when
      `ZELLIJ` is set, and an agent is always inside a session.
   3. `scripts/qa-drive.sh qa-fleet` — the full spine, phases 0–7 (with 5b,
      5c, 6b and 6c in between); stop on first failure.
   **The run needs the maintainer TWICE**: once for the first launch, and
   again at phase 6c, which asks for a quit and a relaunch and waits for
   both (`QA_RELAUNCH_WAIT`, default 1800s each half). Tell him that when you
   hand over the first launch line, so the second ask is expected rather
   than a surprise mid-run.
   **Full 0–7 driven live green: run 4, 2026-08-17**, both eyeball
   checkpoints confirmed; and **run 11, 2026-09-11**, the ten labels that
   existed then (5b and 6b were the new ones), on the first run of the
   one-command loop. The drive carries **twelve** now: 5c joined on
   2026-09-12 and 6c on 2026-09-16. **Run 12, 2026-09-16** drove all twelve:
   phases 0–6b green, and 6c went red on its first complete run, on a real
   defect — the restore of the day (since removed) lost the one row whose
   agent was genuinely running. That is the phase doing its job on
   the first attempt at a class nothing else could see. **Run 16, 2026-09-16,
   is the first ALL TWELVE green**, with the `SessionEnd` fix in and phase 6c
   closing its own tab. Runs 13–15 sit between them and are worth reading as
   a set: 13 proved the fix and went red on a stale assertion, 14 and 15 each
   went green-adjacent on a fleet with no witness in it — the phase now
   asserts the witness is there, which is what ended that.
   **Runs 17 and 18, 2026-09-16**, followed the second swarm review. 17 went
   red at phase 5b on the DRIVE, not the product: the phase ends a session two
   lines above the check and then asserted `idle`, which under the restore's
   `Exited` status (since removed) was the reading for an agent that is still alive. 18 is green in all twelve,
   and is the run that proves both swarm-review blocker fixes live — five rows
   recorded, five rebound by the bar with nothing driven, focused or typed,
   four of them bound before their agent ran, and the tab closed in the first
   session absent from the second. Runs 1–3 each went red on one real finding (all
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
   time: kill, then `just qa <scenario>` and let the maintainer launch into
   its wait. (Re-seeding alone is not a shortcut: `clave dev scenario` calls
   `run_setup`, so it regenerates config.kdl — the #44 hazard that refuses a
   live session in the first place.)

   Phase 1 now NAMES a stale stage rather than failing a count that is
   correctly reading the previous run: it reports the bound-row count, the
   residue count and the two commands that fix it, then continues so the
   shape-independent phases still report. A count read on a stale stage is
   not evidence — the 2026-09-11 "second writer in phase 5b" was six store
   writes for five events on a stale stage and four on a fresh one, the
   extras being `pr-sync` writes from the previous run's rows.
5. Report the per-phase table with measured values; request the two
   eyeballs; hand back the kill pair.

**Phase 6c needs two maintainer launches, and that is the point.** Every other
phase runs inside one session, which is exactly why the live-set decay (#261)
reached a shipped branch with all gates green and a full drive behind it. The
phase is cheap — quit, relaunch, count — and it is the only automated look at
state that crosses a session boundary.

What it asserts (2026-09-22, after the live-set restore was removed, commit
`9670fbf`): a relaunch bakes ONE tab, for the most-recent row. So after the
second launch exactly one row is bound, every other row has no `tab_id` and no
`pane_id`, and no row carries a `Working` or `NeedsYou` from the first
session. Then two `clave-toggle` presses in that one tab, each answered by
exactly one width ask from that tab's bar. Both halves read the store and the
log; nothing is driven between the relaunch and the reading — no focus, no
nav, no keystroke — so the reading is the launch's own work.

Two things about how it is built, each of which a later edit could undo
without any test noticing:

- **It kills nothing and launches nothing.** It prints the pair and waits for
  liveness to drop and return. Session lifecycle stays the maintainer's, and
  `script_hygiene.rs` fails the build for any line in the drive that starts
  or ends a session.
- **The readings are non-vacuous because a launch CLEARS the binds** before
  it bakes the layout (`setup.rs` `clear_session_order`). So every tab id read
  after the relaunch was made by the second session. If that changes, this
  phase is measuring nothing.

The verdict lives in `scripts/qa/lib.sh` (`relaunch_checks`), not in the
phase, so the selftest can run it against a store with two bound rows and
require it to go RED.

## The remote drive (a second machine, over ssh)

Ratified 2026-09-22, after the v0.5.2 rollout: the devbox showed a fleet of
regressions the Mac never did — the width flap needed `pane_frames false`,
which the box has and the Mac does not; the restore defects needed the
box's Claude Code daemon. Every one was found by the human at the box and
diagnosed by an agent reading the box's log over ssh. So that is now the
loop, in one command:

```
just remote-qa qa-fleet          # push HEAD, stage on the box, wait, drive
just remote-log 60               # the box's zellij log tail
just remote-drive-log 60         # the newest drive log on the box
just remote-kill                 # the box SANDBOX, by exact name
```

`scripts/remote-qa.sh` is the whole mechanism. It pushes this HEAD to a
plain git checkout on the remote (`~/code/clave-qa` by default; the repo
accepts a push onto its checked-out branch), then runs the SAME `just qa`
there, in the remote's own environment — its zellij config, its frames
setting, its Claude Code. There is no second drive and no remote-only
phase: one code path. The remote sandbox is `clave dev instance` on the
remote, and the remote's live fleet is untouched for the same reasons the
local one is. `CLAVE_QA_HOST` and `CLAVE_QA_DIR` pick the machine.

The human's one job is unchanged: launch. The line is

```
ssh -t devbox 'cd ~/code/clave-qa && just launch'
```

and it works from ANY terminal, inside zellij or not, because ssh forwards
none of the zellij variables that make `just launch` refuse. Phase 6c's
second launch is the same line after the quit.

**Reading a remote sandbox is not touching the remote fleet.** The zellij
log is a file (`/tmp/zellij-<uid>/zellij-log/zellij.log` on Linux), the
drive log is a file under the remote sandbox state dir, and the store is a
file. `just remote-log` and `just remote-drive-log` read those and nothing
else. A `zellij action` typed into an ssh shell on the box aims at whatever
session that shell sits in — the human's `remote` session — so nothing here
runs one outside `ct.sh`, which the drive already routes through.

**The frames seam has an assertion now.** The shipped bar logs one line per
width ask (`clave-bar: swap-width backwards=… cols=…`, main.rs `render`).
Phase 1 records the launch's ask count per sandbox instance; phase 4 asserts
ZERO asks across both ring-walk legs, because a walk toggles nothing and a
bar at its painted width asks nothing — the flap asked on every focus
change. The reading is per LINE (`swap_ask_count_since`, lib.sh), not per
instance: a flap is one instance asking sixteen times, and an instance
count reads that as 1.

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
5. Phase 6c (the relaunch). LANDED (2026-09-16, #261). Driven live on run 12
   the same day: the settle window held for a six-tab fleet, and the phase
   went RED on a real defect in the restore of the day. The restore itself
   was removed on 2026-09-22 (commit `9670fbf`); 6c now verifies one eager
   tab and a dormant fleet.
