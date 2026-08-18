# Status — terminal tab rows: PR #211 open; next is review loop → merge → v0.2.0

You are the shipping engineer for clave's terminal-row feature (#206, second
half), fluent in the bar's render vocabulary (gutter · cell · ink · chip ·
provenance) and the sandbox drive SOP. The design conversation is OVER —
every decision below is ratified live by Ollie, including two rounds of
performance fixes his own drive flushed out and the sub-second-command
sampling trade (his words: "Happy with that trade"). **PR #211 is OPEN**
with the full design/evidence body. Your job is the review loop, re-running
the two stale verifications, the knowledge write-downs, and handing Ollie
the merge + v0.2.0 tag. Do not reopen design.

## Task Overview

Ship the enriched terminal tab row: console glyph in the status cell
(colour = state), `TERM`/prompt-glyph in the battery cell, tab name as a
fujiWhite-on-black chip, cwd-derived repo cell sharing the agent ink
allocation, borrowed provenance, and most-recent-foreground-command summary.
All implemented and live-validated in the sandbox at `2e0658d`. Success =
PR merged into main (closing #206), mutants + quiescence re-verified on the
final tree, FOOTGUNS/UL entries written, then Ollie tags **v0.2.0** and runs
`just release`.

## Reference Docs

- @docs/status/2026-08-18-1313-terminal-tab-rows.md — the design-phase
  handoff: PaneInfo facts, scope cliffs, sandbox/hot-reload commands, review
  loop mechanics (CodeRabbit `@coderabbitai full review`, ~41min rate-limit
  window). Still accurate for process; its "cwd is NOT exposed" claim was
  WRONG (see Discoveries).
- `crates/clave-bar/src/render.rs` — `TermStatus`, widened
  `RowContent::Terminal` + `RowContent::terminal()` defaults ctor, the two
  goldens (regenerate ritual: temp `examples/dump-golden.rs`, see git log
  `5502837` diff), `TERM_MARK`/`TERM_GLYPH` (`\u{f120}` nf-fa-terminal).
- `crates/clave-bar/src/model.rs` — `TermFacts`/`PaneProbe`,
  `apply_pane_facts`, `probe_targets` (three gates), `speaker_pane`,
  `terminal_content`, `checkout_of`, `provenance_of` (extracted free fn),
  `command_display`/`SHELLS`; tests under "#206 terminal rows".
- `crates/clave-bar/src/main.rs` — subscriptions (`CwdChanged`,
  `CommandChanged`), `probe_term_facts`/`arm_term_poll` (both gated on
  `own_tab_focused`), the third timer class (`TERM_POLL_CUTOFF_SECS` 2.0 /
  `TERM_POLL_SECS` 3.0), changed-only `term-facts` eprintln lines.

## Current State

Clean tree on `terminal-tab-info`, 5 commits ahead of origin/main
(`5502837`..`2e0658d`, all narrative one-liners, read them). Sandbox
`clave-test-alt-f-to-3c9c` is LIVE running build `2e0658d` (hot-reloaded;
`just sandbox ux-gate1` exits 1 at config-regen while the session lives —
benign, the wasm still stages; reload with `scripts/ct.sh
start-or-reload-plugin "file:/Users/olliegilbey/.local/state/clave-dev-alt-f-to-3c9c/data/clave-bar.wasm"
-c clave_binary=clave`). Ollie was mid-manual-drive and happy: "Lag is gone
now, nav is crisp again."

## What's Working

- `just gates` green at HEAD; 197 tests. Goldens pin the new terminal row
  byte-for-byte in both profiles.
- Mutants: 64/66 runs at `5502837` and `c94feb1` — 0 missed each. STALE for
  the last three commits; re-run `just mutants` (never bare `--in-diff`).
- Live-proven via `zellij.log` `term-facts` lines (the ONLY automatable
  evidence — dump-screen is empty for plugin panes): probes deliver
  cwd+command on macOS; command-start deltas reach every instance;
  exit-side covered by the while-running poll; cwd learned ≤3s after `cd`;
  70s idle window added ZERO lines (quiescence — also stale, re-run: count
  `grep -c 'term-facts'` in `$TMPDIR/zellij-501/zellij-log/zellij.log`,
  idle 70s, recount).
- Ollie eyeballed the fleet: chip/TERM/colours "glorious"; Failed glyph,
  borrowed worktree provenance, and fujiWhite fixes all confirmed visually.

## Important Discoveries

- **cwd IS exposed to plugins** — `get_pane_cwd` / `get_pane_running_command`
  (sync, `ReadApplicationState`, already held) + `CwdChanged`/
  `CommandChanged` events. The prior handoff's contrary claim is dead.
- **zellij never pushes the exit-side CommandChanged** (start yes, end no)
  → the 3s while-running poll exists. `CwdChanged` never fired at all in
  practice; probes/poll carry cwd.
- **Sub-second commands are invisible by construction**: zellij samples
  foreground ~1/s, so `ls`/`la` never register and never displace the last
  command that did (`sleep 5` proven to flow; his `la` proven not to).
  RATIFIED — "Happy with that trade". Documented on `TermFacts::last_cmd`
  and in the PR body; scrollback scraping and shell integration are refused
  scope cliffs for v0.2.0. If a reviewer raises it, point at the ledger.
- **Perf lessons (the nav-lag saga, three commits)**: synchronous host
  calls multiply across hidden bar instances (5 instances × 2 calls × tabs
  per PaneUpdate, serialized in the server); EXITED panes can never mint a
  facts entry so they re-qualified as unknown forever (2 failing calls per
  press). Gates now: visible bar only (`own_tab_focused`), unknown-or-running
  panes only, never exited panes; poll shares the visibility gate. These are
  the FOOTGUNS entries to write.
- All-None probes mint no facts entry (birth-failure retry); facts are
  per-instance state and prune with the manifest.
- Golden regeneration ritual: recreate `examples/dump-golden.rs` from the
  `5502837` diff, run, paste escaped lines, DELETE it.

## Next Steps

1. DONE: branch pushed, **PR #211 open**
   (https://github.com/olliegilbey/clave/pull/211) — body carries the design
   summary, gates evidence, and the sampling trade. The trade is also a doc
   comment on `TermFacts::last_cmd` (commit at HEAD).
2. Re-run `just mutants` and the 70s quiescence window at final HEAD.
3. Review loop: fix findings, reply "Ollie's Agent Speaking:", resolve —
   never silent-resolve; `@coderabbitai full review` (plain `review` fails).
4. FOOTGUNS.md: hidden-instance host-call multiplication; exited-pane
   unprobeable-forever. UBIQUITOUS_LANGUAGE.md: **speaker** (the pane that
   speaks for a terminal tab's row), terminal chip semantics (tab name,
   black = unclaimed by agent ink).
5. Merge is Ollie's (`gh pr merge` blocked for agents; the "'main' is
   already used by worktree" error after his merge is cosmetic). Then hand
   him: v0.2.0 tag + `just release` + sandbox teardown pair
   (`zellij kill-session clave-test-alt-f-to-3c9c && zellij delete-session
   --force clave-test-alt-f-to-3c9c`).

Where work stopped — Ollie, verbatim: "Lag is gone now, nav is crisp again.
Only thing I've noticed that seems to be an issue is the tmp pane, when
focused, and I type a command, that command doesn't show in the tab text."
(Answered: short-command sampling gap, see Discoveries; confirm he accepts.)

## Context for the Work

Ratified decision ledger (do NOT reopen): status colouring degrades to
always-running for shells, Done/Failed only on command panes (Q1) · chip =
zellij tab name, rename via zellij IS the labelling mechanism, black chip
stays black on selected rows (Q2/Q4) · keep `TERM`, no pane count (Q3) ·
repo cell = cwd dir name always, matched checkout shows basename(repo_root)
(Q5) · provenance by store prefix-match only, worktree glyphs free via
match, no git reading, no path heuristics (Q6) · summary = currently-running
else last-run, plugin-side state only (Q7) · focused-else-first tiled pane
speaks (Q8) · fujiWhite everywhere a terminal row has no repo ink (live
fixes) · Alt+f LAYER semantics and DORMANT_FADE=0.6 are prior ratified,
untouched. Register: outcome first, no unglossed symbols, ~6 sentences,
decision over mechanism. Sandbox lifecycle is Ollie's; drive via ct.sh only.

## Restart Hint

Clean tree, all committed, sandbox live at HEAD — safe to start at Next
Steps 1 immediately; background the CI/review waits and report between them.

## Suggested Skills

- `superpowers:requesting-code-review` / `superpowers:receiving-code-review`
  — the PR loop is the whole job.
- `superpowers:verification-before-completion` — before claiming the PR
  ready (mutants + quiescence are stale until re-run).
- `handoff` — if the review loop outlives the session.
