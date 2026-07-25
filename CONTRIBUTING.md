# Contributing to clave 🥁

Welcome. This is the map: how clave is built, how a change gets from your
working tree to a release, and where the work is tracked. If you are a coding
agent reading this alongside your human, everything here is meant for you to
follow mechanically — the commands are exact, the paths are absolute, and the
one rule that matters most is that **you never drive the live terminal
yourself**. That belongs to the human. The full reasoning lives in
[`docs/dev/TESTING.md`](docs/dev/TESTING.md); read it before you touch anything
that renders.

clave is a Rust workspace: a host CLI (`crates/clave`, the `clave` binary) and
a Zellij plugin compiled to WebAssembly (`crates/clave-bar`, the sidebar). The
design and rationale live in [`docs/design.md`](docs/design.md); the
blow-by-blow record of what was tried and why lives in
[`docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md`](docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md).

## Two environments, one code path

clave builds its own daily driver: the maintainer runs terminal work *inside* a
clave session, and develops clave *from a project session inside that
environment*. So there are two launch surfaces, and they must never bleed into
each other. There is exactly **one code path** — the sandbox is the stable
behavior with three environment variables redirecting state and artifacts, so
it reproduces production faithfully by construction.

| | Day-to-day (stable) | Feature/dev (sandbox) |
|---|---|---|
| **Launch** | `clave` in a non-zellij terminal | `clave dev launch` in a non-zellij terminal |
| **Zellij session** | `clave` | `clave-test` |
| **State** (store, evlog) | `~/.local/state/clave/` | `~/.local/state/clave-dev/state/` |
| **Artifacts** (wasm, config, layout) | `~/.local/share/clave/` | `~/.local/state/clave-dev/data/` |
| **Binary** | versioned release copy (see below) | working tree via `just dev-install` |
| **Agents** | real work | synthetic — `clave dev scenario <name>` |
| **Teardown** | never | `clave dev reset` |

Two invariants hold everywhere:

- **No beta channel.** Promotion is one-way: validate in the sandbox → cut a
  version → it becomes stable. Nothing lives in between.
- **Claude's identity is never sandboxed** (ruling, 2026-07-18). The sandbox
  isolates *clave's* state only — store, session, config. `claude` always runs
  as the real you, with your real auth and your real `~/.claude`. Sandboxing it
  dragged auth along and broke session seeding; clave is a thin wrapper for
  terminal control, and your identity is not its business.

Both surfaces launch from a **plain, non-zellij terminal** — clave creates or
attaches the multiplexer session itself. Launching from inside a zellij session
nests them.

### The one leak: `clave` on `PATH` (#43, #44)

The table above says the two surfaces use different binaries, and the generated
`config.kdl` honours that — keybinds bake the *absolute* versioned path. But
`clave-bar` also shelled out to the CLI on its own (`snapshot`, `open`, `bind`,
`focus`, `touch`, `prune-tabs`, `add`), invoking plain **`clave`** resolved
through `PATH` — and `just dev-install` put a working-tree build at
`~/.cargo/bin/clave`, the same name the daily surface answered to.

So a dev build **could** drive a stable session, and on 2026-07-22 it did: a
stale `0.1.0` binary on `PATH` served `clave open` inside a `v0.1.1` session and
composed tab layouts pointing at the *old* wasm. Because zellij keys plugin
identity on file location, every tab opened that way loaded a **second bar** —
two populations, no shared beacon state, dead navigation. The release itself was
correct; only the binary the plugin reached for was wrong.

Three changes close it, and all three are needed — the leak had a producer, a
consumer, and a gap:

- **#44** — the bar no longer resolves through `PATH` at all. `clave_binary` is
  injected at config-generation time, so a running session can only ever invoke
  the binary it launched with.
- **#43a** — a cut installs an unversioned **launcher** at
  `~/.local/share/clave/bin/clave`, refreshed on every release. That directory
  is what you put on `PATH`; typing `clave` now means "the version I last
  released", which previously had no answer at all.
- **#43b** — `just dev-install` installs **`clave-dev`**. It writes no name the
  daily surface uses.

**If your machine predates this, it still has the stale file**, and it shadows
the launcher because `~/.cargo/bin` almost always precedes
`~/.local/share/clave/bin` on `PATH`. Identify it before you delete it — the
file could equally be a versioned copy someone put there on purpose:

```sh
command -v clave        # want ~/.local/share/clave/bin/clave
~/.cargo/bin/clave --version   # a dev build reports a short SHA or `dev`
rm ~/.cargo/bin/clave          # only once you have confirmed that
```

