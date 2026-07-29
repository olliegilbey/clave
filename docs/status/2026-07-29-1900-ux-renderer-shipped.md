# Status — the sidebar renderer is built, reviewed, and waiting on a merge

_2026-07-29 · branch `ux` @ `ed3d56d`, 35 commits ahead of `main`, gates green,
271 tests · **PR #86 is open, MERGEABLE, and blocked only on Ollie's approval**_

## Read this first

**`docs/ux/LEDGER.md` is the authority, not this file.** It carries 27 numbered
decisions with their reasoning and overrides any spec. This handoff is the
orientation; the ledger is the record. Read the ledger's **operating rule**, then
its **task table** (the only statement of what has shipped), then D19–D27.

Runnable target render — **look at this before designing anything**:

```bash
cargo run -p clave-bar --example bar-preview
```

## Your role

You are the **standing coordinator and principal engineer for the UX
workstream**, not the executor of a plan. Subagents implement; you brief them,
review each result, absorb what it discovers, and brief the next. Ollie endorsed
Opus for subagent work.

**The operating rule that broke a four-session loop, and must not be relaxed:**

> **Specs are an OUTPUT, not an input.** Nothing under `docs/superpowers/specs/`
> gets amended during the build. Discoveries land in the ledger. Subagents **may
> read** the specs; your overrides travel in their brief.

## State

- `main` @ `48d21aa`. `ux` is 35 commits ahead. **`main` is protected and you
  cannot merge — Ollie does.**
