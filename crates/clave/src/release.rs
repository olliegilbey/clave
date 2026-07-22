//! `clave release` (§2): the version-cut mechanics. A cut is a semver git tag
//! on `main`; `just release` builds the release artifacts and hands them here.
//! This module owns everything version-shaped: the versioned artifact names,
//! the release gate (clean tree + a HEAD `vX.Y.Z` tag matching Cargo.toml),
//! the runtime binary choice (versioned copy vs dev `clave`), and the
//! install-and-regenerate weave. Pure kernels are unit-tested; the IO
//! orchestration (`run_release`) is not, matching `run_setup`/`run_scenario`.
//!
//! Why versioned artifacts at all: the daily environment must be immune to
//! working-tree builds while running AND between launches. A live session
//! only references the versioned files baked into its config at launch, so
//! installing a new release never disturbs it — the upgrade lands at the next
//! cold start (§2 running-session immunity).

use std::path::Path;

use anyhow::{Context, Result};

/// The stable wasm artifact name for a version: `clave-bar-vX.Y.Z.wasm`.
/// The sandbox keeps the unversioned `clave-bar.wasm` (working-tree builds).
pub fn versioned_wasm_name(version: &str) -> String {
    format!("clave-bar-v{version}.wasm")
}

/// The stable CLI copy name for a version: `clave-vX.Y.Z`. Installed under
/// `<data_dir>/bin/`; stable sessions bake its ABSOLUTE path so they never
/// invoke the dev `~/.cargo/bin/clave` (§2 binary split).
pub fn versioned_cli_name(version: &str) -> String {
    format!("clave-v{version}")
}

/// The bar wasm baked into this binary at build time, if any (spec
/// §Distribution). Empty marker ⇒ dev build ⇒ None: the sandbox flow owns
/// wasm placement there (just dev-install).
pub fn embedded_wasm() -> Option<&'static [u8]> {
    static BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/clave-bar.embedded"));
    (!BYTES.is_empty()).then_some(BYTES)
}

/// `clave --version` payload: semver + build tag. Mirrors the bar's load()
/// pattern (`option_env!("CLAVE_BUILD_TAG")` fallback `"dev"`) so "what am I
/// running" is answerable in both environments — `just release` bakes the
/// tag, `just dev-install` bakes the short SHA, a bare `cargo build` is `dev`.
pub fn long_version() -> String {
    format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("CLAVE_BUILD_TAG").unwrap_or("dev")
    )
}

/// The clave binary to bake into commands generated at RUNTIME (the launch
/// layout's eager-tab spawn, `clave add`/`clave open` tabs). Pure core: bake
/// the versioned copy's absolute path IFF it is installed (a stable machine),
/// else bare `clave` on PATH (dev/sandbox — the working-tree binary is what
/// should run there).
///
/// Keyed on the versioned copy's EXISTENCE, not `current_exe()`: a stable
/// session is cold-started by typing `clave`, which resolves on PATH to the
/// DEV binary (`~/.cargo/bin/clave`) — so the launching process is never the
/// versioned copy even in stable. The installed versioned copy under the
/// stable data dir is the reliable "this is a release install" signal.
pub fn baked_binary(versioned_cli: Option<&Path>, installed: bool) -> String {
    match versioned_cli {
        Some(p) if installed => p.to_string_lossy().into_owned(),
        _ => "clave".to_string(),
    }
}

/// Is resolving to bare `clave` an ANOMALY rather than the dev/sandbox norm?
///
/// True iff we are falling back to PATH while `<data>/bin/` already holds a
/// `clave-v*` copy. That is the #44 divergence: `config.kdl` was written with
/// one binary and the launch layout is about to bake another, so zellij's
/// (location, configuration) pipe match misses and every keybind launches a
/// second bar. Pure over its inputs so it tests without a filesystem.
///
/// No `versioned_cli: Option<&Path>` parameter: the brief's sketch carried
/// one, but the predicate never needs the CANDIDATE path, only whether it
/// resolved (`installed`) and what is actually sitting in `<data>/bin`
/// (`siblings`) — a `clave-v0.2.0` copy existing is anomalous evidence
/// whether or not this binary's own version happened to probe for it.
/// Keeping an ignored parameter (`let _ = …`) would just ship dead API
/// surface for callers to puzzle over.
pub fn binary_resolution_is_anomalous(installed: bool, siblings: &[String]) -> bool {
    !installed && siblings.iter().any(|n| n.starts_with("clave-v"))
}

