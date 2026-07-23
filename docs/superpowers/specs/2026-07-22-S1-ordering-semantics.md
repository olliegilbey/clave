# S1 — prompt→top ordering semantics (RC-C, closes #39)

_2026-07-22 · implementation spec · main `50fa26a` · root cause **RC-C** of
`docs/superpowers/specs/2026-07-22-ux-defect-dossier.md`_

Read the dossier first — every mechanic it establishes is taken as given here.
This spec fixes the two pure-sort-maths defects under RC-C and turns the
maintainer's ruling into documented, tested semantics.

## The ruling this implements (binding)

> *"only typing a prompt to the agent should move it up, only user->claude
> interactions, claude finishing should not move it up."*

This resolves **#39 in favour of option (a)** — *keep commitment ordering,
document it as intended* — with the correction that today's implementation of
commitment ordering is **incorrect** in two ways. We are not switching to
`last_interacted`; we are making the commitment key a total order and making
the live→dormant transition order-preserving.

---

## 1. Problem

Two independent defects, both reproducible with no race, both visible daily.

### 1.1 Whole-second ties silently swallow a prompt

Both writers of the order key stamp `now_unix()` — whole seconds
(`crates/clave/src/store.rs:352-357`):

```rust
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
```

Ties resolve on `tab.position` ascending (`crates/clave-bar/src/model.rs:756`
supplies the tiebreak, `:791` applies it), so the **lower-positioned tab wins
regardless of who was actually touched last**.

Worked example — two agents, prompts 300 ms apart inside one wall second:

```text
store: tab_timeline = {10: 1000, 11: 1000}
tabs:  10 @ pos 0 (u-A),  11 @ pos 1 (u-B)

t=1000.10  user prompts u-B  → hook stamps tab_timeline[11] = 1000  (no change)
t=1000.40  user prompts u-A  → hook stamps tab_timeline[10] = 1000  (no change)

rows before = 0: Tab(10)   rows after = 0: Tab(10)     ← identical
              1: Tab(11)                 1: Tab(11)
```

The user typed into B and **nothing moved**. Half the time the tie flatters the
result (A was prompted last and A is on top), which is why this reads as
"sometimes it doesn't work" rather than as a deterministic bug.

A second, worse shape: the pre-lock clock read. `clave touch` computes the
timestamp *before* taking the flock — `crates/clave/src/main.rs:305`,
`store::apply_touch(&paths, tab_id, store::now_unix())` — so two touches can
serialize in the opposite order to their clock reads. `apply_touch`'s max-merge
(`store.rs:213-220`) exists solely to absorb that, and cannot absorb a tie.

### 1.2 The live→dormant demotion reorders neighbours on every close

Closing a tab changes that row's key from `timeline[tab_id]`
(`model.rs:755-756`) to `agent.last_interacted` (`model.rs:777-778`) **and** its
tiebreak class from `t.position` to `usize::MAX - i` (`model.rs:782`). At an
equal key every live row therefore outranks every dormant row.

Worked example (the dossier's, with the numbers made explicit):

```text
store: tab_timeline = {10: 1000, 11: 1000}
       agent u-A: tab_id = 10, last_interacted = 1000
tabs:  10 @ pos 0 (u-A),  11 @ pos 1 (plain terminal tab)

BEFORE   entries = (1000, 0, Tab(10)), (1000, 1, Tab(11))
         rows    = 0: Tab(10)   ← u-A, amber ●
                   1: Tab(11)

Alt+w closes tab 10.
apply_tabs (model.rs:694-712) emits PruneTabs{[10]};
apply_prune_tabs (store.rs:278-298) drops tab_timeline[10] and clears u-A.tab_id.

AFTER    entries = (1000, 1, Tab(11)), (1000, MAX, Dormant(u-A))
         rows    = 0: Tab(11)         ← "an unrelated tab jumped to the top"
                   1: Dormant u-A     ← dim ◌, "it went idle"
```

Tab 11 rose without the user doing anything to it. No race, no stale frame.

**The aggravator.** `apply_touch` (`store.rs:213-220`) bumps only the timeline;
it never writes `last_interacted`. So a tab that was born and used but never
prompted keeps whatever `last_interacted` its record was created with —
`add.rs:748`'s `now_unix()` at `clave add` time, or `0` for a row that never
went through a stamping path. Its live key is a *fresh* touch; its dormant key
is a *creation-time* clock. Closing that tab therefore drops it from the top of
the list to below every row prompted since it was created — in the `0` case, to
the very bottom, beneath every dormant row that has ever been prompted.

### 1.3 Why these are one fix

Both are consequences of the key not being a total order over rows. Once every
committed row carries a **unique** ordinal, (a) ties cannot occur, so the
tiebreak class stops mattering, and (b) the demotion can carry the key across
the live→dormant boundary, so neighbours cannot move. One key change kills both.

---

## 2. Semantics, stated as rules

This table is the answer to #39 and should be pasted into the issue on close.
It covers events we are **not** changing so it is complete.

**Definition.** A row's position is determined by its **commitment ordinal** — a
strictly increasing integer minted by the store under the flock, once per user
commitment. Higher ordinal = higher in the list. A row that has never received a
commitment has ordinal `0` and sits below every row that has.

| # | Event / user action | Signal | Reorders? | Mechanism |
|---|---|---|---|---|
| 1 | **User submits a prompt to an agent** | `UserPromptSubmit` hook (`hook.rs:241-248`) | **YES** — the agent's row goes to the top | mints an ordinal; writes it to `tab_order[bind]` **and** `agent.commit_ord` |
| 2 | Claude finishes a turn | `Stop` | **no** | status → `Done` only (`hook.rs:44`) |
| 3 | Claude's turn fails | `StopFailure` | **no** | status → `Failed` |
| 4 | Claude needs the human | `Notification` / `PermissionRequest` | **no** | status → `NeedsYou` (`hook.rs:46-65`) |
| 5 | Claude's session ends | `SessionEnd` | **no** | status → `Idle` (`hook.rs:45`) |
| 6 | Any other hook (`PreToolUse`, …) | — | **no** | `status_for_event` returns `None` (`hook.rs:67-69`) |
| 7 | Focusing a tab | `clave focus` / `clave-visited` beacon | **no** | unchanged; `apply_focus` (`store.rs:197-207`) never touches the order; beacon at `model.rs:330-341` |
| 8 | `Alt+j`/`Alt+k`/`Alt+↑`/`Alt+↓` nav | plugin nav pipe | **no** | unchanged — the rationale at `setup.rs:96-99` (walking a list that reorders under you ping-pongs) |
| 9 | `Alt+1..9` row jump | plugin nav pipe | **no** | unchanged |
| 10 | Mouse click on a row | `Mouse` event | **no** | unchanged (`model.rs:795-823`) |
| 11 | Bar collapse toggle (`Alt+c`) | `clave collapse` | **no** | unchanged |
| 12 | **New tab created** (`Alt+t`, `clave open`, `clave add`) | birth touch, `clave touch <tab_id>` (`clave-bar/src/main.rs:434-445`) | **the new row enters at the top**; existing rows keep their relative order | mints an ordinal into `tab_order[tab_id]`. Unchanged from today |
| 13 | **New store row created** (`clave add`, resume) | `add.rs:740-758` | **the new row enters at the top** | mints an ordinal into `agent.commit_ord` (new — today it inherits nothing and can sink) |
| 14 | **Tab closed** (`Alt+w`) | `PruneTabs` → `apply_prune_tabs` | **no reorder of survivors**; the closed tab's agent becomes a dormant row **at the same index** | the agent inherits the tab's ordinal (new) |
| 15 | Session (re)create | `clear_session_order` (`setup.rs:651`) | tab ordinals cleared; **agent ordinals survive** | dormant rows keep their cross-session recency order; live tabs sort at `0` until their birth touch lands |
| 16 | **User interacts with a plain terminal tab** | **S2 — not in this workstream** | **will be YES** | S2 calls the *existing* `clave touch <tab_id>`; no new field, no comparator change. See §3.5 |

Two rules follow that are worth stating in prose because they are what the
maintainer will check:

- **R1 — a prompt always moves its row to row 0.** No tie, no clock, no
  position dependence.
- **R2 — closing a tab moves nothing.** The closed tab's row keeps its index and
  changes glyph (`●` → `◌`); every other row keeps its index.

---

## 3. Design

### 3.1 Chosen key — option (a), the store's `seq`, extended to dormant rows

**The ordinal is the store's own `seq`, minted inside `with_store_mut`'s flock.**

`Store.seq` (`store.rs:74-77`) is already a persisted, monotonic, cross-process
counter — every mutating store operation bumps it by 1 under the exclusive flock
(`store.rs:135-164`), and nothing anywhere resets it. It is, precisely, a Lamport
clock for the store. Two distinct commitments are two distinct locked writes and
therefore receive two distinct, ordered ordinals **by construction**, with no
clock involved.

The key space is unified across the two row classes:

| Row class | key today | key after |
|---|---|---|
| live tab row | `tab_timeline[tab_id]` (unix s) | `tab_order[tab_id]` (ordinal) |
| dormant agent row | `agent.last_interacted` (unix s) | `agent.commit_ord` (ordinal), *or* the ordinal of the tab it was just unbound from (§3.3) |

`last_interacted` **keeps its wall-clock meaning and every existing consumer** —
`clave ls`'s recency (`lsview.rs:14`), the eager-launch pick
(`setup.rs:520-532`), the `clave add` picker column (`add.rs:277`), the dev
scenarios' staggered recency (`dev.rs:240`). It is display and cross-session
policy; it is no longer an ordering key. That is the answer to the direction's
"live and dormant keys must remain comparable in one merged list": they are
comparable because **both become ordinals**, not because the ordinal is made to
look like a clock.

Why this works across the awkward cases:

- **Across a restart.** `seq` lives in the store file and only increases, so an
  ordinal minted last week is strictly below one minted today. Dormant rows keep
  their correct relative order at cold start with no clock comparison.
- **Across `clear_session_order`** (the renamed `clear_tab_timeline`,
  `store.rs:340-349`). It clears `tab_order` because tab ids are session-scoped
  — unchanged reasoning. It deliberately does **not** clear `commit_ord`, which
  is agent-scoped and must survive: clearing it would collapse every dormant row
  to `0` and cold-start the list in uuid order.
- **Against `apply_touch`'s max-merge.** With the ordinal minted *inside* the
  lock it is strictly greater than every previously minted ordinal, so a plain
  `insert` is already a max. The whole "a late/duplicate older stamp regresses
  the order" class — which only existed because `now_unix()` was read before the
  lock at `main.rs:305` — is deleted, not defended against.

### 3.2 Ties, honestly

The ordinal makes ties **unreachable for committed rows**, but two residual
classes remain and the existing tiebreak is retained to keep them deterministic:

1. **Ordinal `0`** — never-committed rows. Multiple unstamped live tabs and
   never-prompted agents all sit at `0`. Tiebreak: `t.position` ascending for
   live rows, `usize::MAX - i` for dormant (uuid-descending), exactly as today.
2. **The RC-A eviction window** — `apply_bind` evicts a previous tenant
   (`store.rs:239-245`), leaving an agent with `commit_ord == X` dormant while
   `tab_order[tab] == X` still renders for the new tenant. One tie, transient,
   healed by the next commitment. S0 removes the cause.

So the invariant we test is **"at most one rendered row per non-zero ordinal"**,
not "no ties ever". Claiming the stronger property would be false and would make
the proptest a lie.

**Unstamped live tabs stay at `0`** (i.e. below dormant rows) — deliberately not
changed here. A live tab with no stamp means its birth touch never landed, which
is **RC-B / S0's** defect; papering over it with a sentinel (`u64::MAX`, or
"live always beats dormant") would hide S0's symptom and break R2, which
*requires* a dormant row to be able to sit above a live one.

