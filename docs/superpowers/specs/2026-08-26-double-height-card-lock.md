# Double-height card — locked

_Ratified 2026-08-26 by the maintainer ("that's the one"), from rendered cards
rather than from prose. Revises the 2026-07-25 sidebar visual design lock for
**card mode**, which is the default; that document stays authoritative for the
single-line row, still reachable with `clave rows single`._

Run the design: `cargo run -p clave-bar --example double-preview`

**Vocabulary:** *card, row, gutter, cell, ink, chip, provenance, glass, fade,
zebra* are defined in [UBIQUITOUS_LANGUAGE.md](../../../UBIQUITOUS_LANGUAGE.md).
A **row** is the data side; a **card** is its two-line rendering.

**Source-of-truth hierarchy.** *This document is authoritative* for every
ruling, number and rationale. `crates/clave-bar/examples/double-preview.rs` is
the **illustration**: it renders the mock fleet through `render_rows` itself, so
it cannot drift from the shipped geometry, but where it and this file disagree
this file wins and the example is the bug. The budgets below are the constants in
`crates/clave-bar/src/card.rs`; the goldens at the bottom of that file pin the
picture cell for cell.

---

## 1. The card

```text
  line 1:  status ╭ chip-pill  summary                 tokens
  line 2:  prov   ╰ repo [branch]  #PR  provider  model      elapsed
```

Line 1 answers *what is this and how hot is it*. Line 2 answers *which checkout,
which conversation, how stale*. Nothing crosses lines: a cell that has no value
renders blank in its own line's budget, and no cell reflows into another's.

Two profiles, one geometry. **Collapsed is 38 columns, expanded 48.** Expanded
buys exactly two things — the branch beside the repo on line 2, and ten more
columns of summary on line 1. Nothing else moves; nothing re-arranges. Both
numbers live in `RowHeight::Double::target_cols`, and the card reads its profile
off the width it is actually painted at, so the pane geometry and the drawn
geometry cannot disagree.

## 2. Line 1 — the cells

| Cell | Cells | Content | Ink |
|---|---|---|---|
| status | 3 (` X `) | the status mark; the console mark on a terminal card | the state's fixed semantic colour |
| arc | 2 (`╭ `) | the card's top joiner | alternates two neutrals, card by card |
| pill | 9 (cap + **7** + cap) | the rename you gave the tab; the tab name on a terminal card | title ink background, chip ink text — theme black on a terminal card |
| summary | flex | the agent's own description of the session; the last foreground command on a terminal card | default ink |
| tokens | 1 + **4** | thousands of tokens, right-aligned (`105k`, `1.1m`); `TERM` on a terminal card; blank with no reading | the battery ramp's risk band |
| margin | 1 | — | — |

The summary is the only flexing cell: **`cols - 21`** with a pill (17 collapsed,
27 expanded), **`cols - 12`** without one (26 collapsed, 36 expanded). A session
that was never renamed drops the pill entirely and the summary claims its nine
columns — an unnamed conversation shows more of what it is about rather than a
blank chip.

## 3. Line 2 — the cells

| Cell | Cells | Content | Ink |
|---|---|---|---|
| provenance | 3 (` g `) | worktree or branch mark; **blank** on an ordinary checkout | the repo's palette ink |
| arc | 2 (`╰ `) | the card's bottom joiner | the same neutral as line 1's |
| repo + branch | 1 + **9** collapsed, 1 + **19** expanded | the repo name; expanded adds the branch one space after the repo NAME | repo in its palette ink, branch in meta ink |
| PR | 1 + **5** | `#225`, the PR the checkout is driving; blank when there is none | PR green |
| provider | 3 (2 spaces + glyph) | the provider's brand mark; blank for a provider clave does not know | the provider's fixed brand colour |
| model | 1 + **6** | the model handle (`opus`, `gpt-5`) | meta ink |
| fill | 3 | — | — |
| elapsed | **3** right-aligned | time since your last interaction, coarse (`5m`, `4h`, `2w`) | meta ink |
| margin | 1 | — | — |

**Repo and branch share one collective budget** (`9 + 1 + 9` = 19 in the
expanded profile). The branch starts one space after the repo *name*, not after
its padded cell, and claims every column the repo does not use — with a
guaranteed minimum of **9**, so a long repo truncates before a branch does.
Branch names run longer than repo names, which is the whole reason the minimum
sits on the branch. A branchless checkout gives the whole budget to the repo.
The PR column never moves *where a PR exists* — but a card with **no PR folds
the PR cell's six columns into this budget** (amended at the 2026-08-27 live
drive: six dead cells beside a truncated name were waste).

