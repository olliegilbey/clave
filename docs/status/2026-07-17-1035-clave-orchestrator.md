# Status — clave orchestrator (C6+features SHIPPED & committed; next: C8 design conversation, then C8–C10)

_2026-07-17 10:35 · repo github.com/olliegilbey/clave · branch `main` · HEAD `364fd06` · working tree CLEAN (only untracked docs/status/ + .claude/)_

Predecessor: @docs/status/2026-07-16-1629-clave-orchestrator.md (C6 saga
detail, hot-reload recipe, env notes — read only if you need C6/width-seek
background; its "uncommitted round 20" is now committed).

## Task Overview
Build **clave** (vertical dynamic tab bar for a dedicated Zellij session:
WASM plugin `clave-bar` + `clave` CLI). **Task 9: live validation C1–C10.**
C1–C7 PASS and committed. Two user features (30-col bar, peek-on-nav)
shipped, live-validated ("Works brilliantly"), committed. **C8–C10 remain;
C8 needs a DESIGN CONVERSATION before any driving** (premise broken — see
Discoveries). After C10: Task 10 sweep (4 parked lints, whole-branch
review).

## Reference Docs
- `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` — THE working doc.
  - L536–545: **C8 checklist** (resume via picker + real S4 kill/relaunch,
    include a worktree agent) — verdict pending.
  - L332–337: **C8 pre-registered concern** (the design problem).
  - L547–559: C9 (hydration) + C10 (hook safety) checklists.
  - L306–505: C6 full autopsy incl. round 21 (features) — only for width
    work; lists every forbidden approach (suppress, swap layouts, …).
- Spec `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` —
  §4 S4 (resurrection premise), §6.3 (picker/add), §5 (store), §6.6 (bar).
- SDD ledger `.superpowers/sdd/progress.md` — stale re Task 9; trust the
  validation log + this file.

## Current State
- **All work committed**, tree clean:
  - `d1f3915` C6 collapse-in-place (Alt+c = pure width toggle 26⇄gutter;
    all suppress/repair machinery deleted).
  - `364fd06` 30-col bar + peek-on-nav (round 21, user-validated live).
- **Machine**: wasm `build=0716-165618` hot-reloaded into the running
  clave session; `clave` binary reinstalled; `clave setup` regenerated
  layout.kdl with `size=30`. Running session = HEAD. 59 tests green;
  clippy = only the 4 pre-existing parked lints (add.rs, store.rs ×2,
  lsview.rs — Task 10).
- Peek-on-nav mechanism (model.rs / main.rs): `clave-visited` pipe →
  `visited()` = beacon + arm `peeking` + re-arm seek; main.rs counts one
  `set_timeout(0.9)` per armed peek, last expiry → `peek_expired()` sinks.
  `width_seek` target = `collapsed && !peeking ? 4 : 30`; `toggle()`
  clears peeking. Internal beacon callers (click/nav) deliberately do NOT
  arm peeks — their AnnounceVisit echoes back as clave-visited everywhere,
  so peeks only arm where the sink timer arms (no stuck peek possible).

## Important Discoveries
1. **C8 DESIGN PROBLEM (pre-registered, round 9; drive NOTHING until
   resolved with user):** zellij serializes the LIVE pane process, so a
   resurrected tab re-runs `claude --session-id <uuid>` — but that flag
   CREATES a session; against an existing jsonl it collides. S4's premise
   (serialized layout self-resumes) is broken. The conversation must pick
   the resume mechanism (e.g. layouts re-running idempotent `clave spawn
   <uuid>` which decides create-vs-resume, vs `claude --resume`) and how
   dump-layout serialization interacts with it. Related: `live_uuids`
   parser (add.rs) assumes `claude --session-id` on its own line — C8 must
   exercise a worktree agent too (validation log L536–545).
2. C6/width lore (if any Alt+c work recurs): suppress/swap-layout/
   move_pane are all forbidden — full autopsy in validation log C6
   section. Plugin resizes emit no events; render() is the only feedback.
3. Known accepted quirk: a tab created while collapsed is born expanded
   (missed the pipe). Fix path: carry `collapsed` in store snapshots
   (backlog).
4. Hot-reload loop (PROVEN): `CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S) cargo
   build -p clave-bar --target wasm32-wasip1 --release` → cp wasm to
   `~/.local/share/clave/` → `ZELLIJ_SESSION_NAME=clave zellij action
   start-or-reload-plugin "file:$HOME/.local/share/clave/clave-bar.wasm"`.
   CLI: `cargo install --path crates/clave --locked --force`; `clave
   setup` only when config/layout generation changed.
5. **Zellij CLI safety** (memory file exists): session lifecycle is
   user-driven; ONLY the explicit-env hot-reload + read-only listing are
   sanctioned from Claude's shell. Bare `zellij` commands mutate the
   user's own session. C8's kill/relaunch steps are HIS to run.

## Next Steps
1. **C8 design conversation with user** (before touching code): resolve
   the create-collision (Discovery 1). Inputs: spec §4 S4, validation log
   L332–337 + L536–545, add.rs `live_uuids`/spawn path. Decide, update
   spec §4 in the same commit as any mechanism change, then TDD it.
2. Drive C8 live (user drives; kill-session/relaunch is his) → verdict in
   the validation log.
3. C9 (hydration) then C10 (hook safety) — checklists at L547–559; likely
   quick.
4. Then: backlog — adopt/release external `claude` sessions
   (SessionStart/End adoption); collapsed-flag in snapshots (quirk 3);
   Task 10 sweep (parked lints, whole-branch review).

**Where work stopped — verbatim last exchange:**
> **Claude:** "My recommendation: hand off pointing at the **C8 design
> conversation** as the first action — it gates both C8 itself and the
> session-adoption backlog item. Say the word and I'll run /handoff with
> that framing, or adjust if you'd rather knock out the small quirk fix
> first."
>
> **User:** "/handoff" (accepting the C8-design-first framing)

## Context to Preserve
- **User prefs**: extremely concise, signal over noise; explain while
  doing; MORE code comments than normal (the why); conventional commits
  ending `Claude-Session: <URL>`; **ask before commits** and before
  architecture decisions — his instincts have been right repeatedly
  (percent-grid, collapse-mini-mode, peek-on-nav are his). Never validate
  headless — he drives; you read logs/screenshots. Peek timeout 0.9s is
  user-tuned; don't "normalize" it.
- **Env**: Zellij 0.44.3; zellij-tile/utils vendored in
  `~/.cargo/registry/src/*/zellij-{tile,utils}-0.44.3/` — READ SOURCE
  before trusting zellij semantics (burned repeatedly); zellij-server
  fetchable raw from GitHub tag v0.44.3. Zellij log:
  `$TMPDIR/zellij-501/zellij-log/zellij.log`. Store:
  `~/.local/state/clave/agents.json`. Artifacts: `~/.local/share/clave/`.
  Claude CLI 2.1.211. Edition 2024.
- **1Password SSH signing**: "failed to fill whole buffer" = locked → ask
  him to unlock; staging survives.
- Solo public repo, commit straight to main. TDD per change (superpowers).

## Restart Hint
Tree clean, everything committed and live. Safe to /clear. Start with the
C8 design conversation (Discovery 1 + spec §4) — no code until the user
signs off on the mechanism.
