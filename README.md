# clave 🥁

**Run many Claude Code agents at once and see, at a glance, exactly who
needs you. In your terminal, where you already live.**

Run three agents and you can hold it in your head. Run eight and you can't.
You start tabbing through them to find the one that asked you a question ten
minutes ago. clave gives the fleet one vertical bar: every agent is a Zellij
tab running the real Claude TUI, and a glance tells you who needs an answer,
who is working, and who finished while you were looking elsewhere.

<!-- Regenerate these frames and every icon below with
     `cargo run -q -p clave-bar --example readme-assets`. The fleet is the
     showcase fixture in crates/clave-bar/examples/shared/showcase_fixture.rs.
     Edit it there, never here: the SVG is traced from the plugin's own
     renderer, so it can only change by changing the design. -->
The sidebar sits to the left of your terminal, one row per agent. Expand it
when you want the detail; collapse it to a strip when you don't. Same fleet,
both states:

| expanded | collapsed |
|---|---|
| <img src="docs/assets/sidebar-expanded.svg" alt="The expanded clave sidebar: nine rows, each a coloured status mark, a token count, an optional branch or worktree mark, a rename chip, the repo name, and a one-line description." width="620"> | <img src="docs/assets/sidebar-collapsed.svg" alt="The same nine rows collapsed to a strip: the token count becomes a battery glyph and the text truncates." width="300"> |

## What the colours and glyphs mean

Each row is one conversation. The coloured dot is its state, and you mostly
never need more than that; the rest of the row fills in the detail. Left to
right:

<table>
<tr><td><b>status</b></td><td><img alt="" src="docs/assets/glyphs/status-needs-you.svg" width="18"> waiting on you · <img alt="" src="docs/assets/glyphs/status-working.svg" width="18"> working · <img alt="" src="docs/assets/glyphs/status-done.svg" width="18"> finished while you were away · <img alt="" src="docs/assets/glyphs/status-idle.svg" width="18"> idle · <img alt="" src="docs/assets/glyphs/status-failed.svg" width="18"> last turn failed · <img alt="" src="docs/assets/glyphs/status-stale.svg" width="18"> its directory is gone · <img alt="" src="docs/assets/glyphs/status-dormant.svg" width="18"> dormant, half-faded; opens where it left off · <img alt="" src="docs/assets/glyphs/status-opening.svg" width="18"> opening · <img alt="" src="docs/assets/glyphs/term-running.svg" width="18"> a terminal tab (colours mean the same)</td></tr>
<tr><td><b>battery</b></td><td><code>105k</code> context spent, in tokens<br><img alt="" src="docs/assets/glyphs/battery-00.svg" width="18"><img alt="" src="docs/assets/glyphs/battery-06.svg" width="18"><img alt="" src="docs/assets/glyphs/battery-08.svg" width="18"><img alt="" src="docs/assets/glyphs/battery-10.svg" width="18"> the same reading as a glyph, when the bar is collapsed<br><code>TERM</code>, a terminal tab</td></tr>
<tr><td><b>mark</b></td><td><i>blank</i>, the repo's ordinary checkout · <img alt="" src="docs/assets/glyphs/mark-branch.svg" width="18"> on a branch · <img alt="" src="docs/assets/glyphs/mark-worktree.svg" width="18"> in its own git worktree</td></tr>
<tr><td><b>chip</b></td><td>the name you gave the session with <code>/rename</code>; blank until you do<br>on a terminal row, the tab's name</td></tr>
<tr><td><b>repo</b></td><td><img alt="" src="docs/assets/glyphs/repo-0.svg" width="18"><img alt="" src="docs/assets/glyphs/repo-1.svg" width="18"><img alt="" src="docs/assets/glyphs/repo-4.svg" width="18"><img alt="" src="docs/assets/glyphs/repo-6.svg" width="18"> one colour per repo, wherever it appears</td></tr>
<tr><td><b>text</b></td><td>Claude's own description of the session, not your prompt<br>on a terminal row, the last command it ran</td></tr>
</table>

The battery is how much context the conversation has used: a token count
when the bar is expanded, a battery glyph when it's collapsed. It goes red
as you reach the end of your **smart zone**, the context size you still
trust the model at. Set your own in your shell config:

```bash
export CLAVE_AGENT_SMART_ZONE_TOKENS=150000   # the default
```

