# Claude-Codex Launch Flag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `clave add --codex` as a persistent Claude Code launch choice while retaining Claude's existing session store, hooks, UUIDs, resume behavior, and worktree scoping.

**Architecture:** Persist one host-only `claude_codex: bool` on each agent row. Bake that immutable choice into the existing internal `clave spawn` KDL command, then resolve and direct-exec either ordinary Claude or a real `claude-codex` wrapper at the final spawn boundary. Reuse current executable discovery and preflight infrastructure; clave never handles proxy credentials or protocol translation.

**Tech Stack:** Rust 2021, clap derive, serde/serde_json, tempfile, KDL generation, Unix `CommandExt::exec`, existing clave discovery/doctor/store modules.

## Global Constraints

- Work only in the isolated `worktree-claude-codex-profile` worktree.
- Rebase onto `main` at `50fa26a` or later before changing source.
- TDD: write each failing test first, run it and observe failure, then implement the minimum code.
- Always use `cargo test --workspace` for the final test gate; bare `cargo test` skips the wasm crate.
- Never write under the real `~/.claude/`; integration tests use temporary directories and fake executables.
- Never launch, kill, or drive a Zellij session; live validation belongs to Ollie.
- Never run `just dev-install`, `cargo install`, or `just release` from this working session.
- Do not add provider abstractions, a `LaunchProfile` enum, Codex-store support, a bar marker, or `clave-types` fields.
- Do not move the existing final spawn exec out of `main.rs` as part of this feature.
- Do not add `claude-codex` to ordinary doctor facts/catalogue; it is optional until a stored/requested launch requires it.
- Do not silently fall back from Codex-backed inference to ordinary Claude.
- Do not commit without Ollie's explicit approval. Commit checkpoints below are prepared only; Ollie signs them.

---

## File Structure

### Behavioral changes

- `crates/clave/src/main.rs` — user `--codex`, hidden internal `--claude-codex`, dispatch, and final executable selection.
- `crates/clave/src/store.rs` — backward-compatible host-only `claude_codex` field.
- `crates/clave/src/add.rs` — KDL snapshot flag, requested-profile persistence, resume merge, and add preflight placement.
- `crates/clave/src/open.rs` — dormant-open profile preflight and row-derived KDL.
- `crates/clave/src/setup.rs` — cold-start eager-row snapshot/preflight ordering and row-derived KDL.
- `crates/clave/src/discover.rs` — optional wrapper executable discovery.
- `crates/clave/src/doctor.rs` — centralized wrapper remediation copy.
- `crates/clave/tests/spawn_launch.rs` — fake-executable create/resume integration matrix.

### Mechanical `AgentRecord` literal updates only

- `crates/clave/src/dev.rs`
- `crates/clave/src/hook.rs`
- `crates/clave/src/lsview.rs`
- `crates/clave/tests/kdl_guardrail.rs`

### Explicitly unchanged

- `crates/clave-types/**`
- `crates/clave-bar/**`
- Claude JSONL discovery and munging rules
- Claude hooks and hook payload handling
- `spawn::register_pane` and its required `/bin/sh` double-fork

---

### Task 0: Synchronize the worktree with current main

**Files:**
- Preserve: `docs/superpowers/specs/2026-07-22-claude-codex-launch-profile-design.md`
- Create later: `docs/superpowers/plans/2026-07-22-claude-codex-launch-flag.md`

**Interfaces:**
- Consumes: local `main`/`origin/main` at `50fa26a` or later.
- Produces: worktree source containing the merged doctor/install discovery infrastructure.

- [ ] **Step 1: Confirm only planning documents are untracked**

Run:

```bash
git status --short --branch
```

Expected: branch `worktree-claude-codex-profile`; no modified tracked source; only the approved spec and this plan may be untracked.

- [ ] **Step 2: Rebase the worktree onto current main**

Run:

```bash
git rebase main
```

Expected: successful fast-forward/rebase onto `50fa26a` or later. If any tracked conflict appears, stop and report it rather than resolving by assumption.

- [ ] **Step 3: Verify the merged infrastructure exists**

Run:

```bash
git log -1 --oneline
rg -n 'pub enum ToolId|pub fn preflight' crates/clave/src/discover.rs crates/clave/src/doctor.rs
```

Expected: HEAD contains the doctor/install merge; `ToolId` and `doctor::preflight` are present.

- [ ] **Step 4: Establish a green baseline**

Run:

