# Subsystem interactive validation — verdicts log

Plan: `docs/superpowers/plans/2026-07-03-clave-vertical-tabs-subsystems.md` Task 9.
Spec: canonical spec §9 status note (demoted checkpoints) + §6.6 verify-live items.
**Every checkpoint below is human-in-the-loop** — live Zellij session, real `claude`,
visual observation. Never marked PASS headless. Zellij plugin log (for `eprintln!`):
`$TMPDIR/zellij-<uid>/zellij-log/zellij.log`.

Setup (once, user present — touches real settings.json + permission cache):

```bash
just install          # binary on PATH + wasm into ~/.local/share/clave/
clave setup           # generated config/layout + hooks merge + permission seed
```

Carried into this checklist from task reviews:
- **Task 6:** if renames/`clave focus` never fire live, suspect `get_plugin_ids()`
  in `load()` returning nothing → apply the lazy-call fallback (one line, main.rs).
  Watch C2 (renames) and C9 (hydration) for silently-dropped FIRST effects.
- **Task 5:** C2 must re-verify the Notification substrings ("permission",
  "waiting for your input") against what the current CLI actually sends.
- **Task 7:** C7 must confirm `dump-layout` prints `args "spawn" "<uuid>" …` on
  its own line (the `live_uuids` parser assumes it); C8 must exercise a
  WORKTREE agent resume (the resume-clobber fix's bite point).

---

## C1 — Session + tab template
- Run `clave`. Expect: `clave` session; first tab = bar (~26 cols, left) + shell.
- Native new tab (stock keybind): new tab ALSO has a bar; both bars list both tabs.
- Falsifies → `default_tab_template` fragility (S1): fallback = per-tab bar panes
  + bound new-tab action with explicit layout file (rewrite `layout_kdl`).

**Findings (2026-07-06, two generator bugs found + fixed live):**
1. **KDL node termination:** `bind … { MessagePlugin "…" { … } }` fails zellij's
   parser — the child block needs a trailing `;` before the enclosing `}`
   (`{ … }; }`). Caught pre-launch by `zellij --config <cfg> setup --check`
   (worth keeping as a `clave setup` self-check later). Fixed in
   `setup.rs::config_kdl`'s nav helper.
2. **Split orientation:** sibling layout panes stack HORIZONTALLY (rows) by
   default, so `pane size=26` made a 26-row TOP strip. A left column requires
   a vertical split. Fixed in `setup.rs::layout_kdl` AND `add.rs::tab_layout`;
   regression asserts added to both tests.
3. **`children` must be a DIRECT child of `default_tab_template`** — nesting
   it inside a `pane split_direction="vertical" { … }` wrapper parses fine
   but the empty/new-tab fill path (zellij-utils 0.44.3
   `kdl_layout_parser.rs:1748`) only inserts the default terminal pane at the
   template's TOP-LEVEL `external_children_index`, without recursing — result:
   tabs with a full-screen bar and NO terminal (observed live). Correct form:
   `default_tab_template split_direction="vertical" { pane …; children }`
   (the template node itself accepts `split_direction`). `add.rs::tab_layout`
   keeps the nested-wrapper form legitimately — it uses concrete panes, no
   `children` node (the S2-proven structure).
- Positive signals before the fix: plugin loaded, permissions granted silently
  (pre-seed works), TabUpdate rows rendered (active row inverted),
  `default_tab_template` DID apply to native `Ctrl+t n` tabs (S1's fragility
  fear NOT reproduced), and Alt+↑/↓ display-order nav worked end-to-end.

**Verdict: PASS** (2026-07-06, after the three generator fixes). Left bar 26
cols, 6 tabs listed, active row inverted at row 1, terminals present, native
`Ctrl+t n` tabs get the bar, Alt+↑/↓ nav works.

**Open UX observation (user: "ordering unintuitive"; fold into C4/C5):**
interaction-recency + relative display-order nav ping-pongs — focusing row 2
promotes it to row 1, so repeated Alt+j toggles two tabs instead of walking
the list (true MRU-cycling needs key-release detection zellij lacks).
Candidate fixes to design later: clave-nav jumps DON'T bump recency (only
clicks/prompt-submits do), or Alt+j/k walk stable tab order while clicks/Alt+N
use display order.

## C2 — Agent lifecycle + live glyphs + renames
- `Alt+a` → this repo → `new`. Expect: floating picker; new tab running Claude;
  `clave ls` shows the row.
- Watch from another tab: amber on prompt submit, green on Stop; tab RENAME lands
  when the first-prompt/summary label derives.
- Trigger a permission prompt: red (needs_you). Verify Notification message
  substrings against the live payload; adjust `status_for_event` + test if drifted.

**Findings (2026-07-06):**
- PASS: Alt+a floating picker (zoxide list, cwd preselected) → new → real Claude
  TUI in a new tab; registration joined the glyph to the correct row; amber on
  submit, green on Stop; first-prompt rename (`clave · main · hi, can…`) landed
  with correct dir · branch.
