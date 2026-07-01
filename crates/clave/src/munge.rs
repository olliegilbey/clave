//! The cwd → transcript-dir munging rule (spec §4). This is the JOIN KEY:
//! `clave spawn`'s idempotency check computes
//! `~/.claude/projects/<munge_cwd(cwd)>/<uuid>.jsonl` and tests existence.
//! Claude replaces EVERY non-alphanumeric byte (not just `/`) with `-`, so a
//! `.` becomes `-` too — critical for dotted and worktree paths. Pinned by S0b.

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
        assert_eq!(
            munge_cwd("/Users/olliegilbey/code/clave"),
            "-Users-olliegilbey-code-clave"
        );
    }

    #[test]
    fn munges_dots_and_worktree_double_dash() {
        // Verified-on-disk example (spec §4). Note the `--`: adjacent `/` and `.`
        // in `/.claude-worktrees` each become a dash.
        assert_eq!(
            munge_cwd("/Users/olliegilbey/code/resumate/.claude-worktrees/nalu-cta"),
            "-Users-olliegilbey-code-resumate--claude-worktrees-nalu-cta"
        );
    }

    #[test]
    fn maps_every_non_alnum_including_unicode() {
        // Non-ASCII letters are not [A-Za-z0-9] under Claude's rule → dashed.
        assert_eq!(munge_cwd("a.b_c d"), "a-b-c-d");
        assert_eq!(munge_cwd("café"), "caf-"); // é is non-ASCII-alnum → '-'
    }
}
