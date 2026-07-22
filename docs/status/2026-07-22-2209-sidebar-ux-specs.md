# Status — sidebar UX defects: dossier + eight workstream specs (PR #64 open)

_2026-07-22 22:09 · repo github.com/olliegilbey/clave · branch
`docs/sidebar-ux-specs` @ `289d541` · base `main` @ `50fa26a` · tag `v0.1.1`_

Predecessor: @docs/status/2026-07-22-1845-clave-orchestrator.md — the v0.1.1
PATH incident, the autonomy contract, and the #44 spec. Read it only if you need
release/install context; this file is self-contained for the UX work.

## Task Overview

Two requests, in order.

**1. Version-cut readiness (answered, handed to another agent).** "What's in
since v0.1.1, what should we do before live-session testing?" Answer: **do not
cut v0.1.2 yet** — it deterministically reproduces the 2026-07-22 double-sidebar
incident. Detail in *Important Discoveries*. The maintainer took this away:
*"Okay, I'll tackle those with another agent."*

**2. The real task: four UX defects from daily driving**, researched and specced
so **one agent per worktree** can implement each, with a live-validation script
the agent drives and the maintainer executes. Success = a fresh agent with only
its spec and the repo can do the work and walk him through validating it.

Constraints he set: separate spec per workstream (different agent session and
worktree for each); every live step must tell him what to look at and what to
report; he drives all live zellij input.

## Reference Docs

All committed on this branch. Read the dossier first — the specs assume it.

- `docs/superpowers/specs/2026-07-22-ux-defect-dossier.md` — **read whole (586
  lines)**. Shared research: 4 symptoms → 7 root causes, every claim with
  `file:line`. §"Read-only live diagnosis" (L470-520) is the diagnosis table;
  §"Workstream split and sequencing" (L522-560) is the dependency graph.
- `…/2026-07-22-S0-frame-coherence.md` (1100) — RC-A/RC-B. **Land first, alone.**
- `…/2026-07-22-S1-ordering-semantics.md` (1115) — RC-C. Depends on S0.
- `…/2026-07-22-S2-terminal-interaction-signal.md` (1116) — RC-D. Spike then build.
- `…/2026-07-22-S3-tab-close-correctness.md` (1443) — RC-E. Depends on S0+S1.
- `…/2026-07-22-S4-label-rename-and-live-cwd.md` (1746) — RC-F.
- `…/2026-07-22-S5-per-repo-colour.md` (1616) — RC-G.
- `…/2026-07-22-S6-gutter-glyphs.md` (1960) — §2.8 L616-760 is the **open
  collapsed-mode decision**; §5 Step 1 L1679 records the glyph probe.
- `…/2026-07-22-S8-sidebar-width.md` (1162) — 30→38.
- `AGENTS.md` — operating agreement, **read before touching anything**.
- `docs/dev/TESTING.md` — verification tiers + risk taxonomy. Pick evidence by
  change class before choosing tests.
- `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` — C-sections. Read the one
  for any subsystem you touch; every forbidden path there was expensive.

## Current State

**PR #64** open: https://github.com/olliegilbey/clave/pull/64 — docs only, 10
files, +12051. Commit `289d541`, signed, all pre-commit hooks passed. Base `main`
requires `test` + `wasm-build` green and conversation resolution; **0 approving
reviews required**, `enforce_admins: true` — so direct push to main is blocked
and the maintainer can self-merge once CI is green.

**Issues filed**, each pointing at its spec:

| Issue | Workstream | Depends on |
|---|---|---|
| #55 | S0 frame coherence | — **first, alone** |
| #56 | S1 ordering semantics (closes #39) | S0 |
| #57 | S2 terminal interaction | spike first |
| #58 | S3 tab close | S0, S1 |
| #59 | S4 label + rename | — |
| #60 | S5 colour | — |
| #61 | S6 gutter | — |
| #62 | S7 context battery | deferred; S6 reserves the cell |
| #63 | S8 width 30→38 | — |

**Uncommitted (`git status`):**
- `docs/superpowers/specs/2026-07-22-S6-gutter-glyphs.md` — **+108/−62. This is
  S6's post-commit revision and it must be committed onto the PR branch.** It
  adopts S5's render seam and adds the four costed collapsed options.
- `CLAUDE.md` — **not this session's work.** An `@AGENTS.md` import plus dedup
  from another of the maintainer's sessions. Deliberately left unstaged. Do not
  absorb it into a spec commit.

