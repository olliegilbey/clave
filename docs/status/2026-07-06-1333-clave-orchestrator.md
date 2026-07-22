# Status — clave orchestrator (SUBSYSTEMS BUILT + COMMITTED; mid-Task-9 live validation)

_2026-07-06 13:33 · repo github.com/olliegilbey/clave (public) · branch `main` · tree clean (code)_

Predecessor: @docs/status/2026-07-03-1344-clave-orchestrator.md (foundation+spikes
history; read only for pre-subsystem detail).

## Task Overview
Build **clave**: vertical dynamic tabs for a dedicated Zellij session — the bar
(left, 26 cols, WASM plugin `clave-bar`) lists ALL tabs (Claude agents or plain
terminals) in interaction-recency order, decorates agent rows with live status
glyphs, renames real tabs from session content, click/Alt-key nav. Design was
REFRAMED 2026-07-03 (rows = zellij truth via TabUpdate; order = recency;
decoration = pushed snapshots) — spec revised accordingly, then a 10-task plan
was written and executed via subagent-driven development.

**All 8 code tasks are COMMITTED and reviewed** (`68237a3..80f1335`). We are
**mid-Task-9**: human-in-the-loop live validation, checkpoint C2 of C10.

## Reference Docs
- **SDD ledger** `.superpowers/sdd/progress.md` — READ FIRST (gitignored, on
  disk). Per-task commits/reviews, deferred minors, Task 9 checkpoint section
  at the end (execution order, model policy, carry-forwards).
- **Validation log** `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md`
  (UNTRACKED until Task 9 commits it) — the C1–C10 checklist with verdicts +
  findings so far. This is the working doc for Task 9.
- **Plan** `docs/superpowers/plans/2026-07-03-clave-vertical-tabs-subsystems.md`
  — Task 9 (~line 2280+) and Task 10 (~line 2350+) are what remain; Global
  Constraints (~:17–32) still bind.
- **Canonical spec** `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md`
  (rev 2026-07-03) — §6.5 status semantics, §6.6 bar, §9 revision banner.

## Current State
- Commits (newest→oldest): `80f1335` C1/C2 generated-KDL fixes · `7f9a2de` add
  flow (+resume-clobber fix) · `f75455c` setup/launcher (+serde_json
  preserve_order) · `0ea2dc2` clave-bar rewrite · `2437ec4` hook · `2a7b54a`
  spawn · `6a80dbb` ls/snapshot/focus · `18164cf` store · `68237a3` types.
  All task-reviewed (opus), no open Critical/Important.
- **Machine state:** `clave` installed via `cargo install` (release);
  `clave-bar.wasm` + generated `config.kdl`/`layout.kdl` in
  `~/.local/share/clave/`; hooks merged into `~/.claude/settings.json` (4
  events, additive, order preserved); permissions.kdl seeded (4-perm set, both
  key forms). A live `clave` zellij session exists on the user's machine with
  ~7 tabs incl. one real agent (uuid in store; `clave ls` shows it).
- Untracked: SUBSYSTEM-VALIDATION.md (commits with Task 9), `.claude/`
  (intentional), old status files.

## Important Discoveries
(Full detail in SUBSYSTEM-VALIDATION.md findings + ledger. Cost-ordered:)
1. **Generated-KDL gotchas (all fixed, `80f1335`):** (a) `MessagePlugin "…" { … }`
   inside a bind needs a trailing `;` after the child block; (b) zellij stacks
   layout siblings horizontally — a left bar needs a vertical split; (c) in
   `default_tab_template`, `children` must be a DIRECT child and
   `split_direction="vertical"` goes ON the template node — the empty/new-tab
   fill path (zellij-utils 0.44.3 `kdl_layout_parser.rs:1748`) does NOT recurse
   to nested `external_children_index` (nested form = tabs with no terminal).
   `add.rs::tab_layout`'s nested CONCRETE panes (no `children`) are fine.
   Pre-flight `zellij --config <cfg> setup --check` catches (a)-class bugs.
2. **C1 PASS, C2 near-PASS:** picker→new→Claude TUI, uuid→pane→tab join,
   amber/green glyphs, first-prompt rename all work live. Permission-prompt
   red is the ONLY untested C2 bit.
3. **Idle-notification red is BY DESIGN (user ratified):** Claude fires
   "waiting for your input" ~60s after a turn → needs_you. Confirms the §6.5
   substring match. Backlog: self-explanatory glyphs (emoji?) later.
