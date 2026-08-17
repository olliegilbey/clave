# Live interaction checklist — the sidebar at 54 columns

> **Partially run 2026-07-30, then invalidated twice.** Item 1 passed decisively
> — six-plus toggles across three display widths, the pane moved every time.
> Item 5 surfaced the wrap bug that became D31. **Everything numeric was then
> superseded**: D33 took expanded 44 → 54, and D35 changed how the bar is born.
>
> **Then #181 deleted the width seek outright (LEDGER D39), and that is what
> this run now validates.** Both widths are declared in the layout as swap
> layouts and zellij switches between them; nothing measures a width, decides it
> is wrong and resizes. So there is no seek to settle, no acceptance band, no
> learned step and no drift re-arm — every item that used to ask you to wait for
> convergence now asks you to confirm a single instant change. Two things about
> that mechanism were called unverifiable here — **that a swap re-uses the
> running bar rather than starting a second one, and which tab a plugin's switch
> lands on**. The #197 review read `zellij-server` 0.44.3 and settled both from
> source: the swap DOES re-use the running pane, and the switch lands on the
> FOCUSED tab, not the asking one. Items 1, 2 and 6 still carry them, now as
> confirmation rather than discovery — and **item 10 is what the source could
> not settle**, five edges of the switch that only a live run can answer.

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
  nothing to do with the geometry. Checked in setup step S5.
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
below is derived from it. **There is no resize step any more.** Since D39 the two
widths are whole percentages declared in the layout, so each one is
`floor(percent × W / 100)` and it is the same number every time — there is
nothing to converge to and no lattice to land on.

Both percentages are `round(target × 100 / W)`, chosen when the layout was
written. Predicted widths (not yet confirmed live — that is what this run is
for):

| W | expanded % | expanded (summary) | collapsed % | collapsed (summary) |
|---|---|---|---|---|
| 120 | 45 | 54 (22) | 25 | 30 (7) |
| 160 | 34 | 54 (22) | 19 | 30 (7) |
| 200 | 27 | 54 (22) | 15 | 30 (7) |
| 240 | 23 | 55 (23) | 13 | 31 (8) |
| **280** | 19 | **53 (21)** | 11 | **30 (7)** |
| 320 | 17 | 54 (22) | 9 | 28 (5) |
| 400 | 14 | 56 (24) | 8 | 32 (9) |

54 is not exactly expressible as a whole percent of most displays — at 280 one
percent is 2.8 columns — so 53 rather than 54 is correct, not a defect. Nothing
closes that column any more, which is the point of D39.

**The percentages are baked at LAYOUT-WRITE time, not at resize time.** Every
number above assumes the window is the width it was when the session launched.
Resize the window afterwards and both geometries shrink with it and stay shrunk —
a known and accepted loss (D39, "what is given up"), not a finding. If the
maintainer wants that changed it is a design decision, not a bug report.

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

Every dormant row shows the dormant mark **U+25CC** whatever its stored status:
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

This is where a real bug lived (D21) and was fixed (D26), and it is now where the
whole of D39 gets its only live proof. `Alt+c` is one `next_swap_layout()` call
and nothing else: no arithmetic, no watching, and a second call only if zellij's
own report says the tab is still in the wrong layout (D40).

> **This item PASSED on the old machinery, 2026-07-30** — six-plus toggles across
> three display widths, clean every time. That result does not carry: the
> mechanism underneath it was replaced. Everything here is unconfirmed.

**Do.** Press `Alt+c`, look, press it again. Repeat for **at least six
consecutive toggles**, one at a time, and count. Then do it again in a
**background** tab's view: switch to another tab, press `Alt+c`, and check what
moved.

**Correct.**

- **The pane moves on every single toggle, instantly.** Six presses, six width
  changes, each one arriving in a single step. Any animation, pacing or
  crawl-toward-a-number is a finding — that shape belonged to the deleted seek.
- **Exactly one bar in the tab afterwards.** A swap layout re-instantiating the
  plugin instead of re-using the running one is the #1 unverifiable in D39, and
  a second bar invalidates the whole run (see the global vacuity conditions).
- **The switch lands on the tab you are looking at, and on no other.** After the
  background-tab pass, visit the other tabs: their widths must be untouched.
  This is D39's #2 unverifiable — zellij resolves a plugin's swap request against
  the FOCUSED tab, and the bar is gated to only ask while its own tab is focused.
- The rows render the collapsed profile: repo 3 characters with **no ellipsis**
  (D18) and the summary simply shorter. A row wrapping onto a second line is a
  hard finding (D31).
