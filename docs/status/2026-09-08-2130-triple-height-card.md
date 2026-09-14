# Four-line card — design locked, port not started

## Orientation

- The four-line card geometry is **ratified and written down**, but **nothing ships it**. `crates/clave-bar/src/card.rs` still renders two lines and is untouched this session. | Written and gated this session | `git diff --stat HEAD` shows only `FOOTGUNS.md` modified; `card.rs` is absent from it — **Checked**
- The lock is `docs/superpowers/specs/2026-09-08-triple-height-card-lock.md`, and it is **authoritative over the preview**, which is the reverse of the double-height arrangement. `double-preview.rs` renders through `render_rows` so it cannot drift; `triple-preview.rs` carries its own copy of the geometry and **can**. | Stated in both files' headers | `head -20 crates/clave-bar/examples/triple-preview.rs` — **Checked**
- `RowHeight` is **launch-baked into the Zellij KDL artifacts**, so adding a variant is not a render-layer change. `clave rows <mode>` writes the store *and then reruns `setup::run_setup()`* in the same breath, because `config.kdl` goes stale the instant the store write lands and a relaunch would otherwise spawn a second bar. | Read this session; the reasoning is a review finding from #232 | `crates/clave/src/main.rs:660-690` — **Checked**
- `RowHeight::from_config_value` falls through to `Double` for **any** unrecognised string, so the default is a fallthrough arm, not a `Default` impl alone. Making the four-line card the default means changing that arm, the `Default` impl, and the clap `value_parser` in `main.rs:668-674` together. | Read this session | `grep -n "from_config_value" -A 8 crates/clave-types/src/lib.rs` — **Checked**
- All three **new** card glyphs are in the installed JetBrainsMono Nerd Font Mono — `fa-tree` U+F1BB, `md-source_branch` U+F062C, `md-robot_happy_outline` U+F171A. The SVG generator gets them for free, and once the worktree mark moves off the Bamum letter, **`CLAVE_ASSET_FONT_BAMUM` and the whole Noto Sans Bamum dependency can be deleted** from `readme-assets.rs`. | Enumerated the real `cmap` with fonttools | `/tmp/.fttmp/bin/python /tmp/cov.py` (recreate the script from § Proof if `/tmp` is gone) — **Checked**
- **Four of the six spinner frames are missing from JetBrainsMono Nerd Font**, and `readme-assets.rs` **panics rather than dropping a glyph**. U+2722, U+2733, U+273B and U+273D are absent; only U+00B7 and U+2736 are present. Menlo carries all six. | Same cmap sweep | as above — **Checked**
- The AI spec that started this work is **not in this worktree**. It lives untracked in the main checkout at `/Users/olliegilbey/code/clave/docs/superpowers/specs/2026-09-05-ai-in-clave-design.md`. Same for the recent `docs/status/2026-09-*` files. | Tripped over it this session | `ls docs/superpowers/specs/ | tail -4` in each of the two directories — **Checked**
- Whether `ttf_parser` in `readme-assets.rs` can load **Menlo.ttc** (a font *collection*) is unverified. `Font::face()` parses at `fontNumber: 0`, which is the right shape for a collection, but no one has run it. This decides whether the hero frame can show a mid-animation spinner or must show a static mark. | Not attempted | Point `CLAVE_ASSET_FONT_EXTRA` at `/System/Library/Fonts/Menlo.ttc` and run the generator — **Open**
- Whether 12 four-line cards actually fit the maintainer's iPad in sidecar is a stated assumption, not a measurement. The maintainer accepted the trade in the abstract. | Maintainer's own estimate of the working ceiling | Only a live sandbox on that display settles it — **Open**

**Method.**

- Instrument the width assertion before re-deriving the budget arithmetic; two rounds were lost to paper reasoning about an off-by-one that a single `eprintln!` of `INDENT`/`budget` named instantly.
- Enumerate the installed font's `cmap` before committing any glyph; three candidates from published cheat sheets were wrong this session, and one of them is already shipping.
- Re-render and look, rather than reasoning about the picture — every ruling in the lock was made from `cargo run -p clave-bar --example triple-preview`, never from prose.

