# Status — the four-line card, review findings closed, opening the PR

Branch `worktree-triple-card`, worktree `.claude/worktrees/triple-card`.
Supersedes `2026-09-10-1130-triple-height-card.md`.

## Where it stands

**Done.** The feature was complete at the last handoff; this round is the
subagent review the maintainer asked for, and everything it found is fixed with
a test that fails without the fix. All four gates green: 706 host tests, 282 bar
tests, clippy clean, wasm builds.

## The defects, in the order they cost something

**The timer band collision — the only one that was a live bug in the
feature.** `Event::Timer` carries elapsed seconds and nothing else, so four
timer kinds are sorted by duration bands. The card's spinner (0.2s) was put in
the fast band beside the width cooldown (0.15s) on the reasoning that the
confusion was harmless both ways. It is not symmetric. A width expiry read as a
frame costs a spare repaint; a FRAME read as a width expiry ends the switch
deafness early, and its remaining time is uniform in [0, 0.2) against a 0.15s
cooldown, so it wins most toggles made while a row is mid-turn. The width
machine then judges a pre-swap echo — the exact paint the cooldown exists to
ignore — and burns one of the walk's three asks on it. Three is not spare: the
walk needs all three to cross the swap cycle's hidden birth position, so the bar
can rest at the wrong width until the next toggle. The common case, not a race.

Fixed without tagging timers. `swap_owed` replaces `swap_in_flight`: a count,
two when the shell's spinner timer was armed at the instant of the ask and one
when it was not. Whichever timer the first expiry belonged to, the second is a
full frame behind it, so the deafness always covers its cooldown. The model asks
for that second tick itself (`Effect::RearmWidthCooldown`) rather than waiting
on a frame — the spinner stops the moment the last turn ends, and an ask that
outlives it must not strand. The ordering the whole argument rests on is a
`const _: () = assert!(WIDTH_COOLDOWN_SECS <= ANIM_FRAME_SECS)`; lower the frame
interval and two expiries could both land inside 0.15s.

**The snapshot push had no aim.** `zellij pipe` without `--session` is resolved
by zellij's one-live-session arm, which ignores `ZELLIJ_SESSION_NAME` entirely.
The push looked exempt because it inherits the pane's env. It is not. This is
the defect that cost two rounds on the last drive, closed at the source rather
than only in `ct.sh`: `bounded_pipe_command` now names the session at every
wrapper rung, before the subcommand.

**`wants` contradicted its own row.** It rode straight off the wire per lock
§4.7. The wire is coherent — the host keeps `wants` coextensive with `NeedsYou`
— but the bar has four states that outrank the store's status: stale, opening,
dormant, dormant-selected. Each left the words behind, so a dormant row with no
process drew "waiting on Bash" beside a glyph saying otherwise. Now gated on the
projected `status`, the same distinction §4.4's clock makes two fields up.

**The subagent mark had no end.** It holds on a silent tail, correctly. Nothing
ended the hold, so an exited session kept its 🤖 — depth the user cannot go and
look at, wearing the glyph that says go and look. `take_subagents` is
`take_wants`'s twin and clears on `SessionEnd`.

**`split_once`, not `split(anchor).nth(1)`** in `wants_from_message`. They agree
until the anchor appears twice, where `nth(1)` returns the middle segment.

## Test-quality findings, all real

- **`provenance_wears_a_fixed_semantic_ink_not_the_rows` was the banned
  `contains(&INK.fg())` shape.** `PR_INK` and `WORKTREE_INK` are both
  springGreen, byte-identical, both on line 2 — so with a PR present it passed
  for every provenance including the one it rejects. Demonstrated: painting the
  worktree mark violet left it green. Rewritten off the mark's own SGR run, with
  a PR in the fixture. The probe is now a shared `ink_before` helper.
- **The spinner's ORDER was untested.** Shuffle `THINK_FRAMES` and every
  existing property survives. Order is the animation (§4.5); the ratified
  sequence is pinned as a literal from the spec and re-checked through
  `render_card`.
- **`SUBS_INK` was never asserted.** The mark could have arrived in any colour.
- **The hostile-text sweep only covered `render_double_card`.** The four-line
  card added three cells of agent-authored text — `wants`, `model`, `effort` —
  and `wants` is quoted verbatim from a notification. Swept across five hostile
  strings and five widths, including a leading wide glyph at the crop boundary.
  It holds.
- **A pre-existing hole, found mutating the timer fix.** The walk budget's
  intent guard could be replaced with `true` and all 277 tests stayed green:
  a failed walk's spent budget carried into the next intent, so every further
  toggle emitted nothing and the bar could not be un-stuck. Now tested.

## Mutation

Clean over everything this round touched:

| region | result |
| --- | --- |
| `width_effects` | 12 caught, 1 unviable, 0 missed |
| `width_cooldown_elapsed`, `set_animating` | 8 caught, 1 unviable |
| `take_subagents`, `bounded_pipe_command`, `session_from_env`, `wants_from_message`, `take_wants` | 16 caught, 1 unviable |
| `agent_content` | 5 caught, 1 unviable |
| `think_frame`, `render_card` | 54 caught, 3 unviable |

And over the whole branch diff: **150 mutants, 135 caught, 13 unviable, 2
missed.** Both misses are seams, not gaps. `clave/src/main.rs`'s `fn main`
predates this branch and was already recorded as a non-seam at the last
handoff. `push_snapshot` is fire-and-forget by contract — it spawns a child with
every stream nulled and drops it, so replacing the body with `()` is
indistinguishable from success to any host test; it joins `.cargo/mutants.toml`
with the same reasoning `spawn_pr_sync` carries, and every decision it makes is
mutated one layer down.

`own_session` joined the exclusions too — it reads process env, which the config
already forbids testing against. The rule it applies is split into
`session_from_env` and mutated there.

## Not reachable by a test, deliberately

`main.rs` does not link on the host (`[[bin]] test = false`), so the
`Effect::RearmWidthCooldown` arm cannot be unit-tested. Rather than leave two
arms one `set_timeout` apart, the two width effects share a single arm with the
swap made conditional — there is no second timer call to lose.

## Sandbox

`clave-test-triple-card` is still up and still deliberately dirty: two extra
tabs (`nav-b`, `nav-c`) and two agents pruned by a direct `agents.json` edit.
Both drives and both eyeball checkpoints were done at the last handoff, so it
can be killed by its explicit name (`clave dev instance --field session`)
whenever the maintainer is finished with it. Nothing this round drove it.

## Open — eyeball only, neither a blocker

- **crystalBlue is the rail's ink as well as the clock's.** If it ever competes
  the fix is a distinct live ink, not a different cell.
- **The six spinner frames' weight and baseline.** Five come from Menlo by
  fallback, so they can sit differently from the first.

## Deferred

Step 6, the scribe: tier 2 of `wants` (the agent's closing question, shortened)
and the live headline as a new top tier of the recap → ai-title → first-prompt
ratchet. Not part of this branch.

## Reference

- `docs/superpowers/specs/2026-09-08-triple-height-card-lock.md` — authoritative;
  §4.4 amended 2026-09-09, §4.5 and §4.7 amended 2026-09-10
- `crates/clave-bar/src/model.rs` — `swap_owed`, `width_effects`, `agent_content`
- `crates/clave-bar/src/main.rs` — the timer classifier and its const asserts
- `crates/clave-bar/src/card.rs` — both geometries, `ink_before`, `think_frame`
- `crates/clave/src/hook.rs` — `take_wants`, `take_subagents`, `own_session`
- `FOOTGUNS.md` — the band-collision entry, the two updated ones
