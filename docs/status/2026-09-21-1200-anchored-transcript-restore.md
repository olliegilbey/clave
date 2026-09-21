# Status — 2026-09-21 12:00 — the launch resumes a session that entered a worktree

Branch `worktree-restore-cwd`, worktree `.claude/worktrees/restore-cwd`.

## The defect

The first launch after a session entered a worktree mid-conversation died in
its baked tab. The pane printed spawn's refusal ("transcript … lives under
project dir … but its tail names cwd …") and never registered, so the
sidebar's staggered restore (#261) waited on a bind that never came and the
rest of the fleet sat dormant. A click on the same row resumed it.

Diagnosed from `~/.local/state/clave/clave.log`, the zellij log and the
store alone, then measured: `verified_site` on the row's real inputs
returned the refusal; the same call from the repo root returned `Resume`.

## The cause, measured

Claude keys a transcript's project dir on the cwd where the session
STARTED, and keeps it there when the session walks on. Over 338 transcripts:
10 sit under their last cwd (a relocation, #59/#69), 12 sit under their
first while their tail names somewhere else (`EnterWorktree` and a plain
`cd` alike), none under neither. None of the 12 carries a `relocated` or
`worktree-state` line. FOOTGUNS said relocation was the only behaviour.

The hook (`take_checkout`, 2026-09-15) moves the row's cwd to the tail. The
launch (#261) baked that cwd bare. `clave open` asked `relocated_cwd` first
and, through the OLD minted transcript's tail, baked the repo root — luck,
not a rule.

## The fix

- `SpawnSite::Anchored`: `moved_site` now checks the head (`head_cwd`, new
  `hook::read_head`) when the tail does not key the file. Resume from the
  birth dir; do NOT repoint the row (the hook would move it straight back).
- One rule for a spawn pane's dir: `open::pane_cwd` (pure half
  `pane_cwd_in`), called by `run_open` and by the launch bake in
  `setup::launch_session`. `launch_layout_kdl` stays pure — the caller
  clones the one record with the resolved cwd.
- Docs: FOOTGUNS entry under the relocation one; two vocabulary terms
  (relocated / anchored transcript); two rows in TESTING.md's
  "real behaviour nobody pinned".

Tests red-first: `a_session_that_walked_into_a_new_dir_resumes_where_its_file_is_keyed`,
`the_anchor_is_the_first_cwd_line_of_the_transcript`,
`an_anchored_transcript_with_a_vanished_birth_dir_fails_loudly` (spawn.rs),
`a_pane_is_born_where_the_conversation_went_else_at_the_row` (open.rs).

## Verified

- `just gates` green: 444 host + 340 bar tests, wasm build, clippy `-D warnings`.
- A throwaway probe ran `verified_site` on the maintainer's real row and
  transcript: `Anchored { cwd: ~/code/clave }`. Probe deleted.
- `just mutants` over the diff: see PR.

## Not verified

- No live relaunch. The sandbox's Claude dir cannot hold the maintainer's
  anchored transcript, so the proof is the next stable launch after a
  release that carries this change.
- The launch still drops a row whose cwd is gone (`restore_rows` filters on
  `is_dir`) where `open` would recover it through relocation. Unchanged;
  the hook repoints such rows on their next event, so the window is small.

## Related, not done

- The staggered restore is gated on the baked row binding: one row that
  refuses stalls every deferred row. Worth its own issue with the
  "a restore that never finishes shrinks the fleet" one from #261.
- zellij 0.45.1 (brew, 18 Sep) logs "Action CliPipe did not complete within
  1s timeout" on every hook pipe. Registrations still land. Not looked into.
