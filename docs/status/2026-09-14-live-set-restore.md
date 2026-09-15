# Status — restoring the live set across a relaunch

Worktree `.claude/worktrees/live-set-restore`, branch
`worktree-live-set-restore`, on `main`. It was built on `worktree-triple-card`
— the maintainer's call, since triple-card's sandbox isolation work had to
merge first — and rebased onto `main` once that landed as #259. All four gates
green at HEAD.

There is no committed design spec. An earlier draft of this file pointed at
`docs/superpowers/specs/2026-09-11-live-set-restore-design.md` "on `main`,
untracked" — it is in neither the tree nor git, so nobody in a worktree or a
fresh clone can read it. This file is the record.

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
  active tab. **Corrected 2026-09-15:** that is true of the tab frame and NOT
  of the identity pass it was used to justify — a store snapshot re-enters
  `identity_effects` on every instance, so the start arm now rides the beacon
  (`own_tab_focused`), like every other arm that starts something.
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

**One launch, not two — and that choice is what hid the decay.** The store
learns the live set from rows that still carry their dead session's tab binds,
so the scenario stages rows *already bound to tabs* and a single launch
exercises the capture path. What a single launch cannot exercise is the
SECOND one, which is where the set is rebuilt from what the first session
actually recorded. The drive needs a relaunch phase (see TESTING.md's escape
record); until it has one, a re-drive of this feature must quit and relaunch by
hand, and check the set comes back the same size.

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

## The swarm review, 2026-09-15 — six more findings, all fixed

Seven blind lanes plus a verifier, adapted from the Olympus `swarm-review`
skill. No gate caught any of these either; the count of defects on this branch
that a human or a machine reviewer found, and no gate did, is now nine.

- **The restored set decayed.** `last_live` is built from tab binds, and a
  restored tab wrote no bind until its spawn ran — so the set recorded only the
  tabs the human VISITED. Ten rows restored, two worked in, quit: two rows
  back, then one. The complaint this branch fixes, arriving by a slower route.
  Proved with a temporary store test before fixing. A restored tab now reports
  its row at once (`model::restored_bind_effects`), which makes it the bound
  row it already is on screen; closing one still removes it, through the
  existing prune path. **This is why the second launch has to be driven.**
- **The start arm used the poisoned gate.** See the corrected finding above.
- **The ranking key still had three homes** (bar live, bar dormant, host), and
  the two crates had already drifted in shape. `clave_types::live_key` now.
  The cluster layer moved for this reason in #261; the key did not follow.
- **The binary matcher had two homes and the release arm had no test.** A
  release install is the only environment that bakes the versioned absolute
  path — a sandbox shims a bare `clave` — so the copy that mattered was the one
  neither the suite nor either drive reached. Now `clave_types::is_clave_binary`.
  The same parse also broke on an install path containing a space.
- **An emptied restored set launched nothing.** Emptiness was tested BEFORE the
  cwd filter, so a day when every restored cwd is rejected baked a bar and no
  agent — worse than the cold start it replaced. The whole bake decision moved
  to `setup::bakeable_rows`, beside its siblings, because `launch_session` is
  excluded from `just mutants` on the grounds that its pieces are each tested
  directly, and that had stopped being true.
- **Three documentation debts the project's own rules mandate:** the new terms,
  and two traps left in this file instead of FOOTGUNS.md. Both now written.

`just mutants main` then caught six survivors in the new bind's RETRY BUDGET —
the happy path and the confirm path had tests, the budget did not. That budget
is the whole reason an emitter of fire-and-forget subprocesses is safe to run
from an unelected instance, so it is now pinned: the retry ladder, the cap, and
the fresh budget a renumbered tab gets.

## The CodeRabbit CLI pass, 2026-09-15 — one more, and it was the budget again

Run locally (`coderabbit review --base main --committed`) because the remote is
rate limited. Five findings, all acted on; four were documentation. The tally of
defects on this branch that a reviewer found and no gate did is now **ten**.

**The restored bind's budget did not work.** The fix above gave the new leg a
retry cap, and two tests pinned it — and both tests passed while the cap did
nothing, because each exercises ONE leg alone. The shell runs two:
`settle_identity` calls `restored_bind_effects` and then `identity_effects`, and
`bind_effects` inside the second one does not only write `bind_sent` for the
uuids it emits. It walks EVERY agent and CLEARS the entry of each one with no
registered pane in `uuid_to_pane` — which is the restored leg's entire
population, since a restored row's spawn has not run. So the ledger was wiped
between passes and the cap never bit. Measured at **12 subprocesses against a
budget of 4**, and unbounded in principle: one `clave bind` per store advance,
for as long as the bind failed to land.

