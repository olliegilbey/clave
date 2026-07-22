# clave doctor + install flow — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dependency discovery + `clave doctor` + per-command preflight + one-command first run + single-file distribution (embedded wasm, cargo-dist).

**Architecture:** A pure-core/thin-shell pair of new modules — `discover.rs` (find external binaries beyond PATH) and `doctor.rs` (Facts → Findings → rendered report), with `gather()` doing all IO. Preflight and doctor render from the same `Finding`s so their copy cannot drift. Release binaries embed `clave-bar.wasm` via a build script; `clave setup` extracts it.

**Tech Stack:** Rust (edition 2024 workspace), `which` crate v8 (`which_global`), serde/serde_json (already present), std `IsTerminal`. Spec: `docs/superpowers/specs/2026-07-21-installer-doctor-design.md`.

## Global Constraints

- **Test with `cargo test --workspace` always** — bare `cargo test` silently skips the wasm crate (CLAUDE.md).
- **TDD**: failing test first, watch it fail, then implement. Tests live in-file under `#[cfg(test)] mod tests` (codebase style).
- **Commits need maintainer approval** (CLAUDE.md): at each commit step, stage and show the diffstat; commit only if the maintainer has granted approval for this plan's execution — otherwise stop and ask.
- **Comments explain why** and cite the spec section (`spec §Discovery` style refers to the 2026-07-21 installer-doctor spec).
- Tested zellij version is **0.44.3** exactly. Doctor never halts on version mismatch — Warn only.
- Remediation for **zellij and claude is URL-only** (never a package-manager guess). URLs: `https://zellij.dev/documentation/installation`, `https://github.com/zellij-org/zellij/releases`, `https://code.claude.com/docs`, `https://github.com/junegunn/fzf#installation`, `https://github.com/ajeetdsouza/zoxide#installation`, `https://git-scm.com/downloads`.
- **Never prompt without a TTY**; preflight prints only failures.
- Never run `just install`/`just release` from this working session; sandbox-only for live validation (CLAUDE.md).
- Lint gate: `just clippy` (`cargo clippy --workspace --all-targets -- -D warnings`).

## File Structure

- Create: `crates/clave/src/discover.rs` — ToolId, Via, Discovered, candidate dirs, resolution order, semver key, IO shell.
- Create: `crates/clave/src/doctor.rs` — PkgManager, Facts, Finding, diagnose, remediation copy, renderers, gather, run_doctor, preflight.
- Create: `crates/clave/build.rs` — embeds `$CLAVE_BAR_WASM` (or an empty marker) into OUT_DIR.
- Modify: `crates/clave/src/lib.rs` — register the two modules.
- Modify: `crates/clave/src/release.rs` — `embedded_wasm()`.
- Modify: `crates/clave/src/setup.rs` — `HOOK_EVENTS` made pub, `permissions_seeded()`, `ensure_wasm()`, first-run flow in `launch_session`, discovered zellij path.
- Modify: `crates/clave/src/main.rs` — `Doctor` subcommand; spawn arm uses discovered claude.
- Modify: `crates/clave/src/add.rs` — preflight + pane-hold + discovered fzf/zoxide.
- Modify: `crates/clave/Cargo.toml`, root `Cargo.toml` — `which` dep.
- Modify: `justfile` — `dist-build` recipe.
- Create: `dist-workspace.toml` + `.github/workflows/release.yml` (generated) — Task 11.

---

### Task 1: `discover.rs` pure core — resolution order, candidate dirs, semver key

**Files:**
- Create: `crates/clave/src/discover.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod discover;` alongside the existing mods)

**Interfaces:**
- Consumes: nothing (std + serde only).
- Produces:
  - `pub enum ToolId { Zellij, Claude, Git, Fzf, Zoxide }` with `pub fn bin_name(self) -> &'static str`, `pub fn override_var(self) -> &'static str`
  - `pub enum Via { Override, PathLookup, KnownLocation }`
  - `pub struct Discovered { pub path: PathBuf, pub via: Via }`
  - `pub fn candidate_dirs(tool: ToolId, home: &Path, nvm_versions: &[String]) -> Vec<PathBuf>`
  - `pub fn resolve(override_val: Option<&str>, path_hit: Option<PathBuf>, known_hits: &[PathBuf]) -> Option<Discovered>`
  - `pub fn semver_key(s: &str) -> Option<(u64, u64, u64)>`
  - `pub fn tilde(path: &Path, home: &Path) -> String`

- [ ] **Step 1: Write the failing tests**

Create `crates/clave/src/discover.rs` containing ONLY the test module for now:

```rust
//! Binary discovery beyond PATH (spec §Discovery). PATH is not ground truth:
//! tools live in places only interactive shells know about (nvm bins, the
//! Claude local-install dir), and clave's exec contexts — zellij panes,
//! keybind-spawned commands — don't inherit the user's interactive PATH.
//! Resolution: explicit override var → which_global → curated locations.
//! Found off-PATH ⇒ clave USES the absolute path (the runtime_binary() idiom
//! extended): the user's shell config is their business; clave just works.

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn override_beats_path_beats_known() {
        let known = [PathBuf::from("/known/claude")];
        let r = resolve(Some("/ovr/claude"), Some(PathBuf::from("/path/claude")), &known).unwrap();
        assert_eq!((r.path.as_path(), r.via), (Path::new("/ovr/claude"), Via::Override));
        let r = resolve(None, Some(PathBuf::from("/path/claude")), &known).unwrap();
        assert_eq!((r.path.as_path(), r.via), (Path::new("/path/claude"), Via::PathLookup));
        let r = resolve(None, None, &known).unwrap();
        assert_eq!((r.path.as_path(), r.via), (Path::new("/known/claude"), Via::KnownLocation));
        assert_eq!(resolve(None, None, &[]), None);
    }

    #[test]
    fn override_var_names_follow_the_clave_env_pattern() {
        assert_eq!(ToolId::Claude.override_var(), "CLAVE_CLAUDE_BIN");
        assert_eq!(ToolId::Zellij.override_var(), "CLAVE_ZELLIJ_BIN");
        assert_eq!(ToolId::Fzf.override_var(), "CLAVE_FZF_BIN");
        assert_eq!(ToolId::Zoxide.override_var(), "CLAVE_ZOXIDE_BIN");
        assert_eq!(ToolId::Git.override_var(), "CLAVE_GIT_BIN");
    }

    #[test]
    fn candidate_dirs_shared_list_and_claude_specifics() {
        let home = Path::new("/home/u");
        let dirs = candidate_dirs(ToolId::Claude, home, &["v20.11.0".into(), "v22.1.0".into(), "v9.0.0".into()]);
        // Shared prefix (order = priority).
        assert_eq!(dirs[0], home.join(".local/bin"));
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(dirs.contains(&PathBuf::from("/usr/local/bin")));
        assert!(dirs.contains(&home.join(".cargo/bin")));
        // Claude-specific: the local-install migration dir…
        assert!(dirs.contains(&home.join(".claude/local")));
        // …and NEWEST nvm version by semver, not lexically (v9 < v22).
        assert!(dirs.contains(&home.join(".nvm/versions/node/v22.1.0/bin")));
        assert!(!dirs.iter().any(|d| d.ends_with("v9.0.0/bin")));
        assert!(dirs.contains(&home.join(".volta/bin")));
        assert!(dirs.contains(&home.join(".bun/bin")));
    }

    #[test]
    fn candidate_dirs_fzf_git_install_and_no_claude_dirs_for_others() {
        let home = Path::new("/home/u");
        let dirs = candidate_dirs(ToolId::Fzf, home, &[]);
        assert!(dirs.contains(&home.join(".fzf/bin")));
        assert!(!dirs.iter().any(|d| d.ends_with(".claude/local")));
        let dirs = candidate_dirs(ToolId::Zoxide, home, &[]);
        assert!(dirs.contains(&home.join(".cargo/bin")));
    }

    #[test]
    fn semver_key_parses_and_orders() {
        assert_eq!(semver_key("v22.1.0"), Some((22, 1, 0)));
        assert_eq!(semver_key("0.44.3"), Some((0, 44, 3)));
        assert_eq!(semver_key("garbage"), None);
        assert!(semver_key("v22.1.0") > semver_key("v9.9.9"));
    }

    #[test]
    fn tilde_abbreviates_home() {
        let home = Path::new("/home/u");
        assert_eq!(tilde(Path::new("/home/u/.local/bin/claude"), home), "~/.local/bin/claude");
        assert_eq!(tilde(Path::new("/usr/bin/git"), home), "/usr/bin/git");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --workspace discover 2>&1 | tail -5`
Expected: compile FAILURE (`resolve`, `ToolId`, … not found).

- [ ] **Step 3: Implement**

Add above the test module in `crates/clave/src/discover.rs`:

