# Status — pid gate deleted per #180 ruling; next: land every open branch on main systematically

_Follows @docs/status/2026-08-17-1111-width-loop-fix.md (the width work this
worktree carries). Worktree `qa-182-drive-slice-1`, branch `push-197-fixes` at
`36cd22b`, tree CLEAN, gates green, mutants 16/16 on the changed functions._

## Task Overview

Two things, in order:

1. **DONE this session:** the #180 resume-orphan ruling. Ollie ruled (locked,
   verbatim below): nested claude cannot exist, so the PidGate is deleted —
   `CLAVE_AGENT_UUID` alone binds hooks to rows. Companion principle, also
   locked: **Claude's jsonl is the canon of conversations; clave's store is a
   disposable snapshot of panes** (now in UBIQUITOUS_LANGUAGE.md §2 "canon
   rule"). Out-of-band resume (a claude started outside `clave spawn`) is
   UNSUPPORTED BY DESIGN — recovery is in-band: close pane, Alt+Enter the row.
2. **NEXT:** get all open branches/commits/PRs pushed and merged to main,
   systematically. That is the explicit handoff instruction.

## Current State

- `36cd22b` (**LOCAL ONLY, deliberately unpushed**) — the pid-gate deletion:
  `hook.rs::resolve_row` is two map lookups (session-is-key, else
  env-uuid-is-key); `PidGate`, `CLAUDE_PID_ENV`, `AGENT_PID_ENV` and the spawn
  `.env` for it are deleted; `own_claude` now means "firing claude carries this
  row's env uuid" (computed at the `apply_hook_event` call site); the ONE
  clave-owned nested claude (`dev.rs::seed_transcript`) scrubs the var via
  `env_remove`. Docs updated: FOOTGUNS "[REVERSED 2026-08-17]" entry,
  UBIQUITOUS_LANGUAGE canon rule + transcript noun, BREAKAGE-INVENTORY S17,
  store-hooks.md S3+S17, LIVE-INTERACTION-CHECKLIST nested-claude step.
- `2d4c332` (pushed to `origin/redesign-181-width`) — tracks the width handoff.
- Sandbox `clave-test-qa-182-d-1d57` is ALIVE and now runs the NEW host binary
  (its shim symlinks `clave` → this worktree's `target/release/clave`, rebuilt
  this session — hooks are fresh processes, so no restage was needed).

## The merge train (the actual next task)

Open PRs: **#197** (`redesign-181-width`, the width/fixed-cols work — this
worktree's `push-197-fixes` is its local copy), **#200** (`qa-drive-slice-2`,
needs review), **#190** (`fix-188-alt-f`), **#65** (old codex profile —
probably stale, ask Ollie). Plus `36cd22b` needing a home.

Suggested order (proposed to Ollie, not yet confirmed):

1. **#197 first** — it is validated (§10 drive green, accepted). FOOTGUN:
   `gh pr update-branch` DIVERGES the local copy — fetch+merge before pushing.
   Expect CodeRabbit rounds: fix + reply per finding, never silent-resolve.
2. **#180 PR second** — after #197 merges, branch off fresh main, cherry-pick
   `36cd22b` (it sits on top of width commits here; a direct push would drag
   the width diff into the PR). It touches host + docs only, disjoint from the
   bar, so the pick should be clean.
3. **#190, then #200 review**, then Ollie's calls: #65 close-or-rebase,
   v0.1.3 tag + `just release` (HIS command), worktree cleanup.

Issue hygiene when #180's PR lands: comment the ruling on #180 (attribute
"Ollie's Agent Speaking:", get his go first — promised this session), and note
on #102/#103/#106 what the ruling means for each (see Discoveries).

## What's Working

- 4 gates green (`just gates`), 229 host + 176 bar tests, cargo-mutants
  16/16 caught on `resolve_row` + `apply_hook_event`. The rewritten test
  `a_rotated_session_id_resolves_via_the_panes_env_uuid` (hook.rs) encodes the
  new admission semantics — copy its shape for related work.
- **Live sandbox validation of the deletion**: sent `/clear` + prompt via
  `scripts/ct.sh write-chars "..."` / `ct.sh write 13` (Enter) to the focused
  agent tab; row `…c85c00000001` rotated to a fresh id (NOT a store key),
  rebound purely via env, live_session recorded, battery reset then climbed,
  agent replied "rotation-check-ok". The env-only path is proven end-to-end.
