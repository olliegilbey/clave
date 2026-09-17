# Task Pickup

You are picking up this work session from a prior agent on
`worktree-live-set-restore` (PR #261). The QA automation for the relaunch seam
is BUILT, pushed, and green. Its first complete live run found a REAL DEFECT in
the restore feature. Your job is to fix that defect and get the branch live.

Everything about how the drive phase was built is in
`docs/status/2026-09-16-1242-live-set-restore.md`. Read that only if you need
to change the phase itself. This file is about the fix.

## Orientation

- **THE DEFECT: a relaunch drops every row whose agent was RUNNING at the
  quit.** | Measured on QA run 12 (2026-09-16): six rows bound before the
  quit, five recorded into `last_live`, five restored. The missing row is the
  only one whose claude had really started. | **Checked** — the run log is
  `~/.local/state/clave-dev-live-set-459d/state/qa/drive-1789557833.log`,
  phase `P6c-relaunch`; and `.../state/clave.log` shows the next launch with
  `baked=[` five uuids `]`.
- **The suspect: `apply_hook_pane`'s `SessionEnd` arm clears `tab_id` as well
  as `pane_id`.** | `crates/clave/src/hook.rs:1261-1270`. Its comment says
  "Exit reverts the tab to a terminal tab". | **Open** — read the code, not yet
  reproduced. Settle it with the store-level test in Next Steps 1 BEFORE
  changing anything.
- **`last_live` is derived at LAUNCH from whatever binds survive.** |
  `crates/clave/src/store.rs:903-947` (`clear_session_order`) collects rows
  with a `tab_id`, writes `last_live`, then clears every bind. | **Checked** —
  so any unbind before the next launch silently shrinks the restore set. This
  derivation is the second half of the defect, and the reason a fix can go on
  either side.
- **The two events the code conflates are "the tab CLOSED" and "the agent
  EXITED".** | A closed tab must unbind — that is `clave prune-tabs`
  (`store.rs:708` onward) and phase 3 of the drive covers it. An agent exiting
  leaves the tab open and the row still living in it. | **Checked** by reading
  both paths. The fix lives at this distinction.
- **The bar joins a restored tab to its row from the PANE COMMAND (`clave
  spawn <uuid>`), not from the store's `tab_id`.** | `clave-bar/src/model.rs`
  `spawn_uuid` (~line 76) and its comment about restored tabs coming back as
  terminal rows. | **Open** — this suggests keeping `tab_id` on `SessionEnd`
  may not break the terminal-row display the comment defends, but you must
  confirm what the bar actually renders terminal-ness from before relying on
  it.
- **Why the sandbox lost ONE row where the field would lose the set.** | The
  seeded rows' claude sits at the "Yes, I trust this folder" prompt and never
  starts a session, so no hook ever fires for them. The minted row ran in the
  real worktree, which claude already trusts. | **Checked** — the human's
  screenshot of the restored session shows that prompt. Treat the drive's
  reading as a FLOOR on the defect, not its size.
- **This is why the 2026-09-15 live verification looked green.** | Those four
  restored tabs were held rows whose agents had never started (tab, no pane),
  and the `SessionEnd` unbind is gated on the row OWNING the pane — so it
  could not touch them. | **Checked** against `hook.rs:1263`. The verification
  tested the one population the defect cannot reach.
- **The acceptance test already exists and is currently RED.** | Phase 6c of
  `scripts/qa-drive.sh`. | `just qa qa-fleet` — but it needs the human to
  launch twice, so do not reach for it until the fix is in.
- **The sandbox `clave-test-live-set-459d` is LIVE**, holding run 12's
  restored session and all its evidence. | The human launched it and confirmed
  it. | `./target/release/clave dev status | head -c 200`. Read the drive log
  before you kill it; killing it is yours when the evidence is spent.

**Method.**

Reproduce at the store level before you touch the fix — the whole chain is
three pure functions and needs no zellij.
Read both `prune-tabs` and the `SessionEnd` arm before choosing a side; the
defect is that they answer the same question differently.
Run `bash scripts/qa/lib-selftest.sh` after any change to the drive
instrument; it is under a second and needs no session.

**Proof.** `just gates` — fmt, tests, wasm build, clippy, all silent. The
feature's proof is phase 6c going green on a live `just qa qa-fleet`, which
costs the human two launches — spend it once, at the end.

## Task Overview

Quit clave, relaunch, get your tabs back. The feature shipped on this branch
and was believed done; the drive now shows it restores everything EXCEPT the
tabs that were being worked in.

Success: a human quits a fleet of N tabs with N running agents, relaunches,
and gets N back. Phase 6c asserts exactly this.

## Reference Docs

- `crates/clave/src/hook.rs:1245-1275` — `apply_hook_pane`, the `SessionEnd`
  arm, and the ownership gate. The suspect, with its own rationale.
- `crates/clave/src/store.rs:903-947` — `clear_session_order`: records
  `last_live` from surviving binds, then clears them.
- `crates/clave/src/store.rs:708-730` — `apply_register` and the head of
  `prune-tabs`: the other unbind, the one that is correct.
- `docs/dev/QA-DRIVE.md` — the phase 6c row in the spine table (what the
  phase asserts) and the section under "Phase 6c needs two maintainer
  launches" (the two facts it depends on; if either changes it measures
  nothing).
