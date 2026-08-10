# Status — v0.1.3 milestone is set; YOUR TASK IS THE JOINT PRIORITISATION, THEN BUILD

_2026-07-31 19:5x · `main` at `a201a93`, clean, no open PRs. The open-issue audit
is DONE and its findings are already on the issues themselves._

## Task Overview

The audit the previous session ran is finished. **Your job is the joint
prioritisation conversation with Ollie, then starting the v0.1.3 work.** He chose
the release scope already — do not re-litigate it — but the *sequencing inside
it* is an open conversation, and there is one real dependency risk to put in
front of him early (see Important Discoveries).

**v0.1.3 is milestoned with eight issues**, all fleet legibility:

| # | What |
|---|---|
| #62 | S7 context battery per row — transcript-estimated window fraction |
| #105 | S7 refinement: expanded view shows a token count, not a glyph |
| #112 | Dormant rows crowd the nav ring; separate them |
| #100 | Dwell auto-opens a dormant row; prefer explicit confirm |
| #92 | Unread green clears on a fly-by; should require a dwell |
| #57 | S2 terminal tabs rise on user interaction |
| #110 | Floating panes: clave sizes none, keybind surface part-owned |
| #114 | Show the zellij bar again, plug clave into its tooltips (**filed this session**) |

**Do not start implementing before the prioritisation conversation.** Ollie's
standing preference is to agree the forward path first.

## Reference Docs

- **`AGENTS.md`** — read first. Index to everything below.
- **`FOOTGUNS.md`** — grep before debugging. **Five entries were corrected this
  session** and are now trustworthy: `:80` (dev-install swap), `:122-124`
  (transcript signals + cadence), `:142` (store-strip / re-derivation), `:149`
  (glyph rule). Anything you remember about those from an older handoff is stale.
- **`docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`** — the
  lock that wins wherever an S-issue disagrees with it. Needed for #62/#105
  (battery cell geometry) and #112.
- **`docs/ux/LEDGER.md`**, D37 section — the width-seek gate and the deferred
  `seq == 0` rewrite. Only needed if you touch #89.
- **`docs/status/2026-07-31-1812-post-v012-roadmap.md`** — the previous handoff.
  Its task (the audit) is DONE. Read only for v0.1.2 release mechanics.

## Current State

`main` at `a201a93`, clean, four gates green, **no open PRs**.

**Merged this session:** #113 — the FOOTGUNS correction (docs + two `hook.rs` doc
comments; no executable line changed).

