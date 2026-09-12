# Four-line card — locked

_Ratified 2026-09-08 by the maintainer ("that's the one, let's lock it in"),
from rendered cards rather than from prose, across nine design rounds. Revises
the [2026-08-26 double-height card lock](2026-08-26-double-height-card-lock.md)
for **card mode**, which is the default. That document stays authoritative for
everything this one does not overturn, and the 2026-07-25 sidebar lock still
governs the single-line row reachable with `clave rows single`._

Run the design: `cargo run -p clave-bar --example triple-preview`
Watch it move: `cargo run -p clave-bar --example triple-preview -- --animate`

**Vocabulary:** _card, row, gutter, cell, ink, chip, provenance, glass, fade_
are defined in [UBIQUITOUS_LANGUAGE.md](../../UBIQUITOUS_LANGUAGE.md). Three
terms are new here and defined in §7: **rail**, **shadow rule**, **crop rule**.

**Source-of-truth hierarchy.** _This document is authoritative_ for every
ruling, number and rationale — with the standing exception recorded in §4.4,
which the drive overturned and this file was amended to match.
`crates/clave-bar/examples/triple-preview.rs` is the illustration and nothing
more: the geometry now lives in `card.rs` and the example renders THROUGH
`render_rows`, so the two can no longer disagree by drifting. Where this file
and the code disagree, read it as a spec amendment owed, not a bug filed — the
card shipped and was driven, and §9 records what settled each ruling.

---

## 1. The card

```text
  line 1:  status  │ chip-pill  summary
  line 2:  prov    │ repo [branch]  #PR   provider model effort
  line 3:  subs    │ tokens  clock  wants
  line 4:  the shadow rule — a hairline that closes the card
```

**Three lines of content, one of separation.** Three dense lines butted against
the next three read fine one card at a time and stop being _glanceable_ as a
fleet: the eye cannot find where one card ends. The fourth line fixes that, and
the fleet halves on screen to pay for it — twelve live cards, the maintainer's
working ceiling, still fit a laptop panel and only an iPad in sidecar is tight.

**The three lines are three categories.** Line 1 is _who and what_. Line 2 is
_which checkout, which model_. Line 3 is _how much, how long, what it needs_.
Nothing crosses lines: a cell with no value renders blank in its own line's
budget, and no cell reflows into another's.

**Two profiles, one geometry. Collapsed is 16 columns, expanded 48.** Collapsed
drops from the double-height card's 38 because a card still showing the model,
the branch and two clocks is not collapsed, it is narrow. Both numbers live in
`RowHeight::target_cols`, and the card reads its profile off the width it is
actually painted at.

## 2. Line 1 — the cells

| Cell    | Cells                 | Content                                                            | Ink                             |
| ------- | --------------------- | ------------------------------------------------------------------ | ------------------------------- |
| status  | 3 (` X `)             | the status mark, animated while working (§4.5)                     | the state's fixed semantic ink  |
| rail    | 2 (`│ `)              | the card's identity spine (§4.1)                                   | the repo's ink                  |
| pill    | 9 (cap + **7** + cap) | the rename you gave the tab; the tab name on a terminal card        | title ink ground, chip ink text |
| gap     | 1                     | the pill's trailing air — **absent on a chipless card**             | —                               |
| summary | flex                  | the agent's description of the session; the last command on a term. | default ink                     |
| margin  | 1                     | —                                                                  | —                               |

The summary is the only flexing cell: **`cols - 16`** with a pill (32 expanded,
0 collapsed), **`cols - 6`** without one (42 expanded, 10 collapsed).

**A chipless card loses the gap as well as the pill.** The gap is the pill's
trailing air, not an indent; with no pill there is nothing to hold air away
from, and the space read as a one-cell stagger against the repo and the token
count directly below it. Ratified round 9, on sight.

**A summary budget under 4 renders blank.** At 16 columns with a pill the pill
_is_ the line, and a lone `…` after it says less than an empty cell.

## 3. Line 2 — the cells

| Cell     | Cells       | Content                                | Ink                        |
| -------- | ----------- | -------------------------------------- | -------------------------- |
| prov     | 3 (` X `)   | the provenance glyph (§4.2)            | **fixed and semantic**     |
| rail     | 2 (`│ `)    | the spine                              | the repo's ink             |
| repo     | ≤ **9**     | the checkout's directory name          | the repo's ink             |
| branch   | flex, ≥ **9** | the branch name — expanded only      | meta ink                   |
| PR       | 1 + **5**   | `#204` — expanded only, only if open   | PR ink                     |
| provider | 1 + 1       | the vendor glyph — expanded only       | the vendor's brand ink     |
| model    | 1 + **6**   | `opus`, `gpt-5` — expanded only        | meta ink                   |
| effort   | 1 + **2**   | `hi`, `xh` — expanded only             | meta ink                   |
| margin   | 1           | —                                      | —                          |

