# 2026-09-09 15:08 — nav latency: the beacon is off the CLI pipe (#141)

The work in the previous handoff is DONE and in a PR. This file exists so the
next session inherits the measurements and the two decisions that were made
along the way, not so it re-opens the task.

## What shipped

Branch `worktree-nav-pipe-latency`, PR open against `main`.

The beacon announce — the message one sidebar sends to converge the other nine
after a nav or a click — no longer shells out `zellij pipe`. It goes through
zellij's in-plugin channel (`pipe_message_to_plugin`).

**The load-bearing question is settled twice over.** It was: does that channel
reach instances whose tab is NOT focused? The whole point of the beacon is
converging sidebars nobody is looking at, and zellij's delivery rules differ
per channel — `TabUpdate` reaches only the active tab, which has burned this
repo before.

- **From source.** With neither `plugin_url` nor `destination_plugin_id` set,
  the server takes the `(None, None)` arm and calls `pipe_to_all_plugins` over
  `all_plugin_ids()` — the whole plugin asset map, no tab or focus filter
  anywhere in the path (`zellij-server-0.44.3`, the `MessageFromPlugin`
  handler's `(None, None)` arm at `plugins/mod.rs:1128-1138`, then
  `plugin_map.rs:211`). That is the same function a payload-only `zellij pipe`
  reaches, which is what makes this a channel swap and not a semantic one.
  An independent reviewer re-read the whole server path and confirmed it,
  adding two facts worth keeping: pipe delivery is NOT subscription-gated
  (`wasm_bridge.rs` binds `_subscriptions` and never reads it), and a denied
  permission is LOGGED, not silent (`zellij_exports.rs`).
  Note `zellij-server` is NOT vendored locally — only `zellij-tile` and
  `zellij-utils` are, because those are the plugin's own dependencies. Fetch it
  to read it: `curl -sL
  https://static.crates.io/crates/zellij-server/zellij-server-0.44.3.crate | tar xz`
- **Live.** 30 gestures against a ten-sidebar sandbox produced **300 beacon
  deliveries across 10 distinct instances** — every sidebar received every
  beacon, and only one of the ten is focused at any moment.

Naming our own URL instead would route through `get_or_load_plugins`, which
matches on plugin CONFIG as well as location and can LAUNCH a plugin when none
matches. Narrower, riskier, no gain. Recorded so nobody "tightens" it later.

## The measurements

`scripts/nav-bench.sh`, 30 scripted gestures, ten bar instances, median gap
between landings. Runs were stable to ±3 ms.

| build | median | p90 | vs before |
| --- | --- | --- | --- |
| CLI announce + 10 blank twins (before) | 160 ms | 170 ms | — |
| in-plugin, no beacon line | 119 ms | 124 ms | −26% |
| in-plugin + 10 beacon lines (SHIPPED) | 135 ms | 142 ms | −16% |

Two things make those numbers honest, and both are in the script's header:

- **Fan-out is the variable.** One instance measured 75 ms on the same build
  where ten measured 160 ms. This is why the sandbox always felt faster than
  the live fleet, and it is measured now rather than hypothesised.
- **The scripted stimulus is not free.** There is no way to press a keybind
  from a script, so the bench pokes the bar with `zellij pipe` — itself a CLI
  pipe. A real keypress pays none of that, so the absolute figure OVERSTATES
  what a human feels and only the delta between builds means anything. On a
  real keypress the announce was the only CLI pipe on the nav path, and it is
  now zero.

Corroboration: `Action CliPipe did not complete within 1s timeout` lines fell
from 4 per gesture to 2 — exactly the arithmetic of removing one of the two
CLI pipes a scripted gesture pays.

## Two decisions, both the maintainer's, both settled

**1. The beacon log line stays always-on, not behind a flag.** It costs ~16 ms
a gesture at ten sidebars — about 40% of the win. Kept because it is the only
thing that separates a dead-looking nav's two causes: beacon lines present with
no landing line means the executor election refused; no beacon lines means the
channel starved. Those are indistinguishable otherwise and read the wrong way
for a day in #162.

`clave-bar` ALREADY has a verbose switch — `dbg_log()` over `CLAVE_BAR_DEBUG`
— so gating it would have been one `if`, not new surface. An earlier
answer in this session said otherwise and was corrected. The real reasons it
stays ungated: this file's own rule puts per-FRAME lines behind the flag and
rare-but-diagnostic ones (the click line) in front of it, and a gesture is not
a frame; and `CLAVE_BAR_DEBUG` is read once from the SERVER's environment, so
enabling it means relaunching the fleet, which destroys the intermittent state
you were trying to observe. **#257** holds this decision with the numbers, so a
later perf pass can re-decide without re-measuring. If it ever must be
switchable, the shape is a runtime pipe, not an env read.