**Proof.** `just gates` (all four green as of this writing: 354 + 256 + 11 + 15 + 25 + 1 tests, wasm builds, fmt and clippy clean). The design itself: `cargo run -p clave-bar --example triple-preview` — green is both profiles rendering with the internal width assertion silent, EXPANDED at 48 columns and COLLAPSED at 16. Add `-- --animate` to see the spinner move, which is the one thing a static render cannot confirm.

## Task Overview

The clave sidebar card grows from two lines to **four** — three of content plus a blank separator line carrying a hairline "shadow" — and the collapsed profile shrinks from 38 columns to 16.

**This new height becomes the default.** `RowHeight::Double` stops being what a fresh install draws.

Success is the ratified geometry rendering from `card.rs` through `render_rows`, goldens pinning both profiles cell for cell, the README's SVG assets regenerated from the new card, and `clave rows` offering the new mode as its default.

Constraints the maintainer set explicitly:

- No cost or dollar figure on the card.
- Collapsed is a strict **left-crop** of expanded — never a re-arrangement.
- The two clocks never sit adjacent.
- Blank is the meaning: an absent value renders an empty cell, never an invented one.

## Reference Docs

- **`docs/superpowers/specs/2026-09-08-triple-height-card-lock.md`** — the lock, written this session. Authoritative for every number and ruling. §1–§3 are the per-cell budgets, §4 the ten ratified decisions with reasoning, §5 lines 3–4 plus the crop rule, §8 the eleven rejected paths, **§9 exactly what the port costs and which three cells have no data behind them yet**. Read §9 first.
- `docs/superpowers/specs/2026-08-26-double-height-card-lock.md` — the lock this one revises. Lines 140-166 (rejected paths, and what the 2026-07-25 lock still governs) are the parts not superseded.
- `/Users/olliegilbey/code/clave/docs/superpowers/specs/2026-09-05-ai-in-clave-design.md` — **outside this worktree.** The AI spec behind the `wants` cell and the scribe: free-tier transcript signals, the scribe's measured cost ($0.0033 / 2.4s per fleet sweep), and the ruled decisions on riding the subscription.
- `crates/clave-bar/examples/triple-preview.rs` — the design, runnable. Its module header restates the rulings in the order they were made.
- `crates/clave-bar/examples/double-preview.rs` — **the shape the port ends in.** Its header states the rule plainly: the preview holds the geometry only while it is argued about, then the geometry moves into `card.rs` and the preview is rewritten to call `render_rows`.
- `crates/clave-bar/examples/readme-assets.rs:1-32` — the SVG contract, the font discovery env vars, and why the generator panics on a missing glyph.
- `README.md:10-21` — the regeneration comment and the two hero `<img>` tags with their alt text, both of which describe a *two-line* card today.

## Current State

`git status --short`:

```
 M FOOTGUNS.md
?? crates/clave-bar/examples/triple-preview.rs
?? docs/superpowers/specs/2026-09-08-triple-height-card-lock.md
```

Nothing is committed. The branch is `main` in the worktree `.claude/worktrees/triple-card`.

- **`crates/clave-bar/examples/triple-preview.rs`** (new, ~840 lines) — standalone renderer carrying the ratified geometry, a mock fleet covering the variant corners, a shadow-glyph swatch, a spinner filmstrip, and `--animate [secs]` which loops in place with the cursor hidden.
- **`docs/superpowers/specs/2026-09-08-triple-height-card-lock.md`** (new) — the lock.
- **`FOOTGUNS.md`** (modified, § Text, glyphs, rendering) — one entry resolved and two added; see § Important Discoveries.

No production code touched. `card.rs`, `render.rs`, `clave-types` and `readme-assets.rs` are all unmodified.

## What's Working

