# Plugin Binary Path (#44) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop `clave-bar` resolving the CLI through `PATH`, so a session's bar
can only ever invoke the binary that launched it.

**Architecture:** The absolute binary is emitted as a zellij plugin
*configuration* key, `clave_binary`, into **both** halves of zellij's plugin
identity pair — the layout's `plugin` node and every `MessagePlugin` keybind.
Zellij matches pipe destinations on `(location, configuration)` exactly
(`zellij-server/src/plugins/wasm_bridge.rs:1676-1686`), so both sides must carry
it or keybinds miss and **spawn a duplicate bar**. The bar reads the key at
`load()` and uses it for all seven shellouts.

**Tech Stack:** Rust (workspace: `clave` host CLI, `clave-bar` wasm plugin,
`clave-types` shared vocabulary), zellij 0.44.3 (pinned `=` in Cargo.toml), KDL
4.7.1.

**Design doc:** `docs/superpowers/specs/2026-07-22-plugin-binary-path-design.md`
— read it before starting. It carries the source citations for every claim
below.

> **Executed 2026-07-22 → 2026-07-25; kept as the record of what was planned.**
> Three things diverged during execution, deliberately and with the maintainer's
> agreement — the code, not this plan, is authoritative where they disagree:
> the per-task "commit directly" steps reflect a **session-scoped** blanket
> approval on `fix/plugin-binary-path` only (the standing rule remains: the
> maintainer approves and signs); `binary_resolution_is_anomalous` shipped as
> `(installed, siblings)` after the unused third argument was dropped in review;
> and Tasks 1+2 were merged into a single red→green cycle so no failing commit
> entered history.

## Global Constraints

- **Test with `cargo test --workspace`.** A bare `cargo test` silently skips the
  entire `clave-bar` crate. `--workspace` is load-bearing.
- **Four gates must be green before requesting merge** — run `just gates`:
  `cargo fmt --all --check` · `cargo test --workspace` ·
  `cargo build -p clave-bar --target wasm32-wasip1` ·
  `cargo clippy --workspace --all-targets -- -D warnings` (`--workspace` on
  clippy too — the default form skips the wasm crate). **`fmt --check` was
  missing from this list as originally written**, and CI's lint job runs it
  first — which is exactly how this branch went red with every documented gate
  locally green (corrected 2026-07-25).
- **TDD, red first.** Write the failing test, run it, watch it fail *for the
  right reason*, then implement.
- **NEVER commit without the maintainer's explicit approval.** This overrides
  the commit steps below: prepare the commit, show the diff, ask. He signs via
  1Password. Commit steps in this plan mean *"stage and request approval"*.
- **Never run `just dev-install` or `cargo install`.** They write
  `~/.cargo/bin/clave`, the exact binary the maintainer's live fleet shells out
  to. This caused the v0.1.1 outage.
- **Never launch or kill a zellij session.** Print the command for the human.
- **Dense why-comments.** Explain *why*, citing the issue, the vendored source
  file:line, or the ledger finding — never *what*. Match the surrounding style.
- **Pre-commit PII blocklist** rejects private local paths in staged lines.
  Genericize to `~/…`, `$TMPDIR/…`, `/home/o/…`, `<repo>/…`.
- **Conventional commits** with a `Claude-Session:` trailer.
- Vendored zellij source for verification lives at
  `~/.cargo/registry/src/*/zellij-utils-0.44.3/` and `…/zellij-tile-0.44.3/`.

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/clave-types/src/lib.rs` | shared vocabulary | **add** `CLAVE_BINARY_KEY` — one constant both crates use, so the emitter and the reader can never disagree on the key name |
| `crates/clave/src/setup.rs` | KDL generation | `config_kdl` emits the key in 3 `MessagePlugin` blocks; `layout_kdl` gains a `binary` param and emits it; `launch_layout_kdl` emits it; `write_generated` updated |
| `crates/clave/src/add.rs` | one-shot tab layout | `tab_node` emits the key |
| `crates/clave/src/release.rs` | binary resolution | `runtime_binary()` announces the divergence anomaly |
| `crates/clave/tests/kdl_guardrail.rs` | real-parser guardrail | **new** invariant test + existing tests updated for the new signature |
| `crates/clave-bar/src/main.rs` | plugin shell | `resolve_binary`, `State.clave_binary`, 7 shellout sites |
| `docs/dev/TESTING.md` | live SOP | hot-reload command gains `-c clave_binary=clave` |
| `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` | ledger | same SOP fix |

---

### Task 1: The invariant guard test (RED)

This task lands a test that **fails**, and stays failing until Task 2. That is
intentional — it is the guard the whole design exists to satisfy, and writing it
first proves it can actually detect the broken state.

**Files:**
- Modify: `crates/clave/tests/kdl_guardrail.rs` (append; helpers near the top)

**Interfaces:**
- Consumes: `setup::config_kdl(binary, wasm)`, `setup::launch_layout_kdl(binary, wasm, Option<&AgentRecord>)`, `add::tab_layout(binary, wasm, label, uuid, cwd)` — all existing, unchanged signatures.
- Produces: nothing other tasks consume.

**Why these targets and not `layout.kdl`:** `layout.kdl` is written at
`setup.rs:395` but **never handed to zellij**. `launch_session` passes
`--config config.kdl --layout launch.kdl` (`setup.rs:708-711`); later tabs come
from `add::tab_layout` via `zellij action new-tab --layout` (`open.rs:122`,
`add.rs:726`). Only `doctor` reads `layout.kdl`, and only to check existence.
A test targeting `layout_kdl` would stay green while the live coupling broke.

- [ ] **Step 1: Add the two extraction helpers**

Add near the other helpers at the top of `crates/clave/tests/kdl_guardrail.rs`,
after `assert_config_ok`:

```rust
use std::collections::BTreeMap;
use zellij_utils::input::actions::Action;
use zellij_utils::input::layout::{Run, TiledPaneLayout};

