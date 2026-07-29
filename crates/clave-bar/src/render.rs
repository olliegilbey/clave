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
/// a shifted row (lock §2.1).
const GUTTER_W: usize = 9;
const TITLE_W: usize = 7;
const REPO_W: usize = 7;

/// Fixed columns everywhere; `summary` is the only flex cell (LEDGER D9).
/// Everything else — gutter, title, repo, the two separating spaces, the right
/// margin and both caps — holds its width at any `cols`, so below this floor a
/// row is wider than the pane rather than misaligned. That is deliberate: the
/// collapsed layout is NOT ratified (lock §3) and is S8's to design; a row that
/// silently reflowed its columns to fit would be the one failure mode §2.1
/// exists to forbid. S6 §2.10's `cols - 7` text budget is superseded.
pub const MIN_INTACT_COLS: usize = GUTTER_W + TITLE_W + REPO_W + 4;

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
    let n = display_cells(s);
    if n <= w {
        let mut out = String::from(s);
        out.push_str(&" ".repeat(w - n));
        return out;
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        // Reserve the ellipsis' own cell. A wide glyph that would straddle the
        // boundary is dropped whole, so the column never over-runs by one.
        if used + cw > w - 1 {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push(ELLIPSIS);
    out.push_str(&" ".repeat(w - used - 1));
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
pub fn render_rows(rows: &[Row], cols: usize) -> Vec<String> {
    let any_selected = rows.iter().any(|r| r.selected);
    rows.iter()
        .map(|row| render_row(row, cols, any_selected))
        .collect()
}

fn hue(ink: Option<u8>) -> Rgb {
    ink.and_then(|i| PALETTE.get(usize::from(i)))
        .map_or(UNTINTED, |(c, _)| *c)
}

fn render_row(row: &Row, cols: usize, any_selected: bool) -> String {
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

    // Only `summary` flexes (LEDGER D9). `saturating_sub` rather than a floor
    // check: a 0-width budget must render nothing, not panic.
    let body = cols.saturating_sub(GUTTER_W + 2); // minus right margin and cap
    let summary_w = body.saturating_sub(TITLE_W + REPO_W + 2);

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
                    out.push_str(&clamp(title, TITLE_W));
                    out.push_str(RESET);
                    out.push_str(&o);
                }
                None => {
                    out.push_str(&o);
                    out.push_str(&" ".repeat(TITLE_W));
                }
            }
            out.push_str(&o);
            out.push(' '); // col 17
            // Cols 18–24. Tinted TEXT, keyed by repo root — one repo is one
            // colour everywhere, forever (lock §4).
            out.push_str(&ink(hue(*repo_ink)));
            out.push_str(&clamp(repo, REPO_W));
            out.push_str(&o);
            out.push_str(&o);
            out.push(' '); // col 25
            // Cols 26–42. The selected row leaves the summary at the inherited
            // foreground; every other row is faded fujiWhite.
            if fade > 0.0 {
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
        for cols in [MIN_INTACT_COLS, 30, DESIGN_COLS, 80, 200] {
            for line in render_rows(&fleet(), cols) {
                let width = display_cells(&strip_sgr(&line));
                assert_eq!(width, cols, "at cols={cols}: {line:?}");
            }
        }
    }

    /// Selection must not move a single column (lock §2.2) — the cap columns
    /// are reserved on every row precisely so the eye scans one edge.
    #[test]
    fn title_starts_at_column_ten_selected_or_not() {
        let unselected = agent(RowStatus::Working, Provenance::Branch, Some("S6-GUT"), "x");
        let selected = Row {
            selected: true,
            ..unselected.clone()
        };
        for row in [unselected, selected] {
            let bare = strip_sgr(&render_rows(std::slice::from_ref(&row), DESIGN_COLS)[0]);
            let cols: Vec<char> = bare.chars().collect();
            assert_eq!(
                cols[GUTTER_W..GUTTER_W + TITLE_W]
                    .iter()
                    .collect::<String>(),
                "S6-GUT ",
                "selected={}",
                row.selected
            );
        }
    }

    /// A missing glyph renders a blank cell and does not reflow the row (lock
    /// §2.1). A main checkout is the deliberate blank, and it is the most
    /// common row — so if absence reflowed, most of the bar would be ragged.
    #[test]
    fn an_absent_glyph_blanks_its_cell_without_reflowing() {
        let main = agent(RowStatus::Idle, Provenance::Main, Some("TITLE"), "s");
        let worktree = agent(RowStatus::Idle, Provenance::Worktree, Some("TITLE"), "s");
        let [main, worktree] =
            [main, worktree].map(|r| strip_sgr(&render_rows(&[r], DESIGN_COLS)[0]));

        let cell = |s: &str, i: usize| s.chars().nth(i).unwrap();
        assert_eq!(cell(&main, 7), ' ', "col 8 is blank for a main checkout");
        assert_eq!(cell(&worktree, 7), '\u{168c2}');
        // Same width, same text origin: the blank cost exactly one column.
        assert_eq!(display_cells(&main), display_cells(&worktree));
        assert_eq!(
            main.chars().skip(GUTTER_W).collect::<String>(),
            worktree.chars().skip(GUTTER_W).collect::<String>()
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
        let line = &render_rows(&[row], DESIGN_COLS)[0];
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
            if (GUTTER_W..GUTTER_W + TITLE_W).contains(&i) {
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

    /// Degenerate widths must not panic. Below `MIN_INTACT_COLS` the fixed
    /// columns hold and the row is WIDER than the pane (LEDGER D9) — recorded
    /// here so the behaviour is a decision, not a surprise. Collapsed geometry
    /// is unratified (lock §3) and is S8's to design.
    #[test]
    fn degenerate_widths_do_not_panic() {
        for cols in [0, 1, 9, 20, MIN_INTACT_COLS, DESIGN_COLS, 200] {
            for (i, line) in render_rows(&fleet(), cols).iter().enumerate() {
                let width = display_cells(&strip_sgr(line));
                if cols >= MIN_INTACT_COLS {
                    assert_eq!(width, cols);
                } else {
                    assert!(width <= MIN_INTACT_COLS, "row {i} at cols={cols}: {width}");
                }
            }
        }
        assert_eq!(clamp("anything", 0), "");
    }

    /// The picture, not a fragment. Regenerate by eye against
    /// `cargo run -p clave-bar --example bar-preview` — a diff here is a
    /// deliberate design change or a bug, and both want a human looking.
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
        assert_eq!(render_rows(&rows, DESIGN_COLS), expected);
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
        let unfocused = render_rows(std::slice::from_ref(&row), DESIGN_COLS).remove(0);
        let mut other = agent(RowStatus::Idle, Provenance::Main, None, "s");
        other.selected = true;
        let faded = &render_rows(&[row, other], DESIGN_COLS)[0];

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
    #[test]
    fn mix_rounds_ties_to_even() {
        // blue: 186 + (40 - 186) * 0.25 = 149.5 -> 150
        assert_eq!(RULE_INK.mix(BASE, FADE), Rgb(173, 169, 150));
    }
}
