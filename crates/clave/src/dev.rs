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
    /// #182 (qa-fleet): seed a SECOND real transcript at
    /// `scenario_rotated_uuid` and point the row's `live_session` at it —
    /// modeling the #99 rotation (`/clear`, or any fresh conversation on the
    /// same pane) that `resume_target`/`verified_site` exist to prefer. A
    /// plain agent's `live_session` stays `None`: it never rotated, so the
    /// minted uuid IS the live conversation.
    pub rotated: bool,
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
    /// The card's identity cells (#232), seeded because a dormant row fires
    /// no hooks: without them a live sandbox paints the top half of every
    /// card and leaves the rest blank, which is the one thing the README's
    /// traced SVG cannot tell us — whether the real renderer fills a full
    /// card from a real store. `None` is still worth seeding on some rows:
    /// it is the distinct "nothing read yet" cell.
    pub provider: Option<&'static str>,
    pub model: Option<&'static str>,
    pub effort: Option<&'static str>,
    /// A pull request the branch is driving. A seeded row reads FRESH
    /// (`agent_record` stamps the lookup time), so no background sync
    /// replaces the number before anyone sees it.
    pub pr_number: Option<u32>,
    /// What a waiting agent is blocked on. Only ever set with
    /// `Status::NeedsYou`: structure decides that a row is waiting, and the
    /// words only say what for (design lock §4.7).
    pub wants: Option<&'static str>,
    /// Agents still running under this one. A flag, not a count.
    pub subagents: bool,
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
        rotated: false,
        repo: None,
        branch: None,
        title: None,
        summary: "",
        status: clave_types::Status::Idle,
        context_tokens: None,
        provider: None,
        model: None,
        effort: None,
        pr_number: None,
        wants: None,
        subagents: false,
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
    // #148: rows far past any normal pane height, to drive the viewport
    // live. Nothing here is exercised in isolation the way c8-* / ux-gate1
    // are — its only job is overflow.
    Scenario {
        name: "tall",
        agents: &[
            // A handful of live-style rows: recent, active statuses, on top.
            ScenarioAgent {
                slug: "live-a",
                ago_secs: 60,
                repo: Some("clave"),
                status: clave_types::Status::Working,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "live-b",
                ago_secs: 120,
                repo: Some("clave"),
                status: clave_types::Status::NeedsYou,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "live-c",
                ago_secs: 180,
                repo: Some("clave"),
                status: clave_types::Status::Working,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "live-d",
                ago_secs: 240,
                repo: Some("clave"),
                status: clave_types::Status::Idle,
                ..ScenarioAgent::DEFAULT
            },
            // ~30 dormant rows, varied statuses/repos, staggered recency —
            // enough to overflow any normal pane. #173: they also climb the
            // battery ramp, 8k → 153k in 5k steps, so every fill step from
            // full to empty is on screen at once (and the tail sits past the
            // default smart zone, exercising the clamp). Seeded rows never
            // run a hook, so a count typed here is the only way the gauge is
            // eyeballable in a sandbox.
            ScenarioAgent {
                slug: "d01",
                ago_secs: 3600,
                repo: Some("clave"),
                status: clave_types::Status::Idle,
                context_tokens: Some(8_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d02",
                ago_secs: 5400,
                repo: Some("nalu"),
                status: clave_types::Status::Working,
                context_tokens: Some(13_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d03",
                ago_secs: 7200,
                repo: Some("infra"),
                status: clave_types::Status::NeedsYou,
                context_tokens: Some(18_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d04",
                ago_secs: 9000,
                repo: Some("webapp"),
                status: clave_types::Status::Failed,
                context_tokens: Some(23_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d05",
                ago_secs: 10800,
                repo: Some("docs"),
                status: clave_types::Status::Done,
                context_tokens: Some(28_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d06",
                ago_secs: 12600,
                repo: Some("api"),
                status: clave_types::Status::Idle,
                context_tokens: Some(33_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d07",
                ago_secs: 14400,
                repo: Some("mobile"),
                status: clave_types::Status::Working,
                context_tokens: Some(38_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d08",
                ago_secs: 16200,
                repo: Some("cli"),
                status: clave_types::Status::NeedsYou,
                context_tokens: Some(43_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d09",
                ago_secs: 18000,
                repo: Some("edge"),
                status: clave_types::Status::Failed,
                context_tokens: Some(48_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d10",
                ago_secs: 19800,
                repo: Some("auth"),
                status: clave_types::Status::Done,
                context_tokens: Some(53_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d11",
                ago_secs: 21600,
                repo: Some("clave"),
                status: clave_types::Status::Idle,
                context_tokens: Some(58_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d12",
                ago_secs: 23400,
                repo: Some("nalu"),
                status: clave_types::Status::Working,
                context_tokens: Some(63_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d13",
                ago_secs: 25200,
                repo: Some("infra"),
                status: clave_types::Status::NeedsYou,
                context_tokens: Some(68_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d14",
                ago_secs: 27000,
                repo: Some("webapp"),
                status: clave_types::Status::Failed,
                context_tokens: Some(73_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d15",
                ago_secs: 28800,
                repo: Some("docs"),
                status: clave_types::Status::Done,
                context_tokens: Some(78_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d16",
                ago_secs: 30600,
                repo: Some("api"),
                status: clave_types::Status::Idle,
                context_tokens: Some(83_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d17",
                ago_secs: 32400,
                repo: Some("mobile"),
                status: clave_types::Status::Working,
                context_tokens: Some(88_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d18",
                ago_secs: 34200,
                repo: Some("cli"),
                status: clave_types::Status::NeedsYou,
                context_tokens: Some(93_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d19",
                ago_secs: 36000,
                repo: Some("edge"),
                status: clave_types::Status::Failed,
                context_tokens: Some(98_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d20",
                ago_secs: 37800,
                repo: Some("auth"),
                status: clave_types::Status::Done,
                context_tokens: Some(103_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d21",
                ago_secs: 39600,
                repo: Some("clave"),
                status: clave_types::Status::Idle,
                context_tokens: Some(108_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d22",
                ago_secs: 41400,
                repo: Some("nalu"),
                status: clave_types::Status::Working,
                context_tokens: Some(113_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d23",
                ago_secs: 43200,
                repo: Some("infra"),
                status: clave_types::Status::NeedsYou,
                context_tokens: Some(118_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d24",
                ago_secs: 45000,
                repo: Some("webapp"),
                status: clave_types::Status::Failed,
                context_tokens: Some(123_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d25",
                ago_secs: 46800,
                repo: Some("docs"),
                status: clave_types::Status::Done,
                context_tokens: Some(128_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d26",
                ago_secs: 48600,
                repo: Some("api"),
                status: clave_types::Status::Idle,
                context_tokens: Some(133_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d27",
                ago_secs: 50400,
                repo: Some("mobile"),
                status: clave_types::Status::Working,
                context_tokens: Some(138_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d28",
                ago_secs: 52200,
                repo: Some("cli"),
                status: clave_types::Status::NeedsYou,
                context_tokens: Some(143_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d29",
                ago_secs: 54000,
                repo: Some("edge"),
                status: clave_types::Status::Failed,
                context_tokens: Some(148_000),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "d30",
                ago_secs: 55800,
                repo: Some("auth"),
                status: clave_types::Status::Done,
                context_tokens: Some(153_000),
                ..ScenarioAgent::DEFAULT
            },
        ],
    },
    // #182: the QA drive's fleet — six DORMANT rows only. The eager,
    // live-style row the drive checklist opens with is whichever the
    // human's launch line names (see docs/dev/QA-DRIVE.md), not seeded
    // here — seeding a fake "live" row would just be a second dormant row
    // wearing a different status.
    Scenario {
        name: "qa-fleet",
        agents: &[
            ScenarioAgent {
                slug: "steady",
                ago_secs: 300,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "settled",
                ago_secs: 900,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "quiet",
                ago_secs: 5_400,
                ..ScenarioAgent::DEFAULT
            },
            // Shares a repo with `ghost` below — the pinned caveat
            // (run_scenario's `ensure_worktree` doc comment / ux-gate1's
            // `vanished`): a stale agent sharing a repo MUST be a worktree,
            // or its `delete_cwd_after` would `remove_dir_all` the shared
            // repo dir out from under this row too.
            ScenarioAgent {
                slug: "wt",
                ago_secs: 600,
                worktree: true,
                repo: Some("qa-fleet-shared"),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "ghost",
                ago_secs: 7_200,
                worktree: true,
                delete_cwd_after: true,
                repo: Some("qa-fleet-shared"),
                ..ScenarioAgent::DEFAULT
            },
            // The rotated row (#182): a second real transcript at
            // `scenario_rotated_uuid`, with `live_session` pointed at it —
            // proof the QA drive resumes on the ROTATED conversation, not
            // the frozen minted one.
            ScenarioAgent {
                slug: "rotated",
                ago_secs: 120,
                rotated: true,
                ..ScenarioAgent::DEFAULT
            },
        ],
    },
    // The screenshot fleet: the row vocabulary in one frame, for the sample
    // image AGENTS.md sends every agent to read before anything else.
    //
    // It mirrors `crates/clave-bar/examples/shared/showcase_fixture.rs`,
    // which the README's SVG is traced from, so the photograph and the
    // drawing show the same fleet. The drawing is the design's own output
    // and stays canonical; this is the only way to learn whether the REAL
    // renderer fills a full card from a REAL store, which is the thing a
    // traced SVG can never tell us.
    //
    // Every row here is dormant until someone opens it — seeded rows always
    // are. That is the point of seeding the identity cells: a dormant row
    // fires no hooks, so without them the capture shows nine half-empty
    // cards. Open two or three rows before the capture and the frame carries
    // live rows beside dormant ones, which is the honest daily picture.
    Scenario {
        name: "showcase",
        agents: &[
            ScenarioAgent {
                slug: "gutter",
                ago_secs: 720,
                worktree: true,
                repo: Some("clave"),
                title: Some("S6-GUT"),
                summary: "Wire the status column into render_rows",
                status: clave_types::Status::Working,
                context_tokens: Some(108_000),
                provider: Some("claude"),
                model: Some("sonnet"),
                effort: Some("hi"),
                pr_number: Some(232),
                ..ScenarioAgent::DEFAULT
            },
            // No chip and no PR: a blank cell is part of the vocabulary, and
            // the row must not shift because of it.
            ScenarioAgent {
                slug: "spawn",
                ago_secs: 1_500,
                repo: Some("clave"),
                summary: "Review the spawn identity gate",
                status: clave_types::Status::Idle,
                context_tokens: Some(141_000),
                provider: Some("claude"),
                model: Some("opus"),
                effort: Some("xh"),
                ..ScenarioAgent::DEFAULT
            },
            // Past its smart zone (412k against a 150k default), with agents
            // still running under it. A worktree, because it shares `clave`
            // with the rows above and `delete_cwd_after` removes exactly
            // `cwd` — see `ensure_worktree` and ux-gate1's `vanished`.
            ScenarioAgent {
                slug: "kdl",
                ago_secs: 86_400,
                worktree: true,
                delete_cwd_after: true,
                repo: Some("clave"),
                title: Some("KDL-GRD"),
                summary: "Validate generated KDL artifacts",
                status: clave_types::Status::Working,
                context_tokens: Some(412_000),
                provider: Some("claude"),
                model: Some("fable"),
                effort: Some("hi"),
                pr_number: Some(219),
                subagents: true,
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "auth",
                ago_secs: 240,
                repo: Some("api-svc"),
                branch: Some("key-rotation"),
                title: Some("AUTH-7"),
                summary: "Rotate the signing keys",
                status: clave_types::Status::NeedsYou,
                context_tokens: Some(52_000),
                provider: Some("claude"),
                model: Some("fable"),
                effort: Some("mx"),
                pr_number: Some(184),
                wants: Some("Bash (cargo publish)"),
                ..ScenarioAgent::DEFAULT
            },
            ScenarioAgent {
                slug: "cart",
                ago_secs: 3_600,
                repo: Some("webapp"),
                title: Some("CART-99"),
                summary: "Fix cart total rounding mismatch",
                status: clave_types::Status::Done,
                context_tokens: Some(18_000),
                provider: Some("claude"),
                model: Some("haiku"),
                effort: Some("lo"),
                ..ScenarioAgent::DEFAULT
            },
            // The other provider, and no effort reading — Codex is not
            // driveable yet, but a row can already carry its icon.
            ScenarioAgent {
                slug: "dns",
                ago_secs: 10_800,
                repo: Some("infra"),
                branch: Some("dns-timeout"),
                title: Some("DNS-TTL"),
                summary: "Debug staging rollout DNS timeout",
                status: clave_types::Status::Failed,
                context_tokens: Some(79_000),
                provider: Some("openai"),
                model: Some("5.6sol"),
                pr_number: Some(77),
                ..ScenarioAgent::DEFAULT
            },
            // Two weeks cold, and no reading at all on the last row: the
            // "nothing measured yet" cell beside eight that have one.
            ScenarioAgent {
                slug: "zsh",
                ago_secs: 1_209_600,
                repo: Some("notes"),
                title: Some("ZSH"),
                summary: "Tidy the shell startup files",
                status: clave_types::Status::Idle,
                provider: Some("claude"),
                model: Some("haiku"),
                effort: Some("md"),
                ..ScenarioAgent::DEFAULT
            },
        ],
    },
];

/// Valid v4-shaped, deterministic, self-identifying (`c85c` ≈ c8 scenario).
pub fn scenario_uuid(n: u32) -> String {
    format!("00000000-0000-4000-8000-c85c{n:08}")
}

/// The SECOND mint for a `rotated: true` agent (#182) — the transcript a
/// `/clear` (or any fresh conversation on the same pane) would leave behind,
/// which `live_session` then names. `+ 50` keeps the shared `c85c` prefix (so
/// `is_scenario_jsonl` still sweeps it on `dev reset`) while landing well
/// past any `scenario_uuid(n)` a single scenario mints today — the largest,
/// `tall`, tops out at 34 agents — so the two mints never collide, and
/// `uuid_tag` (last-8-chars) still differs between an agent's own pair.
pub fn scenario_rotated_uuid(n: u32) -> String {
    format!("00000000-0000-4000-8000-c85c{:08}", n + 50)
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
/// Deliberately NO CLAUDE_CONFIG_DIR (revised 2026-07-18, live finding +
/// user ruling): sandboxing claude's identity dragged auth along with it
/// ("Not logged in" / stale-credential failures) — clave is a thin wrapper
/// for terminal control, and claude's identity is not its business. The
/// sandbox isolates CLAVE's state only; scenario transcripts land in the
/// real ~/.claude/projects tagged by the deterministic c85c uuids, and
/// `dev reset` removes them by that tag.
///
/// An explicitly set variable WINS, so a caller that names an instance
/// explicitly (a drive script, a test harness) keeps it.
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
    // A sandbox session cannot be launched from INSIDE a zellij session: it
    // would nest, and the agent driving all of this is always inside one.
    // So the rule "launching is the human's" stops being prose a reader has
    // to find and becomes a refusal the machine issues — which is the same
    // move as `aim_push` and the drive's identity scrub, for the same reason:
    // a rule nothing enforces is one edit, or one new agent, from being gone.
    if std::env::var_os("ZELLIJ").is_some() {
        anyhow::bail!("{}", nested_launch_refusal(&sb));
    }
    sb.ensure()?;
    enter_sandbox(&sb);
    shim_first_on_path(&sb);
    crate::setup::launch_session()
}

/// What to say to whoever just tried to launch from inside a session. Split
/// out to be testable, and written for a reader with ZERO context: it names
/// the one thing they should do instead, in full, rather than describing a
/// rule and leaving them to derive the command.
fn nested_launch_refusal(sb: &crate::sandbox::Sandbox) -> String {
    let cd = match &sb.origin {
        Some(o) => format!("cd {}\n    ", o.display()),
        None => String::new(),
    };
    format!(
        "refusing to launch '{}' from inside a zellij session (ZELLIJ is set).\n\
         \n\
         Launching is the MAINTAINER's step and belongs in a new terminal\n\
         window outside zellij — nesting a sandbox inside a live session is\n\
         how the two get confused for each other. An agent cannot do this and\n\
         should not try: print the command and let the human run it.\n\
         \n    \
         {cd}just launch\n\
         \n\
         Staging (yours) is `just sandbox <scenario>`; `just qa <scenario>`\n\
         stages, waits for that launch, and drives the whole QA loop.",
        sb.session
    )
}

/// The shim FIRST on `PATH`, so a bare `clave` anywhere inside the launched
/// session resolves to the build under test rather than the stable install.
///
/// This is not cosmetic — it is the #43/#44 leak: the bar shells out to a
/// bare `clave` (`clave_binary "clave"` in the generated config) and every
/// Claude Code hook runs `clave hook <Event>`, so without the shim a sandbox
/// silently tests whatever is in `~/.local/share/clave/bin`. It was the
/// caller's job until now, pasted into a five-line `PATH=... CLAVE_...=...`
/// prefix on every single launch, which is exactly the kind of step that gets
/// dropped once and then debugged for an hour. `dev launch` already derives
/// the three `CLAVE_*` vars (`enter_sandbox`); deriving the fourth makes the
/// prefix unnecessary rather than merely tedious.
fn shim_first_on_path(sb: &crate::sandbox::Sandbox) {
    let shim = sb.shim_dir();
    let current = std::env::var_os("PATH").unwrap_or_default();
    // Idempotent: a caller still pasting the old prefix has it first already,
    // and a second copy would only make `PATH` harder to read in a bug report.
    if let Some(s) = current.to_str()
        && s.split(':').next() == shim.to_str()
    {
        return;
    }
    let mut joined = std::ffi::OsString::from(&shim);
    joined.push(":");
    joined.push(&current);
    // SAFETY: single-threaded CLI entry point; set before any spawn.
    unsafe { std::env::set_var("PATH", joined) };
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
    n: u32,
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
        branch: branch.clone(),
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
        pane_id: None,
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
        // #182: most seeded rows never rotate, so `live_session` stays
        // `None` — agreement with the minted uuid is never stored (see
        // spawn.rs's `resume_target` doc comment). A `rotated: true` agent
        // is the exception: `run_scenario` seeds a SECOND real transcript at
        // `scenario_rotated_uuid(n)`, and this is where the row learns to
        // point at it, exactly as a real `/clear` would leave behind.
        live_session: a.rotated.then(|| scenario_rotated_uuid(n)),
        metered_at: 0,
        // Not seeded: the sandbox exercises the real birth-touch inheritance
        // path (touch_in/opener_buckets) rather than a scenario-typed value.
        buckets: Default::default(),
        model: a.model.map(String::from),
        provider: a.provider.map(String::from),
        effort: a.effort.map(String::from),
        pr_number: a.pr_number,
        // A seeded number must read FRESH, or the first background sync for
        // this branch replaces it with the nothing a sandbox repo really has
        // (`pr_is_stale`: a zero check time is stale by definition).
        pr_checked: if a.pr_number.is_some() { now } else { 0 },
        pr_branch: if a.pr_number.is_some() {
            branch.clone()
        } else {
            String::new()
        },
        wants: a.wants.map(String::from),
        subagents: a.subagents,
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
    // enter_sandbox): claude runs as the real user; transcripts go to the
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
        let n = i as u32 + 1;
        let uuid = scenario_uuid(n);
        let repo = root.join("repos").join(repo_dir_name(name, a));
        std::fs::create_dir_all(&repo)?;
        // -b main: pin the branch — else init.defaultBranch (maybe `master`)
        // would disagree with the store row's hardcoded `branch: "main"`.
        run_in(&repo, "git", &["init", "-q", "-b", "main"])?;
        // HERMETIC, like the ensure_worktree test fixture below: a scenario
        // repo is a throwaway fixture, and the seed commit must not read the
        // operator's ~/.gitconfig — the maintainer signs every commit
        // globally, so an unconfigured seed blocks `just sandbox` on a
        // 1Password fingerprint per repo (2026-08-26, staging the #231 drive).
        for (k, v) in [
            ("user.email", "seed@clave.invalid"),
            ("user.name", "clave scenario"),
            ("commit.gpgsign", "false"),
        ] {
            run_in(&repo, "git", &["config", k, v])?;
        }
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
        seed_transcript(&cwd, &cwd_str, &uuid, a.slug)?;
        // #182: a `rotated: true` agent gets a SECOND real transcript at the
        // SAME cwd, guarded by the same idempotence check — the faithful
        // shape of a `/clear`'d pane, which writes a new jsonl next to the
        // old one rather than replacing it. `agent_record` points
        // `live_session` at this mint, and `resume_target`/`verified_site`
        // are what then prefer it over the frozen original.
        if a.rotated {
            seed_transcript(&cwd, &cwd_str, &scenario_rotated_uuid(n), a.slug)?;
        }
        crate::store::with_store_mut(&paths, |s| {
            s.agents.insert(
                uuid.clone(),
                agent_record(name, a, n, &uuid, &cwd_str, &repo.to_string_lossy(), now),
            );
            s.seq += 1;
        })?;
        if a.delete_cwd_after {
            std::fs::remove_dir_all(&cwd)?; // the §6.3 staleness fixture
        }
    }
    crate::evlog::log_event("dev", &format!("scenario {name} seeded"));
    println!(
        "\nScenario `{name}` ready. Launch (the MAINTAINER's step, in a NON-zellij terminal):\n"
    );
    if let Some(origin) = &sb.origin {
        println!("  cd {}", origin.display());
    }
    println!("  just launch");
    // The `cd` is the only part a reader has to get right: the instance is
    // keyed off the working directory, so launching from elsewhere stages one
    // sandbox and launches another. Everything else is derived by `dev launch`
    // itself - an env prefix printed here would omit the PATH shim and so
    // silently drive the sandbox with the stable install (#43/#44).
    println!("\nWhen done: `clave dev reset` (prints the kill command first).");
    Ok(())
}

pub fn run_status() -> Result<()> {
    let sb = crate::sandbox::Sandbox::resolve()?;
    enter_sandbox(&sb);
    let store = crate::store::read_store(&crate::store::store_paths()?)?;
    // Discovered zellij (2026-07-22): both reads below swallow failure with
    // unwrap_or_default, so an off-PATH zellij would report "no live session"
    // rather than erroring — and AGENTS.md tells agents to gate the session
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

/// One real `claude -p --session-id <uuid>` seed at `cwd`, resume-or-create
/// (`seed_needed`) exactly as `run_scenario`'s original single-mint loop did.
/// Factored out for #182: a `rotated: true` agent seeds this TWICE — once
/// for its minted uuid, once for `scenario_rotated_uuid` — and the two calls
/// must stay byte-identical in behaviour or the rotated row's second
/// transcript would be seeded by a subtly different path than the first.
fn seed_transcript(cwd: &Path, cwd_str: &str, uuid: &str, slug: &str) -> Result<()> {
    if seed_needed(&crate::env::claude_config_dir()?, cwd_str, uuid) {
        println!("seeding {uuid} ({slug})…");
        // Discovered claude (coderabbit CLI, 2026-07-22): a contributor
        // whose claude lives off PATH (nvm, ~/.claude/local) could not
        // seed a scenario at all. Unlike dev.rs's zellij calls — session
        // lifecycle the human drives — this is a real exec clave owns.
        // The ONE place clave itself runs a claude inside another's pane:
        // `clave dev` is usually driven from an agent's shell, so this child
        // would inherit that pane's `CLAVE_AGENT_UUID`. Hooks trust that var
        // outright (the pid gate is gone — one pane, one Claude), so scrub it
        // here rather than gate it there.
        let st = Command::new(crate::discover::tool_path(crate::discover::ToolId::Claude))
            .env_remove(clave_types::AGENT_UUID_ENV)
            .current_dir(cwd)
            .args(["-p", "--session-id", uuid, "Reply with exactly: ok"])
            .status()
            .context("running claude -p (is claude discoverable?)")?;
        anyhow::ensure!(st.success(), "claude -p seeding failed for {uuid}");
    } else {
        println!("{uuid} ({slug}) already seeded — reusing its transcript");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_record_carries_the_rotated_mint_only_for_a_rotated_agent() {
        // #182: `live_session` is how `resume_target`/`verified_site` learn
        // a row's real conversation moved on from its minted uuid — a
        // rotated scenario agent must set it to the SECOND mint, a plain one
        // must stay `None` (agreement with the minted uuid is never stored,
        // per spawn.rs's `resume_target` doc comment).
        let rotated = ScenarioAgent {
            slug: "rotated",
            rotated: true,
            ..ScenarioAgent::DEFAULT
        };
        let rec = agent_record(
            "qa-fleet",
            &rotated,
            6,
            &scenario_uuid(6),
            "/cwd",
            "/repo",
            1_000,
        );
        assert_eq!(
            rec.live_session.as_deref(),
            Some(scenario_rotated_uuid(6).as_str())
        );

        let plain = ScenarioAgent {
            slug: "plain",
            ..ScenarioAgent::DEFAULT
        };
        let rec = agent_record(
            "qa-fleet",
            &plain,
            1,
            &scenario_uuid(1),
            "/cwd",
            "/repo",
            1_000,
        );
        assert_eq!(rec.live_session, None);
    }

    #[test]
    fn a_seeded_pull_request_number_reads_fresh_and_a_missing_one_reads_never() {
        // #232: a seeded row is dormant and fires no hooks, so the identity
        // cells only appear on screen if they are seeded. The PR number needs
        // one thing more than the others — a check TIME and the branch it was
        // checked for. `pr_is_stale` calls a zero check time stale, so an
        // unstamped number is replaced by the first background sync with the
        // nothing a sandbox repo really has.
        let with_pr = ScenarioAgent {
            slug: "auth",
            branch: Some("key-rotation"),
            provider: Some("claude"),
            model: Some("fable"),
            effort: Some("mx"),
            pr_number: Some(184),
            wants: Some("Bash (cargo publish)"),
            subagents: true,
            ..ScenarioAgent::DEFAULT
        };
        let rec = agent_record(
            "showcase",
            &with_pr,
            1,
            &scenario_uuid(1),
            "/cwd",
            "/repo",
            9_000,
        );
        assert_eq!(rec.provider.as_deref(), Some("claude"));
        assert_eq!(rec.model.as_deref(), Some("fable"));
        assert_eq!(rec.effort.as_deref(), Some("mx"));
        assert_eq!(rec.wants.as_deref(), Some("Bash (cargo publish)"));
        assert!(rec.subagents);
        assert_eq!(rec.pr_number, Some(184));
        assert_eq!(rec.pr_checked, 9_000);
        assert_eq!(rec.pr_branch, "key-rotation");
        assert!(!crate::pr::pr_is_stale(&rec, 9_000));

        // No number seeded: the row must read "never looked up", which is the
        // state that invites the real lookup rather than blocking it.
        let without = ScenarioAgent {
            slug: "spawn",
            ..ScenarioAgent::DEFAULT
        };
        let rec = agent_record(
            "showcase",
            &without,
            2,
            &scenario_uuid(2),
            "/cwd",
            "/repo",
            9_000,
        );
        assert_eq!(rec.pr_number, None);
        assert_eq!(rec.pr_checked, 0);
        assert!(rec.pr_branch.is_empty());
    }

    #[test]
    fn the_showcase_fleet_paints_every_cell_of_the_card() {
        // The capture is the sample image AGENTS.md sends every agent to read,
        // so a cell missing from the fleet is a cell missing from the only
        // picture of clave most readers ever see. Blank cells are part of the
        // vocabulary and are asserted as deliberately present.
        let sc = SCENARIOS.iter().find(|s| s.name == "showcase").unwrap();
        let a = sc.agents;
        for status in [
            clave_types::Status::Working,
            clave_types::Status::Idle,
            clave_types::Status::NeedsYou,
            clave_types::Status::Done,
            clave_types::Status::Failed,
        ] {
            assert!(
                a.iter().any(|x| x.status == status),
                "no {status:?} row in the capture fleet"
            );
        }
        assert!(a.iter().any(|x| x.provider == Some("openai")));
        assert!(a.iter().any(|x| x.provider == Some("claude")));
        assert!(a.iter().any(|x| x.worktree));
        assert!(a.iter().any(|x| x.branch.is_some()));
        assert!(a.iter().any(|x| !x.worktree && x.branch.is_none()));
        assert!(a.iter().any(|x| x.delete_cwd_after)); // the stale mark
        assert!(a.iter().any(|x| x.subagents));
        assert!(a.iter().any(|x| x.pr_number.is_some()));
        assert!(a.iter().any(|x| x.pr_number.is_none()));
        assert!(a.iter().any(|x| x.title.is_none())); // the blank chip
        assert!(a.iter().any(|x| x.effort.is_none()));
        assert!(a.iter().any(|x| x.context_tokens.is_none()));
        // Only a flagged row says what it is blocked on (design lock §4.7).
        for x in a.iter().filter(|x| x.wants.is_some()) {
            assert_eq!(
                x.status,
                clave_types::Status::NeedsYou,
                "{}: only a flagged row may say what it wants",
                x.slug
            );
        }
        assert!(a.iter().any(|x| x.wants.is_some()));
        // More than one repo, or the per-repo colouring shows nothing.
        let repos: std::collections::BTreeSet<_> = a.iter().filter_map(|x| x.repo).collect();
        assert!(repos.len() >= 4, "only {} repos in the fleet", repos.len());
    }

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
        // Names map 1:1 to the C8 validation steps, plus ux-gate1 (the
        // visual-design decision fixture, #85 follow-up), tall (the #148
        // viewport overflow fixture), qa-fleet (the drive's fleet) and
        // showcase (the README capture fleet). Exact list on purpose (task
        // instruction): a `contains` would let a scenario go missing silently.
        let names: Vec<&str> = SCENARIOS.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "c8-cold-start",
                "c8-worktree",
                "c8-stale",
                "ux-gate1",
                "tall",
                "qa-fleet",
                "showcase"
            ]
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
    fn tall_dormant_rows_span_the_whole_battery_ramp() {
        // #173: found in v0.1.3 live validation — `tall`'s dormant rows seeded
        // no token count at all, so every battery cell in the scenario the
        // viewport is driven with rendered blank. Seeded rows never run a
        // hook, so the counts here are the only ones that scenario will ever
        // have. Bucketed against the DEFAULT smart zone rather than
        // `hook::smart_zone()`: the env var is process-global and this must
        // not depend on the machine running the tests.
        let sc = SCENARIOS.iter().find(|s| s.name == "tall").unwrap();
        let dormant: Vec<&ScenarioAgent> = sc
            .agents
            .iter()
            .filter(|a| a.slug.starts_with('d'))
            .collect();
        assert_eq!(dormant.len(), 30);

        // Every dormant row carries a count — the bug was the `None`s.
        assert!(
            dormant.iter().all(|a| a.context_tokens.is_some()),
            "a dormant row with no context_tokens renders a blank battery"
        );

        let zone = clave_types::DEFAULT_SMART_ZONE_TOKENS;
        let levels: std::collections::BTreeSet<u8> = dormant
            .iter()
            .map(|a| crate::hook::battery_level(a.context_tokens.unwrap(), zone))
            .collect();

        // Full to empty, every fill step on screen at once.
        assert_eq!(
            levels.len(),
            usize::from(clave_types::BATTERY_LEVELS),
            "want every battery level 0..={}, got {levels:?}",
            clave_types::BATTERY_LEVELS - 1
        );

        // The tail sits PAST the zone, so the clamp (and #105's red token
        // text) is exercised too, not just the ramp below it.
        assert!(
            dormant.iter().any(|a| a.context_tokens.unwrap() > zone),
            "no dormant row exceeds the smart zone, so the clamp never shows"
        );
    }

    #[test]
    fn qa_fleet_carries_the_182_dormant_lineup() {
        // #182: the QA drive's fleet — six dormant rows (the eager, live-style
        // row is whichever the human's launch line names, per the design doc,
        // not seeded here). One worktree, one stale (worktree +
        // delete_cwd_after — the pinned caveat: sharing a repo with a live
        // agent, a plain checkout's `remove_dir_all` would take that agent's
        // dir with it), one rotated, three plain.
        let sc = SCENARIOS.iter().find(|s| s.name == "qa-fleet").unwrap();
        assert_eq!(sc.agents.len(), 6);

        let worktrees: Vec<&ScenarioAgent> = sc.agents.iter().filter(|a| a.worktree).collect();
        assert_eq!(worktrees.len(), 2, "one worktree + one stale worktree");

        let stale: Vec<&ScenarioAgent> = sc.agents.iter().filter(|a| a.delete_cwd_after).collect();
        assert_eq!(stale.len(), 1);
        assert!(
            stale[0].worktree,
            "the stale agent must be a worktree (dev.rs's pinned caveat)"
        );

        let rotated: Vec<&ScenarioAgent> = sc.agents.iter().filter(|a| a.rotated).collect();
        assert_eq!(rotated.len(), 1);

        let plain = sc
            .agents
            .iter()
            .filter(|a| !a.worktree && !a.rotated)
            .count();
        assert_eq!(plain, 3);

        // Staggered recency: every ago_secs distinct.
        let ago: std::collections::BTreeSet<u64> = sc.agents.iter().map(|a| a.ago_secs).collect();
        assert_eq!(ago.len(), sc.agents.len(), "ago_secs must all differ");

        // The stale agent shares a repo with the plain worktree agent —
        // exactly the shape the pinned caveat guards (a shared repo's
        // remove_dir_all must remove only the stale agent's OWN worktree
        // dir). Every other agent gets its own distinct repo.
        let shared: std::collections::BTreeSet<&str> =
            worktrees.iter().filter_map(|a| a.repo).collect();
        assert_eq!(shared.len(), 1, "the two worktree agents share one repo");
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
            COLLAPSED_DESIGN_COLS, DESIGN_COLS, Provenance, RowContent, RowHeight, RowStatus,
            Theme, Widths, display_cells, render_rows, strip_sgr,
        };

        let sc = SCENARIOS.iter().find(|s| s.name == "ux-gate1").unwrap();
        let now = 2_000_000_000u64;
        let mut store = crate::store::Store::default();
        for (i, a) in sc.agents.iter().enumerate() {
            let n = i as u32 + 1;
            let uuid = scenario_uuid(n);
            let repo_root = format!("/sandbox/repos/{}", repo_dir_name("ux-gate1", a));
            let cwd = if a.worktree {
                format!("{repo_root}/.claude-worktrees/{}", uuid_tag(&uuid))
            } else {
                repo_root.clone()
            };
            let mut record = agent_record("ux-gate1", a, n, &uuid, &cwd, &repo_root, now);
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
                    floating_visible: false,
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
        // Four geometries, not two (#232): the legacy single-line pair, and
        // the card pair at ITS OWN two targets. The card is the shipping
        // default, so leaving it out of the one test that drives the real
        // store→model→renderer pipeline would leave the design the maintainer
        // actually looks at unexercised against scenario data.
        for (row_height, widths, cols) in [
            (RowHeight::Single, Widths::EXPANDED, DESIGN_COLS),
            (RowHeight::Single, Widths::COLLAPSED, COLLAPSED_DESIGN_COLS),
            (
                RowHeight::Double,
                Widths::EXPANDED,
                RowHeight::Double.target_cols(false),
            ),
            (
                RowHeight::Double,
                Widths::COLLAPSED,
                RowHeight::Double.target_cols(true),
            ),
        ] {
            // One row is `lines_per_row` lines, so every zip below walks the
            // rows REPEATED that many times — the single-line arm is the
            // repeat-once case of the same expression, not a separate path.
            let per_row = row_height.lines_per_row();
            let rows_by_line = || rows.iter().flat_map(|r| std::iter::repeat_n(r, per_row));
            // Design-lock invariant, proven rather than asserted in prose:
            // every row is exactly the profile's target in display cells
            // (bar-preview.rs does the same measurement) — INCLUDING the
            // selected row, whose caps and full-width background are the
            // easiest thing to render one cell wide.
            let lines = render_rows(
                &rows,
                cols,
                rows.len() * per_row,
                widths,
                &Theme::default(),
                row_height,
                0,
            );
            assert_eq!(lines.len(), rows.len() * per_row, "line budget at {cols}");
            for (line, row) in lines.iter().zip(rows_by_line()) {
                let width = display_cells(&strip_sgr(line));
                assert_eq!(width, cols, "row is {width} cells at {cols}: {row:?}");
            }
            // The selected row is the only one with the waveBlue2 background,
            // and every other row is faded 25% toward it (lock §6) — the two
            // halves of recession, checked against scenario data rather than a
            // fixture.
            let (sel, rest): (Vec<_>, Vec<_>) = lines
                .iter()
                .zip(rows_by_line())
                .partition(|(_, r)| r.selected);
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
            for (faded, plain) in lines.iter().zip(render_rows(
                &unfaded,
                cols,
                unfaded.len() * per_row,
                widths,
                &Theme::default(),
                row_height,
                0,
            )) {
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
    fn scenario_rotated_uuid_is_deterministic_and_never_collides_with_the_first_mint() {
        // #182: the rotated mint shares the c85c prefix (so `is_scenario_jsonl`
        // sweeps it on `dev reset` same as the first) but adds 50 to the
        // ordinal, clearing every `scenario_uuid` any scenario mints today
        // (largest is `tall` at 34 agents) and staying under u32::MAX with
        // room to spare.
        let u = scenario_rotated_uuid(1);
        assert_eq!(u, "00000000-0000-4000-8000-c85c00000051");
        assert!(uuid::Uuid::parse_str(&u).is_ok());
        assert_eq!(scenario_rotated_uuid(1), u); // deterministic
        assert_ne!(scenario_rotated_uuid(1), scenario_rotated_uuid(2));
        // Non-collision against every scenario_uuid a scenario under 50
        // agents could mint.
        let firsts: std::collections::BTreeSet<String> = (1..=50).map(scenario_uuid).collect();
        for n in 1..=50 {
            let rotated = scenario_rotated_uuid(n);
            assert!(
                !firsts.contains(&rotated),
                "rotated mint {rotated} collides with a first mint"
            );
        }
        // uuid_tag (last-8-chars) stays unique too — the same worktree-branch
        // hazard `uuid_tag`'s own doc comment records.
        assert_ne!(
            uuid_tag(&scenario_uuid(1)),
            uuid_tag(&scenario_rotated_uuid(1))
        );
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

    /// The refusal has to TEACH, not just decline: whoever hit it is either a
    /// human in the wrong terminal or an agent that should have handed the
    /// command over, and both need the next action spelled out.
    #[test]
    fn the_nested_launch_refusal_names_the_command_to_hand_over() {
        let home = std::path::Path::new("/home/u");
        let sb = crate::sandbox::Sandbox::new(
            home,
            Some("triple-card".to_string()),
            Some(std::path::PathBuf::from(
                "/home/u/code/clave/.claude/worktrees/triple-card",
            )),
        );
        let msg = nested_launch_refusal(&sb);
        assert!(msg.contains("clave-test-triple-card"), "{msg}");
        assert!(msg.contains("ZELLIJ is set"), "{msg}");
        // The cd is load-bearing — the instance is cwd-keyed, so a launch from
        // the wrong directory silently targets a different sandbox.
        assert!(
            msg.contains("cd /home/u/code/clave/.claude/worktrees/triple-card"),
            "{msg}"
        );
        assert!(msg.contains("just launch"), "{msg}");
        // And it must point at the loop, so a zero-context reader does not
        // have to go find out what staging is called.
        assert!(msg.contains("just qa"), "{msg}");
    }

    /// The main checkout has no origin marker (it is never a reap candidate),
    /// so the refusal must still be useful without one.
    #[test]
    fn the_nested_launch_refusal_survives_an_instance_with_no_origin() {
        let sb = crate::sandbox::Sandbox::new(std::path::Path::new("/home/u"), None, None);
        let msg = nested_launch_refusal(&sb);
        assert!(msg.contains("just launch"), "{msg}");
        assert!(!msg.contains("cd \n"), "no empty cd line: {msg}");
    }

    /// An explicitly set variable wins, so a caller that names an instance
    /// keeps it. The rival is the old unconditional `set_var`, under which
    /// the middle case would also be derived.
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
