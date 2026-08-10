# Per-agent sandbox isolation — unfinished, pick up here

## State

Branch `fix/sandbox-segregation`, worktree
`.claude/worktrees/sandbox-segregation`, pushed at `63345ea`. Based on
`origin/main` (e3c2860, the `clave prune --idle-days` merge). **No PR — do not
open one until the verification below is done.**

Two agent sessions were stopped mid-task: the first by the user, the second by
the account's monthly spend limit. The commit is a preservation commit, not a
claim of completion. Its message lists the same gaps as this file; trust
whichever you read second.

## What the change does

The dev sandbox was a singleton — one session name (`clave-test`) and one root
(`~/.local/state/clave-dev/` holding `state/`, `data/`, `shim/`). Concurrent
agents overwrote each other's plugin binary, generated KDL and PATH shim, so one
agent would launch a "sandbox" running another's build. It bit several times,
and once left a dev bar pane inside the maintainer's live fleet.

The branch makes the whole instance per-agent — session name, state dir, data
dir and shim dir all derived from one key, keyed on the worktree directory name
so it is unique by construction and legible in a session list. Running from the
main checkout keeps the familiar `clave-test` name and root. A reaper sweeps
sandboxes whose originating worktree is gone.

Changed: `crates/clave/src/sandbox.rs` (new, ~760 lines), `dev.rs`, `main.rs`,
`lib.rs`, `scripts/sandbox-setup.sh`, `justfile`, `CONTRIBUTING.md`,
`docs/dev/TESTING.md`, `FOOTGUNS.md`, `UBIQUITOUS_LANGUAGE.md`.

## What is verified

`just gates` passes — all four, workspace suite green at 210 + 130. The new
module carries **20 tests** (`crates/clave/src/sandbox.rs`, `mod tests` at
`:454`). Those 20 are the ones to attack first — see below.

## First steps, concretely

```bash
cd .claude/worktrees/sandbox-segregation   # already on fix/sandbox-segregation
git log --oneline -2                       # 54247bd, 63345ea
just gates                                 # confirm the baseline is still green
cargo mutants -p clave --file crates/clave/src/sandbox.rs --timeout 120
```

Then read the diff in full — `git show 63345ea` — before changing anything. It
was written across two interrupted sessions and has never been reviewed.

## What is NOT verified — do this before opening a PR

1. **No test here has been proven able to fail.** This is the repo's recurring
   failure mode (three near-misses on #56 alone; four more on #112, where tests
   kept passing for the wrong reason after the behaviour under them changed).
   Reinstate each defect deliberately, watch the test go red, restore it, and
   say so in the PR. A green suite that never proved it can go red is not
   evidence — treat the whole suite on this branch as unproven.
2. **`cargo mutants` was never completed.** The full-file run over `dev.rs`
   was drowning in pre-existing scenario tables; the diff-scoped run is the one
   that was about to be tried and never was.
3. **Nobody has read the whole diff** — not a human, not an agent. It is ~1260
   lines written across two interrupted sessions.
4. **The zellij session-name rules were to be established from the vendored
   source** (`~/.cargo/registry/src/*/zellij-utils-0.44.3/`), not guessed.
   Confirm that was actually done rather than assumed — length limits and
   illegal characters both matter, and the derivation must be deterministic.

## What cannot be verified from an agent session at all

Proving two sandboxes genuinely coexist needs a session launch, and session
lifecycle is the maintainer's. State that limit plainly in the PR rather than
implying end-to-end coverage.

## Hard constraints that applied and still apply

- Never launch or kill a zellij session; never run a bare `zellij` command. The
  agent runs inside the maintainer's live fleet, so a bare command targets it.
  `zellij list-sessions -n` as a read-only guard is the only sanctioned call.
- **The sandbox session env var FAILS OPEN onto the live fleet** (#137) — set it
  wrong, or not at all, and a command aimed at the sandbox hits the maintainer's
  working session. The maintainer's standing instruction is that driving goes
  through a wrapper (`ct.sh`) rather than bare env vars. **Verify this before
  relying on it: no such script exists on this branch or on `origin/main`
  (`scripts/` holds only `sandbox-setup.sh`), so it lives on an unmerged branch,
  or is still to be built.** Do not assume the safe route exists — establish
  what it actually is first. This correction matters because the fail-open
  direction is toward his live fleet, not away from it.
- Never write `~/.cargo/bin/clave`, `~/.local/share/clave/**`,
  `~/.config/zellij/**`. The staging script self-checks this; keep it working.
- The #44 identity pair: generated `config.kdl` and `layout.kdl` must carry an
  identical `clave_binary`, or the next keypress spawns a second bar.

## Explicitly not this ticket

**Issue #160** — an agent staging from inside the live fleet leaving a dev bar
pane in it. Independent: per-agent roots stop agents clobbering *each other*;
they do nothing about a stray pane in the *live* fleet. Filed separately, and
the likely fix there is refusing to stage from inside any zellij session at all.

## The other branch in flight

`fix/112-dormant-segregation` — PR #151, live/dormant segregation and the
two-ring nav. Complete and green but **not live-validated**; it is based on
`960c8db`, before the prune merge, so it will want a rebase. Nothing it touches
overlaps this branch.