Collapsed has no branch cell of its own, and the repo takes its flat 9 — unless
the card has no PR, in which case the reclaimed columns widen the budget to 15
and a branch renders there too.

## 4. Ratified decisions

**The arc alternation IS the zebra.** `╭`/`╰` alternate between two quiet
neutrals — fujiGray and springViolet2, close enough in weight that neither reads
as a state. Adjacent cards separate in the linework, not in a painted stripe. The
parity is the card's position **in the viewport slice**, not in the list, so the
pane's top card always wears the same ink and a row arriving above the view
cannot invert every stripe on screen.

**Glass, and the `49m` discipline.** Ollie's terminal renders painted cell
backgrounds OPAQUE, and ANSI has no per-cell opacity: a cell is either default
background (glass) or a concrete RGB. So an unselected card paints **nothing**
and re-asserts default background (`\u{1b}[49m`) on every segment, and the
selection is a full opaque bar across both lines. There is no second opacity to
spend, which is what kills every painted-zebra variant below.

**Blank is the meaning.** Provenance, branch, PR, model, provider, tokens and
elapsed all render an empty cell when the value is absent. A main checkout draws
no provenance glyph — that is what keeps the marked states meaningful. An agent
with no token reading blanks the cell rather than inventing a measurement, and
`TERM` belongs to terminal cards alone. A provider clave has never heard of
draws no mark rather than a guess.

**Terminal cards are cards.** Console mark in the status cell coloured by the
terminal's state, a `TERM`-style pill in theme black where an agent's chip
carries a palette colour, the last foreground command as the summary, and `TERM`
in the token cell. Provenance, repo, branch and PR are **borrowed from the
checkout** exactly as an agent card's are: shells and agents read as one fleet
in one visual language.

**Fixed versus theme-following inks.** Theme-following: palette, base,
selection, default ink, chip ink. **Fixed semantic**, never repainted by a
theme: the status marks, the battery ramp bands, the two provider brand colours
(Anthropic coral, OpenAI green), the PR green, the meta grey and the two arc
neutrals. Red means failed everywhere, and a brand colour is the brand's.

**The fade ladder is the single-line row's, unchanged.** Recession is relative —
nothing recedes when nothing is selected. The dormant fade is absolute. `Opening`
escapes it, being mid-launch.

**Row height is a launch-baked flag.** `clave rows single|double` writes the
mode into the generated Zellij artifacts and regenerates setup; the bar reads it
from its plugin config at load. Double is the default. The single-line renderer,
its goldens and its width targets are retained unchanged behind the flag —
model, provider, PR and elapsed stay card-only.

**The odd line is blank by omission.** A pane with room for two and a half cards
draws two; the leftover line is left blank and is inert to clicks. Half a card —
a top arc with no bottom — is not a thing this design has.

**Every glyph is a `\u{...}` escape**, never a literal (carried from the
2026-07-25 lock §5.4; literal glyphs were silently lost in transit twice).

## 5. Rejected paths — do not resurrect

Each of these was rendered and rejected during ratification:

- **Painted zebra rows** — every second card given a background. Kills glass;
  ANSI has no second opacity to offer.
- **Gutter-patch zebra** — the stripe confined to the left gutter. Reads as a
  state marker, not as separation.
- **Dot gutters** — a leading dot column per line instead of the arc. Adds a
  cell, says less than the joiner it replaces.
- **Heavy, curly and square joiners** — `┏`/`┗`, `┌`/`└`, `╔`/`╚`. The light arc
  is the only pair quiet enough to sit under text without becoming chrome.
- **Small-caps state words** — the status rendered as a word rather than a mark.
  Costs columns the summary needs and duplicates the colour channel.
- **Inline and badge token placements** — the count beside the title or in a
  filled badge. Right-aligned is the edge the eye compares magnitudes on.

Also out of scope, from the spec: a third width profile, any per-profile layout
divergence beyond the branch cell and the wider flex, single-line variants of the
card-only cells, and any per-cell opacity ambition.

## 6. What the 2026-07-25 lock still governs

Everything not listed above: the palette and its round-robin allocation, the
status mark table and the thirteen-tool glyph survey behind it, the battery ramp
and the smart zone, the repo-ink-forever rule, the escape rule, and the whole of
the single-line row's geometry, which `clave rows single` still renders.