Repo and branch share **one collective budget**: `cols - (5 + pr + tail + 1)`,
where `tail` is 12 expanded and 0 collapsed, and `pr` is 6 only when a PR is
open. That is 30 expanded without a PR, 24 with one, and 10 collapsed. The repo
takes up to 9 and the branch claims the remainder, so **a long repo truncates
before a branch does**.

**The branch renders only where it can carry `BRANCH_MIN` = 9 cells.** A
five-cell fragment of `fix/evlog-…` identifies nothing, and those columns are
worth more to the repo name, which at least names the checkout. Below the
minimum the branch does not render at all and the repo takes the whole budget.

**A card with no PR folds its six cells back into repo-and-branch.** Nothing is
reserved for an absence.

## 4. Ratified decisions

### 4.1 The rail is the repo's colour

A single `│` running the card's full height — one continuous identity spine per
card, in the same ink as the repo name it sits beside.

It replaced the double-height card's arc (`╭`/`╰`), whose two neutrals
alternated card by card. That zebra was linework doing a separator's job,
because glass forbids a painted stripe. Line 4 does that job properly, so the
zebra retired with the arc.

### 4.2 Provenance keeps its own fixed inks

The rail carries the repo's identity now, so the provenance glyph is free to say
what _kind_ of checkout this is, and to say it the same way on every card.

| Provenance | Glyph                             | Ink                        |
| ---------- | --------------------------------- | -------------------------- |
| worktree   | `\u{f1bb}` `fa-tree`              | `#98BB6C` kanagawa springGreen |
| branch     | `\u{f062c}` `md-source_branch`    | `#957FB8` kanagawa oniViolet   |
| main       | nothing                           | —                          |

**The worktree glyph changed, and the table above is what ships.** Until this
lock the card drew `\u{168c2}`, a Bamum letter, taken knowingly because no
surveyed icon set contains a worktree glyph at all (already in FOOTGUNS.md).
What had not been costed is that it arrives by fallback out of a proportional
historic-script face and so renders **off-centre in its cell** — tolerable
beside one other glyph, not once a third joins the column. It was replaced by
`fa-tree` `\u{f1bb}`, a metaphor rather than a depiction, which is in the font;
`render::WORKTREE_MARK` carries it. See §6.

### 4.3 ~~The two clocks stay apart~~ — REVERSED, see §4.4

> **Amended 2026-09-09.** There are not two clocks. §4.4 records why, and the
> rest of this section is kept because the reasoning below is still the reason
> two clocks would have been wrong — it just turned out they could not exist.
> The layout that shipped is one clock in the LEFT slot beside the token count,
> with `wants` running from it to the right edge.

Adjacent, `20h 20m` reads as one duration — twenty hours and twenty minutes —
rather than as two numbers. Different inks do not save it; only distance does.
So elapsed takes the left slot beside the token count and the turn clock takes
the right edge, with the `wants` message between them.

**Elapsed wins the crop** (round 8, reversing round 6). The animated mark
already says _thinking_, which makes how **long** it has been thinking the
softer fact. Time since you last touched the row is the harder one: past an hour
the prompt cache is gone and the next turn costs more. The reading that changes
what you do is the reading that survives collapse.

### 4.4 The turn clock is the only live number on the card

**AMENDED 2026-09-09, ruled from the drive.** The clock is blue (`#7E9CD8`
kanagawa crystalBlue) while a turn is in flight. It is not a *separate cell*
from `elapsed`, and it does not go blank when the turn ends — it **dims**, and
the same number changes meaning.

The original wording assumed two numbers. There is only one instant to measure.
The store bumps `last_interacted` on `UserPromptSubmit` and nothing else, and
`UserPromptSubmit` is the only event that sets `Working` — so "when the turn
began" and "when you last interacted" are the same moment, always. A separate
turn cell would have printed `elapsed`'s number two columns away, which is the
one thing §4.3 kept them apart to prevent.

What the two readings genuinely differ in is **resolution and meaning**:

| | while `Working` | once the turn is over |
|---|---|---|
| ink | crystalBlue `#7E9CD8` | dimmed meta |
| grain | seconds under a minute (`3s`, `59s`) | minutes and up (`5m`, `2h`) |
| means | how long this turn has run | how stale this row is |

