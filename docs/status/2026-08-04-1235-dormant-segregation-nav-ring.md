# Live/dormant segregation + live-only nav ring (#112) — pick up with `/wayfinder`

## Task Overview

Build **issue #112** — "dormant rows crowd the nav ring; separate them, and
retire the truly dead" — on wayfinder map **#115** (v0.1.3 fleet legibility).

Two deliverables, both settled by design already:

1. **Segregate the list.** Live rows form a contiguous block at the top; dormant
   rows sit in their own block below. Both blocks order by the commitment
   ordinal that #56 just landed; the dormant block is most-recently-closed first.
2. **Live-only nav ring.** `Alt+j`/`Alt+k` wrap *within the live block*. You
   cannot walk into the dormant block. Dormant rows are reached by mouse click
   or `Alt+1-9`.

It is a **build ticket**: it closes by merged PR, not a resolution comment (map
#115's Notes override wayfinder's plan-don't-do default).

**Start with `/wayfinder 112`.** It claims the ticket (assign `olliegilbey`)
before any work — an open, unassigned child issue is unclaimed and other
sessions run in parallel.

**#112 is now unblocked.** Both its blockers are closed: #56 (merged as PR #135,
`94bec1e`) and #116 (the design decision).

## Reference Docs

- **Issue #112's own body is the build order.** Read it first; it is complete,
  current, and short. It names the two deliverables, the zero-live-rows edge
  case, and what is deliberately excluded.
- **#116's resolution comment** — the design ruling, §5 (segregation) and §6
  (the nav ring): https://github.com/olliegilbey/clave/issues/116#issuecomment-5147634478
  Read §5's reasoning before touching the comparator; it is the argument for why
  segregation does *not* reintroduce the bug it appears to.
- `docs/superpowers/specs/2026-07-22-S1-ordering-semantics.md` — #56's spec, now
  merged. Useful slices only:
  - `:115-160` the semantics table (17 rows). **Row 14 already describes what a
    close looks like under your segregation** — it was written for you.
  - `:247-270` §3.3, the demotion carry and **the one ranking rule for both row
    classes**. This is the thing you must not break. See "What's Working".
  - `:915-935` §5.2, the table of tests changed by #56 — one row explicitly
    marks a test as **yours** to change.
- `docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md` — the visual
  lock wins any dispute. Segregation changes row *grouping*, so check §2's
  position-lock before inventing a separator row or a header.
- `docs/dev/TESTING.md` — risk taxonomy (this is **pure logic / model**, so TDD
  red-first plus `cargo test --workspace`) and § the sandbox drive loop.

## Current State

