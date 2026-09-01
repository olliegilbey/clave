//! clave sidebar — the DOUBLE-HEIGHT card, rendered.
//!
//!     cargo run -p clave-bar --example double-preview
//!
//! The preview of the lock ratified 2026-08-26 — not its authority.
//! `docs/superpowers/specs/2026-08-26-double-height-card-lock.md` is
//! authoritative for every ruling, number and rationale; where this example and
//! that document disagree, the document wins and this file is the bug. It plays
//! the role `bar-preview.rs` plays for the single-line lock.
//!
//! Every card below comes from `clave_bar::render::render_rows` in
//! `RowHeight::Double` — the same call the plugin renders with, reaching the
//! same `card::render_card` inside it. During the design rounds this file
//! carried its OWN copy of the geometry, which is exactly the drift the
//! single-line preview was rewritten in Rust to end; the mock fleet stayed, the
//! second renderer did not. Move a cell in `card.rs` and this preview moves with
//! it, or the width assertion below fails.
//!
//! Two profiles, one card:
//!
//! ```text
//!   line 1:  status ╭ chip-pill  summary            tokens
//!   line 2:  prov   ╰ repo [branch]  #PR  provider  model   elapsed
//! ```
//!
//! COLLAPSED is 38 columns; EXPANDED is 48 and adds exactly two things — the
//! branch beside the repo on line 2, and a wider line-1 summary flex. Repo and
//! branch share one collective budget, so a long repo truncates before a branch
//! does. Unselected cards paint NO background (glass); the selection is a full
//! opaque bar; the zebra lives in the arc's alternating ink.
//!
//! GLYPH RULE — load-bearing (lock §5.4). Every glyph below is a `\u{...}`
//! escape, never a literal character: during the design rounds literal glyphs
//! were silently lost in transit twice.

use clave_bar::render::{
    Provenance, RESET, Rgb, Row, RowContent, RowHeight, RowStatus, TermStatus, Theme, Widths,
    display_cells, render_rows, strip_sgr,
};

/// The preview's own chrome — the profile captions. Not part of the design.
const DIM: Rgb = Rgb(0x71, 0x7C, 0x7C);

/// One mock agent card. Field-for-field the shape `RowContent::Agent` carries,
/// with a `Default` so each row below states only what makes it interesting.
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
    /// real snapshot carries them (#105) — the ink is the level's band and the
    /// text the exact magnitude, so a fixture cannot show a card no live row
    /// could produce.
    battery: Option<(u8, u32)>,
    elapsed: &'static str,
    summary: &'static str,
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
            model: Some("fable"),
            effort: Some("hi"),
            battery: Some((5, 100_000)),
            elapsed: "1m",
            summary: "",
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

