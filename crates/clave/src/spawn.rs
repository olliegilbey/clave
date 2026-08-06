//! `clave spawn <uuid> --name <label> --cwd <cwd>` (§6.1) — the command every
//! agent pane runs. Idempotent BY CONSTRUCTION: the same command re-run on
//! Zellij resurrection resumes the same conversation instead of erroring or
//! forking, because the create/resume branch is decided by whether Claude's
//! own transcript jsonl exists (invariant #5).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Result;
use clave_types::Register;

use crate::munge::munge_cwd;
use crate::store::AgentRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnMode {
    /// No jsonl on disk → `claude --session-id <uuid>`.
    /// S0: a fresh uuid CREATES the session and writes the jsonl.
    ///
    /// **No `--name`**, and the constraint is design-lock §2 / LEDGER D19:
    /// the title chip stays BLANK until the user renames. Claude records
    /// `--name` as a `custom-title`, which the hook cannot tell from a
    /// `/rename`, so passing it filled that chip with clave's own label on
    /// an agent nobody had named (#91). Full reasoning, including why
    /// filtering it downstream fails, is at the exec site in `main.rs`.
    Create,
    /// jsonl exists → `claude --resume <uuid>`. `--resume` errors when no
    /// jsonl exists, which is why existence drives the branch (S0).
    Resume,
}

/// Where Claude Code stores this session's transcript, under the given
/// CLAUDE CONFIG DIR (`env::claude_config_dir()` — sandbox-aware, §6.9).
/// `physical_cwd` MUST already be canonicalized (S0b) — pass
/// `std::fs::canonicalize` output, never raw user input.
pub fn jsonl_path(claude_dir: &Path, physical_cwd: &str, uuid: &str) -> PathBuf {
    claude_dir
        .join("projects")
        .join(munge_cwd(physical_cwd))
        .join(format!("{uuid}.jsonl"))
}

pub fn spawn_mode(claude_dir: &Path, physical_cwd: &str, uuid: &str) -> SpawnMode {
    if jsonl_path(claude_dir, physical_cwd, uuid).exists() {
        SpawnMode::Resume
    } else {
        SpawnMode::Create
    }
}

/// WHICH conversation this pane comes back on, and how to open it (#99).
///
/// The minted uuid names the conversation this pane STARTED with, not the one
/// it is in: Claude rotates its session id whenever the pane gets a fresh
/// conversation (a `/clear` is confirmed), and the rotated transcript is a
/// separate file that `--resume <minted>` does not chain forward to. Confirmed
/// live rather than reasoned: resurrecting on the minted uuid reopened the
/// pre-`/clear` file, appended to it, and the agent knew nothing said after the
/// clear. A `just release` puts EVERY pane through that, so the loss lands at
/// upgrade time, silently, across the whole fleet.
///
/// So the store's `live_session` — written by the hook from the payload — is
/// preferred whenever the transcript it names is present on disk. That it
/// DIFFERS from the minted uuid is guaranteed upstream, not checked here: the
/// hook stores agreement as `None` (see `AgentRecord::live_session`), and if an
/// equal value ever did arrive both paths agree anyway — `(Resume, uuid)` when
/// that transcript exists, `(Create, uuid)` when it does not.
///
/// Existence is what keeps the preference from being trust: a stale id (the
/// transcript deleted, the session relocated to another cwd) degrades to
/// exactly the pre-#99 behaviour rather than handing `--resume` an id it will
/// reject, which would leave the pane dead instead of merely behind. The
/// leading-`-` refusal is the same instinct one step earlier — `--resume` takes
/// an OPTIONAL value, so a stored id shaped like a flag would be read by
/// `claude` as a flag rather than rejected as a session. It is stored payload
/// text reaching argv; `resolve_transcript` validates its sibling field from
/// the same payload, and this one should not be the exception.
///
/// The returned id is the exec ARGUMENT only. The row's identity on the wire
/// stays the minted uuid — `CLAVE_AGENT_UUID`, the store key, the bar's join —
/// so nothing downstream has to learn about rotation.
pub fn resume_target(
    claude_dir: &Path,
    physical_cwd: &str,
    uuid: &str,
    live_session: Option<&str>,
) -> (SpawnMode, String) {
    if let Some(live) = live_session.filter(|l| !l.starts_with('-'))
        && jsonl_path(claude_dir, physical_cwd, live).exists()
    {
        return (SpawnMode::Resume, live.to_string());
    }
    (spawn_mode(claude_dir, physical_cwd, uuid), uuid.to_string())
}

