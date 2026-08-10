# Status — the `deliberate` skill, four runs, and the A/B eval still to run

## Task Overview

Ollie asked for a **two-model deliberation workflow**: spin up two agents on
different models, let them explore a codebase blind and without prescription,
then have them debate turn-by-turn until they reach shared understanding on a
design, feature, or bug — looping him in only when a disagreement is genuinely
his to decide.

It was built as a **skill**, not a Workflow. It has been run **four times**. The
remaining task is an **A/B evaluation of the original prompts versus the current
ones**, on a single issue used identically for both arms.

Success criteria for the eval: isolate what the prompt changes actually bought,
given that four runs so far have confounded prompt changes with issue choice and
with tool-permission differences.

## Reference Docs

- `~/.claude/skills/deliberate/SKILL.md` — the skill itself, 472 lines. **This is
  the NEW prompt arm.** Real path `~/dotfiles/src/.claude/skills/deliberate/`;
  **untracked in the dotfiles repo — Ollie has not committed it.**
- `~/deliberate-evals/` — all run artefacts, copied out of ephemeral session
  scratchpad on 2026-07-31. `deliberate-79/` (run 1), `deliberate-39/` (run 2),
  `deliberate-79b/` (run 4, the #79 rerun). Read `OUTPUT.md` in `-39` and `-79b`
  first; run 1 has no `OUTPUT.md` because that requirement did not exist yet.
- GitHub issue #79, comment `5119428973` — run 1's findings, posted publicly.
  **Superseded by run 4 and not yet corrected.** See Next Steps.

## Current State

**The clave repo is clean.** Nothing in this work modified it. `git status` empty,
no uncommitted changes. The only repo artefact is the #79 issue comment and this
status file.

**The skill exists and works**, at `~/.claude/skills/deliberate/SKILL.md`.
Phases 1–6: blind openings → bootstrap → peer-to-peer debate → audit → steelman
swap → deliver.

**Four completed runs**, all on clave issues:

| Run | Issue | Notes |
|---|---|---|
| 1 | #79 | Original prompts. Found `--name` suppression (22/22 vs 0/9), left it BLOCKED pending a live test |
| 2 | #39 | First run of revised prompts. Openings converged; the swap did most of the work |
| 3 | #79 | **Aborted by me.** Agents had started on a newer tree contaminated by run 1's own conclusions |
| 4 | #79 | Rerun, pinned to `48d21aa` (pre-run-1 state). The strongest run |

**A leftover worktree exists** at
`…/scratchpad/deliberate-79b/tree`, registered in the clave repo. Remove with
`git worktree remove <path>` — not done, it is Ollie's call.

## What's Working

**The architecture is settled and correct. Do not redesign it.**

- **Skill, not Workflow.** Workflow scripts only expose stateless one-shot
  `agent()`; there is no `SendMessage` inside a script, so persistent debaters
  are impossible there. A skill instructs the *main session*, which is
  necessarily the coordinator.
- **The coordinator must be the main session.** agentIds exist only in the
  context that spawned them, and `main` is the only well-known address. This is
  forced by the messaging topology, not a preference.
- **Peer-to-peer messaging works via raw agentId.** Names do **not** resolve
  between subagents. The receiving agent sees the sender as its *type*
  (`general-purpose`), not its ID — so a bootstrap handshake is mandatory: spawn
  both, collect both IDs, tell each the other's before the debate opens. Without
  it the debate deadlocks on turn one, silently.
- **The blackboard file works.** `DELIBERATION.md` with SPEC / OPEN CRUXES /
  DEBATE LOG, plus `OUTPUT.md` as the deliverable. Serial turn-taking prevents
  clobbering for free.
- **The steelman swap is the highest-yield component.** It has found real defects
  in **every** run it has been used on, including runs where every audit signal
  said the debate was hard-won. Run it by default.
- **Audit leads work.** Feeding the coordinator's Phase-4 findings into the swap
  as *leads, not conclusions* produced the best result of run 4.

**The single best lead, use it every time:** *"what did you both assume, from
shared priors, that neither of you checked because neither of you disagreed?"*
Two models with overlapping training reading one repo are not independent
witnesses. The protocol only applies pressure where they differ — everything
they share passes untested by construction. This lead produced run 2's best
finding (a `#[serde(default)]` counter that would have silently inverted an
ordering) and run 4's (a live relocated transcript).

## Important Discoveries

**Things that cost time and should not be rediscovered:**

- **Agents die on `529`.** `SendMessage` resumes from transcript with nothing
  lost — tell the resumed agent what moved while it was gone.
- **A worktree does not pin the agents, only the files.** They can `git log`
  forward into later commits. This mattered *only* for the A/B and Ollie
  explicitly ruled it out of the skill as an edge case. **Re-apply it for the
  eval, in the spawn prompt, not the skill.**
- **A denied tool call degrades an agent silently.** In run 3/4's first pass,
  agent A could not read `~/.claude/projects`, fell back to quoting the issue's
  numbers, and carried on. Its headline finding vanished. Restoring access and
  sending it back to the blind phase recovered it. **Check both openings for
  this before seeding cruxes.**
- **Run 1's output contaminated the repo.** Its findings became ledger decisions
  D23/D24 dated the same day, and #86 implemented part of it. Any rerun of #79
  against current `main` is not a clean test — pin to `48d21aa`, where
  `docs/ux/LEDGER.md` does not yet exist.

**Approaches tried and rejected — do not re-propose:**

- **A third debater.** Coalition dynamics; three models agreeing is *cheaper* to
  fake than two. A third *role* (the swap, the audit) is what was needed.
- **A referee agent.** The Phase-4 audit plus the swap already do this, and the
  swap does it better.
- **Structured JSON schemas per turn.** Markdown carried 13 cruxes across two
  rounds with no ambiguity.
- **Triage before spawning, and a lever for convergent openings.** Both proposed
  by me, both rejected by Ollie — see Context to Preserve.

## Next Steps

**1. Design and run the A/B eval.** This is the live task. See the prompt
archive below — the OLD prompts exist nowhere else.

Design constraints that follow from what went wrong before:

- **One issue, both arms, same pinned worktree.** Pick an issue *not* previously
  deliberated (#79 and #39 are both burned — their conclusions are in the repo
  and in issue comments).
- **Equal tool access, verified before the arms start.** This confounded run 3
  entirely. Check both openings mention first-hand measurement.
- **The coordinator is a confound.** I had read every prior run when seeding run
  4's cruxes, and leaked a run-1 insight into a nudge (telling A to measure
  "per-session coverage, not raw line counts"). For a clean eval the seeding must
  come only from the openings in front of it. Consider having the two arms
  coordinated in *separate sessions* so neither coordinator has seen the other's.
