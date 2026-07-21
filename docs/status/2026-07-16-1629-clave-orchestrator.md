# Status — clave orchestrator (C6 SOLVED: collapse-in-place PASS, uncommitted; next: commit → 30-col width + peek-on-nav → C8)

_2026-07-16 16:29 · repo github.com/olliegilbey/clave · branch `main` · HEAD `c71345d` · 3 uncommitted files (see Current State)_

Predecessor: @docs/status/2026-07-15-1527-clave-orchestrator.md (rounds ≤13 era; read only for C8 background + user-pref detail).

## Task Overview
Build **clave** (vertical dynamic tab bar for a dedicated Zellij session:
WASM plugin `clave-bar` + `clave` CLI). **Task 9: live validation C1–C10.**
C1–C5 PASS (committed), C6 **PASS at last** (round 20, uncommitted), C7
fixed+validated (committed in `c71345d`). C8–C10 unstarted; C8 has a
pre-registered design problem. User has queued TWO small features before
C8 (see Next Steps).

## Reference Docs
- **Validation log** `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` —
  THE working doc. C6 section now covers rounds 8–20 including the full
  announce saga, repair saga, swap-layout post-mortem, and the round-20
  collapse-in-place PASS verdict. Read the C6 section (~lines 306–500)
  before touching Alt+c behavior — it lists every forbidden approach.
- **Spec** `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md`
  §6.6 (bar), §6.3 (picker), §5 (store) — committed state is current.
- SDD ledger `.superpowers/sdd/progress.md` — stale re Task 9; trust the
  validation log + this file.

## Current State
- **Committed** (`c71345d`): C6 rounds 14–18 (no-focus toggle via
  show_pane_with_id, bounded announce, executor-heals-all repair net) +
  C7 (double-fork register, picker live/jump). NOTE: most of that repair
  machinery is DELETED by the uncommitted round 20.
- **UNCOMMITTED (3 files, +245/−611): round 20 = C6 the way it shipped.**
  - `crates/clave-bar/src/model.rs` — collapse-in-place: `collapsed` flag,
    `toggle()` flips width target, `width_seek(own_cols)` render-fed state
    machine (seek_budget 16, learned step clamped ≤20, half-step accept,
    in-flight guard, floor-benign). Effects back to ShrinkSelf/GrowSelf.
    Constants: BAR_TARGET_COLS=26, COLLAPSED_TARGET_COLS=4.
  - `crates/clave-bar/src/main.rs` — suppress/unsuppress calls GONE, Timer
    chain GONE, executor repair gate GONE, is_executor() deleted (nav does
    its check inline in handle_pipe). render() calls width_seek ungated.
  - `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` — rounds 19–20 +
    **C6 verdict PASS**.
- **Machine**: everything INSTALLED and validated live by user (clean wasm
  `build=0716-162254` hot-reloaded, `clave` binary reinstalled, layout.kdl
  regenerated WITHOUT swap layouts). Running clave session = working tree.
  Gates green: 56 tests, clippy = only the 4 pre-existing parked lints
  (add.rs, store.rs ×2, lsview.rs — Task 10).
- User's last live report: "Okay, excellent. Works perfectly." (collapse
  gutter ~8 cols w/ glyphs + truncated names + active highlight; expand
  restores; toggle solid).

## Important Discoveries
1. **C6 FINAL ARCHITECTURE (round 20): collapse-in-place.** The bar is
   NEVER hidden/suppressed. Alt+c pipes `clave-toggle` to all instances;
   each flips `collapsed` and its render-fed `width_seek` drives ITS OWN
   pane 26⇄4 cols (zellij floors at ~8). Every instance is always visible
   ⇒ always renders ⇒ always has feedback. All tabs toggle simultaneously.
2. **Why everything else failed (do NOT revisit — validation log has the
   full autopsy):**
   - suppress/unsuppress is lossy: re-insert = fresh 50% split.
   - `suppress_pane` → `extract_pane` → `set_is_tiled_damaged()`, and
     `add_tiled_pane` only auto-relayouts when NOT damaged ⇒ swap layouts
     can NEVER restore an unsuppressed pane (round 19 — our swap layout
     parsed perfectly, verified against real zellij-utils 0.44.3 parser
     via scratch tool, and never fired). `resize_pane_with_id` and
     `resize_whole_tab` also set damaged.
   - Plugin-initiated resizes emit NO events; only the plugin's OWN pane
     re-renders. Cross-tab width healing is therefore blind dead-reckoning
     or per-visit. Executor-heals-all + timers (rounds 16–18) hit this
     ceiling: one step per tab visit.
   - `show_self()` is a focus action server-side (round 14);
     `move_pane_left` = geometry SWAP, races width resizes (round 18);
     step-learning must be clamped ≤20 (round 17, step=60 poison);
     retries must be time-paced, never event-count-paced (round 16).
3. **Round-19 swap-layout code was REVERTED from setup.rs/add.rs** (dead
   weight once suppress is gone; a pane-close relayout could snap a
   collapsed bar to 26). If ever needed: `tab` nodes in swap_tiled_layout
   merge INTO default_tab_template; bare `NewTab` binds fall back to
   session-layout swaps server-side.
