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

## The fix (after the Opus review, commit 2)

- `SpawnSite::Anchored`: `moved_site` checks the tail's cwd RAW against the
  file's dir first (a relocation whose target vanished keeps its own
  message), then the head (`head_cwd`, read by line up to 1 MiB — 8 of 339
  transcripts have their first cwd past 64 KiB). Resume from the birth dir
  and REPOINT the row there: the agent wakes in the birth dir, not the
  worktree it left off in, and loses its tree mark. That is the truth, one
  hook event earlier than the hook would say it. Needs Ollie's ruling.
- `spawn::conversation_home` (`Home::Moved` / `Home::Anchored`) replaces
  `relocated_cwd`; the search stops at the live id once it resolves, so an
  anchored live conversation never falls through to the minted file.
- One rule for a spawn pane's dir: `open::pane_cwd_from` — relocated →
  target; else the row's cwd while it exists; else the anchor. Used by
  `run_open` and the launch bake (`setup::baked_home`, which re-runs
  `validate_cwd` on the substituted value and keeps the row's own cwd if
  it fails, so an unvalidated transcript cwd never reaches KDL).
- Docs: FOOTGUNS entry (limits recorded), two vocabulary terms, two rows
  in TESTING.md.

Review lane 1 (Opus subagent, adversarial): 11 findings, all taken.
Review lane 2 (CodeRabbit CLI, `--committed --base main`): 2 findings, both
taken — the home search now stops at the first id that HAS a transcript
(a live one that refuses no longer falls through to the minted file; test
`a_live_transcript_that_refuses_hides_the_minted_one`), and the stale
"leave the row alone" test doc was corrected.
Declined: none.

Tests red-first: `a_session_that_walked_into_a_new_dir_resumes_where_its_file_is_keyed`,
`the_anchor_is_the_first_cwd_line_of_the_transcript`,
`an_anchored_transcript_with_a_vanished_birth_dir_fails_loudly` (spawn.rs),
`a_pane_is_born_where_the_conversation_went_else_at_the_row` (open.rs),
`the_launch_bakes_the_pane_cwd_only_when_it_passes_the_kdl_guard` (setup.rs).

## Verified

- `just gates` green: 446 host + 340 bar tests, wasm build, clippy `-D warnings`.
- A throwaway probe ran `verified_site` on the maintainer's real row and
  transcript: `Anchored { cwd: ~/code/clave }`. Probe deleted.
- `just mutants` (commit 1): 14 caught, 7 missed, all misses env-reading shells. Rerun after commit 2 pending.
- Next: CodeRabbit CLI lane, then push on Ollie's go.

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
