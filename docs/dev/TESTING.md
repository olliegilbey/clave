# Live-validation SOP

clave drives a live terminal multiplexer. **No automated test can cover live
behavior** — pane geometry, focus, glyph repaints, resurrection, the toggle
reflow. Those are real Zellij, real `claude`, real eyes on the screen. This
document is the method the maintainer and his agent use to validate that
behavior. It cannot be derived from the code; it is written down here because it
is the only way the next session knows how to work.

Read it whole before you touch anything that renders. If you are an agent, the
single most important line is the one in the next section: **you read
observability, you do not drive the terminal.**

## The interaction contract

There is a hard division of labor, and it is not negotiable:

- **The human drives all live input.** Every keypress, every session launch and
  kill, every visual observation — the human. They are the only one who can see
  the screen and the only one who should touch it.
- **The agent reads observability and never puppets the live session.** You read
  logs, the store, and `dev status`. When a session needs to be launched or
  killed, you **print the exact command for the human to run** — you do not run
  it. clave's own `dev` subcommands follow this rule by construction: `dev
  reset` prints the kill-session command rather than executing it, and the
  module never launches or kills a Zellij session itself.

Why so strict? Because the failure modes here are silent and expensive (see the
hazard below), and because the human's screen is ground truth. An agent that
"helpfully" runs a `zellij action` can hang the whole loop with no error.

## Agent-side sanctioned commands

An agent may run **exactly these**, and every one is scoped to the test session.
Nothing else touching Zellij is yours — session lifecycle is always the human's.

**Hot-reload the sandbox bar** (the one sanctioned live mutation an agent may
make — see the instrumentation recipe):

```bash
ZELLIJ_SESSION_NAME=clave-test zellij action start-or-reload-plugin \
  "file:$HOME/.local/state/clave-dev/data/clave-bar.wasm"
