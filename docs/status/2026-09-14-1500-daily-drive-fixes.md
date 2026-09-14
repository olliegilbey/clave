# 2026-09-14 — daily-driving the 0.5.0 cut

Branch `chore/release-0.5.0`, seven commits past `main`. **Nothing pushed, and
CI has seen none of it.** Every item came from Ollie running the cut on his
daily fleet and reporting what he saw.

## Done and committed

| Commit | What was wrong |
|---|---|
| `37a9b4f` | `just release` never set `CLAVE_BAR_WASM`, so every locally cut launcher answered "dev build" about itself and its own `clave setup` refused. Fixed in the recipe; `tests/release_recipe.rs` is the gate. |
| `12d4ca2` | Status record closing the 12:20 second-sidebar diagnosis. |
| `e5cfbc4` | The turn clock went coarse at one minute. Three bands now (`59s` → `1m 0s`…`9m59s` → `10m`), five cells wide; the token gap paid two of them and is now ONE cell, a trade Ollie made with the risk named. |
| `435e69a` | `worktree` was recorded only when clave itself created the worktree, so the tree mark had never rendered on a real fleet. git decides it now, plus a fleet repair in the launch and cut tails. |
| `447853f` | Red outlived the block: answering a permission prompt fires no hook, so `NeedsYou` survived to `Stop`. The statusLine meter clears it — a moved token count is an API response, and none lands while a prompt waits. |
| `6bb83ad` | This record. |
| *(uncommitted at write time)* | QA-DRIVE.md phase 5d — the meter seam, specified. |

Each is gated (`fmt`, `test --workspace`, wasm build, `clippy -D warnings`).
Every new test was proved to fail without its fix.

## NEXT ACTION — Ollie has not released past `12d4ca2`

The clock, the tree mark and the red fix are all **unseen on a real terminal**.

```bash
cd /Users/olliegilbey/code/clave
git merge --ff-only chore/release-0.5.0
git tag -f v0.5.0
just release        # expect: "clave: recorded the worktree for 5 row(s)"
zellij kill-session clave && zellij delete-session --force clave && clave
```

## The QA question Ollie asked, and the answer

**Would the automated drive have caught the red-glyph bug? No, and it could
not have.** Phase 5b drives the hook side: `Notification` → `UserPromptSubmit`
→ `Stop` → `SessionEnd`. `UserPromptSubmit` clears `NeedsYou` by itself, so
the sequence never visits the state the bug lives in — a permission answered
with no further hook, where only the statusLine can speak.

**Phase 5d is now specified in QA-DRIVE.md and NOT YET SCRIPTED.** Three
`clave statusline` runs against the sandbox store: red must survive a repeated
token count and clear on a moved one. Building it is the next drive-side task.
It must run with the drive's scrubbed identity like every other leg —
`run_statusline` PUSHES a snapshot, so an unscrubbed run aims at Ollie's fleet
exactly as a hand-written `clave hook` does (FOOTGUNS #281).

## Reasoning worth keeping

**On the red fix.** The obvious route was to register `PostToolUse` and clear
red when a tool ran. Ollie pushed back — "the data should be somewhere, if
other systems know this" — and he was right. `statusline.rs` already runs on
every assistant message and is already spawned, so the fix cost nothing. Reach
for an existing measurement before adding a hook: the hook budget is the
resource this design has always protected.

**The residual on that fix**, named in the code: a reading paced out by
`APPLY_INTERVAL_SECS`, followed by a statusLine re-run DURING the block, reads
as movement and clears red early. Accepted against red that never clears. If
Ollie reports red dropping while he is still blocked, that is this, and the
proper fix compares against the last RAW reading rather than the stored one —
which needs a field on the record.

**On the clock.** The one-cell gap overrides a ruling the lock made by eye.
The escape hatch is written into §5.3: if `130k 9m59s` ever reads as one
number, widen the collapsed card from 16 columns, do not narrow the clock.

## Open

- **Phase 5d is spec-only.** Script it.
- **`clave rows` is not atomic.** It writes the store, then regenerates. A
  failure between them leaves the split that shows as a second sidebar. The
  release fix removed today's cause, not the class.
- **A row whose worktree was DELETED stays unmarked** — git cannot vouch for
  it and the bar asserts nothing it cannot measure. Three of Ollie's dormant
  rows are in this state. Deliberate.
- **Eyeball checks this branch has never had**: tofu on the provider and
  worktree marks (Nerd Fonts 3.5+), the spinner's six frames (five arrive by
  fallback from Menlo, so weight and baseline can jump), `Alt+c` at 16
  columns, the stacked marks on a busy row.
- **Housekeeping**: a PR for every commit here before the `v0.5.0` tag is
  pushed; then the `triple-card` worktree, its branch, and the stale sandbox
  root `~/.local/state/clave-dev-triple-card`.

## Standing constraints

Ollie launches and kills sessions; he owns every write to
`~/.local/share/clave/`. Do not touch his live session — you run inside it.
Fire hooks only through `scripts/ct.sh --hook`. `just gates` green before any
commit. Worktree-isolated: never `cd` to the main checkout; never bare
`git stash`.
