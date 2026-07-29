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

/// The ratified expanded width (lock §2). `cols` stays a parameter — zellij
/// hands the plugin whatever the pane actually is — but every number in the
/// design was chosen against 44, and at 44 the output IS the lock's table.
pub const DESIGN_COLS: usize = 44;

/// Cols 1–9: cap, status, space, rule, space, battery, space, provenance,
/// space. Position-locked — every cell is one column and renders a space when
/// its glyph is absent, so a dropped glyph degrades to a blank cell rather than
/// a shifted row (lock §2.1). Never parameterised: D16 keeps the gutter
/// IDENTICAL between profiles — that is the whole point of it being a width
/// profile and not a second layout.
const GUTTER_W: usize = 9;

/// Collapsed is a WIDTH PROFILE, not a second layout (LEDGER D16, supersedes
/// D12): one `render_row` body, parameterised by how wide the title and repo
/// cells are. `summary` is never part of the profile — it is the only flex
/// cell in EITHER state (D9, D16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Widths {
    pub title: usize,
    pub repo: usize,
}

impl Widths {
    /// Design-lock §2, unchanged. At `cols == DESIGN_COLS` this profile's
    /// output is byte-for-byte the pre-D16 renderer's — that is the regression
    /// pin (`golden_bar_at_forty_four_columns`, `bar-preview`'s captured
    /// prefix).
    pub const EXPANDED: Widths = Widths { title: 7, repo: 7 };

    /// LEDGER D17: chosen by Ollie from three rendered candidates. Repo drops
    /// to three (`cla`, `nal` — collisions are rare and the repo ink still
    /// disambiguates); title HOLDS at 7, identical to `EXPANDED` — the
    /// rejected 5-column variant bought two summary characters by truncating
    /// the chip itself (`API-GW`/`API-V2` both to `API-`, `KDL-GRD` to
    /// `KDL-G`), and the chip is the thing a tab is identified BY. Holding it
    /// also means the chip does not reflow when the profile toggles, so the
    /// eye keeps its anchor across the transition.
    pub const COLLAPSED: Widths = Widths { title: 7, repo: 3 };

    /// Fixed columns everywhere; `summary` is the only flex cell (LEDGER D9).
    /// Everything else — gutter, title, repo, the two separating spaces, the
    /// right margin and both caps — holds its width at any `cols`, so below
    /// this floor a row is wider than the pane rather than misaligned. `13` is
    /// the 9-column gutter plus the space after title, the space after repo,
    /// the right margin and the right cap (D12's arithmetic, generalised by
    /// D16 to any profile): `27` for `EXPANDED`, `23` for `COLLAPSED` (D17). That is
    /// deliberate, not a compromise: a row that silently reflowed its columns
    /// to fit would be the one failure mode §2.1 exists to forbid. S6 §2.10's
    /// `cols - 7` text budget is superseded.
    pub fn min_intact_cols(self) -> usize {
        13 + self.title + self.repo
    }
}

// ── colour ──────────────────────────────────────────────────────────────────

