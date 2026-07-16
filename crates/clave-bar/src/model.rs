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
    /// move_pane_with_pane_id_in_direction(Plugin(pane_id), Left) — C6
    /// re-show repair. Round 16: the EXECUTOR heals every tab's bar (pane
    /// ids are global, the commands cross-tab) — hidden instances get no
    /// events, so self-only repair left other tabs broken until visited.
    MoveBarLeft { pane_id: u32 },
    /// resize_pane_with_id(Decrease→Right, Plugin(pane_id)) — C6 repair.
    ShrinkBar { pane_id: u32 },
    /// resize_pane_with_id(Increase→Right, Plugin(pane_id)) — C6 overshoot
    /// recovery.
    GrowBar { pane_id: u32 },
}

/// The generated layouts give the bar `size=26` (setup::layout_kdl and
/// add::tab_layout) — repair shrinks a re-shown bar back toward this.
const BAR_TARGET_COLS: usize = 26;
/// Repair steps allowed per show (each is a zellij layout action): enough
/// for a 50% split on a wide screen at ~5%/step, small enough that a layout
/// which refuses to converge isn't fought forever.
const REPAIR_BUDGET: u32 = 16;

/// Per-bar-pane C6 repair progress (see BarModel::repairs).
#[derive(Debug, Default)]
struct PaneRepair {
    /// Remaining steps; zeroed when geometry reaches target (done for good).
    budget: u32,
    /// cols at this pane's last resize action — wait until zellij's effect
    /// is visible (cols changed) before acting again, so in-flight resizes
    /// can't double-fire, and the observed delta is the LEARNED step size.
    last_cols: Option<usize>,
    /// Has a manifest confirmed this pane's x == 0 since arming? Zellij
    /// sometimes re-inserts at the remembered width but on the wrong SIDE
    /// (round 15) — width-accept alone must not retire the pane, or a
    /// correct-width bar stays stranded on the right forever.
    x_ok: bool,
}

/// Deltas beyond this are relayout jumps or double-landed fires, not a
/// single zellij resize step (5% of viewport ≈ 7–14 cols on real screens) —
/// learning one poisons the acceptance band for every pane (step=60 seen
/// live, round 17: it accepted a 13-col bar as "close enough" to 26).
const MAX_LEARNABLE_STEP: usize = 20;

/// One rendered row, already in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub tab_id: usize,
    pub name: String,
    pub active: bool,
    /// (glyph, ANSI colour) for agent rows; None for plain terminal tabs.
    pub glyph: Option<(char, u8)>,
}

#[derive(Default)]
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
    /// C6 repair state per bar pane, armed on toggle-show for every plugin
    /// pane KNOWN at that moment (bars in tabs created later get the
    /// correct template and must not be touched). Round 16: keyed by pane
    /// id because the EXECUTOR heals all tabs' bars — hidden instances get
    /// no events (C3), so self-only repair stranded other tabs until visit.
    repairs: BTreeMap<u32, PaneRepair>,
    /// Zellij's resize increment as observed (≈5% of viewport); 0 until
    /// learned from a resize's effect. Shared across panes — it's an
    /// environment property, not per-pane state.
    repair_step: usize,
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
    /// Bar visibility (Alt+c). main.rs maps this to hide_self/show_self.
    pub hidden: bool,
}

