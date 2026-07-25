# Status — claude-codex profile: re-review found a session lockout; fixes STAGED, unsigned

_2026-07-25 00:57 · worktree `.claude/worktrees/claude-codex-profile` ·
branch `worktree-claude-codex-profile` · **tip `e5f8500` (pushed) + STAGED,
UNCOMMITTED work** · base `main` `50fa26a` · PR #65 draft_

Predecessors: @docs/status/2026-07-23-0954-clave-orchestrator.md (the commit
rationale) and @docs/status/2026-07-23-0959-claude-codex-profile.md (a **peer
session's** review — this is where finding I1 below came from, and it is
committed alongside this file because it was sitting untracked in the worktree).

## ⚠️ Read this first: work is staged but NOT committed

`git commit` hangs on the 1Password signing prompt with no one awake to approve
it. I did **not** commit unsigned and did **not** commit as Claude (the
sign-with-fallback hook explicitly forbids the latter). Everything is staged and
green. To land it:

```bash
cd .claude/worktrees/claude-codex-profile
git commit -F <scratchpad>/round2-commit-msg.txt   # message prepared, verbatim
git push
```

The prepared message and a `round2.patch` backup are in this session's scratchpad.
Nothing is lost if the worktree is untouched — the changes are staged in git.

## What this session did

Re-reviewed the branch with fresh eyes (the maintainer's request), then fixed
what the review found. **Gates verified by me after every change**:
`cargo fmt --check`, `cargo test --workspace` (exit 0, zero failures — checked
unmasked, not through a pipe), `cargo clippy --workspace --all-targets -D
warnings`, `cargo build -p clave-bar --target wasm32-wasip1`.

### I1 — cold-start Codex lockout (FIXED; the important one)

**My earlier review and all three fugu lanes cleared this as "spec-sanctioned,
no action". They were wrong, and so was I.** A peer session caught it.

`launch_session`'s cold-start branch hard-failed when the eager row used the
wrapper and the wrapper did not resolve. Bare `clave` **is** `launch_session`,
and `clave add` only runs **inside** a session — so the Err created no session,
left no tab to reach, and left no way to displace the eager row. One dormant
row's *optional* launcher locked the user out of the entire tool. Strictly
harsher than the plain-`claude` eager path, which launches and fails only in its
own pane.

The trap was in the plan's own wording: "a dead-session preflight failure leaves
the store intact and creates no Zellij session" reads as the *safe* outcome. It
is the lockout. **Generalisable lesson: "fails closed" is only safe if the
recovery path does not live behind the thing that failed.**

Fix: `setup::eager_wrapper_warning` (pure, TDD red→green, 4 cases) warns and
never Errs; the session launches and the eager pane surfaces the real error from
`clave spawn`. `open.rs` keeps its hard failure and now carries a comment
explaining why the asymmetry is deliberate — it aborts one row inside a live
session, so the blast radius is a retry, not a lockout.

### Two guards that did not guard (FIXED)

- **`spawn_launch.rs` anti-shell-wrapping assertion was unfalsifiable** — it
  looked for a file named `false`, but `$(false)` creates none and the cwd was
  the wrong directory. Replaced with canaries a shell genuinely leaves behind;
  **verified falsifiable** (a real `sh -c` with that argv creates both, clave's
  exec creates neither). The exact-argv equality remains the primary guard.
- **Profile flips on resume were invisible** — `Alt a` is bound to plain
  `clave add` in a `close_on_exit` pane, so resuming a codex agent by the bound
  key silently converts it and any printed warning dies with the pane. Added
  `resume_profile_flip` (pure, 6 cases) + an event-log line. Semantics
  unchanged; read under the same lock that writes it.

## Decisions waiting on you (I did not make these)

1. **Should resume preserve the stored profile instead of overwriting it?**
   Today the requested flag wins — your approved design table. The trap is that
   `Alt a` can only ever request *plain*, so the bound key is a one-way
   downgrade. **Key finding: preservation is implementable without breaking the
   immutable-snapshot invariant** — `run_add` already loads the existing row
   *before* building the KDL, so the effective profile can be resolved
   pre-tab-creation, exactly as today. Needs a `--no-codex` to stay explicit.
   *I rejected adding an `Alt A` codex bind:* zellij 0.44.3 normalises `Alt A`
   → `Alt Shift a` (`data.rs:116-149`) so it would not collide, but whether the
   **terminal delivers it** is tier-3 and unverifiable without you. Shipping an
   untestable keybind is the assumed-zellij-behaviour trap.

2. **Separate the wrapper handshake env var from `CLAVE_CLAUDE_BIN`?**
   It is overloaded: your discovery override *and* the wrapper handshake. That
   overload is why the plain path `env_remove`s it, which strips your override
   from anything an agent shells out to in-pane. Low severity (the peer judged
   it harmless; discovery falls back, and the Alt+a pane comes from the zellij
   server env). **Not changed deliberately** — it alters the wrapper contract
   *you must implement*, and the wrapper does not exist yet, so now is the
   cheapest moment to decide. Recommendation: dedicated variable, stop stripping.

3. **File F2 as an issue?** Peer finding, **pre-existing and out of scope**: a
   failed `clave open` leaves the bar's `↻` stuck and the dwell/click retry
   inert, because `RunCommandResult` never clears `opening` on nonzero exit.
   Affects every bail-before-tab path, not just codex. Correct fix is to pass
   the uuid as run_command context and clear it on failure. I did not file it —
   your public repo, you were asleep. Say the word.

## State of play

- **PR #65** (this branch) — draft, CI green at `e5f8500`, `needs-live-validation`.
  The staged work above is **not** in it yet.
- **PR #66** (#44, plugin binary path) — **not mine**: that session (PID 38725)
  is still alive after 2 days. Its `lint` check is red and it is a
  `cargo fmt --check` failure in the `resolve_binary` test — one-line fix, left
  for that session. It has since added a `just gates` standing rule to CLAUDE.md.
- **#65 ↔ #66 will conflict** in `crates/clave/src/add.rs` — confirmed by
  `git merge-tree`; `setup.rs` and the guardrail auto-merge. Semantically
  disjoint (plugin node vs spawn args), so it is a mechanical rebase. **#66
  should land first.** I deliberately did **not** do the `tab_node` params-struct
  refactor I recommended, because it would widen that conflict; do it after.

## Still blocking merge — unchanged

Live validation is yours and cannot be done until Codex usage returns: the real
`claude-codex` wrapper (it does not exist yet), `claude-codex --version`, a short
`--no-session-persistence` inference, and human Zellij acceptance (plain +
`--codex`, dormant profile switch, live jump, dead-session resurrection, a real
worktree row; one bar per tab, single loaded version).

**Add to that list, from I1:** cold-start with a codex eager row and the wrapper
**removed** — the session must now come up with only that pane erroring. And a
dormant `--codex` open with the wrapper removed (the F2 stuck-`↻` path).

## Restart hint

Sign and push the staged commit first, then update PR #65's body — its dossier
still describes only the original commit and does not mention I1.
