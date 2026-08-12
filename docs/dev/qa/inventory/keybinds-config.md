# Keybinds/config — per-item test specs (K1–K8)

This plane is the generated-artifact seam: `clave setup` writes `config.kdl`
(keybinds, unbinds, `session_serialization`) and `layout.kdl` into the data
dir, `clave` composes `launch.kdl` at cold start, and zellij parses all three
at session launch — the worst failure site, because a dead `attach` blocks
forever and shows nothing. Two standing hazards frame everything here:
plugin identity is `(location, configuration)` compared exactly, so any
config/keybind mismatch STARTS A SECOND BAR rather than no-oping; and zellij
live-watches the `--config` file of a running session (~1s poll) while the
on-screen bar keeps its load-time identity. Vocabulary (bar, sandbox, store,
snapshot) is in UBIQUITOUS_LANGUAGE.md. Data dir below: `~/.local/share/clave`
stable, `$(clave dev instance --field data)` for your sandbox.

### K1 — `clave collapse true` could never parse (#5) [SHIPPED BROKEN]
**Seam:** clap-derive's inference vs the plugin's argv — a bare `bool` positional becomes a FLAG (`SetTrue`), so the literal `true`/`false` the bar passes was unparseable, and clap's diagnostic `debug_assert` fires only in debug builds — release was silent.
**Preconditions:** none — pure CLI surface; any scratch store.
**Reproduce:**
1. Debug build e2e: `CLAVE_STATE_DIR=$(mktemp -d) cargo run -p clave -- collapse true` (run OUTSIDE any zellij session — S11: snapshot pushes hit the ambient session).
2. Repeat with `false`.
**Healthy:** both parse and exit 0; the scratch store's `collapsed` flips.
**Broken:** parse error (debug: clap's `debug_assert` trips; release: silent failure — the keybind's subprocess dies and `Alt+c` does nothing).
**Drive assertion:** the two debug-build invocations above exit 0; `jq .collapsed <tmp>/agents.json` reflects the last write.
**Guard today:** `#[arg(action = clap::ArgAction::Set)]` (main.rs:161), parse pin `collapse_cli_parses_absolute_values` (main.rs:610); taxonomy rule — every new CLI surface owes a `try_parse_from` pin plus one DEBUG e2e.
**Refs:** #5, PR #13; `crates/clave/src/main.rs:156-161`, `:610`; TESTING.md escape record; FOOTGUNS.md "clap-derive turns a bare `bool`".

### K2 — `--config` discards the user's zellij config (#122) [FIELD, hidden months]
**Seam:** zellij's config resolution — `Config::try_from` early-returns on `opts.config`, merging clave's file over BUILT-IN defaults and never reading `~/.config/zellij/config.kdl`; the user's keybinds, `default_mode`, `pane_frames`, `ui` all vanish inside every clave session.
**Preconditions:** a user with any non-default zellij config; any clave session. A terminal-level colourscheme masks the theme loss, which is why this hid for months and cost #110 pt2 a misdiagnosis ("user error").
**Reproduce:**
1. Note a distinctive setting in `~/.config/zellij/config.kdl` (e.g. a custom bind, `pane_frames false`).
2. Launch any clave session (human); exercise the setting.
**Healthy (target state, NOT current):** the setting survives — clave's overlay rides as root nodes of the layout file and the user config loads normally.
**Broken (CURRENT SHIPPED STATE):** the setting is inert; the session behaves like stock zellij plus clave's binds. `launch_session` still passes `--config` (setup.rs:857) — the layout-channel ruling (C1, 2026-08-01) is verified but #114 is OPEN and unimplemented, and the "7/7 hostile-config asserts" were a deleted scratch probe: the guardrail assertion is owed, not present. The inventory's "Guard today" column overstates this one.
**Drive assertion:** today: `HUMAN-ONLY: a user-config bind behaves stock inside a clave session`. Post-#114: the guardrail test re-parses layout-over-hostile-user-config with the pinned zellij-utils parser and asserts clave's bind/unbind land AND the user's `default_mode`/`pane_frames`/per-mode bind survive (the probe's seven assertions, recreated in `kdl_guardrail.rs`).
**Guard today:** the C1 ruling + FOOTGUNS entry (diagnosis aid) only; no code guard.
**Refs:** #122 (closed, research), #114 (OPEN, the fix), #110 pt2 (the misdiagnosis); `crates/clave/src/setup.rs:857`; SUBSYSTEM-VALIDATION.md C1 "Config ownership"; zellij-utils `input/config.rs:170-186`.

