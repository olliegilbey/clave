# Handoff: doctor + install flow (branch `worktree-doctor-install`)

## Task Overview

Build clave's install/health story end-to-end: `clave doctor` (grouped health
report), binary discovery beyond PATH, per-command preflight, one-command
first run, and single-file distribution (wasm embedded in release binaries,
cargo-dist → attested GitHub Releases). Success criteria: a fresh box — local
or SSH — goes from one downloaded file to a running session, and every
missing dependency produces exact, copy-pasteable guidance instead of a raw
failure. Sequenced ahead of a README+VHS-GIF rewrite (sub-project 2, NOT
started) so the README documents real installer behavior.

## Reference Docs

- `docs/superpowers/specs/2026-07-21-installer-doctor-design.md` — the locked
  spec, whole file (~300 lines). §Discovery + §First run carry the
  2026-07-22 review addenda (upgrade refresh, current_exe baking).
- `docs/superpowers/plans/2026-07-21-doctor-install.md` — the 11-task
  implementation plan, fully executed. Read only if archaeology is needed;
  the code is the truth now.
- `CONTRIBUTING.md` — two-environments table + release model (the §2
  invariants this work extends).

## Current State

**All implementation is DONE, merged with current main, and live-validated.**
Branch `worktree-doctor-install` in worktree
`.claude/worktrees/doctor-install/`, 23 commits ahead of the old branch
point, including a merge of origin/main (`2a031f0`). Gates: 195 workspace
tests green, `cargo fmt --check` clean, `cargo clippy --workspace
--all-targets -- -D warnings` fully clean.

- New: `crates/clave/src/discover.rs` (ToolId/Via/Discovered, override →
  which_global → curated dirs), `crates/clave/src/doctor.rs` (Facts →
  diagnose → golden-locked renderers, gather, preflight, `clave doctor
  [--json]`), `crates/clave/build.rs` (CLAVE_BAR_WASM embed, wasm-magic
  guard).
- Modified: setup.rs (first-run consent flow, upgrade refresh, extract
  embedded wasm, hooks dedupe, current_exe baking, discovered zellij),
  main.rs (Doctor cmd; spawn execs discovered claude), add.rs (preflight +
  pane-hold; discovered fzf/zoxide/git/zellij), dev.rs (idempotent scenario
  seeding), justfile (`dist-build`), `dist-workspace.toml` +
  `.github/workflows/build-wasm-setup.yml` + generated release.yml
  (cargo-dist 0.32.0: 4 targets incl. both linux-musl, shell installer,
  attestations; `clave-bar` excluded via `dist = false`).

Live validation (human, sandbox): doctor all-green TTY output, Alt+a
missing-fzf pane-hold (brew unlink test), scenario seeding skip — all
verified 2026-07-22 with screenshots in-session.

## Important Discoveries

- **Claude Code refuses `--session-id` reuse** → scenario seeding with
  deterministic UUIDs broke permanently after first success (identity is
  never sandboxed, so transcripts persist in real `~/.claude`). Fixed:
  `seed_needed()` wraps `spawn_mode` — resume-or-create, dev.rs.
- **`wasm_path()`'s unversioned fallback could mask the embedded wasm** —
  run_setup now extracts FIRST (write-if-absent on the versioned name).
- **Upgrades never re-ran setup** (only fired when config.kdl absent) → new
  CLI would run old bar forever. `needs_version_refresh()` in launch now
  auto-runs idempotent setup when the release binary's versioned wasm is
  missing.
- **Bare `clave` was baked into generated config/hooks** — broken for a
  single-file `./clave` install. Release builds (embedded wasm present) now
  bake canonicalized `current_exe()`; dev builds keep bare `clave` on
  purpose (sandbox PATH resolution).
- **CLAVE_*_BIN overrides are deliberately not existence-checked** (fail
  loudly at exec, not silently ignored) — don't "fix" this.
- **1Password commit signing fails in subagent/non-interactive contexts**
  until the user unlocks; leave changes staged and ask rather than
  bypassing with --no-gpg-sign.
- **The fugu-review Workflow's consolidator lane returned placeholder junk**
  (`"summary":"test"`); the raw_reviews were fine — verify findings by hand
  in that case. Haiku's "zellij-tile pin relaxed" was a misread of an
  unchanged context line (but main HAS since pinned `=0.44.3`; kept in the
  merge).
- Zellij 0.44.3 (pinned/tested) IS the latest upstream release as of
  2026-07-22 — no version gap.
- Superpowers skills (`superpowers:*`) failed to load via the Skill tool
  this session ("Unknown skill") though listed; project-local skills loaded
  fine. Worked around by following the patterns manually.

## Next Steps

1. Push the branch, open the PR to main (CodeRabbit reviews it; fugu was
   already run locally — 7 confirmed findings all fixed in commits
   `8e96b17`/`f6dda20`/`8bb7868`).
2. File 5 deferred issues (NOT yet filed): Nerd-Font/separator glyph check;
   Homebrew tap on demand; first-cut CI validation checklist (release.yml
   run + `gh attestation verify` + scp-to-linux smoke); nvm older-node
   discovery miss; concurrent first-run settings.json RMW race. Label per
   CONTRIBUTING (`cli`, `harness`, `upstream-watch`, `good-first-issue`
   where apt).
3. After merge: sub-project 2 — README restructure + VHS demo GIF
   (brainstorm happened in-session 2026-07-21 but NO spec written; key
   decisions: GIF over text mockups because GitHub strips ANSI color, VHS
   .tape for reproducibility, About-line already updated on GitHub).
4. First release cut validates the cargo-dist pipeline end-to-end (CI was
   config-validated with `dist plan` only).

Where work stopped — verbatim from the user's last messages: "I did the
just dev-install and clave doctor from the worktree, things look good."
then "/handoff push, pr".

## Context to Preserve

- Maintainer signs commits (1Password); blanket commit approval was granted
  FOR THIS BRANCH ONLY this session — a new session must re-ask.
- Zellij session lifecycle is the human's; sanctioned agent mutations are
  sandbox-only (see CLAUDE.md / TESTING.md).
- The dev surface (`~/.cargo/bin/clave` + sandbox data dir) is a shared
  slot — the user runs ANOTHER dev session on a different branch; its next
  live test needs its own `just dev-install`. A per-worktree sandbox was
  discussed and deliberately NOT built (not yet worth it).
- Hook slot in `~/.claude/settings.json` currently points at the dev
  binary (accepted policy 2026-07-20; next `just release` heals).
- User prefs: extreme concision, signal over noise; alliteration enjoyed
  (repo About line: "…with cohesive choreography, not chaos").

## Restart Hint

Tree clean, gates green. If PR not yet open: push + `gh pr create` is the
only remaining mechanical step, then file the 5 issues. Safe to /clear.
