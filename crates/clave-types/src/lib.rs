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

/// Env var carrying the agent's store join key across the exec into Claude
/// (#97). `clave spawn` sets it; `clave hook` reads it when the hook payload's
/// `session_id` is not a row clave knows — which is what a resumed session
/// looks like, because Claude mints a new id and starts a new transcript.
///
/// Shared here for the same reason as [`CLAVE_BINARY_KEY`]: two commands in
/// two different processes have to agree on the spelling, and a typo would be
/// silent — the hook would simply go on declining every rotated session, which
/// is exactly the pre-fix behaviour and therefore invisible.
pub const AGENT_UUID_ENV: &str = "CLAVE_AGENT_UUID";

/// Env var naming the agent's SMART ZONE: how many tokens of context this user
/// trusts a model to stay sharp within (S7, #62). ONE global number — it is a
/// property of the user, not of the model, and the same figure holds across a
/// 200k window, a 1M window, or a future non-Claude agent.
///
/// Deliberately NOT the model's real context window, and #62 rejected detecting
/// that outright — `CLAUDE_CODE_DISABLE_1M_CONTEXT`, the 200k/1m threshold
/// tables, the lot. The real ceiling is where Claude auto-compacts, which is not
/// a thing anyone steers by; inferring it would be a guess in service of a
/// number the user does not care about.
///
/// This is the point at which the battery turns RED, not the point at which its
/// ramp ends: past it the reading clamps.
pub const SMART_ZONE_ENV: &str = "CLAVE_AGENT_SMART_ZONE_TOKENS";

/// Default for [`SMART_ZONE_ENV`] — the cap people settle on regardless of the
/// window their model advertises (maintainer, #62).
pub const DEFAULT_SMART_ZONE_TOKENS: u32 = 150_000;

/// Fill steps in the S7 battery ramp: full, nine tenths … one tenth, empty.
///
/// Eleven because that is what a patched Nerd Font's Material Design battery
/// family actually provides — `md-battery`, `md-battery_10` … `md-battery_90`,
/// `md-battery_outline` — verified by parsing the installed font's glyph-name
/// table rather than assumed (#62). Shared here for the same reason as
/// [`BAR_TARGET_COLS`]: the host buckets against it and the renderer's table
/// must be exactly this long, and nothing else links the two numbers.
pub const BATTERY_LEVELS: u8 = 11;

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

/// How the bar ranks rows (2026-08-19 spec). Semi-persistent store state
/// riding every snapshot — the `collapsed` doctrine: one store writer,
/// instances hydrate at birth and heal on every push.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderMode {
    /// The shipped S1 ordering: commitment ordinal descending.
    Recency,
    /// Decayed-commitment score: Σ count × 0.5^(age_days × 24 / half_life).
    /// half-life → 0 behaves like Recency; → ∞ like a 7-day rolling
    /// investment count ([`BUCKET_RETAIN_DAYS`] caps both sides).
    Frecency { half_life_hours: u32 },
}

impl Default for OrderMode {
    fn default() -> Self {
        OrderMode::Frecency {
            half_life_hours: 24,
        }
    }
}