No source code was modified at any point. `cargo test --workspace`, `cargo clippy
--workspace --all-targets -- -D warnings` and the wasm build were all green at
session start and nothing has touched them since.

## What's Working

**The dossier method is the asset — copy it.** Every root cause was traced to
`file:line` by a subagent and cross-checked before being written down. Several
"obvious" hypotheses were *refuted* by that discipline (see Discoveries). When a
new symptom appears, extend the dossier rather than starting a fresh
investigation; the specs all cite it by section.

**Delegation pattern that worked.** Opus subagents given (a) a precise problem
statement with the file:line evidence already found, (b) an explicit design
direction stated as *mine, argue if you disagree*, and (c) a required output
structure. Every one of them pushed back on at least one direction with a
code-grounded counter-argument, and in each case they were right. Do not give
them open-ended briefs; do not give them briefs with no room to disagree.

**Cross-workstream contracts held under parallel authorship.** Six specs written
concurrently by six agents converged because the dossier fixed the shared facts
and each brief named the seam. Where two collided, the conflict surfaced in their
own reports rather than in the code later. Keep doing this.

**The live-validation format the maintainer asked for.** Numbered steps, each
with: what he does · what to look at · what to report back · a branch table
mapping his report to a conclusion and a next action. Every spec has one, every
one opens with a `clave --version` vs loaded-bar-version pre-flight. This format
is endorsed — reuse it verbatim in the implementation PRs.

**Read-only live diagnosis is solved.** The dossier's table (L470-520) maps
observable → conclusion for every root cause. `clave ls --json | jq`, the raw
store, `zellij action list-panes -t`, and the zellij log are enough to
discriminate all of them without mutating anything.

**Constraint narrowness worth knowing:** the specs are *direction*, not reviewed
implementations. They have not been through the fugu gauntlet. Each workstream's
own PR still owes the full review per `AGENTS.md`.

## Important Discoveries

**RC-A: two reported symptoms are one defect.** `is_active_instance()` and
`own_tab_id()` (`crates/clave-bar/src/main.rs:43-71`) join the last PaneUpdate's
`plugin_panes` against the last TabUpdate's `last_tabs` **by tab position**, and
a tab close renumbers positions. The wrong bar elects itself, binds an agent to
the wrong tab, and `apply_bind` evicts the rightful tenant (`store.rs:239-245`).
Sticky — `sent_binds` (`model.rs:197,305,429,431`) has no `remove`, no `clear`,
no reset. This is why S0 lands alone and why S1/S3 are sequential.

**RC-B: the eager cold-start tab is often never bound.** `fire_binds()` is called
after TabUpdate, PaneUpdate, `clave-status` and `clave-register` — but **not**
after the hydrate snapshot (`main.rs:393-412`), which is the only thing that
populates `self.agents` at session birth. The maintainer's own hypothesis
("which claude instance is loaded into a tab first") was correct.

**`CommandChanged` makes terminal-tab ordering viable with no shell config.**
This overturns the 2026-06-30 decision to park the feature. `zellij-server` is
not vendored **but is fetchable from crates.io** — do this rather than guessing;
it is how the mechanism was established. `CommandChanged` is a 1 Hz sampler
(`background_jobs.rs:114` → `pty.rs:2066-2175`), change-gated on argv, and
`is_foreground` means "the pane's direct child has a child" — so its **`true`
transition** is exactly command-start and never completion. Known gap:
sub-second commands fall inside one poll window. `InputReceived` remains useless
— confirmed unit variant, zero payload (`zellij-utils/src/data.rs:959-960`).

**A fifth close defect nobody owned, and it is deterministic.** `birth_touched`
(`model.rs:383-385`) latches on the tab **id**, is never removed, and zellij
recycles ids (`get_new_tab_id` = `keys().last() + 1` over a `BTreeMap`). Close
the highest-id tab, press `Alt+t`, and the new tab is permanently unstamped →
sort key 0 → below every dormant row. No race required; likely fires more often
than the races.

**Refuted, so nobody re-chases them:** zellij position renumbering does *not*
cause reordering (order-preserving, ascending tiebreak). `apply_prune_tabs` does
*not* write `status` — it is not the "goes idle" source. Two concurrent prunes
are safe under the flock; prune-vs-touch is not.

**`DefaultHasher` was rejected and the hash approach overruled entirely.** It is
not stable across toolchains, so colours would reshuffle on a `rustc` upgrade.
The maintainer then rejected hashing outright (*"hashes could collide"*) in
favour of store-backed iterate-and-wrap allocation. That moved S5 onto the
**cross-process/IPC** taxonomy row — it now owes an ordering/idempotency argument
plus an adversarial reviewer.

