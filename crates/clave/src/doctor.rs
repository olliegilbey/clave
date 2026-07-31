//! `clave doctor` + preflight (spec 2026-07-21). Pure-core/thin-shell:
//! gather() does ALL the IO; diagnose() is pure over Facts; both renderers
//! draw from the same Findings so doctor and preflight copy cannot drift
//! (the uv property, collection-shaped).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::discover::{Discovered, ToolId, Via, tilde};

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
    /// Does <data>/bin/clave — the unversioned launcher — exist? Cuts before
    /// v0.1.2 installed only the versioned copy (#43a landed in v0.1.2), so
    /// "a cut is installed" does NOT imply "a launcher is installed", and the
    /// skew warning says opposite things in the two cases. Doctor cannot see
    /// PATH (#48), but it can see this file.
    pub launcher_exists: bool,
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
                _ => (
                    "zoxide",
                    "https://github.com/ajeetdsouza/zoxide#installation",
                ),
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
pub fn diagnose_tool(
    tool: ToolId,
    fact: &ToolFact,
    mgr: Option<PkgManager>,
    home: &Path,
) -> Finding {
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
            // Zellij version check runs FIRST (coderabbit CLI, 2026-07-22):
            // ordering it after the off-PATH return meant an off-PATH zellij
            // with version drift reported only "not on your PATH" and lost
            // the version warning entirely — and an off-PATH cargo-installed
            // zellij is exactly where drift is most likely. Both facts are
            // Warn, so surfacing them together costs no severity, only text.
            if tool == ToolId::Zellij && fact.version.as_deref() != Some(TESTED_ZELLIJ) {
                let got = fact.version.as_deref().unwrap_or("unknown version");
                let mut advice = vec![
                    "Permission-cache format and pane sizing are pinned to the tested".into(),
                    format!("version; if the bar misbehaves, install {TESTED_ZELLIJ}."),
                ];
                if d.via == Via::KnownLocation {
                    advice.push("Also not on your PATH — clave uses this path directly.".into());
                }
                return Finding {
                    group,
                    severity: Severity::Warn,
                    label: format!(
                        "zellij {got} ({shown}) — clave is tested against {TESTED_ZELLIJ}"
                    ),
                    advice,
                };
            }
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
            let label = match &fact.version {
                Some(v) => format!("{name} {v} ({shown})"),
                None => format!("{name} ({shown})"),
            };
            Finding {
                group,
                severity: Severity::Ok,
                label,
                advice: vec![],
            }
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
        setup(
            Severity::Ok,
            "config.kdl + layout.kdl generated".into(),
            vec![],
        )
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
        // The path is IN the label, not just the Ok branch's: a dev build can
        // be pointed at either data dir, and only the sandbox one is what the
        // repair below fills. Seeing `~/.local/state/clave-dev/data` vs
        // `~/.local/share/clave` is how the reader tells which case they are in.
        setup(
            Severity::Problem,
            format!(
                "clave-bar wasm not installed (dev build — no embedded copy): {}",
                tilde(&f.wasm_path, &f.home)
            ),
            // `just sandbox`, NOT `just dev-install` (CONTRIBUTING §Quick start,
            // FOOTGUNS §PATH and version coherence): dev-install builds the same
            // wasm but never regenerates config.kdl, so it leaves a new wasm
            // beside a stale config — indistinguishable from "the fix didn't
            // work" — and it rebuilds in place under a live clave-test session.
            //
            // The last two lines are load-bearing, not a footnote: `just
            // sandbox` fills the SANDBOX data dir, so if this dev build is
            // aimed at the stable one the advised command changes nothing and
            // doctor repeats itself verbatim. The label above carries the path,
            // so the reader can tell which case they are in without a fact
            // doctor would have to guess (CLAVE_DATA_DIR is not the only way to
            // end up there).
            vec![
                "Run `just sandbox` — it builds the working-tree wasm into the sandbox".into(),
                "data dir (~/.local/state/clave-dev/data) and regenerates the config".into(),
                "that references it. (`just dev-install` builds the same wasm but".into(),
                "leaves config.kdl stale, and rebuilds in place under a live".into(),
                "clave-test session.)".into(),
                String::new(),
                "If the path above is NOT that directory, this dev build is aimed at".into(),
                "the stable data dir, which only a release cut fills — `just sandbox`".into(),
                "will not change what you see here.".into(),
            ],
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
    // Independent, not an if/else chain (coderabbit CLI, 2026-07-22): a
    // settings.json can have BOTH an unregistered event and a duplicated one
    // (a half-finished hand-edit), and the chain hid the duplicate — which is
    // the one that silently double-fires.
    if !missing.is_empty() {
        out.push(setup(
            Severity::Problem,
            format!("Claude hooks not registered ({})", missing.join(", ")),
            vec!["Run `clave setup` — agents won't report status without them.".into()],
        ));
    }
    if !dup.is_empty() {
        out.push(setup(
            Severity::Problem,
            format!("duplicate clave hook entries ({})", dup.join(", ")),
            vec![
                "Claude fires ALL matching hooks — duplicates double-fire events.".into(),
                "Run `clave setup` to heal, or edit ~/.claude/settings.json.".into(),
            ],
        ));
    }
    if missing.is_empty() && dup.is_empty() {
        out.push(setup(
            Severity::Ok,
            "Claude hooks merged (1 entry per event)".into(),
            vec![],
        ));
    }
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
        let current =
            crate::discover::semver_key(f.version_line.split_whitespace().next().unwrap_or(""));
        let newest = f
            .installed_releases
            .iter()
            .filter_map(|v| crate::discover::semver_key(v).map(|k| (k, v.clone())))
            .max();
        match (current, newest) {
            (Some(c), Some((n, nv))) if c > n => out.push(setup(
                Severity::Warn,
                format!("this binary is ahead of the newest installed release (v{nv})"),
                // The old copy said "a stable launch will fall back to this dev
                // binary". True before #43a, when nothing owned the name `clave`
                // and whatever PATH resolved won the cold start — that is the
                // 2026-07-22 outage. #43a gave the cut its own launcher at
                // <data>/bin/clave, so on a machine that HAS one the daily
                // surface is not reaching for this binary at all.
                //
                // But #43a shipped in v0.1.2: a machine whose newest cut is
                // older has installed releases and NO launcher, and there the
                // old copy was right. Branch on the file, which doctor can
                // see — never on the version, and never on PATH, which it
                // cannot (#48). Both branches end at `command -v clave`,
                // because even an installed launcher only wins if <data>/bin
                // comes first on PATH — an operator step (release.rs:245),
                // not something a cut can guarantee.
                {
                    let mut advice = vec![
                        "You are running unreleased code (CONTRIBUTING: Two environments,".into(),
                        "one code path).".into(),
                    ];
                    if f.launcher_exists {
                        // "There IS a launcher", not "the v{nv} cut installed
                        // one": the probe sees a file, not who wrote it, and a
                        // hand-placed copy is exactly the case where the
                        // attribution would be a lie told confidently.
                        advice.push(
                            "There is a launcher at <data>/bin/clave, so a cold start there".into(),
                        );
                        advice.push(format!(
                            "runs the installed v{nv} rather than this binary —"
                        ));
                        advice.push("provided <data>/bin comes first on your PATH.".into());
                    } else {
                        advice.push(
                            "There is NO launcher at <data>/bin/clave (cuts before v0.1.2".into(),
                        );
                        advice.push(
                            "installed only the versioned copy), so whatever `clave` resolves"
                                .into(),
                        );
                        advice.push("to on PATH wins the cold start — the #43 failure.".into());
                    }
                    advice.push(
                        "doctor cannot see your PATH: `command -v clave` says which binary".into(),
                    );
                    advice.push(
                        "actually wins (a pre-#43b ~/.cargo/bin/clave is the usual culprit)."
                            .into(),
                    );
                    advice
                },
            )),
            (_, Some((_, nv))) => out.push(setup(
                Severity::Ok,
                format!("stable release installed (v{nv})"),
                vec![],
            )),
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

const GROUP_ORDER: [Group; 4] = [
    Group::RequiredTools,
    Group::AgentPicker,
    Group::Setup,
    Group::Environment,
];

fn glyphs(fancy: bool) -> (&'static str, &'static str, &'static str) {
    // (ok-bullet, warn, problem) — degrades to ASCII off-TTY (spec §Arch).
    if fancy {
        ("•", "!", "✗")
    } else {
        ("-", "!", "x")
    }
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
        let worst = rows
            .iter()
            .map(|f| f.severity)
            .max()
            .unwrap_or(Severity::Ok);
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
    // Flutter-style close. Singular/plural (coderabbit CLI, 2026-07-22): the
    // golden test used a 2-category fixture, so "issues in 1 categories"
    // shipped unnoticed — the commonest real case.
    if bad_groups > 0 {
        let noun = if bad_groups == 1 {
            "category"
        } else {
            "categories"
        };
        out.push_str(&format!("! Doctor found issues in {bad_groups} {noun}.\n"));
    } else {
        out.push_str(&format!(
            "{} No issues found!\n",
            if fancy { "•" } else { "-" }
        ));
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
                        .flat_map(|e| {
                            e.get("hooks")
                                .and_then(|v| v.as_array())
                                .into_iter()
                                .flatten()
                        })
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
        let out = std::process::Command::new(&d.path)
            .arg("--version")
            .output()
            .ok()?;
        short_version(String::from_utf8_lossy(&out.stdout).lines().next()?)
    });
    ToolFact {
        discovered,
        version,
    }
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
        // Reuse the ONE resolved path (coderabbit CLI, 2026-07-22): calling
        // wasm_path() twice could report the permission grant against a
        // different file than the one shown, if extraction raced between.
        perms_seeded: crate::setup::permissions_seeded(&perms, wasm_path.to_str().unwrap_or("")),
        wasm_path,
        has_embedded_wasm: crate::release::embedded_wasm().is_some(),
        hook_counts: hook_entry_counts(&settings),
        bin_dir_exists: bin_dir.is_dir(),
        launcher_exists: bin_dir.join(crate::release::LAUNCHER_NAME).is_file(),
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
            serde_json::to_string_pretty(
                &serde_json::json!({ "facts": facts, "findings": findings })
            )?
        );
    } else {
        print!(
            "{}",
            render_report(&findings, std::io::stdout().is_terminal())
        );
    }
    if findings.iter().any(|f| f.severity == Severity::Problem) {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::{Discovered, ToolId, Via};
    use std::path::{Path, PathBuf};

    fn found(path: &str, via: Via, ver: Option<&str>) -> ToolFact {
        ToolFact {
            discovered: Some(Discovered {
                path: PathBuf::from(path),
                via,
            }),
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
        let f = found(
            "/home/u/.claude/local/claude",
            Via::KnownLocation,
            Some("2.1.4"),
        );
        let d = diagnose_tool(ToolId::Claude, &f, None, Path::new("/home/u"));
        assert_eq!(d.severity, Severity::Warn);
        assert!(d.label.contains("~/.claude/local/claude"));
        assert!(d.label.contains("not on your PATH"));
        assert!(
            d.advice
                .iter()
                .any(|l| l.contains("clave will use this path directly"))
        );
        assert!(d.advice.iter().any(|l| l.contains("alias is not enough")));
    }

    #[test]
    fn missing_tool_is_a_problem_with_remediation() {
        let none = ToolFact {
            discovered: None,
            version: None,
        };
        let d = diagnose_tool(ToolId::Fzf, &none, Some(PkgManager::Brew), Path::new("/h"));
        assert_eq!(d.severity, Severity::Problem);
        assert_eq!(d.label, "fzf not found");
        // Hedged pkg-manager line (flutter voice) + indented command + URL.
        assert!(
            d.advice
                .iter()
                .any(|l| l.contains("likely available from your package manager"))
        );
        assert!(d.advice.iter().any(|l| l == "    brew install fzf"));
        assert!(
            d.advice
                .iter()
                .any(|l| l.contains("github.com/junegunn/fzf"))
        );
    }

    #[test]
    fn zellij_and_claude_remediation_is_url_only() {
        // Even with a probed manager, NEVER print an install command for
        // these two (zellij absent from distro repos; InstallFix for claude).
        for (tool, url) in [
            (ToolId::Zellij, "zellij.dev"),
            (ToolId::Claude, "code.claude.com"),
        ] {
            let adv = missing_advice(tool, Some(PkgManager::Apt));
            assert!(adv.iter().any(|l| l.contains(url)), "{tool:?}");
            assert!(!adv.iter().any(|l| l.contains("apt-get")), "{tool:?}");
        }
        assert!(
            missing_advice(ToolId::Zellij, Some(PkgManager::Apt))
                .iter()
                .any(|l| l.contains("github.com/zellij-org/zellij/releases"))
        );
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
        assert_eq!(
            diagnose_tool(ToolId::Zellij, &ok, None, Path::new("/h")).severity,
            Severity::Ok
        );
        let weird = found("/u/zellij", Via::PathLookup, None);
        assert_eq!(
            diagnose_tool(ToolId::Zellij, &weird, None, Path::new("/h")).severity,
            Severity::Warn
        );
    }

    #[test]
    fn short_version_takes_first_numeric_token() {
        assert_eq!(short_version("zellij 0.44.3").as_deref(), Some("0.44.3"));
        assert_eq!(
            short_version("git version 2.51.0").as_deref(),
            Some("2.51.0")
        );
        assert_eq!(
            short_version("2.1.4 (Claude Code)").as_deref(),
            Some("2.1.4")
        );
        assert_eq!(short_version("v0.9.6").as_deref(), Some("v0.9.6"));
        assert_eq!(short_version("no digits here"), None);
        assert_eq!(short_version(""), None);
    }

    #[test]
    fn pkg_manager_install_lines_match_the_mise_prefixes() {
        assert_eq!(PkgManager::Brew.install_line("fzf"), "brew install fzf");
        assert_eq!(
            PkgManager::Apt.install_line("fzf"),
            "sudo apt-get install -y fzf"
        );
        assert_eq!(
            PkgManager::Dnf.install_line("fzf"),
            "sudo dnf install -y fzf"
        );
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
            hook_counts: crate::setup::HOOK_EVENTS
                .iter()
                .map(|e| (e.to_string(), 1))
                .collect(),
            perms_seeded: true,
            bin_dir_exists: false,
            launcher_exists: false,
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
        // Embedded build → repair is `clave setup`; dev build → `just sandbox`,
        // the only path that leaves a fresh wasm beside a config that names it.
        let wasm = f.iter().find(|x| x.label.contains("wasm")).unwrap();
        assert!(wasm.advice.iter().any(|l| l.contains("clave setup")));
        facts.has_embedded_wasm = false;
        // A real dev build falls through to the UNVERSIONED name (setup.rs:30):
        // the versioned artifact is a release's, and this arm is reached only
        // when there is no release here to have installed one.
        facts.wasm_path = PathBuf::from("/home/u/.local/share/clave/clave-bar.wasm");
        let f = diagnose(&facts);
        let wasm = f.iter().find(|x| x.label.contains("wasm")).unwrap();
        // The FIRST line is the repair — `contains` anywhere would also pass on
        // a revert to "Run `just dev-install`" that merely mentioned sandbox
        // later in the block.
        assert!(wasm.advice[0].contains("Run `just sandbox`"), "{wasm:#?}");
        // The path is what tells a dev build WHICH data dir came up empty, and
        // the advice must say so — `just sandbox` fills the sandbox dir, so
        // against the stable one the advised command changes nothing.
        assert!(
            wasm.label.contains("~/.local/share/clave/clave-bar.wasm"),
            "{}",
            wasm.label
        );
        assert!(wasm.advice.iter().any(|l| l.contains("stable data dir")));
    }

    #[test]
    fn hook_problems_zero_and_duplicate() {
        let mut facts = base_facts();
        facts.hook_counts[1].1 = 0; // Stop unregistered
        let f = diagnose(&facts);
        assert!(
            f.iter()
                .any(|x| x.severity == Severity::Problem && x.label.contains("hooks"))
        );
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
        assert!(
            diagnose(&facts)
                .iter()
                .any(|x| x.severity == Severity::Ok && x.label.contains("0.1.0"))
        );
        facts.version_line = "0.2.0 (dev)".into();
        let f = diagnose(&facts);
        let s = f.iter().find(|x| x.label.contains("ahead")).unwrap();
        assert_eq!(s.severity, Severity::Warn);
        assert!(s.advice.iter().any(|l| l.contains("unreleased")));
        // No launcher (the state every pre-v0.1.2 cut leaves): the warning must
        // say PATH decides the cold start. This is the case the old copy got
        // RIGHT, and the case a launcher-blind rewrite would invert into false
        // reassurance — the whole reason this branches on a probed file.
        let joined = s.advice.join(" ");
        assert!(joined.contains("NO launcher"), "{joined}");
        assert!(joined.contains("wins the cold start"), "{joined}");
        // Launcher present: the daily surface reaches the release, not this
        // binary — but only if <data>/bin is first on PATH, which doctor cannot
        // see (#48), so BOTH branches must hand over `command -v clave`.
        facts.launcher_exists = true;
        let f = diagnose(&facts);
        let s = f.iter().find(|x| x.label.contains("ahead")).unwrap();
        let joined_l = s.advice.join(" ");
        assert!(
            joined_l.contains("There is a launcher at <data>/bin/clave"),
            "{joined_l}"
        );
        assert!(joined_l.contains("runs the installed v0.1.0"), "{joined_l}");
        assert!(joined_l.contains("comes first on your PATH"), "{joined_l}");
        assert!(!joined_l.contains("NO launcher"), "{joined_l}");
        for j in [&joined, &joined_l] {
            assert!(j.contains("command -v clave"), "{j}");
        }
    }

    #[test]
    fn xdg_runtime_dir_ssh_trap() {
        let mut facts = base_facts();
        facts.xdg_runtime_dir = Some(false);
        let f = diagnose(&facts);
        let x = f
            .iter()
            .find(|x| x.label.contains("XDG_RUNTIME_DIR"))
            .unwrap();
        assert_eq!(x.severity, Severity::Warn);
        assert!(
            x.advice
                .iter()
                .any(|l| l.contains("zellij-org/zellij#3708"))
        );
        // None (macOS) → check skipped entirely.
        facts.xdg_runtime_dir = None;
        assert!(!diagnose(&facts).iter().any(|x| x.label.contains("XDG")));
    }

    fn sample_findings() -> Vec<Finding> {
        vec![
            Finding {
                group: Group::RequiredTools,
                severity: Severity::Ok,
                label: "zellij 0.44.3 (/opt/homebrew/bin/zellij)".into(),
                advice: vec![],
            },
            Finding {
                group: Group::AgentPicker,
                severity: Severity::Problem,
                label: "fzf not found".into(),
                advice: vec![
                    "It is likely available from your package manager:".into(),
                    String::new(),
                    "    brew install fzf".into(),
                    String::new(),
                    "or see https://github.com/junegunn/fzf#installation".into(),
                ],
            },
            Finding {
                group: Group::Setup,
                severity: Severity::Warn,
                label: "Zellij plugin permissions not pre-seeded".into(),
                advice: vec!["Run `clave setup`.".into()],
            },
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
    fn single_bad_group_uses_the_singular_noun() {
        // coderabbit CLI 2026-07-22: the 2-category golden hid "issues in 1
        // categories" — and one bad group is the commonest real case.
        let one = vec![Finding {
            group: Group::AgentPicker,
            severity: Severity::Problem,
            label: "fzf not found".into(),
            advice: vec![],
        }];
        assert!(render_report(&one, true).ends_with("! Doctor found issues in 1 category.\n"));
    }

    #[test]
    fn off_path_zellij_still_reports_version_drift() {
        // coderabbit CLI 2026-07-22: the off-PATH branch used to return
        // first, swallowing the version warning — and an off-PATH
        // cargo-installed zellij is exactly where drift is likeliest.
        let f = found(
            "/home/u/.cargo/bin/zellij",
            Via::KnownLocation,
            Some("0.45.0"),
        );
        let d = diagnose_tool(ToolId::Zellij, &f, None, Path::new("/home/u"));
        assert_eq!(d.severity, Severity::Warn);
        assert!(d.label.contains("0.45.0") && d.label.contains(TESTED_ZELLIJ));
        // Both facts surface: version drift AND the off-PATH note.
        assert!(d.advice.iter().any(|l| l.contains("not on your PATH")));
        // A tested-version off-PATH zellij keeps the plain off-PATH finding.
        let ok = found(
            "/home/u/.cargo/bin/zellij",
            Via::KnownLocation,
            Some(TESTED_ZELLIJ),
        );
        let d = diagnose_tool(ToolId::Zellij, &ok, None, Path::new("/home/u"));
        assert!(d.label.contains("not on your PATH"));
    }

    #[test]
    fn missing_and_duplicate_hooks_are_both_reported() {
        // coderabbit CLI 2026-07-22: the if/else chain hid a duplicate when
        // another event was also unregistered — the duplicate is the one that
        // silently double-fires.
        let mut facts = base_facts();
        facts.hook_counts[0].1 = 0; // UserPromptSubmit unregistered
        facts.hook_counts[1].1 = 2; // Stop duplicated
        let f = diagnose(&facts);
        assert!(f.iter().any(|x| x.label.contains("not registered")));
        assert!(f.iter().any(|x| x.label.contains("duplicate")));
    }

    #[test]
    fn all_ok_report_says_so() {
        let ok = vec![Finding {
            group: Group::RequiredTools,
            severity: Severity::Ok,
            label: "git 2.51.0 (/usr/bin/git)".into(),
            advice: vec![],
        }];
        let s = render_report(&ok, true);
        assert!(s.ends_with("• No issues found!\n"));
    }

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

    #[test]
    fn render_failures_is_problems_only() {
        let s = render_failures(
            "clave can't start — missing required tools:",
            &sample_findings(),
        );
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
}
