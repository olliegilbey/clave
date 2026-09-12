# Status — the four-line card, ported and mostly wired

Branch `worktree-triple-card`, worktree `.claude/worktrees/triple-card`.
Supersedes `2026-09-08-2130-triple-height-card.md`.

## Where it stands

Steps 1-4 of the previous handoff are done and committed. Step 5 is two thirds
done; its third part is blocked on a ruling, not on work.

```
6c3fcc8 feat(bar): the four-line card renders from card.rs …
731fb90 docs: the front door shows the four-line card …
bb5115b feat: a waiting agent says what it wants …
9f3fc74 test(bar): the card's right-hand arithmetic is pinned by an ask …
9b5dfc8 feat: the card marks a row that has fanned out …
07fbbca docs: the turn clock is blocked on a ruling …
```

All four gates green on every commit. 672 tests.

## What the card shows now

Three content lines and a hairline, 48 columns expanded, 16 collapsed, default
for a fresh install.

| line | cells |
|---|---|
| 1 | status mark (animated while working) · rail · chip · summary |
| 2 | provenance · rail · repo · branch · PR · provider · model · effort |
| 3 | subagents · rail · tokens · elapsed · **ask** · turn |
| 4 | the shadow rule, outside the selection bar |

Two of line 3's three unwired cells now have sources, both free:

- **ask** — the tool name off the permission `Notification` the hook already
  receives. Gated on `Status::NeedsYou`, cleared the moment the row is not.
  The idle nag is the one message that HOLDS rather than writes.
- **subagents** — `pendingBackgroundAgentCount` off the transcript's closing
  `system`/`turn_duration` line, read from the tail the hook already takes. A
  boolean. Held when a tail carries no closing record.

## Open — the turn clock needs a ruling

**Not a missing field. A collision.** `elapsed` is `now - last_interacted`;
`last_interacted` moves on `UserPromptSubmit` and nothing else; and
`UserPromptSubmit` is the only event that sets `Working`. So "when the turn
began" and "when you last interacted" are the same instant, always. Reading the
turn clock from the store would print `elapsed`'s number two cells away on the
same line — the one thing lock §4.3 separated them to prevent.

Three ways out, all reopening something ratified:

1. **Drop the turn clock.** §4.3 already ruled elapsed is the reading that
   matters and the animated mark already says *thinking*. Reclaims 4 columns
   for `ask`. Reopens §4.4. Simplest, and the recommendation.
2. **Retarget `elapsed` to `last_visited`** — time since you LOOKED, which is a
   genuinely different number. But §4.3's own rationale ("past an hour the
   prompt cache is gone") is about the last *prompt*, so this weakens the cell
   it moves. Also changes the two-line card.
3. **Give the turn clock a new source** — silence since the agent's last
   output, off the transcript tail. A real live number, but no longer free.

Rule from rendered output: `cargo run -p clave-bar --example triple-preview`.

## Owed — live validation

**Nothing has been seen inside a zellij pane.** The sandbox is staged against
this working tree and is `clave-test-triple-card`, launch command in
`just sandbox`'s output. Launching is the maintainer's.

What the drive has to answer, none of it visible in a golden:

- the four-line row against a real pane height — the viewport divides by 4 now
- a click on any of the four lines selects that card
- the spinner's six frames in the real terminal. Five of them come from Menlo
  by fallback (FOOTGUNS), so weight and baseline are eyeball-only:
  `cargo run -p clave-bar --example triple-preview -- --animate`
- the hairline's weight at the real background, and that the selection bar
  stops above it
- the subagent mark beside the status and provenance marks — three glyphs in
  one column is what retired the Bamum worktree letter

## What the mutation run bought

`cargo mutants --in-diff main`, twice: **26 missed of 82**, then **5 of 97**,
then **0 of the four in card.rs**. The only survivor left is
`clave/src/main.rs`'s `fn main`, which is not a seam.

The largest family was line 3's `wants_w`, and the cause is now a FOOTGUNS
entry — `clip_to_cells` guarantees the width at the exit, so a golden whose
flex cells are BLANK cannot see arithmetic that only moves where the right edge
falls. Filling the golden's ask killed eight. What the rest bought, each a real
gap rather than a test-shaped one:

- the ping-pong is a REFLECTION, not just a closed loop over six frames
- the provenance inks live in SGR a stripped golden throws away
- an OPENING row must not be faded while the store still calls it dormant —
  the moment between the click and the first hook event
- the branch reserves one column of AIR beside the repo; off by that one it
  reads as a single long name. Boundary pinned at 24 columns.
- the too-narrow-to-speak threshold is four cells, and four cells still speak

## Deferred

Step 6, the scribe: tier 2 of `ask` (the agent's closing question, shortened)
and the live headline as a new top tier of the recap → ai-title → first-prompt
ratchet. The AI spec is the reference.

## Reference

- `docs/superpowers/specs/2026-09-08-triple-height-card-lock.md` — authoritative
- `crates/clave-bar/src/card.rs` — both geometries; `render_card` is four lines,
  `render_double_card` is two
- `crates/clave/src/hook.rs` — `wants_from_message`, `take_wants`,
  `subagents_from_tail`
- `docs/dev/TESTING.md` § the sandbox drive loop