- **The geometry is signed off by eye, at both profiles, on the maintainer's own display.** It does not need re-litigating. The maintainer's words were "that's the one, let's lock it in."
- **`triple-preview.rs` is a working, gated renderer.** It asserts every line lands on exactly `cols` and that assertion currently passes for all 16 mock cards at both widths. It is the reference implementation the port copies from — the arithmetic in it is correct, not merely plausible.
- **The port has a proven precedent to follow.** The double-height change ran this exact loop and its artifacts survive: `double-preview.rs` shows the end state, and `docs/superpowers/plans/2026-08-26-double-height-cards.md` shows the plan shape that worked.
- **`render_rows` is already the bar's single entry point** and `main.rs` holds no visual logic at all. The port adds a `RowHeight` arm inside a boundary that already exists; it does not need to create one.
- **The store already holds everything lines 1 and 2 need.** Only three cells on line 3 are unbacked (turn clock, subagent boolean, `wants`), and blank is the meaning, so each can land independently and the card is correct at every intermediate stage. This narrowness is the point: **the port is not blocked on the scribe or on any AI work.**
- **The full test suite is green and is a real safety net for this change** — `card.rs` carries goldens that pin the two-line card cell for cell, which is the pattern the four-line goldens copy.

What this does **not** cover: nothing has been rendered inside a live zellij pane, and no SVG has been regenerated. Both are still ahead.

## What success looks like

1. A fresh `clave` install draws four-line cards, because the new height is the default.
2. `card.rs` renders it, `triple-preview.rs` renders *through* `card.rs`, and the two can no longer disagree.
3. Goldens pin both profiles cell for cell.
4. `docs/assets/sidebar-expanded.svg` and `sidebar-collapsed.svg` show the new card, regenerated — not hand-edited — and the README's prose and alt text stop saying "two-line".
5. The three unbacked cells render blank without looking broken, so the port can land before any of the AI work does.

## Important Discoveries

**The worktree glyph was a knowing choice whose cost had not been counted.** `FOOTGUNS.md` already recorded that U+168C2 is BAMUM LETTER PHASE-C MBERAE and that *no* surveyed icon set contains a worktree glyph — zero hits across Nerd Fonts' 10,764 names, 380 octicons and 639 codicons. What nobody had costed is that a fallback face brings **its own metrics**: arriving out of a proportional historic-script font it renders visibly off-centre in its cell. Tolerable beside one other glyph; not once a third joins the column, which is exactly what line 3's subagent mark does. The lock replaces it with `fa-tree` U+F1BB — a metaphor rather than a depiction, and in the font. The generalised rule now in FOOTGUNS: **prefer an in-font metaphor to an exact out-of-font depiction.**