- `docs/status/2026-09-16-1242-live-set-restore.md` — the drive build, if you
  need to change the phase.

No known doc inaccuracies outstanding.

## Current State

Working tree CLEAN, everything pushed, gates green. Five commits this session:

- `3c93878` phase 6c, its verdict in `qa/lib.sh`, the guard that stops the
  drive ever killing or launching a session
- `37f4ace` NOT RUN as a third phase verdict, plus the incomplete exit code
- `85046ab` half-hour ask budget; QA run 12 on the record
- `cf59ba4` the handoff naming the defect

Nothing about the DEFECT is fixed. No code in `crates/` changed this session.

## What's Working

Build ON these.

- **The acceptance test is written and it is honest.** Phase 6c found this
  defect on its first complete run, after four gates, a swarm review,
  CodeRabbit and a full drive missed it. Do not weaken it to make the branch
  green.
- **`relaunch_checks` and the readers in `scripts/qa/lib.sh`** are covered
  offline by `scripts/qa/lib-selftest.sh`, including a store that decayed. Any
  new reading belongs there, not inline in the drive.
- **`clave_types::sort_live_block`, `live_key`, `is_clave_binary`** are the
  single homes for ranking and binary matching. Do not mint a second copy —
  that mistake has been made three times on this feature.
- **The restore machinery itself is sound.** Run 12 restored exactly the set
  it recorded, in rank order, rebound without a human touching a tab. The
  defect is upstream of it: the set recorded was already short.
- **Run 12's evidence is preserved** in the sandbox state dir — drive log,
  store, and `clave.log`.

What this does NOT cover: phase 6c reads the SET, not geometry, and asserts
nothing about what a restored agent does once woken.

## What success looks like

A quit that loses nothing. The row a human was working in must survive its
own agent exiting, because the tab is still there and the row still belongs to
it. A tab that genuinely closes must still unbind — that is `prune-tabs`, and
phase 3 of the drive guards it.

Then: phase 6c green on a live drive, and the branch merged.

## Important Discoveries

**The catch, in full.** Six rows bound at 12:26:30. The human quit at 12:27:00
and relaunched two seconds later. The launch recorded five. The missing row is
`b2206bcf`, the one the drive minted with `clave add` in the real worktree —
the only row whose claude got past the trust prompt. The five survivors all sat
at that prompt with no session started, so no hook ever fired for them.

**Why it escaped everything.** The September verification restored four tabs
and was called proof. Those were held rows: bound by the bar with a tab and no
pane, agents never started. The `SessionEnd` unbind requires the row to OWN the
pane, so held rows are exactly the population it cannot touch. The
verification tested the case the defect cannot reach, and read as green.

**It fits what the human reported before any of this work.** He said that
before the restore existed he had to screenshot his tabs before quitting. The
tabs he lost were the ones he had been working in — which is this defect, not
the one already fixed.

**Two candidate fixes, neither yet chosen.**
(A) On `SessionEnd`, clear `pane_id` only and keep `tab_id`. Small, and it
matches the real distinction: the agent exited, the tab did not close. The
risk is whatever the comment's "reverts the tab to a terminal tab" defends —
establish what the bar renders terminal-ness from first (`spawn_uuid` in
`model.rs` suggests it reads the pane command, not the bind).
(B) Maintain `last_live` incrementally — add a uuid when it binds, remove it
when its tab is PRUNED. Larger, but it stops the restore set depending on
binds surviving a process exit at all.
Prefer (A) if the bar allows it; it removes the conflation rather than working
around it.