/// The day-bucket window, shared by both sides of the wire (maintainer
/// ruling, 2026-08-19 post-drive): the store prunes a row's buckets past
/// this on every bump, and the bar scores an out-of-window bucket as ZERO
/// at every dial — "fully decayed at 7 days" is a semantic, not a numeric
/// accident of the half-life, and it caps per-frame scoring work however
/// stale a long-dormant row's stored buckets are.
pub const BUCKET_RETAIN_DAYS: u32 = 7;

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
    /// unix seconds; bumped on UserPromptSubmit. DISPLAY and cross-session
    /// policy only (`clave ls`, the picker, eager-launch selection) — it is
    /// NOT the bar's sort key (S1/#39: whole seconds tie, and ties resolved
    /// on tab position, so the wrong row won).
    pub last_interacted: u64,
    /// §6.6 commitment ORDINAL: the store `seq` of this row's last user
    /// commitment (a prompt, its creation, or the ordinal inherited from its
    /// tab when that tab closed). Minted under the store flock, so it is a
    /// total order with no clock and no ties. 0 = never committed → bottom.
    #[serde(default)]
    pub commit_ord: u64,
    /// unix seconds; bumped on focus → `unread = done && !visited`.
    pub last_visited: u64,
    /// Zellij tab id hosting this agent (§6.6 Design B): bound ONCE by the
    /// agent tab's own bar instance (`clave bind`) so glyph joins and prompt
    /// stamps ride the snapshot — per-instance register/manifest joins
    /// diverge (round 6). Session-scoped; None until bound / after recreate.
    #[serde(default)]
    pub tab_id: Option<usize>,
    /// Zellij terminal pane id hosting this agent (#178) — the wire twin of
    /// `AgentRecord::pane_id`, which carries the rationale. Without it a
    /// sidebar born after the `clave-register` broadcast can never learn which
    /// pane its own row owns, so the bind is uncomputable in every instance.
    #[serde(default)]
    pub pane_id: Option<u32>,
    /// §5 (2026-07-17): `clave open` found the row's cwd missing → the bar
    /// renders ✗ instead of ○. A row flag, NOT a status (statuses are hook
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
    /// Tokens the row's conversation is currently holding — the newest assistant
    /// turn's input, cache-read and cache-creation counts summed, read from the
    /// transcript tail the hook already takes for `title`/`summary` (S7, #62).
    ///
    /// The RAW figure rides the wire alongside the bucketed level because the
    /// expanded view renders it as text (#105); eleven glyphs can only ever
    /// approximate it. `None` = no reading yet, which renders blank — the bar
    /// never invents a measurement. `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub context_tokens: Option<u32>,
    /// Index into the bar's battery ramp: `0` is full, [`BATTERY_LEVELS`]` - 1`
    /// is empty and past the smart zone (S7, #62).
    ///
    /// Bucketed HOST-side, where [`SMART_ZONE_ENV`] is readable and where a
    /// future non-Claude agent can bring its own thresholds without touching the
    /// wasm; the bar renders an index and holds no opinion about tokens. Stamped
    /// when the row's own agent reports, so a dormant row costs nothing to
    /// project. `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub context_level: Option<u8>,
    /// Commitment day-buckets: unix day → count of user commitments that
    /// day. The frecency numerator; written by the hook on UserPromptSubmit,
    /// seeded at birth from the opener (spec: newborn initialisation),
    /// pruned past 7 days. `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub buckets: std::collections::BTreeMap<u32, u32>,
    /// The model handle this agent is running (e.g. "sonnet", "opus",
    /// "fable") — the double-height card's second row (#232). `None` = no
    /// reading yet, renders blank — the bar never invents a model name.
    /// `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub model: Option<String>,
    /// The provider running `model` above (e.g. "claude"). Kept separate
    /// from `model` rather than folded into one string: AGENTS.md commits
    /// clave to other CLI-based agents down the line, and a future provider
    /// may reuse a model NAME clave already knows under a different one.
    /// `None` = no reading yet. `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub provider: Option<String>,
    /// The open PR number for this row's branch, host-resolved (#232).
    /// `None` = no open PR found, or not looked up yet — the bar renders the
    /// same blank chip either way; the lookup's cache bookkeeping
    /// (`AgentRecord::pr_checked`/`pr_branch`) stays host-side and never
    /// rides the wire. `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub pr_number: Option<u32>,
}

