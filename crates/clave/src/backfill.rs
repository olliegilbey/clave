//! Frecency bucket backfill from Claude's transcripts (§CLAUDE.md: the jsonl
//! store is the source of truth; clave's store is a cache over it). Two
//! consumers: `add::mint_record` seeds a RESUMED row from its own history at
//! birth, and `setup::launch_session` runs the fleet-wide one-shot on version
//! refresh so an upgraded (or stripped, #106) store re-derives its last week.
//!
//! Seed-only, always: a row that has EARNED buckets is never overwritten —
//! the transcript count is slightly conservative (it can't see birth-touches
//! or opener-inherited weight), so replacing earned state with it would
//! silently shrink real scores.

use std::collections::BTreeMap;
use std::path::Path;

use crate::store::Store;

/// The unix day of an ISO-8601 `timestamp` line field ("2026-08-20T…").
/// Days-from-civil (Howard Hinnant's algorithm), dependency-free — the
/// workspace deliberately carries no date crate. Only the date prefix is
/// read: bucket granularity is a DAY, so the time-of-day is noise here.
/// (UTC dates, matching the transcript's `Z` timestamps; `now_unix()/86400`
/// on the scoring side is the same UTC day arithmetic.)
pub fn unix_day_from_iso(ts: &str) -> Option<u32> {
    let b = ts.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = ts.get(0..4)?.parse().ok()?;
    let m: i64 = ts.get(5..7)?.parse().ok()?;
    let d: i64 = ts.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468; // 719468 = days_from_civil(1970,1,1)
    u32::try_from(days).ok()
}

/// One transcript line is a COMMITMENT iff it is what UserPromptSubmit fires
/// on: a genuine user turn. `type:"user"` lines also carry tool results
/// (content arrays of `tool_result` blocks), meta lines (`isMeta`), and
/// subagent turns (`isSidechain`) — none of those were the user committing to
/// this conversation. Verified against a live transcript 2026-08-20: genuine
/// prompts are content STRINGS (or arrays without a `tool_result` block —
/// pasted attachments); every tool result led its array with one.
fn is_commitment(v: &serde_json::Value) -> bool {
    if v.get("type").and_then(|t| t.as_str()) != Some("user") {
        return false;
    }
    if v.get("isMeta").and_then(|m| m.as_bool()) == Some(true)
        || v.get("isSidechain").and_then(|m| m.as_bool()) == Some(true)
    {
        return false;
    }
    match v.get("message").and_then(|m| m.get("content")) {
        Some(serde_json::Value::String(_)) => true,
        Some(serde_json::Value::Array(blocks)) => !blocks
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result")),
        _ => false,
    }
}

/// Derive a row's day buckets from its transcript: genuine user turns per
/// unix day, windowed to the same trailing-7-days arithmetic as the store's
/// `bump_bucket` prune and the bar's scoring cutoff (strict: `day + RETAIN >
/// today`). Unparseable lines are skipped — a transcript is external input
/// and a hostile line must cost nothing (§6.5 zero-risk stance).
pub fn buckets_from_transcript(text: &str, today: u32) -> BTreeMap<u32, u32> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if !is_commitment(&v) {
            continue;
        }
        let Some(day) = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(unix_day_from_iso)
        else {
            continue;
        };
        if day + clave_types::BUCKET_RETAIN_DAYS > today && day <= today {
            *out.entry(day).or_insert(0) += 1;
        }
    }
    out
}

/// Read-and-derive for one row, probing the places the transcript can live
/// (#139: it is not always under the row's cwd). First existing file wins;
/// a rotated row reads its LIVE conversation's transcript, the same file
/// `resolve_transcript` follows for tails.
///
/// When every derived path misses, fall back to scanning `projects/*` for
/// `<session>.jsonl` by NAME: a worktree session's transcript lives under
/// the worktree's own munged dir while the row's cwd names the checkout
/// (found live 2026-08-20 — the maintainer's own fleet had three such
/// rows), and a session id is globally unique, so the first hit is THE
/// transcript. One readdir per cold row, one-shot paths only.
pub fn derive_for_row(
    claude_dir: &Path,
    cwds: &[&str],
    uuid: &str,
    live_session: Option<&str>,
    today: u32,
) -> Option<BTreeMap<u32, u32>> {
    let text = read_transcript_text(claude_dir, cwds, uuid, live_session)?;
    let derived = buckets_from_transcript(&text, today);
    (!derived.is_empty()).then_some(derived)
}