### 3.3 The demotion — the row inherits the tab's ordinal

Two carries, one durable and one immediate. Both read the same authoritative
data, so they cannot diverge from each other.

**Store-side (durable).** `apply_prune_tabs` copies before it deletes: for every
agent whose `tab_id` is in the stale set, `commit_ord = max(commit_ord,
tab_order[tab_id])`, then the entry is removed as today. This is what fixes the
aggravator in §1.2 — a touch-only tab's ordinal lands on the agent instead of
being thrown away.

**Render-side (immediate).** The bar computes a dormant row's ordinal as
`max(agent.commit_ord, tab_order[agent.tab_id])`. Between the tab vanishing from
`TabUpdate` and the prune's snapshot echo landing, the bar still holds both
halves from the *same* seq-gated snapshot, so the row holds its place from the
very first repaint — the ordering fix does not wait on a fire-and-forget
subprocess, and therefore does not depend on S3.

The recycled-tab-id hazard does not reach this leg: `is_dormant`
(`model.rs:479-488`) returns false whenever *any* live tab carries the agent's
`tab_id`, so a dormant row can never read a fresh tenant's ordinal.

Re-running the §1.2 example with the fix, `55`/`56` standing for consecutive
ordinals:

```text
store: tab_order = {10: 56, 11: 55};  u-A: tab_id = 10, commit_ord = 56
tabs:  10 @ pos 0 (u-A),  11 @ pos 1 (plain terminal tab)

BEFORE   rows = 0: Tab(10)  [56]   ← u-A, amber ●
                1: Tab(11)  [55]

Alt+w closes tab 10 → prune carries 56 onto u-A, drops tab_order[10].

AFTER    rows = 0: Dormant u-A [56]  ← same index, glyph ● → ◌
                1: Tab(11)     [55]  ← did not move
```

And the touch-only variant (`u-A.commit_ord == 0`, born but never prompted):
today it falls from row 0 to the bottom; with the carry it holds row 0 at
ordinal `56`.

**Coherence with `dormant_rows_sort_into_the_unified_recency_order`
(`model.rs:2112`).** That test pins *one merged list* — a dormant row can outrank
a live tab row. That behaviour is not merely still wanted, it is **load-bearing
for R2**: if dormant rows sank below live rows, "hold your place on close" would
be unimplementable. The test keeps its shape and its assertion; only its input
key changes from `last_interacted` to `commit_ord`. I disagree with nothing here
and have no counter-case to offer: the alternative (segregate dormant rows to the
bottom) reintroduces the exact jump the maintainer reported.

### 3.4 Rejected alternatives