- CONFIRMED: Notification substring "waiting for your input" matches live — the
  CLI's ~60s idle notification turns a done agent red. USER DECISION
  (2026-07-06): KEEP spec behavior. **SUPERSEDED 2026-07-08 (user, during
  C4/C5): idle notification → red ONLY while status is `working` (blocked
  mid-turn); swallowed for done/idle agents — see §6.5 idle-prompt
  discriminator.** Backlog: consider emoji/status glyphs that self-explain
  better (weigh against spec §1's emoji-render-inconsistency caveat).
- PASS (2026-07-06): permission-prompt red — live prompt (`touch /tmp/clave-permtest`)
  turned the row red in ~2s (substring matched CLI 2.1.201 payload, no drift);
  `clave ls` agreed (needs_you); accept → finish → green. ~60s later the idle
  notification flipped it back to needs_you (store write 13:44:36) — the
  ratified idle-red behaving as decided above.
- RESOLVED-ish: the second empty floating pane behind the picker happens ONLY
  on the very first Alt+a after session spinup (likely zellij materialising the
  tab's floating layer with a default pane); subsequent Alt+a shows one pane.
  Cosmetic; park unless it annoys.
- UX backlog (user): colour-code label segments (dir/branch/summary) in the bar
  instead of ` · ` separators — needs the bar to render agent labels from the
  SNAPSHOT (segments) rather than TabInfo.name; plain tabs keep tab names.
  Fold into a §6.6 render enhancement decision.

**Verdict:** PASS (2026-07-06) — full lifecycle live: spawn/register/rename, amber→green→red
transitions incl. permission prompt; idle-red kept by user decision.

## C3 — Unread clear
- Agent green (done), focus elsewhere → focus its tab. Expect: dims to idle
  immediately; `clave ls` agrees; exactly ONE `clave focus` run in the zellij log.

**Findings (2026-07-06):**
- FAIL (first run): focusing the green agent tab did NOT dim it; repeated
  in/out no help; `clave focus` never ran (store `last_visited` stayed 0).
- ROOT CAUSE (via TabUpdate eprintln trace, 309 samples): **zellij delivers
  `Event::TabUpdate` ONLY to the plugin instance in the currently-active
  tab.** Hidden instances are event-starved, so every instance's stream only
  ever says "my own tab is active" — the `prev_active != now_active`
  transition the clear keyed on can never be observed by ANY instance. See
  Mechanism deltas below for the full consequences (C4, write-gating).
- FIX: `apply_tabs` now runs the §6.5 unread-clear check on EVERY TabUpdate
  (delivery itself is the focus signal); exactly-once via `read_locally`
  (reset by any non-Done snapshot) + the delivery rule. Regression test:
  `done_agent_clears_without_observable_transition`.
- Checklist correction: zellij does NOT log `run_command` executions (0 hits
  for `clave snapshot`, which provably runs) — verify the single `clave
  focus` via the store (`last_visited` set, status idle), not the zellij log.
- Bonus expectation note: green = done-and-unread by design; it dims only on
  visit. If you're already IN the tab when Done lands, the clear fires on the
  next TabUpdate (usually the summary rename) — slight lag, acceptable.

- FAIL #2 (same root cause, second symptom): after the fix, focusing dimmed
  the row on the focused tab's bar, but switching OUT showed it green again —
  other instances never learn about the visit. `apply_focus` was store-only
  by design, its comment encoding the disproven broadcast assumption ("every
  bar instance saw the same TabUpdate"). FIX: `apply_focus` now bumps seq and
  returns a snapshot; the Focus command pushes it — the flip broadcasts over
  the pipe channel like every other status change. Test extended
  (`focus_clears_done_to_idle_and_stamps_visit`).
- OBSERVATION (user, for triage): the CLI's ~60s idle notification re-reds
  the agent even AFTER a visit (idle → needs_you while the tab is focused).
  The earlier KEEP decision ratified idle-red for an UNVISITED done agent;
  re-redding a visited one may warrant suppressing when
  `last_visited > last turn end` — hook has the data. Backlog candidate.

**Verdict:** PASS (2026-07-08, after 2 fixes) — grey on focus-while-green,
stays grey across tabs (snapshot push), store stamps last_visited + idle;
post-visit 60s idle-red observed (ratified behavior, see triage note above).

## C4 — Recency order + plain tabs
- Open a plain tab; interleave focus. Expect: rows reorder by interaction,
  focused tab always row 1, plain tab name-only, closed tab's row vanishes.

**Findings (2026-07-08):**
- Pre-test design fix (root cause shared with C3, ratified by user): recency
  now flows via a `clave-visited` pipe broadcast from the active instance
  (see Mechanism deltas #3); apply_tabs is order-neutral.
- Live: focused tab pinned to row 1 ✓; closed tab's row vanishes ✓; plain
  tabs name-only ✓ (implicit — no glyph reported).
- STILL TO CONFIRM in the C5 re-test round: cross-bar order agreement (hop
  tabs, then compare two different tabs' bars — the divergence the old
  design would have shown).

**Verdict:** **PASS** (2026-07-14, round 7): the C5 Design B fix made
cross-bar agreement structural — order AND decoration ride the snapshot;
only the tab SET is instance-local, and the executor's is fresh. User
confirmed multi-agent walking coherent from every tab; note the ordering
semantics changed after this section was written (§6.6: focus does NOT
reorder; commitments do).

## C5 — Nav (+ switch_tab_to attempt)
- Click a non-active row → jumps. `Alt+j/k` walk display order (wrapping);
  `Alt+2` ≈ alt-tab; `Alt+N` → row N.
- Then, on a scratch branch: swap `focus_pane_with_id` → `switch_tab_to(position+1)`
  in `run_effects`, rebuild, retest clicks. Works → note as viable simplification
  (user decides keep/revert); fails → revert, `focus_pane_with_id` stays. Log either way.

**Findings (2026-07-08, first round):**
- FAIL: mouse needed a DOUBLE click (first click focused the bar pane), and
  Alt+←/→ (MoveFocus) stopped in the bar on the way past. ROOT CAUSE: the
  bar pane was selectable — the stock tab-bar calls `set_selectable(false)`
  in load(); we never did. FIXED (one line + §6.6 note).
- FAIL: Alt+↑/↓ "didn't shift" — the parked nav ping-pong, fully armed by
  the C4 visited-pipe (jump promotes target to row 1 → next step returns).
  USER RATIFIED hybrid nav: dir walks stable TAB-POSITION order; Alt+1..9 +
  clicks stay display-row jumps (Alt+2 ≈ alt-tab). FIXED
  (`dir_nav_walks_tab_position_order_not_display_order`).
- USER-DRIVEN §6.5 REVISION (this round, chat-as-posterchild argument): red
  now means *blocked mid-turn* only — idle notification is swallowed unless
  status is `working`. Supersedes the 2026-07-06 keep-decision (see C2).
- Older observation now explained: the 13:22:27 burst of 12
  "Failed to focus stacked pane" zellij errors — from Alt-nav pipes fanning
  FocusPane out across ALL instances (FocusPane is deliberately ungated).
  Watch for recurrence in the re-test; the `switch_tab_to` attempt above is
  the candidate simplification if focus-targeting still misbehaves.

**Findings (2026-07-08, round 2):** single-click PASS, Alt+←/→ PASS
(set_selectable fix). Alt+↑/↓ FAIL — trace showed the definitive cascade:
hidden instances' TAB SETS are stale too (they last saw N tabs; a newer tab
is invisible to them), so the walk's current_tab lookup failed there and the
stale-active fallback raced SIX divergent SwitchTab targets (0,0,2,0,3,1
observed at 16:15:44); zellij executed them all in ~50ms — the transit
landings really announced, trashing recency (agent tab sank to bottom = the
user's exact sighting) and corrupting current_tab for the next walk. FIXES
(ratified direction: use channels that cannot be stale):
- dir walks → NATIVE `GoToNextTab`/`GoToPreviousTab` binds (server-side tab
  truth; landing announces organically — proven live).
- row jumps → executor-gated (own tab == replicated current_tab ⇒ the active
  instance, fresh rows) + AnnounceVisit broadcast, same shape as clicks.
- apply_snapshot records seen_interacted ONLY on a successful uuid→tab join
  so missed interaction bumps catch up when a stale instance reactivates
  (test: interaction_bump_catches_up_after_late_join).

**Findings (2026-07-08/10, round 3 → ORDERING REDESIGN, user-ratified):**
round-2 native walks worked mechanically but walked POSITION order against a
recency-DISPLAYED list — the user cannot predict where "down" goes when the
visible order isn't the walked order, and focus-bumps kept reshuffling it.
User proposed the Claude-desktop model, adopted as §6.6's new ordering rule:
- **Rows order by last USER COMMITMENT (unix s); focus never reorders.**
  Agents: store last_interacted (spawn-seeded, prompt-bumped), read at
  render. Plain tabs: birth + InputReceived touches via `clave touch`
  relays (host-stamped), max-merged. Beacon pipe = executor election only.
- Walking the DISPLAYED list is now stable (no focus-bumps → no ping-pong):
  Alt+↑/↓ step visible rows, executor-gated; Alt+1..9/click = visible row
  jumps; **Alt+o = native ToggleTab** (replaces the dead Alt+2 trick).
- Deleted: logical recency clocks, visit-bumps, seen_interacted bookkeeping.
- WATCH in re-test: (a) does InputReceived fire only for the active tab's
  instance, and does MOUSE input count (a click into a tab must not touch)?
  (b) birth-touch on first-focus of pre-wasm tabs (one-time spurious front).
  Fallback if InputReceived misbehaves: shell preexec `clave touch` hook.

**Findings (2026-07-10, round 4):** CRASH — zellij server fd exhaustion
("Too many open files", ipc.rs:388 panic) after 3 tabs + walking; tabs also
reordered on focus. ONE feedback loop: (a) InputReceived fires for EVERY
keystroke INCLUDING the nav keybinds → each walk press touched the departing
tab (the observed focus-reorder) and spawned clave-touch + zellij-pipe
processes; (b) the birth-touch guard depended on the pipe ECHO to clear, so
congested echoes re-fired birth touches on every TabUpdate → spawn storm →
EMFILE → server panic. FIXES: InputReceived REMOVED (dead end — cannot
distinguish nav keys from pane input); terminal-input commitment now comes
from a shell preexec hook (`clave touch-pane $ZELLIJ_PANE_ID`, host-stamped,
pane-keyed, joined to tabs at render — self-healing); birth-touch guarded by
an optimistic local ts=0 mark (once-EVER per instance/tab, order-neutral).

**Findings (2026-07-14, round 5):** no crash, executor gating trace-proven
(exactly one nav execution per press, zero timeouts) — but walking
OSCILLATED (prev from tab 2 → 3, prev from 3 → 2, forever) and row order
changed on walk step 2. ROOT CAUSE (trace-confirmed): per-instance timeline
copies DIVERGED — birth-touch echoes are fire-and-forget pipe DELTAS, some
instances miss some echoes under spinup congestion, each bar sorts its own
diverged rows, and each landing's instance computes the next step from ITS
OWN order. Final form of the week's lesson: fire-and-forget deltas with no
reconciliation always eventually diverge; the seq-gated full-state snapshot
is the ONE channel that never has. FIX (user-agreed, implemented
2026-07-14): **tab timeline moved into the STORE** — `tab_timeline:
BTreeMap<tab_id, unix_s>` store field, written only by `clave touch` (locked
RMW, max-merge, seq+1, snapshot push); `AgentSnapshot` carries the map; the
bar REPLACES its copy from each seq-gated snapshot. clave-touch/touch-pane
pipe arms + bar-side merge maps DELETED; birth guard is now a local
once-ever fired-set (echo-independent, rd-4 lesson kept). touch-pane/preexec
PARKED (user declined shell config; plain tabs order by birth only). Bare
`clave` clears tab_timeline when creating (not re-attaching) the session —
tab_ids are session-scoped.

**Findings (2026-07-14, round 6):** store-timeline WORKS — plain-tab
walking flawless and coherent across 6 tabs (16:36 trace), no oscillation,
focus never reorders, birth→top correct (tab-4 report not reproducible in
the data: store stamps were strictly increasing and its own bar rendered it
row 0 — likely the ~100ms touch-push window; watch). Amber/grey glyphs
PASS. BUT with TWO Alt+a agents, walking alternated 6↔5 forever
(trace-confirmed crossed orders: each agent tab's bar had the OTHER agent
tab right after its own). ROOT CAUSE — the THIRD divergence channel:
sort_key still joined agent last_interacted through uuid→pane→manifest→tab,
which is per-instance event-fed state: (a) `clave-register` pipes never
REPLAY, so a bar loaded after an agent spawned can never join it (also =
permanently glyphless row on that bar); (b) hidden instances' PaneManifests
are stale. FIX (user-ratified **Design B**, implemented 2026-07-14): the
STORE binds uuid→tab_id — the agent tab's own bar reports its join once
(`clave bind`, active-gated, sent-guard not echo-guard); the
UserPromptSubmit hook stamps tab_timeline[bind] ATOMICALLY with the
last_interacted bump; bar sort_key = snapshot timeline ONLY; glyph/rename/
unread joins all key on the snapshot bind; resume resets the bind; session
create clears binds + timeline. Registers now matter only for uuid-jump
FocusPane and computing new binds. Expected-by-design in this round:
plain-tab typing doesn't reorder (touch-pane PARKED); manually-run `claude`
untracked (adopt/release = backlog).
Also shipped: `clave-bar: loaded v… build=…` load-line (CLAVE_BUILD_TAG)
for the hot-reload workflow (`zellij action start-or-reload-plugin`).

**Findings (2026-07-14, round 7):** ALL PASS. Two-agent + plain-tab
walking coherent, no alternation/skips; prompted agent rises to top (hook
stamp path); glyphs on every bar including late-loaded instances;
permission → red; green-until-read → grey on visit (intended §6.5). Design
B verified live. **Hot-reload PROVEN**: `zellij action
start-or-reload-plugin "file:$HOME/.local/share/clave/clave-bar.wasm"`
reloaded BOTH live instances within 10ms (log `loaded v0.1.0 build=…`
lines, tag from CLAVE_BUILD_TAG at build time); no visible flicker, state
rebuilt from hydration. New iteration loop: rebuild tagged wasm + cp +
reload — session recreate only for store-schema/config changes. Residual
watch-items: Alt+o barely exercised; one unreproduced "tab 4 not on top"
sighting in round 6 (store stamps were correct; likely the ~100ms
touch-push window).

**Verdict:** **PASS** (2026-07-14, round 7 — TEMP traces removed after)

## C6 — Toggle (`hide_self` reflow)
- `Alt+c`: bars hide in EVERY tab, grid reclaims width; `Alt+c` again: back.
- While hidden, drive a status change; on show, bar reflects it (hidden plugins
  still hear pipes).
- Falsifies → fallback: `close_self()` + relaunch bind (adjust §6.6 + log).

**Findings (2026-07-14, round 8):** hide PASS (all tabs, width reclaimed).
Re-show FAIL: zellij re-INSERTS a shown pane instead of restoring its
geometry — the bar came back as a 50% split on the RIGHT in every tab that
existed at toggle time (tabs created after get the correct template).
FIX (user-picked, implemented 2026-07-14): self-repair on show — toggle-show
arms a budget-capped repair loop; each PaneUpdate steps OWN geometry toward
the template (`move_pane_with_pane_id_in_direction(own, Left)` while x>0,
then `resize_pane_with_id(Decrease→Right, own)` while cols>26), disarming
at target or after 16 steps. Self-targeted → ungated (every instance fixes
its own tab, lazily on its next PaneUpdate for hidden tabs). Bonus: one
hide/show cycle heals tabs damaged by earlier toggles.

**Findings (2026-07-14, round 9):** repair WORKED (bars moved back left,
lazily per-tab with a visible flicker — accepted) but OVERSHOT: zellij
resizes in ~5%-of-viewport steps (~14 cols here), so "shrink while >26"
blew through the target (27→13) and disarmed at ~13 cols. FIX (2026-07-15):
repair LEARNS the step from its own resize's observed effect, accepts
within half a step of 26 (exactness is impossible at that granularity),
GrowSelf recovers overshoot, and it waits for cols to change before acting
again (no double-fire on in-flight resizes). Also confirmed this round:
new-agent pane serializes as `claude --session-id <uuid> …` (parser's new
form → ▶ + jump worked); OLD agents stay `<defunct>` until respawned
(pre-fix zombies) — heal via close+resume or session recreate. C8
PRE-REGISTERED CONCERN: resurrection will re-run the serialized
`claude --session-id <uuid>`, NOT the idempotent `clave spawn` — a create
against an existing jsonl collides; S4's premise needs re-examining there.
(RESOLVED 2026-07-17: confirmed against zellij-server v0.44.3 source —
serialization records the ppid-priority *discovered* process, so even a
resident-parent spawn wouldn't survive, and a mid-tool-call agent serializes
as its child. C8 REDESIGNED: serialization off, clave-owned lazy
resurrection — spec §6.8/§6.6/§6.3 `clave open`, new checklist below.)

**Findings (2026-07-15, round 10):** learning-repair converged (bars
healed to target width) but only ONE STEP PER TAB VISIT — zellij sends no
PaneUpdate for the plugin's own resize's effect, so the PaneUpdate-driven
loop stalled until the next activation. FIX: width repair also chains off
render() (each resize triggers a repaint with the new cols; x is
unknowable there so render drives width only, PaneUpdate drives the move)
→ full convergence within a single visit. Per-tab lazy healing remains
(hidden instances get neither events nor renders) — accepted.

**Findings (2026-07-15, round 11):** width convergence OK but toggle
triggered a REPAIR STORM — bars on random sides/widths, focus jumping,
log shows a ~9ms clave-visited pipe storm + a zellij CliPipe 1s timeout.
Chain: toggle-show broadcasts layout events to EVERY instance; hidden
instances' repairs acted on STALE geometry against real panes in other
tabs; every move/resize broadcast more events (and stale-active-claim
re-announces) → feedback loop. The per-instance 16-step budget was the
circuit breaker (storm self-extinguished — contrast rd-4's fd-exhaustion
crash). Root lesson (C3 corollary, final form): `is_active_instance()` is
NOT a gate — hidden instances' stale tab sets always claim active; the
only trustworthy "on screen" signal is the EXECUTOR gate (own tab ==
replicated beacon, nav-proven). FIX: repair (both phases) is now
executor-gated. Healing stays lazy per-visit by construction.

**Findings (2026-07-15, round 12):** executor-gating repair was NOT
enough — the storm recurred on the gated build (log: pure clave-visited
announces ~15/s for 12s + CliPipe timeouts). The storm is the BEACON WAR
itself: TabUpdate-driven announces are poisoned BY DESIGN (hidden
instances' stale sets always claim own-tab-active, C3; toggle bursts
deliver TabUpdates to all; each announce spawns a CLI client whose attach
appears to trigger further TabUpdates → self-sustaining). REDESIGN: the
TabUpdate announce is DELETED; the beacon is announced from RENDER — the
one signal only the on-screen bar receives (hidden panes never render,
proven round 10) — so poison is structurally impossible. The departing
bar's doomed last renders are suppressed after nav/click (flag cleared by
the next TabUpdate, which for a hidden instance only arrives on
reactivation). TEMP landing-announce trace added for this round's log
check.

**Findings (2026-07-15, round 13):** render-announce CRASHED THE SERVER
(EMFILE, first Alt+c; log: 252 landing-announces, 460 events in the final
second). Render is NOT visibility-gated either — every instance renders at
least once after load, so all ten fresh bars saw beacon≠own and stormed.
FINAL LESSON of the announce saga: any announce driven by per-instance
"am I active" SELF-diagnosis is poisoned during bursts, regardless of
gate or channel (TabUpdate rd 11, render rd 12/13). REDESIGN (bounded
triggers only): announce fires from apply_tabs ONLY at (a) BIRTH — an
instance's first-ever TabUpdate, once per lifetime (covers new tabs +
loads/reloads), or (b) ORGANIC — Alt+o's bind now chains
`ToggleTab; MessagePlugin clave-organic`, arming ONE announce on the next
TabUpdate; any incoming beacon disarms leftover flags. Toggle bursts set
neither flag ⇒ zero announces during churn, structurally. Nav/click
announces unchanged (executor/local-computed, proven). Config regenerated
(Alt+o bind changed) — SESSION RESTART REQUIRED (server died anyway).

**Findings (2026-07-15, round 14 — source-level root cause, pre-test):**
user re-reported multi-Alt+c layout throw-out + TAB SWITCHING on the
bounded-announce build. Root-caused in zellij 0.44.3 SOURCE (server code
fetched from GitHub, not inferred): `show_self()` is a FOCUS action —
server maps it to `Action::FocusPluginPaneWithId` (zellij_exports.rs:2612),
which switches to the pane's tab. On toggle-show EVERY hidden instance
called it ⇒ ~10 racing focus actions ⇒ focus lands on an arbitrary tab +
churn. The API's escape hatch: `show_pane_with_id(own, float=false,
should_focus_pane=false)` routes to `ScreenInstruction::
UnsuppressOrExpandPane` (zellij_exports.rs:2622) → tab.rs
`unsuppress_or_expand_pane` — comment verbatim: "removes a pane from being
suppressed (hidden) but does not focus it"; the screen handler finds the
tab OWNING the pane (has_pane_with_pid includes suppressed_panes) and
restores it there. FIX: `toggle_hidden()` helper in main.rs — show path
now `show_pane_with_id(PaneId::Plugin(own), false, false)`; both toggle
call sites deduped. Layout re-insert-as-split remains expected (repair
heals per visit). Announce volume unchanged (bounded).

**Findings (2026-07-15/16, rounds 15–18 — the repair saga, all live-traced):**
- **Round 15** (alternating good/bad shows): no-focus show works, but zellij
  alternates re-insert shapes — 50% split vs REMEMBERED width on the wrong
  side. Width-accept disarmed the whole repair before the move phase ran.
  FIX: `x_ok` — width-accept may only retire a pane after a manifest
  confirmed x == 0. Also: per-show compare-base reset (stale last_cols
  "learned" the re-insert jump as a step).
- **Round 16** (multi-tab: only visited tabs healed): repair was SELF-only;
  hidden instances get no events (C3) → per-visit healing. REDESIGN:
  executor-heals-all — the one fresh instance repairs EVERY tab's bar
  (pane ids are global, move/resize commands cross-tab; `move_pane_left`
  on a leftmost pane is a verified no-op). Then: resizes fired mid-burst
  were CLOBBERED by the unsuppress relayouts; an observation-count retry
  double-fired in 4ms (renders tick ~1ms under burst). 
- **Round 17** (too narrow + inconsistent): trace showed step=60 — the
  learner attributed relayout jumps to "zellij's increment"; band ±30
  accepted 13-col bars. FIX: learn only deltas ≤ 20; retries TIME-paced
  via set_timeout(0.4) Timer chain (hard cap 30 ticks), never event-paced.
- **Round 18** (focused tab never heals): renders fired width while the bar
  still sat RIGHT (x unknowable in render); zellij's move is a geometry
  SWAP → the landing move handed the shrunk width to the terminal — the
  fastest render chain (own tab) always lost the race, pumping 75→30→75.
  FIX: STRICT phase ordering (no width until manifest confirms x == 0) +
  repair paused while hidden (budget burned at suppressed panes).
- **FINAL CONSTRAINT (round 18, user-observed one-step-per-visit):** zellij
  emits NO events for plugin-initiated resizes — the only feedback is the
  plugin's OWN render. Cross-tab width healing is therefore structurally
  blind (dead-reckoning, drift) or per-visit (rejected UX). Repair-by-
  command has hit its ceiling.

**Verdict:** _PIVOT (user-ratified 2026-07-16): declare a
`swap_tiled_layout` in the generated layout matching the template — the
50% re-insert IS zellij's default 2-pane swap layout; declaring our own
makes zellij restore geometry natively on every unsuppress (instant,
exact, per tab, eventless). Repair machinery stays as the safety net for
damaged tabs (manual resizes skip auto-relayout). Checkpoint commit at
round 18; swap-layout build next._

**Findings (2026-07-16, round 19 — swap layout: parsed perfectly, dead on
arrival):** generated `swap_tiled_layout` (setup children-slot form merged
into default_tab_template + explicit form in the one-shot tab layout) was
verified with the REAL zellij-utils 0.44.3 parser (scratch tool
`kdlcheck`): template = Vertical[bar Fixed(26) plugin, children] ✓; bare
`NewTab` keybinds fall back to the session layout's swaps server-side ✓.
Live: re-show STILL 50/50 (right; our move phase made it look left).
Root cause in zellij source, decisive: `suppress_pane` → `extract_pane` →
`set_is_tiled_damaged()` — **hiding the bar damages the tab's swap state,
and `add_tiled_pane` only auto-relayouts when NOT damaged**, so an
unsuppress can never trigger the swap relayout. Other damage-setters:
`resize_pane_with_id` (our own repair!), `resize_whole_tab` (every window
resize), close/extract/splits. Also this round: the zellij CLI incident —
`zellij attach` variants from Claude's shell injected clave-layout tabs
into the user's MAIN session (their bar instances renamed his tabs via
store-bind tab-id collisions); session lifecycle is USER-driven only (see
memory), and stale store binds self-heal on session recreate.

**Findings (2026-07-16, round 20 — COLLAPSE-IN-PLACE, user-ratified):**
never suppress. Alt+c flips a width target (26 ⇄ 4-col glyph gutter) and
each instance's render-fed width seek drives its OWN pane there — every
instance stays visible, so every instance has the feedback loop that was
the one reliable mechanism of the whole saga. All tabs toggle
simultaneously; zellij's resize floor stops the collapse around ~8 cols
(stable — the in-flight guard makes the floor benign); collapsed bar
keeps glyphs, truncated names, and the active-row highlight ("mini
mode" for free from the existing truncating renderer). DELETED: suppress
calls, move phase, executor gate on repair, per-pane repair map, x_ok,
timer retry chain (~200 lines). Kept: learned+clamped step, half-step
acceptance, in-flight guard, budget. Known quirk (accepted): a tab
created while collapsed is born expanded (missed the pipe) — fix later by
carrying the collapsed flag in store snapshots. Backlog (user): peek-on-
nav (auto-expand the bar ~2s on tab switch while collapsed).

**Verdict:** **PASS** (2026-07-16, round 20 live: expand/collapse both
directions, all tabs at once, no focus jumps, rapid toggles safe; TEMP
traces removed after).

**Round 21 (2026-07-16, features on top of the PASS): 30 cols +
peek-on-nav — user-validated live ("works brilliantly"), hot-reload
only.** BAR_TARGET_COLS 26→30 plus both `size=` templates (setup/add);
existing bars converge on the next toggle cycle, born-at-30 panes need a
session recreate. Peek-on-nav: while collapsed, any nav expands the bar,
sinking 0.9s (user-tuned from 1.0) after the last nav. Mechanism: the
replicated `clave-visited` pipe calls model `visited()` = beacon + arm
`peeking` + re-arm seek toward the template; main.rs counts one
`set_timeout(0.9)` per armed peek and only the LAST expiry calls
`peek_expired()` (sink), so nav bursts stay expanded. `width_seek` target
is `collapsed && !peeking ? 4 : 30`; toggle clears `peeking` (explicit
Alt+c outranks a peek; late timers are no-ops). Deliberate deviation from
the sketched design: `beacon()` keeps its signature — the internal beacon
callers (click/nav) can't start host timers, and their AnnounceVisit
echoes back as clave-visited on every instance anyway, so arming peeks
ONLY on the pipe path means a peek can never exist without its sink
timer. Expanded bars ignore peeks.

## C7 — dump-layout liveness + resume picker
- With one agent live: `zellij action dump-layout | grep -A2 clave` — baked
  `args "spawn" "<uuid>" …` present, on its own line.
- `Alt+a` → same repo → expect jump to the running agent, no duplicate spawn.
- Falsifies → fallback: SessionStart/SessionEnd liveness in the store (§6.3).

**Findings (2026-07-14, round 8):** FAIL, two root causes, both fixed:
1. **`command="<defunct>"` decoded**: zellij serializes the LIVE pane
   process (children of the pane's PID), not the baked layout command. The
   pre-exec fire-and-forget `zellij pipe clave-register` child was inherited
   by the exec'd claude, never reaped → permanent ZOMBIE → serialized as
   `<defunct>` on EVERY agent pane (ps: each `claude --session-id …` had
   exactly one Z child). Blinds liveness AND resurrection. FIX: register
   pipe double-forked via `sh -c '"$@" &'` (sh reaped, grandchild reparents
   to init); `live_uuids` parser now also matches the post-exec
   `--session-id`/`--resume` arg forms.
2. **Auto-jump design flaw**: "repo has a live agent → jump" (spec §6.3
   pre-revision) forbids a SECOND agent in the same repo — user is right
   that fleets-per-repo are the point; it only went unnoticed because
   liveness was blind. The user hit the corollary live: resume-picking an
   on-screen session opened it TWICE (same uuid two tabs; bind can point at
   only one → the glyphless duplicate row in the round-8 screenshot). FIX:
   auto-jump deleted; resume picker now lists live agents MARKED `▶` and a
   live pick JUMPS (clave-nav uuid pipe); dead picks resume as before.
   Spec §6.3 revised.
Note: zombies from pre-fix agents persist until those panes close — re-test
needs fresh agents (or a session recreate).

**Verdict:** _fixes implemented; re-test owed (fresh agent → dump-layout
shows real command; ▶ pick jumps; second agent in same repo works)_

## C8 — Resume + resurrection (S4, REDESIGNED 2026-07-17: lazy clave-owned)
Run in the `clave dev` sandbox (spec §6.9) — scenarios seed the states; the
user drives; Claude reads `clave dev status` + clave.log + zellij.log.
Sandbox isolation caveats (final review, 2026-07-18): the sandbox touches
the real machine in exactly two sanctioned ways — (1) an additive grant for
the sandbox wasm path in zellij's machine-global permissions.kdl (non-
clobbering, keyed per wasm path; survives `dev reset`), (2) `claude -p`
seeding authenticates via the shared macOS Keychain. Everything else
(store, config, hooks, jsonls, session) is env-routed into the sandbox.
- Picker resume (unchanged surface): `Alt+w` an agent's tab; `Alt+a` → same
  repo → `resume` → pick it: history resumes; store row PRESERVED
  (label/worktree/cwd — Task 7 fix).
- Cold start (`c8-cold-start`: 2 dormant + 1 recent): kill-session →
  relaunch. Expect: NO ENTER gates; most-recent agent resumes focused with
  history; other rows dormant ◌ in recency order.
- Walk-through safety: Alt+↓ quickly THROUGH a dormant row to a live row —
  the passed row must NOT open.
- Dwell open: settle 0.4s on a dormant row → ↻ → live resume with history.
  Repeat for the worktree row (`c8-worktree`): resumes in its worktree.
- Explicit pick: click a dormant row → opens immediately (no dwell).
- Stale (`c8-stale`): delete the row's cwd, dwell it → ✗, no tab, session
  unaffected.
- Second kill+relaunch: previously-opened rows dormant again except the most
  recent; no `<defunct>` panes anywhere (`dump-layout`).

**Verdict:** _pending_

## C9 — Hydration (S5)
- With agents in the store, kill+relaunch the session (or reload the plugin):
  bars show correct glyphs/labels BEFORE any new hook event; zellij log shows
  the `clave snapshot` run_command round-trip.

**Verdict:** _pending_

## C10 — Hook safety
- Claude session OUTSIDE the clave session: zero interference;
  `time clave hook Stop <<< '{"session_id":"not-tracked"}'` <50ms, exit 0.
- `echo garbage | clave hook Stop; echo $?` → 0.

**Verdict:** _pending_

---

## Mechanism deltas / spec reconciliation
_(fill as found; update spec §4/§6 in the SAME commit as any mechanism change)_

- **Zellij event delivery (C3, 2026-07-06): `TabUpdate` reaches ONLY the
  active tab's plugin instance.** Spec §6.6 assumed a global broadcast.
  Verified empirically (per-instance trace; each instance only ever sees its
  own tab active, `self_active=true` always). Pipes, by contrast, ARE
  broadcast to all instances with backpressure (buffered through plugin
  load; each `zellij pipe` also delivers one empty EOF message per instance
  — benign, dropped). Consequences:
  1. Transition-based logic is impossible per-instance → unread clear now
     keys on "TabUpdate received with own tab active" (fixed, C3).
  2. `is_active_instance` write-gating is degenerate (every instance's stale
     tabs claim it's active) → renames execute on ALL instances that process
     a snapshot pipe — harmless only because rename_tab_with_id is
     idempotent with identical input. Revisit gating design.
  3. **C4 recency was broken by the same assumption** (hidden instances never
     learn about other tabs' focus → per-bar order divergence). RATIFIED
     (user, 2026-07-08) + IMPLEMENTED: the active instance announces a
     `clave-visited <tab_id>` pipe (run_command → `zellij pipe`, the proven
     broadcast); all instances bump on receipt; announcer bumps locally and
     goes quiet while its tab is the unique recency front (should_announce
     dedupe — re-opens when anything overtakes it, incl. agent snapshot
     bumps). apply_tabs is now order-neutral. Spec §6.5/§6.6 revised in the
     same change. Known accepted quirk (pre-existing): an agent snapshot bump
     can overtake the focused tab's row until the next TabUpdate re-announces.
