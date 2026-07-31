//! clave sidebar — the LOCKED visual design, rendered.
//!
//!     cargo run -p clave-bar --example bar-preview
//!
//! An ILLUSTRATION of the sidebar design ratified 2026-07-25 — not its
//! authority. `docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`
//! is authoritative for every ruling, number and rationale; where this example
//! and that document disagree, the document wins and this file is the bug.
//!
//! Every row below comes from `clave_bar::render::render_rows` — the same
//! function the plugin renders with. That is the whole point of this file
//! existing in Rust: the Python original it replaces was a SECOND
//! implementation of the same geometry, and two renders of one design diverge.
//! Move a column in `render.rs` and this preview changes with it, or the width
//! assertion below fails; it can no longer drift silently.
//!
//! It still asserts its own core invariant, as the Python did: every row is
//! exactly `DESIGN_COLS` display CELLS, measured in cells rather than code
//! points.
//!
//! GLYPH RULE — load-bearing (lock §5.4). Every glyph below is a `\u{...}`
//! escape, never a literal character. During the design rounds literal glyphs
//! were silently lost in transit twice; the first loss was misdiagnosed as
//! missing font coverage and nearly constrained the whole design to one Unicode
//! plane. Escapes survive every tool in the chain.

use clave_bar::render::{
    CHIP_INK, COLLAPSED_DESIGN_COLS, DESIGN_COLS, PALETTE, Provenance, RESET, Rgb, Row, RowContent,
    RowStatus, Widths, display_cells, render_rows, strip_sgr,
};

const BOLD: &str = "\u{1b}[1m";
/// The preview's own chrome — frame, ruler and column map. Not part of the
/// design; it is the graph paper the design is drawn on.
const DIM: Rgb = Rgb(0x71, 0x7C, 0x7C);

fn agent(
    status: RowStatus,
    battery: Option<u8>,
    provenance: Provenance,
    repo: &str,
    repo_ink: u8,
    title: Option<(&str, u8)>,
    summary: &str,
) -> Row {
    Row {
        content: RowContent::Agent {
            status,
            battery,
            provenance,
            title: title.map(|(t, _)| String::from(t)),
            title_ink: title.map(|(_, i)| i),
            repo: String::from(repo),
            repo_ink: Some(repo_ink),
            summary: String::from(summary),
        },
        selected: false,
    }
}

/// One repo is one ink forever (lock §4); these indices stand in for the
/// store-backed round-robin allocation that will assign them for real.
const CLAVE: u8 = 0;
const DOTFILES: u8 = 1;
const API_SVC: u8 = 2;
const INFRA: u8 = 4;
const WEBAPP: u8 = 5;

fn fleet() -> Vec<Row> {
    let mut rows = vec![
        agent(
            RowStatus::NeedsYou,
            Some(1),
            Provenance::Main,
            "dotfiles",
            DOTFILES,
            None,
            "I just passed the spec over",
        ),
        agent(
            RowStatus::Working,
            Some(3),
            Provenance::Worktree,
            "clave",
            CLAVE,
            Some(("S6-GUT", 5)),
            "picking the gutter set",
        ),
        agent(
            RowStatus::Working,
            Some(2),
            Provenance::Branch,
            "api-svc",
            API_SVC,
            Some(("API-GW", 3)),
            "retry budget audit",
        ),
        agent(
            RowStatus::Done,
            Some(0),
            Provenance::Main,
            "clave",
            CLAVE,
            Some(("UX-DOC", 6)),
            "there was a stale anchor",
        ),
        Row {
            content: RowContent::Terminal {
                name: String::from("Tab #16"),
            },
            selected: false,
        },
        agent(
            RowStatus::Stale,
            Some(4),
            Provenance::Worktree,
            "clave",
            CLAVE,
            Some(("KDL-GRD", 7)),
            "the real parser test",
        ),
        agent(
            RowStatus::Dormant,
            Some(0),
            Provenance::Worktree,
            "clave",
            CLAVE,
            None,
            "worktree doctor work",
        ),
        agent(
            RowStatus::Dormant,
            Some(1),
            Provenance::Branch,
            "infra",
            INFRA,
            Some(("CFG", 1)),
            "config plumbing",
        ),
        agent(
            RowStatus::Idle,
            Some(2),
            Provenance::Branch,
            "webapp",
            WEBAPP,
            Some(("API-V2", 2)),
            "cutover plan review",
        ),
    ];
    rows[1].selected = true;
    rows
}