/// 24-bit truecolor, not ANSI-16 (LEDGER D8): the kanagawa palette has no
/// ANSI-16 equivalent, and lock §4.1 grants the provenance cell an arbitrary
/// RGB on purpose. `Status::glyph()` in clave-types keeps its `u8` ANSI
/// contract for the host CLI — the bar owns its own palette (D10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn fg(self) -> String {
        format!("\u{1b}[38;2;{};{};{}m", self.0, self.1, self.2)
    }

    pub fn bg(self) -> String {
        format!("\u{1b}[48;2;{};{};{}m", self.0, self.1, self.2)
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    /// Blend toward `other` by `t`. Ties round to EVEN — not away from zero —
    /// because the ratified preview's captured output was produced by Python's
    /// `round()`, which is round-half-to-even. One channel landing on `.5` is
    /// enough to make the port off-by-one against the design that was signed
    /// off, and `#DCD7BA` faded onto `#1F1F28` puts blue on exactly `149.5`.
    fn mix(self, other: Rgb, t: f64) -> Rgb {
        let ch = |a: u8, b: u8| {
            (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round_ties_even() as u8
        };
        Rgb(
            ch(self.0, other.0),
            ch(self.1, other.1),
            ch(self.2, other.2),
        )
    }
}

pub const RESET: &str = "\u{1b}[0m";

const BASE: Rgb = Rgb(0x1F, 0x1F, 0x28); // sumiInk3 — the bar background
const SEL_BG: Rgb = Rgb(0x2D, 0x4F, 0x67); // waveBlue2 — the selected row
const RULE_INK: Rgb = Rgb(0xDC, 0xD7, 0xBA); // fujiWhite — rule and summary
/// sumiInk0 — text ON a title chip. Public because the preview draws the same
/// chip in its palette swatches (lock §4).
pub const CHIP_INK: Rgb = Rgb(0x16, 0x16, 0x1D);
const TERMINAL_INK: Rgb = Rgb(0x71, 0x7C, 0x7C); // a terminal tab's name

/// The ink a row falls back to when it has no palette entry yet. Reachable:
/// allocation is store-backed iterate-and-wrap (lock §4) and a row can render
/// before its colour exists.
const UNTINTED: Rgb = Rgb(0x54, 0x54, 0x6D); // sumiInk4

/// Eight kanagawa hues, allocated round-robin and keyed by repo root, so one
/// repo is one colour everywhere forever (lock §4). Twelve was rendered first
/// and rejected: they start colliding after the fifth. Hashing is overruled
/// twice over — `DefaultHasher` is not stable across toolchains, and the
/// maintainer rejected collisions outright.
pub const PALETTE: [(Rgb, &str); 8] = [
    (Rgb(0x7E, 0x9C, 0xD8), "crystalBlue"),
    (Rgb(0x98, 0xBB, 0x6C), "springGreen"),
    (Rgb(0xE6, 0xC3, 0x84), "carpYellow"),
    (Rgb(0xE4, 0x68, 0x76), "waveRed"),
    (Rgb(0x95, 0x7F, 0xB8), "oniViolet"),
    (Rgb(0x7A, 0xA8, 0x9F), "waveAqua2"),
    (Rgb(0xFF, 0xA0, 0x66), "surimiOrange"),
    (Rgb(0xD2, 0x7E, 0x99), "sakuraPink"),
];

/// Unselected rows recede 25% toward the bar background (lock §6). Selection by
/// recession costs zero columns and gets MORE effective as the fleet grows,
/// which is the opposite of a background tint — a tint competes with the title
/// chips and repo inks for the same channel, which is why it read as
/// insufficient. Fades at 8/12/15/20/30/40% were rendered and rejected.
const FADE: f64 = 0.25;

// ── glyphs (lock §5) ────────────────────────────────────────────────────────

const LCAP: char = '\u{e0b6}'; // powerline half-circle thick, left
const RCAP: char = '\u{e0b4}'; // powerline half-circle thick, right
const RULE: char = '\u{2502}'; // box drawings light vertical
const ELLIPSIS: char = '\u{2026}';
const CONSOLE: char = '\u{f018d}'; // nf-md-console — a terminal has no battery

/// The S7 magnitude ramp, green through red. Index is the context level.
const BATTERY: [(char, Rgb); 5] = [
    ('\u{f0079}', Rgb(0x98, 0xBB, 0x6C)),
    ('\u{f007e}', Rgb(0x98, 0xBB, 0x6C)),
    ('\u{f007c}', Rgb(0xE6, 0xC3, 0x84)),
    ('\u{f007b}', Rgb(0xFF, 0xA0, 0x66)),
    ('\u{f007a}', Rgb(0xE4, 0x68, 0x76)),
];

// ── the row ─────────────────────────────────────────────────────────────────

/// What the status cell says. Five of these are `clave_types::Status`; the
/// other three are row states the model distinguishes without a `Status` —
/// `Stale` is a `bool` flag (`clave open` found the cwd missing), `Dormant` and
/// `Opening` are model states. The renderer stays total by owning all eight
/// (LEDGER D10); `Status::glyph()` is untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    NeedsYou,
    Working,
    Done,
    Idle,
    Failed,
    Dormant,
    Opening,
    Stale,
}

