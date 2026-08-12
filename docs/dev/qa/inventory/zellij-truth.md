# Zellij tab-pane truth (Z1–Z15)

The plane where zellij's own reporting or behaviour diverges from what a
reasonable reader assumes: layout-parsing quirks that produce a wrong pane
tree from valid-looking KDL, geometry actions zellij refuses or warps
silently, dump output that lies, and session targeting that falls back onto
the wrong session. Vocabulary (zellij session vs agent session, tab, pane,
bar instance, store, bind) is
[UBIQUITOUS_LANGUAGE.md](../../../../UBIQUITOUS_LANGUAGE.md). Four standing
facts govern the plane: **`dump-layout` lies about width** (it normalises
splits to 33%/67% whatever the live geometry — trust it for *structure* and
serialized commands only; a maintainer screenshot is the only width oracle);
`dump-screen` returns nothing for plugin panes; **env-var session targeting
fails OPEN** — a drive must go through `scripts/ct.sh`, never
`ZELLIJ_SESSION_NAME=…`; and **a `zellij action` at an absent or dead session
blocks forever and never errors**, so every call needs a liveness gate and a
timeout (ct.sh supplies both). Conventions: `ZLOG="${TMPDIR%/}/zellij-$(id
-u)/zellij-log/zellij.log"` (machine-shared; read only the tail appended
after your log mark); store probes via `clave dev status | jq '.store'`.

