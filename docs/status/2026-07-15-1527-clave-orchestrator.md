# Status — clave orchestrator (Task 9 live validation: C4/C5/C7 fixed; C6 Alt+c STILL BROKEN after 3 failed announce designs; large uncommitted C6/C7 tree)

_2026-07-15 15:27 · repo github.com/olliegilbey/clave · branch `main` · HEAD `3ffd838` · UNCOMMITTED working tree (see Current State)_

Predecessor: @docs/status/2026-07-14-1608-clave-orchestrator.md (C5 store-timeline era; read only for pre-Design-B context).

## Task Overview
Build **clave**: vertical dynamic tab bar for a dedicated Zellij session (WASM
plugin `clave-bar` + `clave` CLI). **Task 9: human-in-the-loop live validation
(C1–C10)**. C1–C5 PASS and committed. C7 fixes implemented + partially
validated live. **C6 (Alt+c toggle) is the open wound**: three announce
designs failed live (one crashed the zellij server); a fourth
(bounded-announce) is implemented/installed but the user's LAST report says
multi-toggle STILL throws layout out and switches tabs — NOT yet diagnosed.
C8–C10 unstarted; C8 has a pre-registered design problem.

## Reference Docs
- **Validation log** `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md`
  (tracked as of `3ffd838`, has large uncommitted additions) — THE working
  doc. C5 rounds 5–7 (store-timeline + Design B), C6 rounds 8–13 (the
  announce saga — read ALL of it before touching Alt+c), C7 findings
  (zombie/defunct + picker redesign). C4/C5 verdicts PASS.
- **Spec** `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` —
  §5 store paragraph (tab_timeline + tab_id bind), §6.3 (picker revision:
  many agents per repo, ▶ live entries jump), §6.6 (sort key = store
  timeline ONLY; Design B; touch/bind writers). Uncommitted edits cover
  §6.3 + §5; read the DIFF vs HEAD.
- **SDD ledger** `.superpowers/sdd/progress.md` — stale re Task 9; trust the
  validation log + this file.

## Current State
- **Committed** (HEAD `3ffd838`, 2026-07-14): the whole §6.6 ordering
  redesign — store-owned `tab_timeline`, **Design B** (`AgentRecord.tab_id`
  bind via `clave bind`, hook stamps timeline[bind] atomically on
  UserPromptSubmit, bar joins glyphs/renames/unread by snapshot bind), nav
  (executor-gated display walk, Alt+1..9, clicks), idle-red discriminator,
  hot-reload build tag. C4/C5 validated PASS (rounds 6–7).
- **UNCOMMITTED (7 files, +598/−80): the C6+C7 arc**, all of 2026-07-14/15:
  - `crates/clave/src/spawn.rs` — register_pane DOUBLE-FORK (`sh -c '"$@" &'`)
    — kills the zombie that made dump-layout say `command="<defunct>"`.
  - `crates/clave/src/add.rs` — `live_uuids` parses 3 forms (`spawn`,
    `--session-id`, `--resume`); auto-jump DELETED; `ResumeCandidate.live`
    flag; picker marks live with `▶`, live pick JUMPS (clave-nav uuid pipe).
  - `crates/clave-bar/src/model.rs` — C6 repair state machine (MoveSelfLeft/
    ShrinkSelf/GrowSelf, learned step size, half-step acceptance, budget 16,
    waits for own resize to land); bounded announce (birth_announced +
    organic_pending consumed in apply_tabs); beacon() disarms organic flag.
  - `crates/clave-bar/src/main.rs` — repair wiring (PaneUpdate=move phase,
    render=width phase, `is_executor()` gate = own tab == beacon);
    `clave-organic` pipe arm; TabUpdate announce block DELETED (birth-touch
    kept); render announce DELETED.
  - `crates/clave/src/setup.rs` — Alt+o bind now
    `{ ToggleTab; MessagePlugin "file:<wasm>" { name "clave-organic"; }; }`.
  - Spec §5/§6.3 edits; validation-log rounds 8–13.
- **Machine**: everything INSTALLED (wasm w/ CLAVE_BUILD_TAG, clave via cargo
  install, `clave setup` re-run, `zellij setup --check` PASSED). Gates green:
  52 tests, clippy clean except 4 PRE-EXISTING parked lints (add.rs:112-ish,
  store.rs:122/328, lsview.rs:14 — Task 10).
- **Zellij server CRASHED once** (EMFILE, render-announce storm, round 13's
  finding); user restarted `clave` — session RESURRECTED old tabs (crash
  leaves resurrectable state; resurrected agent panes ran the serialized
  `claude --session-id` commands — pollution, see C8 concern).