/// The full-replace snapshot `clave` pushes to `clave-bar` on every change
/// (spec §5 pipe contract). `seq` is monotonic; a consumer applies only the
/// highest `seq` it has seen and discards stale/out-of-order messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub seq: u64,
    pub agents: Vec<Agent>,
    /// tab_id → the commitment ORDINAL of the last user commitment to that tab
    /// (§6.6 / S1). Lives in the STORE and rides every snapshot as seq-gated
    /// full state: per-instance pipe-delta merges diverged live (C5 round 5) —
    /// the bar REPLACES its copy from this map, never merges. `default` keeps
    /// pre-field payloads parseable.
    ///
    /// RENAMED from `tab_timeline`, whose values were unix seconds (S1 §3.6).
    /// The rename is what makes the upgrade safe: an old store file's
    /// `tab_timeline` is ignored rather than misread, so ~1.7e9-second values
    /// can never leak into the ordinal space and outrank every real ordinal.
    #[serde(default)]
    pub tab_order: std::collections::BTreeMap<usize, u64>,
    /// Bar collapse mode (issue #5, C8 parity-desync family): per-instance
    /// memory synced only by the `clave-toggle` broadcast desynced live — a
    /// tab born after a toggle, a plugin reload, or one missed pipe flips an
    /// instance forever. Riding the seq-gated snapshot lets instances
    /// hydrate at birth and heal on every push from the one store writer.
    /// `default` (false = expanded) keeps pre-field payloads parseable and
    /// matches the born-expanded default.
    #[serde(default)]
    pub collapsed: bool,
    /// Row-ordering mode + dial (2026-08-19 spec). Store state like
    /// `collapsed` above, same doctrine. `default` keeps pre-field
    /// payloads parseable.
    #[serde(default)]
    pub order: OrderMode,
    /// Unix DAY at projection time, stamped by the host — the bar never
    /// reads a clock (wasm). Frecency ages every bucket against this.
    /// `default` (0) carries no buckets (pre-frecency payload has none),
    /// so all scores are 0 → the ordinal fallback carries.
    #[serde(default)]
    pub today: u32,
    /// tab_id → commitment day-buckets: the tab-keyed twin of
    /// `Agent::buckets`, exactly as `tab_order` twins `commit_ord` —
    /// covers terminal tabs and the pre-bind window; session-scoped and
    /// pruned with `tab_order`. `default` keeps pre-field payloads
    /// parseable.
    #[serde(default)]
    pub tab_buckets: std::collections::BTreeMap<usize, std::collections::BTreeMap<u32, u32>>,
    /// tab_id → unix seconds of the last USER interaction with that tab —
    /// the wall-clock twin of `tab_order` above, which is ordinal-only and
    /// cannot say "how long ago" (#232's double-height card wants an age).
    /// Stamped in `touch_in` and in `apply_hook_event`'s commit path, so
    /// agent tabs and terminal tabs share one truth; pruned everywhere
    /// `tab_order` is pruned. `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub tab_touched: std::collections::BTreeMap<usize, u64>,
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
// Two artifacts have to agree about how wide the bar is — `clave-bar`'s renderer
// lays every column out against it, and `clave`'s KDL generators turn it into
// the percent both declared geometries are sized at — and until now each held
// its own copy with nothing linking them. Moving `BAR_TARGET_COLS` alone used to
// leave every golden, the preview and the scenario render test green while
// pinning the OLD width.
//
// Since #181 these are DESIGN widths, not runtime targets: nothing converges on
// them any more. They decide the column budget the rows are drawn to and the
// percent the layout declares; whatever zellij then resolves that percent to is
// what the renderer is handed, and `render_rows` clips (D31).
//
// Here rather than in either crate because `clave-types` is already a
// dependency of both and already compiles to wasm; a compile-time constant
// rather than configuration because there is no code path in which a running
// instance's width changes (S8 §3.2 rejects all three config channels — each is
// the #43/#44 mixed-artifact shape).

/// The expanded width the bar is drawn at, and the width its declared geometry
/// is sized to reach (LEDGER D2, then D19, then #105): `12 gutter + 9 title + 1 + 7 repo + 1 +
/// 22 summary + 1 margin + 1 cap`. The renderer takes `cols` as a parameter —
/// zellij hands the plugin whatever the pane actually is — but every number in
/// the ratified design was chosen against this one.
///
/// **44 → 54 (LEDGER D19), Ollie's call after running the fleet live:** *"mostly
/// for the summary, but could do another two chars for the title."* So title
/// takes 7 → 9 and summary takes the remaining 17 → 25. Collapsed is unchanged,
/// which widens the separation from 14 to 24.
///
/// **#105 then takes the gutter's battery cell to four columns** — the token
/// count, which the eleven-glyph ramp can only approximate — spent out of
/// `summary`, the only flex cell there is (D9): gutter 8 → 12, summary 25 →
/// 22. Collapsed keeps the one-column glyph, so its gutter is unchanged at 9.
///
/// At a genuinely 80-column session this leaves the agent pane 26 columns.
/// Accepted (D32): few sessions are that narrow, and collapsed still leaves 50.
pub const BAR_TARGET_COLS: usize = 54;

