//! clave sidebar — DOUBLE-HEIGHT row candidates, rendered for screenshots.
//!
//!     cargo run -p clave-bar --example double-preview
//!
//! NOT the locked design and NOT `render_rows` — this is the candidate
//! explorer for the two-line row (session DOUBLE, 2026-08-26+), the same role
//! the D17 width candidates played. It borrows the ratified colour and glyph
//! machinery from `render.rs` so the candidates look like clave, but the
//! geometry here is proposal, not authority. If a candidate is ratified, the
//! layout moves into `render_rows` and this file becomes the preview of the
//! new lock.
//!
//! Round 9: the 38-col card is ratified as the COLLAPSED profile, and this
//! file now renders the EXPANDED candidate beside it for comparison — same
//! card, +10 columns: a branch beside the repo on line 2 (blank on a
//! default checkout, same "blank is the meaning" rule as the prov glyph)
//! and a wider summary flex on line 1. Nothing else moves. Round 9b: repo
//! and branch share one collective budget — the branch sits one space after
//! the repo NAME (not its padded cell) and takes the leftover columns, with
//! a guaranteed minimum; a long repo truncates before a branch does.
//! RATIFIED 2026-08-26 ("that's the one"): both profiles are now final —
//! this file is the visual spec for #232 in its entirety.
//!
//! Round 6 state: the layout is settled — rounded-corner bracket ╭╰,
//! two-line card, full glass (no painted backgrounds but the selection),
//! token count top-right of line 1 in ramp ink:
//!   line 1:  status ╭ chip-pill  title/summary     tokens
//!   line 2:  prov   ╰ repo  #PR  icon  model      elapsed
//! Ink placement is settled (round 6's O): brackets alternate two neutral
//! inks per card, provenance carries the repo ink. The joiner is the light
//! arc (round 7). Round 8 adds the space rules: a row with NO renamed title
//! drops the pill and lets the summary claim its columns, and terminal tabs
//! are pills too — dark bg where the title chips are coloured. Earlier
//! rounds are retired; git history holds them.
//!
//! Background: Ollie's terminal renders painted cell backgrounds OPAQUE
//! (probe, 2026-08-28) — so translucency ("glass") exists only where NO
//! background is painted. Per-cell opacity is not expressible in ANSI at all:
//! a cell is either default-bg (glass) or a concrete RGB (opaque). The zebra
//! variant paints every second card and leaves the others glass — the closest
//! the protocol gets to "two opacities".
//!
//! GLYPH RULE — every glyph is a `\u{...}` escape, never a literal (lock §5.4).

use clave_bar::render::{
    BASE, CHIP_INK, CONSOLE, DEFAULT_INK, DORMANT_FADE, PALETTE, RESET, Rgb, RowStatus,
    display_cells, strip_sgr,
};
use clave_bar::theme::Theme;

// Mirrors of render.rs internals a candidate needs but the lock keeps private.
const SEL_BG: Rgb = Rgb(0x2D, 0x4F, 0x67); // waveBlue2 — the selected row
const FADE: f64 = 0.25; // unselected recession (lock §6)
const LCAP: char = '\u{e0b6}';
const RCAP: char = '\u{e0b4}';
const ELLIPSIS: char = '\u{2026}';

// The new cells.
const CLAUDE_GLYPH: char = '\u{ec82}'; // nf-cod-claude
const OPENAI_GLYPH: char = '\u{ec81}'; // nf-cod-openai
const CLAUDE_INK: Rgb = Rgb(0xD9, 0x77, 0x57); // Anthropic coral
const OPENAI_INK: Rgb = Rgb(0x10, 0xA3, 0x7F); // OpenAI green
const META_INK: Rgb = Rgb(0x72, 0x71, 0x69); // fujiGray — model name, elapsed
const PR_INK: Rgb = Rgb(0x98, 0xBB, 0x6C); // springGreen — PR number
const BRANCH: char = '\u{f062c}'; // nf-md-source_branch
const WORKTREE: char = '\u{168c2}'; // bamum tree (the invented worktree mark)

