# Frecency ordering — spec

Settled in the 2026-08-19 grilling session with Ollie. Replaces the
commitment-ordinal recency ordering as the DEFAULT sidebar row order;
recency stays switchable (`clave order recency`) until its own bug round
(TabUpdate starvation / C4, terminal commitments) lands separately.

## The problem

Pure recency ordering is jumpy and inconsistent (C4), and buries the
most-invested threads under whatever was touched last. Pure investment
ordering (total turn count) buries new sessions and throwaway terminals.
This is the LRU-vs-LFU tension; the established resolution is **frecency**:
every interaction contributes weight that decays exponentially with age,
rank by the sum.

## The design

**Commitment** keeps its existing meaning (UserPromptSubmit; a row's
creation; a tab's birth touch). Every commitment now writes two numbers:
the existing ordinal (`tab_order` / `commit_ord`), and **+1 in a day
bucket**.

- **Day buckets**: `BTreeMap<u32 /*unix day*/, u32 /*count*/>` — on the
  `AgentRecord` (uuid-keyed, survives `/clear` and dormancy) AND on a new
  `Store.tab_buckets: BTreeMap<usize, BTreeMap<u32, u32>>` (tab-keyed twin,
  session-scoped like `tab_order`, covers terminal tabs and the pre-bind
  window). Same doubled bookkeeping as `commit_ord`/`tab_order`, same
  max-merge read, for the same R2 reason: a row must rank by the same
  number live or dormant.
- **Score** = Σ over buckets of `count × 0.5^((today − day) × 24 / half_life_hours)`.
  Computed in the bar at render time. `today` is stamped by the host into
  every snapshot (`AgentSnapshot.today`) — the wasm bar never reads a
  clock.
- **Retention**: buckets older than 7 days are pruned on every write.
  Fixed, not configurable — at the default half-life a 7-day-old point is
  worth <1% anyway, and at half-life→∞ the pruning IS the original
  "rolling 7-day investment window" design.
- **The dial**: `OrderMode::Frecency { half_life_hours }`, default 24.
  half-life→0 behaves like pure recency; →∞ like a 7-day rolling
  investment count. `OrderMode::Recency` keeps the ordinal comparator
  exactly as shipped. Mode is semi-persistent store state
  (`Store.order`, rides every snapshot — the `collapsed` doctrine),
  toggled by `clave order recency | frecency [HOURS]`.

## Newborn initialisation (cold start)

A new row inherits the **opener's** buckets — an exact copy, no scale
factor. Identical buckets give an identical score, and the existing
position tiebreak (newer tabs have higher zellij positions) puts the
newborn DIRECTLY BELOW its opener until real commitments diverge them.
This is a contextual prior (empirical-Bayes cold start) + Chrome's
insert-adjacent-to-opener, in one bucket copy.

**Wake initialisation (ruled 2026-08-19, from the PR #218 live drive):**
inheritance is for NEW rows only. Waking an existing dormant agent
re-enters it at its **own decayed score** — the frecency computation
simply continues as if the row had zero interactions while dormant, so
an accidentally closed tab woken immediately returns to the rank it
held. A row dormant past the retention window has fully decayed and
wakes at the bottom (zero-score ordinal floor). Mechanically: an
agent-bound tab's twin never holds inherited buckets — `touch_in` seeds
empty when the tab is bound, `apply_bind` clears the twin, covering
both orders of the birth-touch/bind race. The retention window
(`clave_types::BUCKET_RETAIN_DAYS`) is also a **hard scoring cutoff at
every dial**: the store prunes lazily (only on a row's next bump), so
the bar zeroes out-of-window buckets rather than letting a 999h dial
resurrect them — and skips their computation entirely, keeping
long-dormant fleets free per frame.

**Deviation from the grilled agreement, needs Ollie's ratification:**
the agreement said opener = "the tab focused when the new tab was
created". Focus is not in the store (the visited beacon is a
plugin-to-plugin pipe; persisting it would add a store write + snapshot
push per tab switch, against the "focus never reorders" doctrine). v1
uses the nearest store-native proxy: **opener = the tab holding the max
`tab_order` ordinal** — the most-recently-COMMITTED tab. These coincide
whenever you add from the thread you were just working in; they diverge
if you focus a tab and add without prompting first. Self-heals in hours
either way. Upgrade path if dog-fooding says it matters: persist the
visited beacon.

Seeding sites (one rule, `store::opener_buckets`):
- `clave add` seeds the fresh `AgentRecord.buckets` (inside the same
  locked write that mints the row). `merge_resume_record` preserves an
  existing row's real buckets — resume never overwrites history.
- birth touch (`touch_in`) seeds `tab_buckets[tab_id]` if vacant —
  covers terminal tabs and add's own tab. Copy only, no +1: the tie is
  the adjacency mechanism.

## The comparator

`rank_desc` keeps its shape (primary desc, tiebreak asc); the primary key
widens from `u64` to `(u64, u64)`:

- Recency mode: `(ordinal, 0)` — behaviour bit-identical to today.
- Frecency mode: score in millipoints `(score × 1000) as u64`; key is
  `(millipoints, 0)` when > 0, else `(0, ordinal)`.

The zero fallback is load-bearing three ways: (1) upgrade day — existing
stores have no buckets, so the fleet keeps its current ordinal order
instead of collapsing to tab position, then frecency takes over as points
accrue; (2) dormant rows never prompted again keep their ordinal order
forever instead of decaying into uuid order; (3) the existing bar test
suite's ordering expectations (ordinals only, no buckets) remain valid
under the frecency default.

Live/dormant block segregation, `rows()` structure, and "ONE comparator
applied twice" (PR #135) are untouched.

## Out of scope (the separate recency-bug task)

- Terminal tabs earning commitments per command run (the InputReceived
  replacement) — until then terminals hold birth-inherited buckets only.
- C4 / TabUpdate-starvation diagnosis.
- Backfilling buckets from transcripts: forbidden (S4, #98, 64KiB tail).

## Wire changes (all `#[serde(default)]`, pre-field payloads keep parsing)

- `Agent.buckets: BTreeMap<u32, u32>`
- `AgentSnapshot.order: OrderMode`, `.today: u32`,
  `.tab_buckets: BTreeMap<usize, BTreeMap<u32, u32>>`
- `AgentRecord.buckets`, `Store.tab_buckets`, `Store.order`
