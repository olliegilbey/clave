# Status — clave orchestrator (v0.1.1 RELEASED · prod incident diagnosed + resolved · daily-driving again)

_2026-07-22 16:40 · repo github.com/olliegilbey/clave · main `ccd67fb` = tag `v0.1.1`
(pushed) · `just release` RUN, artifacts installed · tree clean except allowlisted
untracked `.claude/` + `AGENTS.md` + this handoff_

Predecessor: @docs/status/2026-07-21-1658-clave-orchestrator.md (v0.1.0 cut + the
three-agent dispatch this session picked up and finished).

## Task Overview

clave = Zellij fleet-orchestration sidebar (wasm `clave-bar` + `clave` CLI).
This session: finished the v0.1.1 milestone end-to-end — re-dispatched the two
agent lanes that had died on the spend limit, ran the full review gauntlet,
merged everything, live-validated in the sandbox, and cut the tag. Work is
issue-driven, feature-branch PRs into protected `main`, squash-merge convention.

## Current State

**Merged this session (all CI green, squash):**
- `c235311` (#25) → closes #17 — label hygiene: harness-injected prompts
  (`<task-notification>` etc.) never earn labels; table-driven test covers every
  prefix on both earn paths.
- `1526d79` (#26) → closes #23 + #6 — nav re-anchor on tab close
  (`Effect::ReanchorVisit`, executor-gated), bind-based liveness replacing the
  MCP-blind command scan, order-safe prune (**stale-ids payload**), bind
  eviction on reused ids, prune retries until the store echo clears.
- `7644fd8` (#27) → closes #4 — width-seek drift re-arm: four gates,
  drift double-confirmation, `settle_at()` pinning the drift anchor to rest.
- `83c4430` (#30) → closes #28 — unbind stock `Ctrl+g/t/o/b` + `Ctrl+q`
  (Claude-key collisions + fleet-kill guard).
- `9f121bc` (#32) — version bump to 0.1.1.
- `ccd67fb` (#42) → closes #41 — `Alt+t { NewTab; }` restoring terminal-tab
  creation that #28's Ctrl+T unbind removed.

**Tag `v0.1.1` created on `ccd67fb` and pushed.** `just release` has **NOT**
been run — it is the maintainer's command (standing rule: the only sanctioned
install from a working session is `just dev-install`). That is the immediate
next action, watched live.

**Worktrees:** the three v011-* worktrees were removed and their branches
deleted (local + remote) after merge. `v011-keybinds` still exists (branch
merged, disposable). This session's home worktree
`.claude/worktrees/issue-10-kdl-guardrail` was **flipped from `0161d99` to
detached `ccd67fb`** so the session works against current code — the path is
unchanged, so the runbook §0 resume lifeline still holds.

**Sandbox:** reseeded and validated at merged main. Config regenerated at 15:25
(carries the unbind line). NOTE: this was a **manual** env-prefixed
`clave setup` — see #31.

## Important Discoveries

1. **zellij REUSES tab_ids** — `get_new_tab_id` is `self.tabs.keys().last() + 1`
   over a `BTreeMap` (`zellij-server/src/screen.rs:1617`, v0.44.3; verified
   independently twice, once by fetching the tagged source). Closing the
   HIGHEST tab recycles its id. This makes bind/timeline pruning
   **correctness-critical**, not hygiene.
2. **Full-live-set prune payloads are order-unsafe** (CodeRabbit CLI MAJOR).
   Two fire-and-forget `clave prune-tabs` subprocesses have no arrival order;
   a "retain only these live ids" payload landing after a new tab's bind
   unbinds a LIVE agent — and `bind_effects` is `sent_binds`-guarded so it
   never re-fires → #6 double-attach via a race. **Fix: carry observed-STALE
   ids** (idempotent, commuting removals). Do not regress this.
3. **Prune emission must NOT be set-change-gated** (Codex P2). A close
   TabUpdate arriving before its PaneUpdate leaves `is_active_instance()`
   false, silently dropping the effect; a set-change gate then never retries.
   Emission is now **detection-driven** (any TabUpdate with a non-empty derived
   stale set), self-limiting via the store echo. `last_live_ids` was deleted.
4. **Announces**: the stranded re-anchor could NOT ride the ungated
   `AnnounceVisit` path — toggle bursts deliver fresh tab sets to ALL
   instances, so it would revive the round-13 beacon-war (EMFILE) class. It
   uses a distinct `ReanchorVisit` effect, executor-gated. Accepted trade
   documented: a transiently-false active check drops the reseed and nav stays
   stranded until a click. **Declined** (twice): pending-until-echo retry —
   that is per-instance persistent arming, the shape the ledger forbids.
5. **Width-seek**: `seek_last_cols` must be synced to the accepted rest width
   at every settle path, else gate B measures drift from a stale mid-flight
   emit anchor and parks off-target (Codex P2, reproduced exactly:
   30→16→6 then external 26). `settle_at()` is the single settle helper.
6. **The SimZellij harness change was proven honest** — the old model still
   fails the pinned seed under the new `drive()`, and a thrashing model trips
   the `iters < 1024` cap. Verified by a reviewer building standalone sims.
7. **External review lanes earn their keep.** CodeRabbit CLI and Codex found
   **four real defects** that two Opus agent lanes plus the principal pass all
   missed (items 2, 3, 5 above + a test-coverage gap). Keep them in the flow.
   CodeRabbit **cloud** is rate-limited (~1/hour); the **CLI** is reliable:
   `coderabbit review --committed --base main` run in the worktree. Note the
   CLI updated to 0.7.0 — `--plain` was REMOVED, plain text is now default.
8. **Every new CLI surface needs a `Cli::try_parse_from` pin** (the clap
   `ArgAction` lesson) — added for `prune-tabs`; also run one sandboxed
   end-to-end (`CLAVE_STATE_DIR=<scratch> cargo run -p clave -- …`) in a DEBUG
   build, which is where clap's debug_assert fires.
9. **Merge mechanics**: repo has auto-merge DISABLED and requires all review
   threads resolved + branch up to date. Sequence that works:
   resolve threads (GraphQL `resolveReviewThread`) → `gh pr update-branch` →
   wait for CI → `gh pr merge --squash`.
10. **Shared `~/.cargo/bin/clave` is a coordination hazard** — two agents
    cargo-installing from different worktrees silently swap the binary under
    each other (happened with the doctor-install worktree). See #31.

## Live-validation results (sandbox, maintainer-driven)

All passed or accepted:
- #23 nav after close: works, **no mouse click needed**.
- Beacon-storm watch (close then Alt+c burst): bars stayed calm.
- #6 store hygiene: watched live in the store — on `Alt+w` close, `tab_id` →
  null and the timeline entry vanished in one write; re-open produced a fresh
  bind to a new id. No dead-glyph inheritance.
- Jump-not-resume: selecting an already-open session focused its tab, no
  double-attach.
- #4 re-seek to 30 cols: quick and correct.
- Anti-thrash/far-drift: imperfect but "good enough, UX is good as is"
  (maintainer's ruling — a quick Alt+c heals it). Not worth debugging now.
- Ctrl+G/T/O/B/Q verified after #28; **Ctrl+P still enters zellij pane mode**
  (proves the unbind stayed surgical).

**Gotchas found during validation (do not re-diagnose):**
- `Ctrl+D` in an agent pane does NOT close the tab — panes run `clave spawn`
  (idempotent resurrect), so zellij holds the pane and re-runs on interaction.
  **`Alt+w` is the close path.**
- Closing a tab leaves a DORMANT ROW by design (rows are agents, not tabs);
  navigating onto it re-opens it via dwell (§6.3). Surprised the maintainer —
  filed on #24.
- A cold launch showing TWO tabs (base `clave` + most-recent agent) is the
  §6.8 eager-most-recent-tab design, not a bug.

## POST-RELEASE INCIDENT (2026-07-22 16:19–16:35) — RESOLVED, read before touching release mechanics

`just release` ran correctly, but the relaunched daily session came up with a
**duplicate sidebar and half-dead navigation**. Diagnosed from the zellij log at
`/var/folders/.../T/zellij-501/zellij-log/zellij.log` (macOS path — NOT
`~/Library/Caches`; the bar logs `clave-bar: loaded vX.Y.Z build=…` at every
load, which is the fastest version-mismatch test there is).

**Root cause, two layers:**
1. `clave-bar` shells out to **bare `clave`** (PATH-resolved) in 7 places —
   `snapshot`, `open`, `bind`, `focus`, `touch`, `prune-tabs`, `add`. config.kdl
   bakes the absolute versioned binary for KEYBINDS only; the plugin's own
   shellouts bypass it. → **#44** (fix: pass the binary path into the plugin via
   its layout `configuration` block at generation time).
2. `~/.cargo/bin/clave` held a **0.1.0** build (from `just dev-install`, and
   earlier from the doctor-install worktree's `cargo install`). So `clave open`
   ran the 0.1.0 binary, which composed tab layouts pointing at
   `clave-bar-v0.1.0.wasm`. Zellij keys plugin identity on LOCATION, so those
   tabs loaded a SECOND bar; two populations don't share beacon/pipe state →
   dead nav. → **#43** (no unversioned stable entry point; dev-install collides
   with the daily launcher name).

**Fix applied:** copied `~/.local/share/clave/bin/clave-v0.1.1` over
`~/.cargo/bin/clave`, then a clean kill+relaunch. Verified: every bar instance
since 16:34:37 reports **v0.1.1** (ids 1–6), store healthy (5 bound tabs,
consistent timeline), hooks already pointed at `clave-v0.1.1`.

**Standing hazard until #44 lands: NEVER `cargo install` / `just dev-install`
from any worktree while the maintainer is daily-driving** — it replaces the
binary the live fleet shells out to. Dev builds must not be named `clave` on
PATH.

Also found: `CliPipe did not complete within 1s` + empty-payload pipe deliveries
are **pre-existing** (present since the log's first line, v0.1.0 era), filed as
**#45** — noisy, and they buried the real evidence during this incident.

## Next Steps

1. **#44 first** — it is the root cause of the incident above and will bite
   again the moment any agent builds a dev CLI. #43 next (stable entry point).
2. **Watch the current session** — it is healthy as of 16:35 but was relaunched
   several times during diagnosis; if the duplicate bar reappears, check bar
   load versions in the zellij log FIRST (one grep, definitive).
3. **Backlog filed this session** — #38 (status glyph fidelity: red lingers
   after interaction; amber never appears during long thinking — two cases,
   one issue, split if root causes diverge), #39 (row order doesn't follow
   interaction recency — currently by-design per the 2026-07-08 §6.6 revision;
   decide in the #24 brainstorm), #40 (Nerd Font portability + fallback),
   #31 (dev-install doesn't regenerate sandbox config; plus the shared
   `~/.cargo/bin` swap hazard).
4. **Then**: the #24 UI epic brainstorm (heavily pre-designed in its comments —
   read them all; this session added the dwell-open + dormant-glyph note), the
   doctor/installer track (`docs/superpowers/specs/2026-07-21-installer-doctor-design.md`,
   locked) → v0.2.0, #15, #11.
5. **Residuals accepted and documented on #6** (do not re-raise without live
   evidence): reuse-ABA fencing via per-record incarnation, and prune emission
   from the snapshot path.

**Where work stopped — verbatim (maintainer):** "Okay, let's do it, run the git
commands, and let's flip over this session to the new main and hope."
→ `git tag v0.1.1` + `git push origin v0.1.1` were run; the session worktree was
flipped to detached `ccd67fb`; `just release` deliberately left to the maintainer.

## Context to Preserve

- **User prefs (binding)**: extremely concise, signal over noise; explain while
  doing; dense why-comments citing spec §/ledger/issue; conventional commits +
  `Claude-Session: <session URL>` trailer on its own line; **never commit
  without explicit approval** (he signs via 1Password; the session may run
  `git commit` once approved); ask before architecture decisions with multiple
  valid approaches.
- **Zellij lifecycle is the human's.** Never launch or kill a session; print
  the command. Sandbox-only hot-reload is the one sanctioned agent live
  mutation. `zellij action` against a dead session blocks forever.
- **Never install to the stable release surface from a working session** — only
  `just dev-install`. `just release` is the maintainer's, watched live. (A
  permission classifier also blocks it, correctly.)
- **Verification bar**: `cargo test --workspace` (--workspace load-bearing) AND
  `cargo build -p clave-bar --target wasm32-wasip1` AND
  `cargo clippy --all-targets -- -D warnings`; rustfmt on NEW code only — never
  full-file reflow of model.rs/store.rs (pre-existing drift, bitten twice).
- **Review flow that works**: brief file → implementer agent (Opus) →
  independent adversarial reviewer (Opus) → principal adjudicates + personal
  pass + independent gate run → **CodeRabbit CLI on the committed branch** →
  respond to PR-bot findings (CodeRabbit cloud + Codex) before merge.
- **`~/.claude/` is READ-ONLY source of truth**; orphaned-cwd sessions: recreate
  the dir at the same absolute path, never relocate jsonls.
- **SSH constraint**: clave must eventually work over SSH — reject designs
  assuming CLI and plugin share a local desktop.
- The maintainer keeps a personal `todo.txt` (symlinked out of home into a
  private notes repo); the clave items in it were triaged into #38/#39/#40 and
  the Ctrl+G one checked off.
- Pre-commit hooks: secret scanners + a **PII blocklist** that rejects private
  path names in staged lines — genericize such paths in tracked docs. This
  fires on handoffs that quote local paths; it caught two this session.

## Restart Hint

Everything committed, merged, and pushed; tag `v0.1.1` is live on `ccd67fb`;
`just release` has RUN and the post-release incident is resolved (see its
section above — read it before touching release/install mechanics). The
maintainer is daily-driving v0.1.1 again. Safe to /clear. Resume with **#44**
(bar's PATH-resolved shellouts — the incident's root cause), then #43, then the
#24 brainstorm / #38–#40 backlog.