/// Where — and as which conversation — this pane should exec claude (#139).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnSite {
    /// The baked cwd hosts the conversation (or legitimately hosts nothing
    /// yet): spawn exactly as before #139.
    Here {
        mode: SpawnMode,
        session: String,
        cwd: String,
    },
    /// The transcript RELOCATED (the #59/#69 move, e.g. the session walked
    /// into a worktree): resume `session` from its true cwd — derived from the
    /// transcript's own tail — and repoint the store row there.
    Moved {
        session: String,
        cwd: String,
        branch: Option<String>,
    },
}

/// Tail budget for the relocation read — same 64 KiB the hook reads.
const RELOC_TAIL_BYTES: u64 = 64 * 1024;

/// The one place `<id>.jsonl` lives under `claude_dir/projects/*/`, by EXACT
/// id (#139). Unambiguous because relocation MOVES the file, never copies it
/// (FOOTGUNS, #59/#69) — this is NOT the newest-transcript heuristic #99 bans,
/// which matched across DIFFERENT ids. If copies do exist (hand surgery), the
/// newest-modified is the one being written, so it wins. Read-only on the
/// claude dir.
pub fn locate_transcript(claude_dir: &Path, id: &str) -> Option<PathBuf> {
    let file = format!("{id}.jsonl");
    let mut hits: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(claude_dir.join("projects"))
        .ok()?
        .flatten()
    {
        let p = entry.path().join(&file);
        if let Ok(meta) = p.metadata() {
            hits.push((meta.modified().unwrap_or(std::time::UNIX_EPOCH), p));
        }
    }
    hits.into_iter().max_by_key(|(m, _)| *m).map(|(_, p)| p)
}

/// The LAST top-level non-empty string `field` in `tail` — newest wins,
/// because a relocated session's tail carries its post-move `cwd` on recent
/// lines while older lines may predate the move. Same line-wise walk as
/// `hook::last_tail_field`, minus the `type` gate: `cwd`/`gitBranch` ride
/// ordinary message lines, not typed metadata lines.
fn last_tail_str(tail: &str, field: &str) -> Option<String> {
    tail.lines().rev().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        let s = v.get(field)?.as_str()?.trim();
        (!s.is_empty()).then(|| s.to_string())
    })
}

/// The transcript's own report of where its session runs (#139): the `cwd`
/// field on recent lines, newest wins.
pub fn cwd_from_tail(tail: &str) -> Option<String> {
    last_tail_str(tail, "cwd")
}

/// The transcript's own report of its branch (`gitBranch`), newest wins.
/// None on transcripts outside a repo — the caller keeps the row's branch.
pub fn branch_from_tail(tail: &str) -> Option<String> {
    last_tail_str(tail, "gitBranch")
}

/// Has this row EVER held a conversation? Gates #139's fail-loudly: a row
/// with no transcript anywhere is either a FRESH agent whose first exec is
/// about to create one (create must proceed — `clave add` new depends on it)
/// or an established session whose transcript is gone (creating would
/// silently shadow it — fail instead). The store can tell the two apart
/// because every prompted session earns row prose: `refresh_row_fields`
/// seeds `summary` from the first prompt, status leaves Idle on it, and
/// `title`/`live_session` only ever come from a live transcript. A fresh
/// `add` row has none of those. (`last_visited` is deliberately NOT
/// evidence: the birth tab is focused before any prompt exists.)
pub fn conversation_evidenced(rec: &AgentRecord) -> bool {
    rec.live_session.is_some()
        || rec.title.is_some()
        || !rec.summary.is_empty()
        || rec.status != clave_types::Status::Idle
}

