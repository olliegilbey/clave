//! The card geometries — two of them, one per row-height mode.
//!
//! Both are PORTS, not redesigns, and both were ratified from rendered output
//! rather than from prose. Each function below reproduces a picture that was
//! signed off, and the goldens at the bottom pin its exact output.
//!
//! **`render_card` — the four-line card, the `card` mode and the default.**
//! Ratified 2026-09-08 by the lock in `docs/superpowers/specs/`; that document
//! is authoritative for every number here. Three lines of content plus a
//! hairline that closes the card, 48 columns expanded and 16 collapsed.
//!
//! ```text
//!   line 1:  status  │ chip-pill  summary
//!   line 2:  prov    │ repo [branch]  #PR   provider model effort
//!   line 3:  subs    │ tokens  elapsed  wants               turn
//!   line 4:  the shadow rule — a hairline that closes the card
//! ```
//!
//! **`render_double_card` — the two-line card, the `double` mode.** Ratified
//! 2026-08-26 from `examples/double-preview.rs`, which is still the picture it
//! reproduces. 48 columns expanded, 38 collapsed, branch in the second only.
//!
//! ```text
//!   line 1:  status ╭ chip-pill  summary            tokens
//!   line 2:  prov   ╰ repo [branch]  #PR  provider model ef  elapsed
//! ```
//!
//! Glass discipline (lock §6, and Ollie's terminal probe): a painted cell
//! background is OPAQUE, so translucency exists only where nothing is painted.
//! Unselected cards paint NO background and re-assert `\u{1b}[49m` on every
//! segment; the selected card is a full opaque bar across its content lines.
//!
//! GLYPH RULE — every glyph is a `\u{...}` escape, never a literal (lock §5.4).
//! And every one is checked against the INSTALLED font's character map, not a
//! cheat sheet — two candidates for the four-line card's marks turned out not
//! to exist at the codepoints published for them (FOOTGUNS, 2026-09-08).

