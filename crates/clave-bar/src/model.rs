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
// against the same numbers this seek drives the pane to, and `clave`'s KDL
// generators size the newborn pane from them — one definition, three artifacts.
use clave_types::{Agent, AgentSnapshot, BAR_TARGET_COLS, COLLAPSED_TARGET_COLS, Status};

use crate::render::{PALETTE, Provenance, Row, RowContent, RowStatus, Widths};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMeta {
    /// Zellij's STABLE tab id (survives reorders) — the recency/rename key.
    pub tab_id: usize,
    /// Current 0-based position — the PaneManifest join key (it's keyed by
    /// position, not id) and the bottom-of-list tiebreak.
    pub position: usize,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMeta {
    pub tab_position: usize,
    pub pane_id: u32,
    pub is_plugin: bool,
    pub is_focused: bool,
}

/// Side effects for main.rs to execute — kept as data so tests assert them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// rename_tab_with_id(tab_id, name) — write clave's label on the real tab.
    RenameTab { tab_id: usize, name: String },
    /// focus_pane_with_id(Terminal(pane_id)) — S2-proven; only for uuid jumps,
    /// where the pane id is broadcast truth and duplicates are same-target.
    FocusPane { pane_id: u32 },
    /// switch_tab_to(position + 1) — row/dir nav. All instances compute the
    /// same target from replicated state, so duplicates are idempotent.
    SwitchTab { position: usize },
    /// run_command zellij pipe clave-visited — converge the other instances
    /// after a single-instance jump (mouse click).
    AnnounceVisit { tab_id: usize },
    /// run_command zellij pipe clave-visited — SAME beacon as AnnounceVisit,
    /// but for the #23 stranded-beacon re-anchor after a tab close. A DISTINCT
    /// variant so run_effects can gate it to the active instance: toggle bursts
    /// deliver the fresh set to ALL instances (doc:371-394), so an ungated
    /// re-anchor announce would revive the per-instance beacon war (round-13
    /// EMFILE class). Birth/organic announces stay AnnounceVisit (ungated,
    /// live-validated).
    ReanchorVisit { tab_id: usize },
    /// run_command(["clave","focus",uuid]) — persist the unread clear.
    MarkRead { uuid: String },
    /// run_command(["clave","bind",uuid,tab_id]) — report the uuid→tab join
    /// to the STORE (§6.6 Design B), fired by the agent tab's own bar.
    Bind { uuid: String, tab_id: usize },
    /// run_command(["clave","prune-tabs", stale_ids…]) — drop store binds and
    /// tab_timeline entries for CLOSED tabs (#6/F3). Carries the OBSERVED-STALE
    /// ids (bound-or-timelined ids ABSENT from the delivered live set), NOT the
    /// live set — removing specific dead ids is idempotent and commutes, so
    /// two out-of-order prunes can't clobber a tab neither observed die (the
    /// full-live-set "retain-only" payload could unbind a tab created after the
    /// prune was computed). Executor-gated in main.rs (keeps duplicate prunes
    /// to the active bar). Zellij reuses tab_ids (screen.rs:1617), so this is
    /// correctness, not just hygiene.
    PruneTabs { stale_ids: Vec<usize> },
    /// resize_pane_with_id(Decrease→Right, own) — C6 collapse/expand seek.
    ShrinkSelf,
    /// resize_pane_with_id(Increase→Right, own) — C6 seek / overshoot
    /// recovery.
    GrowSelf,
    /// set_timeout(DWELL_SECS) on the executor — §6.6 dormant dwell. `gen`
    /// stamps the landing; expiry acts only if the cursor generation still
    /// matches (walk-through safety).
    ArmDwell { r#gen: u64 },
    /// set_timeout(PEEK_SINK_SECS) + pending_peeks bump — a dormant-row nav
    /// landing on a collapsed bar peeks like live nav does (no visited pipe
    /// exists for it, so the model asks explicitly).
    ArmPeek,
    /// run_command(["clave","open",uuid]) — §6.3. Fired by dwell expiry and
    /// explicit picks; the model has already marked the uuid in-flight (↻).
    OpenAgent { uuid: String },
    /// run_command(["clave","touch",tab_id]) — the once-EVER birth stamp for a
    /// tab the store timeline has never seen. Was an inline `run_command` in
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
}

/// §6.6 C8: user-tuned (approved 2026-07-17) — do not normalize with the
/// 0.9s peek sink.
pub const DWELL_SECS: f64 = 0.4;
pub const PEEK_SINK_SECS: f64 = 0.9;
/// Event::Timer(f64) carries ELAPSED sleep seconds (server-side, v0.44.3
/// zellij_exports.rs:2462) ≈ the requested duration — 0.4 vs 0.9 splits
/// cleanly at 0.65.
pub const TIMER_KIND_CUTOFF_SECS: f64 = 0.65;

/// The two timer kinds sharing zellij's single Event::Timer channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerKind {
    Dwell,
    Peek,
}

/// Which timer kind does an expiry belong to? Normally split by elapsed
/// (dwell 0.4 vs peek 0.9 — Timer carries the server-side elapsed sleep).
/// HARDENING: a dwell timer delayed past the cutoff would otherwise never
/// pop its gen and the FIFO would be off-by-one for the life of the
/// instance (every later dwell pops the PREVIOUS landing's gen — a
/// latching failure). When no peeks are pending but dwells are, a
/// long-elapsed expiry can only be a late dwell — classify it as one.
/// The reverse misclassification is impossible: a 0.9s sleep never
/// reports < 0.9 elapsed. The collapsed-walk case heals the same way: a
/// dormant landing arms BOTH a dwell and a peek, so an orphaned peek timer
/// is reclassified as the late dwell and the queue rebalances — delayed
/// opens are deferred, never lost.
pub fn classify_timer(elapsed: f64, pending_dwells: usize, pending_peeks: u32) -> TimerKind {
    // Short elapsed is always a dwell; a long one is a dwell ONLY when it
    // can't be a peek (none pending) yet a dwell is owed — a late dwell.
    let late_dwell = pending_peeks == 0 && pending_dwells > 0;
    if elapsed < TIMER_KIND_CUTOFF_SECS || late_dwell {
        TimerKind::Dwell
    } else {
        TimerKind::Peek
    }
}

/// Seek steps allowed per toggle (each is a real zellij layout action):
/// enough for the widest transition at ~5%-of-viewport per step, small
/// enough that a layout which refuses to converge isn't fought forever.
const SEEK_BUDGET: u32 = 16;
/// Deltas beyond this are external jumps (window resize, relayout), not a
/// single zellij resize step (5% of viewport ≈ 7–14 cols on real screens) —
/// learning one poisons the acceptance band (step=60 seen live, round 17:
/// it accepted a 13-col bar as "close enough" to 26).
const MAX_LEARNABLE_STEP: usize = 20;
/// The step assumed before a resize's effect has taught us zellij's real
/// increment: ±4 cols of slack, so a bar born near the target is accepted
/// rather than nudged into an overshoot dance.
const PRE_LEARNING_STEP: usize = 8;

/// Is `cols` converged on `target`, given the current `step` and the OTHER
/// target? Both halves are load-bearing.
///
/// 1. **Within half a step.** Zellij resizes in ~5%-of-display-area increments,
///    so an exact column count is simply not on the lattice (LEDGER D20) — a
///    band is what lets the seek terminate at all, and `GrowSelf` recovers an
///    overshoot from the far side.
/// 2. **Not equally converged on the other target.** The band spans `step`
///    columns, so the two targets' bands overlap as soon as
///    `step >= separation` — 14 at 44/30, i.e. a display area of roughly 280
///    columns, which the maintainer runs. A width in the overlap is converged
///    for BOTH targets, and `toggle()` deliberately KEEPS the learned step, so
///    Alt+c emits zero resize effects and the pane silently does not move
///    (LEDGER D21; reported live as "some blips" at Gate 1). Before this
///    branch the targets were 30/4 — 26 apart, wider than any learnable step —
///    so disjointness held by luck of the constants rather than by
///    construction, and 44/30 lost it with nothing turning red.
///
/// Derived from the targets themselves, deliberately **not** from a margin
/// chosen against today's numbers: whatever the two targets become (D19 moves
/// the expanded one to 54, where the overlap cannot arise at all), no width is
/// ever accepted for both. Pinned by `no_width_is_accepted_for_both_targets`.
///
/// The cost of condition 2 is paid only inside the overlap, and only on
/// displays coarse enough to have one: a width the lattice cannot improve on
/// is refused, so the seek takes one more step and the bracket rule in gate
/// (D) stops it there. That resting width is **not** guaranteed to be outside
/// the overlap — a census over the whole learnable space found 84 distinct
/// `(mode, step, width)` rests that this function itself would refuse, e.g.
/// 37 resting for both seeks in different runs. What IS guaranteed, and is
/// what the bug was about, is that **Alt+c always moves**: every target flip
/// routes through `arm_seek`, which clears `seek_rest`/`seek_last_cols`/
/// `seek_drift`, so gates (A) and (B) cannot short-circuit it and the first
/// post-flip render always re-evaluates this function against the new target.
/// Verified exhaustively: 9,640 rest states x 6 consecutive toggles, zero
/// failures to move. A visibly-collapsed bar a few columns off target beats a
/// perfectly-parked one that never moved.
fn converged(cols: usize, target: usize, other: usize, step: i64) -> bool {
    let near = |t: usize| 2 * (cols as i64 - t as i64).abs() <= step;
    near(target) && !near(other)
}

/// Row identity (§6.6 C8): a live zellij tab, or a dormant store row
/// (conversation with no tab yet — claude.ai-style list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKey {
    Tab(usize),
    Dormant(String),
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

pub struct BarModel {
    /// The collapse mode is not known yet, so the seek must not act (D37).
    ///
    /// A newborn model is `collapsed: false` because that is the only thing it
    /// can assume, but the mode PERSISTS in the store — so a fleet left
    /// collapsed loads a bar that believes it is expanded, seeks 54, and is
    /// corrected to 30 the moment `clave snapshot` returns. Ollie watched
    /// exactly that: born at the right width (D36), grown wide by the plugin,
    /// then shrunk back about half a second later.
    ///
    /// Since D36 the pane is BORN at the width its true mode wants, so any seek
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
    /// §5 pipe contract: apply only strictly-newer seq.
    seq: u64,
    agents: Vec<Agent>,
    /// uuid → terminal pane id, from clave-register (S2).
    uuid_to_pane: BTreeMap<String, u32>,
    tabs: Vec<TabMeta>,
    panes: Vec<PaneMeta>,
    /// tab_id → unix seconds of the last USER COMMITMENT to that tab
    /// (§6.6). NOT owned here: the store is the one writer (`clave touch`
    /// RMW) and this copy is REPLACED wholesale from every seq-gated
    /// snapshot — instance-local copies merged from pipe deltas diverged
    /// live (C5 round 5) and walking oscillated. Focus is deliberately NOT
    /// a commitment (the list holds still while you look around).
    timeline: BTreeMap<usize, u64>,
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
    /// Local unread-override: Done agents we've already cleared on focus.
    /// Render-side only; `clave focus` persists the real transition.
    read_locally: BTreeSet<String>,
    /// Remaining C6 width-seek steps; armed at birth (see Default) and on
    /// every toggle, zeroed when own width reaches the current target. Round 20:
    /// the bar is NEVER suppressed — Alt+c resizes each instance's OWN pane
    /// between the template width and a glyph gutter. Every instance stays
    /// visible, so every instance gets the render feedback that made the
    /// own-pane width chain the one reliable mechanism of rounds 9–19
    /// (suppress-based hide was structurally hostile: lossy re-insert,
    /// damage flag blocks swap relayouts, plugin resizes emit no events).
    seek_budget: u32,
    /// cols at our last resize action — wait until zellij's effect is
    /// visible (cols changed) before acting again, so in-flight resizes
    /// can't double-fire, and the observed delta is the LEARNED step size.
    seek_last_cols: Option<usize>,
    /// Zellij's resize increment as observed (≈5% of viewport); 0 until
    /// learned from a resize's effect. An environment property — kept
    /// across toggles.
    seek_step: usize,
    /// Width at which the seek last came to REST — converged, exhausted, or
    /// pinned against a zellij refusal wall. A render at exactly this width is
    /// a no-op (the seek is done); only a DIFFERENT width can wake it. This is
    /// the anchor the drift re-arm (issue #4) measures against: the old design
    /// zeroed the budget on convergence and then went permanently silent, so a
    /// window resize / split that drifted the pane off-target left the bar
    /// parked until the next toggle/peek (F1, C8 backlog 2026-07-18).
    seek_rest: Option<usize>,
    /// One-render grace flag for the in-flight guard. cols unchanged since our
    /// last resize means the effect either lands a beat late (latency, round 9
    /// — do NOT double-fire) or was CLOBBERED by a relayout (issue #4). We
    /// cannot tell the two apart in one render, so we grace exactly one beat
    /// (matching the modelled one-render latency) and then, if still far off
    /// target, re-drive — the budget bounds a clobber we cannot win.
    seek_stalled: bool,
    /// A candidate drifted width awaiting confirmation. While DORMANT, the very
    /// first render at an off-target width could be a transient mid-relayout
    /// value (or an oscillating/thrashing layout, round 20). We only re-arm the
    /// seek once the SAME off-target width is seen twice — a stable drift, not
    /// a flicker — so a thrashing layout and a user mid-drag never trigger a
    /// perpetual re-seek (the "must not fight forever" bound).
    seek_drift: Option<usize>,
    /// tab_id of the last visited (focused) tab — replicated on every
    /// instance from the visited-pipe/nav broadcast streams. This is the nav
    /// walk base: the local TabInfo.active flag is stale everywhere except
    /// the active instance (zellij delivery finding, C3–C5).
    current_tab: Option<usize>,
    /// Consumed by the first apply_tabs: a fresh instance announces its
    /// own-active claim once (new tab / plugin (re)load) — the only
    /// self-initiated announce an instance ever gets (rounds 11–12).
    birth_announced: bool,
    /// Armed by the Alt+o bind's `clave-organic` pipe: the NEXT TabUpdate
    /// may announce (steady-state TabUpdates reach only the truly active
    /// instance, C3). Disarmed by any incoming beacon — the active
    /// instance spoke; a stale instance must not answer a leftover flag
    /// with poison during a later event burst.
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
    cursor: Option<String>,
    /// Bumped on EVERY nav landing; ArmDwell carries it so a late timer for
    /// an abandoned landing is provably stale.
    cursor_gen: u64,
}

impl Default for BarModel {
    fn default() -> Self {
        Self {
            // Armed at BIRTH, not just on toggle: the generated layouts size
            // the bar pane in PERCENT (fixed sizes make zellij refuse every
            // resize — CantResizeFixedPanes, the Alt+c-dead finding,
            // c8-cold-start 2026-07-18), so a newborn's cols are
            // window-dependent and must converge on the template.
            seek_budget: SEEK_BUDGET,
            awaiting_hydration: false,
            seq: 0,
            agents: Vec::new(),
            uuid_to_pane: BTreeMap::new(),
            tabs: Vec::new(),
            panes: Vec::new(),
            timeline: BTreeMap::new(),
            birth_touched: BTreeSet::new(),
            renamed: BTreeMap::new(),
            own_pane: None,
            bind_sent: BTreeMap::new(),
            read_locally: BTreeSet::new(),
            seek_last_cols: None,
            seek_step: 0,
            seek_rest: None,
            seek_stalled: false,
            seek_drift: None,
            current_tab: None,
            birth_announced: false,
            organic_pending: false,
            collapsed: false,
            peeking: false,
            pending_collapse: None,
            collapse_reasserted: false,
            opening: BTreeSet::new(),
            cursor: None,
            cursor_gen: 0,
        }
    }
}

