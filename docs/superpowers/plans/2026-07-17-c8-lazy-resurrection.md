# C8 Lazy Resurrection + `clave dev` Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace zellij-serialization-based resurrection with clave-owned lazy
resume (dormant bar rows, dwell-to-open, eager most-recent at launch) and build
the `clave dev` sandboxed live-validation harness.

**Architecture:** Spec commit `65c7e6e` — see
`docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` §6.3 (`clave
open`), §6.6 (dormant rows, dwell nav), §6.8 (serialization off, dynamic launch
layout), §6.9 (harness), §5 (`stale` flag), invariants #5/#11. Zellij session
serialization is turned off; the store becomes the resurrection source of truth.
The bar renders store rows without a live tab as dormant (◌); nav settling on
one for 0.4s (or an explicit click/Alt+N) fires `clave open <uuid>`, which
reuses the §6.3 one-shot-layout machinery and spawn's jsonl-driven
create-vs-resume idempotence.

**Tech Stack:** Rust edition 2024, cargo workspace (`clave`, `clave-bar` wasm32-wasip1, `clave-types`), zellij 0.44.3 (vendored sources at `~/.cargo/registry/src/*/zellij-{tile,utils}-0.44.3/`), serde/serde_json, fs4, tempfile (dev-dep).

## Global Constraints

- **Ask Ollie before every commit** (standing preference). Commit messages: conventional commits ending with the `Claude-Session:` trailer line used by this session.
- TDD per change: failing test → minimal code → pass → commit. Tests live in `#[cfg(test)] mod tests` in the same file (repo convention).
- Comments explain **why**, denser than typical (Ollie's preference). Match existing comment style (spec § references, finding citations).
- NEVER run bare `zellij` commands from the implementing shell — Claude's shell is inside Ollie's `main` session. Only `ZELLIJ_SESSION_NAME=clave-test`-scoped commands and read-only `zellij list-sessions` are sanctioned. Session lifecycle (launch/kill) of any session is Ollie's.
- Do not touch the 4 parked clippy lints (add.rs, store.rs ×2, lsview.rs — Task 10 of the outer project). New code must be clippy-clean.
- `cargo test --workspace` green after every task. Final wasm build via `CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S) cargo build -p clave-bar --target wasm32-wasip1 --release`.
- Dwell = **0.4s**, peek sink = **0.9s** — both user-tuned named constants; never normalize them.
- Zellij semantics used here were source-verified (v0.44.3): serialization records the ppid-priority discovered process (`zellij-server/src/pty.rs populate_session_layout_metadata`); `Event::Timer(f64)` carries the *elapsed* sleep seconds (`zellij-server/src/plugins/zellij_exports.rs:2462`, `zellij-utils/src/plugin_api/event.rs:112`), so 0.4s and 0.9s timers are distinguishable by an `elapsed < 0.65` threshold.

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/clave/src/env.rs` | **Create** | Env-override readers: session name, state dir, data dir, claude config dir |
| `crates/clave/src/evlog.rs` | **Create** | One-line-JSON event log at `<state>/clave.log` |
| `crates/clave/src/open.rs` | **Create** | `clave open <uuid>`: pure decision fn + side-effectful runner |
| `crates/clave/src/dev.rs` | **Create** | `clave dev scenario/status/reset` harness |
| `crates/clave/src/lib.rs` | Modify | Register the four new modules |
| `crates/clave/src/main.rs` | Modify | CLI arms: `Open`, `Dev` |
| `crates/clave/src/store.rs` | Modify | `store_paths()` env override; `AgentRecord.stale`; `apply_open_result` |
| `crates/clave/src/setup.rs` | Modify | `data_dir()` env override; `session_serialization false`; launch: delete stale EXITED session + dynamic layout with eager most-recent tab |
| `crates/clave/src/add.rs` | Modify | Share `tab_node()` with setup; jsonl scan honors claude config dir; evlog call |
| `crates/clave/src/spawn.rs` | Modify | `jsonl_path` honors claude config dir; evlog call |
| `crates/clave-types/src/lib.rs` | Modify | `Agent.stale: bool` (serde default) |
| `crates/clave-bar/src/model.rs` | Modify | `RowKey`, dormant rows, opening/stale glyphs, nav cursor + dwell, new Effects |
| `crates/clave-bar/src/main.rs` | Modify | Execute `ArmDwell`/`ArmPeek`/`OpenAgent`; Timer disambiguation |

Task order matters: 1→2 are foundations consumed everywhere; 3–7 are CLI-side; 8–9 are bar-side; 10 is the harness; 11 is final verification.

---

### Task 1: `env.rs` — sandbox env overrides

**Files:**
- Create: `crates/clave/src/env.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod env;`)
- Modify: `crates/clave/src/store.rs:94-102` (`store_paths` honors override)
- Modify: `crates/clave/src/setup.rs:14-16` (`data_dir` honors override)

**Interfaces:**
- Consumes: nothing.
- Produces (spec §6.9): `env::session_name() -> String`; `env::claude_config_dir() -> anyhow::Result<PathBuf>`; pure kernels `session_name_from(Option<String>) -> String`, `dir_from(Option<String>, default: PathBuf) -> PathBuf`. `store::store_paths()` / `setup::data_dir()` keep their signatures but consult `$CLAVE_STATE_DIR` / `$CLAVE_DATA_DIR`.

- [ ] **Step 1: Write the failing tests** — in the new `crates/clave/src/env.rs` (pure kernels only; the thin `std::env::var` readers stay untested — env vars are process-global and would race parallel tests):

```rust
//! Sandbox env overrides (spec §6.9): the `clave dev` harness redirects every
//! path/session lookup through these so a scenario can never touch the real
//! store, session, or ~/.claude. Pure kernels + thin env readers: the kernels
//! are what's unit-tested (setting real env vars would race parallel tests).

use std::path::PathBuf;

use anyhow::{Context, Result};

/// `$CLAVE_SESSION` or the default dedicated session (§6.8).
pub fn session_name() -> String {
    session_name_from(std::env::var("CLAVE_SESSION").ok())
}

pub fn session_name_from(var: Option<String>) -> String {
    var.filter(|s| !s.is_empty())
        .unwrap_or_else(|| "clave".to_string())
}

/// `$CLAUDE_CONFIG_DIR` or `~/.claude` — where Claude Code keeps
/// `projects/<munged>/<uuid>.jsonl` and settings.json. Claude itself honors
/// the same variable, which is what makes the §6.9 sandbox airtight: the
/// REAL claude processes a scenario spawns write their transcripts here too.
pub fn claude_config_dir() -> Result<PathBuf> {
    let default = dirs::home_dir().context("no home dir")?.join(".claude");
    Ok(dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok(), default))
}

