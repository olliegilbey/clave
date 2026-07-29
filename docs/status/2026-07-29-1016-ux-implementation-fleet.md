# Status — you are the standing coordinator for the UX implementation fleet

_2026-07-29 · worktree `agentsnapshot-v2` · `main` @ `b00edd3`, branch `spec-reconciliation` @ `f1ad519` (PR #82) · gates green, 222 tests_

**Your role changed.** You are not executing a plan. You are the **standing
coordinator and principal engineer for the entire UX workstream** — S5 (#60),
S6 (#61), S8 (#63), governed by the design lock. Subagents implement
sequentially; you review each, absorb what they discover, and brief the next.
You hold the truth. This role persists across the whole workstream, not one PR.

Ollie's words, verbatim:

> "You could become the coordinating agent for all UX implementation, rather
> than us spinning things up in separate contexts with different implementation
> agents. We would still have sub‑agent‑driven development of the full UX, but
> you would act as the coordinating agent and principal engineer for the entire
> fleet. This would allow you to review each sub‑agent's implementation
> sequentially and, as issues are discovered, inject additional suggested
> changes or implement your own changes as we progress."

## Task Overview

Implement the sidebar UX. Reach a working prototype fast, look at it, then
decide: keep, refactor, or re-engineer from the ground up.

**Success:** a real 44-column row rendering from the store, iterated on screen
rather than argued in prose. `main` stays releasable throughout.

### The operating model — this is the point, do not lose it

The previous four sessions went in circles: implement → discover a blocker →
amend the specs → the amendment reveals the next contradiction → repeat. Ollie
called it, and he was right. **The loop's engine was never the discoveries; it
was that every discovery had to be written into a document before work could
continue.**

The rule that kills it, which he endorsed:

> **Specs are an OUTPUT, not an input.** Nothing gets amended during the build.
> Discoveries land in the coordinator ledger. When the UX is real, specs get
> written *from what exists* — or deleted.

Concretely:

- **Subagents MAY read the existing specs.** Ollie's refinement: *"You could
  still tell the sub‑agents to read the specs as they currently exist, with the
  caveat that you will prompt them with overrides to the specs. So, where I'm
  cautious is in having to re‑edit the full specs as we go along."* So: let them
  read, and carry your overrides in the brief. Do **not** re-edit specs.
- **You keep a running ledger.** One small living file — decisions made, what is
  true now, what is next. Not a design doc; a decision log. Your context is not
  durable; the ledger is what lets a compacted or fresh session continue.
  Suggested home: `.superpowers/ux/ledger.md` (git-ignored scratch) or
  `docs/ux/LEDGER.md` if it should be reviewable. The SDD ledger at
  `.superpowers/sdd/progress.md` from this session is the proven shape.
- **The design lock governs anything visual.** Conflict with an S-spec resolves
  to the lock, silently, no amendment round.
- **Cadence:** a long-lived `ux` integration branch. Churn lives there; `main`
  receives only milestones that could be released. This is what protects the
  project invariant *main is always releasable*.
- **Gate 1 is early and explicit:** first full row on screen → look → keep /
  refactor / re-engineer. A checkpoint, not a vibe.

## Reference Docs

- **`docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`** — the
  governing document for anything visual. **§2 (lines 36–75)** is the verified
  44-column table; read it first and treat it as the target.
  `44 = 1 cap + 8 gutter + 7 title + 1 + 7 repo + 1 + 17 summary + 1 margin + 1 cap`.
  Also **§5.4** (glyph escape rule, load-bearing) and **§7.1** (renders from the
  STORE, not the tab name).
- `FOOTGUNS.md` — **grep it the moment something behaves unexpectedly**, before
  debugging. It is now accurate; three entries were corrected this session.
- S4/S5/S6 specs — **frozen after #82 merges.** Historical rationale. Subagents
  may read; you override. Do not amend.
- @docs/status/2026-07-28-1609-agentsnapshot-v2.md — the prior session's handoff,
  for #69 background only. Its live-test plan is superseded (see Discoveries).

## Current State

**Working tree:** both status files (this one and the prior session's) are
committed to #82. Tree should be clean.

**You are on branch `spec-reconciliation`**, which is #82. Once it merges, move
to a fresh `ux` branch off `main` — do not build the UX on this one.

| PR | Branch | State |
|---|---|---|
| **#81** | `worktree-agentsnapshot-v2` | ✅ **MERGED** as `b00edd3` on `main`. |
| **#82** | `spec-reconciliation` @ `f1ad519` | Rebased onto `main`, `MERGEABLE`, 3 commits. **14 review findings** (3 Codex + 11 CodeRabbit) all fixed, answered and resolved — **0 open threads.** CI green. **Awaiting Ollie's merge.** |

**You cannot merge — the merge command is blocked by the permission classifier.
Ask Ollie.**

**Known-good fallback commit: `b00edd3`** (`main`, post-#81). Gates green,
222 tests, sandbox-validated live. Ollie decided against a release cut, and the
reasoning is sound: his daily driver runs the already-installed
`~/.local/share/clave/bin/clave` from the *previous* cut, which nothing here
touches. The SHA is for *source* rollback; the *fleet* is protected by the
stable/sandbox split, not by the SHA.

**If #82 is still open when you resume:** it is finished work. Check CI, ask
Ollie to merge, do not reopen its content. If it has merged, the specs are
frozen from that moment.

**What #81 landed** (inert plumbing, nothing renders differently):
`Agent` gains `title: Option<String>`, `summary: String`, `worktree: Option<String>`;
`AgentRecord` gains `title`/`summary`; `snapshot_from` projects all three;
`LABEL_SEP` in `clave-types`; one-shot `backfill_summaries` riding
`clear_tab_timeline`. All `#[serde(default)]`.

## What's Working

**Build on this. It is verified, not assumed.**

- **`just gates` is green: exit 0, 222 tests, no warnings.** That is your
  baseline; any red is yours.
- **The fields S5 and S6 need now EXIST on the wire.** `title`, `summary`,
  `worktree` are projected by `snapshot_from` and reach the bar. Verified live:
  `clave ls --json` shows them. **S5 and S6 are genuinely unblocked** — do not
  re-derive this.
- **`model.rs` (3217 lines) is host-testable and has a large existing suite.**
  It is `zellij-tile`-free by design. This is where testable logic belongs and
  it is a good safety net.
- **The sandbox works and is safe for you to run.** `just sandbox` is read-only
  with respect to zellij sessions and **prints** the launch command rather than
  running it ("Session lifecycle is the human's: print, never run"). You may run
  `just sandbox` yourself. You may **not** run `dev launch`.
- **The store survived a real upgrade.** Ollie's live store parsed under the new
  binary: **15 agents, not zero.** The `#[serde(default)]` discipline is proven
  against real data.
- **The live validation loop is known-good.** Sandbox + hand-seed + Ollie
  launching + reading back the store worked end to end this session. Reuse it.
- **Subagent-driven development with per-task review works well here.** Four
  tasks, four reviews, one fix round; the reviews caught real defects (see
  Discoveries). The prompts that worked are recoverable from this session's
  pattern: brief file + report file + explicit global constraints + "report
  BLOCKED rather than guess".

**What this does NOT cover:** there is no automated test that observes a *rendered*
row. That is the gap task 1 exists to close. Do not assume any visual behaviour
is pinned — today, none of it is.

## Important Discoveries

### The root cause of the circling — verified, and it is the basis of task 1

**The entire visual surface of the product is the one part of the codebase with
no test access.**

- `crates/clave-bar/Cargo.toml` sets **`test = false`** on the `[[bin]]`, for a
  legitimate reason: `main.rs` calls zellij-tile shims that resolve to the wasm
  host import `host_run_plugin_command`, which has **no symbol on the host
  target**. The bin links for `wasm32-wasip1` alone.
- `fn render` lives at **`crates/clave-bar/src/main.rs:559`**, building the row
  by string concatenation (`main.rs:574-591`), with no column arithmetic and no
  width measurement.
- Therefore every visual decision had to be litigated in **prose** — because
  prose was the only available medium. **6,632 lines of S-spec exist to
  compensate for ~40 untestable lines.** Prose cannot be verified, so it drifts,
  and drift costs an amendment round.

**Task 1: extract the row renderer out of `main.rs` into the lib** as a pure
`fn render_row(&Row, cols) -> String` (or similar). `main.rs` keeps only the
zellij plumbing. Then a golden test **is** the spec: write the 44-column row you
want as a literal, make it true. This converts contrast, adjacency, truncation
and column widths from arguments into assertions.

**Consequence for #47** (Tier-2 harness): it shrinks from critical path to
integration nice-to-have. Most UX verification needs no zellij once the renderer
is pure. #47 looks like the bottleneck only because the renderer's untestability
is hiding inside it.

### Cross-spec defects — the category only a coordinator catches

Codex reviewed #82 and found three real defects **in the specs themselves**. Two
are worth understanding, because they are evidence for why the coordinator model
is the right one:

- **A circular hand-off.** S6 said "tier 2 is S4's to write"; S4 said "S4 stores
  none of them; **S6** decides where it lands." Each assigned the write to the
  other, so **nothing** filled `AgentRecord.worktree` from the transcript and
  `snapshot_from` kept sending `None` — silently, forever. Neither spec is wrong
  in isolation. **Only reading both at once finds it.** That is precisely what
  no per-workstream subagent can do and what you, holding the whole picture, can.
- **Mutually exclusive contracts.** S4 §3.5 told S5 to split the rendered tab
  name on ` · `; S5 (reconciled) lays fixed columns from `Agent.title`/`.summary`.
  A subagent reading S4 would have resurrected the deleted span mechanism and
  bypassed the fields #69 just landed. S4's text was older and more detailed, so
  it would probably have won.

**Generalise this:** when two specs describe the same seam, the failure is not
usually that one is wrong — it is that both are locally reasonable and jointly
incoherent. Brief subagents on **seams**, and review across them yourself.

**Automated review found 14 defects in a documentation-only PR.** Never skip
review because a diff is "just docs" — a spec defect becomes an implementation
defect one dispatch later. Two patterns from that round worth carrying:

- **The dominant defect class was "describes unlanded work as delivered."** S4
  read as if `payload.transcript_path`, the rolling `ai-title` machine, and
  `title`/`summary` writes were shipped; none exist. Fixed **structurally** — S4
  now opens with a banner naming the three gaps — rather than sentence by
  sentence. When several findings share a root cause, fix the root.
- **Specs carry security surface.** `transcript_path` arrives on hook **stdin**,
  and `read_tail` writes what it parses into the store — so an unvalidated path
  is a *write primitive*, not a bad read, and it does not fail closed. S4 now
  mandates canonicalize-and-confine to Claude's projects root plus the expected
  `<uuid>.jsonl`. **When S4 is implemented, this is not optional.**
- The `u8`-has-no-unset trap had leaked into S5's own prose (`unwrap_or(0)`
  described as "untinted"; `0` is crystalBlue). Left alone it would have
  produced a **green test pinning a false expectation** — the worst outcome.

### Known-stale spec content — catalogued, deliberately NOT fixed

Each reconciliation agent was told to **report** contradictions rather than
silently fix them. This is that list. **Do not fix these by amendment** — they
are precisely the treadmill. They are recorded so you recognise them instead of
rediscovering them, and so an implementer who trips on one knows it is known.
If one blocks a task, override it **in the subagent's brief**.

- **S6 §2.10/§2.10.1's `cols - 7` text budget** is design-lock territory and
  unreconciled in the body; §6 still tells S4 and S8 to adopt it. *Highest value
  of these if you touch text budgets.*
- **S6's `glyphs` plugin-config key and the two-tier `GlyphSet`** (§2.6.5,
  §3.1(b), §3.7, four §4.1 tests) — a `glyphs` config key would reproduce the
  v0.1.1 double-sidebar (FOOTGUNS: zellij hashes plugin identity over the whole
  config map). Glyphs are compiled in.
- **S6's terminal mark `\u{f489}`** vs the lock's nf-md-console.
- **S6 cell 3 is two-state ("worktree marker"); the lock says three-state
  provenance.** That is a `Row` design change, not an amendment.
- **S4 §7's "No open format question remains"** — partly addressed on #82, but
  the give-way-vs-fixed-columns question (decision 1 below) is exactly an open
  format question.
- **All `file.rs:line` citations across S4 are pre-#69** and have drifted.
  Trust the code, never a line number in a spec.
- **`bar-preview.py:59` names `#1F1F28` "sumiInk1"**; S5 and the lock say
  "sumiInk3".

**There is no fuller detail to go and find — the list above IS the extraction.**
The three reconciliation agents wrote reports; their amendments landed in the
specs (merged, #82), their unfixed-contradiction lists are the bullets above,
and their design questions are the decisions below. What remained was
pre-merge line-number citations that were already drifting. The reports were
deleted with their worktree rather than kept "just in case" — keeping process
artifacts whose content is already extracted is the same instinct that grew
6,632 lines of spec. **Do not go looking for them.**

### Open design decisions — surfaced, deliberately NOT decided

1. **Two rendering models.** S4 §3.4 specifies give-way truncation over a joined
   string; design-lock §2 specifies fixed-width columns. S4 §7 still claims "no
   open format question remains". **My read: this dissolves once a row is on
   screen — it is a 30-second look, not a spec debate.** I raised it as a blocker
   and then concluded it is not one. Do not re-litigate it in prose.
2. **`oniViolet` measures 4.67 contrast** against S5's own stated ≥5.0 band. A
   ratified hue, so it was recorded rather than substituted. Accessibility call
   for Ollie.
3. **`spawn_mode` orphans relocated sessions** — it derives the transcript path
   from the frozen `rec.cwd`, so a relocated session resumes as a **new** one and
   silently loses its history. **Not yet filed as an issue.** Worth filing.
4. **Issue #63 still says "30 → 38 columns"** while the design lock says 44. The
   *spec* carries a superseded banner; the *issue* — what an implementer actually
   opens — does not. **Not yet updated.** Ollie has standing authorisation to
   amend issues: *"amending and improving issues for clarity and as we learn is
   good if you think things need updating."*

Also unfixed and reported by the spec agents: S6's `cols - 7` text budget
(§2.10) is stale and §6 still tells S4 and S8 to adopt it; S6's `glyphs` config
key and `GlyphSet` two-tier system; `bar-preview.py:59` names `#1F1F28`
"sumiInk1" where the lock says "sumiInk3".

### Errors and traps hit this session

- **A subagent ran `git checkout <file>` to undo a mutation check and reverted
  the whole file**, losing an hour of work (recovered). **Tell every subagent:
  never `git checkout`, `git stash`, or anything mutating HEAD/index.** The
  controller commits.
- **`just sandbox` from the wrong checkout tests the wrong code.** Ollie ran it
  from `~/code/clave` on `main` and it built `main`, not the branch. It uses
  `git rev-parse --show-toplevel`. **Run it from the worktree.**
- **`just sandbox` runs `dev reset` first, which wipes the sandbox store** — so
  any hand-seeded row must be seeded *after* the setup, not before.
- **The sandbox CANNOT exercise the backfill unaided** — every scenario label is
  two-segment (`{name}-{slug} · seeded`) and the backfill needs three. The prior
  handoff's plan to "verify the split in the sandbox" was **vacuous**; a
  three-segment label had to be hand-seeded. Generalise: **check that a sandbox
  scenario can actually produce the state you intend to observe.**
- **CodeRabbit reports `pass` while rate-limited** (#68) — it did exactly this
  again on #81 after the final push. **Read the check detail, not the colour.**
- **A FOOTGUNS entry named the wrong reproduction.** It claimed
  `ls ~/.claude/projects/*clave*` fails; measured, it **exits 0** — tilde
  expansion yields `/Users/…`, no leading dash. The trap is real only for a
  *relative* glob from inside the directory. Corrected. **Lesson: a trap index
  that teaches a false test is worse than no entry**, because the reader runs it,
  sees success, and concludes there is no trap.

### Approaches tried and rejected — do not retry

- **Amending specs to resolve each discovery.** This is the loop. Ollie:
  *"we're continuously moving the ground underneath ourselves, then having to
  update the specs and find something new, move the ground, update the specs."*
- **A store schema/version fence** for the mixed-version write window.
  CodeRabbit asked for one (#81, Major). Declined with reasoning, thread
  resolved: the fields hold no earned state yet (the backfill re-derives them
  from `label`), #69 §2.1 rules out version markers, and the correct mechanism is
  **re-derivation keyed on `label_source`** — a pre-existing field old binaries
  preserve. **S4 owns it.** Recorded in `FOOTGUNS.md`.
- **Landing `repo_ink` / `title_ink` before the ledger.** `u8` has no "unset" —
  `0` is a real palette entry — so every row paints one colour. They land *with*
  the ledger.

## Next Steps

1. **#81 is merged.** If **#82** is still open, check its CI and ask Ollie to
   merge it — you cannot (permission classifier). Its content is finished; do not
   reopen it.
2. **Open the ledger** (see operating model above) and record: the known-good SHA
   `41a6af9`, the four open decisions, and the freeze rule.
3. **Create the `ux` integration branch** off `main` once #81/#82 land.
4. **Task 1 — extract the renderer.** Concretely: move the row-building logic
   out of `crates/clave-bar/src/main.rs` (`fn render` at `:559`, string
   concatenation at `:574-591`) into the host-testable lib as a pure
   `fn render_row(&Row, cols) -> String`. `main.rs` keeps only the zellij
   plumbing that genuinely cannot link on the host. **Do not change any visual
   output in this task** — it is a pure extraction, and that is what makes it
   safe and reviewable. Then add one golden test asserting a 44-column row.

   **This is the unlock: verify it pays off within one task, not one
   workstream.** If it does not, say so and drop it — the operating-model change
   is what breaks the loop, not this refactor.
5. **Then build the row end-to-end** with what exists (`title`, `summary`,
   `worktree` are live) and placeholders where S5/S6 have not landed. **Gate 1:
   look at it with Ollie. Decide keep / refactor / re-engineer.**
6. File the `spawn_mode` bug (decision 3) and update issue #63's width (decision 4).

**Where work stopped, verbatim:**

> "Okay, I think we're on the same page. You could still tell the sub‑agents to
> read the specs as they currently exist, with the caveat that you will prompt
> them with overrides to the specs. So, where I'm cautious is in having to
> re‑edit the full specs as we go along.
>
> I like your statement that the specs are the output rather than the input; we
> will have to be careful about that. Having a running context document will be
> helpful for the main coordinating principal agent, which is you. We should also
> do a handoff now so that you have a clean context to begin this entire
> process... We would need to merge things to `main`.
>
> If there are any reviews still needed or CodeRabbit answers, we should pick
> those apart first. Since there is no functional difference in the way Clave
> works with everything that's been added since the previous release cut, I don't
> think cutting a release now is helpful."

**The framing he set, verbatim — think in this voice:**

> "Momentum beats perfection, while we should have a clean and beautiful code
> base. We need to find a new approach that lets us follow exceptional
> engineering principles while unblocking us and getting us moving."

> "We could get to early prototyping, decide whether we've landed in a good
> place, and then either refactor or completely re‑engineer the design from the
> ground up."

## Context to Preserve

- **Never kill or launch a zellij session. Never run a bare `zellij` command.
  Never `just dev-install`.** Ollie dog-foods clave daily and you run *inside*
  his live session. `just sandbox` is fine and you may run it; `dev launch` is
  his. Print commands, let him run them.
- **UX work NEVER runs against the real store** (`~/.local/state/clave/agents.json`).
  Sandbox only. The one thing that can break his live fleet is the store: a dev
  binary writing new fields, then his stable binary stripping them on its next
  write. The bar itself is harmless by comparison.
- **He signs every commit** (1Password prompts him). Expect a pause on `git
  commit`; never `--no-gpg-sign`.
- **Never commit without showing him first.** Prepare; he approves. Subagent
  commits on a feature branch were accepted this session *after being flagged in
  advance* — flag, don't assume.
- **The repo is PUBLIC.** Scan anything containing real store or transcript
  output before it reaches a public surface — **the pre-commit PII hook does not
  cover `gh`.** Anything another human reads under his identity needs his okay,
  or must open with "Ollie's Agent Speaking:". Bot replies on his own PRs are
  fine unsupervised.
- **Always fix CodeRabbit findings and reply before resolving. Never
  silent-resolve.** Expect multiple rounds.
- **`cargo test --workspace`, always.** Bare `cargo test` silently skips 68 tests
  and exits 0.
- **Preserve cargo's exit status** — `cargo … | grep` lets grep supply the status,
  so a failure reads green. Use `set -o pipefail`, or check `$?` on an unpiped run.
- **GLYPH RULE (design-lock §5.4, load-bearing):** every non-ASCII glyph in
  **source** is a `\u{...}` escape, never a literal. Markdown prose is exempt.
  Literal glyphs were lost in transit twice; the failure mode is tofu in
  production from a clean-looking diff.
- **Dense why-comments** citing spec section, issue or ledger finding — never
  restating what the code does.
- **Be extremely concise. Signal over noise. Explain while doing.**
- **Pinning is coming (#80)** — do not design it out, build nothing for it yet.

## Restart Hint

Gates green, tree clean but for one untracked prior status file. Safe to
`/clear`. **First action: ask Ollie to merge #81 then #82** — you cannot merge
yourself. Then open the ledger and dispatch task 1 (renderer extraction). Do not
amend a spec.
