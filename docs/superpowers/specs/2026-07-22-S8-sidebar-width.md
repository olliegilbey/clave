# S8 — Sidebar width: 30 → 38 columns

> ## ⚠ THE TARGET IS 44, NOT 38 — superseded 2026-07-25
>
> The maintainer ratified the sidebar's visual design from rendered rows on
> 2026-07-25. **Read
> [`2026-07-25-sidebar-visual-design-lock.md`](2026-07-25-sidebar-visual-design-lock.md)
> before this file**, and run `python3 docs/superpowers/specs/bar-preview.py`.
>
> What changed for S8 specifically:
> - **`BAR_TARGET_COLS = 44`**, not 38. Every "38" below is dead. Re-derive the
>   expected-red test set — do **not** trust the one in §6.2.
> - **`COLLAPSED_TARGET_COLS = 4` is unreachable** and always was: zellij's
>   resize floor stops the seek above it (`model.rs`, the doc-comment on the
>   constant itself), so today's collapsed width is whatever the window's
>   granularity floor happens to be — 11 on the maintainer's machine. Any
>   reasoning that assumes the bar is ever 4 columns wide is void.
> - The collapsed target is **still open** and is S8's to settle. The binding
>   constraint is `BAR_TARGET_COLS − COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP`
>   (20), so at 44 the collapsed target must be **< 24**.
>
> **Still valid, and still the trap:** the mechanical-replace hazard in §6.2.
> `30` appears both as *the width target* and as an *arbitrary start width*, and
> `seek_waits_for_inflight_resizes_and_zellijs_floor` must be left alone. That
> lesson applies unchanged to 44.

_2026-07-22 · implementation spec · main `50fa26a` (v0.1.1 + PR #29)_

Read first: [`2026-07-22-ux-defect-dossier.md`](2026-07-22-ux-defect-dossier.md)
(shared source of truth), [`AGENTS.md`](../../../AGENTS.md),
[`docs/dev/TESTING.md`](../../dev/TESTING.md), and — non-negotiable for this
change — **C6** (`SUBSYSTEM-VALIDATION.md:306-508`, the width-seek saga) and the
C8 percent-pane finding (`SUBSYSTEM-VALIDATION.md:568-590`). Issue #24 item 6
says it in one line: *"MUST coordinate with #4 (the 30-col seek target is a
constant in the width-seek machinery)."*

---

## 1. Problem and goal

**Problem.** The uncollapsed sidebar is 30 columns
(`crates/clave-bar/src/model.rs:137`). The render budget is
`cols.saturating_sub(3)` (`crates/clave-bar/src/main.rs:546`) = **27 cells** of
text. S4 recomposes the row as `title · repo · summary` and S6 takes the gutter
from 2 cells to **6 columns** (three spaced glyph cells), dropping the text
budget to **23** (`30 − 6 − 1` right margin). At 23 cells the summary — the only
segment that distinguishes three lookalike rows — is truncated to nothing. That
is #24 item 6, verbatim: *"30 cols truncates everything distinctive."*