/// Every plugin configuration map in a layout artifact, as zellij's REAL
/// parser sees it. Walks tabs AND the template (the bar pane lives in
/// `default_tab_template` for launch.kdl, and in a concrete tab node for the
/// one-shot add/open layout), recursing through `children`.
///
/// This is half of the #44 identity pair: zellij keys a pipe's destination on
/// `(location, configuration)` exactly
/// (zellij-server/src/plugins/wasm_bridge.rs:1676-1686), and a MISS spawns a
/// new plugin rather than no-op'ing (ibid. :1861-1894) — a duplicate bar.
fn layout_plugin_configs(kdl: &str, what: &str) -> Vec<BTreeMap<String, String>> {
    let layout = Layout::from_str(kdl, format!("guardrail:{what}"), None, None)
        .unwrap_or_else(|e| panic!("{what} did not parse: {e:?}\n---\n{kdl}"));

    fn walk(node: &TiledPaneLayout, out: &mut Vec<BTreeMap<String, String>>) {
        // Run::get_run_plugin (zellij-utils input/layout.rs:404) unwraps both
        // the RunPlugin and Alias arms; we only ever emit `file:` URLs, so it
        // is the RunPlugin arm in practice.
        if let Some(rp) = node.run.as_ref().and_then(Run::get_run_plugin) {
            out.push(rp.configuration.inner().clone());
        }
        for child in &node.children {
            walk(child, out);
        }
    }

    let mut out = Vec::new();
    for (_name, tiled, _floating) in &layout.tabs {
        walk(tiled, &mut out);
    }
    if let Some((tiled, _floating)) = &layout.template {
        walk(tiled, &mut out);
    }
    out
}

