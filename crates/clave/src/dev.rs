//! `clave dev` (§6.9): the sandboxed live-validation harness. One command
//! seeds a named, repeatable world state; Ollie drives the checklist in a
//! real `clave-test` session; Claude reads clave.log + `dev status`.
//! Real tabs, real spawns, real jsonls — only the conversation CONTENT is
//! trivial (`claude -p` one-liners). Deliberately minimal: a fixture
//! seeder plus a log — no recorder, no assertion runner, no CI.
//!
//! Session lifecycle stays Ollie's: this module NEVER launches or kills
//! zellij sessions — it prints the commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

pub struct ScenarioAgent {
    pub slug: &'static str,
    /// Seconds before "now" for last_interacted — staggers recency so the
    /// eager-load / dormant-order expectations are deterministic.
    pub ago_secs: u64,
    pub worktree: bool,
    /// c8-stale: delete the agent's cwd AFTER seeding its jsonl, so the
    /// row's dwell-open hits the §6.3 staleness branch.
    pub delete_cwd_after: bool,
}

pub struct Scenario {
    pub name: &'static str,
    pub agents: &'static [ScenarioAgent],
}

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "c8-cold-start",
        agents: &[
            ScenarioAgent {
                slug: "recent",
                ago_secs: 60,
                worktree: false,
                delete_cwd_after: false,
            },
            ScenarioAgent {
                slug: "mid",
                ago_secs: 3_600,
                worktree: false,
                delete_cwd_after: false,
            },
            ScenarioAgent {
                slug: "old",
                ago_secs: 86_400,
                worktree: false,
                delete_cwd_after: false,
            },
        ],
    },
    Scenario {
        name: "c8-worktree",
        agents: &[
            ScenarioAgent {
                slug: "main",
                ago_secs: 60,
                worktree: false,
                delete_cwd_after: false,
            },
            ScenarioAgent {
                slug: "wt",
                ago_secs: 3_600,
                worktree: true,
                delete_cwd_after: false,
            },
        ],
    },
    Scenario {
        name: "c8-stale",
        agents: &[
            ScenarioAgent {
                slug: "alive",
                ago_secs: 60,
                worktree: false,
                delete_cwd_after: false,
            },
            ScenarioAgent {
                slug: "gone",
                ago_secs: 3_600,
                worktree: false,
                delete_cwd_after: true,
            },
        ],
    },
];

/// Valid v4-shaped, deterministic, self-identifying (`c85c` ≈ c8 scenario).
pub fn scenario_uuid(n: u32) -> String {
    format!("00000000-0000-4000-8000-c85c{n:08}")
}

pub fn sandbox_root() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("home")?
        .join(".local/state/clave-dev"))
}

/// The ONE command printed for Ollie to launch the sandboxed session.
///
/// Deliberately NO CLAUDE_CONFIG_DIR (revised 2026-07-18, live finding +
/// user ruling): sandboxing claude's identity dragged auth along with it
/// ("Not logged in" / stale-credential failures) — clave is a thin wrapper
/// for terminal control, and claude's identity is not its business. The
/// sandbox isolates CLAVE's state only; scenario transcripts land in the
/// real ~/.claude/projects tagged by the deterministic c85c uuids, and
/// `dev reset` removes them by that tag.
pub fn launch_command(root: &Path) -> String {
    format!(
        "CLAVE_SESSION=clave-test CLAVE_STATE_DIR={0}/state CLAVE_DATA_DIR={0}/data clave",
        root.display()
    )
}

/// Point THIS process at the sandbox (children inherit — the seeding
/// `claude -p` runs as the REAL user identity but its hook invocations
/// inherit CLAVE_STATE_DIR and land in the sandbox store).
fn enter_sandbox(root: &Path) {
    // SAFETY: single-threaded CLI entry point; set before any spawn.
    unsafe {
        std::env::set_var("CLAVE_SESSION", "clave-test");
        std::env::set_var("CLAVE_STATE_DIR", root.join("state"));
        std::env::set_var("CLAVE_DATA_DIR", root.join("data"));
    }
}

