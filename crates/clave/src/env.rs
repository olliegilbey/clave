//! Sandbox env overrides (spec §6.9): the `clave dev` harness redirects every
//! path/session lookup through these so a scenario can never touch the real
//! store, session, or ~/.claude. Pure kernels + thin env readers: the kernels
//! are what's unit-tested (setting real env vars would race parallel tests).

use std::path::PathBuf;

use anyhow::{Context, Result};

/// `$CLAVE_SESSION` or the default dedicated session (§6.8).
pub fn session_name() -> String {
    session_name_from(std::env::var("CLAVE_SESSION").ok())
}

pub fn session_name_from(var: Option<String>) -> String {
    var.filter(|s| !s.is_empty())
        .unwrap_or_else(|| "clave".to_string())
}

/// `$CLAUDE_CONFIG_DIR` or `~/.claude` — where Claude Code keeps
/// `projects/<munged>/<uuid>.jsonl` and settings.json. Claude itself honors
/// the same variable, which is what makes the §6.9 sandbox airtight: the
/// REAL claude processes a scenario spawns write their transcripts here too.
pub fn claude_config_dir() -> Result<PathBuf> {
    let default = dirs::home_dir().context("no home dir")?.join(".claude");
    Ok(dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok(), default))
}

pub fn dir_from(var: Option<String>, default: PathBuf) -> PathBuf {
    match var.filter(|s| !s.is_empty()) {
        Some(v) => PathBuf::from(v),
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_defaults_and_overrides() {
        assert_eq!(session_name_from(None), "clave");
        assert_eq!(session_name_from(Some(String::new())), "clave"); // empty = unset
        assert_eq!(session_name_from(Some("clave-test".into())), "clave-test");
    }

    #[test]
    fn dir_from_defaults_and_overrides() {
        let d = PathBuf::from("/default");
        assert_eq!(dir_from(None, d.clone()), d);
        assert_eq!(dir_from(Some(String::new()), d.clone()), d);
        assert_eq!(dir_from(Some("/x".into()), d), PathBuf::from("/x"));
    }
}