```bash
cargo test --workspace
```

Expected: PASS before feature changes. If baseline fails, stop; do not attribute existing failures to this feature.

No commit checkpoint: synchronization changes no project content.

---

### Task 1: Add the CLI flags and backward-compatible store field

**Files:**
- Modify: `crates/clave/src/main.rs:33-57,213-262`
- Modify: `crates/clave/src/store.rs:35-69,371-419`
- Modify mechanically: `crates/clave/src/add.rs`, `dev.rs`, `hook.rs`, `lsview.rs`, `open.rs`, `setup.rs`
- Modify mechanically: `crates/clave/tests/kdl_guardrail.rs`

**Interfaces:**
- Consumes: existing `Command::{Add,Spawn}` and `AgentRecord`.
- Produces:
  - `Command::Add { worktree: bool, codex: bool }`
  - `Command::Spawn { uuid, name, cwd, claude_codex: bool }`
  - `AgentRecord::claude_codex: bool`, defaulting to `false` on old JSON.

- [ ] **Step 1: Write the failing CLI parse regression**

Add inside `main.rs`'s existing `#[cfg(test)]` module:

```rust
#[test]
fn claude_codex_cli_flags_parse_only_on_add_and_spawn() {
    let plain = Cli::try_parse_from(["clave", "add"]).unwrap();
    assert!(matches!(
        plain.command,
        Some(Command::Add {
            worktree: false,
            codex: false
        })
    ));

    let codex = Cli::try_parse_from(["clave", "add", "--codex"]).unwrap();
    assert!(matches!(
        codex.command,
        Some(Command::Add {
            worktree: false,
            codex: true
        })
    ));

    let worktree =
        Cli::try_parse_from(["clave", "add", "--worktree", "--codex"]).unwrap();
    assert!(matches!(
        worktree.command,
        Some(Command::Add {
            worktree: true,
            codex: true
        })
    ));

    let spawn = Cli::try_parse_from([
        "clave",
        "spawn",
        "u",
        "--name",
        "n",
        "--cwd",
        "/x",
        "--claude-codex",
    ])
    .unwrap();
    assert!(matches!(
        spawn.command,
        Some(Command::Spawn {
            claude_codex: true,
            ..
        })
    ));

    assert!(Cli::try_parse_from(["clave", "open", "u", "--codex"]).is_err());
}
```

- [ ] **Step 2: Run the CLI test and observe failure**

Run:

```bash
cargo test -p clave claude_codex_cli_flags_parse_only_on_add_and_spawn
```

Expected: FAIL to compile because `codex` and `claude_codex` do not exist.

- [ ] **Step 3: Write the failing store compatibility tests**

Add to `store.rs` tests, after adding `claude_codex: false` to the `rec` fixture only so the test module compiles:

```rust
#[test]
fn agent_record_claude_codex_defaults_false_for_old_json() {
    let mut value = serde_json::to_value(rec("u1")).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("claude_codex");

    let decoded: AgentRecord = serde_json::from_value(value).unwrap();
    assert!(!decoded.claude_codex);
}

#[test]
fn agent_record_claude_codex_roundtrips_both_values() {
    for expected in [false, true] {
        let mut row = rec("u1");
        row.claude_codex = expected;
        let decoded: AgentRecord =
            serde_json::from_str(&serde_json::to_string(&row).unwrap()).unwrap();
        assert_eq!(decoded.claude_codex, expected);
    }
}
```

- [ ] **Step 4: Run the store tests and observe failure**

Run:

```bash
cargo test -p clave agent_record_claude_codex
```

Expected: FAIL because `AgentRecord` has no `claude_codex` field.

- [ ] **Step 5: Add the minimal CLI and store implementation**

Change the enum fields in `main.rs`:

```rust
Add {
    #[arg(long)]
    worktree: bool,
    /// Launch Claude Code through the external claude-codex wrapper.
    #[arg(long)]
    codex: bool,
},

Spawn {
    uuid: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    cwd: String,
    /// Internal immutable launch snapshot baked into generated KDL.
    #[arg(long, hide = true)]
    claude_codex: bool,
},
```

Update only the dispatch patterns; leave each arm's current statements below the match line unchanged until later tasks:

```rust
Some(Command::Add { worktree, codex }) => add::run_add(worktree, codex),
Some(Command::Spawn {
    uuid,
    name,
    cwd,
    claude_codex: _,
}) => {
```

Change the `run_add` signature while deliberately marking the not-yet-used argument:

