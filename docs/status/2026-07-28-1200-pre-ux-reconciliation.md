# Status — everything reconciled onto main; the UX fleet is unblocked

_2026-07-28 · `main` @ `e241f7e` · five PRs merged this session · one draft PR
left open deliberately_

Predecessor, read only if you need its slice:
@docs/status/2026-07-26-1128-sidebar-design-lock.md — how the sidebar visual
design was locked, and the eight workstream specs it obligates. Still the
authority on what S0–S8 must build.

## Task Overview

Two requests. **Fix the review findings on the release runbook**, which grew
into a documentation and review pass across the whole repo. Then **get
everything open merged onto `main`** so the S0–S8 UX agents can worktree off a
clean base.

Both are done. `main` is green, the sandbox works, and the only open PR is a
draft that is deliberately parked.

## Current State

**Merged this session, in order:** `c9dd874` (#71, release runbook — 18 review
threads over three rounds) → `3132932` (#73, FOOTGUNS.md + CONTRIBUTING rewrite
+ leaner AGENTS.md) → `b6d61b5` (#74, sandbox self-check fix) → `ceaa452` (#54,
**the first external contribution**) → `e241f7e` (#70, the release owns an
unversioned launcher; `dev-install` → `clave-dev`).

**Verified on the merged result:** `just gates` exits 0 (208 tests), `just
sandbox` completes with all three stable-surface guards passing, and the
maintainer ran a live interactive test of `main` in the sandbox and found it
behaves as his daily driver.

**PR #65 (`clave add --codex`) is left open as a draft, deliberately.** Codex
integration is deprioritised — Claude-first until the UX is good. It is 2,718
lines and **will conflict with the UX work**, since S0–S8 rewrite the same
model and rendering paths. Decide before starting S-work whether to rebase it
periodically or close it and re-cut later; letting it sit untouched through
eight workstreams is the expensive option.

## What Changed, and Why It Matters for S0–S8

**FOOTGUNS.md is new and is the highest-leverage thing here.** 106 entries,
seven sections, trigger phrase first so the words you would grep for lead the
line. AGENTS.md now says to grep it *before* debugging.

Read its three tags before trusting an entry: untagged = bites you today,
**[FIXED]** = guarded, do not reintroduce, **[DESIGN]** = locked but unbuilt, so
the trap is coding against the old assumption. The `[DESIGN]` entries and the
whole *"The bar's model — frame joins, latches, ordering"* section are aimed
directly at S0–S8.

**Everything in it was verified against source, not merely collected.** Roughly
a quarter of the harvested material did not survive as written. That matters
because it sets the standard: **docs in this repo go stale, and a confidently
worded claim is not evidence.** Concretely, one verifier compiled a scratch
crate against the real `zellij-utils` 0.44.3 parser and executed it, which
overturned two rules that had been repeated across multiple documents.

**AGENTS.md is now an index, not a contract.** It carries one standing
prohibition — never kill or launch a zellij session, because the maintainer
dog-foods clave and the agent is running inside his live fleet — plus the rule
for where knowledge goes: trap → FOOTGUNS.md · term → UBIQUITOUS_LANGUAGE.md ·
dead end → the C-section · how to *use* clave → README.md · how to *work on*
clave → CONTRIBUTING.md.

**CONTRIBUTING.md was rewritten for outsiders.** It had described the pre-#66
PATH leak as live and told readers to treat a hard rule as pending "until #44
lands" — which landed in `fd13c26`. It now leads with a working quick start and
recommends `just sandbox` over `just dev-install`, which is a safety property
rather than a preference.

## Important Discoveries

**The first external PR sat six days because CI never ran on it.** GitHub holds
workflow runs from first-time fork contributors in `action_required` until a
maintainer approves them. The PR *looked* green — only CodeRabbit and
GitGuardian had reported. **Check `gh run list --status action_required` when a
fork PR looks oddly quiet.** Approving the runs was one API call; nobody knew to
make it.

**`lint` is not a required status check.** Branch protection requires only
`["test","wasm-build"]`, so a red `cargo fmt`/`clippy` does not block a merge —
which is exactly how the contributor's PR reached us with a rustfmt diff nobody
was told about. `required_approving_review_count` is also **0**. Both are #68.

