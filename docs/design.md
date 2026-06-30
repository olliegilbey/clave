# clave — design & implementation brief

> Terminal-native orchestration for a fleet of Claude Code agents, driven from a
> Zellij sidebar.

## 0. How to use this document (read first)

This brief is written for an implementing agent starting with **zero prior
context**. It separates three things on purpose:

- **Intent & first principles (§1–§2)** — *fixed*. These capture what `clave` is
  and the invariants every implementation must honour. Relitigate them only with
  strong evidence.
- **Verified knowledge base (§4)** — *facts we already established* (against disk,
  `claude --help`, and the Claude/Zellij docs). **Do not re-research these**; build
  on them. Sources are linked.
- **Subsystem briefs (§6)** — each states the *goal* and *constraints*, then marks
  what is **Open:** — the implementation is yours to explore, prototype, and
  propose. Prefer the simplest thing that satisfies the invariants; earn
  complexity with a concrete reason.

If something here is wrong or a simpler path exists within the invariants, say so.

## 1. What clave is

Running several Claude Code agents at once is painful: they scatter across
terminal tabs with no shared view of which is **working**, which **needs you**,
and which has **finished**. [T3 Code](https://github.com/pingdotgg/t3code) solves
this as an Electron desktop app. `clave` brings the same orchestration UX into
the terminal, where the agents already live.

**Core model:** one dedicated Zellij workspace; **one agent = one Zellij tab**
running the *real* Claude Code TUI; a **vertical left bar** lists agents with a
status emoji; you add and jump between them with `Alt` keys that fire even while
a Claude session has focus.

```
┌─────────────────────┐
│ 🔴 main·api·fix-au…  │  needs you (waiting for input / approval)
│ ⚙️ feat·web·add-na…  │  working
│ ✅ main·docs·updat…  │  done, unread
│ ⚪ main·cli·refacto…  │  idle
└─────────────────────┘
```

The name `clave`: the foundational rhythm an ensemble locks to (the
orchestrator); Spanish for *key/keystone* (keyboard-driven, central); archaic
past tense of *cleave*, to split (panes). Logo: the two-stick percussion clave.

## 2. First principles (invariants)

1. **The agent is the real Claude TUI.** Never parse or scrape its rendered
   output. Users keep vim mode, slash commands, everything.
2. **`clave` owns identity.** Name and colour are assigned by `clave` and *pushed*
   into Claude (e.g. `--name`). Never read them back from `/rename` or `/color` —
   they are not exposed (see §4).
3. **The minted UUID is the join key.** `clave` generates each session's UUID and
   passes it to `--session-id`. That UUID locates the transcript and correlates
   every hook event. Everything keys off it.
4. **Status is event-driven, never polled.** Derive state from Claude hooks and
   turn lifecycle — not from screen text or timers.
5. **Spawn is idempotent → restart-safe.** The pane command must resume-or-create
   by UUID, so Zellij resurrection restores conversations rather than starting
   fresh.
6. **Keys pass through a focused Claude.** All `clave` keybindings are
   `Alt`-prefixed and live in Zellij's `shared_among "normal" "locked"` scope.
   Never depend on the space key or on Claude not having focus.
7. **Claude owns the transcript; `clave` owns a thin index.** Don't reimplement
   conversation storage. Resume via `--resume <uuid>`.
8. **Minimal first; earn complexity.** Prefer the prebuilt vertical-tabs plugin
   and the simplest mechanism that works. Build a custom plugin only when a
   concrete, demonstrated limitation forces it.
9. **One static Rust binary, subcommands** (`add`/`spawn`/`hook`/`ls`). Aligns
   with Zellij + zoxide and lets the engine share a types crate with any future
   WASM plugin.
10. **Keep the core provider-shaped, not Claude-welded.** Confine Claude
    specifics to the spawn + hook adapter; the status model and sidebar logic
    should be generic enough that another agent CLI could slot in later.

## 3. Decisions & rationale

| Considered | Verdict | Why |
|---|---|---|
| Neovim plugin (terminals as nvim buffers) | Rejected | No persistence; nvim must be the always-on host; the leader-key-vs-Claude-vim-mode clash only exists in this design. |
| tmux | Rejected | Heavy existing Zellij investment; the vertical-bar ecosystem is Zellij-side; tmux's one edge (`capture-pane`) is unneeded. |
| Headless Claude + custom UI (T3's path) | Rejected | Throws away the real Claude TUI — the opposite of the goal. |
| Agent = pane (tiled) | Rejected | Tiles show many at once; we want pick-one-and-jump. Tabs fit the vertical bar. |
| Agent = its own Zellij session | Deferred | Stronger isolation, heavier switching; revisit for per-agent remote attach via `BreakPane`. |
| Custom WASM bar plugin from scratch | Deferred | Prebuilt vertical-tabs covers list + switch; only the live-status repaint might force a custom plugin. |
| Engine language Go / Bun | **Rust** | Aligns with Zellij + zoxide; the deferred plugin is Rust/WASM only, so one language shares types. |

## 4. Verified knowledge base (don't re-research)

### Claude Code CLI (from `claude --help` on the target machine)
- `--session-id <uuid>` — start a session with a specific UUID. **We mint it.**
- `-n, --name <name>` — set the session display name at launch (the *push* that
  replaces typing `/rename`). There is **no** `--color` flag.
- `-r, --resume <id>` — resume a conversation by session UUID.
- Also present: `-c/--continue`, `--fork-session`, `--no-session-persistence`,
  `--bg/--background`, `--from-pr`, `--remote-control`.

### Transcript storage (verified on disk)
- Path: `~/.claude/projects/<munged-cwd>/<uuid>.jsonl`, where `<munged-cwd>` is
  the absolute cwd with `/` replaced by `-`
  (e.g. `/Users/olliegilbey/dotfiles` → `-Users-olliegilbey-dotfiles`).
- The jsonl is an append-only event stream. Event `type`s include
  `user`/`assistant`/`tool_use`/`tool_result`/`thinking`/`summary`/`system`.
  The **first user message** gives the initial label; a `summary` entry appears
  later and is the upgrade source.
- `~/.claude/projects/<dir>/sessions-index.json` has a per-session `summary`,
  `firstPrompt`, `gitBranch`, etc. — but it is **lazily written and stale for
  live sessions**. Read the live jsonl directly for fresh naming; treat the index
  as best-effort only.
- `/rename` and `/color` are **not** persisted anywhere readable (confirmed: no
  on-disk trace when run). Requested upstream in
  [claude-code#58588](https://github.com/anthropics/claude-code/issues/58588).
  ⇒ Principle #2.

### Claude Code hooks (the status source)
- Configure **globally** in `~/.claude/settings.json` → every session reports
  automatically. (On this machine that file is managed from the dotfiles repo at
  `src/.claude/settings.json`.)
- Every hook receives JSON on **stdin** with common fields incl. `session_id`,
  `cwd`, `transcript_path`, `permission_mode`, `hook_event_name`.
- Status-relevant events: `UserPromptSubmit` (turn starts → working), `Stop`
  (turn finished → idle/done), `StopFailure` (error), `Notification` with matcher
  `permission_prompt|idle_prompt` (needs you), `PermissionRequest` (approval
  dialog). Lifecycle: `SessionStart`, `SessionEnd`.
- **Known gap:** `AskUserQuestion` does **not** fire `Notification`
  ([claude-code#59908](https://github.com/anthropics/claude-code/issues/59908)) —
  that waiting state is invisible to hooks. Fallback: the `Stop`/idle signal.
- Docs: <https://code.claude.com/docs/en/hooks>.

### Zellij (substrate; v0.44+, the user runs `default_mode "locked"`)
- `zellij action focus-pane terminal_<n>` focuses a pane by id (also `plugin_n`,
  bare int). Pane ids are knowable.
- `zellij action rename-tab <name>` renames the **active** tab. **Cross-tab
  rename from the CLI is the open risk** for repainting a background agent — see
  §6 status.
- `session_serialization true` re-runs each pane's **command** on resurrect
  (behind a "Press ENTER to run…" gate). ⇒ idempotent `spawn` (principle #5).
- Keybinds that must work while Claude is focused live in the
  `shared_among "normal" "locked"` block of the config; `Ctrl h` toggles
  locked↔normal. `Alt` binds already work there (e.g. `Alt+y` floats nvim).
- Plugin API (Rust/WASM) can `rename_tab(index, name)`, focus panes, read
  `PaneInfo.title`; `zellij pipe` pushes external events to a plugin. These are
  the escape hatch if CLI rename proves insufficient.
- Bar plugin: [`cfal/zellij-vertical-tabs`](https://github.com/cfal/zellij-vertical-tabs)
  — vertical tab list, click-to-switch, label from tab name or `{title}`,
  tmux-style `#[fg=…]` colour syntax in the format string; fork
  [`kjaymiller/…-and-panes`](https://github.com/kjaymiller/zellij-vertical-tabs-and-panes)
  adds listing panes. (We use tabs, so the base plugin suffices.)

### Prior art — T3 Code transferable ideas
- Priority-ordered status pill: `needs-approval > awaiting-input > working >
  done`. ~20 lines, provider-agnostic.
- "Unread / needs-you" = `last_completed_at > last_visited_at`.
- Own a tiny store `{uuid, cwd, branch, title, status, last_visited}`; let Claude
  own the transcript; resume via `--resume`.
- One git worktree per agent for concurrent isolation (they left cleanup manual —
  don't inherit that gap).

## 5. Keybindings

All `Alt` (Option on macOS), in `shared_among "normal" "locked"` so they fire
through a focused, vim-mode Claude — no collision with Claude's space.

| Key | Action |
|---|---|
| `Alt+a` | add agent (zoxide picker → tab → spawn) |
| `Alt+c` | toggle the agents bar |
| `Alt+h` / `Alt+l` | cycle agents *(already in the user's config)* |
| `Alt+1…9` | jump to agent N |

## 6. Subsystem briefs (goal + constraints; implementation Open)

### 6.1 `clave spawn <uuid> --name <name> --cwd <cwd>`
**Goal:** the command each agent pane runs; idempotent so resurrection resumes.
**Must:** if the session jsonl for `<uuid>` exists → `claude --resume <uuid>`;
else → `claude --session-id <uuid> --name <name>` in `<cwd>`. Replace the process
(exec semantics) so the pane *is* Claude.
**Open:** exact existence check; how `--name`/colour is (re)applied on resume;
handling a `--resume` that races the not-yet-written jsonl on a brand-new agent.

### 6.2 State store + `clave ls`
**Goal:** the thin index everything else reads/writes; `ls` prints agents +
status emoji.
**Must:** persist at least `{uuid, cwd, branch, name, status, last_visited,
tab_ref}`. Be safe under concurrent writes (many hooks can fire at once).
**Open:** format & location (single JSON under `~/.local/state/clave/` vs SQLite
vs per-agent files); locking strategy; how `tab_ref` is represented and captured.

### 6.3 `clave add` (the `Alt+a` flow)
**Goal:** pick a directory, open a tab, spawn an agent, record it.
**Must:** default to the current cwd but allow choosing another via zoxide
frecency; mint a fresh UUID; derive an initial label (§6.4); create a Zellij tab
running `clave spawn`; record the agent. **Worktree spin-up is opt-in, default
off.**
**Open:** picker UX (fzf over `zoxide query -l`?); how the tab is created
(`zellij action new-tab` + a layout, vs `new-tab` then `new-pane`); how to learn
the new tab's id/ref for `tab_ref`; worktree creation + (eventual) cleanup.

### 6.4 Naming
**Goal:** a glanceable, self-updating label.
**Must:** `branch · cwd · <first words of first user message>`, upgraded to the
first words of the session `summary` once written. `clave` owns the label; never
read `/rename`.
**Open:** refresh mechanism (poll the jsonl, file-watch via `notify`, or
re-derive on `Stop`/`UserPromptSubmit` hooks); truncation/width strategy for the
~18-col bar; optional later: a one-shot LLM title (T3-style).

### 6.5 Status + `clave hook <event>`
**Goal:** translate hook events into the tab's status emoji, keyed by the UUID we
own.
**Must:** read the hook JSON from stdin, map `session_id` → agent, update status
per the priority enum (`needs-you > working > done`; `❌` on failure), and repaint
the tab. Maintain `last_visited` (cleared on focus) so `✅` means "done & unread".
Register the global hooks in `~/.claude/settings.json`.
**Emoji:** 🔴 needs you · ⚙️ working · ❌ failed · ✅ done/unread · ⚪ idle.
**Open (the main risk):** repainting a **background** tab. `zellij action
rename-tab` hits the active tab only — explore: (a) is there a reliable
cross-tab CLI path? (b) `zellij pipe` → a tiny custom plugin that calls
`rename_tab(index)`; (c) encode status in the pane title the plugin reads. **Try
the simplest first; escalate only if it genuinely can't repaint background tabs.**

### 6.6 Bar + keybinds
**Goal:** the vertical left bar and the `Alt` keys.
**Must:** add `cfal/zellij-vertical-tabs` (~18 cols) to an `agents` layout; wire
`Alt+a`, `Alt+c`, `Alt+1…9` into the `shared_among "normal" "locked"` block.
**Open:** whether the agents bar is its own Zellij session or tabs within the
working session; how `Alt+c` toggles the bar (swap-layout vs always-on); whether
the bar tints rows via `#[fg=…]` in the tab name (colour bonus) or relies on the
emoji alone.

## 7. v1 scope / deferred / risks

**v1:** vertical-tabs bar in an `agents` layout · `clave spawn` · `clave add`
(zoxide, worktree opt-in default-off) · status hooks → emoji (simplest rename
path) · keybinds `Alt+a`/`Alt+c`/`Alt+1…9`.

**Deferred:** `clave rebuild` from the store (cold/remote start) · custom
`zellij pipe` plugin (only if simple rename fails) · one-shot LLM titles ·
per-row colour tint · worktree auto-cleanup · `AskUserQuestion`-wait state ·
per-agent remote `BreakPane`.

**Risks to validate early:** cross-tab rename (§6.5); plugin honouring `#[fg=…]`
in tab names; terminal sends Option-as-Meta (existing `Alt` binds already work →
almost certainly yes).

## 8. References

- T3 Code (Electron prior art): <https://github.com/pingdotgg/t3code>
- Zellij vertical tabs: <https://github.com/cfal/zellij-vertical-tabs>
- Claude Code hooks: <https://code.claude.com/docs/en/hooks>
- Programmatic name/colour request: <https://github.com/anthropics/claude-code/issues/58588>
- `AskUserQuestion` hook gap: <https://github.com/anthropics/claude-code/issues/59908>
