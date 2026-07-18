//! Pure display/model logic for clave-bar — deliberately NO zellij-tile
//! imports so it compiles and unit-tests on the host. main.rs adapts zellij
//! events into these plain types and executes the returned `Effect`s.
//!
//! The three separated concerns of spec §6.6:
//!   row SET        = zellij's tabs (apply_tabs)
//!   row ORDER      = interaction recency (logical clock, this module)
//!   row DECORATION = clave's pushed snapshots (apply_snapshot)

use std::collections::{BTreeMap, BTreeSet};

use clave_types::{Agent, AgentSnapshot, Status};

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
    /// run_command(["clave","focus",uuid]) — persist the unread clear.
    MarkRead { uuid: String },
    /// run_command(["clave","bind",uuid,tab_id]) — report the uuid→tab join
    /// to the STORE (§6.6 Design B), fired by the agent tab's own bar.
    Bind { uuid: String, tab_id: usize },
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

/// The expanded width the seek converges to. The generated layouts
/// (setup::layout_kdl and add::tab_layout) size the bar pane in PERCENT —
/// a fixed `size=30` made zellij refuse every resize (CantResizeFixedPanes)
/// — so births land near this and the birth-armed seek finishes the job.
const BAR_TARGET_COLS: usize = 30;
/// Collapsed width target (Alt+c): a glyph gutter — the state glyph plus a
/// couple of name chars survive the renderer's own truncation, so "mini
/// mode" needs no special render path. Zellij's resize floor may stop the
/// seek above this; wherever cols stop changing is accepted.
const COLLAPSED_TARGET_COLS: usize = 4;
/// Seek steps allowed per toggle (each is a real zellij layout action):
/// enough for the widest transition at ~5%-of-viewport per step, small
/// enough that a layout which refuses to converge isn't fought forever.
const SEEK_BUDGET: u32 = 16;
/// Deltas beyond this are external jumps (window resize, relayout), not a
/// single zellij resize step (5% of viewport ≈ 7–14 cols on real screens) —
/// learning one poisons the acceptance band (step=60 seen live, round 17:
/// it accepted a 13-col bar as "close enough" to 26).
const MAX_LEARNABLE_STEP: usize = 20;

/// Row identity (§6.6 C8): a live zellij tab, or a dormant store row
/// (conversation with no tab yet — claude.ai-style list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKey {
    Tab(usize),
    Dormant(String),
}

/// One rendered row, already in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: RowKey,
    pub name: String,
    pub active: bool,
    /// (glyph, ANSI colour) for agent rows; None for plain terminal tabs.
    pub glyph: Option<(char, u8)>,
}

pub struct BarModel {
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
    /// Last uuid→tab bind THIS instance sent (`clave bind`). Guard is
    /// last-SENT, deliberately not the snapshot echo — echo-gated guards
    /// re-fire under congestion (C5 rd 4 spawn storm).
    sent_binds: BTreeMap<String, usize>,
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
            seq: 0,
            agents: Vec::new(),
            uuid_to_pane: BTreeMap::new(),
            tabs: Vec::new(),
            panes: Vec::new(),
            timeline: BTreeMap::new(),
            birth_touched: BTreeSet::new(),
            renamed: BTreeMap::new(),
            sent_binds: BTreeMap::new(),
            read_locally: BTreeSet::new(),
            seek_last_cols: None,
            seek_step: 0,
            current_tab: None,
            birth_announced: false,
            organic_pending: false,
            collapsed: false,
            peeking: false,
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
        self.seek_budget = SEEK_BUDGET; // re-arm toward the template
        self.seek_last_cols = None;
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
        self.seek_budget = SEEK_BUDGET; // re-arm toward the gutter
        self.seek_last_cols = None;
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

    /// The agent bound to this tab, per the SNAPSHOT (§6.6 Design B) — the
    /// only join every instance agrees on. Local register/manifest joins are
    /// used solely to CREATE binds (bind_effects).
    fn agent_in_tab(&self, tab_id: usize) -> Option<&Agent> {
        self.agents.iter().find(|a| a.tab_id == Some(tab_id))
    }

