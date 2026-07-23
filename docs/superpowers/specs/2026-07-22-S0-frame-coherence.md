# S0 — frame coherence & executor election (RC-A, RC-B)

_Implementation spec · 2026-07-22 · workstream **S0** of
[the UX defect dossier](2026-07-22-ux-defect-dossier.md) · main `50fa26a`_

Read the dossier first (RC-A `:73-121`, RC-B `:124-182`). This spec is the
build order for those two root causes and nothing else. S1 and S3 are blocked
on the seam it lands (`§ Risks and out-of-scope`).

---

## Problem

Every write the bar performs — `clave bind`, `clave touch`, `clave prune-tabs`,
`clave focus`, `rename_tab_with_id` — is gated on the answer to one question:
*am I the bar in the currently-active tab, and which tab is that?* The bar
answers it by joining two **independently delivered** zellij frames on **tab
position**: the last `PaneUpdate`'s plugin-pane list against the last
`TabUpdate`'s tab list. Positions are not stable identifiers — zellij renumbers
every position after a closed index — so in the window between one frame landing
and the other, the join silently returns a *different tab's* identity. The bar
that wins that wrong election binds its agent to someone else's tab; `apply_bind`
evicts the rightful tenant; and because `sent_binds` is a permanent latch with no
clear path, the corrective re-bind can never fire for the life of the plugin
instance. Separately, the one snapshot path that actually populates `self.agents`
at session birth — the hydrate — is the one path that never kicks the binder, so
the eager cold-start tab is frequently never bound at all.

| # | Evidence | file:line |
|---|---|---|
| 1 | `is_active_instance()` joins `plugin_panes` (PaneUpdate frame) to `last_tabs` (TabUpdate frame) by position | `crates/clave-bar/src/main.rs:43-54` |
| 2 | `own_tab_id()` does the same join to produce the *identity* every write is keyed on | `crates/clave-bar/src/main.rs:60-71` |
| 3 | `model_tab_active_at()` — the tabs-side half of the join | `crates/clave-bar/src/main.rs:73-81` |
| 4 | `bind_effects` mismatches frames on **both** sides: `own_position` from `self.tabs` (fresh), `tab_position_of_pane` from `self.panes` (stale) | `model.rs:414-426`, `model.rs:396-401` |
| 5 | `sent_binds` — inserted at emit, never removed, never cleared, not reset by `apply_snapshot` | `model.rs:197`, `:305`, `:429`, `:431` |
| 6 | `apply_bind` evicts any other agent holding the target tab | `crates/clave/src/store.rs:239-245` |
| 7 | eviction victim renders as a live agent gone dormant (`◌`, dim 90 — the same dim as `Status::Idle`) | `model.rs:479-488`, `model.rs:770-777` |
| 8 | the wrong tenant's next prompt stamps `tab_timeline[wrong_tab]` → that tab jumps to row 0 | `crates/clave/src/hook.rs:246`, `:250-253`; sort key `model.rs:391-393`, comparator `model.rs:791` |
| 9 | `fire_binds()` call sites: TabUpdate, PaneUpdate, `clave-status` pipe, `clave-register` pipe — **four** | `main.rs:447`, `:467`, `:268`, `:279` |
| 10 | the hydrate arm (`RunCommandResult`) does `apply_snapshot` → `run_effects` and **no** `fire_binds()`; the byte-adjacent `clave-status` arm does | `main.rs:393-412` vs `main.rs:264-270` |
| 11 | hydrate is the only thing that populates `self.agents` at session birth | `main.rs:385-391` |
| 12 | `bind_effects` loops `self.agents` — empty agents means zero iterations, silently | `model.rs:421` |
| 13 | cold start wipes every bind and the whole timeline before session create | `crates/clave/src/setup.rs:647-651` → `store.rs:340-349` |
| 14 | the birth touch is gated on `is_active_instance()` and evaluated **only** in the TabUpdate arm | `main.rs:434-445` |
| 15 | the gate is already on record as structurally weak | `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md:656-659` |
| 16 | `main.rs` is `test = false` — none of 1–3, 9, 10, 14 is reachable by any test | `crates/clave-bar/Cargo.toml` `[[bin]] test = false` |

---

## Why it matters

Two of the four reported symptoms, in the maintainer's words:

- **"some tabs don't shift to the top when interacted with."** A mis-bind sends
  the hook's prompt stamp to the wrong `tab_id` (evidence 8), so the tab you
  typed in never moves and an unrelated one does. RC-B produces the same
  complaint by a different route: an unbound agent has `rec.tab_id == None`, the
  hook skips the stamp entirely (`hook.rs:246`), the tab sorts at key `0`
  (`model.rs:392`) — **below every dormant row** — and the *first prompt to the
  eager cold-start agent never moves its tab, by construction**.
- **"closing a tab goes all kinds of wrong — sometimes idle, sometimes another
  tab moves to top."** The close is what renumbers positions, which is what makes
  the join wrong. The evicted tenant loses its status dot and gains a dim `◌`
  duplicate — read on screen as "it went idle" (evidence 6, 7) — while the
  wrongly-bound tab climbs to row 0 on the next prompt.

Both are **sticky**: `sent_binds` (evidence 5) blocks the re-bind for the life of
the plugin instance, so the fleet stays wrong until the session is relaunched.
That stickiness, not the transient wrongness, is what makes RC-A the
highest-severity item in the dossier.

---

## Design

### The direct route does not exist — reported as required

Direction 1 asked whether zellij-tile 0.44.3 hands a plugin its own tab identity.
**It does not.** Read in full at
`~/.cargo/registry/src/index.crates.io-*/zellij-tile-0.44.3/src/shim.rs` and
`…/zellij-utils-0.44.3/src/data.rs`:

- **`PluginIds` carries no tab** — `zellij-utils-0.44.3/src/data.rs:2701-2706`:

  ```rust
  pub struct PluginIds {
      pub plugin_id: u32,
      pub zellij_pid: u32,
      pub initial_cwd: PathBuf,
      pub client_id: ClientId,
  }
  ```

  and `get_plugin_ids()`'s own doc (`shim.rs:99`) says only *"Returns the unique
  Zellij pane ID for the plugin as well as the Zellij process id."*

- **`PaneInfo` carries no tab** — `data.rs:2296-2347`. The only struct in the
  crate that joins a pane to a tab is `PaneListEntry`
  (`data.rs:2350-2362`: `pane_info`, `tab_id`, `tab_position`, `tab_name`,
  `pane_command`, `pane_cwd`) — and that is the **CLI** `list-panes` response
  type (`pub type ListPanesResponse = Vec<PaneListEntry>`, `data.rs:2364`).
  There is no `PluginCommand` that returns it: the query surface is
  `GetPluginIds`, `GetFocusedPaneInfo`, `GetPaneInfo(PaneId)`,
  `GetTabInfo(usize)`, `GetSessionEnvironmentVariables`, `DumpLayout`,
  `GetLayoutDir` (`data.rs:3574-3582`).

- **`get_pane_info(PaneId::Plugin(own))` is the obvious candidate and is a dead
  end** — it returns a `PaneInfo`, which has geometry, title, focus and
  selectability but no tab of any kind (`data.rs:2296-2347`).

- **`get_focused_pane_info()` returns the *session's* focused tab**, not ours —
  `shim.rs:206-207`: *"Returns the focused pane ID and tab index for the client
  associated with this plugin."* Every bar instance shares one client, so all N
  instances get the same answer. It cannot discriminate.

- **`get_tab_info(tab_id)` inverts the wrong way** — `shim.rs:280-307`, keyed
  *by* tab id, returning `TabInfo { position, name, active, …, tab_id }`
  (`data.rs:2237-2275`). It can freshly confirm a candidate's position, but the
  candidate itself comes from the stale side of the join, so the confirmation is
  circular.

