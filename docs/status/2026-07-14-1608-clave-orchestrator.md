# Status — clave orchestrator (Task 9 live validation, mid-C5; ordering redesign UNCOMMITTED, one known bug to fix first)

_2026-07-14 16:08 · repo github.com/olliegilbey/clave · branch `main` · large UNCOMMITTED working tree (see Current State)_

Predecessor: @docs/status/2026-07-06-1333-clave-orchestrator.md (C1/C2 era; read
only for pre-C3 detail).

## Task Overview
Build **clave**: vertical dynamic tab bar for a dedicated Zellij session (WASM
plugin `clave-bar` + `clave` CLI). We are in **Task 9: human-in-the-loop live
validation** (C1–C10). C1/C2 PASS, C3 PASS (2 fixes, committed `c26967c`).
C4/C5 triggered a cascade of live findings that culminated in a USER-RATIFIED
ORDERING REDESIGN (the "Claude-desktop model") — implemented, live-tested
round 5, and failed on ONE known-cause bug (timeline divergence) whose fix is
designed but NOT implemented. That fix is the immediate next step.

## Reference Docs
- **Validation log** `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` (untracked)
  — THE working doc. C3–C5 findings sections hold the complete diagnosis
  chain, verdicts, and watch-items. Read C5's five rounds before anything.
- **Spec** `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` —
  §6.5 status semantics + idle-red discriminator; §6.6 "Order = last USER
  COMMITMENT" bullet, Nav bullet (rounds 1–3), Instances bullet (zellij event
  delivery model). All revised in the working tree — read the DIFF vs HEAD.
- **SDD ledger** `.superpowers/sdd/progress.md` — per-task history; Task 9
  checkpoint notes at the end are stale relative to this file.

## Current State
- **Committed** (HEAD `c26967c`): C3 unread-clear fixes (clear-on-delivery in
  the bar + `apply_focus` seq-bump+push).
- **UNCOMMITTED working tree** (6 files, +594/−155): the entire post-C3 body
  of work, all validated pieces intermixed with the one known bug:
  - `crates/clave-bar/src/model.rs` — timeline ordering (unix-seconds
    `timeline`/`timeline_panes` maps, `sort_key` = birth ∨ agent
    last_interacted ∨ pane touches, render-time joins), beacon/executor nav,
    `set_selectable`-era click/nav (SwitchTab+AnnounceVisit), tests rewritten.
  - `crates/clave-bar/src/main.rs` — `set_selectable(false)`, executor-gated
    clave-nav, beacon announce + once-ever birth-touch (optimistic ts=0
    mark), clave-touch/touch-pane pipe arms, TEMP `TRACE` eprintlns
    (nav/beacon/touch/birth — REMOVE BEFORE COMMIT).
  - `crates/clave/src/hook.rs` — §6.5 idle-red discriminator
    (`status_for_event(event, msg, current)`: "waiting for your input" →
    needs_you ONLY if current==Working), push_touch/push_touch_pane relays.
  - `crates/clave/src/main.rs` — `clave touch <tab_id>` / `touch-pane
    <pane_id>` commands (host-stamped broadcasts).
  - `crates/clave/src/setup.rs` — binds: Alt+j/Down + Alt+k/Up →
    clave-nav dir pipes (display-walk); **Alt+o → native ToggleTab**.
  - Spec edits per above.
- **Machine**: all artifacts INSTALLED (wasm in ~/.local/share/clave/, clave
  via cargo install, config regenerated + `zellij setup --check` passed).
  Gates green: 39 tests, clippy clean on clave-bar (pre-existing clave lints
  parked for Task 10: lsview.rs:14, store.rs:109-ish sort_by_key ×2,
  suspicious_open_options, field_reassign in tests).
- Untracked: SUBSYSTEM-VALIDATION.md, old status files, `.claude/`.