pub fn dir_from(var: Option<String>, default: PathBuf) -> PathBuf {
    match var.filter(|s| !s.is_empty()) {
        Some(v) => PathBuf::from(v),
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_defaults_and_overrides() {
        assert_eq!(session_name_from(None), "clave");
        assert_eq!(session_name_from(Some(String::new())), "clave"); // empty = unset
        assert_eq!(session_name_from(Some("clave-test".into())), "clave-test");
    }

    #[test]
    fn dir_from_defaults_and_overrides() {
        let d = PathBuf::from("/default");
        assert_eq!(dir_from(None, d.clone()), d);
        assert_eq!(dir_from(Some(String::new()), d.clone()), d);
        assert_eq!(dir_from(Some("/x".into()), d), PathBuf::from("/x"));
    }
}
```

- [ ] **Step 2: Register the module and run tests** — add `pub mod env;` to `crates/clave/src/lib.rs` (alphabetical: after `add`, before `hook`).

Run: `cargo test -p clave env::`
Expected: PASS (kernels are implemented with the tests in one step — they are 6 lines; the TDD cycle here is the test file itself driving the API shape).

- [ ] **Step 3: Thread the overrides through the path helpers.** In `crates/clave/src/store.rs`, replace the body of `store_paths()`:

```rust
/// Spec §5 names the literal path `~/.local/state/clave/`. Built from $HOME
/// rather than `dirs::state_dir()` because the latter is `None` on macOS and
/// we want one path on every platform. `$CLAVE_STATE_DIR` overrides the whole
/// dir (spec §6.9: the dev harness sandboxes the store).
pub fn store_paths() -> Result<StorePaths> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let default = home.join(".local").join("state").join("clave");
    let dir = crate::env::dir_from(std::env::var("CLAVE_STATE_DIR").ok(), default);
    Ok(StorePaths {
        data: dir.join("agents.json"),
        lock: dir.join("agents.lock"),
        dir,
    })
}
```

In `crates/clave/src/setup.rs`, `data_dir()` gets the same treatment with `CLAVE_DATA_DIR` (keep its existing default `~/.local/share/clave`):

```rust
pub fn data_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("home")?;
    let default = home.join(".local").join("share").join("clave");
    Ok(crate::env::dir_from(
        std::env::var("CLAVE_DATA_DIR").ok(),
        default,
    ))
}
```

(Adjust to the actual current body at `setup.rs:14` — preserve any existing doc comment, appending the §6.9 note.)

- [ ] **Step 4: Run the full crate tests**

Run: `cargo test -p clave`
Expected: PASS (no behavior change with the vars unset).

- [ ] **Step 5: Ask Ollie, then commit**

```bash
git add crates/clave/src/env.rs crates/clave/src/lib.rs crates/clave/src/store.rs crates/clave/src/setup.rs
git commit -m "feat(clave): §6.9 env overrides — CLAVE_SESSION/STATE_DIR/DATA_DIR, CLAUDE_CONFIG_DIR"
```

---

### Task 2: `stale` flag — clave-types + store + `apply_open_result`

**Files:**
- Modify: `crates/clave-types/src/lib.rs` (Agent)
- Modify: `crates/clave/src/store.rs` (AgentRecord, `snapshot_from`, new `apply_open_result`)

**Interfaces:**
- Consumes: Task 1 nothing directly.
- Produces: `Agent.stale: bool` (serde default), `AgentRecord.stale: bool` (serde default), `store::apply_open_result(paths: &StorePaths, uuid: &str, stale: bool) -> Result<Option<AgentSnapshot>>` — Some(snapshot) iff the flag changed (Task 6 pushes it), None otherwise or unknown uuid.

- [ ] **Step 1: Write the failing tests.** In `crates/clave-types/src/lib.rs` tests:

```rust
#[test]
fn agent_stale_roundtrips_and_defaults_false() {
    // §5 (2026-07-17): `stale` = `clave open` found the row's cwd missing →
    // bar ✗. A row flag, NOT a status (statuses are hook lifecycle).
    let mut a = Agent {
        uuid: "u1".into(),
        cwd: "/x".into(),
        repo_root: "/x".into(),
        branch: "main".into(),
        label: "x · main".into(),
        status: Status::Idle,
        last_interacted: 0,
        last_visited: 0,
        tab_id: None,
        stale: true,
    };
    let back: Agent = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    assert!(back.stale);
    // Pre-field payloads must parse as not-stale.
    a.stale = false;
    let mut v: serde_json::Value = serde_json::to_value(&a).unwrap();
    v.as_object_mut().unwrap().remove("stale");
    let old: Agent = serde_json::from_value(v).unwrap();
    assert!(!old.stale);
}
```

In `crates/clave/src/store.rs` tests:

```rust
#[test]
fn open_result_sets_and_clears_stale_on_change_only() {
    // §6.3 `clave open`: cwd missing → stale=true (bar ✗); a later
    // successful open clears it. Snapshot back only on CHANGE (§5: no
    // no-op pushes), None for unknown uuids (row may have been pruned).
    let d = tempfile::tempdir().unwrap();
    let p = tmp_paths(d.path());
    with_store_mut(&p, |s| {
        s.agents.insert("u1".into(), rec("u1"));
    })
    .unwrap();
    let snap = apply_open_result(&p, "u1", true).unwrap().expect("changed");
    assert!(snap.agents[0].stale);
    assert_eq!(snap.seq, 1);
    // Same value again: no change, no seq bump, no push.
    assert!(apply_open_result(&p, "u1", true).unwrap().is_none());
    assert_eq!(read_store(&p).unwrap().seq, 1);
    // Successful open clears it.
    let snap = apply_open_result(&p, "u1", false).unwrap().expect("cleared");
    assert!(!snap.agents[0].stale);
    // Unknown uuid: silently none.
    assert!(apply_open_result(&p, "ghost", true).unwrap().is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p clave-types agent_stale && cargo test -p clave open_result`
Expected: FAIL — `no field 'stale'` compile errors.

- [ ] **Step 3: Implement.** `clave-types` `Agent` gains (after `tab_id`):

```rust
/// §5 (2026-07-17): `clave open` found the row's cwd missing → the bar
/// renders ✗ instead of ◌. A row flag, NOT a status (statuses are hook
/// lifecycle); cleared by a later successful open. `default` keeps
/// pre-field payloads parseable.
#[serde(default)]
pub stale: bool,
```

`store.rs` `AgentRecord` gains the same field + doc comment. `snapshot_from` maps `stale: r.stale`. New fn after `apply_bind`:

```rust
/// `clave open` outcome (§6.3, 2026-07-17): record whether the row's cwd was
/// missing. Snapshot back only on CHANGE (a repeated stale open must not
/// generate pipe traffic); None for unknown uuids.
pub fn apply_open_result(
    paths: &StorePaths,
    uuid: &str,
    stale: bool,
) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        let r = s.agents.get_mut(uuid)?;
        if r.stale == stale {
            return None;
        }
        r.stale = stale;
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}
```

- [ ] **Step 4: Fix every `Agent`/`AgentRecord` literal.** Compile will list them: `store.rs::tests::rec`, `add.rs` step-7 `fresh` record + `tests::rec`, `hook.rs` (any Agent construction), `clave-types` tests (`agent_json_has_no_archived_field`, `snapshot_roundtrips`, `agent_tab_id_roundtrips_and_defaults_none`), `clave-bar/src/model.rs::tests::agent`. Add `stale: false` to each.

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Ask Ollie, then commit**

```bash
git add crates/clave-types/src/lib.rs crates/clave/src/store.rs crates/clave/src/add.rs crates/clave/src/hook.rs crates/clave-bar/src/model.rs
git commit -m "feat(clave): §5 stale row flag + apply_open_result (C8)"
```

---

### Task 3: `evlog.rs` — the observability event log

**Files:**
- Create: `crates/clave/src/evlog.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod evlog;`)

**Interfaces:**
- Consumes: `store::store_paths()` (Task 1's override-aware version — the log lands in the sandbox automatically).
- Produces (spec §6.9): `evlog::log_event(cmd: &str, detail: &str)` — best-effort, never errors to the caller; one JSON line `{"ts":<unix_s>,"cmd":"open","detail":"..."}` appended to `<state>/clave.log`. Callers added in Tasks 5/6/7/10.

- [ ] **Step 1: Write the failing test** in the new file:

```rust
//! §6.9 observability: every clave CLI invocation appends ONE JSON line —
//! timestamp, command, decision — to `<state>/clave.log`. This is the log
//! Claude reads after each user-driven validation step. Best-effort by
//! design: a logging failure must never break a spawn/open/hook (same
//! zero-risk stance as `clave hook`, §6.5).

use std::io::Write;

/// Append one event line. Swallows all errors (stderr note only).
pub fn log_event(cmd: &str, detail: &str) {
    if let Err(e) = try_log(cmd, detail) {
        eprintln!("clave evlog: {e:#}");
    }
}

fn try_log(cmd: &str, detail: &str) -> anyhow::Result<()> {
    let paths = crate::store::store_paths()?;
    std::fs::create_dir_all(&paths.dir)?;
    let line = serde_json::json!({
        "ts": crate::store::now_unix(),
        "cmd": cmd,
        "detail": detail,
    });
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.dir.join("clave.log"))?;
    writeln!(f, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn log_lines_are_json_with_ts_cmd_detail() {
        // Shape-only test through the serializer (try_log's path comes from
        // store_paths(), which tests can't redirect without process-global
        // env — the WRITE path is exercised live by every C8 scenario).
        let line = serde_json::json!({"ts": 1u64, "cmd": "open", "detail": "d"});
        let v: serde_json::Value = serde_json::from_str(&line.to_string()).unwrap();
        assert_eq!(v["cmd"], "open");
        assert!(v["ts"].is_u64());
    }
}
```

- [ ] **Step 2: Register + run**

Add `pub mod evlog;` to `lib.rs`. Run: `cargo test -p clave evlog`
Expected: PASS.

- [ ] **Step 3: Ask Ollie, then commit**

```bash
git add crates/clave/src/evlog.rs crates/clave/src/lib.rs
git commit -m "feat(clave): §6.9 clave.log event log"
```

---

### Task 4: `session_serialization false` in the generated config

**Files:**
- Modify: `crates/clave/src/setup.rs:34-84` (`config_kdl`)

**Interfaces:**
- Consumes: nothing new.
- Produces: config text containing a top-level `session_serialization false` line. (§6.8: zellij's serializer records the ppid-priority *discovered* process — post-exec `claude --session-id`, mid-tool-call a child like `cargo build` — so it can never be the resume path.)

- [ ] **Step 1: Write the failing test** in `setup.rs` tests:

```rust
#[test]
fn config_disables_session_serialization() {
    // §6.8 (2026-07-17, C8): resurrection is clave-owned; a serialized
    // session would replay discovered `claude --session-id` commands
    // (create-collision) or mid-tool-call children (pty.rs ppid-priority
    // discovery, v0.44.3 source-verified).
    let kdl = config_kdl("/w.wasm");
    assert!(kdl.contains("session_serialization false"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p clave config_disables`
Expected: FAIL — assertion.

- [ ] **Step 3: Implement.** In `config_kdl`, add the option line above the keybinds block (top-level KDL option, sibling of `keybinds`):

```rust
    format!(
        "// GENERATED by `clave setup` — regenerate, don't hand-edit.\n\
         // §6.8 C8: resurrection is clave-owned (launch + clave open);\n\
         // zellij serialization replays DISCOVERED commands and is off.\n\
         session_serialization false\n\
         // clear-defaults=false: stock zellij behaviour stays; clave only ADDS.\n\
         keybinds clear-defaults=false {{\n\
         \x20   shared_among \"normal\" \"locked\" {{\n{binds}\x20   }}\n\
         }}\n"
    )
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p clave setup`
Expected: PASS (existing config tests still green — check none asserts the exact previous header text; adjust if one does).

- [ ] **Step 5: Ask Ollie, then commit**

```bash
git add crates/clave/src/setup.rs
git commit -m "feat(clave): §6.8 session_serialization off — resurrection is clave-owned"
```

---

### Task 5: launch path — shared tab node, dynamic layout, stale-session delete

**Files:**
- Modify: `crates/clave/src/add.rs:70-88` (`tab_layout` → extract `tab_node`)
- Modify: `crates/clave/src/setup.rs` (`launch_layout_kdl`, `launch_session`, `session_exists`)

**Interfaces:**
- Consumes: `add::tab_node(wasm, label, uuid, cwd) -> String` (extracted here), `store::read_store`, `env::session_name`.
- Produces: `setup::launch_layout_kdl(wasm: &str, most_recent: Option<&AgentRecord>) -> String`; `setup::session_exists(list_output: &str, name: &str) -> bool`. `launch_session` composes the dynamic layout to a temp file and pre-deletes a dead serialized session.

- [ ] **Step 1: Extract the shared tab node (refactor, tests stay green).** In `add.rs`, split `tab_layout`:

```rust
/// The single agent-tab KDL node — shared verbatim by the §6.3 one-shot add
/// layout and the §6.8 eager launch tab so the two can never drift.
pub fn tab_node(wasm: &str, label: &str, uuid: &str, cwd: &str) -> String {
    // split_direction="vertical" is REQUIRED for a LEFT bar: zellij stacks
    // sibling panes horizontally (rows) by default (Task 9 C1 finding; same
    // wrapper as setup::layout_kdl and the S2 spike layout).
    format!(
        r#"    tab name="{label}" focus=true {{
        pane split_direction="vertical" {{
            pane size=30 borderless=true {{
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

/// The one-shot temp layout (§6.3): Zellij KDL has no variable substitution,
/// so the uuid/label/cwd are baked in, the file is passed to
/// `zellij action new-tab --layout`, then deleted. Baking the command in
/// makes tab creation IDEMPOTENT — resurrection is clave's job, not
/// zellij's (§6.8, C8 redesign 2026-07-17).
pub fn tab_layout(wasm: &str, label: &str, uuid: &str, cwd: &str) -> String {
    format!("layout {{\n{}}}\n", tab_node(wasm, label, uuid, cwd))
}
```

Run: `cargo test -p clave add`
Expected: PASS — `tab_layout_bakes_the_idempotent_spawn` still green (KDL content is equivalent; if the test asserts exact indentation, match the original 4-space nesting).

- [ ] **Step 2: Write the failing tests for the launch composer + session_exists** in `setup.rs` tests:

```rust
#[test]
fn launch_layout_is_bar_only_when_store_empty() {
    // §6.8 cold start, empty store: today's behavior — template + one
    // plain tab, no agent tabs.
    let kdl = launch_layout_kdl("/w.wasm", None);
    assert!(kdl.contains("default_tab_template"));
    assert!(kdl.contains("tab name=\"clave\" focus=true"));
    assert!(!kdl.contains("\"spawn\""));
}

#[test]
fn launch_layout_eager_loads_only_the_most_recent_row() {
    // §6.8: eagerness of exactly ONE — the most-recent agent resumes
    // focused at launch; every other row stays dormant in the bar.
    let mut r = AgentRecord {
        uuid: "u-recent".into(),
        cwd: "/repo/.claude-worktrees/ab".into(), // worktree row: bake ITS cwd
        repo_root: "/repo".into(),
        branch: "main".into(),
        label: "repo · main".into(),
        status: clave_types::Status::Idle,
        last_interacted: 100,
        last_visited: 0,
        worktree: Some("/repo/.claude-worktrees/ab".into()),
        label_source: crate::store::LabelSource::FirstPrompt,
        tab_id: None,
        stale: false,
    };
    let kdl = launch_layout_kdl("/w.wasm", Some(&r));
    assert!(kdl.contains("default_tab_template")); // native new-tabs still barred
    assert!(kdl.contains("\"spawn\" \"u-recent\""));
    assert!(kdl.contains("cwd=\"/repo/.claude-worktrees/ab\""));
    // The eager tab replaces the plain placeholder tab entirely.
    assert!(!kdl.contains("tab name=\"clave\" focus=true"));
    r.label = "x".into(); // silence unused-mut if needed
}

#[test]
fn session_exists_vs_live_distinguish_exited() {
    let out = "clave [Created 2h ago] (EXITED - attach to resurrect)\nother [Created 1m ago]\n";
    assert!(session_exists(out, "clave"));
    assert!(!session_is_live(out, "clave")); // existing fn, unchanged
    assert!(!session_exists(out, "missing"));
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p clave launch_layout && cargo test -p clave session_exists`
Expected: FAIL — functions not defined.

- [ ] **Step 4: Implement** in `setup.rs`:

```rust
/// §6.8 (C8): the launch layout, composed DYNAMICALLY at session-create
/// time. Base = the bar template; store non-empty → ONE eager tab for the
/// most-recent row, baked `clave spawn` (resumes via the jsonl check).
/// Everything else surfaces as dormant bar rows (§6.6).
pub fn launch_layout_kdl(wasm: &str, most_recent: Option<&crate::store::AgentRecord>) -> String {
    let tab = match most_recent {
        Some(r) => crate::add::tab_node(wasm, r.label.as_str(), &r.uuid, &r.cwd),
        None => "    tab name=\"clave\" focus=true\n".to_string(),
    };
    format!(
        "// GENERATED at launch — §6.8 clave-owned cold start.\n\
         layout {{\n\
         \x20   default_tab_template split_direction=\"vertical\" {{\n\
         \x20       pane size=30 borderless=true {{\n\
         \x20           plugin location=\"file:{wasm}\"\n\
         \x20       }}\n\
         \x20       children\n\
         \x20   }}\n{tab}}}\n"
    )
}

/// Does `zellij list-sessions -n` mention this session at all (live OR
/// EXITED)? An EXITED session must be DELETED before create: `attach
/// --create` would resurrect its serialized state, ignoring `--layout`
/// (§6.8) — replaying pre-C8 discovered commands.
pub fn session_exists(list_output: &str, name: &str) -> bool {
    list_output
        .lines()
        .any(|l| l.split_whitespace().next() == Some(name))
}
```

Replace `launch_session` (keep the tab-timeline hygiene; note the label passed to `tab_node` is already store-sanitized at add time, but re-sanitize for KDL safety since labels can be hook-derived — reuse `add::sanitize_label`):

```rust
/// Bare `clave`: attach-or-create the dedicated session with OUR config +
/// a DYNAMIC layout (§6.8 C8: eager most-recent tab; serialization is off,
/// so a dead session is deleted, never resurrected).
pub fn launch_session() -> Result<()> {
    let dir = data_dir()?;
    let config = dir.join("config.kdl");
    anyhow::ensure!(config.exists(), "run `clave setup` first");
    let session = crate::env::session_name();
    let list = std::process::Command::new("zellij")
        .args(["list-sessions", "-n"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let live = session_is_live(&list, &session);
    if !live {
        // §6.6 hygiene: tab_ids are SESSION-scoped — drop the previous
        // session's timeline + binds before a CREATE.
        crate::store::clear_tab_timeline(&crate::store::store_paths()?)?;
        if session_exists(&list, &session) {
            // Dead-but-serialized (pre-C8 state, or zellij's own cache):
            // delete so attach --create builds from OUR layout.
            let _ = std::process::Command::new("zellij")
                .args(["delete-session", "--force", &session])
                .status();
        }
    }
    // Compose the launch layout from the store (eager most-recent, §6.8).
    // Harmless when live (attach ignores --layout for an existing session).
    let store = crate::store::read_store(&crate::store::store_paths()?)?;
    let most_recent = store.agents.values().max_by_key(|r| r.last_interacted);
    let wasm = wasm_path()?;
    let layout_text = launch_layout_kdl(
        wasm.to_str().context("wasm path")?,
        most_recent,
    );
    let layout = std::env::temp_dir().join(format!("clave-launch-{}.kdl", std::process::id()));
    std::fs::write(&layout, layout_text)?;
    crate::evlog::log_event(
        "launch",
        &format!(
            "session={session} live={live} eager={:?}",
            most_recent.map(|r| r.uuid.as_str())
        ),
    );
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new("zellij")
        .arg("--config")
        .arg(&config)
        .arg("--layout")
        .arg(&layout)
        .args(["attach", "--create", &session])
        .exec();
    Err(anyhow::anyhow!("exec zellij failed: {err}"))
}
```

Note: `run_setup` still writes the static `layout.kdl` — leave it (harmless, and `clave setup`'s "run setup first" check now only needs config.kdl; update the `ensure!` as shown). The eager tab's label goes through `add::sanitize_label(&r.label)` — wrap the `tab_node` call: `crate::add::tab_node(wasm, &crate::add::sanitize_label(&r.label), &r.uuid, &r.cwd)` (make `sanitize_label` `pub` in add.rs if it isn't).

- [ ] **Step 5: Run tests**

Run: `cargo test -p clave`
Expected: PASS.

- [ ] **Step 6: Ask Ollie, then commit**

```bash
git add crates/clave/src/setup.rs crates/clave/src/add.rs
git commit -m "feat(clave): §6.8 clave-owned cold start — dynamic launch layout, eager most-recent, stale-session delete"
```

---

### Task 6: `clave open <uuid>`

**Files:**
- Create: `crates/clave/src/open.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod open;`)
- Modify: `crates/clave/src/main.rs` (CLI arm)

**Interfaces:**
- Consumes: `add::{live_uuids, tab_layout, sanitize_label}`, `store::{read_store, apply_open_result, store_paths}`, `hook::push_snapshot`, `setup::wasm_path`, `env::session_name`, `evlog::log_event`, `AgentRecord.stale` (Task 2).
- Produces: CLI `clave open <uuid>` (hidden). `open::OpenDecision { AlreadyLive, Stale, Open }` + `open::open_decision(row: &AgentRecord, is_live: bool, cwd_exists: bool) -> OpenDecision`. The bar invokes the CLI via `run_command` (Task 9).

- [ ] **Step 1: Write the failing tests** in the new `open.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AgentRecord, LabelSource};
    use clave_types::Status;

    fn rec(uuid: &str) -> AgentRecord {
        AgentRecord {
            uuid: uuid.into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            stale: false,
        }
    }

    #[test]
    fn open_decision_is_noop_for_live_stale_for_missing_cwd() {
        // §6.3 clave open guards, in priority order:
        // 1. liveness no-op — dwell-timer/click double-fire protection
        //    (second guard; the bar's in-flight set is the first).
        // 2. staleness — missing cwd (deleted worktree) → no tab, bar ✗.
        let r = rec("u1");
        assert_eq!(open_decision(&r, true, true), OpenDecision::AlreadyLive);
        assert_eq!(open_decision(&r, true, false), OpenDecision::AlreadyLive);
        assert_eq!(open_decision(&r, false, false), OpenDecision::Stale);
        assert_eq!(open_decision(&r, false, true), OpenDecision::Open);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p clave open_decision`
Expected: FAIL — module/function not defined.

- [ ] **Step 3: Implement** `open.rs`:

```rust
//! `clave open <uuid>` (§6.3, C8 2026-07-17): the non-interactive sibling of
//! `add` — open a known store row's tab. Invoked by the bar's executor
//! instance when a dormant row's focus settles (0.4s dwell) or on an
//! explicit pick (click / Alt+N). No picker: the row IS the choice.

use anyhow::{Context, Result};

use crate::store::AgentRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDecision {
    /// uuid already in dump-layout: do nothing (double-fire guard #2 —
    /// the bar's in-flight set is #1; `live_uuids` can transiently miss a
    /// mid-tool-call agent, §10, so BOTH exist).
    AlreadyLive,
    /// Row cwd missing on disk (deleted worktree / moved repo): no tab;
    /// the caller records `stale` so the bar shows ✗. Recovery is manual.
    Stale,
    /// Create the tab (baked idempotent spawn → jsonl check resumes).
    Open,
}

pub fn open_decision(_row: &AgentRecord, is_live: bool, cwd_exists: bool) -> OpenDecision {
    if is_live {
        OpenDecision::AlreadyLive
    } else if !cwd_exists {
        OpenDecision::Stale
    } else {
        OpenDecision::Open
    }
}

pub fn run_open(uuid: &str) -> Result<()> {
    let paths = crate::store::store_paths()?;
    let store = crate::store::read_store(&paths)?;
    let Some(row) = store.agents.get(uuid) else {
        crate::evlog::log_event("open", &format!("{uuid}: unknown uuid"));
        anyhow::bail!("clave open: unknown uuid {uuid}");
    };
    // All zellij invocations are EXPLICITLY session-scoped (§6.9 / the
    // sanctioned-commands rule): run_command children inherit the server's
    // env, but never bet on ambient state.
    let session = crate::env::session_name();
    let dump = std::process::Command::new("zellij")
        .env("ZELLIJ_SESSION_NAME", &session)
        .args(["action", "dump-layout"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let is_live = crate::add::live_uuids(&dump).contains(&uuid.to_string());
    let cwd_exists = std::path::Path::new(&row.cwd).is_dir();
    match open_decision(row, is_live, cwd_exists) {
        OpenDecision::AlreadyLive => {
            crate::evlog::log_event("open", &format!("{uuid}: already live, no-op"));
            Ok(())
        }
        OpenDecision::Stale => {
            crate::evlog::log_event("open", &format!("{uuid}: cwd missing → stale"));
            if let Some(snap) = crate::store::apply_open_result(&paths, uuid, true)? {
                crate::hook::push_snapshot(&snap);
            }
            Ok(())
        }
        OpenDecision::Open => {
            let wasm = crate::setup::wasm_path()?;
            let label = crate::add::sanitize_label(&row.label);
            let layout =
                crate::add::tab_layout(wasm.to_str().context("wasm path")?, &label, uuid, &row.cwd);
            let tmp = std::env::temp_dir().join(format!("clave-open-{uuid}.kdl"));
            std::fs::write(&tmp, layout)?;
            let status = std::process::Command::new("zellij")
                .env("ZELLIJ_SESSION_NAME", &session)
                .args(["action", "new-tab", "--layout", tmp.to_str().context("tmp")?])
                .status()?;
            let _ = std::fs::remove_file(&tmp);
            anyhow::ensure!(status.success(), "zellij action new-tab failed");
            crate::evlog::log_event("open", &format!("{uuid}: tab created (resume via spawn)"));
            // A previously-stale row that opens fine heals (§5).
            if let Some(snap) = crate::store::apply_open_result(&paths, uuid, false)? {
                crate::hook::push_snapshot(&snap);
            }
            Ok(())
        }
    }
}
```

(`sanitize_label` must be `pub` in add.rs — done in Task 5.)

- [ ] **Step 4: CLI arm.** In `main.rs`, add to `enum Command` (after `Bind`):

```rust
/// Open a known store row's tab (plugin-internal, §6.3 C8): the bar fires
/// this when a dormant row's focus settles or on an explicit pick.
#[command(hide = true)]
Open {
    /// The agent's session UUID (the store join key).
    uuid: String,
},
```

and to the match: `Some(Command::Open { uuid }) => open::run_open(&uuid),` (add `open` to the `use clave::{...}` list).

- [ ] **Step 5: Run tests + build**

Run: `cargo test -p clave && cargo build -p clave`
Expected: PASS.

- [ ] **Step 6: Ask Ollie, then commit**

```bash
git add crates/clave/src/open.rs crates/clave/src/lib.rs crates/clave/src/main.rs
git commit -m "feat(clave): §6.3 clave open — liveness no-op, stale marking, idempotent tab create"
```

---

### Task 7: spawn/add honor the sandboxed Claude config dir

**Files:**
- Modify: `crates/clave/src/spawn.rs:24-41` (`jsonl_path`, `spawn_mode`)
- Modify: `crates/clave/src/add.rs` (resume-scan `proj_dir`, ~line 244)
- Modify: `crates/clave/src/main.rs` (Spawn arm passes the config dir; evlog)

**Interfaces:**
- Consumes: `env::claude_config_dir()` (Task 1).
- Produces: `spawn::jsonl_path(claude_dir: &Path, physical_cwd: &str, uuid: &str) -> PathBuf` — **signature change**: first param becomes the claude config dir itself (was `home`, which appended `.claude` internally). `spawn::spawn_mode(claude_dir, ...)` likewise.

- [ ] **Step 1: Update the tests first** (they define the new contract) in `spawn.rs`:

```rust
#[test]
fn jsonl_path_uses_munged_physical_cwd_under_claude_dir() {
    // §6.9: the CLAUDE CONFIG DIR is the parameter (not home) so the
    // sandbox override flows through — real claude processes honor
    // $CLAUDE_CONFIG_DIR and write transcripts to the same tree.
    let claude = std::path::Path::new("/Users/x/.claude");
    let p = jsonl_path(claude, "/Users/x/code/clave", "u-1");
    assert_eq!(
        p,
        std::path::PathBuf::from("/Users/x/.claude/projects/-Users-x-code-clave/u-1.jsonl")
    );
}

#[test]
fn spawn_mode_is_resume_iff_jsonl_exists() {
    let d = tempfile::tempdir().unwrap();
    let claude = d.path().join(".claude");
    let cwd = "/Users/x/code/clave";
    assert_eq!(spawn_mode(&claude, cwd, "u-1"), SpawnMode::Create);
    let dir = claude.join("projects/-Users-x-code-clave");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("u-1.jsonl"), b"{}").unwrap();
    assert_eq!(spawn_mode(&claude, cwd, "u-1"), SpawnMode::Resume);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave spawn`
Expected: FAIL (old signature joins `.claude` internally → double path segment / compile mismatch).

- [ ] **Step 3: Implement.**

```rust
/// Where Claude Code stores this session's transcript, under the given
/// CLAUDE CONFIG DIR (`env::claude_config_dir()` — sandbox-aware, §6.9).
/// `physical_cwd` MUST already be canonicalized (S0b) — pass
/// `std::fs::canonicalize` output, never raw user input.
pub fn jsonl_path(claude_dir: &Path, physical_cwd: &str, uuid: &str) -> PathBuf {
    claude_dir
        .join("projects")
        .join(munge_cwd(physical_cwd))
        .join(format!("{uuid}.jsonl"))
}

pub fn spawn_mode(claude_dir: &Path, physical_cwd: &str, uuid: &str) -> SpawnMode {
    if jsonl_path(claude_dir, physical_cwd, uuid).exists() {
        SpawnMode::Resume
    } else {
        SpawnMode::Create
    }
}
```

Call sites: `main.rs` Spawn arm — replace `let home = dirs::home_dir()...; spawn::spawn_mode(&home, ...)` with:

```rust
let claude_dir = clave::env::claude_config_dir()?;
let mode = spawn::spawn_mode(&claude_dir, &physical_str, &uuid);
clave::evlog::log_event("spawn", &format!("{uuid}: {mode:?}"));
```

(add `#[derive(Debug)]`… `SpawnMode` already derives Debug.) `add.rs` resume scan (~line 244): replace

```rust
let proj_dir = dirs::home_dir()
    .context("home")?
    .join(".claude/projects")
    .join(crate::munge::munge_cwd(&physical_str));
```

with

```rust
let proj_dir = crate::env::claude_config_dir()?
    .join("projects")
    .join(crate::munge::munge_cwd(&physical_str));
```

Also add `crate::evlog::log_event("add", &format!("{uuid}: recorded ({choice})"));` right after the step-7 `push_snapshot(&snap);` in `run_add` (where `choice` is the new/resume string picked in step 4 — it is still in scope).

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Ask Ollie, then commit**

```bash
git add crates/clave/src/spawn.rs crates/clave/src/add.rs crates/clave/src/main.rs
git commit -m "feat(clave): §6.9 spawn/add honor CLAUDE_CONFIG_DIR + evlog decisions"
```

---

### Task 8: bar — dormant rows (render side)

**Files:**
- Modify: `crates/clave-bar/src/model.rs` (`RowKey`, `Row`, `rows()`, dormancy predicate, `opening` set, glyphs)
- Modify: `crates/clave-bar/src/main.rs:444` (render loop field rename only)

**Interfaces:**
- Consumes: `Agent.stale` (Task 2).
- Produces (for Task 9): `RowKey { Tab(usize), Dormant(String) }`; `Row { key: RowKey, name, active, glyph }`; `BarModel::is_dormant(&Agent) -> bool` (private); `opening: BTreeSet<String>` field + `prune_opening()` (private); dormant glyphs ◌ `(‘◌’, 90)`, in-flight `('↻', 33)`, stale `('✗', 31)`.

- [ ] **Step 1: Write the failing tests** in `model.rs` tests (the existing `agent()` builder gains `stale: false`; add a `snap()` helper if one doesn't exist — mirror the existing test-builder style):

```rust
#[test]
fn store_rows_without_live_tabs_render_dormant() {
    // §6.6 C8: row set = TabUpdate ∪ dormant store rows. An agent whose
    // bind points at no current tab and whose registered pane is gone
    // renders ◌ dim, labeled from the store, recency = last_interacted.
    let mut m = BarModel::default();
    m.apply_tabs(vec![tab(1, 0, "shell", true)]); // one plain live tab
    let mut a = agent("u-dormant", Status::Idle, None);
    a.label = "repo · main · fix".into();
    a.last_interacted = 500;
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![a],
        tab_timeline: Default::default(),
    });
    let rows = m.rows();
    assert_eq!(rows.len(), 2);
    let d = rows
        .iter()
        .find(|r| r.key == RowKey::Dormant("u-dormant".into()))
        .expect("dormant row rendered");
    assert_eq!(d.name, "repo · main · fix");
    assert!(!d.active);
    assert_eq!(d.glyph, Some(('◌', 90)));
}

#[test]
fn dormant_rows_sort_into_the_unified_recency_order() {
    // One list, claude.ai-style: live tabs keyed by tab_timeline, dormant
    // rows keyed by last_interacted, merged desc.
    let mut m = BarModel::default();
    m.apply_tabs(vec![tab(1, 0, "live", true)]);
    let mut old = agent("u-old", Status::Idle, None);
    old.last_interacted = 100;
    let mut new = agent("u-new", Status::Idle, None);
    new.last_interacted = 900;
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![old, new],
        tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
    });
    let keys: Vec<_> = m.rows().into_iter().map(|r| r.key).collect();
    assert_eq!(
        keys,
        vec![
            RowKey::Dormant("u-new".into()), // 900
            RowKey::Tab(1),                  // 500
            RowKey::Dormant("u-old".into()), // 100
        ]
    );
}

#[test]
fn agent_with_live_tab_or_registered_pane_is_not_dormant() {
    // The same uuid must never render twice: a bound live tab, OR a
    // registered pane still present in the manifest (pre-bind beat, e.g.
    // right after Alt+a's tab appears), suppresses the dormant row.
    let mut m = BarModel::default();
    m.apply_tabs(vec![tab(7, 0, "agent-tab", true)]);
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![agent("u1", Status::Working, Some(7))], // bound → live
        tab_timeline: Default::default(),
    });
    assert!(
        !m.rows()
            .iter()
            .any(|r| r.key == RowKey::Dormant("u1".into()))
    );
    // Bind gone (fresh session) but the pane join exists → still not dormant.
    let mut m = BarModel::default();
    m.apply_tabs(vec![tab(7, 0, "agent-tab", true)]);
    m.register("u2".into(), 42);
    m.apply_panes(vec![pane(0, 42, false, true)]);
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![agent("u2", Status::Working, None)],
        tab_timeline: Default::default(),
    });
    assert!(
        !m.rows()
            .iter()
            .any(|r| r.key == RowKey::Dormant("u2".into()))
    );
}

#[test]
fn opening_and_stale_glyphs_decorate_dormant_rows() {
    // ↻ while an open is in flight (set by open_effects, Task 9 — poke the
    // set directly here); ✗ when the snapshot says stale. A stale=true
    // snapshot also clears the in-flight mark (the open FAILED — the row
    // must become retryable, not stuck ↻).
    let mut m = BarModel::default();
    let mut a = agent("u1", Status::Idle, None);
    a.stale = true;
    m.opening.insert("u1".into());
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![a],
        tab_timeline: Default::default(),
    });
    let rows = m.rows();
    assert_eq!(rows[0].glyph, Some(('✗', 31)));
    assert!(m.opening.is_empty(), "stale snapshot clears in-flight");
    // In-flight (no stale): ↻.
    let mut m = BarModel::default();
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![agent("u2", Status::Idle, None)],
        tab_timeline: Default::default(),
    });
    m.opening.insert("u2".into());
    assert_eq!(m.rows()[0].glyph, Some(('↻', 33)));
}
```

(Use the existing `tab()`/`pane()` test builders — check their exact signatures in the test module and match; `opening` is `pub(crate)` or the tests live in the same module so a private field is fine — they do, keep it private.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave-bar rows_`
Expected: FAIL — `RowKey` not defined.

- [ ] **Step 3: Implement in `model.rs`.** Row identity:

```rust
/// Row identity (§6.6 C8): a live zellij tab, or a dormant store row
/// (conversation with no tab yet — claude.ai-style list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKey {
    Tab(usize),
    Dormant(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: RowKey,
    pub name: String,
    pub active: bool,
    /// (glyph, ANSI colour) for agent rows; None for plain terminal tabs.
    pub glyph: Option<(char, u8)>,
}
```

`BarModel` gains:

```rust
/// uuids with a `clave open` in flight (§6.6): set on fire, shown ↻.
/// Cleared when the row stops being dormant (tab appeared) or a stale=true
/// snapshot lands (open failed → ✗, retryable). First double-fire guard;
/// `clave open`'s liveness no-op is the second.
opening: BTreeSet<String>,
```

Dormancy + pruning + rows:

```rust
/// §6.6 C8 dormancy: no CURRENT tab carries the bind, and no REGISTERED
/// pane is present in the manifest. The pane-join leg is instance-local —
/// divergence only flickers a dormant row briefly (harmless) — but it
/// suppresses the duplicate row in the pre-bind beat after a tab spawns.
fn is_dormant(&self, a: &Agent) -> bool {
    let tab_live = a
        .tab_id
        .is_some_and(|id| self.tabs.iter().any(|t| t.tab_id == id));
    let pane_live = self
        .uuid_to_pane
        .get(&a.uuid)
        .is_some_and(|p| self.tab_position_of_pane(*p).is_some());
    !tab_live && !pane_live
}

/// Drop in-flight marks that resolved: the row went live (open succeeded)
/// or the snapshot flagged it stale (open failed). Called after every
/// input that changes the join picture.
fn prune_opening(&mut self) {
    let resolved: Vec<String> = self
        .opening
        .iter()
        .filter(|u| {
            self.agents
                .iter()
                .find(|a| &&a.uuid == u)
                .is_none_or(|a| !self.is_dormant(a) || a.stale)
        })
        .cloned()
        .collect();
    for u in resolved {
        self.opening.remove(&u);
    }
}
```

Call `self.prune_opening();` at the end of `apply_snapshot` (before returning effects), `apply_tabs` (before returning), `apply_panes`, and `register`.

`rows()` becomes the union (replace the whole fn):

```rust
/// Rows in display order (§6.6 C8): ONE unified recency-desc list — live
/// tabs keyed by the store tab_timeline, dormant store rows keyed by
/// last_interacted. Tiebreak: tab position for live rows (fresh
/// same-second tabs sit in tab order), uuid for dormant rows (stable).
pub fn rows(&self) -> Vec<Row> {
    // (sort_ts desc, tiebreak asc) — tiebreak: live rows by position,
    // dormant by a large offset + stable index so they never interleave
    // nondeterministically with same-second live rows.
    let mut entries: Vec<(u64, usize, Row)> = Vec::new();
    for t in &self.tabs {
        let glyph = self.agent_in_tab(t.tab_id).map(|a| {
            if a.status == Status::Done && self.read_locally.contains(&a.uuid) {
                Status::Idle.glyph()
            } else {
                a.status.glyph()
            }
        });
        entries.push((
            self.sort_key(t),
            t.position,
            Row {
                key: RowKey::Tab(t.tab_id),
                name: t.name.clone(),
                active: t.active,
                glyph,
            },
        ));
    }
    let mut dormant: Vec<&Agent> = self.agents.iter().filter(|a| self.is_dormant(a)).collect();
    dormant.sort_by(|a, b| a.uuid.cmp(&b.uuid)); // stable tiebreak input
    for (i, a) in dormant.into_iter().enumerate() {
        let glyph = if a.stale {
            ('✗', 31) // open found the cwd missing (§5 stale)
        } else if self.opening.contains(&a.uuid) {
            ('↻', 33) // open in flight
        } else {
            ('◌', 90) // dormant conversation
        };
        entries.push((
            a.last_interacted,
            usize::MAX - i, // after any same-second live row, stable
            Row {
                key: RowKey::Dormant(a.uuid.clone()),
                name: a.label.clone(),
                active: false,
                glyph: Some(glyph),
            },
        ));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    entries.into_iter().map(|(_, _, r)| r).collect()
}
```

Fix the compile fallout mechanically — every `r.tab_id` on a `Row` becomes a match on `r.key`:
- `tab_for_line` (model.rs:269): `let RowKey::Tab(row_tab) = self.rows().get(line)?.key else { return None; };` then find as before. (Task 9 replaces its callers' semantics; here just keep it compiling for clicks on live rows.)
- `nav()` dir-walk `rows.iter().position(|r| r.tab_id == own)` → `position(|r| r.key == RowKey::Tab(own))`.
- `main.rs:444` render loop: unchanged fields except none — `row.glyph`/`row.name`/`row.active` still exist. No render change needed this task.

- [ ] **Step 4: Run all bar tests**

Run: `cargo test -p clave-bar`
Expected: PASS — new tests green, all existing ordering/nav/glyph tests still green (they only use live tabs, whose behavior is unchanged).

- [ ] **Step 5: Ask Ollie, then commit**

```bash
git add crates/clave-bar/src/model.rs crates/clave-bar/src/main.rs
git commit -m "feat(clave-bar): §6.6 dormant rows — unified recency list, ◌/↻/✗ glyphs"
```

---

### Task 9: bar — nav cursor, dwell-to-open, effect execution

**Files:**
- Modify: `crates/clave-bar/src/model.rs` (cursor/gen, nav dormant branch, `click`, `dwell_expired`, `open_effects`, new Effects)
- Modify: `crates/clave-bar/src/main.rs` (execute `ArmDwell`/`ArmPeek`/`OpenAgent`; Timer disambiguation)

**Interfaces:**
- Consumes: Task 8's `RowKey`/`opening`/`is_dormant`; `clave open` CLI (Task 6).
- Produces: `Effect::ArmDwell { gen: u64 }`, `Effect::ArmPeek`, `Effect::OpenAgent { uuid: String }`; `BarModel::dwell_expired(gen: u64) -> Vec<Effect>`; constants `DWELL_SECS: f64 = 0.4`, `PEEK_SINK_SECS: f64 = 0.9` (move the existing literal `0.9` in main.rs to this constant), `TIMER_KIND_CUTOFF_SECS: f64 = 0.65`.

- [ ] **Step 1: Write the failing model tests:**

```rust
#[test]
fn nav_onto_dormant_row_arms_dwell_not_open() {
    // §6.6 C8: stepping onto a dormant row moves a virtual cursor and arms
    // a 0.4s dwell — it must NOT switch tabs, announce, or open.
    let mut m = BarModel::default();
    m.apply_tabs(vec![tab(1, 0, "live", true)]);
    let mut a = agent("u-d", Status::Idle, None);
    a.last_interacted = 999; // dormant row sorts FIRST; live row is line 1
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![a],
        tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
    });
    m.beacon(1);
    let fx = m.nav("{\"dir\":\"next\"}", Some(1)); // from live row 1, wrap → row 0 (dormant)
    assert_eq!(fx, vec![Effect::ArmDwell { gen: 1 }]);
    // Cursor moved; a second step continues FROM the cursor, back to live.
    let fx = m.nav("{\"dir\":\"next\"}", Some(1));
    assert!(fx.contains(&Effect::SwitchTab { position: 0 }));
}

#[test]
fn dwell_expiry_opens_only_if_cursor_still_there() {
    // Walk-through safety: the gen stamps each landing; a stale gen (the
    // cursor moved on) must be a no-op — this is what makes walking the
    // unified list safe.
    let mut m = BarModel::default();
    m.apply_tabs(vec![tab(1, 0, "live", true)]);
    let mut a = agent("u-d", Status::Idle, None);
    a.last_interacted = 999;
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![a],
        tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
    });
    m.beacon(1);
    let fx = m.nav("{\"dir\":\"next\"}", Some(1));
    let Effect::ArmDwell { gen } = fx[0] else { panic!() };
    // Cursor moved away before expiry → stale gen, no open.
    m.nav("{\"dir\":\"next\"}", Some(1));
    assert!(m.dwell_expired(gen).is_empty());
    // Land again and let it expire in place → exactly one open, marked ↻.
    let fx = m.nav("{\"dir\":\"prev\"}", Some(1)); // back to dormant row 0
    let Effect::ArmDwell { gen } = fx[0] else { panic!() };
    assert_eq!(
        m.dwell_expired(gen),
        vec![Effect::OpenAgent { uuid: "u-d".into() }]
    );
    // In flight now: a repeat expiry (or landing) must not double-fire.
    assert!(m.dwell_expired(gen).is_empty());
}

#[test]
fn explicit_picks_open_immediately() {
    // Click and Alt+N skip the dwell — explicit intent is unambiguous.
    let mut m = BarModel::default();
    m.apply_tabs(vec![tab(1, 0, "live", true)]);
    let mut a = agent("u-d", Status::Idle, None);
    a.last_interacted = 999;
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![a],
        tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
    });
    assert_eq!(
        m.click(0), // dormant row is line 0
        vec![Effect::OpenAgent { uuid: "u-d".into() }]
    );
    // Alt+1 (row payload) on a dormant row — new model, fresh state:
    let mut m = BarModel::default();
    m.apply_tabs(vec![tab(1, 0, "live", true)]);
    let mut a = agent("u-d", Status::Idle, None);
    a.last_interacted = 999;
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![a],
        tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
    });
    m.beacon(1);
    assert_eq!(
        m.nav("{\"row\":1}", Some(1)),
        vec![Effect::OpenAgent { uuid: "u-d".into() }]
    );
}

#[test]
fn dormant_landing_peeks_a_collapsed_bar() {
    // §6.6: walking dormant rows must keep a collapsed bar peeked, same as
    // live-row nav (whose peek rides the visited pipe — dormant landings
    // have no pipe, so the model returns ArmPeek explicitly).
    let mut m = BarModel::default();
    m.toggle(); // collapsed
    m.apply_tabs(vec![tab(1, 0, "live", true)]);
    let mut a = agent("u-d", Status::Idle, None);
    a.last_interacted = 999;
    m.apply_snapshot(AgentSnapshot {
        seq: 1,
        agents: vec![a],
        tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
    });
    m.beacon(1);
    let fx = m.nav("{\"dir\":\"next\"}", Some(1));
    assert!(fx.contains(&Effect::ArmPeek));
    assert!(fx.iter().any(|e| matches!(e, Effect::ArmDwell { .. })));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave-bar dwell && cargo test -p clave-bar explicit_picks`
Expected: FAIL — Effects not defined.

- [ ] **Step 3: Implement in `model.rs`.** New Effects (extend the enum + doc comments in the existing style):

```rust
/// set_timeout(DWELL_SECS) on the executor — §6.6 dormant dwell. `gen`
/// stamps the landing; expiry acts only if the cursor generation still
/// matches (walk-through safety).
ArmDwell { gen: u64 },
/// set_timeout(PEEK_SINK_SECS) + pending_peeks bump — a dormant-row nav
/// landing on a collapsed bar peeks like live nav does (no visited pipe
/// exists for it, so the model asks explicitly).
ArmPeek,
/// run_command(["clave","open",uuid]) — §6.3. Fired by dwell expiry and
/// explicit picks; the model has already marked the uuid in-flight (↻).
OpenAgent { uuid: String },
```

`BarModel` fields:

```rust
/// §6.6 C8 virtual selection cursor: Some(uuid) while nav sits on a
/// dormant row (there is no tab to focus). Nav steps continue from it;
/// it resolves back to the focused-tab row on any live-row landing.
cursor: Option<String>,
/// Bumped on EVERY nav landing; ArmDwell carries it so a late timer for
/// an abandoned landing is provably stale.
cursor_gen: u64,
```

Model constants + helper + dwell:

```rust
/// §6.6 C8: user-tuned (approved 2026-07-17) — do not normalize with the
/// 0.9s peek sink.
pub const DWELL_SECS: f64 = 0.4;
pub const PEEK_SINK_SECS: f64 = 0.9;
/// Event::Timer(f64) carries ELAPSED sleep seconds (server-side, v0.44.3
/// zellij_exports.rs:2462) ≈ the requested duration — 0.4 vs 0.9 splits
/// cleanly at 0.65.
pub const TIMER_KIND_CUTOFF_SECS: f64 = 0.65;

/// Explicit-open path (click / Alt+N / dwell expiry): mark in-flight and
/// emit the run. The `opening` guard is double-fire protection #1
/// (clave open's liveness no-op is #2). Stale rows may retry — the user
/// might have restored the dir.
fn open_effects(&mut self, uuid: &str) -> Vec<Effect> {
    if self.opening.contains(uuid) {
        return Vec::new();
    }
    self.opening.insert(uuid.to_string());
    vec![Effect::OpenAgent {
        uuid: uuid.to_string(),
    }]
}

/// The dwell timer for landing `gen` expired (main.rs). Opens iff the
/// cursor still sits on that same landing and the row is still dormant.
pub fn dwell_expired(&mut self, gen: u64) -> Vec<Effect> {
    if gen != self.cursor_gen {
        return Vec::new(); // cursor moved on — walk-through, not intent
    }
    let Some(uuid) = self.cursor.clone() else {
        return Vec::new();
    };
    let still_dormant = self
        .agents
        .iter()
        .find(|a| a.uuid == uuid)
        .is_some_and(|a| self.is_dormant(a));
    if !still_dormant {
        return Vec::new();
    }
    self.open_effects(&uuid)
}
```

Rework `nav()`'s row/dir resolution (replace from `let line = ...` to the end):

```rust
    let rows = self.rows();
    let line = if let Some(n) = v.get("row").and_then(|n| n.as_u64()) {
        (n as usize).checked_sub(1) // 1-based → display line
    } else if let Some(dir) = v.get("dir").and_then(|d| d.as_str()) {
        if rows.is_empty() {
            return Vec::new();
        }
        // Walk base: the dormant cursor if set, else the executor's own
        // tab row (§6.6 C8 — the cursor IS the position while walking
        // through dormant rows).
        let cur = self
            .cursor
            .as_ref()
            .and_then(|u| {
                rows.iter()
                    .position(|r| r.key == RowKey::Dormant(u.clone()))
            })
            .or_else(|| rows.iter().position(|r| r.key == RowKey::Tab(own)))
            .unwrap_or(0);
        match dir {
            "next" => Some((cur + 1) % rows.len()),
            "prev" => Some((cur + rows.len() - 1) % rows.len()),
            _ => None,
        }
    } else {
        None
    };
    let is_dir_walk = v.get("dir").is_some();
    let Some(row) = line.and_then(|l| rows.get(l).cloned()) else {
        return Vec::new();
    };
    self.cursor_gen += 1; // every landing invalidates prior dwell arms
    match row.key {
        RowKey::Tab(tab_id) => {
            self.cursor = None; // live landing: focus truth takes over
            let Some(position) = self.tabs.iter().find(|t| t.tab_id == tab_id).map(|t| t.position)
            else {
                return Vec::new();
            };
            self.beacon(tab_id); // executor hand-off hint; pipe echo confirms
            vec![
                Effect::SwitchTab { position },
                Effect::AnnounceVisit { tab_id },
            ]
        }
        RowKey::Dormant(uuid) => {
            if !is_dir_walk {
                // Alt+N explicit pick: open immediately (§6.6).
                return self.open_effects(&uuid);
            }
            self.cursor = Some(uuid);
            let mut fx = vec![Effect::ArmDwell {
                gen: self.cursor_gen,
            }];
            // A collapsed bar peeks while walking dormant rows too — live
            // nav peeks via the visited pipe; there is no pipe here, so
            // arm locally on the executor (the one visible bar).
            if self.collapsed {
                self.peeking = true;
                self.seek_budget = SEEK_BUDGET;
                self.seek_last_cols = None;
                fx.push(Effect::ArmPeek);
            }
            fx
        }
    }
}
```

`click()` gains the dormant branch (replace the body):

```rust
pub fn click(&mut self, line: usize) -> Vec<Effect> {
    let Some(row) = self.rows().get(line).cloned() else {
        return Vec::new();
    };
    match row.key {
        RowKey::Tab(tab_id) => {
            let Some(position) = self.tabs.iter().find(|t| t.tab_id == tab_id).map(|t| t.position)
            else {
                return Vec::new();
            };
            self.beacon(tab_id);
            vec![
                Effect::SwitchTab { position },
                Effect::AnnounceVisit { tab_id },
            ]
        }
        // Explicit pick: open immediately (§6.6 — no dwell for clicks).
        RowKey::Dormant(uuid) => self.open_effects(&uuid),
    }
}
```

(`tab_for_line` is now unused — delete it.)

- [ ] **Step 4: Wire main.rs.** In `run_effects`, add arms (OpenAgent ungated — click reaches exactly one instance, nav effects are executor-only by construction, and the model's `opening` guard + clave open's no-op make duplicates harmless):

```rust
Effect::ArmDwell { gen } => {
    self.pending_dwells.push_back(gen);
    set_timeout(clave_bar::model::DWELL_SECS);
}
Effect::ArmPeek => {
    self.pending_peeks += 1;
    set_timeout(clave_bar::model::PEEK_SINK_SECS);
}
Effect::OpenAgent { uuid } => {
    run_command(&["clave", "open", &uuid], BTreeMap::new());
}
```

`State` gains `pending_dwells: std::collections::VecDeque<u64>` (all dwell timers share one duration, so they fire in arm order — FIFO gen matching is exact). Replace the `Event::Timer(_)` arm:

```rust
Event::Timer(elapsed) => {
    // TWO timer kinds share this event; Timer carries the ELAPSED sleep
    // (≈ requested duration, v0.44.3 zellij_exports.rs:2462) — 0.4s
    // dwells and 0.9s peek sinks split cleanly at the cutoff.
    if elapsed < clave_bar::model::TIMER_KIND_CUTOFF_SECS {
        let Some(gen) = self.pending_dwells.pop_front() else {
            return false;
        };
        let fx = self.model.dwell_expired(gen);
        let fired = !fx.is_empty();
        self.run_effects(fx);
        fired // repaint: the row flips to ↻
    } else {
        // One expiry per armed peek; only the LAST sinks (nav burst =
        // one visible expand, one sink). peek_expired() is false when a
        // toggle already cancelled the peek — no repaint.
        self.pending_peeks = self.pending_peeks.saturating_sub(1);
        self.pending_peeks == 0 && self.model.peek_expired()
    }
}
```

Also replace the existing `set_timeout(0.9)` in the `clave-visited` pipe arm with `set_timeout(clave_bar::model::PEEK_SINK_SECS)` (the constant now lives in the model — keep the user-tuned comment).

- [ ] **Step 5: Run all tests + wasm build check**

Run: `cargo test --workspace && cargo build -p clave-bar --target wasm32-wasip1 --release`
Expected: PASS + clean build.

- [ ] **Step 6: Ask Ollie, then commit**

```bash
git add crates/clave-bar/src/model.rs crates/clave-bar/src/main.rs
git commit -m "feat(clave-bar): §6.6 dwell-to-open nav — cursor, gen-stamped 0.4s dwell, explicit picks immediate"
```

---

### Task 10: `clave dev` harness

**Files:**
- Create: `crates/clave/src/dev.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod dev;`)
- Modify: `crates/clave/src/main.rs` (CLI arm)

**Interfaces:**
- Consumes: everything above via env overrides; `setup::run_setup` (regenerates config/layout into the sandbox data dir once env is set), `store::with_store_mut`, `add::tab_layout` machinery indirectly via `clave open` at validation time.
- Produces (spec §6.9): CLI `clave dev scenario <name>`, `clave dev status`, `clave dev reset`. Sandbox root `~/.local/state/clave-dev/` with `state/`, `data/`, `claude/`, `repos/`. Deterministic uuids `00000000-0000-4000-8000-c85c0000000N`. Session `clave-test`.

- [ ] **Step 1: Write the failing tests** in `dev.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_table_covers_the_c8_checklist() {
        // Names map 1:1 to SUBSYSTEM-VALIDATION.md C8 steps.
        let names: Vec<&str> = SCENARIOS.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["c8-cold-start", "c8-worktree", "c8-stale"]);
        // cold-start: 3 agents, staggered recency, none worktree.
        let cs = &SCENARIOS[0];
        assert_eq!(cs.agents.len(), 3);
        assert!(cs.agents.iter().all(|a| !a.worktree && !a.delete_cwd_after));
        // worktree: exactly one worktree agent.
        assert!(SCENARIOS[1].agents.iter().any(|a| a.worktree));
        // stale: exactly one agent whose cwd the scenario deletes.
        assert!(SCENARIOS[2].agents.iter().any(|a| a.delete_cwd_after));
    }

    #[test]
    fn scenario_uuids_are_valid_deterministic_and_readable() {
        // `claude --session-id` requires a real UUID; c85c ≈ "c8 scenario"
        // makes them self-identifying in clave.log / dump-layout.
        let u = scenario_uuid(1);
        assert_eq!(u, "00000000-0000-4000-8000-c85c00000001");
        assert!(uuid::Uuid::parse_str(&u).is_ok());
        assert_ne!(scenario_uuid(2), u);
    }

    #[test]
    fn launch_command_is_fully_env_prefixed() {
        // The ONE command printed for Ollie: everything sandboxed, nothing
        // ambient (§6.9 — his real session/store/~/.claude untouchable).
        let cmd = launch_command(std::path::Path::new("/sb"));
        assert!(cmd.contains("CLAVE_SESSION=clave-test"));
        assert!(cmd.contains("CLAVE_STATE_DIR=/sb/state"));
        assert!(cmd.contains("CLAVE_DATA_DIR=/sb/data"));
        assert!(cmd.contains("CLAUDE_CONFIG_DIR=/sb/claude"));
        assert!(cmd.trim_end().ends_with("clave"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave dev::`
Expected: FAIL — module not defined.

- [ ] **Step 3: Implement `dev.rs`:**

```rust
//! `clave dev` (§6.9): the sandboxed live-validation harness. One command
//! seeds a named, repeatable world state; Ollie drives the checklist in a
//! real `clave-test` session; Claude reads clave.log + `dev status`.
//! Real tabs, real spawns, real jsonls — only the conversation CONTENT is
//! trivial (`claude -p` one-liners). Deliberately minimal: a fixture
//! seeder plus a log — no recorder, no assertion runner, no CI.
//!
//! Session lifecycle stays Ollie's: this module NEVER launches or kills
//! zellij sessions — it prints the commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

pub struct ScenarioAgent {
    pub slug: &'static str,
    /// Seconds before "now" for last_interacted — staggers recency so the
    /// eager-load / dormant-order expectations are deterministic.
    pub ago_secs: u64,
    pub worktree: bool,
    /// c8-stale: delete the agent's cwd AFTER seeding its jsonl, so the
    /// row's dwell-open hits the §6.3 staleness branch.
    pub delete_cwd_after: bool,
}

pub struct Scenario {
    pub name: &'static str,
    pub agents: &'static [ScenarioAgent],
}

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "c8-cold-start",
        agents: &[
            ScenarioAgent { slug: "recent", ago_secs: 60, worktree: false, delete_cwd_after: false },
            ScenarioAgent { slug: "mid", ago_secs: 3_600, worktree: false, delete_cwd_after: false },
            ScenarioAgent { slug: "old", ago_secs: 86_400, worktree: false, delete_cwd_after: false },
        ],
    },
    Scenario {
        name: "c8-worktree",
        agents: &[
            ScenarioAgent { slug: "main", ago_secs: 60, worktree: false, delete_cwd_after: false },
            ScenarioAgent { slug: "wt", ago_secs: 3_600, worktree: true, delete_cwd_after: false },
        ],
    },
    Scenario {
        name: "c8-stale",
        agents: &[
            ScenarioAgent { slug: "alive", ago_secs: 60, worktree: false, delete_cwd_after: false },
            ScenarioAgent { slug: "gone", ago_secs: 3_600, worktree: false, delete_cwd_after: true },
        ],
    },
];

/// Valid v4-shaped, deterministic, self-identifying (`c85c` ≈ c8 scenario).
pub fn scenario_uuid(n: u32) -> String {
    format!("00000000-0000-4000-8000-c85c{n:08}")
}

pub fn sandbox_root() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("home")?
        .join(".local/state/clave-dev"))
}

/// The ONE command printed for Ollie to launch the sandboxed session.
pub fn launch_command(root: &Path) -> String {
    format!(
        "CLAVE_SESSION=clave-test CLAVE_STATE_DIR={0}/state CLAVE_DATA_DIR={0}/data CLAUDE_CONFIG_DIR={0}/claude clave",
        root.display()
    )
}

/// Point THIS process at the sandbox (children inherit — claude -p seeding
/// and run_setup both land inside).
fn enter_sandbox(root: &Path) {
    // SAFETY: single-threaded CLI entry point; set before any spawn.
    unsafe {
        std::env::set_var("CLAVE_SESSION", "clave-test");
        std::env::set_var("CLAVE_STATE_DIR", root.join("state"));
        std::env::set_var("CLAVE_DATA_DIR", root.join("data"));
        std::env::set_var("CLAUDE_CONFIG_DIR", root.join("claude"));
    }
}

pub fn run_scenario(name: &str) -> Result<()> {
    let sc = SCENARIOS
        .iter()
        .find(|s| s.name == name)
        .with_context(|| {
            let names: Vec<_> = SCENARIOS.iter().map(|s| s.name).collect();
            format!("unknown scenario {name}; have: {names:?}")
        })?;
    let root = sandbox_root()?;
    enter_sandbox(&root);
    for d in ["state", "data", "claude", "repos"] {
        std::fs::create_dir_all(root.join(d))?;
    }
    // Sandbox claude identity: onboarding/account state (~/.claude.json →
    // $CLAUDE_CONFIG_DIR/.claude.json). OAuth creds live in the macOS
    // Keychain, which is machine-ambient — headless `claude -p` works.
    let home = dirs::home_dir().context("home")?;
    let sandbox_cfg = root.join("claude/.claude.json");
    if !sandbox_cfg.exists() && home.join(".claude.json").exists() {
        std::fs::copy(home.join(".claude.json"), &sandbox_cfg)?;
    }
    // Sandbox hooks + config/layout + wasm: reuse the REAL wasm, then run
    // the normal setup against the sandbox dirs (env already points there).
    let real_wasm = home.join(".local/share/clave/clave-bar.wasm");
    std::fs::copy(&real_wasm, root.join("data/clave-bar.wasm"))
        .context("copy clave-bar.wasm (run the real `clave setup` first)")?;
    crate::setup::run_setup()?;

    let now = crate::store::now_unix();
    let paths = crate::store::store_paths()?;
    for (i, a) in sc.agents.iter().enumerate() {
        let uuid = scenario_uuid(i as u32 + 1);
        let repo = root.join("repos").join(format!("{name}-{}", a.slug));
        std::fs::create_dir_all(&repo)?;
        run_in(&repo, "git", &["init", "-q"])?;
        run_in(&repo, "git", &["commit", "--allow-empty", "-q", "-m", "seed"])?;
        let cwd = if a.worktree {
            let wt = repo.join(".claude-worktrees").join(&uuid[..8]);
            run_in(
                &repo,
                "git",
                &["worktree", "add", "-q", "-b", &format!("clave/{}", &uuid[..8]), wt.to_str().context("wt")?],
            )?;
            wt
        } else {
            repo.clone()
        };
        let cwd = std::fs::canonicalize(&cwd)?; // S0b: claude munges getcwd()
        // A REAL resumable jsonl for a few tokens (§6.9): resume-with-
        // history is verified for real, not mocked.
        println!("seeding {uuid} ({})…", a.slug);
        let st = Command::new("claude")
            .current_dir(&cwd)
            .args(["-p", "--session-id", &uuid, "Reply with exactly: ok"])
            .status()
            .context("running claude -p (is claude on PATH?)")?;
        anyhow::ensure!(st.success(), "claude -p seeding failed for {uuid}");
        let cwd_str = cwd.to_str().context("cwd utf8")?.to_string();
        crate::store::with_store_mut(&paths, |s| {
            s.agents.insert(
                uuid.clone(),
                crate::store::AgentRecord {
                    uuid: uuid.clone(),
                    cwd: cwd_str.clone(),
                    repo_root: repo.to_string_lossy().into_owned(),
                    branch: if a.worktree { format!("clave/{}", &uuid[..8]) } else { "main".into() },
                    label: format!("{}-{} · seeded", name, a.slug),
                    status: clave_types::Status::Idle,
                    last_interacted: now.saturating_sub(a.ago_secs),
                    last_visited: 0,
                    worktree: a.worktree.then(|| cwd_str.clone()),
                    label_source: crate::store::LabelSource::FirstPrompt,
                    tab_id: None,
                    stale: false,
                },
            );
            s.seq += 1;
        })?;
        if a.delete_cwd_after {
            std::fs::remove_dir_all(&cwd)?; // the §6.3 staleness fixture
        }
    }
    crate::evlog::log_event("dev", &format!("scenario {name} seeded"));
    println!("\nScenario `{name}` ready. Launch (your command, your pane):\n");
    println!("  {}", launch_command(&root));
    println!("\nWhen done: `clave dev reset` (prints the kill command first).");
    Ok(())
}

pub fn run_status() -> Result<()> {
    let root = sandbox_root()?;
    enter_sandbox(&root);
    let store = crate::store::read_store(&crate::store::store_paths()?)?;
    let list = Command::new("zellij")
        .args(["list-sessions", "-n"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let live_session = crate::setup::session_is_live(&list, "clave-test");
    // Sanctioned §6.9 read: explicitly clave-test-scoped.
    let dump = Command::new("zellij")
        .env("ZELLIJ_SESSION_NAME", "clave-test")
        .args(["action", "dump-layout"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    println!(
        "{}",
        serde_json::json!({
            "session_live": live_session,
            "live_uuids": crate::add::live_uuids(&dump),
            "store": store,
        })
    );
    Ok(())
}

pub fn run_reset() -> Result<()> {
    let root = sandbox_root()?;
    println!("If the session is running, kill it first (your command):\n");
    println!("  zellij kill-session clave-test && zellij delete-session --force clave-test\n");
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
        println!("Sandbox wiped: {}", root.display());
    } else {
        println!("Sandbox already clean: {}", root.display());
    }
    Ok(())
}

fn run_in(dir: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    let st = Command::new(cmd)
        .current_dir(dir)
        .args(args)
        .status()
        .with_context(|| format!("running {cmd}"))?;
    anyhow::ensure!(st.success(), "{cmd} {args:?} failed in {}", dir.display());
    Ok(())
}
```

- [ ] **Step 4: CLI arm.** `main.rs`:

```rust
/// Live-validation harness (§6.9): seed sandboxed scenarios, dump status,
/// reset. The sandbox (session `clave-test`, own store/data/claude dirs)
/// can never touch the real session or ~/.claude.
Dev {
    #[command(subcommand)]
    action: DevAction,
},
```

```rust
#[derive(Subcommand)]
enum DevAction {
    /// Seed a named scenario and print the launch command.
    Scenario { name: String },
    /// Dump sandbox store + live uuids + session liveness as JSON.
    Status,
    /// Wipe the sandbox (prints the kill-session command first).
    Reset,
}
```

Match arm:

```rust
Some(Command::Dev { action }) => match action {
    DevAction::Scenario { name } => dev::run_scenario(&name),
    DevAction::Status => dev::run_status(),
    DevAction::Reset => dev::run_reset(),
},
```

- [ ] **Step 5: Run tests + build**

Run: `cargo test -p clave && cargo build -p clave`
Expected: PASS.

- [ ] **Step 6: Ask Ollie, then commit**

```bash
git add crates/clave/src/dev.rs crates/clave/src/lib.rs crates/clave/src/main.rs
git commit -m "feat(clave): §6.9 clave dev — sandboxed scenarios, status dump, reset"
```

---

### Task 11: final verification + install

- [ ] **Step 1: Full suite + lints**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: tests PASS; clippy shows ONLY the 4 pre-existing parked lints (add.rs, store.rs ×2, lsview.rs).

- [ ] **Step 2: Build both artifacts**

Run: `CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S) cargo build -p clave-bar --target wasm32-wasip1 --release && cargo install --path crates/clave --locked --force`
Expected: clean builds. Copy the wasm: `cp target/wasm32-wasip1/release/clave-bar.wasm ~/.local/share/clave/`.

- [ ] **Step 3: Regenerate real config** — ask Ollie to run `clave setup` (or run it: it only writes `~/.local/share/clave/` + additive merges) so the real session's config gains `session_serialization false`.

- [ ] **Step 4: Hand off to live validation.** Seed `clave dev scenario c8-cold-start`; Ollie drives the C8 checklist (`docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` C8 section); Claude reads `clave dev status` + `~/.local/state/clave-dev/state/clave.log` + the zellij log between steps. Record the verdict in the validation log.

- [ ] **Step 5: Ask Ollie, then commit** any remaining changes and update the validation log verdict when C8 passes.

---

## Self-Review (completed at plan time)

- **Spec coverage:** §6.8 serialization off (T4) + dynamic launch/delete (T5); §6.3 `clave open` (T6); §5 stale (T2); §6.6 dormant rows (T8) + dwell/cursor/explicit picks/ArmPeek (T9); §6.9 env overrides (T1), evlog (T3), CLAUDE_CONFIG_DIR (T7), scenarios/status/reset (T10). Invariant #9 subcommand list already updated in the spec commit.
- **Known deliberate gaps** (spec-sanctioned): nav ring caps (deferred to adoption, §10); stale-row recovery UX (manual, §6.3); non-macOS keychain auth for `claude -p` seeding (harness is this-machine-only).
- **Type consistency check:** `RowKey`/`Row.key` (T8) consumed by T9; `apply_open_result` (T2) consumed by T6; `tab_node`/`sanitize_label` pub (T5) consumed by T6; `session_exists` + `session_is_live` both used in T5/T10; `DWELL_SECS`/`PEEK_SINK_SECS`/`TIMER_KIND_CUTOFF_SECS` defined T9 model, used T9 main.