**Goal.** Move the expanded target to **38 columns** (text budget **31** =
`38 − 6 − 1` with S6's 6-column gutter), without disturbing the width-seek
machinery that has stranded
twice — the drift re-arm (#4, PR #27, commit `7644fd8`) and the stale-anchor
regression (pinned at `model.rs:1773`).

**Accepted cost** (maintainer's call): 8 columns off every agent pane. On a
200-column window an agent pane goes 170 → 162 (−4.7 %); on a 120-column laptop
90 → 82 (−8.9 %). `claude`'s TUI reflows; long tool output rewraps.

**Non-goal.** No behavioural change to the seek. Every gate, every bound, every
comment in `width_seek` (`model.rs:1019-1136`) stays exactly as PR #27 left it.
This change moves a number and re-derives the arithmetic that depends on it.

---

## 2. The complete width-site inventory

Every place the width target is expressed, assumed, or derived. **A missed site
is how this regresses.** Split by whether the site *changes*.

### 2.1 The target itself

| # | Site | Today | Role | Change |
|---|---|---|---|---|
| 1 | `crates/clave-bar/src/model.rs:133-137` | `const BAR_TARGET_COLS: usize = 30;` | **the** expanded target; the only authority | **yes** → 38, relocated to `clave-types` (§3.3) |
| 2 | `crates/clave-bar/src/model.rs:138-142` | `const COLLAPSED_TARGET_COLS: usize = 4;` | collapsed target (Alt+c gutter) | **no** — independent, §3.5 |
| 3 | `crates/clave-bar/src/model.rs:1022-1026` | `let target = if self.collapsed && !self.peeking { COLLAPSED_TARGET_COLS } else { BAR_TARGET_COLS };` | the seek's target selector — the **only** read of either constant in production code | **no** (reads the constant symbolically) |

### 2.2 Predicates and bounds that are *functions of* the target

None of these are the target, and none of them change — but each one's behaviour
shifts when the target moves, so each is a test-derivation site.

| # | Site | Code | Interaction with the target |
|---|---|---|---|
| 4 | `model.rs:1030` | `let step = self.seek_step.max(8) as i64;` | the ±4-col pre-learning slack; acceptance is `2*abs(cols−target) <= step` (`model.rs:1032`). Band **half-width** is `step.max(8)/2`, i.e. ±4 before learning, ±10 at `MAX_LEARNABLE_STEP` |
| 5 | `model.rs:1032` | `let within_band = 2 * diff.abs() <= step;` | the settle predicate. **This is what silently reclassifies old test values** — see §6.2 |
| 6 | `model.rs:1123-1132` | the act/converge branch, re-reading `step` | same predicate, post-learning |
| 7 | `model.rs:1059-1062` | `ours = abs(own_cols − seek_last_cols) <= step` | gate B's self-inflicted test. Target-independent, but its *outcome* on a given pair of widths changes with the learned step |
| 8 | `model.rs:143-146` | `const SEEK_BUDGET: u32 = 16;` | steps per episode. The expand transition is now 8 cols longer → at most one extra step. 4→38 at a 7-col step = 5 steps ≪ 16 |
| 9 | `model.rs:147-151` | `const MAX_LEARNABLE_STEP: usize = 20;` | caps the band half-width at 10. New invariant to pin: `BAR_TARGET_COLS − COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP` (34 > 20 ✓) so no learned step can make one target's band swallow the other |

### 2.3 The generated KDL — percent-sized bar panes

The dossier's note is exact: percent sizing was a deliberate fix. Fixed
`size=30` made zellij refuse **every** resize (`CantResizeFixedPanes`,
`tiled_pane_grid.rs::can_change_pane_size`) — Alt+c was dead in every freshly
launched session until `SUBSYSTEM-VALIDATION.md:568-583`. The percent is a
**birth hint**, not a target: births land window-dependent and the birth-armed
seek (`model.rs:296`, `seek_budget: SEEK_BUDGET` in the manual `Default`)
finishes the job on the first `render()`.

| # | Site | Code | Change |
|---|---|---|---|
| 10 | `crates/clave/src/setup.rs:176` | `\x20       pane size=\"15%\" borderless=true {{\n\` — `layout_kdl`, the session `default_tab_template` (written at `setup.rs:395`) | **yes** → derived percent |
| 11 | `crates/clave/src/setup.rs:214` | same literal in `launch_layout_kdl` (composed at `setup.rs:693`) | **yes** → derived percent |
| 12 | `crates/clave/src/add.rs:108` | `pane size="15%" borderless=true {{` — `tab_node`, the one-shot `zellij action new-tab --layout` file (`add.rs:719`) | **yes** → derived percent |
| 13 | `crates/clave/src/add.rs` `tab_node_bare` | **no `size=` at all** — deliberately bar-less (the double-bar finding, `setup.rs:198-206`) | **no** |
| 14 | `setup.rs:166-171`, `setup.rs:209`, `add.rs:100-101`, `model.rs:135` | four comments that name `size=30` as the forbidden form | **yes**, text only — `size=30` becomes a stale example once the target is 38; keep the prohibition, update the number |

### 2.4 The truncation budget

| # | Site | Code | Change |
|---|---|---|---|
| 15 | `crates/clave-bar/src/main.rs:546` | `let budget = cols.saturating_sub(3); // gutter + margin` | **no — not S8's line.** It is a function of the runtime `cols` parameter (`main.rs:525`), never of `BAR_TARGET_COLS`. S5 replaces it with `cols.saturating_sub(GUTTER_COLS + RIGHT_MARGIN_COLS)`; S6 sets `GUTTER_COLS` to 6 (three spaced glyph cells). See §8 |
| 16 | `crates/clave/src/add.rs:80-88` `sanitize_label` | no length cap | **no** |
| 17 | `crates/clave/src/lsview.rs` | `clave ls` human view — no width logic at all (grep for `width`/`truncat`/`chars().take` returns nothing) | **no** |

### 2.5 Tests and fixtures

| # | Site | What it pins | Change |
|---|---|---|---|
| 18 | `model.rs:1662-1673` `a_newborn_model_seeks_the_template_width` | birth-armed seek from both sides | **strengthen** (passes at 38 but is target-blind — §6.2 T1) |
| 19 | `model.rs:1675-1703` `seek_collapses_to_the_gutter_despite_coarse_steps` | line 1685 `width_seek(30) == []` | **breaks** → 38 |
| 20 | `model.rs:1705-1725` `seek_expands_back_to_template_width` | line 1724 `(cols - 30).abs() <= 4` | **breaks** → 38 |
| 21 | `model.rs:1727-1750` `seek_waits_for_inflight_resizes_and_zellijs_floor` | 30 as a **start width**, target 4 | **do not touch** — the trap, §6.2 T4 |
| 22 | `model.rs:1752-1770` `idle_seek_re_arms_when_a_relayout_drifts_it_off_target` (**#27**) | drift re-arm at the collapsed target | unchanged; **twin added at 38** (§6.3) |
| 23 | `model.rs:1772-1797` `drift_is_measured_from_the_rest_width_not_a_stale_emit_anchor` (**#4**, the `model.rs:1773` pin) | rest-anchored drift at the collapsed target | **unchanged, byte-identical**; twin added at 38 (§6.3) |
| 24 | `model.rs:1799-1820`, `:1822-1840` | oscillation immunity, collapsed target | unchanged |
| 25 | `model.rs:1842-1853` `seek_grows_back_from_an_overshoot` | expanded target | **breaks** → re-derive (§6.2 T9) |
| 26 | `model.rs:1855-1867` `seek_never_learns_an_external_jump_as_the_step_size` | expanded target | **breaks** → re-derive (§6.2 T10) |
| 27 | `model.rs:1869-1880` `seek_budget_caps_a_layout_that_never_converges` | collapsed target | unchanged |
| 28 | `model.rs:1882-1898` `peek_expands_a_collapsed_bar_and_expiry_sinks_it` | peek retargets to the template | passes; **cosmetic** 30→38 |
| 29 | `model.rs:1900-1907` `expanded_bars_ignore_peeks` | line 1906 `width_seek(30) == []` | **breaks** → 38 |
| 30 | `model.rs:1909-1920` `toggle_cancels_a_peek_and_a_late_expiry_is_a_noop` | Grow from 13 | unchanged |
| 31 | `model.rs:1926-1936` `snapshot_hydrates_a_newborn_into_collapse` | Shrink from 30, target 4 | passes; **cosmetic** 30→38 |
| 32 | `model.rs:1943-1977` `snapshot_heals_a_desynced_instance_and_leaves_synced_ones_alone` | line 1948 `width_seek(30) == []` | **breaks** → 38 |
| 33 | `model.rs:2518-2796` the convergence harness (`SimZellij`, `drive`, `band_half`, 6 harness tests) | the render-feedback convergence contract | **no refactor needed** — already parameterised on the constants (`:2699`, `:2716`, `:2746`, `:2765`, `:2792`). §6.4 |
| 34 | `model.rs:2813-2876` `prop_width_seek_converges_or_bounds` | the (a)–(d) contract; target read symbolically at `:2863-2867` | **no edit**; one property **added** (§6.5) |
| 35 | `crates/clave-bar/proptest-regressions/model.txt` | 3 pinned seeds; **2 are `collapsed = false`** so they replay against the new expanded target | **no edit** — a failure here is a finding, not a fixture to update |
| 36 | `crates/clave/src/setup.rs:737-760` `generated_bar_panes_are_percent_sized_not_fixed` | `kdl.contains("size=\"15%\"")` and `!kdl.contains("size=30")` | **breaks** → derived percent + both negative literals |
| 37 | `crates/clave/tests/kdl_guardrail.rs:157,164,172,186` | real-parser validity of all three generated layouts | **no edit** — re-run; a percent change stays valid KDL |
| 38 | `crates/clave/tests/zellij_pin_tripwire.rs`, the #52 version-coherence test | unrelated | re-run only |

### 2.6 Documentation that records the number

| # | Site | Change |
|---|---|---|
| 39 | `SUBSYSTEM-VALIDATION.md:492-496` (C6 round 21: *"BAR_TARGET_COLS 26→30 plus both `size=` templates"*) | **append** a round-22 entry — TESTING.md:377 requires ledger updates in the same commit |
| 40 | `SUBSYSTEM-VALIDATION.md:568-583` (C8: `size="15%"` fix) | append the percent change to the same round-22 entry |
| 41 | `docs/superpowers/specs/2026-07-22-S4-…:43-45,185,208`; `…-S5-…:1229-1230` | already forward-reference S8 by name; **no edit** |
| 42 | `docs/status/…` handoffs naming 26/30 | historical record — **never rewritten** |

**Not a width site, checked and cleared:** `crates/clave/src/dev.rs` (sandbox
launch calls `setup::run_setup` / `setup::launch_session` verbatim,
`dev.rs:135-139,163` — it inherits every generator, expresses no geometry);
`config.kdl` generation (`setup.rs:96-150`, keybinds only); `PaneMeta`
(`model.rs:25-31` — carries no columns, see §9).

---

## 3. Design

### 3.1 Chosen target: **38 columns, exactly**

Not "~38". A range is not implementable and the seek's acceptance band already
supplies the tolerance (±4 cols pre-learning, ±10 at the learnable-step cap), so
"38 ± the band" is what actually ships.

Why 38 is the right exact number:

- **Text budget arithmetic.** With S6's **6-column gutter** (three spaced glyph
  cells) and S5's `RIGHT_MARGIN_COLS = 1`, `budget = cols − 7`: **31** cells at
  38, against **23** at 30. A representative S4-composed label —
  `F-CLA · clave · fix the auth flow` — is 32 chars: cut to `F-CLA · clave · fix the…`
  at 31, but to `F-CLA · clave · fi…` at 23. The +8 is the difference between
  keeping and losing the distinguishing summary segment.
- **It lands on a clean birth percent.** 38 / 200 = **19 %**. The maintainer's
  common geometry is a ~200-column window, so a newborn bar is essentially
  on-target before the seek acts — the same property `15%` bought for 30
  (§3.4).
- **The two targets stay provably disjoint.** `BAR_TARGET_COLS −
  COLLAPSED_TARGET_COLS` = 38 − 4 = 34 > `MAX_LEARNABLE_STEP` (20), so no learned
  step can make the expanded band accept a collapsed width or vice versa. At 30
  the gap was 26; at 38 it is 34 — strictly safer. (This gap is a target
  separation, not the text budget — do not confuse it with the 23/31 above.)
- **It is cheap to revisit.** If live judgement (§7 step 8) says 36 or 40, the
  change is one constant, one percent, and the test values in §6.2. That
  cheapness is why §7 puts a judgement gate *after* the mechanical work rather
  than blocking on a mock-up.

### 3.2 Constant, not configurable — and why

**Decision: a compile-time constant.** Rejected: any runtime-configurable target.

The brief's own framing is the argument. A configurable target must reach both
the CLI (which generates the KDL) and the plugin (which seeks). Three channels
exist; all three are the #43/#44 shape:

| Channel | How it would work | Why it is rejected |
|---|---|---|
| **zellij plugin config in the KDL** — `plugin location="file:…" { target_cols "38" }`, read in `load()` | the generator that sizes the pane also passes the target, so they cannot disagree *within one artifact* | **three** generators emit bar panes (`setup.rs:176`, `setup.rs:214`, `add.rs:108`), and `add::tab_node` runs at `clave open` / `clave add` time from **whatever `clave` is on `PATH`** — #44 is unfixed. A stale binary emits a different target for tabs created later in the same session ⇒ per-tab width divergence with no self-heal. This is precisely the v0.1.1 mixed-artifact incident (#43) |
| **an `AgentSnapshot` field** — the store becomes the authority | buildable: `heal_collapse` (`model.rs:955-961`) is the exact change-gated re-arm pattern | the target then changes **mid-session**, multiplying the seek's state space at the one place two regressions already lived (#4, #27). It buys nothing — nobody wants a per-session sidebar width — and it makes the "does the change re-arm cleanly?" question a live hazard instead of a non-existent code path |
| **an env var read by both** | trivial host-side | the plugin is wasm; env reaches it only through the layout, so this collapses into channel 1 with worse ergonomics |

There is **no user-config surface in clave at all** — recorded independently in
the S5 spec (§7: *"There is no user-config surface in clave; adding one is a CLI
+ artifact change with its own taxonomy row"*). Adding one for this is a
`Install / environment` taxonomy row (TESTING.md:118) plus a
`needs-live-validation` label, for a number the maintainer will set once.

**The decisive property of the constant:** there is no code path in which a
running instance's target changes. The mid-session-change hazard is eliminated by
construction, not handled. §5 spells out what that means for a running fleet.

### 3.3 One definition, in `clave-types`

The one skew that *does* exist today is hand-derivation: the birth percent
(`15%`, three string literals in `crates/clave`) approximates
`BAR_TARGET_COLS / 200` (a constant in `crates/clave-bar`), and nothing connects
them. Round 21 changed 26 → 30 and had to remember to touch *"both `size=`
templates"* (`SUBSYSTEM-VALIDATION.md:494`) by hand.

Fix it while we are here. `crates/clave-types` is already a dependency of both
crates (`crates/clave/Cargo.toml:26`, `crates/clave-bar/Cargo.toml:35`) and
already compiles to wasm. Move the two targets there and add the birth percent
beside them, with the derivation in the doc comment. `setup.rs` and `add.rs` then
**format** the percent instead of hardcoding it, and a Tier-1 test asserts the
emitted KDL carries exactly that number (§6.6).

**Cross-binary skew after this is benign, and that is the point.** A stale
`clave` on `PATH` (issue #44) still emits `15%` into a tab layout while the new
wasm seeks 38. Nothing breaks: the percent is a *birth hint*, the seek is the
authority, and the birth-armed seek converges the difference in ≤ 3 resize steps
on any realistic window. Say this in the PR dossier as the cross-process
argument the taxonomy demands (TESTING.md:117) — the geometry contract is
one-way, so the seam cannot desync.

Cheap to decline: if a reviewer objects to a new public surface in
`clave-types`, the fallback is to leave both constants in `model.rs` and keep the
percent hand-derived with a `// keep in sync with BAR_TARGET_COLS` comment. That
is strictly worse (it is the exact thing round 21 had to do by hand) but it is
not wrong, and it does not change any other part of this spec.

### 3.4 Birth percent: 15 % → 19 %

Keeping `15%` would work — the seek converges either way — but it would make
**every** birth do 1–3 grow steps on the maintainer's common geometry, where
today the newborn lands essentially on target. Each step is a real
`resize_pane_with_id` (`main.rs:169-184`) and therefore a real tab relayout and a
real `claude` TUI reflow, at session launch, across every tab at once. That is
visible flicker for no reason.

19 % is `BAR_TARGET_COLS` against a documented 200-column reference viewport.
Bounds on other geometries, all handled by the seek:

| Window | Birth cols at 19 % | Seek travel to 38 |
|---|---|---|
| 100 | 19 | grow ~2–3 steps |
| 160 | 30 | grow ~1–2 steps |
| 200 | 38 | **0 steps** |
| 400 | 76 | shrink ~2–4 steps |

All well inside `SEEK_BUDGET = 16`.

### 3.5 Collapsed mode: unchanged at 4, and deliberately **not** derived

`COLLAPSED_TARGET_COLS` stays `4`.

- It is a **content** measurement, not a fraction of anything: one status glyph,
  one space, and whatever the renderer's truncation leaves (`model.rs:138-141`).
  S5's collapsed-width test builds directly on that reading. (Name deliberately
  not pinned here: it has been renamed twice — `compose_row_at_collapsed_width`
  → `compose_row_narrow_width_overflow_is_preexisting` → S6's
  `compose_row_emits_no_ellipsis_at_zero_budget`. S6 §2.9.3 owns the final name.)
- Deriving it (`BAR_TARGET_COLS / 8`, say) would couple two independent design
  decisions and silently move the collapsed width every time the expanded one
  moved — the opposite of what #24 item 7 wants, which is a *design pass* on what
  4 columns can distinguish.
- The C8 ruling stands regardless: *"the collapsed FLOOR is granularity-dependent
  … the bar rests one resize-step above it — 14 cols on the dev window vs ~10 in
  the real session"* (`SUBSYSTEM-VALIDATION.md:585-590`), and round 20's
  *"wherever cols stop changing is accepted"*. The nominal 4 is a direction, not
  a promise, and widening the expanded target does not touch it.

The **one** interaction is band disjointness, and it improves: the gap grows
26 → 34 against a maximum band half-width of 10. §6.3 pins it as an assertion so a
future edit cannot erode it silently.

### 3.6 Rejected alternatives

| Alternative | Why not |
|---|---|
| **Window-relative target** (e.g. `min(38, cols_total / 4)`) | the plugin has no viewport width. `render(_rows, cols)` (`main.rs:525`) delivers only the plugin's *own* columns, and `PaneMeta` (`model.rs:25-31`) drops `PaneInfo.pane_columns` at the adapter boundary (`main.rs:452-466`). Implementing it means extending `PaneMeta`, summing per tab, and making the target a function of a *stale* frame — the RC-A staleness class, applied to geometry. Out of scope; see §9 for when it becomes worth it |
| **Make the KDL pane fixed-size at 38** (`size=38`) so no seek is needed | `CantResizeFixedPanes`: zellij refuses **every** resize on a fixed pane, so Alt+c dies session-wide. This is the C8 finding that cost a live round (`SUBSYSTEM-VALIDATION.md:568-583`). The negative assertion at `setup.rs:755-758` exists to prevent exactly this, and §6.6 extends it to the new number |
| **Two targets (wide/narrow) toggled by a third keybind** | a new CLI surface, a new keybind, a new persisted flag, and a third state in the seek's target selector — for a preference that is set once |
| **Leave 30 and rely on S4's give-way truncation alone** | S4 §3.4 makes 23 cells *survivable*, not *useful*; the maintainer has read that spec and asked for the columns anyway. #24 item 6 is a separate item from item 3 for this reason |
| **Change the target but not the birth percent** | correct but flickery — §3.4 |

---

## 4. Implementation, file by file

Six edits. Order matters only in that (1) must precede (2) and (3).

### 4.1 `crates/clave-types/src/lib.rs` — the single definition

Append (top-level, after the `Register` struct at `:98-101`):

```rust
/// The expanded width the sidebar's width seek converges to (#24 item 6).
///
/// THE single authority for sidebar geometry. It is read in exactly one place
/// in production code — `clave_bar::model::width_seek`'s target selector — and
/// by `BAR_BIRTH_PERCENT` below. Nothing that RENDERS may read it: the render
/// path receives `cols` from zellij and every budget is a function of that
/// parameter, never of this constant (S4/S5/S6 contract, S8 §8).
///
/// History: 26 (C6 round 20) → 30 (round 21) → 38 (S8). Each move is a live
/// judgement recorded in the C6 ledger, not a tuning knob.
pub const BAR_TARGET_COLS: usize = 38;

/// Collapsed width target (Alt+c): a glyph gutter — the state glyph plus a
/// couple of name chars survive the renderer's own truncation, so "mini mode"
/// needs no special render path. Zellij's resize floor may stop the seek above
/// this; wherever cols stop changing is accepted (C6 round 20).
///
/// DELIBERATELY INDEPENDENT of `BAR_TARGET_COLS` — a content measurement, not a
/// fraction (S8 §3.5). The one coupling is an invariant, pinned by
/// `targets_are_disjoint_by_more_than_a_learnable_step`: the two targets must
/// stay further apart than any learnable resize step, or one target's
/// acceptance band could swallow the other.
pub const COLLAPSED_TARGET_COLS: usize = 4;

/// The bar pane's birth size in the generated layouts, as a PERCENT.
///
/// `BAR_TARGET_COLS` against a 200-column reference viewport (38/200 = 19 %).
/// The size MUST be a percent: a fixed `size=38` makes zellij refuse every
/// resize on the pane (`CantResizeFixedPanes`) and Alt+c dies session-wide —
/// C8 2026-07-18, `SUBSYSTEM-VALIDATION.md:568-583`.
///
/// This is a BIRTH HINT, not a contract. The bar's birth-armed seek is the
/// authority and converges any starting width onto `BAR_TARGET_COLS`, so a
/// stale `clave` binary emitting an older percent (issue #44) is benign — it
/// costs at most a few resize steps at birth, never a wrong resting width.
///
/// DERIVED, not an independent literal (CodeRabbit 2026-07-22): computed from
/// `BAR_TARGET_COLS` against the reference viewport with round-to-nearest, so a
/// future target change cannot re-introduce the hand-synchronisation defect this
/// section removes. `(38 * 100 + 100) / 200 = 19`.
pub const BAR_BIRTH_REFERENCE_COLS: usize = 200;
pub const BAR_BIRTH_PERCENT: usize =
    (BAR_TARGET_COLS * 100 + BAR_BIRTH_REFERENCE_COLS / 2) / BAR_BIRTH_REFERENCE_COLS;
```

### 4.2 `crates/clave-bar/src/model.rs:133-142` — consume, don't redefine

Replace:

```rust
/// The expanded width the seek converges to. The generated layouts
/// (setup::layout_kdl and add::tab_layout) size the bar pane in PERCENT —
/// a fixed `size=30` made zellij refuse every resize (CantResizeFixedPanes)
/// — so births land near this and the birth-armed seek finishes the job.
const BAR_TARGET_COLS: usize = 30;
/// Collapsed width target (Alt+c): a glyph gutter — the state glyph plus a
/// couple of name chars survive the renderer's own truncation, so "mini
/// mode" needs no special render path. Zellij's resize floor may stop the
/// seek above this; wherever cols stop changing is accepted.
const COLLAPSED_TARGET_COLS: usize = 4;
```

with nothing — and extend the existing import at `model.rs:12`:

```rust
use clave_types::{Agent, AgentSnapshot, BAR_TARGET_COLS, COLLAPSED_TARGET_COLS, Status};
```

`width_seek`'s target selector (`model.rs:1022-1026`) is **untouched** — it
already reads both symbolically. `SEEK_BUDGET` and `MAX_LEARNABLE_STEP` stay
private to `model.rs`: they are properties of the seek algorithm, not of the
geometry contract.

### 4.3 `crates/clave/src/setup.rs` — two generators

`layout_kdl`, replacing `:166-183`:

```rust
    // size MUST be a percent: fixed sizes (`size=38`) make zellij refuse
    // every resize on the pane (CantResizeFixedPanes) — Alt+c collapse was
    // dead in any freshly-launched session (c8-cold-start 2026-07-18; pre-C8
    // sessions were resurrected from the serialized cache, which rewrites
    // sizes as percentages — masking this). The percent is derived from
    // clave_types::BAR_TARGET_COLS against a 200-col reference viewport; the
    // bar's birth-armed width seek converges it onto the exact target.
    let pct = clave_types::BAR_BIRTH_PERCENT;
    format!(
        "// GENERATED by `clave setup` — regenerate, don't hand-edit.\n\
         layout {{\n\
         \x20   default_tab_template split_direction=\"vertical\" {{\n\
         \x20       pane size=\"{pct}%\" borderless=true {{\n\
         \x20           plugin location=\"file:{wasm}\"\n\
         \x20       }}\n\
         \x20       children\n\
         \x20   }}\n\
         \x20   tab name=\"clave\" focus=true\n\
         }}\n"
    )
```

`launch_layout_kdl`, replacing `:209-219` — same substitution:

```rust
    // size="{pct}%" not size=38: fixed panes refuse resizes — see layout_kdl.
    let pct = clave_types::BAR_BIRTH_PERCENT;
    format!(
        "// GENERATED at launch — §6.8 clave-owned cold start.\n\
         layout {{\n\
         \x20   default_tab_template split_direction=\"vertical\" {{\n\
         \x20       pane size=\"{pct}%\" borderless=true {{\n\
         \x20           plugin location=\"file:{wasm}\"\n\
         \x20       }}\n\
         \x20       children\n\
         \x20   }}\n{tab}}}\n"
    )
```

### 4.4 `crates/clave/src/add.rs:100-115` — the one-shot tab layout

Replace the literal in `tab_node`. Note the `r#"…"#` raw string already
interpolates `{label}`/`{wasm}`/`{cwd}`, so `{pct}` needs no escaping:

```rust
    // wrapper as setup::layout_kdl and the S2 spike layout). The size is a
    // PERCENT derived from clave_types::BAR_TARGET_COLS, never a fixed
    // `size=38`: fixed panes refuse resizes — see setup::layout_kdl.
    …
    let pct = clave_types::BAR_BIRTH_PERCENT;
    format!(
        r#"    tab name="{label}" focus=true {{
        pane split_direction="vertical" {{
            pane size="{pct}%" borderless=true {{
```

`tab_node_bare` (`add.rs`, no `size=`) is untouched — it is deliberately
bar-less (`setup.rs:198-206`, the double-bar finding).

### 4.5 `crates/clave/src/setup.rs:737-760` — the guardrail test

Replace the two literal assertions:

```rust
            assert!(
                kdl.contains("size=\"15%\""),
                "bar pane must be percent-sized:\n{kdl}"
            );
            assert!(
                !kdl.contains("size=30"),
                "fixed size resurrects the FIXED! bug:\n{kdl}"
            );
```

with a derived positive and **both** historical negatives:

```rust
            let pct = clave_types::BAR_BIRTH_PERCENT;
            assert!(
                kdl.contains(&format!("size=\"{pct}%\"")),
                "bar pane must carry the derived birth percent:\n{kdl}"
            );
            // The generators emit QUOTED values (`size="19%"`), so a fixed-size
            // regression is `size="30"` / `size="38"`, NOT the bare `size=30`
            // the earlier draft searched — that check could pass while the bug
            // was reintroduced (CodeRabbit 2026-07-22). Forbid the fixed form in
            // BOTH quoted and bare spellings, for both historical numbers and
            // the current target; never forbid the `%` form.
            let target = clave_types::BAR_TARGET_COLS;
            let mut forbidden = vec![
                "size=\"30\"".to_string(), "size=30".to_string(),
                "size=\"38\"".to_string(), "size=38".to_string(),
                format!("size=\"{target}\""), format!("size={target}"),
            ];
            forbidden.dedup();
            for fixed in &forbidden {
                assert!(
                    !kdl.contains(fixed.as_str()),
                    "fixed size {fixed} resurrects the FIXED! bug:\n{kdl}"
                );
            }
```

`crates/clave/tests/kdl_guardrail.rs` needs **no edit** — it parses whatever the
generators emit with the real zellij-utils 0.44.3 parser, and a percent change
stays valid KDL. It must be re-run (it is in `cargo test --workspace`).

### 4.6 `crates/clave-bar/src/model.rs` tests — §6.2 and §6.3

The re-derivations and the new tests. Split out because the derivation is the
risky part of this change.

### 4.7 `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` — the ledger

Append to C6, after the round-21 paragraph (`:492-508`):

> **Round 22 (2026-07-22, S8 / #24 item 6): 30 → 38 cols.** `BAR_TARGET_COLS`
> and `COLLAPSED_TARGET_COLS` relocated to `clave-types` as the single
> definition; the three generated bar panes now derive their birth size from
> `BAR_BIRTH_PERCENT` (19 %, = 38/200-col reference viewport) instead of the
> hand-maintained `15%` round 21 had to update by hand. `COLLAPSED_TARGET_COLS`
> unchanged at 4 and deliberately independent — the two targets are now 34 apart,
> pinned as an invariant against `MAX_LEARNABLE_STEP` so neither band can swallow
> the other. The convergence harness (issue #10) needed no refactor: it was
> already written against the constants. The #4 stale-anchor pin and the #27
> drift-re-arm pin run at the *collapsed* target and are byte-unchanged; both
> gained expanded-target twins, which is where the change actually bites.
> Existing bars retarget on plugin reload (birth-armed seek converges on the
> first render — an improvement on round 21's "next toggle cycle", courtesy of
> #27); born-at-38 panes need a session recreate.

Add the live-validation verdict in the same section once §7 completes.

---

## 5. Reflow and repaint

Two different paths, and they are genuinely different. Read
`SUBSYSTEM-VALIDATION.md:306-508` before arguing with any of this — every
forbidden approach below cost a live round.

### 5.1 Fresh session (launch path)

1. `clave setup` writes `layout.kdl` with `size="19%"` (`setup.rs:395`).
2. `clave launch` composes `launch.kdl` from the store (`setup.rs:693`) and
   `exec`s zellij with `--layout`.
3. Every bar pane is born at 19 % of its tab, window-dependent.
4. Each instance's `BarModel::default()` carries `seek_budget: SEEK_BUDGET`
   (`model.rs:296`) — the C8 birth-arm — so the seek is live from the first
   `render()` (`main.rs:534`) with no user action.
5. Convergence: 0 steps at a 200-col window, ≤ 4 at the extremes (§3.4 table).

Per-tab layouts created later (`clave open`, `clave add` → `add::tab_layout`,
`add.rs:719`) follow the same shape.

### 5.2 Running fleet

**The target cannot change under a running instance** — it is compiled in (§3.2).
So there are exactly two ways 38 reaches a live bar:

| Path | What happens | Re-arm behaviour |
|---|---|---|
| **Plugin reload** (`start-or-reload-plugin`, TESTING.md:212-217) | every reloaded instance is reincarnated from scratch — `BarModel::default()`, budget full, `seek_rest`/`seek_last_cols` `None` | converges on its **first render**. Trace: gate A no (`seek_rest == None`), gate B skipped (`seek_budget != 0`), gate C `None` arm, gate D `diff = 30 − 38 = −8`, `−2·diff = 16 > 8` → `GrowSelf`. One step on a coarse-step layout. **This is better than round 21**, which needed "the next toggle cycle" — the #4 birth-arm made it eager |
| **Session recreate** | births at 19 % (§5.1) | n/a |

**Mixed populations are real and must be watched.** A hot-reload replaces the
wasm at one path; only instances that actually reload retarget. C6's round-10
FINAL CONSTRAINT applies — *"zellij emits NO events for plugin-initiated
resizes"* — so each bar drives only its own pane from its own renders. The
consequence is **per-tab width divergence** (some bars 30, some 38) until a
session recreate. It is cosmetic, it self-heals, and it is the same family as the
C8 collapse parity-desync (`SUBSYSTEM-VALIDATION.md:592-606`). Live steps 3 and
7 watch for it.

### 5.3 Reflow (window resize / split) — the #27 path

Unchanged mechanism, new target. After the seek settles, `settle_at`
(`model.rs:987-992`) parks `seek_budget = 0`, `seek_rest = Some(38)`,
`seek_last_cols = Some(38)`. A window reflow moves cols; gate A misses (rest is
38, cols are not), gate B's `ours` test fails for a far jump, the drift candidate
is recorded, and the **second** sighting of the same width re-arms
(`model.rs:1066-1074`). Nothing about that is target-sensitive — but it is the
regression that stranded the bar for a whole release cycle, so §7 step 5
reproduces it live at the new target, and §6.3 adds a hermetic twin at 38.

### 5.4 Repaint cost

Each seek step is one `resize_pane_with_id` (`main.rs:169-184`) → one tab
relayout → one `claude` TUI reflow in the neighbouring pane. What changes:

- **Expand transitions are 8 cols longer.** Collapsed→expanded (Alt+c) and the
  peek-on-nav expansion (`model.rs:349-357`) each travel 34 instead of 26 → at
  most one extra step at a 7-col granularity (5 steps instead of 4). Budget 16.
- **Nothing else.** No new effect kinds, no new triggers, no cross-tab commands.
  The C6 storm classes (rounds 11–13: announce storms, EMFILE) are untouched —
  the seek is self-targeted and render-driven, and this change adds zero
  event traffic.
- **Peek feels marginally slower** to reach full width. Live watch item (§7
  step 4).

---

## 6. Test plan, by tier

Taxonomy row (TESTING.md:112-119): **Visual / UX — widths — human judgement
only — `host-untestable`**, plus **Pure logic / model** for the seek arithmetic
and **Generated artifacts** for the KDL. So: full Tier 1 on the model and the
artifacts, an explicit written argument where Tier 2 would be, and a heavy Tier 3.
Labels: **`host-untestable`** and **`needs-live-validation`**.

### 6.1 The gate

```bash
cargo test --workspace   # --workspace is load-bearing (skips clave-bar otherwise)
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

### 6.2 Tier 1 — existing tests, re-derived

**The trap, stated first.** `30` appears in these tests in *two* different roles:
as **the target** and as **an arbitrary start width**. A mechanical `30 → 38`
replace is wrong and fails loudly in one place and silently in another.

Worked proof, on `seek_waits_for_inflight_resizes_and_zellijs_floor`
(`model.rs:1727-1750`, inventory #21). It runs at the **collapsed** target (4)
and uses 30 → 16 as a start ladder. Replace 30 with 38 and:

- `width_seek(38)` → Shrink, grace, Shrink — still fine;
- `width_seek(16)`: `delta = |38 − 16| = 22 > MAX_LEARNABLE_STEP (20)` → the step
  is **not learned**, so `seek_step` stays at the 8-col pre-learning slack (it was
  14 with a 30-start);
- the floor loop at `:1747-1749` then evaluates `diff.abs() = 12 <= step = 8` →
  **false**, so it falls through to a re-drive and emits `ShrinkSelf` where the
  test asserts `[]`.

That is the whole risk of this change in one example: the settle predicate
(`model.rs:1032`) is a function of a *learned* step, and the learned step is a
function of the literals in the test. **Leave #21 exactly as it is** and add a
comment saying 30 is a start width, not the template.

| # | Test | Verdict at 38 | Action |
|---|---|---|---|
| T1 | `a_newborn_model_seeks_the_template_width` (`:1662`) | passes — 45 → Shrink, 18 → Grow at either target | **strengthen**: add a third case `width_seek(33) == [GrowSelf]`. 33 sits *inside* 30's band (`2·3 ≤ 8` → settle) and *outside* 38's (`2·5 > 8` → Grow), so it is the sentinel that fails loudly if the constant is ever reverted |
| T2 | `seek_collapses_to_the_gutter_despite_coarse_steps` (`:1675`) | **fails** at `:1685` — `width_seek(30)` now emits `GrowSelf` | `30 → 38`. The collapsed loop below it is target-independent |
| T3 | `seek_expands_back_to_template_width` (`:1705`) | assertion at `:1724` targets 30 | `(cols - 38).abs() <= 4`. With the existing step-9 ladder from 5: 5→14→23→32→**41**, settles (`2·3 ≤ 9`), `\|41−38\| = 3 ≤ 4` ✓ — and 41 is *outside* 30's band (11 > 4), so the test still distinguishes the two targets. Rewrite the `:1721-1723` comment accordingly |
| T4 | `seek_waits_for_inflight_resizes_and_zellijs_floor` (`:1727`) | passes **only if left alone** | **do not touch**; add the "30 is a start width" comment above |
| T5 | `idle_seek_re_arms_when_a_relayout_drifts_it_off_target` (`:1752`, **#27's pin**) | passes — collapsed target throughout | unchanged |
| T6 | `drift_is_measured_from_the_rest_width_not_a_stale_emit_anchor` (`:1772`, **#4's pin at `:1773`**) | passes — collapsed target throughout | **byte-unchanged.** Say so explicitly in the PR |
| T7 | `idle_seek_ignores_an_oscillating_layout_and_a_resting_width` (`:1799`) | passes | unchanged |
| T8 | `idle_seek_oscillating_between_rest_and_one_off_target_never_re_arms` (`:1822`) | passes | unchanged |
| T9 | `seek_grows_back_from_an_overshoot` (`:1842`) | **fails** — `width_seek(27)` asserts `[]` but `2·11 > 14` → `GrowSelf` | re-derive: `13 → 21`, `27 → 39`, `13 → 21`. Trace at 38: `width_seek(21)` → Grow (`2·17 > 8`); `width_seek(39)` → learns step 18, `2·1 ≤ 18` → `settle_at(39)`, `[]` — **a genuine overshoot past 38, which the old numbers never were**; `width_seek(21)` → `ours = \|21−39\| = 18 ≤ 18` → settle in place, `[]` ("retired") |
| T10 | `seek_never_learns_an_external_jump_as_the_step_size` (`:1855`) | **fails** — `width_seek(40)` asserts `Shrink` but `2·2 ≤ 8` → settles | re-derive the third value `40 → 55`: `delta = \|15−55\| = 40 > 20` → not learned; `2·17 > 8` → `Shrink` ✓. Discriminating power preserved: with a poisoned step of 60 the band half-width is 30 and `\|55−38\| = 17 ≤ 30` would fake-accept |
| T11 | `seek_budget_caps_a_layout_that_never_converges` (`:1869`) | passes | unchanged |
| T12 | `peek_expands_a_collapsed_bar_and_expiry_sinks_it` (`:1882`) | passes | **cosmetic** `:1897` `30 → 38` — the literal stands for "the width the peek grew to" and should tell the truth |
| T13 | `expanded_bars_ignore_peeks` (`:1900`) | **fails** at `:1906` | `30 → 38` |
| T14 | `toggle_cancels_a_peek_and_a_late_expiry_is_a_noop` (`:1909`) | passes | unchanged |
| T15 | `snapshot_hydrates_a_newborn_into_collapse` (`:1926`) | passes (target 4) | **cosmetic** `:1935` `30 → 38` — "born at template width among gutter bars" |
| T16 | `snapshot_heals_a_desynced_instance_and_leaves_synced_ones_alone` (`:1943`) | **fails** at `:1948` — `converged` is no longer `[]` | `:1948` and `:1955` `30 → 38`. The `synced` half (`:1962-1976`) is collapsed-target and unchanged |

**Five hard failures** (T2, T9, T10, T13, T16), one strengthening (T1), one
re-derived assertion (T3), three cosmetic updates (T12, T15, and T16's second
line), one deliberate no-touch (T4). **Red-first**: change the constant, run
`cargo test --workspace`, and confirm the failure set is exactly
{T2, T9, T10, T13, T16}. Any other failure is a finding.

### 6.3 Tier 1 — new tests

Three, all in `crates/clave-bar/src/model.rs`.

**(a) `targets_are_disjoint_by_more_than_a_learnable_step`** — the §3.5
invariant, so a future width edit cannot silently erode it:

```rust
// The acceptance band is 2*|cols−target| <= seek_step.max(8), and seek_step is
// capped at MAX_LEARNABLE_STEP, so the widest half-band is 10 cols. If the two
// targets were ever closer than a learnable step, a collapse could settle inside
// the expanded band (or vice versa) and Alt+c would become a no-op at some
// window sizes. 38 − 4 = 34 today.
assert!(BAR_TARGET_COLS - COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP);
```

**(b) `idle_seek_re_arms_when_a_relayout_drifts_the_expanded_bar`** — the #27
drift re-arm at the target that actually changed. Today `:1752` only covers the
collapsed target; that is the real coverage gap this change exposes.

```rust
BarModel::default()                    // expanded, target 38, birth-armed
width_seek(45)  == [ShrinkSelf]        // 2·7 > 8
width_seek(37)  == []                  // learns step 8; 2·1 ≤ 8 → settle_at(37)
seek_budget     == 0
width_seek(140) == []                  // far drift: observe only
width_seek(140) == [ShrinkSelf]        // same width twice → confirmed → re-arm
```

**(c) `expanded_drift_is_measured_from_the_rest_width_not_a_stale_emit_anchor`**
— the #4 stale-anchor regression at the expanded target. Constructed so the final
landing is far from the previous *emit*, and the intruding width is within a
learned step of the **stale** anchor but far from **rest**:

```rust
BarModel::default()                    // expanded, target 38
width_seek(80)  == [ShrinkSelf]        // emit @80
width_seek(62)  == [ShrinkSelf]        // learns step 18; 2·24 > 18; emit @62
width_seek(44)  == []                  // 2·6 ≤ 18 → settle_at(44); rest = last = 44
width_seek(75)  == []                  // |75−44| = 31 > 18 → not ours → drift candidate
width_seek(75)  == [ShrinkSelf]        // confirmed → re-arm, seek 38
```

The counterfactual is the whole point: with the pre-#27 stale anchor (62),
`ours = |75 − 62| = 13 ≤ 18` would settle the bar at 75 — parked 37 columns
off-target. The assertion message must say so, exactly as `:1795` does.

### 6.4 Tier 1 — the convergence harness

**Finding: no refactor is required.** The brief allowed for the harness
hardcoding 30; it does not. `SimZellij` / `drive` / `band_half`
(`model.rs:2543-2689`) are width-agnostic, and all six harness tests read the
constants symbolically:

| Test | Target reference | Behaviour at 38 |
|---|---|---|
| `harness_newborn_converges_on_the_template_from_above` (`:2691`) | `BAR_TARGET_COLS` at `:2699-2700` | start 60, step 12 → 60→48→36; `\|36−38\| = 2 ≤ band_half(12) = 6` ✓ |
| `harness_collapse_converges_on_the_gutter` (`:2709`) | `COLLAPSED_TARGET_COLS` at `:2716` | unchanged |
| `harness_floor_above_target_rests_benignly` (`:2722`) | literal floor 12 vs target 4 | unchanged |
| `harness_latency_path_exercises_the_in_flight_guard` (`:2737`) | `BAR_TARGET_COLS` at `:2746` | start 70, step 11 → 70→59→48→37; `\|37−38\| = 1 ≤ 5` ✓ |
| `harness_peek_cycle_expands_then_sinks` (`:2752`) | both, at `:2760,2765,2773` | gutter 6 → peek 6→14→22→30→**38** (exact) → sink back ✓ |
| `harness_toggle_mid_seek_re_aims_at_the_new_target` (`:2779`) | `COLLAPSED_TARGET_COLS` at `:2792` | toggle at segment 2 (cols 12) → 12→5; `\|5−4\| = 1 ≤ 4` ✓; `max_seg ≤ SEEK_BUDGET` ✓ |

Action: **re-run, assert green, and record in the PR that the harness was
already parameterised.** If any of the six goes red, that is the change's most
important finding and it stops here.

`SETTLE_RENDERS = 2` (`model.rs:2595`) — the PR #27 addition that makes
render-driven re-arms observable — is unchanged and still the mechanism that
would catch a stranded bar at 38.

### 6.5 Tier 1 — properties and pinned seeds

`prop_width_seek_converges_or_bounds` (`model.rs:2814-2876`) reads the target
symbolically at `:2863-2867`. **No edit.** But note honestly what moving the
target does to it: the property admits three terminal states —
`within || at_floor || exhausted` (`:2871-2875`) — and `exhausted` is an escape
hatch. With `start ∈ 0..=500` and `step ∈ 1..=20`, a 38-col target is 8 further
from 0 and 8 nearer 500, so the escape rate is roughly neutral — but it is an
escape hatch either way, and this change is the moment to close part of it.

**New property — `prop_seek_makes_progress_when_it_exhausts`:**

```text
∀ start ∈ 0..=500, step ∈ 5..=20, collapsed, latency;  floor = 0, interrupt = None:
    drive to quiescence
    if the run exhausted the budget:
        |end − target| < |start − target|        // strictly closer than it began
    else:
        |end − target| <= step.max(8)/2          // converged (floor is 0, so no floor rest)
```

`step ≥ 5` keeps it to realistic zellij granularity (the ledger's 7–14, plus
slack); `interrupt = None` keeps a `Jump` from moving the goalposts;
`floor = 0` removes the floor-rest terminal state so the assertion is sharp. This
guards the one thing a wider target makes marginally more likely — a budget spent
without progress.

**Pinned regression seeds** — `crates/clave-bar/proptest-regressions/model.txt`:

```text
cc 547c… # start = 500, step = 1, floor = 0, collapsed = false, …, interrupt = Some((1, Jump(25)))
cc 7357… # start =  35, step = 10, floor = 0, collapsed = false, …, latency = true, interrupt = None
cc 4f46… # start = 416, step = 1, floor = 0, collapsed = true,  …, interrupt = Some((5, Jump(88)))
```

proptest replays these before any novel case. **Two are `collapsed = false`, so
they now execute against 38** — that is the mandated seed from PR #27 exercising
the new target for free. **Do not edit this file.** A failure is a finding about
the change, not a fixture to refresh; if one goes red, the correct response is to
reduce it to a unit test and fix the model, exactly as #27 did.

### 6.6 Tier 1 — generated artifacts

| Instrument | Where | What it must show |
|---|---|---|
| percent-not-fixed guardrail | `setup.rs:737-760`, rewritten per §4.5 | all three generators emit `size="19%"` and none emits `size=30`, `size=38`, or `size=<BAR_TARGET_COLS>` |
| real-parser guardrail | `crates/clave/tests/kdl_guardrail.rs:157,164,172,186` | all three layouts still parse under the real zellij-utils 0.44.3 parser. **No edit; re-run** |
| version-pin tripwire | `crates/clave/tests/zellij_pin_tripwire.rs` | unchanged; re-run |
| version coherence (#52) | `crates/clave/tests/` | unchanged; re-run |

### 6.7 Tier 2 — does not exist (#47)

Blocked on #44. The written argument the taxonomy demands (TESTING.md:117,
AGENTS.md:96-98):

> The only cross-process artefact this change touches is the bar pane's birth
> **percent** in three generated layouts. The geometry contract is **one-way**:
> the percent is a birth hint and `BAR_TARGET_COLS`, compiled into the plugin, is
> the sole authority. A version-skewed `clave` (issue #44) emitting an older
> percent costs at most a few resize steps at birth and can never produce a wrong
> resting width — unlike the v0.1.1 mixed-artifact failure (#43), where the
> stale artifact named a *wasm path* and produced two plugin populations. No
> pipe, no store field, and no shellout changes. The seek itself remains
> single-writer per pane, self-targeted, and render-driven (C6 round 20).

An adversarial reviewer must attack that paragraph specifically.

### 6.8 Tier 3

§7, in full. Labels: `host-untestable` + `needs-live-validation`; batched into
the next tag's maintainer pass (#49).

---

## 7. Live validation

Tier 3, `host-untestable`. **The maintainer runs every command; the driving agent
prints them.** Never aim `zellij` at the live `clave` session — only at
`clave-test`, env-scoped, and only after a liveness gate (TESTING.md:231-236: a
`zellij action` against a dead session **blocks forever without erroring**).

Paths are genericized (`$HOME/…`, `$TMPDIR/…`) — the pre-commit PII blocklist
rejects private local paths (AGENTS.md:122-124).

**The measuring instrument.** `dump-layout` serializes geometry as percentages,
not columns, so it cannot answer "is the bar 38 columns?" directly. For steps
2–6 use TESTING.md's instrumentation recipe (`:313-338`): a temporary marker
`eprintln!` in `render()`, a fresh build tag, the **sandbox** wasm only, and a
log read filtered by both. Strip it before committing.

Add, at `crates/clave-bar/src/main.rs`, immediately before `main.rs:534`:

```rust
eprintln!("CLAVE_DBG_s8 cols={cols} collapsed={} peeking={}",
          self.model.collapsed_dbg(), self.model.peeking_dbg());
```

(or simply `eprintln!("CLAVE_DBG_s8 cols={cols}")` if exposing the flags is not
worth a temporary accessor — the cols trace alone answers every question below.)

---

### Step 0 — Pre-flight: version coherence (#44 is unfixed)

**(a) Run, in a non-zellij terminal:**

```bash
command -v clave
clave --version
grep -h 'clave-bar: loaded' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -5
```

**(b) Look at:** the `clave --version` output against the `vX.Y.Z` in the most
recent `clave-bar: loaded vX.Y.Z build=…` line.

**(c) Report:** both strings verbatim, plus the `build=` tag.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| versions match | the fleet is coherent; live readings are trustworthy | Step 1 |
| versions differ | **#43/#44** — every other reading is suspect (dossier, "Read-only live diagnosis") | **stop.** Report it; do not attribute any width observation to S8 until the sidebars agree |
| no `clave-bar: loaded` lines at all | log rotated, or no session has started since boot | proceed to Step 1 (the sandbox pass does not depend on the live session) but say so |
| `command -v clave` resolves to `$HOME/.cargo/bin/clave` **and** a `$HOME/.local/share/clave/` copy exists | the "one leak" configuration (CONTRIBUTING) | note it; it is the reason Step 1 never installs |

---

### Step 1 — Build and stage the sandbox (never touches the real fleet)

**(a) Run, from the S8 worktree:**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

CLAVE_BUILD_TAG=s8-$(date +%m%d-%H%M%S) \
  cargo build -p clave-bar --target wasm32-wasip1 --release
cp target/wasm32-wasip1/release/clave-bar.wasm \
   "$HOME/.local/state/clave-dev/data/clave-bar.wasm"
```

**(b) Look at:** the gate is green, and the `cp` target is the **sandbox** data
dir. Note the build tag — every later log read filters on it.

**(c) Report:** the gate result and the build tag string.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| green, tag recorded | staged | Step 2 |
| a `model.rs` seek test failed | a §6.2 re-derivation is wrong, or the change broke the seek | **stop.** Which test, and its actual-vs-expected effect list |
| a pinned proptest seed failed | **a real finding** (§6.5) — the new target broke a case #27 pinned | **stop.** Reduce to a unit test before any live work |
| the `cp` path does not exist | `just dev-install` has never run on this machine | run it **only if the real session is not live** (AGENTS.md); otherwise `mkdir -p` and proceed |

> Never run `just release`, `cargo install`, or `just dev-install` while the real
> fleet may be up — that is what broke production (#43).

---

### Step 2 — Sandbox fresh launch: birth geometry

**(a) Run:**

```bash
# agent-safe: uses the worktree's debug binary, never $HOME/.cargo/bin
cargo run -q -p clave -- dev reset
cargo run -q -p clave -- dev scenario c8-cold-start
```

then, **in a non-zellij terminal** (this is the maintainer's, it execs zellij):

```bash
cargo run -q -p clave -- dev launch
```

then, from anywhere:

```bash
grep -h "CLAVE_DBG_s8\|clave-bar: loaded" \
  "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -40
```

**(b) Look at:** the sidebar in the `clave-test` session — is it visibly wider
than usual? — and the last `CLAVE_DBG_s8 cols=` value per instance after the
session settles.

**(c) Report:** the final `cols=` values, how many `CLAVE_DBG_s8` lines appeared
per instance before it went quiet (that is the step count), and whether the
widening was visible as a flicker.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| final `cols` ∈ 34..42, ≤ 3 lines per instance | born near target, converged — §3.4 working | Step 3 |
| final `cols` ≈ 38 with **zero** steps | the 19 % birth landed on target — the best case | Step 3 |
| final `cols` ∈ 34..42 but **> 6** lines per instance | converged, but the birth percent is badly matched to this window | Step 3; report the window width so §3.4's reference viewport can be re-tuned |
| final `cols` far from 38 and the trace **stopped** | **stranded** — the #4 class at the new target | **stop.** Full trace, the window width, and whether a terminal resize preceded it |
| the trace never goes quiet (lines keep arriving) | a seek storm — the C6 round 11/13 class | **stop immediately**, `Alt+c` to change the target and break the loop, then report the last 60 lines |
| Alt+c is dead / the pane flashes "FIXED!" | a fixed `size=` leaked into a generated layout — §4.5's negative assertion missed a path | **stop.** Report, and paste `$HOME/.local/state/clave-dev/data/layout.kdl` |

---

### Step 3 — Toggle cycle (collapse / expand)

**(a) Keystrokes, in the `clave-test` session:** `Alt+c`, wait ~2 s, `Alt+c`,
wait ~2 s. Repeat once rapidly (two `Alt+c` within a second). Then:

```bash
grep -h "CLAVE_DBG_s8" "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -60
```

**(b) Look at:** every bar collapses together and expands together; the collapsed
width looks the same as it always has; the expanded width is the new one.

**(c) Report:** the resting `cols` after each of the four transitions, whether
all instances moved together, and whether the rapid double-toggle left anything
inconsistent.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| collapse rests ~4–14, expand rests 34–42, all together | correct. The collapsed floor is window-dependent by ruling (C8 `:585-590`) | Step 4 |
| collapse rests **wider than before this change** | `COLLAPSED_TARGET_COLS` was touched, or the band is swallowing the target | **stop.** §3.5 says collapsed is unchanged; report both widths |
| one bar stays at a different width | **collapse parity desync** (C8 `:592-606`) or a mixed wasm population (§5.2) | note it; check whether that tab existed before the reload. Not a blocker if it heals on recreate |
| expand takes visibly longer than it used to | expected — 8 more columns, ≤ 1 extra step (§5.4) | Step 4; report only if it reads as *sluggish*, which is a live finding worth the extra step count |

---

### Step 4 — Peek-on-nav

**(a) Keystrokes:** `Alt+c` to collapse. Then `Alt+↓` / `Alt+↑` a few times.
Wait ~1.5 s without touching anything. Then a burst of 5 rapid navs.

**(b) Look at:** the bar expands on nav to the new full width, and sinks back to
the gutter ~0.9 s after the last press. A burst stays expanded until the burst
ends.

**(c) Report:** whether the peek reaches full width before it sinks, and whether
the burst stayed expanded throughout.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| expands fully, sinks once, burst holds | correct — the peek retargets automatically via `model.rs:1022-1026` | Step 5 |
| peek starts sinking **before** reaching full width | 34 cols of travel does not fit inside 0.9 s at this granularity — a genuine S8 side effect | report the step count; the fix is `PEEK_SINK_SECS`, and it is a **new issue**, not a blocker |
| peek expands and never sinks | a peek/timer bug — unrelated to width, but report it (C6 round 21 machinery) | report |

---

### Step 5 — The #27 drift repro: reflow must **re-arm**, not strand

This is the regression that stranded the bar for a release cycle. It is the most
important live step.

**(a) With the sandbox session settled at the new width, resize the terminal
window** — drag the window edge, or change the font size — by a large amount
(halve or double the width). Wait ~2 s. Then:

```bash
grep -h "CLAVE_DBG_s8" "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -40
```

**(b) Look at:** the bar drifts with the reflow (percent geometry), then returns
to the new target within a second or two. In the trace: an off-target `cols`
appearing **twice** (the drift confirmation, `model.rs:1066-1073`), then resize
steps, then quiet.

**(c) Report:** the `cols` sequence from the reflow to quiescence, and the final
value.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| off-target twice → steps → rests 34–42 | **#27 holds at 38.** The drift re-arm is intact | Step 6 |
| drifts and **stays** off-target, trace quiet | **#4/#27 reborn at the new target** — the exact failure this spec exists to prevent | **stop.** Full trace + the before/after window widths. Reduce to a `SimZellij` case before touching anything |
| never settles — steps keep arriving | a re-seek loop; the drift confirmation is not holding | **stop.** `Alt+c` to break it, then report |
| bar fights you *while dragging* | mid-drag re-seek — PR #27's stated watch item, at a longer travel | report the trace; if it is a burst of ≤ `SEEK_BUDGET` refused resizes it is the documented accepted behaviour |
| a brief burst of refused resizes before settling | documented in PR #27 ("stuck far-drift spends a bounded burst") — watch for *visible flicker* | report whether it was visible; that is a UX judgement, not a correctness one |

---

### Step 6 — The #4 stale-anchor case: a *small* nudge

Step 5's large reflow exercises the far-drift path. #4 was subtler: a relayout
landing **within a learned step of a stale emit anchor** but far from the true
rest width. The live analogue is a *small* nudge after a *coarse* convergence.

**(a) Collapse and expand once (`Alt+c` twice) so the seek converges in coarse
steps and comes to rest. Then nudge the terminal window narrower by roughly
10 columns** — a small drag, not a halving. Wait ~2 s. Then read the trace as in
Step 5.

**(b) Look at:** whether the bar returns to the target or parks at the nudged
width.

**(c) Report:** the `cols` sequence and the final resting value.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| returns to 34–42 | the rest-anchored drift test (`settle_at`, `model.rs:987-992`) holds at 38 | Step 7 |
| parks at the nudged width, trace quiet | **the #4 stale-anchor regression at the expanded target** | **stop.** This is the highest-severity outcome available. Full trace; reduce to the §6.3(c) shape with the observed numbers |
| the nudge is inside the acceptance band and nothing happens | correct and expected — the band is ±4 pre-learning, up to ±10 learned (`model.rs:1030-1032`) | Step 7; if the nudge was < 10 columns this branch is the likely one, so try a ~15-column nudge to disambiguate |

---

### Step 7 — Cross to the real fleet (maintainer's choice, at a tag)

Nothing above touched the real session. Getting 38 into the real fleet requires
installing this build — `just release`, which is **the maintainer's act at a
tag** (AGENTS.md:45) and belongs to the #49 batch, not to this PR's merge gate.

**(a) After a release and a session recreate, in the real `clave` session:**
re-run Step 0's version check, then just use the fleet for a working session.
Optionally:

```bash
clave ls --json | jq '{seq, agents: [.agents[] | {label, status, tab_id}]}'
```

**(b) Look at:** (i) the sidebar width against real rows; (ii) every tab's bar the
same width; (iii) `claude` panes reflowed sanely at −8 columns; (iv) no repaint
storms during ordinary work.

**(c) Report:** whether any tab's bar differs from the others, whether any
`claude` pane rendering broke, and whether the narrower agent panes are annoying
in practice.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| all bars equal, panes fine | shipped | Step 8 |
| some bars 30, some 38 | **mixed wasm population** (§5.2) — expected between a reload and a recreate | recreate the session; if it persists, it is a finding |
| a `claude` pane renders badly at the narrower width | the accepted cost, biting | Step 8 — this is exactly the judgement input |
| repaint storms during ordinary work | the C6 round 11/13 class, which this change should not be able to cause | **stop and revert the constant.** Full zellij log for the window |

---

### Step 8 — The judgement: is 38 actually right?

38 was chosen from a mock-up. This step is the only thing that can confirm it,
and it can be run **before** any release — it needs no session at all.

**(a) Run, in any terminal (read-only; `read_store` is lock-free-safe — writers
use temp+atomic-rename, `store.rs:120-129`):**

```bash
printf '%-28s | %-36s\n' 'AT 30 COLS (budget 26)' 'AT 38 COLS (budget 34)'
clave ls --json | jq -r '.agents[] | .label
  | [ (.[0:26]), (.[0:34]) ] | @tsv' \
  | awk -F'\t' '{ printf "%-28s | %-36s\n", $1, $2 }'
```

(`jq` slices by codepoint, so multibyte labels are handled. The comparison is
plain truncation — S4's give-way policy is not in this build — which makes it a
*lower* bound on how much 38 buys.)

**(b) Look at:** for each real row, does the 34-cell column keep the part that
tells it apart from its neighbours, and does the 26-cell column lose it?

**(c) Report:** how many rows become distinguishable at 34 that were not at 26;
whether any row still truncates badly at 34; and the honest verdict on trading
8 columns of agent pane for it.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| most rows distinguishable at 34, agent panes acceptable | **38 confirmed.** Record the verdict in the C6 round-22 ledger entry | done |
| still truncating at 34 | 38 is not enough. Do **not** widen further blind — S4's give-way policy (§3.4 of that spec) is the higher-leverage lever, and #24 item 3 is the parent | file against #24 item 6 with the row lengths that failed |
| distinguishable at 34, but the agent panes now feel cramped | 38 is too generous | try 34 or 36: one constant (§4.1), one percent (17 %), and the §6.2 test values. Cheap by design |
| the difference is marginal either way | the real defect is label *composition*, not width | say so — that is S4's territory, and it changes the priority of this workstream |

---

### Step 9 — Regression sweep on the real fleet

**(a) Ordinary use for one working session: `Alt+↑`/`Alt+↓` nav, `Alt+o`,
`Alt+w` to close a tab, `Alt+a` to add one, `Alt+c` twice.**

**(b) Look at:** nav still lands where expected after a close (#23); a new tab's
bar is born at the new width; no duplicate sidebars (#43).

**(c) Report:** anything that behaves differently from before the width change.

**(d) Branch:**

| Report | Conclusion | Next |
|---|---|---|
| nothing differs but the width | clean | close out; update the ledger verdict |
| a new tab's bar is born at the old width | `add::tab_node` (`add.rs:108`) was missed, or a stale `clave` served `clave add` (#44) | check `command -v clave` (Step 0) before blaming the change |
| two sidebars in one tab | **#43** — a mixed-version artifact, not this change | Step 0's version check, then `clave doctor` |
| nav breaks after a close | **#23 / S0 territory**, not S8 — geometry does not touch the beacon | report against S0 |

---

## 8. Coordination with S4 / S5 / S6

**The interface S8 provides, stated as a contract:**

> `BAR_TARGET_COLS` is the sole authority for the *pane's* width and is read by
> exactly one function — `width_seek`'s target selector (`model.rs:1022-1026`).
> **No renderer may read it.** The render path receives `cols` as a parameter
> from zellij (`main.rs:525`); every text budget is a function of that runtime
> value. S8 changes what `cols` typically *is*; it changes nothing about how a
> renderer derives a budget from it.

Confirmed against the sibling specs as written:

- **S4** already states the rule (`…-S4-…:40-42`: *"The width budget is therefore
  a **parameter**, not `cols - 3`"*) and its API is
  `fit_label_str(name: &str, budget: usize)` (`…-S4-…:495`) — width-independent by
  construction. Its `§5.1` test table uses literal budgets, not the constant. ✓
- **S5** computes `budget = cols.saturating_sub(GUTTER_COLS + RIGHT_MARGIN_COLS)`
  inside `compose_row(&Row, cols)` (`…-S5-…:731,750`) and lists widths as
  out-of-scope, *"it must not change `BAR_TARGET_COLS` or
  `COLLAPSED_TARGET_COLS`"* (`…-S5-…:1229-1230`). ✓ S8 is the workstream that
  does, and S5's `main.rs:546` clamp is S5's line to replace, not S8's.
- **S6** owns `GUTTER_COLS` (2 → 3). S8 asserts only that the gutter width remains
  a constant subtracted from `cols`. The arithmetic in §1 and §3.1 assumes
  `budget = cols − 4` post-S6; if S6 lands a different gutter, only the *sizing
  rationale* moves, not the constant.

**The one rule for S4/S5/S6:** pin **30 and 38 as literal budget inputs** in
tests. Do **not** import `BAR_TARGET_COLS` into a renderer test — that would make
those tests re-derive themselves the next time this constant moves, which is the
exact coupling this contract exists to prevent.

### Sequencing

**S8 is independent of S0/S1/S2/S3 and runs in parallel with all of them.** They
touch ordering, binding, and event plumbing; S8 touches geometry. No shared
function, no shared effect, no shared store field.

Files S8 shares with another workstream, and the collision surface:

| File | S8 touches | Also edited by | Collision |
|---|---|---|---|
| `crates/clave-bar/src/model.rs` | `:133-142` (constants) + the seek tests `:1662-1977`, `:2691-2876` | S0 (`bind_effects :414-439`), S1/S3 (`sort_key :391-393`, `rows() :728-793`, `apply_tabs :593-715`), S4 (`fit_label`, new), S5 (`Row`, `compose_row`, new) | **disjoint hunks.** Only the constants block and the seek tests. Whoever lands second rebases a 10-line header |
| `crates/clave-bar/src/main.rs` | **nothing** — the `eprintln!` in §7 is temporary and stripped | S0 (`:43-71`, `:87-231`, `:434-467`), S5 (`render :536-559`) | none |
| `crates/clave-types/src/lib.rs` | three new `pub const`s, appended | S4 (label fields on `Agent`), S5 (the palette) | **append-only**, trivial |
| `crates/clave/src/setup.rs`, `crates/clave/src/add.rs` | the three `size=` sites + one test | S4 touches `add.rs` label composition (`:699-711`), not the layout strings | different functions |
| `SUBSYSTEM-VALIDATION.md` | appends a C6 round-22 entry | any workstream may append | append-only |

---

## 9. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| **38 columns is too wide on a narrow window.** At 80 cols the bar is 47 % of the screen. The seek has no viewport awareness — `render(_rows, cols)` gives only own cols, and `PaneMeta` (`model.rs:25-31`) drops `PaneInfo.pane_columns` at `main.rs:452-466` | medium if the maintainer ever drives a laptop under ~100 cols | out of scope by decision (§3.6). The fix, if it becomes real, is a clamp `min(BAR_TARGET_COLS, total/4)` requiring `PaneMeta` to carry columns and a per-tab sum — which makes the target a function of a possibly-stale frame (the RC-A class, applied to geometry). File it if step 7 or 8 surfaces it |
| **A missed width site.** The failure mode of this whole change | high | §2 is the inventory; §6.2 states the failure set to expect red-first; §6.6 pins the artifacts. The specific trap (30 as start width vs 30 as target) is proved out in §6.2 |
| **A pinned proptest seed goes red at 38** | high if it happens | §6.5: it is a finding, never a fixture refresh. Reduce to a unit test, red-first, before proceeding |
| **Property 1 weakens** — more cases terminate via the `exhausted` escape hatch (`model.rs:2870-2875`) | low | the new `prop_seek_makes_progress_when_it_exhausts` (§6.5) closes the "budget spent going nowhere" half of it |
| **Mixed wasm populations after a hot-reload** — some bars 30, some 38 | low, cosmetic, self-healing | §5.2; live steps 3 and 7 watch for it; heals on session recreate |
| **Birth-percent skew from a stale `clave`** (#44 unfixed) | low, benign by construction | §3.3, §6.7: the percent is a hint, the seek is the authority. ≤ 3 extra steps at birth, never a wrong resting width |
| **Longer expand travel makes the peek feel sluggish** | low | live step 4; the lever is `PEEK_SINK_SECS`, a separate issue if it bites |
| **Rebase churn in `model.rs`** against S0/S1/S3/S4/S5 | low | §8's table; S8's hunks are the constants header and the seek test block |
| **Extra resize steps read as flicker at session launch** | low | §3.4 chose 19 % specifically to make the common case zero-step; live step 2 measures the step count |

## 10. Out of scope

- **#24 item 7, the collapsed-state design pass** — what 4 columns can actually
  distinguish (glyph + repo colour + battery). S8 keeps `COLLAPSED_TARGET_COLS`
  at 4 and explicitly does not redesign it (§3.5). S5 already renders a tinted
  `…` there for free (`…-S5-…:905`).
- **Any change to the seek algorithm.** Every gate, bound, and comment in
  `width_seek` (`model.rs:1019-1136`), `arm_seek` (`:933-939`) and `settle_at`
  (`:987-992`) is untouched. This spec moves a number and re-derives the tests
  that depend on it.
- **A window-relative or user-configurable width** — §3.2, §3.6.
- **The `cols <= 2` off-by-one** in the render clamp (`main.rs:546-553`) —
  pre-existing, inherited by S5, deliberately not fixed here.
- **Label composition and truncation policy** — S4 (`fit_label`) and S6 (the
  gutter). S8 provides columns; it does not decide what goes in them.
- **`clave ls` / picker widths** — no width logic exists there (§2.4 #17) and
  none is added.
- **Issue #44 (`PATH`-resolved shellouts) and #47 (Tier 2)** — S8 reasons around
  both (§6.7, §7 step 0) and fixes neither.