/// Every `MessagePlugin` keybind's configuration in a config artifact, as
/// zellij's REAL parser sees it. `KeybindPipe` is what a `MessagePlugin` node
/// becomes (zellij-utils kdl/mod.rs:2196-2209); `configuration: None` means an
/// EMPTY map at match time, not a wildcard (input/layout.rs:159-163).
fn keybind_pipe_configs(kdl: &str, what: &str) -> Vec<BTreeMap<String, String>> {
    let config = Config::from_kdl(kdl, None)
        .unwrap_or_else(|e| panic!("{what} did not parse: {e:?}\n---\n{kdl}"));
    let mut out = Vec::new();
    // Keybinds.0 is the public HashMap the existing unbind test already walks.
    for binds in config.keybinds.0.values() {
        for actions in binds.values() {
            for action in actions {
                if let Action::KeybindPipe { configuration, .. } = action {
                    out.push(configuration.clone().unwrap_or_default());
                }
            }
        }
    }
    out
}
```

- [ ] **Step 2: Write the failing invariant test**

Append to `crates/clave/tests/kdl_guardrail.rs`:

```rust
#[test]
fn keybind_and_layout_plugin_configurations_match() {
    // #44: zellij resolves a pipe's destination by EXACT match on
    // (location, configuration) — a nested HashMap keyed by
    // PluginUserConfiguration (zellij-server plugins/wasm_bridge.rs:1676-1686)
    // — and a miss LAUNCHES A NEW PLUGIN (ibid. :1861-1894). So config.kdl's
    // keybinds and the layout that actually starts the bar must carry the
    // SAME configuration, or every Alt+c/Alt+j/Alt+o press spawns a second
    // sidebar: the exact v0.1.1 field failure this change exists to end.
    //
    // Targets are the layouts zellij is actually GIVEN — launch.kdl
    // (setup.rs:708-711) and the one-shot add/open tab layout (open.rs:122,
    // add.rs:726). layout.kdl is generated but never passed to zellij, so it
    // is checked as a bonus, not as the contract.
    let cfg = setup::config_kdl(BIN_ABS, WASM);
    let kb = keybind_pipe_configs(&cfg, "config.kdl");

    // Non-vacuity FIRST (adversarial review, 2026-07-22): a bare equality
    // assert is satisfied when BOTH sides are empty. If a future zellij added
    // `clave_binary` to PluginUserConfiguration::new's strip list
    // (zellij-utils input/layout.rs:530-546), both sides would silently drop
    // it, this test would stay green, and the bar would revert to PATH
    // resolution forever — the precise failure the design exists to prevent.
    assert!(
        !kb.is_empty(),
        "config.kdl produced no MessagePlugin keybinds — the extraction is \
         vacuous:\n{cfg}"
    );
    for c in &kb {
        assert_eq!(
            c.get(clave_types::CLAVE_BINARY_KEY).map(String::as_str),
            Some(BIN_ABS),
            "a MessagePlugin keybind lacks the {} configuration key; it will \
             address a DIFFERENT plugin than the running bar (#44):\n{cfg}",
            clave_types::CLAVE_BINARY_KEY
        );
    }

    let r = eager_record();
    let launch_eager = setup::launch_layout_kdl(BIN_ABS, WASM, Some(&r));
    let launch_empty = setup::launch_layout_kdl(BIN_ABS, WASM, None);
    let one_shot = add::tab_layout(BIN_ABS, WASM, "lbl", "u-1", "/home/o/x");

    for (what, text) in [
        ("launch.kdl (eager most-recent tab)", &launch_eager),
        ("launch.kdl (empty store, bar-only)", &launch_empty),
        ("add/open one-shot tab layout", &one_shot),
    ] {
        let plugins = layout_plugin_configs(text, what);
        assert!(
            !plugins.is_empty(),
            "{what} carries no plugin pane — extraction is vacuous:\n{text}"
        );
        for p in &plugins {
            assert_eq!(
                p.get(clave_types::CLAVE_BINARY_KEY).map(String::as_str),
                Some(BIN_ABS),
                "{what}'s plugin node lacks the {} key (#44):\n{text}",
                clave_types::CLAVE_BINARY_KEY
            );
            // The pair must MATCH, not merely both be present.
            assert_eq!(
                p, &kb[0],
                "{what}'s plugin configuration differs from config.kdl's \
                 keybind configuration — every keybind press will launch a \
                 SECOND bar (#44):\nlayout={p:?}\nkeybind={:?}",
                kb[0]
            );
        }
    }
}
```

- [ ] **Step 3: Run it and confirm it fails for the RIGHT reason**

```bash
cargo test --workspace --test kdl_guardrail keybind_and_layout_plugin_configurations_match
```

Expected: **FAIL to compile**, with `clave_types::CLAVE_BINARY_KEY` unresolved
(the constant does not exist yet). That is the correct first red — Task 2 Step 1
introduces it, after which this fails on the `assert_eq!` instead. Do not
proceed until you have seen a failure whose message is about the *missing key*,
not about a parse error or a missing import.

- [ ] **Step 4: Do NOT commit — continue straight into Task 2**

Task 1 and Task 2 are **one red→green cycle executed by one implementer**
(maintainer ruling, 2026-07-22). Watching the test fail is the point of
red-first; committing the failure is not. Leave the work uncommitted, proceed to
Task 2's steps, and make the single commit there once the suite is green.

---

### Task 2: Emit `clave_binary` from all four generators (GREEN)

**Executed together with Task 1** — same implementer, same working tree, one
commit at the end covering both.

**Files:**
- Modify: `crates/clave-types/src/lib.rs` (add the constant)
- Modify: `crates/clave/src/setup.rs:70-120` (`config_kdl`), `:156-183` (`layout_kdl`), `:190-220` (`launch_layout_kdl`), `:395` (`write_generated`)
- Modify: `crates/clave/src/add.rs:99-116` (`tab_node`)
- Modify: `crates/clave/tests/kdl_guardrail.rs:157` (`layout_kdl` call site)
- Modify: `crates/clave/src/setup.rs:747`, `:967`, `:1270` (test call sites)

**Interfaces:**
- Consumes: `clave_types::CLAVE_BINARY_KEY` (created in Step 1).
- Produces: `setup::layout_kdl(binary: &str, wasm: &str) -> String` — **signature changed**, previously `layout_kdl(wasm: &str)`. All other emitter signatures unchanged.

- [ ] **Step 1: Add the shared constant**

In `crates/clave-types/src/lib.rs`:

```rust
/// The zellij plugin-configuration key carrying the absolute `clave` binary
/// the bar must invoke (#44).
///
/// Lives in the shared crate because BOTH sides must agree: `clave` emits it
/// into config.kdl's MessagePlugin keybinds and into every layout `plugin`
/// node, and `clave-bar` reads it at `load()`. Zellij matches a pipe's
/// destination on (location, configuration) EXACTLY
/// (zellij-server/src/plugins/wasm_bridge.rs:1676-1686), so a typo on one side
/// silently spawns a second bar instead of erroring.
pub const CLAVE_BINARY_KEY: &str = "clave_binary";
```

- [ ] **Step 2: Emit it from `config_kdl`'s three `MessagePlugin` blocks**

In `crates/clave/src/setup.rs`, `config_kdl`. The `nav` closure becomes:

```rust
    let nav = |payload: &str| {
        // Trailing `;` after the child block is REQUIRED: zellij's KDL parser
        // rejects a `}`-closed node that isn't terminated before the enclosing
        // bind-block `}` (found live in Task 9 C1 — `zellij setup --check`
        // failed on exactly these lines while the `;`-terminated Alt+a/Alt+c
        // forms parsed fine).
        //
        // `clave_binary` must match the layout's plugin configuration exactly
        // (#44): zellij resolves this pipe's destination by (location,
        // configuration) hash-map lookup, and a miss LAUNCHES A SECOND BAR
        // (zellij-server plugins/wasm_bridge.rs:1676-1686, :1861-1894).
        format!(
            "MessagePlugin \"file:{wasm}\" {{ name \"clave-nav\"; payload \"{payload}\"; {key} \"{binary}\"; }};",
            key = clave_types::CLAVE_BINARY_KEY
        )
    };
