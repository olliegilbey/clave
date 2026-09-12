**clave** is a vertical tab control system for visualising and managing different agent sessions and terminals. Always begin each session by looking at the clave-working-sample-1.png screenshot to understand the visual element of clave. Clave uses Zellij for a multiplexer session, keyed off a store the agents' hooks write into. We dogfood the system continuously.

You are working here with a pair. You bring software fundamentals, SDLC, and your knowledge from having read _A Philosophy of Software Design_ - Ousterhout, and _The Design of Everyday Things_ - Norman. Apply them freely. Clave is the project you have joined: learn its conventions and build accordingly to avoid CI catching you, and to further expand CI capabilities. Zellij behaviour comes from the vendored source. For any other external API, WebSearch current documentation before relying on it, or brief a subagent to research it.

One workspace: `crates/clave` is the host CLI, `crates/clave-bar` is the sidebar plugin built for `wasm32-wasip1`, and `crates/clave-types` carries the shared vocabulary.

## Documents

- [FOOTGUNS.md](FOOTGUNS.md) — traps that compile and read fine, and are wrong anyway. **Grep it the moment something behaves unexpectedly, before you debug.**
- [UBIQUITOUS_LANGUAGE.md](UBIQUITOUS_LANGUAGE.md) — the vocabulary, binding in code, specs, issues and PRs. "Session" means three different things here. Add a new term in the same change that introduces it.
- [CONTRIBUTING.md](CONTRIBUTING.md) — the two environments, the release model, the PR flow.
- [docs/dev/TESTING.md](docs/dev/TESTING.md) — the verification tiers and the risk taxonomy. Find your change in the taxonomy before you call it verified.
- [docs/dev/QA-DRIVE.md](docs/dev/QA-DRIVE.md) — the regression drive, `scripts/qa-drive.sh <scenario>`. It tests seams, not logic.
- [docs/dev/README-SOP.md](docs/dev/README-SOP.md) — read this before you change README.md.

## What done looks like

Done is a change Ollie keeps, not a test that passes. `main` is releasable at every commit; a `vX.Y.Z` tag with `just release` promotes it. Any feature that uses history meets one more bar: a fresh install on a months-old Claude Code history comes up warm from the jsonl alone — conversations resumable, frecency populated, nothing cold the transcripts could have answered.

## Principles

Ordered by weight. When two collide, the earlier one wins.

- **KISS.** The simplest correct thing is almost always right. Build the minimum that reaches the goal, then stop. Do not keep complexity because it is already there. Do not add machinery because it looks impressive.
- **The transcripts out-rank the store.** Claude writes `~/.claude/projects/**/*.jsonl`. Those files outlive clave, so our store is only a cache over them. Derive history from the transcripts. Never mint a second source.
- **Measured beats assumed.** Take zellij behaviour from the vendored source (`~/.cargo/registry/src/*/zellij-tile-0.44.3/`, `…/zellij-utils-0.44.3/`), or from a run you did yourself. `TabUpdate` reaches only the active tab. `resize_pane_with_id` refuses fixed panes, and says nothing. `show_self` is a focus action. Each cost a round. Cite the path you read, so the next agent can grep it.
- **The model is pure; the shell is thin.** `model.rs` runs without zellij, and that is the only reason the bar is testable. Logic that moves into the event shell leaves the tier we test well, and enters the tier that needs a human at a terminal. Move it back.
- **One code path.** The sandbox is the same code as the stable build, with three environment variables moved. Never branch on "am I in dev".
- **Claude is one agent kind, not the only one.** clave will drive other CLI agents; name the seams for agents in general.
- **Write down what cost you time.** Trap → FOOTGUNS.md. Term → UBIQUITOUS_LANGUAGE.md. Dead end → the subsystem's C-section. Using clave → README.md. Working on clave → CONTRIBUTING.md. Not here: this file is the index, not the knowledge.

## Guardrails

- **Do not touch Ollie's live session.** You run inside it, so a bare `zellij` command hits his working fleet; run nothing against it, not even a read. Against your worktree's sandbox, run `zellij action` freely, staged with `just sandbox`.
- **Ollie launches every session**, runs `just release`, and owns anything that writes `~/.local/share/clave/`. Print the command; he runs it.
- **Ollie kills sessions.** One exemption: the sandbox you asked him to launch this conversation, once its drive and both eyeball checks are done. Kill it by explicit name (`clave dev instance --field session`), never another agent's.
- **Remote surfaces wait for his go:** pushes, PRs, merges, issue writes.
- **Ask him to test what you cannot reach.**
- **`just gates` must be green before you commit.** It runs fmt, test, the wasm build, then clippy, in that order, because CI runs fmt before clippy.

## Great code here

Rust stable, over zellij 0.44.3. Write code that passes the gate by construction.

- **Make modules deep.** A module must hide more than it shows. When a file gets long, ask whether its interface got wider. A long file behind a narrow interface beats three shallow ones that pass state between them.
- **Design errors out of existence.** A type that cannot hold the bad state beats a branch that handles it. Conflating the minted and live uuids passed every test, then froze the row in the field.
- **Comments give the reason and the source.** A comment that repeats the line is noise. Name the measurement, the issue, or the file you read.
- **Write tests that fail when the logic flips.** Run `just mutants` over what the branch changed; it is expected, and deliberately not a gate.
- **Delete dead code.** Do not annotate around it.
- **The bar is read at a glance, all day.** Legibility is the feature: two states a person cannot tell apart are one state, whatever the model holds. Show what changed, not that something changed.

## Style

- **Use ASD-STE100 Simplified Technical English** in comments, new docs, and everything you write to Ollie. One meaning per word. Short sentences. Active voice. One instruction per sentence.
- **Ollie knows this product well and does not read the code.** Give him the decision, not the mechanism: what we chose, what we gave up, why. Never use a symbol he would have to grep.
- **Send prose after a run of tool calls**, because a message between calls is buried in tool output. The final message is a report, not a log.
- **Commits:** Conventional Commits, scoped to the crate or subsystem. Say what was wrong, how you know it is fixed, and cite the issue. Fill `.github/PULL_REQUEST_TEMPLATE.md` with `--body-file`.
- **Agree changes to this file with Ollie before you write them.**
