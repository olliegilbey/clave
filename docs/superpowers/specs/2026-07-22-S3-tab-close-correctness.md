# S3 — tab-close correctness (RC-E)

_Implementation spec · 2026-07-22 · workstream **S3** of
[the UX defect dossier](2026-07-22-ux-defect-dossier.md) · main `50fa26a`_

Read, in order: the dossier (RC-E `:300-367`), then
[S0](2026-07-22-S0-frame-coherence.md) and
[S1](2026-07-22-S1-ordering-semantics.md). S3 lands **after both** and rebases
onto them; § *Interfaces assumed* states exactly what it takes from each and
what breaks if either lands differently.

The maintainer's report, verbatim:

> *"when trying to close a tab, it can go all kinds of wrong, sometimes going
> idle, sometimes moving another tab to the top, not sure on what's going on."*

---

## 1. Problem

One keystroke — `Alt+w` → `CloseTab` (`crates/clave/src/setup.rs:95`) — trips
six independent mechanisms. S0 owns two of them, S1 owns one, and **four remain**
plus one that no workstream had claimed. Each produces a *different* wrong
outcome, which is why the symptom reads as "all kinds of wrong".

### 1.1 Scope split, stated first

| # | Mechanism | Symptom it produces | Owner |
|---|---|---|---|
| — | wrong-bind after position renumbering (RC-A) | a live agent loses its dot and a dim `◌` twin appears; the wrongly-bound tab climbs to row 0 on the next prompt; **sticky** | **S0 — out of scope here** |
| — | live→dormant demotion changes both key and tiebreak class (RC-C) | "an unrelated tab jumped to the top", every close, no race required | **S1 — out of scope here** |
| **C1** | recycled tab id + fire-and-forget prune (`store.rs:274-276`) | the brand-new tab sorts at the bottom, loses its glyph, its agent duplicates as `◌` | **S3** |
| **C1b** | the birth-touch latch is keyed on a **recycled** id and never re-arms (`model.rs:383-385`) | the brand-new tab is never stamped at all → sorts at the bottom **deterministically, with no race** | **S3 (new — see 1.3)** |
| **C2** | `ReanchorVisit` cannot retry (`model.rs:621-624`) | `Alt+j`/`Alt+k` go dead after a close until a mouse click (#23's residual) | **S3** |
| **C3** | dormant `◌` and `Status::Idle` `●` are both dim SGR 90 | "it went idle" — four different causes, one appearance | **S3** |
| **C4** | `SwitchTab` resolves a **position** from a frame that predates the renumbering | a nav or click right after a close lands on the wrong tab | **S3** |

Do not re-derive S0's or S1's halves; do not restate their fixes. Where this
spec touches their code it says so and quotes the post-S0/S1 form.

### 1.2 C1 — the recycled-id race

`get_new_tab_id` is `self.tabs.keys().last() + 1` over a `BTreeMap`
(cited in-code at `model.rs:660-663`, `store.rs:232-238`, `:262-263`), so
**closing the highest-id tab recycles that id onto the next tab created**.
The prune is a double-forked fire-and-forget subprocess; the store accepts it
whenever it wins the flock. The dossier's trace (`ux-defect-dossier.md:332-341`):

```text
t0  close highest-id tab 11 → bar emits PruneTabs{[11]}
t2  `clave prune-tabs 11` SPAWNED (fire-and-forget)
t3  before it takes the flock: a new tab is created → gets recycled id 11
    its bar birth-touches → tab_timeline[11] = now;  binds → u-new.tab_id = 11
t4  the t2 subprocess takes the flock:
      store.rs:284      tab_timeline.retain(…)  → deletes the FRESH entry
      store.rs:286-290                          → u-new.tab_id = None
t5  new tab sorts at key 0 (bottom), loses its glyph, u-new duplicates as ◌
```

This is **accepted in the source as residual** — `store.rs:274-276`:

```rust
/// Self-heal lives in the bar's detection (staleness re-derived each set
/// change), so a lost push is re-emitted while the entry persists. Residual: if
/// a listed id is REUSED within the subprocess-latency window a late removal
/// could unbind the new tenant — `apply_bind` eviction is the backstop and the
/// window is milliseconds. Empty payload = no-op (nothing observed dead).
```

Two claims in that comment are false in the direction that matters. *"`apply_bind`
eviction is the backstop"* — eviction runs when a **later** bind arrives; here the
bind arrived **first** and the prune erased it, and the bar cannot re-send because
`sent_binds` latches (`model.rs:429`). *"The window is milliseconds"* — true of the
window, false of the consequence: the damage is **permanent for the life of the
plugin instance**, because the birth touch is spent (`model.rs:383-385`) and the
re-bind is latched out. S0 fixes the re-bind half. Nothing fixes the timeline half.

The prune's payload design is otherwise correct and stays: remove-listed ids are
idempotent and commute (`model.rs:61-68`, `store.rs:258-277`, the #6/PR-26
escape, `TESTING.md:162`). What is missing is not a different payload but a
**freshness witness**: the emitter cannot see, and the applier cannot ask,
whether the id it is deleting still refers to the tab that died.

### 1.3 C1b — the birth touch is spent on a recycled id (deterministic, no race)

`model.rs:383-385`:

```rust
pub fn needs_birth_touch(&mut self, tab_id: usize) -> bool {
    !self.timeline.contains_key(&tab_id) && self.birth_touched.insert(tab_id)
}
```

`birth_touched` is inserted at `:384` and **never removed** — grep returns
exactly three sites: the declaration (`:190`), the `Default` (`:303`), and that
`insert`. The field doc says so out loud: *"Never re-armed, even if a snapshot
drops the tab"* (`:189`).

Both guards are keyed on the tab **id**, and zellij recycles ids. So:

```text
tabs 10, 11 (11 is the highest id).  Alt+w closes 11.  Alt+t creates a tab.
zellij mints id 11 again.  The surviving bar already has 11 in birth_touched.
needs_birth_touch(11) → insert returns false → NO `clave touch`.
tab 11 has no timeline/tab_order entry → sort key 0 → BELOW every dormant row.
```

No subprocess race, no stale frame, no interleaving: **closing the highest-id
tab and creating another is sufficient**. The mirror guard does not save it
either — once the prune echo lands, `timeline.contains_key(11)` is false, but
`birth_touched.insert(11)` is still false. The dossier names the spent latch as
part of C1's unrecoverability (`:344`); it is in fact a defect in its own right,
it fires far more often than the race, and it is the cleanest explanation of
*"sometimes moving another tab to the top"* that survives S0 and S1: the new tab
does not go to the top, so whatever is at the top **is** another tab.

### 1.4 C2 — `ReanchorVisit` is emitted exactly once and never retried

`apply_tabs` (`model.rs:628-638`):

```rust
        if let Some(active_id) = self.tabs.iter().find(|t| t.active).map(|t| t.tab_id)
            && self.current_tab != Some(active_id)
        {
            if birth_or_organic {
                self.current_tab = Some(active_id);
                effects.push(Effect::AnnounceVisit { tab_id: active_id });
            } else if stranded {
                self.current_tab = Some(active_id);
                effects.push(Effect::ReanchorVisit { tab_id: active_id });
            }
        }
```

`stranded` is `self.current_tab.is_some_and(|id| !live.contains(&id))` (`:603`).
The mutation at `:635` happens **before** the effect is handed to the executor,
so by the time `run_effects` drops it (`main.rs:117`, `active == false`)
`stranded` is already false and can never become true again for that close. The
trade is documented at `:620-624`:

> *"Accepted trade: if is_active_instance is transiently false on the close
> TabUpdate (PaneUpdate lag), the reseed is DROPPED and nav stays stranded until
> a click — the pre-fix symptom, but in a narrow window and strictly better than
> a storm."*

That window is not narrow: the close **is** the event that makes the frames
disagree, so the close `TabUpdate` is precisely the update most likely to find
the gate false. The result is #23's original symptom — `Alt+j`/`Alt+k` dead until
a mouse click — reappearing on some fraction of closes. S0 keeps this effect on
the *weak* (`Presumed`) gate deliberately, and instructs S3 not to tighten it
before giving it a retry (`S0:1096-1097`). § 2.2 does exactly that.

### 1.5 C3 — four causes, one appearance

Dossier table (`ux-defect-dossier.md:346-356`), reproduced with the cause
numbers used throughout this spec:

| # | Path to something that *looks* idle | file:line | Close-triggered? | Correct? |
|---|---|---|---|---|
| **I1** | `SessionEnd` hook → `Status::Idle` | `hook.rs:45`, registered `setup.rs:255` | yes, directly — pane dies, claude exits | **correct** |
| **I2** | `apply_focus` Done→Idle on a *different* agent (close forces focus onto a survivor → `MarkRead`) | `store.rs:201-203`, `model.rs:648-654` | yes, collaterally | **correct** |
| **I3** | dormant glyph `('◌', 90)` — the same dim 90 as `Status::Idle`'s `('●', 90)` | `model.rs:775`, `clave-types/src/lib.rs:29` | yes, on **every** close | **correct-but-illegible** (#24) |
| **I4** | RC-A wrong-bind eviction renders a live agent dormant | `store.rs:239-245`, `model.rs:479-488` | yes | **wrong — S0's** |

`apply_prune_tabs` never writes `status` (`store.rs:278-298`), so the close
itself cannot make anything genuinely idle. Every "it went idle" is one of these
four, and the user cannot tell them apart because I3 and I4 render identically
to I1 — dim gutter, same size, same colour.

### 1.6 C4 — `SwitchTab` carries a position, and positions are the thing that moved

`nav` (`model.rs:888-902`) and `click` (`model.rs:805-818`) both resolve
`t.position` out of `self.tabs` and emit `Effect::SwitchTab { position }`, which
`main.rs:96-101` executes as `switch_tab_to(position as u32 + 1)`. A close
renumbers every position after the closed index, so a nav or click processed
between the close and the renumbering `TabUpdate` lands one tab off. Under S0 the
nav path additionally requires `Election::Confirmed`, which fails while the
frames disagree — but the blind case where **both** frames still predate the
close passes the coherence witness and is still wrong, and the `Mouse` arm
(`main.rs:498-505`) is not gated at all.

---

## 2. Design

### 2.1 C1 + C1b — one witness fixes both: the seq at which we watched the id die

**Adopted: direction (a), verified at apply time, with the fence value pinned at
first detection and carried per death-cohort.**

The payload keeps its shape (observed-stale ids — idempotent, commuting) and
gains one number: `--at-seq S`, the store `seq` the bar held when it *first*
observed those ids die. `apply_prune_tabs` removes an entry only if it was
written no later than `S`.

The test is expressible because of S1. After S1, `tab_order[tab_id]` is a
**commitment ordinal minted from the store's own `seq` under the flock**
(`S1:158-165`, `Store::mint_ord`), and `prop_ordinals_are_a_total_order` asserts
every ordinal is `≤ seq` (`S1:867`). So:

```rust
let written_at = s.tab_order.get(&id).copied().unwrap_or(0);
let fresher_than_our_observation = written_at > at_seq;
```

is exactly *"this entry was written after the state in which I saw the tab die"*.
**The direction's guess is right: under S1 this falls out almost free** — no new
map, no new timestamp, no schema change to `AgentSnapshot`. The only additions
are one optional CLI flag and one field on `Effect::PruneTabs`.

**Why it is sound, both directions.**

- *Never refuses a genuinely dead entry.* The bar lists `X` as stale only
  because `X` is in **its mirror**, and its mirror is a snapshot at `seq == S`.
  Every ordinal present in a snapshot at seq `S` was minted at some `ord ≤ S`.
  So the dead entry always satisfies `ord ≤ at_seq` and is always removed.
- *Always refuses a recycled entry.* The new tab's birth touch mints its ordinal
  **after** the bar observed the death, and `seq` is monotonic under the flock,
  so `ord > S` unconditionally.
- *The fence value must be pinned, not refreshed.* Emission is
  detection-driven and re-derives every `TabUpdate` (`model.rs:673-693`). If the
  retry re-read `self.seq`, a bar that had meanwhile accepted the snapshot
  containing the **fresh** ordinal would emit a bound that outranks it and prune
  the live tenant — the original bug with extra steps. So the first observation
  of a death pins `S` for that id, in a new `observed_dead: BTreeMap<usize, u64>`,
  and every retry re-uses it.

**The same map fixes C1b, which is why they are one change.** An observed close
is first-hand evidence that *everything keyed on that id is now untrustworthy* —
the store's mirror entry (the dead tab's, echo pending) **and** the once-ever
birth-touch latch. `observed_dead` is that evidence, so `needs_birth_touch`
consults it and the observation clears `birth_touched`. One field, two defects,
and the invalidation is scoped to first-hand observation rather than to any
snapshot that happens to drop an entry — which keeps
`birth_touch_fires_once_ever_and_defers_to_snapshot_knowledge`
(`model.rs:1272-1290`) passing **byte-identically**, including its final
assertion that a snapshot dropping a tab must not re-arm the guard.