It only bites an ELECTED instance, because `identity_effects` returns early
otherwise — that is, the human standing on a restored tab in the window before
its spawn registers a pane. Narrow, but it is the exact fd-exhaustion class
(C5 rd 4) that `BIND_MAX_TRIES` exists to bound, and the budget is the whole
stated reason `Effect::BindRestored` is safe to emit ungated.

The ledgers are separate now (`BarModel::restored_bind_sent`), which leaves
`bind_effects`' ping-pong reasoning untouched. The doc comment that invited the
mistake said the two legs are "disjoint by construction"; that is true of what
they EMIT and false of what they CLEAR, and it now says so.

**The lesson is this branch's lesson for the third time:** a test that drives
one leg of a seam proves nothing about the seam. The red test is
`a_restored_binds_budget_survives_the_ordinary_bind_leg_running_beside_it`, and
what makes it different from its two neighbours is one line — it calls the
function the shell calls next.

The four documentation findings: the spec still said the launch layout goes to
a temp file (it is the stable `setup::launch_layout_path`); phase 6c asserted
"bound before its agent runs" for every restored row including the eager first
one, which starts at launch; the glossary's `held` row carried both meanings in
one entry, now split into **held tab** (ours) and **zellij held flag** (theirs);
and `run_of` in the kdl guardrail did not say why it recurses.

**The last survivor is closed too.** `dev::run_scenario` could be replaced
wholesale with `Ok(())` and every run stayed green, because the drive is
validated by running it and no unit test reaches past the first line. The
honest answer was a test, not an `exclude_re` entry: everything after the
scenario lookup resolves a real sandbox and writes to disk, but the lookup
itself returns early, so an unknown name is reachable and worth pinning on its
own merits — a mistyped name reaching `just qa` must stop the run, not stage
nothing and leave the maintainer waiting at a launch prompt for a session that
will never make sense.

## The live relaunch, 2026-09-15 — the fix did not work, and now does

Driven for real (`just sandbox relaunch-restore`, maintainer launched, nothing
touched for 15 s). Everything except the bind was right: four tabs, ranked
d/b/c/a, the top row eager and resuming the ROTATED conversation, the other
three genuinely held with their spawn commands baked. **One bind was written.**

A bar resolves its own tab id through the tab frame, and zellij delivers that
frame only to the FOCUSED tab — the beacon exists in `main.rs` precisely
because pipes broadcast and frames do not. So the per-tab leg could only ever
run on the tab the human was already looking at, which is the one case needing
no help. The feature could not fire in the only situation it was built for.

The elected bar has what the others lack: the pane manifest is GLOBAL, so it
sees every held pane and the uuid in its command, and its tab frame carries the
whole tab list, so it can turn each pane's position into a tab id. It now
reports for every held tab. Only the position-to-id step crosses frames, and
`frames_coherent` — already required by the election — is the guard for exactly
that. The ungated `Effect::BindRestored` is deleted: being elected is what the
shell's ordinary bind gate tests, so the leg rides it like everything else.

**Why no tier caught it, and this is the third time on this branch.** Every
test handed a bar in an unfocused tab a tab frame that a bar in an unfocused
tab never receives. The fixture's own doc recited the rule — "zellij delivers
TabUpdate only to the active tab" — and then built a feature for unvisited tabs
on top of it. Both traps are in FOOTGUNS.md now. The check that found it took
about a minute: launch, touch nothing, count the binds in the store. That is
phase 6c, and it is worth implementing rather than leaving as prose.

**Re-driven live, 2026-09-15, and the relaunch loop closes.** Launch one: four
restored tabs, nothing touched for 15 s, FOUR binds in the store (`seq` 15 to
18 — exactly three subprocesses for three held tabs, so the budget does not
over-fire). Killed. Launch two: baked all four again in the same rank order,
and rebound all four without a tab being visited. Write four, quit, read four,
rewrite four — so the set is now stable across relaunches instead of decaying
toward one. The three held rows carry a `tab_id` with NO `pane_id`, which is
the new leg's whole signature: a row recorded before its spawn has run.

**State at handoff:** thirteen commits on top of the original nine, all four
gates green, and the relaunch seam verified end to end for the first time.

The open question for the next live drive is still cost, not correctness: a
relaunch fires one `clave bind` per restored tab in the first seconds (eleven
rows, eleven short subprocesses, each an RMW under the store's flock). Nothing
measures that yet.

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