/// The transcript text for one row, wherever it lives — the same path
/// resolution `derive_for_row` uses, factored out so `backfill_store` can
/// also read the newest assistant line's model off the identical text
/// (one file read, two derivations).
fn read_transcript_text(
    claude_dir: &Path,
    cwds: &[&str],
    uuid: &str,
    live_session: Option<&str>,
) -> Option<String> {
    let session = live_session.unwrap_or(uuid);
    let path = cwds
        .iter()
        .map(|cwd| crate::spawn::jsonl_path(claude_dir, cwd, session))
        .find(|p| p.exists())
        .or_else(|| scan_projects_for(claude_dir, session))?;
    std::fs::read_to_string(path).ok()
}

/// `projects/*/<session>.jsonl`, wherever it lives.
fn scan_projects_for(claude_dir: &Path, session: &str) -> Option<std::path::PathBuf> {
    let projects = std::fs::read_dir(claude_dir.join("projects")).ok()?;
    projects
        .flatten()
        .map(|d| d.path().join(format!("{session}.jsonl")))
        .find(|p| p.exists())
}

/// The fleet-wide one-shot: seed every row with EMPTY buckets, and seed
/// model/provider independently from the transcript tail. Earned buckets are
/// never touched, so re-running is free — which is what lets `launch_session`
/// fire it on every version refresh without bookkeeping. Model/provider are
/// seeded only if the row's model is None (i.e., backfill is a seeder, never
/// an updater). A row counts as seeded if either buckets or model/provider
/// were written. Returns the number of rows seeded.
pub fn backfill_store(s: &mut Store, claude_dir: &Path, today: u32) -> usize {
    let mut seeded = 0;
    for rec in s.agents.values_mut() {
        if !rec.buckets.is_empty() {
            continue;
        }
        let Some(text) = read_transcript_text(
            claude_dir,
            &[rec.cwd.as_str(), rec.repo_root.as_str()],
            &rec.uuid,
            rec.live_session.as_deref(),
        ) else {
            continue;
        };
        let mut changed = false;

        // Seed buckets only if derived buckets are non-empty.
        let derived = buckets_from_transcript(&text, today);
        if !derived.is_empty() {
            rec.buckets = derived;
            changed = true;
        }

        // Seed model/provider independently, unconditional on bucket emptiness,
        // only if rec.model is None (backfill is a seeder, never an updater).
        if rec.model.is_none()
            && let Some(model) =
                crate::hook::model_from_tail(&text).map(|m| crate::hook::short_model(&m))
        {
            rec.model = Some(model);
            rec.provider = Some("claude".to_string());
            changed = true;
        }

        if changed {
            seeded += 1;
        }
    }
    seeded
}

