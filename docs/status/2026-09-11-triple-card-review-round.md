# Status — PR #259 review round, all six findings closed

Branch `worktree-triple-card`, worktree `.claude/worktrees/triple-card`.
PR https://github.com/olliegilbey/clave/pull/259 — rebased onto `origin/main`
(2f7fdb5, #258) cleanly, four gates green after the round.
Supersedes `2026-09-10-1900-triple-height-card.md` for the review round only;
that file is still the record of the feature itself.

## The six CodeRabbit findings and what each became

| #   | id         | where                       | verdict                          |
| --- | ---------- | --------------------------- | -------------------------------- |
| 1   | 3992106550 | `card.rs:142`               | taken as proposed                |
| 2   | 3992106582 | `main.rs:966`               | **valid, and it undercut my own timer fix** |
| 3   | 3992106585 | `plugin_config.rs:30`       | taken                            |
| 4   | 3992106592 | `clave-types/src/lib.rs:61` | valid, same bug one pipe over    |
| 5   | 3992106602 | spec `:133`                 | both halves verified, both fixed |
| 6   | 3992106610 | `qa-drive.sh:390`           | valid, closed differently        |

### 2 — the fast band now holds ONE timer

The claim was right. `Event::Timer`'s fast leg clears `anim_armed` on ANY
expiry under `TIMER_KIND_CUTOFF_SECS`, because it cannot tell whose expiry it
is — so a width-cooldown expiry disarmed a spinner whose own frame was still
pending, and the next `render` armed a SECOND one. Two pending frames spend
both of an ask's owed ticks inside one 0.15s deafness (0.05 + 0.10), which is
the bug `swap_owed` was added to prevent. My count rested on "at most one
foreign fast timer exists", and that was simply false.

Fixed by removing the ambiguity rather than classifying it: **one timer in the
fast band.** `FAST_TICK_SECS` (0.2s) is now the spinner's frame clock AND the
width machine's deafness clock, armed through one funnel (`arm_fast_tick`) by
whoever wants a tick — a working row, or an ask still owed one. The shell keeps
one `fast_armed` bool, mirrored into the model as `fast_tick_armed`.

What survives from the first pass: `swap_owed` is still a COUNT (an ask made
while a tick is in flight cannot trust that tick, whose remaining time is
anywhere in (0, 0.2]), and the model still asks for its own second tick
(`Effect::RearmWidthCooldown`) so the count cannot strand when the spinner
stops. The difference is that the count is now EXACT, because there is provably
one timer to count. `WIDTH_COOLDOWN_SECS` stops being an interval anything arms
and becomes the documented FLOOR, still held by a const assert
(`WIDTH_COOLDOWN_SECS <= FAST_TICK_SECS`).

**Guarding a shell that does not link.** `main.rs` is the wasm plugin and
cannot be unit-tested (`[[bin]] test = false`), so the one-timer property had
nothing watching it — which is how the first pass shipped. It is now guarded as
SOURCE TEXT by `clave-bar/src/lib.rs` mod `shell_text`, two tests over
`include_str!("main.rs")`: the fast tick is armed from exactly one place and
that place is `arm_fast_tick`, and every write to `fast_armed` is followed by
the matching mirror write. In the lib's own tests rather than `tests/`, because
an integration test makes cargo build the bin and that build is the link
failure `test = false` exists to avoid. Both were confirmed to bite by breaking
`main.rs` on purpose and watching them fail.

Rejected: keeping two timers and making the anim leg count too (same ambiguity,
twice the state); a fourth classifier band (no room — 0.05s of slack); tagged
timers (zellij 0.44.3's `Timer` carries only elapsed seconds).

### 4 — the same leniency bug, one pipe over

`AgentRecord.status` got `lenient` this branch; the WIRE types did not. A newer
host naming a `Status` this bar cannot spell made `clave-bar` reject the whole
SNAPSHOT — the same fleet-goes-inert failure as the `card` row-height variant,
reached through `clave-status` instead of through the store file.

`lenient` moved into `clave-types` so both sides share one definition, and it
is now applied to `Agent.status` and `AgentSnapshot.order` (the snapshot's
other enum, which rides every push, so a wrong guess there cost every update
rather than one row). `RowHeight` is NOT on the wire — it is launch-baked into
the KDL and read from plugin config — so it needs nothing.

**The implementation changed in the move.** The store's version buffered
through `serde_json::Value`, and `clave-types` carries serde and nothing else
at runtime (invariant #9). The shared version uses serde's own buffering
instead — an `#[serde(untagged)]` two-arm enum whose second arm is
`IgnoredAny`, which cannot fail. Same tolerance in both directions (an
unfamiliar spelling AND an unfamiliar shape), no new dependency. Three tests in
`clave-types`: an unknown status costs the field and not the snapshot, a known
status still arrives as itself (the tolerant arm must be second), and an
unknown `order` — spelled wrong or shaped wrong — keeps the dial we ship.

### 6 — closed by widening the check, not by narrowing the claim

The phase-0 preflight resolved `clave` from the DRIVE shell's PATH and then
vouched for it. The reviewer is right that the hook process does not inherit
that PATH, so the check could vouch for a binary no hook runs. Claude's
environment cannot be read from outside it, so the check no longer implies it
can: it now tests every `clave` a hook could plausibly land on — this shell's
and a clean login shell's (`env -i HOME=… bash -lc`), deduped — and requires
each to read the sandbox store. The limit is written into the comment in
those words: these are approximations of the hook environment, not the hook
environment. `shellcheck -S warning` clean; the dedupe was unit-checked against
all five input combinations.

### 1, 3, 5 — cheap and taken

- `THINK_CYCLE` is derived (`2 * THINK_FRAMES.len() - 2`) and the turn point is
  `THINK_FRAMES.len()`, not the literal `6`. The ratified-sequence test pins
  the result, so the derivation cannot drift silently.
- `resolve_row_height`'s doc says why a missing OR unfamiliar value lands on
  `Card`: the config is launch-baked, so the wrong answer is a geometry, and
  the geometry a fresh install draws (lock §1) is the one guess that cannot
  surprise.
- Spec §4.2's prose read as a claim about the shipped card; it was describing
  what shipped BEFORE the lock, now tensed that way and pointed at
  `render::WORKTREE_MARK`. Verified `\u{f1bb}` in the renderer and
  JetBrainsMono in `readme-assets.rs` before touching either. §6 no longer
  names FiraCode; it names the two fonts that actually decide a glyph — the
  terminal's own face and the asset workflow's pinned JetBrainsMono.

## Do not lose

- Local stable is **1.96.1**, CI's `@stable` is **1.98.1**, and
  `clippy::for_unbounded_range` landed between them — `just gates` went green
  on a lint that cannot exist here, and `lint` failed on the PR (fixed,
  01d3f46). **Open for the maintainer:** `rustup update stable` (changes the
  whole machine, other worktrees building) vs pinning CI to a version this repo
  declares. Worth writing into CONTRIBUTING beside the four gate commands
  either way.
- The sandbox `clave-test-triple-card` is **gone**; its socket outlived it and
  was left in place deliberately (see the 1900 handoff).
- Commit trailer: `Claude-Session: https://claude.ai/code/session_01FmBv3E9cVa8EynJQrTXTN3`

## Verification

Four gates green: 373 + 11 + 15 + 1 host tests, 284 bar tests (two new,
`shell_text`), 29 type tests (three new), clippy clean, wasm builds.

Mutation over the branch diff (`just mutants`): **153 mutants, 135 caught, 17
unviable, 1 missed** — `clave/src/main.rs`'s `fn main`, which predates this
branch and was already recorded as a non-seam. `push_snapshot` and
`own_session` no longer surface: both are excluded by name in
`.cargo/mutants.toml` with their reasoning. Worth noting where the new code
landed: both arithmetic mutants over the derived `THINK_CYCLE` came back
UNVIABLE, because the ratified-sequence test types its literal as
`[char; THINK_CYCLE]` — changing the cycle is a compile error, not a failing
assertion.

## The QA drive, extended for the card

`scripts/qa-drive.sh` gained three things the card needs and no earlier phase
could reach. Docs updated in `docs/dev/QA-DRIVE.md`'s phase spine.

- **P0 — the launch-baked geometry.** One `row_height` across `launch.kdl` and
  `config.kdl`, equal to the store's; exactly two declared bar widths; the
  card's ratified 16/48 when that is the mode. The failure it catches is a
  STALE STAGE: a store asking for one geometry while the launched layout bakes
  another, which renders one geometry into another's pane with nothing saying
  so.
- **P5b — the card's cells, from the hook side (new phase).** Five real `clave
  hook` events against the real store and the real transcript reader: a
  permission `Notification` (→ `wants` from its own words, status red),
  `UserPromptSubmit` (→ the words leave with the ask), a `Stop` with a
  `turn_duration` tail carrying `pendingBackgroundAgentCount` (→ the mark
  rises), a silent `Stop` (→ the mark HOLDS), and `SessionEnd` (→ the mark
  clears, row idle). This is the seam two of this branch's defects lived at,
  and the one unit tests cannot see. It appends only to a `c85c` scenario
  transcript and asserts that before writing.
- **P5 — the width the fleet actually rests at.** The bug this branch fixed
  rests the bar at the wrong width, and every existing press check only proves
  the store's INTENT landed. The bar's pane size is now read from the layout
  dump in both settled modes and asserted **only if the two readings differ** —
  a number that moves with the toggle is the applied swap position; one that
  does not is the declared layout, and asserting on it would be a tautology, so
  the phase-5 eyeball stays the oracle in that case. When it does move, it also
  automates that eyeball's own question: one width per mode across the whole
  fleet, no outliers.

Still eyeball-only, deliberately: the spinner's motion and the five fallback
frames' weight, and the card's actual text (the bar pane is unselectable, so
`dump-screen` cannot reach it).

## Next step

Stage and drive: `just sandbox qa-fleet`, the maintainer launches, then
`scripts/qa-drive.sh qa-fleet` — phases 0 through 7 with 5b in between.