/// IO wrapper over `baked_binary`: resolve the versioned copy under the
/// current environment's data dir and probe it. Sandbox data dirs
/// (`$CLAVE_DATA_DIR`) never hold a versioned copy → bare `clave`.
pub fn runtime_binary() -> String {
    let dir = crate::setup::data_dir().ok();
    let versioned = dir.as_ref().map(|d| {
        d.join("bin")
            .join(versioned_cli_name(env!("CARGO_PKG_VERSION")))
    });
    let installed = versioned.as_deref().is_some_and(Path::exists);

    // #44: announce the divergence, don't just take it. Falling back to PATH
    // while a versioned copy sits beside us means config.kdl and the launch
    // layout will disagree about plugin configuration — and zellij treats a
    // configuration mismatch as a DIFFERENT plugin, so each keybind press
    // spawns a second bar. Cheap to detect here; invisible in production.
    if !installed {
        let siblings: Vec<String> = dir
            .as_ref()
            .and_then(|d| std::fs::read_dir(d.join("bin")).ok())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        if binary_resolution_is_anomalous(installed, &siblings) {
            eprintln!(
                "clave: WARNING resolving bare `clave` on PATH, but {} holds \
                 a versioned copy ({}). config.kdl and the launch layout will \
                 disagree and each keybind may open a second bar — run \
                 `clave doctor` (#44).",
                dir.as_ref()
                    .map(|d| d.join("bin").display().to_string())
                    .unwrap_or_else(|| "<data>/bin".into()),
                siblings.join(", ")
            );
        }
    }

    baked_binary(versioned.as_deref(), installed)
}

/// The release gate (§2): refuse unless the working tree is clean AND HEAD
/// carries the exact `vX.Y.Z` tag matching Cargo.toml's version. Pure over
/// the command output strings so it is unit-tested without a live repo:
/// - `status_porcelain` = `git status --porcelain` (empty ⇒ clean),
/// - `head_tags` = `git tag --points-at HEAD` (newline-separated),
/// - `cargo_version` = the release binary's own `CARGO_PKG_VERSION`.
pub fn release_gate(
    status_porcelain: &str,
    head_tags: &str,
    cargo_version: &str,
) -> std::result::Result<(), String> {
    // Untracked (`?? `) lines are exempt ONLY under doc/local-tooling paths
    // (first-cut finding + CodeRabbit P1, 2026-07-21): an in-progress
    // handoff or local agent state must not block a cut, but a blanket
    // untracked exemption is unsound — tracked code can REFERENCE an
    // untracked file (e.g. a `mod foo;` whose `foo.rs` was never added),
    // which builds locally yet cannot be reproduced from the tagged HEAD.
    // docs/ and .claude/ and AGENTS.md are never cargo build inputs; any
    // other untracked path refuses like tracked dirt does.
    const UNTRACKED_EXEMPT: [&str; 3] = ["docs/", ".claude/", "AGENTS.md"];
    let tracked_dirt: Vec<&str> = status_porcelain
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| match l.strip_prefix("?? ") {
            Some(path) => !UNTRACKED_EXEMPT.iter().any(|p| path.starts_with(p)),
            None => true,
        })
        .collect();
    if !tracked_dirt.is_empty() {
        return Err(format!(
            "working tree is dirty — commit or stash before a release cut:\n{}",
            tracked_dirt.join("\n")
        ));
    }
    let want = format!("v{cargo_version}");
    let tagged = head_tags.lines().any(|l| l.trim() == want);
    if !tagged {
        return Err(format!(
            "HEAD is not tagged {want} (Cargo.toml is {cargo_version}) — \
             `git tag {want}` at the commit you want to cut. HEAD tags: {:?}",
            head_tags.split_whitespace().collect::<Vec<_>>()
        ));
    }
    Ok(())
}