- **The title chip REFLOWS, 9 → 7, and that is correct** (D33). Titles of 7
  characters or fewer look identical either way, so on the `ux-gate1` fleet you
  may not see it at all.
- **Record the two widths.** At W = 280 the derivation predicts **53 expanded and
  30 collapsed** — summary cells of **21 and 7**. 53, not 54, is correct: 54/280
  is 19.29% and a KDL size is a whole percent.

**Vacuous if.**

- **You launched the session before this branch.** Both geometries are baked into
  the layout file at session-create time. A session started with an older binary
  has no swap layouts at all and `Alt+c` will do nothing. Relaunch.
- You resized the window since launching. The percentages were computed for the
  width at launch, so both widths will be off and neither number above applies.
- You navigated during the test. A nav onto a dormant row arms a **peek**, which
  switches to the expanded geometry for ~0.9 s even while collapsed. Keep hands
  off the arrow keys for this item.
- You watched a bar in a **non-focused** tab expecting it to move on its own. A
  background bar deliberately issues nothing; it switches when its tab is next
  looked at.

## 2. A new tab's first paint

**This item pins the ABSENCE of birth jank, and it now has to do it three
times** — because a tab can be created three ways and until this branch only one
of them was sized correctly.

**Do (a), a plain tab.** `Alt+t`. Watch the new tab's own bar on its first
paints.

**Correct (a).** At W = 280 the bar is born at **53** and stays there — zero
visible resizes. Any shrink-then-settle is a finding: it means the percent
reaching the layout is not the one derived from the window.

**Do (b), an agent tab from the picker.** `Alt+a`, pick a directory, choose
`new`. Watch the new tab's bar.

**Correct (b).** The same **53**, indistinguishable from (a). **This is the
regression this branch fixed and it is the highest-value observation on the
page**: `clave add` used to hand the layout no width at all and got the
200-column fiction, which on a 280-column display is **75** — visibly and
permanently a third too wide. It now asks the session for the window. If you see
75, the read failed and it silently fell back; say so.

**Do (c), a dwell-opened tab.** Select a dormant row and press `Alt+Enter`.

**Correct (c).** The same **53** again. All three paths, one width.

**Do (d), collapsed.** `Alt+c`, then repeat any one of the above.

**Correct (d).** The new tab is born **collapsed**, at 30 — not born expanded and
then snapping. A visible wide-then-narrow flash on birth is a finding.

**Vacuous if.**

- **You launched the session before this branch.** Percentages and swap layouts
  are baked into `launch.kdl` at session-create time. Relaunch, do not
  hot-reload.
- **You resized the terminal after launching.** Every number above assumes the
  launch width; a later tab is sized against the window as it is now, so (a) and
  (b) can legitimately differ from each other. Do not run this item after a
  resize.
- You watched the *old* tab's bar. Each of these focuses the new tab; the newborn
  is the bar in it.

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

## 6. Window resize — the accepted loss, seen once