```

Alt+c:

```rust
    binds.push_str(&format!(
        "        bind \"Alt c\" {{ MessagePlugin \"file:{wasm}\" {{ name \"clave-toggle\"; {key} \"{binary}\"; }}; }}\n",
        key = clave_types::CLAVE_BINARY_KEY
    ));
```

Alt+o:

```rust
    binds.push_str(&format!(
        "        bind \"Alt o\" {{ ToggleTab; MessagePlugin \"file:{wasm}\" {{ name \"clave-organic\"; {key} \"{binary}\"; }}; }}\n",
        key = clave_types::CLAVE_BINARY_KEY
    ));
```

Note the explicit `key = …` named argument rather than bare `{CLAVE_BINARY_KEY}`
inline capture — it is unambiguous and works regardless of edition inline-args
behaviour for path expressions.

- [ ] **Step 3: Give `layout_kdl` a `binary` parameter and emit the key**

Signature and body in `crates/clave/src/setup.rs`:

```rust
pub fn layout_kdl(binary: &str, wasm: &str) -> String {
```

and the plugin node (keep every existing comment above it):

```rust
    format!(
        "// GENERATED by `clave setup` — regenerate, don't hand-edit.\n\
         layout {{\n\
         \x20   default_tab_template split_direction=\"vertical\" {{\n\
         \x20       pane size=\"15%\" borderless=true {{\n\
         \x20           plugin location=\"file:{wasm}\" {{\n\
         \x20               {key} \"{binary}\"\n\
         \x20           }}\n\
         \x20       }}\n\
         \x20       children\n\
         \x20   }}\n\
         \x20   tab name=\"clave\" focus=true\n\
         }}\n",
        key = clave_types::CLAVE_BINARY_KEY
    )
```

Add this why-comment directly above the `plugin` line:

```rust
    // The plugin's configuration is HALF of zellij's identity for it: pipe
    // destinations match on (location, configuration) exactly, so this key
    // must equal what config_kdl bakes into every MessagePlugin keybind or
    // each keypress launches a second bar (#44).
```

Update `write_generated` at `setup.rs:395`:

```rust
    std::fs::write(dir.join("layout.kdl"), layout_kdl(binary, wasm))?;
```

- [ ] **Step 4: Emit the key from `launch_layout_kdl` and `add::tab_node`**

`launch_layout_kdl`'s template plugin node takes the identical child block as
Step 3 (same `key = …` argument; `binary` is already a parameter).

`crates/clave/src/add.rs`, `tab_node` — the raw string becomes:

```rust
    format!(
        r#"    tab name="{label}" focus=true {{
        pane split_direction="vertical" {{
            pane size="15%" borderless=true {{
                plugin location="file:{wasm}" {{
                    {key} "{binary}"
                }}
            }}
            pane cwd="{cwd}" command="{binary}" {{
                args "spawn" "{uuid}" "--name" "{label}" "--cwd" "{cwd}"
            }}
        }}
    }}
"#,
        key = clave_types::CLAVE_BINARY_KEY
    )
```

- [ ] **Step 5: Fix the `layout_kdl` call sites the signature change broke**

Compile to find them, then update each:

```bash
cargo build --workspace 2>&1 | grep -A 3 "this function takes"
```

Known sites: `crates/clave/tests/kdl_guardrail.rs:157`, `crates/clave/src/setup.rs:747`, `:967`, `:1270`. Pass the same binary the neighbouring assertions use (`"clave"` for dev-shaped tests, `BIN_ABS`/`binary` for versioned ones — at `:1270` it must be `binary` so the version-coherence test stays meaningful).

- [ ] **Step 6: Run the invariant test — it must now PASS**

```bash
cargo test --workspace --test kdl_guardrail
```

Expected: PASS, including `keybind_and_layout_plugin_configurations_match` and the pre-existing structural tests (which now prove the added child block still parses through zellij's real parser).

- [ ] **Step 7: Confirm the version-coherence test picked up the new reference**

```bash
cargo test --workspace generated_artifact_set_is_version_coherent
```

Expected: PASS. `layout.kdl` previously carried a version only via the wasm path and now carries the versioned binary too; `versions_in` returns a `BTreeSet`, so a second path resolving to the *same* version keeps `found.len() == 1`. If this fails with `found.len() == 2`, a call site is passing a mismatched binary — fix the call site, not the test.

- [ ] **Step 8: Run all three gates**

```bash
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all green.

- [ ] **Step 9: Stage and request approval**

```bash
git add crates/clave-types/src/lib.rs crates/clave/src/setup.rs crates/clave/src/add.rs crates/clave/tests/kdl_guardrail.rs
```

The maintainer has granted blanket commit approval on the
`fix/plugin-binary-path` branch (2026-07-22), so commit directly. This one
commit covers Task 1's test and Task 2's implementation. Proposed message:

```
feat(clave): bake the resolved binary into plugin configuration (#44)

Emit `clave_binary` into BOTH halves of zellij's plugin identity pair —
every MessagePlugin keybind in config.kdl and every layout plugin node.
Zellij matches pipe destinations on (location, configuration) exactly,
so emitting on one side only would make each keypress launch a second
bar. The key lives in clave-types so emitter and reader cannot drift.

Turns Task 1's red guard green.

Claude-Session: https://claude.ai/code/session_01JZFxedvD7EtDfLFbNKodZt
```

---

### Task 3: The bar reads the key and uses it for all seven shellouts

**Files:**
- Modify: `crates/clave-bar/src/main.rs` — `State` struct `:16-37`, `load` `:342`, and the seven `run_command` sites at `:143`, `:150`, `:162`, `:198`, `:207`, `:390`, `:444`
- Test: `crates/clave-bar/src/main.rs` (unit tests in the same file, matching the crate's existing convention)

**Interfaces:**
- Consumes: `clave_types::CLAVE_BINARY_KEY`.
- Produces: `fn resolve_binary(config: &BTreeMap<String, String>) -> Option<String>` — pure, host-testable.

- [ ] **Step 1: Write the failing unit tests**

Append to the existing `#[cfg(test)] mod tests` in `crates/clave-bar/src/main.rs` (or create one if absent, mirroring `model.rs`'s style):

```rust
#[test]
fn resolve_binary_takes_the_configured_path() {
    let mut c = BTreeMap::new();
    c.insert(
        clave_types::CLAVE_BINARY_KEY.to_string(),
        "/data/clave/bin/clave-v0.1.1".to_string(),
    );
    assert_eq!(
        resolve_binary(&c).as_deref(),
        Some("/data/clave/bin/clave-v0.1.1")
    );
}

#[test]
fn resolve_binary_is_none_when_absent() {
    // A pre-#44 layout or a hand-edited config. The caller falls back to
    // PATH `clave` AND announces it — silence is what hid the v0.1.1 field
    // incident for hours (#44).
    assert_eq!(resolve_binary(&BTreeMap::new()), None);
}

#[test]
fn resolve_binary_treats_empty_as_absent() {
    // run_command(&["", "open", …]) is a worse failure than the fallback.
    let mut c = BTreeMap::new();
    c.insert(clave_types::CLAVE_BINARY_KEY.to_string(), String::new());
    assert_eq!(resolve_binary(&c), None);
}

#[test]
fn resolve_binary_accepts_bare_clave() {
    // The dev/sandbox value is literally `clave` — present and legitimate,
    // so it must be Some (no warning), NOT conflated with the absent case.
    let mut c = BTreeMap::new();
    c.insert(clave_types::CLAVE_BINARY_KEY.to_string(), "clave".to_string());
    assert_eq!(resolve_binary(&c).as_deref(), Some("clave"));
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test --workspace -p clave-bar resolve_binary
```

Expected: FAIL to compile — `cannot find function resolve_binary`.

- [ ] **Step 3: Implement `resolve_binary`**

Add to `crates/clave-bar/src/main.rs`, above `impl ZellijPlugin for State`:

```rust
/// The `clave` binary this bar must invoke, from its zellij plugin
/// configuration (#44).
///
/// `Option`, not `(String, bool)`: the caller needs to distinguish "key
/// absent" (warn — a pre-#44 layout, and we are about to resolve through
/// PATH, which is exactly what broke v0.1.1) from "key present with the
/// value `clave`" (the legitimate dev/sandbox baking, no warning owed). An
/// empty value counts as absent — running `""` is worse than the fallback.
///
/// Pure so it unit-tests on the host: no wasm target, no zellij, no TTY.
fn resolve_binary(config: &BTreeMap<String, String>) -> Option<String> {
    config
        .get(clave_types::CLAVE_BINARY_KEY)
        .filter(|v| !v.is_empty())
        .cloned()
}
```

- [ ] **Step 4: Run the tests — they must pass**

```bash
cargo test --workspace -p clave-bar resolve_binary
```

Expected: 4 passed.

- [ ] **Step 5: Store it on `State` and wire `load()`**

Add the field to `struct State` (`:16`, which is `#[derive(Default)]`):

```rust
    /// The CLI this bar shells out to, from plugin configuration (#44).
    /// ALWAYS assigned in `load()` — `Default`'s empty string is never
    /// observed, because zellij calls `load` before any event.
    clave_binary: String,
```

Change `load`'s signature from `_config` to `config` and add, immediately after
the existing `eprintln!` version marker:

```rust
        // #44: resolve the CLI from plugin configuration instead of PATH. A
        // stale `clave` on PATH previously served a live session's `clave
        // open`, composing tab layouts against the OLD wasm — and because
        // zellij keys plugin identity on location, every such tab loaded a
        // SECOND bar (duplicate sidebar, dead nav).
        self.clave_binary = resolve_binary(&config).unwrap_or_else(|| {
            // LOUD, not silent: the v0.1.1 incident was invisible for hours
            // precisely because nothing announced which binary answered.
            eprintln!(
                "clave-bar: WARNING no `{}` in plugin configuration \
                 (pre-#44 layout, or a hand-edited config) — falling back to \
                 PATH `clave`. A stale binary here is what broke v0.1.1; \
                 regenerate with `clave setup` or `just release`.",
                clave_types::CLAVE_BINARY_KEY
            );
            "clave".to_string()
        });
```

- [ ] **Step 6: Replace the five shellouts inside `run_effects`**

At the top of `fn run_effects(&mut self, effects: Vec<Effect>)` (`:87`), bind
once — this sidesteps borrow conflicts with the arms that mutate `self`, and a
single `String` clone per effect batch is free:

```rust
        // Bound once: several arms below take `&mut self`, so borrowing the
        // field inline would conflict. One String clone per batch is noise.
        let bin = self.clave_binary.clone();
```

Then replace the literal `"clave"` in each of these arms:

- `:143` `Effect::MarkRead` → `run_command(&[bin.as_str(), "focus", &uuid], BTreeMap::new());`
- `:150` `Effect::Bind` → `run_command(&[bin.as_str(), "bind", &uuid, &tab_id.to_string()], BTreeMap::new());`
- `:162` `Effect::PruneTabs` → `let mut argv: Vec<String> = vec![bin.clone(), "prune-tabs".into()];`
- `:198` `Effect::OpenAgent` → `run_command(&[bin.as_str(), "open", &uuid], BTreeMap::new());`
- `:207` `Effect::PersistCollapse` → first element of the array becomes `bin.as_str()`

- [ ] **Step 7: Replace the two shellouts in `update`**

- `:390` (`Event::PermissionRequestResult`) → `run_command(&[self.clave_binary.as_str(), "snapshot"], BTreeMap::new());`
- `:444` (birth touch) → `run_command(&[self.clave_binary.as_str(), "touch", &active_id.to_string()], BTreeMap::new());`

Leave the two `zellij pipe` shellouts at `:105` and `:126` **unchanged** — they
invoke `zellij`, not `clave`, and are explicitly out of scope for #44.

- [ ] **Step 8: Verify no bare `"clave"` shellout survives**

```bash
grep -n '"clave"' crates/clave-bar/src/main.rs
```

Expected: only the fallback string inside `load`'s `unwrap_or_else`, and any
occurrences inside test bodies. **No `run_command` argument.**

- [ ] **Step 9: Run all three gates**

```bash
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all green. The wasm build matters here — this is the crate that ships as wasm.

- [ ] **Step 10: Stage and request approval**

```bash
git add crates/clave-bar/src/main.rs
```

Proposed message:

```
fix(clave-bar): invoke the configured binary, never PATH (#44)

All seven CLI shellouts (focus/bind/prune-tabs/open/collapse/snapshot/
touch) now use the `clave_binary` handed in via plugin configuration.
The load() map was already delivered and discarded.

Fallback to PATH `clave` is retained — stranding a session is worse —
but it now ANNOUNCES itself. Silence is why the v0.1.1 incident went
undiagnosed for hours.

Claude-Session: https://claude.ai/code/session_01JZFxedvD7EtDfLFbNKodZt
```

---

### Task 4: Announce the resolution anomaly in `runtime_binary()`

**Files:**
- Modify: `crates/clave/src/release.rs:63-82`
- Test: `crates/clave/src/release.rs` (existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `fn binary_resolution_is_anomalous(versioned_cli: Option<&Path>, installed: bool, siblings: &[String]) -> bool` — pure predicate, host-testable.

**Why here and not in `setup`:** the divergence happens at *launch*
(`setup.rs:693`) and at tab-bake (`add.rs`, `open.rs`), not only at generation
(`setup.rs:503`). One placement inside `runtime_binary()` covers all three. The
likely trigger is the version-skewed launcher: `runtime_binary()` probes for
`clave-v{CARGO_PKG_VERSION}` — *its own* version — and `baked_binary`'s comment
(`release.rs:57-62`) records that a stable session is cold-started by the **dev**
PATH binary. A 0.1.0 launcher against a v0.1.1 install finds no `clave-v0.1.0`,
bakes bare `clave`, and diverges from the v0.1.1 `config.kdl`.

- [ ] **Step 1: Write the failing test**

Append to `crates/clave/src/release.rs`'s test module:

```rust
#[test]
fn anomalous_only_when_a_versioned_copy_exists_but_is_not_used() {
    use std::path::Path;
    // Dev/sandbox: no versioned copy anywhere. Bare `clave` is CORRECT here
    // (baked_binary's contract) — warning would fire on every sandbox launch
    // and train the reader to ignore it.
    assert!(!binary_resolution_is_anomalous(None, false, &[]));

    // Stable, healthy: the versioned copy exists and is what we resolved.
    let p = Path::new("/data/clave/bin/clave-v0.1.1");
    assert!(!binary_resolution_is_anomalous(Some(p), true, &["clave-v0.1.1".into()]));

    // THE ANOMALY (#44): we are about to bake bare `clave` even though a
    // versioned copy is sitting in the data dir — a version-skewed launcher.
    // config.kdl and launch.kdl will disagree, and every keybind press will
    // spawn a second bar.
    assert!(binary_resolution_is_anomalous(Some(p), false, &["clave-v0.1.0".into()]));
}
```

- [ ] **Step 2: Run and confirm failure**

```bash
cargo test --workspace -p clave anomalous_only_when
```

Expected: FAIL to compile — `cannot find function binary_resolution_is_anomalous`.

- [ ] **Step 3: Implement the predicate**

In `crates/clave/src/release.rs`, beside `baked_binary`:

```rust
/// Is resolving to bare `clave` an ANOMALY rather than the dev/sandbox norm?
///
/// True iff we are falling back to PATH while `<data>/bin/` already holds a
/// `clave-v*` copy. That is the #44 divergence: `config.kdl` was written with
/// one binary and the launch layout is about to bake another, so zellij's
/// (location, configuration) pipe match misses and every keybind launches a
/// second bar. Pure over its inputs so it tests without a filesystem.
pub fn binary_resolution_is_anomalous(
    versioned_cli: Option<&Path>,
    installed: bool,
    siblings: &[String],
) -> bool {
    let _ = versioned_cli;
    !installed && siblings.iter().any(|n| n.starts_with("clave-v"))
}
```

- [ ] **Step 4: Run the test — it must pass**

```bash
cargo test --workspace -p clave anomalous_only_when
```

Expected: PASS.

- [ ] **Step 5: Wire it into `runtime_binary()`**

```rust
pub fn runtime_binary() -> String {
    let dir = crate::setup::data_dir().ok();
    let versioned = dir.as_ref().map(|d| {
        d.join("bin")
            .join(versioned_cli_name(env!("CARGO_PKG_VERSION")))
    });
    let installed = versioned.as_deref().is_some_and(Path::exists);

    // #44: announce the divergence, don't just take it. Falling back to PATH
    // while a versioned copy sits beside us means config.kdl and the launch
    // layout will disagree about plugin configuration — and zellij treats a
    // configuration mismatch as a DIFFERENT plugin, so each keybind press
    // spawns a second bar. Cheap to detect here; invisible in production.
    if !installed {
        let siblings: Vec<String> = dir
            .as_ref()
            .and_then(|d| std::fs::read_dir(d.join("bin")).ok())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        if binary_resolution_is_anomalous(versioned.as_deref(), installed, &siblings) {
            eprintln!(
                "clave: WARNING resolving bare `clave` on PATH, but {} holds \
                 a versioned copy ({}). config.kdl and the launch layout will \
                 disagree and each keybind may open a second bar — run \
                 `clave doctor` (#44).",
                dir.as_ref()
                    .map(|d| d.join("bin").display().to_string())
                    .unwrap_or_else(|| "<data>/bin".into()),
                siblings.join(", ")
            );
        }
    }

    baked_binary(versioned.as_deref(), installed)
}
```

- [ ] **Step 6: Run all three gates**

```bash
cargo test --workspace
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all green.

- [ ] **Step 7: Stage and request approval**

```bash
git add crates/clave/src/release.rs
```

Proposed message:

```
fix(clave): announce PATH fallback when a versioned copy exists (#44)

runtime_binary() probes for its OWN version's copy, so a version-skewed
launcher (the v0.1.1 field scenario) silently bakes bare `clave` into
launch.kdl while config.kdl carries the versioned path — a plugin
configuration mismatch, and a duplicate bar on every keypress.

One placement covers generation, launch and tab-bake. Dev/sandbox bare
`clave` stays quiet: it is correct there.

Claude-Session: https://claude.ai/code/session_01JZFxedvD7EtDfLFbNKodZt
```

---

### Task 5: Fix the hot-reload SOP in both docs

**Files:**
- Modify: `docs/dev/TESTING.md:215`
- Modify: `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md:295`

**Interfaces:** none — documentation only.

**Why this is mandatory, not cosmetic:** `reload_plugin`
(`zellij-server/src/plugins/wasm_bridge.rs:686-697`) resolves targets by the
same `(location, configuration)` match and then loops over the result. With no
`-c`, the command's configuration is empty (`zellij-utils/src/cli.rs:1370-1374`);
once the sandbox layout carries `clave_binary "clave"`, the running plugin's key
is `{clave_binary: "clave"}`, so the lookup misses. **Corrected 2026-07-25:**
a miss is not a silent no-op — `all_plugin_ids_for_plugin_location` returns
`Err(PluginDoesNotExist)` (`plugin_map.rs:169-171`), `reload_plugin` propagates
it (`wasm_bridge.rs:692-693`), and the error branch logs `"Plugin {} not found,
starting it instead"` and **starts a new instance** (`plugins/mod.rs:446-468`).
So a botched reload spawns a second bar. This is the one live mutation an agent
is permitted, and it would break inside this very PR's live-validation pass.

- [ ] **Step 1: Read both current SOP blocks**

```bash
grep -n "start-or-reload-plugin" -B 4 -A 8 docs/dev/TESTING.md docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md
```

- [ ] **Step 2: Update the command in both files**

The invocation becomes (preserve each file's surrounding prose and env-var style):

```sh
ZELLIJ_SESSION_NAME=clave-test zellij action start-or-reload-plugin \
  "file:$CLAVE_DATA_DIR/clave-bar.wasm" -c clave_binary=clave
```

Add this note immediately below it in **both** files:

> The `-c` is load-bearing, not optional. A plugin's configuration is half of
> its zellij identity, and `reload_plugin` matches on `(location,
> configuration)` exactly (`zellij-server/src/plugins/wasm_bridge.rs:686-697`).
> Without it the command matches nothing, the reload loop body never runs, and
> the command still **exits 0** — you would be validating stale wasm while
> believing the reload worked. The sandbox bakes bare `clave` (#44), so
> `clave_binary=clave` is the value there; a stable session would need its
> versioned absolute path. `PluginUserConfiguration`'s `FromStr`
> (`zellij-utils/src/input/layout.rs:563-576`) is comma-separated `key=value`,
> so a path containing a comma would not survive — none of ours do.

- [ ] **Step 3: Verify no other doc repeats the old command**

```bash
grep -rn "start-or-reload-plugin" docs/ AGENTS.md CONTRIBUTING.md CLAUDE.md
```

Expected: every occurrence now carries `-c clave_binary=`. Fix any that do not.

- [ ] **Step 4: Stage and request approval**

```bash
git add docs/dev/TESTING.md docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md
```

Proposed message:

```
docs(clave): hot-reload SOP must pass the plugin configuration (#44)

Plugin configuration is half of zellij's plugin identity, so a reload
with no -c matches nothing, runs an empty loop and still exits 0 —
silently validating stale wasm. The one sanctioned live mutation an
agent may perform would have broken the moment #44 landed.

Claude-Session: https://claude.ai/code/session_01JZFxedvD7EtDfLFbNKodZt
```

---

### Task 6: Verification dossier, handoff, and PR

**Files:**
- Create: `docs/status/YYYY-MM-DD-HHMM-clave-orchestrator.md`
- Already present, must ride this PR: `docs/status/2026-07-22-1845-clave-orchestrator.md`, `docs/superpowers/specs/2026-07-22-plugin-binary-path-design.md`, this plan

- [ ] **Step 1: Run every gate one final time and capture real output**

Use `just gates` — it runs all four in CI's order and its exit status is the
gates' own. If you must capture output through a pipe, set `set -o pipefail`
first: `cargo … | tail` otherwise reports **tail's** status, so a failed gate
records as a pass.

```bash
just gates                       # fmt --check, test, wasm build, clippy
set -o pipefail; just gates 2>&1 | tail -30   # if you need the tail
```

Paste the **actual** output into the PR. Do not paraphrase, and do not claim a gate passed that you did not run.

- [ ] **Step 2: Run both required review lanes**

Per `AGENTS.md`: the vendored fugu review (`.claude/commands/fugu-review.md`, present on this branch) **and** at least one independent adversarial reviewer that did not write the code. State in the PR which lanes actually executed — a lane that did not run is not a lane that passed.

- [ ] **Step 3: Write the handoff**

Cover: what merged, what was discovered (the plugin-identity semantics and the three findings the adversarial review caught), what was **declined and why** (version-skew guard, basename validation, `layout.kdl` removal), and where work stopped. Note the open question of whether to file `layout.kdl` vestigiality as an issue.

- [ ] **Step 4: Open the PR with the dossier**

Must include: the three gate outputs; the risk-class rows this change straddles (*generated artifacts*, *cross-process/IPC*, *install/environment*); the written ordering/idempotency argument the cross-process row demands; the lanes that ran; and the numbered, sandbox-first live-validation steps below. Apply the **`needs-live-validation`** label.

Live steps for the maintainer (he runs these; the agent never launches a session):

1. `clave dev launch` — sandbox only.
2. Open two or three rows.
3. Confirm exactly **one** bar per tab.
4. Press Alt+c, Alt+o, Alt+j, Alt+k. Each must act on the existing bar; **no second sidebar may appear**. This is the specific regression the identity-pair change could cause.
5. `grep 'clave-bar: loaded' "$TMPDIR"/zellij-*/zellij-log/zellij.log` — every line must report the **same** version.
6. Confirm no `WARNING no clave_binary` line appears in that log (it would mean the layout was generated pre-fix).

- [ ] **Step 5: Gate on green CI, then ask before merging**

The autonomy contract permits executing the merge **once the maintainer approves** — never before.

---

## Self-Review

**Spec coverage:** every design section maps to a task — generation → Task 2;
bar → Task 3; error handling → Tasks 3 (bar) and 4 (`runtime_binary`);
hot-reload SOP → Task 5; the invariant and its non-vacuous test → Tasks 1–2;
verification/dossier → Task 6. The three "deliberately out of scope" items are
absent by design.

**Placeholder scan:** no TBD/TODO; every code step carries real code; no "similar
to Task N" references.

**Type consistency:** `CLAVE_BINARY_KEY` is defined once (Task 2 Step 1) and used
by Tasks 1, 2 and 3 under the same path. `resolve_binary` returns
`Option<String>` in its definition (Task 3 Step 3) and every call site.
`layout_kdl(binary, wasm)`'s changed signature is introduced in Task 2 Step 3 and
its call sites fixed in Step 5. Task 1 deliberately references
`CLAVE_BINARY_KEY` before Task 2 creates it — that is the documented first red.