```rust
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The external binaries clave shells out to (spec §Check catalogue).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolId {
    Zellij,
    Claude,
    Git,
    Fzf,
    Zoxide,
}

impl ToolId {
    pub fn bin_name(self) -> &'static str {
        match self {
            ToolId::Zellij => "zellij",
            ToolId::Claude => "claude",
            ToolId::Git => "git",
            ToolId::Fzf => "fzf",
            ToolId::Zoxide => "zoxide",
        }
    }

    /// `$CLAVE_<TOOL>_BIN` — same env-override pattern as CLAVE_SESSION /
    /// CLAVE_DATA_DIR (spec §Discovery: override always wins).
    pub fn override_var(self) -> &'static str {
        match self {
            ToolId::Zellij => "CLAVE_ZELLIJ_BIN",
            ToolId::Claude => "CLAVE_CLAUDE_BIN",
            ToolId::Git => "CLAVE_GIT_BIN",
            ToolId::Fzf => "CLAVE_FZF_BIN",
            ToolId::Zoxide => "CLAVE_ZOXIDE_BIN",
        }
    }
}

/// How a tool was found — doctor reports it, and PathLookup-vs-KnownLocation
/// decides the off-PATH Warn (spec §Discovery three-state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    Override,
    PathLookup,
    KnownLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Discovered {
    pub path: PathBuf,
    pub via: Via,
}

/// Pure resolution order (spec §Discovery): override → PATH → known dirs.
/// The override is never second-guessed — not even existence-checked here;
/// a broken override should fail loudly at exec, not be silently ignored.
pub fn resolve(
    override_val: Option<&str>,
    path_hit: Option<PathBuf>,
    known_hits: &[PathBuf],
) -> Option<Discovered> {
    if let Some(o) = override_val {
        return Some(Discovered { path: PathBuf::from(o), via: Via::Override });
    }
    if let Some(p) = path_hit {
        return Some(Discovered { path: p, via: Via::PathLookup });
    }
    known_hits
        .first()
        .map(|p| Discovered { path: p.clone(), via: Via::KnownLocation })
}

/// Curated known locations, priority order (spec §Discovery). `nvm_versions`
/// is the pre-listed contents of ~/.nvm/versions/node — pure so the newest-
/// version pick is unit-testable.
pub fn candidate_dirs(tool: ToolId, home: &Path, nvm_versions: &[String]) -> Vec<PathBuf> {
    // Shared: rc-file-only PATH additions and standard prefixes. ~/.cargo/bin
    // covers cargo-installed zellij/zoxide; /sbin covers apk-adjacent boxes.
    let mut dirs = vec![
        home.join(".local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".cargo/bin"),
    ];
    match tool {
        ToolId::Claude => {
            // The local-install migration puts the real binary here and adds
            // only a shell ALIAS — invisible to every spawned process.
            dirs.push(home.join(".claude/local"));
            // npm-global under nvm: PATH is set by rc files only. Newest
            // version by semver — lexical sort would rank v9 above v22.
            if let Some(newest) = nvm_versions
                .iter()
                .filter(|v| semver_key(v).is_some())
                .max_by_key(|v| semver_key(v))
            {
                dirs.push(home.join(".nvm/versions/node").join(newest).join("bin"));
            }
            dirs.push(home.join(".volta/bin"));
            dirs.push(home.join(".bun/bin"));
        }
        ToolId::Fzf => dirs.push(home.join(".fzf/bin")), // fzf's git-install
        ToolId::Zellij | ToolId::Git | ToolId::Zoxide => {}
    }
    dirs
}

/// Parse `1.2.3` / `v1.2.3` into an orderable triple. Shared by the nvm
/// newest-pick and the release-skew check (Task 6).
pub fn semver_key(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let mut it = s.split('.');
    let maj = it.next()?.parse().ok()?;
    let min = it.next()?.parse().ok()?;
    let pat = it.next()?.parse().ok()?;
    Some((maj, min, pat))
}

/// Home-abbreviated display path — doctor prints every resolved path
/// (spec §Discovery: coexisting installs are a real footgun).
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
```

