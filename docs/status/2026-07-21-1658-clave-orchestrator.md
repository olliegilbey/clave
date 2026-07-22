# Status — clave orchestrator (v0.1.0 CUT + MIGRATED · daily-driving live · v0.1.1 agents in flight)

_2026-07-21 16:58 · repo github.com/olliegilbey/clave · main `8e77fdb` = tag `v0.1.0` ·
maintainer is DAILY-DRIVING stable clave; this session runs INSIDE it (adopted row
`e2760b9e…`, rename "F-CLA")_

Predecessor: @docs/status/2026-07-21-1323-clave-orchestrator.md (pre-cut state).

## Task Overview

clave = Zellij fleet-orchestration sidebar (wasm `clave-bar` + `clave` CLI).
**v0.1.0 was cut today** and the maintainer migrated his terminal life into it:
8 real Claude sessions adopted via the new worktree-aware resume, old zellij
world torn down. The dogfooding loop is live — findings now come from real use.

## What shipped today (all merged to main, squash convention)

- **#16** worktree-aware resume (#14) + #9's parked-lint fixes. Adversarial
  review found + fixed a detached-branch conflation (`record_branch`).
- **#18** #9 close-out: rustfmt sweep, CI `lint` job (fmt --check + clippy
  -D warnings --all-targets), `zellij-tile = "=0.44.3"` exact pin, ledger.
- **#19** whole-branch fugu HIGHs: backslash-KDL guard everywhere (guardrail
  test proves it on the REAL zellij parser) + main-root from `worktree list`
  entry 0 (not `show-toplevel`, which lies inside linked worktrees).
- **#21** release gate: untracked exemption ALLOWLISTED to docs//.claude/
  /AGENTS.md (CodeRabbit P1 — blanket exemption was unsound: tracked code can
  reference an untracked build input).
- **#22** docs/status handoffs now TRACKED (maintainer ruling — thinking-log
  history; this file rides the next PR).
- **#13** (prior thread) snapshot-carried collapse — validated LIVE today:
  three-part sandbox round (sync toggle / heal-at-birth after reload while
  collapsed / rapid-double-toggle race) all passed.
- Tag `v0.1.0` on `8e77fdb`; first `just release` run (found the gate bug
  live → #21); versioned artifacts + hooks verified.

## Live findings today (issues filed)

- **#23** Ctrl+D tab close strands Alt+↑/↓ nav until a bar row is clicked.
- **#6 (evidence added)** zellij serializes the MCP-server CHILD as the pane
  command → `live_uuids` string-parsing is blind; move liveness to binds.
  Interim rule: navigate to live agents via the bar, never Alt+a-resume them.
- **#17** orphan `<task-notification>` on resume reaches UserPromptSubmit →
  permanently earns garbage labels. Also: such resumes cost one full-context
  UNCACHED auto-turn — quiesce background work before killing sessions.
- **#20** fugu MEDIUM/LOWs (--worktree branch recording, store parent-fsync,
  tab-before-store write order, ambient pipe scoping) + fugu-workflow CLI-lane
  base bug (external lanes reviewed an empty diff — fix before trusting them).