The residual rule is narrower but real: `just dev-install` rebuilds the
**sandbox** wasm in place, so don't run it against a live `clave-test` session.
`just sandbox` does the sandbox wiring and refuses when that session is live —
prefer it.

### The upgrade window: `config.kdl` is live-watched (#44)

Zellij **watches the `--config` file of every running session** and hot-swaps
the keybinds in place — `report_changes_in_config_file` (`zellij-server
src/lib.rs:2175`, ~1s poll) → `ServerInstruction::ConfigWrittenToDisk`
(`:2298`) → `ScreenInstruction::Reconfigure` (`screen.rs:717`). The **running
bar's identity is not swapped**: it is `initial_userspace_configuration`, fixed
at plugin load.

Since #44 the keybinds carry `clave_binary`, so regenerating `config.kdl`
against a live session re-keys its keybinds to a plugin identity that the
on-screen bar does not have. The next Alt+c / Alt+j / Alt+o misses its
destination, and zellij's response to a miss is to **start a new plugin** — a
second sidebar in every tab, dead navigation. Verbatim #43/#44, triggered by
installing the fix for it.

**So: any `just release`, `clave setup`, or `clave dev scenario` that changes
`clave_binary` or the wasm path requires restarting every affected session.**
Kill and relaunch before pressing any clave key. This bites hardest exactly
once — on the pre-#44 → post-#44 upgrade, where the old bar's configuration is
empty and the new keybinds' is not.

**Diagnosis is one grep**, because the bar logs its version at every load
(zellij's log lives under the OS temp dir, e.g.
`$TMPDIR/zellij-$UID/zellij-log/zellij.log` on macOS):

```sh
grep 'clave-bar: loaded' "$TMPDIR"/zellij-*/zellij-log/zellij.log | tail
```

Every line must report the **same** version. Mixed versions mean mixed plugins:
the symptom is a duplicate sidebar and half-working navigation. #44 removes the
leak by passing the binary's absolute path into the plugin at config-generation
time, so the bar can no longer be pointed at a different clave than the one that
launched the session.

## The release model

A version cut is a deliberate, tagged, reproducible act — never an accident of
`cargo install`.

- **Cuts are semver git tags on `main`** (`vX.Y.Z`; the first cut is `v0.1.0`).
  `main` is always releasable. You tag when you want a cut, not on every merge.
- **`just release`** is the only way to promote to stable. It refuses unless the
  working tree is clean **and** `HEAD` carries an exact `vX.Y.Z` tag matching
  the version in `Cargo.toml`. It then builds the workspace and the wasm plugin
  in release mode and installs *versioned* artifacts:
  `~/.local/share/clave/clave-bar-vX.Y.Z.wasm` and a versioned CLI copy at
  `~/.local/share/clave/bin/clave-vX.Y.Z`. It regenerates the stable
  `config.kdl` / `layout.kdl` and re-merges the Claude hooks so every generated
  reference — plugin location, keybind `Run` commands, hook commands — points at
  the versioned files.
