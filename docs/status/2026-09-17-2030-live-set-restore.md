# Task Pickup

You are picking up `worktree-live-set-restore` (PR #261). The feature is
CORRECT and now also CHEAP: the fleet comes back complete, a closed tab stays
closed, and every restored tab comes back asleep. That last part took four live
runs to get right and is the whole reason the branch exists.

Read `docs/status/2026-09-17-0050-live-set-restore.md` for the earlier fixes.
This file does not repeat them.

## Orientation

- **The LAUNCH names who drives the restore. It is not inferred.** | `b078b86`.
  `Store::restore_owner` holds the uuid of the row the launch bakes; the bar
  reads it off the snapshot and only that tab's bar sequences the queue. |
  **Checked live** 2026-09-17 19:36: owner honoured, 4/4 back, 3/3 restored
  rows ASLEEP, handle peak 160 against a 256 ceiling.
- **The old election was circular and that was the whole defect.** | It asked
  "am I the focused tab", and every step of the restore MAKES a tab, which
  takes the focus. So each restored tab believed it was in charge and restarted
  the queue: beacon named 4 tabs 8 times in 800 ms, 5 tabs built at once, once
  a dead zellij server ("Too many open files"). | **Checked** — FOOTGUNS, three
  entries.
- **A held agent will not start while the restore queue is non-empty.** | Same
  root: a beacon storm is indistinguishable from a human walking about, so
  `beacon_moved` passed on all eight moves and two agents woke with nobody
  near them. The queue draining is what makes an arrival mean something. |
  **Checked live** — 3/3 asleep afterwards.
- **The re-anchor claim survives a frame that beats the tab it made.** |
  `2ff35ce`. A frame arriving before the new tab still names the owner, and
  `apply_tabs` read that as the claim being satisfied. The beacon ended up on
  the last tab built: FIRST Alt+Up did nothing, second worked (maintainer
  confirmed, 19:48). Discharged now only when `opening` is empty. | **Checked
  in tests, NOT yet re-checked live.**
- **`dev scenario` seeds `bound_since_launch` with a bind, and refuses a
  fixture that would restore nothing.** | `00ec60a`. The `relaunch-restore`
  scenario had been silently staging NO restore since `bound_since_launch`
  landed. It cost a maintainer launch to find, because from the outside it
  looks like a working session. It now prints "the next launch restores N
  rows" and bails if N is wrong. | **Checked** — prints 4.
- **`restore_pending` is no longer test-only.** | Carried open item, now
  resolved by use: `run_held_effect` reads it. | **Closed.**
- **A crashed launch permanently shrinks the fleet.** | Untouched, unrelated to
  everything above. The next launch records what the dead session managed to
  bind as the truth. | **Open** — agreed it is probably its own issue. Ask him
  before filing (issue writes need his go).
- **The handle peak is 160, not one tab's worth.** | The curve now climbs and
  DRAINS between tabs, but two of four still overlapped. Under the ceiling, no
  longer a crash risk; a much larger fleet could still climb. | **Open,
  accepted.** The bind lands slightly before zellij finishes building.

**Method.** Every defect on this branch was invisible to a green suite because
the suite ran ONE bar and production runs one per tab. Write two-model tests
for any arm gated on "am I the one" — `two_bars_on_one_snapshot_send_one_open_between_them`
is the shape. Prove each guard by disabling it and watching the right tests go
red; that found the real dependency every time.

**Proof.** `just gates` exits 0 — fmt, 330 bar tests + 406 host, wasm, clippy.

## Current State

Tree CLEAN. **32 commits ahead of `origin/worktree-live-set-restore`, NOT
PUSHED.** Newest first: `2ff35ce` re-anchor claim, `00ec60a` scenario seeder,
`b078b86` the owner election, `cf8590d` pace on the tab, `c196b64` drive build
tag.

**THE DRIVE PASSED — ALL TWELVE PHASES, 237 checks, 0 failures**
(run 22, 2026-09-17, log `/tmp/clave-qa-run.log`, recorded in
`docs/dev/QA-DRIVE.md` and the `scripts/qa-drive.sh` header). P6c's
`restored rows were bound before their agent ran (tab, no pane)` measured
**4 of 4**, the check that measured 2 on the run before and has never passed
until now. Nothing was driven, focused or typed: the sidebar did it alone.

**THREE REVIEW SUBAGENTS WERE RUNNING AT HANDOFF TIME**, at the maintainer's
request, before any push — one on `crates/clave-bar/src/model.rs`, one on the
host crate and types, one on tests-and-prose. If their findings are not in this
file, they did not land; re-run them.

Pre-quit bound set saved at `/tmp/qa-before-set.txt` (5 rows). If P6c times out
again, DO NOT re-drive: run
`./scripts/qa/relaunch-verdict.sh /tmp/qa-before-set.txt 00000000-0000-4000-8000-c85c00000001`
which runs the same verdict function against the relaunched sandbox.
Handle sampler writes `/tmp/fd-sample-restore.log`.

## What success looks like

Twelve green phases, and the P6c check `restored rows were bound before their
agent ran (tab, no pane)` at >=4. That check measured 2 before the owner
election and is the one that says the feature does what it promises. Then
record the run in `docs/dev/QA-DRIVE.md`, then his go to push, rewrite the PR
body and merge.

## Next Steps

1. **Act on the three reviews** (see above). That was the last thing asked
   for and it gates the push.
2. **Rewrite the PR body** with `.github/PULL_REQUEST_TEMPLATE.md` and
   `--body-file`. The current one describes a design that has changed twice.
   Gate numbers: `cargo test --workspace` = 807 passed over 11 suites, 0
   failed; wasm build and clippy clean. The subagent review fills the
   "Independent adversarial reviewer" lane.
3. **Confirm the first Alt+Up works** after a restore — `2ff35ce` is proved in
   tests only. One press in a restored fleet settles it.
4. **On his go only:** push, then merge. Then the crash-shrinks-the-fleet
   issue, which he agreed is probably its own.

## Context for the Work

- The maintainer does not read code. Give him the decision, not the mechanism.
- ASD-STE100 Simplified Technical English in comments, new docs, and everything
  written to him.
- **He launches every session.** Ask, then wait. The relaunch prompt has timed
  out twice at 30 minutes; watch for it rather than letting it sit.
- Fire hooks only through `scripts/ct.sh --hook`.
- Remote surfaces wait for his go: pushes, PRs, merges, issue writes.
- `just gates` green before every commit.
- Commits end `Claude-Session:
  https://claude.ai/code/session_01VxbdvjsGZN9x4hYyGgdXHL`.

**Decision ledger (added this session):**
- The restore's driver is NAMED BY THE LAUNCH in the store, never inferred from
  the focus. A sequencer cannot be elected by a signal its own work destroys.
- No held agent starts while the restore queue is non-empty. Cost: land on a
  restored tab mid-restore and it stays asleep until you leave and return.
  Accepted — waking an agent nobody asked for is the expensive mistake.
- The re-anchor claim is discharged only when no open is in flight.
- A seeded bind implies `bound_since_launch`; a relaunch fixture that stages no
  restore is a hard error, not a quiet pass.
- (Carried) No timer. One row per advance. Do not raise the file-handle limit.

**Harness friction.** The worktree isolation classifier refuses compound bash
with `git` inside `$(...)` and multi-command lines mixing `git`/`sed`. Write
commit messages to `/tmp/*.txt` and use `git commit -F`. Do repetitive edits
with a python script written to `/tmp` via a heredoc, then run it plainly.
`cargo build -p clave-bar` cannot link (wasm-only bin) — use `cargo test -p
clave-bar --lib`. Run `cargo fmt --all` before every `just gates`.

## Subagent review (2026-09-17, after the green drive)

Three reviewers, unbriefed on each other: the host CLI, the tests and prose,
and the bar model. Twenty-one findings. All are resolved in `9f170aa`,
`a698020`, `334fe57` and `b0c61df`, except the two below.

Four were real product defects, each a state a person reaches in ordinary use,
and none of them visible to the tests we had:

1. **Close one restored tab and no held agent ever wakes again.** The wake
   guard asked a per-instance counter a question about the whole fleet. Only
   the elected owner writes that counter, and the bar a person is standing in
   is by definition not the owner.
2. **A row whose folder is gone had the same effect**, from the first moment of
   the session, because it can never come back and so the queue never once read
   empty.
3. **An agent that exited could not be restarted.** Its tab still carries the
   original command text after the process quits, and that held the row out of
   the dormant block, which is the only place the restart is offered.
4. **Two tabs naming one agent** bound it twice every pass, forever.

Plus: the relaunch verdict tool could not fail. It is the documented recovery
when the drive times out waiting for a launch, and it printed a red verdict and
exited 0. Fixed at both ends, and its selftest now runs the script rather than
the function inside it.

Every new guard was proved load-bearing by disabling it and watching the named
test go red. Ten new tests; the bar's are two-model, per the FOOTGUNS rule.

**Still open, both needing Ollie's call:**
- **The launch's status reset is too broad.** It clears `Done` and `Failed`,
  which describe a finished turn rather than a running process, so an unread
  green result is destroyed by a relaunch. It also flattens every seeded demo
  fleet. Narrowing it to the statuses that describe a live process is small.
  **This branch, or a follow-up?**
- **A restore that does not finish permanently shrinks the fleet.** Pre-existing
  and agreed to be its own issue; needs his go to file.

Declined: the duplicated `just --list` line on `mutants-cold` is the house
pattern, not a defect.

**Not yet re-driven.** The four bar fixes above changed product behaviour, so
drive run 22's green is against the previous wasm. The exited-agent restart is
the one worth an eyeball: quit an agent, then press Alt+Enter on its row.

## Restart Hint

Tree clean, gates green, 810 tests passing, 35 commits unpushed. The review is
complete; the two open questions above gate the push.

## Suggested Skills

- `superpowers:test-driven-development` — every fix this session came from
  reproducing the live failure in the model first.
- `catch-up` — for full branch context.
