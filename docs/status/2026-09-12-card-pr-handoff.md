# 2026-09-12 — the card PR: screenshots, the hero frame, and the QA witness

Branch `worktree-triple-card`, PR #259. Four gates green.

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

## Measured: the terminal row's facts (and the diagnosis corrected)

**Ollie's ask was to revert what this PR changed. There is nothing to revert
— checked function by function.** The branch diff against main touches none of
`probe_term_facts`, `own_tab_focused`, `probe_targets`, `pane_facts`, or the
terminal row builder. What IS new is the card's line 2, which displays
`branch` and `pr` for a terminal row; main's renderer took `..` over both. He
ruled: keep the line.

Then the mechanism was MEASURED against the live 7-tab sandbox, and the first
diagnosis in this file was half wrong. One `cd` and one `sleep` in one shell
tab produced a fact delta from **all seven bars**. `Event::CwdChanged` and
`Event::CommandChanged` are ungated in `main.rs` — every instance ingests
them. Only the focused instance ever logged a *probe*.

So the hole is narrower and stranger than "nothing shares what one instance
learned": it is the pane **nothing has happened to**. A shell sitting at its
prompt since before a bar was born fires no event, and that bar never probes
it unless its own tab has been focused while the pane was listed. It has
nothing to render, and having nothing writes no log line.

The fix is still the store, for the same reason `collapsed` and `tab_order`
went there — one writer, every instance rebuilding from it. Separate PR. What
this PR adds is the guard that the event fan-out stays fleet-wide, because
gating those two arms behind visibility would read as an optimisation.

Recorded in FOOTGUNS (the bar's model section) with the measurement.

## Built: the QA witness, and the instrument it needed

- **`scripts/qa/lib.sh`** — the instrument, split out of the drive: tracing,
  the assertions, the zellij log read **per bar instance**, and the session
  readers (`dev_status`, `ct_list_panes`, focus). zellij stamps every plugin
  line with `[id: N]` and nothing had ever read it: every reading was a count
  of LINES, which cannot tell one busy bar from five quiet ones. That is
  exactly why the flicker was invisible.
- **`scripts/qa/lib-selftest.sh`** — 27 assertions over a fixture log,
  offline, gated by `crates/clave/tests/qa_lib.rs`. The fixture holds two
  fleets with COLLIDING instance ids, because that is the real log's shape;
  ids are attributed by intersecting them with the build-tagged `loaded`
  lines. Before this, a wrong `sed` expression could only be found by asking
  Ollie to launch a session.
- **Every run prints an instance ledger** — one row per sandbox bar, naming
  the capabilities its own log shows it exercising. Asserted nowhere; it is
  the first thing to read when a phase goes red.
- **Phase 5c, `P5c-term-facts`** — types a `cd` and a `sleep` into a plain
  shell tab and asserts that the delta reaches **every live bar**, counted as
  live tabs (one bar per tab) rather than as log instances, because phase 3
  closes tabs and their `loaded` lines outlive them. Driven live green
  against the showcase sandbox before it was committed: 7 of 7 on both legs.
- **The keystroke guard.** `write-chars` at a claude prompt submits a turn, so
  the phase types only into a pane whose own process is a shell from an
  allowlist, and `script_hygiene.rs` fails the build if a keystroke appears
  outside phase 5c or before that gate. Both halves were proved to bite.
- The 17 hand-rolled NOTE printfs are now one `note` helper, and the
  script-hygiene guards read every QA script rather than the one file they
  were written against.

What phase 5c deliberately does NOT assert: that every bar holds the same
facts. That cannot be true until the facts are store-backed, and a drive must
go red on regressions, not on known-open defects. When the fix lands, the
phase reads the row every bar renders instead of the deliveries it can see.

## Also open

- Spinner motion and weight: the one eyeball check still unconfirmed. The
  "only S6-GUT spins" sighting is not a defect — `arm_spinner` is gated on
  `own_tab_focused()`, so only the bar being looked at animates, and only a
  row actually mid-turn has anything to animate. Every other showcase row is
  dormant, waiting, or idle.
- Ordering churn while navigating: frecency plus cluster grouping (#234),
  amplified by a seeded store with no bucket history. #217 covers the stale
  half. Whether the order should hold still under the cursor is a design
  call, not a defect.
- After merge, `just release` stops being optional: a stale `clave` earlier
  on PATH rejects a `card` store outright and kills every hook.
