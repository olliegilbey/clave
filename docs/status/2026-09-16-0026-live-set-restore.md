# Task Pickup

You are picking up this work session from a prior agent that was tackling the
dormant-relaunch / live-set-restore feature on `worktree-live-set-restore`
(PR #261). The feature is FINISHED and verified live. What remains is drive
automation and one deferred fixture pipeline.

The companion record is `docs/status/2026-09-14-live-set-restore.md` — the full
design history, all findings, and the live-drive results. Read it only if you
need the *why* of a design choice; this file carries everything needed to act.

## Orientation

- **The branch is pushed and clean; four gates green; 773 tests.** | Ran them
  this session. | `git status --short` (empty) and `just gates`.
- **`just mutants` is now CACHED (`--iterate`) and no longer re-mutates cleared
  code.** | Built and measured this session: 17m → 52s on a warm cache. | `just
  mutants main`; the first log line reports how many it excluded.
- **The mutants cache re-tests on a mutant's IDENTITY (file/line/col/
  description), NOT on a line's meaning.** | Measured: changing `(ordinal, 0)`
  to `(ordinal, 7)` in `clave_types::live_key` produced no new mutant. | Docs
  in `docs/dev/TESTING.md`, section "The cache, and what it is allowed to
  forget". `just mutants-cold` is the escape hatch; run it before a PR.
- **A cold `just mutants main` over the whole branch diff is ZERO missed.** |
  Ran it (17 min, 136 mutants, 124 caught, 12 unviable). | Inherited — do not
  re-run for reassurance; it costs 17 minutes.
- **`TabUpdate` reaches ONLY the focused tab. A bar in an unfocused tab cannot
  resolve its own tab id.** | This killed the first version of the restored-bind
  fix; found by launching, not by any test. | `crates/clave-bar/src/main.rs:299`
  states it in a load-bearing comment; `docs/FOOTGUNS.md` now has two entries.
- **`PaneUpdate` IS global — every bar sees every tab's panes.** | Same
  investigation. | This asymmetry is the whole basis of the current fix.
- **The restored-bind leg runs from the ELECTED bar over EVERY held tab.** |
  Rebuilt and verified live this session. | `BarModel::restored_bind_effects`;
  `Effect::BindRestored` was DELETED — it emits the ordinary `Effect::Bind`.
- **Phase 6c (relaunch) is DOCUMENTED in `docs/dev/QA-DRIVE.md` but NOT
  implemented in `scripts/qa-drive.sh`.** | Grepped for it. | `grep -n "6c"
  scripts/qa-drive.sh` returns nothing. This is the top next step.