/// Fired at the tail of every `setup::run_setup` (first run, upgrade
/// refresh, and the idempotent `clave setup` — the maintainer flow's only
/// reachable path, since `clave release` pre-installs the wasm and the
/// launch refresh therefore never fires on the cutting machine).
/// Best-effort by design: a failed backfill must never block a launch (the
/// same zero-risk stance as the evlog). Quiet when there was nothing to do —
/// most refreshes find a fully-earned store.
pub fn run_on_version_refresh() {
    let res = (|| -> anyhow::Result<usize> {
        let paths = crate::store::store_paths()?;
        let claude_dir = crate::env::claude_config_dir()?;
        let today = crate::store::unix_day(crate::store::now_unix());
        crate::store::with_store_mut(&paths, |s| backfill_store(s, &claude_dir, today))
    })();
    match res {
        Ok(0) => {}
        Ok(n) => {
            println!("clave: seeded frecency for {n} row(s) from transcripts");
            crate::evlog::log_event("backfill", &format!("{n} rows seeded from transcripts"));
        }
        Err(e) => eprintln!("clave backfill: {e:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AgentRecord;

    #[test]
    fn unix_day_matches_the_stores_epoch_arithmetic() {
        // 2026-08-20 = 20685 (1755648000 / 86400) — cross-checked against
        // `date +%s` the day this landed. Epoch day 0 and a leap day pin the
        // civil conversion's edges.
        assert_eq!(unix_day_from_iso("2026-08-20T14:03:11.000Z"), Some(20685));
        assert_eq!(unix_day_from_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_day_from_iso("2024-02-29T23:59:59Z"), Some(19782));
        assert_eq!(unix_day_from_iso("garbage"), None);
        assert_eq!(unix_day_from_iso("2026-13-01T00:00:00Z"), None);
        assert_eq!(unix_day_from_iso(""), None);
        // A bare date (exactly 10 bytes, no time part) is a valid prefix.
        assert_eq!(unix_day_from_iso("2026-08-20"), Some(20685));
        // EITHER separator being wrong refuses — this one parses cleanly as
        // numbers, so the guard is the only thing standing.
        assert_eq!(unix_day_from_iso("2026-08:20T00:00:00Z"), None);
        assert_eq!(unix_day_from_iso("2026:08-20T00:00:00Z"), None);
    }

    fn line(day_iso: &str, content: &str, extra: &str) -> String {
        format!(
            r#"{{"type":"user","timestamp":"{day_iso}T10:00:00.000Z","message":{{"role":"user","content":{content}}}{extra}}}"#
        )
    }

    #[test]
    fn only_genuine_user_turns_in_window_are_counted() {
        // today = 20685 (2026-08-20); window keeps 20679..=20685.
        let t = [
            line("2026-08-20", r#""a real prompt""#, ""), // counts
            // On a day OF THEIR OWN, so a mutant flipping the array branch
            // (counting tool results, dropping attachments) shifts the map
            // instead of trading one for the other invisibly.
            line("2026-08-18", r#"[{"type":"text","text":"pasted"}]"#, ""), // counts (attachment array)
            line(
                "2026-08-17",
                r#"[{"type":"tool_result","content":"x"}]"#,
                "",
            ), // tool result: NOT a commitment
            line("2026-08-20", r#""command output""#, r#","isMeta":true"#), // meta
            line("2026-08-20", r#""subagent turn""#, r#","isSidechain":true"#), // sidechain
            line("2026-08-14", r#""today-6, the window's oldest day""#, ""), // counts
            line("2026-08-13", r#""exactly 7 days old""#, ""),              // outside strict window
            line("2026-08-19", r#""yesterday""#, ""),                       // counts
            line("2026-08-21", r#""future-dated""#, ""),                    // clock skew: dropped
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:00Z"}"#.into(),
            r#"{"type":"user","message":{"content":"no timestamp"}}"#.into(),
            "not json at all".into(),
        ]
        .join("\n");
        let b = buckets_from_transcript(&t, 20685);
        assert_eq!(
            b,
            [(20685u32, 1u32), (20684, 1), (20683, 1), (20679, 1)].into()
        );
    }

    fn rec(uuid: &str, cwd: &str) -> AgentRecord {
        AgentRecord {
            uuid: uuid.into(),
            cwd: cwd.into(),
            repo_root: cwd.into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: clave_types::Status::Idle,
            last_interacted: 0,
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: crate::store::LabelSource::FirstPrompt,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
            buckets: BTreeMap::new(),
            model: None,
            provider: None,
            pr_number: None,
            pr_checked: 0,
            pr_branch: String::new(),
        }
    }

    /// Write a transcript where `jsonl_path` will look for it.
    fn write_transcript(claude_dir: &Path, cwd: &str, session: &str, body: &str) {
        let p = crate::spawn::jsonl_path(claude_dir, cwd, session);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn backfill_seeds_empty_rows_and_never_touches_earned_buckets() {
        let dir = tempfile::tempdir().unwrap();
        let today = 20685;
        let mut s = Store::default();
        s.agents.insert("u-cold".into(), rec("u-cold", "/x"));
        let mut earned = rec("u-earned", "/x");
        earned.buckets = [(20685, 9)].into();
        s.agents.insert("u-earned".into(), earned);
        s.agents
            .insert("u-no-jsonl".into(), rec("u-no-jsonl", "/x"));
        write_transcript(
            dir.path(),
            "/x",
            "u-cold",
            &line("2026-08-20", r#""hey""#, ""),
        );
        write_transcript(
            dir.path(),
            "/x",
            "u-earned",
            &[
                line("2026-08-20", r#""p1""#, ""),
                line("2026-08-20", r#""p2""#, ""),
            ]
            .join("\n"),
        );

        assert_eq!(backfill_store(&mut s, dir.path(), today), 1);
        assert_eq!(s.agents["u-cold"].buckets, [(20685u32, 1u32)].into());
        // Earned state survives even though its transcript disagrees.
        assert_eq!(s.agents["u-earned"].buckets, [(20685u32, 9u32)].into());
        assert!(s.agents["u-no-jsonl"].buckets.is_empty());
        // Idempotent: the seeded row now has buckets, so a second pass is free.
        assert_eq!(backfill_store(&mut s, dir.path(), today), 0);
    }

    #[test]
    fn a_rotated_row_reads_its_live_conversations_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::default();
        let mut rotated = rec("u-minted", "/x");
        rotated.live_session = Some("u-live".into());
        s.agents.insert("u-minted".into(), rotated);
        // The minted uuid's transcript is stale history; the live one is the
        // conversation the row actually is (#99).
        write_transcript(
            dir.path(),
            "/x",
            "u-live",
            &line("2026-08-20", r#""after rotation""#, ""),
        );
        assert_eq!(backfill_store(&mut s, dir.path(), 20685), 1);
        assert_eq!(s.agents["u-minted"].buckets, [(20685u32, 1u32)].into());
    }

    /// A worktree session's transcript lives under the WORKTREE's munged
    /// dir while the row's cwd names the checkout — the by-name scan finds
    /// it anyway.
    #[test]
    fn a_transcript_outside_the_rows_dirs_is_found_by_scan() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::default();
        s.agents.insert("u-wt".into(), rec("u-wt", "/x"));
        write_transcript(
            dir.path(),
            "/x/.claude/worktrees/feature",
            "u-wt",
            &line("2026-08-20", r#""from the worktree""#, ""),
        );
        assert_eq!(backfill_store(&mut s, dir.path(), 20685), 1);
        assert_eq!(s.agents["u-wt"].buckets, [(20685u32, 1u32)].into());
    }

    #[test]
    fn backfill_seeds_model_and_provider_from_the_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::default();
        s.agents.insert("u-model".into(), rec("u-model", "/x"));
        write_transcript(
            dir.path(),
            "/x",
            "u-model",
            &[
                line("2026-08-20", r#""hey""#, ""),
                r#"{"type":"assistant","timestamp":"2026-08-20T10:00:01.000Z","message":{"model":"claude-opus-5","usage":{"input_tokens":10}}}"#.into(),
            ]
            .join("\n"),
        );
        assert_eq!(backfill_store(&mut s, dir.path(), 20685), 1);
        assert_eq!(s.agents["u-model"].model, Some("opus".to_string()));
        assert_eq!(s.agents["u-model"].provider, Some("claude".to_string()));
    }

    #[test]
    fn backfill_never_invents_a_model_without_an_assistant_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::default();
        s.agents.insert("u-nomodel".into(), rec("u-nomodel", "/x"));
        write_transcript(
            dir.path(),
            "/x",
            "u-nomodel",
            &line("2026-08-20", r#""hey""#, ""),
        );
        assert_eq!(backfill_store(&mut s, dir.path(), 20685), 1);
        assert_eq!(s.agents["u-nomodel"].model, None);
        assert_eq!(s.agents["u-nomodel"].provider, None);
    }

    #[test]
    fn an_out_of_window_only_transcript_seeds_model_from_the_tail() {
        // A dormant conversation whose only commitments are outside the 7-day
        // window derives EMPTY buckets, but if the tail carries a valid
        // assistant line, model/provider are seeded nonetheless — the
        // source-of-truth principle protects these long-dormant rows from
        // starting cold.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::default();
        s.agents.insert("u-dormant".into(), rec("u-dormant", "/x"));
        write_transcript(
            dir.path(),
            "/x",
            "u-dormant",
            &[
                line("2026-08-01", r#""long ago""#, ""),
                r#"{"type":"assistant","timestamp":"2026-08-01T10:00:01.000Z","message":{"model":"claude-sonnet-4","usage":{"input_tokens":10}}}"#.into(),
            ]
            .join("\n"),
        );
        assert_eq!(backfill_store(&mut s, dir.path(), 20685), 1);
        // Buckets stay empty — all commitments are outside the window.
        assert!(s.agents["u-dormant"].buckets.is_empty());
        // But model/provider are seeded from the tail's assistant line.
        assert_eq!(s.agents["u-dormant"].model, Some("sonnet".to_string()));
        assert_eq!(s.agents["u-dormant"].provider, Some("claude".to_string()));
    }

    #[test]
    fn an_out_of_window_only_transcript_seeds_nothing() {
        // A dormant giant whose whole history is stale derives an EMPTY map —
        // the row stays empty rather than gaining a zero-weight bucket the
        // store's prune would have dropped.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::default();
        s.agents.insert("u-old".into(), rec("u-old", "/x"));
        write_transcript(
            dir.path(),
            "/x",
            "u-old",
            &line("2026-08-01", r#""long ago""#, ""),
        );
        assert_eq!(backfill_store(&mut s, dir.path(), 20685), 0);
        assert!(s.agents["u-old"].buckets.is_empty());
    }
}
