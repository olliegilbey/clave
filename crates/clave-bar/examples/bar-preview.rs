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
    CHIP_INK, DESIGN_COLS, PALETTE, Provenance, RESET, Rgb, Row, RowContent, RowStatus, Widths,
    display_cells, render_rows, strip_sgr,
};

const BOLD: &str = "\u{1b}[1m";
/// The preview's own chrome — frame, ruler and column map. Not part of the
/// design; it is the graph paper the design is drawn on.
const DIM: Rgb = Rgb(0x71, 0x7C, 0x7C);

fn agent(
    status: RowStatus,
    battery: u8,
    provenance: Provenance,
    repo: &str,
    repo_ink: u8,
    title: Option<(&str, u8)>,
    summary: &str,
) -> Row {
    Row {
        content: RowContent::Agent {
            status,
            battery: Some(battery),
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
            1,
            Provenance::Main,
            "dotfiles",
            DOTFILES,
            None,
            "I just passed the spec over",
        ),
        agent(
            RowStatus::Working,
            3,
            Provenance::Worktree,
            "clave",
            CLAVE,
            Some(("S6-GUT", 5)),
            "picking the gutter set",
        ),
        agent(
            RowStatus::Working,
            2,
            Provenance::Branch,
            "api-svc",
            API_SVC,
            Some(("API-GW", 3)),
            "retry budget audit",
        ),
        agent(
            RowStatus::Done,
            0,
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
            4,
            Provenance::Worktree,
            "clave",
            CLAVE,
            Some(("KDL-GRD", 7)),
            "the real parser test",
        ),
        agent(
            RowStatus::Dormant,
            0,
            Provenance::Worktree,
            "clave",
            CLAVE,
            None,
            "worktree doctor work",
        ),
        agent(
            RowStatus::Dormant,
            1,
            Provenance::Branch,
            "infra",
            INFRA,
            Some(("CFG", 1)),
            "config plumbing",
        ),
        agent(
            RowStatus::Idle,
            2,
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

fn main() {
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

    println!(
        "
  {dim}COLUMN MAP
     1      left cap   \u{2014} powerline half-circle, selected row only
     2      status     \u{2014} colour IS the state
     3      space
     4      rule       \u{2014} U+2502 in fujiWhite, separates status from battery
     5      space
     6      battery    \u{2014} context level (S7); console mark on a terminal tab
     7      space
     8      provenance \u{2014} tinted with the repo ink; BLANK for a main checkout
     9      space
    10-16   title      \u{2014} filled chip, dark text; blank when never renamed
    17      space
    18-24   repo       \u{2014} tinted text, one colour per repo forever
    25      space
    26-42   summary
    43      right margin
    44      right cap  \u{2014} selected row only

  Cap columns are reserved on EVERY row so the selected row does not shift
  one column right of its neighbours. Verified: title starts at column 10
  whether or not the row is selected.{RESET}"
    );

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

    collapsed_candidates();
}

/// A `Widths` candidate under consideration for D16's open pair, labelled with
/// the numbers that produced it (task 1.5).
struct Candidate {
    label: &'static str,
    widths: Widths,
    cols: usize,
}

/// Task 1.5 / LEDGER D16: collapsed is a WIDTH PROFILE (title, repo), not a
/// second layout — same `fleet()`, same `bar()`, different numbers. Renders
/// three candidates so the open pair (title/repo — "to be settled by looking,
/// not by arguing") gets picked by looking, not derived from prose. B and C
/// share `cols` on purpose: the comparison is where the characters go, not how
/// many there are.
fn collapsed_candidates() {
    let dim = DIM.fg();
    let bar_rule = "\u{2550}".repeat(78);
    println!("\n{BOLD}{bar_rule}\ncollapsed \u{2014} candidates (LEDGER D16)\n{bar_rule}{RESET}");

    let candidates = [
        Candidate {
            label: "A \u{2014} tight",
            widths: Widths { title: 5, repo: 3 },
            cols: 26,
        },
        Candidate {
            label: "B \u{2014} roomy",
            widths: Widths { title: 5, repo: 3 },
            cols: 30,
        },
        Candidate {
            label: "C \u{2014} title holds at 7",
            widths: Widths { title: 7, repo: 3 },
            cols: 30,
        },
    ];

    for c in &candidates {
        // Derived, not hard-coded: `summary_w = cols - min_intact_cols()` is
        // the same arithmetic `render_row` uses internally once `cols` clears
        // the floor (`13 + title + repo` — LEDGER D12's arithmetic,
        // generalised to a profile by D16), so this number is guaranteed to
        // match what the box below actually shows.
        let summary_w = c.cols.saturating_sub(c.widths.min_intact_cols());
        bar(
            &fleet(),
            c.cols,
            c.widths,
            &format!(
                "{} \u{2014} title {}, repo {}, cols {}, summary {}",
                c.label, c.widths.title, c.widths.repo, c.cols, summary_w
            ),
        );
    }

    println!(
        "\n  {dim}separation from {DESIGN_COLS}: {}{RESET}",
        candidates
            .iter()
            .map(|c| format!("{} = {}", c.label, DESIGN_COLS - c.cols))
            .collect::<Vec<_>>()
            .join("   ")
    );
    println!(
        "  {dim}LEDGER D15 requires separation > 10 (the widest `width_seek` \
         acceptance half-band) \u{2014} all three candidates clear it.{RESET}"
    );
}
