# Live interaction checklist — the sidebar at 54 columns

> **Partially run 2026-07-30, then invalidated by its own findings.** Item 1
> passed decisively — six-plus toggles across three display widths, the pane
> moved every time, so D21's bug is dead and D26's fix holds live. Item 5 was
> started and surfaced the wrap bug that became D31. **Everything numeric below
> was then superseded**: D33 took expanded 44 → 54, and D35 changed how the bar
> is BORN, which changes every resting width. The numbers here are the new ones,
> re-derived through the real `width_seek`; they have not themselves been seen
> live yet. Item 1 is worth re-running only because the widths it observes are
> different now, not because it failed.

The D28 gate-2 run: **Gate 1 validated the design by looking at it; this validates
the behaviour by driving it.** Nothing below is covered by any automated tier.
It belongs to [`TESTING.md`](TESTING.md)'s
live-validation SOP: the interaction contract there governs (**the human drives
every keypress and every session launch; the agent reads observability and never
puppets the session**), and the sanctioned-command list there is the whole of what
an agent may run.

Read the SOP first if you have not. Then this.

## Why this is not a formality, and the rule that makes it real

Every item below states three things, and the third is the one that keeps this
honest:

- **Do** — the exact keys or commands.
- **Correct** — what the screen must show.
- **Vacuous if** — the conditions under which the observation *means nothing*,
  so a clean run is not mistaken for evidence.

That third field exists because this project has already shipped a vacuous live
plan: a previous session wrote steps to observe a state its own scenario could not
produce. That is one of `TESTING.md`'s six shapes wearing a terminal instead of a
test runner — an observation satisfied by the behaviour *and* by its opposite
(shape 1), or one that has quietly stopped exercising what it names (shape 2).
**Before you record a PASS, check that the scenario could have produced a FAIL.**

Two vacuity conditions are global. If either holds, stop and fix it — every
observation below is void:

- **Two bars in one tab.** The #44 symptom invalidates everything: executor
  election scrambles, nav half-dies, and Alt+c looks dead for a reason that has
  nothing to do with the seek. Checked in setup step S5.
- **The wrong binary answering.** The PATH shim is load-bearing, not tidiness
  (setup step S3). Without it the bar drives the sandbox with the *stable*
  release, and a version string will not give it away.

## Reading the numbers

Several items ask for a **column count**, not a yes/no. Three ways to get one,
cheapest first:

1. **Count the summary cell.** Only `summary` flexes (D9/D16), so for any row
   whose summary text is *longer* than its cell, the rendered summary occupies
   exactly the cell — but the base now differs by profile, because #105 took
   the expanded gutter's battery cell to four columns while collapsed kept its
   one-column glyph: `cols = 16 + title_w + repo_w + summary_cells`, i.e.
   **`32 + summary_cells` expanded `(9, 7)`**, and `cols = 13 + title_w +
   repo_w + summary_cells`, i.e. **`23 + summary_cells` collapsed `(7, 3)`**,
   unchanged. `ux-gate1`'s summaries are all long enough. No rebuild, no
   reload.
2. **Percent × display, from the layout dump** (agent-side, liveness-gated,
   ±2 columns because the serialized constraint is an integer percent):
   `ZELLIJ_SESSION_NAME=clave-test zellij action dump-layout`. The bar pane is
   percent-constrained, so the dump reports `size="N%"`, never a column count.
3. **Instrument it** — exact, and the documented loop: `TESTING.md`'s
   instrumentation recipe, a temporary `eprintln!("CLAVE_DBG_cols …")` in
   `clave-bar`'s `render`, rebuilt with a fresh `CLAVE_BUILD_TAG`, copied into the
   **sandbox** data dir only, hot-reloaded with the `-c clave_binary=clave` form.
   Strip it before committing. Use this if item 1 or 6 produces a surprise worth
   pinning to a number.