**Two glyph candidates from published cheat sheets do not exist.** There is no `cod-robot` at all (the codicon set's nearest is `cod-hubot` U+EB08), and `fa-robot` is U+0EE0D, not U+F544. Both would have shipped as tofu. This is the same trap as the existing `cod-claude`/`cod-openai` FOOTGUNS entry one layer deeper — there the codepoint was right and the installed font version too old; here the codepoint itself is fiction. The fix is to read the installed font's `cmap` directly.

**The animation interval is constrained by a timer classifier, not by cost.** The bar identifies which timer fired **by elapsed seconds**, with bands at 0.15s, 1.0s and 3.0s, guarded by a compile-time assert chain in `crates/clave-bar/src/main.rs`. The chosen 0.2s frame interval sits in a gap between bands. Anything faster requires replacing the classifier with tagged timers first — do not simply lower the number.

**The live turn counter costs no store traffic, by construction.** The store holds *when the turn began*; the bar subtracts from its own wall clock, which it already reads once per render for the elapsed label. The maintainer asked directly whether animating a counter would flood the store across the fleet — it does not, because only the bar re-renders. This was the ruling that made the turn clock affordable.

**`wants` has a free tier that ships without a model.** `crates/clave/src/hook.rs` already receives `Claude needs your permission to use Bash` as a `Notification` and **discards the tool name**. That is the cheapest source and it needs no AI work at all. The design principle that fell out: **structure gates, the model only supplies words** — whether a row is waiting is decided by the free structural signal that already drives the status mark, and `wants` is written only for rows that signal already flagged. A scribe outage therefore degrades to today's dot, not to silence.

### Approaches tried and rejected

The lock's §8 is the full list with reasoning. The ones that cost the most to learn:

- **The PR on line 3.** It reads as identity, not telemetry, and reserving five cells for it among the measurements punched an empty hole through them on every row without a PR. Moved back to line 2 at round 5.
- **The turn clock winning the collapsed crop.** Reversed at round 8: the animated mark already says *thinking*, so how long is the softer fact. Time since last interaction is the harder one, because past an hour the prompt cache is gone and the next turn costs more.
- **A four-cell gutter**, made by butting the rail against the mark column. Rejected on sight — the rule sat on top of the glyphs.
- **Indenting lines 2 and 3 to the pill's label** rather than its cap. Tried and rejected by eye; the pill is a solid block of colour, so the edge the column reads against is its outer edge.
- **A subagent count.** "This row has fanned out" is the whole signal; the digit was noise.
- **The alternating arc/zebra.** It was linework doing a separator's job because glass forbids a painted stripe. Line 4 does that job properly, so the zebra retired with the arc.

### Two process traps hit this session

- **Python heredoc patches failed repeatedly with `AssertionError`** because `cargo fmt` reformats the target string between runs. Grep the *current* text before matching, or use the `Edit` tool, which fails loudly rather than silently missing.
- **A patch that silently applied only part of itself** cost a full debugging round. Three leading-space removals landed; the `const INDENT` change in the same script did not, leaving the constant and the content it budgeted for one cell out of step. **Verify every hunk of a multi-hunk patch, not just the last one.**

## Next Steps

In priority order. Steps 1-3 are the port and are independent of all AI work.

1. **Add the `RowHeight` variant** in `crates/clave-types/src/lib.rs:475-530`: `lines_per_row() == 4`, `target_cols` of 48 expanded / 16 collapsed. Update `Default`, the `from_config_value` fallthrough arm, `as_config_value`, the clap `value_parser` and the match in `crates/clave/src/main.rs:668-674`, and the tests at `lib.rs:1093-1131`. **Making it the default is this step**, and it means regenerating the KDL artifacts — `clave rows` already reruns setup for exactly this reason.
2. **Port the geometry into `crates/clave-bar/src/card.rs`**, copying the arithmetic from `triple-preview.rs` and the ruling numbers from the lock. Add goldens pinning both profiles cell for cell, in the style of the existing two-line goldens around `card.rs:813-914`. Change the worktree mark at `render.rs:284` to `\u{f1bb}` and update the golden strings that spell `\u{168c2}` (`render.rs:1700,1903,2000` and six sites in `card.rs`).
3. **Rewrite `triple-preview.rs` to render through `render_rows`**, the way `double-preview.rs` does, and delete its private copy of the geometry. Update its header — it stops being able to drift, so the "this file can drift" warning comes out.
4. **Regenerate the SVG assets and check them.** `cargo run -q -p clave-bar --example readme-assets`. Specifically:
   - Point the generator at the new height; it currently hardcodes `RowHeight::Double` (see its header, and `readme-assets.rs` around the `render_rows` call).
   - **Delete the Noto Sans Bamum dependency** — `CLAVE_ASSET_FONT_BAMUM` and the `.expect("Noto Sans Bamum not found")` at `readme-assets.rs:93-99`. JetBrainsMono Nerd Font Mono carries all three new marks.
   - **Decide the spinner frame in the hero.** The generator panics on a glyph no font carries, and four of the six frames are missing from JetBrainsMono. Either render the hero's working rows at U+00B7 or U+2736 (both present), or add Menlo via `CLAVE_ASSET_FONT_EXTRA=/System/Library/Fonts/Menlo.ttc` — untested, and it is a `.ttc` collection.
   - New glyph SVGs are needed: `mark-worktree.svg` changes, and a subagent mark joins `docs/assets/glyphs/`.
   - `README.md:10-21` says "Every agent is a two-line card" and both `<img>` alt texts describe two lines. Update the prose and the alt text; `docs/dev/README-SOP.md` governs any README change.
5. **Wire the free structural signals**, each landing independently: the turn-start timestamp into the store; the subagent boolean from `pendingBackgroundAgentCount`; and tier 1 of `wants` — stop discarding the permission tool name in `crates/clave/src/hook.rs`.
6. **The scribe** — tier 2 of `wants`, and the live headline as a new top tier of the existing recap → ai-title → first-prompt ratchet. Deferred; the AI spec is the reference.

**Live validation is still owed.** Nothing here has been seen inside a zellij pane. `docs/dev/TESTING.md` § the sandbox drive loop is the procedure, and launching the sandbox is the maintainer's to run.

Where the work stopped, verbatim — the maintainer's ratification:

> that's the one, let's lock it in.

And the ruling immediately before it, on the last geometry change:

> yes, but, if the pill hasn't been set, the ai summary needs to be aligned with the other text, so one column leftwards

## Context for the Work

**Working style the maintainer expects.** Rulings are made from **rendered output, never from prose** — every decision in the lock came from looking at a render. When the maintainer asked for options early in this work, a multiple-choice question was rejected outright; what he wanted was a **mapped inventory of candidates with their character counts**, so he could choose against the real budget. Offer the measurements, not the menu.

**Security-relevant constraints, preserved verbatim from AGENTS.md:**

> **Drive the sandbox; never touch his session.** Ollie dog-foods clave daily — the Claude you are is running _inside_ a live clave session, so a bare `zellij` command targets his working fleet. Against `clave-test` you may run `zellij action` freely (`ZELLIJ_SESSION_NAME=clave-test …` — stage it with `just sandbox`); against his session you run nothing, not even a read. **Launching any session is his**, as is `just release` and anything writing `~/.local/share/clave/`. Print those; let him run them. Killing is his too, with one exemption: a sandbox you asked him to launch in this conversation, once its drive and both eyeball checkpoints are done — kill it by its explicit name (`clave dev instance --field session`), never a sandbox another agent staged (each has its own name and root).

`HookPayload::transcript_path` in `crates/clave/src/hook.rs` is documented as **attacker-adjacent input** — always go through `resolve_transcript`, never straight to the filesystem. This matters directly to step 5.

The git stash stack is shared with the main checkout and every other worktree. Never use bare `git stash` / `git stash pop`; prefer a temporary WIP commit.

Commit messages end with `Claude-Session: https://claude.ai/code/session_01FmBv3E9cVa8EynJQrTXTN3`; PR descriptions end with the same URL without the prefix.

**Decision ledger — settled, do not reopen.** Each is recorded with its reasoning in the lock's §4 and §8; this list is the index.

| Decision | Where |
|---|---|
| Four lines: three of content, one of separation | lock §1 |
| Collapsed is 16 columns, expanded stays 48 | lock §1 |
| Collapsed is a strict left-crop; column = priority | lock §5.3 |
| The rail is the repo's colour, full height; the arc and zebra retire | lock §4.1 |
| Provenance gets fixed semantic inks — worktree green, branch violet | lock §4.2 |
| Elapsed wins the crop, not the turn clock; the two clocks never adjoin | lock §4.3 |
| The turn clock is blue while running, blank when not — never dimmed | lock §4.4 |
| The status mark animates with Claude Code's spinner at 0.2s | lock §4.5 |
| The subagent mark is a boolean, not a count | lock §4.6 |
| `wants` is the flex cell; structure gates, the model supplies words | lock §4.7 |
| The live headline replaces the AI title in one ratchet, no second slot | lock §4.8 |
| The PR is identity and lives on line 2 | lock §4.9 |
| No cost figure on the card | lock §4.10 |
| Content is flush with the chrome; a chipless card drops the pill's gap too | lock §2 |
| The shadow rule is drawn outside the selection bar | lock §5.2 |

## Restart Hint

Tests green, nothing committed, no work mid-refactor. Three files are uncommitted and self-contained; the next session can commit them as-is before starting the port. Run `cargo run -p clave-bar --example triple-preview` first to see what was locked, then read the lock's §9.

## Suggested Skills

- `superpowers:brainstorming` — only if the port surfaces a geometry question the lock does not answer. The design itself is settled; do not reopen it.
- `superpowers:systematic-debugging` — if a golden or the width assertion disagrees with the preview during the port. Two rounds were lost this session to reasoning about arithmetic instead of instrumenting it.
