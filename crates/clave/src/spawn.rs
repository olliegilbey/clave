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

/// Where Claude Code stores this session's transcript. `physical_cwd` MUST
/// already be canonicalized (S0b: Claude munges getcwd(), which resolves
/// symlinks) — pass the output of `std::fs::canonicalize`, never raw user
/// input, or the join key misses and create collides ("already in use").
pub fn jsonl_path(home: &Path, physical_cwd: &str, uuid: &str) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(munge_cwd(physical_cwd))
        .join(format!("{uuid}.jsonl"))
}

pub fn spawn_mode(home: &Path, physical_cwd: &str, uuid: &str) -> SpawnMode {
    if jsonl_path(home, physical_cwd, uuid).exists() {
        SpawnMode::Resume
    } else {
        SpawnMode::Create
    }
}

/// Register this pane with the bar: uuid → $ZELLIJ_PANE_ID (spike S2 verified
/// the env var IS exported to layout `command` panes). Best-effort: a failed
/// registration only costs nav-to-this-agent until the next register; it must
/// NEVER stop the exec into Claude. Fire-and-forget spawn — `zellij pipe` can
/// dawdle (S1) and the exec below replaces this process anyway.
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
    let _ = Command::new("zellij")
        .args(["pipe", "--name", "clave-register", "--", &payload])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_path_uses_munged_physical_cwd() {
        let home = std::path::Path::new("/Users/x");
        let p = jsonl_path(home, "/Users/x/code/clave", "u-1");
        assert_eq!(
            p,
            std::path::PathBuf::from("/Users/x/.claude/projects/-Users-x-code-clave/u-1.jsonl")
        );
    }

    #[test]
    fn spawn_mode_is_resume_iff_jsonl_exists() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path();
        let cwd = "/Users/x/code/clave";
        assert_eq!(spawn_mode(home, cwd, "u-1"), SpawnMode::Create);
        // Drop the jsonl where Claude would write it → next spawn resumes.
        let dir = home.join(".claude/projects/-Users-x-code-clave");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("u-1.jsonl"), b"{}").unwrap();
        assert_eq!(spawn_mode(home, cwd, "u-1"), SpawnMode::Resume);
    }
}
