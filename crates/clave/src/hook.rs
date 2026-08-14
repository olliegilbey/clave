//! `clave hook <event>` (§6.5) — the ONLY writer of agent status. Runs as a
//! global Claude Code hook, so its prime directive is DO NO HARM: untracked
//! sessions get a lock-free read and exit 0; every internal error also exits
//! 0; it never prints a hook decision to stdout (a PreToolUse-style hook's
//! stdout can approve/deny tool use — ours stays silent and pass-through).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;
use clave_types::{AgentSnapshot, Status};
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
/// Claude, proven by [`PidGate`], belongs to that agent whatever its JSON
/// looked like, and refusing it would reintroduce the freeze for exactly the
/// events most likely to be malformed. The event NAME comes from argv, not
/// from this payload, so the status transition is still driven by something
/// trustworthy. With no `session_id` there is no transcript to trust either,
/// so the tail read is skipped and `title`/`summary` hold. (opus review, #98)
#[derive(Debug, Default, Deserialize)]
pub struct HookPayload {
    #[serde(default)]
    pub session_id: Option<String>,
    /// UserPromptSubmit carries the prompt text — the §6.4 first-label fast
    /// path (no jsonl read needed for the initial label).
    #[serde(default)]
    pub prompt: Option<String>,
    /// Notification carries a human message; §6.5 matches on its text.
    #[serde(default)]
    pub message: Option<String>,
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
    #[serde(default)]
    pub transcript_path: Option<String>,
}

/// §6.5's transition table. Latest-wins, with ONE status-aware exception
/// (revised 2026-07-08): each event maps directly to the new status (a later
/// lower-"priority" event must be able to downgrade needs_you after you
/// answer), but the CLI's idle notification consults `current` — see below.
pub fn status_for_event(event: &str, message: Option<&str>, current: Status) -> Option<Status> {
    match event {
        "UserPromptSubmit" => Some(Status::Working),
        "Stop" => Some(Status::Done),
        "StopFailure" => Some(Status::Failed),
        "SessionEnd" => Some(Status::Idle),
        "PermissionRequest" => Some(Status::NeedsYou),
        "Notification" => {
            // §4: match the notification MESSAGE TEXT. Substrings chosen from
            // the live payloads observed in S1; Task 9 C2 re-verified them
            // against CLI 2.1.201.
            let m = message.unwrap_or("");
            if m.contains("permission") {
                // Permission prompt = blocked mid-turn: red, unconditionally.
                Some(Status::NeedsYou)
            } else if m.contains("waiting for your input") {
                // The CLI fires this ~60s after EVERY turn. It means
                // "blocked mid-turn" only while still working (an in-turn
                // question/approval — no Stop yet). For a finished agent it
                // is just an idle nag: swallowing it keeps red meaningful
                // (green-until-read → grey already covers "turn over").
                (current == Status::Working).then_some(Status::NeedsYou)
            } else {
                None
            }
        }
        // Unknown/other events: strictly no-op. The hook is registered for a
        // fixed set, but a defensive default keeps novel events harmless.
        _ => None,
    }
}

/// Harness-injected prefixes that must NEVER be treated as user intent for
/// labelling (issue #17, observed live 2026-07-21): resuming a session that
/// died with a pending background task makes Claude Code auto-fire a turn
/// whose "prompt" is an orphaned `<task-notification>` tag, not anything the
/// user typed. Because earned labels stick (§6.4: no self-heal), letting one
/// through once bakes it permanently. This list is expected to grow as more
/// injected shapes surface — keep it one named const so every check-site
/// stays in sync. Note: issue #24's rename tier (extracting a `customTitle`
/// for user-driven renames) sits ABOVE this guard and is separate — this
/// const only prevents MIS-earning, it adds no rename affordance.
const HARNESS_INJECTED_PREFIXES: &[&str] = &[
    "<task-notification",
    "<system-reminder",
    "<local-command-caveat",
    "<command-name",
];

