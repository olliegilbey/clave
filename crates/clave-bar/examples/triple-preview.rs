//! clave sidebar — the FOUR-LINE card, a DESIGN ROUND.
//!
//!     cargo run -p clave-bar --example triple-preview
//!
//! RATIFIED 2026-09-08. `docs/superpowers/specs/2026-09-08-triple-height-card-lock.md`
//! is authoritative for every ruling, number and rationale below; where it and
//! this file disagree, it wins and this file is the bug.
//!
//! **This file is not a port.** Unlike `double-preview.rs`, which renders
//! through `render_rows` because its geometry already ships, this one carries
//! its OWN copy — the card below does not exist in `card.rs` yet, so there is
//! nothing to render through, and it CAN drift from the lock in a way
//! `double-preview.rs` cannot. Porting the geometry into `card.rs` and
//! rewriting this file to call the real renderer is the next piece of work;
//! the lock's §9 lists what it costs.
//!
//! ```text
//!   line 1:  status  │ chip-pill  summary
//!   line 2:  prov    │ repo [branch]  #PR   provider model effort
//!   line 3:  subs    │ tokens  elapsed  wants               turn
//!   line 4:  the shadow rule — a hairline that closes the card
//! ```
//!
//! Three lines of content, one of separation. Three dense lines butted against
//! the next three read fine one card at a time and stop being GLANCEABLE as a
//! fleet: the eye cannot find where one card ends (round 2). The fourth line
//! fixes that, and the fleet halves on screen to pay for it.
//!
//! ## The rulings, in the order they were made
//!
//! **The crop rule** (round 6) governs everything else. Collapsed is a LEFT-CROP
//! of expanded, never a re-arrangement — so a cell's COLUMN IS ITS PRIORITY, and
//! any argument about ordering is an argument about what survives 16 columns.
//! What survives: the status mark, the chip, the repo, the token count and the
//! time since you last touched it. What does not: the summary, the branch, the
//! PR, the provider, the model, the effort tag, the turn clock, and the
//! message.
//!
//! **One left edge for all four lines** (round 9). Content starts flush with
//! the chrome on every line, under the pill's left CAP — the pill is a solid
//! block of colour, so its outer edge is what the column reads against, not the
//! first letter inside it. A chipless card therefore drops the pill's trailing
//! GAP along with the pill: with no pill there is nothing to hold air away
//! from, and that space read as a one-cell stagger.
//!
//! **The rail is the repo's colour**, running the card's full height — one
//! continuous identity spine per card. It replaced an arc whose two neutrals
//! alternated card by card, which was linework doing a separator's job because
//! glass forbade a painted stripe. The blank line does that job properly, so
//! the zebra retired with the arc.
//!
//! **Provenance keeps its own fixed inks**, green for a worktree and violet for
//! a branch, since the rail now carries the repo identity the glyph used to
//! borrow. Main draws nothing, as ever.
//!
//! **The three lines are three categories.** Line 1 is who and what. Line 2 is
//! which checkout, which model. Line 3 is how much, how long, what it needs.
//! The PR lives on line 2 because it is IDENTITY — this branch has a PR open —
//! and on line 3 it sat among measurements it has nothing to do with, reserving
//! five cells for an absence on every row without one.
//!
//! **The two clocks stay apart, and elapsed is the one that matters.**
//! Adjacent, `20h 20m` reads as one duration rather than two numbers, and no
//! amount of colour saves it — only distance does. Elapsed takes the left slot
//! and the crop, because the animated mark already says "thinking" and because
//! an hour without interaction invalidates the prompt cache, which changes what
//! the next turn costs. The turn clock takes the right edge.
//!
//! **The turn clock is the only live number on the card**, blue while a turn is
//! in flight and BLANK when none is. It costs no store traffic: the store holds
//! when the turn began and the bar subtracts from its own clock, so the fleet
//! is never re-pushed to animate a counter.
//!
//! **The status mark animates while working** — Claude Code's own spinner, in
//! the one cell every profile keeps, so the collapsed card gains a heartbeat
//! for free. A row blocked on you does NOT animate; the spinner stops when
//! Claude asks.
//!
//! `wants` is the flexing cell and the reason for the whole change: what this
//! agent is blocked on, in its own words. Blank means nothing is needed, so an
//! idle fleet has a quiet right-hand column and the one row that wants you
//! grows text. Its sources, cheapest first: the tool name from a pending
//! permission prompt (free — clave's hook already receives it and throws it
//! away), then the agent's closing question shortened by the scribe (a model
//! call, later).
//!
//! GLYPH RULE — load-bearing (lock §5.4). Every glyph below is a `\u{...}`
//! escape, never a literal character. And every one of them was checked against
//! the installed font's cmap, not a cheat sheet: two candidates from these
//! rounds do not exist at the codepoints the cheat sheets give (lock §6).