/// #139: verify the transcript is where the row says before exec'ing claude,
/// and follow it if it moved.
///
/// Preference order, live conversation first at every step (#99 — resuming
/// the frozen minted file when a rotated live one exists loses everything
/// since the `/clear`, and relocation is just that loss expressed through a
/// worktree):
///
/// 1. live id's jsonl at the baked cwd → resume it here (pre-#139 path).
/// 2. live id's jsonl ANYWHERE else → it relocated; follow it.
/// 3. minted uuid's jsonl at the baked cwd → resume here (pre-#139 path).
/// 4. minted uuid's jsonl anywhere else → follow it.
/// 5. nothing anywhere: create ONLY for a row that has never conversed
///    (`conversation_evidenced` false — the fresh `clave add` birth);
///    otherwise FAIL LOUDLY. Never silently shadow a real session with a
///    fresh one — that miss path is the incident this exists for.
///
/// `physical_cwd` is `None` when the baked cwd no longer exists on disk (a
/// removed worktree whose session moved out): the search still recovers 2/4,
/// and 5 is always an error there — with no cwd there is nowhere to create.
///
/// A found-elsewhere transcript whose tail is malformed (unreadable, no cwd,
/// or a vanished target dir) is a LOUD error even when a minted transcript
/// still sits at the baked cwd. Deliberate, do not "fix" it into a fallback:
/// degrading to the minted id would reopen the pre-`/clear` conversation —
/// exactly the silent wrong-conversation attach #99 exists to forbid. The
/// pane dying with a message naming the transcript is the cheaper failure.
pub fn verified_site(
    claude_dir: &Path,
    physical_cwd: Option<&str>,
    uuid: &str,
    live_session: Option<&str>,
    evidenced: bool,
) -> Result<SpawnSite> {
    // Same refusal as `resume_target`: a stored id shaped like a flag must
    // never reach argv (`--resume` takes an OPTIONAL value).
    let live = live_session.filter(|l| !l.starts_with('-'));
    if let Some(l) = live {
        if let Some(cwd) = physical_cwd
            && jsonl_path(claude_dir, cwd, l).exists()
        {
            return Ok(SpawnSite::Here {
                mode: SpawnMode::Resume,
                session: l.to_string(),
                cwd: cwd.to_string(),
            });
        }
        if let Some(found) = locate_transcript(claude_dir, l) {
            return moved_site(&found, l);
        }
    }
    if let Some(cwd) = physical_cwd
        && jsonl_path(claude_dir, cwd, uuid).exists()
    {
        return Ok(SpawnSite::Here {
            mode: SpawnMode::Resume,
            session: uuid.to_string(),
            cwd: cwd.to_string(),
        });
    }
    if let Some(found) = locate_transcript(claude_dir, uuid) {
        return moved_site(&found, uuid);
    }
    // Zero hits anywhere. Only a row that has NEVER conversed may create —
    // that is the fresh `clave add` birth, whose first exec writes the jsonl.
    match (physical_cwd, evidenced) {
        (Some(cwd), false) => Ok(SpawnSite::Here {
            mode: SpawnMode::Create,
            session: uuid.to_string(),
            cwd: cwd.to_string(),
        }),
        (Some(_), true) => anyhow::bail!(
            "no transcript found for session {uuid} anywhere under {}: this row \
             has already conversed, so starting a FRESH session would silently \
             shadow the real one (deleted transcript, or retention pruned it?). \
             Refusing — find the jsonl or remove the row.",
            claude_dir.join("projects").display()
        ),
        (None, _) => anyhow::bail!(
            "agent cwd no longer exists and no transcript found for session \
             {uuid} under {} — nothing to resume and nowhere to create",
            claude_dir.join("projects").display()
        ),
    }
}

