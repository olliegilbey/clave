# Status — the rotation fix is live-validated; #99 must land before the tag

_2026-07-31 13:42 · branch `fix/session-id-rotation`, **PR #98 open, gates green,
286 tests, live-validated end to end**. `main` is at `67761e0` (PR #90 merged
this session). Run `git log --oneline main..HEAD` for the real commit count._

## Task Overview

**Ollie's goal: cut v0.1.2 quickly** so he can switch his daily driver onto it
and find more issues by living in it. Everything triages against that — prefer
finishing what is started over opening new fronts. Momentum beats perfection;
minor things become issues, not scope.

The one thing he named as a must-have for the next daily driver: **tabs that
don't move to the top when he interacts with them**. That is #97, and it is
fixed and validated in PR #98.

**His ruling this session, verbatim, and it is the gate on the cut:**

> we will need to fix that before the cut. That's a bad bug.

"That" is **#99** — resurrection resumes the pre-rotation conversation and
orphans everything since the last `/clear`. Confirmed live, not theorised.

## Reference Docs

- **`docs/dev/LIVE-INTERACTION-CHECKLIST.md` item 8** (search `## 8. Session-id
  rotation`) — the sandbox procedure written and RUN this session. Read this
  before re-validating anything; it records what each step proves and the three
  ways to run it and learn nothing.
- **`docs/ux/LEDGER.md`** — the authority for UX decisions, 37+ numbered. Read
  its **operating rule** (~line 8) and the **task table** at the bottom (~line
  1069), which is the only statement of what has shipped. Specs are an OUTPUT,
  never amended during a build.
- **`FOOTGUNS.md`** lines ~48-50 — the two traps this session added: session-id
  rotation, and env-before-exec being ambient authority. Both `[FIXED]` with
  their guard sites and their **live residuals** named.
- **`UBIQUITOUS_LANGUAGE.md`** ~lines 25-33 — **minted uuid vs live session id**.
  Added this session; it is the vocabulary #99 is about.
- **`docs/superpowers/specs/2026-07-22-S4-label-rename-and-live-cwd.md:929`** —
  the derived jsonl path is WITHDRAWN and MANDATORY to replace. Do not
  re-adopt it; one attempt this session did, and review caught it.

## Current State

`git status` is dirty with **things that are not mine — do not commit them**:

- `AGENTS.md`, `CLAUDE.md` modified by Ollie/another session (new directives;
  CLAUDE.md now `@OPUS.md`).
- `OPUS.md` — empty, referenced by CLAUDE.md. Untracked deliberately.
- `docs/status/2026-07-31-1142-deliberate-skill-eval.md` — another session's
  handoff. Untracked deliberately. **It was twice swept into my commits by
  `git add -A`; do not let it happen a third time.**

**PR #98, branch `fix/session-id-rotation`, 5 commits ahead of main.** Net diff
is 5 files: `hook.rs`, `main.rs`, `clave-types/lib.rs`, `FOOTGUNS.md`,
`UBIQUITOUS_LANGUAGE.md`, plus the checklist. All four gates green.

Merged this session: **PR #90** (bar width) as `67761e0`. Closed: **#91**.

## What's Working

**Build on this; it is verified, not assumed.**