impl RowStatus {
    /// The COLOUR is the state; the shape varies only where the state is not a
    /// conversation at all (lock §5). `Failed` is U+2716 HEAVY multiplication
    /// x and `Stale` is U+2717 BALLOT x — different glyphs for different
    /// things, and easy to transpose (FOOTGUNS).
    fn mark(self) -> (char, Rgb) {
        match self {
            RowStatus::NeedsYou => ('\u{25cf}', Rgb(0xE4, 0x68, 0x76)), // waveRed
            RowStatus::Working => ('\u{25cf}', Rgb(0xFF, 0x9E, 0x3B)),  // roninYellow
            RowStatus::Done => ('\u{25cf}', Rgb(0x98, 0xBB, 0x6C)),     // springGreen
            RowStatus::Idle => ('\u{25cf}', UNTINTED),
            RowStatus::Failed => ('\u{2716}', Rgb(0xE8, 0x24, 0x24)), // samuraiRed
            RowStatus::Dormant => ('\u{25cc}', UNTINTED),
            RowStatus::Opening => ('\u{21bb}', Rgb(0xE6, 0xC3, 0x84)), // carpYellow
            RowStatus::Stale => ('\u{2717}', Rgb(0xE8, 0x24, 0x24)),   // samuraiRed
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
    fn mark(self) -> Option<char> {
        match self {
            Provenance::Main => None,
            Provenance::Branch => Some('\u{f062c}'), // nf-md-source_branch (lazygit's)
            Provenance::Worktree => Some('\u{168c2}'), // bamum tree
        }
    }
}

/// A row's fields. `Terminal` is a variant rather than a bundle of `None`s
/// because a terminal tab is a different thing: it has no agent record, so it
/// renders its zellij name across the whole body and takes the console mark in
/// the battery cell (lock §5, §7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowContent {
    Agent {
        status: RowStatus,
        /// Index into the S7 ramp; `None` renders a blank cell (a dormant
        /// conversation's reading is last-known, not current — lock §7.2
        /// leaves dim-vs-absent unsettled, so the renderer supports absent).
        battery: Option<u8>,
        provenance: Provenance,
        /// The chip; `None` when the session was never renamed.
        title: Option<String>,
        title_ink: Option<u8>,
        repo: String,
        repo_ink: Option<u8>,
        summary: String,
    },
    Terminal {
        name: String,
    },
}

/// Inks are `Option<u8>`, never a bare `u8` (LEDGER D7): `0` is `crystalBlue`,
/// a real palette entry, so a bare `u8` has no unset value and `unwrap_or(0)`
/// paints every untinted row one colour while reading as "untinted".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub content: RowContent,
    pub selected: bool,
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
/// CLAIMS every row is 44 cells, and a claim about a rendered row is worth
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
    // dropping destroys, and is neutral to every 44-column golden either way.
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

// ── the renderer ────────────────────────────────────────────────────────────

/// The whole bar, one `String` per row.
///
/// Whole-bar rather than per-row (LEDGER D5) because the fade is RELATIVE:
/// lock §6 recedes every *unselected* row only when some row is selected, and a
/// per-row function cannot know that without a parameter that re-states what
/// the slice already knows. It is also the unit a golden test should assert —
/// the picture, not a fragment.
pub fn render_rows(rows: &[Row], cols: usize, widths: Widths) -> Vec<String> {
    let any_selected = rows.iter().any(|r| r.selected);
    rows.iter()
        .map(|row| render_row(row, cols, widths, any_selected))
        .collect()
}

fn hue(ink: Option<u8>) -> Rgb {
    ink.and_then(|i| PALETTE.get(usize::from(i)))
        .map_or(UNTINTED, |(c, _)| *c)
}

fn render_row(row: &Row, cols: usize, widths: Widths, any_selected: bool) -> String {
    // Nothing to recede FROM when nothing is selected, so an unfocused bar
    // renders at full strength (lock §6).
    let fade = if row.selected || !any_selected {
        0.0
    } else {
        FADE
    };
    // Re-asserted after every span that sets its own colour. Emitting it more
    // often than strictly necessary is deliberate: the alternative is tracking
    // SGR state across the row, and a background that lapses for one cell is
    // exactly the ragged selection lock §6 forbids.
    let o = if row.selected {
        SEL_BG.bg()
    } else {
        String::new()
    };
    let ink = |c: Rgb| c.mix(BASE, fade).fg();

    let mut out = String::new();

    // Col 1. The cap is drawn as a FOREGROUND glyph on the default background —
    // that is what rounds the row's end. Reserved (blank) on unselected rows so
    // the selected row's content does not sit one column right of its
    // neighbours (lock §2.2).
    if row.selected {
        out.push_str(&SEL_BG.fg());
        out.push(LCAP);
        out.push_str(&o);
    } else {
        out.push(' ');
    }

    // Cols 2–8, the gutter proper.
    match &row.content {
        RowContent::Terminal { .. } => {
            out.push_str(&o); // col 2 — no status; a terminal has no turn
            out.push(' ');
            push_rule(&mut out, &o, &ink); // cols 3–5
            out.push_str(&o);
            out.push_str(&ink(UNTINTED));
            out.push(CONSOLE); // col 6
            out.push_str(&o);
            out.push_str(&o);
            out.push_str("  "); // cols 7–8 — no provenance yet (lock §7.2)
        }
        RowContent::Agent {
            status,
            battery,
            provenance,
            repo_ink,
            ..
        } => {
            let (glyph, colour) = status.mark();
            out.push_str(&o);
            out.push_str(&ink(colour));
            out.push(glyph); // col 2
            out.push_str(&o);
            push_rule(&mut out, &o, &ink); // cols 3–5
            out.push_str(&o);
            match battery.and_then(|i| BATTERY.get(usize::from(i))) {
                Some((glyph, colour)) => {
                    out.push_str(&ink(*colour));
                    out.push(*glyph); // col 6
                    out.push_str(&o);
                }
                None => out.push(' '),
            }
            out.push_str(&o);
            out.push(' '); // col 7
            out.push_str(&o);
            // Col 8. The one gutter cell permitted an arbitrary RGB: it takes
            // the repo ink, making repo identity a shape in the gutter as well
            // as a colour in the text (lock §4.1).
            match provenance.mark() {
                Some(glyph) => {
                    out.push_str(&ink(hue(*repo_ink)));
                    out.push(glyph);
                    out.push_str(&o);
                }
                None => out.push(' '),
            }
        }
    }
    out.push_str(&o);
    out.push(' '); // col 9

    // Only `summary` flexes (LEDGER D9), in EITHER profile (D16). Below
    // `widths.min_intact_cols()` the fixed columns cannot all fit, and the row
    // is deliberately WIDER than the pane rather than reflowed — but EVERY row
    // kind over-runs to the same width, or the bar goes ragged instead of
    // merely clipped, which is the alignment loss lock §2.1 exists to forbid.
    // A terminal row used to shrink to `cols` while an agent row held at the
    // floor. D16 makes this a guard against pathological widths, not the
    // mechanism a user ever sees: the caller picks the profile by STATE, so
    // the seek never crosses this floor mid-animation (D16's "consequence
    // that matters"). D12's "collapsed is a second layout" conclusion is
    // superseded; this comment's job is the same one D13 gave it.
    let intact = cols.max(widths.min_intact_cols());
    // `saturating_sub` rather than a floor check: a 0-width budget must render
    // nothing, not panic, if these constants ever move.
    let body = intact.saturating_sub(GUTTER_W + 2); // minus right margin and cap
    let summary_w = body.saturating_sub(widths.title + widths.repo + 2);

    match &row.content {
        RowContent::Terminal { name } => {
            out.push_str(&o);
            out.push_str(&ink(TERMINAL_INK));
            out.push_str(&clamp(name, body));
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
            // Cols 10–16. The title is a filled CHIP with dark text, keyed per
            // title WITHIN a repo, so two tabs of one repo never share one
            // (lock §4). Blank when the session was never renamed.
            match title {
                Some(title) => {
                    out.push_str(&hue(*title_ink).mix(BASE, fade).bg());
                    out.push_str(&CHIP_INK.fg());
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
            out.push(' '); // col 17
            // Cols 18–24. Tinted TEXT, keyed by repo root — one repo is one
            // colour everywhere, forever (lock §4).
            out.push_str(&ink(hue(*repo_ink)));
            out.push_str(&clamp(repo, widths.repo));
            out.push_str(&o);
            out.push_str(&o);
            out.push(' '); // col 25
            // Cols 26–42. The selected row leaves the summary at the repo ink
            // set on the line above — deliberate and ratified, it is visible in
            // the preview's selected row. Every OTHER row is fujiWhite, faded
            // or not: gating this on `fade > 0.0` conflated "unselected" with
            // "faded", and those come apart when nothing is selected (every row
            // fades by 0), which silently painted every summary its repo colour
            // until the moment a row was selected.
            if !row.selected {
                out.push_str(&ink(RULE_INK));
            }
            out.push_str(&clamp(summary, summary_w));
            out.push_str(&o);
        }
    }

    out.push_str(&o);
    out.push(' '); // col 43 — right margin
    out.push_str(RESET);
    if row.selected {
        out.push_str(&SEL_BG.fg());
        out.push(RCAP); // col 44
        out.push_str(RESET);
    } else {
        out.push(' ');
    }
    out
}

/// Cols 3–5: space, rule, space. The rule separates the status hue from the
/// battery hue so two adjacent coloured dots do not read as one signal.
fn push_rule(out: &mut String, o: &str, ink: &impl Fn(Rgb) -> String) {
    out.push_str(o);
    out.push(' ');
    out.push_str(&ink(RULE_INK));
    out.push(RULE);
    out.push_str(o);
    out.push(' ');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(status: RowStatus, provenance: Provenance, title: Option<&str>, summary: &str) -> Row {
        Row {
            content: RowContent::Agent {
                status,
                battery: Some(2),
                provenance,
                title: title.map(String::from),
                title_ink: Some(5),
                repo: String::from("clave"),
                repo_ink: Some(0),
                summary: String::from(summary),
            },
            selected: false,
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
                content: RowContent::Terminal {
                    name: String::from("Tab #16"),
                },
                selected: false,
            },
            agent(RowStatus::Stale, Provenance::Branch, Some("KDL-GRD"), "x"),
            Row {
                content: RowContent::Agent {
                    status: RowStatus::Dormant,
                    battery: None,
                    provenance: Provenance::Main,
                    title: None,
                    title_ink: None,
                    repo: String::from("dotfiles"),
                    repo_ink: None,
                    summary: String::new(),
                },
                selected: false,
            },
        ];
        rows[1].selected = true;
        rows
    }

    /// The invariant the lock only CLAIMS in prose, and the one that matters:
    /// a row that is not exactly `cols` cells is a ragged bar.
    #[test]
    fn every_row_is_exactly_cols_cells() {
        for cols in [Widths::EXPANDED.min_intact_cols(), 30, DESIGN_COLS, 80, 200] {
            for line in render_rows(&fleet(), cols, Widths::EXPANDED) {
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
    /// `min_intact_cols_is_thirteen_plus_title_plus_repo`) and `30` is D17's
    /// chosen target; the rest are shared reference points with the
    /// `EXPANDED` test above.
    #[test]
    fn every_row_is_exactly_cols_cells_under_collapsed() {
        for cols in [0, 1, 13, 23, 30, 44] {
            let expected = cols.max(Widths::COLLAPSED.min_intact_cols());
            for line in render_rows(&fleet(), cols, Widths::COLLAPSED) {
                let width = display_cells(&strip_sgr(&line));
                assert_eq!(width, expected, "at cols={cols}: {line:?}");
            }
        }
    }

    /// Selection must not move a single column (lock §2.2) — the cap columns
    /// are reserved on every row precisely so the eye scans one edge. Checked
    /// under BOTH width profiles (LEDGER D16): the gutter is identical between
    /// them, which is the entire point of a width profile rather than a second
    /// layout, so column 10 cannot move when the profile changes either.
    #[test]
    fn title_starts_at_column_ten_under_both_profiles() {
        for widths in [Widths::EXPANDED, Widths::COLLAPSED] {
            let unselected = agent(RowStatus::Working, Provenance::Branch, Some("S6-GUT"), "x");
            let selected = Row {
                selected: true,
                ..unselected.clone()
            };
            // What the chip cell holds at this profile's title width —
            // IDENTICAL under `COLLAPSED`: D17 holds title at 7, matching
            // `EXPANDED` (the chip is what a tab is identified by), starting
            // at the same column either way.
            let chip = clamp("S6-GUT", widths.title);
            for row in [unselected, selected] {
                let bare =
                    strip_sgr(&render_rows(std::slice::from_ref(&row), DESIGN_COLS, widths)[0]);
                assert_eq!(
                    cell_slice(&bare, GUTTER_W, GUTTER_W + widths.title),
                    chip,
                    "selected={} widths={widths:?}",
                    row.selected
                );
            }
        }
    }

    /// LEDGER D16's own arithmetic, pinned by equality rather than trusted from
    /// prose: `13` is the 9-column gutter plus the space after title, the space
    /// after repo, the right margin and the right cap — everything fixed that
    /// is not title or repo itself (D12, generalised to a profile by D16).
    #[test]
    fn min_intact_cols_is_thirteen_plus_title_plus_repo() {
        assert_eq!(Widths::EXPANDED.min_intact_cols(), 13 + 7 + 7);
        assert_eq!(Widths::EXPANDED.min_intact_cols(), 27); // D12, unchanged
        assert_eq!(Widths::COLLAPSED.min_intact_cols(), 13 + 7 + 3);
        assert_eq!(Widths::COLLAPSED.min_intact_cols(), 23); // D17
    }

    /// A missing glyph renders a blank cell and does not reflow the row (lock
    /// §2.1). A main checkout is the deliberate blank, and it is the most
    /// common row — so if absence reflowed, most of the bar would be ragged.
    #[test]
    fn an_absent_glyph_blanks_its_cell_without_reflowing() {
        let main = agent(RowStatus::Idle, Provenance::Main, Some("TITLE"), "s");
        let worktree = agent(RowStatus::Idle, Provenance::Worktree, Some("TITLE"), "s");
        let [main, worktree] = [main, worktree]
            .map(|r| strip_sgr(&render_rows(&[r], DESIGN_COLS, Widths::EXPANDED)[0]));

        // Cell 8, indexed in CELLS — the provenance column is only the eighth
        // `char` while every glyph before it is one cell wide.
        assert_eq!(cell_slice(&main, 7, 8), " ", "col 8 is blank for a main");
        assert_eq!(cell_slice(&worktree, 7, 8), "\u{168c2}");
        // Same width, same text origin: the blank cost exactly one column.
        assert_eq!(display_cells(&main), display_cells(&worktree));
        assert_eq!(
            cell_slice(&main, GUTTER_W, DESIGN_COLS),
            cell_slice(&worktree, GUTTER_W, DESIGN_COLS)
        );
    }

    /// Track the active background per CELL, so "spans all 44 columns" is a
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
    /// selection. Worth a test."
    #[test]
    fn a_selected_rows_background_spans_every_column() {
        let mut row = agent(RowStatus::Working, Provenance::Worktree, Some("T"), "short");
        row.selected = true;
        let line = &render_rows(&[row], DESIGN_COLS, Widths::EXPANDED)[0];
        let bgs = cell_backgrounds(line);
        assert_eq!(bgs.len(), DESIGN_COLS);

        let sel = Some(String::from("45;79;103"));
        // Cols 1 and 44 are the caps: the half-circle is a FOREGROUND glyph on
        // the default background, which is what makes the row read as rounded.
        assert_eq!(bgs[0], None);
        assert_eq!(bgs[DESIGN_COLS - 1], None);
        for (i, bg) in bgs.iter().enumerate().take(DESIGN_COLS - 1).skip(1) {
            assert!(bg.is_some(), "col {} lost its background", i + 1);
        }
        // Everything but the chip (cols 10–16) is the selection colour, and the
        // trailing pad after a 5-cell summary is included.
        for (i, bg) in bgs.iter().enumerate().take(DESIGN_COLS - 1).skip(1) {
            if (GUTTER_W..GUTTER_W + Widths::EXPANDED.title).contains(&i) {
                continue;
            }
            assert_eq!(bg, &sel, "col {}", i + 1);
        }
    }

    /// `chars().count()` is what today's renderer clamps with, and it is wrong
    /// by one column per wide glyph.
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
    /// columns hold and the row is WIDER than the pane (LEDGER D9) — recorded
    /// here so the behaviour is a decision, not a surprise. This is a guard
    /// against pathological widths now (LEDGER D16), not the mechanism a user
    /// ever sees — the collapsed profile is chosen by STATE, so a live seek
    /// never crosses this floor.
    ///
    /// Pinned to equality, not bounded: the widths are exact, and a future
    /// change that silently reflows a fixed column downward to fit should fail
    /// here rather than be tolerated by a `<=`. The fleet mixes agent and
    /// terminal rows on purpose — the over-run has to be UNIFORM across row
    /// kinds, or the visible part of the bar is ragged rather than clipped.
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
            let expected = cols.max(Widths::EXPANDED.min_intact_cols());
            for (i, line) in render_rows(&fleet(), cols, Widths::EXPANDED)
                .iter()
                .enumerate()
            {
                let width = display_cells(&strip_sgr(line));
                assert_eq!(width, expected, "row {i} at cols={cols}");
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
    ///   `1+1+1+1+1+1+1+1+1 + 7 + 1 + 7 + 1 + 17 + 1 + 1 = 44` cells: the
    ///   nine-cell gutter (§2.1), title, space, repo, space, summary, right
    ///   margin, right cap.
    /// - Row 1 has no title, so cols 10–16 are blank; `clave` is padded to 7 at
    ///   cols 18–24. Row 3 is a terminal, so its name runs the whole body.
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
    fn golden_bar_at_forty_four_columns() {
        let rows = vec![
            agent(
                RowStatus::NeedsYou,
                Provenance::Main,
                None,
                "I just passed the spec over",
            ),
            Row {
                selected: true,
                ..agent(
                    RowStatus::Working,
                    Provenance::Worktree,
                    Some("S6-GUT"),
                    "picking the gutter set",
                )
            },
            Row {
                content: RowContent::Terminal {
                    name: String::from("Tab #16"),
                },
                selected: false,
            },
        ];
        let expected = [
            " \u{1b}[38;2;179;86;98m\u{25cf} \u{1b}[38;2;173;169;150m\u{2502} \u{1b}[38;2;180;154;109m\u{f007c}           \u{1b}[38;2;102;125;172mclave   \u{1b}[38;2;173;169;150mI just passed th\u{2026} \u{1b}[0m ",
            "\u{1b}[38;2;45;79;103m\u{e0b6}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m\u{1b}[38;2;255;158;59m\u{25cf}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[38;2;220;215;186m\u{2502}\u{1b}[48;2;45;79;103m \u{1b}[48;2;45;79;103m\u{1b}[38;2;230;195;132m\u{f007c}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[48;2;45;79;103m\u{1b}[38;2;126;156;216m\u{168c2}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[48;2;122;168;159m\u{1b}[38;2;22;22;29mS6-GUT \u{1b}[0m\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[38;2;126;156;216mclave  \u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m picking the gutt\u{2026}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[0m\u{1b}[38;2;45;79;103m\u{e0b4}\u{1b}[0m",
            "   \u{1b}[38;2;173;169;150m\u{2502} \u{1b}[38;2;71;71;92m\u{f018d}   \u{1b}[38;2;92;101;103mTab #16                           \u{1b}[0m ",
        ];
        assert_eq!(render_rows(&rows, DESIGN_COLS, Widths::EXPANDED), expected);
    }

    /// The same picture, one layout, the other profile (LEDGER D16, D17):
    /// same three rows as `golden_bar_at_forty_four_columns`,
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
                ..agent(
                    RowStatus::Working,
                    Provenance::Worktree,
                    Some("S6-GUT"),
                    "picking the gutter set",
                )
            },
            Row {
                content: RowContent::Terminal {
                    name: String::from("Tab #16"),
                },
                selected: false,
            },
        ];
        let expected = [
            " \u{1b}[38;2;179;86;98m\u{25cf} \u{1b}[38;2;173;169;150m\u{2502} \u{1b}[38;2;180;154;109m\u{f007c}           \u{1b}[38;2;102;125;172mcla \u{1b}[38;2;173;169;150mI just\u{2026} \u{1b}[0m ",
            "\u{1b}[38;2;45;79;103m\u{e0b6}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m\u{1b}[38;2;255;158;59m\u{25cf}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[38;2;220;215;186m\u{2502}\u{1b}[48;2;45;79;103m \u{1b}[48;2;45;79;103m\u{1b}[38;2;230;195;132m\u{f007c}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[48;2;45;79;103m\u{1b}[38;2;126;156;216m\u{168c2}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[48;2;122;168;159m\u{1b}[38;2;22;22;29mS6-GUT \u{1b}[0m\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[38;2;126;156;216mcla\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m pickin\u{2026}\u{1b}[48;2;45;79;103m\u{1b}[48;2;45;79;103m \u{1b}[0m\u{1b}[38;2;45;79;103m\u{e0b4}\u{1b}[0m",
            "   \u{1b}[38;2;173;169;150m\u{2502} \u{1b}[38;2;71;71;92m\u{f018d}   \u{1b}[38;2;92;101;103mTab #16             \u{1b}[0m ",
        ];
        assert_eq!(render_rows(&rows, 30, Widths::COLLAPSED), expected);
        for line in &expected {
            assert_eq!(display_cells(&strip_sgr(line)), 30);
        }
        // Title still starts at column 10 (cell index GUTTER_W = 9) — the
        // gutter did not move, and D17 holds title at 7, so the chip itself
        // is untouched by the profile; only repo and summary narrowed.
        assert_eq!(
            cell_slice(
                &strip_sgr(expected[1]),
                GUTTER_W,
                GUTTER_W + Widths::COLLAPSED.title
            ),
            "S6-GUT "
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
            render_rows(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED).remove(0);
        let mut other = agent(RowStatus::Idle, Provenance::Main, None, "s");
        other.selected = true;
        let faded = &render_rows(&[row, other], DESIGN_COLS, Widths::EXPANDED)[0];

        // springGreen at full strength, then faded 25% toward sumiInk3.
        assert!(unfocused.contains("\u{1b}[38;2;152;187;108m"));
        assert!(faded.contains("\u{1b}[38;2;122;148;91m"));
    }

    /// An ink index with no palette entry must fall back visibly, not wrap onto
    /// a real hue (LEDGER D7).
    #[test]
    fn an_unset_or_out_of_range_ink_falls_back_to_untinted() {
        assert_eq!(hue(None), UNTINTED);
        assert_eq!(hue(Some(99)), UNTINTED);
        assert_eq!(hue(Some(0)), PALETTE[0].0);
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
        // The whole faded hue, as it appears in `golden_bar_at_forty_four_columns`.
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
        let samurai_red = Rgb(0xE8, 0x24, 0x24);
        let carp_yellow = Rgb(0xE6, 0xC3, 0x84);
        let table = [
            (RowStatus::NeedsYou, '\u{25cf}', wave_red),
            (RowStatus::Working, '\u{25cf}', ronin_yellow),
            (RowStatus::Done, '\u{25cf}', spring_green),
            (RowStatus::Idle, '\u{25cf}', sumi_ink4),
            (RowStatus::Failed, '\u{2716}', samurai_red), // HEAVY multiplication x
            (RowStatus::Dormant, '\u{25cc}', sumi_ink4),
            (RowStatus::Opening, '\u{21bb}', carp_yellow),
            (RowStatus::Stale, '\u{2717}', samurai_red), // BALLOT x — a flag, not a Status
        ];
        for (status, glyph, colour) in table {
            assert_eq!(status.mark(), (glyph, colour), "{status:?}");
            // And it reaches the row: col 2 is the status cell (lock §2.1).
            let row = agent(status, Provenance::Main, None, "s");
            let bare = strip_sgr(&render_rows(&[row], DESIGN_COLS, Widths::EXPANDED)[0]);
            assert_eq!(cell_slice(&bare, 1, 2), glyph.to_string(), "{status:?}");
        }
        // The two easy to transpose are genuinely different glyphs.
        assert_ne!(
            RowStatus::Failed.mark().0,
            RowStatus::Stale.mark().0,
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
        let line = &render_rows(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED)[0];
        // Unfaded fujiWhite: nothing is selected, so the fade is 0.
        let (before, after) = line.split_once("summary text").expect("the summary");
        assert!(
            before.ends_with(&RULE_INK.fg()),
            "the summary must be set to fujiWhite, not left on the repo ink: {before:?}"
        );
        assert!(!after.is_empty());

        // And the selected row still INHERITS the repo ink — ratified, visible
        // in the preview's selected row.
        let selected = Row {
            selected: true,
            ..row
        };
        let line = &render_rows(&[selected], DESIGN_COLS, Widths::EXPANDED)[0];
        let (before, _) = line.split_once("summary text").expect("the summary");
        assert!(!before.ends_with(&RULE_INK.fg()));
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
        let line = &render_rows(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED)[0];
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
        let line = &render_rows(std::slice::from_ref(&row), DESIGN_COLS, Widths::EXPANDED)[0];
        let bare = strip_sgr(line);
        assert!(!bare.chars().any(char::is_control), "{bare:?}");
        assert!(bare.contains(" [31mred  !"), "{bare:?}");
    }
}