**`𖣂` (U+168C2) is Bamum Supplement, not a Nerd Font glyph.** It renders on the
maintainer's machine (probe run, all five candidates rendered) but a working
Nerd Font battery is evidence about the Private Use Area only. `` is
pre-cleared as a width-identical fallback; S6 keeps both behind a plugin config
key. Terminal icon confirmed as **``**.

**Glyph cell widths were measured, not assumed** — against the exact
`unicode-width` version zellij lays its grid with, under two versions,
cross-checked to Unicode 15.1. All 1 cell, with a *declared* dependency on
ambiguous-width-narrow. The existing clamp counts `str::chars()`, which is
Unicode scalars and not terminal cells; `main.rs` is `test = false`, so nothing
would have caught a 2-cell glyph.

**A mechanical `30 → 38` replace silently breaks a test.** `30` appears both as
*the width target* and as an *arbitrary start width*; in
`seek_waits_for_inflight_resizes_and_zellijs_floor` the substitution pushes the
step past `MAX_LEARNABLE_STEP` and changes what the floor loop asserts. Expected
red set is exactly {T2, T9, T10, T13, T16} — anything else is a finding. Also:
both historical regression pins (#4, #27) run at the **collapsed** target, so
neither covers the number being changed.

**Version-cut analysis (preserved; owned elsewhere now).** Cutting v0.1.2 today
reproduces the incident deterministically: `just release` installs
`bin/clave-v0.1.2` and regenerates config at v0.1.2, but nothing repoints the
unversioned `clave` on PATH — it stays v0.1.1, cold-starts, and `needs_version_refresh()`
(`setup.rs:568`) sees its own wasm present and skips refresh, writing a v0.1.1
`launch.kdl`. Two plugin locations → two bars. `clave doctor` reports "no issues"
throughout, because its skew check compares only its own version against
installed artifacts (`doctor.rs:326-352`) and never inspects PATH. Minimal
pre-tag set, ranked: **#43a** release refreshes the unversioned launcher ·
**#44** inject the absolute binary into the plugin via layout config · **#48**
doctor cross-checks PATH ↔ artifacts ↔ all three KDLs · **#43b** rename
`dev-install`'s output to `clave-dev`. All Tier-1 verifiable.

**Process notes.** Direct push to `main` is blocked (`enforce_admins: true`) —
always PR. The pre-commit blocklist rejects private repo and path names in
staged lines; genericize examples (`$HOME/…`, `$TMPDIR/…`, placeholder repo
names) *before* staging, and keep the reason out of commit messages, PR bodies
and issue text.

## Next Steps

1. **Commit S6's revision onto `docs/sidebar-ux-specs`** (+108/−62, uncommitted).
   It is required — S6 as committed still describes the superseded render seam.
2. **Merge PR #64.** Nothing can start until the specs are on `main`; a worktree
   cut from `origin/main` cannot see them.
3. **Rule on collapsed mode** (S6 §2.8, L616-760). Four options costed:
   **(a) accept gutter-only — recommended**, zero cost, already a net gain over
   today's dot-plus-stray-`…`, but repo identity disappears when collapsed;
   **(c) at 7 columns** is the pre-costed upgrade if that matters (+3 cols, a
   real tinted letter not a tinted `…`, lands in S8's file, passes S8's
   invariant); **(b2)** tints gutter cell 3 always (needs a second glyph probe,
   makes colour the only collapsed identity signal); **(b1) and (d) are
   recommended against in any circumstance** and are documented so they are not
   re-proposed. S6 §5 Step 6 lets him decide from real rows.
4. **Fix two stale gutter-width numbers** — prose only, no structural change.
   S6 owns the number and states it authoritatively at **6 columns** (text
   budget 23 @30, 31 @38). `S8 §1` says 26/34 (assumed a 3-cell gutter) and
   `S5 §7` says 33 @38 (assumed 4). Correct both before their worktrees start,
   or each will build to a wrong budget.
5. **Start the parallel worktrees:** S4 #59 · S5 #60 · S6 #61 · S8 #63 ·
   S2 #57 (spike). No shared files.
6. **Then S0 #55 alone**, then S1 #56, then S3 #58.
7. **Unrelated, tracked elsewhere:** the pre-tag release set (#43a/#44/#48/#43b)
   before any v0.1.2. The maintainer took this to another agent.

**Open questions:** collapsed mode (above). Whether S5's host-side `InkSpan` can
mis-point for a live row whose zellij tab name has diverged from the store label
— live rows render the tab name, not the label, and `model.rs:1326-1353` pins
that we do not re-rename on divergence. Flag for S5's implementing agent.

**Where work stopped — verbatim.** Handoff was requested immediately after the
PR went up; the last substantive exchange was:

> "all 5 symbols showed in my terminal output.
> 4 is the terminal icon I wanted.
>
> Yeah, can commit to main, but I think the repo won't allow it. Will probably
> have to PR I guess."

then, in full: **"Yeah, /handoff"**.

**Endorsed, verbatim — the rulings that supersede earlier decisions.** The row
format, which supersedes both his own earlier `repo · title · branch · summary`
and #24's 2026-07-21 comment:

> "Yeah this sounds fine.
> And let's do:
>
> ● 󰁼 𖣂 F-CLA · clave · \<summary\>
>
> The tree icon signifies a worktree. Nerd fonts work in the terminal here it
> seems, I can see the battery icon which will be for the context level, which I
> think there is an issue for too. The first three glyphs will go in the same
> gutter that the dot glyph is in."

On ordering semantics:

> "only typing a prompt to the agent should move it up, only user->claude
> interactions, claude finishing should not move it up. […] This also extends to
> terminal tabs, interaction from the user->terminal should also bump the
> terminal tab to the top of the list. Terminal responses shouldn't affect if
> there's a process that's running and completes"

On colour, after being asked which segments carry it:

> "the title should get its own unique colour to differentiate it from other
> titles within that repo. This makes every tab visually identifiable in a
> heartbeat."

and, overruling the hash:

> "they just need to be from a repeating set that iterates rather than a hash -
> hashes could collide, and the repeating set should be predefined as light
> coloured text of different and distinct colours but look good on a dark
> background - this will likely become something that matches zellij themes […]
> pulling in their colours for text would be nice and cycling through them."

On the working method, which shaped every spec:

> "all these things will need to be separately researched and specced so that I
> can use a different agent session and worktree for each and be able to live
> test for the agent to tell them what I see at each step."

## Context to Preserve

- **User prefs (binding):** extremely concise, signal over noise; explain while
  doing; dense why-comments citing spec §/ledger/issue; conventional commits +
  `Claude-Session:` trailer; **never commit without explicit approval** (he signs
  via 1Password); ask before architecture decisions with multiple valid
  approaches; he drives ALL live zellij input.
- **He responds better to concrete asks than to summaries.** A dense wrap-up
  drew *"no idea what you need from me."* When you need something, state the
  literal command to run and the literal answer shape you need, one or two items
  at a time.
- **`AGENTS.md` never-list, verbatim in force:** never launch or kill a zellij
  session · never run `just release` · never run `cargo install` or
  `just dev-install` while he may be daily-driving · never write versioned
  artifacts under `~/.local/share/clave/` · never write anywhere under
  `~/.claude/` (read-only) · never commit without explicit approval.
- **Issue #44 is unfixed** — the bar shells out to bare `clave` from PATH. It can
  corrupt any live reading. Every live-validation script starts by confirming
  `clave --version` matches the `clave-bar: loaded vX.Y.Z` lines in
  `$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log`. Treat a mismatch as
  invalidating everything downstream.
- **The one sanctioned live mutation** is hot-reloading the sandbox bar in the
  `clave-test` session. Everything else touching a live terminal: print the
  command, never run it.
- **SSH is a hard constraint** — clave must eventually work with the CLI and the
  terminal on a remote host. Reject designs assuming a shared local desktop.
  S2's Branch A (plugin-side) holds under SSH by construction; a shell hook does
  not survive `ssh` *inside* a pane.
- **Review requirement** for every implementation PR: the vendored fugu harness
  plus an independent adversarial reviewer. In a cloud container do NOT opt into
  `cli_reviewers` — a lane that did not run is not a lane that passed; state
  which lanes actually executed.
- **Promise made:** I said I would bring him S6's collapsed options *read and
  summarised*, not forwarded raw. Step 3 above is that debt.

## Restart Hint

Safe to `/clear`. Two uncommitted files: **commit S6's spec revision to the PR
branch first** (it is this session's work and the branch is stale without it);
leave `CLAUDE.md` alone — it belongs to another session. On branch
`docs/sidebar-ux-specs`, no source touched, tests green, PR #64 awaiting merge.