### K3 — Hot-reload watcher drops the overlay mid-session
**Seam:** zellij's config hot-reload rebuilds from the watched file with `CliArgs::default()` and NO layout — so on the #114 layout route, a user editing their own config mid-session silently drops clave's entire overlay until relaunch, including the `Ctrl q` unbind (stock Quit returns to a live fleet).
**Preconditions:** the #114 layout route LANDED (today the watched file is clave's own generated config, which is K4's hazard instead — the hazard MOVES between the two, it never accumulates); a user editing `~/.config/zellij/config.kdl` during a session.
**Reproduce (post-#114):**
1. Launch a clave session (human).
2. Touch/edit the user's own `config.kdl` (any whitespace change; zellij polls ~1s).
3. Press `Alt+j` and `Ctrl+q`-adjacent keys carefully — or safer, probe: clave nav pipes stop producing focus changes.
**Healthy:** accepted-cost behaviour is DOCUMENTED: overlay gone until relaunch is the known limitation; nothing corrupts.
**Broken:** the same event plus surprise — a fleet where `Ctrl q` is live Quit and every `Alt` bind is dead, with no signal why.
**Drive assertion:** `HUMAN-ONLY: after a mid-session user-config edit, clave binds are gone (expected) and the human knows to relaunch.` A future repair is machine-checkable: `PluginCommand::RebindKeys { write_config_to_disk: false }`, which needs `Permission::Reconfigure` the bar does not hold.
**Guard today:** documented limitation by maintainer decision (C1 ruling, accepted cost); no runtime guard.
**Refs:** SUBSYSTEM-VALIDATION.md C1 (accepted-cost paragraph); zellij-utils `input/config.rs:495-500`, `data.rs:3504-3508`; #114.

### K4 — Regenerating against a LIVE session → second bar [FIELD class]
**Seam:** zellij hot-swaps keybinds from the watched config (~1s) while the running bar keeps its load-time `(location, configuration)` identity — regeneration re-keys the binds to an identity the on-screen bar doesn't have, and the next keypress starts a second bar.
**Preconditions:** a live session whose config file gets rewritten under it — `just release`, `clave setup`, or `clave dev scenario` run while the session is up.
**Reproduce (sandbox only):**
1. Sandbox session live (human launched).
2. Run `clave dev scenario c8-cold-start` against it (this is the forbidden op, staged deliberately; `just sandbox` refuses — bypassing it IS the repro).
3. Press `Alt+c` (human) or send any keybind-path pipe.
**Healthy:** never reached — the guarded paths refuse: `just sandbox` errors while `clave-test` is alive; docs order kill → regenerate → launch.
**Broken:** keybind misses the loaded identity; zellij logs `Plugin … not found, starting it instead` and a SECOND bar pane appears in the tab.
**Drive assertion:** after any regeneration in a drive: `grep -c 'not found, starting it instead'` on the zellij log lines appended since the mark == 0, AND `scripts/ct.sh list-panes -t -j` shows exactly one plugin pane per tab.
**Guard today:** quit-first rule stated in three docs (TESTING.md sandbox lifecycle, FOOTGUNS, RELEASE-RUNBOOK); `just sandbox` refuses against a live sandbox session. Nothing stops a raw `clave setup` against the stable session.
**Refs:** FOOTGUNS.md "zellij live-watches `config.kdl`"; TESTING.md "never regenerate against a LIVE session"; zellij-server `lib.rs:2175` → `screen.rs:717` (Reconfigure path); #44 (the identity mechanism).

### K5 — Inline `bind {…}` fails the parser at launch (C1)
**Seam:** KDL node termination — a `MessagePlugin` child block inside a `bind` needs a node terminator (`;` or newline) after its closing `}`; the inline form parses fine in the KDL abstract but zellij's parser rejects it AT SESSION LAUNCH, where a dead `attach` blocks forever and the human sees nothing.
**Preconditions:** any change to the keybind-generation strings in `setup.rs::config_kdl`.
**Reproduce:**
1. Remove a trailing `;` from the `nav` helper's `MessagePlugin` line (setup.rs:82-85 region) on a scratch branch.
2. `cargo test --workspace` — the guardrail parses every generated artifact with the exact pinned parser.
3. Belt-and-braces live form: `zellij --config <data>/config.kdl setup --check`.
**Healthy:** guardrail red BEFORE any launch; `setup --check` exits 0 on the shipped artifacts.
**Broken:** `Expected valid node terminator.` at launch; the session never creates and attach blocks indefinitely.
**Drive assertion:** `zellij --config "$(clave dev instance --field data)/config.kdl" setup --check` exits 0 (scriptable, no session needed); hermetically, `crates/clave/tests/kdl_guardrail.rs` is the standing gate.
**Guard today:** `kdl_guardrail.rs` (real zellij-utils 0.44.3 + kdl 4.7.1 pinned parsers, all artifacts); the `;` convention documented at the emission site (setup.rs:72-76).
**Refs:** SUBSYSTEM-VALIDATION.md C1 finding 1; `crates/clave/src/setup.rs:72-85`; `crates/clave/tests/kdl_guardrail.rs`; FOOTGUNS.md "A `MessagePlugin` block inside a `bind`".

### K6 — Second `unbind` node silently ignored [NEAR-MISS]
**Seam:** kdl 4.7.1's `KdlDocument::get` returns the FIRST match only — a second `unbind` node in the generated config parses cleanly and is never read, so its keys stay bound with no error.
**Preconditions:** the five unbound keys (`Ctrl g/t/o/b/q`) split across two `unbind` nodes by a future edit.
**Reproduce:**
1. On a scratch branch, split the single `unbind` line (setup.rs:178) into two nodes.
2. `cargo test --workspace` — the count pin goes red.
3. Live symptom if it shipped: `Ctrl+q` (stock Quit) kills the whole fleet session; `Ctrl+t/g/o/b` swallow Claude Code's own keys.
**Healthy:** exactly ONE `unbind` node carrying all five keys; each key stripped from every mode after merge.
**Broken:** silently missing unbinds — the second node's keys behave stock.
**Drive assertion:** `grep -c 'unbind' "$(clave dev instance --field data)/config.kdl"` == 1 AND that line contains all five key literals (scriptable per drive preflight).
**Guard today:** generation emits one node (setup.rs:166-178, mechanism comment inline); pinned by `config_unbinds_claude_code_colliding_keys` asserting `matches("unbind").count() == 1` (setup.rs:1146-1163).
**Refs:** `crates/clave/src/setup.rs:166-178`, `:1146-1163`; FOOTGUNS.md "`kdl` 4.7.1's `KdlDocument::get`"; kdl `document.rs:80`.

### K7 — Partial permission-cache match kills every pipe [FIELD]
**Seam:** zellij plugin permissions are all-or-nothing per plugin — a partial match between the cached grant and the requested set raises an interactive prompt (unanswerable in a narrow bar pane) and withholds the ENTIRE set: every `zellij pipe` times out at 1s, nothing renders.
**Preconditions:** the plugin's requested permission set drifts from what `permissions.kdl` holds (a new permission added in code, or a cache written under only one key form — zellij looks up both `"file:<abs>.wasm"` and `"<abs>.wasm"`).
**Reproduce:**
1. In the sandbox, edit the cache (`~/Library/Caches/org.Zellij-Contributors.Zellij/permissions.kdl` on macOS; `~/.cache/zellij/permissions.kdl` elsewhere) to drop one permission from the sandbox wasm's entry.
2. Human relaunches; drive one pipe: `scripts/ct.sh` any nav action.
**Healthy:** grants pre-seeded under BOTH key forms; pipes deliver (EOF-twin drop lines appear in `zellij.log` — the standing delivery proof); no prompt.
**Broken:** `Action CliPipe did not complete within 1s` on every pipe; a permission prompt wedged in a strip-width pane; bar inert.
**Drive assertion:** preflight (QA-DRIVE phase 0): the cache file contains an entry for the wasm under both key forms with the full current set — `grep -c "$WASM" permissions.kdl` >= 2; post-launch, one probe pipe produces its expected EOF-twin delta in the log.
**Guard today:** `merge_permissions_kdl` seeds both key forms (setup.rs:455-456), `permissions_seeded` probe (setup.rs:485); guardrail parses the cache in both branches (`permissions_kdl_is_well_formed_in_both_branches`, kdl_guardrail.rs:312).
**Refs:** `crates/clave/src/setup.rs:439-486`; `crates/clave/tests/kdl_guardrail.rs:312`; FOOTGUNS.md "Zellij plugin permissions are ALL-OR-NOTHING"; zellij-utils `kdl/mod.rs:5456-5500`.

### K8 — Walked order ≠ displayed order (C1→C5, #112, #100) [FIELD]
**Seam:** the order `Alt+j/k` steps vs the order the bar draws — whenever the two diverge (recency bumps on focus, position-order walks against a recency display), "down" is unpredictable and a walk toggles two tabs forever.
**Preconditions:** ≥3 rows; any ordering rule where focus or transit mutates rank (the C1 ping-pong), or a walk keyed on tab position against a recency-sorted display (C5 r3). Residual trap: `dir` wraps WITHIN one block (live vs dormant, #112's segregation), so a walk from a single live tab bounces in place and reads as a pass.
**Reproduce:**
1. Sandbox with 3+ rows spanning both blocks (QA-DRIVE's `qa-fleet` shape).
2. Pick into the dormant block first (`{"row":N}` pipe or `Alt+N`), then walk: repeated `scripts/ct.sh`-routed `clave-nav` pipes `{"dir":"next"}` ×(rows+1), then `prev` ×2.
3. After each press, join selection/focus against the displayed order (store snapshot order per `rank_desc`).
**Healthy:** each press moves selection exactly one DISPLAYED row, wrapping within its block; the order itself never changes during the walk (focus is not a commitment); N+1 presses land back where they started.
**Broken:** two tabs toggling (the C1 ping-pong); or six racing SwitchTab targets trashing recency (C5 r2); or a one-live-tab walk bouncing on the sender while the store reads "expected" (#148 drive, 160 wasted pipes).
**Drive assertion:** QA-DRIVE phase 4 — per press exactly one focus change (never two: the executor-election property), walk sequence over N rows visits N distinct rows in display order; assert the store's ordinal order is byte-identical before and after the walk.
**Guard today:** commitment-based ordering — rows rank by user-commitment ordinal, focus never reorders (`rank_desc` model.rs:238-248; ordinals in the store, S9); displayed-row stepping executor-gated; `rows_order_by_last_user_commitment` (model.rs:2530) and neighbours. Successor design #179 (one ring over one ordered list) is OPEN — the two-block wrap trap stands until it lands.
**Refs:** #112 (closed), #100, #179 (OPEN); SUBSYSTEM-VALIDATION.md C1 "Open UX observation" + C5 rounds 1-4; `crates/clave-bar/src/model.rs:238`, `:2530`; FOOTGUNS.md "A dir-nav drive from a single live tab".
