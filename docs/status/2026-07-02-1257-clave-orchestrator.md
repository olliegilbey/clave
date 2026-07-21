# Status — clave orchestrator (foundation + spikes; S1 gate CLEARED)

_2026-07-02 12:57 · repo github.com/olliegilbey/clave (public) · branch `main` · tree clean_

Predecessor handoff (pre-implementation, design-locked state):
@docs/status/2026-07-01-1544-clave-orchestrator.md — read only if you need the
brainstorm/fugu-review provenance; most of it is now executed as code.

## Task Overview
Build **clave**: a terminal-native orchestrator for multiple Claude Code agents in a
dedicated Zellij session; one agent = one Zellij tab running the real Claude TUI; a
first-party WASM plugin (`clave-bar`) renders a left sidebar (repo-grouped,
recency-sorted, colour-coded status glyph) from clave's **pushed** model.

**This thread:** finalized the foundation+spikes plan, then executed it via
**superpowers:subagent-driven-development**. Completed Tasks 1–5 (foundation TDD +
spikes S0/S0b/S1). **S1 — THE architecture gate — PASSED.** Remaining in this plan:
**Task 6 (spike S2)**. Then the subsystem plans.

Phase success criteria: all four gating spikes (S0, S0b, S1, S2) pass, proving the
idempotency join key and the plugin architecture *before* building subsystems.

## Reference Docs
- **Canonical spec** `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md`:
  - §2 invariants (~:67–105) — #3 UUID join key, #5 idempotent spawn, #11 bar renders from pushed model, #12 session isolation.
  - §4 verified knowledge base (~:129–229) — **now includes the canonicalize-cwd finding (~:156–166)**. Don't re-research.
  - §6 subsystem specs (~:274–438) — all Decided; the NEXT plan builds these.
  - §9 spike plan (~:492–540) — S0/S0b/S1 done; S2 next; S3–S6 belong with their subsystems.
- **Foundation+spikes plan** `docs/superpowers/plans/2026-07-01-clave-foundation-and-spikes.md`:
  - Global Constraints (~:13–29) — bind every task.
  - **Task 6 = spike S2** (near end of file) — the immediate next work. Use `scripts/task-brief … 6` to extract it; don't eyeball line numbers.
