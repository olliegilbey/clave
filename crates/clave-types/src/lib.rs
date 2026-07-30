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
    /// The REPOSITORY's default branch — the branch a plain checkout of it sits
    /// on. The second input to S6's provenance glyph (#61): design-lock §5.1
    /// says the default checkout renders NOTHING, and §5's whole argument is
    /// that blanking the most common row is what makes the two marked states
    /// mean something. The bar used to decide that by name, treating `main` and
    /// `master` as exhaustive — so an ordinary default checkout of a repo whose
    /// default is `trunk`, `develop` or `dev` got the branch glyph, mislabelled
    /// on naming convention alone (#86). Resolved by the host, where git can
    /// actually be asked (`clave/src/add.rs::resolve_default_branch`).
    ///
    /// `None` is a real answer, not a failure: a repo with no remote may have no
    /// discoverable default, and the bar falls back to its `main`/`master`
    /// heuristic there so behaviour is never WORSE than before this field
    /// existed. `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub default_branch: Option<String>,
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
/// from a clean-looking diff. S4 §4.1 and S5 §3.1 each proposed this constant
/// independently — it lands once, here (#69).
pub const LABEL_SEP: &str = " \u{00b7} ";

// ── sidebar geometry ────────────────────────────────────────────────────────
//
// S8 §3.3, deferred there and taken here (#86): ONE definition per number.
// Three artifacts have to agree about how wide the bar is — `clave-bar`'s width
// seek drives the pane to it, `clave-bar`'s renderer lays every column out
// against it, and `clave`'s three KDL generators size the newborn pane as a
// PERCENT of it — and until now each held its own copy with nothing linking
// them. The very next task moves the expanded target (LEDGER D19), and moving
// `BAR_TARGET_COLS` alone used to leave every golden, the preview and the
// scenario render test green while pinning the OLD width.
//
// Here rather than in either crate because `clave-types` is already a
// dependency of both and already compiles to wasm; a compile-time constant
// rather than configuration because there is no code path in which a running
// instance's target changes (S8 §3.2 rejects all three config channels — each
// is the #43/#44 mixed-artifact shape).

/// The expanded width the bar is drawn at and the width seek converges to
/// (LEDGER D2, then D19): `1 cap + 8 gutter + 9 title + 1 + 7 repo + 1 +
/// 25 summary + 1 margin + 1 cap`. The renderer takes `cols` as a parameter —
/// zellij hands the plugin whatever the pane actually is — but every number in
/// the ratified design was chosen against this one.
///
/// **44 → 54 (LEDGER D19), Ollie's call after running the fleet live:** *"mostly
/// for the summary, but could do another two chars for the title."* So title
/// takes 7 → 9 and summary takes the remaining 17 → 25. Collapsed is unchanged,
/// which widens the separation from 14 to 24 — see the separation test below,
/// where that change retires a whole class of seek behaviour.
///
/// At a genuinely 80-column session this leaves the agent pane 26 columns.
/// Accepted (D32): few sessions are that narrow, and collapsed still leaves 50.
pub const BAR_TARGET_COLS: usize = 54;

/// The collapsed width (Alt+c), LEDGER D17: `30 - 13 - 7 title - 3 repo`
/// leaves the summary 7. Collapsed is a width PROFILE, not a squeezed layout
/// (D16) — the gutter is identical and only repo and summary narrow, through
/// the same `render_rows`.
///
/// Its distance from `BAR_TARGET_COLS` is load-bearing beyond the two renders:
/// the seek's acceptance bands must not overlap, or a collapse is accepted as
/// an expand (LEDGER D21, `clave-bar`'s `converged`).
pub const COLLAPSED_TARGET_COLS: usize = 30;
const _: () = assert!(
    COLLAPSED_TARGET_COLS < BAR_TARGET_COLS,
    "collapsed must be the NARROWER profile: the seek, the width profiles and \
     Alt+c's direction all read it that way"
);

/// The reference viewport the birth percent is derived against (S8 §3.4). A
/// documented fiction: real windows vary, and the birth-armed seek corrects the
/// difference. It exists so the percent is a DERIVATION rather than a number
/// somebody chose.
pub const REFERENCE_VIEWPORT_COLS: usize = 200;

