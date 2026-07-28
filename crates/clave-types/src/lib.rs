//! Shared pipe schema between the `clave` binary and the `clave-bar` plugin.
//! serde-only and target-agnostic (compiles for host AND wasm) — this is the
//! anti-drift guarantee (invariant #9): both artifacts serialize the SAME
//! structs.

use serde::{Deserialize, Serialize};

/// The zellij plugin-configuration key carrying the absolute `clave` binary
/// the bar must invoke (#44).
///
/// Lives in the shared crate because BOTH sides must agree: `clave` emits it
/// into config.kdl's MessagePlugin keybinds and into every layout `plugin`
/// node, and `clave-bar` will read it at `load()`. Zellij matches a pipe's
/// destination on (location, configuration) EXACTLY
/// (zellij-server/src/plugins/wasm_bridge.rs:1676-1686), so a typo on one side
/// silently spawns a second bar instead of erroring.
pub const CLAVE_BINARY_KEY: &str = "clave_binary";

/// Per-agent status. This is a *latest-wins state machine* (spec §6.5), not a
/// priority-max: a later event can downgrade an earlier one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Working,
    NeedsYou,
    Done,
    Failed,
}

impl Status {
    /// The bar/ls glyph: one char whose FONT COLOUR encodes state (spec §6.5).
    /// Returned as (glyph, ANSI SGR colour code) so both artifacts render
    /// identically — raw ANSI SGR is proven to render in a plugin pane (S1).
    pub fn glyph(self) -> (char, u8) {
        match self {
            Status::NeedsYou => ('●', 31), // red: waiting on the human
            Status::Working => ('●', 33),  // amber: agent is running
            Status::Done => ('●', 32),     // green: finished & unread
            Status::Idle => ('●', 90),     // dim: read / no session
            Status::Failed => ('✖', 31),   // red cross: turn failed
        }
    }
}

/// One agent row as the plugin renders it. Mirrors the store record's
/// display-relevant fields (spec §5); the plugin never sees the store, only
/// this snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// Minted session UUID — the join key (invariant #3).
    pub uuid: String,
    /// Current working directory of the agent process.
    pub cwd: String,
    /// git toplevel of `cwd`; the grouping key in the bar.
    pub repo_root: String,
    /// The current git branch the agent is on.
    pub branch: String,
    /// `cwd · branch · summary` (spec §6.4).
    pub label: String,
    /// Per-agent status (latest-wins state machine, spec §6.5).
    pub status: Status,
    /// unix seconds; bumped on UserPromptSubmit → drives recency sort.
    pub last_interacted: u64,
    /// unix seconds; bumped on focus → `unread = done && !visited`.
    pub last_visited: u64,
    /// Zellij tab id hosting this agent (§6.6 Design B): bound ONCE by the
    /// agent tab's own bar instance (`clave bind`) so glyph joins and prompt
    /// stamps ride the snapshot — per-instance register/manifest joins
    /// diverge (round 6). Session-scoped; None until bound / after recreate.
    #[serde(default)]
    pub tab_id: Option<usize>,
    /// §5 (2026-07-17): `clave open` found the row's cwd missing → the bar
    /// renders ✗ instead of ◌. A row flag, NOT a status (statuses are hook
    /// lifecycle); cleared by a later successful open. `default` keeps
    /// pre-field payloads parseable.
    #[serde(default)]
    pub stale: bool,
    /// Claude's session rename (`custom-title` in the transcript) — the
    /// filled chip in design-lock §2's 7-column title field. `None` = never
    /// renamed, which is the majority of rows. Structural rather than parsed
    /// out of `label`: §7.1 rules the bar lays its own fixed-width columns
    /// and needs the VALUE, not a position inside a composed string.
    /// Populated by S4 (#59); `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub title: Option<String>,
    /// The words segment — design-lock §2's 17-column field, the widest on
    /// the row and the one actually read. Reachable today only by splitting
    /// `label`, which §7.1 forbids the bar from doing. Populated by S4 (#59),
    /// whose source tier is retargeted to `ai-title` because the
    /// `type:"summary"` tier is extinct (#79). `default` keeps pre-field
    /// payloads parseable.
    #[serde(default)]
    pub summary: String,
    /// Worktree path if `clave add --worktree` created one, else None — the
    /// input to S6's provenance glyph (#61). Held on `AgentRecord` since
    /// §6.3 and simply never projected until now. `Option<String>` not
    /// `bool`: #24 wants the worktree DIRECTORY NAME, which needs the path.
    /// `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub worktree: Option<String>,
}