```rust
pub fn run_add(worktree: bool, _claude_codex: bool) -> Result<()> {
```

Task 3 renames `_claude_codex` to `claude_codex` when it first affects KDL and persistence.

Add to `AgentRecord` after `label_source`:

```rust
/// True when this Claude session should launch through the external
/// claude-codex wrapper. Host-only: the bar never needs this choice.
#[serde(default)]
pub claude_codex: bool,
```

Add `claude_codex: false` to every `AgentRecord` literal listed in the file map. Do not add it to `snapshot_from`.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p clave claude_codex_cli_flags_parse_only_on_add_and_spawn
cargo test -p clave agent_record_claude_codex
cargo test -p clave
```

Expected: PASS.

- [ ] **Step 7: Prepare the commit checkpoint—do not execute without explicit approval**

```bash
git add crates/clave/src/main.rs crates/clave/src/store.rs \
  crates/clave/src/add.rs crates/clave/src/dev.rs crates/clave/src/hook.rs \
  crates/clave/src/lsview.rs crates/clave/src/open.rs crates/clave/src/setup.rs \
  crates/clave/tests/kdl_guardrail.rs
git commit -m "feat(clave): persist claude-codex launch choice"
```

Expected when authorized: maintainer-signed commit. Until then, leave changes uncommitted.

---

### Task 2: Add optional wrapper discovery and centralized remediation

**Files:**
- Modify: `crates/clave/src/discover.rs:13-45,90-125`
- Modify: `crates/clave/src/doctor.rs:109-150,492-513`

**Interfaces:**
- Consumes: `discover(ToolId)`, `doctor::missing_advice`, `doctor::preflight`.
- Produces: `ToolId::ClaudeCodex`, binary `claude-codex`, override `CLAVE_CLAUDE_CODEX_BIN`, and optional profile remediation.

- [ ] **Step 1: Write failing discovery tests**

Extend the existing discovery test module with:

```rust
#[test]
fn claude_codex_discovery_uses_shared_locations_only() {
    assert_eq!(ToolId::ClaudeCodex.bin_name(), "claude-codex");
    assert_eq!(
        ToolId::ClaudeCodex.override_var(),
        "CLAVE_CLAUDE_CODEX_BIN"
    );

    let home = std::path::Path::new("/home/test");
    let dirs = candidate_dirs(ToolId::ClaudeCodex, home, &["v22.1.0".into()]);
    assert_eq!(
        dirs,
        vec![
            home.join(".local/bin"),
            std::path::PathBuf::from("/opt/homebrew/bin"),
            std::path::PathBuf::from("/usr/local/bin"),
            home.join(".cargo/bin"),
        ]
    );
    assert!(!dirs.iter().any(|p| p.starts_with(home.join(".nvm"))));
    assert!(!dirs.contains(&home.join(".claude/local")));
    assert!(!dirs.contains(&home.join(".volta/bin")));
    assert!(!dirs.contains(&home.join(".bun/bin")));
}
```

- [ ] **Step 2: Write failing remediation tests**

Add to `doctor.rs` tests:

```rust
#[test]
fn claude_codex_remediation_is_actionable() {
    let text = missing_advice(ToolId::ClaudeCodex, None).join("\n");
    assert!(text.contains("real executable"));
    assert!(text.contains("CLAVE_CLAUDE_CODEX_BIN"));
    assert!(text.contains("absolute path"));
}