use clave_bar::render::{display_cells, strip_sgr};
use clave_bar::theme::{
    BATTERY, CLAUDE_GLYPH, CLAUDE_INK, DORMANT_FADE, ELLIPSIS, FADE, LCAP, META_INK, NEEDS_YOU_INK,
    OPENAI_GLYPH, OPENAI_INK, PR_INK, RCAP, RESET, RULE, Rgb, Theme,
};

/// The preview's own chrome — profile captions. Not part of the design.
const DIM: Rgb = Rgb(0x71, 0x7C, 0x7C);

// ── the proposed budgets ────────────────────────────────────────────────────

const CHIP_W: usize = 7;
const REPO_W: usize = 9;
/// The branch's guaranteed minimum: repo and branch share one budget and the
/// branch takes what the repo leaves, never less than this, so a long repo
/// truncates first. Below it the branch does not render at all.
const BRANCH_MIN: usize = 9;
const MODEL_W: usize = 6;
const EFFORT_W: usize = 2;
const PR_W: usize = 5;
const TOKEN_W: usize = 4;
const TURN_W: usize = 3;
const ELAPSED_W: usize = 3;

/// The card's left chrome: margin, mark, air, rail, air. Round 3 tried four by
/// butting the rail against the mark column; rejected on sight — the rule sat
/// on top of the glyphs. The column goes back into the gap, and it costs
/// nothing that matters: the collapsed profile still lands exactly on 16.
const CHROME: usize = 5;

/// Where lines 2 and 3 start their content: flush with the chrome, so the repo
/// name and the token count sit under the pill's left CAP.
///
/// Indenting one cell further — aligning them to the pill's LABEL instead —
/// was tried and rejected by eye. The pill is a solid block of colour, so the
/// edge the column reads against is its outer edge, not the first letter
/// inside it.
const INDENT: usize = CHROME;

/// The gap between the token count and the time beside it. TWO cells, not one:
/// at one they read as a single figure, the same failure that keeps the two
/// clocks apart.
const TOKEN_GAP: usize = 2;

/// Provenance's own inks, FIXED and semantic — no longer borrowed from the
/// repo (round 6). The rail carries the repo's identity now, so the glyph is
/// free to say what KIND of checkout this is and to say it the same way on
/// every card: green for a worktree, violet for a branch, nothing for main.
const WORKTREE_INK: Rgb = Rgb(0x98, 0xBB, 0x6C); // springGreen
const BRANCH_INK: Rgb = Rgb(0x95, 0x7F, 0xB8); // oniViolet

/// The turn clock's ink while a turn is RUNNING: kanagawa crystalBlue. It is
/// the card's only live number, and the blue is the signal — a running turn is
/// the one thing on the card that is true right now rather than true as of the
/// last hook. When no turn is in flight the cell renders BLANK rather than
/// dimming: blank is already the card's word for "no reading", and a blank
/// clock beside a lit one is the cheapest possible "this agent is thinking".
const TURN_INK: Rgb = Rgb(0x7E, 0x9C, 0xD8);

// ── the shadow rule ─────────────────────────────────────────────────────────

