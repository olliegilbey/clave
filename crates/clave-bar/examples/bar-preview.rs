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
    RowStatus, TermStatus, Widths, display_cells, render_rows, strip_sgr,
};

#[path = "shared/showcase_fixture.rs"]
mod showcase_fixture;
use showcase_fixture::{API_SVC, CLAVE, DOTFILES, INFRA, WEBAPP, agent, showcase, terminal};

const BOLD: &str = "\u{1b}[1m";
/// The preview's own chrome — frame, ruler and column map. Not part of the
/// design; it is the graph paper the design is drawn on.
const DIM: Rgb = Rgb(0x71, 0x7C, 0x7C);

fn fleet() -> Vec<Row> {
    let mut rows = vec![
        agent(
            RowStatus::NeedsYou,
            Some((1, 15_000)),
            Provenance::Main,
            "dotfiles",
            DOTFILES,
            None,
            "I just passed the spec over",
        ),
        agent(
            RowStatus::Working,
            Some((3, 45_000)),
            Provenance::Worktree,
            "clave",
            CLAVE,
            Some(("S6-GUT", 5)),
            "picking the gutter set",
        ),
        agent(
            RowStatus::Working,
            Some((2, 30_000)),
            Provenance::Branch,
            "api-svc",
            API_SVC,
            Some(("API-GW", 3)),
            "retry budget audit",
        ),
        agent(
            RowStatus::Done,
            Some((0, 4_000)),
            Provenance::Main,
            "clave",
            CLAVE,
            Some(("UX-DOC", 6)),
            "there was a stale anchor",
        ),
        terminal(
            "Tab #16",
            TermStatus::Running,
            Provenance::Worktree,
            Some(("clave", CLAVE)),
            "cargo test --workspace",
        ),
        // The nothing-known-yet default: a freshly opened tab, at its prompt,
        // before any pane fact has arrived.
        Row {
            content: RowContent::terminal("Tab #3"),
            selected: false,
            dormant: false,
        },
        agent(
            RowStatus::Stale,
            Some((4, 66_000)),
            Provenance::Worktree,
            "clave",
            CLAVE,
            Some(("KDL-GRD", 7)),
            "the real parser test",
        ),
        agent(
            RowStatus::Dormant,
            Some((0, 9_000)),
            Provenance::Worktree,
            "clave",
            CLAVE,
            None,
            "worktree doctor work",
        ),
        agent(
            RowStatus::Dormant,
            Some((1, 22_000)),
            Provenance::Branch,
            "infra",
            INFRA,
            Some(("CFG", 1)),
            "config plumbing",
        ),
        agent(
            RowStatus::Idle,
            Some((2, 38_000)),
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
    for (line, row) in render_rows(rows, cols, rows.len(), widths).iter().zip(rows) {
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

/// Bare rows, nothing else on stdout. The width assertion still runs — a
/// screenshot of a ragged bar would be worse than no screenshot.
fn print_showcase() {
    let rows = showcase();
    for (line, row) in render_rows(&rows, DESIGN_COLS, rows.len(), Widths::EXPANDED)
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
fn field_spans(w: Widths, cols: usize) -> [(usize, usize); 4] {
    // The battery cell opens at column 6 in both profiles and runs as wide as
    // the profile gives it — one column for the glyph, four for the count (#105).
    let battery = (6, 5 + w.battery.width());
    let title = (w.gutter() + 1, w.gutter() + w.title);
    let repo = (title.1 + 2, title.1 + 1 + w.repo);
    let summary = (repo.1 + 2, cols - 2);
    [battery, title, repo, summary]
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
        "\n  {dim}COLUMN MAP \u{2014} everything left of the battery is IDENTICAL in both
  states (D16); the battery cell is 4 cells expanded and 1 collapsed (#105), and
  title, repo and summary follow it. summary = cols \u{2212} gutter \u{2212} 4 \u{2212} title \u{2212} repo,
  and it is the ONLY flexible column (D9), so the wider battery cell was paid for
  out of it. The cells between the named ones are single spaces."
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
            span(xs[0]),
            span(cs[0]),
            "token count expanded, ramp glyph collapsed (S7, #105)",
        ),
        (
            "provenance",
            span((x.gutter() - 1, x.gutter() - 1)),
            span((c.gutter() - 1, c.gutter() - 1)),
            "the repo ink; BLANK for a main checkout",
        ),
        (
            "title",
            span(xs[1]),
            span(cs[1]),
            "filled chip, dark text; blank when never renamed",
        ),
        (
            "repo",
            span(xs[2]),
            span(cs[2]),
            "tinted text, one colour per repo forever",
        ),
        (
            "summary",
            span(xs[3]),
            span(cs[3]),
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
  column right of its neighbours. Verified: the chip starts at its profile's
  first text column whether or not the row is selected.{RESET}"
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
    // Derived, not hard-coded: the same arithmetic `render_row` uses internally
    // once `cols` clears the floor (`gutter + 4 + title + repo` — LEDGER D12's
    // arithmetic, generalised to a profile by D16 and to a variable-width battery
    // cell by #105), so this number is guaranteed to match what the box below
    // actually shows.
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
        "  {dim}LEDGER D39: the separation is now a DESIGN number, not a \
         tolerance. Both widths are declared in the layout and zellij switches \
         between them, so nothing has to tell the two apart by measuring \
         \u{2014} what {separation} columns buys is a visibly different bar.{RESET}"
    );
}
