# S6 — the three-cell gutter: status · context battery · worktree marker (slices #24, opens #40)

_2026-07-22 · workstream **S6** · builds on **RC-G** of
[`2026-07-22-ux-defect-dossier.md`](2026-07-22-ux-defect-dossier.md) · feature,
not a defect · main `50fa26a`_

**The requirement, verbatim from the maintainer:**

> ```
> ● 󰁼 𖣂 F-CLA · clave · <summary>
> ```
>
> "The first three glyphs will go in the same gutter that the dot glyph is in."
>
> "`\u{…}` should replace the battery for terminal tabs." *(a Nerd Font terminal
> icon — see §2.6.4, the codepoint is BMP-PUA and is confirmed by probe, not
> guessed)*
>
> "Nerd fonts work in the terminal here it seems, I can see the battery icon."

Read RC-G (`2026-07-22-ux-defect-dossier.md:440-499`) and **S5 §2.7, §3.3**
(`2026-07-22-S5-per-repo-colour.md:300-395`) first. The render survey, the
escape-blind clamp, the `Row` shape and the `compose_row` / `render_segments`
seam are **not** re-derived here — S6 extends that seam, it does not invent one.

S6 owns **the gutter**: everything left of the first character of row text, and
the arithmetic that fixes where that first character sits. S4 and S5 own the text
after it. The seam between them is one number, §2.10.

## What this closes in #24 / #40

| Item | S6 |
|---|---|
| #24 item 4 "context battery per row" | **slot reserved, not filled.** The cell renders blank and is proven not to reflow when populated (§2.3, §4.3 G2). Filling it is **S7** |
| #24 locked format, marker `𖣂` | **closed** — cell 3 (§2.4), with the font finding in §2.6.3 |
| #24 item 7 "collapsed-state design: what 4 cols can still distinguish" | **answered, and the answer is a maintainer decision** — §2.8 costs four options and recommends one. The shipped default puts three signal cells in exactly 4 columns, replacing today's single glyph plus a stray `…`, but it *drops* item 7's "repo colour" clause. §5 Step 6 is where he rules |
| #24 item 1 "worktree provenance in the name" | **not closed.** S6 renders a *marker*, not the worktree's name. The marker's signal is also incomplete — §2.4.3 states exactly where, and why S6 does not fix it |
| #24 items 2, 3, 5, 6 | untouched (2 = S5, 3 = S4, 5 unstarted, 6 = S8) |
| #40 "Nerd Font dependency" | **partially** — S6 inventories every glyph it renders, measures each one's cell width, and ships the fallback tier plus the doctor advisory (§2.6). It does not audit S7's ramp or a model badge |

---

## 1. Problem and goal

**The problem.** A row's only non-text signal today is one status dot in a
two-cell gutter (`crates/clave-bar/src/main.rs:539-543`):

```rust
let gutter = match row.glyph {
    Some((glyph, colour)) => format!("\u{1b}[{colour}m{glyph}\u{1b}[0m "),
    None => "  ".to_string(),
};
```