/// The gap line's hairline: a rule so dark it reads as a shadow under the card
/// rather than as a border between two. Candidates rendered as a swatch at the
/// bottom — pick by eye on the real display, since how thin "thin" looks is a
/// property of the font and the panel, not of the codepoint.
const SHADOW_GLYPHS: [(char, &str); 4] = [
    (
        '\u{2594}',
        "U+2594  upper one eighth block — hugs the card above",
    ),
    (
        '\u{2500}',
        "U+2500  box light horizontal — mid-cell hairline",
    ),
    (
        '\u{2581}',
        "U+2581  lower one eighth block — sits on the next card",
    ),
    (
        '\u{254c}',
        "U+254C  light double dash — broken, quieter still",
    ),
];

/// Three intensities against the sumiInk3 bar background.
const SHADOW_INKS: [(Rgb, &str); 3] = [
    (Rgb(0x2A, 0x2A, 0x37), "sumiInk4 — barely there"),
    (Rgb(0x36, 0x36, 0x46), "sumiInk5 — the default"),
    (Rgb(0x54, 0x54, 0x6D), "sumiInk6 — plainly a line"),
];

/// What the fleets below draw.
/// Ratified round 4, by eye: the mid-cell hairline at the faintest of the
/// three greys. It closes the card without ever reading as a border between
/// two of them.
const SHADOW: char = SHADOW_GLYPHS[1].0;
const SHADOW_INK: Rgb = SHADOW_INKS[0].0;

/// The subagent mark's ink — the quiet blue-grey of a structural mark, since
/// it says "this row has depth", not "this row is hot".
const SUBS_INK: Rgb = Rgb(0x9C, 0xAB, 0xCA);

/// The subagent mark. `md-robot_happy_outline`, verified present in the
/// installed Nerd Font — the codicon set has no `cod-robot` at all, and its
/// nearest thing is `cod-hubot`.
const SUBS_MARK: char = '\u{f171a}';

/// The card's two width profiles. Expanded is unchanged at 48; collapsed drops
/// from 38 to 16, because a collapsed card still showing the model, the branch
/// and two clocks is not collapsed, it is narrow.
#[derive(Clone, Copy, PartialEq)]
enum Profile {
    /// Everything. 48 columns.
    Expanded,
    /// The leftmost cell of each line and nothing else: chip, repo, tokens+PR.
    Collapsed,
}

// ── the thinking animation ────────────────────────────────────────────────
//
// Claude Code's own spinner, in the card's status cell. A row that is WORKING
// stops wearing a static dot and breathes instead, which turns the one cell
// every profile keeps into a live signal — the collapsed card gains a
// heartbeat for free.
//
// FONT WARNING, measured: five of these six are absent from every Nerd Font
// installed on this machine and arrive by terminal FALLBACK, out of Menlo,
// which does carry all six. Menlo is monospace so the cell width holds — but
// this is the one part of the design that cannot be signed off from a static
// render. Run it with `--animate` and look at it.
/// ORDER IS THE ANIMATION: the mark grows from a dot through progressively
/// heavier stars and back. Listing them in any other sequence gives six glyphs
/// flickering rather than one shape breathing.
const THINK_FRAMES: [char; 6] = [
    '\u{00b7}', // interpunct
    '\u{2722}', // four teardrop-spoked asterisk
    '\u{2733}', // eight-spoked asterisk
    '\u{2736}', // six pointed black star
    '\u{273b}', // teardrop-spoked asterisk
    '\u{273d}', // heavy teardrop-spoked asterisk
];

/// Ping-pong: the six frames out and the four inner ones back, so the cycle
/// turns over without a jump. Ten frames.
const THINK_CYCLE: usize = 10;

fn think_frame(t: usize) -> char {
    let i = t % THINK_CYCLE;
    THINK_FRAMES[if i < 6 { i } else { THINK_CYCLE - i }]
}

// ── the mock fleet ──────────────────────────────────────────────────────────

