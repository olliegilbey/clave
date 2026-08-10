# Status — land PR#143 and PR#150: finish review threads, sync, merge

_2026-08-07 15:08 · CLV-MAIN session handing off at context limit. All code is
COMMITTED AND PUSHED; what remains is thread replies, one small #150 fix,
syncs, and the merges themselves._

## Task Overview

Get the two remaining wayfinder-map PRs merged: **PR#143** (`fix/139-relocated-wake`,
closes #139) and **PR#150** (`feat/149-clave-prune`, closes #149). PR#142 (#131)
is ALREADY MERGED (`960c8db` on main). Success = both PRs merged (squash,
delete branch), all review threads replied-then-resolved (never silent-resolve),
issues #139/#149 closed by the merges.

The map is issue **#115** (wayfinder, v0.1.3 fleet legibility). These merges are
prerequisites for nothing — the model.rs train (#137 → #148+#112 → #92) runs
independently — but Ollie wants the board clear.

## Current State

**PR#143** — branch `fix/139-relocated-wake`, worktree
`.claude/worktrees/agent-a6d7c93b9ca7404c1`, HEAD `ed9d6b6`, clean, pushed,
merged with main as of `960c8db`, gates green locally. Two review rounds of
code fixes are IN the branch. Five CodeRabbit threads remain UNRESOLVED —
the code work for all five is done; only replies+resolves are missing.

**PR#150** — branch `feat/149-clave-prune`, worktree
`/private/tmp/claude-501/-Users-olliegilbey-code-clave/604c27f9-e662-49fa-81d0-2054aff801df/scratchpad/prune-wt`
(NOTE: scratchpad path — if it has been cleaned up, re-create a worktree from
`origin/feat/149-clave-prune`), HEAD `1be71e6` + a main-merge, clean, pushed.
One CodeRabbit thread open with a REAL fix still to implement (below). Branch
is behind main (needs `git merge origin/main` — main moved when #142 merged).

**Merged this thread:** #142/#131 (away-summary tier), #135/#56, #128/#100,
#127/#123, #138 docs, plus earlier #119/#120/#121. `clave prune` shipped in
#150's branch; a 12-row hand prune already relieved Ollie's sidebar.

## What's Working

- **The reply-then-resolve loop is mechanical and proven**: write reply text
  referencing the fixing commit sha → `gh api -X POST
  repos/olliegilbey/clave/pulls/143/comments/<id>/replies -f body="…"` →
  fetch unresolved thread ids via GraphQL `reviewThreads(first:30){nodes{id
  isResolved comments(first:1){nodes{databaseId}}}}` (field is `author`, NOT
  `user`) → `resolveReviewThread(input:{threadId:"…"})`. Batch aliases work.
- **Both worktrees pass `just gates`** as pushed. `cargo check -p clave` is the
  fast smoke.
- **Commits sign fine** — each `git commit` pauses on Ollie's 1Password; wait,
  never `--no-gpg-sign`. One agent commit once fell back to Claude-signing
  while he was away; he wants author = him (amend + `--reset-author` fixed it).
- **CI failures on 2026-08-06 were a GitHub-wide outage** ("Service
  Unavailable" before any code ran) — `gh run rerun <id> --failed` fixed all.
  Do not debug our code off those red runs.

## Important Discoveries

- **#143's five threads and their dispositions** (all code already in
  `ed9d6b6` unless noted):
  - `3730409228` (add.rs): test now uses `Store::default()` — fixed.
  - `3730409239` (main.rs): corrupt store now reads as evidenced → fail-loud,
    no shadow-Create — fixed.
  - `3730409244` (main.rs): repoint moved into `store::apply_relocation`
    (apply_* contract: seq+snapshot only when the row exists) — fixed.
  - `3730409258` (open.rs, the real Major): recovered opens now bake the
    RELOCATED cwd (`spawn::relocated_cwd` returns `Option<String>`; gate and
    bake read the same value) — fixed.
  - `3730409267` (spawn.rs): DECLINE, do not fix. `resume_target` stays: it
    documents the degrade contract `verified_site` deliberately diverges
    from (loud-fail vs degrade, the #99 trade-off), and its tests pin that
    contrast. Reply with that reasoning, then resolve.
- **#150's open thread `3735994930`** (main.rs) — CodeRabbit is RIGHT and the
  fix is NOT yet implemented: the protect closure uses `open::open_is_live`,
  which short-circuits `true` on `tab_id.is_some()`, so dead-session binds
  are still protected whenever the dump succeeds. Fix: build the protected
  set by matching rows directly against `add::live_uuids(dump)` (match
  `r.uuid` OR `r.live_session`), drop the `open_is_live` call. `live_uuids`
  is `pub` (add.rs:26). Also swap that branch's store test from
  `serde_json::from_str("{}")` to `Store::default()` (same review pattern).
  Their further ask (clear tab_id like prune-tabs) is scope creep — decline
  that part: bind lifecycle belongs to launch/prune-tabs, not `clave prune`.
- **GraphQL trap**: `PullRequestReviewComment` has `author`, not `user`.
- **zsh trap**: `echo ===x===` breaks (`=word` expansion) — use plain markers.
- **PII**: repo is public; `grep -c "/Users/"` any body before posting (grep
  exits 1 on zero matches — don't chain with `&&`).

## Next Steps

1. **Reply + resolve the five #143 threads** (texts per dispositions above;
   fixes cite `ed9d6b6`).
2. **Implement the #150 fix** (protect-set via `live_uuids`; test to
   `Store::default()`), `git merge origin/main`, `just gates`, commit
   (1Password), push, reply citing the sha, resolve `3735994930`.
3. **Wait for checks green** on both PRs (CodeRabbit may post a THIRD round —
   verify claims against code, fix-or-decline with reasons, same loop).
4. **Merge both**: `gh pr merge <n> -R olliegilbey/clave --squash
   --delete-branch` (auto-merge is disabled repo-wide; merge manually after
   checks). This closes #139 and #149.
5. Clean up: remove the two worktrees after merge; `git fetch --prune`.

Where work stopped — Ollie, verbatim:

> still seing unresolved coderabbit comments, and branches out of date.

and the instruction before that:

> also on 150. Let's get these in and merged by fixing the last bits

Endorsement worth carrying — on the prune stopgap:

> Cool, worked properly, I now have some space in the sidebar, and it hot
> reloaded immediately, clean.

## Context to Preserve

- **Never touch Ollie's main checkout** — it is on `s7-context-battery` with
  the #62 session's uncommitted work. All PR work happens in the worktrees.
- **Never run zellij against his session; launching/killing sessions is his;
  never `just release`; never write `~/.local/share/clave/`.** Reading
  `~/.claude` is fine, writing banned. `git add -A` banned — explicit paths.
- **FOOTGUNS (new this thread)**: `CLAVE_STATE_DIR` sandboxes the store, NOT
  the session — snapshot-pushing commands run from inside the live session
  pipe sandbox state into the real bars. Strip zellij env for e2e runs.
- Review bots: fix-or-decline-with-reason, reply BEFORE resolving, expect
  multiple rounds. CodeRabbit "pass / rate limited" = reviewed nothing.
- Board after these merges: #137 (Ollie's `seek-storm-137` worktree is staged,
  zero commits), then #148+#112 designed together, then #92 — one merge train
  on model.rs. #62/#105 live in his tree. #57 unblocked. #114 needs only his
  vertical-budget call. Then the release cut (#148 is release-blocking).
- The #100F row (`bf865eb9…`) was repointed to the main repo after its
  worktree was deleted — third hand surgery; PR#143 merging+releasing ends
  that class.

## Restart Hint

Both branches committed & pushed, worktrees clean — safe to /clear. Start at
step 1 (replies are pure `gh` calls, no builds needed).