/// A found-elsewhere transcript → the `Moved` site: its true cwd comes from
/// its OWN tail (newest wins), canonicalized (S0b) and required to exist —
/// a vanished target dir is a loud error, never a silent create.
fn moved_site(transcript: &Path, session: &str) -> Result<SpawnSite> {
    let tail = crate::hook::read_tail(transcript, RELOC_TAIL_BYTES)
        .ok_or_else(|| anyhow::anyhow!("unreadable transcript {}", transcript.display()))?;
    let cwd = cwd_from_tail(&tail).ok_or_else(|| {
        anyhow::anyhow!(
            "transcript {} carries no cwd in its tail; cannot follow the relocation",
            transcript.display()
        )
    })?;
    let canon = std::fs::canonicalize(&cwd).map_err(|e| {
        anyhow::anyhow!(
            "transcript {} relocated to cwd {cwd}, which no longer exists: {e}",
            transcript.display()
        )
    })?;
    let cwd = canon
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF8 relocated cwd"))?
        .to_string();
    // `claude --resume` is project-dir-scoped: it looks under the munged dir
    // of the cwd it runs in. The tail's cwd must therefore map back to the
    // dir the transcript was FOUND in — they disagree when the tail lags the
    // move (relocated file, no post-move lines yet). Resuming there would
    // surface as Claude's opaque "No conversation found"; fail naming both
    // dirs instead (#143 review).
    let found_dir = transcript.parent().and_then(|p| p.file_name());
    if found_dir != Some(std::ffi::OsStr::new(&munge_cwd(&cwd))) {
        anyhow::bail!(
            "transcript {} lives under project dir {:?} but its tail names cwd \
             {cwd} (munged: {}) — the tail lags the move; resume from the \
             transcript's own dir is not derivable (munge is lossy). Refusing \
             rather than resuming where Claude would find nothing.",
            transcript.display(),
            found_dir.unwrap_or_default(),
            munge_cwd(&cwd)
        );
    }
    Ok(SpawnSite::Moved {
        session: session.to_string(),
        cwd,
        branch: branch_from_tail(&tail),
    })
}

/// #139 (review): can this session be recovered through relocation even
/// though its baked cwd is gone? Open-time gate only — `run_open` must not
/// reject a row as stale when the conversation demonstrably moved somewhere
/// spawnable; the spawn re-runs the search and repoints the row.
pub fn relocation_recoverable(claude_dir: &Path, uuid: &str, live_session: Option<&str>) -> bool {
    let live = live_session.filter(|l| !l.starts_with('-'));
    [live, Some(uuid)].into_iter().flatten().any(|id| {
        locate_transcript(claude_dir, id)
            .map(|t| moved_site(&t, id).is_ok())
            .unwrap_or(false)
    })
}

