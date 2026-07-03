# clave Vertical-Tabs Subsystems Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build all clave v1 subsystems on top of the proven foundation: the vertical dynamic tab bar (zellij-truth rows, recency order, status decoration), the idempotent spawn, the hook-driven status pipeline, the add/resume flow, and the session/setup machinery.

**Architecture:** The bar's row **set** comes from Zellij (`TabUpdate` — every tab, Claude or plain), its **order** from plugin-tracked interaction recency, and its **decoration** (glyph/colour/label) from clave's pushed snapshots (spec §5 pipe contract, S1-proven). Labels are written onto **real tabs** via `rename_tab_with_id`; nav is `focus_pane_with_id` (S2-proven — `go_to_tab` is a dead end). The native binary stays a thin clap shell over testable library modules; the plugin keeps all display logic in a zellij-tile-free `model.rs` so it unit-tests on the host.

**Tech Stack:** Rust (edition 2024, resolver 3), `serde`/`serde_json`, `clap`, `uuid`, `dirs`, `fs4` (file locking), `zellij-tile` 0.44 (→ `wasm32-wasip1`), the real `claude` CLI (v2.1.197), Zellij 0.44.3, `fzf` + `zoxide`, `just`.

**Execution mode (read before dispatching):** Tasks 1–8 are automatable TDD — dispatch each to a fresh subagent, review, merge. Task 9 is **human-in-the-loop interactive validation** (live Zellij session, real `claude`, visual observation): a subagent may author scripts/fixtures, but launch → observe → verdict needs the user + main session. **Never mark a Task 9 checkpoint PASS from a headless subagent.** Task 10 is an automatable sweep + final review.

## Global Constraints

_Every task's requirements implicitly include this section. Values copied verbatim from the canonical spec `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` (as revised 2026-07-03) and the SDD ledger._

- **Canonical spec is law.** Read the referenced §-sections before each task. If this plan contradicts the spec, the spec wins — stop and flag it.
- **Toolchain:** Rust stable (`rustc 1.96.1` verified), edition `2024`, workspace `resolver = "3"`. Latest dep majors.
- **Workspace shape (§7):** `crates/clave` (bin+lib, host), `crates/clave-bar` (**binary crate**, `src/main.rs` + `register_plugin!`, **NEVER add an explicit `fn main()`** — the macro supplies it, a second is E0428), `crates/clave-types` (serde-only). `default-members` excludes `clave-bar`; build it with `cargo build -p clave-bar --target wasm32-wasip1`.
- **`clave-types` depends on nothing but `serde`** at runtime; `serde_json` as dev-dep only.
- **JOIN KEY (S0b, verified on disk):** callers MUST `std::fs::canonicalize` the cwd **before** `munge_cwd` (Claude munges the *physical* `getcwd()`; on macOS `/var`→`/private/var`). The munge rule `s/[^A-Za-z0-9]/-/g` is already implemented in `crates/clave/src/munge.rs` — reuse, never reimplement.
- **Pipe contract (§5):** every `clave-status` message is a **full replace** with monotonic `seq`; consumers apply only strictly-newer `seq`. `seq` is persisted in the store so it stays monotonic across processes.
- **Status enum (§5/§6.5):** `idle | working | needs_you | done | failed`, exactly these snake_case strings.
- **Nav mechanism (S2 verdict):** `focus_pane_with_id(PaneId::Terminal(pane_id), false, false)`. **`go_to_tab` is a known dead end — do not use it.** `switch_tab_to` may be *attempted* only in Task 9's checkpoint.
- **Plugin permission set (§6.6):** exactly `ReadCliPipes + ChangeApplicationState + ReadApplicationState + RunCommands`. Grants are **all-or-nothing per plugin**; the prompt is unanswerable in the bar pane → `clave setup` pre-seeds `permissions.kdl` under BOTH `"file:<abs>.wasm"` and `"<abs>.wasm"` key forms. When the requested set changes, the seed must change with it (this re-bit S2).
- **Pipe hygiene (final-review fix `dd38ace`):** plugin pipe handling stays split as `handle_pipe()` + **unconditional** `unblock_cli_pipe_input` in `pipe()`; malformed payloads are `eprintln!`-logged (lands in the zellij log) and dropped.
- **`clave hook` is a zero-risk global citizen (§6.5):** untracked session → **lock-free** store read → exit 0; any internal error → exit 0; never emits a permission decision; the snapshot push is fire-and-forget (never blocks the hook).
- **Store (§5):** single JSON at `~/.local/state/clave/agents.json`; RMW under an advisory lock on the **separate never-renamed lockfile** `agents.lock` (`fs4`, not unmaintained `fs2`); data write = temp file + atomic rename. **Locking the data file directly is a bug.**
- **Repo is public:** no secrets, no machine-specific absolute paths in committed code (`spikes/` remains the sanctioned exception). Generated machine-local files (config/layout/permissions) live under `~/.local/share/clave/`, never in the repo.
- **Comment density:** heavily-commented code, the *why* — including crate manifests (a deferred Task-1 minor this plan sweeps).
- **Commits:** conventional-commit style, one per task-deliverable; stage **explicit paths** (never `git add -A`); stage `Cargo.lock` only when deps change. The executing agent appends its **own** `Claude-Session: <url>` trailer. Solo public repo commits straight to `main`. **Ask before committing.** If signing fails with `1Password: failed to fill whole buffer`, ask the user to unlock 1Password and retry (staging is preserved).
- **Zellij launch forms (S1/S2):** adding tabs to a NEW session uses `-n <layout>`, not `--layout` (with `--layout`, `--session` means "add to an EXISTING session"). Plugin `eprintln!` lands in `$TMPDIR/zellij-<uid>/zellij-log/zellij.log`.

---

## File Structure

| Path | Responsibility |
|---|---|
| `crates/clave-types/src/lib.rs` | Pipe schema. **Drop `Agent.archived`** (§6.7 deleted); add `Status::glyph()` (single source for glyph/colour, used by bar AND `ls`); exhaustive round-trip tests (sweeps the Task-2 minor). |
| `crates/clave/src/store.rs` | §5 store: `AgentRecord`, `Store` (persisted `seq`), lock-free `read_store`, locked `with_store_mut` (lockfile + temp+rename), `snapshot_from`, `now_unix`. |
| `crates/clave/src/hook.rs` | §6.5 status state machine + §6.4 label derivation (payload prompt / jsonl tail-scan) + fire-and-forget `push_snapshot`. |
| `crates/clave/src/spawn.rs` | §6.1 resume-or-create decision (`spawn_mode`) + register payload; `main.rs` does the `exec`. |
| `crates/clave/src/add.rs` | §6.3 pure parts: `live_uuids` (dump-layout parse), resume candidates, `tab_layout` templating, label sanitising. |
| `crates/clave/src/setup.rs` | §6.8/§7: config/layout KDL generation, `merge_hooks` (additive settings.json), `merge_permissions_kdl` (both key forms), paths (`~/.local/share/clave/`). |
| `crates/clave/src/lib.rs` | Adds `pub mod store; pub mod hook; pub mod spawn; pub mod add; pub mod setup;`. |
| `crates/clave/src/main.rs` | clap wiring for `spawn/hook/ls/snapshot/focus/add/setup` + bare-`clave` session launcher; fixes stale crate doc ("status emoji into the tab title"). |
| `crates/clave-bar/src/model.rs` | **Pure** bar model (no zellij-tile): rows/recency/join/rename-guard/click/nav/unread as data-in→`Effect`-out. Host-testable. |
| `crates/clave-bar/src/main.rs` | Thin zellij adapter: subscriptions, permission request (4-set), `handle_pipe`+unconditional unblock, effect execution, render, hydrate via `run_command`. |
| `Cargo.toml` (root) | Add `fs4`, `tempfile` (dev) to `[workspace.dependencies]`. |
| `justfile` | Add `install` (binary + wasm → `~/.local/share/clave/` + PATH) and `clippy` recipes. |
| `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` | Task 9 findings log (checkpoint verdicts, incl. `hide_self`, tab-template, `switch_tab_to`, dump-layout, S4, S5). |

Dependency order: Task 1 (types) → Task 2 (store) → Task 3 (`ls`/`snapshot`/`focus`) → Task 4 (`spawn`) → Task 5 (`hook`) → Task 6 (bar) → Task 7 (`add`) → Task 8 (session/config/`setup`) → Task 9 (interactive validation) → Task 10 (sweep + final review).

---

## Task 1: `clave-types` schema revision (drop `archived`, add `Status::glyph`)

Spec: §5 (pipe schema), §6.7 (archived deleted), §6.5 (glyph table). Sweeps the deferred Task-2 minor (doc/test asymmetry).

**Files:**
- Modify: `crates/clave-types/src/lib.rs`
- Modify: `crates/clave-types/Cargo.toml` (why-comments only)

**Interfaces:**
- Consumes: nothing.
- Produces: `Agent { uuid, cwd, repo_root, branch, label, status, last_interacted, last_visited }` (NO `archived`), `Status::glyph(self) -> (char, u8)` returning `(glyph_char, ansi_sgr_colour)`: `NeedsYou→('●',31)`, `Working→('●',33)`, `Done→('●',32)`, `Idle→('●',90)`, `Failed→('✖',31)`. `AgentSnapshot`/`Register` unchanged.

- [ ] **Step 1: Write the failing tests** — replace the existing test module additions: a glyph table test and an exhaustive serialize+deserialize test (fixes the "deserialize only exercises needs_you" minor). Update `snapshot_roundtrips` to drop `archived: false`.

```rust
    #[test]
    fn status_glyph_encodes_state_colour() {
        // Spec §6.5 glyph table — single source shared by the bar and `clave ls`.
        assert_eq!(Status::NeedsYou.glyph(), ('●', 31)); // red
        assert_eq!(Status::Working.glyph(), ('●', 33));  // amber
        assert_eq!(Status::Done.glyph(), ('●', 32));     // green (done & unread)
        assert_eq!(Status::Idle.glyph(), ('●', 90));     // dim
        assert_eq!(Status::Failed.glyph(), ('✖', 31));   // red cross
    }

    #[test]
    fn status_roundtrips_every_variant() {
        // Exhaustive BOTH ways (the old deserialize test only covered needs_you).
        for (v, s) in [
            (Status::Idle, "\"idle\""),
            (Status::Working, "\"working\""),
            (Status::NeedsYou, "\"needs_you\""),
            (Status::Done, "\"done\""),
            (Status::Failed, "\"failed\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), s);
            assert_eq!(serde_json::from_str::<Status>(s).unwrap(), v);
        }
    }

    #[test]
    fn agent_json_has_no_archived_field() {
        // §6.7 deleted archiving; the pipe schema must not carry the field.
        let a = Agent {
            uuid: "u1".into(), cwd: "/x".into(), repo_root: "/x".into(),
            branch: "main".into(), label: "x · main".into(),
            status: Status::Idle, last_interacted: 0, last_visited: 0,
        };
        assert!(!serde_json::to_string(&a).unwrap().contains("archived"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave-types`
Expected: FAIL — `glyph` not found, `Agent` has an `archived` field (struct literal mismatch).

- [ ] **Step 3: Implement** — in `crates/clave-types/src/lib.rs`: delete the `pub archived: bool` field (and its doc comment) from `Agent`; delete `archived: false` from the old round-trip test; add field-level doc comments to every `Agent` field (the Task-2 minor); add:

```rust
impl Status {
    /// The bar/ls glyph: one char whose FONT COLOUR encodes state (spec §6.5).
    /// Returned as (glyph, ANSI SGR colour code) so both artifacts render
    /// identically — raw ANSI SGR is proven to render in a plugin pane (S1).
    pub fn glyph(self) -> (char, u8) {
        match self {
            Status::NeedsYou => ('●', 31), // red: waiting on the human
            Status::Working => ('●', 33),  // amber: agent is running
            Status::Done => ('●', 32),     // green: finished & unread
            Status::Idle => ('●', 90),     // dim: read / no session
            Status::Failed => ('✖', 31),   // red cross: turn failed
        }
    }
}
```

Also update `crates/clave-types/Cargo.toml` with why-comments (serde-only = compiles for host AND wasm; serde_json dev-only for round-trip tests) — the deferred Task-1 minor for this manifest.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p clave-types && cargo build -p clave-bar --target wasm32-wasip1`
Expected: all tests PASS; the wasm build still compiles (the plugin never names `archived`, serde ignores unknown/missing is not involved — field is gone from both sides).

- [ ] **Step 5: Commit**

```bash
git add crates/clave-types/src/lib.rs crates/clave-types/Cargo.toml
git commit -m "feat(clave-types): drop Agent.archived (spec §6.7 deleted); add Status::glyph"
```

---

## Task 2: State store (`store.rs`)

Spec: §5 verbatim — read it first. The lockfile/atomic-rename discipline is load-bearing for the global-hook fan-in.

**Files:**
- Create: `crates/clave/src/store.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod store;`)
- Modify: `Cargo.toml` (root — add `fs4 = "0.13"` and `tempfile = "3"` to `[workspace.dependencies]`), `crates/clave/Cargo.toml` (use them; `tempfile` as dev-dep; add manifest why-comments — deferred minor)

**Interfaces:**
- Consumes: `clave_types::{Agent, AgentSnapshot, Status}`.
- Produces:
  - `pub enum LabelSource { FirstPrompt, Summary }` (serde snake_case)
  - `pub struct AgentRecord { pub uuid: String, pub cwd: String, pub repo_root: String, pub branch: String, pub label: String, pub status: Status, pub last_interacted: u64, pub last_visited: u64, pub worktree: Option<String>, pub label_source: LabelSource }`
  - `pub struct Store { pub seq: u64, pub agents: BTreeMap<String, AgentRecord> }` (keyed by uuid; both fields `#[serde(default)]`)
  - `pub struct StorePaths { pub dir: PathBuf, pub data: PathBuf, pub lock: PathBuf }`
  - `pub fn store_paths() -> anyhow::Result<StorePaths>` — `~/.local/state/clave/{agents.json,agents.lock}` built from `$HOME` (NOT `dirs::state_dir`, which is `None` on macOS; spec §5 names the literal path)
  - `pub fn read_store(paths: &StorePaths) -> anyhow::Result<Store>` — lock-free; missing file → `Store::default()`
  - `pub fn with_store_mut<T>(paths: &StorePaths, f: impl FnOnce(&mut Store) -> T) -> anyhow::Result<T>` — flock lockfile across whole RMW; temp+rename write
  - `pub fn snapshot_from(store: &Store) -> AgentSnapshot`
  - `pub fn now_unix() -> u64`

