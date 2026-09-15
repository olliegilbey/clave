# Status — restoring the live set across a relaunch

Worktree `.claude/worktrees/live-set-restore`, branch
`worktree-live-set-restore`, on `main`. It was built on `worktree-triple-card`
— the maintainer's call, since triple-card's sandbox isolation work had to
merge first — and rebased onto `main` once that landed as #259. Ten commits,
all four gates green, 754 tests, `just mutants main` at one survivor of 90 (`run_scenario`'s own
return, which only the drive exercises).

The design is `docs/superpowers/specs/2026-09-11-live-set-restore-design.md`
(on `main`, untracked). Read it first; this file is only what has changed
since it was written.

## The complaint being fixed

Quit clave, run `clave` again, and every previously-live row comes back
dormant — you navigate to each and press Alt+Enter. Measured on the real
store: 185 rows, 11 live.

## What is built

1. **`Store::last_live`** — the previous session's live set, recorded by
   `store::clear_session_order` a beat before it clears the session-scoped
   tab/pane binds. Written unconditionally: quitting with nothing open must
   leave an EMPTY set, or a conditional write would resurrect the layout from
   two launches ago. A SET, written in ascending tab id only so the file is
   deterministic.
2. **`setup::restore_rows`** — the read side. Drops pruned rows and vanished
   cwds, then RANKS what is left with `clave_types::sort_live_block`, the comparator
   the bar itself uses.
3. **`setup::launch_layout_kdl` takes a slice, not an Option** — one tab per
   restored row. Only the FIRST runs; the rest are created held
   (`start_suspended`, which zellij parses as `hold_on_start`).
   `add::tab_node_bare` gained `focus` and `held` parameters. Empty set falls
   back to the single eager row, unchanged and still fatal on a bad cwd; across
   a restored set a bad cwd is dropped and logged instead.
4. **`model::run_held_effect`** — the bar instance whose tab the human lands on
   emits `Effect::RunHeldPane`, and `main.rs` calls `rerun_command_pane`.
   `PaneMeta` gained `is_held`.
5. **`model::spawn_binds`** — the row-to-tab join for a tab whose command has
   not run. See the findings below.

## Five findings that shaped it — do not re-derive

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
- **Alt+1..9 indexes rendered ROWS, not the zellij tab strip.** The strip is
  not the interface, which is why the bake order is free to be the rank.

## Two guards in the bar that look optional and are not

- `!exited` — zellij's held flag also means "this ran, it exited, press ENTER
  to re-run". Without the check, walking past a quit agent's tab resurrects it.
- The command must parse as our binary and our subcommand (`model::spawn_uuid`).
  Any `zellij run` command pane waiting to be re-run reports held too.

## The three findings, and what they cost

No gate caught any of them: two came from the maintainer looking at a
screenshot, one from CodeRabbit. All three are fixed.

**The rows split in two.** Every row-to-tab question the bar asks reads the
store's bind, which `clave bind` writes when a spawn RUNS. A restored tab is
by definition the state where that command exists and has not run, so no bind
can exist. Each cold tab rendered as a TERMINAL row showing a raw
`clave spawn <uuid>` line, and its agent rendered a SECOND time as dormant.

The pane's launch command names the uuid, so that is the join now
(`spawn_binds`, cached off the two frames because `rows()` asks once per tab
and once per agent). Deliberately NOT gated on `is_held`: a command pane keeps
its launch command after the process execs, so the same join covers the beat
between the human starting a tab and the bind landing. `is_held`/`exited`
gate the ACTION, where starting something is at stake.

**The wrong row started.** The layout baked `last_live` in its stored order —
ascending tab id, i.e. tab CREATION order — and focused the first tab. That is
not the order the human saw. So the one row a relaunch pays ~350 MB for could
be any row in the list. `restore_rows` now ranks before baking.

That ranking is only possible because `buckets` and `commit_ord` are
**agent-scoped**. Every tab-scoped twin the bar would otherwise use
(`tab_order`, `tab_buckets`) dies with the session. And it only works at all
through the `spawn_binds` join above: a cold tab with no agent attached scores
nothing and sinks to the bottom of the live block.

**The ranking had a second layer.** (CodeRabbit, #261.) The bar ranks live rows
in TWO layers: rows cluster by `repo_root`, and the clusters rank by summed
member score. `restore_rows` reproduced only the inner one, so a repo holding
several middling rows — which outranks another repo's single better one in the
bar — lost on the host. Since launch runs the first row, a relaunch started an
agent that was not the one on top.

The lesson is the one the first copy already taught: **do not mint a second
copy of a ranking rule.** The comparator now lives in
`clave_types::sort_live_block` and both surfaces call it, beside
`frecency_millis`, whose doc had already written down why ("one function, so
the two surfaces can never disagree"). Anything else that ever needs to rank
clave rows goes there too.

## The drive — DONE, twice, both live findings above came from it

`just sandbox relaunch-restore`, scenario in `crates/clave/src/dev.rs`.

**One launch, not two.** The store learns the live set from rows that still
carry their dead session's tab binds — exactly what a killed session leaves
behind — so the scenario stages rows *already bound to tabs* and a single
launch exercises the real capture path end to end.

**The fixture stages recency AGAINST tab order on purpose.** Tab ids are
0, 3, 4, 9 (non-contiguous: gaps are what a real session accumulates) in slug
order a, b, c, d, but the ages make the rank d, b, c, a. Staged in agreement,
a relaunch that simply baked the tab strip would pass.

Verified on screen and by `ps`, 2026-09-14: rows read d, b, c, a; restored-d
selected; exactly ONE claude process in the sandbox; it resumed
`…c85c00000054`, the ROTATED conversation rather than the minted `…0004`
(so launch now exercises #99's resume choice, which used to happen only on
navigation); both dormant rows present with no tabs, including `dormant-e`,
the most recently touched row in the fixture.

The maintainer's eyeball verdict: "this is a UX improvement overall", and
"that seems to work perfectly now".

## Deferred, by agreement

Preemptive warming: the neighbour rule, a few more by frecency
(`clave_types::frecency_millis`), knobs as constants with environment
overrides, and a free-memory check. Its own spec, after this is in daily use.

## Not done

The stress fixture (`~/code/clave-spike/fixtures/medium-2.8mb.jsonl`, 419 KB
compressed) is NOT commit-safe: timestamps and session ids are still real.
Offset and regenerate before it goes in. Copy it per tab at drive start —
resume appends to a transcript in place. Nothing on this branch needs it; it
is for the warming work, which has to know what a real resume costs.

## Open risks

- A restored row whose conversation was deleted resumes into nothing. Today's
  single eager tab has the same exposure; eleven rows multiply it.
- The spike never authenticated, so the cost of the FIRST message after a
  resume — the one thing that does scale with transcript size — is unmeasured.
  It does not affect v1, which starts agents one at a time, but it must be
  measured before any warming wave is trusted.
- A held pane shows zellij's "press ENTER to run" line until started. Never
  actually seen in either drive — the cold tabs looked like ordinary terminal
  tabs — but nothing suppresses it, so it may appear under other geometries.