- **Drive tooling that works**: sandbox store at
  `~/.local/state/clave-dev-qa-182-d-1d57/state/agents.json` (jq it);
  transcripts under `~/.claude/projects/-Users-olliegilbey--local-state-…`;
  `ct.sh dump-layout` for tabs; rebuild release binary = hooks updated
  instantly. Real fleet is READ-ONLY observable: `ps -axww | grep bin/claude`,
  `ps eww <pid>` for env, `~/.local/state/clave/agents.json` + `clave.log`
  (jq/tail — never write, never run hooks against it).

## Important Discoveries

- **The daily-driver blocker is #178 (bar bind leg), NOT resume adoption.**
  Watched Ollie's busiest real session live: store tracked it perfectly through
  two same-day rotations (tokens, summary, live_session all current) but the
  row has `tab_id: null` — no bind, last_visited Aug 11 — so the bar renders
  his busiest session as a bare dormant row ("the real sessions run unbound").
  The bare re-minted rows are agents opened on top of that illusion. #178/P9
  has its own queued harness (QA-DRIVE phase 2 = PR #200's territory).
- **The clave-driven binding chain was never broken** — env survives exec and
  rotation; the old PidGate passed on today's fleet. The gate's real crime was
  fail-closed-forever on anything it did not predict (out-of-band resume,
  future process-tree changes).
- **#103's ghost is REAL, live, and cosmetic**: fleet pane 2264 runs argv
  `--resume 13f2474f` while actually holding a later conversation;
  `add.rs::live_uuid_union` cannot map a SUPERSEDED argv id, so it enters the
  picker as a phantom live entry (jump-to-nowhere). Not fixed; fix belongs
  with #94's adopt-vs-hide decision. #102: half-covered by `locate_transcript`
  exact-id search. #106 (null live_session ambiguity): orthogonal, schema
  versioning, later.
- **`--resume` does NOT rotate the session id; `/clear` does** (FOOTGUNS,
  measured). In-app resume appends to the original transcript under its own
  id. The evlog (`clave.log`) logs spawn/add/open but NOT hook accepts.
- The old hook decline-log block ("gate refused") was deleted with the gate —
  unreachable now.
- `ls` is aliased (eza) in this shell — use `/bin/ls` for `-lat` etc. The
  worktree Bash guard refuses compound/cd commands; keep commands single.

## Next Steps

1. Confirm merge-train order with Ollie (he has not yet replied to it).
2. Drive #197: update-branch via fetch+merge, ride CodeRabbit rounds, merge.
3. Cherry-pick `36cd22b` onto fresh main → PR for #180; on merge, the issue
   comments (attributed, with his go).
4. #190, #200 review, release ceremony (tag + `just release` are HIS).

Where work stopped — my closing report offer, and the handoff instruction:

> "the pid-gate commit is **not pushed** — it's #180 work and I'd give it its
> own PR after the #197 merge train, unless you'd rather fold it in."

Ollie's handoff instruction, verbatim:

> keep what's needed for us to move forward with all of this, to get all open
> branches/commits/prs pushed and merged to main, systematically.

The ruling that drove this session, verbatim (locked, do not relitigate):

> nested claude cannot exist. We've said this. / This doesn't seem kiss. You
> can simplify further, straightforward.

And his acceptance of the frame:

> Yeah, lock it in. Then let's build to this.

## Context to Preserve

- **Never touch Ollie's session or store**: no zellij against his session, no
  writes to `~/.local/state/clave/` or `~/.local/share/clave/`; launches,
  kills, `just release` are his. Sandbox driving via `scripts/ct.sh` only.
- Issue/PR comments he didn't dictate: attribute "Ollie's Agent Speaking:".
  CodeRabbit findings: fix and reply before resolving, never silent-resolve.
- Do not resurrect a pid gate or any nested-claude defence; the scrub in
  `seed_transcript` is the pattern for any future clave-owned nested claude.
- Report register: outcome first, no unglossed symbols, decision over
  mechanism, six-ish sentences after tool streams.
- Sandbox session `clave-test-qa-182-d-1d57` left ALIVE, healthy, on the new
  binary; its `/clear`ed first agent now holds a near-empty conversation
  (expected — I did that).

## Restart Hint

Tree clean, gates green, `36cd22b` LOCAL-ONLY on `push-197-fixes` — do not
lose it and do not push it onto the width PR; start at the merge train order
(Next Steps 1) and confirm with Ollie before merging anything.