/// True if `text`'s trimmed start matches a harness-injected tag (#17). We
/// check the START, not a substring search, and we never try to strip the
/// tag and use what follows — an injected body is harness text end to end,
/// not user intent with a prefix to peel off.
fn is_harness_injected(text: &str) -> bool {
    let t = text.trim_start();
    HARNESS_INJECTED_PREFIXES.iter().any(|p| t.starts_with(p))
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

/// Last ≤`max_bytes` of `path` (lossy UTF-8; we only pattern-match). The
/// jsonl grows unbounded — a full read every turn risks the hook timeout
/// budget, so we read the tail only (§6.4).
pub fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
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

/// Fire-and-forget snapshot push (§5). Spawn WITHOUT waiting: `zellij pipe`
/// can dawdle (S1) and a global hook must never block Claude on it. The
/// child inherits ZELLIJ env vars from the pane, targeting the right session;
/// stdio is nulled so nothing leaks into the hook protocol on stdout.
pub fn push_snapshot(snap: &AgentSnapshot) {
    let Ok(payload) = serde_json::to_string(snap) else {
        return;
    };
    // Discovered path (codex P2 on PR #29): hooks run as claude's children,
    // whose env may lack the interactive PATH — an off-PATH zellij made every
    // status push a silent no-op. Fire-and-forget stays: failure here must
    // never become a hook failure (§6.5 zero-risk citizen).
    let _ = Command::new(crate::discover::tool_path(crate::discover::ToolId::Zellij))
        .args(["pipe", "--name", "clave-status", "--", &payload])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// The one store mutation a hook event performs, factored out of run_hook's
/// lock closure so it unit-tests against a plain `Store`. Returns whether
/// anything changed; bumps `seq` itself (exactly once) when it did.
///
/// `own_claude` is [`PidGate::is_the_agents_own_claude`], and it gates ONE
/// field — `live_session`. Everything else here is driven by an event
/// `resolve_row` already admitted; that pointer is the only value whose
/// lifetime outlives the event, so it is the only one that can be poisoned by a
/// Claude that merely LOOKS like this row's (see the write site below).
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
    if let Some(next) = status_for_event(event, payload.message.as_deref(), rec.status) {
        changed |= rec.status != next;
        rec.status = next;
    }
    // Remember which conversation this row is actually living in (#99). The
    // payload's id is the live one BY DEFINITION — Claude is reporting its own
    // session. Held as `None` when it equals `uuid`, so the field can go BACK
    // to agreeing rather than keep pointing at a superseded conversation.
    //
    // GATED ON THE PID, unlike everything else here, and the asymmetry is the
    // point. `resolve_row` admits a payload whose `session_id` NAMES A ROW
    // without consulting the gate — correctly, that is the ordinary path — but
    // the minted transcript this bug leaves orphaned is listed in `claude
    // --resume`'s own picker, so a Claude started by hand OUTSIDE clave can
    // fire hooks carrying exactly that id. Ungated, its `session_id == uuid`
    // would read as "the two agree again" and WIPE a live pointer that is still
    // true, silently re-arming #99 for the next release. Every other field here
    // describes the event and is corrected by the next one; this pointer is
    // read once, much later, by a process with nothing else to go on.
    //
    // Fails closed: no `CLAUDE_PID`, or a mismatch, and the pointer simply
    // holds. Worst case is the pre-#99 target, never a wrong one.
    //
    // Not part of `changed`, on purpose. `with_store_mut` persists the record
    // either way; `changed` gates only the SNAPSHOT PUSH, and the bar renders
    // nothing from this field — bumping `seq` for it would push a pipe message
    // that changes no pixel.
    //
    // S7 (#62) rides this same signal, and ORDER MATTERS — it compares against
    // `rec.live_session` before the assignment below overwrites it.
    //
    // A rotation IS a `/clear`: Claude mints a new id AND starts a new
    // transcript (FOOTGUNS, "ROTATES its session id on `/clear`"). The new
    // conversation genuinely holds nothing, so the battery returns to full on
    // this event rather than waiting for a usage line to exist. That is the
    // design statement in code: **the battery measures the conversation the row
    // is IN, never the row's history**, so a near-zero reading straight after a
    // `/clear` is CORRECT and is not to be "fixed".
    //
    // `--resume` does NOT rotate (measured, same FOOTGUNS entry), so resuming a
    // conversation correctly keeps its reading.
    let tokens_before = rec.context_tokens;
    if let Some(live) = payload.session_id.as_deref().filter(|_| own_claude) {
        if live != rec.live_session.as_deref().unwrap_or(uuid) {
            rec.context_tokens = Some(0);
        }
        rec.live_session = (live != uuid).then(|| live.to_string());
    }
    // A tail reaches us only on Stop / UserPromptSubmit (`run_hook`'s event
    // gate). No tail, or a tail carrying no usage line, HOLDS the previous
    // reading — §5.4 fail-closed. Never invent a measurement.
    if let Some(tokens) = jsonl_tail.and_then(tokens_from_tail) {
        rec.context_tokens = Some(tokens);
    }
    // Bucketed HERE, in the row's own agent's process, for two reasons: this is
    // where `SMART_ZONE_ENV` means the right thing, and stamping it makes a
    // dormant row free forever after — `snapshot_from` only copies. The fleet's
    // dormant list may eventually hold every conversation the user has ever had.
    let level = rec.context_tokens.map(|t| battery_level(t, smart_zone()));
    // BOTH fields gate the push, not just the level. The glyph only moves once
    // per tenth of the zone, but #105 renders the raw count as text — gating on
    // the level alone would leave that text stale for up to a tenth of the zone
    // (15k tokens at the default), which is a bug shipped early rather than a
    // bug avoided. (CodeRabbit and the spec review agreed here, #147.)
    changed |= rec.context_level != level || rec.context_tokens != tokens_before;
    rec.context_level = level;
    // §6.6 / S1 / #39: a PROMPT is the ONLY event that reorders. Stop,
    // StopFailure, Notification, PermissionRequest and SessionEnd change the
    // STATUS and nothing else — "claude finishing should not move it up"
    // (maintainer ruling, 2026-07-22).
    let commitment = event == "UserPromptSubmit";
    let mut commit_tab = None;
    if commitment {
        rec.last_interacted = now; // wall clock: `clave ls`, picker, eager_row
        // §6.6 Design B: a prompt is a user COMMITMENT to the agent's TAB —
        // stamp through the bind, atomically with the bump (a bar-side stamp
        // would race the user switching away).
        commit_tab = rec.tab_id;
        changed = true;
    }
    changed |= refresh_label(rec, event, payload, jsonl_tail);
    if !changed {
        return false;
    }
    // The write's own seq IS the commitment ordinal — the §5 pipe contract and
    // the §6.6 row order share one counter (see Store::mint_ord).
    let ord = s.mint_ord();
    if commitment {
        if let Some(tab_id) = commit_tab {
            s.tab_order.insert(tab_id, ord);
        }
        if let Some(rec) = s.agents.get_mut(uuid) {
            // Set on the AGENT too, not only the tab: an unbound agent (RC-B,
            // or a prompt landing before `clave bind`) still records its
            // commitment, so the dormant row it becomes on close sorts right
            // even if the prune never lands.
            rec.commit_ord = ord;
        }
    }
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
///
/// **Store membership is NOT sufficient to accept it**, and an earlier version
/// of this function got that wrong. The env var is inherited by every
/// descendant of the agent's Claude, so a nested `claude` — an agent shelling
/// one out, `clave dev`'s own `claude -p` — carries it too, and its session id
/// is likewise unknown to the store. Membership proves the value names *a*
/// row, never *this* row: the nested session's Stop, Notification and
/// UserPromptSubmit would all have driven the parent agent's status, ordering
/// and prose. Caught in review, three independent lanes.
///
/// So the fallback is gated on [`PidGate`]: it is taken only when the Claude
/// that fired this hook IS the Claude clave exec'd. `CLAUDE_PID` is set by
/// Claude Code to its own pid — verified empirically, a nested `claude`
/// reported its own pid and not its parent's — and `exec` preserves the pid,
/// so `clave spawn`'s `process::id()` IS the agent Claude's pid.
///
/// The routes that matter fail CLOSED: a missing value, a STALE one (a dead
/// pid cannot match a live `CLAUDE_PID`), or a pid mismatch all resolve to
/// `None` and the hook declines exactly as it did before any of this existed.
/// The worst case is the old freeze, never an ACCIDENTAL write to the wrong
/// agent — which is the threat, since the mechanism exists because environment
/// is inherited without anyone intending it.
///
/// It is NOT a defence against a deliberate local caller: exporting a real
/// row's uuid with matching `CLAVE_AGENT_PID` and `CLAUDE_PID` passes. That is
/// same-user, same-machine, and anyone who can set that environment can write
/// the store directly, so there is nothing to defend. Stated because an earlier
/// draft claimed "hand-set" values fail closed, and they do not.
///
/// Pure and total: map lookups and integer comparison, no I/O, so it is safe
/// on the §6.5 fast path and testable without a store on disk.
pub fn resolve_row(
    store: &Store,
    session: Option<&str>,
    env_uuid: Option<&str>,
    gate: PidGate,
) -> Option<String> {
    if let Some(s) = session.filter(|s| store.agents.contains_key(*s)) {
        return Some(s.to_string());
    }
    if !gate.is_the_agents_own_claude() {
        return None;
    }
    env_uuid
        .filter(|e| store.agents.contains_key(*e))
        .map(str::to_string)
}

/// "Is the Claude that fired this hook the one clave exec'd?" — the guard that
/// keeps [`resolve_row`]'s env fallback from being ambient authority.
///
/// Both halves are read from the environment by the caller so this stays pure.
/// `agent` is clave's own `CLAVE_AGENT_PID`; `firing` is Claude's `CLAUDE_PID`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PidGate {
    pub agent: Option<u32>,
    pub firing: Option<u32>,
}

impl PidGate {
    /// Read both sides from the ambient environment.
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok()?.parse::<u32>().ok();
        Self {
            agent: get(clave_types::AGENT_PID_ENV),
            firing: get(CLAUDE_PID_ENV),
        }
    }

    /// True only when both are present AND equal. `None` on either side is a
    /// refusal, not a pass: an absent `CLAUDE_PID` means we cannot tell which
    /// Claude fired, and guessing is the whole bug this exists to prevent.
    pub fn is_the_agents_own_claude(self) -> bool {
        matches!((self.agent, self.firing), (Some(a), Some(f)) if a == f)
    }
}