| Option | Verdict | Why |
|---|---|---|
| **(b) milliseconds + a monotonic counter tiebreak** | rejected | Buys a smaller tie window, not a total order — the counter still has to be persisted and compared, which is option (a) with a clock bolted on. Keeps the pre-lock read hazard (`main.rs:305`) and adds a new one: wall clock can step backwards under NTP, and `apply_touch`'s max-merge would then pin an entry in the future permanently. Does nothing at all for §1.2. |
| **(c) a separate per-row monotonic ordinal counter** | rejected as a *separate counter*; adopted as a *concept* | A second counter must be bumped in lockstep with `seq` under the same lock forever; the first commit that bumps one and forgets the other silently corrupts the order with no test able to see it. One counter, one invariant. The mitigation for the conflation risk that a shared counter creates is mechanical: a single mint site (`Store::mint_ord`), and no code path ever compares an ordinal to a snapshot `seq`. |
| **Order by `last_interacted` everywhere** (#39 option b) | rejected by the ruling | Would make focus/close/finish-driven reordering unavoidable and re-open the nav ping-pong that `setup.rs:96-99` documents. |
| **Sentinel key for unstamped live tabs** | rejected | Hides RC-B (S0) and breaks R2 — see §3.2. |
| **Keep the field name `tab_timeline`, change only the values** | rejected | See §3.6 — a store file written by an older binary holds unix seconds (~1.7 × 10⁹) which would outrank every ordinal *forever* under any max-merge. Poison with no expiry. |

### 3.5 The seam S2 must be able to use

S2 (terminal-tab interaction, RC-D) needs "the user gave an instruction to a
plain tab" to reorder that tab. After this change it needs **no schema change and
no comparator change**:

- `tab_order` is keyed by **tab id** and knows nothing about agents. A plain
  terminal tab is already a first-class key holder (birth touches already write
  one).
- The mint is the existing `clave touch <tab_id>` CLI surface, whose signature is
  **unchanged** by this spec (only its internal `now` parameter goes away). S2
  adds a *caller*, not a key.
- Because plain tabs and agent tabs draw from one ordinal space, a terminal
  interaction and a prompt interleave correctly with no cross-space comparison.

The single thing S2 owes: a debounce/gate on its trigger. Every `clave touch` is
a locked RMW plus a snapshot push, and an ungated per-keystroke trigger is the
documented fd-exhaustion path (`SUBSYSTEM-VALIDATION.md:232-243`). That is S2's
problem, not a property of the key.

### 3.6 Migration and compatibility

**Does the store have a version field?** No. `Store` is
`{ seq, agents, tab_timeline, collapsed }` (`store.rs:74-97`); there is no
version, and `serde(deny_unknown_fields)` appears nowhere in `crates/` — every
field is `#[serde(default)]` and unknown fields are ignored.

**Clean break, no schema migration, one data backfill.**

1. **`tab_timeline` is renamed to `tab_order`, not repurposed.** An old store
   file's `tab_timeline` is silently ignored by the new binary; `tab_order`
   defaults empty. This is what makes the break safe: unix-second values can
   never leak into the ordinal space. The transitional window that a
   *repurpose* would leave open is real and long — `clear_tab_timeline` only
   runs at session *create* (`setup.rs:647-651`), so a maintainer who upgrades
   mid-session would carry `1.7e9` values until his next session launch, and
   every one of them would outrank every new ordinal.
2. **No `tab_order` at startup is already a modelled state.** An empty map is
   exactly the cold-start state after `clear_session_order`; live rows sort at
   `0` in tab-position order and the first commitment sorts them. Nothing new.
3. **`commit_ord` needs a backfill, and gets one.** Left to default, every
   pre-existing dormant row would key `0` and the dormant list would render in
   uuid order on the first launch after the upgrade — a visible, immediate
   regression on the maintainer's real fleet. So `clear_session_order` (which
   already runs at every launch, under the lock) seeds ordinals for rows with
   `commit_ord == 0 && last_interacted > 0`, in `last_interacted`-ascending
   order. It is a data backfill of ~10 lines, self-limiting (after one launch
   nothing matches), and it converts the old wall-clock recency into the new
   ordinal space exactly once.

**Mixed-version fleets** (the #43/#44 shape — a stale binary answering a plugin
shellout):

| Combination | Behaviour |
|---|---|
| new CLI + **old plugin** | Old plugin reads `tab_timeline`, absent → empty map → every live row keys `0` → rows render in **tab-position order**. Degraded, deterministic, not corrupt. Identical to the pre-`tab_timeline` behaviour the existing `snapshot_carries_tab_timeline_and_defaults_empty` test already pins (`clave-types/src/lib.rs:254-270`). |
| old CLI + **new plugin** | New plugin reads `tab_order`, absent → empty → same positional degradation. `commit_ord` absent → dormant rows at `0`. |
| new CLI + new plugin, **old store file** | Backfill (3) restores dormant order at the next launch; `tab_order` starts empty as at any cold start. |

This is a **cross-process / IPC** change under the taxonomy
(`docs/dev/TESTING.md:117`) because the wire schema moves: the PR must carry the
written ordering/idempotency argument and an adversarial reviewer. §5 lists what
that argument must contain.

---

## 4. Implementation

Numbered, file by file. Quoted blocks are the code being replaced.

### 4.1 `crates/clave-types/src/lib.rs` — the wire schema

**1.** `Agent` (`:39-68`) gains a field, after `last_interacted` (`:52-53`):

```rust
    /// unix seconds; bumped on UserPromptSubmit → drives recency sort.
    pub last_interacted: u64,
```

becomes

```rust
    /// unix seconds; bumped on UserPromptSubmit. DISPLAY and cross-session
    /// policy only (`clave ls`, the picker, eager-launch selection) — it is
    /// NOT the bar's sort key (S1/#39: whole seconds tie, and ties resolved
    /// on tab position, so the wrong row won).
    pub last_interacted: u64,
    /// §6.6 commitment ORDINAL: the store `seq` of this row's last user
    /// commitment (a prompt, its creation, or the ordinal inherited from its
    /// tab when that tab closed). Minted under the store flock, so it is a
    /// total order with no clock and no ties. 0 = never committed → bottom.
    #[serde(default)]
    pub commit_ord: u64,
```

**2.** `AgentSnapshot.tab_timeline` (`:77-83`) is renamed to `tab_order`, with
the doc-comment reworded from "unix seconds of the last user commitment" to
"commitment ordinal", keeping the existing "REPLACES, never merges" paragraph
verbatim.

**3.** Fixture updates at `:160-172`, `:181-192`, `:205-216`, `:231-242`,
`:258-263`, `:278-283` (mechanical: add `commit_ord`, rename `tab_timeline`).

### 4.2 `crates/clave/src/store.rs` — the mint, the carry, the backfill

**4.** `Store` (`:74-97`): rename `tab_timeline` → `tab_order` (`:82-88`), doc
updated to say the values are ordinals, and add the mint:

```rust
impl Store {
    /// Mint the next commitment ORDINAL (§6.6 / S1). The store's `seq` IS the
    /// ordinal: it is persisted, monotonic, and bumped exactly once per locked
    /// write, so two commitments can never collide and no wall clock is
    /// involved. Callers MUST be inside `with_store_mut` — the flock is what
    /// makes this a total order — and must NOT bump `seq` again afterwards.
    ///
    /// The shared counter is deliberate (S1 §3.4): a second counter would have
    /// to be bumped in lockstep forever, and the first write that forgot would
    /// corrupt the order invisibly. The rule that keeps the two roles from
    /// being conflated: NOTHING ever compares an ordinal to a snapshot `seq`.
    fn mint_ord(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }
}
```

**5.** `AgentRecord` (`:39-69`) gains `commit_ord: u64` with `#[serde(default)]`,
doc-linked to the wire field. Every literal construction site must be updated:
`store.rs:371`, `hook.rs:303`, `lsview.rs:34`, `add.rs:741`, `add.rs:772`,
`setup.rs:792`, `setup.rs:828`, `setup.rs:989`, `setup.rs:1255`, `dev.rs:229`,
`open.rs:144`, `tests/kdl_guardrail.rs:59`.

**6.** `snapshot_from` (`:167-189`): `tab_timeline: store.tab_timeline.clone()`
→ `tab_order: store.tab_order.clone()`, and `commit_ord: r.commit_ord` added to
the `Agent` projection.

**7.** `apply_touch` (`:213-220`) loses its `now` parameter:

