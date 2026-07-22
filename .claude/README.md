# `.claude/`

Repo-vendored Claude Code configuration and tooling. Anything here is checked
into git so it travels with the repo on any clone — including a cloud agent's
sandboxed checkout, which has no access to a maintainer's personal
`~/.claude/` config.

## Contents

- `workflows/fugu-review.js` — the Fugu-style multi-agent review harness: N
  blind reviewers (haiku/sonnet/opus, high effort, optionally plus
  coderabbit/codex/gemini CLI lanes) run in parallel, then an opus
  consolidator dedupes and adversarially verifies findings against the actual
  codebase. Read-only — it never edits files.
- `commands/fugu-review.md` — the `/fugu-review` slash command that briefs
  and invokes the workflow above. See that file for usage, argument shape,
  and known caveats (notably the CLI-lane empty-diff bug tracked as clave
  issue #20).

## Running it

From any session in a clone of this repo: `/fugu-review <what to review>`
(a plan, "the current diff", or a path/feature). No setup beyond having
Claude Code itself — the `coderabbit`/`codex`/`gemini` CLI lanes are optional
extras that degrade to zero findings (not a failure) if their CLI isn't
installed or authenticated.

## `settings.json` vs `settings.local.json`

- `.claude/settings.json` (if present) is **tracked** — shared, repo-wide
  Claude Code settings that should apply to every clone.
- `.claude/settings.local.json` is **gitignored** — per-machine overrides
  (personal permissions, local paths, etc.) that must never be committed.

Anything you add under `.claude/` is public the moment it's committed — this
repo is public. Keep vendored files machine-agnostic: no absolute paths under
a personal home directory, no references to private repos or notes.