```

**List sessions** (read-only, safe anywhere):

```bash
zellij list-sessions
```

**Dump the sandbox layout** (env-scoped to `clave-test` — pane geometry truth):

```bash
ZELLIJ_SESSION_NAME=clave-test zellij action dump-layout
```

> **Hazard — this blocks forever.** A `zellij action` aimed at an absent or dead
> session **blocks indefinitely and never errors**. An ungated `dump-layout`
> once hung `dev status` for minutes before a session existed. Always gate on
> liveness first. `clave dev status` does exactly this: it checks the session is
> live before it ever runs `dump-layout`, and returns an empty dump otherwise.
> When in doubt, ask `dev status` — never fire a bare `zellij action` on faith.

## The sandbox lifecycle

The full reset-to-fresh cycle. Commands marked **(human)** are the human's to
run in a **non-zellij terminal**; the rest an agent may run.

1. **Kill the running session (human):**
   ```bash
   zellij kill-session clave-test && zellij delete-session --force clave-test
   ```
   (`clave dev reset` prints this line for you before it wipes anything.)
2. **Reset the sandbox:** `clave dev reset` — wipes the sandbox root and removes
   the scenario transcripts (see *What reset removes*).
3. **Seed a scenario:** `clave dev scenario <name>` — creates the fixture world
   and prints the launch command.
4. **Launch (human, non-zellij terminal):** `clave dev launch` — attaches or
   creates the `clave-test` session against the sandbox state and data dirs.

### Scenario catalog

The scenarios are defined in `crates/clave/src/dev.rs` and map 1:1 to the C8
steps in the validation ledger. Each seeds a **real** resumable `claude`
transcript (a few tokens via `claude -p`) plus a store row, so resurrection is
verified for real, not mocked. UUIDs are deterministic and self-identifying:
`00000000-0000-4000-8000-c85c…` (`c85c` ≈ "c8 scenario").

| Scenario | Seeds | Validates |
|---|---|---|
| `c8-cold-start` | 3 agents at staggered recency (60s / 1h / 24h ago), none worktree | Cold relaunch: most-recent agent resumes focused with history, no ENTER gates; the rest sit dormant `◌` in recency order |
| `c8-worktree` | 2 agents; one in a real `git worktree` | Dwell-open resumes the agent **in its worktree path**, store row intact |
| `c8-stale` | 2 agents; one has its cwd deleted after seeding | The staleness branch (§6.3): dwelling the row → `✗`, no tab created, the session is unaffected |

### What reset removes

`clave dev reset` removes two things and nothing else:

- **The sandbox scenario state** — `~/.local/state/clave-dev/state/` (store,
  evlog) and `~/.local/state/clave-dev/repos/` (seeded fixture repos). The
  `data/` dir **survives**: it holds the sandbox wasm and generated config, a
  build artifact installed by `just dev-install`, not scenario state — wiping
  it would break the reset → scenario → launch loop with a rebuild demand.
- **The scenario transcripts in the real `~/.claude/projects/`.** Because
  Claude's identity is deliberately *not* sandboxed, those `claude -p` seed
  transcripts land in your real Claude tree. Reset finds them by their
  `c85c`-tagged UUID prefix (the `is_scenario_jsonl` filter) and removes exactly
  those, leaving every real transcript untouched.

**Claude identity is not sandboxed** (ruling, 2026-07-18). The sandbox isolates
clave state only. `claude` runs as the real you; the seed hooks still land in
the sandbox store because they inherit `CLAVE_STATE_DIR` from their `claude`
parent process. Do not try to re-sandbox it — CLAUDE_CONFIG_DIR isolation broke
auth, and that is why the ruling exists.

## The observability map

Four windows into what actually happened. Learn all four.

- **Zellij plugin log** — where `eprintln!` from the wasm bar surfaces:
  ```
  $TMPDIR/zellij-<uid>/zellij-log/zellij.log
  ```
  **This file is shared by every Zellij session on the machine, and old entries
  linger.** Always filter by **today's date AND the build tag** — an unfiltered
  tail is a mix of every session's history and will mislead you.
- **The evlog** — `clave.log`, JSON lines, one per host-side decision. There is
  one per state dir: `~/.local/state/clave/clave.log` for stable,
  `~/.local/state/clave-dev/state/clave.log` for the sandbox.
- **`clave dev status`** — the agent's primary probe. Emits JSON:
  `session_live` (bool), `live_uuids` (parsed from the layout dump), and the
  full `store`. Liveness-gated, so it is always safe to run.
- **Env-scoped `dump-layout`** — `ZELLIJ_SESSION_NAME=clave-test zellij action
  dump-layout` — the ground truth for pane geometry and the serialized spawn
  commands. Gate on liveness first (see the hazard above).

## The instrumentation recipe

This is the debugging loop that has found real bugs (the C6 announce storms, the
C8 fixed-pane resize bug). Follow it in order:

1. **Add a temporary `eprintln!`** with a grep-able marker prefix, e.g.
   `eprintln!("CLAVE_DBG_seek cols={cols}")`.
2. **Rebuild the wasm with a fresh build tag:**
   ```bash
   CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S) \
     cargo build -p clave-bar --target wasm32-wasip1 --release
   ```
3. **Copy it into the SANDBOX data dir only** — never the stable install:
   ```bash
   cp target/wasm32-wasip1/release/clave-bar.wasm \
     ~/.local/state/clave-dev/data/clave-bar.wasm
   ```
4. **Hot-reload** with the sanctioned command from above (env-scoped to
   `clave-test`).
5. **The human exercises the behavior** on screen.
6. **Read the zellij log filtered by your marker AND the build tag** — both, so
   you are reading this run and not a ghost of an earlier one.
7. **Strip the instrumentation before committing.** Temporary means temporary.

> **Caveat — hot-reload resets plugin state.** A reload reincarnates every bar
> model from scratch. That is a confound (state you were watching is gone) *and*
> a tool (it clears a parity desync — see C8 — and re-hydrates cleanly, which is
> how C9 hydration was validated). Know which one you are relying on.

## Lore discipline

Two habits, both born of expensive lessons.

**Read the C-section before you touch the subsystem.**
[`SUBSYSTEM-VALIDATION.md`](../superpowers/spikes/SUBSYSTEM-VALIDATION.md) is the
ledger of what was tried and why it failed. Every entry is a dead end someone
paid for:

- **C6 (toggle / `hide_self` reflow)** is the saga of the announce storms —
  round after round of `is_active_instance` self-diagnosis poisoning during
  event bursts, until the announce was reduced to bounded birth/organic
  triggers. It is also where `show_self()` was found (in the *vendored* Zellij
  source) to be a focus action that switches tabs. Do not reintroduce
  suppress/hide-self tricks, fixed pane sizes, or self-diagnosed announces
  without reading it first.
- **C8 (resume + resurrection)** is why serialization is off and resurrection is
  clave-owned and lazy — serialization records the discovered child process, so
  a serialized `claude --session-id` would collide against an existing jsonl.
  It is also where the fixed-pane resize bug and the collapse **parity-desync**
  bug class live. This is the section your `clave dev` scenarios exercise.

Before touching a subsystem, read its section — the forbidden approaches were
each expensive to learn.

**Never trust assumed Zellij semantics — read the vendored source.** Behavior
that "obviously" works one way has repeatedly worked another (`TabUpdate`
reaches only the active tab; `resize_pane_with_id` silently refuses fixed panes;
`show_self` is a focus action). The vendored crates are here:

```
~/.cargo/registry/src/*/zellij-tile-0.44.3/
~/.cargo/registry/src/*/zellij-utils-0.44.3/
```

`zellij-server` is not vendored but is fetchable from crates.io — several C6/C8
findings were confirmed against it. Read the source before you build on a
behavior, and record what you find in the ledger in the same commit as the
change.