impl BarModel {
    /// A `clave-visited` pipe landed: some tab gained focus. Beacon ONLY —
    /// it elects the nav executor; it never reorders (§6.6: focus is not a
    /// commitment).
    pub fn beacon(&mut self, tab_id: usize) {
        self.current_tab = Some(tab_id);
        self.organic_pending = false; // truth arrived; leftover flags are poison
        // Any real tab visit is live-focus truth, so the §6.6 selection must
        // resolve back to the focused tab. Without this a dwell-open that
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
        self.peeking = true;
        self.arm_seek(); // re-arm toward the template
        true
    }

    /// The LAST peek timer expired (main.rs counts one per nav, so a nav
    /// burst sinks once, ~1s after the final press): sink back to the
    /// gutter. Returns whether anything changed — false when a toggle
    /// already cancelled the peek (a late timer must not re-arm the seek).
    pub fn peek_expired(&mut self) -> bool {
        if !self.peeking {
            return false;
        }
        self.peeking = false;
        self.arm_seek(); // re-arm toward the gutter
        true
    }

    /// Alt+o's bind pipes `clave-organic` alongside the native ToggleTab:
    /// arm ONE announce on the next TabUpdate (rounds 11–12: unbounded
    /// self-diagnosed announces storm; bounded triggers cannot).
    pub fn set_organic_pending(&mut self) {
        self.organic_pending = true;
    }

    /// Should this instance fire `clave touch` for a newly-active tab it has
    /// never seen? True at most ONCE per (instance, tab), and never for a
    /// tab the store timeline already carries. Duplicates across instances
    /// are fine — the store RMW max-merges same-second stamps.
    pub fn needs_birth_touch(&mut self, tab_id: usize) -> bool {
        !self.timeline.contains_key(&tab_id) && self.birth_touched.insert(tab_id)
    }