## Important Discoveries
(Master finding first; everything else is its corollaries. Full text in
SUBSYSTEM-VALIDATION.md + spec §6.6 Instances bullet.)
1. **Zellij delivers `TabUpdate`/`PaneUpdate` ONLY to the active tab's plugin
   instance.** Hidden instances are event-starved: stale active flags, stale
   TAB SETS (new tabs invisible to them), no observable transitions. ONLY
   pipes and store-snapshot pushes broadcast reliably (with backpressure
   through plugin load; each CLI pipe also delivers one empty EOF message per
   instance — benign, dropped).
2. **Ordering design (USER-RATIFIED, "Claude-desktop model")**: rows sort by
   unix-seconds of last USER COMMITMENT — agent prompts (store
   `last_interacted`, spawn-seeded, bumped on UserPromptSubmit) ∨ tab birth ∨
   (parked) shell-command touches. **Focus NEVER reorders.** Walking
   (Alt+↓/↑) steps the DISPLAYED list, executor-gated; Alt+1..9 = row jumps;
   Alt+o = native ToggleTab (the old Alt+2-as-alt-tab died by design);
   clicks jump. Idle-red: notification "waiting for your input" reds an
   agent ONLY while `working` (blocked mid-turn); permission prompts always
   red. All semantics ratified explicitly by the user.
3. **ROUND-5 BUG (the immediate fix)**: per-instance timeline copies DIVERGE
   because birth-touch echoes are fire-and-forget pipe DELTAS — some
   instances miss some echoes (spinup congestion), each bar then sorts
   differently, and walking oscillates (trace: prev from tab 2 → 3, prev
   from 3 → 2, forever — each landing's instance computes from ITS OWN
   diverged rows). **Designed fix (user-agreed): move the tab timeline into
   the STORE** — `clave touch` does a locked RMW (`tab_timeline:
   BTreeMap<usize, u64>` new store field, seq+1) and push_snapshot;
   `AgentSnapshot` carries the map; the bar REPLACES its timeline from each
   snapshot (seq-gated full-state, the only channel that has never
   diverged). Delete the clave-touch/touch-pane pipe arms + bar-side
   max-merge maps. Park touch-pane/zshrc entirely (user declined shell
   config; plain tabs order by creation only). Clear tab_timeline on session
   recreate (launch_session) — tab_ids are session-scoped.
