# Status — clave orchestrator (C8 IMPLEMENTED+committed; live validation mid-flight; OPEN BUG: Alt+c dead in new wasm)

_2026-07-18 12:40 · repo github.com/olliegilbey/clave · branch `main` · HEAD `5cb8b17` · tree CLEAN (only untracked docs/status/ + .claude/)_

Predecessor: @docs/status/2026-07-17-1035-clave-orchestrator.md (C8 design
problem framing + C1–C7 history; its "Next Steps" are DONE).

## Task Overview
Build **clave** (vertical dynamic tab bar for a dedicated Zellij session:
WASM plugin `clave-bar` + `clave` CLI). Task 9 = live validation C1–C10.
**C8 was redesigned (spec commit 65c7e6e) and fully implemented via
subagent-driven plan execution (16 commits, d21db0a..5cb8b17)**: zellij
serialization OFF; clave-owned lazy resurrection (eager most-recent tab at
launch, dormant ◌ rows opening on 0.4s dwell / immediate click/Alt+N, new
`clave open` CLI); `clave dev` sandboxed validation harness. 85 workspace
tests green; per-task reviews + fable whole-branch review all clean.
**Live validation is mid-flight and mostly PASSING; one open bug blocks
the C8 verdict: Alt+c (collapse toggle) dead in the NEW wasm.**

## Reference Docs
- `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` — §6.3
  (`clave open`), §6.6 (dormant rows/dwell nav/cursor), §6.8
  (serialization off, dynamic launch layout), §6.9 (dev harness — REVISED
  2026-07-18: claude identity deliberately NOT sandboxed), §5 (stale
  flag), invariants #5/#11.
- `docs/superpowers/plans/2026-07-17-c8-lazy-resurrection.md` — the
  executed 11-task plan (all done; useful for interface reference only).
- `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` — C8 section: NEW
  checklist + sandbox caveats. C9 (hydration) + C10 (hook safety)
  checklists follow it. C6 section = width/collapse lore (forbidden
  approaches) — READ before touching Alt+c/width code.
- `.superpowers/sdd/progress.md` (tail) — per-task commit map, deferred
  minors list, testing-strategy summary.
- `.superpowers/sdd/testing-strategy-proposal.md` — ranked test-guardrail
  proposals (items 2–5 not yet triaged with user).

## Current State
- **Committed through `5cb8b17`** (all user-signed). Key commits:
  06480fb/7586a50 (bar dormant+dwell), b26e82f (whole-branch review fixes:
  cursor render, eager_row cwd guard, classify_timer, + justfile
  `--workspace` fix), 8f47661 (harness: claude identity un-sandboxed,
  `dev status` hang gate), 292cb71 (`clave dev launch`), 5cb8b17 (double
  bar in eager tab → `tab_node_bare`).
- **Installed & current**: `clave` CLI (cargo install) and wasm
  `build=0718-111914` at `~/.local/share/clave/` AND copied into the
  sandbox (`~/.local/state/clave-dev/data/`). Real config regenerated
  (`session_serialization false` live). Real clave session untouched,
  still running OLD wasm 0716-165618.
- **Sandbox seeded**: scenario `c8-cold-start` (3 real resumable
  sessions, uuids `00000000-0000-4000-8000-c85c0000000{1,2,3}`, recency
  60s/1h/24h). User launches with `clave dev launch` in a NON-zellij
  terminal; teardown `clave dev reset`.
- **Live C8 results so far**: cold start PASS (no ENTER gates, most-recent
  resumed focused, others dormant ◌); dwell-open PASS; click-open PASS;
  nav arrows+letters PASS; widths uniform after 5cb8b17. NOT yet run:
  walk-through-safety formal check, `c8-worktree`, `c8-stale`, second
  kill+relaunch verdict. **Alt+c FAILS (see bug below).** Verdict in
  validation log still _pending_.

## Important Discoveries
1. **OPEN BUG — Alt+c dead, NEW wasm only.** Evidence: real session (old
   wasm) Alt+c works; dev session (new wasm 0718-111914) Alt+c does
   nothing; delivery PROVEN fine (Alt+j/k nav works = same MessagePlugin
   mechanism/config/wasm path; no "ç" typed so the terminal sends Meta).
   Code structure intact (toggle arm main.rs:275, `width_seek` called in
   render() main.rs:480, `model.toggle()` model.rs:722). So it is a
   BEHAVIORAL regression from T8 (dormant rows), T9 (dwell/cursor/timer),
   or b26e82f's fixes — dormant rows exist only in the dev session, so
   dormant/peek/cursor interplay with `width_seek`'s target
   (`collapsed && !peeking`) is the prime suspect family. NEXT DEBUG STEP
   (not started): add eprintln instrumentation to
   `toggle_collapsed`/`toggle()`/`width_seek`, rebuild
   (`CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S) cargo build -p clave-bar
   --target wasm32-wasip1 --release`), `cp` to BOTH
   `~/.local/share/clave/` and `~/.local/state/clave-dev/data/`, then
   hot-reload into the SANDBOX session (sanctioned):
   `ZELLIJ_SESSION_NAME=clave-test zellij action start-or-reload-plugin
   "file:$HOME/.local/state/clave-dev/data/clave-bar.wasm"`, user presses
   Alt+c, read `$TMPDIR/zellij-501/zellij-log/zellij.log`. Do NOT reload
   the real session until the bug is understood (it's the only working
   Alt+c reference).