In `crates/clave/src/lib.rs`, add `pub mod discover;` in alphabetical position among the existing `pub mod` lines.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace discover 2>&1 | tail -5`
Expected: `6 passed`. Then `cargo test --workspace 2>&1 | grep -c "FAILED"` → 0.

- [ ] **Step 5: Commit (per Global Constraints approval rule)**

```bash
git add crates/clave/src/discover.rs crates/clave/src/lib.rs
git commit -m "feat(clave): discover.rs pure core — override/PATH/known resolution (spec §Discovery)"
```

---

### Task 2: `discover.rs` IO shell — `which_global` + executable probe

**Files:**
- Modify: `crates/clave/src/discover.rs`
- Modify: `crates/clave/Cargo.toml`, root `Cargo.toml`

**Interfaces:**
- Consumes: Task 1's `resolve`, `candidate_dirs`, `ToolId`.
- Produces: `pub fn discover(tool: ToolId) -> Option<Discovered>`, `pub fn is_executable(p: &Path) -> bool`.

- [ ] **Step 1: Add the dependency**

Root `Cargo.toml`, `[workspace.dependencies]` (after the `fs4` entry):

```toml
# Binary discovery (spec §Discovery). which_global ONLY — plain which()
# respects cwd, and clave runs in arbitrary project dirs where a ./brew
# would satisfy the probe.
which = "8"
```

`crates/clave/Cargo.toml`, `[dependencies]` (after `fs4.workspace = true`):

```toml
# Binary discovery beyond PATH (spec §Discovery).
which.workspace = true
```

- [ ] **Step 2: Write the failing test**

Append to the test module in `discover.rs`:

```rust
    #[test]
    fn is_executable_requires_the_exec_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("tool");
        std::fs::write(&f, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable(&f));
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&f));
        assert!(!is_executable(&dir.path().join("absent")));
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --workspace is_executable 2>&1 | tail -5`
Expected: compile FAILURE (`is_executable` not found).

- [ ] **Step 4: Implement the shell**

Add to `discover.rs` (above the tests):

```rust
/// Unix exec-bit check for known-location candidates. A readable-but-not-
/// executable file is not a hit — matching what exec() would do.
pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The IO shell: gather probes, delegate to resolve() (pure). Everything
/// here is best-effort — a failed read_dir is an empty nvm list, not an
/// error; discovery answers "where is it", never "why not".
pub fn discover(tool: ToolId) -> Option<Discovered> {
    let override_val = std::env::var(tool.override_var()).ok();
    // which_global, NOT which: cwd must never satisfy a probe (spec §Probes).
    let path_hit = which::which_global(tool.bin_name()).ok();
    let home = dirs::home_dir()?;
    let nvm_versions: Vec<String> = std::fs::read_dir(home.join(".nvm/versions/node"))
        .map(|rd| rd.filter_map(|e| Some(e.ok()?.file_name().to_str()?.to_string())).collect())
        .unwrap_or_default();
    let known_hits: Vec<PathBuf> = candidate_dirs(tool, &home, &nvm_versions)
        .into_iter()
        .map(|d| d.join(tool.bin_name()))
        // ~/.claude/local/claude: candidate_dirs yields ~/.claude/local as a
        // DIR, so the join above already forms the full binary path. apk's
        // /sbin home is covered by the pkg-manager probe, not tool discovery.
        .filter(|p| is_executable(p))
        .collect();
    resolve(override_val.as_deref(), path_hit, &known_hits)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace discover 2>&1 | tail -5`
Expected: `7 passed`. Also run `just clippy` — expect clean.

- [ ] **Step 6: Commit**

```bash
git add crates/clave/src/discover.rs crates/clave/Cargo.toml Cargo.toml Cargo.lock
git commit -m "feat(clave): discover() IO shell — which_global + curated locations (spec §Discovery)"
```

---

### Task 3: wasm embedding — `build.rs`, `embedded_wasm()`, `ensure_wasm()`

**Files:**
- Create: `crates/clave/build.rs`
- Modify: `crates/clave/src/release.rs` (add `embedded_wasm()`)
- Modify: `crates/clave/src/setup.rs` (add `extract_embedded()` + rewire `run_setup`)
- Modify: `justfile` (add `dist-build`)

**Interfaces:**
- Consumes: `setup::data_dir()`, `release::versioned_wasm_name()` (existing).
- Produces: `release::embedded_wasm() -> Option<&'static [u8]>`, `setup::extract_embedded(dir: &Path, bytes: &[u8], version: &str) -> Result<PathBuf>` (write-if-absent), rewired `run_setup`.

- [ ] **Step 1: Create `crates/clave/build.rs`**

```rust
//! Embeds the bar wasm into release binaries (spec §Distribution): cargo-dist
//! ships ONE file, so the wasm must ride inside the CLI. Gated on the
//! CLAVE_BAR_WASM env var (the CLAVE_BUILD_TAG pattern): release CI builds
//! the wasm first and points this var at it; dev builds embed an empty
//! marker and the sandbox flow (just dev-install) is untouched.
fn main() {
    println!("cargo:rerun-if-env-changed=CLAVE_BAR_WASM");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("clave-bar.embedded");
    match std::env::var("CLAVE_BAR_WASM") {
        Ok(src) => {
            std::fs::copy(&src, &out).expect("CLAVE_BAR_WASM is set but unreadable");
            println!("cargo:rerun-if-changed={src}");
        }
        Err(_) => std::fs::write(&out, []).expect("writing empty embed marker"),
    }
}
```

- [ ] **Step 2: Write the failing tests**

In `crates/clave/src/release.rs` tests:

```rust
    #[test]
    fn dev_builds_embed_no_wasm() {
        // cargo test runs without CLAVE_BAR_WASM → empty marker → None.
        assert!(embedded_wasm().is_none());
    }
```

In `crates/clave/src/setup.rs` tests:

```rust
    #[test]
    fn extract_embedded_is_write_if_absent() {
        let dir = tempfile::tempdir().unwrap();
        let p = extract_embedded(dir.path(), b"wasm-bytes", "0.1.0").unwrap();
        assert_eq!(p.file_name().unwrap(), "clave-bar-v0.1.0.wasm");
        assert_eq!(std::fs::read(&p).unwrap(), b"wasm-bytes");
        // Second call must NOT rewrite — a live session's loaded wasm is
        // never touched (§2 running-session immunity).
        extract_embedded(dir.path(), b"DIFFERENT", "0.1.0").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"wasm-bytes");
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --workspace embedded 2>&1 | tail -5`
Expected: compile FAILURE (functions not found).

- [ ] **Step 4: Implement**

`release.rs`:

```rust
/// The bar wasm baked into this binary at build time, if any (spec
/// §Distribution). Empty marker ⇒ dev build ⇒ None: the sandbox flow owns
/// wasm placement there (just dev-install).
pub fn embedded_wasm() -> Option<&'static [u8]> {
    static BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/clave-bar.embedded"));
    (!BYTES.is_empty()).then_some(BYTES)
}
```

`setup.rs`:

```rust
/// Write the embedded wasm as the VERSIONED artifact — which wasm_path()
/// already prefers — if absent (spec §Distribution). Write-if-absent keeps
/// re-runs idempotent and honors running-session immunity (§2).
pub fn extract_embedded(dir: &Path, bytes: &[u8], version: &str) -> Result<PathBuf> {
    let dest = dir.join(crate::release::versioned_wasm_name(version));
    if !dest.exists() {
        std::fs::write(&dest, bytes)
            .with_context(|| format!("extracting embedded wasm to {}", dest.display()))?;
    }
    Ok(dest)
}
```

(`use std::path::Path;` is already imported via `PathBuf`; adjust imports as needed.)

Rewire `run_setup` — replace the `anyhow::ensure!(wasm.exists(), …)` block with:

```rust
    if !wasm.exists() {
        match crate::release::embedded_wasm() {
            // Release binary: self-contained — extract and use the versioned
            // artifact (spec §Distribution: one file IS the install).
            Some(bytes) => {
                extract_embedded(&dir, bytes, env!("CARGO_PKG_VERSION"))?;
            }
            // Dev build: wasm placement belongs to the sandbox flow.
            None => anyhow::bail!(
                "{} missing — run `just dev-install` first (it builds the sandbox wasm here)",
                wasm.display()
            ),
        }
    }
    let wasm = wasm_path()?; // re-resolve: extraction creates the preferred versioned file
```

`justfile`, after `build-bar-release`:

```just
# Local release-parity build: the CLI with the bar wasm EMBEDDED (spec
# §Distribution) — what cargo-dist produces in CI, buildable on any clone.
dist-build: build-bar-release
    CLAVE_BAR_WASM=$(pwd)/target/wasm32-wasip1/release/clave-bar.wasm cargo build --release -p clave
```

- [ ] **Step 5: Run tests + local roundtrip**

Run: `cargo test --workspace 2>&1 | grep -E "test result" | tail -3` — all pass.
Local roundtrip — `HOME` itself is redirected because `run_setup` also writes
the Zellij permission cache at a home-derived path with NO env override
(setup.rs `permissions_cache_path`); `$HOME` sandboxes every write at once:

```bash
just dist-build
T=$(mktemp -d) && HOME=$T CLAVE_DATA_DIR=$T/data CLAUDE_CONFIG_DIR=$T/claude ./target/release/clave setup; ls -la $T/data
```

Expected: `clave-bar-v0.1.0.wasm` present alongside config.kdl/layout.kdl (this proves the embed→extract path with zero pre-installed wasm), and NOTHING outside `$T` is touched.

- [ ] **Step 6: Commit**

```bash
git add crates/clave/build.rs crates/clave/src/release.rs crates/clave/src/setup.rs justfile
git commit -m "feat(clave): embed bar wasm in release builds; setup extracts it (spec §Distribution)"
```

---

### Task 4: doctor pure parsers — versions, package manager, Facts type

**Files:**
- Create: `crates/clave/src/doctor.rs`
- Modify: `crates/clave/src/lib.rs` (add `pub mod doctor;`)
- Modify: `crates/clave/src/setup.rs` (lift `HOOK_EVENTS` to a pub const; add `permissions_seeded`)

**Interfaces:**
- Consumes: `discover::{ToolId, Discovered}`.
- Produces:
  - `pub const TESTED_ZELLIJ: &str = "0.44.3"`
  - `pub enum PkgManager { Brew, Apt, Dnf, Pacman, Apk }` with `pub fn install_line(self, pkg: &str) -> String`
  - `pub fn short_version(line: &str) -> Option<String>`
  - `pub struct ToolFact { pub discovered: Option<Discovered>, pub version: Option<String> }`
  - `pub struct Facts { … }` (full field list below) — all `Serialize`
  - `setup::HOOK_EVENTS: [&str; 4]` (pub), `setup::permissions_seeded(existing: &str, wasm_abs: &str) -> bool`

- [ ] **Step 1: Write the failing tests**

Create `crates/clave/src/doctor.rs`:

```rust
//! `clave doctor` + preflight (spec 2026-07-21). Pure-core/thin-shell:
//! gather() does ALL the IO; diagnose() is pure over Facts; both renderers
//! draw from the same Findings so doctor and preflight copy cannot drift
//! (the uv property, collection-shaped).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_version_takes_first_numeric_token() {
        assert_eq!(short_version("zellij 0.44.3").as_deref(), Some("0.44.3"));
        assert_eq!(short_version("git version 2.51.0").as_deref(), Some("2.51.0"));
        assert_eq!(short_version("2.1.4 (Claude Code)").as_deref(), Some("2.1.4"));
        assert_eq!(short_version("v0.9.6").as_deref(), Some("v0.9.6"));
        assert_eq!(short_version("no digits here"), None);
        assert_eq!(short_version(""), None);
    }

    #[test]
    fn pkg_manager_install_lines_match_the_mise_prefixes() {
        assert_eq!(PkgManager::Brew.install_line("fzf"), "brew install fzf");
        assert_eq!(PkgManager::Apt.install_line("fzf"), "sudo apt-get install -y fzf");
        assert_eq!(PkgManager::Dnf.install_line("fzf"), "sudo dnf install -y fzf");
        assert_eq!(PkgManager::Pacman.install_line("fzf"), "sudo pacman -S fzf");
        assert_eq!(PkgManager::Apk.install_line("fzf"), "sudo apk add fzf");
    }
}
```

And in `setup.rs` tests:

```rust
    #[test]
    fn permissions_seeded_detects_our_grant() {
        let seeded = merge_permissions_kdl("", "/data/clave-bar.wasm");
        assert!(permissions_seeded(&seeded, "/data/clave-bar.wasm"));
        assert!(!permissions_seeded("", "/data/clave-bar.wasm"));
        assert!(!permissions_seeded(&seeded, "/other/clave-bar.wasm"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --workspace doctor 2>&1 | tail -3` and `cargo test --workspace permissions_seeded 2>&1 | tail -3`
Expected: compile FAILURE.

- [ ] **Step 3: Implement**

`doctor.rs` (above tests):

```rust
use std::path::PathBuf;

use serde::Serialize;

use crate::discover::Discovered;

/// The zellij version the validation ledger pins behavior to (permission-
/// cache format, pane-resize semantics). Mismatch WARNS, never halts.
pub const TESTED_ZELLIJ: &str = "0.44.3";

/// Probed package managers, priority order (spec §Probes: probe-first,
/// distro identity never consulted). Prefixes match mise's install_prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PkgManager {
    Brew,
    Apt,
    Dnf,
    Pacman,
    Apk,
}

impl PkgManager {
    pub fn install_line(self, pkg: &str) -> String {
        match self {
            PkgManager::Brew => format!("brew install {pkg}"),
            PkgManager::Apt => format!("sudo apt-get install -y {pkg}"),
            PkgManager::Dnf => format!("sudo dnf install -y {pkg}"),
            PkgManager::Pacman => format!("sudo pacman -S {pkg}"),
            PkgManager::Apk => format!("sudo apk add {pkg}"),
        }
    }
}

/// First whitespace token containing a digit — tolerant of `zellij 0.44.3`,
/// `git version 2.51.0`, `2.1.4 (Claude Code)` alike. Display-only; never
/// an error when unparseable (spec: version checks warn, never halt).
pub fn short_version(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|t| t.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolFact {
    pub discovered: Option<Discovered>,
    pub version: Option<String>, // short_version of `<path> --version`
}

/// Everything gather() learns — the single input to diagnose() (pure) and
/// the `--json` payload.
#[derive(Debug, Clone, Serialize)]
pub struct Facts {
    pub home: PathBuf,
    pub zellij: ToolFact,
    pub claude: ToolFact,
    pub git: ToolFact,
    pub fzf: ToolFact,
    pub zoxide: ToolFact,
    pub pkg_manager: Option<PkgManager>,
    pub config_exists: bool,
    pub layout_exists: bool,
    pub wasm_path: PathBuf,
    pub wasm_exists: bool,
    pub has_embedded_wasm: bool,
    /// (event, clave-entry count) per HOOK_EVENTS — exactly 1 is healthy.
    pub hook_counts: Vec<(String, usize)>,
    pub perms_seeded: bool,
    pub bin_dir_exists: bool,
    /// Semver strings parsed from <data>/bin/clave-v* names.
    pub installed_releases: Vec<String>,
    /// None ⇒ not applicable (non-Linux); Some(false) ⇒ the SSH trap.
    pub xdg_runtime_dir: Option<bool>,
    pub version_line: String, // release::long_version()
}
```

`setup.rs` — lift the events const out of `merge_hooks` (replace the local `const EVENTS…` with a reference to this):

```rust
/// The §6.5 state machine's input events — hook registration AND doctor's
/// exactly-one-entry check key off the same list.
pub const HOOK_EVENTS: [&str; 4] = ["UserPromptSubmit", "Stop", "Notification", "SessionEnd"];
```

(inside `merge_hooks`, change `for ev in EVENTS` to `for ev in HOOK_EVENTS`), and add:

```rust
/// Is our grant present in the permission-cache text? Same key form
/// merge_permissions_kdl writes — doctor never guesses a second format.
pub fn permissions_seeded(existing: &str, wasm_abs: &str) -> bool {
    existing.contains(&format!("\"file:{wasm_abs}\""))
}
```

`lib.rs`: add `pub mod doctor;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace 2>&1 | grep "test result" | tail -3`
Expected: all pass, no failures.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/doctor.rs crates/clave/src/lib.rs crates/clave/src/setup.rs
git commit -m "feat(clave): doctor Facts + version/pkg-manager parsers (spec §Probes)"
```

---

### Task 5: diagnose — tool checks (three-state + zellij version warn)

**Files:**
- Modify: `crates/clave/src/doctor.rs`

**Interfaces:**
- Consumes: Task 4 types; `discover::{ToolId, Via, tilde, semver_key}`.
- Produces:
  - `pub enum Group { RequiredTools, AgentPicker, Setup, Environment }`
  - `pub enum Severity { Ok, Warn, Problem }`
  - `pub struct Finding { pub group: Group, pub severity: Severity, pub label: String, pub advice: Vec<String> }`
  - `pub fn diagnose_tool(tool: ToolId, fact: &ToolFact, mgr: Option<PkgManager>, home: &Path) -> Finding`
  - `pub fn missing_advice(tool: ToolId, mgr: Option<PkgManager>) -> Vec<String>`

- [ ] **Step 1: Write the failing tests**

```rust
    use crate::discover::{Discovered, ToolId, Via};
    use std::path::{Path, PathBuf};

    fn found(path: &str, via: Via, ver: Option<&str>) -> ToolFact {
        ToolFact {
            discovered: Some(Discovered { path: PathBuf::from(path), via }),
            version: ver.map(str::to_string),
        }
    }

    #[test]
    fn tool_on_path_is_ok_with_version_and_tilde_path() {
        let f = found("/home/u/.cargo/bin/zoxide", Via::PathLookup, Some("0.9.6"));
        let d = diagnose_tool(ToolId::Zoxide, &f, None, Path::new("/home/u"));
        assert_eq!(d.severity, Severity::Ok);
        assert_eq!(d.label, "zoxide 0.9.6 (~/.cargo/bin/zoxide)");
        assert_eq!(d.group, Group::AgentPicker);
        assert!(d.advice.is_empty());
    }

    #[test]
    fn tool_off_path_warns_but_is_functional() {
        let f = found("/home/u/.claude/local/claude", Via::KnownLocation, Some("2.1.4"));
        let d = diagnose_tool(ToolId::Claude, &f, None, Path::new("/home/u"));
        assert_eq!(d.severity, Severity::Warn);
        assert!(d.label.contains("~/.claude/local/claude"));
        assert!(d.label.contains("not on your PATH"));
        assert!(d.advice.iter().any(|l| l.contains("clave will use this path directly")));
        assert!(d.advice.iter().any(|l| l.contains("alias is not enough")));
    }

    #[test]
    fn missing_tool_is_a_problem_with_remediation() {
        let none = ToolFact { discovered: None, version: None };
        let d = diagnose_tool(ToolId::Fzf, &none, Some(PkgManager::Brew), Path::new("/h"));
        assert_eq!(d.severity, Severity::Problem);
        assert_eq!(d.label, "fzf not found");
        // Hedged pkg-manager line (flutter voice) + indented command + URL.
        assert!(d.advice.iter().any(|l| l.contains("likely available from your package manager")));
        assert!(d.advice.iter().any(|l| l == "    brew install fzf"));
        assert!(d.advice.iter().any(|l| l.contains("github.com/junegunn/fzf")));
    }

    #[test]
    fn zellij_and_claude_remediation_is_url_only() {
        // Even with a probed manager, NEVER print an install command for
        // these two (zellij absent from distro repos; InstallFix for claude).
        for (tool, url) in [(ToolId::Zellij, "zellij.dev"), (ToolId::Claude, "code.claude.com")] {
            let adv = missing_advice(tool, Some(PkgManager::Apt));
            assert!(adv.iter().any(|l| l.contains(url)), "{tool:?}");
            assert!(!adv.iter().any(|l| l.contains("apt-get")), "{tool:?}");
        }
        assert!(missing_advice(ToolId::Zellij, Some(PkgManager::Apt))
            .iter()
            .any(|l| l.contains("github.com/zellij-org/zellij/releases")));
    }

    #[test]
    fn zellij_version_mismatch_warns_naming_tested() {
        let f = found("/usr/local/bin/zellij", Via::PathLookup, Some("0.45.0"));
        let d = diagnose_tool(ToolId::Zellij, &f, None, Path::new("/h"));
        assert_eq!(d.severity, Severity::Warn);
        assert!(d.label.contains("0.45.0"));
        assert!(d.label.contains(TESTED_ZELLIJ));
        // Exact match is Ok; unparseable is Warn, never Problem.
        let ok = found("/u/zellij", Via::PathLookup, Some("0.44.3"));
        assert_eq!(diagnose_tool(ToolId::Zellij, &ok, None, Path::new("/h")).severity, Severity::Ok);
        let weird = found("/u/zellij", Via::PathLookup, None);
        assert_eq!(diagnose_tool(ToolId::Zellij, &weird, None, Path::new("/h")).severity, Severity::Warn);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --workspace diagnose_tool 2>&1 | tail -3`
Expected: compile FAILURE.

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    RequiredTools,
    AgentPicker,
    Setup,
    Environment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Ok,
    Warn,
    Problem,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub group: Group,
    pub severity: Severity,
    pub label: String,
    /// Structured remediation lines (the neovim advice[] shape). Renderer
    /// indents; lines carrying commands self-indent 4 further.
    pub advice: Vec<String>,
}

use crate::discover::{tilde, ToolId, Via};
use std::path::Path;

fn tool_group(tool: ToolId) -> Group {
    match tool {
        ToolId::Zellij | ToolId::Claude | ToolId::Git => Group::RequiredTools,
        ToolId::Fzf | ToolId::Zoxide => Group::AgentPicker,
    }
}

/// Remediation copy for a missing tool (spec §Check catalogue). ALL user-
/// facing missing-tool strings live here — the flutter user_messages move.
pub fn missing_advice(tool: ToolId, mgr: Option<PkgManager>) -> Vec<String> {
    match tool {
        // URL-only: absent from distro repos — a probed `apt install zellij`
        // would be WRONG advice for the headline dependency (spec §Check).
        ToolId::Zellij => vec![
            "Install from https://zellij.dev/documentation/installation".into(),
            "or grab a binary: https://github.com/zellij-org/zellij/releases".into(),
        ],
        // URL-only: InstallFix ad-poisons copied one-liners and names Claude
        // Code users as a primary target — official docs, nothing else.
        ToolId::Claude => vec!["Install Claude Code: https://code.claude.com/docs".into()],
        ToolId::Git | ToolId::Fzf | ToolId::Zoxide => {
            let (pkg, url) = match tool {
                ToolId::Git => ("git", "https://git-scm.com/downloads"),
                ToolId::Fzf => ("fzf", "https://github.com/junegunn/fzf#installation"),
                _ => ("zoxide", "https://github.com/ajeetdsouza/zoxide#installation"),
            };
            match mgr {
                Some(m) => vec![
                    "It is likely available from your package manager:".into(),
                    String::new(),
                    format!("    {}", m.install_line(pkg)),
                    String::new(),
                    format!("or see {url}"),
                ],
                None => vec![format!("See {url}")],
            }
        }
    }
}

/// One tool → one Finding: on PATH (Ok) / off-PATH (Warn, functional —
/// clave uses the absolute path) / missing (Problem + remediation).
pub fn diagnose_tool(tool: ToolId, fact: &ToolFact, mgr: Option<PkgManager>, home: &Path) -> Finding {
    let group = tool_group(tool);
    let name = tool.bin_name();
    match &fact.discovered {
        None => Finding {
            group,
            severity: Severity::Problem,
            label: format!("{name} not found"),
            advice: missing_advice(tool, mgr),
        },
        Some(d) => {
            let shown = tilde(&d.path, home);
            // Off-PATH: works (we exec the absolute path) but worth knowing.
            if d.via == Via::KnownLocation {
                return Finding {
                    group,
                    severity: Severity::Warn,
                    label: format!("{name} found at {shown} — not on your PATH"),
                    advice: vec![
                        "clave will use this path directly; agent tabs are unaffected.".into(),
                        "Your interactive shell may still need it on PATH".into(),
                        "(a shell alias is not enough for spawned processes).".into(),
                    ],
                };
            }
            // Zellij only: version pinned to the validation ledger — any
            // drift (or an unparseable version) warns, never halts.
            if tool == ToolId::Zellij && fact.version.as_deref() != Some(TESTED_ZELLIJ) {
                let got = fact.version.as_deref().unwrap_or("unknown version");
                return Finding {
                    group,
                    severity: Severity::Warn,
                    label: format!("zellij {got} ({shown}) — clave is tested against {TESTED_ZELLIJ}"),
                    advice: vec![
                        "Permission-cache format and pane sizing are pinned to the tested".into(),
                        format!("version; if the bar misbehaves, install {TESTED_ZELLIJ}."),
                    ],
                };
            }
            let label = match &fact.version {
                Some(v) => format!("{name} {v} ({shown})"),
                None => format!("{name} ({shown})"),
            };
            Finding { group, severity: Severity::Ok, label, advice: vec![] }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace doctor 2>&1 | grep "test result"`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/doctor.rs
git commit -m "feat(clave): diagnose_tool — three-state discovery findings + version warn (spec §Check)"
```

---

### Task 6: diagnose — setup-state + environment checks, full `diagnose()`

**Files:**
- Modify: `crates/clave/src/doctor.rs`

**Interfaces:**
- Consumes: Tasks 4–5 types; `discover::semver_key`.
- Produces: `pub fn diagnose(f: &Facts) -> Vec<Finding>` (ordered: RequiredTools, AgentPicker, Setup, Environment).

- [ ] **Step 1: Write the failing tests**

```rust
    fn base_facts() -> Facts {
        Facts {
            home: PathBuf::from("/home/u"),
            zellij: found("/usr/local/bin/zellij", Via::PathLookup, Some("0.44.3")),
            claude: found("/home/u/.local/bin/claude", Via::PathLookup, Some("2.1.4")),
            git: found("/usr/bin/git", Via::PathLookup, Some("2.51.0")),
            fzf: found("/opt/homebrew/bin/fzf", Via::PathLookup, Some("0.60.0")),
            zoxide: found("/home/u/.cargo/bin/zoxide", Via::PathLookup, Some("0.9.6")),
            pkg_manager: Some(PkgManager::Brew),
            config_exists: true,
            layout_exists: true,
            wasm_path: PathBuf::from("/home/u/.local/share/clave/clave-bar-v0.1.0.wasm"),
            wasm_exists: true,
            has_embedded_wasm: true,
            hook_counts: crate::setup::HOOK_EVENTS.iter().map(|e| (e.to_string(), 1)).collect(),
            perms_seeded: true,
            bin_dir_exists: false,
            installed_releases: vec![],
            xdg_runtime_dir: None,
            version_line: "0.1.0 (dev)".into(),
        }
    }

    #[test]
    fn healthy_facts_produce_no_warns_or_problems() {
        let f = diagnose(&base_facts());
        assert!(f.iter().all(|x| x.severity == Severity::Ok), "{f:#?}");
        // Ordered: tools first, environment last; version info line present.
        assert_eq!(f.first().unwrap().group, Group::RequiredTools);
        assert_eq!(f.last().unwrap().group, Group::Environment);
        assert!(f.iter().any(|x| x.label.contains("clave 0.1.0 (dev)")));
    }

    #[test]
    fn missing_config_and_wasm_point_at_the_right_repair() {
        let mut facts = base_facts();
        facts.config_exists = false;
        facts.wasm_exists = false;
        let f = diagnose(&facts);
        let cfg = f.iter().find(|x| x.label.contains("config.kdl")).unwrap();
        assert_eq!(cfg.severity, Severity::Problem);
        assert!(cfg.advice.iter().any(|l| l.contains("clave setup")));
        // Embedded build → repair is `clave setup`; dev build → dev-install.
        let wasm = f.iter().find(|x| x.label.contains("wasm")).unwrap();
        assert!(wasm.advice.iter().any(|l| l.contains("clave setup")));
        facts.has_embedded_wasm = false;
        let f = diagnose(&facts);
        let wasm = f.iter().find(|x| x.label.contains("wasm")).unwrap();
        assert!(wasm.advice.iter().any(|l| l.contains("just dev-install")));
    }

    #[test]
    fn hook_problems_zero_and_duplicate() {
        let mut facts = base_facts();
        facts.hook_counts[1].1 = 0; // Stop unregistered
        let f = diagnose(&facts);
        assert!(f.iter().any(|x| x.severity == Severity::Problem && x.label.contains("hooks")));
        facts.hook_counts[1].1 = 2; // duplicate — Claude fires ALL matches
        let f = diagnose(&facts);
        let dup = f.iter().find(|x| x.label.contains("duplicate")).unwrap();
        assert_eq!(dup.severity, Severity::Problem);
        assert!(dup.advice.iter().any(|l| l.contains("double-fire")));
    }

    #[test]
    fn perms_unseeded_warns() {
        let mut facts = base_facts();
        facts.perms_seeded = false;
        let f = diagnose(&facts);
        let p = f.iter().find(|x| x.label.contains("permission")).unwrap();
        assert_eq!(p.severity, Severity::Warn);
        assert!(p.advice.iter().any(|l| l.contains("clave setup")));
    }

    #[test]
    fn skew_warns_only_when_dev_is_ahead_and_only_with_bin_dir() {
        let mut facts = base_facts();
        // No bin dir → end-user machine → NO skew finding at all.
        assert!(!diagnose(&facts).iter().any(|x| x.label.contains("release")));
        facts.bin_dir_exists = true;
        facts.installed_releases = vec!["0.1.0".into()];
        // current == newest → Ok mention.
        assert!(diagnose(&facts).iter().any(|x| x.severity == Severity::Ok && x.label.contains("0.1.0")));
        facts.version_line = "0.2.0 (dev)".into();
        let f = diagnose(&facts);
        let s = f.iter().find(|x| x.label.contains("ahead")).unwrap();
        assert_eq!(s.severity, Severity::Warn);
        assert!(s.advice.iter().any(|l| l.contains("unreleased")));
    }

    #[test]
    fn xdg_runtime_dir_ssh_trap() {
        let mut facts = base_facts();
        facts.xdg_runtime_dir = Some(false);
        let f = diagnose(&facts);
        let x = f.iter().find(|x| x.label.contains("XDG_RUNTIME_DIR")).unwrap();
        assert_eq!(x.severity, Severity::Warn);
        assert!(x.advice.iter().any(|l| l.contains("zellij-org/zellij#3708")));
        // None (macOS) → check skipped entirely.
        facts.xdg_runtime_dir = None;
        assert!(!diagnose(&facts).iter().any(|x| x.label.contains("XDG")));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --workspace diagnose 2>&1 | tail -3` — compile FAILURE (`diagnose` not found).

- [ ] **Step 3: Implement `diagnose()`**

```rust
/// The full catalogue (spec §Check catalogue), group-ordered. Version-line
/// semver comes from the leading token of version_line ("0.1.0 (dev)").
pub fn diagnose(f: &Facts) -> Vec<Finding> {
    let mut out = Vec::new();
    let mgr = f.pkg_manager;
    for (tool, fact) in [
        (ToolId::Zellij, &f.zellij),
        (ToolId::Claude, &f.claude),
        (ToolId::Git, &f.git),
        (ToolId::Fzf, &f.fzf),
        (ToolId::Zoxide, &f.zoxide),
    ] {
        out.push(diagnose_tool(tool, fact, mgr, &f.home));
    }

    // Setup state — every repair is `clave setup` (spec: one repair path),
    // except the wasm on a dev build, where placement belongs to the sandbox.
    let setup = |sev, label: String, advice: Vec<String>| Finding {
        group: Group::Setup,
        severity: sev,
        label,
        advice,
    };
    out.push(if f.config_exists && f.layout_exists {
        setup(Severity::Ok, "config.kdl + layout.kdl generated".into(), vec![])
    } else {
        setup(
            Severity::Problem,
            "config.kdl / layout.kdl not generated".into(),
            vec!["Run `clave setup`.".into()],
        )
    });
    out.push(if f.wasm_exists {
        setup(
            Severity::Ok,
            format!("clave-bar wasm present ({})", tilde(&f.wasm_path, &f.home)),
            vec![],
        )
    } else if f.has_embedded_wasm {
        setup(
            Severity::Problem,
            "clave-bar wasm not installed".into(),
            vec!["Run `clave setup` — this binary carries the wasm and will extract it.".into()],
        )
    } else {
        setup(
            Severity::Problem,
            "clave-bar wasm not installed (dev build — no embedded copy)".into(),
            vec!["Run `just dev-install` (builds the sandbox wasm).".into()],
        )
    });
    let missing: Vec<&str> = f
        .hook_counts
        .iter()
        .filter(|(_, n)| *n == 0)
        .map(|(e, _)| e.as_str())
        .collect();
    let dup: Vec<&str> = f
        .hook_counts
        .iter()
        .filter(|(_, n)| *n > 1)
        .map(|(e, _)| e.as_str())
        .collect();
    out.push(if !missing.is_empty() {
        setup(
            Severity::Problem,
            format!("Claude hooks not registered ({})", missing.join(", ")),
            vec!["Run `clave setup` — agents won't report status without them.".into()],
        )
    } else if !dup.is_empty() {
        setup(
            Severity::Problem,
            format!("duplicate clave hook entries ({})", dup.join(", ")),
            vec![
                "Claude fires ALL matching hooks — duplicates double-fire events.".into(),
                "Run `clave setup` to heal, or edit ~/.claude/settings.json.".into(),
            ],
        )
    } else {
        setup(Severity::Ok, "Claude hooks merged (1 entry per event)".into(), vec![])
    });
    out.push(if f.perms_seeded {
        setup(Severity::Ok, "Zellij plugin permissions pre-seeded".into(), vec![])
    } else {
        setup(
            Severity::Warn,
            "Zellij plugin permissions not pre-seeded".into(),
            vec!["Run `clave setup` — the first bar load will show an unanswerable prompt otherwise.".into()],
        )
    });
    // Release skew — maintainer machinery; end users (no <data>/bin) never
    // see it (spec §Check: conditional on the dir existing).
    if f.bin_dir_exists {
        let current = crate::discover::semver_key(f.version_line.split_whitespace().next().unwrap_or(""));
        let newest = f
            .installed_releases
            .iter()
            .filter_map(|v| crate::discover::semver_key(v).map(|k| (k, v.clone())))
            .max();
        match (current, newest) {
            (Some(c), Some((n, nv))) if c > n => out.push(setup(
                Severity::Warn,
                format!("this binary is ahead of the newest installed release (v{nv})"),
                vec![
                    "A stable launch will fall back to this dev binary — you are running".into(),
                    "unreleased code (CONTRIBUTING: the binary split).".into(),
                ],
            )),
            (_, Some((_, nv))) => {
                out.push(setup(Severity::Ok, format!("stable release installed (v{nv})"), vec![]))
            }
            _ => {}
        }
    }

    // Environment.
    if let Some(set) = f.xdg_runtime_dir {
        out.push(Finding {
            group: Group::Environment,
            severity: if set { Severity::Ok } else { Severity::Warn },
            label: if set {
                "XDG_RUNTIME_DIR set".into()
            } else {
                "XDG_RUNTIME_DIR unset — zellij session discovery is unreliable over SSH".into()
            },
            advice: if set {
                vec![]
            } else {
                vec![
                    "Sessions started locally may be invisible to SSH shells and vice".into(),
                    "versa (zellij-org/zellij#3708).".into(),
                ]
            },
        });
    }
    out.push(Finding {
        group: Group::Environment,
        severity: Severity::Ok,
        label: format!("clave {}", f.version_line),
        advice: vec![],
    });
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace doctor 2>&1 | grep "test result"` — all pass. `just clippy` clean.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/doctor.rs
git commit -m "feat(clave): diagnose() full catalogue — setup state, skew, XDG trap (spec §Check)"
```

---

### Task 7: renderers — grouped report + failures view (golden tests)

**Files:**
- Modify: `crates/clave/src/doctor.rs`

**Interfaces:**
- Consumes: `Finding`, `Group`, `Severity`.
- Produces: `pub fn render_report(findings: &[Finding], fancy: bool) -> String`, `pub fn render_failures(context: &str, findings: &[Finding]) -> String`.

- [ ] **Step 1: Write the failing golden tests**

```rust
    fn sample_findings() -> Vec<Finding> {
        vec![
            Finding { group: Group::RequiredTools, severity: Severity::Ok,
                label: "zellij 0.44.3 (/opt/homebrew/bin/zellij)".into(), advice: vec![] },
            Finding { group: Group::AgentPicker, severity: Severity::Problem,
                label: "fzf not found".into(),
                advice: vec![
                    "It is likely available from your package manager:".into(),
                    String::new(),
                    "    brew install fzf".into(),
                    String::new(),
                    "or see https://github.com/junegunn/fzf#installation".into(),
                ] },
            Finding { group: Group::Setup, severity: Severity::Warn,
                label: "Zellij plugin permissions not pre-seeded".into(),
                advice: vec!["Run `clave setup`.".into()] },
        ]
    }

    #[test]
    fn render_report_groups_glyphs_and_summary() {
        let s = render_report(&sample_findings(), true);
        let expected = "\
[✓] Required tools
    • zellij 0.44.3 (/opt/homebrew/bin/zellij)

[✗] Agent picker — needed by `clave add`
    ✗ fzf not found
      It is likely available from your package manager:

          brew install fzf

      or see https://github.com/junegunn/fzf#installation

[!] clave setup
    ! Zellij plugin permissions not pre-seeded
      Run `clave setup`.

! Doctor found issues in 2 categories.
";
        assert_eq!(s, expected);
    }

    #[test]
    fn render_report_ascii_fallback_when_not_a_tty() {
        let s = render_report(&sample_findings(), false);
        assert!(s.contains("[ok] Required tools"));
        assert!(s.contains("[x] Agent picker"));
        assert!(s.contains("    x fzf not found"));
        assert!(!s.contains('✓') && !s.contains('✗') && !s.contains('•'));
    }

    #[test]
    fn all_ok_report_says_so() {
        let ok = vec![Finding { group: Group::RequiredTools, severity: Severity::Ok,
            label: "git 2.51.0 (/usr/bin/git)".into(), advice: vec![] }];
        let s = render_report(&ok, true);
        assert!(s.ends_with("• No issues found!\n"));
    }

    #[test]
    fn render_failures_is_problems_only() {
        let s = render_failures("clave can't start — missing required tools:", &sample_findings());
        let expected = "\
clave can't start — missing required tools:

✗ fzf not found
  It is likely available from your package manager:

      brew install fzf

  or see https://github.com/junegunn/fzf#installation
";
        assert_eq!(s, expected);
        assert!(!s.contains("permissions")); // Warn not included
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --workspace render 2>&1 | tail -3` — compile FAILURE.

- [ ] **Step 3: Implement the renderers**

```rust
impl Group {
    fn title(self) -> &'static str {
        match self {
            Group::RequiredTools => "Required tools",
            Group::AgentPicker => "Agent picker — needed by `clave add`",
            Group::Setup => "clave setup",
            Group::Environment => "Environment",
        }
    }
}

const GROUP_ORDER: [Group; 4] = [Group::RequiredTools, Group::AgentPicker, Group::Setup, Group::Environment];

fn glyphs(fancy: bool) -> (&'static str, &'static str, &'static str) {
    // (ok-bullet, warn, problem) — degrades to ASCII off-TTY (spec §Arch).
    if fancy { ("•", "!", "✗") } else { ("-", "!", "x") }
}

fn header_glyph(sev: Severity, fancy: bool) -> &'static str {
    match (sev, fancy) {
        (Severity::Ok, true) => "✓",
        (Severity::Ok, false) => "ok",
        (Severity::Warn, _) => "!",
        (Severity::Problem, true) => "✗",
        (Severity::Problem, false) => "x",
    }
}

/// The grouped doctor view (spec §Reference output — golden-locked).
pub fn render_report(findings: &[Finding], fancy: bool) -> String {
    let (ok_b, warn_b, prob_b) = glyphs(fancy);
    let mut out = String::new();
    let mut bad_groups = 0;
    for g in GROUP_ORDER {
        let rows: Vec<&Finding> = findings.iter().filter(|f| f.group == g).collect();
        if rows.is_empty() {
            continue;
        }
        let worst = rows.iter().map(|f| f.severity).max().unwrap_or(Severity::Ok);
        if worst > Severity::Ok {
            bad_groups += 1;
        }
        out.push_str(&format!("[{}] {}\n", header_glyph(worst, fancy), g.title()));
        for f in rows {
            let bullet = match f.severity {
                Severity::Ok => ok_b,
                Severity::Warn => warn_b,
                Severity::Problem => prob_b,
            };
            out.push_str(&format!("    {bullet} {}\n", f.label));
            for line in &f.advice {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str(&format!("      {line}\n"));
                }
            }
        }
        out.push('\n');
    }
    // Flutter-style close.
    if bad_groups > 0 {
        out.push_str(&format!("! Doctor found issues in {bad_groups} categories.\n"));
    } else {
        out.push_str(&format!("{} No issues found!\n", if fancy { "•" } else { "-" }));
    }
    out
}

/// Preflight's failures-only view: identical Finding copy, no groups, no
/// clean-bill noise (spec §Preflight). Always fancy=… no — always plain
/// glyph ✗: preflight output goes to a terminal by construction (launch/add
/// are interactive); keep one form for golden stability.
pub fn render_failures(context: &str, findings: &[Finding]) -> String {
    let mut out = format!("{context}\n\n");
    for f in findings.iter().filter(|f| f.severity == Severity::Problem) {
        out.push_str(&format!("✗ {}\n", f.label));
        for line in &f.advice {
            if line.is_empty() {
                out.push('\n');
            } else {
                out.push_str(&format!("  {line}\n"));
            }
        }
    }
    out
}
```

- [ ] **Step 4: Run tests, fix goldens until exact**

Run: `cargo test --workspace render 2>&1 | grep "test result"` — all pass (iterate on whitespace until the goldens match exactly; the goldens are the spec).

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/doctor.rs
git commit -m "feat(clave): doctor renderers — grouped report + failures view, golden-locked"
```

---

### Task 8: `gather()` + `clave doctor` command (+ `--json`, exit code)

**Files:**
- Modify: `crates/clave/src/doctor.rs`
- Modify: `crates/clave/src/main.rs`

**Interfaces:**
- Consumes: everything above; `setup::{data_dir, wasm_path, permissions_cache_path, permissions_seeded, HOOK_EVENTS}`, `setup::is_clave_hook_command`, `env::claude_config_dir`, `release::{long_version, embedded_wasm}`, `discover::discover`.
- Produces: `pub fn gather() -> anyhow::Result<Facts>`, `pub fn run_doctor(json: bool) -> anyhow::Result<()>`; `clave doctor [--json]` wired in main. `pub fn hook_entry_counts(settings: &serde_json::Value) -> Vec<(String, usize)>`.

- [ ] **Step 1: Write the failing test (the one pure piece: hook counts)**

```rust
    #[test]
    fn hook_entry_counts_counts_only_clave_entries() {
        let mut settings = serde_json::json!({});
        crate::setup::merge_hooks(&mut settings, "clave");
        let counts = hook_entry_counts(&settings);
        assert_eq!(counts.len(), crate::setup::HOOK_EVENTS.len());
        assert!(counts.iter().all(|(_, n)| *n == 1));
        // A foreign hook on the same event does not count as ours.
        let counts = hook_entry_counts(&serde_json::json!({
            "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "my-bell hook Stop" } ] } ] }
        }));
        assert_eq!(counts.iter().find(|(e, _)| e == "Stop").unwrap().1, 0);
    }
```

- [ ] **Step 2: Run test to verify it fails** — `cargo test --workspace hook_entry_counts 2>&1 | tail -3` → compile FAILURE.

- [ ] **Step 3: Implement gather + run_doctor**

```rust
/// Count clave hook entries per event — reuses is_clave_hook_command, the
/// SAME matcher merge_hooks writes with (doctor never guesses a second form).
pub fn hook_entry_counts(settings: &serde_json::Value) -> Vec<(String, usize)> {
    crate::setup::HOOK_EVENTS
        .iter()
        .map(|ev| {
            let n = settings
                .get("hooks")
                .and_then(|h| h.get(*ev))
                .and_then(|a| a.as_array())
                .map(|entries| {
                    entries
                        .iter()
                        .flat_map(|e| e.get("hooks").and_then(|v| v.as_array()).into_iter().flatten())
                        .filter(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .is_some_and(|c| crate::setup::is_clave_hook_command(c, ev))
                        })
                        .count()
                })
                .unwrap_or(0);
            (ev.to_string(), n)
        })
        .collect()
}

fn tool_fact(tool: ToolId) -> ToolFact {
    let discovered = crate::discover::discover(tool);
    let version = discovered.as_ref().and_then(|d| {
        let out = std::process::Command::new(&d.path).arg("--version").output().ok()?;
        short_version(String::from_utf8_lossy(&out.stdout).lines().next()?)
    });
    ToolFact { discovered, version }
}

fn probe_pkg_manager() -> Option<PkgManager> {
    // Probe order = priority (spec §Probes). apk also at /sbin (off-PATH).
    for (bin, m) in [
        ("brew", PkgManager::Brew),
        ("apt-get", PkgManager::Apt),
        ("dnf", PkgManager::Dnf),
        ("pacman", PkgManager::Pacman),
        ("apk", PkgManager::Apk),
    ] {
        if which::which_global(bin).is_ok() {
            return Some(m);
        }
    }
    crate::discover::is_executable(std::path::Path::new("/sbin/apk")).then_some(PkgManager::Apk)
}

/// ALL the IO, one place (spec §Architecture). Every probe is best-effort:
/// gather() itself only fails on a missing home dir.
pub fn gather() -> anyhow::Result<Facts> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("home dir"))?;
    let dir = crate::setup::data_dir()?;
    let wasm_path = crate::setup::wasm_path()?;
    let settings: serde_json::Value = crate::env::claude_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let perms = crate::setup::permissions_cache_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let bin_dir = dir.join("bin");
    let installed_releases = std::fs::read_dir(&bin_dir)
        .map(|rd| {
            rd.filter_map(|e| {
                e.ok()?
                    .file_name()
                    .to_str()?
                    .strip_prefix("clave-v")
                    .map(str::to_string)
            })
            .collect()
        })
        .unwrap_or_default();
    Ok(Facts {
        home: home.clone(),
        zellij: tool_fact(ToolId::Zellij),
        claude: tool_fact(ToolId::Claude),
        git: tool_fact(ToolId::Git),
        fzf: tool_fact(ToolId::Fzf),
        zoxide: tool_fact(ToolId::Zoxide),
        pkg_manager: probe_pkg_manager(),
        config_exists: dir.join("config.kdl").exists(),
        layout_exists: dir.join("layout.kdl").exists(),
        wasm_exists: wasm_path.exists(),
        wasm_path,
        has_embedded_wasm: crate::release::embedded_wasm().is_some(),
        hook_counts: hook_entry_counts(&settings),
        perms_seeded: crate::setup::permissions_seeded(
            &perms,
            crate::setup::wasm_path()?.to_str().unwrap_or(""),
        ),
        bin_dir_exists: bin_dir.is_dir(),
        installed_releases,
        // Linux-only check (spec §Check): macOS zellij doesn't use it —
        // flagging there would be flutter#17781 noise.
        xdg_runtime_dir: cfg!(target_os = "linux")
            .then(|| std::env::var_os("XDG_RUNTIME_DIR").is_some()),
        version_line: crate::release::long_version(),
    })
}

/// `clave doctor`: report everything; exit 1 iff any Problem (mise's rule).
pub fn run_doctor(json: bool) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    let facts = gather()?;
    let findings = diagnose(&facts);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "facts": facts, "findings": findings }))?
        );
    } else {
        print!("{}", render_report(&findings, std::io::stdout().is_terminal()));
    }
    if findings.iter().any(|f| f.severity == Severity::Problem) {
        std::process::exit(1);
    }
    Ok(())
}
```

`main.rs` — add to `enum Command` (after `Setup`):

```rust
    /// Health report: required tools, picker deps, clave's own setup state,
    /// environment traps. Diagnose-only — `clave setup` is the repair path.
    Doctor {
        /// Emit facts + findings as JSON instead of the grouped report.
        #[arg(long)]
        json: bool,
    },
