# Status — pre-fleet readiness audit (six blockers found; specs are sound)

_2026-07-25 · branch `docs/sidebar-ux-specs` @ `22ce531` · PR #64 open ·
main @ `50fa26a` · another agent is ALSO working in this worktree_

Predecessor: @docs/status/2026-07-22-2209-sidebar-ux-specs.md — how the nine
specs were produced and what each covers. Read it for the workstream map.

## Task Overview

Assess whether the project is ready to hand S0–S8 to a fleet of implementing
agents, one per worktree. Four read-only audits were run: PR reconciliation,
citation accuracy, cross-spec contract drift, process/testing fit + hygiene.

**Verdict: not ready. Six blockers, all fixable, several one-line.**

## Current State

Committed and pushed this session (`22ce531`, signed): dossier duplicate-S2-row
removal, dossier palette 10→12, S5 budget 33→31, S8 superseded test name, and
the **rescue** of `2026-07-21-multi-provider-claude-codex-design.md` — a 23k
approved design that existed only as an untracked file in a stale codex
worktree. PR #65 is a narrower slice of it (launch *variant*, not the
provider-adapter refactor), so it is **not** superseded.

**NOT mine, left untouched in the working tree** — another agent is locking the
sidebar visual design in this same worktree: `2026-07-25-sidebar-visual-design-lock.md`,
`bar-preview.py`, `UBIQUITOUS_LANGUAGE.md`, and a pre-existing dirty `CLAUDE.md`.
Do not stage these without checking with that agent.

## What's Working — build on this

- **The specs are factually sound.** 192/193 sampled citations correct (766
  total, zero out-of-bounds), and every `§` cross-reference resolves. Problems
  are drift, not sloppiness. Do not re-verify the corpus; spot-check only.
- `cargo test --workspace` 201 pass · clippy clean · wasm builds · fmt clean.
- Findings are filed where the work happens, not buried in this doc:
  **#68** (review lane degraded, `lint` not required, approvals 0), **#69**
  (AgentSnapshot v2), a blocker comment on **#61**, a status change on **#47**,
  and the cross-cutting spec repair list as a comment on **PR #64**.

## Important Discoveries

**The six blockers**, in clearing order:

1. **Specs aren't on `main`** (PR #64 open) — worktrees cut from `main` won't
   contain them. Merge order: **#67 → #64 → #66**.
2. **`build-wasm-setup.yml` fails on every push** — a cargo-dist *fragment* in
   the workflows dir that GitHub executes. #67 fixes it. Nine agents would each
   learn that red CI is normal here.
3. **CodeRabbit reports `pass` while rate-limited** — verified on #64 and #66
   *today*, at ~1 PR/day. Nine concurrent PRs = nine green checks that reviewed
   nothing. Draft PRs report `Review skipped` the same way. See #68.
4. **S6's `glyphs` key reproduces the v0.1.1 double-sidebar bug.** zellij hashes
   plugin identity over the whole config map (`layout.rs:528-529`); a miss
   *starts a new plugin*. Decision owed — see Next Steps.
5. **The one sanctioned live mutation now doubles the sidebar.** Every spec's
   `zellij action start-or-reload-plugin` misses post-#66 without
   `-c clave_binary=clave`.
6. **Nobody owns the `AgentSnapshot` wire format** — five specs each add a field
   to the same struct. See #69.

**Two hard collisions** that would have surfaced as a mid-implementation merge
fight, not a clean conflict:

- **`clear_tab_timeline`**: S1 renames it and adds a backfill; S5 says it is
  "NOT modified" and pins that with a test. Same function, same lines.
- **`fit_label_str` (S4) vs `clamp_name` (S5)**: same job, same call site,
  *different* budget-zero behaviour. S4 claims S5 complies; S5 declines the
  hand-off and never mentions S4's function.

**Tier 2 (#47) changed status both ways.** #66 removes its stated blocker
(#44), so it is buildable — *and* it became a blocker for S1/S3/S2-impl,
because the compensating control (a prose ordering argument in a PR body) does
not compose: four workstreams rewrite `apply_tabs`, so by the time S3 merges,
S0's argument describes code that no longer exists and nothing records it.

**~79% of `clave-bar/main.rs` stays untested after S5/S6.** Their claim that the
residue is "one `println!`" is true of `render()` (39 of 564 lines) and false of
the file — the untested remainder is exactly the effect-dispatch and event-adapter
half where S0/S2/S3 do all their work.

**Line-number staleness is coming.** `clave-bar/main.rs` citations shift by a
known band map once #66 lands (`load` 342→356, `render` 525→559). The
`AgentRecord` literal-site lists are already stale via #65. **Convert both to
symbol anchors** rather than remapping — remapped numbers rot on the next merge.

## Next Steps

**Three decisions are owed before the repair pass can finish** (asked, not yet
answered):

1. **S6's `glyphs` key** — bake into all emitters (S6 becomes a
   generated-artifacts workstream, forcing S8 to land first), or drop the config
   channel for v1 and fold into #40? *Recommended: the second — cheaper, S6
   already names it as the deferral, keeps S6 off the generator files.*
2. **Land AgentSnapshot v2 standalone first?** *Recommended: yes (#69).*
3. **Build a thin #47** (two scenarios) after S0 lands? *Recommended: yes.*

Then the repair pass on the specs, per the checklist in the PR #64 comment:
#44 pre-flight rewrite, hot-reload `-c` flag, `just gates`, `just sandbox`, the
config-regeneration rule, and the two collisions.

**Housekeeping, safe to do now** (the rescued spec is committed, so the worktree
is no longer load-bearing):

```
git worktree remove /Users/olliegilbey/.codex/worktrees/a04a/clave
git branch -D codex/multi-provider-design
git push origin --delete worktree-doctor-install    # PR #29, merged
```

Also: no `docs/README.md` explains plan-vs-spec-vs-spike-vs-status, and root
`spikes/` (archived prototype scaffolding) collides in name with
`docs/superpowers/spikes/` (the validation ledger). `justfile:30` says 33 model
tests; it is 63.

**Where work stopped — verbatim:**

> "so, the other agent is good, we are locking in the sidebar ux design. But I'd
> like us to wrap up here, get our findings to where they need to be, and clean
> things up - what do you reckon?"

**Endorsed earlier, and it shaped the whole audit:**

> "I'd like you to run a check over the full project to assess where it could be
> improved or where it could go wrong, and how it will fit the processes and
> testing we have in place. Make the project beautiful to get ready for
> implementation."

## Context to Preserve

- **User prefs (binding):** extremely concise, signal over noise; explain while
  doing; dense why-comments citing spec §/ledger/issue; conventional commits +
  `Claude-Session:` trailer; **never commit without explicit approval** (he signs
  via 1Password); ask before architecture decisions with multiple valid
  approaches; he drives ALL live zellij input.
- **Never resolve a review thread without a comment saying how it was
  addressed**, and always fix what CodeRabbit returns — his standing correction.
  But note #68: a green CodeRabbit may have reviewed nothing.
- **AGENTS.md never-list** stands: no launching/killing zellij sessions, no
  `just release`, no `cargo install` / `just dev-install` while he may be daily
  driving, no writes under `~/.claude/` or the versioned artifact dir.
- **A second agent is live in this worktree.** Stage by explicit path, never
  `git add -A`.

## Restart Hint

Safe to `/clear`. `22ce531` pushed and signed; only the other agent's files are
dirty. Start from the three decisions above, then the PR #64 comment's repair
checklist. Do not re-audit the citation corpus — it is verified.