#[test]
fn claude_codex_preflight_uses_central_remediation() {
    let finding = Finding {
        group: Group::RequiredTools,
        severity: Severity::Problem,
        label: "claude-codex not found".into(),
        advice: missing_advice(ToolId::ClaudeCodex, None),
    };
    let rendered = render_failures("clave add --codex can't launch:", &[finding]);
    assert!(rendered.contains("claude-codex not found"));
    assert!(rendered.contains("CLAVE_CLAUDE_CODEX_BIN"));
}
```

Do not add the wrapper to `Facts`, `gather`, or the fixed doctor catalogue.

- [ ] **Step 3: Run tests and observe failure**

Run:

```bash
cargo test -p clave claude_codex_discovery_uses_shared_locations_only
cargo test -p clave claude_codex_remediation
cargo test -p clave claude_codex_preflight
```

Expected: FAIL because `ToolId::ClaudeCodex` does not exist.

- [ ] **Step 4: Add the minimal discovery mapping**

Extend `ToolId` and its exhaustive mappings:

```rust
pub enum ToolId {
    Zellij,
    Claude,
    ClaudeCodex,
    Git,
    Fzf,
    Zoxide,
}
```

```rust
ToolId::ClaudeCodex => "claude-codex",
```

```rust
ToolId::ClaudeCodex => "CLAVE_CLAUDE_CODEX_BIN",
```

Keep candidate directories shared-only:

```rust
ToolId::ClaudeCodex | ToolId::Zellij | ToolId::Git | ToolId::Zoxide => {}
```

The existing `if tool == ToolId::Claude` nvm scan remains unchanged.

- [ ] **Step 5: Add centralized remediation**

Classify profile preflight failures with required tools:

```rust
ToolId::Zellij | ToolId::Claude | ToolId::ClaudeCodex | ToolId::Git => {
    Group::RequiredTools
}
```

Add the dedicated match arm:

```rust
ToolId::ClaudeCodex => vec![
    "Install `claude-codex` as a real executable and put it on PATH.".into(),
    "A shell function cannot be launched by clave.".into(),
    "Or set CLAVE_CLAUDE_CODEX_BIN to its absolute path.".into(),
],
```

Do not add it to ordinary doctor collection.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p clave discover::
cargo test -p clave doctor::
cargo test -p clave
```

Expected: PASS; existing ordinary doctor catalogue tests remain unchanged.

- [ ] **Step 7: Prepare the commit checkpoint—do not execute without explicit approval**

```bash
git add crates/clave/src/discover.rs crates/clave/src/doctor.rs
git commit -m "feat(clave): discover optional claude-codex wrapper"
```

---

### Task 3: Bake the launch choice into KDL and preserve it on resume

**Files:**
- Modify: `crates/clave/src/add.rs:89-165,338-354,713-760`
- Modify: `crates/clave/src/setup.rs:190-206`
- Modify: `crates/clave/tests/kdl_guardrail.rs`

**Interfaces:**
- Consumes: `AgentRecord::claude_codex`, requested `run_add(..., claude_codex)` value.
- Produces:
  - `tab_node(..., claude_codex: bool)`
  - `tab_node_bare(..., claude_codex: bool)`
  - `tab_layout(..., claude_codex: bool)`
  - KDL containing optional `--claude-codex`
  - resume merge that copies only the requested launch choice alongside existing intended resets.

- [ ] **Step 1: Write the failing KDL difference test**

Add to `add.rs` tests:

```rust
#[test]
fn tab_layout_codex_diff_is_only_spawn_flag() {
    let plain = tab_layout("/bin/clave", "/bar.wasm", "label", "u", "/repo", false);
    let codex = tab_layout("/bin/clave", "/bar.wasm", "label", "u", "/repo", true);

    assert!(codex.contains(r#""--claude-codex""#));
    assert_eq!(codex.replace(r#" "--claude-codex""#, ""), plain);
}
```

- [ ] **Step 2: Strengthen the existing resume-merge regression**

In `merge_resume_preserves_existing_row_and_resets_status`, set:

```rust
existing.claude_codex = false;
fresh.claude_codex = true;
```

Then assert:

```rust
assert!(merged.claude_codex);
assert_eq!(merged.status, Status::Idle);
assert_eq!(merged.tab_id, None);
assert_eq!(merged.label, existing.label);
assert_eq!(merged.cwd, existing.cwd);
assert_eq!(merged.repo_root, existing.repo_root);
assert_eq!(merged.branch, existing.branch);
assert_eq!(merged.worktree, existing.worktree);
assert_eq!(merged.last_interacted, existing.last_interacted);
assert_eq!(merged.last_visited, existing.last_visited);
assert_eq!(merged.label_source, existing.label_source);
assert_eq!(merged.stale, existing.stale);
```

Add the reverse-direction assertion explicitly:

```rust
let mut previous_codex = existing.clone();
previous_codex.claude_codex = true;
let mut requested_plain = fresh.clone();
requested_plain.claude_codex = false;
let switched_plain = merge_resume_record(Some(&previous_codex), requested_plain);
assert!(!switched_plain.claude_codex);
assert_eq!(switched_plain.cwd, previous_codex.cwd);
assert_eq!(switched_plain.worktree, previous_codex.worktree);
```

- [ ] **Step 3: Write the failing cold-layout test**

Add to `setup.rs` tests by extending the existing eager-layout fixture directly:

```rust
#[test]
fn launch_layout_eager_derives_claude_codex_from_row() {
    let row = crate::store::AgentRecord {
        uuid: "u-codex".into(),
        cwd: "/repo/.claude-worktrees/codex".into(),
        repo_root: "/repo".into(),
        branch: "codex".into(),
        label: "repo · codex".into(),
        status: clave_types::Status::Idle,
        last_interacted: 100,
        last_visited: 0,
        worktree: Some("/repo/.claude-worktrees/codex".into()),
        label_source: crate::store::LabelSource::FirstPrompt,
        claude_codex: true,
        tab_id: None,
        stale: false,
    };

    let layout = launch_layout_kdl("/bin/clave", "/bar.wasm", Some(&row));
    assert!(layout.contains(r#"args "spawn" "u-codex""#));
    assert!(layout.contains(r#""--claude-codex""#));
    assert!(layout.contains(r#"cwd="/repo/.claude-worktrees/codex""#));
}
```

Add `claude_codex: false` to the existing eager-row literals and closure fixtures elsewhere in `setup.rs`.

- [ ] **Step 4: Run tests and observe failure**

Run:

```bash
cargo test -p clave tab_layout_codex_diff_is_only_spawn_flag
cargo test -p clave merge_resume_preserves_existing_row_and_resets_status
cargo test -p clave launch_layout_eager_derives_claude_codex_from_row
```

Expected: FAIL because KDL builders do not accept or emit the choice, and merge preserves the old row's value.

- [ ] **Step 5: Add one shared KDL argument helper**

Add near `tab_node`:

```rust
fn spawn_args_kdl(uuid: &str, label: &str, cwd: &str, claude_codex: bool) -> String {
    let mut args = format!(
        r#"args "spawn" "{uuid}" "--name" "{label}" "--cwd" "{cwd}""#
    );
    if claude_codex {
        args.push_str(r#" "--claude-codex""#);
    }
    args
}
```

Extend all three builder signatures with `claude_codex: bool`. In `tab_node` and `tab_node_bare`, compute:

```rust
let spawn_args = spawn_args_kdl(uuid, label, cwd, claude_codex);
```

Replace the duplicated literal line with:

```rust
                {spawn_args}
```

Update `tab_layout` to forward the boolean.

- [ ] **Step 6: Implement the exact merge behavior**

Change only the existing-row branch:

```rust
Some(row) => AgentRecord {
    claude_codex: fresh.claude_codex,
    status: clave_types::Status::Idle,
    tab_id: None,
    ..row.clone()
},
```

On new/fresh records in `run_add`, set:

```rust
claude_codex,
```

Pass the requested value to the immediate `tab_layout` call.

- [ ] **Step 7: Thread stored values through open/cold KDL call sites**

In `setup::launch_layout_kdl`, pass:

```rust
r.claude_codex
```

to `tab_node_bare`. In `open`, pass `row.claude_codex` to `tab_layout`; Task 4 adds preflight before this call.

Update all tests/call sites with `false` unless specifically testing Codex. Update the real KDL parser guardrail to parse both variants.

- [ ] **Step 8: Run focused and parser tests**

Run:

```bash
cargo test -p clave tab_layout_codex_diff_is_only_spawn_flag
cargo test -p clave merge_resume_preserves_existing_row_and_resets_status
cargo test -p clave launch_layout_eager_derives_claude_codex_from_row
cargo test -p clave --test kdl_guardrail
cargo test -p clave
```

Expected: PASS.

- [ ] **Step 9: Prepare the commit checkpoint—do not execute without explicit approval**

```bash
git add crates/clave/src/add.rs crates/clave/src/setup.rs \
  crates/clave/src/open.rs crates/clave/tests/kdl_guardrail.rs
git commit -m "feat(clave): bake claude-codex choice into spawn layouts"
```

---

### Task 4: Place profile-specific preflight at every launch seam

**Files:**
- Modify: `crates/clave/src/add.rs:457-760`
- Modify: `crates/clave/src/open.rs:45-135`
- Modify: `crates/clave/src/setup.rs:579-714`

**Interfaces:**
- Consumes: `ToolId::ClaudeCodex`, `doctor::preflight`, requested/stored `claude_codex`.
- Produces: no optional-wrapper check for live jumps/attach; early actionable failure for every new/dormant/cold Codex launch.

- [ ] **Step 1: Run existing focused tests before orchestration edits**

Run:

```bash
cargo test -p clave add::
cargo test -p clave open::
cargo test -p clave setup::
```

Expected: PASS. These are the regression baseline for the ordering-only change.

- [ ] **Step 2: Add requested-profile preflight to `run_add`**

Keep the existing initial required-tool preflight unchanged. Add this exact check only after a resume candidate's live-jump early return and before any dormant tab creation:

```rust
if claude_codex {
    crate::doctor::preflight(
        &[crate::discover::ToolId::ClaudeCodex],
        "clave add --codex can't launch — missing wrapper:",
    )?;
}
```

For the new-agent branch, run the same check before `git worktree add` and before any tab/store mutation. Do not check the wrapper before a live candidate can return through navigation.

- [ ] **Step 3: Add dormant-open preflight**

At the start of `OpenDecision::Open`, before `validate_cwd`, KDL write, or `new-tab`:

```rust
if row.claude_codex {
    crate::doctor::preflight(
        &[
            crate::discover::ToolId::Claude,
            crate::discover::ToolId::ClaudeCodex,
        ],
        "clave open can't launch this Codex-profile agent:",
    )?;
}
```

Do not add checks to `AlreadyLive` or `Stale`.

- [ ] **Step 4: Reorder dead-session cold-start preparation**

After computing `live`, read and clone the eager row once, preserving today's live-attach logging/layout input while giving the dead path one immutable check/use snapshot:

```rust
let store = crate::store::read_store(&crate::store::store_paths()?)?;
let eager = eager_row(&store).cloned();

if !live {
    if let Some(row) = eager.as_ref() {
        crate::add::validate_cwd(&row.cwd)?;
        if row.claude_codex {
            crate::doctor::preflight(
                &[crate::discover::ToolId::ClaudeCodex],
                "clave can't restore the eager Codex-profile agent:",
            )?;
        }
    }
}
```

Move the existing `clear_tab_timeline` and best-effort `delete-session --force` block from current `setup.rs:648-677` immediately after this preflight inside the same `if !live` block, without changing its error/logging behavior.

Only after the dead-path preflight may it clear session-scoped store state, attempt `delete-session`, write the launch layout, or attach/create Zellij. Generate KDL from `eager.as_ref()` for both live and dead paths, preserving current behavior; Zellij ignores `--layout` when attaching live.

For `live == true`, skip only the optional wrapper preflight. The ordinary `[Zellij, Claude]` preflight remains where it is.

- [ ] **Step 5: Run regression tests and inspect the diff for ordering**

Run:

```bash
cargo test -p clave add::
cargo test -p clave open::
cargo test -p clave setup::
cargo test -p clave --test kdl_guardrail
git diff --check
```

Expected: PASS. Review the diff and confirm:

- live add selection returns before wrapper preflight;
- new worktree creation occurs after wrapper preflight;
- dormant open checks only the `Open` arm;
- dead cold start checks the cloned eager row before store/session mutation;
- the same eager snapshot feeds launch KDL.

- [ ] **Step 6: Prepare the commit checkpoint—do not execute without explicit approval**

```bash
git add crates/clave/src/add.rs crates/clave/src/open.rs crates/clave/src/setup.rs
git commit -m "feat(clave): preflight claude-codex launch paths"
```

---

### Task 5: Direct-exec the selected launcher and verify create/resume end to end

**Files:**
- Modify: `crates/clave/src/main.rs:218-261`
- Create: `crates/clave/tests/spawn_launch.rs`

**Interfaces:**
- Consumes: hidden `claude_codex` flag, `discover(ToolId::{Claude,ClaudeCodex})`, `spawn_mode`.
- Produces: absolute-path direct exec of ordinary Claude or wrapper; wrapper receives `CLAVE_CLAUDE_BIN=<absolute ordinary Claude path>`.

- [ ] **Step 1: Create the failing integration test fixture**

Create `crates/clave/tests/spawn_launch.rs`:

```rust
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_capture_executable(path: &Path, launcher: &str) {
    let script = format!(
        "#!/bin/sh\n\
         {{\n\
           printf 'launcher=%s\\n' '{launcher}'\n\
           printf 'cwd=%s\\n' \"$PWD\"\n\
           printf 'child_claude=%s\\n' \"${{CLAVE_CLAUDE_BIN-}}\"\n\
           for arg in \"$@\"; do printf 'arg=%s\\n' \"$arg\"; done\n\
         }} > \"$CLAVE_CAPTURE\"\n"
    );
    fs::write(path, script).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn transcript_path(config: &Path, cwd: &Path, uuid: &str) -> PathBuf {
    config
        .join("projects")
        .join(clave::munge::munge_cwd(cwd.to_str().unwrap()))
        .join(format!("{uuid}.jsonl"))
}

#[test]
fn spawn_executes_selected_launcher_with_exact_argv_cwd_and_environment() {
    struct Case {
        codex: bool,
        resume: bool,
        launcher: &'static str,
        args: &'static [&'static str],
    }

    let cases = [
        Case {
            codex: false,
            resume: false,
            launcher: "claude",
            args: &["--session-id", "session-u", "--name", "name ; $(false)"],
        },
        Case {
            codex: false,
            resume: true,
            launcher: "claude",
            args: &["--resume", "session-u"],
        },
        Case {
            codex: true,
            resume: false,
            launcher: "claude-codex",
            args: &["--session-id", "session-u", "--name", "name ; $(false)"],
        },
        Case {
            codex: true,
            resume: true,
            launcher: "claude-codex",
            args: &["--resume", "session-u"],
        },
    ];

    for (index, case) in cases.iter().enumerate() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        let cwd = temp.path().join("repo with spaces");
        let config = temp.path().join("claude-config");
        let capture = temp.path().join(format!("capture-{index}"));
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&config).unwrap();

        let claude = bin.join("claude");
        let wrapper = bin.join("claude-codex");
        write_capture_executable(&claude, "claude");
        write_capture_executable(&wrapper, "claude-codex");

        if case.resume {
            let transcript = transcript_path(&config, &cwd, "session-u");
            fs::create_dir_all(transcript.parent().unwrap()).unwrap();
            fs::write(transcript, "{}\n").unwrap();
        }

        let mut command = Command::new(env!("CARGO_BIN_EXE_clave"));
        command
            .args([
                "spawn",
                "session-u",
                "--name",
                "name ; $(false)",
                "--cwd",
                cwd.to_str().unwrap(),
            ])
            .env("PATH", &bin)
            .env("CLAUDE_CONFIG_DIR", &config)
            .env("CLAVE_STATE_DIR", temp.path().join("state"))
            .env("CLAVE_CAPTURE", &capture)
            .env_remove("CLAVE_CLAUDE_BIN")
            .env_remove("CLAVE_CLAUDE_CODEX_BIN")
            .env_remove("ZELLIJ_PANE_ID");
        if case.codex {
            command.arg("--claude-codex");
        }

        let status = command.status().unwrap();
        assert!(status.success(), "case {index} failed");

        let output = fs::read_to_string(&capture).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines[0], format!("launcher={}", case.launcher));
        assert_eq!(lines[1], format!("cwd={}", cwd.display()));
        if case.codex {
            assert_eq!(lines[2], format!("child_claude={}", claude.display()));
        }
        let captured_args: Vec<_> = lines[3..]
            .iter()
            .map(|line| line.strip_prefix("arg=").unwrap())
            .collect();
        assert_eq!(captured_args, case.args.to_vec());
        assert!(!temp.path().join("false").exists());
    }
}
```

- [ ] **Step 2: Run the integration test and observe failure**

Run:

```bash
cargo test -p clave --test spawn_launch -- --nocapture
```

Expected: Codex cases FAIL because `main.rs` ignores the hidden launch flag and always execs ordinary Claude.

- [ ] **Step 3: Resolve launchers before pane registration**

In the `Command::Spawn` arm, retain cwd canonicalization and `spawn_mode`, then resolve ordinary Claude first using existing centralized advice. Resolve the optional wrapper before `register_pane`:

```rust
let wrapper = if claude_codex {
    Some(
        clave::discover::discover(clave::discover::ToolId::ClaudeCodex)
            .map(|d| d.path)
            .ok_or_else(|| {
                let advice = clave::doctor::missing_advice(
                    clave::discover::ToolId::ClaudeCodex,
                    None,
                )
                .join("\n");
                anyhow::anyhow!("claude-codex not found\n{advice}")
            })?,
    )
} else {
    None
};

spawn::register_pane(&uuid);
std::env::set_current_dir(&physical).context("entering --cwd")?;
```

Do not move or alter `spawn::register_pane` itself.

- [ ] **Step 4: Build one direct-exec command**

Replace the duplicated `Command::new(&claude)` match with:

