//! The two-line card (#232) — the double-height row's geometry.
//!
//! A PORT, not a redesign. Every cell budget, ink and glyph below comes from
//! `render_f_pair` in `examples/double-preview.rs`, ratified 2026-08-26 ("that's
//! the one"); that file is the picture this module reproduces, and the goldens
//! at the bottom pin its exact output. Two profiles, one function: the
//! COLLAPSED card is 38 columns, the EXPANDED one 48 and the branch cell exists
//! only in the second.
//!
//! ```text
//!   line 1:  status ╭ chip-pill  summary            tokens
//!   line 2:  prov   ╰ repo [branch]  #PR  provider model ef  elapsed
//! ```
//!
//! Glass discipline (lock §6, and Ollie's terminal probe): a painted cell
//! background is OPAQUE, so translucency exists only where nothing is painted.
//! Unselected cards paint NO background and re-assert `\u{1b}[49m` on every
//! segment; the selected card is a full opaque bar across both lines.
//!
//! GLYPH RULE — every glyph is a `\u{...}` escape, never a literal (lock §5.4).

use crate::render::{Row, RowContent, cell_slice, clip_to_cells, display_cells, hue, strip_sgr};
use crate::theme::{
    BATTERY, BRACKET_A, BRACKET_B, CARD_BOT, CARD_TOP, CLAUDE_GLYPH, CLAUDE_INK, CONSOLE,
    DORMANT_FADE, ELLIPSIS, FADE, LCAP, META_INK, OPENAI_GLYPH, OPENAI_INK, PR_INK, RCAP, RESET,
    Rgb, TERM_MARK, Theme,
};

// ── the budgets (the example's fixed cells) ─────────────────────────────────

/// The pill's label.
const CHIP_W: usize = 7;
/// The repo name. Nine plus a leading space, so the repo text starts one cell
/// in — vertically aligned with the pill LABEL on line 1 (both at column 7).
const REPO_W: usize = 9;
/// The model handle.
const MODEL_W: usize = 6;
/// The effort tag (`xh`), beside the model. Its column came from the second
/// of round 8c's two spaces before the provider glyph (2026-09-01): line 2
/// had exactly three free cells between model and elapsed, and a tag with a
/// gap on each side needs four.
const EFFORT_W: usize = 2;
/// The branch's guaranteed MINIMUM in the expanded profile: repo and branch
/// share one `REPO_W + 1 + BRANCH_MIN` budget and the branch takes what the
/// repo name leaves, never less than this — branch names run longer than repo
/// names, so a long repo truncates first (round 9b).
const BRANCH_MIN: usize = 9;
const PR_W: usize = 5;
const TOKEN_W: usize = 4;
const ELAPSED_W: usize = 3;

/// The card's own floor and its expanded threshold, taken from the mode that
/// asks zellij for those widths rather than restated here — the geometry the
/// pane gets and the geometry the card draws cannot disagree.
const COLLAPSED_COLS: usize = clave_types::RowHeight::Double.target_cols(true);
const EXPANDED_COLS: usize = clave_types::RowHeight::Double.target_cols(false);

/// The provider's brand cell. An unrecognised provider renders NOTHING — same
/// "blank is the meaning" rule the main-checkout provenance follows: the bar
/// never invents a mark for a runtime it does not know (#232).
fn provider_mark(p: &str) -> Option<(char, Rgb)> {
    match p {
        "claude" => Some((CLAUDE_GLYPH, CLAUDE_INK)),
        "openai" => Some((OPENAI_GLYPH, OPENAI_INK)),
        _ => None,
    }
}

/// What line 1's rightmost cell says. Three states, not two: a terminal row
/// wears `TERM` where an agent shows its count, and an agent with no reading
/// yet blanks the cell rather than inventing a measurement.
enum TokenCell {
    Count(String, Rgb),
    Term,
    Blank,
}

/// One row's cells, resolved from either content variant so the geometry below
/// is written once. The two variants differ in WHERE each cell comes from, not
/// in what the card draws.
struct Cells<'a> {
    mark: (char, Rgb),
    /// Label plus pill background: `Some(hue)` is a title chip, `None` the
    /// TERM pill (theme black), and no pill at all when a session was never
    /// renamed — the summary claims those columns instead (round 8).
    chip: Option<(&'a str, Option<Rgb>)>,
    summary: &'a str,
    tokens: TokenCell,
    prov: Option<char>,
    repo: &'a str,
    /// One ink for the provenance glyph AND the repo name — provenance carries
    /// the repo's identity (round 6's O).
    repo_ink: Rgb,
    branch: &'a str,
    pr: Option<u32>,
    provider: Option<(char, Rgb)>,
    model: &'a str,
    /// The two-letter effort tag; empty when the host has no reading.
    effort: &'a str,
    elapsed: &'a str,
}

