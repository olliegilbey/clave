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
/// novel payload must degrade to a no-op, never an error.
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

/// Claude's rolling auto-description — `{"type":"ai-title","aiTitle":…}`.
/// THE source for `rec.summary` (#79): it is what Claude Code writes where it
/// once wrote `type:"summary"`, present in 74 of 153 local transcripts as of
/// 2026-07-29 while `type:"summary"` appears in 0 of them.
pub fn ai_title_from_tail(tail: &str) -> Option<String> {
    last_tail_field(tail, "ai-title", "aiTitle")
}

/// The user's own session rename — `{"type":"custom-title","customTitle":…}`,
/// re-appended latest-wins. The source for `rec.title` (design-lock §5/§7.1:
/// the filled chip). Verified against a live sandbox transcript 2026-07-29:
/// the line carries exactly `type`, `customTitle`, `sessionId`.
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
/// - `summary` ← `ai-title` (Claude's auto-description), falling back to the
///   extinct `type:"summary"` line, and finally — only while `summary` is
///   still EMPTY — to the current prompt, so a live row is never blank before
///   Claude has written its first `ai-title`. Fill-only-when-empty on that
///   last tier: an earned `ai-title` must never regress to prompt text.
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
        .and_then(|t| ai_title_from_tail(t).or_else(|| summary_from_tail(t)))
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
pub fn apply_hook_event(
    s: &mut crate::store::Store,
    uuid: &str,
    event: &str,
    payload: &HookPayload,
    jsonl_tail: Option<&str>,
    now: u64,
) -> bool {
    let Some(rec) = s.agents.get_mut(uuid) else {
        return false; // raced a prune — fine
    };
    let mut changed = false;
    if let Some(next) = status_for_event(event, payload.message.as_deref(), rec.status) {
        changed |= rec.status != next;
        rec.status = next;
    }
    let mut stamp = None;
    if event == "UserPromptSubmit" {
        rec.last_interacted = now; // recency (§6.6 order)
        // §6.6 Design B: a prompt is a user COMMITMENT to the agent's TAB —
        // stamp the store timeline through the bind, atomically with the
        // bump (a bar-side stamp would race the user switching away).
        stamp = rec.tab_id;
        changed = true;
    }
    changed |= refresh_label(rec, event, payload, jsonl_tail);
    if let Some(tab_id) = stamp {
        let e = s.tab_timeline.entry(tab_id).or_insert(0);
        *e = (*e).max(now);
    }
    if changed {
        s.seq += 1; // monotonic pipe contract (§5)
    }
    changed
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
/// Every route fails CLOSED: a missing, stale or hand-set value, or a pid
/// mismatch, resolves to `None` and the hook declines exactly as it did before
/// any of this existed. The worst case is the old freeze, never a write
/// attributed to the wrong agent.
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
/// (the file moves) and a rotated session id (a `/clear` or resume starts a
/// new file, #97). Deriving from `rec.cwd` + `uuid` misses both.
///
/// `transcript_path` is untrusted input whose CONTENTS get written into the
/// store, so it is canonicalized and then confined:
///
/// - it must resolve inside `<claude_config_dir>/projects` — canonicalized on
///   both sides, so `..` traversal and symlinks out are refused after
///   resolution rather than by string inspection;
/// - its filename must be `<session_id>.jsonl`, i.e. the transcript must be the
///   one belonging to the session that sent the event. This is why rotation
///   needs no stored field: the payload names its own current file.
///
/// A path failing any check falls back to the derived one rather than erroring
/// — a hook that fails hard blocks the agent (§6.5 do-no-harm), and the derived
/// path is exactly today's behaviour.
pub fn resolve_transcript(
    claude_dir: &Path,
    payload_path: Option<&str>,
    session: Option<&str>,
    rec_cwd: &str,
    uuid: &str,
) -> std::path::PathBuf {
    let derived = || jsonl_path(claude_dir, rec_cwd, uuid);
    let (Some(raw), Some(session)) = (payload_path, session) else {
        return derived();
    };
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
            p
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
    // no I/O — and its pid gate keeps the ADMITTED SET unchanged: only the
    // agent's own Claude can reach the lock, exactly as before.
    let env_uuid = std::env::var(clave_types::AGENT_UUID_ENV).ok();
    let Some(uuid) = resolve_row(
        &read_store(&paths)?,
        session.as_deref(),
        env_uuid.as_deref(),
        PidGate::from_env(),
    ) else {
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
            // The payload names its own CURRENT transcript, so this is right
            // through both a relocation and an id rotation without the store
            // having to remember anything (#87 dissolves #97's read half).
            let path = resolve_transcript(
                &claude_dir,
                payload.transcript_path.as_deref(),
                session.as_deref(),
                &rec.cwd,
                &uuid,
            );
            matches!(event, "Stop" | "UserPromptSubmit")
                .then(|| read_tail(&path, 64 * 1024))
                .flatten()
        });
        apply_hook_event(s, &uuid, event, &payload, tail.as_deref(), now_unix())
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
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
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
            std::fs::canonicalize(&rotated).unwrap()
        );

        // Absent payload → today's derived path, unchanged.
        assert_eq!(go(None, Some("rotated")), derived);

        // CONFINEMENT. Outside the projects root, and a path whose filename
        // does not belong to the sending session, both fall back rather than
        // erroring — a hook that fails hard blocks the agent (§6.5).
        assert_eq!(go(outside.to_str(), Some("rotated")), derived);
        assert_eq!(
            go(rotated.to_str(), Some("someone-else")),
            derived,
            "the transcript must belong to the session that sent the event"
        );
        // Traversal is refused AFTER canonicalization, not by string match.
        let traversal = proj.join("..").join("..").join("outside.jsonl");
        assert_eq!(go(traversal.to_str(), Some("outside")), derived);
        assert_eq!(
            go(Some("/nonexistent/rotated.jsonl"), Some("rotated")),
            derived
        );
    }

    #[test]
    fn prompt_stamps_bound_tabs_timeline_atomically() {
        // §6.6 Design B: a prompt is a USER COMMITMENT to the agent's TAB.
        // The hook stamps tab_timeline[bind] in the SAME locked write as the
        // last_interacted bump — no bar round-trip, no switch-away race.
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
            1700
        ));
        assert_eq!(s.agents["u1"].last_interacted, 1700);
        assert_eq!(s.tab_timeline.get(&4), Some(&1700));
        assert_eq!(s.seq, 1); // one bump for the whole atomic change
        // Unbound agent: interaction still recorded, no stamp to place.
        assert!(apply_hook_event(
            &mut s,
            "u2",
            "UserPromptSubmit",
            &p,
            None,
            1800
        ));
        assert_eq!(s.agents["u2"].last_interacted, 1800);
        assert_eq!(s.tab_timeline.len(), 1);
        // Non-commitment events don't stamp the timeline (Stop ≠ user input).
        assert!(apply_hook_event(&mut s, "u1", "Stop", &p, None, 1900));
        assert_eq!(s.tab_timeline.get(&4), Some(&1700));
        // Unknown uuid / no-op event: unchanged, no seq bump.
        let seq = s.seq;
        assert!(!apply_hook_event(&mut s, "ghost", "Stop", &p, None, 2000));
        assert!(!apply_hook_event(
            &mut s,
            "u1",
            "PreToolUse",
            &p,
            None,
            2000
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
                "{}{}",
                ai_title_line(&injected),
                custom_title_line(&injected)
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
