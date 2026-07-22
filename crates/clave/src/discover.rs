//! Binary discovery beyond PATH (spec §Discovery). PATH is not ground truth:
//! tools live in places only interactive shells know about (nvm bins, the
//! Claude local-install dir), and clave's exec contexts — zellij panes,
//! keybind-spawned commands — don't inherit the user's interactive PATH.
//! Resolution: explicit override var → which_global → curated locations.
//! Found off-PATH ⇒ clave USES the absolute path (the runtime_binary() idiom
//! extended): the user's shell config is their business; clave just works.

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
    // Fix 5 (review 2026-07-22): a missing $HOME must NOT kill an override or
    // PATH hit that was already found — known-location probing is the only
    // part that needs home, so scope the None to it (headless daemons with no
    // home still resolve via override/PATH).
    let known_hits: Vec<PathBuf> = dirs::home_dir()
        .map(|home| {
            let nvm_versions: Vec<String> = std::fs::read_dir(home.join(".nvm/versions/node"))
                .map(|rd| rd.filter_map(|e| Some(e.ok()?.file_name().to_str()?.to_string())).collect())
                .unwrap_or_default();
            candidate_dirs(tool, &home, &nvm_versions)
                .into_iter()
                .map(|d| d.join(tool.bin_name()))
                // ~/.claude/local/claude: candidate_dirs yields ~/.claude/local
                // as a DIR, so the join above already forms the full binary
                // path. apk's /sbin home is covered by the pkg-manager probe,
                // not tool discovery.
                .filter(|p| is_executable(p))
                .collect()
        })
        .unwrap_or_default();
    resolve(override_val.as_deref(), path_hit, &known_hits)
}

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
}
