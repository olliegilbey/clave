//! clave — terminal-native orchestration for a fleet of Claude Code agents.
//!
//! Each agent is a Zellij tab running the *real* Claude Code TUI. `clave` spawns
//! them, names them from their session transcript, and reports status via Claude
//! Code hooks (not screen-scraping). The `clave-bar` plugin decorates the fleet
//! rows and renames the real tabs — it does not repaint into a tab title itself.
//!
//! See `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` for the full spec.

use std::path::Path;

use anyhow::{Context, Result};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use clave::{add, dev, hook, lsview, open, release, setup, spawn, store};

#[derive(Parser)]
#[command(
    name = "clave",
    // Version is set at parse time (`long_version`) so `--version` carries
    // the build tag too (§2: "what am I running" in both environments) —
    // option_env! can't be concatenated into a const &str for the derive.
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
        /// The clave label. ACCEPTED AND DELIBERATELY UNUSED since #91 — it
        /// is no longer forwarded to `claude --name`, but it stays parseable
        /// because zellij replays this exact argv from the serialized layout
        /// of every pre-existing session. Pinned by
        /// `spawn_still_accepts_the_baked_name_arg`.
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

    /// Prune store binds + tab_timeline entries for CLOSED tabs
    /// (plugin-internal, #6/F3). The active bar reports the ids it observed DIE
    /// (stale = bound-or-timelined but absent from the live set) whenever the
    /// set changes; the store removes exactly those. Removing specific dead ids
    /// is order-safe (idempotent, commutes) — a full-live-set "retain" payload
    /// would race-unbind a tab created after it was computed. zellij reuses
    /// tab_ids (screen.rs:1617), so this is correctness, not just hygiene.
    #[command(hide = true)]
    PruneTabs {
        /// The zellij tab ids observed dead (the stale set). Empty is a no-op
        /// (nothing observed dead → nothing to remove).
        #[arg(trailing_var_arg = true)]
        stale_ids: Vec<usize>,
    },

    /// Persist the bar collapse mode (plugin-internal, issue #5). The
    /// `clave-toggle` broadcast flips every instance's memory instantly;
    /// the ACTIVE instance then reports the absolute new mode here so the
    /// store — the one writer — carries it in every snapshot, healing any
    /// instance the broadcast missed (C8 parity-desync family).
    #[command(hide = true)]
    Collapse {
        /// The absolute mode: true = gutter, false = expanded. Absolute so
        /// duplicate/raced writes stay idempotent (never a flip).
        /// ArgAction::Set is REQUIRED: clap-derive turns a bare `bool` field
        /// into a SetTrue FLAG — as a positional that trips clap's
        /// debug_assert on every parse and can never accept the literal
        /// `true`/`false` the plugin passes (caught by CodeRabbit CLI on
        /// PR #13; parse pinned in `collapse_cli_parses_absolute_values`).
        #[arg(action = clap::ArgAction::Set)]
        collapsed: bool,
    },

    /// Prepare the machine: generate config/layout, merge Claude hooks,
    /// pre-seed the Zellij permission cache (§6.8/§7). Idempotent.
    Setup,

    /// Health report: required tools, picker deps, clave's own setup state,
    /// environment traps. Diagnose-only — `clave setup` is the repair path.
    Doctor {
        /// Emit facts + findings as JSON instead of the grouped report.
        #[arg(long)]
        json: bool,
    },

    /// Open a known store row's tab (plugin-internal, §6.3 C8): the bar fires
    /// this when a dormant row's focus settles or on an explicit pick.
    #[command(hide = true)]
    Open {
        /// The agent's session UUID (the store join key).
        uuid: String,
        /// The display width the new tab is born into, from the bar's
        /// `get_tab_info().display_area_columns` (task 7b′). Absent =
        /// fall back to the reference viewport.
        #[arg(long)]
        display_cols: Option<usize>,
        /// Born collapsed, so a fleet left collapsed does not flash wide
        /// (LEDGER D36, applied to the open path).
        #[arg(long)]
        collapsed: bool,
    },

    /// Live-validation harness (§6.9): seed sandboxed scenarios, dump status,
    /// reset. The sandbox (session `clave-test`, own store/data/claude dirs)
    /// can never touch the real session or ~/.claude.
    Dev {
        #[command(subcommand)]
        action: DevAction,
    },

    /// Cut a release (invoked by `just release`, not for hand use): gate on a
    /// clean tree + a HEAD `vX.Y.Z` tag matching this binary's version, then
    /// install the versioned wasm + CLI copy and regenerate stable
    /// config/layout/hooks at the versioned paths (§2).
    #[command(hide = true)]
    Release {
        /// The freshly built release wasm to install as the versioned artifact.
        #[arg(long)]
        wasm_src: String,
        /// The freshly built release CLI to install as the versioned copy.
        #[arg(long)]
        cli_src: String,
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
    // Override clap's derive version with semver + build tag at parse time;
    // clap still owns --version/--help exit behavior via get_matches(). clap's
    // Str wants a &'static str, so leak the once-built string (harmless in a
    // short-lived CLI — it lives for the whole process anyway).
    let version: &'static str = Box::leak(release::long_version().into_boxed_str());
    let matches = Cli::command().version(version).get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    match cli.command {
        // Bare `clave` — no subcommand — attaches or creates the session.
        None => setup::launch_session(),
        // Each arm is implemented in its own task — see docs/design.md "v1 scope".
        Some(Command::Add { worktree }) => add::run_add(worktree),
        Some(Command::Spawn {
            uuid,
            name: _name,
            cwd,
        }) => {
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
            // Discovered claude (spec §Discovery): the pane env may lack the
            // interactive PATH (nvm/local-install), so exec the absolute
            // path — resolved FRESH each spawn (the command is replayed on
            // resurrection and must survive reinstalls).
            let claude = clave::discover::discover(clave::discover::ToolId::Claude)
                .map(|d| d.path)
                .ok_or_else(|| {
                    // Reuse the canonical copy (coderabbit 2026-07-22): an
                    // ad-hoc string here is exactly the drift the one-copy-
                    // module rule exists to prevent — this pane message and
                    // doctor's must stay the same words.
                    let advice =
                        clave::doctor::missing_advice(clave::discover::ToolId::Claude, None)
                            .join("\n");
                    anyhow::anyhow!(
                        "claude not found\n{advice}\n\
                         (or set CLAVE_CLAUDE_BIN to its location)"
                    )
                })?;
            let err = match mode {
                // NO `--name` (#91). It used to be passed here on create, and
                // it was the whole bug: Claude records `--name` as a
                // `custom-title` transcript entry, indistinguishable from a
                // user `/rename`, so the hook wrote clave's OWN label into
                // `AgentRecord.title` and the bar rendered a filled title chip
                // on an agent nobody had named. Design-lock §2 and LEDGER D19
                // both require that chip BLANK until renamed.
                //
                // Filtering it downstream was tried and refused: Claude
                // re-emits a per-turn header block that rewrites the current
                // title, so clave's label reappears on turn 2, turn 3, and 30
                // times in one sampled session — a positional "ignore it
                // before the first prompt" rule looks fixed on a one-prompt
                // check and is not.
                //
                // Passing it also cost the user something. With no
                // `custom-title`, `claude --resume`'s picker falls back to
                // `aiTitle` — a description of the actual work. `--name`
                // OVERWROTE that with `<dir> · <branch>`, which is identical
                // across every agent on the same branch. Verified in a real
                // transcript: zero `customTitle` entries, `aiTitle` present,
                // and the picker showed the aiTitle.
                //
                // Nothing else consumed it. The zellij tab name comes from
                // clave's store label directly, and `/rename` still writes a
                // `custom-title` that the hook picks up exactly as before.
                //
                // The `--name` ARG stays parseable on purpose: zellij has it
                // baked into the serialized layouts of every existing session,
                // and a pane whose replayed command no longer parses would
                // fail to resurrect.
                //
                // `CLAVE_AGENT_UUID` carries the row's join key across the
                // exec (#97). Claude mints a NEW session id on resume and
                // writes a new transcript, so the hook's payload id stops
                // matching the store and the row silently freezes — measured
                // at 5.9 days stale on a tab that was in active use. The env
                // is the one channel that survives: `exec` replaces the
                // image but keeps the environment, and hooks are children of
                // that process (proven in situ — `ZELLIJ_PANE_ID`, exported
                // to layout `command` panes, is visible to them).
                //
                // Set on BOTH arms deliberately. Create looks unnecessary
                // today, since the ids agree until the first resume — but
                // the row is created once and resumed forever, and an arm
                // that is right only until someone resumes is the bug.
                spawn::SpawnMode::Create => std::process::Command::new(&claude)
                    .env(clave_types::AGENT_UUID_ENV, &uuid)
                    .args(["--session-id", &uuid])
                    .exec(),
                spawn::SpawnMode::Resume => std::process::Command::new(&claude)
                    .env(clave_types::AGENT_UUID_ENV, &uuid)
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
        Some(Command::PruneTabs { stale_ids }) => {
            let paths = store::store_paths()?;
            // Push only on CHANGE (apply_prune_tabs returns None otherwise) — a
            // prune that matched nothing (already-removed dead ids: idempotent
            // late arrival) must not spam the pipe. The push drops the closed
            // tab's agent to a dormant row on every instance.
            if let Some(snap) = store::apply_prune_tabs(&paths, &stale_ids)? {
                hook::push_snapshot(&snap);
            }
            Ok(())
        }
        Some(Command::Collapse { collapsed }) => {
            let paths = store::store_paths()?;
            // Push only on CHANGE (apply_collapse returns None otherwise) —
            // duplicate executor writes after a broadcast must not spam the
            // pipe (round 11). The push heals every missed-pipe instance.
            if let Some(snap) = store::apply_collapse(&paths, collapsed)? {
                hook::push_snapshot(&snap);
            }
            Ok(())
        }
        Some(Command::Setup) => setup::run_setup(),
        Some(Command::Doctor { json }) => clave::doctor::run_doctor(json),
        Some(Command::Open {
            uuid,
            display_cols,
            collapsed,
        }) => open::run_open(&uuid, display_cols, collapsed),
        Some(Command::Dev { action }) => match action {
            DevAction::Scenario { name } => dev::run_scenario(&name),
            DevAction::Launch => dev::run_launch(),
            DevAction::Status => dev::run_status(),
            DevAction::Reset => dev::run_reset(),
        },
        Some(Command::Release { wasm_src, cli_src }) => {
            release::run_release(Path::new(&wasm_src), Path::new(&cli_src))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #5 (CodeRabbit CLI, PR #13): the plugin shells
    /// `clave collapse true|false` — pin that the positional literally
    /// parses as a VALUE. Without ArgAction::Set, clap-derive makes a bare
    /// bool a SetTrue flag: debug builds panic clap's debug_assert on
    /// every parse and release builds reject the literal — a break no
    /// workspace test caught because nothing exercised the CLI layer.
    #[test]
    fn collapse_cli_parses_absolute_values() {
        for (arg, want) in [("true", true), ("false", false)] {
            let cli = Cli::try_parse_from(["clave", "collapse", arg]).expect("must parse");
            match cli.command {
                Some(Command::Collapse { collapsed }) => assert_eq!(collapsed, want),
                _ => panic!("parsed into the wrong command"),
            }
        }
    }

    /// #91 left `clave spawn --name` accepted but UNUSED, and that is load
    /// bearing rather than dead: zellij has the full `args "spawn" "<uuid>"
    /// "--name" "<label>" "--cwd" "<cwd>"` form baked into the serialized
    /// layout of every session created before the change, and replays it
    /// verbatim on resurrection. Deleting the arg as "unused" would make those
    /// panes fail to parse their own resurrect command — a break that no test
    /// touching `claude`'s argv could catch, because the damage is at clave's
    /// OWN CLI boundary.
    #[test]
    fn spawn_still_accepts_the_baked_name_arg() {
        let cli = Cli::try_parse_from([
            "clave",
            "spawn",
            "u-1",
            "--name",
            "repo \u{00b7} main",
            "--cwd",
            "/x",
        ])
        .expect("the baked resurrect command must still parse");
        match cli.command {
            Some(Command::Spawn { uuid, name, cwd }) => {
                assert_eq!(uuid, "u-1");
                assert_eq!(name, "repo \u{00b7} main");
                assert_eq!(cwd, "/x");
            }
            _ => panic!("parsed into the wrong command"),
        }
    }

    /// Task 7b′: the bar shells `clave open <uuid> [--display-cols N]
    /// [--collapsed]` so a dwell-opened tab is born at the real display width
    /// and in the right mode. Same ledger rule as the two pins around it — a
    /// new CLI surface gets a parse pin, because clap-derive shape bugs panic
    /// debug builds at the parse layer and nothing else reaches it. Both flags
    /// must stay OPTIONAL: a hand-run `clave open` passes neither.
    #[test]
    fn open_cli_parses_the_birth_width_and_mode() {
        let bare = Cli::try_parse_from(["clave", "open", "u-1"]).expect("must parse");
        match bare.command {
            Some(Command::Open {
                uuid,
                display_cols,
                collapsed,
            }) => {
                assert_eq!(uuid, "u-1");
                assert_eq!(display_cols, None);
                assert!(!collapsed);
            }
            _ => panic!("parsed into the wrong command"),
        }
        let full = Cli::try_parse_from([
            "clave",
            "open",
            "u-1",
            "--display-cols",
            "280",
            "--collapsed",
        ])
        .expect("must parse");
        match full.command {
            Some(Command::Open {
                display_cols,
                collapsed,
                ..
            }) => {
                assert_eq!(display_cols, Some(280));
                assert!(collapsed);
            }
            _ => panic!("parsed into the wrong command"),
        }
    }

    /// #6/F3: the plugin shells `clave prune-tabs <stale-id>…` with the ids it
    /// observed die (the inverted, order-safe payload — see apply_prune_tabs).
    /// Same ledger rule as the collapse test above — every new CLI surface gets
    /// a parse pin, because clap-derive shape bugs panic debug builds at the
    /// parse layer and no workspace test reaches it.
    #[test]
    fn prune_tabs_cli_parses_variadic_ids() {
        let cli = Cli::try_parse_from(["clave", "prune-tabs", "3", "1", "7"]).expect("must parse");
        match cli.command {
            Some(Command::PruneTabs { stale_ids }) => assert_eq!(stale_ids, vec![3, 1, 7]),
            _ => panic!("parsed into the wrong command"),
        }
    }
}