fn cells<'a>(content: &'a RowContent, theme: &Theme) -> Cells<'a> {
    match content {
        RowContent::Agent {
            status,
            battery,
            tokens,
            provenance,
            title,
            title_ink,
            repo,
            repo_ink,
            summary,
            model,
            provider,
            effort,
            pr,
            branch,
            elapsed,
        } => {
            // CLAMPED like `render_row`'s: the wire crosses a version boundary
            // and a newer host's longer ramp must read "at least this bad"
            // rather than blank.
            let band = battery
                .map(|i| usize::from(i).min(BATTERY.len() - 1))
                .map_or(theme.default_ink, |i| BATTERY[i].1);
            Cells {
                mark: status.mark(theme),
                chip: title.as_deref().map(|t| (t, Some(hue(*title_ink, theme)))),
                summary,
                // The INK is the ramp's coarse risk band, the TEXT the exact
                // magnitude — the same two axes the single-line battery cell
                // carries (#105), through the same formatter.
                tokens: tokens.map_or(TokenCell::Blank, |t| {
                    TokenCell::Count(crate::render::token_text(t), band)
                }),
                prov: provenance.mark(),
                repo,
                repo_ink: hue(*repo_ink, theme),
                branch,
                pr: *pr,
                provider: provider.as_deref().and_then(provider_mark),
                model: model.as_deref().unwrap_or(""),
                effort: effort.as_deref().unwrap_or(""),
                elapsed: elapsed.as_deref().unwrap_or(""),
            }
        }
        RowContent::Terminal {
            name,
            status,
            provenance,
            repo,
            repo_ink,
            command,
            pr,
            branch,
            elapsed,
        } => Cells {
            // The glyph is the row's KIND, the colour its state — the console
            // mark holds the status cell on a terminal row (#206).
            mark: (CONSOLE, status.ink(theme)),
            chip: Some((name, None)),
            summary: command,
            tokens: TokenCell::Term,
            prov: provenance.mark(),
            repo: repo.as_deref().unwrap_or(""),
            // An UNMATCHED cwd has no allocation, and untinted read as
            // disabled — nearly invisible on the selected row (Ollie, live
            // 2026-08-18) — so the ink-less repo falls back to the default ink
            // rather than the palette's absent-hue grey.
            repo_ink: repo_ink.map_or(theme.default_ink, |i| hue(Some(i), theme)),
            branch,
            pr: *pr,
            provider: None,
            model: "",
            effort: "",
            elapsed: elapsed.as_deref().unwrap_or(""),
        },
    }
}