    /// §6.6 sort key: the STORE's tab timeline, nothing else. Agent prompts
    /// reach it via the hook's bind-keyed stamp (Design B) — a render-time
    /// last_interacted join here is exactly what diverged in round 6 (each
    /// instance's register/manifest state differs).
    fn sort_key(&self, t: &TabMeta) -> u64 {
        self.timeline.get(&t.tab_id).copied().unwrap_or(0)
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
    /// the four effects that latch at emit and therefore CANNOT survive a
    /// fail-closed gate — `RenameTab`, `MarkRead`, `ReanchorVisit`,
    /// `PersistCollapse` all drop silently under a false gate with no trigger
    /// to re-evaluate them, so tightening them converts a wrong-action bug
    /// into a missed-action bug. Their emit-time latch is the real defect and
    /// is deliberately out of scope for #55.
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
        // Birth touch FIRST: a newly-created tab wants its timeline stamp
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
        if !self.frames_coherent() {
            return Vec::new();
        }
        let own_position = self
            .tabs
            .iter()
            .find(|t| t.tab_id == own_tab)
            .map(|t| t.position);
        let seq = self.seq;
        let mut out = Vec::new();
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

    /// Explicit-open path (click / Alt+N / dwell expiry): mark in-flight and
    /// emit the run. The `opening` guard is double-fire protection #1
    /// (clave open's liveness no-op is #2). Stale rows may retry — the user
    /// might have restored the dir.
    fn open_effects(&mut self, uuid: &str) -> Vec<Effect> {
        if self.opening.contains(uuid) {
            return Vec::new();
        }
        self.opening.insert(uuid.to_string());
        vec![Effect::OpenAgent {
            uuid: uuid.to_string(),
        }]
    }

    /// The dwell timer for landing `gen` expired (main.rs). Opens iff the
    /// cursor still sits on that same landing and the row is still dormant.
    pub fn dwell_expired(&mut self, r#gen: u64) -> Vec<Effect> {
        if r#gen != self.cursor_gen {
            return Vec::new(); // cursor moved on — walk-through, not intent
        }
        let Some(uuid) = self.cursor.clone() else {
            return Vec::new();
        };
        let still_dormant = self
            .agents
            .iter()
            .find(|a| a.uuid == uuid)
            .is_some_and(|a| self.is_dormant(a));
        if !still_dormant {
            return Vec::new();
        }
        self.open_effects(&uuid)
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
        // The mode below is now authoritative, so the seek may act (D37).
        self.awaiting_hydration = false;
        self.agents = snap.agents;
        // REPLACE the tab timeline — the store's map is authoritative and
        // self-healing by construction; merging deltas is the exact failure
        // mode that diverged live (C5 round 5).
        self.timeline = snap.tab_timeline;
        let mut effects = Vec::new();
        // Collapse parity heal (issue #5, C8 parity-desync): once
        // seq-accepted, the store's flag is authoritative for any instance
        // with no write in flight — this is what rescues a bar born after
        // the toggle, reborn by a reload, or one that missed the broadcast.
        // ON CHANGE ONLY: re-arming per snapshot would be a perpetual-seek
        // storm (round 11), so an in-sync instance's seek state is left
        // byte-untouched.
        //
        // The pending-write ledger (fix-review MAJOR) refines that
        // authority: while we OWE the store a value, a snapshot carrying it
        // is our write (or a peer's equal one) confirming — clear the debt,
        // heal nothing. A snapshot CONTRADICTING the debt means our write
        // was swallowed by an out-of-order sibling (two rapid toggles: the
        // late-arriving stale value re-wrote the store) — keep USER truth
        // and re-assert, exactly once; a second contradiction means someone
        // else is authoritative after all, and wrong-but-consistent beats a
        // two-instance re-assert ping-pong (round 11). Accepted transient
        // (unchanged): an unrelated push between broadcast and write-landing
        // briefly disagrees; the write's own push heals it.
        match self.pending_collapse {
            Some(want) if snap.collapsed == want => {
                self.pending_collapse = None; // debt settled; local == want already
            }
            Some(want) if !self.collapse_reasserted => {
                self.collapse_reasserted = true;
                effects.push(Effect::PersistCollapse { collapsed: want });
            }
            Some(_) => {
                self.pending_collapse = None; // retry spent: store wins
                self.heal_collapse(snap.collapsed);
            }
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
        self.tabs = tabs;
        let mut effects = Vec::new();
        let live: BTreeSet<usize> = self.tabs.iter().map(|t| t.tab_id).collect();
        // #23 (2026-07-21): a tab CLOSE (Ctrl+D) can STRAND the nav beacon —
        // current_tab still names the closed tab, so executor election (which
        // wants current_tab == some instance's own live tab, main.rs
        // handle_pipe clave-nav) matches nobody and dir-nav goes dead until a
        // mouse click reseeds it. The beacon is stranded exactly when it points
        // outside the live set.
        let stranded = self.current_tab.is_some_and(|id| !live.contains(&id));
        // Bounded beacon announce (rounds 11–12). Two DISTINCT triggers, on
        // purpose:
        //   birth/organic → AnnounceVisit (UNGATED): birth's first-TabUpdate
        //     announce and Alt+o's organic one-shot are live-validated ungated;
        //     left byte-identical.
        //   stranded (#23) → ReanchorVisit (GATED in run_effects to the active
        //     instance). It CANNOT ride the ungated path: TabUpdate normally
        //     reaches only the active instance (C3), BUT toggle bursts deliver
        //     the FRESH set to ALL instances (doc:371-394, main.rs apply_tabs
        //     note) — so between the close and the reseed pipe landing, every
        //     hidden bar whose beacon is still the closed tab would ALSO trip
        //     `stranded` and pipe, reviving the per-instance beacon war that
        //     EMFILE-crashed the server (round 13). Gating pipes it once.
        // All triggers self-clear: birth fires once, organic is one-shot, and
        // the local current_tab mutation makes `stranded` false next pass on
        // EVERY instance (so even a burst-tripped hidden bar arms at most once,
        // and only the active one actually pipes). Accepted trade: if
        // is_active_instance is transiently false on the close TabUpdate
        // (PaneUpdate lag), the reseed is DROPPED and nav stays stranded until a
        // click — the pre-fix symptom, but in a narrow window and strictly
        // better than a storm.
        let birth_or_organic = !self.birth_announced || self.organic_pending;
        self.birth_announced = true;
        self.organic_pending = false; // consumed either way
        if let Some(active_id) = self.tabs.iter().find(|t| t.active).map(|t| t.tab_id)
            && self.current_tab != Some(active_id)
        {
            if birth_or_organic {
                self.current_tab = Some(active_id);
                effects.push(Effect::AnnounceVisit { tab_id: active_id });
            } else if stranded {
                self.current_tab = Some(active_id);
                effects.push(Effect::ReanchorVisit { tab_id: active_id });
            }
        }
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
        // set) so it drops their binds + tab_timeline entries. Correctness, not
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
        // (b) the store push echo clears self.agents/self.timeline, so a clean
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
    /// agent's bind and timeline entry survive into the new tab: it inherits
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
        let mut stale: BTreeSet<usize> = self
            .agents
            .iter()
            .filter_map(|a| a.tab_id)
            .filter(|id| !live.contains(id))
            .collect();
        stale.extend(
            self.timeline
                .keys()
                .copied()
                .filter(|id| !live.contains(id)),
        );
        (!stale.is_empty()).then(|| Effect::PruneTabs {
            stale_ids: stale.into_iter().collect(), // BTreeSet → sorted, deduped
        })
    }

    pub fn apply_panes(&mut self, panes: Vec<PaneMeta>) {
        self.panes = panes;
        self.prune_opening();
    }

    /// The row content for a live-or-dormant agent (lock §2). `dormant` is the
    /// caller's own classification rather than a re-derivation, because the two
    /// loops below already know which list they are walking.
    fn agent_content(&self, a: &Agent, dormant: bool, inks: &ProvisionalInks) -> RowContent {
        // Ordered, and the order is the behaviour: `stale` and `opening` are
        // model states that OUTRANK the store's `Status` (a stale row's status
        // is whatever it was when the cwd vanished), and the local unread
        // override is the last thing between `Done` and the palette. Carried
        // over unchanged from the (char, u8) glyph logic this replaces.
        let status = if a.stale {
            RowStatus::Stale
        } else if self.opening.contains(&a.uuid) {
            RowStatus::Opening
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
        // Three-state (lock §5.1), and a main checkout renders NOTHING — that
        // is the researched choice, and it is what makes the two marked states
        // mean something. NO branch is the case the design did not name: an
        // agent outside a repo has none, and painting the branch glyph for it
        // would assert a provenance nobody has — so it takes the blank.
        //
        // `"-"` is the value that matters, NOT the empty string: the host
        // writes `"-"` when `git rev-parse --abbrev-ref HEAD` fails
        // (`add::run_add`'s fallback) and `record_branch` returns `"-"` for a
        // detached-worktree resume. Nothing in the host ever writes an empty
        // branch — every `branch: String::new()` in the tree is a test builder,
        // so the empty clause is kept only to keep those honest. Verified
        // against the writer, not against a fixture.
        //
        // WHICH branch is the default is the repo's answer, never ours (#86).
        // This used to test `main`/`master` as if they were exhaustive, so a
        // `trunk`-, `develop`- or `dev`-default repository had its ORDINARY
        // checkout marked as a branch — the one row §5.1 requires to be blank,
        // mislabelled on naming convention alone. `default_branch` is resolved
        // by the host (`add::resolve_default_branch`) and rides the snapshot.
        // The name test survives only as the fallback for `None`, which is what
        // an old store row and an undiscoverable default both deserialize to:
        // no snapshot gets a WORSE answer than it got before the field existed.
        let provenance = if a.worktree.is_some() {
            Provenance::Worktree
        } else if a.branch.is_empty() || a.branch == "-" {
            Provenance::Main
        } else if let Some(default) = a.default_branch.as_deref() {
            if a.branch == default {
                Provenance::Main
            } else {
                Provenance::Branch
            }
        } else if a.branch == "main" || a.branch == "master" {
            Provenance::Main
        } else {
            Provenance::Branch
        };
        let title = a.title.clone();
        RowContent::Agent {
            status,
            // S7 has not landed. `None` renders a blank battery cell, which
            // lock §2.1 requires it to do cleanly — inventing a level would be
            // a lie in the one cell whose whole job is a measurement.
            battery: None,
            provenance,
            title_ink: title
                .as_ref()
                .and_then(|t| inks.title.get(&(a.repo_root.clone(), t.clone())).copied()),
            title,
            repo: basename(&a.repo_root).to_string(),
            repo_ink: inks.repo.get(&a.repo_root).copied(),
            summary: a.summary.clone(),
        }
    }

    /// The width profile this bar renders at. Chosen by STATE, never by the
    /// current `cols` (LEDGER D16): a peeking bar is still `collapsed`, but the
    /// peek is showing the template, so it renders EXPANDED — the same rule
    /// `width_seek` picks its target with, deliberately expressed once so the
    /// profile and the target it is seeking cannot drift apart mid-animation.
    pub fn widths(&self) -> Widths {
        if self.showing_collapsed() {
            Widths::COLLAPSED
        } else {
            Widths::EXPANDED
        }
    }

    /// The one collapse predicate. A peeking bar seeks (and renders) the
    /// template width even though it is collapsed — the collapse resumes when
    /// the peek expires.
    fn showing_collapsed(&self) -> bool {
        self.collapsed && !self.peeking
    }

    /// Rows in display order (§6.6 C8): ONE unified recency-desc list — live
    /// tabs keyed by the store tab_timeline, dormant store rows keyed by
    /// last_interacted. Tiebreak: tab position for live rows (fresh
    /// same-second tabs sit in tab order); for same-second dormant rows,
    /// stable and deterministic in uuid-DESCENDING order (uuid-ascending
    /// sort under a `usize::MAX - i` key inverts to descending).
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
        // (sort_ts desc, tiebreak asc) — tiebreak: live rows by position,
        // dormant by a large offset + stable index so they never interleave
        // nondeterministically with same-second live rows.
        let inks = ProvisionalInks::allocate(&self.agents);
        let mut entries: Vec<(u64, usize, (RowKey, Row))> = Vec::new();
        for t in &self.tabs {
            // Lock §7.1: the zellij tab name is used ONLY for a terminal tab.
            // An agent row's identity is its title chip and repo, both from the
            // store — the tab name is clave's own rename echo and would be a
            // second, drifting copy of the label.
            let content = match self.agent_in_tab(t.tab_id) {
                Some(a) => self.agent_content(a, false, &inks),
                None => RowContent::Terminal {
                    name: t.name.clone(),
                },
            };
            entries.push((
                self.sort_key(t),
                t.position,
                (
                    RowKey::Tab(t.tab_id),
                    Row {
                        content,
                        // A dormant selection steals the highlight from every tab.
                        selected: selected_dormant.is_none() && t.active,
                    },
                ),
            ));
        }
        let mut dormant: Vec<&Agent> = self.agents.iter().filter(|a| self.is_dormant(a)).collect();
        dormant.sort_by(|a, b| a.uuid.cmp(&b.uuid)); // stable tiebreak input
        for (i, a) in dormant.into_iter().enumerate() {
            entries.push((
                a.last_interacted,
                // After any same-second live row; among same-second dormant
                // rows this renders uuid-DESCENDING (uuid-asc sort, key
                // inverted) — stable and deterministic, which is all we need.
                usize::MAX - i,
                (
                    RowKey::Dormant(a.uuid.clone()),
                    Row {
                        content: self.agent_content(a, true, &inks),
                        selected: selected_dormant == Some(a.uuid.as_str()),
                    },
                ),
            ));
        }
        entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        entries.into_iter().map(|(_, _, r)| r).collect()
    }

    /// Mouse click on rendered line N (0-based): jump to that row's tab.
    /// A click reaches exactly ONE instance (the visible bar), so the jump
    /// broadcasts a beacon for the other instances' executor election.
    /// Focus is not a commitment — clicks never reorder. A click on a dormant
    /// row opens it immediately (§6.6 — no dwell for explicit picks).
    pub fn click(&mut self, line: usize) -> Vec<Effect> {
        let Some((key, _)) = self.rows().get(line).cloned() else {
            return Vec::new();
        };
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
                self.beacon(tab_id);
                vec![
                    Effect::SwitchTab { position },
                    Effect::AnnounceVisit { tab_id },
                ]
            }
            // Explicit pick: open immediately (§6.6 — no dwell for clicks).
            RowKey::Dormant(uuid) => self.open_effects(&uuid),
        }
    }

    /// The replicated focus truth — main.rs uses it as the row-jump executor
    /// gate (own tab == current_tab ⇒ this instance is the active one).
    pub fn current_tab(&self) -> Option<usize> {
        self.current_tab
    }

    /// clave-nav payloads: {"row":N} | {"uuid":"…"}. (dir walks are native
    /// clave-nav payloads: {"dir":"next"|"prev"} | {"row":N} | {"uuid":"…"}.
    /// uuid → FocusPane on EVERY instance: the pane id is broadcast truth
    /// (clave-register), so duplicates target the same pane.
    /// row (1-based, Alt+1..9) and dir both act on DISPLAY rows and run on
    /// the EXECUTOR only (`executor_own_tab` = Some(own tab) on the active
    /// instance — fresh tab set, and the very bar the user is reading; a
    /// broadcast walk over stale sets raced six divergent targets live).
    /// dir steps ±1 from the executor's own row, wrapping — safe to walk the
    /// visible list now, because focus no longer reorders it (§6.6 revised:
    /// only user commitments move rows, so there is no ping-pong).
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
        let rows = self.rows();
        let line = if let Some(n) = v.get("row").and_then(|n| n.as_u64()) {
            (n as usize).checked_sub(1) // 1-based → display line
        } else if let Some(dir) = v.get("dir").and_then(|d| d.as_str()) {
            if rows.is_empty() {
                return Vec::new();
            }
            // Walk base: the dormant cursor if set, else the executor's own
            // tab row (§6.6 C8 — the cursor IS the position while walking
            // through dormant rows).
            let cur = self
                .cursor
                .as_ref()
                .and_then(|u| {
                    rows.iter()
                        .position(|(k, _)| *k == RowKey::Dormant(u.clone()))
                })
                .or_else(|| rows.iter().position(|(k, _)| *k == RowKey::Tab(own)))
                .unwrap_or(0);
            match dir {
                "next" => Some((cur + 1) % rows.len()),
                "prev" => Some((cur + rows.len() - 1) % rows.len()),
                _ => None,
            }
        } else {
            None
        };
        let is_dir_walk = v.get("dir").is_some();
        let Some((key, _)) = line.and_then(|l| rows.get(l).cloned()) else {
            return Vec::new();
        };
        self.cursor_gen += 1; // every landing invalidates prior dwell arms
        match key {
            RowKey::Tab(tab_id) => {
                self.cursor = None; // live landing: focus truth takes over
                let Some(position) = self
                    .tabs
                    .iter()
                    .find(|t| t.tab_id == tab_id)
                    .map(|t| t.position)
                else {
                    return Vec::new();
                };
                self.beacon(tab_id); // executor hand-off hint; pipe echo confirms
                vec![
                    Effect::SwitchTab { position },
                    Effect::AnnounceVisit { tab_id },
                ]
            }
            RowKey::Dormant(uuid) => {
                if !is_dir_walk {
                    // Alt+N explicit pick: open immediately (§6.6).
                    return self.open_effects(&uuid);
                }
                self.cursor = Some(uuid);
                let mut fx = vec![Effect::ArmDwell {
                    r#gen: self.cursor_gen,
                }];
                // A collapsed bar peeks while walking dormant rows too — live
                // nav peeks via the visited pipe; there is no pipe here, so
                // arm locally on the executor (the one visible bar).
                if self.collapsed {
                    self.peeking = true;
                    self.arm_seek();
                    fx.push(Effect::ArmPeek);
                }
                fx
            }
        }
    }

    /// Re-arm the width seek toward the current target. Every user-intent
    /// trigger (birth, toggle, peek, heal) resets the SAME machine: a fresh
    /// budget, the compare base cleared (round 15: a stale last_cols "learned"
    /// a jump as a bogus step), and — issue #4 — the rest/drift trackers wiped
    /// so a new episode is not silenced by the width it previously settled at
    /// nor by a stale drift candidate. The learned step is deliberately KEPT:
    /// zellij's resize increment is an environment property, not per-episode.
    fn arm_seek(&mut self) {
        self.seek_budget = SEEK_BUDGET;
        self.seek_last_cols = None;
        self.seek_rest = None;
        self.seek_stalled = false;
        self.seek_drift = None;
    }

    /// Alt+c (round 20, collapse-in-place): flip between the template width
    /// and the glyph gutter, arming the width seek. The pane is NEVER
    /// hidden or suppressed — suppress proved structurally hostile in
    /// zellij 0.44 (lossy re-insert; `suppress_pane` marks the tab damaged,
    /// which blocks swap-layout restores; plugin resizes emit no events, so
    /// hidden panes could never heal without a visit). Width is the one
    /// thing an always-visible pane can drive with real feedback: its own
    /// renders. The compare base resets per toggle (round 15: a stale
    /// last_cols "learned" a jump as a bogus step); the learned step is
    /// kept — zellij's resize increment is an environment property.
    /// The snapshot-authoritative collapse flip (issue #5): on CHANGE only,
    /// mirror toggle()'s width-machine reset so the healed instance seeks
    /// its new target; an already-in-sync instance is left byte-untouched
    /// (per-snapshot re-arms would be a perpetual-seek storm, round 11).
    fn heal_collapse(&mut self, collapsed: bool) {
        if self.collapsed != collapsed {
            self.collapsed = collapsed;
            self.peeking = false; // authoritative flip outranks a peek
            self.arm_seek();
        }
    }

    pub fn toggle(&mut self) -> Vec<Effect> {
        self.collapsed = !self.collapsed;
        self.peeking = false; // an explicit toggle outranks a pending peek
        self.arm_seek();
        // Issue #5 durability: record the ABSOLUTE mode we owe the store and
        // emit the persist effect (executor-gated in main.rs — every
        // instance flips + books, exactly one writes). A fresh toggle
        // resets the re-assert budget: it is a new user intent.
        self.pending_collapse = Some(self.collapsed);
        self.collapse_reasserted = false;
        vec![Effect::PersistCollapse {
            collapsed: self.collapsed,
        }]
    }

    /// Bring the seek to REST at `cols`: dormant, silent here, and — crucially
    /// — anchored here. `seek_last_cols` MUST equal the width we came to rest
    /// at, not the mid-flight position of the last emit (Codex PR #27 P2):
    /// gate B measures a later drift as `|own − seek_last_cols|`, so a stale
    /// emit anchor let a FAR external relayout that happened to land within a
    /// learned step of that anchor be misread as our own resize settling —
    /// parking the bar off-target, the F1 corner reborn. Every settle path
    /// (convergence, gate-B accept, floor-accept) routes through here so the
    /// anchor is always the true rest width.
    fn settle_at(&mut self, cols: usize) {
        self.seek_budget = 0;
        self.seek_rest = Some(cols);
        self.seek_last_cols = Some(cols);
        self.seek_drift = None;
    }

    /// Mark this model as awaiting its first snapshot — `main.rs` calls it at
    /// `load()`, right after asking for one. See `awaiting_hydration` (D37).
    pub fn await_hydration(&mut self) {
        self.awaiting_hydration = true;
    }

    /// One width-seek step for OUR OWN pane, driven by render cols (each of
    /// our resizes repaints us with the new width — the feedback loop
    /// proven in rounds 9–10; zellij sends no events for plugin resizes).
    ///
    /// Zellij resizes in ~5%-of-viewport increments (≈7–14 cols), far
    /// coarser than the targets — a naive "shrink while too wide"
    /// overshoots straight through them (27 → 13, round 9). So the step is
    /// LEARNED from each resize's observed effect, acceptance is "within
    /// half a step **and not equally close to the other target**"
    /// (`converged` — LEDGER D21, the overlap that made Alt+c a no-op on a
    /// wide display), and GrowSelf recovers an overshoot. Budget-capped so a
    /// layout that refuses to converge isn't fought forever.
    ///
    /// Issue #4 (F1, C8 drift-on-window-resize backlog): the old design went
    /// permanently silent the instant the budget hit zero, so a window
    /// resize / split that drifted the pane off-target left the bar parked
    /// until the next toggle/peek. The seek now RE-ARMS on drift — but under
    /// three bounds so it can neither thrash nor fight a manual resize:
    ///   1. `seek_rest` — the width we settled at is a no-op; a render there
    ///      never wakes the seek (a converged/floored bar stays quiet).
    ///   2. drift confirmation (`seek_drift`) — we re-arm only when the SAME
    ///      off-target width is observed twice, so an oscillating layout
    ///      (round 20) or a mid-drag flicker never triggers a re-seek.
    ///   3. one-render grace (`seek_stalled`) + the refusal-wall rule — a
    ///      resize that lands a beat late is not double-fired, and a width
    ///      zellij simply refuses to leave (the granularity floor, C8) is
    ///      accepted in place rather than hammered.
    pub fn width_seek(&mut self, own_cols: usize) -> Vec<Effect> {
        // D37: the collapse mode is not known yet. Since D36 the pane is BORN
        // at the width its true mode wants, so a seek on the assumed-expanded
        // default can only move it away from correct — and then visibly move it
        // back when `clave snapshot` returns. Budget is NOT consumed: this is a
        // deferral, not an attempt.
        if self.awaiting_hydration {
            return Vec::new();
        }
        // ONE collapse rule, shared with `widths()` — the profile the rows are
        // drawn at and the width being sought must not drift apart (D16). The
        // OTHER target comes along because acceptance is defined against both
        // (see `converged`), never against ours alone.
        let (target, other) = if self.showing_collapsed() {
            (COLLAPSED_TARGET_COLS, BAR_TARGET_COLS)
        } else {
            (BAR_TARGET_COLS, COLLAPSED_TARGET_COLS)
        };
        // Pre-learning slack of 8 (±4 cols): a bar already within a few
        // cols of the target must be accepted, not nudged into an
        // overshoot dance.
        let step = self.seek_step.max(PRE_LEARNING_STEP) as i64;
        let diff = own_cols as i64 - target as i64;
        let within_band = converged(own_cols, target, other, step);

        // (A) We already settled at exactly this width — a no-op render. Only a
        // DIFFERENT width can wake the seek, so a converged or floored bar
        // stays silent no matter how many times it is re-rendered. Clearing the
        // drift candidate here is load-bearing: a layout that flickers between
        // this rest width and one stable off-target width must NOT re-arm — the
        // rest visit genuinely resets confirmation, so the off-target width's
        // reappearance starts counting from scratch and never "confirms". A
        // real reflow moves cols once to a new stable value and never revisits
        // rest in between, so it still confirms and re-arms as intended (#4).
        if self.seek_rest == Some(own_cols) {
            self.seek_drift = None;
            return Vec::new();
        }

        // (B) Dormant (budget spent). Decide between settling here and
        // re-arming toward the target because an external relayout drifted us.
        if self.seek_budget == 0 {
            // The budget just ran out and cols are OUR OWN doing: either within
            // the band (converged) or within a step of where we last acted —
            // the tail of our final resize / a refusal wall zellij will not
            // leave. Settle in place (round 20: "wherever cols stop changing is
            // accepted"; the SEEK_BUDGET cap means we do not fight a stubborn
            // layout past the cap). NOT a drift — re-arming here would chase our
            // own in-flight resize forever (a step-1 layout under latency would
            // never terminate).
            let ours = self
                .seek_last_cols
                .is_some_and(|last| own_cols.abs_diff(last) <= step as usize);
            if within_band || ours {
                self.settle_at(own_cols);
                return Vec::new();
            }
            // cols jumped FAR from where we last acted while dormant — a window
            // resize / split drifted us (issue #4). Confirm it is STABLE (same
            // width twice) before re-arming, so a thrashing layout or a user
            // mid-drag never provokes a perpetual re-seek.
            if self.seek_drift != Some(own_cols) {
                self.seek_drift = Some(own_cols);
                return Vec::new();
            }
            self.arm_seek(); // stable drift confirmed → re-seek the target
        }

        // (C) Active seek: learn the step / honour the in-flight beat.
        match self.seek_last_cols {
            Some(prev) if prev == own_cols => {
                // cols unchanged since our last resize. Grace EXACTLY one render
                // first: a resize can land a beat late (latency, round 9), and
                // that beat is also when its effect finally teaches us the step.
                // Settling or re-driving before the grace would (a) double-fire
                // an in-flight resize and (b) mistake it for a wall.
                // The one-render grace is calibrated to the ledger's modelled
                // ONE-render resize latency (a subsystem-wide assumption, round
                // 9 / C6). If live zellij ever lands a resize LATER than that
                // under burst, the cost of the misdiagnosis is only a
                // SEEK_BUDGET-bounded re-drive, recovered by the half-step band —
                // a live-validation watch item, not a correctness hole.
                if !self.seek_stalled {
                    self.seek_stalled = true;
                    return Vec::new();
                }
                // Still unchanged after the grace → zellij genuinely refused.
                if diff.abs() <= step {
                    // NEAR the target: the granularity floor (zellij refuses
                    // shrinks below its min pane width, so the bar rests one
                    // step above it, C8 2026-07-18). Accept and stay silent —
                    // this keeps the collapsed floor benign, as the old
                    // in-flight guard did, with no burst of refused resizes.
                    self.settle_at(own_cols);
                    return Vec::new();
                }
                // FAR from the target and still not moving: a relayout CLOBBERED
                // our resize (issue #4). Fall through and re-drive; the budget
                // bounds a clobber we cannot win.
            }
            Some(prev) => {
                let delta = prev.abs_diff(own_cols);
                // Only a plausible single resize step is LEARNED — external
                // jumps (window resizes) poisoned the band (round 17,
                // step=60 accepted a 13-col bar).
                if delta <= MAX_LEARNABLE_STEP {
                    self.seek_step = delta;
                }
            }
            None => {}
        }
        self.seek_stalled = false;

        // (D) Act. Re-read the step in case (C) just learned it.
        let step = self.seek_step.max(PRE_LEARNING_STEP) as i64;
        if converged(own_cols, target, other, step) {
            // Converged: settle and stay done at exactly this width.
            self.settle_at(own_cols);
            return Vec::new();
        }
        // The lattice can be COARSER than the gap between the two targets, and
        // then NO reachable width is acceptable: the step that leaves the
        // overlap zone crosses the target, and the one back crosses it again —
        // an oscillation the budget would spend 16 real resizes on. When ONE OF
        // OUR OWN steps just carried us across the target FROM a width that is
        // no better, we have bracketed it as tightly as zellij's increment
        // allows: settle rather than pace.
        //
        // All three conditions earn their place:
        //   - the crossing itself, or this would settle mid-travel;
        //   - a plausible single step, the same bound the learn arm uses — an
        //     EXTERNAL jump (round 17's 75 → 15) also crosses the target, and
        //     settling on one would park the bar wherever a window resize
        //     dropped it;
        //   - and the width we came FROM being unacceptable too. Otherwise the
        //     honest move is to go BACK: that is the ordinary overshoot (the
        //     pre-learning step of 8 acts on a width a learned step of 11 would
        //     have accepted), and `GrowSelf` recovering it is round 9's lesson,
        //     not something to short-circuit.
        if let Some(prev) = self.seek_last_cols
            && (prev as i64 - target as i64) * diff < 0
            && prev.abs_diff(own_cols) <= MAX_LEARNABLE_STEP
            && !converged(prev, target, other, step)
        {
            self.settle_at(own_cols);
            return Vec::new();
        }
        let action = if diff > 0 {
            Effect::ShrinkSelf
        } else {
            Effect::GrowSelf
        };
        self.seek_budget -= 1;
        self.seek_last_cols = Some(own_cols);
        vec![action]
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
            last_visited: 0,
            tab_id,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: None,
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
            last_visited: 0,
            tab_id,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: None,
        }
    }

    fn snap(seq: u64, agents: Vec<Agent>) -> AgentSnapshot {
        AgentSnapshot {
            collapsed: false,
            seq,
            agents,
            tab_timeline: Default::default(),
        }
    }

    /// Snapshot carrying only a tab timeline (the §6.6 store-timeline).
    fn snap_t(seq: u64, timeline: &[(usize, u64)]) -> AgentSnapshot {
        AgentSnapshot {
            collapsed: false,
            seq,
            agents: vec![],
            tab_timeline: timeline.iter().copied().collect(),
        }
    }

    fn tab(id: usize, pos: usize, name: &str, active: bool) -> TabMeta {
        TabMeta {
            tab_id: id,
            position: pos,
            name: name.into(),
            active,
        }
    }

    fn pane(tab_pos: usize, id: u32, plugin: bool, focused: bool) -> PaneMeta {
        PaneMeta {
            tab_position: tab_pos,
            pane_id: id,
            is_plugin: plugin,
            is_focused: focused,
        }
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
                RowContent::Terminal { name } => name,
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

    // --- tests -------------------------------------------------------------

    #[test]
    fn rows_order_by_last_user_commitment() {
        // §6.6 (store-timeline): one timeline in unix seconds, owned by the
        // STORE and replaced from each snapshot — tab commitments ∨ agent
        // prompts (last_interacted). Focus moves NOTHING.
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(11, 1, "b", true),
            tab(12, 2, "c", false),
        ]);
        // Nothing committed yet → tab-position order, active flag irrelevant.
        assert_eq!(names(&m), vec!["a", "b", "c"]);
        // Commitments arrive via snapshot and order by wall clock…
        m.apply_snapshot(snap_t(1, &[(10, 1000), (11, 2000), (12, 1500)]));
        assert_eq!(names(&m), vec!["b", "c", "a"]);
        // …and focus (beacon) does not reorder.
        m.beacon(10);
        assert_eq!(names(&m)[0], "b");
        // Agent prompts reorder ONLY through the store timeline (the hook
        // stamps tab_timeline via the bind, §6.6 Design B) — an agent's
        // last_interacted alone must NOT sort: render-time joins diverge
        // per instance (round 6).
        let mut s = snap(2, vec![agent("u1", Status::Working, Some(12))]);
        s.agents[0].last_interacted = 9999;
        s.tab_timeline = [(10, 1000), (11, 2000), (12, 1500)].into();
        m.apply_snapshot(s);
        // By KEY from here: tab 12 now hosts an agent, and an agent row does
        // not carry the zellij tab name (lock §7.1).
        assert_eq!(keys(&m)[0], RowKey::Tab(11)); // "b" — li ignored, timeline rules
        // The prompt's stamp arrives IN the timeline → c fronts everywhere.
        m.apply_snapshot(snap_t(3, &[(10, 1000), (11, 2000), (12, 3000)]));
        assert_eq!(keys(&m)[0], RowKey::Tab(12)); // "c"
    }

    #[test]
    fn timeline_is_replaced_from_snapshots_never_merged() {
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
        assert_eq!(
            p.1.content,
            RowContent::Terminal {
                name: "plain".into()
            }
        );
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
        // A later snapshot showing Working clears the local override.
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, Some(10))]));
        assert_eq!(status_at(&m, 0), Some(RowStatus::Working));
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
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Working, None)]));
        assert_eq!(
            m.bind_effects(11),
            vec![Effect::Bind {
                uuid: "u1".into(),
                tab_id: 11
            }]
        );
        // Repeat calls before (or after) the echo: silent.
        assert_eq!(m.bind_effects(11), Vec::<Effect>::new());
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, Some(11))]));
        assert_eq!(m.bind_effects(11), Vec::<Effect>::new());
        // An agent whose pane is NOT in my tab is never mine to bind.
        m.register("u2".into(), 9); // pane 9 unknown to my manifest
        m.apply_snapshot(snap(
            3,
            vec![
                agent("u1", Status::Working, Some(11)),
                agent("u2", Status::Working, None),
            ],
        ));
        assert_eq!(m.bind_effects(11), Vec::<Effect>::new());
    }

    // --- #55 frame coherence & executor election (RC-A / RC-B) -------------

    /// Snapshot carrying agents AND a tab timeline. The #55 tests need both:
    /// a seeded timeline suppresses the birth touch so a test can assert on
    /// binds alone.
    fn snap_full(seq: u64, agents: Vec<Agent>, timeline: &[(usize, u64)]) -> AgentSnapshot {
        AgentSnapshot {
            collapsed: false,
            seq,
            agents,
            tab_timeline: timeline.iter().copied().collect(),
        }
    }

    /// The dossier's reproduction fleet: three tabs `10@0, 11@1, 12@2`, each
    /// with one plugin pane (100/101/102) and one terminal pane (5/6/7). WE
    /// are the bar in tab 11, so our plugin pane is 101.
    fn fleet_of_three(active: usize) -> BarModel {
        let mut m = BarModel::default();
        m.set_own_pane(101);
        m.apply_panes(panes_at(&FLEET_PANES));
        m.apply_tabs(vec![
            tab(10, 0, "a", active == 10),
            tab(11, 1, "b", active == 11),
            tab(12, 2, "c", active == 12),
        ]);
        m
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
            vec![agent("u1", Status::Working, None)],
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
            vec![agent("u1", Status::Working, None)],
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
            vec![agent("u1", Status::Working, Some(11))],
            &[(11, 100)],
        ));
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        // Someone else's bind evicts us (store.rs apply_bind) — tab_id goes
        // back to None at a higher seq. We must fight back.
        m.apply_snapshot(snap_full(
            3,
            vec![agent("u1", Status::Working, None)],
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
            vec![agent("u1", Status::Working, None)],
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
                vec![agent("u1", Status::Working, None)],
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
                vec![agent("u1", Status::Working, None)],
                &[(11, 100)],
            ));
            m.identity_effects();
        }
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        // A confirming snapshot: nothing to send, and the history is kept.
        m.apply_snapshot(snap_full(
            21,
            vec![agent("u1", Status::Working, Some(11))],
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
                vec![agent("u1", Status::Working, None)],
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
                    agent("u1", Status::Working, a),
                    agent("u2", Status::Working, b),
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
                vec![agent("u1", Status::Working, None)],
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
                    vec![agent("u1", Status::Working, Some(11))],
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
            vec![agent("u1", Status::Working, None)],
            &[(11, 100)],
        ));
        assert_eq!(m.identity_effects().len(), 1);
        // The agent's pane moves to another tab: not ours to bind, ledger
        // cleared. When it comes back, the budget is full again.
        m.register("u1".into(), 7);
        m.apply_snapshot(snap_full(
            2,
            vec![agent("u1", Status::Working, None)],
            &[(11, 100)],
        ));
        assert_eq!(m.identity_effects(), Vec::<Effect>::new());
        m.register("u1".into(), 6);
        let mut emitted = 0;
        for seq in 3..=20 {
            m.apply_snapshot(snap_full(
                seq,
                vec![agent("u1", Status::Working, None)],
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
            vec![agent("u1", Status::Working, None)],
            &[(10, 100), (11, 100), (99, 100)],
        ));
        assert_eq!(
            m.identity_effects(),
            vec![
                Effect::Bind {
                    uuid: "u1".into(),
                    tab_id: 11
                },
                // 99 is seeded in the timeline but not in the tab set yet.
                Effect::PruneTabs {
                    stale_ids: vec![99]
                }
            ]
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
            vec![agent("u1", Status::Working, None)],
            &[(11, 100)],
        ));
        m.apply_panes(panes_at(&FLEET_PANES_AFTER_CLOSE));
        assert_eq!(m.bind_effects(11), Vec::<Effect>::new());
    }

    #[test]
    fn nav_is_executor_gated_walks_display_rows_clicks_and_uuid_jumps() {
        let mut m = BarModel::default();
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
            m.click(1),
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
        assert_eq!(m.click(9), Vec::<Effect>::new());
    }

    #[test]
    fn tab_close_reanchors_the_stranded_beacon() {
        // #23 live finding (day one of v0.1.0, 2026-07-21): Ctrl+D closes the
        // focused tab; zellij focuses a survivor and sends ITS bar a TabUpdate,
        // but the replicated beacon (current_tab) still names the CLOSED tab.
        // Executor election keys on current_tab == own live tab (main.rs
        // handle_pipe clave-nav), so a stranded beacon matches NO instance and
        // Alt+↑/↓ goes dead until a mouse click reseeds it. apply_tabs must
        // re-anchor to the post-close active tab — via a DISTINCT, gated
        // effect (birth/organic stay ungated + byte-identical).
        let mut m = BarModel::default();
        // Birth on tab 11 (active): announces once via the PLAIN (ungated)
        // AnnounceVisit — birth's ungated announce is live-validated. c_tab=11.
        let fx = m.apply_tabs(vec![tab(10, 0, "a", false), tab(11, 1, "b", true)]);
        assert!(fx.contains(&Effect::AnnounceVisit { tab_id: 11 }));
        assert_eq!(m.current_tab(), Some(11));
        // Tab 11 (the user's focused tab) closes; zellij focuses the survivor
        // (10) and delivers THIS now-active bar a TabUpdate lacking 11. The
        // stranded re-anchor emits a DISTINCT effect (ReanchorVisit) that
        // run_effects gates to the active instance — toggle bursts deliver the
        // fresh set to ALL instances (doc:371-394), so an ungated announce here
        // would be a beacon war (round-13 EMFILE class).
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
        s.tab_timeline = [(10usize, 100u64), (11, 200)].into();
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
        s.tab_timeline = [(10usize, 100u64), (11, 200)].into();
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
        let mut m = BarModel::default();
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
                fx.iter()
                    .all(|e| !matches!(e, Effect::AnnounceVisit { .. }))
            );
        }
        // Organic switch (Alt+o): the bind's MessagePlugin arms one
        // announce; the next TabUpdate fires it and disarms.
        m.set_organic_pending();
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(fx.contains(&Effect::AnnounceVisit { tab_id: 10 }));
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::AnnounceVisit { .. }))
        );
        // An incoming beacon DISARMS a pending organic announce: the truly
        // active instance already spoke — a stale instance must not answer
        // a leftover flag with poison at its next burst.
        m.set_organic_pending();
        m.beacon(10); // truth arrives (also matches our claim → no-op announce)
        m.beacon(11); // and moves on
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::AnnounceVisit { .. }))
        );
        // A pending organic announce whose claim already MATCHES the beacon
        // stays quiet (nothing to correct).
        m.beacon(10);
        m.set_organic_pending();
        let fx = m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        assert!(
            fx.iter()
                .all(|e| !matches!(e, Effect::AnnounceVisit { .. }))
        );
    }

    /// A model toggled once (expanded → collapsed): seek armed toward the
    /// glyph gutter.
    fn collapsed_model() -> BarModel {
        let mut m = BarModel::default();
        m.toggle();
        m
    }

    /// LEDGER D15 — the separation invariant, derived rather than restated.
    ///
    /// Design-lock §3 gives it as `BAR_TARGET_COLS − COLLAPSED_TARGET_COLS >
    /// MAX_LEARNABLE_STEP (20)`, and at `44 − 30 = 14` that form fails. The
    /// `20` is a restatement of somebody else's derivation, not a physical
    /// bound: `width_seek` accepts when `2 * |cols − target| <= step`, so the
    /// acceptance HALF-band is `step / 2`, and `step` is capped at
    /// `MAX_LEARNABLE_STEP`. The widest half-band is therefore **10**, and the
    /// real requirement is that neither target's value fall inside the other's
    /// band — separation `> 10`. S8 derives exactly this itself and then
    /// asserts `> 20` anyway, because `38 − 4 = 34` cleared it for free.
    ///
    /// Pinned here so a future move of either constant fails loudly at the
    /// bound that is actually load-bearing.
    ///
    /// **What this proves, exactly:** neither TARGET's own value falls inside
    /// the other's band. It says nothing about the WIDTHS between them — a
    /// `w` within half a step of both is accepted for both, which is a
    /// different (and live) property. That one is
    /// `no_width_is_accepted_for_both_targets`, below; the two assertions here
    /// are algebraically the same statement (`2*sep > 20` ⟺ `sep > 10`), so
    /// only one of them is kept.
    #[test]
    fn the_two_targets_are_separated_by_more_than_the_widest_acceptance_band() {
        let half_band = MAX_LEARNABLE_STEP / 2;
        assert_eq!(half_band, 10);
        assert!(
            BAR_TARGET_COLS.abs_diff(COLLAPSED_TARGET_COLS) > half_band,
            "targets {BAR_TARGET_COLS}/{COLLAPSED_TARGET_COLS} are within one \
             acceptance band of each other: a collapse would be accepted as \
             an expand"
        );
    }

    /// LEDGER D21 — the property the test above only LOOKS like it proves, and
    /// the one Alt+c actually depends on: **no width is accepted for both
    /// targets.**
    ///
    /// `width_seek` settles when `2 * |cols − target| <= step`, a band spanning
    /// `step` columns, so the two bands overlap the moment `step >= separation`
    /// (14 at 44/30 — a display area of roughly 280 columns, which the
    /// maintainer runs). Any width in the overlap is "converged" for BOTH
    /// targets, and since `toggle()` deliberately keeps the learned step, Alt+c
    /// then emits ZERO resizes: the pane does not move. On `main` the targets
    /// were 26 apart and no learnable step could reach that, so this held by
    /// luck of the constants; 44/30 lost it silently.
    ///
    /// Driven through `width_seek` rather than re-derived here: a test that
    /// restates the predicate can drift from the predicate. A model with a
    /// fresh budget and no `seek_last_cols` reaches gate (D) directly, so an
    /// empty effect list IS "this width is accepted for this target".
    ///
    /// **This is the test that fails if someone widens the band again.**
    #[test]
    fn no_width_is_accepted_for_both_targets() {
        let quiet_at = |collapsed: bool, step: usize, cols: usize| {
            let mut m = BarModel {
                collapsed,
                seek_step: step,
                ..BarModel::default()
            };
            m.width_seek(cols).is_empty()
        };
        for step in 0..=MAX_LEARNABLE_STEP {
            for cols in 0..=200 {
                assert!(
                    !(quiet_at(false, step, cols) && quiet_at(true, step, cols)),
                    "at a learned step of {step}, {cols} cols is converged for \
                     BOTH {BAR_TARGET_COLS} and {COLLAPSED_TARGET_COLS}: Alt+c \
                     emits no resize and the pane does not move"
                );
            }
        }
    }

    #[test]
    fn a_newborn_model_seeks_the_template_width() {
        // The generated layouts size the bar pane in PERCENT — fixed sizes
        // make zellij refuse every resize (CantResizeFixedPanes, the
        // Alt+c-dead live finding, c8-cold-start 2026-07-18) — so a newborn
        // bar's cols depend on the window. The seek must be armed at birth
        // to converge on the exact template width from either side.
        // Both start widths are outside the pre-learning ±4 band around
        // BAR_TARGET_COLS (54 since D19; 44 before) — and both still are, which
        // is why they did not move here. Re-checked rather than assumed: at 54,
        // |60 − 54| = 6 and |18 − 54| = 36, both over the ±4 slack. A start
        // that silently fell INSIDE the band would be the "the number moved,
        // the test's meaning did not" trap (#63).
        let mut m = BarModel::default();
        assert_eq!(m.width_seek(60), vec![Effect::ShrinkSelf]);
        let mut m = BarModel::default();
        assert_eq!(m.width_seek(18), vec![Effect::GrowSelf]);
    }

    #[test]
    fn seek_collapses_to_the_gutter_despite_coarse_steps() {
        // Round 20 (collapse-in-place): Alt+c drives OWN width between the
        // template (54, LEDGER D2 then D19) and the collapsed profile (30,
        // D17) — the pane is never suppressed. Zellij resizes in
        // ~5%-of-viewport steps (7–14 cols). Since D19 those steps are FINER
        // than the 24-column separation between the two targets rather than
        // coarser than it, which is what retires D26's overlap reservations;
        // the step is still LEARNED from each resize's observed effect and
        // acceptance is within half a step (round-9 lesson: naive loops
        // overshoot straight through).
        let mut m = BarModel::default();
        // Already AT the expanded target: nothing to do.
        assert_eq!(m.width_seek(BAR_TARGET_COLS), Vec::<Effect>::new());
        let mut m = collapsed_model();
        // 72, not 30: 30 IS the collapsed target now, so the old start width
        // would have converged in zero steps and asserted nothing (#63 —
        // `30` was both the target and an arbitrary start width).
        let mut cols = 72i64;
        let mut acted = 0;
        loop {
            match m.width_seek(cols as usize).as_slice() {
                [Effect::ShrinkSelf] => cols -= 7,
                [Effect::GrowSelf] => cols += 7,
                [] => break,
                other => panic!("unexpected seek effects: {other:?}"),
            }
            acted += 1;
            assert!(acted < 20, "did not converge");
        }
        // Within half a learned step of the collapsed target, and dormant: the
        // FIRST render at a wildly different width only OBSERVES it (a drift
        // candidate — it could be a transient mid-relayout value). It does not
        // "stay done" — a SECOND render at 140 confirms the drift and re-arms
        // the seek (issue #4); `idle_seek_re_arms_when_a_relayout_drifts_it_off_target`
        // is where that half is proven.
        assert!((cols - 30).abs() <= 4, "ended at {cols} cols");
        assert_eq!(m.width_seek(140), Vec::<Effect>::new());
    }

    #[test]
    fn seek_expands_back_to_template_width() {
        let mut m = collapsed_model();
        m.toggle(); // expanded again → seek re-armed toward 44
        let mut cols = 5i64;
        let mut acted = 0;
        loop {
            match m.width_seek(cols as usize).as_slice() {
                [Effect::ShrinkSelf] => cols -= 9,
                [Effect::GrowSelf] => cols += 9,
                [] => break,
                other => panic!("unexpected seek effects: {other:?}"),
            }
            acted += 1;
            assert!(acted < 20, "did not converge");
        }
        // Simulated step 9 (not 7): the ladder from 5 must land on a width
        // that DISTINGUISHES the expanded target from the collapsed one, or
        // the assertion passes for the wrong target. 5+9k gives 50, and
        // |50 − 54| = 4 is inside the half-band (2·4 <= 9), where the collapsed
        // target 30 would have stopped at 32 — 18 columns away from where this
        // rests, so a target mix-up cannot pass here.
        assert!(
            (cols - BAR_TARGET_COLS as i64).abs() <= 4,
            "ended at {cols} cols"
        );
    }

    #[test]
    fn seek_waits_for_inflight_resizes_and_zellijs_floor() {
        // In-flight guard, issue-#4 edition. cols unchanged since our resize
        // means the effect either lands a beat late (round-9 double-fire risk —
        // WAIT) or was CLOBBERED by a relayout (must recover, not hang). The
        // old guard could not tell the two apart, so it WAITED FOREVER — the
        // very stall that left a drifted bar parked (F1). Now: grace exactly
        // one render, then re-drive; the budget bounds a clobber we cannot win.
        // FOOTGUNS names this test by name: `30` was BOTH the old collapsed
        // target's arbitrary start width and, now, the target itself. The
        // widths below are re-derived from the new targets, not substituted:
        // start 60 (far above the collapsed target 30), land 44 (delta 16 —
        // a LEARNABLE step, ≤ MAX_LEARNABLE_STEP) leaving |44 − 30| = 14
        // inside that learned step, which is what makes the last stanza a
        // near-target wall rather than a clobber.
        let mut m = collapsed_model(); // target 30
        assert_eq!(m.width_seek(60), vec![Effect::ShrinkSelf]);
        // FAR off target and still 60: one render of grace, no double-fire.
        assert_eq!(m.width_seek(60), Vec::<Effect>::new());
        // Still 60 after the grace → the resize was clobbered; re-drive (#4).
        assert_eq!(m.width_seek(60), vec![Effect::ShrinkSelf]);
        // Landed (60 → 44): learned step 16, keep shrinking toward 30.
        assert_eq!(m.width_seek(44), vec![Effect::ShrinkSelf]);
        // Floor: |44 − 30| = 14 is within the learned step of 16, so zellij's
        // refusal to shrink further is a NEAR-target wall — accepted in place
        // (round 20: "wherever cols stop changing is accepted") and silent
        // forever, with no burst of refused resizes. This keeps the collapsed
        // floor benign.
        for _ in 0..10 {
            assert_eq!(m.width_seek(44), Vec::<Effect>::new());
        }
    }

    #[test]
    fn idle_seek_re_arms_when_a_relayout_drifts_it_off_target() {
        // Issue #4 (F1): the old seek went permanently silent at budget 0, so a
        // window resize / split that drifted the bar off-target left it parked
        // until the next toggle/peek. It must now re-seek — but only after the
        // drift is CONFIRMED stable (same width twice), so a mid-drag flicker
        // or a thrashing layout never provokes a perpetual re-seek.
        let mut m = collapsed_model(); // target 30
        // Converge to the collapsed target and go dormant. 60 → 44 learns a
        // step of 16; 44 → 28 lands 2 columns under target, inside the band.
        assert_eq!(m.width_seek(60), vec![Effect::ShrinkSelf]);
        assert_eq!(m.width_seek(44), vec![Effect::ShrinkSelf]);
        assert_eq!(m.width_seek(28), Vec::<Effect>::new()); // within band → done
        assert_eq!(m.seek_budget, 0, "seek should be dormant after converging");
        // A relayout slams the pane wide (6 → 140). The FIRST render only
        // observes it (could be a transient mid-relayout value); no action yet.
        assert_eq!(m.width_seek(140), Vec::<Effect>::new());
        // The SAME off-target width a second time is a stable drift → re-arm
        // and seek back toward the gutter.
        assert_eq!(m.width_seek(140), vec![Effect::ShrinkSelf]);
    }

    #[test]
    fn drift_is_measured_from_the_rest_width_not_a_stale_emit_anchor() {
        // Codex PR #27 P2: at convergence the seek goes dormant but the anchor
        // used by gate B's `ours = |own − last| ≤ step` test must equal the
        // width we CAME TO REST at, not the mid-flight position of the last
        // emit. Otherwise a later EXTERNAL relayout that lands within a step of
        // the STALE emit anchor (but far from rest) is misread as self-inflicted
        // and accepted as rest — the F1 off-target park, reborn in a corner.
        let mut m = collapsed_model(); // target 30
        // Converge in coarse steps so the FINAL landing (→ 32) is many cols
        // from the previous emit position (52): 72 → 52 → 32, learning a step
        // of 20 (MAX_LEARNABLE_STEP, the widest anchor window there is).
        assert_eq!(m.width_seek(72), vec![Effect::ShrinkSelf]); // emit @72
        assert_eq!(m.width_seek(52), vec![Effect::ShrinkSelf]); // learn 20, emit @52
        assert_eq!(m.width_seek(32), Vec::<Effect>::new()); // within band → REST @32
        assert_eq!(m.seek_budget, 0, "must be dormant after converging");
        // A stable external relayout parks the bar at 62 — FAR from the rest
        // width 32 (|62−32| = 30 > 20), but within a learned step of the stale
        // emit anchor 52 (|62−52| = 10 ≤ 20). A genuine drift: confirm it
        // (twice) and re-seek the collapsed target, NOT settle at 62.
        assert_eq!(m.width_seek(62), Vec::<Effect>::new()); // observe
        assert_eq!(
            m.width_seek(62),
            vec![Effect::ShrinkSelf],
            "a far-from-rest drift must re-arm, not be classified as self-inflicted"
        );
    }

    #[test]
    fn idle_seek_ignores_an_oscillating_layout_and_a_resting_width() {
        // The two bounds that keep the drift re-arm from fighting forever.
        let mut m = collapsed_model(); // target 30
        assert_eq!(m.width_seek(60), vec![Effect::ShrinkSelf]);
        // 60 → 32 is a 28-column delta: NOT learnable (> MAX_LEARNABLE_STEP),
        // so the pre-learning ±4 slack band accepts |32 − 30| = 2 and settles.
        assert_eq!(m.width_seek(32), Vec::<Effect>::new()); // settled at ~target
        // (1) A render at the exact settled width is a no-op, however often it
        // repeats — a converged bar never wakes itself.
        for _ in 0..8 {
            assert_eq!(m.width_seek(32), Vec::<Effect>::new());
        }
        // (2) A layout that never holds still (alternating off-target widths)
        // is never CONFIRMED, so it never re-arms — no perpetual re-seek
        // (round 20). Both widths are far from the collapsed target.
        for cols in [200, 100, 200, 100, 200, 100] {
            assert_eq!(
                m.width_seek(cols),
                Vec::<Effect>::new(),
                "an unstable width must not re-arm the seek"
            );
        }
    }

    #[test]
    fn idle_seek_oscillating_between_rest_and_one_off_target_never_re_arms() {
        // A layout that flickers between the SETTLED width and a single stable
        // off-target width must also never re-arm: a rest visit has to genuinely
        // reset the drift candidate, or the off-target width's every-other-render
        // reappearance would spuriously "confirm" (round 20: never fight a
        // layout that will not hold still). A real reflow, by contrast, moves
        // cols ONCE to a new stable value and never revisits rest in between.
        let mut m = collapsed_model(); // target 30
        assert_eq!(m.width_seek(60), vec![Effect::ShrinkSelf]);
        assert_eq!(m.width_seek(32), Vec::<Effect>::new()); // settled at ~target
        for cols in [200, 32, 200, 32, 200, 32] {
            assert_eq!(
                m.width_seek(cols),
                Vec::<Effect>::new(),
                "a rest↔off-target flicker must not re-arm the seek"
            );
        }
    }

    #[test]
    fn seek_grows_back_from_an_overshoot() {
        // The round-9 live defect, seek edition: an overshoot past the
        // target is recovered by growing, and the half-step band accepts.
        let mut m = collapsed_model();
        m.toggle(); // expanded → target 54 (D19)
        assert_eq!(m.width_seek(30), vec![Effect::GrowSelf]);
        // Landed (+20 → 50): delta 20 is exactly MAX_LEARNABLE_STEP, so it IS
        // learned, and |50 − 54| = 4 clears the half-step band (2·4 <= 20).
        // Also unambiguous under D26's second clause — 50 is 4 from the
        // expanded target and 20 from the collapsed one. Accept and retire.
        assert_eq!(m.width_seek(50), Vec::<Effect>::new());
        assert_eq!(m.width_seek(30), Vec::<Effect>::new()); // retired
    }

    #[test]
    fn seek_never_learns_an_external_jump_as_the_step_size() {
        // Round-17 lesson kept: a window resize can slam cols by far more
        // than one resize step; learning that delta poisons the acceptance
        // band (step=60 accepted a 13-col bar as "close enough" to 26).
        let mut m = collapsed_model();
        m.toggle(); // expanded → target 44
        assert_eq!(m.width_seek(75), vec![Effect::ShrinkSelf]);
        // External jump 75 → 15 (delta 60): recover, but don't learn 60.
        assert_eq!(m.width_seek(15), vec![Effect::GrowSelf]);
        // 60 is 16 off-template: outside the unlearned ±4 band, but well
        // inside the ±30 band a learned step of 60 would have opened — so a
        // poisoned step fake-accepts it and this assertion catches it. (The
        // old `40` was chosen against target 30 and is only 4 off 44, i.e.
        // legitimately accepted now — the numbers are re-derived, not moved.)
        assert_eq!(m.width_seek(60), vec![Effect::ShrinkSelf]);
    }

    #[test]
    fn seek_budget_caps_a_layout_that_never_converges() {
        let mut m = collapsed_model();
        // A pathological layout that thrashes (cols change but never reach
        // the target) must not be fought forever — each step is a real
        // zellij layout action.
        let steps = (0..64)
            .map(|i| m.width_seek(if i % 2 == 0 { 100 } else { 86 }))
            .take_while(|fx| !fx.is_empty())
            .count();
        assert!(steps <= 16, "unbounded seek: {steps} steps");
    }

    #[test]
    fn peek_expands_a_collapsed_bar_and_expiry_sinks_it() {
        // Peek-on-nav: while collapsed, any nav (arriving as the replicated
        // clave-visited pipe) briefly expands the bar; ~1s after the last
        // nav it sinks back to the gutter.
        let mut m = collapsed_model();
        assert_eq!(m.width_seek(30), Vec::<Effect>::new()); // settled, collapsed
        assert!(m.visited(7), "collapsed bar must arm a peek");
        assert_eq!(m.current_tab(), Some(7)); // still a beacon
        // Peek re-armed the seek toward the TEMPLATE despite collapsed: the
        // 14-column separation (D15/D17) exceeds the ±4 pre-learning band, so
        // the same width that was AT rest collapsed is now a grow.
        assert_eq!(m.width_seek(30), vec![Effect::GrowSelf]);
        // A second nav during the peek re-arms (main.rs counts its timers).
        assert!(m.visited(8));
        // Expiry: sink back toward the collapsed target.
        assert!(m.peek_expired());
        assert_eq!(m.width_seek(44), vec![Effect::ShrinkSelf]);
    }

    #[test]
    fn expanded_bars_ignore_peeks() {
        let mut m = BarModel::default();
        assert!(!m.visited(7), "expanded bar must not arm a peek");
        assert_eq!(m.current_tab(), Some(7)); // beacon still lands
        // NOT "no seek was armed": a default model IS birth-armed, which
        // `a_newborn_model_seeks_the_template_width` proves. What this shows
        // is that the beacon left the collapse state alone — the bar is at the
        // EXPANDED target, so an armed seek has nothing to say. Kept (rather
        // than deleted) because it does fail if `visited` ever corrupted
        // `collapsed`: the target would drop to 30 and 44 would emit a shrink.
        assert_eq!(m.width_seek(BAR_TARGET_COLS), Vec::<Effect>::new());
    }

    #[test]
    fn toggle_cancels_a_peek_and_a_late_expiry_is_a_noop() {
        let mut m = collapsed_model();
        assert_eq!(m.width_seek(30), Vec::<Effect>::new());
        assert!(m.visited(7));
        // Alt+c mid-peek: now genuinely expanded; the peek flag must not
        // survive to fight the user's explicit toggle.
        m.toggle();
        assert!(!m.peek_expired(), "late timer after a toggle is a no-op");
        // Seek heads for the template (expanded), unpoisoned by the peek.
        assert_eq!(m.width_seek(13), vec![Effect::GrowSelf]);
    }

    /// Issue #5 path (a), birth-while-collapsed: a bar born after the toggle
    /// (or reborn by a reload — path (b), identical state: fresh default)
    /// missed the broadcast forever. Hydration must come from the snapshot
    /// it fetches at startup, and must arm the seek toward the gutter.
    #[test]
    fn snapshot_hydrates_a_newborn_into_collapse() {
        let mut m = BarModel::default();
        let mut s = snap(1, vec![]);
        s.collapsed = true;
        m.apply_snapshot(s);
        assert!(m.collapsed, "snapshot-carried flag did not hydrate");
        // Born at template width (44) among collapsed bars → must shrink,
        // exactly as if it had heard the toggle itself.
        assert_eq!(m.width_seek(44), vec![Effect::ShrinkSelf]);
    }

    /// Issue #5 path (c), missed pipe: an instance that missed the toggle
    /// broadcast heals from the next (seq-newer) snapshot push; an instance
    /// already in the right state must NOT have its seek re-armed by that
    /// same push (a per-snapshot re-arm would be a perpetual-seek storm,
    /// round 11).
    #[test]
    fn snapshot_heals_a_desynced_instance_and_leaves_synced_ones_alone() {
        // Desynced: expanded while the store says collapsed.
        let mut missed = BarModel::default();
        missed.apply_snapshot(snap(1, vec![])); // hydrated expanded, seq 1
        // AT the expanded target, so "within band" needs no slack to be true.
        let converged = missed.width_seek(BAR_TARGET_COLS); // within band → done
        assert_eq!(converged, Vec::<Effect>::new());
        let mut heal = snap(2, vec![]);
        heal.collapsed = true;
        missed.apply_snapshot(heal);
        assert!(missed.collapsed, "missed-pipe instance did not heal");
        assert_eq!(
            missed.width_seek(BAR_TARGET_COLS),
            vec![Effect::ShrinkSelf],
            "healing must re-arm the seek toward the collapsed target"
        );

        // Synced: toggled locally (broadcast heard), then the store's own
        // collapse push arrives carrying the SAME flag — state untouched.
        let mut synced = BarModel::default();
        synced.apply_snapshot(snap(1, vec![]));
        synced.toggle(); // collapsed, seek armed
        // Drain the seek to quiescence at the collapsed target.
        assert_eq!(synced.width_seek(30), Vec::<Effect>::new());
        let budget_before = synced.seek_budget;
        let mut same = snap(2, vec![]);
        same.collapsed = true;
        let fx = synced.apply_snapshot(same);
        assert!(synced.collapsed);
        assert_eq!(
            synced.seek_budget, budget_before,
            "an unchanged snapshot flag must not re-arm the seek"
        );
        // The confirming push also settles the pending ledger silently —
        // no re-assert traffic when writer and store agree.
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::PersistCollapse { .. })),
            "a confirming snapshot must not trigger a re-assert"
        );
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
    /// push then contradicts the user. The pending ledger keeps USER truth
    /// and re-asserts exactly once; a second contradiction yields to the
    /// store (wrong-but-consistent beats a two-instance ping-pong, rd 11).
    #[test]
    fn out_of_order_write_is_reasserted_once_then_store_wins() {
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
        // The re-assert was lost too (pathological): a second contradicting
        // push spends the retry and the store becomes authoritative.
        let mut bad2 = snap(3, vec![]);
        bad2.collapsed = true;
        let fx = m.apply_snapshot(bad2);
        assert!(m.collapsed, "retry spent: consistency over intent");
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::PersistCollapse { .. })),
            "no further re-asserts — the ledger is closed"
        );
        // Ledger clean again: a later store flip heals normally.
        let mut heal = snap(4, vec![]);
        heal.collapsed = false;
        m.apply_snapshot(heal);
        assert!(!m.collapsed);
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
        s.tab_timeline = [(10, 100), (11, 900), (12, 500)].into();
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
        // S7 has not landed: a blank cell, never an invented level.
        assert_eq!(battery, None);
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
    fn a_tab_with_no_agent_is_a_terminal_row_carrying_the_zellij_name() {
        // Lock §7.1: the zellij tab name is used ONLY for a terminal tab.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "Tab #16", false)]);
        assert_eq!(
            content_at(&m, 0),
            RowContent::Terminal {
                name: "Tab #16".into()
            }
        );
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
        // seek from over-running mid-animation. Same rule width_seek picks its
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
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: Default::default(),
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
    fn dormant_rows_sort_into_the_unified_recency_order() {
        // One list, claude.ai-style: live tabs keyed by tab_timeline, dormant
        // rows keyed by last_interacted, merged desc.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut old = agent("u-old", Status::Idle, None);
        old.last_interacted = 100;
        let mut new = agent("u-new", Status::Idle, None);
        new.last_interacted = 900;
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![old, new],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        assert_eq!(
            keys(&m),
            vec![
                RowKey::Dormant("u-new".into()), // 900
                RowKey::Tab(1),                  // 500
                RowKey::Dormant("u-old".into()), // 100
            ]
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
            collapsed: false,
            seq: 1,
            agents: vec![agent("u1", Status::Working, Some(7))], // bound → live
            tab_timeline: Default::default(),
        });
        assert!(!keys(&m).contains(&RowKey::Dormant("u1".into())));
        // Bind gone (fresh session) but the pane join exists → still not dormant.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(7, 0, "agent-tab", true)]);
        m.register("u2".into(), 42);
        m.apply_panes(vec![pane(0, 42, false, true)]);
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![agent("u2", Status::Working, None)],
            tab_timeline: Default::default(),
        });
        assert!(!keys(&m).contains(&RowKey::Dormant("u2".into())));
    }

    #[test]
    fn nav_onto_dormant_row_arms_dwell_not_open() {
        // §6.6 C8: stepping onto a dormant row moves a virtual cursor and arms
        // a 0.4s dwell — it must NOT switch tabs, announce, or open.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999; // dormant row sorts FIRST; live row is line 1
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        let fx = m.nav("{\"dir\":\"next\"}", Some(1)); // from live row 1, wrap → row 0 (dormant)
        assert_eq!(fx, vec![Effect::ArmDwell { r#gen: 1 }]);
        // Cursor moved; a second step continues FROM the cursor, back to live.
        let fx = m.nav("{\"dir\":\"next\"}", Some(1));
        assert!(fx.contains(&Effect::SwitchTab { position: 0 }));
    }

    #[test]
    fn dwell_expiry_opens_only_if_cursor_still_there() {
        // Walk-through safety: the gen stamps each landing; a stale gen (the
        // cursor moved on) must be a no-op — this is what makes walking the
        // unified list safe.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999;
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        let fx = m.nav("{\"dir\":\"next\"}", Some(1));
        let Effect::ArmDwell { r#gen } = fx[0] else {
            panic!()
        };
        // Cursor moved away before expiry → stale gen, no open.
        m.nav("{\"dir\":\"next\"}", Some(1));
        assert!(m.dwell_expired(r#gen).is_empty());
        // Land again and let it expire in place → exactly one open, marked ↻.
        let fx = m.nav("{\"dir\":\"prev\"}", Some(1)); // back to dormant row 0
        let Effect::ArmDwell { r#gen } = fx[0] else {
            panic!()
        };
        assert_eq!(
            m.dwell_expired(r#gen),
            vec![Effect::OpenAgent { uuid: "u-d".into() }]
        );
        // In flight now: a repeat expiry (or landing) must not double-fire.
        assert!(m.dwell_expired(r#gen).is_empty());
    }

    #[test]
    fn explicit_picks_open_immediately() {
        // Click and Alt+N skip the dwell — explicit intent is unambiguous.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999;
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        assert_eq!(
            m.click(0), // dormant row is line 0
            vec![Effect::OpenAgent { uuid: "u-d".into() }]
        );
        // Alt+1 (row payload) on a dormant row — new model, fresh state:
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999;
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        assert_eq!(
            m.nav("{\"row\":1}", Some(1)),
            vec![Effect::OpenAgent { uuid: "u-d".into() }]
        );
    }

    #[test]
    fn dormant_landing_peeks_a_collapsed_bar() {
        // §6.6: walking dormant rows must keep a collapsed bar peeked, same as
        // live-row nav (whose peek rides the visited pipe — dormant landings
        // have no pipe, so the model returns ArmPeek explicitly).
        let mut m = BarModel::default();
        m.toggle(); // collapsed
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999;
        m.apply_snapshot(AgentSnapshot {
            // Issue #5: snapshots now carry the store's collapse mode; after
            // the toggle above, the real flow's store says collapsed too —
            // `false` here would (correctly!) heal the bar back to expanded.
            collapsed: true,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        let fx = m.nav("{\"dir\":\"next\"}", Some(1));
        assert!(fx.contains(&Effect::ArmPeek));
        assert!(fx.iter().any(|e| matches!(e, Effect::ArmDwell { .. })));
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
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: Default::default(),
        });
        assert_eq!(status_at(&m, 0), Some(RowStatus::Stale));
        assert!(m.opening.is_empty(), "stale snapshot clears in-flight");
        // In-flight (no stale): ↻.
        let mut m = BarModel::default();
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![agent("u2", Status::Idle, None)],
            tab_timeline: Default::default(),
        });
        m.opening.insert("u2".into());
        assert_eq!(status_at(&m, 0), Some(RowStatus::Opening));
    }

    #[test]
    fn virtual_cursor_highlight_follows_the_dormant_walk() {
        // §6.6 C8: nav onto a dormant row moves the SELECTION there — that
        // row reads active and the previously-focused live tab drops its
        // highlight (else the stale live highlight lingers and misleads).
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999; // dormant sorts FIRST; live is line 1
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        // (a) walk onto the dormant row → it is active, the live tab is not.
        m.nav("{\"dir\":\"next\"}", Some(1)); // live row 1 → wrap → dormant row 0
        assert_eq!(keys(&m)[0], RowKey::Dormant("u-d".into()));
        assert_eq!(
            selected(&m),
            vec![true, false],
            "dormant selection holds it"
        );
        // (b) a live landing clears the cursor → the tab highlights again.
        m.nav("{\"dir\":\"next\"}", Some(1)); // dormant → wrap → live tab
        assert_eq!(
            selected(&m),
            vec![false, true],
            "focused tab reclaims the highlight"
        );
    }

    #[test]
    fn stale_cursor_on_a_row_gone_live_self_heals_to_the_tab() {
        // Review minor #7: a dwell-opened row goes LIVE while the cursor still
        // names its uuid. The dormant-key lookup misses, so the highlight
        // falls back to the focused tab — no explicit cursor clear needed.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999;
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        m.nav("{\"dir\":\"next\"}", Some(1)); // cursor now on the dormant row
        assert!(selected(&m)[0]); // dormant selected
        // The row goes LIVE: u-d binds to a new tab (2). Cursor still names
        // "u-d" but it no longer renders dormant.
        m.apply_tabs(vec![tab(1, 0, "live", false), tab(2, 1, "u-d", true)]);
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 2,
            agents: vec![agent("u-d", Status::Working, Some(2))],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64), (2usize, 600u64)]),
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
    fn classify_timer_splits_by_elapsed_and_reclassifies_late_dwells() {
        // Normal split both ways.
        assert_eq!(classify_timer(DWELL_SECS, 1, 0), TimerKind::Dwell);
        assert_eq!(classify_timer(PEEK_SINK_SECS, 0, 1), TimerKind::Peek);
        // HARDENING: a dwell delayed past the cutoff with NO peeks pending is
        // still a dwell — else its gen never pops and the FIFO latches
        // off-by-one for the life of the instance.
        assert_eq!(classify_timer(0.7, 2, 0), TimerKind::Dwell);
        // A late expiry WITH peeks pending stays Peek — a 0.9s sleep never
        // reports < 0.9, so this can't be a mis-delayed dwell.
        assert_eq!(classify_timer(0.7, 1, 1), TimerKind::Peek);
        // Nothing pending, long elapsed → Peek (default; the pop_front guard
        // no-ops harmlessly).
        assert_eq!(classify_timer(0.7, 0, 0), TimerKind::Peek);
    }

    #[test]
    fn native_switch_beacon_clears_a_pinned_dormant_cursor() {
        // Edge (Fix-1 heal): a dwell-open FAILS — the row stays dormant with
        // the cursor pinned to it. A NATIVE tab switch (Alt+o / zellij binds)
        // carries no clave-nav, only a visited-pipe beacon. That beacon must
        // resolve the selection back to the focused tab, else the ✗ row keeps
        // the highlight and the real active tab stays suppressed.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "live", false), tab(2, 1, "other", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999; // dormant sorts FIRST
        m.apply_snapshot(AgentSnapshot {
            collapsed: false,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64), (2usize, 400u64)]),
        });
        m.beacon(1);
        let fx = m.nav("{\"dir\":\"prev\"}", Some(1)); // land on the dormant row
        let Effect::ArmDwell { r#gen } = fx[0] else {
            panic!()
        };
        assert!(selected(&m)[0], "dormant selected before the native switch");
        // Native switch to tab 2 arrives as a visited-pipe beacon (no nav).
        m.beacon(2);
        let rows = m.rows();
        assert!(!rows[0].1.selected, "dormant row releases the highlight");
        let active: Vec<_> = rows
            .iter()
            .filter(|(_, r)| r.selected)
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(active, vec![RowKey::Tab(2)], "focused tab reclaims it");
        // The now-orphaned dwell must no-op (cursor cleared).
        assert!(m.dwell_expired(r#gen).is_empty());
    }

    #[test]
    fn late_dwell_with_sibling_peek_pending_defers_never_drops_the_open() {
        // §6.6 collapsed-walk heal (model.rs ~86–107): a dormant landing on a
        // collapsed bar arms BOTH a 0.4s dwell (gen) and a 0.9s peek from ONE
        // landing. If the dwell's server-measured elapsed creeps past the
        // 0.65 cutoff while its sibling peek is still pending, the FIRST
        // expiry MUST classify as Peek (a 0.9s sleep can never report < 0.9,
        // so late_dwell reclassification is DISABLED while peeks are pending)
        // — the owed dwell is DEFERRED, not consumed. The peek's own later
        // expiry, now with no peeks pending, reclassifies as the late dwell
        // and the queue rebalances. Nothing tested the COMBINED path before;
        // this pins that the delayed open is deferred, never lost.
        let mut m = collapsed_model();
        m.apply_tabs(vec![tab(1, 0, "live", true)]);
        let mut a = agent("u-d", Status::Idle, None);
        a.last_interacted = 999; // dormant sorts FIRST; live row is line 1
        m.apply_snapshot(AgentSnapshot {
            // Issue #5: matches collapsed_model()'s store-side truth — a
            // `false` flag would (correctly) heal the bar expanded and void
            // the collapsed-landing peek this test pins.
            collapsed: true,
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        // (1) Walk onto the dormant row: one landing arms dwell + peek.
        let fx = m.nav("{\"dir\":\"next\"}", Some(1)); // live row 1 → wrap → dormant row 0
        let Effect::ArmDwell { r#gen } = *fx
            .iter()
            .find(|e| matches!(e, Effect::ArmDwell { .. }))
            .expect("dormant landing arms a dwell")
        else {
            unreachable!()
        };
        assert!(
            fx.contains(&Effect::ArmPeek),
            "collapsed landing arms a peek"
        );

        // (2) The dwell's expiry arrives LATE (elapsed past the 0.65 cutoff)
        // while the sibling peek is still pending — it MUST read as Peek, so
        // the dwell is deferred, not spent on this expiry.
        assert_eq!(
            classify_timer(0.7, 1, 1),
            TimerKind::Peek,
            "late dwell defers to the pending peek — reclassification is off"
        );

        // (3) main.rs decrements the peek bookkeeping for that expiry; the
        // SECOND expiry (the peek's own ~0.9s pop) now has no peeks pending
        // but a dwell still owed → reclassify as the late Dwell.
        assert_eq!(
            classify_timer(PEEK_SINK_SECS, 1, 0),
            TimerKind::Dwell,
            "the peek expiry rebalances into the owed late dwell"
        );

        // (4) That late-dwell classification drives dwell_expired(gen): the
        // cursor still sits on the same dormant landing, so the open the
        // first expiry deferred is now delivered — deferred, never dropped.
        assert_eq!(
            m.dwell_expired(r#gen),
            vec![Effect::OpenAgent { uuid: "u-d".into() }],
            "the deferred open self-heals through the sibling peek expiry"
        );
    }

    // === Convergence harness (issue #10 item 2) ============================
    //
    // width_seek is the LAST survivor of the C6 repair saga (rounds 9–20):
    // every event-driven repair failed because zellij emits NO events for a
    // plugin's OWN resize (main.rs ~478; C6 round 18 FINAL CONSTRAINT) — the
    // sole feedback is the plugin's own re-render with the new cols. This
    // harness makes that render loop deterministic: a stand-in zellij that
    // answers ShrinkSelf/GrowSelf by moving cols a coarse step and re-presents
    // them, so the convergence contract the ledger only ever asserted
    // anecdotally on live sessions gets pinned as an executable proof.

    /// A test-side stand-in for zellij's tiled resize engine. Honest to the
    /// ledger in three ways that each broke a real round:
    ///  - `step` is COARSE (≈5% of viewport, 7–14 cols live; C6 round 9) — far
    ///    bigger than the 4/30 targets, so a naive "shrink while too wide"
    ///    overshoots straight through (27→13, round 9). width_seek must LEARN
    ///    the step and accept within half of it.
    ///  - `floor`: zellij REFUSES a resize below its min pane width
    ///    (CantResizeFixedPanes / the granularity floor, C8 2026-07-18 — the
    ///    bar rests one step above the true min). A refusal leaves cols
    ///    UNCHANGED; that must not livelock (round 20: the in-flight guard
    ///    makes the floor benign).
    ///  - `latency`: a resize can land one render late, so the model re-sees
    ///    the OLD cols once (prev == own_cols) before the effect shows — the
    ///    double-fire in-flight guard (round 9) depends on that very beat.
    struct SimZellij {
        cols: usize,
        step: usize,
        floor: usize,
        ceil: usize,
        latency: bool,
        pending: Option<Effect>,
    }

    impl SimZellij {
        fn new(cols: usize, step: usize, floor: usize, ceil: usize, latency: bool) -> Self {
            assert!(floor <= ceil, "sim floor {floor} above ceil {ceil}");
            Self {
                cols: cols.clamp(floor, ceil),
                step: step.max(1), // a 0-col step would never move cols → livelock the SIM
                floor,
                ceil,
                latency,
                pending: None,
            }
        }

        /// Apply one resize toward its direction, coarse and clamped. At the
        /// floor/ceil zellij REFUSES (cols unchanged) — the loop must cope.
        fn apply(&mut self, fx: &Effect) {
            match fx {
                Effect::ShrinkSelf => {
                    self.cols = self.cols.saturating_sub(self.step).max(self.floor)
                }
                Effect::GrowSelf => self.cols = (self.cols + self.step).min(self.ceil),
                _ => {}
            }
        }
    }

    /// A disturbance injected once, after `n` effect-emitting steps.
    #[derive(Clone, Debug)]
    enum Interrupt {
        /// Alt+c mid-seek: flips the target and re-arms the budget (round 20).
        Toggle,
        /// A window reflow / relayout slams cols by a large delta mid-seek —
        /// width_seek must NOT learn it as a step (round 17: step=60 poisoned
        /// the acceptance band into calling a 13-col bar "close enough" to 26).
        Jump(i64),
    }

    /// A relayout keeps re-rendering the plugin even after the seek falls
    /// silent; this many CONSECUTIVE silent re-presentations of the same cols
    /// (with no queued resize) is our proof the bar has genuinely settled, as
    /// opposed to being one render away from noticing a clobber-drift (issue
    /// #4). Two suffices: the seek's own recovery (grace + drift confirmation)
    /// never needs more than one silent render before it acts again.
    const SETTLE_RENDERS: u32 = 2;

    /// Drive width_seek in the render-feedback loop until the model goes SILENT
    /// at stable cols (converged, floored, or budget-exhausted), returning the
    /// largest per-segment effect-step count (a segment is the run between
    /// budget re-arms — a Toggle or an issue-#4 drift re-arm starts a fresh
    /// one). A hard iteration cap turns a non-terminating model (the whole
    /// point of the SEEK_BUDGET cap) into a loud failure instead of a hang.
    fn drive(
        model: &mut BarModel,
        sim: &mut SimZellij,
        interrupt: Option<(u32, Interrupt)>,
    ) -> u32 {
        let mut seg = 0u32; // effect steps since the last (re-)arm
        let mut max_seg = 0u32;
        let mut fired = interrupt.is_none();
        let mut iters = 0u32;
        let mut settle = 0u32; // consecutive idle re-renders at stable cols
        loop {
            iters += 1;
            assert!(iters < 1024, "width_seek livelocked at {} cols", sim.cols);
            let budget_before = model.seek_budget;
            let fx = model.width_seek(sim.cols);
            // A drift re-arm (issue #4) hands the seek a fresh budget — hence a
            // fresh segment, exactly as a Toggle does. Reset the per-segment
            // counter so assertion (a) bounds each episode, not the sum across
            // re-arms.
            if model.seek_budget > budget_before {
                seg = 0;
            }
            match fx.as_slice() {
                [] => {
                    // Model idle. Flush a deferred (latency) resize if queued;
                    // that is progress, not settling.
                    if let Some(p) = sim.pending.take() {
                        sim.apply(&p);
                        settle = 0;
                        continue;
                    }
                    // No queued resize and the model is quiet. Re-present the
                    // current cols: a genuinely settled bar stays silent, but a
                    // bar left off-target by a clobbered resize (issue #4) needs
                    // the next render to notice and re-seek. Sustained silence
                    // across SETTLE_RENDERS re-presentations means settled.
                    settle += 1;
                    if settle >= SETTLE_RENDERS {
                        return max_seg.max(seg);
                    }
                    continue;
                }
                [only] => {
                    settle = 0;
                    seg += 1;
                    max_seg = max_seg.max(seg);
                    if !fired
                        && let Some((after, ref kind)) = interrupt
                        && seg == after
                    {
                        fired = true;
                        match kind {
                            // Supersede the just-emitted resize: the user's
                            // toggle re-aims the seek and re-arms the budget, so
                            // a new segment begins.
                            Interrupt::Toggle => {
                                model.toggle();
                                sim.pending = None;
                                seg = 0;
                                continue;
                            }
                            // The reflow overrides our resize outright.
                            Interrupt::Jump(d) => {
                                sim.cols = (sim.cols as i64 + d)
                                    .clamp(sim.floor as i64, sim.ceil as i64)
                                    as usize;
                                continue;
                            }
                        }
                    }
                    if sim.latency {
                        sim.pending = Some(only.clone());
                    } else {
                        sim.apply(only);
                    }
                }
                other => panic!("width_seek emitted a non-resize effect: {other:?}"),
            }
        }
    }

    /// Half the effective acceptance band: width_seek stops when
    /// `2*|cols-target| <= seek_step.max(8)` (the ±4 pre-learning slack, round
    /// 20), where seek_step learns the sim's own step (≤ MAX_LEARNABLE_STEP).
    fn band_half(step: usize) -> usize {
        step.max(8) / 2
    }

    /// D37, found live: a bar loading into a fleet that was left COLLAPSED
    /// believes it is expanded until `clave snapshot` answers. Acting on that
    /// belief grows a correctly-born 30-column bar toward 54, then shrinks it
    /// back when the snapshot lands — visible jank at every launch and every
    /// new tab.
    ///
    /// The first assertion is the one that matters and the one no existing test
    /// could make: an ungated model at 30 emits `GrowSelf`, which is precisely
    /// the wrong move. Asserting only the post-hydration half would pass
    /// whether or not the gate exists.
    #[test]
    fn a_bar_awaiting_hydration_does_not_seek_on_its_assumed_mode() {
        // Ungated, for contrast: believing itself expanded, it grows away from
        // the width it was correctly born at.
        let mut ungated = BarModel::default();
        assert_eq!(
            ungated.width_seek(COLLAPSED_TARGET_COLS),
            vec![Effect::GrowSelf],
            "the pre-D37 behaviour this gate exists to prevent"
        );

        let mut m = BarModel::default();
        m.await_hydration();
        assert_eq!(
            m.width_seek(COLLAPSED_TARGET_COLS),
            Vec::<Effect>::new(),
            "seeked before knowing its own collapse mode"
        );
        // The budget is intact — a deferral must not spend an attempt, or a
        // slow snapshot would leave the bar unable to correct a stale birth.
        assert_eq!(m.seek_budget, SEEK_BUDGET);

        // The snapshot lands carrying the real mode; the seek is live again and
        // now agrees with the width the pane was born at, so it stays put.
        let mut collapsed_snap = snap(1, vec![]);
        collapsed_snap.collapsed = true;
        m.apply_snapshot(collapsed_snap);
        assert!(m.collapsed);
        assert_eq!(
            m.width_seek(COLLAPSED_TARGET_COLS),
            Vec::<Effect>::new(),
            "a correctly-born collapsed bar must not move"
        );
    }

    #[test]
    fn harness_newborn_converges_on_the_template_from_above() {
        // A percent-sized birth lands window-dependent and the birth-armed seek
        // (C8 2026-07-18) must finish the job onto BAR_TARGET_COLS.
        //
        // The START WIDTH is chosen to force MORE THAN ONE resize, and that is
        // the point of the test — one step would only prove the pre-learning
        // ±4 slack. RE-DERIVED for the 54 target (D19), because the old start
        // of 66 lands 66 → 54 in a single step and would have gone green while
        // covering half of what it claims. Requiring the first landing outside
        // the band and the second inside gives `S − 24 ∈ [48, 60]` and
        // `|S − 66| > 6`, so S > 72: at S = 78, 78 → 66 (diff 12 > the learned
        // band half 6, so it acts again) → 54 (diff 0, accepted).
        //
        // This is the third time this start has had to move with the target,
        // and each time the failure mode is the same one (#63 shape): a test
        // that stays green and quietly covers less.
        let mut m = BarModel::default();
        let mut sim = SimZellij::new(78, 12, 0, 200, false);
        let steps = drive(&mut m, &mut sim, None);
        assert!(
            steps >= 2,
            "start width must drive at least two resizes, drove {steps}"
        );
        assert!(
            sim.cols.abs_diff(BAR_TARGET_COLS) <= band_half(sim.step),
            "newborn ended at {} (target {BAR_TARGET_COLS})",
            sim.cols
        );
        // (c) silent forever at the converged width.
        for _ in 0..4 {
            assert_eq!(m.width_seek(sim.cols), Vec::<Effect>::new());
        }
    }

    #[test]
    fn harness_collapse_converges_on_the_gutter() {
        let mut m = BarModel::default();
        m.toggle(); // collapsed → target COLLAPSED_TARGET_COLS
        // Start at 72, not 30: 30 IS the collapsed target now, and a harness
        // that starts on its target proves nothing (#63).
        let mut sim = SimZellij::new(72, 9, 0, 200, false);
        drive(&mut m, &mut sim, None);
        assert!(
            sim.cols.abs_diff(COLLAPSED_TARGET_COLS) <= band_half(sim.step),
            "collapse ended at {} (target {COLLAPSED_TARGET_COLS})",
            sim.cols
        );
    }

    #[test]
    fn harness_floor_above_target_rests_benignly() {
        // Round 20 ruling: "wherever cols stop changing is accepted." When the
        // resize floor sits ABOVE the collapsed target, the seek rests at the
        // floor and the in-flight guard keeps it silent — no thrash.
        let mut m = BarModel::default();
        m.toggle();
        // Floor 38 > COLLAPSED_TARGET_COLS (30), and far enough above it that
        // the ±4 slack band cannot mistake the floor for convergence. The old
        // pair (start 30, floor 12) was derived against a target of 4.
        let mut sim = SimZellij::new(72, 8, 38, 200, false);
        drive(&mut m, &mut sim, None);
        assert_eq!(sim.cols, 38, "did not rest at the floor");
        for _ in 0..8 {
            assert_eq!(m.width_seek(sim.cols), Vec::<Effect>::new());
        }
    }

    #[test]
    fn harness_latency_path_exercises_the_in_flight_guard() {
        // With one-render latency the model re-sees the old cols once per step
        // (prev == own_cols → the double-fire guard, round 9); convergence must
        // survive it.
        let mut m = BarModel::default();
        let mut sim = SimZellij::new(70, 11, 0, 200, true);
        drive(&mut m, &mut sim, None);
        assert!(
            sim.cols.abs_diff(BAR_TARGET_COLS) <= band_half(sim.step),
            "latency seek ended at {}",
            sim.cols
        );
    }

    #[test]
    fn harness_peek_cycle_expands_then_sinks() {
        // Round 21 peek-on-nav: a collapsed bar seeks the TEMPLATE while
        // peeking, then sinks to the gutter when the peek expires.
        let mut m = BarModel::default();
        m.toggle(); // collapsed
        // 60, not 30: starting on COLLAPSED_TARGET_COLS would make the first
        // leg a no-op and the assertion below vacuous (#63).
        let mut sim = SimZellij::new(60, 8, 0, 200, false);
        drive(&mut m, &mut sim, None); // settle at the collapsed target
        assert!(sim.cols.abs_diff(COLLAPSED_TARGET_COLS) <= band_half(sim.step));
        // A nav arms a peek (collapsed → template) and re-arms the seek.
        assert!(m.visited(7));
        drive(&mut m, &mut sim, None);
        assert!(
            sim.cols.abs_diff(BAR_TARGET_COLS) <= band_half(sim.step),
            "peek did not expand to the template: {}",
            sim.cols
        );
        // Expiry sinks back to the gutter.
        assert!(m.peek_expired());
        drive(&mut m, &mut sim, None);
        assert!(
            sim.cols.abs_diff(COLLAPSED_TARGET_COLS) <= band_half(sim.step),
            "peek did not sink to the gutter: {}",
            sim.cols
        );
    }

    #[test]
    fn harness_toggle_mid_seek_re_aims_at_the_new_target() {
        // Alt+c mid-flight: the in-progress expand is abandoned and the seek
        // re-aims at the collapsed target, still converging within one fresh
        // budget.
        let mut m = BarModel::default(); // expanded, target 44
        let mut sim = SimZellij::new(5, 7, 0, 200, false);
        let max_seg = drive(&mut m, &mut sim, Some((2, Interrupt::Toggle)));
        assert!(
            max_seg <= SEEK_BUDGET,
            "a segment exceeded the budget: {max_seg}"
        );
        assert!(m.collapsed, "toggle should have collapsed the bar");
        assert!(
            sim.cols.abs_diff(COLLAPSED_TARGET_COLS) <= band_half(sim.step),
            "post-toggle seek ended at {}",
            sim.cols
        );
    }

    // === Property tests (issue #10 item 3) =================================
    // proptest generalizes the example-based tests over the model's
    // divergence-critical invariants. Each property cites the ledger finding
    // it guards. Host-side only: proptest is a dev-dependency and never reaches
    // the wasm --target build (see crates/clave-bar/Cargo.toml).
    mod proptests {
        use super::super::*;
        use super::{Interrupt, SimZellij, agent, drive, tab};
        use proptest::prelude::*;

        proptest! {
            // Each case drives a full feedback loop; 128 keeps CI modest while
            // still covering the start×step×floor×interrupt space densely.
            #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

            /// Property 1 — the convergence contract (a)–(d), C6 rounds 9/17/20.
            #[test]
            fn prop_width_seek_converges_or_bounds(
                start in 0usize..=500,
                step in 1usize..=20,
                floor in 0usize..=40,
                collapsed in any::<bool>(),
                peeking in any::<bool>(),
                latency in any::<bool>(),
                interrupt in prop_oneof![
                    Just(Option::<(u32, Interrupt)>::None),
                    (1u32..=6).prop_map(|n| Some((n, Interrupt::Toggle))),
                    (1u32..=6, prop_oneof![-200i64..=-25, 25i64..=200])
                        .prop_map(|(n, d)| Some((n, Interrupt::Jump(d)))),
                ],
            ) {
                // Arm the requested start state directly (private fields are
                // reachable — this module is a descendant of `model`):
                // collapsed/peeking pick the target, a fresh budget mimics the
                // birth/toggle arming.
                let mut m = BarModel {
                    collapsed,
                    peeking,
                    seek_budget: SEEK_BUDGET,
                    seek_last_cols: None,
                    ..BarModel::default()
                };
                let mut sim = SimZellij::new(start, step, floor, 500, latency);
                let max_seg = drive(&mut m, &mut sim, interrupt);

                // (a) each budget segment terminates within SEEK_BUDGET emits.
                prop_assert!(max_seg <= SEEK_BUDGET, "segment {} exceeded budget", max_seg);
                // (d) an external jump is never learned as the step size.
                prop_assert!(
                    m.seek_step <= MAX_LEARNABLE_STEP,
                    "learned an over-size step: {}",
                    m.seek_step
                );
                // (c) silent forever at the resting width — no oscillation.
                for _ in 0..4 {
                    prop_assert!(
                        m.width_seek(sim.cols).is_empty(),
                        "storm at {} cols",
                        sim.cols
                    );
                }
                // (b) FLOOR-PERMITTING convergence: end within half the
                // effective step of the active target, OR rested at the floor,
                // OR the budget was spent mid-travel (round 20 admits all three
                // as terminal states — (c) already proved the rest is quiet).
                // `showing_collapsed()`, never a restatement of it: it is the
                // single source of the peek-aware collapse rule, and a copy
                // here is the one site that could drift from the seek.
                let target = if m.showing_collapsed() {
                    COLLAPSED_TARGET_COLS
                } else {
                    BAR_TARGET_COLS
                };
                // The band is HALF a step — except where the lattice is coarse
                // enough for the two targets' bands to overlap. There,
                // `converged` refuses the overlap outright and the
                // disambiguating step can land up to a FULL step out: a
                // visibly-collapsed bar a few columns off target, rather than a
                // perfectly-parked one that never moved.
                //
                // **RE-TIGHTENED at D19 as D26 required.** The threshold is
                // `>` the separation, not `>=`: at exactly the separation the
                // tight half-band still holds, and the `>=` form permitted
                // `|w − target| <= step`, which at step 20 and target 30 spans
                // 44 — it would have greened "the collapsed bar settled exactly
                // at the expanded target". The invariant is separately and
                // exhaustively pinned by `no_width_is_accepted_for_both_targets`,
                // which mutation testing confirmed is the only test that catches
                // a reversion.
                //
                // At 54/30 the separation is 24 and `effective` cannot exceed
                // `MAX_LEARNABLE_STEP` (20), so the widening branch is now
                // UNREACHABLE — kept because it is the seek's actual contract,
                // not because this geometry needs it. Narrow the targets back
                // under 20 apart and it goes live again.
                let effective = step.max(PRE_LEARNING_STEP);
                let bound = if effective > BAR_TARGET_COLS.abs_diff(COLLAPSED_TARGET_COLS) {
                    effective
                } else {
                    effective / 2
                };
                let within = sim.cols.abs_diff(target) <= bound;
                let at_floor = sim.cols == floor;
                let exhausted = max_seg == SEEK_BUDGET;
                prop_assert!(
                    within || at_floor || exhausted,
                    "ended at {} (target {}, floor {}, seg {})",
                    sim.cols, target, floor, max_seg
                );
            }

            /// Property 2 — focus never reorders (§6.6: focus is a beacon, not a
            /// commitment). Any assignment of the active flag, plus a beacon on
            /// that tab, leaves rows() order identical.
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
                    m.apply_snapshot(AgentSnapshot { collapsed: false, seq: 1, agents: vec![], tab_timeline: tl });
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
            }

            /// Property 3 — rows() is deterministic and unified-recency-ordered:
            /// live tabs key on the STORE tab_timeline (NOT the bound agent's
            /// last_interacted — the round-6 divergence), dormant rows on
            /// last_interacted, merged strictly descending.
            #[test]
            fn prop_rows_deterministic_and_recency_desc(
                n in 1usize..=4,
                tl_vals in prop::collection::vec(0u64..500, 4),
                li_vals in prop::collection::vec(0u64..500, 4),
                dormant_lis in prop::collection::vec(0u64..500, 0..4),
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
                let mut li_by_uuid: std::collections::BTreeMap<String, u64> = Default::default();
                for (j, li) in dormant_lis.iter().enumerate() {
                    let mut a = agent(&format!("d{j}"), Status::Idle, None);
                    a.last_interacted = *li;
                    li_by_uuid.insert(a.uuid.clone(), *li);
                    agents.push(a);
                }
                let timeline: std::collections::BTreeMap<usize, u64> = ids
                    .iter()
                    .enumerate()
                    .map(|(i, &id)| (id, tl_vals[i]))
                    .collect();
                m.apply_snapshot(AgentSnapshot { collapsed: false, seq: 1, agents, tab_timeline: timeline.clone() });

                // Determinism: identical inputs → identical rows.
                prop_assert_eq!(m.rows(), m.rows());

                // Unified recency: each row's sort ts (timeline for live, li for
                // dormant) is non-increasing down the list.
                let rows = m.rows();
                let ts_of = |k: &RowKey| -> u64 {
                    match k {
                        RowKey::Tab(id) => timeline.get(id).copied().unwrap_or(0),
                        RowKey::Dormant(u) => li_by_uuid.get(u).copied().unwrap_or(0),
                    }
                };
                for w in rows.windows(2) {
                    prop_assert!(
                        ts_of(&w[0].0) >= ts_of(&w[1].0),
                        "recency inverted between {:?} and {:?}",
                        w[0].0, w[1].0
                    );
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
                    collapsed: false,
                    seq: cur_seq,
                    agents: vec![agent("u1", Status::Working, Some(0))],
                    tab_timeline: tl0,
                });
                let rows0 = m.rows();
                let timeline0 = m.timeline.clone();
                m.apply_snapshot(AgentSnapshot {
                    collapsed: false,
                    seq: stale_seq,
                    agents: vec![agent("u2", Status::Failed, Some(1))],
                    tab_timeline: tl1,
                });
                prop_assert_eq!(m.rows(), rows0, "stale snapshot mutated rows");
                prop_assert_eq!(m.timeline.clone(), timeline0, "stale snapshot mutated timeline");
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
                m.apply_snapshot(AgentSnapshot { collapsed: false, seq: 1, agents: vec![], tab_timeline: tl0 });
                m.apply_snapshot(AgentSnapshot {
                    collapsed: false,
                    seq: 2,
                    agents: vec![agent("u1", Status::Working, Some(3))],
                    tab_timeline: tl1.clone(),
                });
                prop_assert_eq!(m.timeline.clone(), tl1);
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
                                collapsed: *flag,
                                seq,
                                agents: vec![],
                                tab_timeline: Default::default(),
                            });
                            match pending {
                                Some(w) if *flag == w => pending = None,
                                Some(_) if !reasserted => reasserted = true,
                                Some(_) => {
                                    pending = None;
                                    expected = *flag;
                                }
                                None => expected = *flag,
                            }
                        }
                        None => {
                            m.toggle();
                            expected = !expected;
                            pending = Some(expected);
                            reasserted = false;
                        }
                    }
                    if *inject_stale {
                        // A replayed/out-of-order snapshot (seq <= current)
                        // carrying the OPPOSITE flag must change nothing —
                        // not even the pending ledger.
                        m.apply_snapshot(AgentSnapshot {
                            collapsed: !expected,
                            seq,
                            agents: vec![],
                            tab_timeline: Default::default(),
                        });
                    }
                    prop_assert_eq!(m.collapsed, expected);
                }
            }

            /// Property 6 — nav closure (§6.6 C8), scoped precisely (fugu
            /// review 2026-07-20): with the executor pinned to its birth tab
            /// (as below — a single-instance view), every nav bumps
            /// cursor_gen exactly once, a cursor that lands dormant is always
            /// a DISPLAYED dormant row, and a stale-gen dwell is a no-op.
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
                    collapsed: false,
                    seq: 1,
                    agents,
                    tab_timeline: ids.iter().map(|&id| (id, 50u64)).collect(),
                });
                m.beacon(ids[0]);
                let own = ids[0];
                for d in dirs {
                    let before = m.cursor_gen;
                    let payload = if d { "{\"dir\":\"next\"}" } else { "{\"dir\":\"prev\"}" };
                    m.nav(payload, Some(own));
                    // Rows are non-empty (≥1 live tab) → every dir lands once.
                    prop_assert_eq!(m.cursor_gen, before + 1, "gen did not bump once per nav");
                    if let Some(u) = m.cursor.clone() {
                        let rows = m.rows();
                        prop_assert!(
                            rows.iter().any(|(k, _)| *k == RowKey::Dormant(u.clone())),
                            "cursor on a row that is not displayed: {}",
                            u
                        );
                    }
                }
                // A dwell stamped before any landing (gen 0) is provably stale.
                prop_assert!(m.dwell_expired(0).is_empty(), "stale-gen dwell fired");
            }

            /// Property 7a — classify_timer, spec-phrased partial contract
            /// (fugu review 2026-07-20: the original property re-derived the
            /// function's own branch expression, so a logic bug could never
            /// diverge from the test's expectation). This half states the
            /// doc-comment's FIRST claim on its own terms: a short elapsed is
            /// ALWAYS a dwell — a 0.9s peek sleep never reports < 0.9
            /// (model.rs ~93), so whatever the pending counters say, a
            /// sub-cutoff expiry can only be the dwell timer.
            #[test]
            fn prop_classify_timer_short_elapsed_is_always_dwell(
                elapsed in 0.0f64..TIMER_KIND_CUTOFF_SECS,
                dwells in 0usize..5,
                peeks in 0u32..5,
            ) {
                prop_assert_eq!(classify_timer(elapsed, dwells, peeks), TimerKind::Dwell);
            }

            /// Property 7b — the doc-comment's second claim, independently
            /// phrased: a long-elapsed expiry while a peek IS pending belongs
            /// to that peek — the late-dwell rescue exists only for the
            /// no-peek-pending case and must never steal an owned peek
            /// (otherwise a collapsed bar's sink timer would be eaten and the
            /// gutter never re-sunk). The rescue corner itself is a fixed
            /// boundary, pinned in classify_timer_late_dwell_rescue_boundary.
            #[test]
            fn prop_classify_timer_pending_peek_owns_long_expiries(
                over in 0.0f64..2.0,
                dwells in 0usize..5,
                peeks in 1u32..5,
            ) {
                prop_assert_eq!(
                    classify_timer(TIMER_KIND_CUTOFF_SECS + over, dwells, peeks),
                    TimerKind::Peek
                );
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
                                    },
                                    PaneMeta {
                                        tab_position: pos,
                                        pane_id: term_pane(id),
                                        is_plugin: false,
                                        is_focused: false,
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
                            collapsed: false,
                            seq: self.seq,
                            agents: vec![
                                agent("u1", Status::Working, self.holder(true)),
                                agent("u2", Status::Working, self.holder(false)),
                            ],
                            // Seeded wide so the birth touch is mostly out of
                            // the way — these properties are about binds. A
                            // long run can still create a tab id past the seed
                            // and emit a Touch, which is harmless: only Bind is
                            // inspected, and the fail-closed assert covers all
                            // effects either way.
                            tab_timeline: (0..256).map(|t| (t, 100u64)).collect(),
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

        /// The late-dwell rescue corner (companion to properties 7a/7b): a
        /// long elapsed with NO peek pending but a dwell owed is reclassified
        /// as that dwell (the FIFO off-by-one hardening, model.rs ~88-92);
        /// with nothing owed at all it stays a peek (harmless default — an
        /// unowned peek expiry is dropped by the caller).
        #[test]
        fn classify_timer_late_dwell_rescue_boundary() {
            assert_eq!(
                classify_timer(TIMER_KIND_CUTOFF_SECS, 1, 0),
                TimerKind::Dwell
            );
            assert_eq!(
                classify_timer(TIMER_KIND_CUTOFF_SECS, 0, 0),
                TimerKind::Peek
            );
        }
    }
}
