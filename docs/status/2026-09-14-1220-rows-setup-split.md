# 2026-09-14 12:20 — the second bar after the 0.5.0 cut

Live on Ollie's fleet, after `just release` (v0.5.0) and `clave rows card`.
**Not fixed. Diagnosed from disk only — no session touched.**

## Symptom

A cold start draws CARDS in the launch pane (correct) and a SECOND bar pane
beside it drawing DOUBLE rows. `Alt+Enter` no longer opens a tab.

## What the artifacts say

| File | Written | `row_height` |
|---|---|---|
| store `~/.local/state/clave/agents.json` | 12:1x | `card` |
| `launch.kdl` | 12:20 (launch) | `card` |
| `config.kdl` | 11:51 (`just release`) | `double` |
| `layout.kdl` | 11:51 (`just release`) | `double` |

Zellij keys a plugin instance by location AND its `configuration` map. The
keybinds in `config.kdl` carry `row_height "double"`; the running pane was
launched with `card`. They are therefore two different plugins at one
location, so the first `Alt+…` press STARTED a second bar, and every nav
message goes to that new instance instead of the real one. `Alt+Enter` is
not broken — it is talking to the wrong bar. This is the class
`main.rs:679` already documents (review finding 1, #232).

## Root cause

`clave rows card` does two things in order (`main.rs:659-692`):
`store::set_row_height` FIRST, then `setup::run_setup()` to regenerate
`config.kdl` + `layout.kdl` from the store it just wrote.

The store write landed. The regeneration did not: both files still carry
the release's 11:51 mtime, and **`clave.log` has no `rows` event** — that
line runs after `run_setup()?`, so `run_setup` returned an error and the
command aborted half-done. The error text was not captured.

`write_generated` (`setup.rs:904-917`) writes both files unconditionally
from the store, so a successful `clave setup` is all that is needed.

## Next actions

1. Ollie runs `clave setup` and reports the output. If it errors, that
   error is the whole bug. If it succeeds, `config.kdl` and `layout.kdl`
   get `card` and the split is closed.
2. Kill and cold-start: `zellij kill-session clave` /
   `zellij delete-session --force clave` / `clave`. Keybinds are read at
   session start.
3. Then the eyeball checks this branch has never had.

## The real fix, once the error is known

`clave rows` is not atomic: it persists the mode, then regenerates. A
failure between the two leaves exactly this split, and the tell is a
second bar. Either regenerate BEFORE the store write, or roll the store
back when regeneration fails. FOOTGUNS entry to follow.

## Branch state

`chore/release-0.5.0` — the 0.5.0 bump + cut record. Local only, no tag,
nothing pushed. Cut record: `docs/status/2026-09-14-0.5.0-cut.md`.
