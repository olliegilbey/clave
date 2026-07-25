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
    AgentRecord, LabelSource, now_unix, read_store, snapshot_from, store_paths, with_store_mut,
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

/// Scan a jsonl TAIL for the LAST `{"type":"summary","summary":…}` line.
/// Line-wise serde parse — no regex, no full-file model of Claude's schema.
pub fn summary_from_tail(tail: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        summary: Option<String>,
    }
    tail.lines()
        .rev()
        .find_map(|l| match serde_json::from_str::<Line>(l) {
            Ok(line) if line.kind == "summary" => line.summary,
            _ => None,
        })
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

/// §6.4 label refresh. Returns whether the label changed. The `dir · branch`
/// prefix is rebuilt from the record so the rule stays in one place:
/// label = `<last-path-component of cwd> · <branch> [· <words>]`.
pub fn refresh_label(
    rec: &mut AgentRecord,
    event: &str,
    payload: &HookPayload,
    jsonl_tail: Option<&str>,
) -> bool {
    // Once a summary named the session, it stays (§6.4: stop re-scanning).
    if rec.label_source == LabelSource::Summary {
        return false;
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
    false
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

/// The whole hook flow. Errors bubble up ONLY so main can log them to
/// stderr — main exits 0 no matter what (Global Constraint).
pub fn run_hook(event: &str, stdin_json: &str) -> Result<()> {
    let payload: HookPayload = serde_json::from_str(stdin_json).unwrap_or_default();
    let Some(uuid) = payload.session_id.clone() else {
        return Ok(()); // no session_id → nothing to key on
    };
    let paths = store_paths()?;
    // FAST PATH (§6.5): lock-free read; untracked session → exit immediately.
    // clave must never serialize unrelated sessions' hooks behind its lock.
    if !read_store(&paths)?.agents.contains_key(&uuid) {
        return Ok(());
    }
    // §6.9: claude_config_dir() (not raw home) so the sandbox override
    // reaches the same jsonl tree real claude processes write to.
    let claude_dir = crate::env::claude_config_dir().unwrap_or_default();
    let snap = with_store_mut(&paths, |s| {
        // Label refresh only re-reads the jsonl while it's still cheap to
        // matter (§6.4): source==FirstPrompt and a label-bearing event.
        let tail = s.agents.get(&uuid).and_then(|rec| {
            if rec.label_source == LabelSource::FirstPrompt
                && matches!(event, "Stop" | "UserPromptSubmit")
            {
                read_tail(&jsonl_path(&claude_dir, &rec.cwd, &uuid), 64 * 1024)
            } else {
                None
            }
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
            claude_codex: false,
            tab_id: None,
            stale: false,
        }
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
        };
        assert!(!refresh_label(&mut r, "UserPromptSubmit", &injected, None));
        assert_eq!(r.label, "x · main"); // unchanged — still the bare prefix
        assert_eq!(r.label_source, LabelSource::FirstPrompt); // still eligible

        // The next REAL prompt earns the label exactly as before the guard.
        let real = HookPayload {
            session_id: Some("u1".into()),
            prompt: Some("fix the flaky auth test".into()),
            message: None,
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
        };
        assert!(refresh_label(&mut r, "UserPromptSubmit", &p, None));
        assert!(
            !r.label.contains('\\') && !r.label.contains('"'),
            "unsanitized label: {}",
            r.label
        );
    }
}