```rust
pub fn apply_touch(paths: &StorePaths, tab_id: usize, now: u64) -> Result<AgentSnapshot> {
    with_store_mut(paths, |s| {
        let e = s.tab_timeline.entry(tab_id).or_insert(0);
        *e = (*e).max(now);
        s.seq += 1; // monotonic pipe contract (§5)
        snapshot_from(s)
    })
}
```

becomes

```rust
/// `clave touch <tab_id>` (§6.6): stamp a user commitment on the STORE's tab
/// order and hand back a seq-bumped snapshot for the pipe push. The ordinal is
/// minted INSIDE the lock, so it is strictly greater than every ordinal already
/// in the map — the old max-merge existed only because `now` was read BEFORE
/// the lock (main.rs) and two touches could serialize against their clock
/// reads. That race is now impossible, not merely absorbed.
pub fn apply_touch(paths: &StorePaths, tab_id: usize) -> Result<AgentSnapshot> {
    with_store_mut(paths, |s| {
        let ord = s.mint_ord();
        s.tab_order.insert(tab_id, ord);
        snapshot_from(s)
    })
}
```

**8.** `apply_prune_tabs` (`:278-298`) gains the carry. Replace:

```rust
        let before = s.tab_timeline.len();
        s.tab_timeline.retain(|id, _| !stale_ids.contains(id));
        let mut changed = s.tab_timeline.len() != before;
        for r in s.agents.values_mut() {
            if r.tab_id.is_some_and(|id| stale_ids.contains(&id)) {
                r.tab_id = None;
                changed = true;
            }
        }
```

with:

```rust
        let mut changed = false;
        // S1: the row INHERITS its tab's ordinal before the entry dies, so a
        // close moves NOTHING (R2). Without this the row falls back to a
        // different key in a different tiebreak class and every neighbour
        // re-sorts — the "an unrelated tab jumped to the top" report. `max`
        // keeps this idempotent and commuting with a second prune (the #6/F3
        // order-safety property): a re-run finds tab_id already None and
        // carries nothing.
        for r in s.agents.values_mut() {
            if let Some(id) = r.tab_id.filter(|id| stale_ids.contains(id)) {
                let carried = s_tab_order_get(&s_tab_order, id); // see note
                r.commit_ord = r.commit_ord.max(carried);
                r.tab_id = None;
                changed = true;
            }
        }
        let before = s.tab_order.len();
        s.tab_order.retain(|id, _| !stale_ids.contains(id));
        changed |= s.tab_order.len() != before;
```

*Borrow note:* `s.agents.values_mut()` and `s.tab_order` cannot both be borrowed
in one loop. Take the carries first into a `Vec<(String, u64)>` (or clone the
handful of needed entries out of `tab_order`) before the mutable pass; the
committed form must not use the pseudo-call above.

**9.** `clear_tab_timeline` (`:340-349`) is renamed `clear_session_order`, gains
the backfill, and its write gate is restructured so the backfill is not skipped:

```rust
pub fn clear_tab_timeline(paths: &StorePaths) -> Result<()> {
    with_store_mut(paths, |s| {
        let bound = s.agents.values().any(|r| r.tab_id.is_some());
        if !s.tab_timeline.is_empty() || bound {
            s.tab_timeline.clear();
            s.agents.values_mut().for_each(|r| r.tab_id = None);
            s.seq += 1; // content changed ⇒ seq changed (§5)
        }
    })
}
```

becomes (shape, not final text):

```rust
/// Session (re)create hygiene: tab ids are SESSION-scoped, so a fresh session
/// must inherit neither dead tabs' commitments (reused ids) nor stale uuid→tab
/// binds. Agent ordinals (`commit_ord`) are agent-scoped and deliberately
/// SURVIVE — clearing them would collapse every dormant row to 0 and cold-start
/// the list in uuid order instead of recency order.
///
/// Also the S1 BACKFILL point (see the spec's migration section): a store
/// written by a pre-ordinal binary has `commit_ord == 0` everywhere. Seed those
/// rows from their wall-clock `last_interacted` ranking, once, so the upgrade
/// does not visibly scramble the dormant list. Self-limiting: after one launch
/// nothing matches. No push — no bar instance exists yet at launch time.
pub fn clear_session_order(paths: &StorePaths) -> Result<()> {
    with_store_mut(paths, |s| {
        let changed = !s.tab_order.is_empty() || s.agents.values().any(|r| r.tab_id.is_some());
        s.tab_order.clear();
        s.agents.values_mut().for_each(|r| r.tab_id = None);
        // Backfill, oldest first, so the seeded ordinals preserve the old
        // wall-clock ranking.
        let mut stale: Vec<(u64, String)> = s
            .agents
            .values()
            .filter(|r| r.commit_ord == 0 && r.last_interacted > 0)
            .map(|r| (r.last_interacted, r.uuid.clone()))
            .collect();
        stale.sort();
        // `mint_ord()` bumps `seq` itself, so track whether it ran. The §5
        // invariant is "content changed ⇒ seq advanced exactly once"; a mint
        // already satisfies it, so the trailing bump fires ONLY when we changed
        // something (cleared binds/order) WITHOUT minting (CodeRabbit
        // 2026-07-22: the earlier draft bumped unconditionally on `changed`,
        // double-advancing `seq` whenever the backfill also minted).
        let minted = !stale.is_empty();
        for (_, uuid) in stale {
            let ord = s.mint_ord();
            if let Some(r) = s.agents.get_mut(&uuid) {
                r.commit_ord = ord;
            }
        }
        if changed && !minted {
            s.seq += 1; // content changed but nothing minted ⇒ one bump here (§5)
        }
    })
}
```

Note `mint_ord` already bumps `seq`, so the trailing bump must be skipped when
the backfill minted anything — fold it into one `changed`/`bumped` decision so
the §5 invariant ("content changed ⇒ seq changed") holds exactly once.

**10.** Caller update: `setup.rs:651`
`crate::store::clear_tab_timeline(...)` → `clear_session_order(...)`.

### 4.3 `crates/clave/src/hook.rs` — the one reordering writer

**11.** `apply_hook_event` (`:224-258`). Replace the tail:

```rust
    let mut stamp = None;
    if event == "UserPromptSubmit" {
        rec.last_interacted = now; // recency (§6.6 order)
        stamp = rec.tab_id;
        changed = true;
    }
    changed |= refresh_label(rec, event, payload, jsonl_tail);
    if let Some(tab_id) = stamp {
        let e = s.tab_timeline.entry(tab_id).or_insert(0);
        *e = (*e).max(now);
    }
    if changed {
        s.seq += 1; // monotonic pipe contract (§5)
    }
    changed
```

with:

```rust
    // §6.6 / S1 / #39: a PROMPT is the ONLY event that reorders. Stop,
    // StopFailure, Notification, PermissionRequest and SessionEnd change the
    // STATUS and nothing else — "claude finishing should not move it up"
    // (maintainer ruling, 2026-07-22).
    let commitment = event == "UserPromptSubmit";
    let mut commit_tab = None;
    if commitment {
        rec.last_interacted = now; // wall clock: `clave ls`, picker, eager_row
        // Design B: the prompt commits to the agent's TAB — stamp through the
        // bind, atomically with the bump (a bar-side stamp would race the user
        // switching away).
        commit_tab = rec.tab_id;
        changed = true;
    }
    changed |= refresh_label(rec, event, payload, jsonl_tail);
    if !changed {
        return false;
    }
    // The write's own seq IS the commitment ordinal (§5 pipe contract and §6.6
    // row order share one counter — see Store::mint_ord).
    let ord = s.mint_ord();
    if commitment {
        if let Some(tab_id) = commit_tab {
            s.tab_order.insert(tab_id, ord);
        }
        if let Some(rec) = s.agents.get_mut(uuid) {
            // Set on the AGENT too, not only the tab: an unbound agent (RC-B,
            // or a prompt that lands before `clave bind`) still records its
            // commitment, and the dormant row it becomes on close sorts right
            // even if the prune never lands.
            rec.commit_ord = ord;
        }
    }
    true
```

