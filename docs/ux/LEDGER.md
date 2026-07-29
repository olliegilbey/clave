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

Runnable target render: `python3 docs/superpowers/specs/bar-preview.py`.

## State

- Branch: `ux`, cut from `main` @ `48d21aa`. `main` receives milestones only.
- Known-good fallback: `b00edd3` (gates green, 222 tests, sandbox-validated).
- `main` is protected; the coordinator cannot merge. Ollie merges.

## Decisions

Numbered, dated, and durable. A decision here overrides any spec that disagrees.

### D1 — `oniViolet`'s 4.67 contrast is accepted (2026-07-29)

S5 states a ≥5.0 band; `oniViolet` measures 4.67. **Accepted as-is.** Zellij
theme import is coming, at which point palette hues stop being ours to choose.
Not worth a substitution round now. *(Ollie, 2026-07-29.)*

### D2 — 44 columns is the expanded target (2026-07-29)

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

- **Collapsed geometry is NOT ratified** (lock §3). Constraint: with 44
  expanded, the collapsed target must be **< 24** for the separation invariant
  `BAR_TARGET_COLS − COLLAPSED_TARGET_COLS > MAX_LEARNABLE_STEP (20)`. Also
  unresolved: truncate the whole label vs render field 0 only (title, falling
  back to repo) — field-0-only read better. **Deferred until expanded is real.**
- **`spawn_mode` orphans relocated sessions** — it derives the transcript path
  from the frozen `rec.cwd`, so a relocated session resumes as a *new* one and
  silently loses history. Needs filing as an issue.
- **Issue #63 says "30 → 38 columns"** against D2's 44. Needs amending.
- **Repo/title ink allocation** is store-backed iterate-and-wrap, not hashed
  (lock §4). That is cross-process state and owes an ordering/idempotency
  argument. Not yet built — the renderer takes inks as input.
- **Pinning (#80) is coming.** Do not design it out; build nothing for it yet.

## Task progress

| Task | Status | Commits | Notes |
|---|---|---|---|
| 1 — the pure 44-column renderer | not started | — | |