- **Decide what you are measuring** before running. Candidates: number of
  findings not in the issue; whether the deliverable is publishable without
  editing; whether facts got escalated (a defect); whether the swap found
  anything; how many closures rest on newly-discovered evidence versus argument.

**2. Correct the #79 issue comment.** Run 1's comment says the fix is gated on a
sandbox test Ollie must run. Run 4 settled that causality from existing data and
corrected the mechanism to *titling at birth*, not `--name` specifically. Two
agent analyses that disagree about whether he has homework are now sitting on the
issue. Post run 4's `OUTPUT.md` and explicitly supersede.

**3. Flag the upgrade hazard.** Not recorded anywhere in the repo. Verified
directly: `~/.local/state/clave/agents.json` has 18 rows, `title` and `summary`
**absent on all 18** — the deployed binary predates #81 and strips them on every
hook event. Without a lazy backfill running *before* the earn-gate, the first
hook event after upgrade overwrites 14 rows' labels permanently.

**4. Housekeeping.** Skill untracked in dotfiles; worktree still registered.

---

### PROMPT ARCHIVE — the OLD arm, verbatim

These exist only in the session transcript. Reproduce them exactly for the A
arm.

**OLD opening brief** (sent to each agent at spawn, filenames differing):

> You are one of two agents on different models who will deliberate to a shared
> answer. Right now you are working **blind** — the other agent exists, is
> exploring the same question independently, and you cannot see their work.
> That is deliberate: your independent read is the thing you contribute.
>
> **The question:** `<topic — run 1 used a PARAPHRASE of the issue, asserting its
> premise as fact, plus "(Read the issue itself with gh issue view N.)">`
>
> **Repository:** `<repo path — run 1 used the live repo, NOT a worktree>`
>
> Explore however you see fit. You are given no starting file, no suggested
> method, and no map on purpose — how you choose to get your bearings is part
> of what makes your read different from the other agent's.
>
> Write your opening position to `<rundir>/opening-{a|b}.md`:
>
> - **Position** — what you think the answer is, concretely enough to act on.
> - **Evidence** — every load-bearing claim cited `file:line`. A claim you
>   cannot cite is a guess, and you must label it one.
> - **Uncertainties** — what you could not determine, and what would settle it.
> - **Cruxes** — the specific points where you expect a competent engineer
>   might reasonably disagree with you.
>
> Be genuinely willing to be wrong later, but do not hedge now. A vague opening
> gives the other agent nothing to bite on and wastes the round.
>
> Do not modify any repository files. Write only your opening file.

