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

use std::path::{Path, PathBuf};

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

/// The unversioned entry point a cut installs at `<data_dir>/bin/clave`
/// (#43a). This is the one name an operator TYPES, and the reason the release
/// has to own it: before this, `just release` installed `clave-vX.Y.Z` and
/// nothing else, so "how do I launch the version I just released?" had no
/// answer and whatever `clave` resolved to on PATH won the cold start. On
/// 2026-07-22 that was a stale 0.1.0 dev build, which generated a `launch.kdl`
/// baking v0.1.0 paths beside a v0.1.1 `config.kdl` — two plugin locations,
/// two bar instances, split navigation (#43).
///
/// Deliberately NOT `~/.cargo/bin/clave`: cargo owns that path, and writing
/// it from anything but cargo is the collision this whole issue is about.
/// `<data_dir>/bin` is clave's own directory — the operator puts it on PATH.
pub const LAUNCHER_NAME: &str = "clave";

/// Every file a cut of `version` installs under `dir`. Pure, so the install
/// destinations and the version-shaped names the generated artifacts
/// reference are derived in ONE place and cannot drift apart (#43 was a
/// drift between two artifact sets that no test compared).
pub struct ReleaseArtifacts {
    pub wasm: PathBuf,
    /// The versioned CLI copy. This is what generated config/layout/hooks
    /// bake, because a baked reference must be immutable across cuts.
    pub cli: PathBuf,
    /// The unversioned launcher. Typed, never baked — see `LAUNCHER_NAME`.
    pub launcher: PathBuf,
}

pub fn release_artifacts(dir: &Path, version: &str) -> ReleaseArtifacts {
    let bin = dir.join("bin");
    ReleaseArtifacts {
        wasm: dir.join(versioned_wasm_name(version)),
        cli: bin.join(versioned_cli_name(version)),
        launcher: bin.join(LAUNCHER_NAME),
    }
}

/// Install or REFRESH the unversioned launcher as a copy of `src` (#43a).
///
/// Refresh, not write-if-absent: `extract_embedded`/`install_cli_copy` are
/// write-if-absent because a live session is loading those exact versioned
/// files (§2 running-session immunity). Nothing loads the launcher — it is
/// only ever typed — so it must always name the newest cut, or the stale
/// entry point is back and #43a is merely relocated into the data dir.
///
/// Written via a temp file + rename, never `fs::copy` over the live name:
/// copy truncates the EXISTING inode, and that inode can be a running
/// process image (a cold start in flight). Linux refuses with ETXTBSY,
/// leaving the release half-installed; macOS overwrites a live text segment.
/// `rename` swaps only the directory entry, so anything already executing
/// keeps its own inode — the same atomicity the release model claims for
/// versioned artifacts, extended to the one mutable name.
pub fn install_launcher(bin_dir: &Path, src: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(bin_dir)?;
    let dest = bin_dir.join(LAUNCHER_NAME);
    // Same directory as the destination: rename is only atomic within one
    // filesystem, and $TMPDIR is routinely a different one.
    let tmp = bin_dir.join(format!(".{LAUNCHER_NAME}.{}.tmp", std::process::id()));
    std::fs::copy(src, &tmp)
        .with_context(|| format!("staging launcher {} → {}", src.display(), tmp.display()))?;
    // Every failure AFTER the staging copy removes it (CodeRabbit CLI,
    // 2026-07-25): a leftover dotfile in bin/ outlives the failed release and
    // is scanned by runtime_binary()'s #44 sibling probe. The closure holds
    // the whole staging window, not just the rename.
    let staged = |r: std::io::Result<()>| {
        r.inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Explicit, like the versioned copy: the launcher exists to be typed,
        // and fs::copy's mode carry-over is not worth trusting through a
        // staging file the umask also touched.
        staged(std::fs::set_permissions(
            &tmp,
            std::fs::Permissions::from_mode(0o755),
        ))
        .with_context(|| format!("making launcher executable: {}", tmp.display()))?;
    }
    staged(std::fs::rename(&tmp, &dest))
        .with_context(|| format!("installing launcher → {}", dest.display()))?;
    Ok(dest)
}

