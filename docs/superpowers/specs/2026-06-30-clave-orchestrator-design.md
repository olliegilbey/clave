# clave — canonical design & implementation spec

> Terminal-native orchestration for a fleet of Claude Code agents, driven from a
> Zellij sidebar.

**Status:** decisions **locked** (post-brainstorm, 2026-06-30; **revised
2026-07-03** — the "vertical dynamic tabs" reframing, below). This is the single
canonical spec; it supersedes the original `docs/design.md` brief, which now points
here. Foundation + spikes S0/S0b/S1/S2 are implemented and **PASS** (§9).

> **Revision 2026-07-03 (post-spikes reframing, brainstormed + approved):** the bar
> is now *the tab bar, made vertical* — its **row set is Zellij's live tab list**
> (every tab, Claude agent or plain terminal), its **order is interaction recency**,
> and clave's pushed status is a **decoration layer** (glyph + colour) on agent rows.
> Repo-grouping, the archiving subsystem, and context-sensitive nav (S6) are
> deleted; clave's labels are written onto the **real tabs** via the plugin.
> Changed: §1, §2 #2/#11, §3, §4 (Zellij facts), §5, §6.2/§6.4/§6.5/§6.6/§6.7/§6.8,
> §7–§10.

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
the *real* Claude Code TUI — and a tab may equally be a **plain terminal** (clave
doesn't care); a **vertical left bar** (a first-party Zellij plugin, `clave-bar`)
lists **all tabs of the session**, sorted by interaction recency, agent tabs
decorated with a colour-coded status glyph. You add and jump between them with `Alt`
keys that fire even while a Claude session has focus, or by **clicking a row**. The
bar hides/shows on `Alt+c`; everything else stays stock Zellij — floating panes
(e.g. nvim over the working agents), splits, session-manager all keep working.

```
┌──────────────────────────┐
│ ● clave·main·spawn-cmd    │  ← focused tab (row 1 = most recent, by construction)
│ ● dots·main·fix-auth      │  ● red   = needs you
│ ● dots·feat·add-navbar    │  ● amber = working
│   ~/scratch (zsh)         │  ← plain terminal tab: name only, no glyph
│ ● clave·docs·readme       │  ● green = done & unread
│ ✖ clave·main·flaky-test   │  ✖ red = failed · ● dim = idle
└──────────────────────────┘
```

The status indicator is a **single glyph whose font colour encodes state** (not an
emoji — emoji render inconsistently in the bar).

The name `clave`: the foundational rhythm an ensemble locks to (the orchestrator);
Spanish for *key/keystone* (keyboard-driven, central); archaic past tense of
*cleave*, to split (panes). Logo: the two-stick percussion clave.

---

## 2. First principles (invariants)

1. **The agent is the real Claude TUI.** Never parse or scrape its rendered output.
   Users keep vim mode, slash commands, everything.
2. **`clave` owns identity.** The label (`cwd · branch · summary`) and colours are
   computed by `clave`; the label is written onto the **real Zellij tab**
   (`rename_tab_with_id`, on label *change* only — so manual renames stick between
   changes) and the bar renders `TabInfo.name` for every row. The launch `--name` is
   a courtesy push into Claude's own session list; never read `/rename` or `/color`
   back — they are not exposed (see §4).
3. **The minted UUID is the join key.** `clave` generates each session's UUID and
   passes it to `--session-id`. That UUID locates the transcript, correlates every
   hook event, and joins the store row to its Zellij pane. Everything keys off it.
4. **Status is event-driven, never polled.** Derive state from Claude hooks and turn
   lifecycle — not from screen text or timers.
5. **Spawn is idempotent → restart-safe.** (Revised 2026-07-17, C8 redesign.) The
   pane command resumes-or-creates by UUID. Zellij session serialization is OFF
   (§6.8) — it serializes the live *discovered* process (`claude --session-id`,
   or even a mid-tool-call child), not the baked command, so it can never be the
   resume path. Restart-safety is **clave-owned**: launch eager-loads the
   most-recent agent, every other store row is a dormant bar row that opens on
   settled focus (§6.6/§6.3 `clave open`). Idempotence is what makes eager-load,
   `open`, and double-fires all safe.
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
   (`clave`, subcommands `add`/`open`/`spawn`/`hook`/`ls`/`focus`/`snapshot`/
   `setup`/`dev`)
   and a WASM plugin
   (`clave-bar`) share a `clave-types` crate so the pipe schema cannot drift (§7).
10. **Keep the core provider-shaped, not Claude-welded.** Confine Claude specifics to
    the spawn + hook adapter; the status model and bar logic stay generic enough that
    another agent CLI could slot in later.
11. **The bar's row set is Zellij's truth ∪ the store's memory; order and
    decoration are clave's.** (Revised 2026-07-17, C8: dormant rows.) Live rows
    come from `TabUpdate` — every tab, agent or plain, appears and vanishes for
    free. Store rows with no live tab render as **dormant** conversation rows
    (claude.ai-style list). Display order is **interaction recency**, not tab
    order. clave's pushed status decorates agent rows (glyph/colour). Selection
    maps a displayed row → the tab's focused pane → `focus_pane_with_id` (S2:
    `go_to_tab` is a dead end); a dormant row has no pane — settled focus opens
    it (§6.6).
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
| Painter plugin + stock `cfal/zellij-vertical-tabs` | Rejected | Stock cfal renders in **tab order** with no sort concept (§4). Cannot do recency-sort (still required, 2026-07-03) or per-row status decoration from pushed state. |
| Fork cfal | Rejected | Its architecture mirrors tab order via a format-string DSL; our model (clave-owned order + decoration) fights it; external repo → no shared types; fork drift. |
| **First-party `clave-bar` WASM plugin** | **Chosen** | Owns row order + status decoration (invariant #11, revised); shares `clave-types` with the binary (invariant #9); single repo, no drift; reference cfal (MIT) for technique only. |
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
- **Plugin API can rename any tab without stealing focus:**
  `rename_tab(tab_position, name)` / **`rename_tab_with_id(tab_id, name)`** —
  permission `ChangeApplicationState`. (**We use the `_with_id` form** — 2026-07-03 —
  to write clave's label onto the real tab; the plugin layer is the only mechanism
  that can touch background tabs.)
- **Plugins receive `zellij pipe` messages** via the `pipe()` trait method
  (`CliPipeInput`) — permission `ReadCliPipes`. `clave hook` pushes status with
  `zellij pipe --name clave-status -- '<json>'`.
- **Keybinds can message a plugin:** `MessagePlugin "file:…wasm" { name; payload }`
  (and `MessagePluginId <n> { … }`). This routes `Alt`-nav keys to `clave-bar` while
  Claude is focused, so the bar's *display order* (not tab order) drives navigation.
- Native tab nav: `GoToTab <n>`, `GoToNextTab`, `GoToPreviousTab`.
- **Plugin nav (S2 verdict):** plugin `go_to_tab(index)` is a **silent no-op** in
  practice — fed the correct live value from both keybind and CLI contexts, it never
  switched (0-/1-based mismatch). The proven call is
  **`focus_pane_with_id(PaneId::Terminal(pane_id), false, false)`** — focus the
  pane; Zellij pulls its tab forward. `switch_tab_to(idx)` (what the stock tab-bar's
  click handler uses) is plausible but untested here — candidate simplification only.
- **Tab/pane truth for plugins** (verified against zellij-utils 0.44.3 `data.rs`,
  2026-07-03): `TabUpdate → Vec<TabInfo>` (`position`, `name`, `active`, stable
  `tab_id`); `PaneUpdate → PaneManifest` (`HashMap<tab_position, Vec<PaneInfo>>`;
  `PaneInfo { id, is_plugin, is_focused, … }`) — both need `ReadApplicationState`.
  `Mouse::LeftClick(line, col)` events reach plugin panes (row-click nav).
  `hide_self()` / `show_self(bool)` exist (bar toggle; grid-reflow needs one live
  check). `rename_tab_with_id(tab_id, name)` renames any tab by stable id.
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
  "repo_root":       "/abs/repo", // git toplevel of cwd; keys the add/resume picker (§6.3)
  "branch":          "main",
  "label":           "clave · main · spawn-cmd", // cwd · branch · summary (§6.4)
  "status":          "working",  // idle|working|needs_you|done|failed
  "last_interacted": 0,          // unix s; bumped on UserPromptSubmit → recency sort
  "last_visited":    0,          // unix s; bumped on focus → unread = done & !visited
  "worktree":        null,       // path if spawned in a git worktree, else null
  "label_source":    "first_prompt", // first_prompt|summary; once summary, stop re-scanning jsonl (§6.4)
  "tab_id":          4,          // zellij tab hosting the agent (§6.6 B); null until bound; session-scoped
  "stale":           false       // 2026-07-17: `clave open` found cwd missing → bar ✗; NOT a status
                                 // (statuses are hook lifecycle); cleared by a later successful open
}
```

Beside `seq` and the agent map, the store holds `tab_timeline` (tab_id →
unix s of the last user commitment, §6.6): written only by `clave touch`
(locked RMW, max-merge) and the hook's prompt stamp, carried on every
`AgentSnapshot`, replaced wholesale by each bar instance. The agent `tab_id`
bind (written by `clave bind`, reported once by the agent tab's own bar) is
how prompts reach the timeline and how every bar joins glyphs — local
register/manifest joins diverge per instance (round 6). tab_ids are
session-scoped → bare `clave` clears both the timeline and all binds when it
creates (not re-attaches) the session.

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
agents recency-sorted (repo as a column) with a glyph; `--json` emits the raw model.
`clave snapshot` emits the live `AgentSnapshot` for plugin hydration (the bar
self-hydrates on load via `RunCommands` — §6.6).

### 6.3 `clave add` (the `Alt+a` flow)
**Goal:** pick a directory, open a tab, spawn-or-resume an agent, record it.
**Decided:**
- Pick a repo/dir with **`fzf` over `zoxide query -l`** (fzf present; skim isn't);
  default-select the current cwd.
- Offer **new** vs **resume** (revised 2026-07-14, C7: MANY agents per repo —
  the old "repo has a live agent → auto-jump" rule made a second agent in the
  same repo impossible and only went unnoticed because the liveness check was
  blind, see below):
    - *new:* mint UUID → derive label (§6.4) → create a tab whose pane command is
      `clave spawn <uuid> --name <label> --cwd <dir>` → record.
    - *resume:* **clave owns the picker** — `fzf` over the repo's resumable sessions
      (its own store rows, plus prior Claude sessions discovered by
      scanning `~/.claude/projects/<munged-cwd>/*.jsonl` and deriving a label per
      §6.4). Currently-LIVE agents are included but MARKED (`▶`): picking one
      JUMPS to its tab (`clave-nav` uuid pipe, S2) — resuming a live session
      opens it twice (found live, round 7). Dead pick → known UUID → create a
      tab with the **same idempotent** `clave spawn <uuid> …` command → record.
  - Liveness check: uuids greppable from `zellij action dump-layout`. C7
    finding (2026-07-14): zellij serializes the LIVE pane process, not the
    baked layout command — after `clave spawn` execs, that's
    `claude --session-id <uuid>`/`--resume <uuid>` (parser matches all three
    forms). CRITICALLY, a fire-and-forget child spawned pre-exec became a
    permanent ZOMBIE under claude, and zellij serialized the pane as
    `command="<defunct>"` — blinding liveness AND resurrection; the register
    pipe is therefore double-forked (reparented to init, nothing left in the
    pane's tree).
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
  `zellij action new-tab --layout <that-file>` and deletes it. Baking the command in
  makes tab creation **idempotent** — resurrection is clave's job, not zellij's
  (revised 2026-07-17; the old "survives resurrection" premise was false: zellij
  serializes the live discovered process, see §6.8/S4).
- **`clave open <uuid>` (added 2026-07-17, C8):** the non-interactive sibling of
  `add`, invoked by the bar (executor instance, `run_command`) when a dormant row's
  focus settles, and by explicit picks. No picker — the row is the choice. Flow:
  store row lookup → **liveness no-op guard** (uuid in `dump-layout` per
  `live_uuids` → do nothing; protects against dwell-timer/click double-fires) →
  **staleness check** (row `cwd` missing on disk → no tab; set the row's `stale`
  flag (§5) and push the snapshot so the bar shows ✗; a later successful open
  clears it; recovery manual for now) → one-shot temp
  layout (same `tab_layout`) → `zellij action new-tab --layout`, targeted at the
  clave session via explicit env (never ambient). A vanished jsonl is NOT an
  error: spawn's existence branch simply creates a fresh conversation under the
  same uuid — accepted quirk. Worktree rows bake the worktree cwd (store row is
  worktree-aware, Task 7).
- **Worktree is opt-in, default off.** clave shells out to `git worktree add` itself
  (not Claude's `-w/--worktree`, §4) so it **owns the worktree path** — needed to
  compute the munged jsonl path for the idempotency check and to record `worktree` in
  the store. Auto-cleanup deferred, but the path is recorded (don't inherit T3's
  silent gap).

### 6.4 Naming

> **⚠ This section's summary tier is FALSIFIED — LEDGER D23** (`docs/ux/LEDGER.md`,
> the authority). The `{"type":"summary"}` line the `label_source == summary`
> upgrade waits for appears in **0 of 153** real transcripts; `ai-title` is what
> Claude Code writes, and it **does not roll** (D24). The grammar and the width
> below are also superseded by S4 and the design lock. Left in place so the belief
> is visible — **do not amend it here**; propose a disposition to the coordinator.

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
- **Delivery (2026-07-03):** the label rides the snapshot; `clave-bar` writes it onto
  the **real tab** via `rename_tab_with_id`, and only when the label *changes*
  (tracked per-uuid) — no rename↔`TabUpdate` loop, and manual tab renames stick
  until the next genuine label change. The bar renders `TabInfo.name` for every row,
  so agent and plain tabs share one render path.
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
  `StopFailure → failed` · `Notification[permission_prompt] → needs_you` ·
  `Notification[idle_prompt] → needs_you` **only if currently `working`** ·
  `PermissionRequest → needs_you` · `SessionEnd → idle`. The order
  `needs_you > working > done` is only a **tie-break** for genuinely simultaneous
  distinct signals. After computing status, update the store and push the snapshot.
  (The exact payload field/value to match for `Notification` is captured live in spike
  **S1**; §4's `permission_prompt|idle_prompt` matches against the notification
  message text.) **Idle-prompt discriminator (revised 2026-07-08, C-validation,
  supersedes the 2026-07-06 keep-decision):** the CLI fires "waiting for your
  input" ~60s after EVERY turn. Red must mean *blocked mid-turn* (permission
  prompt, in-turn question/plan approval — the turn is still open, no Stop
  yet, status `working`). For a finished agent (done/idle) the same
  notification is only an idle nag and is swallowed — a completed turn is
  already fully told by green-until-read → grey; always-red after 60s
  destroys red's fleet-triage value.
- **Status = one glyph, colour encodes state** (rendered by the plugin via `#[fg]`):
  `●` red = needs you · `●` amber = working · `●` green = done & unread · `●` dim =
  idle · `✖` red = failed. (Glyph set is a config default; tweakable.)
- **Unread:** `done` shows green until the agent's tab gains focus. (Revised
  2026-07-08, C3 live finding:) zellij delivers `TabUpdate` ONLY to the active
  tab's plugin instance, so a focus *transition* is unobservable — receiving a
  `TabUpdate` with a Done agent in the active tab IS the focus signal. That
  instance runs `clave focus <uuid>` (`RunCommands`; exactly-once via the
  local read-override + the delivery rule), which bumps `last_visited`, flips
  the store row to idle, and **pushes a snapshot** so every hidden bar learns
  the flip — bar and `ls` agree. Non-fatal on failure (self-heals on the next
  push). (Was spike S3.)
- `clave setup` **additively and idempotently merges** the hooks into
  `~/.claude/settings.json`, preserving any existing hook arrays (never clobber the
  user's `SessionStart`/`PreToolUse`/etc.). On this machine `~/.claude` is a
  **symlink into the dotfiles repo** (`~/.claude → ~/dotfiles/src/.claude`), so that
  merge lands in the source tree automatically — `just bootstrap` afterward is a
  personal-workflow nicety, not something the tool bakes in. Machine-specific dotfiles
  mechanics stay out of the canonical design.

### 6.6 Bar + keybinds (`clave-bar` plugin)
**Goal:** the vertical, hideable, mouse-clickable tab bar with live status decoration.
**Decided (revised 2026-07-03):**
- **Three separated concerns:** row **set** = `TabUpdate` (all tabs, live; closed
  tabs vanish for free) **∪ dormant store rows** (revised 2026-07-17: snapshot
  agents with no live tab join — one unified conversation list, claude.ai-style;
  source-agnostic, so jsonl adoption slots in later) · row **order** =
  interaction recency · row **decoration** = clave's pushed status (S1 pipe
  contract unchanged).
- **Rows:** status glyph (agent tabs only; state colour) + `TabInfo.name`, clamped to
  the bar width (~24 cols, configurable) with a trailing `…`. Plain tabs render
  name-only. Focused row highlighted. No repo grouping, no per-repo colours
  (deleted). **Dormant rows** (2026-07-17): ◌ glyph, dimmed, label from the
  store row; transient ↻ while an open's spawn is in flight (flips to live via
  the normal register/TabUpdate path — no bespoke completion signal); ✗ = stale
  (open found the cwd missing). A dormant row that gains a tab becomes a live
  row with the same uuid key — no handoff state.
- **Order = last USER COMMITMENT (revised 2026-07-08 after C4/C5 live rounds;
  user-ratified "Claude-desktop" model):** rows sort by one unified timeline
  in unix seconds — when did the user last commit input to that tab. **Focus
  never reorders**; the list holds still while you look around and navigate
  (this is what makes walking the displayed order stable — no ping-pong).
  The sort key is the STORE's `tab_timeline` map (tab_id → unix s) and
  NOTHING else — no render-time joins (revised 2026-07-14 twice: C5 rd 5
  killed instance-local pipe-delta merges; rd 6 killed the render-time
  `last_interacted` join — register pipes don't replay and hidden manifests
  go stale, so per-instance joins diverge and walking alternated between the
  two agent tabs). The map is written only under the store lock and carried
  on EVERY `AgentSnapshot`; each bar REPLACES its copy from each seq-gated
  snapshot — the one channel that has never diverged. Writers:
  - **Birth**: the active instance fires ONE `clave touch <tab_id>` per tab
    (first TabUpdate for a tab neither the snapshot timeline nor its local
    fired-set knows; guard is local and never echo-dependent — C5 rd 4).
  - **Agent prompts (Design B)**: the store binds uuid→`tab_id`, reported
    ONCE by the agent tab's own bar (`clave bind`, active-instance-gated —
    the only fresh manifest; resume resets the bind, the new tab re-binds).
    The `UserPromptSubmit` hook then stamps `tab_timeline[bind]` atomically
    with the `last_interacted` bump — no bar round-trip, no switch-away
    race. The bind also keys every bar's GLYPH/rename/unread joins off the
    snapshot (fixes round 6's permanently-glyphless rows on late-loaded
    instances).
  tab_ids are session-scoped, so bare `clave` clears the timeline AND all
  binds when it is about to CREATE (not re-attach) the session.
  `InputReceived` is a DEAD END: it fires for every keystroke including nav
  keybinds (rd 4: focus-reorder + spawn storm + server fd exhaustion).
  Shell-command touches (`clave touch-pane` + preexec hook) are PARKED —
  user declined shell config; plain tabs order by birth only.
  Tie-break: tab position. A separate `clave-visited` beacon pipe tracks the
  focused tab purely for nav-executor election — it has NO ordering effect.
  **Dormant rows** (2026-07-17) join the same timeline by the store row's
  `last_interacted` (carried on the snapshot); they hold no tab commitment.
- **uuid→row join (spike S2 + `PaneManifest`):** `clave-register` gives
  `uuid → pane_id`; `PaneManifest` gives pane → tab position; `TabInfo` gives
  position → `tab_id`/name/active.
- **Nav (revised 2026-07-08, C5 rounds 1–3):** everything acts on the
  DISPLAYED list, which is safe to walk because focus never reorders it:
  - `Alt+↑/↓`/`Alt+j/k` step ±1 through the visible rows (wrapping);
    `Alt+1…9` = Nth visible row; clicks jump the clicked row. All use
    `switch_tab_to(position+1)` (the stock tab-bar's own mechanism; S2's
    `go_to_tab` dead end was an unchased indexing quirk) and run on the
    **executor only** — the instance whose own tab == the beacon
    (`clave-visited`-replicated focus). It is the active instance: fresh tab
    set, and the very bar the user is reading. Broadcast execution over
    hidden instances' stale sets raced six divergent targets live (rd 2).
  - **True alt-tab = native `ToggleTab` on `Alt+o`** (last two focused tabs,
    server-side truth). Alt+2's old alt-tab trick died by design: row 2 no
    longer swaps on focus.
  - **uuid jumps** keep `focus_pane_with_id` (S2): the pane id is broadcast
    truth, so every instance targets the same pane.
  - **Dormant rows open on SETTLED focus, not on touch (2026-07-17, C8).**
    Stepping onto a dormant row moves a **virtual selection cursor** (bar
    highlight) WITHOUT switching tabs — there is no tab to switch to. Each
    landing arms one `set_timeout(0.4)` (peek-timer pattern: only the last
    expiry acts); if the cursor is still on that row at expiry, the executor
    fires `run_command(["clave","open",<uuid>])` and shows ↻. Walking past a
    dormant row therefore never spawns it — this is what makes the unified
    list safe to walk. Subsequent nav steps continue from the cursor, which
    resolves back to the focused-tab row when the opened tab takes focus
    (`tab_layout` `focus=true`) or when nav lands on a live row. **Explicit
    picks skip the dwell**: clicks and `Alt+1…9` on a dormant row open
    immediately — explicit intent is unambiguous. The executor also keeps an
    in-flight set: a row already ↻ accepts no further opens (first guard;
    `clave open`'s liveness no-op is the second — belt and suspenders because
    `live_uuids` can transiently miss a mid-tool-call agent, §10). The 0.4s
    dwell is a named constant beside the 0.9s peek sink (both user-tuned;
    don't normalize).
    Nav ring caps (48h / max 10, numbered access to older rows) are DEFERRED
    to the jsonl-adoption phase (§10) — store-only row counts don't need them.
  - The bar pane is `set_selectable(false)` (stock tab-bar pattern): clicks
    reach the plugin without a focus-stealing first click, and `MoveFocus`
    skips the bar.
- **Other keybinds** (clave session config, `shared_among "normal" "locked"`):
  `Alt+a` add (Zellij `Run` → floating `clave add`, §6.3) · `Alt+w` close tab
  (native `CloseTab`, §6.7). Keep the user's existing `Alt+h/l` and `Alt+y`.
- **Toggle (`Alt+c`):** a `clave-toggle` pipe broadcast; each per-tab instance calls
  `hide_self()`/`show_self(false)`. Verify live: the grid reclaims the width, and
  hidden instances still receive pipes. Fallback: `close_self()` + relaunch keybind.
- **Instances:** one bar pane per tab (stock tab-bar pattern) via the session
  layout's tab template (§6.8). **Zellij event delivery (C3/C4 live finding,
  2026-07-08): `TabUpdate` reaches ONLY the active tab's instance** — hidden
  instances are event-starved, so per-instance transition detection and
  active-instance write-gating are impossible. Pipes ARE broadcast to all
  instances (buffered through plugin load; each CLI pipe also delivers one
  empty EOF message — dropped). Everything cross-tab therefore rides pipes:
  status snapshots, registration, visits. A bar-less tab (edge: a native
  new-tab that bypassed the template) still appears in every other tab's bar —
  the tab SET in any received `TabUpdate` is session-wide even though delivery
  is not.
- **Permissions:** `ReadCliPipes + ChangeApplicationState + ReadApplicationState +
  RunCommands` — the EXACT set `clave setup` pre-seeds under both key forms (§7;
  all-or-nothing grant, S1/S2).
- **Hygiene:** pipe handlers `eprintln!`-and-drop malformed payloads (zellij log);
  `unblock_cli_pipe_input` runs unconditionally on every path (the `dd38ace`
  pattern). Context-battery glyph: deferred (§10).

### 6.7 Archiving — DELETED (2026-07-03 reframing)
The bar is bounded by construction: rows are the session's live tabs plus the
store's rows (small for now — dormant rows, 2026-07-17; the jsonl-adoption phase
brings ring caps before the set grows unbounded, §10). Closing a tab (`Alt+w` →
native `CloseTab`) removes its live row; the store row persists and the row falls
back to dormant — the bar now surfaces the archive directly, and the §6.3 resume
picker remains for repo-scoped discovery. No `archived` flag in the store or pipe
schema (drop the existing `Agent.archived` field); no archive subsystem. Pruning
long-dead store rows: deferred.

### 6.8 Session model & keybind scoping
**Goal:** isolate clave from the user's normal Zellij/Claude environment.
**Decided:**
- Agents live in a **dedicated `clave` Zellij session**, launched with a clave-owned
  config + `agents` layout (`zellij --config … --layout … attach -c clave`). The
  `clave` shell command attaches-or-creates it.
- **`session_serialization false`** in the generated config (2026-07-17, C8).
  Zellij's serializer records the *discovered* pane process — post-exec that is
  `claude --session-id/--resume`, and mid-tool-call it is whatever child claude
  has (`ps -ao ppid,args` ppid-priority, zellij-server `pty.rs`
  `populate_session_layout_metadata`, v0.44.3) — so serialization-based
  resurrection replays commands that collide or are plain wrong. OFF entirely:
  no ENTER gates, no serialized-command repair. (`post_command_discovery_hook`
  rewriting was considered and rejected: eager restore of everything, per-tty-
  process `sh` forks every 60s tick, can't fix the tool-child case.)
- **Cold start is clave-owned (lazy)**: when the session is not live,
  `launch_session()` (1) best-effort `zellij delete-session --force clave` — an
  EXITED session with stale serialized state would be *resurrected* by
  `attach --create`, ignoring `--layout`; (2) composes the launch layout
  **dynamically**: the bar-only template, plus — if the store is non-empty —
  ONE tab for the most-recent row (`last_interacted`), pane command baked
  `clave spawn <uuid> …` (resumes via the jsonl check), written to a temp file
  and passed via `--layout`. Every other store row appears as a dormant bar row
  (§6.6). Empty store → today's behavior unchanged. Tab-timeline/bind clearing
  on create is unchanged (§5).
- The session layout defines a **tab template** (`[clave-bar (fixed width) | pane]`)
  so *natively* created tabs get the bar too; `clave add` tabs use the one-shot temp
  layout (§6.3). If `default_tab_template` proves parse-fragile (S1 note), fallback
  = a new-tab keybind that passes an explicit layout file.
- **Keybinds live only in the clave session config** (invariant #12) — the user's
  global Zellij config and other users' configs are untouched.
- **Hooks remain global** (Claude limitation) but no-op fast for untracked sessions.

### 6.9 `clave dev` — live-validation harness (added 2026-07-17)
**Goal:** one command puts the world into a named, repeatable state; the user
drives a real session; Claude reads structured logs. Real tabs, real spawns,
real jsonls — mock only the *content*. Minimal by design: a fixture-seeder plus
a log. No recorder, no assertion runner, no CI.
**Decided:**
- **Clave-state sandbox via env overrides** (REVISED 2026-07-18, live
  finding + user ruling), threaded through the existing path helpers:
  `CLAVE_SESSION` (default `clave`; harness uses `clave-test`),
  `CLAVE_STATE_DIR` (store), `CLAVE_DATA_DIR` (config/layout/wasm).
  **Claude's identity is deliberately NOT sandboxed**: the original
  `CLAUDE_CONFIG_DIR` isolation dragged authentication along with it
  (sandbox claude = "Not logged in"/stale-credential failures) — clave is
  a thin wrapper for terminal control and claude's identity is not its
  business. Scenario transcripts therefore land in the real
  `~/.claude/projects`, tagged by the deterministic `c85c…` uuids;
  `dev reset` removes exactly those. Hook events still land in the
  SANDBOX store because hook processes inherit `CLAVE_STATE_DIR` from
  their claude parent. Scenario repos are temp dirs (plus a real
  `git worktree` for worktree flows).
- **`clave dev scenario <name>`** seeds store rows + jsonls + dirs for a named
  state and prints exactly ONE command for the user to run next. Deterministic
  *readable* uuids (`c8s1-aaaa…`) so logs self-identify. Conversations are
  seeded cheaply but genuinely: `claude -p --session-id <uuid> "reply ok"` in
  the scenario cwd — a real resumable jsonl for a few tokens, so
  resume-with-history is verified for real. Scenario names map 1:1 to
  validation-checklist steps (`c8-cold-start`, `c8-worktree`, `c8-stale`, …).
- **Session control stays the user's**: the harness never launches or kills
  zellij sessions; it prints the env-prefixed commands. Sanctioned exception
  (user-ratified 2026-07-17): commands *explicitly env-scoped to `clave-test`*
  (seeding, `dump-layout` reads) are safe for Claude to run directly.
- **Observability:** every clave CLI invocation appends one JSON line —
  timestamp, argv, decision (e.g. `open: cwd missing → stale`), exit — to
  `<state>/clave.log`; the bar keeps `eprintln!` → zellij log. `clave dev
  status` dumps store + `live_uuids` + session liveness in one read. Per
  checklist step: user runs the step; Claude reads `dev status` + the two
  logs. Screenshots only for visual glyph checks.
- **`clave dev reset`** wipes the sandbox (store, jsonls, temp repos,
  worktrees) — printing the kill-session command for the user first.

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
  `just install` copies both; `clave setup` writes/points the layout, additively
  merges hooks into `~/.claude/settings.json` (§6.5), and **pre-seeds Zellij's
  `permissions.kdl`** with clave-bar's exact permission set under both key forms
  (§6.6 — grants are all-or-nothing and the prompt is unanswerable in the bar pane;
  S1/S2).
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
            clave-bar ──► rows = TabUpdate · order = recency · glyph on agent rows
                    │      label change → rename_tab_with_id (the real tab)
                    ▲ click / Alt nav → MessagePlugin clave-nav → focus_pane_with_id
                    └ focus change (TabUpdate) → clave focus <uuid> → clears unread
```

---

## 9. Spike plan (validate-first, in order)

Each spike has a clear pass/fail and a fallback. **S0/S0b gate the join key**
(idempotency breaks without them); **S1 gates the plugin architecture** (if it fails
we revisit §3).

> **Status (2026-07-03): S0/S0b/S1/S2 all PASS** (findings in
> `docs/superpowers/spikes/`; mechanism deltas folded into §4/§6 — notably nav is
> `focus_pane_with_id`, not `go_to_tab`). **S3 and S6 are deleted** by the reframing
> (focus detection is native `TabUpdate`; nav keys always walk the bar). **S4 and S5
> remain**, demoted to in-plan validation checkpoints, joined by three new small
> ones: `hide_self` grid-reflow (§6.6), tab-template robustness (§6.8), and the
> `switch_tab_to` simplification attempt (§4).

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
- **S4 — tab creation + resurrection.** (REDESIGNED 2026-07-17, C8: the original
  premise — the baked `clave spawn` re-runs on Zellij resurrect — was FALSE.
  Zellij serializes the *discovered* live process (ppid-priority `ps` scan): the
  exec'd `claude --session-id <uuid>` (collides on re-run) or even a mid-tool-
  call child like `cargo build`. Resolution = the old fallback, promoted:
  resurrection is clave-owned.) Serialization OFF; cold start = bar + eager
  most-recent tab (§6.8); every other store row is a dormant bar row opening on
  settled focus via `clave open` (§6.6/§6.3). Cold-start reconciliation is
  per-open: each resumed spawn re-registers its pane; the bar hydrates from
  `clave snapshot` (S5).
- **S5 — plugin hydration.** On (re)load, `clave-bar` hydrates via `clave snapshot`
  (`RunCommands`). *Fallback:* clave re-pushes the full snapshot on a timer/first hook.
- **S6 — context-sensitive nav.** The plugin can tell whether the bar is
  visible and branch nav (agents vs Zellij focus). *Fallback:* nav always = navigate
  agents inside the clave session.

---

## 10. v1 scope / deferred / risks

**v1:** dedicated `clave` session, bar in every tab via the layout's tab template ·
first-party `clave-bar` (all-tabs vertical list, recency-sorted, glyph+colour status
on agent rows, mouse-click nav, `Alt+c` hide/show, real-tab renames) · `clave spawn`
(idempotent) · `clave add` (zoxide picker; new|resume via clave's own picker;
worktree opt-in default-off) · status hooks → `clave hook` → snapshot push · `clave
setup` (hooks merge + permissions seed + config/layout) · keybinds `Alt+a/c/w`,
`Alt+↑/↓`, `Alt+j/k`, `Alt+1…9` (clave-session-scoped).

**Deferred:** **jsonl adoption** (auto-import every claude session ever run —
clave as the hub/controller for all claude sessions; ships WITH the nav ring
caps: Alt+↑/↓ walks only rows from the last 48h capped at 10, older rows
numbered + reachable only by a distinct number-referencing keybind, so nav
never lazily opens its way through thousands of conversations — 2026-07-17) ·
floating helper pane per agent tab (terminal/nvim in the agent's cwd,
2026-07-17) · one-shot LLM titles · store-row pruning (incl. stale-✗ row
recovery/removal UX) · worktree auto-cleanup · `AskUserQuestion`-wait state ·
per-agent remote `BreakPane` · an optional repo-grouped render mode · `cargo
install`/`include_bytes!` packaging · **per-agent context-% battery** (below).

**Backlog — context battery:** show each active agent's context-window usage in the
bar as a depleting battery glyph (🔋 → 🪫). Per-turn token/usage data lives in the
session jsonl; map cumulative usage → % of the model's context window → glyph. Lift
the extraction logic from the user's **`rot-reducer`** project
(`~/code/rot-reducer`), which already pulled context reporting out of Claude logs —
start there.

**Known edge — liveness vs tool children (2026-07-17):** with serialization off,
`dump-layout` still reports the *discovered* command; an agent mid-tool-call can
show its child (e.g. `cargo build`) instead of `claude --session-id`, so the
`live_uuids` check can transiently miss a live agent. Consequence is now limited
to add/open liveness (a false "dead" offer / a redundant open attempt guarded by
the no-op check), no longer broken resurrection. Accepted; revisit only if it
bites live. (The old ENTER-gate friction limitation is DELETED — serialization
is off, so the gate never appears.)

**Risks to validate early:** the join-key and plugin-architecture risks are retired
(S0/S0b/S1/S2 PASS). Remaining, all small: `hide_self` grid-reflow, tab-template
robustness, the resurrection ENTER-gate UX (S4), `switch_tab_to`. Terminal sends
Option-as-Meta — existing `Alt` binds already work, so almost certainly fine.

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