- **`dump_session_layout()` has no ids** — `LayoutMetadata { tabs, creation_time,
  update_time }` (`data.rs:1845-1849`) over `TabMetadata { panes, name }`
  (`data.rs:1930-1933`); `PaneMetadata` is layout geometry. This matches the
  dossier's live measurement that `zellij action dump-layout` prints *"no ids at
  all"* (`ux-defect-dossier.md:273`).

- **No event carries it either.** `EventType`
  (`zellij-utils-0.44.3/src/plugin_api/event.proto:10-58`) has nothing shaped
  like "your pane's tab changed". `Event::Visible(bool)` (`event.proto:29`:
  *"This plugin became visible or invisible"*) is the only per-instance
  visibility signal, but (a) it answers "am I visible", not "which tab am I",
  so it cannot replace `own_tab_id()`, and (b) its emit conditions live in
  `zellij-server`, which is **not vendored**, so they cannot be read from source
  — the same unverifiability that put RC-D behind a spike.

The one authoritative single-frame join that exists is out-of-process:
`zellij action list-panes -t -j` → `Vec<PaneListEntry>`, measured at ~0.19 s
(`ux-defect-dossier.md:271-272`). **Rejected as a runtime mechanism**: it is a
subprocess on the event path, and per-event subprocess spawning from the bar is
the exact shape that exhausted the zellij server's file descriptors
(`SUBSYSTEM-VALIDATION.md:232-243`). It *is* adopted as a **diagnostic** — it is
the ground truth the live-validation section joins against.

**Conclusion: the position join cannot be eliminated. It must be guarded.**

### Adopted: a structural coherence witness, not a bare generation counter

Direction 2 asked for "a generation counter bumped on each TabUpdate and
PaneUpdate; refuse to elect or bind when the two frames were not observed within
the same coherent generation". **I am adopting the intent — fail closed, retry
on the next event — and modifying the mechanism, for a reason that falls out of
the delivery semantics.**

A bare per-stream counter cannot express coherence, because the two streams have
no shared clock and neither implies the other. `tabs_gen == panes_gen` is
meaningless. The nearest well-formed version is an **arrival-order** counter —
"was the manifest observed *after* the tab set?" — and it is wrong in both
directions:

- *Too strict.* A plain focus switch delivers a `TabUpdate` and **no**
  `PaneUpdate` (nothing about panes changed). Under `panes_gen > tabs_gen` the
  instance would refuse forever after any focus move — and `fire_binds()` on
  TabUpdate is exactly what the eager cold-start tab depends on. Fatal.
- *Too loose.* `TabUpdate` reaches **only the active tab's instance** (C3,
  `SUBSYSTEM-VALIDATION.md:646-651`), so a hidden bar's `last_tabs` is *frozen*,
  not merely lagging. Its arrival order still reads "manifest newer than tabs",
  so the counter waves it through — and a frozen tab set claiming its own tab is
  active is precisely the degeneracy on record at `:656-659`.

What actually distinguishes a coherent pair from an incoherent one is
**content**: the `PaneManifest` is *"a dictionary of panes, indexed by the tab
position"* (`data.rs:2277-2282`) and every live tab has at least one pane, so
when the two frames describe the same world, the manifest's tab-position key set
is **exactly** the tab set's position set. They differ precisely when one frame
predates a tab create or close — the renumbering window RC-A rides. That is a
generation check, derived from the frames themselves rather than from a counter
they do not share:

```rust
/// True when the last TabUpdate and the last PaneUpdate describe the same tab
/// set. The manifest is keyed by tab POSITION (data.rs:2277-2282) and every
/// live tab has at least one pane, so a coherent pair covers exactly the same
/// positions. They diverge for exactly as long as one frame predates a tab
/// create/close — the renumbering window RC-A rides.
fn frames_coherent(&self) -> bool {
    if self.tabs.is_empty() || self.panes.is_empty() {
        return false; // pre-first-frame: fail closed
    }
    let tab_pos: BTreeSet<usize> = self.tabs.iter().map(|t| t.position).collect();
    let pane_pos: BTreeSet<usize> = self.panes.iter().map(|p| p.tab_position).collect();
    tab_pos == pane_pos
}
```

Note it reads **all** panes, not just plugin panes. Every clave tab does get
exactly one bar (`default_tab_template`, `setup.rs:153-183`, and the launch
layout's bare eager-tab node at `setup.rs:195-206` exists so it gets exactly
one, not two), but keying the witness on that would make it fail permanently the
day a tab without a bar exists. Keying on all panes is unconditionally true of a
coherent manifest.

Traced against the dossier's own reproduction (`ux-defect-dossier.md:91-99`):
tabs `{10@0, 11@1, 12@2}`, close 10, fresh `last_tabs` = `{11@0, 12@1}`
(positions `{0,1}`) against a stale manifest still covering `{0,1,2}` → sets
differ → **refuse**. The imminent `PaneUpdate` lands with `{0,1}` → coherent →
own plugin pane resolves at position 0 → tab 11. Correct, and the retry is the
event that was always going to arrive anyway. The mirror case (manifest first,
tab set stale) fails the same witness.