/// Register this pane with the bar: uuid → $ZELLIJ_PANE_ID (spike S2 verified
/// the env var IS exported to layout `command` panes). Best-effort: a failed
/// registration only costs nav-to-this-agent until the next register; it must
/// NEVER stop the exec into Claude.
///
/// DOUBLE-FORK, not a plain spawn (C7 finding, 2026-07-14): a directly
/// spawned child is inherited by the exec'd claude, which never reaps it —
/// the permanent ZOMBIE is then what zellij's session serializer reads as
/// the pane's running command, so dump-layout said `<defunct>` for every
/// agent pane, blinding the §6.3 liveness check and breaking resurrection.
/// `sh -c '… &'` backgrounds the pipe, sh exits instantly (we reap it), and
/// the grandchild reparents to init — nothing is left in the pane's tree.
pub fn register_pane(uuid: &str) {
    let Ok(pane_id) = std::env::var("ZELLIJ_PANE_ID") else {
        eprintln!("clave spawn: ZELLIJ_PANE_ID unset; skipping bar registration");
        return;
    };
    let Ok(pane_id) = pane_id.parse::<u32>() else {
        eprintln!("clave spawn: unparseable ZELLIJ_PANE_ID {pane_id:?}");
        return;
    };
    let reg = Register {
        uuid: uuid.to_string(),
        pane_id,
    };
    let payload = match serde_json::to_string(&reg) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("clave spawn: register serialize failed: {e}");
            return;
        }
    };
    // `"$@" &` keeps the payload out of shell-quoting territory: argv is
    // passed verbatim after the `sh` placeholder. status() reaps sh itself.
    let _ = Command::new("/bin/sh")
        .args([
            "-c",
            "\"$@\" >/dev/null 2>&1 &",
            "sh",
            "zellij",
            "pipe",
            "--name",
            "clave-register",
            "--",
            &payload,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_path_uses_munged_physical_cwd_under_claude_dir() {
        // §6.9: the CLAUDE CONFIG DIR is the parameter (not home) so the
        // sandbox override flows through — real claude processes honor
        // $CLAUDE_CONFIG_DIR and write transcripts to the same tree.
        let claude = std::path::Path::new("/Users/x/.claude");
        let p = jsonl_path(claude, "/Users/x/code/clave", "u-1");
        assert_eq!(
            p,
            std::path::PathBuf::from("/Users/x/.claude/projects/-Users-x-code-clave/u-1.jsonl")
        );
    }

    /// #99, confirmed live: resurrection on the minted uuid reopens the
    /// PRE-ROTATION conversation and orphans everything said since the
    /// `/clear`. `--resume <superseded-id>` does not re-chain — it appends to
    /// that file — so the recovery has to happen here, at the choice of id.
    #[test]
    fn resurrection_targets_the_live_conversation_and_degrades_safely() {
        let d = tempfile::tempdir().unwrap();
        let claude = d.path().join(".claude");
        let cwd = "/Users/x/code/clave";
        let dir = claude.join("projects/-Users-x-code-clave");
        std::fs::create_dir_all(&dir).unwrap();
        let plant = |stem: &str| std::fs::write(dir.join(format!("{stem}.jsonl")), b"{}").unwrap();
        let target = |live| resume_target(&claude, cwd, "minted", live);

        // No rotation recorded: exactly the pre-#99 behaviour, both ways.
        assert_eq!(target(None), (SpawnMode::Create, "minted".to_string()));
        plant("minted");
        assert_eq!(target(None), (SpawnMode::Resume, "minted".to_string()));

        // Rotated, but the live transcript is not on disk (deleted, or the
        // session relocated): fall back rather than hand `--resume` an id it
        // would reject, which leaves the pane DEAD instead of merely behind.
        assert_eq!(
            target(Some("rotated")),
            (SpawnMode::Resume, "minted".to_string())
        );

        // Rotated and present: the live conversation wins.
        plant("rotated");
        assert_eq!(
            target(Some("rotated")),
            (SpawnMode::Resume, "rotated".to_string())
        );

        // A live id that AGREES with the minted uuid is stored as `None` by the
        // hook and so should never arrive; pinned anyway because it is what
        // lets this function skip the comparison — it decides nothing.
        assert_eq!(
            resume_target(&claude, cwd, "unminted", Some("unminted")),
            (SpawnMode::Create, "unminted".to_string())
        );
        assert_eq!(
            resume_target(&claude, cwd, "minted", Some("minted")),
            (SpawnMode::Resume, "minted".to_string())
        );

        // A stored id shaped like a FLAG never reaches argv. `--resume` takes an
        // optional value, so `claude` would read it as a flag rather than
        // rejecting it as a session id.
        plant("--dangerously-skip-permissions");
        assert_eq!(
            target(Some("--dangerously-skip-permissions")),
            (SpawnMode::Resume, "minted".to_string())
        );
    }

    /// #139 fixture: a claude dir + a REAL (canonicalized — macOS resolves
    /// /tmp to /private/tmp, and munge keys off the PHYSICAL path) agent cwd,
    /// with a helper that plants a transcript under any cwd's munged dir.
    struct Reloc {
        _tmp: tempfile::TempDir,
        claude: PathBuf,
        cwd: String,
    }

    fn reloc_fixture() -> Reloc {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        std::fs::create_dir_all(claude.join("projects")).unwrap();
        let cwd_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&cwd_dir).unwrap();
        // Canonicalize like every caller must (S0b / FOOTGUNS munge entry).
        let cwd = std::fs::canonicalize(&cwd_dir)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        Reloc {
            _tmp: tmp,
            claude,
            cwd,
        }
    }

    impl Reloc {
        /// Plant `<stem>.jsonl` with `lines` under the munged dir for `cwd`.
        fn plant(&self, cwd: &str, stem: &str, lines: &str) {
            let dir = self.claude.join("projects").join(munge_cwd(cwd));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{stem}.jsonl")), lines).unwrap();
        }
        /// A second real, canonical cwd (the relocation target).
        fn other_cwd(&self, name: &str) -> String {
            let d = self._tmp.path().join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::canonicalize(&d)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        }
    }

    /// Transcript lines the way Claude writes them: ordinary message lines
    /// carry top-level `cwd`/`gitBranch`; typed metadata lines carry neither.
    fn tail_lines(cwd: &str, branch: &str) -> String {
        format!(
            "{}\n{}\n{{\"cwd\":\"{cwd}\",\"gitBranch\":\"{branch}\",\"message\":\"post-move\"}}\n",
            r#"{"type":"ai-title","aiTitle":"doing things"}"#,
            r#"{"cwd":"/somewhere/old","gitBranch":"main","message":"pre-move"}"#,
        )
    }

    #[test]
    fn cwd_and_branch_come_from_the_newest_tail_line() {
        let tail = tail_lines("/its/new/home", "feat/x");
        // Newest wins: the post-move line, not the pre-move one.
        assert_eq!(cwd_from_tail(&tail).as_deref(), Some("/its/new/home"));
        assert_eq!(branch_from_tail(&tail).as_deref(), Some("feat/x"));
        // Empty values and non-carrying/garbage lines are skipped, scanning
        // further back rather than returning blank.
        let tail = "{\"cwd\":\"/real\"}\n{\"cwd\":\"\"}\nnot json\n{\"type\":\"summary\"}\n";
        assert_eq!(cwd_from_tail(tail).as_deref(), Some("/real"));
        assert_eq!(branch_from_tail(tail), None);
        assert_eq!(cwd_from_tail(""), None);
    }

    /// A tail that LAGS the move (transcript found under the new munged dir,
    /// last cwd lines still naming the old home — no post-move lines yet)
    /// must refuse loudly: resuming at the tail's cwd is where Claude finds
    /// no conversation. (#143 review)
    #[test]
    fn a_lagging_tail_refuses_rather_than_resuming_where_nothing_lives() {
        let f = reloc_fixture();
        let old_home = f.other_cwd("old-home");
        // Found under "moved-here"'s dir, tail still says old_home.
        let moved_here = f.other_cwd("moved-here");
        f.plant(&moved_here, "u-lag", &tail_lines(&old_home, "main"));
        let transcript = f
            .claude
            .join("projects")
            .join(munge_cwd(&moved_here))
            .join("u-lag.jsonl");
        let err = moved_site(&transcript, "u-lag").unwrap_err().to_string();
        assert!(err.contains("lags the move"), "unexpected error: {err}");
        // And the open-time gate agrees: not recoverable through relocation.
        assert!(!relocation_recoverable(&f.claude, "u-lag", None));
    }

    /// The open-time gate: a session whose transcript moved somewhere
    /// spawnable IS recoverable; an unknown one is not. (#143 review)
    #[test]
    fn relocation_recoverable_follows_a_clean_move_only() {
        let f = reloc_fixture();
        let new_cwd = f.other_cwd("clean-move");
        f.plant(&new_cwd, "u-moved", &tail_lines(&new_cwd, "feat/x"));
        assert!(relocation_recoverable(&f.claude, "u-moved", None));
        assert!(relocation_recoverable(&f.claude, "other", Some("u-moved")));
        assert!(!relocation_recoverable(&f.claude, "u-nope", None));
    }

    #[test]
    fn locate_transcript_matches_the_exact_uuid_only() {
        let f = reloc_fixture();
        f.plant("/a", "u-1", "{}");
        f.plant("/b", "u-12", "{}"); // a PREFIX-sharing stranger
        let hit = locate_transcript(&f.claude, "u-1").expect("exact hit");
        assert!(hit.ends_with(format!("{}/u-1.jsonl", munge_cwd("/a"))));
        assert_eq!(locate_transcript(&f.claude, "u-nope"), None);
    }

    /// The relocation read must reach well past a kilobyte of trailing
    /// metadata: a busy session's tail is typed lines (no `cwd`) for pages
    /// before the last message line. Pins RELOC_TAIL_BYTES as a real 64 KiB
    /// budget (mutants: `64 * 1024` -> `64 + 1024` starved exactly this).
    #[test]
    fn the_reloc_tail_budget_reaches_past_a_kilobyte_of_metadata() {
        let f = reloc_fixture();
        let target = f.other_cwd("moved-to");
        let filler = r#"{"type":"file-history-snapshot","x":"y"}"#;
        let lines = format!(
            "{{\"cwd\":\"{target}\",\"message\":\"post-move\"}}\n{}",
            vec![filler; 40].join("\n")
        );
        assert!(lines.len() > 1088 && lines.len() < 64 * 1024);
        f.plant(&target, "u-deep", &lines);
        let transcript = f
            .claude
            .join("projects")
            .join(munge_cwd(&target))
            .join("u-deep.jsonl");
        match moved_site(&transcript, "u-deep").unwrap() {
            SpawnSite::Moved { cwd, .. } => assert_eq!(cwd, target),
            other => panic!("expected Moved, got {other:?}"),
        }
    }

    #[test]
    fn a_transcript_at_the_rows_cwd_spawns_here_unchanged() {
        let f = reloc_fixture();
        f.plant(&f.cwd, "minted", "{}");
        let site = verified_site(&f.claude, Some(&f.cwd), "minted", None, true).unwrap();
        assert_eq!(
            site,
            SpawnSite::Here {
                mode: SpawnMode::Resume,
                session: "minted".into(),
                cwd: f.cwd.clone(),
            }
        );
    }

    /// The incident (#139): the row's cwd is frozen where the agent was
    /// born, the transcript walked into a worktree. The spawn must follow
    /// the transcript — resuming at the derived cwd — never create fresh.
    #[test]
    fn a_relocated_transcript_is_followed_to_its_tail_cwd() {
        let f = reloc_fixture();
        let new_cwd = f.other_cwd("repo-wt");
        f.plant(&new_cwd, "minted", &tail_lines(&new_cwd, "fix/100-dwell"));
        let site = verified_site(&f.claude, Some(&f.cwd), "minted", None, true).unwrap();
        assert_eq!(
            site,
            SpawnSite::Moved {
                session: "minted".into(),
                cwd: new_cwd,
                branch: Some("fix/100-dwell".into()),
            }
        );
        // The baked cwd may itself be GONE (worktree removed): the search
        // still recovers the session.
        let site = verified_site(&f.claude, None, "minted", None, true).unwrap();
        assert!(matches!(site, SpawnSite::Moved { .. }));
    }

    /// #99 compounded by relocation: the pane rotated (live id) AND the live
    /// conversation moved away, while the frozen minted file stayed put.
    /// Resuming the local minted file would reopen the pre-/clear
    /// conversation — the live one wins WHEREVER it is.
    #[test]
    fn the_live_conversation_beats_the_local_minted_file() {
        let f = reloc_fixture();
        let new_cwd = f.other_cwd("wt");
        f.plant(&f.cwd, "minted", "{}"); // frozen pre-rotation, still local
        f.plant(&new_cwd, "rotated", &tail_lines(&new_cwd, "feat/y"));
        let site = verified_site(&f.claude, Some(&f.cwd), "minted", Some("rotated"), true).unwrap();
        assert_eq!(
            site,
            SpawnSite::Moved {
                session: "rotated".into(),
                cwd: new_cwd,
                branch: Some("feat/y".into()),
            }
        );
        // Live id shaped like a flag is refused (same rule as resume_target):
        // fall through to the minted file rather than argv-inject.
        let site = verified_site(&f.claude, Some(&f.cwd), "minted", Some("--evil"), true).unwrap();
        assert_eq!(
            site,
            SpawnSite::Here {
                mode: SpawnMode::Resume,
                session: "minted".into(),
                cwd: f.cwd.clone(),
            }
        );
    }

    /// Zero hits anywhere: the fresh `clave add` birth must still CREATE
    /// (no conversation exists to shadow), but an evidenced row — one that
    /// has already conversed — FAILS LOUDLY, naming the uuid and where we
    /// looked. Never a silent fresh session over a real one.
    #[test]
    fn zero_hits_creates_only_for_a_never_conversed_row() {
        let f = reloc_fixture();
        let site = verified_site(&f.claude, Some(&f.cwd), "minted", None, false).unwrap();
        assert_eq!(
            site,
            SpawnSite::Here {
                mode: SpawnMode::Create,
                session: "minted".into(),
                cwd: f.cwd.clone(),
            }
        );
        let err = verified_site(&f.claude, Some(&f.cwd), "minted", None, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("minted"), "names the uuid: {err}");
        assert!(err.contains("projects"), "names the searched tree: {err}");
        // No cwd to create in → error regardless of evidence.
        assert!(verified_site(&f.claude, None, "minted", None, false).is_err());
    }

    /// A found transcript whose tail cwd no longer exists (the worktree was
    /// deleted after the move) cannot be spawned — loud error, not create.
    #[test]
    fn a_relocated_transcript_with_a_vanished_cwd_fails_loudly() {
        let f = reloc_fixture();
        f.plant("/x", "minted", &tail_lines("/gone/for/good", "feat/z"));
        assert!(verified_site(&f.claude, Some(&f.cwd), "minted", None, true).is_err());
        // …and a tail carrying NO cwd at all is equally loud.
        f.plant(
            "/x",
            "minted",
            "{\"type\":\"ai-title\",\"aiTitle\":\"t\"}\n",
        );
        assert!(verified_site(&f.claude, Some(&f.cwd), "minted", None, true).is_err());
    }

    #[test]
    fn evidence_is_prose_a_conversation_left_behind() {
        let rec = |f: fn(&mut AgentRecord)| {
            let mut r = AgentRecord {
                uuid: "u".into(),
                cwd: "/x".into(),
                repo_root: "/x".into(),
                branch: "main".into(),
                label: "x \u{00b7} main".into(),
                status: clave_types::Status::Idle,
                last_interacted: 9,
                commit_ord: 3,
                last_visited: 8, // visited is NOT evidence (birth tab focus)
                worktree: None,
                label_source: crate::store::LabelSource::FirstPrompt,
                tab_id: None,
                stale: false,
                title: None,
                summary: String::new(),
                default_branch: None,
                live_session: None,
            };
            f(&mut r);
            r
        };
        assert!(!conversation_evidenced(&rec(|_| {})));
        assert!(conversation_evidenced(&rec(
            |r| r.summary = "fix auth".into()
        )));
        assert!(conversation_evidenced(&rec(
            |r| r.title = Some("CLV".into())
        )));
        assert!(conversation_evidenced(&rec(
            |r| r.status = clave_types::Status::Working
        )));
        assert!(conversation_evidenced(&rec(
            |r| r.live_session = Some("rot".into())
        )));
    }

    #[test]
    fn spawn_mode_is_resume_iff_jsonl_exists() {
        let d = tempfile::tempdir().unwrap();
        let claude = d.path().join(".claude");
        let cwd = "/Users/x/code/clave";
        assert_eq!(spawn_mode(&claude, cwd, "u-1"), SpawnMode::Create);
        let dir = claude.join("projects/-Users-x-code-clave");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("u-1.jsonl"), b"{}").unwrap();
        assert_eq!(spawn_mode(&claude, cwd, "u-1"), SpawnMode::Resume);
    }
}
