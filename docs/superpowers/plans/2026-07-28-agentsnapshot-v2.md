# AgentSnapshot v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `Agent` / `AgentRecord` the three structural fields the sidebar redesign needs — `title`, `summary`, `worktree` — so S5 (#60) and S6 (#61) can start.

**Architecture:** Purely additive. Three `#[serde(default)]` fields on the wire struct, two on the store record, projected through `snapshot_from` — the single producer, read by `apply_snapshot`, the single consumer. One self-limiting backfill lifts `summary` out of existing composed labels. **Nothing renders differently; this is plumbing.**

**Tech Stack:** Rust (edition 2024), serde/serde_json, two crates in one workspace (`clave` host binary, `clave-bar` wasm32-wasip1 plugin) plus `clave-types` for shared vocabulary.

**Spec:** `docs/superpowers/specs/2026-07-28-agentsnapshot-v2-design.md`

## Global Constraints

- **`cargo test --workspace`, always.** Bare `cargo test` silently skips 68 tests and exits 0.
- **`just gates` runs the four CI gates in CI's order:** `cargo fmt --all --check`, `cargo test --workspace`, `cargo build -p clave-bar --target wasm32-wasip1`, `cargo clippy --workspace --all-targets -- -D warnings`. All four must be green before any PR.
- **GLYPH RULE (design-lock §5.4, load-bearing):** write every non-ASCII glyph as a `\u{...}` escape in source, **never** as a literal character. Literal glyphs were silently lost in transit twice and the failure mode is tofu in production from a diff that looked clean.
- **Every new field carries `#[serde(default)]`** and a why-comment in the existing house style — see `tab_id` and `stale` on `Agent`, each of which says "keeps pre-field payloads parseable". Without it, a missing key is a **whole-document parse failure**, and the first run against the existing `agents.json` would show zero agents.
- **Dense why-comments** citing the spec section, issue or ledger finding. Never restate what the code does.
- **Never run a bare `zellij` command**, never kill or launch a session, never `just dev-install`. The maintainer dog-foods clave and this agent runs inside his live session. Use `just sandbox`.
- **Do not implement any S-workstream.** No colours, no gutter, no widths, no liveness. Liveness belongs to S4 (#59).

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/clave-types/src/lib.rs` | shared wire vocabulary | +3 fields on `Agent`, +`LABEL_SEP`, +2 tests, 4 existing `Agent` literals updated |
| `crates/clave/src/store.rs` | store record, snapshot production, backfill | +2 fields on `AgentRecord`, `snapshot_from` projects 3, new `backfill_summaries`, wired into `clear_tab_timeline` |
| `crates/clave-bar/src/model.rs` | bar state machine (tests only) | 2 test helpers updated — the bar reads none of these fields yet |
| 9 further files | `AgentRecord` literal construction sites | +2 fields each, mechanical |
| `FOOTGUNS.md` | trap index | +3 entries from the spike |

**Not touched, deliberately:** `hook.rs`'s label logic, `add.rs`'s composer, anything in `clave-bar` outside test helpers. `merge_resume_record` (`add.rs:352`) uses `..row.clone()` and therefore **needs no edit** — it preserves the new fields as earned state automatically, which is the correct policy.

---

### Task 1: `Agent` gains `title`, `summary`, `worktree`

The wire struct. `worktree` becomes truthful immediately (the store already has it); `title` and `summary` land as defaults for S4 to populate.

**Files:**
- Modify: `crates/clave-types/src/lib.rs:50-79` (struct), `:169-263` (four existing test literals)
- Modify: `crates/clave/src/store.rs:175-186` (`snapshot_from` must supply the new fields to compile)
- Modify: `crates/clave-bar/src/model.rs:1147-1177` (two test helpers)
- Test: `crates/clave-types/src/lib.rs` (tests module, alongside `agent_stale_roundtrips_and_defaults_false`)

**Interfaces:**
- Consumes: nothing.
- Produces: `Agent { …, title: Option<String>, summary: String, worktree: Option<String> }`. Task 2 populates `title`/`summary` from the record; Task 3 backfills `summary`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/clave-types/src/lib.rs`, immediately after `agent_stale_roundtrips_and_defaults_false`:

```rust
    #[test]
    fn agent_title_summary_worktree_roundtrip_and_default() {
        // Design-lock §7.1: a live row renders from the STORE, so the bar
        // needs the VALUES for its fixed-width title/repo/summary columns —
        // not positions inside the composed `label`. That ruling deleted
        // InkSpan and made these three structural (#69).
        let mut a = Agent {
            uuid: "u1".into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x \u{00b7} main".into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
            tab_id: None,
            stale: false,
            title: Some("CLA-MAIN".into()),
            summary: "fix the flaky auth".into(),
            worktree: Some("/x/.claude/worktrees/wt".into()),
        };
        let back: Agent = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back.title.as_deref(), Some("CLA-MAIN"));
        assert_eq!(back.summary, "fix the flaky auth");
        assert_eq!(back.worktree.as_deref(), Some("/x/.claude/worktrees/wt"));

        // A v1 payload carries none of the three keys and MUST still parse —
        // the CLI and the wasm bar upgrade at different moments (a running
        // session keeps the bar it loaded), so this is a live state.
        a.title = None;
        a.summary = String::new();
        a.worktree = None;
        let mut v: serde_json::Value = serde_json::to_value(&a).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("title");
        o.remove("summary");
        o.remove("worktree");
        let old: Agent = serde_json::from_value(v).unwrap();
        assert_eq!(old.title, None);
        assert!(old.summary.is_empty());
        assert_eq!(old.worktree, None);
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
set -o pipefail   # else grep/tail supplies the exit status and a cargo failure reads green
cargo test -p clave-types agent_title_summary_worktree 2>&1 | tail -n 20
```

Expected: FAIL — compile error, `struct Agent has no field named title`.

- [ ] **Step 3: Add the three fields**

In `crates/clave-types/src/lib.rs`, inside `pub struct Agent`, after the `stale` field (which ends at `:78`):

```rust
    /// Claude's session rename (`custom-title` in the transcript) — the
    /// filled chip in design-lock §2's 7-column title field. `None` = never
    /// renamed, which is the majority of rows. Structural rather than parsed
    /// out of `label`: §7.1 rules the bar lays its own fixed-width columns
    /// and needs the VALUE, not a position inside a composed string.
    /// Populated by S4 (#59); `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub title: Option<String>,
    /// The words segment — design-lock §2's 17-column field, the widest on
    /// the row and the one actually read. Reachable today only by splitting
    /// `label`, which §7.1 forbids the bar from doing. Populated by S4 (#59),
    /// whose source tier is retargeted to `ai-title` because the
    /// `type:"summary"` tier is extinct (#79). `default` keeps pre-field
    /// payloads parseable.
    #[serde(default)]
    pub summary: String,
    /// Worktree path if `clave add --worktree` created one, else None — the
    /// input to S6's provenance glyph (#61). Held on `AgentRecord` since
    /// §6.3 and simply never projected until now. `Option<String>` not
    /// `bool`: #24 wants the worktree DIRECTORY NAME, which needs the path.
    /// `default` keeps pre-field payloads parseable.
    #[serde(default)]
    pub worktree: Option<String>,
```

- [ ] **Step 4: Update the four existing `Agent` literals in this file's tests**

At `:171` (`agent_json_has_no_archived_field`), `:192` (`snapshot_roundtrips`), `:216` (`agent_tab_id_roundtrips_and_defaults_none`) and `:242` (`agent_stale_roundtrips_and_defaults_false`), add these three lines after each literal's `stale:` line:

```rust
            title: None,
            summary: String::new(),
            worktree: None,
```

Match each literal's existing indentation — `:192`'s is nested one level deeper inside `vec![…]`.

- [ ] **Step 5: Make `snapshot_from` compile**

In `crates/clave/src/store.rs`, in the `.map(|r| Agent { … })` closure (`:175`), after `stale: r.stale,`:

```rust
                // Projected now — `AgentRecord` has carried this since §6.3
                // and the wire simply never did (S6 #61 §2.4).
                worktree: r.worktree.clone(),
                // Task 2 replaces these with `r.title` / `r.summary` once the
                // record carries them. Defaults keep the shape honest in the
                // meantime: no consumer reads them yet.
                title: None,
                summary: String::new(),
```

- [ ] **Step 6: Update the two bar test helpers**

In `crates/clave-bar/src/model.rs`, in `fn agent(…)` (`:1148`) and `fn agent_labelled(…)` (`:1164`), add to each `Agent { … }` literal:

```rust
            title: None,
            summary: String::new(),
            worktree: None,
        }