impl BarModel {
    /// A `clave-visited` pipe landed: some tab gained focus. Beacon ONLY —
    /// it elects the nav executor; it never reorders (§6.6: focus is not a
    /// commitment).
    pub fn beacon(&mut self, tab_id: usize) {
        self.current_tab = Some(tab_id);
        self.organic_pending = false; // truth arrived; leftover flags are poison
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

    /// Display line N (0-based) → that row's (tab_id, position).
    fn tab_for_line(&self, line: usize) -> Option<(usize, usize)> {
        let row_tab = self.rows().get(line)?.tab_id;
        let t = self.tabs.iter().find(|t| t.tab_id == row_tab)?;
        Some((t.tab_id, t.position))
    }

    pub fn register(&mut self, uuid: String, pane_id: u32) {
        self.uuid_to_pane.insert(uuid, pane_id);
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
        effects
    }

    pub fn apply_panes(&mut self, panes: Vec<PaneMeta>) {
        self.panes = panes;
    }

    /// Rows in display order: last-user-commitment desc (sort_key: touches ∨
    /// agent prompts, unix s), then tab position asc as the tiebreak (fresh
    /// same-second tabs, and anything never committed to, sit in tab order).
    pub fn rows(&self) -> Vec<Row> {
        let mut order: Vec<&TabMeta> = self.tabs.iter().collect();
        order.sort_by(|a, b| {
            let (ka, kb) = (self.sort_key(a), self.sort_key(b));
            kb.cmp(&ka).then(a.position.cmp(&b.position))
        });
        order
            .into_iter()
            .map(|t| {
                let glyph = self.agent_in_tab(t.tab_id).map(|a| {
                    // Local unread override: render Done as Idle once seen.
                    if a.status == Status::Done && self.read_locally.contains(&a.uuid) {
                        Status::Idle.glyph()
                    } else {
                        a.status.glyph()
                    }
                });
                Row {
                    tab_id: t.tab_id,
                    name: t.name.clone(),
                    active: t.active,
                    glyph,
                }
            })
            .collect()
    }

    /// Mouse click on rendered line N (0-based): jump to that row's tab.
    /// A click reaches exactly ONE instance (the visible bar), so the jump
    /// broadcasts a beacon for the other instances' executor election.
    /// Focus is not a commitment — clicks never reorder.
    pub fn click(&mut self, line: usize) -> Vec<Effect> {
        let Some((tab_id, position)) = self.tab_for_line(line) else {
            return Vec::new();
        };
        self.beacon(tab_id);
        vec![
            Effect::SwitchTab { position },
            Effect::AnnounceVisit { tab_id },
        ]
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
        let line = if let Some(n) = v.get("row").and_then(|n| n.as_u64()) {
            (n as usize).checked_sub(1) // 1-based → display line
        } else if let Some(dir) = v.get("dir").and_then(|d| d.as_str()) {
            let rows = self.rows();
            if rows.is_empty() {
                return Vec::new();
            }
            let cur = rows.iter().position(|r| r.tab_id == own).unwrap_or(0);
            match dir {
                "next" => Some((cur + 1) % rows.len()),
                "prev" => Some((cur + rows.len() - 1) % rows.len()),
                _ => None,
            }
        } else {
            None
        };
        let Some((tab_id, position)) = line.and_then(|l| self.tab_for_line(l)) else {
            return Vec::new();
        };
        self.beacon(tab_id); // executor hand-off hint; pipe echo confirms
        vec![
            Effect::SwitchTab { position },
            Effect::AnnounceVisit { tab_id },
        ]
    }

    /// TEMP round-15: is any C6 repair still armed? Lets main.rs trace
    /// "repair skipped by executor gate" without spamming every render.
    pub fn repair_armed(&self) -> bool {
        self.repairs.values().any(|r| r.budget > 0)
    }

    /// Alt+c. Returns the NEW hidden state; main.rs calls hide_self/show_self.
    /// Re-show ARMS geometry repair for every bar pane known right now:
    /// zellij re-inserts a shown pane as a fresh split (wrong side, 50% or
    /// remembered width) instead of restoring its layout slot (C6 finding,
    /// 2026-07-14). Per-show state starts fresh (round 15: a stale
    /// last_cols "learned" the re-insert jump as a bogus step size); the
    /// learned step itself is kept — zellij's resize increment is an
    /// environment property that doesn't change between shows.
    pub fn toggle(&mut self) -> bool {
        self.hidden = !self.hidden;
        if !self.hidden {
            self.repairs = self
                .panes
                .iter()
                .filter(|p| p.is_plugin)
                .map(|p| {
                    (
                        p.pane_id,
                        PaneRepair {
                            budget: REPAIR_BUDGET,
                            ..PaneRepair::default()
                        },
                    )
                })
                .collect();
        }
        self.hidden
    }

    /// One C6 repair tick over every bar pane the caller can see: move each
    /// to its tab's left edge, resize toward the template width, then
    /// retire it for good — after that, pane geometry is the user's
    /// business. Round 16: driven by the EXECUTOR only (the one instance
    /// with fresh state, nav-proven); `bars` is (pane_id, x, cols) straight
    /// from its manifest, so one toggle heals every tab without visits.
    ///
    /// Zellij resizes in ~5%-of-viewport increments (≈14 cols on a wide
    /// screen), far coarser than the 26-col target — a naive "shrink while
    /// too wide" overshoots straight through it (27 → 13, seen live,
    /// round 9). So the step size is LEARNED from a resize's observed
    /// effect, acceptance is "within half a step", and GrowBar recovers an
    /// overshoot. Waiting for a pane's cols to actually change before
    /// re-acting keeps in-flight resizes from double-firing. Budget-capped
    /// so a layout that refuses to converge isn't fought forever.
    ///
    /// Called from TWO event paths: PaneUpdate (fresh manifest, knows every
    /// pane's x) and render (x unknowable, own cols only — the caller
    /// passes `[(own, None, cols)]`). The render path is what converges the
    /// visible tab within one visit — zellij sends no PaneUpdate for the
    /// plugin's own resize's effect, but the pane re-renders with the new
    /// cols (round 10); other tabs' steps chain off the PaneUpdates their
    /// resizes broadcast.
    pub fn repair_tick(&mut self, bars: &[(u32, Option<usize>, usize)]) -> Vec<Effect> {
        let mut fx = Vec::new();
        // While hidden the bar panes are suppressed — firing at them is a
        // no-op that burns budget (round 18 trace: the timer kept spending
        // budget between a quick hide/show pair).
        if self.hidden {
            return fx;
        }
        for &(pane_id, x, cols) in bars {
            // Only panes armed at toggle-show; entries retire at budget 0.
            let Some(st) = self.repairs.get_mut(&pane_id) else {
                continue;
            };
            if st.budget == 0 {
                continue;
            }
            // TEMP round-15/16 trace — remove after C6 closes.
            eprintln!(
                "clave-bar: TRACE repair pane={pane_id} x={x:?} cols={cols} budget={} last={:?} step={}",
                st.budget, st.last_cols, self.repair_step
            );
            match x {
                Some(x) if x > 0 => {
                    st.budget -= 1;
                    fx.push(Effect::MoveBarLeft { pane_id });
                    continue;
                }
                Some(_) => st.x_ok = true, // at the left edge, confirmed
                // Round 18: STRICT phase ordering — no width fire until a
                // manifest confirms x == 0. Zellij's move is a geometry
                // SWAP: width-resizing a bar still on the right (render
                // can't know x) meant the landing move handed the shrunk
                // width to the terminal and the terminal's half-screen
                // width to the bar — the heal re-broke itself in a loop
                // (the "focused tab never heals" report; its render chain
                // was the fastest, so it always lost the race).
                None if !st.x_ok => continue,
                None => {} // left edge already confirmed: width may fire
            }
            match st.last_cols {
                // Resize in flight: never re-fire from event ticks — they
                // can be milliseconds apart (round 16: a count-based retry
                // double-fired before zellij applied the first resize).
                // Only repair_retry_tick (time-paced) clears this guard.
                Some(prev) if prev == cols => continue,
                Some(prev) => {
                    let delta = prev.abs_diff(cols);
                    // Only a plausible single resize step is LEARNED —
                    // relayout jumps poisoned the band (round 17, step=60).
                    if delta <= MAX_LEARNABLE_STEP {
                        self.repair_step = delta;
                    }
                }
                None => {}
            }
            // Pre-learning slack of 8 (±4 cols): a bar already within a few
            // cols of the template must be accepted, not nudged into an
            // overshoot dance — zellij re-inserts at remembered widths like
            // 29 (round 15), and its real step (~14) dwarfs the difference.
            let step = self.repair_step.max(8) as i64;
            let diff = cols as i64 - BAR_TARGET_COLS as i64;
            let action = if 2 * diff > step {
                Effect::ShrinkBar { pane_id }
            } else if -2 * diff > step {
                Effect::GrowBar { pane_id }
            } else if st.x_ok {
                st.budget = 0; // close enough + left edge: done, stay done
                // TEMP round-15/16 trace — remove after C6 closes.
                eprintln!("clave-bar: TRACE repair pane={pane_id} done at cols={cols}");
                continue;
            } else {
                // Width fine but x unconfirmed (round 15): stay armed so a
                // later manifest can still move a stranded bar off the right.
                continue;
            };
            st.budget -= 1;
            st.last_cols = Some(cols);
            fx.push(action);
        }
        fx
    }

    /// The TIME-paced repair tick (main's ~400ms zellij timer): clears
    /// every armed pane's in-flight guard first, so a resize that zellij
    /// clobbered (the unsuppress burst relayouts every tab, round 16) or
    /// silently dropped is re-fired. Events must never do this — during
    /// bursts they arrive every ~1ms, far faster than zellij applies
    /// resizes. Still budget-capped per pane, so a layout that refuses to
    /// converge is not fought forever.
    pub fn repair_retry_tick(&mut self, bars: &[(u32, Option<usize>, usize)]) -> Vec<Effect> {
        for st in self.repairs.values_mut() {
            if st.budget > 0 {
                st.last_cols = None;
            }
        }
        self.repair_tick(bars)
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
        assert_eq!(m.rows()[0].tab_id, 10);
        // New snapshot: b now leads, and a's old entry is GONE (replace
        // semantics — a merge would have kept a at 2000 and diverged).
        m.apply_snapshot(snap_t(2, &[(11, 1000)]));
        let rows = m.rows();
        assert_eq!(rows[0].tab_id, 11);
        // A stale seq must not replace anything (§5 gate).
        m.apply_snapshot(snap_t(1, &[(10, 9000)]));
        assert_eq!(m.rows()[0].tab_id, 11);
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
        assert_eq!(m.rows()[0].tab_id, 12);
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

    /// A model with `bar_ids` known as plugin panes, toggled hide+show so
    /// repair is armed for all of them.
    fn armed(bar_ids: &[u32]) -> BarModel {
        let mut m = BarModel::default();
        m.apply_panes(
            bar_ids
                .iter()
                .enumerate()
                .map(|(i, id)| pane(i, *id, true, i == 0))
                .collect(),
        );
        m.toggle(); // hidden — no repair while hidden
        m.toggle(); // shown → arms repair for every known bar pane
        m
    }

    #[test]
    fn repair_converges_to_template_width_despite_coarse_steps() {
        // C6 finding: zellij re-INSERTS a shown pane (50% split, wrong
        // side) instead of restoring its layout geometry — the bar repairs
        // itself. Round-9 refinement: zellij resizes in ~5%-of-viewport
        // steps (~14 cols on a wide screen), so "shrink while > 26"
        // overshot straight through the target (27 → 13 → disarm). The
        // repair must LEARN the step from its own observed effect and stop
        // within half a step of the template width.
        let mut m = BarModel::default();
        m.apply_panes(vec![pane(0, 7, true, true)]);
        // Never armed: geometry is the user's business.
        assert_eq!(m.repair_tick(&[(7, Some(40), 140)]), Vec::<Effect>::new());
        let mut m = armed(&[7]);
        // Wrong side first: move left until x == 0.
        assert_eq!(
            m.repair_tick(&[(7, Some(40), 140)]),
            vec![Effect::MoveBarLeft { pane_id: 7 }]
        );
        // Simulate zellij honouring each resize with a 14-col step.
        let mut cols: i64 = 140;
        let mut acted = 0;
        loop {
            match m.repair_tick(&[(7, Some(0), cols as usize)]).as_slice() {
                [Effect::ShrinkBar { pane_id: 7 }] => cols -= 14,
                [Effect::GrowBar { pane_id: 7 }] => cols += 14,
                [] => break,
                other => panic!("unexpected repair effects: {other:?}"),
            }
            acted += 1;
            assert!(acted < 20, "did not converge");
        }
        // Converged within half a step of the 26-col template — and STAYS
        // retired (manual resizes are sacred once geometry is right).
        assert!((cols - 26).abs() <= 7, "ended at {cols} cols");
        assert_eq!(m.repair_tick(&[(7, Some(40), 140)]), Vec::<Effect>::new());
    }

    #[test]
    fn repair_grows_back_from_an_overshoot_and_waits_for_effects() {
        // The exact live defect: a previous repair overshot to 13 cols.
        let mut m = armed(&[7]);
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 13)]),
            vec![Effect::GrowBar { pane_id: 7 }]
        );
        // Same cols again = our resize hasn't landed yet: WAIT, don't
        // double-fire (in-flight actions were the round-9 overshoot risk).
        assert_eq!(m.repair_tick(&[(7, Some(0), 13)]), Vec::<Effect>::new());
        // Resize landed (+14 → 27): learned step 14, |27−26| within half a
        // step → accept and retire.
        assert_eq!(m.repair_tick(&[(7, Some(0), 27)]), Vec::<Effect>::new());
        assert_eq!(m.repair_tick(&[(7, Some(0), 13)]), Vec::<Effect>::new()); // retired
    }

    #[test]
    fn repair_at_correct_width_still_waits_to_confirm_left_edge() {
        // Round-15 live defect (every-other-show breakage): zellij
        // sometimes re-inserts the shown pane at its REMEMBERED width but
        // on the wrong side. The width phase (render, x unknowable) saw
        // "width fine" and zeroed the budget — the move phase never ran,
        // leaving a correct-width bar stranded on the right. Width-accept
        // may only retire the pane once a manifest has confirmed x == 0.
        let mut m = armed(&[7]);
        // Render arrives first: width already acceptable, but x is
        // unconfirmed — no action, and repair must STAY armed.
        assert_eq!(m.repair_tick(&[(7, None, 29)]), Vec::<Effect>::new());
        // Manifest lands: wrong side → move left.
        assert_eq!(
            m.repair_tick(&[(7, Some(96), 29)]),
            vec![Effect::MoveBarLeft { pane_id: 7 }]
        );
        // Move landed at the left edge: width ok + x confirmed → retire.
        assert_eq!(m.repair_tick(&[(7, Some(0), 29)]), Vec::<Effect>::new());
        // Retired for good: later geometry is the user's business.
        assert_eq!(m.repair_tick(&[(7, Some(40), 140)]), Vec::<Effect>::new());
    }

    #[test]
    fn repair_relearns_step_per_show_instead_of_across_hides() {
        // Round-15 companion defect: last_cols survived across hide/show,
        // so the FIRST width call of a new show "learned" the insert jump
        // (|stale last − insert width|) as a step size — a huge bogus step
        // (57 seen live) whose half-step acceptance band can swallow a
        // genuinely wrong width. Arming resets the compare base (the
        // learned step is kept — zellij's increment doesn't change).
        let mut m = armed(&[7]);
        // Converge a first show ending with last=Some(95) on the books.
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 109)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 95)]),
            vec![Effect::ShrinkBar { pane_id: 7 }] // step=14 learned
        );
        m.toggle();
        m.toggle(); // second show: fresh compare base
        // 40 cols is far off-template; a stale step of |95−40|=55 would
        // fake-accept it (2·14 ≤ 55). Fresh state must keep shrinking.
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 40)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
    }

    #[test]
    fn repair_width_chains_from_renders_within_one_visit() {
        // Round-10 quirk: zellij sends no PaneUpdate for the plugin's own
        // resize's effect, so a PaneUpdate-only loop advanced ONE step per
        // tab VISIT (bars stepped down once per Alt+↓ landing). Renders DO
        // follow each resize (the pane repaints with new cols) — width
        // repair chains off render cols (x unknown there → None),
        // converging within a single visit. Round 18: the chain may only
        // start once a manifest has confirmed the left edge.
        let mut m = armed(&[7]);
        assert_eq!(m.repair_tick(&[(7, Some(0), 140)]).len(), 1); // x confirmed
        let mut cols = 126i64;
        let mut acted = 0;
        loop {
            match m.repair_tick(&[(7, None, cols as usize)]).as_slice() {
                [Effect::ShrinkBar { pane_id: 7 }] => cols -= 14,
                [Effect::GrowBar { pane_id: 7 }] => cols += 14,
                [] => break,
                other => panic!("unexpected repair effects: {other:?}"),
            }
            acted += 1;
            assert!(acted < 20, "did not converge");
        }
        assert!((cols - 26).abs() <= 7, "ended at {cols} cols");
        // Width-only repair never moves the pane (x is unknowable in
        // render); the move arm needs a manifest with real x.
        let mut m2 = armed(&[7]);
        assert_eq!(
            m2.repair_tick(&[(7, Some(40), 140)]),
            vec![Effect::MoveBarLeft { pane_id: 7 }]
        );
    }

    #[test]
    fn repair_budget_caps_a_layout_that_never_converges() {
        let mut m = armed(&[7]);
        // A pathological layout that thrashes (cols change but never reach
        // the target) must not be fought forever — each step is a real
        // zellij layout action.
        let steps = (0..64)
            .map(|i| m.repair_tick(&[(7, Some(0), if i % 2 == 0 { 100 } else { 86 })]))
            .take_while(|fx| !fx.is_empty())
            .count();
        assert!(steps <= 16, "unbounded repair: {steps} steps");
    }

    #[test]
    fn repair_retry_tick_refires_a_clobbered_resize_events_never_do() {
        // Round-16/17 live defect: the toggle-show unsuppress BURST
        // relayouts every tab, clobbering resizes fired mid-burst back to
        // 50%. The in-flight guard then waited forever for a cols change
        // that was already reverted. Counting event observations as the
        // retry signal failed live: renders tick every ~1ms during the
        // burst, so the "retry" double-fired before zellij applied the
        // FIRST resize (budget 15→13 in 4ms in the round-16 trace).
        // Retries are therefore TIME-paced: only repair_retry_tick (main's
        // 400ms zellij timer) clears the in-flight guard and re-fires.
        let mut m = armed(&[7]);
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 75)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
        // Event ticks NEVER re-fire while cols are unchanged, no matter
        // how many arrive (they can be milliseconds apart).
        for _ in 0..10 {
            assert_eq!(m.repair_tick(&[(7, Some(0), 75)]), Vec::<Effect>::new());
        }
        // The paced timer tick presumes the fire lost and re-issues it.
        assert_eq!(
            m.repair_retry_tick(&[(7, Some(0), 75)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
        // Once a resize lands, convergence proceeds as usual.
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 61)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
    }

    #[test]
    fn repair_never_learns_a_relayout_jump_as_the_step_size() {
        // Round-17 live defect: step=60 in the trace. The learner
        // attributed ANY observed delta to "zellij's resize increment" —
        // including relayout jumps and double-landed fires (75→15). A
        // poisoned step of 60 makes the half-step acceptance band ±30,
        // which happily accepts a 13-col bar (the "too narrow" report) —
        // and the step survives shows by design. Deltas beyond a plausible
        // single resize must not be learned.
        let mut m = armed(&[7]);
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 75)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
        // A relayout slammed the pane to 15 (delta 60): recover via grow,
        // but do NOT learn 60 as the step.
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 15)]),
            vec![Effect::GrowBar { pane_id: 7 }]
        );
        // 40 cols is far off-template. A learned step of 60 would accept
        // it (2·14 ≤ 60); the unpoisoned step must keep shrinking.
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 40)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
    }

    #[test]
    fn repair_never_resizes_before_the_left_edge_is_confirmed() {
        // Round-18 live defect (the "focused tab never heals"): renders
        // fired width resizes while the bar still sat on the RIGHT (x is
        // unknowable in render), and zellij's move is a geometry SWAP —
        // the landing MoveBarLeft handed the freshly-shrunk width to the
        // terminal and the terminal's half-screen width to the bar. The
        // executor's own tab has the fastest render chain, so it always
        // lost this race and pumped 75→30→75 until the budget died.
        let mut m = armed(&[7]);
        // Renders before any manifest: width must NOT fire.
        assert_eq!(m.repair_tick(&[(7, None, 75)]), Vec::<Effect>::new());
        assert_eq!(m.repair_tick(&[(7, None, 75)]), Vec::<Effect>::new());
        // Manifest: wrong side → move only.
        assert_eq!(
            m.repair_tick(&[(7, Some(75), 75)]),
            vec![Effect::MoveBarLeft { pane_id: 7 }]
        );
        // Manifest confirms the left edge → width may fire, same tick.
        assert_eq!(
            m.repair_tick(&[(7, Some(0), 75)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
        // From here renders chain the width phase as usual.
        assert_eq!(
            m.repair_tick(&[(7, None, 61)]),
            vec![Effect::ShrinkBar { pane_id: 7 }]
        );
    }

    #[test]
    fn repair_pauses_while_hidden() {
        // Round-18: between a quick hide/show pair the timer kept firing
        // at SUPPRESSED panes — no-ops that burned repair budget.
        let mut m = armed(&[7]);
        assert_eq!(
            m.repair_tick(&[(7, Some(75), 75)]),
            vec![Effect::MoveBarLeft { pane_id: 7 }]
        );
        m.toggle(); // hidden
        assert_eq!(m.repair_tick(&[(7, Some(75), 75)]), Vec::<Effect>::new());
        assert_eq!(
            m.repair_retry_tick(&[(7, Some(75), 75)]),
            Vec::<Effect>::new()
        );
        m.toggle(); // shown again → re-armed, repair resumes
        assert_eq!(
            m.repair_tick(&[(7, Some(75), 75)]),
            vec![Effect::MoveBarLeft { pane_id: 7 }]
        );
    }

    #[test]
    fn executor_heals_every_tabs_bar_in_one_pass() {
        // Round-16 live defect: repair was SELF-only, but hidden instances
        // get no events (C3) — every other tab's bar sat broken (right
        // side, wrong width) until the user visited it. One informed actor
        // (the executor) must heal ALL bars from its fresh manifest: pane
        // ids are global and the move/resize commands work cross-tab.
        let mut m = armed(&[7, 8, 9]);
        // One manifest tick: all three re-inserted wrong (right, 50%).
        let fx = m.repair_tick(&[
            (7, Some(96), 95),
            (8, Some(96), 95),
            (9, Some(96), 95),
        ]);
        assert_eq!(
            fx,
            vec![
                Effect::MoveBarLeft { pane_id: 7 },
                Effect::MoveBarLeft { pane_id: 8 },
                Effect::MoveBarLeft { pane_id: 9 },
            ]
        );
        // Panes progress independently: 7 landed left and converges by
        // width; 9 is still mid-move; 8's resize is in flight (same cols).
        let fx = m.repair_tick(&[(7, Some(0), 29), (8, Some(0), 95), (9, Some(48), 95)]);
        assert_eq!(
            fx,
            vec![
                Effect::ShrinkBar { pane_id: 8 },
                Effect::MoveBarLeft { pane_id: 9 },
            ]
        );
        // 7 retires only after x confirmed + width accepted — and a bar
        // pane in a tab created AFTER the toggle (id 99) is never touched.
        let fx = m.repair_tick(&[(7, Some(0), 29), (99, Some(96), 95)]);
        assert_eq!(fx, Vec::<Effect>::new());
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
            rows.iter().map(|r| r.tab_id).collect::<Vec<_>>(),
            vec![11, 12, 10]
        );
        assert_eq!(rows[0].glyph, Some(('●', 33))); // ag-a working
        assert_eq!(rows[1].glyph, Some(('●', 32))); // ag-b done
        assert_eq!(rows[2].glyph, None);
    }
}
