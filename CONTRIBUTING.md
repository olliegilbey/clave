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
`clave-bar` also shells out to the CLI on its own (`snapshot`, `open`, `bind`,
`focus`, `touch`, `prune-tabs`, `add`), and today it invokes plain **`clave`**,
resolved through `PATH`. `just dev-install` puts a working-tree build at
`~/.cargo/bin/clave` — the same name the daily surface answers to.

So a dev build **can** drive a stable session, and on 2026-07-22 it did: a stale
`0.1.0` binary on `PATH` served `clave open` inside a `v0.1.1` session and
composed tab layouts pointing at the *old* wasm. Because zellij keys plugin
identity on file location, every tab opened that way loaded a **second bar** —
two populations, no shared beacon state, dead navigation. The release itself was
correct; only the binary the plugin reached for was wrong.

**Until #44 lands, treat this as a hard rule: never `cargo install` or
`just dev-install` while a stable session is running** — including from a
worktree, and including agent-driven builds. When a dev round ends, restore the
stable binary before daily driving:

```sh
cp ~/.local/share/clave/bin/clave-vX.Y.Z ~/.cargo/bin/clave
```

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
- **A running session is immune to installs.** A live session only ever
  references the versioned files baked into the config it generated at launch.
  Installing a new release never overwrites a file a live session is loading; the
  upgrade lands atomically at the *next* `clave` launch. This is what makes the
  daily environment safe to develop clave from — see C8 in the validation ledger
  for the parity-desync bug class this design closes.
- **The binary split.** `~/.cargo/bin/clave` (a plain `cargo install` from the
  working tree) is the **dev** binary — it is what the sandbox and contributors'
  shells run. Stable sessions never invoke it: their keybinds, layout, and hooks
  bake the absolute path of the versioned copy under `~/.local/share/clave/bin/`.
  One skew edge to know: if the dev binary's version is *ahead* of the latest
  installed release (Cargo.toml bumped, no cut yet), a stable launch finds no
  matching versioned copy and falls back to the dev binary — you are running
  unreleased code, which is exactly what that state means.
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
  and `cargo install`s the dev CLI. The sandbox's generated config references
  those dev artifacts.
- **`clave --version`** prints semver plus build tag, so "what am I actually
  running?" is always answerable in either environment.
- **Fresh clone?** The sandbox works immediately: `just dev-install`, then
  `clave dev scenario c8-cold-start`, then `clave dev launch`. A *stable*
  install only exists once a release has been cut on your machine (`just
  release` from a clean, tagged tree) — that is deliberate: stable is a
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