/// The operator-facing PATH guidance a cut prints (#43a). Two things must be
/// said and neither is guessable: which directory to put on PATH, and that a
/// pre-#43b `~/.cargo/bin/clave` — the stale dev build that caused the
/// 2026-07-22 outage — still shadows it. Every machine that ran the old
/// `just dev-install` has that file, and it wins on PATH by default.
pub fn launcher_hint(bin_dir: &Path) -> String {
    format!(
        "  launcher: {launcher}\n\
         \x20   Put {bin} on your PATH to launch this cut by typing `clave`.\n\
         \x20   If `command -v clave` still shows ~/.cargo/bin/clave, that is the\n\
         \x20   stale dev build (#43) — delete it, or it shadows this launcher.",
        launcher = bin_dir.join(LAUNCHER_NAME).display(),
        bin = bin_dir.display(),
    )
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
/// Keyed on the versioned copy's EXISTENCE, not `current_exe()` (split spec §2
/// Release mechanics — "stable sessions never invoke the dev binary: their
/// keybinds/layout/hooks bake the versioned copy's absolute path"): a stable
/// session is cold-started by typing `clave`, which resolves on PATH to the
/// unversioned launcher (`LAUNCHER_NAME` at `<data>/bin/clave` since #43a;
/// before that, whatever `clave` happened to be — the 2026-07-22 outage) — so
/// the launching process is never the versioned copy even in stable. The
/// installed versioned copy under the stable data dir is the reliable "this is
/// a release install" signal.
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
/// second bar. Pure over its inputs (`installed` = did our own version's copy
/// resolve; `siblings` = what is actually in `<data>/bin`) so it tests without
/// a filesystem.
pub fn binary_resolution_is_anomalous(installed: bool, siblings: &[String]) -> bool {
    // Require a DIGIT right after `clave-v`, not a bare `starts_with` — same
    // discipline as `is_clave_hook_command` (setup.rs), precedent cited there:
    // a foreign `clave-vault`/`clave-verify` sibling shares the text prefix
    // with our versioned copy name but is not one, and a false positive here
    // trains the reader to ignore the warning (the exact failure mode #44's
    // announcement exists to prevent).
    !installed
        && siblings.iter().any(|n| {
            n.strip_prefix("clave-v")
                .is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        })
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
            // Name the repair inline, NOT "run clave doctor": in the case this
            // fires (own version behind the newest installed copy) doctor
            // reports OK and gives no advice (doctor.rs), so deferring to it
            // dead-ends. Same `cp` CONTRIBUTING/CLAUDE.md carry.
            let bin = dir
                .as_ref()
                .map(|d| d.join("bin").display().to_string())
                .unwrap_or_else(|| "<data>/bin".into());
            // The repair advice is PATH ordering, not a copy (adversarial
            // review 2026-07-27). It used to say `cp <versioned>
            // $(command -v clave)`, which was right when bare `clave` meant
            // ~/.cargo/bin/clave — but since #43a the release owns an
            // unversioned launcher in this very directory, so on a correctly
            // configured machine that command expands to copying a file over
            // itself: a no-op that re-fires the warning forever, writes the
            // stable surface from a non-cut, and does it with `cp` — the
            // truncate-a-running-binary hazard install_launcher exists to
            // avoid.
            eprintln!(
                "clave: WARNING resolving bare `clave` on PATH, but {bin} holds \
                 a versioned copy ({}). config.kdl and the launch layout will \
                 disagree and each keybind may open a second bar (#44). Put \
                 {bin} FIRST on your PATH so `clave` is the release launcher. \
                 If something else still shadows it, check what it is first \
                 (`command -v clave; clave --version`) — a stale \
                 ~/.cargo/bin/clave from before #43b is the usual culprit, \
                 but it may be a deliberate install worth keeping.",
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
    let ReleaseArtifacts {
        wasm: wasm_dst,
        cli: cli_dst,
        launcher,
    } = release_artifacts(&dir, version);
    let bin_dir = cli_dst
        .parent()
        .context("versioned copy has no bin dir")?
        .to_path_buf();
    std::fs::create_dir_all(&bin_dir)?;

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

    // #43a: the cut owns the unversioned entry point. LAST, and from the
    // versioned copy we just installed rather than `cli_src`: the launcher
    // must never come to exist for a cut whose generation failed, and the
    // versioned copy is the file this release has already committed to.
    let installed = install_launcher(&bin_dir, &cli_dst)?;
    // The pure kernel and the IO that honours it must name the same file —
    // #43 WAS a drift between two artifact sets that nothing compared. An
    // `ensure!`, not a `debug_assert!` (CodeRabbit CLI, 2026-07-25): a cut is
    // built `--release`, which is exactly where a debug assertion is compiled
    // out, so the check would never run on the only path it guards.
    anyhow::ensure!(
        installed == launcher,
        "launcher installed at {} but the release names {} — the install and \
         the artifact set disagree (#43)",
        installed.display(),
        launcher.display()
    );
    println!(
        "released v{version}:\n  {}\n  {}\n{}",
        wasm_dst.display(),
        cli_dst.display(),
        launcher_hint(&bin_dir)
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

        // False-positive guard (review finding on #44): a foreign sibling
        // sharing the `clave-v` TEXT prefix but no digit after it — e.g. a
        // `clave-vault`/`clave-verify` binary someone else put in the same
        // bin dir — must not be treated as our versioned copy.
        assert!(!binary_resolution_is_anomalous(
            false,
            &["clave-vault".into()]
        ));
        assert!(!binary_resolution_is_anomalous(
            false,
            &["clave-verify".into()]
        ));
    }

    #[test]
    fn release_artifacts_names_the_versioned_pair_and_the_unversioned_launcher() {
        let a = release_artifacts(Path::new("/data/clave"), "0.1.1");
        assert_eq!(a.wasm, PathBuf::from("/data/clave/clave-bar-v0.1.1.wasm"));
        assert_eq!(a.cli, PathBuf::from("/data/clave/bin/clave-v0.1.1"));
        // #43a: the launcher is the one name an operator TYPES, so it must
        // carry no version — a versioned launcher answers the question the
        // versioned copy already answers, and leaves "how do I launch what I
        // just released?" unanswered all over again.
        assert_eq!(a.launcher, PathBuf::from("/data/clave/bin/clave"));
        assert_eq!(a.launcher.parent(), a.cli.parent()); // one bin/ on PATH
    }

    #[test]
    fn install_launcher_refreshes_on_every_cut() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let src = dir.path().join("clave-built");

        std::fs::write(&src, b"binary-v0.1.0").unwrap();
        let launcher = install_launcher(&bin, &src).unwrap();
        assert_eq!(launcher, bin.join("clave"));
        assert_eq!(std::fs::read(&launcher).unwrap(), b"binary-v0.1.0");

        // REFRESH, not write-if-absent — the opposite of install_cli_copy /
        // extract_embedded (§2 running-session immunity applies to the
        // VERSIONED files a live session loaded; the launcher is loaded by
        // nothing and must name the newest cut, or #43a's "whatever `clave`
        // resolves to wins" is merely relocated into the data dir).
        std::fs::write(&src, b"binary-v0.1.1").unwrap();
        install_launcher(&bin, &src).unwrap();
        assert_eq!(std::fs::read(&launcher).unwrap(), b"binary-v0.1.1");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&launcher).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "the launcher is TYPED — it must exec");
        }
        // No debris beside it: a stray temp file in bin/ is a file an operator
        // could type by accident, and it is scanned by the #44 sibling probe.
        let names: Vec<String> = std::fs::read_dir(&bin)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["clave".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn install_launcher_replaces_the_directory_entry_rather_than_truncating() {
        // The load-bearing property (#43a): a refresh must be a rename over
        // the name, never a write THROUGH it. `fs::copy` truncates the
        // existing inode, and that inode may be a running process image — a
        // cold start in flight, or the very `clave` that shelled out to run
        // the cut. Linux answers ETXTBSY (the release fails half-installed);
        // macOS overwrites a live text segment. rename() swaps only the
        // directory entry, so anything already executing keeps its own inode.
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let src = dir.path().join("clave-built");

        std::fs::write(&src, b"v1").unwrap();
        let launcher = install_launcher(&bin, &src).unwrap();
        let before = std::fs::metadata(&launcher).unwrap().ino();

        std::fs::write(&src, b"v2").unwrap();
        install_launcher(&bin, &src).unwrap();
        let after = std::fs::metadata(&launcher).unwrap().ino();
        assert_ne!(
            before, after,
            "the launcher was rewritten in place — a running clave's text \
             segment would have been truncated under it"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_installed_launcher_actually_runs() {
        // The whole point of #43a is that an operator can TYPE it. A launcher
        // that lands non-executable is the same dead end as no launcher at
        // all, and `fs::copy`'s mode carry-over is not something to trust
        // across the staging file (umask applies to the created temp).
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("clave-built");
        std::fs::write(&src, "#!/bin/sh\necho launched\n").unwrap();
        let launcher = install_launcher(&dir.path().join("bin"), &src).unwrap();
        let out = std::process::Command::new(&launcher).output().unwrap();
        assert!(out.status.success(), "launcher did not execute: {out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "launched");
    }

    #[test]
    fn the_launcher_is_never_read_as_a_versioned_copy() {
        // The launcher now SITS IN the directory runtime_binary() scans for
        // the #44 divergence warning. `clave` has no digit after `clave-v`
        // (it has no `clave-v` at all), so it must not, on its own, make a
        // dev/sandbox launch look anomalous — that warning firing on every
        // sandbox run is how a real warning gets ignored (the false-positive
        // discipline this function already carries for `clave-vault`).
        assert!(!binary_resolution_is_anomalous(false, &["clave".into()]));
        // But a launcher BESIDE a versioned copy, while we resolve neither,
        // is still the #44 anomaly: the versioned copy is the evidence.
        assert!(binary_resolution_is_anomalous(
            false,
            &["clave".into(), "clave-v0.1.1".into()]
        ));
        assert!(!binary_resolution_is_anomalous(
            true,
            &["clave".into(), "clave-v0.1.1".into()]
        ));
    }

    #[test]
    fn launcher_hint_names_the_bin_dir_and_the_shadow_that_caused_the_outage() {
        let hint = launcher_hint(Path::new("/data/clave/bin"));
        // The operator has to be told the ONE directory to put on PATH.
        assert!(hint.contains("/data/clave/bin"));
        // ...and that a pre-#43b `~/.cargo/bin/clave` still shadows it. That
        // stale file IS the v0.1.1 outage; a hint that omits it leaves every
        // already-broken machine broken.
        assert!(hint.contains(".cargo/bin/clave"));
    }

    /// Every path the generated KDL references under `root`, `file:` scheme
    /// stripped. Test-only tokenizer: KDL quotes, braces, semicolons and
    /// whitespace cannot appear inside these paths, so splitting on them
    /// yields whole path tokens (same discipline as setup.rs's `versions_in`).
    fn referenced_paths(text: &str, root: &str) -> Vec<String> {
        text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '{' | '}' | ';'))
            .map(|t| t.strip_prefix("file:").unwrap_or(t))
            .filter(|t| t.starts_with(root))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn released_artifacts_exist_and_the_launcher_is_never_baked() {
        // #48's cheap companion, from the install side: setup.rs's
        // `generated_artifact_set_is_version_coherent` proves the generated
        // set agrees on ONE version; this proves those references are the
        // files a cut actually INSTALLS, and that they exist on disk.
        // Hermetic: `release_artifacts` is pure, so the paths are built the
        // same way `run_release` builds them, against a temp data dir.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap().to_string();
        let a = release_artifacts(dir.path(), "0.1.1");
        std::fs::create_dir_all(a.cli.parent().unwrap()).unwrap();
        for p in [&a.wasm, &a.cli, &a.launcher] {
            std::fs::write(p, b"x").unwrap();
        }

        let cfg = crate::setup::config_kdl(
            a.cli.to_str().unwrap(),
            a.wasm.to_str().unwrap(),
            clave_types::RowHeight::Double,
        );
        let lay = crate::setup::layout_kdl(
            a.cli.to_str().unwrap(),
            a.wasm.to_str().unwrap(),
            clave_types::RowHeight::Double,
        );
        for (name, text) in [("config", &cfg), ("layout", &lay)] {
            let refs = referenced_paths(text, &root);
            assert!(!refs.is_empty(), "{name}.kdl referenced no installed path");
            for r in &refs {
                assert!(
                    Path::new(r).exists(),
                    "{name}.kdl references {r}, which no cut installs"
                );
                // The launcher is a TYPED entry point, never a baked one:
                // zellij keys plugin identity on (location, configuration),
                // and an unversioned reference is a different identity from
                // the versioned one every other artifact carries — a second
                // bar, verbatim #43. It also floats across cuts, which is the
                // one thing a baked reference must never do.
                assert_ne!(
                    Path::new(r),
                    a.launcher,
                    "{name}.kdl bakes the unversioned launcher (#43a)"
                );
            }
        }
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