**Nothing for #112 has been started.** No branch, no code. Re-verified
2026-08-07: still OPEN, still unassigned, no branch or PR bears its number, and
`rows()` on `main` still builds ONE merged list. Both blockers (#56, #116) are
closed, so it is ready and unclaimed.

### Changed since this file was first written (verified 2026-08-07)

`main` has moved from `94bec1e` to **`960c8db`**. Three commits, one of which
matters a lot to you:

- **#100 shipped** (PR #128, `15b03a6`) — *"Alt+Enter is the only act that wakes
  a dormant row"*. This is #112's sibling from the same design ruling, and it
  landed in the same code you are about to change: the cursor, the dormant
  selection, and what a nav step does when it lands on a dormant row. **Read its
  diff before you touch the nav ring** — #116 specified the two together, and
  #100 built the half that decides what selection *means*. Your half decides
  what the walk can *reach*.
- #131 (row summary → away-period recap) and a docs commit. Neither touches you.

Two neighbouring tickets appeared that did not exist when this was written:

- **#149 — `clave prune --idle-days <N>`, retire long-idle dormant rows.**
  Claimed, with a branch. This is the retention work #112's body explicitly
  split out. **Not yours**; do not absorb it.
- **#148 — the sidebar has no viewport: rows past the pane height are reachable
  but invisible.** Unclaimed. Directly adjacent — both tickets exist because the
  row list is too long — and worth a moment's thought about whether segregation
  changes what #148 needs. Do not build it here.

Everything in "What's Working" below was re-checked against `960c8db` and still
holds. Line numbers have drifted by ~15 lines in `model.rs`; the names are
current, so search by name rather than trusting a number.

**What #56 delivered that you consume:** the wall clock is gone from row
ordering. Every row now carries a *commitment ordinal* — a strictly increasing
integer minted by the store under its lock, once per user commitment. Ties are
unreachable for committed rows, and a closed tab's ordinal is carried onto its
row so the row keeps its rank.

## What's Working

**Build on these. They are verified green and cost real time to establish.**

- **One ranking rule for both row classes, and it is load-bearing for you.**
  `live_ord` and `dormant_ord` in `crates/clave-bar/src/model.rs` both compute
  `max(the row's own ordinal, its tab's ordinal)`. That identity is not an
  accident and it is not decoration: it is what makes closing a tab unable to
  change a row's rank. **Segregation must not break it.** You are changing which
  *block* a row renders in, not what number ranks it. If you find yourself giving
  dormant rows a different key, stop — that is the defect two reviewers caught on
  #135, arriving by a third route.
- **The close tests are already segregation-proof.** `close_does_not_reorder_neighbours`,
  `close_holds_position_before_the_prune_lands` and
  `prop_close_preserves_relative_order` all assert *relative order* and *ranking
  keys*, never a literal row index — deliberately, so your ticket inherits them
  intact rather than rewriting them. If one of them fails when you segregate,
  that is a real bug in your change, not fixture drift. Treat it as a signal.
- **Exactly one test is yours to change:**
  `dormant_rows_sort_into_the_unified_recency_order` (search by name). Its
  comment says so outright, and the spec's changed-tests table says so at
  `:922`. It pins the single merged list that segregation replaces. Expect to
  rewrite it into two block assertions; that is intended, not collateral damage.
- **`rows()` is where segregation goes** (`model.rs`, search `pub fn rows`). It
  currently builds one `entries` vec of `(ordinal, tiebreak, (RowKey, Row))` and
  sorts descending. The tiebreaks (`t.position` for live, `usize::MAX - i` for
  dormant) are **no longer the ordering mechanism** — ordinals are unique by
  construction — they survive only as a determinism residual for rows at zero
  and for a transient eviction window. Say what you do to them in the PR.
- **The nav walk is one expression** (`model.rs`, `pub fn nav`, around the
  `"next"`/`"prev"` match): `(cur + 1) % rows.len()` and its mirror. That
  `rows.len()` is the whole ring. Restricting the ring to the live block is a
  change to this arithmetic plus a live-block length; it is not a rewrite.
- **The model suite is your safety net and it is strong.** 131 tests in
  `clave-bar`, including seven property tests over ordering, binding and
  collapse. `bind_effects`/`identity_effects` are mutation-clean. If you break
  ordering or binding, this suite tells you.
- **The sandbox drive loop works and is documented** (`docs/dev/TESTING.md`).
  `just sandbox [scenario]` stages everything. You may drive `clave-test` freely
  with `ZELLIJ_SESSION_NAME=clave-test zellij action …`, **including tab
  actions**. Launching or killing any session is Ollie's.

**What this does NOT cover:** none of the above says anything about how the two
blocks should *look*. There is no separator, header, or blank row in the design
today, and #116 did not ask for one. Check the visual design lock before adding
any, and expect Ollie to have an opinion.

## Important Discoveries

- **One live-validation step still describes the pre-segregation world.** The
  spec's step-4 expected-result table (`:1085`) reads *"every surviving row kept
  its index; the closed tab's row is at the **same index**"*. That was left
  deliberately — it is accurate for what shipped in #56 — but it becomes **false
  the day you segregate**, because a demoted row moves to the head of the dormant
  block. **This is yours to edit**, and it is the one item from #112's
  "#56 must be edited first" list that was consciously not done. Every other
  item on that list (the rule statement, the unstamped-tabs argument, the worked
  example and coherence note) was completed inside #135.
- **Segregation was rejected once, by name, and then overruled.** #56's spec
  originally argued the single merged list was load-bearing and rejected
  segregation in one line. #116 overruled it and #135 withdrew the argument, but
  the reasoning matters and is written down: the original objection assumed the
  only way to avoid an untouched row jumping was to keep the closed row's literal
  index. Segregation avoids the same symptom differently. **You will re-derive
  this argument if you do not read #116 §5 first.**
- **Three reviewers each found something real on #56.** CodeRabbit and Codex
  independently found the *same* ordering defect by different routes; a third
  agent found a table omission and a fixture that could not fail. Budget for ≥2
  review rounds. **CodeRabbit reports `pass` while rate-limited — that is not a
  review**; nudge with `@coderabbitai review` and check for actual threads.
- **Tests that cannot fail are the recurring failure mode on this workstream** —
  three near-misses on #56 alone. A property test whose agents all carry the same
  value, a status loop pushing a stale version number, a fixture already at the
  value a bug would write. **Verify every new test against a deliberately
  reinstated defect.** It caught two defects on #56 that a green suite did not.
- **The dormant block is big.** Measured on the real store 2026-07-31: 21 rows,
  4 live, 17 dormant, 3 of those stale. So the live block is short and
  `Alt+1-4` reaches the whole live fleet — which is the entire argument for
  putting it on top.
- **Retention for stale rows is NOT this ticket** (#124 owns it, and its first
  test already landed on main). Three of the 21 rows have a vanished working
  directory and cannot be resurrected — dead, not dormant. A failed open
  deliberately mints nothing, which is the mechanism by which those rows sink.
  **Do not add a mint anywhere near `apply_open_result`**; a test pins this.
- **Concurrent sessions share this checkout.** It moved between four branches
  during #56's session, and `origin/main` moved twice mid-PR. **Work in
  `git worktree add`**, and run `git worktree list` before assuming your cwd —
  a `cd` for one command persists and I twice ran commands against the wrong
  tree from it (one wrote a file into another session's working tree).
- **The wayfinder map body is last-write-wins** — concurrent agents clobber it.
  **Comment on #115 instead of editing the body.**

## Next Steps

1. **`/wayfinder 112`** — claim it, then read #116's resolution comment §5 and
   §6 before touching code.
2. Branch in a worktree off current `main`:
   `git worktree add .claude/worktrees/s112-segregation -b fix/112-dormant-segregation origin/main`
3. Decide the **zero-live-rows** edge case first — #112 names it as undecided and
   suggests walking everything as the obvious fallback. It changes the shape of
   the nav arithmetic, so settle it before writing it.
4. Implement: `rows()` block grouping → the nav ring's live-block length →
   `Alt+1-9` row jump (confirm it still indexes the *rendered* list).
5. Tests: two-block ordering, ring wraps within the live block only, zero-live
   and one-live cases. Rewrite `dormant_rows_sort_into_the_unified_recency_order`.
   Confirm the three close tests still pass **unchanged** — that is the signal
   your change preserved the ranking rule.
6. Edit the spec's live-validation step at `:1085` (see Discoveries).
7. `just gates`, then `cargo mutants` over the block split and the ring.
8. PR with `needs-live-validation`; comment the decision onto map **#115**.

**Open question for Ollie, not yet asked:** whether the two blocks need any
visual separation, or whether position alone carries it.

**Also outstanding, not blocking:** #136 (filed 2026-08-02) — a row's branch and
worktree are recorded once at add time and never refreshed, so the provenance
glyph is confidently wrong for every worktree-driving session. Ollie spotted it
from the sidebar. Unowned.

Where work stopped — Ollie, verbatim:

> "merged, sync main please. Let's do 112 in a new session, so /handoff what is
> useful to that agent session, don't worry about keeping much context of what
> we are doing, unless it's worthwhile for 112. Set yourself up nicely with a
> good document to pick up from."

Earlier in the same session, on how to read a reviewer — verbatim:

> "another agent spotted something and commented on the pr, have a read and let
> me know what you think."

And what he endorsed and acted on, verbatim, after being shown a live/dormant
ordering defect explained without jargon:

> "cool, merged, sync up main please."

## Context to Preserve

- **Response register — this is enforced and was corrected hard mid-session.**
  `OPUS.md` § Response style now carries four rules that can be failed: no
  unglossed symbols (anything you'd have to grep gets cut or explained in
  ordinary words *in the same sentence*), **six sentences** for a report, decision
  not mechanism (what we chose / what we gave up / why — internals only on
  request or via `/teach`), and state-don't-argue. Ollie is a product manager who
  knows clave cold and **does not read the code**. The failure mode is the
  consolidating message at the end of a tool-call stream; write it as a report,
  not a summary of what you did. He said of an earlier attempt: *"that doesn't
  even seem to be English."*
- **He will challenge design, not just code.** On #56 he questioned whether the
  whole approach was over-engineered. That produced a real finding. Verify before
  answering; do not recite a spec back at him.
- **Drive the sandbox; never touch his session.** He dog-foods clave daily and
  the Claude you are runs *inside* a live clave session, so a bare `zellij`
  command targets his working fleet. Against `clave-test` you may run `zellij
  action` freely; against his session you run **nothing, not even a read**.
  **Launching or killing any session is his**, as is `just release`,
  `just dev-install`, `cargo install`, and anything writing
  `~/.local/share/clave/`. Print those; let him run them.
- `~/.claude` is effectively read-only. Note that `clave dev reset` deletes
  c85c-tagged transcripts from `~/.claude/projects`; prefer re-running
  `dev scenario` (re-runnable by design) over `reset`.
- **Never write into his live log from a test.** `evlog::log_event` resolves the
  state dir from the environment — use `log_event_in(&paths.dir, …)` anywhere you
  hold a `StorePaths`, or unit tests write into `~/.local/state/clave/clave.log`.
  A prior session put 117 stray lines there this way.
- **Report failures and self-inflicted messes plainly.** That has landed well
  every time, including admitting a command run against the wrong working tree.
- `needs-live-validation` on merged PRs batches into the pre-tag pass (#49); it
  does not block merge. **Nothing on this workstream has been live-validated
  yet** — #55, #56 and their predecessors are all batched for one pass before the
  v0.1.3 tag. Ollie is aware and chose that sequencing deliberately.

## Restart Hint (re-verified 2026-08-07)

The shared checkout is now on **`s7-context-battery`**, another session's branch,
and there are **five other live worktrees** — #131, #137, #139, #149 and a codex
profile. Do not switch the shared checkout. Branch in your own worktree off
`origin/main` (`960c8db`), and run `git worktree list` before assuming your
working directory: a `cd` persists between commands and has already caused a
write into the wrong tree once.

## Restart Hint (original)

Safe to `/clear`. Working tree is clean apart from untracked status docs, main
is synced at `94bec1e`, and #56 is merged and closed. Start with
`git worktree add` rather than switching the shared checkout — other sessions
are using it.