```

and to the match: `Some(Command::Doctor { json }) => clave::doctor::run_doctor(json),`

- [ ] **Step 4: Verify**

Run: `cargo test --workspace 2>&1 | grep "test result"` — all pass.
Run: `cargo run -p clave -- doctor; echo "exit=$?"` — eyeball the grouped report against the spec's reference output on this machine; exit reflects findings.
Run: `cargo run -p clave -- doctor --json | head -20` — valid JSON.
Run: `cargo run -p clave -- doctor | head -3` (piped → ASCII glyphs, no color).

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/doctor.rs crates/clave/src/main.rs
git commit -m "feat(clave): clave doctor — gather, --json, exit-on-problem (spec §Arch)"
```

---

### Task 9: preflight + launch integration + first-run flow

**Files:**
- Modify: `crates/clave/src/doctor.rs` (preflight)
- Modify: `crates/clave/src/setup.rs` (launch_session: preflight, discovered zellij, first-run)

**Interfaces:**
- Consumes: `discover::discover`, `missing_advice`, `render_failures`, `run_setup`.
- Produces: `doctor::preflight(required: &[ToolId], context: &str) -> anyhow::Result<()>`; `setup::confirm_proceed(is_tty: bool, input: Option<&str>) -> bool`; `setup::first_run_plan(data_dir: &Path, settings_path: &Path) -> String`.

