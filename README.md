# clave 🥁

**Conduct a fleet of Claude Code agents from a Zellij sidebar.**

Run three agents and you can hold it in your head. Run eight and you can't — you
start tabbing through them to find the one that asked you a question ten minutes
ago. clave gives the fleet one vertical bar: every agent is a Zellij tab running
the real Claude TUI, and a glance tells you who needs an answer, who is working,
and who finished while you were looking elsewhere.

<!-- Regenerate: `cargo run -q -p clave-bar --example bar-preview -- --showcase`,
     screenshot the output, replace docs/assets/sidebar.png. The fleet is the
     `showcase()` fixture in that example — edit it there, not here, so the
     frame always comes from the real `render_rows`. -->
<img src="docs/assets/sidebar.png" alt="The clave sidebar: nine rows, each a coloured status dot, an optional branch or worktree mark, a rename chip, the repo name, and a one-line description. Red is waiting on you, amber working, green done, grey idle; a red cross has failed, a hollow ring is dormant, and two rows are plain terminal tabs." width="720">

**Colour is the state.** Red is waiting on you, amber is working, green finished
while you were elsewhere, grey is idle. Shape changes only where a row isn't a
live conversation: `✖` failed, `✗` its directory is gone, `◌` dormant — no
process running, but open it and the conversation picks up where it left off.

Then a mark for a branch or a worktree, blank on a normal checkout. The session's
own name once you `/rename` it. The repo — one colour per repo, wherever it
appears. And Claude's own one-line description of the session: its words, not
your prompt, and a subtitle rather than a status line.

Terminal tabs sit in the same list. The bar is the whole session, not just its
agents.

## Try it

**Runtime:** `zellij` (0.44.3 is what's tested), `claude`, `git`, plus `fzf` and
`zoxide` for the directory picker. macOS and Linux. A
[Nerd Font](https://www.nerdfonts.com/) for the branch and terminal marks.

**To build:** Rust (stable) and [`just`](https://just.systems).

```bash
git clone https://github.com/olliegilbey/clave && cd clave
git checkout v0.1.2        # `just release` refuses a dirty or untagged tree
just setup-toolchain       # adds the wasm32-wasip1 target, once
just release               # builds, then installs the launcher and versioned artifacts
export PATH="$HOME/.local/share/clave/bin:$PATH"
clave                      # from a terminal OUTSIDE zellij — clave makes its own session
```

No packaged release yet; building from a tag is the supported path. Take the tag
literally — earlier ones predate the launcher this puts on your PATH.

`just release` also registers clave's status hooks in `~/.claude/settings.json`
(additive — your own hooks are left alone) and seeds Zellij's plugin permission
cache. That's what lets an agent report its own state. `clave doctor` explains
anything that didn't land.

Then `Alt+a`, pick a directory, choose `new`. That's your first agent.

| | |
|---|---|
| `Alt+a` | add an agent — pick a directory, then new or resume |
| `Alt+↑` `Alt+↓` (or `Alt+k` `Alt+j`) | walk the fleet |
| `Alt+1`…`Alt+9` | jump straight to a row |
| `Alt+o` | back to where you were |
| `Alt+c` | collapse the bar to a strip, or expand it |
| `Alt+t` `Alt+w` | new tab, close tab |

`Alt` is clave's namespace. Five stock Zellij bindings that Claude Code needs
are unbound for you (`Ctrl+g/t/o/b/q`); the rest of Zellij's `Ctrl` keys still
belong to Zellij ([#24](../../issues/24)).

## How it works

- **One agent = one Zellij tab**, running the actual `claude` binary — vim mode,
  slash commands, MCP servers, all of it. clave never reimplements the TUI. It
  just always knows where each conversation is.
- **Status comes from [Claude Code hooks](https://code.claude.com/docs/en/hooks)**,
  not screen-scraping. Each agent reports its own turn lifecycle, so a row
  changes the moment the agent does.
- **Conversations survive restarts.** Every pane runs an idempotent
  resume-or-create. A cold start reopens your most recent agent and brings the
  rest back as dormant rows — open one and it picks up where it left off,
  including the ones you've `/clear`ed, which start a fresh session id
  underneath.
- **Worktrees are first class.** `clave add --worktree` puts an agent on its own
  branch in its own directory, so two agents in one repo never fight over your
  working tree.

**What it isn't:** a wrapper around Claude, a scheduler, or something you
configure. There is no config file — the bar's layout is a ratified design, not
a preference, and clave's job is to get out of the way of the agents.

## Status

🚧 Early, and its author's daily driver — clave is developed from inside a clave
session, which is why the rough edges get found fast. If something breaks,
[open an issue](../../issues); that's the most useful thing you can do right now.

Design and rationale:
[the orchestrator design spec](docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md).
Want to work on it? [CONTRIBUTING](CONTRIBUTING.md) — start there rather than
here, especially before you install anything over a running session.

## Why "clave"?

The **clave** is the foundational rhythm an entire ensemble locks to — the part
everything else syncs around. It's also Spanish for *key* / *keystone* (it's
keyboard-driven), and the archaic past tense of *cleave*, to split — as in
splitting the screen into panes. Logo: the two-stick percussion instrument.

## License

[MIT](LICENSE) © Oliver Gilbey