- **The cut owns the launcher** (#43a). Alongside the versioned copy, a release
  installs — and on every cut *refreshes* — an unversioned **launcher** at
  `~/.local/share/clave/bin/clave`. That is the entry point you type, and
  `~/.local/share/clave/bin` is the directory to put on `PATH`. It is a copy of
  the versioned copy, installed by rename so a cold start already executing the
  old one keeps its inode. The launcher is **typed, never baked**: every
  generated reference stays versioned, because an unversioned plugin location is
  a different plugin identity to zellij — the #43 duplicate-sidebar shape.
- **A running session is immune to installs.** A live session only ever
  references the versioned files baked into the config it generated at launch.
  Installing a new release never overwrites a file a live session is loading; the
  upgrade lands atomically at the *next* `clave` launch. This is what makes the
  daily environment safe to develop clave from — see C8 in the validation ledger
  for the parity-desync bug class this design closes.
- **The binary split.** `~/.cargo/bin/clave-dev` (`just dev-install` from the
  working tree) is the **dev** binary — it is what contributors' shells run.
  Stable sessions never invoke it: their keybinds, layout, and hooks bake the
  absolute path of the versioned copy under `~/.local/share/clave/bin/`, and the
  daily launcher lives in that same directory. Since #43b the two surfaces no
  longer share a *name*, which is the property that failed in v0.1.1. One skew
  edge to know: if the binary you launch is *ahead* of the latest installed
  release (Cargo.toml bumped, no cut yet), a stable launch finds no matching
  versioned copy and falls back to bare `clave` on `PATH` — you are running
  unreleased code, which is exactly what that state means, and `runtime_binary`
  says so out loud.
- **The hook slot is shared.** `~/.claude/settings.json` is one file (Claude's
  identity is never sandboxed) and Claude fires *all* matching hooks, so clave
  keeps exactly **one** hook entry per event — duplicates would double-fire.
  Releases point it at the versioned stable binary; running `clave dev
  scenario`/`clave setup` from the dev loop temporarily re-points it at the dev
  binary, and the next `just release` heals it (accepted policy, 2026-07-20).
  Store routing is unaffected either way — hook processes inherit
  `CLAVE_STATE_DIR` from their `claude` parent, so events always land in the
  right store; only the *binary version* servicing them ping-pongs.
- **`just install` is retired** — it was the foot-gun that clobbered stable
  wasm and CLI straight from an in-progress working tree. Use **`just
  dev-install`** for the dev loop: it builds the wasm (stamped with
  `CLAVE_BUILD_TAG`) into the sandbox data dir `~/.local/state/clave-dev/data/`
  and installs the dev CLI as `~/.cargo/bin/clave-dev` (#43b). The sandbox's
  generated config references those dev artifacts.
- **`clave --version`** prints semver plus build tag, so "what am I actually
  running?" is always answerable in either environment.
- **Fresh clone?** Run **`just sandbox`**. It builds the working tree, seeds a
  scenario, self-checks the generated pair, and prints the launch command for
  you to run in a non-zellij terminal. It is the path to use because the
  sandbox is the **one** surface that still resolves its CLI through `PATH`:
  `runtime_binary()` bakes bare `clave` there by design (§2 binary split — a
  sandbox data dir holds no versioned copy, and a working-tree build is exactly
  what should run), so #44's "the bar calls the binary it belongs to" holds
  through a `clave` that `just sandbox` supplies from a shim scoped to the
  printed command. Stable never does this: it bakes an absolute versioned path.
  Since #43b
  `just dev-install` no longer provides it: it installs `clave-dev`, which is
  what you type for one-off commands, not what the sandbox bar shells out to.
  A *stable* install only exists once a release has been cut on your machine
  (`just release` from a clean, tagged tree) — that is deliberate: stable is a
  promotion target, not a build output.

## Pull-request flow

Direct-to-`main` commits ended at the `v0.1.0` cut. From there:

1. **Branch** off `main` for your change.
2. **Open a PR.** It gets [CodeRabbit](https://coderabbit.ai) review plus this
   repo's own review flow (the fugu / whole-branch dry-run reviews).
3. **Merge to `main`** once reviewed. `main` stays releasable at all times.
4. **Tag** `vX.Y.Z` when you want to cut — the tag, plus `just release`, is what
   turns reviewed `main` into a release.

## The test gate

```bash
cargo test --workspace
```

**The `--workspace` flag is load-bearing.** `default-members` excludes the
wasm-only `clave-bar` crate, so a bare `cargo test` **silently skips** all of
`clave-bar`'s model tests — including the divergence-critical ones. It exits 0
and tells you nothing is wrong. Always run the workspace form; `just test` wraps
it for you. Live, interactive behavior is *not* covered by any of these tests —
that is what [`docs/dev/TESTING.md`](docs/dev/TESTING.md) exists for.

Lint the same way before you push:

```bash
just gates    # fmt --check + test + wasm build + clippy — exactly what CI runs
```

`just clippy` alone is **not** the lint gate: CI's `lint` job runs
`cargo fmt --all --check` first, so hand-written code that clippy accepts can
still fail the build (it did, on #66). `just gates` runs both in CI's order.

## Where work is tracked

Public **GitHub issues** are the single work-tracking surface. The repo is
public and contributor-facing, and a visible backlog is the invitation — so the
backlog lives in the open, not in private notes.

- **Labels over ceremony**: `bar`, `cli`, `harness`, `docs`, `upstream-watch`,
  `good-first-issue`. Start with a `good-first-issue`.
- **One milestone per version cut.**
- A project board only appears if issue volume ever demands it.

## Before you change a subsystem

Read its section in
[`docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md`](docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md)
first. It is the ledger of every approach that was tried and *why it failed* —
`hide_self`, fixed pane sizes, the announce storms, serialization-based
resurrection. Each forbidden path was expensive to learn. And never trust
assumed Zellij semantics: read the vendored source before building on a
behavior. Both disciplines are spelled out in
[`docs/dev/TESTING.md`](docs/dev/TESTING.md).
