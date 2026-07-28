//! `clave ls` rendering — pure function so it's testable without a terminal.
//! Ordering matches the bar (§6.6): interaction recency, newest first. Repo
//! shown as a trailing column (grouping was deleted in the 2026-07-03 rev).

use crate::store::Store;

pub fn render_ls(store: &Store) -> String {
    if store.agents.is_empty() {
        return "no agents\n".to_string();
    }
    let mut rows: Vec<_> = store.agents.values().collect();
    // Same rule the bar uses: most recently interacted first; stable
    // uuid tiebreak (BTreeMap iteration is already uuid-sorted).
    rows.sort_by_key(|r| std::cmp::Reverse(r.last_interacted));
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
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            stale: false,
            title: None,
            summary: String::new(),
        }
    }

    #[test]
    fn ls_sorts_by_recency_desc_and_shows_glyph() {
        let mut s = Store::default();
        let mut a = test_rec("old"); // helper like Task 2's rec()
        a.last_interacted = 100;
        a.status = Status::Done;
        let mut b = test_rec("new");
        b.last_interacted = 200;
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
