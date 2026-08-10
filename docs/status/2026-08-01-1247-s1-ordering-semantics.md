# S1 ordering semantics (#56) — pick up with `/wayfinder`

## Task Overview

Build **S1 / issue #56** — "ordering semantics: total order, no ties, and rows
hold their place on close" — the frontier ticket on wayfinder map **#115**
(v0.1.3 fleet legibility).

It is a **build ticket**: it closes by merged PR, not by a resolution comment
(the map's Notes override wayfinder's plan-don't-do default).

Success = S1 landed per its spec: a seq-minted ordinal replaces the wall clock,
only a user prompt reorders, rows hold their index on close. Resolves #39.

**Start the session with `/wayfinder 56`.** It will claim the ticket (assign to
`olliegilbey`) before any work — an open, unassigned child issue is unclaimed,
and other sessions run in parallel.

## Reference Docs

- `docs/superpowers/specs/2026-07-22-S1-ordering-semantics.md` — **the build
  order. Read it; it is complete and current.** Useful slices:
  - `:10-21` the binding maintainer ruling (only a user prompt reorders)
  - `:115-152` **the semantics table** — 16 events, reorder yes/no, mechanism.
    Paste into #39 on close. Read this before anything else.
  - `:154-347` design: the `seq`-minted ordinal, ties, the demotion carry,
    rejected alternatives, the seam S2 needs, and **migration/compat** (`:303`)
  - `:349-805` implementation, file by file (types → store → hook → CLI →
    lsview → add → model comparator → docs)
  - `:806-914` test plan, incl. **`:854` tests that MUST change** — stated
    intentional decisions, not collateral damage
  - `:915-1070` live validation, 6 steps
  - `:1071-1112` risks; **`:1073` the hard S0 dependency** (now satisfied)
- `docs/superpowers/specs/2026-07-22-ux-defect-dossier.md:199+` — RC-C, the root
  cause. `:29` and `:31` map the maintainer's words to root causes; `:546` is
  the discriminator table row that tells RC-C apart from RC-A on screen.
- `docs/dev/TESTING.md` — risk taxonomy, and **§ the sandbox drive loop**
  (added this session) for the live validation.

## Current State

**S0 (#55) is merged and closed** — PR #120, on `main` as `fc93e95`. That was
this session's work and it was #56's hard blocker. #56 is now **open, unblocked,
unassigned** — the frontier.

Nothing for #56 has been started. No branch, no code, no spec changes.

Repo state at handoff:
- `main` is at **`7605168`**; the working checkout sits on `fix/123-dormant-glyph-ink`
  (**another session's branch — do not disturb it**; make your own worktree).
- Untracked: five `docs/status/*.md` files from prior sessions, plus this one.
  No uncommitted source changes from this session.
- **PR #130 merged** (`7605168`) — docs for the agent/zellij rule and the drive
  loop, after three Codex P2 corrections (a self-contradicting contract, a
  build-tag check that could not fail, and a reuse step that omitted the
  create).

## What's Working

**Build on these; they are verified and cost real time to establish.**

- **The S0 seam S1 was waiting for now exists and is tested.** An agent's
  `tab_id` names the tab its pane is in, or is `None` — S1 can assume that. Any
  ordering experiment before S0 was confounded by wrong binds, and the two were
  indistinguishable from the screen. That confound is gone.
- **`Effect::Touch` exists** (`clave-bar/src/model.rs`), emitted by
  `identity_effects()` and gated `Confirmed`. The S1 spec names it as where a
  monotonic ordinal stamp gets plumbed. It already has a retry trigger.
- **`elects_confirmed()` / `elects_presumed()`** are named, tested predicates.
  Use `Confirmed` for anything that retries; **do not** tighten `RenameTab`,
  `MarkRead`, `ReanchorVisit` or `PersistCollapse` — they latch at emit and have
  no retry, so a stricter gate silently drops them forever.
- **`bind_effects` / `identity_effects` are mutation-clean** (all 25 mutants
  caught) and covered by 2 proptests plus ~17 unit tests in `model.rs`. That
  suite is your safety net when you touch the comparator — it will catch an
  ordering change that breaks binding.
- **The sandbox drive loop works and is documented.** `just sandbox
  [scenario]` stages everything; the loop is in `docs/dev/TESTING.md`. S1 has
  6 live steps and they are cheap to run this way.
- **Agent/zellij rule (ratified 2026-08-01):** drive `clave-test` freely with
  `ZELLIJ_SESSION_NAME=clave-test zellij action …` **including tab actions**;
  run **nothing** against Ollie's session, not even a read; session lifecycle
  (launch/kill, even of `clave-test`) is always his. This is narrower than it
  sounds in one direction only — you may not read his session; everything in the
  sandbox is yours.

## Important Discoveries

- **The symptom is live and Ollie raised it again today**, unprompted: *"a lot
  of tabs aren't moving to the top when I interact with them."* Three root
  causes produce that symptom; **RC-A and RC-B are now fixed (S0), RC-C is
  not** — RC-C is #56. RC-C fits "a lot of tabs" best because it needs no race:
  whole-second timestamps with ties broken by tab position fire deterministically
  whenever two prompts land in the same second.
- **A clean fleet snapshot is not an all-clear.** RC-A was a race and was sticky
  *per plugin instance*; bars reload. Verified Ollie's live fleet mid-session:
  every bound agent matched its real tab, no duplicates. That does not mean the
  bug was not biting.
- **`list-panes -t -j` reports a pane's DEEPEST CHILD, not the agent.** Live
  agent panes showed `rust-analyzer`, `uv … run spotify-mcp`, `caffeinate -i -t
  300` where `claude --resume <uuid>` was expected. Only tabs whose
  `pane_command` names `claude` are joinable to a uuid; **the rest are unknown,
  not mismatched.** Reading them as mismatches invents a bug; filtering them out
  hides a real one. Now in FOOTGUNS. (Extends the pre-existing `dump-layout`
  entry to `list-panes`.)
- **Stage the sandbox with `just sandbox`, never by hand.** Hand-staging cost a
  round this session, twice: `run_setup` rewrites the data dir so a wasm copied
  *first* is silently replaced, and the sandbox bakes a **bare `clave`** resolved
  from PATH — so without the shim the bar shells out to the *stable* binary. The
  second one nearly produced a **false pass**: a CLI-side assertion would have
  measured a binary that lacked the feature under test.
- **Failed approach — installing a versioned CLI into the sandbox.** Putting
  `clave-v0.1.2` in `<sandbox data>/bin/` does **not** make `runtime_binary()`
  bake it. `run_setup`'s dev/sandbox branch hardcodes bare `clave`
  (`setup.rs`, the `else` arm). Use the PATH shim.
- **Concurrent sessions share this checkout.** Another session switched the main
  working tree to its own branch mid-work and left uncommitted files. Work in
  `git worktree add .claude/worktrees/<name>`, and when a shared file (FOOTGUNS,
  AGENTS.md) has someone else's uncommitted hunks in it, stage only your own
  (build a filtered patch and `git apply --cached`).
- **The wayfinder map body is last-write-wins** — concurrent agents clobber it.
  **Comment on #115 instead of editing the body.**
- **Three review passes each found something real on S0.** Do not assume a green
  suite means done: Codex found an unbounded retry loop the cap was supposed to
  bound; an adversarial subagent found a regression the PR itself introduced
  *plus* its starvation mirror; CodeRabbit found stale citations. Budget for
  ≥2 rounds. **CodeRabbit reports `pass` while rate-limited** — that is not a
  review; nudge it with `@coderabbitai review` and check for actual threads.
- **Tests that cannot fail prove nothing.** Two S0 proptests initially *passed*
  against a deliberately disabled coherence witness because they derived the
  property from the function under test. Always re-run new tests against a
  reinstated defect.

## Next Steps

1. **`/wayfinder 56`** — it claims the ticket, then read the S1 spec's semantics
   table (`:115-152`) before touching code.
2. Branch in a worktree off current `main` (`fc93e95` or later).
3. Implement per spec §4, in its order: types → store (mint/carry/backfill) →
   hook (the one reordering writer) → CLI → lsview → add → model comparator.
   **Migration matters** (`:303-347`) — existing stores carry `tab_timeline`.
4. Tests: §5.1 new, **§5.2 the ones that must change** (intentional — say so in
   the PR), §5.3 proptests. Then `just gates` and `cargo mutants` over the
   comparator and the mint.
5. Live-validate via the sandbox drive loop; S1's own 6 steps are at `:915`.
6. PR with `needs-live-validation`; paste the semantics table into **#39** on
   close; comment the decision onto map **#115**.
7. PR #130 is **merged** (`7605168`) — the drive loop and the agent/zellij rule
   are on `main`, reviewed and corrected. Nothing owed there.

**Open question for Ollie, not yet asked:** whether the tab-identity refactor
(below, in Context) should be sequenced before or after #57/#112, since it would
shrink the surface both build on.

Where work stopped — Ollie, verbatim:

> "merged and deleted will you sync main, then write a /handoff for yourself to
> pick up #56 with /wayfinder after this so that we can pick up with fresh
> context, but remembering what is useful for 56. read 56 first."

And immediately before, on the symptom that makes #56 urgent — verbatim:

> "Okay, that's worrying. I think a bunch of issues are caused by that, a lot of
> tabs aren't moving to the top when I interact with them - does it mean that
> clave doesn't treat them the same?"

What he endorsed this session — verbatim:

> "Your driving was very cool, we should document how to do this, while making
> sure that agents don't touch my live session, so they are pedantic about only
> interacting with the sandbox. It's a useful testing methodology."

That "driving" was: stage the sandbox → verify the build tag in the zellij log →
baseline → provoke with real tab closes → re-join store against pane truth after
each → measure a 60s idle → force the case the script missed → report what was
*not* exercised. Reuse that shape for S1.

## Context to Preserve

- **Explain in plain English.** Ollie pushed back hard on jargon mid-session
  ("what language are you speaking?"). He is an expert PM, not reading the code
  with you. Lead with the outcome; give the mechanism in ordinary words.
- **He will challenge design as well as code** — he asked whether the whole S0
  approach was over-engineered and whether tabs could just know their own
  identity. That was a good question and it produced a real finding (below).
  Verify before answering; do not recite the spec back.
- **Report failures and self-inflicted messes plainly.** This session put 117
  stray lines in his live `~/.local/state/clave/clave.log` via a logging helper
  that ignores the store paths it is handed; disclosing it with the fix and a
  cleanup command was the right move. `evlog::log_event` resolves the state dir
  from the environment — use **`log_event_in(&paths.dir, …)`** anywhere you
  already hold a `StorePaths`, or unit tests write into his live log.
- **Promised and not yet done — the tab-identity refactor.** Agreed with him as
  a *separate PR, next*, and never started. The finding: clave creates every
  agent tab via `zellij action new-tab` from the CLI, which returns nothing, so
  the tab's identity is discarded at the moment it is known and every sidebar
  copy spends the session reconstructing it. The cheap fix is to ask
  `zellij action list-panes -t -j` **once, right after creating the tab**, and
  record the id (the agent's uuid is in the pane's command line). The earlier
  spec rejected that command as a *per-event* mechanism — it melted the zellij
  server once — but once per tab creation is a different proposition, and tab
  creation already pays for a `dump-layout`. **Unverified:** whether
  `list-panes` reliably sees the new pane immediately after `new-tab` returns.
  This would not delete S0's guard (rename/prune/nav still need own-identity)
  but would stop correctness depending on it.
- **Never** run `just release`, `just dev-install`, `cargo install`, or write
  `~/.local/share/clave/` — that is his daily surface.
- `~/.claude` is effectively read-only; note that **`clave dev reset` deletes
  c85c-tagged transcripts from `~/.claude/projects`**. It is scoped, but prefer
  re-running `dev scenario` (re-runnable by design) over `reset`.
- **The sandbox is currently up** (`clave-test`, `c8-cold-start`, carrying the
  S0 build `s0-49008a2`). Teardown is his: `clave dev reset` prints the kill
  line first.
- `needs-live-validation` on merged PRs batches into the pre-tag pass (#49); it
  does not block merge.

## Restart Hint

Safe to `/clear` — no uncommitted source changes from this session, everything
is merged or pushed. The shared checkout is on another session's branch, so
start with `git worktree add .claude/worktrees/s1-ordering -b <branch>
origin/main` rather than switching it.
