# Status — clave orchestrator

_2026-07-01 15:44 · repo: github.com/olliegilbey/clave (public) · branch: main_

## Task Overview
Build **clave**: an open-source, terminal-native orchestrator for multiple Claude
Code agents, driven from a Zellij vertical sidebar. One agent = one Zellij tab
running the real Claude TUI; a first-party WASM plugin (`clave-bar`) renders a left
bar listing agents grouped by repo, recency-sorted, with a colour-coded status glyph.

**This thread's work:** ran a `/superpowers:brainstorming` pass that resolved every
open design decision, then a Fugu multi-agent review, folded the verified findings in,
and committed the result. **The design is now locked and reviewed.** The immediate
next action is to run **`/superpowers:writing-plans`** (as a fresh agent) to turn the
spec into a sequenced implementation plan, then build (spikes first).

Predecessor handoff (pre-brainstorm state, mostly superseded): see
@docs/status/2026-06-30-1802-clave-orchestrator.md — only for the dual-repo dotfiles
note and user-pref detail; the design content there is stale (Open: blocks are now
Decided).

## Reference Docs
**Canonical spec** — `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md`
(read this fully before doing anything; `docs/design.md` is now just a pointer to it):
- `:67-105` — §2 **invariants** (fixed constraints; #3 UUID join key, #5 idempotent
  spawn, #11 bar renders from pushed model, #12 session isolation).
- `:109-125` — §3 decisions table (what was rejected and why — don't relitigate).
- `:129-215` — §4 **verified knowledge base** (CLI flags, munging rule, hooks, Zellij
  plugin API). **Do not re-research.**
- `:231-271` — §5 data model + store locking + pipe contract.
- `:273-438` — §6 subsystem specs (all Decided).
- `:491-528` — §9 **spike plan** — S0/S0b/S1 are the gates; build order starts here.
- `:530-562` — §10 scope / deferred / known limitations.

## Current State
**Working tree clean; everything committed and pushed to main.** `git status` shows
only untracked `.claude/` (project settings — see below). No implementation code yet
beyond the clap skeleton.

Commits this thread:
- `de6cddf` docs: lock clave v1 design + fold in fugu-review findings — the canonical
  spec + `design.md` reduced to a pointer + `.gitignore` adds
  `.claude/settings.local.json`.
- (prior) `7fa1caf`, `db9b260` — handoff/backlog note + original scaffold.

Scaffold state (unchanged): `Cargo.toml` (edition 2024), `src/main.rs` clap skeleton
with `add`/`spawn`/`hook`/`ls` stubbed as `todo!()`, compiles green. **The spec now
mandates a Cargo *workspace* (`crates/clave` + `crates/clave-bar` + `crates/clave-types`)
— the current single-package `Cargo.toml` must be restructured during implementation.**

Untracked `.claude/`:
- `settings.local.json` — local permission allowlist, now gitignored (leave it).
- `settings.json` — enables the superpowers plugin. **Deliberately left untracked** —
  open question whether clave (public) should ship with superpowers enabled. User to
  decide; don't commit without asking.

## Important Discoveries
Locked architecture decisions (full rationale in spec §3; **do not re-explore**):
- **Dedicated `clave` Zellij session**, launched with a clave-owned config so keybinds
  never touch the user's global Zellij config (invariant #12).
- **First-party `clave-bar` WASM plugin** (not painter+cfal, not a fork). It renders
  its own repo-grouped, recency-sorted list from a pushed model — because stock cfal
  renders only in tab order with no sort/group, and there is **no CLI cross-tab
  rename** (zellij #4591/#4602). Shares a `clave-types` crate with the binary.
- Store = single JSON + file lock + atomic rename (SQLite rejected).

Fugu review (this thread) — **verified findings already folded into the spec**, so
don't re-report them; they're recorded as decisions:
- cwd munging is `s/[^A-Za-z0-9]/-/g` (dots→dash too), **not** just `/`→`-`. Verified
  on disk. This is the join key; wrong rule breaks idempotency, esp. worktrees.
- Store lock must be a **separate lockfile** (`agents.lock`, `fs4`), never the
  renamed-over data file — else concurrent hooks lose updates.
- Status is a **latest-wins state machine** (see transition table §6.5), not a
  priority-max — a max would stick the red "needs you" glyph after you answer.
- `Alt+a` must be a Zellij `Run` into a **floating pane** (fzf needs a TTY), not a
  MessagePlugin. Dynamic-UUID tab creation via a **one-shot temp `.kdl` layout**.
- Pipe messages are **full-replace + monotonic `seq`** (kills the hydration race).
- Global hooks: untracked fast path must be lock-free + exit-0; never emit permission
  decisions (PermissionRequest is a decision hook — clave stays pass-through).
- Added spikes **S0** (`--session-id` actually creates) + **S0b** (munging round-trip)
  as the true gates, ahead of S1.

Fugu workflow bug (already delegated to another agent — do NOT fix here): the opus
consolidator did the real verification but its final large `StructuredOutput` call
was truncated to only the first field, so it returned an all-`"test"` stub. Findings
were recovered manually from the subagent transcripts. The review conclusions are
sound and already applied.

Dismissed finding: "Cargo.toml is single-package not a workspace" — expected (scaffold
predates the plan; restructure during impl).

## Next Steps
In priority order:
1. **Run `/superpowers:writing-plans` as a fresh agent** against the canonical spec to
   produce the FIRST plan: **foundation + spikes only** (the spec's spike-gate makes
   planning the risky subsystems prematurely wasteful — decided this thread). That plan
   should cover: the Cargo **workspace** restructure (§7) + `clave-types` + the
   `munge_cwd` helper (all TDD), then spikes **S0** (`--session-id` creates) / **S0b**
   (munging round-trip) / **S1** (background repaint — THE gate) / **S2** (uuid→pane
   join). If S1 fails, stop and revisit spec §3 before planning subsystems.
2. **After spikes pass, plan + build the subsystems** in dependency order: `clave spawn`
   (§6.1, uses `munge_cwd`) → state store + `ls` (§6.2) → `clave hook` + status state
   machine (§6.5) → full `clave-bar` (§6.6) → `clave add` (§6.3, temp-layout tab
   creation + fzf) → naming (§6.4) → archiving (§6.7) → session/config + keybinds (§6.8).
3. §6.5/§6.8 wire into the **user's dotfiles repo**, not this one (see Context).
4. Open item: reconcile spec §4/§7 "cdylib" vs the plugin likely being a **plain binary
   crate** — zellij's official rust-plugin-example is a binary (`src/main.rs`,
   `register_plugin!`), not cdylib; confirm during S1.

**Where work stopped:** the design is locked, reviewed, and committed (`de6cddf`), and
this handoff is written. `/superpowers:writing-plans` has **not** been run yet — that
is the next step, and it should be done by a **fresh agent** working purely from this
handoff + the spec (zero-context, so the plan isn't biased by a loaded session and to
validate the docs are self-sufficient). **Immediate next action: a fresh agent runs
`/superpowers:writing-plans`** to produce the foundation+spikes plan (step 1 above).

## Context to Preserve
- **User prefs:** extremely concise, signal over noise; explain while doing; **more
  code comments than normal**; conventional commits; commit messages end with
  `Claude-Session: https://claude.ai/code/session_01RnrkGLSYqx3JeE7Uvsmewb`.
  **Ask before commits/PRs and before architecture decisions with multiple valid
  approaches.** Greybeard shell/dev-env tone.
- **Branch policy:** prior commits go straight to `main` on this solo public repo;
  this thread followed that. Confirm before assuming a feature-branch/PR flow.
- **Dual-repo reality (critical for §6.5/§6.8):** the user's live config is managed
  from `~/dotfiles`. `~/.claude` is a **symlink → `~/dotfiles/src/.claude`**, so
  editing `~/.claude/settings.json` edits the dotfiles source. `clave setup` should do
  an additive/idempotent merge into `~/.claude/settings.json` generically; the
  `just bootstrap` step is a personal nicety, kept out of the canonical spec.
- **Keybinds:** `Alt`-prefixed, in `shared_among "normal" "locked"`, but scoped to the
  clave session's own config. Keep the user's `Alt+y` (nvim float) and `Alt+h/l`.
- **Tech:** Rust, bleeding edge — latest stable + edition 2024. Keep deps latest
  (`cargo outdated`). Workspace: `crates/clave` (bin, host target) + `crates/clave-bar`
  (Zellij WASM plugin → `wasm32-wasip1`; **likely a plain binary crate, not cdylib** —
  Next Steps #4) + `crates/clave-types` (serde-only, compiles for both targets = the
  anti-drift pipe schema). Set workspace `resolver = "3"` and
  `default-members = ["crates/clave","crates/clave-types"]` so a plain `cargo
  build`/`test` skips the wasm-only crate.
- **Environment (verified this thread):** Zellij 0.44.3, Claude 2.1.197,
  `fzf`+`zoxide`+`jq` present, no `skim`. `zellij-tile` crate is **0.44.2** (use
  `= "0.44"`). Rust wasm target **`wasm32-wasip1` is NOT installed** (only
  `wasm32-unknown-unknown`) — `rustup target add wasm32-wasip1` before building the
  plugin. Plugin API confirmed: `MessagePlugin`/`MessagePluginId` route
  keybinds→plugin; `go_to_tab(index)` and `rename_tab(pos,name)` exist; trait =
  `load(cfg)`/`update(Event)->bool`/`pipe(PipeMessage)->bool`/`render(rows,cols)` +
  `register_plugin!`/`request_permission`.
- **Promise:** repo is public at the user's explicit request.

## Restart Hint
Tree clean, committed, pushed — **safe to /clear**. Resume by reading the canonical
spec (`docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md`, §0 → §9), then
run `/superpowers:writing-plans`.
