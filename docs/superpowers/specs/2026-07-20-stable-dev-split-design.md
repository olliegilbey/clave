# Stable/Dev Split, Version Cuts, and the Contributor Surface — Design

_2026-07-20 · approved by Ollie 2026-07-20 (conversation) · status: locked_

## Problem

clave is becoming its own build environment: Ollie's day-to-day terminal
sessions run inside a clave session, while clave itself is developed from a
project session *inside that environment*. Today `just install` clobbers the
stable wasm and CLI straight from the working tree — one absent-minded
install mid-feature-work puts unreviewed code under the daily environment.
A running session loads the bar wasm from disk **per new tab**, so an
install during a live session mixes plugin versions across tabs — the
parity-desync bug class observed live (C8, 2026-07-19). The project is also
going OSS-contributor-facing and needs a public work-tracking surface and
contributor docs.

Requirements: the daily environment must be immune to working-tree builds
while running AND between launches until an explicit upgrade; feature work
needs a full-fidelity environment on the same machine; a version cut must
be a deliberate, tagged, reproducible act; future agents (and contributors)
need the operating knowledge written down.

## 1. Environments

Two launch surfaces, one code path (env-var redirection only — the sandbox
faithfully reproduces stable behavior by construction):

| | Day-to-day (stable) | Feature/dev (sandbox) |
|---|---|---|
| Launch | `clave` in a non-zellij terminal | `clave dev launch` in a non-zellij terminal |
| Zellij session | `clave` | `clave-test` |
| State (store, evlog) | `~/.local/state/clave/` | `~/.local/state/clave-dev/state/` |
| Artifacts (wasm, config, layout) | `~/.local/share/clave/` | `~/.local/state/clave-dev/data/` |
| Binary | versioned release copy (see §2) | working tree via `just dev-install` |
| Agents | real work | synthetic, `clave dev scenario <name>` |
| Teardown | never | `clave dev reset` |

Invariants: no beta channel (promotion is sandbox-validated → cut →
stable); Claude identity is never sandboxed (2026-07-18 ruling — the
sandbox isolates CLAVE state only).

## 2. Release mechanics

- **Cuts are semver git tags** on `main` (`vX.Y.Z`, first cut `v0.1.0`).
  `main` is always releasable; tag when you want a cut.
- **`just release`** refuses unless the tree is clean AND `HEAD` carries an
  exact `vX.Y.Z` tag matching `Cargo.toml`'s version. It then: builds the
  workspace + wasm (release); installs
  `~/.local/share/clave/clave-bar-vX.Y.Z.wasm` **and** a versioned CLI copy
  `~/.local/share/clave/bin/clave-vX.Y.Z`; regenerates stable
  `config.kdl`/`layout.kdl` and re-merges hooks so every generated
  reference (plugin location, keybind `Run` commands, hook commands) points
  at the **versioned** artifacts.
- **Running-session immunity**: a live session only ever references the
  versioned files baked into its generated config at launch. Installing a
  new release never overwrites a file a live session loads; the upgrade
  lands atomically at the next `clave` launch.
- **Binary split**: `~/.cargo/bin/clave` (cargo install from the working
  tree) is the DEV binary — used by the sandbox and by contributors'
  shells. Stable sessions never invoke it: their keybinds/layout/hooks bake
  the versioned copy's absolute path. `merge_hooks` must learn
  replace-on-version-change (a new release's hook command replaces the
  prior clave hook entry rather than duplicating it).
- **`just install` is retired** (it is the foot-gun). `just dev-install`
  builds the wasm (with `CLAVE_BUILD_TAG`) into
  `~/.local/state/clave-dev/data/` and `cargo install`s the dev CLI. The
  sandbox's generated config references the dev artifacts.
- **`clave --version`** prints semver + build tag, so "what am I running"
  is always answerable in both environments.

## 3. Workflow

- **PRs from feature branches** → CodeRabbit review + this repo's review
  flow (fugu/whole-branch) → merge to `main`. Direct-to-main commits end at
  the v0.1.0 cut (this was pre-ratified, gated on validation lock-in —
  Task 9 closed 2026-07-20).
- **Public GitHub issues** are the single work-tracking surface (the repo
  is public and contributor-facing; a visible backlog is the invitation).
  Labels over ceremony: `bar`, `cli`, `harness`, `docs`, `upstream-watch`,
  `good-first-issue`; one milestone per version cut. The internal backlog
  migrates into issues at the v0.1.0 cut (seed list in §6). A gh Project
  board only if issue volume ever demands it.

## 4. Docs

Three artifacts, each with one job:

- **`CONTRIBUTING.md`** (root): the environments table + launch commands,
  the release process, the PR flow, the test gate (`cargo test
  --workspace` — bare `cargo test` silently skips the wasm crate's tests),
  and where work is tracked.
- **`docs/dev/TESTING.md`** (the live-validation SOP): sandbox lifecycle
  (`reset` → `scenario <name>` → `launch`), the scenario catalog, the
  interaction contract (the human drives live input; the agent reads
  observability), the observability map — zellij log at
  `$TMPDIR/zellij-<uid>/zellij-log/zellij.log` (shared across sessions;
  filter by date AND build tag), the evlog (`clave.log` in each state
  dir), `clave dev status`, env-scoped `zellij action dump-layout` — the
  instrumentation recipe (temp eprintln + `CLAVE_BUILD_TAG` rebuild +
  sandbox-only hot-reload), and the zellij CLI safety boundaries (session
  lifecycle is always the human's; agents touch only `clave-test`).
- **root `CLAUDE.md`** (thin, agent-facing): points at TESTING.md and
  CONTRIBUTING.md; carries the always-`--workspace` rule, the
  ask-before-commit + user-signs constraints, and the sandbox-only
  hot-reload sanction.

## 5. Out of scope (tracked as issues, not designed here)

- **Upstream resilience** (`upstream-watch` epic): Claude Code
  auto-updates can break clave suddenly (serialization forms, hook
  payloads, CLI flags). Needs its own session: history audit of past
  breaking changes → prediction → detection automation (agent that diffs
  new CC releases against clave's touchpoints) → patch playbook.
  Generalizes when more CLIs (codex, gemini) join.
- Multi-CLI agent support; beta channel (only if sandbox-only ever
  pinches); store schema versioning/migration.

## 6. Issue seed list (created at lock-in)

Implementation (v0.1.0 milestone): release mechanics per §2 (justfile +
version-aware paths + merge_hooks replacement + `--version`); docs trio per
§4; PR-workflow switch per §3 (branch protection, CodeRabbit config).

Backlog (post-v0.1.0): width-seek drift re-arm (fugu F1); snapshot-carried
collapsed flag closing the parity-desync family (fugu F2 + C8 findings);
tab_id reuse verification + store pruning (fugu F3); floating helper pane
(+ dormant-row selection UX note, C9 finding); jsonl adoption + nav ring
caps; Task 10 sweep (4 parked clippy lints + whole-branch review);
testing-strategy items 2–5 (KDL real-parser validation first); upstream
resilience epic (§5).
