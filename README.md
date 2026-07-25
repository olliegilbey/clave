# clave 🥁

**Conduct a fleet of Claude Code agents from a Zellij sidebar.**

`clave` turns [Zellij](https://zellij.dev) into an orchestration cockpit for
[Claude Code](https://claude.com/claude-code): every agent is a tab running the
real Claude TUI, listed in a vertical left bar that shows — at a glance — which
agent is **working**, which **needs you**, and which has **finished**.

No desktop app, no custom UI, no leaving the terminal. Just the agents you
already run, finally in one view.

```
┌─────────────────────┐
│ 🔴 main·api·fix-au…  │  needs you (waiting for input / approval)
│ ⚙️ feat·web·add-na…  │  working
│ ✅ main·docs·updat…  │  done, unread
│ ⚪ main·cli·refacto…  │  idle
└─────────────────────┘
```

## Why "clave"?

The **clave** is the foundational rhythm an entire ensemble locks to — the part
everything else syncs around. It's also Spanish for *key* / *keystone* (it's
keyboard-driven), and the archaic past tense of *cleave*, to split — as in
splitting the screen into panes. Logo: the two-stick percussion instrument.

## How it works

- **One agent = one Zellij tab** running the actual `claude` TUI — vim mode,
  slash commands, all of it. Not a reimplementation.
- **Status comes from [Claude Code hooks](https://code.claude.com/docs/en/hooks)**,
  not screen-scraping. `clave` mints each session's UUID, so hook events map
  exactly to the right tab and repaint its emoji.
- **Conversations survive restarts** — each pane runs an idempotent
  resume-or-create, so Zellij serialization brings every agent back.
- **Keyboard-first**: `Alt+a` add · `Alt+c` toggle bar · `Alt+t`/`Alt+w` new and
  close tab · `Alt+↑/↓` walk the fleet · `Alt+1…9` jump. Every key fires
  straight through a focused Claude session.

## Status

🚧 Early days. The full design and rationale live in
[`docs/design.md`](docs/design.md).

**If you build from source**, use `just dev-install` — it installs the
working-tree CLI as `clave-dev`, so it never takes over the `clave` command
([#43](https://github.com/olliegilbey/clave/issues/43)). A plain
`cargo install` would put `clave` on your `PATH` under the same name the daily
surface answers to, and a stale build winning that name is what produced two
sidebars and dead navigation in v0.1.1. A release cut installs its own
launcher at `~/.local/share/clave/bin/clave`; put that directory on your
`PATH`. `dev-install` still rewrites the sandbox bar wasm in place, so for
sandbox work prefer `just sandbox`, which refuses while a `clave-test` session
is live. See
[CONTRIBUTING](CONTRIBUTING.md#the-one-leak-clave-on-path-43-44) for the full
story and the one-line diagnosis.

## License

[MIT](LICENSE) © Oliver Gilbey
