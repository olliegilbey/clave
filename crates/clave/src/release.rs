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

/// IO wrapper over `baked_binary`: resolve the versioned copy under the
/// current environment's data dir and probe it. Sandbox data dirs
/// (`$CLAVE_DATA_DIR`) never hold a versioned copy → bare `clave`.
pub fn runtime_binary() -> String {
    let versioned = crate::setup::data_dir()
        .ok()
        .map(|d| d.join("bin").join(versioned_cli_name(env!("CARGO_PKG_VERSION"))));
    let installed = versioned.as_deref().is_some_and(Path::exists);
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
    if !status_porcelain.trim().is_empty() {
        return Err(format!(
            "working tree is dirty — commit or stash before a release cut:\n{}",
            status_porcelain.trim_end()
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
    std::fs::copy(wasm_src, &wasm_dst)
        .with_context(|| format!("installing wasm {} → {}", wasm_src.display(), wasm_dst.display()))?;
    std::fs::copy(cli_src, &cli_dst)
        .with_context(|| format!("installing cli {} → {}", cli_src.display(), cli_dst.display()))?;
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
    println!("released v{version}:\n  {}\n  {}", wasm_dst.display(), cli_dst.display());
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
        let e = release_gate(" M crates/clave/src/setup.rs\n", "v0.1.0\n", "0.1.0")
            .unwrap_err();
        assert!(e.contains("dirty"));
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
