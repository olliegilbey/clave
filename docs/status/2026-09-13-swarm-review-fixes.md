# 2026-09-13 — the branch review, and the five defects it found

Branch `worktree-triple-card`, PR #259. Continues
`2026-09-12-card-pr-handoff.md`, which is still the record of what the card
PR contains. This file is only the review round on top of it.

## What ran

A twelve-lane blind review of the whole branch (`git diff main...HEAD`, 52
commits, 80 files), one lane per house principle, then one verifier that
ruled on all 26 findings against the code. Lanes: simplicity, structure,
safety, tests, correctness, zellij-truth, observability, docs, scope,
staleness, legibility, artefacts. Observability and artefacts came back
clean. Verdict was **merge with changes**.

Two lanes reached the hook-guard hole independently, which is the strongest
signal the method produces.

## Fixed — four commits, gates green

- `88245c6` **the two script guards passed while their hazard was present.**
  The hook guard exempted any line starting with `check`/`measure`/`echo`;
  127 drive lines are `check "…" "$(…)"` and the substitution runs first, so
  `measure "x" "$("$CLAVE_BIN" hook Stop)"` narrated AND fired. The keystroke
  guard matched the literal `"$CT" write `, so `$CT write 13` matched nothing
  and could sit in any phase. Both now read structure, not source text. I
  injected each hazard and watched the test fail before trusting it.
- `31844a3` **the fast band.** Three faces, one machine. (a) `fast_armed` was
  a shell bool mirrored into the model, held together by a text test. (b) It
  was cleared only inside the fast band, so one 0.2s expiry the host reports
  long stranded the spinner and turn clock for the life of the instance. (c)
  `arm_spinner` took the whole fleet where the paint draws a viewport slice.
  The model owns the flag now, stops believing a claim after 2s, and
  `render::visible_rows` is called by both the paint and the arm. The clock
  went with it: `animates_now()` = geometry AND focus, one predicate for the
  timer and the reading, because a bar can become visible while still
  believing it is not focused (stock Alt+h/Alt+l carry no beacon) and it
  printed a frozen `7s` through a whole turn.
- `533d014` **the store write-back.** NOT fixed — see Open below.
- `d53b3cc` **stale defaults, counts and the phase list.** Both `Double`
  fallback comments, TESTING.md's self-contradiction on which phases are
  scripted, the showcase inventory, the seven-card scenario, the preview's
  hardcoded 48, and `just launch` failing with the staging command instead of
  "no such file".

## Open — decisions, not tasks

- **The lenient read is written back, and that erases the user's choice.**
  `with_store_mut` is read-modify-write, so the default an older binary
  substituted for a `row_height` it cannot name is persisted by the next hook
  of any kind. Not a one-session misread — an erase. Same for `order`,
  `status`, `label_source`. The fix is keeping the raw spelling across the
  round trip, which needs raw text held, and clave-types carries serde and
  nothing else at runtime (invariant #9). **That is Ollie's call, not a
  patch.** Until then the doc is honest, FOOTGUNS records it, and
  `store.rs::a_guessed_row_height_is_written_back_over_the_users_choice` pins
  the cost so the fix flips an assertion.
- **CodeRabbit's `swap_owed` item, declined with a reason.** It wants the
  count drained only on a fast expiry, not on any. The concern is real (a
  peek expiry inside the deafness sinks a tick early, buying an extra
  SwapWidth). But draining on every expiry is what stops a misclassified
  expiry stranding the width machine — the verifier used exactly that to
  refute a separate finding. Making the change safe needs the width ask to
  be self-healing the way the spinner now is, in machinery this branch has
  already reworked twice. **Needs a live drive to settle.** Do not change it
  from reasoning alone.
- **Legibility, downgraded to plausible, still worth a ruling.** `NeedsYou`,
  `Done` and `Idle` all render the same filled dot, separated by ink only.
  The table predates this branch and the card ADDS the first textual tell
  (`wants`) — but `wants` does not survive the 16-column crop, so on the
  collapsed card colour is the whole signal. Giving `NeedsYou` its own shape
  is the fix the lock already sanctions for Failed/Stale/Dormant/Opening.
- **Branch cell truncates the tail.** `fix/evlog-shred` and
  `fix/evlog-backfill` both render `fix/evlo…`. Front-truncation keeps the
  shared prefix and cuts what discriminates.

## Not fixed, filed as known minors

From the verifier, in rank order, none blocking: the isolation witness is
guarded by a `DRIVE.contains` that matches a comment (use `code_lines`); the
fade ladder is in three copies; `P6_BARS0` is a masked read (and
`check_numeric` cannot catch it — `bar_loaded_count` always prints an
integer, so probe `$ZLOG` readability instead); `Provenance` is flattened to
a char and matched back by glyph comparison; the phase-5c write guard is
decided once for three writes; three files quote "68 tests skipped" where
it is now ~730; the PR template asks for three gates where `just gates` runs
four; the AGENTS.md rewrite rode in on a card PR unmentioned in the body.

## Blind spots — nobody looked at a terminal

No sandbox ran this session. Unverified by eye: the card in the shipping
fonts with three marks stacked in one column; the 16-column card as a usable
artefact; whether the spinner reads as thinking or as noise; drive phases
3–7 on this branch; `just mutants` over the final `card.rs` and `lib.sh`.
**The three fixes in `31844a3` are reasoned, not measured** — if a sandbox
goes up, watch the spinner slice, the clock on a background tab, and the
fast-tick strand.

## Where I was

Pushed, and asked CodeRabbit for a fresh review of the four commits. Next
step is its reply, then fix what is valid. Its previous round (through
`d4c53a1`) had 10 comments; 9 are done, the tenth is the `swap_owed` item
above.