/// `cols`/`widths` are parameters (task 1.5, LEDGER D16) so the same framed
/// box — ruler, border, per-row width assertion — draws both the ratified
/// expanded render and every collapsed candidate. Behaviour at the original
/// call site (`cols = DESIGN_COLS`, `widths = Widths::EXPANDED`) is unchanged;
/// this is a signature widening, not a rewrite.
fn bar(rows: &[Row], cols: usize, widths: Widths, label: &str) {
    let ruler: String = (0..cols)
        .map(|i| char::from_digit(((i + 1) % 10) as u32, 10).unwrap())
        .collect();
    let dim = DIM.fg();
    let rule = "\u{2500}".repeat(cols);
    println!("\n  {BOLD}{label}{RESET}");
    println!("  {dim}{ruler}\n  \u{250c}{rule}\u{2510}{RESET}");
    for (line, row) in render_rows(rows, cols, widths).iter().zip(rows) {
        // The lock CLAIMS every row is exactly `cols` cells. Prove it rather
        // than asserting it in prose: strip the SGR sequences and measure the
        // remainder in display cells. A miscounted glyph fails the preview
        // loudly instead of shipping a ragged bar.
        let width = display_cells(&strip_sgr(line));
        assert_eq!(width, cols, "row is {width} cells: {row:?}");
        println!("  {dim}\u{2502}{RESET}{line}{dim}\u{2502}{RESET}");
    }
    println!("  {dim}\u{2514}{rule}\u{2518}{RESET}");
}

/// The screenshot fleet (`--showcase`): the row vocabulary in one frame, with
/// NO preview chrome — no ruler, no border, no column map — so a screenshot of
/// this is a screenshot of the bar rather than of the graph paper it is drawn
/// on. Used for the README until a real capture replaces it.
///
/// `battery: None` throughout, deliberately. S7 has not landed, so the shipped
/// bar renders that cell blank, and a promotional image is the last place to
/// show a column the code does not fill yet.
///
/// The summaries copy the SHAPE of real `ai-title` values, sampled from the
/// local transcript corpus 2026-07-31: sentence case, verb first, no trailing
/// period, 13-60 characters. They are invented rather than copied — the corpus
/// is the maintainer's own work and this repo is public — but a fixture that
/// invents the format too would misrepresent the column in the one image most
/// people will ever see. Several run past the 25-cell summary column and
/// truncate, which is the honest common case.
fn showcase() -> Vec<Row> {
    let mut rows = vec![
        agent(
            RowStatus::NeedsYou,
            None,
            Provenance::Branch,
            "api-svc",
            API_SVC,
            Some(("AUTH-7", 3)),
            "Rotate the signing keys",
        ),
        agent(
            RowStatus::Working,
            None,
            Provenance::Worktree,
            "clave",
            CLAVE,
            Some(("S6-GUT", 5)),
            "Wire the status column into render_rows",
        ),
        agent(
            RowStatus::Done,
            None,
            Provenance::Main,
            "webapp",
            WEBAPP,
            Some(("CART-99", 6)),
            "Fix cart total rounding mismatch",
        ),
        Row {
            content: RowContent::Terminal {
                name: String::from("shell"),
            },
            selected: false,
        },
        agent(
            RowStatus::Idle,
            None,
            Provenance::Main,
            "clave",
            CLAVE,
            None,
            "Review the spawn identity gate",
        ),
        agent(
            RowStatus::Failed,
            None,
            Provenance::Branch,
            "infra",
            INFRA,
            Some(("DNS-TTL", 1)),
            "Debug staging rollout DNS timeout",
        ),
        Row {
            content: RowContent::Terminal {
                name: String::from("logs"),
            },
            selected: false,
        },
        agent(
            RowStatus::Stale,
            None,
            Provenance::Worktree,
            "clave",
            CLAVE,
            Some(("KDL-GRD", 7)),
            "Validate generated KDL artifacts",
        ),
        agent(
            RowStatus::Dormant,
            None,
            Provenance::Main,
            "notes",
            DOTFILES,
            Some(("ZSH", 2)),
            "Tidy the shell startup files",
        ),
    ];
    rows[1].selected = true;
    rows
}

