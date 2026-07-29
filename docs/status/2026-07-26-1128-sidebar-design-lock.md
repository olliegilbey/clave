# Status — sidebar visual design locked; three PRs merged; release runway prepared

_2026-07-26 · repo github.com/olliegilbey/clave · `main` @ `ea0f009` ·
tag `v0.1.1` · PRs #70 and #71 open_

Predecessors, read only if you need their slice:
@docs/status/2026-07-25-1500-pre-fleet-audit.md — the six pre-fleet blockers,
the two hard spec collisions, and the three decisions owed. Still live.
@docs/status/2026-07-22-2209-sidebar-ux-specs.md — how the nine specs were
produced and what each covers.

## Task Overview

Two requests, in sequence.

**1. Lock the sidebar's visual design.** The eight workstream specs (S0–S8)
described geometry that had never been *looked at*. Ratify it from rendered
rows instead: widths, gutter, colour, glyphs, the selected row. Success = a
locked design a fresh agent can build to without re-litigating it.

**2. Get the repo ready to move toward the next release.** Merge what's ready,
close the hermetic blockers, and line everything up for the interactive live
test — which the maintainer must drive personally and which gates every cut.

## Reference Docs

- `docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md` — **the
  authority.** §2 geometry table (measured, not asserted) · §3 the one open
  number (collapsed) · §4 colour · §5 glyphs + §5.3 the `glyphs` config-key
  ruling + §5.4 the escape rule · §6 selected row · §7.1 the live-row ruling ·
  §8 two corrected findings · §9 what it obligates.
- `docs/superpowers/specs/bar-preview.py` — run it: `python3
  docs/superpowers/specs/bar-preview.py`. Illustration, self-checking; the
  prose doc wins where they disagree.
- `UBIQUITOUS_LANGUAGE.md` — shared vocabulary. §1 the three-way "session"
  ambiguity · §3.3 title vs label (the trap).
- `docs/dev/RELEASE-RUNBOOK.md` — on branch `docs/release-runbook` (**PR #71,
  open, 12 unresolved findings**). Part C is the maintainer's live test. Do not
  trust it until item 1 of Next Steps is done — six of the findings are real
  defects in it.
- `AGENTS.md`, `CONTRIBUTING.md`, `docs/dev/TESTING.md` — unchanged, still binding.

## Current State

