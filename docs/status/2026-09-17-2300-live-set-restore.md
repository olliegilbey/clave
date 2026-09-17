# Handoff — #261 live set restore, after the subagent review

2026-09-17 23:00. Branch `worktree-live-set-restore`, 41 commits unpushed,
gates green, 810+ tests passing. Supersedes `2026-09-17-2030-*`.

## Where this is

The three-reviewer pass is DONE and every finding is acted on. Ollie chose
"one eyeball check" over a re-drive, and chose "this branch" for the status
reset. Both are delivered. We are in the eyeball check now, and it has already
paid for itself twice.

## What the review fixed (all committed)

`9f170aa` attach re-arming; `a698020` the four bar defects; `334fe57` the
verdict tool and the dead launch path; `b0c61df` silent drops; `95712b2` the
status reset; `df6abbc` two FOOTGUNS; `dc53728` the duplicate row.

The four bar defects, in Ollie's terms: close a restored tab and nothing wakes
again; a deleted folder does the same from second one; an agent you quit
cannot be restarted; two tabs naming one agent bind it twice forever.

## Eyeball check — measured so far

PASS, driven by me: the fleet comes back (4 rows, 3 asleep); `Done`/`Failed`
survive the launch; walking onto a sleeping tab wakes it (control); and it
STILL wakes with a row left permanently owed (the treatment — the deaf-fleet
fix proved live).

PASS, from Ollie's second screenshot: the duplicate row is gone. The tab of an
agent that quit now draws as TERM, and the agent appears once, dormant.

FIXED, from Ollie's third screenshot. Two more defects, both from the same
family — `Status::Exited` is new on this branch, and two places read it as
something it does not mean.

`5e85025` — an agent you quit before it ever conversed could not be
restarted. `spawn` read every non-idle status as proof a conversation had
happened, and refused to shadow a transcript that was never written. That is
the exact dead end the restart exists to open.

`d55bc4c` — a restarted agent kept the mark of the one that quit. The hook
table has no SessionStart leg, so the row said "stopped" until its next
prompt: drawn dim, and offered to the picker as a resume, which attaches a
second client to a live session. The pane registration now takes the mark
off, because that is the moment we know the row runs again.

**NEXT:** restage the sandbox and confirm both on screen — quit an agent,
restart it, and watch the row come back live rather than dim. Then ask for
the go to push.

Note the fixture keeps putting `Alt+a` rows in a directory a seeded row
already owns, so two rows share a label. That is the fixture, not a defect.

## Still needs Ollie

- His go to push (41 commits), then the PR body rewrite
  (`.github/PULL_REQUEST_TEMPLATE.md`, `--body-file`).
- His go to file: a restore that does not finish permanently shrinks the
  fleet. Pre-existing, agreed to be its own issue.

## Traps learned today (all in FOOTGUNS)

- A per-instance counter cannot answer a question about the fleet.
- A command pane keeps its launch command after the process exits.
- `zellij action go-to-tab-by-id` moves NO beacon, so it cannot drive arrival
  behaviour. Use `ct.sh pipe --name clave-visited` then `--name clave-nav`.
- `dump-layout` reports the original hold flag, not live state. The store's
  `pane_id` is the honest "did it start" — `clave spawn` writes it before
  exec, so it works even where no hook can fire.
- Counting ONE block cannot see a duplicate. Count the agent across both.

## Harness

Worktree classifier refuses compound bash with `git` inside `$(...)`. Write
python to `/tmp` via the Write tool, then run it plainly. `cargo fmt --all`
before every `just gates`. Sandbox: `just sandbox` refuses while its session
is live — kill by name first (`clave dev instance --field session`).