/// One card's worth of state. A local struct rather than `RowContent` because
/// two of these fields — turn and wants — do not exist on the wire yet;
/// inventing them here is the point of a design round.
struct Card {
    mark: (char, Rgb),
    prov: Option<char>,
    chip: Option<&'static str>,
    chip_ink: u8,
    summary: &'static str,
    repo: &'static str,
    repo_ink: u8,
    branch: &'static str,
    pr: Option<u32>,
    provider: Option<&'static str>,
    model: &'static str,
    effort: &'static str,
    /// (ramp level, exact tokens) — the ink is the band, the text the magnitude.
    battery: Option<(u8, u32)>,
    /// Whether this row has any subagent in flight. A boolean: the mark shows
    /// or it does not.
    subs: bool,
    /// Working right now — the status cell animates instead of sitting still.
    /// Every row with a live turn clock has this, and no row without one does.
    thinking: bool,
    /// How long the turn in flight has been going. Blank when no turn is.
    turn: &'static str,
    /// What this row is blocked on. Blank is the common case and the point.
    wants: &'static str,
    elapsed: &'static str,
    selected: bool,
    dormant: bool,
}

impl Default for Card {
    fn default() -> Card {
        Card {
            mark: ('\u{25cf}', clave_bar::theme::WORKING_INK),
            prov: None,
            chip: None,
            chip_ink: 0,
            summary: "",
            repo: "clave",
            repo_ink: 0,
            branch: "",
            pr: None,
            provider: Some("claude"),
            model: "opus",
            effort: "hi",
            battery: Some((5, 100_000)),
            subs: false,
            thinking: false,
            turn: "",
            wants: "",
            elapsed: "1m",
            selected: false,
            dormant: false,
        }
    }
}

const BRANCH_MARK: char = '\u{f062c}'; // md-source_branch

/// The worktree mark, CHANGED from the shipped `\u{168c2}` (bamum tree). That
/// codepoint is absent from every Nerd Font on this machine — it was arriving
/// by fallback out of a historic-script font, which is exactly why it never sat
/// on the baseline with the other marks. Not a design preference: a live defect
/// in the two-line card, found by enumerating the font's character map rather
/// than by reading a cheat sheet. `fa-tree` is present, and keeps the meaning.
const WORKTREE_MARK: char = '\u{f1bb}'; // fa-tree