```

(Insert the three fields before the closing brace, matching existing indentation. The bar reads none of these fields — verified: `cwd`, `branch` and `worktree` have zero consumers in `model.rs` and `main.rs` outside test fixtures.)

- [ ] **Step 7: Run the tests**

```bash
set -o pipefail   # else grep/tail supplies the exit status and a cargo failure reads green
cargo test --workspace 2>&1 | grep -E "^(test result|error)" | head -n 20
```

Expected: all suites PASS, including `agent_title_summary_worktree_roundtrip_and_default`.

- [ ] **Step 8: Commit**

```bash
git add crates/clave-types/src/lib.rs crates/clave/src/store.rs crates/clave-bar/src/model.rs
git commit -m "feat(types): Agent carries title, summary and worktree (#69)

Design-lock §7.1 rules that a live row renders from the store, so the bar
needs these as VALUES for its fixed-width columns rather than positions
inside the composed label. worktree is projected truthfully now (the
record has carried it since §6.3); title and summary land as defaults for
S4 (#59) to populate.

All three #[serde(default)] — a missing key is a whole-document parse
failure, and the CLI and wasm bar upgrade at different moments."
```

---

### Task 2: `AgentRecord` carries `title` and `summary`

The store record gains the two fields the wire now exposes, and `snapshot_from` stops defaulting them.

**Files:**
- Modify: `crates/clave/src/store.rs:39-68` (struct), `:175-186` (`snapshot_from`)
- Modify (mechanical, +2 fields each): `crates/clave/src/hook.rs:304`, `crates/clave/src/add.rs:748`, `crates/clave/src/add.rs:780`, `crates/clave/src/lsview.rs:35`, `crates/clave/src/store.rs:372`, `crates/clave/src/dev.rs:236`, `crates/clave/src/open.rs:145`, `crates/clave/src/setup.rs:849`, `crates/clave/src/setup.rs:885`, `crates/clave/src/setup.rs:1046`, `crates/clave/tests/kdl_guardrail.rs:118`
- Test: `crates/clave/src/store.rs` tests module

**Interfaces:**
- Consumes: `Agent { title, summary, worktree }` from Task 1.
- Produces: `AgentRecord { …, title: Option<String>, summary: String }`, and `snapshot_from` projecting `r.title.clone()` / `r.summary.clone()`. Task 3's backfill writes `AgentRecord::summary`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/clave/src/store.rs`:

```rust
    #[test]
    fn snapshot_projects_title_summary_and_worktree_from_the_record() {
        // One producer, one consumer (§5): snapshot_from is the only place
        // a record becomes a wire Agent, so this is the whole contract.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.title = Some("CLA-MAIN".into());
        r.summary = "fix the flaky auth".into();
        r.worktree = Some("/x/.claude/worktrees/wt".into());
        s.agents.insert("u1".into(), r);

        let snap = snapshot_from(&s);
        let a = &snap.agents[0];
        assert_eq!(a.title.as_deref(), Some("CLA-MAIN"));
        assert_eq!(a.summary, "fix the flaky auth");
        assert_eq!(a.worktree.as_deref(), Some("/x/.claude/worktrees/wt"));
    }

    #[test]
    fn agent_record_title_and_summary_default_on_pre_field_store_files() {
        // The first run of a new binary reads the EXISTING agents.json, which
        // has neither key. Without #[serde(default)] that is a whole-store
        // parse failure and every agent vanishes — not a blank field.
        let json = serde_json::to_value(rec("u1")).unwrap();
        let mut o = json.as_object().unwrap().clone();
        o.remove("title");
        o.remove("summary");
        let back: AgentRecord = serde_json::from_value(serde_json::Value::Object(o)).unwrap();
        assert_eq!(back.title, None);
        assert!(back.summary.is_empty());
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
set -o pipefail   # else grep/tail supplies the exit status and a cargo failure reads green
cargo test -p clave snapshot_projects_title 2>&1 | tail -n 20
```

Expected: FAIL — compile error, `struct AgentRecord has no field named title`.

- [ ] **Step 3: Add the two fields to `AgentRecord`**

In `crates/clave/src/store.rs`, inside `pub struct AgentRecord`, after the `stale` field (ending `:67`):

```rust
    /// Claude's session rename, from the transcript's `custom-title` line.
    /// Store-side home for the wire field of the same name (#69). Written by
    /// S4 (#59); nothing populates it yet, so it stays None. `default` keeps
    /// pre-field store files loading — a missing key is a whole-store parse
    /// failure, not a blank field.
    #[serde(default)]
    pub title: Option<String>,
    /// The words segment, held structurally rather than only inside `label`
    /// (design-lock §7.1). Seeded once from existing labels by
    /// `backfill_summaries`; thereafter written by S4 (#59) from `ai-title`,
    /// the `type:"summary"` tier being extinct (#79). `default` keeps
    /// pre-field store files loading.
    #[serde(default)]
    pub summary: String,
```

- [ ] **Step 4: Project them in `snapshot_from`**

In the `.map(|r| Agent { … })` closure, replace the two placeholder lines Task 1 added:

```rust
                title: None,
                summary: String::new(),
```

with:

```rust
                title: r.title.clone(),
                summary: r.summary.clone(),
```

Leave the `worktree: r.worktree.clone(),` line and its comment as they are.

- [ ] **Step 5: Update the eleven `AgentRecord` literal sites**

Each is a full struct literal and now needs both fields. Add to every one:

```rust
            title: None,
            summary: String::new(),
```

Sites, all needing the same two lines (match each one's indentation):

`crates/clave/src/hook.rs:304` · `crates/clave/src/add.rs:748` · `crates/clave/src/add.rs:780` · `crates/clave/src/lsview.rs:35` · `crates/clave/src/store.rs:372` · `crates/clave/src/dev.rs:236` · `crates/clave/src/open.rs:145` · `crates/clave/src/setup.rs:849` · `crates/clave/src/setup.rs:885` · `crates/clave/src/setup.rs:1046` · `crates/clave/tests/kdl_guardrail.rs:118`

**Do not touch `crates/clave/src/add.rs:352`** (`merge_resume_record`). It builds with `..row.clone()`, so it already preserves both new fields from the existing row — which is the correct policy: they are earned state, like `label`, and a re-add has no business resetting them.

If the compiler reports a literal not in this list, add the same two lines; the list was enumerated by grep and a new site may have landed since.

- [ ] **Step 6: Run the tests**

```bash
set -o pipefail   # else grep/tail supplies the exit status and a cargo failure reads green
cargo test --workspace 2>&1 | grep -E "^(test result|error)" | head -n 20
```

Expected: all suites PASS, including both new tests.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(store): AgentRecord carries title and summary, projected to the wire (#69)

snapshot_from is the single producer, so this completes the §7.1 contract:
the bar receives title/summary/worktree as values and never parses a
composed label.

merge_resume_record is deliberately untouched — its ..row.clone() already
preserves both as earned state, matching how label is treated."
```

---

### Task 3: `LABEL_SEP`, and the one-shot summary backfill

Existing rows carry their summary inside `label` and nothing else will ever fill the new field for them — `refresh_label` returns early once `label_source == Summary` (`hook.rs:155`), and dormant rows receive no hook events at all.

**Files:**
- Modify: `crates/clave-types/src/lib.rs` (add `LABEL_SEP` beside the structs)
- Modify: `crates/clave/src/store.rs` (add `backfill_summaries`, wire into `clear_tab_timeline:340-349`)
- Test: `crates/clave/src/store.rs` tests module

**Interfaces:**
- Consumes: `AgentRecord::summary` from Task 2.
- Produces: `clave_types::LABEL_SEP: &str` and `store::backfill_summaries(&mut Store) -> bool`.

**Note on `LABEL_SEP`:** both S4 (§4.1) and S5 (§3.1) propose this constant, and S5's spec says *"If S4 lands `LABEL_SEP` first, delete this copy and import theirs."* That is the same one-decision-made-twice problem #69 exists to end, and the backfill needs the separator — so it lands here, once, correctly escaped.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/clave/src/store.rs`:

```rust
    #[test]
    fn backfill_lifts_the_words_segment_out_of_an_existing_label() {
        // Rows written before `summary` existed carry it only inside `label`.
        // refresh_label returns early forever once label_source == Summary
        // (hook.rs:155), and dormant rows get no hook events at all — so
        // without this they would render a blank 17-column field for good.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.label = "clave \u{00b7} main \u{00b7} fix the flaky auth".into();
        r.summary = String::new();
        s.agents.insert("u1".into(), r);

        assert!(backfill_summaries(&mut s));
        assert_eq!(s.agents["u1"].summary, "fix the flaky auth");
    }

    #[test]
    fn backfill_keeps_a_separator_inside_the_summary_text() {
        // splitn(3) — a summary that itself contains the separator survives
        // whole. A plain split() would truncate it at the first occurrence.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.label = "clave \u{00b7} main \u{00b7} a \u{00b7} b".into();
        r.summary = String::new();
        s.agents.insert("u1".into(), r);

        backfill_summaries(&mut s);
        assert_eq!(s.agents["u1"].summary, "a \u{00b7} b");
    }

    #[test]
    fn backfill_is_idempotent_and_skips_labels_without_a_words_segment() {
        // Self-limiting: it matches only EMPTY summaries, so a second pass
        // changes nothing. Same shape as S1 §3.6's commit_ord backfill.
        let mut s = Store::default();
        let mut earned = rec("u1");
        earned.label = "clave \u{00b7} main \u{00b7} fix the flaky auth".into();
        earned.summary = "already set by S4".into();
        s.agents.insert("u1".into(), earned);
        let mut bare = rec("u2");
        bare.label = "clave \u{00b7} main".into(); // never earned any words
        bare.summary = String::new();
        s.agents.insert("u2".into(), bare);

        assert!(!backfill_summaries(&mut s), "nothing to do");
        assert_eq!(s.agents["u1"].summary, "already set by S4");
        assert!(s.agents["u2"].summary.is_empty());

        // And a real pass must not re-fire on a second run.
        let mut t = Store::default();
        let mut r = rec("u3");
        r.label = "clave \u{00b7} main \u{00b7} words".into();
        r.summary = String::new();
        t.agents.insert("u3".into(), r);
        assert!(backfill_summaries(&mut t));
        assert!(!backfill_summaries(&mut t), "second pass is a no-op");
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
set -o pipefail   # else grep/tail supplies the exit status and a cargo failure reads green
cargo test -p clave backfill 2>&1 | tail -n 20
```

Expected: FAIL — `cannot find function backfill_summaries in this scope`.

- [ ] **Step 3: Add `LABEL_SEP` to `clave-types`**

In `crates/clave-types/src/lib.rs`, after the `Register` struct (`:112`):

```rust
/// The label segment separator: U+0020 U+00B7 U+0020. One constant, two
/// crates — `add.rs` and `hook.rs` compose with it, `store.rs`'s backfill
/// splits on it. Written as an escape, never a literal: design-lock §5.4
/// (load-bearing) records that literal glyphs were silently lost in transit
/// twice, and the failure mode is tofu in production from a clean-looking
/// diff. S4 §4.1 and S5 §3.1 each proposed this constant independently —
/// it lands once, here (#69).
pub const LABEL_SEP: &str = " \u{00b7} ";
```

- [ ] **Step 4: Implement `backfill_summaries`**

In `crates/clave/src/store.rs`, immediately before `clear_tab_timeline`:

```rust
/// Seed `summary` for rows written before the field existed, by lifting the
/// words segment out of the composed label (`dir · branch · words`).
///
/// `splitn(3)` so a summary that itself contains the separator survives
/// whole. Matches only EMPTY summaries, so it is idempotent and self-limiting
/// — after one pass nothing matches again. Same shape as S1 §3.6's
/// `commit_ord` backfill.
///
/// WHY it is needed at all, given S4 (#59) will keep summaries live:
/// `refresh_label` returns early forever once `label_source == Summary`
/// (`hook.rs:155`), and dormant rows receive no hook events by definition —
/// so without this they render a blank 17-column field indefinitely.
///
/// Returns whether anything changed, so the caller can gate its `seq` bump:
/// §5 forbids no-op pushes.
pub fn backfill_summaries(s: &mut Store) -> bool {
    let mut changed = false;
    for r in s.agents.values_mut() {
        if !r.summary.is_empty() {
            continue;
        }
        if let Some(words) = r.label.splitn(3, clave_types::LABEL_SEP).nth(2)
            && !words.is_empty()
        {
            r.summary = words.to_string();
            changed = true;
        }
    }
    changed
}
```

- [ ] **Step 5: Wire it into `clear_tab_timeline`**

Replace the body of `clear_tab_timeline` (`:340-349`) with:

```rust
pub fn clear_tab_timeline(paths: &StorePaths) -> Result<()> {
    with_store_mut(paths, |s| {
        let bound = s.agents.values().any(|r| r.tab_id.is_some());
        let mut changed = false;
        if !s.tab_timeline.is_empty() || bound {
            s.tab_timeline.clear();
            s.agents.values_mut().for_each(|r| r.tab_id = None);
            changed = true;
        }
        // Session create is the one locked pass that runs at every launch,
        // so it is where the one-shot backfill rides (#69). Accepted cost: a
        // MID-session upgrade leaves dormant rows blank until the next
        // launch. The alternative is a migration hook on every store open —
        // more machinery than a cosmetic gap on unused rows justifies.
        changed |= backfill_summaries(s);
        if changed {
            s.seq += 1; // content changed ⇒ seq changed (§5)
        }
    })
}
```

- [ ] **Step 6: Run the tests**

```bash
set -o pipefail   # else grep/tail supplies the exit status and a cargo failure reads green
cargo test --workspace 2>&1 | grep -E "^(test result|error)" | head -n 20
```

Expected: all suites PASS, including the three backfill tests. `clear_tab_timeline_wipes_session_scoped_ids` (`store.rs:655`) must still pass — it asserts the clearing behaviour, which is unchanged.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(store): one-shot summary backfill, and a shared LABEL_SEP (#69)

Existing rows carry their summary only inside the composed label, and
nothing would ever fill the new field for them: refresh_label returns
early forever once label_source == Summary (hook.rs:155), and dormant
rows receive no hook events at all.

Idempotent and self-limiting — it matches only empty summaries. Rides
clear_tab_timeline, the one locked pass that runs at every session
create.

LABEL_SEP lands here rather than in S4 or S5, which each proposed it
independently. Escaped per design-lock §5.4, never a literal."
```

---

### Task 4: Record the spike's traps, and run the full gates

The spike found three things that will cost the next agent time. `FOOTGUNS.md` is where the project puts those.

**Files:**
- Modify: `FOOTGUNS.md`
- Modify: `docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md` §7.1

- [ ] **Step 1: Add a new section for Claude's transcript format**

`FOOTGUNS.md` has no section covering the transcript tree, and two of the three traps live there. Insert a new `##` section after `## The bar's model — frame joins, latches, ordering` (which ends at `:111`), matching the file's bullet style: **bold mechanism first**, then the explanation, then the citation.

```markdown
## Claude transcripts — what the jsonl actually contains

The transcript tree is `~/.claude/projects/<munged-cwd>/<uuid>.jsonl`. Read it,
do not assume it: its line types changed under us once already.

- **`{"type":"summary"}` is EXTINCT — the label tier that has never once fired.** `summary_from_tail` (`hook.rs:116`) scans for it; Claude Code no longer writes it. Measured 2026-07-28: **0 of 919** local transcripts contain one, and all 14 live store rows read `label_source: first_prompt`, so every label is a truncated first-prompt fragment. Any test asserting `Summary`-tier behaviour is asserting against a hand-written fixture, not reality — re-point those at `ai-title`, do not delete them. Detect: `grep -rl '"type":"summary"' --include='*.jsonl' ~/.claude/projects | wc -l`. (#79)
- **The signals that DO exist:** `custom-title` (the user's rename), **`ai-title`** (Claude's rolling auto-description — the real replacement for `summary`), `last-prompt`, `worktree-state` (carries `worktreePath`, `worktreeName`, `worktreeBranch` — provenance with no git call), `relocated`, `agent-name`. Inventory them before designing a tier: `grep -ho '"type":"[a-z-]*"' ~/.claude/projects/*/*.jsonl | sort | uniq -c | sort -rn`.
- **A transcript RELOCATES when the session's cwd changes.** Claude moves the whole `.jsonl` into a project directory keyed on the NEW cwd, carrying its full history, and logs a `relocated` line. So any cwd frozen as a "stable transcript anchor" goes stale and the tail read silently stops returning anything — no error, just a row that never updates again. **S4 §3.2 asserts the opposite (*"never moves it"*) and is wrong.** Use `payload.transcript_path`, which every hook event carries. Verify: `find ~/.claude/projects -name '<uuid>.jsonl'` returns one hit, under the *current* cwd. (#59, #69)
```

- [ ] **Step 2: Add the tooling trap**

Append to the existing `## Process and tooling` section (`:144`):

```markdown
- **`~/.claude/projects/` entry names begin with `-`, so a RELATIVE glob over them parses as flags.** From inside the directory, `ls *clave*` expands to `-Users-…` and `ls` reads it as options: `ls: unrecognized option '--var-folders-…'`, exit 1. **The absolute form is fine** — `ls ~/.claude/projects/*clave*` expands to `/Users/…`, which no `ls` mistakes for a flag (measured: exit 0). Getting this backwards is worse than not knowing it, because the absolute form *succeeds* and reads as "there is no trap here". `ls` is also aliased to `eza`, where `-t` means `--time` and errors demanding a field. Canonical command: `find ~/.claude/projects -maxdepth 1 -name '*clave*'`; `/bin/ls --` also works. Cost a round during the #69 spike. (corrected on #81 — the original entry named the wrong failing form)
```

- [ ] **Step 3: Correct the stale claim in design-lock §7.1**

§7.1 states that `Agent` *"carries `label`, the composed string, and has **no `title` and no `summary` field**"*. That is now false. Append to that paragraph:

```markdown
**Landed 2026-07-28 (#69).** `Agent` now carries `title`, `summary` and
`worktree` structurally — see
`docs/superpowers/specs/2026-07-28-agentsnapshot-v2-design.md`. The
prerequisite this section names is met; S5 and S6 are unblocked.
```

- [ ] **Step 4: Run the full gates**

```bash
just gates
```

Expected: exit 0. All four gates green — `fmt --check`, `test --workspace`, the wasm build, and `clippy -D warnings`.

If clippy objects to the let-chain in `backfill_summaries`, the codebase is edition 2024 and already uses this form (`hook.rs:191-195`, commented *"Collapsed into an edition-2024 let-chain — clippy::collapsible_if"*). Match that pattern rather than reverting to nested `if let`.

- [ ] **Step 5: Verify the change is inert in the sandbox**

```bash
just sandbox
```

Expected: completes with all guards passing. The assertion is that **nothing renders differently** — this PR adds no consumer. If the sidebar looks changed, something is wrong.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs(clave): record the #69 spike's three traps, and un-stale design-lock §7.1

FOOTGUNS gains the extinct summary tier (#79), transcript relocation on
cwd change, and the leading-dash project dirs that break bare ls — each
cost a round during the spike.

§7.1's 'no title and no summary field' is no longer true."
```

---

## Verification before opening the PR

This is a **cross-process/IPC change** under `docs/dev/TESTING.md`'s risk taxonomy. It owes an ordering/idempotency argument and an independent adversarial reviewer, and **the PR body must state which review lanes actually ran — a lane that did not run is not a lane that passed.**

The ordering/idempotency argument, ready to paste:

> `snapshot_from` remains the single producer and `apply_snapshot` the single consumer; no new ordering is introduced and `seq` gating is untouched. The one new mutation, `backfill_summaries`, runs inside the existing `clear_tab_timeline` flock, matches only empty summaries, and is therefore idempotent — a second pass returns `false` and bumps no `seq`. All five new fields are `#[serde(default)]`, so a v1 payload and a v1 store file both parse, which is required because the CLI and the wasm bar upgrade at different moments.

CodeRabbit reports `pass` while rate-limited — **read the check detail, not the colour** (#68). This recurred repeatedly on 2026-07-28.

## Deliberately not in this plan

Spec §4 lists amendments owed to four workstream specs. Only the design-lock
correction is in scope here, because it is the one this PR falsifies directly.
The rest belong to their own workstreams and would bloat an inert plumbing PR
into a cross-workstream edit:

- **S4 (#59)** — its §3.2 invariant is false, `payload.transcript_path` becomes
  mandatory, and the `Summary` tier retargets to `ai-title`. **Already recorded
  as a `> [!WARNING]` comment on #59**, so whoever picks it up sees it before
  implementing.
- **S5 (#60), S6 (#61)** — consume the fields this PR lands; each already owed a
  revision from design-lock §9.
- **The extinct tier itself (#79)** — a live bug, filed, and fixed inside S4
  rather than separately, because S4 rewrites that exact tail-scan.

This PR must not touch `hook.rs`'s label logic. If a task seems to require it,
stop: the boundary has been crossed and the work belongs to S4.
