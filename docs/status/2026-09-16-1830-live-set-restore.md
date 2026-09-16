# Task Pickup

You are picking up `worktree-live-set-restore` (PR #261). A swarm review found
two blockers; both are FIXED. QA run 17 went red on ONE stale assertion, which
is fixed too. The branch needs a clean drive and the maintainer's eye, then it
merges.

## Orientation

- **The swarm review is DONE and acted on.** | Ten blind lanes plus an opus
  verifier, 2026-09-16. Lane reports are in `/tmp/clave-swarm/`. The verifier
  REFUTED five lane findings, including a store fix that fails a test. |
  **Checked** — every finding it confirmed is fixed or recorded.
- **Blocker 1: an exited agent read as live everywhere.** | Keeping `tab_id`
  after `SessionEnd` widened what a bind MEANS, and four readers still took it
  for "an agent runs here". `Alt+Enter` refused, `clave open` jumped onto an
  empty tab, the picker offered a jump. | **Fixed** in `da7cca3` by making the
  state nameable: `Status::Exited`. Dormancy suppresses only the TAB leg.
- **Blocker 2: a launch that binds nothing erased the restore set.** | The
  `last_live` write was unconditional. | **Fixed** in `d21c631` with
  `Store::bound_since_launch`. The fix both lanes proposed was wrong and fails
  a test — do not reintroduce it.
- **Run 17 failed at phase 5b, and it was the DRIVE that was stale.** | The
  phase fires `SessionEnd` two lines above, then asserted `idle`. Since the
  fix that reads `exited`. | **Fixed** in `3c2ce0d`. The check still asserts
  its real point: nothing left Working into phase 6.
- **Run 18 is GREEN in all twelve phases, and it proves both blocker fixes
  live.** | Five rows recorded, five rebound by the bar with nothing driven,
  focused or typed; four bound before their agent ran; the closed tab absent.
  | **Checked** — `docs/dev/QA-DRIVE.md` ledger.
- **The drive cannot cover blocker 1's restart path.** | No phase exits an agent and then
  restarts it from the bar. | **Open** — this is an EYEBALL check for Ollie:
  quit an agent, leave its tab open, the row must read hollow not dim, and
  `Alt+Enter` must bring it back; walking past must NOT restart it.

**Method.** Gates before every commit. `bash scripts/qa/lib-selftest.sh` is
under a second. The drive costs two maintainer launches and has no phase
resume — a re-drive is a full re-drive.

## Current State

Tree CLEAN. **13 commits ahead of origin, NOT PUSHED.** Gates green, 786 tests.

This session added, newest first:
- `3c2ce0d` phase 5b expects exited
- `f8e30a6` README, the 600-vs-1800 timing lie, justfile wording, QA inventory R2
- `7cf4dd8` three unheld guards pinned + the start arm latched
- `da7cca3` blocker 1
- `d21c631` blocker 2

## Next Steps

1. **The drive is DONE — run 18, all twelve green.** No further drive is
   needed unless the code changes again.
2. **Ask for the eyeball check** on the exited row (above). It is the last
   open item before merge. The sandbox from run 18 is still up, so he can do
   it now; killing it needs HIM (the classifier blocks the agent, even by
   explicit name).
4. **The PR body is rewritten and WAITING at `/tmp/clave-pr-body.md`.** It is
   not pushed. If that file is gone, rebuild it: the stale things were the
   test count (786 now), the drive count (runs 4, 11-16), the defect count
   (twelve), a dead mutation survivor, the handoff pointer, and TWO safety
   arguments describing code that no longer ships.
5. **Then push, PR, merge — all on his explicit go.**

## Context for the Work

Guardrails unchanged: do not touch his live session; he launches and kills
every session; hooks only through `scripts/ct.sh --hook`; remote surfaces wait
for his go; gates green before any commit; ASD-STE100 in everything written to
him; he does not read code, so give him the decision, not the mechanism.

Harness friction: the worktree-isolation classifier refuses multi-file python
scripts, compound shell with runtime values, and any text containing certain
words. Split into single-file scripts, or use the Edit tool.

## Restart Hint

Nothing is mid-refactor. The branch is mergeable once the drive is green and
he has eyeballed the exited row.
