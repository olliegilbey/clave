//! `clave spawn <uuid> --name <label> --cwd <cwd>` (§6.1) — the command every
//! agent pane runs. Idempotent BY CONSTRUCTION: the same command re-run on
//! Zellij resurrection resumes the same conversation instead of erroring or
//! forking, because the create/resume branch is decided by whether Claude's
//! own transcript jsonl exists (invariant #5).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clave_types::Register;

use crate::munge::munge_cwd;

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