- [ ] **Step 1: Write the failing tests** (in `store.rs` `#[cfg(test)]`; tests take `StorePaths` pointed at a `tempfile::tempdir()` so nothing touches the real store):

```rust
    fn tmp_paths(dir: &std::path::Path) -> StorePaths {
        StorePaths {
            dir: dir.to_path_buf(),
            data: dir.join("agents.json"),
            lock: dir.join("agents.lock"),
        }
    }

    fn rec(uuid: &str) -> AgentRecord {
        AgentRecord {
            uuid: uuid.into(), cwd: "/x".into(), repo_root: "/x".into(),
            branch: "main".into(), label: "x · main".into(), status: Status::Idle,
            last_interacted: 0, last_visited: 0, worktree: None,
            label_source: LabelSource::FirstPrompt,
        }
    }

    #[test]
    fn missing_file_reads_as_default() {
        let d = tempfile::tempdir().unwrap();
        let s = read_store(&tmp_paths(d.path())).unwrap();
        assert_eq!(s.seq, 0);
        assert!(s.agents.is_empty());
    }

    #[test]
    fn rmw_roundtrips_and_bumps_seq() {
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
            s.seq += 1;
        })
        .unwrap();
        // A second, separate RMW sees the first one's write (file-backed).
        with_store_mut(&p, |s| {
            assert_eq!(s.seq, 1);
            s.agents.get_mut("u1").unwrap().status = Status::Working;
            s.seq += 1;
        })
        .unwrap();
        let s = read_store(&p).unwrap();
        assert_eq!(s.seq, 2);
        assert_eq!(s.agents["u1"].status, Status::Working);
        // The write went through temp+rename: no stray temp file remains.
        assert!(!p.dir.join("agents.json.tmp").exists());
        // The lockfile exists and was NOT renamed away (it is the lock anchor).
        assert!(p.lock.exists());
    }

    #[test]
    fn snapshot_mirrors_store_rows() {
        let mut s = Store::default();
        s.seq = 7;
        s.agents.insert("u1".into(), rec("u1"));
        let snap = snapshot_from(&s);
        assert_eq!(snap.seq, 7);
        assert_eq!(snap.agents.len(), 1);
        assert_eq!(snap.agents[0].uuid, "u1");
        assert_eq!(snap.agents[0].label, "x · main");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave store::`
Expected: FAIL to compile — module/types don't exist yet.

- [ ] **Step 3: Implement `store.rs`**

```rust
//! The clave state store (spec §5): one JSON file, read-modify-written under
//! an advisory lock held on a SEPARATE, never-renamed lockfile, with the data
//! write done as temp-file + atomic rename.
//!
//! WHY the separate lockfile: locking the data file itself would be a bug —
//! the atomic rename swaps the data file's inode out from under a second
//! writer's lock, and concurrent hooks (Claude's global hook fan-in) would
//! silently lose updates. The lockfile never gets renamed, so its inode is a
//! stable lock anchor. The flip side: readers that only need a consistent
//! (possibly slightly stale) view can skip the lock entirely — an
//! atomic-rename reader always sees a whole file. That lock-free path is what
//! keeps `clave hook` from ever delaying an UNTRACKED Claude session (§6.5).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clave_types::{Agent, AgentSnapshot, Status};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

/// Where an agent's label came from (§6.4). While `FirstPrompt`, `clave hook`
/// keeps tail-scanning the jsonl for a session summary; once `Summary`, it
/// stops re-scanning forever (the label only meaningfully changes once).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelSource {
    FirstPrompt,
    Summary,
}

/// One store row (spec §5's agent record, minus the deleted `archived`).
/// Mirrors `clave_types::Agent` plus store-only fields (`worktree`,
/// `label_source`) that the plugin never needs to see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    /// Minted session UUID — the join key (invariant #3).
    pub uuid: String,
    /// PHYSICAL (canonicalized) working directory — see munge.rs / S0b.
    pub cwd: String,
    /// git toplevel of cwd; keys the §6.3 add/resume picker.
    pub repo_root: String,
    pub branch: String,
    /// `dir · branch · summary-or-first-prompt` (§6.4).
    pub label: String,
    pub status: Status,
    /// unix s; bumped on UserPromptSubmit → drives the bar's recency order.
    pub last_interacted: u64,
    /// unix s; bumped on focus (`clave focus`) → clears done-unread.
    pub last_visited: u64,
    /// Worktree path if `clave add --worktree` created one (§6.3), else None.
    pub worktree: Option<String>,
    pub label_source: LabelSource,
}

/// The whole store file. `seq` is the monotonic snapshot counter of the §5
/// pipe contract — persisted HERE so pushes stay monotonic across processes
/// (every hook invocation is a fresh process).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub seq: u64,
    /// Keyed by uuid. BTreeMap for stable iteration order (deterministic
    /// snapshots and `ls` output).
    #[serde(default)]
    pub agents: BTreeMap<String, AgentRecord>,
}

pub struct StorePaths {
    pub dir: PathBuf,
    pub data: PathBuf,
    pub lock: PathBuf,
}

/// Spec §5 names the literal path `~/.local/state/clave/`. Built from $HOME
/// rather than `dirs::state_dir()` because the latter is `None` on macOS and
/// we want one path on every platform.
pub fn store_paths() -> Result<StorePaths> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let dir = home.join(".local").join("state").join("clave");
    Ok(StorePaths {
        data: dir.join("agents.json"),
        lock: dir.join("agents.lock"),
        dir,
    })
}

/// Lock-free read. Safe without the lock because writers replace the file by
/// atomic rename — a reader opens either the old whole file or the new whole
/// file, never a torn write. Missing file = empty store (first run).
pub fn read_store(paths: &StorePaths) -> Result<Store> {
    match fs::read(&paths.data) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(e).context("reading store"),
    }
}

/// Locked read-modify-write. The exclusive flock on the lockfile is held
/// across the WHOLE read→mutate→write, so concurrent hook events serialize
/// instead of clobbering each other. The closure's return value passes
/// through so callers can extract data computed under the lock.
pub fn with_store_mut<T>(paths: &StorePaths, f: impl FnOnce(&mut Store) -> T) -> Result<T> {
    fs::create_dir_all(&paths.dir).context("creating store dir")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&paths.lock)
        .context("opening lockfile")?;
    lock.lock_exclusive().context("locking store")?;
    // From here to the end of the function we hold the lock; any early return
    // (via ?) drops `lock`, which releases the flock — no poisoned state.
    let mut store = read_store(paths)?;
    let out = f(&mut store);
    let tmp = paths.dir.join("agents.json.tmp");
    {
        let mut t = fs::File::create(&tmp).context("creating temp store")?;
        t.write_all(&serde_json::to_vec_pretty(&store)?)?;
        // fsync BEFORE the rename so a crash can't leave a renamed-but-empty
        // file (rename is atomic, buffered content is not).
        t.sync_all()?;
    }
    fs::rename(&tmp, &paths.data).context("atomic store swap")?;
    FileExt::unlock(&lock).context("unlocking store")?;
    Ok(out)
}

/// Store → pipe snapshot (§5): drop the store-only fields, keep the order.
pub fn snapshot_from(store: &Store) -> AgentSnapshot {
    AgentSnapshot {
        seq: store.seq,
        agents: store
            .agents
            .values()
            .map(|r| Agent {
                uuid: r.uuid.clone(),
                cwd: r.cwd.clone(),
                repo_root: r.repo_root.clone(),
                branch: r.branch.clone(),
                label: r.label.clone(),
                status: r.status,
                last_interacted: r.last_interacted,
                last_visited: r.last_visited,
            })
            .collect(),
    }
}

/// Seconds since the epoch — the store's one timestamp format.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
```

Root `Cargo.toml` `[workspace.dependencies]` additions (with why-comments):

```toml
# Advisory file locking for the state store (spec §5). fs4, NOT fs2 — fs2 is
# unmaintained; the spec names fs4 explicitly.
fs4 = "0.13"
# Store tests write to throwaway dirs, never the real ~/.local/state/clave.
tempfile = "3"
```

`crates/clave/Cargo.toml`: add `fs4.workspace = true` under `[dependencies]`, and a `[dev-dependencies]` section with `tempfile.workspace = true`. While in the file, add the deferred why-comments (one line per dep group: CLI parsing, store locking, schema sharing…).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p clave`
Expected: new store tests PASS; the 3 existing munge tests still PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/store.rs crates/clave/src/lib.rs crates/clave/Cargo.toml Cargo.toml Cargo.lock
git commit -m "feat(clave): §5 state store — lockfile+atomic-rename RMW, persisted seq"
```

---

## Task 3: `clave ls`, `clave snapshot`, `clave focus`

Spec: §6.2 (ls/snapshot), §6.5 (unread clears via `clave focus`; the plugin repaints locally — focus does NOT push a pipe).

**Files:**
- Create: `crates/clave/src/lsview.rs` (pure render, testable)
- Modify: `crates/clave/src/lib.rs` (add `pub mod lsview;`)
- Modify: `crates/clave/src/main.rs` (wire `Ls { json }`, `Snapshot`, `Focus { uuid }`; fix the stale crate doc-comment — the bar *decorates rows*, it does not "repaint a status emoji into the tab title")

**Interfaces:**
- Consumes: `store::{read_store, with_store_mut, store_paths, snapshot_from, now_unix, Store}`, `clave_types::Status::glyph`.
- Produces:
  - `pub fn render_ls(store: &Store) -> String` — one line per agent, **recency-sorted desc** (`last_interacted`), format: `<ANSI glyph> <label>  (<repo_root>)`; empty store → `"no agents\n"`.
  - CLI: `clave ls [--json]` (json = raw `AgentSnapshot`); `clave snapshot` (prints `AgentSnapshot` JSON to stdout — the bar hydrates by running this and parsing stdout); `clave focus <uuid>` (store-only: `last_visited = now`, and `Done → Idle`; unknown uuid = silent success — the plugin may race a closed agent).

- [ ] **Step 1: Write the failing tests** (`lsview.rs`):

```rust
    #[test]
    fn ls_sorts_by_recency_desc_and_shows_glyph() {
        let mut s = Store::default();
        let mut a = test_rec("old");            // helper like Task 2's rec()
        a.last_interacted = 100;
        a.status = Status::Done;
        let mut b = test_rec("new");
        b.last_interacted = 200;
        b.status = Status::Working;
        s.agents.insert("old".into(), a);
        s.agents.insert("new".into(), b);
        let out = render_ls(&s);
        let lines: Vec<&str> = out.lines().collect();
        // Most recently interacted first — same ordering rule as the bar.
        assert!(lines[0].contains("new"));
        assert!(lines[1].contains("old"));
        // Working = amber ● (ANSI 33), Done = green ● (ANSI 32).
        assert!(lines[0].contains("\u{1b}[33m●"));
        assert!(lines[1].contains("\u{1b}[32m●"));
    }

    #[test]
    fn ls_empty_store_says_so() {
        assert_eq!(render_ls(&Store::default()), "no agents\n");
    }
```