/// The full-replace snapshot `clave` pushes to `clave-bar` on every change
/// (spec §5 pipe contract). `seq` is monotonic; a consumer applies only the
/// highest `seq` it has seen and discards stale/out-of-order messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub seq: u64,
    pub agents: Vec<Agent>,
    /// tab_id → unix seconds of the last user commitment to that tab (§6.6).
    /// Lives in the STORE and rides every snapshot as seq-gated full state:
    /// per-instance pipe-delta merges diverged live (C5 round 5) — the bar
    /// REPLACES its copy from this map, never merges. `default` keeps
    /// pre-field payloads parseable.
    #[serde(default)]
    pub tab_timeline: std::collections::BTreeMap<usize, u64>,
    /// Bar collapse mode (issue #5, C8 parity-desync family): per-instance
    /// memory synced only by the `clave-toggle` broadcast desynced live — a
    /// tab born after a toggle, a plugin reload, or one missed pipe flips an
    /// instance forever. Riding the seq-gated snapshot lets instances
    /// hydrate at birth and heal on every push from the one store writer.
    /// `default` (false = expanded) keeps pre-field payloads parseable and
    /// matches the born-expanded default.
    #[serde(default)]
    pub collapsed: bool,
}

/// The `clave-register` payload a pane's `clave spawn` pipes to the plugin so it
/// can map uuid → pane_id → live tab position (spec §6.1 / spike S2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Register {
    pub uuid: String,
    pub pane_id: u32,
}

