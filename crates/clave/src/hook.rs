//! `clave hook <event>` (§6.5) — the ONLY writer of agent status. Runs as a
//! global Claude Code hook, so its prime directive is DO NO HARM: untracked
//! sessions get a lock-free read and exit 0; every internal error also exits
//! 0; it never prints a hook decision to stdout (a PreToolUse-style hook's
//! stdout can approve/deny tool use — ours stays silent and pass-through).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;
use clave_types::{AgentSnapshot, Status, lenient};
use serde::Deserialize;

use crate::spawn::jsonl_path;
use crate::store::{
    AgentRecord, LabelSource, Store, now_unix, read_store, snapshot_from, store_paths,
    with_store_mut,
};

/// The fields we care about across ALL hook events (each event's JSON is a
/// superset; serde ignores the rest). Everything optional — a malformed or
/// novel payload must degrade quietly, never error.
///
/// "Quietly" no longer means "no-op unconditionally", and the change is
/// deliberate (#97). An unparseable payload yields `session_id: None`; before
/// the rotation work that returned immediately, because the session id was the
/// only key. It is no longer the only key — a hook firing from the agent's own
/// Claude, proven by its pane's `CLAVE_AGENT_UUID`, belongs to that agent whatever its JSON
/// looked like, and refusing it would reintroduce the freeze for exactly the
/// events most likely to be malformed. The event NAME comes from argv, not
/// from this payload, so the status transition is still driven by something
/// trustworthy. With no `session_id` there is no transcript to trust either,
/// so the tail read is skipped and `title`/`summary` hold. (opus review, #98)
///
/// EVERY field reads through `lenient`. serde fails the WHOLE struct on one
/// field of the wrong type, and `run_hook` reads a failed parse as the empty
/// payload, so a novel shape in a field clave barely uses would blank
/// `session_id` and drop the event. FOOTGUNS has the trap. A new field takes
/// the same attribute.
#[derive(Debug, Default, Deserialize)]
pub struct HookPayload {
    #[serde(default, deserialize_with = "lenient")]
    pub session_id: Option<String>,
    /// UserPromptSubmit carries the prompt text — the §6.4 first-label fast
    /// path (no jsonl read needed for the initial label).
    #[serde(default, deserialize_with = "lenient")]
    pub prompt: Option<String>,
    /// Notification carries a human message. `notice` reads it only when
    /// the payload has no `notification_type`; `wants_from_message` takes the
    /// tool name from it.
    #[serde(default, deserialize_with = "lenient")]
    pub message: Option<String>,
    /// SessionEnd carries why the session ended: `clear`, `resume`,
    /// `logout`, `prompt_input_exit` or `other` (code.claude.com/docs/en/hooks).
    /// Only `other` can be a zellij quit, which is what stamps standby.
    #[serde(default, deserialize_with = "lenient")]
    pub reason: Option<String>,
    /// Where Claude is writing this session's transcript RIGHT NOW (#87).
    ///
    /// The alternative — rebuilding the path from the store's creation-time
    /// `rec.cwd` and `uuid` — is broken twice over, and S4 §4.3a/d records it
    /// as MANDATORY to replace rather than merely preferable: it misses when
    /// the session relocates, and it misses when Claude rotates the session id
    /// (a `/clear` or resume starts a new id AND a new file), which is #97.
    ///
    /// **This is attacker-adjacent input.** It arrives on hook stdin and what
    /// is read from it is written into the store, so an unvalidated path is a
    /// write primitive rather than a bad read. Always go through
    /// [`resolve_transcript`], never straight to the filesystem.
    #[serde(default, deserialize_with = "lenient")]
    pub transcript_path: Option<String>,
    /// The session's working directory, carried by every hook event. Read
    /// only on the #226 adoption path — the one mint input the payload must
    /// supply — and canonicalized (S0b) before it touches the store.
    #[serde(default, deserialize_with = "lenient")]
    pub cwd: Option<String>,
    /// Notification's kind (`permission_prompt`, `idle_prompt`, …). The
    /// contract the message text only hinted at; see [`notice`].
    #[serde(default, deserialize_with = "lenient")]
    pub notification_type: Option<String>,
    /// Present only when the hook fires INSIDE a subagent or an in-process
    /// teammate (code.claude.com/docs/en/hooks, common input fields). The
    /// `session_id` is still the parent's, so this is the one field that
    /// tells the parent's own events from its agents'.
    #[serde(default, deserialize_with = "lenient")]
    pub agent_id: Option<String>,
    /// Claude Code's task registry, on Stop and SubagentStop: the work still
    /// in flight, empty when nothing is. Kept as raw values because clave
    /// reads two keys of each entry and the entry shape is Claude Code's to
    /// grow; see [`fan_out_reading`].
    #[serde(default, deserialize_with = "lenient")]
    pub background_tasks: Option<Vec<serde_json::Value>>,
    /// SessionStart's cause: `startup`, `resume`, `clear`, `compact` or
    /// `fork`. Only `compact` keeps the agents of the session it continues.
    /// On UserPromptSubmit, who wrote the prompt (`is_typed`).
    #[serde(default, deserialize_with = "lenient")]
    pub source: Option<String>,
}

/// §6.5's transition table. Latest-wins, except where `current` decides:
/// a tool batch clears only red, and the idle nag clears only a live turn.
pub fn status_for_event(event: &str, payload: &HookPayload, current: Status) -> Option<Status> {
    match event {
        "UserPromptSubmit" => Some(Status::Working),
        "Stop" => Some(Status::Done),
        // A subagent's API error ends the subagent's turn, not the parent's.
        "StopFailure" if !from_a_subagent(event, payload) => Some(Status::Failed),
        "SessionEnd" => Some(Status::Idle),
        // The "answered" event that clave lacked. A batch resolves only after
        // every tool in it ran, so a permission or a question in it has been
        // answered (code.claude.com/docs/en/hooks, PostToolBatch). Only red
        // moves: a batch inside a live turn says nothing new.
        "PostToolBatch" if !from_a_subagent(event, payload) => {
            (current == Status::NeedsYou).then_some(Status::Working)
        }
        "Notification" => match notice(payload) {
            // Blocked on the human: red, whatever the row showed.
            Notice::Ask => Some(Status::NeedsYou),
            // Claude Code sends this about 60s after a response ends, only
            // if nothing was typed since and no background agent runs. A row
            // that still shows a turn missed its Stop (an interrupt sends
            // none), so the turn is over and nothing is owed. Done keeps its
            // green until read; Idle and Failed have nothing to say.
            Notice::Idle => {
                matches!(current, Status::Working | Status::NeedsYou).then_some(Status::Idle)
            }
            Notice::Other => None,
        },
        // Unknown/other events: strictly no-op. The hook is registered for a
        // fixed set, but a defensive default keeps novel events harmless.
        _ => None,
    }
}

/// A tool batch or an API error from inside a subagent. Its `session_id`
/// names the parent, but the parent's red is not its to clear, and its error
/// does not fail the parent's turn. Claude Code 2.1.299 builds both inputs
/// from the subagent's own context, with its `agent_id`, and the shared query
/// loop runs StopFailure in subagents too (the parent's turn ends in Stop or
/// in its own StopFailure). Measured over the local history, 2026-10-14: 5 API
/// errors ended subagent turns and 4 ended main threads. `run_hook` exits on
/// both before the store read: a busy subagent fires a batch per tool batch.
fn from_a_subagent(event: &str, payload: &HookPayload) -> bool {
    matches!(event, "PostToolBatch" | "StopFailure") && payload.agent_id.is_some()
}

/// Whether this event ends the row's turn: the agent CLI's own end (Stop),
/// an API error's end (StopFailure, the parent's own), or the idle nag on a
/// row that still shows a turn. Esc sends no Stop, so for an interrupted turn
/// the nag about 60 s later is the end clave hears. Read off the status table,
/// so the two cannot disagree on which nag ends a turn.
fn ends_turn(event: &str, payload: &HookPayload, current: Status) -> bool {
    match event {
        "Stop" => true,
        "StopFailure" => !from_a_subagent(event, payload),
        "Notification" => status_for_event(event, payload, current) == Some(Status::Idle),
        _ => false,
    }
}

/// What a Notification asks of the row.
#[derive(Debug, PartialEq, Eq)]
enum Notice {
    Ask,
    Idle,
    Other,
}

/// `notification_type` is the contract (code.claude.com/docs/en/hooks,
/// Notification input). The message text is the fallback for a payload that
/// lacks the field; its substrings were verified against CLI 2.1.201.
fn notice(payload: &HookPayload) -> Notice {
    match payload.notification_type.as_deref() {
        Some("permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog") => Notice::Ask,
        Some("idle_prompt") => Notice::Idle,
        Some(_) => Notice::Other,
        None => {
            let m = payload.message.as_deref().unwrap_or("");
            if m.contains("permission") {
                Notice::Ask
            } else if m.contains("waiting for your input") {
                Notice::Idle
            } else {
                Notice::Other
            }
        }
    }
}

/// Turns the agent CLI submits on its own: nobody typed them, so they move
/// the row to Working but are not a commitment (no `last_interacted`, no
/// ordinal, no frecency). Read from Claude Code 2.1.299: the task-notification
/// and loop-tick turns, the peer, coordinator and observer wrappers (one
/// module, which also defines the `<cross-session-message>` and
/// `<teammate-message>` tags), and the bound-conversation activity lines.
/// Measured over the local transcripts 2026-10-14: 1262 `<task-notification`
/// and 1127 peer turns against 1901 typed ones, each with its prefix at the
/// start. The hook payload carries no prompt source (the builder sends none),
/// so the text is the only witness.
///
/// The hook's `prompt` is the text the turn's user message is built from
/// (the same `prompt.submit` input), so these match the hook as they match
/// the transcript. A turn whose text starts with none of these counts as
/// typed: a queued message the human typed carries no prefix, and it IS a
/// commitment. Known gap: a scheduled prompt (`/loop`, a cron) carries the
/// human's own words, so it counts as typed.
const HARNESS_TURN_PREFIXES: &[&str] = &[
    "<task-notification",
    "<system-reminder",
    "<cross-session-message",
    "<agent-message",
    "<teammate-message",
    "Another Claude session sent a message",
    "A peer session sent a message",
    "The coordinator sent a message",
    "Your background observer (",
    "Activity in the bound conversation",
    "Activity was observed in the bound conversation",
    "# Autonomous loop tick",
    "# Autonomous loop check",
];

/// True if `text`'s trimmed start is a harness turn. The START, never a
/// substring: a typed prompt may quote one.
pub(crate) fn is_harness_turn(text: &str) -> bool {
    let t = text.trim_start();
    HARNESS_TURN_PREFIXES.iter().any(|p| t.starts_with(p))
}

/// Whether a UserPromptSubmit is a typed prompt. The payload's `source` says
/// who wrote the prompt, where the CLI sends it: `user` (the composer) and
/// `sdk` (`-p`, the Agent SDK) are a person; `system`, `loop_wakeup`,
/// `schedule_wakeup` and `poll_event` are the CLI itself (2.1.299 hook input
/// schema: "Payloads may omit it while the field rolls out"). Without the
/// field, the prompt's start decides (`is_harness_turn`). The field closes
/// the `/loop` and cron gap, whose prompts carry the human's own words.
fn is_typed(payload: &HookPayload) -> bool {
    match payload.source.as_deref() {
        Some(source) => matches!(source, "user" | "sdk"),
        None => !payload.prompt.as_deref().is_some_and(is_harness_turn),
    }
}

/// The text the CLI writes as a user record when Esc interrupts a turn:
/// `[Request interrupted by user]`, or `… for tool use]` when a tool was
/// running. Measured over the local history, 2026-10-14: 34 records, each a
/// text block of its own. No hook carries it, so only the transcript does.
pub(crate) const INTERRUPT_MARKER: &str = "[Request interrupted by user";

/// Whether the transcript's newest conversation record is an interrupt: the
/// marker is newer than every assistant record in the tail. Read at a
/// UserPromptSubmit. Every other record is skipped: tool results,
/// attachments, system lines, and the new prompt itself if the CLI wrote it
/// first. So a turn that is still live finds its own assistant record first.
fn interrupted(tail: &str) -> bool {
    for line in tail.lines().rev() {
        // Two byte tests first: the tail parses here only where it can answer.
        let assistant = line.contains(r#""type":"assistant""#);
        if !assistant && !line.contains(INTERRUPT_MARKER) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("isSidechain").and_then(serde_json::Value::as_bool) == Some(true) {
            continue;
        }
        match v.get("type").and_then(serde_json::Value::as_str) {
            Some("assistant") => return false,
            Some("user") if starts_with_marker(&v) => return true,
            _ => {}
        }
    }
    false
}

/// Whether a user record's first text is the interrupt marker, in either of
/// the two shapes a user record's content takes (a string, or blocks).
pub(crate) fn starts_with_marker(v: &serde_json::Value) -> bool {
    let content = v.get("message").and_then(|m| m.get("content"));
    let first_text = match content {
        Some(serde_json::Value::String(t)) => Some(t.as_str()),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .find(|b| b.get("type").and_then(serde_json::Value::as_str) == Some("text"))
            .and_then(|b| b.get("text"))
            .and_then(serde_json::Value::as_str),
        _ => None,
    };
    first_text.is_some_and(|t| t.trim_start().starts_with(INTERRUPT_MARKER))
}

/// Wrappers around text the human DID type (a slash command, a local
/// command's output): a commitment, but never words to earn a label from.
const TYPED_WRAPPER_PREFIXES: &[&str] = &["<local-command-caveat", "<command-name"];

/// True if `text` must NEVER be treated as user intent for labelling (issue
/// #17, observed live 2026-07-21): resuming a session that died with a
/// pending background task makes Claude Code auto-fire a turn whose "prompt"
/// is an orphaned `<task-notification>` tag. Because earned labels stick
/// (§6.4: no self-heal), letting one through once bakes it permanently. We
/// check the START, and we never strip the tag to use what follows: an
/// injected body is harness text end to end. Issue #24's rename tier sits
/// ABOVE this guard; this only prevents MIS-earning.
fn is_harness_injected(text: &str) -> bool {
    let t = text.trim_start();
    is_harness_turn(t) || TYPED_WRAPPER_PREFIXES.iter().any(|p| t.starts_with(p))
}

/// First 4 words, hard cap 32 chars — enough to recognise, short enough for
/// a ~24-col bar row after the `dir · branch ·` prefix is truncated (§6.4:
/// final clamping is the RENDERER's job; this just bounds the stored label).
pub fn first_words(text: &str) -> String {
    let mut s = text
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    if s.len() > 32 {
        // Truncate on a char boundary (labels can contain multibyte chars).
        s = s.chars().take(32).collect();
    }
    s
}

/// Generous bound for `rec.summary`. The bar's summary column is 17 display
/// cells at the profile that actually ships today (44 columns), and it is set
/// to WIDEN — LEDGER D19 takes expanded to 54, which would make it 25 — while
/// `render.rs` does its own cell-accurate clamping either way, so the store
/// holds PROSE rather than a pre-truncated fragment (design-lock §7.1: the row
/// renders from the store). The bound is therefore justified by the CORPUS,
/// not by any one column width: measured 2026-07-29 over the local
/// transcripts, `aiTitle` values run 13–60 chars, so 200 is ~3x headroom while
/// still bounding a field a hook rewrites on every turn.
const SUMMARY_MAX_CHARS: usize = 200;

/// Bound for `rec.title`. A rename is an identifier for a 7-cell chip — D17
/// holds title at 7 in BOTH shipped profiles, and D19 would take expanded to 9
/// — never prose; 64 is far beyond any real one and exists only so a
/// pathological transcript cannot grow the store.
const TITLE_MAX_CHARS: usize = 64;

/// Single-line, whitespace-collapsed, char-boundary-clamped field text.
///
/// NOT `sanitize_label`, deliberately. `sanitize_label` additionally DROPS
/// `"` and `\` because the label is baked into `launch.kdl` (`setup.rs`'s
/// eager row → `add::tab_node_bare`), where either character is a parse error
/// that bricks cold-start launch. Neither `title` nor `summary` is ever baked
/// into KDL — nothing outside `snapshot_from` reads them — so silently
/// deleting characters out of displayed prose would be lossy for no gain.
///
/// What IS shared is the single-line property: a raw `\n` or `\u{1b}` measures
/// 0 cells in `unicode-width`, which is exactly the input LEDGER D14 records as
/// reachable-not-hypothetical. `render.rs` defends itself against it too; that
/// is defence in depth, not a reason for the store to hold multi-line junk.
pub fn clamp_field(text: &str, max_chars: usize) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

/// The LAST line in `tail` of `{"type":<kind>, <field>:<non-empty string>}`.
///
/// One line-wise `serde_json::Value` walk shared by every tail extractor — no
/// regex, no full-file model of Claude's schema, and no per-tier parser to
/// keep in sync. An EMPTY or whitespace-only value is skipped rather than
/// returned, so scanning continues further back: `/clear` appends an empty
/// `custom-title` and the maintainer's ruling (#24) is that clave holds the
/// last real rename across it.
fn last_tail_field(tail: &str, kind: &str, field: &str) -> Option<String> {
    tail.lines().rev().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        if v.get("type")?.as_str()? != kind {
            return None;
        }
        let s = v.get(field)?.as_str()?.trim();
        (!s.is_empty()).then(|| s.to_string())
    })
}

/// The LAST `{"type":"system","subtype":<subtype>, <field>:<non-empty
/// string>}` line in `tail` — [`last_tail_field`]'s discipline for the system
/// channel, which multiplexes many record kinds behind one `type` and
/// discriminates on `subtype` (the FOOTGUNS inventory one-liner buckets `type`
/// only, which is exactly how these records went unnoticed until #111). Same
/// skip-empty rule, so a blank value scans further back rather than winning.
fn last_system_tail_field(tail: &str, subtype: &str, field: &str) -> Option<String> {
    tail.lines().rev().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        if v.get("type")?.as_str()? != "system" || v.get("subtype")?.as_str()? != subtype {
            return None;
        }
        let s = v.get(field)?.as_str()?.trim();
        (!s.is_empty()).then(|| s.to_string())
    })
}

/// The session's freshest away-period recap —
/// `{"type":"system","subtype":"away_summary","content":…}`, written when the
/// user returns after being away: one to two sentences of session state plus
/// next action, RE-GENERATED per away period, so unlike `ai-title` it
/// actually narrates progress (measured 2026-08-01 on #111: 50 local
/// transcripts carry at least one, fleet-born sessions included). The TOP
/// tier for `rec.summary` (#131): the away/return pattern IS dormancy, so the
/// rows whose summary matters most are exactly the ones that earn a recap —
/// and per #111's discriminator finding, fleet-born rows never earn an
/// `ai-title`, so this is the first signal that can upgrade them past the
/// prompt seed. Newest wins by tail position: the lines are appended in
/// order, and the reverse scan takes the last.
pub fn away_summary_from_tail(tail: &str) -> Option<String> {
    last_system_tail_field(tail, "away_summary", "content")
}

/// Claude's own auto-description — `{"type":"ai-title","aiTitle":…}`. NOT
/// rolling, despite how often it is re-emitted: LEDGER D24 measured up to 85
/// `ai-title` lines in one transcript and never more than one distinct VALUE
/// per session. It is a subtitle for the conversation, not a running commentary.
/// THE source for `rec.summary` (#79): it is what Claude Code writes where it
/// once wrote `type:"summary"`, which appears in 0 transcripts.
/// BUT IT IS MOSTLY ABSENT, and increasingly so — re-measured 2026-07-31 over
/// all 770 local transcripts, only 68 carry one (45 of the 390 with >=20 user
/// turns; 7 of 132 for the preceding 7 days), down from 74 of 153 on
/// 2026-07-29. That does NOT mean most summaries are blank: `refresh_row_fields`
/// seeds an empty `summary` from the current prompt, so absence here demotes the
/// row to a prompt-derived summary rather than emptying it. `custom-title` is
/// near-exclusive with this field — across all 770, 62 carry only `ai-title`,
/// 74 only `custom-title`, 6 both — and the discriminator is unknown (#111).
/// Do not read "has ai-title" as "not renamed".
pub fn ai_title_from_tail(tail: &str) -> Option<String> {
    last_tail_field(tail, "ai-title", "aiTitle")
}

/// This session's title — `{"type":"custom-title","customTitle":…}`,
/// re-appended latest-wins. The source for `rec.title` (design-lock §5/§7.1:
/// the filled chip). Verified against a live sandbox transcript 2026-07-29:
/// the line carries exactly `type`, `customTitle`, `sessionId`.
///
/// NOT only a user rename, despite the name. Measured 2026-07-31: it is
/// written from a session's FIRST line and re-stamped every few user turns,
/// single-valued throughout, in sessions that were never renamed (15 lines
/// across 75 user turns in one; 71 across 280; 95 across 363). So a stripped or
/// missing `title` re-derives on the next read in practice — measured, not
/// guaranteed: re-stamping is paced by TURNS while the tail is 64 KiB of BYTES,
/// so a turn emitting more than a window's worth of transcript could still evict
/// the last line. It has not been observed (worst case across all 148 carrying
/// transcripts: 34,808 bytes from EOF). Unlike `summary` this field has NO
/// fallback tier, so absence renders the blank chip D25 ratified. Presence is
/// the real limit: 80 of 770 local transcripts carry one.
pub fn custom_title_from_tail(tail: &str) -> Option<String> {
    last_tail_field(tail, "custom-title", "customTitle")
}

/// Scan a jsonl TAIL for the LAST `{"type":"summary","summary":…}` line.
///
/// EXTINCT tier (#79) — Claude Code no longer emits it, so this has never
/// once fired in production. Kept as the LABEL's source (the §6.4 freeze this
/// task must not disturb) and as `rec.summary`'s legacy fallback behind
/// `ai_title_from_tail`; retargeting the *label* is S4's call, not this one.
pub fn summary_from_tail(tail: &str) -> Option<String> {
    last_tail_field(tail, "summary", "summary")
}

/// Where the session is living RIGHT NOW, and the branch of that same record:
/// `(cwd, branch)`, with `None` for a branch the record cannot name.
///
/// Read as a PAIR from one record, never two scans. A cwd from one line and a
/// branch from another can describe different checkouts, and writing that pair
/// down would mint a row that has never existed.
///
/// **Why the transcript and not the payload.** Every hook event carries a
/// `cwd`, but only the #226 adoption path reads it, so an existing row keeps
/// whatever checkout it was minted in. Nothing else re-derives it: `clave
/// spawn` repoints a row it finds relocated, and that is the only writer. A
/// session that moves AFTER it is opened — a `cd` into a worktree, most often —
/// therefore leaves its row describing the old checkout for ever, and both the
/// branch cell and `pr-sync`'s `gh` question are asked of the wrong one.
/// Measured 2026-09-15: 76 of 1219 transcripts change cwd mid-session, 6% of the whole
/// corpus and 30% of those over 1 MiB — a long conversation is where it happens.
///
/// The transcript answers it for free — 350 of 350 carry both fields inside
/// the 64 KiB window this hook already parses — and it is the source that
/// out-ranks the store (AGENTS.md).
fn checkout_from_tail(tail: &str) -> Option<(String, Option<String>)> {
    tail.lines().rev().find_map(|line| {
        // Byte pre-filter before the parse, same discipline as the mark above.
        if !line.contains(r#""cwd":"#) {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let cwd = v.get("cwd")?.as_str()?.trim();
        if cwd.is_empty() {
            return None;
        }
        // `HEAD` is Claude Code's detached-head answer, not a branch name — 50
        // of 350 transcripts end on it. Recording it would put the literal word
        // HEAD in the card's branch cell and send `gh` looking for a PR on it.
        let branch = v
            .get("gitBranch")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|b| !b.is_empty() && *b != "HEAD")
            .map(str::to_owned);
        Some((cwd.to_owned(), branch))
    })
}

/// Move the row to the checkout its transcript says it is in. Returns whether
/// anything moved, because the card renders all three cells.
///
/// A branch the record could not name is written as `-`, the sentinel
/// `add::record_branch` already uses for a detached resume and the bar already
/// blanks. Holding the PREVIOUS branch would be worse than blank: it names a
/// branch of the checkout the row has just left.
fn take_checkout(rec: &mut AgentRecord, facts: Option<(String, Option<String>)>) -> bool {
    let Some((cwd, branch)) = facts else {
        return false;
    };
    // The same gate `clave add` puts on a cwd, for the same reason: this value
    // is baked RAW into generated KDL, where a `"` or `\` is a parse error and
    // the tab silently fails to open. A directory may legally hold either, so
    // the transcript can name one. Refuse the move instead of persisting a
    // value a later launch cannot use; the row keeps the checkout it had, which
    // is stale but openable. Validation only, deliberately no canonicalize:
    // this runs on the hook's hot path and must not touch the filesystem.
    if crate::add::validate_cwd(&cwd).is_err() {
        return false;
    }
    let branch = branch.unwrap_or_else(|| "-".to_string());
    if rec.cwd == cwd && rec.branch == branch {
        return false;
    }
    // NOTHING here decides which repository the row now belongs to, and that
    // is deliberate. Two rounds of review went the other way and both were
    // wrong, for the same reason #86 gives: a repository fact must not be read
    // off the shape of a path.
    //
    // The tempting rule is "the arriving cwd is outside `repo_root`, so the row
    // left its repo". It does not hold, because `repo_root` is not one kind of
    // thing. `add::run_add`'s resume arm writes the MAIN working tree, but its
    // `new` arm and `hook::mint_adopted` both write `rev-parse --show-toplevel`
    // of the directory in hand — and inside a linked worktree that IS the
    // worktree (the fugu 2026-07-21 finding, `add::main_worktree_path`).
    // Measured on the live store 2026-09-15: 4 of 199 rows have a worktree path
    // as their root and no `worktree` set, and 0 rows have `worktree` set at
    // all. For those four, a `cd` to the repo's own main checkout — an ordinary
    // move, within one repository — reads as "left", and clearing on it costs
    // the row its repo name, its colour, its PR cell and its branch cell for
    // good: `pr::resolve_pr` refuses an empty root for ever and no writer ever
    // restores one, so there is no way back short of a re-mint.
    //
    // The risk that argued for clearing was `gh` being asked about the new
    // branch from the old repository. That is answered where it arises, by
    // asking from the directory the session is actually in — see
    // `pr::checkout_dir`. Consistency by construction beats a guess about
    // containment, and it cannot strand a row.
    // The one move that can take a row OUT of a worktree, so the tree mark
    // must not outlive the directory it describes — the same component-wise
    // rule, and the same reasoning, as `store::apply_relocation`. A move
    // WITHIN the worktree keeps its mark; a move out drops to unknown, which
    // is honest: nothing here has asked git where the new checkout's root is.
    if rec
        .worktree
        .as_deref()
        .is_some_and(|w| !std::path::Path::new(&cwd).starts_with(w))
    {
        rec.worktree = None;
    }
    // The cached PR number was asked from the OLD directory, and
    // `pr::pr_is_stale` keys on the TTL and the branch — neither of which
    // notices the move. Two repos both on `main` therefore leave repo A's
    // number beside repo B's checkout for up to `PR_TTL_SECS`. Measured
    // 2026-09-15: 11 of the 76 cwd-changing transcripts cross a git toplevel,
    // and the common shape is a session walking between sibling repos that are
    // all on their default branch.
    //
    // Zeroing the stamp is the whole fix: `pr_checked == 0` is already the
    // "never looked" case, so the next event re-asks from the new cwd. The
    // NUMBER is deliberately left in place — clearing it would blank the cell
    // on every `cd` into a subdirectory of the same repo, which is the common
    // move by far, and the answer that comes back is the same number.
    if rec.cwd != cwd {
        rec.pr_checked = 0;
    }
    rec.cwd = cwd;
    rec.branch = branch;
    true
}

/// The worktree the session's worktree TOOL has it in, off the newest
/// well-formed `worktree-state` record in `tail`. `None` is no such record in
/// the window, and the caller HOLDS on it.
///
/// Claude Code writes this record when a session enters a worktree through
/// its own worktree tool, and re-stamps it by turns like `custom-title`.
/// Measured 2026-09-18 over the local corpus: 29 transcripts carry it.
///
/// **The `"worktreeSession":null` form is NOT read as "left the worktree",
/// and this reader returns `None` for it on purpose.** It means "no worktree
/// tool session", which is also what a session launched or resumed directly
/// inside a worktree directory writes: measured on
/// `…/b947acf7-….jsonl`, the null re-stamps on three consecutive turns while
/// every `cwd` line still reads `.claude/worktrees/triple-card`. Clearing on
/// it would strip a correct mark, and `add::heal_worktrees` would re-seed it
/// at the next setup, so the two writers would flap. The clear already exists
/// and is keyed on the one fact that can prove a departure — the cwd leaving
/// the tree, in `take_checkout`. (opus review, 2026-09-18)
///
/// **Why a third writer.** `clave add` asks git at mint time, and
/// `add::heal_worktrees` asks git at setup and release time. Neither runs on
/// the hook path, so a row whose session entered a worktree AFTER it was
/// opened followed the move in `cwd` and `branch` (`take_checkout`) and wore
/// the BRANCH mark until the next setup. Reported from the live fleet
/// 2026-09-18: a card reading `clave` with no tree, whose session had been in
/// `.claude/worktrees/term-chip-green` for an hour. The transcript names the
/// path for free, on the tail this hook already parses, and it is the source
/// that out-ranks the store (AGENTS.md).
fn worktree_from_tail(tail: &str) -> Option<String> {
    tail.lines().rev().find_map(|line| {
        // Byte pre-filter before the parse, same discipline as the mark above.
        // The parsed `type` is re-checked because the substring can sit inside
        // another record's payload; only the top-level field is the record.
        if !line.contains(r#""type":"worktree-state""#) {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("type")?.as_str()? != "worktree-state" {
            return None;
        }
        let path = v
            .get("worktreeSession")?
            .get("worktreePath")?
            .as_str()?
            .trim();
        (!path.is_empty()).then(|| path.to_owned())
    })
}