4. **Zellij CLI from Claude's shell**: `zellij attach` variants injected
   clave tabs into the user's MAIN session (its bars then renamed his tabs
   via store tab-id collisions). Cleaned up live. RULE (memory file):
   session lifecycle is user-driven; only the explicit-env hot-reload +
   read-only listing are sanctioned. `clave` execs `zellij attach --create
   clave` ⇒ must be run from a NON-zellij terminal window.
5. Known accepted quirk: a tab created while collapsed is born expanded
   (missed the pipe). Fix path: carry `collapsed` in store snapshots.
6. Scratch tool `kdlcheck` (parses layouts with real zellij-utils) lives in
   this session's scratchpad — gone after cleanup; trivial to recreate
   (cargo project, zellij-utils = "=0.44.3", Layout::from_str).
7. Hot-reload loop (PROVEN, works from Claude's shell):
   `CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S) cargo build -p clave-bar --target
   wasm32-wasip1 --release` → cp wasm to `~/.local/share/clave/` →
   `ZELLIJ_SESSION_NAME=clave zellij action start-or-reload-plugin
   "file:$HOME/.local/share/clave/clave-bar.wasm"`. `clave` binary: cargo
   install --path crates/clave --locked --force; `clave setup` only if
   config/layout generation changed.

## Next Steps
1. **Commit round 20** (ask user first — he was asked, then queued features
   instead of answering). Proposed: `feat(clave): C6 collapse-in-place —
   Alt+c as width toggle, suppress/repair machinery deleted` + body from
   rounds 19–20 findings. Conventional commit ending
   `Claude-Session: <session URL>`. Stage the 3 files explicitly.
2. **User-requested features (design settled, not started):**
   a. Full width 26 → **30 cols**: `BAR_TARGET_COLS` (model.rs:61) + the
      two `size=26` templates (setup.rs:103, add.rs:78) + test literals
      (band asserts around 26/27) → then cargo install + `clave setup`.
      Existing bars self-correct to 30 on next toggle cycle; born-at-30
      needs session recreate (user's call).
   b. **Peek-on-nav**: while `collapsed`, any nav should expand the bar,
      collapsing ~1s after the last nav. Design: every nav already lands
      as a `clave-visited` pipe on every instance → `beacon()`; make
      `beacon()` arm `peeking=true` + re-arm seek and return bool "peek
      armed" when collapsed (CAREFUL: beacon() has model-internal callers
      at model.rs:371 and :427 — handle the signature change); main.rs
      re-subscribes EventType::Timer, counts pending `set_timeout(1.0)`
      per peek arm, and when the counter hits 0 calls `peek_expired()`
      (clears peeking, re-arms seek toward gutter) + repaint. width_seek
      target = `collapsed && !peeking ? 4 : 30`. toggle() clears peeking.
      Expanded bars ignore beacons. TDD the model bits.
3. Then round-21 quick re-verify (toggle, peek, nav, Alt+o) → commit → C8
   design conversation BEFORE driving it (resurrection re-runs serialized
   `claude --session-id <uuid>` = create-collision; S4 premise broken) →
   C9/C10 → Task 10 sweep (parked lints, whole-branch review).

**Where work stopped — verbatim last exchange:**
> **User:** "How easy would it be to, when using the alt navigation to
> switch tab, to show the pane at full width while navigating, and for a
> second after the most recent alt navigation before it collapses again?
> This should only happen if the bar is currently collapsed, rather than
> when the bar is full width, in which case it will just stay full width.
> Also, now that collapse to small works, can we make full width 30 columns
> instead please."
>
> **Claude:** (context limit hit — summarized state, recommended /compact,
> did not start the implementation)

## Context to Preserve
- **User prefs**: extremely concise, signal over noise; explain while
  doing; MORE code comments than normal (the why); conventional commits
  ending `Claude-Session: <URL>`; **ask before commits** and before
  architecture decisions — his instincts have been right repeatedly
  (percent-grid insight, collapse-mini-mode UX, peek-on-nav are his).
  Never validate headless — he drives; you read logs/screenshots (frame
  extraction from .mov works via Swift/AVFoundation script; no ffmpeg).
- Backlog (user): peek-on-nav is being built; adopt/release external
  `claude` sessions (SessionStart/End adoption) after C8; collapsed-flag
  in snapshots (quirk 5); Task 10 lint sweep.
- **Env**: Zellij 0.44.3; zellij-tile/utils vendored in
  `~/.cargo/registry/src/*/zellij-{tile,utils}-0.44.3/` — READ SOURCE
  before trusting any zellij semantics (burned repeatedly); zellij-server
  NOT vendored but fetchable raw from GitHub tag v0.44.3 (that's how the
  damage-flag root cause was found). Zellij log:
  `$TMPDIR/zellij-501/zellij-log/zellij.log` (shared across sessions).
  Store: `~/.local/state/clave/agents.json`. Artifacts:
  `~/.local/share/clave/`. Claude CLI 2.1.211. Edition 2024.
- **1Password SSH signing**: "failed to fill whole buffer" = locked → ask
  him to unlock; staging survives.
- Solo public repo, commit straight to main. 56 tests green; TDD per
  change (superpowers) throughout.

## Restart Hint
Tree holds validated round-20 C6 (running system = working tree) — do NOT
reset. Commit it first (with user's OK), then features 2a/2b. Read the C6
section of SUBSYSTEM-VALIDATION.md before any Alt+c work.