**39 issues audited** by eight Sonnet subagents, every one read against `main`:
2 closed (#61 S6 gutter, #79 summary retarget), 19 commented with evidence, 20
left standing because their threads were already current. **The audit findings
live on the issues** — do not re-run it, and read the issue thread before
re-deriving anything about a subsystem.

**Untracked in `docs/status/`: four status files from OTHER sessions.** Leave
them. **`git add -A` is banned in this tree** — it has swept other sessions'
files three times. Stage explicit paths.

## What's Working

**Build on this. Verified this session, not assumed.**

- **The audit is a real asset, not just triage.** Nineteen issues now carry
  concrete file:line evidence about what shipped and what did not. Before
  designing anything in a subsystem, read its issue thread — the answer is
  usually already there with a commit sha attached.
- **`gh issue comment -F <file>` works; loops and `$(cat …)` do not.** The
  permission classifier blocks `for n in …; do gh issue comment …; done` and
  blocks `gh issue close -c "$(cat f)"`. Individual calls with `-F` pass, and
  several can go in one message in parallel. Close in two steps: `gh issue
  comment -F file` then `gh issue close N --reason completed`.
- **Writing comment bodies to scratchpad files first is the right shape** — it
  survives the quoting hazards, lets you PII-scan the whole batch with one grep
  before anything is public, and makes the posting step mechanical.
- **The four gates are fast** (seconds warm) and the secret-scanning pre-commit
  hook has never false-positived. `cargo test --workspace` is 290 tests.
- **Review bots earned their cost again on #113** — CodeRabbit caught a real
  cohort-mixing error, Codex caught two P2s, one of which was a genuine
  self-contradiction I had introduced. **Verify their claims against code, then
  reply and resolve.** All three were valid this time; that is not the default.
- **Scripted measurement over `~/.claude/projects` is cheap and settles
  arguments.** Three of this session's conclusions came from counting files, not
  reasoning. Write the script to scratchpad — inline shell hits quoting hell with
  JSON patterns.

## Important Discoveries

**RAISE THIS EARLY: #57 has an unmilestoned dependency.** S2 terminal tabs is
spike-first and depends on S1 ordering (#56, §3.5/§4.1), which is **not in
v0.1.3 and is entirely unstarted**. #112's nav-ring separation likely leans the
same way. Neither was pulled into the milestone because that is Ollie's call, not
a scope decision to make silently. **Put this in front of him in the first
exchange** — the options are (a) pull #56 in, (b) descope #57 to the cut after,
or (c) find a separation approach for #112 that does not need total ordering.

**The S0 → S1 → S3 chain (#55, #56, #58) is entirely unstarted and gates a lot.**
Verified by `git log -S` on the defining symbols: `frames_coherent`, `own_pane`,
`commit_ord`, `Store::mint_ord`, `observed_dead` have never appeared outside the
doc-adding commits. #58's C1b is **deterministic, not racy** — close the
highest-id tab, create one, and it strands at the bottom every time.

**Transcript field reality, re-measured over all 770 local transcripts.** This
corrects two rounds of previous belief and is now in FOOTGUNS `:122-124`, `:142`:

- `type:"summary"` — **0 of 770**. Still extinct.
- `custom-title` is **not** a rename signal. It is written from a session's
  **first line**, re-stamped every ~3-5 user turns, single-valued, in sessions
  never renamed. Measured directly on this session's own transcript: 15 lines
  across 75 user turns, first at line 1, never renamed.
- `ai-title` behaves the same way where present, but **presence has collapsed** —
  68 of 770; 7 of 132 substantive sessions in the preceding 7 days.
- The two are **near-mutually-exclusive**: of 390 transcripts with ≥20 user
  turns, 297 carry neither; of the 93 that carry either, 39 only `ai-title`, 48
  only `custom-title`, 6 both. **The discriminator is still unknown — that is
  #111**, and this measurement is its best available input.
- **Do NOT conclude "most rows have no summary."** `refresh_row_fields` falls
  through to a first-prompt seed, so absence demotes the summary to prompt-derived
  rather than emptying it. `title` has no such tier — the chip is what goes blank.

**Approaches already refuted — do not retry:**

1. **Closing #60 as shipped.** A subagent called it resolved; it is not.
   `ProvisionalInks` persists nothing, keys title ink on the **title string**
   where the issue requires the agent uuid ("Claude renames constantly"), has 8
   palette entries against a specified 12, and lacks the `repo_index + 1` cursor
   offset. It renders correctly and reassigns colour on rename. Commented, left
   open.
2. **Claiming a stripped `title` re-derives by guarantee.** Re-stamping is paced
   by turns; the tail window is 64 KiB of **bytes**. Measured safe today (worst
   case 34,808 of 65,536 across 148 transcripts) but not structurally bounded.
   D25's eviction caveat stands.
3. **Backfilling `live_session` from the newest transcript in a project dir** —
   the heuristic #99 exists to avoid.
4. **Prompting live agents to warm a new store field** — measured, does not work.
   The cold restart IS the migration.
5. **Pushing straight to `main`** — `enforce_admins: true`. Everything is a PR.
6. **Deleting dormant store rows to reduce clutter** — a dormant row is a
   resurrectable conversation. The fix is navigational (#112).

**Still-true findings worth acting on outside the milestone**, all now on their
issues: #68's two protections are still unset (`lint` advisory,
`required_approving_review_count: 0` — a settings change, minutes); `doctor.rs`
still emits advice that is wrong post-#43a; #35's musl/`openssl-sys` failure
means **no Linux artifact exists**, so #76's container check has nothing to run;
#24 item 5 (model badge) is the only epic sub-item with no tracking issue.

## Next Steps

1. **Open the prioritisation conversation.** Lead with the #57/#56 dependency
   above — it is the one thing that can reshape the milestone.
2. **Agree sequencing inside v0.1.3.** #92/#100/#112 are one dwell-commit model
   and should be designed together, not as three constants landing separately —
   both #100's and #112's threads already say so.
3. **Then build**, brainstorming before implementation per Ollie's process.
4. Optional cheap wins if he wants them bundled: #68's protections, `doctor.rs`'s
   wrong message.

**Where work stopped — Ollie's instruction, verbatim:**

> yeah, run it, can ignore 65 still.

then, after the audit and the FOOTGUNS PR:

> coderabbit has comments, and codex. Let's get those resolved, and get things
> merged, then we'll handoff again to line us up for the prioritisation flow -
> and tag the issues with what we'd like to see in the next release.

All three parts are done: reviews resolved, #113 merged, issues tagged.

**Scope he chose for v0.1.3, from the AskUserQuestion — his roadmap list**,
which he had stated verbatim in the previous handoff:

> We also need to do an issue audit with a set of subagents … and then we need to
> do a separate session of deciding what goes in next. The context measure and
> battery will be good to have, and the dormant tab separation system - a way of
> keeping dormant tabs somewhat separate to live tabs when navigating. And then
> better terminal handling for terminal tabs. We also need to show the zellij bar
> again, for its tooltips, and plug clave into those tooltips neatly.

**Framing he endorsed, in his voice:**

> Momentum beats perfection, things that can wait for other releases can be gh
> issues if they are minor.

and on the v0.1.2 cut:

> I'd argue that the cut was way more for the ui improvements, I can actually see
> what's going on now. … this release is actually driveable by someone in the
> outside world.

And on how he wants findings handled, from this session's instruction:

> Check for yourself on the things you think are strange, and resolve them as you
> see fit against the findings you can make and what the subagents said.

That licence to verify rather than relay is what caught the #60 mis-call and the
self-contradiction Codex flagged. **Use it — do not take subagent or bot claims
at face value.**

## Context to Preserve

- **Never kill, launch, or run a bare `zellij` command**; never `just release`,
  `just dev-install`, or `dev launch`; never write `~/.local/share/clave/` or
  anything under `~/.claude/` (reading is fine). **`just sandbox` IS yours.**
  Print the command; let Ollie run it.
- **Ollie signs every commit** — `git commit` pauses on a 1Password prompt. Wait.
  Never `--no-gpg-sign`. Prefer `git merge` over `git rebase`.
- **`cargo test --workspace`, always** — bare `cargo test` skips tests, exits 0.
- **GLYPH RULE:** every non-ASCII glyph in Rust source and test literals is a
  `\u{...}` escape. The bar is now compliant; the host side is not (see
  `FOOTGUNS.md:149`).
- **The repo is PUBLIC.** No home paths, transcript content, or personal data in
  code, commits, issues or the README. A pre-commit PII blocklist enforces it.
  **PII-scan any batch of drafted comments before posting:** `grep -rl "/Users/"`.
- **Fix review findings and reply before resolving. Never silent-resolve.**
  CodeRabbit reports `pass` while rate-limited (#68) — read the detail, and note
  it reported exactly that on #113 while having posted a real finding.
- **`git add -A` is banned in this working tree.** Stage explicit paths.
- **Ollie is an excellent live tester and volunteers for it.** Give exact steps,
  one inflection point at a time.
- **Handoffs and the validation ledger are IMMUTABLE history** — correct live
  SOP, never a dated record of what was run.
- **#89 carries his own note** *"I think this is actually fixed, TODO, double
  check"*. Nothing in that path has changed since; it is a live-test call only he
  can make.

## Restart Hint

`main` clean at `a201a93`, gates green, nothing uncommitted but other sessions'
status files. **Safe to start fresh.** Open with the #57/#56 dependency question,
not with code.
