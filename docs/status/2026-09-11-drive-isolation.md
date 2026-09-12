# Status — the drive can no longer reach the maintainer's session

Branch `worktree-triple-card`, worktree `.claude/worktrees/triple-card`, PR #259.
Written after the 2026-09-11 incident and the four-layer fix for it.
Companion to `2026-09-11-triple-card-review-round.md`, which covers the card
feature and the CodeRabbit round; this file is only about the harness.

## What happened

The QA drive hung the maintainer's live `clave` session, twice over
(five hook events per phase-5b run, three runs, plus five more from an ad-hoc
probe run while diagnosing a write count).

Nothing in clave was broken. Phase 5b — written earlier the same day — grew
its own `hook_fire` helper:

```bash
CLAVE_STATE_DIR="$STATE_DIR" CLAVE_DATA_DIR="$DATA_DIR" "$CLAVE_BIN" hook "$event"
```

The store write landed in the sandbox, correctly. But `clave hook` also
PUSHES, and it aims that push with `--session "$ZELLIJ_SESSION_NAME"`
(`hook.rs::own_session`). A drive shell runs INSIDE the maintainer's fleet, so
that variable named **his** session, and every event fired
`zellij pipe --name clave-status` at his bar carrying a 6-row sandbox
snapshot.

FOOTGUNS #281 already described this exact trap, including the sentence "a
drive that sets only the state dir looks entirely correct", and already named
`scripts/ct.sh --hook` as the only sanctioned way to drive a hook. **The rule
existed, was correct, was sufficient, and was useless.** That is the finding
worth keeping: a rule every call site must remember is the wrong shape for
this hazard.

## The four layers

Only the first depends on anyone reading anything.

1. **The binary decides, from the store.** `hook.rs::aim_push` resolves a
   push's destination from the store it just wrote rather than from the env.
   `sandbox::owner_session_of_store` inverts `session_name_for`, so a process
   holding a store knows which bar is entitled to hear about it. Refuses in
   BOTH directions — a sandbox store pushed at a real session, and a real
   store pushed at a sandbox bar. What clave cannot place (a real store, a
   real session) is aimed as asked, because guessing there would be clave
   narrowing a working install. **The env is the caller's claim; the store is
   the fact.** Nine tests, including the incident's exact shape and the
   real-install paths that must keep working.
2. **The refusal is written down.** One `push-refused` line in the store's own
   `clave.log` (`evlog::log_event_in`, so it lands beside the store it
   refused, not the ambient one) plus a stderr note, which is captured in the
   transcript's hook attachments. This class cost rounds because its tell was
   silence.
3. **The drive scrubs once, before its first phase.** `ZELLIJ`/
   `ZELLIJ_PANE_ID` unset, `ZELLIJ_SESSION_NAME` re-pointed at the sandbox,
   store dirs exported — so every child inherits the sandbox whether or not it
   was routed through `ct.sh`. The default is now fail-safe rather than
   fail-dangerous. A tripwire refuses the run otherwise, and exists mainly as
   a reordering guard. `ct.sh` keeps its per-call scrub: it is also run
   directly.
4. **It is enforced, not documented.** `crates/clave/tests/script_hygiene.rs`
   fails the build if any line in the drive fires a hook outside
   `ct.sh --hook`, or if the scrub stops preceding the first phase. Confirmed
   to bite by reintroducing the offending line shape and watching it name it.

**The witness** — new phase `P6b-isolation-witness` — asserts zero
`push-refused` events across a run, that the ambient identity is still the
sandbox's at the END of the run and not merely the start, and that the
inherited session is named nowhere as a push target. It cannot check his
session directly: looking IS touching. The refusal count is the only evidence
available from this side, and it is trustworthy precisely **because the
refusal lives in the binary** — a check the script performed on itself would
have passed happily on the day it was written.

## Easier, not harder

- **`just qa <scenario>`** stages, prints the launch line, blocks until the
  session appears, then drives. Verified live: the drive picked up the launch
  by itself after 292s with no message between agent and human. The loop was
  three messages wide (stage, ask, be told, drive) and every round trip was a
  chance to drive the wrong thing.