/// SET `rec.worktree` from the transcript's reading, or hold. Never clears:
/// see [`worktree_from_tail`] for why the null record cannot be trusted to
/// mean a departure, and `take_checkout` for the clear that can.
///
/// A path is taken only when the row's `cwd` is inside it, component-wise —
/// the same containment `take_checkout` uses to DROP the mark. The two must
/// agree, or a `worktree-state` record older than a later `cwd` move would
/// re-mark a row that has just left the tree. The cwd is the newer fact on
/// every line; the worktree record is re-stamped by turns.
///
/// The path is persisted as Claude Code wrote it, not canonicalized: this is
/// the hook's hot path and must not touch the filesystem, and every consumer
/// of the field today reads it as a boolean (`provenance_of`). `add` writes
/// git's `show-toplevel` for the same field; the two forms agree on every
/// worktree measured, and a symlinked cwd against a canonical path fails the
/// containment test above and simply never sets the mark. No `validate_cwd`
/// either: unlike `cwd`, this value is not baked into generated KDL.
///
/// Returns whether the value moved, so a push happens only when a pixel would.
fn take_worktree(rec: &mut AgentRecord, reading: Option<String>) -> bool {
    let Some(path) = reading else {
        return false;
    };
    if !std::path::Path::new(&rec.cwd).starts_with(&path) {
        return false;
    }
    let next = Some(path);
    let moved = rec.worktree != next;
    rec.worktree = next;
    moved
}

/// The card's subagent mark (lock §4.6) from what the agent CLI reports live:
/// `Some` is a reading, `None` holds the mark. A BOOLEAN, not a count: "this
/// row has agents under it" is the whole signal.
///
/// The task registry is the source. Claude Code lists the work in flight on
/// Stop and SubagentStop as `background_tasks` (code.claude.com/docs/en/hooks,
/// Stop input), and a subagent appears there under its own `agent_id`
/// (2.1.299: a `local_agent` task's id IS the agent id). It lists only
/// BACKGROUND work: a foreground subagent blocks the turn, so its start and
/// stop are the whole of its life.
///
/// - SubagentStart: an agent started (or resumed, or a teammate took a
///   message), so the mark rises.
/// - SubagentStop: the agent that stops may still be listed as running, so
///   only the OTHERS count.
/// - Stop: the turn ended; any agent still listed keeps the mark.
/// - SessionStart: a new process holds no agents of the old one, except
///   that a compaction continues the same session, so it holds.
/// - SessionEnd: nothing can run under an ended session.
///
/// A payload with no registry reads as no agents: the docs name the field
/// present whenever the registry is reachable. This replaced a ledger over
/// the transcript, which a `<task-notification>` without its `<tool-use-id>`
/// broke (FOOTGUNS).
pub fn fan_out_reading(event: &str, payload: &HookPayload) -> Option<bool> {
    let agents_except = |stopping: Option<&str>| {
        payload.background_tasks.iter().flatten().any(|task| {
            matches!(
                task.get("type").and_then(serde_json::Value::as_str),
                Some("subagent" | "teammate")
            ) && task.get("id").and_then(serde_json::Value::as_str) != stopping
        })
    };
    match event {
        "SubagentStart" => Some(true),
        "SubagentStop" => Some(agents_except(payload.agent_id.as_deref())),
        "Stop" => Some(agents_except(None)),
        "SessionStart" => (payload.source.as_deref() != Some("compact")).then_some(false),
        "SessionEnd" => Some(false),
        _ => None,
    }
}

/// The anchor a permission notification's tool name follows. Claude Code's
/// wording, matched as a substring: the CLI owns the sentence and has
/// reworded it before, and a substring survives a prefix change. The kind of
/// a notification comes from `notification_type` (`notice`); the sentence is
/// the only source of the tool name.
const PERMISSION_ANCHOR: &str = "permission to use ";

/// What a blocked agent is asking for, from the `Notification` message clave
/// already receives (lock §4.7 tier 1). `Claude needs your permission to use
/// Bash` yields `Bash`.
///
/// The MESSAGE is the source and that is the whole point of the tier: the tool
/// name is also on a `PreToolUse` payload, but clave does not register that
/// event — reading it there would mean a hook firing on every tool call of
/// every tracked session, to learn something the notification hands over for
/// free. `None` for every other message, including
/// the idle nag: the card's word for "nothing measured" is blank.
pub fn wants_from_message(message: &str) -> Option<String> {
    // `split_once`, not `split(..).nth(1)`: the two differ the moment the
    // anchor appears twice, where `nth(1)` hands back the MIDDLE segment and
    // silently drops everything past the second occurrence. What the cell
    // wants is the whole tail after the first anchor.
    let rest = message.split_once(PERMISSION_ANCHOR)?.1.trim();
    (!rest.is_empty()).then(|| clamp_field(rest, WANTS_MAX_CHARS))
}

/// Cap for [`wants_from_message`]. Wider than the 28 columns the cell holds at
/// 48 (lock §4.7), because the RENDERER does the final clamping — the store's
/// job is only to keep an unbounded notification out of the snapshot.
const WANTS_MAX_CHARS: usize = 64;

/// Write, hold or clear `rec.wants` against the status the caller just set.
///
/// Three cases, and the middle one is the reason this is a function rather
/// than an assignment. A flagged row whose message NAMES a tool takes it. A
/// flagged row whose message names none (a notification of another kind, or
/// an ask that the CLI words without the anchor) HOLDS what it already had,
/// because a message carrying no news must not erase one that did. The idle
/// nag is not one of these: it never fires while a dialog is on screen
/// (FOOTGUNS, "The idle nag never fires mid-turn"), and it ends the turn. An
/// unflagged row wants nothing, whatever it wanted a moment ago.
///
/// Returns whether the value moved, so a push happens only when a pixel would.
pub fn take_wants(rec: &mut AgentRecord, message: Option<&str>) -> bool {
    let next = if rec.status == Status::NeedsYou {
        message
            .and_then(wants_from_message)
            .or_else(|| rec.wants.clone())
    } else {
        None
    };
    let moved = rec.wants != next;
    rec.wants = next;
    moved
}

/// First unsigned integer following `"<key>":` in `s`. Deliberately literal:
/// the leading quote is what keeps `"input_tokens":` from also matching
/// `"cache_read_input_tokens":` and `"ephemeral_1h_input_tokens":`, which sit in
/// the same object and would otherwise be summed twice over.
fn json_u32(s: &str, key: &str) -> Option<u32> {
    let pat = format!("\"{key}\":");
    let rest = &s[s.find(&pat)? + pat.len()..];
    rest.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// The `"usage"` object's OWN keys, with every nested object and array elided.
///
/// A flat scan of the line cannot be trusted, and the capture proves why: the
/// real `usage` object nests `cache_creation` (carrying
/// `ephemeral_1h_input_tokens`) and an `iterations` array that repeats
/// `input_tokens`, `cache_read_input_tokens` and `cache_creation_input_tokens`
/// once per inference step. A key MISSING from the turn's own usage would then
/// be answered by one inference step's copy of it — a wrong reading that looks
/// entirely reasonable. Reading only depth 1 makes that unreachable rather than
/// unlikely. (CodeRabbit, #147)
///
/// Assumes no `{`/`[` inside a string value, which holds for `usage`: every
/// value is a number or a bare enum (`"standard"`, `"not_available"`).
fn usage_fields(line: &str) -> Option<String> {
    let start = line.find(USAGE_KEY)? + USAGE_KEY.len() - 1;
    let mut depth = 0i32;
    let mut out = String::new();
    for c in line[start..].chars() {
        // The brackets themselves are never collected — `json_u32` scans for
        // `"key":` and digits, so punctuation carries nothing. Collecting them
        // only bought two mutants that no test could ever distinguish.
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(out);
                }
            }
            _ if depth == 1 => out.push(c),
            _ => {}
        }
    }
    None // the tail cut mid-object; no complete reading here
}

/// Every literal this module greps for in the transcript. Named here, and used
/// from the parser below, so the capture's liveness test can assert against the
/// SAME strings rather than a hand-copied second list that drifts.
pub const BOUNDARY: &str = "\"subtype\":\"compact_boundary\"";
pub const POST_TOKENS: &str = "postTokens";
pub const USAGE_KEY: &str = "\"usage\":{";
pub const USAGE_SUMMED: [&str; 3] = [
    "input_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
];

/// Tokens the conversation is currently holding, scanned out of a jsonl tail
/// (S7, #62). `None` = no reading available, which HOLDS whatever the row had.
///
/// Ported from rot-reducer's `tokens_from_transcript`: the newest assistant
/// turn's `usage` carries `input_tokens + cache_read_input_tokens +
/// cache_creation_input_tokens`, and their sum is that turn's occupancy.
///
/// COMPACT-AWARE, and that is not decoration. `/compact` leaves the
/// pre-compaction `usage` lines in place, so the newest one names the OLD size —
/// the battery would read red on a session just emptied, and stay wrong until
/// the next `Stop`. So: anchor on the last `compact_boundary`, sum only what
/// follows it, and fall back to that line's own `compactMetadata.postTokens`,
/// which is an EXACT figure rather than an estimate.
///
/// Note what is NOT here: no fallback tier. rot-reducer degrades to a
/// tool-call estimate because it must always produce a number to decide whether
/// to nudge; the battery must not, because a fabricated reading is worse than a
/// blank cell (§5.4 fail-closed, and `agent_content`'s never-invent rule).
pub fn tokens_from_tail(tail: &str) -> Option<u32> {
    let (after, post_tokens) = match tail.rfind(BOUNDARY) {
        Some(i) => {
            // Searched from the MARKER, not from the start of its line:
            // `compactMetadata` always follows `subtype` on the boundary line
            // (verified against a real transcript), so there is nothing to the
            // left of the marker worth reading, and reaching for it would only
            // risk picking up an earlier boundary's figure.
            let end = tail[i..].find('\n').map_or(tail.len(), |n| i + n);
            (&tail[end..], json_u32(&tail[i..end], POST_TOKENS))
        }
        None => (tail, None),
    };
    after
        .lines()
        .rev()
        .find_map(|line| {
            // Depth 1 only — see `usage_fields`. Occupancy is what went IN, so
            // `output_tokens` is deliberately not summed.
            let usage = usage_fields(line)?;
            let sum: u32 = USAGE_SUMMED
                .iter()
                .filter_map(|k| json_u32(&usage, k))
                .sum();
            (sum > 0).then_some(sum)
        })
        .or(post_tokens)
}

/// The newest assistant line's `message.model` — the raw model id, e.g.
/// `claude-fable-5`. Nested under `message`, so `last_tail_field` (top-level
/// fields only) cannot read it. Same reverse-scan, skip-malformed,
/// skip-empty discipline.
pub fn model_from_tail(tail: &str) -> Option<String> {
    tail.lines().rev().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        if v.get("type")?.as_str()? != "assistant" {
            return None;
        }
        let s = v.get("message")?.get("model")?.as_str()?.trim();
        // `<synthetic>` is Claude Code's marker on an assistant line IT wrote
        // — an interrupt notice, an API error stub — not a model that
        // answered. Seen live in the sandbox drive (2026-09-09), where one
        // such line among ten real ones left the card reading `<synt…` until
        // the next real answer overwrote it. Scanning PAST keeps the last
        // model that actually spoke, which is the honest cell. The angle
        // brackets are the discriminator: no model id has ever carried one.
        (!s.is_empty() && !s.starts_with('<')).then(|| s.to_string())
    })
}

/// The newest assistant line's top-level `effort` — the raw level word
/// (`xhigh`), written beside `message.model` on every assistant line.
pub fn effort_from_tail(tail: &str) -> Option<String> {
    last_tail_field(tail, "assistant", "effort")
}

/// Display form of an effort level: the two-letter tag the card's cell holds
/// (`lo` `md` `hi` `xh` `mx` `au`). Anything unrecognised keeps its first two
/// letters rather than vanishing — the transcript said something, and two
/// cells is exactly how much of it there is room for.
pub fn short_effort(raw: &str) -> String {
    match raw {
        "low" => "lo",
        "medium" => "md",
        "high" => "hi",
        "xhigh" => "xh",
        "max" => "mx",
        "auto" => "au",
        other => return other.chars().take(2).collect(),
    }
    .to_string()
}

/// Display form of a model id: for Claude ids, the FAMILY word (`fable`,
/// `opus`, `sonnet`, `haiku`) — the segment after the vendor prefix that
/// isn't a version number; anything else passes through untouched (open
/// strings — other providers name their own). The store carries this SHORT
/// form: the card's model cell is 6 columns and the raw id is unreadable
/// there, and a dumb renderer (truncate, never munge) is the lock's style.
pub fn short_model(raw: &str) -> String {
    match raw.strip_prefix("claude-") {
        Some(rest) => rest
            .split('-')
            .find(|seg| !seg.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(rest)
            .to_string(),
        None => raw.to_string(),
    }
}

/// The agent's smart zone in tokens: [`clave_types::SMART_ZONE_ENV`], else the
/// default. Junk, or zero, falls back rather than failing — a hook must never
/// fail hard (§6.5), and a zero zone has no ramp to divide.
pub fn smart_zone() -> u32 {
    smart_zone_from(std::env::var(clave_types::SMART_ZONE_ENV).ok().as_deref())
}

/// [`smart_zone`]'s decision, split out from the environment read so it can be
/// tested — env vars are process-global and two tests setting one race.
fn smart_zone_from(raw: Option<&str>) -> u32 {
    raw.and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|z| *z > 0)
        .unwrap_or(clave_types::DEFAULT_SMART_ZONE_TOKENS)
}

/// Bucket a token count into the S7 ramp (#62): one step per tenth of the zone,
/// floored, so a row reads full until it has actually spent a tenth.
///
/// The zone is where the battery turns RED — the last index — and not where the
/// ramp ends, so anything past it CLAMPS there. A session at four times its
/// zone reads the same as one a token over: both are out, and #105's token text
/// carries the magnitude the glyph has stopped resolving.
pub fn battery_level(tokens: u32, zone: u32) -> u8 {
    if zone == 0 {
        return 0;
    }
    // Widened before multiplying: a large count against a small zone overflows
    // u32 well before the clamp would rescue it.
    let tenths = u64::from(tokens) * 10 / u64::from(zone);
    tenths.min(u64::from(clave_types::BATTERY_LEVELS - 1)) as u8
}

/// How much of the transcript the readings at a prompt or a turn end see.
/// Each reading parses every line it is handed, so the window is also their
/// cost; each takes the newest record of its kind, so a wider one changes no
/// value.
pub const TAIL_BYTES: u64 = 64 * 1024;

/// The whole lines in the last ≤`max_bytes` of `path` (lossy UTF-8; we only
/// pattern-match). The jsonl grows unbounded — a full read every turn risks
/// the hook timeout budget, so we read the tail only (§6.4).
///
/// A read that starts mid-file starts mid-line, so it is cut forward to the
/// first newline. The readers would skip an unparseable fragment, but the
/// fragment is a well-formed PREFIX of a real record often enough to parse
/// as one and report a stale field. The read starts one byte early, so a
/// window that starts exactly on a line keeps that line: the byte before it
/// is the newline the cut finds.
pub fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start.saturating_sub(1))).ok()?;
    let mut buf = Vec::with_capacity((len - start + 1) as usize);
    f.read_to_end(&mut buf).ok()?;
    let whole = if start == 0 {
        &buf[..]
    } else {
        // Cut on BYTES: one past a `\n` is always a character boundary, and
        // the lossy decode below never sees half a character at the front.
        let nl = buf
            .iter()
            .position(|&b| b == b'\n')
            .map_or(buf.len(), |i| i + 1);
        &buf[nl..]
    };
    Some(String::from_utf8_lossy(whole).into_owned())
}

/// Refresh the row's derived text: `rec.title` and `rec.summary` (design-lock
/// §7.1) plus the §6.4 `label`. Returns whether ANY of the three changed —
/// `apply_hook_event` turns that into the single `seq` bump.
///
/// The two halves are DECOUPLED on purpose. §6.4's freeze — once a summary has
/// named the session the label stops re-deriving — is a rule about the LABEL,
/// which is a zellij tab name and only meaningfully changes once. `title` and
/// `summary` are row FIELDS the bar renders directly from the store, and the
/// summary column is a rolling one-liner of what the agent is doing now;
/// freezing it at the first value forever defeats the column. So the row
/// fields refresh on every event carrying a tail, and the label keeps its
/// freeze exactly.
///
/// The `dir · branch` prefix is rebuilt from the record so the rule stays in
/// one place: label = `<last-path-component of cwd> · <branch> [· <words>]`.
pub fn refresh_label(
    rec: &mut AgentRecord,
    event: &str,
    payload: &HookPayload,
    jsonl_tail: Option<&str>,
) -> bool {
    let changed = refresh_row_fields(rec, event, payload, jsonl_tail);
    // Once a summary named the LABEL, it stays (§6.4: stop re-scanning). Note
    // this returns `changed`, not `false`: the row fields above are outside
    // the freeze and a frozen-label row must still push their updates.
    if rec.label_source == LabelSource::Summary {
        return changed;
    }
    let dir = rec
        .cwd
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&rec.cwd);
    // sanitize_label THROUGHOUT (fugu 2026-07-21, HIGH): hook-derived labels
    // are baked into launch.kdl for the most-recent row (setup.rs) — a raw
    // backslash/quote from prompt or summary text is a KDL parse error that
    // can brick cold-start launch. Sanitizing the prefix too keeps the
    // byte-for-byte first-prompt gate below aligned with add.rs's creation
    // label, which is built through the same function.
    let prefix = crate::add::sanitize_label(&format!("{dir} · {}", rec.branch));
    // Prefer a summary from the tail (Stop is when summaries appear)… unless
    // it's harness-injected text (#17, defensive: summaries come from the
    // CLI's own compaction, not the prompt path that triggered the live
    // defect, but the same "never earn from injected text" rule applies).
    if let Some(summary) = jsonl_tail
        .and_then(summary_from_tail)
        .filter(|s| !is_harness_injected(s))
    {
        let label = crate::add::sanitize_label(&format!("{prefix} · {}", first_words(&summary)));
        rec.label = label;
        rec.label_source = LabelSource::Summary;
        return true;
    }
    // …else, on the first prompt, use the prompt text from the payload.
    // "First" = the label is still the bare `dir · branch` from `clave add`.
    // is_harness_injected (#17) skips the upgrade ENTIRELY for an injected
    // prompt — not strip-and-use-remainder — so label_source stays
    // FirstPrompt and the next REAL prompt still earns the label.
    // (Collapsed into an edition-2024 let-chain — clippy::collapsible_if.)
    if event == "UserPromptSubmit"
        && rec.label == prefix
        && let Some(p) = payload.prompt.as_deref().filter(|p| !p.trim().is_empty())
        && !is_harness_injected(p)
    {
        rec.label = crate::add::sanitize_label(&format!("{prefix} · {}", first_words(p)));
        return true;
    }
    changed
}

/// The row fields the bar renders from the store (design-lock §7.1), refreshed
/// on every event that carries a tail — OUTSIDE §6.4's label freeze (see
/// `refresh_label`). Returns whether either changed, so a no-op event still
/// costs no `seq` bump (§5 forbids no-op pushes).
///
/// Tiers, highest authority first, each held last-non-empty so a `/clear` or a
/// tail that has scrolled past the signal never BLANKS an earned value:
///
/// - `title`   ← `custom-title` — the user's own rename, and nothing else. A
///   wrong title is worse than the blank chip the design already renders.
/// - `summary` ← `away_summary` (the freshest away-period recap, #131), then
///   `ai-title` (Claude's auto-description), falling back to the extinct
///   `type:"summary"` line, and finally — only while `summary` is still
///   EMPTY — to the current prompt, so a live row is never blank before
///   Claude has written anything. Fill-only-when-empty on that last tier: an
///   earned recap or `ai-title` must never regress to prompt text.
fn refresh_row_fields(
    rec: &mut AgentRecord,
    event: &str,
    payload: &HookPayload,
    jsonl_tail: Option<&str>,
) -> bool {
    let mut changed = false;
    // is_harness_injected on both (#17): the "never earn from injected text"
    // rule is about the SOURCE, not about which field it lands in.
    if let Some(t) = jsonl_tail
        .and_then(custom_title_from_tail)
        .filter(|s| !is_harness_injected(s))
    {
        let t = clamp_field(&t, TITLE_MAX_CHARS);
        if !t.is_empty() && rec.title.as_deref() != Some(t.as_str()) {
            rec.title = Some(t);
            changed = true;
        }
    }
    let from_tail = jsonl_tail
        .and_then(|t| {
            away_summary_from_tail(t)
                .or_else(|| ai_title_from_tail(t))
                .or_else(|| summary_from_tail(t))
        })
        .filter(|s| !is_harness_injected(s));
    let seed = (event == "UserPromptSubmit" && rec.summary.is_empty())
        .then_some(payload.prompt.as_deref())
        .flatten()
        .filter(|p| !p.trim().is_empty() && !is_harness_injected(p))
        .map(str::to_owned);
    if let Some(s) = from_tail.or(seed) {
        let s = clamp_field(&s, SUMMARY_MAX_CHARS);
        if !s.is_empty() && rec.summary != s {
            rec.summary = s;
            changed = true;
        }
    }
    changed
}

/// The lifetime bound a status-push child carries (#233). The hook process
/// exits right after spawning, so the bound must live INSIDE the spawned
/// process tree — nothing outside it survives long enough to reap.
#[derive(Debug)]
enum PipeBound {
    /// `timeout`/`gtimeout` found: `<path> <secs> zellij pipe …`.
    Coreutils(std::path::PathBuf),
    /// No coreutils (stock macOS): perl `alarm` — pending alarms survive
    /// exec (POSIX) and SIGALRM's default action terminates, so this bounds
    /// the real pipe client, not a wrapper. Same script as scripts/ct.sh.
    PerlAlarm(std::path::PathBuf),
    /// No wrapper on the machine: today's bare spawn. A push is never
    /// sacrificed to the bound.
    Unbounded,
}

/// Pure half of rung discovery (#233): first hit wins, ct.sh's order.
/// The IO shell gathers the probes; this decides.
fn pipe_bound_ladder(
    timeout: Option<std::path::PathBuf>,
    gtimeout: Option<std::path::PathBuf>,
    perl: Option<std::path::PathBuf>,
) -> PipeBound {
    match (timeout.or(gtimeout), perl) {
        (Some(w), _) => PipeBound::Coreutils(w),
        (None, Some(p)) => PipeBound::PerlAlarm(p),
        (None, None) => PipeBound::Unbounded,
    }
}

/// IO shell of rung discovery: global-PATH probes only (cwd must never
/// satisfy a probe — same discipline as discover.rs), then the pure ladder.
fn discover_pipe_bound() -> PipeBound {
    pipe_bound_ladder(
        which::which_global("timeout").ok(),
        which::which_global("gtimeout").ok(),
        which::which_global("perl").ok(),
    )
}

/// Pure builder for the push child (#233): wraps the `zellij pipe`
/// invocation in the discovered process-level bound. The payload and pipe
/// name pass through untouched; only the outer wrapper varies by rung.
///
/// `session` NAMES THE TARGET, and it is not belt-and-braces (2026-09-10).
/// With no `--session`, zellij's `ActiveSession::One` arm serves the ONLY
/// live session whatever `ZELLIJ_SESSION_NAME` says — the same resolution
/// hole `scripts/ct.sh` exists to close for `zellij action`. This push
/// inherits the pane's env and so had looked safe; it is not, and the drive
/// caught it: a three-row sandbox snapshot fired at the maintainer's live
/// twenty-row fleet, stopped only by `apply_snapshot`'s seq discard, which is
/// an accident of which store had counted higher. Two rounds went chasing the
/// sandbox bar's stale render before the aim was the suspect.
///
/// `None` keeps today's shape exactly — a hook fired outside zellij has no
/// session to name, and there is nothing to guess.
fn bounded_pipe_command(
    zellij: &Path,
    pipe: &str,
    payload: &str,
    bound: &PipeBound,
    secs: u32,
    session: Option<&str>,
) -> Command {
    // BEFORE the subcommand: `--session` is a flag on the `zellij` binary
    // itself, not on `pipe` (zellij-utils-0.44.3 src/cli.rs:52-54).
    let mut zellij_args: Vec<&str> = Vec::new();
    if let Some(name) = session {
        zellij_args.extend(["--session", name]);
    }
    zellij_args.extend(["pipe", "--name", pipe, "--", payload]);
    match bound {
        PipeBound::Coreutils(wrapper) => {
            let mut cmd = Command::new(wrapper);
            cmd.arg(secs.to_string()).arg(zellij).args(zellij_args);
            cmd
        }
        PipeBound::PerlAlarm(perl) => {
            let mut cmd = Command::new(perl);
            cmd.args([
                "-e",
                "alarm shift @ARGV; exec @ARGV or die \"exec failed: $!\\n\"",
            ])
            .arg(secs.to_string())
            .arg(zellij)
            .args(zellij_args);
            cmd
        }
        PipeBound::Unbounded => {
            let mut cmd = Command::new(zellij);
            cmd.args(zellij_args);
            cmd
        }
    }
}

/// The rule for reading a session name out of an env value, split from the
/// read itself so it can be tested (the read cannot be — see `.cargo/
/// mutants.toml`, and `smart_zone`'s note on why env-mutating tests are
/// forbidden here).
///
/// EMPTY IS NOT UNSET, and the asymmetry is the whole content of the
/// function. An exported-but-empty `ZELLIJ_SESSION_NAME` is not a session
/// name; passed to `--session` it would turn a push that today lands
/// correctly into one that dies at argument validation on every event. The
/// same trap `ct.sh` guards on the sandbox roots, from the other direction.
fn session_from_env(raw: Option<String>) -> Option<String> {
    raw.filter(|s| !s.is_empty())
}

/// The session this hook is running inside, as its own pane's env reports it.
/// IO shell over [`session_from_env`]; the discipline is `discover_pipe_bound`
/// over `pipe_bound_ladder`, one function up.
fn own_session() -> Option<String> {
    session_from_env(std::env::var("ZELLIJ_SESSION_NAME").ok())
}

/// Seconds a push child may live (#233). A healthy pipe completes in
/// milliseconds; the footgun-112 orphan spun for days. Matches ct.sh's
/// default — anything seconds-scale kills the defect.
const PUSH_BOUND_SECS: u32 = 15;

/// Fire-and-forget snapshot push (§5). Spawn WITHOUT waiting: `zellij pipe`
/// can dawdle (S1) and a global hook must never block Claude on it. The
/// child is AIMED at this pane's own session by name rather than left to
/// zellij's fallback resolution (see `bounded_pipe_command`); stdio is nulled
/// so nothing leaks into the hook protocol on stdout.
pub fn push_snapshot(snap: &AgentSnapshot) {
    let paths = crate::store::store_paths().ok();
    // Leave the dormant rows out when the last push carried the same ones
    // (`store::wire_for_push`): they are most of the bytes, and every bar
    // decodes every byte.
    let last_pushed = paths.as_ref().and_then(crate::store::last_pushed_tag);
    let wire = crate::store::wire_for_push(snap, last_pushed);
    let Ok(payload) = serde_json::to_string(&wire) else {
        return;
    };
    // WHERE this is allowed to land, decided from the store rather than from
    // the env alone. `own_session` reads `ZELLIJ_SESSION_NAME`, and a process
    // can hold a env naming one session while writing a DIFFERENT session's
    // store — which is not hypothetical: it is what every drive shell inside
    // the maintainer's fleet looks like, and it aimed a sandbox snapshot at
    // his live bar and hung his session (FOOTGUNS #281, again on
    // 2026-09-11). The env is the caller's claim; the store is the fact.
    let aim = aim_push(
        paths.as_ref().map(|p| p.dir.as_path()),
        own_session().as_deref(),
    );
    if let PushAim::Foreign { owner, target } = &aim {
        // DROPPED, and the drop is written down. The whole reason this class
        // of bug cost rounds is that its tell was silence: the wrong bar
        // simply kept rendering, and the right one never heard anything.
        let detail = format!("target={target} store-owner={owner}");
        if let Some(p) = paths.as_ref() {
            crate::evlog::log_event_in(&p.dir, "push-refused", &detail);
        }
        // stderr as well as the log: a hook's stderr is captured in the
        // transcript's hook attachments, which is where a human looking at a
        // misbehaving fleet actually ends up.
        eprintln!("clave: snapshot push refused, {detail}");
        return;
    }
    // Discovered path (codex P2 on PR #29): hooks run as claude's children,
    // whose env may lack the interactive PATH — an off-PATH zellij made every
    // status push a silent no-op. Fire-and-forget stays: failure here must
    // never become a hook failure (§6.5 zero-risk citizen).
    let zellij = crate::discover::tool_path(crate::discover::ToolId::Zellij);
    let spawned = bounded_pipe_command(
        &zellij,
        status_pipe(&wire),
        &payload,
        &discover_pipe_bound(),
        PUSH_BOUND_SECS,
        aim.target(),
    )
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .is_ok();
    // Remember the tag only for a push aimed at a named session. An unaimed
    // push lands where zellij chooses, and fails when several sessions run,
    // so no bar may hold what it carried (review of 4b3d524).
    if let (true, Some(p), Some(_)) = (spawned, paths.as_ref(), aim.target()) {
        crate::store::note_pushed(p, &wire);
    }
}