**This item no longer tests a mechanism, because there is no longer one.** The
bar is a percentage of the window, so it shrinks with the window and nothing
brings it back. The purpose of the item is now to let the maintainer **look at
the loss and decide whether he can live with it** (LEDGER D39, "what is given
up"). Nothing here is a bug report.

**Do.** With the bar settled and expanded, resize the terminal window **by a
lot** — halve its width, from ≈280 to ≈140 — then leave it alone and watch. Then
widen it again.

**Correct — meaning "as designed", not "good".**

- The bar's columns fall proportionally, roughly **53 → 26**, and **stay there**.
  Nothing grows it back. Widening restores it proportionally, and also stays.
- 26 is under `EXPANDED`'s `min_intact_cols()` floor (32 since #105), so expect a
  uniform CLIP — D31 — with every row cut at the same column. **Ragged** clipping,
  rows disagreeing on width, is a real finding.
- The bar must be visually still throughout: nothing on screen moves except
  proportionally with the window. One **invisible** self-ask is expected and
  by design: the snap-back arm cannot tell a window resize from a drag, so it
  spends one switch at the new width — a re-apply that lands on the width the
  pane already has. Visible movement, or the width walking anywhere the
  window did not take it, is a real finding.
- `Alt+c` still works at the new size and still moves the pane, and the collapsed
  geometry is now proportionally narrower too — on a halved window it can be
  narrower than the expanded bar was. That is arithmetic, not a defect.

**Record the judgement, not just the numbers.** Is a bar that shrinks and stays
shrunk acceptable? If not, the fix is a design decision with a real cost — it
means reacting to window changes and issuing corrections, which is the pattern
D39 deleted — and it belongs in the ledger as a new entry, not in an issue as a
regression.

**Vacuous if.**

- You resized by a little. The percentage is unchanged, so a small resize moves
  the bar by a column or none at all and shows you nothing.
- You were still dragging. Let go, then look.
- You relaunched the session after resizing. A relaunch re-derives both
  percentages from the new width, which is exactly the behaviour that still
  works — and it hides the loss this item exists to show.

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

## 10. The width switch's five edges (#197 review)

These come out of the review of the swap-layout width switch, and they exist
because reading zellij's source answered each of them only halfway. **The switch
cycles through three positions, not the two we declare** — zellij hides the
tab's own birth layout ahead of them — so a switch can land somewhere neither
geometry asked for. **A tab zellij considers damaged** (a pane resized or
closed, a border dragged with the mouse) **spends its next switch re-applying
rather than advancing**, which is a way to get no movement at all.

Since the 2026-08-17 rebuild the bar answers both from its PAINTED width: the
two geometries are declared as fixed column counts (54 expanded, 30 collapsed),
so the bar compares the width zellij paints it at against the constant its
mode wants and asks for one switch on a mismatch — then goes DEAF until a
0.15s cooldown judges the latest paint once (a swap's queued repaints echo the
pre-swap width; judging them was the filmed infinite toggle loop), spending at
most three asks per mode-intent (the cycle is three positions long) before it
rests until the intent changes.
The layout name zellij reports is read by NOTHING: zellij only computes it for
tabs with two or more selectable panes, which no clave tab has (FOOTGUNS). And
a border drag is REFUSED by zellij itself — fixed panes cannot be resized from
either side — so snap-back is enforced at the source rather than corrected
after. Everything below is a question about what that looks like from the
chair. None of them has an automated test that could reach it.

### 10a — Cold start into collapse

**Do.** Collapse the sidebar (`Alt+c`), quit the session, relaunch it (S1–S3).
Watch the bar on its very first paints, before touching anything. Then press
`Alt+c` once.

**Correct.** The bar is **born collapsed at 30 and never widens** — no frame at
53, no snap back, and since the 2026-08-17 rebuild no switch at all: the launch
layout carries the collapsed width as a fixed column count, the machine compares
the painted 30 against the same constant, and they agree from the first paint.
A frame at 53 means the launch layout was composed against the wrong store
flag. And the first `Alt+c` after the launch must move the pane, in one press.

**Vacuous if.** The store was not collapsed at kill time — check
`clave dev status | jq .store.collapsed` is `true` before relaunching. Or you
launched by hand instead of using the printed launch command: the fallback
layout has no store to read and always births expanded, which is a known and
accepted wrong, not this finding. Or you blinked — the flash is about one frame,
so repeat it three times before recording a pass.

### 10b — Six slow toggles, then six fast ones

**Do.** On a tab you have not touched since it was born — no pane resized, none
closed, no border dragged — press `Alt+c` six times, several seconds apart, and
**record the column width after each press**. Then press it six times as fast as
you can and record where it comes to rest.

**Correct.** Six presses, six clean moves, alternating 53 / 30 / 53 / 30 / 53 /
30. One press in three walks the cycle through the tab's hidden birth position,
so a brief second step is possible; landing wrong and staying wrong is not.

The fast run is the #197 regression: a rapid burst used to be able to desync the
bar from its tab **permanently** — the pane stuck at one width while the store
said the other, unrecoverable by further presses, by switching tabs, or by
waiting. Whatever the burst leaves on screen, the width and the drawn profile
must agree once it settles, and one further press must move the pane.

**Vacuous if.** You navigated during the run: a peek expands the bar for ~0.9 s
and hides the answer. Or the tab was already damaged when you started, which
moves the awkward press somewhere else — use a tab you have just opened.

### 10c — A dragged pane border is refused outright

**Do.** Try to drag the border between the sidebar and the workspace with the
mouse, in both directions. Then press `Alt+c` once and confirm the toggle
still works.

**Correct.** The border **does not move at all** — the bar pane is fixed-width
(2026-08-17 rebuild), and zellij refuses resizes touching a fixed pane from
either side. This is the 2026-08-15 snap-back ruling enforced at the source:
there is no drag to undo, so there is no correction arm to watch. Zellij may
flash a "FIXED!" notice on the pane — record whether it does and how it looks,
that is the one unknown here. The `Alt+c` after the attempt must move the pane
normally in one press (the drag attempt must not have damaged the tab into
eating a switch — if it did, the press after that one must move it, and that
is a finding to record).

Record which you got: border immobile (correct), border moved at all
(finding — the pane was not emitted fixed; check the generated layout), or a
moved border that snapped back (finding — same, plus the corrector fired).

### 10d — A floating pane over the bar

**Do.** Open the directory picker (`Alt+a`), which is a floating pane. With it
still open, press `Alt+c`. Then close it with Esc and look at the bar.

**Correct.** The question is **not** whether the pane moved while the picker was
up — it is whether the bar's drawing matches its width once the picker is gone.
A collapsed profile drawn into a 53-column pane, or an expanded profile crammed
into 30, is the finding: the mode changed and the geometry did not follow.
Record both numbers — the width, and which profile is drawn. Known, and
deliberately not fixed on this branch: the switch may simply do nothing at all
while a floating pane is up. Say which happened.

**Vacuous if.** You picked a directory or created an agent — that focuses a new
tab, so the bar you are reading is a different one. Cancel with Esc. Or you
toggled after closing the picker rather than during, which is item 1's test.

### 10e — A mouse tab-switch, then a toggle

**Do.** Click another tab on **zellij's own tab strip** at the top of the window
— not a row in the clave sidebar — then press `Alt+c`. Afterwards check at least
two other tabs' widths.

**Correct.** The sidebar that moves is the one in the tab you just clicked into,
and **no other tab's width changes at all**. Zellij resolves a plugin's switch
request against whichever tab is focused rather than the one that asked, and the
bar protects against that by only asking while it believes its own tab is
focused — so a focus change arriving by a route the bar does not hear about is
exactly how the wrong tab gets resized. If a background tab's sidebar moved,
record which one and how you got there.

**Vacuous if.** You switched tabs with `Alt+o` or by clicking a sidebar row.
Both are routes the bar already hears about; the zellij tab strip is the whole
point of this item. Or you toggled before the new tab had finished painting.

## What this checklist CANNOT test

So that absence of a finding is not read as absence of a problem:

- **Whether a swap layout re-uses the running bar.** Settled from
  `zellij-server` 0.44.3 source in the #197 review — the applier matches existing
  panes to the new layout's slots before creating anything — but that is a read of
  one version, and item 1 still observes it on one machine. One clean observation
  plus one source read is the best evidence available, and D39 says so.
- **A window resized while a tab is in the background.** Both geometries are
  percentages, so nothing is watching and nothing needs to be, but the interaction
  between a background tab's stale geometry and the next switch has been reasoned
  about rather than seen.
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

- **A prediction turning out wrong is a finding, not a disappointment.** Every
  number on this page was written down *in order to be falsified* here.
  "Collapsed came out at 33, not 30" and "the Alt+a tab was born at 75" are both
  results. Record the number either way.
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
| Window width (columns) at launch | |
| Expanded width / summary cells | |
| Collapsed width / summary cells | |
| Toggles driven, toggles that moved the pane | |
| One bar per tab after six toggles? | |
| A toggle in one tab left every other tab's width alone? | |
| Birth width: `Alt+t` / `Alt+a` / `Alt+Enter` — all three equal? | |
| Birth width while collapsed: born collapsed, no flash? | |
| Resize: width after halving, and did anything move it back? | |
| Resize: is the shrink-and-stay acceptable? (maintainer's call) | |
| **10a** Collapsed cold start: born at 30, no flash to 53? | |
| **10a** First `Alt+c` after that launch moved the pane? | |
| **10b** Six slow widths in order? | |
| **10b** Six FAST presses: settled width, profile agrees, next press moves? | |
| **10c** Border drag refused (immobile)? FIXED! flash — how does it look? | |
| **10c** `Alt+c` after the drag attempt moved in one press? | |
| **10d** With the picker open: did it move? And which profile at which width after Esc? | |
| **10e** After a zellij tab-strip click: which tab's sidebar moved? | |
| Chip filled after `/rename` + one prompt? | |
| Summary tier observed at each step | |
| Provenance: blank / branch / worktree all correct? | |
| New row with a non-`main` discoverable default → blank? | |
| Selection never opened; `Alt+Enter` did; the ✗ row refused the commit | |
| Terminal row: console mark, name, sort position | |
| One bar per tab, one build tag | |
| **#97** After `/clear`, the row still rises and its status updates | |
| **#97** `title`/`summary` keep rolling from the NEW transcript | |
| **#97** A nested `claude -p` does NOT stamp the row; decline logged | |
| **#99** Which conversation a resurrected rotated tab comes back on | |
