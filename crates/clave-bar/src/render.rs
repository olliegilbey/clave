//! The sidebar's entire visual surface, as a pure function.
//!
//! Deliberately NO zellij-tile imports, for the same reason as `model.rs`: the
//! bin cannot link on the host, so anything that lives there is untestable.
//! Until now `fn render` (main.rs) WAS the product's visual output, which is
//! why four design rounds were litigated in prose — prose was the only medium
//! with a reader. This module is that reader.
//!
//! Authority: `docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`
//! ("the lock"). Where it and an S4/S5/S6 spec disagree, the lock wins. The
//! example at `examples/bar-preview.rs` is driven by `render_rows` for exactly
//! one reason: two renders of the same design diverge, and the Python preview
//! this replaces had already started to.
//!
//! GLYPH RULE, load-bearing (lock §5.4): every non-ASCII glyph below is a
//! `\u{...}` escape, never a literal. Literal glyphs were silently lost in
//! transit twice during the design rounds and the loss was misdiagnosed as
//! missing font coverage; the failure mode is tofu in production from a diff
//! that read clean.

use unicode_width::UnicodeWidthChar;

// ── geometry ────────────────────────────────────────────────────────────────

/// The ratified expanded width (lock §2, widened by LEDGER D19). `cols` stays a
/// parameter — zellij hands the plugin whatever the pane actually is — but every
/// number in the design was chosen against this one, and at this width the
/// output IS the lock's table. The lock's own table is written at 44; D19 moved
/// the width to 54 and the goldens here moved with it.
///
/// **The same number the width seek drives the pane to**, not a parallel copy
/// of it: nothing tied the two together, so moving the seek's target alone left
/// every golden here green while pinning the old width (S8 §3.3, #86). D19 is
/// the move that machinery was built for, and it worked — seven tests failed
/// loudly on the target change rather than passing against a stale picture.
pub const DESIGN_COLS: usize = clave_types::BAR_TARGET_COLS;

/// The collapsed profile's design width (LEDGER D17) — likewise the seek's
/// collapsed target, linked rather than mirrored.
pub const COLLAPSED_DESIGN_COLS: usize = clave_types::COLLAPSED_TARGET_COLS;

/// The gutter's position-locked columns: cap, status, space, rule, space —
/// then, after the battery cell — space, provenance, space. Eight columns that
/// hold their width in EVERY profile, each rendering a space when its glyph is
/// absent, so a dropped glyph degrades to a blank cell rather than a shifted row
/// (lock §2.1).
///
/// The gutter itself is [`Widths::gutter`], because the BATTERY cell between
/// these two halves is the one column count that varies by profile (#105). D16's
/// rule — one layout, parameterised by width — is intact; what it no longer
/// implies is that every gutter cell is one column wide.
const GUTTER_FIXED_W: usize = 8;

/// What the battery cell SHOWS, and therefore how wide it is (#105).
///
/// The ramp resolves magnitude to eleven steps and risk to four bands: the right
/// reading for a glance, the wrong one when the question is "how much". Expanded
/// has the columns to answer it and prints the number; collapsed does not, and at
/// 30 columns the eleven-step glyph IS the right resolution. (Ollie's design
/// call, 2026-07-31.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryCell {
    /// The ramp glyph, one column.
    Glyph,
    /// The token count as right-aligned text in the ramp's ink — FOUR columns,
    /// because every value in the real range fits four characters: `1k`, `400k`,
    /// `999k`, `1m`, `1.1m`.
    Count,
}

impl BatteryCell {
    pub fn width(self) -> usize {
        match self {
            BatteryCell::Glyph => 1,
            BatteryCell::Count => 4,
        }
    }
}

/// Collapsed is a WIDTH PROFILE, not a second layout (LEDGER D16, supersedes
/// D12): one `render_row` body, parameterised by how wide the title and repo
/// cells are. `summary` is never part of the profile — it is the only flex
/// cell in EITHER state (D9, D16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Widths {
    pub title: usize,
    pub repo: usize,
    /// The battery cell — a glyph in one column, or the token count in four
    /// (#105). The only gutter cell whose width is a profile's to choose.
    pub battery: BatteryCell,
}

impl Widths {
    /// LEDGER D19, Ollie's call after the Gate 1 live look: the bar goes to 54
    /// and the extra ten columns split *"mostly for the summary, but could do
    /// another two chars for the title"*. So title 7 → 9, repo unchanged at 7,
    /// and summary takes the rest — `54 - 13 - 9 - 7` = **25**, from 17.
    ///
    /// This supersedes the byte-for-byte pin against the pre-D16 renderer that
    /// stood while the target was 44: the design width moved, so the golden
    /// moves with it. `min_intact_cols()` rises 27 → 29 as a consequence, which
    /// slightly widens the sub-floor regime D31's clip now covers.
    ///
    /// #105 then takes the battery cell to four columns here — the token count,
    /// which eleven glyphs can only approximate — and the three columns come out
    /// of `summary`, the only flexible cell there is (D9): 25 → 22 at the design
    /// width, and the floor rises again, 29 → 32.
    pub const EXPANDED: Widths = Widths {
        title: 9,
        repo: 7,
        battery: BatteryCell::Count,
    };

    /// LEDGER D17: chosen by Ollie from three rendered candidates. Repo drops
    /// to three (`cla`, `nal` — collisions are rare and the repo ink still
    /// disambiguates); title holds at 7 — the rejected 5-column variant bought
    /// two summary characters by truncating the chip itself (`API-GW`/`API-V2`
    /// both to `API-`, `KDL-GRD` to `KDL-G`), and the chip is the thing a tab
    /// is identified BY.
    ///
    /// **D17's second reason is RETIRED at D19, knowingly.** It used to read
    /// "identical to `EXPANDED` … so the chip does not reflow when the profile
    /// toggles, and the eye keeps its anchor across the transition." `EXPANDED`
    /// now takes title 9, so the chip DOES reflow 9 → 7 on every Alt+c. Put to
    /// Ollie against two alternatives that preserve the anchor — holding 9 in
    /// both profiles (collapsed summary 7 → 5) and leaving title at 7 to give
    /// the summary all ten new columns — and he took the reflow. Titles of 7
    /// cells or fewer are unaffected; only longer ones truncate on collapse.
    ///
    /// The battery cell stays a GLYPH here (#105): there is no room for four
    /// columns of digits at 30, and the eleven-step glyph is the right reading at
    /// this width anyway.
    pub const COLLAPSED: Widths = Widths {
        title: 7,
        repo: 3,
        battery: BatteryCell::Glyph,
    };

    /// Cols 1 to the last gutter space: the eight position-locked columns plus
    /// whatever the battery cell costs this profile (#105) — 12 for `EXPANDED`,
    /// 9 for `COLLAPSED`, which is what it used to be for both. The text area
    /// starts at the next column.
    pub fn gutter(self) -> usize {
        GUTTER_FIXED_W + self.battery.width()
    }

    /// Fixed columns everywhere; `summary` is the only flex cell (LEDGER D9).
    /// Everything else — gutter, title, repo, the two separating spaces, the
    /// right margin and both caps — holds its width at any `cols`, so below
    /// this floor a row is wider than the pane rather than misaligned. The `4`
    /// is the space after title, the space after repo, the right margin and the
    /// right cap (D12's arithmetic, generalised by D16 to any profile): `32` for
    /// `EXPANDED` (29 until #105 widened its battery cell; 27 before D33 took it
    /// to (9, 7)), `23` for `COLLAPSED` (D17, unmoved). That is deliberate, not a
    /// compromise: a row that silently reflowed its columns to fit would be the
    /// one failure mode §2.1 exists to forbid. S6 §2.10's `cols - 7` text budget
    /// is superseded.
    pub fn min_intact_cols(self) -> usize {
        self.gutter() + 4 + self.title + self.repo
    }
}

// ── colour and glyphs ───────────────────────────────────────────────────────
//
// Every VALUE lives in `theme.rs` — one home for every visual variable
// (#145). This module keeps the arithmetic; the re-export keeps render as the
// visual surface's façade, so the tests below, the two examples and clave's
// dev preview reach the vocabulary through the module that uses it.

pub use crate::theme::{
    BASE, BATTERY, CHIP_INK, CONSOLE, DEFAULT_INK, DORMANT_FADE, FADE, GREEN, ORANGE, PALETTE, RED,
    RESET, Rgb, SEL_BG, Theme, UNTINTED, YELLOW,
};
use crate::theme::{
    DONE_INK, ELLIPSIS, FAILED_INK, LCAP, NEEDS_YOU_INK, OPENING_INK, RCAP, RULE, TERM_GLYPH,
    TERM_MARK, WORKING_INK,
};

// ── the row ─────────────────────────────────────────────────────────────────

/// What the status cell says. Five of these are `clave_types::Status`; the
/// other four are row states the model distinguishes without a `Status` —
/// `Stale` is a `bool` flag (`clave open` found the cwd missing), `Dormant`,
/// `DormantSelected` and `Opening` are model states. The renderer stays total
/// by owning all nine (LEDGER D10); `Status::glyph()` is untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    NeedsYou,
    Working,
    Done,
    Idle,
    Failed,
    Dormant,
    DormantSelected,
    Opening,
    Stale,
}

impl RowStatus {
    /// The COLOUR is the state; the shape varies only where the state is not a
    /// conversation at all (lock §5). `Failed` is U+2716 HEAVY multiplication
    /// x and `Stale` is U+2717 BALLOT x — different glyphs for different
    /// things, and easy to transpose (FOOTGUNS). `DormantSelected` is U+23CE
    /// RETURN SYMBOL in the Opening tint — the ⏎ affordance IS the first
    /// frame of the launch lifecycle it invites (⏎ → ↻ → status, #100).
    ///
    /// Takes the theme for its two NON-semantic inks only: `Idle`'s untinted
    /// grey and `Dormant`'s default ink follow the user's theme, while the
    /// lifecycle hues stay the fixed consts — red means failed under every
    /// theme (#145).
    pub fn mark(self, theme: &Theme) -> (char, Rgb) {
        match self {
            RowStatus::NeedsYou => ('\u{25cf}', NEEDS_YOU_INK),
            RowStatus::Working => ('\u{25cf}', WORKING_INK),
            RowStatus::Done => ('\u{25cf}', DONE_INK),
            RowStatus::Idle => ('\u{25cf}', theme.untinted),
            RowStatus::Failed => ('\u{2716}', FAILED_INK),
            // Hollow on the DEFAULT ink, never sumiInk4: that read as
            // near-invisible against the bar (#123), and the shape alone
            // carries "not running". #206's row-level fade is applied AFTER
            // this table by `render_row`, so a legible base is what keeps the
            // half-faded glyph above #123's floor.
            RowStatus::Dormant => ('\u{25cb}', theme.default_ink),
            RowStatus::DormantSelected => ('\u{23ce}', OPENING_INK),
            RowStatus::Opening => ('\u{21bb}', OPENING_INK),
            RowStatus::Stale => ('\u{2717}', FAILED_INK),
        }
    }
}

/// What a terminal row's status cell says — the terminal-tab counterpart of
/// `RowStatus`, kept separate because the GLYPH never varies (always the
/// console mark; a terminal has no lifecycle shapes) while the ink reuses the
/// agent colour language: the COLOUR is the state (lock §5). `Done`/`Failed`
/// can only arise on command panes — an interactive shell never exits while
/// the tab lives, so its whole range is `Idle`/`Running`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermStatus {
    Idle,
    Running,
    Done,
    Failed,
}

impl TermStatus {
    /// Same split as [`RowStatus::mark`]: `Idle` follows the theme's default
    /// ink, the lifecycle hues stay fixed semantic (#145).
    pub fn ink(self, theme: &Theme) -> Rgb {
        match self {
            TermStatus::Idle => theme.default_ink,
            TermStatus::Running => WORKING_INK, // roninYellow — Working's ink
            TermStatus::Done => DONE_INK,       // springGreen — Done's ink
            TermStatus::Failed => FAILED_INK,   // samuraiRed — Failed's ink
        }
    }
}

/// Three-state, not the two-state "worktree marker" S6 describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Main,
    Branch,
    Worktree,
}

impl Provenance {
    /// A main checkout renders NOTHING, and that is the researched choice, not
    /// an omission: essentially no surveyed tool marks the default branch with
    /// a glyph, and blanking the most common row is what makes the two marked
    /// states mean something (lock §5.1). The worktree glyph is an invention —
    /// there is no worktree glyph anywhere (lock §5.2).
    pub fn mark(self) -> Option<char> {
        match self {
            Provenance::Main => None,
            Provenance::Branch => Some('\u{f062c}'), // nf-md-source_branch (lazygit's)
            Provenance::Worktree => Some('\u{168c2}'), // bamum tree
        }
    }
}