/// The collapsed width (Alt+c), LEDGER D17: `30 - 13 - 7 title - 3 repo`
/// leaves the summary 7. Collapsed is a width PROFILE, not a squeezed layout
/// (D16) — the gutter is identical to what BOTH profiles used before #105 (9),
/// and only repo and summary narrow relative to EXPANDED's post-#105 numbers,
/// through the same `render_rows`.
///
/// It must stay the NARROWER of the two, and since #181 that is load-bearing in
/// one more place: the bar accepts a geometry switch by checking the pane moved
/// in the direction the new mode wants, so an inverted pair would make every
/// collapse read as a failure and spend its one correction undoing itself.
pub const COLLAPSED_TARGET_COLS: usize = 30;
const _: () = assert!(
    COLLAPSED_TARGET_COLS < BAR_TARGET_COLS,
    "collapsed must be the NARROWER profile: the width profiles, the declared \
     geometries and Alt+c's direction all read it that way"
);

/// The NAME of a collapse mode's swap layout, shared vocabulary because both
/// halves of clave depend on the same two strings (#197).
///
/// The CLI writes them into the generated KDL, as both the `tab_template` and
/// the `swap_tiled_layout` that uses it; zellij then reports the active one
/// back on every `TabInfo` (`active_swap_layout_name`), which is how the bar
/// knows which geometry its tab is ACTUALLY in rather than which one it once
/// asked for. If these two drifted apart the bar would read every tab as being
/// in an unrecognised layout and switch it forever.
pub fn swap_layout_name(collapsed: bool) -> &'static str {
    if collapsed {
        "clave_collapsed"
    } else {
        "clave_expanded"
    }
}

/// The width the bar occupies in a given collapse mode — the one place the two
/// targets are chosen between outside the plugin, so a caller cannot pick the
/// wrong one by writing the constant it happens to remember.
pub fn target_cols_for(collapsed: bool) -> usize {
    if collapsed {
        COLLAPSED_TARGET_COLS
    } else {
        BAR_TARGET_COLS
    }
}

/// Which row geometry the bar renders — the #232 flag. `Double` is the
/// two-line card (the default); `Single` is the legacy one-line row,
/// retained intact behind this flag. Chosen per LAUNCH: the launch layout
/// bakes both the pane sizes and the plugin-config key from it, so the
/// geometry zellij gives the pane and the geometry the bar draws can never
/// disagree mid-session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowHeight {
    Single,
    #[default]
    Double,
}

/// The zellij plugin-config key carrying the mode into the bar (same
/// mechanism as [`CLAVE_BINARY_KEY`], #44).
pub const ROW_HEIGHT_KEY: &str = "row_height";

impl RowHeight {
    /// The width the seek machinery asks for in this mode — the card
    /// budgets ratified in #232, or the legacy pair for `Single`. `const` so
    /// the bar's test-mod width-target pins (`EXP_W`/`COL_W`) can compute
    /// off it at compile time instead of duplicating the numbers.
    pub const fn target_cols(self, collapsed: bool) -> usize {
        match (self, collapsed) {
            (RowHeight::Double, false) => 48,
            (RowHeight::Double, true) => 38,
            (RowHeight::Single, false) => BAR_TARGET_COLS,
            (RowHeight::Single, true) => COLLAPSED_TARGET_COLS,
        }
    }

    /// Terminal lines one row occupies — the `/2` the viewport and click
    /// conversions share (#148 discipline: derived once, here).
    pub fn lines_per_row(self) -> usize {
        match self {
            RowHeight::Single => 1,
            RowHeight::Double => 2,
        }
    }

    /// Parse the plugin-config value, failing CLOSED to the default: a
    /// typo'd or absent key must render the default design, never a
    /// surprise legacy mode.
    pub fn from_config_value(v: Option<&str>) -> RowHeight {
        match v {
            Some("single") => RowHeight::Single,
            _ => RowHeight::Double,
        }
    }
}