**Two drive runs were wasted before the phase ever ran**, both to a
ten-minute ask window closing. Budgets are thirty minutes now. A drive that
refuses BEFORE phase 0 leaves the stage untouched and can simply be re-run;
one that drove any phase needs a fresh stage.

**Harness friction:** compound shell one-liners with runtime variables get
refused by the worktree-isolation classifier, so `git add … && git commit -F …`
must be split. `zellij delete-session --force` was refused by the permission
classifier; `kill-session` was allowed, and the next launch deletes a dead
session itself.

## Next Steps

1. **Reproduce at the store level, test first.** Bind a row to a tab with a
   pane; apply the `SessionEnd` path (`apply_hook_pane`, `hook.rs:1254`); call
   `clear_session_order`; assert the row is ABSENT from `last_live`. Red
   settles the diagnosis and becomes the regression guard.
2. **Establish what the bar renders a terminal row from** — the pane command
   or the store bind. That decides whether fix (A) is available.
3. **Agree the fix with the human before writing it.** Give him the choice in
   his terms: "the tab you were working in comes back" versus what it costs.
   He does not read code.
4. **Fix, with the store-level test going green.** Keep the tab-CLOSED unbind
   (`prune-tabs`) untouched; phase 3 of the drive covers it and a regression
   there is the RC-A class.
5. **Run `just mutants` over the branch diff.** Advisory, not a gate, and the
   cache makes a re-run cheap.
6. **Re-drive.** `just qa qa-fleet`, hand him the launch line, and WARN him
   about the second ask. Phase 6c is the acceptance test.
7. **Then the branch is ready to merge** (his call, and his command).

Where work stopped, verbatim:

> **Human:** "this is what I see, seems to be decent. take a look to check the
> rest, let's get this in."

That referred to the drive work, which is pushed. He then asked for this
handoff "properly so we can fix this and get it live."

Two questions outstanding for him: whether to open an issue for the defect
(issue writes wait for his go), and when to kill the sandbox.

## Context for the Work

Guardrails, verbatim and still binding:

- **Do not touch Ollie's live session.** You run inside it, so a bare `zellij`
  command hits his working fleet; run nothing against it, not even a read.
  Against your worktree's sandbox, run `zellij action` freely, staged with
  `just sandbox`.
- **Ollie launches every session.** `just launch` refuses inside zellij, and
  you are always inside it. Hand him `cd <checkout>` then `just launch`.
- **Ollie kills sessions.** One exemption: the sandbox you asked him to launch
  this conversation, once its drive and both eyeball checks are done.
- **Fire hooks only through `scripts/ct.sh --hook`.**
- **Remote surfaces wait for his go:** pushes, PRs, merges, issue writes.
- **Ask him to test what you cannot reach.**
- **`just gates` must be green before you commit.**
- **Use ASD-STE100 Simplified Technical English** in comments, new docs, and
  everything you write to Ollie.
- **Agree changes to AGENTS.md with Ollie before you write them.**

Decision ledger:

- The drive never kills and never launches; it prints the pair and waits, and
  `script_hygiene.rs` fails the build if that changes.
- A phase that could not run is NOT RUN, and the exit code still refuses to
  read green.
- The relaunch verdict lives in the tested instrument, not in the phase,
  because the phase costs two maintainer launches to try.
- The ask budget is thirty minutes per half, set from two closed windows.
- Phase 6c's red is a finding to fix, not a phase to soften.

Style: he does not read the code. Give the decision, not the mechanism. Never
use a symbol he would have to grep. Send prose AFTER a run of tool calls.

## Restart Hint

Tree clean, everything pushed, gates green, nothing mid-refactor. The sandbox
`clave-test-live-set-459d` is live and holds run 12's evidence — read the
drive log before killing it.

## Suggested Skills

- `superpowers:systematic-debugging` — there is a measurement and a suspect;
  do not skip to the fix.
- `superpowers:test-driven-development` — the store-level reproduction first.
- `superpowers:verification-before-completion` — this branch's whole lesson is
  that green tests are not evidence.
- `unslop` and `agent-prose` — for anything written to the human or into docs.