And a focus test (in `store.rs`'s test module or `lsview.rs` — it exercises the focus mutation helper):

```rust
    #[test]
    fn focus_clears_done_to_idle_and_stamps_visit() {
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut r = rec("u1");
            r.status = Status::Done;
            s.agents.insert("u1".into(), r);
        })
        .unwrap();
        apply_focus(&p, "u1", 999).unwrap();
        let s = read_store(&p).unwrap();
        assert_eq!(s.agents["u1"].status, Status::Idle); // unread cleared
        assert_eq!(s.agents["u1"].last_visited, 999);
        // Unknown uuid: silently fine (plugin may race a just-closed agent).
        apply_focus(&p, "ghost", 1000).unwrap();
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave`
Expected: FAIL to compile — `render_ls`/`apply_focus` don't exist.

- [ ] **Step 3: Implement** — `lsview.rs`:

```rust
//! `clave ls` rendering — pure function so it's testable without a terminal.
//! Ordering matches the bar (§6.6): interaction recency, newest first. Repo
//! shown as a trailing column (grouping was deleted in the 2026-07-03 rev).

use clave_types::Status;

use crate::store::Store;

pub fn render_ls(store: &Store) -> String {
    if store.agents.is_empty() {
        return "no agents\n".to_string();
    }
    let mut rows: Vec<_> = store.agents.values().collect();
    // Same rule the bar uses: most recently interacted first; stable
    // uuid tiebreak (BTreeMap iteration is already uuid-sorted).
    rows.sort_by(|a, b| b.last_interacted.cmp(&a.last_interacted));
    let mut out = String::new();
    for r in rows {
        let (glyph, colour) = r.status.glyph();
        out.push_str(&format!(
            "\u{1b}[{colour}m{glyph}\u{1b}[0m {}  ({})\n",
            r.label, r.repo_root
        ));
    }
    out
}
```

`apply_focus` goes in `store.rs` (it's a store mutation, colocated with the lock discipline):

```rust
/// `clave focus <uuid>` (§6.5): persist the "user looked at it" transition.
/// Store-only — NO pipe push. Every bar instance saw the same TabUpdate focus
/// transition and already repainted locally; this just makes the flip durable
/// (and visible to `clave ls`). Unknown uuid is fine: the plugin can race an
/// agent whose tab just closed.
pub fn apply_focus(paths: &StorePaths, uuid: &str, now: u64) -> Result<()> {
    with_store_mut(paths, |s| {
        if let Some(r) = s.agents.get_mut(uuid) {
            r.last_visited = now;
            if r.status == Status::Done {
                r.status = Status::Idle; // green "done & unread" → dim
            }
        }
    })
}
```

`main.rs` wiring (new arms; `Snapshot` and `Focus` are hidden from help — plugin-internal plumbing):

```rust
        Command::Ls { json } => {
            let paths = store::store_paths()?;
            let s = store::read_store(&paths)?;
            if json {
                println!("{}", serde_json::to_string(&store::snapshot_from(&s))?);
            } else {
                print!("{}", lsview::render_ls(&s));
            }
            Ok(())
        }
        Command::Snapshot => {
            // The bar hydrates on load by running `clave snapshot` via
            // run_command and parsing stdout (spec §6.2/§6.6, was spike S5).
            let paths = store::store_paths()?;
            let s = store::read_store(&paths)?;
            println!("{}", serde_json::to_string(&store::snapshot_from(&s))?);
            Ok(())
        }
        Command::Focus { uuid } => {
            let paths = store::store_paths()?;
            store::apply_focus(&paths, &uuid, store::now_unix())
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p clave && cargo run -p clave -- ls`
Expected: tests PASS; `ls` prints `no agents` (or your real store's rows) instead of the old `todo!` panic.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/lsview.rs crates/clave/src/store.rs crates/clave/src/lib.rs crates/clave/src/main.rs
git commit -m "feat(clave): ls/snapshot/focus — recency-sorted listing, hydration output, unread clear"
```

---

## Task 4: `clave spawn` (idempotent resume-or-create)

Spec: §6.1 verbatim. S0 pinned the semantics: fresh `--session-id` **creates**; a pre-existing uuid with `--session-id` is a **hard error** ("Session ID already in use") — which is exactly why the jsonl existence check must be right, which is why canonicalize-before-munge is a Global Constraint.

**Files:**
- Create: `crates/clave/src/spawn.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod spawn;`), `crates/clave/src/main.rs` (replace the `Spawn` `todo!`)

**Interfaces:**
- Consumes: `munge::munge_cwd`, `clave_types::Register`.
- Produces:
  - `pub enum SpawnMode { Create, Resume }`
  - `pub fn jsonl_path(home: &Path, physical_cwd: &str, uuid: &str) -> PathBuf` — `<home>/.claude/projects/<munge_cwd(physical_cwd)>/<uuid>.jsonl`
  - `pub fn spawn_mode(home: &Path, physical_cwd: &str, uuid: &str) -> SpawnMode` — `Resume` iff the jsonl exists
  - `pub fn register_pane(uuid: &str)` — reads `$ZELLIJ_PANE_ID`, pipes `clave-register` (best-effort: any failure is eprintln-and-continue; spawn must still exec Claude)

- [ ] **Step 1: Write the failing tests** (`spawn.rs`):

```rust
    #[test]
    fn jsonl_path_uses_munged_physical_cwd() {
        let home = std::path::Path::new("/Users/x");
        let p = jsonl_path(home, "/Users/x/code/clave", "u-1");
        assert_eq!(
            p,
            std::path::PathBuf::from(
                "/Users/x/.claude/projects/-Users-x-code-clave/u-1.jsonl"
            )
        );
    }

    #[test]
    fn spawn_mode_is_resume_iff_jsonl_exists() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path();
        let cwd = "/Users/x/code/clave";
        assert_eq!(spawn_mode(home, cwd, "u-1"), SpawnMode::Create);
        // Drop the jsonl where Claude would write it → next spawn resumes.
        let dir = home.join(".claude/projects/-Users-x-code-clave");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("u-1.jsonl"), b"{}").unwrap();
        assert_eq!(spawn_mode(home, cwd, "u-1"), SpawnMode::Resume);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave spawn::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `spawn.rs`**

```rust
//! `clave spawn <uuid> --name <label> --cwd <cwd>` (§6.1) — the command every
//! agent pane runs. Idempotent BY CONSTRUCTION: the same command re-run on
//! Zellij resurrection resumes the same conversation instead of erroring or
//! forking, because the create/resume branch is decided by whether Claude's
//! own transcript jsonl exists (invariant #5).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clave_types::Register;

use crate::munge::munge_cwd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnMode {
    /// No jsonl on disk → `claude --session-id <uuid> --name <label>`.
    /// S0: a fresh uuid CREATES the session and writes the jsonl.
    Create,
    /// jsonl exists → `claude --resume <uuid>`. `--resume` errors when no
    /// jsonl exists, which is why existence drives the branch (S0).
    Resume,
}

/// Where Claude Code stores this session's transcript. `physical_cwd` MUST
/// already be canonicalized (S0b: Claude munges getcwd(), which resolves
/// symlinks) — pass the output of `std::fs::canonicalize`, never raw user
/// input, or the join key misses and create collides ("already in use").
pub fn jsonl_path(home: &Path, physical_cwd: &str, uuid: &str) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(munge_cwd(physical_cwd))
        .join(format!("{uuid}.jsonl"))
}

pub fn spawn_mode(home: &Path, physical_cwd: &str, uuid: &str) -> SpawnMode {
    if jsonl_path(home, physical_cwd, uuid).exists() {
        SpawnMode::Resume
    } else {
        SpawnMode::Create
    }
}

/// Register this pane with the bar: uuid → $ZELLIJ_PANE_ID (spike S2 verified
/// the env var IS exported to layout `command` panes). Best-effort: a failed
/// registration only costs nav-to-this-agent until the next register; it must
/// NEVER stop the exec into Claude. Fire-and-forget spawn — `zellij pipe` can
/// dawdle (S1) and the exec below replaces this process anyway.
pub fn register_pane(uuid: &str) {
    let Ok(pane_id) = std::env::var("ZELLIJ_PANE_ID") else {
        eprintln!("clave spawn: ZELLIJ_PANE_ID unset; skipping bar registration");
        return;
    };
    let Ok(pane_id) = pane_id.parse::<u32>() else {
        eprintln!("clave spawn: unparseable ZELLIJ_PANE_ID {pane_id:?}");
        return;
    };
    let reg = Register { uuid: uuid.to_string(), pane_id };
    let payload = match serde_json::to_string(&reg) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("clave spawn: register serialize failed: {e}");
            return;
        }
    };
    let _ = Command::new("zellij")
        .args(["pipe", "--name", "clave-register", "--", &payload])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
```

`main.rs` `Spawn` arm (uses `std::os::unix::process::CommandExt::exec` — the pane process *becomes* Claude, §6.1):

```rust
        Command::Spawn { uuid, name, cwd } => {
            // S0b: canonicalize BEFORE munging — Claude keys the transcript
            // dir off the PHYSICAL getcwd() path.
            let physical = std::fs::canonicalize(&cwd)
                .with_context(|| format!("canonicalizing --cwd {cwd}"))?;
            let physical_str = physical.to_str().context("non-UTF8 cwd")?.to_string();
            let home = dirs::home_dir().context("no home dir")?;
            let mode = spawn::spawn_mode(&home, &physical_str, &uuid);
            // Register uuid→pane BEFORE exec (this process is about to be
            // replaced; best-effort — see register_pane).
            spawn::register_pane(&uuid);
            std::env::set_current_dir(&physical).context("entering --cwd")?;
            use std::os::unix::process::CommandExt;
            let err = match mode {
                // --name only on create: the bar label is clave-owned (§6.1).
                spawn::SpawnMode::Create => std::process::Command::new("claude")
                    .args(["--session-id", &uuid, "--name", &name])
                    .exec(),
                spawn::SpawnMode::Resume => std::process::Command::new("claude")
                    .args(["--resume", &uuid])
                    .exec(),
            };
            // exec only returns on failure — surface it in the pane.
            Err(anyhow::anyhow!("exec claude failed: {err}"))
        }
```

- [ ] **Step 4: Run tests + a dry host check**

Run: `cargo test -p clave && cargo build -p clave`
Expected: PASS / clean build. (The exec path is validated live in Task 9 — do NOT run `clave spawn` headless; it would launch a real Claude session.)

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/spawn.rs crates/clave/src/lib.rs crates/clave/src/main.rs
git commit -m "feat(clave): idempotent spawn — canonicalize+munge existence check, register pipe, exec claude"
```

---

## Task 5: `clave hook` (status state machine + label derivation + push)

Spec: §6.5 verbatim (the zero-risk constraints are Global), §6.4 (label). The state machine is **latest-wins**, not priority-max — a later event downgrades an earlier one (else `needs_you` sticks red after you answer).

**Files:**
- Create: `crates/clave/src/hook.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod hook;`), `crates/clave/src/main.rs` (replace the `Hook` `todo!`)

**Interfaces:**
- Consumes: `store::*`, `spawn::jsonl_path`, `clave_types::Status`.
- Produces:
  - `pub struct HookPayload { pub session_id: Option<String>, pub prompt: Option<String>, pub message: Option<String> }` (serde, all `#[serde(default)]` — hook JSON varies by event; unknown fields ignored)
  - `pub fn status_for_event(event: &str, message: Option<&str>) -> Option<Status>`
  - `pub fn first_words(text: &str) -> String` — first 4 whitespace-words, hard-capped at 32 chars
  - `pub fn summary_from_tail(tail: &str) -> Option<String>` — last `"type":"summary"` line's `summary` field
  - `pub fn refresh_label(rec: &mut AgentRecord, event: &str, payload: &HookPayload, jsonl_tail: Option<&str>) -> bool` — returns whether the label changed
  - `pub fn read_tail(path: &Path, max_bytes: u64) -> Option<String>` — last ≤64 KiB of the jsonl (never the whole file — §6.4: it grows unbounded and the hook has a timeout budget)
  - `pub fn push_snapshot(snap: &AgentSnapshot)` — fire-and-forget `zellij pipe --name clave-status`
  - `pub fn run_hook(event: &str, stdin_json: &str) -> anyhow::Result<()>` — the whole flow; **caller exits 0 regardless**

- [ ] **Step 1: Write the failing tests** (`hook.rs`):

```rust
    #[test]
    fn state_machine_is_latest_wins() {
        // Spec §6.5 transition table, verbatim.
        assert_eq!(status_for_event("UserPromptSubmit", None), Some(Status::Working));
        assert_eq!(status_for_event("Stop", None), Some(Status::Done));
        assert_eq!(status_for_event("StopFailure", None), Some(Status::Failed));
        assert_eq!(status_for_event("SessionEnd", None), Some(Status::Idle));
        assert_eq!(status_for_event("PermissionRequest", None), Some(Status::NeedsYou));
        // Notification matches on MESSAGE TEXT (§4): permission / idle prompts.
        assert_eq!(
            status_for_event("Notification", Some("Claude needs your permission to use Bash")),
            Some(Status::NeedsYou)
        );
        assert_eq!(
            status_for_event("Notification", Some("Claude is waiting for your input")),
            Some(Status::NeedsYou)
        );
        // Other notifications don't touch status.
        assert_eq!(status_for_event("Notification", Some("compacting…")), None);
        // Unknown events are a no-op — the global hook must never guess.
        assert_eq!(status_for_event("PreToolUse", None), None);
    }

    #[test]
    fn first_words_clamps() {
        assert_eq!(first_words("fix the flaky auth test please"), "fix the flaky auth");
        assert_eq!(first_words("short"), "short");
        assert!(first_words("averyveryverylongsingletokenthatkeepsgoing").len() <= 32);
    }

    #[test]
    fn summary_from_tail_takes_last_summary_line() {
        let tail = concat!(
            "{\"type\":\"user\",\"message\":\"hi\"}\n",
            "{\"type\":\"summary\",\"summary\":\"Old title\"}\n",
            "{\"type\":\"assistant\"}\n",
            "{\"type\":\"summary\",\"summary\":\"Fix auth flow\"}\n",
        );
        assert_eq!(summary_from_tail(tail).as_deref(), Some("Fix auth flow"));
        assert_eq!(summary_from_tail("{\"type\":\"user\"}\n"), None);
    }

    #[test]
    fn refresh_label_upgrades_first_prompt_then_summary_then_stops() {
        let mut r = rec("u1"); // label "x · main", label_source FirstPrompt
        // 1) First prompt arrives IN the UserPromptSubmit payload — no jsonl
        //    read needed for this step (§6.4 fast path).
        let p = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("fix the flaky auth test".into()),
            message: None,
        };
        assert!(refresh_label(&mut r, "UserPromptSubmit", &p, None));
        assert_eq!(r.label, "x · main · fix the flaky auth");
        assert_eq!(r.label_source, LabelSource::FirstPrompt);
        // 2) A summary in the jsonl tail wins and flips the source.
        let tail = "{\"type\":\"summary\",\"summary\":\"Fix auth flow\"}\n";
        assert!(refresh_label(&mut r, "Stop", &p, Some(tail)));
        assert_eq!(r.label, "x · main · Fix auth flow");
        assert_eq!(r.label_source, LabelSource::Summary);
        // 3) Once Summary, we STOP re-deriving (§6.4) — even with new input.
        assert!(!refresh_label(&mut r, "Stop", &p, Some(tail)));
    }
```

(Use the same `rec()` test helper shape as Task 2, with `label: "x · main".into()`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave hook::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `hook.rs`**

```rust
//! `clave hook <event>` (§6.5) — the ONLY writer of agent status. Runs as a
//! global Claude Code hook, so its prime directive is DO NO HARM: untracked
//! sessions get a lock-free read and exit 0; every internal error also exits
//! 0; it never prints a hook decision to stdout (a PreToolUse-style hook's
//! stdout can approve/deny tool use — ours stays silent and pass-through).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;
use clave_types::{AgentSnapshot, Status};
use serde::Deserialize;

use crate::spawn::jsonl_path;
use crate::store::{
    now_unix, read_store, snapshot_from, store_paths, with_store_mut, AgentRecord, LabelSource,
};

/// The fields we care about across ALL hook events (each event's JSON is a
/// superset; serde ignores the rest). Everything optional — a malformed or
/// novel payload must degrade to a no-op, never an error.
#[derive(Debug, Default, Deserialize)]
pub struct HookPayload {
    #[serde(default)]
    pub session_id: Option<String>,
    /// UserPromptSubmit carries the prompt text — the §6.4 first-label fast
    /// path (no jsonl read needed for the initial label).
    #[serde(default)]
    pub prompt: Option<String>,
    /// Notification carries a human message; §6.5 matches on its text.
    #[serde(default)]
    pub message: Option<String>,
}

/// §6.5's transition table, verbatim. Latest-wins: the CURRENT status is
/// irrelevant; each event maps directly to the new one (a later lower-
/// "priority" event must be able to downgrade needs_you after you answer).
pub fn status_for_event(event: &str, message: Option<&str>) -> Option<Status> {
    match event {
        "UserPromptSubmit" => Some(Status::Working),
        "Stop" => Some(Status::Done),
        "StopFailure" => Some(Status::Failed),
        "SessionEnd" => Some(Status::Idle),
        "PermissionRequest" => Some(Status::NeedsYou),
        "Notification" => {
            // §4: match the notification MESSAGE TEXT for the two needs-you
            // cases. Substrings chosen from the live payloads observed in S1;
            // Task 9 checkpoint C2 re-verifies them against the current CLI.
            let m = message.unwrap_or("");
            if m.contains("permission") || m.contains("waiting for your input") {
                Some(Status::NeedsYou)
            } else {
                None
            }
        }
        // Unknown/other events: strictly no-op. The hook is registered for a
        // fixed set, but a defensive default keeps novel events harmless.
        _ => None,
    }
}

/// First 4 words, hard cap 32 chars — enough to recognise, short enough for
/// a ~24-col bar row after the `dir · branch ·` prefix is truncated (§6.4:
/// final clamping is the RENDERER's job; this just bounds the stored label).
pub fn first_words(text: &str) -> String {
    let mut s = text
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    if s.len() > 32 {
        // Truncate on a char boundary (labels can contain multibyte chars).
        s = s.chars().take(32).collect();
    }
    s
}

/// Scan a jsonl TAIL for the LAST `{"type":"summary","summary":…}` line.
/// Line-wise serde parse — no regex, no full-file model of Claude's schema.
pub fn summary_from_tail(tail: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        summary: Option<String>,
    }
    tail.lines()
        .rev()
        .find_map(|l| match serde_json::from_str::<Line>(l) {
            Ok(line) if line.kind == "summary" => line.summary,
            _ => None,
        })
}

/// Last ≤`max_bytes` of `path` (lossy UTF-8; we only pattern-match). The
/// jsonl grows unbounded — a full read every turn risks the hook timeout
/// budget, so we read the tail only (§6.4).
pub fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// §6.4 label refresh. Returns whether the label changed. The `dir · branch`
/// prefix is rebuilt from the record so the rule stays in one place:
/// label = `<last-path-component of cwd> · <branch> [· <words>]`.
pub fn refresh_label(
    rec: &mut AgentRecord,
    event: &str,
    payload: &HookPayload,
    jsonl_tail: Option<&str>,
) -> bool {
    // Once a summary named the session, it stays (§6.4: stop re-scanning).
    if rec.label_source == LabelSource::Summary {
        return false;
    }
    let dir = rec
        .cwd
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&rec.cwd);
    let prefix = format!("{dir} · {}", rec.branch);
    // Prefer a summary from the tail (Stop is when summaries appear)…
    if let Some(summary) = jsonl_tail.and_then(summary_from_tail) {
        let label = format!("{prefix} · {}", first_words(&summary));
        rec.label = label;
        rec.label_source = LabelSource::Summary;
        return true;
    }
    // …else, on the first prompt, use the prompt text from the payload.
    // "First" = the label is still the bare `dir · branch` from `clave add`.
    if event == "UserPromptSubmit" && rec.label == prefix {
        if let Some(p) = payload.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
            rec.label = format!("{prefix} · {}", first_words(p));
            return true;
        }
    }
    false
}

/// Fire-and-forget snapshot push (§5). Spawn WITHOUT waiting: `zellij pipe`
/// can dawdle (S1) and a global hook must never block Claude on it. The
/// child inherits ZELLIJ env vars from the pane, targeting the right session;
/// stdio is nulled so nothing leaks into the hook protocol on stdout.
pub fn push_snapshot(snap: &AgentSnapshot) {
    let Ok(payload) = serde_json::to_string(snap) else {
        return;
    };
    let _ = Command::new("zellij")
        .args(["pipe", "--name", "clave-status", "--", &payload])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// The whole hook flow. Errors bubble up ONLY so main can log them to
/// stderr — main exits 0 no matter what (Global Constraint).
pub fn run_hook(event: &str, stdin_json: &str) -> Result<()> {
    let payload: HookPayload = serde_json::from_str(stdin_json).unwrap_or_default();
    let Some(uuid) = payload.session_id.clone() else {
        return Ok(()); // no session_id → nothing to key on
    };
    let paths = store_paths()?;
    // FAST PATH (§6.5): lock-free read; untracked session → exit immediately.
    // clave must never serialize unrelated sessions' hooks behind its lock.
    if !read_store(&paths)?.agents.contains_key(&uuid) {
        return Ok(());
    }
    let home = dirs::home_dir().unwrap_or_default();
    let snap = with_store_mut(&paths, |s| {
        let Some(rec) = s.agents.get_mut(&uuid) else {
            return None; // raced a prune — fine
        };
        let mut changed = false;
        if let Some(next) = status_for_event(event, payload.message.as_deref()) {
            changed |= rec.status != next;
            rec.status = next;
        }
        if event == "UserPromptSubmit" {
            rec.last_interacted = now_unix(); // recency (§6.6 order)
            changed = true;
        }
        // Label refresh only re-reads the jsonl while it's still cheap to
        // matter (§6.4): source==FirstPrompt and a label-bearing event.
        let tail = if rec.label_source == LabelSource::FirstPrompt
            && matches!(event, "Stop" | "UserPromptSubmit")
        {
            read_tail(&jsonl_path(&home, &rec.cwd, &uuid), 64 * 1024)
        } else {
            None
        };
        changed |= refresh_label(rec, event, &payload, tail.as_deref());
        if changed {
            s.seq += 1; // monotonic pipe contract (§5)
            Some(snapshot_from(s))
        } else {
            None
        }
    })?;
    if let Some(snap) = snap {
        push_snapshot(&snap);
    }
    Ok(())
}
```

`main.rs` `Hook` arm — note the exit-0-always shape:

```rust
        Command::Hook { event } => {
            // Zero-risk global citizen (§6.5): read stdin, do our best, and
            // exit 0 unconditionally — a clave bug must never become a
            // machine-wide Claude failure. Errors go to stderr only.
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            if let Err(e) = hook::run_hook(&event, &input) {
                eprintln!("clave hook: {e:#}");
            }
            Ok(()) // ALWAYS success
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p clave`
Expected: all PASS (munge + store + lsview + spawn + hook).

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/hook.rs crates/clave/src/lib.rs crates/clave/src/main.rs
git commit -m "feat(clave): hook subsystem — latest-wins status machine, label derivation, snapshot push"
```

---

## Task 6: `clave-bar` — the vertical dynamic tab bar

Spec: §6.6 verbatim (read it first), §2 invariant #11 (revised), §4 "Tab/pane truth for plugins". This replaces the spike-scope plugin wholesale. All display logic lives in a **new, zellij-tile-free `model.rs`** (data in → `Effect` out) so it unit-tests on the host; `main.rs` becomes a thin adapter. Carry forward unchanged: the `handle_pipe` split + unconditional `unblock_cli_pipe_input` (dd38ace), seq gating, `focus_pane_with_id` nav.

**Files:**
- Create: `crates/clave-bar/src/model.rs`
- Rewrite: `crates/clave-bar/src/main.rs`
- Modify: `crates/clave-bar/Cargo.toml` (why-comments — deferred minor)

**Interfaces:**
- Consumes: `clave_types::{Agent, AgentSnapshot, Status, Register}`, `Status::glyph()` (Task 1), `clave snapshot` stdout (Task 3), `clave focus <uuid>` (Task 3).
- Produces (in `model.rs`):
  - `pub struct TabMeta { pub tab_id: usize, pub position: usize, pub name: String, pub active: bool }`
  - `pub struct PaneMeta { pub tab_position: usize, pub pane_id: u32, pub is_plugin: bool, pub is_focused: bool }`
  - `pub enum Effect { RenameTab { tab_id: usize, name: String }, FocusPane { pane_id: u32 }, MarkRead { uuid: String } }`
  - `pub struct Row { pub tab_id: usize, pub name: String, pub active: bool, pub glyph: Option<(char, u8)> }`
  - `pub struct BarModel { pub hidden: bool, /* private state */ }` with methods:
    `apply_snapshot(&mut self, AgentSnapshot) -> Vec<Effect>` ·
    `register(&mut self, uuid: String, pane_id: u32)` ·
    `apply_tabs(&mut self, Vec<TabMeta>) -> Vec<Effect>` ·
    `apply_panes(&mut self, Vec<PaneMeta>)` ·
    `rows(&self) -> Vec<Row>` ·
    `click(&self, line: usize) -> Option<Effect>` ·
    `nav(&self, payload: &str) -> Option<Effect>` ·
    `toggle(&mut self) -> bool`

**Semantics locked by the spec (implement exactly):**
- Row **order**: recency (logical clock) desc, tiebreak tab `position` asc. The clock bumps when (a) a tab *becomes* active (`apply_tabs` transition) or (b) an agent's `last_interacted` advances between snapshots. Never-touched tabs have clock 0 → sink to bottom in tab order.
- **Rename guard**: rename only when the snapshot label differs from the label *we last wrote* for that uuid (`renamed: BTreeMap<uuid,label>`) — NOT when it differs from the tab's current name. Manual renames stick until the label genuinely changes.
- **Unread clear**: when a tab becomes active and its agent's status is `Done` (and not already locally cleared), record a local `read` override (render as `Idle`) and emit `MarkRead` — every instance repaints locally; only the ACTIVE-tab instance executes `MarkRead`/`RenameTab` side effects (main.rs gates; avoids N duplicate `clave focus` runs). Clear the local override whenever a snapshot shows the agent in any non-`Done` status.
- **uuid→row join**: `uuid_to_pane` (register) + `PaneMeta.tab_position` (PaneUpdate) + `TabMeta.position` (TabUpdate).
- **click/nav target**: a tab's focused non-plugin pane; if none `is_focused`, the first non-plugin pane; a tab with only plugin panes is skipped (no-op).
- **nav payloads** (`clave-nav`): `{"dir":"next"}` / `{"dir":"prev"}` walk display order relative to the active row (wrapping); `{"row":N}` = 1-based Nth displayed row; `{"uuid":"…"}` = direct (S2 form, kept).

- [ ] **Step 1: Write the failing model tests** (`model.rs` `#[cfg(test)]`; build these tiny helpers first — `agent(uuid, status, last_interacted) -> Agent` (label = uuid, other fields ""), `agent_labelled(uuid, label) -> Agent` (status Idle, last_interacted 0), `snap(seq, agents) -> AgentSnapshot`, `tab(id, pos, name, active) -> TabMeta`, `pane(tab_pos, id, plugin, focused) -> PaneMeta`):

```rust
    #[test]
    fn rows_are_recency_ordered_with_tab_order_tail() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", false), tab(12, 2, "c", false)]);
        // Focus b, then c: recency c > b > (a untouched, clock 0).
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true), tab(12, 2, "c", false)]);
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", false), tab(12, 2, "c", true)]);
        let names: Vec<String> = m.rows().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["c", "b", "a"]);
        // Emergent §6.6 property: active tab is row 1 (Alt+2 ≈ alt-tab).
        assert!(m.rows()[0].active);
    }

    #[test]
    fn agent_rows_get_glyphs_plain_rows_do_not() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "agent-tab", false), tab(11, 1, "plain", false)]);
        m.apply_panes(vec![pane(0, 5, false, true), pane(1, 6, false, true)]);
        m.register("u1".into(), 5); // pane 5 lives in tab position 0
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Working, 100)]));
        let rows = m.rows();
        let a = rows.iter().find(|r| r.name == "agent-tab").unwrap();
        let p = rows.iter().find(|r| r.name == "plain").unwrap();
        assert_eq!(a.glyph, Some(('●', 33))); // Working = amber
        assert_eq!(p.glyph, None);            // plain terminal: name only
    }

    #[test]
    fn snapshot_seq_gate_discards_stale() {
        let mut m = BarModel::default();
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, 100)]));
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Failed, 999)])); // stale
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        m.apply_panes(vec![pane(0, 5, false, true)]);
        m.register("u1".into(), 5);
        assert_eq!(m.rows()[0].glyph, Some(('●', 33))); // still Working
    }

    #[test]
    fn rename_only_when_label_changes_not_when_tab_name_differs() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "old-name", false)]);
        m.apply_panes(vec![pane(0, 5, false, true)]);
        m.register("u1".into(), 5);
        let fx = m.apply_snapshot(snap(1, vec![agent_labelled("u1", "x · main · fix auth")]));
        assert!(fx.contains(&Effect::RenameTab { tab_id: 10, name: "x · main · fix auth".into() }));
        // Same label again — even though the TAB name is still "old-name"
        // (e.g. the user manually renamed it), we do NOT re-rename.
        let fx = m.apply_snapshot(snap(2, vec![agent_labelled("u1", "x · main · fix auth")]));
        assert!(fx.iter().all(|e| !matches!(e, Effect::RenameTab { .. })));
        // A genuinely NEW label renames again.
        let fx = m.apply_snapshot(snap(3, vec![agent_labelled("u1", "x · main · Fix auth flow")]));
        assert!(fx.contains(&Effect::RenameTab { tab_id: 10, name: "x · main · Fix auth flow".into() }));
    }

    #[test]
    fn focus_on_done_agent_marks_read_once_and_renders_idle() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        m.apply_panes(vec![pane(0, 5, false, true)]);
        m.register("u1".into(), 5);
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Done, 100)]));
        assert_eq!(m.rows()[0].glyph, Some(('●', 32))); // green, unread
        // Tab gains focus → local clear + MarkRead effect, exactly once.
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.contains(&Effect::MarkRead { uuid: "u1".into() }));
        assert_eq!(m.rows()[0].glyph, Some(('●', 90))); // rendered dim NOW
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.iter().all(|e| !matches!(e, Effect::MarkRead { .. })));
        // A later snapshot showing Working clears the local override.
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, 200)]));
        assert_eq!(m.rows()[0].glyph, Some(('●', 33)));
    }

    #[test]
    fn click_and_nav_resolve_display_order_to_panes() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        // Tab 0: plugin pane 1 (the bar) + terminal 5 (focused);
        // tab 1: terminal 6 (not marked focused → first-non-plugin fallback).
        m.apply_panes(vec![
            pane(0, 1, true, false),
            pane(0, 5, false, true),
            pane(1, 6, false, false),
        ]);
        // Display order: a (active, most recent) then b.
        assert_eq!(m.click(0), Some(Effect::FocusPane { pane_id: 5 }));
        assert_eq!(m.click(1), Some(Effect::FocusPane { pane_id: 6 }));
        assert_eq!(m.click(9), None); // below the list
        // Active row is 0 ("a") → next wraps forward to "b", prev wraps back.
        assert_eq!(m.nav("{\"dir\":\"next\"}"), Some(Effect::FocusPane { pane_id: 6 }));
        assert_eq!(m.nav("{\"dir\":\"prev\"}"), Some(Effect::FocusPane { pane_id: 6 }));
        assert_eq!(m.nav("{\"row\":1}"), Some(Effect::FocusPane { pane_id: 5 }));
        assert_eq!(m.nav("{\"row\":9}"), None);
        // S2's direct-uuid form still works.
        m.register("u1".into(), 6);
        assert_eq!(m.nav("{\"uuid\":\"u1\"}"), Some(Effect::FocusPane { pane_id: 6 }));
        assert_eq!(m.nav("not json"), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave-bar`
Expected: FAIL to compile (`model` doesn't exist). NOTE: this compiles zellij-tile for the HOST — expected to work (it's docs.rs-buildable). If host compilation of zellij-tile errors, mirror the needed test into a `#[cfg(test)]`-only module and flag it in the task report — do NOT abandon the pure-model split.

- [ ] **Step 3: Implement `model.rs`**

```rust
//! Pure display/model logic for clave-bar — deliberately NO zellij-tile
//! imports so it compiles and unit-tests on the host. main.rs adapts zellij
//! events into these plain types and executes the returned `Effect`s.
//!
//! The three separated concerns of spec §6.6:
//!   row SET        = zellij's tabs (apply_tabs)
//!   row ORDER      = interaction recency (logical clock, this module)
//!   row DECORATION = clave's pushed snapshots (apply_snapshot)

use std::collections::{BTreeMap, BTreeSet};

use clave_types::{Agent, AgentSnapshot, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMeta {
    /// Zellij's STABLE tab id (survives reorders) — the recency/rename key.
    pub tab_id: usize,
    /// Current 0-based position — the PaneManifest join key (it's keyed by
    /// position, not id) and the bottom-of-list tiebreak.
    pub position: usize,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMeta {
    pub tab_position: usize,
    pub pane_id: u32,
    pub is_plugin: bool,
    pub is_focused: bool,
}

/// Side effects for main.rs to execute — kept as data so tests assert them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// rename_tab_with_id(tab_id, name) — write clave's label on the real tab.
    RenameTab { tab_id: usize, name: String },
    /// focus_pane_with_id(Terminal(pane_id)) — S2-proven nav.
    FocusPane { pane_id: u32 },
    /// run_command(["clave","focus",uuid]) — persist the unread clear.
    MarkRead { uuid: String },
}

/// One rendered row, already in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub tab_id: usize,
    pub name: String,
    pub active: bool,
    /// (glyph, ANSI colour) for agent rows; None for plain terminal tabs.
    pub glyph: Option<(char, u8)>,
}

#[derive(Default)]
pub struct BarModel {
    /// §5 pipe contract: apply only strictly-newer seq.
    seq: u64,
    agents: Vec<Agent>,
    /// uuid → terminal pane id, from clave-register (S2).
    uuid_to_pane: BTreeMap<String, u32>,
    tabs: Vec<TabMeta>,
    panes: Vec<PaneMeta>,
    /// tab_id → logical time of last interaction. NOT wall time: agent
    /// last_interacted (unix s) and focus events (no clock in wasm we trust)
    /// only ever BUMP this counter, so the scales never mix.
    recency: BTreeMap<usize, u64>,
    clock: u64,
    /// Last label WE wrote per uuid — the rename loop-guard (§6.4). Renames
    /// fire on label CHANGE only, so user manual renames stick in between.
    renamed: BTreeMap<String, String>,
    /// uuid → last_interacted seen in the previous snapshot (bump detection).
    seen_interacted: BTreeMap<String, u64>,
    /// Local unread-override: Done agents we've already cleared on focus.
    /// Render-side only; `clave focus` persists the real transition.
    read_locally: BTreeSet<String>,
    /// Bar visibility (Alt+c). main.rs maps this to hide_self/show_self.
    pub hidden: bool,
}

impl BarModel {
    fn bump(&mut self, tab_id: usize) {
        self.clock += 1;
        self.recency.insert(tab_id, self.clock);
    }

    /// Which tab (by current position) holds this pane?
    fn tab_position_of_pane(&self, pane_id: u32) -> Option<usize> {
        self.panes
            .iter()
            .find(|p| p.pane_id == pane_id && !p.is_plugin)
            .map(|p| p.tab_position)
    }

    fn tab_at_position(&self, position: usize) -> Option<&TabMeta> {
        self.tabs.iter().find(|t| t.position == position)
    }

    /// The agent living in the tab at `position`, if any (uuid→pane→tab).
    fn agent_at_position(&self, position: usize) -> Option<&Agent> {
        self.agents.iter().find(|a| {
            self.uuid_to_pane
                .get(&a.uuid)
                .and_then(|p| self.tab_position_of_pane(*p))
                == Some(position)
        })
    }

    /// Click/nav target for a tab: its focused non-plugin pane, else the
    /// first non-plugin pane (a tab remembers its internal focus; a tab with
    /// only plugin panes has no sensible target → None).
    fn pane_for_position(&self, position: usize) -> Option<u32> {
        let in_tab = || self.panes.iter().filter(move |p| p.tab_position == position && !p.is_plugin);
        in_tab()
            .find(|p| p.is_focused)
            .or_else(|| in_tab().next())
            .map(|p| p.pane_id)
    }

    pub fn register(&mut self, uuid: String, pane_id: u32) {
        self.uuid_to_pane.insert(uuid, pane_id);
    }

    /// Apply a full-replace snapshot (§5). Returns rename effects (label
    /// changes → real-tab renames) — main.rs gates their EXECUTION to the
    /// active-tab instance, but the guard bookkeeping must run everywhere so
    /// all instances agree on what has been renamed.
    pub fn apply_snapshot(&mut self, snap: AgentSnapshot) -> Vec<Effect> {
        if snap.seq <= self.seq {
            return Vec::new(); // stale/out-of-order: discard (S1)
        }
        self.seq = snap.seq;
        self.agents = snap.agents;
        let mut effects = Vec::new();
        // Borrow-friendly pass: collect (uuid, last_interacted, status, label,
        // tab_id) first, then mutate.
        let views: Vec<(String, u64, Status, String, Option<usize>)> = self
            .agents
            .iter()
            .map(|a| {
                let tab_id = self
                    .uuid_to_pane
                    .get(&a.uuid)
                    .and_then(|p| self.tab_position_of_pane(*p))
                    .and_then(|pos| self.tab_at_position(pos))
                    .map(|t| t.tab_id);
                (a.uuid.clone(), a.last_interacted, a.status, a.label.clone(), tab_id)
            })
            .collect();
        for (uuid, interacted, status, label, tab_id) in views {
            // (b) recency bump when the agent's last_interacted advances.
            let prev = self.seen_interacted.insert(uuid.clone(), interacted);
            if let Some(tab_id) = tab_id {
                if prev.is_some_and(|p| interacted > p) || (prev.is_none() && interacted > 0) {
                    self.bump(tab_id);
                }
                // Rename on label CHANGE only (vs what WE last wrote).
                if self.renamed.get(&uuid) != Some(&label) {
                    self.renamed.insert(uuid.clone(), label.clone());
                    effects.push(Effect::RenameTab { tab_id, name: label });
                }
            }
            // Any authoritative non-Done status clears the local override.
            if status != Status::Done {
                self.read_locally.remove(&uuid);
            }
        }
        effects
    }

    /// Apply zellij's tab truth. Detects newly-active tabs for (a) recency
    /// and the §6.5 unread clear.
    pub fn apply_tabs(&mut self, tabs: Vec<TabMeta>) -> Vec<Effect> {
        let prev_active: Option<usize> = self.tabs.iter().find(|t| t.active).map(|t| t.tab_id);
        self.tabs = tabs;
        let mut effects = Vec::new();
        if let Some(now_active) = self.tabs.iter().find(|t| t.active) {
            let (tab_id, position) = (now_active.tab_id, now_active.position);
            if prev_active != Some(tab_id) {
                self.bump(tab_id);
                // Focused a Done agent → clear unread (render now, persist
                // via MarkRead). BTreeSet.insert returns false if present —
                // that's the exactly-once guard.
                if let Some(a) = self.agent_at_position(position) {
                    if a.status == Status::Done {
                        let uuid = a.uuid.clone();
                        if self.read_locally.insert(uuid.clone()) {
                            effects.push(Effect::MarkRead { uuid });
                        }
                    }
                }
            }
        }
        effects
    }

    pub fn apply_panes(&mut self, panes: Vec<PaneMeta>) {
        self.panes = panes;
    }

    /// Rows in display order: recency desc, then tab position asc (never-
    /// touched tabs all have recency 0 and sort by position — spec §6.6).
    pub fn rows(&self) -> Vec<Row> {
        let mut order: Vec<&TabMeta> = self.tabs.iter().collect();
        order.sort_by(|a, b| {
            let ra = self.recency.get(&a.tab_id).copied().unwrap_or(0);
            let rb = self.recency.get(&b.tab_id).copied().unwrap_or(0);
            rb.cmp(&ra).then(a.position.cmp(&b.position))
        });
        order
            .into_iter()
            .map(|t| {
                let glyph = self.agent_at_position(t.position).map(|a| {
                    // Local unread override: render Done as Idle once seen.
                    if a.status == Status::Done && self.read_locally.contains(&a.uuid) {
                        Status::Idle.glyph()
                    } else {
                        a.status.glyph()
                    }
                });
                Row { tab_id: t.tab_id, name: t.name.clone(), active: t.active, glyph }
            })
            .collect()
    }

    /// Mouse click on rendered line N (0-based) → focus that row's pane.
    pub fn click(&self, line: usize) -> Option<Effect> {
        let rows = self.rows();
        let row = rows.get(line)?;
        let position = self.tabs.iter().find(|t| t.tab_id == row.tab_id)?.position;
        self.pane_for_position(position).map(|pane_id| Effect::FocusPane { pane_id })
    }

    /// clave-nav payloads: {"dir":"next"|"prev"} | {"row":N} | {"uuid":"…"}.
    /// dir walks DISPLAY order relative to the active row, wrapping; row is
    /// 1-based (Alt+1..9). Malformed payloads → None (caller logs).
    pub fn nav(&self, payload: &str) -> Option<Effect> {
        let v: serde_json::Value = serde_json::from_str(payload).ok()?;
        if let Some(uuid) = v.get("uuid").and_then(|u| u.as_str()) {
            let pane_id = *self.uuid_to_pane.get(uuid)?;
            return Some(Effect::FocusPane { pane_id });
        }
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let target_line = if let Some(n) = v.get("row").and_then(|n| n.as_u64()) {
            let idx = (n as usize).checked_sub(1)?; // 1-based → 0-based
            if idx >= rows.len() {
                return None;
            }
            idx
        } else {
            let dir = v.get("dir")?.as_str()?;
            let active = rows.iter().position(|r| r.active).unwrap_or(0);
            match dir {
                "next" => (active + 1) % rows.len(),
                "prev" => (active + rows.len() - 1) % rows.len(),
                _ => return None,
            }
        };
        self.click(target_line)
    }

    /// Alt+c. Returns the NEW hidden state; main.rs calls hide_self/show_self.
    pub fn toggle(&mut self) -> bool {
        self.hidden = !self.hidden;
        self.hidden
    }
}
```

- [ ] **Step 4: Run model tests to verify they pass**

Run: `cargo test -p clave-bar`
Expected: all model tests PASS.

- [ ] **Step 5: Rewrite `crates/clave-bar/src/main.rs` (the zellij adapter)**

```rust
//! clave-bar — the vertical dynamic tab bar (spec §6.6). This file is a THIN
//! adapter: zellij events/pipes in → model.rs (pure, host-tested) → Effects
//! out. Keep logic out of here; if you're writing an `if` about ordering,
//! glyphs, or renames, it belongs in model.rs where it can be unit-tested.

mod model;

use std::collections::BTreeMap;

use model::{BarModel, Effect, PaneMeta, TabMeta};
use zellij_tile::prelude::*;

#[derive(Default)]
struct State {
    model: BarModel,
    /// Our own plugin pane id (get_plugin_ids) — used to decide whether THIS
    /// instance sits in the active tab. There is one bar instance per tab
    /// (§6.6); render-side state converges via broadcast, but WRITE effects
    /// (RenameTab, MarkRead) run on the active-tab instance only, so N
    /// instances don't fire N duplicate renames / `clave focus` runs.
    own_plugin_id: Option<u32>,
    /// Raw pane rows kept so we can locate our own plugin pane per tab.
    plugin_panes: Vec<(usize, u32)>, // (tab_position, plugin pane id)
    /// The last TabUpdate, verbatim — is_active_instance reads it (rows()
    /// is display-ordered, so it can't answer "is position P active").
    last_tabs: Vec<TabMeta>,
}

register_plugin!(State);

impl State {
    /// Is THIS instance the one living in the currently-active tab?
    fn is_active_instance(&self) -> bool {
        let Some(own) = self.own_plugin_id else {
            return false;
        };
        // Find our tab position via our plugin pane id, then check active.
        self.plugin_panes
            .iter()
            .find(|(_, id)| *id == own)
            .map(|(pos, _)| *pos)
            .and_then(|pos| self.model_tab_active_at(pos))
            .unwrap_or(false)
    }

    fn model_tab_active_at(&self, position: usize) -> Option<bool> {
        // rows() is display-ordered; go through the raw tabs instead.
        // (model exposes rows; keep a tiny helper here off the same data we
        // fed it — the last TabUpdate.)
        self.last_tabs
            .iter()
            .find(|t| t.position == position)
            .map(|t| t.active)
    }
}
```

**NOTE for the implementer:** `last_tabs` is set in the TabUpdate arm below. The `impl State` blocks here are split for narrative — write them as one coherent impl.

```rust
impl State {
    /// Execute model effects. Gate WRITES to the active-tab instance;
    /// FocusPane is intentionally ungated (every instance computes the same
    /// target — focusing twice is idempotent, and the keybind MessagePlugin
    /// may reach instances in any order).
    fn run_effects(&mut self, effects: Vec<Effect>) {
        let active = self.is_active_instance();
        for e in effects {
            match e {
                Effect::FocusPane { pane_id } => {
                    // S2-proven nav: focus the terminal pane; Zellij pulls
                    // its tab forward. go_to_tab is a known dead end.
                    focus_pane_with_id(PaneId::Terminal(pane_id), false, false);
                }
                Effect::RenameTab { tab_id, name } if active => {
                    rename_tab_with_id(tab_id as u64, name);
                }
                Effect::MarkRead { uuid } if active => {
                    // Persist the unread clear (§6.5). Fire-and-forget; the
                    // local repaint already happened in the model.
                    run_command(
                        &["clave", "focus", &uuid],
                        BTreeMap::new(),
                    );
                }
                _ => {} // non-active instance skips writes
            }
        }
    }

    /// One pipe message → model. Split out of pipe() so early returns here
    /// can't skip the unconditional unblock (dd38ace — see pipe()).
    fn handle_pipe(&mut self, message: PipeMessage) -> bool {
        let name = message.name.as_str();
        let Some(payload) = message.payload.as_deref() else {
            // Toggle carries no payload; everything else must.
            if name == "clave-toggle" {
                let hidden = self.model.toggle();
                if hidden { hide_self() } else { show_self(false) }
                return true;
            }
            eprintln!("clave-bar: dropped {name} pipe with empty payload");
            return false;
        };
        match name {
            "clave-status" => match serde_json::from_str(payload) {
                Ok(snap) => {
                    let fx = self.model.apply_snapshot(snap);
                    self.run_effects(fx);
                    true
                }
                Err(e) => {
                    eprintln!("clave-bar: bad clave-status payload: {e}");
                    false
                }
            },
            "clave-register" => match serde_json::from_str::<clave_types::Register>(payload) {
                Ok(reg) => {
                    self.model.register(reg.uuid, reg.pane_id);
                    true // a row may just have gained its glyph
                }
                Err(e) => {
                    eprintln!("clave-bar: bad clave-register payload: {e}");
                    false
                }
            },
            "clave-nav" => {
                match self.model.nav(payload) {
                    Some(fx) => self.run_effects(vec![fx]),
                    None => eprintln!("clave-bar: unresolvable clave-nav {payload:?}"),
                }
                false // focus change repaints via TabUpdate anyway
            }
            "clave-toggle" => {
                let hidden = self.model.toggle();
                if hidden { hide_self() } else { show_self(false) }
                true
            }
            other => {
                eprintln!("clave-bar: unknown pipe {other:?}");
                false
            }
        }
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        // §6.6 permission set — EXACTLY these four; grants are all-or-nothing
        // per plugin and the prompt is unanswerable in the bar pane, so
        // `clave setup` pre-seeds permissions.kdl with THIS set (both key
        // forms). Changing this list without changing the seed hangs every
        // pipe (this re-bit S2 — see the ledger).
        request_permission(&[
            PermissionType::ReadCliPipes,          // receive the clave-* pipes
            PermissionType::ChangeApplicationState, // focus_pane / rename_tab / hide_self
            PermissionType::ReadApplicationState,  // TabUpdate + PaneUpdate truth
            PermissionType::RunCommands,           // hydrate (clave snapshot) + clave focus
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::Mouse,
            EventType::RunCommandResult,
            EventType::PermissionRequestResult,
        ]);
        self.own_plugin_id = Some(get_plugin_ids().plugin_id);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(_) => {
                // Permissions just landed (pre-seeded → immediate): hydrate
                // from the store via `clave snapshot` (was spike S5). The
                // result arrives as RunCommandResult below; the seq gate
                // makes any race with live pushes benign (§5).
                run_command(&["clave", "snapshot"], BTreeMap::new());
                false
            }
            Event::RunCommandResult(exit, stdout, stderr, _ctx) => {
                // Only `clave snapshot` produces stdout we care about; the
                // `clave focus` fire-and-forgets also land here — ignore
                // anything that doesn't parse as a snapshot.
                if exit != Some(0) {
                    eprintln!("clave-bar: run_command failed: {}", String::from_utf8_lossy(&stderr));
                    return false;
                }
                match serde_json::from_slice(&stdout) {
                    Ok(snap) => {
                        let fx = self.model.apply_snapshot(snap);
                        self.run_effects(fx);
                        true
                    }
                    Err(_) => false, // not a snapshot (e.g. clave focus) — fine
                }
            }
            Event::TabUpdate(tabs) => {
                let metas: Vec<TabMeta> = tabs
                    .iter()
                    .map(|t| TabMeta {
                        tab_id: t.tab_id,
                        position: t.position,
                        name: t.name.clone(),
                        active: t.active,
                    })
                    .collect();
                self.last_tabs = metas.clone();
                let fx = self.model.apply_tabs(metas);
                self.run_effects(fx);
                true
            }
            Event::PaneUpdate(manifest) => {
                let mut metas = Vec::new();
                self.plugin_panes.clear();
                for (tab_position, panes) in &manifest.panes {
                    for p in panes {
                        if p.is_plugin {
                            self.plugin_panes.push((*tab_position, p.id));
                        }
                        metas.push(PaneMeta {
                            tab_position: *tab_position,
                            pane_id: p.id,
                            is_plugin: p.is_plugin,
                            is_focused: p.is_focused,
                        });
                    }
                }
                self.model.apply_panes(metas);
                true
            }
            Event::Mouse(Mouse::LeftClick(line, _col)) => {
                // §6.6: rows are mouse-clickable. line is the rendered row.
                if line >= 0 {
                    if let Some(fx) = self.model.click(line as usize) {
                        self.run_effects(vec![fx]);
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        // A CLI pipe blocks its caller until unblocked; capture the id BEFORE
        // the message moves. Keybind/plugin sources carry no pipe id.
        let cli_pipe_id = match &message.source {
            PipeSource::Cli(id) => Some(id.clone()),
            _ => None,
        };
        let repaint = self.handle_pipe(message);
        // UNCONDITIONAL unblock (dd38ace): even a malformed payload must not
        // leave `zellij pipe` hanging until Zellij's 1s server timeout.
        if let Some(id) = cli_pipe_id {
            unblock_cli_pipe_input(&id);
        }
        repaint
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        // One line per tab, display-ordered. Active row inverted (SGR 7);
        // agent rows get their state glyph; plain tabs a 2-space gutter so
        // names align. Truncate to the pane width (raw ANSI is S1-proven).
        for row in self.model.rows() {
            let gutter = match row.glyph {
                Some((glyph, colour)) => format!("\u{1b}[{colour}m{glyph}\u{1b}[0m "),
                None => "  ".to_string(),
            };
            // Clamp the NAME to what's left after the 2-cell gutter, with a
            // trailing … (char-boundary safe; labels can be multibyte).
            let budget = cols.saturating_sub(3); // gutter + margin
            let name: String = if row.name.chars().count() > budget {
                let mut n: String = row.name.chars().take(budget.saturating_sub(1)).collect();
                n.push('…');
                n
            } else {
                row.name.clone()
            };
            if row.active {
                println!("{gutter}\u{1b}[7m{name}\u{1b}[0m");
            } else {
                println!("{gutter}{name}");
            }
        }
    }
}

// NOTE: no `fn main()` — register_plugin! supplies the wasm entry point (a
// second one is E0428; confirmed in foundation Task 1).
```

**Implementer notes (bind them into the code, not just the report):**
- `State` needs `last_tabs: Vec<TabMeta>` (referenced by `is_active_instance`).
- `run_command`'s signature in zellij-tile 0.44 is `run_command(cmd: &[&str], context: BTreeMap<String, String>)` — verify against the vendored source (`~/.cargo/registry/src/*/zellij-tile-0.44.3/src/shim.rs`) and adapt if the context type differs; same for `Event::RunCommandResult`'s tuple shape and `Mouse::LeftClick(isize, usize)`.
- `get_plugin_ids()` may not be callable in `load` on some versions — if it panics/returns nothing there, call it lazily on first `update`.

- [ ] **Step 6: Build the wasm + run all tests**

Run: `cargo test -p clave-bar && cargo build -p clave-bar --target wasm32-wasip1 && cargo test`
Expected: model tests PASS, wasm compiles clean, host workspace tests all PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/clave-bar/src/model.rs crates/clave-bar/src/main.rs crates/clave-bar/Cargo.toml
git commit -m "feat(clave-bar): vertical dynamic tab bar — TabUpdate rows, recency order, status decoration"
```

---

## Task 7: `clave add` (the Alt+a flow)

Spec: §6.3 verbatim. Interactive command (fzf needs a TTY — it runs in a floating pane via the Alt+a `Run` keybind), so TDD the pure parts and leave the interactive weave thin; Task 9 validates it live.

**Files:**
- Create: `crates/clave/src/add.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod add;`), `crates/clave/src/main.rs` (replace the `Add` `todo!`; add `--worktree` flag)

**Interfaces:**
- Consumes: `store::*`, `spawn::jsonl_path`, `munge::munge_cwd`, `hook::{first_words, push_snapshot}`, `setup::{wasm_path, data_dir}` (Task 8 — see note below).
- Produces:
  - `pub fn live_uuids(dump_layout: &str) -> Vec<String>` — uuids of running agents, parsed from `zellij action dump-layout` output (every agent pane's baked command is `clave spawn "<uuid>" …`)
  - `pub fn sanitize_label(s: &str) -> String` — strip `"` and control chars (the label is interpolated into KDL and fzf lines)
  - `pub fn tab_layout(wasm: &str, label: &str, uuid: &str, cwd: &str) -> String` — the one-shot temp layout KDL
  - `pub struct ResumeCandidate { pub uuid: String, pub label: String }`
  - `pub fn resume_candidates(store: &Store, repo_root: &str, jsonl_stems: &[(String, u64)], live: &[String]) -> Vec<ResumeCandidate>` — store rows for the repo + jsonl-discovered sessions (stem = uuid, u64 = mtime), minus live uuids, mtime-recency ordered
  - `pub fn run_add(worktree: bool) -> anyhow::Result<()>` — the interactive weave

**Ordering note:** `wasm_path()`/`data_dir()` are defined in Task 8's `setup.rs`. If executing strictly in order, define them HERE in a small `pub mod paths`-style section of `add.rs` and have Task 8 move them into `setup.rs` — or (simpler, recommended) implement Task 8's Step 3 `paths` block first; the two tasks are otherwise independent. Flag whichever you did in the task report.

- [ ] **Step 1: Write the failing tests** (`add.rs`):

```rust
    #[test]
    fn live_uuids_finds_baked_spawn_commands() {
        // Shape of `zellij action dump-layout` output for an agent pane —
        // the args are quoted tokens after "spawn".
        let dump = r#"
            tab name="x · main" {
                pane command="clave" {
                    args "spawn" "3f2a-uuid-1" "--name" "x · main" "--cwd" "/x"
                }
            }
            tab name="plain" { pane }
            tab name="y" {
                pane command="clave" {
                    args "spawn" "9b1c-uuid-2" "--name" "y · dev" "--cwd" "/y"
                }
            }
        "#;
        assert_eq!(live_uuids(dump), vec!["3f2a-uuid-1", "9b1c-uuid-2"]);
        assert!(live_uuids("layout { tab { pane } }").is_empty());
    }

    #[test]
    fn tab_layout_bakes_the_idempotent_spawn() {
        let kdl = tab_layout("/data/clave-bar.wasm", "x · main", "u-1", "/x");
        // The bar pane, the baked spawn (idempotent resurrection, §6.3/S4),
        // and the cwd all present:
        assert!(kdl.contains("location=\"file:/data/clave-bar.wasm\""));
        assert!(kdl.contains("\"spawn\" \"u-1\""));
        assert!(kdl.contains("cwd=\"/x\""));
        assert!(kdl.contains("name=\"x · main\""));
    }

    #[test]
    fn sanitize_label_strips_kdl_breakers() {
        assert_eq!(sanitize_label("fix \"auth\"\nflow"), "fix auth flow");
    }

    #[test]
    fn resume_candidates_exclude_live_and_sort_by_mtime() {
        let mut s = Store::default();
        let mut r = rec("u-live");
        r.repo_root = "/repo".into();
        s.agents.insert("u-live".into(), r);
        let mut r2 = rec("u-old");
        r2.repo_root = "/repo".into();
        r2.label = "repo · main · old thing".into();
        s.agents.insert("u-old".into(), r2);
        let jsonls = vec![("u-old".to_string(), 100u64), ("u-disk".to_string(), 200u64)];
        let live = vec!["u-live".to_string()];
        let c = resume_candidates(&s, "/repo", &jsonls, &live);
        // u-live excluded; u-disk (mtime 200) before u-old (100); the store
        // label wins over the bare uuid when we have one.
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].uuid, "u-disk");
        assert_eq!(c[1].uuid, "u-old");
        assert_eq!(c[1].label, "repo · main · old thing");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave add::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `add.rs`** — pure parts:

```rust
//! `clave add` (§6.3): pick a directory, then new-or-resume an agent in a new
//! tab. The INTERACTIVE weave (fzf) lives in run_add; everything decidable
//! is a pure function above it so it can be unit-tested.

use anyhow::{Context, Result};

use crate::store::Store;

/// Parse `zellij action dump-layout` for live agent uuids: every agent
/// pane's serialized command is `clave` with args `"spawn" "<uuid>" …` (§6.3
/// liveness check — the baked command doubles as the liveness marker).
pub fn live_uuids(dump_layout: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in dump_layout.lines() {
        let Some(rest) = line.trim().strip_prefix("args ") else {
            continue;
        };
        // Tokenize the quoted args: "spawn" "<uuid>" "--name" …
        let tokens: Vec<&str> = rest
            .split('"')
            .enumerate()
            .filter(|(i, _)| i % 2 == 1) // odd indices are inside quotes
            .map(|(_, t)| t)
            .collect();
        if let ["spawn", uuid, ..] = tokens.as_slice() {
            out.push((*uuid).to_string());
        }
    }
    out
}

/// Labels get interpolated into KDL string literals and fzf menu lines —
/// strip the two things that break them (quotes, control chars).
pub fn sanitize_label(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| *c != '"')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The one-shot temp layout (§6.3): Zellij KDL has no variable substitution,
/// so the uuid/label/cwd are baked in, the file is passed to
/// `zellij action new-tab --layout`, then deleted. Baking the command in is
/// also what makes the tab resume on resurrection (S4).
pub fn tab_layout(wasm: &str, label: &str, uuid: &str, cwd: &str) -> String {
    format!(
        r#"layout {{
    tab name="{label}" focus=true {{
        pane size=26 borderless=true {{
            plugin location="file:{wasm}"
        }}
        pane cwd="{cwd}" command="clave" {{
            args "spawn" "{uuid}" "--name" "{label}" "--cwd" "{cwd}"
        }}
    }}
}}
"#
    )
}

pub struct ResumeCandidate {
    pub uuid: String,
    pub label: String,
}

/// §6.3 resume picker input: this repo's store rows + jsonl-discovered
/// sessions (`jsonl_stems` = (uuid, mtime) from listing the munged project
/// dir), MINUS currently-live uuids. Recency (mtime) first; store labels
/// beat bare uuids.
pub fn resume_candidates(
    store: &Store,
    repo_root: &str,
    jsonl_stems: &[(String, u64)],
    live: &[String],
) -> Vec<ResumeCandidate> {
    let mut by_uuid: std::collections::BTreeMap<String, (u64, Option<String>)> = Default::default();
    for (uuid, mtime) in jsonl_stems {
        by_uuid.insert(uuid.clone(), (*mtime, None));
    }
    for r in store.agents.values().filter(|r| r.repo_root == repo_root) {
        let e = by_uuid.entry(r.uuid.clone()).or_insert((r.last_interacted, None));
        e.1 = Some(r.label.clone());
    }
    let mut list: Vec<(u64, ResumeCandidate)> = by_uuid
        .into_iter()
        .filter(|(uuid, _)| !live.contains(uuid))
        .map(|(uuid, (mtime, label))| {
            let label = label.unwrap_or_else(|| uuid.clone());
            (mtime, ResumeCandidate { uuid, label })
        })
        .collect();
    list.sort_by(|a, b| b.0.cmp(&a.0));
    list.into_iter().map(|(_, c)| c).collect()
}
```

And the interactive weave (same file; every subprocess is commented with *why*):

```rust
use std::io::Write as _;
use std::process::{Command, Stdio};

use crate::hook::push_snapshot;
use crate::setup::{data_dir, wasm_path};
use crate::store::{
    now_unix, snapshot_from, store_paths, with_store_mut, AgentRecord, LabelSource,
};

/// Run `fzf` over `lines`, return the picked line. fzf opens /dev/tty itself,
/// so this works inside the Alt+a floating pane; stdin carries the menu,
/// stdout the choice. None = user aborted (Esc) → caller exits quietly.
fn fzf_pick(lines: &[String], prompt: &str) -> Result<Option<String>> {
    let mut child = Command::new("fzf")
        .args(["--prompt", prompt, "--height", "100%"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("launching fzf (is it installed?)")?;
    child
        .stdin
        .take()
        .context("fzf stdin")?
        .write_all(lines.join("\n").as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Ok(None); // Esc/ctrl-c in fzf — a normal abort, not an error
    }
    let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!picked.is_empty()).then_some(picked))
}

fn cmd_stdout(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd).args(args).output()
        .with_context(|| format!("running {cmd}"))?;
    anyhow::ensure!(out.status.success(), "{cmd} {args:?} failed");
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn run_add(worktree: bool) -> Result<()> {
    // 1) Pick a directory: fzf over zoxide's ranked list, current dir first
    //    (§6.3 — fzf+zoxide are verified present on the target machine).
    let cwd = std::env::current_dir()?.to_string_lossy().into_owned();
    let mut dirs: Vec<String> = vec![cwd.clone()];
    dirs.extend(cmd_stdout("zoxide", &["query", "-l"])?.lines().map(String::from));
    dirs.dedup();
    let Some(dir) = fzf_pick(&dirs, "agent dir> ")? else { return Ok(()) };

    // 2) Canonicalize FIRST (S0b) — everything downstream keys off the
    //    physical path: repo_root, munged jsonl dir, the spawn command.
    let physical = std::fs::canonicalize(&dir).with_context(|| format!("canonicalizing {dir}"))?;
    let physical_str = physical.to_str().context("non-UTF8 dir")?.to_string();
    let repo_root = cmd_stdout("git", &["-C", &physical_str, "rev-parse", "--show-toplevel"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| physical_str.clone()); // non-repo dirs are fine
    let branch = cmd_stdout("git", &["-C", &physical_str, "rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "-".to_string());

    // 3) Liveness: an agent already running for this repo → just jump (§6.3).
    let dump = cmd_stdout("zellij", &["action", "dump-layout"]).unwrap_or_default();
    let live = live_uuids(&dump);
    let paths = store_paths()?;
    let store = crate::store::read_store(&paths)?;
    if let Some(running) = store
        .agents
        .values()
        .find(|r| r.repo_root == repo_root && live.contains(&r.uuid))
    {
        // clave-nav via the CLI pipe: works (S2), and `add` runs INSIDE the
        // session so the env targets the right zellij.
        let payload = format!("{{\"uuid\":\"{}\"}}", running.uuid);
        let _ = Command::new("zellij")
            .args(["pipe", "--name", "clave-nav", "--", &payload])
            .status();
        return Ok(());
    }

    // 4) new vs resume.
    let Some(choice) = fzf_pick(&["new".into(), "resume".into()], "agent> ")? else {
        return Ok(());
    };
    let (uuid, worktree_path) = if choice == "resume" {
        // clave owns the picker (§6.3 — claude --resume's own picker would
        // break resurrection). Candidates: store rows + jsonl scan.
        let proj_dir = dirs::home_dir()
            .context("home")?
            .join(".claude/projects")
            .join(crate::munge::munge_cwd(&physical_str));
        let mut stems: Vec<(String, u64)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&proj_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "jsonl") {
                    let stem = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                    let mtime = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    stems.push((stem, mtime));
                }
            }
        }
        let candidates = resume_candidates(&store, &repo_root, &stems, &live);
        anyhow::ensure!(!candidates.is_empty(), "no resumable sessions for {repo_root}");
        let lines: Vec<String> = candidates
            .iter()
            .map(|c| format!("{}\t{}", c.label, c.uuid)) // label shown, uuid carried
            .collect();
        let Some(picked) = fzf_pick(&lines, "resume> ")? else { return Ok(()) };
        let uuid = picked.rsplit('\t').next().context("picker line")?.to_string();
        (uuid, None)
    } else {
        let uuid = uuid::Uuid::new_v4().to_string();
        // Worktree opt-in (§6.3): clave shells out itself (never claude -w)
        // so it OWNS the path — needed for the munged jsonl existence check
        // and the store record.
        let wt = if worktree {
            let short = &uuid[..8];
            let path = format!("{repo_root}/.claude-worktrees/{short}");
            cmd_stdout("git", &["-C", &repo_root, "worktree", "add", "-b", &format!("clave/{short}"), &path])?;
            Some(path)
        } else {
            None
        };
        (uuid, wt)
    };

    // 5) The agent's cwd: the worktree if we made one, else the picked dir.
    //    Canonicalize AGAIN for the worktree (it's brand new — S0b applies).
    let agent_cwd = match &worktree_path {
        Some(w) => std::fs::canonicalize(w)?.to_str().context("wt path")?.to_string(),
        None => physical_str.clone(),
    };
    let dir_name = agent_cwd.rsplit('/').next().unwrap_or(&agent_cwd);
    let label = sanitize_label(&format!("{dir_name} · {branch}"));

    // 6) One-shot temp layout → new tab (§6.3). $TMPDIR, deleted after.
    let wasm = wasm_path()?.to_str().context("wasm path")?.to_string();
    let layout = tab_layout(&wasm, &label, &uuid, &agent_cwd);
    let tmp = std::env::temp_dir().join(format!("clave-{uuid}.kdl"));
    std::fs::write(&tmp, layout)?;
    let status = Command::new("zellij")
        .args(["action", "new-tab", "--layout", tmp.to_str().context("tmp path")?])
        .status()?;
    let _ = std::fs::remove_file(&tmp);
    anyhow::ensure!(status.success(), "zellij action new-tab failed");

    // 7) Record + push (§6.3): the row exists BEFORE the first hook event so
    //    the hook's untracked fast path doesn't drop this agent's events.
    let snap = with_store_mut(&paths, |s| {
        s.agents.insert(
            uuid.clone(),
            AgentRecord {
                uuid: uuid.clone(),
                cwd: agent_cwd.clone(),
                repo_root: repo_root.clone(),
                branch: branch.clone(),
                label: label.clone(),
                status: clave_types::Status::Idle,
                last_interacted: now_unix(),
                last_visited: 0,
                worktree: worktree_path.clone(),
                label_source: LabelSource::FirstPrompt,
            },
        );
        s.seq += 1;
        snapshot_from(s)
    })?;
    push_snapshot(&snap);
    Ok(())
}
```

`main.rs`: `Add` gains `#[arg(long)] worktree: bool`; the arm is `Command::Add { worktree } => add::run_add(worktree)`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p clave && cargo build -p clave`
Expected: PASS / clean. (Do NOT run `clave add` headless — fzf + zellij needed; Task 9 validates.)

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/add.rs crates/clave/src/lib.rs crates/clave/src/main.rs
git commit -m "feat(clave): add flow — zoxide/fzf picker, dump-layout liveness, one-shot tab layout"
```

---

## Task 8: Session model, config/layout generation, `clave setup`, launcher

Spec: §6.8, §7 (setup/permissions), §6.6 (keybinds). Everything machine-specific is GENERATED into `~/.local/share/clave/` — never committed.

**Files:**
- Create: `crates/clave/src/setup.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod setup;`), `crates/clave/src/main.rs` (add `Setup`; make the subcommand optional — bare `clave` launches/attaches the session), `justfile` (`install`, `clippy` recipes)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `pub fn data_dir() -> anyhow::Result<PathBuf>` — `~/.local/share/clave`
  - `pub fn wasm_path() -> anyhow::Result<PathBuf>` — `<data_dir>/clave-bar.wasm`
  - `pub fn config_kdl(wasm: &str) -> String` / `pub fn layout_kdl(wasm: &str) -> String`
  - `pub fn merge_hooks(settings: &mut serde_json::Value, clave_bin: &str) -> bool` — additive/idempotent; returns changed
  - `pub const BAR_PERMISSIONS: [&str; 4]`
  - `pub fn merge_permissions_kdl(existing: &str, wasm_abs: &str) -> String` — replaces/appends BOTH key forms, preserves other plugins' entries
  - `pub fn permissions_cache_path() -> anyhow::Result<PathBuf>` — macOS `~/Library/Caches/org.Zellij-Contributors.Zellij/permissions.kdl`, else `~/.cache/zellij/permissions.kdl`
  - `pub fn run_setup() -> anyhow::Result<()>` · `pub fn launch_session() -> anyhow::Result<()>`
  - CLI: `clave setup`; bare `clave` → `launch_session()`

- [ ] **Step 1: Write the failing tests** (`setup.rs`):

```rust
    #[test]
    fn hooks_merge_is_additive_and_idempotent() {
        // Existing user hook MUST survive (§6.5: never clobber).
        let mut v: serde_json::Value = serde_json::json!({
            "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "my-bell" } ] } ] }
        });
        assert!(merge_hooks(&mut v, "clave"));
        let stops = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stops.len(), 2); // user's + ours
        assert_eq!(stops[0]["hooks"][0]["command"], "my-bell");
        assert_eq!(stops[1]["hooks"][0]["command"], "clave hook Stop");
        // Every event we need is registered.
        for ev in ["UserPromptSubmit", "Notification", "SessionEnd"] {
            assert!(v["hooks"][ev].as_array().is_some(), "{ev} missing");
        }
        // Second run: no change, no duplicates.
        assert!(!merge_hooks(&mut v, "clave"));
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn permissions_merge_seeds_both_key_forms_and_preserves_others() {
        let existing = "\"file:/other/plugin.wasm\" {\n    ReadCliPipes\n}\n";
        let merged = merge_permissions_kdl(existing, "/data/clave-bar.wasm");
        // Other plugin untouched:
        assert!(merged.contains("/other/plugin.wasm"));
        // Both key forms present (S1/S2: key form matters):
        assert!(merged.contains("\"file:/data/clave-bar.wasm\""));
        assert!(merged.contains("\"/data/clave-bar.wasm\""));
        // The EXACT §6.6 set under each:
        for p in BAR_PERMISSIONS {
            assert!(merged.matches(p).count() >= 2, "{p} missing from a key form");
        }
        // Idempotent: re-merging replaces our blocks, not duplicates them.
        let again = merge_permissions_kdl(&merged, "/data/clave-bar.wasm");
        assert_eq!(again.matches("file:/data/clave-bar.wasm").count(), 1);
    }

    #[test]
    fn generated_kdl_carries_the_wasm_path_and_alt_keys() {
        let cfg = config_kdl("/data/clave-bar.wasm");
        for key in ["Alt a", "Alt c", "Alt w", "Alt j", "Alt k", "Alt 1", "Alt 9"] {
            assert!(cfg.contains(&format!("bind \"{key}\"")) || cfg.contains(&format!("\"{key}\"")), "{key} unbound");
        }
        assert!(cfg.contains("shared_among \"normal\" \"locked\"")); // invariant #6
        assert!(cfg.contains("clave-nav") && cfg.contains("clave-toggle"));
        let lay = layout_kdl("/data/clave-bar.wasm");
        assert!(lay.contains("default_tab_template"));
        assert!(lay.contains("file:/data/clave-bar.wasm"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave setup::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `setup.rs`** — paths, templates, merges, weave:

```rust
//! `clave setup` (§6.8/§7): make the machine ready — generated session
//! config + layout in ~/.local/share/clave/, Claude hooks merged into
//! ~/.claude/settings.json (ADDITIVELY — the file may be a dotfiles symlink;
//! we edit through it and never clobber existing hooks), and Zellij's
//! permission cache pre-seeded (grants are all-or-nothing and the in-bar
//! prompt is unanswerable — S1/S2).

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Where clave's generated artifacts live. NOT the repo: these files embed
/// machine-absolute paths (the wasm location) and the repo is public.
pub fn data_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir().context("home")?.join(".local/share/clave"))
}

pub fn wasm_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("clave-bar.wasm"))
}

/// §6.6's exact permission set. Keep THIS list, load()'s request_permission
/// call, and the seeded cache in lockstep — a partial cache match raises the
/// unanswerable prompt and withholds everything.
pub const BAR_PERMISSIONS: [&str; 4] = [
    "ReadCliPipes",
    "ChangeApplicationState",
    "ReadApplicationState",
    "RunCommands",
];

/// The clave session config: Alt keybinds in shared_among normal+locked
/// (invariant #6 — they must fire while Claude has focus), defaults kept.
pub fn config_kdl(wasm: &str) -> String {
    let nav = |payload: &str| {
        format!(
            "MessagePlugin \"file:{wasm}\" {{ name \"clave-nav\"; payload \"{payload}\"; }}"
        )
    };
    let mut binds = String::new();
    binds.push_str(&format!(
        "        bind \"Alt a\" {{ Run \"clave\" \"add\" {{ floating true; close_on_exit true; }}; }}\n"
    ));
    binds.push_str(&format!(
        "        bind \"Alt c\" {{ MessagePlugin \"file:{wasm}\" {{ name \"clave-toggle\"; }}; }}\n"
    ));
    binds.push_str("        bind \"Alt w\" { CloseTab; }\n");
    binds.push_str(&format!("        bind \"Alt j\" \"Alt Down\" {{ {} }}\n", nav("{\\\"dir\\\":\\\"next\\\"}")));
    binds.push_str(&format!("        bind \"Alt k\" \"Alt Up\" {{ {} }}\n", nav("{\\\"dir\\\":\\\"prev\\\"}")));
    for n in 1..=9 {
        binds.push_str(&format!("        bind \"Alt {n}\" {{ {} }}\n", nav(&format!("{{\\\"row\\\":{n}}}"))));
    }
    format!(
        "// GENERATED by `clave setup` — regenerate, don't hand-edit.\n\
         // clear-defaults=false: stock zellij behaviour stays; clave only ADDS.\n\
         keybinds clear-defaults=false {{\n\
         \x20   shared_among \"normal\" \"locked\" {{\n{binds}\x20   }}\n\
         }}\n"
    )
}

/// The session layout: EVERY tab gets the bar via default_tab_template
/// (§6.8). Task 9 checkpoint C6 validates the template survives real use;
/// fallback = per-tab panes + a new-tab keybind with an explicit layout.
pub fn layout_kdl(wasm: &str) -> String {
    format!(
        "// GENERATED by `clave setup` — regenerate, don't hand-edit.\n\
         layout {{\n\
         \x20   default_tab_template {{\n\
         \x20       pane size=26 borderless=true {{\n\
         \x20           plugin location=\"file:{wasm}\"\n\
         \x20       }}\n\
         \x20       children\n\
         \x20   }}\n\
         \x20   tab name=\"clave\" focus=true\n\
         }}\n"
    )
}

/// Additively merge clave's hook registrations into a settings.json value.
/// Never touches existing entries; skips events already carrying our command.
pub fn merge_hooks(settings: &mut serde_json::Value, clave_bin: &str) -> bool {
    // The §6.5 state machine's input events. PermissionRequest/StopFailure
    // are handled IF the CLI sends them, but registration sticks to the
    // documented set; Notification covers the needs-you cases.
    const EVENTS: [&str; 4] = ["UserPromptSubmit", "Stop", "Notification", "SessionEnd"];
    let mut changed = false;
    let hooks = settings
        .as_object_mut()
        .map(|o| o.entry("hooks").or_insert_with(|| serde_json::json!({})))
        .expect("settings.json root must be an object");
    for ev in EVENTS {
        let cmd = format!("{clave_bin} hook {ev}");
        let arr = hooks
            .as_object_mut()
            .expect("hooks must be an object")
            .entry(ev)
            .or_insert_with(|| serde_json::json!([]));
        let entries = arr.as_array_mut().expect("hook event must be an array");
        let present = entries.iter().any(|e| {
            e["hooks"]
                .as_array()
                .is_some_and(|hs| hs.iter().any(|h| h["command"] == serde_json::json!(cmd)))
        });
        if !present {
            entries.push(serde_json::json!({
                "hooks": [ { "type": "command", "command": cmd } ]
            }));
            changed = true;
        }
    }
    changed
}

/// Zellij's permission cache location (verified on this machine in S1).
pub fn permissions_cache_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("home")?;
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/Caches/org.Zellij-Contributors.Zellij/permissions.kdl"))
    } else {
        Ok(home.join(".cache/zellij/permissions.kdl"))
    }
}

/// Merge our grant into the cache text, preserving everyone else's entries.
/// Format (verified against zellij 0.44.3 PermissionCache::to_string): one
/// quoted-location node per plugin, children = PermissionType names. We
/// remove any existing clave-bar nodes (both key forms) then append fresh
/// ones — replace-not-accumulate keeps re-runs idempotent even when the
/// permission set changes (the S2 lesson).
pub fn merge_permissions_kdl(existing: &str, wasm_abs: &str) -> String {
    let keys = [format!("\"file:{wasm_abs}\""), format!("\"{wasm_abs}\"")];
    let mut out = String::new();
    let mut skipping = false;
    for line in existing.lines() {
        let t = line.trim_start();
        if !skipping && keys.iter().any(|k| t.starts_with(k.as_str())) {
            skipping = true; // drop this node…
        }
        if skipping {
            if t.trim_end().ends_with('}') {
                skipping = false; // …through its closing brace
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    for key in keys {
        out.push_str(&format!("{key} {{\n"));
        for p in BAR_PERMISSIONS {
            out.push_str(&format!("    {p}\n"));
        }
        out.push_str("}\n");
    }
    out
}

/// The whole setup weave. Idempotent by construction — every part merges.
pub fn run_setup() -> Result<()> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    let wasm = wasm_path()?;
    let wasm_str = wasm.to_str().context("wasm path")?;
    anyhow::ensure!(
        wasm.exists(),
        "{} missing — run `just install` first (it copies the built wasm here)",
        wasm.display()
    );
    std::fs::write(dir.join("config.kdl"), config_kdl(wasm_str))?;
    std::fs::write(dir.join("layout.kdl"), layout_kdl(wasm_str))?;

    // Hooks: read-merge-write ~/.claude/settings.json. The path may be a
    // symlink into a dotfiles repo — fs::read/write follow it, which is
    // exactly what we want (§6.5).
    let settings_path = dirs::home_dir().context("home")?.join(".claude/settings.json");
    let mut settings: serde_json::Value = match std::fs::read(&settings_path) {
        Ok(b) => serde_json::from_slice(&b).context("parsing ~/.claude/settings.json")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e).context("reading settings.json"),
    };
    if merge_hooks(&mut settings, "clave") {
        std::fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)?;
        println!("hooks merged into {}", settings_path.display());
    } else {
        println!("hooks already registered");
    }

    // Permissions pre-seed (§7): merge, preserving other plugins.
    let cache = permissions_cache_path()?;
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::read_to_string(&cache).unwrap_or_default();
    std::fs::write(&cache, merge_permissions_kdl(&existing, wasm_str))?;
    println!("permissions seeded in {}", cache.display());
    Ok(())
}

/// Bare `clave`: attach-or-create the dedicated session with OUR config +
/// layout (§6.8 — the user's global zellij config is never touched).
pub fn launch_session() -> Result<()> {
    let dir = data_dir()?;
    let (config, layout) = (dir.join("config.kdl"), dir.join("layout.kdl"));
    anyhow::ensure!(config.exists() && layout.exists(), "run `clave setup` first");
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new("zellij")
        .arg("--config").arg(&config)
        .arg("--layout").arg(&layout)
        .args(["attach", "--create", "clave"])
        .exec();
    Err(anyhow::anyhow!("exec zellij failed: {err}"))
}
```

`main.rs`: make the subcommand `Option<Command>`; `None => setup::launch_session()`; add `Command::Setup => setup::run_setup()`. `justfile` additions:

```make
# Copy both artifacts where the generated layout/config expect them:
# the wasm into ~/.local/share/clave/, the binary onto PATH.
install: build build-bar-release
    mkdir -p ~/.local/share/clave
    cp target/wasm32-wasip1/release/clave-bar.wasm ~/.local/share/clave/
    cargo install --path crates/clave --locked

clippy:
    cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p clave && cargo build -p clave`
Expected: PASS. (Do NOT run `clave setup` from the subagent — it writes the developer's real settings.json/permission cache. Task 9 runs it live with the user watching.)

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/setup.rs crates/clave/src/lib.rs crates/clave/src/main.rs justfile
git commit -m "feat(clave): setup + session launcher — generated config/layout, additive hooks merge, permission seed"
```

---

## Task 9: Interactive validation (HUMAN-IN-THE-LOOP — main session + user)

Spec: §9 status note (the demoted checkpoints), §6.6 verify-live items. **Do not dispatch the live steps to a headless subagent.** Record every checkpoint verdict in `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` (create it from the checklist below); fold mechanism changes back into the spec, as S2 did. Zellij log for plugin `eprintln!`: `$TMPDIR/zellij-<uid>/zellij-log/zellij.log`.

**Files:**
- Create: `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` (verdicts log)
- Possibly modify: whatever the checkpoints falsify (each has a pre-agreed fallback — apply it, re-test, log).

**Setup (once):**

```bash
just install          # binary on PATH + wasm into ~/.local/share/clave/
clave setup           # generated config/layout + hooks merge + permission seed
```

- [ ] **C1 — Session + tab template.** Run `clave`. Expected: a `clave` session opens; the first tab shows the bar (≈26 cols, left) + a shell. Open a native new tab (stock keybind): the new tab ALSO has a bar pane, and both bars list both tabs. **Falsifies → S1's `default_tab_template` fragility:** fallback = per-tab bar panes in the layout + a bound new-tab action that passes an explicit layout file (rewrite `layout_kdl` accordingly, note in log).
- [ ] **C2 — Agent lifecycle + live glyphs.** `Alt+a` → pick this repo → `new`. Expected: floating picker works; a new tab appears running Claude; the store row exists (`clave ls`). Submit a prompt from another tab's shell watching the bar: glyph turns amber (working) on submit, green (done) on Stop, and the tab RENAME lands once a summary/first-prompt label derives. Trigger a permission prompt: glyph turns red (needs_you) — **verify the Notification message substrings** (`"permission"`, `"waiting for your input"`) against what the current CLI actually sends (adjust `status_for_event` + its test if drifted).
- [ ] **C3 — Unread clear.** With an agent green (done) and focus elsewhere: focus its tab. Expected: glyph dims to idle immediately (local), `clave ls` agrees (persisted via `clave focus`), and it happens ONCE (check the zellij log for a single run).
- [ ] **C4 — Recency order + plain tabs.** Open a plain tab; interleave focusing tabs. Expected: rows reorder by last interaction, focused tab is always row 1, plain tab shows name-only, closing a tab removes its row.
- [ ] **C5 — Nav.** Mouse-click a non-active row → jumps to that tab. `Alt+j`/`Alt+k` walk display order (wrap included); `Alt+2` behaves as alt-tab; `Alt+N` jumps to row N. **Then the `switch_tab_to` attempt (§4):** on a scratch branch, swap `focus_pane_with_id` for `switch_tab_to(position+1)` in `run_effects`, rebuild, retest clicks. If it works, note it as a viable simplification (decide keep/revert with the user); if not, revert — `focus_pane_with_id` stays. Log either way.
- [ ] **C6 — Toggle (`hide_self` reflow).** `Alt+c`: bars hide in EVERY tab and the grid reclaims the width; `Alt+c` again: bars return. While hidden, drive a status change: on show, the bar reflects it (hidden plugins still receive pipes). **Falsifies →** fallback: `close_self()` on toggle-off + a `LaunchOrFocusPlugin`-style relaunch bind on toggle-on (adjust §6.6 + log).
- [ ] **C7 — dump-layout liveness.** With one agent live: `zellij action dump-layout | grep -A2 clave` — confirm the baked `args "spawn" "<uuid>" …` appear (the §6.3 liveness mechanism). Then `Alt+a` → same repo → expected: jumps to the running agent instead of spawning a duplicate. **Falsifies →** fallback: SessionStart/SessionEnd liveness tracking in the store (already sketched in §6.3; implement, log).
- [ ] **C8 — Resume + resurrection (S4).** `Alt+w` the agent's tab; `Alt+a` → same repo → `resume` → pick it: conversation resumes (same history). Then the real S4: `zellij kill-session clave`, relaunch `clave`, press ENTER through the serialization gates. Expected: agent tabs re-run their baked `clave spawn` and RESUME (not fresh); bars rebuild (registers re-fire); glyphs recover after hydration/next event. Note the ENTER-gate friction count (known limitation, §10).
- [ ] **C9 — Hydration (S5).** With agents in the store, kill+relaunch just the session (or reload the plugin): bars show correct glyphs/labels BEFORE any new hook event (i.e. `clave snapshot` hydration worked — check the zellij log for the run_command round-trip).
- [ ] **C10 — Hook safety.** Run a claude session OUTSIDE the clave session (untracked): confirm zero interference and `time clave hook Stop <<< '{"session_id":"not-tracked"}'` is <50ms with exit 0. Also `echo garbage | clave hook Stop; echo $?` → 0.
- [ ] **Record + reconcile.** Fill SUBSYSTEM-VALIDATION.md with per-checkpoint verdicts and any mechanism deltas; update spec §4/§6 where reality disagreed (as S2 did); commit the log + any fixes.

```bash
git add docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md
git commit -m "docs(validation): subsystem interactive validation verdicts (C1–C10)"
```

---

## Task 10: Deferred-minors sweep + final whole-branch review

**Files:**
- Modify: any manifest still missing why-comments (`crates/*/Cargo.toml` — deferred Task-1 minor), `crates/clave/src/main.rs` (crate doc must describe the REAL mechanism: bar decorates rows + renames real tabs; no "status emoji into the tab title"), anything clippy flags.

- [ ] **Step 1: Sweep.** Verify each deferred minor from the ledger is now closed: manifest comments (Tasks 1/2 here), clave-types doc/test asymmetry (Task 1 here), silent-drop payload logging (Task 6 here). Fix any stragglers.
- [ ] **Step 2: Full gates.**

Run: `cargo test && cargo test -p clave-bar && cargo build -p clave-bar --target wasm32-wasip1 && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: all green. Fix and re-run until they are.

- [ ] **Step 3: Final review.** Request a whole-branch review (superpowers:requesting-code-review or fugu-review) over everything since `dd38ace`, with the revised spec as the brief. Apply Critical/Important findings; log the rest.
- [ ] **Step 4: Commit the sweep** (if it changed anything):

```bash
git add -u crates justfile
git commit -m "chore: close deferred minors; fmt/clippy sweep post-review"
```

---

## Post-plan notes for the conductor

- Tasks 1–5 are independent enough to review quickly in sequence; Task 6 is the largest single diff — budget review time there.
- Task 7/8 cross-reference (`wasm_path`): see Task 7's ordering note.
- After Task 9's verdicts, if `switch_tab_to` or the tab-template fallback changed mechanisms, update spec §4/§6 in the SAME commit as the code change — the spec's "verified knowledge base" only works if it stays true.