```rust
use std::os::unix::process::CommandExt;

let executable = wrapper.as_ref().unwrap_or(&claude);
let mut command = std::process::Command::new(executable);
if claude_codex {
    command.env("CLAVE_CLAUDE_BIN", &claude);
}
match mode {
    spawn::SpawnMode::Create => {
        command.args(["--session-id", &uuid, "--name", &name]);
    }
    spawn::SpawnMode::Resume => {
        command.args(["--resume", &uuid]);
    }
}
let err = command.exec();
Err(anyhow::anyhow!(
    "exec {} failed: {err}",
    executable.display()
))
```

This must be direct `Command::new(path)` execution—no `sh -c`, `zsh -lic`, or joined command string.

- [ ] **Step 5: Run integration and existing spawn tests**

Run:

```bash
cargo test -p clave --test spawn_launch -- --nocapture
cargo test -p clave spawn_mode_is_resume_iff_jsonl_exists
cargo test -p clave
```

Expected: PASS for all four create/resume × plain/Codex cases. The name containing shell metacharacters remains one exact argv element and creates no side-effect file.

- [ ] **Step 6: Prepare the commit checkpoint—do not execute without explicit approval**

```bash
git add crates/clave/src/main.rs crates/clave/tests/spawn_launch.rs
git commit -m "feat(clave): launch agents through optional codex wrapper"
```

---

### Task 6: Run complete verification and review the narrow boundary

**Files:**
- Review all changed files.
- Update if necessary: `docs/superpowers/specs/2026-07-22-claude-codex-launch-profile-design.md`
- Include when approved: `docs/superpowers/plans/2026-07-22-claude-codex-launch-flag.md`

**Interfaces:**
- Consumes: completed implementation.
- Produces: verification dossier, independent review results, and human-run live-validation instructions.

- [ ] **Step 1: Format only changed Rust code**

Run:

```bash
cargo fmt --all -- --check
```

If it fails, run `cargo fmt --all`, inspect that unrelated large files were not reformatted, then rerun `cargo fmt --all -- --check`.

Expected: PASS.

- [ ] **Step 2: Run the required repository gates**

Run exactly:

```bash
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all PASS. Report any failure verbatim; do not mark verification complete while a gate is red.

- [ ] **Step 3: Audit scope and invariants**

Run:

```bash
git diff --check
git diff --stat main
git status --short
rg -n 'ClaudeCodex|claude_codex|claude-codex|--codex' crates/clave
```

Check manually:

- no `clave-types` or bar behavior changes;
- no proxy credential/model/base-URL handling in Rust;
- no Codex-store code;
- no launcher shell wrapping;
- ordinary doctor catalogue unchanged;
- optional preflight absent from live jump/attach;
- executable resolution precedes pane registration;
- worktree cwd remains physical and untouched.

- [ ] **Step 4: Run both required review lanes**

Invoke the vendored fugu review and one independent adversarial reviewer that did not implement the code. Record which lanes actually ran and every declined finding with reasoning.

Expected: no unresolved confirmed correctness findings.

- [ ] **Step 5: Ask Ollie to run the real non-persistent wrapper smokes**

Print, but do not execute on their behalf:

```bash
claude-codex --version
claude-codex -p --no-session-persistence 'Reply with exactly: ok'
```

Expected: Claude Code version output, then exactly `ok` through the proxy without a durable session.

- [ ] **Step 6: Prepare human-driven Zellij validation**

Provide a checklist for Ollie to run only after the sandbox/status preconditions in `docs/dev/TESTING.md` are satisfied:

1. new plain agent;
2. new `clave add --codex` agent;
3. close and dormant-resume Codex → plain;
4. close and dormant-resume plain → Codex;
5. select a live row with the opposite flag and confirm it only jumps;
6. dead-session relaunch with a Codex eager row;
7. repeat create/resume from a registered git worktree and confirm history/cwd.

The agent observes logs/store/status only; Ollie owns all Zellij input and lifecycle.

- [ ] **Step 7: Prepare final commit(s)—do not execute without explicit approval**

Before any commit, present the complete diff, test results, review results, and proposed commit grouping to Ollie. Only after explicit approval:

```bash
git add crates/clave docs/superpowers/specs/2026-07-22-claude-codex-launch-profile-design.md \
  docs/superpowers/plans/2026-07-22-claude-codex-launch-flag.md
git commit -m "feat(clave): add claude-codex launch flag"
```

The maintainer signs the commit. Do not push or open a PR unless separately requested/authorized.
