# UX ledger

The coordinator's decision log for the sidebar UX workstream (S5 #60, S6 #61,
S8 #63). **Not a design doc.** It records what is true now, what was decided and
why, and what is next — so a compacted or fresh session can continue without
re-deriving anything.

## The operating rule

> **Specs are an OUTPUT, not an input.** Nothing gets amended during the build.
> Discoveries land here. When the UX is real, specs get written *from what
> exists* — or deleted.

The four prior sessions circled because every discovery had to be written into a
spec before work could continue. Subagents **may read** the existing specs;
overrides travel in their brief, not in an edit to the spec.

Governing document for anything visual:
`docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`. Where it and
an S-spec disagree, the lock wins, silently, with no amendment round.

Runnable target render: `cargo run -p clave-bar --example bar-preview`.

## State

- Branch: `ux`, cut from `main` @ `48d21aa`. `main` receives milestones only.
- Known-good fallback: `b00edd3` (gates green, 222 tests, sandbox-validated).
- `main` is protected; the coordinator cannot merge. Ollie merges.

## Decisions

Numbered, dated, and durable. A decision here overrides any spec that disagrees.

> **A decision is a RULING, not a claim about the code.** Most are implemented;
> some are decided and not yet built, and those carry a **`NOT YET
> IMPLEMENTED`** banner naming what still reads differently. The task table at
> the bottom is the only statement of what has shipped.
>
> This distinction is load-bearing. The dominant defect class in this project's
> documentation has been *describing unlanded work as delivered* — a spec review
> once found fourteen instances in one pass, and a reviewer caught this file
> doing it again with D19. **When you add a decision that is not yet in the
> code, band it.**

### D1 — `oniViolet`'s 4.67 contrast is accepted (2026-07-29)

S5 states a ≥5.0 band; `oniViolet` measures 4.67. **Accepted as-is.** Zellij
theme import is coming, at which point palette hues stop being ours to choose.
Not worth a substitution round now. *(Ollie, 2026-07-29.)*

### D2 — 44 columns is the expanded target (2026-07-29)

> **SUPERSEDED by D19, implemented at D33 (2026-07-30). The expanded target is
> 54.** Left standing rather than rewritten: 44 was the width every ratified
> number in the design-lock was chosen against, and the goldens, the preview and
> `min_intact_cols` all trace back through it. Read this entry as the origin of
> the arithmetic, not as the current width.

Confirmed. Issue #63 still says "30 → 38 columns" and is wrong; the issue gets
amended rather than the design changed.

### D3 — The coordinator may amend specs and issues (2026-07-29)

Ollie, verbatim: *"You can amend things as you go, you are the principal … the
repo is greenfield, so your judgement calls to override past findings or
information sensibly is respected."* This does **not** reopen the amendment
treadmill: D3 authorises *deleting* stale claims and correcting issues, not
resolving build-time discoveries by editing a spec. Discoveries still land here.

### D4 — Task 1 is not a pure extraction (2026-07-29)

The handoff specified "extract `fn render` with no visual change, then build the
row". Overridden. The current renderer is ~20 lines of glyph-plus-name
concatenation (`crates/clave-bar/src/main.rs:573-593`) with no column
arithmetic; it shares nothing with the 44-column target beyond the word "row".
A behaviour-preserving extraction preserves behaviour worth nothing and is
rewritten the same day.

Instead the renderer is **written to the locked design directly**, in the lib,
pure and host-testable, with golden tests. The safety the extraction was buying
comes from the tests, not from the intermediate step.

### D5 — The render entry point is `render_rows(&[Row], cols) -> Vec<String>` (2026-07-29)

Not per-row. Design-lock §6 fades **every unselected row 25% toward the bar
background** *when a row is selected* — a per-row function cannot know whether
any sibling is selected without a second parameter that only exists to
reconstruct what the slice already knows. Whole-bar is also the unit you
actually look at, so a golden test asserts the picture rather than a fragment.

### D6 — One row type, grown; no parallel view struct (2026-07-29)

`Row` moves to `render.rs` and grows the presentation fields the lock needs.
`model.rs` keeps building it. A separate `RowView` projection would be a second
type to keep in sync for no gain at this size — "avoid overengineering early".

### D7 — Inks are `Option<u8>`, never bare `u8` (2026-07-29)

`u8` has no unset value: `0` is `crystalBlue`, a real palette entry, so
`unwrap_or(0)` silently paints every row one colour while reading as
"untinted". This already leaked into S5's prose and would have produced a green
test pinning a false expectation. Recorded in `FOOTGUNS.md`.

### D8 — Colour output is 24-bit truecolor (2026-07-29)

The kanagawa palette has no ANSI-16 equivalent, and lock §4.1 explicitly permits
the provenance cell "an arbitrary RGB". `Row.glyph`'s current `u8` ANSI colour
goes away with it.

### D9 — Fixed columns everywhere; `summary` is the only flex cell (2026-07-29)

