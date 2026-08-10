# Sidebar visual design — locked

_Ratified 2026-07-25 by the maintainer, from rendered rows rather than from
prose. Supersedes the geometry in `2026-07-22-S6-gutter-glyphs.md` and
`2026-07-22-S8-sidebar-width.md`; both must be revised against this file before
their worktrees start._

Run the design: `cargo run -p clave-bar --example bar-preview`

**Vocabulary:** every term used here — *gutter, cell, rule, cap, provenance,
ink, chip, tint, fade, title vs label, live vs dormant row* — is defined in
[UBIQUITOUS_LANGUAGE.md](../../../UBIQUITOUS_LANGUAGE.md) §3. Read that first if
any word below is doing more work than you expect.

**Source-of-truth hierarchy — one rule, no ambiguity.** *This document is
authoritative* for every ruling, number and rationale. The `bar-preview` example
(`crates/clave-bar/examples/bar-preview.rs`, formerly `bar-preview.py` in this
directory) is an
**illustration**: it shows what the rulings look like, and it self-checks that
every row is exactly 44 display cells, but where the two ever disagree **this
file wins and the script is the bug**. Every number below was chosen against
alternatives that were rendered and rejected, and the rejected ones are recorded
so they are not re-proposed.

> [!NOTE]
> **This document STAYS authoritative for anything visual — 2026-07-29.** The
> sidebar was rewritten on the `ux` branch; unlike the S-specs, this file was
> *built from* rendered rows and its earned knowledge stands (the thirteen-tool
> glyph survey, why no worktree glyph exists, the `\u{...}`-escape rule, the two
> colour channels). Where it and an S-spec disagree, this file still wins.
>
> Two items in **§3 (collapsed geometry)** are superseded — that section was
> always banded `NOT YET RATIFIED`, and collapsed has since been decided:
>
> - **§3 constraint 2's `< 24` is superseded by LEDGER D15.** The
>   `> MAX_LEARNABLE_STEP (20)` separation was a *restatement* of a margin S8
>   chose while it was free, not a bound. The acceptance half-band is 10, so the
>   requirement is separation `> 10` and collapsed may be anything under **34**.
> - **§3's other open question is settled by LEDGER D16/D17** — truncate the whole
>   label vs render field 0 only is **moot**. Collapsed is a *width profile*, not a
>   second layout: all three fields survive, narrower — `(title 7, repo 3)` at
>   **30 columns**, chosen from rendered candidates. §9 item 2's "the `< 24`
>   invariant is S8's" goes with it.
>
> Everything else here reads as ratified because it is. The authority for what is
> *true now* — including anything §3 touches — is
> **[`docs/ux/LEDGER.md`](../../ux/LEDGER.md)**. **Do not amend this file** to
> reconcile it; propose the disposition to the coordinator.

---

## 1. How this was decided

Eight rounds of rendered mockups, each one narrowing a single question, with
the maintainer judging from real rows in his own terminal. Three of his
rulings overturned a recommendation of mine, and two of my stated findings
turned out to be wrong (§8). **The method is the asset: render it, look at it,
then decide.** Prose comparison of layout options was consistently misleading —
including to me.

---

## 2. Geometry — expanded

**44 columns.** Measured, not asserted; every row in the preview is verified at
exactly 44.

| cols | field | notes |
|---|---|---|
| 1 | left cap | powerline half-circle `\u{e0b6}`, selected row only |
| 2 | status | the glyph's **colour** is the state |
| 3 | space | |
| 4 | rule | `\u{2502}` in fujiWhite |
| 5 | space | |
| 6 | battery | context level (S7); console mark on a terminal tab |
| 7 | space | |
| 8 | provenance | tinted with the repo ink; **blank** for a main checkout |
| 9 | space | |
| 10–16 | title | 7, filled chip; blank when the session was never renamed |
| 17 | space | |
| 18–24 | repo | 7, tinted text |
| 25 | space | |
| 26–42 | summary | 17 |
| 43 | right margin | |
| 44 | right cap | selected row only |

### 2.1 The gutter is position-locked

Every gutter cell is exactly one column and renders a **space** when its glyph
is absent. A missing glyph must never reflow the row. Consequences:

- like glyphs line up vertically down the whole bar;
- the text always starts at column 10, so the eye scans one column, not a
  ragged edge;
- **a font that drops a glyph degrades to a blank cell, not to a shifted row.**

The current renderer builds its gutter by string concatenation
(the gutter branch of `Bar::render`, `clave-bar/src/main.rs`), so a dropped or double-width glyph
*does* shift text today. This needs an explicit test — `main.rs` is
`test = false`, so nothing would catch a regression.

### 2.2 Cap columns are reserved on every row

The selected row's powerline caps occupy columns 1 and 44. Those columns are
reserved — rendered blank — on **unselected** rows too. Without that, the
selected row's content sits one column right of its neighbours, which violates
§2.1 exactly when the eye is most focused on it. Verified: the title starts at
column 10 whether or not the row is selected.

### 2.3 Fixed-width columns, not separators

Fields are fixed-width and padded. Alignment *is* the separator, which is why
one space suffices where the bar previously spent three on ` · `. The maintainer's
requirement, verbatim: *"the text will always start at the first character of
the text area, rather than shifting columns left or right based on the glyphs."*

### 2.4 What was rejected

- **A branch column.** Proposed at 7 columns, rendered, dropped. The gutter's
  provenance glyph already says worktree/branch/main, and 7 characters of a
  branch name is usually just the prefix convention (`agent/…`, `chore/…`,
  `issue-…`). Dropping it moved the summary from 9 columns to 17 — the summary
  is the field you actually read.
- **` · ` separators with fixed columns.** At 44 they leave the summary 3
  columns.
- **A 3-cell gutter** (S6 as written). Superseded.

---

## 3. Geometry — collapsed  ⚠ NOT YET RATIFIED

**This is the one number still open.** Every collapsed candidate rendered
during the design rounds used the *old* 6-column gutter; the gutter has since
grown to 9 columns of lead-in (cols 1–9). The collapsed layout must be
re-rendered before it is fixed.

Two hard constraints already established:

1. **`COLLAPSED_TARGET_COLS = 4` is unreachable.** Its own doc-comment
   (the doc-comment on `COLLAPSED_TARGET_COLS`, `clave-bar/src/model.rs`) says zellij's resize floor may stop the seek above it
   and *"wherever cols stop changing is accepted"*. On the maintainer's window
   it rests at **11**. So today's collapsed width is not designed — it is
   whatever the window's granularity floor happens to be. Setting a reachable
   target makes it deterministic. **S6 §2.8's entire "budget 0, no text"
   analysis was reasoning about a width that never occurs.**
2. **The separation invariant** `BAR_TARGET_COLS − COLLAPSED_TARGET_COLS >
   MAX_LEARNABLE_STEP (20)` (S8 §2.2 item 9) must hold so no learned step lets one
   target's acceptance band swallow the other. At 44 this means the collapsed
   target must be **< 24**.
   **⚠ Superseded — LEDGER D15: this is a restatement, not a bound. The
   separation requirement is `> 10`, so the bound is `< 34`. Collapsed is 30.**

Also unresolved: whether collapsed truncates the whole label or renders
**field 0 only** (title, falling back to repo).
**⚠ Settled — LEDGER D16/D17: moot. Collapsed is a width profile that keeps all
three fields, `(title 7, repo 3)` at 30 columns.**
Field-0-only was rendered and
read better — at 8 text cells, truncating the whole label spends a column on an
ellipsis carrying no information (`dotfile…`), where field-0-only gives eight
real characters (`dotfiles`).

**Sequencing consequence:** any collapsed target that needs the 44-column
expanded width to satisfy the invariant cannot land before S8. At the old
30-column width, a 13-column collapsed target fails (`30 − 13 = 17`, not > 20).

---

## 4. Colour

Two channels, deliberately distinct, because they answer different questions.

| channel | render | keyed by | question it answers |
|---|---|---|---|
| **repo** | tinted **text** | repo root | *which project is this?* |
| **title** | filled **chip**, dark text | title, scoped within its repo | *which of my tabs is this?* |

So one repo is one colour everywhere and forever, while two tabs of the same
repo never share a chip. Maintainer's ruling, verbatim: *"the title should get
its own unique colour to differentiate it from other titles within that repo.
This makes every tab visually identifiable in a heartbeat."*