/// Bare rows, nothing else on stdout. The width assertion still runs — a
/// screenshot of a ragged bar would be worse than no screenshot.
fn print_showcase() {
    let rows = showcase();
    for (line, row) in render_rows(&rows, DESIGN_COLS, Widths::EXPANDED)
        .iter()
        .zip(&rows)
    {
        let width = display_cells(&strip_sgr(line));
        assert_eq!(width, DESIGN_COLS, "row is {width} cells: {row:?}");
        println!("{line}");
    }
}

fn main() {
    // `--showcase` prints the screenshot frame and exits: the chrome below is
    // for reading the design, not for showing it.
    if std::env::args().any(|a| a == "--showcase") {
        print_showcase();
        return;
    }
    let dim = DIM.fg();
    let bar_rule = "\u{2550}".repeat(78);
    println!(
        "\n{BOLD}{bar_rule}\nclave sidebar \u{2014} locked visual design \
         (2026-07-25)\n{bar_rule}{RESET}"
    );
    bar(
        &fleet(),
        DESIGN_COLS,
        Widths::EXPANDED,
        &format!("expanded \u{2014} {DESIGN_COLS} columns"),
    );

    column_map();

    println!("\n  {BOLD}palette \u{2014} 8 kanagawa hues, round-robin{RESET}\n");
    for (i, (hue, name)) in PALETTE.iter().enumerate() {
        let (fg, bg, hex) = (hue.fg(), hue.bg(), hue.hex());
        let chip = CHIP_INK.fg();
        println!(
            "   {dim}{i}{RESET} {fg}\u{2588}\u{2588}\u{2588}\u{2588}{RESET}  \
             {fg}repo-name{RESET}   {bg}{chip} TITLE {RESET}   {dim}{hex}  {name}{RESET}"
        );
    }
    println!();

    collapsed();
}

/// Where each field starts and ends, for a profile at a given width — the same
/// arithmetic `render_row` lays the row out with, so the map below cannot
/// describe a layout the renderer does not produce.
fn field_spans(w: Widths, cols: usize) -> [(usize, usize); 3] {
    let title = (10, 9 + w.title);
    let repo = (title.1 + 2, title.1 + 1 + w.repo);
    let summary = (repo.1 + 2, cols - 2);
    [title, repo, summary]
}

fn span(s: (usize, usize)) -> String {
    if s.0 == s.1 {
        format!("{}", s.0)
    } else {
        format!("{}-{}", s.0, s.1)
    }
}

