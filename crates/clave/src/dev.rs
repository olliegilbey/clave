//! `clave dev` (§6.9): the sandboxed live-validation harness. One command
//! seeds a named, repeatable world state; Ollie drives the checklist in a
//! real `clave-test` session; Claude reads clave.log + `dev status`.
//! Real tabs, real spawns, real jsonls — only the conversation CONTENT is
//! trivial (`claude -p` one-liners). Deliberately minimal: a fixture
//! seeder plus a log — no recorder, no assertion runner, no CI.
//!
//! Session lifecycle stays Ollie's: this module NEVER launches or kills
//! zellij sessions — it prints the commands.

use std::path::Path;
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
    /// Explicit short repo directory name (e.g. `"clave"`), shared by every
    /// agent that names it — one shared dir under `repos/` is what lets
    /// several agents render the SAME repo ink (render.rs lock §4). `None`
    /// keeps the original per-agent `{scenario}-{slug}` naming the three c8
    /// scenarios were reviewed with, where every agent is its own repo.
    pub repo: Option<&'static str>,
    /// Non-worktree branch override, for `Provenance::Branch` (render.rs
    /// lock §5.1's third state) without a real `git worktree add` — the
    /// store's branch field is metadata only (see the `-b main` comment
    /// below); nothing checks it against cwd's real git state. `None` keeps
    /// the original hardcoded `"main"`. Ignored when `worktree` is true: a
    /// worktree's branch is always the generated `clave/{uuid8}`.
    pub branch: Option<&'static str>,
    /// Claude's session rename (§6.4) — the render's title chip. `None`
    /// renders a blank chip, which is itself worth seeding: `ux-gate1` wants
    /// at least one row proving a missing title doesn't shift the row.
    pub title: Option<&'static str>,
    /// A hook-shaped one-liner (design-lock §7.1). Empty string is the
    /// original default and renders a blank summary cell.
    pub summary: &'static str,
    pub status: clave_types::Status,
    /// A seeded context reading (S7, #62), in tokens. Seeded rows are dormant
    /// and dormant rows fire no hooks, so without this the battery column is
    /// blank across the whole sandbox and a visual check of the ramp validates
    /// nothing — which is the one tier this change class cannot get from tests.
    ///
    /// Seeding it is also faithful rather than a fixture cheat: the ruling that
    /// closed the design lock's open question is precisely that a dormant row
    /// carries a real last reading, because a dormant conversation consumes
    /// nothing and its stored figure IS its current occupancy. `None` renders a
    /// blank cell, which is the distinct "no reading yet" case and worth having
    /// on screen beside the others.
    pub context_tokens: Option<u32>,
}