4. **Failed approaches (do NOT retry)**: transition-based unread clear;
   store-only apply_focus; broadcast nav where each instance computes its
   own target (raced 6 divergent SwitchTabs); walk-by-position against a
   recency display (user can't predict it); `InputReceived` for typing
   detection (fires for EVERY keystroke incl. nav keybinds → touched the
   departing tab + spawn storm → zellij server fd exhaustion panic);
   echo-dependent birth guards (re-fire loop). Also historic: go_to_tab (S2;
   but `switch_tab_to(position+1)` WORKS and is now the jump mechanism),
   nested `children` in tab templates (C1).
5. **Working mechanisms (validated live, keep)**: executor gating via the
   clave-visited beacon (exactly-one nav execution, zero timeouts, round 5
   trace-proven); `set_selectable(false)` (single-click + MoveFocus
   pass-through PASS); `switch_tab_to`; unread-clear + snapshot push (C3
   PASS); permission-red ~2s (C2 PASS); Alt+o native ToggleTab (untested but
   native). The C4 cross-bar order-agreement check is still OWED after the
   store-timeline fix.

## Next Steps
1. **Implement the store-timeline fix** (design in Discovery 3): clave-types
   `AgentSnapshot` + store `Store` gain `tab_timeline`; `apply_touch` in
   store.rs (RMW, seq+1, snapshot return) mirroring `apply_focus`;
   `clave touch` pushes; bar model: timeline replaced in apply_snapshot,
   delete touch pipes/`timeline_panes`/touch-pane command; TDD per change.
2. Rebuild+install (wasm cp + cargo install), `clave setup`, session
   recreate, **round-6 validation**: rapid Alt+↓/↑ (one row per press, NO
   oscillation, NO reorder), cross-bar order agreement, agent prompt → top,
   glyph-in-place, Alt+o, idle-red (stay-green after 90s; red only on
   pending permission). User drives; traces are in the wasm — read
   `$TMPDIR/zellij-501/zellij-log/zellij.log` (`grep "TRACE"`).
3. On PASS: remove TEMP TRACE lines, gates, **commit** (ask user; one commit:
   ordering redesign + C5 fixes + idle-red discriminator + spec §6.5/§6.6 —
   message should tell the whole arc), fill C4/C5 verdicts.
4. Resume checklist: C6 (Alt+c toggle), C7 (dump-layout liveness — note
   `command="<defunct>"` quirk seen in dump-layout, investigate there), C8
   (resume/resurrection + worktree; also the user's "delete-session
   sometimes resurrects" annoyance), C9 (hydration), C10 (hook safety), then
   Task 10 sweep (ledger minors + pre-existing clippy + final whole-branch
   review over `6f6ad5a..HEAD`).
5. Deferred/backlog: touch-pane + `clave setup --shell-hook` opt-in; label
   segment colours; emoji glyphs; first-Alt+a ghost pane; idle-red
   suppress-after-visit refinement was SUPERSEDED by the discriminator.

**Where work stopped — verbatim last exchange:**
> **User:** "Okay, row order changes on step two, should I keep going through
> the steps?"
> **Assistant:** "No — stop there, the trace answers everything. […] The root
> lesson (same one, final form): fire-and-forget pipe deltas with no
> reconciliation will always eventually diverge. The one channel that has
> never diverged all week is the store snapshot — seq-gated, full-state
> replace, self-healing by construction. […] Recommendation: I write the
> /handoff now" → **User invoked /handoff.**

## Context to Preserve
- **User prefs**: extremely concise, signal over noise; explain while doing;
  MORE code comments than normal (the why); conventional commits ending
  `Claude-Session: <session URL>`; **ask before commits** (Task 9/10) and
  before architecture decisions with multiple valid approaches — but he
  engages deeply on UX design and his instincts have been RIGHT (the
  prompt-recency model was his); never validate headless — he drives the
  live session, you read the zellij log traces.
- **Live-validation loop**: fix inline → gates → `cargo build -p clave-bar
  --release --target wasm32-wasip1 && cp target/wasm32-wasip1/release/
  clave-bar.wasm ~/.local/share/clave/ && cargo install --path crates/clave
  --locked --force` → `clave setup` if config changed (then `zellij --config
  ~/.local/share/clave/config.kdl setup --check`) → user recreates (detach →
  `zellij delete-session clave --force` → `clave`; NEVER kill-all-sessions —
  his chat session is also zellij; if resurrection persists:
  `zellij kill-session clave` first) → log finding in
  SUBSYSTEM-VALIDATION.md → commit with approval.
- **1Password SSH signing**: "failed to fill whole buffer" = locked → ask him
  to unlock; staging survives.
- Solo public repo, commit straight to main, stage explicit paths only.
- **Env**: Zellij 0.44.3 (zellij-tile/utils vendored in ~/.cargo — READ THEM
  before guessing plugin API/layout questions), Claude CLI 2.1.201, edition
  2024 (let-chains fine). Zellij log: `$TMPDIR/zellij-501/zellij-log/
  zellij.log` (shared across sessions; grep by date/time).
- Store: `~/.local/state/clave/agents.json` (jq it for status/seq/
  last_visited evidence). Generated artifacts: `~/.local/share/clave/`.

## Restart Hint
Working tree holds ~600 lines of validated-but-uncommitted work — do NOT
reset/checkout anything; no stash needed (single-user machine), just
continue: read SUBSYSTEM-VALIDATION.md C5 rounds, implement Next Step 1
(store-timeline), then round-6 with the user.