- Gate 1 happened: Ollie ran the fleet live and ruled **keep**.
- Two PR threads are **deliberately unresolved**: 54 columns, and
  `transcript_path` (filed as **#87**). Resolving them would imply done rather
  than sequenced.
- **`CodeRabbit` reports `pass` while rate-limited** (FOOTGUNS, #68). It did
  exactly this on #86. **Read the check detail, not the colour.**

## What shipped

A pure, host-testable renderer (`crates/clave-bar/src/render.rs`) wired to the
store, with the preview (`examples/bar-preview.rs`) driven by the same function —
so a code change that moves a column moves the picture. The hook persists
`title` and `summary`. Widths live once, in `clave-types`.

## The S-specs are now ACTIVELY WRONG. Do not trust them.

This is the most important thing on this page. The operating rule says specs are
an output — the corollary nobody wrote down is that **the inputs have gone stale
in ways that will mislead you**, and three of them were falsified *by
measurement* this session:

| Spec claim | Reality |
|---|---|
| S4/§6.4's summary tier earns the label from `{"type":"summary"}` | **Extinct.** 0 of 153 real transcripts. `ai-title` (74/153) is what Claude writes. (D23) |
| Every spec calls `ai-title` "rolling" | **It is not.** Up to 85 lines per transcript, never more than one distinct value. (D24) |
| S8 §3.6: "the plugin has no viewport width" | **False.** `TabInfo.display_area_columns`, `get_tab_info()`, and `PaneInfo.pane_columns` — already in the manifest. (D20) |
| Design-lock §3: collapsed must be `< 24` | **A restatement, not a bound.** The real one is `< 34`. (D15) |

Plus the whole "known-stale spec content" list in the ledger.

**Queued decision, and it is Ollie's:** the operating rule ends *"when the UX is
real, specs get written from what exists — or deleted."* The UX is now real.
Someone has to decide, per spec, reconcile-or-delete. **Do not start that by
amending them** — that is the treadmill this workstream escaped. Propose the
disposition, get a ruling, then execute.

## Open decisions for Ollie

1. ~~Does this ship to his daily driver, and when?~~ **Ruled — see D28.** Merge,
   then a live **interaction** test, then confidence in the testing itself, then
   `just release` and switch the daily driver. His hands for the last one.
   **Consequence: #63 and S5 land AFTER a release that ships 44** — deliberate,
   so the validated design reaches his daily driver rather than waiting behind
   more change.
2. **Reconcile-or-delete, per S-spec** (above). Still open, and still his.
3. ~~The two open PR threads~~ — **both resolved**, each with a reply pointing at
   where the work is tracked: 54 columns is **#63** (retitled, carrying the full
   arithmetic and D26's inherited reservations), `transcript_path` is **#87**.

## Known-unverified — do not assume these are fine

- **`just mutants` (the `--in-diff` path) has never run end-to-end here.** The
  branch diff is ~3,940 changed Rust lines and would run for hours. The diff
  plumbing was verified in isolation; the combination was not.
- **Steps above `MAX_LEARNABLE_STEP` livelock** in a re-arm/resize storm — 50,576
  configurations on `main`, 111,788 on `ux`. Needs a display around 400 columns;
  Ollie runs ~280, so out of reach today, **not forever**. The proptest generator
  stops at 20, so nothing would ever catch it.
- **Collapse may rest wider than 30**, and can rest as low as 14 — inside its own
  clipping regime. Only a live look settles it. (D26, Gate 1 list.)
- **Two live mutants survive in `Rgb::hex`** — its only caller is the excluded
  preview example.

**The session's scratchpad reports, briefs and review packages are gone.** Their
content was extracted into the ledger and `docs/dev/TESTING.md` deliberately —
keeping process artifacts whose content is already extracted is the same instinct
that grew 6,632 lines of spec. **Do not go looking for them.**

## Next, in order

1. **Expanded 44 → 54** (D19, banded `NOT YET IMPLEMENTED`). The arithmetic,
   spelled out because the coordinator got it wrong once and a subagent caught it
   by deriving instead of trusting the brief:

   ```
   fixed overhead = 13   (1 cap + 8 gutter + 2 separators + 1 margin + 1 cap)
   summary        = cols - 13 - title - repo
   expanded  54 = 13 + 9 title + 7 repo + 25 summary
   collapsed 30 = 13 + 7 title + 3 repo +  7 summary   (unchanged)
   ```

   **Derive every number; do not restate one.** Then the birth percent needs
   re-deriving too — 22% was for 44.

   **It inherits D26's four reservations** — read D26 before starting. At 54/30
   the separation is 24, above the 20-column maximum step, so three become dead
   paths — but **the widened property-test clause must be re-tightened**, and the
   threshold should be `> separation`, not `>=` (the tight half-band still holds
   at exactly 14).
2. **The width simplifications** (D20). The seek *learns* a step it can already
   read: `TabInfo.display_area_columns` exists, `get_tab_info()` is synchronous,
   and `PaneInfo.pane_columns` is already in the manifest `main.rs` **drops**.
   Birth is hand-derived against a fictional 200-column viewport — computing it
   from the real terminal width removes the jank Ollie saw. S8 §3.6's "the plugin
   has no viewport width" is **false**.
3. **S5 — store-backed ink allocation.** Ollie's live complaint: a title's colour
   must lock to its name. Today's allocator is provisional, in-memory and
   positional, and every repo's *first* title gets index 0 — which is why most
   chips were blue. Allocate **globally**, not per-repo.
4. **S4** — the title/summary machine proper, including #87.

## Traps this session paid for

- **`{"type":"summary"}` is extinct** (D23) — 0 of 153 real transcripts. An
  entire feature tier was dead in the field while its tests passed on
  hand-written fixtures. **Measure against real data, not fixtures.**
- **`ai-title` does not roll** (D24). Every spec calls it rolling. It is not.
- **A fixture is not a specification.** The provenance guard tested `""` because
  the *test builder* wrote `""`; the real writer writes `"-"`.
- **Six shapes of green-and-worthless test** — now in `docs/dev/TESTING.md`, with
  `just mutants` / `just mutants-file` to catch four of them. Its first real run
  found two live survivors in `Rgb::hex`.
- **`cargo-mutants` needs `--workspace` and has no config key for it** — without
  it, `--list` reports **0 mutants and exits 0**. Same shape as bare
  `cargo test`.
- **Verify that a scenario can produce the state you intend to observe.**
  `just sandbox` runs `dev reset` first, so running it twice is two *cold* seeds
  and proves nothing about re-runnability. The real path is `dev scenario` twice.

## Rules that are not negotiable

- **Never kill, launch, or run a bare `zellij` command**, never `just
  dev-install` / `just release` / `dev launch`, never write
  `~/.local/state/clave/` or anything under `~/.claude/`. `just sandbox` is
  yours to run; the session lifecycle is Ollie's — **print, never run**.
- **`cargo test --workspace`, always.** Bare `cargo test` skips 68 tests and
  exits 0. Never pipe cargo into grep without `set -o pipefail`.
- **GLYPH RULE (design-lock §5.4):** every non-ASCII glyph in Rust source and
  test literals is a `\u{...}` escape. Literal glyphs were lost in transit twice.
- **Ollie signs every commit** — `git commit` pauses on a 1Password prompt. Wait
  for it. Never `--no-gpg-sign`. If it times out and something else signs it,
  **flag it before pushing**; that happened once here and was re-signed.
- **The repo is PUBLIC.** No home-directory paths, transcript content or personal
  data. The pre-commit PII hook does not cover `gh`.
- **Fix review findings and reply before resolving. Never silent-resolve.**

## What worked, and is worth repeating

Subagent-driven development with a **review after every task**, and an
adversarial review of anything touching the width seek. Every review round found
something real. Three times a subagent **corrected the coordinator's own brief** —
including proving a prescribed fix would oscillate to budget exhaustion, and that
a prescribed verification would have proven nothing. **Brief them to report
BLOCKED rather than guess, and mean it.**

Decisions Ollie should make get **rendered, not argued**. D17 and D18 were both
settled in one look at three candidates, and D18 was a bug no test could have
caught.