**Per-cohort grouping.** Different ids can have different pinned seqs (a second
close before the first prune landed). One flag cannot express two bounds, so
`apply_tabs` groups the stale set by pinned seq and emits **one `PruneTabs` per
cohort**, ordered by seq ascending for determinism. In the overwhelmingly common
case there is exactly one cohort and exactly one subprocess, as today.

**Rejected alternatives.**

| Option | Failure mode |
|---|---|
| **(b) carry the uuid the emitter believed held the tab; prune only if the store still agrees** | Fails the reachable *same-uuid* recycle: close the highest-id tab hosting `u-A`, then dwell-open `u-A` from its dormant row (`Effect::OpenAgent`, `model.rs:83-85`); the new tab takes the recycled id and re-binds **`u-A`**, so the store still "agrees" and the late prune unbinds a live agent. It is also silent about the `tab_order` leg, which is the half that does not self-heal: stale ids derived from `self.timeline` carry no uuid at all (`model.rs:701-706`). |
| **(c) a generation number the bar owns, with the store idempotent against it** | A bar-owned generation is not comparable across instances (each bar's counter is private) and a store-owned one is `seq` under a different name — the store already has exactly one monotonic counter and S1 just finished arguing that a second counter is a silent-corruption hazard (`S1:278`). This collapses into (a). |
| **A whole-payload fence: refuse the prune if `store.seq != at_seq`** | The store advances `seq` on every hook event, so an unrelated `Stop` between spawn and flock kills the prune. Refusals then wait for the next `TabUpdate`, which after a close may never come — the exact unbounded-stale-window that made emission detection-driven in the first place (`model.rs:674-681`, PR #26/Codex P2). |
| **Make the prune synchronous (bar blocks on the result)** | The bar has no blocking call, and per-event subprocess pressure from the bar is the fd-exhaustion incident (`SUBSYSTEM-VALIDATION.md:232-243`). |
| **Stop recycling ids** | Not ours: `get_new_tab_id` is zellij's (`screen.rs:1617`). |
| **`bind_ord` on `AgentRecord`: fence the bind leg on its own write seq too** | Considered and **deferred**, not rejected outright. It would close the residual in § 5.1 (a bind written before the prune lands is still unbound) at the cost of a new store field written at one site (`apply_bind`) and cleared at three (`apply_prune_tabs`, `clear_session_order`, the eviction branch) — a four-site invariant guarding a case S0 already heals within one `seq` advance. If live validation step 5 shows the flicker is visible, this is the follow-up. |

### 2.2 C2 — a capped, ack-gated retry ladder; the first emit keeps S0's weak gate

S0's instruction is explicit (`S0:1096-1097`): *"S3 must not flip `ReanchorVisit`
to `confirmed` without first giving it a retry — that ordering is load-bearing."*
**I build on it and go one step further: even with a retry, the first emit stays
on `Presumed`.**

The argument for not tightening at all is in S0's own analysis
(`S0:238-240`): `ReanchorVisit`'s payload is derived purely from the **fresh**
`self.tabs`, so a mis-elected instance still emits the *correct* tab id. Wrong
election is harmless here and catastrophic for `Bind`. Tightening therefore buys
nothing and costs exactly the drop we are fixing. So:

- **Attempt 1** — `Effect::ReanchorVisit`, gated `Presumed`. Byte-identical to
  today; #23's shipped fix is untouched and cannot regress.
- **Attempts 2..N** — `Effect::ReanchorRetry`, a **distinct variant with the same
  payload**, gated `Confirmed`. Retries only ever fire from an instance whose two
  frames agree and whose own tab is the active one — the strongest gate available.
- **Ack** — any `clave-visited` pipe clears the debt. Our own announce echoes
  back to every instance including the sender (stated at `model.rs:344-348`), and
  *any* beacon naming a live tab achieves what the re-anchor wanted: a fleet-wide
  `current_tab` inside the live set.
- **Cap** — `REANCHOR_MAX_RETRIES = 2`, then silence for that episode.

Using a distinct effect variant purely to obtain a different gate is not novel —
it is the precedent this very code established when it split `ReanchorVisit` out
of `AnnounceVisit` for exactly that reason (`model.rs:47-54`).

**Why this is not the C5-round-4 echo-gated storm.** That incident
(`SUBSYSTEM-VALIDATION.md:232-243`) was an *uncapped* guard that re-armed on
every `TabUpdate` while echoes were congested, from *every* instance, spawning a
subprocess each time. Here: the trigger is capped at 3 total emissions per
episode; retries require `Confirmed`, which at most one instance can satisfy;
the debt is cleared by the first beacon of any kind; and the episode itself
requires a stranded beacon, which requires an observed close. Worst case is
**3 pipes per close from one instance**, against round 13's sustained ~15/s from
every instance.

**Rejected alternatives.**

| Option | Failure mode |
|---|---|
| **Drop the local `current_tab = Some(active_id)` mutation so `stranded` stays true and retries naturally** | That mutation is what makes nav work locally the instant the close lands, and — per `model.rs:617-620` — it is what bounds burst-tripped hidden instances to one arm each. Removing it re-opens the round-13 beacon war and delays the local fix. |
| **Tighten `ReanchorVisit` to `Confirmed` with the retry** | Widens the drop window on attempt 1 for no benefit (payload is frame-independent, `S0:238-240`) and regresses #23 in exactly the shape S0 warned about. |
| **Uncapped detection-driven retry, mirroring the prune** | The prune self-limits on the store echo clearing the mirror; the beacon has no store to echo it. An unacked beacon would retry at `TabUpdate` rate forever. |
| **Re-derive `stranded` from a separate "beacon owed" flag cleared only by our own pipe id** | Pipes carry no correlation id (`PipeMessage`), so "our own" is not expressible. Any-beacon-acks is both simpler and strictly more convergent. |

### 2.3 C3 — visual for I3, documentation for I1/I2, nothing behavioural

**Decision, per cause:**

| Cause | Fix | Rationale |
|---|---|---|
| **I3** — dormant `◌` reads as idle | **visual** — the dormant glyph stops sharing the status palette's shape class | This is the only one of the four that is a *rendering* defect. The row is correct; it is unreadable. |
| **I1** — `SessionEnd` → real `Idle` | **documentation** | Correct behaviour. What is missing is a legend the maintainer can check against, and the live diagnostic table in § 5. |
| **I2** — a *different* agent flips Done→Idle via collateral `MarkRead` | **documentation** | Correct behaviour, and genuinely surprising: the close moves focus, focus marks the survivor read. It needs one sentence in the design doc, not code. |
| **I4** — eviction renders a live agent dormant | **nothing here** | S0's. Post-S0 it should not occur; the I3 fix is what makes its *recurrence* legible if it ever does. |

**No behavioural change.** A closed tab leaving a dormant row is the product —
it is the claude.ai-style list that `Effect::OpenAgent` resurrects from
(`model.rs:83-85`, C8), and after S1 the row **holds its index** across the
close (S1 R2, `S1:149-150`), so the only perceptual delta is the glyph itself.
Making that delta legible is the whole job. Suppressing or delaying the dormant
row would break resurrection and re-open #24 from the other side.

**The visual change.** `model.rs:770-777` builds the dormant gutter inline; the
base glyph becomes a named module const beside the other `model.rs` consts so it
is greppable and testable:

```rust
/// The DORMANT row glyph (§6.6 C8): a conversation with no tab, dwell to
/// resurrect. Deliberately NOT a filled dot: `Status::Idle` is ('●', 90)
/// (clave-types/src/lib.rs:29) and every close turns a live row dim, so a dim
/// FILLED dot is read as "the agent went idle" when it means "the tab is gone"
/// (dossier RC-E, cause I3; issue #24). Hollow-vs-filled is the distinction;
/// the dim SGR 90 stays, because a dormant row must still be quiet.
const DORMANT_GLYPH: (char, u8) = ('○', 90);
```

`○` (U+25CB) over the incumbent `◌` (U+25CC, dotted circle): both are BMP
geometric shapes in the same family already used by this codebase
(`● ✗ ↻ ✖`), so font risk is unchanged, but the hollow/filled pair is the
conventional absent/present encoding and survives a 30-column glance where a
dotted ring does not. **This is a Tier-3 judgement** (`TESTING.md:119`, visual
⇒ `host-untestable`), so live validation step 6 puts three candidates in front
of the maintainer and he picks; the const is the one line that changes.

**Rejected alternatives.**

| Option | Failure mode |
|---|---|
| **Recolour dormant out of SGR 90 (e.g. 34/36)** | The status palette is 31/33/32/90 (`clave-types/src/lib.rs:24-32`); a fifth hue in the gutter reads as a fifth *state*, and a bright dormant row inverts the intended visual weight. Offered as candidate C in step 6 only. |
| **Change `Status::Idle`'s glyph instead** | `Status::glyph()` is shared with `clave ls` (`lsview.rs`) and is spec'd at §6.5; the blast radius is the whole status vocabulary for a defect that lives in one bar-local branch. |
| **Structure instead of glyph (indent, bracket, separator)** | `Row` is `{key, name, active, glyph}` and the render is a 2-cell gutter with a **non-escape-aware** width clamp (`main.rs:539-557`, dossier RC-G) — restructuring the gutter is S5's seam, and doing it here guarantees a conflict. |
| **Suppress the dormant row for N seconds after a close** | Breaks S1's R2 (the row must hold its index), breaks resurrection discoverability, and adds a timer to the one path that must not gain one. |

### 2.4 C4 — carry the identity now, switch by identity once it is proven

**Research finding, from the vendored source.** The plugin wire protocol *does*
have identity-addressed tab focus: `PluginCommand::SwitchTabToId(u64)` and
`GoToTabWithId(u64)` — `zellij-utils-0.44.3/src/data.rs:3491-3492`, with proto
names at `plugin_command.proto:187` and round-trip codecs at
`plugin_command.rs:1861-1871`, `:3658-3666`. But **zellij-tile 0.44.3 exposes no
shim wrapper for either** — `grep` over `zellij-tile-0.44.3/src/` returns nothing
— while it *does* wrap the sibling id-addressed actions (`rename_tab_with_id`
`shim.rs:1452`, `close_tab_with_id` `:2326`). Issuing the command from clave-bar
is possible: `object_to_stdout` is public (`shim.rs:2792`) and `PluginCommand` is
re-exported through the prelude, but `host_run_plugin_command` is a **private**
extern (`shim.rs:2903-2906`), so clave-bar would have to declare its own
`#[link(wasm_import_module = "zellij")] extern "C"` import. And the handler lives
in `zellij-server`, which is **not vendored** — so per AGENTS.md
(*"never trust an assumed Zellij behaviour"*) its behaviour is **unverifiable
from source**.

The risk is asymmetric: the defect is a millisecond-wide mis-target; a
`SwitchTabToId` that the server silently ignores is **total nav death**. So:

- **Now:** `Effect::SwitchTab` carries `{ position, tab_id }`. Execution stays
  `switch_tab_to(position + 1)` — byte-identical behaviour.
- **Now:** the `SwitchTab` **emission** is gated on frame coherence in the model,
  which extends S0's witness to the `Mouse`/`click` path that S0 left ungated
  (`main.rs:498-505`) and makes the drop testable at Tier 1.
- **Then:** live-validation step 7 is a sandbox A/B that decides
  `SwitchTabToId`. If it moves focus, `main.rs`'s arm becomes a two-line
  id-addressed call and the position field is deleted in a follow-up. If it does
  not, the field never ships and the coherence gate is the whole fix. Either way
  the model, the tests and the effect payload are unchanged — the decision costs
  one adapter line.

**Positional execution is the interim, not an accepted fallback (CodeRabbit,
2026-07-22).** The coherence gate closes the *renumbering-in-progress* window
(frames of different sizes), but it does **not** close the coherent-but-stale
case S0 Residual 2 documents — close the lowest tab and create one, and both
frames agree on a position set that no longer means what the emitter thinks. In
that window `switch_tab_to(position + 1)` still targets the wrong tab. Therefore
**id-addressed switching is the required end state, and positional is explicitly
the interim** pending step 7's sandbox decision — it is presented as a fallback
only because `SwitchTabToId` traverses the unvendored server half and a silent
no-op is total nav death, so it must be proven live before it can ship. Until it
does, this spec inherits S0 Residual 2's live stop condition verbatim: a
one-frame focus flicker after closing the lowest-numbered tab is expected; a
switch that lands on and *stays* on the wrong tab is an S3 failure, not a
residual. The gate is a mitigation of the class, not a proof it is closed.

**Rejected alternatives.**

| Option | Failure mode |
|---|---|
| **Verify-and-correct: remember the intended `tab_id`, and on the next `TabUpdate` re-switch if the active tab is not it** | **Structurally impossible.** `TabUpdate` reaches only the **active** tab's instance (C3, `SUBSYSTEM-VALIDATION.md:646-651`) — after a successful switch that is the *target's* bar, not the emitter's. The emitter can never observe its own outcome. |
| **Let the target's instance correct the beacon→active mismatch** | That instance cannot distinguish "I was mis-switched to" from "the user switched here deliberately", and acting on the difference is a two-instance beacon ping-pong (round 13). |
| **Ship the raw `SwitchTabToId` extern import immediately** | Unvendored server half; a silent no-op is total nav death. Gated behind step 7 instead. |
| **Resolve the position at execution time in `main.rs`** | `main.rs` reads the same two frames the model does, is `test = false`, and would move a decision back out of the tested half — the precise structural mistake S0 spent its spec undoing. |

---

## 3. Implementation

Numbered, file by file. Quoted blocks are the code being replaced, in its
**post-S0/S1** form where those specs already rewrote it.

### 3.1 `crates/clave-bar/src/model.rs`

**1. New state on `BarModel`** (beside `birth_touched`, `model.rs:186-190`):

```rust
    /// tab_id → the store `seq` this instance held when it FIRST observed that
    /// id die. Two jobs, one witness (S3 §2.1):
    ///   (a) the prune fence — `clave prune-tabs --at-seq S` refuses to remove
    ///       an entry written after S, so a RECYCLED id (get_new_tab_id =
    ///       max-key+1, screen.rs:1617) cannot be erased by a prune that was
    ///       computed before it was born. PINNED at first observation and
    ///       never refreshed: a retry that re-read self.seq would eventually
    ///       outrank the fresh entry and reproduce the bug.
    ///   (b) invalidation — an observed close is first-hand evidence that
    ///       everything keyed on that id is stale, including our own
    ///       once-ever birth-touch latch (see needs_birth_touch).
    /// Bounded: entries are dropped as soon as the store mirror has nothing
    /// left to prune for that id (apply_snapshot).
    observed_dead: BTreeMap<usize, u64>,
    /// The re-anchor beacon we owe and how many RETRIES we have spent (#23,
    /// S3 §2.2). `None` = nothing owed. Cleared by any `clave-visited` pipe:
    /// our own announce echoes back to every instance, and ANY beacon naming a
    /// live tab already achieves what the re-anchor wanted.
    reanchor_owed: Option<(usize, u32)>,
```

`Default` (`model.rs:288-324`): `observed_dead: BTreeMap::new()`,
`reanchor_owed: None`.

**2. New consts**, beside the existing module consts (`model.rs:137-151`):

```rust
/// Re-anchor beacon retries after the first (Presumed-gated) attempt, each
/// Confirmed-gated (S3 §2.2). 3 total emissions per close episode bounds the
/// round-13 beacon-war class; the debt clears on the first beacon of any kind.
const REANCHOR_MAX_RETRIES: u32 = 2;

/// The DORMANT row glyph (§6.6 C8) — see S3 §2.3. Hollow, not filled:
/// Status::Idle is ('●', 90) and every close turns a live row dim, so a dim
/// FILLED dot reads as "the agent went idle" when it means "the tab is gone".
const DORMANT_GLYPH: (char, u8) = ('○', 90);
```

**3. `Effect` changes** (`model.rs:34-92`):

```rust
    /// switch_tab_to(position + 1) — row/dir nav. All instances compute the
    /// same target from replicated state, so duplicates are idempotent.
    SwitchTab { position: usize },
```

becomes

```rust
    /// switch_tab_to(position + 1) — row/dir nav. All instances compute the
    /// same target from replicated state, so duplicates are idempotent.
    /// `tab_id` is the INTENT; `position` is how we currently have to express
    /// it. zellij-tile 0.44.3 wraps no id-addressed focus action (the wire
    /// protocol has SwitchTabToId/GoToTabWithId, data.rs:3491-3492, but the
    /// shim exposes neither and the server half is unvendored) — S3 §2.4 and
    /// its live step 7 decide whether the adapter can use the id directly.
    /// Emission is coherence-gated so a position from a frame that predates a
    /// close renumbering is never sent.
    SwitchTab { position: usize, tab_id: usize },
```

```rust
    PruneTabs { stale_ids: Vec<usize> },
```

becomes

```rust
    /// run_command(["clave","prune-tabs","--at-seq",at_seq, stale_ids…]).
    /// Payload semantics are UNCHANGED (observed-stale ids: idempotent,
    /// commuting — #6/F3). `at_seq` is the store seq this instance held when
    /// it FIRST observed these ids die: the store removes an entry only if it
    /// was written no later than that, which is what stops a late prune from
    /// erasing a RECYCLED id's fresh entry (S3 §2.1). One effect per death
    /// cohort — different ids can carry different pinned seqs.
    PruneTabs { stale_ids: Vec<usize>, at_seq: u64 },
```

and a new variant beside `ReanchorVisit`:

```rust
    /// run_command zellij pipe clave-visited — the RETRY leg of ReanchorVisit
    /// (#23 / S3 §2.2). Same payload, DISTINCT variant purely so run_effects
    /// can gate it to Election::Confirmed while attempt 1 stays on the
    /// pre-S0/S3 Presumed gate (same trick that split ReanchorVisit out of
    /// AnnounceVisit above). Capped at REANCHOR_MAX_RETRIES per episode.
    ReanchorRetry { tab_id: usize },
```

**4. `needs_birth_touch`** (`model.rs:379-385`). Replacing:

```rust
    pub fn needs_birth_touch(&mut self, tab_id: usize) -> bool {
        !self.timeline.contains_key(&tab_id) && self.birth_touched.insert(tab_id)
    }
```

with (field name `tab_order` per S1 §4.7):

```rust
    /// Should this instance fire `clave touch` for a newly-active tab it has
    /// never seen? True at most ONCE per (instance, tab INCARNATION).
    ///
    /// S3: "incarnation", not "id". zellij RECYCLES tab ids, and BOTH guards
    /// below are keyed on the id — the store mirror (which may still hold the
    /// DEAD tab's ordinal, prune echo pending) and the local once-ever latch
    /// (never removed: model.rs:189). Closing the highest-id tab and opening
    /// another was therefore sufficient, with NO race, to leave the new tab
    /// unstamped forever → sort key 0 → below every dormant row (dossier
    /// RC-E). An OBSERVED close invalidates both; `observed_dead` is that
    /// observation, and apply_tabs clears the latch when it records one.
    /// A snapshot merely DROPPING a tab still does not re-arm anything —
    /// that is second-hand and stays pinned by
    /// birth_touch_fires_once_ever_and_defers_to_snapshot_knowledge.
    pub fn needs_birth_touch(&mut self, tab_id: usize) -> bool {
        let mirror_is_trustworthy = !self.observed_dead.contains_key(&tab_id);
        if self.tab_order.contains_key(&tab_id) && mirror_is_trustworthy {
            return false;
        }
        let fire = self.birth_touched.insert(tab_id);
        if fire {
            // The stamp we are about to mint supersedes the death we recorded.
            self.observed_dead.remove(&tab_id);
        }
        fire
    }
```

**5. `apply_snapshot`** (`model.rs:519-528`, post-S1 `self.tab_order = snap.tab_order`).
Immediately after the assignment, add:

```rust
        // Forget deaths the store has already acted on. Nothing left to prune
        // for an id means nothing left to distrust about it — and this is what
        // bounds `observed_dead` by the mirror's own size.
        let mirror: BTreeSet<usize> = self
            .tab_order
            .keys()
            .copied()
            .chain(self.agents.iter().filter_map(|a| a.tab_id))
            .collect();
        self.observed_dead.retain(|id, _| mirror.contains(id));
```

**6. `apply_tabs` — the re-anchor ladder** (`model.rs:628-638`). The
`if birth_or_organic / else if stranded` block keeps its shape; the `stranded`
arm gains the debt, and a retry block follows it:

```rust
            } else if stranded {
                self.current_tab = Some(active_id);
                // Attempt 1 keeps the PRESUMED gate — byte-identical to the
                // shipped #23 fix. The debt is what makes the drop recoverable.
                self.reanchor_owed = Some((active_id, 0));
                effects.push(Effect::ReanchorVisit { tab_id: active_id });
            }
        }
        // #23 retry ladder (S3 §2.2). `current_tab` is mutated above BEFORE
        // the executor can drop the effect, so `stranded` is false forever
        // after and the beacon was never re-broadcast — Alt+j/k went dead
        // until a click (documented trade, model.rs:620-624). The debt
        // survives that mutation; the retries are Confirmed-gated in
        // run_effects, and ANY incoming beacon clears them (visited()).
        //
        // CRITICAL (CodeRabbit 2026-07-22): `tries` is NOT incremented here.
        // A non-confirmed instance also runs apply_tabs, so counting at
        // enqueue would let every ineligible instance burn the whole budget
        // before any confirmed instance sends — and if confirmation arrives
        // late, the debt is already exhausted. The budget is spent only on an
        // ELIGIBLE (Confirmed-gated) send, which happens in run_effects. Here
        // we only (re)arm the effect; the counter advances at the send site.
        if let Some((target, tries)) = self.reanchor_owed {
            if !self.tabs.iter().any(|t| t.tab_id == target) {
                self.reanchor_owed = None; // target died too — a new episode will arm
            } else if tries < REANCHOR_MAX_RETRIES
                && !effects
                    .iter()
                    .any(|e| matches!(e, Effect::ReanchorVisit { .. }))
            {
                // Re-arm only. Do NOT bump `tries` — see note above.
                effects.push(Effect::ReanchorRetry { tab_id: target });
            }
        }
```

**The retry counter advances at the send, not the enqueue (CodeRabbit,
2026-07-22).** `run_effects` increments `reanchor_owed`'s `tries` only when it
actually sends a `ReanchorRetry` under the `Confirmed` gate — the same place the
beacon is broadcast. An instance that drops the effect because it is not the
confirmed executor leaves the budget untouched, so a late-confirming instance
still has its full `REANCHOR_MAX_RETRIES` to spend. **Required test:** a
multi-instance case where confirmation arrives after several ineligible
`apply_tabs` passes, asserting the retry budget is intact at confirmation and the
beacon is re-broadcast exactly once per remaining slot.

**7. `apply_tabs` — the prune block** (`model.rs:694-712`). Replacing:

```rust
        if !live.is_empty() {
            let mut stale: BTreeSet<usize> = self
                .agents
                .iter()
                .filter_map(|a| a.tab_id)
                .filter(|id| !live.contains(id))
                .collect();
            stale.extend(
                self.timeline
                    .keys()
                    .copied()
                    .filter(|id| !live.contains(id)),
            );
            if !stale.is_empty() {
                effects.push(Effect::PruneTabs {
                    stale_ids: stale.into_iter().collect(), // BTreeSet → sorted, deduped
                });
            }
        }
```

with (the surrounding 40-line rationale comment at `:657-693` stays; append the
S3 paragraph quoted in § 4.2):

```rust
        if !live.is_empty() {
            let mut stale: BTreeSet<usize> = self
                .agents
                .iter()
                .filter_map(|a| a.tab_id)
                .filter(|id| !live.contains(id))
                .collect();
            stale.extend(self.tab_order.keys().copied().filter(|id| !live.contains(id)));
            for id in &stale {
                // PIN the fence seq at the FIRST observation and never refresh
                // it (S3 §2.1): a retry that re-read self.seq could outrank a
                // recycled id's fresh ordinal and delete it.
                self.observed_dead.entry(*id).or_insert(self.seq);
                // Our once-ever birth-touch latch is keyed on the id, and ids
                // are RECYCLED — an observed close is the only evidence that
                // will ever arrive, so consume it here.
                self.birth_touched.remove(id);
            }
            // One effect per death COHORT: a second close before the first
            // prune landed leaves two pinned seqs, and one flag cannot carry
            // two bounds. Sorted by seq for determinism; the common case is
            // exactly one cohort and exactly one subprocess, as today.
            let mut cohorts: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
            for id in stale {
                let at = self.observed_dead[&id];
                cohorts.entry(at).or_default().push(id);
            }
            for (at_seq, stale_ids) in cohorts {
                effects.push(Effect::PruneTabs { stale_ids, at_seq });
            }
        }
```

**8. `visited` — the ack** (`model.rs:349-357`). At the top of the body, before
`self.beacon(tab_id)`:

```rust
        // #23 ack (S3 §2.2): a beacon arrived over the pipe — our own announce
        // echoes back to every instance, and ANY beacon naming a tab is a
        // fleet-wide current_tab we did not have to broadcast ourselves. Debt
        // settled; stop retrying. Deliberately NOT in beacon(): click/nav call
        // that locally, and a local call is not evidence the pipe went out.
        self.reanchor_owed = None;
```

**9. `rows` — the dormant glyph** (`model.rs:770-777`):

```rust
            } else {
                ('◌', 90) // dormant conversation
            };
```

becomes

```rust
            } else {
                DORMANT_GLYPH // dormant conversation — hollow, not idle (S3 §2.3)
            };
```

**10. `click` and `nav` — coherence-gated identity-carrying switches**
(`model.rs:805-818` and `:888-902`). Both `RowKey::Tab` arms currently do:

```rust
                let Some(position) = self
                    .tabs
                    .iter()
                    .find(|t| t.tab_id == tab_id)
                    .map(|t| t.position)
                else {
                    return Vec::new();
                };
```

Both become (a shared private helper, so the two paths cannot drift):

```rust
    /// Resolve a tab id to the switch effect, or nothing. Coherence-gated
    /// (S0's witness): a close renumbers every position after the closed
    /// index, so a position read from a frame that predates the renumbering
    /// lands on the WRONG tab (dossier RC-E, "also in the blast radius").
    /// S0 already gates the nav EXECUTOR this way; the Mouse path was not
    /// gated at all (main.rs:498-505). Dropping is safe for both: a click is
    /// repeatable and the nav bind is a keypress.
    fn switch_to_tab(&self, tab_id: usize) -> Option<Effect> {
        if !self.frames_coherent() {
            return None;
        }
        let position = self.tabs.iter().find(|t| t.tab_id == tab_id)?.position;
        Some(Effect::SwitchTab { position, tab_id })
    }
```

with each call site becoming `let Some(sw) = self.switch_to_tab(tab_id) else {
return Vec::new() };` and pushing `sw` where `Effect::SwitchTab { position }`
was. `frames_coherent()` is S0's (`S0:186-193`); it is `fn` (private) there —
this is an intra-module call, so no visibility change is needed.

### 3.2 `crates/clave-bar/src/main.rs`

**11. `run_effects` — two new/changed arms** (post-S0 `confirmed`/`presumed`
flags, `S0:606-607`):

```rust
                Effect::SwitchTab { position } => {
                    switch_tab_to(position as u32 + 1);
                }
```

becomes

```rust
                Effect::SwitchTab { position, tab_id: _ } => {
                    // 1-based, like the stock tab-bar's click handler.
                    // `tab_id` is carried but unused until S3 live step 7
                    // settles whether PluginCommand::SwitchTabToId (wire:
                    // zellij-utils data.rs:3491) is honoured by the server —
                    // zellij-tile 0.44.3 wraps no id-addressed focus action
                    // and zellij-server is not vendored, so it is unverifiable
                    // from source. Emission is coherence-gated in the model.
                    switch_tab_to(position as u32 + 1);
                }
```

new arm, immediately after `Effect::ReanchorVisit` (`main.rs:117-136`):

```rust
                Effect::ReanchorRetry { tab_id } if confirmed => {
                    // #23 retry (S3 §2.2): same beacon, STRONGER gate. Attempt
                    // 1 (ReanchorVisit, above) keeps the pre-S0 `presumed`
                    // gate so the shipped fix cannot regress; retries fire only
                    // from an instance whose frames agree AND whose own tab is
                    // active, capped at REANCHOR_MAX_RETRIES and cleared by any
                    // incoming clave-visited pipe.
                    run_command(
                        &["zellij", "pipe", "--name", "clave-visited", "--", &tab_id.to_string()],
                        BTreeMap::new(),
                    );
                }
```

and the prune arm (`main.rs:154-166`) gains the flag:

```rust
                Effect::PruneTabs { stale_ids, at_seq } if confirmed => {
                    // …existing #6/F3 rationale unchanged…
                    // S3: --at-seq is the store seq at which THIS instance
                    // first observed these ids die. The store refuses to
                    // remove an entry written after it, which is what stops a
                    // late prune erasing a RECYCLED id's fresh entry.
                    let mut argv: Vec<String> = vec![
                        "clave".into(),
                        "prune-tabs".into(),
                        "--at-seq".into(),
                        at_seq.to_string(),
                    ];
                    argv.extend(stale_ids.iter().map(usize::to_string));
                    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
                    run_command(&refs, BTreeMap::new());
                }
```

### 3.3 `crates/clave/src/main.rs` — the CLI surface

**12. `Command::PruneTabs`** (`:115-127`) gains an optional named flag. It must
be **named**, not positional, and it must be `Option`:

```rust
    #[command(hide = true)]
    PruneTabs {
        /// The store `seq` the reporting bar held when it first observed these
        /// ids die. The store refuses to remove an entry written AFTER it, so a
        /// tab id RECYCLED between the report and the flock (get_new_tab_id =
        /// max-key+1, screen.rs:1617) survives (S3 §2.1). Optional: absent =
        /// unfenced, i.e. the pre-S3 behaviour, which is the shape an OLD
        /// plugin sends. Named rather than positional on purpose — a positional
        /// would be silently read as a TAB ID by a pre-S3 binary (#44 makes
        /// mixed CLI/plugin versions a live hazard) and prune a live tab.
        #[arg(long = "at-seq")]
        at_seq: Option<u64>,
        /// The zellij tab ids observed dead (the stale set). Empty is a no-op.
        #[arg(trailing_var_arg = true)]
        stale_ids: Vec<usize>,
    },
```

**13. The handler** (`:317-327`):

```rust
        Some(Command::PruneTabs { at_seq, stale_ids }) => {
            let paths = store::store_paths()?;
            if let Some(snap) = store::apply_prune_tabs(&paths, &stale_ids, at_seq)? {
                hook::push_snapshot(&snap);
            }
            Ok(())
        }
```

### 3.4 `crates/clave/src/store.rs`

**14. `apply_prune_tabs`** (`:258-298`), in its **post-S1** form (S1 §4.2 step 8
added the ordinal carry onto `commit_ord`). The doc comment's residual paragraph
(`:274-276`, quoted in § 1.2) is **deleted and replaced**; the signature and body
become:

```rust
/// … (existing #6/F3 REMOVE-LISTED rationale unchanged) …
///
/// S3 FENCE: `at_seq` is the store seq the reporting bar held when it FIRST
/// observed these ids die. An entry whose ordinal is GREATER was written after
/// that observation, which — because ordinals are minted from this same `seq`
/// under this same flock (S1 Store::mint_ord) — can only mean the id was
/// RECYCLED onto a new tab (get_new_tab_id = max-key+1, screen.rs:1617) and is
/// now live. Refuse it. This replaces the accepted residual ("apply_bind
/// eviction is the backstop and the window is milliseconds"): the eviction only
/// helps when the bind arrives LAST, and the damage was permanent because the
/// birth touch is once-ever and sent_binds latched. `None` = unfenced, the
/// pre-S3 behaviour; only an old plugin sends that.
///
/// Ordering: with the fence, `prune-tabs` and `touch` COMMUTE — see the PR
/// dossier's written argument (S3 §4.2).
pub fn apply_prune_tabs(
    paths: &StorePaths,
    stale_ids: &[usize],
    at_seq: Option<u64>,
) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        if stale_ids.is_empty() {
            return None; // nothing observed dead
        }
        let fence = at_seq.unwrap_or(u64::MAX);
        // An id is REMOVABLE only if the store's own record of it is no newer
        // than the observation that condemned it. Absent entry ⇒ ordinal 0 ⇒
        // removable (the id was derived from a BIND, not the order map).
        let removable: Vec<usize> = stale_ids
            .iter()
            .copied()
            .filter(|id| s.tab_order.get(id).copied().unwrap_or(0) <= fence)
            .collect();
        if removable.is_empty() {
            return None; // every listed id was recycled; §5: no no-op pushes
        }
        let mut changed = false;
        // S1 carry, S3-fenced: the row inherits its tab's ordinal before the
        // entry dies (R2 — a close moves nothing). Skipped for a fenced id,
        // so a live tenant's ordinal is never carried onto a stranger's row.
        let carries: Vec<(String, u64)> = s
            .agents
            .values()
            .filter_map(|r| {
                let id = r.tab_id.filter(|id| removable.contains(id))?;
                Some((r.uuid.clone(), s.tab_order.get(&id).copied().unwrap_or(0)))
            })
            .collect();
        for (uuid, carried) in carries {
            if let Some(r) = s.agents.get_mut(&uuid) {
                r.commit_ord = r.commit_ord.max(carried);
                r.tab_id = None;
                changed = true;
            }
        }
        let before = s.tab_order.len();
        s.tab_order.retain(|id, _| !removable.contains(id));
        changed |= s.tab_order.len() != before;
        if !changed {
            return None; // §5: no no-op pushes
        }
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}
```

*Borrow note (same hazard S1 flagged):* the carry vector is collected before the
`values_mut()` pass because `s.agents.values_mut()` and `s.tab_order` cannot both
be borrowed in one loop.

**15. `apply_bind` — record the eviction is now the ONLY unbind that races**
(no code change; S0 §3 already adds the `bind-evict` `log_event`). Add one line
to its doc block (`:222-226`) pointing at the fence, so the next reader sees the
two mechanisms as a pair rather than as duplicates.

### 3.5 Documentation

**16.** `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md:506` —
the dormant-row line (*"◌ glyph, dimmed"*) is updated to the new glyph and gains
the one-sentence reason (dim-filled is `Status::Idle`).

**17.** Same design doc, §6.5: add the **"why a row went dim"** table (I1–I4 of
§ 1.5) as the user-facing legend. This is the documentation half of the C3 fix;
it is what makes the maintainer's "sometimes going idle" answerable without a
store dump.

**18.** Issue **#24** is answered with § 2.3's ruling: the dormant row stays (it
is the resurrection list), the glyph changes, and the behaviour does not.

**19.** S1's `Store::mint_ord` doc comment says *"NOTHING ever compares an
ordinal to a snapshot `seq`"* (`S1:402`). S3 introduces **exactly one** such
comparison and must amend that sentence rather than silently violate it:

> …the rule that keeps the two roles from being conflated: nothing compares an
> ordinal to a snapshot `seq` **in order to order rows**. There is exactly one
> sanctioned cross-comparison, at one site — `apply_prune_tabs`'s `at_seq` fence
> (S3 §2.1) — and it is not an ordering question but a happens-before one:
> *was this entry written after the state the reporter observed?* It is sound
> precisely **because** the two draw on one counter under one flock.

---

## 4. Test plan

**Change classes**, per the taxonomy (`TESTING.md:112-120`):

| Class | What in S3 | What the PR must show |
|---|---|---|
| **Pure logic / model** | everything in § 3.1 | TDD red-first; `cargo test --workspace`; new proptests for the new branches |
| **CLI surface** | `--at-seq` on `clave prune-tabs` (§ 3.3) | `Cli::try_parse_from` pin **plus** one sandboxed end-to-end run in a **debug** build |
| **Cross-process / IPC** | the fence itself, the prune/touch/bind interleaving, mixed plugin/CLI versions | **written ordering/idempotency argument in the dossier (§ 4.2, paste verbatim) + an adversarial reviewer briefed to attack it** |
| **Visual / UX** | the dormant glyph (§ 3.1 item 9) | human judgement only ⇒ `host-untestable`; live step 6 |

Labels: **`needs-live-validation`** (the close race is only observable against a
real fleet) and **`host-untestable`** on the glyph decision.

### 4.1 What must be testable, and what cannot be

`crates/clave-bar/src/main.rs` is `test = false`, so the two adapter changes
(§ 3.2) are unguarded by construction — the gate flag each new arm reads, and
the argv assembly for `--at-seq`. Mitigation is the parse pin (which proves the
argv *shape* the plugin sends actually parses) plus adversarial review aimed
specifically at those two arms. Everything else lives in `model.rs`/`store.rs`.

### 4.2 The written ordering / idempotency argument (paste into the PR verbatim)

**Claim 1 — the fence never refuses a genuinely dead entry.**
The bar lists id `X` as stale only because `X` appears in its mirror, and the
mirror is a snapshot at `seq == S`. Every ordinal in a snapshot at seq `S` was
minted by a locked write with `seq ≤ S` (S1 `mint_ord`; asserted by
`prop_ordinals_are_a_total_order`). So the condemned entry satisfies
`ord ≤ S = at_seq` and is always removed. **The fence cannot cause a leak.**

**Claim 2 — the fence always refuses a recycled entry.**
A recycled id's entry is written by the new tab's birth touch, which happens
after the bar observed the death of the old tab. `seq` increases monotonically
under an exclusive flock (`store.rs:135-164`), so that write's ordinal is
`> S`. **The fence cannot fail to protect a live tenant's order entry.**

**Claim 3 — pinning is load-bearing.** Emission is detection-driven and repeats
every `TabUpdate` until the store echo clears the mirror (`model.rs:673-693`). A
retry that re-read `self.seq` would, once the bar had accepted the snapshot
containing the recycled tab's fresh ordinal, carry a bound `≥` that ordinal and
delete it. `observed_dead` pins `S` at the first observation; every retry re-uses
it, so retries are **monotonically no more permissive** than the first attempt.

**Claim 4 — `prune-tabs` and `touch` now COMMUTE.** Two orders, one outcome:
- *touch → prune*: the fresh ordinal is `> at_seq`, the prune skips the id; final
  state = fresh ordinal present.
- *prune → touch*: the prune removes the dead entry, the touch mints a fresh one;
  final state = fresh ordinal present.
This is the property the accepted residual at `store.rs:274-276` lacked.

**Claim 5 — prunes still commute with each other and remain idempotent.**
Removals are still remove-listed and set-shaped. Two fenced prunes for the same
id with bounds `S1 < S2` against an entry at `ord`: if `ord ≤ S1` both remove
(second is a no-op, no seq bump, no push — the existing change gate); if
`S1 < ord ≤ S2`, only the `S2` one removes, in either arrival order; if
`ord > S2`, neither does. In all three the final state is order-independent.

**Claim 6 — the residual, stated.** The **bind** leg is fenced by the same
`tab_order`-derived witness, which does not move when a bind is written. So the
window *bind lands → prune lands → birth touch lands* can still unbind a fresh
tenant. It is strictly narrower than today (the touch is emitted **before** the
bind by `identity_effects`, S0 §1.5, so the fence usually already holds), and it
is **self-healing** under S0: the prune's own push advances `seq`, which is
exactly the condition S0's bind ledger waits on, so `bind_effects` re-emits
within one round trip (`S0:536-548`) and the tenant is re-bound. The order key,
the half that did **not** self-heal, is protected unconditionally. Closing this
last window needs a per-record `bind_ord` (§ 2.1's deferred alternative); it is
filed, not shipped.

**Claim 7 — mixed versions cannot corrupt.** `--at-seq` is a *named optional*
flag, so: new plugin + **old CLI** ⇒ clap rejects the unknown flag ⇒ the prune
fails and entries linger (degrades to pre-#6 hygiene, no wrong write); old
plugin + **new CLI** ⇒ `None` ⇒ unfenced ⇒ today's exact behaviour. A
*positional* seq would have been read by a pre-S3 binary as a **tab id** and
pruned a live tab — which is why it is a flag. Given #44 is unfixed, mixed
versions are a live hazard, not a hypothetical (`TESTING.md:159`).

**Claim 8 — the beacon retry is bounded.** ≤3 emissions per close episode
(1 `Presumed` + 2 `Confirmed`), cleared by the first incoming `clave-visited`
pipe of any kind, and only ever armed by an observed close. Retries require
`Confirmed`, which at most one instance satisfies. Contrast round 13: uncapped,
per-instance, echo-congestion-driven (`SUBSYSTEM-VALIDATION.md:232-243`).

**Claim 9 — the birth-touch re-arm cannot storm.** `birth_touched.remove` runs
only for ids the instance observed die; `needs_birth_touch` is reached only from
`identity_effects` under `Confirmed` **and** `active == own` (S0 §1.5), and the
latch re-closes on the first fire. So a re-arm can produce **at most one** extra
`clave touch` per instance per observed close, and only if that dead id is later
recycled into this instance's own active tab.

### 4.3 Tier 1 — new tests

**`crates/clave-bar/src/model.rs`** (TDD, red first, existing long-sentence
naming convention):

| Test | Asserts |
|---|---|
| `prune_carries_the_seq_observed_when_the_close_was_first_seen` | after `apply_snapshot(seq=7)` then a close, the emitted effect is `PruneTabs { stale_ids: vec![11], at_seq: 7 }` |
| `prune_retries_reuse_the_pinned_seq_even_after_the_store_moves_on` | the retry leg of `tab_close_prunes_stale_ids_…` with an intervening `apply_snapshot(seq=9)` that still shows the stale bind: the re-emitted effect **still** carries `at_seq: 7`. This is Claim 3, and it is the test the naive implementation fails |
| `two_closes_at_different_seqs_emit_two_fenced_cohorts` | ids condemned at seq 7 and seq 9 produce two `PruneTabs`, seq-ascending, each with its own ids |
| `a_recycled_tab_id_is_birth_touched_again_after_an_observed_close` | tabs {10,11}; snapshot with `tab_order[11]`; close 11; new `TabUpdate` reintroduces 11 → `needs_birth_touch(11)` is **true**. Fails today (deterministically) |
| `a_snapshot_dropping_a_tab_still_does_not_re_arm_the_birth_touch` | the second-hand case stays closed — the discipline that keeps `model.rs:1272` valid |
| `observed_dead_is_forgotten_once_the_store_has_nothing_left_to_prune` | after a snapshot with neither the entry nor the bind, the map is empty (bound growth) |
| `reanchor_retries_are_capped_and_cleared_by_any_beacon` | close → `ReanchorVisit`; two further `TabUpdate`s → two `ReanchorRetry`; a third → none; then a fresh episode after `visited()` re-arms at full budget |
| `reanchor_debt_clears_when_the_target_tab_dies` | the target closes before the ack: no orphan retries |
| `switch_effects_carry_the_tab_id_and_are_dropped_on_an_incoherent_frame` | `click` and `nav` both: coherent ⇒ `SwitchTab { position, tab_id }`; the dossier's incoherent frame (`ux-defect-dossier.md:91-99`) ⇒ `[]` |
| `dormant_rows_are_not_a_dim_status_dot` | the glyph is `DORMANT_GLYPH` and `DORMANT_GLYPH.0 != Status::Idle.glyph().0` — the property, not the codepoint, so a later palette change cannot silently re-collide |

**`crates/clave/src/store.rs`:**

| Test | Asserts |
|---|---|
| `prune_refuses_an_entry_written_after_the_reported_observation` | the C1 trace end to end: touch(11) at ord 3, prune `[11]` with `at_seq=2` ⇒ `None`, entry intact, bind intact |
| `prune_removes_an_entry_written_before_the_observation` | the ordinary close: ord 3, `at_seq=5` ⇒ removed, `commit_ord` carried |
| `prune_without_a_fence_behaves_exactly_as_before` | `at_seq: None` (old plugin) ⇒ pre-S3 semantics |
| `prune_fences_per_id_within_one_payload` | `[11, 12]` where 11 is recycled and 12 is dead ⇒ 12 removed, 11 untouched, one push |
| `fenced_prune_and_touch_commute` | both orders from one fixture ⇒ identical final store (Claim 4) |
| `two_fenced_prunes_commute_and_are_idempotent` | Claim 5's three cases, both arrival orders |

**`crates/clave/src/main.rs`** — parse pins (`TESTING.md:116`, and the
`ArgAction` escape is why this row exists):

| Test | Asserts |
|---|---|
| `prune_tabs_parses_the_argv_the_plugin_actually_sends` | `["clave","prune-tabs","--at-seq","7","11","12"]` ⇒ `at_seq: Some(7)`, `stale_ids: [11,12]` — proves `--at-seq` before a `trailing_var_arg` positional parses |
| `prune_tabs_still_parses_without_a_fence` | `["clave","prune-tabs","11"]` ⇒ `at_seq: None` (the old-plugin shape) |

Plus one sandboxed debug e2e (`TESTING.md:31`):
`CLAVE_STATE_DIR=<scratch> cargo run -p clave -- prune-tabs --at-seq 1 5`.

### 4.4 Tier 1 — proptests

New branches ⇒ new properties (`TESTING.md:121-126`).

| Proptest | Crate | Property |
|---|---|---|
| `prop_fenced_prune_never_removes_an_entry_written_after_its_bound` | `clave` | over arbitrary `(tab_order, stale_ids, at_seq)`: every surviving entry either was not listed or has `ord > at_seq`; every removed entry has `ord ≤ at_seq` |
| `prop_touch_and_prune_commute_under_the_fence` | `clave` | arbitrary interleavings of `touch_in`/`prune_in` (S1 §5.3 already factors the pure halves out of the locked closures) reach the same store regardless of order |
| `prop_a_recycled_id_always_regains_a_stamp` | `clave-bar` | over generated close/create sequences that recycle ids: every live tab that has been active on the instance ends with either a `tab_order` entry or a pending `Effect::Touch` — i.e. no live tab is permanently unstamped |
| `prop_reanchor_emissions_are_bounded` | `clave-bar` | over arbitrary `TabUpdate`/`visited` interleavings: emissions per stranding episode ≤ `1 + REANCHOR_MAX_RETRIES` |

### 4.5 Existing tests: which change, and why

| Test | file:line | Change | Why |
|---|---|---|---|
| `tab_close_prunes_stale_ids_and_retries_until_echo_clears` | `model.rs:1531` | **mechanical + one new leg** — expected effects gain `at_seq`; the retry leg gains an intervening higher-seq snapshot asserting the pinned bound is **not** refreshed | the contract it pins (detection-driven retry, self-limiting on echo) is unchanged and must stay; the new leg is Claim 3 |
| `birth_and_steady_state_never_prune` | `model.rs:1581` | **mechanical** — `matches!(e, Effect::PruneTabs { .. })` already ignores fields | silence semantics unchanged |
| `tab_close_reanchors_the_stranded_beacon` | `model.rs:1483` | **behavioural, intentional** — its final leg asserts *"re-anchor must not re-fire once the beacon is live again"*. Under S3 the second `TabUpdate` **does** emit `ReanchorRetry`. Amended to: attempt 1 is `ReanchorVisit`, attempts 2–3 are `ReanchorRetry`, attempt 4 is silence, and a `visited()` between any two of them silences immediately | that assertion pinned the *bug* — the one-shot emission is exactly why a dropped re-anchor left nav dead (§ 1.4). The storm property it was protecting is preserved by the cap and the ack, and is asserted explicitly |
| `birth_touch_fires_once_ever_and_defers_to_snapshot_knowledge` | `model.rs:1272` | **unchanged, byte for byte** — including *"Replace semantics dropping a tab from the map must NOT re-arm an already-fired guard"* | the re-arm is scoped to a **first-hand observed close**, never to a snapshot drop. If this test needs editing, the implementation is wrong |
| `dormant_rows_sort_into_the_unified_recency_order` | `model.rs:2112` (assertion at `:2108`) | **mechanical** — `assert_eq!(d.glyph, Some(('◌', 90)))` → `Some(DORMANT_GLYPH)` | glyph const; its ordering assertions are S1's and stay |
| `bind_effects_report_own_tab_join_once_and_echo_independently` | `model.rs:1389` | **unchanged** | S0 already re-verified it; S3 touches no bind path |
| `prune_tabs_removes_listed_stale_ids_order_safe_and_change_gated` | `store.rs:516` | **extended** — call sites gain the `at_seq` argument (S1 already extends it for the carry); add the fenced-refusal and per-id-fencing legs | the function gains a parameter and a branch; its existing order-safety assertions are exactly what Claim 5 generalises |
| `bind_evicts_a_reused_tab_id_from_the_previous_agent` | `store.rs:555` | **unchanged** | eviction stays the backstop for the bind leg (Claim 6) |
| `touch_stamps_timeline_bumps_seq_and_never_regresses` / `clear_session_state_wipes_timeline_and_binds` / `clear_tab_timeline_wipes_session_scoped_ids` | `store.rs:609`, `:591`, `:655` | **already rewritten by S1**; S3 rebases only | S1 §5.2 owns them |

### 4.6 Gate

```bash
cargo test --workspace   # --workspace is load-bearing (TESTING.md:36)
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

No generated-artifact change ⇒ the KDL guardrail and the version-pin tripwire
are untouched.

### 4.7 Tier 2 / Tier 3

Tier 2 does not exist (#47). When it does, S3 contributes two first scenarios
alongside the existing five (`TESTING.md:68-70`): **close the highest-id tab and
immediately create one** (assert the new tab is stamped and bound), and **close a
tab then nav** (assert `Alt+j` moves focus without a click). Everything else is
Tier 3, below.

---

## 5. Live validation

**Contract.** Every step is the **maintainer's** to execute. The driving agent
prints commands and reads what he pastes back; it never runs `zellij` against his
session — a `zellij action` aimed at a dead session **blocks forever without
erroring** (`TESTING.md:231-236`). The one sanctioned agent-side mutation is the
sandbox hot-reload in step 5, env-scoped to `clave-test`. All diagnostics below
are **read-only**: `read_store` is lock-free-safe (writers use temp+atomic-rename,
`store.rs:120-129`). Paths are genericised (`$HOME`, `$TMPDIR`) because the
pre-commit PII blocklist rejects private local paths and has fired twice
(`AGENTS.md:122-124`).

Field names below are **post-S1** (`tab_order`, `commit_ord`). If S1 has not
merged, substitute `tab_timeline` and skip step 3's ordinal comparison — and stop,
because S3 does not function without S1 (§ 6).

### Phase 0 — pre-flight

> **Step 1 — binary/bar version coherence (issue #44 is unfixed).**
>
> **(a) He runs**, inside the clave session:
> ```bash
> command -v clave; clave --version
> grep 'clave-bar: loaded' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -3
> ```
> **(b) He looks at** the path `clave` resolves to, its version, and the
> `v<X.Y.Z> build=<tag>` on the last loaded-bar lines.
> **(c) He reports** all lines verbatim.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | versions match, one `loaded v…` line per tab | coherent; readings are trustworthy | step 2 |
> | `clave --version` ≠ the `loaded v…` version | **#44/#43** — the bar shells out to a different binary; **every** reading below is suspect (`ux-defect-dossier.md:534`) | **stop.** Report that S3 cannot be validated until #44 lands or he reinstalls |
> | two different `loaded v…` versions in the tail | two plugin populations (the v0.1.1 incident, `TESTING.md:159`) | **stop**, same as above |
> | `command -v clave` under `.cargo/bin` while a release install exists | the PATH leak (CONTRIBUTING "The one leak") | **stop**, report the path |

### Phase 1 — baseline, before the fix

> **Step 2 — the store and the server, side by side.**
>
> **(a) He runs:**
> ```bash
> clave ls --json | jq '{seq, tab_order,
>   agents: [.agents[] | {uuid, label, status, tab_id, commit_ord, last_interacted}]}'
> zellij action list-panes -t
> ```
> **(b) He looks at** the **highest TAB_ID** in the second output, and whether
> every agent's store `tab_id` matches the tab its pane is actually in.
> **(c) He reports** both outputs whole, and names the highest tab id.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | joins agree | clean baseline | step 3 |
> | agent A's `tab_id` names a tab holding agent B's pane | **RC-A / S0**, not S3 (`ux-defect-dossier.md:527`) | record; S3 cannot be judged over an S0 defect — if S0 has merged, **stop and report** |
> | an agent has `tab_id: null` but its pane is listed | S0's eviction victim, or the C1 race already fired | record; continue — step 3 discriminates |

> **Step 3 — the recycled-id repro (C1 + C1b). This is the headline step.**
>
> **(a) He does**, with at least three tabs open:
> focus the tab whose **TAB_ID is the highest** (from step 2), press **`Alt+w`**,
> then **immediately** press **`Alt+t`** (a plain new tab — no claude involved, so
> only the touch path is exercised).
> **(b) He looks at** where the new tab's row appears in the sidebar: top, or
> bottom (below the dim rows)?
> **(c) He runs and reports:**
> ```bash
> clave ls --json | jq '{seq, tab_order}'
> zellij action list-panes -t
> ```
>
> | If he reports | That means | Do next |
> |---|---|---|
> | the new tab is at the **bottom**, and its TAB_ID (= the closed one) has **no** `tab_order` entry | **C1b confirmed** (birth touch spent on the recycled id) and/or **C1** (the prune erased a fresh entry). Both are S3's; step 4 tells them apart | record as the pre-fix baseline; step 4 |
> | the new tab is at the **top** with a `tab_order` entry | neither fired this time — the ids may not have recycled | re-check that the closed tab really had the highest id; repeat up to 3× |
> | the new tab is at the top but an *unrelated* row moved | **RC-C / S1** demotion, not S3 (`ux-defect-dossier.md:196-212`) | note and continue |
>
> **Then repeat once with `Alt+a`** (an agent tab: exercises touch **and** bind).
> If the new agent shows a dim row **and** `tab_id: null`, that is the full C1
> trace including the unbind.

> **Step 4 — separate the race from the spent latch.**
>
> **(a) He does:** step 3 again, but **waits ~3 seconds** between `Alt+w` and
> `Alt+t` (long enough for the prune subprocess to have taken the flock).
> **(b) He looks at** the same thing: top or bottom.
> **(c) He reports** the position and the `tab_order` JSON.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | still at the bottom, still no `tab_order` entry | **C1b** — deterministic, no race involved (§ 1.3). The single most valuable pre-fix datapoint | record verbatim; it is the assertion for step 8 |
> | at the top with the pause, at the bottom without | **C1** — genuinely the subprocess race | record; both are fixed by the same change |
> | at the top both ways | neither reproduced on this instance; the Tier-1 tests carry the proof | note "not reproduced"; continue |

> **Step 5 — the re-anchor repro (C2).**
>
> **(a) He does:** with ≥3 tabs, focus a **non-last** tab, press **`Alt+w`**, then
> **immediately** press **`Alt+j`** and **`Alt+k`** (do not click, do not touch the
> mouse).
> **(b) He looks at** whether focus moves at all.
> **(c) He reports** whether the first `Alt+j` moved focus, whether a second
> press did, and whether a **mouse click on any row** then restores nav.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | nav dead until a click | **C2 reproduced** — the dropped `ReanchorVisit`; the beacon is stranded (`model.rs:603`) | record as baseline; this is what step 9 must eliminate |
> | first press dead, second press works | the re-anchor landed late | record — post-fix, the **first** press must work |
> | nav fine every time | not reproduced this run; it depends on `PaneUpdate` lag | repeat 3×; if still clean, record "not reproduced" |

> **Step 6 — which of the four "went idle" causes is it? (C3)**
>
> **(a) He runs**, immediately after any close that made a row go dim:
> ```bash
> clave ls --json | jq '[.agents[] | {label, status, tab_id, last_visited}]'
> zellij action list-panes -t
> grep '"cmd":"bind-evict"' "$HOME/.local/state/clave/clave.log" | tail -5
> ```
> **(b) He looks at** the dim row's `status` and `tab_id`, whether its pane is
> still listed, and whether any evict line names it.
> **(c) He reports** all three outputs plus which row went dim.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | `status: "idle"`, no pane listed, `tab_id: null` | **I1** — the pane died and `SessionEnd` fired. Correct behaviour | nothing to fix; note it |
> | `status: "idle"`, pane **still listed**, `tab_id` set, `last_visited` ≈ the close time | **I2** — collateral `MarkRead` on the survivor the close focused. Correct behaviour, genuinely surprising | this is what § 3.5 item 17 documents |
> | `tab_id: null`, **no** pane, and this is the tab he just closed | **I3** — the by-design dormant row (#24). Not idle at all | this is what the glyph change addresses |
> | `tab_id: null` but the pane **is** listed, or a `bind-evict` line names it | **I4 — RC-A / S0** | **stop** and report; an S0 regression outranks everything here |
>
> **(d) The glyph A/B (Tier 3, `host-untestable`).** With the sandbox build from
> step 7 loaded, the agent rebuilds once per candidate and he picks:
> **A** = `('○', 90)` (spec default — hollow, still dim);
> **B** = `('◌', 90)` (today — dotted ring, still dim);
> **C** = `('○', 36)` (hollow, cyan — leaves the status palette entirely).
> He reports which one he can tell apart from a dim `●` **at a glance, without
> leaning in**. His answer is final and the const is the only line that changes.

### Phase 2 — the fix, in the sandbox

> **Step 7 — build and hot-reload the sandbox bar.**
>
> **(a) The agent runs** (the one sanctioned live mutation, env-scoped to
> `clave-test`, never his session):
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
> Never `just dev-install`, `cargo install` or `just release` — assume he is
> daily-driving (`AGENTS.md:46`). Note the confound: a hot-reload reincarnates
> every bar model from scratch (`TESTING.md:335-338`), which **clears
> `birth_touched`** — so C1b cannot be reproduced across a reload. Reproduce it
> only within one continuous plugin lifetime.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | session up, 3 rows | ready | step 8 |
> | `clave dev launch` hangs | stale/exited `clave-test` | ask him to run the kill line `clave dev reset` printed, then retry once |
> | two bars per tab | wrong wasm — the #44 class | **stop**; re-verify step 1 against `clave-test` |

> **Step 8 — the recycled id, fixed.**
>
> **(a) He does:** in `clave-test`, `Alt+t` twice, then focus the **highest-id**
> tab, `Alt+w`, then immediately `Alt+t`. Repeat 5×.
> **(b) He looks at** where each new tab lands.
> **(c) He reports:**
> ```bash
> clave dev status | jq '{tab_order: .store.tab_order, seq: .store.seq,
>   agents: [.store.agents[] | {uuid, tab_id, commit_ord}]}'
> ZELLIJ_SESSION_NAME=clave-test zellij action list-panes -t
> ```
>
> | If he reports | That means | Do next |
> |---|---|---|
> | every new tab enters at row 0; each live TAB_ID has a `tab_order` entry; ordinals strictly increase | **C1 + C1b fixed** | step 9 |
> | a new tab at the bottom, **no** `tab_order` entry for its id | the birth-touch re-arm did not fire — check whether that tab was ever *active* on that instance (`identity_effects` needs `active == own`, S0 §1.5) | report which tab was focused; investigate before proceeding |
> | entry present but ordinal **lower** than a neighbour's | the fence let a stale prune through and the touch re-minted out of order, or S1's mint is not monotonic | **stop**; report `tab_order` and `seq` — this contradicts Claim 2 |
> | agent tab created via `Alt+a` shows `tab_id: null` for more than ~1 s | the bind leg's residual (Claim 6) is not healing | report; this is the trigger for the deferred `bind_ord` follow-up |

> **Step 9 — nav after a close, fixed.**
>
> **(a) He does:** focus a non-last tab, `Alt+w`, then **immediately** `Alt+j`.
> Repeat 5×, never touching the mouse.
> **(b) He looks at** whether the **first** press moves focus every time.
> **(c) He reports** the count of first-press successes out of 5, and whether he
> ever needed a click.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | 5/5 first-press, no clicks needed | **C2 fixed** | step 10 |
> | first press dead, second works, no click needed | the re-anchor is landing on the **retry** — working as designed but one beat late | acceptable; report so the cap can be reconsidered |
> | still needs a click | the retry is not firing: either the debt is being cleared too early, or `Confirmed` is never true after a close | **stop**; ask for `ZELLIJ_SESSION_NAME=clave-test zellij action list-panes -t` and the tab count |
> | focus jumps to the **wrong** tab | C4, not C2 — the positional switch | record; it is the step-11 decision |

> **Step 10 — the anti-storm assertion (the thing that must not regress).**
>
> **(a) He runs**, right after step 8's and step 9's ten closes:
> ```bash
> wc -l < "$HOME/.local/state/clave-dev/state/clave.log"
> ```
> then leaves the session **completely idle for 60 seconds** and runs it again.
> **(b) He looks at** whether the count moved while idle, and whether zellij
> feels responsive.
> **(c) He reports** both counts and any lag.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | identical counts | quiescence costs nothing — the caps and the pin hold | step 11 |
> | the count grows while idle | a prune, touch or beacon loop is running | **stop immediately.** This is the round-4/round-13 fd-exhaustion class (`SUBSYSTEM-VALIDATION.md:232-243`). Ask him to `clave dev reset` and report the growing lines |
> | zellij feels laggy | possible spawn pressure | **stop**, same as above |

> **Step 11 — the `SwitchTabToId` decision (C4).**
>
> Only if steps 8–10 are green. The agent rebuilds the sandbox wasm with
> `main.rs`'s `Effect::SwitchTab` arm issuing
> `PluginCommand::SwitchTabToId(tab_id as u64)` through a locally-declared
> `#[link(wasm_import_module = "zellij")] extern "C" { fn host_run_plugin_command(); }`
> (see § 2.4), hot-reloads, and asks:
>
> **(a) He does:** `Alt+1`, `Alt+2`, `Alt+3`, then `Alt+j`/`Alt+k` around the
> list, then a mouse click on a row.
> **(b) He looks at** whether focus moves **at all**, and whether it lands on the
> row he aimed at.
> **(c) He reports** which of the four interactions moved focus.
>
> | If he reports | That means | Do next |
> |---|---|---|
> | all four move focus, always to the aimed row | the server honours `SwitchTabToId` | **adopt**: the adapter keeps the id call, the `position` field is deleted in a follow-up PR, and C4 is closed by identity rather than by a gate |
> | nothing moves focus | the command is unhandled server-side (unvendored, unverifiable from source) | **revert the arm** to `switch_tab_to(position + 1)`; the coherence gate (§ 3.1 item 10) is the whole C4 fix. Record the finding in `SUBSYSTEM-VALIDATION.md` — it is exactly the class of lore that document exists for |
> | some move, some do not | partial handling | revert as above and report which |

### Phase 3 — his real fleet

> **Step 12 — re-run Phase 0 and Phase 1 against the stable session.**
>
> Only after **he** decides to install a tagged build — the agent never runs
> `just release` (`AGENTS.md:45`). Repeat step 1, then steps 3, 4, 5 and 6
> verbatim and diff against the Phase-1 baseline.
>
> | If the diff shows | That means | Do next |
> |---|---|---|
> | Phase 1 put the new tab at the bottom, Phase 3 puts it at the top, ≥5 trials | **C1/C1b validated on the real fleet** | record in the PR dossier; drop `needs-live-validation` |
> | Phase 1 needed a click for nav, Phase 3 does not, ≥5 trials | **C2 validated** | as above |
> | he can now tell a dormant row from an idle one at a glance | **C3 validated** (Tier 3, his call, final) | as above |
> | a *new* symptom: a click or `Alt+N` occasionally does nothing right after a close | expected and by design — the switch is coherence-gated (§ 3.1 item 10); a repeat press must work | confirm a repeat always works; if it does **not**, the witness is stuck false — ask for `zellij action list-panes -t` and the tab count |
> | tab hygiene regresses: `tab_order` grows entries for tabs that no longer exist | the fence is refusing prunes it should allow — Claim 1 violated | **stop**; collect `clave ls --json` plus `zellij action list-panes -t` and reopen |

---

## 6. Risks, sequencing and out-of-scope

### 6.1 Interfaces assumed

**From S0** (hard; S3 does not build without it):

1. `Election::Confirmed` / `Election::Presumed` exist as model predicates
   (`elects_confirmed()`, `elects_presumed()`), and `run_effects` carries both
   flags (`S0:606-607`). S3 adds one `confirmed` arm (`ReanchorRetry`) and one
   `presumed` arm is left exactly as it was (`ReanchorVisit`).
2. `frames_coherent()` exists on `BarModel` (`S0:186-193`). § 3.1 item 10 calls
   it directly. If S0 lands the witness under another name, that is the only
   rename S3 needs.
3. `PruneTabs` is `Confirmed`-gated (`S0:227`). S3's fence is **independent** of
   that gate — it also closes the "a frozen instance lists a live tab as stale"
   hole for the `tab_order` leg on its own — but the two together are what make
   the prune safe in both directions.
4. `identity_effects()` emits `Effect::Touch` and is called from every
   external-input arm (`S0:559-575`). C1b's re-arm needs a *trigger*, and that is
   it. **Fallback if S0 lands differently:** apply the re-arm at the surviving
   inline birth-touch site (`main.rs:434-445`); the model-side change is
   unaffected.
5. The bind ledger re-emits once `seq` advances (`S0:536-548`). Claim 6's
   self-healing depends on it. Without it the bind leg's residual is permanent
   again and the deferred `bind_ord` (§ 2.1) becomes required, not optional.

**From S1** (hard; the fence is *unimplementable* without it):

6. `tab_timeline` → `tab_order`, values are **ordinals** minted by
   `Store::mint_ord()` from the shared `seq`, strictly increasing and always
   `≤ store.seq` (`S1:158-165`, `:391-407`).
   **If S1 lands with wall-clock values in that map, the fence refuses every
   prune** (unix seconds ≈ 1.7 × 10⁹ ≫ any `seq`) and tab hygiene stops
   entirely. The mechanical guard is S1's own
   `prop_ordinals_are_a_total_order` (`S1:867`) plus S3's
   `prune_removes_an_entry_written_before_the_observation`, which fails loudly if
   a clock leaks in. **S3 must not merge before that proptest is green.**
7. `apply_prune_tabs` carries the tab's ordinal onto `agent.commit_ord` before
   deleting (`S1:467-487`). S3 wraps that carry in the fence: a fenced id
   carries nothing and unbinds nothing.
8. `dormant_ord` reads `tab_order[a.tab_id]` for the render-side carry
   (`S1:750-757`). **S3 therefore must not locally drop mirror entries** for
   observed-dead ids — an early local delete would make the dormant row plunge,
   re-creating precisely the symptom S1 fixed. `observed_dead` records the death
   *beside* the mirror instead of mutating it, and that is why.
9. S1's rule *"nothing ever compares an ordinal to a snapshot `seq`"*
   (`S1:402`) is **amended by S3** at exactly one site (§ 3.5 item 19). If a
   reviewer objects to the amendment, the alternative is a second per-entry
   write-seq map, which is the duplicated-counter hazard S1 rejected — the same
   argument, one level down.

### 6.2 Risks taken

| Risk | Severity | Mitigation |
|---|---|---|
| **A fenced prune refuses a removal it should allow**, leaving a stale entry | medium — tab hygiene degrades and a reused id could inherit a dead glyph | Claim 1 proves it cannot happen for an entry the reporter saw; the only path is a wall-clock value in `tab_order` (risk 6 above), which the proptest catches at Tier 1. Live step 12's last row watches for it |
| **The bind leg is still racy** (Claim 6) | low | strictly narrower than today, self-heals through S0's ledger, and the deferred `bind_ord` is specified and filed |
| **Cohort splitting emits more subprocesses** | low | bounded by the number of *distinct pinned seqs* in the mirror; one in the common case. Live step 10 asserts quiescence |
| **The re-anchor retries add pipes** | low | ≤3 per episode, `Confirmed`-gated, ack-cleared. Live step 10 |
| **The birth-touch re-arm re-opens the C5-rd-4 storm** | medium if wrong | Claim 9; the re-arm requires a first-hand observed close, and the latch re-closes on the first fire. `birth_touch_fires_once_ever_…` (`model.rs:1272`) must pass unchanged, which is the mechanical guard that the re-arm did not become snapshot-driven |
| **The coherence gate drops a click** | low | a click is repeatable; the alternative is landing on the wrong tab. Watched by live step 12 |
| **The glyph change lands wrong for the maintainer's font/eyes** | low | Tier 3 by construction — step 6(d) is his decision and the const is one line |
| **`--at-seq` before a `trailing_var_arg` positional fails to parse** | high if unnoticed (the prune would break entirely) | the `ArgAction` escape is exactly this class (`TESTING.md:161`); two parse pins plus a sandboxed **debug** e2e (clap's `debug_assert` fires only there) |
| **`main.rs`'s two new arms are untestable** | medium | `test = false`; adversarial review pointed specifically at the argv assembly and the gate flags |

### 6.3 Sequencing

**S0 → S1 → S3**, as the dossier sequences it (`ux-defect-dossier.md:561-570`).
S1 and S3 both edit `model.rs`'s `apply_tabs` and `store.rs`'s
`apply_prune_tabs`, so they are **not parallel**; S3 rebases onto S1's form and
every quoted block above is the post-S1 text. S3 is parallel with S2, S4 and S5
(different files) — except that **S5 must not recolour the dormant glyph back
into the status palette**; § 2.3's const is the contract between them.

### 6.4 Out of scope

- **RC-A, RC-B** (frame coherence, the eager tab's bind) — S0.
- **RC-C** (ties, the demotion carry, the ordering key) — S1. S3 changes no
  comparator, no key and no tiebreak.
- **RC-D, RC-F, RC-G** — S2, S4, S5.
- **The lost `clave-register` pipe** (`spawn.rs:55-93`) — S0 declared it out of
  scope and filed it; it is not a close-path defect.
- **`MarkRead` / `RenameTab` emit-time latches** — the same "latches at emit,
  drops silently" shape as C2, but neither is close-triggered in a way the user
  reported. S3 fixes the one that is. The general fix belongs with whoever owns
  §6.5/#23.
- **Per-record `bind_ord`** — specified as the deferred close of Claim 6's
  residual; file it, do not ship it here.
- **Adopting `SwitchTabToId`** — decided by live step 11, delivered (if at all)
  as a follow-up PR whose entire diff is one `main.rs` arm.
