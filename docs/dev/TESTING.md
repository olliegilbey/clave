# Testing clave — verification tiers, risk taxonomy, and the live SOP

clave has three tiers of verification and they are not interchangeable. Most of
this document is **Tier 3**, the live-validation SOP: hard-won method that cannot
be derived from the code. But the live SOP is the tier that *requires the
maintainer at a terminal*, and treating it as the whole strategy is how the
v0.1.1 field incident happened — a correct release, a wrong binary on `PATH`, two
sidebars, dead navigation, and not one automated test in a position to notice
(#43, #44; CONTRIBUTING "The one leak").

So: read the tiers, find your change in the **risk taxonomy**, produce what that
row demands, and record it in the PR dossier. The **escape record** at the end is
the evidence the taxonomy is built from — every row is a defect that actually got
through, and the tier that would have stopped it.

## The verification tiers

### Tier 1 — hermetic

Runs anywhere: no TTY, no zellij, no `claude`, no auth, no network. This is the
tier an agent completes unattended, and it is the whole of CI today
(`.github/workflows/ci.yml`: test, wasm-build, lint).

| Instrument | Where | What it holds |
|---|---|---|
| Unit tests | across both crates; **63 in `crates/clave-bar/src/model.rs`** | the bar's state machine — the pure event→effect core, superbly covered |
| Proptests | `model.rs` (`proptest` is a `clave-bar` dev-dep) | invariants over generated event sequences; extend them whenever a new branch becomes reachable |
| Real-KDL-parser guardrail | `crates/clave/tests/kdl_guardrail.rs` | every generated artifact (config/layout/launch, the one-shot tab layout, the permission cache) parsed by the **exact** zellij-utils 0.44.3 parser. Substring tests assert *content*; this asserts *validity* — a dropped brace or a missing trailing `;` otherwise fails at session launch, where a dead `attach` blocks forever |
| Version-pin tripwire | `crates/clave/tests/zellij_pin_tripwire.rs` | every zellij-family crate in `Cargo.lock` resolves to **one** version, so the guardrail can never green-light templates against a parser the plugin no longer runs |
| CLI parse pins | `Cli::try_parse_from` tests in `crates/clave/src/main.rs` | that each plugin-invoked subcommand parses the literal arguments the plugin passes. Added after the `ArgAction` escape; required for **every new surface** |
| Sandboxed subcommand e2e | `CLAVE_STATE_DIR=<scratch> cargo run -p clave -- …` | one real end-to-end run of a new subcommand against a scratch store. Do it in a **debug** build — clap's `debug_assert` only fires there |

The gate:

```bash
cargo test --workspace   # --workspace is load-bearing: default-members excludes clave-bar
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

A bare `cargo test` exits 0 while skipping every one of the 63 model tests. Use
`just test`.

### Tier 2 — real-zellij integration (**does not exist yet — #47**)

Blocked on **#44** (the plugin must be told which binary to call instead of
resolving bare `clave` through `PATH`), because the harness needs exactly that
injection point to aim a session at a test binary. Ruling (2026-07-22): build it
immediately after #44.

The shape, so you know what is coming and can design toward it:

- **Isolation by construction** — the primitive already exists as the
  `CLAVE_SESSION` / `CLAVE_STATE_DIR` / `CLAVE_DATA_DIR` triple. A run creates
  `clave-it-<pid>` against temp dirs, so it can never touch the maintainer's
  `clave` session. The standing safety boundary holds mechanically, not by
  discipline.
- **The pane command is injectable** — tests spawn `sleep`/`cat`, not `claude`.
  CI therefore needs zellij on `PATH` and nothing else: no Claude Code, no auth,
  no network. That is what makes it cloud-agent runnable.
- **Method** — spin the session from generated artifacts, drive it with `zellij
  action`, assert against `dump-layout` / `dump-screen`, tear down in a guard
  that runs on panic.
- **Hazards to design around** — a `zellij action` against a dead session blocks
  forever, so every call needs a liveness gate *and* a timeout or CI hangs
  instead of failing; a PTY is required (`script -q` or a pty crate); flake
  quarantine and retry policy get decided up front, not bolted on.
- **First scenarios** — one bar per tab (the #44 regression), nav after an
  `Alt+w` close (#23), bind/prune round-trip through the store (#6), tab creation
  via `clave open`, collapse toggle parity (#5).

**Until it lands, treat this list as UNGUARDED.** Nothing automated today
executes the `clave` binary against a real zellij, so every process- and
environment-seam property is unverified: which binary answers a plugin shellout,
whether one session ends up with one bar per tab, whether navigation survives a
close, whether pipe deliveries arrive, whether the generated artifact set agrees
on a single version. A change touching that seam must carry a written argument in
the PR and an adversarial reviewer — that is the only guard there is.

One cheap piece is **Tier 1 and can be done today**: the pure-function assertion
proposed on #48 that the generated artifact set references exactly one version
and that every referenced path exists. That alone would have failed on the
mixed-version `launch.kdl`.

### Tier 3 — human, at a terminal

Everything below the taxonomy. Glyph fidelity, colour, Nerd Font coverage, pane
geometry, focus, repaint, resurrection, the toggle reflow, and "does this feel
right" on the maintainer's real fleet. No tier below it can substitute; the human
is ground truth.

Two labels carry this into the process:

- **`needs-live-validation`** — merged, but a maintainer pass is owed before the
  next tag. The PR must state its live steps, numbered, sandbox-first, each with
  its expected observation.
- **`host-untestable`** — human judgement only, permanently. Nothing automated
  will ever cover it.

`main` is guaranteed green, reviewed and hermetically verified; it is **not**
guaranteed live-validated. The **tag** is the promotion event, and #49 batches
every `needs-live-validation` PR merged since the previous tag into one focused
maintainer pass per cut. That is what keeps the maintainer the *last* line of
defence rather than the first — which is precisely how the v0.1.1 cycle went
wrong.

## The risk taxonomy

Find your change's class. The right-hand column is what the PR dossier must show
before you ask for a merge.

| Change class | Examples | Required before requesting merge | Label |
|---|---|---|---|
| **Pure logic / model** | `model.rs` state machine, store transitions | TDD red-first; `cargo test --workspace` (`--workspace` is load-bearing); extend proptests if a new branch is reachable | — |
| **Generated artifacts** | `config.kdl` / `layout.kdl` / `launch.kdl` generation | + real-parser guardrail + version-coherence and path-existence assertions | — |
| **CLI surface** | new subcommand or flag | + `Cli::try_parse_from` pin + one sandboxed end-to-end run in a **debug** build (clap's `debug_assert` only fires there) | — |
| **Cross-process / IPC** | pipes, plugin shellouts, multi-writer store paths | + written argument for ordering/idempotency in the PR dossier; adversarial reviewer must attack it; tier-2 coverage once #47 lands | — |
| **Install / environment** | release mechanics, dev-install, `PATH`, doctor | + fresh-environment reasoning; assume nothing about the maintainer's machine | `needs-live-validation` |
| **Visual / UX** | glyphs, colours, widths, fonts | human judgement only | `host-untestable` |

Why each row is what it is:

- **Pure logic / model** — this is the tier that already works, and the reason it
  works is red-first discipline plus proptests. The failure mode is not weak
  coverage but *unreached* coverage: the stale width-seek anchor was pure logic
  living behind an interrupt shape the generator never produced (#4). A new
  branch without a new property is a new blind spot.
- **Generated artifacts** — a KDL string that *contains* the right substrings can
  still be structurally invalid, and it fails at **session launch**, the worst
  possible place: a dead `attach` blocks forever and the human sees nothing. The
  parser guardrail catches invalidity; version-coherence catches the other half —
  a perfectly valid `launch.kdl` baking `v0.1.0` paths inside a `v0.1.1` session
  is what produced two sidebars (#43).
- **CLI surface** — the CLI layer had *no* coverage at all until the `ArgAction`
  escape, because the plugin is the only caller and nothing in the suite invoked
  it. clap-derive turns a bare `bool` field into a flag, so `clave collapse true`
  could never parse — and the diagnostic `debug_assert` fires only in debug
  builds, which is why the e2e run must be a debug build.
- **Cross-process / IPC** — no test in any existing tier models *arrival order*
  between two fire-and-forget subprocesses. The prune race (#6/#26) was found by
  a reviewer reasoning about ordering, not by execution, and the fix was to make
  the payload idempotent and commuting rather than to test harder. Until #47,
  written argument plus adversarial review **is** the verification.
- **Install / environment** — these changes are validated against exactly one
  machine, the maintainer's, and that machine already has state on it. The
  v0.1.1 incident was a `PATH` collision that no clean checkout would ever show.
  Reason from a fresh environment, and assume the maintainer is daily-driving:
  never `cargo install` or `just dev-install` from a working session.
- **Visual / UX** — no automated tier will ever adjudicate a glyph, a colour, or
  whether the reflow feels right. Label it and write the steps.

## The escape record

The taxonomy is derived from these, not asserted. Every row is a real defect that
reached `main` or the field.

| Escaped | Where it bit | Why no tier caught it | What catches it now |
|---|---|---|---|
| `clave-bar` shells out to bare `clave` through `PATH`, in 7 places (#44) | v0.1.1 daily driving, 2026-07-22: a stale `0.1.0` dev binary served `clave open` inside a `v0.1.1` session and composed tab layouts pointing at the old wasm → **two plugin populations**, duplicate sidebar, half-dead nav | no test executes the binary, and none runs a real zellij — the process seam is entirely unmodelled | #44 injects the absolute binary path at config-generation time; #47 (tier 2) asserts one bar per tab; #48 (`clave doctor`) asserts version coherence live |
| No unversioned stable entry point; `just dev-install` writes `~/.cargo/bin/clave` (#43) | same incident: whatever `clave` resolved to won the cold start and generated a mixed-version `launch.kdl` | the KDL guardrail asserts *validity*, nothing asserted *coherence* across the artifact set; the failure is invisible until it manifests as duplicated UI | the #48 pure-function test (one version, all paths exist) — tier 1, cheap, would have failed on that `launch.kdl` |
| clap `ArgAction` on the `collapse` bool positional (#5, PR #13) | `clave collapse true` could not parse at all; debug builds trip clap's own `debug_assert` on every parse | **nothing exercised the CLI layer** — the plugin was the only caller and no test invoked it | `Cli::try_parse_from` pin per subcommand + one sandboxed debug e2e; both now taxonomy requirements. Found by CodeRabbit CLI, an independent lane |
| Full-live-set prune payload was order-unsafe (#6, PR #26) | two fire-and-forget `clave prune-tabs` subprocesses have no arrival order; a "retain these live ids" payload landing after a new tab's bind unbinds a **live** agent, and `bind_effects` is `sent_binds`-guarded so it never re-fires → #6 double-attach via a race | model tests cover single-writer logic; nothing models cross-process arrival order | payload carries observed-**stale** ids (idempotent, commuting). Found by review, not by tests |
| Prune emission was set-change-gated (PR #26, Codex) | a close `TabUpdate` arriving before its `PaneUpdate` leaves `is_active_instance()` false, silently dropping the effect — and the gate then never retries | an event-interleaving shape the proptests never generated | emission is detection-driven and self-limiting via the store echo; `last_live_ids` deleted |
| Stale width-seek anchor (#4, PR #27, Codex) | drift gate measured against a mid-flight *emit* anchor, so the bar parked off-target (reproduced: 30→16→6, then an external 26) | pure logic **inside** a covered tier — the sim harness existed, but the proptest never generated that interrupt shape | `settle_at()` pins the anchor to the accepted rest width at every settle path, plus a pinned regression seed |
| `Alt+w` close stranded Alt-↑/↓ nav until a mouse click (#23) | live sessions only | the beacon/anchor relationship only exists once real tabs open and close | `Effect::ReanchorVisit`, executor-gated; tier 2 will assert it (#47 first scenarios) |
| `CliPipe did not complete within 1s` + empty-payload deliveries (#45) | present since the log's first line, v0.1.0 era; buried the real evidence during the v0.1.1 incident | no tier reads the zellij log; nothing asserts on pipe delivery | nothing yet — it is filed. Observability discipline (below) is the only detector |

Read the pattern before you argue with the taxonomy: **the pure state machine has
never been the problem.** Everything that escaped lived at a seam — process,
environment, event ordering, or the screen.

---

The rest of this document is **Tier 3 in full**: the live-validation SOP. It is
unchanged and load-bearing.

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
  "file:$HOME/.local/state/clave-dev/data/clave-bar.wasm" -c clave_binary=clave
```

> The `-c` is load-bearing, not optional. A plugin's configuration is half of
> its zellij identity, and `reload_plugin` matches on `(location,
> configuration)` exactly (`zellij-server/src/plugins/wasm_bridge.rs:686-697`).
> Without it the command matches nothing, the reload loop body never runs, and
> the command still **exits 0** — you would be validating stale wasm while
> believing the reload worked. The sandbox bakes bare `clave` (#44), so
> `clave_binary=clave` is the value there; a stable session would need its
> versioned absolute path. `PluginUserConfiguration`'s `FromStr`
> (`zellij-utils/src/input/layout.rs:563-576`) is comma-separated `key=value`,
> so a path containing a comma would not survive — none of ours do.

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
