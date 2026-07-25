# S5 — per-repo and per-title colour, allocated by iteration (RC-G, closes #24 item 2)

_2026-07-22 · workstream **S5**, root cause **RC-G** of
[`2026-07-22-ux-defect-dossier.md`](2026-07-22-ux-defect-dossier.md) · feature,
not a defect · **revised** after the maintainer overruled the hash_

**The requirement, verbatim from the maintainer.** First statement:

> "I want each repo name to show in a different colour to the others, for visual
> distinction, so all sessions from one repo share a colour and a second repo's
> sessions share a different one — and I mean the text of that cwd name in the
> sidebar. It should go through a list, it can be a circular list of say 10
> colours, and when it reaches the end, it goes back to the beginning."

Overruling the hash, and setting the palette:

> "they just need to be from a repeating set that iterates rather than a hash —
> hashes could collide, and the repeating set should be predefined as light
> coloured text of different and distinct colours but look good on a dark
> background — this will likely become something that matches zellij themes,
> like I currently use kanagawa, pulling in their colours for text would be nice
> and cycling through them."

Adding the second axis:

> "the title should get its own unique colour to differentiate it from other
> titles within that repo. This makes every tab visually identifiable in a
> heartbeat."

Read the dossier's **RC-G** section first
(`2026-07-22-ux-defect-dossier.md:440-499`). Its rendering survey — the two ANSI
sites, the escape-blind clamp, the `Row` shape, the `repo_root` availability, the
`zellij-tile` `Text` limitation — is **not** re-derived here.

## What this closes in #24

| #24 item | S5 |
|---|---|
| 2. "deterministic `repo_root` → ANSI colour" | **closed here**, by iteration rather than by hash |
| 2. "worktrees as a shade/variant of their parent repo's colour" | **not closed** — a worktree gets its parent repo's colour identically. §7 |
| 3 (partially). "rows are hard to tell apart — three `nalu · …` lookalikes" | **the title axis closes exactly this**: same repo, different agents, different title colours |
| 7. "collapsed-state: what 4 cols can still distinguish" | **regressed, not served** — the S6 gutter consumes the whole collapsed width. §7 flags it as an S6/S8 decision |
| 1, 4, 5, 6 | untouched |

## What changed in this revision

| Was | Now |
|---|---|
| FNV-1a hash → palette index, computed in the bar | **store-backed allocation on first sight**, iterating a cursor and wrapping (§2.2) |
| one axis (repo) | **two axes** — repo ink and title ink, both allocated by iteration (§2.3) |
| 10 indexed-256 cube colours, legible on light **and** dark | **12 kanagawa-derived truecolor entries, dark backgrounds only** (§2.4) |
| palette order irrelevant (a hash spreads uniformly) | **palette order is load-bearing** — consecutive allocations must look maximally different (§2.4) |
| theme-sourcing rejected | **roadmap v2**, with the seam specified so it is a palette-source swap (§2.6) |
| `REPO_SEGMENT` global constant | **per-row paint list computed host-side** — no constant to flip (§2.1) |
| budget `cols - 3` | **`cols - gutter_width - 1`**, gutter supplied by S6 (§2.7) |

Kept unchanged: the colour-after-clamp structural seam, the
sanitize → clamp → split → ink order, the P1 escape-integrity and P2
colour-independence proptests, and the finding that no test constructs a `Row`
literal.

---

## 1. Problem and goal

