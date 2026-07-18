//! clave — terminal-native orchestration for a fleet of Claude Code agents.
//!
//! Each agent is a Zellij tab running the *real* Claude Code TUI. `clave` spawns
//! them, names them from their session transcript, and reports status via Claude
//! Code hooks (not screen-scraping). The `clave-bar` plugin decorates the fleet
//! rows and renames the real tabs — it does not repaint into a tab title itself.
//!
//! See `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` for the full spec.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use clave::{add, dev, hook, lsview, open, setup, spawn, store};

#[derive(Parser)]
#[command(
    name = "clave",
    version,
    about = "Conduct a fleet of Claude Code agents from a Zellij sidebar"
)]
struct Cli {
    /// Bare `clave` (no subcommand) attaches/creates the dedicated session
    /// with clave's own config + layout (§6.8) — hence `Option`.
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Add a new agent: pick a directory (zoxide), open a tab, spawn Claude.
    Add {
        /// Create a dedicated git worktree for the agent (§6.3): clave shells
        /// out `git worktree add` itself so it owns the path (needed for the
        /// munged jsonl check and the store record).
        #[arg(long)]
        worktree: bool,
    },

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

    /// Stamp "the user committed to this tab" (birth, for now) into the
    /// STORE's tab timeline with host time, then push the snapshot that
    /// carries the new order to every bar instance (§6.6; the wasm plugin
    /// has no trustworthy clock, and per-instance pipe deltas diverged —
    /// C5 round 5). Fired by the bar at tab birth.
    Touch {
        /// Zellij's stable tab id.
        tab_id: usize,
    },

    /// Persist the uuid→tab_id join reported by the agent tab's own bar
    /// (plugin-internal, §6.6 Design B). The bind keys the hook's
    /// prompt→timeline stamp and every bar's glyph join — local
    /// register/manifest joins diverge across instances (round 6).
    #[command(hide = true)]
    Bind {
        /// The agent's session UUID (the store join key).
        uuid: String,
        /// Zellij's stable tab id hosting the agent's pane.
        tab_id: usize,
    },

    /// Prepare the machine: generate config/layout, merge Claude hooks,
    /// pre-seed the Zellij permission cache (§6.8/§7). Idempotent.
    Setup,

    /// Open a known store row's tab (plugin-internal, §6.3 C8): the bar fires
    /// this when a dormant row's focus settles or on an explicit pick.
    #[command(hide = true)]
    Open {
        /// The agent's session UUID (the store join key).
        uuid: String,
    },

    /// Live-validation harness (§6.9): seed sandboxed scenarios, dump status,
    /// reset. The sandbox (session `clave-test`, own store/data/claude dirs)
    /// can never touch the real session or ~/.claude.
    Dev {
        #[command(subcommand)]
        action: DevAction,
    },
}

#[derive(Subcommand)]
enum DevAction {
    /// Seed a named scenario and print the launch command.
    Scenario { name: String },
    /// Attach/create the sandboxed clave-test session (sandbox env set
    /// internally — the short form of the env-prefixed launch). Run this
    /// yourself in a non-zellij terminal.
    Launch,
    /// Dump sandbox store + live uuids + session liveness as JSON.
    Status,
    /// Wipe the sandbox (prints the kill-session command first).
    Reset,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        // Bare `clave` — no subcommand — attaches or creates the session.
        None => setup::launch_session(),
        // Each arm is implemented in its own task — see docs/design.md "v1 scope".
        Some(Command::Add { worktree }) => add::run_add(worktree),
        Some(Command::Spawn { uuid, name, cwd }) => {
            // S0b: canonicalize BEFORE munging — Claude keys the transcript
            // dir off the PHYSICAL getcwd() path.
            let physical = std::fs::canonicalize(&cwd)
                .with_context(|| format!("canonicalizing --cwd {cwd}"))?;
            let physical_str = physical.to_str().context("non-UTF8 cwd")?.to_string();
            let claude_dir = clave::env::claude_config_dir()?;
            let mode = spawn::spawn_mode(&claude_dir, &physical_str, &uuid);
            clave::evlog::log_event("spawn", &format!("{uuid}: {mode:?}"));
            // Register uuid→pane BEFORE exec (this process is about to be
            // replaced; best-effort — see register_pane).
            spawn::register_pane(&uuid);
            std::env::set_current_dir(&physical).context("entering --cwd")?;
            use std::os::unix::process::CommandExt;
            let err = match mode {
                // --name only on create: the bar label is clave-owned (§6.1).
                spawn::SpawnMode::Create => std::process::Command::new("claude")
                    .args(["--session-id", &uuid, "--name", &name])
                    .exec(),
                spawn::SpawnMode::Resume => std::process::Command::new("claude")
                    .args(["--resume", &uuid])
                    .exec(),
            };
            // exec only returns on failure — surface it in the pane.
            Err(anyhow::anyhow!("exec claude failed: {err}"))
        }
        Some(Command::Hook { event }) => {
            // Zero-risk global citizen (§6.5): read stdin, do our best, and
            // exit 0 unconditionally — a clave bug must never become a
            // machine-wide Claude failure. Errors go to stderr only.
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            if let Err(e) = hook::run_hook(&event, &input) {
                eprintln!("clave hook: {e:#}");
            }
            Ok(()) // ALWAYS success
        }
        Some(Command::Ls { json }) => {
            let paths = store::store_paths()?;
            let s = store::read_store(&paths)?;
            if json {
                println!("{}", serde_json::to_string(&store::snapshot_from(&s))?);
            } else {
                print!("{}", lsview::render_ls(&s));
            }
            Ok(())
        }
        Some(Command::Snapshot) => {
            // The bar hydrates on load by running `clave snapshot` via
            // run_command and parsing stdout (spec §6.2/§6.6, was spike S5).
            let paths = store::store_paths()?;
            let s = store::read_store(&paths)?;
            println!("{}", serde_json::to_string(&store::snapshot_from(&s))?);
            Ok(())
        }
        Some(Command::Focus { uuid }) => {
            let paths = store::store_paths()?;
            // Broadcast the flip: only the focused tab's bar repainted
            // locally (zellij starves hidden instances of TabUpdates).
            if let Some(snap) = store::apply_focus(&paths, &uuid, store::now_unix())? {
                hook::push_snapshot(&snap);
            }
            Ok(())
        }
        Some(Command::Touch { tab_id }) => {
            let paths = store::store_paths()?;
            // Locked RMW in the store (the ONE order writer), then broadcast
            // the seq-gated full state — the channel that never diverged.
            let snap = store::apply_touch(&paths, tab_id, store::now_unix())?;
            hook::push_snapshot(&snap);
            Ok(())
        }
        Some(Command::Bind { uuid, tab_id }) => {
            let paths = store::store_paths()?;
            // Push only on CHANGE (apply_bind returns None otherwise) — a
            // re-reported existing bind must not generate pipe traffic.
            if let Some(snap) = store::apply_bind(&paths, &uuid, tab_id)? {
                hook::push_snapshot(&snap);
            }
            Ok(())
        }
        Some(Command::Setup) => setup::run_setup(),
        Some(Command::Open { uuid }) => open::run_open(&uuid),
        Some(Command::Dev { action }) => match action {
            DevAction::Scenario { name } => dev::run_scenario(&name),
            DevAction::Launch => dev::run_launch(),
            DevAction::Status => dev::run_status(),
            DevAction::Reset => dev::run_reset(),
        },
    }
}
