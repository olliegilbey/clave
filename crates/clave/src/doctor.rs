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
}