Pairs well with [rot-reducer](https://github.com/olliegilbey/rot-reducer),
which tells Claude when its context is filling up so long sessions wrap up
work cleanly before auto-compaction kicks in.

## Try it

You need [`zellij`](https://zellij.dev) (0.44.3 is what's tested), `claude`,
`git`, plus `fzf` and `zoxide` for the directory picker, and a
[Nerd Font](https://www.nerdfonts.com/) in your terminal. macOS and Linux.

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/olliegilbey/clave/releases/latest/download/clave-installer.sh | sh

clave   # from a terminal OUTSIDE zellij; clave makes its own session
```

First launch sets the machine up: Zellij config and keybinds, clave's status
hooks in `~/.claude/settings.json` (additive, your own hooks are left alone),
and the plugin permission cache. That's what lets an agent report its own
state. `clave doctor` explains anything that didn't land.

Then `Alt+a`, pick a directory, choose `new`. That's your first agent.

**Upgrading?** Re-run the installer, quit every running clave session, start
fresh. Zellij swaps regenerated keybinds into sessions that are already
running, but a sidebar that is already loaded keeps the identity it booted
with, so a warm session would grow a second sidebar.

Tarballs, sha256s and build attestations are on the
[releases page](../../releases); verify with `gh attestation verify`.

<details>
<summary><b>Building from source</b></summary>

Rust (stable) and [`just`](https://just.systems), then:

```bash
git clone https://github.com/olliegilbey/clave && cd clave
git checkout v0.2.0        # `just release` refuses a dirty or untagged tree
just setup-toolchain       # adds the wasm32-wasip1 target, once
just release               # builds, then installs the launcher and versioned artifacts

# Put the launcher on your PATH in your SHELL CONFIG, not just this shell.
echo 'export PATH="$HOME/.local/share/clave/bin:$PATH"' >> ~/.zshrc    # zsh
echo 'export PATH="$HOME/.local/share/clave/bin:$PATH"' >> ~/.bashrc   # bash
exec $SHELL
```

</details>

## The keys

| | |
|---|---|
| `Alt+a` | add an agent: pick a directory, then new or resume |
| `Alt+↑` `Alt+↓` (or `Alt+k` `Alt+j`) | walk the running agents |
| `Alt+1`…`Alt+9` | jump straight to a row, running or closed |
| `Alt+Enter` | wake the selected closed row |
| `Alt+o` | back to where you were |
| `Alt+c` | collapse the bar to a strip, or expand it |
| `Alt+t` `Alt+w` | new tab, close tab |
| `Alt+f` | a scratch shell floating over the fleet; press again to tuck it away |

Everything clave binds lives on `Alt`. Five stock Zellij `Ctrl` bindings that
Claude Code needs (`Ctrl+g/t/o/b/q`) are unbound for you; the rest of
Zellij's keys still belong to Zellij.

## How it works

- **One agent is one Zellij tab** running the actual `claude` binary. Vim
  mode, slash commands, MCP servers, all of it. clave never reimplements the
  TUI; it just always knows where each conversation is.
- **Status comes from [Claude Code hooks](https://code.claude.com/docs/en/hooks)**,
  not screen-scraping. A row changes the moment the agent does.
- **Conversations survive restarts.** A cold start brings the fleet back as
  dormant rows; open one and it picks up where it left off.
- **Terminal tabs are rows too**, with their checkout, their last command,
  and whether it failed.
- **Running agents sit above closed ones**, so the fleet you're actually
  working stays a short cycle however many conversations you've accumulated.
- **Worktrees are first class.** An agent can have its own branch in its own
  directory, so two agents in one repo never fight over your working tree.

**What it isn't:** a wrapper around Claude, a scheduler, or something you
configure. There is no config file. The bar's layout is a ratified design,
not a preference, and clave's job is to get out of the way of the agents.

## Contributing

Want to work on it? Start with [CONTRIBUTING](CONTRIBUTING.md), especially
before you install anything over a running session. Something broke?
[Open an issue](../../issues); that's the most useful thing you can do.

## Why "clave"?

The **clave** is the foundational rhythm an entire ensemble locks to, the
part everything else syncs around. It's also Spanish for *key* / *keystone*
(it's keyboard-driven), and the archaic past tense of *cleave*, to split, as
in splitting the screen into panes. Logo: the two-stick percussion instrument.

## License

[MIT](LICENSE) © Oliver Gilbey
