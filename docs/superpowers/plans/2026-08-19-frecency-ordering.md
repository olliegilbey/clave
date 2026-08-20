# Frecency Ordering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Historical record.** This plan was executed; its task bodies predate the
> 2026-08-19 maintainer ruling that made `BUCKET_RETAIN_DAYS` a hard scoring
> cutoff (Task 6's sketch and expected values still count out-of-window
> buckets). Where this file and the shipped code differ, the code and
> `docs/superpowers/specs/2026-08-19-frecency-ordering.md` are authoritative.

**Goal:** Sidebar rows rank by a decayed-commitment (frecency) score by default, switchable back to the shipped ordinal-recency ordering via `clave order`.

**Architecture:** Every commitment (already: UserPromptSubmit, row creation, birth touch) additionally increments a per-day bucket count; buckets live on the `AgentRecord` (uuid-keyed) and in a tab-keyed twin map, mirroring the existing `commit_ord`/`tab_order` doubled bookkeeping. The bar computes `Σ count × 0.5^(age_days × 24 / half_life_hours)` at render time from a host-stamped `today`, and feeds it through the existing two-block, one-comparator `rows()` pipeline with a widened primary key. Newborn rows cold-start by copying the opener's buckets (opener := tab with max `tab_order` ordinal).

**Tech Stack:** Rust workspace; `crates/clave-types` (wire), `crates/clave` (host, serde_json store), `crates/clave-bar` (wasm32-wasip1 plugin, pure `model.rs`).

**Spec:** `docs/superpowers/specs/2026-08-19-frecency-ordering.md`

## Global Constraints

