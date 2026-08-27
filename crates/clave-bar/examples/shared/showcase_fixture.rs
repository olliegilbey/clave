//! The showcase fleet — the README's hero frame, shared between `bar-preview`
//! (prints it as ANSI for eyeballing) and `readme-assets` (renders it as the
//! committed SVG). One fixture, two consumers, so the frame the README shows
//! and the frame the preview prints can never diverge.
//!
//! GLYPH RULE — load-bearing (lock §5.4): every glyph is a `\u{...}` escape,
//! never a literal character. See `bar-preview.rs` for the incident history.

use clave_bar::render::{Provenance, Row, RowContent, RowStatus, TermStatus};

/// `battery` is the ramp level AND the count it was bucketed from, together, the
/// way a real snapshot carries them (#105): the expanded profile prints the
/// figure and inks it with the level's band, so a fixture that set them
/// independently could show a preview no live row could ever produce.
pub fn agent(
    status: RowStatus,
    battery: Option<(u8, u32)>,
    provenance: Provenance,
    repo: &str,
    repo_ink: u8,
    title: Option<(&str, u8)>,
    summary: &str,
) -> Row {
    Row {
        content: RowContent::Agent {
            status,
            battery: battery.map(|(level, _)| level),
            tokens: battery.map(|(_, tokens)| tokens),
            provenance,
            title: title.map(|(t, _)| String::from(t)),
            title_ink: title.map(|(_, i)| i),
            repo: String::from(repo),
            repo_ink: Some(repo_ink),
            summary: String::from(summary),
            model: None,
            provider: None,
            pr: None,
            branch: String::new(),
            elapsed: None,
        },
        selected: false,
        dormant: matches!(status, RowStatus::Dormant | RowStatus::DormantSelected),
    }
}

/// A terminal row with its pane facts filled in (#206): the tab name is the
/// chip, the cwd's directory name rides the repo ink allocation, and the
/// summary carries the focused pane's most recent foreground command.
/// `RowContent::terminal(name)` remains the nothing-known-yet default.
pub fn terminal(
    name: &str,
    status: TermStatus,
    provenance: Provenance,
    repo: Option<(&str, u8)>,
    command: &str,
) -> Row {
    Row {
        content: RowContent::Terminal {
            name: String::from(name),
            status,
            provenance,
            repo: repo.map(|(r, _)| String::from(r)),
            repo_ink: repo.map(|(_, i)| i),
            command: String::from(command),
            pr: None,
            branch: String::new(),
            elapsed: None,
        },
        selected: false,
        dormant: false,
    }
}

/// The card-only cells (#232): model, provider, PR, branch and elapsed. The
/// single-line row ignores every one of them and the two-line card draws them
/// all, so one fixture feeds both renderers and the README's card frames are
/// full rather than half-blank. A terminal row borrows the checkout's PR,
/// branch and elapsed, and has no model or provider of its own.
pub fn detail(
    mut row: Row,
    model: &str,
    provider: &str,
    pr: Option<u32>,
    branch: &str,
    elapsed: &str,
) -> Row {
    match &mut row.content {
        RowContent::Agent {
            model: m,
            provider: p,
            pr: n,
            branch: b,
            elapsed: e,
            ..
        } => {
            *m = Some(String::from(model));
            *p = Some(String::from(provider));
            *n = pr;
            *b = String::from(branch);
            *e = Some(String::from(elapsed));
        }
        RowContent::Terminal {
            pr: n,
            branch: b,
            elapsed: e,
            ..
        } => {
            *n = pr;
            *b = String::from(branch);
            *e = Some(String::from(elapsed));
        }
    }
    row
}

/// One repo is one ink forever (lock §4); these indices stand in for the
/// store-backed round-robin allocation that will assign them for real.
pub const CLAVE: u8 = 0;
pub const DOTFILES: u8 = 1;
pub const API_SVC: u8 = 2;
pub const INFRA: u8 = 4;
pub const WEBAPP: u8 = 5;