impl ScenarioAgent {
    /// Base for the c8 scenarios' struct-update literals — every field the
    /// visual-design scenario needed and the c8 ones never touched, defaulted
    /// to exactly what the inline construction used to hardcode (`title:
    /// None`, `summary: ""`, `status: Idle`, per-agent repo, hardcoded
    /// branch), so ..DEFAULT changes zero observed behaviour.
    const DEFAULT: ScenarioAgent = ScenarioAgent {
        slug: "",
        ago_secs: 0,
        worktree: false,
        delete_cwd_after: false,
        repo: None,
        branch: None,
        title: None,
        summary: "",
        status: clave_types::Status::Idle,
        context_tokens: None,
    };
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
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "mid",
                ago_secs: 3_600,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "old",
                ago_secs: 86_400,
                ..ScenarioAgent::DEFAULT
            },
        ],
    },
    Scenario {
        name: "c8-worktree",
        agents: &[
            ScenarioAgent {
                slug: "main",
                ago_secs: 60,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "wt",
                ago_secs: 3_600,
                worktree: true,
                ..ScenarioAgent::DEFAULT
            },
        ],
    },
    Scenario {
        name: "c8-stale",
        agents: &[
            ScenarioAgent {
                slug: "alive",
                ago_secs: 60,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "gone",
                ago_secs: 3_600,
                delete_cwd_after: true,
                ..ScenarioAgent::DEFAULT
            },
        ],
    },
    // The visual-design decision fixture (#85 follow-up): every status, every
    // provenance, a missing title, and repos SHORT and DISTINCT enough (first
    // three characters differ) to read cleanly in the 3-column collapsed repo
    // field. c8-* stays a checklist of ONE mechanism each; this one is a fleet.
    Scenario {
        name: "ux-gate1",
        agents: &[
            // Main checkout, Idle, no title — proves the blank title chip
            // doesn't shift the row, and Provenance::Main renders nothing.
            ScenarioAgent {
                slug: "cold",
                ago_secs: 180,
                repo: Some("clave"),
                summary: "ready for the next prompt",
                status: clave_types::Status::Idle,
                context_tokens: Some(12_000),
                ..ScenarioAgent::DEFAULT
            },
            // Same repo, a worktree this time — same ink as `cold` above,
            // proving one repo is one colour across provenances.
            ScenarioAgent {
                slug: "gate",
                ago_secs: 90,
                worktree: true,
                repo: Some("clave"),
                title: Some("UX-GATE"),
                summary: "wiring the new status column into render_rows before the review",
                status: clave_types::Status::Working,
                context_tokens: Some(61_000),
                ..ScenarioAgent::DEFAULT
            },
            // A plain branch checkout (no worktree) — the third provenance.
            ScenarioAgent {
                slug: "sync",
                ago_secs: 45,
                repo: Some("nalu"),
                branch: Some("feature/sync-timer"),
                title: Some("SYNC-T9"),
                summary: "two tests disagree about the debounce window, need a read",
                status: clave_types::Status::NeedsYou,
                context_tokens: Some(96_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "dns",
                ago_secs: 600,
                repo: Some("infra"),
                branch: Some("hotfix/dns-ttl"),
                title: Some("DNS-TTL"),
                summary: "the staging rollout keeps timing out against the new zone",
                status: clave_types::Status::Failed,
                context_tokens: Some(128_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "cart",
                ago_secs: 30,
                repo: Some("webapp"),
                title: Some("CART-99"),
                summary: "cart totals now round the same way on server and client",
                status: clave_types::Status::Done,
                context_tokens: Some(158_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "readme",
                ago_secs: 7_200,
                repo: Some("docs"),
                title: Some("README"),
                summary: "trimmed the quickstart down to five commands",
                status: clave_types::Status::Idle,
                ..ScenarioAgent::DEFAULT
            },
            // The §6.3 staleness fixture, same mechanism as c8-stale's `gone`,
            // dropped into the fleet so the decision review sees it alongside
            // everything else rather than in isolation. WORKTREE, not a plain
            // checkout: `repo` is shared with `cold`/`gate` above, and
            // `delete_cwd_after` removes exactly `cwd` — a plain checkout's
            // `cwd` IS the shared repo dir, which would delete `cold` and
            // `gate`'s worktree out from under them too. A worktree's `cwd`
            // is its own `.claude-worktrees/<uuid8>` subdir, so only this
            // row's directory goes.
            ScenarioAgent {
                slug: "vanished",
                ago_secs: 3_600,
                worktree: true,
                repo: Some("clave"),
                title: Some("KDL-GRD"),
                summary: "validating every generated KDL artifact against the real zellij parser",
                status: clave_types::Status::Working,
                delete_cwd_after: true,
                ..ScenarioAgent::DEFAULT
            },
        ],
    },
];

/// Valid v4-shaped, deterministic, self-identifying (`c85c` ≈ c8 scenario).
pub fn scenario_uuid(n: u32) -> String {
    format!("00000000-0000-4000-8000-c85c{n:08}")
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
///
/// `clave-dev`, NOT bare `clave` (#43b): `just dev-install` now installs the
/// working-tree CLI under that name precisely so it stops colliding with the
/// daily surface — so bare `clave` here would either be `command not found` on
/// a contributor's box or, worse, silently drive the sandbox with the STABLE
/// release instead of the working tree under test.
///
/// The env prefix is not decoration now that the instance is per-worktree
/// (`sandbox.rs`): this line is meant to be pasted into a fresh terminal in
/// an arbitrary directory, where `dev launch`'s own cwd-based derivation
/// would resolve to the MAIN checkout's sandbox instead of the worktree that
/// printed it. `enter_sandbox` therefore lets an explicit value win.
pub fn launch_command(sb: &crate::sandbox::Sandbox) -> String {
    format!(
        "CLAVE_SESSION={} CLAVE_STATE_DIR={} CLAVE_DATA_DIR={} clave-dev",
        sb.session,
        sb.state_dir().display(),
        sb.data_dir().display()
    )
}

/// Should this variable be filled in from the derived instance? Only when
/// the caller did not name one — same "override always wins, empty means
/// unset" rule as `env::session_name_from` / `env::dir_from`. Pure because
/// setting real env vars would race parallel tests (see `env.rs`).
pub fn env_should_be_derived(current: Option<&str>) -> bool {
    current.is_none_or(str::is_empty)
}

/// Point THIS process at the sandbox (children inherit — the seeding
/// `claude -p` runs as the REAL user identity but its hook invocations
/// inherit CLAVE_STATE_DIR and land in the sandbox store).
///
/// An explicitly set variable WINS, so the env-prefixed `launch_command`
/// stays truthful when it is pasted somewhere else on disk.
fn enter_sandbox(sb: &crate::sandbox::Sandbox) {
    let vars: [(&str, std::ffi::OsString); 3] = [
        ("CLAVE_SESSION", sb.session.clone().into()),
        ("CLAVE_STATE_DIR", sb.state_dir().into()),
        ("CLAVE_DATA_DIR", sb.data_dir().into()),
    ];
    for (k, v) in vars {
        if env_should_be_derived(std::env::var(k).ok().as_deref()) {
            // SAFETY: single-threaded CLI entry point; set before any spawn.
            unsafe { std::env::set_var(k, v) };
        }
    }
}

/// The unconditional form, for STAGING (`dev scenario`): staging's identity
/// is the working tree it runs in, full stop. Under the respect-env form, a
/// shell that inherited another sandbox's `CLAVE_*` — any pane inside a
/// launched sandbox session qualifies — would seed repos under THIS root
/// while every store write lands in the OTHER instance's state dir, a
/// split-brain the setup script's self-check only reports after the foreign
/// root is already written (#161 review). `dev launch` keeps respect-env:
/// its pasted prefix is the caller naming an instance deliberately. Like
/// `enter_sandbox`, an expected mutant survivor — env writes cannot be
/// exercised by parallel tests (see `env.rs`).
fn force_sandbox(sb: &crate::sandbox::Sandbox) {
    // SAFETY: single-threaded CLI entry point; set before any spawn.
    unsafe {
        std::env::set_var("CLAVE_SESSION", &sb.session);
        std::env::set_var("CLAVE_STATE_DIR", sb.state_dir());
        std::env::set_var("CLAVE_DATA_DIR", sb.data_dir());
    }
}

/// `clave dev launch`: the sandbox session in one short command — sets the
/// sandbox env (children inherit) and execs the NORMAL launch path.
/// Session lifecycle stays the user's: this exists to be typed BY the
/// user in a non-zellij terminal, replacing the printed env-var wall.
pub fn run_launch() -> Result<()> {
    let sb = crate::sandbox::Sandbox::resolve()?;
    sb.ensure()?;
    enter_sandbox(&sb);
    crate::setup::launch_session()
}

/// `clave dev instance`: which sandbox this working tree stages into.
///
/// Resolves AND materialises — it creates the root and stamps the `origin`
/// marker — because `scripts/sandbox-setup.sh` calls it as its first CLI
/// action and everything after that writes into the root. A root that exists
/// with no marker is un-reapable by design (`sandbox::verdict`), so the
/// marker must not lag behind the directory.
///
/// `--field` prints one raw value with no decoration, for the script.
pub fn run_instance(field: Option<&str>) -> Result<()> {
    let sb = crate::sandbox::Sandbox::resolve()?;
    sb.ensure()?;
    match field {
        None => {
            println!("session  {}", sb.session);
            println!("root     {}", sb.root.display());
            println!(
                "key      {}",
                sb.key.as_deref().unwrap_or("(main checkout — shared)")
            );
        }
        Some("session") => println!("{}", sb.session),
        Some("root") => println!("{}", sb.root.display()),
        Some("state") => println!("{}", sb.state_dir().display()),
        Some("data") => println!("{}", sb.data_dir().display()),
        Some("shim") => println!("{}", sb.shim_dir().display()),
        Some("key") => println!("{}", sb.key.as_deref().unwrap_or("")),
        Some(other) => anyhow::bail!("unknown --field {other:?}"),
    }
    Ok(())
}

/// The per-agent tag for a worktree's branch/dir name — the LAST 8 hex
/// digits, not the first (contrast `add.rs`'s real-uuid `&uuid[..8]`, which
/// is fine there because a real v4 uuid's first 8 chars ARE effectively
/// unique). `scenario_uuid` mints deterministic uuids of the shape
/// `00000000-0000-4000-8000-c85c{n:08}` — every one of them starts with the
/// literal `00000000`, so slicing the FRONT 8 chars gives the same tag to
/// every scenario agent. Live finding: `ux-gate1` is the first scenario with
/// two worktree agents in one repo, and `git worktree add -b clave/00000000`
/// twice failed closed with "a branch named 'clave/00000000' already
/// exists" — the collision c8-worktree's single worktree agent could never
/// surface. `{n:08}` sits at the string's tail, so the tail 8 chars vary.
fn uuid_tag(uuid: &str) -> &str {
    &uuid[uuid.len() - 8..]
}

/// The `repos/` dir name for one scenario agent. `Some(r)` is a SHARED name —
/// every agent in the scenario naming the same `r` lands in the same repo
/// dir, which is how `ux-gate1` gets one repo-ink across main/branch/worktree
/// rows. `None` reproduces the original `{scenario}-{slug}` naming, where
/// every agent was its own repo (still exactly what the three c8 scenarios
/// get, since none of them set `repo`).
fn repo_dir_name(scenario_name: &str, a: &ScenarioAgent) -> String {
    match a.repo {
        Some(r) => r.to_string(),
        None => format!("{scenario_name}-{}", a.slug),
    }
}

/// The store row for one scenario agent — pure (no filesystem or process
/// work), so it is the SAME function `run_scenario` seeds with and a render
/// test can call directly against synthetic paths. Keeping it one function is
/// what guarantees a render test proves what seeding will actually produce.
fn agent_record(
    scenario_name: &str,
    a: &ScenarioAgent,
    uuid: &str,
    cwd_str: &str,
    repo_root: &str,
    now: u64,
) -> crate::store::AgentRecord {
    let branch = if a.worktree {
        format!("clave/{}", uuid_tag(uuid))
    } else {
        a.branch.unwrap_or("main").to_string()
    };
    crate::store::AgentRecord {
        uuid: uuid.to_string(),
        cwd: cwd_str.to_string(),
        repo_root: repo_root.to_string(),
        branch,
        label: format!("{scenario_name}-{} · seeded", a.slug),
        status: a.status,
        last_interacted: now.saturating_sub(a.ago_secs),
        // Deliberately unminted (S1). Seeding runs BEFORE any session exists,
        // and `launch_session` calls `clear_session_order` on the way into a
        // create — whose backfill seeds ordinals from `last_interacted`, oldest
        // first. So the scenario's staggered recency survives into the ordinal
        // space, and the sandbox exercises the real upgrade path rather than a
        // special-cased one. Minting here would instead follow SEEDING order,
        // which has nothing to do with `ago_secs`.
        commit_ord: 0,
        last_visited: 0,
        worktree: a.worktree.then(|| cwd_str.to_string()),
        label_source: crate::store::LabelSource::FirstPrompt,
        tab_id: None,
        stale: false,
        title: a.title.map(String::from),
        summary: a.summary.to_string(),
        // S7 (#62). The LEVEL is not seeded — it is derived here from the same
        // bucketing the hook uses, against this machine's own smart zone, so a
        // sandbox run measures the real arithmetic rather than a number someone
        // typed. Change the zone and the seeded fleet re-colours accordingly,
        // which is itself the cheapest check that the env var is wired.
        context_tokens: a.context_tokens,
        context_level: a
            .context_tokens
            .map(|t| crate::hook::battery_level(t, crate::hook::smart_zone())),
        // `run_scenario` pins every seeded repo with `git init -q -b main`
        // (see the comment there), so this is the repo's REAL default, not a
        // guess — which is what makes `cold`'s blank provenance travel the
        // #86 known-default path rather than the `main`/`master` fallback.
        default_branch: Some("main".to_string()),
        // Seeded rows have no transcript at all, so there is no rotation to
        // model; the scenario's agents are always on their minted uuid.
        live_session: None,
    }
}

pub fn run_scenario(name: &str) -> Result<()> {
    let sc = SCENARIOS.iter().find(|s| s.name == name).with_context(|| {
        let names: Vec<_> = SCENARIOS.iter().map(|s| s.name).collect();
        format!("unknown scenario {name}; have: {names:?}")
    })?;
    let sb = crate::sandbox::Sandbox::resolve()?;
    sb.ensure()?;
    let root = sb.root.clone();
    force_sandbox(&sb);
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
    // built under target/ and copied into the sandbox data dir by
    // `just dev-install` (§2 —
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
        let repo = root.join("repos").join(repo_dir_name(name, a));
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
            let wt = repo.join(".claude-worktrees").join(uuid_tag(&uuid));
            ensure_worktree(&repo, &wt, &format!("clave/{}", uuid_tag(&uuid)))?;
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
                agent_record(name, a, &uuid, &cwd_str, &repo.to_string_lossy(), now),
            );
            s.seq += 1;
        })?;
        if a.delete_cwd_after {
            std::fs::remove_dir_all(&cwd)?; // the §6.3 staleness fixture
        }
    }
    crate::evlog::log_event("dev", &format!("scenario {name} seeded"));
    println!("\nScenario `{name}` ready. Launch (your command, in a NON-zellij terminal):\n");
    println!("  clave-dev dev launch");
    println!("\n(equivalent env form: {})", launch_command(&sb));
    println!("\nWhen done: `clave-dev dev reset` (prints the kill command first).");
    Ok(())
}