4. **Nav ping-pong (open design item):** interaction-recency + display-order
   nav means jumping to row 2 promotes it to row 1 → repeated Alt+j toggles
   two tabs (alt-tab behavior, can't walk deeper; key-release detection is
   impossible in zellij). Candidates: nav jumps don't bump recency, or Alt+j/k
   walk tab order. User: "we can work on that in a bit."
5. **UX backlog (user):** colour-coded label segments in the bar (needs bar to
   render agent labels from SNAPSHOT segments instead of TabInfo.name — §6.6
   render enhancement); ghost floating pane on FIRST Alt+a per session
   (cosmetic, park).
6. Earlier (still relevant): resume path must PRESERVE store rows
   (merge_resume_record, worktree bite-point → C8); Task 6 get_plugin_ids-in-
   load risk → if renames/MarkRead never fire, lazy-call fallback (watch C2/C9);
   pre-existing clippy warnings lsview.rs:14 + store.rs:109 → Task 10 gate.

**Failed approaches (do NOT retry):** nested `children` under a wrapper pane in
default_tab_template; `go_to_tab` (S2 dead end — nav is focus_pane_with_id);
answering zellij permission prompts in the bar pane (pre-seed only).

## Next Steps
(Working doc: SUBSYSTEM-VALIDATION.md — fill verdicts as you go; fixes follow
the C1 pattern: fix inline, gates, `cargo install --path crates/clave --locked
--force && clave setup`, user recreates/re-tests, log finding, commit with user
approval.)
1. **C2 finish:** trigger a real permission prompt in the agent → red glyph.
2. **C3:** focus the green agent tab → dims immediately, `clave ls` agrees,
   exactly one `clave focus` in the zellij log (`$TMPDIR/zellij-*/zellij-log/`).
3. **C4–C7** per the checklist (recency+plain tabs; click nav + the
   `switch_tab_to` scratch-branch attempt; Alt+c hide_self toggle; dump-layout
   liveness + Alt+a-jump-no-duplicate).
4. **C8** resume + resurrection incl. a WORKTREE agent; **C9** hydration;
   **C10** hook safety timing.
5. Commit SUBSYSTEM-VALIDATION.md with verdicts; reconcile any mechanism
   deltas into spec §4/§6 in the same commit.
6. **Task 10:** minors sweep (ledger list; incl. the two pre-existing clippy
   warnings), full gates, final whole-branch review (most capable model) over
   `6f6ad5a..HEAD`, then superpowers:finishing-a-development-branch.
7. Then the deferred UX designs: nav ping-pong, segment colours, emoji glyphs.

**Where work stopped — verbatim last exchange:**
> **User:** "commit the fixes, then we'll do a /handoff to free up context for
> a new context window for the dev agent working"

(The fixes are committed as `80f1335`; this file is that handoff.)

## Context to Preserve
- **User prefs:** extremely concise, signal over noise; explain while doing;
  MORE code comments than normal (the why); conventional commits ending
  `Claude-Session: <own session URL>`; ask before commits (BLANKET approval
  was given for plan Tasks 1–8 only — Task 9/10 commits: ask); ask before
  architecture decisions with multiple valid approaches. Subagents = opus
  unless really trivial (user instruction, mid-plan).
- **1Password signing:** commits SSH-signed; "failed to fill whole buffer" =
  locked → ask user to unlock, staging survives.
- **Solo public repo, commit straight to main.** No machine-specific paths in
  committed code (generated files live in ~/.local/share/clave/; spikes/ is
  the sanctioned exception). Stage explicit paths, never `git add -A`.
- **SDD discipline:** fresh implementer subagent per task + task review +
  re-review after fixes; ledger every outcome in `.superpowers/sdd/progress.md`
  (survives compaction — trust it + git log over memory).
- **Dual-repo:** `~/.claude` → symlink into `~/dotfiles/src/.claude`; clave
  setup edits THROUGH it (that's intended). settings.json diff in the dotfiles
  repo includes unrelated user drift — don't attribute it to clave.
- **Env:** Zellij 0.44.3, Claude CLI 2.1.201, rustc 1.96.1, wasm32-wasip1;
  zellij-tile/zellij-utils 0.44.3 vendored in ~/.cargo (source of truth for
  plugin API + layout parser questions — read it before guessing).
- Session recreate dance after layout changes: detach → `zellij delete-session
  clave --force` → `clave` (attach --create reuses stale layouts otherwise).

## Restart Hint
Tree clean, all code committed, gates green — safe to /clear. Resume: read the
SDD ledger + SUBSYSTEM-VALIDATION.md, then continue Task 9 at C2's
permission-prompt check (user drives the live session; never validate headless).
