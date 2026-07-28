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

**If you build from source**, use `just sandbox` — it wires an isolated
`clave-test` session to your working tree and verifies it touched neither your
installed binary nor your stable artifacts. `just dev-install` overwrites
`~/.cargo/bin/clave`, so your next launch would run a working-tree build; that
is sometimes what you want, but rarely by accident.

Either way: **don't install over a session that is currently running.** See
[CONTRIBUTING](CONTRIBUTING.md#the-rule-that-matters-most).

## License

[MIT](LICENSE) © Oliver Gilbey
