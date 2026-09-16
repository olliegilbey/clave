# Task Pickup

You are picking up `worktree-live-set-restore` (PR #261). The restore feature
works and is proved across a real session boundary. The two blockers the last
handoff named are now FIXED, tested and committed, and **nothing on this branch
has been run live since `0c6e286`**. The sandbox is staged. The branch needs one
clean drive, then the maintainer's go.

Read `docs/status/2026-09-17-0010-live-set-restore.md` for the handle
measurement and the earlier fixes; `docs/status/2026-09-16-2052-live-set-restore.md`
for the swarm review. This file does not repeat them.

## Orientation

- **The restore now brings a tab back ASLEEP and hands the focus straight
  back.** | `0028822`. `clave open --restore-to <tab_id>` bakes the spawn held
  and then runs `zellij action go-to-tab-by-id`. | **Checked** in tests, 798
  green, gates green — **never run live.**
- **`zellij action new-tab` ALWAYS takes the focus. The layout cannot refuse
  it.** | A tab node's `focus` property becomes the layout's
  `focused_tab_index`, while the new-tab path reads the ROOT PANE's `focus`,
  which a tab node never sets — so the first tab always wins
  (`zellij-utils-0.44.3/src/input/actions.rs:1611-1625`,
  `kdl_layout_parser.rs:1191`). No CLI flag exists (`cli.rs:1229-1258`). |
  **Checked** — source read, now `docs/FOOTGUNS.md`. This is why the hold alone
  was not enough: a held tab the human is left standing on is started by its
  own bar at once.
- **The newborn tab's bar announces its own birth beacon, and that is two
  separate hazards.** | (a) The beacon ends up naming a tab nobody stands in:
  nav reads as dead and the queue stalls, because only the beacon's bar opens
  the next row. (b) That same beacon reads as the human arriving, which starts
  the agent. | **Checked** — (a) is answered by `restore_reanchor_owed`, paid on
  the next tab frame (the frame the returning focus delivers). (b) is answered
  by `beacon_moved`, set only in `set_beacon`, read by `run_held_effect`.
- **Both guards are proved load-bearing.** | Disabled each in turn: the
  re-anchor pair goes red without its trigger, and
  `a_tab_born_focused_never_starts_its_own_held_spawn` goes red without the
  move check. | **Checked** — measured this session, not argued.
- **The two old tests of the held-run arm were built out of the birth
  announce.** | `fleet_bar_with_held_own_pane` never called `beacon()`; the
  birth announce set the beacon to its own tab, so the fixture modelled the one
  state that must NOT start an agent, and called it a landing. FOOTGUNS.md:211
  in the very file that records it. | **Checked** — the fixture now walks the
  beacon (10 → 11), and `born_in_a_held_tab` keeps the birth state for the test
  that asserts it starts nothing.
- **A residual race remains, and only a drive can settle it.** | If the
  newborn bar's own announce reaches it AFTER the sequencer's re-anchor, the
  beacon moves A → B and the guard passes while the focus is really on A. The
  ordering says it cannot: B announces off the frame it gets while focused,
  which precedes the focus return that triggers A's re-anchor. | **Open** —
  ordering argued, not measured. Phase 6c's held floor is the detector.
- **A restored tab reached by NATIVE zellij tab nav may not start its agent.** |
  Its bar's birth announce is what moves the beacon, and `beacon_moved` refuses
  a first-ever beacon. Any earlier beacon in the session (the eager bar's birth
  announce, any clave nav, the sequencer's re-anchor) clears it. | **Open,
  accepted** — clave's own nav always works. Watch for it on the drive; tell
  the maintainer if he meets it.
- **A crashed launch permanently shrinks the fleet.** | Carried over,
  untouched: the next launch records what the dead session managed to bind as
  the truth. | **Open** — a real second defect, independent of everything above.
- **`BarModel::restore_pending` is public and called only from tests.** |
  Carried over. The store advance is the clock, so the shell does not need it. |
  **Open** — delete it, or say why it stays.

**Method.** Read the vendored zellij source before believing a layout property
does what it reads like — `focus=false` parses, is honoured nowhere the CLI
path looks, and cost the whole first design. Prove each guard by disabling it
and watching the test go red; two guards here were written before that check
and one of them was aimed at the wrong thing until it was run.

**Proof.** `just gates` exits 0 — fmt, 798 tests over 11 suites, the wasm
build, clippy. `bash scripts/qa/lib-selftest.sh` prints `0 failure(s)`.

## Task Overview

#261: the fleet you had open comes back; a tab you closed stays closed; nothing
starts until you go there. Success is the maintainer keeping it — a clean
twelve-phase drive, then his go to push, update the PR body and merge.

## Reference Docs

- `docs/dev/QA-DRIVE.md` — the ledger. Runs 13-18 are the useful history.
- `docs/FOOTGUNS.md` — this branch's entries end with the new-tab focus one.
- `docs/UBIQUITOUS_LANGUAGE.md:211-214` — `exited row`, the six `Status`
  variants.
- `/tmp/clave-pr-body.md` — **tmp, may be gone.** Regenerate from the log.

## Current State

Tree CLEAN. **24 commits ahead of `origin/worktree-live-set-restore`, NOT
PUSHED.** Gates green, 798 tests.

Newest first:
- `b5bec87` FOOTGUNS: a new tab always takes the focus
- `0028822` the held open, the focus return, both beacon guards
- `59c5868` handoff
- `0c6e286` the staggered restore — the resource blocker

**The sandbox is staged and ready.** `./scripts/sandbox-setup.sh qa-fleet` ran
clean this session; the session name is `clave-test-live-set-459d`. The stale
one from the previous session was killed by explicit name, and that worked.

## What's Working

- **The restore itself is correct**, proved across a real relaunch. Build on
  it; do not re-litigate it.
- **`set_beacon` is now the ONE place `current_tab` is written.** Four callers
  route through it. Keep that — the `None → x` / `x → y` distinction is the
  whole guard, and a caller that assigns the field directly silently loses it.
- **`--restore-to` carries the hold AND the focus target in one option.** They
  are never separable: a hold with no focus return is a hold that does not
  hold. Do not split them into two flags.
- **`TabSpec` replaced eight positional arguments** to `tab_layout`/`tab_node`.
  Add tab inputs there, not to a parameter list.
- **`relaunch_checks` in `scripts/qa/lib.sh`** is the home for relaunch
  verdicts, and every check in it has a selftest case that makes it go red.

## What success looks like

Twelve green phases, no "Too many open files", the fleet coming back complete,
**and every restored row asleep until the maintainer walks to it** — phase 6c's
`check_min "restored rows were bound before their agent ran (tab, no pane)"` is
the one that says so. Then push, PR body, merge.

## Important Discoveries

**The hold was only half the fix, and the half that was missing is not in the
bar at all.** The last handoff framed the blocker as "the restore uses the
Alt+Enter path, which resumes". True, and fixing only that would have failed
the drive anyway: the tab takes the focus regardless, and the bar starts a held
agent when the human lands. Two maintainer launches saved by reading
`actions.rs` instead of driving.

**A guard you have not watched fail is not a guard.** The `beacon_moved` check
was written for the newborn bar's self-nomination; running the suite showed the
two existing held-run tests failing, and the reason was that their FIXTURE was
that same self-nomination. The guard was right and the tests had been green on
the wrong input all along.

**`go-to-tab-by-id` exists in the CLI and not in the plugin API.** FOOTGUNS
records "zellij-tile 0.44.3 has no switch-tab-by-id"; that is true of the
plugin shim and NOT of `zellij action` (`cli.rs:1214`). A stable id is what
makes the focus return safe while the tab list is moving.

## Next Steps

1. **Drive it.** The sandbox is staged. Ask the maintainer to `cd` here and run
   `just launch`. Start `scripts/qa/fd-sampler.sh clave-test-live-set-459d`
   first — the peak handle count should now stay near one tab's worth.
2. **Watch two things the drive does not name.** Does the focus stay put while
   the tabs arrive? And does any restored agent start before he walks to it
   (`clave dev status`, or the store's pane ids)? Both are the residual race.
3. **Watch the restore complete.** `last_live` says how many are owed; the
   store's bound rows say how many arrived. A stall means the chain broke.
4. **Record the run** in `docs/dev/QA-DRIVE.md`.
5. **Consider a drive assertion** that the restored fleet comes back COMPLETE —
   nothing currently names a queue that stalls halfway.
6. **Then the crash-shrinks-the-fleet defect.** Probably its own issue rather
   than more scope on #261 — ask him.
7. **On his go only:** push, PR body, merge.

Verbatim, where the design came from:

> Yeah, I think it should bring back the one that's opened on anyway, then just
> go vertically down the list in the background bringing them up, a few seconds
> between each. And if the user navs to a tab before it's brought to life, it
> gets brought back as they land on it. It's the most straightforward
> implementation, and then we bring them back sequentially.
> And then, what we can do to protect our resources and be efficient in these
> scenarios is important.

Implemented with two deliberate changes, both agreed with him: **the spacing is
not a fixed number of seconds** but "when the previous tab has appeared", which
is self-pacing and needed no timer; and **the tabs come back asleep**, so
"bringing them up" means the row and its tab, not the agent. He has not seen
either run.

## Context for the Work

- The maintainer does not read code. Give him the decision, not the mechanism,
  and never a symbol he would have to grep.
- ASD-STE100 Simplified Technical English in comments, new docs, and everything
  written to him.
- **He launches every session.** `just launch` refuses inside zellij.
- Fire hooks only through `scripts/ct.sh --hook`.
- Remote surfaces wait for his go: pushes, PRs, merges, issue writes.
- `just gates` green before every commit.
- Commits end `Claude-Session:
  https://claude.ai/code/session_01VxbdvjsGZN9x4hYyGgdXHL`; PR descriptions end
  with the same URL, bare.

**Decision ledger (added this session):**
- The restore's open is held AND returns the focus. One option, `--restore-to`,
  because the pair must never be asked for separately.
- The focus return is sent by `clave open`, not by a second bar effect: only
  that process knows the tab exists, and `run_command` gives the bar no
  completion to sequence against.
- The focus return is addressed by STABLE tab id, never by position.
- The sequencer's re-anchor gets its OWN claim, not `organic_pending`, which
  `beacon()` clears because an arriving beacon is truth — here the arriving
  beacon is the thing to undo.
- A held agent starts on a beacon that MOVED, never on a first-ever beacon.
- `tab_layout` takes a named `TabSpec`.
- (Carried) No timer. The store advance is the clock. One row per advance. A
  launch clears every status. Do not raise the file-handle limit.

**Harness friction.** The worktree isolation classifier refuses compound bash
with runtime values, `git` inside a `$(...)`, and `sed` with computed programs.
Write the commit message to a file and use `git commit -F`. A short python
script written with the Write tool, then run as one plain command, is the way
to do a repetitive multi-file edit.

## Restart Hint

Tree clean, gates green, 798 tests, 24 commits unpushed, sandbox staged.
Nothing mid-edit. The next action needs the maintainer: ask him to `cd` here
and run `just launch`.

## Suggested Skills

- `superpowers:test-driven-development` — every defect on this branch was
  invisible to a green suite, twice because the fixture was wrong rather than
  the assertion.
- `catch-up` — for full branch context before touching anything.
