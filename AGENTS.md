# AGENTS.md — clave

**clave** is a Zellij fleet-orchestration sidebar: it controls and manages a set of `claude` agents as vertical tabs in one multiplexer session, keyed off a store the agents' hooks write into. It builds its own daily driver — we live inside a clave session while developing clave from a session inside it.

clave will support other CLI-based agents down the line, keep this in mind as a design decision for DRY and neat APIs that are sensible and straightforward.

Be deliberate in avoiding overengineering early - this is greenfield, we are iterating quickly with minimal buildout to achieve the goals. Focus on KISS.

Ground all work in the screenshots - look at the clave-working-sample PNG images in the root first to gain a visual understanding.

Two crates, one workspace:

| Crate              | Target          | What it is                                                                                 |
| ------------------ | --------------- | ------------------------------------------------------------------------------------------ |
| `crates/clave`     | host binary     | the `clave` CLI — store, setup/release, KDL generation, hooks, the `dev` sandbox           |
| `crates/clave-bar` | `wasm32-wasip1` | the sidebar plugin — pure state machine (`model.rs`) plus a thin zellij event/effect shell |

`crates/clave-types` carries the shared vocabulary. `main` is always releasable;
a `vX.Y.Z` tag plus `just release` is the promotion event.

## Useful Documents:

[FOOTGUNS.md](FOOTGUNS.md) | traps that already cost someone a round — things that compile, read, or look fine and are wrong anyway. **Grep it the moment something behaves unexpectedly**, before you start debugging. Add to it when you lose time to something the next agent would also lose time to

[UBIQUITOUS_LANGUAGE.md](UBIQUITOUS_LANGUAGE.md) | the shared vocabulary. **zellij session vs agent session**, **title vs label**, gutter · cell · ink · chip · provenance. Short, and it unlocks every other document — "session" alone is ambiguous three ways in this codebase - add to it when a term is agreed upon between you and the maintainer.

[CONTRIBUTING.md](CONTRIBUTING.md) | the two environments (stable vs sandbox), the release model, the PR flow, where work is tracked — **and "The one leak"**, the PATH hazard that broke v0.1.1 in the field (#43, #44)

[docs/dev/README-SOP.md](docs/dev/README-SOP.md) | touching README.md? The ratified rules, layout and exemplar every README change is written from and checked against

[docs/dev/TESTING.md](docs/dev/TESTING.md) | the three verification tiers, the risk taxonomy (change class → required verification), the escape record, and the live-validation SOP

[SUBSYSTEM-VALIDATION.md](docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md) | the C-section for the subsystem you are about to change — the ledger of approaches tried and _why they failed_. Read it first; every forbidden path was expensive to learn

**Drive the sandbox; never touch his session.** Ollie dog-foods clave daily —
the Claude you are is running _inside_ a live clave session, so a bare `zellij`
command targets his working fleet. Against `clave-test` you may run `zellij
action` freely (`ZELLIJ_SESSION_NAME=clave-test …` — stage it with `just
sandbox`); against his session you run nothing, not even a read. **Launching
any session is his**, as is `just release` and anything writing
`~/.local/share/clave/`. Print those; let him run them. Killing is his too,
with one exemption: a sandbox you asked him to launch in this conversation,
once its drive and both eyeball checkpoints are done — kill it by its explicit
name (`clave dev instance --field session`), never a sandbox another agent
staged (each has its own name and root). The loop is
[docs/dev/TESTING.md](docs/dev/TESTING.md) § the sandbox drive loop.

Something behaving strangely? Grep FOOTGUNS.md before you start debugging.

Write what you learn where it belongs — trap → FOOTGUNS.md · term →
UBIQUITOUS*LANGUAGE.md · dead end → the subsystem's C-section · how to \_use*
clave → README.md · how to _work on_ clave → CONTRIBUTING.md. Not here. This
file is the index, not the knowledge.

Read the vendored source for zellij behaviour
(`~/.cargo/registry/src/*/zellij-tile-0.44.3/`, `…/zellij-utils-0.44.3/`) before
building on it — `TabUpdate` reaches only the active tab, `resize_pane_with_id`
silently refuses fixed panes, `show_self` is a focus action. Each of those cost a
round.

Ollie is happy to test anything you can't discover yourself inside his terminal or clave session.

The four commands a PR must show green — or `just gates`, which runs exactly
these in this order:

```bash
cargo fmt --all --check      # CI's lint job runs fmt BEFORE clippy
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

And use cargo mutants.

When considering updates for CLAUDE.md or AGENTS.md, first have a conversation with Ollie to lock in directives and information.