**Residual 1 — count-preserving reorder.** A tab *reorder* that preserves the
count (zellij's `MoveTab`) passes the witness while invalidating the join.
Nothing available in-process detects it, a counter would not have either, and
clave binds no move-tab key (`setup.rs:86-124`; #28 unbound the tab-mode entry
points). Filed as a known residual, not fixed here.

**Residual 2 — identity permutation at a preserved position set (CodeRabbit,
2026-07-22, and the reason position-set equality is necessary but not
sufficient).** Close the *lowest* tab and create one in the same window: tabs
`{A@0,B@1,C@2}` → close A → `{B@0,C@1}` → create D → `{B@0,C@1,D@2}`. A stale
manifest still covering `{0,1,2}` now has the **same position set** as the fresh
tab list `{0,1,2}`, so `frames_coherent()` returns `true` while every occupant
has shifted — own pane at position 0 resolves to B, not A. The witness cannot
catch this: `PaneManifest` carries pane ids but **no `tab_id`** (`data.rs:2277-2282`),
`TabInfo` carries `tab_id` but no pane identity, and the plugin has no direct
self-tab route (S0 §"no direct route": `PluginIds` has no tab, `get_tab_info`
inverts the wrong way). Position is the *only* cross-frame key these two events
share, so no pure identity/epoch witness is constructible from them — confirming
the reviewer's "if the data cannot support one, classify it as unresolved."

**Why it is nonetheless not a regression, and is bounded.** Two things contain
it. (i) The *complete* fix is identity-addressed operation — bind and switch by
`tab_id`, not position — which S3 §C4 commits to via the `SwitchTabToId`/
`GoToTabWithId` host import; once that lands, the position join is gone and this
class dissolves. Until then it is the interim. (ii) Crucially, S0's self-healing
bind (Direction 3, below) removes the permanent `sent_binds` latch: a wrong bind
emitted during this window is **re-evaluated on the next coherent frame** and
corrected, where today it is sticky for the life of the plugin instance. So this
residual degrades from "permanent mis-bind" (RC-A as reported) to "at most one
transient mis-bind that self-corrects" — a strict improvement even unfixed.

**Required follow-through** (per the finding): this interleaving is added to the
model tests as an explicit case (`frames_coherent` returns `true`, and the
self-healing retry is asserted to correct the resulting bind on the next frame),
and to the live-validation stop conditions in §5 — the driving agent must call
out "close the lowest-numbered tab, then immediately open a new one" as a case
where a one-frame flicker is expected-and-self-correcting, and a *persistent*
mis-bind after it is a stop-the-line S0 failure, not a residual.

### Adopted with a caveat: two gate strengths, classified by retry

Direction 2 says "refuse to elect **or bind**". Applied literally to
`run_effects`'s single `active` flag (`main.rs:88`), it would tighten six
effects at once — and **four of them latch at emit time and never retry**, so
tightening converts a wrong-action bug into a missed-action bug:

| Effect | Emitted from | Retry today | Verdict |
|---|---|---|---|
| `Bind` | `bind_effects` | **none** — `sent_binds` latch (`model.rs:429-431`) | **strict**, once the latch is fixed below |
| `PruneTabs` | `apply_tabs` | **yes** — detection-driven, re-derived every TabUpdate until the store echo clears the mirror (`model.rs:673-693`) | **strict** — a frozen instance's payload can list a tab created after its frame as stale, unbinding a live agent (the #6 class, `TESTING.md:162`) |
| birth touch | inline `run_command`, `main.rs:444` | latch not consumed when the gate is false (`&&` short-circuits) but **no trigger re-evaluates it** — see below | **strict** + a trigger |
| `ReanchorVisit` | `apply_tabs:636` | **none** — `current_tab` is mutated at `:635` before the push, so `stranded` is false forever after | **unchanged** (weak) |
| `MarkRead` | `apply_tabs:653` | **none** — `read_locally.insert` latches at emit (`:652`) | **unchanged** (weak) |
| `RenameTab` | `apply_snapshot:576` | **none** — `renamed.insert` latches at emit (`:575`) | **unchanged** (weak) |
| `PersistCollapse` | `toggle`/`apply_snapshot` | its own pending-write ledger + one re-assert (`model.rs:549-562`) | **unchanged** (weak) |

Tightening `ReanchorVisit` would regress #23 outright: the drop window is
*documented* as the accepted trade at `model.rs:620-624`, and a stricter gate
widens it. Tightening `MarkRead` is worse than useless — `read_locally` has
already flipped the local render, so gating only the store write makes screen and
store disagree. And note `ReanchorVisit`'s payload is derived purely from the
**fresh** `self.tabs` (`model.rs:628`), so even a mis-elected instance emits the
*correct* tab id; wrong election is harmless there and catastrophic for `Bind`.

So: **two named gates, both in `model.rs`, both testable.**

- `Election::Confirmed` — frames coherent **and** the resolved own tab is the
  active one. Gates `Bind`, `Touch`, `PruneTabs`, and the `clave-nav` executor.
- `Election::Presumed` — today's computation, byte-for-byte, moved into
  `model.rs`. Gates `RenameTab`, `MarkRead`, `ReanchorVisit`,
  `PersistCollapse`. Zero behaviour change for those four.

`Confirmed` implies `Presumed`, so the ordering is sound.

`clave-nav` (`main.rs:315-328`) moves to `Confirmed` deliberately: its executor
test is already `own_tab_id() == current_tab` (`main.rs:319-321`), and the
dossier's residual *"a nav or click processed between the close and the
renumbering TabUpdate lands on the wrong tab"* (`ux-defect-dossier.md:364-367`)
is exactly a coherence failure. A dropped `Alt+j` is a repeatable keypress; a
jump to the wrong tab is not.

### Adopted, with the storm argument spelled out: seq-clocked, capped bind retry

Direction 3: the `sent_binds` "last sent, never cleared" latch goes. Preferred
rule was "re-emit when the snapshot shows `a.tab_id != Some(own_tab)`, with
whatever debounce is needed". **Adopted, with the debounce clocked on the store's
`seq` rather than on frames or wall time**, plus a hard attempt cap.

Why `seq` and not a frame counter or a timer:

- The bar **has no clock** — that is why `clave touch` host-stamps
  (`crates/clave/src/main.rs:93-101`). A frame counter is not a wall clock
  either: an event burst delivers many frames in milliseconds while the
  `clave bind` round trip (spawn → flock → `seq+1` → `hook::push_snapshot` →
  pipe → `apply_snapshot`) takes tens of milliseconds. A frame-count debounce
  would re-fire mid-flight during exactly the bursts that matter.
- `seq` is the store's monotonic progress counter (`store.rs:253`, §5 pipe
  contract). A successful `clave bind` *always* bumps it and pushes
  (`store.rs:246-254`, `crates/clave/src/main.rs:309-315`). So "re-emit only
  once a strictly newer snapshot has been accepted and the join **still**
  disagrees" means: never while our own write is in flight, and immediately once
  its result is known. This is the same self-limiting shape the prune already
  relies on — *"the store push echo clears self.agents/self.timeline, so a clean
  store self-limits"* (`model.rs:683-685`).
- Bonus property: when `apply_bind` returns `None` because the uuid is unknown
  (`store.rs:229-231`), there is **no push and no seq bump**, so the retry does
  not fire at all. An unwinnable bind costs exactly one subprocess.

**The storm risk, explicitly.** The incident to avoid is round 4,
`SUBSYSTEM-VALIDATION.md:232-243`: *"zellij server fd exhaustion ('Too many open
files', ipc.rs:388 panic)… the birth-touch guard depended on the pipe ECHO to
clear, so congested echoes re-fired birth touches on every TabUpdate → spawn
storm → EMFILE → server panic."* That is precisely an echo-gated guard, which is
why `sent_binds` was made echo-independent in the first place (`model.rs:194-197`).
Three properties keep the new rule out of that class:

1. **Coherence makes bind claims mutually exclusive.** `joined_here` requires the
   agent's registered pane to sit at *my* tab's position in *my* manifest
   (`model.rs:422-426`); under a coherent frame exactly one tab holds that pane,
   so at most one instance can ever claim a given agent. Today, under
   mismatched frames, two instances can claim the same agent and fight. The guard
   removes the multi-writer case that makes a storm compounding rather than
   linear.
2. **Progress-gated, not event-gated.** Re-emission requires `self.seq` to have
   strictly advanced since our last emit for that uuid. Frames, renders, pipes
   and timers do not advance `seq`; only a real store mutation does. A quiescent
   store costs zero subprocesses no matter how many events arrive.
3. **Hard cap.** The one loop that *does* advance `seq` on every round is
   eviction ping-pong: two agents whose panes both resolve into one tab, each
   bind evicting the other (`store.rs:239-245`), each push advancing `seq`.
   `BIND_MAX_TRIES` bounds it at 4 emissions per (uuid, target tab) episode,
   reset only when the snapshot confirms the join or the target changes. This is
   the `collapse_reasserted` precedent verbatim — *"a second contradiction means
   someone else is authoritative after all, and wrong-but-consistent beats a
   two-instance re-assert ping-pong (round 11)"* (`model.rs:543-546`).

Worst case is therefore **4 spawns per agent per divergence episode, from one
instance**, versus round 4's unbounded per-render spawns from every instance.

### Adopted, restructured: one identity entry point instead of a fifth call site

Direction 4 asks why the byte-adjacent `clave-status` arm calls `fire_binds()`
and the `RunCommandResult` arm does not. The answer is in the shape of
`apply_snapshot`'s contract, not in either arm:

`apply_snapshot` returns *decoration* effects (`RenameTab`, `PersistCollapse` —
`model.rs:519-589`). Bind emission is **not** one of them; it is a separate
adapter-level call, because `bind_effects` needs an argument — `own_tab` —
that the model could not compute for itself. So every snapshot path must
remember to make a second call, and one of them has to be first to forget. The
`clave-status` arm (`main.rs:264-270`) was written as the *live push* path, whose
causal story is "a new agent row just appeared, it may need its bind" — the
comment at `:268` says exactly that. The `RunCommandResult` arm (`main.rs:393-412`)
was written as the *hydrate* path from spike S5, whose mental model was "catch up
on decoration"; decoration comes back in `fx`, so `run_effects(fx)` looked
complete. It is an omission with a plausible local story, which is why it
survived review.

Adding a fifth `fire_binds()` call fixes the instance and leaves the class. The
restructure fixes the class: once `own_pane` lives in the model, the model can
compute its own identity, and **one** method returns everything that depends on
it:

```rust
/// Every effect that depends on THIS instance's tab identity. Fail-closed:
/// returns nothing while the two zellij frames disagree (§ frames_coherent),
/// and the caller re-enters on the next event — which is the very frame that
/// makes them agree again.
pub fn identity_effects(&mut self) -> Vec<Effect>
```

main.rs calls it from **every** arm that mutates model state from an external
input — TabUpdate, PaneUpdate, both snapshot arms, `clave-register` — and the
birth touch stops being an inline `run_command` in the adapter.