/// The label segment separator: U+0020 U+00B7 U+0020. Only `store.rs`'s
/// backfill splits on it so far; the five composition sites in `add.rs` and
/// `hook.rs` still spell the separator literally, and converting them is
/// deliberately out of scope for #69 (a label-composition change is a render
/// change, and this branch is inert plumbing). Written as an escape, never a
/// literal: design-lock §5.4 (load-bearing) records that literal glyphs were
/// silently lost in transit twice, and the failure mode is tofu in production
/// from a clean-looking diff. S4 §4.1 and S5 §3.1 each proposed this constant independently —
/// it lands once, here (#69).
pub const LABEL_SEP: &str = " \u{00b7} ";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_as_spec_snake_case() {
        // Exactly the strings the spec (§5/§6.5) mandates.
        assert_eq!(serde_json::to_string(&Status::Idle).unwrap(), "\"idle\"");
        assert_eq!(
            serde_json::to_string(&Status::Working).unwrap(),
            "\"working\""
        );
        assert_eq!(
            serde_json::to_string(&Status::NeedsYou).unwrap(),
            "\"needs_you\""
        );
        assert_eq!(serde_json::to_string(&Status::Done).unwrap(), "\"done\"");
        assert_eq!(
            serde_json::to_string(&Status::Failed).unwrap(),
            "\"failed\""
        );
    }

    #[test]
    fn status_deserializes_from_snake_case() {
        let s: Status = serde_json::from_str("\"needs_you\"").unwrap();
        assert_eq!(s, Status::NeedsYou);
    }

    #[test]
    fn status_glyph_encodes_state_colour() {
        // Spec §6.5 glyph table — single source shared by the bar and `clave ls`.
        assert_eq!(Status::NeedsYou.glyph(), ('●', 31)); // red
        assert_eq!(Status::Working.glyph(), ('●', 33)); // amber
        assert_eq!(Status::Done.glyph(), ('●', 32)); // green (done & unread)
        assert_eq!(Status::Idle.glyph(), ('●', 90)); // dim
        assert_eq!(Status::Failed.glyph(), ('✖', 31)); // red cross
    }

    #[test]
    fn status_roundtrips_every_variant() {
        // Exhaustive BOTH ways (the old deserialize test only covered needs_you).
        for (v, s) in [
            (Status::Idle, "\"idle\""),
            (Status::Working, "\"working\""),
            (Status::NeedsYou, "\"needs_you\""),
            (Status::Done, "\"done\""),
            (Status::Failed, "\"failed\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), s);
            assert_eq!(serde_json::from_str::<Status>(s).unwrap(), v);
        }
    }

    #[test]
    fn agent_json_has_no_archived_field() {
        // §6.7 deleted archiving; the pipe schema must not carry the field.
        let a = Agent {
            uuid: "u1".into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
            tab_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
        };
        assert!(!serde_json::to_string(&a).unwrap().contains("archived"));
    }

    #[test]
    fn snapshot_roundtrips() {
        let snap = AgentSnapshot {
            seq: 7,
            tab_timeline: Default::default(),
            collapsed: false,
            agents: vec![Agent {
                uuid: "u1".into(),
                cwd: "/Users/x/code/clave".into(),
                repo_root: "/Users/x/code/clave".into(),
                branch: "main".into(),
                label: "clave · main · hello".into(),
                status: Status::Working,
                last_interacted: 1000,
                last_visited: 0,
                tab_id: None,
                stale: false,
                title: None,
                summary: String::new(),
                worktree: None,
            }],
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: AgentSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, back);
    }

    #[test]
    fn agent_tab_id_roundtrips_and_defaults_none() {
        // §6.6 Design B: the store binds uuid→tab_id (reported once by the
        // agent tab's own bar) so ordering stamps AND glyph joins ride the
        // snapshot instead of per-instance register/manifest joins — the
        // third divergence channel found in round 6.
        let mut a = Agent {
            uuid: "u1".into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
            tab_id: Some(4),
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
        };
        let back: Agent = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back.tab_id, Some(4));
        // Pre-field payloads must parse as unbound.
        a.tab_id = None;
        let mut v: serde_json::Value = serde_json::to_value(&a).unwrap();
        v.as_object_mut().unwrap().remove("tab_id");
        let old: Agent = serde_json::from_value(v).unwrap();
        assert_eq!(old.tab_id, None);
    }

    #[test]
    fn agent_stale_roundtrips_and_defaults_false() {
        // §5 (2026-07-17): `stale` = `clave open` found the row's cwd missing →
        // bar ✗. A row flag, NOT a status (statuses are hook lifecycle).
        let mut a = Agent {
            uuid: "u1".into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
            tab_id: None,
            stale: true,
            title: None,
            summary: String::new(),
            worktree: None,
        };
        let back: Agent = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert!(back.stale);
        // Pre-field payloads must parse as not-stale.
        a.stale = false;
        let mut v: serde_json::Value = serde_json::to_value(&a).unwrap();
        v.as_object_mut().unwrap().remove("stale");
        let old: Agent = serde_json::from_value(v).unwrap();
        assert!(!old.stale);
    }

    #[test]
    fn agent_title_summary_worktree_roundtrip_and_default() {
        // Design-lock §7.1: a live row renders from the STORE, so the bar
        // needs the VALUES for its fixed-width title/repo/summary columns —
        // not positions inside the composed `label`. That ruling deleted
        // InkSpan and made these three structural (#69).
        let mut a = Agent {
            uuid: "u1".into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x \u{00b7} main".into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
            tab_id: None,
            stale: false,
            title: Some("CLA-MAIN".into()),
            summary: "fix the flaky auth".into(),
            worktree: Some("/x/.claude/worktrees/wt".into()),
        };
        let back: Agent = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back.title.as_deref(), Some("CLA-MAIN"));
        assert_eq!(back.summary, "fix the flaky auth");
        assert_eq!(back.worktree.as_deref(), Some("/x/.claude/worktrees/wt"));

        // A v1 payload carries none of the three keys and MUST still parse —
        // the CLI and the wasm bar upgrade at different moments (a running
        // session keeps the bar it loaded), so this is a live state.
        a.title = None;
        a.summary = String::new();
        a.worktree = None;
        let mut v: serde_json::Value = serde_json::to_value(&a).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("title");
        o.remove("summary");
        o.remove("worktree");
        let old: Agent = serde_json::from_value(v).unwrap();
        assert_eq!(old.title, None);
        assert!(old.summary.is_empty());
        assert_eq!(old.worktree, None);
    }

    #[test]
    fn snapshot_carries_tab_timeline_and_defaults_empty() {
        // §6.6 store-timeline: row order ships IN the snapshot — seq-gated
        // full-state replace, the one channel that never diverged (C5 rd 5:
        // fire-and-forget pipe deltas diverged per instance).
        let snap = AgentSnapshot {
            seq: 1,
            agents: vec![],
            tab_timeline: std::collections::BTreeMap::from([(4usize, 1700u64)]),
            collapsed: false,
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: AgentSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tab_timeline.get(&4), Some(&1700));
        // Pre-field payloads (old store hydration) must still parse.
        let old: AgentSnapshot = serde_json::from_str("{\"seq\":1,\"agents\":[]}").unwrap();
        assert!(old.tab_timeline.is_empty());
    }

    #[test]
    fn snapshot_carries_collapsed_and_defaults_false() {
        // Issue #5 (C8 parity-desync): the collapse mode rides the snapshot
        // so instances hydrate/heal from the store writer. An old-wire
        // payload without the field must parse as expanded (false) — the
        // born-expanded default — for old-CLI/new-plugin interop.
        let snap = AgentSnapshot {
            seq: 2,
            agents: vec![],
            tab_timeline: Default::default(),
            collapsed: true,
        };
        let back: AgentSnapshot =
            serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
        assert!(back.collapsed);
        let old: AgentSnapshot = serde_json::from_str("{\"seq\":1,\"agents\":[]}").unwrap();
        assert!(!old.collapsed);
    }

    #[test]
    fn register_roundtrips() {
        let reg = Register {
            uuid: "u1".into(),
            pane_id: 42,
        };
        let back: Register = serde_json::from_str(&serde_json::to_string(&reg).unwrap()).unwrap();
        assert_eq!(reg, back);
    }
}
