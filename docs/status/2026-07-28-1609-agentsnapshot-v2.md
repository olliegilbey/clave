# Status — execute the AgentSnapshot v2 plan (#69) as subagent coordinator

_2026-07-28 · worktree `agentsnapshot-v2`, branch `worktree-agentsnapshot-v2` @ `26e3d0a` · clean, gates green_

**You have two roles, in sequence.** First **coordinator**: dispatch a fresh
subagent per task, review between tasks, keep the boundary the plan draws. Then
**fixer**: consolidate what the four agents produce, and run the live interactive
test with Ollie driving.

The spec and the plan are written, reviewed and committed. Do not re-brainstorm;
do not rewrite the plan. Everything below exists so you do not have to re-derive
it.

## Task Overview

Execute `docs/superpowers/plans/2026-07-28-agentsnapshot-v2.md` — four tasks
landing the `AgentSnapshot` v2 wire shape — using
**`superpowers:subagent-driven-development`**, one subagent per task, reviewing
between each.

Success: `just gates` green, `just sandbox` clean, **nothing renders
differently** (this is inert plumbing), and S5 (#60) / S6 (#61) unblocked.

**Ollie chose subagent-driven over inline.** He asked for opus subagents. The
tasks were drawn so a reviewer could reject one while approving its neighbour.

**Do not fan all four out at once.** They are sequential — Task 2 needs Task 1's
struct, Task 3 needs Task 2's field. Task 2's eleven-site edit is the one where a
fresh agent most plausibly misses a site; review it carefully.

## Reference Docs

Read these two in full before dispatching. Everything else is a targeted slice.

- **`docs/superpowers/plans/2026-07-28-agentsnapshot-v2.md`** — the plan. Complete
  code in every step; no placeholders. Its closing **"Deliberately not in this
  plan"** section is the scope guardrail.
- **`docs/superpowers/specs/2026-07-28-agentsnapshot-v2-design.md`** — the spec.
  §2 the wire shape, §3 the spike findings, §4 amendments owed, §5 verification.

Slices only if you need them:

- `docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md` **§2**
  (lines 36–102, the 44-column geometry: title 7 · repo 7 · summary 17 — note
  **no cwd column and no branch column**), **§5.4** (lines 224–232, the glyph
  escape rule — load-bearing), **§7.1** (lines 258–299, the ruling that makes
  #69 a blocker).
- `docs/superpowers/specs/2026-07-22-S4-label-rename-and-live-cwd.md` — **1787
  lines. Do not read whole.** Only §3.2 (lines 256–300) if you need to see the
  false invariant. **S4 is not your work.**
- `AGENTS.md` — short, read whole if unfamiliar with the repo.

## Current State

Working tree **clean**. `git diff --stat HEAD` empty. Two commits on this branch
beyond `main`:

- `af33b82` — the spec
- `26e3d0a` — the plan

**No implementation code has been written.** Not one field added.

GitHub, already done — do not redo:

- **#79 filed** — the extinct summary tier, with evidence.
- **#80 filed** — pinned tabs, as a do-not-design-out constraint on #56.
- **#69 body edited** — a `> [!IMPORTANT]` ruling banner explaining that three
  fields land and two are ruled back.
- **#59 commented** — a `> [!WARNING]` that S4's §3.2 invariant is false.

## What's Working

**Build on this. It is verified and should not be re-litigated.**

- **`just gates` was green at baseline on this branch** — exit 0, **215 tests**
  (the older handoff said 208; 215 is current). Run it before dispatching so any
  red is the subagent's.
- **The plan is complete and self-reviewed.** Every step carries real code.
  Task boundaries are TDD cycles ending in a commit. Trust it; if a subagent
  reports the plan is wrong, verify before believing.
- **`#[serde(default)]` is the established house pattern** — `tab_id` and
  `stale` on `Agent` both carry it with why-comments saying "keeps pre-field
  payloads parseable". The plan copies that voice. Existing round-trip tests
  (`agent_tab_id_roundtrips_and_defaults_none`,
  `agent_stale_roundtrips_and_defaults_false`, `clave-types/src/lib.rs:211-263`)
  are the exact template for the new ones.
- **One producer, one consumer.** `snapshot_from` (`store.rs:167`) →
  `apply_snapshot` (`clave-bar/src/model.rs`). No scattered construction sites
  on the wire side. This is a genuinely easy refactor surface.
- **The bar reads none of the new fields.** Verified: `cwd`, `branch` and
  `worktree` have **zero consumers** in `clave-bar/src/model.rs` and `main.rs`
  outside test fixtures. So Task 1 cannot change rendering — if it appears to,
  something else is wrong.
- **`merge_resume_record` (`add.rs:352`) needs no edit** — it builds with
  `..row.clone()`, so it already preserves new fields as earned state. Verified
  by reading it. It looks like a twelfth site and is not one.
- **The sandbox works** — `just sandbox` on `main` completes with all guards
  passing as of today.

**What this does NOT cover:** there is no automated test that observes a real bar
consuming a real snapshot in a live zellij session — that is Tier 3, blocked on
#47. Confidence comes from round-trip tests plus the single producer/consumer
pair. Do not invent a Tier 3 test.

## Important Discoveries

### The spike — findings that reshaped the design

Measured on Ollie's live tree, 2026-07-28. All reproducible; commands in spec §3.6.

1. **`{"type":"summary"}` is extinct — 0 of 919 transcripts.** `summary_from_tail`
   (`hook.rs:116`) scans for a line type Claude Code no longer writes. All 14
   live store rows read `label_source: first_prompt`, so **every label in Ollie's
   sidebar is a truncated first-prompt fragment.** Filed as #79.
2. **`ai-title` is the replacement** — Claude's rolling auto-description
   (`{"aiTitle":"Get approval to proceed"}`), 373 across 40 newest transcripts.
   **Absent from every spec.** `custom-title` (1057) is the user's rename.
   `last-prompt` (1340), `worktree-state` (384), `relocated` (131) also exist.
3. **Transcripts RELOCATE on cwd change.** A session that entered a worktree had
   its whole `.jsonl` moved to a project dir keyed on the new cwd, carrying
   history; `find` shows nothing at the old path. **S4 §3.2 asserts the
   opposite** and freezes `rec.cwd` to protect the tail read — so the freeze
   causes the silent staleness it exists to prevent. Fix is
   `payload.transcript_path`, which S4 itself deferred as an optimisation.
4. **`worktree-state` carries `worktreePath` / `worktreeName` / `worktreeBranch`**
   — provenance with no git subprocess, for worktree sessions.
5. **`/clear` keeps the same session id** and `custom-title` survives it in the
   transcript. Confirms S4's hold-last-non-empty design; no change owed.

### Approaches tried and abandoned — do not retry

- **Freezing `rec.cwd` as an immutable transcript anchor.** Proposed, approved by
  Ollie, then **falsified by finding 3**. The evidence is the exact inverse of
  the reasoning. Superseded by `payload.transcript_path`, and it is S4's problem
  now, not this PR's.
- **Writing a "live row" spec that owned liveness.** A full draft was written and
  **deleted**. S4 already owns liveness across 1787 lines with a test plan and a
  live-validation SOP. Writing it again was duplicating the exact contention #69
  exists to end. The rescope was Ollie's call and it was correct.
- **Landing `repo_ink` / `title_ink` early.** Rejected: `u8` has no "unset" —
  `0` is a real palette entry (crystalBlue) — so every row paints one colour
  until S5's ledger exists. S5 lands them *with* the ledger.
- **Landing S1's `tab_order` rename.** Rejected: S1 §3.6 states the rename exists
  *to discard* unix-second values that would outrank every ordinal forever. It is
  inseparable from `mint_ord`. S1 lands it.
- **Dropping the backfill** as redundant once S4 makes summaries live. Rejected:
  dormant rows receive no hook events by definition, so they would render a blank
  17-column field indefinitely.

### Traps that cost time this session

- **`ls ~/.claude/projects/*clave*` silently fails.** Entry names begin with `-`,
  which `ls` parses as flags; `ls` is also aliased to **eza**, where `-t` means
  `--time` and errors. Use `/bin/ls --` or `find`. This produced a false "no such
  directory" that nearly closed the investigation early.
- **The pre-commit PII hook does not cover `gh`.** A commit was blocked for a
  personal marker in a pasted store sample — but the same sample had **already
  been published** to public issue #79 via `gh issue create`, which runs no scan.
  It was remediated. **Scan anything containing real store or transcript output
  before it goes to a public surface.** The repo is PUBLIC.
- **`LABEL_SEP` is a deliberate small scope addition** beyond the committed spec.
  S4 §4.1 and S5 §3.1 each propose the constant independently; the backfill needs
  it. It lands once in `clave-types`, written `" \u{00b7} "` per design-lock §5.4.
  Ollie was told and did not object.

## Next Steps

1. **`just gates`** — confirm the baseline is green (215 tests, exit 0) before
   dispatching anything.
2. **Invoke `superpowers:subagent-driven-development`** and run the plan's four
   tasks in order, one opus subagent each, reviewing between.
3. **Open the PR** once Task 4 is green. It is **cross-process/IPC class** under
   `docs/dev/TESTING.md` — it owes an ordering/idempotency argument (one is
   written verbatim at the end of the plan, ready to paste) and an independent
   adversarial reviewer. **State which review lanes actually ran.**
4. **Then, a separate piece of work Ollie has asked for:** reconcile the S-specs
   with the spike findings. Spec §4 is the work list. S4 (#59) is the heaviest
   and already carries a warning comment.

**Between 2 and 3 your role changes — see the next section. Do not skip it.**

## After the subagents return — you become the fixer

Ollie's instruction, verbatim:

> "once all the agents come back, you will become the fixer and coordinator,
> which will be implementing the consolidation to get everything working and then
> run the live interactive test with me driving."

So there are two distinct jobs after dispatch, and **you do both yourself** — do
not hand either to a subagent.

### Job 1 — consolidation

Four subagents each see only their own task. Expect drift and fix it in your own
voice rather than accepting four dialects:

- **A missed construction site in Task 2.** Eleven `AgentRecord` literals need
  two fields each. A miss is a compile error, so it is cheap to catch — but
  `crates/clave/tests/kdl_guardrail.rs:118` sits outside `src/` and is the one an
  agent scanning only `src/` will skip. `cargo test --workspace` catches it;
  bare `cargo test` **does not**.
- **Doc-comment voice drift.** The house style is dense why-comments citing a
  spec section, issue or ledger finding. A subagent will tend to restate what the
  code does. Rewrite those; they are load-bearing documentation in this repo.
- **Test-name drift** from the house pattern
  (`agent_<field>_roundtrips_and_defaults_<value>`).
- **Duplicated or contradictory comments** where Task 1 left placeholder lines in
  `snapshot_from` and Task 2 replaced them. Confirm the placeholder comment went
  with them.

Then: `just gates` as a whole, not per-task. Green is the gate.

### Job 2 — live interactive test, Ollie driving

**There is no Tier 3 harness (#47), so this is manual and he runs it.** Print
commands for him; never run `zellij`, never launch or kill a session, never
`just dev-install`.

The model for how this repo writes a live validation SOP is **S4 §6** (steps 0–7,
lines 1470–1680) — numbered steps, each with an explicit expected reading and a
"if you see X instead, that means Y". Write this one in that shape. Note that
S4's Step 0 caveat (*"issue #44 is unfixed; skip this and every reading below is
suspect"*) is **stale — #44 is fixed** (`fd13c26`, `b6d61b5`) and closed.

What makes this validation unusual: **the change is inert, so the headline
assertion is that nothing changed.** Do not let that make the test vacuous — there
are two real observables:

1. **The store still loads and every agent is still there.** This is the
   `#[serde(default)]` proof. If any field lacked it, the first run against his
   existing `agents.json` shows **zero agents**. That is the failure mode worth
   actually looking for.
2. **The backfill populated `summary` on his 14 rows.** Before the run they are
   all empty; after a session create they should hold the words segment lifted
   out of each label. This is the only user-visible-ish change and the only part
   with a wrong-answer risk (a bad split).

Sequence it **sandbox first, real store second**, and say so explicitly:

- `just sandbox` uses a `CLAVE_STATE_DIR` override, so the backfill runs against
  a throwaway store. Verify the split there, where a mistake costs nothing.
- Only then discuss touching `~/.local/state/clave/agents.json`. The backfill is
  **non-destructive by construction** — it writes only where `summary` is empty
  and never overwrites — but it is his live fleet index, so it is his call and he
  should see the reasoning before agreeing, not after.

Expect to iterate with him. He is fast at spotting a wrong reading and will tell
you plainly; treat a surprising result as a real finding and grep `FOOTGUNS.md`
before debugging, per `AGENTS.md`.

**Where work stopped, verbatim:**

> "so, we should carry through for you as the subagent coordinator, only the
> context you need to run the subagents. So, please write a /handoff for yourself
> to come back to entirely fresh, zero context, and preserve what will be best
> for you to know what needs doing, the nuance, the explorations and where to
> look for things, what to re-read, and any footguns or nuances that need
> remembering. Put it all in the handoff, and you'll pick up from that document
> in a moment after we refresh."

**What he endorsed, verbatim — this is the framing to think in:**

> "Oh so we're just building a nicer datastructure that we can pass around, that
> sounds like a much better design, also for extensibility down the line. So this
> way we can have a nice deep module with a simple interface, a lot of
> functionality. That's good engineering."

and

> "So yes. this all makes sense - we need to build the plumbing under the hood
> that more than one feature relies on, so that they are built from a shared
> place - rather than having each agent working on a feature building its own
> implementation."

and, authorising the issue work:

> "yes to both issues, then commit and write the plan. all the previous issues
> were written by you and me, but in other sessions, some of which were using a
> less frontier claude model, so amending and improving issues for clarity and as
> we learn is good if you think things need updating."

## Context to Preserve

- **He signs every commit** — 1Password prompts him. Signing is configured
  (`commit.gpgsign=true`, ssh format) and worked this session.
- **Never commit without showing him first.** Prepare; he approves.
- **Never run a bare `zellij` command, never kill or launch a session.** He
  dog-foods clave and this agent runs inside his live session. `just sandbox`,
  never `just dev-install`.
- **The repo is PUBLIC.** Anything another human reads under his GitHub identity
  needs his okay, or must open with "Ollie's Agent Speaking:". Bot replies on his
  own PRs are fine unsupervised. **Scan for personal markers before publishing
  store or transcript output** — the pre-commit hook does not cover `gh`.
- **Be extremely concise. Signal over noise. Explain while doing.**
- **Dense why-comments** citing spec section, issue or ledger finding — never
  restating what the code does.
- **`cargo test --workspace`, always.** Bare `cargo test` silently skips 68 tests
  and exits 0.
- **CodeRabbit reports `pass` while rate-limited** — read the check *detail*, not
  the colour (#68). Happened repeatedly on 2026-07-28.
- **Back-compat is relaxed pre-v1**, verbatim: *"if there is a worry about
  breaking older clave sessions, don't worry about that till we have got to a
  full v1."* But `#[serde(default)]` stays — it is not legacy compat, it is what
  stops the **first run** against the existing `agents.json` seeing zero agents,
  and what lets the **dev sandbox hot-reload the bar** without a hard failure.
- **Pinning is coming** (#80): *"we will later add a pinning functionality to pin
  tabs to the top so they don't move or change order relative to each other.
  Something to be aware of, but a later feature we aren't building yet."* Do not
  design it out; build nothing.
- **`/recap`** may become a summary tier above `ai-title` later. The tier table
  should stay extensible. Not now.

## Restart Hint

Clean tree, gates green, two commits on `worktree-agentsnapshot-v2` @ `26e3d0a`.
Safe to start immediately: `just gates` to confirm the baseline, then invoke
`superpowers:subagent-driven-development` on the plan. No stash, no WIP, nothing
half-applied.
