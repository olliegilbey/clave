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
are defined in [UBIQUITOUS_LANGUAGE.md](../../../UBIQUITOUS_LANGUAGE.md). Three
terms are new here and defined in §7: **rail**, **shadow rule**, **crop rule**.

**Source-of-truth hierarchy.** _This document is authoritative_ for every
ruling, number and rationale. `crates/clave-bar/examples/triple-preview.rs` is
the illustration, and **today it is also the only implementation** — the
geometry has not yet been ported into `card.rs`, so unlike `double-preview.rs`
it carries its own copy and can drift. Where the two disagree, this file wins
and the example is the bug. §8 lists what porting costs.

---

## 1. The card

```text
  line 1:  status  │ chip-pill  summary
  line 2:  prov    │ repo [branch]  #PR   provider model effort
  line 3:  subs    │ tokens  elapsed  wants                turn
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

**The worktree glyph changed.** The shipped card uses `\u{168c2}`, a Bamum
letter, taken knowingly because no surveyed icon set contains a worktree glyph
at all (already in FOOTGUNS.md). What had not been costed is that it arrives by
fallback out of a proportional historic-script face and so renders **off-centre
in its cell** — tolerable beside one other glyph, not once a third joins the
column. `fa-tree` `\u{f1bb}` is a metaphor rather than a depiction and is in the
font. See §6.

### 4.3 The two clocks stay apart, and elapsed is the one that matters

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

Blue (`#7E9CD8` kanagawa crystalBlue) while a turn is in flight, and **blank**
when none is — not dimmed. Blank is already the card's word for "no reading",
and a blank clock beside a lit one is the cheapest possible "this agent is
thinking".

**It costs no store traffic.** The store holds *when the turn began*; the bar
subtracts from its own wall clock, which it already reads once per render for
the elapsed label. The fleet is never re-pushed to animate a counter.

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
timer fired by elapsed seconds, with bands at 0.15s, 1.0s and 3.0s guarded by a
compile-time assert chain in `crates/clave-bar/src/main.rs`. 0.2s sits in a gap;
anything faster requires replacing the classifier with tagged timers first.

### 4.6 The subagent mark is a boolean

`\u{f171a}` `md-robot_happy_outline`, in `#9CABCA` — the quiet blue-grey of a
structural mark, because it says "this row has depth", not "this row is hot". It
is the third structural glyph, stacked under status and provenance.

**A count was tried and dropped.** "This row has fanned out" is the whole
signal; the digit beside it was noise.

### 4.7 `wants` — the flexing cell, and the reason for the change

What this agent is blocked on, in its own words, in `NEEDS_YOU_INK`. It claims
every cell the numbers on line 3 do not: **28 at 48 columns**. Blank means
nothing is needed, so an idle fleet has a quiet right-hand column and the one
row that wants you grows text.

**Structure gates; the model only supplies words.** Whether a row is waiting is
decided by the free structural signal that already drives the status mark — a
fact. `wants` is written only for rows that signal already flagged, so a scribe
outage degrades to today's dot rather than to silence.

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
| gap     | **2**       | —                                      | —                      |
| elapsed | **3**, right-aligned | time since you last interacted | meta ink               |
| wants   | 1 + flex    | §4.7 — expanded only                   | needs-you ink          |
| turn    | 1 + **3**, right-aligned | the live turn clock — expanded only | crystalBlue, or blank |
| margin  | 1           | —                                      | —                      |

**The token gap is two cells, not one.** At one, `211k 20m` reads as a single
figure — the same failure that keeps the two clocks apart.

**Collapsed locks elapsed to the right edge**, one cell in, computed from what
the line has already spent rather than from a second margin constant. That is
the same column the turn clock holds in the expanded card, so whichever clock is
rightmost sits in the same place and the eye finds a duration in one spot in
both profiles.

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
branch, the PR, the provider, the model, the effort tag, the turn clock, and the
`wants` message.

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

Enumerate the real `cmap` of the installed `FiraCodeNerdFontMono-Regular.ttf`
before committing a glyph. And a fallback glyph brings its **own metrics**: it
holds its cell if the fallback face is monospace, but weight and baseline do
not, and neither shows in a static render or a golden. That is what caught the
old worktree glyph (§4.2), and it is why five of the six spinner frames — which
also come by fallback, out of Menlo — need an eyeball on the real terminal
rather than a signed-off screenshot. All three entries are in
[FOOTGUNS.md](../../../FOOTGUNS.md) § Text, glyphs, rendering.

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
- **The turn clock winning the crop** — §4.3, reversed at round 8.
- **The PR on line 3** — §4.9.
- **The two clocks adjacent** — §4.3.
- **Collapsed at 38 columns** — §1. Not collapsed, just narrow.
- **Dimming the turn clock when no turn is running** — §4.4. Blank is the
  card's existing word for "no reading".

## 9. What the port still costs

The geometry above is ratified; `card.rs` does not implement it yet. Porting it
means, in order:

1. A `RowHeight` variant carrying `lines_per_row() == 4` and
   `target_cols(collapsed)` of 16 / 48. Every width in the codebase flows
   through this enum, and it is **launch-baked into the Zellij KDL artifacts** —
   changing profile means regenerating them.
2. `card::render_card` returning four lines instead of two, with goldens pinning
   both profiles cell for cell.
3. Rewriting `triple-preview.rs` to render through `render_rows`, the way
   `double-preview.rs` does, so the illustration can no longer drift.
4. Fixing the worktree glyph on the shipped double-height card too (§4.2) — it
   renders off-centre there today, independent of this lock.

Three cells have **no data behind them yet** and must render blank until their
source is wired:

| Cell     | Needs                                                                |
| -------- | -------------------------------------------------------------------- |
| turn     | the turn-start timestamp in the store; the bar subtracts (§4.4)      |
| subs     | the subagent boolean, from `pendingBackgroundAgentCount` in the transcript |
| `wants`  | tier 1 is free — stop discarding the permission tool name in `hook.rs`. Tier 2 is the scribe. |

Blank is the meaning, so each can land independently and the card is correct at
every stage.
