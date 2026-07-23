# Status — clave orchestrator (claude-codex launch profile: committed, PR open, live-validation PENDING)

_2026-07-23 09:54 · repo github.com/olliegilbey/clave · branch
`worktree-claude-codex-profile` · worktree
`.claude/worktrees/claude-codex-profile` · one commit ahead of main
(`8227837`) · tree clean after this handoff commits_

## What this session did

Picked up an **interrupted** SDD session. The predecessor was
`gpt-5.6-sol` (proxied through the local gateway at `127.0.0.1:8317`); its
whole fleet — main loop and 43 subagents — died at once at 20:43 on
2026-07-22 when the gateway returned `503 auth_not_found` (out of Codex
usage). The provider that serves the session is itself *named* `codex`, so
the failure looked like the feature's own wrapper shellouts failing; it was
not. The session had completed SDD Tasks 0–5 and Task 6 steps 1–3
(hermetic gates), and died **mid-Task-6-step-4**: the final review lanes
were dispatched at 20:39 and killed four minutes later.

This session resumed at exactly that point: re-verified the gates, ran the
two required review lanes, applied the surviving low/nit findings, and
committed.

## Current State

`worktree-claude-codex-profile` = `8227837`, tree clean. **One squash-ready
feature commit**, signed by the maintainer.

**Feature — the claude-codex launch profile (`clave add --codex`).** Treats
claude-codex as a launch *variant* of Claude Code, not a second provider: a
host-only `AgentRecord.claude_codex` bool routes tab spawn through the
external `claude-codex` wrapper. Clave owns no proxy secret, model mapping,
or API translation. Design: `docs/superpowers/specs/2026-07-22-claude-codex-
launch-profile-design.md`. Plan: `docs/superpowers/plans/2026-07-22-claude-
codex-launch-flag.md`.

Gates verified **by this session** in the worktree (not just the dead
session's subagent claims):

- `cargo test --workspace` — exit 0 (host + wasm + types; incl. the new
  `spawn_launch` fake-executable integration)
- `cargo build -p clave-bar --target wasm32-wasip1` — exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo fmt --all -- --check` — clean

## Review — both lanes, GO

**Lane 1 — fugu blind multi-model** (haiku/sonnet/opus + opus consolidator,
run local from the repo copy). **Lane 2 — an independent adversarial pass**
by this session (did not write the code). Both: **GO, no correctness or
safety defect survived verification.**

Honest lane accounting (a lane that did not run is not a lane that passed):

- haiku / sonnet / opus blind lanes: ran, converged; only low/nit items.
- independent adversarial pass: ran; no bug, two low notes.
- **CodeRabbit CLI lane MISFIRED** — it reviewed `CLAUDE.md` against a
  different branch and never saw the target code; its zero-findings is NOT
  signal. **Re-run CodeRabbit against this branch/diff during review.**
- Codex CLI lane: empty (unauthed locally).

Findings triaged (nothing blocked):

- *(applied)* codex preflight now shares the base preflight's
  hold-open-on-TTY treatment via `preflight_codex_wrapper`, so
  `clave add --codex` guidance survives a `close_on_exit` pane and the two
  `run_add` arms cannot drift.
- *(applied)* `tab_node`/`tab_node_bare` why-comments note the
  `claude_codex` param is baked via `spawn_args_kdl` (dense-why-comments
  standing rule).
- *(applied)* `doctor::tool_group`'s `ClaudeCodex` arm now carries a comment
  that it is intentionally never diagnosed — do not wire it into the facts
  list (would mis-report missing for every non-codex user).
- *(spec-sanctioned, no action)* missing eager wrapper aborts cold-start
  (design §Preflight: "never silently substitute ordinary Claude"); and
  preflight-then-spawn double discovery (spawn re-resolves fresh so a
  replayed resurrection survives a reinstall — design out-of-scope note).
- *(noted, no action)* `setup.rs` moved the eager-row `validate_cwd` inside
  `if !live`, so a malformed eager cwd no longer blocks a *live attach*
  (an improvement; leans on the pre-existing "attach ignores --layout"
  zellij assumption).

## Next Steps — the REQUIREMENT before merge

**LIVE-VALIDATION IS OUTSTANDING and gates the merge.** Deferred here only
because Codex usage is exhausted, so the interactive path cannot be
exercised right now. The PR is labelled `needs-live-validation`. Owner:
maintainer. Per design §Maintainer-run real smokes:

1. **The real `claude-codex` wrapper must exist as an executable** (the plan
   always had the maintainer externalize/smoke it in parallel; implementation
   used fake executables). It must forward args, preserve cwd, set the proxy
   env, and `exec "${CLAVE_CLAUDE_BIN:-claude}" "$@"`.
2. `claude-codex --version`.
3. A short proxy inference with `--no-session-persistence`.
4. **Human-driven Zellij acceptance**: new plain + `--codex` agents; dormant
   profile switch (plain↔codex resume overwrites the stored bit); live
   picker jump (profile unchanged); dead-session resurrection replays the
   stored profile; a real registered worktree row. Watch for exactly one bar
   per tab and a single loaded bar version (the #44 grep).

This touches the launch/spawn surface that broke v0.1.1 — do not tag a
release including it until the above passes.

## Where work stopped — verbatim (maintainer)

"Let's commit and leave it on a branch, we've run out of codex usage for the
time being, so we can't interactively test right now, so that'll be a todo
and requirement. Can actually PR too, and leave it with me for a later full
interactive review."

Done: committed (`8227837`, signed), this handoff added, branch pushed, PR
opened (draft, `needs-live-validation`). Left for the maintainer: the full
interactive review + the live-validation checklist above, then merge.

## Context to Preserve

- No dedicated GitHub issue tracks this feature — it was commissioned
  interactively. PR is the anchor; open an issue only if you want one.
- The predecessor's `.superpowers/sdd/` task briefs/reports live in the
  worktree (gitignored) — task-6-report.md holds the dead session's own
  dossier and its own DONE_WITH_CONCERNS.
- `~/.claude/` is READ-ONLY; `just release`/`dev-install`/`cargo install`
  remain off-limits from a working session (the #44 leak). Session lifecycle
  and all live zellij input are the maintainer's.
- Review requirement satisfied with fugu + an independent adversarial lane;
  **CodeRabbit must be re-run against this branch** (its automated lane did
  not see the code).
