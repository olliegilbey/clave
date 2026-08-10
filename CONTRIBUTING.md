# Contributing to clave 🥁

Thanks for being here. clave is early — which means the good problems are still
unclaimed, and a small change can matter a lot.

This document gets you from clone to pull request. If anything here is wrong,
confusing, or out of date, **that is a bug** — open an issue, or fix it in a PR.
Documentation fixes are real contributions and are reviewed like any other.

New to the project? [README.md](README.md) explains what clave *is*.
[UBIQUITOUS_LANGUAGE.md](UBIQUITOUS_LANGUAGE.md) is short and worth five
minutes — "session" alone means three different things in this codebase.

---

## Quick start

You need **Rust** (stable), **[Zellij](https://zellij.dev)**, and
**[Claude Code](https://claude.com/claude-code)** on your PATH.

```bash
git clone https://github.com/olliegilbey/clave
cd clave
just setup-toolchain     # adds the wasm32-wasip1 target
just sandbox             # builds, wires an isolated sandbox, verifies it
```

`just sandbox` prints the exact command to launch it. **Run that yourself, in a
new terminal, outside zellij** — clave creates its own multiplexer session, so
launching from inside one nests them.

That gives you a throwaway `clave-test` session with synthetic agents, entirely
separate from any real clave install. Reset it any time with `clave dev reset`.

> **Use `just sandbox`, not `just dev-install`, unless you know you want the
> difference.** `dev-install` installs the working-tree CLI as `clave-dev`, so it
> leaves your `clave` command alone — but it *does* rebuild the sandbox bar wasm
> in place, which is unsafe while a `clave-test` session is running.
> `just sandbox` refuses while that session is alive, and verifies it touched
> neither `~/.cargo/bin` nor `~/.local/share/clave` before it prints anything.

## Two environments, one code path

clave builds its own daily driver, so there are two launch surfaces that must
never bleed into each other. There is exactly **one code path** — the sandbox is
the stable behaviour with three environment variables redirecting state and
artifacts, so it reproduces production by construction.

| | Day-to-day (stable) | Development (sandbox) |
|---|---|---|
| **Launch** | `clave`, in a non-zellij terminal | the command `just sandbox` prints |
| **Zellij session** | `clave` | `clave-test` |
| **State** (store, evlog) | `~/.local/state/clave/` | `~/.local/state/clave-dev/state/` |
| **Artifacts** (wasm, config) | `~/.local/share/clave/` | `~/.local/state/clave-dev/data/` |
| **Agents** | your real work | synthetic — `clave dev scenario <name>` |
| **Teardown** | never | `clave dev reset` |

### The sandbox column is per working tree

Those sandbox names are the **main checkout's**. Work from a linked git
worktree and the whole instance moves with it: a worktree directory named
`prune-wt` gets the session `clave-test-prune-wt` and the root
`~/.local/state/clave-dev-prune-wt`, session name and state, artifact and shim
directories all derived from that one key.

That exists because several agents work this repo at once, each in its own
worktree, and a single shared root meant the second one to stage silently
overwrote the first one's plugin binary and generated config — so an agent
launched a "sandbox" running someone else's build and measured it for a full
round. The worktree directory name is the key rather than a random id because
it stays legible in `zellij list-sessions`.

Ask rather than guess, and clean up abandoned ones:

```bash
clave dev instance             # this tree's session name and root
clave dev reap --dry-run       # sandboxes whose worktree is gone
```

`reap` deletes only directories, never a session. A sandbox whose worktree is
gone but whose session is still up is printed with the kill command for you to
run.

Two invariants:

- **No beta channel.** Promotion is one-way: validate in the sandbox → cut a
  version → it becomes stable. Nothing lives in between.
- **Claude's identity is never sandboxed.** The sandbox isolates *clave's*
  state only. `claude` always runs as the real you, with your real auth and your
  real `~/.claude`. Sandboxing it dragged auth along and broke session seeding;
  clave is a thin wrapper for terminal control, and your identity is not its
  business.

## The rule that matters most

**Never install over a running clave session, and never kill or launch a session
on someone else's behalf.**

A live session only ever loads the versioned files baked into the config it
generated at launch, so a normal release lands atomically at the *next* launch
and cannot disturb a running one. What breaks that safety is writing the
binary a session will reach for, or regenerating the config a session is
watching, while it is live. zellij live-watches `config.kdl` and hot-swaps
keybinds into running sessions — but the running plugin keeps its load-time
identity, so a regenerated config re-keys the keybinds to a plugin that isn't
there, and zellij's response to that miss is to **start a second one**.

The symptom is two sidebars and half-working navigation. It shipped once, in
v0.1.1. [FOOTGUNS.md](FOOTGUNS.md) has the mechanism and the one-line
diagnosis.

So: after any `just release`, `clave setup`, or `clave dev scenario` that
changes the baked binary or wasm path, **restart the affected session** before
pressing any clave key.

## Making a change

1. **Find or open an issue.** The backlog is public and is the invitation —
   start with [`good-first-issue`](https://github.com/olliegilbey/clave/labels/good%20first%20issue).
   For anything non-trivial, comment on the issue before you build, so you don't
   duplicate work in flight.
2. **Branch off `main`.** `main` is always releasable.
3. **Write the failing test first.** This codebase is test-first, and the model
   layer (`crates/clave-bar/src/model.rs`) is a pure state machine specifically
   so behaviour can be tested without a terminal.
4. **Run the gates before you push:**

   ```bash
   just gates    # fmt --check + test + wasm build + clippy — exactly what CI runs
   ```

**Two gate details that bite people:**

- **`cargo test --workspace` is load-bearing.** `default-members` excludes the
  wasm-only `clave-bar` crate, so a bare `cargo test` **silently skips 68
  tests** and exits 0. Use `just test` or the `--workspace` form.
- **`cargo fmt --all --check` runs before clippy in CI.** Hand-written code
  that clippy accepts can still fail the build. `just gates` runs both, in CI's
  order.

5. **On a pure-logic change, run the mutation check too:**

   ```bash
   just mutants  # cargo-mutants over the lines this branch changed vs `main`
   ```

   Deliberately **not** in `just gates` — gates run on every PR and must stay
   fast. A surviving mutant is a *finding*: a line you can change while every
   test keeps passing. Triage it in the PR dossier; never weaken a test to make
   one go away. Which change classes owe a run, and the six shapes of
   green-and-worthless test the habit exists for, are in
   [`docs/dev/TESTING.md`](docs/dev/TESTING.md).

Live, interactive behaviour is not covered by any automated test — that is what
[`docs/dev/TESTING.md`](docs/dev/TESTING.md) exists for.

## Before you change a subsystem

Two documents will save you a wasted afternoon:

- **[FOOTGUNS.md](FOOTGUNS.md)** — traps that already cost someone a round.
  Grep it the moment something behaves unexpectedly, *before* you start
  debugging. If you lose time to something new, add it.
- **[SUBSYSTEM-VALIDATION.md](docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md)**
  — the ledger of approaches tried and *why they failed*. Every forbidden path
  in there was expensive to learn.

And never trust an assumed Zellij behaviour. Read the vendored source
(`~/.cargo/registry/src/*/zellij-tile-0.44.3/`, `…/zellij-utils-0.44.3/`)
before building on it. `TabUpdate` reaches only the active tab,
`resize_pane_with_id` silently refuses fixed panes, `show_self` is a focus
action — each of those cost a round.

## If you are working with an AI agent

Plenty of contributors here will be. The repo is set up for it:

- **[AGENTS.md](AGENTS.md)** is the entry point — deliberately short, and it
  points at everything else. Most agent harnesses read it automatically.
- **[FOOTGUNS.md](FOOTGUNS.md)** is written to be *grepped*, with the error
  string or symbol at the front of each line. Point your agent at it.
- **[UBIQUITOUS_LANGUAGE.md](UBIQUITOUS_LANGUAGE.md)** stops the ambiguity that
  causes the most wasted agent turns.

Two things to hold your agent to:

- **It must not launch or kill zellij sessions**, or run anything that writes
  the stable surface. Have it print the command for you to run. A `zellij
  action` against a dead session blocks forever without erroring, which is a
  bad thing to hand an autonomous loop.
- **Verify what it cites.** Docs go stale. A claim about current behaviour
  should be checked against current source before it lands in a PR — several
  entries in FOOTGUNS.md were wrong when first collected, and were caught
  exactly that way.

Agent-authored contributions are welcome. Say so in the PR — it is useful
review context, not a mark against the change.

## Opening a pull request

1. **Conventional commits** — `fix(bar): …`, `feat(cli): …`, `docs: …`. The
   scope is usually the crate or subsystem.
2. **Explain the *why*.** The commit body and PR description should say what
   was wrong and how you know the fix works. Cite the issue.
3. **Push and open the PR.** CI runs `test` and `wasm-build` as required checks,
   plus `lint`; [CodeRabbit](https://coderabbit.ai) reviews automatically.
4. **Expect a couple of rounds.** Review here tends to find real things —
   respond to each comment saying how you addressed it, then resolve the thread.
   Disagreeing is fine and often right; say why.

Comments must all be resolved and the branch up to date with `main` before a
merge. Because merging one PR pushes the next one out of date, a second CI round
is normal, not a problem with your change.

## Style

Match the surrounding code. The one convention worth stating outright: **comments
explain *why*, not *what*.** They cite the spec section, the issue, or the
ledger finding that forced the decision. A comment that restates the code is
noise; a comment that records why an obvious approach was rejected saves the
next person a day.

## Where work is tracked

Public **GitHub issues**, exclusively — a visible backlog is the invitation, so
nothing lives in private notes.

- **Labels**: `bar`, `cli`, `harness`, `docs`, `upstream-watch`,
  `good-first-issue`.
- One milestone per version cut.

## Releases

Cuts are maintainer-owned: a semver tag on `main` plus `just release`, which
refuses unless the tree is clean and `HEAD` carries a matching `vX.Y.Z` tag. It
installs *versioned* artifacts and regenerates every generated reference to
point at them. Every cut ends with an interactive live test — see
[`docs/dev/RELEASE-RUNBOOK.md`](docs/dev/RELEASE-RUNBOOK.md).

You do not need to run any of that to contribute. `main` stays releasable; the
maintainer decides when to cut.

## Being decent

Be kind, assume good faith, and critique the code rather than the person.
Maintainer time is the scarcest resource here — a clear reproduction, a focused
diff, and a PR description that explains itself are the most generous things you
can bring.

## License

By contributing, you agree your contributions are licensed under the
[MIT License](LICENSE), the same as the rest of the project.
