# AGENTS.md — clave

**clave** is a Zellij fleet-orchestration sidebar: it launches and navigates a
fleet of `claude` agents as tabs in one multiplexer session, keyed off a store
the agents' hooks write into. It builds its own daily driver — the maintainer
lives inside a clave session while developing clave from a session inside it.

Two crates, one workspace:

| Crate | Target | What it is |
|---|---|---|
| `crates/clave` | host binary | the `clave` CLI — store, setup/release, KDL generation, hooks, the `dev` sandbox |
| `crates/clave-bar` | `wasm32-wasip1` | the sidebar plugin — pure state machine (`model.rs`) plus a thin zellij event/effect shell |

`crates/clave-types` carries the shared vocabulary. `main` is always releasable;
a `vX.Y.Z` tag plus `just release` is the promotion event.

## Read next, in this order

| Document | What it settles |
|---|---|
| [UBIQUITOUS_LANGUAGE.md](UBIQUITOUS_LANGUAGE.md) | the shared vocabulary. **zellij session vs agent session**, **title vs label**, gutter · cell · ink · chip · provenance. Short, and it unlocks every other document — "session" alone is ambiguous three ways in this codebase |
| [CONTRIBUTING.md](CONTRIBUTING.md) | the two environments (stable vs sandbox), the release model, the PR flow, where work is tracked — **and "The one leak"**, the PATH hazard that broke v0.1.1 in the field (#43, #44) |
| [docs/dev/TESTING.md](docs/dev/TESTING.md) | the three verification tiers, the risk taxonomy (change class → required verification), the escape record, and the live-validation SOP |
| [docs/status/](docs/status/) | the newest handoff **is** current state: what shipped, what was declined, what is mid-flight. Handoffs are tracked (#22 ruling). Read the newest before you plan |
| [SUBSYSTEM-VALIDATION.md](docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md) | the C-section for the subsystem you are about to change — the ledger of approaches tried and *why they failed*. Read it first; every forbidden path was expensive to learn |

Never trust an assumed Zellij behaviour. Read the vendored source
(`~/.cargo/registry/src/*/zellij-tile-0.44.3/`, `…/zellij-utils-0.44.3/`) before
building on it — `TabUpdate` reaches only the active tab, `resize_pane_with_id`
silently refuses fixed panes, `show_self` is a focus action. Each of those cost a
round.

## The autonomy contract (maintainer ruling, 2026-07-22)

An agent may **implement a tracked issue end to end**: branch, TDD, run the
review gauntlet below, open the PR with its verification dossier filled in, and
gate on green CI. It **asks the maintainer before merging, and may execute the
merge itself once approved.**

It **never**:

| Never | Why |
|---|---|
| launches or kills a zellij session | the human owns session lifecycle and is the only one who can see the screen; `zellij action` against a dead session **blocks forever without erroring** |
| runs `just release` | a release is a deliberate, tagged, watched act — the maintainer's, always |
| runs `cargo install` or `just dev-install` while the maintainer may be daily-driving | this is what broke production — see CONTRIBUTING "The one leak" (#43, #44). #44 stopped the bar resolving through `PATH` and #43b moved the dev binary to `clave-dev`, but a bare `cargo install` still writes `~/.cargo/bin/clave` and `dev-install` still rewrites the sandbox wasm in place. Assume he is driving; use `just sandbox` |
| writes anything under `~/.local/share/clave/` | that is the stable release surface — the versioned artifacts AND the `bin/clave` launcher (#43a). Only an install writes it: `just release`, or `clave setup` run from a release binary. Neither is yours to run |
| writes anywhere under `~/.claude/` | read-only source of truth: Claude's identity is deliberately **not** sandboxed, so a stray write hits the maintainer's real config and transcripts |
| commits without explicit approval | he signs the commits. You prepare; he approves |

The one sanctioned live mutation is hot-reloading the **sandbox** bar in the
`clave-test` session (exact command in TESTING.md). Everything else that touches
a live terminal is the human's — you print the command, you do not run it.

## Required review before requesting merge

**One lane is required, one is recommended**, every non-trivial change:

1. **REQUIRED — at least one independent adversarial reviewer** — a lane that
   did not write the code. In practice: a fresh agent briefed to attack the
   change, plus the PR bots (CodeRabbit CLI on the committed branch, Codex).
   Subagent-driven development satisfies this with its per-task reviewers plus
   the whole-branch review, provided those reviewers are genuinely independent
   of the implementer. **One such lane discharges the requirement.** The PR bots
   (CodeRabbit, Codex) are additional evidence when they actually run — they are
   third-party services that may be rate-limited or absent, so they never gate a
   merge on their own.
2. **RECOMMENDED — the vendored fugu review** — `.claude/commands/fugu-review.md`
   (blind multi-model dry-run review, consolidated by a verifier). Valuable, but
   token-heavy (four model lanes), so it is a recommendation, not a gate: run it
   when the change is subtle or the budget allows, and skip it — saying so in the
   PR — when independent adversarial review has already been thorough. It may not
   exist on branches cut before that lane landed.

**If you are a cloud or remote agent**, assume the external CLI lanes are
unavailable: `coderabbit`, `codex` and `gemini` are third-party binaries that a
container almost never has, and that need interactive auth even when present.
Do not opt into fugu's `cli_reviewers` there. If you run fugu at all, run only
its **model lanes** (they need nothing but the repo); the required adversarial
reviewer (lane 1) is always available as a fresh in-repo agent. Then say in the
PR which lanes actually executed — **a lane that did not run is not a lane that
passed**, and a dossier listing six lanes of which three were silently absent is
worse than one honestly listing three.

This is not ceremony. On this repo, independent lanes have repeatedly caught
defects the implementer and a single reviewer both missed:

- the **cross-process prune race** — a "retain these live ids" payload arriving
  after a new tab's bind unbinds a *live* agent (CodeRabbit CLI, #6/#26);
- the **clap `ArgAction` bug** — `clave collapse true` could never parse, because
  clap-derive makes a bare `bool` a flag, and nothing exercised the CLI layer
  (CodeRabbit CLI, #5/#13);
- the **stale width-seek anchor** — drift measured from a mid-flight emit anchor,
  parking the bar off-target (Codex, #4/#27).

Findings you **decline** are recorded in the PR with the reasoning, not dropped.

## Verification, in one paragraph

Three tiers. **Tier 1 (hermetic)** is everything that runs anywhere with no TTY
and no Claude: unit tests, the `model.rs` proptests, the real-KDL-parser
guardrail, the zellij version pin, CLI parse pins, sandboxed subcommand runs. It
is the tier you can complete unattended, and the gate is `cargo test --workspace`
— **`--workspace` is load-bearing**, a bare `cargo test` silently skips the whole
wasm crate. **Tier 2 (real zellij in an isolated `clave-it-<pid>` session)** does
not exist yet (#47, blocked on #44) — so today *nothing* automated crosses the
process/environment seam, which is exactly where the v0.1.1 breakage lived.
**Tier 3 (human)** is glyphs, colour, fonts, feel and the maintainer's real
fleet. Before you start, read the risk taxonomy in
[docs/dev/TESTING.md](docs/dev/TESTING.md) and find your change's class — it
tells you what you must produce, and whether the PR needs the
`needs-live-validation` label.

The four commands a PR must show green — or `just gates`, which runs exactly
these in this order:

```bash
cargo fmt --all --check      # CI's lint job runs fmt BEFORE clippy
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

**`cargo fmt --all --check` is a gate, not a nicety.** CI's `lint` job runs it
first, so hand-written code that clippy accepts still fails the build. This list
omitted it until 2026-07-25 and #66 duly went red on three hand-edited files
with every documented gate locally green. If you edit Rust by hand, run
`cargo fmt --all` before you commit.

## Handoff duty

Before you finish, clear, or hand back, write
`docs/status/YYYY-MM-DD-HHMM-clave-orchestrator.md` and include it in the PR.
Handoffs are **tracked** (#22 ruling) — they are the project's thinking log, and
the next session resumes from the newest one. Cover: what merged, what was
discovered, what was **declined and why**, and where work stopped.

Two mechanical notes: a worktree only sees *committed* handoffs, so a fresh one
written elsewhere is invisible to you until it merges; and the pre-commit PII
blocklist rejects private local paths in staged lines — genericize them (`~/…`,
`$TMPDIR/…`, `<repo>/…`). It has fired twice.
