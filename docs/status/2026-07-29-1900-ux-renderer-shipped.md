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

## Next, in order

1. **Expanded 44 → 54** (D19, banded `NOT YET IMPLEMENTED`). Title 7 → 9, the
   rest to summary. **It inherits D26's four reservations** — read D26 before
   starting; at 54/30 three of them become dead paths, but **the widened
   property-test clause must be re-tightened**, and the threshold should be
   `> separation`, not `>=`.
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
