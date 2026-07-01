# clave Foundation & Spikes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the clave Cargo workspace + shared pipe schema + the cwd-munging join-key helper, then run the four gating spikes (S0, S0b, S1, S2) that prove the idempotency model and the plugin architecture before any subsystem is built.

**Architecture:** One Cargo workspace with three crates — `clave` (native binary, lib+bin), `clave-types` (serde-only pipe schema shared by both artifacts = the anti-drift mechanism), and `clave-bar` (the Zellij WASM plugin, a binary crate → `wasm32-wasip1`). Foundation tasks (1–3) are TDD. Spike tasks (4–6) are validate-first experiments with an explicit pass/fail and a documented fallback; their findings are logged under `docs/superpowers/spikes/`.

**Tech Stack:** Rust (edition 2024, resolver 3), `serde`/`serde_json`, `clap`, `uuid`, `dirs`, `zellij-tile` 0.44 (→ WASM), the real `claude` CLI (v2.1.197), Zellij 0.44.3, `fzf`/`zoxide` (later tasks), `just`.

## Global Constraints

_Every task's requirements implicitly include this section. Values copied verbatim from the canonical spec `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md`._

- **Canonical spec is law.** Read the referenced §-sections before each task. If something in this plan contradicts the spec, the spec wins — stop and flag it.
- **Toolchain:** Rust latest stable (verified `rustc 1.96.1`), **edition `2024`**, workspace **`resolver = "3"`**. Keep deps at latest majors.
- **Workspace shape (spec §7):** `crates/clave` (bin, host target), `crates/clave-bar` (a **binary crate** — `src/main.rs` + `register_plugin!`, matching zellij's official rust-plugin-example; dep `zellij-tile`, target `wasm32-wasip1`), `crates/clave-types` (serde-only, target-agnostic). Set **`default-members = ["crates/clave", "crates/clave-types"]`** so a plain `cargo build`/`cargo test` skips the WASM-only crate.
- **WASM target must be installed first:** `rustup target add wasm32-wasip1` (only `wasm32-unknown-unknown` is present). Build the plugin with `cargo build -p clave-bar --release --target wasm32-wasip1`.
- **`zellij-tile = "0.44"`** (0.44.2 is the installed/available point release; pin the minor, not the patch).
- **`clave-types` depends on nothing but `serde`** at runtime (serde-only, so it compiles for both host and wasm). `serde_json` may be a **dev-dependency** for round-trip tests only.
- **The munging rule is `s/[^A-Za-z0-9]/-/g`** — replace **every** non-ASCII-alphanumeric character with `-` (a `.` becomes `-` too). It is the **join key** (invariant #3); it lives in **one** shared helper (`munge_cwd`) and is pinned by spike **S0b**. The old `/`→`-` shorthand is wrong.
- **Status enum values (spec §5/§6.5):** `idle | working | needs_you | done | failed` (serialize as exactly these snake_case strings).
- **Pipe contract (spec §5):** every `clave-status` message is an authoritative **full replace** carrying a monotonic **`seq`**; a consumer applies only the highest `seq` it has seen and discards stale/out-of-order messages.
- **Spikes gate the build (spec §9):** S0/S0b gate the join key; **S1 gates the plugin architecture — if S1 fails, STOP and revisit spec §3; do NOT proceed to any subsystem plan.**
- **Commits:** conventional-commit style, frequent, one per task-deliverable. The executing agent appends its own `Claude-Session: <url>` trailer to each commit (do not hardcode a session URL from this plan). **Branch policy:** this solo public repo commits straight to `main`; confirm with the user before assuming a feature-branch/PR flow. **Ask before committing.**
- **Repo is public** (at the user's explicit request) — no secrets, no machine-specific absolute paths baked into committed code (spike layouts that reference an absolute wasm path are the one exception, and they live under `spikes/` as throwaway artifacts).
- **Comment density:** more comments than typical — the user prefers heavily-commented code; keep the explanatory comments in every code block, and comment the *why*, not just the *what*.

---

## File Structure

Created or modified across this plan:

| Path | Responsibility |
|---|---|
| `Cargo.toml` (root) | **Workspace** manifest: members, `default-members`, `resolver="3"`, shared `[workspace.package]` + `[workspace.dependencies]`. |
| `crates/clave/Cargo.toml` | The `clave` binary package (inherits workspace deps). |
| `crates/clave/src/main.rs` | Thin clap CLI entry (relocated from `src/main.rs`). |
| `crates/clave/src/lib.rs` | Library root for the binary's testable logic (`pub mod munge;` + future modules). |
| `crates/clave/src/munge.rs` | `munge_cwd` — the cwd→transcript-dir join-key rule (spec §4) + unit tests. |
| `crates/clave/examples/munge.rs` | Debug helper: prints `munge_cwd(argv[1])`; used by the S0b harness. |
| `crates/clave-types/Cargo.toml` | serde-only shared-schema package. |
| `crates/clave-types/src/lib.rs` | `Status`, `Agent`, `AgentSnapshot`, `Register` — the pipe schema (spec §5) + round-trip tests. |
| `crates/clave-bar/Cargo.toml` | The Zellij plugin package (binary crate, `zellij-tile`). |
| `crates/clave-bar/src/main.rs` | The `ZellijPlugin` — minimal in Task 1, grows through S1/S2. |
| `justfile` | Build/test orchestration (`build`, `build-bar`, `test`, `setup-toolchain`). |
| `spikes/` | Throwaway spike harness scripts + test layouts (committed as reproducible validation artifacts). |
| `docs/superpowers/spikes/*.md` | Per-spike findings logs (feed decisions back into the spec). |

---

## Task 1: Cargo workspace restructure

Turn the single-package scaffold into the three-crate workspace, get the host build + tests green, and prove the WASM toolchain builds a (minimal, valid) plugin. This is the "both artifacts build" gate. **It uses a binary crate (matching zellij's official rust-plugin-example); S1 (Task 5) confirms the wasm actually loads in Zellij, with a cdylib fallback if it does not.**

**Files:**
- Create: `Cargo.toml` (new workspace root — the old package manifest moves out)
- Create: `crates/clave/Cargo.toml`, `crates/clave/src/lib.rs`
- Move: `src/main.rs` → `crates/clave/src/main.rs`
- Create: `crates/clave-types/Cargo.toml`, `crates/clave-types/src/lib.rs`
- Create: `crates/clave-bar/Cargo.toml`, `crates/clave-bar/src/main.rs`
- Create: `justfile`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: a buildable workspace. `cargo build` and `cargo test` (default-members) are green; `cargo build -p clave-bar --target wasm32-wasip1` emits `target/wasm32-wasip1/debug/clave-bar.wasm`.

- [ ] **Step 1: Install the WASM target (one-time toolchain step)**

Run:
```bash
rustup target add wasm32-wasip1
rustup target list --installed | grep wasm32-wasip1
```
Expected: the second command prints `wasm32-wasip1`.

- [ ] **Step 2: Relocate the existing package into `crates/clave`**

Run (uses `git mv` so history follows the files):
```bash
mkdir -p crates/clave/src crates/clave-types/src crates/clave-bar/src
git mv src/main.rs crates/clave/src/main.rs
git mv Cargo.toml crates/clave/Cargo.toml
rmdir src 2>/dev/null || true
```
Expected: `crates/clave/src/main.rs` and `crates/clave/Cargo.toml` exist; the old top-level `src/` is gone. (`Cargo.lock` stays at the root and will regenerate on the next build.)

- [ ] **Step 3: Write the new workspace-root `Cargo.toml`**

Create `Cargo.toml` (repo root):
```toml
[workspace]
resolver = "3"
members = ["crates/clave", "crates/clave-bar", "crates/clave-types"]
# clave-bar is WASM-only; excluding it here means a plain `cargo build`/`cargo test`
# on the host never tries to compile it (spec §7).
default-members = ["crates/clave", "crates/clave-types"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
repository = "https://github.com/olliegilbey/clave"

[workspace.dependencies]
# Native binary deps
clap = { version = "4", features = ["derive"] }
anyhow = "1"
uuid = { version = "1", features = ["v4"] }
dirs = "6"
# Shared serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
# Zellij plugin
zellij-tile = "0.44"
```

- [ ] **Step 4: Rewrite `crates/clave/Cargo.toml` to inherit from the workspace**

Overwrite `crates/clave/Cargo.toml`:
```toml
[package]
name = "clave"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Conduct a fleet of Claude Code agents from a Zellij sidebar"

[dependencies]
clap.workspace = true
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
dirs.workspace = true
clave-types = { path = "../clave-types" }
```
(The `readme` field is dropped — the README lives at the repo root, not in the crate, and it is only needed for publishing.)

- [ ] **Step 5: Add a library root so the binary's logic is testable**

Create `crates/clave/src/lib.rs`:
```rust
//! Library root for the `clave` binary. Reusable, testable logic lives here as
//! modules; `main.rs` stays a thin clap entry point that calls into this crate.
//! (A bin crate can't be reached by integration tests or examples, so we split
//! out a lib — this is what lets the S0b spike and later tasks call `munge_cwd`.)

// Modules are added per task. Task 3 adds `pub mod munge;`.
```
Leave `crates/clave/src/main.rs` as it was (the clap skeleton with `todo!()` arms) — it still compiles. Optionally update its module-doc reference from `docs/design.md` to the canonical spec, since the file is being touched:

Modify `crates/clave/src/main.rs:7`:
```rust
//! See `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` for the full spec.
```

- [ ] **Step 6: Create the `clave-types` crate as a compiling placeholder**

Create `crates/clave-types/Cargo.toml`:
```toml
[package]
name = "clave-types"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/clave-types/src/lib.rs`:
```rust
//! Shared pipe schema between the `clave` binary and the `clave-bar` plugin.
//! serde-only and target-agnostic (compiles for host AND wasm) — this is the
//! anti-drift guarantee (invariant #9): both artifacts serialize the SAME
//! structs. Populated in Task 2.
```

- [ ] **Step 7: Create the `clave-bar` crate with a minimal valid plugin**

Create `crates/clave-bar/Cargo.toml`:
```toml
[package]
name = "clave-bar"
version.workspace = true
edition.workspace = true
license.workspace = true

# A Zellij plugin is a binary crate compiled to wasm32-wasip1 (src/main.rs +
# register_plugin!), matching zellij's official rust-plugin-example — no `[lib]`
# block. Fallback if S1 shows it does not load: a cdylib
# (`[lib] crate-type=["cdylib"]`, move to src/lib.rs, artifact `clave_bar.wasm`).

[dependencies]
zellij-tile = { workspace = true }
```

Create `crates/clave-bar/src/main.rs`:
```rust
//! clave-bar — the Zellij WASM plugin that renders the agent sidebar.
//! Task 1 is a MINIMAL valid plugin: it proves the binary→wasm32-wasip1
//! toolchain and the `register_plugin!` wiring. Real rendering arrives in S1.

use std::collections::BTreeMap;
use zellij_tile::prelude::*;

#[derive(Default)]
struct State;

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {}
    fn update(&mut self, _event: Event) -> bool {
        false
    }
    fn render(&mut self, _rows: usize, _cols: usize) {
        print!("clave-bar");
    }
}

// A binary crate needs a `main`; `register_plugin!` supplies the wasm plugin
// entry points (load/update/render/pipe), not `main`, so this stays empty.
fn main() {}
```

- [ ] **Step 8: Write the `justfile`**

Create `justfile`:
```just
# clave build orchestration. `just` with no target lists the recipes.
default:
    @just --list

# One-time: add the Zellij plugin's WASM target.
setup-toolchain:
    rustup target add wasm32-wasip1

# Host build — skips the WASM-only clave-bar via default-members.
build:
    cargo build

# Build the Zellij plugin to WASM (debug).
build-bar:
    cargo build -p clave-bar --target wasm32-wasip1

# Build the plugin release artifact.
build-bar-release:
    cargo build -p clave-bar --release --target wasm32-wasip1

# Everything (host + plugin).
build-all: build build-bar

test:
    cargo test
```

- [ ] **Step 9: Verify the host build and tests are green**

Run:
```bash
cargo build
cargo test
```
Expected: both succeed. `cargo build` compiles `clave` + `clave-types` only (not `clave-bar`, which is excluded from `default-members`). `cargo test` passes (no tests yet ⇒ "0 passed"), confirming the workspace is wired correctly.

- [ ] **Step 10: Verify the plugin builds to WASM**

Run:
```bash
cargo build -p clave-bar --target wasm32-wasip1
ls -la target/wasm32-wasip1/debug/clave-bar.wasm
```
Expected: compiles clean; `clave-bar.wasm` exists (a binary crate keeps the hyphen in the artifact name). If the build reports `main function not found`, `register_plugin!` isn't supplying the entry on this zellij-tile version — the empty `fn main` from Step 7 covers it. Any other failure: capture the error for S1 (Task 5).

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "chore: restructure into a cargo workspace (clave + clave-types + clave-bar)"
```
(Append your `Claude-Session:` trailer. Ask the user before committing per Global Constraints.)

---

## Task 2: `clave-types` pipe schema (TDD)

Define the exact structs that cross the `zellij pipe` boundary. This crate is the single source of truth for the wire format, so both the binary and the plugin deserialize the *same* types (invariant #9). `clave-types` carries the **display/pipe** shape only — the richer on-disk store record (adding `worktree` and `label_source`, each `#[serde(default)]` for forward-compat) is a separate type defined with the store subsystem (§6.2), not here; the plugin never sees it.

**Files:**
- Modify: `crates/clave-types/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in the same file (uses the `serde_json` dev-dependency)

**Interfaces:**
- Consumes: `serde` (workspace).
- Produces:
  - `pub enum Status { Idle, Working, NeedsYou, Done, Failed }` — serde `snake_case` ⇒ `"idle"|"working"|"needs_you"|"done"|"failed"`.
  - `pub struct Agent { uuid: String, cwd: String, repo_root: String, branch: String, label: String, status: Status, last_interacted: u64, last_visited: u64, archived: bool }`.
  - `pub struct AgentSnapshot { seq: u64, agents: Vec<Agent> }` — the full-replace + monotonic-`seq` payload for `clave-status`.
  - `pub struct Register { uuid: String, pane_id: u32 }` — the `clave-register` payload (spec §6.1 / S2).

- [ ] **Step 1: Write the failing tests**

Replace `crates/clave-types/src/lib.rs` with the doc comment plus the tests (types don't exist yet, so this fails to compile — that is the failing state):
```rust
//! Shared pipe schema between the `clave` binary and the `clave-bar` plugin.
//! serde-only and target-agnostic (compiles for host AND wasm) — this is the
//! anti-drift guarantee (invariant #9): both artifacts serialize the SAME
//! structs.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_as_spec_snake_case() {
        // Exactly the strings the spec (§5/§6.5) mandates.
        assert_eq!(serde_json::to_string(&Status::Idle).unwrap(), "\"idle\"");
        assert_eq!(serde_json::to_string(&Status::Working).unwrap(), "\"working\"");
        assert_eq!(serde_json::to_string(&Status::NeedsYou).unwrap(), "\"needs_you\"");
        assert_eq!(serde_json::to_string(&Status::Done).unwrap(), "\"done\"");
        assert_eq!(serde_json::to_string(&Status::Failed).unwrap(), "\"failed\"");
    }

    #[test]
    fn status_deserializes_from_snake_case() {
        let s: Status = serde_json::from_str("\"needs_you\"").unwrap();
        assert_eq!(s, Status::NeedsYou);
    }

    #[test]
    fn snapshot_roundtrips() {
        let snap = AgentSnapshot {
            seq: 7,
            agents: vec![Agent {
                uuid: "u1".into(),
                cwd: "/Users/x/code/clave".into(),
                repo_root: "/Users/x/code/clave".into(),
                branch: "main".into(),
                label: "clave · main · hello".into(),
                status: Status::Working,
                last_interacted: 1000,
                last_visited: 0,
                archived: false,
            }],
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: AgentSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, back);
    }

    #[test]
    fn register_roundtrips() {
        let reg = Register { uuid: "u1".into(), pane_id: 42 };
        let back: Register = serde_json::from_str(&serde_json::to_string(&reg).unwrap()).unwrap();
        assert_eq!(reg, back);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p clave-types`
Expected: **compile error** — `Status`, `Agent`, `AgentSnapshot`, `Register` are undefined.

- [ ] **Step 3: Write the minimal implementation**

Insert the type definitions above the `#[cfg(test)]` block in `crates/clave-types/src/lib.rs`:
```rust
use serde::{Deserialize, Serialize};

/// Per-agent status. This is a *latest-wins state machine* (spec §6.5), not a
/// priority-max: a later event can downgrade an earlier one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Working,
    NeedsYou,
    Done,
    Failed,
}

/// One agent row as the plugin renders it. Mirrors the store record's
/// display-relevant fields (spec §5); the plugin never sees the store, only
/// this snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// Minted session UUID — the join key (invariant #3).
    pub uuid: String,
    pub cwd: String,
    /// git toplevel of `cwd`; the grouping key in the bar.
    pub repo_root: String,
    pub branch: String,
    /// `cwd · branch · summary` (spec §6.4).
    pub label: String,
    pub status: Status,
    /// unix seconds; bumped on UserPromptSubmit → drives recency sort.
    pub last_interacted: u64,
    /// unix seconds; bumped on focus → `unread = done && !visited`.
    pub last_visited: u64,
    pub archived: bool,
}

/// The full-replace snapshot `clave` pushes to `clave-bar` on every change
/// (spec §5 pipe contract). `seq` is monotonic; a consumer applies only the
/// highest `seq` it has seen and discards stale/out-of-order messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub seq: u64,
    pub agents: Vec<Agent>,
}

/// The `clave-register` payload a pane's `clave spawn` pipes to the plugin so it
/// can map uuid → pane_id → live tab position (spec §6.1 / spike S2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Register {
    pub uuid: String,
    pub pane_id: u32,
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p clave-types`
Expected: **PASS** — 4 tests pass.

- [ ] **Step 5: Confirm the schema also compiles to WASM (it must — the plugin depends on it)**

Run: `cargo build -p clave-types --target wasm32-wasip1`
Expected: compiles clean (proves the serde-only crate is target-agnostic).

- [ ] **Step 6: Commit**

```bash
git add crates/clave-types
git commit -m "feat(types): add shared pipe schema (Status, Agent, AgentSnapshot, Register)"
```

---

## Task 3: `munge_cwd` join-key helper (TDD)

Implement the cwd→transcript-dir rule. This is the single most correctness-critical helper in the project: it computes the path `clave spawn` tests for existence to decide resume-vs-create (idempotency, invariant #5), and a wrong rule silently breaks worktrees.

**Files:**
- Create: `crates/clave/src/munge.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod munge;`)
- Create: `crates/clave/examples/munge.rs` (debug helper for the S0b harness)

**Interfaces:**
- Consumes: nothing external.
- Produces: `pub fn munge_cwd(cwd: &str) -> String` in module `clave::munge` — replaces every non-ASCII-alphanumeric char with `-`. Consumed by the S0b harness (Task 4) and, later, `clave spawn`/`clave add`.

- [ ] **Step 1: Write the failing tests**

Create `crates/clave/src/munge.rs`:
```rust
//! The cwd → transcript-dir munging rule (spec §4). This is the JOIN KEY:
//! `clave spawn`'s idempotency check computes
//! `~/.claude/projects/<munge_cwd(cwd)>/<uuid>.jsonl` and tests existence.
//! Claude replaces EVERY non-alphanumeric byte (not just `/`) with `-`, so a
//! `.` becomes `-` too — critical for dotted and worktree paths. Pinned by S0b.

// Implementation is added in Step 3.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn munges_leading_slash_path() {
        assert_eq!(
            munge_cwd("/Users/olliegilbey/code/clave"),
            "-Users-olliegilbey-code-clave"
        );
    }

    #[test]
    fn munges_dots_and_worktree_double_dash() {
        // Verified-on-disk example (spec §4). Note the `--`: adjacent `/` and `.`
        // in `/.claude-worktrees` each become a dash.
        assert_eq!(
            munge_cwd("/Users/olliegilbey/code/resumate/.claude-worktrees/nalu-cta"),
            "-Users-olliegilbey-code-resumate--claude-worktrees-nalu-cta"
        );
    }

    #[test]
    fn maps_every_non_alnum_including_unicode() {
        // Non-ASCII letters are not [A-Za-z0-9] under Claude's rule → dashed.
        assert_eq!(munge_cwd("a.b_c d"), "a-b-c-d");
        assert_eq!(munge_cwd("café"), "caf-"); // é is non-ASCII-alnum → '-'
    }
}
```

- [ ] **Step 2: Wire the module and run tests to verify they fail**

Modify `crates/clave/src/lib.rs` — add:
```rust
pub mod munge;
```
Run: `cargo test -p clave munge`
Expected: **compile error** — `munge_cwd` is not defined.

- [ ] **Step 3: Write the minimal implementation**

Add to `crates/clave/src/munge.rs` (above the `#[cfg(test)]` block):
```rust
/// Replace every ASCII-non-alphanumeric character in `cwd` with `-`, matching
/// Claude Code's `~/.claude/projects/<dir>` naming (empirically
/// `s/[^A-Za-z0-9]/-/g`, verified on disk — spec §4). Non-ASCII characters are
/// not `[A-Za-z0-9]`, so they are dashed too.
pub fn munge_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p clave munge`
Expected: **PASS** — 3 tests pass.

- [ ] **Step 5: Add the debug example the S0b harness will call**

Create `crates/clave/examples/munge.rs`:
```rust
//! Debug helper for the S0b spike (Task 4): prints `munge_cwd(argv[1])` so a
//! shell harness can compare our computed dir name against what Claude wrote to
//! `~/.claude/projects/`. Not part of the shipped binary.
use clave::munge::munge_cwd;

fn main() {
    let arg = std::env::args()
        .nth(1)
        .expect("usage: cargo run -p clave --example munge -- <path>");
    println!("{}", munge_cwd(&arg));
}
```

- [ ] **Step 6: Verify the example runs**

Run:
```bash
cargo run -q -p clave --example munge -- "/Users/olliegilbey/code/clave"
```
Expected output: `-Users-olliegilbey-code-clave`

- [ ] **Step 7: Commit**

```bash
git add crates/clave/src/lib.rs crates/clave/src/munge.rs crates/clave/examples/munge.rs
git commit -m "feat(clave): add munge_cwd join-key helper (spec §4 rule)"
```

---

## Task 4: Spike S0 + S0b — `--session-id` create semantics & munge-matches-disk

**This is a validation spike, not a feature.** It launches **real** Claude sessions (network + tokens) to confirm two coupled facts the whole idempotency model rests on:
- **S0:** `claude --session-id <fresh-uuid>` *creates* a new session and writes its `.jsonl` (rather than erroring or resuming). Also observe what a *pre-existing* UUID does.
- **S0b:** our `munge_cwd` output matches the actual `~/.claude/projects/<dir>` name Claude writes, across plain / dotted / worktree cwds.

They are done together because you cannot confirm "munge matches disk" without creating a session, and creating one is exactly what proves S0.

**Files:**
- Create: `spikes/s0-create-and-munge.sh` (harness, committed as a reproducible artifact)
- Create: `docs/superpowers/spikes/S0-S0b.md` (findings log)

**Interfaces:**
- Consumes: `munge_cwd` via `cargo run -p clave --example munge` (Task 3).
- Produces: a recorded PASS/FAIL for S0 and S0b in the findings log; on FAIL, the corrected rule/behavior to fold back into spec §4/§6.1.

- [ ] **Step 1: Write the spike harness**

Create `spikes/s0-create-and-munge.sh`:
```bash
#!/usr/bin/env bash
# Spike S0 + S0b — does `claude --session-id <fresh-uuid>` CREATE a session
# jsonl, and does our munge_cwd() match Claude's on-disk projects/<dir> naming?
#
# WARNING: launches REAL Claude sessions (network + tokens). Run deliberately.
set -euo pipefail

PROJECTS="$HOME/.claude/projects"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
munge() { ( cd "$REPO" && cargo run -q -p clave --example munge -- "$1" ); }

# Three path shapes: plain, dotted, and a git worktree under .claude-worktrees.
ROOT="$(mktemp -d)/clave.spike"          # dotted segment forces the `.`→`-` rule
mkdir -p "$ROOT/plain" "$ROOT/dot.dir"
git init -q "$ROOT/base"
( cd "$ROOT/base" && git commit -q --allow-empty -m init )
git -C "$ROOT/base" worktree add -q "$ROOT/base/.claude-worktrees/wt" -b spike

echo "=== S0 + S0b: fresh-uuid create + munge-matches-disk ==="
for CWD in "$ROOT/plain" "$ROOT/dot.dir" "$ROOT/base/.claude-worktrees/wt"; do
  UUID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
  echo "-- cwd=$CWD  uuid=$UUID"
  # Primary: headless print mode. If it does not persist a jsonl, use the
  # interactive fallback documented in S0-S0b.md.
  ( cd "$CWD" && claude --session-id "$UUID" -p "reply with the single word: ok" >/dev/null 2>&1 ) \
    || echo "   warn: claude -p exited non-zero"
  DIR="$(munge "$CWD")"
  JSONL="$PROJECTS/$DIR/$UUID.jsonl"
  if [[ -f "$JSONL" ]]; then
    echo "   PASS created + munge matches: $JSONL"
  else
    echo "   FAIL not at computed path: $JSONL"
    echo "   where did the uuid actually land? ->"
    grep -rl "$UUID" "$PROJECTS" 2>/dev/null | sed 's#^#     #' || echo "     (nowhere — creation failed)"
  fi
done

echo
echo "=== S0: pre-existing-uuid behavior (resume vs error) ==="
UUID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
CWD="$ROOT/plain"; DIR="$(munge "$CWD")"; JSONL="$PROJECTS/$DIR/$UUID.jsonl"
( cd "$CWD" && claude --session-id "$UUID" -p "first message" >/dev/null 2>&1 ) || true
BEFORE=$(wc -l < "$JSONL" 2>/dev/null || echo 0)
echo "-- re-running with the SAME uuid:"
( cd "$CWD" && claude --session-id "$UUID" -p "second message" ); echo "   exit=$?"
AFTER=$(wc -l < "$JSONL" 2>/dev/null || echo NA)
echo "   jsonl lines before=$BEFORE after=$AFTER"
echo "   (grew ⇒ silently resumed; non-zero exit/error ⇒ collision is a hard error)"

echo
echo "Cleanup: rm -rf $ROOT   (leaves the ~/.claude/projects spike sessions for inspection)"
```

- [ ] **Step 2: Make it executable and run it**

Run:
```bash
chmod +x spikes/s0-create-and-munge.sh
./spikes/s0-create-and-munge.sh
```
Expected (PASS): each of the three cwds prints `PASS created + munge matches: …/<uuid>.jsonl`, and the pre-existing-uuid section shows either the jsonl growing (resume) or a non-zero exit (hard error).

- [ ] **Step 3: If `claude -p` did not persist a jsonl, run the interactive fallback**

Some Claude modes may not persist under `-p`. If Step 2 printed `FAIL … creation failed` for the plain cwd, verify creation manually:
```bash
CWD=$(mktemp -d); UUID=$(uuidgen | tr '[:upper:]' '[:lower:]')
cd "$CWD" && claude --session-id "$UUID"   # type one message, then /exit
ls -la "$HOME/.claude/projects/$(cd - >/dev/null && cargo run -q -p clave --example munge -- "$CWD")/$UUID.jsonl"
```
Expected: the `.jsonl` exists. Record which mode (`-p` vs interactive) actually persists — that decision affects nothing in `clave spawn` (which always launches interactively) but must be noted so the spike is reproducible.

- [ ] **Step 4: Record findings**

Create `docs/superpowers/spikes/S0-S0b.md` capturing:
- **S0 verdict** (PASS/FAIL): does a fresh `--session-id` create a jsonl? Which launch mode persists?
- **S0 pre-existing-uuid behavior:** resume vs error (and therefore whether `clave spawn`'s "collision is a genuine error, surface it" stance (spec §6.1) holds).
- **S0b verdict** (PASS/FAIL): did `munge_cwd` match disk for all three path shapes? If any mismatched, paste the actual dir name from the `grep -rl` output and the corrected rule.
- **Fallbacks if FAIL:**
  - S0 fails (no create / always resumes) ⇒ the idempotency model in spec §6.1 is wrong — **stop and revise §4/§6.1 before writing `clave spawn`.**
  - S0b mismatch ⇒ derive the true rule from the observed dir name, update `munge_cwd` + its tests + spec §4, and re-run this spike.

- [ ] **Step 5: Commit**

```bash
git add spikes/s0-create-and-munge.sh docs/superpowers/spikes/S0-S0b.md
git commit -m "spike(s0): verify --session-id create semantics + munge-matches-disk"
```

---

## Task 5: Spike S1 — background repaint (THE GATE)

Prove the core architectural bet (invariant #11): a `clave-bar` plugin can render a status glyph for a **non-focused** agent row and update it live on a `zellij pipe` message **without stealing focus**. Extend the Task-1 plugin to consume the real `clave-status` snapshot, load it in a Zellij session, and observe repaint.

> **STOP CONDITION:** If S1 fails, do **not** proceed to Task 6 or to any subsystem plan. Record the failure and revisit spec §3 (reconsider `rename_tab`-based painting or forking cfal). This spike gates the entire plugin architecture.

> **Forward note (for the real §6.6 bar, not this spike):** production renders the bar in *every* tab via `default_tab_template`, i.e. one plugin instance per tab — so "which instance does a `zellij pipe` message reach, and do all instances repaint?" is an open question this single-pane S1 layout deliberately does not touch. Carry it into the §6.6 plan.

**Files:**
- Modify: `crates/clave-bar/Cargo.toml` (add `clave-types`, `serde_json`)
- Modify: `crates/clave-bar/src/main.rs` (real `pipe()` + colored `render()`)
- Create: `spikes/layouts/s1.kdl` (throwaway test layout)
- Create: `docs/superpowers/spikes/S1.md` (findings log)

**Interfaces:**
- Consumes: `clave_types::{AgentSnapshot, Agent, Status}` (Task 2).
- Produces: a plugin that applies the highest-`seq` `clave-status` snapshot and renders one colored glyph + label per agent. Verified: non-focused repaint, seq gating, no focus theft.

- [ ] **Step 1: Add the plugin's runtime dependencies**

Modify `crates/clave-bar/Cargo.toml` `[dependencies]`:
```toml
[dependencies]
zellij-tile = { workspace = true }
clave-types = { path = "../clave-types" }
serde_json = { workspace = true }
```

- [ ] **Step 2: Implement snapshot consumption + colored render**

Overwrite `crates/clave-bar/src/main.rs`:
```rust
//! clave-bar — the Zellij WASM plugin that renders the agent sidebar.
//! S1 scope: consume the authoritative `clave-status` snapshot (full-replace +
//! monotonic seq, spec §5) and render one colored status glyph per agent,
//! including for NON-focused rows, without stealing focus.

use std::collections::BTreeMap;
use zellij_tile::prelude::*;
use clave_types::{Agent, AgentSnapshot, Status};

#[derive(Default)]
struct State {
    /// Highest snapshot seq applied so far (stale messages are discarded).
    seq: u64,
    agents: Vec<Agent>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        // We only need to receive `zellij pipe` messages for S1.
        request_permission(&[PermissionType::ReadCliPipes]);
    }

    fn update(&mut self, _event: Event) -> bool {
        false
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        if message.name != "clave-status" {
            return false;
        }
        let Some(payload) = message.payload else {
            return false;
        };
        let Ok(snap) = serde_json::from_str::<AgentSnapshot>(&payload) else {
            return false;
        };
        // Full-replace + monotonic seq (spec §5): apply only strictly-newer
        // snapshots; discard stale/out-of-order without repainting.
        if snap.seq <= self.seq {
            return false;
        }
        self.seq = snap.seq;
        self.agents = snap.agents;
        true // request a re-render
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        for a in &self.agents {
            // One glyph; the FONT COLOUR encodes state (spec §6.5). Raw ANSI SGR
            // codes — Zellij interprets escape sequences in plugin output.
            let (glyph, color) = match a.status {
                Status::NeedsYou => ('●', 31), // red
                Status::Working => ('●', 33),  // amber / yellow
                Status::Done => ('●', 32),     // green
                Status::Idle => ('●', 90),     // dim (bright black)
                Status::Failed => ('✖', 31),   // red cross
            };
            println!("\u{1b}[{color}m{glyph}\u{1b}[0m {}", a.label);
        }
    }
}

// A binary crate needs a `main`; `register_plugin!` supplies the plugin exports
// (load/update/render/pipe), not `main`, so this stays empty.
fn main() {}
```

- [ ] **Step 3: Build the plugin**

Run: `cargo build -p clave-bar --target wasm32-wasip1`
Expected: compiles clean; `target/wasm32-wasip1/debug/clave-bar.wasm` refreshed.

- [ ] **Step 4: Write the test layout**

Create `spikes/layouts/s1.kdl` (replace the wasm path with the absolute path printed by `pwd`; layouts do **not** expand `~` or env vars):
```kdl
// S1 test layout: [ clave-bar | shell ] side by side. The plugin pane is on the
// LEFT and we never focus it — we drive it entirely via `zellij pipe`.
layout {
    pane split_direction="vertical" {
        pane size="26" {
            plugin location="file:/Users/olliegilbey/code/clave/target/wasm32-wasip1/debug/clave-bar.wasm"
        }
        pane
    }
}
```

- [ ] **Step 5: Launch a throwaway Zellij session with the layout**

Run:
```bash
zellij --session clave-s1 --layout "$(pwd)/spikes/layouts/s1.kdl"
```
On first load, Zellij prompts to grant the plugin's `ReadCliPipes` permission — **approve it**. Focus stays in the right-hand shell pane. Keep this session open for the next steps.

- [ ] **Step 6: Push a snapshot and observe a NON-focused repaint**

From the right-hand (focused) shell pane, run:
```bash
zellij pipe --name clave-status -- '{"seq":1,"agents":[{"uuid":"u1","cwd":"/x","repo_root":"/x","branch":"main","label":"x · main · working","status":"working","last_interacted":1,"last_visited":0,"archived":false}]}'
```
Expected: the **left** (non-focused) plugin pane shows `● x · main · working` with an **amber** glyph. Focus did **not** move.

- [ ] **Step 7: Push a newer snapshot and confirm live state change**

Run:
```bash
zellij pipe --name clave-status -- '{"seq":2,"agents":[{"uuid":"u1","cwd":"/x","repo_root":"/x","branch":"main","label":"x · main · needs you","status":"needs_you","last_interacted":2,"last_visited":0,"archived":false}]}'
```
Expected: the glyph turns **red** and the label updates — live, still no focus steal.

- [ ] **Step 8: Push a stale seq and confirm it is ignored (seq gating)**

Run:
```bash
zellij pipe --name clave-status -- '{"seq":1,"agents":[{"uuid":"u1","cwd":"/x","repo_root":"/x","branch":"main","label":"STALE","status":"done","last_interacted":9,"last_visited":0,"archived":false}]}'
```
Expected: **no change** — the row still shows the red `needs you` state from seq 2 (the plugin discarded the lower-seq message).

- [ ] **Step 9: Tear down the session**

Run: `zellij delete-session clave-s1 --force` (or `exit` the panes first).

- [ ] **Step 10: Record the verdict**

Create `docs/superpowers/spikes/S1.md`:
- **Verdict (PASS/FAIL).** PASS = non-focused glyph/colour updated live on pipe, stale seq ignored, focus never moved.
- **Plugin-load form:** did the binary-crate wasm load and render? If it failed to load, the **fallback** is a cdylib — add `[lib] crate-type=["cdylib"]`, move the code to `src/lib.rs`, drop `fn main`, and rebuild (artifact becomes `clave_bar.wasm`, so update the layout path). Record which form loaded.
- **Notes:** the exact permission prompt text, any Zellij log noise, ANSI rendering fidelity.
- **On FAIL:** **STOP.** Do not start Task 6 or any subsystem. Revisit spec §3 (rename_tab painting / fork cfal) and re-brief.

- [ ] **Step 11: Commit**

```bash
git add crates/clave-bar spikes/layouts/s1.kdl docs/superpowers/spikes/S1.md
git commit -m "spike(s1): clave-bar renders + repaints a non-focused row from clave-status"
```

---

## Task 6: Spike S2 — uuid→pane join

**Run only after S1 PASSES.** Prove the plugin can turn a displayed agent row into a focus jump: confirm `$ZELLIJ_PANE_ID` is exported to a pane, have the plugin ingest `clave-register {uuid, pane_id}`, track pane→tab from `PaneManifest`, and `go_to_tab` correctly even after tabs are reordered/closed. (In production, the nav trigger is a `MessagePlugin` keybind → the plugin, per spec §6.6; the `clave-nav` pipe used here stands in for that keybind so the spike is drivable from the shell.)

**Files:**
- Modify: `crates/clave-bar/src/main.rs` (register map, pane→tab map, nav handler; add permissions + subscription)
- Create: `spikes/layouts/s2.kdl` (multi-tab test layout)
- Create: `spikes/s2-register.sh` (helper each test pane runs to register itself)
- Create: `docs/superpowers/spikes/S2.md` (findings log)

**Interfaces:**
- Consumes: `clave_types::Register` (Task 2); `$ZELLIJ_PANE_ID`.
- Produces: a plugin that resolves `uuid → pane_id → tab position → go_to_tab`. Verified: env var exported; nav jumps focus to the right tab after reorder/close.

- [ ] **Step 1: Confirm `$ZELLIJ_PANE_ID` is exported**

In any pane inside a Zellij session, run:
```bash
echo "ZELLIJ_PANE_ID=[$ZELLIJ_PANE_ID]"
```
Expected: a non-empty integer (e.g. `[1]`). If empty ⇒ record the **fallback** (register-while-active heuristic, or match on pane cwd/title) in S2.md and adjust the spike accordingly before continuing.

- [ ] **Step 2: Extend the plugin with the join maps and nav handler**

Overwrite `crates/clave-bar/src/main.rs` (this supersedes the S1 version, keeping its snapshot rendering and adding the join):
```rust
//! clave-bar — the Zellij WASM plugin that renders the agent sidebar.
//! S1 scope: consume `clave-status` snapshots and render colored glyphs.
//! S2 scope: map uuid → pane_id (from `clave-register`) → live tab position
//! (from `PaneManifest`) and `go_to_tab` on a `clave-nav {uuid}` message.

use std::collections::BTreeMap;
use zellij_tile::prelude::*;
use clave_types::{Agent, AgentSnapshot, Register, Status};

#[derive(Default)]
struct State {
    seq: u64,
    agents: Vec<Agent>,
    /// uuid → pane_id, learned from `clave-register` messages (spec §6.1).
    uuid_to_pane: BTreeMap<String, u32>,
    /// pane_id → tab position, rebuilt from every `PaneManifest` (spec §6.6/S2).
    pane_to_tab: BTreeMap<u32, usize>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        // ReadCliPipes: receive status/register/nav pipes.
        // ChangeApplicationState: call go_to_tab.
        request_permission(&[
            PermissionType::ReadCliPipes,
            PermissionType::ChangeApplicationState,
        ]);
        // PaneUpdate delivers the PaneManifest we use for pane→tab resolution.
        subscribe(&[EventType::PaneUpdate]);
    }

    fn update(&mut self, event: Event) -> bool {
        if let Event::PaneUpdate(manifest) = event {
            // manifest.panes: tab_position -> panes in that tab.
            self.pane_to_tab.clear();
            for (tab_index, panes) in manifest.panes {
                for p in panes {
                    self.pane_to_tab.insert(p.id, tab_index);
                }
            }
        }
        false // no repaint needed for join-map bookkeeping
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        match message.name.as_str() {
            "clave-status" => {
                let Some(payload) = message.payload else { return false };
                let Ok(snap) = serde_json::from_str::<AgentSnapshot>(&payload) else {
                    return false;
                };
                if snap.seq <= self.seq {
                    return false;
                }
                self.seq = snap.seq;
                self.agents = snap.agents;
                true
            }
            "clave-register" => {
                let Some(payload) = message.payload else { return false };
                if let Ok(reg) = serde_json::from_str::<Register>(&payload) {
                    self.uuid_to_pane.insert(reg.uuid, reg.pane_id);
                }
                false
            }
            "clave-nav" => {
                // Payload: {"uuid":"..."} — jump focus to that agent's tab.
                let Some(payload) = message.payload else { return false };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload) else {
                    return false;
                };
                let Some(uuid) = v.get("uuid").and_then(|u| u.as_str()) else {
                    return false;
                };
                if let Some(pane) = self.uuid_to_pane.get(uuid) {
                    if let Some(tab) = self.pane_to_tab.get(pane) {
                        // NOTE: confirm go_to_tab indexing during the spike —
                        // PaneManifest tab keys and go_to_tab may differ by 1.
                        go_to_tab((*tab as u32) + 1);
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        for a in &self.agents {
            let (glyph, color) = match a.status {
                Status::NeedsYou => ('●', 31),
                Status::Working => ('●', 33),
                Status::Done => ('●', 32),
                Status::Idle => ('●', 90),
                Status::Failed => ('✖', 31),
            };
            println!("\u{1b}[{color}m{glyph}\u{1b}[0m {}", a.label);
        }
    }
}

// A binary crate needs a `main`; `register_plugin!` supplies the plugin exports
// (load/update/render/pipe), not `main`, so this stays empty.
fn main() {}
```

- [ ] **Step 3: Build the plugin**

Run: `cargo build -p clave-bar --target wasm32-wasip1`
Expected: compiles clean. If `Event::PaneUpdate`, `PaneManifest.panes`, `PaneInfo.id`, or `go_to_tab` do not match these names/shapes in `zellij-tile` 0.44, consult `docs.rs/zellij-tile/0.44` and adjust — record any signature deltas in S2.md (they matter for the real §6.6 build).

- [ ] **Step 4: Write a self-registration helper for test panes**

Create `spikes/s2-register.sh`:
```bash
#!/usr/bin/env bash
# S2 helper: a test pane registers its own pane_id under a given uuid, then
# drops to a shell so the pane stays alive and focusable.
# usage: s2-register.sh <uuid>
set -euo pipefail
UUID="${1:?usage: s2-register.sh <uuid>}"
echo "pane $ZELLIJ_PANE_ID registering as uuid=$UUID"
zellij pipe --name clave-register -- "{\"uuid\":\"$UUID\",\"pane_id\":$ZELLIJ_PANE_ID}"
exec "${SHELL:-/bin/zsh}"
```
Then: `chmod +x spikes/s2-register.sh`

- [ ] **Step 5: Write a multi-tab test layout**

Create `spikes/layouts/s2.kdl` (again, absolute wasm path):
```kdl
// S2 test layout: the clave-bar plugin in tab 1, plus two agent panes that
// register themselves under uuids u1 and u2. Reorder/close tabs to test the
// join's resilience.
layout {
    tab name="bar" {
        pane split_direction="vertical" {
            pane size="26" {
                plugin location="file:/Users/olliegilbey/code/clave/target/wasm32-wasip1/debug/clave-bar.wasm"
            }
            pane
        }
    }
    tab name="u1" {
        pane command="/Users/olliegilbey/code/clave/spikes/s2-register.sh" {
            args "u1"
        }
    }
    tab name="u2" {
        pane command="/Users/olliegilbey/code/clave/spikes/s2-register.sh" {
            args "u2"
        }
    }
}
```

- [ ] **Step 6: Launch and drive the join**

Run:
```bash
zellij --session clave-s2 --layout "$(pwd)/spikes/layouts/s2.kdl"
```
Approve the plugin permissions. The `u1`/`u2` panes each print their registration line. Then, from any pane, jump to `u2`:
```bash
zellij pipe --name clave-nav -- '{"uuid":"u2"}'
```
Expected: focus moves to the **u2** tab. Try `'{"uuid":"u1"}'` → focus moves to u1. If focus lands on the wrong tab, the `go_to_tab` off-by-one is the cause — adjust the `+ 1` in Step 2 and rebuild.

- [ ] **Step 7: Test resilience to reorder/close**

In the running session, move a tab (`Alt`+nav or Zellij's move-tab keybind) and/or close the `u1` tab, then re-run:
```bash
zellij pipe --name clave-nav -- '{"uuid":"u2"}'
```
Expected: focus still lands on u2 correctly (the `PaneUpdate` subscription keeps `pane_to_tab` current). Navigating to a closed uuid is a no-op (acceptable for the spike; the real bar won't list closed agents).

- [ ] **Step 8: Tear down**

Run: `zellij delete-session clave-s2 --force`

- [ ] **Step 9: Record the verdict**

Create `docs/superpowers/spikes/S2.md`:
- **`$ZELLIJ_PANE_ID` exported?** (yes/no; value seen).
- **Verdict (PASS/FAIL):** did nav jump to the correct tab, and stay correct after reorder/close?
- **`go_to_tab` indexing:** 0- or 1-based (what worked).
- **zellij-tile API deltas:** any `Event::PaneUpdate`/`PaneManifest`/`PaneInfo`/`go_to_tab` signature differences from Step 2 (these carry into the real §6.6 plugin).
- **Fallbacks if FAIL:** env var absent ⇒ register-while-active heuristic or match on pane cwd/title; wrong-tab focus ⇒ document the correct index mapping.

- [ ] **Step 10: Commit**

```bash
git add crates/clave-bar spikes/s2-register.sh spikes/layouts/s2.kdl docs/superpowers/spikes/S2.md
git commit -m "spike(s2): uuid→pane→tab join via clave-register + go_to_tab"
```

---

## After the spikes — the gate decision

- **All of S0/S0b/S1/S2 PASS** ⇒ the join key and the plugin architecture are proven. Proceed to the **subsystem plan** (a separate `/superpowers:writing-plans` pass) in spec dependency order: `clave spawn` (§6.1) → state store + `ls` (§6.2) → `clave hook` + status state machine (§6.5) → full `clave-bar` (§6.6) → `clave add` + temp-layout tab creation + fzf (§6.3) → naming (§6.4) → archiving (§6.7) → session/config + keybinds (§6.8). The S1/S2 plugin becomes the foundation of the real §6.6 bar.
- **S0 or S0b FAILS** ⇒ the idempotency model is wrong; fix `munge_cwd`/spec §4/§6.1 and re-run before any spawn work.
- **S1 FAILS** ⇒ **STOP.** Revisit spec §3 (rename_tab painting or forking cfal) and re-brief the user before continuing. Do not plan subsystems on an unproven architecture.

---

## Self-Review

**1. Spec coverage (foundation+spikes scope only — subsystems §6.1–6.8 are intentionally deferred to the next plan):**
- §7 workspace / three crates / `default-members` / WASM target → Task 1. ✓
- §5 data model + pipe contract (`seq`, full-replace) → Task 2 (`AgentSnapshot`) + enforced in S1 seq-gating. ✓
- §4 munging rule (join key) → Task 3, pinned by Task 4 (S0b). ✓
- §9 S0 (`--session-id` create) → Task 4. ✓ · S0b (munge round-trip) → Task 4. ✓ · S1 (background repaint) → Task 5. ✓ · S2 (uuid→pane join) → Task 6. ✓
- Invariant #9 (shared types, no drift) → Task 2 + Task 1 wiring. ✓ · #11 (render from pushed model) → Task 5. ✓ · #3 (uuid join key) → Tasks 3/4/6. ✓
- S3/S4/S5/S6 spikes are **not** in this plan (they belong with the subsystems they gate — S3/focus, S4/tab-creation+resurrection, S5/hydration, S6/context-nav). Noted here as a deliberate gap, consistent with the handoff's "foundation + spikes only" scope.

**2. Placeholder scan:** No `TBD`/`add error handling`/`similar to Task N`. Every code step shows complete code; every command shows expected output. Spike observation steps state exact pass conditions. ✓

**3. Type consistency:** `munge_cwd(&str)->String` used identically in Tasks 3/4. `AgentSnapshot{seq,agents}`, `Agent{...}`, `Status::{Idle,Working,NeedsYou,Done,Failed}`, `Register{uuid,pane_id}` defined in Task 2 and consumed with the same field names in Tasks 5/6. Pipe names `clave-status`/`clave-register`/`clave-nav` consistent across plugin code and the `zellij pipe` commands. ✓

**Known live uncertainties flagged inline (not placeholders — genuine spike unknowns):** `claude -p` persistence (Task 4 Step 3 fallback), plugin-load form binary-vs-cdylib (Task 5 Step 10 fallback), `zellij-tile` 0.44 exact API names for `PaneManifest`/`go_to_tab` and its tab indexing (Task 6 Steps 2–3, 6).
