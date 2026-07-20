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
just clippy   # cargo clippy --workspace --all-targets -- -D warnings
```

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
