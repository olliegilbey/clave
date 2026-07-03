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

/// §6.5's transition table, verbatim. Latest-wins: the CURRENT status is
/// irrelevant; each event maps directly to the new one (a later lower-
/// "priority" event must be able to downgrade needs_you after you answer).
pub fn status_for_event(event: &str, message: Option<&str>) -> Option<Status> {
    match event {
        "UserPromptSubmit" => Some(Status::Working),
        "Stop" => Some(Status::Done),
        "StopFailure" => Some(Status::Failed),
        "SessionEnd" => Some(Status::Idle),
        "PermissionRequest" => Some(Status::NeedsYou),
        "Notification" => {
            // §4: match the notification MESSAGE TEXT for the two needs-you
            // cases. Substrings chosen from the live payloads observed in S1;
            // Task 9 checkpoint C2 re-verifies them against the current CLI.
            let m = message.unwrap_or("");
            if m.contains("permission") || m.contains("waiting for your input") {
                Some(Status::NeedsYou)
            } else {
                None
            }
        }
        // Unknown/other events: strictly no-op. The hook is registered for a
        // fixed set, but a defensive default keeps novel events harmless.
        _ => None,
    }
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
    let prefix = format!("{dir} · {}", rec.branch);
    // Prefer a summary from the tail (Stop is when summaries appear)…
    if let Some(summary) = jsonl_tail.and_then(summary_from_tail) {
        let label = format!("{prefix} · {}", first_words(&summary));
        rec.label = label;
        rec.label_source = LabelSource::Summary;
        return true;
    }
    // …else, on the first prompt, use the prompt text from the payload.
    // "First" = the label is still the bare `dir · branch` from `clave add`.
    // (Collapsed into an edition-2024 let-chain — clippy::collapsible_if.)
    if event == "UserPromptSubmit"
        && rec.label == prefix
        && let Some(p) = payload.prompt.as_deref().filter(|p| !p.trim().is_empty())
    {
        rec.label = format!("{prefix} · {}", first_words(p));
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
    let _ = Command::new("zellij")
        .args(["pipe", "--name", "clave-status", "--", &payload])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
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
    let home = dirs::home_dir().unwrap_or_default();
    let snap = with_store_mut(&paths, |s| {
        let Some(rec) = s.agents.get_mut(&uuid) else {
            return None; // raced a prune — fine
        };
        let mut changed = false;
        if let Some(next) = status_for_event(event, payload.message.as_deref()) {
            changed |= rec.status != next;
            rec.status = next;
        }
        if event == "UserPromptSubmit" {
            rec.last_interacted = now_unix(); // recency (§6.6 order)
            changed = true;
        }
        // Label refresh only re-reads the jsonl while it's still cheap to
        // matter (§6.4): source==FirstPrompt and a label-bearing event.
        let tail = if rec.label_source == LabelSource::FirstPrompt
            && matches!(event, "Stop" | "UserPromptSubmit")
        {
            read_tail(&jsonl_path(&home, &rec.cwd, &uuid), 64 * 1024)
        } else {
            None
        };
        changed |= refresh_label(rec, event, &payload, tail.as_deref());
        if changed {
            s.seq += 1; // monotonic pipe contract (§5)
            Some(snapshot_from(s))
        } else {
            None
        }
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
        }
    }

    #[test]
    fn state_machine_is_latest_wins() {
        // Spec §6.5 transition table, verbatim.
        assert_eq!(
            status_for_event("UserPromptSubmit", None),
            Some(Status::Working)
        );
        assert_eq!(status_for_event("Stop", None), Some(Status::Done));
        assert_eq!(status_for_event("StopFailure", None), Some(Status::Failed));
        assert_eq!(status_for_event("SessionEnd", None), Some(Status::Idle));
        assert_eq!(
            status_for_event("PermissionRequest", None),
            Some(Status::NeedsYou)
        );
        // Notification matches on MESSAGE TEXT (§4): permission / idle prompts.
        assert_eq!(
            status_for_event(
                "Notification",
                Some("Claude needs your permission to use Bash")
            ),
            Some(Status::NeedsYou)
        );
        assert_eq!(
            status_for_event("Notification", Some("Claude is waiting for your input")),
            Some(Status::NeedsYou)
        );
        // Other notifications don't touch status.
        assert_eq!(status_for_event("Notification", Some("compacting…")), None);
        // Unknown events are a no-op — the global hook must never guess.
        assert_eq!(status_for_event("PreToolUse", None), None);
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
}