- **PR #98 is live-validated end to end** and ready to merge. Ollie merges;
  `main` is protected. Five checks passed in the sandbox — env reaches the hook,
  the tail follows the live transcript, `title` is blank on a new agent (#91),
  ordering works end to end, and a nested `claude` is refused with three decline
  lines logged.
- **`PidGate` (`hook.rs`) is the identity primitive.** `CLAUDE_PID` is Claude's
  own pid, re-exported per process — **verified empirically**: a nested `claude`
  reported `66139` against its parent's `38725`. `exec` preserves the pid, so
  `clave spawn`'s `process::id()` IS the agent Claude's. Fails closed on any
  missing side. Copy this pattern; do not re-derive it.
- **`resolve_transcript` (`hook.rs`) is the trusted-path primitive.** Canonicalize
  → confine under `<claude_config_dir>/projects` → require `<session_id>.jsonl`
  → require an ABSOLUTE root → return `None` rather than a stale file when the
  row was reached by rotation. Reuse it; do not write a second path resolver.
- **The mutation-check habit is the safety net that worked.** Every new guard
  this session was verified by breaking it and watching the right test fail.
  `cp file /tmp/x.bak`, mutate with perl, `cargo test`, restore. Cheap, and it
  caught a test that asserted against a copy of its own logic.
- **The sandbox works and is safe.** `just sandbox` is YOURS to run (AGENTS.md
  says so explicitly); it self-checks that stable surfaces are untouched and
  fails closed. It does NOT need `just dev-install` — it is the documented safe
  alternative. Session launch is Ollie's.
- **Ollie is an excellent live tester** and volunteers for it. He corrects
  wrong framings precisely (he caught that focus/clicks don't reorder — only
  prompts do, per S1). Give him exact steps and one inflection point at a time.

Narrow scopes worth not widening defensively: `PidGate` defends **accidental**
inheritance, not a deliberate local caller — that is fine and documented, since
anyone who can set that env can write the store directly.

## Important Discoveries

**#97's mechanism (fixed).** Claude starts a NEW session id and transcript
whenever a pane gets a fresh conversation — **`/clear` is confirmed**; resume is
likely and NOT proven. The hook then fires with an id naming no row, took its
untracked-session fast path, and the row silently froze: no `last_interacted`,
so it never rose again. Measured at **5.9 days stale on Ollie's live tab while
he was typing in it**. Rows that DO rise (DELIBR8) are simply ones never cleared.

**#99's mechanism (confirmed, NOT fixed).** `spawn_mode` keys on
`<minted>.jsonl` existing and the exec passes `--resume <minted>`. Live result:
the resumed agent knew only the pre-`/clear` content; the post-`/clear`
transcript was frozen and orphaned; **no third transcript was created, so
`--resume <superseded-id>` does NOT re-chain** — it reopens that file and
continues it. The row's summary visibly regressed to the older `ai-title`.

**Approaches tried and REFUTED — do not retry:**

1. **#91 candidate (2), "ignore `custom-title` before the first user prompt".**
   Dead. Claude re-emits a per-turn HEADER block that rewrites the title, so
   clave's label returns on turn 2 (30× in one sampled session). Would look
   fixed on a one-prompt check. Fixed instead by dropping `--name` entirely,
   which also RESTORES `aiTitle` in `claude --resume`'s picker.
2. **A stored `live_session` field on `AgentRecord`.** Written, then deleted.
   It existed only to keep the derived jsonl path alive, which S4 forbids.
   `payload.transcript_path` dissolves rotation and relocation together and
   removes a field instead of adding one. **#99 may need to reintroduce a
   stored live id — that is now justified by measured loss, but know it was
   deliberately removed once.**
3. **Env alone as identity.** `CLAVE_AGENT_UUID` is inherited by every
   descendant, so a nested `claude` took the same fallback and would have
   written another agent's row. Store membership proves the value names *a*
   row, never *this* row. Hence `PidGate`.

**Process failures worth not repeating — all three were me asserting instead of
checking:**

- Two **overclaiming doc comments** shipped and were caught by reviewers
  ("never a misattributed write"; "keeps the ADMITTED SET unchanged"). This repo
  treats comments as authority.
- A **commit message claimed a file removal that `git add -A` had silently
  undone**. Stage explicit paths. Verify the claim before writing it down.
- A **test asserted against a copy of its own logic** (re-implemented the
  selection in a local closure), so deleting the real line left it green. I had
  named this exact risk out loud before doing it.

**Review value:** three rounds each found something real. The Fugu 5-lane run
(`Workflow` name `fugu-review`) found the ambient-authority bug with 3/5 lanes
agreeing including Codex, which never saw the brief.

## Next Steps

1. **Fix #99.** Ollie's ruling: it blocks the cut. The fix is cheap now that
   `payload.transcript_path` is trusted — the live session id is recoverable
   from the transcript filename, so `spawn_mode`/the exec can target the live
   conversation. Read the #99 thread first; it has the live evidence and the
   caveat about reintroducing a stored live-id field. Also in scope there:
   `add.rs::resume_candidates` joins jsonl stems by `stem == uuid`, so a rotated
   session shows in the picker as an unattached candidate for a live agent.
2. **Merge PR #98** (Ollie's hand — main is protected). It is ready; #99 can
   land on top rather than blocking it.
3. **Then the cut.** D28's release sequence, `just release`, switch the daily
   driver. His hands for the last two.
4. Remaining, all filed, none blocking: **#89** launch flash, **#94** adoption
   of externally-started sessions, **#95** programmatic testing planes,
   **#96** PR number in the bar, **#100** dwell confirm-before-launch.

**Where work stopped, verbatim:**

> we will need to fix that before the cut. That's a bad bug.
>
> Please write a /handoff document for yourself to pick up from when we come
> back in the fresh session.

**Endorsed framing worth keeping (his voice, earlier in the session):**

> Momentum beats perfection, things that can wait for other releases can be gh
> issues if they are minor. We want to get to something mostly useable for this
> early version, so we can cut it and get to daily driving it to find more
> issues.

And, on how to work with him:

> Also, you can ask me to check things for you...

He means it. He drove every live step this session and corrected two of my
framings accurately.

## Context to Preserve

- **Never kill, launch, or run a bare `zellij` command**; never `just release`,
  `just dev-install`, or `dev launch`; never write `~/.local/share/clave/` or
  anything under `~/.claude/`. **`just sandbox` IS yours to run.** Print the
  session-launch command; let him run it.
- **`cargo test --workspace`, always** — bare `cargo test` skips tests, exits 0.
- **GLYPH RULE:** every non-ASCII glyph in Rust source and test literals is a
  `\u{...}` escape.
- **Ollie signs every commit** — `git commit` pauses on a 1Password prompt.
  Wait. Never `--no-gpg-sign`. Prefer `git merge` over `git rebase`.
- **The repo is PUBLIC.** No home paths, transcript content, or personal data
  in code, commits, or issues. The pre-commit PII hook does not cover `gh`.
- **Fix review findings and reply before resolving. Never silent-resolve.**
  CodeRabbit reports `pass` while rate-limited (#68) — read the detail.
- **`git add -A` is banned in this repo's working tree.** It has twice swept in
  other sessions' files. Stage explicit paths.
- The sandbox may still be up: `clave-dev dev reset` tears it down (it prints
  the kill command rather than running it).
- Session inventory written to `~/clave-session-inventory-2026-07-31.md`
  (outside the repo, deliberately — the repo is public).

## Restart Hint

Tests green, branch pushed, nothing of mine uncommitted — **safe to `/clear`**.
The dirty files in `git status` belong to Ollie/another session; leave them.
Start by reading the #99 thread on GitHub, then `hook.rs::resolve_transcript`
and `spawn.rs::spawn_mode`.