**2. The blank twin's replacement is better, not merely equal.** The old CLI
announce made zellij append a blank message per instance (#45), and the field
counted those as fan-out evidence. They were an artifact: no payload, present
only because the channel was a subprocess, in a log shared by every session on
the machine — countable, never attributable. The beacon line is the same volume
with the tab in the text and the instance id in the log's own column.

Be precise about what that buys, because an earlier draft overclaimed it. The
line is attributable to a PLUGIN ID, not to a session. Plugin ids are
per-server and the log still carries no session identity, so two live clave
sessions interleave beacon lines under colliding ids. Counting instances is
only sound with one session running; `scripts/nav-bench.sh` now detects that
and warns.

Also killed while in this code: the `QA pipe-delivery P8` comment that
justified keeping the blank-twin drop line. The drive runs P0-P7 and has never
had a P8. That was flagged in the previous handoff and is now resolved in the
comment.

## Verification

- `just gates` green: **355** / 11 / 15+1 / **256** / 25. One test over the
  previous baseline — `the_bars_permission_ask_matches_the_seeded_grant`.
- `scripts/qa-drive.sh qa-fleet` — **all 8 phases PASS**, driven twice: once on
  the channel swap, once on the final tree. Both eyeball checkpoints given.
- `just mutants main --jobs 2` — clean, but **vacuously**: it reported "No
  mutants to filter". The diff is a channel swap inside an effect arm, a const
  list and log lines, so nothing in it has a mutable return value. Say this
  plainly rather than calling it a pass.
- **CodeRabbit CLI** — 3 minor, all accepted (bench slice truncation, unvalidated
  gesture count, unlabelled before-figures).
- **Independent adversarial reviewer** — 10 findings, all accepted and fixed.
  The load-bearing delivery claim survived it. The one that mattered most is
  in the permission section below; the rest were the bench script's stdin
  guard and swallowed stderr, two false statements in comments, the
  session-attribution overclaim, and two holes in the new lockstep test (a
  second `request_permission` call would have been invisible to it, and it
  compared in order where zellij's own check is containment — both closed).

**The one thing that is NOT verified, and it is the acceptance test.** The
maintainer mashing `Alt+Up`/`Alt+Down` in his OWN ten-tab fleet and it keeping
up with him. That needs the change in his fleet, which needs a release, which
is his to run. Everything above is a sandbox with idle sidebars, and the
previous handoff's own finding is that a sandbox understates per-delivery work.
**Ask for it after the merge and the cut, and do not claim the fix worked until
it is in.**

## The permission, which is the upgrade hazard

`pipe_message_to_plugin` is gated on `MessageAndLaunchOtherPlugins`, because
the same call can launch a plugin — the check is on the command, not its
arguments. So the bar's ask and `BAR_PERMISSIONS` both grew by one.

Grants are all-or-nothing per plugin and the prompt is unanswerable in a bar
pane, so a sidebar asking for a permission the cache does not hold hangs every
pipe with nothing in the build to say why. `clave setup` re-seeds the cache, so
a normal release path is fine — but **an existing session must be relaunched
after the cut**, and that is the same rule as any keybind change.

A new test reads the bar's `request_permission` block out of the source and
asserts it matches `BAR_PERMISSIONS` as a SET, because the two lists live in
different crates and the bar's is wasm-only, so no shared const can reach it.
That guard did not exist before; the doc comment had asked for it in prose for
months.

**And `permissions_seeded` — what `clave doctor` reports — was blind to exactly
this failure.** It checked only that the wasm key existed in the cache, not
which permissions sat under it, so a cache holding a PREVIOUS release's shorter
set read as green while every pipe to that bar was dead. The reachable path is
the dev loop, not a release: `just dev-install` overwrites the sandbox wasm IN
PLACE so the cache key does not change, and `dev launch` skips `run_setup`
because a dev build has no embedded wasm to refresh against. A release is
immune for the opposite reason — the versioned filename changes, so the merge
seeds a fresh key with the current set. Found by the adversarial reviewer;
`permissions_seeded` now checks the set inside our own node, with a test for
the stale-shorter-grant case and for a neighbouring plugin's grant not
satisfying ours.

## Reference

- Issue **#141** — the task. Its two comments carry the pre-work measurements.
- Issue **#257** — the log line's cost, the declined flag, and the correction.
- `crates/clave-bar/src/main.rs` — the effect arm (the swap and why the
  delivery set is unchanged), the `clave-visited` receive arm (the beacon line
  and why it is ungated), the permission block.
- `crates/clave/src/setup.rs` — `BAR_PERMISSIONS` and the lockstep test.
- `scripts/nav-bench.sh` — the bench, with both caveats in its header.
- `docs/dev/TESTING.md` — the two nav lines and the discriminator they form.

## Restart hint

Nothing to unwind. Worktree `.claude/worktrees/nav-pipe-latency` on branch
`worktree-nav-pipe-latency`, sandbox `clave-test-nav-pipe-a97e` killed and
deleted. If the PR is merged, the only work left is the cut and the
maintainer's key-mash. If review reopens something, the bench and the drive are
both one command each and the sandbox stages with `just sandbox qa-fleet`.
