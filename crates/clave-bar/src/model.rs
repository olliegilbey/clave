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
    /// focus_pane_with_id(Terminal(pane_id)) — S2-proven nav.
    FocusPane { pane_id: u32 },
    /// run_command(["clave","focus",uuid]) — persist the unread clear.
    MarkRead { uuid: String },
}

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
    /// tab_id → logical time of last interaction. NOT wall time: agent
    /// last_interacted (unix s) and focus events (no clock in wasm we trust)
    /// only ever BUMP this counter, so the scales never mix.
    recency: BTreeMap<usize, u64>,
    clock: u64,
    /// Last label WE wrote per uuid — the rename loop-guard (§6.4). Renames
    /// fire on label CHANGE only, so user manual renames stick in between.
    renamed: BTreeMap<String, String>,
    /// uuid → last_interacted seen in the previous snapshot (bump detection).
    seen_interacted: BTreeMap<String, u64>,
    /// Local unread-override: Done agents we've already cleared on focus.
    /// Render-side only; `clave focus` persists the real transition.
    read_locally: BTreeSet<String>,
    /// Bar visibility (Alt+c). main.rs maps this to hide_self/show_self.
    pub hidden: bool,
}

impl BarModel {
    fn bump(&mut self, tab_id: usize) {
        self.clock += 1;
        self.recency.insert(tab_id, self.clock);
    }

    /// Which tab (by current position) holds this pane?
    fn tab_position_of_pane(&self, pane_id: u32) -> Option<usize> {
        self.panes
            .iter()
            .find(|p| p.pane_id == pane_id && !p.is_plugin)
            .map(|p| p.tab_position)
    }

    fn tab_at_position(&self, position: usize) -> Option<&TabMeta> {
        self.tabs.iter().find(|t| t.position == position)
    }

    /// The agent living in the tab at `position`, if any (uuid→pane→tab).
    fn agent_at_position(&self, position: usize) -> Option<&Agent> {
        self.agents.iter().find(|a| {
            self.uuid_to_pane
                .get(&a.uuid)
                .and_then(|p| self.tab_position_of_pane(*p))
                == Some(position)
        })
    }

