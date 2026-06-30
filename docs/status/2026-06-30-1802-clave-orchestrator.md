# Status — clave orchestrator

_2026-06-30 18:02 · repo: github.com/olliegilbey/clave (public) · branch: main_

## Task Overview
Build **clave**: an open-source, terminal-native orchestrator for multiple
Claude Code agents, driven from a Zellij vertical sidebar. One agent = one Zellij
tab running the real Claude TUI; a left bar lists them with a status emoji; `Alt`
keys add/jump between them.

**Success (v1):** vertical-tabs bar in an `agents` layout · `clave spawn` ·
`clave add` (zoxide picker, worktree opt-in default-off) · status hooks → tab
emoji · keybinds `Alt+a`/`Alt+c`/`Alt+1…9`. Full scope in design.md §7.

This file is a handoff; the **authoritative spec is `docs/design.md`**, written
for zero-context implementers. Read that first.

## Reference Docs
- `docs/design.md:6-23` — §0 how to use the brief (intent fixed, impl open).
- `docs/design.md:50-79` — §2 **first principles / invariants** (the locked
  constraints — honour these; relitigate only with strong evidence).
- `docs/design.md:92-162` — §4 **verified knowledge base** — Claude CLI flags,
  jsonl path scheme, hooks + payloads, Zellij capabilities, T3 lessons. **Do not
  re-research these.**
- `docs/design.md:175-234` — §6 per-subsystem goal + constraints, each with an
  **Open:** block for the implementer to explore.
- `docs/design.md:235-249` — §7 v1 scope / deferred / risks.

## Current State
`git status` clean; everything committed (`db9b260`) and pushed to public origin.

Committed scaffold:
- `Cargo.toml` — edition 2024, deps all latest (`cargo outdated` = clean).
- `src/main.rs` — clap skeleton, subcommands `add`/`spawn`/`hook`/`ls` stubbed
  with `todo!()` pointing at task numbers. **Compiles green** on Rust 1.96.1.
- `README.md`, `docs/design.md`, `LICENSE` (MIT), `.gitignore`, `Cargo.lock`.

Tasks: #1 scaffold **done**. #2–#7 **pending** (mirror design.md §6):
#2 `clave spawn` · #3 state store + `ls` · #4 `clave add` · #5 jsonl naming ·
#6 status hooks + tab repaint · #7 vertical-tabs plugin + layout + keybinds.

No code logic exists yet — only the skeleton. Nothing is wired into the user's
environment.

## Important Discoveries
Architecture rejections (full rationale design.md §3, **do not re-explore**):
nvim-plugin (no persistence), tmux (Zellij investment wins), headless+custom-UI à
la T3 (loses the real TUI), tiled panes (want pick-one-jump → tabs), build-a-WASM-
plugin-first (prebuilt `cfal/zellij-vertical-tabs` suffices for v1).

Hard-won facts (captured in design.md §4 so they survive):
- `claude` supports `--session-id <uuid>`, `--name`, `--resume` — `clave` mints
  the UUID, which is the join key for jsonl + hook correlation. **No `--color`
  flag.**
- `/rename` + `/color` are **not readable** anywhere on disk (verified) →
  orchestrator owns name/colour ([#58588]).
- Status comes from **Claude hooks** (global `~/.claude/settings.json`), keyed by
  `session_id`. `AskUserQuestion` does **not** fire `Notification` ([#59908]) —
  known blind spot.
- Zellij `rename-tab` targets the **active** tab → repainting a background agent's
  emoji is the **#1 open risk** (design.md §6.5). Try simplest path first;
  escalate to `zellij pipe` + tiny plugin only if forced.

Session gotchas (prevent repeats):
- `cargo add dirs` left it at v5 because the hand-written `"5"` req capped it; had
  to `cargo add dirs@6`. Always verify with `cargo outdated` after adding.
- Rust was 5 releases behind (1.91→1.96.1); updated via `rustup update stable`.
- The T3 research (pingdotgg/t3code) was distilled into design.md §4; raw agent
  output lived in scratchpad (now gone). To go deeper, re-clone and read
  `apps/server/src/provider/Layers/ClaudeAdapter.ts` and
  `apps/web/src/components/Sidebar.logic.ts`.

[#58588]: https://github.com/anthropics/claude-code/issues/58588
[#59908]: https://github.com/anthropics/claude-code/issues/59908

## Next Steps
In priority order:
1. **Task #2 `clave spawn`** (design.md §6.1) — small, foundational, everything
   leans on it. Idempotent resume-or-create by UUID.
2. **Task #3** state store + `ls` (§6.2) — the thin index #4/#6 read & write.
3. **Task #4** `clave add` (§6.3) — zoxide picker → new tab → spawn → record.
4. **Task #5** naming (§6.4), **Task #6** status hooks (§6.5), **Task #7** bar +
   keybinds (§6.6).

**Open question / where the user paused:** they had not chosen who builds (fresh
zero-context agent vs. continue in a context-loaded session) — they ran /handoff
instead of answering. Verbatim, the assistant's last message:

> How do you want to take it from here?
> - **Launch a zero-context agent now** — I can fire one (or a few, per subsystem) against the brief to prototype and propose, then you review.
> - **You start a fresh session in the repo** — cleanest "true zero-context" run; the brief is built for it.
> - **I keep building here** — straight into task #2, `clave spawn`, with all our context already loaded.
>
> My recommendation: for the *first* slice, **let me build `spawn` here** — it's small, foundational, and benefits from the full design context being live; then hand the broader §6 surface to fresh agents once the spine exists. Your call though.

So: confirm with the user which path they want before diving in.

**Blocker to be aware of:** tasks #6 and #7 wire into the **user's dotfiles repo,
not this one** — see Context below.

## Context to Preserve
- **Dual-repo reality (critical):** the user's live config is managed from
  `~/dotfiles`. Global Claude hooks (#6) go in `~/dotfiles/src/.claude/settings.json`;
  Zellij keybinds + the `agents` layout (#7) go in
  `~/dotfiles/src/.config/zellij/config.kdl` (+ `layouts/`). Edit `src/*` there,
  then `just bootstrap` — never edit `~/.claude` or `~/.config` directly. The
  `clave` repo holds the binary + example config only.
- **Keybinds:** `Alt`-prefixed so they fire in Zellij `locked` mode through a
  focused Claude (no space collision). Chosen: `Alt+a` add, `Alt+c` toggle bar
  (right-hand Dvorak, "claude"), `Alt+1…9` jump. **Keep the user's `Alt+y`** for
  the nvim float. New binds go in the `shared_among "normal" "locked"` block.
- **User prefs:** extremely concise; explain while doing; **more code comments
  than normal**; conventional commits; commit messages end with
  `Claude-Session: https://claude.ai/code/session_01PoTr1Trkmn4QnkiXdpE2aN`.
  **Ask before commits/PRs and before architecture choices with multiple valid
  approaches.** Greybeard shell/dev-env tone.
- **Tech:** Rust, bleeding edge — latest stable + edition 2024 (nightly available
  but deliberately not used). Keep deps at latest (`cargo outdated`).
- **Promises:** repo is public at the user's explicit request. No other
  outstanding promises.

## Restart Hint
Tree clean, committed, pushed — **safe to /clear**. Resume by reading
`docs/design.md` (§0 → §6.1), then confirm the build-path question above and start
task #2 `clave spawn`.
