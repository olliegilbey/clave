# Task Pickup

You are picking up `worktree-live-set-restore` (PR #261). The restore feature
works and is proved across a real session boundary. A LIVE QA drive then found
a resource blocker — restoring the whole fleet at once crashes zellij — and the
fix for that is written, tested and committed but **has never been run live**.
The branch needs one clean drive, then the maintainer's go.

Read `docs/status/2026-09-16-2052-live-set-restore.md` for the swarm review and
the earlier fixes. This file does not repeat them.

## Orientation

- **Building a tab costs a burst of about fifty file handles, opened in the
  same instant and drained a second later.** | Sampled the zellij server during
  three launches, 2026-09-16. Four tabs peaked at **252** against macOS's
  default soft limit of **256**; five crashed the server with "Too many open
  files", reproduced twice. A RUNNING session is cheap — seven tabs sat steady
  at 68. | **Inherited** — cost three maintainer launches. Re-measure with
  `scripts/qa/fd-sampler.sh <session>` if you need it again.
- **The maintainer will NOT raise his handle limit, and he is right.** | His
  daily fleet has run at 256 for months. Raising it would hide the asymmetry
  that identified the bug. | **Checked** — do not propose it again.
- **The fix is committed and UNPROVEN live.** | `0c6e286`: the launch bakes one
  tab, the bar opens the rest one at a time. 790 tests green, gates green. |
  **Open** — no live run has exercised it. This is the next thing to do.
- **The restore needs no timer, and deliberately has none.** | Opening a row
  binds it, binding writes the store, the store pushes the next snapshot. The
  chain paces itself at one tab per store advance. | **Checked** — the wire is
  one line at the end of `apply_snapshot` (`crates/clave-bar/src/model.rs`).
  The shell's `Event::Timer` classifier was the alternative and is a trap: it
  sorts timer kinds by elapsed seconds into bands, and a fourth kind eats
  another's expiry. `main.rs` is `test = false`, so none of it is covered.
- **A crashed launch permanently shrinks the fleet.** | The next launch records
  what the dead session managed to bind as the truth. Watched it go 5 → 4 on
  2026-09-16. | **Open** — a real second defect, not yet fixed, and independent
  of the handles. `bound_since_launch` guards only the zero-bind case.
- **Phase 6c's verdict passed by hand, all seven checks.** | Run against the
  live restored session with `scripts/qa/relaunch-verdict.sh`. | **Checked** —
  but against a 4-row set from a CRASHED session, and it was the OLD all-at-once
  restore. It says nothing about `0c6e286`.
- **The sandbox's minted row has no transcript, so `clave spawn` refuses it.** |
  The drive creates it from hooks alone. | **Checked** — reads like a product
  defect on screen and is not one.

**Method.** Sample before theorising: two plausible mechanisms (a probe storm,
a plugin-cache cost) were both wrong, and the sampler settled it in one run.
Read `clave.log` in the sandbox state dir before believing a screenshot — it
proved a spawn had NOT come from `Alt+Enter`. Run `bash
scripts/qa/lib-selftest.sh` after any `scripts/qa/lib.sh` edit; it is under a
second, and the drive costs two maintainer launches.

**Proof.** `just gates` exits 0, 11 suites, 790 tests.
`bash scripts/qa/lib-selftest.sh` prints `0 failure(s)`.

## Task Overview

#261: the fleet you had open comes back; a tab you closed stays closed. Success
is the maintainer keeping it — a clean twelve-phase drive, then his go to push,
update the PR body and merge.

## Reference Docs

- `docs/dev/QA-DRIVE.md` — the ledger. Runs 13-18 are the useful history.
- `docs/FOOTGUNS.md:214-216` — this branch's entries.
- `docs/UBIQUITOUS_LANGUAGE.md:211-214` — `exited row`, the six `Status`
  variants.
- `/tmp/clave-pr-body.md`, `/tmp/clave-swarm/` — **tmp, may be gone.**
  Regenerate the PR body from the commit log if needed.

## Current State

Tree CLEAN. **22 commits ahead of `origin/worktree-live-set-restore`, NOT
PUSHED.** Gates green, 790 tests.

Newest first:
- `0c6e286` the staggered restore — the resource blocker
- `c919233` handoff
- `1b52f41` the relaunch verdict refuses a stale status
- `a256631` the launch clears status

New files this session: `scripts/qa/fd-sampler.sh` (handle sampler),
`scripts/qa/relaunch-verdict.sh` (phase 6c's verdict, standalone).

A sandbox may still be live: `clave-test-live-set-459d`. **The maintainer
authorised the agent to run `zellij kill-session` by that explicit name**, and
it worked once — earlier attempts were refused by the safety classifier, so
expect it to be unreliable and ask if refused.

## What's Working

- **The restore itself is correct**, proved across a real relaunch: right rows
  back, closed tab stayed closed, rows bound before their agents ran, no stale
  status. Build on it; do not re-litigate it.
- **`relaunch_checks` in `scripts/qa/lib.sh`** is the home for relaunch
  verdicts, and every check in it has a selftest case that makes it go red.
  Copy that shape. The fixture must carry every field the check reads.
- **`Effect::OpenAgent`** is the one open path, shared by `Alt+Enter` and the
  restore. `clave open` is a no-op on a live row, which is why two bars cannot
  double-open the same tab.
- **The one-line wire at the end of `apply_snapshot`** is the whole sequencer.
  Keep it there; a timer is the thing to avoid.

## What success looks like

Twelve green phases on a drive of `0c6e286`, with no "Too many open files" and
the fleet coming back complete. Then push, PR body, merge.

## Important Discoveries

**Why the crash was new.** Before this branch a launch built ONE tab, because
the restore did not work — the bug we fixed. Building the fleet at once was
therefore a load nothing had ever placed on zellij. The maintainer's own fleet
is bigger than the sandbox's, so the feature would have failed on exactly the
fleets it exists to serve. His question — "the same session is now costing
more, and it's a small number of tabs" — is what forced the measurement instead
of another guess.

**Two wrong theories, both discarded on evidence.** (1) A probe storm: each
restored bar interrogating every pane, quadratic in fleet size. Refuted —
`probe_term_facts` is gated on `own_tab_focused`, so only one bar probes. (2) A
cold plugin cache forcing simultaneous wasm compiles. Refuted — the module
cache is shared across sessions. The handles were **pipes**, 198 of them in one
instant, which is process creation, not plugin loading.

**The sampler had a bug that cost a launch.** It counted loop ITERATIONS as
seconds, so a 900-second wait gave up after 450. It timed out before the
maintainer launched, and the first restore crash went unmeasured. Fixed: it now
counts real seconds and polls with no sleep, because the server can die within
a second.

**Adding a field to `AgentSnapshot` breaks 39 struct literals.** They are
exhaustive. A scripted insert is fine but matches `-> AgentSnapshot {` in
function signatures and the struct definition too — check those three shapes if
you add another field.

## Next Steps

1. **Drive `0c6e286`.** Stage with `./scripts/sandbox-setup.sh qa-fleet`, then
   the maintainer launches. Start `scripts/qa/fd-sampler.sh
   clave-test-live-set-459d` first — the peak handle count is the evidence the
   fix works, and it should now stay near one tab's worth.
2. **Watch the restore actually complete.** The tabs arrive one at a time over
   several seconds. `last_live` says how many are owed; the store's bound rows
   say how many arrived. A stall means the chain broke — the most likely cause
   is a row whose open failed, which halts the queue by design.
3. **Record the run** in `docs/dev/QA-DRIVE.md`, and add a FOOTGUNS entry for
   the handle burst.
4. **Consider a drive assertion** that the restored fleet comes back COMPLETE,
   since nothing currently fails if the queue stalls halfway.
5. **Then the crash-shrinks-the-fleet defect** (see Orientation). Probably its
   own issue rather than more scope on #261 — ask him.
6. **On his go only:** push, PR body, merge.

Verbatim, where the design came from:

> Yeah, I think it should bring back the one that's opened on anyway, then just
> go vertically down the list in the background bringing them up, a few seconds
> between each. And if the user navs to a tab before it's brought to life, it
> gets brought back as they land on it. It's the most straightforward
> implementation, and then we bring them back sequentially.
> And then, what we can do to protect our resources and be efficient in these
> scenarios is important.

Implemented as written, with one deliberate change: **the spacing is not a
fixed number of seconds** but "when the previous tab has appeared", which is
self-pacing and needed no timer. Tell him this; he has not seen it run.

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

**Decision ledger (this session):**
- The launch bakes ONE tab; the bar opens the rest. The one-row limit is the
  fix, not a detail of it.
- No timer. The store advance is the clock.
- Only the focused bar drives the sequence.
- The restore owes each row exactly one open, latched — a tab the human closes
  stays closed.
- Deferred is not dropped: a deferred row still comes back.
- A launch clears every status; status is session-scoped like tab and pane ids.
- Do not raise the file-handle limit to make a test pass.

**Harness friction.** The worktree isolation classifier refuses compound bash
with runtime values, heredocs feeding python, and `sed` with computed programs.
Use the Write tool for scripts, then run them as a single plain command.

## Restart Hint

Tree clean, gates green, 790 tests, 22 commits unpushed. Nothing mid-edit. The
next action needs the maintainer: stage the sandbox and ask him to launch.

## Suggested Skills

- `superpowers:test-driven-development` — every defect on this branch was
  invisible to a green suite.
- `catch-up` — for full branch context before touching anything.