/// The pipe a payload rides. A slim push has its own, so a bar older than it
/// drops it rather than reading it as the whole fleet
/// ([`clave_types::SLIM_STATUS_PIPE`]).
fn status_pipe(wire: &AgentSnapshot) -> &'static str {
    match wire.dormant {
        clave_types::DormantRows::Omitted { .. } => clave_types::SLIM_STATUS_PIPE,
        clave_types::DormantRows::Included { .. } => clave_types::STATUS_PIPE,
    }
}

/// Where a snapshot push may land.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PushAim {
    /// Aim it at this session by name.
    At(String),
    /// The env names no session, so zellij's own resolution is all there is.
    /// Left alone deliberately: outside a multiplexer there is nothing to
    /// disagree with, and narrowing this is a change to real installs.
    Unaimed,
    /// The env names a session that does not own the store this snapshot
    /// describes. Dropped.
    Foreign { owner: String, target: String },
}

impl PushAim {
    fn target(&self) -> Option<&str> {
        match self {
            PushAim::At(s) => Some(s.as_str()),
            PushAim::Unaimed | PushAim::Foreign { .. } => None,
        }
    }
}

/// Decide a push's destination from the store it came from, refusing the two
/// ways a snapshot can reach a bar that has no business seeing it.
///
/// A SANDBOX store may only be pushed at its own session — the mistake a
/// drive makes, because `CLAVE_STATE_DIR` selects the store and nothing in
/// the env has to agree. A REAL store may not be pushed at a sandbox session
/// — the same mistake inverted, which would show a live fleet inside a test
/// one. Anything clave cannot place (a real store, a real session) is aimed
/// as asked: guessing there would be clave narrowing a working install.
///
/// An UNNAMED target is the same hazard with the claim missing rather than
/// wrong (CodeRabbit, 2026-09-13). Unaimed is not "nowhere": zellij resolves
/// a session of its own choosing, which on the machine that matters is the
/// maintainer's live fleet. A real store still goes unaimed, because clave
/// does not get to predict a real install's session name — but a sandbox
/// store does not have to guess, it knows its owner, and the store is the
/// fact. If that session is not running, the push simply fails, which is the
/// outcome the whole module prefers to a snapshot landing on a live bar.
pub(crate) fn aim_push(state_dir: Option<&Path>, target: Option<&str>) -> PushAim {
    let owner = state_dir.and_then(crate::sandbox::owner_session_of_store);
    let Some(target) = target else {
        return match owner {
            Some(owner) => PushAim::At(owner),
            None => PushAim::Unaimed,
        };
    };
    match owner {
        Some(owner) if owner != target => PushAim::Foreign {
            owner,
            target: target.to_string(),
        },
        Some(_) => PushAim::At(target.to_string()),
        None if crate::sandbox::is_sandbox_session(target) => PushAim::Foreign {
            owner: "<not a sandbox store>".to_string(),
            target: target.to_string(),
        },
        None => PushAim::At(target.to_string()),
    }
}

/// Remember which conversation this row is actually living in (#99). The
/// payload's id is the live one BY DEFINITION — Claude is reporting its own
/// session. Held as `None` when it equals `uuid`, so the field can go BACK
/// to agreeing rather than keep pointing at a superseded conversation.
///
/// GATED ON THE ENV, unlike everything else in the two callers, and the
/// asymmetry is the point. `resolve_row` admits a payload whose `session_id`
/// NAMES A ROW without consulting the env — correctly, that is the ordinary
/// path — but the minted transcript this bug leaves orphaned is listed in
/// `claude --resume`'s own picker, so a Claude started by hand OUTSIDE clave
/// can fire hooks carrying exactly that id. Ungated, its `session_id == uuid`
/// would read as "the two agree again" and WIPE a live pointer that is still
/// true, silently re-arming #99 for the next release. Every other field the
/// callers write describes the event and is corrected by the next one; this
/// pointer is read once, much later, by a process with nothing else to go on.
///
/// Fails closed: no `CLAVE_AGENT_UUID`, or one naming another row, and the
/// pointer simply holds. Worst case is the pre-#99 target, never a wrong one.
///
/// A rotation IS a `/clear`: Claude mints a new id AND starts a new
/// transcript (FOOTGUNS, "ROTATES its session id on `/clear`"). The new
/// conversation genuinely holds nothing, so the battery returns to full on
/// this event rather than waiting for a usage line to exist — and the meter's
/// stamp (#245) goes with it, because the new conversation has not been
/// metered. That is the design statement in code: **the battery measures the
/// conversation the row is IN, never the row's history**, so a near-zero
/// reading straight after a `/clear` is CORRECT and is not to be "fixed".
///
/// `--resume` does NOT rotate (measured, same FOOTGUNS entry), so resuming a
/// conversation correctly keeps its reading.
///
/// Shared by the hook and the statusLine paths, which is why it lives here
/// rather than inline: a `/clear` reaches whichever of the two speaks first.
pub(crate) fn note_live_session(
    rec: &mut AgentRecord,
    uuid: &str,
    live: Option<&str>,
    own_claude: bool,
) {
    let Some(live) = live.filter(|_| own_claude) else {
        return;
    };
    if live != rec.live_session.as_deref().unwrap_or(uuid) {
        rec.context_tokens = Some(0);
        rec.metered_at = 0;
    }
    rec.live_session = (live != uuid).then(|| live.to_string());
}

/// Bucket the row's count into its level and stamp it; returns whether the
/// level moved. Bucketed HERE, in the row's own agent's process, for two
/// reasons: this is where `SMART_ZONE_ENV` means the right thing, and
/// stamping it makes a dormant row free forever after — `snapshot_from` only
/// copies. The fleet's dormant list may eventually hold every conversation
/// the user has ever had.
pub(crate) fn restamp_level(rec: &mut AgentRecord, zone: u32) -> bool {
    let level = rec.context_tokens.map(|t| battery_level(t, zone));
    let moved = rec.context_level != level;
    rec.context_level = level;
    moved
}

/// A row takes a model reading in the raw form both sources carry
/// (`claude-fable-5-1`), stored short. Returns whether a rendered cell moved.
/// Provider is "claude" by construction: both readers are Claude Code's own
/// channels; other providers arrive with their own path, not a guess.
pub fn take_model(rec: &mut AgentRecord, raw: &str) -> bool {
    let short = short_model(raw);
    let mut changed = rec.model.as_deref() != Some(short.as_str());
    rec.model = Some(short);
    if rec.provider.as_deref() != Some("claude") {
        rec.provider = Some("claude".to_string());
        changed = true;
    }
    changed
}

/// The effort twin of `take_model` (`medium` → `md`).
pub fn take_effort(rec: &mut AgentRecord, raw: &str) -> bool {
    let short = short_effort(raw);
    let changed = rec.effort.as_deref() != Some(short.as_str());
    rec.effort = Some(short);
    changed
}

/// The one store mutation a hook event performs, factored out of run_hook's
/// lock closure so it unit-tests against a plain `Store`. Returns whether
/// anything changed; bumps `seq` itself (exactly once) when it did.
///
/// `own_claude` is "the firing Claude carries THIS row's `CLAVE_AGENT_UUID`",
/// and it gates ONE field — `live_session`. Everything else here is driven by
/// an event `resolve_row` already admitted; that pointer is the only value
/// whose lifetime outlives the event, so it is the only one that can be
/// poisoned by a Claude that merely LOOKS like this row's (see the write site
/// below).
pub fn apply_hook_event(
    s: &mut crate::store::Store,
    uuid: &str,
    event: &str,
    payload: &HookPayload,
    jsonl_tail: Option<&str>,
    now: u64,
    own_claude: bool,
) -> bool {
    let Some(rec) = s.agents.get_mut(uuid) else {
        return false; // raced a prune — fine
    };
    let mut changed = false;
    let was = rec.status;
    if let Some(next) = status_for_event(event, payload, rec.status) {
        changed |= rec.status != next;
        rec.status = next;
    }
    // The turn clock. A prompt that starts a turn starts it, typed or not:
    // the spinner counts the work, whoever asked for it. A block inside the
    // turn keeps it, so the answer resumes the same count. Every other status
    // ends the turn, and a zero renders blank.
    //
    // An interrupt is the exception. Esc sends no Stop, so the row still shows
    // Working when the next prompt comes, and the old clock would count the
    // dead turn into the new one. The transcript tells the two apart: after
    // an interrupt its newest record is the CLI's interrupt marker
    // (`interrupted`); a message queued into a live turn finds the turn's own
    // records newest. Measured 2026-10-14: 29 of 40 interrupts were followed
    // by a prompt, and 51 prompts were queued into live turns.
    let fresh = event == "UserPromptSubmit"
        && (was != Status::Working || jsonl_tail.is_some_and(interrupted));
    let turn_started = match rec.status {
        Status::Working if fresh => now,
        Status::Working | Status::NeedsYou => rec.turn_started,
        _ => 0,
    };
    changed |= rec.turn_started != turn_started;
    rec.turn_started = turn_started;
    // The card's `wants` cell (lock §4.7). AFTER the transition, never before:
    // the gate reads the status this event just produced, so the ask and the
    // flag it explains can never disagree by one event.
    changed |= take_wants(rec, payload.message.as_deref());
    // Which conversation the row is living in (#99), and the rotation reset
    // that rides it — see `note_live_session`. Not part of `changed`, on
    // purpose: `with_store_mut` persists the record either way; `changed`
    // gates only the SNAPSHOT PUSH, and the bar renders nothing from the
    // pointer — bumping `seq` for it would push a pipe message that changes
    // no pixel. S7 (#62) rides the same signal, and ORDER MATTERS: the
    // battery reset counts as a change against the count taken BEFORE it.
    let tokens_before = rec.context_tokens;
    note_live_session(rec, uuid, payload.session_id.as_deref(), own_claude);
    // #245: the three cells below have a fresher source when the statusLine
    // is wired — the meter reads the API response a turn end's tail may not
    // yet have had flushed to it. While the meter is speaking the tail keeps quiet
    // on them; `title` and `summary` still read the whole tail
    // (`refresh_label`, below), those stay on the transcript by design.
    let parsed = jsonl_tail;
    let tail = parsed.filter(|_| !crate::statusline::hook_yields(rec, now));
    // The end of a turn hands the floor back, AFTER the yield decision above.
    // A cleared stamp lets the meter's next reading land regardless of its
    // interval, and until it arrives a prompt's tail, post-turn and complete,
    // may speak. Not part of `changed`: the stamp is persisted either way and
    // renders nothing.
    let turn_end = ends_turn(event, payload, was);
    if turn_end {
        rec.metered_at = 0;
    }
    // A tail reaches us only on a prompt or a turn end (`reads_tail`). No
    // tail, or a tail carrying no usage line, HOLDS the previous
    // reading — §5.4 fail-closed. Never invent a measurement.
    //
    // The resting count. The meter paces a working row's count, so the count
    // a turn ends on can be one it held, and the statusLine runs on no turn
    // end (code.claude.com/docs/en/statusline, "When the status line
    // updates"). So the turn end lands the transcript's count over the yield
    // when it is the HIGHER one. Between two compactions of one conversation
    // the context only grows. A compaction shrinks it, and `tokens_from_tail`
    // reads only past the compaction's boundary. A rotation zeroes the stored
    // count first (`note_live_session`). Both readings can lag the truth, and the higher
    // one lags it least. The tail can lack the final response (the CLI drains
    // its transcript writes on a 100 ms timer, and the Stop hook started a
    // median 45 ms after the final message, measured over 2,585 Stops on
    // 2026-10-14); the meter's run 300 ms after that message then lands at
    // once on the cleared stamp.
    if let Some(tokens) = parsed
        .and_then(tokens_from_tail)
        .filter(|&t| tail.is_some() || (turn_end && rec.context_tokens.is_none_or(|have| t > have)))
    {
        rec.context_tokens = Some(tokens);
    }
    // #232: the card's model cell. Same source and cadence as the token
    // reading — the tail the hook already took. Provider is "claude" by
    // construction here: this tail IS a Claude Code transcript. Other
    // providers arrive with their own hook path, not a guess.
    if let Some(raw) = tail.and_then(model_from_tail) {
        changed |= take_model(rec, &raw);
    }
    // The card's effort cell: the top-level `effort` on the same assistant
    // lines the model comes from, stored short (`xh`) the way the model is.
    // A tail without one HOLDS the previous reading — an older Claude Code
    // never wrote the field, and blanking a real reading would be a lie.
    if let Some(raw) = tail.and_then(effort_from_tail) {
        changed |= take_effort(rec, &raw);
    }
    // The card's subagent mark, from the payload: no tail, and so no yield
    // to the meter, which carries no reading of it.
    if let Some(next) = fan_out_reading(event, payload) {
        changed |= rec.subagents != next;
        rec.subagents = next;
    }
    // Which checkout this row is living in (#232's branch and PR cells). Read
    // from `parsed`, not `tail`: the meter has no reading of its own to yield
    // to here. A row that moved keeps a
    // stale `pr_number` for at most one TTL — `pr_is_stale` fires on the
    // branch change itself, so the next hook spawns `pr-sync` for the new
    // question without waiting the 5 minutes out.
    changed |= take_checkout(rec, parsed.and_then(checkout_from_tail));
    // The tree mark, from the same tail, AFTER the cwd has moved: the taker
    // judges containment against the cwd the row now holds. `take_checkout`
    // owns the clear; this only sets.
    changed |= take_worktree(rec, parsed.and_then(worktree_from_tail));
    let level_moved = restamp_level(rec, smart_zone());
    // BOTH fields gate the push, not just the level. The glyph only moves once
    // per tenth of the zone, but #105 renders the raw count as text — gating on
    // the level alone would leave that text stale for up to a tenth of the zone
    // (15k tokens at the default), which is a bug shipped early rather than a
    // bug avoided. (CodeRabbit and the spec review agreed here, #147.)
    changed |= level_moved || rec.context_tokens != tokens_before;
    // §6.6 / S1 / #39: a TYPED prompt is the ONLY event that reorders. Stop,
    // StopFailure, Notification, PostToolBatch and SessionEnd change the
    // STATUS and nothing else — "claude finishing should not move it up"
    // (maintainer ruling, 2026-07-22). A harness turn is the agent CLI
    // talking to itself: measured 2026-10-14 on the live fleet, 32 of 40
    // turn-starting prompts were harness turns, and each one ranked its row
    // above the rows the human was working in.
    let commitment = event == "UserPromptSubmit" && is_typed(payload);
    let mut commit_tab = None;
    if commitment {
        rec.last_interacted = now; // wall clock: `clave ls`, picker, eager_row
        // §6.6 Design B: a prompt is a user COMMITMENT to the agent's TAB —
        // stamp through the bind, atomically with the bump (a bar-side stamp
        // would race the user switching away).
        commit_tab = rec.tab_id;
        changed = true;
    }
    changed |= refresh_label(rec, event, payload, parsed);
    if !changed {
        return false;
    }
    // The write's own seq IS the commitment ordinal — the §5 pipe contract and
    // the §6.6 row order share one counter (see Store::mint_ord).
    let ord = s.mint_ord();
    if commitment {
        let now_hour = crate::store::unix_hour(now);
        if let Some(tab_id) = commit_tab {
            s.tab_order.insert(tab_id, ord);
            // #232: the wall-clock twin of the ordinal above, stamped at the
            // same site so agent tabs and terminal tabs (`touch_in`) share
            // one truth for "how long ago".
            s.tab_touched.insert(tab_id, now);
            crate::store::bump_bucket(s.tab_buckets.entry(tab_id).or_default(), now_hour);
        }
        if let Some(rec) = s.agents.get_mut(uuid) {
            // Set on the AGENT too, not only the tab: an unbound agent (RC-B,
            // or a prompt landing before `clave bind`) still records its
            // commitment, so the dormant row it becomes on close sorts right
            // even if the prune never lands.
            rec.commit_ord = ord;
            crate::store::bump_bucket(&mut rec.buckets, now_hour);
        }
    }
    true
}

/// The mint half of #226 live adoption: an unknown session speaking from a
/// verified clave pane becomes a row, keyed by its own session id (which IS
/// the minted uuid for an adopted row — no rotation-following needed: a
/// `/clear` mints the successor and the bind eviction hands the tab over).
/// Delegates to `add::mint_record`, the same mint every `clave add` runs, so
/// the ordinal, opener-inheritance/own-buckets seeding and the racing-mint
/// preserve path (`merge_resume_record`) are shared, not re-derived. The
/// label is the byte-exact `<dir> · <branch>` base form run_add mints —
/// `refresh_label` reconstructs that prefix to gate the first-prompt upgrade.
/// Callers hold the store lock; all derivation (git, transcript read) happens
/// outside it.
pub fn mint_adopted(
    s: &mut Store,
    session: &str,
    cwd: &str,
    repo_root: &str,
    branch: &str,
    default_branch: Option<String>,
    own_buckets: Option<std::collections::BTreeMap<u32, u32>>,
) -> String {
    let dir_name = cwd.rsplit('/').next().unwrap_or(cwd);
    let label = crate::add::sanitize_label(&format!("{dir_name} · {branch}"));
    crate::add::mint_record(
        s,
        crate::add::FreshRecordInputs {
            uuid: session,
            cwd,
            repo_root,
            branch,
            label: &label,
            worktree: None,
            default_branch,
            own_buckets,
        },
    );
    session.to_string()
}

/// The pane half of a hook write (#226 live adoption): the association facts,
/// kept apart from [`apply_hook_event`]'s event facts on purpose — they answer
/// different questions ("what is the session doing" vs "where is it running")
/// and only this one needs the verified pane. `pane` is [`adoption_pane`]'s
/// output: already proven to be a pane of clave's own zellij session, which is
/// the ownership test — a claude speaking from a clave pane IS where the row
/// runs, `clave spawn` or not (the S17 reversal). Change-gated; bumps `seq`
/// itself on a real write because the bar renders and joins from `pane_id`
/// (unlike `live_session`, which pushes no pixel).
pub fn apply_hook_pane(
    s: &mut Store,
    uuid: &str,
    event: &str,
    pane: Option<u32>,
    reason: Option<&str>,
    now: u64,
) -> bool {
    let Some(pane) = pane else {
        return false; // unverified pane: never write, never erase
    };
    // Read before the row borrow below: the end carries its tab's ordinal.
    let tab_ords = &s.tab_order;
    let Some(rec) = s.agents.get_mut(uuid) else {
        return false; // raced a prune — fine
    };
    if event == "SessionEnd" {
        // Exit reverts the tab to a terminal tab — but only the pane the row
        // OWNS may say so. A `/clear` needs no carve-out here: its SessionEnd
        // unbinds and the successor session's mint re-claims the tab.
        if rec.pane_id != Some(pane) {
            return false;
        }
        // Standby: a zellij quit fires SessionEnd for every running agent,
        // and the unbind below erases the only record that this row was live
        // (QA run 12), so the end itself stamps it. Only `other` (a killed
        // pane) counts: /exit, /clear, /resume and /logout are the human's
        // own endings (reasons: code.claude.com/docs/en/hooks). No reason is
        // not a quit either, because a false stamp spawns an agent on
        // arrival. A tab close is also `other`; its prune dismisses the
        // stamp by tab.
        if rec.tab_id.is_some() && reason == Some("other") {
            rec.standby_stamp = Some(crate::store::Standby {
                since: now,
                tab: rec.tab_id,
            });
        }
        // The row inherits its tab's ordinal before the unbind drops the
        // tab, the prune's own rule (R2): without it, a standby row came back
        // ranked by an older prompt, not where the quit left it.
        let carried = rec.tab_id.and_then(|t| tab_ords.get(&t)).copied();
        rec.commit_ord = rec.commit_ord.max(carried.unwrap_or(0));
        rec.pane_id = None;
        rec.tab_id = None;
        s.seq += 1; // monotonic pipe contract (§5)
        return true;
    }
    if rec.pane_id == Some(pane) {
        return false; // re-registration of the same pane: free
    }
    rec.pane_id = Some(pane);
    s.seq += 1; // monotonic pipe contract (§5)
    true
}

/// Which store row does this hook event belong to? (#97)
///
/// The payload's `session_id` is authoritative WHEN IT IS A ROW — that is the
/// ordinary case and it stays first, so nothing about a normal session changes.
/// It stops being a row the moment Claude rotates the id on resume: a new
/// session id, a new transcript, and a lookup that misses. Before this, the
/// hook returned there, so the row never stamped `last_interacted` again and
/// silently stopped rising. Measured on a live tab: 5.9 days stale while in
/// active use.
///
/// `CLAVE_AGENT_UUID` is the fallback, set by `clave spawn` before the exec.
/// One pane holds exactly one Claude — clave EXECS it, nothing nests another
/// (maintainer ruling on #180, 2026-08-17) — so the inherited uuid IS the
/// pane speaking, and it survives every rotation, `/clear` and resume inside
/// that pane. (A pid gate used to stand here against a hypothetical nested
/// `claude`; it was the fail-closed trap — any process-tree shape it did not
/// predict froze the row forever, and the nested case it defended against
/// cannot occur.)
///
/// Both routes fail CLOSED on a Claude started outside `clave spawn`: no env
/// uuid, and a session id naming no row, resolve to `None` and the hook
/// declines. The worst case is a row that does not track, never a write to
/// the wrong row — an env value can only name the row whose pane exported it.
///
/// It is NOT a defence against a deliberate local caller: exporting a real
/// row's uuid passes. That is same-user, same-machine, and anyone who can set
/// that environment can write the store directly, so there is nothing to
/// defend.
///
/// Pure and total: map lookups, no I/O, so it is safe on the §6.5 fast path
/// and testable without a store on disk.
/// The firing pane's identity, trusted only inside clave's OWN zellij session
/// (#226). Pane ids are session-scoped — a claude in another zellij session
/// (or under no zellij) would contribute a foreign id that the bar's pane→tab
/// join resolves against a stranger, so both legs fail closed. Pure kernel,
/// env.rs-style: the env reader stays thin so tests never touch real env vars.
pub fn adoption_pane_from(
    zellij_session: Option<String>,
    zellij_pane: Option<String>,
    own_session: &str,
) -> Option<u32> {
    if zellij_session.as_deref() != Some(own_session) {
        return None;
    }
    zellij_pane?.parse().ok()
}

/// Env half of [`adoption_pane_from`] — two `getenv` calls, safe on the §6.5
/// fast path and always done OUTSIDE the store lock.
fn adoption_pane() -> Option<u32> {
    adoption_pane_from(
        std::env::var("ZELLIJ_SESSION_NAME").ok(),
        std::env::var("ZELLIJ_PANE_ID").ok(),
        &crate::env::session_name(),
    )
}

pub fn resolve_row(store: &Store, session: Option<&str>, env_uuid: Option<&str>) -> Option<String> {
    session
        .filter(|s| store.agents.contains_key(*s))
        .or(env_uuid.filter(|e| store.agents.contains_key(*e)))
        .map(str::to_string)
}

/// The transcript to read for this event, validated (#87 / S4 §4.3a/d).
///
/// Prefers the payload's `transcript_path`, which is the only source that is
/// right under BOTH failure modes of the derived path: a relocated session
/// (the file moves) and a rotated session id (a `/clear` starts a
/// new file, #97). Deriving from `rec.cwd` + `uuid` misses both.
///
/// `transcript_path` is untrusted input whose CONTENTS get written into the
/// store, so it is canonicalized and then confined:
///
/// - it must resolve inside `<claude_config_dir>/projects` — canonicalized on
///   both sides, so `..` traversal and symlinks out are refused after
///   resolution rather than by string inspection;
/// - its filename must be `<session_id>.jsonl`. Note precisely what that does
///   and does not buy: both the path and the id come from the SAME payload, so
///   this is self-CONSISTENCY, not verification — a liar can lie consistently.
///   #87 specified the store row's uuid here, which would be a real binding;
///   rotation makes that impossible, because the live file is not named after
///   the minted uuid. What still holds is confinement plus [`resolve_row`]'s
///   admission (the payload id named a row, or the pane's own env did), so the
///   worst case is grafting another transcript from the same tree, not an
///   arbitrary file read. When `session_id` DOES name a row the binding is
///   strong again, because then `uuid == session`.
///
/// **Returns `None` rather than the derived path when the row was reached by
/// rotation** (`uuid != session`). A hook must never fail hard — that blocks
/// the agent (§6.5) — but for a rotated row the derived path names the
/// PRE-ROTATION transcript, which still exists and still reads, so falling
/// back to it would roll `title`/`summary` out of an abandoned conversation.
/// That is the "subtler bug than the freeze" this branch's first attempt
/// named, and #87's fall-back-to-derived rule predates rotation being known:
/// it assumed the fallback was a MISSING file, not a stale-but-readable one.
/// No tail this event means the held values stand, which is S4 §5.4's
/// fail-closed rule. (opus review, #98)
pub fn resolve_transcript(
    claude_dir: &Path,
    payload_path: Option<&str>,
    session: Option<&str>,
    rec_cwd: &str,
    uuid: &str,
) -> Option<std::path::PathBuf> {
    // Safe only while the row was reached by its own id; see above.
    let derived = || (session == Some(uuid)).then(|| jsonl_path(claude_dir, rec_cwd, uuid));
    let (Some(raw), Some(session)) = (payload_path, session) else {
        return derived();
    };
    // The root must be ABSOLUTE before it can confine anything. `run_hook`
    // builds `claude_dir` with `unwrap_or_default()`, so a failure there gives
    // an EMPTY path — `join("projects")` would then be the relative `projects`,
    // and `canonicalize` resolves that against the hook's working directory,
    // which is the agent's cwd. A `projects` directory sitting there would
    // become the confinement root, and every check below would pass for a file
    // the payload chose. The derived path degrades in the same case, but a
    // wrong derived read is a MISSING tail; a wrong root is an accepted read of
    // an attacker-named file whose contents reach the store. (CodeRabbit, #98)
    if !claude_dir.is_absolute() {
        return derived();
    }
    let root = match std::fs::canonicalize(claude_dir.join("projects")) {
        Ok(r) => r,
        Err(_) => return derived(),
    };
    match std::fs::canonicalize(raw) {
        Ok(p)
            if p.starts_with(&root)
                && p.file_name()
                    .is_some_and(|f| f == format!("{session}.jsonl").as_str()) =>
        {
            Some(p)
        }
        _ => derived(),
    }
}

/// The #226 mint inputs, derived OUTSIDE the store lock: git identity is a
/// subprocess and the buckets read a whole transcript — neither belongs under
/// the flock (§6.5). `None` = the cwd is gone or unusable; adoption declines
/// and the hook stays a no-op, never an error.
struct AdoptionInputs {
    cwd: String,
    repo_root: String,
    branch: String,
    default_branch: Option<String>,
    own_buckets: Option<std::collections::BTreeMap<u32, u32>>,
}