This retires open decision 1 (S4 §3.4's give-way truncation over a joined string
vs the lock's fixed-width columns) **without a spec round**: the lock governs
anything visual, so fixed columns win by construction. At `cols == 44` the
layout is exactly lock §2. Away from 44, cells 1–25 and the caps hold their
widths and `summary` absorbs the difference, floored at 0. S6 §2.10's `cols - 7`
text budget is superseded and is not to be adopted.

### D10 — The bar owns its status palette; `Status::glyph()` is untouched (2026-07-29)

`clave-types`' `Status::glyph()` returns `(char, u8)` with ANSI colours and is
consumed by the host CLI. The bar needs 24-bit hues (D8) and needs three row
states that are not `Status` variants at all — `Dormant`, `Opening` and the
`stale` flag, which the renderer already distinguishes today. So the mapping
lives in `render.rs`, and `Status::glyph()` keeps its current contract.

| row state | glyph | colour |
|---|---|---|
| `NeedsYou` | `\u{25cf}` | `#E46876` waveRed |
| `Working` | `\u{25cf}` | `#FF9E3B` roninYellow |
| `Done` | `\u{25cf}` | `#98BB6C` springGreen |
| `Idle` | `\u{25cf}` | `#54546D` sumiInk4 |
| `Failed` | `\u{2716}` | `#E82424` samuraiRed |
| `Dormant` | `\u{25cc}` | `#54546D` sumiInk4 |
| `Opening` | `\u{21bb}` | `#E6C384` carpYellow |
| `Stale` | `\u{2717}` | `#E82424` samuraiRed |

`Failed` is U+2716 **heavy** multiplication x; the `stale` flag is U+2717. They
are different glyphs for different things (lock §5) and are easy to transpose.

### D11 — `bar-preview.py` becomes a Rust example and is deleted (2026-07-29)

The Python's own header already asked for this: it *"duplicates geometry that
`compose_row` will own … it should become a Rust example driven by the real
constants, so a code change that moves a column breaks the preview instead of
silently diverging from it."* Two renders of the same design is the divergence
this workstream exists to stop. `cargo run -p clave-bar --example bar-preview`
replaces it, and the Python's captured output is the byte-exact acceptance test
for the port.

### D12 — Collapsed is a different LAYOUT, not a narrower `cols` (2026-07-29)

Discovered by task 1, and it settles half of lock §3's open question with
arithmetic instead of taste.

An agent row's fixed cells sum to **27** before the summary gets a single
column: `9 gutter + 7 title + 1 + 7 repo + 1 + 1 margin + 1 cap`. So D9's fixed
columns cannot render an agent row below 27 cells — `MIN_INTACT_COLS`. But the
separation invariant `BAR_TARGET_COLS − COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP (20)`
puts the collapsed target **below 24** once expanded is 44. Both cannot hold.

Therefore collapsed cannot be this layout squeezed; it is **a second layout**,
and lock §3's "field 0 only (title, falling back to repo)" is the shape it
wants. Terminal rows already diverge here — they shrink freely, down to 11 cells
at `cols == 0`, because they have one flexible text field and no fixed columns.

**Consequence for S8 (#63), and it is not cosmetic.** The width seek moves
through intermediate widths on its way down, so every collapse transits the
band below 27. Without a collapsed layout, agent rows over-run the pane for the
duration of the animation on every single toggle. The collapsed layout is
therefore a *correctness* requirement of the seek, not deferred polish — which
is the opposite of how §3 currently reads.

Review sharpened this further: below the floor the failure was not a uniform
over-run but a **ragged one** — agent rows sat at 27 cells while terminal rows
sat at exactly `cols`, so the two kinds disagreed with *each other*. That is the
alignment loss §2.1 exists to forbid, arriving by the back door. D13 makes the
transient coherent; it does not remove the need for the collapsed layout.

### D13 — Below the floor, every row kind reports the same width (2026-07-29)

A terminal row has one flexible text field and no fixed columns, so it can
shrink to 11 cells while an agent row cannot go below 27. Left alone, the band
`11 <= cols < 27` renders a bar whose rows are different widths.

Uniform over-run is strictly better than ragged: consistent clipping keeps the
columns that *are* visible aligned, which is what §2.1 is protecting. So a
terminal row floors at `MIN_INTACT_COLS` too. This is a stopgap that makes a
transient state coherent — **not** a substitute for D12's collapsed layout.

### D14 — The renderer sanitises control characters itself (2026-07-29)

`unicode-width` reports 0 cells for C0/C1 controls, so a `summary` containing
`\n` or `\u{1b}` would break the row while still satisfying the every-row-is-
`cols`-cells test. Summaries are **agent-authored data arriving through hooks**,
so this is reachable input, not a hypothetical.

The obvious home is the wiring boundary. It goes in `render.rs` instead, on the
principle that **`render.rs` is what guarantees the width invariant, and a
guarantee that holds only if someone else sanitises first is not a guarantee.**
Deliberately minimal — this defends one stated invariant, it is not general
input sanitisation.

### D15 — The separation invariant is `> 10`, not `> 20`. The lock overstates it (2026-07-29)

**This is the finding that unblocks collapsed, and it was hiding in a
restatement.**

Design-lock §3 gives the constraint as
`BAR_TARGET_COLS − COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP (20)`, and
concludes that at 44 the collapsed target must be `< 24`. At 24 the gutter (9)
plus structure (4) leaves 11 columns for title, repo *and* summary — which is
why collapsed looked impossible.

Read the code and S8 together and the 20 evaporates. `width_seek`
(`model.rs:1032`) accepts when `2 * |cols − target| <= step`, and `seek_step` is
capped at `MAX_LEARNABLE_STEP`, so **the widest acceptance half-band is 10**.
S8 derives exactly this itself (`…-S8-sidebar-width.md:95`, `:667-672`:
*"caps the band half-width at 10"*) and then asserts `> 20` anyway — a free 2×
safety margin at the time, because `38 − 4 = 34` cleared it effortlessly.

So `> 20` is a **chosen margin that was free, not a physical bound.** The real
requirement is that neither target's *value* fall inside the other's band:
separation `> 10`. At 44 the margin stopped being free, and it was inherited as
though it were the constraint.

**Ruling: the bound is `> 10`.** Collapsed may be anything under 34. When S8 is
implemented the assertion gets relaxed with this derivation in a comment; until
then, do not design against 24.

**Generalise this.** It is the same class as the circular S4/S6 hand-off: a
document restated another document's derived number as a requirement, both were
locally reasonable, and only reading the code and both specs at once shows the
gap. Trust a derivation over a restatement — and check which one you are holding.

### D16 — Collapsed is a WIDTH PROFILE, not a second layout (2026-07-29)

Supersedes D12's conclusion, keeps its arithmetic. Ollie's directive: the gutter
stays identical, each text section shows fewer characters, repo drops to three
(`cla`, `nal` — collisions are rare, and the repo *ink* still disambiguates),
title keeps a few more than repo because it matters more, and **summary runs to
the cutoff in both states**.

That is one layout parameterised by `(title_w, repo_w)`, with summary flexing:

```
expanded  44 = 1 cap + 8 gutter + 7 title + 1 + 7 repo + 1 + 17 summary + 1 margin + 1 cap
collapsed 26 = 1 cap + 8 gutter + 5 title + 1 + 3 repo + 1 +  5 summary + 1 margin + 1 cap
```

Fixed overhead is **13** (`1 cap + 8 gutter + 2 separators + 1 margin + 1 cap`), so
`summary = cols - 13 - title - repo`. The collapsed line above originally read
`6 summary` and summed to 27, not 26 — a coordinator arithmetic slip, caught by
the implementing agent deriving the value instead of trusting the brief. Every
candidate's summary width was one too high. **Derive; do not restate.** That is
the same failure mode as D15, one document away.

Not a separate code path — the same `render_rows` with a different profile.

**The consequence that matters, and it was not the goal:** the profile is chosen
by **state**, not by current width. So while the seek animates 44 → 26 the rows
are *already* on the collapsed profile and the summary simply grows shorter.
**Nothing ever over-runs**, and the mid-collapse raggedness D12 and D13 were
built to contain cannot arise. D13 survives only as a guard against pathological
widths, not as something a user would ever see.

### D17 — Collapsed is `(title 7, repo 3)` at 30 columns (2026-07-29)

Chosen by Ollie from three rendered candidates. `Widths::COLLAPSED = { title: 7, repo: 3 }`,
`COLLAPSED_TARGET_COLS = 30`, summary = `30 − 13 − 7 − 3` = **7**. Separation
from 44 is 14, clearing D15's bound of 10.

**The title holds at 7 and does not shrink.** The rejected 5-column variant
bought two more summary characters and cost title legibility outright:
`API-GW` and `API-V2` both render `API-` plus one character, and `KDL-GRD`
becomes `KDL-G`. The chip is the thing you identify a tab *by* — truncating it
to buy prose is the wrong trade. Holding it also means the chip does not reflow
when you toggle, so the eye keeps its anchor across the transition.

### D18 — An ellipsis is suppressed in columns of 4 cells or fewer (2026-07-29)

**Found by looking, and it would have shipped otherwise.** At `repo = 3`,
`clamp` spent one of the three cells on `\u{2026}`, so every repo rendered as two
characters plus an ellipsis — `cl\u{2026}`, `do\u{2026}`, `ap\u{2026}` — which defeats the entire
reason 3 was chosen (*"'cla' for clave, and 'nal' for nalu — distinct"*).

Rule: at 4 cells or fewer the ellipsis consumes 25% or more of the field, and in
a fixed-column layout it tells the reader nothing they cannot already see. So it
is dropped and the field truncates hard. Above 4 it stays.

This leaves the ratified 44-column render untouched (expanded repo is 7, so
`dotfil\u{2026}` is unchanged) and gives collapsed the three real characters intended.

**Deferred to Gate 1, deliberately:** the design lock's own §3 argues the
stronger form — that an ellipsis *"carries no information"* in an identifier
field at any width, and that `dotfiles` beats `dotfile\u{2026}`. That would drop the
ellipsis from repo and title everywhere, including at 44, gaining a character on
every truncated identifier. It is a change to the ratified render, and it is
exactly the kind of judgement that is cheap to make while looking at a real bar
and expensive to argue in prose. **Look at it then; do not litigate it now.**

### D19 — Gate 1 verdict: KEEP. Expanded goes to 54 (2026-07-29)

> **NOT YET IMPLEMENTED.** The verdict is real; the width is not. The tree still
> ships **44** with `Widths::EXPANDED = { title: 7, repo: 7 }` and a 17-cell
> summary, and D2 describes that shipped state correctly. 54 lands in its own
> task, together with the birth percent and D26's four inherited reservations,
> because moving the target alone would leave every golden green while pinning
> the wrong width. Codex flagged the gap between this entry and the code, and
> was right to.

Ollie ran the `ux-gate1` fleet live and ruled **keep** — not refactor, not
re-engineer. *"It's looking very good indeed… The expansion and collapse do seem
overall better than my daily driver."* The blank title chip reads as deliberate.
The blank battery cell is accepted.

Expanded moves **44 → 54**, his call, *"by a fair amount"*. The split, his
words: *"mostly for the summary, but could do another two chars for the title."*
So `Widths::EXPANDED = { title: 9, repo: 7 }` and summary `54 − 13 − 9 − 7` =
**25** (from 17). Collapsed is unchanged at 30 with `(7, 3)`; separation becomes
24, comfortably over D15's bound of 10.

### D20 — The width seek stops learning what it can already read (2026-07-29)

Ollie challenged the seek's complexity: *"could this not be much more
straightforward, which should help us not have to go through the healing process
as rigorously?"* Investigated against the vendored zellij 0.44.3 source. **He is
right about two of the five moving parts, and wrong about the other three — and
the two he is right about are the two he can see.**

Forced by zellij, and staying: render-driven feedback (zellij emits no event for
a plugin's own resize); a bounded budget; an acceptance band (resizes move in
5%-of-display-area steps, so an exact column count is simply *not on the
lattice*); drift re-arm (percent geometry moves under window resize).

**Not forced, and going:**

1. **The step is computable, not learnable.** `TabInfo` carries
   `display_area_columns` and `get_tab_info(tab_id)` is a synchronous host call
   — and every `PaneInfo` in the manifest clave *already receives* carries
   `pane_columns`, which `main.rs` currently **drops** when building `PaneMeta`.
   The step is `display_area_columns * 5 / 100`, knowable before the first
   resize. **S8 §3.6's claim that "the plugin has no viewport width" is false**
   at the API level. The whole learning apparatus — `seek_step`,
   `MAX_LEARNABLE_STEP`, the learn arm, the round-17 poisoning class — exists to
   rediscover a number already in hand.
2. **The birth jank is a birth bug, not a seek bug.** The birth percent is
   hand-derived against a *fictional 200-column reference viewport*, so on a wide
   window the bar is born far too wide and visibly shrinks. Emit the percent
   computed from the **real** terminal width and birth lands within a column of
   target, with zero seek steps, on every window. Ollie saw exactly this:
   *"First paint of new tab did go very wide and then healed, it looked janky."*

Suppressing the first paints was considered and **rejected**: the pane is still
physically wide, so the neighbouring TUI reflows either way — it trades a wide
bar for a wide blank strip.

### D21 — A latent toggle bug on wide windows, predicted from source (2026-07-29)

`width_seek` accepts when `2*|cols − target| <= step`. The existing guard proves
neither *target* lies in the other's band. It does **not** prove no *width* lies
in both — any `w` within half a step of both satisfies both, which becomes
possible as soon as `step >= 14`, i.e. a display area of roughly 280 columns or
more. On such a window a bar settling near 37 is "converged" for **both**
targets and **Alt+c becomes a visual no-op**.

Ollie's window is that wide, and he reported *"some blips on the healing"*.
Unconfirmed as the cause, but it is the first hypothesis to test. D20's computed
step does not fix this by itself — the band must also be bounded so the two
targets' bands cannot overlap.

### D22 — Swap layouts are a SPIKE, not a plan; and FOOTGUNS overreaches (2026-07-29)

There is a genuine fundamental alternative: declare two named
`swap_tiled_layout`s with the bar at `Fixed(54)` / `Fixed(30)` and toggle with
`zellij action next-swap-layout --tab-id N`. That deletes resizing altogether —
exact widths, one relayout per toggle, fixed dimensions surviving window resize,
and the entire drift machinery with it.

**`SUBSYSTEM-VALIDATION` round 19 records swap layouts as "dead on arrival", and
that record is right about a different mechanism.** Round 19 relied on zellij
*implicitly* relayouting after an unsuppress, which `set_is_tiled_damaged()`
blocks. The **explicit** path is not damage-gated — the damage flag is consulted
only in the selector, where a damaged tab re-applies its current layout instead
of advancing. Clave also no longer suppresses anything (round 20). So the
blocker does not apply. **`FOOTGUNS.md:39` must be scoped to the implicit path**;
as written it reads as a blanket prohibition and would stop the next agent.

It is real in the API and traced end to end, but it has **never run live**, and
this subsystem has a twenty-round history of paths that read correctly and
behaved otherwise. Two known costs: a fixed-width bar makes its neighbour
horizontally user-unresizable (zellij refuses resizes touching a fixed pane *or
its neighbours*), and peek-on-nav becomes a layout switch too. **One sandbox tab
settles it. Do not refactor on it first.**

Also rejected on source: `OverrideLayout` **closes every tab not named in the
applied layout** — session-destroying for a fleet orchestrator.

### D23 — `{"type":"summary"}` is EXTINCT. The label's summary tier has never fired (2026-07-29)

Measured, not inferred: **0 of 153 local transcripts** contain the
`{"type":"summary"}` line that `hook.rs`'s `summary_from_tail` scans for. Claude
Code writes `{"type":"ai-title"}` instead — present in **74 of 153**.

So §6.4's entire "a summary earns the label" tier is **dead code against real
data**, and has been. Its tests pass because they use hand-written fixtures
containing a line shape the field no longer produces. **This is the largest
instance yet of the failure mode this workstream keeps finding: a green test
pinning an expectation reality abandoned.** It was invisible for as long as
nobody measured against real transcripts.

The row fields are retargeted to `ai-title`, with the extinct line kept as a
fallback. **The LABEL's tier is deliberately left pointing at the extinct
source** — retargeting it changes every tab name in the field, and that is S4's
call to make deliberately, not a side effect of this task.

### D24 — `ai-title` does not roll (2026-07-29)

Also measured: up to 85 `ai-title` lines per transcript, and **never more than
one distinct value per session**. Claude re-stamps the same string.

Every spec in this repo calls it "rolling". **It is not.** The summary column is
a stable session descriptor, not a narration of progress — closer to a subtitle
than a status line. The write path is correct either way, but any design that
assumed the column would tell you what an agent is doing *right now* is wrong,
and should be rethought rather than quietly disappointed.

### D25 — The bar is non-regressive, so `main` can take it (2026-07-29)

The merge test is not "gates green", it is **"could this be cut as a release"**.
Before this task it could not: the bar reads `title`/`summary` from the store,
nothing wrote them, and a real fleet would have shown a blank chip and a blank
column where the old bar showed `dir \u{b7} branch \u{b7} words`.

Now every row carries at least what the old label carried, via a three-tier
summary — `ai-title`, else the extinct line, else **the first prompt** — plus
the repo, the provenance glyph and the status colour the old bar never had. A
title appears only when the session was genuinely renamed (`custom-title`),
which is the ratified blank-chip behaviour rather than a gap.

Remaining known hazard, accepted: an **older binary's RMW strips earned
`title`/`summary`** in a mixed-version window (`FOOTGUNS.md:136`). Both
re-derive from the transcript on the next hook event, so it self-heals unless
the tail has also scrolled past the source. Not worth a schema fence — the
handoff already declined one, and re-derivation is the mechanism it named.

### D26 — D21 is fixed, and D19 inherits the leftovers on purpose (2026-07-29)

D21's predicted bug was real and is now dead. Acceptance is no longer a plain
band: a width counts as converged only if it is within half a step of **our**
target **and not equally near the other**. Verified by exhaustive census through
the real `width_seek`, not by argument — 404,880 runs, zero livelocks; and the
operative property tested directly, 9,640 reachable rest states × 6 consecutive
toggles with **zero cases where Alt+c fails to move the pane**.

The coordinator prescribed a simpler fix — clamp the band to `separation − 1` —
and it was **correctly overruled**. It refuses widths that are unambiguous, and
having no terminal rule it does not merely settle wrong: it **oscillates
23 → 43 → 23 → 43 to budget exhaustion**, 16 real resizes. Derive the property;
do not approximate it with a margin.

**Four reservations are carried into D19 rather than fixed now, because D19
deletes them.** At 54/30 the separation is 24, over the 20-column maximum step,
so the bands cannot overlap and the disqualification, the bracket rule and both
resting-width costs become dead paths:

- The property test's terminal clause was **widened** to pass, and now permits
  `|w − target| <= step` when the step is ≥ 14 — at step 20 and target 30 that
  range contains 44, so it would green "the collapsed bar settled exactly at the
  expanded target". The invariant is still separately and exhaustively pinned by
  `no_width_is_accepted_for_both_targets`, which mutation-testing confirms is the
  only test that catches a reversion. **D19 must re-tighten this**, and the
  threshold should be `> separation`, not `>=` — the tight half-band still holds
  at exactly 14.
- The bracket rule can rest a *collapsed* bar at 39 — nearer the expanded target
  than its own. A weaker instance of the state the clamp was rejected for.
- Collapsed can now rest as low as **14**, below `Widths::COLLAPSED`'s own
  27-cell floor, i.e. inside its clipping regime. A sharper version of the Gate 1
  watch item below.
- **Pre-existing and doubled by this branch:** real steps above
  `MAX_LEARNABLE_STEP` livelock in a re-arm/resize storm — 50,576 configurations
  on `main`, 111,788 here. The proptest generator stops at 20, so nothing would
  ever catch it. Needs a display area around 400 columns; Ollie runs ~280, so it
  is out of reach today. **Not out of reach forever.**

### D34 — The target is a suggestion. 54 does not reach Ollie's display (2026-07-30)

> **This is the finding that matters from D33's task, and it says the task did
> not achieve what it was for.** Measured through the real `width_seek`, not
> argued.

The seek accepts any width within HALF a step of its target. At a display of 280
columns the step is 14, so the band is ±7 — and the widths actually reachable are
lattice points, one step apart. From a collapsed rest of 33 the next point up is
47, and `2 × |47 − 54| = 14 <= 14`, so **47 is accepted and the seek stops
there.** It never reaches 54.

Swept across plausible displays, newborn driven to rest then toggled twice so the
expanded figure is the steady-state one:

| display | step | expanded rest | summary | collapsed rest | summary |
|---|---|---|---|---|---|
| 120 | 6 | 50 | 21 | 32 | 9 |
| 160 | 8 | 51 | 22 | 27 | **4** |
| 200 | 10 | **54** | 25 | 34 | 11 |
| 240 | 12 | 52 | 23 | 28 | 5 |
| **280 (Ollie's)** | 14 | **47** | **18** | 33 | 10 |
| 320 | 16 | **54** | 25 | 38 | 15 |
| 400 | 20 | 48 | 19 | 28 | 5 |

**Consequence, and it is a regression for the maintainer.** At 280 the expanded
bar rested at 47 before this change too — but under the old `(7, 7)` profile that
bought `47 − 13 − 7 − 7` = **20** summary cells. Under `(9, 7)` the same 47 buys
**18**. The title took two columns and the pane never widened, so the change
Ollie asked for *"mostly for the summary"* delivers two FEWER summary characters
in the state he spends his time in. Only a freshly-born bar (61) is genuinely
wider, and the first Alt+c cycle destroys that.

54 is also the unluckiest available number: it sits exactly where the nearer
lattice point is still inside the band. `2 × 7 <= 14` is an equality, so one
column either way on the target changes the answer.

**This is not a bug in D33's implementation** — every number there is right, and
the gates and mutation run are clean. It is the seek's acceptance contract
meeting a coarse lattice, which is precisely what **D20's second item** (compute
the step from `display_area_columns` and land on the target instead of near it)
exists to remove. **D19 does not actually ship until that lands.** Recorded here
so the next agent does not read D33's green gates as "the bar is 54 wide now".

Do not "fix" this by nudging the target until 280 happens to work: the lattice is
display-dependent, so a number tuned for 280 is wrong for 240 and 400. The
options are the real fix (D20), or a strict acceptance that refuses the nearer
point, which touches the property D26 verified by exhaustive census and must not
be done casually.

### D33 — 54 lands, and it retires D17's anchor property (2026-07-30)

D19 is implemented. `BAR_TARGET_COLS` 44 → 54, `Widths::EXPANDED` `(7, 7)` →
`(9, 7)`, summary 17 → 25, collapsed untouched at 30 `(7, 3)`. The birth percent
re-derives itself (`54 * 100 / 200` = 27, was 22) because #86 made it a
computation rather than three literals — that machinery earned itself here.

**The thing D19 did not notice: it breaks D17.** D17 held collapsed's title at 7
for two reasons, and the second was *"identical to `EXPANDED` … so the chip does
not reflow when the profile toggles, and the eye keeps its anchor across the
transition."* Taking expanded to 9 makes the chip reflow 9 → 7 on every Alt+c —
precisely the property D17 chose that layout to preserve.

Put to Ollie as a three-way with the arithmetic rendered, because it is a design
question and not an implementation one:

| | expanded | collapsed | anchor |
|---|---|---|---|
| **taken** | 9 title, 25 summary | 7 title, 7 summary | reflows 9 → 7 |
| holding 9 in both | 9 title, 25 summary | 9 title, **5** summary | holds |
| all ten to summary | 7 title, **27** summary | 7 title, 7 summary | holds |

**Ruled: take the reflow.** Titles of 7 cells or fewer are unaffected; only
longer ones truncate on collapse. D17's first reason — the chip is what a tab is
identified BY, so do not truncate it — still stands and is why collapsed did not
simply follow expanded to 9.

**Three of D26's four reservations are now dead**, as D26 predicted. The
separation goes 14 → 24, above `MAX_LEARNABLE_STEP` (20), so the two acceptance
bands can no longer overlap at any learnable step: the disqualification, the
bracket rule and both resting-width costs become unreachable. The proptest's
widened terminal clause was **re-tightened to `>` the separation rather than
`>=`**, exactly as D26 required — at 24 the branch is unreachable either way, but
the `>=` form was wrong on its own terms and would have greened "the collapsed
bar settled at the expanded target" at step 20. The fourth reservation
(livelock above `MAX_LEARNABLE_STEP`, needing ~400 columns) is untouched and
still out of reach.

**Seven tests failed on the constant change rather than passing against a stale
picture** — the #86 single-source work doing its job. Two needed their start
widths RE-DERIVED rather than bumped, because at 54 they would have converged in
one step and gone green while covering less (#63's shape, and the harness
newborn's start has now moved with the target three times).

At a genuinely 80-column session this leaves the agent pane 26 columns. Accepted
(D32): few sessions are that narrow, and collapsed still leaves 50.

### D32 — Fixed pane sizes are not on the table, and absolute widths already are (2026-07-30)

Ollie, during the gate-2 run: *"why does it resist fixed pane sizes? … Why do we
need relative? Is that a zellij quirk?"* Worth answering from the source once, in
the ledger, because the question recurs every time someone meets the birth
percent.

`Dimension { constraint, inner }` (`zellij-utils-0.44.3/src/pane_size.rs:96-160`).
`inner` is the resolved column count; `constraint` says how it is derived —
`Percent(p)` resolves to `(p/100) * full_size`, `Fixed(n)` resolves to `n` and
ignores the container. The decisive detail is what is **absent**: the only
constraint mutator is `set_percent` (`:122`). There is **no `set_fixed`**.
Resizing in zellij *is* percent arithmetic — the engine rewrites the percent and
re-resolves. A `Fixed` pane has no percent to rewrite, so the resize is rejected
wholesale as `CantResizeFixedPanes` (`errors.rs:710`).

So it is not a relative-versus-absolute preference. **`Fixed` is a lock** — "the
user pinned this, never touch it" — and `Percent` is the only writable channel
for a width. `size=44` would leave `Alt+c` permanently dead, which is D21's bug
made unconditional.

**And the premise is already satisfied.** The seek's targets are absolute column
counts; it converges the pane onto exactly those. Percent is the *encoding*
zellij accepts and the *birth hint*, nothing more. The 200-column fiction affects
precisely one thing: the width of the first frame, before the seek acts.

Ollie's 80-column argument is sound on its own terms — 54 + 26 does fit, and few
users run an 80-column session; if they do, collapsed leaves 50. The coordinator
initially rebutted it with "22% of 80 is 17 columns, below both floors", which
describes *today's percent scheme* and is therefore an argument **for** his
complaint, not against his proposal. Recorded because the rebuttal was aimed at
the wrong claim, and the next agent should not re-run it.

Ruled, and still open: constants like these should eventually be **user
configurable**, in a style matching zellij's own config. A later refactor, not
this branch.

### D31 — The sub-floor over-run is CLIPPED here, not left to the terminal (2026-07-30)

**D13 is half wrong, and the live gate-2 run falsified the half that mattered.**

D13 ruled that below `min_intact_cols()` the fixed columns hold and the row is
built WIDER than the pane, uniformly across row kinds, rather than reflowing a
fixed column — that part stands and is not touched. What it also assumed is that
an over-wide row would read on screen as **clipped**. It does not. **A terminal
wraps it**, and a wrapped row makes every bar row double-height with a blank
second line.

Observed by Ollie 2026-07-30, twice, in the D28 gate-2 run: once on moving the
window to a second monitor, and once on every tab spawn — a new tab is born at
the birth percent, which under ~123 columns lands the pane below `EXPANDED`'s
27-cell floor while the state still (correctly) says expanded. In the second
screenshot the repo names themselves spilled to line two. **`Alt+c` healed it
every time**, because any event forces a fresh render at the settled width.

The uniformity D13 chose was intact throughout — every row over-ran by the same
amount. Only the assumption about what a terminal does with an over-wide line was
false.

**The fix is one function.** `render_row` still builds at the floor, so no fixed
column reflows and D9/D13's guarantee is untouched; `render_rows` then clips the
finished line to `cols` (`clip_to_cells`), SGR-aware so the colour state is
closed rather than left open, and padding rather than half-drawing a wide glyph
that straddles the cut.

**Three tests PINNED the over-run and were changed** — this is a behavioural
change, not a bug fix, and it should read as one:
`every_row_is_exactly_cols_cells_under_collapsed` and
`degenerate_widths_do_not_panic` both asserted
`cols.max(min_intact_cols())`; both now assert `cols` unconditionally.
`every_row_is_exactly_cols_cells` gained sub-floor widths it never covered.

**A correction the next agent needs, because D20 invites the opposite reading.**
D20 says `main.rs` drops `pane_columns` when building `PaneMeta`, which is true —
but that is about computing the seek's STEP. It does **not** mean the bar renders
at a stale belief: `ZellijPlugin::render(&mut self, _rows, cols)` receives the
real pane width from zellij and passes it straight to `render_rows`
(`main.rs:560`). The bar has always known its own width at render time. The wrap
was never a missing input — it was a deliberate over-run meeting a terminal that
wraps.

**What this does NOT fix.** The birth width is still derived against the fictional
200-column viewport (D20's second item, still open). A stale or wrong birth now
renders *clipped* instead of *corrupted*, and the seek corrects it within a frame
— so the remaining symptom is a brief flicker, not a broken bar.

### D30 — `just sandbox` DOES write `~/.claude/settings.json`. Two documents say it does not (2026-07-29)

**Found by the agent writing the live-interaction checklist, which refused to run
`just sandbox` because its brief forbade writing under `~/.claude/` — and it was
right to refuse.** The coordinator ran `just sandbox` repeatedly this session on
the documented belief that it was safe.

`dev scenario` → `run_setup` → `merge_hooks(settings, "clave")`
(`setup.rs:426-450`) rewrites the **daily fleet's** hook commands in the real
`~/.claude/settings.json`. Measured after this session's runs: all four hooks now
read bare `clave` rather than a versioned absolute path.

**Why this is more than untidy.** A bare command is PATH-resolved, and PATH
resolution in a hook is *"the one leak"* — the hazard `CONTRIBUTING.md` records
as breaking v0.1.1 in the field (#43/#44). On this machine bare `clave` resolves
to `~/.cargo/bin/clave` (v0.1.1), and `~/.local/share/clave/bin/clave` — the #43a
launcher — **does not exist**, so the pre-#43b path is what answers. Nothing is
visibly broken; the *mechanism* is the one the release model exists to remove.

**Two documents assert the opposite** and both must be corrected:
`scripts/sandbox-setup.sh`'s header and `docs/dev/TESTING.md`'s lifecycle
section. The script's own self-check greps `~/.cargo/bin/clave`,
`~/.local/share/clave` and the launcher — and passes — because
`~/.claude/settings.json` **is not among the surfaces it guards.** A safety check
that omits a surface reads as proof that surface is safe.

**Do not fix this by hand.** `just release` regenerates hooks with versioned
paths, and a release is imminent (D28). But it must be **verified after the
cut**, not assumed: `jq` the hook commands and confirm they are absolute.

**And the standing instruction was right all along** — `~/.claude` is
Claude's, not ours. That rule existed; the tooling violated it and the docs
covered for it. **Ollie should decide whether `just sandbox` should touch it at
all**, or whether the sandbox needs its own `CLAUDE_CONFIG_DIR` for hooks the way
it already has one for state and data.

### D29 — The S-specs get SALVAGED and DELETED, not updated (2026-07-29)

> **NOT YET IMPLEMENTED**, by design. Banners land now (see below); the salvage
> and deletion happen **after the release**, as their own PR.

The operating rule ends *"specs get written from what exists — or deleted."*
Ruling: **deleted.** Not updated, and not merely warned about in a handoff.

**Why not updated.** All 6,632 lines exist because the visual surface had no test
access, so prose was the only medium available. That is no longer true — there is
an executable render, a golden-tested renderer and this ledger. Maintaining a
second description of what the code now describes better *is* the treadmill, and
this session proved these documents drift **silently**: a claim was false in the
field for months and nothing noticed.

**Why not just a handoff directive.** A warning in a status file does not stop a
`grep`. The next agent searches for "provenance" or "width budget", lands in S4,
and finds false content with no warning attached. `FOOTGUNS` already names this
failure: *a trap index that teaches a false test is worse than no entry*, because
the reader runs it and concludes there is no trap.

**Three documents, three dispositions — this is not uniform:**

- **The design lock STAYS, and stays authoritative.** It is not an implementation
  plan; it is earned knowledge that cannot live in code — the thirteen-tool survey
  finding that essentially nothing marks a default branch, why no worktree glyph
  exists anywhere, why hashing was overruled twice, the glyph rule. Amend §3
  (collapsed is settled by D16/D17, and its `< 24` by D15). Nothing else.
- **S4/S5/S6 get salvaged, then deleted.** Rationale moves to the homes that
  already own it *and are actually read*: traps → `FOOTGUNS.md`, dead ends →
  `SUBSYSTEM-VALIDATION.md`'s C-sections (which exist for exactly this),
  vocabulary → `UBIQUITOUS_LANGUAGE.md`, remaining work → the issues already
  tracking it (#59, #60, #61).
- **S8 is handled as part of #63**, not in a separate sweep — it is about to be
  actively worked, so its live content matters there.

**The test for every claim: could this be re-derived from the code or a test in
under a minute?** Yes → delete. No → it is earned knowledge, and it moves. Nothing
is "updated in place."

**The risk to guard against is an agent *reconciling* instead of salvaging** —
that instinct is how 6,632 lines happened. Deletion is reversible (git has it);
a reconciliation round is not, because it costs a session.

**Banners now, on #86.** Four claims are known false and carry no warning today.
A banner is a **warning, not a reconciliation** — it marks a document as
untrustworthy rather than resolving a discovery by editing it, which is precisely
the line between this fix and the loop. Cheap, surgical, and it does not
invalidate #86's three review rounds the way a 6,632-line deletion would.

### D28 — The release sequence, ruled by Ollie (2026-07-29)

Merging #86 **installs nothing** — his fleet runs the stable binary from a
previous cut, and only `just release` (his command, never an agent's) promotes.
So "merged" is the midpoint, not the finish line. Asked, and ruled:

> *"after a live interaction test and being happy with testing, then we will
> release and switch my daily driver."*

The gates, in order, each of which must pass before the next:

1. **Merge #86.** Done when he approves; all threads resolved, all checks green.
2. **A live INTERACTION test** — not another look. Gate 1 validated the *design*;
   this validates the *behaviour*, and no interaction path has run live at 44
   columns. The checklist lives with `docs/dev/TESTING.md`'s live-validation SOP.
   **D26's and the Gate 1 section's predictions exist to be settled here.**
3. **Confidence in the testing itself** — `docs/dev/TESTING.md`'s six shapes and
   the `just mutants` gate. Note the honest gap: the `--in-diff` path has never
   run end to end.
4. **`just release`, then switch the daily driver.** His hands only.

**Consequence for planning:** the 54-column work (#63) and S5 sit *after* a
release that has already shipped 44. That is deliberate — it gets the validated
design onto his daily driver rather than holding it behind more change, and it
means 44 must be genuinely good rather than merely a waypoint.

### D27 — Five shapes of bad test, all found in one session (2026-07-29)

**Queued work, agreed with Ollie: write this into `docs/dev/TESTING.md` and add
`cargo-mutants` as a gate on changed files — both AFTER the merge of #86.**
Recorded here first so the evidence is not lost to a compaction.

This session found five *distinct* ways a test can be green and worthless. That
is a pattern, not luck, and the pattern has one root: **the test asserts against
the implementation instead of against an independently derived expectation.**

| # | Shape | Instance |
|---|---|---|
| 1 | Passes under **both** branches of the thing it names | `mix_rounds_ties_to_even` asserted 149.5 → 150, identical under ties-to-even and half-away-from-zero |
| 2 | Goes **green-and-vacuous** when a constant moves | two seek tests whose simulator now started already on its target |
| 3 | Stays green and silently **covers less** | a harness test that drove two resizes against 30 and one against 44 |
| 4 | Name and comment claim a property it **never proves** | the band-disjointness assertion was algebraically identical to the line above it — and sat exactly where the next agent would look to rule the bug out |
| 5 | Tests a fixture shape **reality abandoned** | `{"type":"summary"}` — 0 of 153 real transcripts; the whole tier was dead in the field |

Shapes 1–4 are "test and code agree because the test came from the code".
Shape 5 is "test and code agree because both were written from the same wrong
belief". **None is caught by coverage, and none by CI.** Shape 3 is not even
caught by red/green — the suite goes green *before and after* the coverage
shrinks.

What the fixes should be, in leverage order:

1. **`cargo-mutants` on changed files.** The only mechanical catcher of 1–4. It
   was used by hand three times this session and found something every time,
   including proving that one test was the *sole* guard against a reversion.

   **Landed as `just mutants` / `just mutants-file`, and DELIBERATELY NOT in
   `just gates`.** This entry originally said "as a gate", which CodeRabbit
   correctly flagged as prescribing a workflow the implementation then rejected.
   The reasoning for rejecting it is better than the original wording: gates run
   on every PR and must stay fast, a full run over `model.rs` is enormous, and
   **a gate nobody can afford to run is a gate nobody runs.** Which change
   classes owe a run lives in `docs/dev/TESTING.md`'s risk taxonomy instead —
   deliberate act, not a checkbox. First real run: 81 mutants on `render.rs`,
   two survivors in `Rgb::hex`.
2. **Fixtures captured from reality, with a liveness assertion** — a test that
   fails when the shape we parse appears in **zero** real transcripts would have
   caught shape 5 the day it went extinct, rather than months later by accident.
3. **A golden must carry its derivation in its doc-comment**, so a reviewer can
   check the literal against the *design* rather than against the code that
   emitted it. Started on the collapsed golden; make it the rule.

What is genuinely good and should not be disturbed: the **lib/bin split** is why
any of this is testable at all, and `SimZellij` is strong enough that a reviewer
ran **404,880 exhaustive runs** through the real state machine to settle a
question that would otherwise have been argued in prose.

## Gate 1 — DONE, and what it did and did not settle

**Gate 1 happened on 2026-07-29 and the verdict is D19: keep.** Ollie ran the
`ux-gate1` fleet live, expanded and collapsed, and reported the blank title chip
and blank battery cell both read correctly.

What follows were the three predictions written *before* that look, kept because
**two of them are still open** — a live look answers a question, it does not
retire the class. Prediction 1 is confirmed in a stronger form than written: the
overlap it describes is now understood exactly (D21) and fixed (D26).

1. **Collapse will probably rest wider than 30.** On a ~200-column window
   zellij's resize step is ~10 columns, so 44 → 30 is about 1.4 steps: the first
   shrink lands near 34, acceptance is `2*|diff| <= step` → `2*4 <= 10` accepts,
   and the bar settles at ~34 while rendering the `COLLAPSED` profile. That
   gives roughly an 11-character summary instead of the designed 7. This is
   *correct* behaviour — the seek's contract is "wherever cols stop changing is
   accepted" — but the visible expanded/collapsed difference will be smaller
   than the constants suggest. If it bothers the eye, the lever is
   `COLLAPSED_TARGET_COLS`, not the profile.
2. **A newborn bar on a narrow window will over-run at birth.** The layout sizes
   the pane at 22% (D-derived for 44), so on a window under ~125 columns the
   newborn bar is below `EXPANDED`'s 27-cell floor and rows over-run until the
   seek grows it. Same class as D12/D13, but **newly reachable at birth rather
   than only mid-collapse**. Watch the first paint of a fresh tab.
3. **Provisional inks renumber when the repo set changes.** Allocation is
   positional over the sorted repo set, so adding a repo that sorts early shifts
   every repo after it. Lock §4's store-backed allocator exists precisely to
   prevent this and it is the one property the stand-in cannot fake. **If
   colours shift between looks, that is why — it is not a renderer bug.**

## Known-stale spec content — recognise, do not fix

Catalogued deliberately. If one blocks a task, override it in the brief.

- S6 §2.10/§2.10.1's `cols - 7` text budget — superseded by D9.
- S6's `glyphs` plugin-config key and two-tier `GlyphSet` (§2.6.5, §3.1(b),
  §3.7, four §4.1 tests) — a `glyphs` config key reproduces the v0.1.1
  double-sidebar; zellij hashes plugin identity over the whole config map.
  Glyphs are compiled in (lock §5.3).
- S6's terminal mark `\u{f489}` vs the lock's nf-md-console `\u{f018d}`.
- S6 cell 3 is two-state ("worktree marker"); the lock says **three-state**
  provenance (main / branch / worktree). A `Row` design change, not an amendment.
- All `file.rs:line` citations across S4 are pre-#69 and have drifted. Trust the
  code, never a line number in a spec.
- `bar-preview.py:59` names `#1F1F28` "sumiInk1"; S5 and the lock say "sumiInk3".

## Open items

- **Collapsed geometry** — no longer blocked. Lock §3's `< 24` rested on a
  restatement that D15 refutes; the bound is `< 34`. The shape is D16's width
  profile. What remains is picking two numbers by looking at them. Lock §3's
  other open question — truncate the whole label vs render field 0 only — is
  **moot**: the profile keeps all three fields, just narrower.
- ~~**`spawn_mode` orphans relocated sessions**~~ — **investigated 2026-07-29,
  closed without filing. Do not re-investigate.** The handoff carried this as a
  probable silent-data-loss bug. It is mostly not one, and the correction is
  worth more than the original claim.

  The common case is **already handled and loud**: `open.rs:88` and
  `setup.rs:577` both pre-filter on `Path::is_dir()`, so a moved or deleted cwd
  yields `OpenDecision::Stale` and a `\u{2717}` row rather than reaching
  `clave spawn`; if it does reach it, `canonicalize` at `main.rs:221` fails and
  the pane errors visibly. That is issue **#15**, which calls it correct.

  What is genuinely real is narrower: `spawn_mode` checks the frozen cwd for
  **existence, never for identity**. Delete the directory at the frozen path and
  replace it with a *symlink to a different target at the same path*, and every
  guard passes — `is_dir()` follows symlinks — while `canonicalize` now resolves
  elsewhere, so the jsonl lookup misses and the session silently starts fresh.
  Contrived enough that it is not worth an issue on its own; recorded here so it
  is recognised if a future change widens the trigger.

  Two things learned that outlive the bug: `munge_cwd` (`munge.rs:20-24`) is
  **not injective** (`/a/b/c` and `/a-b-c` both give `-a-b-c`), which is
  harmless only because the uuid filename disambiguates — and it is not ours to
  fix anyway, since it must mirror Claude Code's own munging. And the general
  invariant worth holding: **clave verifies a cwd exists, never that it is the
  same place.** Cheap to violate more seriously than this.
- ~~**Issue #63 says "30 → 38 columns"**~~ — amended 2026-07-29 to 44, with a
  superseded banner. Its three findings were *measured at 38*; they are kept for
  their reasoning and explicitly flagged as needing re-measurement, because the
  expected-red-set finding is arithmetic in the target and does not transfer.
- **Repo/title ink allocation** is store-backed iterate-and-wrap, not hashed
  (lock §4). That is cross-process state and owes an ordering/idempotency
  argument. Not yet built — the renderer takes inks as input.
- **Pinning (#80) is coming.** Do not design it out; build nothing for it yet.

## Task progress

| Task | Status | Commits | Notes |
|---|---|---|---|
| 1 — the pure 44-column renderer | **complete** | `8fb4aca`..`ca884d1` | `render.rs` + 17 tests; `bar-preview` is a Rust example driven by `render_rows`, byte-identical to the Python it deletes. Reviewed; one fix round (2 Important + 7 Minor), each fix mutation-checked. 236 tests. Not wired — `main.rs`/`model.rs` untouched. |
| 1.5 — the collapsed width profile (D16) | **complete** | `84e3348`..`d7b2783` | `Widths` profile, D17 chosen from rendered candidates, D18's ellipsis rule, column map derived from the profiles. 241 tests. |
| 2 — wire `model.rs` and `main.rs` to `render_rows` | **complete** | `c48f0b4`..`65496b4` | `Row` unified, projection from `&Agent`, provisional inks, targets 30→44 and 4→30, birth percent 15%→22%. Reviewed; one fix round (4 Important + 6 Minor). 255 tests. |
| 3 — the `ux-gate1` sandbox scenario | **complete** | `0ff1e05` | 7 agents, 5 distinct repos, every status, all three provenances. The existing scenarios seed `title: None`, `summary: ""` and all-`Idle` — they could not have shown the design. |
| 4 — the hook persists `title` and `summary` | **complete** | `0ccf04d` | Found `{"type":"summary"}` extinct (D23) and retargeted to `ai-title`. Three-tier summary makes the bar non-regressive (D25). |
| **Gate 1 — live look** | **DONE 2026-07-29** | — | Verdict **keep** (D19). |
| 5 — pre-merge review round | **complete** | `3da3235`..`94daccb` | Whole-branch review + a focused review of the seek change. D21's bug fixed and verified by exhaustive census (D26). |
| 6 — expanded 44 → 54 (D19) | **implemented, NOT delivered** | this branch | Target, profile `(9, 7)`, summary 25, birth percent 27 — all derived, not restated. Retires D17's anchor property (D33) and three of D26's four reservations; proptest clause re-tightened to `>`. **But see D34: the seek rests at 47 on a 280-column display, so the maintainer gets 18 summary cells where he had 20. Does not ship without 7c.** |
| 7a — the sub-floor clip (D31) | **complete** | this branch | `clip_to_cells`: the over-run is truncated here rather than left to a terminal that wraps it. Fixes the live double-height bug. Three tests that PINNED the over-run changed. |
| 7b — birth from the real terminal width (D20) | **not started, and now cosmetic** | — | Blocked on a dependency call: `clave` has nothing that reads terminal width. With D31's clip in, a wrong birth is a flicker rather than a corrupted bar. |
| 7c — the step from `display_area_columns` (D20) | not started | — | `PaneInfo.pane_columns` is in the manifest and `main.rs` drops it. Deletes the whole learning apparatus. Larger and riskier than 7a/7b — D26's census covers current behaviour. |
| 8 — S5, store-backed ink allocation | not started | — | Ollie's colour-stability requirement. The provisional allocator is positional and renumbers. |
