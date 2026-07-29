//! The cwd → transcript-dir munging rule (spec §4). This is the JOIN KEY:
//! `clave spawn`'s idempotency check computes
//! `~/.claude/projects/<munge_cwd(cwd)>/<uuid>.jsonl` and tests existence.
//! Claude replaces EVERY non-alphanumeric character (not just `/`) with `-`, so
//! a `.` becomes `-` too — critical for dotted and worktree paths.
//!
//! CANONICALIZE FIRST (verified on disk by S0b, 2026-07-01): Claude munges the
//! PHYSICAL cwd — it reads `getcwd()`, which resolves symlinks (on macOS
//! `/var` → `/private/var`, `/tmp` → `/private/tmp`). `munge_cwd` is a pure
//! string transform, so callers (`clave spawn`, `clave add`) MUST resolve the
//! cwd to its physical path (`std::fs::canonicalize`) BEFORE calling — else the
//! join key misses the real jsonl, `spawn` takes the create path, and Claude
//! aborts with `Session ID … is already in use`. The char-rule below matched
//! disk exactly (worktree `--` included); only the input needed canonicalizing.

/// Replace every ASCII-non-alphanumeric character in `cwd` with `-`, matching
/// Claude Code's `~/.claude/projects/<dir>` naming (empirically
/// `s/[^A-Za-z0-9]/-/g`, verified on disk — spec §4). Non-ASCII characters are
/// not `[A-Za-z0-9]`, so they are dashed too.
pub fn munge_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn munges_leading_slash_path() {
        // Generic paths on purpose: this repo is public, and a fixture only
        // needs the SHAPE — a leading slash and interior separators.
        assert_eq!(munge_cwd("/Users/o/code/clave"), "-Users-o-code-clave");
    }

    #[test]
    fn munges_dots_and_worktree_double_dash() {
        // The on-disk shape (spec §4), with the names genericized. What is
        // load-bearing is the `--`: adjacent `/` and `.` in
        // `/.claude-worktrees` each become a dash, which is why a worktree cwd
        // must be munged rather than pattern-matched.
        assert_eq!(
            munge_cwd("/Users/o/code/proj/.claude-worktrees/wt-one"),
            "-Users-o-code-proj--claude-worktrees-wt-one"
        );
    }

    #[test]
    fn maps_every_non_alnum_including_unicode() {
        // Non-ASCII letters are not [A-Za-z0-9] under Claude's rule → dashed.
        assert_eq!(munge_cwd("a.b_c d"), "a-b-c-d");
        assert_eq!(munge_cwd("café"), "caf-"); // é is non-ASCII-alnum → '-'
    }
}