Everything else a row could say — how much context the agent has burned, whether
it is a worktree or the main checkout, whether it is an agent at all — must
compete for the same 27 text columns that already cannot hold
`issue-10-kdl-guardrail · -` (#24, S4 §1.4). Colour (S5) frees *one* channel.
The gutter frees three, at a fixed and knowable cost.

**The goal.** A gutter of exactly **three glyph cells** in fixed order — status,
context battery, worktree marker — whose total width is a constant that does not
depend on which cells are occupied, on the bar's width, on the row's kind, or on
whether the user has a patched font. Text begins in the same column on every row
of every kind, always.

**The one thing that can go wrong, and it is the whole spec.** A gutter cell that
is two terminal columns instead of one shifts every row's text by one column, on
that row only, and nothing in the codebase would notice: `main.rs` is
`test = false` (`crates/clave-bar/Cargo.toml:25`) and the existing clamp counts
`str::chars()`, which is a count of Unicode scalars and not a count of terminal
cells. §2.2 determines the width of every glyph rather than assuming it, and §4
turns the determination into a test that runs on every commit.

**Non-goal.** The gutter is decoration *stacked on* signals that already exist in
text: dormancy is still a distinct glyph shape (S3), status is still a colour
*and* a shape, worktree provenance still lives in the branch segment (S4 §2, #24
item 1). Nothing becomes glyph-only.

---

## 2. Design

### 2.1 The gutter, in cells

Three cells, fixed order, one space after each. **Expanded gutter = 6 columns.**

| # | Cell | Glyph (Full tier) | Codepoint | Cells | When blank | Ink |
|---|---|---|---|---|---|---|
| 1 | **status** | `●` / `✖` / `○` | U+25CF, U+2716, U+25CB | 1 | plain terminal tab → 1 space | basic SGR `31/33/32/90` (`Status::glyph`, `clave-types/src/lib.rs:24-32`); dormant `90` (S3 `DORMANT_GLYPH`) |
| 2 | **context battery** | *reserved — always blank in this batch*; plain terminal tab → terminal mark | S7 decides (§2.6.4); terminal mark BMP-PUA, probe-confirmed | 1 | agent row → 1 space, until S7 | S7: basic SGR ramp; terminal mark `90` |
| 3 | **worktree** | `𖣂` | U+168C2 | 1 | not a worktree, or not an agent → 1 space | basic SGR `90` |
| — | separator | `' '` ×1 after each cell | U+0020 | 3 | never | none |

Worked rows at the maintainer's format (`␣` marks a deliberate blank cell):

```
cols ->  0123456789…
         ● ␣ 𖣂 F-CLA · clave · fix the flaky…     agent, worktree, battery pending
         ● ␣ ␣ other-repo · main · bump deps      agent, main checkout
         ␣ ⌸ ␣ shell                              plain terminal tab (⌸ = terminal mark)
         ○ ␣ 𖣂 clave · F-CLA · clave/ab12cd34     dormant worktree row
```

Text begins at column 6 on all four. That is the invariant.

**Why one space after each cell rather than a packed `●󰁼𖣂`.** Three reasons, in
weight order: the maintainer's format is spaced and it is his row; adjacent icon
glyphs from a patched font have tight side bearings and read as one compound mark
at terminal font sizes; and a following space is the only cheap defence against
Nerd Font *overdraw* (§2.2.3) — an icon drawn wider than its cell bleeds into a
space instead of over a neighbouring glyph. The packed form is not discarded, it
is the **collapsed** gutter (§2.8), and switching the expanded gutter to it is a
one-constant change that §4.3 G1 already covers.

**The gutter is never reverse-videoed.** Today `\x1b[7m` opens *after* the gutter
(`main.rs:554-556`); that is preserved. The active-row highlight marks the text,
so a selected row's glyphs keep their own colours and the highlight's left edge
is a reliable "text starts here" ruler for the maintainer's own eye — which
matters for live validation Step 4.

### 2.2 Cell width: determined, not assumed

This is the highest-risk detail in the spec. Three independent layers have to
agree that each glyph is one cell.

#### 2.2.1 What actually decides the width

The bar `println!`s into a zellij **plugin pane**. Zellij's server parses that
byte stream into its grid and then re-emits the grid to the host terminal. So the
column a character lands in is decided **twice**: once by zellij's width table,
once by the host terminal emulator's. Cell width is a property of those *tables*,
not of the font — a font missing a glyph draws tofu **inside one cell** and the
geometry survives; §2.6.3 is about legibility, this section is about geometry.

Zellij's table is knowable from source: `zellij-utils-0.44.3/Cargo.toml:185-187`
declares `unicode-width = "0.1.8", default-features = false`, and it is used at
`zellij-utils-0.44.3/src/data.rs:22` (`UnicodeWidthChar`) and
`src/shared.rs:12` (`UnicodeWidthStr`). `zellij-server` is not vendored, so the
grid's own call site cannot be read — but the workspace resolves one
`unicode-width` and the crate has one non-CJK width function.

#### 2.2.2 The measurement

Measured, not recalled: a throwaway crate against the resolved `unicode-width`,
run for `0.1.12` and again for `0.2.0` (both present in the local registry) to
prove the answer is not table-version-dependent. Python's `unicodedata`
(Unicode 15.1.0) supplied the East Asian Width class independently.

| Glyph | Codepoint | Block | EAW | `width()` | `width_cjk()` |
|---|---|---|---|---|---|
| `●` | U+25CF | Geometric Shapes | A | **1** | 2 |
| `✖` | U+2716 | Dingbats | N | **1** | 1 |
| `○` | U+25CB (S3) | Geometric Shapes | A | **1** | 2 |
| `◌` | U+25CC (today) | Geometric Shapes | N | **1** | 1 |
| `✗` | U+2717 | Dingbats | N | **1** | 1 |
| `↻` | U+21BB | Arrows | N | **1** | 1 |
| `𖣂` | U+168C2 | **Bamum Supplement** | N | **1** | **1** |
| MDI battery | U+F007C | SPUA-A (Nerd Fonts MDI) | A | **1** | 2 |
| terminal mark | BMP-PUA (§2.6.4) | PUA | A | **1** | 2 |
| block eighths | U+2581–U+2588 | Block Elements | A | **1** | 2 |
| `…` | U+2026 | General Punctuation | A | **1** | 2 |
| `·` | U+00B7 | Latin-1 Supplement | A | **1** | 2 |
| `' '` | U+0020 | ASCII | Na | **1** | 1 |

**Every glyph the gutter can render is one cell.** Two consequences that are the
actual design decisions:

1. **The gutter's width is `3 + 3 = 6`, a constant**, and `str::chars().count()`
   equals the display width *for the gutter* — so S6 does **not** need to fix the
   clamp's scalar-vs-cell bug to be correct. It states that bug's boundary
   instead (§2.9.3) and leaves it where it is.
2. **The CJK column is the failure mode.** Under `width_cjk()` the gutter is
   *mixed*: `𖣂` and `✖` stay 1 while `●` and the battery become 2. There is no
   gutter width that is stable under an ambiguous-wide table, so the design
   **declares its dependency** rather than defending against it: clave requires
   the terminal's East-Asian-ambiguous setting to be *narrow*, which is the
   default everywhere and is already load-bearing today (`●`, `…` and `·` all
   render today, and today's 2-cell gutter is already wrong under `width_cjk`).
   Live validation Step 2 measures it directly; the branch table names the
   terminal setting to change.

#### 2.2.3 Nerd Font overdraw is not reflow — and the exception

A patched font's non-`Mono` variants ("Nerd Font", "Nerd Font Propo") draw many
icons with a **double advance width**. The terminal still allocates cells from
its width table, so the next character still starts at column N+1 — the icon
*bleeds over* the neighbour rather than pushing it. That is a smear, not a shift,
and the trailing space in each cell pair is where it lands.

**The exception that must be checked live:** some emulators (kitty, WezTerm,
Ghostty in some configurations) measure the *font's* advance for PUA codepoints
and allocate two cells. If the maintainer's terminal does that, the battery cell
and the terminal mark become 2 cells while `𖣂` stays 1 — a per-row shift. This
cannot be settled from source; it is Step 2's job, and Step 2's branch table
resolves it to the `Mono` font variant or to the `plain` tier (§2.6).

### 2.3 Blank cells occupy space, exactly

A blank cell is emitted as **one U+0020 with no ink**, and its separator space is
emitted unconditionally. Never an empty string, never a skipped separator.

Concretely, the gutter is always this sequence and never a shorter one:

```
expanded (6):   [cell0][' '][cell1][' '][cell2][' ']
collapsed (4):  [cell0][cell1][cell2][' ']
```

where `cellN` is a glyph char or `' '`. S5's `compose_row` push helper skips
empty text (`S5 §3.5`, the `push` closure) — but the gutter never reaches that
closure: S6 builds it in `gutter_segments` (§3.5) as one `Segment` per cell, and
a blank cell's text is a space, never `""`. Emptiness cannot arise. §4.1
(`blank_cells_are_spaces_not_omissions`) pins it anyway, because the failure is
invisible until a column moves.

**This is what "reserve the slot" means and what S7 inherits:** cell 1 blank on a
terminal tab, cell 2 blank on every agent row until S7 lands, cell 3 blank on a
main-checkout row — all three are the same code path, and §4.3 G2 proves over
generated input that occupancy never changes the gutter's width.

### 2.4 The worktree signal

#### 2.4.1 The field exists and the plugin never sees it — confirmed

`AgentRecord.worktree: Option<String>` — `crates/clave/src/store.rs:54-55`,
*"Worktree path if `clave add --worktree` created one (§6.3), else None."*

`snapshot_from` (`store.rs:166-189`) maps ten fields onto `clave_types::Agent`
and `worktree` is not among them:

```rust
/// Store → pipe snapshot (§5): drop the store-only fields, keep the order.
pub fn snapshot_from(store: &Store) -> AgentSnapshot {
    …
            .map(|r| Agent {
                uuid: r.uuid.clone(),
                cwd: r.cwd.clone(),
                repo_root: r.repo_root.clone(),
                branch: r.branch.clone(),
                label: r.label.clone(),
                status: r.status,
                last_interacted: r.last_interacted,
                last_visited: r.last_visited,
                tab_id: r.tab_id,
                stale: r.stale,
            })
```

The struct doc says so out loud (`store.rs:35-37`): *"plus store-only fields
(`worktree`, `label_source`) that the plugin never needs to see."* The dossier
records the same from the other end (`:540-541`). **Confirmed: the plugin cannot
know today.**

#### 2.4.2 The plumbing

`Agent` gains `#[serde(default)] pub worktree: Option<String>` and
`snapshot_from` carries `r.worktree.clone()`. `Option<String>`, not `bool`:

- #24 item 1 wants `<repo> » <worktree-dir>` in the *name*, which needs the path.
  A `bool` would force a second wire change for the same fact.
- The wire already carries `cwd` and `repo_root` in full; a worktree path adds no
  new class of information.
- `#[serde(default)]` means an old `clave` pushing to a new bar yields `None` (no
  marker — a missing hint, never a wrong one), and a new `clave` pushing to an
  old bar is ignored. This is exactly the #43/#44 mixed-binary window the
  `tab_id` and `stale` fields were designed for (`clave-types/src/lib.rs:60-67`),
  and `agent_worktree_roundtrips_and_defaults_none` pins it the same way.

The bar derives `Row.worktree: Option<(char, u8)>` — `Some(mark)` when
`a.worktree.is_some()`, `None` otherwise. Resolving the *character* in `rows()`
(where `self.glyphs` lives) rather than in `compose_row` is what keeps
`compose_row` a pure geometry function with no knowledge of font tiers (§2.9.1).

#### 2.4.3 Is the signal trustworthy? Precisely: it is sound, not complete

`worktree` has exactly one writer in production, `add.rs:750`
(`worktree: worktree_path.clone()`), and `worktree_path` is `Some` only in the
`new` arm when `--worktree` was passed and clave itself ran
`git worktree add -b clave/<short>` (`add.rs:647-668`). Plus `dev.rs:242`
(scenario seeding) and `merge_resume_record`'s `..row.clone()`
(`add.rs:343-354`), which preserves it across a resume for free.

| Case | Recorded | Marker | Verdict |
|---|---|---|---|
| `clave add --worktree` → `new` | `Some(path)` | shown | **correct** |
| resume of an agent with a store row | preserved by `..row.clone()` | shown | **correct** |
| `clave add` → `new` in the main checkout | `None` | hidden | **correct** |
| `clave add` → `new` from *inside* a linked worktree | `None` | hidden | **false negative** |
| resume of a **jsonl-only** worktree session (no store row) | `None` — `fresh` is stored as-is (`merge_resume_record`'s `None` arm) | hidden | **false negative**, even though the picker computed `is_worktree` for that very candidate at `add.rs:577` and rendered it as `(wt)` at `add.rs:301-306` |
| a worktree deleted after the row was created | still `Some(path)` | shown | stale, but `stale: true` already covers a missing cwd (`open.rs:16`) |

**There is no false positive.** The marker's honest semantics is *"clave created
a worktree for this agent"* — provenance clave is certain of. A missing marker is
a missing hint; a wrong marker would be a lie. Shipping the sound half is safe.

**But the false-negative rate is real, not theoretical.** The maintainer's own
reported case, `issue-10-kdl-guardrail`, is a *named* worktree not created by
`clave add --worktree` (clave's own naming is `.claude-worktrees/<8-hex>` on
branch `clave/<8-hex>`, `add.rs:648-664`). His daily fleet contains worktrees S6
will not mark.

**The correct long-term source, and why S6 does not build it.** Git's definitive
test is one `stat`: in a linked worktree `.git` is a **file** containing
`gitdir: <path>`; in a main checkout it is a directory. S4 is already building
exactly that walk — `crates/clave/src/head.rs::git_dir_for`, S4 §4.2 steps 2–3.
So the accurate signal is three lines inside S4's `refresh_label`, not a new
subsystem. S6 declines to write them because:

1. They live in `add.rs`'s record-construction block and `hook.rs`'s
   `refresh_label` — **both of which S4 is restructuring** (S4 §4.3(c), §4.5(a)
   moves the `AgentRecord` construction above the layout write). Two workstreams
   editing that block guarantees the conflict the dossier's split exists to
   avoid (`:548-551`).
2. It changes the field's *meaning* from "clave made this worktree" to "this cwd
   is a linked worktree", which is a store-semantics decision with a resume and a
   picker downstream (`resume_candidates` reads `r.worktree.is_some()` at
   `add.rs:281` to drive the `(wt)` picker suffix). That is #24 item 1's call.
3. **It moves no columns.** The marker's accuracy is a strictly separable upgrade
   that cannot invalidate a single line of S6's geometry or a single test in §4.

**The three-line upgrade, ready to take, if S4 has landed first.** In
`refresh_label`, beside the branch derivation (S4 §4.3(c)):

```rust
// #24 item 1 / S6 §2.4.3: the worktree marker's SOUND-not-complete signal
// becomes complete here. `.git` is a FILE in a linked worktree and a
// DIRECTORY in a main checkout — git's own definitive test, already
// walked by head::git_dir_for. Only ever UPGRADES: never clears a path
// clave itself created.
if rec.worktree.is_none()
    && let Some(cwd) = rec.live_cwd.as_deref()
    && crate::head::is_linked_worktree(std::path::Path::new(cwd))
{
    rec.worktree = Some(cwd.to_string());
}
```

The maintainer's call, recorded in §5 Step 5's branch table, not taken by an
agent.

#### 2.4.4 The dossier's accepted `repo_root` limitation does **not** affect the marker

The dossier and S5 §2.2(4) record that `clave add` → `new` from inside a linked
worktree writes the *worktree's* root into `repo_root`, because the `new` arm
never runs `git worktree list` and `rev-parse --show-toplevel` inside a linked
worktree returns the worktree (`main_worktree_path`, `add.rs:200-206`, exists
only on the resume arm at `add.rs:558-561`). That limitation is about **which
repo a row is grouped and tinted under** — it is S5's colour key and the picker's
filter (`add.rs:275`). The marker reads `worktree`, a different field with a
different writer, so the two failures are independent: that same case is already
in the §2.4.3 table as a false negative for its own separate reason (nothing set
`worktree`), and fixing `repo_root` would not set it either. **No interaction.**

### 2.5 Colour: the rule that keeps gutter and text separable

S5's current revision tints row **text** with **truecolor** (`\x1b[38;2;R;G;Bm`)
from a hardcoded kanagawa palette, allocated host-side and delivered per row as
`clave_types::InkSpan { segment, ink }` (S5 §2.4, §2.7). Its `Ink` enum is
`Sgr(u8) | Rgb(u8, u8, u8) | Indexed(u8)`, the last retained only as a
nearest-cube fallback. The gutter's status dot stays basic SGR (31/33/32/90). The
rule makes the split total, restated for that vocabulary:

> **Rule G/T — every gutter segment carries `Ink::Sgr` or nothing. Every name
> segment carries `Ink::Rgb` (or its `Ink::Indexed` fallback) or nothing.
> Neither family crosses.**

That is not a style guide, it is a **Tier 1 assertion** (§4.1
`gutter_and_text_inks_never_cross`) and a proptest (§4.3 G4). S5 §2.5 reaches the
same conclusion from its side — *"different families, different positions (the
fixed gutter versus the name), and after S6 the gutter is glyphs only"* — so this
is one rule stated twice, not two rules that must be kept in step.

Why this rule and not "pick non-clashing hues":

1. **It is an encoding split, so it cannot rot.** Any future gutter cell is
   basic-SGR by the variant it is allowed to carry; any future text tint is RGB.
   Nobody has to remember a colour table.
2. **It puts the two channels on opposite sides of the user's theme.** Basic SGR
   30–37/90–97 is exactly what a terminal theme remaps; a truecolor triple is
   exactly what it cannot touch. So gutter *state* looks native to whatever theme
   the maintainer runs — which is what you want for red/amber/green semantics —
   while a repo's colour is the same everywhere. They are visually separable
   *because* one moves with the theme and one does not. (This survives S5's v2
   theme sourcing unchanged: v2 swaps `BarModel.palette` for the user's
   `Styling.multiplayer_user_colors`, which are still `Ink::Rgb`, still not basic
   SGR.)
3. **Position already separates them.** The gutter is six fixed columns of
   single glyphs; the tint is a run of letters starting at column 6. As S5 §2.5
   puts it: a kanagawa-red name field and a themed-red status dot sit adjacent
   and are never ambiguous — *"one is a `●`, the other is a word."*

**The one place the rule is contested is collapsed mode**, where there is no
word — see §2.8 option (b).

Per-cell inks:

| Cell | Ink | Reason |
|---|---|---|
| status | `Sgr(31/33/32/90)` — unchanged | `Status::glyph()` is shared with `clave ls` (`lsview.rs`); changing it is a status-vocabulary change with a spec §6.5 blast radius |
| battery (S7) | `Sgr(32→33→31)` — reserved | #24's ramp is green→red; keeping it basic SGR keeps it inside Rule G/T and inside the theme, where "red means bad" already lives |
| worktree | `Sgr(90)` — **dim** | provenance is not state. Dim is the codebase's existing word for "true, but not something to act on" (`Status::Idle`, `DORMANT_GLYPH`), and it keeps the marker from competing with the status dot two columns to its left. **This is the cell §2.8(b) would change**, and the only one that could take a repo ink without ambiguity |
| terminal mark | `Sgr(90)` — dim | same: a plain tab is a thing that exists, not a thing that needs you |
| separators | none | plain U+0020 |

**Collision check against S3.** S3 argues the dormant glyph must leave the status
palette's *shape* class (`◌` → `○`) because dim-90 `●` and dim-90 `◌` read alike
(S3 §I3). S6 adds two more dim-90 marks — but in **different columns**, never in
column 0, and never as circles. A dormant row reads `○ ␣ 𖣂`; an idle live row
reads `● ␣ ␣`. S6 composes with S3 and does not re-collide. §4.1 pins the
property (`gutter_marks_are_not_status_shapes`) rather than the codepoints, so a
later palette or glyph change cannot silently re-collide.

### 2.6 Nerd Font dependency (#40)

#### 2.6.1 The inventory (#40 scope item 1)

Every glyph clave renders, after S6:

| Glyph | Codepoint | Source | Font requirement |
|---|---|---|---|
| `●` `✖` | U+25CF, U+2716 | `Status::glyph()` | none — every monospace font |
| `○` / `◌` `✗` `↻` | U+25CB / U+25CC, U+2717, U+21BB | `model.rs:770-777` | none |
| `…` | U+2026 | the clamp | none |
| `·` | U+00B7 | `LABEL_SEP` | none |
| **battery ramp** | S7 | S7 | **open — §2.6.4** |
| **worktree marker** | U+168C2 | S6 cell 3 | **Bamum Supplement — §2.6.3** |
| **terminal mark** | BMP-PUA | S6 cell 2 | **Nerd Font** |

clave's pre-S6 set is entirely stock Unicode. **S6 is the change that introduces
a font dependency**, which is why #40's fallback is S6's to ship and not S7's to
inherit.

#### 2.6.2 The two tiers

```rust
pub enum GlyphSet { Full, Plain }
```

`Full` (default) uses the maintainer's marks. `Plain` uses only characters
present in every monospace font shipped in the last twenty years:

| Cell | `Full` | `Plain` | Why the `Plain` choice |
|---|---|---|---|
| status | unchanged | unchanged | already stock Unicode; there is nothing to degrade |
| worktree | `𖣂` U+168C2 | `‡` U+2021 | General Punctuation, universal coverage, EAW A → 1 cell, no shape collision with `● ○ ✖ ✗ ↻`, and it reads as "derived from" |
| terminal | BMP-PUA mark | `>` U+003E | ASCII; reads as a shell prompt, which is what the row is |
| battery (S7) | S7's ramp | the block eighths U+2581–U+2588 | #24's own 2026-07-21 ruling, chosen there **for exactly this reason** |

Rejected for `Plain`'s worktree cell: `⑂` U+2442 (OCR Fork — semantically
perfect, coverage in practice limited to DejaVu and the Noto Symbols fonts, i.e.
it fails the one job the `Plain` tier has); `Y`/`y` (reads as text); `+` (reads as
a diff marker).

**The tiers are width-identical.** Every character in both columns is one cell
under `width()` (§2.2.2 — U+2021 and U+003E measured with the rest). Switching
tiers therefore cannot move a column, which is what makes the fallback safe to
flip at any time and what §4.3 G2 proves.

#### 2.6.3 The finding that matters: `𖣂` is the one glyph a Nerd Font does not give you

The maintainer's evidence — *"Nerd fonts work in the terminal here it seems, I
can see the battery icon"* — establishes that his terminal renders **Nerd Font
PUA glyphs**. It establishes nothing about `𖣂`, which is **not** a Nerd Font
glyph: U+168C2 is BAMUM LETTER PHASE-C MBERAE, a real Unicode letter in the
Bamum Supplement block (U+16800–U+16A38), and no Nerd Font patch adds it. In
practice its only common coverage is Noto Sans Bamum. Every other glyph in the
gutter is either stock Unicode (status) or Nerd Font PUA (battery, terminal
mark) — `𖣂` is the sole codepoint that is neither.

The geometry is safe either way (§2.2.1: a missing glyph is tofu in one cell, not
a reflow). The risk is purely that cell 3 renders as `▯` forever.

**S6 ships `𖣂` as the default anyway** — it is the maintainer's explicit pick and
it is one constant — and pre-clears two alternates so that one live probe settles
it in one round rather than three:

| Candidate | Codepoint | Cells | Coverage argument |
|---|---|---|---|
| `𖣂` | U+168C2 | 1 | the maintainer's pick; requires a Bamum-covering font |
| Powerline branch | `\u{e0a0}` | 1 | **the recommendation if `𖣂` tofus.** The oldest and most universally patched PUA glyph there is — present in every Nerd Font *and* every Powerline-patched font — semantically exact (it is *the* git-branch mark), and it is guaranteed by the same condition the battery already relies on |
| `‡` | U+2021 | 1 | the `Plain` tier's mark, usable in `Full` if the maintainer prefers stock Unicode everywhere |

Live validation Step 1 prints all three beside the battery and the maintainer
picks; the change is `WORKTREE_MARK` plus one test.

#### 2.6.4 The battery codepoint is S7's, and #24 has a conflict to resolve

The maintainer's pasted battery is **U+F007C**, inside the Nerd Fonts Material
Design Icons range (U+F0001–U+F1AF0, plane-15 SPUA-A; the `nf-md-battery*` ramp
occupies roughly U+F0079–U+F0083, and U+F007C is a partially-drained cell).

This **contradicts #24's own battery ruling** (2026-07-21), which chose the lower
block eighths U+2581–U+2588 and explicitly rejected *"Nerd Font battery / emoji
(font-gated or double-width, violates plain-Unicode + SSH rules)"*.

S6 does not resolve it — S7 owns the ramp — but S6 must record two things:

1. **Geometry is unaffected either way.** Both are one cell under `width()`
   (§2.2.2, both measured). Whatever S7 picks slots into the reserved cell with
   no reflow.
2. **The MDI battery is one glyph; the eighths are eight.** An eight-level ramp
   carries a magnitude in *shape*, which survives monochrome and survives a
   colour-blind reader; a single battery icon carries magnitude only in colour,
   which is the thing S5 §2.9 forbids for load-bearing signals. That is an
   argument S7 should weigh, and it is the reason #40 flagged this glyph choice
   in the first place.

The **terminal mark** has the same open-codepoint status, for a different and
more mundane reason: **the pasted character could not be recovered with
certainty** while this spec was written. U+F007C and U+168C2 both round-tripped
through the authoring pipeline and were read back exactly; the terminal mark did
not. No inference about its range is drawn from that — it is a property of one
copy-paste path, not of the codepoint — and a spec should not carry a shaky
deduction where a two-second probe gives certainty. Hence S6 ships
`TERMINAL_MARK = '\u{f489}'` (`nf-oct-terminal`) as the placeholder and Step 1's
probe prints the four plausible codepoints — `\u{e795}` (`nf-dev-terminal`),
`\u{f120}` (`nf-fa-terminal`), `\u{f489}` (`nf-oct-terminal`),
`\u{ea85}` (`nf-cod-terminal`) — for the maintainer to identify in one look. All
four measure at one cell, so the constant can be corrected without touching a
single test's geometry.

#### 2.6.5 How the tier is selected — and why not an env var

**Selection: the plugin's own KDL configuration key.** `load()` already receives
the map and discards it (`crates/clave-bar/src/main.rs:342`,
`fn load(&mut self, _config: BTreeMap<String, String>)`). S6 reads
`config.get("glyphs")`; absent or unrecognised ⇒ `Full`.

```kdl
plugin location="file:{wasm}" {
    glyphs "plain"
}
```

Chosen over the alternatives on one property — **every bar instance must agree**:

| Mechanism | Verdict |
|---|---|
| **plugin config key** | **adopted.** N instances, one per tab (`main.rs:20-22`), all loaded from the same layout: identical by construction, no wire change, no store change, no new CLI surface, and **no change to any generated artifact for v1** because absent means `Full`. A user on a stock font adds three lines to `~/.config/clave/layout.kdl` and reloads |
| `CLAVE_GLYPHS` read host-side into the snapshot | rejected for v1. Snapshots are built by `clave add`, `clave hook` (inheriting *Claude's* env), `clave open`, `clave focus` — four different environments. An inconsistent export makes the tier flicker per push. Width is invariant so the flicker would be cosmetic, but a cosmetic flicker with no off switch is worse than an edit-and-reload |
| a `clave glyphs <tier>` CLI + store field (mirroring `clave collapse`) | rejected as premature. It is the *right* end state if the tier ever needs changing without a reload, but it is a CLI-surface taxonomy row (parse pin + sandboxed e2e) for a once-per-machine decision |
| autodetect the font | rejected — impossible. See §2.6.6 |

**The known gap, stated rather than hidden:** `clave setup` regenerates
`layout.kdl` wholesale (`setup.rs:177`, `:215`) and `add.rs:109` generates the
per-tab layout, so a hand-edited key is lost on the next `setup` and never
reaches new tabs. The complete fix is *"`clave setup` reads `CLAVE_GLYPHS` from
its own environment once and bakes the key into all three plugin blocks"* — a
generated-artifacts change with a guardrail obligation, and the natural body of
work for **#40**. S6 opens the mechanism; #40 makes it stick. §6 records it.

#### 2.6.6 Does a font check belong in `clave doctor`? Yes — as an advisory, never as a check

`clave doctor` exists (`crates/clave/src/doctor.rs`, 1089 lines; `Facts` →
`diagnose()` → `Finding { group, severity, advice }`, `:56-104`). A **font check
must not** be added to it, and the reason is structural, not effort:

- **doctor runs on the machine; the font is chosen by the terminal.** Under SSH —
  a standing design lens for clave — the terminal emulator, its font
  configuration and its width table are on a *different host* that `clave doctor`
  will never see. Every existing doctor probe (`zellij`, `claude`, `git`, `fzf`,
  `XDG_RUNTIME_DIR`) is a property of the machine doctor runs on. A font is not.
- **Installation is not selection.** `fc-list` / `system_profiler
  SPFontsDataType` prove a font is *installed*. They cannot prove the terminal
  profile *uses* it, cannot see per-profile overrides, and on macOS terminals
  routinely use fonts fontconfig does not enumerate.
- **doctor's contract is that a `Problem` is actionable and true.** A check that
  can be confidently wrong in both directions would be the first one that is not
  — and the taxonomy's own rule is that version checks *warn, never halt*
  (`doctor.rs:11-14`).

What S6 adds instead: one `Group::Environment` finding, severity **always `Ok`**,
that reports the resolved tier and hands the human the only reliable test —
their own eyes:

```
Environment
  ok  glyph set: full (Nerd Font glyphs required for the battery and terminal marks)
        check your terminal renders them — run:
            printf 'ruler   [x] [x] [x] [x]\n'
            printf 'status  [\u25CF] [\u2716] [\u25CB]\n'
            printf 'worktree[\U000168C2] [\uE0A0] [\u2021]\n'
            printf 'battery [\U000F007C] [\u2588] [\u2581]\n'
            printf 'terminal[\uF489] [\uE795] [\uEA85] [\uF120]\n'
        every bracket must hold ONE glyph, not a box, and every line's
        brackets must sit in the same columns as the ruler line.
        if any is a box or the brackets misalign, switch to the plain set:
            add `glyphs "plain"` inside the plugin block of ~/.config/clave/layout.kdl
```

This satisfies #40's scope item 4 (*"the doctor/installer track should surface a
font check at setup time"*) honestly: doctor surfaces the question and the
remedy; the human answers it.

### 2.7 Terminal tabs

A plain zellij tab has no joined agent (`agent_in_tab` returns `None`,
`model.rs:747`), therefore no status, no battery and no worktree. Its gutter is:

| Cell | Renders |
|---|---|
| 1 status | **blank** — 1 space. Unchanged in kind from today, where `glyph: None` produces `"  "` |
| 2 battery | **the terminal mark** — the maintainer's ruling |
| 3 worktree | **blank** — 1 space |

The mark sits in the middle column, which looks odd written down and is right on
screen: it is what keeps a terminal row's text starting in the same column as
every agent row's, and it puts a tab's one distinguishing mark where the eye is
already scanning for the battery ramp. Stating it because a reviewer will ask.

`Row.glyph` stays `None` for a plain tab — the distinction *"has an agent"* is
still exactly `glyph.is_some()`, so nothing downstream of `rows()` changes
meaning. §4.1 pins it (`plain_tabs_carry_no_status_and_no_worktree`).

### 2.8 Collapsed mode — **an open decision for the maintainer**

`COLLAPSED_TARGET_COLS = 4` (`model.rs:142`, Alt+c). The expanded 6-column gutter
does not fit, so collapsed uses the **packed** form: three cells, no inter-cell
separators, one trailing space — `GUTTER_COLS_COLLAPSED = 3 + 1 = 4`.

The gutter must fit its pane, which is asserted at compile time — but as an
**inequality**, deliberately, because §2.8.2 may move the collapsed target:

```rust
/// The collapsed gutter must FIT the collapsed pane. Not an equality: §2.8's
/// decision may widen COLLAPSED_TARGET_COLS to buy text back. Fails the BUILD
/// rather than silently clipping the worktree marker off the right edge.
const _: () = assert!(GUTTER_COLS_COLLAPSED <= COLLAPSED_TARGET_COLS);
```

#### 2.8.1 The problem: at 4 columns, three cells leave zero text

With `RIGHT_MARGIN_COLS = 1`, the text budget collapsed is
`4 − 4 − 1 → 0`. §2.9.3 makes a zero budget render *nothing* rather than an `…`,
so a collapsed row is exactly its gutter:

```
today (2-cell gutter, budget 1):     ● …      status, plus a truncation artefact
option (a) (packed gutter, budget 0): ●␣𖣂␣   status + battery slot + worktree
```

**This regresses #24 item 7 as written.** Item 7 asked *"what 4 cols can still
distinguish — glyph + repo colour + battery?"* With no text there is nothing for
S5's repo ink to paint, so **repo identity disappears when collapsed**. S5 flags
this from its side as a medium risk owned by S6/S8 (`S5 §7`, risk table row 1),
and it is right to. It is a genuine loss, not a technicality: the collapsed bar
is exactly the mode in which you have least to go on.

Two arithmetic facts that constrain every option below:

- **Dropping the trailing space buys nothing.** A 3-column gutter still gives
  `4 − 3 − 1 = 0`. The margin, not the separator, is what consumes the last
  column.
- **A budget of 1 shows an ellipsis, not a letter.** `clamp_name`'s
  `take(budget.saturating_sub(1))` means budget 1 → `…`, budget 2 → `c…`. So
  "one visible character of the repo name" needs budget **2**, while "one
  *tinted* cell" needs only budget 1 (a coloured `…` still carries the ink).

#### 2.8.2 The four options, costed

**(a) Accept gutter-only collapsed. — recommended for this batch.**

| | |
|---|---|
| Geometry | gutter 4, budget 0, `COLLAPSED_TARGET_COLS` unchanged at 4 |
| Shows | status · battery slot (S7) · worktree marker |
| Loses | repo identity entirely while collapsed |
| Costs | nothing — no constant moves, no C6 ledger territory, no new glyph |
| Against today | today shows one dot plus a *fixed* `…` on every row (the clamp's `take(0)` degenerate case at `main.rs:547-553` — it is not a truncated name, every row's is identical). Three independent signals replace one signal plus one character of noise |

**(b) Tint a gutter glyph with the repo ink.**

Which cell can take it without ambiguity — the answer is forced:

| Cell | Can it take the repo ink? |
|---|---|
| 1 status | **no.** Its colour *is* the status. S5 already rejected this outright — *"its colour already encodes status; overloading deletes a signal"* (`S5 §2.10`) |
| 2 battery | **no.** Its colour is S7's magnitude ramp (green→red). And it is blank today, so tinting it paints nothing: a coloured *space* is invisible without a background attribute, which is a much louder thing than a foreground tint |
| 3 worktree | **the only candidate** — but only if it always draws something. Today it is a blank space for a main-checkout row, i.e. the majority, and again a coloured space is invisible |

So (b) is really: **cell 3 always draws a provenance glyph** (`𖣂` for a worktree,
some neutral mark for a main checkout) **and carries the repo ink instead of
`Sgr(90)`**. Two sub-variants:

- **(b1) collapsed only** — cell 3 is dim-90 expanded and repo-tinted collapsed.
  Answering the question directly: **yes, this is confusing.** A glyph that
  changes colour when you press Alt+c reads as a state change, which is exactly
  what the gutter's other two cells mean. It also makes the mode flip visually
  loud, and it puts a `Rgb` ink in the gutter in one mode only, so Rule G/T and
  its test become mode-conditional. Not recommended in any circumstance.
- **(b2) always** — cell 3 is repo-tinted in both modes; shape = provenance,
  colour = repo. Consistent, no mode-dependent behaviour, zero width cost, and
  repo identity survives collapse. Costs: **Rule G/T is amended** to "cell 3 is
  the one gutter cell permitted `Ink::Rgb`" (statable and testable, but it is an
  exception where there were none); the marker stops being dim and starts
  competing with the status dot for attention; and it needs a **second new
  glyph** — a main-checkout mark — which is another #40 probe on top of the one
  §2.6.3 has not resolved yet.

The honest counter-argument that applies to (b) in *any* form: at 4 columns,
repo identity can only be carried by colour, so it becomes colour-only —
violating S5 §2.9's *"colour is decoration"* stance for the collapsed mode.
That is not a defect of (b), it is a property of the requirement: #24 item 7's
own wording ("glyph + repo colour + battery") already accepts colour-only
identity at that width. Worth saying out loud so nobody discovers it later.

**(c) Widen the collapsed target.**

| Goal | Needed width | Budget | Shows |
|---|---|---|---|
| one *tinted* cell | `4 + 1 + 1 = 6` | 1 | gutter + a repo-coloured `…` |
| one visible repo *letter*, tinted | `4 + 2 + 1 = 7` | 2 | gutter + e.g. a repo-coloured `c…` |
| two letters | `4 + 3 + 1 = 8` | 3 | gutter + `cl…` |

**S8's invariant is not broken.** S8 pins
`BAR_TARGET_COLS − COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP` (`S8:66`) so no
learned step can make one target's acceptance band swallow the other. With
`MAX_LEARNABLE_STEP = 20` (`model.rs:151`):

- at S8's 38: `38 − C > 20` ⇒ `C < 18`. Every candidate above (6, 7, 8) passes
  with large margin.
- at today's 30, if S6 lands before S8: `30 − C > 20` ⇒ `C < 10`. 6, 7 and 8
  still pass; 10 would not.

So **(c) is cheap on the axis S8 cares about**, and the honest costs are
elsewhere: `COLLAPSED_TARGET_COLS` is a C6 width-seek constant, so moving it is
S8's file and S8's ledger obligation (`SUBSYSTEM-VALIDATION.md` C6, and S8 §2.1
currently lists that constant as explicitly *unchanged*); it makes the mode
chosen to reclaim width ~75 % wider; and at budget 1 the ink lands on an `…`,
which reads as "truncated", not as "this repo".

**(d) Drop a gutter cell when collapsed.**

| Drop | Collapsed gutter | Budget | Verdict |
|---|---|---|---|
| one cell (battery, the blank one) | 2 + 1 = 3 | `4 − 3 − 1 = 0` | **buys nothing** — the margin still eats the last column |
| two cells | 1 + 1 = 2 | 1 | a tinted `…` — but this is today's bar with extra steps, and it throws away both new signals |

**Does it break the width-invariance proptest? No — and that is worth stating
precisely.** §4.3 G1 asserts the gutter equals `gutter_cols(cols, collapsed)`, a
function of the mode, not a single global constant; the spec *already* varies the
gutter by mode (packed vs spaced). G2 is scoped "at the same `cols` and
`collapsed`", which is the correct scoping regardless. What (d) does cost is
semantic: the collapsed bar would show a *different set* of signals from the
expanded one, so the user has to remember which cell vanished — and at 4 columns
it buys zero text anyway. **Dominated by (a).**

#### 2.8.3 Recommendation, and what is left open

**Ship (a).** It costs nothing, it is already a net gain over today, and it does
not spend a C6-ledger constant or a second unresolved glyph on a judgement nobody
has made from real rows yet. **(c) at 7** is the pre-costed upgrade if the
maintainer decides repo identity collapsed is essential — 7 rather than 6 so the
row shows a real tinted letter rather than a tinted ellipsis, at +3 columns, and
it lands in S8's file as a one-constant change with S8's existing seek tests to
re-baseline. **(b2)** is the answer if width is sacred and a second glyph probe is
acceptable. **(b1) and (d) are not recommended in any circumstance** and are
documented so they are not re-proposed.

**This is explicitly the maintainer's call, from real rows.** §5 Step 6 is built
to let him make it: it puts the collapsed bar in front of him with the options
named, and its branch table routes each verdict to the exact change. Nothing in
§3 or §4 depends on the outcome except two constants and two test expectations —
`gutter_sequence` already emits a mode-parameterised cell list, and G1/G2 already
assert against `gutter_cols(...)` rather than a literal.

#### 2.8.4 Mode, not width — and the one degradation

The packed/spaced choice keys on `BarModel.collapsed` (`model.rs:255-257`,
already `pub`), never on `cols`. A `cols`-derived rule was rejected: packed is
always 2 columns cheaper than spaced, so *any* width threshold makes the text
budget non-monotone in `cols` (at the threshold minus one the budget would jump
*up*), and a collapse is a deliberate user act whose discontinuity is the point.

During an **expand** seek the mode flips to expanded while `cols` is still 4, so
for a frame or two a 6-column gutter is asked to fit in 4. `gutter_cols`'s
`want.min(cols)` clips it, which keeps the hard invariant *"the rendered line
never exceeds `cols` cells"* true at **every** width — strengthening S5's P3 from
`cols >= gutter_cols + 2` to unconditional (§4.3).

### 2.9 The seam: S5's `compose_row`, with the gutter handed in

#### 2.9.1 Adopted as S5 now defines it — the gutter is a parameter, not a branch

S5's current revision (`S5 §2.7`, §3.3) settles the seam as

```
compose_row(&Row, cols, gutter: &[Segment]) -> Vec<Segment>
render_segments(&[Segment]) -> String
```

`compose_row` copies the gutter through verbatim, measures it
(`gutter.iter().map(|s| s.text.chars().count()).sum()` — exact, because
`Segment.text` is escape-free by construction), and derives
`budget = cols − gutter_cols − RIGHT_MARGIN_COLS`.

**S6 adopts this and does not extend the signature.** An earlier S6 draft
proposed `compose_row(&Row, cols, collapsed: bool)` with the gutter built inside.
S5's version is better and the reason is S6's own stated goal: `compose_row` is a
*geometry* function, and `collapsed` is *UI state*. Passing the mode in smuggles
state into geometry; passing the finished segments in does not. S6 keeps
everything it wanted — the gutter's construction, its cell order, its inks, its
mode-dependent packing — and gives up only the placement of one function call.

Concretely, S6 replaces exactly one function and one call site:

1. **`model::gutter_segments`** — S5 ships it as a transitional stand-in that
   reproduces today's 2-cell gutter from `row.glyph`
   (`S5 §3.5`, *"S6 replaces this function and nothing else"*). S6 replaces its
   body and widens its signature to `gutter_segments(row: &Row, cols: usize,
   collapsed: bool) -> Vec<Segment>`. **The mode stops at this function.**
2. **The `render()` call site in `main.rs`** — one line, §3.7(b).

`compose_row`, `render_segments`, `Ink`, `Segment`, `clamp_name`, `segment_span`
and the whole text half are **untouched by S6** except for §2.9.3's one deliberate
change to `clamp_name`.

**`REPO_SEGMENT` is gone and S6 needs nothing in its place.** An earlier S6 draft
listed it among the constants it inherits. S5 deleted it (`S5 §2.1`, §2.10): the
title field is optional, so the repo is field 1 *with* a title and field 0
*without*, and no global positional constant can be right — independently of how
S4 orders the grammar. The equivalent information now arrives per row as
`clave_types::InkSpan { segment, ink }`, computed host-side in `snapshot_from`
against the same field order `compose_label` emits. S6 does not read it, does not
depend on it, and must not reintroduce a positional constant.

#### 2.9.2 `Row` becomes three uniform cells

S5's `Row` carries `inks: Vec<(usize, Ink)>` for the text and keeps
`glyph: Option<(char, u8)>` for the gutter's first cell. S6 gives it two more
fields shaped exactly like `glyph`, so the gutter is literally
`[Option<(char, u8)>; 3]`:

```rust
pub glyph: Option<(char, u8)>,      // cell 1 — unchanged field, now named as a cell
pub battery: Option<(char, u8)>,    // cell 2
pub worktree: Option<(char, u8)>,   // cell 3
```

`(char, u8)` throughout, resolved in `rows()`. Three consequences, each
deliberate:

- **Nothing downstream of `gutter_segments` learns about fonts.** `GlyphSet` is
  consumed in `rows()`, which has `self`; `gutter_segments` lays out three
  optional cells and cannot tell `Full` from `Plain`, and `compose_row` sees only
  finished `Segment`s. That is why §2.6.2's "tiers are width-identical" claim
  needs no code to enforce it.
- **Nothing learns about the battery ramp either.** S7 changes only the producer
  in `rows()`; `Option<(char, u8)>` is already the whole contract. This is what
  "reserve the slot" buys.
- **The plain-tab terminal mark rides `battery`.** `battery` names the cell, not
  the semantics; the doc comment says so.

#### 2.9.3 The one behaviour S6 changes in S5's text half — and why it must

S5 §2.7 preserves today's `budget == 0` behaviour verbatim and pins it:

```rust
/// The `budget == 0` case emits a single `…` — one cell more than the
/// budget. That is PRE-EXISTING behaviour (`main.rs:547-553`), preserved
/// verbatim and pinned by a test so it cannot silently change under S5.
fn clamp_name(name: &str, budget: usize) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() <= budget {
        return clean;
    }
    let mut n: String = clean.chars().take(budget.saturating_sub(1)).collect();
    n.push('…');
    n
}
```

**S6 changes it to emit nothing at `budget == 0`:**

```rust
    if budget == 0 {
        // S6 §2.9.3: with a 3-cell gutter, a ZERO budget stops being an
        // unreachable edge and becomes the NORMAL collapsed row (the packed
        // gutter fills COLLAPSED_TARGET_COLS exactly under §2.8 option (a)).
        // The pre-existing `take(0) + '…'` would then overflow the collapsed
        // pane by one cell on EVERY row, every render. Emitting nothing also
        // makes P3 (`visible width <= cols`) unconditional.
        return String::new();
    }
```

This is a cross-workstream change and is called out as one: S5's
`compose_row_narrow_width_overflow_is_preexisting` becomes
`compose_row_emits_no_ellipsis_at_zero_budget`, and S5's P3 loses its
`cols >= gutter_cols + 2` gate. §6 carries the coordination note. The
justification is not tidiness: before S6 the zero-budget path was reachable only
at `cols <= 2`, a width no user has; after S6 it is what Alt+c produces — under
§2.8 option (a), on every row, forever.

**If §2.8 resolves to (c), this change is still right** (a widened collapsed
target makes budget 1 or 2 normal, and the zero-budget path returns to being a
seek transient) — but it stops being urgent, and its test can revert to S5's
"pre-existing" framing. Recorded so the two decisions are not silently coupled.

### 2.10 The width budget — the parameter S4 and S5 consume, and the S8 interaction

S5's `compose_row` no longer takes a gutter *width*; it **measures the segments it
is handed** (§2.9.1). So there is no constant for S4/S5 to import — but there is
still a number, because it is what every width judgement is made against, and S6
is where it is decided:

```
gutter_cols   = sum of gutter_segments(row, cols, collapsed) text widths
budget        = cols - gutter_cols - RIGHT_MARGIN_COLS
              = cols - 6 - 1   (expanded, cols >= 6)
              = cols - 4 - 1   (collapsed, cols >= 4)
```

**The authoritative value is 6 columns expanded, so the text budget is
`cols − 7`.** S5 needs no change to consume it (`compose_row` derives it, and
`compose_row_measures_the_gutter_it_is_given` already pins the derivation
against a 2-cell and a 4-cell gutter — that test should gain a 6-cell case,
§4.2). S4's `fit_label(name, budget)` takes the number as an argument and needs
no structural change either.

| `cols` | today | after S6 | Δ |
|---|---|---|---|
| 30 (`BAR_TARGET_COLS`, `model.rs:137`) | 27 | **23** | **−4** |
| 38 (S8's proposal) | 35 | **31** | +4 vs today |
| 4 (`COLLAPSED_TARGET_COLS`) | 1 | **0** | −1 under §2.8(a) — and that 1 was a fixed `…`, not a name. §2.8 is where this row is decided |

**S6 is width-independent but it is not width-free.** At today's 30 columns it
costs 4 text columns, and S4 §3.4's drop policy is what absorbs that: at
`budget = 23`, `clave · F-CLA · main · Fix auth flow` (36) drops branch → 29,
drops words → `clave · F-CLA` (13). The two segments the maintainer named as
most important still fit. At S8's 38 the same row keeps its words.

**Sequencing recommendation, for the maintainer to rule on:** land **S8 before or
with S6**. S6 does not depend on S8 — the gutter is a constant regardless of
`cols`, and §4.3 G1 proves it across `0..=200` — but landing S6 alone tightens
every row by four columns for however long S8 takes. §5 Step 8 is where he judges
whether 23 columns is livable if the order slips.

#### 2.10.1 Reconciliation — the gutter is **6 columns**, confirmed

Two sibling specs were written in parallel against different assumptions and both
are stale on this number:

| Spec | Assumed | Its stated budgets | Correct |
|---|---|---|---|
| S8 (`2026-07-22-S8-sidebar-width.md:20-27`) | gutter 3 (`cols − 3 − 1`) | 26 @30, 34 @38 | **23 @30, 31 @38** |
| S5 (`2026-07-22-S5-per-repo-colour.md:1576-1577`) | gutter 4 (`38 − 4 − 1`) | 33 @38 | **31 @38** |

**S6 is authoritative** — the ruling assigned the gutter to this workstream — and
the value is **6**, for the reasons in §2.1: the
maintainer's format is spaced, adjacent patched-font icons read as one compound
mark, and text abutting a glyph with no separating column is unreadable and is
exactly where Nerd Font overdraw (§2.2.3) lands. Today's 2-cell gutter is
*already* glyph-plus-space; a 3-column gutter would be the first time clave
printed a name flush against a glyph.

The three candidate forms and what they cost, so the choice is the maintainer's
and not an accident of which spec was read last:

| Form | Gutter | Budget @30 | Budget @38 | Verdict |
|---|---|---|---|---|
| spaced `● ␣ 𖣂 ` | **6** | **23** | **31** | **S6's ruling** — the maintainer's verbatim format |
| packed + trailing space `●␣𖣂 ` | 4 | 25 | 33 | S5's assumption; the **collapsed** form (§2.8). Viable expanded if two columns matter more than the separation |
| packed, no trailing space `●␣𖣂` | 3 | 26 | 34 | S8's assumption. **Rejected** — text abuts the marker |

Switching the expanded gutter to the second form is one constant —
`GUTTER_COLS_EXPANDED = GUTTER_CELLS + 1` — and §4.3 G1/G2 already cover it,
because they assert the width is *whatever `gutter_segments` produced*, not the
literal 6. So if the extra columns turn out to matter more than the spacing, the
change is one line, two test-expectation numbers, and no re-validation of
geometry.

**Neither sibling needs a structural change.** S5 measures the gutter it is
handed, so it imports no number at all (`S5 §2.7`); its §7 prose figure is
illustrative. S8's own §2.4 already correctly identifies `main.rs:546` as *"not
S8's line — a function of the runtime `cols` parameter"* (`S8:90`); only its §1
prose figures need correcting. Whoever lands after S6 fixes the prose.

### 2.11 Rejected alternatives

| Rejected | Why |
|---|---|
| Pack the expanded gutter (`●󰁼𖣂 `, 4 cols) | saves 2 columns but is not the maintainer's format, and adjacent patched-font icons read as one compound mark. Kept as the **collapsed** form (§2.8), where 4 columns is the entire budget |
| A variable-width gutter (drop trailing blanks) | the whole point is that text starts in the same column on every row. A gutter that shrinks when a cell is empty is a ragged text column — the defect §2.3 exists to prevent |
| `Row.worktree: bool`, resolve the glyph in `gutter_segments` | forces the gutter builder to reach for `self.glyphs` it does not have (it is a free function taking `&Row`), or forces `GlyphSet` into `Row`. Resolving to `(char, u8)` in `rows()` keeps the tier confined to the one place that already holds `self` |
| `compose_row(&Row, cols, collapsed: bool)` with the gutter built inside | §2.9.1 — S5's ruling. `collapsed` is UI state and `compose_row` is geometry; passing finished segments keeps state out of it and costs S6 one call-site line |
| `Agent.worktree: bool` on the wire | #24 item 1 needs the path for `<repo> » <worktree-dir>`; a bool buys nothing and costs a second wire change |
| Fix the `worktree` field's false negatives in S6 | §2.4.3 — it lands inside the `add.rs` / `hook.rs` blocks S4 is restructuring, it is a store-semantics decision with picker fallout, and it moves no columns |
| Derive collapsed/expanded from `cols` instead of the mode flag | §2.8 — makes the text budget non-monotone in `cols` at the threshold |
| `CLAVE_GLYPHS` env var read at snapshot time | §2.6.5 — four writer environments, tier flickers per push |
| A `clave glyphs <tier>` subcommand | §2.6.5 — CLI-surface taxonomy row for a once-per-machine decision; the right end state, not the right v1 |
| A real font check in `clave doctor` | §2.6.6 — doctor sees the machine, the font lives in the terminal, which under SSH is a different host. A check that can be wrong is worse than a printed probe |
| Fill the battery cell in this batch | it needs the hook's tail-scan, a store field, rot-reducer's token estimation and the profile env resolution (#24's battery comment). That is a workstream, and it is **S7** |
| `zellij-tile`'s `Text` builder for the gutter | RC-G / S5 §2.6 — semantic index-levels resolved host-side, no arbitrary palette, and it cannot express "SGR 90 on this one cell" |
| Emit the gutter inside `Row.name` | RC-G — the clamp counts scalars; this is the exact defect the dossier warns about |
| Reverse-video the gutter on the active row | today's behaviour opens SGR 7 *after* the gutter (`main.rs:554-556`). Inverting glyph cells would swap a dim marker to a bright one on selection — motion where none is meant — and would destroy the "highlight's left edge = text column" ruler live validation Step 4 depends on |

---

## 3. Implementation

**Hard dependency: S5 lands first.** S6 extends `compose_row` / `render_segments`
/ `Ink` / `Segment`; none of them exist before S5. If S6 must go first it would
have to build S5's seam, which the dossier's split forbids (`:548-551`). Every
quotation below of "current code" is marked **pre-S5** or **post-S5**.

Change classes (`docs/dev/TESTING.md:110-120`): **Pure logic / model** (the
gutter, `rows()`, `snapshot_from`) + **Visual / UX** (every glyph choice). Not
CLI surface — no subcommand or flag is added (§2.6.5). Not generated artifacts —
the `glyphs` key is *optional and absent by default*, so no template changes for
v1 (§2.6.5's gap, deferred to #40). TDD red-first throughout.

### 3.1 `crates/clave-types/src/lib.rs` — the wire field and the glyph vocabulary

**(a) `Agent` gains `worktree`.** After `stale` (`:66-67`):

```rust
    /// The worktree path when this agent lives in one, else `None` — the
    /// gutter's cell-3 marker (#24 item 4's sibling, S6 §2.4). The PATH and
    /// not a bool: #24 item 1 renders `<repo> » <worktree-dir>` from it, and
    /// a bool would force a second wire change for the same fact.
    ///
    /// SOUND, NOT COMPLETE (S6 §2.4.3): it is set only where clave itself
    /// created the worktree (`add.rs:750`) or preserved one across a resume,
    /// so a session started inside a worktree clave did not make records
    /// `None`. A missing marker is a missing hint; there is no false
    /// positive. `default` keeps pre-field payloads parseable — an old
    /// `clave` pushing to a new bar simply shows no markers.
    #[serde(default)]
    pub worktree: Option<String>,
```

Every `Agent` literal gains it — the compiler enumerates them:
`clave-types/src/lib.rs:160`, `:181`, `:205`, `:231` (tests, `None`);
`crates/clave/src/store.rs:175` (§3.2); `crates/clave-bar/src/model.rs:1148`,
`:1164` (test helpers, `None`). `crates/clave/tests/kdl_guardrail.rs` builds an
`AgentRecord`, not an `Agent`, and is untouched.

**(b) `GlyphSet` and the marks.** After `impl Status` (`:33`), beside
`Status::glyph()` for the reason `lib.rs:21-23` already gives — *"so both
artifacts render identically"*; a future `clave ls` worktree marker must not
drift from the bar's:

```rust
/// Which glyph vocabulary the bar draws (#40). `Full` uses the marks the
/// maintainer chose, two of which need a patched (Nerd) font; `Plain` uses
/// only characters present in every monospace font.
///
/// THE TWO TIERS ARE WIDTH-IDENTICAL: every character either tier can emit
/// is ONE terminal cell under `unicode-width`'s non-CJK `width()` (S6 §2.2.2,
/// pinned by `every_glyph_is_one_cell`). Switching tiers therefore cannot
/// move a column, which is what makes the fallback safe to flip at any time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlyphSet {
    #[default]
    Full,
    Plain,
}

impl GlyphSet {
    /// Parse the bar's `glyphs` plugin-config value. Anything unrecognised —
    /// including absent — is `Full`: a typo must degrade to the default
    /// vocabulary, never to a half-configured one.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "plain" => GlyphSet::Plain,
            _ => GlyphSet::Full,
        }
    }

    /// Gutter cell 3: this agent lives in a git worktree.
    ///
    /// `Full` is U+168C2 BAMUM LETTER PHASE-C MBERAE — the maintainer's pick
    /// (#24, 2026-07-21). It is the ONE glyph clave renders that a Nerd Font
    /// does NOT provide (S6 §2.6.3): it is stock Unicode from the Bamum
    /// Supplement, whose coverage in practice is Noto Sans Bamum alone. If it
    /// tofus, the pre-cleared alternate is `\u{e0a0}` (the Powerline branch
    /// mark — every Nerd Font AND every Powerline-patched font, semantically
    /// exact). Both are one cell, so either is a one-constant change.
    pub fn worktree_mark(self) -> (char, u8) {
        match self {
            GlyphSet::Full => ('\u{168c2}', 90),
            // U+2021 DOUBLE DAGGER: General Punctuation, universal coverage,
            // one cell, and no shape collision with the status set.
            GlyphSet::Plain => ('\u{2021}', 90),
        }
    }

    /// Gutter cell 2 for a PLAIN TERMINAL TAB — the maintainer's ruling that
    /// the terminal mark "replaces the battery for terminal tabs". Dim,
    /// because a terminal tab is a thing that exists, not a thing that needs
    /// you.
    ///
    /// PLACEHOLDER CODEPOINT (S6 §2.6.4): the character the maintainer pasted
    /// could not be recovered with certainty, so its exact value is settled by
    /// live-validation Step 1, which prints the four plausible Nerd Font
    /// terminal icons for him to identify. All four are one cell, so
    /// correcting this constant cannot move a column.
    pub fn terminal_mark(self) -> (char, u8) {
        match self {
            GlyphSet::Full => ('\u{f489}', 90), // nf-oct-terminal
            GlyphSet::Plain => ('>', 90),
        }
    }
}
```

No struct change beyond (a), so `AgentSnapshot`/`Register` are untouched.

### 3.2 `crates/clave/src/store.rs` — stop dropping `worktree`

Replace `store.rs:166-189`:

```rust
/// Store → pipe snapshot (§5): drop the store-only fields, keep the order.
pub fn snapshot_from(store: &Store) -> AgentSnapshot {
    …
                last_visited: r.last_visited,
                tab_id: r.tab_id,
                stale: r.stale,
            })
```

with (one line added; the doc comment corrected because `worktree` stops being
store-only):

```rust
/// Store → pipe snapshot (§5): drop the store-only fields, keep the order.
/// `worktree` is NO LONGER store-only (S6 §2.4.2) — the bar's gutter cell 3
/// renders it. `label_source` remains store-only.
pub fn snapshot_from(store: &Store) -> AgentSnapshot {
    …
                last_visited: r.last_visited,
                tab_id: r.tab_id,
                stale: r.stale,
                worktree: r.worktree.clone(),
            })
```

And `AgentRecord`'s struct doc (`store.rs:35-37`) loses `worktree` from its
store-only list.

### 3.3 `crates/clave-bar/src/model.rs` — `Row` becomes three cells

Replace **post-S5** `Row` (S5 §3.4(a)):

```rust
pub struct Row {
    pub key: RowKey,
    /// PLAIN text — never contains an escape.
    pub name: String,
    pub active: bool,
    /// (glyph, ANSI colour) for agent rows; None for plain terminal tabs.
    /// S6 takes ownership of the gutter; until then `gutter_segments` reads
    /// this.
    pub glyph: Option<(char, u8)>,
    /// Resolved paint list: (field index in `name`, colour). Empty for a
    /// plain terminal tab.
    pub inks: Vec<(usize, Ink)>,
}
```

with (S5's fields verbatim; two added, and `glyph`'s comment finished now that S6
has in fact taken the gutter):

```rust
pub struct Row {
    pub key: RowKey,
    /// PLAIN text — never contains an escape (S5 §2.7).
    pub name: String,
    pub active: bool,
    /// GUTTER CELL 1 — status. `(glyph, basic ANSI SGR)` for agent rows;
    /// `None` for plain terminal tabs. `glyph.is_some()` is still exactly
    /// "this row has a joined agent"; S6 did not move that meaning.
    pub glyph: Option<(char, u8)>,
    /// GUTTER CELL 2 — the context battery (#24 item 4, S7). ALWAYS `None`
    /// for agent rows in this batch: the cell renders as one blank cell so
    /// that populating it later cannot move the text column (S6 §2.3, proved
    /// by G1/G2). A plain terminal tab puts the TERMINAL MARK here — the
    /// field names the cell, not the semantics.
    pub battery: Option<(char, u8)>,
    /// GUTTER CELL 3 — the worktree marker (#24 locked format). Resolved in
    /// `rows()` from `Agent.worktree` through the active `GlyphSet`, so
    /// nothing downstream learns about fonts (S6 §2.9.2).
    pub worktree: Option<(char, u8)>,
    /// Resolved paint list for the TEXT: (field index in `name`, colour).
    /// Empty for a plain terminal tab. Rule G/T (S6 §2.5): these are
    /// `Ink::Rgb`; the three gutter cells above are basic `Ink::Sgr`. Neither
    /// family crosses.
    pub inks: Vec<(usize, Ink)>,
}
```

### 3.4 `crates/clave-bar/src/model.rs` — the gutter constants and builder

New, placed beside S5's `RIGHT_MARGIN_COLS` (S5 §3.5). S5 has no `GUTTER_COLS`
constant — `compose_row` measures the segments — so these are additions, not
replacements:

```rust
/// The gutter's three cells, in fixed order: status, context battery
/// (S7) / terminal mark, worktree marker. THREE is the number the maintainer
/// specified and the number every width constant below is derived from.
const GUTTER_CELLS: usize = 3;
/// Expanded gutter: every cell followed by one space (the maintainer's
/// format `● 󰁼 𖣂 …`). 6 columns, INDEPENDENT of which cells are occupied —
/// that invariance is the whole of S6 and is pinned by a proptest.
const GUTTER_COLS_EXPANDED: usize = GUTTER_CELLS * 2;
/// Collapsed gutter (Alt+c): cells PACKED, one trailing space. 4 columns —
/// under S6 §2.8 option (a) that is the entire collapsed row.
const GUTTER_COLS_COLLAPSED: usize = GUTTER_CELLS + 1;
/// The right margin the renderer has always reserved (S5 already owns this
/// constant; the old literal `3` at `main.rs:546` was it plus the 2-cell
/// gutter).
const RIGHT_MARGIN_COLS: usize = 1;

/// The collapsed gutter must FIT the collapsed pane. An INEQUALITY, not an
/// equality: S6 §2.8's open decision may widen COLLAPSED_TARGET_COLS to buy
/// text back, and that must not require touching this line. Fails the BUILD
/// rather than silently clipping the worktree marker off the right edge on
/// every render.
const _: () = assert!(GUTTER_COLS_COLLAPSED <= COLLAPSED_TARGET_COLS);

/// Total gutter width in cells. A function of the MODE and nothing else —
/// not of content, not of the row's kind, not of the GlyphSet. `min(cols)`
/// is the sole degradation: during an expand seek the mode flips to expanded
/// while `cols` is still 4, and clipping the gutter is the only way to keep
/// "the rendered line never exceeds `cols`" true at EVERY width.
fn gutter_cols(cols: usize, collapsed: bool) -> usize {
    let want = if collapsed {
        GUTTER_COLS_COLLAPSED
    } else {
        GUTTER_COLS_EXPANDED
    };
    want.min(cols)
}

/// The gutter as a flat cell sequence: `(char, optional basic-SGR colour)`.
/// A BLANK cell is a SPACE, never an empty string and never a skipped
/// separator — a gutter that shrinks when a cell is empty is a ragged text
/// column, which is the defect this whole workstream exists to prevent
/// (S6 §2.3).
///
/// Every char here is one terminal cell (S6 §2.2.2, pinned by
/// `every_glyph_is_one_cell`), so `.len()` IS the display width and the
/// scalar-vs-cell hazard in the name clamp does not reach the gutter.
fn gutter_sequence(row: &Row, collapsed: bool) -> Vec<(char, Option<u8>)> {
    let cells = [row.glyph, row.battery, row.worktree];
    let mut out = Vec::with_capacity(GUTTER_COLS_EXPANDED);
    for cell in cells {
        match cell {
            Some((ch, colour)) => out.push((ch, Some(colour))),
            None => out.push((' ', None)),
        }
        if !collapsed {
            out.push((' ', None));
        }
    }
    if collapsed {
        out.push((' ', None));
    }
    out
}
```

### 3.5 `crates/clave-bar/src/model.rs` — replace `gutter_segments`, and only that

**`compose_row` is not touched.** S5 §3.5 ships `gutter_segments` explicitly as
the transitional stand-in — *"S6 replaces this function and nothing else"* — and
that is exactly what happens. Replace **post-S5**:

```rust
/// Today's 2-cell gutter, rebuilt as segments. **S6 replaces this function
/// and nothing else** — `compose_row` already treats the gutter as opaque.
pub fn gutter_segments(row: &Row) -> Vec<Segment> {
    match row.glyph {
        Some((glyph, colour)) => vec![
            Segment { text: glyph.to_string(), ink: Some(Ink::Sgr(colour)), reverse: false },
            Segment { text: " ".to_string(), ink: None, reverse: false },
        ],
        // Plain tabs get a 2-space gutter so names align.
        None => vec![Segment { text: "  ".to_string(), ink: None, reverse: false }],
    }
}
```

with:

```rust
/// The three-cell gutter (S6): status · context battery (S7) / terminal mark ·
/// worktree marker, each followed by a space when expanded, packed with one
/// trailing space when collapsed.
///
/// `collapsed` is a parameter and NOT derived from `cols`: packed is always 2
/// columns cheaper than spaced, so any width threshold would make the text
/// budget non-monotone in `cols`, and a collapse is a deliberate user act
/// whose discontinuity is the point (S6 §2.8.4). THE MODE STOPS HERE —
/// `compose_row` receives finished segments and stays pure geometry.
///
/// Never reverse-videoed: the active row's SGR 7 opens at the first character
/// of TEXT, which is what makes the highlight's left edge a reliable
/// "text starts here" ruler (and is today's behaviour, `main.rs:554-556`).
///
/// Rule G/T (S6 §2.5): every ink here is `Ink::Sgr`. `Ink::Rgb` belongs to the
/// name and may never appear in this function's output.
pub fn gutter_segments(row: &Row, cols: usize, collapsed: bool) -> Vec<Segment> {
    gutter_sequence(row, collapsed)
        .into_iter()
        .take(gutter_cols(cols, collapsed))
        .map(|(ch, colour)| Segment {
            text: ch.to_string(),
            ink: colour.map(Ink::Sgr),
            reverse: false,
        })
        .collect()
}
```

The only other model-side change is the zero-budget early return in `clamp_name`
(§2.9.3, code quoted there).

`compose_row` and `render_segments` are **unchanged**. `render_segments` remains
the only `\x1b` writer in `crates/clave-bar`; `compose_row` continues to measure
whatever gutter it is given, which is now 6 cells instead of 2 — and S5's
`compose_row_measures_the_gutter_it_is_given` already asserts that it does.

### 3.6 `crates/clave-bar/src/model.rs` — `rows()` resolves the cells

`BarModel` gains one field, beside `pub collapsed` (`model.rs:255-257`) and
S5's `palette` (S5 §3.4(b)):

```rust
    /// The glyph vocabulary this instance draws (#40, S6 §2.6.5). Set once
    /// from the plugin's KDL `glyphs` config key in `load()`; every instance
    /// in the session loads the same layout, so all N agree by construction
    /// — which is precisely why this is NOT an env var read per snapshot
    /// writer (four different environments, one flicker per push).
    pub glyphs: GlyphSet,
```

initialised `glyphs: GlyphSet::default()` in the constructor beside
`collapsed: false` (`model.rs:315`).

**Live-tab branch.** Replace **post-S5** `model.rs:746-765` (S5 §3.4(c)):

```rust
        for t in &self.tabs {
            let joined = self.agent_in_tab(t.tab_id);
            let glyph = joined.map(|a| { … });
            let inks = joined.map(|a| self.row_inks(a)).unwrap_or_default();
            entries.push((
                self.sort_key(t),
                t.position,
                Row {
                    key: RowKey::Tab(t.tab_id),
                    name: t.name.clone(),
                    active: selected_dormant.is_none() && t.active,
                    glyph,
                    inks,
                },
            ));
        }
```

with:

```rust
        for t in &self.tabs {
            let joined = self.agent_in_tab(t.tab_id);
            let glyph = joined.map(|a| { … });                    // unchanged
            let inks = joined.map(|a| self.row_inks(a)).unwrap_or_default();
            // Gutter cell 2 (S6 §2.7): an agent row leaves it BLANK until S7
            // fills it; a plain terminal tab — no joined agent — gets the
            // terminal mark, which is the maintainer's ruling and is what
            // keeps a terminal row's text in the same column as an agent's.
            let battery = match joined {
                Some(_) => None,
                None => Some(self.glyphs.terminal_mark()),
            };
            // Gutter cell 3: sound-not-complete provenance (S6 §2.4.3).
            let worktree = joined
                .filter(|a| a.worktree.is_some())
                .map(|_| self.glyphs.worktree_mark());
            entries.push((
                self.sort_key(t),
                t.position,
                Row {
                    key: RowKey::Tab(t.tab_id),
                    name: t.name.clone(),
                    active: selected_dormant.is_none() && t.active,
                    glyph,
                    battery,
                    worktree,
                    inks,
                },
            ));
        }
```

`self.glyphs` and `self.row_inks(a)` are both immutable borrows of `self`
alongside `joined`, so this compiles unchanged.

**Dormant branch.** In **post-S5** `model.rs:783-789` the agent `a` is in hand
(and `glyph` is S3's `DORMANT_GLYPH` if S3 landed — S6 does not touch that
expression):

```rust
                Row {
                    key: RowKey::Dormant(a.uuid.clone()),
                    name: a.label.clone(),
                    active: selected_dormant == Some(a.uuid.as_str()),
                    glyph: Some(glyph),
                    // A dormant row is an agent with no tab: cell 2 stays
                    // blank (the terminal mark is for rows with NO agent),
                    // cell 3 still tells you where it lives.
                    battery: None,
                    worktree: a
                        .worktree
                        .is_some()
                        .then(|| self.glyphs.worktree_mark()),
                    inks: self.row_inks(a),
                },
```

### 3.7 `crates/clave-bar/src/main.rs` — read the config, build the gutter

**(a)** `load()` — replace `main.rs:342`:

```rust
    fn load(&mut self, _config: BTreeMap<String, String>) {
```

with:

```rust
    fn load(&mut self, config: BTreeMap<String, String>) {
        // #40 / S6 §2.6.5: the glyph vocabulary, from the plugin's own KDL
        // config block. Absent (the shipped default) or unrecognised ⇒ Full.
        // Read from the LAYOUT rather than the environment or the snapshot
        // because every instance in the session loads the same layout, so
        // all N agree by construction.
        self.model.glyphs = config
            .get("glyphs")
            .map(|s| GlyphSet::parse(s))
            .unwrap_or_default();
```

with `GlyphSet` added to the `clave_types` import at `main.rs:10-12`.

**(b)** `render()` — replace **post-S5** `main.rs:536-538` (S5 §3.6, the adapter):

```rust
        for row in self.model.rows() {
            let gutter = gutter_segments(&row);
            println!("{}", render_segments(&compose_row(&row, cols, &gutter)));
        }
```

with (**one line changes** — `gutter_segments` gains the two arguments the mode
needs, and the mode stops there):

```rust
        // The gutter is built HERE and handed to compose_row as finished
        // segments (S6 §2.9.1): `collapsed` is UI state, and compose_row is
        // geometry. It is passed, not derived from `cols`, because a
        // width-derived rule makes the text budget non-monotone (§2.8.4).
        let collapsed = self.model.collapsed;
        for row in self.model.rows() {
            let gutter = gutter_segments(&row, cols, collapsed);
            println!("{}", render_segments(&compose_row(&row, cols, &gutter)));
        }
```

The untested residue in `main.rs` remains one `println!` of a fully composed
string plus one config read.

### 3.8 `crates/clave-bar/Cargo.toml` — `unicode-width` as a **dev**-dependency

```toml
[dev-dependencies]
proptest = "1"
# S6 §2.2: the gutter's width invariant is MEASURED, not asserted. This is
# the same crate zellij itself uses to lay out the grid
# (zellij-utils-0.44.3/Cargo.toml:185), so `every_glyph_is_one_cell` checks
# our glyphs against the host's own table rather than against a comment.
# DEV-only on purpose: production code carries a plain `1` per cell, so the
# shipped wasm gains no bytes and no dependency (dev-deps are excluded from
# `cargo build --target wasm32-wasip1`, the same reasoning as proptest above).
unicode-width = "0.1"
```

### 3.9 What does **not** change

- **No generated-artifact change.** The `glyphs` key is optional and absent by
  default, so `setup.rs:177`, `:215` and `add.rs:109` are untouched and the KDL
  guardrail is unaffected. (Baking the key is #40's follow-up, §6.)
- **No CLI surface**, so no `Cli::try_parse_from` pin and no sandboxed e2e run is
  owed (`TESTING.md:116`).
- **No new zellij subscription or permission** (`main.rs:356-374` unchanged).
  `load()` already receives the config map; S6 stops discarding it.
- **No change to `Status::glyph()`** or to `clave ls` (`lsview.rs`).
- **No change to ordering** — `sort_key`, the comparator (`model.rs:391-393`,
  `:791`) and `click()`'s one-line-per-row indexing (`model.rs:800-803`) are
  untouched; the gutter adds no lines.
- **No change to `AgentRecord`'s `worktree` writers.** §2.4.3.

---

## 4. Test plan

Taxonomy rows: **Pure logic / model** (`TESTING.md:114`) — TDD red-first,
`cargo test --workspace`, extend the proptests for every newly reachable branch —
plus **Visual / UX** (`:120`) for every glyph choice, which no tier can
adjudicate. The PR carries `needs-live-validation` **and** `host-untestable`.

`--workspace` is load-bearing: a bare `cargo test` skips `clave-bar` entirely
(`TESTING.md:36-42`), which is where every gutter test lives.

### 4.1 Tier 1 — unit tests

**`crates/clave-types/src/lib.rs`:**

| Test | Asserts |
|---|---|
| `every_glyph_is_one_cell` | **the load-bearing test of this spec.** For every char in `{Status::glyph()} ∪ {DORMANT_GLYPH, '✗', '↻'} ∪ {both tiers' worktree_mark, terminal_mark} ∪ {'…', ' '}`: `UnicodeWidthChar::width(c) == Some(1)`. Fails the build the day someone adds a two-cell glyph — the one failure mode that shifts every row and that nothing else would catch |
| `glyph_tiers_are_width_identical` | `Full` and `Plain` produce the same total width for each cell. The claim §2.6.2 makes, mechanised |
| `glyph_set_parses_and_defaults_to_full` | `parse("plain") == Plain`; `parse("Plain")`, `parse(" plain ")` likewise; `parse("")`, `parse("nerd")`, `parse("ful")` all `Full`. A typo must degrade to the default vocabulary |
| `gutter_marks_are_not_status_shapes` | for both tiers, `worktree_mark().0` and `terminal_mark().0` are absent from the status/dormant char set. Pins the S3-composition property (§2.5), not the codepoints, so a later glyph change cannot silently re-collide |
| `agent_worktree_roundtrips_and_defaults_none` | mirrors `agent_stale_roundtrips_and_defaults_false` (`:227-251`): `Some(path)` round-trips; a payload with the key removed parses as `None`. The #43/#44 mixed-binary guarantee |

**`crates/clave/src/store.rs`:**

| Test | Asserts |
|---|---|
| `snapshot_carries_worktree` | a record with `worktree: Some("/x/.claude-worktrees/ab")` reaches `snapshot_from`'s `Agent` intact, and a `None` record yields `None`. The exact drop this workstream exists to fix |

**`crates/clave-bar/src/model.rs`:**

| Test | Asserts |
|---|---|
| `gutter_is_six_cells_regardless_of_occupancy` | the table in §2.1, enumerated: all 8 combinations of (status, battery, worktree) present/absent → `gutter_segments(row, 30, false)` concatenates to exactly 6 chars, and the 7th char of `compose_row`'s visible line is the name's first char in all 8 |
| `blank_cells_are_spaces_not_omissions` | a row with `battery: None, worktree: None` yields gutter text `"● \u{20}\u{20}\u{20}\u{20}"` — the separators are real segments, not skipped |
| `plain_tabs_carry_no_status_and_no_worktree` | `glyph: None` ⇒ gutter is `' '`,`' '`,mark,`' '`,`' '`,`' '`; and `glyph.is_some()` still means "has an agent" |
| `worktree_rows_carry_the_marker` | a live tab bound to an agent with `worktree: Some(_)` gets `Row.worktree == Some(GlyphSet::Full.worktree_mark())`; the same agent with `worktree: None` gets `None` |
| `dormant_rows_carry_the_marker_too` | the dormant branch reads `a.worktree`, not just `a.label` |
| `plain_tier_swaps_marks_without_moving_a_column` | the same row under `Full` and `Plain` produces gutters of equal char count and a name starting at the same index |
| `gutter_and_text_inks_never_cross` | Rule G/T (§2.5): no gutter segment carries `Ink::Rgb`; no name segment carries `Ink::Sgr` |
| `gutter_is_never_reversed` | for `active: true`, every gutter segment has `reverse: false` and the first `reverse: true` segment is the name's |
| `collapsed_gutter_is_exactly_four_cells` | `gutter_segments(row, 4, true)` → 4 chars, three of them cells; `compose_row`'s budget is 0 and it emits **no `…`**. **Pins §2.8 option (a)** — if the maintainer rules for (c) this test moves with the constant |
| `collapsed_gutter_shows_all_three_cells` | at `cols == 4` a worktree agent row still shows status *and* worktree marker — §2.8(a)'s answer to #24 item 7 |
| `expand_seek_transient_never_overflows` | `gutter_segments(row, 4, false)` (expanded gutter, 4 columns) → visible width ≤ 4, gutter clipped by `gutter_cols`'s `min` |
| `compose_row_emits_no_ellipsis_at_zero_budget` | **replaces S5's `compose_row_narrow_width_overflow_is_preexisting`** (§2.9.3, §6). At `budget == 0` the name contributes no segments at all |
| `text_budget_is_cols_minus_seven` | with a 6-cell gutter, `cols = 30` ⇒ a 40-char name clamps to 23 cells; `cols = 38` ⇒ 31. The authoritative number (§2.10.1), pinned |

### 4.2 Tier 1 — tests that must change

| Site | Action |
|---|---|
| `model.rs:1147-1161` `fn agent`, `:1163-1175` `fn agent_labelled` | add `worktree: None` ⇒ every pre-existing test keeps a blank cell 3, which makes them a control group. Add a builder `agent_in_worktree(uuid, tab_id)` for the new tests |
| **S5's `compose_row_measures_the_gutter_it_is_given`** | **extend, do not replace** — it currently compares a 2-cell and a 4-cell gutter. Add a 6-cell case so the shipped gutter width is exercised by S5's own contract test as well as S6's |
| S5's other `compose_row_*` tests (S5 §5.1) | mechanical: they build a gutter via `gutter_segments`, whose signature gains `cols` and `collapsed`. Their *name-side* assertions are unchanged, which is the point — S6 must not perturb S5's text half |
| S5's `compose_row_narrow_width_overflow_is_preexisting` | **superseded** by `compose_row_emits_no_ellipsis_at_zero_budget` (§2.9.3). Record it in the PR as a deliberate supersession, not a regression |
| S5's `render_segments_matches_the_pre_s5_line_when_untinted` | keep, but re-baseline: the line it reproduces now carries a 6-cell gutter |
| `clave-types` tests `:158-173`, `:199-225`, `:227-251`, `:253-270`, `:272-289` | `Agent` literals gain `worktree: None`; assertions unchanged |
| `crates/clave/tests/kdl_guardrail.rs` | unchanged — it builds an `AgentRecord`, which already has the field, and no generated artifact changes |
| `model.rs:2106` `store_rows_without_live_tabs_render_dormant` | extend with `assert_eq!(d.worktree, None)` so the empty-`worktree` path is pinned rather than incidental |

**A test S5 no longer has:** an earlier S6 draft said it would rewrite
`compose_row_at_collapsed_width`. That test does not exist in S5's current
revision — S5 removed it for the same reason §2.8.1 gives (at a 4-column
collapsed width with S6's gutter there is no text to tint, so the assertion had
nothing to make). S6 adds `collapsed_gutter_is_exactly_four_cells` in its place.

### 4.3 Tier 1 — proptests (`model.rs mod proptests`, `:2803+`)

**Numbering note:** S5's proptest block already defines P1–P6 for the text half
and P7–P8 for store allocation. S6's properties below are **G1–G7** to avoid a
collision; S5's P6 (*"the gutter passes through verbatim"*) is the hinge between
the two sets and is not duplicated here.

Generators: `cols in 0usize..=200`, `collapsed in any::<bool>()`,
`active in any::<bool>()`, `name` from `"[\\PC ·…]{0,60}"` plus S5's
escape-injecting strategy, `inks` as S5, and — the new axis —

```rust
let cell = prop_oneof![
    Just(None),
    Just(Some(('\u{25cf}', 33u8))),          // status-ish
    Just(Some(('\u{168c2}', 90u8))),         // Full worktree mark
    Just(Some(('\u{2021}', 90u8))),          // Plain worktree mark
    Just(Some(('\u{f489}', 90u8))),          // terminal mark
    Just(Some(('\u{2588}', 32u8))),          // a POPULATED battery — S7's future
];
```

The `\u{2588}` arm is the point: **the battery cell is generated as occupied even
though production never populates it in this batch.** That is what turns
"reserving the slot" from a promise into a proof.

| Property | Statement |
|---|---|
| **G1 — gutter width is invariant** | **the required property.** For every combination of the three cells present/absent, every `cols`, every `collapsed`: `gutter_segments(row, cols, collapsed)` concatenates to exactly `gutter_cols(cols, collapsed)` characters — `GUTTER_COLS_EXPANDED` / `GUTTER_COLS_COLLAPSED` whenever `cols` is at least that wide. 8 occupancy shapes × both modes, none of which may differ. **Asserted against `gutter_cols(...)`, never a literal**, so §2.8's open decision and §2.10.1's packing choice both leave it valid |
| **G2 — the text column never moves** | for two rows differing *only* in cell occupancy, at the same `cols` and `collapsed`, the index at which the name's first character appears in the visible line is identical. G1 restated where the user actually sees it — and the direct proof that S7 populating cell 2 cannot reflow. Scoped per mode, deliberately: a mode change is *allowed* to move the column, which is what makes §2.8(c)/(d) implementable without weakening this |
| **G3 — the line never exceeds `cols`** | the escape-stripped visible line is at most `cols` cells wide, for **every** `cols` including 0–6. S5's P3 is gated at `cols >= gutter_cols + 2` by the ellipsis overflow; §2.9.3 removes the gate and S5's P3 can adopt this form |
| **G4 — Rule G/T holds** | no segment inside the gutter carries `Ink::Rgb` (or `Ink::Indexed`); no segment after it carries `Ink::Sgr` (§2.5) |
| **G5 — no escape is truncated** (S5 P1, re-run) | every `\x1b` opens a complete `\x1b[…m` and the introducer/reset counts match, now with six more segments per line to get wrong |
| **G6 — gutter chars are all one cell** | every char emitted into the gutter satisfies `unicode_width::width(c) == Some(1)`. Catches a generated or future glyph that would break G1 from the inside, and is the runtime twin of `every_glyph_is_one_cell` |
| **G7 — stripping the gutter reproduces the S5 line** | deleting the first `gutter_cols(...)` characters of the visible line yields exactly what S5's renderer produced for the same row with a 0-cell gutter. S6 is additive to the text half, mechanised. This is S5's own P6 (*"the gutter passes through verbatim"*) viewed from the other end, and one of the two may be dropped if they prove redundant in review |

Ledger rationale for adding properties at all: `TESTING.md:121-126` — *"A new
branch without a new property is a new blind spot"*; the width-seek escape (#4)
was pure logic inside a covered tier the generator never reached.

### 4.4 Tier 2

Does not exist (#47, blocked on #44 — `TESTING.md:44-50`). S6 crosses the
process seam in exactly one place — `snapshot_from` now serialises a field the
bar deserialises — and that seam is covered at Tier 1 from both ends
(`snapshot_carries_worktree` in `clave`, `agent_worktree_roundtrips_and_defaults_none`
in `clave-types`) with the same `#[serde(default)]` compatibility argument that
`tab_id` and `stale` already carry. **The written cross-process argument owed in
the PR dossier is one paragraph:** an old `clave` pushing to a new bar omits the
key ⇒ `None` ⇒ no markers; a new `clave` pushing to an old bar adds a key the old
`Agent` ignores (no `deny_unknown_fields`). Both directions degrade to today's
behaviour. Nothing else in S6 leaves the process.

The other seam S6 touches is the **screen**, which is Tier 3 by definition.

### 4.5 Tier 3

Everything in §5. Whether a glyph renders, whether it occupies one cell in the
maintainer's terminal, and whether three marks in six columns help or clutter is
`host-untestable` by the taxonomy's own row (`TESTING.md:120`).

### 4.6 The gate

```bash
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

---

## 5. Live validation

**Contract** (`AGENTS.md:51-53`, `TESTING.md:188-204`). The maintainer runs every
step. The driving agent **prints** commands and never executes them against a
live session, never launches or kills a session, never runs `just release`,
`cargo install` or `just dev-install`. Paths are genericized (`$HOME/…`,
`$TMPDIR/…`) — the pre-commit PII blocklist rejects private local paths and has
fired twice (`AGENTS.md:122-124`).

Throughout, **"the text column"** means the screen column at which a row's name
begins. The single question this run answers is whether that column is the same
on every row.

### Step 0 — pre-flight (issue #44 is unfixed; skip this and every reading below is suspect)

**(a) Run:**
```bash
command -v clave && clave --version
grep -n 'clave-bar: loaded' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -5
```

**(b) Look at:** the version from `clave --version` versus the version in the most
recent `clave-bar: loaded vX.Y.Z build=…` line.

**(c) Report:** both strings verbatim, plus the `build=` tag.

| Report | Conclusion | Next |
|---|---|---|
| the two versions match | the fleet is coherent | Step 1 |
| they differ | **#44/#43** — the plugin shells out to a different binary than the one on `PATH` | **stop.** No observation below can be trusted. Report and abandon the run |
| no `clave-bar: loaded` line from today | the log is stale or the filter is wrong (the file is shared by every session on the machine, `TESTING.md:295-300`) | re-run with `tail -50`; if still nothing, report and stop |
| `build=` is not `dev` and you did not just hot-reload | you are looking at an instrumented sandbox wasm | note which session you are in and repeat in the intended one |

### Step 1 — **the glyph decision, recorded** — do they render, and which codepoints? (no clave involved)

**This step is the deciding input for two constants the spec deliberately left
pending** (§2.6.3, §2.6.4): the worktree marker (U+168C2 versus the Powerline
branch mark) and the terminal mark (four candidates). Both ship as named
constants with placeholder values precisely so this step's answer is a one-line
edit. **The report from this step is the record**; nothing else settles them.

Codepoints are written as `printf` escapes rather than pasted characters,
deliberately: a private-use glyph does not survive every copy-paste path — this
spec's own authoring pipeline dropped them — so an escape is the only form
guaranteed to be exactly what was intended.

**(a) Run** in any pane of the terminal you actually use:
```bash
printf 'ruler    [x] [x] [x] [x] [x] [x]\n'
printf 'status   [●] [✖] [○] [◌] [✗] [↻]\n'
printf 'worktree [\U000168C2] [] [‡]\n'
printf 'battery  [\U000F007C] [█] [▅] [▁]\n'
printf 'terminal [] [] [] []\n'
```

Legend, in the same order:

| line | entries |
|---|---|
| `status` | `●` `✖` `○` `◌` `✗` `↻` — today's set plus S3's proposed `○` |
| `worktree` | 1 · U+168C2 Bamum, **the spec's default and your pick** · 2 · U+E0A0 Powerline branch, the pre-cleared alternate · 3 · `‡` U+2021, the `Plain` tier |
| `battery` | 1 · U+F007C, the MDI battery **you pasted** · 2–4 · the block eighths `█ ▅ ▁`, which is #24's own earlier ruling (§2.6.4) |
| `terminal` | 1 · U+E795 `nf-dev-terminal` · 2 · U+F120 `nf-fa-terminal` · 3 · U+F489 `nf-oct-terminal`, the spec's placeholder · 4 · U+EA85 `nf-cod-terminal` |

**(b) Look at:** two things, separately.

1. **Shape** — is anything a box, a blank, or a question mark rather than a
   glyph? Pay particular attention to the *first* entry on the `worktree` line
   (U+168C2): it is the only glyph in the whole set that a Nerd Font does
   **not** provide (§2.6.3).
2. **Alignment** — every line has its brackets in the same columns as the
   `ruler` line. If any bracket pair is wider, that glyph is two cells, not one.

**(c) Report** — this is the record, so all four please:

1. for each line, which entries rendered as real glyphs;
2. whether every line's brackets align with `ruler`'s;
3. **which of the four `terminal` entries is the icon you pasted** (1/2/3/4);
4. **whether you want U+168C2 or the Powerline branch mark** for cell 3 — if
   U+168C2 rendered, that is a taste question and yours alone.

| Report | Conclusion | Next |
|---|---|---|
| everything renders, everything aligns with `ruler` | the `Full` tier is viable on your terminal and every glyph is one cell — the §2.2 measurement is confirmed live | note the terminal-icon position and go to Step 2 |
| `𖣂` is a box but everything else renders | **the predicted failure** (§2.6.3): your font has no Bamum Supplement. Geometry is fine (a box is still one cell) | report it; the fix is `WORKTREE_MARK = '\u{e0a0}'` (2nd entry on that line — confirm it rendered) plus one test. Continue to Step 2 with the box in place |
| the `battery` or `terminal` brackets are wider than `ruler`'s | **the exception in §2.2.3**: your terminal measures the font's advance for PUA and allocates two cells. This is the one finding that breaks the design as written | **report immediately with your terminal name and font name.** The remedies, in order: switch to the font's `… Nerd Font Mono` variant (single-cell advance) and re-run this step; failing that, ship `glyphs "plain"` (Step 7). Do not proceed to Step 2 until the brackets align |
| the `status` brackets are wider than `ruler`'s | your terminal treats East-Asian-Ambiguous as WIDE. clave already depends on the narrow interpretation today (`●`, `…`, `·`) | report your terminal + setting name; the fix is a terminal setting, not a code change. Do not proceed |
| the whole `terminal` line is boxes but `battery` renders | your patched font covers MDI but not the older icon sets — unusual | report; the terminal mark moves to the `Plain` tier's `>` while the rest stays `Full`, which needs a per-cell tier and is a design change. Report before anyone builds it |
| none of the PUA lines render, everything else does | you do not have a patched font in this terminal profile (even if you do in another) | report; this is exactly what the `Plain` tier is for. Go to Step 7, then re-enter at Step 2 |

### Step 2 — the text column, in the sandbox, with and without each glyph

Sandbox first: `c8-worktree` seeds exactly the comparison this step needs — two
agents, one in a real `git worktree` (`dev.rs:57-72`, `TESTING.md` scenario
catalog).

**(a) Run** (agent-safe, non-mutating to the real fleet):
```bash
clave dev reset
clave dev scenario c8-worktree
```
then **you**, in a **non-zellij** terminal:
```bash
clave dev launch
```

**(b) Look at:** the sandbox sidebar. There should be a worktree agent row, a
non-worktree agent row, and — after you open one — a plain terminal tab. Read
**down** the left edge and check that the first letter of every row's name is in
the same column. The easiest read: put the cursor on a row (its highlight starts
at the text) and compare the highlight's left edge across rows.

**(c) Run and report:**
```bash
clave dev status | jq '.store.agents | map({label, worktree, tab_id})'
```
plus, for each visible row, transcribe the first 8 characters exactly as drawn
(use `␣` for a space, `▯` for a box).

| Report | Conclusion | Next |
|---|---|---|
| all rows' names start in the same column, and the worktree row is the one with a mark in cell 3 | **the gutter is correct** — the invariant holds live on real rows | Step 3 |
| the worktree row's name starts one column right of the others | the worktree mark is two cells in this terminal — Step 1 should have caught it | **report immediately** with the terminal + font; re-run Step 1's `worktree` line |
| the terminal tab's name starts in a different column from the agent rows | the terminal mark is two cells, or a blank cell was omitted instead of spaced | **report immediately** — if Step 1 aligned, this is the `blank_cells_are_spaces_not_omissions` path and is a code bug |
| **no** row has a cell-3 mark, but `jq` shows `worktree: "…"` on one | the field is on the wire but the bar is not reading it, or the bar is stale | re-check Step 0's build tag; if current, report — `snapshot_carries_worktree` passed but `rows()` is wrong |
| `jq` shows `worktree: null` on the row you know is a worktree | **the expected false negative** (§2.4.3) — clave did not create that worktree | not a bug. Note it, and see Step 5 for whether you want the upgrade |
| every row shows a `▯` in cell 3 including non-worktree rows | the blank cell is emitting a glyph | report immediately |

### Step 3 — the real fleet: a worktree row beside a non-worktree row

**(a) Do:** in your **real** clave session, make sure two agents of the same repo
are open — one created with `clave add --worktree` (Alt+a → new → worktree) and
one in the main checkout.

**(b) Look at:** the two rows adjacent to each other. Compare cell 3, and compare
where the text begins.

**(c) Run and report:**
```bash
clave ls --json | jq -r '.agents[] | "\(.worktree // "-")\t\(.label)"'
```
plus a transcription of both rows' first 8 characters.

| Report | Conclusion | Next |
|---|---|---|
| the worktree row shows the mark, the main-checkout row shows a blank, both names start in the same column | **shipped behaviour confirmed on the real fleet** | Step 4 |
| both show the mark | a false positive — impossible by §2.4.3's analysis, so something else writes the field | **report immediately** with the `jq` output |
| the mark is present but visually competes with the status dot | the dim-90 choice is not dim enough on your theme (§2.5) | report; the fallback is a different SGR for the marker only, one constant plus one test |
| you cannot tell at a glance which rows are worktrees | the marker is too subtle at your font size | report; options are a brighter SGR or a heavier glyph. This is a Tier 3 judgement and it is yours |

### Step 4 — prove the reserved cell does not reflow

The battery cell is blank in this batch, so its geometry has to be *provoked*.
Tier 1's G2 proves it over generated input; this step confirms the terminal
agrees. Uses the **one sanctioned live mutation** — the sandbox hot-reload
(`AGENTS.md:51-53`, `TESTING.md`).

**(a) Do:** an agent applies a one-line temporary change in the **sandbox
worktree only** — in `rows()`, the live-tab branch, `battery` becomes
`Some(('\u{2588}', 32))` for every agent row — rebuilds the sandbox wasm with a
fresh tag per the instrumentation recipe, and hot-reloads:
```bash
ZELLIJ_SESSION_NAME=clave-test zellij action start-or-reload-plugin \
  "file:$HOME/.local/state/clave-dev/data/clave-bar.wasm"
```

**(b) Look at:** the sandbox sidebar before and after. The middle cell fills with
a solid block on agent rows. **Nothing else may move.**

**(c) Report:** whether any row's text shifted by even one column; whether the
terminal tab's row (whose cell 2 already held the terminal mark) changed at all.

| Report | Conclusion | Next |
|---|---|---|
| the blocks appear and no text moves | **the slot is genuinely reserved** — S7 can populate it with no reflow and no re-validation | revert the temporary change; Step 5 |
| text shifts right by one when the block appears | a blank cell was emitting an empty string, not a space | **report immediately** — `blank_cells_are_spaces_not_omissions` should have caught it |
| the block renders as two cells | U+2588 is wide in your terminal — relevant because it is #24's own battery ramp (§2.6.4) | **report** — this constrains S7's glyph choice and belongs in the ledger |

### Step 5 — the marker's coverage, and whether you want the upgrade

**(a) Run:**
```bash
clave ls --json | jq -r '.agents[] | "\(.worktree // "-")\t\(.cwd)"'
```

**(b) Look at:** rows whose `cwd` is obviously inside a worktree (a
`.claude-worktrees/…` or similar path) but whose `worktree` column is `-`.

**(c) Report:** how many of your live rows are worktrees clave did not create.

| Report | Conclusion | Next |
|---|---|---|
| none — every worktree of yours was made by `clave add --worktree` | the sound-not-complete signal is complete in practice for you | Step 6 |
| one or two | the false negatives are real but tolerable | your call: take §2.4.3's three-line upgrade now (it needs S4's `head.rs` landed) or defer to #24 item 1. **Say which** |
| most of them | the marker is silent where it matters most | take the upgrade — but as its own change with its own review, **not** folded into S6's merge. S6's geometry is independent and can ship first |

### Step 6 — collapsed mode: **the open decision, made from real rows** (§2.8)

This step is not a check, it is the **decision point for §2.8**. The spec ships
option (a) so there is something on screen to judge; (b2) and (c) are costed and
ready. Read §2.8.2 before running it — it takes two minutes and the branch table
below assumes you have.

**(a) Do:** in the sandbox session (two repos open if you can, so the question
"can I still tell these apart?" is answerable), press **Alt+c** to collapse,
look, then **Alt+c** to expand. Do it a few times.

**(b) Look at:** the collapsed bar. Expect four columns holding three marks and
nothing else — status, the (blank) battery slot, the worktree marker. Compare
against today's collapsed bar, which showed one dot and an identical `…` on
every row. Then ask the one question that decides §2.8:

> **With no text at all, can you tell which row belongs to which repo?**

**(c) Report:** exactly what each collapsed row shows (transcribe, `␣` for a
space); whether anything overflows or wraps; and your answer to the question
above.

| Report | Conclusion | Next |
|---|---|---|
| four columns, three marks, nothing wraps — and losing repo identity while collapsed is **fine**, because collapsed is a glance-at-status mode | **§2.8 resolves to (a).** #24 item 7 is answered as "status + battery + worktree", and its "repo colour" clause is deliberately dropped | ship as specified. Step 7 |
| it works, but **I want repo identity collapsed and I will not pay columns for it** | **§2.8 resolves to (b2).** Cell 3 becomes an always-drawn provenance glyph carrying the repo colour in *both* modes | report — this needs a second glyph (a main-checkout mark), a Rule G/T exception for cell 3, and its own probe round. It is a **follow-up change**, not an S6 amendment: S6's geometry is unaffected |
| it works, but **I want repo identity collapsed and 3 more columns is cheap** | **§2.8 resolves to (c) at 7.** Budget 2 ⇒ one real tinted letter plus `…` | report — `COLLAPSED_TARGET_COLS` 4 → 7 is a one-constant change **in S8's file**, with S8's seek tests to re-baseline. It does **not** break S8's `BAR_TARGET_COLS − COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP` invariant (38 − 7 = 31 > 20; even pre-S8, 30 − 7 = 23 > 20) |
| I want just the *colour*, one cell is enough | **§2.8 (c) at 6.** Budget 1 ⇒ a repo-coloured `…` | report — same one-constant change, one column cheaper. Note that a tinted `…` reads as "truncated" more than as "this repo"; that is exactly the judgement being asked for |
| a `…` still appears at width 4 | §2.9.3's zero-budget change did not land | report — `compose_row_emits_no_ellipsis_at_zero_budget` should have caught it |
| the worktree mark is cut off at the right edge | the collapsed gutter is wider than the pane | report the actual collapsed width. The `const _: () = assert!(GUTTER_COLS_COLLAPSED <= COLLAPSED_TARGET_COLS)` makes this impossible unless the seek stopped *below* 4 (`model.rs:1022-1026` — zellij's resize floor may stop it, and wherever cols stop changing is accepted) |
| the bar snaps back to full width on its own | expected — the seek re-targets the expanded width unless collapsed; use `Alt+c`, not a manual resize | retry |
| the mode flip itself is visually jarring (glyphs jumping together) | the packed↔spaced transition, §2.8.4 | report; the alternative is a permanently packed 4-column gutter in both modes (§2.10.1 row 2), which costs the separation but removes the flip |

### Step 7 — the fallback tier (#40), only if Step 1 or Step 2 demanded it

**(a) Do:** edit the plugin block in `$HOME/.config/clave/layout.kdl` to read
```kdl
plugin location="file:…/clave-bar.wasm" {
    glyphs "plain"
}
```
then relaunch the session the way you normally do (the key is read in `load()`,
so it needs a plugin reload, not just a repaint).

**(b) Look at:** the same rows from Step 2/3. Cell 3 becomes `‡`; a terminal
tab's cell 2 becomes `>`. Text must begin in the same column as before.

**(c) Report:** whether the tier switched, whether anything moved, and whether
the `Plain` marks are legible enough to live with.

| Report | Conclusion | Next |
|---|---|---|
| tier switched, nothing moved, marks legible | **the #40 fallback works**, and §2.6.2's width-identity claim holds live | Step 8 |
| tier switched but a column moved | the two tiers are not width-identical in your terminal | **report immediately** — `glyph_tiers_are_width_identical` and `every_glyph_is_one_cell` both passed, so the host table and yours disagree |
| the key had no effect | the plugin config is not reaching `load()`, or the layout was regenerated by a `clave setup` in between (§2.6.5's known gap) | check the file still contains the key; if it does, report |
| you had to hand-edit and would rather not | **that is #40's follow-up**: `clave setup` should read `CLAVE_GLYPHS` and bake the key. Say so and it gets filed |

### Step 8 — does it actually help? (the reason the feature exists)

**(a) Do:** use the real session normally for a while with at least four rows
across two repos, at least one of them a worktree.

**(b) Look at:** whether the gutter reads as one glance or as clutter; whether
the four columns it cost the text hurt; whether the status dot still reads first.

**(c) Report:** a plain judgement, plus anything that reads worse than before.

| Report | Conclusion | Next |
|---|---|---|
| reads at a glance, status still dominates, the text loss is fine | **ship it**; #24's locked-format marker clause is closed, and item 7 closes per your Step 6 ruling | merge per the autonomy contract, `needs-live-validation` cleared |
| the marks pull attention off the status dot | the three cells compete | report; the lever is the marker's SGR (dim → dimmer, or the marker moved to cell 3-only-when-selected). Do not merge on this reading without a decision |
| the four columns hurt at 30 | **the S8 interaction** (§2.10) | report; the ruling is to land S8's widening before or with S6, which is the recommended order anyway |
| the battery's blank column looks like a bug | expected while S7 is unbuilt | report; if it bothers you, the interim option is to collapse the gutter to two cells until S7 lands — one constant, and G1 already covers it |

---

## 6. Risks, dependencies, coordination, and out of scope

### Dependencies and landing order

| Workstream | Relationship |
|---|---|
| **S5** | **hard dependency, must land first.** S6 replaces exactly one S5 function — `gutter_segments`, which S5 ships as a transitional stand-in for this purpose — and does not touch `compose_row`, `render_segments`, `Ink`, `Segment` or `clamp_name` except for §2.9.3 (§2.9.1). Building the seam here would duplicate S5's |
| **S3** | composes cleanly. S3 changes the dormant glyph expression *above* the `Row` literal; S6 adds fields *inside* it. Same block, trivial conflict, no semantic overlap. S6 strengthens S3's argument (§2.5): dim marks now live in columns 2 and 4, never column 0, and never as circles |
| **S4** | no file overlap in S6's diff. S4 owns `hook.rs`/`add.rs`/`store.rs`'s record; S6 touches `store.rs` only at `snapshot_from`. The one *contract* S6 hands S4 is the text budget, `cols - 7` (§2.10). §2.4.3's optional marker upgrade would sit in S4's `refresh_label` and is deliberately not taken here |
| **S7** (context battery) | consumes the reserved cell. See below |
| **S8** (widths, #24 item 6) | independent but **recommended first** — S6 costs 4 text columns at 30 (§2.10) |

### What S7 inherits, exactly

1. **A reserved cell with proven geometry.** Populate `Row.battery` in `rows()`
   with `Some((char, u8))`; nothing else in the render path changes, and §4.3 G2
   plus §5 Step 4 have already proven no reflow. `compose_row` needs no edit.
2. **An open glyph decision with a live conflict** (§2.6.4): the maintainer's
   pasted MDI battery U+F007C versus #24's own ruling for the lower block eighths
   U+2581–U+2588 — which that ruling chose *specifically* to avoid the font gate
   S6 has now introduced elsewhere. Both are one cell; the argument is about
   shape-carries-magnitude versus a single icon, and about #40.
3. **Rule G/T** (§2.5): the ramp must be **basic SGR**, so it stays inside the
   user's theme and cannot collide with S5's truecolor text palette.
4. **The `GlyphSet` tier** (§2.6.2): whatever ramp S7 picks needs a `Plain`
   sibling, and the block eighths are already the natural one.
5. **The dormant question S6 did not answer**: S6 leaves cell 2 blank on dormant
   rows. S7 must decide whether a dormant agent shows its last-known battery
   (informative, possibly stale) or stays blank (honest, loses a signal).
6. **§2.8's outcome, whatever it is.** Under (a) the collapsed row is three
   glyph cells, so S7's ramp is one third of the entire collapsed bar and its
   legibility at that size matters more than it looks. Under (c) there is text
   beside it. Under (b2) cell 3 is repo-coloured, which constrains how loud the
   ramp beside it can be.
7. **The cadence question from #24**, untouched: clave's hooks fire on
   prompt/stop boundaries, so a meter lags mid-turn; a throttled `PostToolUse`
   entry would track live at the cost of a store RMW per tool call.

### Coordination notes the PR must carry

- **S5's `compose_row_narrow_width_overflow_is_preexisting` is deliberately
  superseded** (§2.9.3). S5 pinned the `budget == 0` ellipsis as pre-existing
  behaviour; S6 removes it because the collapsed row makes that path normal
  rather than unreachable. Named in the PR, not silently deleted.
- **S5's `compose_row_measures_the_gutter_it_is_given` gains a 6-cell case**
  (§4.2) — S5 wrote it as the S6 contract test with 2- and 4-cell gutters, and
  the shipped width is 6.
- **S5's P3 loses its `cols >= gutter_cols + 2` gate** and becomes unconditional
  (§4.3 G3).
- **S6's proptests are numbered G1–G7**, not P1–P7, because S5 already owns
  P1–P8 in the same module (§4.3).
- **S4's width budget parameter is `cols - 7`** expanded, `cols - 5` collapsed
  (§2.10) — S4's `fit_label(name, budget)` needs the number, not a restructure.
- **Both siblings' prose budget figures are stale** (§2.10.1): S8 §1 says 26/34
  (assumed a 3-column gutter), S5 §7 says 33 @38 (assumed 4). The authoritative
  figures are **23 @30 and 31 @38**. Neither needs a structural change — S5
  measures the gutter it is handed and S8 correctly scopes `main.rs:546` out of
  its own diff — only the prose. Whoever lands after S6 corrects it.
- **#24 item 7 is *regressed* under §2.8 option (a)**, and that is a decision the
  maintainer makes at §5 Step 6, not a defect. S5 flags the same thing from its
  side (`S5 §7`, risk table). Whichever option he picks, say in the PR which one
  and why.

### Risks

| Risk | Severity | Mitigation |
|---|---|---|
| A gutter glyph is two cells in the maintainer's terminal (the PUA-advance exception, §2.2.3) | **high — it is the failure this spec exists to prevent** | Step 1 measures it against a ruler before any code is looked at, and Step 2 measures it again on real rows. `every_glyph_is_one_cell` pins the host table at Tier 1. Remedies are ordered in Step 1's branch table: `Mono` font variant, then the `Plain` tier |
| `𖣂` renders as tofu | medium, **expected** | §2.6.3 predicts it and pre-clears `\u{e0a0}`. Geometry is unaffected (tofu is one cell); it is a one-constant change |
| The terminal mark's codepoint is wrong (§2.6.4) | low | Step 1 identifies it by position among four candidates; all four are one cell, so no test's geometry changes |
| 4 fewer text columns at 30 makes rows worse overall | medium | S4's drop policy absorbs it (§2.10); S8 restores it. Step 8's branch table is where the maintainer rules |
| The worktree marker is silent for worktrees clave did not create | medium | §2.4.3 states it precisely, Step 5 measures its real incidence, and the three-line upgrade is written out and ready |
| A hand-edited `glyphs` key is lost to the next `clave setup` | low | §2.6.5 states it; the complete fix is #40's follow-up. Absent = `Full` means the default path is unaffected |
| The three cells clutter more than they inform | medium | Tier 3 by construction. Step 8 is the maintainer's verdict; the interim lever (collapse to two cells) is one constant |
| **#24 item 7 regresses**: repo identity vanishes when collapsed (§2.8.1) | medium | Not mitigated — **decided**. §2.8.2 costs four options, §2.8.3 recommends (a) and names the pre-costed upgrades, and §5 Step 6 puts the choice in front of the maintainer with real rows. S5 flags the same risk from its side |
| §2.8 resolves to (c) after S6 has merged, so `COLLAPSED_TARGET_COLS` moves later | low | The const-assert is an **inequality** (§3.4) and G1/G2 assert against `gutter_cols(...)`, not literals, so the constant can move without touching the gutter or its proofs. The cost is re-baselining S8's seek tests, which S8 already enumerates |
| Issue #44 corrupts a live reading | high, standing | Step 0 is mandatory and terminal |

### Out of scope

- **The context battery itself** (#24 item 4) — S7. S6 reserves the cell and
  nothing more.
- **The model badge** (#24 item 5) — a *fourth* cell, which would be a fourth
  column and a re-run of §5. Not designed here; note that `gutter_sequence` and
  `GUTTER_CELLS` generalise, and that a fourth cell makes
  `GUTTER_COLS_COLLAPSED` = 5 > `COLLAPSED_TARGET_COLS` = 4, tripping §3.4's
  compile-time assertion — which is the assertion doing its job, and which forces
  #24 item 5 to resolve §2.8 first.
- **Worktree provenance in the *name*** (#24 item 1, `<repo> » <worktree-dir>`) —
  S6 renders a marker, not a name. The wire now carries the path it would need.
- **Fixing `worktree`'s false negatives** (§2.4.3) and **fixing `repo_root` for
  `new`-inside-a-worktree** (§2.4.4) — both are `add.rs`/`hook.rs` changes inside
  S4's restructure, both are #24 item 1's territory, and neither moves a column.
- **Baking the `glyphs` key into the generated layouts** (§2.6.5) — #40.
- **A real font check in `doctor`** (§2.6.6) — declined on principle, not
  deferred.
- **Widths** (#24 item 6 / S8): S6 must not change `BAR_TARGET_COLS`
  (`model.rs:137`); the C6 ledger applies to anyone who tries. It must not change
  `COLLAPSED_TARGET_COLS` (`model.rs:142`) **in this batch** either — §2.8 option
  (c) would, and that is why it is written as a follow-up landing in S8's file
  rather than as an S6 amendment.
- **The clamp's scalar-vs-cell bug** for *name* text (§2.2.2 consequence 1). A
  CJK or emoji character in a label still counts as one column when it draws two.
  Pre-existing, unchanged by S6 (whose glyphs are all one cell), and correctly
  S5's clamp to fix — with `unicode-width` now already a dev-dependency if
  someone wants to prove it first.
