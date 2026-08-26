# Double-Height Cards Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the sidebar row into a two-line card (both width profiles), behind a row-height flag defaulting to double, with model/provider/elapsed/PR data plumbed from the transcript jsonl and a cached `gh` lookup.

**Architecture:** The whole-bar render seam (`render_rows`) stays the only render entry; a new `card.rs` module holds the two-line geometry ported from the ratified example, and `render_rows` dispatches on a `RowHeight` mode. The shared `viewport_top` stays in ROW units; only the two boundary conversions (`height → row budget` in render, `pointer line → row` in click) divide by the mode's lines-per-row. New data rides the existing store→snapshot→model→Row pipeline; the hook's transcript-tail parsers gain a model extractor; PR numbers are resolved by a detached host subcommand with a TTL cache.

**Tech Stack:** Rust workspace; `clave-bar` builds for `wasm32-wasip1` (host-testable via lib split); serde-defaulted wire types; `gh` CLI (injected for tests).

**Spec:** GitHub issue #232 (`gh issue view 232`). Visual authority: `crates/clave-bar/examples/double-preview.rs` (committed, RATIFIED — every cell budget, ink, and glyph is a decision). Reference render: `cargo run -p clave-bar --example double-preview`.

## Global Constraints

- The four gates, in order (`just gates`): `cargo fmt --all --check` · `cargo test --workspace` · `cargo build -p clave-bar --target wasm32-wasip1` · `cargo clippy --workspace --all-targets -- -D warnings`. Every task's commit lands with all four green.
- Glyphs are `\u{...}` escapes ONLY, never literals (design-lock §5.4 — literals were silently lost twice).
- The single-line renderer, its goldens, and its width targets are retained UNCHANGED behind the flag (spec: out of scope to remove).
- Card budgets (ratified): collapsed 38 cols; expanded 48 cols; repo+branch collective budget 19 cells (9 + 1 + 9), branch minimum 9, long repo truncates first; PR column never moves; odd pane height leaves one blank line.
- `model`/`provider` are open Strings end to end — never enums (future providers: "terra, sol, luna" style names were mused).
- The bar never invents a measurement: absent model/PR/tokens/elapsed render blank.
- Glass rule: unselected cards paint NO background; every unpainted segment re-asserts default bg (`\u{1b}[49m`); selection is a full opaque bar of `theme.sel_bg`.
- Terminology (UBIQUITOUS_LANGUAGE.md): "agent session" never bare "session"; the store is a cache over the transcripts (jsonl is canon).
- Never touch the maintainer's live zellij session; live checks go through the per-worktree sandbox (`clave dev instance`, docs/dev/TESTING.md).
- Max logging on render/viewport/click decision points (maintainer: "all the logs possible").
- Commit messages end with `Claude-Session: https://claude.ai/code/session_01G9ciynQVsqu6q9XK9VCjpR`.

---

### Task 1: `RowHeight` in clave-types — the mode and its width targets

**Files:**
- Modify: `crates/clave-types/src/lib.rs` (near `BAR_TARGET_COLS`, line ~337)
- Test: same file, `#[cfg(test)]` mod at bottom

**Interfaces:**
- Produces: `pub enum RowHeight { Single, Double }` with `Default = Double`; `RowHeight::target_cols(self, collapsed: bool) -> usize`; `RowHeight::lines_per_row(self) -> usize`; `pub const ROW_HEIGHT_KEY: &str = "row_height"`; `RowHeight::from_config_value(v: Option<&str>) -> RowHeight`; serde as lowercase strings `"single"`/`"double"`.

- [ ] **Step 1: Write the failing tests** (append to the existing test mod in `crates/clave-types/src/lib.rs`):

```rust
#[test]
fn row_height_defaults_to_double_and_maps_its_targets() {
    assert_eq!(RowHeight::default(), RowHeight::Double);
    // Double: the ratified card budgets (#232). Single: the legacy pair,
    // which MUST keep reading the existing constants so the old design
    // cannot drift from the flag's legacy arm.
    assert_eq!(RowHeight::Double.target_cols(false), 48);
    assert_eq!(RowHeight::Double.target_cols(true), 38);
    assert_eq!(RowHeight::Single.target_cols(false), BAR_TARGET_COLS);
    assert_eq!(RowHeight::Single.target_cols(true), COLLAPSED_TARGET_COLS);
    assert_eq!(RowHeight::Double.lines_per_row(), 2);
    assert_eq!(RowHeight::Single.lines_per_row(), 1);
}

#[test]
fn row_height_parses_its_config_value_failing_closed_to_double() {
    assert_eq!(RowHeight::from_config_value(Some("single")), RowHeight::Single);
    assert_eq!(RowHeight::from_config_value(Some("double")), RowHeight::Double);
    // Absent, empty, or junk → the default. A typo must not strand a user
    // in a mode they didn't ask for.
    assert_eq!(RowHeight::from_config_value(None), RowHeight::Double);
    assert_eq!(RowHeight::from_config_value(Some("")), RowHeight::Double);
    assert_eq!(RowHeight::from_config_value(Some("tall")), RowHeight::Double);
}

#[test]
fn row_height_serde_is_lowercase_and_defaultable() {
    assert_eq!(serde_json::to_string(&RowHeight::Single).unwrap(), "\"single\"");
    let d: RowHeight = serde_json::from_str("\"double\"").unwrap();
    assert_eq!(d, RowHeight::Double);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p clave-types row_height -- --nocapture`
Expected: FAIL — `RowHeight` not found.

- [ ] **Step 3: Implement** (beside the width constants, whose doc comments explain the seek machinery — read them first; do NOT rename or change `BAR_TARGET_COLS`/`COLLAPSED_TARGET_COLS`, the single-line arm keeps them):

