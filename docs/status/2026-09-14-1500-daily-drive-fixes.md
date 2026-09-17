# 2026-09-14 — daily-driving the 0.5.0 cut

Branch `chore/release-0.5.0`, now **pushed and open as PR #260**. Every item
came from Ollie running the cut on his daily fleet and reporting what he saw.

CI passed on the first run: `test`, `wasm-build`, `lint`, `plan`, GitGuardian.
The PR-side CodeRabbit skipped itself ("manual review required for this OSS
repository"), so it was asked explicitly in a comment; the CLI lane had already
run against `origin/main` and its three findings are fixed in `dc996eb`.
The PR round then found two more, fixed in `adb2d8a`. Five findings, none
declined. Each thread has a reply; **the threads need Ollie to resolve
them** — the agent's token is refused on `resolveReviewThread`. That is
CONTRIBUTING's convention, not a gate: GitHub reports the PR `MERGEABLE` /
`CLEAN` with both threads open, so resolution is not enforced by branch
protection.

**Three blind reviewers then read the branch** (one per area: the worktree
recording, the status transition, the card geometry). They found one MAJOR
defect in a fix committed the same afternoon, one inverse defect the
widening had made reachable, two tests that passed by construction, and two
user-facing docs left stale. All fixed. The card geometry was proved sound
exhaustively — every reading 0..1300s crossed with nine token values, both
widths, every column 1..=80.

## Done and committed

| Commit | What was wrong |
|---|---|
| `37a9b4f` | `just release` never set `CLAVE_BAR_WASM`, so every locally cut launcher answered "dev build" about itself and its own `clave setup` refused. Fixed in the recipe; `tests/release_recipe.rs` is the gate. |
| `12d4ca2` | Status record closing the 12:20 second-sidebar diagnosis. |
| `e5cfbc4` | The turn clock went coarse at one minute. Three bands now (`59s` → `1m 0s`…`9m59s` → `10m`), five cells wide; the token gap paid two of them and is now ONE cell, a trade Ollie made with the risk named. |
| `435e69a` | `worktree` was recorded only when clave itself created the worktree, so the tree mark had never rendered on a real fleet. git decides it now, plus a fleet repair in the launch and cut tails. |
| `447853f` | Red outlived the block: answering a permission prompt fires no hook, so `NeedsYou` survived to `Stop`. The statusLine meter clears it — a moved token count is an API response, and none lands while a prompt waits. |
| `6bb83ad` | This record. |
| `77f64ee` | QA-DRIVE.md phase 5d — the meter seam, specified. |
| `dc996eb` | Review round: the worktree repair was spawning git inside the store lock, so every hook in the fleet queued behind it. The calls moved out; only the mutation takes the lock. |
| `adb2d8a` | Second review round: the repair compared a row's `cwd` to worktree roots for EQUALITY, but a row's cwd is wherever it was added from, so every row added in a subdirectory of a worktree kept the branch mark. It also recorded the cwd rather than the worktree, which disagreed with the add path. Both fixed by `worktree_holding`. |
| `9d01f68` | **The red fix was wrong, and its failure mode was worse than the defect.** It compared the reading to the count in the RECORD, which `count_due` can hold back — so a prompt landing in that gap cleared on the withheld figure, then the idle nag restored red. Red → amber → red, with the `wants` cell blanking each time. Two readings inside the block are needed; `metered_at` counts them. |
| `31d6ce1` | `apply_relocation` left `worktree` set, so a row following its transcript OUT of a worktree kept the tree mark forever — the inverse of `435e69a`, made reachable by it. Plus a resume test that passed by construction. |
| `7d67682` | Nothing tied the clock's widest reading to the cell that holds it; README and UBIQUITOUS_LANGUAGE still described a two-band clock. |

Each is gated (`fmt`, `test --workspace`, wasm build, `clippy -D warnings`).
Every new test was proved to fail without its fix.

## NEXT ACTION — nothing here has been seen on a real terminal

The clock, the tree mark and the red fix have only ever been rendered by tests
and by `examples/triple-preview.rs`. Ollie has released nothing past `12d4ca2`.

**The repo squash-merges** — every commit on `main` ends in `(#N)` and there
are no merge commits in its history. So PR #260's commits become ONE new
commit, and the local `v0.5.0` tag at `12d4ca2` names a commit that will never
exist in public history. The tag has to move onto the squashed commit.

Local cut now, for the eyeball checks, while the PR sits:

```bash
cd <the main checkout>
git merge --ff-only chore/release-0.5.0
git tag -f v0.5.0
just release        # expect: "clave: recorded the worktree for 5 row(s)"
zellij kill-session clave && zellij delete-session --force clave && clave
```

After #260 merges: reset `main` to `origin/main`, re-tag `v0.5.0` there, run
`just release` again, and push the tag LAST — it fires `release.yml`, which
triggers on tags. The nine numbered live steps with their expected observations
are in the PR body.

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

## NEXT DEFECT, diagnosed and NOT yet fixed — the subagent mark sticks

Ollie, 2026-09-14 16:30: the subagent glyph appeared while three reviewers ran
and **never went away** after they finished. Same class as the red glyph: a
held state whose only clearing signal cannot reach the code.

`take_subagents` (`hook.rs`) holds on `None` by design — "no reading" must not
blank a real mark — and only `SessionEnd` forces it false. The reading comes
from `subagents_from_tail`, which wants the last `system` / `turn_duration`
record in the 64 KiB tail (`hook.rs`, `read_tail(&path, 64 * 1024)`).

**Measured on the live transcript of this very session:**

| | |
|---|---|
| transcript size | 37.2 MB |
| `turn_duration` records in it | 110 |
| records inside the 64 KiB window | **0** |
| distance from EOF to the last one | **742,873 bytes** (11x the window) |
| median gap between consecutive records | 125,755 bytes (2x the window) |
| max gap | 4,264,884 bytes |
| that last record's `pendingBackgroundAgentCount` | absent, so it would have read FALSE and cleared |

So on any tool-heavy session `subagents_from_tail` returns `None` on every
`Stop`, the hold rule fires every time, and a mark set once can never clear.
The hold was written to protect a Claude Code too old to emit the field; the
field is emitted 110 times here, just never where we look. FOOTGUNS already
predicted this shape for `title`/`summary` ("a tool-output-heavy turn is the
shape that would break it") — this is that prediction coming true on a
different field, and on a boolean it is unrecoverable rather than merely stale.

`pendingBackgroundAgentCount` appears nowhere else as data: the 29 string hits
inside the window are this conversation TALKING about the field, not carrying
it. `system` / `turn_duration` is the only source.

**Recommended fix, not yet written.** A bounded backward search for the record
on `Stop` only, capped around 1 MiB — the median gap says that finds it almost
always, the record is authoritative when found, and the hold stays as the
fallback when it is not. Rejected: widening the shared tail (every hook pays,
and 4.2 MB gaps beat any sane cap), and clearing on silence (silence is now the
normal case, so the mark would blank while subagents are still running).

Whatever is chosen, the test that pins it must assert against a tail with NO
`turn_duration` record in it, because that is the live shape and no existing
test uses it.

## Open

- **Phase 5d is spec-only.** Script it. It is now the ONLY automated check
  that could reach the red transition, and that transition has been wrong once
  already.
- **`heal_worktrees` and `linked_worktree_root` have no test against a real
  repo.** Only the pure helpers are covered. The fact the whole feature rests
  on — that `rev-parse --show-toplevel` and a `worktree list --porcelain` entry
  canonicalize to the SAME string — is asserted nowhere. The harness exists
  (`store_worktree_dirs_lists_main_and_linked_from_a_real_repo` builds a real
  repo with a real linked worktree in a tempdir), so this is a dozen lines.
- **The worktree repair has no hand-run verb.** `backfill` has `clave
  backfill` for exactly the "it did not run when it should have" case; this
  has nothing, and it only runs on a version refresh.
- **`clave rows` is not atomic.** It writes the store, then regenerates. A
  failure between them leaves the split that shows as a second sidebar. The
  release fix removed today's cause, not the class.
- **A row whose worktree was DELETED stays unmarked** — git cannot vouch for
  it and the bar asserts nothing it cannot measure. Three of Ollie's dormant
  rows are in this state. Deliberate.
- **Legibility to judge by eye, named by the review:** the middle band's real
  worst case is not `130k9m59s` but `113k 1m 7s` — the gap between the two
  numbers is one cell and the gap INSIDE the clock is also one cell, so line 3
  can read as three evenly spaced groups instead of two numbers. A megatoken
  row makes it `1m   1m 7s`, two different `m` units on one line. Ink separates
  them while the turn is live and stops doing so after. §5.3's escape hatch is
  two more columns on the collapsed card.
- **Eyeball checks this branch has never had**: tofu on the provider and
  worktree marks (Nerd Fonts 3.5+), the spinner's six frames (five arrive by
  fallback from Menlo, so weight and baseline can jump), `Alt+c` at 16
  columns, the stacked marks on a busy row.
- **Housekeeping**: after #260 merges, the `triple-card` worktree, its branch,
  and the stale sandbox root `~/.local/state/clave-dev-triple-card`.

## Standing constraints

Ollie launches and kills sessions; he owns every write to
`~/.local/share/clave/`. Do not touch his live session — you run inside it.
Fire hooks only through `scripts/ct.sh --hook`. `just gates` green before any
commit. Worktree-isolated: never `cd` to the main checkout; never bare
`git stash`.
