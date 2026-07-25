# CLAUDE.md — clave

You are pairing on clave: a Rust CLI (`crates/clave`) plus a Zellij plugin
compiled to wasm (`crates/clave-bar`). These documents carry the operating
knowledge — read them, don't duplicate them here:

- **[AGENTS.md](AGENTS.md)** — **start here.** The autonomy contract (what you
  may do unsupervised, what is never yours to do), the required review lanes,
  the verification tiers in brief, and your handoff duty.
- **[CONTRIBUTING.md](CONTRIBUTING.md)** — the two environments (stable vs
  sandbox), the release model, the PR flow, where work is tracked, and **"The
  one leak"** — the PATH hazard that broke v0.1.1 in the field (#43, #44).
- **[docs/dev/TESTING.md](docs/dev/TESTING.md)** — the three verification tiers,
  the risk taxonomy (change class → what you must produce), the escape record,
  and the live-validation SOP with its interaction contract, sandbox lifecycle,
  observability map, and Zellij safety boundaries.
- **[docs/status/](docs/status/)** — session handoffs, TRACKED (ruling
  2026-07-21): they are the project's thinking-log history. Write yours
  there at session end and include it in your PR; resume from the newest
  one. They live in the MAIN checkout — a worktree only sees committed
  ones (a fresh handoff written elsewhere is invisible until merged).

## Standing rules

- **Test with `cargo test --workspace`, always.** Bare `cargo test` silently
  skips the wasm crate's tests. Use the workspace form or `just test`.
- **`just gates` before every push.** It runs all four CI gates in CI's order —
  `fmt --check`, test, wasm build, clippy. `cargo fmt --all --check` is the one
  that bites: CI's lint job runs it before clippy, so hand-edited Rust can be
  clippy-clean and still fail the build (#66).
- **TDD.** Write the failing test first, watch it fail, then implement.
- **Dense why-comments.** Match the codebase: comments explain *why* and cite
  the spec section or the ledger finding, not *what*.
- **Never commit without the maintainer's explicit approval.** The maintainer
  signs the commits. You prepare; they approve and sign.
- **Never install to the stable release surface from a working session.** That
  means never run `just install` (retired) or `just release` off feature work,
  and never write anything under `~/.local/share/clave/` — the versioned
  artifacts or the `bin/clave` launcher a cut owns (#43a).
- **`just dev-install` is NOT safe while the maintainer is daily-driving.**
  (Corrected 2026-07-22 — this rule used to call it unconditionally sanctioned,
  and that is what broke production.) Since #43b it installs `clave-dev`, so it
  no longer takes over the daily launcher's name — but it still rebuilds the
  sandbox wasm in place under a possibly-live `clave-test`, and a bare
  `cargo install` still writes `~/.cargo/bin/clave`, which shadows the #43a
  launcher on `PATH`. Assume he is driving unless he has said otherwise, and use
  `just sandbox` — it refuses against a live sandbox session and writes neither
  surface. See CONTRIBUTING "The one leak".
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
