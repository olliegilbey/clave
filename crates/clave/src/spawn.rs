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
    /// No jsonl on disk → `claude --session-id <uuid> --name <label>`.
    /// S0: a fresh uuid CREATES the session and writes the jsonl.
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