- **Spike findings**: `docs/superpowers/spikes/S0-S0b.md` (canonicalize), `docs/superpowers/spikes/S1.md` (PASS + the permission mechanism, in detail).
- **SDD progress ledger** `.superpowers/sdd/progress.md` — **READ THIS FIRST.** Gitignored scratch (won't show in `git status`, still on disk). Durable per-task record + carried-forward decisions.

## Current State
Tree clean, all committed to `main`; only `.claude/` untracked (intentional). Verify with `git status` / `git log`.

Commits this thread (newest→oldest): `0344d12 0a83a26`… full list:
`0344d12`(S1 PASS) `aef6b31`(Cargo.lock) `433f666`(S1 plugin) `15c5efa`(S0b findings)
`0a83a26`(S0 guard) `18400b7`(S0 harness) `30e2c37`(munge) `8e85a09`(types)
`f02119b`(plan fold) `1b413c5`(workspace) `bb08de2`(gitignore) `c84fbad`(plan final).

Done + reviewed:
- **Plan finalized** (`c84fbad`) — writing-plans pass added: execution-mode note (spikes are human-in-the-loop), S2 `is_plugin` pane-id filter, register_plugin!/fn main branch, render-form fallback, non-ASCII munge caveat.
- **Task 1** (`1b413c5`) — 3-crate workspace. Review ✅.
- **Task 2** (`8e85a09`) — clave-types schema (Status/Agent/AgentSnapshot/Register), 4 tests. Review ✅.
- **Task 3** (`30e2c37`) — munge_cwd join key, 3 tests. Review ✅.
- **Task 4 / S0+S0b** (`18400b7`,`0a83a26`,`15c5efa`) — ran real Claude. **S0 PASS** (fresh `--session-id -p` creates jsonl; `-p` persists). Pre-existing uuid = **hard error** (exit 1, "already in use"). **S0b: munge rule correct, but must canonicalize cwd first** (see Discoveries).
- **Task 5 / S1** (`433f666`,`aef6b31`,`0344d12`) — **PASS.** Non-focused repaint via `zellij pipe`, seq-gating, no focus theft, raw ANSI renders clean, binary-crate wasm loads.

Key files: `crates/clave/src/{main.rs,lib.rs,munge.rs}` + `examples/munge.rs`; `crates/clave-types/src/lib.rs`; `crates/clave-bar/src/main.rs` (ZellijPlugin: `pipe()` consumes `clave-status` AgentSnapshot with seq-gating, `render()` prints colored glyph, NO `fn main`); `justfile`; workspace `Cargo.toml` (resolver 3, edition 2024, default-members excludes clave-bar). Spike artifacts: `spikes/s0-create-and-munge.sh`, `spikes/layouts/s1.kdl`, `spikes/s1-msgs/*.json`.

## Important Discoveries
(Ordered by cost to re-learn.)

1. **Zellij plugin PERMISSIONS gate everything — S1's big wall (many failed rounds).** A plugin needs `ReadCliPipes` **granted** or `zellij pipe` times out server-side (`Action CliPipe did not complete within 1s`) and nothing renders. The interactive permission prompt is effectively **unusable** in a narrow/unfocused bar pane (never surfaced answerably; tried focus, mouse, wider pane — all failed). **SOLVED by pre-writing** `~/Library/Caches/org.Zellij-Contributors.Zellij/permissions.kdl`. Format (from zellij v0.44.3 `zellij-utils/src/kdl/mod.rs` `PermissionCache::{to_string,from_string}`, ~line 5456): each plugin = a KDL node keyed by its location string, children = `PermissionType` variant names (strum `Display` = variant name). I granted `ReadApplicationState` + `ChangeApplicationState` + `ReadCliPipes` to BOTH `"file:<abs-wasm-path>"` and `"<abs-wasm-path>"` key forms. **This file already exists & works — S2 will NOT re-prompt** (ChangeApplicationState for `go_to_tab` is pre-granted). **PRODUCT ACTION ITEM:** `clave setup` (§6.5/§6.8/§7) must grant clave-bar's permissions (write/merge permissions.kdl, or grant on first launch); don't rely on the user finding the prompt.

2. **munge_cwd must be fed the CANONICAL cwd (S0b).** Claude munges the physical `getcwd()` path (macOS `/var`→`/private/var`, `/tmp`→`/private/tmp`). The char-rule `s/[^A-Za-z0-9]/-/g` is correct & verified on disk (worktree `--` included), but callers (`clave spawn`/`add`) MUST `std::fs::canonicalize` the cwd BEFORE `munge_cwd`, else the join key misses the jsonl and spawn hits "Session ID already in use". In spec §4 + munge.rs doc + ledger. munge_cwd itself unchanged.

3. **`register_plugin!` supplies its own `main` on zellij-tile 0.44** — never add an explicit `fn main()` to clave-bar (E0428). Plan Tasks 5/6 already patched (`f02119b`); current clave-bar/main.rs has a NOTE.

4. **Raw ANSI SGR works in plugin `render()`** — no need for zellij `#[fg]`/`Text` builder. Binary-crate wasm loads (no cdylib). zellij-tile 0.44 API matched the plan verbatim (no deltas).

5. **Zellij CLI gotchas (for S2 + any interactive spike):**
   - New named session + layout from a fresh terminal: `zellij -s <name> -n "$PWD/spikes/layouts/<f>.kdl"`. Do NOT use `zellij --session <name> --layout <f>` — with `--layout`, `--session` means "add tabs to the EXISTING session <name>" → errors if absent.
   - `zellij pipe --name <n> < file.json` (STDIN) avoids inline-`-- '<json>'` shell-quote/paste hangs. `zellij pipe` may not return to the prompt (bidirectional; `Ctrl-C` to reclaim) — the plugin effect happens regardless.
   - Zellij logs: `$TMPDIR/zellij-<uid>/zellij-log/zellij.log` — invaluable for plugin/permission/pipe debugging.
   - `default_tab_template` mixed with bare top-level panes = KDL parse error; keep test layouts minimal.
   - Claude Code runs INSIDE the user's Zellij `main` session, so my Bash `$ZELLIJ_SESSION_NAME=main` reflects Claude's env, NOT the user's other terminal windows.

6. **`claude -p` billing** — suspected pay-per-token API vs subscription; **no credits observed** on the S0 run → likely subscription, treat as UNCONFIRMED. Moot for the product (never uses `-p`; spawn launches the interactive TUI). S0 harness pins `--model haiku` as a cheap default.

## Next Steps
1. **Resume SDD — `cat .superpowers/sdd/progress.md` FIRST.** Trust the ledger + `git log`; do NOT re-dispatch Tasks 1–5.
2. **Execute Task 6 (spike S2)** with subagent-driven-development, author-only + interactive split (same pattern as S1):
   - `SKILL=$(…)/superpowers/6.1.0/skills/subagent-driven-development; "$SKILL/scripts/task-brief" docs/superpowers/plans/2026-07-01-clave-foundation-and-spikes.md 6`.
   - Dispatch an **author-only** subagent (sonnet): extend `crates/clave-bar/src/main.rs` per Task 6 — add `uuid_to_pane` + `pane_to_tab` maps, `PaneUpdate` subscription, `clave-register`/`clave-nav` pipe handlers, `go_to_tab`; **filter `!p.is_plugin` when building pane_to_tab** (terminal vs plugin pane-id spaces differ — plan already has this); **do NOT add `fn main`**. Create `spikes/s2-register.sh` + `spikes/layouts/s2.kdl`; build wasm; run host `cargo build`/`test`. Do NOT launch Zellij. Author `docs/superpowers/spikes/S2.md` as a template (verdict PENDING). Stage EXPLICIT paths (+ Cargo.lock if deps change; leave `.claude/` untracked). Commit with your own `Claude-Session:` trailer. Base = current HEAD (`0344d12`).
   - Review the plugin diff (task reviewer, sonnet), then **user drives** interactive validation (permissions already granted): confirm `$ZELLIJ_PANE_ID` exported; two panes self-register; `zellij pipe --name clave-nav -- '{"uuid":"u2"}'` jumps focus to that tab; verify after tab reorder/close. Fill S2.md.
   - Watch: `go_to_tab` indexing (0- vs 1-based; plan has a `+1` to confirm).
3. **After S2:** write the SUBSYSTEM plan (fresh `/superpowers:writing-plans`) in dependency order — `clave spawn` (§6.1, uses canonicalize+munge_cwd) → store+`ls` (§6.2) → `hook`+status (§6.5) → full `clave-bar` bar (§6.6) → `add` (§6.3) → naming (§6.4) → archiving (§6.7) → session/config+keybinds (§6.8, **include the permissions.kdl setup action item**). If S2 FAILS, use the plan's fallback (register-while-active / match on cwd|title) — S2 does NOT gate the architecture (S1 already did), so a soft fail is acceptable.
4. **Deferred Minors** (in ledger, for the final whole-branch review): Task 1 Cargo.toml comment density; Task 2 partial deserialize test coverage. (Task 3 byte→char already fixed.)

**Where work stopped — verbatim last exchange:**
> **User:** "yeah, checkpoint and /handoff so that the new you with fresh context can pick up accordingly from no working memory."

(Immediately prior, I had offered: *"1. Push into S2 now … 2. Checkpoint here — I write a /handoff … Which do you want?"* — user chose checkpoint.)

## Context to Preserve
- **User prefs:** extremely concise, signal over noise; explain while doing; **MORE code comments than normal** (heavily-commented, the *why*); conventional commits; **commit messages end with `Claude-Session: https://claude.ai/code/session_<id>`** — each executing agent uses ITS OWN session URL, do not hardcode this thread's. **Ask before commits/PRs and before architecture decisions with multiple valid approaches.** Greybeard shell/dev tone.
- **Branch policy:** solo public repo → commit straight to `main` (confirmed this thread). Public repo: no secrets, no machine-specific abs paths in committed code (spike layouts with an absolute wasm path under `spikes/` are the sanctioned exception).
- **SDD staging discipline:** stage EXPLICIT paths (never `git add -A`). `.claude/settings.json` is deliberately untracked (open question: should clave ship with superpowers enabled? don't commit without asking). `.superpowers/` is gitignored scratch. When a task edits a crate's `Cargo.toml` deps, `Cargo.lock` also changes — stage it too (missed once → fixed in `aef6b31`).
- **Dual-repo (for §6.5/§6.8):** `~/.claude` is a symlink → `~/dotfiles/src/.claude`; editing `~/.claude/settings.json` edits the dotfiles source. `clave setup` should do additive/idempotent merges generically.
- **Env (verified):** Zellij 0.44.3, Claude 2.1.197, rustc 1.96.1, `wasm32-wasip1` INSTALLED, fzf/zoxide/jq present, zellij-tile 0.44.3. Pre-commit secret-scan hook (gitleaks/trufflehog/ripsecrets) runs on every commit.
- **Re-running S1** (if ever needed): `spikes/s1-msgs/*.json` + `zellij pipe --name clave-status < <file>`; `permissions.kdl` already grants it.

## Restart Hint
Tree clean, all committed, S1 gate cleared — **safe to /clear.** Resume: read this file + `.superpowers/sdd/progress.md`, then spec §4/§6 + plan Task 6, then execute Task 6 (S2) via subagent-driven-development (author-only subagent → user-driven Zellij validation; permissions already granted).