### Z1 — `default_tab_template` wraps explicit tabs → two bars (commit 5cb8b17)
**Seam:** the layout generator vs zellij's template application — the template wraps EXPLICIT `tab` nodes too, so a tab node carrying its own bar pane gets a second one from the template.
**Preconditions:** a launch layout whose `tab` node contains a bar pane (any regression in `setup.rs`'s launch composition).
**Reproduce:** 1. Bake a bar pane inside an explicit `tab` node of a sandbox `launch.kdl`. 2. Human cold-starts. 3. Inspect the first tab.
**Healthy:** bare tab node — the template's `children` slot supplies the terminal; one plugin instance per tab; `grep -c 'clave-bar: loaded' $ZLOG` (after the log mark) equals the tab count.
**Broken:** two plugin instances in one tab: a visibly wider bar, scrambled executor election, presenting as a dead `Alt+c`.
**Drive assertion:** post-launch: loaded-line count == tab count, and `scripts/ct.sh dump-layout` shows exactly one clave-bar plugin pane per tab (structure is trustworthy; width is not).
**Guard today:** bare-tab-node rule in the generator (`crates/clave/src/setup.rs:287` comment); the KDL guardrail; RELEASE-RUNBOOK Step 3's double-sidebar check.
**Refs:** commit `5cb8b17`; FOOTGUNS.md "`default_tab_template` wraps EXPLICIT `tab` nodes too".

### Z2 — Nested `children` → bar with no terminal (C1)
**Seam:** zellij's default-template fill path inserts the default terminal pane only at the template's TOP-LEVEL `external_children_index` and does not recurse — a wrapper pane around `children` parses fine and starves every tab of its terminal.
**Preconditions:** `children` nested inside a wrapper `pane` node in `default_tab_template`.
**Reproduce:** 1. Regenerate a sandbox layout with `pane split_direction="vertical" { pane …; children }` as the template body. 2. Cold start; open a new tab.
**Healthy:** `default_tab_template split_direction="vertical" { pane …; children }` — the template node itself carries the split; every tab has bar + terminal.
**Broken:** tabs with a full-screen bar and NO terminal (observed live 2026-07-06); the wrapper keeps `external_children_index=Some(n)` forever in dumps.
**Drive assertion:** `scripts/ct.sh dump-layout` → every tab contains at least one non-plugin pane.
**Guard today:** direct-child rule in both generators, pinned by regression asserts (`crates/clave/src/setup.rs:1195-1199`; `crates/clave/src/add.rs:1209` — `tab_layout` keeps its nested form legitimately: concrete panes, no `children`).
**Refs:** SUBSYSTEM-VALIDATION.md C1 finding 3; vendored `zellij-utils/src/kdl/kdl_layout_parser.rs:1747-1761` (non-recursion at `:1712`).

### Z3 — Sibling panes stack horizontally (C1)
**Seam:** KDL layout defaults — `SplitDirection` defaults to Horizontal, under which `size=N` binds to ROWS; a "left column" written without `split_direction="vertical"` becomes a top strip.
**Preconditions:** any generator emitting sibling panes without the explicit vertical split.
**Reproduce:** drop `split_direction="vertical"` from a generator; regenerate; cold start.
**Healthy:** the bar is a left column at the template's width.
**Broken:** a 26-ROW strip across the top of every tab (observed live 2026-07-06).
**Drive assertion:** hermetic: both generators' tests assert the attribute (`crates/clave/src/setup.rs:1199`, `crates/clave/src/add.rs:1209`). Live confirmation is HUMAN-ONLY (orientation is visual; dump-layout width untrustworthy).
**Guard today:** regression asserts in both generators.
**Refs:** SUBSYSTEM-VALIDATION.md C1 finding 2; vendored `zellij-utils/src/input/layout.rs:2065-2069`, `:1947-1955`.

### Z4 — `resize_pane_with_id` refuses fixed panes (C8) [SANDBOX]
**Seam:** layout-fixed pane sizes vs plugin resize commands — zellij refuses with `CantResizeFixedPanes`, flashing "FIXED!" on the pane; the plugin sees nothing.
**Preconditions:** a bar born from a `size=30` (fixed) layout via an honest fresh-from-layout launch. The bug hid because earlier passes ran on laundered geometry (re-inserted or serialization-rewritten panes get percents).
**Reproduce:** 1. Regenerate a sandbox layout with fixed `size=30`. 2. Cold start. 3. `Alt+c`: the whole chain fires (pipe → toggle → width_seek → ShrinkSelf) and zellij eats the resize silently.
**Healthy:** all three generators emit percent sizes derived from `clave_types::BAR_BIRTH_PERCENT`; the birth-armed width seek converges the percent onto template cols.
**Broken:** bar width never changes on any toggle; "FIXED!" flash (human-visible only).
**Drive assertion:** the honest guard is hermetic: `crates/clave/src/setup.rs:908-914` asserts percent form and the absence of `size=30`. Live width verification is HUMAN-ONLY (dump-layout lies about width).
**Guard today:** percent emission in all generators + those tests.
**Refs:** SUBSYSTEM-VALIDATION.md C8 findings 2026-07-18; `setup.rs:205-229`, `add.rs:144-156`.

### Z5 — Unsuppress re-inserts at 50% (C6 r8) [SANDBOX]
**Seam:** zellij re-INSERTS a shown (unsuppressed) pane instead of restoring its geometry — the bar came back as a 50% split on the WRONG side in every tab that existed at toggle time.
**Preconditions:** the hide/show (suppress) toggle architecture — a forbidden path today.
**Reproduce:** historical, forbidden: reintroduce `hide_self`/show; toggle twice; observe pre-toggle tabs.
**Healthy:** no suppress anywhere — collapse-in-place: `Alt+c` flips a width target and every instance's render-fed seek drives its OWN pane; every instance stays visible and keeps its feedback loop.
**Broken:** bar re-appears right-side at 50% in old tabs; correct only in tabs born after.
**Drive assertion:** HUMAN-ONLY: side/width after a double `Alt+c` (dump-layout cannot adjudicate width). Structural check: no hide/show calls exist (`crates/clave-bar/src/main.rs:290-291` records the rule).
**Guard today:** the collapse-in-place pivot (C6 r20) — suppress calls, move phase, repair map all deleted.
**Refs:** SUBSYSTEM-VALIDATION.md C6 round 8, round 20 verdict.

### Z6 — `suppress_pane` damages swap state (C6 r19) [SANDBOX]
**Seam:** `suppress_pane` → `extract_pane` → `set_is_tiled_damaged()`, and `add_tiled_pane` auto-relayouts only when NOT damaged — so a declared `swap_tiled_layout` can never restore an unsuppressed pane. Other damage-setters: `resize_pane_with_id`, `resize_whole_tab`, close/extract/splits.
**Preconditions:** the swap-layout repair architecture (forbidden path).
**Reproduce:** historical: declare a swap layout matching the template (it parses perfectly, verified with the real 0.44.3 parser), hide then show the bar.
**Healthy:** n/a for current code — the approach is dead; collapse-in-place never suppresses, so swap state is never consulted.
**Broken:** re-show still 50/50 despite a valid, matching swap layout — "parsed perfectly and never fired".
**Drive assertion:** HUMAN-ONLY: none automatable; the guard is the recorded forbidden approach — grep the bar for `suppress_pane` (must be zero call sites).
**Guard today:** forbidden-approach record (the C6 round-19 ledger entry + FOOTGUNS).
**Refs:** SUBSYSTEM-VALIDATION.md C6 round 19; FOOTGUNS.md `suppress_pane` entry.

### Z7 — `show_self()` is a focus action (C6 r14) [SANDBOX]
**Seam:** the server maps `show_self` to `Action::FocusPluginPaneWithId`, which switches to that pane's tab — ten hidden instances calling it is ten racing focus actions.
**Preconditions:** any show path calling `show_self()` across multiple hidden instances (forbidden path).
**Reproduce:** historical: toggle-show with ~10 hidden instances → focus lands on an arbitrary tab, visible churn.
**Healthy:** if a no-focus show is ever needed again: `show_pane_with_id(PaneId::Plugin(own), false, false)` → `UnsuppressOrExpandPane`, which restores without focusing. Today, no show path exists at all.
**Broken:** multi-tab focus scramble on every toggle-show.
**Drive assertion:** HUMAN-ONLY: focus stability across a toggle. Structural: no `show_self` call sites in `crates/clave-bar/`.
**Guard today:** collapse-in-place removed the call; the API escape hatch is recorded for any future show path.
**Refs:** SUBSYSTEM-VALIDATION.md C6 round 14 (source-confirmed: `zellij_exports.rs:2612`/`:2622` at tag v0.44.3).

### Z8 — Hidden instances repaired stale geometry (C6 r11–12) [SANDBOX]
**Seam:** broadcast toggles deliver to every instance, but hidden instances hold stale geometry — their "repairs" moved/resized real panes in other tabs, and every command broadcast more events: a self-sustaining storm.
**Preconditions:** any cross-tab repair-by-command machinery driven by per-instance state (forbidden path). Executor-gating it was NOT enough (round 12) — the announces themselves were the war.
**Reproduce:** historical: toggle with repair armed → bars on random sides/widths, ~15 announces/s for 12s, CliPipe timeouts; the per-instance 16-step budget was the only circuit breaker.
**Healthy:** the repair machinery is deleted; each instance drives only its OWN pane from its own render feedback; a budget bounds any residual movement logic.
**Broken:** pipe storm plus randomized geometry across tabs.
**Drive assertion:** QA-DRIVE phases 5–6: a 12-press collapse burst then 60s idle — evlog, `$ZLOG` tail, and store `seq` all flat after the burst.
**Guard today:** deletion; the width seek's budget as circuit breaker.
**Refs:** SUBSYSTEM-VALIDATION.md C6 rounds 11–12; FOOTGUNS.md "zellij emits NO events for plugin-initiated resizes".

### Z9 — `move_pane…in_direction` is a geometry SWAP (C6 r18) [SANDBOX]
**Seam:** `move_pane_with_pane_id_in_direction` swaps geometries — the landing move hands the bar's shrunk width to the terminal, so a width-then-move ordering races and pumps.
**Preconditions:** the move-phase repair architecture (obsolete post-pivot).
**Reproduce:** historical: width fired while the bar still sat right; the fastest render chain (own tab) always lost the race — 75→30→75 pumping on the focused tab.
**Healthy:** n/a — no move phase exists; nothing repositions panes.
**Broken:** the focused tab's bar pumps between widths and never heals.
**Drive assertion:** HUMAN-ONLY: width stability on the focused tab (every automated width probe is a known liar). Structural: no `move_pane` call sites in `crates/clave-bar/`.
**Guard today:** obsolete by the collapse-in-place pivot; recorded as forbidden.
**Refs:** SUBSYSTEM-VALIDATION.md C6 round 18.

### Z10 — dump-layout reports deepest child (#6) [FIELD]
**Seam:** zellij serializes the LIVE discovered pane process — a pane's deepest child — not the baked layout command; MCP servers, LSPs and `caffeinate` under an agent pane replace `claude` in every dump.
**Preconditions:** an agent whose Claude runs MCP/LSP/keep-awake children (i.e. the maintainer's normal fleet).
**Reproduce:** 1. Wake an agent with an MCP server configured. 2. `scripts/ct.sh dump-layout` / `scripts/ct.sh list-panes -t -j`. 3. The pane reads `uv … run main.py`, `rust-analyzer`, or `caffeinate -i -t 300`.
**Healthy:** liveness never derived from serialized command strings — the store bind is the truth (`open_is_live` prefers the bind, `crates/clave/src/open.rs:40`); panes whose command is not `claude` are treated as **unknown, not mismatched**, printed and marked.
**Broken:** command-string liveness goes blind: a live session offered as a resume candidate (double attach); phantom "mis-binds" invented from a filtered join. Also `start_suspended true` appears on perfectly healthy panes.
**Drive assertion:** QA-DRIVE phase 1 baseline join: print every pane, MARK unresolvables (never filter); assert only `claude`-visible panes are joined to uuids.
**Guard today:** store-bind liveness (`open.rs:40`, test `:200`; `crates/clave/src/add.rs:85` `live_uuid_union`); the join rule in TESTING.md.
**Refs:** #6; FOOTGUNS.md two dump-layout entries; PR #120 live validation.

### Z11 — Un-reaped register child → `<defunct>` panes (C7 r8)
**Seam:** a directly spawned pre-exec pipe child is inherited by the exec'd `claude`, never reaped — the permanent zombie is what zellij's serializer reads as the pane's command.
**Preconditions:** an agent pane whose `clave spawn` fired the register pipe without the double-fork.
**Reproduce:** (pre-fix) spawn any agent; `scripts/ct.sh dump-layout` → `command="<defunct>"` on every agent pane; `ps` shows each `claude` with exactly one Z child.
**Healthy:** register pipe double-forked via `/bin/sh -c '"$@" >/dev/null 2>&1 &'` — sh is reaped, the grandchild reparents to init, the dump shows the real command (`crates/clave/src/spawn.rs:346-390`).
**Broken:** every agent pane defunct in dumps — liveness AND resurrection blind. Zombies from pre-fix agents persist until those panes close.
**Drive assertion:** after a wake: `scripts/ct.sh dump-layout | grep -c defunct` prints `0` (print the number — empty is never a pass).
**Guard today:** the double-fork in `spawn.rs::register_pane`.
**Refs:** SUBSYSTEM-VALIDATION.md C7 round 8; FOOTGUNS.md serializer entry.

### Z12 — Backslash in cwd/label bricks cold start
**Seam:** `\` is KDL's escape introducer — a raw backslash reaching `launch.kdl` through a baked cwd or a prompt-derived label is a layout parse error at session launch, where a dead `attach` blocks forever and the human sees nothing.
**Preconditions:** a first prompt like `fix the \d regex` (label path) or a cwd containing `\` (path path), with a sanitizer regression.
**Reproduce:** hermetically: feed such strings to the generators and parse the artifact with the pinned real parser. Never live — the failure mode is a bricked launch.
**Healthy:** `sanitize_label` filters `"` and `\` (`crates/clave/src/add.rs:106`); `validate_cwd` REJECTS rather than munges (`add.rs:199` — a mangled path points nowhere and `clave spawn` would canonicalize-fail on the lie).
**Broken:** layout parse error at launch; attach blocks indefinitely.
**Drive assertion:** hermetic: unit tests (`add.rs:1477`, `:1485`) + `crates/clave/tests/kdl_guardrail.rs` (real 0.44.3 parser over every generated artifact). Pre-launch live check: `zellij --config <cfg> setup --check`.
**Guard today:** both functions + the guardrail.
**Refs:** FOOTGUNS.md "Backslash is KDL's escape introducer".

### Z13 — Bare `zellij attach` from agent shell hit the live session (C6 r19) [FIELD]
**Seam:** an agent shell runs INSIDE the maintainer's zellij session — bare `zellij` commands inherit that context and mutate his fleet.
**Preconditions:** any agent shell in a clave session; one unguarded `zellij` invocation.
**Reproduce:** DO NOT. Historical record: `zellij attach` variants injected clave-layout tabs into the maintainer's main session, and the injected bar instances renamed his tabs via store-bind tab-id collisions (2026-07-16).
**Healthy:** every drive command goes through `scripts/ct.sh`; session lifecycle (launch/kill, even the sandbox's) is the human's; against his session the agent runs nothing, not even a read.
**Broken:** the maintainer's tabs renamed/rearranged; working state disturbed.
**Drive assertion:** HUMAN-ONLY in effect — the guard is procedural. Mechanical speed bump only: the `.claude/settings.json` denylist matches command TEXT (`zellij kill-session clave` spellings), which any variable or respelling walks past; treat it as the last of three guards, after the per-worktree instance and ct.sh.
**Guard today:** interaction contract (TESTING.md); denylist speed bump; stale store binds self-heal on session recreate.
**Refs:** SUBSYSTEM-VALIDATION.md C6 round 19; FOOTGUNS.md "Bare `zellij` commands…" and the denylist entry.

### Z14 — Env-var session route fails OPEN onto the live fleet (2026-08-07) [FIELD]
**Seam:** zellij's CLI target resolution — with no `--session` flag, the `ActiveSession::One` arm serves the ONLY live session whatever `ZELLIJ_SESSION_NAME` says (`src/commands.rs:407-452`, tag v0.44.3). The env var is a preference, not a boundary.
**Preconditions:** agent shell inside the live session (inherits `ZELLIJ`, `ZELLIJ_PANE_ID`); the sandbox dies mid-drive (another agent's reset, a crash, a kill).
**Reproduce:** DO NOT. Historical: env-only targeting + a killed sandbox put a sandbox-built debug bar into the maintainer's live session as a real pane and sent ten `clave-toggle` pipes to his fleet; the tell — ONE bar instance reported where twelve were expected — was rationalised away.
**Healthy:** `scripts/ct.sh` on every command: proves the socket exists under `${TMPDIR%/}/zellij-$(id -u)/contract_version_*/` (trailing-slash normalised — the naive interpolation refused everything on macOS, PR #152), demands a live `zellij --server <socket>` process, clears `ZELLIJ`/`ZELLIJ_PANE_ID`, passes `--session` explicitly, and bounds the client with a 15s timeout (a dead-session `zellij action` otherwise blocks forever). Every call re-runs the guard.
**Broken:** the whole queued drive lands on the daily fleet.
**Drive assertion:** structural (the env-var form is never written) + the instance-count rule each phase: bar-instance count == tab count; a surprising count is a session-identity failure — STOP and prove which session you are talking to.
**Guard today:** `scripts/ct.sh` (fail-closed by construction).
**Refs:** FOOTGUNS.md "`ZELLIJ_SESSION_NAME=clave-test` is NOT a safety boundary"; `scripts/ct.sh`; PR #152.

### Z15 — zellij recycles tab ids (max+1)
**Seam:** `get_new_tab_id` is `tabs.keys().last() + 1` over a BTreeMap — closing the HIGHEST-id tab hands that id to the next tab created, so any state latched on a tab id outlives its tab.
**Preconditions:** two steps, in order: close the highest tab, THEN create one. Closing alone proves nothing about reuse (five closes in a row never recycled an id).
**Reproduce:** 1. `scripts/ct.sh go-to-tab <highest>` then `scripts/ct.sh close-tab`. 2. `scripts/ct.sh new-tab`. 3. Join store against tabs.
**Healthy:** the recycled id carries nothing inherited — no dead agent's bind, no stale `tab_order` stamp; the new tab sorts as a newborn.
**Broken:** the new tab inherits the dead row's stamp/bind — it sinks below every dormant row (the deterministic B14 latch) or wears a dead agent's glyph/label.
**Drive assertion:** QA-DRIVE phase 3: after close-highest + create, the new tab's id is absent from every store bind and its ordering entry is fresh (`clave dev status | jq '.store.agents[].tab_id, .store.tab_order'`).
**Guard today:** prune correctness is the load-bearing dependency (P12/P13; store test `crates/clave/src/store.rs:911`); drive-loop step 7 forces the two-step shape scripted rounds drift away from.
**Refs:** FOOTGUNS.md "zellij RECYCLES tab ids" (vendored `zellij-server/src/screen.rs:1617`, tag v0.44.3); #6.
