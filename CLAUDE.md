# CLAUDE.md — clave

You are pairing on clave: a Rust CLI (`crates/clave`) plus a Zellij plugin
compiled to wasm (`crates/clave-bar`). Two documents carry the operating
knowledge — read them, don't duplicate them here:

- **[CONTRIBUTING.md](CONTRIBUTING.md)** — the two environments (stable vs
  sandbox), the release model, the PR flow, where work is tracked.
- **[docs/dev/TESTING.md](docs/dev/TESTING.md)** — the live-validation SOP: the
  interaction contract, the sandbox lifecycle, the observability map, the
  instrumentation recipe, and the Zellij safety boundaries.
- **[docs/status/](docs/status/)** — session handoffs, TRACKED (ruling
  2026-07-21): they are the project's thinking-log history. Write yours
  there at session end and include it in your PR; resume from the newest
  one. They live in the MAIN checkout — a worktree only sees committed
  ones (a fresh handoff written elsewhere is invisible until merged).

## Standing rules

- **Test with `cargo test --workspace`, always.** Bare `cargo test` silently
  skips the wasm crate's tests. Use the workspace form or `just test`.
- **TDD.** Write the failing test first, watch it fail, then implement.
- **Dense why-comments.** Match the codebase: comments explain *why* and cite
  the spec section or the ledger finding, not *what*.
- **Never commit without the maintainer's explicit approval.** The maintainer
  signs the commits. You prepare; they approve and sign.
- **Never install to the stable release surface from a working session.** That
  means never run `just install` (retired) or `just release` off feature work,
  and never write the versioned artifacts under `~/.local/share/clave/`. The
  only sanctioned install from a working session is `just dev-install` (dev CLI
  → `~/.cargo/bin`, wasm → the sandbox data dir).
- **Zellij session lifecycle belongs to the human.** You never launch or kill a
  session — you print the command for the human to run. `zellij action` against
  a dead session blocks forever, so gate on liveness (`clave dev status`).
- **Sandbox-only hot-reload is the one sanctioned live mutation** an agent may
  perform, and only against the `clave-test` session's wasm in the sandbox data
  dir. Everything else touching the live terminal is the human's.

When in doubt, read the C-section in
[SUBSYSTEM-VALIDATION.md](docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md) before
changing a subsystem, and read the vendored Zellij source before trusting an
assumed Zellij behavior.