2. **default_tab_template wraps EXPLICIT tab nodes too** (5cb8b17): a
   bar-carrying tab node in a template layout = DOUBLE bar (two plugin
   instances in one tab, broken executor election). One-shot
   `new-tab --layout` files do NOT pass through the template → they keep
   `tab_node` (with bar); template layouts use `tab_node_bare`.
3. **`zellij action` against an absent session BLOCKS forever** (no
   error) — `dev status` gates dump-layout on `session_is_live` (8f47661).
   Same hazard is impossible in open/add (they run inside a live session).
4. **Claude identity must NOT be sandboxed** (user ruling, 8f47661):
   CLAUDE_CONFIG_DIR isolation dragged auth along ("Not logged in", then
   stale copied credentials → "OAuth expired"). Harness now sandboxes
   CLAVE state only; scenario transcripts land in real ~/.claude/projects
   c85c-tagged; `dev reset` removes exactly those. Hook events still reach
   the sandbox store via CLAVE_STATE_DIR inheritance from the claude
   parent process.
5. **"CliPipe did not complete within 1s" log errors are OLD noise**
   (exist since 07-14, present during PASSING C7 rounds) — do not chase
   them for the Alt+c bug. Also: every CLI pipe delivers one extra
   empty-payload EOF message per instance ("dropped clave-*" DEBUG lines
   = normal).
6. **Timer facts (source-verified v0.44.3)**: `Event::Timer(f64)` carries
   ELAPSED sleep seconds; dwell 0.4 vs peek 0.9 split at 0.65 via
   `classify_timer` (pure, model.rs) with late-dwell reclassification.
7. **bare `cargo test` runs only 52/85 tests** (default-members excludes
   wasm-only clave-bar; model.rs tests silently skipped) — justfile now
   uses `--workspace` (b26e82f). Always `cargo test --workspace`.
8. Sub-agent commits: 1Password signing fails when locked ("failed to
   fill whole buffer") — stage + let Ollie sign, never bypass.

## Next Steps
1. **Debug Alt+c** (Discovery 1's instrumentation plan). Root-cause, fix,
   TDD a regression test (model-level if the bug is in model state, e.g.
   a toggle/peek/cursor interaction).
2. Finish C8 checklist: walk-through-safety check, `c8-worktree`,
   `c8-stale` scenarios, second kill+relaunch → write verdict in
   SUBSYSTEM-VALIDATION.md C8 section.
3. C9 (hydration) + C10 (hook safety) — checklists in validation log;
   likely quick.
4. Triage testing-strategy items 2–5 with user (KDL real-parser
   validation recommended before C9/C10; convergence harness, proptests,
   zellij pin tripwire after).
5. Then: backlog (jsonl adoption + nav ring caps, floating helper pane,
   Task 10 sweep: 4 parked lints + whole-branch review) and the
   **PR+CodeRabbit workflow switch** (see memory
   pr-workflow-with-coderabbit — after validation "locks in").

**Where work stopped — verbatim last exchange:**
> **User:** "I can confirm that the non-dev clave session that is still
> up is working correctly with the alt-c. In our dev session, alt+c is
> not typing that character, so it must be sending it correctly. alt+j
> and alt+k work to navigate too."
>
> **Claude:** "Clean split: old wasm works, new wasm doesn't, delivery is
> proven (j/k arrive). That's a regression in the new wasm's
> toggle→collapse path." (then context ran out before instrumenting)

## Context to Preserve
- **User prefs**: extremely concise; explain while doing; MORE why-comments
  than normal; conventional commits ending
  `Claude-Session: <session URL>` on its own line; ask before commits
  UNLESS he ratifies autonomous runs; he signs via 1Password (retry when
  he says unlocked). His architecture instincts have been right repeatedly
  — ask before architecture decisions. Dwell 0.4s / peek 0.9s are
  user-tuned; never normalize. He drives ALL live validation; you read
  logs/`clave dev status`. Subagent-driven dev: sonnet floor, opus for
  complex tasks, fable only for whole-branch review.
- **Zellij CLI safety (memory file exists)**: Claude's shell is INSIDE
  his `main` session. Sanctioned: explicit-env hot-reload, read-only
  `zellij list-sessions`, and commands explicitly env-scoped to
  `clave-test`. Session lifecycle (launch/kill/delete) is ALWAYS his —
  print commands for him.
- **Env**: Zellij 0.44.3 vendored at
  `~/.cargo/registry/src/*/zellij-{tile,utils}-0.44.3/` — READ SOURCE
  before trusting semantics (burned repeatedly; ppid-priority discovery
  in zellij-server pty.rs was this session's key find). Zellij log:
  `$TMPDIR/zellij-501/zellij-log/zellij.log` (SHARED by all sessions,
  old entries linger — filter by date AND build tag). Sandbox:
  `~/.local/state/clave-dev/` (state/data/repos). Real store:
  `~/.local/state/clave/agents.json`. clave.log (evlog JSON lines) in
  each state dir. Edition 2024 (`gen` reserved → `r#gen`).
- SDD ledger `.superpowers/sdd/progress.md` is CURRENT through this
  session (unlike older notes saying it's stale).

## Restart Hint
Tree clean, all committed+signed, artifacts installed. Safe to /clear.
Start at Next Step 1 (Alt+c instrumentation); the dev session may still
be running with the seeded scenario — ask the user before reseeding.