- All new serde fields carry `#[serde(default)]` — a pre-field store file or pipe payload must keep parsing (repo-wide doctrine, see any existing field comment).
- The four gates must pass: `cargo fmt --all --check`, `cargo test --workspace`, `cargo build -p clave-bar --target wasm32-wasip1`, `cargo clippy --workspace --all-targets -- -D warnings` (or `just gates`).
- `rows()` keeps ONE comparator applied to both blocks separately (PR #135 doctrine) — never give live/dormant different rules.
- Never compare an ordinal to a snapshot `seq` (S1 §3.4).
- The bar never reads a wall clock — `today` arrives in the snapshot.
- Bucket retention is fixed: `BUCKET_RETAIN_DAYS = 7`.
- The wasm bar cannot use `std::time`, file I/O, or host-only deps.
- Commit after each green task; message style follows repo history (sentence-style, what-changed-and-why).

---

### Task 1: Wire vocabulary (`clave-types`)

**Files:**
- Modify: `crates/clave-types/src/lib.rs` (Agent struct ~line 89-193, AgentSnapshot ~line 198-223)

**Interfaces:**
- Produces: `pub enum OrderMode { Recency, Frecency { half_life_hours: u32 } }` with `Default = Frecency { half_life_hours: 24 }`; `Agent.buckets: BTreeMap<u32, u32>`; `AgentSnapshot.order: OrderMode`, `.today: u32`, `.tab_buckets: BTreeMap<usize, BTreeMap<u32, u32>>`. All later tasks use these exact names.

- [ ] **Step 1: Write the failing tests** (in `clave-types`'s existing test module, or a new `#[cfg(test)] mod` at the bottom)

```rust
#[test]
fn order_mode_defaults_to_frecency_24h() {
    assert_eq!(
        OrderMode::default(),
        OrderMode::Frecency { half_life_hours: 24 }
    );
}

/// Pre-field payloads must keep parsing — the repo-wide `serde(default)`
/// doctrine, pinned here for the four new fields at once.
#[test]
fn pre_frecency_snapshot_payload_still_parses() {
    let old = r#"{"seq":1,"agents":[]}"#;
    let snap: AgentSnapshot = serde_json::from_str(old).unwrap();
    assert_eq!(snap.order, OrderMode::default());
    assert_eq!(snap.today, 0);
    assert!(snap.tab_buckets.is_empty());
}

#[test]
fn order_mode_round_trips_both_variants() {
    for m in [OrderMode::Recency, OrderMode::Frecency { half_life_hours: 6 }] {
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<OrderMode>(&json).unwrap(), m);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave-types`
Expected: FAIL to compile — `OrderMode` not defined.

- [ ] **Step 3: Implement**

Add above the `Agent` struct:

```rust
/// How the bar ranks rows (2026-08-19 spec). Semi-persistent store state
/// riding every snapshot — the `collapsed` doctrine: one store writer,
/// instances hydrate at birth and heal on every push.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderMode {
    /// The shipped S1 ordering: commitment ordinal descending.
    Recency,
    /// Decayed-commitment score: Σ count × 0.5^(age_days × 24 / half_life).
    /// half-life → 0 behaves like Recency; → ∞ like a 7-day rolling
    /// investment count (buckets are pruned at 7 days regardless).
    Frecency { half_life_hours: u32 },
}

impl Default for OrderMode {
    fn default() -> Self {
        OrderMode::Frecency { half_life_hours: 24 }
    }
}
```

Add to `Agent` (after `context_level`):

```rust
    /// Commitment day-buckets: unix day → count of user commitments that
    /// day. The frecency numerator; written by the hook on UserPromptSubmit,
    /// seeded at birth from the opener (spec: newborn initialisation),
    /// pruned past 7 days. `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub buckets: std::collections::BTreeMap<u32, u32>,
```

Add to `AgentSnapshot` (after `collapsed`):

```rust
    /// Row-ordering mode + dial (2026-08-19 spec). Store state like
    /// `collapsed` above, same doctrine. `default` keeps pre-field
    /// payloads parseable.
    #[serde(default)]
    pub order: OrderMode,
    /// Unix DAY at projection time, stamped by the host — the bar never
    /// reads a clock (wasm). Frecency ages every bucket against this.
    /// `default` (0) makes all ages huge → scores ~0 → the ordinal
    /// fallback carries, which is exactly the pre-snapshot cold state.
    #[serde(default)]
    pub today: u32,
    /// tab_id → commitment day-buckets: the tab-keyed twin of
    /// `Agent::buckets`, exactly as `tab_order` twins `commit_ord` —
    /// covers terminal tabs and the pre-bind window; session-scoped and
    /// pruned with `tab_order`. `default` keeps pre-field payloads
    /// parseable.
    #[serde(default)]
    pub tab_buckets: std::collections::BTreeMap<usize, std::collections::BTreeMap<u32, u32>>,
```

- [ ] **Step 4: Fix struct-literal fallout in this crate only, then run**

Run: `cargo test -p clave-types`
Expected: PASS. (Other crates now fail to compile — that is Tasks 2-6's work; do not fix them here beyond what `-p clave-types` needs.)

- [ ] **Step 5: Commit**

```bash
git add crates/clave-types/src/lib.rs
git commit -m "Wire vocabulary for frecency ordering: OrderMode, day buckets, host-stamped today"
```

---

### Task 2: Store schema + helpers (`crates/clave/src/store.rs`)

**Files:**
- Modify: `crates/clave/src/store.rs` (AgentRecord ~line 40, Store ~line 170, `touch_in` ~line 387, `snapshot_from` ~line 289, `apply_prune_tabs` ~line 500, `apply_collapse` ~line 553 as the model for `apply_order`, `clear_session_order` ~line 630)

**Interfaces:**
- Consumes: `clave_types::OrderMode` (Task 1).
- Produces: `AgentRecord.buckets: BTreeMap<u32, u32>`; `Store.tab_buckets: BTreeMap<usize, BTreeMap<u32, u32>>`; `Store.order: OrderMode`; `pub const BUCKET_RETAIN_DAYS: u32 = 7`; `pub fn unix_day(unix_secs: u64) -> u32`; `pub(crate) fn bump_bucket(map: &mut BTreeMap<u32, u32>, today: u32)`; `pub(crate) fn opener_buckets(s: &Store) -> BTreeMap<u32, u32>`; `pub fn apply_order(paths: &StorePaths, mode: OrderMode) -> Result<AgentSnapshot>`. Tasks 3-5 call these exact names.

- [ ] **Step 1: Write the failing tests** (in store.rs's existing test module; use the module's existing `rec()`/temp-paths helpers)

```rust
#[test]
fn bump_bucket_increments_today_and_prunes_past_retention() {
    let mut m: BTreeMap<u32, u32> = [(100, 3), (93, 9)].into();
    bump_bucket(&mut m, 100);
    assert_eq!(m.get(&100), Some(&4));
    assert!(!m.contains_key(&93)); // 100 - 7 = 93 is out of retention
}

#[test]
fn opener_buckets_prefers_the_max_ordinal_tabs_agent() {
    let mut s = Store::default();
    let mut a = rec("u1");
    a.tab_id = Some(7);
    a.buckets = [(100, 5)].into();
    s.agents.insert("u1".into(), a);
    s.tab_order = [(7, 50), (9, 40)].into();
    s.tab_buckets.insert(9, [(100, 2)].into());
    // tab 7 holds the max ordinal and hosts u1 → u1's buckets win.
    assert_eq!(opener_buckets(&s), [(100u32, 5u32)].into());
}

#[test]
fn opener_buckets_falls_back_to_tab_buckets_then_empty() {
    let mut s = Store::default();
    s.tab_order = [(9, 40)].into();
    s.tab_buckets.insert(9, [(100, 2)].into());
    assert_eq!(opener_buckets(&s), [(100u32, 2u32)].into());
    assert!(opener_buckets(&Store::default()).is_empty());
}

#[test]
fn birth_touch_seeds_tab_buckets_from_the_opener_copy_only() {
    let mut s = Store::default();
    let mut a = rec("u1");
    a.tab_id = Some(7);
    a.buckets = [(100, 5)].into();
    s.agents.insert("u1".into(), a);
    s.tab_order = [(7, 50)].into();
    touch_in(&mut s, 11);
    // EXACT copy — no +1. The tie IS the adjacency mechanism (spec).
    assert_eq!(s.tab_buckets.get(&11), Some(&[(100u32, 5u32)].into()));
    // A second touch on a seeded tab never re-seeds.
    s.agents.get_mut("u1").unwrap().buckets = [(101, 9)].into();
    touch_in(&mut s, 11);
    assert_eq!(s.tab_buckets.get(&11), Some(&[(100u32, 5u32)].into()));
}

#[test]
fn apply_order_persists_and_snapshots_the_mode() {
    let (_tmp, p) = paths(); // module's existing tempdir helper — reuse its real name
    let snap = apply_order(&p, OrderMode::Recency).unwrap();
    assert_eq!(snap.order, OrderMode::Recency);
    assert_eq!(read_store(&p).unwrap().order, OrderMode::Recency);
}

#[test]
fn snapshot_carries_buckets_tab_buckets_order_and_today() {
    let mut s = Store::default();
    let mut a = rec("u1");
    a.buckets = [(100, 5)].into();
    s.agents.insert("u1".into(), a);
    s.tab_buckets.insert(3, [(100, 1)].into());
    let snap = snapshot_from(&s);
    assert_eq!(snap.agents[0].buckets, [(100u32, 5u32)].into());
    assert_eq!(snap.tab_buckets.get(&3), Some(&[(100u32, 1u32)].into()));
    assert_eq!(snap.order, OrderMode::default());
    assert_eq!(snap.today, unix_day(now_unix())); // however the module names its now-helper
}

#[test]
fn prune_tabs_and_session_clear_drop_tab_buckets_with_tab_order() {
    // Extend the EXISTING apply_prune_tabs and clear_session_order tests'
    // arrange blocks with `s.tab_buckets.insert(<same ids>, [(100,1)].into())`
    // and assert the pruned/cleared ids are gone from tab_buckets too.
}
```

(Adjust helper names — `paths()`, `rec()`, the now-helper — to whatever the module actually defines; the assertions are the contract.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave 2>&1 | head -30`
Expected: compile failures — new fields/functions missing.

- [ ] **Step 3: Implement**

`AgentRecord` gains (after `live_session`):

```rust
    /// Commitment day-buckets (unix day → count) — the frecency numerator.
    /// Store twin of `clave_types::Agent::buckets`, which carries the
    /// rationale. Written on UserPromptSubmit; seeded at row creation from
    /// the opener; pruned past [`BUCKET_RETAIN_DAYS`] on every bump.
    #[serde(default)]
    pub buckets: BTreeMap<u32, u32>,
```

`Store` gains (after `collapsed`):

```rust
    /// tab_id → commitment day-buckets: the tab-keyed twin of the record's
    /// `buckets`, exactly as `tab_order` twins `commit_ord` — it covers
    /// terminal tabs and the pre-bind window, and the bar max-merges the
    /// two (R2: same rank live or dormant). Session-scoped like
    /// `tab_order`: cleared on session recreate, pruned with dead tabs.
    #[serde(default)]
    pub tab_buckets: BTreeMap<usize, BTreeMap<u32, u32>>,
    /// Row-ordering mode + dial — see `clave_types::OrderMode`. Store
    /// state under the `collapsed` doctrine: one writer, rides every push.
    #[serde(default)]
    pub order: clave_types::OrderMode,
```

Helpers (near `mint_ord`):

```rust
/// Buckets older than this contribute <1% at the default half-life; at
/// half-life → ∞ this pruning IS the spec's rolling 7-day window.
pub const BUCKET_RETAIN_DAYS: u32 = 7;

/// Unix seconds → unix day. The one place the day arithmetic lives.
pub fn unix_day(unix_secs: u64) -> u32 {
    (unix_secs / 86_400) as u32
}

/// +1 commitment today, and prune everything out of retention. Both maps
/// (record and tab twin) go through here, so retention cannot skew.
pub(crate) fn bump_bucket(map: &mut BTreeMap<u32, u32>, today: u32) {
    *map.entry(today).or_insert(0) += 1;
    map.retain(|day, _| *day + BUCKET_RETAIN_DAYS >= today);
}

/// The newborn-inheritance source (spec: newborn initialisation): the
/// buckets of the most-recently-COMMITTED tab — max `tab_order` ordinal,
/// preferring the agent bound to that tab over the tab twin. A store-native
/// proxy for "the tab focused at creation"; the two diverge only when the
/// user focuses a tab and adds without prompting first.
pub(crate) fn opener_buckets(s: &Store) -> BTreeMap<u32, u32> {
    let Some((&tab_id, _)) = s.tab_order.iter().max_by_key(|(_, ord)| **ord) else {
        return BTreeMap::new();
    };
    s.agents
        .values()
        .find(|r| r.tab_id == Some(tab_id))
        .map(|r| r.buckets.clone())
        .filter(|b| !b.is_empty())
        .or_else(|| s.tab_buckets.get(&tab_id).cloned())
        .unwrap_or_default()
}
```

`touch_in` (line ~387) gains the seed — copy only, on vacancy, computed BEFORE the mint so the newborn's own stamp can't shift the opener:

```rust
pub(crate) fn touch_in(s: &mut Store, tab_id: usize) -> u64 {
    if !s.tab_buckets.contains_key(&tab_id) {
        let inherited = opener_buckets(s);
        s.tab_buckets.insert(tab_id, inherited);
    }
    let ord = s.mint_ord();
    s.tab_order.insert(tab_id, ord);
    ord
}
```

`apply_order`, modeled on `apply_collapse` (line ~553) but unconditional (a mode set is always a push — it re-sorts every instance):

```rust
/// `clave order <mode>`: persist the ordering mode and push. Same shape
/// as apply_collapse; unconditional because the whole point of the write
/// is the fleet-wide re-sort.
pub fn apply_order(paths: &StorePaths, mode: clave_types::OrderMode) -> Result<AgentSnapshot> {
    with_store_mut(paths, |s| {
        s.order = mode;
        s.seq += 1; // monotonic pipe contract (§5)
        snapshot_from(s)
    })
}
```

`snapshot_from` (line ~289): project the new fields — in the per-agent projection add `buckets: r.buckets.clone(),` and in the snapshot literal add:

```rust
        tab_buckets: store.tab_buckets.clone(),
        order: store.order,
        today: unix_day(/* the module's existing now-source; if snapshot_from
            has no clock today, use SystemTime::now() via the same
            UNIX_EPOCH pattern the file already imports */),
```

`apply_prune_tabs` (line ~500): wherever stale ids are removed from `tab_order`, add `s.tab_buckets.remove(&id);`. `clear_session_order` (line ~630): wherever `tab_order` is cleared, add `s.tab_buckets.clear();`.

Fix `AgentRecord`/`Agent` struct-literal fallout across `crates/clave` (the `rec()` test helpers in store.rs/hook.rs/add.rs/open.rs and `add.rs:1041`'s `fresh` literal — add `buckets: BTreeMap::new(),`; `add.rs` gets its real value in Task 4).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p clave`
Expected: PASS, including the extended prune/clear tests.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/store.rs crates/clave/src/add.rs crates/clave/src/open.rs crates/clave/src/hook.rs
git commit -m "Store learns day buckets: schema, birth-touch inheritance, apply_order, projection"
```

---

### Task 3: Prompt commitments write buckets (`crates/clave/src/hook.rs`)

**Files:**
- Modify: `crates/clave/src/hook.rs` (the commitment block, ~lines 679-708)

**Interfaces:**
- Consumes: `store::{bump_bucket, unix_day}` (Task 2).
- Produces: on `UserPromptSubmit`, `rec.buckets[today] += 1` and `tab_buckets[bound tab][today] += 1`, both pruned.

- [ ] **Step 1: Write the failing test** (hook.rs test module, alongside the existing commitment-ordinal tests, reusing their arrange helpers)

```rust
/// A prompt is one commitment: +1 in the record's day bucket AND the
/// bound tab's twin — the same doubled bookkeeping as commit_ord/tab_order.
#[test]
fn a_prompt_buckets_one_commitment_on_record_and_bound_tab() {
    // Arrange exactly as the existing test that asserts
    // `rec.commit_ord` and `tab_order` after a UserPromptSubmit
    // (row bound to a tab, apply_hook_event with event "UserPromptSubmit").
    // Then additionally assert:
    let today = crate::store::unix_day(/* the `now` the arrange passed */);
    let rec = s.agents.get("u1").unwrap();
    assert_eq!(rec.buckets.get(&today), Some(&1));
    assert_eq!(
        s.tab_buckets.get(&TAB_ID).and_then(|m| m.get(&today)),
        Some(&1)
    );
}

/// Stop/Notification/SessionEnd are not commitments — no bucket moves.
#[test]
fn non_commitment_events_write_no_buckets() {
    // Same arrange, event "Stop": assert rec.buckets stays empty.
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave a_prompt_buckets`
Expected: FAIL — buckets empty.

- [ ] **Step 3: Implement**

In the commitment block (hook.rs ~695-707), extend both existing stamp sites:

```rust
    let ord = s.mint_ord();
    if commitment {
        let today = crate::store::unix_day(now);
        if let Some(tab_id) = commit_tab {
            s.tab_order.insert(tab_id, ord);
            crate::store::bump_bucket(s.tab_buckets.entry(tab_id).or_default(), today);
        }
        if let Some(rec) = s.agents.get_mut(uuid) {
            rec.commit_ord = ord;
            crate::store::bump_bucket(&mut rec.buckets, today);
        }
    }
```

(`now` is already in scope — it stamps `last_interacted` at line ~682.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p clave`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/hook.rs
git commit -m "A prompt banks a frecency point: hook buckets commitments beside the ordinal"
```

---

### Task 4: `clave add` seeds the newborn's buckets (`crates/clave/src/add.rs`)

**Files:**
- Modify: `crates/clave/src/add.rs` (the `fresh` record literal ~line 1041, inside the `with_store_mut` at ~1036; `merge_resume_record` ~line 485)

**Interfaces:**
- Consumes: `store::opener_buckets` (Task 2).
- Produces: a fresh row born with the opener's bucket copy; a resumed row keeping its own real buckets.

- [ ] **Step 1: Write the failing tests** (add.rs test module, reusing `rec()`)

```rust
/// Spec: newborn initialisation. A fresh row inherits the opener's
/// buckets — exact copy, so the tie + position tiebreak lands it
/// directly below its opener in frecency mode.
#[test]
fn a_fresh_row_inherits_the_openers_buckets() {
    let mut s = Store::default();
    let mut opener = rec("u-opener");
    opener.tab_id = Some(4);
    opener.buckets = [(100, 6)].into();
    s.agents.insert("u-opener".into(), opener);
    s.tab_order = [(4, 90)].into();
    // Drive the same record-mint path run_add uses (extract the
    // with_store_mut closure body into a testable fn if it isn't already —
    // mint_record(s, fresh_inputs...) — matching how sibling tests drive it).
    let merged = mint_record_under_test(&mut s, "u-new");
    assert_eq!(merged.buckets, [(100u32, 6u32)].into());
}

/// Resume must never overwrite earned history with an inherited copy.
#[test]
fn merge_resume_record_keeps_the_existing_rows_buckets() {
    let mut existing = rec("u1");
    existing.buckets = [(99, 42)].into();
    let mut fresh = rec("u1");
    fresh.buckets = [(100, 1)].into(); // whatever add would seed
    let merged = merge_resume_record(Some(&existing), fresh);
    assert_eq!(merged.buckets, [(99u32, 42u32)].into());
}
```

(If `merge_resume_record`'s existing contract is `..row.clone()` for the preserved side, the second test may already pass once the field exists — keep it as the pin either way.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave inherits_the_openers`
Expected: FAIL — fresh row has empty buckets.

- [ ] **Step 3: Implement**

In the `with_store_mut` closure (~line 1036), before the `fresh` literal:

```rust
        // Spec (2026-08-19): a newborn inherits the opener's buckets — an
        // exact copy, so identical scores + the position tiebreak put it
        // directly below its opener until real commitments diverge them.
        let inherited = crate::store::opener_buckets(s);
```

and in the literal replace the Task 2 placeholder with `buckets: inherited,`. Confirm `merge_resume_record` preserves the existing row's `buckets` via its `..row.clone()` arm (it should by construction; the test pins it).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p clave`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/add.rs
git commit -m "A newborn row starts life as its opener's echo: add seeds inherited buckets"
```

---

### Task 5: `clave order` CLI (`crates/clave/src/main.rs`)

**Files:**
- Modify: `crates/clave/src/main.rs` (Command enum ~line 32, dispatch ~line 248, parse-pin tests ~line 585+)

**Interfaces:**
- Consumes: `store::apply_order`, `hook::push_snapshot`.
- Produces: `clave order` (prints current mode), `clave order recency`, `clave order frecency`, `clave order frecency 12`.

- [ ] **Step 1: Write the failing parse-pin test** (the module's convention: every new CLI surface gets one — see `open_cli_parses_the_birth_mode`)

```rust
#[test]
fn order_cli_parses_mode_and_optional_half_life() {
    let bare = Cli::parse_from(["clave", "order"]);
    match bare.command {
        Some(Command::Order { mode: None, half_life: None }) => {}
        other => panic!("bare order misparsed: {other:?}"),
    }
    let full = Cli::parse_from(["clave", "order", "frecency", "12"]);
    match full.command {
        Some(Command::Order { mode: Some(m), half_life: Some(12) }) => {
            assert_eq!(m, "frecency")
        }
        other => panic!("full order misparsed: {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave order_cli_parses`
Expected: FAIL to compile — no `Command::Order`.

- [ ] **Step 3: Implement**

Command enum variant:

```rust
    /// Set (or print) the sidebar row-ordering mode.
    /// `clave order` prints; `clave order recency` | `clave order
    /// frecency [HALF_LIFE_HOURS]` set and push fleet-wide.
    Order {
        /// "recency" or "frecency"
        mode: Option<String>,
        /// Frecency half-life in hours (default 24). Dial: small ≈
        /// recency, huge ≈ 7-day rolling investment count.
        half_life: Option<u32>,
    },
```

Dispatch arm:

```rust
        Some(Command::Order { mode, half_life }) => {
            let paths = store::store_paths()?;
            let Some(mode) = mode else {
                println!("{:?}", store::read_store(&paths)?.order);
                return Ok(());
            };
            let mode = match mode.as_str() {
                "recency" => clave_types::OrderMode::Recency,
                "frecency" => clave_types::OrderMode::Frecency {
                    half_life_hours: half_life.unwrap_or(24),
                },
                other => anyhow::bail!("unknown order mode {other:?} (recency|frecency)"),
            };
            let snap = store::apply_order(&paths, mode)?;
            hook::push_snapshot(&snap);
            evlog::log_event("order", &format!("{mode:?}"));
            Ok(())
        }
```

(Match the surrounding arms' exact import style — the file may use `clave::store::` paths.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p clave`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/clave/src/main.rs
git commit -m "clave order: the ordering dial reaches the CLI"
```

---

### Task 6: The bar ranks by frecency (`crates/clave-bar/src/model.rs`)

**Files:**
- Modify: `crates/clave-bar/src/model.rs` (`rank_desc` ~line 366, model fields + `apply_snapshot` ~line 1228-1272, `live_ord`/`dormant_ord` ~line 777-805, `rows()` ~line 1915-1993, fixtures `agent()` ~2517 / `snap()` ~2576)

**Interfaces:**
- Consumes: `OrderMode`, `Agent.buckets`, snapshot `order`/`today`/`tab_buckets` (Task 1).
- Produces: `fn frecency_millis(buckets, today, half_life_hours) -> u64`; `fn live_key(&self, t: &TabMeta) -> (u64, u64)`; `fn dormant_key(&self, a: &Agent) -> (u64, u64)`; `rank_desc` over `((u64, u64), usize, (RowKey, Row))`.

- [ ] **Step 1: Write the failing tests** (model.rs test module, reusing `agent()`/`snap()`; extend `snap()` with `order`/`today`/`tab_buckets` defaults and add a `snap_at(today, ...)` variant or setters as the module style prefers)

```rust
/// The decay curve itself: today full weight, each day halves (24h
/// half-life), future-dated buckets clamp to age 0, empty map is 0.
#[test]
fn frecency_millis_decays_by_half_lives() {
    let b: BTreeMap<u32, u32> = [(100, 4), (99, 4), (93, 4)].into();
    // today=100, hl=24h: 4*1000 + 4*500 + 4*7.8125 = 6031 (floor)
    assert_eq!(frecency_millis(&b, 100, 24), 6031);
    assert_eq!(frecency_millis(&BTreeMap::new(), 100, 24), 0);
    let future: BTreeMap<u32, u32> = [(105, 2)].into();
    assert_eq!(frecency_millis(&future, 100, 24), 2000); // clamp, not panic
}

/// Frecency mode: more decayed weight ranks higher, regardless of who
/// committed last (the whole point vs recency).
#[test]
fn frecency_ranks_invested_rows_above_recent_one_offs() {
    // Arrange two live agent tabs via the module's snapshot/tab helpers:
    // "u-big" buckets [(100,10)], tab ordinal 5;
    // "u-latest" buckets [(100,1)], tab ordinal 9 (the newest commitment).
    // Snapshot order = Frecency{24}, today = 100.
    // Assert rows() lists u-big above u-latest.
}

/// The adjacency mechanism end-to-end: an exact bucket copy ties, and
/// the existing position tiebreak puts the newborn DIRECTLY BELOW its
/// opener.
#[test]
fn an_inherited_copy_sits_directly_below_its_opener() {
    // Two live tabs, same buckets [(100,6)] (agent on the opener tab,
    // tab_buckets twin on the newborn tab), opener position 2, newborn
    // position 5, plus a third tab with buckets [(100,9)] above both and
    // a fourth with [(100,1)] below both. Assert the exact row order.
}

/// Zero-score rows fall back to ordinal order — upgrade day and
/// never-touched dormant rows keep the shipped S1 ordering instead of
/// collapsing to tab position (spec: the comparator's zero fallback).
#[test]
fn zero_scores_fall_back_to_ordinal_order() {
    // Frecency mode, today=100, NO buckets anywhere; three live tabs
    // with tab_order ordinals 30/10/20. Assert rows() order matches the
    // ordinal-descending order, not position order.
}

/// Recency mode is bit-identical to the shipped behaviour.
#[test]
fn recency_mode_ranks_by_ordinal_exactly_as_before() {
    // Same arrange as frecency_ranks_invested_rows_above_recent_one_offs
    // but order = Recency: assert u-latest is ABOVE u-big.
}

/// Snapshot state replaces, never merges (tab_order doctrine).
#[test]
fn order_today_and_tab_buckets_are_replaced_from_snapshots() {
    // apply_snapshot with order=Recency/today=100/tab_buckets{3:{100:1}},
    // then a later snapshot with order=Frecency{24}/today=101/empty map;
    // assert the model holds the later values wholesale.
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave-bar 2>&1 | head -30`
Expected: compile failures (`frecency_millis` undefined, `snap()` missing fields).

- [ ] **Step 3: Implement**

Model fields (beside `tab_order`/`collapsed`):

```rust
    /// Ordering mode + dial, `today`, and the tab-keyed bucket twin — all
    /// REPLACED wholesale from every snapshot, never merged (the
    /// tab_order doctrine, C5 round 5).
    order: OrderMode,
    today: u32,
    tab_buckets: BTreeMap<usize, BTreeMap<u32, u32>>,
```

(Initialize in the model's constructor/Default with `OrderMode::default()`, `0`, `BTreeMap::new()`.) In `apply_snapshot` beside `self.tab_order = snap.tab_order;` (line ~1272):

```rust
        self.order = snap.order;
        self.today = snap.today;
        self.tab_buckets = snap.tab_buckets;
```

The score, near `rank_desc`:

```rust
/// Frecency score in millipoints: Σ count × 0.5^(age_days × 24 / half_life_hours),
/// ×1000, floored. Millipoints keep the comparator integral; identical
/// bucket maps produce identical sums (BTreeMap order is deterministic),
/// which is what makes the newborn-adjacency tie exact. Future-dated
/// buckets (clock skew) clamp to age 0. half_life_hours == 0 is treated
/// as 1 (the CLI can't produce 0, but the wire could).
fn frecency_millis(buckets: &BTreeMap<u32, u32>, today: u32, half_life_hours: u32) -> u64 {
    let hl = half_life_hours.max(1) as f64;
    let sum: f64 = buckets
        .iter()
        .map(|(&day, &count)| {
            let age_days = today.saturating_sub(day) as f64;
            count as f64 * 0.5_f64.powf(age_days * 24.0 / hl)
        })
        .sum();
    (sum * 1000.0) as u64
}
```

The keys, beside `live_ord`/`dormant_ord` (which stay, unchanged — they are the recency mode and the fallback):

```rust
    /// The comparator's primary key for a LIVE row. Recency: the shipped
    /// ordinal. Frecency: millipoints, max-merged across the tab twin and
    /// the agent's own buckets (same R2 identity as live_ord); zero-score
    /// rows fall back to (0, ordinal) so an unbucketed fleet — upgrade
    /// day, cold dormants, the whole existing test suite — keeps S1 order.
    fn live_key(&self, t: &TabMeta) -> (u64, u64) {
        match self.order {
            OrderMode::Recency => (self.live_ord(t), 0),
            OrderMode::Frecency { half_life_hours } => {
                let tab = self
                    .tab_buckets
                    .get(&t.tab_id)
                    .map_or(0, |b| frecency_millis(b, self.today, half_life_hours));
                let agent = self
                    .agent_in_tab(t.tab_id)
                    .map_or(0, |a| frecency_millis(&a.buckets, self.today, half_life_hours));
                let millis = tab.max(agent);
                if millis > 0 { (millis, 0) } else { (0, self.live_ord(t)) }
            }
        }
    }

    /// Same rule read from the dormant side — one rule for both row
    /// classes, or closing a tab would change a row's rank (R2).
    fn dormant_key(&self, a: &Agent) -> (u64, u64) {
        match self.order {
            OrderMode::Recency => (self.dormant_ord(a), 0),
            OrderMode::Frecency { half_life_hours } => {
                let own = frecency_millis(&a.buckets, self.today, half_life_hours);
                let carried = a
                    .tab_id
                    .and_then(|id| self.tab_buckets.get(&id))
                    .map_or(0, |b| frecency_millis(b, self.today, half_life_hours));
                let millis = own.max(carried);
                if millis > 0 { (millis, 0) } else { (0, self.dormant_ord(a)) }
            }
        }
    }
```

`rank_desc` widens its first element (comparator body unchanged in spirit):

```rust
fn rank_desc(
    a: &((u64, u64), usize, (RowKey, Row)),
    b: &((u64, u64), usize, (RowKey, Row)),
) -> std::cmp::Ordering {
    b.0.cmp(&a.0).then(a.1.cmp(&b.1))
}
```

`rows()` swaps the key calls: `live.push((self.live_key(t), t.position, ...))` and `dormant.push((self.dormant_key(a), usize::MAX - i, ...))`, types updated to `Vec<((u64, u64), usize, (RowKey, Row))>`.

Fixture fallout: `agent()` gains `buckets: BTreeMap::new(),`; `snap()` gains `order: OrderMode::default(), today: 0, tab_buckets: BTreeMap::new(),`; the ~25 raw `Agent {` literals get the field mechanically.

- [ ] **Step 4: Run to verify pass — including that ZERO existing ordering tests changed expectations**

Run: `cargo test -p clave-bar && cargo build -p clave-bar --target wasm32-wasip1`
Expected: PASS. If any pre-existing ordering test fails, that is a defect in the zero-fallback, not in the test — stop and fix the fallback.

- [ ] **Step 5: Commit**

```bash
git add crates/clave-bar/src/model.rs
git commit -m "The bar learns to weigh time: frecency keys with an ordinal floor"
```

---

### Task 7: Docs + gates

**Files:**
- Modify: `UBIQUITOUS_LANGUAGE.md` (add: frecency, day bucket, opener, the widened commitment), `README.md` (the `clave order` surface, brief)

- [ ] **Step 1: Write the entries** — UBIQUITOUS_LANGUAGE gets one line each in its existing style: **frecency** (decayed-commitment score, the default row order), **day bucket** (unix-day → commitment count, 7-day retention), **opener** (the max-tab_order tab a newborn inherits buckets from — a proxy for "focused at creation"). README documents `clave order recency|frecency [HOURS]` next to its nearest CLI sibling.

- [ ] **Step 2: Run the full gates**

Run: `just gates` (or the four commands in order: fmt --check, test --workspace, wasm build, clippy -D warnings)
Expected: all green.

- [ ] **Step 3: Mutation-test the new decision points**

Run: `cargo mutants -p clave-bar -f model.rs -F frecency 2>/dev/null || cargo mutants -p clave-bar --file crates/clave-bar/src/model.rs` (scope to the new fns if runtime is long; kill surviving mutants in `frecency_millis`/`live_key`/`dormant_key`/`bump_bucket`/`opener_buckets` with sharper asserts)

- [ ] **Step 4: Commit**

```bash
git add UBIQUITOUS_LANGUAGE.md README.md
git commit -m "Name the new vocabulary: frecency, day bucket, opener"
```

---

## Self-review notes (spec → plan)

- Spec "buckets written unconditionally in both modes": satisfied — Tasks 2-4 write regardless of `Store.order`; only the bar's key construction branches.
- Spec "recency stays switchable": Task 5 CLI + Task 6 Recency arm delegating to untouched `live_ord`/`dormant_ord`.
- Spec deviation (opener proxy): implemented as `opener_buckets` max-tab_order rule, comment carries the caveat — Ollie ratifies before execution.
- Live validation (docs/dev/TESTING.md tiers): after gates, drive the sandbox — `clave order frecency`, prompt two agents unevenly, verify the invested one holds rank; `clave order recency`, verify last-prompted jumps up. Not automatable; SOP in TESTING.md.
