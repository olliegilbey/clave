# Status — terminal tab rows: design grill next, then build, then tag v0.2.0

You are a design partner and grilling counterpart on clave's sidebar, fluent
in the bar's render vocabulary (gutter · cell · ink · chip · provenance,
UBIQUITOUS_LANGUAGE.md) and in what zellij's plugin API can and cannot see.
The next session is a CONVERSATION first: grill Ollie on what a terminal tab
row should say, decide together, and only then implement. Work in his
register: outcome first, no unglossed symbols, ~six sentences, decision over
mechanism, KISS over flourish.

## Task Overview

Improve the terminal tab row in the sidebar — today it renders only the
console glyph and the zellij tab name ("Tab #6") in katanaGray, using 2 of
its 5 information cells. This is the SECOND half of #206 (the dimming half
merged as PR #210) and the last item before release: Ollie will tag
**v0.2.0** (not v0.1.3/v0.1.4 — his ruling, enough has moved) once this
lands. Success = a design he ratified through grilling, implemented in
`crates/clave-bar/` (likely render.rs + model.rs only), gates + mutants
green, eyeballed live in the sandbox.

## Reference Docs

- Issue #206 — the one-liner for this half: "improve the plain terminal
  tab's appearance in the bar", KISS, no new state machinery.
- `crates/clave-bar/src/render.rs:706-728` — the terminal row arm today
  (gutter: no status, rule, CONSOLE mark in the battery cell, no
  provenance) and `:820-825` (name in TERMINAL_INK, body otherwise blank).
- `crates/clave-bar/src/model.rs:20-43` — TabMeta/PaneMeta, the 4 fields
  kept per pane today; `:1668-1692` — where terminal rows are built
  (`RowContent::Terminal { name }`, lock §7.1: tab NAME is only used for
  terminal tabs).
- Vendored `zellij-utils-0.44.3/src/data.rs:2296-2336` — full PaneInfo.

## Current State

Nothing implemented; the fact-finding is done (see Discoveries). Worktree
`.claude/worktrees/alt-f-toggle-207`, branch `terminal-tab-info` ==
origin/main at `186b28e`, clean. Today's merges: #209 (Alt+f toggle,
closed #207) and #210 (dormant dimming, DORMANT_FADE=0.6 ratified live).
#206 remains OPEN for exactly this half. The v0.1.3 tag exists locally,
unpushed, and is now obsolete — v0.2.0 will be cut fresh after this work.

Sandbox `clave-test-alt-f-to-3c9c` is LIVE with the `ux-gate1` scenario and
current main's bar. Fine for fleet visuals; note its terminal-tab surface is
thin — a drive of THIS feature wants real terminal tabs (open some via the
session, or extend a scenario).

## What's Working

- The render seam is clean and total: `RowContent::Terminal { name }` is one
  enum arm; adding fields to it is a local change. The `agent()` /
  `numbered()` test fixtures and the golden-bar tests are the safety net,
  and `Row.dormant` (new, #210) carries block membership separately from
  the status glyph — copy that shape if terminal rows need row-level flags.
- The PaneManifest pipeline already flows: `apply_panes` ingests a
  session-wide manifest (fresh for ALL tabs on the active bar instance —
  hidden bars are stale but invisible, so cross-tab staleness is NOT a
  blocker here). Widening `PaneMeta` with more PaneInfo fields is the
  established pattern (#207 added `is_floating` exactly this way).
- `just gates`, `just mutants` (NEVER bare `cargo mutants --in-diff` — it
  silently generates zero mutants for clave-bar; the recipe's `--workspace`
  is load-bearing), hot-reload via
  `scripts/ct.sh start-or-reload-plugin "file:<sandbox>/clave-bar.wasm" -c
  clave_binary=clave` (the `-c` is identity; bare URL mints a second bar —
  FOOTGUNS). Live-eyeball loop with Ollie is fast and he enjoys it.

## Important Discoveries

- **Unread PaneInfo fields, per pane, already delivered**: `title` (the
  pane's UI title — program name or shell-emitted OSC title),
  `terminal_command` (full command string for command panes, `None` for
  plain shells), `exited` + `exit_status` + `is_held` (finished command
  panes and reruns-pending), focus and geometry (→ focused pane per tab,
  pane count per tab).
- **cwd is NOT exposed to plugins.** "Which project is this shell in" needs
  shell-emitted titles or new host machinery — scope cliff #1; descope
  unless Ollie insists.
- **Terminal tabs have no store row.** Anything persistent (custom names,
  notes) is new host machinery — scope cliff #2; the issue says no new
  state machinery.
- Column budget is fixed: chip 9 · repo 7 · summary flex; battery cell
  currently holds the CONSOLE mark on terminal rows. Lock §2 (uniform row
  width) and §7.1 (tab name only for terminal rows) bind any design.
- Terminal rows are exempt from dormant fade by construction
  (`dormant: false` at model.rs:1689) — they are always "live".

## Next Steps

1. **Grill Ollie** (superpowers:brainstorming or mattpocock-skills:grilling —
   he asked for this explicitly). Seed questions: what does he want at a
   glance — what's running, done/failed, or where it lives? Focused pane's
   title/command into the summary cell? Pane count anywhere? Do
   exited/held command panes get status marks like agents? Is "Tab #6" a
   clave problem (derive a better label) or a user habit (rename in
   zellij)? Which of the two scope cliffs, if either, is worth crossing?
2. Implement the ratified design: widen `PaneMeta`, extend
   `RowContent::Terminal`, render — tests alongside, goldens updated only
   after confirming against the lock.
3. Sandbox eyeball with real terminal tabs → gates → mutants → PR → the
   CodeRabbit/Codex loop (fix, reply "Ollie's Agent Speaking:", resolve;
   plain `review` fails — use `@coderabbitai full review`; expect a
   rate-limit window ~41min after the first).
4. Merge (the `gh pr merge` command is HIS — permission classifier blocks
   the agent; the `'main' is already used by worktree` error after his
   merge is cosmetic).
5. Hand him the v0.2.0 tag + `just release`; sandbox kill pair when done:
   `zellij kill-session clave-test-alt-f-to-3c9c ; zellij delete-session
   --force clave-test-alt-f-to-3c9c`.

Where work stopped — Ollie, verbatim: "It's to improve the terminal tab
content in the clave sidebar for terminal tabs. […] the build will be to
improve the information, we'll decide on how before implementation by
talking about it together."

## Context for the Work

- Decisions ratified today, do not reopen: Alt+f keeps LAYER semantics (no
  per-pane shell tracking — zellij has no pane-scoped show/hide; both
  reviewers raised it, Ollie ruled "simpler and working is better");
  DORMANT_FADE=0.6; shell geometry y=4/w=98/h=92.
- The sandbox drive SOP: ct.sh only, session lifecycle is Ollie's,
  hot-reload always with `-c clave_binary=clave`, prefer kill+restage when
  in doubt.
- He reads mockups well — for render work, consider showing 2-3 candidate
  row layouts as text mockups during the grill rather than describing them.
- Commit messages in the repo's narrative one-liner style.

## Restart Hint

Clean tree on `terminal-tab-info` == origin/main; safe to start straight at
the grill. Read the render/model line ranges above before proposing designs.

## Suggested Skills

- `superpowers:brainstorming` or `mattpocock-skills:grilling` — FIRST, before
  any code; Ollie asked for the design conversation explicitly.
- `mattpocock-skills:prototype` — if candidate row layouts want a quick
  visual (the `bar-preview` example renders rows without a session).
- `superpowers:verification-before-completion` + `handoff` at the end.
