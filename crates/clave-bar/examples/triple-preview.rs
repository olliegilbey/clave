//! clave sidebar — the FOUR-LINE card, rendered.
//!
//!     cargo run -p clave-bar --example triple-preview
//!     cargo run -p clave-bar --example triple-preview -- --animate [secs]
//!
//! The preview of the lock ratified 2026-09-08 — not its authority. The lock in
//! `docs/superpowers/specs/` is authoritative for every ruling, number and
//! rationale; where this example and that document disagree, the document wins
//! and this file is the bug.
//!
//! Every card below comes from `clave_bar::render::render_rows` in
//! `RowHeight::Card` — the same call the plugin renders with, reaching the same
//! `card::render_card` inside it. Through the design rounds this file carried
//! its OWN copy of the geometry and could drift from the lock; the port ended
//! that. The mock fleet stayed, the second renderer did not. Move a cell in
//! `card.rs` and this preview moves with it, or the width assertion below fails.
//!
//! ```text
//!   line 1:  status  │ chip-pill  summary
//!   line 2:  prov    │ repo [branch]  #PR   provider model effort
//!   line 3:  subs    │ tokens  clock  ask
//!   line 4:  the shadow rule — a hairline that closes the card
//! ```
//!
//! All three of the cells this preview once rendered blank — the subagent
//! mark, `wants` and the clock — are wired, and the fleet below fills them.
//! Blank is still the meaning wherever a row has no reading; it is now the
//! card saying so, rather than the source not existing yet.
//!
//! COLLAPSED is 16 columns and a strict LEFT-CROP of the 48-column expanded
//! card — never a re-arrangement, so a cell's column is its priority. What
//! survives: the status mark, the chip, the repo, the token count, and the time
//! since you last touched the row.
//!
//! `--animate` is the one thing a static render cannot confirm. The status
//! spinner's frames arrive by terminal fallback out of Menlo on most machines,
//! so their weight and baseline show up nowhere else — not in a golden, not in
//! the filmstrip below.

use clave_bar::render::{
    Provenance, RESET, Rgb, Row, RowContent, RowHeight, RowStatus, TermStatus, Theme, Widths,
    display_cells, render_rows, strip_sgr,
};

/// The preview's own chrome — the profile captions. Not part of the design.
const DIM: Rgb = Rgb(0x71, 0x7C, 0x7C);

/// One mock agent card, field-for-field the shape `RowContent::Agent` carries.
struct A {
    status: RowStatus,
    prov: Provenance,
    chip: Option<&'static str>,
    chip_ink: Option<u8>,
    repo: &'static str,
    repo_ink: Option<u8>,
    branch: &'static str,
    pr: Option<u32>,
    provider: Option<&'static str>,
    model: Option<&'static str>,
    effort: Option<&'static str>,
    /// The ramp level AND the count it was bucketed from, together, the way a
    /// real snapshot carries them — the ink is the level's band and the text
    /// the exact magnitude, so a fixture cannot show a card no live row could.
    battery: Option<(u8, u32)>,
    elapsed: &'static str,
    summary: &'static str,
    /// What the row is blocked on. Set only on a `NeedsYou` row — the host
    /// writes it nowhere else, so a fixture that filled it on a working row
    /// would preview a card the product cannot produce.
    wants: Option<&'static str>,
    /// Whether anything is still running under this row.
    subs: bool,
    selected: bool,
    dormant: bool,
}

impl Default for A {
    fn default() -> A {
        A {
            status: RowStatus::Working,
            prov: Provenance::Main,
            chip: None,
            chip_ink: None,
            repo: "clave",
            repo_ink: Some(0),
            branch: "",
            pr: None,
            provider: Some("claude"),
            model: Some("opus"),
            effort: Some("hi"),
            battery: Some((5, 100_000)),
            elapsed: "1m",
            summary: "",
            wants: None,
            subs: false,
            selected: false,
            dormant: false,
        }
    }
}

impl A {
    fn row(self) -> Row {
        Row {
            content: RowContent::Agent {
                status: self.status,
                battery: self.battery.map(|(level, _)| level),
                tokens: self.battery.map(|(_, tokens)| tokens),
                provenance: self.prov,
                title: self.chip.map(String::from),
                title_ink: self.chip_ink,
                repo: self.repo.into(),
                repo_ink: self.repo_ink,
                summary: self.summary.into(),
                model: self.model.map(String::from),
                provider: self.provider.map(String::from),
                effort: self.effort.map(String::from),
                pr: self.pr,
                branch: self.branch.into(),
                elapsed: Some(self.elapsed.into()),
                wants: self.wants.map(String::from),
                subagents: self.subs,
            },
            selected: self.selected,
            dormant: self.dormant,
        }
    }
}