    /// Click/nav target for a tab: its focused non-plugin pane, else the
    /// first non-plugin pane (a tab remembers its internal focus; a tab with
    /// only plugin panes has no sensible target → None).
    fn pane_for_position(&self, position: usize) -> Option<u32> {
        let in_tab = || {
            self.panes
                .iter()
                .filter(move |p| p.tab_position == position && !p.is_plugin)
        };
        in_tab()
            .find(|p| p.is_focused)
            .or_else(|| in_tab().next())
            .map(|p| p.pane_id)
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
        let mut effects = Vec::new();
        // Borrow-friendly pass: collect (uuid, last_interacted, status, label,
        // tab_id) first, then mutate.
        let views: Vec<(String, u64, Status, String, Option<usize>)> = self
            .agents
            .iter()
            .map(|a| {
                let tab_id = self
                    .uuid_to_pane
                    .get(&a.uuid)
                    .and_then(|p| self.tab_position_of_pane(*p))
                    .and_then(|pos| self.tab_at_position(pos))
                    .map(|t| t.tab_id);
                (
                    a.uuid.clone(),
                    a.last_interacted,
                    a.status,
                    a.label.clone(),
                    tab_id,
                )
            })
            .collect();
        for (uuid, interacted, status, label, tab_id) in views {
            // (b) recency bump when the agent's last_interacted advances.
            let prev = self.seen_interacted.insert(uuid.clone(), interacted);
            if let Some(tab_id) = tab_id {
                if prev.is_some_and(|p| interacted > p) || (prev.is_none() && interacted > 0) {
                    self.bump(tab_id);
                }
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

    /// Apply zellij's tab truth. Detects newly-active tabs for (a) recency
    /// and the §6.5 unread clear.
    pub fn apply_tabs(&mut self, tabs: Vec<TabMeta>) -> Vec<Effect> {
        let prev_active: Option<usize> = self.tabs.iter().find(|t| t.active).map(|t| t.tab_id);
        self.tabs = tabs;
        let mut effects = Vec::new();
        if let Some(now_active) = self.tabs.iter().find(|t| t.active) {
            let (tab_id, position) = (now_active.tab_id, now_active.position);
            if prev_active != Some(tab_id) {
                self.bump(tab_id);
                // Focused a Done agent → clear unread (render now, persist
                // via MarkRead). BTreeSet.insert returns false if present —
                // that's the exactly-once guard.
                if let Some(a) = self.agent_at_position(position) {
                    if a.status == Status::Done {
                        let uuid = a.uuid.clone();
                        if self.read_locally.insert(uuid.clone()) {
                            effects.push(Effect::MarkRead { uuid });
                        }
                    }
                }
            }
        }
        effects
    }

    pub fn apply_panes(&mut self, panes: Vec<PaneMeta>) {
        self.panes = panes;
    }

    /// Rows in display order: recency desc, then tab position asc (never-
    /// touched tabs all have recency 0 and sort by position — spec §6.6).
    pub fn rows(&self) -> Vec<Row> {
        let mut order: Vec<&TabMeta> = self.tabs.iter().collect();
        order.sort_by(|a, b| {
            let ra = self.recency.get(&a.tab_id).copied().unwrap_or(0);
            let rb = self.recency.get(&b.tab_id).copied().unwrap_or(0);
            rb.cmp(&ra).then(a.position.cmp(&b.position))
        });
        order
            .into_iter()
            .map(|t| {
                let glyph = self.agent_at_position(t.position).map(|a| {
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

    /// Mouse click on rendered line N (0-based) → focus that row's pane.
    pub fn click(&self, line: usize) -> Option<Effect> {
        let rows = self.rows();
        let row = rows.get(line)?;
        let position = self.tabs.iter().find(|t| t.tab_id == row.tab_id)?.position;
        self.pane_for_position(position)
            .map(|pane_id| Effect::FocusPane { pane_id })
    }

    /// clave-nav payloads: {"dir":"next"|"prev"} | {"row":N} | {"uuid":"…"}.
    /// dir walks DISPLAY order relative to the active row, wrapping; row is
    /// 1-based (Alt+1..9). Malformed payloads → None (caller logs).
    pub fn nav(&self, payload: &str) -> Option<Effect> {
        let v: serde_json::Value = serde_json::from_str(payload).ok()?;
        if let Some(uuid) = v.get("uuid").and_then(|u| u.as_str()) {
            let pane_id = *self.uuid_to_pane.get(uuid)?;
            return Some(Effect::FocusPane { pane_id });
        }
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let target_line = if let Some(n) = v.get("row").and_then(|n| n.as_u64()) {
            let idx = (n as usize).checked_sub(1)?; // 1-based → 0-based
            if idx >= rows.len() {
                return None;
            }
            idx
        } else {
            let dir = v.get("dir")?.as_str()?;
            let active = rows.iter().position(|r| r.active).unwrap_or(0);
            match dir {
                "next" => (active + 1) % rows.len(),
                "prev" => (active + rows.len() - 1) % rows.len(),
                _ => return None,
            }
        };
        self.click(target_line)
    }

    /// Alt+c. Returns the NEW hidden state; main.rs calls hide_self/show_self.
    pub fn toggle(&mut self) -> bool {
        self.hidden = !self.hidden;
        self.hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- tiny builders (Step 1) --------------------------------------------

    /// An agent whose label == uuid (other string fields blank).
    fn agent(uuid: &str, status: Status, last_interacted: u64) -> Agent {
        Agent {
            uuid: uuid.into(),
            cwd: String::new(),
            repo_root: String::new(),
            branch: String::new(),
            label: uuid.into(),
            status,
            last_interacted,
            last_visited: 0,
        }
    }

    /// An agent with an explicit label (Idle, never interacted).
    fn agent_labelled(uuid: &str, label: &str) -> Agent {
        Agent {
            uuid: uuid.into(),
            cwd: String::new(),
            repo_root: String::new(),
            branch: String::new(),
            label: label.into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
        }
    }

    fn snap(seq: u64, agents: Vec<Agent>) -> AgentSnapshot {
        AgentSnapshot { seq, agents }
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
    fn rows_are_recency_ordered_with_tab_order_tail() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(11, 1, "b", false),
            tab(12, 2, "c", false),
        ]);
        // Focus b, then c: recency c > b > (a untouched, clock 0).
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(11, 1, "b", true),
            tab(12, 2, "c", false),
        ]);
        m.apply_tabs(vec![
            tab(10, 0, "a", false),
            tab(11, 1, "b", false),
            tab(12, 2, "c", true),
        ]);
        let names: Vec<String> = m.rows().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["c", "b", "a"]);
        // Emergent §6.6 property: active tab is row 1 (Alt+2 ≈ alt-tab).
        assert!(m.rows()[0].active);
    }

    #[test]
    fn agent_rows_get_glyphs_plain_rows_do_not() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![
            tab(10, 0, "agent-tab", false),
            tab(11, 1, "plain", false),
        ]);
        m.apply_panes(vec![pane(0, 5, false, true), pane(1, 6, false, true)]);
        m.register("u1".into(), 5); // pane 5 lives in tab position 0
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Working, 100)]));
        let rows = m.rows();
        let a = rows.iter().find(|r| r.name == "agent-tab").unwrap();
        let p = rows.iter().find(|r| r.name == "plain").unwrap();
        assert_eq!(a.glyph, Some(('●', 33))); // Working = amber
        assert_eq!(p.glyph, None); // plain terminal: name only
    }

    #[test]
    fn snapshot_seq_gate_discards_stale() {
        let mut m = BarModel::default();
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, 100)]));
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Failed, 999)])); // stale
        m.apply_tabs(vec![tab(10, 0, "t", false)]);
        m.apply_panes(vec![pane(0, 5, false, true)]);
        m.register("u1".into(), 5);
        assert_eq!(m.rows()[0].glyph, Some(('●', 33))); // still Working
    }

    #[test]
    fn rename_only_when_label_changes_not_when_tab_name_differs() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "old-name", false)]);
        m.apply_panes(vec![pane(0, 5, false, true)]);
        m.register("u1".into(), 5);
        let fx = m.apply_snapshot(snap(1, vec![agent_labelled("u1", "x · main · fix auth")]));
        assert!(fx.contains(&Effect::RenameTab {
            tab_id: 10,
            name: "x · main · fix auth".into()
        }));
        // Same label again — even though the TAB name is still "old-name"
        // (e.g. the user manually renamed it), we do NOT re-rename.
        let fx = m.apply_snapshot(snap(2, vec![agent_labelled("u1", "x · main · fix auth")]));
        assert!(fx.iter().all(|e| !matches!(e, Effect::RenameTab { .. })));
        // A genuinely NEW label renames again.
        let fx = m.apply_snapshot(snap(
            3,
            vec![agent_labelled("u1", "x · main · Fix auth flow")],
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
        m.apply_panes(vec![pane(0, 5, false, true)]);
        m.register("u1".into(), 5);
        m.apply_snapshot(snap(1, vec![agent("u1", Status::Done, 100)]));
        assert_eq!(m.rows()[0].glyph, Some(('●', 32))); // green, unread
        // Tab gains focus → local clear + MarkRead effect, exactly once.
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.contains(&Effect::MarkRead { uuid: "u1".into() }));
        assert_eq!(m.rows()[0].glyph, Some(('●', 90))); // rendered dim NOW
        let fx = m.apply_tabs(vec![tab(10, 0, "t", true)]);
        assert!(fx.iter().all(|e| !matches!(e, Effect::MarkRead { .. })));
        // A later snapshot showing Working clears the local override.
        m.apply_snapshot(snap(2, vec![agent("u1", Status::Working, 200)]));
        assert_eq!(m.rows()[0].glyph, Some(('●', 33)));
    }

    #[test]
    fn click_and_nav_resolve_display_order_to_panes() {
        let mut m = BarModel::default();
        m.apply_tabs(vec![tab(10, 0, "a", true), tab(11, 1, "b", false)]);
        // Tab 0: plugin pane 1 (the bar) + terminal 5 (focused);
        // tab 1: terminal 6 (not marked focused → first-non-plugin fallback).
        m.apply_panes(vec![
            pane(0, 1, true, false),
            pane(0, 5, false, true),
            pane(1, 6, false, false),
        ]);
        // Display order: a (active, most recent) then b.
        assert_eq!(m.click(0), Some(Effect::FocusPane { pane_id: 5 }));
        assert_eq!(m.click(1), Some(Effect::FocusPane { pane_id: 6 }));
        assert_eq!(m.click(9), None); // below the list
        // Active row is 0 ("a") → next wraps forward to "b", prev wraps back.
        assert_eq!(
            m.nav("{\"dir\":\"next\"}"),
            Some(Effect::FocusPane { pane_id: 6 })
        );
        assert_eq!(
            m.nav("{\"dir\":\"prev\"}"),
            Some(Effect::FocusPane { pane_id: 6 })
        );
        assert_eq!(m.nav("{\"row\":1}"), Some(Effect::FocusPane { pane_id: 5 }));
        assert_eq!(m.nav("{\"row\":9}"), None);
        // S2's direct-uuid form still works.
        m.register("u1".into(), 6);
        assert_eq!(
            m.nav("{\"uuid\":\"u1\"}"),
            Some(Effect::FocusPane { pane_id: 6 })
        );
        assert_eq!(m.nav("not json"), None);
    }
}
