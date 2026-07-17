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
            ScenarioAgent { slug: "recent", ago_secs: 60, worktree: false, delete_cwd_after: false },
            ScenarioAgent { slug: "mid", ago_secs: 3_600, worktree: false, delete_cwd_after: false },
            ScenarioAgent { slug: "old", ago_secs: 86_400, worktree: false, delete_cwd_after: false },
        ],
    },
    Scenario {
        name: "c8-worktree",
        agents: &[
            ScenarioAgent { slug: "main", ago_secs: 60, worktree: false, delete_cwd_after: false },
            ScenarioAgent { slug: "wt", ago_secs: 3_600, worktree: true, delete_cwd_after: false },
        ],
    },
    Scenario {
        name: "c8-stale",
        agents: &[
            ScenarioAgent { slug: "alive", ago_secs: 60, worktree: false, delete_cwd_after: false },
            ScenarioAgent { slug: "gone", ago_secs: 3_600, worktree: false, delete_cwd_after: true },
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
pub fn launch_command(root: &Path) -> String {
    format!(
        "CLAVE_SESSION=clave-test CLAVE_STATE_DIR={0}/state CLAVE_DATA_DIR={0}/data CLAUDE_CONFIG_DIR={0}/claude clave",
        root.display()
    )
}

/// Point THIS process at the sandbox (children inherit — claude -p seeding
/// and run_setup both land inside).
fn enter_sandbox(root: &Path) {
    // SAFETY: single-threaded CLI entry point; set before any spawn.
    unsafe {
        std::env::set_var("CLAVE_SESSION", "clave-test");
        std::env::set_var("CLAVE_STATE_DIR", root.join("state"));
        std::env::set_var("CLAVE_DATA_DIR", root.join("data"));
        std::env::set_var("CLAUDE_CONFIG_DIR", root.join("claude"));
    }
}

pub fn run_scenario(name: &str) -> Result<()> {
    let sc = SCENARIOS
        .iter()
        .find(|s| s.name == name)
        .with_context(|| {
            let names: Vec<_> = SCENARIOS.iter().map(|s| s.name).collect();
            format!("unknown scenario {name}; have: {names:?}")
        })?;
    let root = sandbox_root()?;
    enter_sandbox(&root);
    for d in ["state", "data", "claude", "repos"] {
        std::fs::create_dir_all(root.join(d))?;
    }
    // Sandbox claude identity: onboarding/account state (~/.claude.json →
    // $CLAUDE_CONFIG_DIR/.claude.json). OAuth creds live in the macOS
    // Keychain, which is machine-ambient — headless `claude -p` works.
    let home = dirs::home_dir().context("home")?;
    let sandbox_cfg = root.join("claude/.claude.json");
    if !sandbox_cfg.exists() && home.join(".claude.json").exists() {
        std::fs::copy(home.join(".claude.json"), &sandbox_cfg)?;
    }
    // Sandbox hooks + config/layout + wasm: reuse the REAL wasm, then run
    // the normal setup against the sandbox dirs (env already points there).
    let real_wasm = home.join(".local/share/clave/clave-bar.wasm");
    std::fs::copy(&real_wasm, root.join("data/clave-bar.wasm"))
        .context("copy clave-bar.wasm (run the real `clave setup` first)")?;
    crate::setup::run_setup()?;

    let now = crate::store::now_unix();
    let paths = crate::store::store_paths()?;
    // A `?` mid-loop leaves the sandbox partially seeded — that's fine: it's
    // fully recoverable with `clave dev reset` (wipes the whole sandbox root).
    for (i, a) in sc.agents.iter().enumerate() {
        let uuid = scenario_uuid(i as u32 + 1);
        let repo = root.join("repos").join(format!("{name}-{}", a.slug));
        std::fs::create_dir_all(&repo)?;
        // -b main: pin the branch — else init.defaultBranch (maybe `master`)
        // would disagree with the store row's hardcoded `branch: "main"`.
        run_in(&repo, "git", &["init", "-q", "-b", "main"])?;
        run_in(&repo, "git", &["commit", "--allow-empty", "-q", "-m", "seed"])?;
        let cwd = if a.worktree {
            let wt = repo.join(".claude-worktrees").join(&uuid[..8]);
            run_in(
                &repo,
                "git",
                &["worktree", "add", "-q", "-b", &format!("clave/{}", &uuid[..8]), wt.to_str().context("wt")?],
            )?;
            wt
        } else {
            repo.clone()
        };
        let cwd = std::fs::canonicalize(&cwd)?; // S0b: claude munges getcwd()
        // A REAL resumable jsonl for a few tokens (§6.9): resume-with-
        // history is verified for real, not mocked.
        println!("seeding {uuid} ({})…", a.slug);
        let st = Command::new("claude")
            .current_dir(&cwd)
            .args(["-p", "--session-id", &uuid, "Reply with exactly: ok"])
            .status()
            .context("running claude -p (is claude on PATH?)")?;
        anyhow::ensure!(st.success(), "claude -p seeding failed for {uuid}");
        let cwd_str = cwd.to_str().context("cwd utf8")?.to_string();
        crate::store::with_store_mut(&paths, |s| {
            s.agents.insert(
                uuid.clone(),
                crate::store::AgentRecord {
                    uuid: uuid.clone(),
                    cwd: cwd_str.clone(),
                    repo_root: repo.to_string_lossy().into_owned(),
                    branch: if a.worktree { format!("clave/{}", &uuid[..8]) } else { "main".into() },
                    label: format!("{}-{} · seeded", name, a.slug),
                    status: clave_types::Status::Idle,
                    last_interacted: now.saturating_sub(a.ago_secs),
                    last_visited: 0,
                    worktree: a.worktree.then(|| cwd_str.clone()),
                    label_source: crate::store::LabelSource::FirstPrompt,
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
    println!("\nScenario `{name}` ready. Launch (your command, your pane):\n");
    println!("  {}", launch_command(&root));
    println!("\nWhen done: `clave dev reset` (prints the kill command first).");
    Ok(())
}

pub fn run_status() -> Result<()> {
    let root = sandbox_root()?;
    enter_sandbox(&root);
    let store = crate::store::read_store(&crate::store::store_paths()?)?;
    let list = Command::new("zellij")
        .args(["list-sessions", "-n"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let live_session = crate::setup::session_is_live(&list, "clave-test");
    // Sanctioned §6.9 read: explicitly clave-test-scoped.
    let dump = Command::new("zellij")
        .env("ZELLIJ_SESSION_NAME", "clave-test")
        .args(["action", "dump-layout"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
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

pub fn run_reset() -> Result<()> {
    let root = sandbox_root()?;
    println!("If the session is running, kill it first (your command):\n");
    println!("  zellij kill-session clave-test && zellij delete-session --force clave-test\n");
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
        println!("Sandbox wiped: {}", root.display());
    } else {
        println!("Sandbox already clean: {}", root.display());
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn launch_command_is_fully_env_prefixed() {
        // The ONE command printed for Ollie: everything sandboxed, nothing
        // ambient (§6.9 — his real session/store/~/.claude untouchable).
        let cmd = launch_command(std::path::Path::new("/sb"));
        assert!(cmd.contains("CLAVE_SESSION=clave-test"));
        assert!(cmd.contains("CLAVE_STATE_DIR=/sb/state"));
        assert!(cmd.contains("CLAVE_DATA_DIR=/sb/data"));
        assert!(cmd.contains("CLAUDE_CONFIG_DIR=/sb/claude"));
        assert!(cmd.trim_end().ends_with("clave"));
    }
}