The drive is what settled it: the card read `0m` for the whole of a turn Claude
Code's own footer was calling `3s`. Staleness in seconds would be noise; a turn
in minutes says nothing at all.

**It costs no store traffic and no new timer.** The bar already repaints every
0.2s while any row is thinking, and arms nothing when none is (`arm_anim`), so
the seconds tick for free at one end and an idle fleet pays nothing at the
other. The fleet is never re-pushed to animate a counter.

Dropping the separate cell returns **four columns** to `ask` (§4.7).

### 4.5 The status mark animates while working

Claude Code's own spinner, in the one cell every profile keeps — so the
collapsed card gains a heartbeat for free.

```
·  ✢  ✳  ✶  ✻  ✽  ✻  ✶  ✳  ✢
```

`\u{00b7}` `\u{2722}` `\u{2733}` `\u{2736}` `\u{273b}` `\u{273d}`, then the four
inner frames back — ten frames, ping-pong, so the cycle turns over without a
jump. **Order is the animation**: the mark grows from a dot through
progressively heavier stars and back. Any other sequence gives six glyphs
flickering rather than one shape breathing.

**A row blocked on you does not animate.** The spinner stops when Claude asks.

Frame interval **0.2s**, and that number is not free. The bar identifies which
timer fired by elapsed seconds alone, guarded by a compile-time assert chain in
`crates/clave-bar/src/main.rs`.

**Corrected 2026-09-10:** 0.2s does NOT sit in a gap. The classifier has three
bands and the fastest one already holds the width cooldown at 0.15s, so the
spinner SHARES that band rather than getting one of its own — the two cannot be
told apart by elapsed.

**Resolved the same day, without tagged timers.** The sharing is harmless in one
direction and not the other. A width expiry mistaken for a frame costs a spare
repaint. A FRAME mistaken for a width expiry ends the switch deafness early —
its remaining time is uniform in [0, 0.2) against a 0.15s cooldown, so it wins
most toggles made while a row is mid-turn — and the judgement then lands on a
pre-swap echo and spends one of the walk's three asks on it. That is a bar
resting at the wrong width, and it needs no rare timing to happen.

The fix is to stop trusting any single expiry: `swap_owed` is a COUNT, set to
two when the shell's spinner timer was armed at the instant of the ask and one
when it was not. Whichever timer the first expiry really belonged to, the second
is a full frame behind it, so the deafness always covers its cooldown. The model
asks for that second tick itself (`Effect::RearmWidthCooldown`) rather than
waiting on a frame — the spinner stops the moment the last turn ends, and an ask
that outlived it must not strand. Tagged timers remain the prerequisite for
anything FASTER than 0.15s.

### 4.6 The subagent mark is a boolean

`\u{f171a}` `md-robot_happy_outline`, in `#9CABCA` — the quiet blue-grey of a
structural mark, because it says "this row has depth", not "this row is hot". It
is the third structural glyph, stacked under status and provenance.

**A count was tried and dropped.** "This row has fanned out" is the whole
signal; the digit beside it was noise.

### 4.7 `wants` — the flexing cell, and the reason for the change

What this agent is blocked on, in its own words, in `NEEDS_YOU_INK`. It claims
every cell the numbers on line 3 do not: **31 at 48 columns** (28 until the
clock merger of §4.4 returned three columns and the wider token gap took one
back). Blank means
nothing is needed, so an idle fleet has a quiet right-hand column and the one
row that wants you grows text.

**Structure gates; the model only supplies words.** Whether a row is waiting is
decided by the free structural signal that already drives the status mark — a
fact. `wants` is written only for rows that signal already flagged, so a scribe
outage degrades to today's dot rather than to silence.

**Amended 2026-09-10: the gate is the PROJECTED status, not the wire's.** This
section said the bar draws the words straight off the wire, on the reasoning
that the host has already decided. The host has — `take_wants` keeps `wants`
exactly coextensive with `NeedsYou` — but the BAR has four states that outrank
the store's status entirely: stale, opening, dormant and dormant-selected. Each
of them left the words behind, so a dormant row with no process, a stale row
whose checkout is gone, and an opening row starting a fresh session all drew
"waiting on Bash" beside a glyph saying otherwise. The card contradicted itself
in the one cell whose whole job is to say what to do next. The words now render
only where the projection says `NeedsYou`, the same `status`-not-`a.status`
distinction §4.4's clock already makes.