/// One mock terminal card: the tab name is the chip, the focused pane's last
/// foreground command is the summary, and provenance, branch and PR are
/// borrowed from the checkout exactly as an agent card's are.
struct T {
    name: &'static str,
    prov: Provenance,
    repo: &'static str,
    repo_ink: Option<u8>,
    branch: &'static str,
    command: &'static str,
    pr: Option<u32>,
    elapsed: &'static str,
}

impl T {
    fn row(self) -> Row {
        Row {
            content: RowContent::Terminal {
                name: self.name.into(),
                status: TermStatus::Idle,
                provenance: self.prov,
                repo: Some(self.repo.into()),
                repo_ink: self.repo_ink,
                command: self.command.into(),
                pr: self.pr,
                branch: self.branch.into(),
                elapsed: Some(self.elapsed.into()),
            },
            selected: false,
            dormant: false,
        }
    }
}

/// The fleet the design rounds were judged on: the corners of the variant
/// space, plus the cases the four-line card exists to serve — a row blocked on
/// you (whose spinner must STOP), a branch name the two-line card truncated, a
/// repo name past its budget, and the many rows that want nothing at all.
fn fleet() -> Vec<Row> {
    use Provenance::{Branch, Main, Worktree};
    use RowStatus::{Done, Failed, NeedsYou, Working};
    vec![
        // Blocked on a permission prompt. NOT working, so the mark holds still.
        A {
            status: NeedsYou,
            chip: Some("CLA-AI"),
            chip_ink: Some(3),
            branch: "fix/evlog-atomic-append",
            prov: Branch,
            pr: Some(256),
            effort: Some("xh"),
            battery: Some((4, 71_000)),
            elapsed: "27m",
            summary: "Clave AI assessment",
            // Tier 1: the tool name off the permission notification.
            wants: Some("Bash (cargo mutants)"),
            ..A::default()
        }
        .row(),
        // Blocked on a prose question — the tier the scribe still owes, so
        // the cell stays blank and the dot carries the row on its own.
        A {
            status: NeedsYou,
            chip: Some("BEACON"),
            chip_ink: Some(6),
            repo: "beacon",
            repo_ink: Some(2),
            battery: Some((6, 129_000)),
            elapsed: "17m",
            summary: "Olympus build",
            ..A::default()
        }
        .row(),
        A {
            subs: true,
            chip: Some("OLYMPUS"),
            chip_ink: Some(1),
            repo: "olympus",
            repo_ink: Some(1),
            branch: "chore/workspace-lint",
            prov: Worktree,
            model: Some("fable"),
            effort: Some("xh"),
            battery: Some((4, 80_000)),
            elapsed: "42s",
            summary: "Validate and fix codebase",
            ..A::default()
        }
        .row(),
        // The long-branch case: 23 cells, which the two-line card truncated.
        A {
            chip: Some("RECUT"),
            chip_ink: Some(5),
            branch: "statusline-battery",
            prov: Branch,
            battery: Some((9, 211_000)),
            elapsed: "20m",
            summary: "Recut, retag",
            ..A::default()
        }
        .row(),
        // Selected, with a PR.
        A {
            status: Working,
            chip: Some("CLV-M2"),
            chip_ink: Some(4),
            branch: "v022-prep",
            prov: Worktree,
            pr: Some(225),
            effort: Some("xh"),
            battery: Some((8, 130_000)),
            elapsed: "5m",
            summary: "Goal is shipping v0.2.2 cleanly",
            selected: true,
            ..A::default()
        }
        .row(),
        // No chip: the summary claims the pill's nine columns AND its gap, so
        // its text starts flush with the repo and the token count below it.
        A {
            repo: "rot-reducer",
            repo_ink: Some(4),
            battery: Some((9, 176_000)),
            elapsed: "3h",
            summary: "Rot reducer injection frequency analysis",
            ..A::default()
        }
        .row(),
        // Done, OpenAI, no effort reading, no PR.
        A {
            status: Done,
            chip: Some("DJ"),
            chip_ink: Some(4),
            repo: "beacon",
            repo_ink: Some(2),
            provider: Some("openai"),
            model: Some("gpt-5"),
            effort: None,
            battery: Some((7, 169_000)),
            elapsed: "1d",
            summary: "Consecutive artists in DJ set",
            ..A::default()
        }
        .row(),
        // Repo name overflowing its budget, PR present, unknown provider — the
        // brand cell blanks rather than inventing a mark.
        A {
            repo: "clave-website",
            repo_ink: Some(6),
            branch: "hero-copy",
            prov: Branch,
            pr: Some(12),
            provider: None,
            model: None,
            effort: None,
            battery: Some((3, 55_000)),
            elapsed: "30m",
            summary: "Landing page hero copy rewrite pass",
            ..A::default()
        }
        .row(),
        // Failed, high burn, worktree.
        A {
            status: Failed,
            chip: Some("MIGRATE"),
            chip_ink: Some(3),
            repo: "market-scanner",
            repo_ink: Some(1),
            branch: "pg17-migrate",
            prov: Worktree,
            pr: Some(88),
            model: Some("sonnet"),
            battery: Some((10, 201_000)),
            elapsed: "4h",
            summary: "Postgres 15 to 17 migration runbook",
            ..A::default()
        }
        .row(),
        // A terminal tab: the TERM pill, and the token cell says TERM.
        T {
            name: "shell",
            prov: Main,
            repo: "clave",
            repo_ink: Some(0),
            branch: "",
            command: "cargo test --workspace",
            pr: None,
            elapsed: "2m",
        }
        .row(),
        // Dormant: absolute fade, no reading in flight.
        A {
            status: RowStatus::Dormant,
            chip: Some("FOOTER"),
            chip_ink: Some(0),
            repo: "resumaker",
            repo_ink: Some(7),
            battery: Some((4, 73_000)),
            elapsed: "2w",
            summary: "ollie.gg company details footer",
            dormant: true,
            ..A::default()
        }
        .row(),
    ]
}