/// `clave dev launch`: the sandbox session in one short command — sets the
/// sandbox env (children inherit) and execs the NORMAL launch path.
/// Session lifecycle stays the user's: this exists to be typed BY the
/// user in a non-zellij terminal, replacing the printed env-var wall.
pub fn run_launch() -> Result<()> {
    let root = sandbox_root()?;
    enter_sandbox(&root);
    crate::setup::launch_session()
}

pub fn run_scenario(name: &str) -> Result<()> {
    let sc = SCENARIOS.iter().find(|s| s.name == name).with_context(|| {
        let names: Vec<_> = SCENARIOS.iter().map(|s| s.name).collect();
        format!("unknown scenario {name}; have: {names:?}")
    })?;
    let root = sandbox_root()?;
    enter_sandbox(&root);
    for d in ["state", "data", "repos"] {
        std::fs::create_dir_all(root.join(d))?;
    }
    // NO claude-identity sandboxing (2026-07-18 ruling — see
    // launch_command): claude runs as the real user; transcripts go to the
    // real ~/.claude/projects and are c85c-tagged for reset cleanup. Hooks
    // are already registered in the real settings.json (run_setup below
    // re-merges idempotently); hook processes inherit CLAVE_STATE_DIR from
    // their claude parent, so events still land in the SANDBOX store.
    // Sandbox clave config/layout: run the normal setup against the sandbox
    // dirs (env already points there). The unversioned `clave-bar.wasm` is
    // built straight into the sandbox data dir by `just dev-install` (§2 —
    // the stable dir now holds only VERSIONED wasm, so there is nothing to
    // copy from there); run_setup ensures it exists with a pointer to
    // dev-install if not.
    crate::setup::run_setup()?;

    let now = crate::store::now_unix();
    let paths = crate::store::store_paths()?;
    // A `?` mid-loop leaves the sandbox partially seeded — that's fine: it's
    // fully recoverable with `clave dev reset` (wipes scenario state; see
    // SCENARIO_STATE_DIRS — the build artifact in data/ survives).
    for (i, a) in sc.agents.iter().enumerate() {
        let uuid = scenario_uuid(i as u32 + 1);
        let repo = root.join("repos").join(format!("{name}-{}", a.slug));
        std::fs::create_dir_all(&repo)?;
        // -b main: pin the branch — else init.defaultBranch (maybe `master`)
        // would disagree with the store row's hardcoded `branch: "main"`.
        run_in(&repo, "git", &["init", "-q", "-b", "main"])?;
        run_in(
            &repo,
            "git",
            &["commit", "--allow-empty", "-q", "-m", "seed"],
        )?;
        let cwd = if a.worktree {
            let wt = repo.join(".claude-worktrees").join(&uuid[..8]);
            run_in(
                &repo,
                "git",
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    &format!("clave/{}", &uuid[..8]),
                    wt.to_str().context("wt")?,
                ],
            )?;
            wt
        } else {
            repo.clone()
        };
        let cwd = std::fs::canonicalize(&cwd)?; // S0b: claude munges getcwd()
        let cwd_str = cwd.to_str().context("cwd utf8")?.to_string();
        // A REAL resumable jsonl for a few tokens (§6.9): resume-with-
        // history is verified for real, not mocked. Resume-or-create like
        // spawn (S0): scenario UUIDs are deterministic and claude's identity
        // is never sandboxed, so a prior run's transcript persists and
        // `--session-id` reuse is REFUSED — an existing jsonl means this
        // agent is already seeded, which is the goal state, not an error.
        if seed_needed(&crate::env::claude_config_dir()?, &cwd_str, &uuid) {
            println!("seeding {uuid} ({})…", a.slug);
            // Discovered claude (coderabbit CLI, 2026-07-22): a contributor
            // whose claude lives off PATH (nvm, ~/.claude/local) could not
            // seed a scenario at all. Unlike dev.rs's zellij calls — session
            // lifecycle the human drives — this is a real exec clave owns.
            let st = Command::new(crate::discover::tool_path(crate::discover::ToolId::Claude))
                .current_dir(&cwd)
                .args(["-p", "--session-id", &uuid, "Reply with exactly: ok"])
                .status()
                .context("running claude -p (is claude discoverable?)")?;
            anyhow::ensure!(st.success(), "claude -p seeding failed for {uuid}");
        } else {
            println!(
                "{uuid} ({}) already seeded — reusing its transcript",
                a.slug
            );
        }
        crate::store::with_store_mut(&paths, |s| {
            s.agents.insert(
                uuid.clone(),
                crate::store::AgentRecord {
                    uuid: uuid.clone(),
                    cwd: cwd_str.clone(),
                    repo_root: repo.to_string_lossy().into_owned(),
                    branch: if a.worktree {
                        format!("clave/{}", &uuid[..8])
                    } else {
                        "main".into()
                    },
                    label: format!("{}-{} · seeded", name, a.slug),
                    status: clave_types::Status::Idle,
                    last_interacted: now.saturating_sub(a.ago_secs),
                    last_visited: 0,
                    worktree: a.worktree.then(|| cwd_str.clone()),
                    label_source: crate::store::LabelSource::FirstPrompt,
                    claude_codex: false,
                    tab_id: None,
                    stale: false,
                },
            );
            s.seq += 1;
        })?;
        if a.delete_cwd_after {
            std::fs::remove_dir_all(&cwd)?; // the §6.3 staleness fixture
        }
    }
    crate::evlog::log_event("dev", &format!("scenario {name} seeded"));
    println!("\nScenario `{name}` ready. Launch (your command, in a NON-zellij terminal):\n");
    println!("  clave dev launch");
    println!("\n(equivalent env form: {})", launch_command(&root));
    println!("\nWhen done: `clave dev reset` (prints the kill command first).");
    Ok(())
}