Behaviour preserved: the function still returns "did anything change" and still
bumps `seq` exactly once. `commitment` implies `changed`, so `mint_ord` always
runs for a prompt.

### 4.4 `crates/clave/src/main.rs` — CLI

**12.** `Command::Touch` handler (`:301-308`): drop the pre-lock clock read.

```rust
            let snap = store::apply_touch(&paths, tab_id, store::now_unix())?;
```
→
```rust
            let snap = store::apply_touch(&paths, tab_id)?;
```

The clap surface (`:93-101`) is **unchanged** — `clave touch <tab_id>` still
takes exactly one argument, so no new parse pin is required and S2 inherits the
surface as-is. Update the doc comment's "with host time" wording.

**13.** `Command::PruneTabs` doc (`:115-121`): note that the prune now also
carries the tab's ordinal onto the agent.

### 4.5 `crates/clave/src/lsview.rs` — make the documented claim true

**14.** `render_ls` (`:7-24`) currently claims "Ordering matches the bar (§6.6)"
(`:2-3`) while sorting on `last_interacted` (`:14`) — which is not, and never
was, the bar's rule. Replace:

```rust
    rows.sort_by_key(|r| std::cmp::Reverse(r.last_interacted));
```

with a sort on the row's **effective ordinal** — the tab's ordinal while it is
bound and live, else the agent's own:

```rust
    // The BAR's rule (§6.6 / S1): the commitment ordinal, tab-first. This makes
    // `clave ls` an exact oracle for the sidebar's agent-row order — the
    // divergence between the two was itself a diagnostic signature (dossier
    // "Read-only live diagnosis"), and it can only be one if the two agree when
    // nothing is broken. `last_interacted` remains the display/cross-session
    // clock and the secondary key.
    let ord = |r: &&AgentRecord| {
        let tab = r.tab_id.and_then(|id| store.tab_order.get(&id)).copied().unwrap_or(0);
        (tab.max(r.commit_ord), r.last_interacted)
    };
    rows.sort_by_key(|r| std::cmp::Reverse(ord(r)));
```

`clave ls` shows only store rows, so plain terminal tabs are absent from the
comparison — say so in the doc comment; the live SOP relies on it (§6).

