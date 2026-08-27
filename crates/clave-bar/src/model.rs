//! Pure display/model logic for clave-bar — deliberately NO zellij-tile
//! imports so it compiles and unit-tests on the host. main.rs adapts zellij
//! events into these plain types and executes the returned `Effect`s.
//!
//! The three separated concerns of spec §6.6:
//!   row SET        = zellij's tabs (apply_tabs)
//!   row ORDER      = interaction recency (logical clock, this module)
//!   row DECORATION = clave's pushed snapshots (apply_snapshot)

use std::collections::{BTreeMap, BTreeSet};

// The two width targets live in `clave-types` (S8 §3.3): the renderer draws
// against the same numbers the layout sizes the pane to, and `clave`'s KDL
// generators size the newborn pane from them — one definition, three artifacts.
use clave_types::{Agent, AgentSnapshot, OrderMode, RowHeight, Status};

use crate::render::{
    PALETTE, Provenance, Row, RowContent, RowStatus, TermStatus, Widths, viewport_top,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMeta {
    /// Zellij's STABLE tab id (survives reorders) — the recency/rename key.
    pub tab_id: usize,
    /// Current 0-based position — the PaneManifest join key (it's keyed by
    /// position, not id) and the bottom-of-list tiebreak.
    pub position: usize,
    pub name: String,
    pub active: bool,
    /// zellij's `are_floating_panes_visible` for this tab — whether the
    /// floating set is currently shown. One input of the Alt+f decision (#207).
    pub floating_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMeta {
    pub tab_position: usize,
    pub pane_id: u32,
    pub is_plugin: bool,
    pub is_focused: bool,
    /// Floating panes ride the same manifest as tiled ones, flagged. Their
    /// presence on the active tab is what separates Alt+f's show from its
    /// spawn (#207) — respawning over a hidden shell is the stacking bug.
    pub is_floating: bool,
    /// The launch command of a COMMAND pane (`zellij run`, layout `command`
    /// nodes) — static, never the shell's current foreground. `None` for
    /// ordinary shell panes, which is the case `TermFacts` exists for (#206).
    pub terminal_command: Option<String>,
    /// A finished command pane, and how it finished — the only place an exit
    /// code exists (an interactive shell never exits while its tab lives), so
    /// the only source of a terminal row's Done/Failed (#206).
    pub exited: bool,
    pub exit_status: Option<i32>,
}

/// What the bar has learned about a terminal pane beyond the manifest (#206):
/// the cwd and foreground command are OS truths zellij only surrenders on
/// request (`get_pane_cwd` / `get_pane_running_command`) or by subscription
/// (`CwdChanged` / `CommandChanged`), so they live in a side map keyed by pane
/// id rather than on `PaneMeta`, whose rows are rebuilt whole every manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermFacts {
    /// The pane process's current working directory, as the OS reports it.
    pub cwd: Option<String>,
    /// The most recent foreground command that was not the shell itself —
    /// lingering after it finishes, which is what makes the summary read as
    /// "most recently run" at an idle prompt. Precisely: the most recent
    /// command ZELLIJ NOTICED. Its foreground detection samples roughly once
    /// a second, so a sub-second command (`ls`, `la`) starts and exits
    /// between samples, emits no `CommandChanged`, and cannot be recovered
    /// by any probe — the process is gone before anyone could ask the OS.
    /// Ratified trade (Ollie, live 2026-08-18): commands that run long
    /// enough to matter register; instant ones never displace the last one
    /// that did. The alternatives — scrollback scraping or shell
    /// integration — are scope cliffs refused for v0.2.0.
    pub last_cmd: Option<String>,
    /// Whether that command is in flight RIGHT NOW — the running/idle axis of
    /// the status glyph. Derived: the reported foreground is running unless it
    /// is the shell.
    pub running: bool,
}

/// One OS answer about one pane, on its way into `TermFacts` (#206). Both
/// fields are "answered or not asked" — see `apply_pane_facts`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneProbe {
    pub pane_id: u32,
    pub cwd: Option<String>,
    /// The foreground argv exactly as the OS reported it; classification
    /// (shell vs command) is `apply_pane_facts`'s job, not the caller's.
    pub foreground: Option<Vec<String>>,
}

/// Foreground argvs that are the SHELL ITSELF, i.e. an idle prompt. Names,
/// because that is all the OS report offers generically: an exotic shell not
/// listed reads as an eternally running command, which is visible and
/// harmless; the empty argv reads as idle.
const SHELLS: [&str; 9] = [
    "zsh", "bash", "fish", "sh", "dash", "nu", "ksh", "tcsh", "pwsh",
];

/// Three-state (lock §5.1), and a main checkout renders NOTHING — that is the
/// researched choice, and it is what makes the two marked states mean
/// something. NO branch is the case the design did not name: an agent outside
/// a repo has none, and painting the branch glyph for it would assert a
/// provenance nobody has — so it takes the blank.
///
/// `"-"` is the value that matters, NOT the empty string: the host writes
/// `"-"` when `git rev-parse --abbrev-ref HEAD` fails (`add::run_add`'s
/// fallback) and `record_branch` returns `"-"` for a detached-worktree
/// resume. Nothing in the host ever writes an empty branch — every
/// `branch: String::new()` in the tree is a test builder, so the empty clause
/// is kept only to keep those honest. Verified against the writer, not
/// against a fixture.
///
/// WHICH branch is the default is the repo's answer, never ours (#86). This
/// used to test `main`/`master` as if they were exhaustive, so a `trunk`-,
/// `develop`- or `dev`-default repository had its ORDINARY checkout marked as
/// a branch — the one row §5.1 requires to be blank, mislabelled on naming
/// convention alone. `default_branch` is resolved by the host
/// (`add::resolve_default_branch`) and rides the snapshot. The name test
/// survives only as the fallback for `None`, which is what an old store row
/// and an undiscoverable default both deserialize to: no snapshot gets a
/// WORSE answer than it got before the field existed.
///
/// A free function since #206: terminal rows borrow a matched agent's
/// provenance, and a borrowed glyph must be computed by the same rules as the
/// row it was borrowed from.
fn provenance_of(a: &Agent) -> Provenance {
    if a.worktree.is_some() {
        Provenance::Worktree
    } else if is_default_checkout(a) {
        Provenance::Main
    } else {
        Provenance::Branch
    }
}

/// True when `a.branch` reads as an ordinary checkout of the repo's default
/// branch: empty (no repo), the host's git-failure sentinel `"-"`, or a name
/// match against the resolved default (falling back to the bare main/master
/// heuristic when the host couldn't resolve one). This is the exact name-only
/// half of [`provenance_of`]'s `Main` case (the worktree branch is decided
/// separately, above it) — shared rather than re-derived (#232) because the
/// branch cell blanks on precisely this predicate too.
fn is_default_checkout(a: &Agent) -> bool {
    a.branch.is_empty()
        || a.branch == "-"
        || match a.default_branch.as_deref() {
            Some(default) => a.branch == default,
            None => a.branch == "main" || a.branch == "master",
        }
}

/// m → h → d → w, each unit taking over at 1.0 of itself; sub-minute is "0m".
/// `then == 0` is "never" (no interaction on record — the store never mints
/// that value for a real one), which renders blank rather than "now": the bar
/// never invents a measurement (#232).
pub(crate) fn elapsed_label(now: u64, then: u64) -> Option<String> {
    if then == 0 {
        return None;
    }
    let s = now.saturating_sub(then);
    Some(match s {
        0..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        86_400..=604_799 => format!("{}d", s / 86_400),
        _ => format!("{}w", s / 604_800),
    })
}

/// The summary-cell text for a foreground argv, or `None` when it is the
/// shell at its prompt. argv[0] arrives as a path (`/bin/zsh`) or a login
/// shell's dashed name (`-zsh`); both normalise before the shell test, and
/// the display keeps the basename — `cargo test`, not
/// `/Users/x/.cargo/bin/cargo test`.
fn command_display(argv: &[String]) -> Option<String> {
    let head = argv.first()?;
    let bin = basename(head).trim_start_matches('-');
    if bin.is_empty() || SHELLS.contains(&bin) {
        return None;
    }
    let mut out = bin.to_string();
    for a in &argv[1..] {
        out.push(' ');
        out.push_str(a);
    }
    Some(out)
}

/// Side effects for main.rs to execute — kept as data so tests assert them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// rename_tab_with_id(tab_id, name) — write clave's label on the real tab.
    RenameTab { tab_id: usize, name: String },
    /// focus_pane_with_id(Terminal(pane_id)) — S2-proven; uuid jumps and any
    /// live pick on a tab with a registered agent pane. The pane id is
    /// broadcast truth: position-immune (a starved executor's positions can
    /// predate a close — the QA-drive nav wedge) and same-target across
    /// duplicate instances.
    FocusPane { pane_id: u32 },
    /// switch_tab_to(position + 1) — clicks and the nav fallback for tabs with
    /// no registered pane. All instances compute the same target from
    /// replicated state, so duplicates are idempotent.
    SwitchTab { position: usize },
    /// run_command zellij pipe clave-visited — converge the other instances
    /// after a single-instance jump (mouse click).
    AnnounceVisit { tab_id: usize },
    /// run_command zellij pipe clave-visited — SAME beacon as AnnounceVisit,
    /// but for the #23 stranded-beacon re-anchor after a tab close AND the
    /// Alt+o organic one-shot. Only ONE instance may send either: toggle
    /// bursts deliver the fresh tab set to ALL instances (doc:371-394), so an
    /// ungated announce from either trigger is a per-instance beacon war
    /// (round-13 EMFILE class for stranded; N×~1s router stalls per Alt+o for
    /// organic, #128 2026-08-02).
    ///
    /// Since #162 the ELECTION happens at emit time, in the TWO emitters this
    /// variant has: `apply_tabs` (under `elects_presumed()`) and `apply_panes`
    /// (the debt payment, under the stricter `elects_confirmed()`). Either way
    /// it is produced only by an instance that has already elected itself
    /// to send it, so the beacon and the trigger it consumes move together and
    /// an un-elected instance keeps its trigger for the next frame. It
    /// stays a distinct variant because that provenance is the whole contract —
    /// AnnounceVisit is the UNGATED birth/click/nav beacon, which must be able
    /// to fire before the first PaneUpdate can satisfy any gate.
    ReanchorVisit { tab_id: usize },
    /// run_command(["clave","focus",uuid]) — persist the unread clear.
    MarkRead { uuid: String },
    /// run_command(["clave","bind",uuid,tab_id]) — report the uuid→tab join
    /// to the STORE (§6.6 Design B), fired by the agent tab's own bar.
    Bind { uuid: String, tab_id: usize },
    /// run_command(["clave","prune-tabs", stale_ids…]) — drop store binds and
    /// tab_order entries for CLOSED tabs (#6/F3). Carries the OBSERVED-STALE
    /// ids (bound-or-ordered ids ABSENT from the delivered live set), NOT the
    /// live set — removing specific dead ids is idempotent and commutes, so
    /// two out-of-order prunes can't clobber a tab neither observed die (the
    /// full-live-set "retain-only" payload could unbind a tab created after the
    /// prune was computed). Executor-gated in main.rs (keeps duplicate prunes
    /// to the active bar). Zellij reuses tab_ids (screen.rs:1617), so this is
    /// correctness, not just hygiene.
    PruneTabs { stale_ids: Vec<usize> },
    /// `next_swap_layout()` — switch this tab to the OTHER declared bar
    /// geometry (#181).
    ///
    /// The two widths are `swap_tiled_layout` nodes in the generated layout, so
    /// zellij resolves the percent against the real window and applies it in one
    /// relayout. Nothing here computes a column count, waits for a repaint, or
    /// can be starved of one — which is the entire content of the change: the
    /// old pair of effects asked zellij to step the pane by an amount it would
    /// not disclose, and the bar watched its own renders to find out what had
    /// happened. That loop is what ran the sidebar to 141 columns and to 11.
    SwapWidth,
    /// set_timeout(PEEK_SINK_SECS) + pending_peeks bump — a dormant-row nav
    /// landing on a collapsed bar peeks like live nav does (no visited pipe
    /// exists for it, so the model asks explicitly).
    ArmPeek,
    /// hide_floating_panes(None) — Alt+f while the active tab's floating set
    /// is visible (#207). `None` targets the active tab server-side, and the
    /// emitter is the beacon-named instance, whose tab that is.
    ShellHide,
    /// show_floating_panes(None) — Alt+f while the active tab HAS floating
    /// panes but they are hidden. Never a spawn: respawning over a hidden
    /// shell is #207's every-press-stacks bug.
    ShellShow,
    /// open_terminal_floating(cwd, shell geometry) — Alt+f on a tab with no
    /// floating pane at all. The ONLY non-idempotent arm of the trio, which is
    /// why `shell_toggle` emits solely from the beacon-named instance: N
    /// emitters would be N shells. Spawning by COORDINATES is load-bearing —
    /// it is the one geometry path that floors x at the viewport's left edge
    /// (pane_size.rs `adjust_coordinates`); a swap_floating_layout resolves
    /// the same percents from absolute column 0 and puts the shell over the
    /// bar (#207 live probe, 2026-08-17).
    /// `cwd` is the active tab's location (#215): store truth for an agent
    /// tab, the speaker pane's OS-reported cwd for a terminal tab, `None`
    /// (→ the bar's `initial_cwd`) when neither is known. Decided here so
    /// tests reach it; main.rs only translates `None`.
    ShellSpawn { cwd: Option<String> },
    /// run_command(["clave","open",uuid]) — §6.3. Fired ONLY by the Alt+Enter
    /// commit (#100 dwell-commit: selection and launch are separate acts);
    /// the model has already marked the uuid in-flight (↻).
    OpenAgent { uuid: String },
    /// run_command(["clave","touch",tab_id]) — the once-EVER birth stamp for a
    /// tab the store's tab order has never seen. Was an inline `run_command` in
    /// the adapter, which put it out of reach of every test (`main.rs` is
    /// `test = false`) and gave it no retry trigger: the block lived only in
    /// the TabUpdate arm, and a close delivers exactly ONE TabUpdate to the
    /// newly-active instance — the one where the manifest is stale and the
    /// gate is false. As an effect it is emitted by `identity_effects`, which
    /// every frame arm re-enters, so the PaneUpdate that resolves the
    /// incoherence is the retry (#55, RC-A/RC-B).
    Touch { tab_id: usize },
    /// run_command(["clave","collapse",bool]) — issue #5 durability. Emitted
    /// by toggle() (the write we owe the store) and by apply_snapshot's
    /// one-shot re-assert (out-of-order write recovery). Bookkeeping runs on
    /// every instance; main.rs gates EXECUTION to the active one, same as
    /// MarkRead/Bind (one writer, round 11).
    PersistCollapse { collapsed: bool },
    /// #178: the bind leg stopped emitting, and left no trace doing it. Every
    /// exit in `bind_effects` that produces nothing is silent by design — the
    /// frames disagree and the next frame is the retry, or a row's pane simply
    /// is not ours — which is correct when a next frame arrives and a permanent
    /// stall when one never does. The field failure (the first two wakes bind,
    /// every later one does not) writes nothing to the store, nothing to the
    /// event log, and nothing to the zellij log, so there is currently no way
    /// to tell those two apart from outside.
    ///
    /// Breadcrumb discipline: state-CHANGE triggered, so
    /// a steady state costs one line and not one per frame; printed by main.rs
    /// rather than shelled out to the CLI, because a subprocess on a path we
    /// suspect of starvation would be the same mistake in a smaller font.
    BindStall {
        state: BindStallState,
        /// Unbound rows whose pane id we know but cannot place in any tab —
        /// the starvation signature (`PaneKnownButAbsent`); zero otherwise.
        stranded: usize,
        /// `None` when the frames cannot resolve this instance's own tab —
        /// which is itself one of the reported states, so it must not be a
        /// precondition for reporting.
        own_tab: Option<usize>,
    },
}

/// Why the bind leg is (or is no longer) silent. See `Effect::BindStall`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindStallState {
    /// The tab frame and the pane frame disagree, so `bind_effects` refuses
    /// the whole pass and waits for a frame that resolves it.
    FramesIncoherent,
    /// Our own tab is absent from the tab frame: no row can match our
    /// position, so every row looks like someone else's.
    OwnPositionUnknown,
    /// The prime suspect. A `clave-register` broadcast told us a row's pane
    /// id — broadcasts reach every instance — but our pane frame does not
    /// contain that pane, and pane frames reach only the ACTIVE tab
    /// (FOOTGUNS, "TabUpdate reaches only the ACTIVE tab's plugin instance …
    /// PaneUpdate likewise"). We therefore cannot compute the pane's tab, so
    /// we emit no bind and spend no budget: silence that lasts exactly as
    /// long as the starvation does.
    PaneKnownButAbsent,
    /// None of the above still holds — the leg is live again.
    Cleared,
}

/// The peek sink timer. Event::Timer carries TWO kinds again since the
/// 2026-08-17 width-loop fix (the width cooldown below rejoined it; #100 had
/// deleted the 0.4s dormant dwell and with it the classification) — main.rs
/// tells them apart by the elapsed seconds the event echoes, the same
/// duration-cutoff scheme the dwell era used.
pub const PEEK_SINK_SECS: f64 = 0.9;

/// How long a switch ask deafens the width machine (`main.rs` arms it via
/// `set_timeout` on every `Effect::SwapWidth`). A swap's repaints arrive
/// QUEUED and stale — the QA-drive trace (2026-08-17) showed a burst of
/// pre-swap widths landing within ~10ms of the ask — so a machine that
/// judges every paint spends its whole budget on echoes of its own last
/// move. One ask buys silence until this clock expires; the expiry judges
/// the LATEST painted width once. 150ms is an order of magnitude past the
/// measured burst and well under [`PEEK_SINK_SECS`], which keeps the two
/// timer kinds separable by elapsed time.
pub const WIDTH_COOLDOWN_SECS: f64 = 0.15;

/// The width machine's walk budget (`BarModel::walk_spent`): the most switch
/// asks one mode-intent may spend without ever landing its target width.
/// Three is derived, not tuned: the swap cycle is three positions long
/// (FOOTGUNS — birth, then the two declared geometries) and a user-damaged
/// tab re-applies once before advancing, so three cooldown-spaced asks — each
/// judged against the width the previous one actually produced — provably
/// visit every position; a fourth could only repeat the walk. The budget is
/// per INTENT, never re-armed by mere movement: a walk whose positions keep
/// the width changing without ever producing the target (a window too narrow
/// to hold it) would otherwise loop forever at the cooldown's cadence — the
/// same runaway the QA drive filmed, just slower.
pub const WALK_ASK_CAP: u8 = 3;

/// Manifests a pane sits out after a probe both OS queries failed on (see
/// `probe_backoff`; FOOTGUNS — the running-latch probe retry, 2026-08-25/26).
/// Three keeps a genuinely slow pane off the 100ms-timeout
/// treadmill (the 3s exit poll stretches to ~12s effective) while a pane
/// mid-spawn — the all-None guard's original case — is back in reach within
/// a few frames; its first answered delta clears the stand-down early anyway.
pub const PROBE_FAILURE_STANDDOWN: u8 = 3;

/// A row that has never received a user commitment (S1). Sorts below every row
/// that has. Reachable for a LIVE tab only when its birth touch never landed —
/// that is RC-B/S0's defect, deliberately NOT papered over with a sentinel that
/// would hide it (S1 §3.2). Also the cold-start state of every tab right after
/// a session recreate clears the store's tab order.
const NO_COMMITMENT: u64 = 0;

/// Row identity (§6.6 C8): a live zellij tab, or a dormant store row
/// (conversation with no tab yet — claude.ai-style list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKey {
    Tab(usize),
    Dormant(String),
}

/// The row comparator (S1): commitment ordinal DESCENDING, then the
/// determinism tiebreak ascending. [`BarModel::rows`] applies it to the live
/// block and the dormant block separately — one rule for both row classes, so
/// segregation (#112) changes which block a row lands in and never what number
/// ranks it.
/// One block entry pre-sort: the primary key (widened to `(u64, u64)` for the
/// frecency fallback), the determinism tiebreak, and the row it ranks.
type Ranked = ((u64, u64), usize, (RowKey, Row));

fn rank_desc(a: &Ranked, b: &Ranked) -> std::cmp::Ordering {
    b.0.cmp(&a.0).then(a.1.cmp(&b.1))
}

/// Frecency score in millipoints: Σ count × 0.5^(age_days × 24 / half_life_hours),
/// ×1000, floored. Millipoints keep the comparator integral; identical bucket
/// maps produce identical sums (BTreeMap order is deterministic), which is
/// what makes the newborn-adjacency tie exact. Future-dated buckets (clock
/// skew) clamp to age 0. half_life_hours == 0 is clamped to 1 (zero half-life
/// is possible via CLI or wire).
///
/// Buckets outside [`clave_types::BUCKET_RETAIN_DAYS`] score ZERO at every
/// dial (maintainer ruling, 2026-08-19): the store prunes them lazily — only
/// on a row's next bump — so a long-dormant row can still carry stale days,
/// and without this cut a huge dial (999h) would resurrect them. "Fully
/// decayed at 7 days" is the semantic; skipping the `powf` is the bonus.
fn frecency_millis(buckets: &BTreeMap<u32, u32>, today: u32, half_life_hours: u32) -> u64 {
    let hl = half_life_hours.max(1) as f64;
    let sum: f64 = buckets
        .iter()
        // Same window arithmetic as the store's `bump_bucket` prune (strict:
        // day + RETAIN > today keeps today-6..=today, seven days inclusive).
        .filter(|&(&day, _)| day + clave_types::BUCKET_RETAIN_DAYS > today)
        .map(|(&day, &count)| {
            let age_days = today.saturating_sub(day) as f64;
            count as f64 * 0.5_f64.powf(age_days * 24.0 / hl)
        })
        .sum();
    (sum * 1000.0) as u64
}

/// PROVISIONAL — delete when S5 lands. Design-lock §4 requires ink allocation
/// to be **store-backed, round-robin, iterate-and-wrap**: one repo is one
/// colour forever, and a title chip is unique within its repo. That is
/// cross-process state with an ordering/idempotency argument to make, and it is
/// S5's job, not this module's. Hashing is overruled twice over — `DefaultHasher`
/// is not stable across toolchains, and the maintainer rejected collisions
/// outright.
///
/// This stand-in exists for exactly one reason: a colourless bar tells the
/// maintainer nothing at the design checkpoint this wiring exists to reach. It
/// is recomputed from every snapshot, persists nothing, and is one field and one
/// function to delete. **Determinism is the part that matters** — a `HashMap`
/// here would reshuffle every colour between processes (and Rust's default
/// hasher is randomly seeded per process, so even one process's two renders
/// could disagree). `BTreeSet`/`BTreeMap` give a stable sort order, so the same
/// snapshot always yields the same palette assignment.
#[derive(Debug, Default)]
struct ProvisionalInks {
    /// repo_root → palette index.
    repo: BTreeMap<String, u8>,
    /// (repo_root, title) → palette index, allocated WITHIN the repo so two
    /// tabs of one repo never share a chip (lock §4).
    title: BTreeMap<(String, String), u8>,
}

impl ProvisionalInks {
    /// PROVISIONAL (see `ProvisionalInks`): sorted distinct keys, index
    /// round-robin by position, wrapping at the palette length.
    fn allocate(agents: &[Agent]) -> Self {
        let mut repos: BTreeSet<&str> = BTreeSet::new();
        let mut titles: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for a in agents {
            // An agent outside a repo has no repo identity to colour; D7 says
            // that is `None`, never `unwrap_or(0)` (0 is crystalBlue, a real
            // hue — a bare `u8` has no unset value).
            if a.repo_root.is_empty() {
                continue;
            }
            repos.insert(a.repo_root.as_str());
            if let Some(t) = a.title.as_deref().filter(|t| !t.is_empty()) {
                titles.entry(a.repo_root.as_str()).or_default().insert(t);
            }
        }
        let wrap = |i: usize| (i % PALETTE.len()) as u8;
        Self {
            repo: repos
                .into_iter()
                .enumerate()
                .map(|(i, r)| (r.to_string(), wrap(i)))
                .collect(),
            title: titles
                .into_iter()
                .flat_map(|(repo, ts)| {
                    ts.into_iter()
                        .enumerate()
                        .map(move |(i, t)| ((repo.to_string(), t.to_string()), wrap(i)))
                })
                .collect(),
        }
    }
}

/// The last non-empty path component of a repo root — lock §2's repo column is
/// a NAME, not a path, and at 7 cells (3 collapsed) a path renders as ellipsis.
/// Trailing slashes are tolerated because a store value is hook-written.
fn basename(path: &str) -> &str {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("")
}

/// One instance's outstanding `clave bind` for a uuid: which tab we claimed,
/// the store `seq` at the moment we claimed it, and how many times we have
/// tried THIS target. See `bind_effects` for the emit rule.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BindSent {
    tab_id: usize,
    at_seq: u64,
    tries: u32,
    /// Consecutive store advances that have shown this bind CONFIRMED. The
    /// budget is refunded only once this reaches `BIND_CONFIRMS_TO_RESET`;
    /// any divergence resets it to zero. See `bind_effects`.
    confirms: u32,
}

/// Bind re-emissions per (uuid, target tab) episode before we stop fighting.
/// The heal RC-A needs is ONE; a lost push needs one or two; beyond that we
/// are in an eviction ping-pong we cannot win (two agents whose panes both
/// resolve into one tab, each bind evicting the other and advancing `seq`),
/// and wrong-but-consistent beats a storm — the `collapse_reasserted`
/// precedent (a second contradiction means someone else is authoritative).
const BIND_MAX_TRIES: u32 = 4;

/// Consecutive confirming store advances required before a (uuid, tab)
/// episode is considered genuinely healed and its attempt budget refunded.
///
/// It must be at least TWO, and that is the whole design. Eviction ping-pong
/// produces a confirmation for whoever won each round, so a budget refunded on
/// a SINGLE confirmation is refunded to every contender on the round it wins —
/// which is the unbounded loop this cap exists to stop. Alternating winners can
/// never post two consecutive confirmations, so they stay capped; a genuinely
/// healed bind sits confirmed across successive snapshots and resets.
const BIND_CONFIRMS_TO_RESET: u32 = 2;

#[derive(Default)]
pub struct BarModel {
    /// The collapse mode is not known yet, so no width switch may be asked for
    /// (D37): the pane is BORN at the width its persisted mode wants, so a
    /// switch on the assumed-expanded default can only move it away from
    /// correct and then visibly back when `clave snapshot` returns.
    ///
    /// A newborn model is `collapsed: false` because that is the only thing it
    /// can assume, but the mode PERSISTS in the store — so a fleet left
    /// collapsed loads a bar that believes it is expanded, seeks 54, and is
    /// corrected to 30 the moment `clave snapshot` returns. Ollie watched
    /// exactly that: born at the right width (D36), grown wide by the plugin,
    /// then shrunk back about half a second later.
    ///
    /// Since D36 the pane is BORN at the width its true mode wants, so any switch
    /// before hydration can only move it away from correct. Gating is therefore
    /// strictly better than seeking on a guess.
    ///
    /// **Defaults to `false` (= ready) so that every existing model test keeps
    /// its exact meaning** — they construct models that are by definition in a
    /// known state. `main.rs`'s `load()` is the one place that sets it, marking
    /// the real plugin as awaiting the snapshot it has just asked for. If that
    /// shellout never returns the bar simply stays at its birth width, which is
    /// correct for the launch tab and stale-but-static otherwise — strictly
    /// better than today's guaranteed wrong-then-heal.
    awaiting_hydration: bool,
    /// Alt+f's spawn is in flight: `ShellSpawn` was emitted and no pane frame
    /// has landed since. Until one does, `shell_toggle`'s inputs are the
    /// pre-spawn state — a second press would read "no floating pane" and
    /// spawn again (key-repeat fans out shells; CodeRabbit, PR #209). Cleared
    /// by ANY `apply_panes`, not only one showing a floating pane: a spawn
    /// that failed must not wedge the key forever ("a dropped Alt+f is a
    /// repeatable keypress"), and the manifest the spawn itself causes is the
    /// next one in flight anyway.
    shell_spawn_pending: bool,
    /// §5 pipe contract: apply only strictly-newer seq.
    seq: u64,
    agents: Vec<Agent>,
    /// uuid → terminal pane id, from clave-register (S2).
    uuid_to_pane: BTreeMap<String, u32>,
    tabs: Vec<TabMeta>,
    /// Tab ids this instance WITNESSED dying: present in one delivered tab
    /// frame, absent from the next. The only ids `prune_effect` may claim —
    /// TabUpdate reaches only the active tab, so a background bar's `tabs`
    /// can predate a create, and any id it was never shown dying is a newborn,
    /// not a corpse (2026-08-17 QA drive: a starved bar pruned a just-created
    /// tab's register+touch+bind in one write, and #187's replace-not-merge
    /// made the unbind permanent). A claim is settled by the STORE ECHO — the
    /// snapshot that no longer references the id — never at emit (emit-time
    /// consumption is the missed-action class: a false executor gate would
    /// eat the claim with no retry). Settling matters because zellij REUSES
    /// ids (a closed top tab's id returns on the next create — Codex P1,
    /// PR #202): a claim that outlived its own echo would read the reused
    /// id's newborn as the corpse it once witnessed. A delivered frame that
    /// shows a claimed id ALIVE again also voids the claim — witnessed
    /// reborn, so whatever write raced ahead belongs to the newborn.
    witnessed_dead: BTreeSet<usize>,
    panes: Vec<PaneMeta>,
    /// pane id → what the OS said about it (#206). Written by
    /// `apply_pane_facts` (main.rs probes and event deltas), pruned by
    /// `apply_panes` when the manifest no longer carries the pane. Only
    /// terminal-tab speaker panes are ever probed, but the map holds whatever
    /// it is handed — the row build, not the ingest, decides relevance.
    pane_facts: BTreeMap<u32, TermFacts>,
    /// pane id → manifests left to skip after a probe both OS queries failed
    /// on (all-None). The all-None guard mints no facts entry — correctly, a
    /// failure is not knowledge — which left "retry until known" with no
    /// can-this-ever-succeed test: one pane burned four hours of 100ms query
    /// timeouts, re-qualifying on every manifest (2026-08-25). Entries decay
    /// one per delivered manifest in `apply_panes`, die with their pane, and
    /// are cleared outright by any answered probe — a stand-down, never a
    /// blacklist.
    probe_backoff: BTreeMap<u32, u8>,
    /// tab_id → the commitment ORDINAL of the last USER COMMITMENT to that tab
    /// (§6.6 / S1). NOT owned here: the store is the one writer (`clave touch`
    /// RMW) and this copy is REPLACED wholesale from every seq-gated
    /// snapshot — instance-local copies merged from pipe deltas diverged
    /// live (C5 round 5) and walking oscillated. Focus is deliberately NOT
    /// a commitment (the list holds still while you look around).
    tab_order: BTreeMap<usize, u64>,
    /// Ordering mode + dial, `today`, and the tab-keyed bucket twin — all
    /// REPLACED wholesale from every snapshot, never merged (the tab_order
    /// doctrine, C5 round 5). `today` is the HOST's unix day, stamped into
    /// every snapshot: the bar never reads a wall clock.
    order: OrderMode,
    today: u32,
    tab_buckets: BTreeMap<usize, BTreeMap<u32, u32>>,
    /// tab_id → unix secs of the last USER interaction with a TERMINAL tab
    /// (#232's wall-clock twin of `tab_order`'s commitment ordinal). Same
    /// REPLACE doctrine as its neighbours above: the store is the one writer,
    /// this copy exists only so `terminal_content`'s elapsed cell has
    /// something to read — an agent row reads `Agent::last_interacted`
    /// instead and never touches this map.
    tab_touched: BTreeMap<usize, u64>,
    /// Tabs THIS instance has already birth-touched (`clave touch` spawned).
    /// Local and echo-independent on purpose: a guard that waited for the
    /// store echo re-fired per TabUpdate (C5 rd 4 spawn storm → server fd
    /// exhaustion). Never re-armed, even if a snapshot drops the tab.
    birth_touched: BTreeSet<usize>,
    /// Last label WE wrote per uuid — the rename loop-guard (§6.4). Renames
    /// fire on label CHANGE only, so user manual renames stick in between.
    renamed: BTreeMap<String, String>,
    /// OUR plugin pane id (`get_plugin_ids().plugin_id`), set once at load.
    /// Identity resolution lives HERE, not in the adapter, because `main.rs`
    /// is `test = false` — everything that joins the two zellij frames must be
    /// host-testable or it is unguarded (RC-A shipped for exactly that reason).
    own_pane: Option<u32>,
    /// uuid → the bind we last SENT, the `seq` in effect when we sent it, and
    /// how many times we have tried this target. Replaces `sent_binds`, whose
    /// "last sent, never cleared" latch made a wrong bind permanent for the
    /// life of the plugin instance (RC-A, #55). The guard is still NOT the
    /// snapshot echo — it is the store's monotonic `seq`, which advances only
    /// on a real store mutation, so a quiescent store costs zero subprocesses
    /// no matter how many frames arrive (C5 rd 4's echo gate re-fired per
    /// TabUpdate and exhausted the server's fds; this cannot).
    bind_sent: BTreeMap<String, BindSent>,
    /// Last bind-leg state we reported (#178). Only a CHANGE is worth a line;
    /// see `Effect::BindStall`.
    bind_stall: Option<BindStallState>,
    /// Local unread-override: Done agents we've already cleared on focus.
    /// Render-side only; `clave focus` persists the real transition.
    read_locally: BTreeSet<String>,
    /// The width machine's state (#197, rebuilt on painted truth; paced by a
    /// cooldown after the 2026-08-17 QA drive filmed the paint-echo runaway).
    ///
    /// A switch is DERIVED: it is owed whenever the width zellij paints this
    /// pane at is not the width the mode declares
    /// (`self.row_height.target_cols` — a fixed column count, per #232's
    /// mode, that the layouts carry verbatim, so the comparison is equality
    /// against a constant). There is no queue to replay, only an end state
    /// to reach; a mode that leaves and returns owes nothing.
    ///
    /// `swap_in_flight`: an ask has been sent and its cooldown timer has not
    /// fired yet. While set the machine records paints but judges none of
    /// them — a swap's repaints arrive queued and STALE, and a machine that
    /// judged them spent its whole budget on echoes of its own last move,
    /// which is exactly the infinite toggle loop the QA drive filmed
    /// (three asks per millisecond, each burst re-arming the next).
    /// Cleared only by [`Self::width_cooldown_elapsed`], which judges the
    /// latest paint once.
    swap_in_flight: bool,
    /// The width of the most recent paint, recorded deaf or not — what the
    /// cooldown expiry judges instead of the paint that preceded the ask.
    last_painted: Option<usize>,
    /// The walk budget: `(wanted mode, asks spent on it without landing)`.
    /// Cleared when the painted width EQUALS the target or the intent
    /// changes (toggle, peek, focus loss) — never by mere movement, which
    /// the machine's own walk causes ([`WALK_ASK_CAP`]). Never consulted to
    /// decide where the tab is, only whether asking again can help.
    walk_spent: Option<(bool, u8)>,
    /// tab_id of the last visited (focused) tab — replicated on every
    /// instance from the visited-pipe/nav broadcast streams. This is the nav
    /// walk base: the local TabInfo.active flag is stale everywhere except
    /// the active instance (zellij delivery finding, C3–C5).
    current_tab: Option<usize>,
    /// A fresh instance announces its own-active claim once (new tab / plugin
    /// (re)load) — the only self-initiated announce an instance ever gets
    /// (rounds 11–12). Spent when the claim is either MADE or already true of
    /// the beacon, never merely because a tab frame arrived: a first TabUpdate
    /// carrying no active tab used to burn it on nothing, which is why a fresh
    /// tab could not heal a session with a stranded beacon (#162). The cost of
    /// that patience: an instance that has never seen a frame WITH an active
    /// tab still holds its UNGATED announce, so a later toggle burst (which
    /// hands every instance a fresh set) can be the frame that spends it. That
    /// is bounded to exactly those instances — one announce each, ever — and
    /// the frame that spends it carries the fresh set, so the claim it makes is
    /// true of that frame.
    birth_announced: bool,
    /// A re-anchor this instance OWES: the last delivered tab frame showed the
    /// beacon outside the live set, and this instance could not send the
    /// re-anchor on that frame because its own two frames disagreed (#162).
    /// `apply_panes` pays it on the pane frame that restores coherence.
    ///
    /// It cannot be re-derived on demand, which is the whole reason it is a
    /// field: `beacon_stranded()` asks "is the beacon absent from `self.tabs`",
    /// and a starved instance's `self.tabs` is FROZEN (FOOTGUNS.md:63), so for
    /// it that question conflates DEAD with CREATED-SINCE-MY-LAST-FRAME. Every
    /// new tab broadcasts a birth beacon, so a re-derived debt would be owed by
    /// every hidden bar on the commonest gesture in the product. Written only
    /// in `apply_tabs`, from the frame it just stored; cleared by `beacon()`,
    /// because any incoming beacon is fresher than the frame that convicted the
    /// old one, and by the payment itself.
    ///
    /// It is a DEBT, never a licence: the only thing it buys is one pipe, on a
    /// frame zellij delivered. It was briefly the licence for a nav fallback
    /// (prefer local truth while the beacon is provably wrong) and that shape
    /// is unsound — see `nav_executor`, and FOOTGUNS.md's "a licence tried and
    /// removed".
    reanchor_owed: bool,
    /// Armed by the Alt+o bind's `clave-organic` pipe: the NEXT TabUpdate
    /// may announce (steady-state TabUpdates reach only the truly active
    /// instance, C3). Disarmed by any incoming beacon — the active
    /// instance spoke; a stale instance must not answer a leftover flag
    /// with poison during a later event burst — and, since #162, disarmed by
    /// the announce itself rather than by the frame that refused to send it.
    organic_pending: bool,
    /// Bar collapsed to the glyph gutter (Alt+c)? Round 20: purely a width
    /// state — the pane itself is never hidden or suppressed.
    pub collapsed: bool,
    /// Peek-on-nav: a collapsed bar showing the template width because the
    /// user is navigating. Armed by `visited` (the replicated clave-visited
    /// pipe), cleared by `peek_expired` (main.rs's ~1s timer) or `toggle`.
    peeking: bool,
    /// Issue #5 pending-write ledger: the collapse mode we still OWE the
    /// store after a toggle (fix-review MAJOR: two rapid toggles spawn two
    /// `clave collapse` subprocesses with no arrival-order guarantee — the
    /// change-gate can swallow the correct write and the store's push then
    /// overrides the user). Cleared when an accepted snapshot carries the
    /// owed value; a contradicting accepted snapshot instead keeps USER
    /// truth and re-asserts — at most once (`collapse_reasserted`), so two
    /// instances with conflicting pendings can never ping-pong (round 11).
    pending_collapse: Option<bool>,
    /// The single re-assert of `pending_collapse` has been spent; the next
    /// contradicting snapshot wins (wrong-but-consistent beats a storm).
    collapse_reasserted: bool,
    /// uuids with a `clave open` in flight (§6.6): set on fire, shown ↻.
    /// Cleared when the row stops being dormant (tab appeared) or a stale=true
    /// snapshot lands (open failed → ✗, retryable). First double-fire guard;
    /// `clave open`'s liveness no-op is the second.
    opening: BTreeSet<String>,
    /// §6.6 C8 virtual selection cursor: Some(uuid) while nav sits on a
    /// dormant row (there is no tab to focus). Nav steps continue from it;
    /// it resolves back to the focused-tab row on any live-row landing.
    /// Alt+Enter commits it (#100) — selection alone never launches.
    cursor: Option<String>,
    /// Bumped on EVERY landing so a late timer for an abandoned landing is
    /// provably stale. Its consumer (the dwell) died with #100; kept because
    /// #92's read timer takes over the same invalidation role.
    #[allow(dead_code)]
    cursor_gen: u64,
    /// Which row geometry this bar renders (#232) — set once at `load()`
    /// from `plugin_config::resolve_row_height`, never afterward: the launch
    /// layout bakes both the pane sizes and this config key from the same
    /// choice, so a bar cannot disagree with the geometry zellij actually
    /// gave its pane mid-session. Every width ask (`widths_at`,
    /// `width_effects`) reads the target through `RowHeight::target_cols`
    /// instead of a raw `BAR_TARGET_COLS`/`COLLAPSED_TARGET_COLS` constant,
    /// so the two arms of the flag share one seek machine. Defaults to
    /// `RowHeight::default()` (`Double`) so existing tests that never call
    /// `set_row_height` keep testing the shipping default.
    row_height: RowHeight,
    /// The shell's wall clock, set by [`Self::tick`] (#232). A field rather
    /// than a parameter threaded through `rows`/`click`/`agent_content`/
    /// `terminal_content`: those four signatures would otherwise all need a
    /// `now` neither `click` nor most tests care about, for the sake of the
    /// one leaf (`elapsed_label`) that does. The model still never READS a
    /// clock itself — `main.rs` is the only caller of `tick`, once per
    /// render, with `wall_now()` — so model.rs stays a pure state machine
    /// driven entirely by values handed to it.
    now: u64,
}

impl BarModel {
    /// A `clave-visited` pipe landed: some tab gained focus. Beacon ONLY —
    /// it elects the nav executor; it never reorders (§6.6: focus is not a
    /// commitment).
    pub fn beacon(&mut self, tab_id: usize) {
        self.current_tab = Some(tab_id);
        self.organic_pending = false; // truth arrived; leftover flags are poison
        // A new beacon re-anchors the election, so whatever an earlier tab
        // frame proved about the OLD beacon is spent, debt included (#162):
        // re-anchoring a beacon that has already moved would announce a tab
        // nobody asked for.
        self.reanchor_owed = false;
        // Any real tab visit is live-focus truth, so the §6.6 selection must
        // resolve back to the focused tab. Without this a committed open that
        // FAILED (row goes ✗ stale but stays dormant, cursor pinned to it)
        // would keep the selection highlight — and suppress the real active
        // tab — through a NATIVE switch (Alt+o / zellij binds) that carries
        // no clave-nav. The dormant nav branch sets `cursor` AFTER its (no)
        // beacon call, so clearing here never races a fresh dormant landing.
        self.cursor = None;
    }

    /// The `clave-visited` pipe entry: beacon, plus peek-on-nav — a
    /// collapsed bar expands while the user navigates. Returns true when a
    /// peek was armed so main.rs starts the ~1s sink timer. ONLY this pipe
    /// path arms peeks: the internal beacon callers (click/nav) can't start
    /// host timers, and their AnnounceVisit echoes back as this very pipe
    /// on every instance — a peek armed without a timer would stick.
    pub fn visited(&mut self, tab_id: usize) -> bool {
        self.beacon(tab_id);
        if !self.collapsed {
            return false; // expanded bars stay expanded — nothing to peek
        }
        self.peeking = true; // the expanded geometry is now owed (`swap_owed`)
        true
    }

    /// The LAST peek timer expired (main.rs counts one per nav, so a nav
    /// burst sinks once, ~1s after the final press): sink back to the
    /// gutter. Returns whether anything changed — false when a toggle
    /// already cancelled the peek (a late timer must not book a switch).
    pub fn peek_expired(&mut self) -> bool {
        if !self.peeking {
            return false;
        }
        self.peeking = false;
        // The gutter geometry is owed again the moment `peeking` drops
        // (`swap_owed`); nothing to book.
        true
    }

    /// Alt+o's bind pipes `clave-organic` alongside the native ToggleTab:
    /// arm ONE announce on the next TabUpdate (rounds 11–12: unbounded
    /// self-diagnosed announces storm; bounded triggers cannot).
    pub fn set_organic_pending(&mut self) {
        self.organic_pending = true;
        // #100 commit-race guard (Codex P2 on #128): the native switch has
        // ALREADY happened server-side, but this instance's beacon lags
        // until the new active bar's TabUpdate announces and the visited
        // pipe returns. Spend the dormant selection NOW — this pipe rides
        // the same Alt+o keybind, so it lands synchronously on every
        // instance — else a broadcast Alt+Enter inside that gap passes the
        // stale executor gate on the DEPARTED bar and commits a selection
        // the visible bar does not show.
        self.cursor = None;
    }

    /// Should this instance fire `clave touch` for a newly-active tab it has
    /// never seen? True at most ONCE per (instance, tab), and never for a
    /// tab the store's tab order already carries. Duplicates across instances
    /// are fine — each mints its own ordinal under the store flock, and the
    /// later one simply wins.
    pub fn needs_birth_touch(&mut self, tab_id: usize) -> bool {
        !self.tab_order.contains_key(&tab_id) && self.birth_touched.insert(tab_id)
    }

    /// §6.6 ordering key for a LIVE tab row (S1): the higher of the STORE's
    /// ordinal for that tab and — when an agent occupies it — that agent's own
    /// ordinal.
    ///
    /// A plain terminal tab has only the tab's ordinal, which is why the tab
    /// map is still the primary source. Taking the max with the agent's is what
    /// makes this the SAME rule as [`Self::dormant_ord`], and that identity is
    /// load-bearing: a row must rank by the same number whether it is live or
    /// dormant, or merely closing its tab would change its rank (R2). `clave
    /// add` creates the tab BEFORE it writes the row, so the two ordinals
    /// genuinely can arrive in either order — see
    /// `a_rows_rank_does_not_change_when_it_goes_dormant` (Codex, PR #135).
    ///
    /// Both values ride the SAME seq-gated snapshot, so this is not the
    /// round-6 hazard: that one was a render-time join against per-instance
    /// register/manifest state, which differs between instances. This has one
    /// source and cannot diverge.
    fn live_ord(&self, t: &TabMeta) -> u64 {
        let tab = self
            .tab_order
            .get(&t.tab_id)
            .copied()
            .unwrap_or(NO_COMMITMENT);
        match self.agent_in_tab(t.tab_id) {
            Some(a) => tab.max(a.commit_ord),
            None => tab,
        }
    }

    /// §6.6 ordering key for a DORMANT row (S1) — the same rule as
    /// [`Self::live_ord`], read from the other side: the agent's own ordinal,
    /// OR, while the store has not yet pruned the tab it was bound to, that
    /// tab's ordinal.
    ///
    /// That second leg is what holds the row's RANK on the FIRST repaint,
    /// without waiting for the fire-and-forget `clave prune-tabs` echo.
    /// `is_dormant` guarantees no LIVE tab holds `a.tab_id`, so a recycled id
    /// can never be read here.
    fn dormant_ord(&self, a: &Agent) -> u64 {
        let carried = a
            .tab_id
            .and_then(|id| self.tab_order.get(&id))
            .copied()
            .unwrap_or(NO_COMMITMENT);
        a.commit_ord.max(carried)
    }

    /// The comparator's primary key for a LIVE row. Recency: the shipped
    /// ordinal. Frecency: millipoints, max-merged across the tab twin and the
    /// agent's own buckets (same R2 identity as [`Self::live_ord`]); a
    /// zero-score row falls back to `(0, ordinal)` so an unbucketed fleet —
    /// upgrade day, cold dormants, the whole pre-frecency test suite — keeps
    /// the shipped S1 order instead of collapsing to tab position.
    fn live_key(&self, t: &TabMeta) -> (u64, u64) {
        match self.order {
            OrderMode::Recency => (self.live_ord(t), 0),
            OrderMode::Frecency { half_life_hours } => {
                let tab = self
                    .tab_buckets
                    .get(&t.tab_id)
                    .map_or(0, |b| frecency_millis(b, self.today, half_life_hours));
                let agent = self.agent_in_tab(t.tab_id).map_or(0, |a| {
                    frecency_millis(&a.buckets, self.today, half_life_hours)
                });
                let millis = tab.max(agent);
                if millis > 0 {
                    (millis, 0)
                } else {
                    (0, self.live_ord(t))
                }
            }
        }
    }

    /// Same rule read from the dormant side — one rule for both row classes,
    /// or closing a tab would change a row's rank (R2).
    fn dormant_key(&self, a: &Agent) -> (u64, u64) {
        match self.order {
            OrderMode::Recency => (self.dormant_ord(a), 0),
            OrderMode::Frecency { half_life_hours } => {
                let own = frecency_millis(&a.buckets, self.today, half_life_hours);
                let carried = a
                    .tab_id
                    .and_then(|id| self.tab_buckets.get(&id))
                    .map_or(0, |b| frecency_millis(b, self.today, half_life_hours));
                let millis = own.max(carried);
                if millis > 0 {
                    (millis, 0)
                } else {
                    (0, self.dormant_ord(a))
                }
            }
        }
    }

    /// #178 instrumentation. Reports the bind leg's state, and ONLY when it
    /// changes — a bar sitting healthy, or sitting starved, costs nothing per
    /// frame. Read-only over the frames; it decides nothing.
    ///
    /// `stranded` counts the rows that carry the whole hypothesis: unbound in
    /// the store, pane id known to us from the snapshot or the broadcast, and
    /// that pane absent from our pane frame. A dormant row that has never been
    /// woken has no pane id at all and is deliberately NOT counted — dormancy
    /// is not a stall, and conflating the two would make this line fire on
    /// every healthy fleet.
    /// **Runs BEFORE every gate, and that is the whole point** (#184 review,
    /// found independently by two reviewers). `identity_effects` refuses on an
    /// unelected instance and again on an unresolved own tab, and both refusals
    /// return before `bind_effects` is ever called — so a report computed
    /// inside the bind pass could only ever describe frames that were already
    /// healthy enough to bind. The two states this hunt most needs to see are
    /// exactly the ones those gates swallow.
    pub fn bind_stall_report(&mut self) -> Option<Effect> {
        let stranded = self
            .agents
            .iter()
            .filter(|a| a.tab_id.is_none())
            .filter(|a| {
                self.uuid_to_pane
                    .get(&a.uuid)
                    .is_some_and(|p| self.tab_position_of_pane(*p).is_none())
            })
            .count();
        // `own_tab()` is `Some` only when the frames agree, so it answers both
        // of the first two questions — but not which one, and the difference
        // is a stale pane frame versus a tab that has left the frame entirely.
        let own_tab = self.own_tab();
        let state = if !self.frames_coherent() {
            BindStallState::FramesIncoherent
        } else if own_tab.is_none() {
            BindStallState::OwnPositionUnknown
        } else if stranded > 0 {
            BindStallState::PaneKnownButAbsent
        } else {
            BindStallState::Cleared
        };
        // First frames of a healthy birth are incoherent by construction, so
        // Cleared is only worth saying after something was said before it.
        if self.bind_stall == Some(state)
            || (self.bind_stall.is_none() && state == BindStallState::Cleared)
        {
            self.bind_stall = Some(state);
            return None;
        }
        self.bind_stall = Some(state);
        Some(Effect::BindStall {
            state,
            stranded,
            own_tab,
        })
    }

    /// Which tab (by current position) holds this pane?
    fn tab_position_of_pane(&self, pane_id: u32) -> Option<usize> {
        self.panes
            .iter()
            .find(|p| p.pane_id == pane_id && !p.is_plugin)
            .map(|p| p.tab_position)
    }

    /// Record OUR plugin pane id. `main.rs`'s `load()` is the one caller.
    pub fn set_own_pane(&mut self, plugin_pane_id: u32) {
        self.own_pane = Some(plugin_pane_id);
    }

    /// Sets the row-height mode this bar renders — `main.rs`'s `load()`
    /// calls it once, right after resolving it from plugin configuration
    /// (#232). See the `row_height` field doc for why this is the only
    /// writer.
    pub fn set_row_height(&mut self, row_height: RowHeight) {
        self.row_height = row_height;
    }

    /// The geometry this bar draws — `main.rs` hands it to `render_rows` so
    /// the renderer and [`Self::click`]'s `/2` read the same field, never two
    /// separately-resolved copies of the mode (#148 discipline).
    pub fn row_height(&self) -> RowHeight {
        self.row_height
    }

    /// The shell's wall clock, called once before every render (#232):
    /// `main.rs` supplies `wall_now()`, and the existing `TERM_POLL_SECS`
    /// cadence's re-render is what keeps the elapsed cell's minutes honest
    /// between pushes. See the `now` field doc for why this is a tick rather
    /// than a parameter on `rows`.
    pub fn tick(&mut self, now: u64) {
        self.now = now;
    }

    /// Our own tab position, per the LAST PaneUpdate. Plugin and terminal pane
    /// ids are separate id spaces ("unique to all panes of this kind",
    /// zellij-utils data.rs:2297-2300), so the `is_plugin` filter is
    /// load-bearing — the same reason `tab_position_of_pane` filters
    /// `!is_plugin`.
    fn own_tab_position(&self) -> Option<usize> {
        let own = self.own_pane?;
        self.panes
            .iter()
            .find(|p| p.is_plugin && p.pane_id == own)
            .map(|p| p.tab_position)
    }

    /// True when the last TabUpdate and the last PaneUpdate describe the same
    /// tab set. The manifest is keyed by tab POSITION (zellij-utils
    /// data.rs:2277-2282) and every live tab has at least one pane, so a
    /// coherent pair covers exactly the same positions. They diverge for
    /// exactly as long as one frame predates a tab create/close — the
    /// renumbering window RC-A rides.
    ///
    /// It reads ALL panes, not just plugin ones. Every clave tab does get
    /// exactly one bar, but keying the witness on that would make it fail
    /// permanently the day a tab without a bar exists; keying on all panes is
    /// unconditionally true of a coherent manifest.
    ///
    /// Known residual (documented, not fixed here): an identity permutation
    /// that PRESERVES the position set — close the lowest tab and create one
    /// in the same window — satisfies this witness while every occupant has
    /// shifted. `PaneManifest` carries no `tab_id` and `TabInfo` carries no
    /// pane identity, so position is the only cross-frame key these two events
    /// share and no stronger witness is constructible from them. The
    /// self-healing bind below degrades that case from "permanent mis-bind" to
    /// "one transient mis-bind that self-corrects on the next `seq` advance".
    fn frames_coherent(&self) -> bool {
        // Pre-first-frame: fail closed. Note for the next reader (and for
        // `cargo mutants`, which flags `||`→`&&` here as MISSED): the two
        // operators are behaviourally identical, because a frame pair with
        // exactly ONE side empty already fails the set comparison below. The
        // case this line uniquely covers is BOTH sides empty, where two empty
        // frames would otherwise trivially "agree". That state is unreachable
        // through any public path — `own_tab_position()` is None with no panes
        // — so no test can kill the mutant. It is equivalent, not a gap.
        if self.tabs.is_empty() || self.panes.is_empty() {
            return false;
        }
        let tab_pos: BTreeSet<usize> = self.tabs.iter().map(|t| t.position).collect();
        let pane_pos: BTreeSet<usize> = self.panes.iter().map(|p| p.tab_position).collect();
        tab_pos == pane_pos
    }

    /// Our tab id — `Some` ONLY when the two frames agree. Fail closed: the
    /// caller does nothing and re-enters on the next frame, which is the very
    /// frame that resolves the disagreement.
    pub fn own_tab(&self) -> Option<usize> {
        if !self.frames_coherent() {
            return None;
        }
        let pos = self.own_tab_position()?;
        self.tabs
            .iter()
            .find(|t| t.position == pos)
            .map(|t| t.tab_id)
    }

    /// Election, strong form: the frames agree AND the tab they resolve us
    /// into is the active one. Gates `Bind`, `Touch`, `PruneTabs` and the
    /// `clave-nav` executor — every effect that either retries or would do
    /// lasting damage from the wrong instance. `Confirmed` implies `Presumed`.
    pub fn elects_confirmed(&self) -> bool {
        self.own_tab()
            .is_some_and(|id| self.tabs.iter().any(|t| t.tab_id == id && t.active))
    }

    /// The PRE-#55 computation, byte-for-byte: join the two frames on position
    /// and ask whether that tab is active, with no coherence witness. Kept for
    /// the effects that latch at emit and therefore CANNOT survive a
    /// fail-closed gate — `RenameTab`, `MarkRead` and `PersistCollapse` all
    /// drop silently under a false gate with no trigger to re-evaluate them, so
    /// tightening them converts a wrong-action bug into a missed-action bug.
    /// Their emit-time latch is the real defect and is out of scope for #55.
    /// `ReanchorVisit` was the fourth until #162 gave it a retry trigger; it
    /// asks this same question, but from `apply_tabs`, at emit time.
    pub fn elects_presumed(&self) -> bool {
        self.own_tab_position()
            .and_then(|pos| self.tabs.iter().find(|t| t.position == pos))
            .is_some_and(|t| t.active)
    }

    /// The tab zellij's last frame says is active — any instance's view.
    pub fn active_tab_id(&self) -> Option<usize> {
        self.tabs.iter().find(|t| t.active).map(|t| t.tab_id)
    }

    /// Every effect keyed on THIS instance's tab identity. Fail-closed by
    /// construction: `elects_confirmed()` is false while the frames disagree,
    /// so this returns nothing and the caller re-enters on the next frame.
    ///
    /// The single entry point exists because bind emission used to be an
    /// adapter-level call every snapshot/frame arm had to remember separately
    /// — and one of them forgot, which is RC-B.
    pub fn identity_effects(&mut self) -> Vec<Effect> {
        if !self.elects_confirmed() {
            return Vec::new();
        }
        let Some(own) = self.own_tab() else {
            return Vec::new();
        };
        let mut fx = Vec::new();
        // Birth touch FIRST: a newly-created tab wants its ordinal stamp
        // before its bind. `needs_birth_touch` is once-EVER per (instance,
        // tab) and the latch is consumed here and only here — i.e. only when
        // we are actually emitting, so a false gate DEFERS rather than
        // consumes (C5 rd 4: echo-gated re-arming is the storm).
        //
        // `active == own` is the fix to a second, one-line-away instance of
        // RC-A: the old adapter block touched the active id from the TAB frame
        // while gating on a position join against the PANE frame. Requiring
        // the active tab to be OUR tab makes the touch self-consistent by
        // construction.
        if let Some(active) = self.active_tab_id()
            && active == own
            && self.needs_birth_touch(active)
        {
            fx.push(Effect::Touch { tab_id: active });
        }
        fx.extend(self.bind_effects(own));
        // Prune LAST: its payload is disjoint from the touch's (dead ids vs a
        // live one) and from any bind's, so ordering is free, and keeping it
        // after the binds means a settle that both binds and prunes reports
        // the bind first.
        fx.extend(self.prune_effect());
        fx
    }

    /// The agent bound to this tab, per the SNAPSHOT (§6.6 Design B) — the
    /// only join every instance agrees on. Local register/manifest joins are
    /// used solely to CREATE binds (bind_effects).
    fn agent_in_tab(&self, tab_id: usize) -> Option<&Agent> {
        self.agents.iter().find(|a| a.tab_id == Some(tab_id))
    }

    /// §6.6 Design B bootstrap: agents whose REGISTERED pane sits in
    /// `own_tab` per MY manifest, but whose snapshot bind disagrees.
    ///
    /// Reached through `identity_effects`, which is already coherence-gated;
    /// the early return below closes the DIRECT-call path so this is not a
    /// hole (a stale manifest joined to a fresh tab set is RC-A itself).
    ///
    /// The emit rule is progress-gated, not echo-gated: re-emission requires
    /// the store's `seq` to have strictly advanced since our own send, and is
    /// capped at `BIND_MAX_TRIES` per (uuid, target) episode. A successful
    /// `clave bind` always bumps `seq` and pushes, so "only once a strictly
    /// newer snapshot has been accepted and the join STILL disagrees" means:
    /// never while our own write is in flight, and immediately once its result
    /// is known. An unwinnable bind (`apply_bind` returns `None` for an
    /// unknown uuid — no push, no `seq` bump) costs exactly one subprocess.
    pub fn bind_effects(&mut self, own_tab: usize) -> Vec<Effect> {
        let mut out = Vec::new();
        if !self.frames_coherent() {
            return out;
        }
        let own_position = self
            .tabs
            .iter()
            .find(|t| t.tab_id == own_tab)
            .map(|t| t.position);
        let seq = self.seq;
        let mut clear: Vec<String> = Vec::new();
        let mut confirmed: Vec<String> = Vec::new();
        let mut diverged: Vec<String> = Vec::new();
        let mut sent: Vec<(String, BindSent)> = Vec::new();
        for a in &self.agents {
            let joined_here = self
                .uuid_to_pane
                .get(&a.uuid)
                .and_then(|p| self.tab_position_of_pane(*p))
                .is_some_and(|pos| Some(pos) == own_position);
            // The pane has left our tab: the episode is genuinely over and the
            // ledger clears, so a later arrival is fought afresh at full
            // budget.
            if !joined_here {
                clear.push(a.uuid.clone());
                continue;
            }
            // The store confirms our join: nothing to send. The attempt
            // history is NOT dropped here — it is dropped only once the
            // confirmation has HELD across BIND_CONFIRMS_TO_RESET store
            // advances (handled after the loop).
            //
            // Dropping it on a single confirmation is the obvious move and it
            // is wrong (Codex P1 on PR #120; the S0 spec §1.4 prescribed it,
            // and the spec's own storm argument contradicts it). Under
            // eviction ping-pong — two agents whose panes both resolve into
            // this tab — every bind evicts the other and pushes a snapshot
            // confirming the new winner, so a single-confirmation reset hands
            // the budget back to each contender on the round it wins and
            // BIND_MAX_TRIES bounds nothing: an unbounded subprocess loop, the
            // C5 rd-4 fd-exhaustion class.
            //
            // But never resetting is the mirror bug (adversarial review, same
            // PR): an eviction changes neither our pane nor our target, so
            // every eviction our agent ever suffers would spend a try
            // permanently, and after four LIFETIME evictions a correct bind
            // would be silenced for the life of the plugin instance — the old
            // `sent_binds` permanent latch, reached slowly. Requiring the
            // confirmation to hold separates the two: a fight cannot post two
            // consecutive confirmations, a healed bind can.
            if a.tab_id == Some(own_tab) {
                confirmed.push(a.uuid.clone());
                continue;
            }
            // Joined here and NOT confirmed: the run of confirmations is
            // broken whether or not we go on to emit. Resetting only on emit
            // let a capped agent quietly bank alternating confirmations until
            // it hit the reset threshold and won its budget back — which is
            // the ping-pong refund by a slower route.
            diverged.push(a.uuid.clone());
            let prev = self.bind_sent.get(&a.uuid);
            let (may_send, tries) = match prev {
                // Never tried this episode.
                None => (true, 0),
                // A different target is a NEW episode: full budget.
                Some(s) if s.tab_id != own_tab => (true, 0),
                // Same target: only once the store has moved on since our
                // send, and only BIND_MAX_TRIES times. `seq` advances on a
                // real store mutation and nothing else, so a quiescent store
                // costs zero subprocesses however many frames arrive.
                Some(s) => (seq > s.at_seq && s.tries < BIND_MAX_TRIES, s.tries),
            };
            if may_send {
                sent.push((
                    a.uuid.clone(),
                    BindSent {
                        tab_id: own_tab,
                        at_seq: seq,
                        tries: tries + 1,
                        confirms: 0, // a divergence resets the run of confirmations
                    },
                ));
                out.push(Effect::Bind {
                    uuid: a.uuid.clone(),
                    tab_id: own_tab,
                });
            }
        }
        for uuid in clear {
            self.bind_sent.remove(&uuid);
        }
        for uuid in diverged {
            if let Some(rec) = self.bind_sent.get_mut(&uuid) {
                rec.confirms = 0;
            }
        }
        // A confirmation counts only when the STORE has advanced since we last
        // looked, so a burst of frames at one seq cannot fake a run of them.
        for uuid in confirmed {
            let Some(rec) = self.bind_sent.get_mut(&uuid) else {
                continue; // nothing outstanding — already healed
            };
            if seq > rec.at_seq {
                rec.at_seq = seq;
                rec.confirms += 1;
            }
            if rec.confirms >= BIND_CONFIRMS_TO_RESET {
                self.bind_sent.remove(&uuid); // held: the episode is over
            }
        }
        for (uuid, s) in sent {
            self.bind_sent.insert(uuid, s);
        }
        // Ledger hygiene: an agent that has left the snapshot entirely can
        // never be matched again, so its entry would otherwise persist for the
        // life of the instance.
        let known: BTreeSet<&str> = self.agents.iter().map(|a| a.uuid.as_str()).collect();
        self.bind_sent
            .retain(|uuid, _| known.contains(uuid.as_str()));
        out
    }

    /// The commit path (Alt+Enter, #100): mark in-flight and emit the run.
    /// The `opening` guard is double-fire protection #1 (clave open's
    /// liveness no-op is #2). The caller has already refused stale rows —
    /// ✗ offers no launch, and a dead row is #112's retirement business.
    fn open_effects(&mut self, uuid: &str) -> Vec<Effect> {
        if self.opening.contains(uuid) {
            return Vec::new();
        }
        self.opening.insert(uuid.to_string());
        vec![Effect::OpenAgent {
            uuid: uuid.to_string(),
        }]
    }

    /// §6.6 C8 dormancy: no CURRENT tab carries the bind, and no REGISTERED
    /// pane is present in the manifest. The pane-join leg is instance-local —
    /// divergence only flickers a dormant row briefly (harmless) — but it
    /// suppresses the duplicate row in the pre-bind beat after a tab spawns.
    fn is_dormant(&self, a: &Agent) -> bool {
        let tab_live = a
            .tab_id
            .is_some_and(|id| self.tabs.iter().any(|t| t.tab_id == id));
        let pane_live = self
            .uuid_to_pane
            .get(&a.uuid)
            .is_some_and(|p| self.tab_position_of_pane(*p).is_some());
        !tab_live && !pane_live
    }

    /// Drop in-flight marks that resolved: the row went live (open succeeded)
    /// or the snapshot flagged it stale (open failed). Called after every
    /// input that changes the join picture.
    fn prune_opening(&mut self) {
        let resolved: Vec<String> = self
            .opening
            .iter()
            .filter(|u| {
                self.agents
                    .iter()
                    .find(|a| &&a.uuid == u)
                    .is_none_or(|a| !self.is_dormant(a) || a.stale)
            })
            .cloned()
            .collect();
        for u in resolved {
            self.opening.remove(&u);
        }
    }

    /// The `clave-register` broadcast (S2). A HINT, not a fact: since #187 the
    /// snapshot is authoritative for this map, so what lands here survives only
    /// until the next accepted snapshot — which re-asserts it iff the
    /// registration's (best-effort) store write also landed. That is what buys
    /// the map a removal path; the bridging value still matters, because it is
    /// what binds the interval before the store's echo comes back.
    ///
    /// The bridge holds only UNCONDITIONALLY absent a snapshot in flight. A
    /// snapshot generated before this registration but sequence-numbered after
    /// our current position is still accepted (§5 gates on `seq`, not on wall
    /// time) and retires the announced pane on arrival — and nothing schedules
    /// a replacement, because the registration itself pushes no snapshot (see
    /// `spawn.rs`, "No push is needed"). The map is then whatever the NEXT
    /// store write happens to carry.
    pub fn register(&mut self, uuid: String, pane_id: u32) {
        self.uuid_to_pane.insert(uuid, pane_id);
        self.prune_opening();
    }

    /// Apply a full-replace snapshot (§5). Returns rename effects (label
    /// changes → real-tab renames) — main.rs gates their EXECUTION to the
    /// active-tab instance, but the guard bookkeeping must run everywhere so
    /// all instances agree on what has been renamed.
    pub fn apply_snapshot(&mut self, snap: AgentSnapshot) -> Vec<Effect> {
        if snap.seq <= self.seq {
            return Vec::new(); // stale/out-of-order: discard (S1)
        }
        self.seq = snap.seq;
        // The mode below is now authoritative, so a switch may be booked (D37).
        self.awaiting_hydration = false;
        self.agents = snap.agents;
        // Hydrate the pane mapping from the snapshot (#178). `clave-register`
        // is a broadcast, so it reaches only the instances alive when it fires
        // — a tab born by a wake never hears about its OWN pane, while the
        // instances that did hear are background ones that cannot see a pane
        // outside their tab. Seeding here is what makes the bind computable in
        // the one instance that can act on it, on this snapshot or any later
        // one; it is self-healing rather than timing-dependent.
        //
        // REPLACE, not merge (#187). A merge has no removal path at all, so a
        // pane learned from the broadcast outlived the pane itself: entries only
        // ever accumulated for the life of the instance. A dead entry is not
        // inert — it fakes a `PaneKnownButAbsent` episode in the #184 breadcrumb
        // (masking a real one) and lets a uuid-directed nav aim at a pane that
        // is gone. Replacing makes the store the single source of truth: a row
        // that carries no pane RETIRES the cached mapping, and the store now
        // clears `pane_id` on tab close and on session recreate alike (#185).
        //
        // ACCEPTED DEGRADATION: `register_pane` persists best-effort, because it
        // runs immediately before the exec into Claude and must never block or
        // fail it. So if that store write fails while the in-process broadcast
        // lands, this replace drops the live mapping at the next snapshot and
        // the row CANNOT bind until the agent is reopened — a running agent
        // never registers a second time, so there is no later announcement to
        // recover from. Nor is it observed: the #184 bind-stall breadcrumb, the
        // one instrument built to catch a stuck bind leg, reads the retirement
        // as the stall ending and reports `Cleared`. That is the price of having
        // a removal path at all — silent, not self-healing — and it is accepted
        // deliberately.
        self.uuid_to_pane = self
            .agents
            .iter()
            .filter_map(|a| a.pane_id.map(|pane| (a.uuid.clone(), pane)))
            .collect();
        // REPLACE the tab order — the store's map is authoritative and
        // self-healing by construction; merging deltas is the exact failure
        // mode that diverged live (C5 round 5).
        self.tab_order = snap.tab_order;
        // Same doctrine for the ordering mode, the host's day, and the
        // tab-keyed bucket twin: the store is the one writer, REPLACE.
        self.order = snap.order;
        self.today = snap.today;
        self.tab_buckets = snap.tab_buckets;
        self.tab_touched = snap.tab_touched;
        // Settle death claims: a witnessed-dead id the store no longer
        // references has had its prune land (or never needed one) — the claim
        // is spent. Settling at the ECHO, not at emit, is what keeps the
        // prune retryable under a false executor gate; dropping the claim
        // HERE is what stops it outliving its purpose and reading a REUSED
        // id's newborn as the corpse it once witnessed (Codex P1, PR #202).
        // An id still referenced keeps its claim: the prune has not landed
        // yet, and the next coherent settle re-derives it (detection-driven).
        self.witnessed_dead.retain(|id| {
            self.agents.iter().any(|a| a.tab_id == Some(*id)) || self.tab_order.contains_key(id)
        });
        let mut effects = Vec::new();
        // Collapse parity heal (issue #5, C8 parity-desync): once
        // seq-accepted, the store's flag is authoritative for any instance
        // with no write in flight — this is what rescues a bar born after
        // the toggle, reborn by a reload, or one that missed the broadcast.
        // ON CHANGE ONLY: switching per snapshot would be a perpetual relayout
        // (round 11), so an in-sync instance's width state is left
        // byte-untouched.
        //
        // The pending-write ledger (fix-review MAJOR) refines that
        // authority: while we OWE the store a value, a snapshot carrying it
        // is our write (or a peer's equal one) confirming — clear the debt,
        // heal nothing. A snapshot CONTRADICTING the debt means our write
        // was swallowed by an out-of-order sibling (two rapid toggles: the
        // late-arriving stale value re-wrote the store) — keep USER truth
        // and re-assert, exactly once per press. Further contradictions while
        // the debt stands change nothing at all — and THAT is what bounds the
        // writes (#137: yielding to them let a ten-press burst buy ~26 store
        // writes, each one broadcasting another contradicting snapshot).
        // Accepted transient (unchanged): an unrelated
        // push between broadcast and write-landing briefly disagrees; the
        // write's own push heals it.
        match self.pending_collapse {
            Some(want) if snap.collapsed == want => {
                self.pending_collapse = None; // debt settled; local == want already
            }
            Some(want) if !self.collapse_reasserted => {
                self.collapse_reasserted = true;
                effects.push(Effect::PersistCollapse { collapsed: want });
            }
            // #137: while we still OWE the store a write, its flag is not
            // evidence about the user's intent — it is evidence about how far
            // behind the store is. The old arm gave up here and healed, which
            // is what let a snapshot one burst old overrule a press the user
            // had just made; combined with the per-press re-assert reset, that
            // was the flip-flop the storm rode on.
            //
            // Stated trade: an instance whose write AND its one re-assert are
            // both swallowed now keeps its own mode until its next press or a
            // reload, where before it would eventually converge on the store.
            // Collapse is a display mode that every instance flips on the same
            // broadcast, so the cost is one bar briefly disagreeing about its
            // own width — against a fleet-killing resize loop, which is what
            // the give-up arm was buying.
            Some(_) => {}
            None => self.heal_collapse(snap.collapsed),
        }
        // Borrow-friendly pass: snapshot views first, then mutate the guards.
        let views: Vec<(String, Status, String, Option<usize>)> = self
            .agents
            .iter()
            .map(|a| (a.uuid.clone(), a.status, a.label.clone(), a.tab_id))
            .collect();
        for (uuid, status, label, tab_id) in views {
            // Rename through the SNAPSHOT bind (§6.6 Design B) — the local
            // manifest join diverges per instance (round 6).
            if let Some(tab_id) = tab_id {
                // Rename on label CHANGE only (vs what WE last wrote).
                if self.renamed.get(&uuid) != Some(&label) {
                    self.renamed.insert(uuid.clone(), label.clone());
                    effects.push(Effect::RenameTab {
                        tab_id,
                        name: label,
                    });
                }
            }
            // Any authoritative non-Done status clears the local override.
            if status != Status::Done {
                self.read_locally.remove(&uuid);
            }
        }
        self.prune_opening(); // stale=true clears ↻ → ✗; new binds clear it
        effects
    }

    /// Apply zellij's tab truth (row SET only — order moves via visit(), the
    /// §6.5 unread clear is the one action keyed on the active tab here).
    pub fn apply_tabs(&mut self, tabs: Vec<TabMeta>) -> Vec<Effect> {
        // Witness deaths BEFORE replacing the frame: an id in the outgoing
        // frame and not the incoming one was SEEN to die, and that witness is
        // the only licence `prune_effect` accepts. An id back alive voids any
        // standing claim on it — witnessed reborn (id reuse), not stale.
        let incoming: BTreeSet<usize> = tabs.iter().map(|t| t.tab_id).collect();
        // Never witness against an EMPTY frame: closing the last tab closes
        // the session, so it is a degenerate update, not a mass death.
        if !incoming.is_empty() {
            self.witnessed_dead.extend(
                self.tabs
                    .iter()
                    .map(|t| t.tab_id)
                    .filter(|id| !incoming.contains(id)),
            );
            self.witnessed_dead.retain(|id| !incoming.contains(id));
        }
        self.tabs = tabs;
        let mut effects = Vec::new();
        // #23 (2026-07-21): a tab CLOSE (`Alt+w`; `Ctrl+D` closes a plain shell
        // tab but never an agent pane, FOOTGUNS.md) can STRAND the nav beacon —
        // current_tab still names the closed tab, so executor election (which
        // wants current_tab == some instance's own live tab, `nav_executor`)
        // matches nobody and dir-nav goes dead until a mouse click reseeds it.
        // The beacon is stranded exactly when it points outside the live set.
        //
        // Derived HERE and nowhere else: `self.tabs` was assigned one line
        // above, so this verdict is witnessed by a frame zellij just delivered.
        // Re-derived on demand it would be worthless to a starved instance,
        // whose frozen set makes any tab born since its last frame look dead
        // (FOOTGUNS.md:63) — see `reanchor_owed`.
        let stranded = self.beacon_stranded();
        // Bounded beacon announce (rounds 11–12). Two DISTINCT triggers, on
        // purpose:
        //   birth → AnnounceVisit (UNGATED): a newborn announces its own tab
        //     once, before its first PaneUpdate can satisfy the active gate —
        //     live-validated ungated; left byte-identical.
        //   organic (Alt+o) / stranded (#23) → ReanchorVisit (GATED HERE, at
        //     emit time, to the active instance — #162 moved the election out
        //     of run_effects into apply_tabs itself; see below). Neither may
        //     ride the ungated path: TabUpdate normally reaches only the active
        //     instance (C3), BUT a TOGGLE delivers the FRESH set to ALL
        //     instances (doc:371-394) — and the organic pipe is a broadcast,
        //     so every hidden bar arrives here armed. Ungated, each fired its
        //     own `zellij pipe` subprocess, and every CLI pipe blocks the
        //     server router ~1s (#45): four tabs froze nav for ~2s per Alt+o
        //     (#128 live check, 2026-08-02 — the storm was latent while the
        //     organic pipe was dropped for its missing payload, and the old
        //     "live-validated ungated" claim here predates that guard).
        //     Gating pipes it once; the toggle's fresh set is exactly the
        //     input the gate needs, so the C3 stale-claim poison isn't in
        //     play.
        //
        // #162 — the election is HERE, not in run_effects, and every trigger is
        // consumed only when it actually emits. The gate used to live in the
        // adapter while the local `current_tab` moved unconditionally, so a
        // refused re-anchor cleared `stranded` anyway: with the announcing bar
        // dead (it was the tab that closed) no trigger was left and nav was
        // dead for the rest of the session. Leaving the beacon stranded is what
        // makes the NEXT tab frame re-derive and re-emit — the same
        // detection-driven retry the prune below relies on, and the retry
        // trigger the adapter's "do not tighten an emit-time latch" warning
        // asks for. The gate stays `elects_presumed` (the pre-#55 position
        // join), so the emit decision is byte-identical to the one run_effects
        // used to make; tightening it to `elects_confirmed` is now SAFE for the
        // first time, and deliberately not done here.
        //
        // Bounded still: birth fires once per instance, organic is one-shot per
        // Alt+o, and a re-anchor that emits also moves the beacon — so a
        // burst-tripped hidden bar that is not the active one emits nothing and
        // simply keeps looking stranded until the real active bar's beacon
        // arrives.
        let birth = !self.birth_announced;
        let organic = self.organic_pending;
        if let Some(active_id) = self.tabs.iter().find(|t| t.active).map(|t| t.tab_id) {
            if self.current_tab == Some(active_id) {
                // The beacon ALREADY names the active tab: both claims are
                // satisfied, not lost, so spend them. Retaining an ungated
                // birth announce here would let every instance fire one at a
                // later burst — the round-11 storm shape.
                self.birth_announced = true;
                self.organic_pending = false;
            } else if birth {
                // UNGATED (live-validated): a newborn must announce its own
                // tab before its first PaneUpdate can satisfy any gate. The
                // birth announce carries everything an armed organic wanted.
                self.birth_announced = true;
                self.organic_pending = false;
                self.current_tab = Some(active_id);
                effects.push(Effect::AnnounceVisit { tab_id: active_id });
            } else if (organic || stranded) && self.elects_presumed() {
                self.organic_pending = false;
                self.current_tab = Some(active_id);
                effects.push(Effect::ReanchorVisit { tab_id: active_id });
            }
        }
        // The debt `apply_panes` pays (#162). Re-derived rather than copied
        // from `stranded` because the branches above may have MOVED the beacon
        // onto this frame's active tab: what is recorded is what this frame
        // leaves OWING, not what it found. So the close's survivor that could
        // not send its re-anchor still owes one, the one that did send it owes
        // nothing, and a frame carrying a live beacon clears the debt.
        self.reanchor_owed = self.beacon_stranded();
        if let Some(now_active) = self.tabs.iter().find(|t| t.active) {
            // §6.5 unread clear — checked on EVERY TabUpdate, NOT on a
            // prev!=now transition: zellij delivers TabUpdate only to the
            // instance in the active tab (C3 live finding, 2026-07-06), so a
            // hidden instance never observes a transition; receiving an
            // update with our tab active IS the focus signal. Exactly-once
            // comes from read_locally (reset by any non-Done snapshot) plus
            // the delivery rule itself (hidden instances get no TabUpdate).
            // Joined via the snapshot bind (§6.6 Design B).
            if let Some(a) = self.agent_in_tab(now_active.tab_id)
                && a.status == Status::Done
            {
                let uuid = a.uuid.clone();
                if self.read_locally.insert(uuid.clone()) {
                    effects.push(Effect::MarkRead { uuid });
                }
            }
        }
        // #6/F3 store hygiene: on ANY TabUpdate, tell the store which OBSERVED
        // ids just died (bound-or-timelined ids absent from this delivered live
        // set) so it drops their binds + tab_order entries. Correctness, not
        // just hygiene — zellij REUSES tab_ids (get_new_tab_id = max-key+1,
        // screen.rs:1617; a closed top tab's id returns on the next new tab), so
        // a survivor entry would let a reused-id tab inherit a dead agent's
        // glyph/order.
        //
        // The payload is the STALE ids, not the live set. Order-safety: two
        // fire-and-forget `clave prune-tabs` have no arrival order (the collapse
        // pending-write class); removing SPECIFIC dead ids is idempotent and
        // commutes, so a late prune can only re-remove ids already judged dead —
        // it never touches a tab it did not observe die. (The full-live-set
        // "retain only these" payload had the opposite property: a late prune
        // would strip the bind of ANY tab created after it was computed → live
        // agent rendered dormant → #6 double-attach via a race.)
        //
        // Emission is DETECTION-driven, NOT gated on a set-change (Codex P2):
        // a close TabUpdate that arrives BEFORE the matching PaneUpdate finds
        // is_active_instance() false (stale plugin_panes), so run_effects drops
        // the prune — a set-change gate would then never re-emit for that same
        // set and the stale bind would make a DEAD agent read LIVE to
        // bound_live_uuids (jump-only, un-resumable) for an unbounded window.
        // Re-deriving staleness every TabUpdate makes retry automatic: the
        // TabUpdate after the PaneUpdate lands re-derives the same stale set and
        // re-emits, now executing. It stays bounded because (a) removals are
        // idempotent so duplicate emissions in the echo window are harmless;
        // (b) the store push echo clears self.agents/self.tab_order, so a clean
        // store self-limits — a steady focus-move TabUpdate derives an EMPTY
        // stale set and spawns nothing; (c) it is TabUpdate-rate, not
        // render-rate — the C5 rd-4 spawn-storm bar was PER-RENDER triggers;
        // (d) run_effects executor-gates it, so hidden instances (incl.
        // burst-delivered fresh sets, doc:371-394) never spawn anything. Honest
        // residual: a PERMANENTLY-failing store write retries at TabUpdate rate
        // — a failure mode that already breaks bind/collapse identically. Never
        // treat an EMPTY live set as "all died": closing the last tab closes the
        // session, so it is a degenerate update, not a real signal.
        self.prune_opening(); // an appeared tab retires its ↻ mark
        effects
    }

    /// The observed-stale tab ids, per the contract documented above. Derived
    /// fresh from the CURRENT tab set every time — never cached, never
    /// set-change-gated.
    ///
    /// Emitted from `identity_effects`, not from `apply_tabs`. It used to be
    /// emitted here and gated to the active instance in the adapter, which was
    /// correct until #55 tightened that gate to require frame coherence: a
    /// close TabUpdate is precisely when the frames disagree, and `apply_tabs`
    /// is reached only BY a TabUpdate, so the drop had no retry. A close at a
    /// position above ours would then never prune at all — the next TabUpdate
    /// arrives when a tab is created, and zellij reuses ids, so by then the
    /// dead id is back in the live set and no longer reads as stale. The dead
    /// agent's bind and tab_order entry survive into the new tab: it inherits
    /// the dead row's glyph and sort order, and the dead row reads live to
    /// `bound_live_uuids`. Emitting from `identity_effects` gives it the same
    /// retry `Touch` gets — the PaneUpdate that restores coherence.
    ///
    /// Rate note: this is now re-derived on every identity settle (both frame
    /// kinds, both snapshot paths) rather than on TabUpdate alone. The bound is
    /// unchanged in kind and still the store echo — a clean store derives an
    /// EMPTY stale set and spawns nothing — but the window between emitting a
    /// prune and its echo landing now admits more re-emissions. Removals are
    /// idempotent and commute, and this fires only when a tab has actually
    /// died, so the cost is a few duplicate subprocesses on a real close, not
    /// the per-render unbounded class of C5 rd 4.
    fn prune_effect(&self) -> Option<Effect> {
        let live: BTreeSet<usize> = self.tabs.iter().map(|t| t.tab_id).collect();
        // Never treat an EMPTY live set as "all died": closing the last tab
        // closes the session, so it is a degenerate update, not a real signal.
        if live.is_empty() {
            return None;
        }
        // "Observed-stale" means WITNESSED: only an id this instance saw
        // leave a delivered tab frame is its to prune — anything else is a
        // newborn beyond a starved frame's reach (2026-08-17 QA drive, P2
        // rung 1), including a REUSED id whose earlier death this instance
        // witnessed and whose claim the store echo has since settled
        // (Codex P1, PR #202). The `witnessed_dead` field doc carries the
        // full claim lifecycle.
        let observed_stale = |id: &usize| !live.contains(id) && self.witnessed_dead.contains(id);
        let mut stale: BTreeSet<usize> = self
            .agents
            .iter()
            .filter_map(|a| a.tab_id)
            .filter(&observed_stale)
            .collect();
        stale.extend(self.tab_order.keys().copied().filter(observed_stale));
        (!stale.is_empty()).then(|| Effect::PruneTabs {
            stale_ids: stale.into_iter().collect(), // BTreeSet → sorted, deduped
        })
    }

    /// Apply zellij's pane truth — and pay any re-anchor this instance owes.
    ///
    /// The pane frame is the SECOND retry trigger for the stranded beacon
    /// (#162), and it is the one that always arrives. A tab close renumbers
    /// positions, so it delivers a TabUpdate and a PaneUpdate; when the tab
    /// frame lands first the election refuses (the manifest still describes the
    /// old set) and `apply_tabs` records the debt. That refusal needed a NEXT
    /// frame, and the tab frame zellij owes us none of — but the pane frame is
    /// already in flight, being the very thing that ends the disagreement. It
    /// is also a precondition either way: `elects_confirmed()` is false until
    /// it lands, so nothing this debt could ever buy was reachable before it.
    ///
    /// Deliberately the STRONG election, unlike `apply_tabs`'s: coherence is
    /// exactly what this frame establishes, so requiring the witness costs
    /// nothing here. And a hidden instance receives no pane frames at all (C3),
    /// so the payment is frame-witnessed in the same sense the debt is.
    pub fn apply_panes(&mut self, panes: Vec<PaneMeta>) -> Vec<Effect> {
        self.panes = panes;
        // A closed pane's facts must not survive it: pane ids are minted
        // monotonically by zellij, but a map that only grows is a leak in a
        // bar that lives for the session. Plugin panes are excluded from the
        // live set — terminal and plugin ids are separate id spaces (the same
        // fact `own_tab_position` is built on), so a plugin pane's id must
        // not keep a dead terminal's facts alive for a reborn pane to inherit.
        let live: BTreeSet<u32> = self
            .panes
            .iter()
            .filter(|p| !p.is_plugin)
            .map(|p| p.pane_id)
            .collect();
        self.pane_facts.retain(|id, _| live.contains(id));
        // Stand-downs decay here, not on a clock: the manifest is what
        // re-arms a probe pass, so a delivered manifest is the one honest
        // unit of "retry skipped" (FOOTGUNS — the running-latch probe
        // retry). Dead panes drop with their facts above.
        self.probe_backoff.retain(|id, n| {
            *n -= 1;
            *n > 0 && live.contains(id)
        });
        // The frame that ends the Alt+f spawn window: from here shell_toggle
        // reads delivered truth again (see `shell_spawn_pending`).
        self.shell_spawn_pending = false;
        self.prune_opening();
        if self.reanchor_owed
            && self.elects_confirmed()
            && let Some(own) = self.own_tab()
        {
            self.reanchor_owed = false;
            // Answered by the send, like every other trigger since #162 — an
            // Alt+o still pending wanted exactly this announce.
            self.organic_pending = false;
            self.current_tab = Some(own);
            return vec![Effect::ReanchorVisit { tab_id: own }];
        }
        Vec::new()
    }

    /// One OS answer about one pane, as main.rs collected it (#206) — from a
    /// `PaneUpdate` probe (both fields asked) or a `CwdChanged`/`CommandChanged`
    /// delta (one field each). `None` means "not asked / no answer", never
    /// "clear what you knew": a failed probe must not blank a row that was
    /// telling the truth a frame ago.
    pub fn apply_pane_facts(&mut self, probes: Vec<PaneProbe>) -> bool {
        let mut changed = false;
        for p in probes {
            // An all-None probe (both OS queries failed — a pane mid-spawn,
            // a process already gone) must not mint an entry: an entry is
            // "known", and known-and-idle panes are never re-probed, so a
            // transient failure at birth would otherwise stick forever.
            if p.cwd.is_none() && p.foreground.is_none() {
                // …and it stands the pane down (see `probe_backoff`): without
                // this, an unanswerable pane re-qualifies on every manifest.
                self.probe_backoff
                    .insert(p.pane_id, PROBE_FAILURE_STANDDOWN);
                continue;
            }
            // An answered probe is the can-succeed proof; drop any stand-down.
            self.probe_backoff.remove(&p.pane_id);
            let f = self.pane_facts.entry(p.pane_id).or_default();
            let before = f.clone();
            if let Some(cwd) = p.cwd {
                f.cwd = Some(cwd);
            }
            if let Some(argv) = p.foreground {
                match command_display(&argv) {
                    Some(cmd) => {
                        f.last_cmd = Some(cmd);
                        f.running = true;
                    }
                    // The shell itself: the prompt is idle, and the last
                    // command LINGERS — that is what "most recently run" means.
                    None => f.running = false,
                }
            }
            changed |= *f != before;
        }
        changed
    }

    /// Whether the term-facts poll should be armed (#206): some pane the bar
    /// is tracking claims a running foreground command. zellij never pushes
    /// the exit-side delta, so "running" is exactly the state that needs a
    /// second look. Scoped to `probe_targets`, not the whole facts map: only
    /// a pane the next probe will actually visit can clear its own running
    /// flag, and a pane that stopped being its tab's speaker (focus moved to
    /// a sibling) would otherwise arm a 3s timer nothing can ever satisfy —
    /// the unreachable-success retry loop FOOTGUNS.md records for exited
    /// panes, reached by a second route. Its stale flag is harmless in
    /// itself: only the speaker's facts reach a row, and speakership coming
    /// back re-lists the pane here.
    pub fn term_poll_wanted(&self) -> bool {
        // Membership alone is not enough — `probe_targets` also lists
        // never-probed panes, which are not awaiting an exit.
        self.probe_targets()
            .iter()
            .any(|id| self.pane_facts.get(id).is_some_and(|f| f.running))
    }

    /// The panes main.rs should ask the OS about after this manifest (#206):
    /// each terminal tab's speaker. Agent tabs are excluded — their row is the
    /// store's, and their panes' cwds are already store truth.
    ///
    /// A speaker already known and idle is not listed at all (first sandbox
    /// drive, 2026-08-18, nav lag): the steady state is ZERO probes per
    /// manifest, with freshness between probes the events' and the
    /// while-running poll's job. The companion gate — only the VISIBLE bar
    /// probes — lives at the call site on `own_tab_focused`, because tests
    /// drive this listing without establishing an identity.
    pub fn probe_targets(&self) -> Vec<u32> {
        // Before hydration the agent list is empty, so the filter below would
        // read EVERY tab as a terminal — the newborn visible bar then
        // serially interrogated the whole session, blocking round-trips
        // queuing ahead of the very snapshot result that shrinks this list
        // (live capture 2026-08-26 09:44: five full passes in 3.1s while the
        // bar rendered all-TERM). A bar that cannot yet tell agents from
        // terminals asks the OS about no one; either snapshot path clears
        // the flag, and the next store write heals a failed hydrate.
        if self.awaiting_hydration {
            return Vec::new();
        }
        self.tabs
            .iter()
            .filter(|t| self.agent_in_tab(t.tab_id).is_none())
            .filter_map(|t| self.speaker_pane(t.position))
            // An EXITED pane is never probed: both OS queries fail on a dead
            // process, the all-None guard mints no entry, and the pane
            // re-qualified as unknown on every manifest — two failing
            // round-trips per press, forever (the residual nav lag,
            // 2026-08-18). Its status and command are manifest truth anyway.
            .filter(|p| !p.exited)
            .map(|p| p.pane_id)
            .filter(|id| self.pane_facts.get(id).is_none_or(|f| f.running))
            // A stood-down pane sits out its remaining manifests (see
            // `probe_backoff`); this also silences `term_poll_wanted`, which
            // derives from this listing.
            .filter(|id| !self.probe_backoff.contains_key(id))
            .collect()
    }

    /// The pane that speaks for a terminal tab's row (#206, ratified): the
    /// tab's focused tiled terminal, falling back to its first. Plugin panes
    /// never speak (the bar itself lives in one), and floating panes don't
    /// either — the row describes the tab's resident content, not the scratch
    /// shell hovering over it.
    fn speaker_pane(&self, position: usize) -> Option<&PaneMeta> {
        let resident = |p: &&PaneMeta| p.tab_position == position && !p.is_plugin && !p.is_floating;
        self.panes
            .iter()
            .find(|p| resident(p) && p.is_focused)
            .or_else(|| self.panes.iter().find(resident))
    }

    /// The row content for a terminal tab (#206): the tab name as the chip,
    /// and everything else read off the speaker pane — status from its
    /// command-pane exit or its foreground facts, location from its cwd
    /// through the fleet's own checkouts, summary from its most recent
    /// foreground command.
    fn terminal_content(&self, t: &TabMeta, inks: &ProvisionalInks) -> RowContent {
        let speaker = self.speaker_pane(t.position);
        let facts = speaker.and_then(|p| self.pane_facts.get(&p.pane_id));
        let status = match speaker {
            // A command pane is the one terminal whose lifecycle is visible:
            // running until it exits, then Done or Failed by exit code. A
            // held pane (rerun pending) reads as its last exit, honestly.
            Some(p) if p.terminal_command.is_some() => {
                if !p.exited {
                    TermStatus::Running
                } else if p.exit_status.unwrap_or(0) == 0 {
                    TermStatus::Done
                } else {
                    TermStatus::Failed
                }
            }
            _ if facts.is_some_and(|f| f.running) => TermStatus::Running,
            _ => TermStatus::Idle,
        };
        // The cwd through the fleet's checkouts (#206, ratified): a prefix
        // match against an agent's checkout reuses that row's repo name, ink
        // and provenance verbatim — correct by construction, no git reading.
        // Unmatched cwds show their directory name untinted, provenance
        // blank: "where it lives" is useful even outside the fleet.
        let (repo, repo_ink, provenance, pr, branch) = match facts.and_then(|f| f.cwd.as_deref()) {
            Some(cwd) => match self.checkout_of(cwd) {
                Some(a) => (
                    Some(basename(&a.repo_root).to_string()),
                    inks.repo.get(&a.repo_root).copied(),
                    provenance_of(a),
                    a.pr_number,
                    // Blanked on the exact predicate `provenance` above just
                    // rendered blank via its `Main` case — shared, not
                    // re-derived (#232), same as `agent_content`'s branch.
                    if is_default_checkout(a) {
                        String::new()
                    } else {
                        a.branch.clone()
                    },
                ),
                None => (
                    Some(basename(cwd).to_string()),
                    None,
                    Provenance::Main,
                    None,
                    String::new(),
                ),
            },
            None => (None, None, Provenance::Main, None, String::new()),
        };
        let command = facts
            .and_then(|f| f.last_cmd.clone())
            .or_else(|| speaker.and_then(|p| p.terminal_command.clone()))
            .unwrap_or_default();
        // Elapsed reads the store's `tab_touched` wall-clock twin, not any
        // agent field — a terminal tab has no agent record of its own. An
        // absent entry defaults to 0, which `elapsed_label` already renders
        // as "never interacted" (#232).
        let elapsed = elapsed_label(
            self.now,
            self.tab_touched.get(&t.tab_id).copied().unwrap_or(0),
        );
        RowContent::Terminal {
            name: t.name.clone(),
            status,
            provenance,
            repo,
            repo_ink,
            command,
            pr,
            branch,
            elapsed,
        }
    }

    /// The agent whose checkout contains `cwd`, if any — worktree path when
    /// the agent has one, repo root otherwise. Longest match wins: a worktree
    /// under the main repo's tree must claim its own panes.
    fn checkout_of(&self, cwd: &str) -> Option<&Agent> {
        self.agents
            .iter()
            .filter_map(|a| {
                let root = a.worktree.as_deref().unwrap_or(&a.repo_root);
                (!root.is_empty()
                    && (cwd == root || cwd.strip_prefix(root).is_some_and(|r| r.starts_with('/'))))
                .then_some((root.len(), a))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, a)| a)
    }

    /// The row content for a live-or-dormant agent (lock §2). `dormant` and
    /// `selected` are the caller's own classification rather than a
    /// re-derivation, because the two loops below already know which list
    /// they are walking and where the cursor sits.
    fn agent_content(
        &self,
        a: &Agent,
        dormant: bool,
        selected: bool,
        inks: &ProvisionalInks,
    ) -> RowContent {
        // Ordered, and the order is the behaviour: `stale` and `opening` are
        // model states that OUTRANK the store's `Status` (a stale row's status
        // is whatever it was when the cwd vanished), and the local unread
        // override is the last thing between `Done` and the palette. The
        // #100 tier — stale ✗ > opening ↻ > selected-dormant ⏎ > status —
        // is self-truthful: a stale row shows ✗ and never offers a launch
        // that would fail, and committing turns ⏎ into the ↻ already worn
        // while a session spins up.
        let status = if a.stale {
            RowStatus::Stale
        } else if self.opening.contains(&a.uuid) {
            RowStatus::Opening
        } else if dormant && selected {
            RowStatus::DormantSelected
        } else if dormant {
            RowStatus::Dormant
        } else if a.status == Status::Done && self.read_locally.contains(&a.uuid) {
            // Local unread override: a Done agent already seen renders Idle
            // until `clave focus` persists the transition (§6.5).
            RowStatus::Idle
        } else {
            match a.status {
                Status::Idle => RowStatus::Idle,
                Status::Working => RowStatus::Working,
                Status::NeedsYou => RowStatus::NeedsYou,
                Status::Done => RowStatus::Done,
                Status::Failed => RowStatus::Failed,
            }
        };
        let provenance = provenance_of(a);
        let title = a.title.clone();
        RowContent::Agent {
            status,
            // S7 (#62). Bucketed host-side, where the smart zone is readable;
            // the bar renders an index and holds no opinion about tokens. `None`
            // is still a real answer — no reading yet — and renders a blank
            // cell, which lock §2.1 requires it to do cleanly: inventing a level
            // would be a lie in the one cell whose whole job is a measurement.
            battery: a.context_level,
            // The raw count rides alongside the level for the expanded profile,
            // which renders it as text (#105) — eleven glyphs can only
            // approximate it. Bucketing stays host-side; the bar picks the ramp's
            // INK by level and prints the figure it was bucketed from.
            tokens: a.context_tokens,
            provenance,
            title_ink: title
                .as_ref()
                .and_then(|t| inks.title.get(&(a.repo_root.clone(), t.clone())).copied()),
            title,
            repo: basename(&a.repo_root).to_string(),
            repo_ink: inks.repo.get(&a.repo_root).copied(),
            summary: a.summary.clone(),
            model: a.model.clone(),
            provider: a.provider.clone(),
            pr: a.pr_number,
            // Blanked, not `a.branch.clone()`, on the exact predicate
            // `provenance` above just rendered blank via its `Main` case —
            // shared, not re-derived (#232).
            branch: if is_default_checkout(a) {
                String::new()
            } else {
                a.branch.clone()
            },
            elapsed: elapsed_label(self.now, a.last_interacted),
        }
    }

    /// The width profile this bar renders at. Chosen by STATE, never by the
    /// current `cols` (LEDGER D16): a peeking bar is still `collapsed`, but the
    /// peek is showing the template, so it renders EXPANDED — the same rule
    /// the geometry switch is judged against, deliberately expressed once so the
    /// profile and the target it is seeking cannot drift apart mid-animation.
    pub fn widths(&self) -> Widths {
        if self.showing_collapsed() {
            Widths::COLLAPSED
        } else {
            Widths::EXPANDED
        }
    }

    /// The width profile for one PAINT. Identical to [`Self::widths`] except
    /// during the D37 hydration window, where the mode is still `Default`'s
    /// guess while the painted width is store truth (the launch layout is
    /// composed from `store.collapsed`) — so the paint picks the profile. A
    /// collapsed cold start otherwise drew the expanded profile squeezed
    /// into the 30-col pane for the ~200ms before the snapshot arrived (QA
    /// drive, 2026-08-17): the geometry never moved, but the ink flashed.
    /// This carves NO exception into D16's state-not-cols lock — a hydrated
    /// bar (every peek, every toggle) still chooses by state alone.
    pub fn widths_at(&self, cols: usize) -> Widths {
        if self.awaiting_hydration && cols == self.row_height.target_cols(true) {
            return Widths::COLLAPSED;
        }
        self.widths()
    }

    /// The one collapse predicate. A peeking bar seeks (and renders) the
    /// template width even though it is collapsed — the collapse resumes when
    /// the peek expires.
    fn showing_collapsed(&self) -> bool {
        self.collapsed && !self.peeking
    }

    /// Rows in display order (§6.6 C8 / S1, as segregated by #112): TWO
    /// contiguous blocks — every live row first, then every dormant row —
    /// each ordered by the commitment ORDINAL descending, live tabs keyed by
    /// the store's tab order and dormant store rows by [`Self::dormant_ord`].
    ///
    /// Segregation (#116 §5) overrules S1's R2 from "closing a tab moves
    /// nothing" to "closing a tab reorders nothing RELATIVE to anything
    /// else": the closed row leaves the live block for the head of the
    /// dormant one, and every survivor holds its relative order.
    ///
    /// **The ordinal is untouched by any of this.** Both blocks sort on the
    /// same key by the same comparator, which is what keeps `live_ord` and
    /// `dormant_ord` the one ranking rule for both row classes — the identity
    /// that stops a close from changing a row's rank. Segregation decides
    /// which BLOCK a row renders in, never what number ranks it.
    ///
    /// The tiebreaks below (tab position for live rows, `usize::MAX - i` for
    /// dormant) are no longer the ordering mechanism: ordinals are minted under
    /// the store flock and are unique by construction, so committed rows cannot
    /// tie at all. They survive as a DETERMINISM RESIDUAL for the two shapes
    /// that can still collide (S1 §3.2): rows at [`NO_COMMITMENT`], and the
    /// transient window where an evicted tenant still shares its ordinal with
    /// the tab that replaced it. Splitting the sort narrows their reach —
    /// they now only ever break ties WITHIN a block — but does not retire
    /// them: two dormant rows at `NO_COMMITMENT` still need a stable order.
    pub fn rows(&self) -> Vec<(RowKey, Row)> {
        // §6.6 C8 virtual cursor: `Row.selected` means "visually SELECTED".
        // While nav sits on a dormant row, the selection follows the walk
        // (claude.ai-style) — that dormant row reads active and EVERY live
        // row drops its highlight, so the stale previous-tab highlight can't
        // linger and mislead. Stale-cursor self-heal (documents review minor
        // #7): a dwell-opened row goes LIVE, so this dormant-key lookup
        // MISSES; selection falls back to the focused tab (tab.active) with
        // no explicit cursor clear needed.
        let selected_dormant: Option<&str> = self.cursor.as_deref().filter(|u| {
            self.agents
                .iter()
                .any(|a| &a.uuid == u && self.is_dormant(a))
        });
        // (ordinal desc, tiebreak asc), applied to each block separately —
        // tiebreak: live rows by position, dormant by a large offset + stable
        // index. The dormant offset is now only ever compared against other
        // dormant rows, but it costs nothing and keeps the two keys readable
        // as the single scheme they are.
        let inks = ProvisionalInks::allocate(&self.agents);
        let mut live: Vec<Ranked> = Vec::new();
        for t in &self.tabs {
            // Lock §7.1: the zellij tab name is used ONLY for a terminal tab.
            // An agent row's identity is its title chip and repo, both from the
            // store — the tab name is clave's own rename echo and would be a
            // second, drifting copy of the label.
            let content = match self.agent_in_tab(t.tab_id) {
                Some(a) => self.agent_content(a, false, false, &inks),
                None => self.terminal_content(t, &inks),
            };
            live.push((
                self.live_key(t),
                t.position,
                (
                    RowKey::Tab(t.tab_id),
                    Row {
                        content,
                        // A dormant selection steals the highlight from every tab.
                        selected: selected_dormant.is_none() && t.active,
                        dormant: false,
                    },
                ),
            ));
        }
        let mut agents: Vec<&Agent> = self.agents.iter().filter(|a| self.is_dormant(a)).collect();
        agents.sort_by(|a, b| a.uuid.cmp(&b.uuid)); // stable tiebreak input
        let mut dormant: Vec<Ranked> = Vec::new();
        for (i, a) in agents.into_iter().enumerate() {
            dormant.push((
                self.dormant_key(a),
                // Among dormant rows sharing an ordinal this renders
                // uuid-DESCENDING (uuid-asc sort, key inverted) — stable and
                // deterministic, which is all we need.
                usize::MAX - i,
                (
                    RowKey::Dormant(a.uuid.clone()),
                    Row {
                        content: self.agent_content(
                            a,
                            true,
                            selected_dormant == Some(a.uuid.as_str()),
                            &inks,
                        ),
                        selected: selected_dormant == Some(a.uuid.as_str()),
                        // Block membership for the renderer's #206 fade —
                        // carried on the Row because stale/opening outrank
                        // Dormant in the status tier above.
                        dormant: true,
                    },
                ),
            ));
        }
        // ONE comparator, applied twice. Sorting the blocks separately is the
        // whole of segregation; giving either block its own rule would be the
        // defect two reviewers caught on PR #135, arriving by a third route.
        live.sort_by(rank_desc);
        dormant.sort_by(rank_desc);
        live.into_iter().chain(dormant).map(|(_, _, r)| r).collect()
    }

    /// How many leading rows form the LIVE block (#112). [`Self::rows`] emits
    /// every live row before every dormant one, so the live block is exactly
    /// the leading run of [`RowKey::Tab`] — derived from the rendered list
    /// rather than from `self.tabs`, so the nav ring cannot drift out of step
    /// with what the user is actually looking at.
    fn live_block_len(rows: &[(RowKey, Row)]) -> usize {
        rows.iter()
            .take_while(|(k, _)| matches!(k, RowKey::Tab(_)))
            .count()
    }

    /// Mouse click on rendered line N of a pane `pane_height` lines tall
    /// (0-based, counted from the TOP OF THE SCREEN, not the top of the list —
    /// see #148 below): jump to that row's tab. Both arguments are TERMINAL
    /// LINES, as zellij reports them; a row is one or two of those depending
    /// on the geometry (#232), and this function is where they convert.
    /// A click reaches exactly ONE instance (the visible bar), so the jump
    /// broadcasts a beacon for the other instances' executor election.
    /// Focus is not a commitment — clicks never reorder. A click on a
    /// dormant row SELECTS it (#100): with the mouse as the main path to
    /// dormant rows past Alt+9, a click that launched would move the
    /// accidental-spawn problem into the mouse channel — only Alt+Enter
    /// wakes a dormant row.
    pub fn click(&mut self, line: usize, pane_height: usize) -> Vec<Effect> {
        // #148: `line` is a line of the VIEWPORT — it counts from the first row
        // on screen, not from row 0 of the list. It reads the offset off the
        // SAME `viewport_top` the renderer draws with, because the 2026-08-06
        // overflow had two symptoms (invisible rows, and clicks landing one or
        // two rows above the pointer) and a second copy of this arithmetic
        // would let them come apart again.
        // #232: both arguments cross from TERMINAL LINES into ROWS here, at
        // the boundary, and `viewport_top` stays row-unit for both callers.
        // Converting one and not the other is exactly the #148 divergence —
        // a click would land the right row against the wrong window.
        // Integer division is also the odd-remainder rule: a click on a
        // half-drawn last line (which the renderer never writes) folds onto
        // the last whole card rather than off the end.
        let lines_per_row = self.row_height.lines_per_row();
        let raw_line = line;
        let line = line / lines_per_row;
        let rows = self.rows();
        let top = viewport_top(
            rows.len(),
            rows.iter().position(|(_, r)| r.selected),
            pane_height / lines_per_row,
        );
        // `rows.get` bounds-checks against the LIST, not the window: a `line`
        // at or past `pane_height` (reachable only in the one-frame-stale
        // `pane_height` window below) selects a row off screen harmlessly,
        // rather than panicking.
        let hit = top.checked_add(line).and_then(|i| rows.get(i)).cloned();
        // UNCONDITIONAL (#232, the maintainer's ask): clicks are rare, and
        // this one line is what debugs the next #148 — every term of the
        // conversion, plus the row it resolved to, in the order they apply.
        eprintln!(
            "clave-bar click: raw_line={raw_line} lines_per_row={lines_per_row} row_line={line} top={top} -> key={:?}",
            hit.as_ref().map(|(k, _)| k)
        );
        let Some((key, _)) = hit else {
            return Vec::new();
        };
        self.cursor_gen += 1; // a click is a landing like any other
        match key {
            RowKey::Tab(tab_id) => {
                let Some(position) = self
                    .tabs
                    .iter()
                    .find(|t| t.tab_id == tab_id)
                    .map(|t| t.position)
                else {
                    return Vec::new();
                };
                self.beacon(tab_id); // clears any dormant selection too
                vec![
                    Effect::SwitchTab { position },
                    Effect::AnnounceVisit { tab_id },
                ]
            }
            RowKey::Dormant(uuid) => {
                self.cursor = Some(uuid);
                Vec::new()
            }
        }
    }

    /// The replicated focus truth — the beacon the nav election reads.
    pub fn current_tab(&self) -> Option<usize> {
        self.current_tab
    }

    /// The `clave-nav` executor: `Some(own tab)` on the ONE instance allowed to
    /// act on a nav press, `None` everywhere else. Row jumps and dir walks need
    /// a FRESH tab set, and only the active instance has one (C3).
    ///
    /// Normally the instance whose own tab IS the replicated beacon — the
    /// channel that cannot be stale, which is why local active flags were
    /// abandoned (C5 round 2 raced six divergent SwitchTab targets).
    /// `own_tab()` is None while the two zellij frames disagree, so a press
    /// inside the renumbering window resolves no position off a mismatched
    /// pair (#55).
    ///
    /// The rule is the beacon and NOTHING else, which is what makes it
    /// exclusive: exactly one live tab can be the one the last broadcast named,
    /// so exactly one instance can answer. Every local signal is fakeable by a
    /// frozen instance — its own frame pair is self-coherent and claims its own
    /// tab active (FOOTGUNS.md:64) — and a beacon is not, because an instance
    /// cannot deliver itself one.
    ///
    /// A stranded beacon (a close killed the tab it named) elects nobody, and
    /// that is #162: nav died for the session because the re-anchor that should
    /// have re-seeded the beacon had no retry trigger. The fix is to re-seed
    /// it — `apply_tabs` and `apply_panes` between them re-emit on the next
    /// frame of either kind — and NOT to let the refusing instance nav on local
    /// truth meanwhile. That fallback was written, and it is unsound: its
    /// licence is armed by a frame and can only be spent after a LATER frame
    /// restores coherence, so it necessarily outlives the focus that earned it.
    /// A beaconless native tab switch (mouse on zellij's tab bar) then leaves
    /// it armed on a bar that is no longer active while the newly active bar
    /// arms one the same way — two executors, two SwitchTab targets, C5 round 2
    /// (test: `a_beaconless_focus_change_never_leaves_two_nav_executors`).
    ///
    /// Known and accepted: after any beaconless switch the beacon still names
    /// the tab the user left, so the first press walks from there and re-anchors
    /// on landing. One stale press, one executor — the fail-closed trade this
    /// whole subsystem takes, since a repeated keypress costs less than a jump
    /// to the wrong tab. That trade only holds if the press LANDS somewhere:
    /// a position-addressed switch from a starved executor can be silently
    /// refused (out of range after a close) and heals nothing, which is why
    /// live picks now land by pane id where one is registered (the QA-drive
    /// phase-3 wedge; FOOTGUNS.md).
    pub fn nav_executor(&self) -> Option<usize> {
        let own = self.own_tab()?;
        (self.current_tab == Some(own)).then_some(own)
    }

    /// Is this bar's own tab the one the user is looking at? Same rule as
    /// `nav_executor` and for the same reason — the replicated beacon is the
    /// only focus signal a background instance cannot fake (a frozen instance's
    /// own tab frame claims itself active, FOOTGUNS.md) — but the answer here is
    /// about permission to touch the FOCUSED tab, not about electing anybody.
    /// Pub since #206: the term-facts probe is gated on it — five hidden
    /// bars each firing synchronous OS round-trips per manifest made nav
    /// drag on the first sandbox drive (2026-08-18); only the bar being
    /// LOOKED AT pays for OS truth, and a hidden bar converges the moment it
    /// is seen.
    pub fn own_tab_focused(&self) -> bool {
        self.own_tab()
            .is_some_and(|own| self.current_tab == Some(own))
    }

    /// Alt+f (#207): decide what the press means for the ACTIVE tab — spawn
    /// the scratch shell, show it, or hide it — and decide it HERE, where a
    /// test can reach it (#162 template). The keybind's `MessagePlugin`
    /// broadcasts to every instance, and only the spawn arm is dangerous in
    /// duplicate (N emitters, N shells), so the whole decision runs on the
    /// beacon-named instance alone — `own_tab_focused`, the same election as
    /// `clave-nav`, because a starved bar's own frames claim it is active
    /// (FOOTGUNS.md:63-65) and only the replicated beacon refuses it.
    /// Fail-closed: a dropped Alt+f is a repeatable keypress.
    pub fn shell_toggle(&mut self) -> Vec<Effect> {
        if !self.own_tab_focused() {
            return Vec::new();
        }
        // A spawn is in flight: every input below is pre-spawn state, so any
        // decision would be wrong (worst: another spawn). Drop the press.
        if self.shell_spawn_pending {
            return Vec::new();
        }
        // Under the gate, own tab == the tab the user is looking at, and the
        // frames are coherent — both lookups below resolve against the same
        // delivered pair.
        if self.own_tab_floating_visible() {
            return vec![Effect::ShellHide];
        }
        let has_floating = self.own_tab_position().is_some_and(|pos| {
            self.panes
                .iter()
                .any(|p| p.tab_position == pos && p.is_floating)
        });
        if has_floating {
            vec![Effect::ShellShow]
        } else {
            self.shell_spawn_pending = true;
            vec![Effect::ShellSpawn {
                cwd: self.shell_spawn_cwd(),
            }]
        }
    }

    /// Whether the bar's own tab currently shows its floating set (#207,
    /// #208). Meaningful only under the focus gate, where the delivered
    /// frame is the one the user is looking at.
    fn own_tab_floating_visible(&self) -> bool {
        self.own_tab().is_some_and(|own| {
            self.tabs
                .iter()
                .any(|t| t.tab_id == own && t.floating_visible)
        })
    }

    /// Where the Alt+f shell should open (#215): the active tab's location,
    /// by the same rules its row reads — an agent tab's cwd is store truth
    /// (agent tabs are never probed, #206), a terminal tab's is its speaker
    /// pane's OS-reported cwd. `None` when neither source knows; the adapter
    /// falls back to the bar's own `initial_cwd`, the pre-#215 behaviour.
    /// Ratified trade: a `cd` in an idle shell lands only on the next probe,
    /// so a just-moved speaker may seed the old cwd — accepted, not re-probed.
    fn shell_spawn_cwd(&self) -> Option<String> {
        let own = self.own_tab()?;
        if let Some(a) = self.agent_in_tab(own) {
            return Some(a.cwd.clone()).filter(|c| !c.is_empty());
        }
        let speaker = self.speaker_pane(self.own_tab_position()?)?;
        // Same emptiness filter as the agent branch: an empty-but-successful
        // OS probe must fall back to initial_cwd, not spawn at "".
        self.pane_facts
            .get(&speaker.pane_id)?
            .cwd
            .clone()
            .filter(|c| !c.is_empty())
    }

    /// Does the beacon name a tab that is not in the last delivered tab set?
    /// A beacon that never landed (`None`) is not stranded — it is simply
    /// unset, and birth is the trigger that answers for it.
    fn beacon_stranded(&self) -> bool {
        self.current_tab
            .is_some_and(|id| !self.tabs.iter().any(|t| t.tab_id == id))
    }

    /// clave-nav payloads: {"dir":"next"|"prev"} | {"row":N} | {"uuid":"…"}
    /// | {"commit":true} (Alt+Enter, #100).
    /// uuid → FocusPane on EVERY instance: the pane id is broadcast truth
    /// (clave-register), so duplicates target the same pane.
    /// row (1-based, Alt+1..9) and dir both act on DISPLAY rows and run on
    /// the EXECUTOR only (`executor_own_tab` = Some(own tab) on the active
    /// instance — fresh tab set, and the very bar the user is reading; a
    /// broadcast walk over stale sets raced six divergent targets live).
    /// dir steps ±1 and wraps WITHIN ONE BLOCK (#112) — the live block by
    /// default, the dormant block while a dormant row is selected. Picking a
    /// row is what moves that focus between the blocks; a walk never does.
    /// Safe to walk the visible list, because focus no longer reorders it
    /// (§6.6 revised: only user commitments move rows, so there is no
    /// ping-pong). row (Alt+1-9) indexes the whole rendered list, so it both
    /// reaches the dormant block and hands the walk to it; putting the live
    /// block on top is what keeps the low numbers pointed at the live fleet.
    pub fn nav(&mut self, payload: &str, executor_own_tab: Option<usize>) -> Vec<Effect> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
            return Vec::new();
        };
        if let Some(uuid) = v.get("uuid").and_then(|u| u.as_str()) {
            return match self.uuid_to_pane.get(uuid) {
                Some(&pane_id) => vec![Effect::FocusPane { pane_id }],
                None => Vec::new(),
            };
        }
        let Some(own) = executor_own_tab else {
            return Vec::new();
        };
        // Alt+Enter (#100): commit the dormant selection — the ONLY thing
        // that launches a dormant row. Executor-gated like row/dir (the
        // cursor is executor-local state), a no-op unless a dormant row is
        // selected. Not a landing: the selection is being spent, not moved,
        // so cursor_gen holds. STALE rows refuse the commit (ratified live,
        // 2026-08-01): the ✗ gutter offers no launch, and the mark must not
        // lie — a dead row is #112's retirement business, not a retry's.
        if v.get("commit").and_then(|c| c.as_bool()) == Some(true) {
            let Some(uuid) = self.cursor.clone() else {
                return Vec::new();
            };
            let committable = self
                .agents
                .iter()
                .find(|a| a.uuid == uuid)
                .is_some_and(|a| self.is_dormant(a) && !a.stale);
            if !committable {
                return Vec::new();
            }
            return self.open_effects(&uuid);
        }
        let rows = self.rows();
        let line = if let Some(n) = v.get("row").and_then(|n| n.as_u64()) {
            (n as usize).checked_sub(1) // 1-based → display line
        } else if let Some(dir) = v.get("dir").and_then(|d| d.as_str()) {
            if rows.is_empty() {
                return Vec::new();
            }
            // #112: there are TWO rings, one per block, and the walk never
            // crosses between them. Which block the walk belongs to is a
            // FOCUS, held by the cursor and changed only by an explicit pick
            // (click or Alt+N):
            //
            //   no dormant selection  → the live block, based on the
            //                           executor's own row (focus truth)
            //   a dormant selection   → the dormant block, based on the
            //                           selected row
            //
            // So clicking into the dormant list keeps `Alt+j`/`Alt+k` inside
            // it until you pick a live row again, and a walk can never fall
            // from one block into the other. That matters because on the real
            // store 17 of 21 rows are dormant: one ring over both is mostly
            // rows with no process behind them, and the live fleet is four
            // steps of it.
            //
            // The cursor lookup is by DISPLAYED position, so a selection whose
            // row has gone live (or vanished) silently returns the walk to the
            // live block — the same self-heal `rows()` does for the highlight.
            let live_len = Self::live_block_len(&rows);
            let selected = self.cursor.as_ref().and_then(|u| {
                rows.iter()
                    .position(|(k, _)| *k == RowKey::Dormant(u.clone()))
            });
            // (first line of the block, length of the block, position in it)
            let (base, len, cur) = match selected {
                Some(p) => (live_len, rows.len() - live_len, p),
                // No live rows AND no selection: unreachable in a real session
                // — every zellij tab carries a bar instance, so the tab list is
                // never empty — but the model is pure and must answer. Walk the
                // dormant block from its head rather than going dead; a dormant
                // landing only ever selects (#100), so it cannot spawn.
                None if live_len == 0 => (0, rows.len(), 0),
                None => (
                    0,
                    live_len,
                    rows.iter()
                        .position(|(k, _)| *k == RowKey::Tab(own))
                        .unwrap_or(0),
                ),
            };
            let offset = cur - base;
            match dir {
                "next" => Some(base + (offset + 1) % len),
                "prev" => Some(base + (offset + len - 1) % len),
                _ => None,
            }
        } else {
            None
        };
        let Some((key, _)) = line.and_then(|l| rows.get(l).cloned()) else {
            return Vec::new();
        };
        self.cursor_gen += 1; // every landing invalidates prior dwell arms
        match key {
            RowKey::Tab(tab_id) => {
                self.cursor = None; // live landing: focus truth takes over
                // QA-drive phase-3 wedge (2026-08-17): this executor may be a
                // starved background bar, so `self.tabs` positions can predate
                // a close — a SwitchTab aimed one past the end is silently
                // refused by zellij, and the AnnounceVisit below still
                // broadcasts, re-electing the same stale executor forever.
                // The store's uuid→pane join is broadcast truth and
                // position-immune, so a tab with a registered agent pane lands
                // there; position remains only for tabs with no pane to ride.
                let jump = match self
                    .agent_in_tab(tab_id)
                    .and_then(|a| self.uuid_to_pane.get(&a.uuid))
                {
                    Some(&pane_id) => Effect::FocusPane { pane_id },
                    None => {
                        let Some(position) = self
                            .tabs
                            .iter()
                            .find(|t| t.tab_id == tab_id)
                            .map(|t| t.position)
                        else {
                            return Vec::new();
                        };
                        Effect::SwitchTab { position }
                    }
                };
                self.beacon(tab_id); // executor hand-off hint; pipe echo confirms
                vec![jump, Effect::AnnounceVisit { tab_id }]
            }
            RowKey::Dormant(uuid) => {
                // #100 dwell-commit: EVERY dormant landing — dir walk and
                // Alt+N alike — selects and stops. The ⏎ affordance renders
                // immediately; only Alt+Enter launches. (Alt+N used to open
                // immediately; that just moved the accidental-spawn problem
                // around, so explicit picks demote to selection too.)
                self.cursor = Some(uuid);
                let mut fx = Vec::new();
                // A collapsed bar peeks while sitting on dormant rows too —
                // live nav peeks via the visited pipe; there is no pipe here,
                // so arm locally on the executor (the one visible bar).
                if self.collapsed {
                    self.peeking = true;
                    fx.push(Effect::ArmPeek);
                }
                fx
            }
        }
    }

    /// Alt+c (round 20, collapse-in-place): flip between the template width and
    /// the glyph gutter. The pane is NEVER hidden or suppressed — suppress
    /// proved structurally hostile in zellij 0.44 (lossy re-insert; it marks the
    /// tab damaged, which blocks swap relayouts). Since #181 the flip is a swap
    /// layout switch, so the width itself is zellij's arithmetic and no longer
    /// this model's.
    ///
    /// The snapshot-authoritative flip (issue #5) is the same act on CHANGE
    /// ONLY: an already-in-sync instance is left byte-untouched, because a
    /// per-snapshot re-switch would be a perpetual relayout (round 11).
    fn heal_collapse(&mut self, collapsed: bool) {
        if self.collapsed != collapsed {
            self.collapsed = collapsed;
            self.peeking = false; // authoritative flip outranks a peek
        }
    }

    pub fn toggle(&mut self) -> Vec<Effect> {
        self.collapsed = !self.collapsed;
        self.peeking = false; // an explicit toggle outranks a pending peek
        // Issue #5 durability: record the ABSOLUTE mode we owe the store and
        // emit the persist effect (executor-gated in main.rs — every
        // instance flips + books, exactly one writes). A fresh toggle
        // resets the re-assert budget: it is a new user intent.
        // #137 kept the write count bounded by REFUSING to yield to a
        // contradicting snapshot at all (see `apply_snapshot`), which is what
        // closed the amplification loop: a press books one write, its first
        // contradiction books one repair, every later contradiction books
        // nothing. Two writes per press, whatever the store does.
        //
        // A tighter "one per burst" bound was tried here and withdrawn (Codex P2
        // on PR #152). It used the outstanding debt as the burst boundary — and
        // an unresolved debt never clears, so one burst that ended with the
        // store holding the wrong value made every later press part of that dead
        // burst, and the bar could never write the correction. Nine fewer writes
        // across a ten-press burst is not worth a ledger that can wedge.
        self.pending_collapse = Some(self.collapsed);
        self.collapse_reasserted = false;
        vec![Effect::PersistCollapse {
            collapsed: self.collapsed,
        }]
    }

    /// Mark this model as awaiting its first snapshot — `main.rs` calls it at
    /// `load()`, right after asking for one. See `awaiting_hydration` (D37).
    pub fn await_hydration(&mut self) {
        self.awaiting_hydration = true;
    }

    /// The whole width mechanism (#181; rebuilt on reported truth in #197,
    /// then on PAINTED truth after the 2026-08-17 QA drive).
    ///
    /// One rule: **while this pane is painted at a width other than the one
    /// the store's mode declares, and this bar's own tab is the focused one,
    /// ask for one switch per judged paint — and an ask defers all judgement
    /// to its cooldown expiry.** The declared widths are fixed column
    /// counts ([`clave_types::RowHeight::target_cols`], read through
    /// `self.row_height`) carried verbatim by the layouts and applied
    /// exactly by layout application, so "which geometry
    /// am I in" is one equality against a constant — the same shape as the
    /// battery cell's one-bit read of the mode, pointed at the supply side.
    ///
    /// Two machines died here, for opposite halves of the same lesson. The
    /// pre-#197 machine kept a BELIEF about which geometry its tab was in and
    /// nothing re-derived it, so a rapid double-press desynced a tab
    /// permanently. Its replacement read zellij's answer instead —
    /// `active_swap_layout_name` — which turned out to be a report zellij
    /// only produces for tabs with two or more SELECTABLE panes: every clave
    /// tab (unselectable bar + one workspace pane) reads `None` forever, so
    /// the reported-truth machine was blind in the entire product and every
    /// third toggle lost its press (FOOTGUNS: "no layout for single pane").
    /// The painted width is the one input zellij cannot withhold: it arrives
    /// with every render, and it is the very thing the user sees.
    ///
    /// **The cooldown** (`swap_in_flight` + [`WIDTH_COOLDOWN_SECS`]). A
    /// swap's repaints arrive queued and STALE — a toggle's pane resize
    /// lands renders after its `TabUpdate` (measured 2026-08-15), and the
    /// 2026-08-17 QA drive filmed the consequence: paints echoing the
    /// PREVIOUS width bought three asks inside a millisecond, the three-ask
    /// walk lapped the whole cycle, and the walk's own paint wake re-armed
    /// the next burst — an infinite expand/collapse loop at paint speed.
    /// So an ask buys deafness: paints are recorded (`last_painted`) but not
    /// judged until the cooldown timer fires and
    /// [`Self::width_cooldown_elapsed`] judges the latest width exactly
    /// once. Every ask is thereby judged against the width the previous ask
    /// actually produced, never against its echoes. The first mismatching
    /// paint still asks IMMEDIATELY — only follow-ups wait — so the idle
    /// starvation that killed the stillness gate (FOOTGUNS: zellij never
    /// repaints an idle plugin) cannot recur: the timer, unlike a render, is
    /// always delivered.
    ///
    /// **The walk budget** (`walk_spent`, [`WALK_ASK_CAP`]). A switch that
    /// cannot reach the target must not become a loop: the hidden birth
    /// position doubles the birth geometry's width, a user-damaged tab
    /// spends its next switch re-applying instead of advancing, and a window
    /// too narrow to hold the target never reaches it — its positions can
    /// keep the width CHANGING forever without ever producing the target,
    /// so the budget is per intent and is not re-armed by movement. Three
    /// cooldown-spaced asks provably visit every position even through one
    /// damage re-apply; an intent still unmet after three is one zellij
    /// cannot produce, and the bar rests wherever the walk stopped until the
    /// intent changes (the round-20 "wherever cols stop changing" ruling,
    /// narrowed: a toggle, peek, or refocus re-arms it; a window resize
    /// alone no longer does).
    ///
    /// **The drag arm is GONE, not moved.** Fixed-width panes cannot be
    /// resized in zellij at all, from either side of the border (FOOTGUNS,
    /// C6/C8), so a dragged sidebar border is refused at the source — the
    /// 2026-08-15 snap-back ruling enforced by zellij instead of by a
    /// correction arm. Window resizes cannot drift a fixed pane either, so
    /// there is nothing to concede and nothing to learn: `settled_w`, the
    /// stillness witness and the snap ask all deleted with it.
    ///
    /// **The focus gate,** unchanged. Zellij resolves a plugin's swap-layout
    /// request against the FOCUSED tab, discarding the pane id the request
    /// carries (v0.44.3 — FOOTGUNS.md), and mode changes reach every bar
    /// through the store snapshot — so an ungated switch is a background bar
    /// resizing somebody else's tab. A background bar emits nothing and
    /// writes nothing down: the disagreement is still there when its tab
    /// comes back, and the paint that follows settles it.
    pub fn width_effects(&mut self, own_cols: Option<usize>) -> Vec<Effect> {
        // D37: the mode is not known yet, so any switch would be against a
        // guess. The pane is already born at the persisted mode's width.
        if self.awaiting_hydration {
            return Vec::new();
        }
        if !self.own_tab_focused() {
            self.walk_spent = None;
            return Vec::new();
        }
        // Only render carries a width; a frame with none has nothing to say.
        let Some(cols) = own_cols else {
            return Vec::new();
        };
        self.last_painted = Some(cols);
        // #208: while this tab's floating set is visible, zellij routes a
        // swap-layout switch to the FLOATING ring, not the tiled one
        // (`Tab::next_swap_layout`, zellij-server 0.44.3 tab/mod.rs:1151) —
        // an ask cannot move this pane, only spend the walk budget against
        // the shell's layer. Hold judgement; the TabUpdate that reports the
        // hide re-renders, and that paint settles the disagreement.
        if self.own_tab_floating_visible() {
            return Vec::new();
        }
        // Deaf: an ask is in flight and its repaint echoes prove nothing.
        // The cooldown expiry judges `last_painted` once, in this instant's
        // place.
        if self.swap_in_flight {
            return Vec::new();
        }
        let want = self.showing_collapsed();
        if cols == self.row_height.target_cols(want) {
            self.walk_spent = None;
            return Vec::new();
        }
        let spent = match self.walk_spent {
            Some((w, n)) if w == want => n,
            _ => 0,
        };
        if spent >= WALK_ASK_CAP {
            return Vec::new();
        }
        self.walk_spent = Some((want, spent + 1));
        self.swap_in_flight = true;
        vec![Effect::SwapWidth]
    }

    /// The width cooldown fired (`main.rs` arms one [`WIDTH_COOLDOWN_SECS`]
    /// timer per `Effect::SwapWidth`): end the deafness and judge the latest
    /// painted width exactly once. Inert when no ask is in flight, so a
    /// misrouted timer expiry (the peek sink shares the event) costs
    /// nothing.
    pub fn width_cooldown_elapsed(&mut self) -> Vec<Effect> {
        if !self.swap_in_flight {
            return Vec::new();
        }
        self.swap_in_flight = false;
        self.width_effects(self.last_painted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- tiny builders (Step 1) --------------------------------------------

    /// An agent whose label == uuid, bound to `tab_id` (§6.6 Design B: the
    /// snapshot bind is the decoration join key).
    fn agent(uuid: &str, status: Status, tab_id: Option<usize>) -> Agent {
        Agent {
            uuid: uuid.into(),
            cwd: String::new(),
            repo_root: String::new(),
            branch: String::new(),
            label: uuid.into(),
            status,
            last_interacted: 0,
            commit_ord: 0,
            buckets: Default::default(),
            last_visited: 0,
            tab_id,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: None,
            context_tokens: None,
            context_level: None,
            model: None,
            provider: None,
            pr_number: None,
        }
    }

    /// The same row carrying its terminal pane — the production shape since
    /// #178, and since #187 the ONLY lasting source of the uuid→pane mapping:
    /// the bar replaces its map from every snapshot it accepts, so a row that
    /// omits the pane retires it. A test that wants the mapping to survive a
    /// snapshot puts it on the row here, exactly as the store does.
    fn agent_at(uuid: &str, status: Status, tab_id: Option<usize>, pane_id: u32) -> Agent {
        Agent {
            pane_id: Some(pane_id),
            ..agent(uuid, status, tab_id)
        }
    }

    /// An agent with an explicit label (Idle, never interacted).
    fn agent_labelled(uuid: &str, label: &str, tab_id: Option<usize>) -> Agent {
        Agent {
            uuid: uuid.into(),
            cwd: String::new(),
            repo_root: String::new(),
            branch: String::new(),
            label: label.into(),
            status: Status::Idle,
            last_interacted: 0,
            commit_ord: 0,
            buckets: Default::default(),
            last_visited: 0,
            tab_id,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: None,
            context_tokens: None,
            context_level: None,
            model: None,
            provider: None,
            pr_number: None,
        }
    }

    fn snap(seq: u64, agents: Vec<Agent>) -> AgentSnapshot {
        AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq,
            agents,
            tab_order: Default::default(),
        }
    }

    /// Snapshot carrying only a tab order (the §6.6 store tab order): pairs of
    /// (tab_id, commitment ordinal).
    fn snap_t(seq: u64, ords: &[(usize, u64)]) -> AgentSnapshot {
        AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq,
            agents: vec![],
            tab_order: ords.iter().copied().collect(),
        }
    }

    fn tab(id: usize, pos: usize, name: &str, active: bool) -> TabMeta {
        TabMeta {
            tab_id: id,
            position: pos,
            name: name.into(),
            active,
            floating_visible: false,
        }
    }

    /// The three-tab fleet's next `TabUpdate`, with `active` naming the
    /// focused tab.
    fn frame(m: &mut BarModel, active: usize) {
        m.apply_tabs(vec![
            tab(10, 0, "a", active == 10),
            tab(11, 1, "b", active == 11),
            tab(12, 2, "c", active == 12),
        ]);
    }

    fn pane(tab_pos: usize, id: u32, plugin: bool, focused: bool) -> PaneMeta {
        PaneMeta {
            tab_position: tab_pos,
            pane_id: id,
            is_plugin: plugin,
            is_focused: focused,
            is_floating: false,
            terminal_command: None,
            exited: false,
            exit_status: None,
        }
    }

    // --- #206 terminal rows: speakers, facts, borrowed provenance ----------

    /// The first terminal row's content — terminal-row tests care about THE
    /// terminal row, wherever recency sorted it.
    fn terminal_row(m: &BarModel) -> RowContent {
        m.rows()
            .into_iter()
            .map(|(_, r)| r.content)
            .find(|c| matches!(c, RowContent::Terminal { .. }))
            .expect("no terminal row")
    }

    fn probe(pane_id: u32, cwd: Option<&str>, foreground: Option<&[&str]>) -> PaneProbe {
        PaneProbe {
            pane_id,
            cwd: cwd.map(String::from),
            foreground: foreground.map(|a| a.iter().map(|s| s.to_string()).collect()),
        }
    }

    /// A foreground command runs; the shell coming back to its prompt goes
    /// idle but the command LINGERS — that is what "most recently run" means.
    #[test]
    fn a_shell_at_its_prompt_keeps_its_last_command() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #1", true)]);
        m.apply_panes(vec![pane(0, 5, false, true)]);
        assert!(m.apply_pane_facts(vec![probe(5, None, Some(&["cargo", "test"]))]));
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal { status: TermStatus::Running, command, .. } if command == "cargo test"
        ));
        // A running speaker is exactly what the exit-side poll exists for.
        assert!(m.term_poll_wanted());
        // Login shells report a dashed argv0; that is the prompt, not a command.
        assert!(m.apply_pane_facts(vec![probe(5, None, Some(&["-zsh"]))]));
        assert!(!m.term_poll_wanted());
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal { status: TermStatus::Idle, command, .. } if command == "cargo test"
        ));
        // A full-path argv0 is a command like any other, shown by basename.
        assert!(m.apply_pane_facts(vec![probe(5, None, Some(&["/usr/bin/git", "status"]))]));
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal { status: TermStatus::Running, command, .. } if command == "git status"
        ));
    }

    /// The newborn probe storm (live capture 2026-08-26 09:44): before
    /// hydration the agent list is empty, so every tab reads as a terminal
    /// and a freshly loaded visible bar serially interrogated the whole
    /// session — blocking round-trips that queue ahead of the very snapshot
    /// result that would have shrunk the list. A bar that cannot yet tell
    /// agents from terminals asks the OS about no one.
    #[test]
    fn a_hydrating_bar_probes_no_one() {
        let mut m = BarModel::default();
        m.await_hydration();
        m.apply_tabs(vec![
            tab(10, 0, "Tab #1", true),
            tab(11, 1, "Tab #2", false),
        ]);
        m.apply_panes(vec![pane(0, 5, false, true), pane(1, 6, false, true)]);
        assert_eq!(
            m.probe_targets(),
            Vec::<u32>::new(),
            "an unhydrated bar must not probe"
        );
        // Either snapshot path — the hydrate result or a live push — ends
        // the wait; a bar whose `clave snapshot` failed is healed by the
        // next store write like every other starved instance.
        m.apply_snapshot(snap(1, vec![]));
        assert_eq!(m.probe_targets(), vec![5, 6], "hydration re-opens probing");
    }

    /// The unanswerable-pane loop (live capture 2026-08-25: one pane, four
    /// hours of 100ms query timeouts): a pane whose running flag was latched
    /// by an event delta while both sync queries fail re-qualified on every
    /// manifest — the all-None guard deliberately mints no entry, so "retry
    /// until known" had no can-this-ever-succeed test. A failed probe now
    /// stands the pane down for the next few manifests; any real answer
    /// clears the stand-down at once.
    #[test]
    fn a_failed_probe_stands_its_pane_down() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #1", true)]);
        let manifest = vec![pane(0, 5, false, true)];
        m.apply_panes(manifest.clone());
        // The delta latches running; both sync queries then fail (all-None).
        m.apply_pane_facts(vec![probe(5, None, Some(&["cargo", "build"]))]);
        m.apply_pane_facts(vec![probe(5, None, None)]);
        assert_eq!(
            m.probe_targets(),
            Vec::<u32>::new(),
            "a just-failed pane is not re-listed"
        );
        assert!(
            !m.term_poll_wanted(),
            "the exit poll must not re-arm on a stood-down pane"
        );
        // Each manifest decays the stand-down; the pane re-qualifies after
        // three — a delay, never a blacklist.
        m.apply_panes(manifest.clone());
        m.apply_panes(manifest.clone());
        assert_eq!(m.probe_targets(), Vec::<u32>::new());
        m.apply_panes(manifest.clone());
        assert_eq!(m.probe_targets(), vec![5]);
        // A real answer clears the stand-down immediately.
        m.apply_pane_facts(vec![probe(5, None, None)]);
        assert_eq!(m.probe_targets(), Vec::<u32>::new());
        m.apply_pane_facts(vec![probe(5, None, Some(&["cargo", "build"]))]);
        assert_eq!(m.probe_targets(), vec![5]);
    }

    /// The cwd through the fleet's checkouts: inside an agent's worktree the
    /// terminal borrows that row's repo name, ink, provenance and branch
    /// verbatim (#232 for the branch: shared from the same matched row `pr`
    /// is borrowed from); outside the fleet it shows its directory name,
    /// untinted, unmarked, with no branch to borrow.
    #[test]
    fn a_terminal_cwd_borrows_a_matching_agents_repo_ink_and_provenance() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #1", true), tab(11, 1, "agent", false)]);
        m.apply_panes(vec![pane(0, 5, false, true), pane(1, 6, false, true)]);
        let a = Agent {
            repo_root: "/r/clave".into(),
            worktree: Some("/w/clave-wt".into()),
            branch: "feat".into(),
            ..agent_at("u1", Status::Idle, Some(11), 6)
        };
        m.apply_snapshot(snap(1, vec![a]));
        m.apply_pane_facts(vec![probe(5, Some("/w/clave-wt/crates"), None)]);
        let agent_ink = m
            .rows()
            .into_iter()
            .find_map(|(_, r)| match r.content {
                RowContent::Agent { repo_ink, .. } => Some(repo_ink),
                _ => None,
            })
            .expect("no agent row");
        match terminal_row(&m) {
            RowContent::Terminal {
                repo,
                repo_ink,
                provenance,
                branch,
                ..
            } => {
                assert_eq!(repo.as_deref(), Some("clave"));
                assert_eq!(repo_ink, agent_ink);
                assert_eq!(provenance, Provenance::Worktree);
                assert_eq!(branch, "feat");
            }
            c => panic!("not a terminal row: {c:?}"),
        }
        // Outside every checkout: directory name, no ink, no provenance, no
        // branch. A sibling path sharing the checkout's PREFIX must not
        // match — the boundary is a path component, not a byte prefix.
        m.apply_pane_facts(vec![probe(5, Some("/w/clave-wt-other"), None)]);
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal { repo: Some(r), repo_ink: None, provenance: Provenance::Main, branch, .. }
                if r == "clave-wt-other" && branch.is_empty()
        ));
    }

    /// A terminal in a DEFAULT checkout borrows no branch — the same
    /// predicate `agent_content` blanks its own branch cell on (#232),
    /// shared rather than re-derived, applied here through
    /// `terminal_content`'s borrow.
    #[test]
    fn a_terminal_in_a_default_checkout_borrows_no_branch() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #1", true), tab(11, 1, "agent", false)]);
        m.apply_panes(vec![pane(0, 5, false, true), pane(1, 6, false, true)]);
        let a = Agent {
            repo_root: "/r/clave".into(),
            branch: "main".into(),
            ..agent_at("u1", Status::Idle, Some(11), 6)
        };
        m.apply_snapshot(snap(1, vec![a]));
        m.apply_pane_facts(vec![probe(5, Some("/r/clave/crates"), None)]);
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal { branch, .. } if branch.is_empty()
        ));
    }

    /// A command pane is the one terminal with a visible lifecycle: running
    /// until it exits, then Done or Failed by exit code, its launch command
    /// standing in for the summary until any foreground fact arrives.
    #[test]
    fn a_command_pane_reports_done_or_failed_by_exit_code() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #1", true)]);
        let cmd_pane = |exited: bool, code: Option<i32>| PaneMeta {
            terminal_command: Some("cargo test --workspace".into()),
            exited,
            exit_status: code,
            ..pane(0, 5, false, true)
        };
        m.apply_panes(vec![cmd_pane(false, None)]);
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal { status: TermStatus::Running, command, .. }
                if command == "cargo test --workspace"
        ));
        m.apply_panes(vec![cmd_pane(true, Some(0))]);
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal {
                status: TermStatus::Done,
                ..
            }
        ));
        m.apply_panes(vec![cmd_pane(true, Some(101))]);
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal {
                status: TermStatus::Failed,
                ..
            }
        ));
    }

    /// The speaker is the focused tiled terminal, falling back to the first;
    /// plugin panes (the bar itself) and floating panes (the Alt+f scratch
    /// shell) never speak. Agent tabs are not probed at all — their row is
    /// the store's.
    #[test]
    fn the_speaker_is_the_focused_tiled_terminal_and_agent_tabs_are_not_probed() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #1", true), tab(11, 1, "agent", false)]);
        m.apply_snapshot(snap(1, vec![agent_at("u1", Status::Idle, Some(11), 9)]));
        let floating = PaneMeta {
            is_floating: true,
            ..pane(0, 7, false, true)
        };
        m.apply_panes(vec![
            pane(0, 1, true, false), // the bar
            pane(0, 5, false, false),
            pane(0, 6, false, true),
            floating,
            pane(1, 9, false, true), // the agent's pane
        ]);
        assert_eq!(m.probe_targets(), vec![6]);
        // Known and idle → not listed (the zero-steady-state gate); running
        // → listed again, because the exit-side poll needs a second look.
        m.apply_pane_facts(vec![probe(6, Some("/tmp"), Some(&["zsh"]))]);
        assert_eq!(m.probe_targets(), Vec::<u32>::new());
        m.apply_pane_facts(vec![probe(6, None, Some(&["cargo", "build"]))]);
        assert_eq!(m.probe_targets(), vec![6]);
        // No tiled pane focused (focus sits on the floating shell): first
        // tiled terminal speaks.
        m.apply_panes(vec![
            pane(0, 1, true, false),
            pane(0, 5, false, false),
            pane(0, 6, false, false),
        ]);
        assert_eq!(m.probe_targets(), vec![5]);
    }

    /// A closed pane's facts must not survive it — and a pane id reborn later
    /// must start blank, not inherit a dead shell's history.
    #[test]
    fn pane_facts_die_with_their_pane() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #1", true)]);
        m.apply_panes(vec![pane(0, 5, false, true)]);
        m.apply_pane_facts(vec![probe(5, Some("/tmp/x"), Some(&["cargo", "test"]))]);
        m.apply_panes(vec![pane(0, 6, false, true)]);
        m.apply_panes(vec![pane(0, 5, false, true)]);
        // A probe where BOTH queries failed (pane mid-spawn, process gone)
        // must mint nothing: an entry is "known", and a known-and-idle pane
        // is never re-probed, so a birth failure would stick forever.
        assert!(!m.apply_pane_facts(vec![probe(5, None, None)]));
        // The failure stands the pane down rather than re-listing it (see
        // `probe_backoff`); the birth-failure RETRY the no-mint rule protects
        // still happens, one stand-down later.
        assert_eq!(m.probe_targets(), Vec::<u32>::new());
        for _ in 0..PROBE_FAILURE_STANDDOWN {
            m.apply_panes(vec![pane(0, 5, false, true)]);
        }
        assert_eq!(m.probe_targets(), vec![5]);
        assert!(matches!(
            terminal_row(&m),
            RowContent::Terminal {
                status: TermStatus::Idle,
                repo: None,
                command,
                ..
            } if command.is_empty()
        ));
    }

    /// A running fact whose pane stopped being the speaker must not keep the
    /// exit-side poll armed: the poll only ever re-probes `probe_targets`, so
    /// a flag no probe can clear would re-arm a 3s timer for as long as the
    /// pane lives (the unreachable-success retry loop, FOOTGUNS.md).
    #[test]
    fn a_stranded_running_fact_does_not_arm_the_poll() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #1", true)]);
        m.apply_panes(vec![pane(0, 5, false, true), pane(0, 6, false, false)]);
        m.apply_pane_facts(vec![probe(5, None, Some(&["sleep", "999"]))]);
        assert!(m.term_poll_wanted());
        // Focus moves to the sibling: 6 speaks now, 5's flag is unreachable.
        m.apply_panes(vec![pane(0, 5, false, false), pane(0, 6, false, true)]);
        assert_eq!(m.probe_targets(), vec![6]);
        assert!(!m.term_poll_wanted());
        // Speakership coming back re-lists the pane, and the poll resumes.
        m.apply_panes(vec![pane(0, 5, false, true), pane(0, 6, false, false)]);
        assert!(m.term_poll_wanted());
    }

    // --- row projections ---------------------------------------------------
    // `rows()` yields `(RowKey, render::Row)`: model identity plus the whole
    // presentation (LEDGER D6 — one row type). Most assertions here are about
    // ONE of those, so these pull out the field under test rather than
    // spelling out a `RowContent` at every site.

    fn keys(m: &BarModel) -> Vec<RowKey> {
        m.rows().into_iter().map(|(k, _)| k).collect()
    }

    fn selected(m: &BarModel) -> Vec<bool> {
        m.rows().into_iter().map(|(_, r)| r.selected).collect()
    }

    /// Terminal-tab names in display order. Lock §7.1: the zellij tab name is
    /// used ONLY for a terminal tab, so an agent row has none — tests that mix
    /// the two assert on `keys` instead.
    fn names(m: &BarModel) -> Vec<String> {
        m.rows()
            .into_iter()
            .map(|(k, r)| match r.content {
                RowContent::Terminal { name, .. } => name,
                RowContent::Agent { .. } => panic!("{k:?} is an agent row, not a terminal"),
            })
            .collect()
    }

    /// The status cell of row `i`; `None` for a terminal row, which has no
    /// turn to be in.
    fn status_at(m: &BarModel, i: usize) -> Option<RowStatus> {
        match &m.rows()[i].1.content {
            RowContent::Agent { status, .. } => Some(*status),
            RowContent::Terminal { .. } => None,
        }
    }

    /// The rendered line the dormant row sits on in [`live_plus_dormant`].
    const DORMANT_LINE: usize = 1;

    /// The dwell-commit fixture: one live tab (id 1, ordinal 500) and one
    /// dormant row (`u-d`, ordinal 999), shared by the #100 selection tests.
    ///
    /// **The dormant row's ordinal is deliberately the HIGHER of the two.**
    /// Before #112 that put it on line 0, above the live tab; segregation puts
    /// it on line 1 regardless, because the block a row renders in outranks
    /// the number that sorts it. So every caller's line indices double as a
    /// guard: merge the two blocks back into one list and they all move.
    fn live_plus_dormant() -> BarModel {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.commit_ord = 999;
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m
    }

    /// A pane taller than any fixture here. With nothing overflowing, the
    /// viewport (#148) rests at the top and a rendered line IS its model row —
    /// which is what every click assertion written before the viewport assumed.
    const TALL_PANE: usize = 64;

    /// The legacy geometry, said out loud. Every click assertion below was
    /// written when one rendered LINE was one model ROW, and #232 moved the
    /// DEFAULT to the two-line card — so those tests pin `Single` explicitly
    /// rather than riding a default that has shifted underneath them. The
    /// card's own click map is
    /// `a_click_on_either_line_of_a_card_selects_that_card`.
    fn one_line_bar(mut m: BarModel) -> BarModel {
        m.set_row_height(RowHeight::Single);
        m
    }

    /// One live tab (id 1, active) and `dormant` dormant rows, ordinals
    /// descending so the dormant block's display order is `u-00`, `u-01`, …
    /// A fleet built to OVERFLOW: the viewport tests below give it a pane that
    /// cannot hold it.
    fn overflowing_fleet(dormant: usize) -> BarModel {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let agents = (0..dormant)
            .map(|i| {
                let mut a = agent(&format!("u-{i:02}"), Status::Idle, None);
                a.commit_ord = 900 - i as u64;
                a
            })
            .collect();
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents,
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m
    }

    /// Select the dormant row the way a user now can — Alt+N on its rendered
    /// line. #112 confines the dir walk to the live block, so the walk that
    /// these tests used to reach it with no longer arrives.
    fn select_dormant(m: &mut BarModel) -> Vec<Effect> {
        m.nav(&format!("{{\"row\":{}}}", DORMANT_LINE + 1), Some(1))
    }

    // --- tests -------------------------------------------------------------

    #[test]
    fn rows_order_by_last_user_commitment() {
        // §6.6 / S1: one list of commitment ORDINALS, owned by the STORE and
        // replaced from each snapshot — tab commitments ∨ agent prompts. Focus
        // moves NOTHING. (The values below are ordinals, not clock readings;
        // the wall clock stopped being the key in S1/#39.)
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(11, 1, "b", true),
            tab(12, 2, "c", false),
        ]);
        // Nothing committed yet → tab-position order, active flag irrelevant.
        assert_eq!(names(&m), vec!["a", "b", "c"]);
        // Commitments arrive via snapshot and order by ordinal, descending…
        m.apply_snapshot(snap_t(1, &[(10, 1000), (11, 2000), (12, 1500)]));
        assert_eq!(names(&m), vec!["b", "c", "a"]);
        // …and focus (beacon) does not reorder.
        m.beacon(10);
        assert_eq!(names(&m)[0], "b");
        // Agent prompts reorder ONLY through the store's tab order (the hook
        // stamps it via the bind, §6.6 Design B) — an agent's last_interacted
        // alone must NOT sort: render-time joins diverge per instance
        // (round 6).
        let mut s = snap(2, vec![agent("u1", Status::Working, Some(12))]);
        s.agents[0].last_interacted = 9999;
        s.tab_order = [(10, 1000), (11, 2000), (12, 1500)].into();
        m.apply_snapshot(s);
        // By KEY from here: tab 12 now hosts an agent, and an agent row does
        // not carry the zellij tab name (lock §7.1).
        assert_eq!(keys(&m)[0], RowKey::Tab(11)); // "b" — li ignored, ordinal rules
        // The prompt's stamp arrives IN the tab order → c fronts everywhere.
        m.apply_snapshot(snap_t(3, &[(10, 1000), (11, 2000), (12, 3000)]));
        assert_eq!(keys(&m)[0], RowKey::Tab(12)); // "c"
    }

    #[test]
    fn tab_order_is_replaced_from_snapshots_never_merged() {
        // C5 round 5: per-instance merged copies of pipe deltas DIVERGED
        // (missed echoes under spinup congestion) and walking oscillated.
        // The fix: the snapshot's map is authoritative — REPLACE, don't
        // merge, so every instance converges on the store's order.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", false)]);
        m.apply_snapshot(snap_t(1, &[(10, 2000)]));
        assert_eq!(keys(&m)[0], RowKey::Tab(10));
        // New snapshot: b now leads, and a's old entry is GONE (replace
        // semantics — a merge would have kept a at 2000 and diverged).
        m.apply_snapshot(snap_t(2, &[(11, 1000)]));
        assert_eq!(keys(&m)[0], RowKey::Tab(11));
        // A stale seq must not replace anything (§5 gate).
        m.apply_snapshot(snap_t(1, &[(10, 9000)]));
        assert_eq!(keys(&m)[0], RowKey::Tab(11));
    }

    // --- frecency ordering (spec 2026-08-19) --------------------------------

    /// The decay curve itself: today full weight, each day halves (24h
    /// half-life), future-dated buckets clamp to age 0, empty map is 0.
    #[test]
    fn frecency_millis_decays_by_half_lives() {
        let b: BTreeMap<u32, u32> = [(100, 4), (99, 4), (93, 4)].into();
        // today=100, hl=24h: 4*1000 + 4*500; day 93 is exactly 7 days old —
        // outside the retention window, so it scores ZERO, mirroring the
        // store prune that would have dropped it on the row's next bump.
        assert_eq!(frecency_millis(&b, 100, 24), 6000);
        assert_eq!(frecency_millis(&BTreeMap::new(), 100, 24), 0);
        let future: BTreeMap<u32, u32> = [(105, 2)].into();
        assert_eq!(frecency_millis(&future, 100, 24), 2000); // clamp, not panic
        // A zero dial is reachable from the CLI and the wire; it must behave
        // as a 1-hour half-life, not as a division by zero.
        let b0: BTreeMap<u32, u32> = [(100, 4), (99, 4)].into();
        assert_eq!(frecency_millis(&b0, 100, 0), frecency_millis(&b0, 100, 1));
        assert!(frecency_millis(&b0, 100, 0) >= 4000);
    }

    /// Accelerated time: a hand-computed week of daily driving, two live
    /// agents, 24h dial. The maintainer cannot wall-clock this before living
    /// with it, so the regimes are pinned here: burst wins day 0; steady
    /// daily use overtakes a stale burst by day 3; the burst exits the 7-day
    /// window and survives only on the ordinal floor; by day 10 everything
    /// has decayed and the ordinal fallback quietly flips the order back.
    /// Days 7 and 10 re-rank on IDENTICAL buckets — only `snapshot.today`
    /// moves, which is exactly what an idle fleet's midnight rollover does
    /// (today is host-computed per push; the bar never reads a clock).
    #[test]
    fn a_week_of_daily_driving_reranks_only_by_decay_and_the_window() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "burst", false), tab(2, 1, "steady", false)]);
        let mut burst = agent("u-burst", Status::Idle, Some(1));
        let mut steady = agent("u-steady", Status::Idle, Some(2));
        // Ordinals for the fully-decayed endgame: burst committed last long
        // ago (tab 1 ord 9 > tab 2 ord 5).
        let ords: &[(usize, u64)] = &[(1, 9), (2, 5)];

        // Day 100: five prompts into burst, one into steady. 5000 > 1000.
        burst.buckets = [(100, 5)].into();
        steady.buckets = [(100, 1)].into();
        let mut snap = snap_full(1, vec![burst.clone(), steady.clone()], ords);
        snap.today = 100;
        m.apply_snapshot(snap);
        assert_eq!(keys(&m), vec![RowKey::Tab(1), RowKey::Tab(2)]);

        // Day 103: steady prompted once each day, burst went silent.
        // burst 5*0.5^3 = 625; steady 125+250+500+1000 = 1875.
        steady.buckets = [(100, 1), (101, 1), (102, 1), (103, 1)].into();
        let mut snap = snap_full(2, vec![burst.clone(), steady.clone()], ords);
        snap.today = 103;
        m.apply_snapshot(snap);
        assert_eq!(frecency_millis(&burst.buckets, 103, 24), 625);
        assert_eq!(frecency_millis(&steady.buckets, 103, 24), 1875);
        assert_eq!(keys(&m), vec![RowKey::Tab(2), RowKey::Tab(1)]);

        // Day 107, nobody prompted since: burst's day-100 bucket is now 7
        // days old — out of the window, score ZERO, alive on the ordinal
        // floor only. steady still carries 15.625+31.25+62.5, floored = 109.
        let mut snap = snap_full(3, vec![burst.clone(), steady.clone()], ords);
        snap.today = 107;
        m.apply_snapshot(snap);
        assert_eq!(frecency_millis(&burst.buckets, 107, 24), 0);
        assert_eq!(frecency_millis(&steady.buckets, 107, 24), 109);
        assert_eq!(keys(&m), vec![RowKey::Tab(2), RowKey::Tab(1)]);

        // Day 110, same buckets again: steady's history has left the window
        // too. Both zero → ordinal fallback, and the order flips back to
        // burst (ord 9 > 5) with no interaction having happened at all.
        let mut snap = snap_full(4, vec![burst.clone(), steady.clone()], ords);
        snap.today = 110;
        m.apply_snapshot(snap);
        assert_eq!(frecency_millis(&steady.buckets, 110, 24), 0);
        assert_eq!(keys(&m), vec![RowKey::Tab(1), RowKey::Tab(2)]);
    }

    /// Two live agent tabs: `u-big` invested 10 commitments today but committed
    /// FIRST (ordinal 5, position 1); `u-latest` one commitment, the most
    /// recent (ordinal 9, position 0). The two modes must disagree about them.
    fn invested_vs_one_off(order: OrderMode) -> BarModel {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "latest", false), tab(2, 1, "big", false)]);
        let mut latest = agent("u-latest", Status::Idle, Some(1));
        latest.buckets = [(100, 1)].into();
        let mut big = agent("u-big", Status::Idle, Some(2));
        big.buckets = [(100, 10)].into();
        let mut s = snap_full(1, vec![latest, big], &[(1, 9), (2, 5)]);
        s.order = order;
        s.today = 100;
        m.apply_snapshot(s);
        m
    }

    /// Maintainer ruling (2026-08-19): "fully decayed at 7 days" is a
    /// semantic, at EVERY dial. The store prunes lazily — only on a row's
    /// next bump — so a long-dormant row still carries stale days, and
    /// without the scoring cut a 999h dial would resurrect them.
    #[test]
    fn a_bucket_past_retention_scores_zero_at_every_dial() {
        let stale: BTreeMap<u32, u32> = [(93, 1000)].into(); // exactly 7 days old
        assert_eq!(frecency_millis(&stale, 100, 999), 0);
        assert_eq!(frecency_millis(&stale, 100, 1), 0);
        let edge: BTreeMap<u32, u32> = [(94, 4)].into(); // today-6: last day in
        assert!(frecency_millis(&edge, 100, 999) > 0);
    }

    /// Frecency mode: more decayed weight ranks higher, regardless of who
    /// committed last (the whole point vs recency).
    #[test]
    fn frecency_ranks_invested_rows_above_recent_one_offs() {
        let m = invested_vs_one_off(OrderMode::Frecency {
            half_life_hours: 24,
        });
        assert_eq!(keys(&m), vec![RowKey::Tab(2), RowKey::Tab(1)]);
    }

    /// Recency mode is bit-identical to the shipped behaviour: the last
    /// commitment fronts, weight ignored.
    #[test]
    fn recency_mode_ranks_by_ordinal_exactly_as_before() {
        let m = invested_vs_one_off(OrderMode::Recency);
        assert_eq!(keys(&m), vec![RowKey::Tab(1), RowKey::Tab(2)]);
    }

    /// The adjacency mechanism end-to-end: an exact bucket copy ties, and the
    /// existing position tiebreak puts the newborn DIRECTLY BELOW its opener.
    /// The opener scores through its AGENT's buckets, the newborn (a plain
    /// terminal tab) through its `tab_buckets` twin — same map, exact tie.
    #[test]
    fn an_inherited_copy_sits_directly_below_its_opener() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(9, 0, "top", false),
            tab(1, 1, "small", false),
            tab(2, 2, "opener", false),
            tab(5, 5, "newborn", false),
        ]);
        let mut opener = agent("u-opener", Status::Idle, Some(2));
        opener.buckets = [(100, 6)].into();
        let mut s = snap(1, vec![opener]);
        s.order = OrderMode::Frecency {
            half_life_hours: 24,
        };
        s.today = 100;
        s.tab_buckets = [
            (9usize, [(100u32, 9u32)].into()),
            (5, [(100, 6)].into()), // the newborn's inherited exact copy
            (1, [(100, 1)].into()),
        ]
        .into();
        m.apply_snapshot(s);
        assert_eq!(
            keys(&m),
            vec![
                RowKey::Tab(9), // 9000 millipoints
                RowKey::Tab(2), // 6000, position 2 — the opener
                RowKey::Tab(5), // 6000, position 5 — its echo, directly below
                RowKey::Tab(1), // 1000
            ]
        );
    }

    /// Zero-score rows fall back to ordinal order — upgrade day and
    /// never-touched fleets keep the shipped S1 ordering instead of collapsing
    /// to tab position (spec: the comparator's zero fallback).
    #[test]
    fn zero_scores_fall_back_to_ordinal_order() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(1, 0, "a", false),
            tab(2, 1, "b", false),
            tab(3, 2, "c", false),
        ]);
        // NO buckets anywhere; ordinals 30/10/20 disagree with position order.
        let mut s = snap_t(1, &[(1, 30), (2, 10), (3, 20)]);
        s.order = OrderMode::Frecency {
            half_life_hours: 24,
        };
        s.today = 100;
        m.apply_snapshot(s);
        assert_eq!(
            keys(&m),
            vec![RowKey::Tab(1), RowKey::Tab(3), RowKey::Tab(2)]
        );
    }

    /// The dormant side of the R2 identity: a dormant row's decayed weight
    /// outranks a dormant row's bare ordinal, same as it would live — closing
    /// a tab must not change which number ranks a row. (Mutation testing
    /// caught this side uncovered: `dormant_key`'s `millis > 0` survived a
    /// flip with a live-only suite.)
    #[test]
    fn a_dormant_rows_weight_outranks_a_dormant_rows_ordinal() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut invested = agent("u-invested", Status::Idle, None);
        invested.buckets = [(100, 3)].into();
        let mut cold = agent("u-cold", Status::Idle, None);
        cold.commit_ord = 999;
        let mut s = snap(1, vec![invested, cold]); // order: the Frecency default
        s.today = 100;
        m.apply_snapshot(s);
        assert_eq!(
            keys(&m),
            vec![
                RowKey::Tab(1),                       // block segregation: live always above
                RowKey::Dormant("u-invested".into()), // 3000 millipoints
                RowKey::Dormant("u-cold".into()),     // zero fallback, ordinal 999
            ]
        );
    }

    /// Snapshot state replaces, never merges (the tab_order doctrine, C5
    /// round 5).
    #[test]
    fn order_today_and_tab_buckets_are_replaced_from_snapshots() {
        let mut m = BarModel::default();
        let mut s1 = snap(1, vec![]);
        s1.order = OrderMode::Recency;
        s1.today = 100;
        s1.tab_buckets = [(3usize, [(100u32, 1u32)].into())].into();
        m.apply_snapshot(s1);
        assert_eq!(m.order, OrderMode::Recency);
        assert_eq!(m.today, 100);
        assert_eq!(m.tab_buckets.get(&3).and_then(|b| b.get(&100)), Some(&1));
        let mut s2 = snap(2, vec![]);
        s2.order = OrderMode::Frecency {
            half_life_hours: 24,
        };
        s2.today = 101;
        m.apply_snapshot(s2);
        assert_eq!(
            m.order,
            OrderMode::Frecency {
                half_life_hours: 24
            }
        );
        assert_eq!(m.today, 101);
        assert!(m.tab_buckets.is_empty()); // replaced wholesale, not merged
    }

    #[test]
    fn birth_touch_fires_once_ever_and_defers_to_snapshot_knowledge() {
        // The guard must be LOCAL and echo-independent (C5 rd 4: guards that
        // waited on the pipe echo re-fired per TabUpdate → spawn storm →
        // zellij server fd exhaustion).
        let mut m = BarModel::default();
        // Unknown tab: fire exactly once, no matter how many TabUpdates land
        // before the store snapshot echoes back.
        assert!(m.needs_birth_touch(10));
        assert!(!m.needs_birth_touch(10));
        assert!(!m.needs_birth_touch(10));
        // A tab the store already knows (snapshot arrived first — e.g. this
        // instance loaded after another instance birth-touched it): never fire.
        m.apply_snapshot(snap_t(1, &[(11, 1000)]));
        assert!(!m.needs_birth_touch(11));
        // Replace semantics dropping a tab from the map must NOT re-arm an
        // already-fired guard (10 fired above, snapshot 1 didn't carry it).
        assert!(!m.needs_birth_touch(10));
    }

    #[test]
    fn agent_rows_get_glyphs_via_snapshot_bind_no_local_joins() {
        // §6.6 Design B: decoration keys on the agent's snapshot tab_id —
        // NO register, NO pane manifest. This is exactly the round-6 case:
        // an instance that loaded after the agent registered (registers
        // never replay) must still decorate and order correctly.
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "agent-tab", false),
            tab(11, 1, "plain", false),
        ]);
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Working, Some(10))]));
        let rows = m.rows();
        let a = rows.iter().find(|(k, _)| *k == RowKey::Tab(10)).unwrap();
        let p = rows.iter().find(|(k, _)| *k == RowKey::Tab(11)).unwrap();
        assert!(matches!(
            a.1.content,
            RowContent::Agent {
                status: RowStatus::Working,
                ..
            }
        ));
        // The bound tab renders as an AGENT, so the zellij tab name is gone
        // (lock §7.1); the unbound one is still a terminal and keeps it.
        assert_eq!(p.1.content, RowContent::terminal("plain"));
        // An UNBOUND agent (bind not landed yet) decorates nothing.
        let mut m2 = BarModel::default();
        m2.apply_tabs(vec![tab(10, 0, "agent-tab", false)]);
        m2.apply_snapshot(snap(1, vec![agent("u1", Status::Working, None)]));
        assert_eq!(status_at(&m2, 0), None); // still a terminal row
    }

    #[test]
    fn snapshot_seq_gate_discards_stale() {
        let mut m = BarModel::default();
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, Some(10))]));
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Failed, Some(10))])); // stale
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        assert_eq!(status_at(&m, 0), Some(RowStatus::Working)); // still Working
    }

    #[test]
    fn rename_only_when_label_changes_not_when_tab_name_differs() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "old-name", false)]);
        let fx = m.apply_snapshot(snap(
            1,
            vec![agent_labelled("u1", "x · main · fix auth", Some(10))],
        ));
        assert!(fx.contains(&Effect::RenameTab {
            tab_id: 10,
            name: "x · main · fix auth".into()
        }));
        // Same label again — even though the TAB name is still "old-name"
        // (e.g. the user manually renamed it), we do NOT re-rename.
        let fx = m.apply_snapshot(snap(
            2,
            vec![agent_labelled("u1", "x · main · fix auth", Some(10))],
        ));
        assert!(fx.iter().all(|e| !matches!(e, Effect::RenameTab { .. })));
        // A genuinely NEW label renames again.
        let fx = m.apply_snapshot(snap(
            3,
            vec![agent_labelled("u1", "x · main · Fix auth flow", Some(10))],
        ));
        assert!(fx.contains(&Effect::RenameTab {
            tab_id: 10,
            name: "x · main · Fix auth flow".into()
        }));
    }

    #[test]
    fn focus_on_done_agent_marks_read_once_and_renders_idle() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Done, Some(10))]));
        assert_eq!(status_at(&m, 0), Some(RowStatus::Done)); // green, unread
        // Tab gains focus → local clear + MarkRead effect, exactly once.
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.contains(&Effect::MarkRead { uuid: "u1".into() }));
        // The local unread override: a READ Done renders Idle (§6.5).
        assert_eq!(status_at(&m, 0), Some(RowStatus::Idle)); // rendered dim NOW
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.iter().all(|e| !matches!(e, Effect::MarkRead { .. })));
        // A repeat of the SAME Done — any unrelated store push carries every
        // row's status — must NOT disturb the override: the user has read this
        // completion and the row stays dim. This is the assertion that tells
        // the rule apart from its inversion; the Working one below cannot,
        // because the override only ever changes how a `Done` renders, so a
        // Working row reads Working either way. Inverted, every push while an
        // agent sits finished re-lights it green, and "green" stops meaning
        // "there is something here you have not seen". (cargo mutants
        // 2026-08-15: `status != Done` → `==` survived on the Working
        // assertion alone.)
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Done, Some(10))]));
        assert_eq!(
            status_at(&m, 0),
            Some(RowStatus::Idle),
            "a re-push of the same Done is not a new completion"
        );
        // A later snapshot showing Working clears the local override.
        m.apply_snapshot(snap(3, vec![agent("u1", Status::Working, Some(10))]));
        assert_eq!(status_at(&m, 0), Some(RowStatus::Working));
        // And the clear is real: the NEXT completion is unread again, which is
        // the thing the user is waiting to be told about.
        m.apply_snapshot(snap(4, vec![agent("u1", Status::Done, Some(10))]));
        assert_eq!(
            status_at(&m, 0),
            Some(RowStatus::Done),
            "a second completion is unread again"
        );
    }

    #[test]
    fn done_agent_clears_without_observable_transition() {
        // Live C3 finding (2026-07-06): zellij delivers TabUpdate ONLY to the
        // instance in the active tab, so an instance's stream always claims
        // its own tab is active — there is never a prev!=now transition to
        // observe. Receiving a TabUpdate at all IS the focus signal.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "t", true)]); // loaded with own tab active
        // User leaves (this instance hears NOTHING), agent finishes via pipe:
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Done, Some(10))]));
        // User returns: the update still says "own tab active" — must clear.
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.contains(&Effect::MarkRead { uuid: "u1".into() }));
        assert_eq!(status_at(&m, 0), Some(RowStatus::Idle)); // dim immediately
    }

    #[test]
    fn bind_effects_report_own_tab_join_once_and_echo_independently() {
        // §6.6 Design B bootstrap: the agent tab's OWN bar (fresh manifest,
        // active at spawn) computes uuid→pane→own tab and reports it ONCE via
        // `clave bind`. The guard is last-SENT, never the snapshot echo
        // (echo-gated guards storm — C5 rd 4).
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(11, 1, "ag", true)]);
        m.apply_panes(vec![pane(1, 5, false, true)]);
        m.register("u1".into(), 5);
        m.apply_snapshot(snap(1, vec![agent_at("u1", Status::Working, None, 5)]));
        assert_eq!(
            m.bind_effects(11),
            vec![Effect::Bind {
                uuid: "u1".into(),
                tab_id: 11
            }]
        );
        // Repeat calls before (or after) the echo: silent.
        assert_eq!(m.bind_effects(11), Vec::<Effect>::new());
        m.apply_snapshot(snap(2, vec![agent_at("u1", Status::Working, Some(11), 5)]));
        assert_eq!(m.bind_effects(11), Vec::<Effect>::new());
        // An agent whose pane is NOT in my tab is never mine to bind — but
        // pane 9 is in no tab of my frame AT ALL, which since #178 is worth
        // exactly one breadcrumb: it is indistinguishable, from in here, from
        // a pane frame that never reached this instance.
        m.register("u2".into(), 9); // pane 9 unknown to my manifest
        m.apply_snapshot(snap(
            3,
            vec![
                agent_at("u1", Status::Working, Some(11), 5),
                agent_at("u2", Status::Working, None, 9),
            ],
        ));
        // The bind pass itself stays pure: it computes binds, and reports
        // nothing (the #178 breadcrumb runs ahead of the gates, in
        // `identity_effects`).
        assert_eq!(m.bind_effects(11), Vec::<Effect>::new());
    }

    // --- #55 frame coherence & executor election (RC-A / RC-B) -------------

    /// Snapshot carrying agents AND a tab order. The #55 tests need both:
    /// a seeded tab order suppresses the birth touch so a test can assert on
    /// binds alone.
    fn snap_full(seq: u64, agents: Vec<Agent>, ords: &[(usize, u64)]) -> AgentSnapshot {
        AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq,
            agents,
            tab_order: ords.iter().copied().collect(),
        }
    }

    /// One bar of the three-tab fleet `10@0, 11@1, 12@2` (plugin panes
    /// 100/101/102, terminals 5/6/7), holding the coherent frame pair it had
    /// while `active` was the focused tab. Its birth announce has been spent on
    /// that tab, so its beacon names `active` — which is what every real
    /// instance holds once the session has settled.
    fn fleet_bar(own_tab: usize, active: usize) -> BarModel {
        let mut m = BarModel::default();
        m.set_own_pane(FLEET_PANES[own_tab - 10].1);
        m.apply_panes(panes_at(&FLEET_PANES));
        m.apply_tabs(vec![
            tab(10, 0, "a", active == 10),
            tab(11, 1, "b", active == 11),
            tab(12, 2, "c", active == 12),
        ]);
        m
    }

    /// The dossier's reproduction fleet, seen from the bar in tab 11 (plugin
    /// pane 101).
    fn fleet_of_three(active: usize) -> BarModel {
        fleet_bar(11, active)
    }

    /// The adversarial instance FOOTGUNS.md:63 describes: a bar that was the
    /// active one once and has been event-starved ever since. Its tab/pane
    /// frame pair is FROZEN, self-coherent, and names its OWN tab active —
    /// which is why self-diagnosed "am I active" is poison (FOOTGUNS.md:64).
    /// Only its beacon can still move, because pipes broadcast and frames do
    /// not. `own_tab` picks which of the three-tab fleet's bars this is.
    fn starved_bar(own_tab: usize) -> BarModel {
        fleet_bar(own_tab, own_tab)
    }

    /// Deliver a fleet's beacon pipes the way zellij does: every
    /// `AnnounceVisit`/`ReanchorVisit` is a `clave-visited` broadcast, so it
    /// reaches EVERY instance including the sender. Lets a multi-bar test drive
    /// frames at one bar and still hold the others' replicated state true.
    fn fan_beacons(bars: &mut [BarModel], fx: &[Effect]) {
        for e in fx {
            if let Effect::AnnounceVisit { tab_id } | Effect::ReanchorVisit { tab_id } = e {
                for m in bars.iter_mut() {
                    m.beacon(*tab_id);
                }
            }
        }
    }

    /// Which bars of a fleet would act on one broadcast `clave-nav`. More than
    /// one is the C5 round-2 divergence: each walks its own tab set and they
    /// switch to different targets.
    fn nav_executors(bars: &[BarModel]) -> Vec<usize> {
        bars.iter().filter_map(|m| m.nav_executor()).collect()
    }

    /// A manifest over `(tab_position, plugin pane, terminal pane)` triples.
    /// Pane ids are stable IDENTITIES and positions are not — the whole of
    /// RC-A is that a close renumbers the latter — so these tests must name
    /// both, never derive one from the other.
    fn panes_at(rows: &[(usize, u32, u32)]) -> Vec<PaneMeta> {
        rows.iter()
            .flat_map(|&(pos, plug, term)| {
                [pane(pos, plug, true, false), pane(pos, term, false, false)]
            })
            .collect()
    }

    /// The three-tab fleet's coherent manifest: bars 100/101/102 and terminals
    /// 5/6/7 at positions 0/1/2.
    const FLEET_PANES: [(usize, u32, u32); 3] = [(0, 100, 5), (1, 101, 6), (2, 102, 7)];
    /// The same fleet after tab 10 (position 0) closes: everything shifts down
    /// one, so OUR pane 101 moves from position 1 to position 0.
    const FLEET_PANES_AFTER_CLOSE: [(usize, u32, u32); 2] = [(0, 101, 6), (1, 102, 7)];

    #[test]
    fn own_tab_is_none_while_the_pane_and_tab_frames_disagree() {
        // RC-A verbatim. The two frames are delivered independently and joined
        // on tab POSITION, which zellij renumbers after a close — so in the
        // window between one frame landing and the other, the join names a
        // DIFFERENT tab. Driven manifest-first (the mirror of the dossier's
        // trace) because that is the direction in which the pre-#55
        // computation does not merely fail, it confidently returns a dead tab.
        let mut m = fleet_of_three(10);
        // The close of tab 10 renumbers everything; the PaneUpdate lands first.
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        // Our pane is now at position 0. The FROZEN tab frame says position 0
        // is tab 10 — the tab that was just closed — and that it is active.
        assert_eq!(m.own_tab(), None, "fail closed while the frames disagree");
        assert!(!m.elects_confirmed());
        // The regression this fix pins: the pre-#55 join elects us, and would
        // have bound our agent to tab 10, evicting whoever holds it next.
        assert!(
            m.elects_presumed(),
            "the pre-#55 computation elects on the stale join — this is RC-A"
        );
    }

    #[test]
    fn a_newborn_model_is_incoherent_before_either_frame_has_arrived() {
        // The pre-first-frame fail-closed arm. Asserted on `frames_coherent`
        // directly because two EMPTY frames trivially cover the same (empty)
        // position set — the set comparison alone would call that coherent,
        // which is the one case the explicit guard exists for.
        let mut m = BarModel::default();
        m.set_own_pane(101);
        assert!(!m.frames_coherent());
        assert_eq!(m.own_tab(), None);
        assert!(!m.elects_confirmed());
        assert!(!m.elects_presumed());
        // One frame alone is not enough either.
        m.apply_tabs(vec![tab(11, 0, "b", true)]);
        assert!(!m.frames_coherent());
        assert_eq!(m.own_tab(), None);
    }

    #[test]
    fn own_tab_resolves_once_the_lagging_tab_frame_lands() {
        let mut m = fleet_of_three(10);
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        assert_eq!(m.own_tab(), None);
        // The renumbering TabUpdate — the event that was always going to
        // arrive anyway — is the retry.
        m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        assert_eq!(m.own_tab(), Some(11));
        assert!(m.elects_confirmed());
    }

    #[test]
    fn own_tab_is_none_while_the_tab_frame_is_the_fresh_one() {
        // The dossier's own direction: fresh tabs, stale manifest. The witness
        // is symmetric — position-set equality does not care which side lags.
        let mut m = fleet_of_three(10);
        m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        assert_eq!(m.own_tab(), None);
        assert!(!m.elects_confirmed());
    }

    #[test]
    fn identity_effects_emit_nothing_from_an_incoherent_frame_and_retry_on_the_next() {
        // The fail-closed contract end to end — the test RC-A would have
        // failed. Timeline pre-seeded so the birth touch is out of the way.
        let mut m = fleet_of_three(10);
        m.register("u1".into(), 6); // our tab's terminal pane
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, None, 6)],
            &[(10, 100), (11, 100), (12, 100)],
        ));
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        assert_eq!(
            m.identity_effects(),
            Vec::<Effect>::new(),
            "an incoherent frame emits nothing at all"
        );
        m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        // Tab 10 died with the close, so the same settle also prunes it — one
        // coherent frame pair, every identity-keyed effect it authorises.
        assert_eq!(
            m.identity_effects(),
            vec![
                Effect::Bind {
                    uuid: "u1".into(),
                    tab_id: 11
                },
                Effect::PruneTabs {
                    stale_ids: vec![10]
                }
            ]
        );
    }

    #[test]
    fn bind_re_emits_after_an_eviction_once_the_store_seq_advances() {
        // Under the old `sent_binds` latch this returned [] forever: the wrong
        // bind was sticky for the life of the plugin instance, which is what
        // made RC-A a fleet-wide outage rather than a flicker.
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 6);
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, None, 6)],
            &[(11, 100)],
        ));
        assert_eq!(
            m.identity_effects(),
            vec![Effect::Bind {
                uuid: "u1".into(),
                tab_id: 11
            }]
        );
        // The store confirms the join: nothing to send. The attempt history
        // is deliberately NOT dropped on a single confirmation — see
        // `bind_effects`; dropping it there was the unbounded-ping-pong bug.
        m.apply_snapshot(snap_full(
            2,
            vec![agent_at("u1", Status::Working, Some(11), 6)],
            &[(11, 100)],
        ));
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        // Someone else's bind evicts us (store.rs apply_bind) — tab_id goes
        // back to None at a higher seq. We must fight back.
        m.apply_snapshot(snap_full(
            3,
            vec![agent_at("u1", Status::Working, None, 6)],
            &[(11, 100)],
        ));
        assert_eq!(
            m.identity_effects(),
            vec![Effect::Bind {
                uuid: "u1".into(),
                tab_id: 11
            }]
        );
    }

    #[test]
    fn bind_is_silent_while_its_own_write_is_in_flight() {
        // The debounce. `seq` advances only on a real store mutation, so
        // frames, renders, pipes and timers cost nothing — this is what keeps
        // the retry out of the C5 rd-4 echo-gated storm class.
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 6);
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, None, 6)],
            &[(11, 100)],
        ));
        assert_eq!(m.identity_effects().len(), 1);
        for _ in 0..20 {
            // A burst of frames at an unadvanced seq: our `clave bind` is
            // still in flight and must not be re-spawned.
            m.apply_panes(panes_at(&FLEET_PANES));
            assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        }
    }

    #[test]
    fn bind_stops_after_bind_max_tries_against_an_unwinnable_target() {
        // The anti-storm bound. The one loop that DOES advance seq every round
        // is eviction ping-pong — two agents whose panes both resolve into one
        // tab, each bind evicting the other. Wrong-but-consistent beats a
        // storm, so we stop fighting after BIND_MAX_TRIES.
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 6);
        let mut emitted = 0;
        for seq in 1..=20 {
            m.apply_snapshot(snap_full(
                seq,
                vec![agent_at("u1", Status::Working, None, 6)],
                &[(11, 100)],
            ));
            emitted += m.identity_effects().len();
        }
        assert_eq!(emitted, BIND_MAX_TRIES as usize);
    }

    #[test]
    fn a_confirmation_does_not_refresh_an_exhausted_attempt_budget() {
        // The counterpart to the ping-pong test, stated as a unit: once a
        // target's budget is spent, a confirming snapshot does NOT hand it
        // back. Confirmations are exactly what an eviction fight produces, so
        // treating one as "episode over, start again" is what made the cap
        // unbounded in the first cut (Codex P1 on PR #120).
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 6);
        for seq in 1..=20 {
            m.apply_snapshot(snap_full(
                seq,
                vec![agent_at("u1", Status::Working, None, 6)],
                &[(11, 100)],
            ));
            m.identity_effects();
        }
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        // A confirming snapshot: nothing to send, and the history is kept.
        m.apply_snapshot(snap_full(
            21,
            vec![agent_at("u1", Status::Working, Some(11), 6)],
            &[(11, 100)],
        ));
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        // A later divergence against the SAME target stays silent — we have
        // already lost this fight four times and wrong-but-consistent beats a
        // storm.
        let mut emitted = 0;
        for seq in 22..=40 {
            m.apply_snapshot(snap_full(
                seq,
                vec![agent_at("u1", Status::Working, None, 6)],
                &[(11, 100)],
            ));
            emitted += m.identity_effects().len();
        }
        assert_eq!(emitted, 0);
        // But a genuinely NEW target is a new fight, at full budget — the
        // budget tracks a contested tab, not the agent.
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(99, 1, "b", true),
            tab(12, 2, "c", false),
        ]);
        // (tab 99 is new to the timeline, so it earns its birth stamp too.)
        assert_eq!(
            m.identity_effects(),
            vec![
                Effect::Touch { tab_id: 99 },
                Effect::Bind {
                    uuid: "u1".into(),
                    tab_id: 99
                },
                // 11 left the tab set when 99 replaced it at that position.
                Effect::PruneTabs {
                    stale_ids: vec![11]
                }
            ]
        );
    }

    #[test]
    fn two_agents_contesting_one_tab_cannot_ping_pong_forever() {
        // The exact scenario BIND_MAX_TRIES exists to bound, and the one the
        // first cut of this code did NOT bound (Codex P1 on PR #120): two
        // agents whose registered panes both resolve into our tab. Each bind
        // evicts the other (`store.rs` apply_bind) and pushes a snapshot
        // CONFIRMING the new winner — so if confirmation cleared the ledger,
        // every agent's counter would be wiped on the round it won and it
        // would re-enter the fight at tries=1, forever. Unbounded subprocess
        // loop; the C5 rd-4 fd-exhaustion class.
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 6);
        m.register("u2".into(), 6); // same pane → both resolve into tab 11
        let mut emitted = 0;
        let mut winner_is_u1 = true;
        for seq in 1..=40 {
            // The store: whoever bound last holds 11, the other is evicted.
            let (a, b) = if winner_is_u1 {
                (Some(11), None)
            } else {
                (None, Some(11))
            };
            m.apply_snapshot(snap_full(
                seq,
                vec![
                    agent_at("u1", Status::Working, a, 6),
                    agent_at("u2", Status::Working, b, 6),
                ],
                &[(11, 100)],
            ));
            emitted += m.identity_effects().len();
            // Frames keep arriving BETWEEN store pushes. They must not be able
            // to complete a run of confirmations: only a real store advance
            // counts, or a burst of PaneUpdates would refund the budget at the
            // same seq and the cap would break by a second route.
            for _ in 0..3 {
                m.apply_panes(panes_at(&FLEET_PANES));
                emitted += m.identity_effects().len();
            }
            winner_is_u1 = !winner_is_u1;
        }
        // Both agents' budgets are finite and never refreshed by the
        // confirmations they win, so the whole episode is bounded.
        assert!(
            emitted <= 2 * BIND_MAX_TRIES as usize,
            "ping-pong emitted {emitted} binds — the cap is not holding"
        );
    }

    #[test]
    fn a_bind_that_keeps_healing_is_never_starved_of_budget() {
        // The mirror of the ping-pong bound, and the reason the reset needs a
        // threshold rather than being removed outright (adversarial review, PR
        // #120). An eviction changes neither our pane nor our target, so if a
        // confirmation never refunded the budget, four LIFETIME evictions
        // would silence a correct bind for the life of the plugin instance —
        // the old `sent_binds` permanent latch, reached slowly. A HELD
        // confirmation must hand the budget back.
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 6);
        let mut seq = 0;
        for cycle in 0..(BIND_MAX_TRIES + 4) {
            // Evicted: we must fight for our tab again.
            seq += 1;
            m.apply_snapshot(snap_full(
                seq,
                vec![agent_at("u1", Status::Working, None, 6)],
                &[(11, 100)],
            ));
            assert_eq!(
                m.identity_effects(),
                vec![Effect::Bind {
                    uuid: "u1".into(),
                    tab_id: 11
                }],
                "cycle {cycle}: a healed bind was starved of budget"
            );
            // The bind lands and HOLDS across successive store advances.
            for _ in 0..BIND_CONFIRMS_TO_RESET {
                seq += 1;
                m.apply_snapshot(snap_full(
                    seq,
                    vec![agent_at("u1", Status::Working, Some(11), 6)],
                    &[(11, 100)],
                ));
                assert_eq!(m.identity_effects(), Vec::<Effect>::new());
            }
        }
    }

    #[test]
    fn bind_ledger_clears_when_the_pane_leaves_our_tab() {
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 6);
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, None, 6)],
            &[(11, 100)],
        ));
        assert_eq!(m.identity_effects().len(), 1);
        // The agent's pane moves to another tab: not ours to bind, ledger
        // cleared. When it comes back, the budget is full again.
        m.register("u1".into(), 7);
        m.apply_snapshot(snap_full(
            2,
            vec![agent_at("u1", Status::Working, None, 7)],
            &[(11, 100)],
        ));
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        m.register("u1".into(), 6);
        let mut emitted = 0;
        for seq in 3..=20 {
            m.apply_snapshot(snap_full(
                seq,
                vec![agent_at("u1", Status::Working, None, 6)],
                &[(11, 100)],
            ));
            emitted += m.identity_effects().len();
        }
        assert_eq!(emitted, BIND_MAX_TRIES as usize);
    }

    #[test]
    fn birth_touch_is_deferred_not_consumed_when_the_frames_disagree() {
        // The `&&` short-circuit was always an intentional property — a false
        // gate must DEFER the once-ever latch, not spend it. What it lacked
        // was a trigger: the block lived only in the TabUpdate arm, and a
        // close delivers exactly one TabUpdate to the newly-active instance —
        // the one where the frames disagree. `identity_effects` runs on both
        // frame kinds, so the resolving PaneUpdate is now the retry.
        let mut m = fleet_of_three(10);
        m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        assert_eq!(
            m.identity_effects(),
            Vec::<Effect>::new(),
            "incoherent: no touch, and the latch is NOT spent"
        );
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        assert_eq!(m.identity_effects(), vec![Effect::Touch { tab_id: 11 }]);
        // Once-EVER: never a second time, however many frames arrive.
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
    }

    #[test]
    fn birth_touch_targets_our_own_tab_only() {
        // The old adapter touched the ACTIVE id from the tab frame while
        // gating on a position join against the pane frame — RC-A's shape, one
        // line away. Requiring active == own makes it self-consistent.
        let mut m = fleet_of_three(12); // coherent, but tab 12 is active
        assert!(!m.elects_confirmed());
        // Not a coherence failure — the frames agree. We are simply not the
        // active instance, which the weak gate agrees about.
        assert!(m.frames_coherent());
        assert!(!m.elects_presumed());
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
    }

    #[test]
    fn a_new_target_starts_a_fresh_episode_without_waiting_for_the_store() {
        // zellij REUSES tab ids (get_new_tab_id = max-key+1), so the tab at
        // our position can become a DIFFERENT tab without our pane moving.
        // That is a new target, not a retry of the old one, so it must not
        // inherit the old episode's debounce — it emits immediately, at the
        // same seq, with a full attempt budget.
        let mut m = BarModel::default();
        m.set_own_pane(101);
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        m.register("u1".into(), 6);
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, None, 6)],
            &[(10, 100), (11, 100), (99, 100)],
        ));
        assert_eq!(
            m.identity_effects(),
            vec![Effect::Bind {
                uuid: "u1".into(),
                tab_id: 11
            }],
            // 99 is seeded in the timeline but not in the tab set yet — and
            // it ARRIVES two lines down, which is exactly why it must not be
            // pruned here: an id this instance never witnessed dying is a
            // tab created beyond a starved frame's reach, and pruning it is
            // the newborn-revert the 2026-08-17 QA drive caught live
            // (this assertion expected PruneTabs{[99]} until then).
        );
        // Same seq, same pane, new tab id at our position.
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(99, 1, "b", true)]);
        assert_eq!(
            m.identity_effects(),
            vec![
                Effect::Bind {
                    uuid: "u1".into(),
                    tab_id: 99
                },
                Effect::PruneTabs {
                    stale_ids: vec![11]
                }
            ],
            "a new target must not wait behind the old target's debounce"
        );
    }

    #[test]
    fn bind_effects_called_directly_refuse_an_incoherent_frame() {
        // `identity_effects` is already gated; this closes the direct-call
        // path so the guard is not a hole.
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 6);
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, None, 6)],
            &[(11, 100)],
        ));
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        assert_eq!(m.bind_effects(11), Vec::<Effect>::new());
        // ...and the production path says so, which is the point of #178's
        // breadcrumb: `identity_effects` returns on the unresolved own tab
        // before it ever reaches the bind pass, so a report computed inside
        // that pass could never describe this frame (#184 review, found by two
        // reviewers independently).
        assert_eq!(
            m.bind_stall_report(),
            Some(Effect::BindStall {
                state: BindStallState::FramesIncoherent,
                stranded: 0,
                own_tab: None,
            })
        );
    }

    #[test]
    fn the_bind_stall_breadcrumb_fires_on_change_only_and_clears_when_the_leg_recovers() {
        // #178's whole difficulty is that the silent path is silent. This is
        // the contract of the breadcrumb that ends that: one line per state
        // CHANGE (a starved bar must not spam a shared log), and a closing
        // line when the leg comes back, so a log can be read as episodes.
        // Driven through the PRODUCTION entry point, not the bind pass.
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 9); // announced pane, absent from our frame
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, None, 9)],
            &[(11, 100)],
        ));
        assert_eq!(
            m.bind_stall_report(),
            Some(Effect::BindStall {
                state: BindStallState::PaneKnownButAbsent,
                stranded: 1,
                own_tab: Some(11),
            })
        );
        // Steady state is quiet, however many frames arrive.
        assert_eq!(m.bind_stall_report(), None);
        assert_eq!(m.bind_stall_report(), None);
        // The pane frame finally reaches us: the leg recovers and says so
        // ONCE, and the bind it was owed lands on the same settle.
        let mut panes = panes_at(&FLEET_PANES);
        panes.push(pane(1, 9, false, false));
        m.apply_panes(panes);
        assert_eq!(
            m.bind_stall_report(),
            Some(Effect::BindStall {
                state: BindStallState::Cleared,
                stranded: 0,
                own_tab: Some(11),
            })
        );
        assert_eq!(m.bind_stall_report(), None);
        assert_eq!(
            m.identity_effects(),
            vec![Effect::Bind {
                uuid: "u1".into(),
                tab_id: 11,
            }]
        );
    }

    #[test]
    fn a_snapshot_pane_id_binds_an_instance_that_never_heard_the_register() {
        // #178 itself. A tab born by a wake is not running when its own
        // `clave-register` broadcast fires, so it never learns its pane — and
        // the instances that DID hear are background ones that cannot see a
        // pane outside their tab. Nobody could compute the bind. Carrying the
        // pane on the snapshot closes it: no `register()` call here at all.
        let mut m = fleet_of_three(11);
        let mut a = agent("u1", Status::Working, None);
        a.pane_id = Some(6); // the terminal pane in tab 11, per FLEET_PANES
        m.apply_snapshot(snap_full(1, vec![a], &[(11, 100)]));
        assert_eq!(
            m.identity_effects(),
            vec![Effect::Bind {
                uuid: "u1".into(),
                tab_id: 11,
            }],
            "the snapshot is the one view every instance agrees on"
        );
        // And it is not mistaken for a stall on the way: the leg never went
        // quiet, so there is no episode to report.
        assert_eq!(m.bind_stall_report(), None);
    }

    #[test]
    fn a_snapshot_pane_in_another_tab_still_binds_nothing_here() {
        // The mirror: hydrating from the snapshot must not let an instance
        // bind a pane that is not in ITS tab. Pane 7 lives in tab 12.
        let mut m = fleet_of_three(11);
        let mut a = agent("u1", Status::Working, None);
        a.pane_id = Some(7);
        m.apply_snapshot(snap_full(1, vec![a], &[(11, 100)]));
        assert!(
            !m.identity_effects()
                .iter()
                .any(|e| matches!(e, Effect::Bind { .. })),
            "a pane outside our tab is another instance's bind to make"
        );
    }

    #[test]
    fn a_snapshot_without_the_pane_retires_one_learned_from_the_broadcast() {
        // #187. The map had no removal path on any version: a pane learned from
        // the `clave-register` broadcast outlived the pane itself. Left set, it
        // is not inert — it fakes a PaneKnownButAbsent episode in the #184
        // breadcrumb, which would mask a real one, and it leaves a uuid jump
        // aimed at a pane that is gone. The snapshot is authoritative, so a row
        // carrying no pane retires the cached mapping.
        let mut m = fleet_of_three(11);
        m.register("u1".into(), 9); // announced, absent from our pane frame
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, None, 9)],
            &[(11, 100)],
        ));
        assert_eq!(
            m.bind_stall_report(),
            Some(Effect::BindStall {
                state: BindStallState::PaneKnownButAbsent,
                stranded: 1,
                own_tab: Some(11),
            })
        );
        assert_eq!(
            m.nav("{\"uuid\":\"u1\"}", None),
            vec![Effect::FocusPane { pane_id: 9 }]
        );
        // The tab closes. The store clears the row's pane alongside its tab
        // (#185), so the next snapshot carries none — and the entry we learned
        // from the broadcast goes with it.
        m.apply_snapshot(snap_full(
            2,
            vec![agent("u1", Status::Idle, None)],
            &[(11, 100)],
        ));
        assert_eq!(
            m.nav("{\"uuid\":\"u1\"}", None),
            Vec::<Effect>::new(),
            "a retired pane must never be a nav target"
        );
        assert_eq!(
            m.bind_stall_report(),
            Some(Effect::BindStall {
                state: BindStallState::Cleared,
                stranded: 0,
                own_tab: Some(11),
            }),
            "the breadcrumb must stop claiming a stall that only a stale entry made"
        );
    }

    #[test]
    fn a_stale_snapshot_retires_nothing() {
        // The retirement rides the seq gate, not the arrival: an out-of-order
        // snapshot is discarded whole (§5), so it cannot strip a mapping the
        // store has since re-asserted.
        let mut m = fleet_of_three(11);
        m.apply_snapshot(snap_full(
            5,
            vec![agent_at("u1", Status::Working, None, 6)],
            &[(11, 100)],
        ));
        m.apply_snapshot(snap_full(
            4,
            vec![agent("u1", Status::Working, None)],
            &[(11, 100)],
        ));
        assert_eq!(
            m.nav("{\"uuid\":\"u1\"}", None),
            vec![Effect::FocusPane { pane_id: 6 }]
        );
    }

    #[test]
    fn the_register_broadcast_binds_a_row_the_snapshot_carried_without_a_pane() {
        // The bridge #187 left standing, and the ONLY test that holds it. Every
        // other bind test puts the pane on the snapshot row, so neutering
        // `register()` fails none of them — the announcement is what closes the
        // interval between a spawn and the store's echo, and without this test
        // that leg could be deleted silently.
        let mut m = fleet_of_three(11);
        m.apply_snapshot(snap_full(
            1,
            vec![agent("u1", Status::Working, None)],
            &[(11, 100)],
        ));
        assert!(
            !m.identity_effects()
                .iter()
                .any(|e| matches!(e, Effect::Bind { .. })),
            "no pane anywhere yet: nothing to bind to"
        );
        m.register("u1".into(), 6); // pane 6 is tab 11's terminal, per FLEET_PANES
        assert_eq!(
            m.identity_effects(),
            vec![Effect::Bind {
                uuid: "u1".into(),
                tab_id: 11,
            }],
            "the announcement must bind on its own, before any store echo"
        );
    }

    #[test]
    fn a_dormant_row_that_was_never_woken_is_not_a_bind_stall() {
        // The false positive that would make the breadcrumb worthless: every
        // healthy fleet carries unbound rows. Only a row whose pane was
        // ANNOUNCED and cannot be placed counts.
        let mut m = fleet_of_three(11);
        m.apply_snapshot(snap_full(
            1,
            vec![
                agent("dormant-1", Status::Idle, None),
                agent("dormant-2", Status::Idle, None),
            ],
            &[(11, 100)],
        ));
        assert!(
            !m.identity_effects()
                .iter()
                .any(|e| matches!(e, Effect::BindStall { .. })),
            "unbound rows with no announced pane are ordinary dormancy"
        );
    }

    #[test]
    fn nav_is_executor_gated_walks_display_rows_clicks_and_uuid_jumps() {
        let mut m = one_line_bar(BarModel::default());
        m.apply_tabs(vec![
            tab(10, 0, "a", true),
            tab(11, 1, "b", false),
            tab(12, 2, "c", false),
        ]);
        m.apply_snapshot(snap_t(1, &[(12, 1000)])); // display: c, then a/b by position tie-break
        // Row jumps and dir walks run ONLY on the executor (the active
        // instance — fresh tab set, and the very bar the user is reading);
        // non-executors stay silent — broadcast execution over stale sets
        // raced six divergent targets live (C5 round 2).
        assert_eq!(m.nav("{\"row\":1}", None), Vec::<Effect>::new());
        assert_eq!(
            m.nav("{\"row\":1}", Some(10)),
            vec![
                Effect::SwitchTab { position: 2 },
                Effect::AnnounceVisit { tab_id: 12 }
            ]
        );
        // dir steps the DISPLAYED list from the executor's own row, wrapping.
        // Display is c(12), a(10), b(11); from own=12: next → a, prev → b.
        assert_eq!(
            m.nav("{\"dir\":\"next\"}", Some(12)),
            vec![
                Effect::SwitchTab { position: 0 },
                Effect::AnnounceVisit { tab_id: 10 }
            ]
        );
        assert_eq!(
            m.nav("{\"dir\":\"prev\"}", Some(12)),
            vec![
                Effect::SwitchTab { position: 1 },
                Effect::AnnounceVisit { tab_id: 11 }
            ]
        );
        // Walking must not reorder: c is still row 1 after both walks.
        assert_eq!(keys(&m)[0], RowKey::Tab(12));
        // Clicks land on ONE instance (the visible bar): same effect shape.
        assert_eq!(
            m.click(1, TALL_PANE),
            vec![
                Effect::SwitchTab { position: 0 },
                Effect::AnnounceVisit { tab_id: 10 }
            ]
        );
        // uuid jumps run on EVERY instance: the pane id is broadcast truth
        // (clave-register), so all duplicates target the SAME pane.
        m.register("u1".into(), 6);
        assert_eq!(
            m.nav("{\"uuid\":\"u1\"}", None),
            vec![Effect::FocusPane { pane_id: 6 }]
        );
        // Malformed / out-of-range → empty.
        assert_eq!(m.nav("not json", Some(10)), Vec::<Effect>::new());
        assert_eq!(m.nav("{\"row\":9}", Some(10)), Vec::<Effect>::new());
        assert_eq!(m.click(9, TALL_PANE), Vec::<Effect>::new());
    }

    #[test]
    fn a_stale_executor_lands_a_live_pick_by_pane_id_not_position() {
        // The 2026-08-17 QA-drive phase-3 wedge: the elected executor was a
        // starved background bar whose tab frame predated a close, so its
        // position-based SwitchTab aimed one past the end and zellij silently
        // refused it — and because its AnnounceVisit still broadcast, the same
        // stale executor was re-elected on every further press. Positions are
        // frame truth and frames starve (TabUpdate reaches only the active
        // tab); the store's uuid→pane join is broadcast truth. So a live pick
        // on a tab with a registered agent pane must land by pane id.
        let mut m = starved_bar(11);
        m.apply_snapshot(snap_full(
            1,
            vec![agent_at("u1", Status::Working, Some(12), 7)],
            &[(12, 1000)],
        ));
        assert_eq!(
            m.nav("{\"row\":1}", Some(11)),
            vec![
                Effect::FocusPane { pane_id: 7 },
                Effect::AnnounceVisit { tab_id: 12 },
            ],
            "a bound live pick must ride the position-immune pane id"
        );
    }

    #[test]
    fn a_live_pick_with_no_registered_pane_falls_back_to_position() {
        // The remainder the pane-id landing accepts: a bound row whose pane
        // was never announced (and any plain terminal tab) has no stable id
        // to ride, so position — fresh on the bar the user is reading, the
        // common case — is the only address there is.
        let mut m = starved_bar(11);
        m.apply_snapshot(snap_full(
            1,
            vec![agent("u1", Status::Working, Some(12))],
            &[(12, 1000)],
        ));
        assert_eq!(
            m.nav("{\"row\":1}", Some(11)),
            vec![
                Effect::SwitchTab { position: 2 },
                Effect::AnnounceVisit { tab_id: 12 },
            ]
        );
    }

    #[test]
    fn tab_close_reanchors_the_stranded_beacon() {
        // #23 live finding (day one of v0.1.0, 2026-07-21): closing the focused
        // tab makes zellij focus a survivor and send ITS bar a TabUpdate, while
        // the replicated beacon (current_tab) still names the CLOSED tab.
        // Executor election keys on current_tab == own live tab
        // (`nav_executor`), so a stranded beacon matches NO instance and
        // Alt+↑/↓ goes dead until a mouse click reseeds it. apply_tabs must
        // re-anchor to the post-close active tab — via a DISTINCT effect the
        // model elects itself to send (#162; birth stays ungated).
        //
        // The frame pair matters since #162: we are the bar in tab 10 at
        // position 0 and the tab that closes sits BELOW us, so the manifest is
        // still true of us and the election passes on the close's own frame.
        let mut m = BarModel::default();
        m.set_own_pane(100);
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        // Birth on tab 11 (active): announces once via the PLAIN (ungated)
        // AnnounceVisit — birth's ungated announce is live-validated. c_tab=11.
        let fx = m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        assert!(fx.contains(&Effect::AnnounceVisit { tab_id: 11 }));
        assert_eq!(m.current_tab(), Some(11));
        // Tab 11 (the user's focused tab) closes; zellij focuses the survivor
        // (10) and delivers THIS now-active bar a TabUpdate lacking 11. The
        // stranded re-anchor emits a DISTINCT effect (ReanchorVisit) that
        // apply_tabs itself gates to the elected instance (#162: the election
        // moved out of run_effects) — toggle bursts deliver the fresh set to
        // ALL instances (doc:371-394), so an ungated announce here would be a
        // beacon war (round-13 EMFILE class).
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true)]);
        assert!(
            fx.contains(&Effect::ReanchorVisit { tab_id: 10 }),
            "a stranded beacon must re-anchor via the GATED ReanchorVisit"
        );
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::AnnounceVisit { .. })),
            "the stranded path must NOT emit the ungated AnnounceVisit"
        );
        assert_eq!(m.current_tab(), Some(10));
        // Bounded: a further TabUpdate with the same (now-consistent) beacon
        // must NOT re-announce (no round-11 storm) — the local current_tab
        // mutation self-clears `stranded` per instance, so even a hidden bar
        // that trips it during a burst fires at most once, and only the active
        // one actually pipes.
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true)]);
        assert!(
            fx.iter().all(|e| !matches!(
                e,
                Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
            )),
            "re-anchor must not re-fire once the beacon is live again"
        );
    }

    #[test]
    fn a_refused_reanchor_keeps_its_trigger_and_re_emits_on_the_next_frame() {
        // #162: the re-anchor may only be emitted by an instance that has
        // ALREADY elected itself to send it, because the beacon it consumes
        // moves at the same moment. Before that, the close's TabUpdate spent
        // `stranded` locally whether or not the pipe ran — and with the
        // announcing bar dead there was no other trigger left, so nav stayed
        // dead for the life of the session (the "narrow window" the old
        // comment here promised is terminal once the announcer is the tab that
        // died).
        //
        // The repro is the dossier fleet: we are the bar in tab 11 (position
        // 1), the user is on tab 10 (position 0), and closing tab 10
        // renumbers us — so the still-stale manifest joins us to tab 12, the
        // election refuses, and the pipe would never have run.
        let mut m = fleet_of_three(10);
        assert_eq!(m.current_tab(), Some(10)); // birth announced the focused tab
        let fx = m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        assert!(
            fx.iter().all(|e| !matches!(
                e,
                Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
            )),
            "an un-elected instance must not emit a beacon it cannot send"
        );
        assert_eq!(
            m.current_tab(),
            Some(10),
            "a refused re-anchor must leave the beacon stranded — that IS the retry trigger"
        );
        // The PaneUpdate restores coherence, and it is the frame that pays the
        // debt — the retry is not owed to a NEXT TabUpdate, which zellij may
        // never send, but to the next frame of EITHER kind.
        let fx = m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        assert!(
            fx.contains(&Effect::ReanchorVisit { tab_id: 11 }),
            "the frame that restores coherence must re-emit the re-anchor"
        );
        assert_eq!(m.current_tab(), Some(11));
        // Consumed ON EXECUTION is still consumed, on BOTH retry paths — pane
        // frames are frequent (any pane open, close or focus move in the active
        // tab) and a debt that survived its payment would pipe on every one.
        for fx in [
            m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE)),
            m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]),
        ] {
            assert!(
                fx.iter().all(|e| !matches!(
                    e,
                    Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
                )),
                "no re-fire, no storm"
            );
        }
    }

    #[test]
    fn nav_survives_the_close_that_killed_the_announcing_bar() {
        // #162, second half: the whole point of the retry is that the user can
        // still navigate after the close, and a quiet session may deliver no
        // further TabUpdate at all. So the close's OWN pane frame — the one
        // that ends the frame disagreement, which is in flight the moment
        // positions renumber — re-seeds the beacon, and the election is back to
        // the one rule that cannot be faked by a frozen instance: my tab is the
        // tab the last broadcast named.
        let mut m = fleet_of_three(10);
        m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        assert_eq!(
            m.current_tab(),
            Some(10),
            "the refused re-anchor leaves the beacon on the closed tab"
        );
        assert_eq!(
            m.nav_executor(),
            None,
            "and a stranded beacon elects nobody"
        );
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        assert_eq!(
            m.current_tab(),
            Some(11),
            "the pane frame pays the debt and the beacon is live again"
        );
        assert_eq!(
            m.nav_executor(),
            Some(11),
            "the surviving active bar is the beacon's own instance"
        );
        let executor = m.nav_executor();
        assert_eq!(
            m.nav("{\"dir\":\"next\"}", executor),
            vec![
                Effect::SwitchTab { position: 1 },
                Effect::AnnounceVisit { tab_id: 12 }
            ],
            "nav must not be dead after the announcing bar dies"
        );
        // The landing moved the beacon onto tab 12, which is not ours, so the
        // press now belongs to that instance. The beacon is the whole rule
        // (C5 round 2: local active flags raced six divergent targets).
        assert_eq!(m.current_tab(), Some(12));
        assert_eq!(m.nav_executor(), None);
    }

    #[test]
    fn a_stranded_beacon_elects_only_the_active_instance() {
        // There is no fallback: a stranded beacon elects nobody at all. This
        // pins that plus who may still act — a hidden bar reading a stranded
        // beacon must stay silent, or a broadcast nav is back to racing
        // divergent targets off stale tab sets (C5 round 2).
        let mut m = BarModel::default();
        m.set_own_pane(102); // we are the bar in tab 12, position 2
        m.apply_panes(panes_at(&FLEET_PANES));
        m.apply_tabs(vec![
            tab(10, 0, "a", true),
            tab(11, 1, "b", false),
            tab(12, 2, "c", false),
        ]);
        // Tab 10 closes: the survivor 11 is focused, we are still hidden.
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        assert_eq!(m.current_tab(), Some(10), "the beacon is stranded here too");
        assert_eq!(
            m.nav_executor(),
            None,
            "a hidden bar must not elect itself off a stranded beacon"
        );
        // And a LIVE beacon naming someone else's tab stays authoritative:
        // the press belongs to that instance, not to us.
        m.beacon(11);
        assert_eq!(m.nav_executor(), None);
    }

    #[test]
    fn a_new_tabs_birth_beacon_elects_no_executor_among_starved_bars() {
        // This was the fallback's hard case (C5 round 2) and is now held with
        // no licence at all: beacon-only election refuses starved bars by
        // construction. The reason stranding must be FRAME-WITNESSED rather
        // than re-derived stays live: `beacon_stranded()` asks
        // "does the beacon name a tab outside MY tab set" — and a starved
        // bar's tab set is frozen (FOOTGUNS.md:63), so for it that question
        // cannot tell DEAD from CREATED-SINCE-MY-LAST-FRAME. Creating a tab is
        // the commonest gesture in the product and its newborn broadcasts a
        // birth beacon to every instance, so re-deriving would hand every
        // once-focused hidden bar both halves of the fallback at once: its
        // frozen pair is self-coherent and claims its own tab active
        // (FOOTGUNS.md:64). Three of them would then walk three different tab
        // sets and race three divergent SwitchTab targets — C5 round 2, and
        // the #45/#128 pipe storm behind it.
        let mut bars: Vec<BarModel> = vec![starved_bar(10), starved_bar(11), starved_bar(12)];
        for m in &mut bars {
            // The poison, asserted: every one of them self-diagnoses as active.
            assert!(m.elects_confirmed(), "the frozen pair claims its own tab");
            // A fourth tab is created. Its newborn announces once (ungated,
            // by birth) and that pipe is a BROADCAST: the beacon moves on
            // every instance, while no tab frame reaches any of these three.
            m.beacon(13);
        }
        for m in &mut bars {
            let own = m.own_tab().unwrap();
            assert_eq!(
                m.nav_executor(),
                None,
                "tab {own}'s starved bar must not elect itself off a stranding no frame witnessed"
            );
            let executor = m.nav_executor();
            assert_eq!(
                m.nav("{\"dir\":\"next\"}", executor),
                Vec::<Effect>::new(),
                "tab {own}'s starved bar must not walk its frozen tab set"
            );
        }
        // And the frame that DOES reach them all — a toggle burst hands every
        // instance the fresh set (doc:371-394) — witnesses nothing, because the
        // beacon it arrives with is live in that set. Bar 12 gets the burst's
        // manifest as well, so it is fully coherent and still declines: the
        // press belongs to the bar the beacon names.
        let m = &mut bars[2];
        m.apply_panes(panes_at(&[
            (0, 100, 5),
            (1, 101, 6),
            (2, 102, 7),
            (3, 103, 8),
        ]));
        let fx = m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(11, 1, "b", false),
            tab(12, 2, "c", false),
            tab(13, 3, "d", true),
        ]);
        assert!(
            fx.iter().all(|e| !matches!(
                e,
                Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
            )),
            "a burst frame carrying a live beacon must pipe nothing"
        );
        assert_eq!(m.own_tab(), Some(12), "the burst left it coherent");
        assert_eq!(
            m.nav_executor(),
            None,
            "a live beacon outranks local truth, coherent frames or not"
        );
    }

    #[test]
    fn alt_f_spawns_shows_or_hides_from_the_active_tabs_floating_state() {
        // #207: one press, one executor decision — spawn when the tab has no
        // floating pane, show when one exists hidden, hide when visible. The
        // decision lives here so a test can reach it (#162 template), and the
        // acting instance is the beacon-named one, same rule as `clave-nav`.
        let mut m = fleet_of_three(11);
        m.beacon(11); // the user is looking at our tab
        assert_eq!(
            m.shell_toggle(),
            vec![Effect::ShellSpawn { cwd: None }],
            "no floating pane on the active tab: the press must spawn the shell"
        );
        // A floating pane now exists on our tab (position 1) but the tab frame
        // says the floating set is not visible — the post-hide state.
        let mut panes = panes_at(&FLEET_PANES);
        panes.push(PaneMeta {
            tab_position: 1,
            pane_id: 200,
            is_plugin: false,
            is_focused: false,
            is_floating: true,
            terminal_command: None,
            exited: false,
            exit_status: None,
        });
        m.apply_panes(panes);
        assert_eq!(
            m.shell_toggle(),
            vec![Effect::ShellShow],
            "a hidden floating pane must be shown, NOT respawned — respawn here \
             is #207's stacking bug reborn"
        );
        // The tab frame now reports the floating set visible.
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            TabMeta {
                floating_visible: true,
                ..tab(11, 1, "b", true)
            },
            tab(12, 2, "c", false),
        ]);
        assert_eq!(
            m.shell_toggle(),
            vec![Effect::ShellHide],
            "visible floating panes: the press must hide them"
        );
    }

    #[test]
    fn alt_f_seeds_the_shell_with_the_terminal_tabs_speaker_cwd() {
        // #215: the scratch shell must open where the tab lives, not where
        // the bar was born. A terminal tab's location is its speaker pane's
        // OS-reported cwd (#206 pane facts) — the same truth its row shows.
        let mut m = fleet_of_three(11);
        m.beacon(11);
        m.apply_pane_facts(vec![probe(6, Some("/w/clave"), None)]);
        assert_eq!(
            m.shell_toggle(),
            vec![Effect::ShellSpawn {
                cwd: Some("/w/clave".into())
            }],
            "the speaker pane's cwd must seed the spawn"
        );
        // An empty-but-successful probe must fall back (None → initial_cwd
        // in the adapter), never spawn at "" — same filter as the agent
        // branch (CodeRabbit, PR #222). Fresh model: the first toggle above
        // left a spawn pending, which swallows a second press by design.
        let mut m2 = fleet_of_three(11);
        m2.beacon(11);
        m2.apply_pane_facts(vec![probe(6, Some(""), None)]);
        assert_eq!(m2.shell_toggle(), vec![Effect::ShellSpawn { cwd: None }]);
    }

    #[test]
    fn alt_f_on_an_agent_tab_seeds_from_store_truth() {
        // #215: agent tabs are never probed (#206 — their cwd is store
        // truth), so the spawn reads the snapshot-bound agent's cwd instead.
        let mut m = fleet_of_three(11);
        m.beacon(11);
        m.apply_snapshot(snap(
            1,
            vec![Agent {
                cwd: "/w/clave-wt".into(),
                ..agent("u1", Status::Idle, Some(11))
            }],
        ));
        assert_eq!(
            m.shell_toggle(),
            vec![Effect::ShellSpawn {
                cwd: Some("/w/clave-wt".into())
            }],
            "an agent tab's spawn must open in the agent's store cwd"
        );
    }

    #[test]
    fn a_second_press_before_the_pane_frame_never_spawns_twice() {
        // Key-repeat on Alt+f: presses land faster than zellij's PaneUpdate,
        // so every one of them reads "no floating pane" — without the latch,
        // N presses fan out N shells (CodeRabbit, PR #209). The latch drops
        // presses until ANY pane frame lands, including one that shows no
        // floating pane at all: a failed spawn must leave the key repeatable,
        // not wedged.
        let mut m = fleet_of_three(11);
        m.beacon(11);
        assert_eq!(m.shell_toggle(), vec![Effect::ShellSpawn { cwd: None }]);
        assert_eq!(
            m.shell_toggle(),
            Vec::<Effect>::new(),
            "spawn in flight: a repeat press must be dropped, not re-spawn"
        );
        // The spawn failed — the next manifest still shows no floating pane.
        m.apply_panes(panes_at(&FLEET_PANES));
        assert_eq!(
            m.shell_toggle(),
            vec![Effect::ShellSpawn { cwd: None }],
            "any pane frame reopens the decision; a failed spawn must not \
             wedge Alt+f forever"
        );
        // This time the frame delivers the pane: the latch clears into the
        // normal show/hide table, not another spawn.
        let mut panes = panes_at(&FLEET_PANES);
        panes.push(PaneMeta {
            tab_position: 1,
            pane_id: 200,
            is_plugin: false,
            is_focused: false,
            is_floating: true,
            terminal_command: None,
            exited: false,
            exit_status: None,
        });
        m.apply_panes(panes);
        assert_eq!(m.shell_toggle(), vec![Effect::ShellShow]);
    }

    #[test]
    fn a_starved_bar_never_acts_on_alt_f() {
        // FOOTGUNS.md:63-65: a starved bar's frozen frames are self-coherent
        // and claim its OWN tab active, so `elects_confirmed` is exactly the
        // self-diagnosed "am I active" that lies. Only the replicated beacon
        // can refuse it — and the spawn arm is the one that MUST be exclusive:
        // N acting bars would open N shells (the CLI-twin shape of #207).
        let mut m = starved_bar(12);
        m.beacon(11); // the broadcast says the user is elsewhere
        assert_eq!(
            m.shell_toggle(),
            Vec::<Effect>::new(),
            "the beacon names another tab: a starved bar must not act"
        );
        // A newborn instance (no frames at all) refuses too — fail-closed,
        // a dropped Alt+f is a repeatable keypress.
        let mut newborn = BarModel::default();
        newborn.set_own_pane(101);
        assert_eq!(newborn.shell_toggle(), Vec::<Effect>::new());
    }

    #[test]
    fn a_beaconless_focus_change_never_leaves_two_nav_executors() {
        // The whole reason the re-anchor retries on EITHER frame rather than
        // handing the refusing instance a standing licence to nav on local
        // truth. A licence is armed by a frame that convicted the beacon, and
        // it can only be spent once the instance's PANE frame has restored
        // coherence — so it necessarily outlives the frame that granted it. A
        // NATIVE tab switch (mouse on zellij's tab bar) carries no clave pipe
        // and no beacon, and delivers frames only to the newly active bar, so
        // the former active bar sits frozen mid-licence, still claiming itself
        // active (FOOTGUNS.md:64) — and the new one arms a licence the same way
        // its predecessor did. Two licences, two executors, one keypress, two
        // divergent SwitchTabs: C5 round 2 again.
        //
        // Every beacon each bar emits is fanned to the whole fleet here,
        // because that is what the pipe does — which is exactly how the retry
        // keeps the election exclusive: the survivor's re-anchor lands on the
        // other bars before any of them can convict the beacon themselves.
        let mut bars = vec![fleet_bar(11, 10), fleet_bar(12, 10)];
        // Tab 10 — the announcing tab — closes. Focus falls to tab 11, so only
        // that bar receives the close's frames (C3), and its TabUpdate arrives
        // first, so its own re-anchor is refused off the stale manifest.
        let fx = bars[0].apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        fan_beacons(&mut bars, &fx);
        assert_eq!(
            nav_executors(&bars),
            Vec::<usize>::new(),
            "mid-close, with no coherent frame pair anywhere, nobody may act"
        );
        // The pane frame that restores its coherence is the retry.
        let fx = bars[0].apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        fan_beacons(&mut bars, &fx);
        assert_eq!(
            nav_executors(&bars),
            vec![11],
            "the survivor, alone — nav must be alive after the announcer died"
        );
        // Now the native switch to tab 12. No pipe, no beacon; tab 11's bar
        // hears nothing at all from here on.
        let fx = bars[1].apply_tabs(vec![tab(11, 0, "b", false), tab(12, 1, "c", true)]);
        fan_beacons(&mut bars, &fx);
        let fx = bars[1].apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        fan_beacons(&mut bars, &fx);
        let elected = nav_executors(&bars);
        assert_eq!(
            elected.len(),
            1,
            "one press may move focus once: {elected:?} both claim it"
        );
        // Which one it is, stated rather than left implicit: the beacon's
        // owner, tab 11's bar, even though focus is on tab 12. That is the
        // known cost of a beaconless switch and it is INDEPENDENT of this bug —
        // a native switch always leaves the beacon behind, so the first press
        // after one walks from the old tab and re-anchors on landing. One
        // stale press, never two targets.
        assert_eq!(elected, vec![11]);
        let executor = bars[0].nav_executor();
        assert_eq!(
            bars[0].nav("{\"dir\":\"next\"}", executor),
            vec![
                Effect::SwitchTab { position: 1 },
                Effect::AnnounceVisit { tab_id: 12 }
            ],
            "and its landing re-anchors the fleet onto the tab the user is on"
        );
    }

    #[test]
    fn a_tab_frame_with_no_active_tab_does_not_spend_the_birth_announce() {
        // #162: `birth_announced` was set BEFORE the decision to announce, so
        // a first TabUpdate carrying no active tab — reachable during a close
        // — spent a newborn's one and only announce on nothing. That is why a
        // freshly created tab could not heal a stranded session.
        let mut m = BarModel::default();
        let fx = m.apply_tabs(vec![tab(10, 0, "a", false)]);
        assert!(fx.iter().all(|e| !matches!(
            e,
            Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
        )));
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true)]);
        assert!(
            fx.contains(&Effect::AnnounceVisit { tab_id: 10 }),
            "the newborn's announce must survive an active-less frame"
        );
        // Still once-EVER: spent by the announce it made...
        let fx = m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        assert!(fx.iter().all(|e| !matches!(
            e,
            Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
        )));
        // ...and equally spent by an active tab the beacon ALREADY names. A
        // bar whose beacon arrived before its first frame has nothing to
        // announce, and must not save the UNGATED announce up for a later
        // burst — every instance firing one is the round-11 storm shape.
        let mut m = BarModel::default();
        m.beacon(10);
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(fx.iter().all(|e| !matches!(
            e,
            Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
        )));
        let fx = m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        assert!(
            fx.iter().all(|e| !matches!(
                e,
                Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
            )),
            "a satisfied birth claim must stay spent when the active tab moves"
        );
    }

    #[test]
    fn tab_close_prunes_stale_ids_and_retries_until_echo_clears() {
        // #6/F3: on tab CLOSE the model emits the OBSERVED-STALE ids
        // (bound-or-timelined ids absent from the delivered live set) — NOT the
        // live set, so out-of-order prunes commute (idempotent removes) and a
        // late one can't unbind a tab created after it.
        //
        // Emission is DETECTION-driven, never set-change-gated, and since #55
        // it comes from `identity_effects` rather than `apply_tabs` — which is
        // what gives it a retry. A close is exactly when the two zellij frames
        // disagree, so the prune is refused on the close TabUpdate; the
        // PaneUpdate that restores coherence must re-derive and emit it. Only
        // the store echo clearing the mirror silences it.
        let mut m = BarModel::default();
        m.set_own_pane(100); // our bar sits in tab 10, at position 0
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        let mut s = snap(1, vec![agent("u-b", Status::Working, Some(11))]);
        s.tab_order = [(10usize, 100u64), (11, 200)].into();
        m.apply_snapshot(s);
        assert_eq!(
            m.identity_effects(),
            Vec::<Effect>::new(),
            "all live: no prune"
        );

        // Tab 11 closes ABOVE us. The TabUpdate leads, so the frames disagree.
        m.apply_tabs(vec![tab(10, 0, "a", true)]);
        assert_eq!(
            m.identity_effects(),
            Vec::<Effect>::new(),
            "incoherent frames authorise nothing — the prune included"
        );
        // The resolving PaneUpdate is the retry. Without this the prune is
        // lost outright: the next TabUpdate arrives when a tab is CREATED, and
        // zellij reuses ids, so the dead id would be back in the live set and
        // no longer read as stale — the dead agent's bind and timeline entry
        // would be inherited by the new tab.
        m.apply_panes(panes_at(&[(0, 100, 5)]));
        assert_eq!(
            m.identity_effects(),
            vec![Effect::PruneTabs {
                stale_ids: vec![11]
            }],
            "the frame that restores coherence must re-derive the prune"
        );
        // Still pending in the mirror → still emitted (detection-driven).
        assert_eq!(
            m.identity_effects(),
            vec![Effect::PruneTabs {
                stale_ids: vec![11]
            }]
        );
        // The store's prune echoes back: an EMPTY stale set, so it self-limits.
        m.apply_snapshot(snap_t(2, &[(10, 100)]));
        assert_eq!(
            m.identity_effects(),
            Vec::<Effect>::new(),
            "a clean store must cost no prune subprocess"
        );
    }

    #[test]
    fn a_bar_never_shown_a_tab_must_not_prune_its_newborn_bind() {
        // 2026-08-17 QA-drive P2 rung 1, run 2: `clave add` created tab 1 and
        // its agent registered, touched, and bound — three store writes — and
        // tab 0's bar, background since the create with a tab frame still
        // reading {0}, received the snapshot, derived the bound tab 1 as
        // observed-stale, and pruned the newborn. One write reverted all
        // three, and (per #187) the retired pane mapping meant the row could
        // never bind again. "Observed-stale" must mean OBSERVED: an id above
        // everything this instance has ever been shown was never witnessed
        // alive or dead, and is not this bar's to prune.
        let mut m = BarModel::default();
        m.set_own_pane(100);
        m.apply_panes(panes_at(&[(0, 100, 5)]));
        m.apply_tabs(vec![tab(10, 0, "a", true)]);
        let mut s = snap(1, vec![agent("u-new", Status::Working, Some(11))]);
        s.tab_order = [(10usize, 100u64), (11, 200)].into();
        m.apply_snapshot(s);
        assert!(
            m.identity_effects()
                .iter()
                .all(|e| !matches!(e, Effect::PruneTabs { .. })),
            "a tab this instance never witnessed dying is a newborn, not a corpse"
        );
        // The moment a frame shows tab 11 alive and a later one shows it
        // gone, the same instance's prune is back in business.
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        m.apply_tabs(vec![tab(10, 0, "a", true)]);
        m.apply_panes(panes_at(&[(0, 100, 5)]));
        assert!(
            m.identity_effects().contains(&Effect::PruneTabs {
                stale_ids: vec![11]
            }),
            "a witnessed close must still prune"
        );
    }

    #[test]
    fn a_reused_tab_id_is_a_newborn_not_a_corpse_to_a_starved_bar() {
        // Codex P1 on PR #202: zellij RECYCLES tab ids (FOOTGUNS — closing
        // the highest tab hands its id to the next tab created). A bar that
        // witnessed the old incarnation die retains that id under its
        // high-water mark, so when it is later starved — frames frozen
        // mutually-coherent, still claiming its own tab active — the
        // newborn's broadcast snapshot reads as observed-stale and its fresh
        // register/touch/bind is pruned, permanently (#187). A settled claim
        // must not outlive its own store echo.
        let mut m = BarModel::default();
        m.set_own_pane(100);
        // Tabs 10 (ours, active) and 11 alive together.
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        let mut s = snap(1, vec![agent("u-old", Status::Working, Some(11))]);
        s.tab_order = [(10usize, 100u64), (11, 200)].into();
        m.apply_snapshot(s);
        // Tab 11 — the highest — closes; we witness it and prune, correctly.
        m.apply_tabs(vec![tab(10, 0, "a", true)]);
        m.apply_panes(panes_at(&[(0, 100, 5)]));
        assert!(
            m.identity_effects().contains(&Effect::PruneTabs {
                stale_ids: vec![11]
            }),
            "the witnessed close prunes"
        );
        // The store echo settles the claim: nothing references 11 any more.
        m.apply_snapshot(snap_t(2, &[(10, 100)]));
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        // A new tab is created and zellij hands it id 11 again. Creation
        // focuses the newborn, so THIS bar is starved — no TabUpdate, no
        // PaneUpdate, frozen frames still claiming tab 10 active. Only the
        // snapshot push arrives, carrying the newborn's register/touch/bind.
        let mut s = snap(3, vec![agent("u-reborn", Status::Working, Some(11))]);
        s.tab_order = [(10usize, 100u64), (11, 300)].into();
        m.apply_snapshot(s);
        assert!(
            m.identity_effects()
                .iter()
                .all(|e| !matches!(e, Effect::PruneTabs { .. })),
            "a reused id whose death was already settled is a newborn, not a corpse"
        );
    }

    #[test]
    fn a_claim_still_referenced_by_a_bind_alone_keeps_its_prune_retrying() {
        // The settle condition is a DISJUNCTION over both store surfaces: a
        // witnessed-dead id is spent only when NEITHER an agent bind NOR a
        // tab_order entry references it. An echo that dropped the ordinal but
        // not the bind (a partial prune landing, or an unrelated push racing
        // it) must keep the claim — dropping it there loses the retry and
        // strands the dead bind forever (the #55 missed-action class).
        let mut m = BarModel::default();
        m.set_own_pane(100);
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        let mut s = snap(1, vec![agent("u-old", Status::Working, Some(11))]);
        s.tab_order = [(10usize, 100u64), (11, 200)].into();
        m.apply_snapshot(s);
        // Witness the close.
        m.apply_tabs(vec![tab(10, 0, "a", true)]);
        m.apply_panes(panes_at(&[(0, 100, 5)]));
        assert!(m.identity_effects().contains(&Effect::PruneTabs {
            stale_ids: vec![11]
        }));
        // Echo with the BIND still standing and the ordinal gone: unsettled.
        let mut s = snap(2, vec![agent("u-old", Status::Working, Some(11))]);
        s.tab_order = [(10usize, 100u64)].into();
        m.apply_snapshot(s);
        assert!(
            m.identity_effects().contains(&Effect::PruneTabs {
                stale_ids: vec![11]
            }),
            "a bind-only reference keeps the claim: the prune retries"
        );
    }

    #[test]
    fn birth_and_steady_state_never_prune() {
        // Detection self-limit: silence comes from an EMPTY derived stale set,
        // not a set-change gate. A fresh instance (no binds/timeline yet)
        // derives nothing stale at birth, and an all-live set derives nothing
        // stale on every focus-move — only genuinely-absent ids emit.
        let mut m = BarModel::default();
        m.set_own_pane(100);
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(
            m.identity_effects()
                .iter()
                .all(|e| !matches!(e, Effect::PruneTabs { .. }))
        );
        let mut s = snap(1, vec![agent("u-b", Status::Working, Some(11))]);
        s.tab_order = [(10usize, 100u64), (11, 200)].into();
        m.apply_snapshot(s);
        // Focus moves (active flag flips) but every id is still live: no prune.
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        assert!(
            m.identity_effects()
                .iter()
                .all(|e| !matches!(e, Effect::PruneTabs { .. }))
        );
    }

    #[test]
    fn announces_are_bounded_birth_once_then_organic_only() {
        // Rounds 11–12 postmortem: ANY announce driven by per-instance
        // "am I active" data is poisoned during event bursts (C3: stale
        // sets always claim own-tab-active; render isn't visibility-gated
        // either — the render announce EMFILE-crashed the server). So
        // announces fire only on provably-bounded triggers:
        //   birth   — an instance's first-ever TabUpdate (once per life);
        //   organic — Alt+o's bind pipes clave-organic, arming ONE
        //             announce on the next TabUpdate.
        // We are the bar in tab 10, position 0, with a coherent manifest: since
        // #162 the organic re-anchor elects itself before emitting, so a test
        // that gives it no pane frame is testing a bar that cannot send.
        let mut m = BarModel::default();
        m.set_own_pane(100);
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        // Birth: first TabUpdate announces own-active claim, exactly once.
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(fx.contains(&Effect::AnnounceVisit { tab_id: 10 }));
        // Toggle-burst safety: endless further TabUpdates with the same
        // stale own-active claim announce NOTHING, even with a stale
        // beacon (this exact pattern was the round-11/12 storm).
        m.beacon(11);
        for _ in 0..50 {
            let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
            assert!(
                fx.iter().all(|e| !matches!(
                    e,
                    Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
                )),
                "a disagreeing beacon is not a trigger — only birth, organic and \
                 stranded are"
            );
        }
        // Organic switch (Alt+o): the bind's MessagePlugin arms one
        // announce; the next TabUpdate fires it and disarms. GATED
        // (ReanchorVisit, not AnnounceVisit): the organic pipe broadcasts
        // and a toggle's TabUpdate reaches every instance, so an ungated
        // announce here is one `zellij pipe` subprocess PER BAR — measured
        // live as ~2s of frozen nav per Alt+o (#128, 2026-08-02).
        m.set_organic_pending();
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(fx.contains(&Effect::ReanchorVisit { tab_id: 10 }));
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::AnnounceVisit { .. })),
            "the ungated announce must not fire for an organic switch"
        );
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(fx.iter().all(|e| !matches!(
            e,
            Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
        )));
        // An incoming beacon DISARMS a pending organic announce: the truly
        // active instance already spoke — a stale instance must not answer
        // a leftover flag with poison at its next burst.
        m.set_organic_pending();
        m.beacon(10); // truth arrives (also matches our claim → no-op announce)
        m.beacon(11); // and moves on
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(fx.iter().all(|e| !matches!(
            e,
            Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
        )));
        // A pending organic announce whose claim already MATCHES the beacon
        // stays quiet (nothing to correct).
        m.beacon(10);
        m.set_organic_pending();
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(fx.iter().all(|e| !matches!(
            e,
            Effect::AnnounceVisit { .. } | Effect::ReanchorVisit { .. }
        )));
    }

    #[test]
    fn an_answered_organic_one_shot_is_spent_not_saved() {
        // #162 moves the one-shot's clear onto the branch that emits, so the
        // other exits have to say WHY they may still spend it: a beacon that
        // already names the active tab has nothing to correct, and a birth
        // announce says everything the one-shot wanted said. Saving it instead
        // hands the instance a free announce to spend at some later switch it
        // was never asked about — an unbounded trigger, which is the round-11
        // storm shape.
        //
        // We are the bar in tab 11 (position 1), so the frames elect us exactly
        // when tab 11 is the active one.
        let mut m = BarModel::default();
        m.set_own_pane(101);
        m.apply_panes(panes_at(&[(0, 100, 5), (1, 101, 6)]));
        m.set_organic_pending();
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(
            fx.contains(&Effect::AnnounceVisit { tab_id: 10 }),
            "birth answers first, and ungated"
        );
        // A native switch onto OUR tab with the beacon still on 10. We are the
        // elected instance here, so a saved one-shot WOULD fire.
        let fx = m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::ReanchorVisit { .. })),
            "the birth announce spends the armed one-shot"
        );
        // Now the satisfied exit: armed while the beacon already names the
        // active tab.
        m.beacon(10);
        m.set_organic_pending();
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        let fx = m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::ReanchorVisit { .. })),
            "an agreeing beacon spends the armed one-shot too"
        );
        // And the SEND spends it. The ordinary path hides this, because a
        // re-anchor moves the beacon onto the active tab and the frame after it
        // therefore takes the agreeing exit above — so these are the frames that
        // would let a saved one-shot buy a SECOND pipe: emit once, then a
        // renumber that leaves us elected under a different active tab with the
        // old beacon still live, so the stranded trigger is not in play either.
        // One Alt+o must buy one pipe, burst or no burst.
        m.beacon(10);
        m.set_organic_pending();
        let fx = m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        assert!(fx.contains(&Effect::ReanchorVisit { tab_id: 11 }));
        let fx = m.apply_tabs(vec![tab(11, 0, "b", false), tab(14, 1, "e", true)]);
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::ReanchorVisit { .. })),
            "the one-shot is spent by the pipe it bought, not by the next frame"
        );
    }

    /// A bar of the three-tab fleet whose OWN tab is the focused one. Every
    /// width test needs the focus half — `width_effects` refuses to touch the
    /// layout from a background tab, because zellij would apply the switch to
    /// whichever tab has focus rather than to the one that asked.
    fn focused_bar() -> BarModel {
        let mut m = fleet_bar(11, 11);
        m.beacon(11);
        frame(&mut m, 11);
        assert!(m.own_tab_focused(), "focused_bar is not focused");
        m
    }

    /// The same bar, on a tab the user is not looking at.
    fn background_bar() -> BarModel {
        let mut m = fleet_bar(11, 10);
        m.beacon(10);
        frame(&mut m, 10);
        assert!(!m.own_tab_focused(), "background_bar is focused");
        m
    }

    fn collapsed_model() -> BarModel {
        let mut m = focused_bar();
        m.toggle();
        m
    }

    /// The two widths the layouts declare, verbatim (fixed column counts since
    /// the 2026-08-17 rebuild — the machine compares painted width against
    /// these constants and nothing else). Pinned to the `Double` arm (#232's
    /// shipping default, and every `BarModel::default()` model's mode below)
    /// — a test that needs to pin `Single` builds its own model via
    /// `model_with_row_height(RowHeight::Single)` instead of this pair.
    const EXP_W: usize = RowHeight::Double.target_cols(false);
    const COL_W: usize = RowHeight::Double.target_cols(true);

    /// Test-only constructor for a model whose row-height mode is not the
    /// default (#232) — mirrors production's `set_row_height`, called from
    /// `main.rs::load()` after `resolve_row_height`.
    fn model_with_row_height(row_height: RowHeight) -> BarModel {
        let mut m = BarModel::default();
        m.set_row_height(row_height);
        m
    }

    /// The one width property that matters: **a mode change moves the pane,
    /// and the pane ARRIVING at the mode's width is what ends the episode.**
    ///
    /// Driven through the real entry points: `width_effects` once per render
    /// with the width zellij just painted us at, and `width_cooldown_elapsed`
    /// for the timer main.rs arms per ask — the sequence below is exactly
    /// what a live bar sees. The paint after the toggle still shows the old
    /// width and asks; the paints after the switch land while the ask's
    /// cooldown holds, and the expiry finds the target width and rests.
    #[test]
    fn a_toggle_asks_on_the_first_paint_and_the_target_width_ends_it() {
        let mut m = focused_bar();
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        m.toggle(); // wants collapsed
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        m.toggle(); // and back
        assert_eq!(m.width_effects(Some(COL_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
    }

    /// The swap cycle hides the tab's birth layout as a position of its own
    /// (FOOTGUNS), so one switch can land on a DIFFERENT position with the
    /// SAME width. The cooldown expiry sees the width unchanged and asks
    /// again rather than conclude it already asked.
    #[test]
    fn a_no_move_landing_is_asked_again_from_the_same_width() {
        let mut m = focused_bar();
        m.toggle(); // wants collapsed
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        // The switch traded one expanded-width position for another
        // (birth → declared expanded): same width, new paint, judged deaf —
        // the expiry spends the second ask.
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
    }

    /// A paint that arrives while an ask's cooldown holds buys NOTHING —
    /// however many arrive, whatever they claim. This is the regression the
    /// QA drive filmed (2026-08-17): a swap's queued repaints echo the
    /// pre-swap width, and a machine that judged them spent three asks
    /// inside a millisecond, lapped the whole swap cycle, and re-armed
    /// itself off its own paint wake — expand/collapse at paint speed,
    /// forever. Replayed verbatim from the live trace.
    #[test]
    fn the_paint_echo_burst_buys_one_ask_and_the_expiry_ends_the_episode() {
        let mut m = focused_bar();
        m.toggle(); // wants collapsed
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        // The live trace: five echoes of the pre-swap width, then the true
        // landing, all inside one cooldown. Every one of them judged
        // nothing.
        for _ in 0..5 {
            assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        }
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        // The expiry judges the LATEST width — the true landing — and
        // rests. One press, one ask, episode over.
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
    }

    /// #208: while this tab's floating set is visible, zellij routes a
    /// swap-layout switch to the FLOATING ring, not the tiled one
    /// (`Tab::next_swap_layout`, zellij-server 0.44.3 tab/mod.rs:1151) — an
    /// ask cannot move this pane, only spend the walk budget against the
    /// shell's layer. So paints under a shown shell judge nothing and spend
    /// nothing; the TabUpdate that reports the hide repaints, and THAT paint
    /// spends the first ask.
    #[test]
    fn a_visible_floating_set_holds_the_ask_and_the_hide_releases_it() {
        let mut m = focused_bar();
        // The Alt+f shell is shown on the bar's own tab.
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            TabMeta {
                floating_visible: true,
                ..tab(11, 1, "b", true)
            },
            tab(12, 2, "c", false),
        ]);
        m.toggle(); // wants collapsed
        // However many paints arrive while the shell is up, none asks and
        // none spends budget — the pre-fix machine burned all three asks
        // here, each one relayouting the floating ring.
        for _ in 0..4 {
            assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        }
        // Alt+f hides the shell; the TabUpdate repaints and judgement
        // resumes with the full budget.
        frame(&mut m, 11);
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
    }

    /// A width zellij cannot produce (a window too narrow for the target)
    /// must not become a spin: [`WALK_ASK_CAP`] cooldown-spaced asks walk
    /// the whole cycle, and an intent still unmet after them is conceded —
    /// the bar rests wherever the walk stopped until the intent changes.
    /// (The round-20 "wherever cols stop changing" ruling.)
    #[test]
    fn a_width_zellij_cannot_produce_is_asked_thrice_then_rested() {
        let mut m = focused_bar();
        m.toggle(); // wants collapsed; the window can only paint 47
        assert_eq!(m.width_effects(Some(47)), vec![Effect::SwapWidth]);
        for _ in 1..WALK_ASK_CAP {
            assert_eq!(m.width_cooldown_elapsed(), vec![Effect::SwapWidth]);
        }
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new(), "rested");
        assert_eq!(m.width_effects(Some(47)), Vec::<Effect>::new(), "still");
    }

    /// The budget is per INTENT, not per width: a walk whose positions keep
    /// the width moving without ever producing the target must still rest
    /// after [`WALK_ASK_CAP`] asks. Movement re-arming the budget was the
    /// second half of the filmed runaway — on a window that can paint both
    /// 47 and 30 but never 54, each landing re-armed the other's counter and
    /// the walk cycled at the cooldown's cadence, forever.
    #[test]
    fn a_walk_that_keeps_moving_without_landing_rests_after_its_budget() {
        let mut m = collapsed_model();
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        m.toggle(); // wants expanded; the window cannot hold 54
        assert_eq!(m.width_effects(Some(COL_W)), vec![Effect::SwapWidth]);
        for paint in [47, COL_W] {
            assert_eq!(m.width_effects(Some(paint)), Vec::<Effect>::new());
            assert_eq!(m.width_cooldown_elapsed(), vec![Effect::SwapWidth]);
        }
        // Three spent without a landing: the walk rests, moving or not.
        assert_eq!(m.width_effects(Some(47)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new(), "rested");
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new(), "still");
    }

    /// A stale pre-resize paint can drain from the pipeline AFTER the
    /// episode already ended at the target (a landing clears the budget).
    /// The machine cannot tell it from a real deviation and spends an ask —
    /// a visible flap at worst, never a wedge: the walk re-converges and
    /// rests, and the flap cannot re-arm itself because every follow-up
    /// judgement waits out a cooldown.
    #[test]
    fn a_stale_paint_after_a_landing_costs_a_flap_and_converges() {
        let mut m = focused_bar();
        m.toggle(); // wants collapsed
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new(), "landed");
        // The stale echo, arriving at rest: one ask, fresh budget.
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        // Its walk goes home again and the expiry rests.
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
    }

    /// The mode changing re-arms the budget even at an unchanged width: a
    /// new intent deserves a fresh walk.
    #[test]
    fn a_mode_flip_rearms_the_walk_budget() {
        let mut m = focused_bar();
        m.toggle(); // wants collapsed; the pane never moves
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        for _ in 1..WALK_ASK_CAP {
            assert_eq!(m.width_cooldown_elapsed(), vec![Effect::SwapWidth]);
        }
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        m.toggle(); // wants expanded again — and the pane is already there
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        m.toggle(); // wants collapsed: same width, fresh intent
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
    }

    /// A toggle pressed while an ask is in flight is not lost and not
    /// double-served: the paint stays unjudged until the expiry, which
    /// serves the NEW intent.
    #[test]
    fn a_toggle_mid_cooldown_is_served_at_the_expiry() {
        let mut m = focused_bar();
        m.toggle(); // wants collapsed
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        m.toggle(); // changed their mind before the swap settled
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        // The expiry judges the latest paint against the CURRENT intent:
        // expanded, already painted expanded — nothing owed.
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
    }

    /// The peek sink shares the Timer event, so an expiry can reach the
    /// width machine with no ask in flight. It must judge nothing — ending
    /// a deafness that does not exist would be a second entry point into
    /// the ask logic.
    #[test]
    fn a_cooldown_expiry_at_rest_is_inert() {
        let mut m = focused_bar();
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        m.toggle(); // a mismatch exists, but no ask is in flight
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
    }

    /// An untouched bar at its mode's width never asks for anything, however
    /// many times it is repainted. This is the property the old seek's
    /// runaway violated.
    #[test]
    fn an_untouched_bar_never_asks_for_anything() {
        let mut m = focused_bar();
        for _ in 0..8 {
            assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
            frame(&mut m, 11);
        }
    }

    /// A paint that carries no width (`width_effects` from a non-render
    /// caller) decides nothing: only render knows the painted width.
    #[test]
    fn a_frame_without_a_width_decides_nothing() {
        let mut m = focused_bar();
        m.toggle();
        assert_eq!(m.width_effects(None), Vec::<Effect>::new());
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
    }

    /// Zellij resolves a plugin's swap-layout request against the FOCUSED
    /// tab, not the asking one (FOOTGUNS) — so a background bar seeing a
    /// wrong width emits nothing and books nothing.
    #[test]
    fn a_background_bar_never_switches_a_layout_it_does_not_own() {
        let mut m = background_bar();
        m.toggle(); // wants collapsed, painted expanded: a switch is owed
        for _ in 0..4 {
            assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        }
    }

    /// The owed switch fires on the first paint after the tab regains focus —
    /// the disagreement is re-derived from the paint, not replayed from a
    /// queue.
    #[test]
    fn a_held_switch_fires_on_the_paint_after_the_tab_regains_focus() {
        let mut m = background_bar();
        m.toggle();
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new(), "held");
        m.beacon(11);
        frame(&mut m, 11); // the user arrives
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
    }

    /// Intent changing twice while held nets out to nothing: there is no
    /// queue, only an end state, and the end state is the width the pane is
    /// already at.
    #[test]
    fn intent_changing_twice_while_held_costs_nothing() {
        let mut m = background_bar();
        m.toggle();
        m.toggle(); // back to expanded before anyone looked
        m.beacon(11);
        frame(&mut m, 11);
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
    }

    #[test]
    fn a_peek_that_comes_and_goes_in_the_background_owes_nothing() {
        let mut m = background_bar();
        m.toggle(); // collapsed
        m.beacon(11);
        frame(&mut m, 11);
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        m.beacon(10); // the user leaves
        assert!(m.visited(10)); // and navs, which every bar hears
        assert!(m.peek_expired());
        m.beacon(11); // and comes back
        frame(&mut m, 11);
        assert_eq!(
            m.width_effects(Some(COL_W)),
            Vec::<Effect>::new(),
            "a peek nobody saw must not move the pane"
        );
    }

    #[test]
    fn peek_expands_a_collapsed_bar_and_expiry_sinks_it() {
        // Peek-on-nav: while collapsed, any nav (arriving as the replicated
        // clave-visited pipe) briefly expands the bar; ~1s after the last
        // nav it sinks back to the gutter.
        let mut m = collapsed_model();
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        // The nav lands on THIS bar's own tab — the bar the user is now
        // reading, and so the only one entitled to move a pane.
        assert!(m.visited(11), "collapsed bar must arm a peek");
        assert_eq!(m.current_tab(), Some(11)); // still a beacon
        // The peek is a geometry switch like any other since #181 — the bar
        // shows the expanded profile and occupies the expanded pane.
        assert_eq!(m.width_effects(Some(COL_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        // A second nav during the peek re-arms (main.rs counts its timers).
        assert!(m.visited(11));
        // Expiry: sink back to the gutter.
        assert!(m.peek_expired());
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
    }

    #[test]
    fn expanded_bars_ignore_peeks() {
        let mut m = focused_bar();
        assert!(!m.visited(11), "expanded bar must not arm a peek");
        assert_eq!(m.current_tab(), Some(11)); // beacon still lands
        // And asked for no geometry switch: a beacon that corrupted
        // `collapsed` would have booked one.
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
    }

    #[test]
    fn toggle_cancels_a_peek_and_a_late_expiry_is_a_noop() {
        let mut m = collapsed_model();
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        assert!(m.visited(11));
        // Alt+c mid-peek: now genuinely expanded; the peek flag must not
        // survive to fight the user's explicit toggle.
        m.toggle();
        assert!(!m.peek_expired(), "late timer after a toggle is a no-op");
        // One switch, to the expanded geometry, unpoisoned by the peek.
        assert_eq!(m.width_effects(Some(COL_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
    }

    /// Issue #5 path (a), birth-while-collapsed: a bar born after the toggle
    /// (or reborn by a reload — path (b), identical state: fresh default)
    /// missed the broadcast forever. Hydration must come from the snapshot
    /// it fetches at startup — and since the pane is BORN at the persisted
    /// mode's width (fixed cols in the launch layout), hydration finds the
    /// painted width already correct and owes zellij nothing.
    #[test]
    fn snapshot_hydrates_a_newborn_into_collapse() {
        let mut m = focused_bar();
        m.await_hydration();
        let mut s = snap(1, vec![]);
        s.collapsed = true;
        m.apply_snapshot(s);
        assert!(m.collapsed, "snapshot-carried flag did not hydrate");
        assert_eq!(
            m.width_effects(Some(COL_W)),
            Vec::<Effect>::new(),
            "a pane born at the persisted width owes no switch"
        );
        // A newborn whose pane predates the flip (reload mid-toggle) is the
        // one that owes a move, and the paint says so.
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
    }

    /// An instance that missed the toggle broadcast heals from the next
    /// (seq-newer) snapshot push; an instance already in sync is left
    /// byte-untouched (a per-snapshot switch would be a perpetual relayout,
    /// round 11).
    #[test]
    fn snapshot_heals_a_desynced_instance_and_leaves_synced_ones_alone() {
        let mut missed = focused_bar();
        missed.apply_snapshot(snap(1, vec![])); // hydrated expanded, seq 1
        assert_eq!(missed.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        let mut heal = snap(2, vec![]);
        heal.collapsed = true;
        missed.apply_snapshot(heal);
        assert_eq!(
            missed.width_effects(Some(EXP_W)),
            vec![Effect::SwapWidth],
            "a seq-newer contradicting snapshot must move the pane"
        );

        let mut synced = collapsed_model();
        synced.apply_snapshot(snap(1, vec![]));
        // The store echo (seq 1, expanded) must NOT overrule the owed press:
        // the pane keeps walking to the collapsed width.
        assert_eq!(synced.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(synced.width_effects(Some(COL_W)), Vec::<Effect>::new());
    }

    /// Issue #5 durability: a toggle books the write it owes the store and
    /// emits the persist effect carrying the ABSOLUTE new mode (main.rs
    /// executes it on the active instance only).
    #[test]
    fn toggle_emits_the_persist_effect_with_the_absolute_mode() {
        let mut m = BarModel::default();
        assert_eq!(
            m.toggle(),
            vec![Effect::PersistCollapse { collapsed: true }]
        );
        assert_eq!(
            m.toggle(),
            vec![Effect::PersistCollapse { collapsed: false }]
        );
    }

    /// Fix-review MAJOR (issue #5): two rapid toggles spawn two writes with
    /// no arrival-order guarantee — the stale one can win and the store's
    /// push then contradicts the user. The pending ledger keeps USER truth and
    /// re-asserts exactly once.
    ///
    /// **Rule changed by #137.** This test used to assert that a SECOND
    /// contradiction yields to the store — "wrong-but-consistent beats a
    /// ping-pong". Live driving showed that yielding is what let a store one
    /// burst behind overrule a press the user had just made, and the resulting
    /// mode flip-flop drove the width machine fast enough to storm the session.
    /// While a write is owed, the store's flag is now ignored outright: the
    /// user's last press stands until the store catches up. The ping-pong the
    /// old rule feared is prevented instead by contradictions being INERT: they
    /// neither flip the mode nor buy a write, so the amplification loop that
    /// fed the storm has nothing to feed on.
    #[test]
    fn a_contradicting_store_never_overrules_an_owed_press() {
        let mut m = BarModel::default();
        m.apply_snapshot(snap(1, vec![]));
        m.toggle(); // → collapsed, owes true
        m.toggle(); // → expanded, owes false (double Alt+c)
        // The late 'true' write won the race; its push arrives:
        let mut bad = snap(2, vec![]);
        bad.collapsed = true;
        let fx = m.apply_snapshot(bad);
        assert!(!m.collapsed, "first contradiction must keep user truth");
        assert!(
            fx.contains(&Effect::PersistCollapse { collapsed: false }),
            "the owed value must be re-asserted"
        );
        // The re-assert was lost too (pathological): further contradicting
        // pushes change NOTHING — no flip, and no second write.
        let mut bad2 = snap(3, vec![]);
        bad2.collapsed = true;
        let fx = m.apply_snapshot(bad2);
        assert!(
            !m.collapsed,
            "the store overruled a press it had not caught up to"
        );
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::PersistCollapse { .. })),
            "no further re-asserts — one per burst"
        );
        // The debt settles the moment the store agrees, and healing resumes:
        // an instance with nothing owed follows the store as it always did.
        let mut caught_up = snap(4, vec![]);
        caught_up.collapsed = false;
        m.apply_snapshot(caught_up);
        assert!(!m.collapsed);
        let mut heal = snap(5, vec![]);
        heal.collapsed = true;
        m.apply_snapshot(heal);
        assert!(m.collapsed, "a settled ledger must still follow the store");
    }

    /// Issue #5 seq-gate interplay: a STALE snapshot (seq <=) is discarded
    /// wholesale — its collapsed flag included — so it can never fight a
    /// fresher local toggle.
    #[test]
    fn stale_snapshot_cannot_flip_collapse() {
        let mut m = BarModel::default();
        m.apply_snapshot(snap(5, vec![]));
        m.toggle(); // collapsed, locally, after seq 5
        let mut stale = snap(5, vec![]); // same seq: stale by the gate
        stale.collapsed = false;
        m.apply_snapshot(stale);
        assert!(m.collapsed, "a stale snapshot must not undo a local toggle");
    }

    #[test]
    fn stale_instance_orders_and_decorates_from_snapshot_alone() {
        // The round-6 regression test: an instance with NO registers and NO
        // manifest (loaded late, event-starved) must still agree with every
        // other instance on order and glyphs, because both ride the
        // snapshot. Only the tab SET is local — and the executor's is fresh.
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "term", false),
            tab(11, 1, "ag-a", false),
            tab(12, 2, "ag-b", true),
        ]);
        let mut s = snap(
            1,
            vec![
                agent("u1", Status::Working, Some(11)),
                agent("u2", Status::Done, Some(12)),
            ],
        );
        s.tab_order = [(10, 100), (11, 900), (12, 500)].into();
        m.apply_snapshot(s);
        assert_eq!(
            keys(&m),
            vec![RowKey::Tab(11), RowKey::Tab(12), RowKey::Tab(10)]
        );
        assert_eq!(status_at(&m, 0), Some(RowStatus::Working)); // ag-a
        assert_eq!(status_at(&m, 1), Some(RowStatus::Done)); // ag-b
        assert_eq!(status_at(&m, 2), None); // the plain terminal tab
    }

    // --- projecting a Row from an Agent (LEDGER D6, design-lock §2) --------

    /// An agent placed in repo `root` on `branch`, with a title and summary.
    fn dressed(
        uuid: &str,
        root: &str,
        branch: &str,
        title: Option<&str>,
        tab: Option<usize>,
    ) -> Agent {
        let mut a = agent(uuid, Status::Working, tab);
        a.repo_root = root.into();
        a.branch = branch.into();
        a.title = title.map(String::from);
        a.summary = format!("{uuid} is working");
        a
    }

    fn content_at(m: &BarModel, i: usize) -> RowContent {
        m.rows()[i].1.content.clone()
    }

    #[test]
    fn a_row_projects_title_summary_and_the_repo_basename() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "whatever", false)]);
        m.apply_snapshot(snap(
            1,
            vec![dressed(
                "u1",
                "/Users/o/code/clave",
                "main",
                Some("S6-GUT"),
                Some(10),
            )],
        ));
        let RowContent::Agent {
            title,
            repo,
            summary,
            battery,
            ..
        } = content_at(&m, 0)
        else {
            panic!("a bound tab renders as an agent row");
        };
        assert_eq!(title.as_deref(), Some("S6-GUT"));
        // BASENAME, not the path: the repo column is 7 cells (3 collapsed), so
        // a path would render as ellipsis and identify nothing.
        assert_eq!(repo, "clave");
        assert_eq!(summary, "u1 is working");
        // S7 HAS landed (#62) — this fixture just never sets `context_level`,
        // which is the "no reading yet" case, and it must stay a blank cell
        // rather than an invented level. A dormant row is a different thing
        // entirely and renders in full ramp colour.
        assert_eq!(battery, None);
    }

    /// Both halves of the battery reach the row, from one snapshot: the bucketed
    /// level the collapsed profile draws its glyph from, and the raw count the
    /// expanded profile prints (#105). Bucketing stays host-side — the bar must
    /// not re-derive either from the other, which is why this asserts the pair
    /// rather than one of them.
    #[test]
    fn a_row_carries_both_the_battery_level_and_the_token_count() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "whatever", false)]);
        let mut a = agent("u1", Status::Working, Some(10));
        a.context_tokens = Some(105_000);
        a.context_level = Some(7);
        m.apply_snapshot(snap(1, vec![a]));
        let RowContent::Agent {
            battery, tokens, ..
        } = content_at(&m, 0)
        else {
            panic!("a bound tab renders as an agent row");
        };
        assert_eq!(battery, Some(7));
        assert_eq!(tokens, Some(105_000));
    }

    #[test]
    fn provenance_is_three_state_worktree_branch_and_blank_main() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(11, 1, "b", false),
            tab(12, 2, "c", false),
            tab(13, 3, "d", false),
        ]);
        let mut wt = dressed("u-wt", "/r/one", "feature/x", None, Some(10));
        wt.worktree = Some("/r/one-wt".into());
        m.apply_snapshot(snap(
            1,
            vec![
                wt,
                dressed("u-br", "/r/one", "feature/x", None, Some(11)),
                dressed("u-main", "/r/one", "main", None, Some(12)),
                dressed("u-master", "/r/one", "master", None, Some(13)),
            ],
        ));
        let provenance = |i: usize| match content_at(&m, i) {
            RowContent::Agent { provenance, .. } => provenance,
            RowContent::Terminal { .. } => panic!("row {i} is not an agent"),
        };
        // A worktree outranks its branch: the worktree IS the provenance.
        assert_eq!(provenance(0), Provenance::Worktree);
        assert_eq!(provenance(1), Provenance::Branch);
        // Both default-branch names render NOTHING (lock §5.1) — blanking the
        // most common row is what makes the two marked states mean something.
        //
        // `dressed` leaves `default_branch` at None, so this is now specifically
        // the #86 FALLBACK path: an old store row, or a repo whose default git
        // could not name, still gets exactly the answer it got before the field
        // existed. That is the guarantee — never a worse answer, only a better
        // one when the repo supplies it (see the test below).
        assert_eq!(provenance(2), Provenance::Main);
        assert_eq!(provenance(3), Provenance::Main);
    }

    #[test]
    fn provenance_prefers_the_repos_own_default_branch_over_the_name_heuristic() {
        // #86: `main`/`master` are not exhaustive. A repository whose default is
        // `trunk` (or `develop`, or `dev`) had its ORDINARY checkout marked as a
        // branch — the one row design-lock §5.1 requires to be blank — purely on
        // naming convention. The host resolves the real default
        // (`add::resolve_default_branch`) and it rides the snapshot; when it is
        // present it OUTRANKS the name test in both directions.
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(11, 1, "b", false),
            tab(12, 2, "c", false),
            tab(13, 3, "d", false),
        ]);
        let with_default = |uuid: &str, branch: &str, tab: usize| {
            let mut a = dressed(uuid, "/r/trunkrepo", branch, None, Some(tab));
            a.default_branch = Some("trunk".into());
            a
        };
        let mut wt = with_default("u-wt", "trunk", 10);
        wt.worktree = Some("/r/trunkrepo-wt".into());
        m.apply_snapshot(snap(
            1,
            vec![
                wt,
                with_default("u-default", "trunk", 11),
                with_default("u-feature", "feature/x", 12),
                // `main` is NOT special here: in a trunk-default repository it
                // is an ordinary side branch and must be marked as one. This is
                // the assertion the old hardcoded list could never make.
                with_default("u-main", "main", 13),
            ],
        ));
        let provenance = |i: usize| match content_at(&m, i) {
            RowContent::Agent { provenance, .. } => provenance,
            RowContent::Terminal { .. } => panic!("row {i} is not an agent"),
        };
        // A worktree still outranks everything — the worktree IS the provenance.
        assert_eq!(provenance(0), Provenance::Worktree);
        assert_eq!(provenance(1), Provenance::Main, "the repo's real default");
        assert_eq!(provenance(2), Provenance::Branch);
        assert_eq!(
            provenance(3),
            Provenance::Branch,
            "`main` in a trunk-default repo is a side branch"
        );
    }

    #[test]
    fn an_agent_outside_a_repo_takes_the_blank_provenance() {
        // Not in the design, and decided here: an agent outside a repo has no
        // branch, and painting the branch glyph for it would assert a
        // provenance nobody has. The blank cell is the honest one.
        //
        // `"-"` is the value that matters: it is what the HOST actually writes
        // (`clave/src/add.rs:517`, the `git rev-parse --abbrev-ref HEAD`
        // fallback, and `record_branch` for a detached-worktree resume). The
        // empty string is only ever produced by test builders — asserted
        // second, and kept, so those builders stay honest.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", false)]);
        m.apply_snapshot(snap(
            1,
            vec![
                dressed("u-dash", "/r/one", "-", None, Some(10)),
                dressed("u-empty", "/r/one", "", None, Some(11)),
            ],
        ));
        let provenance = |i: usize| match content_at(&m, i) {
            RowContent::Agent { provenance, .. } => provenance,
            RowContent::Terminal { .. } => panic!("row {i} is not an agent"),
        };
        assert_eq!(provenance(0), Provenance::Main, "the host's non-repo value");
        assert_eq!(provenance(1), Provenance::Main, "a test builder's default");
    }

    #[test]
    fn elapsed_label_is_coarse_and_never_invented() {
        assert_eq!(elapsed_label(1000, 0), None); // then=0: never interacted
        assert_eq!(elapsed_label(100, 100).as_deref(), Some("0m"));
        assert_eq!(elapsed_label(100 + 59, 100).as_deref(), Some("0m"));
        assert_eq!(elapsed_label(100 + 5 * 60, 100).as_deref(), Some("5m"));
        assert_eq!(elapsed_label(100 + 2 * 3600, 100).as_deref(), Some("2h"));
        assert_eq!(elapsed_label(100 + 3 * 86_400, 100).as_deref(), Some("3d"));
        assert_eq!(elapsed_label(100 + 2 * 604_800, 100).as_deref(), Some("2w"));
        assert_eq!(elapsed_label(50, 100).as_deref(), Some("0m")); // clock skew: clamp, don't panic
    }

    #[test]
    fn agent_content_carries_the_card_fields() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", false)]);
        m.tick(1_100);
        let mut on_branch = dressed("u-branch", "/r/one", "feature/x", None, Some(10));
        on_branch.model = Some("sonnet".into());
        on_branch.provider = Some("claude".into());
        on_branch.pr_number = Some(232);
        on_branch.last_interacted = 1_000;
        let mut on_default = dressed("u-default", "/r/one", "main", None, Some(11));
        on_default.default_branch = Some("main".into());
        on_default.last_interacted = 0;
        m.apply_snapshot(snap(1, vec![on_branch, on_default]));
        let fields = |i: usize| match content_at(&m, i) {
            RowContent::Agent {
                model,
                provider,
                pr,
                branch,
                elapsed,
                ..
            } => (model, provider, pr, branch, elapsed),
            RowContent::Terminal { .. } => panic!("row {i} is not an agent"),
        };
        let (model, provider, pr, branch, elapsed) = fields(0);
        assert_eq!(model.as_deref(), Some("sonnet"));
        assert_eq!(provider.as_deref(), Some("claude"));
        assert_eq!(pr, Some(232));
        assert_eq!(branch, "feature/x");
        assert_eq!(elapsed.as_deref(), Some("1m")); // 100s since last_interacted
        // The default checkout blanks its branch cell — the same predicate
        // `provenance` renders blank via its `Main` case — and never having
        // interacted (`last_interacted: 0`) renders no elapsed at all.
        let (_, _, _, branch, elapsed) = fields(1);
        assert_eq!(branch, "");
        assert_eq!(elapsed, None);
    }

    #[test]
    fn a_tab_with_no_agent_is_a_terminal_row_carrying_the_zellij_name() {
        // Lock §7.1: the zellij tab name is used ONLY for a terminal tab.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #16", false)]);
        assert_eq!(content_at(&m, 0), RowContent::terminal("Tab #16"));
    }

    #[test]
    fn the_local_unread_override_still_turns_a_read_done_into_idle() {
        // §6.5, carried over from the (char, u8) glyph logic: a Done agent
        // already seen renders Idle until `clave focus` persists it. The
        // status mapping is otherwise one-to-one, so this is the single rule
        // that a mechanical Status → RowStatus port would have dropped.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Done, Some(10))]));
        assert_eq!(status_at(&m, 0), Some(RowStatus::Done));
        m.apply_tabs(vec![tab(10, 0, "t", true)]); // focus marks it read
        assert_eq!(status_at(&m, 0), Some(RowStatus::Idle));
    }

    #[test]
    fn model_states_outrank_the_stores_status() {
        // Stale and Opening are not `Status` variants at all (LEDGER D10) and
        // must win over whatever the store last said.
        let mut m = BarModel::default();
        let mut a = agent("u1", Status::Working, None);
        a.stale = true;
        m.apply_snapshot(snap(1, vec![a]));
        assert_eq!(status_at(&m, 0), Some(RowStatus::Stale));

        let mut m = BarModel::default();
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Working, None)]));
        m.opening.insert("u1".into());
        assert_eq!(status_at(&m, 0), Some(RowStatus::Opening));
        // …and with neither flag, a dormant row is Dormant regardless of the
        // status the store carries.
        m.opening.clear();
        assert_eq!(status_at(&m, 0), Some(RowStatus::Dormant));

        // The half that is NEW. Every case above is a dormant row, where the
        // old (char, u8) logic already answered the same way — it only reached
        // stale/opening off the dormant path. The unified projection applies
        // the precedence to a LIVE row too, so bind the agent to a tab and
        // assert the model state still outranks the store's `Status`.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        let mut a = agent("u1", Status::Working, Some(10));
        a.stale = true;
        m.apply_snapshot(snap(1, vec![a]));
        assert_eq!(status_at(&m, 0), Some(RowStatus::Stale), "live and stale");

        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Working, Some(10))]));
        // Sanity: without a flag the live row does show the store's status,
        // so the two assertions below are a genuine override, not a constant.
        assert_eq!(status_at(&m, 0), Some(RowStatus::Working));
        m.opening.insert("u1".into());
        assert_eq!(
            status_at(&m, 0),
            Some(RowStatus::Opening),
            "live and opening"
        );
    }

    #[test]
    fn the_width_profile_follows_state_not_current_width() {
        // LEDGER D16: the profile is chosen by STATE, which is what stops the
        // pane from over-running mid-animation. Same rule the width machine picks its
        // target with — a peeking bar is collapsed but showing the template.
        let mut m = BarModel::default();
        assert_eq!(m.widths(), Widths::EXPANDED);
        m.toggle();
        assert_eq!(m.widths(), Widths::COLLAPSED);
        assert!(m.visited(7), "a collapsed bar peeks on nav");
        assert_eq!(m.widths(), Widths::EXPANDED, "a peek shows the template");
        assert!(m.peek_expired());
        assert_eq!(m.widths(), Widths::COLLAPSED);
    }

    /// The one exception window: while awaiting hydration the mode is a
    /// default guess but the painted width is store truth (the launch layout
    /// is composed from `store.collapsed`), so the PAINT picks the profile —
    /// a collapsed cold start must not flash the expanded ink into its
    /// 30-col pane. Hydration restores D16's state-only rule.
    #[test]
    fn a_bar_awaiting_hydration_draws_the_profile_its_pane_was_born_at() {
        let mut m = BarModel::default();
        m.await_hydration();
        assert_eq!(m.widths_at(COL_W), Widths::COLLAPSED, "born collapsed");
        assert_eq!(m.widths_at(EXP_W), Widths::EXPANDED, "born expanded");
        let mut s = snap(1, vec![]);
        s.collapsed = true;
        m.apply_snapshot(s);
        // Hydrated: state rules again, whatever width a stale paint claims.
        assert_eq!(m.widths_at(EXP_W), Widths::COLLAPSED);
    }

    /// The hydration-window collapse check (above) must ask the MODE, not a
    /// hardcoded constant: a double-mode bar's collapsed target is 38, not
    /// `Single`'s 30 — painting at 30 must NOT be read as "born collapsed".
    #[test]
    fn hydration_collapse_detection_follows_the_mode() {
        let mut m = model_with_row_height(RowHeight::Double);
        m.awaiting_hydration = true;
        assert_eq!(m.widths_at(38), Widths::COLLAPSED);
        assert_eq!(m.widths_at(30), m.widths());
    }

    // --- PROVISIONAL ink allocation (delete with `ProvisionalInks`) --------

    #[test]
    fn provisional_inks_are_stable_across_two_identical_snapshots() {
        // The property a `HashMap` would break: Rust's default hasher is
        // randomly seeded, so iteration order varies per process AND the
        // colours would reshuffle between renders. Sorted keys make the same
        // snapshot always yield the same palette assignment.
        let agents = || {
            vec![
                dressed("u1", "/r/zebra", "main", Some("ZZ"), None),
                dressed("u2", "/r/alpha", "main", Some("AA"), None),
                dressed("u3", "/r/alpha", "main", Some("BB"), None),
            ]
        };
        let inks_of = |seq: u64| {
            let mut m = BarModel::default();
            m.apply_snapshot(snap(seq, agents()));
            m.rows()
                .into_iter()
                .map(|(k, r)| match r.content {
                    RowContent::Agent {
                        repo_ink,
                        title_ink,
                        ..
                    } => (k, repo_ink, title_ink),
                    RowContent::Terminal { .. } => panic!("no terminals here"),
                })
                .collect::<Vec<_>>()
        };
        // Two INDEPENDENT models over the same snapshot, which is where a
        // per-process hash seed would show up.
        let a = inks_of(1);
        let b = inks_of(1);
        assert_eq!(a, b);
        // Sorted order: /r/alpha is index 0, /r/zebra index 1. Within alpha,
        // AA is 0 and BB is 1 — a title chip is unique within its repo (lock
        // §4), so the two alpha rows do not collide.
        let repo_ink = |uuid: &str| {
            a.iter()
                .find(|(k, _, _)| *k == RowKey::Dormant(uuid.into()))
                .map(|(_, r, _)| *r)
                .unwrap()
        };
        let title_ink = |uuid: &str| {
            a.iter()
                .find(|(k, _, _)| *k == RowKey::Dormant(uuid.into()))
                .map(|(_, _, t)| *t)
                .unwrap()
        };
        assert_eq!(repo_ink("u2"), Some(0));
        assert_eq!(repo_ink("u3"), Some(0), "one repo is one colour");
        assert_eq!(repo_ink("u1"), Some(1));
        assert_eq!(title_ink("u2"), Some(0));
        assert_eq!(title_ink("u3"), Some(1), "chips differ within a repo");
    }

    #[test]
    fn provisional_inks_wrap_at_the_palette_length() {
        let agents: Vec<Agent> = (0..11)
            .map(|i| {
                dressed(
                    &format!("u{i:02}"),
                    &format!("/r/{i:02}"),
                    "main",
                    None,
                    None,
                )
            })
            .collect();
        let inks = ProvisionalInks::allocate(&agents);
        assert_eq!(inks.repo.len(), 11);
        assert_eq!(inks.repo["/r/00"], 0);
        assert_eq!(inks.repo["/r/07"], 7);
        assert_eq!(inks.repo["/r/08"], 0, "round-robin wraps at 8");
        assert_eq!(inks.repo["/r/10"], 2);
    }

    #[test]
    fn an_agent_outside_a_repo_gets_no_ink_never_index_zero() {
        // LEDGER D7: `0` is crystalBlue, a real hue, so `unwrap_or(0)` paints
        // every untinted row one colour while reading as "untinted".
        let inks = ProvisionalInks::allocate(&[agent("u1", Status::Idle, None)]);
        assert!(inks.repo.is_empty());
        assert!(inks.title.is_empty());
    }

    #[test]
    fn basename_takes_the_last_non_empty_component() {
        assert_eq!(basename("/Users/o/code/clave"), "clave");
        assert_eq!(basename("/Users/o/code/clave/"), "clave");
        assert_eq!(basename("clave"), "clave");
        assert_eq!(basename(""), "");
        assert_eq!(basename("/"), "");
    }

    #[test]
    fn store_rows_without_live_tabs_render_dormant() {
        // §6.6 C8: row set = TabUpdate ∪ dormant store rows. An agent whose
        // bind points at no current tab and whose registered pane is gone gets
        // a row of its own, marked Dormant, recency = last_interacted. It is
        // NOT labeled from the store any more — the projection renders title,
        // repo and summary (lock §2), so `label` is dead to this row.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "shell", true)]); // one plain live tab
        let mut a = agent("u-dormant", Status::Idle, None);
        a.last_interacted = 500;
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_order: Default::default(),
        });
        let rows = m.rows();
        assert_eq!(rows.len(), 2);
        let d = rows
            .iter()
            .find(|(k, _)| *k == RowKey::Dormant("u-dormant".into()))
            .expect("dormant row rendered");
        assert!(!d.1.selected);
        // The label is NOT the row any more: an agent row renders its title
        // chip, repo and summary from the store (lock §2), so the dormant
        // marker is what identifies the state here.
        assert!(matches!(
            d.1.content,
            RowContent::Agent {
                status: RowStatus::Dormant,
                ..
            }
        ));
    }

    #[test]
    fn rows_render_as_two_blocks_live_above_dormant() {
        // #112, replacing `dormant_rows_sort_into_the_unified_recency_order`:
        // the ONE merged list is gone. Live rows form a contiguous block at
        // the top, dormant rows their own block below, and each block sorts by
        // the commitment ordinal descending.
        //
        // The ordinals below are chosen so a merged list and a segregated one
        // give DIFFERENT answers: `u-new` at 900 outranks the live tab at 500
        // and would have led a merged list. It renders below it now. That is
        // the whole change, and this is the assertion that fails if the two
        // blocks are ever merged back.
        //
        // `last_interacted` is set to the OPPOSITE ranking on purpose: if the
        // comparator ever fell back to the wall clock, the within-block order
        // would invert and this test would fail loudly rather than keep
        // passing for the wrong reason.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut old = agent("u-old", Status::Idle, None);
        old.commit_ord = 100;
        old.last_interacted = 900;
        let mut new = agent("u-new", Status::Idle, None);
        new.commit_ord = 900;
        new.last_interacted = 100;
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents: vec![old, new],
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        assert_eq!(
            keys(&m),
            vec![
                RowKey::Tab(1),                  // the live block: 500
                RowKey::Dormant("u-new".into()), // then dormant, 900 …
                RowKey::Dormant("u-old".into()), // … desc … 100
            ],
            "live block first, each block ordinal-descending within itself"
        );
    }

    /// The order of the rows that SURVIVE a close, with the closed row removed.
    /// Relative order is the invariant S1 defends (R2, as amended by #116) —
    /// never a literal index, because #112's live/dormant segregation moves
    /// indices by design and must not break these tests.
    fn surviving_order(m: &BarModel, closed: &RowKey) -> Vec<RowKey> {
        keys(m).into_iter().filter(|k| k != closed).collect()
    }

    #[test]
    fn a_rows_rank_does_not_change_when_it_goes_dormant() {
        // Codex review, PR #135. `clave add` creates the tab (step 6) BEFORE it
        // writes the row (step 7), so the new tab's birth touch can mint its
        // ordinal FIRST and the row's own ordinal comes out higher. If a live
        // row keyed only on its tab while a dormant row keyed on its own
        // ordinal, closing the tab would swap which of the two numbers ranks
        // the row — and any commitment that landed in between would be
        // overtaken on close. That is exactly the "unrelated tab jumped"
        // symptom R2 forbids, reintroduced through a different door.
        //
        // The fix is one key for both classes: a row ranks by the HIGHER of its
        // own ordinal and its tab's, live or dormant. Going dormant then cannot
        // change the number.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "ours", true), tab(11, 1, "other", false)]);
        let mut ours = agent("u-ours", Status::Idle, Some(10));
        ours.commit_ord = 3; // add's write, minted LAST
        let mut other = agent("u-other", Status::Idle, Some(11));
        other.commit_ord = 2; // a prompt that landed in between
        m.apply_snapshot(snap_full(
            1,
            vec![ours, other],
            &[(10, 1), (11, 2)], // 1 = our birth touch, minted FIRST
        ));
        let before = keys(&m);
        assert_eq!(
            before,
            vec![RowKey::Tab(10), RowKey::Tab(11)],
            "our row ranks by its own ordinal 3, above the intervening prompt at 2"
        );
        // The RANK is what this test is about, so assert the number itself
        // rather than a position that stands in for it. Under #112 the
        // position moves by design and the number must not.
        let rank_when_live = m.live_ord(&tab(10, 0, "ours", true));
        assert_eq!(rank_when_live, 3, "max(own 3, tab's 1)");

        // Close ours. The prune carries max(3, 1) = 3 — unchanged.
        m.apply_tabs(vec![tab(11, 0, "other", true)]);
        let mut dormant = agent("u-ours", Status::Idle, None);
        dormant.commit_ord = 3;
        let mut other = agent("u-other", Status::Idle, Some(11));
        other.commit_ord = 2;
        m.apply_snapshot(snap_full(2, vec![dormant, other], &[(11, 2)]));
        let ours = m.agents.iter().find(|a| a.uuid == "u-ours").unwrap();
        assert_eq!(
            m.dormant_ord(ours),
            rank_when_live,
            "going dormant must not change which number ranks the row"
        );
        // And the row is now in the dormant block, below the live one — the
        // #112 segregation, which is a change of BLOCK and not of rank.
        assert_eq!(
            keys(&m),
            vec![RowKey::Tab(11), RowKey::Dormant("u-ours".into())],
        );
    }

    #[test]
    fn close_does_not_reorder_neighbours() {
        // The S1 §1.2 regression test, end to end through the model.
        //
        // The reported symptom: close one tab and an UNRELATED tab jumps to the
        // top. Cause: the closed row lost its ordering key and fell back to a
        // different one in a different tiebreak class, so everything re-sorted
        // around it. With the carry, the survivors hold their relative order and
        // the closed row keeps its rank among them.
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "a", true),
            tab(11, 1, "b", false),
            tab(12, 2, "c", false),
        ]);
        let mut a = agent("u-A", Status::Idle, Some(10));
        a.commit_ord = 30;
        m.apply_snapshot(snap_full(1, vec![a], &[(10, 30), (11, 20), (12, 10)]));
        let before = surviving_order(&m, &RowKey::Tab(10));
        assert_eq!(before, vec![RowKey::Tab(11), RowKey::Tab(12)]);

        // Tab 10 closes. The store prunes: its agent inherits ordinal 30 and
        // the tab entry goes.
        m.apply_tabs(vec![tab(11, 0, "b", true), tab(12, 1, "c", false)]);
        let mut dormant = agent("u-A", Status::Idle, None);
        dormant.commit_ord = 30;
        m.apply_snapshot(snap_full(2, vec![dormant], &[(11, 20), (12, 10)]));

        let closed = RowKey::Dormant("u-A".into());
        assert_eq!(
            surviving_order(&m, &closed),
            before,
            "survivors must keep their relative order"
        );
        // The closed row still ranks 30, exactly as its tab did — but #112
        // renders it in the dormant block, so it leads THAT block rather than
        // the whole list. R2 as amended: a close reorders nothing relative to
        // anything else; it moves the closed row out of the live block, which
        // is user-caused and visible.
        let a = m.agents.iter().find(|a| a.uuid == "u-A").unwrap();
        assert_eq!(m.dormant_ord(a), 30, "the rank survives the close");
        assert_eq!(
            keys(&m),
            vec![RowKey::Tab(11), RowKey::Tab(12), closed],
            "the survivors keep the live block; the closed row heads the \
             dormant one below it"
        );
    }

    #[test]
    fn close_holds_position_before_the_prune_lands() {
        // The render-side half of the carry. `clave prune-tabs` is
        // fire-and-forget, so between the tab vanishing from TabUpdate and the
        // prune's snapshot echo arriving, the bar still holds BOTH halves from
        // the same seq-gated snapshot. The row must therefore hold its rank on
        // the FIRST repaint — the fix cannot wait on a subprocess.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        let mut a = agent("u-A", Status::Idle, Some(10));
        a.commit_ord = 0; // never prompted — everything rides on the tab
        m.apply_snapshot(snap_full(1, vec![a], &[(10, 30), (11, 20)]));
        assert_eq!(keys(&m)[0], RowKey::Tab(10));

        // Tab gone from the tab list; NO new snapshot — the store still says
        // the agent is bound to 10 and that 10 has ordinal 30.
        m.apply_tabs(vec![tab(11, 0, "b", true)]);
        let a = m.agents.iter().find(|a| a.uuid == "u-A").unwrap();
        assert_eq!(
            m.dormant_ord(a),
            30,
            "the row must hold its rank before the prune echo lands — the \
             carry reads the tab's ordinal, not the row's own 0"
        );
        // #112: holding the RANK no longer means holding the POSITION. The
        // row moves to the dormant block on the same repaint, and its rank is
        // what orders it there.
        assert_eq!(
            keys(&m),
            vec![RowKey::Tab(11), RowKey::Dormant("u-A".into())],
        );
    }

    #[test]
    fn touch_only_tab_holds_its_place_on_close() {
        // The §1.2 aggravator, and the case that used to look worst on screen.
        // A tab born but never prompted has no ordinal of its own, so on close
        // it used to fall to the very bottom — beneath every dormant row ever
        // prompted. The carry gives it its tab's ordinal instead.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "fresh", true)]);
        let mut fresh = agent("u-fresh", Status::Idle, Some(10));
        fresh.commit_ord = 0;
        let mut older = agent("u-older", Status::Idle, None);
        older.commit_ord = 5; // prompted long ago, still ranks below the tab
        m.apply_snapshot(snap_full(1, vec![fresh, older], &[(10, 30)]));
        assert_eq!(keys(&m)[0], RowKey::Tab(10));

        // Close it, and the prune carries 30 onto the row.
        m.apply_tabs(vec![]);
        let mut carried = agent("u-fresh", Status::Idle, None);
        carried.commit_ord = 30;
        let mut older = agent("u-older", Status::Idle, None);
        older.commit_ord = 5;
        m.apply_snapshot(snap_full(2, vec![carried, older], &[]));
        assert_eq!(
            keys(&m),
            vec![
                RowKey::Dormant("u-fresh".into()),
                RowKey::Dormant("u-older".into())
            ],
            "a never-prompted tab must not plunge on close"
        );
    }

    #[test]
    fn dormant_row_never_reads_a_recycled_tabs_ordinal() {
        // Zellij REUSES tab ids (get_new_tab_id = max-key+1). The render-side
        // carry reads `tab_order[agent.tab_id]`, so a stale bind pointing at a
        // recycled id could in principle read the NEW tenant's rank. It cannot:
        // `is_dormant` returns false while any live tab holds that id, so such a
        // row is not dormant at all and never reaches the dormant key.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "reborn", true)]);
        // u-old still claims tab 10, which now belongs to a fresh tenant.
        let mut old = agent("u-old", Status::Idle, Some(10));
        old.commit_ord = 1;
        m.apply_snapshot(snap_full(1, vec![old], &[(10, 99)]));
        // It renders as the live tab's row, not as a dormant row borrowing 99.
        assert_eq!(keys(&m), vec![RowKey::Tab(10)]);
        assert!(
            !keys(&m).contains(&RowKey::Dormant("u-old".into())),
            "a row whose tab id is live is never dormant"
        );
    }

    #[test]
    fn agent_with_live_tab_or_registered_pane_is_not_dormant() {
        // The same uuid must never render twice: a bound live tab, OR a
        // registered pane still present in the manifest (pre-bind beat, e.g.
        // right after Alt+a's tab appears), suppresses the dormant row.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(7, 0, "agent-tab", true)]);
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents: vec![agent("u1", Status::Working, Some(7))], // bound → live
            tab_order: Default::default(),
        });
        assert!(!keys(&m).contains(&RowKey::Dormant("u1".into())));
        // Bind gone (fresh session) but the pane join exists → still not dormant.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(7, 0, "agent-tab", true)]);
        m.register("u2".into(), 42);
        m.apply_panes(vec![pane(0, 42, false, true)]);
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents: vec![agent_at("u2", Status::Working, None, 42)],
            tab_order: Default::default(),
        });
        assert!(!keys(&m).contains(&RowKey::Dormant("u2".into())));
    }

    /// Two live tabs and three dormant rows, the dormant ones deliberately
    /// out-ranking both tabs — the shape #112 exists for, in miniature. On the
    /// real store it is 4 live and 17 dormant.
    fn fleet_two_live_three_dormant() -> BarModel {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "a", true), tab(2, 1, "b", false)]);
        let mut agents = Vec::new();
        for (j, ord) in [700u64, 800, 900].into_iter().enumerate() {
            let mut a = agent(&format!("u-d{j}"), Status::Idle, None);
            a.commit_ord = ord;
            agents.push(a);
        }
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents,
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64), (2usize, 400u64)]),
        });
        m
    }

    #[test]
    fn the_nav_ring_wraps_inside_the_live_block() {
        // #112, the headline: Alt+j/Alt+k cycle the LIVE block and wrap at its
        // end. Every dormant row here outranks both tabs, so a merged list
        // would have put three of them above the live pair and made them the
        // first thing a walk hit.
        let mut m = fleet_two_live_three_dormant();
        assert_eq!(
            keys(&m),
            vec![
                RowKey::Tab(1),
                RowKey::Tab(2),
                RowKey::Dormant("u-d2".into()), // 900
                RowKey::Dormant("u-d1".into()), // 800
                RowKey::Dormant("u-d0".into()), // 700
            ]
        );
        m.beacon(1);
        // Forward off the end of the live block wraps to its head, NOT into
        // the dormant block below.
        assert!(
            m.nav("{\"dir\":\"next\"}", Some(1))
                .contains(&Effect::SwitchTab { position: 1 }),
            "tab 1 → tab 2"
        );
        m.beacon(2);
        assert!(
            m.nav("{\"dir\":\"next\"}", Some(2))
                .contains(&Effect::SwitchTab { position: 0 }),
            "tab 2 wraps to tab 1, not down into the dormant block"
        );
        // And backward off the head wraps to the live block's TAIL — the
        // nearest dormant row is directly below it on screen, which is the
        // step this must not take.
        m.beacon(1);
        assert!(
            m.nav("{\"dir\":\"prev\"}", Some(1))
                .contains(&Effect::SwitchTab { position: 1 }),
            "tab 1 wraps back to tab 2"
        );
        assert!(m.cursor.is_none(), "no walk may leave a dormant selection");
    }

    #[test]
    fn clicking_into_the_dormant_block_hands_it_the_walk() {
        // The interaction Ollie specified, 2026-08-07: clicking a dormant row
        // FOCUSES that list, and Alt+j/Alt+k then walk it; clicking back to
        // the live list focuses the live list again. The walk itself never
        // moves between the blocks — only a pick does.
        let mut m = one_line_bar(fleet_two_live_three_dormant());
        m.beacon(1);
        // Click the middle dormant row (line 3 = u-d1 at 800).
        m.click(3, TALL_PANE);
        assert_eq!(m.cursor.as_deref(), Some("u-d1"));
        // Alt+j now walks the DORMANT block, and switches no tab.
        let fx = m.nav("{\"dir\":\"next\"}", Some(1));
        assert!(fx.is_empty(), "walking the dormant block switches nothing");
        assert_eq!(m.cursor.as_deref(), Some("u-d0"), "down to the last row");
        // …and wraps at that block's end rather than escaping into the live
        // block above it.
        m.nav("{\"dir\":\"next\"}", Some(1));
        assert_eq!(
            m.cursor.as_deref(),
            Some("u-d2"),
            "wraps to the head of the dormant block, not into the live one"
        );
        m.nav("{\"dir\":\"prev\"}", Some(1));
        assert_eq!(m.cursor.as_deref(), Some("u-d0"), "and back the other way");
        // Clicking a live row hands the walk back to the live block.
        m.click(0, TALL_PANE);
        assert!(m.cursor.is_none(), "the live pick released the selection");
        assert!(
            m.nav("{\"dir\":\"next\"}", Some(1))
                .contains(&Effect::SwitchTab { position: 1 }),
            "the live block has the walk again"
        );
    }

    #[test]
    fn a_selection_gone_live_returns_the_walk_to_the_live_block() {
        // The self-heal: the walk's block is decided by where the cursor is
        // DISPLAYED, so a selected row that gets opened (or disappears) hands
        // the walk back on its own. Without this the ring would be stranded on
        // a row that is no longer in the dormant block.
        let mut m = live_plus_dormant();
        m.beacon(1);
        select_dormant(&mut m);
        assert_eq!(m.cursor.as_deref(), Some("u-d"));
        // u-d comes up as a tab of its own; the cursor still names it.
        m.apply_tabs(vec![tab(1, 0, "live", false), tab(2, 1, "u-d", true)]);
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 2,
            agents: vec![agent("u-d", Status::Working, Some(2))],
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64), (2usize, 600u64)]),
        });
        m.beacon(2);
        assert!(
            m.nav("{\"dir\":\"next\"}", Some(2))
                .contains(&Effect::SwitchTab { position: 0 }),
            "the walk is back on the live block"
        );
    }

    #[test]
    fn a_single_live_row_wraps_onto_itself() {
        // The one-live case. The ring has length 1, so both directions land
        // back on the same tab — dormant rows below stay unreachable by walk.
        let mut m = live_plus_dormant();
        m.beacon(1);
        for dir in ["next", "prev"] {
            let fx = m.nav(&format!("{{\"dir\":\"{dir}\"}}"), Some(1));
            assert!(
                fx.contains(&Effect::SwitchTab { position: 0 }),
                "{dir} must land back on the only live tab"
            );
            assert!(m.cursor.is_none(), "{dir} reached the dormant block");
        }
    }

    #[test]
    fn with_no_live_rows_the_walk_falls_back_to_the_dormant_block() {
        // The zero-live edge case, decided rather than left to fall out: the
        // ring becomes the whole list so the keyboard is never dead. It is
        // unreachable in a real session — every zellij tab carries a bar, so
        // the tab list is never empty — but the model is pure and must answer.
        // Safe by construction: a dormant landing only ever selects (#100), so
        // this cannot spawn anything.
        let mut m = BarModel::default();
        m.apply_tabs(vec![]);
        let mut agents = Vec::new();
        for (j, ord) in [10u64, 20].into_iter().enumerate() {
            let mut a = agent(&format!("u-d{j}"), Status::Idle, None);
            a.commit_ord = ord;
            agents.push(a);
        }
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents,
            tab_order: Default::default(),
        });
        assert_eq!(
            keys(&m),
            vec![
                RowKey::Dormant("u-d1".into()), // 20
                RowKey::Dormant("u-d0".into()), // 10
            ]
        );
        // No cursor yet → the walk starts at row 0 and steps to row 1.
        assert!(m.nav("{\"dir\":\"next\"}", Some(1)).is_empty());
        assert_eq!(m.cursor.as_deref(), Some("u-d0"));
        // And it wraps within the dormant block rather than running off it.
        assert!(m.nav("{\"dir\":\"next\"}", Some(1)).is_empty());
        assert_eq!(m.cursor.as_deref(), Some("u-d1"));
    }

    #[test]
    fn alt_n_still_reaches_the_dormant_block() {
        // The other half of the ring decision: taking dormant rows out of the
        // walk is only tolerable because the numbers still reach them. Alt+N
        // indexes the RENDERED list, so putting the live block on top is what
        // keeps the low numbers pointed at the live fleet — Alt+1-2 here, with
        // the dormant block starting at Alt+3.
        let mut m = one_line_bar(fleet_two_live_three_dormant());
        m.beacon(1);
        assert!(
            m.nav("{\"row\":2}", Some(1))
                .contains(&Effect::SwitchTab { position: 1 }),
            "Alt+2 is the second LIVE row"
        );
        m.beacon(2);
        assert!(
            m.nav("{\"row\":3}", Some(2)).is_empty(),
            "Alt+3 is the head of the dormant block, and selecting is silent"
        );
        assert_eq!(m.cursor.as_deref(), Some("u-d2"));
        assert!(m.nav("{\"row\":5}", Some(2)).is_empty());
        assert_eq!(m.cursor.as_deref(), Some("u-d0"), "Alt+5 reaches the last");
        // A click reaches the same row — the mouse is the route past Alt+9.
        m.click(3, TALL_PANE);
        assert_eq!(m.cursor.as_deref(), Some("u-d1"));
    }

    #[test]
    fn nav_onto_dormant_row_selects_without_opening() {
        // #100 dwell-commit: landing on a dormant row moves the virtual cursor
        // and STOPS — no timer, no open, no tab switch. #112 changed only HOW
        // you land there: the dir walk is confined to the live block now, so
        // Alt+N is the keyboard route (the mouse is the other).
        let mut m = one_line_bar(live_plus_dormant());
        m.beacon(1);
        let fx = m.nav("{\"row\":2}", Some(1)); // Alt+2 → the dormant row
        assert!(fx.is_empty(), "a dormant landing is pure selection: {fx:?}");
        assert!(
            selected(&m)[DORMANT_LINE],
            "the dormant row holds the selection"
        );
        // The walk now belongs to the dormant block, so a dir step stays in
        // it — with one dormant row that means landing back on itself, and
        // still no tab switch.
        let fx = m.nav("{\"dir\":\"next\"}", Some(1));
        assert!(fx.is_empty(), "still pure selection: {fx:?}");
        assert!(
            selected(&m)[DORMANT_LINE],
            "the walk stays in the dormant block until a live row is picked"
        );
        // Picking a live row is what hands the walk back.
        m.click(0, TALL_PANE);
        assert!(m.cursor.is_none());
        assert!(
            m.nav("{\"dir\":\"next\"}", Some(1))
                .contains(&Effect::SwitchTab { position: 0 }),
            "the live block has the walk again"
        );
    }

    #[test]
    fn commit_opens_the_selected_dormant_row_exactly_once() {
        // #100: Alt+Enter spends the selection — one open, marked ↻; a
        // repeat commit while in flight must not double-fire.
        let mut m = live_plus_dormant();
        m.beacon(1);
        select_dormant(&mut m);
        assert_eq!(
            m.nav("{\"commit\":true}", Some(1)),
            vec![Effect::OpenAgent { uuid: "u-d".into() }]
        );
        assert!(
            m.nav("{\"commit\":true}", Some(1)).is_empty(),
            "in-flight open must not double-fire"
        );
    }

    /// The in-flight mark is double-fire guard #1 (`clave open`'s own liveness
    /// no-op is #2), and it is only a guard for as long as it survives the
    /// store traffic that arrives while the open is still running. It clears on
    /// exactly two things — the row stopped being dormant (the tab appeared) or
    /// the snapshot flagged it stale (the open failed, so it must be
    /// retryable) — and on nothing else.
    ///
    /// If an unrelated push cleared it, a user who pressed Alt+Enter watches
    /// the ↻ vanish, presses again, and gets a SECOND `clave open` for a
    /// session already launching: the double-attach this guard exists to stop.
    ///
    /// (cargo mutants 2026-08-15 found two ways to that: dropping the `!` so
    /// a still-dormant row clears the mark, and flipping the row lookup so the
    /// verdict is read off whichever OTHER agent happens to come first — which
    /// is why a second, live agent is in the snapshot below.)
    #[test]
    fn an_in_flight_open_holds_its_mark_through_unrelated_snapshots() {
        let mut m = live_plus_dormant();
        m.beacon(1);
        select_dormant(&mut m);
        assert_eq!(
            m.nav("{\"commit\":true}", Some(1)),
            vec![Effect::OpenAgent { uuid: "u-d".into() }]
        );
        // A push lands while the open is still in flight: an unrelated agent
        // binds into the live tab. `u-d` is still dormant and still not stale.
        let mut d = agent("u-d", Status::Idle, None);
        d.commit_ord = 999;
        m.apply_snapshot(snap(2, vec![d, agent("u-other", Status::Idle, Some(1))]));
        assert_eq!(
            status_at(&m, DORMANT_LINE),
            Some(RowStatus::Opening),
            "the row is still launching, so it still says so"
        );
        assert!(
            m.nav("{\"commit\":true}", Some(1)).is_empty(),
            "and the guard still refuses the second press"
        );
        // The tab appears: the open resolved, so the mark retires and the row
        // leaves the dormant block entirely.
        m.apply_tabs(vec![tab(1, 0, "live", true), tab(2, 1, "woken", false)]);
        m.apply_snapshot(snap(
            3,
            vec![
                agent("u-d", Status::Idle, Some(2)),
                agent("u-other", Status::Idle, Some(1)),
            ],
        ));
        assert!(
            keys(&m).iter().all(|k| *k != RowKey::Dormant("u-d".into())),
            "the woken row is live now"
        );
        assert_ne!(status_at(&m, 1), Some(RowStatus::Opening));
    }

    #[test]
    fn commit_without_a_dormant_selection_is_a_noop() {
        // The bind is global; the model is the gate. No cursor → nothing to
        // wake. Non-executor instances (no fresh tab set) also no-op — their
        // cursor is never set, and the gate returns before any open.
        let mut m = live_plus_dormant();
        m.beacon(1);
        assert!(
            m.nav("{\"commit\":true}", Some(1)).is_empty(),
            "no selection"
        );
        select_dormant(&mut m);
        assert!(
            selected(&m)[DORMANT_LINE],
            "a selection must exist, or the next assertion cannot fail"
        );
        assert!(
            m.nav("{\"commit\":true}", None).is_empty(),
            "non-executor never commits"
        );
    }

    #[test]
    fn explicit_picks_select_without_opening() {
        // #100 reverses the original proposal: click and Alt+N on a dormant
        // row SELECT it. The mouse is the main path to dormant rows past
        // Alt+9, so a click that launched would just move the accidental
        // spawn from the keyboard channel into the mouse channel. Both are
        // now the ONLY routes to a dormant row (#112 took the walk away), so
        // this pair covers the whole reachable surface.
        let mut m = one_line_bar(live_plus_dormant());
        assert!(
            m.click(DORMANT_LINE, TALL_PANE).is_empty(),
            "click selects, never opens"
        );
        assert!(
            selected(&m)[DORMANT_LINE],
            "clicked dormant row holds the selection"
        );
        // Alt+N (row payload) on a dormant row — new model, fresh state:
        let mut m = one_line_bar(live_plus_dormant());
        m.beacon(1);
        assert!(select_dormant(&mut m).is_empty(), "Alt+N selects too");
        assert!(selected(&m)[DORMANT_LINE]);
        // A commit after either pick launches — selection and launch are two
        // separate acts on every input path.
        assert_eq!(
            m.nav("{\"commit\":true}", Some(1)),
            vec![Effect::OpenAgent { uuid: "u-d".into() }]
        );
    }

    /// The overflow incident's SECOND symptom (#148): with the list scrolled,
    /// clicks landed one or two rows above the row under the pointer, because
    /// the hit test counted from model row 0 while the screen showed a window
    /// starting further down. Draw and hit test now read the same offset, so
    /// this fixes the visible bug and the invisible one together.
    #[test]
    fn a_click_lands_on_the_row_under_the_pointer_while_scrolled() {
        // 12 rows (one live, eleven dormant) in a five-line pane. Selecting the
        // last row slides the window to model rows 7..=11.
        let mut m = one_line_bar(overflowing_fleet(11));
        assert_eq!(keys(&m).len(), 12);
        m.nav("{\"row\":12}", Some(1));
        assert!(selected(&m)[11], "the fixture must be scrolled to the end");

        // Line 0 of that pane is model row 7 — the pre-viewport click map read
        // it as row 0, the live tab, seven rows off.
        m.click(0, 5);
        let mut expected = vec![false; 12];
        expected[7] = true;
        assert_eq!(
            selected(&m),
            expected,
            "the top visible line is model row 7"
        );

        // And the bottom line of that same pane is the last row of the list.
        // A fresh model, because the click above MOVED the selection and the
        // view follows the selection — see the next test.
        let mut m = one_line_bar(overflowing_fleet(11));
        m.nav("{\"row\":12}", Some(1));
        m.click(4, 5);
        let mut expected = vec![false; 12];
        expected[11] = true;
        assert_eq!(selected(&m), expected, "the last visible line is row 11");
    }

    /// A click is a landing like any other, so the view follows it: picking the
    /// top visible row gives that row its two rows of lookahead back, which
    /// slides the window up under the pointer. Pinned because it is the visible
    /// consequence of a viewport derived from the selection alone (#148) — the
    /// clicked row stays on screen, which is the invariant that matters.
    #[test]
    fn the_view_follows_a_click_the_way_it_follows_a_walk() {
        let mut m = one_line_bar(overflowing_fleet(11));
        m.nav("{\"row\":12}", Some(1)); // window 7..=11
        m.click(0, 5); // model row 7
        // The window is now 5..=9, so the row just clicked sits on line 2 —
        // a second click on line 2 is the same row, not a new one.
        m.click(2, 5);
        let mut expected = vec![false; 12];
        expected[7] = true;
        assert_eq!(
            selected(&m),
            expected,
            "the clicked row moved under the pointer"
        );
    }

    /// A pane with room for the whole fleet keeps the identity mapping: line N
    /// is row N. The offset is a consequence of overflow, never a constant.
    #[test]
    fn a_click_on_an_unscrolled_bar_still_lands_on_its_own_line() {
        let mut m = one_line_bar(overflowing_fleet(11));
        m.click(9, TALL_PANE);
        let mut expected = vec![false; 12];
        expected[9] = true;
        assert_eq!(selected(&m), expected);
    }

    /// #232's click map. A card owns TWO screen lines, so both of them are the
    /// same target — the pointer landing on a card's second line must not
    /// select the card below it, which is the #148 failure shape one geometry
    /// short. Both terms convert here (`line` AND `pane_height`), so a fleet
    /// too tall for the pane scrolls in cards and the hit test counts in the
    /// same cards.
    #[test]
    fn a_click_on_either_line_of_a_card_selects_that_card() {
        // The shipping default: `BarModel::default()` is already `Double`.
        // 12 rows (one live tab, eleven dormant) in an EIGHT-line pane — four
        // cards, model rows 0..=3, because the selection rests at the top.
        let live = vec![
            Effect::SwitchTab { position: 0 },
            Effect::AnnounceVisit { tab_id: 1 },
        ];
        for line in [0, 1] {
            let mut m = overflowing_fleet(11);
            assert_eq!(m.click(line, 8), live, "line {line} is the first card");
        }
        // The fourth card spans lines 6 and 7 — dormant row `u-02`, which a
        // click SELECTS rather than opens (#100).
        for line in [6, 7] {
            let mut m = overflowing_fleet(11);
            assert!(m.click(line, 8).is_empty(), "a dormant click opens nothing");
            let mut expected = vec![false; 12];
            expected[3] = true;
            assert_eq!(selected(&m), expected, "line {line} is the fourth card");
        }
        // Scrolled, in cards: selecting the last row slides the card window to
        // model rows 9..=11 in a six-line (three-card) pane, and line 0 of
        // that pane is model row 9 — the whole #148 lesson, one geometry over.
        let mut m = overflowing_fleet(11);
        m.nav("{\"row\":12}", Some(1));
        assert!(selected(&m)[11], "the fixture must be scrolled to the end");
        m.click(1, 6);
        let mut expected = vec![false; 12];
        expected[9] = true;
        assert_eq!(
            selected(&m),
            expected,
            "the top visible card is model row 9"
        );
        // The legacy arm is untouched: one line, one row, same fixture.
        let mut m = one_line_bar(overflowing_fleet(11));
        assert!(m.click(3, 8).is_empty());
        let mut expected = vec![false; 12];
        expected[3] = true;
        assert_eq!(selected(&m), expected, "Single still maps line 3 to row 3");
    }

    #[test]
    fn click_on_a_live_tab_releases_the_dormant_selection() {
        // Selection follows every input path: picking a live row (mouse or
        // nav) resolves the highlight back to focus truth.
        let mut m = one_line_bar(live_plus_dormant());
        m.click(DORMANT_LINE, TALL_PANE); // select the dormant row
        assert_eq!(
            selected(&m),
            vec![false, true],
            "the dormant selection steals the highlight from the live tab"
        );
        m.click(0, TALL_PANE); // pick the live tab
        assert_eq!(
            selected(&m),
            vec![true, false],
            "live pick releases the dormant selection"
        );
        // The abandoned selection must not be committable.
        m.beacon(1);
        assert!(m.nav("{\"commit\":true}", Some(1)).is_empty());
    }

    #[test]
    fn dormant_landing_peeks_a_collapsed_bar() {
        // §6.6: landing on a dormant row must keep a collapsed bar peeked,
        // same as live-row nav (whose peek rides the visited pipe — dormant
        // landings have no pipe, so the model returns ArmPeek explicitly).
        // The landing is an Alt+N pick now that #112 keeps the walk live-only.
        let mut m = BarModel::default();
        m.toggle(); // collapsed
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.commit_ord = 999;
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            // Issue #5: snapshots now carry the store's collapse mode; after
            // the toggle above, the real flow's store says collapsed too —
            // `false` here would (correctly!) heal the bar back to expanded.
            collapsed: true,
            seq: 1,
            agents: vec![a],
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        let fx = select_dormant(&mut m);
        // The peek is the landing's ONLY effect — the dwell died with #100.
        assert_eq!(fx, vec![Effect::ArmPeek]);
    }

    #[test]
    fn opening_and_stale_glyphs_decorate_dormant_rows() {
        // ↻ while an open is in flight (set by open_effects, Task 9 — poke the
        // set directly here); ✗ when the snapshot says stale. A stale=true
        // snapshot also clears the in-flight mark (the open FAILED — the row
        // must become retryable, not stuck ↻).
        let mut m = BarModel::default();
        let mut a = agent("u1", Status::Idle, None);
        a.stale = true;
        m.opening.insert("u1".into());
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_order: Default::default(),
        });
        assert_eq!(status_at(&m, 0), Some(RowStatus::Stale));
        assert!(m.opening.is_empty(), "stale snapshot clears in-flight");
        // In-flight (no stale): ↻.
        let mut m = BarModel::default();
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents: vec![agent("u2", Status::Idle, None)],
            tab_order: Default::default(),
        });
        m.opening.insert("u2".into());
        assert_eq!(status_at(&m, 0), Some(RowStatus::Opening));
    }

    #[test]
    fn virtual_cursor_highlight_follows_the_dormant_walk() {
        // §6.6 C8: landing on a dormant row moves the SELECTION there — that
        // row reads active and the previously-focused live tab drops its
        // highlight (else the stale live highlight lingers and misleads).
        let mut m = live_plus_dormant();
        m.beacon(1);
        // (a) pick the dormant row → it is active, the live tab is not.
        select_dormant(&mut m);
        assert_eq!(keys(&m)[DORMANT_LINE], RowKey::Dormant("u-d".into()));
        assert_eq!(
            selected(&m),
            vec![false, true],
            "dormant selection holds it"
        );
        // (b) picking a live row clears the cursor → the tab highlights again.
        // It must be a PICK: a dir walk would stay in the dormant block now.
        m.nav("{\"row\":1}", Some(1));
        assert_eq!(
            selected(&m),
            vec![true, false],
            "focused tab reclaims the highlight"
        );
    }

    #[test]
    fn stale_cursor_on_a_row_gone_live_self_heals_to_the_tab() {
        // Review minor #7: a dwell-opened row goes LIVE while the cursor still
        // names its uuid. The dormant-key lookup misses, so the highlight
        // falls back to the focused tab — no explicit cursor clear needed.
        let mut m = live_plus_dormant();
        m.beacon(1);
        select_dormant(&mut m); // cursor now on the dormant row
        assert!(selected(&m)[DORMANT_LINE]); // dormant selected
        // The row goes LIVE: u-d binds to a new tab (2). Cursor still names
        // "u-d" but it no longer renders dormant.
        m.apply_tabs(vec![tab(1, 0, "live", false), tab(2, 1, "u-d", true)]);
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 2,
            agents: vec![agent("u-d", Status::Working, Some(2))],
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64), (2usize, 600u64)]),
        });
        let rows = m.rows();
        assert!(!keys(&m).contains(&RowKey::Dormant("u-d".into())));
        let active: Vec<_> = rows
            .iter()
            .filter(|(_, r)| r.selected)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(
            active,
            vec![RowKey::Tab(2)],
            "highlight self-heals to the focused tab"
        );
    }

    #[test]
    fn native_switch_beacon_clears_a_pinned_dormant_cursor() {
        // Edge (Fix-1 heal): a committed open FAILS — the row stays dormant
        // with the cursor pinned to it. A NATIVE tab switch (Alt+o / zellij
        // binds) carries no clave-nav, only a visited-pipe beacon. That
        // beacon must resolve the selection back to the focused tab, else the
        // ✗ row keeps the highlight and the real active tab stays suppressed.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", false), tab(2, 1, "other", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        // Outranks both tabs and still renders last — #112 puts it in the
        // dormant block, below the two live rows.
        a.commit_ord = 999;
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64), (2usize, 400u64)]),
        });
        m.beacon(1);
        let fx = m.nav("{\"row\":3}", Some(1)); // Alt+3 → the dormant row
        assert!(fx.is_empty(), "the landing is pure selection");
        assert!(selected(&m)[2], "dormant selected before the native switch");
        // Native switch to tab 2 arrives as a visited-pipe beacon (no nav).
        m.beacon(2);
        let rows = m.rows();
        assert!(!rows[2].1.selected, "dormant row releases the highlight");
        let active: Vec<_> = rows
            .iter()
            .filter(|(_, r)| r.selected)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(active, vec![RowKey::Tab(2)], "focused tab reclaims it");
        // The abandoned selection must not be committable.
        assert!(m.nav("{\"commit\":true}", Some(2)).is_empty());
    }

    #[test]
    fn organic_switch_spends_the_selection_before_the_beacon_lands() {
        // Codex P2 (#128): Alt+o's native switch happens server-side at
        // once, but the departed instance keeps `current_tab == own` until
        // the new bar's visited beacon round-trips. An Alt+Enter broadcast
        // inside that gap reaches the departed bar with its stale executor
        // view — the organic pipe (same keybind, synchronous) must have
        // already spent the selection, or a hidden bar commits a row the
        // visible bar does not show as selected.
        let mut m = live_plus_dormant();
        m.beacon(1);
        select_dormant(&mut m);
        assert!(selected(&m)[DORMANT_LINE]);
        m.set_organic_pending(); // Alt+o pressed; beacon not yet returned
        assert!(
            m.nav("{\"commit\":true}", Some(1)).is_empty(),
            "the beacon-gap commit must find no selection"
        );
    }

    #[test]
    fn gutter_tier_selected_dormant_shows_the_commit_mark() {
        // #100 §4: stale ✗ > opening ↻ > selected-dormant ⏎ > status. The
        // chain is self-truthful — committing turns ⏎ into ↻, and a stale
        // row never offers a launch that would fail.
        let mut m = live_plus_dormant();
        assert_eq!(status_at(&m, DORMANT_LINE), Some(RowStatus::Dormant));
        m.beacon(1);
        select_dormant(&mut m);
        assert_eq!(
            status_at(&m, DORMANT_LINE),
            Some(RowStatus::DormantSelected)
        );
        m.nav("{\"commit\":true}", Some(1)); // launch it
        assert_eq!(
            status_at(&m, DORMANT_LINE),
            Some(RowStatus::Opening),
            "↻ outranks the still-selected cursor"
        );
        // Stale outranks everything: the same selected row gone stale shows ✗
        // (the open failed — apply_snapshot clears `opening`, cursor holds).
        let mut a = agent("u-d", Status::Idle, None);
        a.commit_ord = 999;
        a.stale = true;
        m.apply_snapshot(AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed: false,
            seq: 2,
            agents: vec![a],
            tab_order: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        assert_eq!(status_at(&m, DORMANT_LINE), Some(RowStatus::Stale));
        // And ✗ means it: the selected-but-stale row REFUSES the commit
        // (ratified live 2026-08-01) — the gutter never offers a launch it
        // won't perform.
        assert!(
            m.nav("{\"commit\":true}", Some(1)).is_empty(),
            "a stale row must refuse the commit"
        );
    }

    /// D37: a bar that has not yet heard from the store does not know which
    /// mode it is in, so it must not switch anything. The pane is BORN at the
    /// width its persisted mode wants, so a switch on the assumed-expanded
    /// default can only move it away from correct — and then visibly back when
    /// `clave snapshot` returns.
    #[test]
    fn a_bar_awaiting_hydration_never_switches_geometry() {
        let mut m = focused_bar();
        m.await_hydration();
        m.toggle(); // a keypress before hydration is still not a licence
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        // The snapshot arrives and the mode is authoritative from here on.
        let mut snapshot = snap(1, vec![]);
        snapshot.collapsed = true;
        m.apply_snapshot(snapshot);
        // The pane is painted expanded and the store now says collapsed:
        // one switch, and arriving at the collapsed width ends it.
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
        assert_eq!(m.width_effects(Some(COL_W)), Vec::<Effect>::new());
        assert_eq!(m.width_cooldown_elapsed(), Vec::<Effect>::new());
        // A keypress AFTER hydration is a licence, and moves the pane.
        m.toggle();
        assert_eq!(m.width_effects(Some(COL_W)), vec![Effect::SwapWidth]);
    }

    /// Finding K (#197 review): the cold-start flash. `clave launch` composes
    /// the layout with the store's collapse flag, so a fleet left collapsed is
    /// BORN at the collapsed width — and a bar that widened it on hydration and
    /// snapped it back is what the field saw on every collapsed cold start.
    ///
    /// That was fixed by seeding a belief, then by a naming switch. It is now
    /// structural twice over: the launch layout carries the collapsed width as
    /// a fixed column count, and the machine compares the painted width
    /// against that same constant — equal from the first paint, so a collapsed
    /// cold start owes zellij nothing at all.
    #[test]
    fn a_collapsed_cold_start_does_not_flash_wide() {
        let mut m = fleet_bar(11, 11);
        m.beacon(11);
        m.await_hydration();
        frame(&mut m, 11); // born at the collapsed width
        let mut snapshot = snap(1, vec![]);
        snapshot.collapsed = true;
        m.apply_snapshot(snapshot);
        assert_eq!(
            m.width_effects(Some(COL_W)),
            Vec::<Effect>::new(),
            "a pane born collapsed must not be widened by hydration"
        );
        assert_eq!(
            m.width_effects(Some(COL_W)),
            Vec::<Effect>::new(),
            "nor later"
        );
    }

    /// The other half: an EXPANDED cold start settles the same way, for the
    /// same reason rather than because expanded happens to be the default the
    /// field started at.
    #[test]
    fn an_expanded_cold_start_settles_the_same_way() {
        let mut m = fleet_bar(11, 11);
        m.beacon(11);
        m.await_hydration();
        frame(&mut m, 11);
        m.apply_snapshot(snap(1, vec![])); // collapsed: false
        assert_eq!(m.width_effects(Some(EXP_W)), Vec::<Effect>::new());
        m.toggle();
        assert_eq!(m.width_effects(Some(EXP_W)), vec![Effect::SwapWidth]);
    }

    // === #137: the collapse-mode repair storm ==============================
    // The 2026-08-01 incident: ~12 tabs, repeated Alt+c, and the bar cycled a
    // full-width reheal for about a minute. Reproduced under instrumentation on
    // 2026-08-07 — 1,413 renders and 634 resizes in three seconds across twelve
    // instances, from ten keypresses. The cause was never the width machine:
    // the collapse MODE kept moving, because a store round-trip
    // is slower than a keypress and every lagging snapshot contradicted newer
    // local state. Each contradiction bought a re-assert write (another
    // snapshot) and each flip bought a fresh 16-step budget.

    /// A snapshot carrying `collapsed`, at `seq`. Nothing else moves, so any
    /// behaviour these tests see is the collapse ledger's alone.
    fn collapse_snap(seq: u64, collapsed: bool) -> AgentSnapshot {
        AgentSnapshot {
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            collapsed,
            seq,
            agents: vec![],
            tab_order: Default::default(),
        }
    }

    #[test]
    fn a_toggle_burst_books_two_writes_per_press_not_one_per_snapshot() {
        let mut m = BarModel::default();
        m.apply_snapshot(collapse_snap(1, false));
        let mut writes = 0;
        let mut seq = 1u64;
        const PRESSES: usize = 10;
        for _ in 0..PRESSES {
            let before = m.collapsed;
            writes += m
                .toggle()
                .iter()
                .filter(|e| matches!(e, Effect::PersistCollapse { .. }))
                .count();
            // Contradicting snapshots: our write has not landed yet, and the
            // store keeps pushing. Three per press, so a per-snapshot budget
            // shows up as 4x rather than 2x.
            for _ in 0..3 {
                seq += 1;
                writes += m
                    .apply_snapshot(collapse_snap(seq, before))
                    .iter()
                    .filter(|e| matches!(e, Effect::PersistCollapse { .. }))
                    .count();
            }
        }
        assert!(
            writes <= 2 * PRESSES,
            "{PRESSES} presses booked {writes} store writes; the re-assert budget is per-snapshot again"
        );
    }

    /// Codex P2 on PR #152. A press must always be able to repair a store that
    /// disagrees with it, however the previous burst ended. Bounding the
    /// re-assert to one per BURST used the outstanding debt as the burst
    /// boundary — and an unresolved debt never clears, so once one burst ended
    /// with the store holding the wrong value, every later press was classified
    /// as part of that dead burst and the bar could never write the correction.
    /// Store and bar then disagreed for the life of the plugin instance.
    #[test]
    fn a_press_arriving_on_an_unresolved_debt_can_still_repair_the_store() {
        let mut m = BarModel::default();
        m.apply_snapshot(collapse_snap(1, false));
        m.toggle(); // owes `true`
        assert!(
            m.apply_snapshot(collapse_snap(2, false))
                .iter()
                .any(|e| matches!(e, Effect::PersistCollapse { collapsed: true })),
            "the first contradiction must book the repair"
        );
        assert!(
            !m.apply_snapshot(collapse_snap(3, false))
                .iter()
                .any(|e| matches!(e, Effect::PersistCollapse { .. })),
            "the repair is once per press, not once per snapshot"
        );
        // That debt is never settled — the store ended the burst holding the
        // wrong value, which is exactly the out-of-order landing #5 exists for.
        // A NEW press is a new intent and owes the store a new write.
        m.toggle(); // owes `false`
        assert!(
            m.apply_snapshot(collapse_snap(4, true))
                .iter()
                .any(|e| matches!(e, Effect::PersistCollapse { collapsed: false })),
            "a press arriving on an unresolved debt could never repair the store"
        );
    }

    /// Last press wins: a burst leaves the bar in the mode the final press
    /// asked for, and a snapshot that is merely BEHIND cannot overrule it.
    #[test]
    fn the_last_press_wins_over_a_lagging_snapshot() {
        let mut m = BarModel::default();
        m.apply_snapshot(collapse_snap(1, false));
        m.toggle(); // -> collapsed
        m.toggle(); // -> expanded
        m.toggle(); // -> collapsed, and this is the press that must stand
        assert!(m.collapsed);
        // Two snapshots still carrying the pre-burst value.
        m.apply_snapshot(collapse_snap(2, false));
        m.apply_snapshot(collapse_snap(3, false));
        assert!(
            m.collapsed,
            "a store one burst behind overruled the user's last press"
        );
        // Once the store catches up, the debt settles and nothing moves.
        m.apply_snapshot(collapse_snap(4, true));
        assert!(m.collapsed);
    }

    // === Property tests (issue #10 item 3) =================================
    // proptest generalizes the example-based tests over the model's
    // divergence-critical invariants. Each property cites the ledger finding
    // it guards. Host-side only: proptest is a dev-dependency and never reaches
    // the wasm --target build (see crates/clave-bar/Cargo.toml).
    mod proptests {
        use super::super::*;
        use super::{agent, agent_at, tab};
        use proptest::prelude::*;

        proptest! {
                            // Each case drives a full feedback loop; 128 keeps CI modest while
                            // still covering the start×step×floor×interrupt space densely.
                            #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

                            /// Property 2 — focus never reorders (§6.6: focus is a beacon, not a
                            /// commitment). Any assignment of the active flag, plus a beacon on
                            /// that tab, leaves rows() order identical.
                            ///
                            /// Extended for S1 with the status-only leg: a `Stop`/`SessionEnd`
                            /// shaped snapshot — statuses change, ordinals do not — must also
                            /// leave the order untouched. That is the bar-side mirror of the
                            /// maintainer's ruling; the host-side half is
                            /// `prop_only_prompts_change_the_order`.
                            #[test]
                            fn prop_focus_never_reorders(
                                n in 1usize..=5,
                                timeline in prop::collection::vec(0u64..1000, 5),
                            ) {
                                let ids: Vec<usize> = (0..n).map(|i| 10 + i).collect();
                                let build = |active: usize| {
                                    let mut m = BarModel::default();
                                    let tabs: Vec<_> = ids
                                        .iter()
                                        .enumerate()
                                        .map(|(i, &id)| tab(id, i, &format!("t{id}"), i == active))
                                        .collect();
                                    m.apply_tabs(tabs);
                                    let tl: std::collections::BTreeMap<usize, u64> = ids
                                        .iter()
                                        .enumerate()
                                        .map(|(i, &id)| (id, timeline[i]))
                                        .collect();
                                    m.apply_snapshot(AgentSnapshot { collapsed: false, seq: 1, agents: vec![], tab_order: tl, order: OrderMode::default(), today: 0, tab_buckets: Default::default(), tab_touched: Default::default() });
                                    m
                                };
                                let baseline: Vec<RowKey> =
                                    build(0).rows().into_iter().map(|(k, _)| k).collect();
                                for (active, &id) in ids.iter().enumerate() {
                                    let mut m = build(active);
                                    m.beacon(id); // live-focus truth on a different tab
                                    let order: Vec<RowKey> = m.rows().into_iter().map(|(k, _)| k).collect();
                                    prop_assert_eq!(&order, &baseline, "focus reordered rows");
                                }

                                // A status-only push: same tab order, agents whose statuses have
                                // all moved. Nothing may shift.
                                let mut m = build(0);
                                let before: Vec<RowKey> = m.rows().into_iter().map(|(k, _)| k).collect();
                                let tl: std::collections::BTreeMap<usize, u64> =
                                    ids.iter().enumerate().map(|(i, &id)| (id, timeline[i])).collect();
                                // Each push needs a STRICTLY newer seq or the §5 gate discards
                                // it — `build(0)` already applied seq 1, so a constant seq 2
                                // here would land only the first status and silently re-assert
                                // the same state twice (CodeRabbit, PR #135). A leg that cannot
                                // fail is worse than no leg: it reads as three statuses covered.
                                for (i, status) in [Status::Done, Status::Idle, Status::Failed]
                                    .into_iter()
                                    .enumerate()
                                {
                                    m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                        collapsed: false,
                                        seq: 2 + i as u64,
                                        agents: ids
                                            .iter()
                                            .map(|&id| agent(&format!("u{id}"), status, Some(id)))
                                            .collect(),
                                        tab_order: tl.clone(),
                                    });
                                    let after: Vec<RowKey> = m.rows().into_iter().map(|(k, _)| k).collect();
                                    prop_assert_eq!(&after, &before, "a status-only change reordered rows");
                                }
                            }

                            /// Property 3 — rows() is deterministic and ordinal-ordered in TWO
                            /// BLOCKS (#112): every live row precedes every dormant one, and
                            /// within each block the ordinal is non-increasing. Live tabs key
                            /// on the STORE's tab order (NOT the bound agent's last_interacted
                            /// — the round-6 divergence), dormant rows on their commitment
                            /// ordinal.
                            ///
                            /// Non-increasing ACROSS the join is exactly what segregation
                            /// gives up: a dormant row may outrank the live row above it, and
                            /// the generators here produce that case freely.
                            ///
                            /// S1 retargeted both dormant legs from `last_interacted` to
                            /// `commit_ord`. Every agent here carries a wall clock that
                            /// CONTRADICTS its ordinal, so a comparator that fell back to the
                            /// clock fails this rather than passing for the wrong reason.
                            #[test]
                            fn prop_rows_deterministic_and_recency_desc(
                                n in 1usize..=4,
                                tl_vals in prop::collection::vec(0u64..500, 4),
                                li_vals in prop::collection::vec(0u64..500, 4),
                                dormant_ords in prop::collection::vec(0u64..500, 0..4),
                            ) {
                                let mut m = BarModel::default();
                                let ids: Vec<usize> = (0..n).map(|i| 10 + i).collect();
                                let tabs: Vec<_> = ids
                                    .iter()
                                    .enumerate()
                                    .map(|(i, &id)| tab(id, i, &format!("t{id}"), i == 0))
                                    .collect();
                                m.apply_tabs(tabs);
                                // Each live tab carries a bound agent whose last_interacted is
                                // INDEPENDENT of its timeline stamp — the case that separates a
                                // timeline sort from a last_interacted sort (round 6).
                                let mut agents: Vec<Agent> = ids
                                    .iter()
                                    .enumerate()
                                    .map(|(i, &id)| {
                                        let mut a = agent(&format!("bound{id}"), Status::Working, Some(id));
                                        a.last_interacted = li_vals[i];
                                        a
                                    })
                                    .collect();
                                let mut ord_by_uuid: std::collections::BTreeMap<String, u64> = Default::default();
                                for (j, ord) in dormant_ords.iter().enumerate() {
                                    let mut a = agent(&format!("d{j}"), Status::Idle, None);
                                    a.commit_ord = *ord;
                                    // Adversarial clock: inverted relative to the ordinal, so a
                                    // last_interacted fallback cannot pass this property.
                                    a.last_interacted = 500 - *ord;
                                    ord_by_uuid.insert(a.uuid.clone(), *ord);
                                    agents.push(a);
                                }
                                let timeline: std::collections::BTreeMap<usize, u64> = ids
                                    .iter()
                                    .enumerate()
                                    .map(|(i, &id)| (id, tl_vals[i]))
                                    .collect();
                                m.apply_snapshot(AgentSnapshot { collapsed: false, seq: 1, agents, tab_order: timeline.clone(), order: OrderMode::default(), today: 0, tab_buckets: Default::default(), tab_touched: Default::default() });

                                // Determinism: identical inputs → identical rows.
                                prop_assert_eq!(m.rows(), m.rows());

                                let rows = m.rows();
                                let ts_of = |k: &RowKey| -> u64 {
                                    match k {
                                        RowKey::Tab(id) => timeline.get(id).copied().unwrap_or(0),
                                        RowKey::Dormant(u) => ord_by_uuid.get(u).copied().unwrap_or(0),
                                    }
                                };
                                // The blocks are contiguous, and the live one holds every tab.
                                let live_len = BarModel::live_block_len(&rows);
                                prop_assert_eq!(
                                    live_len, n,
                                    "every live tab belongs to the leading block"
                                );
                                prop_assert!(
                                    rows[live_len..]
                                        .iter()
                                        .all(|(k, _)| matches!(k, RowKey::Dormant(_))),
                                    "a live row rendered below a dormant one"
                                );
                                // Ordinal non-increasing WITHIN each block. Deliberately not
                                // asserted across the join — see the doc comment.
                                for block in [&rows[..live_len], &rows[live_len..]] {
                                    for w in block.windows(2) {
                                        prop_assert!(
                                            ts_of(&w[0].0) >= ts_of(&w[1].0),
                                            "recency inverted between {:?} and {:?}",
                                            w[0].0, w[1].0
                                        );
                                    }
                                }
                            }

                            /// Property 4 — the §5 seq gate: a snapshot with seq ≤ current is
                            /// fully discarded (C5 round 5), leaving rows() AND the timeline
                            /// untouched.
                            #[test]
                            fn prop_stale_snapshot_is_a_full_noop(
                                cur_seq in 1u64..=50,
                                stale_delta in 0u64..=50,
                                tl0 in prop::collection::btree_map(0usize..8, 0u64..500, 0..5),
                                tl1 in prop::collection::btree_map(0usize..8, 0u64..500, 0..5),
                            ) {
                                let stale_seq = cur_seq.saturating_sub(stale_delta); // ≤ cur_seq
                                let mut m = BarModel::default();
                                m.apply_tabs(vec![tab(0, 0, "a", true), tab(1, 1, "b", false)]);
                                m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                    collapsed: false,
                                    seq: cur_seq,
                                    agents: vec![agent("u1", Status::Working, Some(0))],
                                    tab_order: tl0,
                                });
                                let rows0 = m.rows();
                                let timeline0 = m.tab_order.clone();
                                m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                    collapsed: false,
                                    seq: stale_seq,
                                    agents: vec![agent("u2", Status::Failed, Some(1))],
                                    tab_order: tl1,
                                });
                                prop_assert_eq!(m.rows(), rows0, "stale snapshot mutated rows");
                                prop_assert_eq!(m.tab_order.clone(), timeline0, "stale snapshot mutated timeline");
                            }

                            /// Property 5 — the timeline is REPLACED wholesale, never merged
                            /// (C5 round 5: per-instance merges diverged). After a strictly
                            /// newer snapshot with timeline T, the model's timeline == T exactly,
                            /// regardless of prior state.
                            #[test]
                            fn prop_timeline_is_replaced_wholesale(
                                tl0 in prop::collection::btree_map(0usize..8, 0u64..500, 0..5),
                                tl1 in prop::collection::btree_map(0usize..8, 0u64..500, 0..5),
                            ) {
                                let mut m = BarModel::default();
                                m.apply_snapshot(AgentSnapshot { collapsed: false, seq: 1, agents: vec![], tab_order: tl0, order: OrderMode::default(), today: 0, tab_buckets: Default::default(), tab_touched: Default::default() });
                                m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                    collapsed: false,
                                    seq: 2,
                                    agents: vec![agent("u1", Status::Working, Some(3))],
                                    tab_order: tl1.clone(),
                                });
                                prop_assert_eq!(m.tab_order.clone(), tl1);
                            }

                            /// Property 6 — no two RENDERED rows share a non-zero ordinal
                            /// (S1 §3.2, stated honestly).
                            ///
                            /// The `0` class is excluded deliberately, and this is the whole
                            /// reason the property is phrased this way rather than as "no ties
                            /// ever": rows that never received a commitment all sit at 0, which
                            /// is a reachable and correct state. Claiming the stronger property
                            /// would be false and would make this proptest a lie.
                            #[test]
                            fn prop_rows_no_duplicate_nonzero_ordinal(
                                n in 1usize..=4,
                                tl_vals in prop::collection::vec(0u64..40, 4),
                                dormant_ords in prop::collection::vec(0u64..40, 0..4),
                            ) {
                                let mut m = BarModel::default();
                                let ids: Vec<usize> = (0..n).map(|i| 10 + i).collect();
                                m.apply_tabs(
                                    ids.iter()
                                        .enumerate()
                                        .map(|(i, &id)| tab(id, i, &format!("t{id}"), i == 0))
                                        .collect(),
                                );
                                // Live tabs draw ordinals from the generated map. Dormant rows
                                // are UNBOUND (tab_id: None), which is what excludes the RC-A
                                // eviction shape by construction — an agent can never be both
                                // dormant and holding a live tab's id here.
                                let mut agents: Vec<Agent> = Vec::new();
                                for (j, ord) in dormant_ords.iter().enumerate() {
                                    let mut a = agent(&format!("d{j}"), Status::Idle, None);
                                    a.commit_ord = *ord;
                                    agents.push(a);
                                }
                                let timeline: std::collections::BTreeMap<usize, u64> =
                                    ids.iter().enumerate().map(|(i, &id)| (id, tl_vals[i])).collect();
                                m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                    collapsed: false,
                                    seq: 1,
                                    agents,
                                    tab_order: timeline.clone(),
                                });

                                // Duplicates are only a defect above zero — but the ordinals
                                // here are GENERATED, not minted, so a collision is possible by
                                // construction. Skip those draws rather than assert a falsehood.
                                let mut all: Vec<u64> = timeline.values().copied().collect();
                                all.extend(dormant_ords.iter().copied());
                                let nonzero: Vec<u64> = all.iter().copied().filter(|v| *v != 0).collect();
                                let mut uniq = nonzero.clone();
                                uniq.sort_unstable();
                                uniq.dedup();
                                prop_assume!(uniq.len() == nonzero.len());

                                // With distinct inputs, no two rendered rows may share a key.
                                let mut seen = std::collections::BTreeSet::new();
                                for (k, _) in m.rows() {
                                    let ord = match &k {
                                        RowKey::Tab(id) => timeline.get(id).copied().unwrap_or(0),
                                        RowKey::Dormant(u) => {
                                            m.agents.iter().find(|a| &a.uuid == u).map(|a| a.commit_ord).unwrap_or(0)
                                        }
                                    };
                                    if ord != 0 {
                                        prop_assert!(seen.insert(ord), "two rows share ordinal {}", ord);
                                    }
                                }
                            }

                            /// Property 7 — THE HEADLINE PROPERTY. Closing a tab preserves the
                            /// relative order of everything else (R2, as amended by #116).
                            ///
                            /// For arbitrary tabs, ordinals and a chosen tab to close: the
                            /// sequence of surviving row keys after the close equals the
                            /// sequence before it, with the closed tab's row removed. Run twice
                            /// — once with the store's prune applied, once WITHOUT it — so both
                            /// legs of the carry are covered: the durable one and the
                            /// render-side one that must hold on the very first repaint.
                            ///
                            /// Relative order, never a literal index: #112's live/dormant
                            /// segregation will move indices by design, and this property must
                            /// survive it untouched.
                            #[test]
                            fn prop_close_preserves_relative_order(
                                n in 2usize..=4,
                                tl_vals in prop::collection::vec(1u64..40, 4),
                                // Each agent's OWN ordinal, generated independently of its
                                // tab's. They diverge for real: `clave add` creates the tab
                                // before it writes the row, so the birth touch and the row's
                                // mint can arrive in either order (Codex, PR #135). Generating
                                // them together would have hidden that entirely — as the first
                                // version of this property did.
                                own_vals in prop::collection::vec(0u64..40, 4),
                                victim in 0usize..4,
                                pruned in prop::bool::ANY,
                            ) {
                                let ids: Vec<usize> = (0..n).map(|i| 10 + i).collect();
                                let victim = victim % n;
                                let victim_id = ids[victim];
                                // Distinct ordinals only — ties are the `0`/eviction residual
                                // covered by property 6, not this one.
                                let vals: Vec<u64> = tl_vals.iter().take(n).copied().collect();
                                let owns: Vec<u64> = own_vals.iter().take(n).copied().collect();
                                // A row's rank is the higher of its two ordinals. Ties between
                                // ROWS are the `0`/eviction residual covered by property 6, not
                                // this one, so only distinct effective ranks are considered.
                                let ranks: Vec<u64> = vals
                                    .iter()
                                    .zip(&owns)
                                    .map(|(t, o)| *t.max(o))
                                    .collect();
                                let mut uniq = ranks.clone();
                                uniq.sort_unstable();
                                uniq.dedup();
                                prop_assume!(uniq.len() == ranks.len());

                                let timeline: std::collections::BTreeMap<usize, u64> =
                                    ids.iter().enumerate().map(|(i, &id)| (id, vals[i])).collect();
                                // Every tab hosts an agent, so every close produces a dormant
                                // row. Each agent carries its OWN ordinal, independent of its
                                // tab's — including the case where the row's is higher, which
                                // is what `clave add`'s tab-before-row sequencing produces.
                                let agents: Vec<Agent> = ids
                                    .iter()
                                    .enumerate()
                                    .map(|(i, &id)| {
                                        let mut a = agent(&format!("u{id}"), Status::Idle, Some(id));
                                        a.commit_ord = owns[i];
                                        a
                                    })
                                    .collect();

                                let mut m = BarModel::default();
                                m.apply_tabs(
                                    ids.iter()
                                        .enumerate()
                                        .map(|(i, &id)| tab(id, i, &format!("t{id}"), i == 0))
                                        .collect(),
                                );
                                m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                    collapsed: false,
                                    seq: 1,
                                    agents: agents.clone(),
                                    tab_order: timeline.clone(),
                                });
                                let before: Vec<RowKey> = m
                                    .rows()
                                    .into_iter()
                                    .map(|(k, _)| k)
                                    .filter(|k| k != &RowKey::Tab(victim_id))
                                    .collect();
                                // The rank the row holds while it is still LIVE. Comparing this
                                // against its dormant rank below is what makes the property see
                                // a live/dormant key mismatch at all — comparing only survivors
                                // cannot, because they are all still live and all still keyed
                                // the same way (Codex, PR #135).
                                let victim_tab = m
                                    .tabs
                                    .iter()
                                    .find(|t| t.tab_id == victim_id)
                                    .expect("victim tab")
                                    .clone();
                                let live_rank = m.live_ord(&victim_tab);

                                // The close. The tab leaves the tab list either way; the store
                                // echo may or may not have landed yet.
                                m.apply_tabs(
                                    ids.iter()
                                        .filter(|&&id| id != victim_id)
                                        .enumerate()
                                        .map(|(i, &id)| tab(id, i, &format!("t{id}"), i == 0))
                                        .collect(),
                                );
                                if pruned {
                                    let carried = timeline[&victim_id];
                                    let agents: Vec<Agent> = agents
                                        .iter()
                                        .cloned()
                                        .map(|mut a| {
                                            if a.tab_id == Some(victim_id) {
                                                a.tab_id = None;
                                                // `max`, mirroring the store's carry — an
                                                // assignment here would model a demotion the
                                                // real prune refuses to perform.
                                                a.commit_ord = a.commit_ord.max(carried);
                                            }
                                            a
                                        })
                                        .collect();
                                    let mut tl = timeline.clone();
                                    tl.remove(&victim_id);
                                    m.apply_snapshot(AgentSnapshot { collapsed: false, seq: 2, agents, tab_order: tl, order: OrderMode::default(), today: 0, tab_buckets: Default::default(), tab_touched: Default::default() });
                                }

                                let closed = RowKey::Dormant(format!("u{victim_id}"));
                                let after: Vec<RowKey> = m
                                    .rows()
                                    .into_iter()
                                    .map(|(k, _)| k)
                                    .filter(|k| k != &closed)
                                    .collect();
                                prop_assert_eq!(
                                    &after, &before,
                                    "close reordered survivors (pruned={})", pruned
                                );
                                // …and the closed row is still present.
                                prop_assert!(m.rows().iter().any(|(k, _)| k == &closed));
                                // The substantive half: the closed row's ORDERING KEY is the
                                // ordinal its tab held. Asserting the key rather than an index
                                // is what makes this survive #112's segregation — under either
                                // layout the row is ranked by the same number — while still
                                // failing loudly if either leg of the carry is dropped.
                                let row = m
                                    .agents
                                    .iter()
                                    .find(|a| a.uuid == format!("u{victim_id}"))
                                    .expect("the closed tab's agent");
                                prop_assert_eq!(
                                    m.dormant_ord(row),
                                    ranks[victim],
                                    "the closed row's rank changed on close (pruned={})", pruned
                                );
                                // The same claim stated as the identity that actually matters:
                                // closing a tab must not change WHICH NUMBER ranks the row.
                                // This is segregation-proof — it is about the key, not the
                                // position — and it is the assertion that catches a live_ord
                                // and dormant_ord that disagree.
                                prop_assert_eq!(
                                    live_rank,
                                    m.dormant_ord(row),
                                    "live and dormant rank the same row differently (pruned={})", pruned
                                );
                            }

                            /// Property 5b — collapse hydration converges (issue #5, C8
                            /// parity-desync): under any interleaving of seq-fresh
                            /// snapshots, LOCAL toggles, and stale snapshots, the model
                            /// follows the pending-ledger contract: stale snapshots write
                            /// nothing (the seq gate); with no debt pending, a fresh
                            /// snapshot imposes the store flag; while a toggle's write is
                            /// owed, a confirming flag settles the debt, the first
                            /// contradiction is absorbed (user truth kept, one re-assert),
                            /// and a second contradiction yields to the store. The fold
                            /// below IS that spec restated — the mutation-catching burden
                            /// for each branch sits with the example tests around
                            /// `out_of_order_write_is_reasserted_once_then_store_wins`.
                            #[test]
                            fn prop_collapse_follows_last_accepted_writer(
                                ops in prop::collection::vec(
                                    prop_oneof![
                                        prop::bool::ANY.prop_map(Some), // fresh snapshot carrying f
                                        Just(None),                            // local toggle
                                    ],
                                    1..=16,
                                ),
                                stale_at in prop::collection::vec(prop::bool::ANY, 1..=16),
                            ) {
                                let mut m = BarModel::default();
                                let mut seq = 0u64;
                                let mut expected = false; // born expanded
                                // The spec-fold's ledger mirror: what the store is owed.
                                let mut pending: Option<bool> = None;
                                let mut reasserted = false;
                                for (op, inject_stale) in ops.iter().zip(stale_at.iter()) {
                                    match op {
                                        Some(flag) => {
                                            seq += 1;
                                            m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                                collapsed: *flag,
                                                seq,
                                                agents: vec![],
                                                tab_order: Default::default(),
                                            });
                                            match pending {
                                                Some(w) if *flag == w => pending = None,
                                                Some(_) if !reasserted => reasserted = true,
                                                // #137: no give-up arm. While a write is owed,
                                                // the store's flag is ignored — the press stands.
                                                Some(_) => {}
                                                None => expected = *flag,
                                            }
                                        }
                                        None => {
                                            m.toggle();
                                            expected = !expected;
                                            pending = Some(expected);
                                            // One re-assert per press: a press is always allowed
                                            // to repair a store that disagrees with it, however
                                            // the previous burst ended. Writes stay bounded
                                            // because the give-up arm above is gone, not because
                                            // the repair is rationed across presses (Codex P2 on
                                            // PR #152 — rationing it wedged the ledger).
                                            reasserted = false;
                                        }
                                    }
                                    if *inject_stale {
                                        // A replayed/out-of-order snapshot (seq <= current)
                                        // carrying the OPPOSITE flag must change nothing —
                                        // not even the pending ledger.
                                        m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                            collapsed: !expected,
                                            seq,
                                            agents: vec![],
                                            tab_order: Default::default(),
                                        });
                                    }
                                    prop_assert_eq!(m.collapsed, expected);
                                }
                            }

                            /// Property 6 — nav closure (§6.6 C8), scoped precisely (fugu
                            /// review 2026-07-20): with the executor pinned to its birth tab
                            /// (as below — a single-instance view), every nav bumps
                            /// cursor_gen exactly once (its consumer is #92's read timer,
                            /// post-#100), and — the #112 invariant — a dir walk NEVER
                            /// leaves the live block however long it runs.
                            ///
                            /// That last leg replaces "a dormant cursor names a displayed
                            /// row", which a live-only ring makes vacuous: the walk can no
                            /// longer set a cursor at all, so asserting about one would be a
                            /// test that cannot fail. Every generated case here has at least
                            /// one live tab; the zero-live fallback is a unit test.
                            ///
                            /// Walk PROGRESSION across focus-following executors (the live
                            /// multi-instance behavior, where own_tab moves with focus) is
                            /// deliberately not modeled here — that is live-validation
                            /// territory (TESTING.md).
                            #[test]
                            fn prop_nav_closure(
                                n in 1usize..=4,
                                dormant in 0usize..=3,
                                dirs in prop::collection::vec(any::<bool>(), 1..=12),
                            ) {
                                let mut m = BarModel::default();
                                let ids: Vec<usize> = (0..n).map(|i| 10 + i).collect();
                                let tabs: Vec<_> = ids
                                    .iter()
                                    .enumerate()
                                    .map(|(i, &id)| tab(id, i, &format!("t{id}"), i == 0))
                                    .collect();
                                m.apply_tabs(tabs);
                                let mut agents = vec![];
                                for j in 0..dormant {
                                    let mut a = agent(&format!("d{j}"), Status::Idle, None);
                                    a.last_interacted = 100 + j as u64; // distinct, all dormant
                                    agents.push(a);
                                }
                                m.apply_snapshot(AgentSnapshot {
                order: OrderMode::default(),
                today: 0,
                tab_buckets: Default::default(),
        tab_touched: Default::default(),
                                    collapsed: false,
                                    seq: 1,
                                    agents,
                                    tab_order: ids.iter().map(|&id| (id, 50u64)).collect(),
                                });
                                m.beacon(ids[0]);
                                let own = ids[0];
                                for d in dirs {
                                    let before = m.cursor_gen;
                                    let payload = if d { "{\"dir\":\"next\"}" } else { "{\"dir\":\"prev\"}" };
                                    m.nav(payload, Some(own));
                                    // Rows are non-empty (≥1 live tab) → every dir lands once.
                                    prop_assert_eq!(m.cursor_gen, before + 1, "gen did not bump once per nav");
                                    // #112: the ring is the live block. However many dormant
                                    // rows are rendered below it, a walk never reaches one —
                                    // so it never leaves a selection behind, and it always
                                    // ends on a live tab.
                                    prop_assert!(
                                        m.cursor.is_none(),
                                        "a dir walk fell into the dormant block: {:?}",
                                        m.cursor
                                    );
                                    prop_assert!(
                                        m.current_tab.is_some_and(|t| ids.contains(&t)),
                                        "the walk landed outside the live block: {:?}",
                                        m.current_tab
                                    );
                                }
                            }

                            /// Property 8 (#55) — the invariant RC-A violates, stated
                            /// directly: a `Bind` never names a tab that does not hold the
                            /// agent's registered pane.
                            ///
                            /// The strong claim is scoped to frames delivered from the SAME
                            /// world version, which is exactly the scope the coherence witness
                            /// can defend. A position-preserving identity permutation (close
                            /// the lowest tab, create one in the same window) satisfies the
                            /// witness across two different worlds — the documented residual
                            /// — so asserting over it would be asserting a property the data
                            /// cannot support. The unscoped half is still universal and is
                            /// asserted alongside: nothing is ever emitted from an incoherent
                            /// frame.
                            #[test]
                            fn prop_bind_never_names_a_tab_that_does_not_hold_the_pane(
                                ops in prop::collection::vec(0u8..6, 1..40),
                            ) {
                                let mut w = IdentityWorld::new();
                                for op in ops {
                                    w.step(op)?;
                                }
                            }

                            /// Property 9 (#55) — the anti-storm rule as a universal: a bind
                            /// is never re-emitted until the store makes PROGRESS or our own
                            /// identity changes. Frames, renders, pipes and timers do not
                            /// advance `seq`, so a quiescent store costs zero subprocesses
                            /// however many events arrive — which is precisely what keeps the
                            /// self-healing retry out of the C5 rd-4 echo-gated storm class
                            /// (that guard re-fired per TabUpdate and exhausted the zellij
                            /// server's file descriptors). Asserted inside `settle`, where the
                            /// (own tab, seq) pair each emission happened under is known.
                            #[test]
                            fn prop_binds_never_repeat_without_store_progress(
                                ops in prop::collection::vec(0u8..6, 1..40),
                            ) {
                                let mut w = IdentityWorld::new();
                                for op in ops {
                                    w.step(op)?;
                                }
                            }
                        }

        /// A tiny zellij + store world for properties 8 and 9. Tabs live at
        /// their vector index (position), each carrying one bar pane and one
        /// terminal pane whose ids are STABLE — the renumbering is the point.
        /// We are the bar in tab `OWN_TAB`; agent `u1` owns the terminal pane
        /// of tab `AGENT_TAB` (the same tab, so binds are ours to make).
        struct IdentityWorld {
            m: BarModel,
            /// tab ids at their current positions.
            tabs: Vec<usize>,
            active: usize,
            next_tab: usize,
            /// Bumped by every mutation of `tabs` — two frames stamped with
            /// the same version describe the same world.
            version: u64,
            /// The (world version, tab count) each delivered frame was stamped
            /// with, or None before that frame kind has ever arrived. Judged
            /// from the WORLD, never from the model: a witness that grades its
            /// own homework proves nothing.
            tabs_frame: Option<(u64, Vec<usize>)>,
            panes_frame: Option<(u64, Vec<usize>)>,
            seq: u64,
            /// Which of the two contenders currently holds the tab, flipped
            /// on every store push — `apply_bind` evicts the incumbent, so a
            /// real fight produces exactly this alternation of confirmations.
            u1_holds: bool,
            /// Every (uuid, own tab, seq) a bind has been emitted under.
            /// Property 9 asserts this set never takes the same triple twice —
            /// tracking only the MOST RECENT triple was weaker than the
            /// property claimed, because two agents emitting in one settle
            /// reset each other's counter.
            emitted: BTreeSet<(String, usize, u64)>,
        }

        /// Tab id → its bar pane / its terminal pane. Stable across closes.
        fn bar_pane(tab_id: usize) -> u32 {
            100 + tab_id as u32
        }
        fn term_pane(tab_id: usize) -> u32 {
            200 + tab_id as u32
        }

        impl IdentityWorld {
            const OWN_TAB: usize = 11;

            fn new() -> Self {
                let mut m = BarModel::default();
                m.set_own_pane(bar_pane(Self::OWN_TAB));
                // TWO agents on ONE pane: both resolve into the same tab, so
                // every generated run can produce the eviction contention that
                // a single-agent world structurally cannot (the gap that let
                // the unbounded ping-pong through review on PR #120).
                m.register("u1".into(), term_pane(Self::OWN_TAB));
                m.register("u2".into(), term_pane(Self::OWN_TAB));
                Self {
                    m,
                    tabs: vec![10, 11, 12],
                    active: Self::OWN_TAB,
                    next_tab: 13,
                    version: 0,
                    tabs_frame: None,
                    panes_frame: None,
                    seq: 0,
                    u1_holds: true,
                    emitted: BTreeSet::new(),
                }
            }

            /// The tab truly holding our agent's pane in the world `tabs`
            /// describes, or None if that tab had been closed by then. Judged
            /// against the world the FRAMES describe, not the world now: a bar
            /// acting on a coherent pair of stale frames is acting correctly
            /// on the information it has, and the store's own bind is
            /// idempotent about a tab that has since died.
            fn true_tab_in(tabs: &[usize]) -> Option<usize> {
                tabs.contains(&Self::OWN_TAB).then_some(Self::OWN_TAB)
            }

            fn step(&mut self, op: u8) -> Result<(), TestCaseError> {
                match op {
                    // Close a tab (never the last remaining one) — this is the
                    // renumbering that RC-A rides.
                    0 | 1 if self.tabs.len() > 1 => {
                        let i = self.version as usize % self.tabs.len();
                        self.tabs.remove(i);
                        self.active = self.tabs[0];
                        self.version += 1;
                    }
                    // Create a tab.
                    2 => {
                        self.tabs.push(self.next_tab);
                        self.active = self.next_tab;
                        self.next_tab += 1;
                        self.version += 1;
                    }
                    // Deliver a TabUpdate.
                    3 => {
                        let metas: Vec<TabMeta> = self
                            .tabs
                            .iter()
                            .enumerate()
                            .map(|(pos, &id)| tab(id, pos, "t", id == self.active))
                            .collect();
                        self.m.apply_tabs(metas);
                        self.tabs_frame = Some((self.version, self.tabs.clone()));
                        self.settle()?;
                    }
                    // Deliver a PaneUpdate.
                    4 => {
                        let metas: Vec<PaneMeta> = self
                            .tabs
                            .iter()
                            .enumerate()
                            .flat_map(|(pos, &id)| {
                                [
                                    PaneMeta {
                                        tab_position: pos,
                                        pane_id: bar_pane(id),
                                        is_plugin: true,
                                        is_focused: false,
                                        is_floating: false,
                                        terminal_command: None,
                                        exited: false,
                                        exit_status: None,
                                    },
                                    PaneMeta {
                                        tab_position: pos,
                                        pane_id: term_pane(id),
                                        is_plugin: false,
                                        is_focused: false,
                                        is_floating: false,
                                        terminal_command: None,
                                        exited: false,
                                        exit_status: None,
                                    },
                                ]
                            })
                            .collect();
                        self.m.apply_panes(metas);
                        self.panes_frame = Some((self.version, self.tabs.clone()));
                        self.settle()?;
                    }
                    // A store push that never confirms the bind — the
                    // adversarial case for the retry cap.
                    _ => {
                        self.seq += 1;
                        self.u1_holds = !self.u1_holds; // the eviction flip
                        self.m.apply_snapshot(AgentSnapshot {
                            order: OrderMode::default(),
                            today: 0,
                            tab_buckets: Default::default(),
                            tab_touched: Default::default(),
                            collapsed: false,
                            seq: self.seq,
                            agents: vec![
                                agent_at(
                                    "u1",
                                    Status::Working,
                                    self.holder(true),
                                    term_pane(Self::OWN_TAB),
                                ),
                                agent_at(
                                    "u2",
                                    Status::Working,
                                    self.holder(false),
                                    term_pane(Self::OWN_TAB),
                                ),
                            ],
                            // Seeded wide so the birth touch is mostly out of
                            // the way — these properties are about binds. A
                            // long run can still create a tab id past the seed
                            // and emit a Touch, which is harmless: only Bind is
                            // inspected, and the fail-closed assert covers all
                            // effects either way.
                            tab_order: (0..256).map(|t| (t, 100u64)).collect(),
                        });
                        self.settle()?;
                    }
                }
                Ok(())
            }

            /// The tab id an agent's snapshot row carries: the incumbent
            /// holds `OWN_TAB`, the loser holds nothing.
            fn holder(&self, is_u1: bool) -> Option<usize> {
                (is_u1 == self.u1_holds).then_some(Self::OWN_TAB)
            }

            fn settle(&mut self) -> Result<(), TestCaseError> {
                // Coherence as the WORLD defines it, computed with no help
                // from the code under test: both frame kinds must have
                // arrived, and they must describe worlds with the same number
                // of tabs (positions are always 0..n, so equal counts is
                // exactly equal position sets).
                let witness_should_hold = match (&self.tabs_frame, &self.panes_frame) {
                    (Some((_, t)), Some((_, p))) => t.len() == p.len(),
                    _ => false,
                };
                let fx = self.m.identity_effects();
                if !witness_should_hold {
                    prop_assert!(
                        fx.is_empty(),
                        "emitted {fx:?} from frames describing different tab \
                         sets — this is RC-A"
                    );
                }
                for e in &fx {
                    if let Effect::Bind { uuid, tab_id } = e {
                        // Property 9: one emission per (uuid, own tab, seq).
                        let here = (uuid.clone(), *tab_id, self.seq);
                        prop_assert!(
                            self.emitted.insert(here),
                            "re-emitted a bind for {} → tab {} without the \
                             store advancing past seq {} — the debounce is \
                             not holding",
                            uuid,
                            tab_id,
                            self.seq
                        );
                        // Same-world frames: the witness must have resolved us
                        // to the tab that genuinely holds the pane. Frames
                        // from DIFFERENT worlds that happen to share a tab
                        // count are the documented residual (a position-
                        // preserving identity permutation), which no witness
                        // constructible from these two events can catch — so
                        // the strong claim is deliberately not asserted there.
                        if let (Some((tv, tabs)), Some((pv, _))) =
                            (&self.tabs_frame, &self.panes_frame)
                            && tv == pv
                        {
                            prop_assert_eq!(
                                Some(*tab_id),
                                Self::true_tab_in(tabs),
                                "bound to a tab that does not hold the pane"
                            );
                        }
                    }
                }
                Ok(())
            }
        }
    }
}
