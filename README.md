# clave 🥁

**Conduct a fleet of Claude Code agents from a Zellij sidebar.**

`clave` turns [Zellij](https://zellij.dev) into an orchestration cockpit for
[Claude Code](https://claude.com/claude-code): every agent is a tab running the
real Claude TUI, listed in a vertical left bar that shows — at a glance — which
agent is **working**, which **needs you**, and which has **finished**.

No desktop app, no custom UI, no leaving the terminal. Just the agents you
already run, finally in one view.

```
┌──────────────────────────────────────────────────────┐
│ ● │   󰘬           api     rotate the signing keys    │
│ ● │   𖣂 S6-GUT    web     add the nav skeleton       │
│ ● │               docs    update the install steps   │
│ ● │   󰘬           cli     refactor the arg parser    │
│ ◌ │               infra   bump the runner image      │
│   │ 󰆍   shell                                        │
└──────────────────────────────────────────────────────┘
```

Rendered by the real bar, so the columns are exactly what you get. Reading
left to right: **status**, then a fixed gutter, then **provenance**, the
**rename chip**, the **repo**, and what the agent is actually doing.

**Colour is the state**, which a code block cannot show: the four `●` rows
above are red *needs you*, amber *working*, green *done*, and grey *idle*. Only
where a row is not a live conversation does the shape change too — `◌` dormant,
`↻` opening, `✖` failed, `✗` its directory has gone.

A **main checkout renders nothing** in the provenance cell. That is deliberate:
blanking the most common row is what makes `󰘬` (a branch) and `𖣂` (a worktree)
mean something at a glance. The chip beside it stays empty until you `/rename`
a session — clave never fills it with a label of its own — and the last column
is Claude's own description of the work, not your prompt.

Terminal tabs sit in the same list rather than being hidden, so the bar is the
whole session and not just its agents.

> Needs a [Nerd Font](https://www.nerdfonts.com/) in your terminal for the
> provenance and terminal glyphs. Everything else is plain Unicode.

## Why "clave"?

The **clave** is the foundational rhythm an entire ensemble locks to — the part
everything else syncs around. It's also Spanish for *key* / *keystone* (it's
keyboard-driven), and the archaic past tense of *cleave*, to split — as in
splitting the screen into panes. Logo: the two-stick percussion instrument.

## How it works

- **One agent = one Zellij tab** running the actual `claude` TUI — vim mode,
  slash commands, all of it. Not a reimplementation.
- **Status comes from [Claude Code hooks](https://code.claude.com/docs/en/hooks)**,
  not screen-scraping. `clave` mints each session's UUID and hands it to the
  agent's own process, so every event lands on the right row — including after
  a `/clear`, which starts Claude on a fresh id.
- **Conversations survive restarts** — each pane runs an idempotent
  resume-or-create, so Zellij serialization brings every agent back on the
  conversation you were actually in, not the one the tab started with.
- **Keyboard-first**: `Alt+a` add · `Alt+c` toggle bar · `Alt+t`/`Alt+w` new and
  close tab · `Alt+↑/↓` walk the fleet · `Alt+1…9` jump. Every key fires
  straight through a focused Claude session.

## Status

🚧 Early days. The full design and rationale live in
[`docs/design.md`](docs/design.md).

**If you build from source**, use `just sandbox` — it wires an isolated
`clave-test` session to your working tree and verifies it touched neither your
installed binary nor your stable artifacts.

`just dev-install` installs the working-tree CLI as **`clave-dev`**, so it never
takes over the `clave` command ([#43](https://github.com/olliegilbey/clave/issues/43)).
A plain `cargo install` would put `clave` on your `PATH` under the same name the
daily surface answers to, and a stale build winning that name is what produced
two sidebars and dead navigation in v0.1.1.

A release cut installs its own launcher at `~/.local/share/clave/bin/clave` —
put that directory on your `PATH`.

Either way: **don't install over a session that is currently running.** See
[CONTRIBUTING](CONTRIBUTING.md#the-rule-that-matters-most).

## License

[MIT](LICENSE) © Oliver Gilbey