**`clave ls` is an exact order oracle only for rows with unique, non-zero
ordinals (CodeRabbit 2026-07-22).** Even with the ordinal-aware `ord` above,
`clave ls` cannot reproduce the sidebar order in two cases: (i) multiple live
rows at ordinal `0` — a reachable state — where the bar tie-breaks on **tab
position** and `clave ls` has no position column, and (ii) transient ordinal
ties during a burst. The CLI is not given tab positions (adding them is possible
but out of S1's scope), so the live SOP in §6 must **compare only rows with
unique non-zero ordinals** and treat any ordinal-0 or tied-ordinal rows as
"order unspecified between them, not a discrepancy." §6's steps are written to
that rule; a mismatch there is a real bug only when both rows carry distinct
non-zero ordinals.

This is the one piece of scope beyond the two defects. It is justified because
(a) the doc comment is currently false, and (b) §6's live validation needs a
read-only oracle for the expected order. If a reviewer objects, it can be split
into a follow-up commit without affecting anything else.

### 4.6 `crates/clave/src/add.rs` — new rows get an ordinal

**15.** Record creation (`:740-758`). `last_interacted: now_unix()` stays;
`commit_ord` is minted from the same locked write:

```rust
    let snap = with_store_mut(&paths, |s| {
        let ord = s.mint_ord();          // replaces the `s.seq += 1` below
        let fresh = AgentRecord { /* … */ last_interacted: now_unix(), commit_ord: ord, /* … */ };
        let mut merged = merge_resume_record(s.agents.get(&uuid), fresh);
        // A resume opens a brand-new tab, which birth-touches to the top
        // anyway; giving the ROW the same ordinal keeps the two consistent and
        // stops the row plunging if that tab is closed before any prompt.
        merged.commit_ord = ord;
        s.agents.insert(uuid.clone(), merged);
        snapshot_from(s)
    })?;
```

`merge_resume_record` (`:343-354`) needs no change: it preserves everything via
`..row.clone()`, and the explicit assignment above overrides. Its pinning test
(`merge_resume_preserves_existing_row_and_resets_status`, `:890-921`) keeps
passing; add an assertion that `commit_ord` is among the preserved fields.

**16.** `clave open` (`open.rs`) needs **no** change: opening a dormant row
creates a tab, the tab birth-touches, and the row rises through `tab_order`. Its
`commit_ord` catches up on the next prune-carry or prompt.

### 4.7 `crates/clave-bar/src/model.rs` — the comparator

**17.** Field rename (`:179-185`): `timeline` → `tab_order`, doc updated
("unix seconds of the last USER COMMITMENT" → "commitment ordinal"). Follow-on
sites: `:384` (`needs_birth_touch`), `:528` (`self.timeline = snap.tab_timeline`
→ `self.tab_order = snap.tab_order`), `:702` (stale detection).

**18.** Replace `sort_key` (`:387-393`):

```rust
    /// §6.6 sort key: the STORE's tab timeline, nothing else.
    fn sort_key(&self, t: &TabMeta) -> u64 {
        self.timeline.get(&t.tab_id).copied().unwrap_or(0)
    }
```

with a pair, plus a module-level sentinel beside the other `model.rs` consts
(`:137-151`) — module level, not an associated const, so the methods read
`NO_COMMITMENT` rather than `Self::NO_COMMITMENT`:

```rust
/// A row that has never received a user commitment. Sorts below every row that
/// has. Reachable for a live tab only when its birth touch never landed — that
/// is RC-B/S0's defect, deliberately NOT papered over here (S1 §3.2).
const NO_COMMITMENT: u64 = 0;
```

```rust
    /// §6.6 ordering key for a LIVE tab row: the STORE's tab_order, nothing
    /// else. Agent prompts reach it via the hook's bind-keyed stamp (Design B)
    /// — a render-time `last_interacted` join here is exactly what diverged in
    /// round 6 (each instance's register/manifest state differs).
    fn live_ord(&self, t: &TabMeta) -> u64 {
        self.tab_order.get(&t.tab_id).copied().unwrap_or(NO_COMMITMENT)
    }

    /// §6.6 ordering key for a DORMANT row (S1). The agent's own ordinal, OR —
    /// while the store has not yet pruned the tab it was bound to — that tab's
    /// ordinal. The second leg is what makes a close move NOTHING (R2) on the
    /// FIRST repaint, without waiting for the fire-and-forget `clave prune-tabs`
    /// echo. Both legs come from the SAME seq-gated snapshot, so no instance can
    /// compute a different value (the round-6 / C5-rd-5 divergence class needs
    /// two independent sources; this has one). `is_dormant` guarantees no LIVE
    /// tab holds `a.tab_id`, so a recycled id can never be read here.
    fn dormant_ord(&self, a: &Agent) -> u64 {
        let carried = a
            .tab_id
            .and_then(|id| self.tab_order.get(&id))
            .copied()
            .unwrap_or(NO_COMMITMENT);
        a.commit_ord.max(carried)
    }
```

**19.** `rows()` (`:722-793`): `self.sort_key(t)` → `self.live_ord(t)` at `:756`;
`a.last_interacted` → `self.dormant_ord(a)` at `:778`. The tiebreaks (`t.position`
at `:756`, `usize::MAX - i` at `:782`) and the comparator (`:791`) are
**unchanged** — they are now a determinism residual for the `0` class and the
RC-A eviction window (§3.2), not the ordering mechanism. Rewrite the doc comment
at `:722-727` accordingly.

**20.** Test-fixture churn: 29 `AgentSnapshot { … }` literals in `model.rs` need
`tab_timeline:` → `tab_order:`, plus the `snap_t` helper (`:1188-1195`). Purely
mechanical; the behavioural test changes are listed in §5.2.

### 4.8 Documentation

**21.** `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md`: §5's
store paragraph (`:310-316`) and §6.6's *Order = last USER COMMITMENT* block
(`:511-544`) describe "one unified timeline in unix seconds" (`:514-515`),
"Tie-break: tab position" (`:541`) and dormant rows joining "by the store row's
`last_interacted`" (`:543-544`). All three are superseded — replace with the §2 rules table and the ordinal
definition, keeping the C4/C5/round-6 provenance notes verbatim.

**22.** `crates/clave/src/setup.rs:96-99` (the comment above the `Alt+j/k`
binds) is the user-facing statement of the rule and is quoted in #39. Extend it
with R1/R2.

**23.** Close #39 with the §2 table as the closing comment.

---

## 5. Test plan

Change class per the risk taxonomy (`docs/dev/TESTING.md:112-120`): **pure
logic / model** for the comparator and store transitions, **plus cross-process /
IPC** because the wire schema changes. So: TDD red-first, `cargo test
--workspace`, extended proptests, **and** a written ordering/idempotency
argument in the PR dossier plus an adversarial reviewer. No
`needs-live-validation` label is strictly required by the taxonomy, but §6 exists
and the PR should carry the label anyway — this is a change the maintainer will
judge with his eyes.

### 5.1 Tier 1 — new tests

**`crates/clave/src/store.rs`**

| Test | Asserts |
|---|---|
| `ordinals_are_minted_strictly_increasing_under_the_lock` | successive `apply_touch` calls on the *same* tab and on *different* tabs produce strictly increasing values; the map value never regresses without any `now` argument existing |
| `prune_carries_the_tabs_ordinal_onto_the_agent` | the §1.2 fixture: after `apply_prune_tabs(&[10])`, `agents["u-A"].commit_ord == tab_order[10]`'s old value, entry gone, `tab_id == None` |
| `prune_carry_is_idempotent_and_commutes` | second prune of the same id: no change, no push, no seq bump, `commit_ord` unchanged — the #6/F3 property, extended to the new write |
| `prune_carry_never_lowers_an_agents_ordinal` | agent prompted *after* its tab's last touch: `max` keeps the higher value |
| `clear_session_order_backfills_pre_ordinal_rows` | rows with `commit_ord == 0 && last_interacted > 0` get ordinals in `last_interacted` order; rows already carrying an ordinal are untouched; a second call is a no-op |
| `clear_session_order_preserves_agent_ordinals` | `commit_ord` survives, `tab_order` and binds do not |

**`crates/clave/src/hook.rs`**

| Test | Asserts |
|---|---|
| `only_user_prompt_submit_moves_the_order` | drive `Stop`, `StopFailure`, `SessionEnd`, `Notification`, `PermissionRequest`, `PreToolUse` against a bound agent — `tab_order` and `commit_ord` byte-identical afterwards, while `status` does change. **This is the executable form of the §2 table and of the maintainer's ruling.** |
| `prompt_stamps_tab_and_agent_with_the_same_ordinal` | one prompt writes the same value to `tab_order[bind]` and `agent.commit_ord`, and that value equals the write's `seq` |
| `unbound_agent_prompt_still_records_its_ordinal` | `tab_id == None`: `commit_ord` set, `tab_order` untouched (the RC-B case) |
| `two_prompts_in_the_same_wall_second_get_distinct_ordinals` | **the §1.1 regression test** — `apply_hook_event(..., now = 1000)` twice for two agents, assert strict ordering |

**`crates/clave/src/lsview.rs`**

| Test | Asserts |
|---|---|
| `ls_orders_by_commitment_ordinal_tab_first` | a bound live agent with a high `tab_order` outranks a dormant agent with a higher `last_interacted` — i.e. `clave ls` agrees with the bar |

**`crates/clave-bar/src/model.rs`**

| Test | Asserts |
|---|---|
| `close_does_not_reorder_neighbours` | **the §1.2 regression test**, driven end to end through the model: `apply_tabs` + `apply_snapshot` → record `rows()`; then `apply_tabs` without the closed tab and `apply_snapshot` with the pruned store state → every surviving row keeps its index and the closed tab's agent occupies the closed tab's old index as `Dormant` |
| `close_holds_position_before_the_prune_lands` | the render-side carry: drop the tab from `apply_tabs` but push **no** new snapshot — the dormant row must already be at the old index |
| `touch_only_tab_holds_its_place_on_close` | `commit_ord == 0`, tab ordinal high: the row does not plunge |
| `dormant_row_never_reads_a_recycled_tabs_ordinal` | tab id reused by a live tab ⇒ the old agent is not dormant at all (`is_dormant` guard), so no row can read the fresh ordinal |

### 5.2 Tier 1 — tests that MUST change (stated intentional decisions)

These pin current behaviour on purpose. Changing them is a decision, recorded
here and to be restated in the PR dossier.

| Test | file:line | Change | Intentional because |
|---|---|---|---|
| `rows_order_by_last_user_commitment` | `model.rs:1218` | **mechanical only** — `snap_t` values become ordinals; every assertion (`b, c, a`; focus doesn't reorder; `last_interacted` alone must not sort) stays byte-identical. Comment "order by wall clock" → "order by commitment ordinal" | the behaviour it pins is exactly what we are keeping; only the units change |
| `dormant_rows_sort_into_the_unified_recency_order` | `model.rs:2112` | **behavioural** — dormant keys move from `last_interacted` to `commit_ord`; the asserted output order is unchanged | the *unified merged list* is load-bearing for R2 (§3.3). A dormant row must be able to outrank a live one, or "hold your place on close" is unimplementable. The test's shape is right; its key is not |
| `prop_rows_deterministic_and_recency_desc` | `model.rs:2918` | **behavioural and strengthened** — `ts_of` maps `Dormant → commit_ord` (and the carry); the `>=` window assertion is joined by the new uniqueness property below | the old property could not distinguish "correctly ordered" from "tied and arbitrarily broken", which is precisely defect §1.1 |
| `prompt_stamps_bound_tabs_timeline_atomically` | `hook.rs:321` | asserted values change from `1700` (the `now` argument) to the minted ordinal; the `Stop` leg at `:357-359` becomes the seed of `only_user_prompt_submit_moves_the_order` | the stamp is no longer a timestamp |
| `touch_stamps_timeline_bumps_seq_and_never_regresses` | `store.rs:609` | rewritten — no `now` argument exists; "a late/duplicate OLDER stamp can't regress it" becomes "the mint is monotone by construction" | the max-merge defended against a pre-lock clock read that no longer happens (§4.2 step 7) |
| `prune_tabs_removes_listed_stale_ids_order_safe_and_change_gated` | `store.rs:516` | extended with the carry assertions | new write in the same function |
| `snapshot_mirrors_store_rows` | `store.rs:447` | field rename + `commit_ord` projection | wire change |
| `clear_session_state_wipes_timeline_and_binds` / `clear_tab_timeline_wipes_session_scoped_ids` | `store.rs:590`, `:654` | renamed; extended with "and does NOT clear `commit_ord`" | §3.1 |
| `snapshot_carries_tab_timeline_and_defaults_empty` | `clave-types/src/lib.rs:254` | renamed to `…tab_order…`; the "pre-field payloads must still parse" leg is **kept and is now the mixed-version compat test** — add a case feeding a payload carrying the *old* `tab_timeline` key and assert it is ignored, not misread | §3.6 |
| `ls_sorts_by_recency_desc_and_shows_glyph` | `lsview.rs:51` | extended for the ordinal key | §4.5 |
| `merge_resume_preserves_existing_row_and_resets_status` | `add.rs:890` | add `commit_ord` to the preserved-fields assertion | new field |

### 5.3 New proptests

`clave-bar` already has `proptest` as a dev-dep. `clave` does **not** — add it
(host-only dev-dep, one line, same justification as `clave-bar`'s manifest
comment). To keep the store properties I/O-free, factor the pure halves out of
the locked closures exactly as `apply_hook_event` already was
(`hook.rs:221-223`): `touch_in(&mut Store, tab_id) -> u64` and
`prune_in(&mut Store, &[usize]) -> bool`, with `apply_touch` / `apply_prune_tabs`
becoming thin `with_store_mut` wrappers.

| Proptest | Crate | Property |
|---|---|---|
| `prop_ordinals_are_a_total_order` | `clave` | over an arbitrary sequence of ops (`touch_in`, `prune_in`, `apply_hook_event` with arbitrary events, `clear_session_order`'s pure half) on a plain `Store`: every ordinal ever minted is distinct and strictly increasing, and every value in `tab_order` and every non-zero `commit_ord` is ≤ `seq` |
| `prop_only_prompts_change_the_order` | `clave` | for any event sequence containing no `UserPromptSubmit`, the pair (`tab_order`, `{uuid → commit_ord}`) is unchanged — the §2 table as an invariant, not a table of examples |
| `prop_rows_no_duplicate_nonzero_ordinal` | `clave-bar` | for arbitrary tabs/agents/ordinals, no two rendered rows share a non-zero key. Encodes §3.2 honestly: the `0` class is excluded, and the RC-A eviction shape is excluded by construction (an agent's `tab_id` is never generated as a live tab id when the agent is also dormant) |
| `prop_close_preserves_relative_order` | `clave-bar` | **the headline property.** For arbitrary tabs, agents, ordinals and a chosen tab to close: after `apply_tabs`(without it) + `apply_snapshot`(post-prune), the sequence of surviving row keys equals the pre-close sequence with the closed tab's key replaced in place by its agent's `Dormant` key. Run it twice — once with the prune applied, once without (render-side carry only) — so both legs of §3.3 are covered |
| `prop_rows_deterministic_and_recency_desc` | `clave-bar` | existing, retargeted (§5.2) |

`prop_focus_never_reorders` (`model.rs:2882`) needs only the field rename and
should be extended: a `Stop`/`SessionEnd`-shaped snapshot (status changes, no
ordinal change) must also leave `rows()` order identical.

### 5.4 Tier 2

Does not exist (#47, blocked on #44). The cross-process seam here — the prune's
arrival order versus a fresh bind, and the mixed-version wire degradation — is
covered by the written argument in the PR plus adversarial review, per
`TESTING.md:78`. The argument must contain, at minimum: why `insert` is a max
(the mint is inside the flock); why the carry is idempotent and commutes; why the
render-side carry cannot diverge across instances (single snapshot source); and
the mixed-version table from §3.6.

### 5.5 Gate

```bash
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

---

## 6. Live validation

**Contract.** The maintainer runs every step; the driving agent prints commands
and never executes them against a live session. Only the dossier's read-only
diagnostics are used — nothing here launches, kills, reloads or pipes.

Throughout, "the sidebar order" means the rows top-to-bottom in the bar pane, and
"the expected order" means the output of `clave ls` (§4.5 makes it the oracle).
`clave ls` lists **agents only** — plain terminal tabs appear in the sidebar and
not in `ls`; ignore them when comparing, except where a step says otherwise.

### Step 0 — pre-flight (issue #44 is unfixed; skip this and every reading below is suspect)

**(a) Run:**
```bash
command -v clave && clave --version
grep -n 'clave-bar' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -5
```

**(b) Look at:** the version string from `clave --version` versus the version in
the most recent `clave-bar: loaded vX.Y.Z` log line.

**(c) Report:** both version strings verbatim.

| Report | Conclusion | Next |
|---|---|---|
| the two versions match | the fleet is coherent | go to Step 1 |
| they differ | **#44/#43** — the plugin is shelling out to a different binary than the one you installed | **stop.** No observation below can be trusted. Report and abandon the run |
| no `clave-bar: loaded` line today | the log is stale or filtered wrong | re-run with `tail -50`; if still nothing, report and stop |

### Step 1 — baseline

**(a) Run:**
```bash
clave ls
clave ls --json | jq '{seq, tab_order,
  agents: [.agents[] | {label, status, tab_id, commit_ord, last_interacted}]}'
```

**(b) Look at:** the sidebar, and whether its agent rows appear in the same
order as `clave ls`.

**(c) Report:** the sidebar rows top-to-bottom (labels are enough, plus which
have a glyph), and the JSON.

| Report | Conclusion | Next |
|---|---|---|
| orders agree | baseline good | Step 2 |
| orders disagree, and a disagreeing agent has `tab_id` naming a tab that hosts a *different* agent | **RC-A / S0 leaking in** — not an S1 defect | record and stop; S1 cannot be judged until S0 lands |
| orders disagree, and some live agent is missing from `tab_order` | **RC-B / S0** — the birth touch never landed | record and stop |
| orders disagree with neither signature | **new S1 defect** | report the full JSON and the sidebar; this is the interesting failure |

### Step 2 — the same-second tie (the §1.1 defect)

**(a) Do:** pick two agent tabs, call them **A** (higher in the sidebar) and **B**
(lower). Switch to **B**, type a one-word prompt and press Enter. Immediately —
inside one second, do not read anything in between — switch to **A**, type a
one-word prompt and press Enter.

**(b) Look at:** the sidebar immediately after the second Enter.

**(c) Run and report:**
```bash
clave ls --json | jq '{tab_order,
  agents: [.agents[] | {label, tab_id, commit_ord, last_interacted}]}'
```
plus the sidebar order.

| Report | Conclusion | Next |
|---|---|---|
| **A** is row 0, **B** is row 1, and A's `commit_ord` > B's `commit_ord`, and the two `last_interacted` values are **equal** | **fixed, and the tie was genuinely exercised** — equal seconds, distinct ordinals | Step 3 |
| A is row 0 and B row 1, but the two `last_interacted` differ | correct, but the second-boundary was crossed — **the tie was not exercised** | repeat Step 2 faster; if it never ties in 3 attempts, accept and note it |
| B is above A while B's `commit_ord` < A's | the store is right and the **render** is wrong | report — comparator bug in `rows()` |
| A and B have **equal** `commit_ord` | the **mint** is wrong — two commitments shared an ordinal | report immediately; this breaks the total-order invariant |
| neither row moved at all | the prompt never reached the store | check `tab_id` is non-null for both in the JSON; if null → **RC-B/S0**, not S1 |

### Step 3 — "claude finishing does not move it up"

**(a) Do:** with A at row 0 from Step 2, give **B** a prompt that takes a while,
then immediately prompt **A** again so A is at row 0 and B is still working.
Wait for B's glyph to turn green (`Done`).

**(b) Look at:** the sidebar at the moment B's glyph changes.

**(c) Report:** whether any row changed index when B finished, and B's row index
before and after.

| Report | Conclusion | Next |
|---|---|---|
| nothing moved; only B's glyph changed colour | **rule 2 of §2 holds** | Step 4 |
| B moved up on finishing | `Stop` is stamping — a regression against the ruling | report; check `only_user_prompt_submit_moves_the_order` was actually run |
| an unrelated row moved | ordinals collided or a stale snapshot was applied | capture the JSON from Step 1's command and report |

### Step 4 — closing a tab with a same-second neighbour (the §1.2 defect)

**(a) Do:** create the tie first — pick two **adjacent** sidebar rows and prompt
both inside one second, as in Step 2. Note the full sidebar order. Then focus the
**upper** of the two and press `Alt+w`.

**(b) Look at:** the sidebar immediately after the close, then again ~2 seconds
later (the prune is a fire-and-forget subprocess; both readings matter).

**(c) Report:** the sidebar order before, immediately after, and 2 s after; and
```bash
clave ls --json | jq '{tab_order,
  agents: [.agents[] | {label, tab_id, commit_ord}]}'
```

| Report | Conclusion | Next |
|---|---|---|
| every surviving row kept its index; the closed tab's row is at the **same index**, now `◌` | **R2 holds, both legs** (render-side carry and store carry) | Step 5 |
| correct immediately, wrong 2 s later | the **store carry** is wrong — `apply_prune_tabs` lowered `commit_ord` | report the JSON; look for `commit_ord` below the old `tab_order` value |
| wrong immediately, correct 2 s later | the **render-side carry** is not firing — `dormant_ord` is not reading `tab_order[a.tab_id]` | report |
| a neighbour jumped to the top in both readings | the fix did not take effect at all | confirm Step 0's versions again, then report |
| the closed tab's row **vanished** rather than going `◌` | not an ordering defect — the agent row is gone from the store | report; this is S3/RC-E territory |

### Step 5 — the touch-only close (the aggravator)

**(a) Do:** `Alt+a` to add an agent (or `clave add` from a pane) so a new tab
appears at row 0. **Do not prompt it.** Confirm it is at row 0, note the row
below it, then `Alt+w` its tab.

**(b) Look at:** where the new agent's row lands.

**(c) Report:** its index before and after, and its `commit_ord` and
`last_interacted` from the Step 1 JSON command.

| Report | Conclusion | Next |
|---|---|---|
| it holds row 0 as `◌`; nothing below it moved | **the aggravator is fixed** — the ordinal was carried, not the (zero) clock | done |
| it fell to the bottom of the list | the carry did not happen for a never-prompted agent; check `commit_ord == 0` in the JSON | report — either `add` did not mint (§4.6) or the prune carry did not run |
| it holds position but `commit_ord` is 0 | the render-side carry is doing all the work and the store carry is missing | report; it will regress on the next snapshot that drops `tab_order` |

### Step 6 — restart coherence

**(a) Do:** close the session and relaunch it the way you normally do. Before
relaunching, run:
```bash
jq '{seq, tab_order, agents: [.agents[] | {label, commit_ord, last_interacted}]}' \
  "$HOME/.local/state/clave/agents.json"
```
and again after the session is up.

**(b) Look at:** the dormant row order in the fresh sidebar.

**(c) Report:** both JSON dumps and the sidebar order.

| Report | Conclusion | Next |
|---|---|---|
| dormant rows are in the same relative order as before the restart; `tab_order` is empty in the post-launch dump before any tab is used | **§3.1 restart behaviour holds** | done |
| dormant rows are scrambled and every `commit_ord` is 0 | the **backfill** did not run (§4.2 step 9) — expected exactly once if this is the first launch on the new binary, a bug on any later launch | report which launch it was |
| dormant rows are scrambled but `commit_ord` values differ | the comparator is not reading `commit_ord` | report |
| the eager tab sits **below** the dormant rows and stays there | its birth touch never landed — **RC-B / S0**, not S1 | report; expected to be impossible once S0 has landed |

---

## 7. Risks, dependencies and out of scope

### Dependency on S0 — hard, and stated

**S1 must land after S0.** An unreliable bind masks every ordering fix: if
`clave bind` names the wrong tab, the prompt stamp lands on a stranger's row and
no comparator can save it (dossier RC-A). Every live-validation step above has a
branch that bails to "this is S0, stop" precisely because the two are not
separable by observation.

What this spec **assumes S0 delivers**:

1. `is_active_instance()` and `own_tab_id()` (`clave-bar/src/main.rs:43-71`) join
   pane→tab **within one coherent frame**, so the executor election cannot bind a
   stale pane to a fresh tab.
2. `apply_bind` therefore does not evict a live tenant, so
   `agent.tab_id` names the tab the agent is actually in — which is what makes
   `tab_order[bind]` the right stamp target and what removes the residual tie in
   §3.2 case 2.
3. The birth touch reliably lands for every tab, including the eager cold-start
   tab (RC-B) — which is what keeps ordinal `0` a genuinely rare state and lets
   §3.2's "unstamped live tabs sort at the bottom" stay a non-issue in practice.
4. `sent_binds` (`model.rs:197,305,429,431`) can re-arm, so a mis-bind is
   correctable within the life of a plugin instance.

If S0 lands with a different seam than expected, only §4.7's assumption that
`agent.tab_id` is trustworthy needs re-checking; nothing in the key design
depends on *how* S0 fixes the frame join.

### Sequencing with S3

S1 and S3 both edit `model.rs` ordering / `apply_tabs` and `store.rs`'s prune —
**not parallelisable** (dossier "Workstream split"). S1 first: S3's close-path
work should inherit an ordering that is already close-safe rather than
re-deriving it. S1 deliberately does **not** depend on prune reliability (the
render-side carry, §3.3), so S3 can change prune emission freely afterwards.

### Risks

| Risk | Severity | Mitigation |
|---|---|---|
| **Shared counter conflation** — someone compares an ordinal to a snapshot `seq`, or adds a `seq` bump beside a `mint_ord` call | high (silent order corruption) | one mint site (`Store::mint_ord`), documented; `prop_ordinals_are_a_total_order` asserts every ordinal ≤ `seq`; the doc comment states the rule explicitly |
| **Mechanical rename misses a site** | low | `cargo build` catches every one — both fields are renamed, not aliased, so there is no path where old code silently compiles against a default |
| **Upgrade scrambles the dormant list once** | medium (visible to the maintainer immediately) | the backfill (§4.2 step 9) + live Step 6 |
| **Mixed-version fleet renders positionally** | medium | degrades to a deterministic, explainable order, never to garbage; §3.6 table; fully mitigated by #44 |
| **`clave ls` order change is user-visible** | low | it makes a false doc comment true; separable into its own commit if reviewed against |
| **Prune-carry inside the recycled-id race** (RC-E) | low | strictly better than today: the fresh tenant's ordinal is preserved onto the agent instead of discarded. The unbind itself is unchanged and remains S3's |
| **`AgentRecord` gains a field ⇒ 12 literal sites** | low | compile-time; listed in §4.2 step 5 |

### Out of scope

- **Plain terminal tab interaction** (RC-D) — S2. This spec only guarantees the
  key can accept a terminal stamp without redesign (§3.5).
- **Frame coherence, the executor election, the eager tab's bind** (RC-A, RC-B)
  — S0.
- **Prune reliability, tab-id reuse, `ReanchorVisit`'s no-retry** (RC-E) — S3.
- **Labels** (RC-F) — S4. **Per-repo colour** (RC-G) — S5.
- **Sub-second wall-clock precision anywhere.** `last_interacted` stays
  second-resolution; `eager_row`'s accepted tie (`setup.rs:525-531`) stays
  accepted — it picks between two equally-recent rows one keystroke apart, and
  §3.1's reasoning does not apply to it.
- **Any change to focus, click or nav semantics.** Rows 7–10 of the §2 table are
  reproduced only so the table is complete.