Row text carries no colour today. The only two ANSI sites in the whole plugin are
the status glyph (`crates/clave-bar/src/main.rs:541`) and the active-row reverse
video (`main.rs:555`). With three `nalu · chore/pending-conf…` lookalikes stacked
in a narrow bar (#24, with screenshot on file), the eye has nothing but the first
few characters to separate rows — and the clamp eats exactly those.

**Goal.** Two colour axes, both stable and both allocated by iteration:

1. **Repo ink** — one colour per `repo_root`. Every `clave` row shares it.
2. **Title ink** — one colour per agent, unique among the agents of *that* repo.

Together: the repo tells you *which project* at a glance, the title tells you
*which of that project's agents*. "Every tab visually identifiable in a
heartbeat."

**Non-goal.** Colour is never a load-bearing signal. Ordering, status, staleness
and identity all remain fully expressed without it, and the row text is rendered
character-for-character the same with colour as without — §2.9 makes that a test,
not a promise.

---

## 2. Design

### 2.1 Where the colour attaches — no segment constant survives

S4 (revised in parallel) composes `title · repo · summary`: **title is field 0,
repo is field 1**, branch is gone. The previous revision of this spec had a
`REPO_SEGMENT` constant to flip. **That constant is now deleted**, for a reason
that has nothing to do with S4 churn:

> **The title is optional.** Before Claude's first `custom-title`, S4's
> `compose_label` drops the absent segment without leaving a separator, so the
> label is `repo · summary` and the repo is field **0**. With a title it is field
> **1**. A global constant is wrong for half the fleet at any moment.

The fix is to stop guessing. **The side that composed the label emits the paint
list.** `clave-types` gains:

```rust
pub struct InkSpan { pub segment: u8, pub ink: u8 }
```

read as *"colour the `segment`-th ` · `-delimited field of this row's name with
palette entry `ink`"*, and `Agent` gains `inks: Vec<InkSpan>`. The host builds it
in `snapshot_from`, walking the same fields in the same order `compose_label`
does (§3.2), so composer and painter agree **by construction**. S4 may reorder,
insert or drop segments freely and the bar needs no change.

Confirming the coordinator's question: `segment_span(name, n)` was already written
for arbitrary `n` and is **unchanged** — it is now called once per `InkSpan`
instead of once per row. The cheap part was cheap; the constant was the part worth
deleting.

**Two things S5 must not assume**, both still true after the revision:

1. **`Row.name` for a live row is the ZELLIJ TAB NAME, not the store label** —
   `model.rs:760` is `name: t.name.clone()`. It converges to the label only via
   `Effect::RenameTab`, which fires on label *change* only and deliberately lets a
   user's manual rename stick (`model.rs:569-582`). A manual rename can therefore
   put the ink on the wrong words. Accepted, cosmetic, observed in §6 Step 7.
2. S5 still **declines S4's `fit_label(...) -> Vec<String>` hand-off**: it is
   unreachable for live rows (point 1), S5 owns the gutter/margin arithmetic a
   store-side composer cannot know, and `compose_row` must also handle a plain
   terminal tab, which has no label at all.

### 2.2 Allocation: store-backed, on first sight, iterating and wrapping

The maintainer's objection to the hash is exact: *hashes could collide.* An
iterating allocator cannot collide until the palette is exhausted, and then it
collides in the order he described — "when it reaches the end, it goes back to the
beginning".

Iteration needs **memory**, and memory in clave has exactly one home. The doctrine
is already written down twice over (`store.rs:82-96`: `tab_timeline` and
`collapsed` both live in the store because per-instance copies *diverged live*).
Allocation state must therefore be **in the store, delivered in the snapshot, and
never computed in the bar** — there are N bar instances, one per tab
(`main.rs:20-22`), and any instance-local allocator would produce N different
palettes.

**Where allocation happens.** One function, `store::allocate_inks(&mut Store)`,
idempotent and monotone, called from two places:

| Call site | Role |
|---|---|
| `with_store_mut`, immediately after the caller's closure returns and before the write (`store.rs:151-153`) | **the universal backstop.** Every mutation path — `add.rs`, `dev.rs:226-247`, hooks, binds, prunes — passes through here, so no path can forget, and a store file written by an older clave is healed on its first write |
| inside `add.rs`'s creation closure (`add.rs:740-759`), after `s.agents.insert(...)` and before `snapshot_from(s)` | **the fast path.** The new agent is coloured in the very snapshot that announces it, so a freshly-opened tab is never briefly uncoloured |

Rejected sites: **`clave add` only** — misses `dev.rs` seeding and every legacy
row; **the hook** — hooks run per prompt, so a never-prompted agent stays
uncoloured, and the hook's untracked fast path deliberately skips the lock
(`store.rs:9-12`); **the bar** — ruled out above.

**Concurrency, under the flock.** `with_store_mut` holds an exclusive flock on a
separate, never-renamed lockfile across the whole read → mutate → write
(`store.rs:135-164`). Two `clave add` processes first-sighting the *same* repo
therefore serialize: the winner allocates and writes; the loser's `read_store`
already sees `repo_inks[repo]` populated and allocates nothing. Same index, no
race. Two processes first-sighting *different* repos get indices in flock-arrival
order — nondeterministic across runs, but inherent to "iterate in order of first
sight" and harmless: either order is equally correct.

**Seq is deliberately NOT bumped by `allocate_inks`.** The backstop runs after the
caller has already built its snapshot, so bumping there would either desync that
payload or manufacture a no-op push — and "no no-op pushes" is an explicit §5 rule
enforced at `store.rs:292-294`, `:308-310`, `:327-329`. The consequence is bounded
and named: an agent coloured only by the backstop renders untinted until the next
real push. In practice that is `dev.rs`-seeded rows and legacy store files, both
healed by `clear_tab_timeline` at launch (`setup.rs:647-651`) before any bar
instance exists.

**Surviving `clear_tab_timeline`.** That function is *session-recreate hygiene*: it
wipes `tab_timeline` and every `tab_id` because tab ids are session-scoped
(`store.rs:336-349`). Ink allocation is **not** session-scoped — it is the thing
the maintainer must be able to learn — so the new maps live beside `agents`, not
beside `tab_timeline`, and `clear_tab_timeline` is **not modified**. A test pins
that it leaves every ink map untouched (§5.1); without that test the feature is one
careless `s.repo_inks.clear()` away from reshuffling on every launch.

**Release policy: colours are never released. Recommended, and here is why.**

- Agents are **never deleted** — verified: `grep -rn "agents.remove\|\.agents\.retain" crates/clave/src/` returns nothing. `apply_prune_tabs` clears binds, not rows. So the title ledger only grows with real agents and there is nothing to release.
- The only releasable thing would be a repo whose agents are all gone. Reclaiming it requires a refcount maintained across every write path and every concurrent writer — a new invariant of exactly the class that produced the prune race (#6/#26) and the recycled-tab-id race (`store.rs:274-276`). The ledger's lesson is that cross-process reclamation is where clave's bugs live.
- The user **learns** the mapping. Moving a colour he has learned is worse than reusing one: after release, closing repo A's last tab and opening repo B silently gives B the colour A had, and A gets a different one when it returns. That is the reshuffling the feature exists to prevent.
- Exhaustion is benign and is the maintainer's own stated model: the 13th repo shares with the 1st. Non-release reaches that point sooner; it never behaves *worse* than wrapping, which is the specified behaviour anyway.
- Cost: one short string plus one byte per repo ever seen, and one uuid plus one byte per agent. A heavy user accumulates tens of repos. If it ever matters, a `clave doctor` GC pass is future work (§7), not a v1 concern.

### 2.3 The two axes, and the within-row collision rule

**Repo ink.** Keyed on `repo_root` (the full path — it is the store's own grouping
key, `clave-types/src/lib.rs:44-45`). Allocated from a single global cursor:
`index = repo_ink_cursor % PALETTE_LEN`, cursor incremented.

This is a deliberate change from the previous revision, which keyed on the
*basename*. Iteration makes that argument moot: two checkouts of the same repo at
different paths were given one colour by basename-hashing because a hash has no
memory; with a ledger they are two entries and get two colours, which is *more*
informative and cannot be mistaken for a collision. The one place it shows is the
dev sandbox (`dev.rs:232` seeds repos under a sandbox root), where the sandbox and
the real session now legitimately differ — stated in §6 Step 2's branch table so
it is never misread as a bug.

**Title ink.** Keyed on the agent **uuid**, not the title string. This is the
important choice: Claude renames sessions repeatedly (S4 records 61 `custom-title`
records in one transcript, latest-wins, and `/clear` clears it). Keying on the
title text would flip the colour on every rename — the exact instability the
feature exists to remove. Keying on the uuid (the store's join key, invariant #3,
never changes) means **the tab keeps its colour for life**, which is what
"visually identifiable in a heartbeat" requires. Consequence, stated: two agents in
the same repo that happen to share a title still get different colours. Correct —
they are different tabs.

Allocated from a **per-repo** cursor, so uniqueness is scoped as asked ("unique
among titles *within that repo*").

**Collision within a row — the coordinator's rule confirmed, with one
refinement.** The title allocator skips the repo's own index. The refinement is
where each repo's title cursor *starts*:

```text
title cursor for repo R starts at (repo_index(R) + 1), not at 0
allocate: idx = cursor % LEN;  if idx == repo_index { cursor += 1; idx = cursor % LEN }
          cursor += 1
```

Two properties fall out, both free:

1. **`idx == repo_index` is impossible.** One skip always suffices because
   `PALETTE_LEN >= 2` (asserted, §5.1).
2. **The first agent of every repo is maximally distinct from its own repo tint.**
   The palette is ordered so consecutive entries are as far apart as possible
   (§2.4) — minimum adjacent ΔE 55.2 — so starting at `repo_index + 1` puts the
   commonest case (one agent in a repo) at the largest available distance. It also
   staggers different repos' title cycles, so two repos' *first* agents get
   different title colours rather than both getting entry 0.

Effective title cycle length is `PALETTE_LEN - 1` = 11 per repo.

Rejected: **disjoint sub-palettes** (repo draws from entries 0–5, titles from
6–11). Collision-free without a skip, but it halves both cycles, needs 24 distinct
colours to keep today's headroom, and breaks the maintainer's model of one
repeating set. Rejected: **tint the title a lightness variant of the repo colour** —
attractive, but it makes the two axes *harder* to separate and re-opens the
worktree-shading design §7 defers.

### 2.4 The palette: 12 kanagawa entries, truecolor, dark backgrounds only

The both-backgrounds contrast band from the previous revision is **dropped** — he
only cares about dark. Replaced by three constraints, all measured against
kanagawa's own wave background `sumiInk3 #1F1F28`:

1. **contrast ≥ 5.0** against `sumiInk3` — comfortably past WCAG AA (4.5) for
   normal text, which is what "light coloured text" means operationally;
2. **ΔE (CIE76) ≥ 20 from `fujiWhite #DCD7BA`**, kanagawa's default foreground —
   a tinted word must not read as an untinted one;
3. **mutually distinguishable**, maximised as described below.

Source of truth: `rebelot/kanagawa.nvim`, `lua/kanagawa/colors.lua` (fetched
2026-07-22). Excluded on constraint 2: `fujiWhite`, `oldWhite`. Excluded as
"reads as the dim/idle grey": `fujiGray`, `katanaGray`. Excluded on constraint 1:
`autumnGreen`, `dragonBlue`, `waveAqua1`, `autumnRed`, `samuraiRed`. That leaves a
16-colour pool.

**Count: 12, not 10.** "Say 10" was an approximation; the data says 12 is nearly
free. Best achievable minimum pairwise ΔE over that pool:

| entries | min pairwise ΔE |
|---|---|
| 10 | 19.1 |
| 11 | 18.0 |
| **12** | **16.1** |

Two extra colours cost 3 ΔE units. They buy one more repo before the cycle wraps
and — because the title cycle is `LEN - 1` — 11 rather than 9 distinctly-coloured
agents per repo. With two axes drawing from one table, headroom is worth more than
it was for a single axis.

**Palette order is load-bearing now, and it is not hue order.** With a hash,
assignment was uniform and order meant nothing. With iteration, entry `k` and entry
`k+1` are *the colours the user's next two repos will get*, so adjacent entries
must be maximally unlike. The order below is the cyclic arrangement (found by local
search over permutations) that **maximises the minimum adjacent ΔE**: hue order
would give 14.0; this gives **55.2**.

| slot | kanagawa name | hex | rgb | contrast vs `#1F1F28` | ΔE to next slot |
|---|---|---|---|---|---|
| 0 | `roninYellow` | `#FF9E3B` | 255, 158, 59 | 7.94 | 90.1 |
| 1 | `springBlue` | `#7FB4CA` | 127, 180, 202 | 7.23 | 70.3 |
| 2 | `waveRed` | `#E46876` | 228, 104, 118 | 5.09 | 69.0 |
| 3 | `waveAqua2` | `#7AA89F` | 122, 168, 159 | 6.17 | 85.2 |
| 4 | `peachRed` | `#FF5D62` | 255, 93, 98 | 5.44 | 60.9 |
| 5 | `carpYellow` | `#E6C384` | 230, 195, 132 | 9.73 | 72.0 |
| 6 | `crystalBlue` | `#7E9CD8` | 126, 156, 216 | 5.94 | 83.0 |
| 7 | `surimiOrange` | `#FFA066` | 255, 160, 102 | 8.15 | 68.2 |
| 8 | `lightBlue` | `#A3D4D5` | 163, 212, 213 | 10.07 | 55.2 |
| 9 | `sakuraPink` | `#D27E99` | 210, 126, 153 | 5.59 | 72.0 |
| 10 | `springGreen` | `#98BB6C` | 152, 187, 108 | 7.52 | 59.3 |
| 11 | `oniViolet2` | `#B8B4D0` | 184, 180, 208 | 8.15 | 80.3 (wraps to slot 0) |

The four weakest **non-adjacent** pairs are `springBlue`/`lightBlue` (16.1),
`waveAqua2`/`lightBlue` (17.1), `springBlue`/`waveAqua2` (18.0) and
`roninYellow`/`surimiOrange` (19.1) — a blue-aqua cluster and an orange pair. They
are never allocated consecutively, so they only co-occur once six or more repos
exist. If the maintainer cannot separate them at his font size, dropping
`lightBlue` and `waveAqua2` yields the 10-entry set at ΔE 19.1 — a one-line change
plus the golden test. §6 Step 1 makes that an explicit branch.

**Truecolor (`\x1b[38;2;R;G;Bm`), re-argued rather than inherited.** The previous
revision chose indexed-256 for portability. That answer does not survive the new
requirement:

- Kanagawa's colours are RGB and have **no exact 256-cube equivalent**.
  `crystalBlue #7E9CD8` quantises to cube index 110, `#87afd7` — visibly a
  different blue. "Pull in kanagawa's colours" and "indexed-256" are not
  simultaneously satisfiable.
- zellij treats RGB as first class: `PaletteColor::Rgb((u8, u8, u8))`
  (`zellij-utils-0.44.3/src/data.rs:1211-1213`), and its themes are authored in
  RGB. The v2 theme path (§2.6) hands back `PaletteColor`, so an indexed-only
  renderer would have to quantise the user's own theme.
- The old portability worry was *silent downsampling by an intermediate
  multiplexer*. Re-examined: clave **is** the multiplexer session, so the nesting
  case is not the use case; over plain SSH the escape bytes pass through untouched
  and the decision is made by the user's own terminal emulator at the near end.
  Every terminal that runs zellij comfortably (kitty, wezterm, ghostty, alacritty,
  iTerm2, Windows Terminal, foot) supports truecolor.
- Degradation is graceful and non-structural: a terminal without truecolor
  approximates the colour. It does not leak escapes and it does not break the
  clamp.

`Ink::Indexed(u8)` is nevertheless **retained** in the enum. If Step 1 turns up a
terminal that mangles truecolor, a nearest-cube fallback table is a data change
plus one line, not a redesign.

### 2.5 Are the status glyph colours still safe?

Yes, and the reasoning changed. The glyph colours are *basic* SGR (31/33/32/90 —
`clave-types/src/lib.rs:24-32`; dormant `('◌', 90)` at `model.rs:775`), which the
user's theme remaps. The repo/title palette is truecolor, which no theme touches.
Different families, different positions (the fixed gutter versus the name), and
after S6 the gutter is glyphs only. A kanagawa-red name field and a themed red
status dot sit adjacent but are never ambiguous — one is a `●`, the other is a
word.

### 2.6 Theme sourcing: v1 hardcoded, v2 from `ModeUpdate` — and the seam

Promoted from "rejected alternative" to **roadmap**, as instructed.

**v1 — hardcoded kanagawa.** The table in §2.4, in `clave-types`. Works on the
maintainer's machine today; needs no subscription, no runtime state, no
per-instance agreement.

**v2 — the user's zellij theme.** `Event::ModeUpdate(ModeInfo)` carries
`style: Style`, and `Style` carries `colors: Styling`
(`zellij-utils-0.44.3/src/data.rs:1375-1379`). Verified in the vendored source:
`Styling` contains **`multiplayer_user_colors: MultiplayerColors`** with fields
`player_1 … player_10` (`data.rs:1700-1716`) — **a ten-entry categorical ramp that
zellij itself derives from the active theme for the express purpose of telling
participants apart.** That is precisely the shape v1 hardcodes, sourced from the
user's own theme, and it retires the objection the previous revision raised
("`Styling` has no categorical ramp"). It supplies 10; the remaining 2 come from
the theme's `emphasis_*` slots, or the palette shrinks to 10 under a theme.

**The seam, so v2 is a palette-source swap and nothing else.** Three rules, all
enforced in v1:

1. **The store persists INDICES, never colours.** `repo_inks`/`title_inks` hold
   `u8` palette positions; `InkSpan.ink` is a position. A theme change therefore
   reassigns nothing and needs no store migration. *This is the load-bearing rule* —
   had the store held RGB, v2 would be a data migration.
2. **Index → colour resolves in exactly one place**: `BarModel::ink(idx) -> Option<Ink>`,
   reading `BarModel.palette: Vec<Ink>`, itself initialised from
   `clave_types::PALETTE`. `rows()` calls it; nothing else does.
3. **v2 is then**: subscribe `EventType::ModeUpdate` (`main.rs:363-375`), and on
   the event overwrite `BarModel.palette` from `mode_info.style.colors`. Zero other
   lines. The storm risk that justified the previous rejection is handled by the
   discipline the rest of the plugin already uses: the handler returns `true`
   (repaint) **only when the derived palette actually changed**, so mode toggles
   that leave colours alone cost nothing — the change-gating pattern of
   `apply_bind`/`apply_collapse` (`store.rs:246-249`, `:308-310`).

v2 is explicitly **not** in this workstream's scope; the deliverable here is that
its diff is (1) a subscription, (2) a `Styling → Vec<Ink>` function, (3) a
change-gated assignment.

`zellij-tile`'s `Text` builder (`ui_components/text.rs`, `color_range`,
`DIM_LEVEL`) stays rejected for the reason the dossier records: semantic
index-levels resolved host-side, **no arbitrary-palette API**. "Colour #7 of my
twelve" is unsayable in it.

### 2.7 The render restructure — colour after the clamp, by construction

Unchanged in principle, extended in three ways: the gutter is a parameter, the
budget derives from it, and the paint list is a list.

Today's renderer (`crates/clave-bar/src/main.rs:539-559`) clamps by counting
Unicode scalars straight off `row.name`:

```rust
let budget = cols.saturating_sub(3);          // gutter + margin
let name: String = if row.name.chars().count() > budget {
    let mut n: String = row.name.chars().take(budget.saturating_sub(1)).collect();
    n.push('…'); n
} else { row.name.clone() };
```

ANSI placed *inside* `row.name` would be counted as visible characters: truncating
text early, or cutting mid-escape and leaking `[38;2;255` into the pane, or
emitting an introducer whose reset fell off the end and bleeding colour down the
line. "Apply colour after clamping" cannot be left to discipline, because
`main.rs` is `test = false` (`crates/clave-bar/Cargo.toml:25`) — nothing would
ever catch a regression.

**The structural answer, unchanged: the plugin gets exactly one escape-emitting
function, reachable only through a type that cannot hold an escape.**

```rust
rows()  →  Row { name: String (plain), inks: Vec<(usize, Ink)>, … }
              │
              ▼   model.rs, pure, host-tested
        compose_row(&Row, cols, gutter: &[Segment]) -> Vec<Segment>
              │        sanitize → clamp → resolve spans → attach ink
              ▼
        render_segments(&[Segment]) -> String
              │        THE ONLY \x1b in crates/clave-bar
              ▼
        main.rs: println!("{}", …)
```

**The gutter is given, not built.** S6 owns the first three glyphs (status,
reserved battery slot, worktree marker). `compose_row` takes them as
`&[Segment]`, measures them (`text.chars().count()` — exact, because
`Segment.text` is escape-free by construction), and derives

```rust
budget = cols - gutter_cols - RIGHT_MARGIN_COLS
```

replacing the hardcoded `- 3`. Until S6 lands, `model::gutter_segments(&Row)`
reproduces today's 2-cell gutter from `row.glyph`; S6 replaces that one function
and touches nothing else. S5 **colours text only** and never inspects the gutter's
contents.

**Multiple spans.** `compose_row` resolves each `(segment_index, ink)` against the
**clamped** name via `segment_span`, drops spans truncation removed, sorts by
start, and walks left to right emitting alternating plain and inked runs. Four
properties fall out, each a test rather than a comment:

1. **The clamp never sees an escape.** `compose_row` sanitizes (drops every
   `char::is_control()`) and *then* clamps, before any ink is attached.
   `render_segments` writes complete `\x1b[…m` … `\x1b[0m` pairs in one push — a
   partial sequence is unrepresentable.
2. **Truncation is handled by resolving spans against the CLAMPED string.** A
   truncated field yields a shorter span; a field truncation removed entirely
   yields `None` and is simply not painted. If the clamp ate the separator before
   the repo, the repo span is absent and the surviving text stays plain — correct,
   and it needs no branch. If the clamp landed mid-separator (`"F-CLA ·…"`), two
   extra characters carry the preceding field's ink; cosmetically invisible,
   explicitly accepted, pinned by a test.
3. **Reverse video composes.** The active row's SGR 7 is carried per segment
   rather than wrapped around the whole name, so an inked segment's `\x1b[0m`
   cannot cancel the highlight on the segments after it. An active inked segment
   emits `\x1b[7;38;2;255;158;59m…`.
4. **The gutter and budget arithmetic move into the tested half.**

Sanitizing also fixes a real latent bug for free: a store label containing a
newline (labels descend from Claude's transcript, `hook.rs:120-135`) currently
breaks the one-line-per-row contract that `click()` depends on
(`model.rs:800-803` indexes rows by rendered line).

The pre-existing behaviour when `budget == 0` — name non-empty, so `take(0)` plus
`'…'` emits one character in a zero-cell budget — is **preserved verbatim** and
pinned by a test naming it as pre-existing.

### 2.8 Widths: hardcode neither 30 nor 38

S8 widens the bar from 30 to ~38. Nothing in S5 may hardcode either: `compose_row`
takes `cols`, and every truncation test runs over `[30, 38]` (§5.1). S5 must not
change `BAR_TARGET_COLS` or `COLLAPSED_TARGET_COLS` (`model.rs:137,142`) — those
are S8's, and the C6 ledger governs them.

### 2.9 Accessibility: colour is decoration, and that is checkable

- Row **order** comes from the timeline, untouched (`model.rs:391-393,791`).
- Row **status** comes from the glyph, which keeps a distinct *character* as well
  as a colour (`● ✖ ◌`) — colour was never its only channel.
- Row **identity** comes from the name text, byte-identical with and without ink.

Pinned: stripping all ink from `compose_row`'s output must reproduce exactly the
pre-S5 visible line. A monochrome terminal, a colour-blind reader and a
screen-scraper all see what they saw before. The honest caveat: the *added value*
of this feature is colour-only, so a colour-blind user gains nothing — but loses
nothing either, which is the requirement.

### 2.10 Rejected alternatives

| Rejected | Why |
|---|---|
| Hash → palette index | **overruled by the maintainer** — hashes collide. Also: a hash cannot honour "unique within this repo", which the title axis requires |
| Colour the whole row | he was explicit — *"I mean the text of that cwd name"* |
| Colour the status glyph instead | its colour already encodes status; overloading deletes a signal |
| Allocate in the bar | N instances, N allocators, N palettes — the divergence class the store doctrine exists to prevent (`store.rs:82-96`) |
| Persist RGB in the store | breaks the v2 theme seam: a theme change becomes a store migration (§2.6 rule 1) |
| Key title ink on the title string | Claude renames constantly and `/clear` clears it; the colour would flip on every rename |
| Release colours when a repo empties | §2.2 — cross-process refcounting, and it moves a colour the user has learned |
| Disjoint repo/title sub-palettes | halves both cycles and needs 24 distinct colours; breaks "one repeating set" |
| `REPO_SEGMENT` constant | §2.1 — wrong for every agent without a title, independent of S4 |
| Embed ANSI in `Row.name` | §2.7 — the clamp counts scalars; the RC-G defect |
| Indexed-256 | §2.4 — kanagawa has no cube equivalents, and v2 hands back RGB |
| `zellij-tile` `Text` + `color_range` | no arbitrary-palette API (RC-G) |

---

## 3. Implementation

Ordered so each step compiles. Red-first throughout.

### 3.1 `crates/clave-types/src/lib.rs` — palette, `InkSpan`, wire field

Append after `impl Status` (currently ends `:33`):

```rust
/// Twelve foreground colours for the bar's per-repo and per-title tints
/// (#24 item 2). Derived from kanagawa (`rebelot/kanagawa.nvim`,
/// `lua/kanagawa/colors.lua`), the maintainer's theme, and constrained to:
///   * contrast >= 5.0 against kanagawa's `sumiInk3` #1F1F28 background
///     (past WCAG AA for normal text — "light coloured text");
///   * CIE76 dE >= 20 from `fujiWhite` #DCD7BA, the default foreground, so a
///     tinted word never reads as an untinted one;
///   * mutually distinguishable — minimum pairwise dE 16.1.
///
/// ORDER IS LOAD-BEARING. Allocation ITERATES this array, so entry k and
/// k+1 are the colours the user's next two repos receive. The order below
/// maximises the MINIMUM ADJACENT dE (55.2; hue order would give 14.0). Do
/// not "tidy" it into rainbow order — that would make consecutive
/// allocations look alike, which is the one thing this must not do.
///
/// Truecolor, not indexed-256: kanagawa's values have no exact cube
/// equivalent (`crystalBlue` #7E9CD8 quantises to #87afd7, a visibly
/// different blue), and the v2 theme path hands back RGB.
pub const PALETTE: [(u8, u8, u8); 12] = [
    (255, 158, 59),  // #FF9E3B roninYellow
    (127, 180, 202), // #7FB4CA springBlue
    (228, 104, 118), // #E46876 waveRed
    (122, 168, 159), // #7AA89F waveAqua2
    (255, 93, 98),   // #FF5D62 peachRed
    (230, 195, 132), // #E6C384 carpYellow
    (126, 156, 216), // #7E9CD8 crystalBlue
    (255, 160, 102), // #FFA066 surimiOrange
    (163, 212, 213), // #A3D4D5 lightBlue
    (210, 126, 153), // #D27E99 sakuraPink
    (152, 187, 108), // #98BB6C springGreen
    (184, 180, 208), // #B8B4D0 oniViolet2
];

/// Must stay >= 2: the title allocator skips the repo's own index and relies
/// on ONE skip always finding a different entry (§2.3).
pub const PALETTE_LEN: usize = PALETTE.len();

/// "Colour the `segment`-th ` · `-delimited field of this row's name with
/// palette entry `ink`."
///
/// Emitted by the HOST, which composed the label and therefore knows where
/// each field landed — the bar never guesses. That matters because the title
/// segment is OPTIONAL: with a title the repo is field 1, without one it is
/// field 0, so no constant can be right (§2.1). It also means S4 may reorder
/// segments with no change here at all.
///
/// `ink` is a palette INDEX, never a colour: the store persists indices so
/// that swapping the palette (v2 theme sourcing) reassigns nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InkSpan {
    pub segment: u8,
    pub ink: u8,
}

/// The label segment separator: U+0020 U+00B7 U+0020 (bytes `20 c2 b7 20`).
/// One constant, two crates — `add.rs` and S4's `compose_label` build with
/// it, `clave-bar`'s `segment_span` splits on it.
pub const LABEL_SEP: &str = " · ";
```

(If S4 lands `LABEL_SEP` first, delete this copy and import theirs.)

Extend `Agent` (`:39-68`):

```rust
    /// Host-computed paint list (§S5). `default` keeps pre-field payloads
    /// parseable, and an empty list means "no tint" — the correct rendering
    /// for a plain terminal tab AND for an old CLI talking to a new bar.
    #[serde(default)]
    pub inks: Vec<InkSpan>,
```

### 3.2 `crates/clave/src/store.rs` — the allocation ledger

**(a) Extend `Store`** (`:74-97`), beside `agents` and deliberately **not** beside
`tab_timeline`:

```rust
    /// repo_root → palette index (§S5). Append-only allocation ledger:
    /// allocation is on FIRST SIGHT and never released, so a colour the user
    /// has learned never moves. NOT session-scoped — `clear_tab_timeline`
    /// must never touch this, or every launch would reshuffle.
    #[serde(default)]
    pub repo_inks: BTreeMap<String, u8>,
    /// Monotone cursor for `repo_inks`; index = cursor % PALETTE_LEN. A
    /// counter rather than `repo_inks.len()` so the wrap point is explicit
    /// and survives any future removal.
    #[serde(default)]
    pub repo_ink_cursor: u64,
    /// agent uuid → palette index for that agent's TITLE field. Keyed on
    /// UUID, not on the title text: Claude renames sessions repeatedly and
    /// `/clear` clears the rename, so a text key would flip the colour on
    /// every rename. The tab keeps its colour for life.
    #[serde(default)]
    pub title_inks: BTreeMap<String, u8>,
    /// repo_root → monotone cursor for that repo's title allocations, so
    /// title colours are unique WITHIN a repo (the maintainer's ask). Seeded
    /// at `repo_index + 1` (§2.3) so a repo's first agent sits at the
    /// palette's maximum distance from its own repo tint.
    #[serde(default)]
    pub title_ink_cursors: BTreeMap<String, u64>,
```

**(b) New function:**

```rust
/// Allocate inks for every agent that lacks them. Idempotent, monotone, and
/// ALWAYS called under the store flock — that is what makes concurrent
/// first-sight of the same repo safe: writers serialize, and the loser reads
/// the winner's assignment instead of allocating a second one.
///
/// Deliberately does NOT bump `seq`. It runs after the caller's closure has
/// already built its snapshot, so a bump here would either desync that
/// payload or manufacture a no-op push (§5 forbids those — store.rs:292-294).
/// The cost is bounded: an agent coloured only by this backstop renders
/// untinted until the next real push. `add.rs` closes that window for the one
/// path where a human is watching.
pub fn allocate_inks(s: &mut Store) -> bool {
    let len = clave_types::PALETTE_LEN as u64;
    debug_assert!(len >= 2, "the title skip needs at least two palette entries");
    let mut changed = false;
    // Snapshot the (uuid, repo) pairs first: BTreeMap iteration is
    // uuid-ascending, so a batch of first-sightings allocates identically on
    // every machine and in every replay.
    let pending: Vec<(String, String)> = s
        .agents
        .values()
        .filter(|r| !r.repo_root.is_empty())
        .map(|r| (r.uuid.clone(), r.repo_root.clone()))
        .collect();
    for (uuid, repo) in pending {
        let repo_idx = match s.repo_inks.get(&repo) {
            Some(i) => *i,
            None => {
                let i = (s.repo_ink_cursor % len) as u8;
                s.repo_ink_cursor += 1;
                s.repo_inks.insert(repo.clone(), i);
                changed = true;
                i
            }
        };
        if !s.title_inks.contains_key(&uuid) {
            let cur = s
                .title_ink_cursors
                .entry(repo.clone())
                .or_insert(repo_idx as u64 + 1);
            let mut idx = (*cur % len) as u8;
            if idx == repo_idx {
                *cur += 1; // one skip always suffices: PALETTE_LEN >= 2
                idx = (*cur % len) as u8;
            }
            *cur += 1;
            s.title_inks.insert(uuid.clone(), idx);
            changed = true;
        }
    }
    changed
}
```

**(c) Wire the backstop into `with_store_mut`.** Replace `store.rs:151-152`:

```rust
    let mut store = read_store(paths)?;
    let out = f(&mut store);
```

with:

```rust
    let mut store = read_store(paths)?;
    let out = f(&mut store);
    // Universal ink backstop (§S5): every mutation path passes through here,
    // so no writer can forget, and a store file written by an older clave is
    // healed on its first write. Idempotent; see allocate_inks on why no seq.
    allocate_inks(&mut store);
```

**(d) Extend `snapshot_from`** (`:167-189`) to emit the paint list. This mirrors
S4's `compose_label` field order (`title · repo · summary`), and a test pins the
two in agreement (§5.1). Replace the `.map(|r| Agent { … })` closure body's tail
with:

```rust
            .map(|r| {
                // Field indices MUST match compose_label's emission order.
                // The title is OPTIONAL, so the repo is field 1 with a title
                // and field 0 without — computed, never assumed (§2.1).
                let mut inks = Vec::new();
                let mut seg = 0u8;
                if r.title.is_some()
                    && let Some(i) = store.title_inks.get(&r.uuid)
                {
                    inks.push(InkSpan { segment: seg, ink: *i });
                    seg += 1;
                }
                if let Some(i) = store.repo_inks.get(&r.repo_root) {
                    inks.push(InkSpan { segment: seg, ink: *i });
                }
                Agent {
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
                    inks,
                }
            })
```

`r.title` is S4's field. If S5 lands first, omit the title branch entirely and add
it with S4 — §7 sequences it.

**(e) `clear_tab_timeline` (`:340-349`) is NOT modified.** §5.1 pins that.

### 3.3 `crates/clave/src/add.rs` — the fast path

In the creation closure (`add.rs:740-759`), between the insert and the snapshot:

```rust
        let merged = merge_resume_record(s.agents.get(&uuid), fresh);
        s.agents.insert(uuid.clone(), merged);
        // Colour the new agent in the SAME snapshot that announces it — the
        // with_store_mut backstop runs AFTER this closure, so without this
        // line a freshly-opened tab renders untinted until the next push.
        crate::store::allocate_inks(s);
        s.seq += 1;
        snapshot_from(s)
```

`merge_resume_record` (`:343-355`) needs no change: inks live in `Store`, not in
`AgentRecord`, so a resumed agent keeps its colour automatically. §5.1 pins it.

### 3.4 `crates/clave-bar/src/model.rs` — `Row`, the palette, `rows()`

Import (`model.rs:12`):

```rust
use clave_types::{Agent, AgentSnapshot, LABEL_SEP, PALETTE, Status};
```

**(a) Replace `Row`** (`:161-169`):

```rust
/// One rendered row, already in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: RowKey,
    /// PLAIN text — never contains an escape. `compose_row` is the only
    /// consumer allowed to add one (RC-G: the width clamp counts Unicode
    /// scalars, so ANSI in here truncates mid-escape).
    pub name: String,
    pub active: bool,
    /// (glyph, ANSI colour) for agent rows; None for plain terminal tabs.
    /// S6 takes ownership of the gutter; until then `gutter_segments` reads
    /// this.
    pub glyph: Option<(char, u8)>,
    /// Resolved paint list: (field index in `name`, colour). Empty for a
    /// plain terminal tab. Positions come from the HOST (`InkSpan`), colours
    /// from `BarModel::ink` — the single index→colour resolution point that
    /// makes v2 theme sourcing a palette swap (§2.6).
    pub inks: Vec<(usize, Ink)>,
}
```

**(b) Add to `BarModel`**, beside the other display state:

```rust
    /// Index → colour. Initialised from `clave_types::PALETTE`; v2 overwrites
    /// it from the user's zellij theme (`ModeUpdate` → `Styling`). The ONE
    /// resolution point — the store persists indices, never colours.
    palette: Vec<Ink>,
```

initialised in `Default` as
`PALETTE.iter().map(|&(r, g, b)| Ink::Rgb(r, g, b)).collect()`, with:

```rust
    /// Resolve a palette index. Out-of-range WRAPS rather than panicking: a
    /// snapshot from a newer clave with a longer palette must degrade, not
    /// crash a wasm plugin whose backtrace the user never sees.
    fn ink(&self, idx: u8) -> Option<Ink> {
        if self.palette.is_empty() {
            return None;
        }
        Some(self.palette[idx as usize % self.palette.len()])
    }

    /// Build a row's paint list from the agent's host-computed spans.
    fn row_inks(&self, a: &Agent) -> Vec<(usize, Ink)> {
        a.inks
            .iter()
            .filter_map(|s| self.ink(s.ink).map(|i| (s.segment as usize, i)))
            .collect()
    }
```

**(c) `rows()`, live-tab branch.** Replace `model.rs:746-765`:

```rust
        for t in &self.tabs {
            let glyph = self.agent_in_tab(t.tab_id).map(|a| {
                // Local unread override: render Done as Idle once seen.
                if a.status == Status::Done && self.read_locally.contains(&a.uuid) {
                    Status::Idle.glyph()
                } else {
                    a.status.glyph()
                }
            });
            entries.push((
                self.sort_key(t),
                t.position,
                Row {
                    key: RowKey::Tab(t.tab_id),
                    name: t.name.clone(),
                    // A dormant selection steals the highlight from every tab.
                    active: selected_dormant.is_none() && t.active,
                    glyph,
                },
            ));
        }
```

with (the join at `:747` was already computed and discarded; it is now bound and
read twice):

```rust
        for t in &self.tabs {
            let joined = self.agent_in_tab(t.tab_id);
            let glyph = joined.map(|a| {
                // Local unread override: render Done as Idle once seen.
                if a.status == Status::Done && self.read_locally.contains(&a.uuid) {
                    Status::Idle.glyph()
                } else {
                    a.status.glyph()
                }
            });
            // A plain terminal tab has no joined agent → no tint.
            let inks = joined.map(|a| self.row_inks(a)).unwrap_or_default();
            entries.push((
                self.sort_key(t),
                t.position,
                Row {
                    key: RowKey::Tab(t.tab_id),
                    name: t.name.clone(),
                    // A dormant selection steals the highlight from every tab.
                    active: selected_dormant.is_none() && t.active,
                    glyph,
                    inks,
                },
            ));
        }
```

**(d) Dormant branch.** Replace `model.rs:783-789`:

```rust
                Row {
                    key: RowKey::Dormant(a.uuid.clone()),
                    name: a.label.clone(),
                    active: selected_dormant == Some(a.uuid.as_str()),
                    glyph: Some(glyph),
                },
```

with:

```rust
                Row {
                    key: RowKey::Dormant(a.uuid.clone()),
                    name: a.label.clone(),
                    active: selected_dormant == Some(a.uuid.as_str()),
                    glyph: Some(glyph),
                    inks: self.row_inks(a),
                },
```

### 3.5 `crates/clave-bar/src/model.rs` — the composition seam (new)

New section after `Row`. This is the code `main.rs` currently owns and that no
test can reach.

```rust
/// The 1-cell right margin the renderer has always reserved. The GUTTER is
/// no longer a constant: S6 owns it and passes it in, so its width is
/// MEASURED, not assumed (§2.7).
const RIGHT_MARGIN_COLS: usize = 1;

/// How a segment is coloured. Three families, and the split is the point:
/// `Sgr` is a basic code (31/33/32/90 — `Status::glyph`) which the user's
/// THEME remaps; `Rgb` is truecolor, which no theme touches, and is where
/// the repo/title palette lives; `Indexed` is retained so a nearest-cube
/// fallback for a terminal without truecolor is a data change, not a
/// redesign (§2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Sgr(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// One run of PLAIN text plus the attributes that wrap it.
///
/// `text` is escape-free by construction: `compose_row` is its only producer
/// and it sanitizes control characters BEFORE clamping. That is what makes
/// "colour after the clamp" structural rather than a rule someone has to
/// remember (RC-G: the clamp counts Unicode scalars).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub ink: Option<Ink>,
    /// SGR 7 reverse video — the active-row highlight. Carried PER SEGMENT,
    /// so an inked segment's reset can't cancel the highlight after it.
    pub reverse: bool,
}

/// Byte range of the `n`th `LABEL_SEP`-delimited field, or `None` if the name
/// has fewer than `n + 1` fields. Separator bytes belong to neither
/// neighbour: field 0 of `"a · b"` is `"a"`, field 1 is `"b"`. Always a char
/// boundary (`LABEL_SEP` is a literal).
///
/// Called on the CLAMPED name, never the original, so a field truncation
/// removed simply returns `None` and is not painted.
fn segment_span(name: &str, n: usize) -> Option<std::ops::Range<usize>> {
    let mut start = 0usize;
    for _ in 0..n {
        let i = name[start..].find(LABEL_SEP)? + start;
        start = i + LABEL_SEP.len();
    }
    let end = name[start..]
        .find(LABEL_SEP)
        .map_or(name.len(), |i| i + start);
    Some(start..end)
}

/// Drop control characters (escapes, newlines, tabs) and clamp to `budget`
/// display cells with a trailing `…`, char-boundary safe.
///
/// Sanitizing first is load-bearing twice: an escape in a label would
/// otherwise be counted as visible width AND could be cut mid-sequence, and a
/// newline would break the one-line-per-row contract `click()` indexes on
/// (`model.rs:800-803`). Labels descend from Claude's transcript
/// (`hook.rs:120-135`) — not trusted text.
///
/// `budget == 0` emits a single `…`, one cell over budget. PRE-EXISTING
/// (`main.rs:547-553`), preserved verbatim and pinned by a test.
fn clamp_name(name: &str, budget: usize) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() <= budget {
        return clean;
    }
    let mut n: String = clean.chars().take(budget.saturating_sub(1)).collect();
    n.push('…');
    n
}

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

/// The whole rendered line for one row: the caller's gutter, then the row's
/// name clamped to what is left and painted per the row's ink spans.
///
/// Order of operations, and it is the design: sanitize → clamp → resolve
/// spans against the CLAMPED text → attach ink. Ink is attached LAST and only
/// to whole segments, so no escape can be truncated and no line can end
/// mid-colour.
///
/// `cols` is a parameter and no width is hardcoded: the bar is 30 today and
/// ~38 after S8 (§2.8).
pub fn compose_row(row: &Row, cols: usize, gutter: &[Segment]) -> Vec<Segment> {
    let mut out: Vec<Segment> = gutter.to_vec();
    // Exact, because Segment.text is escape-free by construction.
    let gutter_cols: usize = gutter.iter().map(|s| s.text.chars().count()).sum();
    let budget = cols.saturating_sub(gutter_cols + RIGHT_MARGIN_COLS);
    let name = clamp_name(&row.name, budget);

    let mut spans: Vec<(std::ops::Range<usize>, Ink)> = row
        .inks
        .iter()
        .filter_map(|(n, ink)| segment_span(&name, *n).map(|r| (r, *ink)))
        .filter(|(r, _)| !r.is_empty())
        .collect();
    spans.sort_by_key(|(r, _)| r.start);

    let active = row.active;
    let mut push = |text: &str, ink: Option<Ink>| {
        if !text.is_empty() {
            out.push(Segment { text: text.to_string(), ink, reverse: active });
        }
    };
    let mut cur = 0usize;
    for (r, ink) in spans {
        if r.start < cur {
            continue; // defensive: spans from a malformed payload never overlap
        }
        push(&name[cur..r.start], None);
        push(&name[r.clone()], Some(ink));
        cur = r.end;
    }
    push(&name[cur..], None);
    out
}

/// Serialize segments to a terminal line. **The only place in `clave-bar`
/// that writes an escape byte.** Every introducer is emitted together with
/// its `\x1b[0m` in one push, so a partial or unreset sequence is
/// unrepresentable; `Segment.text` is escape-free by construction.
pub fn render_segments(segs: &[Segment]) -> String {
    let mut out = String::new();
    for s in segs {
        let mut params: Vec<String> = Vec::new();
        if s.reverse {
            params.push("7".to_string());
        }
        match s.ink {
            Some(Ink::Sgr(c)) => params.push(c.to_string()),
            Some(Ink::Indexed(n)) => params.push(format!("38;5;{n}")),
            Some(Ink::Rgb(r, g, b)) => params.push(format!("38;2;{r};{g};{b}")),
            None => {}
        }
        if params.is_empty() {
            out.push_str(&s.text);
        } else {
            out.push_str("\u{1b}[");
            out.push_str(&params.join(";"));
            out.push('m');
            out.push_str(&s.text);
            out.push_str("\u{1b}[0m");
        }
    }
    out
}
```

### 3.6 `crates/clave-bar/src/main.rs` — the adapter shrinks

Replace `main.rs:536-559` in full:

```rust
        // One line per tab, display-ordered. Active row inverted (SGR 7);
        // agent rows get their state glyph; plain tabs a 2-space gutter so
        // names align. Truncate to the pane width (raw ANSI is S1-proven).
        for row in self.model.rows() {
            let gutter = match row.glyph {
                Some((glyph, colour)) => format!("\u{1b}[{colour}m{glyph}\u{1b}[0m "),
                None => "  ".to_string(),
            };
            // Clamp the NAME to what's left after the 2-cell gutter, with a
            // trailing … (char-boundary safe; labels can be multibyte).
            let budget = cols.saturating_sub(3); // gutter + margin
            let name: String = if row.name.chars().count() > budget {
                let mut n: String = row.name.chars().take(budget.saturating_sub(1)).collect();
                n.push('…');
                n
            } else {
                row.name.clone()
            };
            if row.active {
                println!("{gutter}\u{1b}[7m{name}\u{1b}[0m");
            } else {
                println!("{gutter}{name}");
            }
        }
```

with:

```rust
        // One line per tab, display-ordered. Composition (gutter, width
        // clamp, repo/title tint, active-row inversion) lives in model.rs
        // where it host-tests: this file is `test = false`, so anything
        // decided here is unguarded forever. Raw ANSI is S1-proven.
        for row in self.model.rows() {
            let gutter = gutter_segments(&row); // S6 takes ownership of this
            println!("{}", render_segments(&compose_row(&row, cols, &gutter)));
        }
```

and extend the import at `main.rs:10-12`:

```rust
use clave_bar::model::{
    BarModel, DWELL_SECS, Effect, PEEK_SINK_SECS, PaneMeta, TabMeta, TimerKind, classify_timer,
    compose_row, gutter_segments, render_segments,
};
```

Net: `main.rs` loses 24 lines of untestable logic and gains three.

### 3.7 What does **not** change

- **No new zellij subscription or permission** in v1 (`main.rs:357-375` untouched).
  The `ModeUpdate` subscription is v2's.
- **No generated artifact change**, so the KDL guardrail is unaffected.
- **No CLI surface**, so no `Cli::try_parse_from` pin is owed.
- **No new dependency** in any crate.
- `Status::glyph()` and the dormant glyph table (`model.rs:770-777`) keep their
  basic-SGR codes exactly.
- `clear_tab_timeline`, `merge_resume_record`, `BAR_TARGET_COLS`,
  `COLLAPSED_TARGET_COLS`.

---

## 4. Risk class

Two rows of the taxonomy (`docs/dev/TESTING.md:112-120`), and the second is **new
to this revision** — store-backed allocation moved S5 across the process seam:

| Class | Why it applies | What it demands |
|---|---|---|
| **Pure logic / model** | `rows()`, `compose_row`, `render_segments`, `allocate_inks` | TDD red-first; `cargo test --workspace`; extend proptests for newly-reachable branches |
| **Cross-process / IPC** | `allocate_inks` is a **multi-writer store path** — every `clave` process reaches it through `with_store_mut` | written ordering/idempotency argument in the PR dossier; an adversarial reviewer must attack it; tier-2 coverage once #47 lands |
| **Visual / UX** | the palette | human judgement only |

Labels: `needs-live-validation` **and** `host-untestable`.

**The ordering/idempotency argument, for the dossier** (§2.2 in short form):
`allocate_inks` runs only inside `with_store_mut`, which holds an exclusive flock
on a separate never-renamed lockfile across the whole read → mutate → write
(`store.rs:135-164`). It is idempotent — an agent with an entry is skipped — and
monotone: cursors only increase and no path removes a ledger entry. Concurrent
first-sight of the same repo therefore serializes to one allocation and the loser
observes it. Concurrent first-sight of *different* repos yields flock-arrival
order, nondeterministic but equally correct under an order-of-first-sight rule.
There is no payload that can arrive out of order — unlike `prune-tabs`, this is not
a fire-and-forget subprocess message but an in-lock derivation from the store's own
contents.

---

## 5. Test plan

**The point of the plan.** `crates/clave-bar/src/main.rs` is `test = false`
(`Cargo.toml:25`), so *the render half is unguarded today* — no test anywhere
asserts on a rendered line. Moving the gutter, the clamp, the span resolution and
the ink attachment into `model.rs` (§3.5) is not tidying; it is the only way the
clamp-plus-ANSI interaction gets covered by a test instead of by inspection. After
§3.6 the untested residue in `main.rs` is one `println!` of a composed string.

### 5.1 Tier 1 — new unit tests

**`crates/clave-types/src/lib.rs`:**

| Test | Asserts |
|---|---|
| `palette_is_twelve_distinct_entries` | length 12, all entries distinct, `PALETTE_LEN >= 2` (the title-skip precondition) |
| `palette_is_pinned` | the §2.4 table verbatim, by index. A deliberate palette change must edit this test |
| `ink_span_roundtrips_and_agent_inks_default_empty` | `InkSpan` serde round-trip; an `Agent` JSON with no `inks` key parses as empty (old-CLI/new-bar interop) |

**`crates/clave/src/store.rs`:**

| Test | Asserts |
|---|---|
| `first_sight_allocates_iterating_and_wrapping` | 13 repos → indices `0..=11` then `0` again. The maintainer's literal ask |
| `allocation_is_idempotent` | a second `allocate_inks` changes nothing and returns `false` |
| `same_repo_shares_one_ink` | three agents, one `repo_root` → one `repo_inks` entry, all three carry it |
| `title_inks_are_unique_within_a_repo` | five agents in one repo → five distinct title indices, none equal to the repo's |
| `title_cursor_starts_one_past_the_repo_ink` | a repo at index `k` gives its first agent index `k+1` |
| `title_inks_wrap_and_still_skip_the_repo_index` | twelve agents in one repo → eleven distinct values, the repo's index never appears, the twelfth reuses |
| `title_ink_survives_a_rename` | change `title`, re-run — the uuid's index is unchanged |
| `clear_tab_timeline_preserves_every_ink` | the regression guard for §2.2. Without it, one careless `.clear()` reshuffles every launch |
| `merge_resume_record_preserves_ink` | a resumed agent keeps its title index (inks live in `Store`, not `AgentRecord`) |
| `with_store_mut_leaves_no_unallocated_agent` | insert a bare record through any path; after the write every agent has both inks |
| `snapshot_ink_segments_match_compose_label_fields` | **the load-bearing cross-check**: for agents with and without a title, `InkSpan.segment` indexes the field of `compose_label()`'s output that actually holds that value. This is what keeps §3.2(d)'s mirrored ordering honest when S4 changes |

**`crates/clave-bar/src/model.rs`:**

| Test | Asserts |
|---|---|
| `rows_carry_inks_from_the_joined_agent` | a bound live tab gets the agent's resolved spans; a plain tab gets `vec![]` |
| `dormant_rows_carry_inks_too` | the dormant branch reads `a.inks` |
| `palette_index_resolves_and_wraps` | `ink(0)` is `Rgb(255,158,59)`; an out-of-range index wraps rather than panicking |
| `compose_row_tints_title_and_repo_at_the_right_fields` | `"F-CLA · clave · fix auth"` with spans `[(0,A),(1,B)]` → texts `["F-CLA", " · ", "clave", " · fix auth"]`, inks `[A, None, B, None]` |
| `compose_row_tints_repo_at_field_zero_when_untitled` | `"clave · fix auth"` with span `[(0,B)]` → the repo run is tinted, the summary is not |
| `compose_row_truncation_drops_spans_it_removed` | at a width that cuts before field 1, only field 0 is painted and nothing else changes |
| `compose_row_truncating_mid_separator_is_accepted` | pins the §2.7(2) cosmetic case |
| `compose_row_measures_the_gutter_it_is_given` | a 2-cell and a 4-cell gutter over the same row and `cols` → name budgets differ by exactly 2. The S6 contract |
| `compose_row_leaves_plain_tabs_untinted` | `inks: vec![]` ⇒ every name segment has `ink: None` |
| `compose_row_carries_reverse_per_segment` | active + tinted → `\x1b[7;38;2;255;158;59m` for the inked run, and the run after it is still inverted |
| `compose_row_strips_control_characters_before_clamping` | a name containing `\x1b[31m` and `\n` renders with neither, and width is computed on the stripped text |
| `compose_row_narrow_width_overflow_is_preexisting` | `budget == 0` → one `…` over budget, named in the test as pre-existing |
| `render_segments_matches_the_pre_s5_line_when_untinted` | the exact byte string the old renderer produced, active and inactive |
| `stripping_ink_reproduces_the_plain_line` | §2.9 — colour is decoration |
| `segment_span_indexes_arbitrary_fields` | `segment_span("a · b · c", 0/1/2/3)` → `Some(0..1)`, `Some(5..6)`, `Some(10..11)`, `None` (byte ranges; `LABEL_SEP` is 4 bytes) |

**Width parameterisation.** Every truncation test above runs over
`const TEST_WIDTHS: [usize; 2] = [30, 38]` — the current width and S8's target —
asserting the same structural property at both. No test hardcodes either number
inline, and the constant carries a comment naming S8.

### 5.2 Tier 1 — tests that must change, and tests that must not

**Verified against the tree, and the finding survives the revision.** `grep -n
"Row {" crates/clave-bar/src/model.rs` returns exactly **three** hits — the struct
definition at `:163` and the two construction sites at `:758` and `:783`, both
inside `rows()`. **No test constructs a `Row` literal**, so adding a field breaks
no test at compile time. The cited sites `model.rs:1229-1234`, `:1237`, `:1246`,
`:1249`, `:1305-1306`, `:2106` all read `Row.name`, which keeps its type and value.

| Site | Action |
|---|---|
| `model.rs:1147-1161` `fn agent(...)`, `:1163-1175` `fn agent_labelled(...)` | both set `repo_root: String::new()`; they now also default `inks: vec![]` ⇒ no tint in any pre-existing test. **Leave them** — the existing suite stays a control group. Add `fn agent_inked(uuid, spans, tab_id)` for the new tests |
| `:1229-1234`, `:1237`, `:1246`, `:1249` | unchanged — ordering is orthogonal and must stay visibly so |
| `:1305-1306` | extend with `assert!(a.inks.is_empty())` and `assert!(p.inks.is_empty())`, pinning the no-ink path rather than leaving it incidental |
| `:2106` | extend with `assert!(d.inks.is_empty())`; the row's label starts `"repo · "` while its `inks` are empty — pinning that proves the tint comes from the host's spans and **not** from parsing the label |
| `crates/clave/src/store.rs` tests (`:359+`), `add.rs:772+`, `hook.rs:303+`, `open.rs:144+`, `lsview.rs:34+`, `setup.rs:792+`, `dev.rs`, `crates/clave/tests/kdl_guardrail.rs:63` | mechanical: `AgentRecord`/`Agent` literals gain the new fields; `Store` literals gain four defaults, or use `..Default::default()` where the test already does |

### 5.3 Tier 1 — proptests (`model.rs mod proptests`, `:2803+`)

Generators: `name` from `"[\\PC ·…]{0,60}"` plus an escape-injecting strategy;
`cols in 0usize..=200`; `active in any::<bool>()`; `gutter` from a small set of
0-, 2- and 4-cell segment vectors; `inks` from
`prop::collection::vec((0usize..4, palette_ink()), 0..3)`.

| Property | Statement |
|---|---|
| **P1 — no escape is ever truncated, every sequence is reset** | scan `render_segments(&compose_row(&row, cols, &gutter))`: every `\x1b` begins a complete `\x1b[` … `m`, the parameter body matches `[0-9;]*`, and the count of non-`0` introducers equals the count of `\x1b[0m`. The line never ends inside a sequence. *Kept verbatim from the previous revision — this is the property the dossier's warning is about* |
| **P2 — visible text is colour-independent** | concatenating `Segment.text` yields the same string whatever `inks` contains. §2.9, mechanised. *Kept verbatim* |
| **P3 — width is respected** | for `cols >= gutter_cols + 2`, the escape-stripped line is at most `cols` characters. Gated because `budget == 0` is the pinned pre-existing overflow |
| **P4 — segment text is control-free** | for arbitrary names *including* injected `\x1b[31m` and `\n`, no `Segment.text` contains a control char |
| **P5 — spans never overlap and never reorder** | the emitted segments concatenate to the clamped name in order; each inked run corresponds to exactly one requested span; a span truncation removed is absent |
| **P6 — the gutter passes through verbatim** | the first `gutter.len()` output segments equal `gutter`, byte for byte. S6's contract |

**In `crates/clave/src/store.rs`** (plain loops, not proptest — `clave` has no
`proptest` dev-dep and these need no generator):

| Property | Statement |
|---|---|
| **P7 — allocation never collides within a repo** | over 200 randomly-ordered agent insertions across 30 repos: within any repo, no two agents share a title index until that repo's agent count exceeds `PALETTE_LEN - 1`, and no title index ever equals its repo's index |
| **P8 — allocation is order-stable under replay** | inserting the same agent set through many `with_store_mut` calls in the same order always yields the same ledger |

Ledger rationale for adding properties at all: `TESTING.md:121-126` — *"A new
branch without a new property is a new blind spot"*.

### 5.4 Tier 2

Does not exist (#47, blocked on #44). The **Cross-process / IPC** row therefore
buys written argument plus adversarial review (§4), not coverage. Named first
scenario for when #47 lands: two concurrent `clave add` runs against one scratch
store first-sighting the same repo, asserting a single `repo_inks` entry.

### 5.5 Tier 3

Everything in §6. The palette on the maintainer's real terminal, theme and font is
`host-untestable` by the taxonomy's own row.

### 5.6 The gate

```bash
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

`--workspace` is load-bearing: a bare `cargo test` skips `clave-bar` entirely
(`TESTING.md:36-42`), which is where every render test lives.

---

## 6. Live validation

**Contract** (`AGENTS.md:51-53`, `TESTING.md:188-204`). The maintainer runs every
step. The driving agent **prints** commands and never executes them against a live
session, never launches or kills a session, never runs `just release`,
`cargo install` or `just dev-install`. Paths are genericized (`$HOME/…`,
`$TMPDIR/…`) because the pre-commit PII blocklist rejects private local paths and
has already fired twice (`AGENTS.md:122-124`).

Vocabulary: **repo tint** = the colour of the repo field of a row's text;
**title tint** = the colour of the title field. Neither is the status glyph.

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
| they differ | **#44/#43** — the plugin is shelling out to a different binary than the one on `PATH` | **stop.** No observation below can be trusted. Report and abandon the run |
| no `clave-bar: loaded` line from today | the log is stale or the filter is wrong (the file is shared by every session on the machine, `TESTING.md:295-300`) | re-run with `tail -50`; if still nothing, report and stop |
| `build=` is not `dev` and you did not just hot-reload | you are looking at an instrumented sandbox wasm | note which session you are in and repeat in the intended one |

### Step 1 — does the palette read on YOUR terminal, in YOUR theme? (no clave involved)

**(a) Run** in any pane:
```bash
i=0
for c in "255;158;59"  "127;180;202" "228;104;118" "122;168;159" \
         "255;93;98"   "230;195;132" "126;156;216" "255;160;102" \
         "163;212;213" "210;126;153" "152;187;108" "184;180;208"; do
  printf '\033[38;2;%sm%2d  F-CLA · some-repo-name · a summary tail\033[0m\n' "$c" "$i"
  i=$((i+1))
done
printf '\033[7;38;2;255;158;59m F-CLA \033[0m\033[7m · clave · inverted row \033[0m\n'
printf 'plain default text for comparison — no entry above should resemble this\n'
echo "COLORTERM=$COLORTERM  TERM=$TERM"
```

**(b) Look at:** all twelve lines in your normal kanagawa dark theme, at your
normal font size. Then the thirteenth line — that is an *active* (selected) row:
reverse video with one field tinted. Then the fourteenth, untinted.

**(c) Report:** any index you cannot comfortably read; any pair you cannot tell
apart at a glance; whether any entry looks like plain untinted text; whether the
inverted line is acceptable; and the `COLORTERM`/`TERM` line.

| Report | Conclusion | Next |
|---|---|---|
| all twelve legible and mutually distinct; none resembles plain text | palette accepted | Step 2 |
| indices 1 / 3 / 8 (`springBlue` / `waveAqua2` / `lightBlue`) blur together | the known blue-aqua cluster (min pairwise ΔE 16.1) is too tight at your font size | report which pair; dropping `lightBlue` and `waveAqua2` gives a 10-entry palette at ΔE 19.1 — a one-line change plus the golden test. Re-run Step 1 |
| indices 0 / 7 (`roninYellow` / `surimiOrange`) blur | the known orange pair (ΔE 19.1) | same remedy: drop one, re-run |
| an entry looks like ordinary text | it is too close to `fujiWhite` on your actual background | report the index — the ΔE ≥ 20 floor is being violated in practice, which is a real finding |
| the colours look nothing like the table, and `COLORTERM` is empty | your terminal is not advertising truecolor | report both values. This is the case `Ink::Indexed` was retained for — the remedy is a nearest-cube fallback table |
| the inverted line reads badly | reverse-video + tint is the wrong composition | report; the fallback is to drop `inks` when `row.active` — one line in `compose_row` plus one test |

### Step 2 — two repos: distinct repo tints, shared within a repo

**(a) Do:** have agents from **two different repos** open simultaneously — e.g.
`$HOME/code/clave` and another repo of yours — with **at least two agents in one
of them**. Use `Alt+a` in each directory if needed.

**(b) Look at:** the repo field of every row.

**(c) Run and report:**
```bash
clave ls --json | jq -r '.agents[] | "\(.repo_root)\t\(.label)\t\(.inks)"'
jq '{repo_inks, repo_ink_cursor, title_inks, title_ink_cursors}' \
   "$HOME/.local/state/clave/agents.json"
```
plus, per row, the repo tint and title tint you actually see.

| Report | Conclusion | Next |
|---|---|---|
| the two repos differ; every row of one repo shares its repo tint | **the repo axis works** | Step 3 |
| two rows of the **same** `repo_root` differ | `repo_inks` is not being consulted — report the JSON immediately | **stop; report** |
| two rows with **different** `repo_root` share a tint | check `repo_ink_cursor`: ≥ 12 ⇒ the specified wrap; < 12 ⇒ a bug, report | **stop; report** |
| the sandbox (`clave-test`) shows different colours than the real session for the same repo *name* | **expected and by design** — allocation keys on the full `repo_root`, and the sandbox seeds repos under its own root (`dev.rs:232`) | note it and continue |
| no row is tinted | `inks` empty in the JSON ⇒ allocation never ran; `inks` populated ⇒ the bar is stale, re-check Step 0 | report which |
| plain terminal tabs are tinted | `inks` leaked to unjoined rows — report | **stop; report** |

### Step 3 — the title axis: same repo, different agents

**(a) Do:** in the repo with two or more agents, ensure each has a distinct Claude
session title (rename one with Claude's own rename, or let them earn different
titles).

**(b) Look at:** the **title** field of those same-repo rows, and compare each
title tint against that row's repo tint.

**(c) Report:** for each same-repo row, the title tint; and whether any title tint
equals its own row's repo tint.

| Report | Conclusion | Next |
|---|---|---|
| every same-repo agent has a different title tint, and none equals the repo tint | **the title axis works and the skip rule holds** — #24's "three `nalu` lookalikes" complaint closed | Step 4 |
| a title tint equals its row's repo tint | the skip rule failed — **report immediately** with the `title_inks`/`repo_inks` JSON; `title_inks_are_unique_within_a_repo` should have caught it | **stop; report** |
| two agents in the same repo share a title tint | check that repo's `title_ink_cursors` value: ≥ 12 ⇒ specified wrap; < 12 ⇒ bug, report | **stop; report** |
| agents in *different* repos share a title tint | expected — uniqueness is scoped per repo, as asked | continue |
| an **untitled** agent's tint sits on the wrong words | the segment index is wrong for the no-title case | **report immediately** — `snapshot_ink_segments_match_compose_label_fields` should have caught it |

### Step 4 — allocation is iteration: a new repo takes the next colour

**(a) Run** first, to record the cursor:
```bash
jq '{repo_ink_cursor, repo_inks}' "$HOME/.local/state/clave/agents.json"
```
Then, in a **non-zellij** terminal:
```bash
mkdir -p "$HOME/code/clave-ink-probe-one" "$HOME/code/clave-ink-probe-two"
git -C "$HOME/code/clave-ink-probe-one" init -q
git -C "$HOME/code/clave-ink-probe-two" init -q
```
Then `Alt+a` each in turn, `new`.

**(b) Look at:** the repo tint of each new row, against the palette you printed in
Step 1. They should be entries `cursor` and `cursor + 1` (mod 12) — the two
entries *after* whatever the cursor read.

**(c) Run and report:**
```bash
jq '{repo_ink_cursor, repo_inks}' "$HOME/.local/state/clave/agents.json"
```
plus the two observed colours and their Step-1 indices.

| Report | Conclusion | Next |
|---|---|---|
| the two new repos took the next two indices in order, and the cursor advanced by exactly 2 | **iteration confirmed** — the maintainer's stated model | Step 5 |
| the cursor advanced by more than 2 | something allocated a repo you did not open — inspect `repo_inks` keys for an unexpected path | report the keys |
| the colours are the next two indices but look similar to each other | the palette's adjacency ordering has failed | **report immediately** with both indices — `palette_is_pinned` should have caught a reordering |
| a new repo reused an existing colour while the cursor is < 12 | allocation is not monotone — report | **stop; report** |

**Cleanup**, your call afterwards:
`rm -rf "$HOME/code/clave-ink-probe-one" "$HOME/code/clave-ink-probe-two"`.

### Step 5 — force truncation, at both widths

**(a) Do**, in a **non-zellij** terminal:
```bash
mkdir -p "$HOME/code/clave-truncation-probe-repository"
git -C "$HOME/code/clave-truncation-probe-repository" init -q
```
(33-character basename — longer than the name budget at either 30 or ~38 columns.)
`Alt+a` it, `new`. Then press `Alt+c` to collapse the bar and `Alt+c` again to
expand.

**(b) Look at:** the new row at full width, and collapsed.

**(c) Report:** exactly what the row shows at each width (transcribe it, including
the trailing `…`), and whether **any** stray characters appear — a literal `[38`,
a `;2;`, a lone `[0m`, a stray `m` — or colour bleeding onto the row below.

| Report | Conclusion | Next |
|---|---|---|
| a tinted truncated name ending `…`, no stray characters, no bleed, at both widths | **the clamp/ANSI interaction is correct** — the §2.7 structural claim holds live | Step 6 |
| any literal escape text visible (`[38`, `;2;`, `[0m`) | an escape was truncated — the exact RC-G failure the design forbids | **report immediately with the transcription.** P1 should have caught it; the generator is wrong |
| colour continues onto the next row or the rest of the pane | a sequence was emitted without its reset | report immediately — `render_segments` is emitting an unpaired introducer |
| the row truncates **shorter** than an untinted row of the same length | escape bytes are being counted as visible width — ink applied before the clamp | report immediately; §3.5's order of operations was not followed |
| collapsed mode shows no text at all | **expected once S6 lands** — a 3-glyph gutter consumes the whole 4-column collapsed width, so there is no name left to tint. §7 records this as an S6/S8 decision, not an S5 bug | note it and continue |
| the bar snaps back to full width on its own | expected — the width seek re-targets its constant unless collapsed (`model.rs:1022-1026`); use `Alt+c`, not a manual resize | retry with `Alt+c` |

### Step 6 — colours survive a session restart, and a store round-trip

**(a) Do:** capture first, then restart. **You**, in a non-zellij terminal:
```bash
jq -S '{repo_inks, repo_ink_cursor, title_inks, title_ink_cursors}' \
   "$HOME/.local/state/clave/agents.json" > "$TMPDIR/clave-inks-before.json"
zellij kill-session clave
```
Relaunch the session the way you normally do, and reopen the same repos.

**(b) Look at:** every row's repo tint and title tint, against what you saw in
Steps 2–3.

**(c) Run and report:**
```bash
jq -S '{repo_inks, repo_ink_cursor, title_inks, title_ink_cursors}' \
   "$HOME/.local/state/clave/agents.json" > "$TMPDIR/clave-inks-after.json"
diff "$TMPDIR/clave-inks-before.json" "$TMPDIR/clave-inks-after.json" && echo IDENTICAL
```

| Report | Conclusion | Next |
|---|---|---|
| `IDENTICAL`, and every row kept both tints | **stability confirmed** across process death, plugin reload, store rehydration and `clear_tab_timeline` | Step 7 |
| the diff shows entries **added** (repos opened since the capture) but none changed | correct — the ledger is append-only | continue |
| an existing entry **changed or disappeared** | `clear_tab_timeline` or another path is clearing the ledger — **report immediately** with the diff. `clear_tab_timeline_preserves_every_ink` should have caught it | **stop; report** |
| the ledger is unchanged but a row's tint moved | the bar is resolving indices differently — report that row's `inks` from `clave ls --json` | **stop; report** |

### Step 7 — manual rename, and the honest limitation

**(a) Do:** manually rename one agent's **zellij tab** (not the Claude session) to
something with a different shape — e.g. two words with no ` · `.

**(b) Look at:** where the tint lands on that row.

**(c) Report:** what is tinted and whether it looks wrong.

| Report | Conclusion | Next |
|---|---|---|
| the whole (single-field) name takes the first span's tint, or nothing is tinted | **the specified degradation** — `Row.name` for a live row is the zellij tab name, not the store label (§2.1), so the host's field indices no longer describe it | note it; not a bug |
| stray escapes or bleed appear | a real defect regardless of the rename | report immediately |
| it looks bad enough to matter to you | worth an issue against #24 | report; the fix (rendering `Agent.label` for live rows) is a separate change with its own rename-loop implications |

### Step 8 — does it actually help? (the reason the feature exists)

**(a) Do:** open the sidebar with several rows across at least two repos, including
the #24 lookalike case (several agents of one repo on similar branches).

**(b) Look at:** whether you find the row you want faster, and whether the tints
compete with the status glyph.

**(c) Report:** a plain judgement, plus anything that reads worse than before.

| Report | Conclusion | Next |
|---|---|---|
| faster, and the glyph still reads first | **ship it**; #24 item 2 closed, item 3's lookalike complaint substantially addressed | merge per the autonomy contract |
| the tints pull attention off the status glyph | the channels compete | report; options are a dimmer palette variant or tinting only one axis. Do not merge on this reading without a decision |
| two axes is too much colour per row | over-decorated | report; dropping the title axis is a one-line revert in `snapshot_from`. Better to learn this now than after S6 adds three glyphs |
| no real improvement because truncation still eats the distinctive part | S5 alone is insufficient — #24 items 3/6 territory | report; S5 can still merge, #24 stays open on those |

---

## 7. Risks, dependencies and out of scope

### Dependencies and sequencing

| Workstream | Relationship |
|---|---|
| **S4** (label) | **hard dependency for the title axis only.** `snapshot_from` reads `r.title`, an S4 field. If S5 lands first, ship the repo axis alone (omit the title branch; `inks` has one entry) and land the title axis with S4 in a follow-up of ~10 lines. The repo axis has no S4 dependency at all |
| **S6** (gutter) | **soft, and S5 is built for it.** `compose_row` takes the gutter as `&[Segment]` and measures it; `gutter_segments` is the transitional stand-in S6 replaces. S6 must not change `compose_row`'s signature |
| **S8** (width 30 → ~38) | **none** — no width is hardcoded, and tests run at both (§5.1). S5 must not touch `BAR_TARGET_COLS` |
| **S0 / S1 / S3** | none — different files |

Shared-file overlap is confined to `crates/clave-types/src/lib.rs` (S4 adds schema
fields, S5 adds the palette and `InkSpan` — different regions, `LABEL_SEP` in
common) and `crates/clave/src/store.rs` (S4 adds `AgentRecord` fields, S5 adds
`Store` fields and `allocate_inks`).

#### Reconciling with S6's draft — three deltas, all in S6's favour

S6's spec (`2026-07-22-S6-gutter-glyphs.md:650-702`) was written against the
*previous* revision of this document and assumes a different seam. The
differences, and why the version here is the one to build:

1. **Signature.** S6 §2.9 proposes `compose_row(&Row, cols, collapsed: bool)` with
   the gutter built *inside* and `GUTTER_COLS` becoming `gutter_cols(cols, collapsed)`.
   This spec instead passes `gutter: &[Segment]` and measures it. That is strictly
   closer to S6's own stated goal — *"what keeps `compose_row` a pure geometry
   function with no knowledge of font tiers"* (`S6:269-270`) — because it also
   keeps `compose_row` free of the **collapse mode**, which is state, not geometry.
   S6 keeps everything it wanted: `gutter_segments(row, cols, collapsed)` becomes
   S6's function, producing the cells; `compose_row` never learns what they mean.
   The one line S6 must change from its draft is its own call site.
2. **`REPO_SEGMENT` no longer exists** (`S6:679` lists it among the constants it
   inherits). §2.1 deleted it: the title field is optional, so no global constant
   can be right. The equivalent information now arrives per row as
   `clave_types::InkSpan`, host-computed. S6 needs nothing in its place.
3. **Two test names S6 cites have changed.**
   `compose_row_at_collapsed_width` (`S6:634`) is gone — with a 4-cell gutter at a
   4-column collapsed width the budget is 0 and there is no text to tint, so the
   assertion it named is now `compose_row_narrow_width_overflow_is_preexisting`
   (`S6:742` already anticipates renaming that one). S6's
   `GUTTER_COLS_COLLAPSED == COLLAPSED_TARGET_COLS` const-assert (`S6:606-612`) is
   unaffected and is the right place for that invariant.

From S8: the text budget at the new width is `38 - 6 - 1 = 31` with S6's gutter,
and `38 - 2 - 1 = 35` before S6 lands. Neither number appears anywhere in S5 —
`compose_row` derives both — and S8 correctly identifies `main.rs:546` as *not*
its line (`S8:90`).

### Risks

| Risk | Severity | Mitigation |
|---|---|---|
| **The collapsed bar loses its text entirely** once S6's 3-glyph gutter meets the 4-column collapsed width — so #24 item 7's "glyph + repo colour at 4 cols" is *regressed*, not served | medium | Named here and in Step 5's branch table so it is not misread as an S5 defect. The decision (widen `COLLAPSED_TARGET_COLS`, or accept glyph-only collapse) belongs to S6/S8, and the C6 ledger governs it |
| A terminal without truecolor renders the palette approximately | medium | Step 1 catches it before merge. `Ink::Indexed` is retained so a nearest-cube fallback is data plus one line |
| The blue-aqua cluster (ΔE 16.1) is indistinguishable at the maintainer's font size | medium | Step 1 has an explicit branch and a costed remedy (drop to 10 entries at ΔE 19.1) |
| `clear_tab_timeline` — or a future session-hygiene function — clears the ink ledger, reshuffling every launch | **high if it happens** | `clear_tab_timeline_preserves_every_ink` is a dedicated regression test, the field doc comments say so in-place, and Step 6 verifies it live with a `diff` |
| Concurrent allocation under the flock | low | §4's ordering argument; adversarial review required by the Cross-process row; §5.3 P7/P8 |
| The ink ledger grows unbounded | low | One short string plus one byte per repo/agent ever seen. A `clave doctor` GC pass is future work |
| `snapshot_from`'s field ordering silently drifts from S4's `compose_label` | medium | `snapshot_ink_segments_match_compose_label_fields` is the cross-check; it fails the moment either side reorders |
| Two axes of colour is simply too much decoration | low | Step 8 asks directly, and removing the title axis is a one-line revert in `snapshot_from` |
| Issue #44 corrupts a live reading | high, standing | Step 0 is mandatory and terminal |

### Out of scope

- **v2 theme sourcing** (§2.6). The deliverable here is that its diff is a
  subscription, a `Styling → Vec<Ink>` function, and a change-gated assignment.
- **Worktree shades/variants** (#24 item 2, second clause). A worktree gets its
  parent's colour where `repo_root` says so, never a lighter one. Shading needs a
  second colour axis *and* a worktree signal in the snapshot — `worktree` is in
  the store record but dropped by `snapshot_from` (`store.rs:167-189`).
- **A per-repo colour override.** No user-config surface exists; adding one is a
  CLI plus artifact change with its own taxonomy row. The ledger makes a manual
  override trivial to *implement* later (edit `repo_inks` in the store), so this
  is a UI question, not an architectural one.
- **Colouring `clave ls`.** The palette is in `clave-types` so it stays cheap;
  `lsview.rs` is untouched here.
- **Releasing or garbage-collecting ink assignments.** §2.2.
- **Nerd Font portability (#40).** Separate: S5 adds **no glyph**. Every character
  it emits is already rendered today (`…`, label text) or is an ANSI escape, which
  is not a glyph. S6's three gutter glyphs *are* #40's business.
- **Widths and collapsed-state design** (#24 items 6/7, S8). S5 must not change
  `BAR_TARGET_COLS` or `COLLAPSED_TARGET_COLS`.
- **The `budget == 0` off-by-one** in the clamp. Pre-existing, pinned,
  deliberately not fixed inside this change.
