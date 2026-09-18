# 2026-09-18 01:30 — the tree mark gets a writer on the hook path

## What was reported

A card on the live fleet read `clave` with no branch, no PR and no tree,
while its session had been in `.claude/worktrees/term-chip-green` on
`worktree-term-chip-green` with #264 open for an hour. A second screenshot
showed the subagent glyph on the same card after its one agent had finished.

## What was actually wrong

Three things, and only one needed code.

1. **The installed binary is v0.5.0.** PR #260 landed after that tag with
   `take_checkout` (cwd and branch follow the transcript) and the subagent
   ledger (launches minus their closing notifications). Both fix what the
   screenshots showed. The ledger reads the reported session correctly: one
   launch, one notification. A 0.5.1 cut is what delivers them.
2. **The tree mark had no writer on the hook path.** `clave add` asks git at
   mint time; `heal_worktrees` asks git at setup and release. `take_checkout`
   clears the mark on a move OUT and nothing set it on a move IN. A row
   minted on `main` whose session then used Claude Code's worktree tool would
   follow the cwd and wear the branch glyph until the next setup.
3. **The stuck glyph** was the one-turn lag of the v0.5.0 count-based
   reading, already replaced by the ledger.

## What this branch does

`hook::worktree_from_tail` reads the newest well-formed `worktree-state`
record; `take_worktree` sets the mark only where the row's cwd is inside the
named path, the same containment `take_checkout` drops on. It never clears.

The first cut cleared on the `"worktreeSession":null` form. The opus
reviewer caught that the null means "no worktree-tool session", which a
session launched directly inside a worktree also writes on every turn while
its cwd stays inside the tree (measured on `b947acf7`). Clearing on it would
strip a correct mark and flap with the setup-time repair. FOOTGUNS records
both the missing-writer rule and the null-form trap.

## Verified

- `just gates` green. 14 then 13 mutants in the diff, all caught.
- Two blind reviewers (opus, sonnet) and CodeRabbit CLI twice, zero
  findings on the reworked branch.
- Not verified live. The fixture is a copy of the real record shape.

## Left for the maintainer

- Merge, then cut 0.5.1. The release is what fixes the reported card.
- Eyeball on the next session that enters a worktree mid-conversation: the
  tree glyph should appear on its next Stop or prompt.