- [ ] **Step 1: Write the failing tests**

`doctor.rs`:

```rust
    #[test]
    fn preflight_failure_text_carries_full_remediation() {
        // Pure path: build the failure the way preflight does.
        let missing = vec![Finding {
            group: Group::RequiredTools,
            severity: Severity::Problem,
            label: "zellij not found".into(),
            advice: missing_advice(ToolId::Zellij, None),
        }];
        let s = render_failures("clave can't start — missing required tools:", &missing);
        assert!(s.contains("zellij.dev/documentation/installation"));
        assert!(s.contains("github.com/zellij-org/zellij/releases"));
    }
```

`setup.rs`:

```rust
    #[test]
    fn confirm_proceed_tty_gating_and_default_yes() {
        // Never prompt without a TTY (Homebrew 2026 rule) → proceed.
        assert!(confirm_proceed(false, None));
        // TTY: empty/y/Y/yes proceed; n/no abort.
        for yes in ["", "y", "Y", "yes", " y "] {
            assert!(confirm_proceed(true, Some(yes)), "{yes:?}");
        }
        for no in ["n", "N", "no", "nope"] {
            assert!(!confirm_proceed(true, Some(no)), "{no:?}");
        }
    }

    #[test]
    fn first_run_plan_names_the_three_mutations() {
        let s = first_run_plan(Path::new("/home/u/.local/share/clave"), Path::new("/home/u/.claude/settings.json"));
        assert!(s.contains("First run"));
        assert!(s.contains("/home/u/.local/share/clave"));
        assert!(s.contains("/home/u/.claude/settings.json"));
        assert!(s.contains("additive"));
        assert!(s.contains("permission cache"));
    }
```