Also record the **window width** once, in columns, because every expected number
below is derived from it. Zellij resizes in ≈5%-of-display-area steps, so
`step ≈ W × 5 / 100`. At W = 280 (the maintainer's usual), `step = 14`.

**Since D35 the step no longer decides where the bar rests — birth does.** The
bar is born at `birth_percent_for(W)` of the real terminal, which lands it within
a column or two of 54, inside the acceptance band at any step. It therefore
settles immediately and every later toggle returns to that same width. Predicted
rests, derived through the real `width_seek` (not yet confirmed live — that is
what this run is for):

| W | step | born | expanded rest (summary) | collapsed rest (summary) |
|---|---|---|---|---|
| 120 | 6 | 54 | 54 (25) | 30 (7) |
| 160 | 8 | 54 | 54 (25) | 30 (7) |
| 200 | 10 | 54 | 54 (25) | 34 (11) |
| 240 | 12 | 55 | 55 (26) | 31 (8) |
| **280** | 14 | 53 | **53 (24)** | **25 (2)** |
| 320 | 16 | 54 | 54 (25) | 38 (15) |
| 400 | 20 | 56 | 56 (27) | 36 (13) |

54 is not exactly expressible as a whole percent of most displays — at 280 one
percent is 2.8 columns — so 53 rather than 54 is correct, not a defect.

**The collapsed number at 280 is the one to look at with fresh eyes.** It was 33
(summary 10) before this branch and is predicted at 25 (summary 2). The title
chip and the 3-cell repo still fit (13 + 7 + 3 = 23), so the gutter reads; only
the summary is nearly gone. If that is too thin, the lever is
`COLLAPSED_TARGET_COLS` — around 36–39 selects the 39 lattice point instead
(summary 16). **This is a design question for the maintainer, not a bug.**

## Setup

Commands marked **(human)** are the human's, in a terminal **outside** zellij.
The rest an agent may run.

**S1 — kill any live sandbox session (human).** `just sandbox` refuses while a
`clave-test` session lives, and it is right to: regenerating `config.kdl` under a
live session re-keys its keybinds to an identity the running bar does not have,
and the next keypress starts a second bar.

```bash
zellij kill-session clave-test && zellij delete-session --force clave-test
```

**S2 — seed the fleet.** From the repo root:

```bash
just sandbox ux-gate1
```

What it does, and the parts that matter: it builds the working tree, drops the
wasm in the sandbox data dir, wires the PATH shim, runs **`dev reset`** (which
wipes the sandbox store and removes the `c85c`-tagged scenario transcripts), then
`dev scenario ux-gate1`, which regenerates `config.kdl` **and** `layout.kdl`
together and re-seeds seven agents with seven real `claude -p` calls. It
self-checks the #44 identity pair, proves it touched neither `~/.cargo/bin/clave`
nor `~/.local/share/clave`, and **prints the launch command rather than running
it**.

Two consequences worth knowing before you run it:

- **It writes `~/.claude/settings.json`.** `dev scenario` calls `clave setup`,
  which merges the hook registrations keyed on the *sandbox's* binary — bare
  `clave`. So a sandbox setup rewrites the daily fleet's hook commands from the
  versioned absolute path to bare `clave`, resolved through `PATH` at hook time.
  It is replace-in-place and idempotent, and the next `just release` writes the
  versioned path back. Nothing else outside the sandbox is touched.
- **Every run is a cold seed.** Because `dev reset` runs first, running
  `just sandbox` twice proves nothing about re-runnability; `dev scenario` twice
  is that test, and it is not this one.

**S3 — launch (human, non-zellij terminal).** Use the command the script printed,
verbatim, **including the PATH shim** — the sandbox bakes bare `clave`, so without
the shim the bar shells out to the stable launcher and the run tests the wrong
binary.

**S4 — first paint, and the fleet you should be looking at.** The launch layout
eagerly opens the single most-recent row and leaves the rest dormant, so expect
**one live tab and seven rows**, recency-ordered:

| # | row | expect |
|---|---|---|
| 1 | `CART-99` / webapp | live and selected; its store status is `Done`, so a green dot at first paint that clears to untinted once the §6.5 unread clear fires on focus |
| 2 | `SYNC-T9` / nalu | dormant, branch mark |
| 3 | `UX-GATE` / clave | dormant, worktree mark |
| 4 | *(blank chip)* / clave | dormant, **no** provenance mark |
| 5 | `DNS-TTL` / infra | dormant, branch mark |
| 6 | `KDL-GRD` / clave | dormant, worktree mark — this is the one whose cwd was deleted |
| 7 | `README` / docs | dormant |

Every dormant row shows the dormant mark **U+25CB** whatever its stored status:
dormancy outranks `Status` in the row-state order, so the seeded `NeedsYou`,
`Failed` and `Working` values are not visible until those rows are opened.

The blank chip on row 4 and the blank battery cell on every row are **ratified**
(D19), not gaps. Rows 3, 4 and 6 share one repo and must therefore share one repo
ink and one gutter-mark colour.

**S5 — one bar per tab (the master vacuity check).** Agent-side:

```bash
grep 'clave-bar: loaded' "$TMPDIR"/zellij-*/zellij-log/zellij.log | tail -5
```

Every line from this run must report the **same version and the same `build=`
tag** — the tag is what distinguishes two builds of one version. And grep for
`not found, starting it instead`: a hit means a plugin-identity miss spawned a
second bar. Visually: each tab has exactly one sidebar.

**Teardown.** From the repo root, after the run:

```bash
./target/release/clave dev reset      # prints the kill-session line, then wipes
```

(`clave-dev dev reset` is the same thing if `clave-dev` is installed.) Reset
removes the sandbox store, the seeded repos and the `c85c`-tagged transcripts.
It does **not** remove a transcript for any agent you create by hand in item 3 or
4 — those are real sessions in real project dirs and are not scenario-tagged — and
it does not restore `~/.claude/settings.json`'s hook path.

---

## 1. Collapse and expand — the highest-value item

This is where a real bug lived (D21) and was fixed (D26). The bug was that
`Alt+c` emitted **zero** resizes on a wide display and the pane silently did not
move.

> **This item already PASSED, 2026-07-30**: six-plus toggles across three display
> widths, clean every time, including a monitor change and a half-width window.
> D21 is dead and D26's fix holds live. It is re-listed only because D33 and D35
> changed every width it observes — the property is settled, the numbers are not.

**Do.** Press `Alt+c`. Wait until the bar stops moving. Press it again. Repeat for
**at least six consecutive toggles**, one at a time, and count.

**Correct.**

- **The pane moves on every single toggle.** Six presses, six visible width
  changes. This is the D26 property, and it is the whole point of the item.
- It comes to rest somewhere and stays there — no pacing, no oscillation, no
  crawl.
- The rows render the collapsed profile the whole way: repo 3 characters with
  **no ellipsis** (D18) and the summary simply shorter. Nothing over-runs the
  pane at any point in the animation — and since D31 nothing *can*, so a row
  wrapping onto a second line is now a hard finding rather than an expected
  transient.
- **The title chip now REFLOWS, 9 → 7, and that is correct** (D33). D17's
  no-reflow-across-toggle property was retired deliberately when expanded took
  the title to 9. Titles of 7 characters or fewer look identical either way, so
  on the `ux-gate1` fleet you may not see it at all.
- **Record the two rest widths.** At W = 280 the derivation predicts **53
  expanded and 25 collapsed** — summary cells of **24 and 2**. Both differ from
  the pre-D35 run (47 and 33), and for a different reason than before: the bar
  is now born near target and the toggle lattice is anchored there.
  - **53, not 54, is correct.** 54/280 is 19.29%, and a KDL size is a whole
    percent, so 19% floors to 53.
  - **Collapsed at 25 with a 2-cell summary is the number to judge with fresh
    eyes.** It is above `Widths::COLLAPSED`'s 23-cell floor, so nothing clips —
    the chip and repo are intact and only the summary is nearly gone. If it
    reads as too thin, `COLLAPSED_TARGET_COLS` around 36–39 selects the 39
    lattice point instead (summary 16). A design call, not a bug.

**Vacuous if.**

- You toggled again before the previous seek settled. Every toggle re-arms the
  machine, so a burst measures nothing — the rest width you record is an
  intermediate. One press, wait, look, then press.
- You navigated during the test. A nav onto a dormant row arms a **peek**, which
  renders and seeks the *expanded* profile for 0.9 s even while collapsed. Keep
  hands off the arrow keys for this item.
- Your window is around 400 columns wide. Above `MAX_LEARNABLE_STEP = 20`
  (display ≈ 400) the seek enters a regime the proptest generator never reaches
  and D26's census does not cover. Absence of a finding there proves nothing —
  see *What this cannot test*.
- You watched a bar in a **non-focused** tab. Every instance drives its own pane,
  but only the tab you are looking at is being exercised.

## 2. A new tab's first paint

**This item tested the birth jank, and D35 is supposed to have removed it.** The
percent is now derived from the real terminal at launch rather than a fictional
200-column viewport, so the newborn should arrive at its rest width instead of
healing toward it. The item is therefore inverted: it used to pin the jank, and
now it pins the jank's ABSENCE.

**Do (a), wide window.** `Alt+t`. Watch the new tab's own bar on its first paints.

**Correct (a).** At W = 280 the bar is born at **53** and **stays there — zero
visible resizes**. Any shrink-then-settle is a finding now, not the expected
behaviour: it means the percent reaching the layout is not the one derived from
the terminal. Record the birth width and the number of steps.

**Do (b), narrow window.** Resize the terminal to **≈ 115 columns**, then
`Alt+t`.

**Correct (b).** `birth_percent_for(115)` = 47%, which floors to **54** — still
at target, still above `EXPANDED`'s `min_intact_cols()` floor (32 as of #105), so no over-run at all. The old
expectation here was a deliberately clipped newborn healing upward; that regime
is now only reachable on a display too narrow to hold 54, where the percent
clamps to 100 and D31's clip keeps the rows inside the pane. **Ragged** clipping
— agent rows and terminal rows disagreeing on width — remains a finding.

**Vacuous if.**

- **You launched the session before this branch.** The percent is baked into
  `launch.kdl` at session-create time, so a session started with the old binary
  carries the old percent whatever the code says. Relaunch, do not hot-reload.
- **You resized the terminal after launching.** The template's percent was
  correct for the width at launch; resizing changes the display area underneath
  it, so a later `Alt+t` is born against the old ratio. That is expected, and it
  is what item 6's drift re-arm exists to absorb.
- **You opened the tab with `Alt+a` or by committing a dormant row.** Those go
  through `clave open`, which builds a one-shot layout that bypasses the
  template — and still uses the fiction (D35's named gap, task 7b′). Only
  `Alt+t` exercises the fixed path.
- You watched the *old* tab's bar. `Alt+t` focuses the new tab; the newborn is the
  bar in it.
- The bar was collapsed. Then the newborn seeks 30, not 54, and the arithmetic
  above does not apply.

## 3. The hook writing `title` and `summary` — the newest, least-tested path

Before this branch **nothing wrote `title` or `summary`**. This is the youngest
code in the change and the only path whose input comes from outside the repository.

**`ux-gate1`'s rows have seeded `title`/`summary` and prove nothing about the
hook.** They were written straight into the store by the seeder, and **every one
of the seven has a non-empty `summary`** — so the prompt-seed tier, which fills
only while `summary` is empty, cannot fire on any of them. This item therefore
requires a **genuinely new agent**; a resumed seeded row can corroborate the
rename half at best.

**Do.**

1. Create a new agent: `Alt+a` → pick a directory → `new`. It runs a real `claude`
   in a real dir, and the row lands in the **sandbox** store with an empty
   `summary` and no `title`. (`Alt+a`'s picker is fzf over zoxide's list plus the
   pane's cwd, so `zoxide add <dir>` first if the dir you want is not in it.)
2. Send it a prompt. Watch the summary column.
3. Let the turn finish. Watch it again.
4. Rename the session (`/rename LIVE-1` — the command that appends a
   `custom-title` line). Then **send another prompt**.
5. Optional, cheap, and worth it: `/clear`, then prompt again.

**Correct.**

- **After step 2** the summary shows your prompt text. That is tier 3, and it is
  fill-only-while-empty — a seeded or earned summary is never regressed to prompt
  text.
- **After step 3** the summary is replaced by Claude's `ai-title` (tier 1), if
  Claude wrote one this session. Measured today, 2026-07-29: **75 of 153** local
  transcripts carry an `ai-title` line, **76 of 153** carry a `custom-title`, and
  `{"type":"summary"}` — tier 2, the extinct one — appears in **0**. So tier 2
  will not fire, ever, and its silence is not a defect (D23).
- **After step 4** the title chip fills with `LIVE-1`, dark text on a palette
  background, clamped to 7 cells. **The chip does not appear at the moment you
  rename** — the tail is read only on `Stop` and `UserPromptSubmit`, so the rename
  lands on the next of those. A blank chip immediately after `/rename` is correct;
  a blank chip after the next prompt is a finding.
- **After step 5** the chip still reads `LIVE-1`. `/clear` appends an *empty*
  `custom-title`, and the empty-value skip is what holds the last real rename
  across it (#24). A chip that blanks here is a regression in that skip.
- **`ai-title` does not roll (D24).** Up to 85 lines per transcript and never
  more than one distinct value. A summary column that never changes again for the
  life of the session is **correct**, not stuck. Do not file it.
- The chip's colour is provisional and positional: every repo's *first* title gets
  palette index 0, which is why most chips are blue. S5 fixes it. Not a finding.

**Vacuous if.**

- You read a seeded row's chip or summary. Those values came from the seeder.
  The proof is that the value you observe is one **only the hook could have
  written** — your typed rename, your prompt text, Claude's own title.
- You never sent a prompt after the rename. Nothing reads the tail otherwise.
- The agent's transcript moved. A session whose cwd changes gets its `.jsonl`
  **relocated** into a new project dir, and a stale path silently stops returning
  anything (#87). Confirm the file is where the hook looks:
  `find ~/.claude/projects -name '<uuid>.jsonl'` — one hit, under the current cwd.
- The row you are watching is dormant. Dormant rows have no live hook.

Agent-side corroboration, so a screen reading is not the only evidence:

```bash
./target/release/clave dev status | jq '.store.agents[] | {label, title, summary, default_branch}'
```

## 4. Provenance — all three states, and the one that needs a new row

Three states, not two (lock §5.1): a **main** checkout renders **nothing**, a
branch renders U+F062C, a worktree renders U+168C2, and the mark takes the repo's
ink so repo identity is a shape in the gutter as well as a colour in the text.

**Do (a), the states the fleet can already show.** Read rows 2–6 from S4: `cold`
blank (main), `sync` and `dns` marked branch, `gate` and `vanished` marked
worktree. Confirm the two marked shapes differ and the three `clave` rows share
one colour.

**Do (b), the behaviour this branch actually added.** `default_branch` is
`None` for **every row created before this branch**, and `merge_resume_record`
preserves it wholesale, so a pre-existing row can never demonstrate the new path —
it falls back to the `main`/`master` name test, which is exactly what shipped
before. What demonstrates it is a **new row in a repo whose default branch is
neither `main` nor `master`**:

```bash
mkdir -p "$TMPDIR/trunk-repo" && cd "$TMPDIR/trunk-repo"
git init -q -b trunk && git commit -q --allow-empty -m init
git config init.defaultBranch trunk        # makes the default DISCOVERABLE
zoxide add "$PWD"                          # so Alt+a's picker can reach it
```

Then, in the sandbox session: `Alt+a` → that dir → `new`. Expect **a blank
provenance cell** — the old heuristic would have marked `trunk` as a branch. Then
`git switch -c wip` in that repo and add a second agent: **marked branch**. Same
repo, two rows, opposite answers.

**Correct.** Blank for the default branch whatever it is called; marked for
anything else; worktree beats both.

**Vacuous if.**

- `default_branch` came back `None`. `resolve_default_branch` asks
  `origin/HEAD` first, then `init.defaultBranch` **and verifies the ref exists** —
  a bare `git init -b trunk` with a global default of `main` resolves to `None`,
  falls back to the name test, and marks `trunk` as a branch. That is *correct*
  behaviour for an undiscoverable default and tells you nothing about the new
  path. Check the field before interpreting the glyph:
  `./target/release/clave dev status | jq '.store.agents[] | {branch, default_branch}'`.
- The glyph is missing rather than absent. **U+168C2 is BAMUM LETTER PHASE-C
  MBERAE and is not a Nerd Font glyph** — in practice only Noto Sans Bamum has it,
  so a working battery glyph says nothing about its coverage. A blank where a
  worktree mark belongs may be font fallback, not logic. Distinguish by checking a
  row you know is a worktree against `dev status`.
- You read the marks on the row you are hovering. The 25% fade dims unselected
  rows; a dim mark is still a mark.

## 5. Navigation and selection

**Do, and check each.**

- **The list is two blocks** (#112). Every live row first, then every dormant
  row, each block ordered by the commitment ordinal. Confirm the live block is
  **contiguous and complete** — no dormant row above a live one, and no live
  row stranded below the join. On the real store expect ~4 live above ~17
  dormant.
- `Alt+Down` / `Alt+j` — next display row **within the focused block**. `Alt+Up`
  / `Alt+k` — previous. Both wrap at that block's ends: walk forward off the
  last live row and you arrive back at the first, **not** on the dormant row
  sitting directly below it on screen. Cycle the whole live block twice in each
  direction and confirm you never leave it (#112).
- **Clicking a list focuses it.** Click any dormant row, then `Alt+j`/`Alt+k`:
  the walk is now inside the **dormant** block, moving the selection row by row
  and switching no tab, and it wraps at that block's ends without escaping into
  the live block above. Click any live row and the walk returns to the live
  block. **A walk never changes which block it is in — only a pick does**
  (click, or `Alt+N`).
- **The selection self-heals.** With a dormant row selected, `Alt+Enter` it. Once
  it comes up as a tab, `Alt+j` should be walking the **live** block again — the
  focus follows the row, and the row is no longer dormant.
- `Alt+1` … `Alt+9` — jump to display row N (1-based), counting across BOTH
  blocks. This is the only keyboard route into the dormant block, so with 4 live
  rows `Alt+5` is the most recently closed one. On a **dormant** row it
  **selects** — nothing opens (#100 reversed the old immediate-open; selection
  and launch are two separate acts on every input path).
- **Nothing wakes a dormant row except `Alt+Enter`** (#100/#116). Pick a dormant
  row with `Alt+N` or the mouse and **stop** — the gutter shows the commit mark
  **U+23CE** (carpYellow), the row takes the highlight, and no matter how long
  you park, **no tab appears**. The 0.4 s dwell is deleted.
- **A dormant selection survives a walk.** With a dormant row selected, press
  `Alt+j`: the selection moves to the next dormant row and stays in that block
  (#112). Only picking a live row hands the walk back.
- **Commit-to-open.** With a dormant row selected, `Alt+Enter`. The mark goes
  ⏎ → ↻ (U+21BB, carpYellow), a tab appears, the row goes live and the
  selection reverts to the focused tab. A second `Alt+Enter` while ↻ is
  in flight is a no-op. `Alt+Enter` with no dormant selection is a no-op and
  the keypress is consumed — it must NOT reach the terminal (bare Enter must,
  or you cannot talk to Claude).
- **The dead row.** Select the `KDL-GRD` row (its cwd was deleted; it wears the
  stale mark **U+2717** in red after any failed open). The ✗ **outranks** the ⏎
  — a stale row never offers a launch — and `Alt+Enter` on it **refuses**: no
  ↻, no tab, no store write (ratified live 2026-08-01; a dead row is #112's
  retirement business). U+2717 is the stale flag; U+2716 is `Failed`. They
  render the same red, so shape is the only discriminator — do not transpose
  them.
- **The virtual cursor.** While the cursor sits on a dormant row, **every live row
  loses its highlight** and the dormant row carries it — background waveBlue2 plus
  both powerline caps. Exactly one row is highlighted at all times.
- **The selection dies with the context.** Select a dormant row, then `Alt+o`
  (or click a live row): the highlight and ⏎ resolve back to the focused tab,
  and an `Alt+Enter` pressed immediately after opens nothing — the organic
  pipe spends the selection before the beacon returns (#128 review).
- **The fade.** Unselected rows sit 25% toward the bar background. Compare a
  chip's colour on the selected row against the same chip unselected.
- **Peek while collapsed.** `Alt+c`, then navigate — onto a dormant row, or to
  another live tab (that path arrives as the `clave-visited` pipe). The bar
  expands to the template width while you walk and sinks back ~0.9 s after the
  **last** press, so a burst of presses is one expand and one sink, not a
  flicker per key. An explicit `Alt+c` mid-peek outranks it and stays where you
  put it.
- **Nav after a close.** `Alt+w` on a tab, then `Alt+Up`/`Alt+Down`. Nav must keep
  working — this stranded until a mouse click before `Effect::ReanchorVisit`
  (#23), and only a live session can show it.
- **Mouse.** Click a live row → switches to that tab. Click a dormant row →
  **selects** it (#100: the mouse is the main path to dormant rows past
  `Alt+9`, so a click that launched would just move the accidental spawn into
  the mouse channel). Click reaches only the visible bar.

**Vacuous if.**

- You navigated in a bar that is not in the focused tab. Row-walk, row-jump and
  the commit are **executor-gated**: only the instance whose own tab is the
  current tab computes the step. Watching the wrong bar looks like dead nav.
- You held the key down. Key repeat is a burst of landings; the selection just
  follows the cursor, nothing opens, and that is correct.
- Your terminal is not delivering mouse events to zellij. Verify by clicking a
  live row first — if a switch happens, the mouse path is alive.
- You pressed `Alt+Enter` with the cursor on a live row (or no cursor at all).
  The commit acts only on a selected, non-stale, still-dormant row.

## 6. Window resize — drift re-arm

Percent geometry moves under a window resize, so a resized window leaves the bar
off-target and the seek must **re-arm** (#4) — under bounds that stop it fighting
a mid-drag flicker or a thrashing layout.

**Do.** With the bar settled and expanded, resize the terminal window **by a
lot** — halve its width, from ≈280 to ≈140 — then leave it alone and watch.

**Correct.** The bar's cols fall proportionally (≈47 → ≈23, which is under
`EXPANDED`'s `min_intact_cols()` floor (32 as of #105), so expect a transient uniform CLIP — D31, not an over-run), the
same off-target width is observed twice, the seek re-arms, and the bar grows back
to ≈54 and stops. Then
widen the window again and watch it come back down. No thrashing, no parking
off-target, no fight with the layout.

**Vacuous if.**

- You resized by a little. Gate B settles in place when the new width is within
  one learned step of where the seek last acted — deliberately, because re-arming
  there would chase its own in-flight resize forever. A small resize *should*
  produce no movement; observing none proves nothing about drift.
- The new width happens to fall inside the acceptance band. Same outcome, same
  non-observation.
- You were still dragging. Drift requires the **same** width twice; a mid-drag
  flicker never confirms, by design. Let go, then watch.
- **Watch for this one, it is a real open question:** re-arm needs a *second*
  render at the stable new width, and renders are event-driven. If the bar sits
  off-target after the resize, generate an event — `Alt+o`, or switch tabs — and
  see whether it heals then. **If it heals only on the next event, the drift
  confirmation is waiting for a render a quiet session may never deliver.** That
  is a finding worth an issue, and nothing in the hermetic tiers can see it.

## 7. Terminal tabs

A terminal tab has no store row, so it renders differently on purpose (lock §5,
§7.1): the console mark in the battery cell, and its zellij **name** across the
whole body.

**Do.** `Alt+t`, then read the new tab's row in the bar.

**Correct.** Blank status cell (a terminal has no turn), the rule, the console
mark **U+F018D** where a battery would be, a blank provenance cell, and the zellij
tab name (`Tab #N`) in muted grey across the body, clamped to the body width with
an ellipsis when it does not fit. The name is the **only** place a zellij tab name
reaches the bar; an agent row must never show one.

**Watch item, already recorded as a trap:** the row's sort key is the store's tab
timeline, stamped once at birth, and **zellij recycles tab ids** — `get_new_tab_id`
is `keys().last() + 1`. So: `Alt+t`, `Alt+w` on that newest tab, `Alt+t` again. The
recycled id was already birth-touched by the previous instance, `birth_touched`
never re-arms, and the new tab can end up permanently unstamped — which sorts it
**below every dormant row**. If a fresh terminal tab appears at the bottom of the
list, that is this, and it is deterministic rather than racy.

**Vacuous if.** You renamed the tab, or your zellij config names tabs differently
— then you are reading a different string than the default and the clamp behaviour
you are checking is not the one the golden pins.

## 8. Session-id rotation and the identity gate (#97, PR #98)

**This item is unlike the others: it verifies an assumption, not a rendering.**
The whole rotation fix rests on one thing no test can reach — that a hook
process inherits the environment `clave spawn` set before exec'ing Claude. If
it does not, `resolve_row`'s fallback never fires and the fix is a silent no-op
that still passes all 286 tests.

**Partial reassurance before you start, so you know what is actually novel.**
`dev.rs:352` records that hook processes already inherit `CLAVE_STATE_DIR` from
their Claude parent — that is how sandbox events reach the sandbox store, and it
has been load-bearing since 2026-07-18. So inheritance-through-Claude is proven.
What is NOT proven is this injection point: `CLAVE_STATE_DIR` is set on the
zellij session and flows down, while `CLAVE_AGENT_UUID` is set on the
`Command` immediately before `exec`. Both should land in the same place. Should.

### Setup

`just dev-install` builds the working tree as `clave-dev` and copies the wasm —
**Ollie runs this**, it writes outside the repo. Then `just sandbox`, which
wipes the sandbox store first; that is fine here because this item needs a
genuinely new agent anyway.

### Do

1. **Create a new agent** — `Alt+a` → pick a directory → `new`. A seeded row
   cannot be used: seeded rows never went near a real transcript or a real
   `clave spawn`, so they have no `CLAVE_AGENT_UUID` in any process.
2. Send it a prompt, let the turn finish. Confirm the row behaves normally —
   this is the **control**, and it exercises the payload-id path, not the fix.
3. **`/clear` the agent.** This is the rotation trigger, confirmed on
   2026-07-31: Claude mints a new session id and starts a new transcript.
4. **Send another prompt.** This is the whole experiment.
5. **The nested-Claude probe.** In the same agent, run
   `claude -p 'Reply with exactly: OK'` and let it finish.
6. **Optional, answers #99.** Note both session ids, close the tab, reopen the
   row from the bar, and see which conversation comes back.

### Correct

- **After step 4 the row rises to the top and its status updates.** That is the
  fix working, and it is decisive: after a `/clear` the payload's session id
  names no row, so the *only* thing that can have resolved it is
  `CLAVE_AGENT_UUID` reaching the hook. **A row that goes quiet here means the
  env did not arrive, and the fix is inert.** That is the single most important
  observation in this item.
- **`title` and `summary` keep rolling** on subsequent turns, read from the new
  transcript via `payload.transcript_path`. A row that rises but whose summary
  freezes at its pre-`/clear` value means `resolve_transcript` is falling back
  to the derived path — a different bug, and one the fix explicitly guards
  against by holding the old value rather than reading the abandoned file.
- **After step 5 the row is NOT stamped by the nested run**, and
  `~/.local/state/clave-dev/state/clave.log` gains a `declined … is not the
  agent's` line naming both pids. A row whose `last_interacted` jumps when the
  nested Claude finishes means `PidGate` failed open, which is the
  ambient-authority bug the review caught before it shipped.

### Vacuous if

- **You used a seeded row.** No real spawn, so no env, so the experiment tests
  nothing. Only an `Alt+a` → `new` agent works.
- **You are running the stable `clave`, not `clave-dev`.** The stable binary
  predates this branch and sets neither variable — the row will freeze exactly
  as it does today, and that is not a finding. Check `clave-dev --version`
  carries the working-tree build tag.
- **The tab was resurrected rather than freshly spawned** between steps 3 and 4.
  Resurrection re-execs `clave spawn`, which re-sets the env — so the experiment
  still passes but proves less than you think; it no longer isolates the
  rotation path.

### What I read, and what I will not do

Ollie drives; the observability is mine to read and report:

- `clave-dev dev status` — the sandbox store as JSON. `last_interacted`,
  `status`, `title`, `summary` per row.
- `~/.local/state/clave-dev/state/clave.log` — the decline lines from step 5.
- The two transcript ids under `~/.claude/projects/<munged cwd>/`, which show
  the rotation directly.

I will not drive the session, and I will not run `zellij`, `just release` or
`just dev-install`. If step 4 fails, the next move is the instrumentation recipe
in `docs/dev/TESTING.md`, **not** another hypothesis — this branch has already
spent three review rounds on assertions that were not checked.

### If it fails

The fix degrades to today's behaviour rather than misbehaving, so a failure here
is not a rollback — it is "#97 is still open and the mechanism was wrong". The
next candidate is not another env variable: it is reading the identity from a
channel Claude cannot drop, most plausibly the pane. Record the result either
way in **FOOTGUNS.md**, since "does a hook inherit the spawner's env" is exactly
the kind of fact that costs a round to rediscover.

## 9. Resurrection comes back on the LIVE conversation (#99)

**RUN AND PASSED 2026-07-31** (PR #101, sandbox, CLI v2.1.220). Minted
`162e889f`, rotated to `02fdcf5a` on the `/clear`; the resurrected pane execed
`Resume 02fdcf5a` (evlog), answered `7272` from memory with the post-clear
history intact, and the minted transcript's mtime never moved. Two facts fell
out that were open beforehand: **a `--resume` does not rotate** (no third
transcript, `live_session` unchanged after resurrection), and the raw
`live_uuids` in `dev status` does report the rotated id, as predicted below.

**Item 8's step 6, run in the other direction.** That step measured the loss;
this one confirms it is gone. Same setup, and it is the same experiment — if you
are running item 8 anyway, carry straight on from its step 5 rather than
rebuilding the world.

Worth stating plainly because it decides the release: `just release` kills and
relaunches every session, so it puts EVERY pane through this path at once. A
regression here loses conversations across the whole fleet, silently, at upgrade
time. That is why this item exists and why a green test suite is not enough —
nothing in it can exec a real `claude --resume`.

### Do

1. From an agent you have already `/clear`ed and prompted again (item 8, steps
   3–4), say something the pre-`/clear` conversation cannot know — a number is
   easiest: *"remember the number 7272"*.
2. Note the two transcript **ids and mtimes** under
   `~/.claude/projects/<munged cwd>/`: the minted one, frozen at the clear, and
   the live one still moving. (Do not count files — that dir holds one jsonl per
   session that has ever run in this cwd.)
3. **Check `clave-dev dev status` shows this row's `live_session` set to the
   rotated id, BEFORE closing the tab.** If it is `null`, stop: the pointer the
   fix reads was never written, and everything below will reproduce the pre-fix
   answer for an item-8 reason. This is the step that makes the rest mean
   something.
4. Close the tab (`Alt+w`) and reopen the row from the bar.
5. Ask the resurrected agent, with no tool access, what number it was told.

### Correct

- **It answers 7272.** The pane came back on the post-`/clear` conversation,
  which is the entire fix.
- **The LIVE transcript is the one that grows.** Compare mtimes against step 2
  after step **5**, not after the reopen — resuming writes nothing, so both
  files sit still until the agent actually answers. The rotated file's mtime
  must move and the minted one's must not. A minted file that gets appended to
  means the exec targeted the wrong id: the pre-fix behaviour exactly.
- **`clave-dev dev status` still keys the row on the MINTED uuid**, and its
  `live_session` names the rotated id. The row's identity must not follow the
  conversation; if the store key moved, binds and the tab timeline moved with
  it, which is a worse bug than the one being fixed. Note its top-level
  `live_uuids` is the RAW dump scan and is deliberately not translated through
  the store — a rotated id appearing there is expected, not a finding.
- **The row's `summary` rolls forward, not back.** After step 5 it describes the
  new exchange; a summary that reverts to a pre-`/clear` `ai-title` is the
  pre-fix symptom. Nothing moves before step 5 — `title` and `summary` only roll
  on `UserPromptSubmit` and `Stop`, and resurrection fires neither.

### Vacuous if

- **The agent never rotated.** No `/clear` between spawn and resurrection means
  minted == live and every path agrees — the run proves nothing. The check is
  the two ids from step 2, not the file count.
- **`live_session` was never written** (step 3 said `null`). The row's live
  pointer is what the fix reads; without it resurrection correctly falls back to
  the minted uuid, so you would reproduce the pre-fix answer while testing #97,
  not #99. It is written only by a hook that fired AFTER the clear and passed
  the pid gate.
- **You asked the resurrected agent to *check* the number** (grep, a file, its
  own transcript). It will find it and answer correctly from the wrong
  conversation. Ask it what it REMEMBERS, with no tools.
- **The store was wiped between the clear and the resurrection.** `live_session`
  lives in the store, so a wipe degrades this to the pre-fix path by design.

## What this checklist CANNOT test

So that absence of a finding is not read as absence of a problem:

- **Steps above `MAX_LEARNABLE_STEP`.** Real resize steps over 20 livelock the
  seek in a re-arm/resize storm — 111,788 configurations on this branch, 50,576 on
  `main`. It needs a display area around **400 columns**; a ~280-column window
  cannot produce one. The proptest generator stops at 20, so nothing automated
  will ever catch it either. Out of reach today, **not out of reach forever** —
  a wider monitor or a projector re-opens it.
- **The two surviving `Rgb::hex` mutants.** `hex()`'s only caller is
  `bar-preview.rs`, an excluded example, so it is unreachable from the plugin. No
  live observation can exercise it; the honest options remain a test or a
  deletion.
- **Ink stability across looks.** Allocation is provisional, in-memory and
  positional over the sorted repo set, so adding a repo that sorts early
  renumbers every repo after it. **Colours shifting between two runs is not a
  renderer bug** (Gate-1 prediction 3); it is what S5's store-backed allocator
  exists to fix.
- **A mixed-version store.** An older `clave` binary's read-modify-write strips
  `title`/`summary`. `summary` self-heals on the next turn because `ai-title` is
  re-stamped; `title` heals only while a `custom-title` line is still inside the
  64 KiB tail, so on a long-running session a stripped title is lost until the
  next `/rename`. Reproducing this needs two binaries and is not part of this run.
- **Everything Tier 2 will own (#47).** One bar per tab, nav after a close, pipe
  delivery, bind/prune round-trips: this checklist observes them, but a human
  observing once is not a regression guard.
- **Glyph and font coverage** is `host-untestable` by definition. What you can do
  is record *which* glyph was missing, on which font, so the design question is
  separable from the logic question.

## What to do with a finding

- **A prediction turning out wrong is a finding, not a disappointment.** D26's
  four reservations and the Gate 1 list were written down *in order to be
  falsified* here. "Collapsed rested at 33, not 30" and "the newborn healed in one
  step, not three" are both results. Record the number either way.
- **A ruling or a design answer → `docs/ux/LEDGER.md`**, as the next numbered
  decision, dated, with its reasoning. A decision that is not yet in the code
  carries the `NOT YET IMPLEMENTED` banner.
- **A trap the next agent would also lose time to → `FOOTGUNS.md`**, in the
  section it belongs to, with the mechanism and the diagnosis command.
- **Work → an issue**, and note in it which tier should have caught it. If the
  answer is "a tier that exists", `docs/dev/TESTING.md`'s taxonomy or escape
  record wants a row.
- **Noise that is not a finding:** `CliPipe did not complete within 1s`, empty
  payload deliveries, and `1000 consecutive unknown messages` in the zellij log
  are benign and pre-existing since v0.1.0 (#45). They buried the real evidence
  once already. `not found, starting it instead` in the same log is the opposite —
  that one is always a finding.

## Observation log

Fill it in as you go; the numbers are the deliverable, not the ticks.

| | observed |
|---|---|
| Window width (columns), and derived step | |
| Expanded rest width / summary cells | |
| Collapsed rest width / summary cells | |
| Toggles driven, toggles that moved the pane | |
| Newborn birth width, steps to rest (wide window) | |
| Newborn birth width, over-run seen? (narrow window) | |
| Chip filled after `/rename` + one prompt? | |
| Summary tier observed at each step | |
| Provenance: blank / branch / worktree all correct? | |
| New row with a non-`main` discoverable default → blank? | |
| Selection never opened; `Alt+Enter` did; the ✗ row refused the commit | |
| Resize healed unaided / healed on next event / parked | |
| Terminal row: console mark, name, sort position | |
| One bar per tab, one build tag | |
| **#97** After `/clear`, the row still rises and its status updates | |
| **#97** `title`/`summary` keep rolling from the NEW transcript | |
| **#97** A nested `claude -p` does NOT stamp the row; decline logged | |
| **#99** Which conversation a resurrected rotated tab comes back on | |
