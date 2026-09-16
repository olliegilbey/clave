# Task Pickup

You are picking up this work session from a prior agent that was tackling the
dormant-relaunch / live-set-restore feature on `worktree-live-set-restore`
(PR #261). The drive automation is BUILT and pushed. Its first complete live
run found a REAL DEFECT in the feature, which is now the whole job.

The companion record is `docs/status/2026-09-16-0026-live-set-restore.md` —
the state before this session, and the only place the earlier design history
is summarised. Read it if you need the *why* of the restore design; this file
carries everything needed to act.

## Orientation

- **Phase 6c (the relaunch) is implemented, pushed, and has driven live once.**
  | Built and run this session. | `scripts/qa-drive.sh`, search `P6c-relaunch`;
  run log `~/.local/state/clave-dev-live-set-459d/state/qa/drive-1789557833.log`.
- **THE DEFECT: the live set drops every row whose agent was genuinely
  RUNNING at the quit.** | Measured on run 12 (2026-09-16): six rows bound
  before the quit, five recorded into `last_live` and five restored. The one
  that vanished is the only row whose claude had really started. | Re-read the
  drive log above, phase P6c, the `set the launch recorded` line.
- **The mechanism, most likely: `SessionEnd` clears the row's bind.** |
  `crates/clave/src/hook.rs:1261` — a `SessionEnd` from the pane the row OWNS
  sets `tab_id = None` and `pane_id = None`. Killing the session fires that for
  every live agent, and `clear_session_order` (the next launch) then records
  `last_live` from whatever binds are LEFT. | **Open** — the causal chain is
  reasoned from the code plus the measurement, not yet reproduced. Settle it
  with a store-level test: bind a row, apply a `SessionEnd` from its own pane,
  call `clear_session_order`, and see the row absent from `last_live`.
- **Why the sandbox only lost ONE row, not all six.** | The seeded rows' claude
  sits at the "Yes, I trust this folder" prompt and never starts a session, so
  no hook ever fires for them; the minted row ran in the real worktree, which
  claude already trusts. | The human's screenshot of the restored session shows
  the trust prompt. **This makes the sandbox WEAKER than the field** — every
  real tab has a started agent, so the field loses the whole set, not one row.
- **This is why the 2026-09-15 live verification looked green.** | Those four
  restored tabs were HELD rows whose agents had never been started (tab, no
  pane), so `SessionEnd` never fired and nothing unbound them. | **Open** — the
  field consequence (a working fleet restores as one row) is inferred, not
  measured. One real quit-and-relaunch of the human's own session would settle
  it, and he is the only one who can run that.
- **A launch CLEARS every bind before it bakes the layout, and records the set
  it cleared into `last_live` on the same pass.** | `crates/clave/src/setup.rs`
  `launch_session` → `store::clear_session_order` (`store.rs:903`). | This is
  what makes phase 6c's readings non-vacuous; if it changes, the phase measures
  nothing.
- **The drive's verdict is testable offline.** | `relaunch_checks` in
  `scripts/qa/lib.sh`, run against a decayed store by the selftest. |
  `bash scripts/qa/lib-selftest.sh` — 0 failures, under a second.
- **A phase that cannot run is NOT RUN, not FAIL, and the exit code still says
  incomplete.** | Built after the first attempt timed out and threw away ten
  green phases. | `skip_phase` / `exit_on_incomplete` in `scripts/qa/lib.sh`.
- **The sandbox `clave-test-live-set-459d` is LIVE**, holding the restored
  second session from run 12. | The human launched it and confirmed it by
  screenshot. | `./target/release/clave dev status | head -c 200`. Both eyeball
  checkpoints are effectively done — he called it "decent" and the fleet
  matches the store. Killing it is yours when you no longer need the evidence.
- **Writing into `~/.claude/projects/` is BLOCKED by the harness.** | Inherited
  from the previous session; unchanged. | Do not route around it.

**Method.**

Read the drive log before theorising; it records the set on both sides of the
quit, which no test can reconstruct.
Grep `docs/FOOTGUNS.md` before debugging anything that "reads fine" — this
finding is not in it yet, so add it when the mechanism is confirmed.
Put every new reading in `scripts/qa/lib.sh` and cover it in
`scripts/qa/lib-selftest.sh`; a drive assertion that needs a launched session
to try is one nobody tries.

**Proof.** `just gates` — fmt, tests, wasm build, clippy, all silent.
For the drive instrument alone: `bash scripts/qa/lib-selftest.sh`.

## Task Overview

Quit clave, relaunch, and get your previously-live tabs back. The feature
shipped on this branch and was believed done. Phase 6c of the QA drive now
says it is not: the set comes back missing exactly the rows that were being
worked in.

Success is a relaunch that restores the set a human actually had, including
tabs whose agents were running when they quit.

## Reference Docs

- `docs/dev/QA-DRIVE.md` — the phase 6c row in the spine table (the phase's
  acceptance criteria), and the three-bullet section under "Phase 6c needs two
  maintainer launches" (how it is built, and the two facts it depends on).
  The run history names run 12 and what it found.
- `docs/status/2026-09-16-0026-live-set-restore.md` — the previous handoff.
  Read "Important Discoveries" only if you need the history of the restore fix
  itself.
- `crates/clave/src/hook.rs:1255-1275` — the `SessionEnd` unbind, with its own
  rationale in the comment. This is the suspect.
- `crates/clave/src/store.rs:903-947` — `clear_session_order`: records
  `last_live`, then clears the binds. The other half of the mechanism.

No known doc inaccuracies outstanding.

## Current State

Working tree CLEAN, everything pushed. Three commits this session on top of
the thirteen already there:

- `3c93878` phase 6c, its verdict in `qa/lib.sh`, and the guard that stops the
  drive ever killing or launching a session
- `37f4ace` NOT RUN as a third phase verdict, plus the incomplete exit code
- `85046ab` half-hour ask budget; run 12 on the record

Touched: `scripts/qa-drive.sh`, `scripts/qa/lib.sh`,
`scripts/qa/lib-selftest.sh`, `crates/clave/tests/script_hygiene.rs`,
`justfile`, `docs/dev/QA-DRIVE.md`.

## What's Working

Build ON these.

- **The phase works.** It found a defect that four gates, a swarm review,
  CodeRabbit and a full drive all missed, on its first complete run. Do not
  weaken it to make the branch green.
- **`relaunch_checks` (`scripts/qa/lib.sh`) is the verdict, and the selftest
  drives it against a store that decayed.** Copy that shape for anything else
  that can only otherwise be tried by spending a maintainer's launch.
- **`bound_uuids` / `held_bound_uuids` / `last_live_uuids` / `uuid_count`** are
  the single home for reading a fleet out of `dev status`. Do not hand-roll
  another jq; that mistake has now been made three times on this feature.
- **`script_hygiene.rs` now fails the build if any drive line starts or ends a
  zellij session.** The drive prints the pair and waits. Keep it that way.
- **The run-12 evidence is preserved** in the sandbox state dir — the drive
  log, the store, and `clave.log` with the launch lines showing `baked=[5]`.

What the phase does NOT cover: it reads the set, not the geometry, and it
asserts nothing about what the restored agents do after they are woken.

## What success looks like

The live set survives a quit by a human who was actually working. Concretely:
bind N rows with N running agents, quit, relaunch, and `last_live` holds N.
Phase 6c already asserts exactly this and currently goes red on it.

Secondarily, the sandbox stops being weaker than the field: the seeded rows'
trust prompt means the drive's own agents never start, so the drive only saw
one row of the defect instead of six.

## Important Discoveries

**The drive caught a live defect on its first complete run, and the shape of
the catch matters.** Six rows bound at 12:26:30; the human quit at 12:27:00
and relaunched two seconds later; the launch recorded five. The missing row is
`b2206bcf` — the one the drive minted with `clave add` in the real worktree,
the only row whose claude got past the trust prompt. The five survivors all
sat at that prompt with no session started.

**Why this escaped every earlier check.** The 2026-09-15 live verification
restored four tabs and was called proof. Those were held rows: bound by the
bar with a tab and no pane, agents never started. `SessionEnd`'s unbind is
gated on the row OWNING the pane, so held rows cannot be unbound by it. The
verification tested exactly the population the defect cannot touch.

**It also fits what the human said at the start of this work:** that before
the restore existed, he had to screenshot his tabs before quitting. The tabs
he lost were the ones he had been working in.

**Two runs were wasted before the drive ever reached the phase.** The first
attempt's ask timed out at ten minutes and marked the phase FAILED, discarding
ten green phases; the second run's FIRST launch window closed the same way.
Both budgets are now thirty minutes, and a missed window no longer fails the
run. The re-stage is only free if nothing drove: a drive that refused before
phase 0 leaves the stage untouched and can simply be re-run.

**Harness friction, unchanged and worth knowing:** compound shell one-liners
with runtime variables get refused by the worktree-isolation classifier, so
`git add … && git commit -F …` must be split into separate commands.
`zellij delete-session --force` was refused by the permission classifier;
`kill-session` was allowed, and the next launch deletes a dead session itself,
so this does not block anything.

## Next Steps

1. **Confirm the mechanism with a test before changing anything.** Store-level:
   bind a row with a pane, apply the `SessionEnd` path
   (`hook.rs:1261`), run `clear_session_order`, assert the row is absent from
   `last_live`. If that goes red the diagnosis is settled and the test is the
   regression guard.
2. **Decide where the fix belongs, with the human.** The two candidates:
   `SessionEnd` should not unbind when the session is ending anyway (hard —
   the hook cannot tell a quit from an exit), or `last_live` should be
   recorded from something that survives an agent exiting. The second is more
   likely right, and the branch's own principle applies: the transcripts and
   the tab history outrank a bind that a dying process can clear.
3. **Re-drive once the fix is in.** `just qa qa-fleet`, then hand him the
   launch line and warn him about the second ask. Phase 6c is the acceptance
   test and it is already written.
4. **Consider making the sandbox fire real hooks.** The trust prompt is why
   the drive saw one row instead of six. Until that changes, phase 6c reads a
   weaker fleet than the field has.

Where work stopped, verbatim:

> **Human:** "this is what I see, seems to be decent. take a look to check the
> rest, let's get this in."

That referred to the drive work, which is now pushed. The defect it found is
NOT fixed, and he has not yet been asked whether he wants an issue opened for
it (remote surfaces wait for his go).

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
- **Remote surfaces wait for his go:** pushes, PRs, merges, issue writes. He
  gave the go for this branch's push.
- **Ask him to test what you cannot reach.**
- **`just gates` must be green before you commit.**
- **Use ASD-STE100 Simplified Technical English** in comments, new docs, and
  everything you write to Ollie.
- **Agree changes to AGENTS.md with Ollie before you write them.**

Decision ledger from this session:

- The drive never kills and never launches; it prints the pair and waits. A
  test enforces it rather than a comment.
- A phase that could not run is NOT RUN, and the exit code still refuses to
  read green. A skip must never look like a pass.
- The relaunch verdict lives in the tested instrument, not in the phase,
  because the phase costs two maintainer launches to try.
- The ask budget is thirty minutes for each half, set from two closed windows
  in one day.

Style: he does not read the code. Give the decision, not the mechanism. Never
use a symbol he would have to grep.

## Restart Hint

Tree clean, everything pushed, gates green. The sandbox
`clave-test-live-set-459d` is LIVE and holds run 12's evidence — read the
drive log before you kill it. Nothing is mid-refactor.

## Suggested Skills

- `superpowers:systematic-debugging` — for the `SessionEnd` unbind. There is a
  measurement and a suspect; do not skip to the fix.
- `superpowers:test-driven-development` — the store-level reproduction first.
- `superpowers:verification-before-completion` — this branch's whole lesson is
  that green tests are not evidence.
- `unslop` and `agent-prose` — for anything written to the human or into docs.
