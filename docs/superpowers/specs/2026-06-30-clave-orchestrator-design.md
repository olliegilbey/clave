# clave — canonical design & implementation spec

> Terminal-native orchestration for a fleet of Claude Code agents, driven from a
> Zellij sidebar.

**Status:** decisions **locked** (post-brainstorm, 2026-06-30). This is the single
canonical spec; it supersedes the original `docs/design.md` brief, which now points
here. Implementation has not started beyond the scaffold (`src/main.rs` clap
skeleton, compiles green).

---

## 0. How to use this document

This spec separates fixed intent from settled implementation:

- **Intent & invariants (§1–§2)** — *fixed*. Honour these; relitigate only with
  strong evidence.
- **Verified knowledge base (§4)** — *facts established against disk,
  `claude --help`, Zellij docs, and the plugin API*. **Do not re-research.** Sources
  linked.
- **Subsystem specs (§6)** — every prior `Open:` block is now a locked **Decided:**.
  Where reality is still genuinely uncertain, the decision names the approach *and*
  cites the **spike** (§9) that must validate it before the surrounding code is
  trusted.

If something here is wrong, or a simpler path exists within the invariants, say so.

---

## 1. What clave is

Running several Claude Code agents at once is painful: they scatter across terminal
tabs with no shared view of which is **working**, which **needs you**, and which has
**finished**. [T3 Code](https://github.com/pingdotgg/t3code) solves this as an
Electron desktop app. `clave` brings the same orchestration UX into the terminal,
where the agents already live.

**Core model:** one dedicated Zellij session; **one agent = one Zellij tab** running
the *real* Claude Code TUI; a **vertical left bar** (a first-party Zellij plugin,
`clave-bar`) lists agents grouped by repo and sorted by recency, each with a
colour-coded status glyph; you add, archive, and jump between them with `Alt` keys
that fire even while a Claude session has focus.

```
┌────────────────────────┐
│ dotfiles               │  ← repo group header (per-repo colour)
│  ● main·fix-auth-flow   │  ● red  = needs you
│  ● feat·add-navbar      │  ● amber = working
│ clave                  │
│  ● main·spawn-cmd       │  ● green = done & unread
│  ● docs·update-readme   │  ● dim  = idle
│  ✖ main·flaky-test      │  ✖ red  = failed
└────────────────────────┘
```

The status indicator is a **single glyph whose font colour encodes state** (not an
emoji — emoji render inconsistently in the bar). The `cwd` segment of each label is
tinted with a stable per-repo colour so repos are visually distinct.

The name `clave`: the foundational rhythm an ensemble locks to (the orchestrator);
Spanish for *key/keystone* (keyboard-driven, central); archaic past tense of
*cleave*, to split (panes). Logo: the two-stick percussion clave.

---

## 2. First principles (invariants)

1. **The agent is the real Claude TUI.** Never parse or scrape its rendered output.
   Users keep vim mode, slash commands, everything.
2. **`clave` owns identity.** The label (`cwd · branch · summary`) and colours are
   computed by `clave` and rendered by `clave-bar`. The launch `--name` is a
   courtesy push into Claude's own session list; never read `/rename` or `/color`
   back — they are not exposed (see §4).
3. **The minted UUID is the join key.** `clave` generates each session's UUID and
   passes it to `--session-id`. That UUID locates the transcript, correlates every
   hook event, and joins the store row to its Zellij pane. Everything keys off it.
4. **Status is event-driven, never polled.** Derive state from Claude hooks and turn
   lifecycle — not from screen text or timers.
5. **Spawn is idempotent → restart-safe.** The pane command resumes-or-creates by
   UUID, so Zellij resurrection restores conversations rather than starting fresh.
6. **Keys pass through a focused Claude.** All `clave` keybindings are `Alt`-prefixed
   and live in `shared_among "normal" "locked"`. Never depend on the space key or on
   Claude not having focus.
7. **Claude owns the transcript; `clave` owns a thin index.** Don't reimplement
   conversation storage. Resume via `--resume`.
8. **Minimal first; earn complexity with evidence.** We escalated to a custom plugin
   only after proving the prebuilt path cannot satisfy the requirements (no CLI
   cross-tab rename; no sort/grouping in stock cfal — see §3/§4). Every further step
   up must be similarly earned.
9. **One Cargo workspace, two artifacts, shared types.** A native binary
   (`clave`, subcommands `add`/`spawn`/`hook`/`ls`/`archive`/`focus`/`snapshot`/`setup`)
   and a WASM plugin
   (`clave-bar`) share a `clave-types` crate so the pipe schema cannot drift (§7).
10. **Keep the core provider-shaped, not Claude-welded.** Confine Claude specifics to
    the spawn + hook adapter; the status model and bar logic stay generic enough that
    another agent CLI could slot in later.
11. **`clave-bar` renders from clave's pushed model, decoupled from Zellij tab
    order.** This is what enables repo-grouping and recency-sort: the bar's display
    order is *not* the tab order. Selection maps a displayed row → the agent's pane →
    `go_to_tab`.
12. **clave is self-contained in its own session.** The dedicated `clave` Zellij
    session is launched with a clave-owned config; clave **never mutates the user's
    global Zellij config**. (Claude *hooks* are necessarily global — see §4 — but
    no-op fast for sessions clave doesn't track.)

---

## 3. Decisions & rationale (locked)

| Considered | Verdict | Why |
|---|---|---|
| Neovim plugin (terminals as nvim buffers) | Rejected | No persistence; nvim must be the always-on host; leader-key vs Claude-vim clash. |
| tmux | Rejected | Heavy existing Zellij investment; the vertical-bar ecosystem is Zellij-side. |
| Headless Claude + custom UI (T3's path) | Rejected | Throws away the real Claude TUI — the opposite of the goal. |
| Agent = pane (tiled) | Rejected | Tiles show many at once; we want pick-one-and-jump → tabs. |
| Agents in the current Zellij session | Rejected | Mixes orchestration with normal tabs; `Alt+1…9` and the bar become ambiguous. |
| **Agents in a dedicated `clave` session** | **Chosen** | One isolated workspace (intent §1); bar always present; keybinds scoped to clave's own config (invariant #12). |
| Repaint background tab via CLI `rename-tab` | Rejected | **No CLI cross-tab rename exists** (§4; zellij #4591/#4602). Renames only the active tab. |
| Painter plugin + stock `cfal/zellij-vertical-tabs` | Rejected | Stock cfal renders in **tab order** with no sort/group concept (§4). Cannot do recency-sort, 2-level repo grouping, or an archive view — all required. |
| Fork cfal | Rejected | Its architecture mirrors tab order via a format-string DSL; our model (render from pushed state) fights it; external repo → no shared types; fork drift. |
| **First-party `clave-bar` WASM plugin** | **Chosen** | Renders directly from clave's agent model (invariant #11); shares `clave-types` with the binary (invariant #9); single repo, no drift; reference cfal (MIT) for technique only. |
| State store: SQLite | Rejected | Overkill; clave state won't bloat to needing it. |
| **State store: single JSON + file lock + atomic rename** | **Chosen** | Simple, greppable, concurrency-safe for the hook fan-in. |
| Engine language Go / Bun | **Rust** | Aligns with Zellij + zoxide; the WASM plugin is Rust/WASM, so one language shares types. |

---

## 4. Verified knowledge base (don't re-research)

### Claude Code CLI (`claude --help`, v2.1.197 on target)
- `--session-id <uuid>` — start a session with a specific UUID. **We mint it.**
  (`--help` text is ambiguous on create-vs-resume; spike **S0** pins that a *fresh*
  UUID creates.)
- `-n, --name <name>` — set the session display name at launch. **No `--color` flag.**
- `-r, --resume <id>` — resume by UUID. (`--resume` with no id shows Claude's own
  picker, but clave does **not** use that: it must know the UUID up front so the pane
  command is the idempotent `clave spawn <uuid>` — see §6.3 and invariant #5.)
- `-w, --worktree [name]` (and `--tmux`) — Claude can create a git worktree itself.
  clave does **not** use it: clave must own the worktree path to compute the munged
  jsonl path for the idempotency check and to record it in the store (§6.3); Claude's
  flag would hide that path.
- Also present: `-c/--continue`, `--fork-session`, `--no-session-persistence`,
  `--bg/--background`, `--from-pr`, `--remote-control`.

### Transcript storage (verified on disk)
- Path: `~/.claude/projects/<munged-cwd>/<uuid>.jsonl`. **Munging replaces every
  non-alphanumeric character — not just `/` — with `-`** (empirically
  `s/[^A-Za-z0-9]/-/g`; a `.` becomes `-` too). Verified on disk:
  `/Users/olliegilbey/code/resumate/.claude-worktrees/nalu-cta` →
  `-Users-olliegilbey-code-resumate--claude-worktrees-nalu-cta` — note the `--`, one
  dash for the `/` and one for the `.`. This string is the **join key** `clave spawn`
  computes for its existence check, so the rule must live in one shared helper and be
  pinned by spike **S0b**. The old `/`→`-` shorthand was wrong and breaks any dotted
  or worktree path (worktrees live under `.claude-worktrees`).
- **Canonicalize the cwd first — S0b, verified on disk 2026-07-01.** Claude munges the
  **physical** cwd: it reads `getcwd()`, which resolves symlinks (macOS
  `/var/folders/…` → `/private/var/folders/…`, `/tmp` → `/private/tmp`). So `clave`
  must `std::fs::canonicalize` the cwd **before** `munge_cwd`, or the join key misses
  the real jsonl, `clave spawn` wrongly takes the create path, and Claude aborts:
  `Error: Session ID … is already in use` — also verified, so a pre-existing
  `--session-id` is a **hard error** (exit 1), confirming §6.1. The `munge_cwd`
  character rule matched disk exactly (worktree `--` included); only the input needed
  canonicalizing. (Headless `-p` mode does persist the jsonl — used only by the S0b
  harness; the product launches the interactive TUI per invariant #1.)
- Append-only event stream; `type`s include
  `user`/`assistant`/`tool_use`/`tool_result`/`thinking`/`summary`/`system`. The
  **first user message** gives the initial label; a `summary` entry appears later and
  is the upgrade source.
- `~/.claude/projects/<dir>/sessions-index.json` has per-session `summary`,
  `firstPrompt`, `gitBranch` — but is **lazily written and stale for live sessions**.
  Read the live jsonl directly; treat the index as best-effort.
- `/rename` and `/color` are **not persisted anywhere readable** (confirmed). Upstream
  request: [claude-code#58588]. ⇒ invariant #2.

### Claude Code hooks (the status source)
- Configured **globally** in `~/.claude/settings.json` (managed on this machine from
  the dotfiles repo at `src/.claude/settings.json`). Every session reports; there is
  no per-session hook config — so `clave hook` must **no-op fast** for any
  `session_id` not in its store.
- Each hook receives JSON on **stdin** incl. `session_id`, `cwd`, `transcript_path`,
  `permission_mode`, `hook_event_name`.
- Status-relevant events: `UserPromptSubmit` (turn starts → working; also bumps
  `last_interacted`), `Stop` (turn finished → done/idle), `StopFailure` (error),
  `Notification` matcher `permission_prompt|idle_prompt` (needs you),
  `PermissionRequest` (approval). Lifecycle: `SessionStart`, `SessionEnd`.
- **Known gap:** `AskUserQuestion` does **not** fire `Notification`
  ([claude-code#59908]) — that waiting state is invisible; fall back to `Stop`/idle.
- Docs: <https://code.claude.com/docs/en/hooks>.

### Zellij (substrate; v0.44.3; user runs `default_mode "locked"`)
- **No CLI path renames a background tab.** `zellij action rename-tab` renames the
  **active** tab only; renaming by index/id without focus is an open feature request
  ([zellij#4591], [zellij#4602]) — unimplemented in 0.44.3.
- **Plugin API can rename any tab by index without stealing focus:**
  `rename_tab(tab_position: u32, name)` — permission `ChangeApplicationState`. (We do
  not use this in the chosen design — `clave-bar` renders its own list — but it
  confirms the plugin layer is the only mechanism that can touch background tabs.)
- **Plugins receive `zellij pipe` messages** via the `pipe()` trait method
  (`CliPipeInput`) — permission `ReadCliPipes`. `clave hook` pushes status with
  `zellij pipe --name clave-status -- '<json>'`.
- **Keybinds can message a plugin:** `MessagePlugin "file:…wasm" { name; payload }`
  (and `MessagePluginId <n> { … }`). This routes `Alt`-nav keys to `clave-bar` while
  Claude is focused, so the bar's *display order* (not tab order) drives navigation.
- Native tab nav: `GoToTab <n>`, `GoToNextTab`, `GoToPreviousTab`.
- `zellij action focus-pane` / plugin `go_to_tab(index)` / `focus_or_create_tab` move
  focus to a pane/tab by id/index.
- `session_serialization true` re-runs each pane's **command** on resurrect (behind a
  "Press ENTER to run…" gate). ⇒ idempotent `spawn` (invariant #5).
- A session can be launched with its own config + layout:
  `zellij --config <clave.kdl> --layout <agents.kdl> attach -c clave`. ⇒ keybind
  isolation (invariant #12).
- Plugin model (`zellij-tile`, Rust→WASM): implement `ZellijPlugin` —
  `load(config)`, `update(Event) -> bool`, `pipe(PipeMessage) -> bool`,
  `render(rows, cols)`; `register_plugin!`, `request_permission`, `subscribe`.
  Build to `wasm32-wasip1` as a **binary crate** (`src/main.rs` + `register_plugin!`,
  matching zellij's official rust-plugin-example — not a cdylib); load via layout
  `plugin location="file:…wasm"`. Plugins can run commands (`RunCommands`) and read
  the Zellij-cwd filesystem (sandboxed).
- Reference plugin: [`cfal/zellij-vertical-tabs`](https://github.com/cfal/zellij-vertical-tabs)
  (MIT). Renders tabs **in tab order** from `{index}/{name}/{title}`; config knobs
  are `format`, `format_active`, `indicator_*`, `max_name_length`, `border`,
  `start_index`, `padding_*`, `overflow_*`, `activity_format`. Supports tmux-style
  `#[fg=…,bold,dim]` colour. Its `activity` pipe message renders sub-agent/todo rows
  *beneath* a tab — **not** a per-tab status indicator, and it has **no sort/group
  option**. We borrow its truncation/render technique, not its architecture.

### Prior art — T3 Code transferable ideas
- Priority-ordered status: `needs-you > working > done` (`failed` is its own glyph).
- "Unread / needs-you" = `last_completed_at > last_visited_at`.
- Tiny store `{uuid, cwd, branch, label, status, last_visited}`; Claude owns the
  transcript; resume via `--resume`.
- One git worktree per agent for isolation (they left cleanup manual — we track it).

[claude-code#58588]: https://github.com/anthropics/claude-code/issues/58588
[claude-code#59908]: https://github.com/anthropics/claude-code/issues/59908
[zellij#4591]: https://github.com/zellij-org/zellij/issues/4591
[zellij#4602]: https://github.com/zellij-org/zellij/issues/4602

---

## 5. Data model & state store

**Store:** single JSON file at `~/.local/state/clave/agents.json`. Read-modify-write
under an advisory lock on a **separate, never-renamed lockfile**
(`~/.local/state/clave/agents.lock`, `fs4`/flock — `fs2` is unmaintained) held across
the whole RMW; the data write itself is temp-file + atomic `rename`. **Locking the
data file directly would be a bug** — the rename swaps the inode out from under a
second writer's lock, so concurrent hooks (§4 fan-in) silently lose updates; the
dedicated lockfile fixes that. The untracked-session fast path (§6.5) reads
**without** the lock (an atomic-rename reader always sees a whole file), so clave
never serialises unrelated sessions' hooks.

**Agent record:**

```jsonc
{
  "uuid":            "…",        // minted; --session-id; the join key (invariant #3)
  "cwd":             "/abs/path",
  "repo_root":       "/abs/repo", // git toplevel of cwd; the grouping key
  "branch":          "main",
  "label":           "clave · main · spawn-cmd", // cwd · branch · summary (§6.4)
  "status":          "working",  // idle|working|needs_you|done|failed
  "last_interacted": 0,          // unix s; bumped on UserPromptSubmit → recency sort
  "last_visited":    0,          // unix s; bumped on focus → unread = done & !visited
  "archived":        false,      // archived agents are hidden from the bar
  "worktree":        null,       // path if spawned in a git worktree, else null
  "label_source":    "first_prompt" // first_prompt|summary; once summary, stop re-scanning jsonl (§6.4)
}
```

**`clave-types` (the pipe schema, shared by binary + plugin):** `clave` pushes the
**full** (small) agent list to `clave-bar` on every change via
`zellij pipe --name clave-status -- '<AgentSnapshot json>'`. **Contract:** every
`clave-status` message is an authoritative **full replace** carrying a monotonic
`seq`; the plugin applies only the highest `seq` it has seen and discards
stale/out-of-order messages. This makes the startup race benign — whether the first
message the plugin sees is a hook push or the `clave snapshot` hydrate (§5 / **S5**),
last-writer-by-`seq` wins with no lost update. The uuid→pane join lives in the plugin
(§6.6 / **S2**); the store records no live tab position.

---

## 6. Subsystem specs (Decided)

### 6.1 `clave spawn <uuid> --name <label> --cwd <cwd>`
**Goal:** the command each agent pane runs; idempotent so resurrection resumes.
**Decided:**
- Existence check (via the shared munging helper, §4): if
  `~/.claude/projects/<munged-cwd>/<uuid>.jsonl` exists → `exec claude --resume
  <uuid>`; else → `exec claude --session-id <uuid> --name <label>` in `<cwd>`. `exec`
  (replace process) so the pane *is* Claude.
- A brand-new agent has no jsonl ⇒ create path; a resume can never race a
  not-yet-written jsonl. Spike **S0** confirms `--session-id <fresh-uuid>` *creates*
  (and writes the jsonl) rather than erroring or resuming — the whole idempotency
  model rests on it. (An earlier note said "fall back to `--resume` on collision";
  that was wrong — `--resume` errors when no jsonl exists. A UUID collision is a
  genuine error: surface it, don't silently resume.)
- `--name` is set only on create; the bar label is clave-owned and rendered by
  `clave-bar`, so nothing is re-pushed on resume.
- On start, `clave spawn` registers its pane with `clave-bar`:
  `zellij pipe --name clave-register -- '{"uuid":…, "pane_id":<$ZELLIJ_PANE_ID>}'`
  so the plugin can map uuid → pane → live tab position (spike **S2** verifies
  `ZELLIJ_PANE_ID` is exported; fallback: register-while-active heuristic).

### 6.2 State store + `clave ls`
**Goal:** the thin index everything reads/writes; `ls` prints agents + status.
**Decided:** format/locking/atomicity per §5. `clave ls` reads the store and prints
agents (grouped by repo, recency-sorted) with a glyph; `--json` emits the raw model;
`--archived` includes archived rows. `clave snapshot` emits the live `AgentSnapshot`
for plugin hydration.

### 6.3 `clave add` (the `Alt+a` flow)
**Goal:** pick a directory, open a tab, spawn-or-resume an agent, record it.
**Decided:**
- Pick a repo/dir with **`fzf` over `zoxide query -l`** (fzf present; skim isn't);
  default-select the current cwd.
- Consult the store for that `repo_root`:
  - **Already running in clave** (live, not archived) → jump to it; no duplicate spawn.
  - Otherwise offer **new** vs **resume**:
    - *new:* mint UUID → derive label (§6.4) → create a tab whose pane command is
      `clave spawn <uuid> --name <label> --cwd <dir>` → record.
    - *resume:* **clave owns the picker** — `fzf` over the repo's resumable sessions
      (its own store rows, incl. archived, plus prior Claude sessions discovered by
      scanning `~/.claude/projects/<munged-cwd>/*.jsonl` and deriving a label per
      §6.4). Pick → known UUID → create a tab with the **same idempotent**
      `clave spawn <uuid> …` command → record/unarchive.
  - *Rejected:* letting `claude --resume` show its own picker. It's tempting (no
    picker to build), but the UUID would only be known *after* launch (via the
    `SessionStart` hook), leaving the pane command as `claude --resume` — which
    re-shows the picker on Zellij resurrect instead of resuming, breaking invariant
    #5. clave owning the picker keeps every tab's command `clave spawn <uuid>`.
- **Launch surface (`Alt+a`):** `clave add` is an interactive `fzf` flow, so it needs
  a real TTY. `Alt+a` is therefore a Zellij **`Run` into a floating pane**
  (`Run "clave" "add" { floating true; close_on_exit true }`) — **not** a
  `MessagePlugin` (a plugin can't host a TTY picker). This is distinct from the
  plugin-message nav keys (§6.6).
- **Tab creation (dynamic UUID):** Zellij KDL layouts don't do variable substitution,
  so `clave add` writes a **one-shot temp layout** (`$TMPDIR/clave-<uuid>.kdl`) with
  the `[ clave-bar | claude ]` template and the pane command
  `clave spawn <uuid> --name <label> --cwd <dir>` baked in, then runs
  `zellij action new-tab --layout <that-file>` and deletes it. Baking the command into
  the layout is also what makes it survive resurrection (spike **S4**).
- **Worktree is opt-in, default off.** clave shells out to `git worktree add` itself
  (not Claude's `-w/--worktree`, §4) so it **owns the worktree path** — needed to
  compute the munged jsonl path for the idempotency check and to record `worktree` in
  the store. Auto-cleanup deferred, but the path is recorded (don't inherit T3's
  silent gap).

### 6.4 Naming
**Goal:** a glanceable, self-updating label.
**Decided:**
- Label = **`cwd · branch · <first words of first user message>`** (cwd first),
  upgraded to the first words of the session `summary` once written.
- Refresh is **hook-driven re-derive**, with a fast path. The label only meaningfully
  changes when the `summary` first appears, so on `Stop`/`UserPromptSubmit` `clave
  hook` re-reads the jsonl **only while `label_source == first_prompt`**, and reads
  the **tail** rather than the whole file (jsonl grows unbounded — a full re-scan every
  turn risks the hook timeout). Once a summary is found it sets `label_source =
  summary` and stops re-scanning. Then update the store + push the snapshot —
  event-driven, no watcher (invariant #4).
- Truncation is the **plugin's** job: `clave-bar` clamps each row to the bar width
  (~22 cols, configurable) with a trailing `…`, prioritising the summary segment.
- One-shot LLM titles: deferred.

### 6.5 Status + `clave hook <event>`
**Goal:** translate hook events into per-agent status, keyed by the UUID we own.
**Decided:**
- `clave hook <event>` reads the hook JSON from stdin and maps `session_id` → agent.
  If the session is **untracked** it exits 0 immediately on a **lock-free** read
  (§5) — clave must never delay or perturb other sessions. It **never emits a
  permission decision**: a `PermissionRequest`/PreToolUse hook *can* approve/deny tool
  use, so clave's handler is strictly pass-through and any internal error still exits
  0 (a global hook must not become a machine-wide failure point).
- **Status is a latest-wins state machine, not a monotonic max.** Each event maps
  directly to the new status, so a later lower-"priority" event can downgrade an
  earlier one (else `needs_you` would stick red after you've answered). Transitions:
  `UserPromptSubmit → working` (bump `last_interacted`) · `Stop → done` ·
  `StopFailure → failed` · `Notification[permission_prompt|idle_prompt] → needs_you` ·
  `PermissionRequest → needs_you` · `SessionEnd → idle`. The order
  `needs_you > working > done` is only a **tie-break** for genuinely simultaneous
  distinct signals. After computing status, update the store and push the snapshot.
  (The exact payload field/value to match for `Notification` is captured live in spike
  **S1**; §4's `permission_prompt|idle_prompt` matches against the notification
  message text.)
- **Status = one glyph, colour encodes state** (rendered by the plugin via `#[fg]`):
  `●` red = needs you · `●` amber = working · `●` green = done & unread · `●` dim =
  idle · `✖` red = failed. (Glyph set is a config default; tweakable.)
- **Unread:** `done` shows green until the agent's tab gains focus; on focus the
  plugin bumps `last_visited` (via `clave focus <uuid>`) and the row falls to idle
  dim. The plugin detects focus from `TabUpdate`/`PaneUpdate` (spike **S3**).
- `clave setup` **additively and idempotently merges** the hooks into
  `~/.claude/settings.json`, preserving any existing hook arrays (never clobber the
  user's `SessionStart`/`PreToolUse`/etc.). On this machine `~/.claude` is a
  **symlink into the dotfiles repo** (`~/.claude → ~/dotfiles/src/.claude`), so that
  merge lands in the source tree automatically — `just bootstrap` afterward is a
  personal-workflow nicety, not something the tool bakes in. Machine-specific dotfiles
  mechanics stay out of the canonical design.

### 6.6 Bar + keybinds (`clave-bar` plugin)
**Goal:** the vertical left bar, its ordering/grouping, and the `Alt` keys.
**Decided:**
- **First-party `clave-bar`** WASM plugin renders from clave's pushed `AgentSnapshot`
  (invariant #11): **group by `repo_root`** (a dim repo-header row, tinted with a
  stable per-repo colour) → **within group, sort by `last_interacted` desc**. Each
  row: status glyph (state colour) + label (cwd in the repo colour · branch dim ·
  summary). Archived agents are not rendered.
- **uuid→tab join (spike S2):** the plugin keeps a `uuid → pane_id` map from
  `clave-register` messages; at render/selection it finds the tab containing that
  pane and its live position → `go_to_tab`.
- **Navigation routes through the plugin** so it follows *display* order, not tab
  order: keybinds use `MessagePlugin "clave-bar" { name:"nav"; payload:… }`. The
  plugin computes the target row and calls `go_to_tab`.
- **Keybinds** (in the clave session's own config, `shared_among "normal" "locked"`):
  `Alt+a` add (Zellij `Run` → floating `clave add`, §6.3) · `Alt+c` toggle bar ·
  `Alt+w` archive focused agent · `Alt+↑/↓` and `Alt+j/k` navigate agents (display
  order, via `MessagePlugin` → plugin) · `Alt+1…9` jump to the Nth displayed agent
  (agent rows only, skipping repo-header rows). Keep the user's existing `Alt+h/l` and
  `Alt+y`.
- **Context-sensitive nav (goal, spike S6):** when the bar is visible, `Alt+j/k/↑/↓`
  navigate agents; when hidden (`Alt+c`), they fall back to normal Zellij focus
  movement. The plugin owns this branch (it knows its own visibility); if visibility
  detection proves fiddly, **fallback** = nav always means "navigate agents" inside
  the clave session.
- Per-row colour tint: **included** (per-repo cwd colour + state-coloured glyph),
  since the plugin renders natively. Context-battery glyph: deferred (§10).

### 6.7 Archiving
**Goal:** keep the bar bounded; an ever-growing list is unusable in this format.
**Decided:**
- **Archive** (`Alt+w` / `clave archive <uuid>`) = close the agent's Zellij tab +
  set `archived:true` in the store. Claude's jsonl persists, so it stays resumable.
- The bar shows only **active** (non-archived, live) agents → bounded.
- **Restore** = the `add`/resume picker (§6.3) surfaces archived sessions for the
  repo; resuming one re-creates a tab (`clave spawn` resume path) and clears
  `archived`.
- Auto-archive (e.g. idle > N days) is **deferred**; v1 is manual.

### 6.8 Session model & keybind scoping
**Goal:** isolate clave from the user's normal Zellij/Claude environment.
**Decided:**
- Agents live in a **dedicated `clave` Zellij session**, launched with a clave-owned
  config + `agents` layout (`zellij --config … --layout … attach -c clave`). The
  `clave` shell command attaches-or-creates it.
- **Keybinds live only in the clave session config** (invariant #12) — the user's
  global Zellij config and other users' configs are untouched.
- **Hooks remain global** (Claude limitation) but no-op fast for untracked sessions.

---

## 7. Build, workspace & packaging

One Cargo workspace, two build artifacts, shared types — **co-versioned, released
together** (the shared crate is the anti-drift mechanism, invariant #9):

```
clave/
├─ Cargo.toml            # [workspace] members; default-members = ["crates/clave"]
├─ crates/
│  ├─ clave/             # bin · native host target · add/spawn/hook/ls/setup/...
│  ├─ clave-bar/         # binary crate (src/main.rs + register_plugin!) · dep zellij-tile · wasm32-wasip1
│  └─ clave-types/       # serde-only, target-agnostic · the pipe schema
└─ justfile             # build/install orchestration
```

- `clave-types` depends on nothing but `serde` → compiles for **both** host and wasm;
  both artifacts use the same structs for the pipe payload.
- Two targets: `cargo build -p clave --release` (host) and
  `cargo build -p clave-bar --release --target wasm32-wasip1`
  (`rustup target add wasm32-wasip1`). `clave-bar` is excluded from `default-members`
  so a plain host `cargo build` doesn't try to compile the WASM-only crate.
- Install destinations differ: the `clave` binary → PATH; `clave-bar.wasm` → a fixed
  path the layout references (`plugin location="file:~/.local/share/clave/clave-bar.wasm"`).
  `just install` copies both; `clave setup` writes/points the layout and additively
  merges hooks into `~/.claude/settings.json` (§6.5).
- Distribution (mechanism deferred): ship `clave-bar.wasm` as a release artifact; a
  future `cargo install clave` can `include_bytes!` the wasm and have `clave setup`
  extract it, so one install delivers both. Dev uses the file path directly.

---

## 8. End-to-end data flow

```
Alt+a → clave add ──┐ pick repo (fzf/zoxide) · new|resume · mint/pick uuid
                    │ derive label · create tab · record · push snapshot
                    ▼
   clave session tab:  [ clave-bar plugin | claude pane: `clave spawn <uuid> …` ]
                    │                              │
   clave spawn pipes clave-register {uuid,pane_id}─┘
                    │
Claude hooks ──► clave hook <event>   (stdin JSON; session_id == uuid)
                    │ update store (lock+atomic) · map status · bump recency
                    ▼ zellij pipe --name clave-status -- <AgentSnapshot>
            clave-bar ──► group by repo · sort by recency · render glyph+colour
                    ▲ Alt nav → MessagePlugin clave-bar → go_to_tab
                    └ focus change → clave focus <uuid> → clears unread
```

---

## 9. Spike plan (validate-first, in order)

Each spike has a clear pass/fail and a fallback. **S0/S0b gate the join key**
(idempotency breaks without them); **S1 gates the plugin architecture** (if it fails
we revisit §3).

- **S0 — `--session-id` create semantics.** `claude --session-id <fresh-uuid>` in a
  clean cwd *creates* a new session and writes
  `~/.claude/projects/<munged-cwd>/<uuid>.jsonl`; also observe what a *pre-existing*
  UUID does (error vs resume). The whole idempotency model (§6.1) rests on this.
- **S0b — cwd munging.** Round-trip several cwds (plain, dotted, worktree under
  `.claude-worktrees`) through the munging helper and confirm the computed jsonl path
  matches disk. Locks the rule in §4 (`s/[^A-Za-z0-9]/-/g`).
- **S1 — background repaint.** A `clave-bar` skeleton, loaded in the layout, renders a
  status glyph for a **non-focused** agent's row and updates it on a `zellij pipe`
  message, without stealing focus. *Pass:* the inactive row's glyph/colour changes
  live. *Fallback:* reconsider rename_tab-based painting / fork.
- **S2 — uuid→pane join.** Confirm `$ZELLIJ_PANE_ID` is exported to the pane process;
  `clave spawn` pipes `clave-register {uuid,pane_id}`; the plugin maps it to the tab
  and `go_to_tab`s correctly after tabs are reordered/closed. *Fallback:*
  register-while-active heuristic, or match on pane cwd/title.
- **S3 — focus → unread.** The plugin detects the active-tab change and calls
  `clave focus <uuid>`; the green "done & unread" row falls to idle. *Fallback:* clear
  unread on the next `UserPromptSubmit` for that agent.
- **S4 — tab creation + resurrection.** `clave add` builds a tab via a one-shot temp
  layout (§6.3) whose pane command is `clave spawn <uuid>`; that command re-runs on
  Zellij resurrect and resumes. Also test **cold-start reconciliation**: after a full
  restart the plugin's uuid→pane map is empty and every pane sits behind the "Press
  ENTER" gate — confirm the map rebuilds (each resumed `clave spawn` re-registers, or
  the plugin rebuilds from `PaneManifest`) and status recovers. *Fallback:*
  `clave rebuild` from the store.
- **S5 — plugin hydration.** On (re)load, `clave-bar` hydrates via `clave snapshot`
  (`RunCommands`). *Fallback:* clave re-pushes the full snapshot on a timer/first hook.
- **S6 — context-sensitive nav.** The plugin can tell whether the bar is
  visible and branch nav (agents vs Zellij focus). *Fallback:* nav always = navigate
  agents inside the clave session.

---

## 10. v1 scope / deferred / risks

**v1:** dedicated `clave` session + `agents` layout · first-party `clave-bar`
(repo-grouped, recency-sorted, glyph+colour status, per-repo cwd colour) · `clave
spawn` (idempotent) · `clave add` (zoxide picker; new|resume via clave's own picker;
worktree opt-in default-off) · status hooks → `clave hook` → snapshot push ·
archiving (manual) · keybinds `Alt+a/c/w`, `Alt+↑/↓`, `Alt+j/k`, `Alt+1…9`
(clave-session-scoped).

**Deferred:** `clave rebuild` (cold/remote start) · one-shot LLM titles · auto-archive
· worktree auto-cleanup · `AskUserQuestion`-wait state · per-agent remote `BreakPane`
· archived-view toggle in the bar · `cargo install`/`include_bytes!` packaging ·
**per-agent context-% battery** (below).

**Backlog — context battery:** show each active agent's context-window usage in the
bar as a depleting battery glyph (🔋 → 🪫). Per-turn token/usage data lives in the
session jsonl; map cumulative usage → % of the model's context window → glyph. Lift
the extraction logic from the user's **`rot-reducer`** project
(`~/code/rot-reducer`), which already pulled context reporting out of Claude logs —
start there.

**Known limitation — resurrection friction:** Zellij's serialization gate needs one
"Press ENTER" per pane on cold restart, so an N-agent fleet needs N confirmations
before agents reconnect (spike **S4** covers *recovery* once run, not the gate
itself). Mitigations to evaluate later: a `clave rebuild` boot path instead of relying
on serialization, or a Zellij option to skip the gate if 0.44.3 exposes one.

**Risks to validate early:** all of §9, especially **S0** (`--session-id` create) and
**S0b** (munging) — the join key breaks without them — then **S1** (background
repaint) and **S2** (pane join). Terminal sends Option-as-Meta — existing `Alt` binds
already work, so almost certainly fine.

---

## 11. References

- T3 Code (Electron prior art): <https://github.com/pingdotgg/t3code>
- Zellij vertical tabs (reference, MIT): <https://github.com/cfal/zellij-vertical-tabs>
- Zellij plugin API: <https://zellij.dev/documentation/plugin-api.html> ·
  `zellij-tile`: <https://docs.rs/zellij-tile/latest/zellij_tile/>
- Zellij keybinding actions (`MessagePlugin`, `GoToTab`):
  <https://zellij.dev/documentation/keybindings-possible-actions.html>
- Claude Code hooks: <https://code.claude.com/docs/en/hooks>
- Programmatic name/colour request: [claude-code#58588]
- `AskUserQuestion` hook gap: [claude-code#59908]
- Cross-tab rename requests: [zellij#4591], [zellij#4602]