/// The fleet: the corners of the variant space, plus the cases this round
/// exists to judge — a permission ask, a prose question, a branch name that
/// used to truncate, and the many rows that want nothing at all.
fn fleet() -> Vec<Card> {
    use clave_bar::theme::{DONE_INK, FAILED_INK, NEEDS_YOU_INK as RED, WORKING_INK};
    let dot = '\u{25cf}';
    vec![
        // Blocked on a permission prompt: `wants` from the hook, no model.
        Card {
            mark: (dot, RED),
            chip: Some("CLA-AI"),
            chip_ink: 3,
            summary: "Clave AI assessment",
            branch: "fix/evlog-atomic-append",
            prov: Some(BRANCH_MARK),
            pr: Some(256),
            effort: "xh",
            battery: Some((4, 71_000)),
            // Blocked on you: the spinner stops when Claude asks.
            thinking: false,
            turn: "12m",
            wants: "perm: Bash",
            elapsed: "27m",
            ..Card::default()
        },
        // Blocked on a prose question: `wants` from the scribe, stage two.
        Card {
            mark: (dot, RED),
            chip: Some("BEACON"),
            chip_ink: 6,
            summary: "Olympus build",
            repo: "beacon",
            repo_ink: 2,
            battery: Some((6, 129_000)),
            // Blocked on you: the spinner stops when Claude asks.
            thinking: false,
            turn: "3m",
            wants: "which migration order?",
            elapsed: "17m",
            ..Card::default()
        },
        Card {
            chip: Some("OLYMPUS"),
            chip_ink: 1,
            summary: "Validate and fix codebase",
            repo: "olympus",
            repo_ink: 1,
            branch: "chore/workspace-lint",
            prov: Some(WORKTREE_MARK),
            model: "fable",
            effort: "xh",
            battery: Some((4, 80_000)),
            subs: true,
            thinking: true,
            turn: "8m",
            elapsed: "1m",
            ..Card::default()
        },
        // The long-branch case: 23 cells, which the two-line card truncated.
        Card {
            chip: Some("RECUT"),
            chip_ink: 5,
            summary: "Recut, retag",
            branch: "statusline-battery",
            prov: Some(BRANCH_MARK),
            battery: Some((9, 211_000)),
            thinking: true,
            turn: "1m",
            elapsed: "20m",
            ..Card::default()
        },
        // Selected, with a PR and a long wants that must clamp.
        Card {
            mark: (dot, WORKING_INK),
            chip: Some("CLV-M2"),
            chip_ink: 4,
            summary: "Goal is shipping v0.2.2 cleanly",
            branch: "v022-prep",
            prov: Some(WORKTREE_MARK),
            pr: Some(225),
            effort: "xh",
            battery: Some((8, 130_000)),
            subs: true,
            thinking: true,
            turn: "42m",
            wants: "confirm the release cut before I tag",
            elapsed: "5m",
            selected: true,
            ..Card::default()
        },
        // No chip: the summary claims the pill's nine columns.
        Card {
            summary: "Rot reducer injection frequency analysis",
            repo: "rot-reducer",
            repo_ink: 4,
            battery: Some((9, 176_000)),
            thinking: true,
            turn: "2m",
            elapsed: "3h",
            ..Card::default()
        },
        // Done, OpenAI, no effort reading, no PR.
        Card {
            mark: (dot, DONE_INK),
            chip: Some("DJ"),
            chip_ink: 4,
            summary: "Consecutive artists in DJ set",
            repo: "beacon",
            repo_ink: 2,
            provider: Some("openai"),
            model: "gpt-5",
            effort: "",
            battery: Some((7, 169_000)),
            elapsed: "1d",
            ..Card::default()
        },
        // Repo name overflowing its budget, PR present, unknown provider.
        Card {
            mark: (dot, WORKING_INK),
            summary: "Landing page hero copy rewrite pass",
            repo: "clave-website",
            repo_ink: 6,
            branch: "hero-copy",
            prov: Some(BRANCH_MARK),
            pr: Some(12),
            provider: None,
            model: "",
            effort: "",
            battery: Some((3, 55_000)),
            elapsed: "30m",
            ..Card::default()
        },
        // Failed, high burn, worktree.
        Card {
            mark: ('\u{2716}', FAILED_INK),
            chip: Some("MIGRATE"),
            chip_ink: 3,
            summary: "Postgres 15 to 17 migration runbook",
            repo: "market-scanner",
            repo_ink: 1,
            branch: "pg17-migrate",
            prov: Some(WORKTREE_MARK),
            pr: Some(88),
            model: "sonnet",
            battery: Some((10, 201_000)),
            elapsed: "4h",
            ..Card::default()
        },
        // Dormant: absolute fade, no reading in flight.
        Card {
            mark: ('\u{25cb}', clave_bar::theme::DEFAULT_INK),
            chip: Some("FOOTER"),
            chip_ink: 0,
            summary: "ollie.gg company details footer",
            repo: "resumaker",
            repo_ink: 7,
            battery: Some((4, 73_000)),
            elapsed: "2w",
            dormant: true,
            ..Card::default()
        },
    ]
}

// ── the geometry ────────────────────────────────────────────────────────────