- [ ] **Step 2: Run to verify failures** — `cargo test --workspace confirm_proceed 2>&1 | tail -3` → compile FAILURE.

- [ ] **Step 3: Implement**

`doctor.rs`:

```rust
/// Per-command dependency gate (spec §Preflight): only UNDISCOVERABLE tools
/// halt — off-PATH finds pass silently (clave uses the absolute path).
/// Prints nothing on success, no clean-bill banner.
pub fn preflight(required: &[ToolId], context: &str) -> anyhow::Result<()> {
    let missing: Vec<Finding> = required
        .iter()
        .filter(|t| crate::discover::discover(**t).is_none())
        .map(|t| Finding {
            group: Group::RequiredTools,
            severity: Severity::Problem,
            label: format!("{} not found", t.bin_name()),
            advice: missing_advice(*t, probe_pkg_manager()),
        })
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("{}", render_failures(context, &missing)))
    }
}
```

`setup.rs`:

```rust
/// First-run consent (spec §First run): the plan prints ALWAYS; the prompt
/// fires only on a TTY — never prompt without one (Homebrew 2026). Pure
/// over (tty, read line) so the gate is unit-testable.
pub fn confirm_proceed(is_tty: bool, input: Option<&str>) -> bool {
    if !is_tty {
        return true; // invoking `clave` IS the named intent; setup is idempotent
    }
    matches!(
        input.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("") | Some("y") | Some("yes") | None
    )
}

pub fn first_run_plan(data_dir: &Path, settings_path: &Path) -> String {
    format!(
        "First run — clave needs to prepare this machine:\n\
         \n\
         \x20 • generate session config + layout in {}\n\
         \x20 • register status hooks in {} (additive — your existing hooks are never touched)\n\
         \x20 • pre-seed Zellij's plugin permission cache\n",
        data_dir.display(),
        settings_path.display()
    )
}
```