/// Claude Code's own pid, exported into every process it spawns — including
/// hooks. NOT clave's to set, which is why it lives here rather than in
/// `clave-types` beside the two clave owns: it is an OBSERVED property of an
/// external tool (verified 2026-07-31; a nested `claude` reported its own pid,
/// not its parent's), and if Claude ever stops setting it, [`PidGate`] fails
/// closed and the rotation fix degrades to the pre-fix freeze.
const CLAUDE_PID_ENV: &str = "CLAUDE_PID";

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
///   the minted uuid. What still holds is confinement plus [`PidGate`], so the
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

/// The whole hook flow. Errors bubble up ONLY so main can log them to
/// stderr — main exits 0 no matter what (Global Constraint).
pub fn run_hook(event: &str, stdin_json: &str) -> Result<()> {
    let payload: HookPayload = serde_json::from_str(stdin_json).unwrap_or_default();
    let session = payload.session_id.clone();
    let paths = store_paths()?;
    // FAST PATH (§6.5): lock-free read; untracked session → exit immediately.
    // clave must never serialize unrelated sessions' hooks behind its lock.
    // `resolve_row` keeps that property — map lookups and an integer compare,
    // no I/O. The admitted set does GROW, by exactly one case and deliberately:
    // the agent's own Claude firing with a rotated id, which used to return
    // early and is the whole point of #97. What the pid gate preserves is the
    // invariant §6.5 actually cares about — no UNRELATED session ever reaches
    // this lock. (An earlier draft of this comment claimed the set was
    // unchanged; that was an overclaim, and this file treats those as defects.)
    let env_uuid = std::env::var(clave_types::AGENT_UUID_ENV).ok();
    let gate = PidGate::from_env();
    let store = read_store(&paths)?;
    let Some(uuid) = resolve_row(&store, session.as_deref(), env_uuid.as_deref(), gate) else {
        // Observability for the ONE silent drop that is hard to reason about
        // from outside: the env named a real row and the gate refused it. The
        // condition is deliberately narrow — an unrelated session carries no
        // `CLAVE_AGENT_UUID`, so this cannot become per-event noise for the
        // whole machine, which §6.5 forbids. #97 was a silent freeze; a
        // mechanism that can silently decline should say so. (CodeRabbit, #98)
        if let Some(e) = env_uuid
            .as_deref()
            .filter(|e| store.agents.contains_key(*e))
        {
            crate::evlog::log_event(
                "hook",
                &format!(
                    "{event}: declined {e} — firing claude {:?} is not the agent's {:?}",
                    gate.firing, gate.agent
                ),
            );
        }
        return Ok(());
    };
    // §6.9: claude_config_dir() (not raw home) so the sandbox override
    // reaches the same jsonl tree real claude processes write to.
    let claude_dir = crate::env::claude_config_dir().unwrap_or_default();
    let snap = with_store_mut(&paths, |s| {
        // The tail read is gated on the EVENT only — no longer on
        // `label_source`. `title` and `summary` roll for the whole life of a
        // row (design-lock §7.1), so gating the read on "the label has not
        // been earned yet" would freeze the bar's two live columns the moment
        // the label froze — the regression this exists to remove. Cost is one
        // 64 KiB tail read on the two label-bearing events, well inside the
        // §6.5 hook budget; the other events still read nothing.
        let tail = s.agents.get(&uuid).and_then(|rec| {
            // Event gate FIRST. `resolve_transcript` does up to two
            // `canonicalize` calls, and this closure runs while
            // `with_store_mut` holds the exclusive flock — computing a path
            // only to discard it would put filesystem syscalls under the lock
            // on every `PreToolUse`, i.e. on every tool call of every tracked
            // agent. The comment above promises the other events read nothing;
            // this is what makes that true. (CodeRabbit, #98 — the previous
            // ordering was a regression against the code this replaced.)
            if !matches!(event, "Stop" | "UserPromptSubmit") {
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
            .and_then(|path| read_tail(&path, 64 * 1024))
        });
        apply_hook_event(
            s,
            &uuid,
            event,
            &payload,
            tail.as_deref(),
            now_unix(),
            gate.is_the_agents_own_claude(),
        )
        .then(|| snapshot_from(s))
    })?;
    if let Some(snap) = snap {
        push_snapshot(&snap);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
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
    fn a_rotated_session_id_resolves_only_for_the_agents_own_claude() {
        let mut store = Store::default();
        store.agents.insert("minted".into(), rec("minted"));
        let same = PidGate {
            agent: Some(42),
            firing: Some(42),
        };
        let nested = PidGate {
            agent: Some(42),
            firing: Some(99),
        };

        // The ordinary case is UNCHANGED and needs no gate at all: a payload
        // id that names a row wins outright, whatever the pids say.
        for g in [same, nested, PidGate::default()] {
            assert_eq!(
                resolve_row(&store, Some("minted"), None, g).as_deref(),
                Some("minted")
            );
        }
        assert_eq!(
            resolve_row(&store, Some("minted"), Some("other"), same).as_deref(),
            Some("minted"),
            "a valid payload id must not be overridden by the environment"
        );

        // The rotation: the new id names nothing, the env carries the minted
        // key, and the firing Claude IS the agent's — so the row is found.
        assert_eq!(
            resolve_row(&store, Some("rotated"), Some("minted"), same).as_deref(),
            Some("minted")
        );

        // THE REVIEW FINDING. A nested `claude` inherits the env and its own
        // session id is equally unknown, so store membership alone would have
        // handed it this row. The pid gate is the only thing refusing it.
        assert_eq!(
            resolve_row(&store, Some("nested-session"), Some("minted"), nested),
            None,
            "a nested claude must never resolve to the agent's row"
        );

        // Every unknown fails CLOSED, including a half-populated gate — an
        // absent CLAUDE_PID means we cannot tell which Claude fired.
        for g in [
            PidGate::default(),
            PidGate {
                agent: Some(42),
                firing: None,
            },
            PidGate {
                agent: None,
                firing: Some(42),
            },
        ] {
            assert_eq!(
                resolve_row(&store, Some("rotated"), Some("minted"), g),
                None
            );
        }
        assert_eq!(
            resolve_row(&store, Some("rotated"), Some("bogus"), same),
            None
        );
        assert_eq!(resolve_row(&store, None, None, same), None);

        // A MALFORMED payload (serde yields `session_id: None`) resolves via
        // the env when the gate passes. This is a deliberate behaviour change
        // — the old code returned early on a missing session id — and it was
        // untested until the opus review pointed out that a mutation
        // restoring the `session.is_some()` requirement would have survived.
        assert_eq!(
            resolve_row(&store, None, Some("minted"), same).as_deref(),
            Some("minted"),
            "the agent's own Claude is still its own Claude with unparseable JSON"
        );
        // …but never for a nested one, which is the whole point of the gate.
        assert_eq!(resolve_row(&store, None, Some("minted"), nested), None);
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
            prompt: None,
            message: None,
            transcript_path: None,
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
            prompt: None,
            message: Some("needs your permission".into()),
            transcript_path: None,
        };
        for (event, payload, expected) in [
            ("Stop", &p, Status::Done),
            ("StopFailure", &p, Status::Failed),
            ("PermissionRequest", &p, Status::NeedsYou),
            ("Notification", &perm, Status::NeedsYou),
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
            prompt: None,
            message: None,
            transcript_path: None,
        };
        let pb = HookPayload {
            session_id: Some("u-b".into()),
            prompt: None,
            message: None,
            transcript_path: None,
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
            prompt: None,
            message: None,
            transcript_path: None,
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
    fn state_machine_is_latest_wins() {
        // Spec §6.5 transition table, verbatim.
        assert_eq!(
            status_for_event("UserPromptSubmit", None, Status::Idle),
            Some(Status::Working)
        );
        assert_eq!(
            status_for_event("Stop", None, Status::Working),
            Some(Status::Done)
        );
        assert_eq!(
            status_for_event("StopFailure", None, Status::Working),
            Some(Status::Failed)
        );
        assert_eq!(
            status_for_event("SessionEnd", None, Status::Done),
            Some(Status::Idle)
        );
        assert_eq!(
            status_for_event("PermissionRequest", None, Status::Working),
            Some(Status::NeedsYou)
        );
        // Notification matches on MESSAGE TEXT (§4). Permission prompts are a
        // mid-turn block → red regardless of current status.
        assert_eq!(
            status_for_event(
                "Notification",
                Some("Claude needs your permission to use Bash"),
                Status::Done
            ),
            Some(Status::NeedsYou)
        );
        // §6.5 revised 2026-07-08: the CLI's ~60s idle nag fires after EVERY
        // turn. It means "blocked mid-turn" ONLY while still working (an
        // in-turn question/approval — no Stop yet). A finished agent stays
        // done/idle: green-until-read already tells the user everything.
        assert_eq!(
            status_for_event(
                "Notification",
                Some("Claude is waiting for your input"),
                Status::Working
            ),
            Some(Status::NeedsYou)
        );
        assert_eq!(
            status_for_event(
                "Notification",
                Some("Claude is waiting for your input"),
                Status::Done
            ),
            None
        );
        assert_eq!(
            status_for_event(
                "Notification",
                Some("Claude is waiting for your input"),
                Status::Idle
            ),
            None
        );
        // Other notifications don't touch status.
        assert_eq!(
            status_for_event("Notification", Some("compacting…"), Status::Working),
            None
        );
        // Unknown events are a no-op — the global hook must never guess.
        assert_eq!(status_for_event("PreToolUse", None, Status::Idle), None);
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
            message: None,
            transcript_path: None,
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
            message: None,
            transcript_path: None,
        };
        assert!(!refresh_label(&mut r, "UserPromptSubmit", &injected, None));
        assert_eq!(r.label, "x · main"); // unchanged — still the bare prefix
        assert_eq!(r.label_source, LabelSource::FirstPrompt); // still eligible

        // The next REAL prompt earns the label exactly as before the guard.
        let real = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("fix the flaky auth test".into()),
            message: None,
            transcript_path: None,
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
        for prefix in HARNESS_INJECTED_PREFIXES {
            let injected_text = format!("  \n\t{prefix}> payload we must never earn");
            // First-prompt path: the upgrade is skipped outright.
            let mut r = rec("u1");
            let p = HookPayload {
                session_id: Some("u1".into()),
                prompt: Some(injected_text.clone()),
                message: None,
                transcript_path: None,
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
        for prefix in HARNESS_INJECTED_PREFIXES {
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
            message: None,
            transcript_path: None,
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
            message: None,
            transcript_path: None,
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
            message: None,
            transcript_path: None,
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
            message: None,
            transcript_path: None,
        };
        assert!(refresh_label(&mut r, "UserPromptSubmit", &p, None));
        assert!(
            !r.label.contains('\\') && !r.label.contains('"'),
            "unsanitized label: {}",
            r.label
        );
    }
}