/// A row's fields. `Terminal` is a variant rather than a bundle of `None`s
/// because a terminal tab is a different thing: it has no agent record, so its
/// zellij name is the chip, the console mark holds the status cell and `TERM`
/// the battery cell (lock §5, §7.1; #206).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowContent {
    Agent {
        status: RowStatus,
        /// Index into the S7 ramp, saturating at its last entry. `None` renders
        /// a blank cell, and that case is now narrow: **no reading yet**, not
        /// "dormant". §7.2 is settled (#62) — a dormant conversation consumes
        /// nothing, so its stored figure is EXACTLY its current occupancy and it
        /// renders in full ramp colour like any other row. It is the live row
        /// whose reading is a turn behind.
        battery: Option<u8>,
        /// The raw count the level was bucketed from, rendered as text where the
        /// profile has four columns for it (#105). Rides alongside `battery`
        /// rather than replacing it: the ink is the ramp's coarse risk band and
        /// the text is the exact magnitude, and the two axes are the whole point.
        /// `None` renders a blank cell — the bar never invents a measurement.
        tokens: Option<u32>,
        provenance: Provenance,
        /// The chip; `None` when the session was never renamed.
        title: Option<String>,
        title_ink: Option<u8>,
        repo: String,
        repo_ink: Option<u8>,
        summary: String,
    },
    Terminal {
        /// The zellij tab name — the chip. Lock §7.1: this is the only row
        /// kind that uses it, and a zellij rename IS the labelling mechanism.
        name: String,
        status: TermStatus,
        /// Prefix-matched from a store row whose checkout contains the pane's
        /// cwd — reused verbatim, never computed here. `Main` (blank) when no
        /// agent occupies the repo.
        provenance: Provenance,
        /// The cwd's final directory name, through the same ink allocation the
        /// agent rows use — one repo is one colour everywhere (lock §4).
        /// `None` blanks the cell: the bar never invents a location.
        repo: Option<String>,
        repo_ink: Option<u8>,
        /// The focused pane's foreground command — live while it runs,
        /// lingering as "most recently run" at the prompt. Empty until the
        /// first command.
        command: String,
    },
}

impl RowContent {
    /// A terminal row before anything is known about its pane: idle, no
    /// location, no command. Every field the pane facts fill in later has an
    /// honest blank, so construction sites that only carry the tab name (and
    /// every pre-#206 test fixture) stay one line.
    pub fn terminal(name: impl Into<String>) -> RowContent {
        RowContent::Terminal {
            name: name.into(),
            status: TermStatus::Idle,
            provenance: Provenance::Main,
            repo: None,
            repo_ink: None,
            command: String::new(),
        }
    }
}

/// Inks are `Option<u8>`, never a bare `u8` (LEDGER D7): `0` is `crystalBlue`,
/// a real palette entry, so a bare `u8` has no unset value and `unwrap_or(0)`
/// paints every untinted row one colour while reading as "untinted".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub content: RowContent,
    pub selected: bool,
    /// Block membership, carried SEPARATELY from the status glyph (#206):
    /// `stale` and `opening` outrank `Dormant` in `agent_content`'s tier, so
    /// the status alone cannot say which block the model placed the row in —
    /// a stale dormant row wears ✗ and would read as live to any
    /// status-derived fade (Codex, PR #210).
    pub dormant: bool,
}

// ── measurement ─────────────────────────────────────────────────────────────

/// Terminal CELLS, not scalars. `str::chars()` counts code points: a wide
/// (East-Asian W/F) glyph occupies two columns and a combining mark none, so
/// code-point arithmetic silently misaligns every column to its right. This is
/// the hazard the dossier records against today's clamp.
pub fn display_cells(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// The substring occupying display cells `[start, end)`. Tests that want "the
/// title column" mean CELLS; `chars().collect::<Vec<_>>()[9..16]` only agrees
/// when every preceding glyph happens to be one cell wide, which is exactly the
/// assumption this module exists to stop relying on. A wide glyph straddling
/// either boundary is excluded — the caller asked for a cell range, and half a
/// glyph is not a cell.
pub fn cell_slice(s: &str, start: usize, end: usize) -> String {
    let mut at = 0;
    let mut out = String::new();
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if at >= start && at + w <= end {
            out.push(ch);
        }
        at += w;
    }
    out
}