const CHIP_W: usize = 7;
// 9 plus a leading space (round 8d; was 6, then 12, then 10): the repo text
// starts one cell in, vertically aligned with the pill LABEL on line 1
// (both start at column 7). The repo cell still absorbs line 2's slack so
// the PR number starts late and the model→elapsed gap stays closed.
const REPO_W: usize = 9;
const MODEL_W: usize = 6;
// The expanded profile's branch MINIMUM (round 9b): repo and branch share
// a collective REPO_W + 1 + BRANCH_W budget, branch taking whatever the
// repo name leaves, never less than this. 0 = collapsed, no branch.
const BRANCH_W: usize = 9;

#[derive(Clone, Copy)]
enum Provider {
    Claude,
    OpenAi,
}

impl Provider {
    fn mark(self) -> (char, Rgb) {
        match self {
            Provider::Claude => (CLAUDE_GLYPH, CLAUDE_INK),
            Provider::OpenAi => (OPENAI_GLYPH, OPENAI_INK),
        }
    }
}

#[derive(Clone, Copy)]
enum Prov {
    Main,
    Branch,
    Worktree,
}

impl Prov {
    fn mark(self) -> Option<char> {
        match self {
            Prov::Main => None,
            Prov::Branch => Some(BRANCH),
            Prov::Worktree => Some(WORKTREE),
        }
    }
}