// ── floating pane geometry ──────────────────────────────────────────────────
//
// ONE geometry for every floating pane clave's own UI opens: the `Alt a`
// directory picker today (#110), the row-detail helper pane when #7 lands. Two
// panes that appear over the same fleet should appear at the same size.
//
// The `Alt f` scratch shell (#188) is deliberately NOT one of them and carries
// its own, smaller pair below — the picker's values are near-fullscreen because
// its own layout needs them, and a scratch terminal wants the fleet still
// visible around it.
//
// **The bar needs no clearing — zellij fences floating panes off it already.**
// `clave-bar` calls `set_selectable(false)` at load, and zellij's
// `offset_viewport` shrinks a tab's viewport by any full-height non-selectable
// pane sitting on its edge, so the viewport BEGINS at the bar's right edge.
// Every number below is a percent OF THAT REDUCED AREA, and `x` is floored at
// its left edge (`zellij-utils/src/pane_size.rs:304`, `adjust_coordinates` —
// the path a `Run { floating true; … }` keybind takes). A floating pane
// therefore cannot cover the bar however it is sized or dragged; #110 was
// raised believing it could, and the whole `x`-past-the-bar arithmetic that
// implied was never needed. What IS needed is a size: with no geometry zellij
// applies `half_size_middle_geom`, half of the already-reduced area, which is
// the "tiny pane" the issue actually reports.

/// Left edge: `0%` resolves below the viewport's own `x` and so is clamped up
/// to it — flush against the bar, no gap to hand-derive. Written rather than
/// omitted because omitting `x` centres the pane on `viewport.cols` measured
/// from absolute zero, which lands inside the bar and gets clamped to the same
/// place anyway, only after wasting the right-hand slack.
pub const FLOATING_X_PERCENT: usize = 0;

/// Full width of the non-bar area. `x + cols` lands exactly on the viewport's
/// right edge, which the overflow test (`>`) does not trip, so nothing is
/// shrunk. Wider than 100 would only be clipped back to this.
pub const FLOATING_WIDTH_PERCENT: usize = 100;

/// A row of breathing space above and below, so the pane reads as floating
/// rather than as a second tiled pane.
pub const FLOATING_Y_PERCENT: usize = 5;

/// Paired with [`FLOATING_Y_PERCENT`]: the two must sum to at most 100 or the
/// pane's height is silently shrunk to fit the viewport's bottom edge.
pub const FLOATING_HEIGHT_PERCENT: usize = 90;

const _: () = assert!(
    FLOATING_X_PERCENT + FLOATING_WIDTH_PERCENT <= 100
        && FLOATING_Y_PERCENT + FLOATING_HEIGHT_PERCENT <= 100,
    "a floating pane overflowing its viewport is shrunk to fit, so the geometry \
     would not be the one written here"
);

// ── the Alt+f scratch shell's geometry (#188) ───────────────────────────────
//
// A box flush against the bar's right edge, reaching almost all the way to the
// screen's right (Ollie's call, 2026-08-17 #207 drive): big enough to work in
// without resizing, with a sliver of fleet visible at the right edge. Same
// percent-of-the-non-bar-area rules as the picker's constants above.
//
// Since #207 these feed the bar's `open_terminal_floating` call, not a
// keybind: `Alt f` pipes to the bar (`clave-shell`), which spawns through the
// COORDINATES path — the only geometry path that floors x at the viewport's
// left edge. A `swap_floating_layout` resolves the same percents from
// absolute column zero and parks the shell over the bar (live probe
// 2026-08-17); do not move them back into layout KDL.
//
// It is NOT horizontally centred and cannot be: zellij resolves `x` as a percent
// of the non-bar width but measures it from absolute column zero, so anything
// non-zero moves the left edge only on terminals wide enough for the percentage
// to clear the bar — pinning it at 0 keeps the pane flush against the bar at
// every width.
//
// Without a geometry zellij applies `half_size_middle_geom` — half of an area
// the bar has ALREADY shrunk — which is the unusable sliver #188 reports. Stock
// `Alt f` had no geometry because it was zellij's own binding, not clave's.

/// Left edge: flush against the bar, like the picker's [`FLOATING_X_PERCENT`].
pub const SHELL_FLOATING_X_PERCENT: usize = 0;

/// Top edge. `y` is floored at the viewport's top exactly as `x` is at its left,
/// but nothing sits above the viewport here, so the sliver lands where written
/// and is paired by one below.
pub const SHELL_FLOATING_Y_PERCENT: usize = 4;

/// Nearly the whole non-bar width — the right edge stops a few columns short
/// of the screen's (`adjust_coordinates` narrows an overflow to the viewport's
/// right edge, so even 100 would be safe; 98 leaves the gap deliberate).
pub const SHELL_FLOATING_WIDTH_PERCENT: usize = 98;

/// Nearly the whole non-bar height, margins matching the horizontal ones.
pub const SHELL_FLOATING_HEIGHT_PERCENT: usize = 92;