use crate::render::{Row, RowContent, cell_slice, clip_to_cells, display_cells, hue, strip_sgr};
use crate::theme::{
    BATTERY, BRACKET_A, BRACKET_B, BRANCH_INK, CARD_BOT, CARD_TOP, CLAUDE_GLYPH, CLAUDE_INK,
    CONSOLE, DORMANT_FADE, ELLIPSIS, FADE, LCAP, META_INK, NEEDS_YOU_INK, OPENAI_GLYPH, OPENAI_INK,
    PR_INK, RCAP, RESET, RULE, Rgb, SUBS_INK, SUBS_MARK, TERM_MARK, TURN_INK, Theme, WORKTREE_INK,
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

// ── the four-line card's own budgets (lock §2, §3, §5) ──────────────────────

/// Its floor and threshold, from its own mode for the same reason as above.
const CARD_COLLAPSED_COLS: usize = clave_types::RowHeight::Card.target_cols(true);
const CARD_EXPANDED_COLS: usize = clave_types::RowHeight::Card.target_cols(false);

/// The card's left chrome: margin, mark, air, rail, air. A four-cell version
/// that butted the rail against the mark column was rejected on sight — the
/// rule sat on top of the glyphs. The fifth column costs nothing that matters:
/// the collapsed profile still lands exactly on 16.
const CHROME: usize = 5;

/// Where lines 2 and 3 start their content — flush with the chrome, so the
/// repo name and the token count sit under the pill's left CAP. Indenting one
/// further, to the pill's LABEL, was tried and rejected by eye: the pill is a
/// solid block of colour, so the edge the column reads against is its outer
/// edge, not the first letter inside it.
const INDENT: usize = CHROME;

/// The gap between the token count and the time beside it. TWO cells, not one:
/// at one they read as a single figure, the same failure that keeps the two
/// clocks apart.
const TOKEN_GAP: usize = 2;

/// The turn clock — the card's only live number.
const TURN_W: usize = 3;

/// The tail line 2 pays for provider, model and effort in the expanded
/// profile, and hands entirely to the repo in the collapsed one.
const META_TAIL_W: usize = 12;

/// Line 4's hairline and its ink — a rule so dark it reads as a shadow under
/// the card rather than as a border between two. Chosen by eye from a swatch
/// of four glyphs at three intensities, because how thin "thin" looks is a
/// property of the font and the panel, not of the codepoint.
const SHADOW: char = '\u{2500}'; // box drawings light horizontal
const SHADOW_INK: Rgb = Rgb(0x2A, 0x2A, 0x37); // sumiInk4

/// Claude Code's own spinner, in the card's status cell. ORDER IS THE
/// ANIMATION: the mark grows from a dot through progressively heavier stars
/// and back, so it reads as one shape breathing. Listing them in any other
/// sequence gives six glyphs flickering.
///
/// FONT WARNING, measured: most of these are absent from the Nerd Fonts
/// installed here and arrive by terminal FALLBACK out of Menlo, which carries
/// all six. Menlo is monospace so the cell width holds — but weight and
/// baseline do not show up in a golden, so this is the one part of the card
/// that can only be signed off on a real terminal.
const THINK_FRAMES: [char; 6] = [
    '\u{00b7}', // interpunct
    '\u{2722}', // four teardrop-spoked asterisk
    '\u{2733}', // eight-spoked asterisk
    '\u{2736}', // six pointed black star
    '\u{273b}', // teardrop-spoked asterisk
    '\u{273d}', // heavy teardrop-spoked asterisk
];

/// Ping-pong: the six frames out and the four inner ones back, so the cycle
/// turns over without a jump.
const THINK_CYCLE: usize = 10;

/// The spinner's glyph at animation frame `t`. `pub(crate)` so the shell can
/// decide whether any row is animating without a second copy of the cycle.
pub(crate) fn think_frame(t: usize) -> char {
    let i = t % THINK_CYCLE;
    THINK_FRAMES[if i < 6 { i } else { THINK_CYCLE - i }]
}

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

    // ── the four-line card's own cells. The two-line card ignores all four.
    /// Working right now, so the status mark animates instead of sitting
    /// still. Derived from the status the card already draws — no new wire
    /// field, and no way for the spinner and the mark's colour to disagree.
    thinking: bool,
    /// Whether this row has any subagent in flight, off the transcript's
    /// pending-background-agent count (lock 4.6). A terminal row never has
    /// one.
    subs: bool,
    /// How long the turn in flight has been going. NO SOURCE YET: the store
    /// will hold when the turn BEGAN and the bar subtracts from the wall clock
    /// it already reads once per render, so animating this costs no store
    /// traffic across the fleet. Blank when no turn is in flight.
    turn: &'a str,
    /// What this row is blocked on, in its own words (lock §4.7). Tier 1 is
    /// wired: the tool name off the permission notification clave's hook
    /// already receives. Blank is the common case and the point — an idle
    /// fleet has a quiet right-hand column, and the one row that wants you
    /// grows text.
    wants: &'a str,
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
            wants,
            subagents,
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
                thinking: status.thinking(),
                subs: *subagents,
                turn: "",
                wants: wants.as_deref().unwrap_or(""),
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
            // A terminal row has no turn, no subagents and nothing it is
            // blocked on — and a shell running a command is not "thinking" in
            // the sense the spinner means.
            thinking: false,
            subs: false,
            turn: "",
            wants: "",
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
pub(crate) fn render_double_card(
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

/// One row as FOUR lines, each EXACTLY `cols` display cells — the `card` mode
/// and the bar's default geometry.
///
/// `frame` is the animation tick driving the status spinner; a row that is not
/// working ignores it, so a still fleet renders identically at every frame and
/// the goldens below can pin frame 0 without freezing the animation out.
///
/// Three cells have no source on the wire yet — the subagent mark, the turn
/// clock and `wants` — and each renders BLANK until one is wired. Blank is the
/// meaning on this card, so that is a correct card rather than a placeholder,
/// and the three can land independently.
///
/// `pub(crate)` for the same reason as its two-line sibling: `render_rows` is
/// the bar's one entry point, and a card is a geometry inside it rather than a
/// second public renderer for a caller to reach past the dispatch and pick.
pub(crate) fn render_card(
    row: &Row,
    cols: usize,
    any_selected: bool,
    frame: usize,
    theme: &Theme,
) -> [String; 4] {
    // Built at the design floor even when the pane is narrower, then clipped —
    // the fixed cells cannot reflow, so a sub-floor card over-runs UNIFORMLY
    // and loses the same trailing cells on every row rather than going ragged
    // or, worse, wrapping into a fifth line.
    let build = cols.max(CARD_COLLAPSED_COLS);
    let expanded = build >= CARD_EXPANDED_COLS;
    let c = cells(&row.content, theme);

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
    let row_bg: Option<Rgb> = row.selected.then_some(theme.sel_bg);
    let seg = |c: Rgb, s: &str| match row_bg {
        Some(b) => format!("{}{}{s}", b.bg(), c.fg()),
        None => format!("\u{1b}[49m{}{s}", c.fg()),
    };
    // The rail IS the repo's colour, running the card's full height — one
    // continuous identity spine per card. It replaced an arc whose two neutrals
    // alternated card by card, which was linework doing a separator's job
    // because glass forbade a painted stripe. Line 4 does that job properly,
    // so the zebra retired with the arc and this function takes no parity.
    let rail = ink(c.repo_ink);

    // ── line 1: status │ chip-pill summary ──
    let mut l1 = String::new();
    let (mark, mark_ink) = c.mark;
    // A row blocked on you does NOT animate: the spinner stops when Claude
    // asks, which is what makes a stopped spinner mean something.
    let mark = if c.thinking { think_frame(frame) } else { mark };
    l1.push_str(&seg(ink(mark_ink), &format!(" {mark} ")));
    l1.push_str(&seg(rail, &format!("{RULE} ")));
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
        // The TERM pill: the title chip's shape in theme black.
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
    // A chipless card hands the pill's nine columns to the summary AND its
    // trailing gap, so the summary starts flush at the indent with the repo
    // and the token count below it. The gap is the pill's air, not an indent:
    // with no pill there is nothing to hold air away from, and that space read
    // as a one-cell stagger against every other line.
    let gap = if c.chip.is_some() { " " } else { "" };
    let summary_w = build.saturating_sub(if c.chip.is_some() {
        CHROME + 11
    } else {
        CHROME + 1
    });
    // A cell with room for an ellipsis and nothing else says less than a blank
    // one: at 16 columns the pill IS the line, and a lone `…` after it is noise.
    let summary = if summary_w < 4 { "" } else { c.summary };
    l1.push_str(&seg(
        ink(theme.default_ink),
        &format!("{gap}{}", pad(summary, summary_w)),
    ));
    l1.push_str(&seg(theme.default_ink, " "));

    // ── line 2: prov │ repo [branch] #PR provider model effort ──
    let mut l2 = String::new();
    let (prov, prov_ink) = match c.prov {
        Some(g) if g == crate::render::WORKTREE_MARK => (g.to_string(), WORKTREE_INK),
        Some(g) => (g.to_string(), BRANCH_INK),
        None => (" ".to_string(), theme.default_ink),
    };
    l2.push_str(&seg(ink(prov_ink), &format!(" {prov} ")));
    l2.push_str(&seg(rail, &format!("{RULE} ")));
    // The PR is IDENTITY — "this branch has a PR open" — which is why it lives
    // on line 2 and not among line 3's measurements, where it reserved five
    // cells for an absence on every row without one. A card with no PR folds
    // those columns back into the repo-and-branch budget.
    let pr_w = if expanded && c.pr.is_some() {
        PR_W + 1
    } else {
        0
    };
    let tail_w = if expanded { META_TAIL_W } else { 0 };
    let budget = build.saturating_sub(INDENT + pr_w + tail_w + 1);
    // The branch renders only where it can carry its guaranteed minimum. A
    // five-cell fragment of `fix/evlog-…` identifies nothing, and those columns
    // are worth more to the repo name, which at least names the checkout.
    let branch_fits = budget >= display_cells(c.repo).min(REPO_W) + 1 + BRANCH_MIN;
    if c.branch.is_empty() || !branch_fits {
        l2.push_str(&seg(ink(c.repo_ink), &pad(c.repo, budget)));
    } else {
        // The branch starts one space after the repo NAME and claims whatever
        // the repo did not use, so a long repo truncates before a branch does.
        let repo_w = display_cells(c.repo).min(REPO_W);
        l2.push_str(&seg(ink(c.repo_ink), &pad(c.repo, repo_w)));
        l2.push_str(&seg(
            ink(META_INK),
            &format!(" {}", pad(c.branch, budget - repo_w - 1)),
        ));
    }
    if pr_w > 0 {
        let pr = c.pr.map_or_else(String::new, |n| format!("#{n}"));
        l2.push_str(&seg(ink(PR_INK), &format!(" {}", pad(&pr, PR_W))));
    }
    if expanded {
        match c.provider {
            Some((glyph, brand)) => l2.push_str(&seg(ink(brand), &format!(" {glyph}"))),
            None => l2.push_str(&seg(theme.default_ink, "  ")),
        }
        l2.push_str(&seg(ink(META_INK), &format!(" {}", pad(c.model, MODEL_W))));
        l2.push_str(&seg(
            ink(META_INK),
            &format!(" {}", pad(c.effort, EFFORT_W)),
        ));
    }
    l2.push_str(&seg(theme.default_ink, " "));

    // ── line 3: subs │ tokens elapsed wants … turn ──
    let mut l3 = String::new();
    // The third structural mark, stacked under status and provenance. A
    // BOOLEAN, not a count: "this row has fanned out" is the whole signal, and
    // a digit beside it was noise.
    let subs = if c.subs { SUBS_MARK } else { ' ' };
    l3.push_str(&seg(ink(SUBS_INK), &format!(" {subs} ")));
    l3.push_str(&seg(rail, &format!("{RULE} ")));
    // Left-aligned, unlike the two-line card's right-aligned count: the four
    // lines share ONE left edge, and line 3's numbers start at it.
    let token_text = match &c.tokens {
        TokenCell::Count(text, band) => (text.clone(), *band),
        TokenCell::Term => (TERM_MARK.to_string(), META_INK),
        TokenCell::Blank => (String::new(), theme.default_ink),
    };
    l3.push_str(&seg(ink(token_text.1), &pad(&token_text.0, TOKEN_W)));
    if expanded {
        // Three measurements and one message. `wants` claims every cell the
        // numbers do not.
        let wants_w =
            build.saturating_sub(INDENT + TOKEN_W + TOKEN_GAP + ELAPSED_W + 1 + 1 + TURN_W + 1);
        // The clocks stay APART: adjacent, `20h 20m` reads as one duration —
        // twenty hours and twenty minutes — rather than two numbers, and no
        // amount of colour saves it, only distance. Elapsed takes the LEFT
        // slot and therefore the crop, because the animated mark already says
        // "thinking" while past an hour without interaction the prompt cache
        // is gone and the next turn costs more. The one that changes what you
        // do is the one that survives collapse.
        l3.push_str(&seg(
            ink(META_INK),
            &format!("{}{}", " ".repeat(TOKEN_GAP), rpad(c.elapsed, ELAPSED_W)),
        ));
        l3.push_str(&seg(
            ink(NEEDS_YOU_INK),
            &format!(" {}", pad(c.wants, wants_w)),
        ));
        l3.push_str(&seg(ink(TURN_INK), &format!(" {}", rpad(c.turn, TURN_W))));
        l3.push_str(&seg(theme.default_ink, " "));
    } else {
        // The crop rule: collapsed is a LEFT-CROP of expanded, never a
        // re-arrangement, so a cell's column IS its priority. Line 3's crop
        // reaches the token count and the time since you last touched the row
        // — how hot, and how stale — and stops. The time LOCKS to the right
        // edge, one cell in: the same edge the turn clock holds in the
        // expanded card, so whichever clock is rightmost sits in the same
        // column and the eye finds a duration in one place in both profiles.
        // The fill is whatever is left rather than a fixed cell, so the line
        // lands on `build` without a second constant to keep in step.
        let fill = build.saturating_sub(display_cells(&strip_sgr(&l3)) + ELAPSED_W + 1);
        l3.push_str(&seg(theme.default_ink, &" ".repeat(fill)));
        l3.push_str(&seg(ink(META_INK), &rpad(c.elapsed, ELAPSED_W)));
        l3.push_str(&seg(theme.default_ink, " "));
    }

    // ── line 4: the shadow rule ──
    // Full width and OUTSIDE the selection bar: the separator rhythm stays
    // unbroken across the whole fleet, and the selection is exactly the three
    // lines that carry content.
    let l4 = format!(
        "\u{1b}[49m{}{}",
        SHADOW_INK.fg(),
        SHADOW.to_string().repeat(build)
    );

    // ONE exit for all four lines, as the two-line card has for its two: the
    // clip closes the SGR state and guarantees exactly `cols` cells.
    [
        clip_to_cells(&l1, cols),
        clip_to_cells(&l2, cols),
        clip_to_cells(&l3, cols),
        clip_to_cells(&l4, cols),
    ]
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
        wants: Option<&'static str>,
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
                model: Some("fable"),
                effort: Some("hi"),
                tokens: Some(100_000),
                battery: Some(5),
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
                    wants: self.wants.map(String::from),
                    subagents: self.subs,
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
                // The only fleet row with an ask. `wants` is written for
                // flagged rows and nothing else, so this row is also the only
                // one that COULD carry it — and pinning it is what holds
                // line 3's right-hand arithmetic, which the width invariant
                // alone cannot: `clip_to_cells` would silently eat a
                // miscounted cell that had nothing in it.
                wants: Some("Bash (cargo mutants --in-diff)"),
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
                // The one fleet row that has fanned out, so the golden pins
                // both states of the third structural mark.
                subs: true,
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
        let (l1, l2) = render_double_card(row, cols, true, false, &Theme::default());
        (strip_sgr(&l1), strip_sgr(&l2))
    }

    #[test]
    fn every_card_line_is_exactly_cols_wide_in_both_profiles() {
        for cols in [38, 48] {
            for (i, row) in fleet().iter().enumerate() {
                for zebra in [false, true] {
                    for any in [false, true] {
                        let (l1, l2) = render_double_card(row, cols, any, zebra, &Theme::default());
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
                let (l1, l2) = render_double_card(row, cols, true, i % 2 == 1, &Theme::default());
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
                " \u{f1bb} \u{2570}  clave     #225  \u{ec82} fable  xh  5m ".to_string(),
            )
        );
        assert_eq!(
            pin(sel, 48),
            (
                " \u{25cf} \u{256d} \u{e0b6}CLV-M2 \u{e0b4} Goal is shipping v0.2.2 cl\u{2026} 130k "
                    .to_string(),
                " \u{f1bb} \u{2570}  clave v022-prep     #225  \u{ec82} fable  xh  5m ".to_string(),
            )
        );
    }

    // ── the four-line card ──────────────────────────────────────────────────

    /// The four-line card, cell for cell, at both profiles. Lines 1-3 only —
    /// line 4 is the same rule on every card, and is pinned once by
    /// `the_shadow_rule_closes_every_card_outside_the_selection` rather than
    /// repeated seven times here.
    ///
    /// Seven rows, chosen for their corners: a worktree with a PR and a branch,
    /// a terminal, a chipless card, a repo past its budget beside a branch, a
    /// failed row whose summary truncates, a row with NO token reading at all
    /// (line 3 goes fully blank, which is the design and not a hole), and the
    /// `Opening` row that escapes the dormant fade.
    ///
    /// The subagent mark, `wants` and the turn clock render BLANK throughout,
    /// because nothing writes them to the wire yet. These goldens therefore
    /// pin what ships today; wiring each source is a change that MOVES them,
    /// which is exactly what a golden is for.
    #[test]
    fn the_four_line_card_pins_both_profiles() {
        let f = fleet();
        let want: [(usize, [&str; 3], [&str; 3]); 8] = [
            (
                // The one flagged row, and so the only one carrying an ask.
                // Its line 3 is what pins the right-hand arithmetic: the
                // ellipsis lands where `wants_w` says it does, and a
                // miscount that `clip_to_cells` would swallow on a blank
                // cell moves it here.
                0,
                [
                    " \u{25cf} \u{2502} \u{e0b6}CORTI2 \u{e0b4} Qdos IR35 assessment: the contr\u{2026} ",
                    "   \u{2502} hermes                         \u{ec82} fable  hi ",
                    "   \u{2502} 105k   3m Bash (cargo mutants --in-di\u{2026}     ",
                ],
                [
                    " \u{25cf} \u{2502} \u{e0b6}CORTI2 \u{e0b4}  ",
                    "   \u{2502} hermes     ",
                    "   \u{2502} 105k    3m ",
                ],
            ),
            (
                4,
                [
                    " \u{b7} \u{2502} \u{e0b6}CLV-3  \u{e0b4} Drive launch                     ",
                    " \u{f1bb} \u{2502} clave drive-launch       #204  \u{ec82} sonnet hi ",
                    " \u{f171a} \u{2502} 117k  45m                                  ",
                ],
                [
                    " \u{b7} \u{2502} \u{e0b6}CLV-3  \u{e0b4}  ",
                    " \u{f1bb} \u{2502} clave      ",
                    " \u{f171a} \u{2502} 117k   45m ",
                ],
            ),
            (
                6,
                [
                    " \u{f018d} \u{2502} \u{e0b6}Tab #12\u{e0b4} zsh                              ",
                    "   \u{2502} clave                                      ",
                    "   \u{2502} TERM   7m                                  ",
                ],
                [
                    " \u{f018d} \u{2502} \u{e0b6}Tab #12\u{e0b4}  ",
                    "   \u{2502} clave      ",
                    "   \u{2502} TERM    7m ",
                ],
            ),
            (
                9,
                [
                    " \u{b7} \u{2502} Create close conversation summary flow     ",
                    "   \u{2502} hermes                         \u{ec82} opus   hi ",
                    "   \u{2502} 34k    2h                                  ",
                ],
                [
                    " \u{b7} \u{2502} Create cl\u{2026} ",
                    "   \u{2502} hermes     ",
                    "   \u{2502} 34k     2h ",
                ],
            ),
            (
                13,
                [
                    " \u{b7} \u{2502} Landing page hero copy rewrite pass        ",
                    " \u{f062c} \u{2502} clave-we\u{2026} hero-copy      #12   \u{ec81} gpt-5     ",
                    "   \u{2502} 55k   30m                                  ",
                ],
                [
                    " \u{b7} \u{2502} Landing p\u{2026} ",
                    " \u{f062c} \u{2502} clave-web\u{2026} ",
                    "   \u{2502} 55k    30m ",
                ],
            ),
            (
                14,
                [
                    " \u{2716} \u{2502} \u{e0b6}MIGRATE\u{e0b4} Postgres 15 to 17 migration run\u{2026} ",
                    " \u{f1bb} \u{2502} market-s\u{2026} pg17-migrate   #88   \u{ec82} sonnet hi ",
                    "   \u{2502} 201k   4h                                  ",
                ],
                [
                    " \u{2716} \u{2502} \u{e0b6}MIGRATE\u{e0b4}  ",
                    " \u{f1bb} \u{2502} market-sc\u{2026} ",
                    "   \u{2502} 201k    4h ",
                ],
            ),
            (
                16,
                [
                    " \u{b7} \u{2502} \u{e0b6}GROK   \u{e0b4} Provider clave has never heard \u{2026} ",
                    "   \u{2502} clave                                      ",
                    "   \u{2502}                                            ",
                ],
                [
                    " \u{b7} \u{2502} \u{e0b6}GROK   \u{e0b4}  ",
                    "   \u{2502} clave      ",
                    "   \u{2502}            ",
                ],
            ),
            (
                17,
                [
                    " \u{21bb} \u{2502} \u{e0b6}OPENING\u{e0b4} Just launched                    ",
                    "   \u{2502} clave                            fable     ",
                    "   \u{2502} 100k   1m                                  ",
                ],
                [
                    " \u{21bb} \u{2502} \u{e0b6}OPENING\u{e0b4}  ",
                    "   \u{2502} clave      ",
                    "   \u{2502} 100k    1m ",
                ],
            ),
        ];
        for (i, expanded, collapsed) in want {
            for (cols, want_lines) in [
                (CARD_EXPANDED_COLS, expanded),
                (CARD_COLLAPSED_COLS, collapsed),
            ] {
                let got = render_card(&f[i], cols, true, 0, &Theme::default());
                for (line, want_line) in want_lines.iter().enumerate() {
                    assert_eq!(
                        strip_sgr(&got[line]),
                        *want_line,
                        "row {i} line {} at {cols} cols",
                        line + 1
                    );
                }
            }
        }
    }

    /// Line 4 is the whole reason the card grew: three dense lines butted
    /// against the next three read fine one card at a time and stop being
    /// glanceable as a fleet, because the eye cannot find where one card ends.
    ///
    /// Two properties, and the second is the subtle one. The rule spans the
    /// full pane at every width — and it carries NO background even on the
    /// selected card, because the separator rhythm has to stay unbroken across
    /// the whole fleet while the selection is exactly the three lines that
    /// carry content.
    #[test]
    fn the_shadow_rule_closes_every_card_outside_the_selection() {
        let f = fleet();
        let bar = Theme::default().sel_bg.bg();
        for cols in [CARD_EXPANDED_COLS, CARD_COLLAPSED_COLS, 30, 8] {
            for (i, row) in f.iter().enumerate() {
                let l4 = &render_card(row, cols, true, 0, &Theme::default())[3];
                assert_eq!(
                    strip_sgr(l4),
                    SHADOW.to_string().repeat(cols),
                    "row {i} at {cols} cols is not a full-width rule"
                );
                assert!(
                    !l4.contains(&bar),
                    "row {i} painted the selection background onto the separator"
                );
            }
        }
        // The selected card proves the second property rather than assuming
        // it: its three content lines DO paint the bar, and its fourth does
        // not, so the assertion above is measuring something real.
        let selected = f.iter().position(|r| r.selected).expect("fleet fixture");
        let card = render_card(&f[selected], CARD_EXPANDED_COLS, true, 0, &Theme::default());
        for (line, painted) in card.iter().take(3).enumerate() {
            assert!(
                painted.contains(&bar),
                "the selected card's line {} lost its bar",
                line + 1
            );
        }
    }

    /// The crop rule: collapsed is a strict LEFT-CROP of expanded, never a
    /// re-arrangement — so a cell's column IS its priority, and every argument
    /// about ordering is an argument about what survives 16 columns.
    ///
    /// The crop is over CELLS, not characters, and the difference is not a
    /// technicality. Collapsing hands the branch, PR, provider, model and
    /// effort columns back to the repo cell, so a repo name that truncates at
    /// 48 shows MORE of itself at 16 — `clave-we…` becomes `clave-web…`. A
    /// naive "the narrow line is a prefix of the wide one" property is
    /// therefore false on a correct card, and this test asserts the three
    /// things that actually hold.
    ///
    /// One: the chrome is at the same columns in both profiles, on all three
    /// content lines — that is the "one left edge for all four lines" ruling,
    /// and it is what a re-arrangement would break first. Two: the pill is
    /// untouched by the crop, because it is what survives 16 columns. Three:
    /// the elapsed clock is flush to the right margin in both profiles, so
    /// whichever clock is rightmost sits in the same column and the eye finds
    /// a duration in one place.
    #[test]
    fn the_collapsed_card_crops_cells_without_moving_them() {
        let f = fleet();
        for (i, row) in f.iter().enumerate() {
            let wide = render_card(row, CARD_EXPANDED_COLS, true, 0, &Theme::default());
            let narrow = render_card(row, CARD_COLLAPSED_COLS, true, 0, &Theme::default());
            for line in 0..3 {
                let w: String = strip_sgr(&wide[line]).chars().take(CHROME).collect();
                let n: String = strip_sgr(&narrow[line]).chars().take(CHROME).collect();
                assert_eq!(w, n, "row {i} line {} moved its chrome", line + 1);
                assert!(
                    w.ends_with(&format!("{RULE} ")),
                    "row {i} line {}: the rail left its column ({w:?})",
                    line + 1
                );
            }
            // The pill survives the crop whole — it is the first thing the
            // column order protects.
            let pill: String = strip_sgr(&wide[0]).chars().skip(CHROME).take(9).collect();
            let cropped: String = strip_sgr(&narrow[0]).chars().skip(CHROME).take(9).collect();
            assert_eq!(pill, cropped, "row {i} clipped its pill");
            // Line 3's rightmost cell is a duration, one margin in, at both
            // widths — the turn clock expanded, the elapsed clock collapsed.
            for card in [&wide, &narrow] {
                let l3 = strip_sgr(&card[2]);
                assert!(
                    l3.ends_with(' '),
                    "row {i} line 3 lost its right margin: {l3:?}"
                );
            }
            // Only where there IS a reading. A row that has never been touched
            // renders the cell blank rather than inventing a duration, which
            // is the same "blank is the meaning" rule the whole line follows.
            if !cells(&row.content, &Theme::default()).elapsed.is_empty() {
                let narrow_l3 = strip_sgr(&narrow[2]);
                let tail: String = narrow_l3.chars().rev().skip(1).take(ELAPSED_W).collect();
                assert!(
                    !tail.trim().is_empty(),
                    "row {i}: the collapsed card's elapsed clock is not flush right ({narrow_l3:?})"
                );
            }
        }
    }

    /// The spinner: a working row's status mark BREATHES, and nothing else on
    /// the card moves with it. A row blocked on you holds still, which is what
    /// makes a stopped spinner mean "Claude is asking".
    #[test]
    fn only_a_working_rows_mark_animates() {
        let working = A {
            status: RowStatus::Working,
            ..A::default()
        }
        .row();
        let asking = A {
            status: RowStatus::NeedsYou,
            ..A::default()
        }
        .row();
        let at = |row: &Row, frame: usize| {
            strip_sgr(&render_card(row, CARD_EXPANDED_COLS, false, frame, &Theme::default())[0])
        };
        // The ping-pong closes: frame 10 is frame 0 again, so the cycle turns
        // over without a jump, and every one of the six glyphs is visited.
        let cycle: Vec<String> = (0..THINK_CYCLE).map(|t| at(&working, t)).collect();
        assert_eq!(at(&working, THINK_CYCLE), cycle[0], "the cycle must close");
        assert_eq!(
            cycle
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            THINK_FRAMES.len(),
            "the ping-pong must visit every frame"
        );
        // Only the mark moves: every frame is identical past the status cell,
        // so an animating card cannot smuggle a reflow through.
        for (t, line) in cycle.iter().enumerate() {
            assert_eq!(display_cells(line), CARD_EXPANDED_COLS, "frame {t} width");
            assert_eq!(
                line.chars().skip(2).collect::<String>(),
                cycle[0].chars().skip(2).collect::<String>(),
                "frame {t} moved something other than the status mark"
            );
        }
        // The ping-pong is a REFLECTION, not just a closed loop: the walk back
        // must retrace the frames it walked out on. Counting distinct frames
        // cannot see that — a cycle that returned by some other route would
        // still visit all six — so the symmetry is asserted directly.
        for k in 1..THINK_FRAMES.len() {
            assert_eq!(
                think_frame(THINK_CYCLE - k),
                think_frame(k),
                "frame {} is not the reflection of frame {k}",
                THINK_CYCLE - k
            );
        }

        // The row blocked on you holds still across the whole cycle.
        let still = at(&asking, 0);
        for t in 0..THINK_CYCLE {
            assert_eq!(
                at(&asking, t),
                still,
                "a NeedsYou row animated at frame {t}"
            );
        }
    }

    #[test]
    fn provenance_wears_a_fixed_semantic_ink_not_the_rows() {
        // Lock §4.2: worktree green, branch violet, and an ordinary checkout
        // no mark at all. The inks are the reason a third glyph could join
        // that column at all, and a golden over stripped text cannot see them
        // — so this reads the SGR the goldens throw away.
        let line2 = |prov: Provenance| {
            render_card(
                &A {
                    prov,
                    branch: "topic",
                    ..A::default()
                }
                .row(),
                CARD_EXPANDED_COLS,
                false,
                0,
                &Theme::default(),
            )[1]
            .clone()
        };
        let worktree = line2(Provenance::Worktree);
        let branch = line2(Provenance::Branch);
        assert!(
            worktree.contains(&WORKTREE_INK.fg()),
            "the worktree mark must be springGreen: {worktree:?}"
        );
        assert!(
            branch.contains(&BRANCH_INK.fg()),
            "the branch mark must be oniViolet: {branch:?}"
        );
        assert!(
            !worktree.contains(&BRANCH_INK.fg()) && !branch.contains(&WORKTREE_INK.fg()),
            "the two inks must not both be painted"
        );
        // An ordinary checkout has no mark, so it claims neither ink.
        let main = line2(Provenance::Main);
        assert!(!main.contains(&WORKTREE_INK.fg()) && !main.contains(&BRANCH_INK.fg()));
    }

    #[test]
    fn a_summary_cell_too_narrow_to_say_anything_says_nothing() {
        // Lock §2: a cell with room for an ellipsis and nothing else says less
        // than a blank one. The threshold is four cells, so the card is swept
        // across the widths where it is crossed rather than pinned at one.
        let row = A {
            chip: Some("CHIPPED"),
            summary: "a summary long enough to be cut",
            ..A::default()
        }
        .row();
        for cols in CARD_COLLAPSED_COLS..CARD_EXPANDED_COLS {
            let l1 = strip_sgr(&render_card(&row, cols, false, 0, &Theme::default())[0]);
            assert_eq!(display_cells(&l1), cols, "width at {cols}");
            let shown = l1.split('\u{e0b4}').nth(1).unwrap_or("").trim();
            assert!(
                shown.is_empty() || shown.chars().filter(|c| *c != ELLIPSIS).count() >= 3,
                "at {cols} the summary cell said only {shown:?}"
            );
        }
    }

    #[test]
    fn the_wants_cell_carries_the_ask_and_survives_the_crop_only_as_far_as_it_fits() {
        // Lock §4.7: `wants` claims every cell the numbers on line 3 leave, so
        // the expanded card shows the ask and the collapsed one — 16 columns,
        // all of them spoken for by the token count and the clock — shows none
        // of it. Blank stays blank: a row that is not blocked grows no text.
        let blocked = A {
            status: RowStatus::NeedsYou,
            wants: Some("Bash (git push --force)"),
            ..A::default()
        }
        .row();
        let quiet = A {
            status: RowStatus::NeedsYou,
            ..A::default()
        }
        .row();
        let line3 = |row: &Row, cols: usize| {
            strip_sgr(&render_card(row, cols, false, 0, &Theme::default())[2])
        };

        let wide = line3(&blocked, CARD_EXPANDED_COLS);
        assert!(
            wide.contains("Bash (git push --for"),
            "the expanded card must show the ask: {wide:?}"
        );
        assert_eq!(display_cells(&wide), CARD_EXPANDED_COLS);

        let narrow = line3(&blocked, CARD_COLLAPSED_COLS);
        assert!(
            !narrow.contains("Bash"),
            "16 columns have no room for an ask: {narrow:?}"
        );
        assert_eq!(display_cells(&narrow), CARD_COLLAPSED_COLS);

        // The unblocked row's line 3 is the same shape with the cell empty —
        // the ask grows into space that was already there, it does not reflow
        // the numbers beside it.
        let empty = line3(&quiet, CARD_EXPANDED_COLS);
        assert_eq!(display_cells(&empty), CARD_EXPANDED_COLS);
        let shared = empty
            .chars()
            .zip(wide.chars())
            .take_while(|(a, b)| a == b)
            .count();
        assert!(
            empty.chars().skip(shared).all(char::is_whitespace),
            "the ask must grow into space that was already blank: {empty:?}"
        );
    }

    // ── the two-line card ───────────────────────────────────────────────────

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
                " \u{f1bb} \u{2570}  clave     #204  \u{ec82} sonnet hi 45m ",
                " \u{25cf} \u{256d} \u{e0b6}CLV-3  \u{e0b4} Drive launch                117k ",
                " \u{f1bb} \u{2570}  clave drive-launch  #204  \u{ec82} sonnet hi 45m ",
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
                " \u{f1bb} \u{2570}  market-s\u{2026} #88   \u{ec82} sonnet hi  4h ",
                " \u{2716} \u{256d} \u{e0b6}MIGRATE\u{e0b4} Postgres 15 to 17 migratio\u{2026} 201k ",
                " \u{f1bb} \u{2570}  market-s\u{2026} pg17-mig\u{2026} #88   \u{ec82} sonnet hi  4h ",
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
            " \u{f1bb} \u{2570}  resumaker resu\u{2026}              1h "
        );
        assert_eq!(
            pin(&f[12], 48).1,
            " \u{f1bb} \u{2570}  resumaker resume-fix                   1h "
        );
    }

    #[test]
    fn glass_rows_reassert_default_bg_and_selection_paints_sel_bg() {
        let theme = Theme::default();
        let f = fleet();
        let (l1, l2) = render_double_card(&f[0], 38, true, false, &theme);
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
        let (s1, s2) = render_double_card(&f[7], 38, true, false, &theme);
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
        let (_, l2) = render_double_card(&row, 38, true, false, &Theme::default());
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
            render_double_card(&row, 38, false, false, &theme).0
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
        let (l1, _) = render_double_card(&f[0], 38, false, false, &theme);
        assert!(l1.contains(&ink(theme.default_ink, 0.0)));
        // Something selected: an unselected card recedes — line 1's summary
        // ink and line 2's metadata ink both.
        let (l1, l2) = render_double_card(&f[0], 38, true, false, &theme);
        assert!(l1.contains(&ink(theme.default_ink, FADE)));
        assert!(l2.contains(&ink(META_INK, FADE)));
        // Dormant: both lines, whether or not anything is selected.
        for any in [false, true] {
            let (l1, l2) = render_double_card(&f[15], 38, any, false, &theme);
            for l in [&l1, &l2] {
                assert!(
                    l.contains(&ink(META_INK, DORMANT_FADE)),
                    "dormant, any={any}"
                );
            }
        }
        // Opening: flagged dormant, rendered live.
        let (l1, _) = render_double_card(&f[17], 38, false, false, &theme);
        assert!(!l1.contains(&ink(META_INK, DORMANT_FADE)));
    }

    /// The zebra lives in the LINEWORK: alternating bracket inks, never a
    /// second background — glass has no second opacity to give.
    #[test]
    fn the_zebra_alternates_the_bracket_ink() {
        let theme = Theme::default();
        let f = fleet();
        let (a1, a2) = render_double_card(&f[0], 38, false, false, &theme);
        let (b1, b2) = render_double_card(&f[0], 38, false, true, &theme);
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
                let (l1, l2) = render_double_card(&row, cols, true, false, &Theme::default());
                for l in [&l1, &l2] {
                    assert_eq!(display_cells(&strip_sgr(l)), cols, "{summary:?} at {cols}");
                }
                assert!(!strip_sgr(&l1).contains('\n'));
            }
        }
    }
}