/// The ratified fleet: the corners of the variant space in one frame — chip and
/// chipless, agent and terminal, both providers, main / branch / worktree, repo
/// names short, exactly at budget and overflowing, and the selected and dormant
/// cards.
fn fleet() -> Vec<Row> {
    use Provenance::{Branch, Main, Worktree};
    use RowStatus::{Done, Failed, NeedsYou};
    vec![
        A {
            status: NeedsYou,
            chip: Some("CORTI2"),
            chip_ink: Some(2),
            repo: "hermes",
            repo_ink: Some(2),
            battery: Some((6, 105_000)),
            elapsed: "3m",
            summary: "Qdos IR35 assessment: the contract",
            ..A::default()
        }
        .row(),
        A {
            chip: Some("HERMES"),
            chip_ink: Some(6),
            repo: "hermes",
            repo_ink: Some(2),
            model: Some("opus"),
            battery: Some((4, 78_000)),
            elapsed: "18m",
            summary: "Personal reflections and planning",
            ..A::default()
        }
        .row(),
        A {
            chip: Some("XPS"),
            chip_ink: Some(1),
            repo: "hermes",
            repo_ink: Some(2),
            battery: Some((10, 234_000)),
            elapsed: "1h",
            summary: "XPS dev server setup and deploy",
            ..A::default()
        }
        .row(),
        A {
            status: NeedsYou,
            chip: Some("REASSOC"),
            chip_ink: Some(3),
            battery: Some((5, 86_000)),
            elapsed: "12m",
            summary: "Clave session reassociation pass",
            ..A::default()
        }
        .row(),
        A {
            prov: Worktree,
            chip: Some("CLV-3"),
            chip_ink: Some(3),
            branch: "drive-launch",
            pr: Some(204),
            model: Some("sonnet"),
            battery: Some((7, 117_000)),
            elapsed: "45m",
            summary: "Drive launch",
            ..A::default()
        }
        .row(),
        A {
            prov: Worktree,
            chip: Some("COLOUR"),
            chip_ink: Some(5),
            branch: "colour",
            battery: Some((4, 79_000)),
            elapsed: "2h",
            summary: "Zellij theme passthrough spike",
            ..A::default()
        }
        .row(),
        T {
            name: "Tab #12",
            prov: Main,
            repo: "clave",
            repo_ink: Some(0),
            branch: "",
            command: "zsh",
            pr: None,
            elapsed: "7m",
        }
        .row(),
        A {
            prov: Worktree,
            chip: Some("CLV-M2"),
            effort: Some("xh"),
            chip_ink: Some(4),
            branch: "v022-prep",
            pr: Some(225),
            battery: Some((8, 130_000)),
            elapsed: "5m",
            summary: "Goal is shipping v0.2.2 cleanly",
            selected: true,
            ..A::default()
        }
        .row(),
        A {
            status: Done,
            chip: Some("DJ"),
            chip_ink: Some(4),
            repo: "hermes",
            repo_ink: Some(2),
            provider: Some("openai"),
            model: Some("gpt-5"),
            effort: None,
            battery: Some((4, 78_000)),
            elapsed: "3h",
            summary: "DJ queue setup",
            ..A::default()
        }
        .row(),
        // No renamed title: the summary fills the pill's columns too.
        A {
            repo: "hermes",
            repo_ink: Some(2),
            model: Some("opus"),
            battery: Some((2, 34_000)),
            elapsed: "2h",
            summary: "Create close conversation summary flow",
            ..A::default()
        }
        .row(),
        A {
            status: Done,
            prov: Branch,
            chip: Some("GTMSS"),
            chip_ink: Some(0),
            repo: "nalu",
            repo_ink: Some(5),
            branch: "gtm-pass",
            pr: Some(31),
            model: Some("haiku"),
            battery: Some((7, 119_000)),
            elapsed: "1d",
            summary: "GTM Landscape - and first pass",
            ..A::default()
        }
        .row(),
        // Terminal on a branch with a PR.
        T {
            name: "Tab #3",
            prov: Branch,
            repo: "clave",
            repo_ink: Some(0),
            branch: "double-rows",
            command: "gh pr checks --watch",
            pr: Some(232),
            elapsed: "3m",
        }
        .row(),
        // Terminal in a worktree, repo name filling the 9-cell budget exactly.
        T {
            name: "Tab #7",
            prov: Worktree,
            repo: "resumaker",
            repo_ink: Some(7),
            branch: "resume-fix",
            command: "just dev",
            pr: None,
            elapsed: "1h",
        }
        .row(),
        // Chipless + branch + PR + OpenAI, repo name OVERFLOWING to ellipsis.
        A {
            prov: Branch,
            repo: "clave-website",
            repo_ink: Some(6),
            branch: "hero-copy",
            pr: Some(12),
            provider: Some("openai"),
            model: Some("gpt-5"),
            effort: None,
            battery: Some((3, 55_000)),
            elapsed: "30m",
            summary: "Landing page hero copy rewrite pass",
            ..A::default()
        }
        .row(),
        // FAILED agent in a worktree with a PR, long repo, high burn.
        A {
            status: Failed,
            prov: Worktree,
            chip: Some("MIGRATE"),
            chip_ink: Some(3),
            repo: "market-scanner",
            repo_ink: Some(1),
            branch: "pg17-migrate",
            pr: Some(88),
            model: Some("sonnet"),
            battery: Some((10, 201_000)),
            elapsed: "4h",
            summary: "Postgres 15 to 17 migration runbook",
            ..A::default()
        }
        .row(),
        A {
            status: RowStatus::Dormant,
            chip: Some("FOOTER"),
            chip_ink: Some(0),
            repo: "resumaker",
            repo_ink: Some(7),
            model: Some("opus"),
            battery: Some((4, 73_000)),
            elapsed: "2w",
            summary: "ollie.gg company details footer",
            dormant: true,
            ..A::default()
        }
        .row(),
    ]
}

fn main() {
    let profiles = [
        ("COLLAPSED — the 38-column card", 38, Widths::COLLAPSED),
        (
            "EXPANDED — +10: branch beside repo, wider summary",
            48,
            Widths::EXPANDED,
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
            rows.len() * RowHeight::Double.lines_per_row(),
            widths,
            &Theme::default(),
            RowHeight::Double,
        );
        for (i, line) in lines.iter().enumerate() {
            let w = display_cells(&strip_sgr(line));
            assert_eq!(w, cols, "{name}: line {i} rendered {w} cells, want {cols}");
            println!("{line}");
        }
    }
    println!();
}