**Merged to `main`, in this order, CI green at each:** `a7c2b7b` (#67, the
cargo-dist fragment that failed every push) → `643fff4` (#64, the spec corpus +
the design lock) → `fd13c26` (#66, the #44 fix — the bar now calls the binary
it belongs to) → `ea0f009` (#72, `@AGENTS.md` imported into CLAUDE.md).

**PR #71 open** — `docs/release-runbook`: the runbook + this handoff, one
commit. **Blocked on 12 CodeRabbit findings** (triaged in a section below).

**PR #70 open** — https://github.com/olliegilbey/clave/pull/70 —
`fix/release-owns-the-launcher`, labelled `needs-live-validation`. Implements
#43a (the release installs an unversioned launcher at `<data>/bin/clave`) and
#43b (`just dev-install` now produces `clave-dev`). `lint`/`test`/`wasm-build`/
`plan` green. **Not merge-ready:** only the CodeRabbit CLI lane ran.

**Repo hygiene done:** local `main` was five commits stale and is now synced;
`CLAUDE.md` resolved via #72; five merged worktrees and four merged local
branches removed. **Three merged REMOTE branches still to prune:**
`docs/sidebar-ux-specs`, `fix/dist-build-setup-path`, `fix/plugin-binary-path`.
Remaining worktrees are only the two with live PRs (#70, #65).

**A trap #72 avoided, worth knowing:** the working-tree `CLAUDE.md` predated #66
and would have DELETED the `just gates` rule that #66 added. Always diff a
long-dirty file against `origin/main`, not against your local branch tip.

**No source code was written this session, and none of the sidebar UX is
implemented.** The design lock and the glossary are documents. PR #70's code was
written by a delegated agent in its own worktree. S0–S8 are all unstarted.

## What's Working

**Render it, look at it, then decide.** Eight rounds of mockups, each narrowing
one question, judged from real rows in the maintainer's terminal. Prose
comparison of layouts was consistently misleading — including to me. Three of
his rulings overturned my recommendation and he was right every time. **If a
visual question comes up, build the mockup.** The mockup harness pattern is
worth copying: hardcode a fleet, render candidates side by side at real widths,
print a column ruler, and verify programmatically that every row is exactly N
cells before showing it.

**The design is locked and self-verifying.** `bar-preview.py` asserts every row
is exactly 44 display cells, measured in cells (East-Asian W/F as 2, combining
as 0) rather than code points. A miscounted glyph fails the preview loudly.

**Discovery is wired.** `AGENTS.md` opens its reading order with
`UBIQUITOUS_LANGUAGE.md`; the dossier, S5, S6 and S8 each carry a supersession
banner naming exactly what is dead in them, with an explicit precedence rule
(banner wins over the retained body). Every relative link was validated.

**What this does NOT cover:** the spec *bodies* are untouched. The banners
redirect; they do not rewrite. §9 lists what each spec still owes.

**Delegation that worked**, again: a precise problem statement with the
evidence already found, a design direction stated as *mine, argue if you
disagree*, and a required output structure. The delegated agent diverged from
my direction three times and was right each time (see Discoveries).

## Important Discoveries

**Two earlier findings were wrong. Both are recorded in the lock doc §8 so
nobody re-derives them.**

1. **`COLLAPSED_TARGET_COLS = 4` is a width the bar never has.** Its own
   doc-comment says zellij's resize floor may stop the seek above it and
   "wherever cols stop changing is accepted". It rests at **11** on the
   maintainer's window. S6 §2.8 costed four collapsed-mode options against a
   "text budget 0" row that does not occur. That analysis is void.
2. **The "this terminal only renders Plane-15 glyphs" rule was an encoding
   bug, not a font fact.** A probe showed every BMP-PUA glyph blank; the
   codepoints had been lost between writing the probe script and running it.
   A corrected probe rendered **every** candidate, including U+E0A0 — which is
   why powerline caps are in the final design at all.

**The lasting output of #2 is a hard rule:** write glyphs as `\u{...}` escapes
in source, never as literal characters. It bit twice in one session. Literals
get silently eaten in transit and the failure mode is tofu in production from a
diff that looked clean.

**Deny S6's `glyphs` plugin-config key.** zellij hashes plugin identity over the
whole config map, so a key miss **starts a second plugin** — the v0.1.1
double-sidebar mechanism. Found independently by the pre-fleet audit as its
blocker 4. Customisation folds into #40.

**A live row must render from the store, not the zellij tab name** (lock §7.1).
A tab name is one opaque string with no field structure, so it cannot fill
fixed-width columns; a manually renamed tab would lose column alignment exactly
where the user cared most. This **deletes `InkSpan`, `segment_span`, the
optional-title index arithmetic and `snapshot_ink_segments_match_compose_label_fields`**
from S5 — the parse-a-composed-name mechanism they serve no longer exists. It
also makes **#69 (AgentSnapshot v2) a blocker for S5 and S6**: `Agent` carries
only the composed `label`, with no `title` or `summary` field.

**Cap columns must be reserved on every row.** The selected row's powerline caps
occupy columns 1 and 44; without reserving them on unselected rows, the selected
row's content sits one column right of its neighbours — violating the alignment
rule exactly when the eye is most focused on it. Verified: title starts at
column 10 either way.

**Convention research (agent, primary sources).** `\u{f062c}` nf-md-source_branch
is lazygit's default and the Plane-15 carrier of the U+E0A0 powerline-branch
shape that starship/oh-my-posh/p10k share. **No worktree glyph exists anywhere**
— zero hits across Nerd Fonts' 10,764 names, 380 octicons, 639 codicons; only
lazygit draws one (`\u{f0339}` link_variant). **Essentially no tool marks the
default branch with a glyph** — the field uses colour (eza) or a text badge
(GitLab, lazygit). Hence: nothing for a main checkout.

**PR #70's delegated agent diverged three times, correctly:** `install_launcher`
**renames rather than copies** (a copy truncates an inode that may be a *running*
`clave` — ETXTBSY on Linux, live text segment on macOS); it extended the change
to `clave setup`'s single-file path, without which #29's "the scp'd file becomes
disposable" stays false; and CodeRabbit caught that its `debug_assert_eq!` on
launcher coherence is **compiled out of `--release`**, the only build a cut ever
uses — now `anyhow::ensure!`.

**Side effect of #43b:** `just dev-install` alone no longer sets up a working
sandbox. The sandbox bakes bare `clave` by design and used to get it from
`~/.cargo/bin/clave`; `just sandbox` is now the only supplier.

**Process gotchas hit this session.** A branch updated via `gh pr update-branch`
diverges from your local copy — fetch and merge before pushing. Branch
protection requires up-to-date-with-base, so merging PR N forces a branch update
on PR N+1 and a fresh CI round. And **CodeRabbit reports `pass` while
rate-limited** ("Review rate limited" in the check detail) — a green CodeRabbit
may have reviewed nothing. That is #68, and it is live.

## Next Steps

> **⚠ NOTHING IN THE SIDEBAR UX IS IMPLEMENTED.** This session locked the
> *design*. **Zero source code changed.** The bar today still renders the old
> 2-cell gutter at 30 columns with no colour. S0–S8 (#55–#63) are all
> unstarted. Do not assume any of the locked design exists in code.

**In priority order. Items 1–5 are the session plan.**

1. **Fix the 12 CodeRabbit findings on PR #71**, then reply to each thread with
   its disposition and resolve it (never silent-resolve — house rule). Branch
   `docs/release-runbook` is checked out and clean at `5ec7cc8`. Full triage is
   in the next section; **six of these are real defects that would break a
   release**, not nitpicks. Then merge #71.
2. **PR #70 needs a second review lane before merge.** Only the CodeRabbit CLI
   ran. AGENTS.md wants the vendored fugu lane (needs a session with the
   `Workflow` tool) **plus** an independent adversarial reviewer — but it also
   permits saying fugu was absent and substituting a second independent
   reviewer, so **fugu is not a hard gate**. State in the PR which lanes
   actually executed — a lane that did not run is not a lane that passed. Then
   ask before merging.
3. **Update the issues** — the checklist is a section of its own below. None of
   it is done, and it is the cheapest way to stop the next agent re-deriving
   what the design lock settled.
4. **#48 — `clave doctor` version-coherence.** The last hermetic blocker before
   a cut, and the highest-leverage one: it collapses runbook steps 1, 2 and 5
   into one assertable command with `--json` and a non-zero exit. PR #70's agent
   flags that **doctor currently says nothing about the launcher** — whether one
   exists, which version it copies, whether it is the `clave` PATH resolves.
   That is now the most valuable part of #48. Note the version-agreement half
   already exists (`generated_artifact_set_is_version_coherent`, PR #52); the
   path-existence half landed in #70; the live five-way check is what remains.
5. **The interactive live test, then the tag.** Follow
   `docs/dev/RELEASE-RUNBOOK.md`. Parts A and D are agent work; **Part C is the
   maintainer's and cannot be delegated or automated** — Tier 2 does not exist
   (#47), so nothing automated crosses the process/environment seam. `just
   release` and the tag are his alone, always.

**Open, and owed:**
- **The collapsed geometry is still not ratified** (lock §3). Every collapsed
  candidate was rendered against the old 6-column gutter, which is now 9 columns
  of lead-in. Re-render before fixing the number. Binding constraint:
  `BAR_TARGET_COLS − COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP (20)`, so at 44
  the collapsed target must be **< 24**. Also unresolved: whether collapsed
  truncates the whole label or renders **field 0 only** (title, else repo) —
  field-0-only rendered better.
- **The `~/.claude` never-write rule has a hole.** Sandbox task-output paths
  **symlink** into `~/.claude/projects/`, so a write believed to be sandboxed
  lands in the maintainer's real transcript store. Belongs in AGENTS.md.
- The three decisions owed from the pre-fleet audit, and its two hard spec
  collisions (`clear_tab_timeline`: S1 vs S5; `fit_label_str` vs `clamp_name`:
  S4 vs S5) still need an ownership ruling. Rebasing does not resolve them.

**Where work stopped — verbatim.** The last design ruling:

> "5c 25% fade.
> That's the one, looks amazing.
>
> Let's lock this in into a nice doc.
> Or with a good script.
> Then also, I want to lock in some terminology with you and put it in
> UBIQUITOUS_LANGUAGE.md - so that we have shared terms for all the aspects of
> this project. Things like the gutter, glyphs, provenance, the different text
> elements, tabs vs sessions vs panes - the renamed tab field, and whatever else
> we can agree on quickly."

and the instruction that shaped the release work:

> "we do have to do a proper interactive live test before cutting a release,
> which needs to always happen before cutting.
> we need to get everything lined up for that."

**Endorsed, verbatim — the rulings that produced the locked design.**

On the gutter, which became the position-lock invariant:

> "We should also keep the gutter the same width no matter how many glyphs
> render or not - and lock the render position of certain glyphs in the gutter.
> That way, if something goes wrong, all the icons that are the same will be
> vertically matching - and importantly, the text will always start at the first
> character of the text area, rather than shifting columns left or right based
> on the glyphs."

On column alignment, which killed the branch column:

> "I am realising something important though. We need to vertically align the
> text. So we will need a set truncated width for each of the pieces of text we
> show. For example, 7 chars for the rename text, 7 chars for the repo name, 7
> chars for the branch name, and then as many characters fit the width for the
> summary."

then, after seeing it rendered: *"You're right, the branch column isn't really
needed"*.

On the status rule:

> "I'm also wondering, the battery next to the status indicator is difficult to
> distinguish colour of the status indicator quickly. I think the status
> indicator should be separated from the rest by, instead of a blank column to
> the right of the circle glyph, we use the long vertical pipe character in
> white"

On the two colour channels — the reason repo and title render differently:

> "for the renamed tabs, we have solid background colour with dark text, while
> for repo name we have just the text colour - these two colours are both
> telling something distinct - repo colours should be different and distinct,
> but for example, two clave repos should both be the same colour. For renames,
> these also need to be distinct from each other, so two renames of clave tabs
> should not be the same colour."

On the palette:

> "the colours look good for the first few, but they start colliding after the
> 5th colour, some are too similar. We probably don't need as many, but a
> distinct 8 colours would be good. Maybe pull from the kanagawa theme colours
> if possible"

On provenance, after the convention research: *"I think the conventional is the
best approach, no main glyph."*

On width, which set 44:

> "I've actually found myself using clave in full-width sidebar most of the time
> because I still have plenty of room to work when the sidebar is full width […]
> So, a bit more width for the sidebar is better even when collapsed."

---

## PR #71 — the 12 CodeRabbit findings, triaged

Read in full and analysed; **six are real defects that would break a release**,
not style. The runbook gates every cut, so these matter more than their size
suggests. Branch `docs/release-runbook`, clean at `5ec7cc8`.

### Fix — ordering and correctness

1. **Part B pushes the tag before anything is validated** (P1). `git push origin
   vX.Y.Z` immediately triggers `release.yml` and publishes the release — so any
   defect Part C finds is discovered *after* it is public. Worse, **`just
   release` never appears in the runbook at all**, so the local launcher and
   generated KDL are still on the previous version and Step 1 validates the OLD
   cut. Correct sequence: **tag locally → `just release` → Part C → push the tag
   only on a go decision.** This is the single most important fix.
2. **Step 2's regex cannot detect what Step 2 claims to reject** (P1). It matches
   only `clave-bar-vX.Y.Z.wasm|clave-vX.Y.Z`, so a bare `clave` or an
   unversioned `clave-bar.wasm` is invisible — and if the same files also hold
   one correct versioned path, the output is "exactly one version == the tag"
   and the table advances to Step 3. That is precisely the skew the check
   exists to stop. Add an explicit search that FAILS on unversioned references.
3. **`launch.kdl` is stale by design before launch** (P1). `just release`
   rewrites `config.kdl` and `layout.kdl` only; `launch.kdl` is rewritten by
   `clave` at cold start (Step 3). The `*.kdl` glob therefore reports two
   versions and orders a STOP on a perfectly healthy upgrade. Exclude
   `launch.kdl` from Step 2; assert it in Step 3, after it has been written.
4. **Rollback does not roll back** (P1). Relaunching the old versioned binary
   reattaches a live session; and even after the session is killed, `config.kdl`
   still names the failed version, because the old binary sees its own wasm
   present and skips the setup refresh (`needs_version_refresh`). Rollback must:
   human kills the session → regenerate stable config/hooks from the last-good
   tagged release → relaunch.
5. **The log filter is scoped to the calendar day, not the launch** (P2). The
   zellij log is shared by every session on the machine and old entries linger,
   so a normal release day returns both the previous and the new version and
   falsely fails a coherent cut; a midnight crossing returns nothing. Capture
   the log's line count immediately before Step 3 and read only lines appended
   after it (`tail -n +$((N+1))`).
6. **A nonzero `clave doctor` exit is never defined as STOP.**
   `crates/clave/src/doctor.rs` exits 1 on a `Severity::Problem`, but Step 5
   only asks for the output. Make the exit status the pass/fail condition.
7. **MD031** — blank lines around fenced blocks (markdownlint). Mechanical.

### Decline, with reason

8. *"Add the promised sidebar design-lock handoff."* It **is** in this PR, as
   `docs/status/2026-07-26-1128-sidebar-design-lock.md`. CodeRabbit read only
   `RELEASE-RUNBOOK.md` and concluded it was missing. Reply saying so; do not
   change scope.

### Amend — already applied above

9. *"Keep the recommended fugu lane optional."* Correct. AGENTS.md says "two
   lanes, both", but also permits stating fugu was absent and substituting a
   second independent reviewer — so it is not a hard gate. Next Steps item 2
   now reflects that.

**Process rule that applies here:** always fix what CodeRabbit returns, reply
saying how it was addressed, *then* resolve. Never silent-resolve. Expect
several rounds. And note #68 — a green CodeRabbit check may say "Review rate
limited" and have reviewed nothing; read the check detail, not just the colour.

---

## Issues to update — none of this is done

Cheap, and it stops the next agent re-deriving what is already settled.

| Issue | What it needs |
|---|---|
| **#44** | closed by #66 — verify it auto-closed, close by hand if not |
| **#43** | #70 implements 43a (release owns an unversioned launcher) and 43b (`dev-install` → `clave-dev`). Comment; close when #70 merges |
| **#24** | sidebar-distinctiveness epic — the design lock settles items 2, 6 and 7. Comment with a pointer to the lock doc |
| **#60** (S5) | **supersession note**: palette 12 → 8 kanagawa; title is a filled CHIP not tinted text; fixed-width columns; **`InkSpan`/`segment_span` deleted** |
| **#61** (S6) | **supersession note**: gutter is not 3 cells; §2.8 void; glyph set settled; **drop the `glyphs` config key** (identity-hash hazard); escape rule |
| **#63** (S8) | **supersession note**: target is **44**, not 38; re-derive the expected-red test set; collapsed target still open, must be **< 24** |
| **#69** | now a **BLOCKER for S5 and S6** — `Agent` carries only the composed `label`; the locked design needs `title` and `summary` as structural wire fields |
| **#40** | absorbs user-facing glyph customisation, since the `glyphs` plugin-config key was rejected |
| **#35 / #49** | advanced by #71's runbook |
| **#62** (S7) | the battery cell is reserved in the locked gutter — note the geometry |
| **#47** | Tier 2 still does not exist; it is why Part C cannot be automated |
| **#68** | still open — review lane degraded; observed live twice this session |

## Context to Preserve

- **User prefs (binding):** extremely concise, signal over noise; explain while
  doing; dense why-comments citing spec §/ledger/issue, never restating what;
  conventional commits + the `Claude-Session:` trailer; **never commit without
  explicit approval** — he signs; ask before architecture decisions with
  multiple valid approaches; **he drives ALL live zellij input**.
- **He decides from rendered artifacts, not from prose.** Build the mockup.
  Offer costed options with a recommendation, and expect to be overruled.
- **He responds better to concrete asks than to summaries.** State the literal
  command to run and the literal answer shape needed, one or two at a time.
- **`AGENTS.md` never-list, in force:** never launch or kill a zellij session ·
  never run `just release` · never run `cargo install` or `just dev-install`
  while he may be daily-driving · never write versioned artifacts under
  `~/.local/share/clave/` · never write anywhere under `~/.claude/` · never
  commit without explicit approval.
- **The one sanctioned live mutation** is hot-reloading the sandbox bar in the
  `clave-test` session. Post-#66 that command needs `-c clave_binary=clave` or
  it starts a second bar.
- **Never resolve a review thread without a comment saying how it was
  addressed**, and always fix what CodeRabbit returns. But see #68 — a green
  CodeRabbit may have reviewed nothing.
- **SSH is a hard constraint** — clave must eventually work with the CLI and the
  terminal on a remote host. Reject designs assuming a shared local desktop.
- **A second agent may be live in this worktree.** Stage by explicit path, never
  `git add -A`.
- **Promise made:** the collapsed geometry would be re-rendered against the new
  gutter before its number is fixed. Not yet done.

## Restart Hint

**Start here:** `git checkout docs/release-runbook` (it is the PR #71 branch and
holds this file), then work Next Steps item 1 — the 12 findings on #71. `main`
is green at `ea0f009`; no source is touched anywhere and nothing is
mid-refactor, so the tree is safe.

Read, in this order: this file → `AGENTS.md` (which now imports itself into
CLAUDE.md) → `UBIQUITOUS_LANGUAGE.md` → the design lock. Run `cargo run -p
clave-bar --example bar-preview` once to see what was ratified — it is
one screen and it saves reading §2 of the lock doc twice.

**Do not** assume any sidebar UX exists in code. It does not.