**Palette: 8 kanagawa hues, allocated round-robin** — crystalBlue, springGreen,
carpYellow, waveRed, oniViolet, waveAqua2, surimiOrange, sakuraPink. Twelve was
rendered first and rejected: *"they start colliding after the 5th colour."*

**Hashing is overruled**, twice over. `DefaultHasher` is not stable across
toolchains, so colours would reshuffle on a `rustc` upgrade; and the maintainer
rejected hashing outright — *"hashes could collide"* — in favour of store-backed
iterate-and-wrap allocation. That puts allocation on the **cross-process/IPC**
row of the risk taxonomy: it owes an ordering/idempotency argument and an
adversarial reviewer.

### 4.1 Colours the gutter may not take

- **status cell** — its colour *is* the status. Overloading it deletes a signal.
- **battery cell** — its colour is S7's magnitude ramp.
- **provenance cell** — takes the repo ink. This is the one gutter cell
  permitted an arbitrary RGB, and it is deliberate: it makes repo identity a
  shape in the gutter as well as a colour in the text.

---

## 5. Glyphs

| cell | glyph | codepoint | provenance of the choice |
|---|---|---|---|
| status | `●` | U+25CF | Idle · Working · NeedsYou · Done — the **colour** is the state (`Status::glyph`, `clave-types`) |
| status | `✖` | U+2716 | `Status::Failed`. Note U+2716 **heavy** multiplication x — not U+2717 |
| status | `✗` | U+2717 | the **stale** row flag, set when `clave open` finds the cwd missing. A flag, not a `Status` — see `BarModel::rows` |
| status | `◌` | U+25CC | a dormant row |
| status | `↻` | U+21BB | an open in flight (`opening`) |
| rule | `│` | U+2502 | new — separates the status hue from the battery hue |
| battery | nf-md-battery ramp | U+F0079, U+F0082…U+F007A, U+F008E | S7 (#62). **Eleven** fill steps, not five: `md-battery` full, `md-battery_90`…`md-battery_10` descending (note they run BACKWARDS through the codepoints), then `md-battery_outline` empty. Verified by parsing the installed patched font's glyph-name table, not assumed |
| terminal | nf-md-console | U+F018D | replaces the terminal mark; a terminal has no context window, so it takes the battery cell |
| worktree | bamum tree | U+168C2 | **invention** — no convention exists |
| branch | nf-md-source_branch | U+F062C | **convention** — lazygit's default, and the Plane-15 carrier of the U+E0A0 powerline-branch shape used by starship, oh-my-posh and p10k |
| main checkout | *nothing* | — | **convention** — no surveyed tool marks the default branch with a glyph |
| caps | powerline half-circle thick | U+E0B6 / U+E0B4 | selected row only |

### 5.1 Why a main checkout shows nothing

A survey of starship, oh-my-posh, powerlevel10k, lazygit, gitui, gitmux, eza,
VS Code, JetBrains, GitHub/GitLab, octicons, codicons and the Nerd Fonts glyph
list found that **essentially no tool marks the default branch with a glyph**.
The three that distinguish it use colour (eza: green main / yellow other) or a
text badge (GitLab `default`, lazygit `(main)`). A positive marker would be a
novel invention that earns no free recognition — and blanking the cell makes
the two *marked* states actually mean something, since a main checkout is the
most common row.

### 5.2 There is no worktree glyph, anywhere

Zero hits for "worktree" across Nerd Fonts' 10,764 glyph names, all 380
octicons, and all 639 codicons. Exactly one surveyed tool draws one: lazygit,
using `\u{f0339}` nf-md-link_variant (a chain link). Git's own marker is `+` in
cyan. The bamum tree is therefore an invention chosen on legibility, with
link_variant as the precedent-backed alternative if it ever needs to change.

### 5.3 No `glyphs` plugin-config key — glyphs are compiled in

S6 proposed putting the glyph choice behind a plugin config key so a candidate
could be swapped without a rebuild. **Drop it**, for two independent reasons:

1. **It is unnecessary.** The key existed to hedge against a font not carrying
   a glyph. Every candidate renders on the target terminal — the round-1 probe
   that suggested otherwise was an encoding bug (§8.2), not a coverage gap.
2. **It is dangerous.** zellij hashes plugin identity over the **whole config
   map**, so a key that does not match **starts a new plugin instance** rather
   than reusing the running one. That is the mechanism behind the v0.1.1
   double-sidebar incident (#43/#44). Found independently by the pre-fleet
   audit (2026-07-25) as blocker 4 against S6.

User-facing glyph customisation folds into **#40**, where it can be designed
with the identity-hash hazard in view.

### 5.4 GLYPH RULE — load-bearing

**Write every glyph as a `\u{...}` escape in source, never as a literal
character.** During these rounds, literal glyphs were silently lost in transit
twice. The first time we misread the result as missing font coverage and nearly
constrained the entire design to one Unicode plane on the strength of it (§8.2).
Escapes survive every tool in the chain; literals do not, and the failure mode
is tofu in production from a diff that looked clean.

---

## 6. The selected row

**Powerline half-circle caps + waveBlue2 `#2D4F67` background + every other row
faded 25% toward the bar background.**

The fade is the load-bearing part. Selection by *recession* rather than by
ornament costs zero columns, adds no new signal to compete with the colour
system, and gets **more** effective as the fleet grows — which is the opposite
of a background tint, which competes with the title chips and repo inks for the
same channel. That competition is why a background alone read as insufficient.

Rejected, having been rendered: full SGR-7 reverse (inverts the title chip,
which is why the current selected row looks muddy); brighter backgrounds
(dragonBlue, crystalBlue and carpYellow with dark text); a dedicated marker
column with `█ ▌ ▐ ▶ ❯ ┃`; underline; and fades at 8/12/15/20/30/40%.

**A row background must span all 44 columns**, including the pad after a short
summary — resetting at end-of-text leaves a ragged selection. Worth a test.

---

## 7. Row text — and what is deferred

### 7.1 A live row renders from the STORE, not from the zellij tab name — RULED

**Ruling, 2026-07-25.** `BarModel::rows` takes a live row's title, repo and
summary from `agent_in_tab(t.tab_id)` — which it *already calls* on that same
line to pick the status glyph. The zellij tab name is used only for a
**terminal tab**, which has no agent record to read.

**Why the fixed-column design forces this.** A tab name is one opaque string
with no field structure. You cannot slot it into three fixed-width columns, so
a manually renamed tab would have to fall back to a single truncated line —
losing the column alignment that §2 is built on, for precisely the tabs the
user cared enough to name. Rendering from the store keeps every row aligned.

**What this DELETES.** `InkSpan` existed to say *"paint the Nth ` · `-delimited
field of this row's name"* — a mechanism for locating fields inside a composed
string by parsing it. The bar now lays the columns out itself and needs the
**values** plus two ink values, not positions. So S5 loses: the `InkSpan` type,
`segment_span`, the "title is optional so repo is field 0 or 1" index
arithmetic, and `snapshot_ink_segments_match_compose_label_fields`. The
mis-point bug this section previously agonised over cannot occur, because
nothing parses a name any more.

**What this REQUIRES — and it is a hard prerequisite.** `Agent` (`clave-types`)
carries `label`, the *composed* string, and has **no `title` and no `summary`
field**. The snapshot must carry them structurally before S5 or S6 can render a
column. That is issue **#69 (AgentSnapshot v2)**, filed independently by the
pre-fleet audit on the grounds that five specs each add a field to the same
struct with nobody owning it. This ruling upgrades #69 from hygiene to a
**blocker for S5 and S6**.

**Landed 2026-07-28 (#69).** `Agent` now carries `title`, `summary` and
`worktree` structurally — see
`docs/superpowers/specs/2026-07-28-agentsnapshot-v2-design.md`. The
prerequisite this section names is met; S5 and S6 are unblocked.

**What this does NOT change.** `Effect::RenameTab` still writes clave's label
onto the real zellij tab — that is what zellij's own tab bar shows, and it is
untouched. The rename loop-guard still fires on label *change* only, so a manual
rename still sticks on the tab itself, and
`rename_only_when_label_changes_not_when_tab_name_differs` remains valid and
should not be deleted. What changes is only that **the sidebar shows clave's
view of a session**, not zellij's. Accepted cost: a manual `zellij` rename no
longer appears in the sidebar.

**Sequencing note.** This edits `BarModel::rows`, which S0, S1 and S3 also
rewrite. It is a fourth writer to that function — the pre-fleet audit already
flags `rows()`/`apply_tabs` as a multi-workstream collision zone.

### 7.2 Deferred, with a note

- **Terminal tabs need a real identity.** `Tab #16` is a placeholder. They
  should carry cwd and branch, with the same provenance glyphs as agent rows,
  and the console mark in place of the battery. Tracked as future work.
- ~~**Dormant rows and the battery.**~~ **SETTLED (#62, 2026-08-01): full ramp
  colour, same as a live row.** This entry's premise was backwards, which is why
  it read as a hard call. A dormant conversation consumes nothing, so its stored
  figure is *exactly* its current occupancy — it is the **live** row whose
  reading is always a turn behind. There is nothing stale to mark. It is also
  the reading that earns its column: "resuming this one starts you back at 140k"
  is what the eye wants before choosing where to return.

  Dimming was rejected on §4.1 grounds: it would put liveness into a cell whose
  colour *is* the magnitude ramp, duplicating what the status glyph says one
  column to its left. Absent was rejected for discarding a correct figure and
  blanking most of the bar, since most rows in a fleet are dormant at any moment.

  Carries a standing performance rule with it: **a dormant row costs nothing.**
  The level is stamped when that row's own agent reports; projection copies it.
  No dormant row triggers a read, a parse or a computation, because that list
  grows toward every conversation the user has ever had.

---

## 8. Two corrections to earlier findings

Recorded because both were stated confidently and both were wrong.

### 8.1 `COLLAPSED_TARGET_COLS = 4` is not a width the bar ever has

See §3. S6 §2.8 costed four options against a "budget 0" collapsed row. That
width does not occur; zellij's floor stops the seek above it, window-dependent.
Any collapsed reasoning that predates this file is reasoning about a phantom.

### 8.2 The "Plane 15 only" font rule was an encoding bug, not a font fact

Round 1's probe showed every BMP Private Use Area glyph blank and every
Plane-15 glyph rendering, from which a rule was inferred: *this terminal
supports `nf-md-*` and not `nf-pl-*`/`nf-dev-*`/`nf-fa-*`/`nf-oct-*`*. That rule
was wrong. The blanks were codepoints lost between writing the probe script and
running it — **not** missing font coverage. A corrected probe showed **every
candidate rendering**, including U+E0A0, which is why powerline caps are in the
final design at all. The lasting output of the mistake is §5.4.

---

## 9. What this obligates

1. **S6** (`2026-07-22-S6-gutter-glyphs.md`) — revise: the gutter is 4 cells
   plus a rule, not 3; §2.8's collapsed analysis is void (§3); the glyph set is
   settled (§5); the escape rule is a requirement (§5.4); drop the `glyphs`
   config key (§5.3).
2. **S8** (`2026-07-22-S8-sidebar-width.md`) — revise: the target is **44**, not
   38 and not 30. Re-derive the expected-red test set; the mechanical-replace
   hazard it documents still applies (`30` appears both as the width target and
   as an arbitrary start width, and `seek_waits_for_inflight_resizes_and_zellijs_floor`
   must be left alone). Collapsed target and the `< 24` invariant are S8's.
3. **S5** (`2026-07-22-S5-per-repo-colour.md`) — revise: palette is 8 kanagawa
   entries, not 12; the title channel is a **chip**, not tinted text; and
   **delete `InkSpan`, `segment_span`, the optional-title index arithmetic and
   `snapshot_ink_segments_match_compose_label_fields`** — §7.1 removes the
   parse-a-composed-name mechanism they exist to serve.
4. **#69 (AgentSnapshot v2) is now a BLOCKER for S5 and S6**, not hygiene.
   §7.1 needs `title` and `summary` as structural fields on `Agent`; today the
   wire format carries only the composed `label`.
5. **A gutter-invariance test** — position-locked cells, blank on absence,
   reserved cap columns, and a full-width selected background. None of this is
   covered today because `main.rs` is `test = false`.
6. **PR #64 must merge before any worktree starts.** As committed it describes a
   3-cell gutter, a 38-column bar and a zero-text collapsed mode — all three
   overturned here.