Also fix a pre-existing fresh-box bug found in Task 3's roundtrip: `run_setup`'s
settings.json write path assumes the config dir exists (`~/.claude` on real
machines, but a first-run box may lack it). In the settings-write section of
`write_generated`, before the `std::fs::write(&settings_path, …)` call, add:

```rust
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?; // fresh box: ~/.claude may not exist yet
    }
```

Rewire `launch_session` — at the very top add:

```rust
    // Preflight BEFORE anything (spec §Preflight): zellij because we exec
    // it; claude because the eager tab's spawn would otherwise fail INSIDE
    // a pane — the worst place to read an error.
    crate::doctor::preflight(
        &[crate::discover::ToolId::Zellij, crate::discover::ToolId::Claude],
        "clave can't start — missing required tools:",
    )?;
```

Replace `anyhow::ensure!(config.exists(), "run `clave setup` first");` with:

```rust
    if !config.exists() {
        // First run (spec §First run): plan → TTY-gated consent → setup.
        use std::io::IsTerminal;
        let settings = crate::env::claude_config_dir()?.join("settings.json");
        println!("{}", first_run_plan(&dir, &settings));
        let is_tty = std::io::stdin().is_terminal();
        let input = if is_tty {
            print!("Proceed? [Y/n] ");
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            Some(line)
        } else {
            None
        };
        anyhow::ensure!(
            confirm_proceed(is_tty, input.as_deref()),
            "aborted — run `clave setup` when ready"
        );
        run_setup()?;
    }
```