fn adoption_inputs(raw_cwd: &str, claude_dir: &Path, session: &str) -> Option<AdoptionInputs> {
    // Canonicalize FIRST (S0b) — everything downstream keys off the physical
    // path, exactly as run_add does for a picked dir.
    let physical = std::fs::canonicalize(raw_cwd).ok()?;
    let cwd = physical.to_str()?.to_string();
    // A cwd `validate_cwd` would refuse to bake must decline ADOPTION too:
    // the adopted row becomes the newest, so launch's eager pick would reject
    // the stored cwd on every later `clave` start (#227 review).
    crate::add::validate_cwd(&cwd).ok()?;
    let git = crate::discover::tool_path(crate::discover::ToolId::Git);
    let repo_root = crate::add::cmd_stdout(&git, &["-C", &cwd, "rev-parse", "--show-toplevel"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| cwd.clone()); // non-repo dirs are fine
    let branch = crate::add::cmd_stdout(&git, &["-C", &cwd, "rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "-".to_string());
    // Same derivation as `clave add` (#227 review): without it a `trunk`
    // repo's ordinary checkout renders as a branch glyph only on adopted
    // rows. `None` stays a real answer (non-repo, local-only repo).
    let default_branch = crate::add::resolve_default_branch(&git, &repo_root);
    // The jsonl is the source of truth: an adopted conversation enters at its
    // own transcript-derived weight, same ruling as the resume picker's mint.
    let own_buckets = crate::backfill::derive_for_row(
        claude_dir,
        &[cwd.as_str(), repo_root.as_str()],
        session,
        None,
        crate::store::unix_hour(now_unix()),
    );
    Some(AdoptionInputs {
        cwd,
        repo_root,
        branch,
        default_branch,
        own_buckets,
    })
}

/// Whether this tool batch leaves the store exactly as it is: both halves of
/// the hook write, run on a lock-free copy and compared whole. The whole
/// store, not the `changed` bools: those gate the PUSH, and a write can
/// persist a field that pushes nothing (`live_session`).
///
/// PostToolBatch only. It fires once per tool batch of every agent, so it is
/// the one event whose lock is worth saving. The copy's "no" is final, and a
/// writer that holds the lock can still change the row after the copy was
/// read. For a batch that is safe: it clears only red, and red clears only
/// after a person answered, seconds after the ask's write landed. For any
/// other event it is not (PR #285 review): a SubagentStart that read the copy
/// before a SubagentStop's write would exit, and the mark would stay down
/// while the new agent runs. So every other event takes the lock and decides
/// there. A row the store does not hold is an adoption, and its mint is the
/// change.
fn changes_nothing(
    store: &Store,
    uuid: &str,
    event: &str,
    payload: &HookPayload,
    pane: Option<u32>,
    now: u64,
    own_claude: bool,
) -> bool {
    if event != "PostToolBatch" || !store.agents.contains_key(uuid) {
        return false;
    }
    let mut probe = store.clone();
    apply_hook_event(&mut probe, uuid, event, payload, None, now, own_claude);
    apply_hook_pane(
        &mut probe,
        uuid,
        event,
        pane,
        payload.reason.as_deref(),
        now,
    );
    probe == *store
}

/// Whether this event owes the row a `pr-sync`: its PR cache is stale, and
/// the event is one of the five clave registered before PR #285.
/// PostToolBatch fires once per tool batch, so a burst of batches inside one
/// `gh` call would spawn a `gh` for each. The subagent events and StopFailure
/// add nothing the five do not already cover: the turn's prompt, its Stop
/// and its notifications.
fn owes_pr_sync(event: &str, rec: &AgentRecord, now: u64) -> bool {
    matches!(
        event,
        "UserPromptSubmit" | "Stop" | "Notification" | "SessionEnd" | "SessionStart"
    ) && crate::pr::pr_is_stale(rec, now)
}

/// Whether this event reads the transcript's tail: a prompt (the label, the
/// interrupt marker), a Stop (the summary, the checkout), and any other turn
/// end (the resting count). The rest read nothing, because the read runs
/// under the lock.
fn reads_tail(event: &str, payload: &HookPayload, current: Status) -> bool {
    matches!(event, "Stop" | "UserPromptSubmit") || ends_turn(event, payload, current)
}

/// The whole hook flow. Errors bubble up ONLY so main can log them to
/// stderr — main exits 0 no matter what (Global Constraint).
pub fn run_hook(event: &str, stdin_json: &str) -> Result<()> {
    let payload: HookPayload = serde_json::from_str(stdin_json).unwrap_or_default();
    if from_a_subagent(event, &payload) {
        return Ok(());
    }
    let session = payload.session_id.clone();
    let paths = store_paths()?;
    // FAST PATH (§6.5): lock-free read; untracked session → exit immediately.
    // clave must never serialize unrelated sessions' hooks behind its lock.
    // `resolve_row` keeps that property — map lookups, no I/O. A session
    // outside the fleet carries no `CLAVE_AGENT_UUID` and its id names no
    // row, so it never reaches the lock — UNLESS it speaks from a verified
    // clave pane, which is #226's adoption gate: a claude the user ran by
    // hand inside the clave session joins the fleet instead of being ignored.
    // Claudes elsewhere on the box keep the lock-free exit unchanged.
    let env_uuid = std::env::var(clave_types::AGENT_UUID_ENV).ok();
    let pane = adoption_pane();
    let store = read_store(&paths)?;
    let resolved = resolve_row(&store, session.as_deref(), env_uuid.as_deref());
    let adopting = resolved.is_none() && pane.is_some();
    let Some(uuid) = resolved.or_else(|| adopting.then(|| session.clone()).flatten()) else {
        return Ok(());
    };
    // §6.9: claude_config_dir() (not raw home) so the sandbox override
    // reaches the same jsonl tree real claude processes write to.
    let claude_dir = crate::env::claude_config_dir().unwrap_or_default();
    // Adoption derivation (git, transcript read) runs OUTSIDE the lock; a
    // missing/unusable cwd declines the mint and the hook stays a no-op.
    let mint = if adopting {
        let Some(m) = payload
            .cwd
            .as_deref()
            .and_then(|c| adoption_inputs(c, &claude_dir, &uuid))
        else {
            return Ok(());
        };
        Some(m)
    } else {
        None
    };
    let now = now_unix();
    let own_claude = env_uuid.as_deref() == Some(uuid.as_str());
    // The second fast path, the `statusline::meter` pattern: a tool batch on
    // a row that is not red moves nothing, so it exits before the lock. It
    // owes no PR sync either (`owes_pr_sync`).
    if changes_nothing(&store, &uuid, event, &payload, pane, now, own_claude) {
        return Ok(());
    }
    let (snap, pr_stale) = with_store_mut(&paths, |s| {
        if let Some(m) = &mint {
            // Re-checked under the flock: a racing hook may have minted first,
            // in which case mint_adopted lands on the preserve path anyway.
            if !s.agents.contains_key(&uuid) {
                mint_adopted(
                    s,
                    &uuid,
                    &m.cwd,
                    &m.repo_root,
                    &m.branch,
                    m.default_branch.clone(),
                    m.own_buckets.clone(),
                );
            }
        }
        // The tail read is gated on the EVENT only — no longer on
        // `label_source`. `title` and `summary` roll for the whole life of a
        // row (design-lock §7.1), so gating the read on "the label has not
        // been earned yet" would freeze the bar's two live columns the moment
        // the label froze — the regression this exists to remove. Cost is one
        // [`TAIL_BYTES`] read on a prompt, a Stop, and a turn end that is not
        // a Stop (`ends_turn`), well inside the §6.5 hook budget; the other
        // events read nothing.
        let tail = s.agents.get(&uuid).and_then(|rec| {
            // Event gate FIRST. `resolve_transcript` does up to two
            // `canonicalize` calls, and this closure runs while
            // `with_store_mut` holds the exclusive flock — computing a path
            // only to discard it would put filesystem syscalls under the lock
            // on every `PreToolUse`, i.e. on every tool call of every tracked
            // agent. The comment above promises the other events read nothing;
            // this is what makes that true. (CodeRabbit, #98 — the previous
            // ordering was a regression against the code this replaced.)
            if !reads_tail(event, &payload, rec.status) {
                return None;
            }
            // The payload names its own CURRENT transcript, so this is right
            // through both a relocation and an id rotation without the store
            // having to remember anything (#87 dissolves #97's read half).
            resolve_transcript(
                &claude_dir,
                payload.transcript_path.as_deref(),
                session.as_deref(),
                &rec.cwd,
                &uuid,
            )
            .and_then(|path| read_tail(&path, TAIL_BYTES))
        });
        let mut changed =
            apply_hook_event(s, &uuid, event, &payload, tail.as_deref(), now, own_claude);
        // The pane half (#226): register on any event, revert on SessionEnd —
        // pane-verified, change-gated, and the bar renders from it. A fresh
        // mint always lands here too (row pane None → Some), so an adoption
        // pushes even when the event itself moved nothing.
        changed |= apply_hook_pane(s, &uuid, event, pane, payload.reason.as_deref(), now);
        // Read-only (#232): comparing two integers/strings against the row
        // this same write just touched. NOT a network call — that discipline
        // is the whole point (§6.5) — just the decision whether one is owed.
        let pr_stale = s
            .agents
            .get(&uuid)
            .is_some_and(|rec| owes_pr_sync(event, rec, now));
        (changed.then(|| snapshot_from(s)), pr_stale)
    })?;
    if let Some(snap) = snap {
        push_snapshot(&snap);
    }
    // Spawned OUTSIDE the flock — `with_store_mut` above has already
    // returned, so `pr-sync` (its own locked RMW) can never deadlock against
    // this hook's lock. `pr-sync` re-checks staleness itself, so a hook that
    // fires again before `pr-sync` has answered spawns harmlessly again;
    // the TTL is what keeps `gh` from being asked twice for the same answer.
    if pr_stale {
        crate::pr::spawn_pr_sync(&uuid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn args_of(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_str().unwrap().to_string())
            .collect()
    }

    /// A sandbox store's snapshot, aimed at the maintainer's live fleet. This
    /// is the 2026-09-11 incident exactly: `CLAVE_STATE_DIR` selected the
    /// sandbox, `ZELLIJ_SESSION_NAME` still named his session, and the push
    /// went where the env said. The store is the fact, so it is refused.
    #[test]
    fn a_sandbox_snapshot_may_not_be_pushed_at_the_maintainers_fleet() {
        let sandbox = PathBuf::from("/home/u/.local/state/clave-dev-triple-card/state");
        assert_eq!(
            aim_push(Some(&sandbox), Some("clave")),
            PushAim::Foreign {
                owner: "clave-test-triple-card".to_string(),
                target: "clave".to_string(),
            }
        );
    }

    /// And one sandbox may not reach ANOTHER — each worktree stages its own
    /// instance, and the reaper joins root to session through the same
    /// formatter this inverts.
    #[test]
    fn one_sandbox_snapshot_may_not_be_pushed_at_another_sandbox() {
        let sandbox = PathBuf::from("/home/u/.local/state/clave-dev-triple-card/state");
        assert!(matches!(
            aim_push(Some(&sandbox), Some("clave-test-prune-wt")),
            PushAim::Foreign { .. }
        ));
    }

    #[test]
    fn a_sandbox_snapshot_aimed_at_its_own_session_is_aimed_as_asked() {
        let sandbox = PathBuf::from("/home/u/.local/state/clave-dev-triple-card/state");
        assert_eq!(
            aim_push(Some(&sandbox), Some("clave-test-triple-card")),
            PushAim::At("clave-test-triple-card".to_string())
        );
    }

    /// The main checkout's own instance keeps the bare names, and
    /// `key_from_root_name` refuses to key it on purpose. It is still owned,
    /// and this guard still applies to it.
    #[test]
    fn the_main_checkout_instance_is_owned_too() {
        let main = PathBuf::from("/home/u/.local/state/clave-dev/state");
        assert_eq!(
            aim_push(Some(&main), Some("clave-test")),
            PushAim::At("clave-test".to_string())
        );
        assert!(matches!(
            aim_push(Some(&main), Some("clave")),
            PushAim::Foreign { .. }
        ));
    }

    /// The inverse mistake: a REAL store's snapshot pushed into a test
    /// session, which would render a live fleet inside a sandbox.
    #[test]
    fn a_real_snapshot_may_not_be_pushed_at_a_sandbox_bar() {
        let real = PathBuf::from("/home/u/.local/state/clave");
        assert!(matches!(
            aim_push(Some(&real), Some("clave-test-triple-card")),
            PushAim::Foreign { .. }
        ));
    }

    /// What must keep working: a real install, aimed at its own session.
    /// clave cannot predict a real session's name, so an unplaceable pair is
    /// aimed as asked — narrowing here would be clave breaking working
    /// installs to protect a drive.
    #[test]
    fn a_real_install_is_aimed_as_asked() {
        let real = PathBuf::from("/home/u/.local/state/clave");
        assert_eq!(
            aim_push(Some(&real), Some("clave")),
            PushAim::At("clave".to_string())
        );
        assert_eq!(
            aim_push(Some(&real), Some("some-other-multiplexer-session")),
            PushAim::At("some-other-multiplexer-session".to_string())
        );
    }

    /// No session in the env. A REAL store is left to zellij's own
    /// resolution — outside a multiplexer there is nothing to disagree with,
    /// and clave cannot predict a real install's session name.
    ///
    /// A SANDBOX store is not left there, and the difference is the point:
    /// unaimed means zellij picks, and what it picks on this machine is the
    /// maintainer's live fleet. The store knows its owner, so the push goes
    /// there or nowhere.
    #[test]
    fn an_unnamed_target_follows_the_store_when_the_store_knows_its_own() {
        let sandbox = PathBuf::from("/home/u/.local/state/clave-dev-triple-card/state");
        assert_eq!(
            aim_push(Some(&sandbox), None),
            PushAim::At("clave-test-triple-card".to_string())
        );
        let real = PathBuf::from("/home/u/.local/share/clave");
        assert_eq!(aim_push(Some(&real), None), PushAim::Unaimed);
        assert_eq!(aim_push(None, None), PushAim::Unaimed);
    }

    /// A refused aim carries NO target, so the spawn below it cannot
    /// accidentally inherit zellij's fallback resolution — the very path
    /// that made this class of bug reach a live fleet in the first place.
    #[test]
    fn a_refused_aim_hands_the_spawn_no_target() {
        let foreign = PushAim::Foreign {
            owner: "clave-test-triple-card".to_string(),
            target: "clave".to_string(),
        };
        assert_eq!(foreign.target(), None);
        assert_eq!(PushAim::Unaimed.target(), None);
        assert_eq!(
            PushAim::At("clave-test-x".to_string()).target(),
            Some("clave-test-x")
        );
    }

    /// Matched on the whole segment. A real install called `clave-testbed`
    /// is not a sandbox, and reading it as one would refuse its own pushes.
    #[test]
    fn a_sandbox_session_is_matched_on_the_segment_not_the_prefix() {
        assert!(crate::sandbox::is_sandbox_session("clave-test"));
        assert!(crate::sandbox::is_sandbox_session("clave-test-triple-card"));
        assert!(!crate::sandbox::is_sandbox_session("clave-testbed"));
        assert!(!crate::sandbox::is_sandbox_session("clave"));
    }

    /// No coreutils (stock macOS): a pending perl `alarm` survives exec and
    /// SIGALRM terminates by default, so the alarm bounds the REAL pipe
    /// client, not a wrapper. Expected script is ct.sh's ratified ladder.
    #[test]
    fn the_perl_rung_alarms_the_real_pipe_client_not_a_wrapper() {
        let cmd = bounded_pipe_command(
            Path::new("/opt/zellij"),
            "clave-status",
            "p",
            &PipeBound::PerlAlarm(PathBuf::from("/usr/bin/perl")),
            15,
            None,
        );
        assert_eq!(cmd.get_program(), "/usr/bin/perl");
        assert_eq!(
            args_of(&cmd),
            [
                "-e",
                "alarm shift @ARGV; exec @ARGV or die \"exec failed: $!\\n\"",
                "15",
                "/opt/zellij",
                "pipe",
                "--name",
                "clave-status",
                "--",
                "p"
            ]
        );
    }

    /// The arg-shape tests above prove what WOULD be spawned; this proves
    /// the bound is real: the machine's own discovered rung, a fake pipe
    /// client that sleeps forever, a 1-second bound — the child must die.
    /// (The footgun-112 orphan spun for TWO DAYS; this is its coffin lid.)
    #[test]
    fn the_bound_actually_kills_a_wedged_pipe_client() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("wedged-zellij");
        std::fs::write(&fake, "#!/bin/sh\nsleep 300\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bound = discover_pipe_bound();
        assert!(
            !matches!(bound, PipeBound::Unbounded),
            "dev/CI machines must discover a rung (perl at minimum)"
        );

        let mut child = bounded_pipe_command(&fake, "clave-status", "p", &bound, 1, None)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                break; // bounded: the wedged client died on its own
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                panic!("bound did not fire: wedged pipe client still alive after 10s");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// The rung order is ct.sh's: timeout → gtimeout → perl → bare. Each
    /// row removes the preferred rung and the choice slides down one.
    #[test]
    fn the_ladder_prefers_timeout_then_gtimeout_then_perl_then_bare() {
        let t = || Some(PathBuf::from("/t"));
        let g = || Some(PathBuf::from("/g"));
        let p = || Some(PathBuf::from("/p"));
        assert!(matches!(
            pipe_bound_ladder(t(), g(), p()),
            PipeBound::Coreutils(w) if w == Path::new("/t")
        ));
        assert!(matches!(
            pipe_bound_ladder(None, g(), p()),
            PipeBound::Coreutils(w) if w == Path::new("/g")
        ));
        assert!(matches!(
            pipe_bound_ladder(None, None, p()),
            PipeBound::PerlAlarm(w) if w == Path::new("/p")
        ));
        assert!(matches!(
            pipe_bound_ladder(None, None, None),
            PipeBound::Unbounded
        ));
    }

    /// **The push must AIM.** Without `--session`, zellij's one-live-session
    /// arm serves whichever session exists, ignoring `ZELLIJ_SESSION_NAME`
    /// entirely — so a hook carefully pointed at a sandbox store still fired
    /// its snapshot at the maintainer's live fleet. It happened on the
    /// four-line card's drive, and only `apply_snapshot`'s seq discard stopped
    /// a three-row snapshot landing on a twenty-row session.
    ///
    /// Asserted at every rung, because the flag sits on the `zellij` binary
    /// rather than on `pipe` and the two wrapper rungs each build that argv
    /// separately — a fix applied to one of three is the same bug with better
    /// odds.
    #[test]
    fn every_rung_aims_the_push_at_the_session_it_names() {
        let rungs = [
            PipeBound::Unbounded,
            PipeBound::Coreutils(PathBuf::from("/usr/bin/timeout")),
            PipeBound::PerlAlarm(PathBuf::from("/usr/bin/perl")),
        ];
        for bound in &rungs {
            let cmd = bounded_pipe_command(
                Path::new("/opt/zellij"),
                "clave-status",
                "p",
                bound,
                15,
                Some("clave-test-triple-card"),
            );
            let args = args_of(&cmd);
            let at = args
                .iter()
                .position(|a| a == "--session")
                .unwrap_or_else(|| panic!("{bound:?} sent an unaimed push: {args:?}"));
            assert_eq!(args[at + 1], "clave-test-triple-card");
            // BEFORE the subcommand. `zellij pipe --session x` is not the same
            // command — `pipe` has no such flag, and the push would die at
            // argument parsing on every event.
            let pipe = args
                .iter()
                .position(|a| a == "pipe")
                .expect("the pipe verb");
            assert!(
                at + 1 < pipe,
                "{bound:?} put --session after the subcommand: {args:?}"
            );
        }
    }

    /// Empty is not unset. An exported-but-empty `ZELLIJ_SESSION_NAME` names
    /// nothing, and handing it to `--session` would trade a push that lands
    /// for one that dies at argument validation on every hook event — a
    /// working feature turned off by a variable someone cleared.
    #[test]
    fn an_empty_session_variable_names_no_session() {
        assert_eq!(session_from_env(None), None);
        assert_eq!(session_from_env(Some(String::new())), None);
        assert_eq!(
            session_from_env(Some("clave-test".into())).as_deref(),
            Some("clave-test")
        );
    }

    /// `None` leaves the argv exactly as it was. A hook fired outside zellij
    /// has no session to name, and inventing one would turn a push that today
    /// does nothing into a push that errors — worse, and for no gain.
    #[test]
    fn a_hook_outside_zellij_sends_the_same_unaimed_push_it_always_did() {
        let cmd = bounded_pipe_command(
            Path::new("/opt/zellij"),
            "clave-status",
            "p",
            &PipeBound::Unbounded,
            15,
            None,
        );
        assert_eq!(args_of(&cmd), ["pipe", "--name", "clave-status", "--", "p"]);
    }

    /// A 0.5.5 bar listens only on `clave-status` and reads every push there
    /// as the whole fleet. A slim push on that name would drop its dormant
    /// rows, so a slim push rides its own name, which an old bar never hears.
    #[test]
    fn a_slim_push_rides_its_own_pipe_and_a_whole_push_rides_the_old_one() {
        let mut wire: AgentSnapshot = serde_json::from_str(r#"{"seq":1,"agents":[]}"#).unwrap();
        assert_eq!(status_pipe(&wire), "clave-status");
        wire.dormant = clave_types::DormantRows::Omitted { tag: 7 };
        assert_eq!(status_pipe(&wire), "clave-status-slim");
        let cmd = bounded_pipe_command(
            Path::new("/opt/zellij"),
            status_pipe(&wire),
            "p",
            &PipeBound::Unbounded,
            15,
            None,
        );
        assert_eq!(
            args_of(&cmd),
            ["pipe", "--name", "clave-status-slim", "--", "p"]
        );
    }

    /// A machine with no wrapper at all keeps TODAY's behavior — a status
    /// push is never sacrificed to the bound (#233 story 5).
    #[test]
    fn no_rung_degrades_to_the_bare_unbounded_pipe() {
        let cmd = bounded_pipe_command(
            Path::new("/opt/zellij"),
            "clave-status",
            "p",
            &PipeBound::Unbounded,
            15,
            None,
        );
        assert_eq!(cmd.get_program(), "/opt/zellij");
        assert_eq!(args_of(&cmd), ["pipe", "--name", "clave-status", "--", "p"]);
    }

    /// Fix 2 (#233): the push child must carry its own lifetime bound —
    /// the hook process exits immediately, so nothing outside the child's
    /// process tree can reap it. Coreutils rung: `timeout <secs> zellij …`.
    #[test]
    fn a_coreutils_rung_wraps_the_pipe_in_a_process_level_bound() {
        let cmd = bounded_pipe_command(
            Path::new("/opt/zellij"),
            "clave-status",
            r#"{"agents":[]}"#,
            &PipeBound::Coreutils(PathBuf::from("/usr/bin/timeout")),
            15,
            None,
        );
        assert_eq!(cmd.get_program(), "/usr/bin/timeout");
        assert_eq!(
            args_of(&cmd),
            [
                "15",
                "/opt/zellij",
                "pipe",
                "--name",
                "clave-status",
                "--",
                r#"{"agents":[]}"#
            ]
        );
    }

    // Local copy of the Task 2 `rec()` shape: label "x · main", cwd "/x",
    // branch "main", source FirstPrompt — the pre-labelled starting record.
    fn rec(uuid: &str) -> AgentRecord {
        AgentRecord {
            uuid: uuid.into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: Status::Idle,
            last_interacted: 0,
            turn_started: 0,
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            pane_id: None,
            stale: false,
            standby_stamp: None,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
            metered_at: 0,
            buckets: BTreeMap::new(),
            model: None,
            provider: None,
            effort: None,
            pr_number: None,
            pr_checked: 0,
            pr_branch: String::new(),
            wants: None,
            subagents: false,
        }
    }

    /// #99's write half: the row remembers WHICH conversation it is living in.
    ///
    /// `clave spawn` runs before any Claude exists to send a payload, so the
    /// live id has to have been written down beforehand or resurrection has
    /// nothing but the minted uuid — which is the pre-rotation conversation,
    /// confirmed live to lose everything said after a `/clear`.
    #[test]
    fn a_rotated_payload_id_is_remembered_and_an_agreeing_one_clears_it() {
        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        let event = |session: Option<&str>| HookPayload {
            session_id: session.map(str::to_string),
            ..Default::default()
        };
        let live = |s: &Store| s.agents["minted"].live_session.clone();
        // A REGISTERED event (setup.rs's HOOK_EVENTS) that moves nothing else:
        // an unmatched Notification maps to no status and carries no tail, so
        // `changed` reflects the live-id write alone. Using an event clave does
        // not register would prove the property on a path that never fires.
        let quiet = "Notification";
        // Park the battery where a `/clear` would put it. S7 (#62) resets to
        // full on rotation, which IS snapshot-worthy — so without this the
        // assertion below would fail on the battery's change rather than the
        // property under test. Pre-setting it isolates the live-id write, which
        // is what this test is about; `s7_rotation_resets_the_battery_and_pushes`
        // covers the reset itself.
        let r = s.agents.get_mut("minted").unwrap();
        r.context_tokens = Some(0);
        r.context_level = Some(0);

        // A rotated id is recorded — and NOT as a snapshot-worthy change: the
        // bar renders nothing from this field, and `with_store_mut` persists
        // the record whether or not `seq` moves.
        let before = s.seq;
        assert!(!apply_hook_event(
            &mut s,
            "minted",
            quiet,
            &event(Some("rotated")),
            None,
            100,
            true
        ));
        assert_eq!(live(&s).as_deref(), Some("rotated"));
        assert_eq!(s.seq, before, "a live-id write pushes no pipe message");

        // A payload that AGREES with the minted uuid clears it. Without this
        // the row would keep pointing at a superseded file forever — the exact
        // stale-but-readable failure #98 refused for the transcript read.
        apply_hook_event(
            &mut s,
            "minted",
            quiet,
            &event(Some("minted")),
            None,
            101,
            true,
        );
        assert_eq!(live(&s), None);

        // A malformed payload (serde yields `session_id: None`) says nothing
        // about which conversation is live, so it must not erase what does.
        apply_hook_event(
            &mut s,
            "minted",
            quiet,
            &event(Some("rotated")),
            None,
            102,
            true,
        );
        apply_hook_event(&mut s, "minted", quiet, &event(None), None, 103, true);
        assert_eq!(live(&s).as_deref(), Some("rotated"));

        // …and NEITHER does a Claude that is not this agent's. `resolve_row`
        // admits a payload whose id names a row without consulting the gate, so
        // a hand-started `claude --resume <minted>` — the orphaned transcript
        // is in Claude's own picker — arrives here looking like agreement. It
        // must not wipe a pointer that is still true, or the next release
        // resurrects on the superseded conversation again.
        apply_hook_event(
            &mut s,
            "minted",
            quiet,
            &event(Some("minted")),
            None,
            104,
            false,
        );
        assert_eq!(live(&s).as_deref(), Some("rotated"));
    }

    /// The probe that lets a quiet tool batch skip the lock. It may say
    /// "nothing" only when the locked write would persist nothing, and that includes a
    /// field that pushes no pixel: a live-id write must still take the lock,
    /// or a rotation is lost and the next release resumes the old file.
    #[test]
    fn only_a_tool_batch_that_moves_nothing_skips_the_lock() {
        let mut s = Store::default();
        let mut r = rec("u");
        r.status = Status::Working;
        r.pane_id = Some(4);
        s.agents.insert("u".into(), r);
        let said = |id: &str, msg: Option<&str>| HookPayload {
            session_id: Some(id.into()),
            message: msg.map(str::to_string),
            ..Default::default()
        };
        let quiet =
            |event, p: &HookPayload, pane| changes_nothing(&s, "u", event, p, pane, 100, true);
        assert!(
            quiet("PostToolBatch", &said("u", None), Some(4)),
            "a batch inside a live turn"
        );
        assert!(
            !quiet("PostToolBatch", &said("u", None), Some(5)),
            "a new pane"
        );
        assert!(
            !quiet("PostToolBatch", &said("rotated", None), Some(4)),
            "a live-id write"
        );
        let mut red = s.clone();
        red.agents.get_mut("u").unwrap().status = Status::NeedsYou;
        assert!(
            !changes_nothing(
                &red,
                "u",
                "PostToolBatch",
                &said("u", None),
                Some(4),
                100,
                true
            ),
            "the answer clears red"
        );
        // Every other event takes the lock, even when the copy says it
        // moves nothing: its "no" could be stale (PR #285 review).
        assert!(
            !quiet("SessionStart", &said("u", None), Some(4)),
            "the pane it holds"
        );
        assert!(
            !quiet("Notification", &said("u", Some("auth ok")), None),
            "no news"
        );
        let started = HookPayload {
            agent_id: Some("a1".into()),
            ..said("u", None)
        };
        let mut lit = s.clone();
        lit.agents.get_mut("u").unwrap().subagents = true;
        assert!(
            !changes_nothing(&lit, "u", "SubagentStart", &started, Some(4), 100, true),
            "a start on a lit mark"
        );
        // A row the store does not hold is an adoption: the mint is the
        // change, though the copy, with no row to move, moves nothing.
        assert!(
            !changes_nothing(
                &s,
                "new",
                "PostToolBatch",
                &said("new", None),
                Some(4),
                100,
                true
            ),
            "an adoption always takes the lock"
        );
    }

    /// Only the five events clave registered before PR #285 spawn `pr-sync`,
    /// and only for a stale cache. A tool batch fires once per batch, and
    /// each spawn may run `gh`.
    #[test]
    fn a_tool_batch_never_asks_for_a_pr_sync() {
        let stale = rec("u1"); // never checked
        let mut fresh = rec("u1");
        fresh.pr_checked = 1000;
        fresh.pr_branch = fresh.branch.clone();
        for ev in [
            "UserPromptSubmit",
            "Stop",
            "Notification",
            "SessionEnd",
            "SessionStart",
        ] {
            assert!(owes_pr_sync(ev, &stale, 1010), "{ev}");
            assert!(!owes_pr_sync(ev, &fresh, 1010), "{ev} on a fresh cache");
        }
        for ev in [
            "PostToolBatch",
            "StopFailure",
            "SubagentStart",
            "SubagentStop",
        ] {
            assert!(!owes_pr_sync(ev, &stale, 1010), "{ev}");
        }
    }

    /// The tail is read under the lock, so only the events that use it read
    /// it: a prompt, a Stop, and any other turn end.
    #[test]
    fn only_a_prompt_or_a_turn_end_reads_the_tail() {
        use Status::*;
        let of_kind = |kind: Option<&str>, agent: Option<&str>| HookPayload {
            notification_type: kind.map(str::to_string),
            agent_id: agent.map(str::to_string),
            ..Default::default()
        };
        let own = of_kind(None, None);
        let sub = of_kind(None, Some("a1"));
        let nag = of_kind(Some("idle_prompt"), None);
        let ask = of_kind(Some("permission_prompt"), None);
        assert!(reads_tail("UserPromptSubmit", &own, Done));
        assert!(reads_tail("Stop", &own, Done), "a Stop on any row");
        assert!(reads_tail("StopFailure", &own, Working));
        assert!(
            reads_tail("Notification", &nag, Working),
            "the nag ends a turn"
        );
        assert!(!reads_tail("Notification", &nag, Done), "the turn is over");
        assert!(!reads_tail("Notification", &ask, Working));
        assert!(
            !reads_tail("StopFailure", &sub, Working),
            "a subagent's error"
        );
        for ev in [
            "PostToolBatch",
            "SessionEnd",
            "SessionStart",
            "SubagentStart",
            "SubagentStop",
        ] {
            assert!(!reads_tail(ev, &own, Working), "{ev}");
        }
    }

    /// #97, the bug this exists to prevent recurring.
    ///
    /// Claude starts a NEW session id and transcript when the pane gets a
    /// fresh conversation, so the payload id stops naming a row. `run_hook`
    /// returned there, and the row silently stopped stamping
    /// `last_interacted` — 5.9 days stale on a tab in active use.
    ///
    /// Why no test caught it: every fixture used ONE uuid for the life of a
    /// row, so rotation was not an input anywhere and there was nothing to
    /// vary. Same family as D23 and #91 — the fixture encoded the assumption
    /// under test.
    #[test]
    fn a_rotated_session_id_resolves_via_the_panes_env_uuid() {
        let mut store = Store::default();
        store.agents.insert("minted".into(), rec("minted"));

        // The ordinary case: a payload id that names a row wins outright,
        // with or without an environment.
        assert_eq!(
            resolve_row(&store, Some("minted"), None).as_deref(),
            Some("minted")
        );
        assert_eq!(
            resolve_row(&store, Some("minted"), Some("other")).as_deref(),
            Some("minted"),
            "a valid payload id must not be overridden by the environment"
        );

        // The rotation: the new id names nothing, the env carries the minted
        // key — the pane's own Claude, so the row is found. No pid gate: one
        // pane holds exactly one Claude (#180 ruling, 2026-08-17), so the
        // inherited uuid IS the pane speaking.
        assert_eq!(
            resolve_row(&store, Some("rotated"), Some("minted")).as_deref(),
            Some("minted")
        );

        // Out-of-band fails CLOSED: a Claude started outside `clave spawn`
        // carries no env uuid, and an env value naming no row is refused.
        assert_eq!(resolve_row(&store, Some("rotated"), None), None);
        assert_eq!(resolve_row(&store, Some("rotated"), Some("bogus")), None);
        assert_eq!(resolve_row(&store, None, None), None);

        // A MALFORMED payload (serde yields `session_id: None`) resolves via
        // the env. This is a deliberate behaviour — the pre-#97 code returned
        // early on a missing session id — and it was untested until the opus
        // review pointed out that a mutation restoring the
        // `session.is_some()` requirement would have survived.
        assert_eq!(
            resolve_row(&store, None, Some("minted")).as_deref(),
            Some("minted"),
            "the agent's own Claude is still its own Claude with unparseable JSON"
        );
    }

    /// #87 / S4 §4.3a/d. The payload names its own current transcript, which
    /// is right through BOTH a relocation and an id rotation — but it arrives
    /// on hook stdin and its contents are written to the store, so it is a
    /// write primitive unless confined.
    ///
    /// Asserts on `resolve_transcript` itself, not on a re-implementation of
    /// its rule. The previous version of this test rebuilt the selection in a
    /// local closure and asserted on the copy, so deleting the real one left
    /// it green — caught in review.
    #[test]
    fn the_transcript_path_is_used_when_confined_and_refused_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path();
        let proj = claude.join("projects").join(crate::munge::munge_cwd("/x"));
        std::fs::create_dir_all(&proj).unwrap();
        for id in ["minted", "rotated"] {
            std::fs::write(proj.join(format!("{id}.jsonl")), "{}\n").unwrap();
        }
        let outside = dir.path().join("outside.jsonl");
        std::fs::write(&outside, "{}\n").unwrap();
        let derived = jsonl_path(claude, "/x", "minted");
        let go =
            |p: Option<&str>, s: Option<&str>| resolve_transcript(claude, p, s, "/x", "minted");

        // The rotation case: the payload points at the LIVE file, and it is
        // taken — no stored field involved, which is why #97 needs none.
        let rotated = proj.join("rotated.jsonl");
        assert_eq!(
            go(rotated.to_str(), Some("rotated")),
            Some(std::fs::canonicalize(&rotated).unwrap())
        );

        // RELOCATION — #87's original motivation, and a gap the opus review
        // found: every other case here lives in the SAME munged dir as the
        // derived path, so a rule constraining accepted paths to
        // `<root>/<munge(rec_cwd)>/` would have passed the whole suite while
        // silently reintroducing the bug this function exists to fix.
        let moved = claude
            .join("projects")
            .join(crate::munge::munge_cwd("/somewhere/else"));
        std::fs::create_dir_all(&moved).unwrap();
        let relocated = moved.join("minted.jsonl");
        std::fs::write(&relocated, "{}\n").unwrap();
        assert_eq!(
            go(relocated.to_str(), Some("minted")),
            Some(std::fs::canonicalize(&relocated).unwrap()),
            "a relocated transcript is still this session's, in any project dir"
        );

        // Absent payload, NOT rotated → the derived path, unchanged.
        assert_eq!(go(None, Some("minted")), Some(derived.clone()));

        // Absent payload WHILE rotated → no tail at all. The derived path
        // would name the pre-rotation transcript, which still exists and
        // still reads, so falling back would roll title/summary out of an
        // abandoned conversation — worse than holding them. (opus review)
        assert_eq!(go(None, Some("rotated")), None);

        // CONFINEMENT. Outside the projects root, and a path whose filename
        // does not belong to the sending session. Both refuse; because these
        // are rotated, refusing means None rather than the stale file.
        assert_eq!(go(outside.to_str(), Some("rotated")), None);
        assert_eq!(
            go(rotated.to_str(), Some("someone-else")),
            None,
            "the path and the id must at least agree with each other"
        );
        // Traversal is refused AFTER canonicalization, not by string match.
        let traversal = proj.join("..").join("..").join("outside.jsonl");
        assert_eq!(go(traversal.to_str(), Some("outside")), None);
        assert_eq!(
            go(Some("/nonexistent/rotated.jsonl"), Some("rotated")),
            None
        );
        // A refused path on a NON-rotated row still gets the derived file:
        // there it is the row's own live transcript, not a stale one.
        assert_eq!(
            go(outside.to_str(), Some("minted")),
            Some(derived),
            "the derived fallback is safe exactly when the row was reached by its own id"
        );
    }

    /// CodeRabbit on #98. `run_hook` builds `claude_dir` with
    /// `unwrap_or_default()`, so a failure there yields an EMPTY path — and
    /// `join("projects")` on empty is the RELATIVE `projects`, which
    /// `canonicalize` resolves against the hook's working directory (the
    /// agent's cwd). A `projects` dir sitting there would have become the
    /// confinement root, and every later check would pass for a file the
    /// payload named. A wrong derived path is a missing tail; a wrong root is
    /// an accepted read whose contents reach the store.
    /// Does NOT mutate the process cwd to build the trap. An earlier version
    /// called `set_current_dir`, which is process-global — cargo runs this
    /// binary multi-threaded, so it was a latent flake for whoever next added
    /// a cwd-dependent test (opus review). The property under test is
    /// "non-absolute root ⇒ never confine", and that is expressible directly:
    /// a relative `claude_dir` reaches the guard whatever the cwd happens to
    /// be, so the assertion holds without touching global state.
    #[test]
    fn a_relative_claude_dir_never_confines_anything() {
        let dir = tempfile::tempdir().unwrap();
        // A real, canonicalizable planted file whose name satisfies the
        // session check — so ONLY the absolute-root guard can refuse it.
        let proj = dir.path().join("projects").join("anything");
        std::fs::create_dir_all(&proj).unwrap();
        let planted = proj.join("evil.jsonl");
        std::fs::write(&planted, "{}\n").unwrap();

        // Both non-absolute shapes: empty (what `unwrap_or_default()` yields)
        // and an ordinary relative path.
        for root in [Path::new(""), Path::new("some/relative/dir")] {
            let got = resolve_transcript(root, planted.to_str(), Some("evil"), "/x", "evil");
            assert_eq!(
                got,
                Some(jsonl_path(root, "/x", "evil")),
                "a non-absolute claude_dir must fall back, never confine"
            );
            assert_ne!(
                got,
                Some(planted.clone()),
                "the planted file must never be accepted"
            );
        }
    }

    /// A harness turn works the row and commits nothing. Measured 2026-10-14
    /// on the live fleet: 32 of 40 turn-starting prompts were the agent CLI
    /// talking to itself, and each one ranked its row and stamped "you last
    /// spoke" as if the human had typed.
    #[test]
    fn a_harness_turn_works_the_row_but_commits_nothing() {
        let mut s = Store::default();
        let mut r = rec("u1");
        r.status = Status::Done;
        r.tab_id = Some(4);
        r.last_interacted = 1000;
        s.agents.insert("u1".into(), r);
        let say = |text: &str| HookPayload {
            session_id: Some("u1".into()),
            prompt: Some(text.into()),
            ..Default::default()
        };
        let peer = say("Another Claude session sent a message:\n<cross-session-message>");
        let ups = "UserPromptSubmit";

        // On a finished row it starts a turn: Working, and the clock runs.
        assert!(apply_hook_event(&mut s, "u1", ups, &peer, None, 2000, true));
        let r = &s.agents["u1"];
        assert_eq!((r.status, r.turn_started), (Status::Working, 2000));
        assert_eq!(r.last_interacted, 1000, "nobody typed");
        assert_eq!(r.commit_ord, 0, "no ordinal");
        assert!(r.buckets.is_empty(), "no frecency");
        assert!(s.tab_order.is_empty() && s.tab_buckets.is_empty());
        assert!(s.tab_touched.is_empty());

        // On a working row it changes nothing, so it pushes nothing. Every
        // prefix, padded the way the harness pads.
        for prefix in HARNESS_TURN_PREFIXES {
            let seq = s.seq;
            let text = format!("  \n{prefix} body");
            assert!(
                !apply_hook_event(&mut s, "u1", ups, &say(&text), None, 2100, true),
                "{prefix}"
            );
            assert_eq!(s.seq, seq, "{prefix}");
        }
        assert_eq!(s.agents["u1"].turn_started, 2000, "the turn goes on");

        // A typed prompt on the same working row IS a commitment, and it
        // does not restart the turn it joins (a queued message).
        assert!(apply_hook_event(
            &mut s,
            "u1",
            ups,
            &say("also fix the docs"),
            None,
            2200,
            true
        ));
        let r = &s.agents["u1"];
        assert_eq!((r.last_interacted, r.turn_started), (2200, 2000));
        assert_ne!(r.commit_ord, 0);
        assert_eq!(s.tab_order.get(&4), Some(&r.commit_ord));
        // A prompt that merely QUOTES a prefix is typed: the rule is the start.
        let quoting = say("why did <task-notification> fire twice?");
        let before = s.agents["u1"].commit_ord;
        apply_hook_event(&mut s, "u1", ups, &quoting, None, 2300, true);
        assert!(s.agents["u1"].commit_ord > before);
    }

    /// A status move is a change on its own, with the turn clock still: a
    /// finished row closing (Done to Idle) keeps a zero clock, and it must
    /// still push, or every bar keeps showing Done for a session that ended.
    #[test]
    fn a_status_move_is_a_change_while_the_clock_stands_still() {
        let mut s = Store::default();
        let mut r = rec("u1");
        r.status = Status::Done;
        s.agents.insert("u1".into(), r);
        let p = HookPayload {
            session_id: Some("u1".into()),
            ..Default::default()
        };
        assert!(apply_hook_event(
            &mut s,
            "u1",
            "SessionEnd",
            &p,
            None,
            100,
            true
        ));
        assert_eq!(
            (s.agents["u1"].status, s.agents["u1"].turn_started),
            (Status::Idle, 0)
        );
    }

    /// The turn clock lives exactly as long as the turn: a prompt that starts
    /// one starts it, a block inside it keeps it, every ending zeroes it.
    #[test]
    fn the_turn_clock_lives_exactly_as_long_as_the_turn() {
        let mut s = Store::default();
        s.agents.insert("u1".into(), rec("u1"));
        let of_kind = |kind: Option<&str>| HookPayload {
            session_id: Some("u1".into()),
            notification_type: kind.map(str::to_string),
            ..Default::default()
        };
        let (p, ask, nag) = (
            of_kind(None),
            of_kind(Some("permission_prompt")),
            of_kind(Some("idle_prompt")),
        );
        let mut at = |event, payload: &HookPayload, now| {
            apply_hook_event(&mut s, "u1", event, payload, None, now, true);
            (s.agents["u1"].status, s.agents["u1"].turn_started)
        };
        use Status::*;
        assert_eq!(at("UserPromptSubmit", &p, 100), (Working, 100));
        assert_eq!(
            at("Notification", &ask, 110),
            (NeedsYou, 100),
            "a block keeps it"
        );
        assert_eq!(
            at("PostToolBatch", &p, 120),
            (Working, 100),
            "the answer resumes it"
        );
        assert_eq!(at("Stop", &p, 130), (Done, 0));
        assert_eq!(at("UserPromptSubmit", &p, 300), (Working, 300));
        assert_eq!(at("StopFailure", &p, 310), (Failed, 0));
        assert_eq!(at("UserPromptSubmit", &p, 400), (Working, 400));
        assert_eq!(at("Notification", &nag, 460), (Idle, 0), "a missed Stop");
        assert_eq!(at("UserPromptSubmit", &p, 500), (Working, 500));
        assert_eq!(at("SessionEnd", &p, 510), (Idle, 0));
    }

    /// Esc sends no Stop, so the row still shows Working when the corrected
    /// prompt comes. The interrupt marker in the transcript gives the new
    /// turn a clock of its own (PR #285 review: a 0 s turn read 4m30s). A
    /// message queued into a live turn finds the turn's own records newer,
    /// and joins its clock.
    #[test]
    fn an_interrupted_turn_gives_the_next_prompt_a_clock_of_its_own() {
        let said = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Working on it"}]}}"#;
        let esc = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
        let esc_tool = r#"{"type":"user","message":{"role":"user","content":"[Request interrupted by user for tool use]"}}"#;
        let result = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#;
        let quoted = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"[Request interrupted by user]"}]}}"#;
        let side = r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
        let tail = |lines: &[&str]| lines.join("\n");
        assert!(interrupted(&tail(&[said, esc])));
        assert!(interrupted(&tail(&[said, esc_tool])), "the tool-use form");
        assert!(
            !interrupted(&tail(&[esc, said, result])),
            "a live turn since"
        );
        assert!(
            !interrupted(&tail(&[said, quoted])),
            "a tool result quoting it"
        );
        assert!(!interrupted(&tail(&[said, side])), "a subagent's record");
        assert!(!interrupted(""));

        let mut s = Store::default();
        let mut r = rec("u1");
        r.status = Status::Working;
        r.turn_started = 100;
        s.agents.insert("u1".into(), r);
        let typed = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("no, the other file".into()),
            ..Default::default()
        };
        let ups = "UserPromptSubmit";
        let after_esc = tail(&[said, esc]);
        apply_hook_event(&mut s, "u1", ups, &typed, Some(&after_esc), 370, true);
        assert_eq!(
            s.agents["u1"].turn_started, 370,
            "the interrupted turn is over"
        );
        let live = tail(&[esc, said, result]);
        apply_hook_event(&mut s, "u1", ups, &typed, Some(&live), 400, true);
        assert_eq!(
            s.agents["u1"].turn_started, 370,
            "a queued message joins it"
        );
    }

    /// The payload's `source` says who wrote the prompt, and it wins over the
    /// text where the CLI sends it. Without it, the text's start decides.
    #[test]
    fn the_prompts_source_says_who_wrote_it() {
        let ups = |source: Option<&str>, text: &str| HookPayload {
            prompt: Some(text.into()),
            source: source.map(str::to_string),
            ..Default::default()
        };
        for who in ["user", "sdk"] {
            assert!(
                is_typed(&ups(Some(who), "<task-notification> quoted")),
                "{who}"
            );
        }
        for who in ["system", "loop_wakeup", "schedule_wakeup", "poll_event"] {
            assert!(!is_typed(&ups(Some(who), "check the build")), "{who}");
        }
        assert!(is_typed(&ups(None, "check the build")));
        assert!(!is_typed(&ups(None, "<agent-message from=\"a1\">done")));
        assert!(
            is_typed(&HookPayload::default()),
            "no prompt reads as typed"
        );
    }

    /// The resting count (PR #285 review). The meter paces a working row's
    /// count and runs on no turn end, so the count a turn ends on can be a
    /// held one. A turn end lands the transcript's count over the meter's
    /// yield when it is higher, and only then: a lower one may be a tail that
    /// lacks the final response.
    #[test]
    fn a_turn_end_lands_the_transcripts_count_when_it_is_higher() {
        let usage = |n: u32| {
            format!(r#"{{"type":"assistant","message":{{"usage":{{"input_tokens":{n}}}}}}}"#)
        };
        let of_kind = |kind: Option<&str>, agent: Option<&str>| HookPayload {
            session_id: Some("u1".into()),
            notification_type: kind.map(str::to_string),
            agent_id: agent.map(str::to_string),
            ..Default::default()
        };
        let own = of_kind(None, None);
        let nag = of_kind(Some("idle_prompt"), None);
        // A metered row, inside the yield, holding 1000.
        let run = |event: &str, p: &HookPayload, status: Status, tail: u32| {
            let mut s = Store::default();
            let mut r = rec("u1");
            r.status = status;
            r.context_tokens = Some(1000);
            r.metered_at = 990;
            s.agents.insert("u1".into(), r);
            apply_hook_event(&mut s, "u1", event, p, Some(&usage(tail)), 1000, true);
            let r = &s.agents["u1"];
            (r.context_tokens, r.metered_at)
        };
        use Status::*;
        assert_eq!(run("Stop", &own, Working, 1500), (Some(1500), 0));
        assert_eq!(
            run("Stop", &own, Working, 800),
            (Some(1000), 0),
            "lower holds"
        );
        assert_eq!(run("StopFailure", &own, Working, 1500), (Some(1500), 0));
        assert_eq!(run("Notification", &nag, Working, 1500), (Some(1500), 0));
        // Not a turn end: the yield holds, and so does the stamp.
        assert_eq!(run("Notification", &nag, Done, 1500), (Some(1000), 990));
        assert_eq!(run("UserPromptSubmit", &own, Done, 1500), (Some(1000), 990));
        let sub = of_kind(None, Some("a1"));
        assert_eq!(run("StopFailure", &sub, Working, 1500), (Some(1000), 990));
        assert!(!ends_turn("StopFailure", &sub, Working));
        assert!(!ends_turn("SessionEnd", &own, Working));
    }

    #[test]
    fn only_user_prompt_submit_moves_the_order() {
        // The executable form of the maintainer's ruling (2026-07-22): "only a
        // user prompt to an agent reorders. Claude finishing does not."
        //
        // Every non-prompt event below DOES change the row — status, label —
        // and therefore bumps seq and pushes. What none of them may do is move
        // the row: the tab order and the row's ordinal must come out
        // byte-identical. Asserting "nothing changed" would be a weaker and
        // wrong test, since these events are supposed to change things.
        let mut s = crate::store::Store::default();
        let mut r = rec("u1");
        r.tab_id = Some(4);
        s.agents.insert("u1".into(), r);
        let p = HookPayload {
            session_id: Some("u1".into()),
            ..Default::default()
        };
        // One real commitment first, so there is a rank to preserve.
        assert!(apply_hook_event(
            &mut s,
            "u1",
            "UserPromptSubmit",
            &p,
            None,
            1000,
            true
        ));
        let order = s.tab_order.clone();
        let ord = s.agents["u1"].commit_ord;
        assert_eq!((ord, order.get(&4)), (1, Some(&1)));

        // Events that DO drive status. Each must move the glyph and leave the
        // rank alone — the pairing is the ruling.
        let perm = HookPayload {
            session_id: Some("u1".into()),
            message: Some("needs your permission".into()),
            ..Default::default()
        };
        for (event, payload, expected) in [
            ("Stop", &p, Status::Done),
            ("StopFailure", &p, Status::Failed),
            ("Notification", &perm, Status::NeedsYou),
            ("PostToolBatch", &p, Status::Working),
            ("SessionEnd", &p, Status::Idle),
        ] {
            apply_hook_event(&mut s, "u1", event, payload, None, 9999, true);
            assert_eq!(
                s.agents["u1"].status, expected,
                "{event} should still drive status"
            );
            assert_eq!(s.tab_order, order, "{event} must not move the tab");
            assert_eq!(
                s.agents["u1"].commit_ord, ord,
                "{event} must not re-rank the row"
            );
        }
        // A hook event that is not in the status map at all is a total no-op —
        // it must not even mint an ordinal it then discards.
        let seq = s.seq;
        assert!(!apply_hook_event(
            &mut s,
            "u1",
            "PreToolUse",
            &p,
            None,
            9999,
            true
        ));
        assert_eq!(s.seq, seq, "a no-op event must not consume an ordinal");
        assert_eq!(s.tab_order, order);
        assert_eq!(s.agents["u1"].commit_ord, ord);
        // And the clock stayed put too — only a prompt writes it.
        assert_eq!(s.agents["u1"].last_interacted, 1000);
    }

    #[test]
    fn two_prompts_in_the_same_wall_second_get_distinct_ordinals() {
        // The S1 §1.1 regression test. The old key was whole unix SECONDS, so
        // two prompts landing in the same second tied — and the tie was broken
        // by TAB POSITION, meaning the wrong row won and one prompt was
        // silently swallowed. Same `now` for both here: the ordinals must still
        // separate, because they come from the lock and not the clock.
        let mut s = crate::store::Store::default();
        let mut a = rec("u-a");
        a.tab_id = Some(1);
        s.agents.insert("u-a".into(), a);
        let mut b = rec("u-b");
        b.tab_id = Some(2);
        s.agents.insert("u-b".into(), b);
        let pa = HookPayload {
            session_id: Some("u-a".into()),
            ..Default::default()
        };
        let pb = HookPayload {
            session_id: Some("u-b".into()),
            ..Default::default()
        };
        apply_hook_event(&mut s, "u-a", "UserPromptSubmit", &pa, None, 1000, true);
        apply_hook_event(&mut s, "u-b", "UserPromptSubmit", &pb, None, 1000, true);
        assert_eq!(
            s.agents["u-a"].last_interacted,
            s.agents["u-b"].last_interacted
        );
        assert!(
            s.tab_order[&2] > s.tab_order[&1],
            "the later prompt must rank higher despite an identical clock"
        );
        assert!(s.agents["u-b"].commit_ord > s.agents["u-a"].commit_ord);
    }

    #[test]
    fn prompt_stamps_bound_tabs_order_atomically() {
        // §6.6 Design B: a prompt is a USER COMMITMENT to the agent's TAB.
        // The hook stamps the tab order through the bind in the SAME locked
        // write as the last_interacted bump — no bar round-trip, no
        // switch-away race.
        //
        // S1: the stamped VALUE is now the write's own minted ordinal, not the
        // `now` argument. `now` still lands on `last_interacted`, which stays a
        // display clock, so the two are asserted separately below — that
        // separation is the whole point of the change.
        let mut s = crate::store::Store::default();
        let mut r = rec("u1");
        r.tab_id = Some(4);
        s.agents.insert("u1".into(), r);
        s.agents.insert("u2".into(), rec("u2")); // unbound
        let p = HookPayload {
            session_id: Some("u1".into()),
            ..Default::default()
        };
        assert!(apply_hook_event(
            &mut s,
            "u1",
            "UserPromptSubmit",
            &p,
            None,
            1700,
            true
        ));
        assert_eq!(s.agents["u1"].last_interacted, 1700); // the clock
        assert_eq!(s.tab_order.get(&4), Some(&1)); // the ordinal — seq, not now
        assert_eq!(s.tab_touched.get(&4), Some(&1700)); // the wall-clock twin
        assert_eq!(s.agents["u1"].commit_ord, 1); // same value on both halves
        assert_eq!(s.seq, 1); // one bump for the whole atomic change
        // Unbound agent: interaction still recorded, no stamp to place.
        assert!(apply_hook_event(
            &mut s,
            "u2",
            "UserPromptSubmit",
            &p,
            None,
            1800,
            true
        ));
        assert_eq!(s.agents["u2"].last_interacted, 1800);
        assert_eq!(s.tab_order.len(), 1);
        assert_eq!(s.tab_touched.len(), 1); // only u1's tab has a touch stamp
        // The RC-B case: unbound, so nothing to stamp on a tab — but the ROW
        // still records its ordinal, so the dormant row it becomes sorts right
        // even if no bind or prune ever lands.
        assert_eq!(s.agents["u2"].commit_ord, 2);
        // Non-commitment events don't touch the order (Stop ≠ user input).
        assert!(apply_hook_event(&mut s, "u1", "Stop", &p, None, 1900, true));
        assert_eq!(s.tab_order.get(&4), Some(&1));
        assert_eq!(
            s.agents["u1"].commit_ord, 1,
            "Stop must not re-rank the row"
        );
        // Unknown uuid / no-op event: unchanged, no seq bump.
        let seq = s.seq;
        assert!(!apply_hook_event(
            &mut s, "ghost", "Stop", &p, None, 2000, true
        ));
        assert!(!apply_hook_event(
            &mut s,
            "u1",
            "PreToolUse",
            &p,
            None,
            2000,
            true
        ));
        assert_eq!(s.seq, seq);
    }

    #[test]
    fn a_prompt_buckets_one_commitment_on_record_and_bound_tab() {
        // A prompt is one commitment: +1 in the record's hour bucket AND the
        // bound tab's twin — the same doubled bookkeeping as commit_ord/tab_order.
        let mut s = crate::store::Store::default();
        let mut r = rec("u1");
        r.tab_id = Some(4);
        s.agents.insert("u1".into(), r);
        let p = HookPayload {
            session_id: Some("u1".into()),
            ..Default::default()
        };
        assert!(apply_hook_event(
            &mut s,
            "u1",
            "UserPromptSubmit",
            &p,
            None,
            1700,
            true
        ));
        let now_hour = crate::store::unix_hour(1700);
        assert_eq!(s.agents["u1"].buckets.get(&now_hour), Some(&1));
        assert_eq!(
            s.tab_buckets.get(&4).and_then(|m| m.get(&now_hour)),
            Some(&1)
        );
    }

    #[test]
    fn non_commitment_events_write_no_buckets() {
        // Stop/Notification/SessionEnd are not commitments — no bucket moves.
        let mut s = crate::store::Store::default();
        let mut r = rec("u1");
        r.tab_id = Some(4);
        s.agents.insert("u1".into(), r);
        let p = HookPayload {
            session_id: Some("u1".into()),
            ..Default::default()
        };
        apply_hook_event(&mut s, "u1", "Stop", &p, None, 1700, true);
        assert!(s.agents["u1"].buckets.is_empty());
        assert!(s.tab_buckets.get(&4).is_none_or(|m| m.is_empty()));
    }

    /// A payload with only the fields the status table reads.
    fn said(kind: Option<&str>, message: Option<&str>, agent: Option<&str>) -> HookPayload {
        HookPayload {
            notification_type: kind.map(str::to_string),
            message: message.map(str::to_string),
            agent_id: agent.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn state_machine_is_latest_wins() {
        use Status::*;
        let none = HookPayload::default();
        let at = |event, p: &HookPayload, current| status_for_event(event, p, current);
        // Spec §6.5 transition table: these four ignore `current`.
        assert_eq!(at("UserPromptSubmit", &none, Idle), Some(Working));
        assert_eq!(at("Stop", &none, Working), Some(Done));
        assert_eq!(at("StopFailure", &none, Working), Some(Failed));
        assert_eq!(at("SessionEnd", &none, Done), Some(Idle));
        // PermissionRequest is not registered (setup.rs HOOK_EVENTS): the
        // Notification below carries the same ask. An arm for it was dead.
        assert_eq!(at("PermissionRequest", &none, Working), None);
        // Unknown events are a no-op — the global hook must never guess.
        assert_eq!(at("PreToolUse", &none, Idle), None);
    }

    /// The ask is the contract's three kinds, and red whatever the row
    /// showed. The text is only the fallback for a payload with no kind.
    #[test]
    fn a_notification_asks_by_its_kind_and_falls_back_to_its_text() {
        use Status::*;
        for kind in [
            "permission_prompt",
            "elicitation_dialog",
            "elicitation_url_dialog",
        ] {
            for current in [Working, Done, Idle] {
                let p = said(Some(kind), None, None);
                assert_eq!(
                    status_for_event("Notification", &p, current),
                    Some(NeedsYou)
                );
            }
        }
        let text = said(None, Some("Claude needs your permission to use Bash"), None);
        assert_eq!(
            status_for_event("Notification", &text, Done),
            Some(NeedsYou)
        );
        // The kind wins over the text: a kind clave does not act on stays
        // quiet, whatever words its message holds.
        let auth = said(Some("auth_success"), Some("permission granted"), None);
        assert_eq!(status_for_event("Notification", &auth, Working), None);
        let other = said(None, Some("compacting…"), None);
        assert_eq!(status_for_event("Notification", &other, Working), None);
    }

    /// The idle nag ends a live turn whose Stop never came (an interrupt sends
    /// none). It never touches a turn that did end: Done keeps its green.
    #[test]
    fn the_idle_nag_ends_only_a_live_turn() {
        use Status::*;
        let by_kind = said(Some("idle_prompt"), None, None);
        let by_text = said(None, Some("Claude is waiting for your input"), None);
        for p in [&by_kind, &by_text] {
            assert_eq!(status_for_event("Notification", p, Working), Some(Idle));
            assert_eq!(status_for_event("Notification", p, NeedsYou), Some(Idle));
            for held in [Done, Idle, Failed] {
                assert_eq!(status_for_event("Notification", p, held), None, "{held:?}");
            }
        }
    }

    /// The answer clave lacked: a tool batch resolves only after every tool
    /// in it ran, so red is over. A subagent's batch carries `agent_id` and
    /// names the parent's session; it is not the parent's answer.
    #[test]
    fn a_tool_batch_clears_red_and_nothing_else() {
        use Status::*;
        let own = said(None, None, None);
        assert_eq!(
            status_for_event("PostToolBatch", &own, NeedsYou),
            Some(Working)
        );
        for held in [Working, Done, Idle, Failed] {
            assert_eq!(
                status_for_event("PostToolBatch", &own, held),
                None,
                "{held:?}"
            );
        }
        let sub = said(None, None, Some("a1b2"));
        assert_eq!(status_for_event("PostToolBatch", &sub, NeedsYou), None);
        assert!(from_a_subagent("PostToolBatch", &sub));
        assert!(!from_a_subagent("PostToolBatch", &own));
        assert!(
            !from_a_subagent("Notification", &sub),
            "a batch or an error only"
        );
    }

    /// StopFailure carries `agent_id` when the error ended a subagent's turn
    /// (2.1.299 builds its input from the subagent's context). The parent's
    /// turn goes on, so the parent row must not show Failed.
    #[test]
    fn a_subagents_api_error_does_not_fail_the_parent() {
        use Status::*;
        let own = said(None, None, None);
        let sub = said(None, None, Some("a1b2"));
        assert_eq!(status_for_event("StopFailure", &own, Working), Some(Failed));
        assert_eq!(status_for_event("StopFailure", &sub, Working), None);
        assert!(from_a_subagent("StopFailure", &sub));
        assert!(!from_a_subagent("StopFailure", &own));
    }

    /// The strict-payload trap (FOOTGUNS). One field of a novel type used to
    /// fail the whole parse, and `run_hook` read that as an empty payload:
    /// no `session_id`, so the event was lost. Every field is lenient now.
    #[test]
    fn a_field_of_a_novel_type_does_not_blank_the_session() {
        let json = r#"{"session_id":"s1","agent_id":7,"notification_type":["x"],
            "message":{"text":"hi"},"cwd":"/w","background_tasks":"later"}"#;
        let p: HookPayload = serde_json::from_str(json).unwrap();
        assert_eq!(p.session_id.as_deref(), Some("s1"));
        assert_eq!(p.cwd.as_deref(), Some("/w"));
        assert_eq!(
            (p.agent_id, p.notification_type, p.message),
            (None, None, None)
        );
    }

    /// A registry entry in the shape Claude Code 2.1.299 sends: `type` is
    /// the label (`subagent` for a `local_agent` task), `id` is the task id.
    fn task(kind: &str, id: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "type": kind, "status": "running", "description": "lane"})
    }

    fn registry(
        event: &str,
        agent: Option<&str>,
        tasks: Option<Vec<serde_json::Value>>,
    ) -> Option<bool> {
        let p = HookPayload {
            agent_id: agent.map(str::to_string),
            background_tasks: tasks,
            ..Default::default()
        };
        fan_out_reading(event, &p)
    }

    /// The mark is what the agent CLI says is in flight, at the events that
    /// say it. Each rule is pinned on its own, so a flipped branch fails here.
    #[test]
    fn the_subagent_mark_follows_the_task_registry() {
        let sub = |id| task("subagent", id);
        assert_eq!(registry("SubagentStart", Some("a1"), None), Some(true));
        // Stop: any agent still listed holds the mark; other work does not.
        assert_eq!(registry("Stop", None, Some(vec![sub("a1")])), Some(true));
        assert_eq!(
            registry("Stop", None, Some(vec![task("teammate", "t1")])),
            Some(true)
        );
        let other_work = vec![task("shell", "b1"), task("monitor", "m1")];
        assert_eq!(registry("Stop", None, Some(other_work)), Some(false));
        assert_eq!(registry("Stop", None, Some(vec![])), Some(false));
        assert_eq!(
            registry("Stop", None, None),
            Some(false),
            "no registry, no agents"
        );
        // SubagentStop: the stopping agent may still be listed; only the
        // others count.
        assert_eq!(
            registry("SubagentStop", Some("a1"), Some(vec![sub("a1")])),
            Some(false)
        );
        let two = vec![sub("a1"), sub("a2")];
        assert_eq!(registry("SubagentStop", Some("a1"), Some(two)), Some(true));
        assert_eq!(registry("SubagentStop", Some("a1"), None), Some(false));
        // An entry with no `type` or no `id` is no agent, and no panic.
        let odd = vec![serde_json::json!({"id": "x"}), serde_json::json!("text")];
        assert_eq!(registry("Stop", None, Some(odd)), Some(false));
        // Events that say nothing about agents hold.
        for quiet in [
            "UserPromptSubmit",
            "Notification",
            "PostToolBatch",
            "StopFailure",
        ] {
            assert_eq!(
                registry(quiet, None, Some(vec![sub("a1")])),
                None,
                "{quiet}"
            );
        }
        assert_eq!(
            registry("SessionEnd", None, Some(vec![sub("a1")])),
            Some(false)
        );
    }

    /// A compaction continues the SAME session, agents and all, so it holds.
    /// Every other start is a new process, which holds no agents of the old.
    #[test]
    fn only_a_compaction_carries_the_mark_across_a_session_start() {
        let start = |source: Option<&str>| {
            let p = HookPayload {
                source: source.map(str::to_string),
                ..Default::default()
            };
            fan_out_reading("SessionStart", &p)
        };
        assert_eq!(start(Some("compact")), None);
        for fresh in [
            Some("startup"),
            Some("resume"),
            Some("clear"),
            Some("fork"),
            None,
        ] {
            assert_eq!(start(fresh), Some(false), "{fresh:?}");
        }
    }

    #[test]
    fn the_subagent_mark_moving_is_itself_a_store_write() {
        // `changed` is what mints an ord and pushes the fleet. A mark that
        // flipped without reporting it would be correct in memory and stale on
        // every bar until some unrelated event happened to push — and the
        // reverse, a push on every repeat, is traffic for nothing.
        let mut s = Store::default();
        s.agents.insert("u1".into(), rec("u1"));
        let ev = |agent: Option<&str>, tasks: Option<Vec<serde_json::Value>>| HookPayload {
            session_id: Some("u1".into()),
            agent_id: agent.map(str::to_string),
            background_tasks: tasks,
            ..Default::default()
        };
        let mut fire =
            |event, p: &HookPayload| apply_hook_event(&mut s, "u1", event, p, None, 1000, true);
        let started = ev(Some("a1"), None);
        assert!(
            fire("SubagentStart", &started),
            "the mark lighting up is a change"
        );
        assert!(!fire("SubagentStart", &started), "and staying lit is not");
        assert!(
            !fire("Notification", &ev(None, None)),
            "a quiet event holds it"
        );
        let stopped = ev(Some("a1"), Some(vec![task("subagent", "a1")]));
        assert!(fire("SubagentStop", &stopped), "going out is a change too");
        assert!(!s.agents["u1"].subagents);
    }

    /// The whole lines in a tail read, and never a fragment: the first line
    /// of a read that starts mid-file is a PREFIX of a real record, and that
    /// often parses as one and reports a stale field.
    #[test]
    fn a_tail_read_hands_on_only_whole_lines() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("t.jsonl");
        let filler = "x".repeat(200);
        let body = (0..60)
            .map(|i| format!(r#"{{"n":{i},"pad":"{filler}"}}"#))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, &body).unwrap();
        let cut = read_tail(&path, 1024).unwrap();
        assert!(!cut.is_empty() && cut.len() <= 1024);
        assert!(body.ends_with(&cut), "the cut is a SUFFIX, never a copy");
        for line in cut.lines() {
            serde_json::from_str::<serde_json::Value>(line).expect("every line must be whole");
        }
        // A file inside the window is handed on whole, first line and all.
        std::fs::write(&path, "a\nb").unwrap();
        assert_eq!(read_tail(&path, 1024).as_deref(), Some("a\nb"));
        // A window with no line break holds no whole line: nothing, not a
        // fragment.
        std::fs::write(&path, "y".repeat(2048)).unwrap();
        assert_eq!(read_tail(&path, 1024).as_deref(), Some(""));
        // A cut inside a multi-byte character neither panics nor leaks half
        // of one: the leading `a` puts every `é` on an odd offset.
        std::fs::write(&path, format!("a{}\n{}", "é".repeat(600), "é".repeat(100))).unwrap();
        assert_eq!(read_tail(&path, 1024), Some("é".repeat(100)));
        // A window that starts exactly on a line keeps that line (PR #285
        // review): the last 5 bytes of `ab\ncd\nef` are `cd\nef`.
        std::fs::write(&path, "ab\ncd\nef").unwrap();
        assert_eq!(read_tail(&path, 5).as_deref(), Some("cd\nef"));
        assert_eq!(read_tail(&path, 4).as_deref(), Some("ef"), "mid-line: cut");
    }

    /// The window must reach the newest token reading. Measured 2026-10-14
    /// over 278 local transcripts: the newest assistant `usage` record starts
    /// a median 6.7 KB from the end, p90 42 KB, and 5 of 278 lie past 64 KB.
    /// A record at the p90 distance must still land.
    #[test]
    fn the_tail_window_reaches_a_reading_behind_a_large_record() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("t.jsonl");
        let reading = r#"{"type":"assistant","message":{"model":"claude-opus-5","usage":{"input_tokens":1200,"cache_read_input_tokens":800}}}"#;
        let big = format!(
            r#"{{"type":"user","message":{{"role":"user","content":"{}"}}}}"#,
            "x".repeat(42 * 1024)
        );
        std::fs::write(&path, format!("{reading}\n{big}\n")).unwrap();
        let tail = read_tail(&path, TAIL_BYTES).unwrap();
        assert_eq!(tokens_from_tail(&tail), Some(2000));
    }

    /// `split_once`, not `split(anchor).nth(1)`. The two agree until the
    /// anchor appears twice, where `nth(1)` returns the MIDDLE segment and
    /// drops everything past the second occurrence — so a tool invocation that
    /// happens to quote the CLI's own sentence loses its tail. Agent-adjacent
    /// text reaches this function; it should not be able to choose which half
    /// of itself the card shows.
    #[test]
    fn a_repeated_anchor_yields_everything_after_the_first_one() {
        assert_eq!(
            wants_from_message(
                "Claude needs your permission to use Bash (grep 'permission to use ' log)"
            )
            .as_deref(),
            Some("Bash (grep 'permission to use ' log)")
        );
    }

    /// A tail carrying every reading a Stop takes from the transcript: the
    /// token usage, the model, the effort, and the checkout.
    fn four_signal_tail() -> String {
        [
            r#"{"type":"assistant","effort":"high","message":{"model":"claude-opus-5","usage":{"input_tokens":1200,"cache_read_input_tokens":800}}}"#,
            // The checkout pair belongs in this fixture or the yield guard
            // below cannot see the cell it is guarding: with no `cwd` record
            // `checkout_from_tail` answers None in BOTH arms, so the assertion
            // compares two values that could not have differed. Found
            // 2026-09-15 by moving the checkout read behind the yield and
            // watching all 410 tests stay green.
            r#"{"type":"user","cwd":"/repo/moved","gitBranch":"moved","message":{"role":"user"}}"#,
        ]
        .join("\n")
    }

    #[test]
    fn the_meters_yield_covers_three_cells_and_no_others() {
        // The class guard, not the instance. The bug above was one new tail
        // reader picking up the suppressed local instead of the raw one, and
        // nothing about that call site looks wrong. This runs the SAME event
        // against a metered row and a quiet one and demands the two records come
        // out identical once the three documented cells are set aside — so a
        // fourth cell added under the wrong local fails here, whatever it is.
        let p = HookPayload {
            session_id: Some("u1".into()),
            ..HookPayload::default()
        };
        let tail = four_signal_tail();
        let run = |metered_at: u64| {
            let mut s = Store::default();
            let mut r = rec("u1");
            r.metered_at = metered_at;
            s.agents.insert("u1".into(), r);
            apply_hook_event(&mut s, "u1", "Stop", &p, Some(&tail), 1000, true);
            s.agents["u1"].clone()
        };

        let mut quiet = run(0);
        let mut metered = run(900);
        for r in [&mut quiet, &mut metered] {
            // The yield's whole scope. Three CELLS, five fields: the level is
            // derived from the count and the provider is set alongside the
            // model, and the meter writes both of those too — so they ride with
            // their cell rather than being a fourth thing the yield covers.
            r.context_tokens = None;
            r.context_level = None;
            r.model = None;
            r.provider = None;
            r.effort = None;
        }
        assert_eq!(
            quiet, metered,
            "the meter may only silence tokens, model and effort — every other \
             cell has no fresher source to yield to"
        );
    }

    /// A row follows its session into a worktree, because the transcript says
    /// where the session is and the row does not.
    ///
    /// Reported from the live fleet 2026-09-15: a card showing `clave` with no
    /// branch and no PR, whose session was in `.../worktrees/live-set-restore`
    /// on `worktree-live-set-restore` with #261 open. The row still said `main`
    /// — so the branch cell blanked (the bar hides the default branch) and
    /// `pr-sync` asked `gh` for a PR on `main`, correctly found none, and cached
    /// the miss. Nothing was broken in the PR path; the question was wrong.
    #[test]
    fn a_row_follows_its_session_into_the_checkout_the_transcript_names() {
        let line = |cwd: &str, branch: &str| {
            format!(
                r#"{{"type":"user","cwd":"{cwd}","gitBranch":"{branch}","message":{{"role":"user","content":"hi"}}}}"#
            )
        };
        let mut rec = rec("u1");
        rec.cwd = "/repo".into();
        rec.branch = "main".into();

        // The move the field reported: same repo, into a linked worktree.
        assert!(take_checkout(
            &mut rec,
            checkout_from_tail(&line("/repo/.claude/worktrees/wt", "worktree-wt"))
        ));
        assert_eq!(rec.cwd, "/repo/.claude/worktrees/wt");
        assert_eq!(rec.branch, "worktree-wt");
        // Idempotent: a row already where the transcript says is not a write,
        // and `changed` gates the snapshot push — every hook event would
        // otherwise repaint the whole fleet for nothing.
        assert!(!take_checkout(
            &mut rec,
            checkout_from_tail(&line("/repo/.claude/worktrees/wt", "worktree-wt"))
        ));

        // The NEWEST record wins, and the pair comes from ONE record: a cwd
        // from one line beside a branch from another describes a checkout that
        // has never existed.
        let moved = format!(
            "{}\n{}",
            line("/repo/.claude/worktrees/wt", "worktree-wt"),
            line("/repo", "main")
        );
        assert_eq!(
            checkout_from_tail(&moved),
            Some(("/repo".to_string(), Some("main".to_string())))
        );

        // A detached head is not a branch name. `HEAD` in the cell would be a
        // lie the bar renders, and a PR question `gh` cannot answer.
        let mut detached = rec.clone();
        assert!(take_checkout(
            &mut detached,
            checkout_from_tail(&line("/repo/other", "HEAD"))
        ));
        assert_eq!(detached.branch, "-", "the sentinel, not the old branch");
        assert_eq!(
            crate::pr::resolve_pr(&|_| Some("261".into()), "/repo", "-"),
            None,
            "the sentinel must never reach `gh` as a --head argument"
        );

        // A record with no cwd at all leaves the row alone — the tail is the
        // only source, and silence in it is not an instruction to move.
        let mut untouched = rec.clone();
        assert!(!take_checkout(
            &mut untouched,
            checkout_from_tail(r#"{"type":"assistant","message":{"model":"claude-opus-5"}}"#)
        ));
        assert_eq!(untouched.cwd, "/repo/.claude/worktrees/wt");
    }

    /// The worktree mark dies with the directory it describes.
    ///
    /// Component-wise, the same rule `store::apply_relocation` already applies
    /// on the spawn path: a move WITHIN the worktree keeps the mark, a move out
    /// drops it to unknown. Nothing here asks git where the new checkout's root
    /// is, so unknown is the honest answer — and a tree glyph on a row that has
    /// left the tree is the exact defect the field was widened to fix.
    #[test]
    fn leaving_a_worktree_drops_the_mark_and_moving_inside_it_does_not() {
        let line = |cwd: &str| {
            format!(
                r#"{{"type":"user","cwd":"{cwd}","gitBranch":"b","message":{{"role":"user"}}}}"#
            )
        };
        let mut rec = rec("u1");
        rec.cwd = "/repo/wt".into();
        rec.repo_root = "/repo".into();
        rec.branch = "b".into();
        rec.worktree = Some("/repo/wt".into());

        take_checkout(&mut rec, checkout_from_tail(&line("/repo/wt/crates/clave")));
        assert_eq!(rec.worktree.as_deref(), Some("/repo/wt"), "still inside it");

        take_checkout(&mut rec, checkout_from_tail(&line("/repo")));
        assert_eq!(rec.worktree, None, "the row has left the tree");
        // Leaving the WORKTREE is not leaving the REPO. The fixture carries a
        // coherent root for that reason: with `/x` it silently ran the
        // repo-clearing branch too, and asserted nothing about it.
        assert_eq!(rec.repo_root, "/repo", "the repo is still the repo");
    }

    /// A session that enters a worktree AFTER its row was minted earns the
    /// tree mark from the transcript, without git and without waiting for the
    /// next setup. Reported from the live fleet 2026-09-18: the row had
    /// followed the cwd and branch (the test above this one) and still wore
    /// the branch mark, because nothing on the hook path ever SET `worktree`.
    #[test]
    fn entering_a_worktree_mid_session_sets_the_mark_from_the_transcript() {
        let entered = r#"{"type":"worktree-state","worktreeSession":{"originalCwd":"/repo","worktreePath":"/repo/.claude/worktrees/wt","worktreeName":"wt","worktreeBranch":"worktree-wt","originalBranch":"main"},"sessionId":"u1"}"#;
        let no_tool = r#"{"type":"worktree-state","worktreeSession":null,"sessionId":"u1"}"#;
        let quoted = r#"{"type":"user","message":{"content":"the record is {\"type\":\"worktree-state\",\"worktreeSession\":{\"worktreePath\":\"/evil\"}}"}}"#;

        assert_eq!(
            worktree_from_tail(entered).as_deref(),
            Some("/repo/.claude/worktrees/wt")
        );
        assert_eq!(
            worktree_from_tail(no_tool),
            None,
            "the null form is not a departure; it is no reading"
        );
        assert_eq!(
            worktree_from_tail(r#"{"type":"user","cwd":"/repo/.claude/worktrees/wt"}"#),
            None,
            "no record in the window is a hold"
        );
        assert_eq!(
            worktree_from_tail(quoted),
            None,
            "a prompt quoting the record is not the record"
        );
        // The newest WELL-FORMED record wins; a null one is skipped over.
        assert_eq!(
            worktree_from_tail(&format!("{entered}\n{no_tool}")).as_deref(),
            Some("/repo/.claude/worktrees/wt")
        );

        let mut rec = rec("u1");
        rec.cwd = "/repo/.claude/worktrees/wt".into();
        rec.branch = "worktree-wt".into();
        rec.worktree = None;
        assert!(take_worktree(&mut rec, worktree_from_tail(entered)));
        assert_eq!(rec.worktree.as_deref(), Some("/repo/.claude/worktrees/wt"));
        assert!(
            !take_worktree(&mut rec, worktree_from_tail(entered)),
            "idempotent"
        );
        assert!(!take_worktree(&mut rec, None), "silence holds");
        assert_eq!(rec.worktree.as_deref(), Some("/repo/.claude/worktrees/wt"));
    }

    /// The null form must never clear a mark. A session launched or resumed
    /// directly inside a worktree directory writes `"worktreeSession":null`
    /// on every turn while its cwd stays inside the tree (measured on
    /// `b947acf7`, 2026-09-18). A clear on it would fight the setup-time
    /// repair, which re-seeds from git.
    #[test]
    fn the_null_worktree_record_holds_a_mark_the_cwd_still_vouches_for() {
        let tail = concat!(
            r#"{"type":"worktree-state","worktreeSession":null,"sessionId":"u1"}"#,
            "\n",
            r#"{"type":"user","cwd":"/repo/.claude/worktrees/wt","gitBranch":"worktree-wt","message":{"role":"user","content":"hi"}}"#,
        );
        let mut s = Store::default();
        let mut r = rec("u1");
        r.cwd = "/repo/.claude/worktrees/wt".into();
        r.branch = "worktree-wt".into();
        r.worktree = Some("/repo/.claude/worktrees/wt".into());
        s.agents.insert("u1".into(), r);
        let p = HookPayload {
            session_id: Some("u1".into()),
            ..Default::default()
        };
        apply_hook_event(&mut s, "u1", "Stop", &p, Some(tail), 100, true);
        assert_eq!(
            s.agents["u1"].worktree.as_deref(),
            Some("/repo/.claude/worktrees/wt"),
            "the mark survives the null record"
        );
    }

    /// The record is believed only where the cwd agrees with it. A cwd line is
    /// written on every turn; the worktree record is re-stamped less often, so
    /// after a move OUT of the tree the newest worktree record can still name
    /// it. Taking it would re-mark the row `take_checkout` just unmarked.
    /// Component-wise, so `/repo/wt` cannot claim `/repo/wt2`.
    #[test]
    fn the_tree_mark_is_taken_only_where_the_cwd_is_inside_it() {
        let named = |path: &str| Some(path.to_string());
        let mut rec = rec("u1");
        rec.cwd = "/repo".into();
        rec.worktree = None;
        assert!(!take_worktree(&mut rec, named("/repo/wt")));
        assert_eq!(rec.worktree, None, "the cwd is outside the named tree");

        rec.cwd = "/repo/wt2/src".into();
        assert!(!take_worktree(&mut rec, named("/repo/wt")));
        assert_eq!(
            rec.worktree, None,
            "a prefix of the name is not containment"
        );

        rec.cwd = "/repo/wt/src".into();
        assert!(take_worktree(&mut rec, named("/repo/wt")));
        assert_eq!(
            rec.worktree.as_deref(),
            Some("/repo/wt"),
            "a subdirectory is inside"
        );
    }

    /// End to end through `apply_hook_event`, so deleting the call site fails
    /// the suite: the cwd moves first, then the mark is judged against the
    /// moved cwd. One Stop tail, both records, the row ends up marked. This is
    /// the shape the 2026-09-18 transcript had.
    #[test]
    fn one_stop_tail_moves_the_row_into_the_worktree_and_marks_it() {
        let tail = concat!(
            r#"{"type":"worktree-state","worktreeSession":{"originalCwd":"/repo","worktreePath":"/repo/.claude/worktrees/wt","worktreeName":"wt","worktreeBranch":"worktree-wt","originalBranch":"main"},"sessionId":"u1"}"#,
            "\n",
            r#"{"type":"user","cwd":"/repo/.claude/worktrees/wt","gitBranch":"worktree-wt","message":{"role":"user","content":"hi"}}"#,
        );
        let mut s = Store::default();
        let mut r = rec("u1");
        r.cwd = "/repo".into();
        r.branch = "main".into();
        r.worktree = None;
        s.agents.insert("u1".into(), r);
        let p = HookPayload {
            session_id: Some("u1".into()),
            ..Default::default()
        };
        assert!(apply_hook_event(
            &mut s,
            "u1",
            "Stop",
            &p,
            Some(tail),
            100,
            true
        ));
        let rec = &s.agents["u1"];
        assert_eq!(rec.cwd, "/repo/.claude/worktrees/wt");
        assert_eq!(rec.branch, "worktree-wt");
        assert_eq!(rec.worktree.as_deref(), Some("/repo/.claude/worktrees/wt"));
    }

    /// A checkout the layout generator cannot bake must not reach the store.
    ///
    /// CodeRabbit 2026-09-15 (major): `cwd` is interpolated RAW into generated
    /// KDL, and `add::validate_cwd` refuses a `"`, a `\` or a control char for
    /// that reason. This path wrote the transcript's value straight past that
    /// gate, so a session running in a directory holding one of those — legal
    /// on unix, and the transcript reports it faithfully — persisted a value
    /// that made the NEXT launch or resume fail to open a tab, long after the
    /// hook that recorded it.
    #[test]
    fn a_checkout_the_layout_cannot_bake_is_refused_rather_than_stored() {
        // Built with `json!`, not `format!`: a hand-written fixture holding a
        // quote or a backslash is INVALID JSON, so the parser refuses it first
        // and the test passes without ever reaching the guard. Caught by
        // reverting the guard and finding this test still green.
        let line = |cwd: &str| {
            serde_json::json!({
                "type": "user",
                "cwd": cwd,
                "gitBranch": "b",
                "message": { "role": "user" },
            })
            .to_string()
        };
        let mut rec = rec("u1");

        // A quote and a backslash each break the KDL string the cwd is baked
        // into. The row keeps the checkout it had: stale, but openable.
        for bad in ["/repo/\"evil\"", r"/repo/we\ird", "/repo/a\nb"] {
            let facts = checkout_from_tail(&line(bad));
            assert_eq!(
                facts.as_ref().map(|(c, _)| c.as_str()),
                Some(bad),
                "the fixture must reach the guard, not die in the parser"
            );
            let moved = take_checkout(&mut rec, facts);
            assert!(!moved, "{bad:?} must not move the row");
            assert_eq!(rec.cwd, "/x", "{bad:?} must not reach the record");
            assert_eq!(rec.branch, "main", "and must not take the branch with it");
        }

        // The gate is the character, not the move: an ordinary path still moves,
        // spaces and all.
        assert!(take_checkout(
            &mut rec,
            checkout_from_tail(&line("/repo/a b/dir"))
        ));
        assert_eq!(rec.cwd, "/repo/a b/dir");
    }

    /// A cached PR number must not outlive the checkout it was asked about.
    ///
    /// Blind review 2026-09-15 (minor, a regression this branch introduced).
    /// `pr::pr_is_stale` keys on the TTL and the branch, and a session moving
    /// between two repos that are BOTH on `main` changes neither — so repo A's
    /// number sat beside repo B's checkout for up to five minutes. Measured:
    /// 11 of the 76 cwd-changing transcripts cross a git toplevel, and the
    /// common shape is a session walking between sibling repos on their default
    /// branches. `pr::checkout_dir` fixed where the QUESTION is asked; this
    /// fixes how long the old ANSWER is believed.
    #[test]
    fn a_pr_number_stops_being_believed_when_the_checkout_moves() {
        let line = |cwd: &str, branch: &str| {
            serde_json::json!({
                "type": "user",
                "cwd": cwd,
                "gitBranch": branch,
                "message": { "role": "user" },
            })
            .to_string()
        };
        let mut rec = rec("u1");
        rec.cwd = "/repos/a".into();
        rec.branch = "main".into();
        rec.pr_number = Some(260);
        rec.pr_branch = "main".into();
        rec.pr_checked = 1_000;

        // Same branch name, different repository: the branch test cannot see
        // this move, so the stamp is what has to.
        assert!(take_checkout(
            &mut rec,
            checkout_from_tail(&line("/repos/b", "main"))
        ));
        assert_eq!(rec.pr_checked, 0, "the answer was asked somewhere else");
        assert!(
            crate::pr::pr_is_stale(&rec, 1_001),
            "and the next event must re-ask, inside the TTL or not"
        );
        assert_eq!(
            rec.pr_number,
            Some(260),
            "the number is left alone on purpose: clearing it would blank the \
             cell on every cd inside one repo, which is the common move"
        );

        // A branch change with no move still restales, as it always did.
        rec.pr_checked = 2_000;
        assert!(take_checkout(
            &mut rec,
            checkout_from_tail(&line("/repos/b", "feature"))
        ));
        assert_eq!(rec.pr_checked, 2_000, "no move, so the stamp stands");
        assert!(
            crate::pr::pr_is_stale(&rec, 2_001),
            "the branch term already covers this one"
        );
    }

    /// A move must not rewrite a fact only git could settle.
    ///
    /// Two review rounds pushed this the other way — clear the repo facts when
    /// the arriving cwd sits outside the recorded root — and both were wrong.
    /// `repo_root` is not one kind of thing: `add::run_add`'s resume arm writes
    /// the MAIN working tree, while its `new` arm and `hook::mint_adopted`
    /// write `rev-parse --show-toplevel` of the directory in hand, which inside
    /// a linked worktree is the WORKTREE. Measured on the live store
    /// 2026-09-15: 4 of 199 rows carry a worktree path as their root with no
    /// `worktree` set, and 0 rows have `worktree` set at all — so the rule
    /// would have fired on an ordinary `cd` from a worktree to its own repo's
    /// main checkout, inside ONE repository, and stranded the row for good.
    ///
    /// The wrong-PR risk that argued for clearing is answered in `pr.rs`, by
    /// asking `gh` from the directory the session is in. Here, the row keeps
    /// what only git could have told it.
    #[test]
    fn a_move_never_rewrites_a_fact_only_git_could_settle() {
        let line = |cwd: &str, branch: &str| {
            serde_json::json!({
                "type": "user",
                "cwd": cwd,
                "gitBranch": branch,
                "message": { "role": "user" },
            })
            .to_string()
        };
        // The shape `mint_adopted` writes inside a linked worktree: the root IS
        // the worktree, and `worktree` is None.
        let mut rec = rec("u1");
        rec.repo_root = "/repo/clave/.claude/worktrees/card".into();
        rec.cwd = "/repo/clave/.claude/worktrees/card".into();
        rec.worktree = None;
        rec.branch = "card".into();
        rec.default_branch = Some("trunk".into());
        rec.pr_number = Some(259);

        // A `cd` to the repo's own main checkout. One repository, and the row
        // must come through it whole.
        assert!(take_checkout(
            &mut rec,
            checkout_from_tail(&line("/repo/clave", "main"))
        ));
        assert_eq!(rec.cwd, "/repo/clave", "the move itself still happens");
        assert_eq!(rec.branch, "main");
        assert_eq!(
            rec.repo_root, "/repo/clave/.claude/worktrees/card",
            "only git could say the root moved, and nothing here asked it"
        );
        assert_eq!(rec.default_branch.as_deref(), Some("trunk"));
        assert_eq!(rec.pr_number, Some(259));

        // Even a move to a genuinely different repository leaves the recorded
        // facts alone. Clearing them cannot be undone; `pr::checkout_dir` asks
        // from the new cwd, so the stale root cannot mislead the question.
        assert!(take_checkout(
            &mut rec,
            checkout_from_tail(&line("/somewhere/else", "main"))
        ));
        assert_eq!(rec.cwd, "/somewhere/else");
        assert_eq!(
            rec.repo_root, "/repo/clave/.claude/worktrees/card",
            "still git's to settle, not a path comparison's"
        );
    }

    #[test]
    fn wants_lives_exactly_as_long_as_the_block_it_names() {
        // Lock §4.7: structure gates, the words only fill in. `wants` is
        // written for a row the status machine flagged, and dies with the flag
        // — never held past the block, never invented without one.
        let mut s = Store::default();
        s.agents.insert("u1".into(), rec("u1"));
        let msg = |m: &str| HookPayload {
            session_id: Some("u1".into()),
            message: Some(m.into()),
            ..HookPayload::default()
        };
        let bare = HookPayload {
            session_id: Some("u1".into()),
            ..HookPayload::default()
        };

        apply_hook_event(&mut s, "u1", "UserPromptSubmit", &bare, None, 1000, true);
        assert_eq!(s.agents["u1"].wants, None, "a working row wants nothing");

        let perm = msg("Claude needs your permission to use Bash");
        apply_hook_event(&mut s, "u1", "Notification", &perm, None, 1001, true);
        assert_eq!(s.agents["u1"].wants.as_deref(), Some("Bash"));

        // A repeat ask that names no tool arrives while the SAME block
        // stands: the interactive dialog's own text is the bare "Claude needs
        // your permission" (Claude Code 2.1.299). Blanking here would drop a
        // true reading for a message that carries no news.
        let bare_ask = msg("Claude needs your permission");
        apply_hook_event(&mut s, "u1", "Notification", &bare_ask, None, 1002, true);
        assert_eq!(
            s.agents["u1"].wants.as_deref(),
            Some("Bash"),
            "a repeat mid-block holds the ask it cannot restate"
        );

        // You answered: the batch resolved, so nothing is wanted.
        apply_hook_event(&mut s, "u1", "PostToolBatch", &bare, None, 1003, true);
        assert_eq!(s.agents["u1"].status, Status::Working);
        assert_eq!(
            s.agents["u1"].wants, None,
            "leaving needs-you clears the ask"
        );

        // The idle nag ends the turn; it never invents a block.
        let nag = msg("Claude is waiting for your input");
        apply_hook_event(&mut s, "u1", "Notification", &nag, None, 1004, true);
        assert_eq!(s.agents["u1"].status, Status::Idle);
        assert_eq!(s.agents["u1"].wants, None);
    }

    #[test]
    fn a_permission_notification_names_the_tool_it_is_blocked_on() {
        assert_eq!(
            wants_from_message("Claude needs your permission to use Bash").as_deref(),
            Some("Bash")
        );
        // The parenthetical the CLI sometimes appends is the useful half of
        // the ask, so it rides along rather than being trimmed to the bare
        // tool name.
        assert_eq!(
            wants_from_message("Claude needs your permission to use Bash (git push)").as_deref(),
            Some("Bash (git push)")
        );
        // Every other notification the hook already sees yields nothing: the
        // idle nag names no tool, and inventing one would be a measurement
        // the card never took.
        assert_eq!(wants_from_message("Claude is waiting for your input"), None);
        assert_eq!(wants_from_message("compacting…"), None);
        // A trailing anchor with nothing after it is not a tool name.
        assert_eq!(wants_from_message("… permission to use "), None);
    }

    #[test]
    fn first_words_clamps() {
        assert_eq!(
            first_words("fix the flaky auth test please"),
            "fix the flaky auth"
        );
        assert_eq!(first_words("short"), "short");
        assert!(first_words("averyveryverylongsingletokenthatkeepsgoing").len() <= 32);
    }

    #[test]
    fn summary_from_tail_takes_last_summary_line() {
        let tail = concat!(
            "{\"type\":\"user\",\"message\":\"hi\"}\n",
            "{\"type\":\"summary\",\"summary\":\"Old title\"}\n",
            "{\"type\":\"assistant\"}\n",
            "{\"type\":\"summary\",\"summary\":\"Fix auth flow\"}\n",
        );
        assert_eq!(summary_from_tail(tail).as_deref(), Some("Fix auth flow"));
        assert_eq!(summary_from_tail("{\"type\":\"user\"}\n"), None);
    }

    // ── S7, the context battery (#62) ───────────────────────────────────────

    /// A CAPTURE, not an invention — TESTING.md § "Fixtures captured from
    /// reality". The `usage` and `compactMetadata` objects inside are verbatim
    /// from the maintainer's own transcripts (2026-08-07), scrubbed of
    /// everything that is not shape. Its header records the provenance, the
    /// dated field measurement, and the re-measurement commands.
    ///
    /// This matters because `tokens_from_tail` is a SUBSTRING parser over an
    /// external format nobody here controls. A fixture reconstructed from the
    /// parser's own assumptions could only ever confirm them; this one can be
    /// contradicted by the world (Codex review, #147).
    const CAPTURE: &str = include_str!("../tests/fixtures/transcripts/compacted-session.jsonl");

    /// The capture's data lines: `#` header lines dropped, `n` lines kept.
    ///
    /// Slicing by line is how one capture serves every case — a pre-compaction
    /// tail, a tail ending AT the boundary, and a tail with a fresh turn past
    /// it are all prefixes of the same real session.
    fn capture(n: usize) -> String {
        let body: Vec<&str> = CAPTURE.lines().filter(|l| !l.starts_with('#')).collect();
        body[..n].join("\n") + "\n"
    }

    #[test]
    fn s7_the_capture_still_carries_every_shape_the_parser_reads() {
        // The hermetic half of TESTING.md's liveness assertion. Asserted
        // against the SAME consts production greps with, not a hand-copied
        // second list — a copy drifts, and a drifted copy passes.
        //
        // Scope, stated honestly because escape record 5 is precisely a
        // comment claiming more than its test proves: this catches a literal
        // that no longer appears in any real sample. It does NOT catch a
        // parser taught a shape via a fresh inline literal that was never
        // added to these consts. Keeping every grepped string in `USAGE_*` /
        // `BOUNDARY` / `POST_TOKENS` is what makes it worth anything, and that
        // part is convention, not compiler-enforced.
        for shape in USAGE_SUMMED {
            let key = format!("\"{shape}\":");
            assert!(CAPTURE.contains(&key), "no captured line carries {key}");
        }
        for shape in [USAGE_KEY, BOUNDARY, POST_TOKENS] {
            assert!(CAPTURE.contains(shape), "no captured line carries {shape}");
        }
    }

    #[test]
    fn s7_sums_the_newest_turns_three_input_counts() {
        // Two real assistant turns, no boundary yet. The newest wins: the
        // measured reading of that session was 2 + 211125 + 4989.
        assert_eq!(tokens_from_tail(&capture(2)), Some(216_116));
        // Proven against the decoys the capture carries for free — `iterations`
        // repeats every usage key per inference step, `cache_creation` nests
        // `ephemeral_*_input_tokens`, and `output_tokens` sits between the two
        // counts that ARE summed. Occupancy is what went IN.
        assert!(
            CAPTURE.contains("\"iterations\":["),
            "the decoy must survive"
        );
        assert!(CAPTURE.contains("\"output_tokens\":"));
        // No usage line anywhere is NOT zero — it is no reading, which holds.
        assert_eq!(tokens_from_tail("{\"type\":\"user\"}\n"), None);
    }

    #[test]
    fn s7_reads_past_a_compact_boundary_and_falls_back_to_its_post_tokens() {
        // Ending AT the boundary is the real few-second window after a manual
        // `/compact`: the pre-compaction lines still sit there naming 216k, and
        // no fresh turn has landed. Take the boundary's own exact figure, or the
        // battery paints a just-emptied session red.
        assert_eq!(tokens_from_tail(&capture(3)), Some(24_456));
        // One turn later, the fresh reading wins.
        assert_eq!(tokens_from_tail(&capture(4)), Some(37_437));
        // `preTokens` precedes `postTokens` on that same real line — a looser
        // scan would report the PRE-compaction size, the exact inverse.
        assert!(CAPTURE.contains("\"preTokens\":435777"));
    }

    #[test]
    fn s7_a_missing_usage_key_is_not_answered_by_the_iterations_copy() {
        // The turn's own `usage` nests an `iterations` array repeating every
        // key once per inference step. Drop `cache_read_input_tokens` from the
        // TOP level of a captured line and the flat scan this replaced would
        // have answered from `iterations` — reporting one inference step as if
        // it were the turn. Reading depth 1 only makes that unreachable.
        // (CodeRabbit, #147)
        let lines: Vec<&str> = CAPTURE.lines().filter(|l| !l.starts_with('#')).collect();
        let holed = lines[1].replacen("\"cache_read_input_tokens\":211125,", "", 1);
        assert!(
            holed.contains("\"cache_read_input_tokens\":211125"),
            "the iterations copy must still be there, or this proves nothing"
        );
        // 2 + 4989, with the 211125 inside `iterations` correctly ignored.
        assert_eq!(tokens_from_tail(&format!("{holed}\n")), Some(4_991));
    }

    #[test]
    fn s7_a_summed_key_after_a_nested_object_is_still_read() {
        // Today all three summed keys precede the first nested object, so
        // stopping at the first closing brace would happen to work. That is a
        // property of Claude's current key ORDER, not of the format, and key
        // order is exactly what a serializer reshuffles without telling anyone.
        // Move one past the nesting and it must still be read. (Surfaced by
        // `just mutants`: without this, ending the scan early survives.)
        let lines: Vec<&str> = CAPTURE.lines().filter(|l| !l.starts_with('#')).collect();
        let moved = lines[1]
            .replacen("\"cache_read_input_tokens\":211125,", "", 1)
            .replacen(
                "\"iterations\":",
                "\"cache_read_input_tokens\":211125,\"iterations\":",
                1,
            );
        assert_eq!(tokens_from_tail(&format!("{moved}\n")), Some(216_116));
    }

    #[test]
    fn s7_a_reading_that_moves_inside_one_bucket_still_pushes() {
        // The glyph only moves once per tenth of the zone, but #105 renders the
        // raw count as TEXT. Gating the push on the level alone would leave
        // that text stale for up to 15k tokens at the default zone. Both
        // fields have to gate it. (Surfaced by `just mutants`: without this,
        // narrowing the condition to AND survives.)
        let lines: Vec<&str> = CAPTURE.lines().filter(|l| !l.starts_with('#')).collect();
        // 2 + 998 + 0 = 1000 — a real line shape, a different number. Both
        // this and the parked value bucket to level 0 against the 150k default.
        let small = lines[0]
            .replacen(
                "\"cache_creation_input_tokens\":15884",
                "\"cache_creation_input_tokens\":998",
                1,
            )
            .replacen(
                "\"cache_read_input_tokens\":21551",
                "\"cache_read_input_tokens\":0",
                1,
            );

        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        let r = s.agents.get_mut("minted").unwrap();
        r.context_tokens = Some(0);
        r.context_level = Some(0);
        let payload = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        // A QUIET event — an unmatched `Notification` maps to no status and
        // reorders nothing — so `changed` reflects the battery alone. `Stop`
        // would flip Idle→Done and push regardless, masking the very thing
        // under test; that masking is why this survived a mutation round.
        assert!(apply_hook_event(
            &mut s,
            "minted",
            "Notification",
            &payload,
            Some(&format!("{small}\n")),
            100,
            true
        ));
        assert_eq!(s.agents["minted"].context_tokens, Some(1_000));
        assert_eq!(
            s.agents["minted"].context_level,
            Some(0),
            "the level must NOT have moved, or this proves nothing"
        );
    }

    #[test]
    fn s7_only_the_newest_boundary_anchors() {
        // A session compacted twice. Built from the captured boundary rather
        // than a written one; the figures differ so reading the earlier line
        // would be visible rather than coincidentally right.
        let lines: Vec<&str> = CAPTURE.lines().filter(|l| !l.starts_with('#')).collect();
        let newest = lines[2];
        let older = newest.replace("24456", "99999");
        assert_eq!(
            tokens_from_tail(&format!("{}\n{older}\n{newest}\n", lines[0])),
            Some(24_456)
        );
    }

    #[test]
    fn s7_a_zero_sum_usage_line_is_not_a_reading() {
        // Occupancy is never zero on a real turn, so an all-zero `usage` is a
        // malformed line, not a measurement. Taking it would paint a full
        // battery on a session that may be nearly out — the one failure a meter
        // must not have. Zeroed from the captured line so the shape stays real.
        let lines: Vec<&str> = CAPTURE.lines().filter(|l| !l.starts_with('#')).collect();
        let zeroed = |l: &str| {
            l.replace("\"input_tokens\":2", "\"input_tokens\":0")
                .replace(
                    "\"cache_creation_input_tokens\":15884",
                    "\"cache_creation_input_tokens\":0",
                )
                .replace(
                    "\"cache_read_input_tokens\":21551",
                    "\"cache_read_input_tokens\":0",
                )
        };
        assert_eq!(tokens_from_tail(&format!("{}\n", zeroed(lines[0]))), None);
        // And it must not shadow a boundary's exact figure either.
        let tail = format!("{}\n{}\n", lines[2], zeroed(lines[0]));
        assert_eq!(tokens_from_tail(&tail), Some(24_456));
    }

    #[test]
    fn s7_smart_zone_falls_back_rather_than_failing() {
        let default = clave_types::DEFAULT_SMART_ZONE_TOKENS;
        assert_eq!(smart_zone_from(Some("120000")), 120_000);
        assert_eq!(
            smart_zone_from(Some("  120000 \n")),
            120_000,
            "shell exports drag whitespace"
        );
        assert_eq!(smart_zone_from(None), default);
        assert_eq!(smart_zone_from(Some("")), default);
        assert_eq!(
            smart_zone_from(Some("150k")),
            default,
            "junk falls back, never fails"
        );
        // Zero is the dangerous one: it parses, and a zero zone has no ramp to
        // divide. A hook must never fail hard (§6.5).
        assert_eq!(smart_zone_from(Some("0")), default);
    }

    #[test]
    fn s7_ramp_puts_red_at_the_zone_and_clamps_beyond_it() {
        let z = 150_000;
        let top = clave_types::BATTERY_LEVELS - 1;
        // Full until a tenth is actually spent; one step per tenth thereafter.
        assert_eq!(battery_level(0, z), 0);
        assert_eq!(battery_level(14_999, z), 0);
        assert_eq!(battery_level(15_000, z), 1);
        assert_eq!(battery_level(90_000, z), 6); // ink crosses to yellow here
        assert_eq!(battery_level(120_000, z), 8); // and to orange here
        // The zone is where it turns RED, not where the ramp ends.
        assert_eq!(battery_level(149_999, z), 9);
        assert_eq!(battery_level(z, z), top);
        assert_eq!(
            battery_level(216_116, z),
            top,
            "clamps rather than overflowing"
        );
        assert_eq!(
            battery_level(u32::MAX, 1),
            top,
            "no overflow before the clamp"
        );
        // A zero zone would divide by nothing; a hook must never fail hard.
        assert_eq!(battery_level(50_000, 0), 0);
    }

    #[test]
    fn s7_rotation_resets_the_battery_and_pushes() {
        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        s.agents.get_mut("minted").unwrap().context_tokens = Some(216_116);
        s.agents.get_mut("minted").unwrap().context_level = Some(10);
        let ev = |session: &str| HookPayload {
            session_id: Some(session.into()),
            ..Default::default()
        };

        // `/clear` mints a new id and a new transcript. The conversation the row
        // is now IN holds nothing, so the battery is full — and that IS a pixel,
        // so it pushes.
        assert!(apply_hook_event(
            &mut s,
            "minted",
            "Notification",
            &ev("cleared"),
            None,
            100,
            true
        ));
        assert_eq!(s.agents["minted"].context_tokens, Some(0));
        assert_eq!(s.agents["minted"].context_level, Some(0));

        // A second event on the SAME conversation is not a rotation, and a
        // tail-less event holds rather than re-reading.
        s.agents.get_mut("minted").unwrap().context_tokens = Some(40_000);
        s.agents.get_mut("minted").unwrap().context_level = Some(2);
        apply_hook_event(
            &mut s,
            "minted",
            "Notification",
            &ev("cleared"),
            None,
            101,
            true,
        );
        assert_eq!(s.agents["minted"].context_tokens, Some(40_000));
    }

    #[test]
    fn s7_a_tail_without_usage_holds_the_previous_reading() {
        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        s.agents.get_mut("minted").unwrap().context_tokens = Some(90_000);
        s.agents.get_mut("minted").unwrap().context_level = Some(6);
        let payload = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        // Never invent a measurement: a readable tail with nothing to read is
        // not a reading of zero.
        apply_hook_event(
            &mut s,
            "minted",
            "Stop",
            &payload,
            Some("{\"type\":\"user\",\"message\":\"go on\"}\n"),
            100,
            true,
        );
        assert_eq!(s.agents["minted"].context_tokens, Some(90_000));
        assert_eq!(s.agents["minted"].context_level, Some(6));
    }

    #[test]
    fn refresh_label_upgrades_first_prompt_then_summary_then_stops() {
        let mut r = rec("u1"); // label "x · main", label_source FirstPrompt
        // 1) First prompt arrives IN the UserPromptSubmit payload — no jsonl
        //    read needed for this step (§6.4 fast path).
        let p = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("fix the flaky auth test".into()),
            ..Default::default()
        };
        assert!(refresh_label(&mut r, "UserPromptSubmit", &p, None));
        assert_eq!(r.label, "x · main · fix the flaky auth");
        assert_eq!(r.label_source, LabelSource::FirstPrompt);
        // 2) A summary in the jsonl tail wins and flips the source.
        let tail = "{\"type\":\"summary\",\"summary\":\"Fix auth flow\"}\n";
        assert!(refresh_label(&mut r, "Stop", &p, Some(tail)));
        assert_eq!(r.label, "x · main · Fix auth flow");
        assert_eq!(r.label_source, LabelSource::Summary);
        // 3) Once Summary, we STOP re-deriving (§6.4) — even with new input.
        assert!(!refresh_label(&mut r, "Stop", &p, Some(tail)));
    }

    #[test]
    fn injected_task_notification_does_not_earn_label_but_next_prompt_does() {
        // Issue #17, observed live 2026-07-21: resuming a session that died
        // with a pending background task makes Claude Code auto-fire a turn
        // whose "prompt" is an orphaned <task-notification> tag, not user
        // intent. Before this guard, refresh_label's first-prompt upgrade
        // earned it verbatim and the garbage label stuck forever (earned
        // labels don't self-heal). Skip the upgrade entirely: label stays
        // bare so the label_source is still FirstPrompt for the next turn.
        let mut r = rec("u1");
        let injected = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("<task-notification> <task-id>bai</task-notification>".into()),
            ..Default::default()
        };
        assert!(!refresh_label(&mut r, "UserPromptSubmit", &injected, None));
        assert_eq!(r.label, "x · main"); // unchanged — still the bare prefix
        assert_eq!(r.label_source, LabelSource::FirstPrompt); // still eligible

        // The next REAL prompt earns the label exactly as before the guard.
        let real = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("fix the flaky auth test".into()),
            ..Default::default()
        };
        assert!(refresh_label(&mut r, "UserPromptSubmit", &real, None));
        assert_eq!(r.label, "x · main · fix the flaky auth");
    }

    #[test]
    fn every_injected_prefix_is_blocked_on_both_earn_paths() {
        // #17 breadth (CodeRabbit, PR #25): the const is expected to GROW, and
        // a prefix that guards only one earn path is a latent re-leak — so
        // table-drive EVERY prefix through BOTH paths, with leading whitespace
        // (the guard trims start, matching how the harness pads injections).
        for prefix in HARNESS_TURN_PREFIXES.iter().chain(TYPED_WRAPPER_PREFIXES) {
            let injected_text = format!("  \n\t{prefix}> payload we must never earn");
            // First-prompt path: the upgrade is skipped outright.
            let mut r = rec("u1");
            let p = HookPayload {
                session_id: Some("u1".into()),
                prompt: Some(injected_text.clone()),
                ..Default::default()
            };
            assert!(
                !refresh_label(&mut r, "UserPromptSubmit", &p, None),
                "prompt path leaked for prefix {prefix}"
            );
            assert_eq!(r.label, "x · main");
            assert_eq!(r.label_source, LabelSource::FirstPrompt);
            // Summary path: an injected summary must not flip the source
            // either (defensive — summaries come from the CLI's own
            // compaction, but the rule is "never earn from injected text").
            let tail = format!(
                "{{\"type\":\"summary\",\"summary\":{}}}\n",
                serde_json::to_string(&injected_text).unwrap()
            );
            assert!(
                !refresh_label(&mut r, "Stop", &p, Some(&tail)),
                "summary path leaked for prefix {prefix}"
            );
            assert_eq!(r.label, "x · main");
            assert_eq!(r.label_source, LabelSource::FirstPrompt);
        }
    }

    /// One `ai-title` jsonl line, JSON-escaped so a fixture with quotes or
    /// backslashes stays a valid tail.
    fn ai_title_line(v: &str) -> String {
        format!(
            "{{\"type\":\"ai-title\",\"aiTitle\":{},\"sessionId\":\"u1\"}}\n",
            serde_json::to_string(v).unwrap()
        )
    }

    /// One `custom-title` jsonl line — the exact three-key shape verified
    /// against a live transcript 2026-07-29.
    fn custom_title_line(v: &str) -> String {
        format!(
            "{{\"type\":\"custom-title\",\"customTitle\":{},\"sessionId\":\"u1\"}}\n",
            serde_json::to_string(v).unwrap()
        )
    }

    /// One `away_summary` jsonl line — the shape measured on #111
    /// (2026-08-01): a `type:"system"` line discriminated by `subtype`, with a
    /// plain-string `content` inside the full conversation-line envelope.
    fn away_summary_line(v: &str) -> String {
        format!(
            "{{\"type\":\"system\",\"subtype\":\"away_summary\",\"content\":{},\"timestamp\":\"2026-08-01T10:00:00.000Z\",\"sessionId\":\"u1\",\"uuid\":\"w1\"}}\n",
            serde_json::to_string(v).unwrap()
        )
    }

    #[test]
    fn summary_is_written_structurally_not_only_into_the_label() {
        // Design-lock §7.1: the bar lays its own fixed-width columns and reads
        // `Agent.summary` — a summary that exists only as the label's third
        // \u{00b7}-segment renders a BLANK column. That was the regression.
        let mut r = rec("u1");
        let p = HookPayload::default();
        let tail = ai_title_line("Wire the summary column to the store");
        assert!(refresh_label(&mut r, "Stop", &p, Some(&tail)));
        assert_eq!(r.summary, "Wire the summary column to the store");
    }

    #[test]
    fn summary_keeps_rolling_after_the_label_has_frozen() {
        // The §6.4 freeze is a rule about the LABEL (a zellij tab name, which
        // meaningfully changes once). The summary column is a rolling
        // one-liner of what the agent is doing NOW, so it is decoupled: the
        // label stays pinned to the first summary, `rec.summary` keeps moving.
        let mut r = rec("u1");
        let p = HookPayload::default();
        // Freeze the label through its OWN (legacy) tier — the label's source
        // is deliberately not retargeted here, so an ai-title alone never
        // flips `label_source`. That asymmetry is the point of this test.
        let freeze = "{\"type\":\"summary\",\"summary\":\"Read the transcript tail\"}\n";
        assert!(refresh_label(&mut r, "Stop", &p, Some(freeze)));
        assert_eq!(r.label_source, LabelSource::Summary);
        let frozen = r.label.clone();
        assert_eq!(r.summary, "Read the transcript tail");

        // Two more turns, each with a NEW ai-title: summary follows, label does not.
        for next in ["Write the row fields", "Run the four gates"] {
            assert!(
                refresh_label(&mut r, "Stop", &p, Some(&ai_title_line(next))),
                "a fresh summary after the freeze must still count as a change"
            );
            assert_eq!(r.summary, next);
            assert_eq!(r.label, frozen, "the label must stay frozen (\u{00a7}6.4)");
        }
        // An UNCHANGED tail is a no-op — \u{00a7}5 forbids no-op pushes.
        assert!(!refresh_label(
            &mut r,
            "Stop",
            &p,
            Some(&ai_title_line("Run the four gates"))
        ));
    }

    #[test]
    fn harness_injected_summary_writes_neither_field() {
        // #17's rule is about the SOURCE, not about which field it lands in:
        // an injected tag must never earn the label, the summary, or the title.
        for prefix in HARNESS_TURN_PREFIXES.iter().chain(TYPED_WRAPPER_PREFIXES) {
            let injected = format!("  \n\t{prefix}> payload we must never earn");
            let mut r = rec("u1");
            let p = HookPayload::default();
            let tail = format!(
                "{}{}{}",
                ai_title_line(&injected),
                custom_title_line(&injected),
                away_summary_line(&injected)
            );
            assert!(
                !refresh_label(&mut r, "Stop", &p, Some(&tail)),
                "injected text leaked into a row field for prefix {prefix}"
            );
            assert!(r.summary.is_empty());
            assert_eq!(r.title, None);
            assert_eq!(r.label, "x \u{00b7} main");
        }
    }

    #[test]
    fn stored_summary_outruns_the_label_truncation_and_is_still_bounded() {
        // The label uses first_words (4 words / 32 chars) because a tab name
        // is short. The bar's summary column is 17 cells at the profile that
        // ships today and is set to widen (LEDGER D19), and `render.rs` clamps
        // cell-accurately — so the store must hold MORE than the label does,
        // bounded but generous.
        let long =
            "alpha bravo charlie delta echo foxtrot golf hotel india juliett kilo lima ".repeat(8);
        let mut r = rec("u1");
        let p = HookPayload::default();
        assert!(refresh_label(
            &mut r,
            "Stop",
            &p,
            Some(&ai_title_line(&long))
        ));
        assert!(
            r.summary.chars().count() > first_words(&long).chars().count(),
            "stored summary must outrun the label's first_words truncation"
        );
        assert_eq!(r.summary.chars().count(), SUMMARY_MAX_CHARS);
        assert!(r.summary.starts_with("alpha bravo charlie delta echo"));
    }

    #[test]
    fn ai_title_beats_the_extinct_summary_line_and_the_prompt_seed() {
        // #79: `type:"summary"` appears in 0 of 153 local transcripts; it is
        // kept only as a legacy fallback BEHIND ai-title. And the prompt seed
        // is the lowest tier of all — it exists so a live row is never blank
        // before Claude has written its first ai-title.
        let p = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("a prompt that must not win".into()),
            ..Default::default()
        };
        let mut r = rec("u1");
        let tail = format!(
            "{{\"type\":\"summary\",\"summary\":\"legacy\"}}\n{}",
            ai_title_line("live")
        );
        assert!(refresh_label(&mut r, "UserPromptSubmit", &p, Some(&tail)));
        assert_eq!(r.summary, "live");

        // Legacy alone still works — the pinned freeze test depends on it.
        let mut r = rec("u1");
        let legacy = "{\"type\":\"summary\",\"summary\":\"legacy\"}\n";
        assert!(refresh_label(&mut r, "UserPromptSubmit", &p, Some(legacy)));
        assert_eq!(r.summary, "legacy");

        // No tail at all: the prompt seeds the column, ONCE. A later prompt
        // must not overwrite it — only an ai-title may.
        let mut r = rec("u1");
        assert!(refresh_label(&mut r, "UserPromptSubmit", &p, None));
        assert_eq!(r.summary, "a prompt that must not win");
        let p2 = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("a later prompt".into()),
            ..Default::default()
        };
        refresh_label(&mut r, "UserPromptSubmit", &p2, None);
        assert_eq!(r.summary, "a prompt that must not win");
        assert!(refresh_label(
            &mut r,
            "UserPromptSubmit",
            &p2,
            Some(&ai_title_line("earned"))
        ));
        assert_eq!(r.summary, "earned");
    }

    /// TESTING.md's external-format row: the parser against a CAPTURED
    /// record, not an invented one — the fixture is a verbatim 2026-07-15
    /// field capture (Claude Code 2.1.209), home path scrubbed, byte order
    /// preserved. Liveness measurement, dated: 50 local transcripts carried
    /// the shape on 2026-08-01, newest stamp 2026-07-31 (#111, #131). If the
    /// field stops appearing, re-measure before trusting this tier.
    #[test]
    fn the_captured_away_summary_record_parses_as_found_in_the_field() {
        let captured = include_str!("../tests/fixtures/transcripts/away-summary-2026-07-15.jsonl");
        assert_eq!(
            away_summary_from_tail(captured).as_deref(),
            Some(
                "Validating clave's tab bar: the Alt+c toggle fix now works in a \
                 single tab with no storms. Next: create 2\u{2013}3 tabs and retest \
                 toggling, tab-switch behavior, per-tab healing, and nav before I \
                 strip traces and we commit."
            )
        );
    }

    #[test]
    fn away_summary_outranks_ai_title_and_the_newest_recap_wins() {
        // #131: the recap is the only signal that can upgrade a fleet-born
        // row's summary — per #111 those never earn an ai-title — and it is
        // re-generated per away period, so the LAST one in the tail is the
        // current session state.
        let p = HookPayload::default();
        let mut r = rec("u1");
        let tail = format!(
            "{}{}{}",
            away_summary_line("stale recap from the first away period"),
            ai_title_line("the one auto-title"),
            away_summary_line("fresh recap: gates green, next wire the tier")
        );
        assert!(refresh_label(&mut r, "Stop", &p, Some(&tail)));
        assert_eq!(r.summary, "fresh recap: gates green, next wire the tier");

        // The tier is AUTHORITY, not file position: an ai-title stamped after
        // the recap (every reopen re-appends it) still loses to it.
        let mut r = rec("u1");
        let tail = format!(
            "{}{}",
            away_summary_line("the recap"),
            ai_title_line("the one auto-title")
        );
        assert!(refresh_label(&mut r, "Stop", &p, Some(&tail)));
        assert_eq!(r.summary, "the recap");

        // …and an earned recap never regresses to prompt text: the seed tier
        // stays fill-only-when-empty. (refresh_label still returns true here —
        // the LABEL is still bare and legitimately earns the prompt; the
        // property under test is the summary holding.)
        let prompted = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("a prompt that must not win".into()),
            ..Default::default()
        };
        refresh_label(&mut r, "UserPromptSubmit", &prompted, None);
        assert_eq!(r.summary, "the recap");
    }

    #[test]
    fn a_system_line_with_another_subtype_is_not_a_recap() {
        // The system channel carries many subtypes; matching on `type` alone —
        // what `last_tail_field` does for every other tier — would graft
        // arbitrary system prose into the summary column.
        let p = HookPayload::default();
        let mut r = rec("u1");
        let tail = format!(
            "{}{}",
            ai_title_line("the auto-title"),
            "{\"type\":\"system\",\"subtype\":\"turn_duration\",\"content\":\"not a recap\"}\n"
        );
        assert!(refresh_label(&mut r, "Stop", &p, Some(&tail)));
        assert_eq!(r.summary, "the auto-title");

        // An EMPTY recap is skipped, not returned — the scan continues to the
        // last real one, matching the /clear rule every other tier follows.
        let mut r = rec("u1");
        let tail = format!(
            "{}{}",
            away_summary_line("the real recap"),
            away_summary_line("  ")
        );
        assert!(refresh_label(&mut r, "Stop", &p, Some(&tail)));
        assert_eq!(r.summary, "the real recap");

        // No recap at all: the existing tiers stand exactly as before.
        let mut r = rec("u1");
        assert!(refresh_label(
            &mut r,
            "Stop",
            &p,
            Some(&ai_title_line("the auto-title"))
        ));
        assert_eq!(r.summary, "the auto-title");
    }

    #[test]
    fn a_long_recap_rides_the_same_summary_bound() {
        // A recap is one-to-two SENTENCES, the longest prose this column has
        // ever carried — it rides the existing SUMMARY_MAX_CHARS clamp, no
        // new bound (the store holds prose; render.rs clamps cells).
        let long = "charting the wayfinder map and wiring fourteen tickets ".repeat(8);
        let mut r = rec("u1");
        let p = HookPayload::default();
        assert!(refresh_label(
            &mut r,
            "Stop",
            &p,
            Some(&away_summary_line(&long))
        ));
        assert_eq!(r.summary.chars().count(), SUMMARY_MAX_CHARS);
        assert!(r.summary.starts_with("charting the wayfinder map"));
    }

    #[test]
    fn title_comes_from_custom_title_and_is_held_last_non_empty() {
        // Design-lock §5/§7.1: `title` is Claude's session rename and nothing
        // else. Verified live 2026-07-29 — `/rename` writes
        // {"type":"custom-title","customTitle":…,"sessionId":…} to the jsonl,
        // re-appended latest-wins.
        let mut r = rec("u1");
        let p = HookPayload::default();
        assert_eq!(r.title, None); // never renamed = blank chip, by design
        assert!(refresh_label(
            &mut r,
            "Stop",
            &p,
            Some(&custom_title_line("NAL-MAIN"))
        ));
        assert_eq!(r.title.as_deref(), Some("NAL-MAIN"));

        // A later rename wins; an identical one is a no-op.
        assert!(refresh_label(
            &mut r,
            "Stop",
            &p,
            Some(&custom_title_line("NAL-WT"))
        ));
        assert_eq!(r.title.as_deref(), Some("NAL-WT"));
        assert!(!refresh_label(
            &mut r,
            "Stop",
            &p,
            Some(&custom_title_line("NAL-WT"))
        ));

        // `/clear` appends an EMPTY custom-title; #24's ruling is that the
        // last real rename is held across it, never blanked.
        let cleared = format!("{}{}", custom_title_line("NAL-WT"), custom_title_line(""));
        assert!(!refresh_label(&mut r, "Stop", &p, Some(&cleared)));
        assert_eq!(r.title.as_deref(), Some("NAL-WT"));
        // A tail that has scrolled past the rename entirely does not blank it.
        assert!(!refresh_label(
            &mut r,
            "Stop",
            &p,
            Some("{\"type\":\"user\"}\n")
        ));
        assert_eq!(r.title.as_deref(), Some("NAL-WT"));
    }

    #[test]
    fn clamp_field_is_single_line_and_char_bounded() {
        // LEDGER D14: `unicode-width` reports 0 cells for C0/C1 controls, so a
        // stored `\n` or `\u{1b}` breaks a row that still passes the
        // every-row-is-cols-cells test. Collapse them at the write site too.
        assert_eq!(clamp_field("a\nb\tc  d", 64), "a b c d");
        assert_eq!(clamp_field("  padded \u{1b}[0m ", 64), "padded [0m");
        // Multibyte input clamps on a CHAR boundary, never mid-codepoint.
        let wide = "\u{3042}".repeat(50);
        assert_eq!(clamp_field(&wide, 10).chars().count(), 10);
        // Unlike sanitize_label it does NOT delete quotes/backslashes: neither
        // title nor summary is ever baked into launch.kdl, and dropping
        // characters out of displayed prose would be lossy for no gain.
        assert_eq!(clamp_field(r#"the \d "regex""#, 64), r#"the \d "regex""#);
    }

    #[test]
    fn refresh_label_sanitizes_kdl_metacharacters() {
        // Fugu 2026-07-21 (pre-v0.1.0, HIGH): hook-derived labels are baked
        // into launch.kdl for the most-recent row (setup.rs) — a raw
        // backslash is a KDL escape introducer and a raw quote closes the
        // string literal, so an unsanitized label can brick cold-start
        // launch. Labels must pass sanitize_label like add.rs-built ones.
        let mut r = rec("u1");
        let p = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some(r#"fix the \d "regex" now"#.into()),
            ..Default::default()
        };
        assert!(refresh_label(&mut r, "UserPromptSubmit", &p, None));
        assert!(
            !r.label.contains('\\') && !r.label.contains('"'),
            "unsanitized label: {}",
            r.label
        );
    }

    /// #226 adoption gate: a pane identity is trusted only when the hook fired
    /// INSIDE clave's own zellij session — pane ids are session-scoped, so a
    /// claude in another session (or under no zellij) must contribute nothing.
    #[test]
    fn adoption_pane_requires_the_own_session_and_a_parseable_pane() {
        let pane = |sess: Option<&str>, pane: Option<&str>| {
            adoption_pane_from(sess.map(str::to_string), pane.map(str::to_string), "clave")
        };
        assert_eq!(pane(Some("clave"), Some("7")), Some(7));
        assert_eq!(pane(Some("clave-test"), Some("7")), None, "foreign session");
        assert_eq!(pane(None, Some("7")), None, "no zellij at all");
        assert_eq!(pane(Some("clave"), None), None, "no pane id");
        assert_eq!(pane(Some("clave"), Some("plugin:x")), None, "unparseable");
    }

    /// #226 live adoption, register half: a hook firing from a verified clave
    /// pane writes that pane onto its row — the only pane-id producer besides
    /// `clave spawn`, which is what lets a hand-resumed claude's tab bind (and
    /// a mis-pruned row heal, #195). Change-gated like `apply_register`; the
    /// bar renders/joins from `pane_id`, so a real write bumps `seq`.
    #[test]
    fn a_hook_from_a_clave_pane_registers_it_and_re_registration_is_free() {
        let mut s = Store::default();
        s.agents.insert("u1".into(), rec("u1"));
        let before = s.seq;
        assert!(apply_hook_pane(
            &mut s,
            "u1",
            "Notification",
            Some(7),
            None,
            0
        ));
        assert_eq!(s.agents["u1"].pane_id, Some(7));
        assert_eq!(s.seq, before + 1, "pane_id is bar-rendered: seq bumps");
        assert!(
            !apply_hook_pane(&mut s, "u1", "Notification", Some(7), None, 0),
            "re-registration of the same pane is free"
        );
        assert!(
            !apply_hook_pane(&mut s, "u1", "Notification", None, None, 0),
            "no verified pane, no write"
        );
        assert_eq!(
            s.agents["u1"].pane_id,
            Some(7),
            "an absent pane never erases"
        );
        // Last-writer-wins (#226 ruling): resuming the same session in a second
        // pane steals the register; the old tab reverts via the bind eviction.
        assert!(apply_hook_pane(
            &mut s,
            "u1",
            "Notification",
            Some(9),
            None,
            0
        ));
        assert_eq!(s.agents["u1"].pane_id, Some(9));
    }

    /// #226 live adoption, revert half: exiting claude drops the pane to its
    /// shell, so a `SessionEnd` from the pane the row OWNS clears the register
    /// and the bind — the tab renders as a terminal tab again, the row goes
    /// dormant, and a later `claude --resume` in that shell re-adopts (the
    /// closed loop). Pane-match is the test: any other pane's `SessionEnd`
    /// (or one with no verified pane) must not strip a live association.
    #[test]
    fn session_end_from_the_owning_pane_reverts_the_tab_to_terminal() {
        let mut s = Store::default();
        let mut r = rec("u1");
        r.pane_id = Some(7);
        r.tab_id = Some(3);
        s.agents.insert("u1".into(), r);
        // A stranger pane's SessionEnd (pane 9) clears nothing.
        assert!(!apply_hook_pane(
            &mut s,
            "u1",
            "SessionEnd",
            Some(9),
            None,
            0
        ));
        assert_eq!(s.agents["u1"].pane_id, Some(7));
        assert_eq!(s.agents["u1"].tab_id, Some(3));
        // No verified pane: also nothing.
        assert!(!apply_hook_pane(&mut s, "u1", "SessionEnd", None, None, 0));
        assert_eq!(s.agents["u1"].tab_id, Some(3));
        // The owning pane's SessionEnd reverts — both halves, one seq bump.
        let before = s.seq;
        assert!(apply_hook_pane(
            &mut s,
            "u1",
            "SessionEnd",
            Some(7),
            None,
            0
        ));
        assert_eq!(s.agents["u1"].pane_id, None);
        assert_eq!(s.agents["u1"].tab_id, None);
        assert_eq!(s.seq, before + 1);
        // Idempotent: a second SessionEnd finds nothing to clear.
        assert!(!apply_hook_pane(
            &mut s,
            "u1",
            "SessionEnd",
            Some(7),
            None,
            0
        ));
    }

    /// Standby (2026-09-22): a zellij quit fires SessionEnd for every running
    /// agent, and the unbind below erases the only record that the row was
    /// live (QA run 12). So the end itself stamps it — with the tab, which a
    /// later prune of that tab uses to dismiss the stamp. The human's own
    /// endings (`/exit`, `/clear`, `/resume`, `/logout`) stamp nothing, and a
    /// row with no tab was never live.
    #[test]
    fn a_session_end_carries_the_tabs_rank_into_the_row() {
        // The same rule as the prune (R2): the row takes its tab's ordinal
        // before the unbind drops the tab, so the quit moves nothing.
        let mut r = rec("u1");
        r.pane_id = Some(7);
        r.tab_id = Some(3);
        r.commit_ord = 5;
        let mut s = Store::default();
        s.agents.insert("u1".into(), r);
        s.tab_order.insert(3, 12);
        assert!(apply_hook_pane(
            &mut s,
            "u1",
            "SessionEnd",
            Some(7),
            Some("other"),
            1_000
        ));
        assert_eq!(s.agents["u1"].commit_ord, 12);
    }

    #[test]
    fn a_session_end_by_quit_stamps_standby_and_the_humans_own_end_does_not() {
        let bound = |pane| {
            let mut r = rec("u1");
            r.pane_id = Some(pane);
            r.tab_id = Some(3);
            let mut s = Store::default();
            s.agents.insert("u1".into(), r);
            s
        };
        {
            let mut s = bound(7);
            assert!(apply_hook_pane(
                &mut s,
                "u1",
                "SessionEnd",
                Some(7),
                Some("other"),
                1_000
            ));
            assert_eq!(
                s.agents["u1"].standby_stamp,
                Some(crate::store::Standby {
                    since: 1_000,
                    tab: Some(3)
                })
            );
            assert_eq!(s.agents["u1"].tab_id, None, "the unbind still happens");
        }
        // No reason is not a quit either. A standby row opens on arrival, so
        // a false stamp spawns an agent the human ended; a false dormant row
        // costs one Alt+Enter. A hand-fired hook (the QA drive) sends none.
        for reason in [
            Some("prompt_input_exit"),
            Some("clear"),
            Some("resume"),
            Some("logout"),
            None,
        ] {
            let mut s = bound(7);
            assert!(apply_hook_pane(
                &mut s,
                "u1",
                "SessionEnd",
                Some(7),
                reason,
                1_000
            ));
            assert_eq!(s.agents["u1"].standby_stamp, None, "{reason:?}");
        }
        // No tab bound: the agent was never live in a tab, so nothing to keep.
        let mut s = bound(7);
        s.agents.get_mut("u1").unwrap().tab_id = None;
        assert!(apply_hook_pane(
            &mut s,
            "u1",
            "SessionEnd",
            Some(7),
            Some("other"), // a quit's reason: only the missing tab says no
            1_000
        ));
        assert_eq!(s.agents["u1"].standby_stamp, None);
    }

    /// #226 live adoption, mint half: a session clave has never seen, speaking
    /// from a verified clave pane, becomes a row — the jsonl is the source of
    /// truth, so any claude the user runs by hand joins the fleet. The mint is
    /// MINIMAL (uuid + cwd + the base label); richness arrives through the
    /// same refresh machinery every row uses. Label must be the byte-exact
    /// `<dir> · <branch>` base form — `refresh_label` reconstructs that prefix
    /// to gate the first-prompt upgrade (the run_add cross-task coupling).
    #[test]
    fn an_unknown_session_from_a_clave_pane_mints_a_minimal_row() {
        let mut s = Store::default();
        let uuid = mint_adopted(
            &mut s,
            "sess-1",
            "/home/u/proj",
            "/home/u/proj",
            "main",
            None,
            None,
        );
        assert_eq!(uuid, "sess-1");
        let r = &s.agents["sess-1"];
        assert_eq!(r.cwd, "/home/u/proj");
        assert_eq!(r.label, "proj · main");
        assert_eq!(r.status, Status::Idle);
        assert_eq!((r.tab_id, r.pane_id), (None, None), "register/bind follow");
        // Racing hooks both pass the mint gate under their own lock turns: the
        // second mint must land on merge_resume_record's preserve path, not
        // clobber what the first (or an old row) already earned.
        let earned = {
            let r = s.agents.get_mut("sess-1").unwrap();
            r.title = Some("DJ".into());
            r.buckets.insert(1, 4);
            r.buckets.clone()
        };
        mint_adopted(
            &mut s,
            "sess-1",
            "/home/u/proj",
            "/home/u/proj",
            "main",
            None,
            None,
        );
        let r = &s.agents["sess-1"];
        assert_eq!(r.title.as_deref(), Some("DJ"), "existing row preserved");
        assert_eq!(r.buckets, earned, "earned buckets preserved");
    }

    /// #226 mutants escape: the mint derivation must actually derive — a
    /// `None`-swallowed `adoption_inputs` silently declines every adoption
    /// and no other test notices. Contract pinned: an existing cwd mints
    /// (canonicalized, S0b), fallbacks are non-empty, no transcript means no
    /// derived weight, and a vanished cwd declines.
    #[test]
    fn adoption_inputs_canonicalize_the_cwd_and_decline_a_missing_one() {
        let dir = std::env::temp_dir().join("clave-adopt-inputs-test");
        std::fs::create_dir_all(&dir).unwrap();
        let m = adoption_inputs(
            dir.to_str().unwrap(),
            Path::new("/nonexistent-claude-dir"),
            "sess-x",
        )
        .expect("an existing cwd yields mint inputs");
        let physical = std::fs::canonicalize(&dir).unwrap();
        assert_eq!(m.cwd, physical.to_str().unwrap());
        assert!(!m.repo_root.is_empty() && !m.branch.is_empty());
        assert_eq!(m.own_buckets, None, "no transcript → no derived weight");
        assert_eq!(m.default_branch, None, "non-repo dir has no default branch");
        assert!(adoption_inputs("/definitely/not/a/dir", Path::new("/x"), "s").is_none());
    }

    /// #227 review: a cwd that `validate_cwd` would refuse to bake must
    /// decline ADOPTION too — the adopted row becomes the newest, launch's
    /// eager pick then rejects the stored cwd, and one hand-started claude in
    /// a quote-named directory wedges every later `clave` start.
    #[test]
    fn adoption_inputs_decline_a_kdl_unsafe_cwd() {
        let dir = std::env::temp_dir().join("clave-adopt-\"unsafe\"-test");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(
            adoption_inputs(dir.to_str().unwrap(), Path::new("/x"), "s").is_none(),
            "a KDL-unsafe cwd must decline the mint, not persist"
        );
    }

    /// #227 review: adoption derives `default_branch` like `clave add` does —
    /// a `trunk` repo's ordinary checkout must not render as a branch glyph
    /// only on adopted rows.
    #[test]
    fn mint_adopted_carries_the_default_branch() {
        let mut s = Store::default();
        mint_adopted(
            &mut s,
            "sess-db",
            "/home/u/proj",
            "/home/u/proj",
            "trunk",
            Some("trunk".into()),
            None,
        );
        assert_eq!(s.agents["sess-db"].default_branch.as_deref(), Some("trunk"));
    }

    // ── #232, the card's model cell ─────────────────────────────────────────

    #[test]
    fn model_from_tail_reads_the_newest_assistant_lines_nested_model() {
        let tail = concat!(
            r#"{"type":"assistant","message":{"model":"claude-opus-5","usage":{"input_tokens":5}}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"model":"claude-fable-5","usage":{"input_tokens":9}}}"#,
            "\n",
        );
        assert_eq!(model_from_tail(tail).as_deref(), Some("claude-fable-5"));
        assert_eq!(model_from_tail(r#"{"type":"user"}"#), None);
        // A malformed line scans PAST, not fails — same discipline as
        // last_tail_field.
        let dirty = format!("not-json\n{tail}");
        assert_eq!(model_from_tail(&dirty).as_deref(), Some("claude-fable-5"));

        // A synthetic line is one Claude Code wrote itself, not a model that
        // answered. It must be scanned PAST — not taken as the newest reading,
        // and not treated as "no reading" either, which would leave the card
        // holding something even staler than the real answer below it.
        let synthetic = format!(
            "{tail}{}\n",
            r#"{"type":"assistant","message":{"model":"<synthetic>"}}"#
        );
        assert_eq!(
            model_from_tail(&synthetic).as_deref(),
            Some("claude-fable-5")
        );
        // With nothing real anywhere, it is genuinely no reading, and the
        // fail-closed rule holds whatever the store already knew.
        assert_eq!(
            model_from_tail(r#"{"type":"assistant","message":{"model":"<synthetic>"}}"#),
            None
        );
    }

    #[test]
    fn short_model_derives_the_family_word() {
        // The display forms the ratified example renders: fable / opus /
        // sonnet / haiku / gpt-5.
        assert_eq!(short_model("claude-fable-5"), "fable");
        assert_eq!(short_model("claude-opus-5"), "opus");
        assert_eq!(short_model("claude-sonnet-5"), "sonnet");
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "haiku");
        assert_eq!(short_model("claude-3-5-sonnet-20241022"), "sonnet");
        // Unknown vendors pass through untouched — open strings, never enums.
        assert_eq!(short_model("gpt-5"), "gpt-5");
        assert_eq!(short_model("sol-2"), "sol-2");
    }

    #[test]
    fn a_stop_event_stamps_model_and_provider() {
        // Pattern: the s7_* tests build a store + payload and call
        // apply_hook_event with a synthetic tail. Reuse rec()/capture().
        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        let tail = r#"{"type":"assistant","message":{"model":"claude-fable-5","usage":{"input_tokens":1000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        let p = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        apply_hook_event(&mut s, "minted", "Stop", &p, Some(tail), 100, true);
        assert_eq!(s.agents["minted"].model.as_deref(), Some("fable"));
        assert_eq!(s.agents["minted"].provider.as_deref(), Some("claude"));
    }

    // ── the card's effort cell ──────────────────────────────────────────────

    #[test]
    fn effort_from_tail_reads_the_newest_assistant_lines_top_level_effort() {
        let tail = [
            r#"{"type":"assistant","effort":"high","message":{"model":"claude-opus-5"}}"#,
            r#"{"type":"user","effort":"low"}"#,
            r#"{"type":"assistant","effort":"xhigh","message":{"model":"claude-fable-5"}}"#,
            r#"{"type":"user"}"#,
        ]
        .join("\n");
        assert_eq!(effort_from_tail(&tail).as_deref(), Some("xhigh"));
        assert_eq!(
            effort_from_tail(r#"{"type":"assistant","message":{}}"#),
            None
        );
    }

    #[test]
    fn short_effort_maps_the_six_levels_to_two_letters() {
        for (raw, short) in [
            ("low", "lo"),
            ("medium", "md"),
            ("high", "hi"),
            ("xhigh", "xh"),
            ("max", "mx"),
            ("auto", "au"),
        ] {
            assert_eq!(short_effort(raw), short, "{raw}");
        }
        // An unrecognised level keeps its first two letters rather than
        // vanishing: the transcript said something, and the cell holds
        // exactly that much of it.
        assert_eq!(short_effort("adaptive"), "ad");
        assert_eq!(short_effort("x"), "x");
    }

    #[test]
    fn a_stop_event_stamps_effort() {
        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        let tail = r#"{"type":"assistant","effort":"xhigh","message":{"model":"claude-fable-5","usage":{"input_tokens":1000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        let p = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        apply_hook_event(&mut s, "minted", "Stop", &p, Some(tail), 100, true);
        assert_eq!(s.agents["minted"].effort.as_deref(), Some("xh"));
    }

    #[test]
    fn a_tail_without_an_effort_holds_the_previous_reading() {
        // Fail-closed like the model and token readings: an older Claude
        // Code that never wrote the field must not blank a real reading.
        let mut s = Store::default();
        let mut r = rec("minted");
        r.effort = Some("hi".into());
        s.agents.insert("minted".into(), r);
        let tail = r#"{"type":"assistant","message":{"model":"claude-fable-5","usage":{"input_tokens":1000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
        let p = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        apply_hook_event(&mut s, "minted", "Stop", &p, Some(tail), 100, true);
        assert_eq!(s.agents["minted"].effort.as_deref(), Some("hi"));
    }

    // ---- #245: the tail readers yield to the meter ---------------------------

    /// A Stop tail one response behind, written over a fresher statusLine
    /// reading: the bug #245 exists to remove.
    fn stale_tail() -> String {
        [
            r#"{"type":"assistant","effort":"low","message":{"model":"claude-opus-5","usage":{"input_tokens":11,"cache_creation_input_tokens":800,"cache_read_input_tokens":19000,"output_tokens":5}}}"#,
            r#"{"type":"user"}"#,
        ]
        .join("\n")
    }

    fn metered_rec(uuid: &str, at: u64) -> AgentRecord {
        let mut r = rec(uuid);
        r.context_tokens = Some(20_508);
        r.context_level = Some(1);
        r.model = Some("fable".into());
        r.provider = Some("claude".into());
        r.effort = Some("hi".into());
        r.metered_at = at;
        r
    }

    #[test]
    fn the_hook_yields_tokens_model_and_effort_while_the_meter_is_speaking() {
        let mut s = Store::default();
        s.agents
            .insert("minted".into(), metered_rec("minted", 1000));
        let ev = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        // The return is not asserted: the label path may still move on a Stop
        // tail (summary, title), and that is right — only these three yield.
        apply_hook_event(
            &mut s,
            "minted",
            "Stop",
            &ev,
            Some(&stale_tail()),
            1005,
            true,
        );
        let row = &s.agents["minted"];
        assert_eq!(row.context_tokens, Some(20_508));
        assert_eq!(row.model.as_deref(), Some("fable"));
        assert_eq!(row.effort.as_deref(), Some("hi"));
    }

    #[test]
    fn the_hook_speaks_again_once_the_meter_has_gone_quiet() {
        let mut s = Store::default();
        s.agents
            .insert("minted".into(), metered_rec("minted", 1000));
        let ev = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        let quiet = 1000 + crate::statusline::HOOK_YIELD_SECS;
        assert!(apply_hook_event(
            &mut s,
            "minted",
            "Stop",
            &ev,
            Some(&stale_tail()),
            quiet,
            true
        ));
        let row = &s.agents["minted"];
        assert_eq!(row.context_tokens, Some(19_811));
        assert_eq!(row.model.as_deref(), Some("opus"));
        assert_eq!(row.effort.as_deref(), Some("lo"));
    }

    #[test]
    fn a_rotation_resets_the_meter_so_the_hook_speaks_in_the_new_conversation() {
        let mut s = Store::default();
        s.agents
            .insert("minted".into(), metered_rec("minted", 1000));
        let ev = HookPayload {
            session_id: Some("cleared".into()),
            ..Default::default()
        };
        apply_hook_event(&mut s, "minted", "Notification", &ev, None, 1001, true);
        assert_eq!(s.agents["minted"].metered_at, 0);
        assert!(apply_hook_event(
            &mut s,
            "minted",
            "Stop",
            &ev,
            Some(&stale_tail()),
            1002,
            true
        ));
        assert_eq!(s.agents["minted"].context_tokens, Some(19_811));
    }

    /// Stop hands the floor back. Its OWN tail still yields — the reset comes
    /// after the tail decision, so the stale-by-one figure #245 exists to
    /// remove never lands — but the stamp is cleared: the meter's next
    /// reading lands regardless of the interval, and until it arrives a
    /// prompt's tail, which is post-turn and complete, may speak.
    #[test]
    fn a_stop_yields_its_own_tail_then_hands_the_floor_back() {
        let mut s = Store::default();
        s.agents
            .insert("minted".into(), metered_rec("minted", 1000));
        let ev = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        apply_hook_event(
            &mut s,
            "minted",
            "Stop",
            &ev,
            Some(&stale_tail()),
            1005,
            true,
        );
        let row = &s.agents["minted"];
        assert_eq!(row.context_tokens, Some(20_508));
        assert_eq!(row.metered_at, 0);
        apply_hook_event(
            &mut s,
            "minted",
            "UserPromptSubmit",
            &ev,
            Some(&stale_tail()),
            1006,
            true,
        );
        assert_eq!(s.agents["minted"].context_tokens, Some(19_811));
    }

    /// A model or effort change with nothing else moving still pushes: the
    /// bar renders both cells. Two `just mutants` survivors (2026-09-02)
    /// showed the `|=` at each site unpinned once the blocks became helpers.
    #[test]
    fn a_model_change_alone_pushes() {
        let mut s = Store::default();
        let mut r = rec("minted");
        r.status = Status::Done;
        r.model = Some("fable".into());
        r.provider = Some("claude".into());
        s.agents.insert("minted".into(), r);
        let ev = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        let tail = r#"{"type":"assistant","message":{"model":"claude-opus-5"}}"#;
        assert!(apply_hook_event(
            &mut s,
            "minted",
            "Stop",
            &ev,
            Some(tail),
            1,
            true
        ));
        assert_eq!(s.agents["minted"].model.as_deref(), Some("opus"));
    }

    #[test]
    fn an_effort_change_alone_pushes() {
        let mut s = Store::default();
        let mut r = rec("minted");
        r.status = Status::Done;
        r.model = Some("opus".into());
        r.provider = Some("claude".into());
        r.effort = Some("md".into());
        s.agents.insert("minted".into(), r);
        let ev = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        let tail = r#"{"type":"assistant","effort":"high","message":{"model":"claude-opus-5"}}"#;
        assert!(apply_hook_event(
            &mut s,
            "minted",
            "Stop",
            &ev,
            Some(tail),
            1,
            true
        ));
        assert_eq!(s.agents["minted"].effort.as_deref(), Some("hi"));
    }

    /// A count that moves WITHIN a level still pushes: #105 renders the raw
    /// figure as text, so gating on the glyph alone would leave it stale for
    /// up to a tenth of the zone (the #147 ruling). A `just mutants` survivor
    /// (2026-09-01) showed nothing pinned the `||` here.
    #[test]
    fn a_count_that_moves_inside_its_level_still_pushes() {
        let usage = |n: u32| {
            format!(
                r#"{{"type":"assistant","message":{{"model":"claude-opus-5","usage":{{"input_tokens":{n},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
            )
        };
        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        let ev = HookPayload {
            session_id: Some("minted".into()),
            ..Default::default()
        };
        // First tail absorbs whatever else a Stop tail moves (model, label).
        apply_hook_event(&mut s, "minted", "Stop", &ev, Some(&usage(40_000)), 1, true);
        assert_eq!(s.agents["minted"].context_level, Some(2));
        assert!(apply_hook_event(
            &mut s,
            "minted",
            "Stop",
            &ev,
            Some(&usage(41_000)),
            2,
            true
        ));
        assert_eq!(s.agents["minted"].context_level, Some(2));
        assert_eq!(s.agents["minted"].context_tokens, Some(41_000));
    }
}
