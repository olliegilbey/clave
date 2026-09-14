# Status — restoring the live set across a relaunch

Worktree `.claude/worktrees/live-set-restore`, branch
`worktree-live-set-restore`, **based on `worktree-triple-card`, not `main`**
(the maintainer's call: triple-card's sandbox isolation work merges first).
Three commits, all four gates green, `just mutants worktree-triple-card`
clean — 32 mutants, 29 caught, 3 unviable, **no survivors**. Nothing pushed,
no PR.

The design is `docs/superpowers/specs/2026-09-11-live-set-restore-design.md`
(on `main`, untracked). Read it first; this file is only what has changed
since it was written.

## The complaint being fixed

Quit clave, run `clave` again, and every previously-live row comes back
dormant — you navigate to each and press Alt+Enter. Measured on the real
store: 185 rows, 11 live.

## What is built

1. **`Store::last_live`** — the previous session's live set, agent uuids in
   screen order, recorded by `store::clear_session_order` a beat before it
   clears the session-scoped tab/pane binds. Written unconditionally: quitting
   with nothing open must leave an EMPTY set, or a conditional write would
   resurrect the layout from two launches ago. `setup::restore_rows` is the
   read side (drops pruned rows and vanished cwds, keeps order).
2. **`setup::launch_layout_kdl` takes a slice, not an Option** — one tab per
   restored row, in screen order. Only the FIRST runs; the rest are created
   held (`start_suspended`, which zellij parses as `hold_on_start`).
   `add::tab_node_bare` gained `focus` and `held` parameters. Empty set falls
   back to the single eager row, unchanged and still fatal on a bad cwd; across
   a restored set a bad cwd is dropped and logged instead.
3. **`model::run_held_effect`** — the bar instance whose tab the human lands on
   emits `Effect::RunHeldPane`, and `main.rs` calls `rerun_command_pane`.
   `PaneMeta` gained `is_held`.

## Four findings that shaped it — do not re-derive

- **A resume costs ~350 MB resident regardless of transcript size** (226 MB
  for a fresh session; an 18 MB transcript is 356 MB, and 90% of that gap is
  bought by the first 800 KB). **Staggering saves no memory** — sessions never
  exit. Only the tab count matters. This is why v1 is lazy and has no cap.
  Measured 2026-09-11; harness and numbers in `~/code/clave-spike/`.
- **The memory cliff is real.** 8 concurrent resumes is comfortable on this
  machine (18 GB, ~5 GB typically free); a 16-way run started with 4.1 GB free
  took 17.3 s instead of 3.5 s.
- **Receiving a TabUpdate IS the focus signal.** zellij delivers it only to the
  active tab. No new signal was needed — the effect rides `identity_effects`,
  the existing coherence+active election.
- **`add::live_uuids` already matches the baked `spawn` args form**, for the
  pre-exec window. A held pane is that window standing still, so a commit on a
  restored row already finds its tab instead of opening a second one. **No
  liveness change was needed and none should be added.**

## Two guards in the bar that look optional and are not

- `!exited` — zellij's held flag also means "this ran, it exited, press ENTER
  to re-run". Without the check, walking past a quit agent's tab resurrects it.
- The command must parse as our binary and our subcommand (`model::spawn_uuid`).
  Any `zellij run` command pane waiting to be re-run reports held too.

## Next: the sandbox drive — NOT yet started

The maintainer's instruction was "push on and drive both together", i.e. drive
the layout and the bar in one go. Nothing has been staged or driven.

**The drive needs one launch, not two.** The obvious shape is: come up,
establish a live set, quit, come up again. It does not need that. The store
learns the live set from rows that still carry their dead session's tab binds
— exactly what a killed session leaves behind. So a scenario can stage rows
*already bound to tabs*, and a single launch exercises the real capture path
end to end.

That needs, in `crates/clave/src/dev.rs`:
- a new field on `ScenarioAgent` for the tab it held (there is none today —
  every existing scenario seeds dormant rows only), and
- a new scenario seeding perhaps four bound rows.

Then the assertions: N tabs on screen in the staged order, **exactly one
`claude` process**, the rest of the panes held; navigate one tab down and a
second `claude` appears. That last pair is the whole feature.

Use the triple-card drive harness — `scripts/ct.sh`, the env scrub, and
`hook.rs::aim_push`, all added after the 2026-09-11 incident where a drive hung
the maintainer's live session. `docs/status/2026-09-11-drive-isolation.md` is
the account. **Launching is the maintainer's**; print the command.

The stress fixture (`~/code/clave-spike/fixtures/medium-2.8mb.jsonl`, 419 KB
compressed) is NOT commit-safe yet: timestamps and session ids are still real.
Offset and regenerate before it goes in. Copy it per tab at drive start —
resume appends to a transcript in place.

## Deferred, by agreement

Preemptive warming: the neighbour rule, a few more by frecency
(`clave_types::frecency_millis`), knobs as constants with environment
overrides, and a free-memory check. Its own spec, after this is in daily use.

## Open risks

- A held pane shows zellij's "press ENTER to run" line until started.
  Cosmetic, visible, unaddressed.
- A restored row whose conversation was deleted resumes into nothing. Today's
  single eager tab has the same exposure; eleven rows multiply it.
- The spike never authenticated, so the cost of the FIRST message after a
  resume — the one thing that does scale with transcript size — is unmeasured.
  It does not affect v1, which starts agents one at a time, but it must be
  measured before any warming wave is trusted.