/// One card as four lines, each EXACTLY `cols` display cells.
fn render_card(
    c: &Card,
    cols: usize,
    profile: Profile,
    any_selected: bool,
    frame: usize,
) -> Vec<String> {
    let theme = Theme::default();
    let fade = if c.selected {
        0.0
    } else if c.dormant {
        DORMANT_FADE
    } else if !any_selected {
        0.0
    } else {
        FADE
    };
    let ink = |x: Rgb| x.mix(theme.base, fade);
    // Glass: `None` paints NOTHING and re-asserts the default background, so a
    // selection bar never bleeds into the glass after it.
    let row_bg: Option<Rgb> = c.selected.then_some(theme.sel_bg);
    let seg = |x: Rgb, s: &str| match row_bg {
        Some(b) => format!("{}{}{s}", b.bg(), x.fg()),
        None => format!("\u{1b}[49m{}{s}", x.fg()),
    };
    let repo_ink = ink(hue(c.repo_ink, &theme));
    // The rail IS the repo's colour, running the card's full height — one
    // continuous identity spine per card, which is what the retired zebra was
    // reaching for and could never be, since a neutral says nothing.
    let rail = repo_ink;
    let expanded = profile == Profile::Expanded;

    // ── line 1: mark │ chip-pill summary ──
    let mut l1 = String::new();
    let (mut mark, mark_ink) = c.mark;
    if c.thinking {
        mark = think_frame(frame);
    }
    l1.push_str(&seg(ink(mark_ink), &format!(" {mark} ")));
    l1.push_str(&seg(rail, &format!("{RULE} ")));
    if let Some(label) = c.chip {
        let bg = ink(hue(c.chip_ink, &theme));
        l1.push_str(&seg(bg, &LCAP.to_string()));
        l1.push_str(&format!(
            "{}{}{}{RESET}",
            bg.bg(),
            ink(theme.chip_ink).fg(),
            pad(label, CHIP_W)
        ));
        l1.push_str(&seg(bg, &RCAP.to_string()));
    }
    // Chrome + 9 pill + 1 gap + 1 margin. A chipless card hands the pill's nine
    // columns to the summary, exactly as the two-line card does — AND its gap,
    // so the summary starts flush at the indent with the repo and the token
    // count below it. The gap is the pill's trailing air, not an indent: with no
    // pill there is nothing to hold air away from, and the space read as a
    // one-cell stagger against every other line.
    let gap = if c.chip.is_some() { " " } else { "" };
    let summary_w = cols.saturating_sub(if c.chip.is_some() {
        CHROME + 11
    } else {
        CHROME + 1
    });
    // A cell with room for an ellipsis and nothing else says less than a blank
    // one: at 16 the pill IS the line, and a lone `…` after it is noise.
    let summary = if summary_w < 4 { "" } else { c.summary };
    l1.push_str(&seg(
        ink(theme.default_ink),
        &format!("{gap}{}", pad(summary, summary_w)),
    ));
    l1.push_str(&seg(theme.default_ink, " "));

    // ── line 2: prov │ repo [branch] provider model effort ──
    let mut l2 = String::new();
    let (prov, prov_ink) = match c.prov {
        Some(g) if g == WORKTREE_MARK => (g.to_string(), WORKTREE_INK),
        Some(g) => (g.to_string(), BRANCH_INK),
        None => (" ".to_string(), theme.default_ink),
    };
    l2.push_str(&seg(ink(prov_ink), &format!(" {prov} ")));
    l2.push_str(&seg(rail, &format!("{RULE} ")));
    // Expanded pays 2 + 7 + 3 for provider, model and effort; collapsed pays
    // none of them and hands every one of those columns to the repo.
    // Round 5: the PR comes back to line 2. It is IDENTITY — "this branch has
    // a PR open" — and on line 3 it sat among measurements it has nothing to
    // do with, punching a reserved-but-empty hole through the middle of them
    // on every row without one. A card with no PR folds those six cells back
    // into the repo-and-branch budget, so nothing is reserved for an absence.
    let pr = c.pr.map_or_else(String::new, |n| format!("#{n}"));
    let pr_w = if expanded && c.pr.is_some() {
        PR_W + 1
    } else {
        0
    };
    let tail_w = if expanded { 12 } else { 0 };
    let budget = cols.saturating_sub(INDENT + pr_w + tail_w + 1);
    // The branch renders only where it can carry its guaranteed minimum. A
    // five-cell fragment of `fix/evlog-…` identifies nothing, and those columns
    // are worth more to the repo name, which at least names the checkout.
    let branch_fits = budget >= display_cells(c.repo).min(REPO_W) + 1 + BRANCH_MIN;
    if c.branch.is_empty() || !branch_fits {
        l2.push_str(&seg(repo_ink, &pad(c.repo, budget)));
    } else {
        // The branch starts one space after the repo NAME and claims whatever
        // the repo did not use, so a long repo truncates before a branch does.
        let rw = display_cells(c.repo).min(REPO_W);
        l2.push_str(&seg(repo_ink, &pad(c.repo, rw)));
        l2.push_str(&seg(
            ink(META_INK),
            &format!(" {}", pad(c.branch, budget - rw - 1)),
        ));
    }
    if pr_w > 0 {
        l2.push_str(&seg(ink(PR_INK), &format!(" {}", pad(&pr, PR_W))));
    }
    if expanded {
        match c.provider.and_then(provider_mark) {
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

    // ── line 3: │ tokens elapsed wants … turn ──
    let mut l3 = String::new();
    // The third structural mark, stacked under status and provenance. A
    // BOOLEAN, not a count (round 4): "this row has fanned out" is the whole
    // signal, and a digit beside it was noise.
    let subs = if c.subs { SUBS_MARK } else { ' ' };
    l3.push_str(&seg(ink(SUBS_INK), &format!(" {subs} ")));
    l3.push_str(&seg(rail, &format!("{RULE} ")));
    let (tok_text, tok_ink) = match c.battery {
        Some((level, n)) => (
            token_text(n),
            BATTERY[usize::from(level).min(BATTERY.len() - 1)].1,
        ),
        None => (String::new(), theme.default_ink),
    };
    l3.push_str(&seg(ink(tok_ink), &pad(&tok_text, TOKEN_W)));
    if expanded {
        // Three measurements and one message. `wants` claims every cell the
        // numbers do not: 28 at 48 columns.
        let wants_w =
            cols.saturating_sub(INDENT + TOKEN_W + TOKEN_GAP + ELAPSED_W + 1 + 1 + TURN_W + 1);
        // The clocks swap (round 8). Elapsed goes LEFT, into the crop, and
        // the turn clock takes the right edge. The animated mark already says
        // "thinking", so how LONG it has been thinking is the softer fact;
        // time-since-you-touched-it is the harder one, because past an hour
        // the prompt cache is gone and the next turn costs more. The one that
        // changes what you do is the one that survives collapse.
        //
        // They stay APART either way: adjacent, `20h 20m` reads as one
        // duration — twenty hours and twenty minutes — not as two numbers.
        // Different inks do not save it; only distance does.
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
        // — how hot, and how stale — and stops. The margin is whatever is
        // left rather than a fixed cell, so the line lands on `cols` at any
        // collapsed width without a second constant to keep in step.
        // The time LOCKS to the right edge, one cell in — the same edge the
        // turn clock holds in the expanded card, which the crop takes away.
        // Whichever clock is rightmost sits in the same column, so the eye
        // finds a duration in one place in both profiles.
        let fill = cols.saturating_sub(display_cells(&strip_sgr(&l3)) + ELAPSED_W + 1);
        l3.push_str(&seg(theme.default_ink, &" ".repeat(fill)));
        l3.push_str(&seg(ink(META_INK), &rpad(c.elapsed, ELAPSED_W)));
        l3.push_str(&seg(theme.default_ink, " "));
    }

    // ── line 4: the shadow rule ──
    // Full width and OUTSIDE the selection bar (round 2's ruling): the
    // separator rhythm stays unbroken across the whole fleet, and the
    // selection is exactly the three lines that carry content.
    let l4 = format!(
        "\u{1b}[49m{}{}{RESET}",
        SHADOW_INK.fg(),
        SHADOW.to_string().repeat(cols)
    );

    vec![l1, l2, l3, l4]
        .into_iter()
        .map(|l| clip(&l, cols))
        .collect()
}

fn provider_mark(p: &str) -> Option<(char, Rgb)> {
    match p {
        "claude" => Some((CLAUDE_GLYPH, CLAUDE_INK)),
        "openai" => Some((OPENAI_GLYPH, OPENAI_INK)),
        _ => None,
    }
}

fn hue(i: u8, theme: &Theme) -> Rgb {
    theme
        .palette
        .get(usize::from(i))
        .copied()
        .unwrap_or(theme.untinted)
}

/// A local copy of the bar's own thousands/millions formatter — the design
/// round does not widen production visibility to draw a picture.
fn token_text(tokens: u32) -> String {
    let tokens = u64::from(tokens);
    let thousands = (tokens + 500) / 1_000;
    if thousands < 1_000 {
        return format!("{thousands}k");
    }
    let tenths = (tokens + 50_000) / 100_000;
    let (millions, tenth) = (tenths / 10, tenths % 10);
    if millions < 10 && tenth != 0 {
        return format!("{millions}.{tenth}m");
    }
    format!("{}m", millions.min(999))
}

fn pad(s: &str, w: usize) -> String {
    let n = display_cells(s);
    if n == w {
        return s.to_string();
    }
    if n < w {
        return format!("{s}{}", " ".repeat(w - n));
    }
    if w == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = display_cells(&ch.to_string());
        if used + cw > w - 1 {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push(ELLIPSIS);
    used += 1;
    format!("{out}{}", " ".repeat(w - used))
}

fn rpad(s: &str, w: usize) -> String {
    let n = display_cells(s);
    if n >= w {
        return pad(s, w);
    }
    format!("{}{s}", " ".repeat(w - n))
}

/// Truncate to exactly `cols` display cells and close the SGR state.
fn clip(line: &str, cols: usize) -> String {
    let bare = strip_sgr(line);
    if display_cells(&bare) <= cols {
        return format!("{line}{RESET}");
    }
    let mut out = String::new();
    let mut used = 0;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            out.push(ch);
            for c2 in chars.by_ref() {
                out.push(c2);
                if c2 == 'm' {
                    break;
                }
            }
            continue;
        }
        let cw = display_cells(&ch.to_string());
        if used + cw > cols {
            break;
        }
        out.push(ch);
        used += cw;
    }
    format!("{out}{RESET}")
}

/// Seconds a frame holds, overridable: `--animate 0.2`. Not one second — at a
/// second a card stutters rather than breathes.
///
/// The CEILING is not performance, it is the bar's timer classifier: the plugin
/// works out which of its timers fired from how many seconds elapsed, with
/// bands at 0.15 (width cooldown), 1.0 (peek sink) and 3.0 (terminal poll),
/// guarded by a compile-time assert chain. An animation tick must sit strictly
/// inside one of the gaps or it gets read as another timer's expiry. 0.2s — five
/// frames a second, near Claude's own — is available without touching any of
/// that. Faster means replacing the classifier with properly tagged timers,
/// which is a real change with a real regression behind it.
const DEFAULT_FRAME_SECS: f64 = 0.2;

/// The whole fleet, redrawn in place until Ctrl-C. The ONLY way to judge an
/// animation; a filmstrip shows the glyphs but not whether the motion reads as
/// thinking or as noise.
fn animate(cards: &[Card], any_selected: bool, frame_secs: f64) {
    print!("\u{1b}[?25l"); // hide the cursor
    for frame in 0.. {
        print!("\u{1b}[H\u{1b}[2J");
        println!(
            "{}THINKING — {frame_secs}s a frame, Ctrl-C to stop{RESET}\n",
            DIM.fg()
        );
        for c in cards {
            for line in render_card(c, 48, Profile::Expanded, any_selected, frame) {
                println!("{line}");
            }
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(frame_secs));
    }
}

fn main() {
    let cards = fleet();
    let any_selected = cards.iter().any(|c| c.selected);
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--animate") {
        let secs = args
            .get(i + 1)
            .and_then(|a| a.parse().ok())
            .unwrap_or(DEFAULT_FRAME_SECS);
        animate(&cards, any_selected, secs);
        return;
    }
    let frame = 0;
    let profiles = [
        ("EXPANDED — 48 columns", 48, Profile::Expanded),
        ("COLLAPSED — 16 columns", 16, Profile::Collapsed),
    ];
    for (name, cols, profile) in profiles {
        println!("\n{}{name}{RESET}\n", DIM.fg());
        for (i, c) in cards.iter().enumerate() {
            for (j, line) in render_card(c, cols, profile, any_selected, frame)
                .iter()
                .enumerate()
            {
                let w = display_cells(&strip_sgr(line));
                assert_eq!(
                    w, cols,
                    "{name}: card {i} line {j} is {w} cells, want {cols}"
                );
                println!("{line}");
            }
        }
    }

    // ── the thinking filmstrip ──
    // The ten frames laid out flat, so a missing glyph shows up as tofu here
    // rather than as a flicker in the animation.
    println!(
        "\n{}THINKING — the ten frames, then `--animate` to see them move{RESET}\n",
        DIM.fg()
    );
    let strip: String = (0..THINK_CYCLE)
        .map(|t| format!(" {} ", think_frame(t)))
        .collect();
    println!("  {}{strip}{RESET}\n", clave_bar::theme::WORKING_INK.fg());
}
