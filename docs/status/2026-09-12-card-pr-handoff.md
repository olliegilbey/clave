# 2026-09-12 — the card PR: screenshots, hero frame, and the terminal-facts finding

Branch `worktree-triple-card`, PR #259. Four gates green at `05a10ad`.
Remote is behind: **five commits unpushed** (`48ada8d`, `e1436f3`, `d4c114d`,
`be1873b`, `05a10ad`) — a plain push, no force.

## Done this session

- **Doc audit.** Every launch instruction in the repo is now `cd <checkout>`
  then `just launch`. `launch_command` and its two tests are deleted: it
  printed an env prefix that could not carry the PATH shim, which is the
  #43/#44 leak. Its CLAUDE_CONFIG_DIR ruling moved onto `enter_sandbox`.
- **AGENTS.md taken over** from the `agents-md-audit` worktree, plus that
  session's root cleanup (FOOTGUNS and UBIQUITOUS_LANGUAGE into `docs/`,
  CLAUDE.md into `.claude/`, OPUS.md deleted). Verified: 78 relative links in
  live docs all resolve (the readme's `../../issues` is GitHub's own form).
- **Three AGENTS.md corrections agreed and written:** the drive is `just qa`,
  the launch is `cd` + `just launch`, hooks fire only through `ct.sh --hook`.
  The "C-section" term is gone — failed approaches route to FOOTGUNS, because
  the validation ledger now declares itself historical and is not a write
  target.
- **The scenario seeder can fill a card.** `ScenarioAgent` gained provider,
  model, effort, `pr_number`, `wants`, `subagents`; a seeded PR is stamped
  as freshly checked so no background sync wipes it. New `showcase` scenario,
  two tests.
- **Sample screenshots recaptured** against the card from a live sandbox:
  `docs/assets/clave-working-sample-1.png` (expanded) and
  `clave-working-sample-collapsed.png`. AGENTS.md points at the first;
  the collapsed one has no pointer yet (needs Ollie's nod for that line).
- **Hero frame trimmed to six rows** (three live, one terminal, two dormant)
  and rendered at spinner frame 5, the open flower. `Failed` and `Stale` are
  out of the frame: Failed is unreachable (#157) and Stale carried the only
  reading past the smart zone. Nothing reads above 141k now. README alt text
  and the battery-glyph aside updated with it.

## Open: the terminal row's facts flicker

**Ollie's ask was to revert what this PR changed. There is nothing to revert
— checked function by function.** `git diff main...HEAD` touches none of
`probe_term_facts`, `own_tab_focused`, `probe_targets`, `pane_facts`, or the
terminal row builder. What IS new is the card's line 2, which displays
`branch` and `pr` for a terminal row; main's renderer took `..` over both.

So the gap is pre-existing and was invisible:

- Terminal facts (cwd, last command, running) live in per-instance plugin
  state, filled by OS probes (`get_pane_cwd` / `get_pane_running_command`).
- Only the instance whose own tab is focused probes (nav-lag fix,
  2026-08-18), and non-focused instances get no pane manifest at all
  (FOOTGUNS: TabUpdate/PaneUpdate reach only the active tab).
- Nothing shares what one instance learned. The log from the capture session
  shows five bars running and exactly one recording a term-facts update.
- The repo cell has the same dependency, so main flickers too — it just had
  one cell to lose instead of three.

**The fix, and the precedent:** `collapsed` and `tab_order` both began as
per-instance state synced by broadcast, both diverged in the field, and both
moved into the store so every instance rebuilds from one writer. Terminal
facts are the last piece of that shape. Separate PR.

**Unmeasured, and the next thing to measure:** whether `get_pane_cwd`
answers for a pane in a NON-active tab. If it does, the focused instance
learns every terminal's facts and the flicker is a transient before the first
probe answers; if it does not, a terminal's facts can only ever be known in
its own tab's bar. The answer decides whether the store fix needs a host-side
resolver (#239's shape) or only a publish step.

## Open: prevent it in the auto-QA drive

Asked for, not yet built. The constraint: bar pixels are unreadable
(`dump-screen` is empty for plugin panes), so the only witness is
`zellij.log`. Proposal, in the drive's own idiom:

1. After the terminal-tab phase, count DISTINCT instance ids that logged
   `term-facts probe updated`, and print it beside the bar count as a NOTE.
2. Hard-check only what is true today: at least one instance learned them.
3. When the store fix lands, the NOTE becomes a check — every instance
   renders the same terminal row, read from the store rather than the log.

## Also open

- Spinner motion and weight: the one eyeball check still unconfirmed.
- Ordering churn while navigating: frecency plus cluster grouping (#234),
  amplified by a seeded store with no bucket history. #217 covers the stale
  half. Whether the order should hold still under the cursor is a design
  call, not a defect.
- After merge, `just release` stops being optional: a stale `clave` earlier
  on PATH rejects a `card` store outright and kills every hook.