/// Seconds a frame holds, overridable: `--animate 0.2`. Not one second — at a
/// second a card stutters rather than breathes. This is the plugin's own
/// `ANIM_FRAME_SECS`, restated so the preview moves at the shipping cadence.
const DEFAULT_FRAME_SECS: f64 = 0.2;

/// The whole fleet, redrawn in place until Ctrl-C. The ONLY way to judge an
/// animation: a filmstrip shows the glyphs but not whether the motion reads as
/// thinking or as noise.
fn animate(frame_secs: f64) {
    print!("\u{1b}[?25l"); // hide the cursor
    let rows = fleet();
    let height = rows.len() * RowHeight::Card.lines_per_row();
    for frame in 0.. {
        print!("\u{1b}[H\u{1b}[2J");
        println!(
            "{}THINKING — {frame_secs}s a frame, Ctrl-C to stop{RESET}\n",
            DIM.fg()
        );
        for line in render_rows(
            &rows,
            48,
            height,
            Widths::EXPANDED,
            &Theme::default(),
            RowHeight::Card,
            frame,
        ) {
            println!("{line}");
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(frame_secs));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--animate") {
        let secs = args
            .get(i + 1)
            .and_then(|a| a.parse().ok())
            .unwrap_or(DEFAULT_FRAME_SECS);
        animate(secs);
        return;
    }
    let profiles = [
        (
            "EXPANDED — 48 columns",
            RowHeight::Card.target_cols(false),
            Widths::EXPANDED,
        ),
        (
            "COLLAPSED — 16 columns, a strict left-crop",
            RowHeight::Card.target_cols(true),
            Widths::COLLAPSED,
        ),
    ];
    for (name, cols, widths) in profiles {
        println!("\n{}{name}{RESET}\n", DIM.fg());
        let rows = fleet();
        // A pane tall enough for every card, so the preview shows the whole
        // fleet rather than the viewport's slice of it.
        let lines = render_rows(
            &rows,
            cols,
            rows.len() * RowHeight::Card.lines_per_row(),
            widths,
            &Theme::default(),
            RowHeight::Card,
            0,
        );
        for (i, line) in lines.iter().enumerate() {
            let w = display_cells(&strip_sgr(line));
            assert_eq!(w, cols, "{name}: line {i} rendered {w} cells, want {cols}");
            println!("{line}");
        }
    }
    println!();
}
