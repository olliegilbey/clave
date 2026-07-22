//! `clave doctor` + preflight (spec 2026-07-21). Pure-core/thin-shell:
//! gather() does ALL the IO; diagnose() is pure over Facts; both renderers
//! draw from the same Findings so doctor and preflight copy cannot drift
//! (the uv property, collection-shaped).

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