Then replace every `std::process::Command::new("zellij")` in `launch_session` (list-sessions, delete-session, and the final exec) with the discovered path:

```rust
    // Discovered once, used for every zellij invocation in this launch —
    // an off-PATH zellij (e.g. ~/.cargo/bin over SSH) still works (spec
    // §Discovery: found off-PATH ⇒ use the absolute path).
    let zellij = crate::discover::discover(crate::discover::ToolId::Zellij)
        .map(|d| d.path)
        .unwrap_or_else(|| std::path::PathBuf::from("zellij")); // preflight guarantees Some
```

(with `Command::new(&zellij)` at each site).

- [ ] **Step 4: Verify**

`cargo test --workspace 2>&1 | grep "test result"` — all pass. `just clippy` clean.
Do NOT launch a session to verify (Zellij lifecycle belongs to the human — CLAUDE.md). The unit tests cover the consent gate and plan text; live first-run validation is item 4 of the sandbox checklist at the end of this plan.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/doctor.rs crates/clave/src/setup.rs
git commit -m "feat(clave): launch preflight + one-command first run (spec §First run)"
```

---

### Task 10: spawn + add integration — discovered claude/fzf/zoxide, pane-hold

**Files:**
- Modify: `crates/clave/src/main.rs` (Spawn arm)
- Modify: `crates/clave/src/add.rs`

**Interfaces:**
- Consumes: `discover::{discover, ToolId}`, `doctor::preflight`.
- Produces: `add::hold_open_if_tty()`; spawn/add exec discovered absolute paths.

- [ ] **Step 1: Spawn arm — discovered claude**

In `main.rs`'s `Some(Command::Spawn { .. })` arm, before the `match mode`:

```rust
            // Discovered claude (spec §Discovery): the pane env may lack the
            // interactive PATH (nvm/local-install), so exec the absolute
            // path — resolved FRESH each spawn (the command is replayed on
            // resurrection and must survive reinstalls).
            let claude = clave::discover::discover(clave::discover::ToolId::Claude)
                .map(|d| d.path)
                .ok_or_else(|| anyhow::anyhow!(
                    "claude not found — install it: https://code.claude.com/docs\n\
                     (or set CLAVE_CLAUDE_BIN to its location)"
                ))?;
```

and change both `std::process::Command::new("claude")` to `std::process::Command::new(&claude)`.

- [ ] **Step 2: add.rs — preflight with pane-hold, discovered paths**

At the top of `run_add`:

```rust
    // Preflight (spec §Preflight): the fzf weave and git/claude are all
    // needed before any tab exists — abort BEFORE creating anything.
    if let Err(e) = crate::doctor::preflight(
        &[
            crate::discover::ToolId::Fzf,
            crate::discover::ToolId::Zoxide,
            crate::discover::ToolId::Git,
            crate::discover::ToolId::Claude,
        ],
        "clave add needs tools that are missing:",
    ) {
        eprintln!("{e}");
        eprintln!("You can install them from another tab without leaving the session.");
        hold_open_if_tty();
        anyhow::bail!("missing dependencies for `clave add`");
    }
```

Add:

```rust
/// The Alt+a keybind runs add in a floating pane with close_on_exit=true —
/// an abort's message would flash and VANISH (spec §Preflight pane-hold).
/// Block on Enter so the guidance is readable; TTY-gated so scripted
/// invocations never hang.
fn hold_open_if_tty() {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        eprintln!("\npress Enter to close");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}
```

Then swap the hardcoded invocations to discovered paths: in `fzf_pick`, change `Command::new("fzf")` to accept the path — add a module-level helper used by both call paths:

```rust
/// Discovered-or-bare tool path: preflight has already guaranteed presence,
/// so the fallback only preserves behavior if discovery races an uninstall.
fn tool_path(tool: crate::discover::ToolId) -> std::path::PathBuf {
    crate::discover::discover(tool)
        .map(|d| d.path)
        .unwrap_or_else(|| std::path::PathBuf::from(tool.bin_name()))
}
```

- `fzf_pick`: `Command::new(tool_path(crate::discover::ToolId::Fzf))`
- the zoxide call: `cmd_stdout` currently takes `"zoxide"` — pass `tool_path(ToolId::Zoxide)` (adjust `cmd_stdout`'s parameter from `&str` to `impl AsRef<std::ffi::OsStr>` — `Command::new` accepts it directly).

- [ ] **Step 3: Verify**

`cargo test --workspace 2>&1 | grep "test result"` — all pass (add.rs's pure tests unaffected). `just clippy` clean.
Live pane-hold behavior → sandbox validation checklist (human-driven, TESTING.md).

- [ ] **Step 4: Commit**

```bash
git add crates/clave/src/main.rs crates/clave/src/add.rs
git commit -m "feat(clave): spawn/add use discovered binaries; add preflight holds the pane (spec §Preflight)"
```

---

### Task 11: cargo-dist — attested releases with embedded wasm

**Files:**
- Create: `dist-workspace.toml` (via `dist init`)
- Create: `.github/workflows/release.yml` (generated by dist)
- Create: `.github/workflows/build-wasm-setup.yml` (build-setup steps)

**Note:** `dist` config surfaces move between versions — treat the snippets below as intent; the generated files are authoritative. `dist plan` locally is the verification gate; the full CI pipeline is validated at the first `v0.1.0` cut (spec §Distribution), NOT from this branch.

- [ ] **Step 1: Install and init (maintainer-run, answers documented)**

```bash
cargo install cargo-dist --locked
dist init --yes
```

Then edit the generated `dist-workspace.toml` so it contains (keys per the installed dist version — adjust names, keep intent):

```toml
[dist]
cargo-dist-version = "<whatever dist init pinned>"
ci = "github"
installers = ["shell"]          # checksummed + attested; README still leads with explicit download
targets = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-musl",   # static — no toolchain on the remote box (spec §Distribution)
    "aarch64-unknown-linux-musl",
]
github-attestations = true       # provenance beats cargo install (no crate-level attestations)
github-build-setup = "../build-wasm-setup.yml"
```

- [ ] **Step 2: Write the build-setup steps** (`.github/workflows/build-wasm-setup.yml`)

```yaml
# Injected into dist's build job BEFORE `cargo build` (spec §Distribution):
# build the bar wasm first, then export CLAVE_BAR_WASM so build.rs embeds it.
- name: Build clave-bar wasm
  run: |
    rustup target add wasm32-wasip1
    cargo build -p clave-bar --release --target wasm32-wasip1
    echo "CLAVE_BAR_WASM=$GITHUB_WORKSPACE/target/wasm32-wasip1/release/clave-bar.wasm" >> "$GITHUB_ENV"
    echo "CLAVE_BUILD_TAG=$(git describe --tags --exact-match HEAD 2>/dev/null || git rev-parse --short HEAD)" >> "$GITHUB_ENV"
```

- [ ] **Step 3: Verify locally**

```bash
dist plan            # validates config + computes artifacts without CI
just dist-build      # release-parity local build still green (Task 3)
cargo test --workspace 2>&1 | grep "test result"
```

Expected: `dist plan` lists the 4 targets + shell installer; tests green.

- [ ] **Step 4: Commit**

```bash
git add dist-workspace.toml .github/workflows/release.yml .github/workflows/build-wasm-setup.yml
git commit -m "feat(clave): cargo-dist config — attested releases, musl targets, embedded wasm (spec §Distribution)"
```

- [ ] **Step 5: File the deferred issues** (maintainer-approved, `gh issue create`): Nerd-Font/separator-glyph check; Homebrew tap on demand; first-cut CI validation checklist (release.yml run, attestation verify with `gh attestation verify`, scp-to-Linux-box smoke test).

---

## Live validation (sandbox, human-driven — after all tasks)

Per TESTING.md; the agent prepares, the human drives:

1. `just dev-install` → `clave dev scenario c8-cold-start` → human runs `clave dev launch` — confirm preflight is silent when healthy.
2. `clave doctor` in the sandbox — compare against the spec's reference output.
3. Rename fzf temporarily (`PATH` mask) → human presses Alt+a in the sandbox → guidance shows, pane holds until Enter.
4. Fresh-box first-run: `T=$(mktemp -d); CLAVE_DATA_DIR=$T CLAUDE_CONFIG_DIR=$T ./target/release/clave` (dist-build binary) in a plain terminal — plan → Y → setup → zellij session.
5. SSH box (or `env -u XDG_RUNTIME_DIR`) → doctor shows the Environment warn.