const _: () = assert!(
    SHELL_FLOATING_X_PERCENT + SHELL_FLOATING_WIDTH_PERCENT <= 100
        && SHELL_FLOATING_Y_PERCENT + SHELL_FLOATING_HEIGHT_PERCENT <= 100,
    "a floating pane overflowing its viewport is shrunk to fit, so the geometry \
     would not be the one written here"
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The two targets, and the property that is not local to either: their
    /// separation, 24 columns since D19. Fail here if it changes.
    ///
    /// **Since D39 this is a design number, not a tolerance.** Both widths are
    /// declared in the layout and zellij switches between them, so nothing has
    /// to tell the two apart by measuring and no acceptance band can overlap.
    /// What the separation buys now is only what it looks like it buys: Alt+c
    /// produces a visibly different bar, wide enough to hold the summary and
    /// narrow enough to be a gutter.
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
            commit_ord: 0,
            last_visited: 0,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: None,
            context_tokens: None,
            context_level: None,
            buckets: Default::default(),
            model: None,
            provider: None,
            pr_number: None,
        };
        assert!(!serde_json::to_string(&a).unwrap().contains("archived"));
    }

    #[test]
    fn snapshot_roundtrips() {
        let snap = AgentSnapshot {
            seq: 7,
            tab_order: Default::default(),
            collapsed: false,
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
            agents: vec![Agent {
                uuid: "u1".into(),
                cwd: "/Users/x/code/clave".into(),
                repo_root: "/Users/x/code/clave".into(),
                branch: "main".into(),
                label: "clave · main · hello".into(),
                status: Status::Working,
                last_interacted: 1000,
                commit_ord: 0,
                last_visited: 0,
                tab_id: None,
                pane_id: None,
                stale: false,
                title: None,
                summary: String::new(),
                worktree: None,
                default_branch: None,
                context_tokens: None,
                context_level: None,
                buckets: Default::default(),
                model: None,
                provider: None,
                pr_number: None,
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
            commit_ord: 0,
            last_visited: 0,
            tab_id: Some(4),
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: None,
            context_tokens: None,
            context_level: None,
            buckets: Default::default(),
            model: None,
            provider: None,
            pr_number: None,
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
            commit_ord: 0,
            last_visited: 0,
            tab_id: None,
            pane_id: None,
            stale: true,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: None,
            context_tokens: None,
            context_level: None,
            buckets: Default::default(),
            model: None,
            provider: None,
            pr_number: None,
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
            commit_ord: 0,
            last_visited: 0,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: Some("CLA-MAIN".into()),
            summary: "fix the flaky auth".into(),
            worktree: Some("/x/.claude/worktrees/wt".into()),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            buckets: Default::default(),
            model: None,
            provider: None,
            pr_number: None,
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
            commit_ord: 0,
            last_visited: 0,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: Some("trunk".into()),
            context_tokens: None,
            context_level: None,
            buckets: Default::default(),
            model: None,
            provider: None,
            pr_number: None,
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
    fn snapshot_carries_tab_order_and_defaults_empty() {
        // §6.6 store tab order: row order ships IN the snapshot — seq-gated
        // full-state replace, the one channel that never diverged (C5 rd 5:
        // fire-and-forget pipe deltas diverged per instance).
        let snap = AgentSnapshot {
            seq: 1,
            agents: vec![],
            tab_order: std::collections::BTreeMap::from([(4usize, 12u64)]),
            collapsed: false,
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: AgentSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tab_order.get(&4), Some(&12));
        // Pre-field payloads (old store hydration) must still parse.
        let old: AgentSnapshot = serde_json::from_str("{\"seq\":1,\"agents\":[]}").unwrap();
        assert!(old.tab_order.is_empty());

        // MIXED-VERSION COMPAT (S1 §3.6). The field was RENAMED from
        // `tab_timeline`, whose values were unix SECONDS. A payload from an old
        // CLI must be IGNORED, not misread — this is the whole reason for
        // renaming instead of repurposing. A ~1.7e9 second value read as an
        // ordinal would outrank every ordinal ever minted, permanently, with no
        // expiry: the list would freeze in the old order and no prompt could
        // move anything. Degrading to "empty, so rows sort by tab position" is
        // deterministic and self-heals on the first commitment.
        let legacy: AgentSnapshot =
            serde_json::from_str(r#"{"seq":1,"agents":[],"tab_timeline":{"4":1753000000}}"#)
                .unwrap();
        assert!(
            legacy.tab_order.is_empty(),
            "an old tab_timeline must be ignored, never adopted as ordinals"
        );
    }

    #[test]
    fn agent_commit_ord_defaults_for_pre_s1_payloads() {
        // The other half of the mixed-version story: an old CLI's `Agent` has
        // no `commit_ord` at all. It must parse and default to 0 — "never
        // committed", which sorts to the bottom — rather than failing the whole
        // snapshot parse and blanking the bar.
        let a = Agent {
            uuid: "u1".into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x".into(),
            status: Status::Idle,
            last_interacted: 0,
            commit_ord: 7,
            last_visited: 0,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            worktree: None,
            default_branch: None,
            context_tokens: None,
            context_level: None,
            buckets: Default::default(),
            model: None,
            provider: None,
            pr_number: None,
        };
        let mut v: serde_json::Value = serde_json::to_value(&a).unwrap();
        v.as_object_mut().unwrap().remove("commit_ord");
        let old: Agent = serde_json::from_value(v).unwrap();
        assert_eq!(old.commit_ord, 0);
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
            tab_order: Default::default(),
            collapsed: true,
            order: OrderMode::default(),
            today: 0,
            tab_buckets: Default::default(),
            tab_touched: Default::default(),
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

    #[test]
    fn order_mode_defaults_to_frecency_24h() {
        assert_eq!(
            OrderMode::default(),
            OrderMode::Frecency {
                half_life_hours: 24
            }
        );
    }

    /// Pre-field payloads must keep parsing — the repo-wide `serde(default)`
    /// doctrine, pinned here for the four new fields at once.
    #[test]
    fn pre_frecency_snapshot_payload_still_parses() {
        let old = r#"{"seq":1,"agents":[]}"#;
        let snap: AgentSnapshot = serde_json::from_str(old).unwrap();
        assert_eq!(snap.order, OrderMode::default());
        assert_eq!(snap.today, 0);
        assert!(snap.tab_buckets.is_empty());
    }

    #[test]
    fn order_mode_round_trips_both_variants() {
        for m in [
            OrderMode::Recency,
            OrderMode::Frecency { half_life_hours: 6 },
        ] {
            let json = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<OrderMode>(&json).unwrap(), m);
        }
    }

    #[test]
    fn row_height_defaults_to_double_and_maps_its_targets() {
        assert_eq!(RowHeight::default(), RowHeight::Double);
        // Double: the ratified card budgets (#232). Single: the legacy pair,
        // which MUST keep reading the existing constants so the old design
        // cannot drift from the flag's legacy arm.
        assert_eq!(RowHeight::Double.target_cols(false), 48);
        assert_eq!(RowHeight::Double.target_cols(true), 38);
        assert_eq!(RowHeight::Single.target_cols(false), BAR_TARGET_COLS);
        assert_eq!(RowHeight::Single.target_cols(true), COLLAPSED_TARGET_COLS);
        assert_eq!(RowHeight::Double.lines_per_row(), 2);
        assert_eq!(RowHeight::Single.lines_per_row(), 1);
    }

    #[test]
    fn row_height_parses_its_config_value_failing_closed_to_double() {
        assert_eq!(
            RowHeight::from_config_value(Some("single")),
            RowHeight::Single
        );
        assert_eq!(
            RowHeight::from_config_value(Some("double")),
            RowHeight::Double
        );
        // Absent, empty, or junk → the default. A typo must not strand a user
        // in a mode they didn't ask for.
        assert_eq!(RowHeight::from_config_value(None), RowHeight::Double);
        assert_eq!(RowHeight::from_config_value(Some("")), RowHeight::Double);
        assert_eq!(
            RowHeight::from_config_value(Some("tall")),
            RowHeight::Double
        );
    }

    #[test]
    fn row_height_serde_is_lowercase_and_defaultable() {
        assert_eq!(
            serde_json::to_string(&RowHeight::Single).unwrap(),
            "\"single\""
        );
        let d: RowHeight = serde_json::from_str("\"double\"").unwrap();
        assert_eq!(d, RowHeight::Double);
    }
}