/// The column map, DERIVED from the `Widths` profiles rather than restated in
/// prose. Review finding 8 flagged the old hard-coded table as the one part of
/// this preview that could still silently diverge from the renderer — and it is
/// the part a reader trusts most. Moving a column now moves these numbers too.
fn column_map() {
    let dim = DIM.fg();
    let (x, c) = (Widths::EXPANDED, Widths::COLLAPSED);
    let (xs, cs) = (
        field_spans(x, DESIGN_COLS),
        field_spans(c, COLLAPSED_DESIGN_COLS),
    );

    println!(
        "\n  {dim}COLUMN MAP \u{2014} the 9-cell gutter is IDENTICAL in both states (D16);
  only title, repo and summary move. summary = cols \u{2212} 13 \u{2212} title \u{2212} repo,
  and it is the ONLY flexible column (D9). Odd cells 3, 5, 7 and 9 are spaces."
    );
    println!(
        "\n  {:<13}{:<11}{:<12}what it carries",
        "field", "expanded", "collapsed"
    );
    for (field, xa, ca, note) in [
        (
            "left cap",
            span((1, 1)),
            span((1, 1)),
            "powerline half-circle, selected row only",
        ),
        (
            "status",
            span((2, 2)),
            span((2, 2)),
            "the COLOUR is the state",
        ),
        (
            "rule",
            span((4, 4)),
            span((4, 4)),
            "U+2502 in fujiWhite, splits status from battery",
        ),
        (
            "battery",
            span((6, 6)),
            span((6, 6)),
            "context level (S7); console mark on a terminal tab",
        ),
        (
            "provenance",
            span((8, 8)),
            span((8, 8)),
            "the repo ink; BLANK for a main checkout",
        ),
        (
            "title",
            span(xs[0]),
            span(cs[0]),
            "filled chip, dark text; blank when never renamed",
        ),
        (
            "repo",
            span(xs[1]),
            span(cs[1]),
            "tinted text, one colour per repo forever",
        ),
        (
            "summary",
            span(xs[2]),
            span(cs[2]),
            "flexes; no ellipsis at \u{2264}4 cells (D18)",
        ),
        (
            "right margin",
            span((DESIGN_COLS - 1, DESIGN_COLS - 1)),
            span((COLLAPSED_DESIGN_COLS - 1, COLLAPSED_DESIGN_COLS - 1)),
            "",
        ),
        (
            "right cap",
            span((DESIGN_COLS, DESIGN_COLS)),
            span((COLLAPSED_DESIGN_COLS, COLLAPSED_DESIGN_COLS)),
            "selected row only",
        ),
    ] {
        println!(
            "{}",
            format!("  {field:<13}{xa:<11}{ca:<12}{note}").trim_end()
        );
    }
    println!(
        "
  Cap columns are reserved on EVERY row so the selected row does not shift one
  column right of its neighbours. Verified: title starts at column 10 whether
  or not the row is selected, and in either state.{RESET}"
    );
}

/// The chosen collapsed profile (task 1.5 / LEDGER D17), rendered the same
/// way as the expanded section above it: `bar()` draws the ruler, the framed
/// box and the per-row width assertion for either profile identically — the
/// candidates comparison this replaces has served its purpose, Ollie picked
/// `(title 7, repo 3)` at 30 columns by looking at it.
fn collapsed() {
    let dim = DIM.fg();

    let widths = Widths::COLLAPSED;
    let cols = COLLAPSED_DESIGN_COLS;
    // Derived, not hard-coded: the same arithmetic `render_row` uses
    // internally once `cols` clears the floor (`13 + title + repo` — LEDGER
    // D12's arithmetic, generalised to a profile by D16), so this number is
    // guaranteed to match what the box below actually shows.
    let summary_w = cols.saturating_sub(widths.min_intact_cols());
    bar(
        &fleet(),
        cols,
        widths,
        &format!(
            "collapsed \u{2014} {cols} columns \u{2014} title {}, repo {}, summary {}",
            widths.title, widths.repo, summary_w
        ),
    );

    let separation = DESIGN_COLS - cols;
    println!("\n  {dim}separation from {DESIGN_COLS}: {separation}{RESET}");
    println!(
        "  {dim}LEDGER D15 requires separation > 10 (the widest `width_seek` \
         acceptance half-band) \u{2014} {separation} clears it. D21: below the \
         full band, so `width_seek` refuses the overlap outright.{RESET}"
    );
}