- **The launch is two lines, and it refuses the wrong hands.** `clave dev
  launch` now derives the PATH shim itself (`shim_first_on_path`) alongside
  the three `CLAVE_*` vars it already derived, so the five-line pasted env
  prefix is gone: `cd <worktree> && just launch`. The `cd` is the only
  load-bearing part, because the instance is cwd-keyed. And it **refuses when
  `ZELLIJ` is set** — an agent is always inside a session, so the rule
  "launching is the human's" is now enforced rather than remembered, with a
  refusal that names the command to hand over and points at `just qa`.
- **The preflight checks the bar's SOURCE, not its tag.** `build=<short HEAD>`
  broke whenever HEAD moved without the bar moving — commit a host-only fix
  and a byte-identical bar read as stale, clearable only by a relaunch, which
  is a strong incentive not to commit mid-drive. An older tag is now accepted
  only when the `crates/clave-bar` and `crates/clave-types` trees hash equal
  at that commit and at HEAD, with a dirty bar tree disqualifying it. That is
  a stronger claim than tag equality, which never looked at the source.
- **A stale stage says so.** Phase 1 counts SEEDED rows (`c85c` uuids) and
  reports the bound-row count, the residue count and the two commands that
  fix it, then continues so the shape-independent phases still report.

## Do not lose

- **The drive is NOT idempotent and cannot be.** Phases 2-5 bind dormant rows,
  mint a row (`clave add` at rung 1, by design), churn tabs and toggle width —
  they consume the very starting shape they assert against. An earlier claim in
  this session that phase 1's row-count fix made re-runs work was wrong: the
  dormant count is stage-fresh-only too. Re-staging needs the session down, so
  the loop is kill → `just qa` → launch.
- **`clave dev scenario` calls `run_setup`**, so it regenerates config.kdl —
  it is not a shortcut around the #44 refusal that blocks re-staging under a
  live session.
- **`seq` is not a per-hook-event write counter** (now FOOTGUNS): a stale PR
  cache makes the hook spawn `pr-sync` outside the flock, a second writer
  landing off-schedule. Phase 5b warms the cache before counting.
- **CLOSED:** the "second writer" in phase 5b was never real. It measured 6
  writes for 5 events on a STALE stage and 4 on a fresh one — the extras were
  `pr-sync` writes from the previous run's rows landing inside this run's
  sampling windows. Two lessons, both already fixed above: a stale stage
  poisons counts in ways that look like defects, and chasing this one by
  hand-firing hooks is what put fifteen extra pushes into the maintainer's
  session. Drive hooks through `ct.sh --hook`, and re-stage before believing a
  count.
- **OPEN:** the released `clave` (v0.4.0) rejects a `card` store outright
  (`unknown variant`). After this merges, `just release` stops being optional —
  a stale `clave` earlier on a PATH takes every hook down silently. Nothing to
  fix in code; leniency only helps readers that already shipped with it.
- Commit trailer: `Claude-Session: https://claude.ai/code/session_01FmBv3E9cVa8EynJQrTXTN3`

## Verification

Four gates green: 382 + 11 + 15 + 4 + 1 host tests (the 4 are the new
`script_hygiene` suite), 284 bar, 29 types; clippy clean; wasm builds. The
host count is net-down two: `launch_command` and its two format-shape tests
went with the env-prefix wall it printed, and the per-worktree property they
guarded is asserted directly in `sandbox.rs`
(`a_linked_worktree_gets_its_own_session_and_root`).

Both new guards were confirmed to FAIL when the property they protect is
broken on purpose — the script-hygiene test against a reintroduced direct
`clave hook`, and the aim guard by unit test against the incident's shape. The
nested-launch refusal was confirmed live from inside a session, which is the
only place an agent can ever run it from.

**The drive itself: FULL GREEN, all ten phases**, run 11, 2026-09-11, first
run of the one-command loop (`just qa qa-fleet`):

| P0 | P1 | P2 | P3 | P4 | P5 | P5b | P6 | P6b | P7 |
| -- | -- | -- | -- | -- | -- | --- | -- | --- | -- |
| ✓  | ✓  | ✓  | ✓  | ✓  | ✓  | ✓   | ✓  | ✓   | ✓  |

The witness read zero refusals, the identity still `clave-test-triple-card` at
the end, and `clave` — the session it was launched from — named nowhere as a
push target. Phase 5b's twelve card-cell assertions all passed against the
real store and the real transcript reader.

Still eyeball-only, deliberately: the spinner's motion and weight, and the
card's actual four-line text (the bar pane is unselectable, so `dump-screen`
cannot reach it).