pub fn run_status() -> Result<()> {
    let root = sandbox_root()?;
    enter_sandbox(&root);
    let store = crate::store::read_store(&crate::store::store_paths()?)?;
    // Discovered zellij (2026-07-22): both reads below swallow failure with
    // unwrap_or_default, so an off-PATH zellij would report "no live session"
    // rather than erroring — and CLAUDE.md tells agents to gate the session
    // lifecycle on exactly this output. A false negative here is worse than
    // a loud failure.
    let zellij = crate::discover::tool_path(crate::discover::ToolId::Zellij);
    let list = Command::new(&zellij)
        .args(["list-sessions", "-n"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let live_session = crate::setup::session_is_live(&list, "clave-test");
    // Sanctioned §6.9 read: explicitly clave-test-scoped. GATED on
    // liveness (live finding, 2026-07-18): `zellij action` against an
    // absent/dead session BLOCKS indefinitely instead of erroring —
    // an ungated dump-layout hung `dev status` for minutes pre-launch.
    let dump = if live_session {
        Command::new(&zellij)
            .env("ZELLIJ_SESSION_NAME", "clave-test")
            .args(["action", "dump-layout"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    } else {
        String::new()
    };
    println!(
        "{}",
        serde_json::json!({
            "session_live": live_session,
            "live_uuids": crate::add::live_uuids(&dump),
            "store": store,
        })
    );
    Ok(())
}

/// Is this file a scenario-seeded transcript? The deterministic uuid
/// prefix (scenario_uuid) doubles as the cleanup tag — with claude
/// identity un-sandboxed (2026-07-18), scenario jsonls live in the REAL
/// ~/.claude/projects and reset must remove exactly them, nothing else.
pub fn is_scenario_jsonl(file_name: &str) -> bool {
    file_name.starts_with("00000000-0000-4000-8000-c85c") && file_name.ends_with(".jsonl")
}

/// Scenario-state subdirs `dev reset` wipes. Deliberately EXCLUDES `data/`:
/// that dir holds `clave-bar.wasm`, a build artifact installed once by
/// `just dev-install`, not scenario state seeded by `dev scenario`. Wiping
/// it used to break the documented reset → scenario → launch lifecycle —
/// the next scenario's `run_setup` finds no wasm and aborts asking for a
/// rebuild the user never asked for.
const SCENARIO_STATE_DIRS: [&str; 2] = ["state", "repos"];

/// Remove each of `SCENARIO_STATE_DIRS` under `root` that exists, leaving
/// `data/` (and anything else) untouched. Returns the subset actually
/// removed, for the caller's status message. Pure enough to unit-test
/// against a tempdir — the real entry point is `run_reset`, which always
/// calls this with the real `sandbox_root()`.
fn wipe_scenario_state(root: &Path) -> Result<Vec<&'static str>> {
    let mut wiped = Vec::new();
    for d in SCENARIO_STATE_DIRS {
        let p = root.join(d);
        if p.exists() {
            std::fs::remove_dir_all(&p)?;
            wiped.push(d);
        }
    }
    Ok(wiped)
}

pub fn run_reset() -> Result<()> {
    let root = sandbox_root()?;
    println!("If the session is running, kill it first (your command):\n");
    println!("  zellij kill-session clave-test && zellij delete-session --force clave-test\n");
    let wiped = wipe_scenario_state(&root)?;
    if wiped.is_empty() {
        println!("Scenario state already clean: {}", root.display());
    } else {
        println!(
            "Scenario state wiped ({}): {}",
            wiped.join(", "),
            root.display()
        );
    }
    // Scenario transcripts in the real claude tree (c85c-tagged, see
    // is_scenario_jsonl). Best-effort walk of projects/*/: a vanished dir
    // or unreadable entry only skips itself.
    let projects = crate::env::claude_config_dir()?.join("projects");
    let mut removed = 0u32;
    if let Ok(rd) = std::fs::read_dir(&projects) {
        for proj in rd.flatten() {
            if let Ok(files) = std::fs::read_dir(proj.path()) {
                for f in files.flatten() {
                    let name = f.file_name().to_string_lossy().into_owned();
                    if is_scenario_jsonl(&name) && std::fs::remove_file(f.path()).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
    }
    println!(
        "Scenario transcripts removed from {}: {removed}",
        projects.display()
    );
    Ok(())
}

fn run_in(dir: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    let st = Command::new(cmd)
        .current_dir(dir)
        .args(args)
        .status()
        .with_context(|| format!("running {cmd}"))?;
    anyhow::ensure!(st.success(), "{cmd} {args:?} failed in {}", dir.display());
    Ok(())
}

/// Does this scenario agent still need its `claude -p` seed? Existence of
/// the munged jsonl drives the branch (S0 — the same rule `claude --resume`
/// itself enforces), via the SAME `spawn_mode` check the pane path uses, so
/// scenario seeding (§6.9) and pane spawning can never disagree about what
/// "already exists" means.
fn seed_needed(claude_dir: &Path, physical_cwd: &str, uuid: &str) -> bool {
    matches!(
        crate::spawn::spawn_mode(claude_dir, physical_cwd, uuid),
        crate::spawn::SpawnMode::Create
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeding_skips_an_already_seeded_session() {
        // Deterministic scenario UUIDs + never-sandboxed claude identity
        // (§6.9 ruling) ⇒ a prior run's transcript persists in the REAL
        // ~/.claude, and claude REFUSES --session-id reuse ("already in
        // use", found live 2026-07-22). An existing jsonl is the GOAL
        // state, not an error — resume-or-create, exactly like spawn (S0).
        let claude = tempfile::tempdir().unwrap();
        let cwd = "/tmp/clave-dev/repos/c8-cold-start-x";
        let uuid = scenario_uuid(1);
        assert!(seed_needed(claude.path(), cwd, &uuid));
        let jsonl = crate::spawn::jsonl_path(claude.path(), cwd, &uuid);
        std::fs::create_dir_all(jsonl.parent().unwrap()).unwrap();
        std::fs::write(&jsonl, "{}").unwrap();
        assert!(!seed_needed(claude.path(), cwd, &uuid));
    }

    #[test]
    fn scenario_state_dirs_excludes_the_data_build_artifact() {
        // Fix: `dev reset` used to remove_dir_all the whole sandbox root,
        // deleting data/clave-bar.wasm (a `just dev-install` build artifact,
        // not scenario state) and silently breaking reset → scenario →
        // launch. Reset must target ONLY scenario state.
        assert_eq!(SCENARIO_STATE_DIRS, ["state", "repos"]);
        assert!(!SCENARIO_STATE_DIRS.contains(&"data"));
    }

    #[test]
    fn wipe_scenario_state_removes_state_and_repos_but_preserves_data() {
        // Behavioral proof of the fix, against a real tempdir (never the
        // real sandbox root): state/ and repos/ go, data/clave-bar.wasm —
        // the just-dev-install build artifact — survives untouched.
        let root =
            std::env::temp_dir().join(format!("clave-wipe-scenario-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root); // clean slate if a prior run leaked
        std::fs::create_dir_all(root.join("state")).unwrap();
        std::fs::create_dir_all(root.join("repos")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data").join("clave-bar.wasm"), b"wasm").unwrap();

        let wiped = wipe_scenario_state(&root).unwrap();

        assert_eq!(wiped, vec!["state", "repos"]);
        assert!(!root.join("state").exists());
        assert!(!root.join("repos").exists());
        assert!(root.join("data").join("clave-bar.wasm").exists()); // survives

        std::fs::remove_dir_all(&root).unwrap(); // test cleanup
    }

    #[test]
    fn wipe_scenario_state_is_a_noop_on_an_already_clean_root() {
        // No state/ or repos/ present (e.g. reset run twice in a row):
        // nothing to remove, no error, empty report.
        let root = std::env::temp_dir().join(format!(
            "clave-wipe-scenario-state-clean-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        assert_eq!(wipe_scenario_state(&root).unwrap(), Vec::<&str>::new());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn scenario_table_covers_the_c8_checklist() {
        // Names map 1:1 to SUBSYSTEM-VALIDATION.md C8 steps.
        let names: Vec<&str> = SCENARIOS.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["c8-cold-start", "c8-worktree", "c8-stale"]);
        // cold-start: 3 agents, staggered recency, none worktree.
        let cs = &SCENARIOS[0];
        assert_eq!(cs.agents.len(), 3);
        assert!(cs.agents.iter().all(|a| !a.worktree && !a.delete_cwd_after));
        // worktree: exactly one worktree agent.
        assert!(SCENARIOS[1].agents.iter().any(|a| a.worktree));
        // stale: exactly one agent whose cwd the scenario deletes.
        assert!(SCENARIOS[2].agents.iter().any(|a| a.delete_cwd_after));
    }

    #[test]
    fn scenario_uuids_are_valid_deterministic_and_readable() {
        // `claude --session-id` requires a real UUID; c85c ≈ "c8 scenario"
        // makes them self-identifying in clave.log / dump-layout.
        let u = scenario_uuid(1);
        assert_eq!(u, "00000000-0000-4000-8000-c85c00000001");
        assert!(uuid::Uuid::parse_str(&u).is_ok());
        assert_ne!(scenario_uuid(2), u);
    }

    #[test]
    fn launch_command_sandboxes_clave_state_only() {
        // §6.9 revised 2026-07-18: CLAVE state is sandboxed; claude's
        // identity is deliberately NOT (thin-wrapper ruling — sandboxing
        // it dragged auth along and broke seeding).
        let cmd = launch_command(std::path::Path::new("/sb"));
        assert!(cmd.contains("CLAVE_SESSION=clave-test"));
        assert!(cmd.contains("CLAVE_STATE_DIR=/sb/state"));
        assert!(cmd.contains("CLAVE_DATA_DIR=/sb/data"));
        assert!(!cmd.contains("CLAUDE_CONFIG_DIR"));
        assert!(cmd.trim_end().ends_with("clave"));
    }

    #[test]
    fn scenario_jsonl_tag_matches_exactly_the_seeded_uuids() {
        // The cleanup tag must cover every scenario_uuid and nothing a
        // real session could plausibly produce (v4 uuids are random).
        assert!(is_scenario_jsonl(&format!("{}.jsonl", scenario_uuid(1))));
        assert!(is_scenario_jsonl(&format!("{}.jsonl", scenario_uuid(99))));
        assert!(!is_scenario_jsonl(
            "a1b2c3d4-0000-4000-8000-c85c00000001.jsonl" // wrong prefix
        ));
        assert!(!is_scenario_jsonl(
            "00000000-0000-4000-8000-c85c00000001.json" // not a transcript
        ));
    }
}
