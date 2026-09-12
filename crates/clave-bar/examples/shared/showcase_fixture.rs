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
            effort: None,
            pr: None,
            branch: String::new(),
            elapsed: None,
            wants: None,
            subagents: false,
        },
        selected: false,
        dormant: matches!(status, RowStatus::Dormant | RowStatus::DormantSelected),
    }
}

/// Fill the `wants` cell. Only ever applied to a `NeedsYou` row, because that
/// is the rule the host enforces (lock 4.7): structure decides whether a row
/// is waiting, and the words only say what for. A fixture that filled this on
/// a working row would show a card the product cannot produce.
pub fn blocked_on(mut row: Row, ask: &str) -> Row {
    if let RowContent::Agent { status, wants, .. } = &mut row.content {
        assert!(
            matches!(status, RowStatus::NeedsYou),
            "only a flagged row wants anything"
        );
        *wants = Some(String::from(ask));
    }
    row
}

/// Mark the row as having agents still running under it. A boolean, not a
/// count: the mark says "this row has fanned out" and nothing more.
pub fn fanned_out(mut row: Row) -> Row {
    if let RowContent::Agent { subagents, .. } = &mut row.content {
        *subagents = true;
    }
    row
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

/// The card-only cells (#232): model, effort, provider, PR, branch and
/// elapsed. The single-line row ignores every one of them and the two-line
/// card draws them all, so one fixture feeds both renderers and the README's
/// card frames are full rather than half-blank. A terminal row borrows the
/// checkout's PR, branch and elapsed, and has no model, effort or provider of
/// its own. An empty `effort` is no reading — the cell a provider that never
/// reports one renders blank.
pub fn detail(
    mut row: Row,
    model: &str,
    effort: &str,
    provider: &str,
    pr: Option<u32>,
    branch: &str,
    elapsed: &str,
) -> Row {
    match &mut row.content {
        RowContent::Agent {
            model: m,
            effort: f,
            provider: p,
            pr: n,
            branch: b,
            elapsed: e,
            ..
        } => {
            *m = Some(String::from(model));
            *f = (!effort.is_empty()).then(|| String::from(effort));
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
/// on.
///
/// SIX rows, not nine (Ollie, 2026-09-12): three live, one terminal, two
/// dormant says everything the vocabulary can say, and a taller frame only
/// costs the reader vertical scroll on the page where clave is introduced.
/// Every permutation that matters survives the cut — a worktree and a branch
/// and two plain checkouts, both providers, a chip and a blank one, the
/// subagent mark, and one row waiting on a person.
///
/// Two states are deliberately ABSENT. `Failed` is unreachable in the field
/// (#157: an API error leaves the row amber), and its heavy cross had never
/// appeared in any real session — an image is the wrong place to promise a
/// state the product cannot produce. `Stale` went with the row that carried
/// it, which was also the only reading past the smart zone; nothing in the
/// frame now reads above 141k, because a promotional frame showing a
/// conversation four times past its useful length sells the meter, not the
/// product.
///
/// The battery levels still span most of the ramp (S7, #62): a row barely
/// started, two past halfway, one near the top. Each count is a plausible
/// tenth of the default 150k zone for the level beside it (#105). The dormant
/// rows carry real levels too — a dormant conversation consumes nothing, so
/// its reading is exactly current, which is the ruling that closed
/// design-lock §7.2.
///
/// The summaries copy the SHAPE of real `ai-title` values, sampled from the
/// local transcript corpus 2026-07-31: sentence case, verb first, no trailing
/// period, 13-60 characters. They are invented rather than copied — the corpus
/// is the maintainer's own work and this repo is public — but a fixture that
/// invents the format too would misrepresent the column in the one image most
/// people will ever see. Several run past the summary column and truncate,
/// which is the honest common case.
///
/// The rows sit in cluster order (#234): a repo's tabs travel together, and
/// clusters rank by their summed frecency — clave's two tabs are the hottest
/// cluster, then api-svc and webapp, with the dormant block last. A fixture
/// that interleaved repos would show an order the live block can no longer
/// produce.
pub fn showcase() -> Vec<Row> {
    let mut rows = vec![
        // clave's cluster: the agent being watched, and the shell beside it.
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
            "hi",
            "claude",
            Some(232),
            "double-rows",
            "12m",
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
            "",
            Some(232),
            "double-rows",
            "2m",
        ),
        // The second working row: a plain branch, and the only one that has
        // fanned out to subagents.
        fanned_out(detail(
            agent(
                RowStatus::Working,
                Some((9, 141_000)),
                Provenance::Branch,
                "api-svc",
                API_SVC,
                Some(("AUTH-7", 3)),
                "Rotate the signing keys",
            ),
            "fable",
            "hi",
            "claude",
            Some(184),
            "key-rotation",
            "4m",
        )),
        // The one row waiting on a person — the state the sidebar exists to
        // make visible from across the room.
        blocked_on(
            detail(
                agent(
                    RowStatus::NeedsYou,
                    Some((3, 52_000)),
                    Provenance::Main,
                    "webapp",
                    WEBAPP,
                    Some(("CART-99", 6)),
                    "Fix cart total rounding mismatch",
                ),
                "haiku",
                "lo",
                "claude",
                None,
                "",
                "9m",
            ),
            "Bash (cargo publish)",
        ),
        // The dormant block: half-faded, opens where it left off. One of them
        // carries the other provider, so both icons appear in the frame.
        detail(
            agent(
                RowStatus::Dormant,
                Some((5, 79_000)),
                Provenance::Main,
                "infra",
                INFRA,
                Some(("DNS-TTL", 1)),
                "Debug staging rollout DNS timeout",
            ),
            "5.6sol",
            "",
            "openai",
            None,
            "",
            "3h",
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
            "md",
            "claude",
            None,
            "",
            "2w",
        ),
    ];
    rows[0].selected = true;
    rows
}
