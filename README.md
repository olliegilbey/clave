# clave 🥁

**Conduct a fleet of Claude Code agents from a Zellij sidebar.**

Run three agents and you can hold it in your head. Run eight and you can't — you
start tabbing through them to find the one that asked you a question ten minutes
ago. clave gives the fleet one vertical bar: every agent is a Zellij tab running
the real Claude TUI, and a glance tells you who needs an answer, who is working,
and who finished while you were looking elsewhere.

```
┌──────────────────────────────────────────────────────┐
│ ● │   󰘬           api     rotate the signing keys    │ ← needs you
│ ● │   𖣂 S6-GUT    web     add the nav skeleton       │ ← working
│ ● │               docs    update the install steps   │ ← done, unread
│ ● │               cli     refactor the arg parser    │ ← idle
│ ◌ │               infra   bump the runner image      │ ← dormant, no process
│   │ 󰆍   shell                                        │ ← a plain terminal tab
└──────────────────────────────────────────────────────┘
```

**Colour is the state** — the four dots above are red, amber, green and grey in a
real terminal, which is the whole point and the one thing a code block can't
show. Shape changes only where a row isn't a live conversation.

Then: a mark for a branch or a worktree, blank on a normal checkout. The
session's own name, once you `/rename` it. The repo. And Claude's description of
what it's actually doing — its words, not your prompt.

## Try it

Needs `zellij`, `claude` and `git` on your PATH, plus `fzf` and `zoxide` for the
directory picker, and a [Nerd Font](https://www.nerdfonts.com/) in your terminal.

```bash
git clone https://github.com/olliegilbey/clave && cd clave
git checkout "$(git tag --sort=-v:refname | head -1)"   # release cuts are tagged
just release          # builds, then installs the launcher and versioned artifacts
export PATH="$HOME/.local/share/clave/bin:$PATH"
clave                 # from a terminal OUTSIDE zellij — clave makes its own session
```

No packaged release yet; building from a tag is the supported path. `just
release` refuses a dirty or untagged tree on purpose, and `clave doctor` will
tell you what's missing if the launch doesn't go cleanly.

Then `Alt+a` and pick a directory. That's your first agent.

| | |
|---|---|
| `Alt+a` | add an agent — pick a directory, optionally in its own git worktree |
| `Alt+↑` `Alt+↓` (or `Alt+k` `Alt+j`) | walk the fleet |
| `Alt+1`…`Alt+9` | jump straight to a row |
| `Alt+o` | back to where you were |
| `Alt+c` | collapse the bar to a strip, or expand it |
| `Alt+t` `Alt+w` | new tab, close tab |

Every other keystroke goes straight through to the focused Claude session.

## How it works

- **One agent = one Zellij tab**, running the actual `claude` binary — vim mode,
  slash commands, MCP servers, all of it. clave never reimplements the TUI. It
  just always knows where each conversation is.
- **Status comes from [Claude Code hooks](https://code.claude.com/docs/en/hooks)**,
  not screen-scraping. Each agent reports its own turn lifecycle, so a row
  changes the moment the agent does.
- **Conversations survive restarts.** Every pane runs an idempotent
  resume-or-create, so closing a tab, restarting Zellij, or upgrading clave
  brings the same conversations back — including the ones you've `/clear`ed,
  which start a fresh session id underneath.
- **Worktrees are first class.** Add an agent on its own git worktree and it
  gets its own branch and directory, so two agents in one repo never fight over
  your working tree.

## Why "clave"?

The **clave** is the foundational rhythm an entire ensemble locks to — the part
everything else syncs around. It's also Spanish for *key* / *keystone* (it's
keyboard-driven), and the archaic past tense of *cleave*, to split — as in
splitting the screen into panes. Logo: the two-stick percussion instrument.

## Status

🚧 Early, and its author's daily driver — clave is developed from inside a clave
session, which is why the rough edges get found fast. If something breaks,
[open an issue](../../issues); that's the most useful thing you can do right now.

Design and rationale: [`docs/design.md`](docs/design.md). Want to work on it?
[CONTRIBUTING](CONTRIBUTING.md) — start there rather than here, especially
before you install anything over a running session.

## License

[MIT](LICENSE) © Oliver Gilbey
