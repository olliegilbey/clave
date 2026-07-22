//! `clave doctor` + preflight (spec 2026-07-21). Pure-core/thin-shell:
//! gather() does ALL the IO; diagnose() is pure over Facts; both renderers
//! draw from the same Findings so doctor and preflight copy cannot drift
//! (the uv property, collection-shaped).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::discover::{tilde, Discovered, ToolId, Via};

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

#[cfg(test)]
mod tests {
    use super::*;
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