```rust
/// Which row geometry the bar renders — the #232 flag. `Double` is the
/// two-line card (the default); `Single` is the legacy one-line row,
/// retained intact behind this flag. Chosen per LAUNCH: the launch layout
/// bakes both the pane sizes and the plugin-config key from it, so the
/// geometry zellij gives the pane and the geometry the bar draws can never
/// disagree mid-session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowHeight {
    Single,
    #[default]
    Double,
}

/// The zellij plugin-config key carrying the mode into the bar (same
/// mechanism as [`CLAVE_BINARY_KEY`], #44).
pub const ROW_HEIGHT_KEY: &str = "row_height";

impl RowHeight {
    /// The width the seek machinery asks for in this mode — the card
    /// budgets ratified in #232, or the legacy pair for `Single`.
    pub fn target_cols(self, collapsed: bool) -> usize {
        match (self, collapsed) {
            (RowHeight::Double, false) => 48,
            (RowHeight::Double, true) => 38,
            (RowHeight::Single, false) => BAR_TARGET_COLS,
            (RowHeight::Single, true) => COLLAPSED_TARGET_COLS,
        }
    }

    /// Terminal lines one row occupies — the `/2` the viewport and click
    /// conversions share (#148 discipline: derived once, here).
    pub fn lines_per_row(self) -> usize {
        match self {
            RowHeight::Single => 1,
            RowHeight::Double => 2,
        }
    }

    /// Parse the plugin-config value, failing CLOSED to the default: a
    /// typo'd or absent key must render the default design, never a
    /// surprise legacy mode.
    pub fn from_config_value(v: Option<&str>) -> RowHeight {
        match v {
            Some("single") => RowHeight::Single,
            _ => RowHeight::Double,
        }
    }
}
```

- [ ] **Step 4: Run tests** — `cargo test -p clave-types row_height` → PASS.
- [ ] **Step 5: Commit** — `git add crates/clave-types/src/lib.rs && git commit -m "feat(types): RowHeight mode with card width targets (#232)"`

---

### Task 2: Store + wire fields — `model`, `provider`, `pr_number`

**Files:**
- Modify: `crates/clave-types/src/lib.rs` (`pub struct Agent`, line ~120: append after `context_level`)
- Modify: `crates/clave/src/store.rs` (`pub struct AgentRecord` line ~40: same three fields plus `pr_checked: u64` and `pr_branch: String`; `snapshot_from` line ~343: project the three wire fields)
- Modify: `crates/clave/src/backfill.rs` (~line 265) and `crates/clave/src/hook.rs` (~lines 1115, 1149): struct literals gain the new fields (compiler drives — every `Agent`/`AgentRecord` literal in tests too)
- Test: `crates/clave/src/store.rs` test mod

**Interfaces:**
- Produces on wire `Agent` AND `AgentRecord`: `pub model: Option<String>`, `pub provider: Option<String>`, `pub pr_number: Option<u32>` — all `#[serde(default)]`. On `AgentRecord` only: `pub pr_checked: u64` (unix secs of last lookup attempt, 0 = never), `pub pr_branch: String` (the branch the cached `pr_number` was resolved for) — both `#[serde(default)]`, host-side cache bookkeeping the bar never sees.

- [ ] **Step 1: Write the failing test** (store.rs test mod; mirror the existing projection test style):

```rust
#[test]
fn snapshot_projects_model_provider_and_pr_but_not_the_cache_bookkeeping() {
    let mut s = Store::default();
    let mut r = rec("u1"); // the existing test fixture helper
    r.model = Some("fable".into());
    r.provider = Some("claude".into());
    r.pr_number = Some(232);
    r.pr_checked = 1_756_200_000;
    r.pr_branch = "double-rows".into();
    s.agents.insert("u1".into(), r);
    let snap = snapshot_from(&s);
    let a = &snap.agents[0];
    assert_eq!(a.model.as_deref(), Some("fable"));
    assert_eq!(a.provider.as_deref(), Some("claude"));
    assert_eq!(a.pr_number, Some(232));
}

#[test]
fn pre_field_payloads_still_parse() {
    // serde(default) is the compat contract every store field carries.
    let a: clave_types::Agent =
        serde_json::from_str(r#"{"uuid":"u","cwd":"/","repo_root":"/","branch":"main",
            "label":"l","status":"Idle","last_interacted":0,"last_visited":0}"#).unwrap();
    assert_eq!(a.model, None);
    assert_eq!(a.provider, None);
    assert_eq!(a.pr_number, None);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p clave snapshot_projects_model` → FAIL (no such fields).
- [ ] **Step 3: Implement** — add the fields with doc comments in the file's established voice (each field says WHY it exists and what `None` means: "`None` = no reading yet, renders blank — the bar never invents a measurement"). Project the three in `snapshot_from` with a comment noting `pr_checked`/`pr_branch` stay host-side (cache bookkeeping, not display). Chase every struct-literal compile error (`cargo check --workspace` lists them all) adding `model: None, provider: None, pr_number: None` (+ `pr_checked: 0, pr_branch: String::new()` on records).
- [ ] **Step 4: Run** — `cargo test --workspace` → PASS (existing suites prove compat).
- [ ] **Step 5: Commit** — `feat(store): model, provider, pr_number ride the record and the wire (#232)`

---

### Task 3: Hook tail-parse — `model_from_tail` + `short_model`, stamped on hook events

**Files:**
- Modify: `crates/clave/src/hook.rs` — new parsers beside `tokens_from_tail` (line ~368); stamping inside `apply_hook_event` (line ~600), next to the `tokens_from_tail` block (line ~666)
- Test: hook.rs test mod (fixture style of `s7_*` tests, line ~1751)

