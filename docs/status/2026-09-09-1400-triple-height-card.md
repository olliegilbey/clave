# Status — the four-line card, driven once and amended from it

Branch `worktree-triple-card`, worktree `.claude/worktrees/triple-card`.
Supersedes `2026-09-09-0200-triple-height-card.md`.

## Where it stands

The card is fully wired — every cell has a source. The first sandbox drive is
done, the maintainer has ruled on what it showed, and the three defects it
surfaced are fixed. **A second drive is owed** for the cells the first one
could not reach.

```
07fbbca docs: the turn clock is blocked on a ruling …
cdb4ee9 …
b148914 …
07aa880 fix: the elapsed clock holds one column across the crop, and a
        synthetic line is not a model
8df4163 feat(bar): one clock, lit and counting seconds while the turn runs
```

All four gates green. 269 bar tests, 358 host tests.

## The card as it now stands

| line | cells |
|---|---|
| 1 | status mark (animated while working) · rail · chip · summary |
| 2 | provenance · rail · repo · branch · PR · provider · model · effort |
| 3 | subagents · rail · tokens · **clock** · ask |
| 4 | the shadow rule, outside the selection bar |

## Closed — the turn clock (lock §4.4, amended)

**There was never a second number.** `last_interacted` moves on
`UserPromptSubmit` and nothing else; `UserPromptSubmit` is the only event that
sets `Working`. "Turn began" and "last interacted" are one instant.

One cell, two renderings: crystalBlue and counting SECONDS while the turn runs,
dimmed and coarse once it is over, when the same number means staleness. The
drive is what settled it — the card read `0m` through a turn Claude Code's own
footer called `3s`.

Free at both ends: the bar already repaints every 0.2s while any row is
thinking (`arm_anim`) and arms nothing when none is. Dropping the separate cell
returned four columns to `ask`.

## What the first drive bought

Three defects, none of which a golden could see:

1. **The clock sat one column left in expanded.** The crop test named for
   exactly this passed anyway — it checked that collapsed's clock was flush
   right and that both lines kept a margin, but never that the two agreed on a
   column. It now derives the column from the COLLAPSED geometry, so it cannot
   agree with the expanded arithmetic by sharing a constant with it.
2. **The subagent mark could never clear.** Every `turn_duration` record in the
   transcript omitted `pendingBackgroundAgentCount` rather than writing `0`,
   and the fail-closed rule read that as "no reading" and HELD. Only a tail
   with no closing record at all holds now.
3. **`<synthetic>` was taken as a model.** One such line among ten real answers
   left the card reading `<synt…`. `model_from_tail` scans past it.

Confirmed working live: four lines per row against a real pane; the click hit
test at `lines_per_row=4` (`raw_line=5 → row_line=1`); the collapse toggle
48 ↔ 16; the six-frame spinner; 60s idle quiescence at zero store writes.

## Owed — a second drive, for what the first could not reach

The maintainer's own words: *"I'd need a session with subagents and on a branch
if I wanted those glyphs to show."*

- **`ask` never populated** — the drive's Bash call was auto-approved, so no
  permission `Notification` ever fired. Needs a tool call that actually prompts.
- **the subagent mark never rendered** — no background agents ran, so
  `pendingBackgroundAgentCount` never appeared at all. Needs a session that
  dispatches subagents.
- **the branch mark** — the sandbox agent sat on a default checkout.
- **the lit clock** — new since the drive. Blue and ticking seconds mid-turn,
  dimming when it stops. Eyeball only; the goldens cannot see ink.
- **post-turn quiescence** — the animation timer's disarm was never measured
  after a Working row went idle.

## Open — one thing to watch, not a bug

crystalBlue is the rail's ink as well as the clock's, so a lit clock is blue on
a line that already has blue on it. The rail is a constant on every row and the
clock is digits, so they should not compete — but it is an eyeball call, and if
it reads badly the fix is a distinct live ink, not a different cell.

## Deferred

Step 6, the scribe: tier 2 of `ask` (the agent's closing question, shortened)
and the live headline as a new top tier of the recap → ai-title → first-prompt
ratchet.

## Reference

- `docs/superpowers/specs/2026-09-08-triple-height-card-lock.md` — authoritative,
  §4.4 amended 2026-09-09
- `crates/clave-bar/src/card.rs` — both geometries
- `crates/clave-bar/src/model.rs` — `turn_label` / `elapsed_label`
- `crates/clave/src/hook.rs` — `wants_from_message`, `subagents_from_tail`,
  `model_from_tail`
- `docs/dev/TESTING.md` § the sandbox drive loop
