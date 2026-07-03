//! clave — terminal-native orchestration for a fleet of Claude Code agents.
//!
//! Each agent is a Zellij tab running the *real* Claude Code TUI. `clave` spawns
//! them, names them from their session transcript, and reports status via Claude
//! Code hooks (not screen-scraping). The `clave-bar` plugin decorates the fleet
//! rows and renames the real tabs — it does not repaint into a tab title itself.
//!
//! See `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` for the full spec.

use anyhow::Result;
use clap::{Parser, Subcommand};

use clave::{lsview, store};

#[derive(Parser)]
#[command(
    name = "clave",
    version,
    about = "Conduct a fleet of Claude Code agents from a Zellij sidebar"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a new agent: pick a directory (zoxide), open a tab, spawn Claude.
    Add,

    /// Resume-or-create the Claude session for a pane (idempotent).
    ///
    /// This is the command each agent pane actually runs. Because it is
    /// idempotent, Zellij serialization can re-run it verbatim on resurrect and
    /// the conversation resumes instead of starting fresh.
    Spawn {
        /// The Claude session UUID this agent owns (minted by `clave add`).
        uuid: String,
        /// Display name passed to `claude --name` on first creation.
        #[arg(long)]
        name: String,
        /// Working directory to start the agent in.
        #[arg(long)]
        cwd: String,
    },

    /// Handle a Claude Code hook event (reads the hook JSON payload from stdin).
    ///
    /// Configured globally in ~/.claude/settings.json so every session reports
    /// status automatically. The payload carries `session_id`, which maps back
    /// to exactly one agent because we minted that id at spawn time.
    Hook {
        /// Hook event name, e.g. Stop, Notification, UserPromptSubmit.
        event: String,
    },

    /// List agents and their current status.
    Ls {
        /// Emit the raw `AgentSnapshot` JSON instead of the human table.
        #[arg(long)]
        json: bool,
    },

    /// Print the current `AgentSnapshot` as JSON (plugin-internal hydration).
    ///
    /// Hidden from `--help`: the bar runs this on load to seed its state (spec
    /// §6.2/§6.6, spike S5), it is not a user-facing command.
    #[command(hide = true)]
    Snapshot,

    /// Persist a focus transition for an agent (plugin-internal).
    ///
    /// Hidden from `--help`: the bar calls this when the user visits a tab so
    /// the done-unread flag clears durably (§6.5). Store-only, no pipe push.
    #[command(hide = true)]
    Focus {
        /// The agent's session UUID (the store join key).
        uuid: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        // Each arm is implemented in its own task — see docs/design.md "v1 scope".
        Command::Add => todo!("clave add — task #4"),
        Command::Spawn { .. } => todo!("clave spawn — task #2"),
        Command::Hook { .. } => todo!("clave hook — task #6"),
        Command::Ls { json } => {
            let paths = store::store_paths()?;
            let s = store::read_store(&paths)?;
            if json {
                println!("{}", serde_json::to_string(&store::snapshot_from(&s))?);
            } else {
                print!("{}", lsview::render_ls(&s));
            }
            Ok(())
        }
        Command::Snapshot => {
            // The bar hydrates on load by running `clave snapshot` via
            // run_command and parsing stdout (spec §6.2/§6.6, was spike S5).
            let paths = store::store_paths()?;
            let s = store::read_store(&paths)?;
            println!("{}", serde_json::to_string(&store::snapshot_from(&s))?);
            Ok(())
        }
        Command::Focus { uuid } => {
            let paths = store::store_paths()?;
            store::apply_focus(&paths, &uuid, store::now_unix())
        }
    }
}