**Interfaces:**
- Consumes: `last_tail_field` cannot reach nested JSON — write a nested reader.
- Produces: `pub fn model_from_tail(tail: &str) -> Option<String>` (raw model id from the newest assistant line's `message.model`); `pub fn short_model(raw: &str) -> String` (display form, ≤ the card's 6-cell budget in the common case but NOT truncated here — the renderer owns truncation).

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn model_from_tail_reads_the_newest_assistant_lines_nested_model() {
    let tail = concat!(
        r#"{"type":"assistant","message":{"model":"claude-opus-5","usage":{"input_tokens":5}}}"#, "\n",
        r#"{"type":"user","message":{"role":"user"}}"#, "\n",
        r#"{"type":"assistant","message":{"model":"claude-fable-5","usage":{"input_tokens":9}}}"#, "\n",
    );
    assert_eq!(model_from_tail(tail).as_deref(), Some("claude-fable-5"));
    assert_eq!(model_from_tail(r#"{"type":"user"}"#), None);
    // A malformed line scans PAST, not fails — same discipline as
    // last_tail_field.
    let dirty = format!("not-json\n{tail}");
    assert_eq!(model_from_tail(&dirty).as_deref(), Some("claude-fable-5"));
}

#[test]
fn short_model_derives_the_family_word() {
    // The display forms the ratified example renders: fable / opus /
    // sonnet / haiku / gpt-5.
    assert_eq!(short_model("claude-fable-5"), "fable");
    assert_eq!(short_model("claude-opus-5"), "opus");
    assert_eq!(short_model("claude-sonnet-5"), "sonnet");
    assert_eq!(short_model("claude-haiku-4-5-20251001"), "haiku");
    assert_eq!(short_model("claude-3-5-sonnet-20241022"), "sonnet");
    // Unknown vendors pass through untouched — open strings, never enums.
    assert_eq!(short_model("gpt-5"), "gpt-5");
    assert_eq!(short_model("sol-2"), "sol-2");
}

#[test]
fn a_stop_event_stamps_model_and_provider() {
    // Pattern: the s7_* tests build a store + payload and call
    // apply_hook_event with a synthetic tail. Reuse rec()/capture().
    let mut s = Store::default();
    s.agents.insert("minted".into(), rec("minted"));
    let tail = r#"{"type":"assistant","message":{"model":"claude-fable-5","usage":{"input_tokens":1000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
    let p = HookPayload { session_id: Some("minted".into()), ..Default::default() };
    apply_hook_event(&mut s, "minted", "Stop", &p, Some(tail), 100, true);
    assert_eq!(s.agents["minted"].model.as_deref(), Some("fable"));
    assert_eq!(s.agents["minted"].provider.as_deref(), Some("claude"));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p clave model_from_tail short_model a_stop_event_stamps` → FAIL.
- [ ] **Step 3: Implement:**

```rust
/// The newest assistant line's `message.model` — the raw model id, e.g.
/// `claude-fable-5`. Nested under `message`, so `last_tail_field` (top-level
/// fields only) cannot read it. Same reverse-scan, skip-malformed,
/// skip-empty discipline.
pub fn model_from_tail(tail: &str) -> Option<String> {
    tail.lines().rev().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        if v.get("type")?.as_str()? != "assistant" {
            return None;
        }
        let s = v.get("message")?.get("model")?.as_str()?.trim();
        (!s.is_empty()).then(|| s.to_string())
    })
}

/// Display form of a model id: for Claude ids, the FAMILY word (`fable`,
/// `opus`, `sonnet`, `haiku`) — the segment after the vendor prefix that
/// isn't a version number; anything else passes through untouched (open
/// strings — other providers name their own). The store carries this SHORT
/// form: the card's model cell is 6 columns and the raw id is unreadable
/// there, and a dumb renderer (truncate, never munge) is the lock's style.
pub fn short_model(raw: &str) -> String {
    match raw.strip_prefix("claude-") {
        Some(rest) => rest
            .split('-')
            .find(|seg| !seg.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(rest)
            .to_string(),
        None => raw.to_string(),
    }
}
```

Stamping, inside `apply_hook_event` directly after the `tokens_from_tail` block (~line 668) — same fail-closed shape (no tail / no assistant line HOLDS the previous value):

```rust
    // #232: the card's model cell. Same source and cadence as the token
    // reading — the tail the hook already took. Provider is "claude" by
    // construction here: this tail IS a Claude Code transcript. Other
    // providers arrive with their own hook path, not a guess.
    if let Some(raw) = jsonl_tail.and_then(|t| model_from_tail(t)) {
        let short = short_model(&raw);
        changed |= rec.model.as_deref() != Some(short.as_str());
        rec.model = Some(short);
        if rec.provider.as_deref() != Some("claude") {
            rec.provider = Some("claude".to_string());
            changed = true;
        }
    }
```

- [ ] **Step 4: Run** — the three new tests + full `cargo test -p clave` → PASS.
- [ ] **Step 5: Commit** — `feat(hook): model and provider stamped from the transcript tail (#232)`

---

### Task 4: Backfill — model/provider from the jsonl (fresh-install richness)

**Files:**
- Modify: `crates/clave/src/backfill.rs` (the record-building site ~line 265 where `context_tokens: None` sits; the transcript text is already in hand there — find the function that reads each transcript and reuse its text)
- Test: backfill.rs test mod (the `write_transcript` fixture, line ~273)

**Interfaces:**
- Consumes: `hook::model_from_tail`, `hook::short_model` (Task 3), the transcript text backfill already reads for buckets.

- [ ] **Step 1: Write the failing test** (fixture pattern of `a_rotated_row_reads_its_live_conversations_transcript`):

```rust
#[test]
fn backfill_seeds_model_and_provider_from_the_transcript() {
    // write_transcript with a body carrying an assistant line:
    let body = r#"{"type":"assistant","timestamp":"2026-08-26T10:00:00Z","message":{"model":"claude-opus-5","usage":{"input_tokens":10}}}"#;
    // ... (temp claude_dir + store as the neighbouring tests build them)
    // After the backfill under test:
    // assert record.model == Some("opus") && record.provider == Some("claude")
}
```

Fill the scaffolding by copying the neighbouring test verbatim and changing only body + assertions — the fixture helpers are local and short.

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** — at the record-build site, from the transcript text `text`: `let model = crate::hook::model_from_tail(text).map(|m| crate::hook::short_model(&m));` then `model` and `provider: model.is_some().then(|| "claude".to_string())` into the record (backfill reads Claude's own projects tree — provider is "claude" by construction, same argument as Task 3). `pr_number: None, pr_checked: 0, pr_branch: String::new()` — backfill never invents a PR.
- [ ] **Step 4: Run** — `cargo test -p clave backfill` → PASS.
- [ ] **Step 5: Commit** — `feat(backfill): a fresh install seeds model and provider from the jsonl (#232)`

---

### Task 5: The flag's journey — store preference, `clave rows`, launch layout + plugin config

**Files:**
- Modify: `crates/clave/src/store.rs` — `Store` gains `pub row_height: RowHeight` `#[serde(default)]`
- Modify: `crates/clave/src/main.rs` — subcommand `rows [single|double]` (mirror the existing `order` subcommand's wiring end to end: parse → `with_store_mut` → print)
- Modify: `crates/clave/src/setup.rs` — `launch_layout_kdl` (and the swap-template generation it drives) takes the mode; pane sizes come from `mode.target_cols(collapsed)`; the plugin `configuration` block gains `row_height "<value>"` beside the `CLAVE_BINARY_KEY` entry
- Test: setup.rs test mod (the `birth_size` helpers, line ~1290)

**Interfaces:**
- Consumes: `RowHeight` (Task 1).
- Produces: layouts whose pane geometry AND plugin-config mode always agree; `clave rows single` persists and prints "takes effect on the next clave launch".

- [ ] **Step 1: Write the failing tests** (setup.rs, using the existing `birth_size` reader — never `contains`, #181):

```rust
#[test]
fn the_launch_birth_size_follows_the_row_height_mode() {
    // Double (default): the card budgets.
    let expanded = launch_layout_kdl_for("clave", "/w.wasm", None, false, RowHeight::Double);
    let collapsed = launch_layout_kdl_for("clave", "/w.wasm", None, true, RowHeight::Double);
    assert_eq!(birth_size(&expanded, "default_tab_template"), "48");
    assert_eq!(birth_size(&collapsed, "default_tab_template"), "38");
    // Single: byte-identical geometry to the legacy constants.
    let legacy = launch_layout_kdl_for("clave", "/w.wasm", None, false, RowHeight::Single);
    assert_eq!(
        birth_size(&legacy, "default_tab_template"),
        clave_types::BAR_TARGET_COLS.to_string()
    );
}

#[test]
fn the_layout_bakes_the_row_height_key_matching_its_geometry() {
    let kdl = launch_layout_kdl_for("clave", "/w.wasm", None, false, RowHeight::Single);
    assert!(kdl.contains(r#"row_height "single""#), "{kdl}");
    let kdl = launch_layout_kdl_for("clave", "/w.wasm", None, false, RowHeight::Double);
    assert!(kdl.contains(r#"row_height "double""#), "{kdl}");
}
```

(If `launch_layout_kdl`'s signature is reworked in place rather than adding a `_for` variant, update its existing callers and tests mechanically — the compiler lists them; existing size assertions that pin `BAR_TARGET_COLS` move to the `Single` arm or to `Double`'s new values per what they test.)

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Thread `store.row_height` from the launch path into layout generation; sizes from `mode.target_cols(...)` everywhere the layout writes a bar-pane `size` (birth pane AND both swap templates — three sites, the `birth_size` test comment names them). Add the config key line beside the binary key. CLI: `clave rows` with no arg prints the current value; with `single`/`double` persists via `with_store_mut` and prints the takes-effect-next-launch note. Reject other values with the two valid ones in the error.
- [ ] **Step 4: Run** — `cargo test -p clave setup rows` and full workspace → PASS.
- [ ] **Step 5: Commit** — `feat(cli): clave rows — the row-height flag bakes geometry and config together (#232)`

---

### Task 6: The bar learns its mode — plugin config → model, mode-aware width asks

**Files:**
- Modify: `crates/clave-bar/src/plugin_config.rs` — `pub fn resolve_row_height(config: &BTreeMap<String, String>) -> RowHeight`
- Modify: `crates/clave-bar/src/main.rs` — read it in `load()` where `resolve_binary` is read; store on the plugin state; pass into the model
- Modify: `crates/clave-bar/src/model.rs` — `Model` gains `row_height: RowHeight` (constructor threads it; default `RowHeight::default()` in test constructors); every place the model names a target width asks the mode: line ~1969 (`widths_at`'s hydration check compares against `self.row_height.target_cols(true)`), and the width-machine target sites at lines ~5361–5362 (`EXP_W`/`COL_W` become mode lookups — grep `BAR_TARGET_COLS` in model.rs for the full set)
- Test: plugin_config.rs + model.rs test mods

**Interfaces:**
- Consumes: `RowHeight::from_config_value`, `ROW_HEIGHT_KEY` (Task 1).
- Produces: `Model::row_height: RowHeight` (pub(crate) or via constructor) — Tasks 8–9 read it; `resolve_row_height` for main.rs.

- [ ] **Step 1: Failing tests:**

```rust
// plugin_config.rs
#[test]
fn resolve_row_height_reads_the_key_and_defaults_double() {
    let mut c = BTreeMap::new();
    assert_eq!(resolve_row_height(&c), RowHeight::Double);
    c.insert(clave_types::ROW_HEIGHT_KEY.into(), "single".into());
    assert_eq!(resolve_row_height(&c), RowHeight::Single);
    c.insert(clave_types::ROW_HEIGHT_KEY.into(), "garbage".into());
    assert_eq!(resolve_row_height(&c), RowHeight::Double);
}

// model.rs — beside the existing widths_at tests
#[test]
fn hydration_collapse_detection_follows_the_mode() {
    // A double-mode bar born at 38 cols is a collapsed bar awaiting
    // hydration; 30 (the single-mode collapsed target) must NOT trigger it.
    let mut m = model_with_row_height(RowHeight::Double); // small test ctor helper
    m.awaiting_hydration = true;
    assert_eq!(m.widths_at(38), Widths::COLLAPSED);
    assert_eq!(m.widths_at(30), m.widths());
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** — `resolve_row_height` is one line over `from_config_value`. Thread through main.rs `load()`. In model.rs, replace each raw constant read with `self.row_height.target_cols(collapsed)`; the grep list is the contract — zero raw `BAR_TARGET_COLS`/`COLLAPSED_TARGET_COLS` reads remain in model.rs when done. Log the resolved mode once at load (`eprintln!` is the plugin's log channel): `row_height: double (from plugin config)` / `... (default; key absent)`.
- [ ] **Step 4: Run** — `cargo test -p clave-bar` AND `cargo build -p clave-bar --target wasm32-wasip1` → PASS.
- [ ] **Step 5: Commit** — `feat(bar): the bar reads its row-height mode from plugin config (#232)`

---

### Task 7: Row data — the model projects `model`/`provider`/`pr`/`branch`/`elapsed` into `RowContent`

**Files:**
- Modify: `crates/clave-bar/src/render.rs` — `RowContent::Agent` (line ~286) gains `model: Option<String>`, `provider: Option<String>`, `pr: Option<u32>`, `branch: String` (empty = default checkout, blank cell), `elapsed: Option<String>`; `RowContent::Terminal` (line ~310) gains `pr: Option<u32>`, `elapsed: Option<String>`
- Modify: `crates/clave-bar/src/model.rs` — `agent_content` (line ~1884) fills them from the wire `Agent`; `terminal_content` (line ~1815) borrows `pr` from the same prefix-matched store row it already borrows provenance from; new pure fn `elapsed_label`
- Test: model.rs test mod

**Interfaces:**
- Consumes: wire fields (Task 2).
- Produces: the exact `RowContent` shape Task 8's card renderer consumes; `pub(crate) fn elapsed_label(now: u64, then: u64) -> Option<String>`.

Elapsed design (the KISS reading of the spec): `now` enters the model the way the snapshot's `today` already does — the SHELL stamps it. `main.rs` passes `wall_now()` (a `std::time::SystemTime::now()` wrapper; WASI provides the clock — VERIFY in the Task 11 sandbox drive, it is on the checklist) into the render call path, and the existing `TERM_POLL_SECS` timer cadence (main.rs line ~135) already re-renders, which keeps the minutes honest. Branch for agents: `a.branch`, blanked when it equals the repo's default (`a.default_branch`, falling back to the `main`/`master` heuristic exactly as the provenance glyph decides `Prov::Main` — same rule, same site, share the predicate). Terminal elapsed: `None` for now — no wall-clock source exists for tab activity and the bar never invents a measurement (deviation from the mock's `7m`, noted for the maintainer in the PR body).

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn elapsed_label_is_coarse_and_never_invented() {
    assert_eq!(elapsed_label(1000, 0), None); // then=0: never interacted
    assert_eq!(elapsed_label(100, 100).as_deref(), Some("0m"));
    assert_eq!(elapsed_label(100 + 59, 100).as_deref(), Some("0m"));
    assert_eq!(elapsed_label(100 + 5 * 60, 100).as_deref(), Some("5m"));
    assert_eq!(elapsed_label(100 + 2 * 3600, 100).as_deref(), Some("2h"));
    assert_eq!(elapsed_label(100 + 3 * 86_400, 100).as_deref(), Some("3d"));
    assert_eq!(elapsed_label(100 + 2 * 604_800, 100).as_deref(), Some("2w"));
    assert_eq!(elapsed_label(50, 100).as_deref(), Some("0m")); // clock skew: clamp, don't panic
}

#[test]
fn agent_content_carries_the_card_fields() {
    // Build a wire Agent as neighbouring agent_content tests do; set
    // model/provider/pr_number/branch/default_branch/last_interacted;
    // assert the RowContent::Agent fields, including branch BLANKED when
    // it equals the default branch.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement:**

```rust
/// m → h → d → w, each unit taking over at 1.0 of itself; sub-minute is
/// "0m". `then == 0` is "never", which renders blank — the bar never
/// invents a measurement.
pub(crate) fn elapsed_label(now: u64, then: u64) -> Option<String> {
    if then == 0 {
        return None;
    }
    let s = now.saturating_sub(then);
    Some(match s {
        0..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        86_400..=604_799 => format!("{}d", s / 86_400),
        _ => format!("{}w", s / 604_800),
    })
}
```

Fill the `RowContent` fields in `agent_content`/`terminal_content`; thread `now: u64` down from the shell (render + click paths take it, or the model stores it on a `tick(now)` the shell calls before rendering — pick whichever touches fewer signatures, document the choice at the definition). Compiler drives every `RowContent::Agent{..}` literal in tests (add `model: None, provider: None, pr: None, branch: String::new(), elapsed: None`).
- [ ] **Step 4: Run** — `cargo test -p clave-bar` → PASS (single-line goldens untouched: they don't render the new fields).
- [ ] **Step 5: Commit** — `feat(bar): row content carries the card's data (#232)`

---

### Task 8: The card renderer — `card.rs`, geometry ported from the ratified example

**Files:**
- Create: `crates/clave-bar/src/card.rs` (declare `pub(crate) mod card;` in lib.rs)
- Modify: `crates/clave-bar/src/render.rs` — expose to `card.rs` (same crate, `pub(crate)`) what the example mirrored privately: `SEL_BG` is `theme.sel_bg`, recession/fade consts, `clip_to_cells`
- Test: `card.rs` test mod — the goldens, in the example's per-line width-assertion style

**Interfaces:**
- Consumes: `RowContent` fields (Task 7), `Theme`, `display_cells`/`cell_slice`/`strip_sgr`, `RowStatus::mark(&Theme)`.
- Produces: `pub(crate) fn render_card(row: &Row, cols: usize, any_selected: bool, theme: &Theme) -> (String, String)` — two strings, each EXACTLY `cols` display cells.

**The geometry is a PORT, not a redesign.** Source of truth: `render_f_pair` in `crates/clave-bar/examples/double-preview.rs` (committed at this branch). Port it cell-for-cell with these translations and NO others:

| Example | card.rs |
|---|---|
| `PALETTE[i].0` | `theme.palette[i]` via render.rs's `hue(ink, theme)` (out-of-range → `theme.untinted`) |
| `BASE` / `SEL_BG` / `CHIP_INK` / `DEFAULT_INK` | `theme.base` / `theme.sel_bg` / `theme.chip_ink` / `theme.default_ink` |
| `r.status.mark(&Theme::default())` | `row`'s status mark via the theme parameter |
| `FADE` (0.25) / `DORMANT_FADE` | render.rs's own recession constants (single-source them; if private, make `pub(crate)`) |
| mock `Provider::mark()` | `provider_mark(p: &str) -> Option<(char, Rgb)>`: `"claude"` → `('\u{ec82}', Rgb(0xD9,0x77,0x57))`, `"openai"` → `('\u{ec81}', Rgb(0x10,0xA3,0x7F))`, anything else → `None` (blank cell — open strings) |
| mock `tok_ink` | the REAL ramp: ink from `context_level` via the existing battery-band colours (render.rs owns them; reuse, don't re-approximate) |
| `zebra_paint: bool` | the card's index parity in the viewport slice, computed by the caller (Task 9) |
| fixed budgets | `CHIP_W=7, REPO_W=9, MODEL_W=6, BRANCH_MIN=9` as card.rs consts; branch cell exists iff `cols >= 48` (the expanded budget) — collapsed renders no branch, per the ratified example |

Keep the example's exact repo+branch collective algorithm (round 9b), the chipless-flex rule, the TERM pill, the glass `\u{1b}[49m` discipline, and the `╭`/`╰` alternating `BRACKET_A`/`BRACKET_B` inks (fixed consts like the status marks — spec decision). Every glyph `\u{...}`.

- [ ] **Step 1: Write the golden tests FIRST** — a `fleet()` fixture in card.rs's test mod mirroring the example's 16 mock rows (same variant corners: coloured/dark/absent pill, all three provenances, PR on agents and terminals, both providers + unknown-provider blank, failed/dormant/selected, repo and branch names short / exactly-9 / overflowing), then:

```rust
#[test]
fn every_card_line_is_exactly_cols_wide_in_both_profiles() {
    for cols in [38, 48] {
        for (i, row) in fleet().iter().enumerate() {
            let (l1, l2) = render_card(row, cols, true, &Theme::default());
            for l in [&l1, &l2] {
                assert_eq!(display_cells(&strip_sgr(l)), cols, "row {i} at {cols}");
            }
        }
    }
}

#[test]
fn the_selected_card_pins_its_cells() {
    // The CLV-M2 analogue: strip SGR and assert the exact text of both
    // lines at 38 and at 48 — chip label, summary truncation point, token
    // count right-aligned, repo·branch·pr layout, model, elapsed. This is
    // the picture-pin; write the expected strings by running the example
    // and copying its output for the matching mock row.
}

#[test]
fn glass_rows_reassert_default_bg_and_selection_paints_sel_bg() {
    let (l1, _) = render_card(&fleet()[0], 38, true, &Theme::default()); // unselected
    assert!(l1.contains("\u{1b}[49m"));
    assert!(!l1.contains(&Theme::default().sel_bg.bg()));
    let sel = fleet().into_iter().find(|r| r.selected).unwrap();
    let (s1, _) = render_card(&sel, 38, true, &Theme::default());
    assert!(s1.contains(&Theme::default().sel_bg.bg()));
}

#[test]
fn collapsed_renders_no_branch_and_expanded_shares_the_collective_budget() {
    // A row with repo "clave", branch "drive-launch": at 38 the branch
    // string is absent from the stripped line; at 48 it appears IN FULL
    // one space after "clave" (the round-9b rule), and a 14-cell repo
    // truncates to 9 with ellipsis while its branch keeps >= 9 cells.
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p clave-bar card` → FAIL (`render_card` undefined).
- [ ] **Step 3: Port the implementation** from the example per the table above. Order within the function follows the example exactly (line 1 then line 2). Instrument: a `#[cfg(not(test))] eprintln!` behind a `fn card_log` helper is NOT wanted per-render (hot path) — instead log once per render pass in Task 9. No logging inside `render_card`.
- [ ] **Step 4: Run** — card tests + `cargo build -p clave-bar --target wasm32-wasip1` → PASS.
- [ ] **Step 5: Commit** — `feat(bar): the two-line card renderer, geometry from the ratified example (#232)`

---

### Task 9: The seam converts — `render_rows` dispatch, viewport in card units, click `/2`

**Files:**
- Modify: `crates/clave-bar/src/render.rs` — `render_rows` (line ~596) gains `row_height: RowHeight`; in `Double` it slices the viewport with a row budget of `height / 2`, renders via `render_card` (zebra parity = viewport-slice index parity), pushes both lines per card, and clips each to `cols` below the card floor; odd remainder line stays ABSENT from the output (the pane's last line simply isn't written — blank by omission)
- Modify: `crates/clave-bar/src/model.rs` — `click` (line ~2108): `let line = line / self.row_height.lines_per_row();` before the existing viewport arithmetic, and `pane_height` becomes `pane_height / lines_per_row` in the same expression — BOTH conversions at the boundary, `viewport_top` itself untouched and still row-unit (#148: one copy of the arithmetic)
- Modify: `crates/clave-bar/src/main.rs` — the render call (line ~918) passes `self.model.row_height`; the click call (line ~857) is already in raw lines (model converts)
- Test: render.rs + model.rs test mods

**Interfaces:**
- Consumes: `render_card` (Task 8), `Model::row_height` (Task 6), `viewport_top` (unchanged).
- Produces: `render_rows(rows, cols, height, widths, theme, row_height) -> Vec<String>` — existing single-line callers/tests pass `RowHeight::Single` and stay byte-identical.

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn double_mode_budgets_half_the_lines_and_blanks_the_odd_remainder() {
    let rows = fleet(); // the render.rs test fixture
    for height in [0usize, 1, 2, 3, 7, 8] {
        let out = render_rows(&rows, 38, height, Widths::COLLAPSED, &Theme::default(), RowHeight::Double);
        // height/2 cards, two lines each; an odd height emits height-1
        // lines (the last line is blank by omission), and height 1 emits
        // nothing — never half a card.
        assert_eq!(out.len(), (height / 2) * 2, "height {height}");
    }
}

#[test]
fn the_selected_card_is_always_inside_the_double_viewport() {
    // Extend the existing viewport proptest/loop: for every list length,
    // selection index, and pane height 0..=20, the selected row's index is
    // within [top, top + height/2) — same invariant, card units.
}

#[test]
fn a_click_on_either_line_of_a_card_selects_that_card() {
    // Model in Double mode, N rows, pane_height 8: click(0,..) and
    // click(1,..) both land row 0 (+viewport offset); click(6,..) and
    // click(7,..) both land row 3. In Single mode click(3,..) still lands
    // row 3 — the legacy arm unchanged. Assert via the Effect::SwitchTab
    // position, as the existing #148 click tests do.
}

#[test]
fn single_mode_output_is_byte_identical_to_before_the_flag() {
    // The strongest regression guard: the existing goldens ALREADY assert
    // this by passing RowHeight::Single at their call sites. This test is
    // the sentinel that the parameter default path did not fork: render
    // one fixture row both through the old golden string and the new
    // signature and assert equality.
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** Mechanically update every existing `render_rows` call site (render.rs tests lines ~999/2167/2264/2273, dev.rs preview at line ~1643) to pass `RowHeight::Single` — those pin the legacy design; dev.rs's preview grid ADDS the two card geometries `(RowHeight::Double, 38)` / `(RowHeight::Double, 48)` so `bar-preview` shows the new lock (the example's end-state, status doc). Logging (the maintainer's resilience ask), at the render pass level in main.rs: `eprintln!("render: mode={:?} cols={} height={} rows={} top={} shown={}", ...)` gated behind a `fn dbg_log()` that reads an env/config once — and in `click`: `eprintln!("click: raw_line={} lines_per_row={} row_line={} top={} -> key={:?}", ...)` unconditionally (clicks are rare; this line is exactly what debugs the next #148).
- [ ] **Step 4: Run** — full `cargo test --workspace` + wasm build → PASS.
- [ ] **Step 5: Commit** — `feat(bar): render_rows speaks card, viewport and click convert together (#232)`

---

### Task 10: PR numbers — `clave pr-sync`, detached from the hook, TTL-cached

**Files:**
- Create: `crates/clave/src/pr.rs` (declare in lib.rs)
- Modify: `crates/clave/src/hook.rs` — at the end of `run_hook`'s store-write path (line ~983), the staleness check + detached spawn
- Modify: `crates/clave/src/main.rs` — hidden subcommand `pr-sync <uuid>`
- Test: pr.rs test mod

**Interfaces:**
- Produces: `pub fn resolve_pr(run: &dyn Fn(&[&str]) -> Option<String>, repo_root: &str, branch: &str) -> Option<u32>` (pure over an injected runner); `pub fn pr_is_stale(rec: &AgentRecord, now: u64) -> bool`; `pub const PR_TTL_SECS: u64 = 300;`
- Consumes: `pr_number`/`pr_checked`/`pr_branch` (Task 2).

Starship-style discipline (maintainer ruling): external command, strict timeout, silent degradation, never on a hot path. The hook itself runs NO network call — it only compares two integers and, when stale, spawns `clave pr-sync <uuid>` detached (`std::process::Command` with stdin/stdout/stderr null, `spawn()` and drop — the hook returns immediately). `pr-sync` runs `gh pr list --head <branch> --json number --jq .[0].number` with `--repo` derived from the checkout's cwd, under a process-level timeout (kill after 5s), writes the result (INCLUDING a miss: `pr_number = None`, `pr_checked = now`, `pr_branch = branch` — a miss is an answer and must not retrigger for a TTL), bumps seq, pushes the snapshot.

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn resolve_pr_parses_the_number_and_degrades_silently() {
    let hit = |_: &[&str]| Some("204\n".to_string());
    assert_eq!(resolve_pr(&hit, "/r", "drive-launch"), Some(204));
    let empty = |_: &[&str]| Some("".to_string());
    assert_eq!(resolve_pr(&empty, "/r", "drive-launch"), None);
    let dead = |_: &[&str]| None; // gh missing / timed out / non-zero
    assert_eq!(resolve_pr(&dead, "/r", "drive-launch"), None);
    let junk = |_: &[&str]| Some("not a number".to_string());
    assert_eq!(resolve_pr(&junk, "/r", "drive-launch"), None);
}

#[test]
fn pr_staleness_is_ttl_or_branch_change() {
    let mut r = rec("u"); // store test fixture
    r.branch = "drive-launch".into();
    r.pr_checked = 1000;
    r.pr_branch = "drive-launch".into();
    assert!(!pr_is_stale(&r, 1000 + PR_TTL_SECS - 1));
    assert!(pr_is_stale(&r, 1000 + PR_TTL_SECS + 1));
    r.pr_branch = "old-branch".into(); // branch moved: cache is for the wrong question
    assert!(pr_is_stale(&r, 1001));
    let fresh = rec("u2"); // pr_checked = 0: never looked
    assert!(pr_is_stale(&fresh, 1));
}
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement** pr.rs (resolve + staleness + the `run_pr_sync(uuid)` entry that loads the store, resolves via the real `gh` runner, writes, pushes — the real runner is the only untested line-set, kept to a dozen lines). Hook spawn: inside the store-write closure read-only check, spawn OUTSIDE the flock (never hold the store lock across a spawn). `pr-sync` re-checks staleness after acquiring the store (two hooks may both have spawned). Terminals: no record of their own — the bar already borrows PR from the prefix-matched agent row (Task 7); no extra source.
- [ ] **Step 4: Run** — `cargo test -p clave pr` → PASS.
- [ ] **Step 5: Commit** — `feat(cli): pr-sync — starship-discipline PR lookup, TTL-cached, hook-detached (#232)`

---

### Task 11: Docs, lock revision, mutants, sandbox drive, PR

**Files:**
- Create: `docs/superpowers/specs/2026-08-26-double-height-card-lock.md` (the ledger entry / lock revision superseding the affected sections of `2026-07-25-sidebar-visual-design-lock.md`, which gets a superseded-by pointer at top)
- Modify: `UBIQUITOUS_LANGUAGE.md` — **card** (a row's two-line rendering; line 1 status/title/tokens, line 2 identity/model/elapsed), noting row stays the data-side word
- Modify: `crates/clave-bar/examples/double-preview.rs` — header note flips from "candidate explorer" to "preview of the 2026-08-26 lock"; it now renders via the REAL `render_card` (delete the mirrored geometry, keep the mock fleet) so example and renderer can never drift
- Modify: `README.md` ONLY per docs/dev/README-SOP.md if user-facing behavior needs it (`clave rows`)

- [ ] **Step 1:** Write the lock revision: the two profiles' cell tables (copy budgets from card.rs consts), the ratified decisions list (arc alternation IS the zebra; glass; blank-is-the-meaning for prov/branch/PR; rejected paths list carried over from #232 Out of Scope).
- [ ] **Step 2:** Rewire the example onto `render_card`; run it; its width assertions still pass.
- [ ] **Step 3:** `cargo mutants -p clave-bar --file src/card.rs --file src/render.rs -- --workspace false` and `cargo mutants -p clave --file src/pr.rs --file src/hook.rs` (scope to touched files; triage survivors — kill or justify each in the PR body).
- [ ] **Step 4: Sandbox drive** (docs/dev/TESTING.md § the sandbox drive loop; `clave dev instance`, fail-closed on its socket): verify — card renders in both profiles (Alt+c toggles 38↔48), clicks land on the pointed-at card top AND bottom line, selection follow-scroll with halved rows (the #148 lookahead), odd pane height shows no half-card, `wall_now()` ticks elapsed under WASI (if the clock is unavailable in the sandbox wasm, STOP and redesign elapsed to a snapshot-stamped `now` — flagged decision, tell the maintainer), `clave rows single` + relaunch shows the legacy bar, model/provider populate after a Stop hook, PR cell fills within a TTL of a hook on a PR branch. Screenshot round with the maintainer for the final look (the agent cannot see rendered output).
- [ ] **Step 5:** `just gates` green; PR via `gh pr create` — body: spec #232, the terminal-elapsed deviation (Task 7), mutants triage, the live-validation checklist results; labels `bar`, `cli`, `needs-live-validation`. PR body ends with the session link.

---

## Self-Review (done at write time)

- **Spec coverage:** stories 1–2 (model/provider) → T3/T4/T7/T8; 3 (elapsed) → T7; 4/17 (PR) → T10; 5 (tokens ramp) → T8; 6/7 (chipless, TERM) → T8 port; 8/9 (prov ink, blank main) → T8; 10–12 (glass, selection, zebra) → T8; 13–15 (click, viewport, odd line) → T9; 16 (backfill) → T4; 18 (blank cells) → T3/T7/T8/T10; 19 (theme) → T8 table; 20 (open strings) → T2/T3/T8 `provider_mark`; 21 (logging) → T6/T9; 22 (dormant fade) → T8; 23–25 (branch cell, wider flex, two budgets) → T8; 26 (flag) → T1/T5/T6/T9.
- **Known deviation:** terminal-row elapsed renders blank (no honest source) — surfaced to maintainer in Task 11's PR body.
- **Type consistency:** `RowHeight` (T1) consumed by T5/T6/T9; `render_card(row, cols, any_selected, theme) -> (String, String)` (T8) consumed by T9; store fields (T2) consumed by T3/T4/T7/T10; `elapsed_label` name used consistently.