## Important Discoveries
1. **THE ANNOUNCE SAGA (C6, rounds 11–13) — do NOT retry these:**
   - (a) TabUpdate-driven announce (original): hidden instances' stale sets
     ALWAYS claim own-tab-active (C3), toggle bursts deliver TabUpdates to
     all 10 instances → beacon war, ~15 CLI pipes/s for 12s + CliPipe
     timeouts. Executor-gating repair did NOT stop it (the announcer itself
     was the storm).
   - (b) Render-driven announce: render is NOT visibility-gated (every
     instance renders ≥once after load) → all instances announced →
     exponential spawn storm → **EMFILE server crash** in seconds. The
     round-10 inference "hidden panes never render" was only true in steady
     state.
   - Root law (C3 corollary, final form): **any announce where an instance
     self-diagnoses "I'm active" is poisoned during event bursts, regardless
     of gate or transport.** Each `run_command zellij pipe` also spawns a
     CLI client whose attach appears to trigger further TabUpdates
     (feedback); and each CLI pipe delivers one empty EOF msg per instance.
   - (c) CURRENT (installed, **user reports still broken — unvalidated
     diagnosis**): announces only on bounded triggers — birth (first-ever
     TabUpdate per instance) or `clave-organic` (Alt+o bind arms ONE
     announce; any incoming beacon disarms). Toggle bursts arm nothing.
2. **User's LAST live report (after fresh session on the new build): multi
   Alt+c STILL throws layout out AND switches tabs.** NOT yet root-caused.
   Prime suspects for the next session, in order:
   - `show_self(false)` may FOCUS the shown pane's tab (zellij unsuppress
     semantics) — 10 instances showing = focus jumps + layout churn. Never
     investigated. Check zellij-server unsuppress code / test with 2 tabs.
   - zellij re-INSERTS a re-shown pane as a fresh 50% split (proven, round
     8) — the layout "throw-out" part is EXPECTED until per-tab repair
     runs; repair is executor-gated + lazy per visit.
   - Resurrected-session pollution (crashed server state) may have
     contributed — retest on a CLEAN session (`zellij kill-session clave`,
     `zellij delete-session clave --force`, then `clave`).
   - Repair itself moving/resizing while toggling rapidly (armed budget 16
     per show; re-arms each show).
   - Consider FALLBACK per validation doc C6: accept hide-only/document, or
     `close_self()` + relaunch. User tolerance for quirks here is decent;
     his priority is "get things working neatly".
3. **C7 (fixed, partially validated)**: zellij serializes the LIVE pane
   process, not the baked layout command. `clave spawn`'s pre-exec
   fire-and-forget register spawn became a permanent ZOMBIE under claude
   (`ps`: one `Z+ <defunct>` child per agent) → dump-layout printed
   `command="<defunct>"` for every agent pane → liveness blind → the old
   §6.3 auto-jump never fired → its design flaw (forbids 2nd agent per
   repo) went unnoticed; resume-picking a live session opened it TWICE
   (same uuid 2 tabs; bind points at one → glyphless duplicate row).
   Fixes: double-fork register; parser matches post-exec forms; auto-jump
   deleted; ▶ live entries jump. VALIDATED live: new agent serialized as
   `claude --session-id <uuid> …`, ▶ shown, jump worked. Old (pre-fix)
   agents keep zombies until respawned.
4. **C8 PRE-REGISTERED PROBLEM**: resurrection re-runs the SERIALIZED
   command = `claude --session-id <uuid>` (a create!) — collides with the
   existing jsonl. S4's premise (re-run idempotent `clave spawn`) is
   broken by exec. Design before driving C8. (Possible directions: don't
   exec (but zellij serializes the child anyway); or clave-side re-create
   flow from the store on attach; or accept + `clave heal`.)
5. **C6 repair mechanics that DO work** (keep): learned-step resize
   (zellij steps ~5%/viewport ≈14 cols; naive shrink overshot 27→13),
   half-step acceptance band (26±7), GrowSelf recovery, wait-for-landing
   (no double-fire), render-chained width phase (zellij sends no PaneUpdate
   for the plugin's own resize; PaneUpdate-only advanced one step per tab
   VISIT), budget 16 as circuit breaker (it's what kept storms from
   EMFILE — except the render one), executor gate `is_executor()` (own tab
   == replicated beacon) — the ONLY trustworthy "on screen" check;
   `is_active_instance()` is degenerate (C3) and must never gate anything
   new.