**OLD debate brief** — identical to the current one in `SKILL.md` EXCEPT it
lacked: the batching clause, the per-side `A1/B1` numbering clause, the
decision-tree framing of OPEN CRUXES, the "shared understanding" definition (it
said *"Your goal is a shared answer you both actually believe"*), the
`OUTPUT.md` requirement in the convergence section, the "escalate only if the
spec is blocked" qualifier, and the "carry both positions" escalation rule (it
instead said *"and your own recommendation"*, which Ollie later identified as
structurally biased). The **Blocked facts** section did not exist and was added
mid-run-1 by hand.

**Also differing in the OLD arm, and worth reproducing:** the `Agent` tool
`description` field named the models (`"deliberation side A opus"`), openings
were spawned in two separate messages, and Phase 5 was *offered* to the user
with a recommendation table rather than run by default.

---

### Verbatim — where work stopped

> **Ollie:** okay, can you think of a decent way of a-b testing the old prompt
> versus the new, on a problem or issue that can be used the same for both runs.
> Before you do though, we should write a /handoff document on all this so that
> you can pick things up with a clear context on what you need to know to perform
> this, and what the old prompts were, and how to best approach this eval.

### Verbatim — endorsements, for voice calibration

> **Ollie:** Yeah, this is excellent.

> **Ollie:** the skill shouldn't say anything about that, we need the agents
> blind for the most part, and to deliberate naturally, but to just get to a
> consistently usable output. We shouldn't need them to triage either, if they
> are being asked to deliberate, that's because we want them to deliberate, not
> exit early, so again, not worth mentioning. It's better to mention too little
> than too much for this skill.

> **Ollie:** worrying about cost, tokens, clock, not an issue for this. you are
> overengineering if you are thinking about that.

## Context to Preserve

**Ollie's directives on this skill — these overrode my proposals repeatedly:**

- **Say less.** "It's better to mention too little than too much for this skill."
  I twice added guidance he removed. When in doubt, cut.
- **Keep the agents blind.** No triage step, no telling them what is already
  settled, no lever when their openings converge. "If they both agree, they will
  deliberate anyway and figure out whether they agree on all aspects or not."
- **Do not encode this experiment's constraints into the general skill.** The
  `git log` ban and the tool-parity check are eval scaffolding, not skill
  content. He rejected both on those grounds.
- **Cost, tokens and wall-clock are not concerns.** Raising them is
  overengineering. Runs cost ~200–280k tokens per agent; he does not care.
- **Do not name the models to the agents.** Neutral `description` fields, no
  model named in any prompt. A model that believes it is arguing with a stronger
  one has a reason to defer that has nothing to do with evidence.

**Repo constraints (from AGENTS.md, security-relevant, preserve verbatim):**

> **Never kill or launch a zellij session.** Ollie dog-foods clave daily — the
> Claude you are is running *inside* a live clave session, so `zellij
> kill-session` takes down his working fleet, and a bare `zellij` command targets
> his session, not a sandbox. Print the command; let him run it. Same for `just
> release` and anything writing `~/.local/share/clave/`.

Agents in every run were forbidden from launching `claude` processes for the
same reason. This is what created the BLOCKED FACT category.

**Outbound comms:** GitHub comments written as him need attribution. The #79
comment opens with a blockquote beginning *"Ollie's Agent Speaking."* — match
that pattern.

**A promise made:** I offered to post run 4's `OUTPUT.md` to #79 and to clean up
the worktree. He has not answered. Ask before doing either.

## Restart Hint

Clean tree, nothing staged, safe to `/clear`. The skill is untracked in
`~/dotfiles` — if he wants it kept, that needs a commit there, not here.
