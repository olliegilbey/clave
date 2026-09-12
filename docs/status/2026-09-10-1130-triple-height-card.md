# Status — the four-line card, drives done, ready for a PR

Branch `worktree-triple-card`, worktree `.claude/worktrees/triple-card`.
Supersedes `2026-09-09-1400-triple-height-card.md`.

## Where it stands

**The feature is complete and the second drive is done.** Every cell the first
drive could not reach has now been seen — by the maintainer, against a real
agent, in the sandbox. Nothing on the owed list is still owed. All four gates
green; 271 bar tests, 358 host tests.

```
b148914 test(bar): the last four surviving mutants in the card …
07aa880 fix: the elapsed clock holds one column across the crop …
8df4163 feat(bar): one clock, lit and counting seconds while the turn runs
8fa6915 test: an ask that fits proves nothing, and a mark that moves must say so
c258c34 docs(bar): the preview shows the clock at turn resolution
647e54d fix(bar): seconds only where something repaints them
6f902f7 fix(scripts): ct.sh guards the second door, the one clave hook opens
ecfb0b1 test(bar): the collapsed card is pinned at its widest, not argued about
```

## Closed since the last handoff

**The lit clock, confirmed live.** The maintainer drove a real turn: `18k 5s`
then `19k 4s` in crystalBlue while working, `19k 0m` dim once it stopped. That
is one subtraction, `now - last_interacted`, at two resolutions — not two
measurements swapped, which is the reading it invites. It works only because
`last_interacted` moves solely on `UserPromptSubmit` and that is also the only
event setting `Working`, so "turn began" and "last interacted" are the same
instant. A separate turn-clock cell was built first and dropped on finding it
would print the same number twice, two columns apart.

**Collapsed width, pinned rather than argued.** The worry was a collapsed line
reading `123k #1234 30m`. It cannot occur: the PR lives on line 2 and the crop
drops it there, so collapsed line 3 is the token count and the clock alone.
`the_collapsed_card_fits_its_widest_cells` asserts it at each cell's ceiling —
`9.9m` (the widest the formatter yields before it gives up the tenth) and `59s`
— with the PR surviving on neither line and every line still sixteen columns.

**`TOKEN_GAP` is 3, not 2.** The maintainer's one-column complaint. Collapsed
is the ruled position and expanded now matches it.

**Nav (`alt+up` after `alt+down`) — not a defect.** Reported, not reproduced
here, and the maintainer has since confirmed it works in the sandbox. The
`BIND STALLED (unresolved tab)` line in the log is healthy for a frame or two.
Not filed.

## The one that cost two rounds — `clave hook` had no guard

`ct.sh` guarded `zellij action` and nothing else. `clave hook` writes the store
(safe — `CLAVE_STATE_DIR` selects it) and then PUSHES the snapshot with
`zellij pipe`, and **that push carries no `--session`** (`bounded_pipe_command`).
So a drive that set `CLAVE_STATE_DIR` carefully wrote the right store and fired
the notification at the maintainer's live fleet. A 3-row sandbox snapshot was
aimed at a live 20-row session; only `apply_snapshot`'s `snap.seq <= self.seq`
discard stopped it landing, and that is an accident of which store held the
higher seq, not protection.

It fails silent in the direction that matters: the sandbox bar keeps rendering
its stale snapshot, so the feature under test looks broken. Both symptoms the
maintainer reported that round — the clock "stuck on 1h", and a decisive test's
ask never appearing — were this, not the feature.

Closed by `scripts/ct.sh --hook <Event> [payload-json]`, which asks the binary
for the state and data roots exactly as the session lookup does. Plus a
FOOTGUNS entry, because the flag's absence is invisible at the call site.

## The mutation run

**109 mutants over the diff, 102 caught, 7 unviable, 0 missed.** Run as two
`--in-diff` halves at `-j 1`; the whole diff was killed twice by the OS for low
memory with three worktrees and two sandboxes live. The only survivor anywhere
is `clave/src/main.rs`'s `fn main`, not a seam, predating this branch.

## Open — eyeball only, neither a blocker

- **crystalBlue is the rail's ink as well as the clock's.** The live drive read
  fine, but if it ever competes the fix is a distinct live ink, not a different
  cell.
- **The six spinner frames' weight and baseline.** Five come from Menlo by
  fallback, so they can sit differently from the first.

## Sandbox

`clave-test-triple-card` was left poked at, not clean: two extra tabs
(`nav-b`, `nav-c`) from the nav investigation, and two agents pruned from its
store by a direct `agents.json` edit. Both drives and both eyeball checkpoints
are done, so it can be killed by its explicit name
(`clave dev instance --field session`) whenever the maintainer is finished
with it.

## Deferred

Step 6, the scribe: tier 2 of `ask` (the agent's closing question, shortened)
and the live headline as a new top tier of the recap → ai-title → first-prompt
ratchet. Not part of this branch.

## Reference

- `docs/superpowers/specs/2026-09-08-triple-height-card-lock.md` — authoritative,
  §4.4 amended 2026-09-09
- `crates/clave-bar/src/card.rs` — both geometries, one `clip_to_cells` exit
- `crates/clave-bar/src/model.rs` — `turn_label` / `elapsed_label`
- `crates/clave-types/src/lib.rs` — `RowHeight::animates`, two callers that
  must agree
- `crates/clave/src/hook.rs` — `wants_from_message`, `subagents_from_tail`,
  `model_from_tail`
- `scripts/ct.sh` — both doors
- `docs/dev/TESTING.md` § the sandbox drive loop
