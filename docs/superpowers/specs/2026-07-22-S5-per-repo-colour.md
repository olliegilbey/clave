# S5 — per-repo and per-title colour, allocated by iteration (RC-G, closes #24 item 2)

> ## Status — reconciled 2026-07-28, and UNBLOCKED
>
> Two external rulings have been folded **into the body below**; there is no
> banner-versus-body precedence to keep in your head any more, and no footnote
> to reconcile. Read
> [`2026-07-25-sidebar-visual-design-lock.md`](2026-07-25-sidebar-visual-design-lock.md)
> first anyway — it owns the geometry this spec paints into — and run
> `cargo run -p clave-bar --example bar-preview`.
>
> 1. **Design lock (2026-07-25), §4 · §2 · §7.1 · §9 item 3.** The palette is
>    **8 kanagawa hues, not 12**; the title channel is a **filled chip**, not
>    tinted text; fields are **fixed-width columns** the bar lays itself
>    (title 7 · repo 7 · summary 17 inside a 44-column row); and **`InkSpan`,
>    `segment_span`, the optional-title index arithmetic and
>    `snapshot_ink_segments_match_compose_label_fields` are deleted** — nothing
>    parses a composed name any more. §2.1, §2.3, §2.4, §2.7, §3.1, §3.5.
> 2. **AgentSnapshot v2 (#69), landed 2026-07-28** —
>    [`2026-07-28-agentsnapshot-v2-design.md`](2026-07-28-agentsnapshot-v2-design.md).
>    `Agent` now carries `title`, `summary` and `worktree` structurally, and
>    `LABEL_SEP` exists in `clave-types`. S5 **consumes** all four; it proposes
>    none of them. The blocker design-lock §7.1 named is cleared.
>
> **What #69 deliberately left to S5, and why it matters here.** `repo_ink` and
> `title_ink` were the two fields #69 ruled *out* (its §2.2): they are `u8`, and
> **`u8` has no "unset"** — `0` is a real palette entry (crystalBlue). Landing
> them ahead of the ledger that assigns them would paint every row one colour.
> **S5 lands the fields and the ledger in the same change**, never the fields
> first. §3.1.
>
> **Unchanged and still binding:** hashing is overruled (`DefaultHasher` is not
> toolchain-stable, and the maintainer rejected hashing outright), so allocation
> is store-backed iterate-and-wrap — which keeps S5 on the cross-process/IPC row
> of the risk taxonomy, owing an ordering/idempotency argument and an
> adversarial reviewer.

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
| 10 indexed-256 cube colours, legible on light **and** dark | **kanagawa-derived truecolor entries, dark backgrounds only** (§2.4 — 12 in this revision, cut to the ratified 8 by the design lock) |
| palette order irrelevant (a hash spreads uniformly) | **palette order is load-bearing** — consecutive allocations must look maximally different (§2.4) |
| theme-sourcing rejected | **roadmap v2**, with the seam specified so it is a palette-source swap (§2.6) |
| `REPO_SEGMENT` global constant | **two per-agent palette indices, allocated host-side** — no constant to flip, and no field position to compute (§2.1) |
| budget `cols - 3` | **`cols - lead_in - trailing`**, the gutter supplied by S6 and measured, not assumed (§2.7) |

Kept unchanged: the colour-after-clamp structural seam, the
sanitize → clamp → split → ink order, the P1 escape-integrity and P2
colour-independence proptests, and the finding that no test constructs a `Row`
literal.

---

## 1. Problem and goal

Row text carries no colour today. The only two ANSI sites in the whole plugin are
the status glyph (`crates/clave-bar/src/main.rs:575`) and the active-row reverse
video (`main.rs:589`). With three `nalu · chore/pending-conf…` lookalikes stacked
in a narrow bar (#24, with screenshot on file), the eye has nothing but the first
few characters to separate rows — and the clamp eats exactly those.

**Goal.** Two colour axes, both stable, both allocated by iteration, and — since
the design lock — **rendered two different ways**, because they answer two
different questions:

1. **Repo ink** — one colour per `repo_root`, drawn as **tinted text** in the
   7-column repo field (and on the gutter's provenance glyph, design-lock §4.1).
   Every row of that repo shares it, everywhere and forever.
2. **Title ink** — one colour per agent, unique among the agents of *that* repo,
   drawn as a **filled chip**: the ink is the background, `sumiInk0` the text.

Together: the repo tells you *which project* at a glance, the chip tells you
*which of that project's tabs*. "Every tab visually identifiable in a
heartbeat."

**Non-goal.** Colour is never a load-bearing signal. Ordering, status, staleness
and identity all remain fully expressed without it, and the row text is rendered
character-for-character the same with colour as without — §2.9 makes that a test,
not a promise.

---

## 2. Design

### 2.1 Where the colour attaches — no field position survives, either

The previous revision had a `REPO_SEGMENT` constant to flip, deleted it because
**the title is optional** (with a title the repo is field 1, without one field
0 — no global constant is right for half the fleet at any moment), and replaced
it with a host-computed paint list: `InkSpan { segment, ink }`, read as *"colour
the `segment`-th ` · `-delimited field of this row's name"*.

**Design-lock §7.1 deletes that too, and for a better reason than the one above:
there is no composed name left to index into.** The bar lays **its own
fixed-width columns from values** — title 7, repo 7, summary 17 — and a live
row's values come from `agent_in_tab(t.tab_id)`, the join `rows()` already
performs on that same line to pick the status glyph. Locating a field by
splitting on ` · ` is the mechanism the ruling removed. So S5 loses `InkSpan`,
`segment_span`, the optional-title index arithmetic and
`snapshot_ink_segments_match_compose_label_fields` (design-lock §9 item 3), and
what crosses the wire is what a painter actually needs:

```rust
repo_ink:  u8,   // palette index for this agent's repo   → tinted TEXT
title_ink: u8,   // palette index for this agent's title  → filled CHIP
```

Two indices, no positions. The composer and the painter cannot disagree about
field order because neither one has a field order — §3.2's mirrored-ordering
hazard and its cross-check test are gone with the mechanism.

**Three consequences, all in S5's favour:**

1. **The values are already on the wire.** #69 landed `title: Option<String>`,
   `summary: String` and `worktree: Option<String>` on `clave_types::Agent`,
   `#[serde(default)]`, projected by `snapshot_from`. S5 **adds no string field
   and parses no label.**
2. **The "`Row.name` for a live row is the ZELLIJ TAB NAME" hazard is gone.**
   The previous revision accepted that a manual `zellij` rename could put the
   ink on the wrong words, because `model.rs` built a live row from
   `t.name.clone()`. §7.1 rules the sidebar renders clave's view of a session;
   the tab name is read **only** for a terminal tab, which has no agent record.
   The accepted cost is now the other one, stated by §7.1: *a manual `zellij`
   rename no longer appears in the sidebar at all.* §6 Step 7 observes that
   instead.
3. **S5 still declines S4's `fit_label(...) -> Vec<String>` hand-off** — S5
   owns the gutter/margin arithmetic a store-side composer cannot know, and
   `compose_row` must also handle a plain terminal tab, which has no label,
   no title and no repo.

**Sequencing, not owned here.** Rendering a live row from the store edits
`BarModel::rows`, which S0, S1, S3 and S6 also touch — design-lock §7.1's own
sequencing note calls it a multi-workstream collision zone. S5 needs the values;
it does not need to be the workstream that lands the switch. Whoever lands
first owns it, and the rest rebase.

### 2.2 Allocation: store-backed, on first sight, iterating and wrapping

The maintainer's objection to the hash is exact: *hashes could collide.* An
iterating allocator cannot collide until the palette is exhausted, and then it
collides in the order he described — "when it reaches the end, it goes back to the
beginning".

Iteration needs **memory**, and memory in clave has exactly one home. The doctrine
is already written down twice over (`store.rs:97-110`: `tab_timeline` and
`collapsed` both live in the store because per-instance copies *diverged live*).
Allocation state must therefore be **in the store, delivered in the snapshot, and
never computed in the bar** — there are N bar instances, one per tab
(`main.rs:20-22`), and any instance-local allocator would produce N different
palettes.

**Where allocation happens.** One function, `store::allocate_inks(&mut Store)`,
idempotent and monotone, called from two places:

| Call site | Role |
|---|---|
| `with_store_mut`, immediately after the caller's closure returns and before the write (`store.rs:166-167`) | **the universal backstop.** Every mutation path — `add.rs`, `dev.rs:233-247`, hooks, binds, prunes — passes through here, so no path can forget, and a store file written by an older clave is healed on its first write |
| inside `add.rs`'s creation closure (`add.rs:765-767`), after `s.agents.insert(...)` and before `snapshot_from(s)` | **the fast path.** The new agent is coloured in the very snapshot that announces it, so a freshly-opened tab is never briefly uncoloured |

Rejected sites: **`clave add` only** — misses `dev.rs` seeding and every legacy
row; **the hook** — hooks run per prompt, so a never-prompted agent stays
uncoloured, and the hook's untracked fast path deliberately skips the lock
(`store.rs:7-12`); **the bar** — ruled out above.

**Concurrency, under the flock.** `with_store_mut` holds an exclusive flock on a
separate, never-renamed lockfile across the whole read → mutate → write
(`store.rs:150-179`). Two `clave add` processes first-sighting the *same* repo
therefore serialize: the winner allocates and writes; the loser's `read_store`
already sees `repo_inks[repo]` populated and allocates nothing. Same index, no
race. Two processes first-sighting *different* repos get indices in flock-arrival
order — nondeterministic across runs, but inherent to "iterate in order of first
sight" and harmless: either order is equally correct.

**Seq is deliberately NOT bumped by `allocate_inks`.** The backstop runs after the
caller has already built its snapshot, so bumping there would either desync that
payload or manufacture a no-op push — and "no no-op pushes" is an explicit §5 rule
enforced at `store.rs:316`. The consequence is bounded
and named: an agent coloured only by the backstop **renders in `crystalBlue`,
palette index 0 — NOT untinted.** `u8` has no "unset": `unwrap_or(0)` is not a
blank, it is a real hue, which is exactly why the ink fields were kept off the
#69 wire until this ledger exists (v2 spec §2.2). Any assertion here must expect
crystalBlue, not absence. (Corrected on #82 — the earlier wording said
"untinted" and would have produced a test asserting the wrong thing.) The row
holds that colour until the next real push. In practice that is `dev.rs`-seeded rows and legacy store files, both
healed by `clear_tab_timeline` at launch (`setup.rs:708`) before any bar
instance exists.

**Surviving `clear_tab_timeline`.** That function is *session-recreate hygiene*: it
wipes `tab_timeline` and every `tab_id` because tab ids are session-scoped
(`store.rs:394+`). Ink allocation is **not** session-scoped — it is the thing
the maintainer must be able to learn — so the new maps live beside `agents`, not
beside `tab_timeline`, and `clear_tab_timeline` is **not modified**. A test pins
that it leaves every ink map untouched (§5.1); without that test the feature is one
careless `s.repo_inks.clear()` away from reshuffling on every launch.

**Release policy: colours are never released. Recommended, and here is why.**

- Agents are **never deleted** — verified: `grep -rn "agents.remove\|\.agents\.retain" crates/clave/src/` returns nothing. `apply_prune_tabs` clears binds, not rows. So the title ledger only grows with real agents and there is nothing to release.
- The only releasable thing would be a repo whose agents are all gone. Reclaiming it requires a refcount maintained across every write path and every concurrent writer — a new invariant of exactly the class that produced the prune race (#6/#26) and the recycled-tab-id race (`store.rs:298-300`). The ledger's lesson is that cross-process reclamation is where clave's bugs live.
- The user **learns** the mapping. Moving a colour he has learned is worse than reusing one: after release, closing repo A's last tab and opening repo B silently gives B the colour A had, and A gets a different one when it returns. That is the reshuffling the feature exists to prevent.
- Exhaustion is benign and is the maintainer's own stated model: the 13th repo shares with the 1st. Non-release reaches that point sooner; it never behaves *worse* than wrapping, which is the specified behaviour anyway.
- Cost: one short string plus one byte per repo ever seen, and one uuid plus one byte per agent. A heavy user accumulates tens of repos. If it ever matters, a `clave doctor` GC pass is future work (§7), not a v1 concern.

### 2.3 The two axes, and the within-row collision rule

**Repo ink.** Keyed on `repo_root` (the full path — it is the store's own grouping
key, `clave-types/src/lib.rs:55-56`). Allocated from a single global cursor:
`index = repo_ink_cursor % PALETTE_LEN`, cursor incremented.

This is a deliberate change from the previous revision, which keyed on the
*basename*. Iteration makes that argument moot: two checkouts of the same repo at
different paths were given one colour by basename-hashing because a hash has no
memory; with a ledger they are two entries and get two colours, which is *more*
informative and cannot be mistaken for a collision. The one place it shows is the
dev sandbox (`dev.rs:239` seeds repos under a sandbox root), where the sandbox and
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
refinement.** The title allocator skips the repo's own index. It survives the
design lock's split into two *renderings* (chip versus tinted text) because the
repo hue now appears **twice** on every row — design-lock §4.1 gives the gutter's
provenance glyph the repo ink as well — so a chip in that same hue would be the
third instance of one colour on one row. The refinement is where each repo's
title cursor *starts*:

```text
title cursor for repo R starts at (repo_index(R) + 1), not at 0
allocate: idx = cursor % LEN;  if idx == repo_index { cursor += 1; idx = cursor % LEN }
          cursor += 1
```

Two properties fall out, both free:

1. **`idx == repo_index` is impossible.** One skip always suffices because
   `PALETTE_LEN >= 2` (asserted, §5.1).
2. **The first agent of every repo is distinct from its own repo tint.**
   Starting at `repo_index + 1` puts the commonest case (one agent in a repo) at
   the largest distance the *ratified* order offers — **minimum adjacent ΔE
   30.7**, not the 55.2 quoted here before #82's review. **55.2 belonged to the
   rejected 12-entry palette**; the ratified 8-hue order was not
   adjacency-optimised (§2.4), so this mechanism buys separation but no longer a
   strong visual-spacing guarantee. Do not build anything on 55.2. It also
   staggers different repos' title cycles, so two repos' *first* agents get
   different title colours rather than both getting entry 0.

Effective title cycle length is `PALETTE_LEN - 1` = **7** per repo (8 hues, one
of them the repo's own).

Rejected: **disjoint sub-palettes** (repo draws from entries 0–3, titles from
4–7). Collision-free without a skip, but it halves both cycles — 4 repos before
a wrap — needs 16 distinct colours to restore the headroom the design lock just
cut to 8, and breaks the maintainer's model of one repeating set. Rejected:
**tint the title a lightness variant of the repo colour** —
attractive, but it makes the two axes *harder* to separate and re-opens the
worktree-shading design §7 defers.

### 2.4 The palette: 8 ratified kanagawa entries, truecolor, dark backgrounds only

**The set and its order are ratified, not derived here.** Design-lock §4 fixes
them from rendered rows, and `bar-preview.py:78-81` is the same eight in the same
sequence. **Twelve was rendered first and rejected** — *"they start colliding
after the 5th colour"* — which retires this section's previous argument that two
extra entries cost only 3 ΔE units: the cost was never ΔE, it was the maintainer
failing to tell them apart on his own screen. **Do not re-propose 12.**

The both-backgrounds contrast band from the earlier revision remains dropped — he
only cares about dark. The constraints the set is *checked* against, all measured
against kanagawa's wave background `#1F1F28` (this file has called it `sumiInk3`;
`bar-preview.py:59` calls it `sumiInk1` — same hex, and the preview is the one
that renders):

1. **contrast ≥ 5.0** against the bar background — comfortably past WCAG AA (4.5)
   for normal text, which is what "light coloured text" means operationally;
2. **ΔE (CIE76) ≥ 20 from `fujiWhite #DCD7BA`**, kanagawa's default foreground —
   a tinted word must not read as an untinted one;
3. **mutually distinguishable**;
4. **new, and only meaningful since the chip:** contrast ≥ 4.5 of `sumiInk0
   #16161D` **on** the hue. A repo tint only ever has to be legible *as* text;
   a title chip has to be legible *under* text.

Source of truth for the values: `rebelot/kanagawa.nvim`,
`lua/kanagawa/colors.lua` (fetched 2026-07-22), cross-checked hex-for-hex
against `bar-preview.py`.

| slot | kanagawa name | hex | rgb | contrast vs `#1F1F28` | ΔE to `fujiWhite` | chip: `#16161D` on hue | ΔE to next slot |
|---|---|---|---|---|---|---|---|
| 0 | `crystalBlue` | `#7E9CD8` | 126, 156, 216 | 5.94 | 53.9 | 6.54 | 76.6 |
| 1 | `springGreen` | `#98BB6C` | 152, 187, 108 | 7.52 | 33.7 | 8.28 | 30.7 |
| 2 | `carpYellow` | `#E6C384` | 230, 195, 132 | 9.73 | 23.1 | 10.72 | 53.8 |
| 3 | `waveRed` | `#E46876` | 228, 104, 118 | 5.09 | 58.6 | 5.61 | 52.0 |
| 4 | `oniViolet` | `#957FB8` | 149, 127, 184 | **4.67** | 55.7 | 5.15 | 46.6 |
| 5 | `waveAqua2` | `#7AA89F` | 122, 168, 159 | 6.17 | 29.1 | 6.80 | 65.9 |
| 6 | `surimiOrange` | `#FFA066` | 255, 160, 102 | 8.15 | 45.8 | 8.98 | 47.0 |
| 7 | `sakuraPink` | `#D27E99` | 210, 126, 153 | 5.59 | 47.9 | 6.16 | 45.6 (wraps to slot 0) |

Recomputed 2026-07-28 over the ratified set (WCAG 2.x contrast, CIE76 ΔE,
D65/sRGB). Three readings are worth stating plainly rather than leaving to a
future reader to rediscover:

- **`oniViolet` is 4.67, under this section's own ≥ 5.0 floor** — it clears WCAG
  AA (4.5) but not the stricter band, and it is in the set because the maintainer
  ratified the set by eye. `oniViolet` is also **not** one of the twelve this
  section previously listed (that was `oniViolet2 #B8B4D0`). Recorded, not
  "fixed": overriding a ratified visual decision on an arithmetic band is a
  design decision, and §6 Step 1 already asks the only question that settles it.
  Every other entry clears every band, chip included.
- **Minimum pairwise ΔE is 18.2** (`crystalBlue`/`oniViolet`), *better* than the
  12-entry set's 16.1 — the blue-aqua cluster that worried the previous revision
  went out with `springBlue` and `lightBlue`. Next weakest: `waveRed`/`sakuraPink`
  21.5, `carpYellow`/`surimiOrange` 27.8.
- **Palette order stays load-bearing** — with iteration, entry `k` and `k+1` are
  the colours the user's next two repos get, so adjacent entries must be
  maximally unlike, and a "tidy" rainbow reordering is the one thing this must
  not do. The ratified order's minimum adjacent ΔE is **30.7**
  (`springGreen`→`carpYellow`). The 12-entry order reached 55.2 because it was
  optimised for exactly that; the 8 were not, and **re-optimising the order of a
  ratified set is a design decision, not an amendment** — it would move colours
  the maintainer has already seen. Raise it with him or leave it.

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

Yes, and the reasoning changed twice. The glyph colours are *basic* SGR
(31/33/32/90 — `clave-types/src/lib.rs:35-43`; dormant `('◌', 90)` at
`model.rs:775`), which the user's theme remaps. The repo/title palette is
truecolor, which no theme touches. A kanagawa-red name field and a themed red
status dot sit adjacent but are never ambiguous — one is a `●`, the other is a
word.

What has changed since: the gutter is **no longer** ink-free. Design-lock §4.1
rules which cells the palette may enter, and it is a whitelist of one — the
**provenance** cell takes the repo ink, deliberately, so repo identity is a shape
in the gutter as well as a colour in the text. The **status** cell and the
**battery** cell (S7's magnitude ramp) are named as forbidden: their colour *is*
their signal, and overloading it deletes one. S5 therefore paints exactly two
things — the repo column and the title chip — and hands the repo ink to S6 for
the provenance cell rather than reaching into the gutter itself.

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
("`Styling` has no categorical ramp"). It supplies 10 and the ratified palette
is 8 — the shortfall this section previously had to paper over (12 wanted, 10
offered) **no longer exists**. A theme-sourced palette takes the first 8 and has
two in reserve.

**The seam, so v2 is a palette-source swap and nothing else.** Three rules, all
enforced in v1:

1. **The store persists INDICES, never colours.** `repo_inks`/`title_inks` hold
   `u8` palette positions, and `Agent.repo_ink`/`Agent.title_ink` carry those
   same positions on the wire. A theme change therefore reassigns nothing and
   needs no store migration. *This is the load-bearing rule* — had the store held
   RGB, v2 would be a data migration. #69 §6 states the same rule from the wire
   side: snapshot fields carry **identity, never resolved RGB**.
2. **Index → colour resolves in exactly one place**: `BarModel::ink(idx) -> Option<Ink>`,
   reading `BarModel.palette: Vec<Ink>`, itself initialised from
   `clave_types::PALETTE`. `rows()` calls it; nothing else does.
3. **v2 is then**: subscribe `EventType::ModeUpdate` (`main.rs:363-375`), and on
   the event overwrite `BarModel.palette` from `mode_info.style.colors`. Zero other
   lines. The storm risk that justified the previous rejection is handled by the
   discipline the rest of the plugin already uses: the handler returns `true`
   (repaint) **only when the derived palette actually changed**, so mode toggles
   that leave colours alone cost nothing — the change-gating pattern of
   `apply_bind`/`apply_collapse` (`store.rs:250+`, `:329+`).

v2 is explicitly **not** in this workstream's scope; the deliverable here is that
its diff is (1) a subscription, (2) a `Styling → Vec<Ink>` function, (3) a
change-gated assignment.

`zellij-tile`'s `Text` builder (`ui_components/text.rs`, `color_range`,
`DIM_LEVEL`) stays rejected for the reason the dossier records: semantic
index-levels resolved host-side, **no arbitrary-palette API**. "Colour #7 of my
eight" is unsayable in it, and neither is "this run on a `#957FB8` background",
which the chip needs.

### 2.7 The render restructure — colour after the clamp, by construction

Unchanged in principle. What changed with the design lock is *what gets clamped*:
not one composed name with fields located inside it, but **three independent
column values**, each clamped to its own fixed width.

Today's renderer (`crates/clave-bar/src/main.rs:573-592`) clamps by counting
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
rows()  →  Row { title, repo, summary (plain values), repo_ink, title_ink, … }
              │
              ▼   model.rs, pure, host-tested
        compose_row(&Row, cols, gutter: &[Segment]) -> Vec<Segment>
              │        sanitize → clamp EACH column → attach ink
              ▼
        render_segments(&[Segment]) -> String
              │        THE ONLY \x1b in crates/clave-bar
              ▼
        main.rs: println!("{}", …)
```

**The gutter is given, not built.** S6 owns the lead-in — design-lock §2 fixes it
at **9 columns** (cap · status · space · rule · space · battery · space ·
provenance · space), not the 3 this section originally assumed. `compose_row`
takes it as `&[Segment]`, measures it (`text.chars().count()` — exact, because
`Segment.text` is escape-free by construction), and derives

```rust
body = cols - gutter_cols - TRAILING_COLS      // 44 - 9 - 2 = 33
```

replacing the hardcoded `- 3`. `TRAILING_COLS` is 2: the right margin plus the
reserved right cap, which design-lock §2.2 reserves on **every** row so the
selected row's content does not shift. Until S6 lands,
`model::gutter_segments(&Row)` reproduces today's 2-cell gutter from `row.glyph`;
S6 replaces that one function and touches nothing else. S5 never inspects the
gutter's contents — it only measures them and hands S6 the repo ink for the
provenance cell.

**Columns, not spans.** Inside `body`, design-lock §2 fixes the split, and
`compose_row` lays it out: `title 7 · space · repo 7 · space · summary 17`.
Each column is clamped to *its own* width and padded to it, so a short value
never lets the next column slide left (§2.3 of the lock: alignment **is** the
separator). Five properties fall out, each a test rather than a comment:

1. **The clamp never sees an escape.** `compose_row` sanitizes (drops every
   `char::is_control()`) and *then* clamps, before any ink is attached.
   `render_segments` writes complete `\x1b[…m` … `\x1b[0m` pairs in one push — a
   partial sequence is unrepresentable.
2. **Truncation is per column and cannot move ink.** A long repo name is clamped
   to 7 and stays tinted; a long summary is clamped to 17. Nothing is ever
   located by searching for a separator, so the whole family of mis-point bugs
   the previous revision reasoned about — a clamp landing mid-separator, a span
   surviving into the wrong words — **cannot occur**. That is the design lock's
   own claim in §7.1, inherited here.
3. **The title is a chip, so ink is a background.** An empty title renders 7
   spaces with **no** background (design-lock §2: blank when never renamed) —
   not an empty chip, which would be a coloured rectangle asserting a rename
   that never happened.
4. **Reverse video composes.** The active row's attributes are carried per
   segment rather than wrapped around the whole line, so an inked segment's
   `\x1b[0m` cannot cancel the highlight on the segments after it.
5. **The gutter and budget arithmetic move into the tested half.**

Sanitizing also fixes a real latent bug for free: a store value containing a
newline (titles and summaries descend from Claude's transcript,
`hook.rs:120-135`) currently breaks the one-line-per-row contract that `click()`
depends on (`model.rs:800-803` indexes rows by rendered line).

**Degenerate widths.** The old `budget == 0` overflow — name non-empty, so
`take(0)` plus `'…'` emitted one character in a zero-cell budget — is
**preserved verbatim** for the single-value path and pinned by a test naming it
as pre-existing. The column path needs its own answer for `body < 17`, and it is
**not settled here**: design-lock §3 marks the collapsed geometry *not yet
ratified*, and §8.1 records that `COLLAPSED_TARGET_COLS = 4` is a width the bar
never actually has. S5's rule is only that `compose_row` must not panic and must
not emit a partial escape at any `cols` — proptest P3 covers that. **Which
columns survive a narrow row is S8's ruling to make**, and the lock's own
leaning is field-0-only (title, falling back to repo) rather than an ellipsis.

### 2.8 Widths: hardcode neither 30 nor 44

S8 widens the bar from 30 to **44** — design-lock §9 item 2 replaced the 38 this
section was written against; do not reintroduce it. Nothing in S5 may hardcode
either number: `compose_row` takes `cols`, and every truncation test runs over
`[30, 44]` (§5.1). S5 must not change `BAR_TARGET_COLS` or
`COLLAPSED_TARGET_COLS` (`model.rs:137,142`) — those are S8's, and the C6 ledger
governs them.

The one thing S5 *does* hardcode is the **column split inside `body`** (7 · 7 ·
17), because design-lock §2 ratified it as a table of columns, not as a ratio.
At `cols = 44` it fits exactly; below that the split is S8's question (§2.7,
"degenerate widths").

### 2.9 Accessibility: colour is decoration, and that is checkable

- Row **order** comes from the timeline, untouched (`model.rs:391-393,791`).
- Row **status** comes from the glyph, which keeps a distinct *character* as well
  as a colour (`● ✖ ◌`) — colour was never its only channel.
- Row **identity** comes from the column text, byte-identical with and without
  ink. The title chip is the one place to watch: it must not be the *only* thing
  distinguishing two rows, and it is not — the title text sits inside it.

Pinned: stripping all ink from `compose_row`'s output must leave the visible
characters unchanged. (The pre-S5 line is no longer the comparand — the design
lock replaced one composed label with three columns, so the *layout* changes for
everyone regardless of colour. What must not change with colour is the text.) A
monochrome terminal, a colour-blind reader and a screen-scraper all read the same
row. The honest caveat: the *added value*
of this feature is colour-only, so a colour-blind user gains nothing — but loses
nothing either, which is the requirement.

### 2.10 Rejected alternatives

| Rejected | Why |
|---|---|
| Hash → palette index | **overruled by the maintainer** — hashes collide. Also: a hash cannot honour "unique within this repo", which the title axis requires |
| Colour the whole row | he was explicit — *"I mean the text of that cwd name"* |
| Colour the status glyph instead | its colour already encodes status; overloading deletes a signal |
| Allocate in the bar | N instances, N allocators, N palettes — the divergence class the store doctrine exists to prevent (`store.rs:97-110`) |
| Persist RGB in the store | breaks the v2 theme seam: a theme change becomes a store migration (§2.6 rule 1) |
| Key title ink on the title string | Claude renames constantly and `/clear` clears it; the colour would flip on every rename |
| Release colours when a repo empties | §2.2 — cross-process refcounting, and it moves a colour the user has learned |
| Disjoint repo/title sub-palettes | halves both cycles and needs 16 distinct colours; breaks "one repeating set" |
| `REPO_SEGMENT` constant | §2.1 — wrong for every agent without a title, independent of S4 |
| `InkSpan` + `segment_span` (this spec's own previous answer) | design-lock §7.1 — the bar lays its own columns from **values**, so there is no composed name to index into. §2.1 |
| Parsing `title`/`summary` out of `label` in the bar | the same ruling, from the other end: #69 put both on the wire as fields (`clave-types/src/lib.rs:79-94`), so parsing is now *also* redundant |
| Tinting the whole title column instead of a chip | design-lock §4 — two channels answering two questions need two **renderings**, not one rendering twice |
| Twelve palette entries | design-lock §4 — rendered and rejected: *"they start colliding after the 5th colour"*. §2.4 |
| Embed ANSI in a `Row` string field | §2.7 — the clamp counts scalars; the RC-G defect |
| Indexed-256 | §2.4 — kanagawa has no cube equivalents, and v2 hands back RGB |
| `zellij-tile` `Text` + `color_range` | no arbitrary-palette API, and no background at all (RC-G) |

---

## 3. Implementation

Ordered so each step compiles. Red-first throughout.

### 3.1 `crates/clave-types/src/lib.rs` — the palette and two wire fields

Append after `impl Status` (currently ends `:44`):

```rust
/// The eight colours the bar allocates to repos and to title chips
/// (#24 item 2). RATIFIED from rendered rows — design-lock §4, mirrored in
/// `docs/superpowers/specs/bar-preview.py`. Twelve was rendered first and
/// rejected ("they start colliding after the 5th colour"); do not re-propose
/// it, and do not reorder (see below). Values from kanagawa
/// (`rebelot/kanagawa.nvim`, `lua/kanagawa/colors.lua`), the maintainer's
/// theme, checked against:
///   * contrast >= 5.0 on the #1F1F28 bar background — `oniViolet` is 4.67,
///     under the band but past WCAG AA, and is in the set because the set was
///     ratified by eye (spec §2.4);
///   * CIE76 dE >= 20 from `fujiWhite` #DCD7BA, so a tinted word never reads
///     as an untinted one;
///   * mutually distinguishable — minimum pairwise dE 18.2;
///   * legible UNDER text: `sumiInk0` #16161D on any entry clears 5.1, which
///     is what a title chip needs and a tint does not.
///
/// ORDER IS LOAD-BEARING. Allocation ITERATES this array, so entry k and
/// k+1 are the colours the user's next two repos receive; a "tidy" rainbow
/// reordering would make consecutive allocations look alike, which is the one
/// thing this must not do. Minimum adjacent dE in this order is 30.7.
///
/// Truecolor, not indexed-256: kanagawa's values have no exact cube
/// equivalent (`crystalBlue` #7E9CD8 quantises to #87afd7, a visibly
/// different blue), and the v2 theme path hands back RGB.
pub const PALETTE: [(u8, u8, u8); 8] = [
    (126, 156, 216), // #7E9CD8 crystalBlue
    (152, 187, 108), // #98BB6C springGreen
    (230, 195, 132), // #E6C384 carpYellow
    (228, 104, 118), // #E46876 waveRed
    (149, 127, 184), // #957FB8 oniViolet
    (122, 168, 159), // #7AA89F waveAqua2
    (255, 160, 102), // #FFA066 surimiOrange
    (210, 126, 153), // #D27E99 sakuraPink
];

/// Must stay >= 2: the title allocator skips the repo's own index and relies
/// on ONE skip always finding a different entry (§2.3).
pub const PALETTE_LEN: usize = PALETTE.len();

/// Text ON a title chip — kanagawa `sumiInk0`. The chip's ink is its
/// BACKGROUND (design-lock §4), so this is the only foreground it ever takes.
pub const CHIP_TEXT: (u8, u8, u8) = (22, 22, 29); // #16161D
```

`LABEL_SEP` is **not** proposed here. It landed with #69
(`clave-types/src/lib.rs:146`, `" \u{00b7} "`); this section's previous copy and
its "if S4 lands it first, delete this one" note are both resolved — import it.
Nothing in S5 splits on it any more either (§2.1), so S5's only interest is that
it exists and is spelled once. (Accuracy, because it invites a wrong assumption:
the five label *composition* sites in `add.rs` and `hook.rs` still spell the
separator literally. Unifying them was deliberately out of #69's scope — a
label-composition change is a render change — and it is not S5's either.)

Extend `Agent` (`:50-102`) with **two** fields, and land them in the same change
as §3.2's ledger:

```rust
    /// Palette index for this agent's repo — the tinted repo column, and the
    /// gutter's provenance glyph (design-lock §4.1). An INDEX, never a
    /// colour: the store persists indices so swapping the palette (v2 theme
    /// sourcing) reassigns nothing.
    #[serde(default)]
    pub repo_ink: u8,
    /// Palette index for this agent's title chip. Keyed on the agent UUID,
    /// not the title text, so a rename never moves it (§2.3).
    #[serde(default)]
    pub title_ink: u8,
```

**These two fields may not land before the ledger that fills them, and #69 held
them back for exactly this reason** (its §2.2, and design-lock §7.1 for the shape
they replaced):

> a field whose name promises what its values cannot yet deliver.

`u8` **has no "unset"**. `#[serde(default)]` is `0`, and `0` is a real palette
entry — `crystalBlue`. Ship the fields ahead of §3.2's allocation and every row
in the fleet paints the same blue, with nothing in the type system to say the
value is a placeholder rather than an assignment. So S5 lands `repo_ink`,
`title_ink`, `repo_inks`, `title_inks` and the cursors **as one change**.

The residue this leaves, stated: `#[serde(default)]` still means index 0 under
the one skew #69 §2.1 names — an old CLI writing to a new bar, which the dev
sandbox reaches routinely because it **hot-reloads the bar without relaunching**.
Those rows read crystalBlue until the CLI is rebuilt. Bounded, cosmetic, and it
resolves itself on the next `just dev-install`. (`Option<u8>` would make
"unallocated" representable and delete even that. Not taken here: #69 §2.2 names
the replacement as two palette *indices*, every agent is allocated by §3.2's
backstop, and a wider wire type to describe a state that only a stale binary can
produce is the sort of hedge `AGENTS.md` calls overengineering. If the sandbox
skew ever confuses a live reading, this is the one-line escape hatch.)

### 3.2 `crates/clave/src/store.rs` — the allocation ledger

All line numbers in this section are post-#69 (`store.rs` grew by ~25 lines when
`title`/`summary` landed); re-grep before trusting any of them.

**(a) Extend `Store`** (`:90+`), beside `agents` and deliberately **not** beside
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
/// payload or manufacture a no-op push (§5 forbids those — store.rs:316).
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

**(c) Wire the backstop into `with_store_mut`.** Replace `store.rs:166-167`:

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

**(d) Extend `snapshot_from`** (`:185-212`) with **two lines**. The paint-list
construction the previous revision specified here is gone with `InkSpan` (§2.1),
and with it the whole mirrored-field-order hazard: there is no ordering to keep
in step with `compose_label`, so no cross-check test either. Add to the existing
`.map(|r| Agent { … })` literal, which #69 already left projecting
`title`/`summary`/`worktree`:

```rust
                // Ledger lookups, not derivations. `unwrap_or(0)` is reachable
                // only for a record the backstop has not yet seen — every write
                // path runs `allocate_inks` under the same flock (§2.2).
                repo_ink: store.repo_inks.get(&r.repo_root).copied().unwrap_or(0),
                title_ink: store.title_inks.get(&r.uuid).copied().unwrap_or(0),
```

Note what is **not** here. `snapshot_from` does not consult `r.title` to decide
anything: `title_ink` is allocated per uuid whether or not the session has ever
been renamed (§2.3), and the *rendering* decision — a chip, or seven blank
columns — belongs to the bar, which can see `title.is_none()` for itself.

**(e) `clear_tab_timeline` (`:394+`) is NOT modified.** §5.1 pins that.
Neither is `backfill_summaries` (`:374`, #69's one-shot label split): it seeds a
*string*, runs in the same locked RMW, and has nothing to say about inks.

### 3.3 `crates/clave/src/add.rs` — the fast path

In the creation closure (`add.rs:765-767`), between the insert and the snapshot:

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

`merge_resume_record` (`:350+`) needs no change: inks live in `Store`, not in
`AgentRecord`, so a resumed agent keeps its colour automatically. §5.1 pins it.

### 3.4 `crates/clave-bar/src/model.rs` — `Row`, the palette, `rows()`

Import (`model.rs:12`):

```rust
use clave_types::{Agent, AgentSnapshot, CHIP_TEXT, PALETTE, Status};
```

`LABEL_SEP` is deliberately **not** imported: nothing in the bar splits on it any
more (§2.1).

**(a) Replace `Row`** (`:161-169`). It stops carrying one composed `name` and
starts carrying the three column values design-lock §2 lays out — this is the
`rows()` restructure §2.1's sequencing note flags as shared with S0/S1/S3/S6:

```rust
/// One rendered row, already in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: RowKey,
    /// Column values, PLAIN — never an escape. `compose_row` is the only
    /// consumer allowed to add one (RC-G: the width clamp counts Unicode
    /// scalars, so ANSI in here truncates mid-escape). A live row takes all
    /// three from its joined agent, NOT from the zellij tab name
    /// (design-lock §7.1).
    ///
    /// `title: None` renders seven blank columns, not an empty chip.
    pub title: Option<String>,
    pub repo: String,
    pub summary: String,
    /// A plain terminal tab has no agent record: it renders the zellij tab
    /// name across the body and takes no ink at all (§7.1, §7.2 — a real
    /// identity for terminal tabs is deferred).
    pub terminal_name: Option<String>,
    pub active: bool,
    /// (glyph, ANSI colour) for agent rows; None for plain terminal tabs.
    /// S6 takes ownership of the gutter; until then `gutter_segments` reads
    /// this.
    pub glyph: Option<(char, u8)>,
    /// Resolved colours. `None` for a terminal tab. Indices come from the
    /// HOST, colours from `BarModel::ink` — the single index→colour
    /// resolution point that makes v2 theme sourcing a palette swap (§2.6).
    /// `repo_ink` also paints the gutter's provenance cell (design-lock §4.1),
    /// which is S6's to read.
    pub repo_ink: Option<Ink>,
    pub title_ink: Option<Ink>,
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
read three times — glyph, values, inks):

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
            entries.push((
                self.sort_key(t),
                t.position,
                Row {
                    key: RowKey::Tab(t.tab_id),
                    // design-lock §7.1: a live row renders from the STORE.
                    // The zellij tab name is read ONLY when there is no agent
                    // — a plain terminal tab.
                    title: joined.and_then(|a| a.title.clone()),
                    repo: joined.map(|a| repo_name(&a.repo_root)).unwrap_or_default(),
                    summary: joined.map(|a| a.summary.clone()).unwrap_or_default(),
                    terminal_name: joined.is_none().then(|| t.name.clone()),
                    // A dormant selection steals the highlight from every tab.
                    active: selected_dormant.is_none() && t.active,
                    glyph,
                    repo_ink: joined.and_then(|a| self.ink(a.repo_ink)),
                    title_ink: joined.and_then(|a| self.ink(a.title_ink)),
                },
            ));
        }
```

`repo_name` is the 7-column display form of `repo_root`, which is a full path
(`clave-types/src/lib.rs:55-56`) — the basename, and nothing more clever. The
**allocation** key stays the full path (§2.3); only the *rendering* shortens it,
so two checkouts of the same repo still read alike and still colour differently.

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
                    // A dormant row ALWAYS rendered from the store — it has no
                    // tab. It now reads the same fields the live branch does
                    // instead of the composed label.
                    title: a.title.clone(),
                    repo: repo_name(&a.repo_root),
                    summary: a.summary.clone(),
                    terminal_name: None,
                    active: selected_dormant == Some(a.uuid.as_str()),
                    glyph: Some(glyph),
                    repo_ink: self.ink(a.repo_ink),
                    title_ink: self.ink(a.title_ink),
                },
```

A dormant row's `summary` is the field #69's one-shot backfill exists for
(its §2.3): rows written before the field existed carry their summary only
inside `label`, and **dormant rows receive no hook events**, so nothing else
would ever fill them. Without that backfill this branch would render a blank
17-column field for every pre-#69 row.

### 3.5 `crates/clave-bar/src/model.rs` — the composition seam (new)

New section after `Row`. This is the code `main.rs` currently owns and that no
test can reach.

```rust
/// Right margin plus the reserved right cap. The cap column is blank on an
/// UNSELECTED row rather than absent — design-lock §2.2, so the selected
/// row's content never sits one column right of its neighbours. The GUTTER
/// is not a constant: S6 owns it and passes it in, so its width is MEASURED,
/// not assumed (§2.7).
const TRAILING_COLS: usize = 2;

/// design-lock §2: the body splits into three fixed-width columns separated
/// by one space each. 7 + 1 + 7 + 1 + 17 = 33 = 44 - 9 - 2.
const TITLE_COLS: usize = 7;
const REPO_COLS: usize = 7;
const SUMMARY_COLS: usize = 17;

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
    /// Foreground. The repo column's tint; `CHIP_TEXT` on a title chip.
    pub ink: Option<Ink>,
    /// Background — the TITLE CHIP, and nothing else in S5. The design lock
    /// makes the two colour channels two different renderings on purpose
    /// (§4): a repo answers *which project*, a chip answers *which of my
    /// tabs*, and one channel rendered twice answers neither.
    pub bg: Option<Ink>,
    /// SGR 7 reverse video — the active-row highlight. Carried PER SEGMENT,
    /// so an inked segment's reset can't cancel the highlight after it.
    pub reverse: bool,
}
```

`segment_span` is **deleted** (design-lock §9 item 3) — with it the last code in
`clave-bar` that knew a label had fields inside it.

```rust
/// One fixed-width column: sanitize, truncate long with a trailing `…`, pad
/// short with spaces. Padding is not cosmetic — it IS the separator
/// (design-lock §2.3), and it is why a short summary cannot let the next row's
/// column slide.
///
/// Sanitizing first is load-bearing twice: an escape in a value would
/// otherwise be counted as visible width AND could be cut mid-sequence, and a
/// newline would break the one-line-per-row contract `click()` indexes on
/// (`model.rs:800-803`). Titles and summaries descend from Claude's transcript
/// (`hook.rs:120-135`) — not trusted text.
///
/// KNOWN GAP, inherited not introduced: this counts Unicode SCALARS, as the
/// current renderer does, while `bar-preview.py:123-135` measures display
/// CELLS. They disagree on CJK and on emoji in a summary, which would shift
/// every column to its right. Design-lock §2.1 already owes a gutter-invariance
/// test for the same hazard on the glyph side; a switch to cell measurement is
/// one shared decision, not an S5 one.
fn fit(value: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    // NOT `filter(|c| !c.is_control())` — that drops the `\x1b` and leaves the
    // PAYLOAD: `\x1b[31mfoo` becomes a visible `[31mfoo`, which is both wrong
    // on screen and four cells wider than the caller budgeted, breaking the
    // locked column arithmetic. Strip the whole CSI sequence — `\x1b`, the
    // `[`, the parameter/intermediate bytes, and the final byte in `@`..`~` —
    // then drop any remaining control chars. Pin it with a test asserting no
    // residual `[31m` survives. (#82 review.)
    let clean: String = strip_ansi(value);
    let n = clean.chars().count();
    if n <= w {
        return clean + &" ".repeat(w - n);
    }
    let mut out: String = clean.chars().take(w - 1).collect();
    out.push('…');
    out
}

/// Today's 2-cell gutter, rebuilt as segments. **S6 replaces this function
/// and nothing else** — `compose_row` already treats the gutter as opaque,
/// and S6's version is 9 cells wide (design-lock §2), takes `row.repo_ink`
/// for the provenance cell, and renders a SPACE wherever a glyph is absent so
/// a missing glyph can never reflow the row (§2.1 of the lock).
pub fn gutter_segments(row: &Row) -> Vec<Segment> {
    let plain = |text: &str| Segment {
        text: text.to_string(), ink: None, bg: None, reverse: false,
    };
    match row.glyph {
        Some((glyph, colour)) => vec![
            Segment {
                text: glyph.to_string(),
                ink: Some(Ink::Sgr(colour)),
                bg: None,
                reverse: false,
            },
            plain(" "),
        ],
        // Plain tabs get a 2-space gutter so text aligns.
        None => vec![plain("  ")],
    }
}

/// The whole rendered line for one row: the caller's gutter, then the body
/// laid out as fixed-width columns and painted.
///
/// Order of operations, and it is the design: sanitize → fit EACH column →
/// attach ink. Ink is attached LAST and only to whole segments, so no escape
/// can be truncated and no line can end mid-colour.
///
/// `cols` is a parameter and no width is hardcoded: the bar is 30 today and
/// 44 after S8 (§2.8). What IS fixed is the split inside the body, because
/// design-lock §2 ratified it as columns rather than as a ratio.
pub fn compose_row(row: &Row, cols: usize, gutter: &[Segment]) -> Vec<Segment> {
    let mut out: Vec<Segment> = gutter.to_vec();
    // Exact, because Segment.text is escape-free by construction.
    let gutter_cols: usize = gutter.iter().map(|s| s.text.chars().count()).sum();
    let body = cols.saturating_sub(gutter_cols + TRAILING_COLS);
    let active = row.active;
    let mut push = |text: String, ink: Option<Ink>, bg: Option<Ink>| {
        if !text.is_empty() {
            out.push(Segment { text, ink, bg, reverse: active });
        }
    };

    // A terminal tab has no columns to lay out: one value, the whole body.
    // Its identity is a placeholder pending design-lock §7.2.
    if let Some(name) = &row.terminal_name {
        push(fit(name, body), None, None);
        // The reserved tail is part of the ROW, not of `body` — see below.
        push(" ".repeat(TRAILING_COLS), None, None);
        return out;
    }

    // Narrow rows: design-lock §3 has not ratified the collapsed geometry, so
    // the ONLY contract here is "never panic, never emit a partial escape".
    // See §2.7, "degenerate widths" — the ruling is S8's.
    let summary_w = body.saturating_sub(TITLE_COLS + REPO_COLS + 2);

    match &row.title {
        // The chip: the ink is the BACKGROUND, sumiInk0 the text.
        Some(t) if !t.is_empty() => push(
            fit(t, TITLE_COLS),
            Some(Ink::Rgb(CHIP_TEXT.0, CHIP_TEXT.1, CHIP_TEXT.2)),
            row.title_ink,
        ),
        // Never renamed → blank columns, NOT an empty chip. A coloured
        // rectangle would assert a rename that never happened.
        _ => push(" ".repeat(TITLE_COLS), None, None),
    }
    push(" ".to_string(), None, None);
    push(fit(&row.repo, REPO_COLS), row.repo_ink, None);
    push(" ".to_string(), None, None);
    push(fit(&row.summary, summary_w), None, None);
    // `body` is 33 = 44 - 9 gutter - 2 TRAILING_COLS, so the three fields and
    // their separators exhaust it. The reserved tail must still be EMITTED or
    // the row is 42 cells, not 44: `compose_row_lays_the_locked_columns` fails,
    // and — the reason it is reserved at all — the selected row's background
    // stops short of the right margin and cap, leaving the ragged selection
    // design-lock §6 exists to kill.
    push(" ".repeat(TRAILING_COLS), None, None);
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
        match s.bg {
            Some(Ink::Sgr(c)) => params.push((c + 10).to_string()),
            Some(Ink::Indexed(n)) => params.push(format!("48;5;{n}")),
            Some(Ink::Rgb(r, g, b)) => params.push(format!("48;2;{r};{g};{b}")),
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

**Not specified here, and deliberately.** Design-lock §6 rules the selected row
by *recession* — waveBlue2 `#2D4F67` behind it, every other row faded 25% toward
the bar background — which is a **per-row background and a colour transform**, not
a per-segment attribute, and `bar-preview.py:179` applies that fade to the chip
too. S5 leaves `reverse` in place as today's highlight and hands the design lock's
treatment to whoever lands §6: the seam it owes is that `Segment` already carries
a background, so the fade is a transform over segments rather than a rewrite of
one. **The chip must fade with its row** — an unfaded chip on a receded row would
make every unselected row shout.

### 3.6 `crates/clave-bar/src/main.rs` — the adapter shrinks

Replace `main.rs:570-592` in full:

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
        // fixed-width columns, repo tint, title chip, active-row
        // inversion) lives in model.rs
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
- `clear_tab_timeline`, `backfill_summaries`, `merge_resume_record`,
  `BAR_TARGET_COLS`, `COLLAPSED_TARGET_COLS`.
- **`Agent.label` stays, and stays composed.** The bar stops *rendering* it, but
  `Effect::RenameTab` still writes it onto the real zellij tab — that is what
  zellij's own tab bar shows — and the rename loop-guard still fires on label
  change only, so `rename_only_when_label_changes_not_when_tab_name_differs`
  remains valid and must not be deleted (design-lock §7.1). Nor do the five
  label-composition sites in `add.rs`/`hook.rs` change: composing a label is a
  render change, out of scope for #69 and for S5 alike.

---

## 4. Risk class

Two rows of the taxonomy (`docs/dev/TESTING.md:112-120`), and the second is **new
to this revision** — store-backed allocation moved S5 across the process seam:

| Class | Why it applies | What it demands |
|---|---|---|
| **Pure logic / model** | `rows()`, `compose_row`, `render_segments`, `allocate_inks` | TDD red-first; `cargo test --workspace`; extend proptests for newly-reachable branches |
| **Cross-process / IPC** | `allocate_inks` is a **multi-writer store path** — every `clave` process reaches it through `with_store_mut` | written ordering/idempotency argument in the PR dossier; an adversarial reviewer must attack it; tier-2 coverage once #47 lands |
| **Visual / UX** | the palette **and the chip**, which is a stronger signal than a tint and interacts with design-lock §6's fade | human judgement only. The palette itself is already ratified (design-lock §4); what §6 asks is whether it works *in a row* |

Labels: `needs-live-validation` **and** `host-untestable`.

**The ordering/idempotency argument, for the dossier** (§2.2 in short form):
`allocate_inks` runs only inside `with_store_mut`, which holds an exclusive flock
on a separate never-renamed lockfile across the whole read → mutate → write
(`store.rs:150-179`). It is idempotent — an agent with an entry is skipped — and
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
asserts on a rendered line. Moving the gutter, the per-column fit and
the ink attachment into `model.rs` (§3.5) is not tidying; it is the only way the
clamp-plus-ANSI interaction gets covered by a test instead of by inspection. After
§3.6 the untested residue in `main.rs` is one `println!` of a composed string.

### 5.1 Tier 1 — new unit tests

**`crates/clave-types/src/lib.rs`:**

| Test | Asserts |
|---|---|
| `palette_is_eight_distinct_entries` | length 8, all entries distinct, `PALETTE_LEN >= 2` (the title-skip precondition) |
| `palette_is_pinned` | the §2.4 table verbatim, by index — **and the set is ratified** (design-lock §4), so a failure here means someone re-proposed 12 or "tidied" the order, not that the test is stale |
| `agent_ink_fields_roundtrip_and_default_to_zero` | `repo_ink`/`title_ink` serde round-trip; an `Agent` JSON with neither key parses as `0`. The test exists to make the §3.1 hazard *visible* — index 0 is crystalBlue, not "unset" |

**`crates/clave/src/store.rs`:**

| Test | Asserts |
|---|---|
| `first_sight_allocates_iterating_and_wrapping` | 9 repos → indices `0..=7` then `0` again. The maintainer's literal ask |
| `allocation_is_idempotent` | a second `allocate_inks` changes nothing and returns `false` |
| `same_repo_shares_one_ink` | three agents, one `repo_root` → one `repo_inks` entry, all three carry it |
| `title_inks_are_unique_within_a_repo` | five agents in one repo → five distinct title indices, none equal to the repo's |
| `title_cursor_starts_one_past_the_repo_ink` | a repo at index `k` gives its first agent index `k+1` |
| `title_inks_wrap_and_still_skip_the_repo_index` | eight agents in one repo → seven distinct values, the repo's index never appears, the eighth reuses |
| `title_ink_survives_a_rename` | change `title`, re-run — the uuid's index is unchanged. It is keyed on the uuid, and #69 made `title` a real field, so this is now a one-line mutation rather than a label rewrite |
| `title_ink_is_allocated_for_an_untitled_agent` | `title: None` still gets an index — the chip's *absence* is a render decision, not a missing allocation (§3.2 d) |
| `clear_tab_timeline_preserves_every_ink` | the regression guard for §2.2. Without it, one careless `.clear()` reshuffles every launch |
| `merge_resume_record_preserves_ink` | a resumed agent keeps its title index (inks live in `Store`, not `AgentRecord`) |
| `with_store_mut_leaves_no_unallocated_agent` | insert a bare record through any path; after the write every agent has both inks |
| `snapshot_projects_both_ink_indices` | `snapshot_from` carries the ledger's values through unchanged. Replaces `snapshot_ink_segments_match_compose_label_fields`, **deleted** with `InkSpan` (design-lock §9 item 3): there is no field ordering left to cross-check |

**`crates/clave-bar/src/model.rs`:**

| Test | Asserts |
|---|---|
| `rows_read_values_from_the_joined_agent_not_the_tab_name` | design-lock §7.1: a live row bound to an agent whose `title`/`summary` differ from the zellij tab name renders the **agent's** values. The one test that pins the ruling |
| `rows_carry_inks_from_the_joined_agent` | a bound live tab resolves both indices; a plain terminal tab gets `None`/`None` and its tab name in `terminal_name` |
| `dormant_rows_carry_values_and_inks_too` | the dormant branch reads `a.title`/`a.summary`/`a.repo_ink`/`a.title_ink` |
| `palette_index_resolves_and_wraps` | `ink(0)` is `Rgb(126,156,216)` (crystalBlue); an out-of-range index wraps rather than panicking |
| `compose_row_lays_the_locked_columns` | at `cols = 44` with a 9-cell gutter: title 7, space, repo 7, space, summary 17, and the escape-stripped line is exactly 44 |
| `compose_row_pads_short_values_so_columns_align` | two rows with different-length repos put the summary at the same column. Alignment **is** the separator (design-lock §2.3) |
| `compose_row_renders_a_chip_for_a_title` | `title: Some(_)` → that segment has `bg == title_ink` and `ink == CHIP_TEXT` |
| `compose_row_renders_no_chip_when_never_renamed` | `title: None` → seven spaces with `bg: None`. **Not** an empty coloured rectangle |
| `compose_row_tints_the_repo_column_only` | the repo segment carries `repo_ink` as a foreground; the summary segment carries none |
| `compose_row_truncates_per_column` | an over-long repo is clamped to 7 with `…` and **stays tinted**; the summary column is unaffected. Replaces the three span/separator truncation tests, which described a mechanism that no longer exists |
| `compose_row_measures_the_gutter_it_is_given` | a 2-cell and a 9-cell gutter over the same row and `cols` → bodies differ by exactly 7. The S6 contract |
| `compose_row_leaves_terminal_tabs_untinted` | `terminal_name: Some(_)` ⇒ one body segment, `ink: None`, `bg: None` |
| `compose_row_carries_reverse_per_segment` | active + tinted → the inked run and the run after it are both inverted |
| `compose_row_strips_control_characters_before_fitting` | a value containing `\x1b[31m` and `\n` renders with neither, and width is computed on the stripped text |
| `compose_row_never_panics_at_narrow_widths` | every `cols` in `0..=44` returns, emits no partial escape, and the design-lock §3 ruling is not pre-empted (§2.7) |
| `stripping_ink_leaves_the_visible_text_unchanged` | §2.9 — colour is decoration |

**Deleted with the mechanism** (design-lock §9 item 3), listed so nobody
resurrects them from an older draft: `segment_span_indexes_arbitrary_fields`,
`compose_row_tints_title_and_repo_at_the_right_fields`,
`compose_row_tints_repo_at_field_zero_when_untitled`,
`compose_row_truncating_mid_separator_is_accepted`, and
`render_segments_matches_the_pre_s5_line_when_untinted` — the last because the
pre-S5 line is no longer the target output at all (§2.9).

**Width parameterisation.** Every truncation test above runs over
`const TEST_WIDTHS: [usize; 2] = [30, 44]` — the current width and S8's target
(**44**, design-lock §9 item 2; the 38 this spec was written against is dead) —
asserting the same structural property at both. No test hardcodes either number
inline, and the constant carries a comment naming S8. At 30 the body is 19 cells
against a 33-cell design, so the widths do not merely scale: they exercise the
degenerate path §2.7 leaves to S8.

### 5.2 Tier 1 — tests that must change, and tests that must not

**Verified against the tree, and the finding survives the revision — but its
consequence does not.** `grep -n "Row {" crates/clave-bar/src/model.rs` returns
exactly **three** hits: the struct definition and the two construction sites,
both inside `rows()`. **No test constructs a `Row` literal**, so *adding* a field
breaks no test at compile time. That was the whole comfort of the previous
revision, and the design lock spends it: `Row.name` is **removed**, not extended,
so every test that reads a row's name — `model.rs:1229-1234`, `:1237`, `:1246`,
`:1249`, `:1305-1306`, `:2106` — must change. Budget for it; this is the largest
mechanical edit in S5.

| Site | Action |
|---|---|
| `model.rs:1147-1161` `fn agent(...)`, `:1163-1175` `fn agent_labelled(...)` | both set `repo_root: String::new()`; they now also default `repo_ink: 0`, `title_ink: 0`, `title: None`, `summary: String::new()`. **Leave the helpers' shape** — the existing suite stays a control group — but note that 0/0 is *crystalBlue*, not "no tint", so an assertion of "untinted" must be written against `title: None` and an empty repo, never against index 0 |
| `:1229-1234`, `:1237`, `:1246`, `:1249` | ordering assertions: re-point from `Row.name` to whichever column value identifies the row (`repo`, or `title`). Ordering is orthogonal to colour and must stay visibly so |
| `:1305-1306` | pin the terminal-tab path: `terminal_name.is_some()`, `repo_ink.is_none()`, `title_ink.is_none()` |
| `:2106` | the dormant row: assert it renders `a.summary`, **not** a slice of `a.label`. With #69's field on the wire this is the test that proves nothing parses the label any more |
| `crates/clave/src/store.rs` tests (`:400+`), `add.rs:800+`, `hook.rs`, `open.rs`, `lsview.rs`, `setup.rs`, `dev.rs`, `crates/clave/tests/kdl_guardrail.rs` | mechanical: `Agent` literals gain two `u8`s; `Store` literals gain four defaults, or use `..Default::default()` where the test already does. `AgentRecord` gains **nothing** — the ledger lives in `Store` (§2.2), and #69 already added `title`/`summary` there |

### 5.3 Tier 1 — proptests (`model.rs mod proptests`, `:2803+`)

Generators: `title`, `repo` and `summary` each from `"[\\PC ·…]{0,60}"` plus an
escape-injecting strategy, with `title` also `None` half the time; `cols in
0usize..=200`; `active in any::<bool>()`; `gutter` from a small set of 0-, 2- and
9-cell segment vectors; the two inks from `option::of(palette_ink())`.

| Property | Statement |
|---|---|
| **P1 — no escape is ever truncated, every sequence is reset** | scan `render_segments(&compose_row(&row, cols, &gutter))`: every `\x1b` begins a complete `\x1b[` … `m`, the parameter body matches `[0-9;]*`, and the count of non-`0` introducers equals the count of `\x1b[0m`. The line never ends inside a sequence. *Kept verbatim from the previous revision — this is the property the dossier's warning is about, and the chip's `48;2;…` background now rides the same guarantee* |
| **P2 — visible text is colour-independent** | concatenating `Segment.text` yields the same string whatever the two inks contain. §2.9, mechanised. *Kept verbatim* |
| **P3 — width is respected** | for `cols >= gutter_cols + TRAILING_COLS + TITLE_COLS + REPO_COLS + 2` (i.e. `body >= 16`), the escape-stripped line is at most `cols` characters. **The old gate `cols >= gutter_cols + TRAILING_COLS` was FALSE** (#82 review): `compose_row` emits the fixed 7-column title and repo plus their two separators unconditionally, so at a 9-cell gutter and `cols = 11` it already writes 16 cells into a 0-cell body. `summary_w` saturates at 0 and absorbs nothing. Below that threshold the row is the **undefined narrow band** — §2.7 leaves collapsed geometry to S8 and design-lock §3 has not ratified it, so the only contract there remains "never panic, never emit a partial escape". Do not assert width in that band until the collapsed layout exists |
| **P4 — segment text is control-free** | for arbitrary values *including* injected `\x1b[31m` and `\n`, no `Segment.text` contains a control char |
| **P5 — columns hold their positions** | *replaces the old span property, which described a mechanism that no longer exists.* Whenever `body >= TITLE_COLS + REPO_COLS + SUMMARY_COLS + 2`, the repo column starts at the same offset for every generated row, and so does the summary — whatever the values' lengths. This is the invariant the whole fixed-width design exists to provide (design-lock §2.1/§2.3) |
| **P6 — the gutter passes through verbatim** | the first `gutter.len()` output segments equal `gutter`, byte for byte. S6's contract |
| **P7 — a background appears only under a title** | no segment carries `bg: Some(_)` unless the row has a non-empty title, and then only that one segment does. Pins "blank, not an empty chip" against every value the generator can produce |

**In `crates/clave/src/store.rs`** (plain loops, not proptest — `clave` has no
`proptest` dev-dep and these need no generator):

| Property | Statement |
|---|---|
| **P8 — allocation never collides within a repo** | over 200 randomly-ordered agent insertions across 30 repos: within any repo, no two agents share a title index until that repo's agent count exceeds `PALETTE_LEN - 1` (now **7**), and no title index ever equals its repo's index |
| **P9 — allocation is order-stable under replay** | inserting the same agent set through many `with_store_mut` calls in the same order always yields the same ledger |

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

Vocabulary: **repo tint** = the colour of the repo column's text; **title
chip** = the filled 7-column block the title sits in, its ink the background.
Neither is the status glyph, whose colour is the status
(design-lock §4.1). All three terms are defined in
[UBIQUITOUS_LANGUAGE.md](../../../UBIQUITOUS_LANGUAGE.md) §3.

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

The design lock's own preview is the better first look — it renders the whole
44-column design, chips included:

```bash
cargo run -p clave-bar --example bar-preview
```

This step then isolates the palette from the layout, and adds the question the
preview cannot answer: whether a chip is legible on **your** screen, which is a
different question from whether a tint is.

**(a) Run** in any pane:
```bash
i=0
for c in "126;156;216" "152;187;108" "230;195;132" "228;104;118" \
         "149;127;184" "122;168;159" "255;160;102" "210;126;153"; do
  printf '\033[38;2;%sm%d  repo-name\033[0m   \033[48;2;%sm\033[38;2;22;22;29m TITLE \033[0m   \033[38;2;%sm%s\033[0m\n' \
         "$c" "$i" "$c" "$c" "a summary tail"
  i=$((i+1))
done
printf 'plain default text for comparison — no tint above should resemble this\n'
echo "COLORTERM=$COLORTERM  TERM=$TERM"
```

**(b) Look at:** all eight lines in your normal kanagawa dark theme, at your
normal font size. Each line shows the same hue **twice**: as tinted text (how a
repo renders) and as a filled chip with dark text (how a title renders). Then the
ninth line, untinted.

**(c) Report:** any index you cannot comfortably read as text; any chip whose
dark text is hard to read; any pair you cannot tell apart at a glance; whether
any tint looks like plain untinted text; and the `COLORTERM`/`TERM` line.

| Report | Conclusion | Next |
|---|---|---|
| all eight legible as text and as chips, mutually distinct; none resembles plain text | palette accepted | Step 2 |
| indices 0 / 4 (`crystalBlue` / `oniViolet`) blur together | the tightest pair in the set (ΔE 18.2) | report it. This is a **ratified** set (design-lock §4), so the remedy is a decision for the maintainer, not a spec edit — substituting a hue means re-rendering the preview |
| index 4 (`oniViolet`) reads dim or muddy against the bar background | expected, and predicted: it measures 4.67 against the ≥ 5.0 band this spec asks for (§2.4) | report it. Also a maintainer decision — it was ratified by eye over the arithmetic |
| a chip's text is hard to read | the ≥ 4.5 `sumiInk0`-on-hue floor is failing in practice at your font size | report which — the measured minimum is 5.15 on `oniViolet`, so a failure here is a real finding about rendering, not about the numbers |
| a tint looks like ordinary text | it is too close to `fujiWhite` on your actual background | report the index — the ΔE ≥ 20 floor is being violated in practice |
| the colours look nothing like the table, and `COLORTERM` is empty | your terminal is not advertising truecolor | report both values. This is the case `Ink::Indexed` was retained for — the remedy is a nearest-cube fallback table |

### Step 2 — two repos: distinct repo tints, shared within a repo

**(a) Do:** have agents from **two different repos** open simultaneously — e.g.
`$HOME/code/clave` and another repo of yours — with **at least two agents in one
of them**. Use `Alt+a` in each directory if needed.

**(b) Look at:** the repo field of every row.

**(c) Run and report:**
```bash
clave ls --json | jq -r '.agents[] | "\(.repo_root)\t\(.title // "-")\t\(.repo_ink)\t\(.title_ink)"'
jq '{repo_inks, repo_ink_cursor, title_inks, title_ink_cursors}' \
   "$HOME/.local/state/clave/agents.json"
```
plus, per row, the repo tint and title chip you actually see.

| Report | Conclusion | Next |
|---|---|---|
| the two repos differ; every row of one repo shares its repo tint | **the repo axis works** | Step 3 |
| two rows of the **same** `repo_root` differ | `repo_inks` is not being consulted — report the JSON immediately | **stop; report** |
| two rows with **different** `repo_root` share a tint | check `repo_ink_cursor`: ≥ 8 ⇒ the specified wrap; < 8 ⇒ a bug, report | **stop; report** |
| the sandbox (`clave-test`) shows different colours than the real session for the same repo *name* | **expected and by design** — allocation keys on the full `repo_root`, and the sandbox seeds repos under its own root (`dev.rs:239`) | note it and continue |
| **every** row is crystalBlue (index 0) | the §3.1 hazard, live: `repo_ink` defaulted rather than being allocated. Check whether `repo_inks` is populated in the store — if it is, the CLI writing the snapshot predates the field, which in the sandbox means the bar hot-reloaded and the binary did not | report which |
| no row is tinted at all | inks present in the JSON ⇒ the bar is stale, re-check Step 0; absent ⇒ allocation never ran | report which |
| plain terminal tabs are tinted | a row with no joined agent resolved an ink — report | **stop; report** |

### Step 3 — the title axis: same repo, different agents

**(a) Do:** in the repo with two or more agents, ensure each has a distinct Claude
session title (rename one with Claude's own rename, or let them earn different
titles).

**(b) Look at:** the **title chip** on those same-repo rows, and compare each
chip's colour against that row's repo tint.

**(c) Report:** for each same-repo row, the chip colour; whether any chip equals
its own row's repo tint; and whether the chip's dark text is readable at your
font size in a real row (Step 1 asked the same question in isolation).

| Report | Conclusion | Next |
|---|---|---|
| every same-repo agent has a different chip, and none equals the repo tint | **the title axis works and the skip rule holds** — #24's "three `nalu` lookalikes" complaint closed | Step 4 |
| a chip equals its row's repo tint | the skip rule failed — **report immediately** with the `title_inks`/`repo_inks` JSON; `title_inks_are_unique_within_a_repo` should have caught it | **stop; report** |
| two agents in the same repo share a chip | check that repo's `title_ink_cursors` value: ≥ 8 ⇒ specified wrap; < 8 ⇒ bug, report | **stop; report** |
| agents in *different* repos share a chip | expected — uniqueness is scoped per repo, as asked | continue |
| an **untitled** agent shows an empty coloured rectangle | it should show seven blank columns, not a chip (§2.7) — **report**; `compose_row_renders_no_chip_when_never_renamed` should have caught it | **stop; report** |
| **no** row has a title at all | expected until S4 (#59) lands: #69 put `title` on the wire, nothing populates it yet (`add.rs:761`, `hook.rs:317` both write `None`). The repo axis is fully testable regardless | note it and continue |

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
Step 1. They should be entries `cursor` and `cursor + 1` (mod 8) — the two
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
| the colours are the next two indices but look similar to each other | the ratified order puts them adjacent (minimum adjacent ΔE 30.7, `springGreen`→`carpYellow`) | report which pair. Reordering a ratified set is the maintainer's call, not a spec edit — §2.4 |
| a new repo reused an existing colour while the cursor is < 8 | allocation is not monotone — report | **stop; report** |

**Cleanup**, your call afterwards:
`rm -rf "$HOME/code/clave-ink-probe-one" "$HOME/code/clave-ink-probe-two"`.

### Step 5 — force truncation, at both widths

**(a) Do**, in a **non-zellij** terminal:
```bash
mkdir -p "$HOME/code/clave-truncation-probe-repository"
git -C "$HOME/code/clave-truncation-probe-repository" init -q
```
(33-character basename — far longer than the 7-column repo field at any width.)
`Alt+a` it, `new`. Then press `Alt+c` to collapse the bar and `Alt+c` again to
expand.

**(b) Look at:** the new row at full width, and collapsed. The repo column should
show 6 characters and a `…`, still tinted, with the summary column starting at
exactly the same offset as on every other row.

**(c) Report:** exactly what the row shows at each width (transcribe it, including
the trailing `…`), whether the columns still line up down the bar, and whether
**any** stray characters appear — a literal `[38`, a `;2;`, a `[48`, a lone
`[0m`, a stray `m` — or colour bleeding onto the row below.

| Report | Conclusion | Next |
|---|---|---|
| a tinted truncated repo ending `…`, columns still aligned, no stray characters, no bleed, at both widths | **the clamp/ANSI interaction is correct** — the §2.7 structural claim holds live | Step 6 |
| the columns to the right of the long repo shift | the per-column pad is not being applied — the one thing the fixed-width design exists to prevent (design-lock §2.3) | **report immediately**; `compose_row_pads_short_values_so_columns_align` should have caught it |
| any literal escape text visible (`[38`, `;2;`, `[0m`) | an escape was truncated — the exact RC-G failure the design forbids | **report immediately with the transcription.** P1 should have caught it; the generator is wrong |
| colour continues onto the next row or the rest of the pane | a sequence was emitted without its reset | report immediately — `render_segments` is emitting an unpaired introducer |
| the row truncates **shorter** than an untinted row of the same length | escape bytes are being counted as visible width — ink applied before the clamp | report immediately; §3.5's order of operations was not followed |
| collapsed mode shows little or no text | **expected, and undesigned** — design-lock §3 has not ratified the collapsed geometry, and §8.1 records that `COLLAPSED_TARGET_COLS = 4` is a width the bar never actually reaches (it rests at 11 on your window). Whatever you see is the resize floor, not a design. Transcribe it — it is *input* to S8's ruling, not an S5 bug | note it and continue |
| the bar snaps back to full width on its own | expected — the width seek re-targets its constant unless collapsed (`model.rs:1022-1026`); use `Alt+c`, not a manual resize | retry with `Alt+c` |

### Step 6 — colours survive a session restart, and a store round-trip

**(a) Do:** capture first, then restart. **You**, in a non-zellij terminal:
```bash
jq -S '{repo_inks, repo_ink_cursor, title_inks, title_ink_cursors}' \
   "$HOME/.local/state/clave/agents.json" > "$TMPDIR/clave-inks-before.json"
zellij kill-session clave
```
Relaunch the session the way you normally do, and reopen the same repos.

**(b) Look at:** every row's repo tint and title chip, against what you saw in
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

### Step 7 — manual rename, and the honest limitation (now the OTHER one)

The limitation inverted. The previous revision expected a manual rename to put
the ink on the wrong words, because a live row rendered the zellij tab name.
Design-lock §7.1 rules that a live row renders from the **store**, so the ink can
no longer land wrong — and the cost moved: *a manual rename does not appear in
the sidebar at all.* This step now checks that the accepted cost is the one that
actually shows up.

**(a) Do:** manually rename one agent's **zellij tab** (not the Claude session) to
something obviously different — e.g. `ZZZ-MANUAL`.

**(b) Look at:** that row in the sidebar, and at zellij's own tab bar.

**(c) Report:** what each one shows.

| Report | Conclusion | Next |
|---|---|---|
| zellij's tab bar shows `ZZZ-MANUAL`; the sidebar row is unchanged | **the specified behaviour** — design-lock §7.1's accepted cost. `Effect::RenameTab` still fires on label *change* only, so your rename sticks on the tab itself | note it; not a bug |
| the sidebar row changes to `ZZZ-MANUAL` | a live row is still reading the tab name — §7.1 was not implemented, or `rows()` regressed | **report**; `rows_read_values_from_the_joined_agent_not_the_tab_name` should have caught it |
| your rename is immediately overwritten on the tab itself | the loop-guard regressed — that is `rename_only_when_label_changes_not_when_tab_name_differs`, which §3.7 says must not be deleted | **stop; report** |
| it bothers you that the sidebar ignores the rename | worth an issue against #24 | report; the answer is a *clave-level* rename (a title the store owns), not re-reading the tab name — that direction was already ruled out |

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
| the chips dominate the bar | a filled background is a stronger signal than a tint, by design — but the design lock also fades every unselected row 25% (§6), and if that fade is not yet implemented you are seeing the chips at full strength | report whether the fade is in. §3.5's closing note is the seam |
| two axes is too much colour per row | over-decorated | report; dropping the title axis is one branch in `compose_row` (render the 7 columns blank), and the ledger stays. Better to learn this now than after S6 fills the gutter |
| no real improvement because truncation still eats the distinctive part | S5 alone is insufficient — #24 items 3/6 territory | report; S5 can still merge, #24 stays open on those |

---

## 7. Risks, dependencies and out of scope

### Dependencies and sequencing

| Workstream | Relationship |
|---|---|
| **#69** (AgentSnapshot v2) | **was the blocker; landed 2026-07-28.** `title`, `summary`, `worktree` and `LABEL_SEP` are on the wire (`clave-types/src/lib.rs:79-101,146`) and projected by `snapshot_from` (`store.rs:204-208`). S5 **consumes**; it must not re-propose any of them |
| **S4** (label) | **no longer a compile-time dependency, and never was for the ledger.** #69 landed the `title` *field*; S4 (#59) fills its *value* — today `add.rs:761` and `hook.rs:317` write `None`. So S5 ships complete: every agent gets a `title_ink`, and the chip simply does not render until a session has been renamed. **Do not gate S5 on S4.** The same holds for `summary`: #69's backfill seeds it from existing labels, so the 17-column field is not blank on dormant rows either |
| **S6** (gutter) | **soft, and S5 is built for it.** `compose_row` takes the gutter as `&[Segment]` and measures it; `gutter_segments` is the transitional stand-in S6 replaces with the ratified 9 cells. S6 must not change `compose_row`'s signature. One new coupling: S6's provenance cell takes `Row.repo_ink` (design-lock §4.1) |
| **S8** (width 30 → **44**) | **near-none** — no bar width is hardcoded and tests run at both (§5.1). But the body split (7 · 7 · 17) *is* fixed, and it only fits at 44, so a bar narrower than that renders a degenerate row until S8 lands (§2.7). S5 must not touch `BAR_TARGET_COLS` |
| **S0 / S1 / S3** | `BarModel::rows` only — the collision zone §2.1 names. Different concerns, same function |

Shared-file overlap is confined to `crates/clave-types/src/lib.rs` (S5 adds the
palette and two `Agent` fields; #69 already took the region S4 wanted) and
`crates/clave/src/store.rs` (S5 adds `Store` fields and `allocate_inks`; S4
writes `AgentRecord.title`/`.summary`, which #69 already declared).

#### Reconciling with S6's draft — three deltas, all in S6's favour

**Both documents have since been overtaken by the design lock**, which supersedes
S6's geometry outright (its own preamble says so) and is being amended in
parallel. Where the lock speaks, it wins over both. These three deltas concern
the *seam* between S5 and S6, which the lock does not legislate:

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
   inherits) — and neither does its replacement. §2.1 deleted the constant
   because the title field is optional; design-lock §7.1 then deleted `InkSpan`
   too, because there is no composed name to index into. What S6 actually needs
   from S5 is one value: `Row.repo_ink`, for the provenance cell (design-lock
   §4.1). Nothing else.
3. **Two test names S6 cites have changed.**
   `compose_row_at_collapsed_width` (`S6:634`) is gone, and the replacement it was
   given in the previous revision — `compose_row_narrow_width_overflow_is_preexisting`
   — is gone too: design-lock §8.1 rules that the 4-column collapsed width it
   asserted against **is a width the bar never has**. The surviving assertion is
   `compose_row_never_panics_at_narrow_widths` (§5.1). S6's
   `GUTTER_COLS_COLLAPSED == COLLAPSED_TARGET_COLS` const-assert (`S6:606-612`) is
   the right *place* for a collapsed invariant, but the invariant itself waits on
   design-lock §3.

From S8: at the ratified 44 the body is `44 - 9 - 2 = 33` with the design lock's
gutter, and `44 - 2 - 2 = 40` before S6 lands. Neither number is hardcoded in S5 —
`compose_row` derives both from what it is handed — and S8 correctly identifies
`main.rs:546` as *not* its line (`S8:90`).

### Risks

| Risk | Severity | Mitigation |
|---|---|---|
| **The collapsed bar has no design.** The 9-column lead-in cannot fit in a collapsed row, and design-lock §3 has **not ratified** the collapsed geometry — §8.1 further records that the 4-column width this risk was originally written about never occurs (the resize floor rests at 11) | medium | Named here and in Step 5 so it is not misread as an S5 defect. S5's only obligation is not to panic (§5.1, P3). The ruling — including whether a narrow row renders field 0 only — is S8's, and the C6 ledger governs it |
| A terminal without truecolor renders the palette approximately | medium | Step 1 catches it before merge. `Ink::Indexed` is retained so a nearest-cube fallback is data plus one line — for the chip **background** as well, which is `48;2;…` and degrades the same way |
| `crystalBlue`/`oniViolet` (ΔE 18.2) are indistinguishable at the maintainer's font size, or `oniViolet` (contrast 4.67) reads dim | medium | Step 1 has explicit branches for both. Neither has a spec-side remedy: the set is **ratified** (design-lock §4), so a substitution is a maintainer decision |
| **The ink fields ship ahead of the ledger**, or the ledger is reverted leaving them | **high if it happens** | §3.1: `u8` has no "unset", `0` is crystalBlue, and the whole fleet paints one colour. #69 §2.2 held these two fields back for exactly this reason. They land in one change, and `agent_ink_fields_roundtrip_and_default_to_zero` exists to make the default *visible* rather than reassuring |
| `clear_tab_timeline` — or a future session-hygiene function — clears the ink ledger, reshuffling every launch | **high if it happens** | `clear_tab_timeline_preserves_every_ink` is a dedicated regression test, the field doc comments say so in-place, and Step 6 verifies it live with a `diff` |
| Concurrent allocation under the flock | low | §4's ordering argument; adversarial review required by the Cross-process row; §5.3 P8/P9 |
| The ink ledger grows unbounded | low | One short string plus one byte per repo/agent ever seen. A `clave doctor` GC pass is future work |
| ~~`snapshot_from`'s field ordering silently drifts from S4's `compose_label`~~ | **gone** | Design-lock §7.1 removed the mechanism: the bar lays its own columns from values, so there is no ordering to drift and no cross-check to maintain |
| A wide character in a title or summary shifts every column to its right | low | Inherited, not introduced — the current renderer counts scalars too. `fit`'s doc comment names it; a switch to cell measurement is one decision shared with the gutter-invariance test design-lock §9 item 5 already owes |
| Two axes of colour is simply too much decoration | low | Step 8 asks directly, and removing the title axis is one branch in `compose_row` — the ledger and the wire field stay, so it is reversible either way |
| Issue #44 corrupts a live reading | high, standing | Step 0 is mandatory and terminal |

### Out of scope

- **v2 theme sourcing** (§2.6). The deliverable here is that its diff is a
  subscription, a `Styling → Vec<Ink>` function, and a change-gated assignment.
- **Worktree shades/variants** (#24 item 2, second clause). A worktree gets its
  parent's colour where `repo_root` says so, never a lighter one. The reason
  given here has expired — `worktree` **is** on the wire now (#69,
  `store.rs:208`) — but the conclusion has not: the design lock spends the
  worktree signal on the gutter's **provenance glyph** (§2 col 8, §5), tinted
  with the repo ink, which answers "is this a worktree?" as a shape instead of as
  a shade. A second colour axis would now compete with that. Out of scope, and
  the reason is now design rather than plumbing.
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
- **Widths and collapsed-state design** (#24 items 6/7, S8, design-lock §3 — the
  one number still open). S5 must not change `BAR_TARGET_COLS` or
  `COLLAPSED_TARGET_COLS`, and must not decide which columns survive a narrow
  row (§2.7).
- **The selected row's treatment** — powerline caps, waveBlue2 background, and
  the 25% fade on every other row (design-lock §6). S5 keeps today's reverse
  video and leaves `Segment.bg` as the seam (§3.5).
- **Cell-accurate width measurement.** `fit` counts scalars, as the current
  renderer does. Changing that is shared with the gutter (design-lock §2.1) and
  is one decision, not two.
- **Terminal-tab identity.** `Tab #16` is a placeholder (design-lock §7.2); S5
  renders it across the body and tints nothing.
