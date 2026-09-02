//! The statusLine feed (#245): Claude Code's statusLine JSON is the primary
//! source for the card's tokens, model and effort; the transcript tail the
//! hook reads stays as the fallback.
//!
//! Why a second channel: the Stop hook fires BEFORE the final assistant line
//! reaches the jsonl, so its tail read is one API response behind — every
//! idle row in the live store held an older figure than its own transcript
//! (12 of 12, measured 2026-09-01). The statusLine carries
//! `context_window.total_input_tokens` "from the most recent API response",
//! after every assistant message, pre-summed. That is the fix.
//!
//! The channel is a MEASUREMENT of the live conversation, not a history
//! source: fresh-install population and backfill still derive from the
//! transcripts (CLAUDE.md, the jsonl store is the source of truth).
//!
//! Two rules keep the two sources from fighting:
//!
//! - **The hook yields.** Once the meter has spoken for a row, the hook's
//!   tail readers hold their three cells for [`HOOK_YIELD_SECS`]; without
//!   that, every turn ends with Stop overwriting a fresh figure with a stale
//!   one. Time-bounded rather than for-the-conversation so a meter that goes
//!   quiet (a settings edit, a schema change) degrades back to today's
//!   behaviour instead of freezing the row. A rotation (`/clear`) resets the
//!   stamp with the battery: the new conversation has not been metered. A
//!   Stop resets it too, after its own tail has yielded: the turn is over,
//!   so the meter's next reading lands regardless of its interval and a
//!   prompt's tail, post-turn and complete, may speak until it arrives.
//! - **The meter paces itself.** The statusLine runs on every assistant
//!   message (6 runs, 4 distinct counts in one 15-second sandbox turn), and
//!   every applied reading is a store rewrite under the flock plus a pipe
//!   push that re-renders every bar. A moved count lands only once per
//!   [`APPLY_INTERVAL_SECS`], unless the level moved (a glyph) or the model or
//!   effort moved (a cell), which land at once — and bring the held count
//!   with them, since the row is being written anyway.
//!
//! Never invent: a `0` total (session start) or a `null` `current_usage`
//! (documented for the window after `/compact`) is not a token reading, and
//! the row HOLDS what it had. Model and effort in the same payload are still
//! true and still land.

use std::io::Write;
use std::process::{Child, Command, Stdio};

use anyhow::Result;
use clave_types::AgentSnapshot;
use serde::Deserialize;

use crate::hook::{
    battery_level, note_live_session, push_snapshot, resolve_row, restamp_level, short_effort,
    short_model, smart_zone,
};
use crate::store::{
    AgentRecord, Store, StorePaths, now_unix, read_store, snapshot_from, store_paths,
    with_store_mut,
};

