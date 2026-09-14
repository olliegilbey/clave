# 2026-09-14 — daily-driving the 0.5.0 cut: three fixes, one bug open

Branch `chore/release-0.5.0`, four commits past `main`. **Nothing pushed, and
CI has seen none of it.** Ollie is running the cut on his daily fleet and
reporting what he sees; each item below came from that.

## Done and committed

| Commit | What was wrong |
|---|---|
| `37a9b4f` | `just release` never set `CLAVE_BAR_WASM`, so every locally cut launcher answered "dev build" about itself and its own `clave setup` refused. Fixed in the recipe; `tests/release_recipe.rs` is the new gate. |
| `12d4ca2` | Status record closing the 12:20 second-sidebar diagnosis. |
| `e5cfbc4` | The turn clock went coarse at one minute. Three bands now, five cells wide, and the token gap paid two of them. |
| `435e69a` | `worktree` was recorded only when clave itself created the worktree, so the tree mark had never rendered. git decides it now, plus a fleet repair. |

Each is gated (`fmt`, `test --workspace`, wasm build, `clippy -D warnings`).

### What Ollie still has to run

```bash
cd /Users/olliegilbey/code/clave
git merge --ff-only chore/release-0.5.0
git tag -f v0.5.0
just release        # expect: "clave: recorded the worktree for 5 row(s)"
zellij kill-session clave && zellij delete-session --force clave && clave
```

He has NOT run this for the last two commits yet. The clock and the tree mark
are unseen on a real terminal.

## Open — the red glyph outlives the block

**Reported 2026-09-14 ~14:54 with a screenshot.** A row goes red on a
permission prompt. Ollie approves. The agent resumes (`Spelunking… 1m 6s` in
its own footer) and **the row stays red** for the rest of the turn.

Diagnosed, not fixed. It is a hole in the transition table, not a race.

`setup.rs::HOOK_EVENTS` registers exactly five: `UserPromptSubmit`, `Stop`,
`Notification`, `SessionEnd`, `SessionStart`. `hook.rs::status_for_event` maps
them. Once `Notification` (message contains `permission`) sets `NeedsYou`, the
only events that can arrive are:

- another `Notification` — the `waiting for your input` arm is gated on
  `current == Working`, so it returns `None` and changes nothing;
- `Stop` → `Done`, at the END of the turn;
- `UserPromptSubmit` → `Working`, only if Ollie types again.

**Approving a permission fires no hook clave listens to.** The tool simply
runs. So there is no event that means "the block cleared", and red persists
until the turn ends.

### The candidate fix, and why it needs a ruling

Register `PostToolUse` and map it to "clear `NeedsYou` back to `Working`" —
a tool that has RUN is proof the block is gone.

The cost is the whole question: `PostToolUse` fires on **every tool call of
every tracked agent**, and each firing is a subprocess doing a locked store
RMW plus a zellij pipe. `hook.rs:1536` already names that budget when it
explains why the transcript tail is gated on two events. A fleet of ten busy
agents would multiply the hook rate by a large factor.

Options worth weighing before writing anything:

1. **Register `PostToolUse`, and make the handler cheap and conditional** —
   return before taking the lock unless the row is actually `NeedsYou`. Most
   firings would then be a read and an exit. Needs measuring, not assuming:
   the read still opens the store.
2. **Matcher-scoped registration.** Claude Code's `PostToolUse` takes a tool
   matcher. Permission prompts cluster on a few tools (`Bash`, `Write`,
   `Edit`, MCP calls). Registering the matcher rather than `*` cuts the rate
   a long way at the price of missing the rest.
3. **Leave it and dim instead.** Red that lies is worse than red that fades;
   the bar could treat a `NeedsYou` older than its last activity as stale.
   This invents a measurement, which §4.4's ruling forbids elsewhere.

**Ollie has not chosen.** Ask before building — option 1 changes the hook rate
on his live fleet, which is the one resource this design has always protected.

## Other things known and not done

- **`clave rows` is not atomic.** It writes the store, then regenerates. A
  failure between them leaves the split that shows as a second sidebar. The
  release fix removed today's cause, not the class.
- **A row whose worktree was DELETED stays unmarked** — git cannot vouch for
  it, and the bar asserts nothing it cannot measure. Three of Ollie's dormant
  rows are in this state. Deliberate; revisit only if he asks.
- **The eyeball checks this branch has never had**: tofu on the provider and
  worktree marks (Nerd Fonts 3.5+ required), the spinner's six frames (five
  arrive by fallback from Menlo, so weight and baseline can jump), `Alt+c` at
  16 columns, the four-line card's stacked marks.
- **Housekeeping**: the `triple-card` worktree and its branch, the stale
  sandbox root `~/.local/state/clave-dev-triple-card`, and a PR for all four
  commits before the `v0.5.0` tag is pushed.

## Standing constraints

Ollie launches and kills sessions; he owns every write to
`~/.local/share/clave/`. Do not touch his live session — you run inside it.
Fire hooks only through `scripts/ct.sh --hook`. `just gates` green before any
commit.