fn git_output(args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("running git {args:?}"))?;
    // status/tag exit 0 in a normal repo; a non-zero here means we're not in
    // one — surface it rather than silently treating the tree as clean.
    anyhow::ensure!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `clave release` (invoked by `just release`): gate, then install the
/// versioned artifacts and regenerate stable config/layout/hooks so every
/// generated reference points at the versioned paths. `wasm_src`/`cli_src`
/// are the freshly built release artifacts the justfile hands us — keeping
/// the build in `just` and the version-shaped install here means every
/// versioned filename is derived (and unit-tested) in one place.
pub fn run_release(wasm_src: &Path, cli_src: &Path) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    // Gate FIRST — nothing is installed on a dirty tree or an untagged HEAD.
    let status = git_output(&["status", "--porcelain"])?;
    let tags = git_output(&["tag", "--points-at", "HEAD"])?;
    release_gate(&status, &tags, version).map_err(|e| anyhow::anyhow!(e))?;

    // Stable data dir — NOT a sandbox: a release is always the real install
    // (`$CLAVE_DATA_DIR` would only be set inside `clave dev`, which never
    // cuts releases). data_dir() honors the override regardless, harmlessly.
    let dir = crate::setup::data_dir()?;
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let wasm_dst = dir.join(versioned_wasm_name(version));
    let cli_dst = bin_dir.join(versioned_cli_name(version));

    // Copy the built artifacts to their versioned homes. A live session
    // loads only the files baked into ITS config, so writing NEW versioned
    // files never disturbs it — the upgrade is atomic at the next launch.
    std::fs::copy(wasm_src, &wasm_dst).with_context(|| {
        format!(
            "installing wasm {} → {}",
            wasm_src.display(),
            wasm_dst.display()
        )
    })?;
    std::fs::copy(cli_src, &cli_dst).with_context(|| {
        format!(
            "installing cli {} → {}",
            cli_src.display(),
            cli_dst.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // fs::copy carries the mode, but be explicit: the versioned copy MUST
        // be executable — the keybinds/layout invoke it by absolute path.
        std::fs::set_permissions(&cli_dst, std::fs::Permissions::from_mode(0o755))?;
    }

    let wasm_str = wasm_dst.to_str().context("wasm path utf8")?;
    let cli_str = cli_dst.to_str().context("cli path utf8")?;
    // Regenerate stable config/layout/hooks at the VERSIONED paths: keybind
    // `Run` and pane `command` bake the absolute CLI copy; the plugin
    // location and hook commands bake the versioned wasm/CLI. merge_hooks
    // replaces any prior clave hook entry (old version, or the dev bare
    // `clave`) rather than duplicating it.
    crate::setup::write_generated(&dir, cli_str, wasm_str)?;
    println!(
        "released v{version}:\n  {}\n  {}",
        wasm_dst.display(),
        cli_dst.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn versioned_names_embed_the_version() {
        assert_eq!(versioned_wasm_name("0.1.0"), "clave-bar-v0.1.0.wasm");
        assert_eq!(versioned_cli_name("0.1.0"), "clave-v0.1.0");
        // A later cut changes both names — old artifacts are never overwritten.
        assert_eq!(versioned_wasm_name("1.2.3"), "clave-bar-v1.2.3.wasm");
        assert_eq!(versioned_cli_name("1.2.3"), "clave-v1.2.3");
    }

    #[test]
    fn baked_binary_picks_versioned_when_installed_else_bare_clave() {
        let v = PathBuf::from("/home/o/.local/share/clave/bin/clave-v0.1.0");
        // Stable: versioned copy installed → bake its absolute path.
        assert_eq!(baked_binary(Some(&v), true), v.to_string_lossy());
        // Sandbox/dev: copy absent → bare `clave` on PATH (the dev binary).
        assert_eq!(baked_binary(Some(&v), false), "clave");
        // No data dir resolvable at all → bare `clave`.
        assert_eq!(baked_binary(None, false), "clave");
        assert_eq!(baked_binary(None, true), "clave");
    }

    #[test]
    fn dev_builds_embed_no_wasm() {
        // cargo test runs without CLAVE_BAR_WASM → empty marker → None.
        assert!(embedded_wasm().is_none());
    }

    #[test]
    fn long_version_carries_semver_and_a_build_tag() {
        let v = long_version();
        assert!(v.starts_with(env!("CARGO_PKG_VERSION")));
        // A bare `cargo test` build has no CLAVE_BUILD_TAG → "dev".
        assert!(v.contains('(') && v.ends_with(')'));
        assert!(v.contains(option_env!("CLAVE_BUILD_TAG").unwrap_or("dev")));
    }

    #[test]
    fn release_gate_passes_only_on_clean_tree_with_matching_tag() {
        // Clean tree, HEAD tagged exactly v0.1.0 → OK.
        assert!(release_gate("", "v0.1.0\n", "0.1.0").is_ok());
        // Tag among several HEAD tags still passes.
        assert!(release_gate("", "some-other\nv0.1.0\nlatest\n", "0.1.0").is_ok());
    }

    #[test]
    fn release_gate_refuses_dirty_tree() {
        let e = release_gate(" M crates/clave/src/setup.rs\n", "v0.1.0\n", "0.1.0").unwrap_err();
        assert!(e.contains("dirty"));
    }

    #[test]
    fn release_gate_ignores_untracked_files() {
        // First-cut finding (2026-07-21): status handoffs live UNTRACKED in
        // docs/status/ by convention, so `?? ` lines must not block a cut —
        // untracked files are not in the tagged HEAD, so they cannot make
        // the build differ from the tag. Tracked modifications still refuse,
        // including when mixed with untracked noise.
        assert!(release_gate("?? docs/status/x.md\n?? AGENTS.md\n", "v0.1.0\n", "0.1.0").is_ok());
        assert!(release_gate("?? .claude/worktrees/x/y.rs\n", "v0.1.0\n", "0.1.0").is_ok());
        // CodeRabbit P1 (2026-07-21): an untracked file OUTSIDE the doc/
        // local-tooling allowlist can be a build input a tracked `mod`
        // references — it builds locally but the tagged HEAD cannot
        // reproduce the artifact. Refuse it like tracked dirt.
        let e = release_gate("?? crates/clave/src/foo.rs\n", "v0.1.0\n", "0.1.0").unwrap_err();
        assert!(e.contains("dirty"));
        let e =
            release_gate("?? docs/status/x.md\n M src/lib.rs\n", "v0.1.0\n", "0.1.0").unwrap_err();
        assert!(e.contains("dirty"));
    }

    #[test]
    fn anomalous_only_when_a_versioned_copy_exists_but_is_not_used() {
        // Dev/sandbox: no versioned copy anywhere. Bare `clave` is CORRECT here
        // (baked_binary's contract) — warning would fire on every sandbox launch
        // and train the reader to ignore it.
        assert!(!binary_resolution_is_anomalous(false, &[]));

        // Stable, healthy: the versioned copy exists and is what we resolved.
        assert!(!binary_resolution_is_anomalous(
            true,
            &["clave-v0.1.1".into()]
        ));

        // THE ANOMALY (#44): we are about to bake bare `clave` even though a
        // versioned copy is sitting in the data dir — a version-skewed launcher.
        // config.kdl and launch.kdl will disagree, and every keybind press will
        // spawn a second bar.
        assert!(binary_resolution_is_anomalous(
            false,
            &["clave-v0.1.0".into()]
        ));
    }

    #[test]
    fn release_gate_refuses_missing_or_mismatched_tag() {
        // No tag at all.
        assert!(release_gate("", "", "0.1.0").is_err());
        // A tag, but for a different version than Cargo.toml.
        let e = release_gate("", "v0.2.0\n", "0.1.0").unwrap_err();
        assert!(e.contains("v0.1.0"));
        // A near-miss substring must NOT count (whole-line match).
        assert!(release_gate("", "v0.1.00\nxv0.1.0\n", "0.1.0").is_err());
    }
}