struct R {
    status: RowStatus,
    prov: Prov,
    /// label + palette index; `None` bg means the TERM chip (dark on default).
    chip: Option<(&'static str, Option<usize>)>,
    repo: &'static str,
    repo_ink: usize,
    /// The checkout's branch — expanded profile only; "" on a default
    /// checkout (blank is the meaning, like the prov glyph).
    branch: &'static str,
    pr: Option<u32>,
    provider: Option<Provider>,
    model: &'static str,
    /// `None` renders the TERM tag in the token cell (current design language).
    tokens: Option<&'static str>,
    elapsed: &'static str,
    summary: &'static str,
    selected: bool,
    dormant: bool,
}

/// Round 7: O is ratified-in-waiting — alternating neutral brackets, repo
/// ink on provenance, full glass. The last hunt is the joiner GLYPH: every
/// stackable top/bottom pair Unicode offers, rendered side by side.
struct FV {
    name: &'static str,
    cols: usize,
    top: char,
    bot: char,
    /// Branch cell width on line 2; 0 renders the collapsed card (no cell).
    branch_w: usize,
}

/// The bracket's alternating pair — two quiet kanagawa inks, cool and warm,
/// close enough in weight that neither reads as a state.
const BRACKET_A: Rgb = Rgb(0x72, 0x71, 0x69); // fujiGray
const BRACKET_B: Rgb = Rgb(0x9C, 0xAB, 0xCA); // springViolet2

fn pad(s: &str, w: usize) -> String {
    let cells = display_cells(s);
    if cells > w {
        let mut out = clave_bar::render::cell_slice(s, 0, w.saturating_sub(1));
        out.push(ELLIPSIS);
        out
    } else {
        format!("{s}{}", " ".repeat(w - cells))
    }
}

fn rpad(s: &str, w: usize) -> String {
    let cells = display_cells(s);
    if cells >= w {
        pad(s, w)
    } else {
        format!("{}{s}", " ".repeat(w - cells))
    }
}

/// Token-count ink by the ramp's risk bands, approximated for the mock.
fn tok_ink(t: &str) -> Rgb {
    let k: u32 = t.trim_end_matches('k').parse().unwrap_or(0);
    match k {
        0..=79 => Rgb(0x98, 0xBB, 0x6C),    // springGreen
        80..=129 => Rgb(0xE6, 0xC3, 0x84),  // carpYellow
        130..=169 => Rgb(0xFF, 0xA0, 0x66), // surimiOrange
        _ => Rgb(0xE4, 0x68, 0x76),         // waveRed
    }
}

fn render_f_pair(r: &R, v: &FV, zebra_paint: bool) -> (String, String) {
    // Glass rule: `None` paints NOTHING — the terminal's own (translucent)
    // background shows through, with `49m` (default bg) re-asserted so the
    // selection bar never bleeds into the glass after it. Selection stays a
    // full opaque bar.
    let row_bg: Option<Rgb> = if r.selected { Some(SEL_BG) } else { None };
    let mix_to = row_bg.unwrap_or(BASE);
    let ink = |c: Rgb| -> Rgb {
        if r.dormant {
            c.mix(mix_to, DORMANT_FADE)
        } else if !r.selected {
            c.mix(mix_to, FADE)
        } else {
            c
        }
    };
    let paint = |bg: Option<Rgb>, c: Rgb, s: &str| match bg {
        Some(b) => format!("{}{}{s}", b.bg(), c.fg(), s = s),
        None => format!("\u{1b}[49m{}{s}", c.fg(), s = s),
    };
    let seg = |c: Rgb, s: &str| paint(row_bg, c, s);
    // O's inking, now baked in: the bracket alternates between two neutrals
    // (the zebra lives in the linework), and provenance carries the repo ink.
    let bracket_ink = ink(if zebra_paint { BRACKET_B } else { BRACKET_A });
    let prov_ink = ink(PALETTE[r.repo_ink].0);
    let terminal = r.tokens.is_none();

    // ── line 1: status ╭ chip-pill title [tokens?] ──
    let (mark, mark_ink) = if terminal {
        (CONSOLE, DEFAULT_INK)
    } else {
        r.status.mark(&Theme::default())
    };
    let mut l1 = String::new();
    l1.push_str(&seg(ink(mark_ink), &format!(" {mark} ")));
    l1.push_str(&seg(bracket_ink, &format!("{} ", v.top)));
    match r.chip {
        Some((label, Some(pal))) => {
            let chip_bg = ink(PALETTE[pal].0);
            l1.push_str(&seg(chip_bg, &LCAP.to_string()));
            l1.push_str(&format!(
                "{}{}{}{RESET}",
                chip_bg.bg(),
                ink(CHIP_INK).fg(),
                pad(label, CHIP_W)
            ));
            l1.push_str(&seg(chip_bg, &RCAP.to_string()));
        }
        // The TERM pill: same pill shape as the title chips, dark bg instead
        // of a palette colour (round 8, Ollie's mock).
        Some((label, None)) => {
            l1.push_str(&seg(ink(CHIP_INK), &LCAP.to_string()));
            l1.push_str(&format!(
                "{}{}{}{RESET}",
                CHIP_INK.bg(),
                ink(DEFAULT_INK).fg(),
                pad(label, CHIP_W)
            ));
            l1.push_str(&seg(ink(CHIP_INK), &RCAP.to_string()));
        }
        // No renamed title: no pill at all — the summary claims its columns
        // (round 8): the text starts at the bracket and runs the full flex.
        None => {}
    }
    let flex = if r.chip.is_some() {
        v.cols - 21
    } else {
        v.cols - 12
    };
    l1.push_str(&seg(
        ink(DEFAULT_INK),
        &format!(" {}", pad(r.summary, flex)),
    ));
    match r.tokens {
        Some(t) => l1.push_str(&seg(ink(tok_ink(t)), &format!(" {}", rpad(t, 4)))),
        None => l1.push_str(&seg(ink(META_INK), &format!(" {}", rpad("TERM", 4)))),
    }
    l1.push_str(&seg(DEFAULT_INK, " "));

    // ── line 2: prov ╰ repo #PR [tokens?] icon model … elapsed [tokens?] ──
    let mut l2 = String::new();
    let prov = r.prov.mark().map(|g| g.to_string()).unwrap_or(" ".into());
    l2.push_str(&seg(prov_ink, &format!(" {prov} ")));
    l2.push_str(&seg(bracket_ink, &format!("{} ", v.bot)));
    // Round 9, expanded only: repo and branch share ONE collective budget
    // (round 9b) — the branch starts right after the repo name and claims
    // every column the repo doesn't use, in meta ink so the repo's palette
    // ink keeps carrying the identity. Branch names run longer than repo
    // names, so the branch is guaranteed its `branch_w` minimum: a long
    // repo truncates first. The PR column never moves.
    if v.branch_w == 0 {
        l2.push_str(&seg(
            ink(PALETTE[r.repo_ink].0),
            &format!(" {}", pad(r.repo, REPO_W)),
        ));
    } else {
        let total = REPO_W + 1 + v.branch_w;
        if r.branch.is_empty() {
            l2.push_str(&seg(
                ink(PALETTE[r.repo_ink].0),
                &format!(" {}", pad(r.repo, total)),
            ));
        } else {
            let repo_w = display_cells(r.repo).min(total - v.branch_w - 1);
            l2.push_str(&seg(
                ink(PALETTE[r.repo_ink].0),
                &format!(" {}", pad(r.repo, repo_w)),
            ));
            l2.push_str(&seg(
                ink(META_INK),
                &format!(" {}", pad(r.branch, total - repo_w - 1)),
            ));
        }
    }
    let pr = r.pr.map(|n| format!("#{n}")).unwrap_or_default();
    l2.push_str(&seg(ink(PR_INK), &format!(" {}", pad(&pr, 5))));
    match r.provider {
        // Two spaces before the icon (round 8c): the extra column between
        // the PR number and the model name.
        Some(p) => {
            let (g, c) = p.mark();
            l2.push_str(&seg(ink(c), &format!("  {g}")));
        }
        None => l2.push_str(&seg(DEFAULT_INK, "   ")),
    }
    l2.push_str(&seg(ink(META_INK), &format!(" {}", pad(r.model, MODEL_W))));
    let fill = v
        .cols
        .saturating_sub(display_cells(&strip_sgr(&l2)) + 3 + 1);
    l2.push_str(&seg(DEFAULT_INK, &" ".repeat(fill)));
    l2.push_str(&seg(ink(META_INK), &rpad(r.elapsed, 3)));
    l2.push_str(&seg(DEFAULT_INK, " "));

    (format!("{l1}{RESET}"), format!("{l2}{RESET}"))
}

fn fleet() -> Vec<R> {
    use RowStatus::*;
    let a = |status,
             prov,
             chip,
             pal,
             repo,
             repo_ink,
             branch,
             pr,
             provider,
             model,
             tokens,
             elapsed,
             summary| {
        R {
            status,
            prov,
            chip: Some((chip, pal)),
            repo,
            repo_ink,
            branch,
            pr,
            provider: Some(provider),
            model,
            tokens: Some(tokens),
            elapsed,
            summary,
            selected: false,
            dormant: false,
        }
    };
    let mut rows = vec![
        a(
            NeedsYou,
            Prov::Main,
            "CORTI2",
            Some(2),
            "hermes",
            2,
            "",
            None,
            Provider::Claude,
            "fable",
            "105k",
            "3m",
            "Qdos IR35 assessment: the contract",
        ),
        a(
            Working,
            Prov::Main,
            "HERMES",
            Some(6),
            "hermes",
            2,
            "",
            None,
            Provider::Claude,
            "opus",
            "78k",
            "18m",
            "Personal reflections and planning",
        ),
        a(
            Working,
            Prov::Main,
            "XPS",
            Some(1),
            "hermes",
            2,
            "",
            None,
            Provider::Claude,
            "fable",
            "234k",
            "1h",
            "XPS dev server setup and deploy",
        ),
        a(
            NeedsYou,
            Prov::Main,
            "REASSOC",
            Some(3),
            "clave",
            0,
            "",
            None,
            Provider::Claude,
            "fable",
            "86k",
            "12m",
            "Clave session reassociation pass",
        ),
        a(
            Working,
            Prov::Worktree,
            "CLV-3",
            Some(3),
            "clave",
            0,
            "drive-launch",
            Some(204),
            Provider::Claude,
            "sonnet",
            "117k",
            "45m",
            "Drive launch",
        ),
        a(
            Working,
            Prov::Worktree,
            "COLOUR",
            Some(5),
            "clave",
            0,
            "colour",
            None,
            Provider::Claude,
            "fable",
            "79k",
            "2h",
            "Zellij theme passthrough spike",
        ),
        R {
            status: Idle,
            prov: Prov::Main,
            chip: Some(("Tab #12", None)),
            repo: "clave",
            repo_ink: 0,
            branch: "",
            pr: None,
            provider: None,
            model: "",
            tokens: None,
            elapsed: "7m",
            summary: "zsh",
            selected: false,
            dormant: false,
        },
        a(
            Working,
            Prov::Worktree,
            "CLV-M2",
            Some(4),
            "clave",
            0,
            "v022-prep",
            Some(225),
            Provider::Claude,
            "fable",
            "130k",
            "5m",
            "Goal is shipping v0.2.2 cleanly",
        ),
        a(
            Done,
            Prov::Main,
            "DJ",
            Some(4),
            "hermes",
            2,
            "",
            None,
            Provider::OpenAi,
            "gpt-5",
            "78k",
            "3h",
            "DJ queue setup",
        ),
        // A row with no renamed title: the ai-summary fills the pill's
        // columns too (round 8).
        R {
            status: Working,
            prov: Prov::Main,
            chip: None,
            repo: "hermes",
            repo_ink: 2,
            branch: "",
            pr: None,
            provider: Some(Provider::Claude),
            model: "opus",
            tokens: Some("34k"),
            elapsed: "2h",
            summary: "Create close conversation summary flow",
            selected: false,
            dormant: false,
        },
        a(
            Done,
            Prov::Branch,
            "GTMSS",
            Some(0),
            "nalu",
            5,
            "gtm-pass",
            Some(31),
            Provider::Claude,
            "haiku",
            "119k",
            "1d",
            "GTM Landscape - and first pass",
        ),
        // ── coverage rows (round 8e): the corners of the variant space ──
        // Terminal on a BRANCH with a PR.
        R {
            status: Idle,
            prov: Prov::Branch,
            chip: Some(("Tab #3", None)),
            repo: "clave",
            repo_ink: 0,
            branch: "double-rows",
            pr: Some(232),
            provider: None,
            model: "",
            tokens: None,
            elapsed: "3m",
            summary: "gh pr checks --watch",
            selected: false,
            dormant: false,
        },
        // Terminal in a WORKTREE, repo name filling the 9-cell budget exactly.
        R {
            status: Idle,
            prov: Prov::Worktree,
            chip: Some(("Tab #7", None)),
            repo: "resumaker",
            repo_ink: 7,
            branch: "resume-fix",
            pr: None,
            provider: None,
            model: "",
            tokens: None,
            elapsed: "1h",
            summary: "just dev",
            selected: false,
            dormant: false,
        },
        // Chipless + branch + PR + OpenAI, repo name OVERFLOWING to ellipsis.
        R {
            status: Working,
            prov: Prov::Branch,
            chip: None,
            repo: "clave-website",
            repo_ink: 6,
            branch: "hero-copy",
            pr: Some(12),
            provider: Some(Provider::OpenAi),
            model: "gpt-5",
            tokens: Some("55k"),
            elapsed: "30m",
            summary: "Landing page hero copy rewrite pass",
            selected: false,
            dormant: false,
        },
        // FAILED agent in a worktree with a PR, long repo, high burn.
        a(
            Failed,
            Prov::Worktree,
            "MIGRATE",
            Some(3),
            "market-scanner",
            1,
            "pg17-migrate",
            Some(88),
            Provider::Claude,
            "sonnet",
            "201k",
            "4h",
            "Postgres 15 to 17 migration runbook",
        ),
    ];
    rows[7].selected = true;
    let mut dormant = a(
        Dormant,
        Prov::Main,
        "FOOTER",
        Some(0),
        "resumaker",
        7,
        "",
        None,
        Provider::Claude,
        "opus",
        "73k",
        "2w",
        "ollie.gg company details footer",
    );
    dormant.dormant = true;
    rows.push(dormant);
    rows
}

fn main() {
    let dim = Rgb(0x71, 0x7C, 0x7C);
    let variants = [
        FV {
            name: "COLLAPSED — the ratified 38-col card",
            cols: 38,
            top: '\u{256d}',
            bot: '\u{2570}',
            branch_w: 0,
        },
        FV {
            name: "EXPANDED — +10: branch beside repo, wider summary",
            cols: 48,
            top: '\u{256d}',
            bot: '\u{2570}',
            branch_w: BRANCH_W,
        },
    ];
    for v in &variants {
        println!("\n{}{}{RESET}\n", dim.fg(), v.name);
        for (i, r) in fleet().iter().enumerate() {
            let (l1, l2) = render_f_pair(r, v, i % 2 == 1);
            for l in [&l1, &l2] {
                let w = display_cells(&strip_sgr(l));
                assert_eq!(
                    w, v.cols,
                    "{}: row {i} rendered {w} cells, want {}",
                    v.name, v.cols
                );
            }
            println!("{l1}\n{l2}");
        }
    }
    println!();
}