- **#24 UI epic — heavily pre-designed in comments** (read them all):
  - Row format LOCKED: `● ▅ F-CLA · clave · 𖣂 · <summary-or-first-words>`
    (status glyph · context battery · RENAME · main-repo name · worktree
    marker · words). Render from structured FIELDS width-aware, not the
    stored label string; colour carries the repo channel.
  - Renames live in the jsonl as `{"type":"custom-title","customTitle":…}`
    (latest-wins, re-appended; verified on this session's transcript).
    RULING: clave persists latest NON-EMPTY across /clear (Claude clears its
    own); empty records ignored.
  - Battery: lower-block eighths ▁▂▃▄▅▆▇█ (NOT left-blocks — vertical
    depletion), green→red, distinct fresh + past-100% alarm endpoint states;
    100% = rot-reducer's L4 smart-zone ceiling; mirror rot-reducer's env
    config exactly (~/code/rot-reducer/scripts/rot-reducer.sh:
    `tokens_from_transcript` is the proven extractor, compact-boundary-aware).
  - Model badge (fable/opus/sonnet/haiku) from the transcript `model` field.
  - Still for brainstorm: colour system, collapsed 4-col design, widths
    (couples to #4's 30-col constant), braille-vs-blocks eyeball, 𖣂 font check.

## In flight — OUTCOME UPDATE (patched 2026-07-21 ~17:10)

- **#17 DONE + principal-verified, awaiting maintainer signature**: worktree
  `v011-label-hygiene` holds the uncommitted hook.rs diff (+67/-2; const +
  guard + test). Gates re-verified independently: workspace green, clippy
  clean. Next: maintainer signs → PR → merge.
- **#4 and #23+#6 agents DIED on the MONTHLY SPEND LIMIT** (same intermittent
  incident as 2026-07-20's handoff) during their reading phase — worktrees
  `v011-width-seek` and `v011-nav-liveness` are CLEAN, zero work done. The
  briefs below are still the spec; re-dispatch when spend allows, or a fresh
  session implements them directly. Maintainer decides: raise limit at
  claude.ai/settings/usage, or wait for reset.

## Original dispatch (three background agents, worktrees under .claude/worktrees/)

- `v011-nav-liveness` (branch fix/nav-liveness, Opus): #23 + #6 — nav-ring
  re-anchor on tab close + bind-based liveness + stale bind/timeline pruning.
- `v011-width-seek` (branch fix/width-seek-rearm, Opus): #4 with the
  deterministic proptest seed (on #4's comments) committed as red first.
- `v011-label-hygiene` (branch fix/label-hygiene, Sonnet): #17 option (a) —
  injected-tag prompts are not label-worthy; hook.rs only.

Agents never commit — maintainer reviews, signs (1Password popup), PRs merge
per CONTRIBUTING. Milestone **v0.1.1** = #4 #6 #17 #23.

Agent reports land as task notifications in the DISPATCHING session only. A
fresh session should instead: inspect each worktree's uncommitted diff
directly (`git -C <worktree> diff`), and read the report tails at
`/private/tmp/claude-501/-Users-olliegilbey-code-clave--claude-worktrees-issue-10-kdl-guardrail/e2760b9e-a938-49b6-8094-cc206d368b4b/tasks/*.output`
(JSONL transcripts — read only the final result entries, not the whole files).

## Next Steps

1. Review the three agents' work (principal adjudicates + personal pass, then
   maintainer signs) → PRs → merge → **cut v0.1.1** (bump Cargo.toml, tag,
   `just release` — remember the maintainer relaunches `clave` to upgrade).
2. Live-validate #23/#6/#4 fixes per the agents' reported scenario steps
   (sandbox first, TESTING.md SOP).
3. **#24 brainstorm session** (fresh thread, superpowers:brainstorming, with
   the maintainer): colours, collapsed state, widths. Read #24's comments
   first — most decisions are already locked there.
4. Then: doctor/installer spec (docs/superpowers/specs/2026-07-21-installer-
   doctor-design.md, locked) → v0.2.0 milestone; #15 orphan-cwd docs; #11
   upstream-watch epic (add the /clear-clears-rename + orphan-notification
   quirks to its watch list).

**Where work stopped — verbatim:** the maintainer, after the #24 meter-glyph
discussion concluded: "Cool, let's do it. Lock it in." — locking in BOTH
standing offers: dispatch the v0.1.1 briefs (done, three agents launched) and
write this handoff (done). Immediately after, he invoked /handoff — the next
act is REVIEWING THE THREE AGENTS' WORK when their reports arrive, then
maintainer sign-off → PRs → v0.1.1 cut.

## Context to Preserve (rulings + lore, binding)

- **~/.claude/ is READ-ONLY source of truth** (memory file exists): scan
  transcripts, never move/rewrite. Orphaned-cwd sessions: recreate the dir at
  the same absolute path (verified twice) — never relocate jsonls.
- Migration runbook + session manifest: maintainer's private local SOP notes
  (§0 lifeline: this session resumes from the issue-10 worktree path, uuid
  `e2760b9e-a938-49b6-8094-cc206d368b4b`).
- Old pre-split `clave` session/store were torn down at migration (§3b);
  stable store is fresh — only adopted rows.
- User prefs unchanged: extremely concise; explain while doing; dense
  why-comments citing spec §/ledger; conventional commits + Claude-Session
  trailer; ask before architecture pivots; human drives all live zellij;
  sandbox-only hot-reload is the one agent live mutation; `cargo test
  --workspace` always; read C-sections before touching subsystems.
- Review flow that works: brief file → implementer agent → independent
  adversarial reviewer → principal adjudicates + personal pass → CodeRabbit
  on the PR; fugu for whole-branch. CLI lanes in fugu are broken (base bug,
  #20) — don't trust their silence.

## Restart Hint

Main == tag v0.1.0, clean. Three agent worktrees carry UNCOMMITTED work —
do not delete them; resume by reading the agents' reports (task outputs) or
re-dispatching from the briefs in this file's In-flight section. This
session lives at the runbook §0 lifeline. The maintainer is IN clave now:
his screen is ground truth, and new findings arrive as issues.