**Its twin, the same day.** §4.6's subagent mark HOLDS on a silent tail, which
is right — an older Claude Code never wrote the field — but nothing ended the
hold. A `SessionEnd` now clears it (`take_subagents`): a session that has
exited can have nothing pending under it, and a held mark would sit on a
dormant row claiming depth the user cannot go and look at.

Its sources, cheapest first:

1. **The tool name from a pending permission prompt.** Free and structural:
   `crates/clave/src/hook.rs` already receives `Claude needs your permission to
   use Bash` as a `Notification` and throws the tool name away. Ships without a
   model.
2. **The agent's closing question, shortened.** The last assistant text block is
   always present, always inside clave's 64KB transcript tail, and carries the
   ask verbatim — but it needs the scribe to shorten it.
3. **Blank.**

### 4.8 The live headline replaces the AI title, it does not sit beside it

There is one summary field, not two. `hook.rs` already runs a never-regress
ratchet of recap → ai-title → first prompt; the live headline becomes a new top
tier of that same ratchet. No second slot on the card.

### 4.9 The PR is identity, so it lives on line 2

Round 5 moved it off line 3. It answers _this branch has a PR open_, which is
the same question the repo and the branch answer; among line 3's measurements it
had nothing to do with its neighbours, and it punched a reserved-but-empty hole
through them on every row without one.

### 4.10 No cost figure on the card

Ruled out at the first inventory. The token count is the burn signal; a dollar
figure is a different question, asked less often, and it would cost columns the
`wants` message needs more.

## 5. Line 3, line 4, and the crop rule

### 5.1 Line 3 — the cells

| Cell    | Cells       | Content                                | Ink                    |
| ------- | ----------- | -------------------------------------- | ---------------------- |
| subs    | 3 (` X `)   | the subagent mark, boolean (§4.6)      | `#9CABCA`              |
| rail    | 2 (`│ `)    | the spine                              | the repo's ink         |
| tokens  | **4**       | thousands of tokens (`211k`, `1.1m`)   | the battery ramp band  |
| gap     | **3**       | —                                      | —                      |
| clock   | **3**, right-aligned | ONE cell, two resolutions (§4.4) | crystalBlue while `Working`, else meta ink |
| wants   | 1 + flex    | §4.7 — expanded only                   | needs-you ink          |
| margin  | 1           | —                                      | —                      |

**The token gap is three cells, not one.** At one, `211k 20m` reads as a single
figure — the same failure that kept the two clocks apart. Two was the ruling
until the card was driven; three is what actually puts the expanded clock in the
columns the COLLAPSED card holds it in, and the collapsed position is the ruled
one (§5.3). The number is load-bearing, not taste.

**Collapsed locks the clock to the right edge**, one cell in, computed from what
the line has already spent rather than from a second margin constant. With one
clock this is no longer about which of two is rightmost: it is the crop rule
applied to a cell that survives, so the same columns hold the duration in both
profiles and the eye finds it in one place.

### 5.2 Line 4 — the shadow rule

`\u{2500}` box light horizontal, full width, in `#2A2A37` kanagawa sumiInk4 —
a rule so dark against the bar's sumiInk3 that it reads as a **shadow under the
card** rather than as a border between two of them. Four glyphs and three
intensities were rendered as a swatch and picked by eye; how thin "thin" looks
is a property of the font and the panel, not of the codepoint, so the swatch
stays in the preview.

**It is drawn outside the selection bar.** The separator rhythm stays unbroken
across the whole fleet, and the selection is exactly the three lines that carry
content.

### 5.3 The crop rule

**Collapsed is a left-crop of expanded, never a re-arrangement.** A cell's
_column is its priority ranking_, and any argument about ordering is an argument
about what survives 16 columns.

What survives: the status mark, the chip, the repo, the token count, the time
since you last touched the row. What does not: the summary (with a pill), the
branch, the PR, the provider, the model, the effort tag, and the `wants`
message. The clock survives — it is the same cell as "the time since you last
touched the row" above, at whichever resolution applies (§4.4).

This rule settles orderings that were otherwise a matter of taste, and it is why
the most useful reading on every line sits at the far left.

### 5.4 The glyph rule — load-bearing, carried forward

Every glyph in the source is a `\u{...}` escape, never a literal character.
During the double-height rounds literal glyphs were silently lost in transit
twice. Unchanged from the 2026-08-26 lock.

## 6. Verify codepoints against the installed font, not a cheat sheet

Two glyphs proposed during these rounds do not exist at the codepoints every
published Nerd Font cheat sheet gives for them, and both would have shipped as
tofu:

| Claimed                | Truth                                                                       |
| ---------------------- | --------------------------------------------------------------------------- |
| `cod-robot` `\u{f544}` | there is no `cod-robot`; the codicon set's nearest is `cod-hubot` `\u{eb08}` |
| `fa-robot` `\u{f0e9c}` | `fa-robot` is `\u{0ee0d}`                                                     |

Enumerate the real `cmap` of the font you will actually render with before
committing a glyph — both of them, since there are two: the terminal's own
face, and `JetBrainsMonoNerdFontMono-Regular.ttf`, which is the default the
asset workflow pins and traces outlines from (`examples/readme-assets.rs`). And a fallback glyph brings its **own metrics**: it
holds its cell if the fallback face is monospace, but weight and baseline do
not, and neither shows in a static render or a golden. That is what caught the
old worktree glyph (§4.2), and it is why five of the six spinner frames — which
also come by fallback, out of Menlo — need an eyeball on the real terminal
rather than a signed-off screenshot. All three entries are in
[FOOTGUNS.md](../../FOOTGUNS.md) § Text, glyphs, rendering.

## 7. New vocabulary

- **rail** — the full-height `│` in the repo's ink, one per card. Replaces the
  arc.
- **shadow rule** — line 4's hairline. A shadow under a card, not a border
  between two.
- **crop rule** — collapsed is a left-crop of expanded. Column = priority.

## 8. Rejected paths — do not resurrect

- **A cost figure on the card** — §4.10.
- **A subagent count** — §4.6. The boolean is the whole signal.
- **The alternating arc / zebra** — §4.1. Line 4 separates cards now.
- **A four-cell gutter**, made by butting the rail against the mark column —
  rejected on sight, the rule sat on top of the glyphs. The fifth column goes
  back into the gap and collapsed still lands exactly on 16.
- **Indenting lines 2 and 3 to the pill's label** rather than its cap — tried
  and rejected by eye. The pill is a solid block of colour, so the edge the
  column reads against is its outer edge, not the first letter inside it.
- **A leading space on the chipless summary** — §2. It staggered the card.
- **The turn clock winning the crop** — §4.3, reversed at round 8, then made
  moot: §4.4's merger left one clock, and it survives the crop.
- **The PR on line 3** — §4.9.
- **The two clocks adjacent** — §4.3.
- **Collapsed at 38 columns** — §1. Not collapsed, just narrow.
- ~~**Dimming the turn clock when no turn is running**~~ — **this is what
  shipped.** Forbidden while the clock was a cell of its own, where blank was
  the card's word for "no reading". §4.4's merger changed the question: the cell
  always holds a real number, so the choice was never blank-vs-dim but
  lit-vs-dim, and dim is right.

## 9. What the port cost — closed 2026-09-10

All four steps are done and all three blank cells are wired. Kept as the record
of what each step actually settled, since three of the five amendments above
came out of doing them rather than out of planning them.

1. **`RowHeight::Card`** — `lines_per_row() == 4`, `target_cols` of 48 / 16,
   launch-baked into the KDL as predicted. It grew a fifth caller nobody
   planned: `RowHeight::animates()`, because §4.4's seconds are only honest
   where something repaints them, and the two legacy geometries arm no timer.
2. **`card::render_card` returning four lines**, goldens pinning both profiles.
   The goldens caught less than expected — `clip_to_cells` guarantees the width,
   so a mis-sized cell is invisible unless the fixture OVERFLOWS. Two rounds of
   surviving mutants were the same lesson twice (FOOTGUNS).
3. **`triple-preview.rs` renders through `render_rows`**, so it can no longer
   drift.
4. **The double-height worktree glyph** — fixed.

The three cells, and what wiring each one taught:

| Cell     | Wired from                                                           |
| -------- | -------------------------------------------------------------------- |
| clock    | **no new field.** `last_interacted` already was the turn start, because `UserPromptSubmit` both moves it and is the only event that sets `Working`. The store timestamp §4.4 asked for was never needed. |
| subs     | `pendingBackgroundAgentCount`, from the turn's own closing record. A closing record that names NO count is a zero, not a silence — held the mark lit forever until fixed. And it must read the raw tail, not the statusLine-suppressed one: the meter has no subagent reading to yield to, and gated on it the mark never lands in a released install. |
| `wants`  | tier 1 — the permission tool name `hook.rs` was discarding. Tier 2, the scribe, is still deferred. |

Blank stayed the meaning throughout, so each landed independently and the card
was correct at every stage — which is the one prediction in this section that
held exactly.