/// Drop SGR sequences so what remains can be MEASURED. Shared by the width
/// invariant in the tests and by the preview's self-check — the lock only
/// CLAIMS every row is `DESIGN_COLS` cells, and a claim about a rendered row is worth
/// exactly as much as the thing that proves it.
pub fn strip_sgr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("\u{1b}[") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 2..];
        // Params are ASCII, so the byte index is also the char index.
        match tail.find(|c: char| !matches!(c, '0'..='9' | ';')) {
            Some(j) if tail.as_bytes()[j] == b'm' => rest = &tail[j + 1..],
            // Not SGR: keep it verbatim rather than swallowing the rest of the
            // row on a sequence this function does not model.
            _ => {
                out.push_str("\u{1b}[");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Truncate a RENDERED row — SGR sequences and all — to exactly `cols` cells.
///
/// `render_row` builds every row at `widths.min_intact_cols()` even when the
/// pane is narrower: the fixed columns cannot reflow (lock §2.1), so a
/// sub-floor row over-runs UNIFORMLY rather than going ragged (D13). D13 then
/// assumed the terminal would clip that over-run. **It does not — it wraps
/// it**, and a wrapped row makes every bar row double-height with a blank
/// second line. Observed live 2026-07-29: on a monitor change, and on every
/// tab spawn below ~123 columns where the birth percent lands the pane under
/// `EXPANDED`'s `min_intact_cols()` floor (32 as of #105).
///
/// So the clip happens here instead of being assumed. Only truncation changes:
/// the row is still BUILT at the floor, so no column reflows and every row
/// loses the same trailing cells — the uniformity D13 chose is preserved, and
/// only the wrap is removed.
///
/// The pad after `RESET` matters. A wide glyph straddling the boundary is
/// excluded rather than half-drawn (same rule as `cell_slice`), which can
/// leave the row one cell short; padding unstyled keeps the row exactly `cols`
/// without painting a colour into a cell the clip just took away.
fn clip_to_cells(s: &str, cols: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    let mut rest = s;
    while !rest.is_empty() {
        // SGR carries no width and must survive the cut: dropping it would
        // leave the terminal's colour state open past the end of the row.
        if let Some(tail) = rest.strip_prefix("\u{1b}[")
            && let Some(j) = tail.find(|c: char| !matches!(c, '0'..='9' | ';'))
            && tail.as_bytes()[j] == b'm'
        {
            out.push_str(&rest[..2 + j + 1]);
            rest = &tail[j + 1..];
            continue;
        }
        let ch = rest.chars().next().expect("rest is non-empty");
        let w = ch.width().unwrap_or(0);
        if at + w > cols {
            break;
        }
        out.push(ch);
        at += w;
        rest = &rest[ch.len_utf8()..];
    }
    out.push_str(RESET);
    out.push_str(&" ".repeat(cols - at));
    out
}

/// A fixed-width column, measured in cells: truncate when long, pad when short.
/// The PAD is load-bearing — alignment is the separator (lock §2.3), which is
/// why one space suffices where the bar previously spent three on ` \u{b7} `.
fn clamp(s: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    // Summaries are agent-authored: hooks write them, so a `\n`, `\r` or a bare
    // `\u{1b}` is reachable input, not a hypothetical. Control characters
    // measure as ZERO cells, so they would sail through this clamp and through
    // the every-row-is-`cols`-cells invariant while breaking the row on screen.
    // `render.rs` is what GUARANTEES that invariant, and a guarantee that holds
    // only when someone else sanitises first is not a guarantee — so it is
    // enforced here, at the point text enters a cell, rather than at the
    // wiring boundary. `char::is_control()` is exactly Cc: C0, DEL and C1.
    //
    // REPLACED with a space, not dropped (task 1.5 D16 follow-up): a `\n` in a
    // summary is a wrapped sentence, and `"a\nb"` collapsing to `"ab"` silently
    // merges two words at the join. A space preserves the word boundary that
    // dropping destroys, and is neutral to every golden either way.
    let s: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let s = s.as_str();
    let n = display_cells(s);
    if n <= w {
        let mut out = String::from(s);
        out.push_str(&" ".repeat(w - n));
        return out;
    }
    // LEDGER D18: at 4 cells or fewer the ellipsis eats 25%+ of the field and,
    // in a fixed-column layout, tells the reader nothing they cannot already
    // see — so it is dropped and the field truncates hard. Above 4 it stays.
    // This is what turns `repo = 3`'s `cl\u{2026}` back into the three real
    // characters D17 chose 3 for (`cla`, `nal`), and it is neutral at
    // `EXPANDED`'s `repo = 7`, well above the threshold.
    let ellipsis = w > 4;
    let budget = if ellipsis { w - 1 } else { w };
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        // Wide glyph would straddle the boundary — dropped whole, so the
        // column never over-runs by one.
        if used + cw > budget {
            break;
        }
        out.push(ch);
        used += cw;
    }
    if ellipsis {
        out.push(ELLIPSIS);
        used += 1;
    }
    out.push_str(&" ".repeat(w - used));
    out
}

// ── the viewport ────────────────────────────────────────────────────────────

/// Rows kept on screen BELOW the selection while the view is scrolled (#148) —
/// the lookahead. Honoured only as far as the pane has room for it (the
/// selection's own line is never spent on it) and as far as rows exist below —
/// that second limit is the end-of-list clamp in [`viewport_top`], not a
/// separate cap.
const LOOKAHEAD: usize = 2;

/// The first row the viewport shows, given the row count, which row is selected
/// and the pane height in lines (#148).
///
/// ONE function for both the draw and the click map, on purpose: the 2026-08-06
/// incident had two symptoms, invisible rows *and* clicks landing one or two
/// rows above the pointer, and a fix that scrolled the picture while the hit
/// test kept counting from row 0 would have cured the first and hidden the
/// second until the next overflow.
///
/// The rule (#148 spec, "follow rule") is derived, not remembered: top-anchored
/// whenever that keeps the selection on screen, otherwise the MINIMAL slide
/// that shows the selection plus its lookahead. Holding no scroll position of
/// its own is what makes a snapshot unable to yank the view — a row spawning
/// above the selection moves the selection's index and this offset by exactly
/// one, so the same rows stay under the reader's eye.
///
/// The one place that costs something, ruled acceptable by the maintainer on
/// 2026-08-11: while the selection sits on the last row, the end-of-list clamp
/// is holding the view above where the follow rule wants it, so rows arriving
/// BELOW release that clamp and the suppressed lookahead reasserts itself — the
/// view slides down by at most [`LOOKAHEAD`] rows with no navigation. That is
/// the accepted trade for a viewport that is derived and never remembered:
/// remembering a scroll position is the only way to refuse the slide, and a
/// remembered position is one an arriving snapshot can leave the selection
/// outside of.
pub fn viewport_top(len: usize, selected: Option<usize>, height: usize) -> usize {
    // A pane with no lines shows no rows, and must not hand the click map an
    // offset for a screen nobody can see.
    if height == 0 {
        return 0;
    }
    // No selection (no focused tab) is a resting bar, not a scrolled one.
    let Some(selected) = selected else {
        return 0;
    };
    // Lookahead is a courtesy, never a cost: it is capped by the room left in
    // the pane once the selection has its own line, or a one-line pane would
    // scroll away the very row it exists to pin.
    let lookahead = LOOKAHEAD.min(height - 1);
    // Two clamps, and between them the whole rule. `saturating_sub` IS the
    // "top-anchored whenever it fits" arm: while the selection and its
    // lookahead land inside the first screenful the wanted top is 0 or below
    // it, and a list shorter than the pane can never want one. `min` is the
    // end of the list: the view never slides past the last screenful, which is
    // also what quietly shortens the lookahead as the selection reaches the
    // final rows — there is nothing below them to look ahead at.
    (selected + lookahead + 1)
        .saturating_sub(height)
        .min(len.saturating_sub(height))
}

// ── the renderer ────────────────────────────────────────────────────────────

/// The whole bar, one `String` per row the pane can hold — `height` lines at
/// most, sliced to the viewport (#148), so the caller prints what it is given
/// and never has to know which rows those are.
///
/// Whole-bar rather than per-row (LEDGER D5) because the fade is RELATIVE:
/// lock §6 recedes every *unselected* row only when some row is selected, and a
/// per-row function cannot know that without a parameter that re-states what
/// the slice already knows. It is also the unit a golden test should assert —
/// the picture, not a fragment.
pub fn render_rows(
    rows: &[Row],
    cols: usize,
    height: usize,
    widths: Widths,
    theme: &Theme,
) -> Vec<String> {
    // The viewport (#148): the pane height is a hard budget, and a bar that
    // printed past it drew rows zellij clipped away — nav-reachable, invisible.
    let top = viewport_top(rows.len(), rows.iter().position(|r| r.selected), height);
    let rows = &rows[top..top.saturating_add(height).min(rows.len())];
    // `viewport_top` guarantees the selected row (if any) is inside this
    // slice — pinned by the proptest — so the fade computed over the slice
    // is equivalent to computing it over the whole list.
    let any_selected = rows.iter().any(|r| r.selected);
    rows.iter()
        .map(|row| render_row(row, cols, widths, any_selected, theme))
        // Below the floor the row was built wider than the pane on purpose;
        // `clip_to_cells` is what stops the terminal WRAPPING that over-run
        // into a second, blank line. Above the floor the row is already
        // exactly `cols`, so this is a no-op the branch skips.
        .map(|line| {
            if cols < widths.min_intact_cols() {
                clip_to_cells(&line, cols)
            } else {
                line
            }
        })
        .collect()
}

fn hue(ink: Option<u8>, theme: &Theme) -> Rgb {
    ink.and_then(|i| theme.palette.get(usize::from(i)))
        .map_or(theme.untinted, |c| *c)
}

fn render_row(row: &Row, cols: usize, widths: Widths, any_selected: bool, theme: &Theme) -> String {
    // Nothing to recede FROM when nothing is selected, so an unfocused bar
    // renders at full strength (lock §6) — except a dormant-block row, whose
    // fade is absolute rather than relative (#206). The flag, not the status:
    // stale outranks Dormant in the glyph tier, and a stale dormant row must
    // still recede (Codex, PR #210). Two escapes stay at full strength —
    // `DormantSelected` lands in the `selected` arm, and `Opening` is the
    // row the user JUST launched, mid-transition to live (⏎ → ↻, #100).
    let dormant = row.dormant
        && !matches!(
            row.content,
            RowContent::Agent {
                status: RowStatus::Opening,
                ..
            }
        );
    let fade = if row.selected {
        0.0
    } else if dormant {
        DORMANT_FADE
    } else if !any_selected {
        0.0
    } else {
        FADE
    };
    // Re-asserted after every span that sets its own colour. Emitting it more
    // often than strictly necessary is deliberate: the alternative is tracking
    // SGR state across the row, and a background that lapses for one cell is
    // exactly the ragged selection lock §6 forbids.
    let o = if row.selected {
        theme.sel_bg.bg()
    } else {
        String::new()
    };
    let ink = |c: Rgb| c.mix(theme.base, fade).fg();

    let mut out = String::new();

    // Col 1. The cap is drawn as a FOREGROUND glyph on the default background —
    // that is what rounds the row's end. Reserved (blank) on unselected rows so
    // the selected row's content does not sit one column right of its
    // neighbours (lock §2.2).
    if row.selected {
        out.push_str(&theme.sel_bg.fg());
        out.push(LCAP);
        out.push_str(&o);
    } else {
        out.push(' ');
    }

    // The gutter proper: status, rule, battery, provenance. Every cell but the
    // battery is one column in either profile (#105).
    let battery_w = widths.battery.width();
    match &row.content {
        RowContent::Terminal {
            status,
            provenance,
            repo_ink,
            ..
        } => {
            // The console mark moves to the STATUS cell (#206): the glyph is
            // the row's kind, the colour its state — the same axis split every
            // agent row already reads by (lock §5). Idle wears fujiWhite, not
            // katanaGray: a terminal is a real row, not a disabled one.
            out.push_str(&o);
            out.push_str(&ink(status.ink(theme)));
            out.push(CONSOLE); // col 2
            out.push_str(&o);
            push_rule(&mut out, &o, &ink, theme.default_ink); // cols 3–5
            out.push_str(&o);
            out.push_str(&ink(theme.default_ink));
            match widths.battery {
                // `TERM` where an agent row shows its count — four cells,
                // exactly, so the class marker lands on the digits' edge
                // (#105) and terminal rows scan as a class.
                BatteryCell::Count => out.push_str(&rjust(TERM_MARK, battery_w)),
                BatteryCell::Glyph => out.push(TERM_GLYPH),
            }
            out.push_str(&o);
            out.push(' '); // the space after the battery cell
            out.push_str(&o);
            // Same rule as the agent arm: the one gutter cell permitted an
            // arbitrary RGB, in the repo's ink (lock §4.1). The provenance is
            // borrowed from the store row whose checkout holds this pane's
            // cwd, so blank also means "no agent knows this repo".
            match provenance.mark() {
                Some(glyph) => {
                    out.push_str(&ink(hue(*repo_ink, theme)));
                    out.push(glyph);
                    out.push_str(&o);
                }
                None => out.push(' '),
            }
        }
        RowContent::Agent {
            status,
            battery,
            tokens,
            provenance,
            repo_ink,
            ..
        } => {
            let (glyph, colour) = status.mark(theme);
            out.push_str(&o);
            out.push_str(&ink(colour));
            out.push(glyph); // col 2
            out.push_str(&o);
            push_rule(&mut out, &o, &ink, theme.default_ink); // cols 3–5
            out.push_str(&o);
            // CLAMPED, not indexed. The host already clamps, so an
            // out-of-range level cannot arise from this version — but the wire
            // crosses a version boundary, and a newer host with a longer ramp
            // would otherwise blank the cell on an old bar. Blank means "no
            // reading" here, so the failure mode would be a full-looking row
            // for a session that is out. Saturating to the last entry says
            // "at least this bad", which is the safe direction to be wrong in.
            // (CodeRabbit, #147)
            let battery = battery.map(|i| usize::from(i).min(BATTERY.len() - 1));
            match widths.battery {
                BatteryCell::Glyph => match battery.map(|i| BATTERY[i]) {
                    Some((glyph, colour)) => {
                        out.push_str(&ink(colour));
                        out.push(glyph);
                        out.push_str(&o);
                    }
                    None => out.push(' '),
                },
                // The INK is the ramp's band, the TEXT is the exact count
                // (#105). Each half stays independently total: a count with no
                // level cannot arise from any host — the level is bucketed FROM
                // the count — but the wire crosses a version boundary, and
                // fujiWhite is the honest ink for a number with no band behind
                // it. An absent count blanks the cell, whatever the level says:
                // the ramp index is not a token figure and rendering one from it
                // would be inventing a measurement.
                BatteryCell::Count => match tokens {
                    Some(t) => {
                        out.push_str(&ink(battery.map_or(theme.default_ink, |i| BATTERY[i].1)));
                        out.push_str(&rjust(&token_text(*t), battery_w));
                        out.push_str(&o);
                    }
                    None => out.push_str(&" ".repeat(battery_w)),
                },
            }
            out.push_str(&o);
            out.push(' '); // the space after the battery cell
            out.push_str(&o);
            // The one gutter cell permitted an arbitrary RGB: it takes the repo
            // ink, making repo identity a shape in the gutter as well as a
            // colour in the text (lock §4.1).
            match provenance.mark() {
                Some(glyph) => {
                    out.push_str(&ink(hue(*repo_ink, theme)));
                    out.push(glyph);
                    out.push_str(&o);
                }
                None => out.push(' '),
            }
        }
    }
    out.push_str(&o);
    out.push(' '); // the last gutter column

    // Only `summary` flexes (LEDGER D9), in EITHER profile (D16). Below
    // `widths.min_intact_cols()` the fixed columns cannot all fit, and the row
    // is deliberately WIDER than the pane rather than reflowed — but EVERY row
    // kind over-runs to the same width, or the bar goes ragged instead of
    // merely clipped, which is the alignment loss lock §2.1 exists to forbid.
    // A terminal row used to shrink to `cols` while an agent row held at the
    // floor. D16 makes this a guard against pathological widths, but NOT one
    // a user never sees: the collapsed resting width (30) sits two below the
    // EXPANDED floor (32), and both peek-on-nav (main.rs's `clave-visited`
    // arm) and Alt+c's expand routinely draw the EXPANDED profile at 30-31
    // cols — the row still holds exactly `cols` cells, but has already lost
    // its right margin, and (for the selected row) its right cap, until the
    // seek reaches 32. The visible effect is a cosmetic one-frame blink
    // during the grow animation, not a wrap or a misalignment: every row is
    // still uniform and still exactly `cols` cells, which is the property
    // this comment's job is to state. D12's "collapsed is a second layout"
    // conclusion is superseded; this comment's job is the same one D13 gave
    // it.
    let intact = cols.max(widths.min_intact_cols());
    // `saturating_sub` rather than a floor check: a 0-width budget must render
    // nothing, not panic, if these constants ever move.
    let body = intact.saturating_sub(widths.gutter() + 2); // minus right margin and cap
    let summary_w = body.saturating_sub(widths.title + widths.repo + 2);

    match &row.content {
        RowContent::Terminal {
            name,
            repo,
            repo_ink,
            command,
            ..
        } => {
            // The tab NAME becomes the chip (#206): fujiWhite text on
            // theme-black — the inversion of an agent chip (dark text on an
            // allocated colour), which is exactly the semantics: a block no
            // agent ink has claimed. `zellij action rename-tab` is the
            // labelling mechanism; the default `Tab #N` wears the chip too.
            // The block keeps its black on the selected row (ratified).
            out.push_str(&theme.chip_ink.mix(theme.base, fade).bg());
            out.push_str(&theme.default_ink.fg());
            out.push_str(&clamp(name, widths.title));
            out.push_str(RESET);
            out.push_str(&o);
            out.push_str(&o);
            out.push(' '); // the space after the chip
            // The cwd's directory name through the same ink the agent rows
            // use — a terminal sitting in a fleet repo shares its colour
            // without either knowing about the other (lock §4). An UNMATCHED
            // cwd has no allocation, and UNTINTED read as disabled — nearly
            // invisible on the selected row (Ollie, live 2026-08-18) — so the
            // ink-less repo falls back to fujiWhite like the rest of the row.
            match repo_ink {
                Some(i) => out.push_str(&ink(hue(Some(*i), theme))),
                None => out.push_str(&ink(theme.default_ink)),
            }
            out.push_str(&clamp(repo.as_deref().unwrap_or(""), widths.repo));
            out.push_str(&o);
            out.push_str(&o);
            out.push(' '); // the space after the repo
            // UNCONDITIONALLY fujiWhite, selected or not (ratified): the
            // agent arm's carry-the-repo-ink-through rule is an agent
            // aesthetic, and on a terminal row it painted the selected
            // summary in whatever the repo cell wore — gray, when unmatched
            // (Ollie, live 2026-08-18).
            out.push_str(&ink(theme.default_ink));
            out.push_str(&clamp(command, summary_w));
            out.push_str(&o);
        }
        RowContent::Agent {
            title,
            title_ink,
            repo,
            repo_ink,
            summary,
            ..
        } => {
            // The text area opens with the title: a filled CHIP with dark text,
            // keyed per title WITHIN a repo, so two tabs of one repo never share
            // one (lock §4). Blank when the session was never renamed.
            match title {
                Some(title) => {
                    out.push_str(&hue(*title_ink, theme).mix(BASE, fade).bg());
                    out.push_str(&theme.chip_ink.fg());
                    out.push_str(&clamp(title, widths.title));
                    out.push_str(RESET);
                    out.push_str(&o);
                }
                None => {
                    out.push_str(&o);
                    out.push_str(&" ".repeat(widths.title));
                }
            }
            out.push_str(&o);
            out.push(' '); // the space after the chip
            // Tinted TEXT, keyed by repo root — one repo is one colour
            // everywhere, forever (lock §4).
            out.push_str(&ink(hue(*repo_ink, theme)));
            out.push_str(&clamp(repo, widths.repo));
            out.push_str(&o);
            out.push_str(&o);
            out.push(' '); // the space after the repo
            // The selected row leaves the summary at the repo ink
            // set on the line above — deliberate and ratified, it is visible in
            // the preview's selected row. Every OTHER row is fujiWhite, faded
            // or not: gating this on `fade > 0.0` conflated "unselected" with
            // "faded", and those come apart when nothing is selected (every row
            // fades by 0), which silently painted every summary its repo colour
            // until the moment a row was selected.
            if !row.selected {
                out.push_str(&ink(theme.default_ink));
            }
            out.push_str(&clamp(summary, summary_w));
            out.push_str(&o);
        }
    }

    out.push_str(&o);
    out.push(' '); // the right margin
    out.push_str(RESET);
    if row.selected {
        out.push_str(&theme.sel_bg.fg());
        out.push(RCAP); // the right cap, col `cols`
        out.push_str(RESET);
    } else {
        out.push(' ');
    }
    out
}

/// Right-aligned in `w` cells. The eye compares magnitudes on the RIGHT edge, so
/// that is the edge the battery cell aligns to (#105).
///
/// Never truncates, and does not need to: `token_text` is bounded at four cells
/// by construction (pinned over every `u32` by
/// `a_token_count_never_outgrows_its_cell`) and the console mark is one. A
/// caller that broke that bound would show up as a ragged row in the width
/// invariants rather than as a silently halved number.
fn rjust(s: &str, w: usize) -> String {
    let mut out = " ".repeat(w.saturating_sub(display_cells(s)));
    out.push_str(s);
    out
}

/// A token count in at most four cells (#105): thousands below a million, tenths
/// of a million above it, and a BARE `1m` rather than `1.0m` — the `.0` spends
/// half the field on a digit that carries nothing, and mixing the two shapes
/// makes the column jitter (Ollie's ruling).
///
/// ROUNDED to the nearest step, not floored: the cell is a measurement, and a
/// battery that reads `999k` at 999,600 tokens is wrong in the one direction a
/// battery must not be wrong in. Above 9.95m the decimal is dropped rather than
/// overflowing the cell (`10m`), and the whole figure saturates at `999m` — the
/// same "at least this bad" direction the ramp's own clamp takes.
fn token_text(tokens: u32) -> String {
    // Widened before the arithmetic: `u32::MAX + 500` is not a `u32`, and the
    // rounding is the whole point of doing it in the wider type.
    let tokens = u64::from(tokens);
    let thousands = (tokens + 500) / 1_000;
    if thousands < 1_000 {
        return format!("{thousands}k");
    }
    // Tenths of a million, rounded. `999_500` lands here rather than rendering a
    // five-cell `1000k`, and reads `1m`.
    let tenths = (tokens + 50_000) / 100_000;
    let (millions, tenth) = (tenths / 10, tenths % 10);
    // The decimal survives only while it fits: `9.9m` is four cells, `10.1m` is
    // five, so from ten million on the tenth is what gives way. Written against
    // `millions` rather than `tenths` so both halves of the condition are
    // load-bearing — `tenths < 100` reads the same and is unfalsifiable at its
    // own boundary, where the tenth is zero anyway (a survivor `just mutants`
    // found, and the FOOTGUNS rule: drop the redundant guard, do not test around
    // it).
    if millions < 10 && tenth != 0 {
        return format!("{millions}.{tenth}m");
    }
    // The decimal gave way, but the dropped tenth still says which whole
    // million is nearer: floor here would under-report (10,940,000 → `10m`
    // rather than the nearer `11m`), the one direction this cell must not be
    // wrong in. Saturate AFTER rounding, not before, so a round-up at 999
    // still lands on the four-cell cap rather than a five-cell `1000m`.
    let millions = millions + u64::from(tenth >= 5);
    format!("{}m", millions.min(999))
}

/// Cols 3–5: space, rule, space. The rule separates the status hue from the
/// battery hue so two adjacent coloured dots do not read as one signal.
fn push_rule(out: &mut String, o: &str, ink: &impl Fn(Rgb) -> String, default_ink: Rgb) {
    out.push_str(o);
    out.push(' ');
    out.push_str(&ink(default_ink));
    out.push(RULE);
    out.push_str(o);
    out.push(' ');
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The pre-viewport call: a pane tall enough for every row, which is the
    /// regime every golden below was written against. Viewport behaviour is
    /// asserted by the `on_screen` tests, which pass a real pane height.
    fn render_all(rows: &[Row], cols: usize, widths: Widths) -> Vec<String> {
        render_rows(rows, cols, rows.len(), widths, &Theme::default())
    }

    fn agent(status: RowStatus, provenance: Provenance, title: Option<&str>, summary: &str) -> Row {
        Row {
            content: RowContent::Agent {
                status,
                // Seven tenths spent: `md-battery_30` in yellow. Chosen so the
                // golden exercises a row where glyph and ink DISAGREE about
                // resolution — the ink has crossed one band, the glyph has moved
                // seven steps — which is the whole point of the two-axis ramp
                // (#62). A green row would assert nothing about the bands.
                battery: Some(7),
                // The count the level was bucketed from at the default 150k
                // smart zone: seven tenths of it, so the two halves of the cell
                // agree the way a real snapshot's do. Four cells wide, which is
                // the case #105 chose the column for.
                tokens: Some(105_000),
                provenance,
                title: title.map(String::from),
                title_ink: Some(5),
                repo: String::from("clave"),
                repo_ink: Some(0),
                summary: String::from(summary),
            },
            selected: false,
            // The helper mirrors the model's tier: a fixture asking for a
            // dormant status is a dormant-block row unless the test overrides.
            dormant: matches!(status, RowStatus::Dormant | RowStatus::DormantSelected),
        }
    }

    /// One of every shape the renderer has to be total over.
    fn fleet() -> Vec<Row> {
        let mut rows = vec![
            agent(
                RowStatus::NeedsYou,
                Provenance::Main,
                None,
                "I just passed the spec over",
            ),
            agent(
                RowStatus::Working,
                Provenance::Worktree,
                Some("S6-GUT"),
                "picking the gutter set",
            ),
            Row {
                content: RowContent::terminal(String::from("Tab #16")),
                selected: false,
                dormant: false,
            },
            agent(RowStatus::Stale, Provenance::Branch, Some("KDL-GRD"), "x"),
            Row {
                content: RowContent::Agent {
                    status: RowStatus::Dormant,
                    battery: None,
                    tokens: None,
                    provenance: Provenance::Main,
                    title: None,
                    title_ink: None,
                    repo: String::from("dotfiles"),
                    repo_ink: None,
                    summary: String::new(),
                },
                selected: false,
                dormant: true,
            },
        ];
        rows[1].selected = true;
        rows
    }

    /// The invariant the lock only CLAIMS in prose, and the one that matters:
    /// a row that is not exactly `cols` cells is a ragged bar.
    #[test]
    fn every_row_is_exactly_cols_cells() {
        for cols in [
            // BELOW `EXPANDED`'s `min_intact_cols()` floor (32 as of #105): the
            // row is built at the floor and clipped back, which is the regime a
            // spawning tab lands in on any window under ~123 columns. Before
            // `clip_to_cells` these widths produced full-floor rows in a
            // narrower pane, and the terminal wrapped them. `COLLAPSED_DESIGN_COLS`
            // (30) belongs to the same sub-floor regime now that the floor moved
            // 29 → 32 (#105) — it is no longer only 1/20/26 that exercise the clip.
            1,
            20,
            26,
            Widths::EXPANDED.min_intact_cols(),
            COLLAPSED_DESIGN_COLS,
            DESIGN_COLS,
            80,
            200,
        ] {
            for line in render_all(&fleet(), cols, Widths::EXPANDED) {
                let width = display_cells(&strip_sgr(&line));
                assert_eq!(width, cols, "at cols={cols}: {line:?}");
            }
        }
    }

    /// The same invariant, under `COLLAPSED` (task 1.5 / LEDGER D16, D17): one
    /// `render_row` body serving two profiles is only actually one layout if
    /// BOTH profiles hold the width guarantee at the same set of pathological
    /// `cols`, not just the profile that shipped first. `13` and `23` are
    /// `COLLAPSED`'s own floor arithmetic (`13 + 7 + 3`, see
    /// `min_intact_cols_is_the_gutter_plus_four_plus_title_plus_repo`) and `30` is D17's
    /// chosen target; the rest are shared reference points with the
    /// `EXPANDED` test above.
    #[test]
    fn every_row_is_exactly_cols_cells_under_collapsed() {
        for cols in [0, 1, 13, 23, COLLAPSED_DESIGN_COLS, DESIGN_COLS] {
            // Was `cols.max(min_intact_cols())` — the sub-floor over-run this
            // test used to PIN. `clip_to_cells` now truncates it back, so the
            // guarantee is unconditional: a row is `cols` cells at every width,
            // including the pathological ones.
            for line in render_all(&fleet(), cols, Widths::COLLAPSED) {
                let width = display_cells(&strip_sgr(&line));
                assert_eq!(width, cols, "at cols={cols}: {line:?}");
            }
        }
    }

    /// The live 2026-07-29 regression, at the width that produced it: an
    /// `EXPANDED` bar in a pane below its intact floor. The bar was built at the
    /// floor (27 then, 29 since D33), the pane was narrower, and the terminal
    /// wrapped the surplus onto a blank second line — every row double-height.
    /// Asserting `<= cols` rather
    /// than `== cols` would pass on a row of zero cells, so it asserts both.
    #[test]
    fn a_sub_floor_pane_never_receives_a_row_wider_than_itself() {
        for cols in 1..Widths::EXPANDED.min_intact_cols() {
            for line in render_all(&fleet(), cols, Widths::EXPANDED) {
                let width = display_cells(&strip_sgr(&line));
                assert_eq!(width, cols, "at cols={cols}: {line:?}");
            }
        }
    }

    /// A level past the end of the ramp SATURATES to empty-and-red; it must
    /// never fall through to the blank cell. Asserted under COLLAPSED, where the
    /// level IS the cell's content; `a_level_past_the_ramp_saturates_the_counts_ink`
    /// covers the expanded profile, where the level is only the ink (#105).
    ///
    /// Unreachable from this version — the host clamps before it sends — but
    /// the snapshot crosses a version boundary, and a newer host with a longer
    /// ramp is exactly how it becomes reachable. The direction matters: blank
    /// means "no reading", so falling through would render a row that looks
    /// FRESH for a session that is out of its zone. Saturating says "at least
    /// this bad", which is the safe way to be wrong. (CodeRabbit, #147; the
    /// arithmetic behind the clamp was surviving `just mutants` until this.)
    #[test]
    fn a_battery_level_past_the_ramp_saturates_to_empty_red() {
        let render_at = |level: u8| {
            let mut rows = fleet();
            let RowContent::Agent { battery, .. } = &mut rows[0].content else {
                panic!("fixture row 0 must be an agent");
            };
            *battery = Some(level);
            render_all(&rows, COLLAPSED_DESIGN_COLS, Widths::COLLAPSED)[0].clone()
        };

        // Compared against the LAST VALID level rather than against a literal
        // colour: unselected rows recede 25% toward the background (§6), so the
        // emitted ink is a faded red, not `RED` itself. Asserting equality with
        // the saturation target says exactly what "saturates" means and stays
        // true whatever the fade does.
        let saturated = render_at(clave_types::BATTERY_LEVELS - 1);
        assert!(
            saturated.contains(BATTERY[BATTERY.len() - 1].0),
            "the reference row must carry the empty glyph"
        );
        for level in [
            clave_types::BATTERY_LEVELS,
            clave_types::BATTERY_LEVELS + 1,
            u8::MAX,
        ] {
            assert_eq!(render_at(level), saturated, "level {level} must saturate");
        }
    }

    // ── the battery as a count (#105) ───────────────────────────────────────

    /// The same clamp, on the other side of the profile split: under EXPANDED an
    /// out-of-range level reaches the INK rather than a glyph, so it must
    /// saturate to the ramp's red instead of falling back to fujiWhite — which
    /// is what a number with no band means, and would read as "no reading" on a
    /// row that is out of its zone. (The #147 argument, moved to where the level
    /// now lands.)
    #[test]
    fn a_level_past_the_ramp_saturates_the_counts_ink() {
        let inked = |level: u8| {
            let mut row = agent(RowStatus::Working, Provenance::Main, Some("T"), "s");
            let RowContent::Agent { battery, .. } = &mut row.content else {
                panic!("fixture must be an agent");
            };
            *battery = Some(level);
            render_all(&[row], DESIGN_COLS, Widths::EXPANDED).remove(0)
        };
        let saturated = inked(clave_types::BATTERY_LEVELS - 1);
        assert!(saturated.contains(&format!("{}105k", RED.fg())));
        for level in [clave_types::BATTERY_LEVELS, u8::MAX] {
            assert_eq!(inked(level), saturated, "level {level} must saturate");
        }
    }

    /// The four-character range from the ticket, which is the whole reason four
    /// columns is the right size: every reading a real conversation produces
    /// fits it. `1.0m` is deliberately NOT one of the shapes — mixing it with
    /// `400k` would leave the field jittering between two widths of million.
    #[test]
    fn a_token_count_reads_as_thousands_or_millions() {
        for (tokens, want) in [
            // A row that just `/clear`ed is genuinely near zero, and says so
            // rather than rounding itself up into a reading it has not spent.
            (0, "0k"),
            (1_000, "1k"),
            (105_000, "105k"),
            (400_000, "400k"),
            (999_000, "999k"),
            (1_000_000, "1m"),
            (1_100_000, "1.1m"),
        ] {
            assert_eq!(token_text(tokens), want, "{tokens} tokens");
        }
    }

    /// Rounding, and the two boundaries the branches meet at. Floored counts
    /// under-report, and under-reporting is the one direction a battery must not
    /// be wrong in.
    #[test]
    fn a_token_count_rounds_to_its_nearest_step() {
        assert_eq!(token_text(1_499), "1k");
        assert_eq!(token_text(1_500), "2k");
        // A hair under a million rounds INTO millions rather than rendering a
        // five-cell `1000k` that would break the row.
        assert_eq!(token_text(999_499), "999k");
        assert_eq!(token_text(999_500), "1m");
        // A whole million reads bare from either side of it.
        assert_eq!(token_text(1_049_999), "1m");
        assert_eq!(token_text(1_050_000), "1.1m");
        // Past 9.95m the DECIMAL gives way, not the cell: `10.1m` would be five
        // columns, so double-digit millions round to whole ones.
        assert_eq!(token_text(9_949_999), "9.9m");
        assert_eq!(token_text(9_950_000), "10m");
        assert_eq!(token_text(10_100_000), "10m");
        // Past ten million the digit that gave way still decides which whole
        // million is nearer: floor here under-reports (10,940,000 is nearer
        // 11m than 10m), the one direction the doc forbids.
        assert_eq!(token_text(10_940_000), "11m");
        // And the whole figure saturates rather than overflowing the cell — the
        // ramp's own "at least this bad" direction.
        assert_eq!(token_text(u32::MAX), "999m");
    }

    /// #105's deliverable, both halves of it: expanded prints the count in the
    /// ramp's ink, collapsed keeps the glyph. One row, two profiles, so the
    /// profile is the only thing that differs.
    #[test]
    fn expanded_renders_the_count_and_collapsed_keeps_the_glyph() {
        let row = agent(RowStatus::Working, Provenance::Main, Some("T"), "s");
        let cell = |widths: Widths, cols| {
            let bare = strip_sgr(&render_all(std::slice::from_ref(&row), cols, widths)[0]);
            cell_slice(&bare, 5, 5 + widths.battery.width())
        };
        assert_eq!(cell(Widths::EXPANDED, DESIGN_COLS), "105k");
        assert_eq!(
            cell(Widths::COLLAPSED, COLLAPSED_DESIGN_COLS),
            BATTERY[7].0.to_string()
        );
    }

    /// Right-aligned: the eye compares magnitudes on the right edge, so a short
    /// reading pads on the LEFT and the units character never moves column.
    #[test]
    fn a_short_count_pads_on_the_left() {
        let mut row = agent(RowStatus::Working, Provenance::Main, Some("T"), "s");
        let RowContent::Agent { tokens, .. } = &mut row.content else {
            panic!("fixture must be an agent");
        };
        *tokens = Some(1_000);
        let bare = strip_sgr(&render_all(&[row], DESIGN_COLS, Widths::EXPANDED)[0]);
        assert_eq!(cell_slice(&bare, 5, 9), "  1k");
    }

    /// The INK is the ramp's, tracked per level rather than fixed: the digits
    /// carry the magnitude and the colour carries the risk band, which is the
    /// two-axis reading #62 built and #105 keeps.
    #[test]
    fn the_count_is_inked_by_the_ramp_band() {
        let inked = |level: u8| {
            let mut row = agent(RowStatus::Working, Provenance::Main, Some("T"), "s");
            let RowContent::Agent {
                battery, tokens, ..
            } = &mut row.content
            else {
                panic!("fixture must be an agent");
            };
            *battery = Some(level);
            *tokens = Some(45_000);
            render_all(&[row], DESIGN_COLS, Widths::EXPANDED).remove(0)
        };
        // Nothing is selected, so the fade is zero and the band's own hue is
        // what reaches the row, immediately before the digits.
        for (level, band) in [(1, GREEN), (7, YELLOW), (9, ORANGE), (10, RED)] {
            let want = format!("{} 45k", band.fg());
            assert!(inked(level).contains(&want), "level {level} lost its band");
        }
    }

    /// An unknown count renders BLANK, and stays blank even when a level came
    /// through: the ramp index is a bucket, not a token figure, and printing a
    /// number derived from it would be inventing a measurement (#105's ruling,
    /// and `agent_content`'s rule).
    #[test]
    fn an_unknown_count_blanks_the_cell_whatever_the_level_says() {
        for level in [None, Some(0), Some(7)] {
            let mut row = agent(RowStatus::Working, Provenance::Main, Some("T"), "s");
            let RowContent::Agent {
                battery, tokens, ..
            } = &mut row.content
            else {
                panic!("fixture must be an agent");
            };
            *battery = level;
            *tokens = None;
            let bare = strip_sgr(&render_all(&[row], DESIGN_COLS, Widths::EXPANDED)[0]);
            assert_eq!(cell_slice(&bare, 5, 9), "    ", "level {level:?}");
        }
    }

    /// A count with no level is unreachable from any host — the level is
    /// bucketed FROM the count — but the wire crosses a version boundary, so the
    /// two halves of the cell stay independently total: the figure still prints,
    /// in fujiWhite, which claims no risk band it does not have.
    #[test]
    fn a_count_with_no_level_still_prints_in_the_default_ink() {
        let mut row = agent(RowStatus::Working, Provenance::Main, Some("T"), "s");
        let RowContent::Agent {
            battery, tokens, ..
        } = &mut row.content
        else {
            panic!("fixture must be an agent");
        };
        *battery = None;
        *tokens = Some(45_000);
        let line = &render_all(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED)[0];
        assert_eq!(cell_slice(&strip_sgr(line), 5, 9), " 45k");
        assert!(
            line.contains(&format!("{} 45k", DEFAULT_INK.fg())),
            "{line:?}"
        );
    }

    /// The gutter is the eight position-locked columns plus the battery cell, so
    /// #105 moves the text area three columns right under `EXPANDED` and leaves
    /// `COLLAPSED` exactly where D16 put it. What D16 still guarantees is
    /// everything LEFT of the battery: cap, status, rule — identical columns in
    /// both profiles, which is why the two are one layout and not two.
    #[test]
    fn the_gutter_widens_with_the_battery_cell() {
        assert_eq!(BatteryCell::Glyph.width(), 1);
        assert_eq!(BatteryCell::Count.width(), 4);
        assert_eq!(Widths::EXPANDED.gutter(), 8 + 4);
        assert_eq!(Widths::COLLAPSED.gutter(), 8 + 1);
        assert_eq!(
            Widths::COLLAPSED.gutter(),
            9,
            "the pre-#105 gutter, unmoved"
        );

        let row = agent(RowStatus::Working, Provenance::Branch, Some("T"), "s");
        let head = |widths: Widths, cols| {
            let bare = strip_sgr(&render_all(std::slice::from_ref(&row), cols, widths)[0]);
            cell_slice(&bare, 0, 5)
        };
        assert_eq!(
            head(Widths::EXPANDED, DESIGN_COLS),
            head(Widths::COLLAPSED, COLLAPSED_DESIGN_COLS)
        );
    }

    /// The console mark sits in the STATUS cell (#206) — the glyph is the
    /// row's kind, its colour the state — and the battery cell carries the
    /// class marker instead: `TERM` filling the expanded cell to the digits'
    /// right edge, the prompt glyph in the one-column collapsed cell.
    #[test]
    fn a_terminal_marks_the_status_cell_and_says_term_in_the_battery_cell() {
        let row = Row {
            content: RowContent::terminal(String::from("Tab #16")),
            selected: false,
            dormant: false,
        };
        let bare =
            strip_sgr(&render_all(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED)[0]);
        assert_eq!(cell_slice(&bare, 1, 2), CONSOLE.to_string());
        assert_eq!(cell_slice(&bare, 5, 9), TERM_MARK);
        let bare = strip_sgr(
            &render_all(
                std::slice::from_ref(&row),
                COLLAPSED_DESIGN_COLS,
                Widths::COLLAPSED,
            )[0],
        );
        assert_eq!(cell_slice(&bare, 1, 2), CONSOLE.to_string());
        assert_eq!(cell_slice(&bare, 5, 6), TERM_GLYPH.to_string());
    }

    /// A POPULATED terminal row (#206) — the goldens and the test above both
    /// use `RowContent::terminal`'s idle defaults, which left three of the
    /// four status inks and the repo fallback unasserted. The console mark's
    /// ink is the state; the repo cell wears its allocated ink when matched
    /// and falls back to fujiWhite when not — never UNTINTED, which read as
    /// disabled on the selected row (Ollie, live 2026-08-18).
    #[test]
    fn a_populated_terminal_row_inks_its_status_and_repo() {
        let row = |status: TermStatus, repo_ink: Option<u8>| Row {
            content: RowContent::Terminal {
                name: String::from("Tab #16"),
                status,
                provenance: Provenance::Main,
                repo: Some(String::from("clave")),
                repo_ink,
                command: String::from("cargo test"),
            },
            selected: false,
            dormant: false,
        };
        for (status, band) in [
            (TermStatus::Idle, DEFAULT_INK),
            (TermStatus::Running, Rgb(0xFF, 0x9E, 0x3B)),
            (TermStatus::Done, Rgb(0x98, 0xBB, 0x6C)),
            (TermStatus::Failed, Rgb(0xE8, 0x24, 0x24)),
        ] {
            let line = &render_all(&[row(status, Some(0))], DESIGN_COLS, Widths::EXPANDED)[0];
            assert!(
                line.contains(&format!("{}{CONSOLE}", band.fg())),
                "{status:?} lost its ink"
            );
        }
        let inked = &render_all(
            &[row(TermStatus::Idle, Some(0))],
            DESIGN_COLS,
            Widths::EXPANDED,
        )[0];
        assert!(inked.contains(&format!("{}clave", PALETTE[0].0.fg())));
        let bare = &render_all(
            &[row(TermStatus::Idle, None)],
            DESIGN_COLS,
            Widths::EXPANDED,
        )[0];
        assert!(bare.contains(&format!("{}clave", DEFAULT_INK.fg())));
        assert!(!bare.contains(&format!("{}clave", UNTINTED.fg())));
        assert!(strip_sgr(bare).contains("cargo test"));
    }

    /// The clip's boundary is inclusive on the PASS-THROUGH side: a pane at
    /// exactly the floor is already `cols` cells, so it must not be re-walked.
    ///
    /// Pinned byte-for-byte because a width assertion cannot see this — at
    /// `cols == min_intact_cols()` clipping and not clipping produce the same
    /// number of cells, differing only by a redundant trailing `RESET`. Mutation
    /// testing found exactly that hole: `<` → `<=` at the branch survived the
    /// entire suite. One cell below the floor the two must differ.
    #[test]
    fn the_floor_itself_is_not_clipped() {
        let floor = Widths::EXPANDED.min_intact_cols();
        let rows = fleet();
        let any = rows.iter().any(|r| r.selected);

        for (clipped, direct) in render_all(&rows, floor, Widths::EXPANDED).iter().zip(
            rows.iter()
                .map(|r| render_row(r, floor, Widths::EXPANDED, any, &Theme::default())),
        ) {
            assert_eq!(*clipped, direct, "the floor took the clip path");
        }

        let under = floor - 1;
        let clipped = &render_all(&rows, under, Widths::EXPANDED)[0];
        let direct = render_row(&rows[0], under, Widths::EXPANDED, any, &Theme::default());
        assert_ne!(
            *clipped, direct,
            "one cell below the floor the clip must engage"
        );
    }

    /// A clip that dropped SGR would leave the row exactly `cols` cells and
    /// still be wrong — the colour state would stay open and bleed into
    /// whatever the terminal drew next. So the width assertion alone cannot
    /// carry this; the reset must be checked directly.
    #[test]
    fn clipping_keeps_the_colour_it_cuts_through_and_closes_it() {
        let painted = format!("{}abcdef{}", CHIP_INK.fg(), RESET);
        let clipped = clip_to_cells(&painted, 3);
        assert_eq!(strip_sgr(&clipped), "abc");
        assert!(
            clipped.contains(&CHIP_INK.fg()),
            "the SGR the cut passed through was dropped: {clipped:?}"
        );
        assert!(
            clipped.ends_with(RESET) || clipped.contains(RESET),
            "colour left open past the end of the row: {clipped:?}"
        );
    }

    /// `chars().count()` would cut a two-cell glyph in half and report success.
    /// The glyph is excluded and the row padded back, so the width holds
    /// without half a character on screen.
    #[test]
    fn clipping_excludes_a_wide_glyph_that_straddles_the_boundary() {
        // U+1F600 is two cells; it cannot fit in one and must not be halved.
        assert_eq!(strip_sgr(&clip_to_cells("\u{1f600}", 1)), " ");
        assert_eq!(strip_sgr(&clip_to_cells("\u{1f600}", 2)), "\u{1f600}");
    }

    /// Selection must not move a single column (lock §2.2) — the cap columns
    /// are reserved on every row precisely so the eye scans one edge. Checked
    /// under BOTH width profiles (LEDGER D16): one `render_row` body serves
    /// both, so the chip must start at its profile's first text column whether
    /// or not the row is selected.
    ///
    /// That column used to be 10 in both, and #105 moved `EXPANDED`'s to 13 by
    /// widening the battery cell — so the assertion is derived from
    /// `widths.gutter()` rather than pinned at a number, which is what makes it
    /// a claim about selection rather than about the width of the day.
    #[test]
    fn the_chip_starts_at_the_first_text_column_under_both_profiles() {
        for widths in [Widths::EXPANDED, Widths::COLLAPSED] {
            let unselected = agent(RowStatus::Working, Provenance::Branch, Some("S6-GUT"), "x");
            let selected = Row {
                selected: true,
                dormant: false,
                ..unselected.clone()
            };
            // What the chip cell holds at this profile's title width —
            // IDENTICAL under `COLLAPSED`: D17 holds title at 7, matching
            // `EXPANDED` (the chip is what a tab is identified by), starting
            // at the same column either way.
            let chip = clamp("S6-GUT", widths.title);
            for row in [unselected, selected] {
                let bare =
                    strip_sgr(&render_all(std::slice::from_ref(&row), DESIGN_COLS, widths)[0]);
                assert_eq!(
                    cell_slice(&bare, widths.gutter(), widths.gutter() + widths.title),
                    chip,
                    "selected={} widths={widths:?}",
                    row.selected
                );
            }
        }
    }

    /// LEDGER D16's own arithmetic, pinned by equality rather than trusted from
    /// prose: the gutter, plus the space after title, the space after repo, the
    /// right margin and the right cap — everything fixed that is not title or
    /// repo itself (D12, generalised to a profile by D16). The `13` this test
    /// used to spell is now `gutter() + 4`, because #105 made the gutter a
    /// property of the profile rather than a constant.
    #[test]
    fn min_intact_cols_is_the_gutter_plus_four_plus_title_plus_repo() {
        assert_eq!(Widths::EXPANDED.min_intact_cols(), 12 + 4 + 9 + 7);
        assert_eq!(Widths::EXPANDED.min_intact_cols(), 32); // 29 before #105, 27 before D19
        assert_eq!(Widths::COLLAPSED.min_intact_cols(), 9 + 4 + 7 + 3);
        assert_eq!(Widths::COLLAPSED.min_intact_cols(), 23); // D17, unchanged by #105
    }

    /// A missing glyph renders a blank cell and does not reflow the row (lock
    /// §2.1). A main checkout is the deliberate blank, and it is the most
    /// common row — so if absence reflowed, most of the bar would be ragged.
    #[test]
    fn an_absent_glyph_blanks_its_cell_without_reflowing() {
        let main = agent(RowStatus::Idle, Provenance::Main, Some("TITLE"), "s");
        let worktree = agent(RowStatus::Idle, Provenance::Worktree, Some("TITLE"), "s");
        let [main, worktree] = [main, worktree]
            .map(|r| strip_sgr(&render_all(&[r], DESIGN_COLS, Widths::EXPANDED)[0]));

        // Indexed in CELLS, and DERIVED: the provenance cell is the second-last
        // column of the gutter, whatever the battery cell before it costs (#105).
        // A `chars()` index only agrees while every glyph to its left is one cell
        // wide, which is exactly the assumption this module exists to drop.
        let provenance = Widths::EXPANDED.gutter() - 2;
        assert_eq!(
            cell_slice(&main, provenance, provenance + 1),
            " ",
            "the provenance cell is blank for a main checkout"
        );
        assert_eq!(
            cell_slice(&worktree, provenance, provenance + 1),
            "\u{168c2}"
        );
        // Same width, same text origin: the blank cost exactly one column.
        assert_eq!(display_cells(&main), display_cells(&worktree));
        let text = Widths::EXPANDED.gutter();
        assert_eq!(
            cell_slice(&main, text, DESIGN_COLS),
            cell_slice(&worktree, text, DESIGN_COLS)
        );
    }

    /// Track the active background per CELL, so "spans all columns" is a
    /// measurement rather than an assertion. `None` = terminal default.
    fn cell_backgrounds(line: &str) -> Vec<Option<String>> {
        let mut out = Vec::new();
        let mut bg: Option<String> = None;
        let mut rest = line;
        while let Some(i) = rest.find("\u{1b}[") {
            for ch in rest[..i].chars() {
                for _ in 0..ch.width().unwrap_or(0) {
                    out.push(bg.clone());
                }
            }
            let tail = &rest[i + 2..];
            let j = tail.find('m').expect("SGR");
            let params = &tail[..j];
            if params == "0" {
                bg = None;
            } else if let Some(rgb) = params.strip_prefix("48;2;") {
                bg = Some(rgb.to_string());
            }
            rest = &tail[j + 1..];
        }
        for ch in rest.chars() {
            for _ in 0..ch.width().unwrap_or(0) {
                out.push(bg.clone());
            }
        }
        out
    }

    /// Lock §6, verbatim: "a row background must span all 44 columns, including
    /// the pad after a short summary — resetting at end-of-text leaves a ragged
    /// selection. Worth a test." (The lock says 44 because it predates D19; the
    /// assertions below read `DESIGN_COLS`, so the property is "all columns",
    /// whatever the width is. Quote left as written rather than silently
    /// re-numbered.)
    #[test]
    fn a_selected_rows_background_spans_every_column() {
        let mut row = agent(RowStatus::Working, Provenance::Worktree, Some("T"), "short");
        row.selected = true;
        let line = &render_all(&[row], DESIGN_COLS, Widths::EXPANDED)[0];
        let bgs = cell_backgrounds(line);
        assert_eq!(bgs.len(), DESIGN_COLS);

        let sel = Some(String::from("45;79;103"));
        // Cols 1 and DESIGN_COLS are the caps: the half-circle is a FOREGROUND glyph on
        // the default background, which is what makes the row read as rounded.
        assert_eq!(bgs[0], None);
        assert_eq!(bgs[DESIGN_COLS - 1], None);
        for (i, bg) in bgs.iter().enumerate().take(DESIGN_COLS - 1).skip(1) {
            assert!(bg.is_some(), "col {} lost its background", i + 1);
        }
        // Everything but the chip is the selection colour, and the trailing pad
        // after a 5-cell summary is included.
        let chip = Widths::EXPANDED.gutter()..Widths::EXPANDED.gutter() + Widths::EXPANDED.title;
        for (i, bg) in bgs.iter().enumerate().take(DESIGN_COLS - 1).skip(1) {
            if chip.contains(&i) {
                continue;
            }
            assert_eq!(bg, &sel, "col {}", i + 1);
        }
    }

    /// A `chars().count()` clamp is wrong by one column per wide glyph. `main.rs`
    /// used to do exactly that; this renderer replaced it, and this test is what
    /// pins the replacement to cells rather than code points.
    #[test]
    fn truncation_is_cell_correct_not_char_correct() {
        // Eight W-width CJK chars = 16 cells in 8 code points.
        let wide = "\u{4f60}\u{597d}\u{4e16}\u{754c}\u{4f60}\u{597d}\u{4e16}\u{754c}";
        let out = clamp(wide, 7);
        assert_eq!(display_cells(&out), 7);
        // 3 wide glyphs (6 cells) + ellipsis: a char-count clamp would have
        // taken 6 and over-run the column by 5.
        assert_eq!(out, "\u{4f60}\u{597d}\u{4e16}\u{2026}");
        assert_eq!(out.chars().count(), 4);

        // The odd-cell case: the 4th glyph would straddle the ellipsis' cell,
        // so it is dropped whole and the column pads instead.
        let out = clamp(wide, 8);
        assert_eq!(display_cells(&out), 8);
        assert_eq!(out, "\u{4f60}\u{597d}\u{4e16}\u{2026} ");
    }

    /// LEDGER D18: the ellipsis is suppressed at 4 cells or fewer, and kept
    /// above that — both sides of the boundary pinned, not just the collapsed
    /// `repo = 3` case D17 needed it for. At `w = 4` an ellipsis would spend a
    /// quarter of the field on a glyph that tells the reader nothing they
    /// cannot already see; at `w = 5` it still earns its cell.
    #[test]
    fn clamp_suppresses_the_ellipsis_at_four_cells_or_fewer() {
        // 5 characters into 4 cells: hard truncate, no ellipsis.
        assert_eq!(clamp("clave", 4), "clav");
        // 6 characters into 5 cells: one cell over the boundary, and the
        // ellipsis returns.
        assert_eq!(clamp("clavex", 5), "clav\u{2026}");
    }

    /// Degenerate widths must not panic. Below `min_intact_cols()` the fixed
    /// columns hold and the row is BUILT wider than the pane (LEDGER D9) —
    /// recorded here so the behaviour is a decision, not a surprise.
    ///
    /// This used to add "not the mechanism a user ever sees — the collapsed
    /// profile is chosen by STATE, so a live seek never crosses this floor."
    /// **That was false, and the live run of 2026-07-29 falsified it.** The
    /// seek is not the only thing that sets the width: a tab is BORN at the
    /// birth percent, which on any window under ~123 columns lands the pane
    /// below `EXPANDED`'s floor while the state still says expanded. Ollie hit
    /// it on every tab spawn and on a monitor change.
    ///
    /// So the row is still built at the floor — no fixed column reflows — but
    /// `render_rows` now clips the result back to `cols`. Pinned to equality,
    /// not bounded: a future change that silently reflows a fixed column
    /// downward to fit should fail here rather than be tolerated by a `<=`.
    /// The fleet mixes agent and terminal rows on purpose — the clip has to be
    /// UNIFORM across row kinds, or the bar goes ragged instead of clipped.
    #[test]
    fn degenerate_widths_do_not_panic() {
        for cols in [
            0,
            1,
            9,
            20,
            Widths::EXPANDED.min_intact_cols(),
            DESIGN_COLS,
            200,
        ] {
            for (i, line) in render_all(&fleet(), cols, Widths::EXPANDED)
                .iter()
                .enumerate()
            {
                let width = display_cells(&strip_sgr(line));
                assert_eq!(width, cols, "row {i} at cols={cols}");
            }
        }
        assert_eq!(clamp("anything", 0), "");
    }

    /// The picture, not a fragment. A diff here is a deliberate design change
    /// or a bug, and both want a human looking.
    ///
    /// The expected value traces to the LOCK, not to the code that produced it
    /// — check it against design-lock §2 rather than against `render_rows`:
    ///
    /// - Stripped of SGR, each row is
    ///   `1+1+1+1+1+4+1+1+1 + 9 + 1 + 7 + 1 + 22 + 1 + 1 = 54` cells: the
    ///   twelve-cell gutter (§2.1, with #105's four-column battery cell), title,
    ///   space, repo, space, summary, right margin, right cap. (`9 + 1 + 7 + 1 +
    ///   25` behind a nine-cell gutter before #105; `7 + 1 + 7 + 1 + 17` = 44
    ///   before D19.)
    /// - Row 1 has no title, so cols 13–21 are blank; `clave` is padded to 7 at
    ///   cols 23–29. Row 3 is a terminal (#206): the console mark in the status
    ///   cell, `TERM` in the battery cell, the tab name as a chip on sumiInk0,
    ///   and a blank repo/summary — nothing is known about its pane yet.
    /// - The battery cell is cols 6–9, right-aligned: `105k`, seven tenths of the
    ///   default smart zone, in the ramp's yellow band (#105).
    /// - The hues are crystalBlue `#7E9CD8`, waveRed `#E46876`, carpYellow
    ///   `#E6C384` and fujiWhite `#DCD7BA` (§4), each mixed 25% toward sumiInk3
    ///   `#1F1F28` on the unselected rows (§6) — e.g. waveRed's red is
    ///   `228 + (31 - 228) * 0.25 = 178.75 -> 179`.
    /// - The selected row is the only one carrying `48;2;45;79;103`
    ///   (waveBlue2 `#2D4F67`), its caps (§2.2) and unfaded hues.
    ///
    /// Regenerate with `cargo run -p clave-bar --example bar-preview` only
    /// AFTER confirming the change against the lock.
    #[test]
    fn golden_bar_at_fifty_four_columns() {
        let rows = vec![
            agent(
                RowStatus::NeedsYou,
                Provenance::Main,
                None,
                "I just passed the spec over",
            ),
            Row {
                selected: true,
                dormant: false,
                ..agent(
                    RowStatus::Working,
                    Provenance::Worktree,
                    Some("S6-GUT"),
                    "picking the gutter set",
                )
            },
            Row {
                content: RowContent::terminal(String::from("Tab #16")),
                selected: false,
                dormant: false,
            },
        ];
        let expected = [
            " \u{1b}[38;2;179;86;98m\u{25cf} \u{1b}[38;2;173;169;150m\u{2502} \u{1b}[38;2;180;154;109m105k             \u{1b}[38;2;102;125;172mclave   \u{1b}[38;2;173;169;150mI just passed the spe\u{2026} \u{1b}[0m ",
            "\u{1b}[38;2;45;79;103m\u{e0b6}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m\u{1b}[38;2;255;158;59m\u{25cf}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[38;2;220;215;186m\u{2502}\u{1b}[48;2;45;79;103m \u{1b}[48;2;45;79;103m\u{1b}[38;2;230;195;132m105k\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[48;2;45;79;103m\u{1b}[38;2;126;156;216m\u{168c2}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[48;2;122;168;159m\u{1b}[38;2;22;22;29mS6-GUT   \u{1b}[0m\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[38;2;126;156;216mclave  \u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m picking the gutter set\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[0m\u{1b}[38;2;45;79;103m\u{e0b4}\u{1b}[0m",
            " \u{1b}[38;2;173;169;150m\u{f018d} \u{1b}[38;2;173;169;150m\u{2502} \u{1b}[38;2;173;169;150mTERM   \u{1b}[48;2;24;24;32m\u{1b}[38;2;220;215;186mTab #16  \u{1b}[0m \u{1b}[38;2;173;169;150m        \u{1b}[38;2;173;169;150m                       \u{1b}[0m ",
        ];
        assert_eq!(render_all(&rows, DESIGN_COLS, Widths::EXPANDED), expected);
        // The same derived self-checks the COLLAPSED golden carries. A golden
        // is only as good as its regeneration ritual, and this is the more
        // load-bearing of the two — everything in the design was chosen at this width.
        // These re-derive the column map from `Widths::EXPANDED` rather than
        // reading it off the string above, so a golden regenerated from a
        // renderer that moved a column fails HERE, in arithmetic traceable to
        // lock §2, instead of being accepted as the new picture.
        for line in &expected {
            assert_eq!(display_cells(&strip_sgr(line)), DESIGN_COLS);
        }
        let w = Widths::EXPANDED;
        let battery = |line: &str| cell_slice(&strip_sgr(line), 5, 5 + w.battery.width());
        let title = |line: &str| cell_slice(&strip_sgr(line), w.gutter(), w.gutter() + w.title);
        let repo = |line: &str| {
            cell_slice(
                &strip_sgr(line),
                w.gutter() + w.title + 1,
                w.gutter() + w.title + 1 + w.repo,
            )
        };
        // The count, not the glyph, and it starts where the glyph used to (#105).
        assert_eq!(battery(expected[0]), "105k");
        assert_eq!(battery(expected[1]), "105k");
        assert_eq!(battery(expected[2]), TERM_MARK);
        // Row 1 has no title: nine blank cells, and the repo still starts
        // exactly one space later (an absent chip must not pull the row left).
        assert_eq!(title(expected[0]), " ".repeat(w.title));
        assert_eq!(repo(expected[0]), "clave  ");
        // Row 2's chip is NINE cells since D19, and therefore no longer
        // byte-identical to the COLLAPSED golden's seven. That anchor property
        // was D17's, and Ollie retired it knowingly when he took the title to 9
        // — see `Widths::COLLAPSED`. Its repo is the full 7-cell field.
        assert_eq!(title(expected[1]), "S6-GUT   ");
        assert_eq!(repo(expected[1]), "clave  ");
        // Summary runs from after the repo to the right margin: `cols - gutter -
        // 4 - title - repo` = 22 cells (D16's formula, with #105's wider gutter —
        // 25 before it, and the three columns came from here because summary is
        // the only flexible cell there is, D9), ellipsis included.
        let summary_start = w.gutter() + w.title + 1 + w.repo + 1;
        assert_eq!(
            cell_slice(&strip_sgr(expected[0]), summary_start, DESIGN_COLS - 2),
            "I just passed the spe\u{2026}"
        );
        assert_eq!(DESIGN_COLS - 2 - summary_start, 22);
    }

    /// The same picture, one layout, the other profile (LEDGER D16, D17):
    /// same three rows as `golden_bar_at_fifty_four_columns`,
    /// `Widths::COLLAPSED` at `cols = 30` — Ollie's chosen collapsed profile.
    ///
    /// Column arithmetic, derived from D16's formula (`summary = cols - 13 -
    /// title - repo`), not pasted from whatever the code emits: at `title =
    /// 7, repo = 3` (D17) that is `summary_w = 30 - 13 - 7 - 3 = 7`. Laid out:
    /// `1 cap + 8 gutter (+1 gutter space = 9) + 7 title + 1 + 3 repo + 1 + 7
    /// summary + 1 margin + 1 cap = 30`. Title still starts at column 10 —
    /// cols 1–9 are the gutter, unchanged from `EXPANDED` (D16's whole
    /// point), and title HOLDS at 7 rather than shrinking (D17), so the chip
    /// itself is byte-identical to the `EXPANDED` golden's `S6-GUT ` before
    /// hitting the repo column.
    ///
    /// `repo = 3` is the case D18 exists for: `clamp` at 4 cells or fewer
    /// drops the ellipsis, so `"clave"` truncates to `"cla"`, not `"cl\u{2026}"` —
    /// spending one of three cells on a glyph that tells the reader nothing
    /// would have defeated the entire reason 3 was chosen. `summary = 7` is
    /// above the D18 threshold, so it keeps its ellipsis exactly as `title`
    /// and `repo` do at `EXPANDED`'s wider columns.
    #[test]
    fn golden_bar_collapsed_at_thirty_columns() {
        let rows = vec![
            agent(
                RowStatus::NeedsYou,
                Provenance::Main,
                None,
                "I just passed the spec over",
            ),
            Row {
                selected: true,
                dormant: false,
                ..agent(
                    RowStatus::Working,
                    Provenance::Worktree,
                    Some("S6-GUT"),
                    "picking the gutter set",
                )
            },
            Row {
                content: RowContent::terminal(String::from("Tab #16")),
                selected: false,
                dormant: false,
            },
        ];
        let expected = [
            " \u{1b}[38;2;179;86;98m\u{25cf} \u{1b}[38;2;173;169;150m\u{2502} \u{1b}[38;2;180;154;109m\u{f007c}           \u{1b}[38;2;102;125;172mcla \u{1b}[38;2;173;169;150mI just\u{2026} \u{1b}[0m ",
            "\u{1b}[38;2;45;79;103m\u{e0b6}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m\u{1b}[38;2;255;158;59m\u{25cf}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[38;2;220;215;186m\u{2502}\u{1b}[48;2;45;79;103m \u{1b}[48;2;45;79;103m\u{1b}[38;2;230;195;132m\u{f007c}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[48;2;45;79;103m\u{1b}[38;2;126;156;216m\u{168c2}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[48;2;122;168;159m\u{1b}[38;2;22;22;29mS6-GUT \u{1b}[0m\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[38;2;126;156;216mcla\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m pickin\u{2026}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[0m\u{1b}[38;2;45;79;103m\u{e0b4}\u{1b}[0m",
            " \u{1b}[38;2;173;169;150m\u{f018d} \u{1b}[38;2;173;169;150m\u{2502} \u{1b}[38;2;173;169;150m\u{f120}   \u{1b}[48;2;24;24;32m\u{1b}[38;2;220;215;186mTab #16\u{1b}[0m \u{1b}[38;2;173;169;150m    \u{1b}[38;2;173;169;150m        \u{1b}[0m ",
        ];
        assert_eq!(
            render_all(&rows, COLLAPSED_DESIGN_COLS, Widths::COLLAPSED),
            expected
        );
        for line in &expected {
            assert_eq!(display_cells(&strip_sgr(line)), COLLAPSED_DESIGN_COLS);
        }
        // Title still starts at column 10 (cell index 9) — this profile's gutter
        // did not move, and D17 holds title at 7, so the chip itself is untouched
        // by the profile; only repo and summary narrowed. #105 widened the
        // EXPANDED gutter alone, which is why this is now the profile whose text
        // area starts where BOTH used to.
        let w = Widths::COLLAPSED;
        assert_eq!(w.gutter(), 9);
        assert_eq!(
            cell_slice(&strip_sgr(expected[1]), w.gutter(), w.gutter() + w.title),
            "S6-GUT "
        );
        // The glyph, not the count: the battery cell is one column here (#105).
        assert_eq!(
            cell_slice(&strip_sgr(expected[1]), 5, 6),
            BATTERY[7].0.to_string()
        );
    }

    #[test]
    fn strip_sgr_leaves_a_non_sgr_sequence_alone() {
        assert_eq!(strip_sgr("\u{1b}[0mx\u{1b}[38;2;1;2;3my"), "xy");
        assert_eq!(strip_sgr("\u{1b}[2Jx"), "\u{1b}[2Jx");
    }

    /// Recession is relative (lock §6): with nothing selected there is nothing
    /// to recede from, so an unfocused bar renders at full strength.
    #[test]
    fn nothing_selected_means_nothing_faded() {
        let row = agent(RowStatus::Done, Provenance::Branch, None, "s");
        let unfocused =
            render_all(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED).remove(0);
        let mut other = agent(RowStatus::Idle, Provenance::Main, None, "s");
        other.selected = true;
        let faded = &render_all(&[row, other], DESIGN_COLS, Widths::EXPANDED)[0];

        // springGreen at full strength, then faded 25% toward sumiInk3.
        assert!(unfocused.contains("\u{1b}[38;2;152;187;108m"));
        assert!(faded.contains("\u{1b}[38;2;122;148;91m"));
    }

    /// A dormant row's fade is ABSOLUTE (#206): deep toward the bar background
    /// whether anything is selected or not, where live rows fade only
    /// relatively (lock §6). The tell was previously the hollow glyph alone.
    #[test]
    fn a_dormant_row_dims_regardless_of_selection() {
        let dormant = agent(RowStatus::Dormant, Provenance::Main, Some("T"), "s");
        // fujiWhite `#DCD7BA` mixed 60% toward sumiInk3 `#1F1F28`:
        // (106.6 -> 107, 104.6 -> 105, 98.4 -> 98).
        let half = "\u{1b}[38;2;107;105;98m";
        // The chip is the one BACKGROUND the fade touches (CodeRabbit, #210).
        let chip_faded = hue(Some(5), &Theme::default()).mix(BASE, DORMANT_FADE).bg();
        let chip_full = hue(Some(5), &Theme::default()).bg();
        let alone = render_all(
            std::slice::from_ref(&dormant),
            DESIGN_COLS,
            Widths::EXPANDED,
        )
        .remove(0);
        assert!(alone.contains(half), "unselected bar: dormant still dims");
        assert!(
            !alone.contains("\u{1b}[38;2;220;215;186m"),
            "no full-strength fujiWhite anywhere on a dormant row"
        );
        assert!(alone.contains(&chip_faded) && !alone.contains(&chip_full));
        let mut live = agent(RowStatus::Working, Provenance::Main, None, "s");
        live.selected = true;
        let beside = &render_all(&[dormant, live], DESIGN_COLS, Widths::EXPANDED)[0];
        assert!(
            beside.contains(half) && !beside.contains("\u{1b}[38;2;173;169;150m"),
            "a selection must not lift dormant to the shallower relative fade"
        );
    }

    /// The fade keys off BLOCK membership, not the glyph: `stale` outranks
    /// `Dormant` in `agent_content`'s tier, so a dormant row whose cwd
    /// vanished wears ✗ — and must still recede (Codex, #210). `Opening` is
    /// the deliberate exception: the row the user just launched is
    /// mid-transition to live and reads at full strength (⏎ → ↻, #100).
    #[test]
    fn stale_dormant_dims_but_an_opening_row_does_not() {
        let mut stale = agent(RowStatus::Stale, Provenance::Main, None, "s");
        stale.dormant = true;
        let row = render_all(std::slice::from_ref(&stale), DESIGN_COLS, Widths::EXPANDED).remove(0);
        // samuraiRed `#E82424` mixed 60% toward sumiInk3: (111.4 -> 111,
        // 33, 38.4 -> 38).
        assert!(
            row.contains("\u{1b}[38;2;111;33;38m"),
            "a stale dormant row still recedes with its block"
        );
        let mut opening = agent(RowStatus::Opening, Provenance::Main, None, "s");
        opening.dormant = true;
        let row = render_all(
            std::slice::from_ref(&opening),
            DESIGN_COLS,
            Widths::EXPANDED,
        )
        .remove(0);
        // carpYellow `#E6C384` at full strength.
        assert!(
            row.contains("\u{1b}[38;2;230;195;132m"),
            "a launching row must not dim mid-transition"
        );
    }

    /// An ink index with no palette entry must fall back visibly, not wrap onto
    /// a real hue (LEDGER D7).
    #[test]
    fn an_unset_or_out_of_range_ink_falls_back_to_untinted() {
        assert_eq!(hue(None, &Theme::default()), UNTINTED);
        assert_eq!(hue(Some(99), &Theme::default()), UNTINTED);
        assert_eq!(hue(Some(0), &Theme::default()), PALETTE[0].0);
    }

    /// Ties round to even, as Python's `round()` does — the ratified preview's
    /// captured output depends on it.
    ///
    /// waveRed's BLUE is the channel that discriminates, and it is already in
    /// the golden: `118 + (40 - 118) * 0.25 = 98.5`, which is **98** under
    /// ties-to-even and **99** under half-away-from-zero. fujiWhite's `149.5`
    /// is 150 under BOTH modes, so asserting it proves nothing.
    #[test]
    fn mix_rounds_ties_to_even() {
        let wave_red = PALETTE[3].0;
        assert_eq!(wave_red, Rgb(0xE4, 0x68, 0x76));
        assert_eq!(wave_red.mix(BASE, FADE).2, 98);
        // The whole faded hue, as it appears in `golden_bar_at_fifty_four_columns`.
        assert_eq!(wave_red.mix(BASE, FADE), Rgb(179, 86, 98));
    }

    /// Every row state's glyph and colour, against the table in LEDGER D10.
    /// `Failed` and `Opening` are constructed by no fixture and by no preview
    /// row, so without this their glyphs were asserted by nothing — and the
    /// U+2716 / U+2717 transposition is the one lock §5 calls out BY NAME.
    #[test]
    fn every_row_state_matches_the_ledger_glyph_table() {
        let wave_red = Rgb(0xE4, 0x68, 0x76);
        let ronin_yellow = Rgb(0xFF, 0x9E, 0x3B);
        let spring_green = Rgb(0x98, 0xBB, 0x6C);
        let sumi_ink4 = Rgb(0x54, 0x54, 0x6D);
        let fuji_white = Rgb(0xDC, 0xD7, 0xBA);
        let samurai_red = Rgb(0xE8, 0x24, 0x24);
        let carp_yellow = Rgb(0xE6, 0xC3, 0x84);
        let table = [
            (RowStatus::NeedsYou, '\u{25cf}', wave_red),
            (RowStatus::Working, '\u{25cf}', ronin_yellow),
            (RowStatus::Done, '\u{25cf}', spring_green),
            (RowStatus::Idle, '\u{25cf}', sumi_ink4),
            (RowStatus::Failed, '\u{2716}', samurai_red), // HEAVY multiplication x
            (RowStatus::Dormant, '\u{25cb}', fuji_white),
            (RowStatus::DormantSelected, '\u{23ce}', carp_yellow), // ⏎ commit affordance (#100)
            (RowStatus::Opening, '\u{21bb}', carp_yellow),
            (RowStatus::Stale, '\u{2717}', samurai_red), // BALLOT x — a flag, not a Status
        ];
        for (status, glyph, colour) in table {
            assert_eq!(
                status.mark(&Theme::default()),
                (glyph, colour),
                "{status:?}"
            );
            // Every marker is exactly one cell: the gutter is position-locked
            // (lock §2.1), so a two-cell glyph shifts the whole row right.
            assert_eq!(glyph.width(), Some(1), "{status:?} is not one cell wide");
            // And it reaches the row: col 2 is the status cell (lock §2.1).
            let row = agent(status, Provenance::Main, None, "s");
            let bare = strip_sgr(&render_all(&[row], DESIGN_COLS, Widths::EXPANDED)[0]);
            assert_eq!(cell_slice(&bare, 1, 2), glyph.to_string(), "{status:?}");
        }
        // The two easy to transpose are genuinely different glyphs.
        assert_ne!(
            RowStatus::Failed.mark(&Theme::default()).0,
            RowStatus::Stale.mark(&Theme::default()).0,
            "U+2716 and U+2717 must not collapse"
        );
    }

    /// Fix to the summary's colour gate: with NOTHING selected every row fades
    /// by 0, so `fade > 0.0` made every summary inherit its REPO ink — and all
    /// of them flipped to fujiWhite the instant any row was selected. That
    /// colour change is undesigned. `nothing_selected_means_nothing_faded` only
    /// inspects status hues and did not catch it.
    #[test]
    fn an_unselected_summary_is_fujiwhite_even_when_nothing_is_selected() {
        let row = agent(RowStatus::Done, Provenance::Main, Some("T"), "summary text");
        let line = &render_all(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED)[0];
        // Unfaded fujiWhite: nothing is selected, so the fade is 0.
        let (before, after) = line.split_once("summary text").expect("the summary");
        assert!(
            before.ends_with(&DEFAULT_INK.fg()),
            "the summary must be set to fujiWhite, not left on the repo ink: {before:?}"
        );
        assert!(!after.is_empty());

        // And the selected row still INHERITS the repo ink — ratified, visible
        // in the preview's selected row.
        let selected = Row {
            selected: true,
            dormant: false,
            ..row
        };
        let line = &render_all(&[selected], DESIGN_COLS, Widths::EXPANDED)[0];
        let (before, _) = line.split_once("summary text").expect("the summary");
        assert!(!before.ends_with(&DEFAULT_INK.fg()));
        assert!(
            before.contains(&PALETTE[0].0.fg()),
            "the repo ink carries over"
        );
    }

    /// Summaries are agent-authored data written by hooks, so a control
    /// character is reachable input. They measure as ZERO cells, so an
    /// unfiltered `\n` or `\u{1b}` would break the row on screen while still
    /// satisfying `every_row_is_exactly_cols_cells`.
    #[test]
    fn control_characters_cannot_break_a_row() {
        let mut row = agent(RowStatus::Working, Provenance::Main, None, "");
        if let RowContent::Agent { summary, repo, .. } = &mut row.content {
            *summary = String::from("one\ntwo");
            *repo = String::from("re\u{1b}po");
        }
        let line = &render_all(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED)[0];
        let bare = strip_sgr(line);
        assert_eq!(display_cells(&bare), DESIGN_COLS);
        assert!(
            !bare.chars().any(char::is_control),
            "a control character reached the row: {bare:?}"
        );
        // Task 1.5's follow-up: a control char becomes a SPACE, not nothing —
        // dropping `\n` merges "one" and "two" into one word, and a summary
        // containing `\n` is a wrapped sentence, not two words meant to touch.
        assert!(bare.contains("one two"), "{bare:?}");
        assert!(!bare.contains("onetwo"), "{bare:?}");
    }

    /// A control character elsewhere in a fixed-width cell gets the same
    /// treatment, and clashing SGR-like text stays literal once stripped —
    /// `strip_sgr` only removes sequences THIS renderer emitted.
    #[test]
    fn a_control_character_mid_fixed_cell_becomes_a_space() {
        let row = agent(
            RowStatus::Working,
            Provenance::Main,
            None,
            "\u{1b}[31mred\u{7f}\u{9b}!",
        );
        let line = &render_all(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED)[0];
        let bare = strip_sgr(line);
        assert!(!bare.chars().any(char::is_control), "{bare:?}");
        assert!(bare.contains(" [31mred  !"), "{bare:?}");
    }

    // ── the viewport (#148) ─────────────────────────────────────────────────

    /// `n` terminal rows named `t00`, `t01`, … with `sel` selected. The
    /// viewport decides only WHICH rows reach the screen, so the cheapest
    /// row shape that carries a legible identity is the right fixture.
    fn numbered(n: usize, sel: usize) -> Vec<Row> {
        (0..n)
            .map(|i| Row {
                content: RowContent::terminal(format!("t{i:02}")),
                selected: i == sel,
                dormant: false,
            })
            .collect()
    }

    /// The model indices the pane actually SHOWS, top-down, recovered from the
    /// rendered lines. Every viewport assertion below reads the picture back
    /// this way rather than the offset behind it — zero-padded names so `t01`
    /// is never a substring of `t12`.
    fn on_screen_at(rows: &[Row], height: usize, cols: usize, widths: Widths) -> Vec<usize> {
        render_rows(rows, cols, height, widths, &Theme::default())
            .iter()
            .map(|line| {
                let bare = strip_sgr(line);
                (0..rows.len())
                    .find(|i| bare.contains(&format!("t{i:02}")))
                    .unwrap_or_else(|| panic!("no row name in rendered line {bare:?}"))
            })
            .collect()
    }

    fn on_screen(rows: &[Row], height: usize) -> Vec<usize> {
        on_screen_at(rows, height, DESIGN_COLS, Widths::EXPANDED)
    }

    /// The resting state: the list is anchored at the top and overflows off the
    /// BOTTOM only, so the live block is always the first thing on screen.
    /// This is the 2026-08-06 incident in miniature — before the viewport the
    /// bar printed all ten rows into a four-line pane, and zellij clipped the
    /// surplus into rows that nav could reach and the eye could not see.
    #[test]
    fn a_pane_shorter_than_the_fleet_rests_at_the_top() {
        assert_eq!(on_screen(&numbered(10, 0), 4), vec![0, 1, 2, 3]);
    }

    /// Nothing is selected when no tab is focused — the view still rests home.
    #[test]
    fn a_list_with_no_selection_rests_at_the_top() {
        let rows = numbered(10, usize::MAX); // `sel` matches no index
        assert_eq!(on_screen(&rows, 3), vec![0, 1, 2]);
    }

    /// The follow rule, walked down one row at a time in a five-line pane over
    /// ten rows. Top-anchored while the selection plus its two rows of
    /// LOOKAHEAD still fit the first screenful (rows 0–2); from row 3 the
    /// window makes the minimal slide that keeps both on screen.
    #[test]
    fn the_window_slides_only_when_the_selection_outruns_its_lookahead() {
        let windows: Vec<Vec<usize>> = (0..10).map(|s| on_screen(&numbered(10, s), 5)).collect();
        assert_eq!(
            windows,
            vec![
                vec![0, 1, 2, 3, 4], // rest
                vec![0, 1, 2, 3, 4],
                vec![0, 1, 2, 3, 4], // two rows of lookahead, still home
                vec![1, 2, 3, 4, 5], // the first slide
                vec![2, 3, 4, 5, 6],
                vec![3, 4, 5, 6, 7],
                vec![4, 5, 6, 7, 8],
                vec![5, 6, 7, 8, 9], // the end of the list stops the slide
                vec![5, 6, 7, 8, 9], // lookahead shrinks to the rows that exist
                vec![5, 6, 7, 8, 9], // the last row rides the bottom edge
            ]
        );
    }

    /// Walking back up returns the view, and it SNAPS HOME the moment the
    /// selection fits the first screenful again — the resting state is always
    /// the same one. Asserted as the descending walk so the direction is a
    /// real claim and not the previous test read backwards.
    #[test]
    fn walking_back_up_returns_the_view_and_snaps_home() {
        let up: Vec<Vec<usize>> = (0..10)
            .rev()
            .map(|s| on_screen(&numbered(10, s), 5))
            .collect();
        assert_eq!(up.first().unwrap(), &vec![5, 6, 7, 8, 9]);
        assert_eq!(up.last().unwrap(), &vec![0, 1, 2, 3, 4]);
        // Monotone all the way home: the view never jumps down while the
        // selection walks up.
        for pair in up.windows(2) {
            assert!(
                pair[1][0] <= pair[0][0],
                "the window slid the wrong way: {pair:?}"
            );
        }
    }

    /// A pane exactly as tall as the list, and one taller: no slice, no gap.
    #[test]
    fn a_pane_that_fits_the_list_draws_all_of_it() {
        for height in [5, 6, 40] {
            assert_eq!(on_screen(&numbered(5, 4), height), vec![0, 1, 2, 3, 4]);
        }
    }

    /// Degenerate heights. A zero-line pane draws nothing (zellij hands out
    /// 0 mid-layout); a one-line pane spends its only line on the selection,
    /// because lookahead never costs the selection its own place.
    #[test]
    fn degenerate_pane_heights_stay_total() {
        assert!(
            render_rows(
                &numbered(10, 3),
                DESIGN_COLS,
                0,
                Widths::EXPANDED,
                &Theme::default()
            )
            .is_empty()
        );
        assert_eq!(on_screen(&numbered(10, 6), 1), vec![6]);
        assert_eq!(on_screen(&numbered(10, 9), 1), vec![9]);
        // Two lines afford ONE row of lookahead, not two — the pane's room for
        // it is what caps it.
        assert_eq!(on_screen(&numbered(10, 5), 2), vec![5, 6]);
        assert!(render_rows(&[], DESIGN_COLS, 4, Widths::EXPANDED, &Theme::default()).is_empty());
    }

    /// Collapsed is a WIDTH profile (LEDGER D16): narrowing the bar must not
    /// change which rows exist. Same height, same slice, both profiles.
    #[test]
    fn collapsed_mode_windows_the_identical_rows() {
        for sel in [0, 3, 7, 9] {
            let rows = numbered(10, sel);
            assert_eq!(
                on_screen_at(&rows, 5, COLLAPSED_DESIGN_COLS, Widths::COLLAPSED),
                on_screen_at(&rows, 5, DESIGN_COLS, Widths::EXPANDED),
                "profiles disagreed at sel={sel}"
            );
        }
    }

    /// A tab spawning while the view is scrolled deep must not yank it. Growth
    /// arrives at either end: a new LIVE row lands above the dormant block and
    /// shifts every index below it, a new dormant row lands below. Neither
    /// changes the rows under the reader's eye — with one bounded exception,
    /// ruled acceptable and pinned at the end of this test: growth below a
    /// selection sitting on the LAST row slides the view down by up to LOOKAHEAD
    /// rows, because that is where the end-of-list clamp releases. See
    /// [`viewport_top`].
    #[test]
    fn a_snapshot_arriving_while_scrolled_leaves_the_view_alone() {
        let before = on_screen(&numbered(10, 7), 5);
        assert_eq!(before, vec![5, 6, 7, 8, 9]);

        // Two rows appended below the selection.
        let grown = numbered(12, 7);
        assert_eq!(on_screen(&grown, 5), before);

        // A row spawning ABOVE it: the selection is now index 8, and the
        // window shows the same five rows it did before (old 5..9).
        let mut above = numbered(10, 7);
        above.insert(
            0,
            Row {
                content: RowContent::terminal(String::from("fresh")),
                selected: false,
                dormant: false,
            },
        );
        let shifted: Vec<usize> = on_screen_at(&above, 5, DESIGN_COLS, Widths::EXPANDED);
        assert_eq!(shifted, before, "growth above the selection moved the view");

        // The one corner where growth below DOES move the view, and the
        // maintainer's 2026-08-11 ruling accepts it: the selection sits on the
        // LAST row, so the end-of-list clamp has been holding the view two rows
        // higher than the follow rule wants. Appending rows releases that clamp
        // and the lookahead it was suppressing reasserts itself — the view
        // slides down by LOOKAHEAD, at most, with no navigation. Restoring the
        // lookahead beats strict never-yank here, because the alternative is a
        // remembered scroll position, and a viewport that remembers is a
        // viewport that can hide the selection.
        let tail = on_screen(&numbered(10, 9), 5);
        assert_eq!(tail, vec![5, 6, 7, 8, 9], "the clamp holds the tail view");
        assert_eq!(
            on_screen(&numbered(12, 9), 5),
            vec![7, 8, 9, 10, 11],
            "the released clamp slid the view by other than LOOKAHEAD rows"
        );
    }

    proptest! {
        /// `rjust` does not truncate, so the four-cell bound is `token_text`'s to
        /// hold — over EVERY `u32`, not just the plausible range. Four billion
        /// tokens is not a real reading, but it is a reachable `u32`, and a
        /// fifth cell would push the whole row's text one column right.
        #[test]
        fn a_token_count_never_outgrows_its_cell(tokens: u32) {
            let text = token_text(tokens);
            prop_assert!(
                display_cells(&text) <= BatteryCell::Count.width(),
                "{tokens} rendered {text:?}"
            );
            prop_assert!(!text.is_empty(), "{tokens} rendered nothing");
        }

        /// The invariant the whole ticket exists for: whatever the fleet size,
        /// pane height and selection, the selected row is ON SCREEN — never
        /// reachable-but-invisible. Plus the shape rules around it: the pane is
        /// filled if there are rows to fill it, the window is contiguous and
        /// top-down, and the lookahead below the selection is honoured as far
        /// as the rows and the pane allow.
        #[test]
        fn the_selection_is_always_inside_the_viewport(
            n in 1usize..40,
            sel_seed in 0usize..40,
            height in 0usize..24,
        ) {
            let sel = sel_seed % n;
            let rows = numbered(n, sel);
            let seen = on_screen(&rows, height);

            prop_assert_eq!(seen.len(), n.min(height), "the pane is not full");
            for pair in seen.windows(2) {
                prop_assert_eq!(pair[1], pair[0] + 1, "the window is not contiguous");
            }
            if height == 0 {
                return Ok(());
            }
            prop_assert!(seen.contains(&sel), "selection {} off screen: {:?}", sel, seen);

            // Lookahead: two rows below the selection, capped by the rows that
            // actually exist below it and by the room the pane has for them.
            let want = LOOKAHEAD.min(n - 1 - sel).min(height - 1);
            let below = seen.iter().filter(|&&i| i > sel).count();
            prop_assert!(below >= want, "lookahead {} < {}: {:?}", below, want, seen);

            // Rests at the top unless the selection and that lookahead force
            // the slide, and never slides further than it must.
            let forced = sel + want + 1 > height;
            prop_assert_eq!(seen[0] == 0, !forced, "anchoring wrong: {:?}", seen);
            if forced {
                prop_assert_eq!(seen[0], sel + want + 1 - height, "over-slid: {:?}", seen);
            }
        }

        /// Walking the selection down never moves the window UP, and walking
        /// up never moves it down: one selection step slides the view by at
        /// most one row, monotonically. That is what makes the bar feel stable
        /// under a nav burst.
        #[test]
        fn one_step_of_the_selection_slides_the_view_by_at_most_one(
            n in 1usize..40,
            height in 1usize..24,
        ) {
            let mut previous_top = 0usize;
            for sel in 0..n {
                let top = on_screen(&numbered(n, sel), height)[0];
                prop_assert!(top >= previous_top, "the view slid up while walking down");
                prop_assert!(top - previous_top <= 1, "the view jumped {} rows", top - previous_top);
                previous_top = top;
            }
        }
    }
}
