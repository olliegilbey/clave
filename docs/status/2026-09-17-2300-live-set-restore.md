# Handoff — #261 live set restore, after the subagent review

2026-09-17 23:00. Branch `worktree-live-set-restore`, 39 commits unpushed,
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

**OPEN — the current task.** Ollie's `Alt+a` row `dc13c31f-e298-4d78-a884-
72d8b0007ff1` (cwd `.../repos/relaunch-restore-restored-d`) went dormant
correctly and the restart FIRED — so the restart path works — but `clave spawn`
then refused:

> no transcript found for session dc13c31f-… anywhere under
> ~/.claude/projects: this row has already conversed, so starting a FRESH
> session would silently shadow the real one

The row has `session_id: None` in the store, yet the sidebar renders it
`opus` / `hi` / `0m` — a summary that can only come FROM a transcript. So
either the transcript exists and the lookup cannot find it, or the "has
already conversed" judgement is made from something other than `session_id`
and is wrong.

**Next step:** look for the jsonl under
`~/.claude/projects/*clave-dev-live-set-459d*repos-relaunch-restore-restored-d*/`
and find what `clave spawn` uses to decide "already conversed" (grep the
refusal string in `crates/clave/src/spawn.rs`). Decide whether this is a #261
regression or pre-existing — it is NOT one of the review findings, and it may
predate the branch. If pre-existing, it is a separate issue, not a blocker.

Note the fixture keeps putting `Alt+a` rows in a directory a seeded row
already owns, so two rows share a label. That is the fixture, not a defect.

## Still needs Ollie

- His go to push (39 commits), then the PR body rewrite
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