    /// §6.6 Design B bootstrap: agents whose REGISTERED pane sits in
    /// `own_tab` per MY manifest, but whose snapshot bind disagrees. Callers
    /// gate to the active instance (fresh manifest — a stale one would bind
    /// wrong tabs). Once per computed value per instance (sent_binds).
    pub fn bind_effects(&mut self, own_tab: usize) -> Vec<Effect> {
        let own_position = self
            .tabs
            .iter()
            .find(|t| t.tab_id == own_tab)
            .map(|t| t.position);
        let mut out = Vec::new();
        for a in &self.agents {
            let joined_here = self
                .uuid_to_pane
                .get(&a.uuid)
                .and_then(|p| self.tab_position_of_pane(*p))
                .is_some_and(|pos| Some(pos) == own_position);
            if joined_here
                && a.tab_id != Some(own_tab)
                && self.sent_binds.get(&a.uuid) != Some(&own_tab)
            {
                self.sent_binds.insert(a.uuid.clone(), own_tab);
                out.push(Effect::Bind {
                    uuid: a.uuid.clone(),
                    tab_id: own_tab,
                });
            }
        }
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
        self.agents = snap.agents;
        // REPLACE the tab timeline — the store's map is authoritative and
        // self-healing by construction; merging deltas is the exact failure
        // mode that diverged live (C5 round 5).
        self.timeline = snap.tab_timeline;
        let mut effects = Vec::new();
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
        // Bounded beacon announce (rounds 11–12): only at BIRTH (first
        // TabUpdate this instance ever gets — new tab or plugin load) or
        // when ARMED by Alt+o's clave-organic pipe. Never self-diagnosed
        // beyond that: per-instance active claims are stale during bursts
        // (C3) and every unbounded announce design stormed.
        let armed = !self.birth_announced || self.organic_pending;
        self.birth_announced = true;
        self.organic_pending = false; // consumed either way
        if armed
            && let Some(active) = self.tabs.iter().find(|t| t.active)
            && self.current_tab != Some(active.tab_id)
        {
            self.current_tab = Some(active.tab_id);
            effects.push(Effect::AnnounceVisit {
                tab_id: active.tab_id,
            });
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
        self.prune_opening(); // an appeared tab retires its ↻ mark
        effects
    }

    pub fn apply_panes(&mut self, panes: Vec<PaneMeta>) {
        self.panes = panes;
        self.prune_opening();
    }

    /// Rows in display order (§6.6 C8): ONE unified recency-desc list — live
    /// tabs keyed by the store tab_timeline, dormant store rows keyed by
    /// last_interacted. Tiebreak: tab position for live rows (fresh
    /// same-second tabs sit in tab order); for same-second dormant rows,
    /// stable and deterministic in uuid-DESCENDING order (uuid-ascending
    /// sort under a `usize::MAX - i` key inverts to descending).
    pub fn rows(&self) -> Vec<Row> {
        // §6.6 C8 virtual cursor: `Row.active` means "visually SELECTED".
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
        let mut entries: Vec<(u64, usize, Row)> = Vec::new();
        for t in &self.tabs {
            let glyph = self.agent_in_tab(t.tab_id).map(|a| {
                // Local unread override: render Done as Idle once seen.
                if a.status == Status::Done && self.read_locally.contains(&a.uuid) {
                    Status::Idle.glyph()
                } else {
                    a.status.glyph()
                }
            });
            entries.push((
                self.sort_key(t),
                t.position,
                Row {
                    key: RowKey::Tab(t.tab_id),
                    name: t.name.clone(),
                    // A dormant selection steals the highlight from every tab.
                    active: selected_dormant.is_none() && t.active,
                    glyph,
                },
            ));
        }
        let mut dormant: Vec<&Agent> =
            self.agents.iter().filter(|a| self.is_dormant(a)).collect();
        dormant.sort_by(|a, b| a.uuid.cmp(&b.uuid)); // stable tiebreak input
        for (i, a) in dormant.into_iter().enumerate() {
            let glyph = if a.stale {
                ('✗', 31) // open found the cwd missing (§5 stale)
            } else if self.opening.contains(&a.uuid) {
                ('↻', 33) // open in flight
            } else {
                ('◌', 90) // dormant conversation
            };
            entries.push((
                a.last_interacted,
                // After any same-second live row; among same-second dormant
                // rows this renders uuid-DESCENDING (uuid-asc sort, key
                // inverted) — stable and deterministic, which is all we need.
                usize::MAX - i,
                Row {
                    key: RowKey::Dormant(a.uuid.clone()),
                    name: a.label.clone(),
                    active: selected_dormant == Some(a.uuid.as_str()),
                    glyph: Some(glyph),
                },
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
        let Some(row) = self.rows().get(line).cloned() else {
            return Vec::new();
        };
        match row.key {
            RowKey::Tab(tab_id) => {
                let Some(position) =
                    self.tabs.iter().find(|t| t.tab_id == tab_id).map(|t| t.position)
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
                        .position(|r| r.key == RowKey::Dormant(u.clone()))
                })
                .or_else(|| rows.iter().position(|r| r.key == RowKey::Tab(own)))
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
        let Some(row) = line.and_then(|l| rows.get(l).cloned()) else {
            return Vec::new();
        };
        self.cursor_gen += 1; // every landing invalidates prior dwell arms
        match row.key {
            RowKey::Tab(tab_id) => {
                self.cursor = None; // live landing: focus truth takes over
                let Some(position) =
                    self.tabs.iter().find(|t| t.tab_id == tab_id).map(|t| t.position)
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
                    self.seek_budget = SEEK_BUDGET;
                    self.seek_last_cols = None;
                    fx.push(Effect::ArmPeek);
                }
                fx
            }
        }
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
    pub fn toggle(&mut self) -> bool {
        self.collapsed = !self.collapsed;
        self.peeking = false; // an explicit toggle outranks a pending peek
        self.seek_budget = SEEK_BUDGET;
        self.seek_last_cols = None;
        self.collapsed
    }

    /// One width-seek step for OUR OWN pane, driven by render cols (each of
    /// our resizes repaints us with the new width — the feedback loop
    /// proven in rounds 9–10; zellij sends no events for plugin resizes).
    ///
    /// Zellij resizes in ~5%-of-viewport increments (≈7–14 cols), far
    /// coarser than the targets — a naive "shrink while too wide"
    /// overshoots straight through them (27 → 13, round 9). So the step is
    /// LEARNED from each resize's observed effect, acceptance is "within
    /// half a step", and GrowSelf recovers an overshoot. Waiting for cols
    /// to actually change before re-acting keeps in-flight resizes from
    /// double-firing — which also makes zellij's resize FLOOR benign: at
    /// the floor cols stop changing, so the seek just stops firing.
    /// Budget-capped so a layout that refuses to converge isn't fought
    /// forever.
    pub fn width_seek(&mut self, own_cols: usize) -> Vec<Effect> {
        if self.seek_budget == 0 {
            return Vec::new();
        }
        // A peeking bar seeks the template width even though collapsed —
        // the collapse resumes when the peek expires.
        let target = if self.collapsed && !self.peeking {
            COLLAPSED_TARGET_COLS
        } else {
            BAR_TARGET_COLS
        };
        match self.seek_last_cols {
            Some(prev) if prev == own_cols => return Vec::new(), // in flight / floor
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
        // Pre-learning slack of 8 (±4 cols): a bar already within a few
        // cols of the target must be accepted, not nudged into an
        // overshoot dance.
        let step = self.seek_step.max(8) as i64;
        let diff = own_cols as i64 - target as i64;
        let action = if 2 * diff > step {
            Effect::ShrinkSelf
        } else if -2 * diff > step {
            Effect::GrowSelf
        } else {
            self.seek_budget = 0; // close enough: done, stay done
            return Vec::new();
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
        }
    }

    fn snap(seq: u64, agents: Vec<Agent>) -> AgentSnapshot {
        AgentSnapshot {
            seq,
            agents,
            tab_timeline: Default::default(),
        }
    }

    /// Snapshot carrying only a tab timeline (the §6.6 store-timeline).
    fn snap_t(seq: u64, timeline: &[(usize, u64)]) -> AgentSnapshot {
        AgentSnapshot {
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
        let names: Vec<String> = m.rows().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        // Commitments arrive via snapshot and order by wall clock…
        m.apply_snapshot(snap_t(1, &[(10, 1000), (11, 2000), (12, 1500)]));
        let names: Vec<String> = m.rows().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["b", "c", "a"]);
        // …and focus (beacon) does not reorder.
        m.beacon(10);
        assert_eq!(m.rows()[0].name, "b");
        // Agent prompts reorder ONLY through the store timeline (the hook
        // stamps tab_timeline via the bind, §6.6 Design B) — an agent's
        // last_interacted alone must NOT sort: render-time joins diverge
        // per instance (round 6).
        let mut s = snap(2, vec![agent("u1", Status::Working, Some(12))]);
        s.agents[0].last_interacted = 9999;
        s.tab_timeline = [(10, 1000), (11, 2000), (12, 1500)].into();
        m.apply_snapshot(s);
        assert_eq!(m.rows()[0].name, "b"); // li ignored, timeline rules
        // The prompt's stamp arrives IN the timeline → c fronts everywhere.
        m.apply_snapshot(snap_t(3, &[(10, 1000), (11, 2000), (12, 3000)]));
        assert_eq!(m.rows()[0].name, "c");
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
        assert_eq!(m.rows()[0].key, RowKey::Tab(10));
        // New snapshot: b now leads, and a's old entry is GONE (replace
        // semantics — a merge would have kept a at 2000 and diverged).
        m.apply_snapshot(snap_t(2, &[(11, 1000)]));
        let rows = m.rows();
        assert_eq!(rows[0].key, RowKey::Tab(11));
        // A stale seq must not replace anything (§5 gate).
        m.apply_snapshot(snap_t(1, &[(10, 9000)]));
        assert_eq!(m.rows()[0].key, RowKey::Tab(11));
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
        let a = rows.iter().find(|r| r.name == "agent-tab").unwrap();
        let p = rows.iter().find(|r| r.name == "plain").unwrap();
        assert_eq!(a.glyph, Some(('●', 33))); // Working = amber
        assert_eq!(p.glyph, None); // plain terminal: name only
        // An UNBOUND agent (bind not landed yet) decorates nothing.
        let mut m2 = BarModel::default();
        m2.apply_tabs(vec![tab(10, 0, "agent-tab", false)]);
        m2.apply_snapshot(snap(1, vec![agent("u1", Status::Working, None)]));
        assert_eq!(m2.rows()[0].glyph, None);
    }

    #[test]
    fn snapshot_seq_gate_discards_stale() {
        let mut m = BarModel::default();
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, Some(10))]));
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Failed, Some(10))])); // stale
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        assert_eq!(m.rows()[0].glyph, Some(('●', 33))); // still Working
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
        assert_eq!(m.rows()[0].glyph, Some(('●', 32))); // green, unread
        // Tab gains focus → local clear + MarkRead effect, exactly once.
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.contains(&Effect::MarkRead { uuid: "u1".into() }));
        assert_eq!(m.rows()[0].glyph, Some(('●', 90))); // rendered dim NOW
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.iter().all(|e| !matches!(e, Effect::MarkRead { .. })));
        // A later snapshot showing Working clears the local override.
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, Some(10))]));
        assert_eq!(m.rows()[0].glyph, Some(('●', 33)));
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
        assert_eq!(m.rows()[0].glyph, Some(('●', 90))); // dim immediately
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
        assert_eq!(m.rows()[0].key, RowKey::Tab(12));
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

    #[test]
    fn a_newborn_model_seeks_the_template_width() {
        // The generated layouts size the bar pane in PERCENT — fixed sizes
        // make zellij refuse every resize (CantResizeFixedPanes, the
        // Alt+c-dead live finding, c8-cold-start 2026-07-18) — so a newborn
        // bar's cols depend on the window. The seek must be armed at birth
        // to converge on the exact template width from either side.
        let mut m = BarModel::default();
        assert_eq!(m.width_seek(45), vec![Effect::ShrinkSelf]);
        let mut m = BarModel::default();
        assert_eq!(m.width_seek(18), vec![Effect::GrowSelf]);
    }

    #[test]
    fn seek_collapses_to_the_gutter_despite_coarse_steps() {
        // Round 20 (collapse-in-place): Alt+c drives OWN width between the
        // template (30) and the glyph gutter (4) — the pane is never
        // suppressed. Zellij resizes in ~5%-of-viewport steps (7–14 cols),
        // far coarser than either target: the step is LEARNED from each
        // resize's observed effect and acceptance is within half a step
        // (round-9 lesson: naive loops overshoot straight through).
        let mut m = BarModel::default();
        // Never toggled: geometry is the user's business.
        assert_eq!(m.width_seek(30), Vec::<Effect>::new());
        let mut m = collapsed_model();
        let mut cols = 30i64;
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
        // Within half a learned step of the 4-col gutter — and STAYS done
        // (later geometry is the user's business until the next toggle).
        assert!((cols - 4).abs() <= 4, "ended at {cols} cols");
        assert_eq!(m.width_seek(140), Vec::<Effect>::new());
    }

    #[test]
    fn seek_expands_back_to_template_width() {
        let mut m = collapsed_model();
        m.toggle(); // expanded again → seek re-armed toward 30
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
        // Simulated step 9 (not 7): from 5 the 7-ladder lands ON 26, which
        // the ±4 slack band accepts for either template width — 9 makes the
        // end position actually distinguish 30 from the old 26.
        assert!((cols - 30).abs() <= 4, "ended at {cols} cols");
    }

    #[test]
    fn seek_waits_for_inflight_resizes_and_zellijs_floor() {
        // In-flight guard (round-9 overshoot risk): same cols again = our
        // resize hasn't landed yet — WAIT, don't double-fire. The same
        // guard makes zellij's resize FLOOR benign: at the floor cols stop
        // changing, so the seek stops firing instead of thrashing.
        let mut m = collapsed_model();
        assert_eq!(m.width_seek(30), vec![Effect::ShrinkSelf]);
        for _ in 0..10 {
            assert_eq!(m.width_seek(30), Vec::<Effect>::new());
        }
        // Landed (30 → 16): learned step 14, keep shrinking toward 4.
        assert_eq!(m.width_seek(16), vec![Effect::ShrinkSelf]);
        // Floor: zellij refuses to go below 16 — cols never change again,
        // the guard holds forever, no thrash.
        for _ in 0..10 {
            assert_eq!(m.width_seek(16), Vec::<Effect>::new());
        }
    }

    #[test]
    fn seek_grows_back_from_an_overshoot() {
        // The round-9 live defect, seek edition: an overshoot past the
        // target is recovered by growing, and the half-step band accepts.
        let mut m = collapsed_model();
        m.toggle(); // expanded → target 30
        assert_eq!(m.width_seek(13), vec![Effect::GrowSelf]);
        // Landed (+14 → 27): learned step 14, |27−30| within half a step →
        // accept and retire.
        assert_eq!(m.width_seek(27), Vec::<Effect>::new());
        assert_eq!(m.width_seek(13), Vec::<Effect>::new()); // retired
    }

    #[test]
    fn seek_never_learns_an_external_jump_as_the_step_size() {
        // Round-17 lesson kept: a window resize can slam cols by far more
        // than one resize step; learning that delta poisons the acceptance
        // band (step=60 accepted a 13-col bar as "close enough" to 26).
        let mut m = collapsed_model();
        m.toggle(); // expanded → target 30
        assert_eq!(m.width_seek(75), vec![Effect::ShrinkSelf]);
        // External jump 75 → 15 (delta 60): recover, but don't learn 60.
        assert_eq!(m.width_seek(15), vec![Effect::GrowSelf]);
        // 40 is far off-template; a step of 60 would fake-accept it.
        assert_eq!(m.width_seek(40), vec![Effect::ShrinkSelf]);
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
        assert_eq!(m.width_seek(4), Vec::<Effect>::new()); // settled at gutter
        assert!(m.visited(7), "collapsed bar must arm a peek");
        assert_eq!(m.current_tab(), Some(7)); // still a beacon
        // Peek re-armed the seek toward the TEMPLATE despite collapsed.
        assert_eq!(m.width_seek(4), vec![Effect::GrowSelf]);
        // A second nav during the peek re-arms (main.rs counts its timers).
        assert!(m.visited(8));
        // Expiry: sink back toward the gutter.
        assert!(m.peek_expired());
        assert_eq!(m.width_seek(30), vec![Effect::ShrinkSelf]);
    }

    #[test]
    fn expanded_bars_ignore_peeks() {
        let mut m = BarModel::default();
        assert!(!m.visited(7), "expanded bar must not arm a peek");
        assert_eq!(m.current_tab(), Some(7)); // beacon still lands
        // No seek was armed — geometry stays the user's business.
        assert_eq!(m.width_seek(30), Vec::<Effect>::new());
    }

    #[test]
    fn toggle_cancels_a_peek_and_a_late_expiry_is_a_noop() {
        let mut m = collapsed_model();
        assert_eq!(m.width_seek(4), Vec::<Effect>::new());
        assert!(m.visited(7));
        // Alt+c mid-peek: now genuinely expanded; the peek flag must not
        // survive to fight the user's explicit toggle.
        m.toggle();
        assert!(!m.peek_expired(), "late timer after a toggle is a no-op");
        // Seek heads for the template (expanded), unpoisoned by the peek.
        assert_eq!(m.width_seek(13), vec![Effect::GrowSelf]);
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
        let rows = m.rows();
        assert_eq!(
            rows.iter().map(|r| r.key.clone()).collect::<Vec<_>>(),
            vec![RowKey::Tab(11), RowKey::Tab(12), RowKey::Tab(10)]
        );
        assert_eq!(rows[0].glyph, Some(('●', 33))); // ag-a working
        assert_eq!(rows[1].glyph, Some(('●', 32))); // ag-b done
        assert_eq!(rows[2].glyph, None);
    }

    #[test]
    fn store_rows_without_live_tabs_render_dormant() {
        // §6.6 C8: row set = TabUpdate ∪ dormant store rows. An agent whose
        // bind points at no current tab and whose registered pane is gone
        // renders ◌ dim, labeled from the store, recency = last_interacted.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(1, 0, "shell", true)]); // one plain live tab
        let mut a = agent("u-dormant", Status::Idle, None);
        a.label = "repo · main · fix".into();
        a.last_interacted = 500;
        m.apply_snapshot(AgentSnapshot {
            seq: 1,
            agents: vec![a],
            tab_timeline: Default::default(),
        });
        let rows = m.rows();
        assert_eq!(rows.len(), 2);
        let d = rows
            .iter()
            .find(|r| r.key == RowKey::Dormant("u-dormant".into()))
            .expect("dormant row rendered");
        assert_eq!(d.name, "repo · main · fix");
        assert!(!d.active);
        assert_eq!(d.glyph, Some(('◌', 90)));
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
            seq: 1,
            agents: vec![old, new],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        let keys: Vec<_> = m.rows().into_iter().map(|r| r.key).collect();
        assert_eq!(
            keys,
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
            seq: 1,
            agents: vec![agent("u1", Status::Working, Some(7))], // bound → live
            tab_timeline: Default::default(),
        });
        assert!(
            !m.rows()
                .iter()
                .any(|r| r.key == RowKey::Dormant("u1".into()))
        );
        // Bind gone (fresh session) but the pane join exists → still not dormant.
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(7, 0, "agent-tab", true)]);
        m.register("u2".into(), 42);
        m.apply_panes(vec![pane(0, 42, false, true)]);
        m.apply_snapshot(AgentSnapshot {
            seq: 1,
            agents: vec![agent("u2", Status::Working, None)],
            tab_timeline: Default::default(),
        });
        assert!(
            !m.rows()
                .iter()
                .any(|r| r.key == RowKey::Dormant("u2".into()))
        );
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
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        let fx = m.nav("{\"dir\":\"next\"}", Some(1));
        let Effect::ArmDwell { r#gen } = fx[0] else { panic!() };
        // Cursor moved away before expiry → stale gen, no open.
        m.nav("{\"dir\":\"next\"}", Some(1));
        assert!(m.dwell_expired(r#gen).is_empty());
        // Land again and let it expire in place → exactly one open, marked ↻.
        let fx = m.nav("{\"dir\":\"prev\"}", Some(1)); // back to dormant row 0
        let Effect::ArmDwell { r#gen } = fx[0] else { panic!() };
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
            seq: 1,
            agents: vec![a],
            tab_timeline: Default::default(),
        });
        let rows = m.rows();
        assert_eq!(rows[0].glyph, Some(('✗', 31)));
        assert!(m.opening.is_empty(), "stale snapshot clears in-flight");
        // In-flight (no stale): ↻.
        let mut m = BarModel::default();
        m.apply_snapshot(AgentSnapshot {
            seq: 1,
            agents: vec![agent("u2", Status::Idle, None)],
            tab_timeline: Default::default(),
        });
        m.opening.insert("u2".into());
        assert_eq!(m.rows()[0].glyph, Some(('↻', 33)));
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
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        // (a) walk onto the dormant row → it is active, the live tab is not.
        m.nav("{\"dir\":\"next\"}", Some(1)); // live row 1 → wrap → dormant row 0
        let rows = m.rows();
        assert_eq!(rows[0].key, RowKey::Dormant("u-d".into()));
        assert!(rows[0].active, "dormant selection highlighted");
        assert!(!rows[1].active, "live tab drops its highlight");
        // (b) a live landing clears the cursor → the tab highlights again.
        m.nav("{\"dir\":\"next\"}", Some(1)); // dormant → wrap → live tab
        let rows = m.rows();
        assert!(!rows[0].active, "dormant no longer selected");
        assert!(rows[1].active, "focused tab reclaims the highlight");
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
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64)]),
        });
        m.beacon(1);
        m.nav("{\"dir\":\"next\"}", Some(1)); // cursor now on the dormant row
        assert!(m.rows()[0].active); // dormant selected
        // The row goes LIVE: u-d binds to a new tab (2). Cursor still names
        // "u-d" but it no longer renders dormant.
        m.apply_tabs(vec![tab(1, 0, "live", false), tab(2, 1, "u-d", true)]);
        m.apply_snapshot(AgentSnapshot {
            seq: 2,
            agents: vec![agent("u-d", Status::Working, Some(2))],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64), (2usize, 600u64)]),
        });
        let rows = m.rows();
        assert!(rows.iter().all(|r| r.key != RowKey::Dormant("u-d".into())));
        let active: Vec<_> = rows
            .iter()
            .filter(|r| r.active)
            .map(|r| r.key.clone())
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
            seq: 1,
            agents: vec![a],
            tab_timeline: std::collections::BTreeMap::from([(1usize, 500u64), (2usize, 400u64)]),
        });
        m.beacon(1);
        let fx = m.nav("{\"dir\":\"prev\"}", Some(1)); // land on the dormant row
        let Effect::ArmDwell { r#gen } = fx[0] else { panic!() };
        assert!(m.rows()[0].active, "dormant selected before the native switch");
        // Native switch to tab 2 arrives as a visited-pipe beacon (no nav).
        m.beacon(2);
        let rows = m.rows();
        assert!(!rows[0].active, "dormant row releases the highlight");
        let active: Vec<_> = rows
            .iter()
            .filter(|r| r.active)
            .map(|r| r.key.clone())
            .collect();
        assert_eq!(active, vec![RowKey::Tab(2)], "focused tab reclaims it");
        // The now-orphaned dwell must no-op (cursor cleared).
        assert!(m.dwell_expired(r#gen).is_empty());
    }
}