### Adopted, with a correction: the birth touch already retries; it has no trigger

Direction 5 says "make the birth touch retry when it was skipped because
`is_active_instance()` was false". Read literally, the retry is already there —
`&&` short-circuits at `main.rs:434-437`, so when `is_active_instance()` is
false, `needs_birth_touch()` is never called and the `birth_touched` latch
(`model.rs:383-385`) is **not** consumed:

```rust
if let Some(active_id) = self.last_tabs.iter().find(|t| t.active).map(|t| t.tab_id)
    && self.is_active_instance()
    && self.model.needs_birth_touch(active_id)
```

The bug is that nothing re-evaluates it. The block lives **only** in the
`TabUpdate` arm, and a close delivers exactly one `TabUpdate` to the newly-active
instance — the one where the manifest is stale and the gate is false. There is no
second TabUpdate until the next focus change, so the retry never runs. Moving the
birth touch into `identity_effects()` gives it the trigger it lacked: the
`PaneUpdate` that resolves the incoherence is the retry. So the direction is
adopted; the mechanism is "give it a trigger", not "add a retry".

### Rejected: a liveness sanity check in `store::apply_bind`

Direction 6 asks whether `apply_bind` should refuse, or loudly log, a bind to a
tab another live agent holds. **Refusing is over-reach and would reintroduce a
fixed bug.** The eviction at `store.rs:239-245` exists *because* zellij reuses
tab ids (`get_new_tab_id = max-key+1`, cited `store.rs:232-238`,
`model.rs:660-663`): after a reuse, the previous holder is genuinely dead and
refusing the bind leaves a dead agent decorating a live tab — the exact ghost the
eviction was written to kill. And the store cannot tell the two apart: it has no
view of zellij's tab set, and no field on `AgentRecord` encodes tab liveness
(`status` is claude's state, `stale` is a missing cwd). "Refuse if another *live*
agent holds it" is not expressible at that layer. Correctness belongs where the
information is — the bar must not emit the wrong bind, which is what the
coherence gate does.

**Logging is adopted, and is the cheap half.** The dossier records that this
whole bug class is invisible to the evlog — *"`touch`, `bind`, `prune-tabs`,
`focus` and `collapse` call no `log_event`"* (`ux-defect-dossier.md:536-538`).
One `log_event` on the **evicting branch only** costs nothing when nothing is
wrong, and turns "an agent's `tab_id` names a tab hosting a different agent's
pane" from an inference across two commands into a line in `clave.log`. The live
validation below reads it.

### Rejected outright

- **Subscribing `EventType::Visible`** to answer "am I active" — it answers a
  different question (visibility, not identity), its emit conditions are in the
  unvendored `zellij-server`, and the C6 saga is a long record of
  visibility-driven self-diagnosis poisoning the election
  (`SUBSYSTEM-VALIDATION.md` C6, summarised `TESTING.md:346-355`).
- **`zellij action list-panes` from the plugin** — a subprocess on the event
  path; see the fd incident above. Diagnostic only.
- **Making `apply_bind` idempotent-by-tab rather than by-uuid** (one writer per
  tab enforced store-side) — same information problem as direction 6, and it
  would break `clave add`'s guaranteed post-creation push (`add.rs:757-760`).

---

## Implementation

### 1. `crates/clave-bar/src/model.rs` — identity moves into the model

**1.1 New state on `BarModel`** (beside `panes`/`tabs`, `model.rs:171-286`):

```rust
    /// OUR plugin pane id (get_plugin_ids().plugin_id), set once at load.
    /// Identity resolution lives here, not in the adapter, because main.rs is
    /// `test = false` — everything that joins the two zellij frames must be
    /// host-testable or it is unguarded (RC-A shipped for exactly that reason).
    own_pane: Option<u32>,
    /// uuid → the bind we last SENT, the seq in effect when we sent it, and
    /// how many times we have tried this target. Replaces `sent_binds`, whose
    /// "last sent, never cleared" latch made a wrong bind permanent (RC-A).
    bind_sent: BTreeMap<String, BindSent>,
```

with

```rust
struct BindSent { tab_id: usize, at_seq: u64, tries: u32 }

/// Bind re-emissions per (uuid, target tab) episode before we stop fighting.
/// The heal RC-A needs is ONE; a lost push needs one or two; beyond that we
/// are in an eviction ping-pong we cannot win, and wrong-but-consistent beats
/// a storm (the `collapse_reasserted` precedent, model.rs:543-546).
const BIND_MAX_TRIES: u32 = 4;
```

`Default` (`model.rs:288-324`): `own_pane: None`, `bind_sent: BTreeMap::new()`;
**delete** `sent_binds` and its field doc at `:194-197` and `:305`.

**1.2 New `Effect::Touch`** (add to the enum at `model.rs:34-92`):

```rust
    /// run_command(["clave","touch",tab_id]) — the once-ever birth stamp for a
    /// tab the store timeline has never seen. Was an inline run_command in the
    /// adapter (main.rs:434-445), which put it out of reach of every test and
    /// gave it no retry trigger; as an effect it is emitted by
    /// identity_effects and gated Confirmed.
    Touch { tab_id: usize },
```

**1.3 Identity resolution** (new block near `tab_position_of_pane`,
`model.rs:396-401`):

```rust
    pub fn set_own_pane(&mut self, plugin_pane_id: u32) { self.own_pane = Some(plugin_pane_id); }

    /// Our own tab position, per the LAST PaneUpdate. Plugin and terminal pane
    /// ids are separate id spaces ("unique to all panes of this kind",
    /// data.rs:2297-2300), so the is_plugin filter is load-bearing — the same
    /// reason tab_position_of_pane filters !is_plugin.
    fn own_tab_position(&self) -> Option<usize> {
        let own = self.own_pane?;
        self.panes.iter().find(|p| p.is_plugin && p.pane_id == own).map(|p| p.tab_position)
    }

    fn frames_coherent(&self) -> bool { /* as quoted in Design */ }

    /// Our tab id — Some ONLY when the two frames agree. Fail closed: the
    /// caller does nothing and re-enters on the next frame, which is the frame
    /// that resolves the disagreement.
    pub fn own_tab(&self) -> Option<usize> {
        if !self.frames_coherent() { return None; }
        let pos = self.own_tab_position()?;
        self.tabs.iter().find(|t| t.position == pos).map(|t| t.tab_id)
    }

    /// Election, by strength. Confirmed ⇒ Presumed.
    pub fn elects_confirmed(&self) -> bool {
        self.own_tab().is_some_and(|id| self.tabs.iter().any(|t| t.tab_id == id && t.active))
    }
    /// The PRE-S0 computation, byte-for-byte, for effects that latch at emit
    /// and therefore cannot survive a fail-closed gate (see the gate table).
    pub fn elects_presumed(&self) -> bool {
        self.own_tab_position()
            .and_then(|pos| self.tabs.iter().find(|t| t.position == pos))
            .is_some_and(|t| t.active)
    }

    pub fn active_tab_id(&self) -> Option<usize> {
        self.tabs.iter().find(|t| t.active).map(|t| t.tab_id)
    }
```

**1.4 `bind_effects` — replace the permanent latch.** Replacing
`model.rs:414-439`; the existing body is:

```rust
    pub fn bind_effects(&mut self, own_tab: usize) -> Vec<Effect> {
        let own_position = self.tabs.iter().find(|t| t.tab_id == own_tab).map(|t| t.position);
        let mut out = Vec::new();
        for a in &self.agents {
            let joined_here = self.uuid_to_pane.get(&a.uuid)
                .and_then(|p| self.tab_position_of_pane(*p))
                .is_some_and(|pos| Some(pos) == own_position);
            if joined_here
                && a.tab_id != Some(own_tab)
                && self.sent_binds.get(&a.uuid) != Some(&own_tab)
            {
                self.sent_binds.insert(a.uuid.clone(), own_tab);
                out.push(Effect::Bind { uuid: a.uuid.clone(), tab_id: own_tab });
            }
        }
        out
    }
```

Keep the signature (an existing test calls it directly at `model.rs:1389-1420`).
Three changes to the body:

- `own_position` is now `self.tabs…position` **only when `frames_coherent()`** —
  callers reach it through `identity_effects`, but the direct-call path must not
  be a hole. Return `Vec::new()` early if `!self.frames_coherent()`.
- Confirmation clears the ledger: when `joined_here && a.tab_id == Some(own_tab)`,
  `self.bind_sent.remove(&a.uuid)` — so a *later* divergence re-arms the healer
  immediately, at full attempt budget. Also remove when `!joined_here` (the pane
  left our tab; the episode is over).
- The emit predicate becomes:

```rust
            let may_send = match self.bind_sent.get(&a.uuid) {
                None => true,                                  // never tried this episode
                Some(s) if s.tab_id != own_tab => true,        // new target → new episode
                // Progress-gated: only once the store has moved on since our
                // send, and only BIND_MAX_TRIES times. `seq` advances on a
                // real store mutation and nothing else, so a quiescent store
                // costs zero subprocesses no matter how many frames arrive.
                Some(s) => self.seq > s.at_seq && s.tries < BIND_MAX_TRIES,
            };
```

and on emit, `self.bind_sent.insert(a.uuid.clone(), BindSent { tab_id: own_tab,
at_seq: self.seq, tries: prev_tries_for_this_target + 1 })`.

**1.5 `identity_effects`** (new, adjacent to `bind_effects`):

```rust
    /// Every effect keyed on THIS instance's tab identity. Fail-closed by
    /// construction: elects_confirmed() is false while the frames disagree, so
    /// this returns nothing and the caller re-enters on the next frame.
    /// Called from EVERY external-input arm in main.rs — the single entry
    /// point exists because four of the five snapshot/frame arms used to have
    /// to remember a separate fire_binds() call, and one of them forgot (RC-B).
    pub fn identity_effects(&mut self) -> Vec<Effect> {
        if !self.elects_confirmed() { return Vec::new(); }
        let Some(own) = self.own_tab() else { return Vec::new() };
        let mut fx = Vec::new();
        // Birth touch FIRST: a newly-created tab wants its timeline stamp
        // before its bind, and needs_birth_touch is once-EVER per (instance,
        // tab) — the latch is consumed here and only here, i.e. only when we
        // are actually emitting (C5 rd 4: echo-gated re-arming is the storm).
        if let Some(active) = self.active_tab_id()
            && active == own
            && self.needs_birth_touch(active)
        {
            fx.push(Effect::Touch { tab_id: active });
        }
        fx.extend(self.bind_effects(own));
        fx
    }
```

Note `active == own`: today's code touches `active_id` from the tab frame while
gating on a *different* instance's position join — the same mismatched-frame
shape as RC-A, one line away. Requiring the active tab to be *our* tab makes the
touch self-consistent by construction.

### 2. `crates/clave-bar/src/main.rs` — the adapter shrinks

**2.1 Delete duplicated frame state.** `State` (`main.rs:15-37`): remove
`plugin_panes` (`:25`) and `last_tabs` (`:26-28`). They are verbatim copies of
`model.panes` / `model.tabs`, written in the same handlers.

**2.2 Delete the three joins.** `is_active_instance()` (`:43-54`),
`own_tab_id()` (`:60-71`), `model_tab_active_at()` (`:73-81`) — replaced by
`model.elects_confirmed()` / `model.elects_presumed()` / `model.own_tab()`.

**2.3 `run_effects` (`:87-217`) — two gates.** Replace

```rust
        let active = self.is_active_instance();
```

with

```rust
        // Two strengths (see the S0 gate table). `confirmed` requires the two
        // zellij frames to agree; `presumed` is the pre-S0 computation, kept
        // for the four effects that latch at emit and therefore cannot survive
        // a fail-closed gate. confirmed ⇒ presumed.
        let confirmed = self.model.elects_confirmed();
        let presumed = self.model.elects_presumed();
```

Then, arm by arm: `Effect::Bind … if confirmed` (`:145`),
`Effect::PruneTabs … if confirmed` (`:154`), new `Effect::Touch { tab_id } if
confirmed` → `run_command(&["clave","touch",&tab_id.to_string()], …)`. Leave
`ReanchorVisit` (`:117`), `RenameTab` (`:137`), `MarkRead` (`:140`) and
`PersistCollapse` (`:200`) on `presumed` — unchanged behaviour, and say so in a
comment so a later reader does not "tidy" them into `confirmed`.

**2.4 Replace `fire_binds` (`:222-231`).**

```rust
    fn settle_identity(&mut self) {
        let fx = self.model.identity_effects();
        if !fx.is_empty() { self.run_effects(fx); }
    }
```

**2.5 One snapshot path.** Both snapshot arms currently duplicate
`apply_snapshot` → `run_effects`, and only one of them kicks the binder — that
duplication *is* RC-B. Fold them:

```rust
    /// The ONE snapshot path (hydrate and clave-status both land here). Two
    /// call sites that each had to remember settle_identity() is how the
    /// hydrate came to be the only snapshot that never bound anything (RC-B,
    /// main.rs:393-412 vs :264-270).
    fn apply_snapshot_and_settle(&mut self, snap: clave_types::AgentSnapshot) {
        let fx = self.model.apply_snapshot(snap);
        self.run_effects(fx);
        self.settle_identity();
    }
```

`clave-status` (`:264-270`) and `RunCommandResult` (`:404-411`) both call it.

**2.6 `TabUpdate` arm (`:413-449`).** Drop `self.last_tabs = metas.clone();`
(`:423`) and pass `metas` by value. **Delete the inline birth-touch block
(`:434-445`)** — its comment (the C5 rd-4 rationale) moves onto
`Effect::Touch` and `identity_effects`. End with `self.run_effects(fx);
self.settle_identity();`.

Ordering note for the reviewer: the touch now runs *after* `PruneTabs` rather
than before. Both are fire-and-forget subprocesses with no arrival-order
guarantee either way, and their payloads are disjoint id sets (prune removes
observed-dead ids, touch stamps a live one), so they commute. The recycled-id
race between them is unchanged in kind and belongs to S3
(`ux-defect-dossier.md:330-341`).

**2.7 `PaneUpdate` arm (`:450-469`).** Drop `self.plugin_panes` bookkeeping
(`:452`, `:456`); still push every pane into `metas` including plugin panes (the
model's `own_tab_position` needs them, and `frames_coherent` reads all of them).
End with `self.model.apply_panes(metas); self.settle_identity();`.

**2.8 `clave-register` arm (`:276-286`).** `self.fire_binds()` →
`self.settle_identity()`.

**2.9 `clave-nav` arm (`:315-328`).** `self.own_tab_id()` → `self.model.own_tab()`
(now fail-closed). Comment the change: a nav in the renumbering window used to
resolve `Effect::SwitchTab`'s position off a mismatched pair.

**2.10 `load()` (`:380`).** `self.own_plugin_id = Some(get_plugin_ids().plugin_id);`
becomes `let id = get_plugin_ids().plugin_id; self.own_plugin_id = Some(id);
self.model.set_own_pane(id);` — `own_plugin_id` stays because `ShrinkSelf` /
`GrowSelf` (`:169-184`) still need it for `resize_pane_with_id`.

### 3. `crates/clave/src/store.rs` — observability only, no behaviour change

In `apply_bind` (`:227-256`), on the evicting branch only (`:239-245`), record
the eviction. `with_store_mut` holds an exclusive flock across the closure
(`store.rs:135-164`), so keep the call cheap and side-effect-free with respect to
the store; `log_event` opens `clave.log` append-only (`evlog.rs:16-30`) and
swallows its own errors (`:11-13`).

```rust
        if evicted {
            crate::evlog::log_event(
                "bind-evict",
                &format!("tab={tab_id} winner={uuid} evicted={evicted_uuids:?}"),
            );
        }
```

Collect `evicted_uuids: Vec<String>` in the existing loop. This is the only
non-`clave-bar` change in S0 and it writes nothing to the store.

---

## Test plan

Risk class, per the taxonomy at `docs/dev/TESTING.md:112-120`: **Pure logic /
model** for everything in §1, plus **Cross-process / IPC** for the bind-retry
rule (it changes the rate and ordering of `clave bind` subprocesses). The
cross-process row requires *"written argument for ordering/idempotency in the PR
dossier; adversarial reviewer must attack it"* — the storm argument in
§ Design is that written argument, and it must be pasted into the PR verbatim,
not linked. Tier 2 does not exist (#47), so nothing automated crosses the seam.
Recommend the **`needs-live-validation`** label: the failure this fixes is only
observable against a real fleet, and the live section below is the evidence.

### What must move to be testable at all

`crates/clave-bar/src/main.rs` is `test = false` (`crates/clave-bar/Cargo.toml`,
`[[bin]]`) because the bin links wasm host imports that have no host symbol.
**Nothing in it can ever be asserted on.** RC-A shipped and survived review
precisely because `is_active_instance`, `own_tab_id` and `model_tab_active_at`
live there. So the following must land in `model.rs` or it is untested by
construction — this is the *only* way to test S0 at Tier 1:

| Moves to `model.rs` | Was |
|---|---|
| `own_tab_position()`, `frames_coherent()`, `own_tab()` | `own_tab_id()` `main.rs:60-71` |
| `elects_confirmed()`, `elects_presumed()` | `is_active_instance()` `main.rs:43-54`, `model_tab_active_at()` `:73-81` |
| `active_tab_id()` | inline `main.rs:434` |
| birth-touch decision → `Effect::Touch` | inline `run_command` `main.rs:444` |
| `identity_effects()` | `fire_binds()` `main.rs:222-231` |

What is left in `main.rs` and therefore still unguarded, stated for the PR: the
`Effect → zellij shim` dispatch (which gate flag each arm reads), the
`apply_snapshot_and_settle` wiring, and the fact that `settle_identity()` is
called from all five arms. Mitigation is review-only — an adversarial reviewer
should be pointed at exactly those three, since #2.3 and #2.5 are where a
transcription slip would silently reinstate RC-B.

### Tier 1 — unit tests to add, in `model.rs`

TDD, red-first, in this order. All names follow the existing long-sentence
convention.

1. `own_tab_is_none_while_the_pane_and_tab_frames_disagree` — the dossier's
   reproduction (`ux-defect-dossier.md:91-99`) verbatim: tabs
   `10@0, 11@1, 12@2` with a plugin pane per tab; `set_own_pane(bar11)`;
   `apply_panes` covering positions `{0,1,2}`; then `apply_tabs` with the
   post-close set `{11@0(active), 12@1}` and **no** new `apply_panes`. Assert
   `own_tab() == None`, `elects_confirmed() == false`, and — the regression that
   matters — that `elects_presumed() == true` in the *same* state, pinning the
   difference the fix makes.
2. `own_tab_resolves_once_the_lagging_pane_frame_lands` — continue (1) with
   `apply_panes` over `{0,1}`; assert `own_tab() == Some(11)` and
   `elects_confirmed() == true`.
3. `identity_effects_emit_nothing_from_an_incoherent_frame_and_retry_on_the_next`
   — the fail-closed contract end to end: with a registered pane and a
   populated snapshot, the incoherent state returns `[]`; the following
   `apply_panes` returns `[Effect::Bind { uuid, tab_id: 11 }]`. This is the
   test RC-A would have failed.
4. `bind_re_emits_after_an_eviction_once_the_store_seq_advances` — bind,
   confirm via a snapshot carrying the join, then a higher-seq snapshot with
   `tab_id: None` (the `apply_bind` eviction, `store.rs:239-245`); assert a
   fresh `Effect::Bind`. Under `sent_binds` this returned `[]` forever.
5. `bind_is_silent_while_its_own_write_is_in_flight` — after an emit, repeated
   `identity_effects()` calls at the **same** `seq` return `[]` however many
   frames arrive. The debounce.
6. `bind_stops_after_bind_max_tries_against_an_unwinnable_target` — advance
   `seq` with contradicting snapshots in a loop; assert exactly
   `BIND_MAX_TRIES` emissions, then permanent silence for that target. The
   anti-storm bound.
7. `bind_ledger_clears_on_confirmation_so_a_later_divergence_rebinds_at_full_budget`
   — after 6 exhausts an episode, a confirming snapshot followed by a fresh
   divergence must emit again.
8. `birth_touch_is_deferred_not_consumed_when_the_frames_disagree` — pin the
   `&&` short-circuit as an intentional property, now with a trigger: no
   `Effect::Touch` on the incoherent frame, exactly one on the next coherent
   one, and never a second (`needs_birth_touch` once-ever, `model.rs:383-385`).
9. `birth_touch_targets_our_own_tab_only` — a coherent frame where the active
   tab is *not* ours emits no `Touch`.
10. Amend `bind_effects_report_own_tab_join_once_and_echo_independently`
    (`model.rs:1389-1420`) — it must keep passing unchanged (verified by
    inspection: its repeat call happens at an unadvanced `seq`, so the debounce
    keeps it silent; its confirming snapshot clears the ledger and `joined_here
    && a.tab_id == Some(11)` emits nothing). Update its doc comment from "the
    guard is last-SENT, never the snapshot echo" to the seq-clocked rule, and
    add an inline note on why seq-clocking is not the C5-rd-4 echo gate.

### Tier 1 — proptests to add (`model.rs` `mod proptests`, `:2797+`)

The escape record's lesson is that pure logic escapes through *unreached*
branches (`TESTING.md:121-126`): a new branch without a new property is a new
blind spot. Two properties, over a generated interleaving of `apply_tabs` /
`apply_panes` / `apply_snapshot` / `register`:

- `prop_bind_never_names_a_tab_that_does_not_hold_the_pane` — for every
  `Effect::Bind { uuid, tab_id }` emitted anywhere in the sequence, the pane
  registered for `uuid` sits in `tab_id` per the **most recent** manifest joined
  to the **most recent** tab set. This is the invariant RC-A violates, stated
  directly.
- `prop_identity_effects_are_empty_whenever_frames_are_incoherent` — the
  fail-closed contract as a universal, and the bind-count bound: emissions per
  `(uuid, tab)` episode never exceed `BIND_MAX_TRIES`.

### Tier 1 — the gate

```bash
cargo test --workspace   # --workspace is load-bearing (TESTING.md:36)
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

No CLI surface changes (no new subcommand, no new flag — `Effect::Touch` invokes
the existing `clave touch`), so no `Cli::try_parse_from` pin is owed. No
generated-artifact changes, so the KDL guardrail is untouched. `store.rs`'s
`log_event` addition needs no new test beyond the existing `apply_bind` unit
tests continuing to pass.

### Tier 3

Everything below. The bug is a race between two event streams in a live
multiplexer; no tier below 3 can observe it.

---

## Live validation

**Contract.** Every step below is the **maintainer's** to execute. The driving
agent prints commands and reads what he pastes back; it never runs `zellij`
against his session. The *one* sanctioned agent-side mutation is the sandbox
hot-reload in step 6, which is env-scoped to `clave-test`
(`TESTING.md:206-218`). A `zellij action` aimed at a dead session **blocks
forever without erroring** (`TESTING.md:231-236`) — that is why the agent must
not fire one on faith.

Paths are genericized (`$HOME`, `$TMPDIR`) because the pre-commit PII blocklist
rejects private local paths and has fired twice (`AGENTS.md:122-124`).

### Phase 0 — pre-flight (run before Phase 1 **and** again before Phase 3)

> **Step 1 — binary/bar version coherence.**
>
> **(a) He runs**, in the clave session:
> ```bash
> command -v clave; clave --version
> grep 'clave-bar: loaded' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -3
> ```
> **(b) He looks at** the path `clave` resolves to, its version, and the
> `v<X.Y.Z> build=<tag>` in the last loaded-bar lines.
> **(c) He reports** all three lines verbatim.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | versions match, one loaded-bar line per tab | clean; readings are trustworthy | continue to step 2 |
> | `clave --version` ≠ the `loaded v…` version | **issue #44/#43** — the bar shells out to a *different* binary; every store reading below is suspect (`ux-defect-dossier.md:534`) | **stop.** Report that S0 cannot be live-validated until #44 lands or he reinstalls; do not interpret any later step |
> | two different `loaded v…` versions in the tail | two plugin populations (the v0.1.1 incident, `TESTING.md:159`) | **stop.** Same as above |
> | `command -v clave` is under `.cargo/bin` while a release install exists | the PATH leak (CONTRIBUTING "The one leak") | **stop**, report the path |

### Phase 1 — baseline capture, before any fix

> **Step 2 — the store's view.**
>
> **(a) He runs:**
> ```bash
> clave ls --json | jq '{seq, tab_timeline,
>   agents: [.agents[] | {uuid, label, status, tab_id, stale, last_interacted}]}'
> ```
> **(b) He looks at** which agents have a `tab_id`, which are `null`, and which
> tab ids appear in `tab_timeline`.
> **(c) He reports** the whole JSON.
>
> Read-only and lock-free-safe: writers use temp+atomic-rename
> (`store.rs:120-129`), so this cannot tear or block the fleet.

> **Step 3 — zellij's ground truth, and the RC-A join.**
>
> **(a) He runs**, inside the live session:
> ```bash
> zellij action list-panes -t -j | jq -r '.[]
>   | select((.pane_command // "") | test("clave spawn"))
>   | [.tab_id, .tab_position, .id, .pane_command] | @tsv'
> ```
> Fallback if the JSON shape differs (the columns are `TAB_ID / TAB_POS /
> TAB_NAME / PANE_ID`, measured at ~0.04 s):
> ```bash
> zellij action list-panes -t
> ```
> **(b) He looks at** the `tab_id` each agent pane actually sits in. The agent
> pane's baked command is `clave spawn <uuid> …` (`spawn.rs`; the string is
> static, `ux-defect-dossier.md:256-259`), so the uuid is in the row.
> **(c) He reports** the full output.
>
> The driving agent now **joins step 2 to step 3 on uuid** and classifies:
>
> | If the join shows | That means | Do next |
> |---|---|---|
> | every agent's store `tab_id` == the tab its pane is in | no mis-bind present right now | continue to step 4 — the defect is a race, absence is not evidence |
> | agent A's store `tab_id` names a tab whose pane belongs to agent B | **RC-A confirmed**, the exact signature (`ux-defect-dossier.md:527`) | record it as the pre-fix baseline; continue to step 4 |
> | an agent has `tab_id: null` but its pane is listed | **RC-A eviction victim** (`store.rs:239-245`) | record; continue |
> | the eager agent has `tab_id: null` and *no* `tab_timeline` entry | **RC-B confirmed** — never bound, birth touch also missed | record; go to step 5 to discriminate lost-register from missed-kick |
> | a uuid in the store has no pane at all | dormant agent — expected, not a defect | ignore |

> **Step 4 — provoke the close race (the RC-A trigger).**
>
> **(a) He does**, with at least three tabs open and agents in at least two:
> focus the **lowest-positioned** tab (not the last one), press **`Alt+w`**.
> **(b) He looks at** the sidebar immediately: does a tab other than the one he
> was in jump to the top? does a live agent's row lose its coloured dot and a dim
> `◌` duplicate appear below?
> **(c) He reports** what the sidebar did, then re-runs **step 2 and step 3** and
> reports both.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | an unrelated tab moved to row 0 **and** the join (step 3) now disagrees | **RC-A reproduced live** — the strongest possible baseline | record verbatim; this is the case the fix must eliminate; go to step 5 |
> | a live agent shows a dim `◌` duplicate and its store `tab_id` is `null` | the eviction half of RC-A | record; go to step 5 |
> | a row moved but both stores agree with the panes | not RC-A — this is **RC-C**, the by-design live→dormant demotion (`ux-defect-dossier.md:196-212`), S1's problem | note it and continue; do not treat it as an S0 failure |
> | nothing visibly wrong | the race did not fire this time | repeat up to 3 times, varying which tab is closed; if still clean, record "not reproduced" and proceed — the Tier-1 tests carry the proof, the live pass is confirmation |

> **Step 5 — discriminate lost-register from missed-bind-kick (RC-B).**
>
> **(a) He runs** and then looks at his screen:
> ```bash
> grep '"cmd":"launch"' "$HOME/.local/state/clave/clave.log" | tail -3
> ```
> then, **while focused inside the eager (cold-start) tab**, looks at that tab's
> own sidebar.
> **(b) He looks at** whether a dim `◌` ghost row exists for the agent whose tab
> he is sitting in — and whether the *other* tabs' sidebars show the same ghost.
> **(c) He reports** the log lines and both observations.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | ghost `◌` visible **only** from inside that tab | the `clave-register` pipe was **lost** — permanent, and S0's hydrate fix cannot heal it (`ux-defect-dossier.md:178-181`) | record as out-of-scope; file the follow-up named in § Risks |
> | **no** ghost, but the tab still will not rise on a prompt | the **bind kick was missed** — this is exactly what §2.5 fixes | record as the pre-fix baseline for the RC-B assertion |
> | eager agent has a `tab_id` and rises normally | RC-B did not fire this cold start | record; RC-B validation moves to the sandbox cold start in step 7, which is deterministic |

### Phase 2 — the fix, in the sandbox

> **Step 6 — build and hot-reload the sandbox bar.**
>
> **(a) The agent runs** (this is the one sanctioned live mutation, and it is
> env-scoped to `clave-test`, never the maintainer's session):
> ```bash
> CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S) \
>   cargo build -p clave-bar --target wasm32-wasip1 --release
> cp target/wasm32-wasip1/release/clave-bar.wasm \
>   "$HOME/.local/state/clave-dev/data/clave-bar.wasm"
> ```
> **(b) The maintainer runs**, in a **non-zellij** terminal:
> ```bash
> clave dev reset          # prints the kill-session line for him first
> clave dev scenario c8-cold-start
> clave dev launch
> ```
> **(c) He reports** that `clave-test` is up and how many rows the sidebar shows.
>
> Do **not** run `just dev-install`, `cargo install` or `just release` — assume
> he is daily-driving (`AGENTS.md:46`). Note the caveat: a hot-reload
> reincarnates every bar model from scratch (`TESTING.md:335-338`), so state you
> were watching is gone — that is a confound *and* a tool.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | session up, 3 rows (`c8-cold-start` seeds 3 agents) | ready | step 7 |
> | `clave dev launch` hangs | a stale/exited `clave-test` session | ask him to run the kill line `clave dev reset` printed, then retry once |
> | the sidebar shows two bars per tab | wrong wasm loaded — the #44 class | **stop**; re-verify step 1 against `clave-test` |

> **Step 7 — RC-B: does the eager tab get bound at cold start?**
>
> **(a) He runs**, immediately after the session settles (no keypresses first):
> ```bash
> clave dev status | jq '{session_live, live_uuids,
>   agents: [.store.agents[] | {uuid, label, tab_id}], tl: .store.tab_timeline}'
> ```
> `clave dev status` is liveness-gated and reads the **sandbox** store
> (`dev.rs:262-265`) — in the sandbox that makes it the right tool, not the
> wrong one.
> **(b) He looks at** whether the eager agent has a non-null `tab_id` and an
> entry in `tab_timeline`.
> **(c) He reports** the JSON.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | eager agent has `tab_id` set **and** a `tab_timeline` entry | **RC-B fixed** — the hydrate now kicks the binder (§2.5) and the birth touch has a trigger (§1.5) | step 8 |
> | `tab_id` set, **no** timeline entry | bind healed, birth touch did not — check whether the eager tab was ever the *active* tab (`identity_effects` requires `active == own`); report which tab is focused | investigate before proceeding; likely a `active_tab_id()` mismatch |
> | `tab_id` still null | either the fix regressed or `clave-register` was lost | run step 5's ghost check inside the sandbox; if a ghost is visible only from inside that tab, it is the lost register (out of scope) — otherwise **the fix failed**, stop and report |

> **Step 8 — RC-A: the close race, in the sandbox.**
>
> **(a) He does:** `Alt+a` twice to reach four tabs, focus the **lowest-positioned**
> tab, press **`Alt+w`**. Repeat 5 times, varying which non-last tab is closed.
> **(b) He looks at** the sidebar after each close: does a live agent go dim
> `◌`? does an unrelated tab jump to row 0?
> **(c) He reports**, after the five closes:
> ```bash
> clave dev status | jq '{agents: [.store.agents[] | {uuid, tab_id}],
>   tl: .store.tab_timeline}'
> ZELLIJ_SESSION_NAME=clave-test zellij action list-panes -t
> grep '"cmd":"bind-evict"' "$HOME/.local/state/clave-dev/state/clave.log" | tail -20
> ```
> (the third command reads the new eviction log from §3 — it is the direct RC-A
> detector, and it is empty when nothing went wrong).
>
> | If he reports | That means | Do next |
> |---|---|---|
> | zero `bind-evict` lines, and the store/pane join agrees | **RC-A fixed** — no wrong bind was ever emitted | step 9 |
> | `bind-evict` lines whose `evicted` uuid still has a **live pane** in the same tab | the guard let a wrong bind through | **stop.** Ask for the zellij log filtered by the step-6 build tag; the coherence witness is being satisfied by a case it should reject (suspect a tab reorder — the documented residual) |
> | `bind-evict` lines whose `evicted` uuid has **no** pane | correct eviction after a tab-id reuse — the branch `store.rs:232-238` exists for | benign; continue to step 9 |
> | a row goes dim `◌` but the join agrees and there are no evict lines | **RC-C**, the by-design demotion — S1's, not S0's | note it; continue |

> **Step 9 — the anti-storm assertion (the thing that must not regress).**
>
> **(a) He runs**, right after step 8's five closes:
> ```bash
> wc -l < "$HOME/.local/state/clave-dev/state/clave.log"
> ZELLIJ_SESSION_NAME=clave-test zellij action dump-layout | head -5
> ```
> and then leaves the session **completely idle for 60 seconds** and runs the
> first command again.
> **(b) He looks at** whether the line count moved while idle, and whether the
> session feels responsive.
> **(c) He reports** both counts and any lag.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | identical counts across the idle minute | **quiescent store costs zero subprocesses** — the seq-gated debounce holds | step 10 |
> | the count grows while idle | a bind or touch loop is running: the `seq`-clocked debounce or the `BIND_MAX_TRIES` cap is not holding | **stop immediately.** This is the round-4 fd-exhaustion class (`SUBSYSTEM-VALIDATION.md:232-243`). Ask him to `clave dev reset` and report the growing lines |
> | zellij feels laggy or `dump-layout` is slow | possible spawn pressure | **stop**, same as above |

### Phase 3 — the maintainer's real fleet

> **Step 10 — re-run Phase 0 and Phase 1 against the stable session.**
>
> Only after **he** decides to install a tagged build — the agent never runs
> `just release` (`AGENTS.md:45`). Repeat step 1 (version coherence), then steps
> 2, 3 and 4 verbatim, and diff against the Phase-1 baseline.
>
> | If the diff shows | That means | Do next |
> |---|---|---|
> | Phase-1 had a disagreeing join, Phase-3 does not, across ≥5 closes | **S0 validated on the real fleet** | record in the PR dossier and drop `needs-live-validation` |
> | still disagreeing | the fix does not cover his interleaving | collect step 3's output plus `grep '"cmd":"bind-evict"' "$HOME/.local/state/clave/clave.log"` and reopen |
> | a *new* symptom: nav (Alt+j/k) occasionally does nothing | expected, and by design — `clave-nav` is now fail-closed (§2.9); a repeat keypress must work | confirm a repeat press always works; if it does **not**, the coherence witness is stuck false — ask for `zellij action list-panes -t` and the tab count |
> | a tab's label stops updating after a close | `RenameTab` regression — it must still be on the **presumed** gate (§2.3) | check §2.3 was not over-tightened |

---

## Risks and out-of-scope

**Risks taken.**

1. **Fail-closed drops user actions.** `clave-nav` and the binder now do nothing
   during the incoherence window. Nav is a repeatable keypress; the binder's
   retry is the very frame that closes the window. Accepted, and step 10's
   branch table watches for it.
2. **A count-preserving tab reorder defeats the witness.** `MoveTab` changes
   positions without changing the position *set*. clave binds no such key
   (`setup.rs:86-124`) and no in-process signal can detect it. Documented
   residual; a user who reorders tabs via a native binding can still produce one
   wrong bind — which now self-heals on the next seq advance, where before it was
   permanent.
3. **`PaneUpdate` delivery breadth is unverified from source.** `zellij-server`
   is not vendored (`ux-defect-dossier.md:228-230`), so "PaneUpdate reaches every
   instance" is inference from behaviour, not from code. The design is
   insensitive to it: an instance that never receives `PaneUpdate` simply never
   confirms, and refuses — the safe direction.
4. **The bind retry changes subprocess timing.** Bounded at `BIND_MAX_TRIES` per
   episode, progress-gated on `seq`, and single-writer under coherence. Step 9
   is the live assertion; the written argument is in § Design.
5. **`main.rs`'s dispatch stays unguarded.** Nothing automated can assert which
   gate flag each arm reads. Adversarial review of §2.3 and §2.5 is the only
   guard, and #47 (Tier 2) should list "one bind per agent per tab across a
   close" as a first scenario alongside the existing five (`TESTING.md:68-70`).

**Explicitly out of scope for S0.**

- **The lost `clave-register` pipe** (`spawn.rs:55-93`) — RC-B's permanent
  variant. Fire-and-forget, emitted once, dropped if the wasm is not yet loaded,
  and only the same-tab bar can heal it (`model.rs:426`). S0 makes it
  *diagnosable* (step 5's discriminator) but cannot fix it; it needs a re-send or
  a pull channel. File separately.
- **`ReanchorVisit` / `MarkRead` / `RenameTab` retry semantics** — all three
  latch at emit and drop silently under a false gate. S0 deliberately leaves
  their gate at pre-S0 strength rather than widen a known gap. The emit-time
  latch is the real defect and belongs with whoever owns §6.5/#23.
- **Everything RC-C, RC-D, RC-E, RC-F, RC-G.**

**What S1 and S3 get from this seam.**

- **S1 (RC-C, prompt→top ordering)** depends on `tab_timeline` entries being
  keyed to the *right* tab. Until S0 lands, any ordering experiment is confounded
  by wrong binds — a re-sorted list may be the sort maths or may be a mis-bind,
  and the two are indistinguishable from the screen. After S0, S1 can assume: an
  agent's `tab_id` names the tab its pane is in, or is `None`. S1 also inherits
  `Effect::Touch`, which is where a sub-second or monotonic timeline stamp would
  be plumbed if S1 chooses that route.
- **S3 (RC-E, tab-close correctness)** inherits three things: `PruneTabs` now
  fires only from a **confirmed** instance, which closes the "a frozen instance's
  payload lists a live tab as stale" hole (the #6 class) without changing the
  detection-driven retry it depends on (`model.rs:673-693`); `elects_confirmed()`
  /`elects_presumed()` exist as named, tested predicates to reason about the
  `ReanchorVisit` drop it must fix; and `identity_effects()` is the natural home
  for any close-time re-derivation, since it already runs on both frame kinds.
  S3 must **not** flip `ReanchorVisit` to `confirmed` without first giving it a
  retry — that ordering is load-bearing.
- **Merge order.** S0 alone, first, as the dossier sequences it
  (`ux-defect-dossier.md:561-563`). S1 and S3 are not parallel with each other;
  both rebase onto this.