/// The screenshot fleet (`--showcase`): the row vocabulary in one frame, with
/// NO preview chrome — no ruler, no border, no column map — so a screenshot of
/// this is a screenshot of the bar rather than of the graph paper it is drawn
/// on. Used for the README until a real capture replaces it.
///
/// The battery levels span the ramp on purpose (S7, #62): the fleet shows a
/// fresh row, a couple past halfway, one nearly out and one past its smart zone,
/// because a promotional image of a meter that reads the same on every row sells
/// the column short. Each count is a plausible tenth of the default 150k zone
/// for the level beside it (#105) — and the row past its zone reads `412k`
/// against a glyph that has run out of levels, which is the case the count
/// exists for. The dormant row carries a real level too — a dormant conversation
/// consumes nothing, so its reading is exactly current, which is the ruling that
/// closed design-lock §7.2.
///
/// The summaries copy the SHAPE of real `ai-title` values, sampled from the
/// local transcript corpus 2026-07-31: sentence case, verb first, no trailing
/// period, 13-60 characters. They are invented rather than copied — the corpus
/// is the maintainer's own work and this repo is public — but a fixture that
/// invents the format too would misrepresent the column in the one image most
/// people will ever see. Several run past the 22-cell summary column and
/// truncate, which is the honest common case.
pub fn showcase() -> Vec<Row> {
    let mut rows = vec![
        detail(
            agent(
                RowStatus::NeedsYou,
                Some((3, 52_000)),
                Provenance::Branch,
                "api-svc",
                API_SVC,
                Some(("AUTH-7", 3)),
                "Rotate the signing keys",
            ),
            "opus",
            "claude",
            Some(184),
            "key-rotation",
            "4m",
        ),
        detail(
            agent(
                RowStatus::Working,
                Some((7, 108_000)),
                Provenance::Worktree,
                "clave",
                CLAVE,
                Some(("S6-GUT", 5)),
                "Wire the status column into render_rows",
            ),
            "sonnet",
            "claude",
            Some(232),
            "double-rows",
            "12m",
        ),
        detail(
            agent(
                RowStatus::Done,
                Some((1, 18_000)),
                Provenance::Main,
                "webapp",
                WEBAPP,
                Some(("CART-99", 6)),
                "Fix cart total rounding mismatch",
            ),
            "haiku",
            "claude",
            None,
            "",
            "1h",
        ),
        detail(
            terminal(
                "shell",
                TermStatus::Running,
                Provenance::Worktree,
                Some(("clave", CLAVE)),
                "just gates",
            ),
            "",
            "",
            Some(232),
            "double-rows",
            "2m",
        ),
        detail(
            agent(
                RowStatus::Idle,
                Some((9, 141_000)),
                Provenance::Main,
                "clave",
                CLAVE,
                None,
                "Review the spawn identity gate",
            ),
            "opus",
            "claude",
            None,
            "",
            "25m",
        ),
        detail(
            agent(
                RowStatus::Failed,
                Some((5, 79_000)),
                Provenance::Branch,
                "infra",
                INFRA,
                Some(("DNS-TTL", 1)),
                "Debug staging rollout DNS timeout",
            ),
            "gpt-5",
            "openai",
            Some(77),
            "dns-timeout",
            "3h",
        ),
        detail(
            terminal(
                "logs",
                TermStatus::Failed,
                Provenance::Main,
                Some(("infra", INFRA)),
                "kubectl logs -f api-7d9",
            ),
            "",
            "",
            None,
            "",
            "8m",
        ),
        detail(
            agent(
                RowStatus::Stale,
                Some((10, 412_000)),
                Provenance::Worktree,
                "clave",
                CLAVE,
                Some(("KDL-GRD", 7)),
                "Validate generated KDL artifacts",
            ),
            "sonnet",
            "claude",
            Some(219),
            "kdl-guard",
            "1d",
        ),
        detail(
            agent(
                RowStatus::Dormant,
                Some((6, 93_000)),
                Provenance::Main,
                "notes",
                DOTFILES,
                Some(("ZSH", 2)),
                "Tidy the shell startup files",
            ),
            "haiku",
            "claude",
            None,
            "",
            "2w",
        ),
    ];
    rows[1].selected = true;
    rows
}
