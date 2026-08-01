//! `clave ls` rendering — pure function so it's testable without a terminal.
//! Ordering matches the bar (§6.6 / S1): the commitment ordinal, tab-first.
//! Repo shown as a trailing column (grouping was deleted in the 2026-07-03 rev).
//!
//! `clave ls` shows only STORE rows, so plain terminal tabs never appear here —
//! the bar interleaves them and this listing cannot. It is also an exact order
//! oracle only for rows with unique, NON-ZERO ordinals: several rows at 0 is a
//! reachable state, and the bar breaks that tie on tab POSITION, which the CLI
//! is never given. Rows at 0, or sharing an ordinal, are "order unspecified
//! between them" — not a discrepancy. The live-validation SOP depends on this
//! distinction, so a mismatch is a real bug only when both rows carry distinct
//! non-zero ordinals.

use crate::store::{AgentRecord, Store};

pub fn render_ls(store: &Store) -> String {
    if store.agents.is_empty() {
        return "no agents\n".to_string();
    }
    let mut rows: Vec<_> = store.agents.values().collect();
    // The BAR's rule (§6.6 / S1): the commitment ordinal, tab-first — the tab's
    // ordinal while the row is bound and live, else the row's own. This is what
    // makes `clave ls` an oracle for the sidebar's agent-row order; the two
    // diverging is itself a diagnostic signature, and it can only be one if
    // they agree when nothing is broken. `last_interacted` stays as the
    // secondary key: display clock, no longer the order.
    let ord = |r: &&AgentRecord| {
        let tab = r
            .tab_id
            .and_then(|id| store.tab_order.get(&id))
            .copied()
            .unwrap_or(0);
        (tab.max(r.commit_ord), r.last_interacted)
    };
    rows.sort_by_key(|r| std::cmp::Reverse(ord(r)));
    let mut out = String::new();
    for r in rows {
        let (glyph, colour) = r.status.glyph();
        out.push_str(&format!(
            "\u{1b}[{colour}m{glyph}\u{1b}[0m {}  ({})\n",
            r.label, r.repo_root
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::lsview::render_ls;
    use crate::store::{AgentRecord, LabelSource, Store};
    use clave_types::Status;

    /// Fixture mirroring store.rs's `rec()` (Task 2) — a minimal valid row we
    /// then tweak per test.
    fn test_rec(uuid: &str) -> AgentRecord {
        AgentRecord {
            uuid: uuid.into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: uuid.into(),
            status: Status::Idle,
            last_interacted: 0,
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            live_session: None,
        }
    }

    #[test]
    fn ls_orders_by_commitment_ordinal_tab_first() {
        // S1 §4.5: `clave ls` is the read-only ORACLE the live-validation SOP
        // compares the sidebar against, so it has to sort by the bar's rule —
        // the commitment ordinal, tab-first. Before S1 it sorted on the wall
        // clock while claiming in its own doc comment to match the bar.
        //
        // The discriminating case: a LIVE bound row whose tab carries a high
        // ordinal, against a DORMANT row with a much later wall clock. Under
        // the old rule the dormant row won; under the bar's rule the live one
        // does.
        let mut s = Store::default();
        let mut live = test_rec("live");
        live.tab_id = Some(4);
        live.commit_ord = 1; // its own ordinal is stale…
        live.last_interacted = 100;
        let mut dormant = test_rec("dormant");
        dormant.commit_ord = 5;
        dormant.last_interacted = 999; // …and its clock is the newest here
        s.agents.insert("live".into(), live);
        s.agents.insert("dormant".into(), dormant);
        s.tab_order.insert(4, 9); // …but the TAB it is bound to is the freshest
        let lines: Vec<String> = render_ls(&s).lines().map(String::from).collect();
        assert!(lines[0].contains("live"), "tab ordinal must win: {lines:?}");
        assert!(lines[1].contains("dormant"));
        // Unbind it and the row falls back to its own ordinal, which loses.
        s.agents.get_mut("live").unwrap().tab_id = None;
        let lines: Vec<String> = render_ls(&s).lines().map(String::from).collect();
        assert!(lines[0].contains("dormant"), "own ordinal now rules: {lines:?}");
    }

    #[test]
    fn ls_sorts_by_recency_desc_and_shows_glyph() {
        let mut s = Store::default();
        let mut a = test_rec("old"); // helper like Task 2's rec()
        a.last_interacted = 100;
        a.commit_ord = 1;
        a.status = Status::Done;
        let mut b = test_rec("new");
        b.last_interacted = 200;
        b.commit_ord = 2;
        b.status = Status::Working;
        s.agents.insert("old".into(), a);
        s.agents.insert("new".into(), b);
        let out = render_ls(&s);
        let lines: Vec<&str> = out.lines().collect();
        // Most recently interacted first — same ordering rule as the bar.
        assert!(lines[0].contains("new"));
        assert!(lines[1].contains("old"));
        // Working = amber ● (ANSI 33), Done = green ● (ANSI 32).
        assert!(lines[0].contains("\u{1b}[33m●"));
        assert!(lines[1].contains("\u{1b}[32m●"));
    }

    #[test]
    fn ls_empty_store_says_so() {
        assert_eq!(render_ls(&Store::default()), "no agents\n");
    }
}