pub fn run_status() -> Result<()> {
    let sb = crate::sandbox::Sandbox::resolve()?;
    enter_sandbox(&sb);
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
    let live_session = crate::setup::session_is_live(&list, &sb.session);
    // Sanctioned §6.9 read: explicitly clave-test-scoped. GATED on
    // liveness (live finding, 2026-07-18): `zellij action` against an
    // absent/dead session BLOCKS indefinitely instead of erroring —
    // an ungated dump-layout hung `dev status` for minutes pre-launch.
    let dump = if live_session {
        Command::new(&zellij)
            .env("ZELLIJ_SESSION_NAME", &sb.session)
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
            // Which instance this answer is ABOUT: with a sandbox per
            // worktree, "session_live: false" is ambiguous until you know
            // which session was asked after.
            "session": sb.session,
            "root": sb.root,
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

/// The `~/.claude/projects/<munged-cwd>` directory names belonging to THIS
/// sandbox instance — one per scenario cwd that exists under `root/repos`.
///
/// This exists because scenario uuids are deterministic and therefore
/// IDENTICAL across instances (`scenario_uuid`), so `dev reset`'s old
/// machine-wide `c85c-*.jsonl` sweep deleted every other agent's scenario
/// transcripts as well as its own — and `scripts/sandbox-setup.sh` runs
/// `dev reset` on every staging run, so per-worktree roots alone would have
/// left that firing more often, not less.
///
/// EXACT names, never a prefix match on the munged root. Munging replaces
/// every non-alphanumeric character with `-` (`munge.rs`), so the main
/// checkout's `…-clave-dev` is also a prefix of a worktree's
/// `…-clave-dev-wt-a` — a prefix rule would reinstate the very deletion it
/// was written to stop.
///
/// Reset-twice-in-a-row leaks: the second call finds no `repos/` and so
/// names no directories. The leaked files are inert one-turn transcripts and
/// the next `dev scenario` reseeds over them; deleting by tag alone is the
/// thing that cannot be made safe.
fn scenario_project_dirs(root: &Path) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut note = |p: &Path| {
        // Canonicalize to match seeding: `run_scenario` munges the PHYSICAL
        // cwd (macOS /var -> /private/var), so an uncanonicalized path here
        // would name a directory Claude never created (munge.rs header).
        if let Some(s) = std::fs::canonicalize(p).ok().and_then(|c| {
            c.to_str()
                .map(crate::munge::munge_cwd)
                .filter(|s| !s.is_empty())
        }) {
            out.insert(s);
        }
    };
    let Ok(repos) = std::fs::read_dir(root.join("repos")) else {
        return out;
    };
    for repo in repos.flatten() {
        // A plain-checkout agent's cwd is the repo dir itself; a worktree
        // agent's is `<repo>/.claude-worktrees/<tag>` (see `run_scenario`).
        note(&repo.path());
        if let Ok(wts) = std::fs::read_dir(repo.path().join(".claude-worktrees")) {
            for wt in wts.flatten() {
                note(&wt.path());
            }
        }
    }
    out
}

/// Delete the c85c-tagged transcripts under `projects`, restricted to the
/// project directories `mine` names. Best-effort: a vanished directory or an
/// unreadable entry only skips itself.
fn sweep_scenario_transcripts(projects: &Path, mine: &std::collections::BTreeSet<String>) -> u32 {
    let mut removed = 0u32;
    for dir in mine {
        let Ok(files) = std::fs::read_dir(projects.join(dir)) else {
            continue;
        };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if is_scenario_jsonl(&name) && std::fs::remove_file(f.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

pub fn run_reset() -> Result<()> {
    let sb = crate::sandbox::Sandbox::resolve()?;
    let root = sb.root.clone();
    println!("If the session is running, kill it first (your command):\n");
    println!(
        "  zellij kill-session {0} ; zellij delete-session --force {0}\n",
        sb.session
    );
    // Named BEFORE the wipe: `repos/` is what says which project directories
    // are this instance's, and the wipe removes it.
    let mine = scenario_project_dirs(&root);
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
    // is_scenario_jsonl), scoped to this instance's own project dirs.
    let projects = crate::env::claude_config_dir()?.join("projects");
    let removed = sweep_scenario_transcripts(&projects, &mine);
    println!(
        "Scenario transcripts removed from {}: {removed} (in {} of this sandbox's project dirs)",
        projects.display(),
        mine.len()
    );
    Ok(())
}

/// `git worktree add`, made RE-RUNNABLE. `run_scenario` is deliberately
/// re-runnable without a `dev reset` — its `seed_needed` branch prints "already
/// seeded — reusing its transcript" — and until #86 that held for worktree
/// agents only by accident: a stale fixture used to be a plain checkout, so
/// `delete_cwd_after`'s `remove_dir_all(&cwd)` took the WHOLE `repos/<dir>` tree
/// with it and the next run rebuilt from nothing.
///
/// `ux-gate1` is the first scenario that breaks that. Its `vanished` agent
/// SHARES `repo: Some("clave")` with `cold`/`gate`, so deleting its cwd removes
/// only `.claude-worktrees/<tag>` while `repos/clave/.git` survives — including
/// the branch AND the now-dangling worktree registration. A second
/// `clave dev ux-gate1` then died at `git worktree add -b clave/<tag>` with "a
/// branch named … already exists"; `gate`, whose worktree dir is simply still
/// there, failed the same way one line earlier.
///
/// Idempotent in three steps: prune the registrations whose directory is gone,
/// leave an existing worktree dir alone, and CHECK OUT a surviving branch
/// instead of asking git to create it again.
fn ensure_worktree(repo: &Path, wt: &Path, branch: &str) -> Result<()> {
    // Drops the registration `remove_dir_all` orphaned; a no-op otherwise.
    run_in(repo, "git", &["worktree", "prune"])?;
    if wt.exists() {
        return Ok(()); // registered and on disk — already the goal state
    }
    let wt_str = wt.to_str().context("worktree path is not UTF-8")?;
    // `.output()`, not `.status()`: --quiet silences the ERROR, not the sha
    // this prints on success, and the seeding console is read by a human.
    let exists = Command::new("git")
        .current_dir(repo)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .with_context(|| format!("git rev-parse in {}", repo.display()))?
        .status
        .success();
    if exists {
        run_in(repo, "git", &["worktree", "add", "-q", wt_str, branch])
    } else {
        run_in(
            repo,
            "git",
            &["worktree", "add", "-q", "-b", branch, wt_str],
        )
    }
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

    /// Both scenario cwd shapes are named, and only real directories are.
    /// The worktree half is the one a "just list `repos/*`" implementation
    /// would drop, and a worktree agent's transcript is exactly the one whose
    /// loss is most visible in a live drive.
    #[test]
    fn scenario_project_dirs_names_both_plain_and_worktree_cwds() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("clave-dev-wt-a");
        let repo = root.join("repos").join("clave");
        let wt = repo.join(".claude-worktrees").join("00000001");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::create_dir_all(root.join("repos").join("other")).unwrap();

        let got = scenario_project_dirs(&root);

        // Canonicalized then munged, exactly as `run_scenario` seeds them.
        let want =
            |p: &Path| crate::munge::munge_cwd(std::fs::canonicalize(p).unwrap().to_str().unwrap());
        assert!(
            got.contains(&want(&repo)),
            "plain checkout missing: {got:?}"
        );
        assert!(got.contains(&want(&wt)), "worktree cwd missing: {got:?}");
        assert_eq!(got.len(), 3, "{got:?}"); // clave, clave's worktree, other
        // No `repos/` at all (a second `dev reset`) names nothing rather
        // than falling back to a machine-wide tag sweep.
        assert!(scenario_project_dirs(tmp.path()).is_empty());
    }

    /// The whole point: agent A's reset must not delete agent B's scenario
    /// transcripts, even though the uuids are byte-identical by design. The
    /// rival is the old tag-only sweep, which deletes all three files here.
    #[test]
    fn a_reset_sweeps_only_its_own_instances_transcripts() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join("projects");
        let jsonl = format!("{}.jsonl", scenario_uuid(1));

        let mine = "-h--local-state-clave-dev-wt-a-repos-clave";
        // The munged form of the MAIN checkout's root is a strict prefix of
        // the worktree one, so a prefix rule reads this as ours.
        let theirs = "-h--local-state-clave-dev-wt-b-repos-clave";
        let real_work = "-h-code-someones-actual-project";
        for d in [mine, theirs, real_work] {
            std::fs::create_dir_all(projects.join(d)).unwrap();
            std::fs::write(projects.join(d).join(&jsonl), b"{}").unwrap();
            std::fs::write(projects.join(d).join("real-session.jsonl"), b"{}").unwrap();
        }

        let removed =
            sweep_scenario_transcripts(&projects, &std::iter::once(mine.to_string()).collect());

        assert_eq!(removed, 1);
        assert!(!projects.join(mine).join(&jsonl).exists());
        assert!(projects.join(theirs).join(&jsonl).exists(), "clobbered B");
        assert!(projects.join(real_work).join(&jsonl).exists());
        // And a non-scenario transcript is never touched, in any directory.
        for d in [mine, theirs, real_work] {
            assert!(projects.join(d).join("real-session.jsonl").exists());
        }
    }

    #[test]
    fn scenario_table_covers_the_c8_checklist() {
        // Names map 1:1 to SUBSYSTEM-VALIDATION.md C8 steps, plus ux-gate1
        // (the visual-design decision fixture, #85 follow-up). Exact list on
        // purpose (task instruction): a `contains` would let a scenario go
        // missing silently.
        let names: Vec<&str> = SCENARIOS.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec!["c8-cold-start", "c8-worktree", "c8-stale", "ux-gate1"]
        );
        // cold-start: 3 agents, staggered recency, none worktree.
        let cs = &SCENARIOS[0];
        assert_eq!(cs.agents.len(), 3);
        assert!(cs.agents.iter().all(|a| !a.worktree && !a.delete_cwd_after));
        // worktree: exactly one worktree agent.
        assert!(SCENARIOS[1].agents.iter().any(|a| a.worktree));
        // stale: exactly one agent whose cwd the scenario deletes.
        assert!(SCENARIOS[2].agents.iter().any(|a| a.delete_cwd_after));
        // c8-* agents are untouched by the new ScenarioAgent fields — the
        // ..DEFAULT struct-update must reproduce exactly the old inline
        // hardcoding (title None, summary "", status Idle, no repo/branch
        // override), or the three reviewed validation paths silently change.
        for sc in &SCENARIOS[..3] {
            for a in sc.agents {
                assert_eq!(a.title, None);
                assert_eq!(a.summary, "");
                assert_eq!(a.status, clave_types::Status::Idle);
                assert_eq!(a.repo, None);
                assert_eq!(a.branch, None);
            }
        }
    }

    #[test]
    fn ux_gate1_exercises_the_whole_visual_design() {
        // Every field the render test below turns into pixels, checked
        // structurally first so a future edit that breaks one property fails
        // with a clear name instead of a mysterious render assertion.
        let sc = SCENARIOS.iter().find(|s| s.name == "ux-gate1").unwrap();
        assert!(
            (6..=8).contains(&sc.agents.len()),
            "want 6-8 agents, got {}",
            sc.agents.len()
        );

        // Every Status variant appears at least once.
        use clave_types::Status;
        for want in [
            Status::NeedsYou,
            Status::Working,
            Status::Done,
            Status::Idle,
            Status::Failed,
        ] {
            assert!(
                sc.agents.iter().any(|a| a.status == want),
                "missing status {want:?}"
            );
        }

        // All three provenances: at least one worktree, one plain branch
        // override, and one plain main (no worktree, no branch override).
        assert!(sc.agents.iter().any(|a| a.worktree));
        assert!(sc.agents.iter().any(|a| !a.worktree && a.branch.is_some()));
        assert!(sc.agents.iter().any(|a| !a.worktree && a.branch.is_none()));

        // At least one title, at least one blank chip, most have a title.
        let titled = sc.agents.iter().filter(|a| a.title.is_some()).count();
        assert!(titled >= 1 && titled < sc.agents.len());

        // Every summary is non-empty prose, not the old blank default.
        assert!(sc.agents.iter().all(|a| !a.summary.is_empty()));

        // Repos are SHORT and mutually distinguishable by their first three
        // characters — the collapsed profile's whole repo column (D17).
        let repos: std::collections::BTreeSet<&str> = sc
            .agents
            .iter()
            .map(|a| a.repo.expect("ux-gate1 names every agent's repo"))
            .collect();
        assert!(
            (4..=6).contains(&repos.len()),
            "want 4-6 distinct repos, got {repos:?}"
        );
        let prefixes: std::collections::BTreeSet<&str> =
            repos.iter().map(|r| &r[..r.len().min(3)]).collect();
        assert_eq!(
            prefixes.len(),
            repos.len(),
            "two repos share a 3-char prefix: {repos:?}"
        );

        // At least one stale fixture, and (per the run_scenario safety note
        // above) it must not be a plain checkout sharing a repo with a live
        // agent's cwd — it has to be its own worktree.
        let stale: Vec<&ScenarioAgent> = sc.agents.iter().filter(|a| a.delete_cwd_after).collect();
        assert!(!stale.is_empty());
        assert!(stale.iter().all(|a| a.worktree));
    }

    #[test]
    fn ux_gate1_renders_the_locked_visual_design() {
        // The real pipeline, not a reimplementation: store::snapshot_from →
        // BarModel::apply_snapshot → BarModel::rows() → render::render_rows —
        // the SAME functions the plugin renders with (render.rs's own header
        // comment says as much of render_rows + bar-preview). Every agent
        // gets a synthetic tab_id and a matching TabMeta so `agent_content`
        // reads its TRUE Status rather than demoting every row to
        // `RowStatus::Dormant` (model.rs's `is_dormant` is keyed on tab
        // liveness) — a freshly seeded, nobody's-opened-it-yet row rendering
        // dormant is correct sandbox behaviour, but this test's job is to prove
        // the FIELDS the scenario writes render well once opened, which is
        // the state the maintainer's fleet is in for the actual review.
        use clave_bar::model::{BarModel, TabMeta};
        use clave_bar::render::{
            COLLAPSED_DESIGN_COLS, DESIGN_COLS, Provenance, RowContent, RowStatus, Widths,
            display_cells, render_rows, strip_sgr,
        };

        let sc = SCENARIOS.iter().find(|s| s.name == "ux-gate1").unwrap();
        let now = 2_000_000_000u64;
        let mut store = crate::store::Store::default();
        for (i, a) in sc.agents.iter().enumerate() {
            let uuid = scenario_uuid(i as u32 + 1);
            let repo_root = format!("/sandbox/repos/{}", repo_dir_name("ux-gate1", a));
            let cwd = if a.worktree {
                format!("{repo_root}/.claude-worktrees/{}", uuid_tag(&uuid))
            } else {
                repo_root.clone()
            };
            let mut record = agent_record("ux-gate1", a, &uuid, &cwd, &repo_root, now);
            record.tab_id = Some(i); // simulate: every row open in a live tab
            store.agents.insert(uuid, record);
        }
        store.seq = 1;

        let snapshot = crate::store::snapshot_from(&store);
        let mut model = BarModel::default();
        model.apply_snapshot(snapshot);
        // ONE tab is focused. A fleet with nothing selected renders no
        // selected-row caps, no waveBlue2 background and — because recession is
        // RELATIVE (lock §6) — no 25% fade on anybody: three quarters of the
        // design would go unexercised against real scenario data, which is the
        // half of the review this test exists to stand in for.
        model.apply_tabs(
            (0..sc.agents.len())
                .map(|i| TabMeta {
                    tab_id: i,
                    position: i,
                    name: format!("tab-{i}"),
                    active: i == 0,
                })
                .collect(),
        );

        let rows: Vec<_> = model.rows().into_iter().map(|(_, row)| row).collect();
        assert_eq!(rows.len(), sc.agents.len());
        assert_eq!(
            rows.iter().filter(|r| r.selected).count(),
            1,
            "exactly one row must carry the selection"
        );

        // The same rows with nothing selected — the control for the fade check
        // below, hoisted because both width profiles reuse it.
        let unfaded: Vec<_> = rows
            .iter()
            .cloned()
            .map(|mut r| {
                r.selected = false;
                r
            })
            .collect();

        // BOTH profiles, each against ITS OWN target width (review #86). This
        // used to render `Widths::EXPANDED` at `DESIGN_COLS` only, which left
        // the collapsed half of the design unexercised — and collapsed is where
        // this scenario's short, 3-character-distinct repo names are actually
        // load-bearing (D17's 3-cell repo column, D18's suppressed ellipsis). A
        // repo field truncating below 3 cells, or a row missing the narrower
        // target, would have passed.
        for (widths, cols) in [
            (Widths::EXPANDED, DESIGN_COLS),
            (Widths::COLLAPSED, COLLAPSED_DESIGN_COLS),
        ] {
            // Design-lock invariant, proven rather than asserted in prose:
            // every row is exactly the profile's target in display cells
            // (bar-preview.rs does the same measurement) — INCLUDING the
            // selected row, whose caps and full-width background are the
            // easiest thing to render one cell wide.
            let lines = render_rows(&rows, cols, rows.len(), widths);
            for (line, row) in lines.iter().zip(&rows) {
                let width = display_cells(&strip_sgr(line));
                assert_eq!(width, cols, "row is {width} cells at {cols}: {row:?}");
            }
            // The selected row is the only one with the waveBlue2 background,
            // and every other row is faded 25% toward it (lock §6) — the two
            // halves of recession, checked against scenario data rather than a
            // fixture.
            let (sel, rest): (Vec<_>, Vec<_>) =
                lines.iter().zip(&rows).partition(|(_, r)| r.selected);
            assert!(
                sel[0].0.contains("48;2;45;79;103"),
                "no selected background at {cols}"
            );
            for (line, _) in &rest {
                assert!(
                    !line.contains("48;2;45;79;103"),
                    "an unselected row carries the selection background at {cols}"
                );
            }
            // And the fade is real, not just claimed: the unselected control
            // renders at full strength, so every line must differ from its
            // faded self. `mix` rounds ties to even (a ported Python detail) —
            // a fade that silently stopped applying would leave these
            // byte-identical.
            for (faded, plain) in lines.iter().zip(render_rows(&unfaded, cols, unfaded.len(), widths)) {
                assert_ne!(*faded, plain, "recession did not change this row at {cols}");
            }
        }

        let agent_field = |row: &clave_bar::render::Row| match &row.content {
            RowContent::Agent {
                status,
                provenance,
                title,
                ..
            } => (*status, *provenance, title.clone()),
            RowContent::Terminal { .. } => unreachable!("ux-gate1 seeds only agents"),
        };
        let fields: Vec<_> = rows.iter().map(agent_field).collect();

        // Every status is represented, and the render disagrees on colour —
        // proof this scenario is not "all one glyph" (the whole point).
        for want in [
            RowStatus::NeedsYou,
            RowStatus::Working,
            RowStatus::Done,
            RowStatus::Idle,
            RowStatus::Failed,
        ] {
            assert!(
                fields.iter().any(|(s, ..)| *s == want),
                "missing rendered status {want:?}"
            );
        }

        // Every provenance is represented, including the blank main mark.
        for want in [Provenance::Main, Provenance::Branch, Provenance::Worktree] {
            assert!(
                fields.iter().any(|(_, p, _)| *p == want),
                "missing rendered provenance {want:?}"
            );
        }

        // At least one blank title chip and at least one filled one.
        assert!(fields.iter().any(|(_, _, t)| t.is_none()));
        assert!(fields.iter().any(|(_, _, t)| t.is_some()));
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
    fn uuid_tag_differs_across_scenario_uuids() {
        // Live finding: scenario uuids ALL start "00000000" (scenario_uuid's
        // deterministic shape), so slicing the FRONT 8 chars (the pattern
        // `add.rs` uses on real, effectively-random uuids) gives every
        // scenario agent the identical tag — `git worktree add -b
        // clave/00000000` collided the moment ux-gate1 put a second worktree
        // agent in one repo. The tag must vary per agent, or a repo with 2+
        // worktree agents can never seed.
        assert_ne!(uuid_tag(&scenario_uuid(1)), uuid_tag(&scenario_uuid(2)));
        assert_eq!(uuid_tag(&scenario_uuid(1)), "00000001");
        assert_eq!(uuid_tag(&scenario_uuid(2)), "00000002");
    }

    #[test]
    fn ensure_worktree_is_re_runnable_over_a_shared_repo() {
        // Review finding #86, reproduced against real git. `run_scenario` is
        // designed to be re-run WITHOUT a `dev reset` — its `seed_needed`
        // branch says so in as many words — and `ux-gate1` is the first
        // scenario for which that was false: two worktree agents share one
        // repo, so the stale fixture's `remove_dir_all(&cwd)` takes only
        // `.claude-worktrees/<tag>` and leaves `.git` — branch, worktree
        // registration and all. Both of the shapes below made a bare
        // `git worktree add -b` fail closed with "a branch named … already
        // exists"; this test runs the whole thing THREE times.
        //
        // Shells out to real git deliberately: the bug lives entirely in git's
        // semantics, so a mocked one would have agreed with the broken code.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repos").join("clave");
        std::fs::create_dir_all(&repo).unwrap();
        run_in(&repo, "git", &["init", "-q", "-b", "main"]).unwrap();
        // HERMETIC, not decorative: this passed locally and failed in CI on
        // exactly this line. A developer machine supplies user.name/user.email
        // from ~/.gitconfig and a runner does not, so the test was reading its
        // environment rather than its fixture. `commit.gpgsign=false` is here
        // for the same reason in reverse — the maintainer signs every commit
        // globally, and a runner has no key to sign with.
        for (k, v) in [
            ("user.email", "test@example.invalid"),
            ("user.name", "clave test"),
            ("commit.gpgsign", "false"),
        ] {
            run_in(&repo, "git", &["config", k, v]).unwrap();
        }
        run_in(
            &repo,
            "git",
            &["commit", "--allow-empty", "-q", "-m", "seed"],
        )
        .unwrap();
        // Two worktree agents in ONE repo, exactly `gate` and `vanished`.
        let gate = repo.join(".claude-worktrees").join("00000002");
        let vanished = repo.join(".claude-worktrees").join("00000007");
        for run in 1..=3 {
            ensure_worktree(&repo, &gate, "clave/00000002")
                .unwrap_or_else(|e| panic!("run {run}: gate: {e:#}"));
            ensure_worktree(&repo, &vanished, "clave/00000007")
                .unwrap_or_else(|e| panic!("run {run}: vanished: {e:#}"));
            assert!(gate.is_dir(), "run {run}: gate's worktree is missing");
            assert!(
                vanished.is_dir(),
                "run {run}: vanished's worktree is missing"
            );
            // The §6.3 staleness fixture, applied to the SAME repo the live
            // agents share — the whole reason the branch outlives its dir.
            std::fs::remove_dir_all(&vanished).unwrap();
        }
    }

    #[test]
    fn ux_gate1_worktree_agents_get_distinct_branch_tags() {
        // The regression this scenario itself hit: two worktree agents
        // (`gate`, `vanished`) share `repo: Some("clave")`. Prove their
        // MINTED store branches (the same field `git worktree add -b`
        // consumes) never collide, for every repo any scenario shares.
        for sc in SCENARIOS {
            let mut by_repo: std::collections::BTreeMap<&str, Vec<String>> =
                std::collections::BTreeMap::new();
            for (i, a) in sc.agents.iter().enumerate() {
                if !a.worktree {
                    continue;
                }
                let uuid = scenario_uuid(i as u32 + 1);
                let branch = format!("clave/{}", uuid_tag(&uuid));
                by_repo
                    .entry(a.repo.unwrap_or(a.slug))
                    .or_default()
                    .push(branch);
            }
            for (repo, branches) in by_repo {
                let unique: std::collections::BTreeSet<&String> = branches.iter().collect();
                assert_eq!(
                    unique.len(),
                    branches.len(),
                    "{}: repo {repo} has colliding worktree branches: {branches:?}",
                    sc.name
                );
            }
        }
    }

    #[test]
    fn launch_command_sandboxes_clave_state_only() {
        // §6.9 revised 2026-07-18: CLAVE state is sandboxed; claude's
        // identity is deliberately NOT (thin-wrapper ruling — sandboxing
        // it dragged auth along and broke seeding).
        let main = crate::sandbox::Sandbox::new(std::path::Path::new("/h"), None, None);
        let cmd = launch_command(&main);
        assert!(cmd.contains("CLAVE_SESSION=clave-test"));
        assert!(cmd.contains("CLAVE_STATE_DIR=/h/.local/state/clave-dev/state"));
        assert!(cmd.contains("CLAVE_DATA_DIR=/h/.local/state/clave-dev/data"));
        assert!(!cmd.contains("CLAUDE_CONFIG_DIR"));
        // clave-dev, not clave (#43b): dev-install stopped writing the
        // daily name, so the printed command must name what it installs.
        assert!(cmd.trim_end().ends_with("clave-dev"));
    }

    /// The printed command is meant to be pasted into a fresh terminal in an
    /// unknown directory, so it must carry the WORKTREE's instance and not
    /// the one `dev launch` would derive from wherever it is run. Witness:
    /// every one of the three values differs from the main checkout's.
    #[test]
    fn launch_command_carries_this_worktrees_instance_not_the_shared_one() {
        let wt = crate::sandbox::Sandbox::new(
            std::path::Path::new("/h"),
            Some("prune-wt".into()),
            Some("/h/code/clave/wt/prune-wt".into()),
        );
        let cmd = launch_command(&wt);
        assert!(cmd.contains("CLAVE_SESSION=clave-test-prune-wt"), "{cmd}");
        assert!(
            cmd.contains("CLAVE_STATE_DIR=/h/.local/state/clave-dev-prune-wt/state"),
            "{cmd}"
        );
        assert!(
            cmd.contains("CLAVE_DATA_DIR=/h/.local/state/clave-dev-prune-wt/data"),
            "{cmd}"
        );
    }

    /// An explicitly set variable wins, which is what makes the pasted
    /// `launch_command` truthful. The rival is the old unconditional
    /// `set_var`, under which the middle case would also be derived.
    #[test]
    fn an_explicit_env_value_wins_over_the_derived_instance() {
        assert!(env_should_be_derived(None));
        assert!(env_should_be_derived(Some(""))); // empty means unset, per env.rs
        assert!(!env_should_be_derived(Some("clave-test-prune-wt")));
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