/// The `size="N%"` every generated bar pane is born at.
///
/// It MUST be a percent: a fixed `size=30` makes zellij refuse every resize on
/// the pane (`CantResizeFixedPanes`), which left Alt+c dead in any freshly
/// launched session (c8-cold-start 2026-07-18). So this is a birth HINT and the
/// seek is the authority — a stale `clave` on `PATH` (#44) emitting last
/// version's percent costs a visible flicker at birth, not a wrong bar, and
/// that one-way geometry contract is why the seam cannot desync.
///
/// Computed, not hand-derived: the literal used to live in three format
/// strings, and round 21 had to remember to touch each one by hand.
pub const BAR_BIRTH_PERCENT: usize = BAR_TARGET_COLS * 100 / REFERENCE_VIEWPORT_COLS;

#[cfg(test)]
mod tests {
    use super::*;

    /// The one place the birth percent's VALUE is checked. Every other site
    /// (three KDL generators and the test that reads them back) formats this
    /// constant, so a target move carries the percent with it — the skew S8
    /// §3.3 was written to remove, and the one round 21 had to fix by hand.
    #[test]
    fn birth_percent_is_derived_from_the_bar_target() {
        // 54 * 100 / 200. Derived, not chosen — it was 22 at a 44 target.
        assert_eq!(BAR_BIRTH_PERCENT, 27);
        assert_eq!(
            BAR_BIRTH_PERCENT,
            BAR_TARGET_COLS * 100 / REFERENCE_VIEWPORT_COLS
        );
        // Percent sizing is not a style choice: a fixed `size=N` makes zellij
        // refuse every resize on the pane, so the newborn width must be
        // expressible as a whole percent of a real viewport.
        assert!((1..=100).contains(&BAR_BIRTH_PERCENT));
    }

    /// The seek's two targets, and the property that is not local to either:
    /// their separation. `clave-bar`'s `converged` refuses any width both
    /// bands would accept, so the geometry cannot silently make Alt+c a no-op
    /// again (LEDGER D21). Fail here if it changes.
    ///
    /// **24 since D19, and the number crossed a threshold on the way.** At
    /// 44/30 the separation was 14 — at or below the widest learnable step
    /// (`MAX_LEARNABLE_STEP` = 20), so the two acceptance bands could overlap
    /// and every toggle on a wide display paid a disambiguating step. At 54/30
    /// the separation EXCEEDS the widest step the seek can learn, so the bands
    /// can no longer overlap at all: the disqualification, the bracket rule and
    /// both resting-width costs become unreachable paths rather than live ones
    /// (D26's four inherited reservations, three of which this retires).
    #[test]
    fn the_collapsed_target_is_narrower_and_separated_from_the_expanded_one() {
        assert_eq!(BAR_TARGET_COLS.abs_diff(COLLAPSED_TARGET_COLS), 24);
    }

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
            default_branch: None,
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
                default_branch: None,
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
            default_branch: None,
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
            default_branch: None,
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
            default_branch: None,
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
    fn agent_default_branch_roundtrips_and_defaults_none() {
        // #86: provenance may not decide "this is the default checkout" from a
        // hardcoded `main`/`master` list — a `trunk`-default repo's ordinary
        // checkout would take the branch glyph. The host resolves the repo's
        // real default and it rides the wire; `None` means "not discoverable",
        // which is a legitimate answer for a repo with no remote and is exactly
        // what an OLD payload deserializes to.
        let mut a = Agent {
            uuid: "u1".into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "trunk".into(),
            label: "x \u{00b7} trunk".into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
            tab_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: Some("trunk".into()),
        };
        let back: Agent = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back.default_branch.as_deref(), Some("trunk"));

        // The v2 payload (#69's shape) has no such key and MUST still parse —
        // the CLI and the wasm bar upgrade at different moments, so a new bar
        // reading an old push is a live state, not a migration.
        a.default_branch = None;
        let mut v: serde_json::Value = serde_json::to_value(&a).unwrap();
        v.as_object_mut().unwrap().remove("default_branch");
        let old: Agent = serde_json::from_value(v).unwrap();
        assert_eq!(old.default_branch, None);
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