- **Writing into `~/.claude/projects/` is BLOCKED by the harness** as session
  transcript tampering. | Hit it trying to stage a big transcript fixture. |
  Do not route around it. The human declined to change it ("nevermind about
  it"), so fixture placement must happen inside `clave dev`, or be his step.
- **Open: the sandbox session `clave-test-live-set-459d` was LEFT RUNNING** at
  the end of the session (the human launched it for the second relaunch check
  and it was never killed). | Settle with `clave dev status`; kill with
  `zellij kill-session clave-test-live-set-459d`. Only ever by that literal
  name.
- **Open: the relaunch's subprocess cost is measured only at N=3.** | `seq`
  went 15→18 for three held tabs. | Eleven real rows is untested; nothing says
  the flock contention is fine at that size.

**Method.**

Launch it before believing it; three review layers and 773 tests passed a leg
that could never fire in the field.
Build a bar fixture from what the shell delivers to THAT instance in THAT state
— never call `apply_tabs` on a bar whose tab is not focused.
Read `docs/FOOTGUNS.md` before debugging anything that "compiles and reads
fine" in the bar.

**Proof.** `just gates` — fmt, 773 tests, wasm build, clippy, all silent.
For the feature itself, the one-minute live check is in *Next Steps*.

## Task Overview

Quit clave, relaunch, and get your previously-live tabs back instead of a list
of dormant rows. Success is: the live set comes back the SAME SIZE across
relaunches, ranked the way the bar ranks it, with only the top row's agent
actually running (a resume costs ~350 MB regardless of transcript size).

Constraint that shaped everything: a restored tab holds a row before its
`clave spawn` has run, so every "which row is in which tab" answer derived from
a running process is blind to exactly the case that matters.

## Reference Docs

- `docs/status/2026-09-14-live-set-restore.md` — the full task record. Sections
  worth reading only on demand: "Five findings that shaped it" (memory/resume
  costs, do not re-derive), and "The live relaunch, 2026-09-15" (why the first
  fix failed).
- `docs/dev/TESTING.md` — the escape record's three rules for this branch, and
  the mutation-cache section. The third rule ("drive every leg the SHELL runs")
  is this branch's hardest-won.
- `docs/dev/QA-DRIVE.md`, phase 6c row — the relaunch phase spec, written but
  unimplemented. It is the acceptance criteria for the next step.
- `docs/FOOTGUNS.md` — the bar's traps, including the two added this session.

No known inaccuracies outstanding; four doc defects found by CodeRabbit this
session were fixed.

## Current State

Working tree CLEAN, everything pushed. Thirteen commits on top of the original
nine. Recent:

- `0e57fc9` docs: relaunch seam verified end to end
- `2e4c81b` fix(bar): held-tab bind runs from the elected bar, over every tab
- `88ad2c7` test(dev): pin the drive's refusal (last surviving mutant)
- `6042cad` fix(bar): restored bind's retry budget needed its own ledger
- `027581b` build(mutants): cache cleared mutants

Touched this session: `crates/clave-bar/src/model.rs`, `crates/clave-bar/src/
main.rs`, `crates/clave/src/dev.rs`, `crates/clave/tests/kdl_guardrail.rs`,
`justfile`, and five docs.

## What's Working

Build ON these; they are verified and cost real time to establish.

- **The feature works end to end.** Two launches, nothing touched between:
  four tabs baked in rank order, four binds recorded, killed, relaunched, four
  back and rebound. The held rows carry a `tab_id` with NO `pane_id` — that
  combination is the leg's signature and the thing to assert on.
- **The one-minute live check is the highest-value instrument on this branch.**
  Stage, launch, touch nothing for 15 s, count binds in the store. It caught
  what nine review findings, a swarm review, CodeRabbit and 773 tests did not.
- **`fleet_bar_with_held_neighbours`** (`model.rs` test module) is the correct
  fixture shape for anything relaunch-related: our tab focused and running, the
  NEIGHBOURS held. Copy it rather than `fleet_bar_with_held_own_pane`, which
  models only the landed-on case.
- **`clave_types::sort_live_block`, `live_key`, `is_clave_binary`** are the
  single homes for ranking and binary matching. Both surfaces call them. Do not
  mint a second copy — that mistake has now been made three times.
- **The separate `restored_bind_sent` ledger.** `bind_effects` clears
  `bind_sent` for every uuid without a registered pane, which is the restored
  leg's entire population. They must stay separate. The test that holds this is
  `a_restored_binds_budget_survives_the_ordinary_bind_leg_running_beside_it`.
- **`.cargo/mutants.toml`'s `exclude_re` is deliberately small.** The last
  survivor (`dev::run_scenario`) was closed with a real test rather than an
  exclusion; prefer that order.

What the current fix does NOT cover: it binds held tabs only while an elected
bar exists and frames are coherent. It makes no attempt to act from an
unelected instance, and should not — that is the RC-A class.

## What success looks like

The relaunch seam is regression-proof without a human. Today the only thing
that can catch a break in it is a maintainer launching twice by hand, which is
why the defect survived so long. Phase 6c in `scripts/qa-drive.sh` closes that.

Secondarily: a relaunch with realistic transcript sizes is measured, so the
warming work has a real cost model instead of a spike number.

## Important Discoveries

**The first fix could never work, and every test passed.** It had each restored
tab report its own row. A bar resolves its own tab id through the tab frame, and
zellij sends that frame only to the focused tab — so the only instance able to
act was the tab the human was already looking at, the one case needing no help.
Four restored tabs produced ONE bind live. Every unit test passed because each
handed a bar in an unfocused tab a tab frame it would never receive. The
fixture's own doc comment recited the rule and then contradicted it.

The rebuild puts the work on the elected bar, which has the global pane
manifest (every held pane, every uuid) plus the full tab list (position → id).
Only the position-to-id step crosses frames, and `frames_coherent` — already
required by `elects_confirmed` — guards exactly that. `Effect::BindRestored`
and its ungated shell arm were deleted; being elected is what the shell's
ordinary bind gate already tests.

**The retry budget was inert before that** (CodeRabbit, local CLI run).
`restored_bind_effects` shared `bind_sent` with `bind_effects`, justified by a
doc comment claiming the legs are "disjoint by construction". They are disjoint
in what they EMIT and not in what they CLEAR. Measured 12 subprocesses against
a budget of 4.

**Tried and rejected:** having the host write binds for baked rows at launch —
zellij-server is not vendored, so tab-id assignment from a layout is
unmeasurable, and a wrong bind is the RC-A disaster class. Also rejected:
moving the mutants diff base to a "last clean" marker — `--iterate` subsumes it
and is more precise, keying on the mutant rather than on commit boundaries.

**A bulk `sed`/`python` replace across the test module removed seven copies of
an `apply_tabs` block when only four were intended**, breaking three unrelated
tests (`a_stranded_beacon_elects_only_the_active_instance`,
`close_does_not_reorder_neighbours`, `nav_is_executor_gated_...`). That block is
load-bearing in several places. Edit these tests individually.

**Harness friction worth knowing:** compound shell one-liners with runtime
variables get refused by the worktree-isolation classifier. Split into plain
commands or use the file tools. `pgrep -f cargo-mutants` inside a wait-loop
matches the loop's own command line and spins forever.

## Next Steps

1. **Kill the leftover sandbox session** if it is still up:
   `clave dev status`, then `zellij kill-session clave-test-live-set-459d`.
   Never another name — the human's live fleet shares this machine.
2. **Implement phase 6c in `scripts/qa-drive.sh`.** Spec is the phase 6c row in
   `docs/dev/QA-DRIVE.md`. It needs two maintainer launches, so the drive must
   stage, wait, assert, print the kill+relaunch pair, wait again, assert. The
   assertions that matter: the set comes back the SAME SIZE; every restored row
   carries a `tab_id`; each row except the first carries it BEFORE its agent
   runs (the first is eager and starts at launch); a tab closed in session N is
   absent in N+1.
3. **The transcript fixture pipeline (deferred by the human, do not start
   without asking).** To test long-conversation resumes, transcripts must be
   laundered from real ones — the schema is Claude Code's internal format and a
   from-scratch generator would be fragile. Real fixtures exist at
   `~/code/clave-spike/fixtures/{small-0.8mb,medium-2.8mb,large-18mb}.jsonl` and
   are NOT commit-safe. Record types and key sets were surveyed this session.
   Blocker: placing a fixture requires writing to `~/.claude/projects/`, which
   the harness refuses; it must happen inside `run_scenario` or be the human's
   step. UNVERIFIED: whether `claude --resume` accepts a rewritten transcript
   at all. Settle that before building anything on top.
4. **Measure relaunch cost at realistic N.** Three held tabs cost three binds;
   eleven rows is the real case and is unmeasured.

Where work stopped, verbatim:

> **Human:** "it's up, but I haven't touched anything"
>
> **Agent:** reported four tabs baked in rank order and four rows bound —
> "Write four, quit, read four, rewrite four. The decay is gone."

The human had earlier said, of the pre-fix behaviour: *"the way it works now is
actually good enough, when I land on each tab, it restores... Every time I was
going to close down clave I had to take a screenshot to remember what tabs were
open."* He then asked whether the proper fix was big, was told it was contained,
and said **"Okay, then give it a go, and can kill."** The fix was built and
verified; his original "good enough" position is superseded.

## Context for the Work

Guardrails, verbatim and still binding:

- **Do not touch Ollie's live session.** You run inside it, so a bare `zellij`
  command hits his working fleet; run nothing against it, not even a read.
  Against your worktree's sandbox, run `zellij action` freely, staged with
  `just sandbox`.
- **Ollie launches every session.** `just launch` refuses inside zellij, and
  you are always inside it. Hand him `cd <checkout>` then `just launch`. He
  also runs `just release` and owns anything that writes `~/.local/share/clave/`.
- **Ollie kills sessions.** One exemption: the sandbox you asked him to launch
  this conversation, once its drive and both eyeball checks are done. Kill it
  by explicit name (`clave dev instance --field session`), never another
  agent's.
- **Fire hooks only through `scripts/ct.sh --hook`.**
- **Remote surfaces wait for his go:** pushes, PRs, merges, issue writes.
- **Ask him to test what you cannot reach.**
- **`just gates` must be green before you commit.**
- **Use ASD-STE100 Simplified Technical English** in comments, new docs, and
  everything you write to Ollie.
- **Agree changes to AGENTS.md with Ollie before you write them.**

Decision ledger from this session:

- Mutation caching: `--iterate` plus a config/toolchain cache key, with
  `mutants-cold` as the manual drop. Accepted that a WEAKENED test can be
  masked; a BROKEN one cannot (the unmutated baseline aborts the run). This is
  affordable only because mutants is advisory, not a gate.
- `mutants-file` stays uncached — it is the deliberate deep run.
- The last mutant survivor was closed with a test, not an `exclude_re` entry,
  because the survivor pointed at real untested behaviour.
- Pushing happens on his say-so; he gave it for this branch's work each time.

Style: he does not read the code. Give the decision, not the mechanism. Never
use a symbol he would have to grep.

## Restart Hint

Tree clean, everything pushed at `0e57fc9`, gates green. Safe to start
anywhere. Check first whether `clave-test-live-set-459d` is still running.

## Suggested Skills

- `superpowers:verification-before-completion` — this branch's entire lesson is
  that green tests are not evidence; run the live check before claiming a fix.
- `superpowers:test-driven-development` — for phase 6c, write the failing
  assertion first.
- `unslop` and `agent-prose` — for anything written to the human or into docs.