6. Hot-reload loop (PROVEN): rebuild w/ `CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S)`
   → cp wasm → `zellij action start-or-reload-plugin "file:$HOME/.local/share/clave/clave-bar.wasm"`
   (run inside session) → grep log for `loaded v0.1.0 build=…`. Session
   recreate ONLY for store-schema/config changes. `clave` binary: cargo
   install is the reload.
7. Backlog (user-requested): adopt/release external `claude` sessions
   (manual `claude` in a tab ↔ agent row; ctrl+c → plain tab) — fits as
   SessionStart/SessionEnd hook adoption after C8. Parked: touch-pane/
   preexec shell hook.

## Next Steps
1. **Root-cause the remaining Alt+c breakage** (Discovery 2 suspects). Start
   CLEAN: kill+delete session, fresh `clave`, 2–3 tabs only, single toggle,
   read `$TMPDIR/zellij-501/zellij-log/zellij.log` (no more TRACE lines in
   wasm — re-add TEMP traces if needed; announce volume should be near-zero
   now). Specifically test whether show_self focuses/switches tabs with the
   bounded-announce build. Decide fix vs fallback (hide-only / documented
   quirk) WITH the user.
2. On C6 resolution: round-14 full re-verify (nav walk, Alt+o organic
   announce → highlight follows, Alt+c cycles), fill C6/C7 verdicts, then
   **commit the C6+C7 arc** (ask user; conventional commit ending
   `Claude-Session: <session URL>`; stage explicit paths only; the 7 files
   in Current State + status files if he wants).
3. C8 design conversation (Discovery 4) BEFORE driving it; then C8–C10
   checkpoints; then Task 10 sweep (parked clippy lints, ledger minors,
   whole-branch review over `6f6ad5a..HEAD`).

**Where work stopped — verbatim last exchange:**
> **User:** "running clave did pull in the session again, and the alt+c
> toggle multiple times still has similar breakage of throwing out the
> layout and switching tabs when only toggling.
> But yes, let's /handoff with this info too"

## Context to Preserve
- **User prefs**: extremely concise, signal over noise; explain while doing;
  MORE code comments than normal (the why); conventional commits ending
  `Claude-Session: <URL>`; **ask before commits** and before architecture
  decisions with multiple valid approaches — he engages deeply on UX and his
  instincts have been right repeatedly (prompt-recency model, multi-agent-
  per-repo, resume-should-jump were all his). Never validate headless — he
  drives; you read zellij logs. He tolerates cosmetic quirks if the outcome
  works (per-tab lazy healing accepted).
- **Live loop**: fix → gates (`cargo test --workspace`, clippy) → tagged
  wasm build + cp + `cargo install --path crates/clave --locked --force` →
  `clave setup` only if config_kdl changed (then
  `zellij --config ~/.local/share/clave/config.kdl setup --check`) →
  hot-reload or session recreate → user drives → log findings in
  SUBSYSTEM-VALIDATION.md.
- Session recreate: detach → `zellij delete-session clave --force` →
  `clave`. If resurrection pollutes: `zellij kill-session clave` first.
  NEVER kill-all-sessions (his chat session is zellij too).
- **1Password SSH signing**: "failed to fill whole buffer" = locked → ask
  him to unlock; staging survives.
- Solo public repo, commit straight to main.
- **Env**: Zellij 0.44.3 — zellij-tile/utils vendored in
  `~/.cargo/registry/src/*/zellij-{tile,utils}-0.44.3/` — READ THEM before
  guessing plugin API (zellij-server is NOT vendored; broadcast semantics of
  `pipe_message_to_plugin` with no url/destination are UNVERIFIED — do not
  bet on them). Claude CLI 2.1.209. Edition 2024 (let-chains fine).
  Store: `~/.local/state/clave/agents.json` (jq for evidence). Artifacts:
  `~/.local/share/clave/`. Zellij log: `$TMPDIR/zellij-501/zellij-log/zellij.log`
  (shared across sessions, grep by date/time; storms show as
  `dropped clave-visited pipe with empty payload` floods = CLI-pipe EOF
  echoes).
- 52 tests green at handoff; TDD per change (superpowers) throughout.

## Restart Hint
Tree holds the uncommitted C6+C7 arc — do NOT reset; gates green, artifacts
installed and AHEAD of HEAD (running system = working tree, not HEAD). Read
SUBSYSTEM-VALIDATION.md rounds 8–13 first, then Next Step 1 (clean-session
Alt+c diagnosis) with the user driving.