/// One row as two lines, each EXACTLY `cols` display cells.
///
/// `zebra_paint` is the card's parity in the viewport slice — the zebra lives
/// in the linework (round 6's O), alternating the bracket between two neutral
/// inks rather than painting every second background, which glass forbids.
/// `pub(crate)` since #232 task 9 gave it its caller: `render_rows` is the
/// bar's ONE entry point (LEDGER D5) and the card is a geometry inside it, not
/// a second public renderer for a caller to reach past the dispatch and pick.
pub(crate) fn render_card(
    row: &Row,
    cols: usize,
    any_selected: bool,
    zebra_paint: bool,
    theme: &Theme,
) -> (String, String) {
    // Built at the design floor even when the pane is narrower, then clipped:
    // the fixed cells cannot reflow, so a sub-floor card over-runs UNIFORMLY
    // and loses the same trailing cells on every row (LEDGER D13), rather than
    // going ragged or — worse — wrapping into a third line.
    let build = cols.max(COLLAPSED_COLS);
    let branch_w = if build >= EXPANDED_COLS {
        BRANCH_MIN
    } else {
        0
    };
    let c = cells(&row.content, theme);

    // `render_row`'s ladder, unchanged: nothing to recede FROM when nothing is
    // selected, and the dormant fade is ABSOLUTE rather than relative (#206).
    // `Opening` escapes it — that row was just launched, mid-transition to
    // live (#100).
    let dormant = row.dormant
        && !matches!(
            row.content,
            RowContent::Agent {
                status: crate::render::RowStatus::Opening,
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
    let ink = |c: Rgb| c.mix(theme.base, fade);
    // Glass: `None` paints NOTHING and re-asserts the default background, so
    // the selection bar never bleeds into the glass after it.
    let row_bg: Option<Rgb> = row.selected.then_some(theme.sel_bg);
    let seg = |c: Rgb, s: &str| match row_bg {
        Some(b) => format!("{}{}{s}", b.bg(), c.fg()),
        None => format!("\u{1b}[49m{}{s}", c.fg()),
    };
    let bracket_ink = ink(if zebra_paint { BRACKET_B } else { BRACKET_A });

    // ── line 1: status ╭ chip-pill summary tokens ──
    let mut l1 = String::new();
    let (mark, mark_ink) = c.mark;
    l1.push_str(&seg(ink(mark_ink), &format!(" {mark} ")));
    l1.push_str(&seg(bracket_ink, &format!("{CARD_TOP} ")));
    match &c.chip {
        Some((label, Some(bg))) => {
            let chip_bg = ink(*bg);
            l1.push_str(&seg(chip_bg, &LCAP.to_string()));
            l1.push_str(&format!(
                "{}{}{}{RESET}",
                chip_bg.bg(),
                ink(theme.chip_ink).fg(),
                pad(label, CHIP_W)
            ));
            l1.push_str(&seg(chip_bg, &RCAP.to_string()));
        }
        // The TERM pill: the title chip's shape with theme black instead of a
        // palette colour — a block no agent ink has claimed (round 8).
        Some((label, None)) => {
            l1.push_str(&seg(ink(theme.chip_ink), &LCAP.to_string()));
            l1.push_str(&format!(
                "{}{}{}{RESET}",
                theme.chip_ink.bg(),
                ink(theme.default_ink).fg(),
                pad(label, CHIP_W)
            ));
            l1.push_str(&seg(ink(theme.chip_ink), &RCAP.to_string()));
        }
        None => {}
    }
    // The summary is the only flexing cell. With a pill: 3 mark + 2 bracket +
    // 9 pill + 1 space + 5 token cell + 1 margin = 21 fixed. Without one, the
    // text starts at the bracket and claims the pill's nine columns too.
    let flex = if c.chip.is_some() {
        build.saturating_sub(21)
    } else {
        build.saturating_sub(12)
    };
    l1.push_str(&seg(
        ink(theme.default_ink),
        &format!(" {}", pad(c.summary, flex)),
    ));
    match &c.tokens {
        TokenCell::Count(text, band) => {
            l1.push_str(&seg(ink(*band), &format!(" {}", rpad(text, TOKEN_W))))
        }
        TokenCell::Term => l1.push_str(&seg(
            ink(META_INK),
            &format!(" {}", rpad(TERM_MARK, TOKEN_W)),
        )),
        TokenCell::Blank => l1.push_str(&seg(
            ink(theme.default_ink),
            &format!(" {}", rpad("", TOKEN_W)),
        )),
    }
    l1.push_str(&seg(theme.default_ink, " "));

    // ── line 2: prov ╰ repo [branch] #PR provider model effort … elapsed ──
    let mut l2 = String::new();
    let prov = c.prov.map_or_else(|| " ".to_string(), String::from);
    l2.push_str(&seg(ink(c.repo_ink), &format!(" {prov} ")));
    l2.push_str(&seg(bracket_ink, &format!("{CARD_BOT} ")));
    // Round 9b, amended by the 2026-08-27 drive: repo and branch share ONE
    // budget, and a card with no PR folds the PR cell's columns into it —
    // blank was the meaning, but six dead cells beside a truncated name were
    // waste. The PR column never moves WHERE A PR EXISTS; only absent PRs
    // yield their columns. In the collapsed profile the reclaimed columns are
    // what lets a branch render at all.
    let pr_extra = if c.pr.is_none() { PR_W + 1 } else { 0 };
    let total = REPO_W + pr_extra + if branch_w > 0 { 1 + branch_w } else { 0 };
    if c.branch.is_empty() || total == REPO_W {
        l2.push_str(&seg(ink(c.repo_ink), &format!(" {}", pad(c.repo, total))));
    } else {
        // The branch starts one space after the repo NAME (not its padded
        // cell) and claims every column the repo does not use, in meta ink so
        // the repo's palette ink keeps carrying the identity.
        let repo_w = display_cells(c.repo).min(REPO_W);
        l2.push_str(&seg(ink(c.repo_ink), &format!(" {}", pad(c.repo, repo_w))));
        l2.push_str(&seg(
            ink(META_INK),
            &format!(" {}", pad(c.branch, total - repo_w - 1)),
        ));
    }
    if let Some(n) = c.pr {
        l2.push_str(&seg(
            ink(PR_INK),
            &format!(" {}", pad(&format!("#{n}"), PR_W)),
        ));
    }
    match c.provider {
        // One space before the icon: round 8c gave it two, and the effort
        // cell took the second (see `EFFORT_W`). The PR cell's own trailing
        // pad keeps two visual spaces there for any PR under five digits.
        Some((glyph, brand)) => l2.push_str(&seg(ink(brand), &format!(" {glyph}"))),
        None => l2.push_str(&seg(theme.default_ink, "  ")),
    }
    l2.push_str(&seg(ink(META_INK), &format!(" {}", pad(c.model, MODEL_W))));
    l2.push_str(&seg(
        ink(META_INK),
        &format!(" {}", pad(c.effort, EFFORT_W)),
    ));
    let fill = build.saturating_sub(display_cells(&strip_sgr(&l2)) + ELAPSED_W + 1);
    l2.push_str(&seg(theme.default_ink, &" ".repeat(fill)));
    l2.push_str(&seg(ink(META_INK), &rpad(c.elapsed, ELAPSED_W)));
    l2.push_str(&seg(theme.default_ink, " "));

    // ONE exit for both lines: the clip closes the SGR state and guarantees
    // exactly `cols` cells. Above the floor it is a no-op beyond the `RESET`
    // it appends — and below it, it is the thing that stops a card the pane
    // cannot hold from WRAPPING into a third line.
    (clip_to_cells(&l1, cols), clip_to_cells(&l2, cols))
}

/// A fixed-width cell, measured in CELLS: truncate with an ellipsis when long,
/// pad when short.
///
/// Control characters are replaced with a space, exactly as `render_row`'s
/// clamp does: summaries and tab names are agent-authored, a `\n` measures as
/// ZERO cells, and one of those would sail through the every-line-is-`cols`
/// invariant while breaking the card on screen. A space keeps the word
/// boundary that dropping destroys.
fn pad(s: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    let s = printable(s);
    let cells = display_cells(&s);
    if cells <= w {
        return format!("{s}{}", " ".repeat(w - cells));
    }
    let mut out = cell_slice(&s, 0, w - 1);
    out.push(ELLIPSIS);
    // A WIDE glyph straddling the cut is excluded whole rather than
    // half-drawn, which can leave the cell one column short — pad it back.
    out.push_str(&" ".repeat(w.saturating_sub(display_cells(&out))));
    out
}

/// Right-aligned in `w` cells — the edge the eye compares magnitudes on
/// (#105), which is why the token count and the elapsed time align to it.
fn rpad(s: &str, w: usize) -> String {
    let s = printable(s);
    let cells = display_cells(&s);
    if cells >= w {
        pad(&s, w)
    } else {
        format!("{}{s}", " ".repeat(w - cells))
    }
}

fn printable(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Provenance, RowStatus, TermStatus};

    /// The mock the ratified preview renders, as real rows. Indices 0–15 are
    /// `double-preview.rs`'s sixteen in order, so a golden here can be read
    /// straight off that file's output; 16 and 17 add the two corners the mock
    /// had no field for (an unknown provider, and the `Opening` row that
    /// escapes the dormant fade).
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
        tokens: Option<u32>,
        battery: Option<u8>,
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
                tokens: Some(100_000),
                battery: Some(5),
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
                    battery: self.battery,
                    tokens: self.tokens,
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
                tokens: Some(105_000),
                battery: Some(6),
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
                tokens: Some(78_000),
                battery: Some(4),
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
                tokens: Some(234_000),
                battery: Some(10),
                elapsed: "1h",
                summary: "XPS dev server setup and deploy",
                ..A::default()
            }
            .row(),
            A {
                status: NeedsYou,
                chip: Some("REASSOC"),
                chip_ink: Some(3),
                tokens: Some(86_000),
                battery: Some(5),
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
                tokens: Some(117_000),
                battery: Some(7),
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
                tokens: Some(79_000),
                battery: Some(4),
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
                tokens: Some(130_000),
                battery: Some(8),
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
                tokens: Some(78_000),
                battery: Some(4),
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
                tokens: Some(34_000),
                battery: Some(2),
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
                tokens: Some(119_000),
                battery: Some(7),
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
            // Terminal in a worktree, repo name filling the 9-cell budget
            // exactly.
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
            // Chipless + branch + PR + OpenAI, repo name OVERFLOWING.
            A {
                prov: Branch,
                repo: "clave-website",
                repo_ink: Some(6),
                branch: "hero-copy",
                pr: Some(12),
                provider: Some("openai"),
                model: Some("gpt-5"),
                effort: None,
                tokens: Some(55_000),
                battery: Some(3),
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
                tokens: Some(201_000),
                battery: Some(10),
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
                tokens: Some(73_000),
                battery: Some(4),
                elapsed: "2w",
                summary: "ollie.gg company details footer",
                dormant: true,
                ..A::default()
            }
            .row(),
            // ── beyond the mock: the fields it had no shape for ──
            // An unknown provider, no model, no reading yet, no elapsed.
            A {
                chip: Some("GROK"),
                chip_ink: Some(1),
                provider: Some("grok"),
                model: None,
                effort: None,
                tokens: None,
                battery: None,
                elapsed: "",
                summary: "Provider clave has never heard of",
                ..A::default()
            }
            .row(),
            // Opening — dormant-flagged, but mid-launch and so at full
            // strength.
            A {
                status: RowStatus::Opening,
                chip: Some("OPENING"),
                chip_ink: Some(2),
                provider: None,
                effort: None,
                summary: "Just launched",
                dormant: true,
                ..A::default()
            }
            .row(),
        ]
    }

    /// Both lines of a card, SGR stripped — the picture, measured.
    fn pin(row: &Row, cols: usize) -> (String, String) {
        let (l1, l2) = render_card(row, cols, true, false, &Theme::default());
        (strip_sgr(&l1), strip_sgr(&l2))
    }

    #[test]
    fn every_card_line_is_exactly_cols_wide_in_both_profiles() {
        for cols in [38, 48] {
            for (i, row) in fleet().iter().enumerate() {
                for zebra in [false, true] {
                    for any in [false, true] {
                        let (l1, l2) = render_card(row, cols, any, zebra, &Theme::default());
                        for l in [&l1, &l2] {
                            assert_eq!(display_cells(&strip_sgr(l)), cols, "row {i} at {cols}");
                        }
                    }
                }
            }
        }
    }

    /// Off-profile widths too: the pane passes through every column between
    /// the two targets while zellij animates a resize, and below the floor the
    /// card is clipped rather than left to WRAP into a third line (D13).
    #[test]
    fn a_card_holds_its_width_at_every_pane_width() {
        for cols in 1..=80 {
            for (i, row) in fleet().iter().enumerate() {
                let (l1, l2) = render_card(row, cols, true, i % 2 == 1, &Theme::default());
                for l in [&l1, &l2] {
                    assert_eq!(display_cells(&strip_sgr(l)), cols, "row {i} at {cols}");
                }
            }
        }
    }

    /// The picture-pin: the selected card, cell for cell, in both profiles —
    /// copied from the ratified preview's output for its matching mock row.
    #[test]
    fn the_selected_card_pins_its_cells() {
        let f = fleet();
        let sel = &f[7];
        assert_eq!(
            pin(sel, 38),
            (
                " \u{25cf} \u{256d} \u{e0b6}CLV-M2 \u{e0b4} Goal is shipping\u{2026} 130k "
                    .to_string(),
                " \u{168c2} \u{2570}  clave     #225  \u{ec82} fable  xh  5m ".to_string(),
            )
        );
        assert_eq!(
            pin(sel, 48),
            (
                " \u{25cf} \u{256d} \u{e0b6}CLV-M2 \u{e0b4} Goal is shipping v0.2.2 cl\u{2026} 130k "
                    .to_string(),
                " \u{168c2} \u{2570}  clave v022-prep     #225  \u{ec82} fable  xh  5m ".to_string(),
            )
        );
    }

    /// The rest of the ratified picture, at the corners of the variant space:
    /// the branch card, the TERM pill, the chipless flex, the exactly-9 repo,
    /// the overflowing repo, and the dormant row.
    #[test]
    fn the_ratified_fleet_pins_its_cards() {
        let f = fleet();
        let want: [(usize, &str, &str, &str, &str); 7] = [
            (
                4,
                " \u{25cf} \u{256d} \u{e0b6}CLV-3  \u{e0b4} Drive launch      117k ",
                " \u{168c2} \u{2570}  clave     #204  \u{ec82} sonnet hi 45m ",
                " \u{25cf} \u{256d} \u{e0b6}CLV-3  \u{e0b4} Drive launch                117k ",
                " \u{168c2} \u{2570}  clave drive-launch  #204  \u{ec82} sonnet hi 45m ",
            ),
            (
                6,
                " \u{f018d} \u{256d} \u{e0b6}Tab #12\u{e0b4} zsh               TERM ",
                "   \u{2570}  clave                        7m ",
                " \u{f018d} \u{256d} \u{e0b6}Tab #12\u{e0b4} zsh                         TERM ",
                "   \u{2570}  clave                                  7m ",
            ),
            (
                9,
                " \u{25cf} \u{256d}  Create close conversation\u{2026}  34k ",
                "   \u{2570}  hermes          \u{ec82} opus   hi  2h ",
                " \u{25cf} \u{256d}  Create close conversation summary f\u{2026}  34k ",
                "   \u{2570}  hermes                    \u{ec82} opus   hi  2h ",
            ),
            (
                10,
                " \u{25cf} \u{256d} \u{e0b6}GTMSS  \u{e0b4} GTM Landscape - \u{2026} 119k ",
                " \u{f062c} \u{2570}  nalu      #31   \u{ec82} haiku  hi  1d ",
                " \u{25cf} \u{256d} \u{e0b6}GTMSS  \u{e0b4} GTM Landscape - and first \u{2026} 119k ",
                " \u{f062c} \u{2570}  nalu gtm-pass       #31   \u{ec82} haiku  hi  1d ",
            ),
            (
                13,
                " \u{25cf} \u{256d}  Landing page hero copy re\u{2026}  55k ",
                " \u{f062c} \u{2570}  clave-we\u{2026} #12   \u{ec81} gpt-5     30m ",
                " \u{25cf} \u{256d}  Landing page hero copy rewrite pass   55k ",
                " \u{f062c} \u{2570}  clave-we\u{2026} hero-copy #12   \u{ec81} gpt-5     30m ",
            ),
            (
                14,
                " \u{2716} \u{256d} \u{e0b6}MIGRATE\u{e0b4} Postgres 15 to 1\u{2026} 201k ",
                " \u{168c2} \u{2570}  market-s\u{2026} #88   \u{ec82} sonnet hi  4h ",
                " \u{2716} \u{256d} \u{e0b6}MIGRATE\u{e0b4} Postgres 15 to 17 migratio\u{2026} 201k ",
                " \u{168c2} \u{2570}  market-s\u{2026} pg17-mig\u{2026} #88   \u{ec82} sonnet hi  4h ",
            ),
            (
                15,
                " \u{25cb} \u{256d} \u{e0b6}FOOTER \u{e0b4} ollie.gg company\u{2026}  73k ",
                "   \u{2570}  resumaker       \u{ec82} opus   hi  2w ",
                " \u{25cb} \u{256d} \u{e0b6}FOOTER \u{e0b4} ollie.gg company details f\u{2026}  73k ",
                "   \u{2570}  resumaker                 \u{ec82} opus   hi  2w ",
            ),
        ];
        for (i, c1, c2, e1, e2) in want {
            assert_eq!(pin(&f[i], 38), (c1.to_string(), c2.to_string()), "row {i}");
            assert_eq!(pin(&f[i], 48), (e1.to_string(), e2.to_string()), "row {i}");
        }
    }

    /// A terminal card's branch is borrowed the same way its `pr` is (#232):
    /// the expanded profile shares the repo/branch budget exactly as an agent
    /// card does. Row 12 (`resumaker`, `resume-fix`) is the boundary corner —
    /// a 9-cell repo name PLUS a branch, which is `repo_w == 9` against the
    /// collective budget (the ratified preview's Tab #7 shape).
    #[test]
    fn a_terminal_card_shares_its_borrowed_branch_across_the_collective_budget() {
        let f = fleet();
        assert_eq!(
            pin(&f[11], 48),
            (
                " \u{f018d} \u{256d} \u{e0b6}Tab #3 \u{e0b4} gh pr checks --watch        TERM "
                    .to_string(),
                " \u{f062c} \u{2570}  clave double-rows   #232               3m ".to_string(),
            )
        );
        // No PR on this tab, so the PR cell's columns flow into the shared
        // budget (2026-08-27 drive): collapsed gains a truncated branch,
        // expanded shows `resume-fix` whole.
        assert_eq!(
            pin(&f[12], 38).1,
            " \u{168c2} \u{2570}  resumaker resu\u{2026}              1h "
        );
        assert_eq!(
            pin(&f[12], 48).1,
            " \u{168c2} \u{2570}  resumaker resume-fix                   1h "
        );
    }

    #[test]
    fn glass_rows_reassert_default_bg_and_selection_paints_sel_bg() {
        let theme = Theme::default();
        let f = fleet();
        let (l1, l2) = render_card(&f[0], 38, true, false, &theme);
        for l in [&l1, &l2] {
            assert!(
                l.contains("\u{1b}[49m"),
                "unselected card must re-open glass"
            );
            assert!(
                !l.contains(&theme.sel_bg.bg()),
                "unselected card must paint no selection"
            );
        }
        let (s1, s2) = render_card(&f[7], 38, true, false, &theme);
        for l in [&s1, &s2] {
            assert!(
                l.contains(&theme.sel_bg.bg()),
                "the selection is a full bar"
            );
            assert!(
                !l.contains("\u{1b}[49m"),
                "the selection bar never opens glass mid-row"
            );
        }
    }

    #[test]
    fn collapsed_renders_no_branch_and_expanded_shares_the_collective_budget() {
        let f = fleet();
        // repo "clave", branch "drive-launch", PR #204: the PR holds its
        // columns, so the branch is absent at 38 and in FULL one space after
        // the repo NAME at 48.
        assert!(!pin(&f[4], 38).1.contains("drive-launch"));
        assert!(pin(&f[4], 48).1.contains(" clave drive-launch "));
        // A 14-cell repo truncates to nine with an ellipsis, and its branch
        // still gets its nine.
        let l2 = pin(&f[14], 48).1;
        assert!(l2.contains("market-s\u{2026} pg17-mig\u{2026}"), "{l2}");
    }

    /// A card with NO PR folds the PR cell's six columns into the repo/branch
    /// budget (2026-08-27 drive): the COLOUR worktree row gains its branch even
    /// in the collapsed profile, and a PR-less row with no branch gives the
    /// whole widened budget to a long repo name. The PR column itself never
    /// moves where a PR exists — f[4] above is that half of the invariant.
    #[test]
    fn a_card_without_a_pr_flows_its_columns_into_repo_and_branch() {
        let f = fleet();
        assert!(
            pin(&f[5], 38).1.contains(" clave colour "),
            "{}",
            pin(&f[5], 38).1
        );
        assert!(
            pin(&f[5], 48).1.contains(" clave colour "),
            "{}",
            pin(&f[5], 48).1
        );
        // Same row with a PR: collapsed hides the branch again.
        let row = A {
            prov: Provenance::Worktree,
            branch: "colour",
            pr: Some(9),
            ..A::default()
        }
        .row();
        let (_, l2) = render_card(&row, 38, true, false, &Theme::default());
        assert!(!strip_sgr(&l2).contains("colour"), "{}", strip_sgr(&l2));
    }

    /// The token cell wears the RAMP's band, not an approximation of it: the
    /// ink is `BATTERY`'s, indexed by the row's context level, and it saturates
    /// rather than blanking when a newer host sends a longer ramp.
    #[test]
    fn the_token_cell_wears_the_real_ramp_band() {
        let theme = Theme::default();
        let band = |battery| {
            let row = A {
                battery,
                tokens: Some(130_000),
                ..A::default()
            }
            .row();
            render_card(&row, 38, false, false, &theme).0
        };
        assert!(band(Some(0)).contains(&BATTERY[0].1.fg()));
        assert!(band(Some(10)).contains(&BATTERY[10].1.fg()));
        // Out of range: the last band, never a blank cell.
        assert!(band(Some(200)).contains(&BATTERY[BATTERY.len() - 1].1.fg()));
        // No reading yet: the default ink carries the count it does have.
        assert!(band(None).contains(&theme.default_ink.fg()));
    }

    /// An agent with no token count blanks the cell — the bar never invents a
    /// measurement, and `TERM` belongs to terminal rows alone.
    #[test]
    fn an_agent_without_a_reading_blanks_its_token_cell() {
        let f = fleet();
        let l1 = pin(&f[16], 38).0;
        assert!(!l1.contains("TERM"), "{l1}");
        assert!(l1.ends_with("     "), "{l1}");
    }

    #[test]
    fn only_a_known_provider_marks_its_cell() {
        let f = fleet();
        assert!(pin(&f[7], 38).1.contains('\u{ec82}'), "claude");
        assert!(pin(&f[13], 38).1.contains('\u{ec81}'), "openai");
        for i in [16, 17] {
            let l2 = pin(&f[i], 38).1;
            assert!(!l2.contains('\u{ec82}') && !l2.contains('\u{ec81}'), "{l2}");
        }
    }

    /// Recession is RELATIVE (lock §6) and the dormant fade ABSOLUTE (#206) —
    /// and `Opening` escapes it, mid-launch.
    #[test]
    fn the_fade_ladder_matches_the_single_line_row() {
        let theme = Theme::default();
        let f = fleet();
        let ink = |c: Rgb, f: f64| c.mix(theme.base, f).fg();
        // Nothing selected: no recession at all.
        let (l1, _) = render_card(&f[0], 38, false, false, &theme);
        assert!(l1.contains(&ink(theme.default_ink, 0.0)));
        // Something selected: an unselected card recedes — line 1's summary
        // ink and line 2's metadata ink both.
        let (l1, l2) = render_card(&f[0], 38, true, false, &theme);
        assert!(l1.contains(&ink(theme.default_ink, FADE)));
        assert!(l2.contains(&ink(META_INK, FADE)));
        // Dormant: both lines, whether or not anything is selected.
        for any in [false, true] {
            let (l1, l2) = render_card(&f[15], 38, any, false, &theme);
            for l in [&l1, &l2] {
                assert!(
                    l.contains(&ink(META_INK, DORMANT_FADE)),
                    "dormant, any={any}"
                );
            }
        }
        // Opening: flagged dormant, rendered live.
        let (l1, _) = render_card(&f[17], 38, false, false, &theme);
        assert!(!l1.contains(&ink(META_INK, DORMANT_FADE)));
    }

    /// The zebra lives in the LINEWORK: alternating bracket inks, never a
    /// second background — glass has no second opacity to give.
    #[test]
    fn the_zebra_alternates_the_bracket_ink() {
        let theme = Theme::default();
        let f = fleet();
        let (a1, a2) = render_card(&f[0], 38, false, false, &theme);
        let (b1, b2) = render_card(&f[0], 38, false, true, &theme);
        assert!(a1.contains(&BRACKET_A.fg()) && a2.contains(&BRACKET_A.fg()));
        assert!(b1.contains(&BRACKET_B.fg()) && b2.contains(&BRACKET_B.fg()));
        assert_eq!(strip_sgr(&a1), strip_sgr(&b1), "the zebra costs no cells");
    }

    /// Agent-authored text reaches these cells raw. A control character
    /// measures as zero cells and would silently break the card; a wide glyph
    /// straddling a truncation is dropped whole and must not leave the cell
    /// short.
    #[test]
    fn hostile_text_cannot_break_a_card() {
        for summary in [
            "line one\nline two\r\u{1b}",
            "\u{65e5}\u{672c}\u{8a9e}\u{3067}\u{3059}, a wide summary that runs on",
            "\u{1f600}\u{1f600}\u{1f600} emoji",
        ] {
            for cols in [38, 48] {
                let row = A {
                    summary,
                    repo: "\u{65e5}\u{672c}\u{8a9e}\u{3067}\u{3059}\u{65e5}",
                    branch: "\u{65e5}\u{672c}\u{8a9e}\u{3067}\u{3059}\u{65e5}\u{672c}",
                    ..A::default()
                }
                .row();
                let (l1, l2) = render_card(&row, cols, true, false, &Theme::default());
                for l in [&l1, &l2] {
                    assert_eq!(display_cells(&strip_sgr(l)), cols, "{summary:?} at {cols}");
                }
                assert!(!strip_sgr(&l1).contains('\n'));
            }
        }
    }
}