/// The subset of Claude Code's statusLine JSON clave reads. Everything else
/// on the wire (cost, duration, lines changed, rate limits, vim mode) is
/// ignored by construction — nothing on the card asks for it.
#[derive(Debug, Default, Deserialize)]
pub struct StatuslinePayload {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub model: Option<ModelField>,
    #[serde(default)]
    pub effort: Option<EffortField>,
    #[serde(default)]
    pub context_window: Option<ContextWindow>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ModelField {
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EffortField {
    #[serde(default)]
    pub level: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ContextWindow {
    #[serde(default)]
    pub total_input_tokens: Option<u64>,
    /// `null` before the first API call and again after `/compact` until the
    /// next one (statusLine docs). Its PRESENCE is what makes the total a
    /// reading; its fields are read only by the test pinning the sum identity.
    #[serde(default)]
    pub current_usage: Option<Usage>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

/// One measurement off the meter, in the raw forms Claude Code sends
/// (`claude-fable-5-1`, `medium`); `apply_statusline` shortens them the way
/// the hook does. `tokens` is `None` whenever the payload did not carry a
/// real count — see the module doc's never-invent rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reading {
    pub session_id: Option<String>,
    pub tokens: Option<u32>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl Reading {
    /// Unparseable stdin is an empty reading, never an error: like the hook,
    /// the statusLine command is a zero-risk citizen of Claude's process.
    pub fn parse(json: &str) -> Reading {
        let p: StatuslinePayload = serde_json::from_str(json).unwrap_or_default();
        let tokens = p.context_window.as_ref().and_then(|cw| {
            cw.current_usage.as_ref()?;
            let total = cw.total_input_tokens.filter(|&t| t > 0)?;
            Some(u32::try_from(total).unwrap_or(u32::MAX))
        });
        Reading {
            session_id: p.session_id,
            tokens,
            model: p.model.and_then(|m| m.id),
            effort: p.effort.and_then(|e| e.level),
        }
    }
}

/// How often a moved count is allowed to land on its own. Chosen against the
/// measured turn (readings 0, 24, 26, 27, 29 and 38 seconds in): three of six
/// land instead of four, and a long turn repaints the count every ten seconds
/// rather than on every tool call. Tune against a live drive, not by feel.
pub const APPLY_INTERVAL_SECS: u64 = 10;

/// How long after the meter's last token reading the hook's tail readers hold
/// their three cells. Long enough that a Stop after a multi-minute final tool
/// call still yields; short enough that a meter gone quiet hands back to the
/// hook within a coffee break.
pub const HOOK_YIELD_SECS: u64 = 600;

/// The hook's side of the bargain: while the meter has spoken recently the
/// hook's tail readers keep quiet on tokens, model and effort. `0` is "not
/// metered since the last rotation or Stop" — the resets those two perform.
/// A clock stepped backwards reads as recent, never as an underflow.
pub fn hook_yields(rec: &AgentRecord, now: u64) -> bool {
    rec.metered_at != 0 && now.saturating_sub(rec.metered_at) < HOOK_YIELD_SECS
}

/// Whether a moved count is owed a write on its own merits (the module doc's
/// pacing rule). A row with no count yet, or none since its rotation, always
/// takes the first one.
fn count_due(rec: &AgentRecord, tokens: u32, now: u64, zone: u32) -> bool {
    let Some(have) = rec.context_tokens else {
        return true;
    };
    tokens != have
        && (rec.metered_at == 0
            || rec.context_level != Some(battery_level(tokens, zone))
            || now.saturating_sub(rec.metered_at) >= APPLY_INTERVAL_SECS)
}

/// The one store mutation a statusLine run performs — the meter's twin of
/// `hook::apply_hook_event`, pure over a `Store` for the same reason. Returns
/// whether anything the bar renders moved; bumps `seq` exactly once when it
/// did. `run_statusline` also uses that answer as its fast path: applied to a
/// lock-free copy first, a `false` means the lock is never taken.
///
/// `own_claude` gates only the live-session pointer, exactly as in the hook
/// (see the write site there for why): a reading admitted by id from a Claude
/// that merely looks like this row's still lands, but never moves the pointer.
pub fn apply_statusline(
    s: &mut Store,
    uuid: &str,
    reading: &Reading,
    now: u64,
    own_claude: bool,
    zone: u32,
) -> bool {
    let Some(rec) = s.agents.get_mut(uuid) else {
        return false; // raced a prune — fine
    };
    let tokens_before = rec.context_tokens;
    note_live_session(rec, uuid, reading.session_id.as_deref(), own_claude);
    let mut changed = false;
    if let Some(short) = reading.model.as_deref().map(short_model) {
        changed |= rec.model.as_deref() != Some(short.as_str());
        rec.model = Some(short);
        if rec.provider.as_deref() != Some("claude") {
            rec.provider = Some("claude".to_string());
            changed = true;
        }
    }
    if let Some(short) = reading.effort.as_deref().map(short_effort) {
        changed |= rec.effort.as_deref() != Some(short.as_str());
        rec.effort = Some(short);
    }
    if let Some(tokens) = reading
        .tokens
        .filter(|&t| changed || count_due(rec, t, now, zone))
    {
        rec.context_tokens = Some(tokens);
        rec.metered_at = now;
    }
    let level_moved = restamp_level(rec, zone);
    changed |= level_moved || rec.context_tokens != tokens_before;
    if changed {
        s.mint_ord();
    }
    changed
}

/// Start the wrapped command with the payload on its stdin — the SAME bytes
/// Claude Code sent, so whatever the user's script parsed before it parses
/// now. Runs through `sh -c` because `statusLine.command` is a shell string
/// (docs: "the command field runs in a shell"), and `setup` hands it over as
/// one quoted argument so pipes and quotes inside it survive the outer shell.
///
/// Spawned BEFORE the store work, so the user's line never waits on clave's
/// flock; the two overlap and `run_statusline` collects the status last.
fn spawn_passthrough(payload: &str, command: &str) -> std::io::Result<Child> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // A script that never reads stdin turns this into an EPIPE once it
        // exits — not an error worth relaying. Rust ignores SIGPIPE, so the
        // write returns instead of killing us. `stdin` drops here: the EOF
        // a `cat`-style reader is waiting for.
        let _ = stdin.write_all(payload.as_bytes());
    }
    Ok(child)
}

fn wait_code(mut child: Child) -> i32 {
    child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
}

/// Feed `payload` to `command` and return its exit status. The IO shell
/// splits this into spawn and wait around the store work; the tests use it
/// whole.
pub fn passthrough(payload: &str, command: &str) -> i32 {
    spawn_passthrough(payload, command).map_or(1, wait_code)
}

/// The IO shell's decision, with the paths injected so it tests against a
/// throwaway store. Parse, admit through `resolve_row` (the hook's own gate:
/// a `session_id` naming a row, or the pane's `CLAVE_AGENT_UUID`), then
/// PROBE on a lock-free copy and take the flock only when the probe says a
/// pixel moves. A session outside the fleet, or a reading that changes
/// nothing, never reaches the lock — the §6.5 property, and this path's
/// throttle in practice: most runs of a busy turn end right here.
pub fn meter(
    paths: &StorePaths,
    json: &str,
    env_uuid: Option<&str>,
    now: u64,
    zone: u32,
) -> Result<Option<AgentSnapshot>> {
    let reading = Reading::parse(json);
    let mut probe = read_store(paths)?;
    let Some(uuid) = resolve_row(&probe, reading.session_id.as_deref(), env_uuid) else {
        return Ok(None);
    };
    let own_claude = env_uuid == Some(uuid.as_str());
    if !apply_statusline(&mut probe, &uuid, &reading, now, own_claude, zone) {
        return Ok(None);
    }
    with_store_mut(paths, |s| {
        // Re-applied under the flock: a hook may have moved the row since
        // the probe, and this is the copy that persists.
        apply_statusline(s, &uuid, &reading, now, own_claude, zone).then(|| snapshot_from(s))
    })
}

/// The whole statusLine flow: hand the bytes on first, meter second, exit
/// with the wrapped command's status. Errors go to stderr only — the hook's
/// zero-risk citizenship, and here the user's status line is the thing a
/// clave bug must never blank.
pub fn run_statusline(stdin_json: &str, wrapped: &[String]) -> i32 {
    let child = (!wrapped.is_empty()).then(|| spawn_passthrough(stdin_json, &wrapped.join(" ")));
    let env_uuid = std::env::var(clave_types::AGENT_UUID_ENV).ok();
    match store_paths().and_then(|paths| {
        meter(
            &paths,
            stdin_json,
            env_uuid.as_deref(),
            now_unix(),
            smart_zone(),
        )
    }) {
        Ok(Some(snap)) => push_snapshot(&snap),
        Ok(None) => {}
        Err(e) => eprintln!("clave statusline: {e:#}"),
    }
    match child {
        None => 0,
        Some(Ok(child)) => wait_code(child),
        Some(Err(e)) => {
            eprintln!("clave statusline: spawning the wrapped command: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LabelSource;
    use clave_types::DEFAULT_SMART_ZONE_TOKENS as ZONE;
    use clave_types::Status;
    use std::collections::BTreeMap;

    /// The first payload captured live from the sandbox (Claude Code 2.1.257,
    /// 2026-09-01), trimmed to the keys clave reads plus a few it must ignore.
    const CAPTURED: &str = r#"{"session_id":"00000000-0000-4000-8000-c85c00000001","model":{"id":"claude-fable-5-1","display_name":"Fable 5.1"},"effort":{"level":"medium"},"context_window":{"total_input_tokens":19110,"total_output_tokens":4,"context_window_size":1000000,"current_usage":{"input_tokens":2,"output_tokens":4,"cache_creation_input_tokens":30,"cache_read_input_tokens":19078},"used_percentage":2,"remaining_percentage":98},"version":"2.1.257","cwd":"/x","exceeds_200k_tokens":false,"fast_mode":{"enabled":false},"vim":{"mode":"NORMAL"}}"#;

    /// The `context_window` of all six runs from that one turn, in order.
    const CAPTURED_WINDOWS: [&str; 6] = [
        r#"{"total_input_tokens":19110,"current_usage":{"input_tokens":2,"output_tokens":4,"cache_creation_input_tokens":30,"cache_read_input_tokens":19078}}"#,
        r#"{"total_input_tokens":19179,"current_usage":{"input_tokens":2,"output_tokens":1,"cache_creation_input_tokens":66,"cache_read_input_tokens":19111}}"#,
        r#"{"total_input_tokens":19179,"current_usage":{"input_tokens":2,"output_tokens":187,"cache_creation_input_tokens":66,"cache_read_input_tokens":19111}}"#,
        r#"{"total_input_tokens":19179,"current_usage":{"input_tokens":2,"output_tokens":187,"cache_creation_input_tokens":66,"cache_read_input_tokens":19111}}"#,
        r#"{"total_input_tokens":19811,"current_usage":{"input_tokens":56,"output_tokens":23,"cache_creation_input_tokens":578,"cache_read_input_tokens":19177}}"#,
        r#"{"total_input_tokens":20508,"current_usage":{"input_tokens":4,"output_tokens":46,"cache_creation_input_tokens":703,"cache_read_input_tokens":19801}}"#,
    ];

    fn payload(session: &str, total: &str, usage: &str, model: &str, effort: &str) -> String {
        format!(
            r#"{{"session_id":"{session}","model":{{"id":"{model}"}},"effort":{{"level":"{effort}"}},"context_window":{{"total_input_tokens":{total},"current_usage":{usage}}}}}"#
        )
    }

    fn reading(session: &str, tokens: Option<u32>, model: &str, effort: &str) -> Reading {
        Reading {
            session_id: Some(session.into()),
            tokens,
            model: Some(model.into()),
            effort: Some(effort.into()),
        }
    }

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
            metered_at: 0,
            buckets: BTreeMap::new(),
            model: None,
            provider: None,
            effort: None,
            pr_number: None,
            pr_checked: 0,
            pr_branch: String::new(),
        }
    }

    /// A row that already took one reading: 19110 tokens at t=1000.
    fn metered_store() -> Store {
        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        assert!(apply_statusline(
            &mut s,
            "minted",
            &reading("minted", Some(19_110), "claude-fable-5-1", "medium"),
            1000,
            true,
            ZONE,
        ));
        s
    }

    // ---- parsing -----------------------------------------------------------

    #[test]
    fn a_captured_payload_reads_as_session_tokens_model_and_effort() {
        assert_eq!(
            Reading::parse(CAPTURED),
            reading(
                "00000000-0000-4000-8000-c85c00000001",
                Some(19_110),
                "claude-fable-5-1",
                "medium"
            )
        );
    }

    #[test]
    fn a_zero_total_is_not_a_token_reading() {
        // Session start: no API response yet. Model and effort are still true.
        let r = Reading::parse(&payload("s", "0", "null", "claude-opus-5", "high"));
        assert_eq!(r, reading("s", None, "claude-opus-5", "high"));
    }

    #[test]
    fn a_null_usage_is_not_a_token_reading() {
        // The documented `/compact` window: a total with no usage behind it.
        let r = Reading::parse(&payload("s", "15500", "null", "claude-opus-5", "high"));
        assert_eq!(r.tokens, None);
    }

    #[test]
    fn a_missing_window_or_garbage_stdin_reads_as_nothing() {
        assert_eq!(
            Reading::parse(r#"{"session_id":"s"}"#),
            Reading {
                session_id: Some("s".into()),
                ..Default::default()
            }
        );
        assert_eq!(Reading::parse("not json"), Reading::default());
        assert_eq!(Reading::parse(""), Reading::default());
    }

    #[test]
    fn the_total_is_the_usage_summed_in_every_captured_run() {
        // The identity the hook path's USAGE_SUMMED relies on, pinned on the
        // captured runs so a future schema drift shows up here first.
        for w in CAPTURED_WINDOWS {
            let cw: ContextWindow = serde_json::from_str(w).unwrap();
            let u = cw.current_usage.unwrap();
            assert_eq!(
                cw.total_input_tokens.unwrap(),
                u.input_tokens + u.cache_creation_input_tokens + u.cache_read_input_tokens
            );
        }
    }

    // ---- apply -------------------------------------------------------------

    #[test]
    fn the_first_reading_lands_every_cell_and_stamps_the_meter() {
        let s = metered_store();
        let r = &s.agents["minted"];
        assert_eq!(r.context_tokens, Some(19_110));
        assert_eq!(r.context_level, Some(1));
        assert_eq!(r.model.as_deref(), Some("fable"));
        assert_eq!(r.provider.as_deref(), Some("claude"));
        assert_eq!(r.effort.as_deref(), Some("md"));
        assert_eq!(r.metered_at, 1000);
        assert_eq!(s.seq, 1);
    }

    #[test]
    fn a_moved_count_inside_the_interval_holds() {
        let mut s = metered_store();
        let r = reading("minted", Some(19_811), "claude-fable-5-1", "medium");
        assert!(!apply_statusline(
            &mut s,
            "minted",
            &r,
            1000 + APPLY_INTERVAL_SECS - 1,
            true,
            ZONE
        ));
        assert_eq!(s.agents["minted"].context_tokens, Some(19_110));
        assert_eq!(s.agents["minted"].metered_at, 1000);
        assert_eq!(s.seq, 1);
    }

    #[test]
    fn a_moved_count_after_the_interval_lands() {
        let mut s = metered_store();
        let r = reading("minted", Some(19_811), "claude-fable-5-1", "medium");
        assert!(apply_statusline(
            &mut s,
            "minted",
            &r,
            1000 + APPLY_INTERVAL_SECS,
            true,
            ZONE
        ));
        assert_eq!(s.agents["minted"].context_tokens, Some(19_811));
        assert_eq!(s.agents["minted"].metered_at, 1000 + APPLY_INTERVAL_SECS);
        assert_eq!(s.seq, 2);
    }

    #[test]
    fn a_level_change_inside_the_interval_lands_at_once() {
        let mut s = metered_store();
        let r = reading("minted", Some(31_000), "claude-fable-5-1", "medium");
        assert!(apply_statusline(&mut s, "minted", &r, 1001, true, ZONE));
        assert_eq!(s.agents["minted"].context_tokens, Some(31_000));
        assert_eq!(s.agents["minted"].context_level, Some(2));
        assert_eq!(s.agents["minted"].metered_at, 1001);
    }

    #[test]
    fn an_identical_reading_is_not_a_change() {
        let mut s = metered_store();
        let r = reading("minted", Some(19_110), "claude-fable-5-1", "medium");
        assert!(!apply_statusline(&mut s, "minted", &r, 5000, true, ZONE));
        assert_eq!(s.agents["minted"].metered_at, 1000);
        assert_eq!(s.seq, 1);
    }

    #[test]
    fn an_effort_change_inside_the_interval_lands_and_brings_the_count_with_it() {
        // `/effort high` mid-turn: the cell moves now, and since the row is
        // being written anyway the held count rides along for free.
        let mut s = metered_store();
        let r = reading("minted", Some(19_811), "claude-fable-5-1", "high");
        assert!(apply_statusline(&mut s, "minted", &r, 1001, true, ZONE));
        assert_eq!(s.agents["minted"].effort.as_deref(), Some("hi"));
        assert_eq!(s.agents["minted"].context_tokens, Some(19_811));
        assert_eq!(s.agents["minted"].metered_at, 1001);
    }

    #[test]
    fn a_model_change_inside_the_interval_lands() {
        let mut s = metered_store();
        let r = reading("minted", Some(19_110), "claude-opus-5", "medium");
        assert!(apply_statusline(&mut s, "minted", &r, 1001, true, ZONE));
        assert_eq!(s.agents["minted"].model.as_deref(), Some("opus"));
    }

    #[test]
    fn a_tokenless_reading_lands_model_and_effort_and_leaves_the_meter_unstamped() {
        // Session start: Claude Code runs the statusLine before any API call.
        let mut s = Store::default();
        s.agents.insert("minted".into(), rec("minted"));
        let r = reading("minted", None, "claude-fable-5-1", "medium");
        assert!(apply_statusline(&mut s, "minted", &r, 1000, true, ZONE));
        let row = &s.agents["minted"];
        assert_eq!(row.context_tokens, None);
        assert_eq!(row.context_level, None);
        assert_eq!(row.model.as_deref(), Some("fable"));
        assert_eq!(row.effort.as_deref(), Some("md"));
        assert_eq!(row.metered_at, 0);
    }

    #[test]
    fn a_tokenless_reading_holds_a_previous_count() {
        let mut s = metered_store();
        let r = reading("minted", None, "claude-fable-5-1", "medium");
        assert!(!apply_statusline(&mut s, "minted", &r, 5000, true, ZONE));
        assert_eq!(s.agents["minted"].context_tokens, Some(19_110));
    }

    #[test]
    fn a_rotation_resets_the_meter_and_lands_the_new_conversations_first_reading() {
        // `/clear`: a new id, a near-empty conversation. The reading is small
        // and arrives inside the interval, and it must land anyway — the
        // interval belonged to the conversation that just ended.
        let mut s = metered_store();
        let r = reading("cleared", Some(3_000), "claude-fable-5-1", "medium");
        assert!(apply_statusline(&mut s, "minted", &r, 1001, true, ZONE));
        let row = &s.agents["minted"];
        assert_eq!(row.live_session.as_deref(), Some("cleared"));
        assert_eq!(row.context_tokens, Some(3_000));
        assert_eq!(row.context_level, Some(0));
        assert_eq!(row.metered_at, 1001);
    }

    #[test]
    fn a_stop_cleared_stamp_lets_the_next_reading_land_inside_the_interval() {
        // The hook's Stop cleared the stamp (same conversation, a count
        // already held): the turn's final reading must not wait out the
        // interval that the turn's earlier readings paid.
        let mut s = metered_store();
        s.agents.get_mut("minted").unwrap().metered_at = 0;
        let r = reading("minted", Some(19_200), "claude-fable-5-1", "medium");
        assert!(apply_statusline(&mut s, "minted", &r, 1001, true, ZONE));
        assert_eq!(s.agents["minted"].context_tokens, Some(19_200));
        assert_eq!(s.agents["minted"].metered_at, 1001);
    }

    #[test]
    fn a_rotation_with_no_reading_yet_still_empties_the_battery() {
        // The statusLine's session-start run after a `/clear`: new id, zero
        // total. The battery goes to full because the conversation is new.
        let mut s = metered_store();
        let r = reading("cleared", None, "claude-fable-5-1", "medium");
        assert!(apply_statusline(&mut s, "minted", &r, 1001, true, ZONE));
        let row = &s.agents["minted"];
        assert_eq!(row.live_session.as_deref(), Some("cleared"));
        assert_eq!(row.context_tokens, Some(0));
        assert_eq!(row.metered_at, 0);
    }

    #[test]
    fn a_foreign_claude_never_moves_the_live_pointer() {
        // An outside `claude --resume <minted>` on the orphaned transcript:
        // admitted by id, so its reading lands, but the pointer holds (#99).
        let mut s = metered_store();
        s.agents.get_mut("minted").unwrap().live_session = Some("cleared".into());
        let r = reading("minted", Some(40_000), "claude-fable-5-1", "medium");
        assert!(apply_statusline(&mut s, "minted", &r, 5000, false, ZONE));
        let row = &s.agents["minted"];
        assert_eq!(row.live_session.as_deref(), Some("cleared"));
        assert_eq!(row.context_tokens, Some(40_000));
    }

    #[test]
    fn an_unknown_row_is_a_no_op() {
        let mut s = Store::default();
        let r = reading("ghost", Some(1), "claude-fable-5-1", "medium");
        assert!(!apply_statusline(&mut s, "ghost", &r, 1000, true, ZONE));
        assert_eq!(s.seq, 0);
    }

    // ---- the hook's side of the bargain -------------------------------------

    #[test]
    fn the_hook_yields_only_while_the_meter_is_recent() {
        let mut r = rec("minted");
        assert!(!hook_yields(&r, 1000));
        r.metered_at = 1000;
        assert!(hook_yields(&r, 1000));
        assert!(hook_yields(&r, 1000 + HOOK_YIELD_SECS - 1));
        assert!(!hook_yields(&r, 1000 + HOOK_YIELD_SECS));
        // A clock stepped backwards is "recent", never an underflow.
        assert!(hook_yields(&r, 999));
    }

    // ---- the passthrough --------------------------------------------------------

    #[test]
    fn the_wrapped_command_gets_the_same_bytes_on_stdin() {
        assert_eq!(
            passthrough("{\"a\":1}\n", r#"test "$(cat)" = '{"a":1}'"#),
            0
        );
        assert_eq!(passthrough("{\"a\":1}\n", r#"test "$(cat)" = other"#), 1);
    }

    #[test]
    fn the_wrapped_commands_exit_status_is_relayed() {
        assert_eq!(passthrough("", "exit 3"), 3);
    }

    #[test]
    fn a_wrapped_command_that_never_reads_a_large_payload_does_not_wedge() {
        // Bigger than a pipe buffer; the child exits without reading.
        assert_eq!(passthrough(&"x".repeat(200_000), "true"), 0);
    }

    // ---- the IO shell's decision ------------------------------------------------

    fn tmp_paths(dir: &std::path::Path) -> crate::store::StorePaths {
        crate::store::StorePaths {
            dir: dir.to_path_buf(),
            data: dir.join("agents.json"),
            lock: dir.join("agents.lock"),
        }
    }

    const CAPTURED_UUID: &str = "00000000-0000-4000-8000-c85c00000001";

    #[test]
    fn a_session_outside_the_fleet_never_touches_the_store() {
        let d = tempfile::tempdir().unwrap();
        let paths = tmp_paths(d.path());
        assert!(meter(&paths, CAPTURED, None, 1000, ZONE).unwrap().is_none());
        assert!(!paths.data.exists());
    }

    #[test]
    fn a_fleet_row_takes_the_reading_and_the_store_persists_it() {
        let d = tempfile::tempdir().unwrap();
        let paths = tmp_paths(d.path());
        crate::store::with_store_mut(&paths, |s| {
            s.agents.insert(CAPTURED_UUID.into(), rec(CAPTURED_UUID));
        })
        .unwrap();
        let snap = meter(&paths, CAPTURED, None, 1000, ZONE).unwrap();
        assert!(snap.is_some());
        let row = &crate::store::read_store(&paths).unwrap().agents[CAPTURED_UUID];
        assert_eq!(row.context_tokens, Some(19_110));
        assert_eq!(row.metered_at, 1000);
    }

    #[test]
    fn a_reading_that_moves_nothing_leaves_the_store_bytes_alone() {
        let d = tempfile::tempdir().unwrap();
        let paths = tmp_paths(d.path());
        crate::store::with_store_mut(&paths, |s| {
            s.agents.insert(CAPTURED_UUID.into(), rec(CAPTURED_UUID));
        })
        .unwrap();
        meter(&paths, CAPTURED, None, 1000, ZONE).unwrap();
        let before = std::fs::read(&paths.data).unwrap();
        assert!(meter(&paths, CAPTURED, None, 5000, ZONE).unwrap().is_none());
        assert_eq!(std::fs::read(&paths.data).unwrap(), before);
    }

    #[test]
    fn the_panes_env_uuid_admits_a_rotated_session_and_moves_the_pointer() {
        let d = tempfile::tempdir().unwrap();
        let paths = tmp_paths(d.path());
        crate::store::with_store_mut(&paths, |s| {
            s.agents.insert("minted".into(), rec("minted"));
        })
        .unwrap();
        let json = payload(
            "cleared",
            "3000",
            r#"{"input_tokens":3000}"#,
            "claude-fable-5-1",
            "low",
        );
        assert!(
            meter(&paths, &json, Some("minted"), 1000, ZONE)
                .unwrap()
                .is_some()
        );
        let row = &crate::store::read_store(&paths).unwrap().agents["minted"];
        assert_eq!(row.live_session.as_deref(), Some("cleared"));
        assert_eq!(row.context_tokens, Some(3_000));
        assert_eq!(row.effort.as_deref(), Some("lo"));
    }
}