**Two independent review lanes converged on the same P1 in #70** — `run_setup`
installed the launcher *before* `write_generated`, while `run_release` installs
it last and states the invariant one function away. A generation failure would
have left `bin/clave` on the new version with `config.kdl` describing the old
one: #43 reproduced by the fix for #43. Convergence from two lanes that did not
see each other is the strongest signal this session produced.

**A trap class bit twice in one day, in two files.** `launch.kdl` is written
only by a cold start, so any check that asserts on it *before* a launch fails
closed on a healthy tree. It was in the release runbook's version glob, and then
in `sandbox-setup.sh`'s identity check — where a six-day-old `launch.kdl` made
`just sandbox` refuse to set up a perfectly good sandbox, **breaking the
documented contributor onboarding path on `main`**. Now a FOOTGUNS entry.

**The `\u{...}` glyph-escape rule is a hard rule that nothing follows.** Zero
glyphs in `crates/` use the escape form; 24 rendered sites are literals. The
highest-stakes one is `·` U+00B7, the label separator baked into every composed
session label and therefore every zellij tab name. Recorded in FOOTGUNS.md as
unapplied. **S5/S6 will touch most of those sites anyway** — converting them in
the same pass is nearly free, and doing it separately later is not.

**Empty output means opposite things in the two runbook checks.**
`clave_unversioned` silent is the good outcome; `clave_versions` silent is a
failure. Identical on screen. That asymmetry is now called out explicitly,
because it is the kind of thing a tired human reads wrong at tag time.

## Next Steps

**The UX fleet is unblocked. Before starting S-work:**

1. **#69 (AgentSnapshot v2) is a hard blocker for S5 and S6.** `Agent` carries
   only the composed `label` — no `title`, no `summary` — and the locked design
   needs them as structural wire fields. Land it first or those two stall.
2. **Read FOOTGUNS.md's model section before touching `clave-bar/src/model.rs`.**
   S0, S1, S3 and the design lock's §7.1 ruling are all writers to
   `BarModel::rows` — a known collision zone with four claimants.
3. **Decide #65's fate** (rebase-through or close-and-recut). See above.
4. **The collapsed geometry is still not ratified** — a promise made and unkept.
   Every candidate was rendered against the old 6-column gutter, now 9. Binding
   constraint: at 44 the collapsed target must be **< 24**. Also unresolved:
   whether collapsed truncates the whole label or renders field 0 only
   (field-0-only rendered better).

**Filed this session, not blocking:** #75 (collapse state not inherited at tab
birth — observed live, low priority, likely falls out of S0), #76 (clean-install
container test), #77 (cross-process binary skew — a design call, options
written up).

**Owed but not urgent:** #48 gained scope — `clave doctor` has no launcher check
at all, and its release-skew advice is now factually wrong post-#43a.

## Context to Preserve

- **The maintainer signs every commit**; 1Password prompts him. If it times out
  he is away — stop and say so rather than substituting a signature.
- **He dog-foods clave daily and the agent runs inside his live session.** Never
  run a bare `zellij` command; print it.
- **He decides from rendered artifacts, not prose.** Build the mockup. Offer
  costed options with a recommendation, and expect to be overruled.
- **`just sandbox`, never `just dev-install`,** unless you specifically want the
  daily binary replaced. Post-#70 `dev-install` produces `clave-dev`.
- **Review rounds find real things.** Rounds 2 and 3 on #71 each found a defect
  introduced by the previous round's fix — one of them mine. Do not treat a
  second round as friction.
- **CodeRabbit reports `pass` while rate-limited.** Read the check *detail*, not
  the colour (#68). It happened repeatedly today.
- **`gh pr update-branch` diverges your local copy** — fetch and merge before
  pushing. Hit live this session, on an entry written hours earlier.

## Restart Hint

`main` @ `e241f7e`, clean, gates green, sandbox verified. Nothing is
mid-refactor and no worktree holds uncommitted work except the parked
`claude-codex-profile`.

Read, in this order: this file → `AGENTS.md` → `FOOTGUNS.md` (skim the headings,
grep it later) → the design lock at
`docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`. Run `python3
docs/superpowers/specs/bar-preview.py` once to see what was ratified.

**Do not assume any sidebar UX exists in code. None of it does.**
