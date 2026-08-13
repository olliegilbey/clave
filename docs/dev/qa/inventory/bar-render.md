# Bar model/render — per-item specs (B1–B22)

This plane is the sidebar plugin: `crates/clave-bar`'s pure state machine
(`model.rs`) and renderer (`render.rs`), one **bar** instance per zellij tab.
Recurring vocabulary (see UBIQUITOUS_LANGUAGE.md for the rest): the **seek** is
the render-driven loop that drives the bar pane toward a target column count;
**collapse** is the Alt+c width-profile toggle; a **snapshot** is the store's
broadcast of fleet state to every instance; the **executor gate** is the
election (own tab == replicated beacon) that picks the ONE instance allowed to
act on a broadcast; the **evlog** is the JSON event log `clave.log` (sandbox:
`~/.local/state/clave-dev*/state/clave.log`); **EOF twins** are the benign
per-instance `dropped … pipe with empty payload` lines every CLI pipe leaves in
`zellij.log` — corroborating telemetry only, since the log is user-global and a
twin carries no session identity; delivery itself is read from the store. Width claims have known liars:
`dump-layout` normalises to 33%/67%, `dump-screen` is empty for plugin panes,
and the model's own `cols` is belief, not pane truth — a maintainer screenshot
is the only reliable width oracle. Drive commands go through `scripts/ct.sh`
(fail-closed sandbox wrapper); `ZLOG="${TMPDIR%/}/zellij-$(id -u)/zellij-log/zellij.log"`
with a pre-marked line count is the log-read idiom throughout.

### B1 — Width seek blew through target on coarse steps (C6 r9)
**Seam:** seek loop vs zellij's resize granularity — resizes land in ~5%-of-viewport steps, not 1-col.
**Preconditions:** any live bar with the seek armed (birth, Alt+c, peek); window wide enough that 5% ≈ 14 cols.
**Reproduce:** historically: a "shrink while cols > target" loop during collapse repair. Today: `scripts/ct.sh` hot-reload a bar, press `Alt+c`, watch the seek trace (`scripts/seek-trace.sh` header re-enables it).
**Healthy:** seek settles within half a learned step of target (half-step acceptance); trace shows monotone approach.
**Broken:** overshoot straight through target — measured 27→13 cols where target was 26.
**Drive assertion:** unit tier: `cargo test --workspace -p clave-bar` seek suite (16+ tests, step clamp + half-step acceptance). Live width truth is `HUMAN-ONLY: screenshot of bar width after Alt+c`.
**Guard today:** learned step clamped to `MAX_LEARNABLE_STEP = 20` (`crates/clave-bar/src/model.rs:141`), half-step acceptance; extensive unit tests.
**Refs:** SUBSYSTEM-VALIDATION C6 rounds 9/16/17/20; FOOTGUNS "zellij resizes in ~5%-of-viewport steps".

### B2 — ShrinkSelf unreliable; bar grew to ~90% (#137) [FIELD]
**Seam:** `resize_pane_with_id(Decrease, Right)` resolves against whatever neighbour the live layout puts on that edge — the effect's *sign* is a layout property, not an API contract.
**Preconditions:** live fleet (~12 tabs), a bar whose right-edge neighbour differs from the template's; observed on stable v0.1.2, 2026-08-07.
**Reproduce:** `Alt+c` on such a bar. Same instance, same session measured both senses: one episode 79→72→65 on ShrinkSelf; another 65→72→79→…→117, climbing until zellij refused then burning the whole budget at the wall.
**Healthy:** each ShrinkSelf makes the bar narrower; seek converges on target.
**Broken:** bar at ~90% of the terminal; `dump-layout` still reports 33% (it LIES).
**Drive assertion:** `HUMAN-ONLY: screenshot is the width oracle`. The seek trace (instrumented `cols=` per render) can show the wrong-direction walk, but only for VISIBLE instances.
**Guard today:** the seek learns the sign from the observed delta (`model.rs:486` doc comment: "one wrong step teaches the sign"); unit tests.
**Refs:** #137 round 2; FOOTGUNS:157; TESTING.md observability map (dump-layout width lie).

### B3 — Runaway seek passes convergence asserts (#137) [NEAR-MISS]
**Seam:** test-assertion semantics vs seek termination — "budget spent mid-travel" is an accepted terminal state, so endpoint assertions green on a runaway.
**Preconditions:** a wrong-direction defect (B2's shape) reinstated in the model; the pre-#137 assertion set.
**Reproduce:** reinstate a wrong-direction drive (flip the learned sign) and run the old convergence assertions — they greened on a 31-step wrong-direction walk.
**Healthy:** a journey assertion goes red on any reinstated wrong-direction defect.
**Broken:** all "did it converge" assertions pass while the bar walks away from target.
**Drive assertion:** unit tier only — the journey property (the seek never moves further from target more than twice in a row: one overshoot + one beat of latency) in the `model.rs` seek suite; verify any NEW seek assertion against a reinstated defect before trusting it.
**Guard today:** journey assertion in the seek tests; TESTING.md "six shapes of green-and-worthless test" discipline.
**Refs:** #137; FOOTGUNS:159; TESTING.md § escape record.

### B4 — Distance-based acceptance settles off-target under an external mover (#153) [FIELD, OPEN]
**Seam:** the seek's own-resize acceptance uses distance (`|cols − seek_last_cols| <= step`), not provenance — a slow external reflow is indistinguishable from our own resize landing.
**Preconditions:** sandbox, ~12 tabs, seek instrumentation enabled (`scripts/seek-trace.sh`); an external mover reflowing ~one learned step per render (D37's unaccounted mover).
**Reproduce:** 1. `just sandbox`, human launches, prove build (drive-loop step 3). 2. Burst of collapse presses (`Alt+c`) at twelve tabs. 3. Read the trace for the affected bars.
**Healthy:** `cols` reaches `tgt`; `acts` increments when the seek resizes.
**Broken:** verbatim trace: `cols=35 fx=[] acts=132 tgt=54 …` then `cols=27 … acts=132` — `acts` frozen (seek emitting nothing) while `cols` walks 42→35→27 one learned step per render; bar parks off-target.
**Drive assertion:** with the trace enabled: grep the appended `$ZLOG` for the marker; assert `cols` at rest equals `tgt` (bounded wait ~10s after last press). Final width truth: `HUMAN-ONLY: screenshot`.
**Guard today:** **nothing — open.** Discriminator worth building: solicited vs unsolicited (the model knows when it emitted a resize).
**Refs:** #153; #89; LEDGER D37; `model.rs:2007-2037` (`settle_at` doc); FOOTGUNS:160.

### B5 — Stale seek anchor parked the bar (#4)
**Seam:** the drift gate measured against a mid-flight *emit* anchor instead of the accepted rest width, so an interrupt mid-seek left a stale anchor and a later external relayout was misread as our own settle.
**Preconditions:** pure model logic; a seek interrupted mid-flight (target flips while in flight), then an external resize.
**Reproduce:** reproduced as 30→16→6, then an external 26 accepted as settled. Unit-reproducible: the sim harness with the interrupt shape (the original proptest never generated it).
**Healthy:** every settle path pins the anchor to the accepted rest width; a later drift re-arms the seek.
**Broken:** bar parked off-target; drift never re-arms because the stale anchor absorbs it.
**Drive assertion:** `cargo test --workspace -p clave-bar` — `settle_at` paths plus the pinned proptest regression seed.
**Guard today:** `settle_at()` is the single settle helper pinning the anchor (`crates/clave-bar/src/model.rs:2023-2037`); pinned seed.
**Refs:** #4, PR #27; TESTING.md escape record ("stale width-seek anchor"); FOOTGUNS:161.

### B6 — Storm brake refilled off a repaint zellij never sends (#137) [NEAR-MISS]
**Seam:** model correctness keyed on render cadence — zellij NEVER repaints an idle plugin (no clock in the render path), so "refill the allowance after three renders at rest" turns a rate limit into a lifetime budget.
**Preconditions:** any bar; the pre-fix brake refilled only after renders-at-rest that a quiet pane never receives.
**Reproduce:** 33 clean `Alt+c` presses (each spends allowance, none refills it).
**Healthy:** every press gets its journey; the storm ceiling exists only for pathological bursts.
**Broken:** the sidebar stops resizing **forever** — allowance exhausted, no event ever refills it.
**Drive assertion:** unit tier: the six brake tests around `refill_storm_allowance` (`model.rs:2043`, comment at `:175`). Live: after 40+ spaced `Alt+c` presses the bar still toggles — `HUMAN-ONLY: bar still resizes on the 40th press`.
**Guard today:** refill on *arrival* (each press funds itself), six tests; ruling: "laziness may key off renders, correctness may not".
**Refs:** #137, caught on PR #152 review 2026-08-10; FOOTGUNS "Zellij NEVER repaints an idle plugin".

### B7 — Pending-write ledger was a storm engine (#137)
**Seam:** bar ↔ store write loop — the ledger re-asserted user truth on every contradicting snapshot, and each write broadcast another snapshot contradicting newer local state.
**Preconditions:** sandbox fleet, several instances; rapid collapse presses building a snapshot backlog.
**Reproduce:** 10 rapid `Alt+c` presses drove ~26 store writes pre-fix.
**Healthy:** store writes stay proportional to presses (the single re-assert per press), then quiesce.
**Broken:** write amplification — snapshot storm sustained after the presses stop.
**Drive assertion:** record `clave dev status | jq '.store.seq'` before; press `Alt+c` ×10 via keybind (human) or `scripts/ct.sh` pipe `clave-toggle`; wait 10s; assert `seq` delta is small (≈ presses, not ~3× presses), then idle 60s and assert `seq` and the evlog line count both stay put (drive-loop step 6, the anti-storm assertion).
**Guard today:** a contradicting snapshot is **inert** while a write is owed (`model.rs:1138-1143`); re-assert stays once per press (the once-per-burst rationing was tried on PR #152 and withdrawn — an unresolved debt never clears).
**Refs:** #137; FOOTGUNS:162; TESTING.md drive loop step 6.

### B8 — Two rapid collapse writes have no order (#5)
**Seam:** two fire-and-forget `clave collapse` subprocesses race the store — no arrival-order guarantee, so the change-gate can swallow the correct write and push stale truth, stickily.
**Preconditions:** any fleet; two collapse writes in flight at once (double `Alt+c`).
**Reproduce:** press `Alt+c` twice in quick succession; the stale subprocess lands second.
**Healthy:** the store's `collapsed` field ends equal to the user's final parity; bars agree.
**Broken:** store holds the wrong parity and the change-gate refuses the correcting write — bars stuck contradicting the last press.
**Drive assertion:** after a double toggle, bounded wait 5s, then `clave dev status | jq '.store'` — the snapshot `collapsed` flag (`crates/clave-types/src/lib.rs:226`) equals the expected final parity. Visual parity across tabs: `HUMAN-ONLY`.
**Guard today:** `pending_collapse` + `collapse_reasserted` ledger (`model.rs:561-564`, `:1138-1143`), unit-tested.
**Refs:** #5; FOOTGUNS:162; #137 (the ledger's own storm form, B7).

### B9 — "On change only" sustained the flip-flop (#137)
**Seam:** replicated display mode with two writers at different latencies — local press immediate on all instances, store snapshot a round-trip behind, and a value-only change-gate cannot express ordering.
**Preconditions:** snapshot backlog (burst of toggles across a fleet); `heal_collapse` gated on "mode actually changed".
**Reproduce:** toggle burst under backlog — every backlogged snapshot IS a change, so the gate re-arms the seek per snapshot and the heal storms.
**Healthy:** local intent wins until the store confirms it; backlogged snapshots are inert.
**Broken:** heal storm — the flip-flop the gate was cited as preventing, sustained by the gate itself.
**Drive assertion:** same probe as B7: toggle burst then 60s quiescence on `.store.seq` and evlog growth; a functional pass with a growing idle log is a failure.
**Guard today:** local-intent-wins-until-confirmed (the #137 choice); the inert-while-owed rule (B7).
**Refs:** #137; FOOTGUNS:163-164 ("before trusting a change-gate, ask what happens when the comparand is systematically stale").

### B10 — Collapse parity desync across instances (C8) [SANDBOX]
**Seam:** collapse was per-instance memory synced only by the broadcast pipe — a reload or one missed pipe flips one instance forever (background instances get no render feedback to self-correct: a cross-tab double-toggle is net-zero for them).
**Preconditions:** sandbox fleet, ≥3 tabs; one instance reloaded mid-session or one broadcast missed.
**Reproduce:** 1. `just sandbox c8-cold-start`, human launches. 2. Hot-reload ONE bar (`scripts/ct.sh start-or-reload-plugin "file:$SB_DATA/clave-bar.wasm" -c clave_binary=clave`). 3. `Alt+c` toggles.
**Healthy:** all bars collapse/expand uniformly; a reborn instance heals at birth from the snapshot.
**Broken:** one bar pinned at 10% while others sit at 5% — visible, idle, never sinking (observed 2026-07-19/20).
**Drive assertion:** `HUMAN-ONLY: all bars at the same width after a toggle following a reload` (width truth; dump-layout lies).
**Guard today:** `collapsed` rides the seq-gated store snapshot (`clave-types/src/lib.rs:226`) — heal-at-birth and on every push.
**Refs:** SUBSYSTEM-VALIDATION C8 (2026-07-19/20 finding); C6 round 20 known quirk.

### B11 — Tab born expanded in a collapsed fleet
**Seam:** a new tab's bar missed the collapse pipe that predates it — birth state came from the template, not fleet parity.
**Preconditions:** fleet collapsed (`Alt+c`), then a new tab created.
**Reproduce:** `Alt+c` (collapse all), then `Alt+t` (new tab).
**Healthy:** the newborn bar is born collapsed — it hydrates parity from the snapshot and `target_cols_for` (`clave-types/src/lib.rs:368`) seeks the collapsed width.
**Broken:** new bar wide while every other tab shows a strip.
**Drive assertion:** `HUMAN-ONLY: newborn bar matches fleet width`. Model tier: newborn-hydration tests around `target_cols_for` / birth seek.
**Guard today:** closed by B10's snapshot flag (collapse parity persists across birth and launch).
**Refs:** SUBSYSTEM-VALIDATION C6 round 20 ("a tab created while collapsed is born expanded — fix later by carrying the collapsed flag in store snapshots"); B10.

### B12 — No viewport; clicks landed 1–2 rows high (#148) [FIELD]
**Seam:** render vs hit-test — `render` counted from row 0 while the screen showed a window; a mouse click carries a line of the VIEWPORT, not a row of the list. Two symptoms (invisible rows AND click drift), one cause.
**Preconditions:** fleet taller than the bar pane (~40 rows vs pane height; dev scenarios can mint dozens of store rows cheaply).
**Reproduce:** 1. Seed an oversized fleet in the sandbox. 2. Scroll state where `viewport_top > 0`. 3. Click a visible row.
**Broken:** rows past the pane bottom reachable (`Alt+up`) but invisible; clicks land 1–2 rows above the row under the pointer.
**Healthy:** selection always on-screen; a click lands on the row under the pointer; `Alt+1..9` (fleet-row payload `{"row":N}`) is deliberately NOT offset.
**Drive assertion:** unit tier: `viewport_top` goldens plus proptest `the_selection_is_always_inside_the_viewport` (`crates/clave-bar/src/render.rs:602`, `:2189`). Live click accuracy: `HUMAN-ONLY: click row N focuses row N's tab`.
**Guard today:** `render_rows` and `BarModel::click` read the one `viewport_top` function; the shell remembers height at render (a `Mouse` event does not carry it); live-validated 2026-08-11.
**Refs:** #148 (closed); FOOTGUNS:213-214 (incl. the two-clamps mutant-masking note); PR #170.

### B13 — Over-run rows wrapped not clipped (D13) [FIELD]
**Seam:** renderer vs terminal clipping assumption — D13 assumed the terminal clips a too-wide row; it WRAPS it instead.
**Preconditions:** bar pane below `min_intact_cols()` (32 for the EXPANDED profile as of #105, 23 COLLAPSED); rows are two-stage — fixed columns never reflow, so below the floor a built row is deliberately wider than the pane.
**Reproduce:** collapse the pane below the floor (a tab spawned below ~123 total columns is born under the EXPANDED floor; peek/expand also draws EXPANDED through 30-31 cols).
**Healthy:** every row is exactly `cols` cells wide; uniform truncation, no wrap. (The 30-31-col one-frame blink during the grow animation is cosmetic, not this bug.)
**Broken:** a blank second line under every row (observed live 2026-07-29).
**Drive assertion:** unit tier: `clip_to_cells` per-cell asserts (`render.rs:480`, tests `:1374-1418` incl. wide-glyph straddle). Visual: `HUMAN-ONLY: no doubled row height at narrow widths`.
**Guard today:** `render_row` builds at `cols.max(min_intact_cols())`, `render_rows` clips via `clip_to_cells`; per-cell assertions.
**Refs:** LEDGER D13/D16/D17; FOOTGUNS:206; `render.rs::Widths::min_intact_cols` (`:158`).

### B14 — `birth_touched` latches a recycled tab id
**Seam:** a once-ever-per-(instance, tab_id) latch vs zellij recycling tab ids — `get_new_tab_id` is `max+1`, so closing the HIGHEST tab hands its id to the next tab created.
**Preconditions:** sandbox session; the current highest-id tab is closable (non-last).
**Reproduce:** 1. `scripts/ct.sh go-to-tab <highest>`; `scripts/ct.sh close-tab`. 2. Create a new agent tab (`Alt+t` — human, or drive the spawn). 3. Check the new row's sort position. Closing alone proves nothing — reuse takes both steps.
**Healthy:** the new tab receives its birth touch: its store row is stamped and it sorts into the live block.
**Broken:** new tab permanently unstamped → sort key falls to `unwrap_or(0)` → sinks below every dormant row. Deterministic, no race required.
**Drive assertion:** after step 2, bounded wait 5s: `clave dev status | jq '.store'` — the new row's recency stamp is fresh (non-zero, ≈ now). Row placement on screen: `HUMAN-ONLY`.
**Guard today:** drive-loop step 7 (doc step) only; the latch is `model.rs:392` (`birth_touched`), `needs_birth_touch` at `:703`.
**Refs:** FOOTGUNS:153 ("birth_touched latches on the tab ID"); FOOTGUNS:72 (id recycling); TESTING.md drive loop step 7.

### B15 — Position join elected wrong bar, evicted tenant (#55) [FIELD]
**Seam:** two independently delivered zellij frames (`PaneUpdate` manifest, `TabUpdate` tab set) joined BY TAB POSITION — a close renumbers positions, and in the window between frames the join returns a different tab's identity. The join cannot be eliminated: zellij-tile 0.44.3 gives a plugin no own-tab identity.
**Preconditions:** sandbox fleet with ≥3 bound agent tabs.
**Reproduce:** `scripts/ct.sh close-tab` on a **non-last** tab (that is what renumbers positions), repeatedly; re-join store vs pane truth after every provocation, not once at the end.
**Healthy:** `frames_coherent()` holds `own_tab()` at `None` while the frame key-sets disagree; no eviction; next frame is the retry.
**Broken:** the wrong bar elects itself, binds its agent to someone else's tab, and `apply_bind` evicts the rightful tenant — the evlog line `bind-evict` is the direct detector.
**Drive assertion:** after a close burst, grep the sandbox evlog: `grep -c '"cmd":"bind-evict"' ~/.local/state/clave-dev*/state/clave.log` → expected 0 new lines (mark the count first).
**Guard today:** `frames_coherent()` witness (`model.rs:800`), identity resolved in `model.rs` (testable), `bind-evict` logging (`crates/clave/src/store.rs:394-420`). **Residual by construction:** a position-preserving permutation (close lowest + create in the same window) satisfies the witness — one transient mis-bind, self-healed by the seq-gated re-bind.
**Refs:** #55 (closed, PR #120); FOOTGUNS:151; TESTING.md drive loop step 5.

### B16 — Plugin pane shadowed terminal pane id
**Seam:** terminal and plugin pane-id spaces overlap — a numeric pane id alone is ambiguous, so an unfiltered lookup can resolve a plugin pane where a terminal was meant.
**Preconditions:** a fleet where a plugin pane id numerically collides with a terminal pane id (routine — both spaces start low).
**Reproduce:** Repro unknown — detection only: a nav/bind action resolves the wrong tab with no other coherent explanation; the join from uuid → pane → tab lands on a tab whose pane is a plugin.
**Healthy:** `tab_position_of_pane()` considers terminal panes only.
**Broken:** wrong tab resolved for a bind or jump.
**Drive assertion:** unit tier only: the `!p.is_plugin` filter test in `model.rs` (`tab_position_of_pane`, `model.rs:755`; `is_plugin` populated at `main.rs:573`).
**Guard today:** `!p.is_plugin` filter, unit test.
**Refs:** FOOTGUNS:187.

### B17 — Literal glyphs lost in transit, twice in one session
**Seam:** source-diff transit (editor/agent/patch pipeline) silently drops non-ASCII glyph literals — the diff looks clean, production renders tofu, and test literals lose in the same diff so the suite stays green.
**Preconditions:** any change touching a file that spells a glyph as a literal character rather than a `\u{...}` escape.
**Reproduce:** Repro unknown — detection only: tofu in a rendered surface (bar row, tab name, `clave ls`), or a label rendering `x main fix auth` where `x · main · fix auth` was meant (`·` U+00B7 is the label separator, the highest-stakes literal).
**Healthy:** the bar carries every glyph as a `\u{...}` escape (`render.rs`, compliant since #86).
**Broken:** HOST SIDE IS NOT compliant: `Status::glyph()` literals (`crates/clave-types/src/lib.rs:85`), its test assertions, and label-composition sites spelling `·` inline (`crates/clave/src/add.rs:384`, `:946`; `hook.rs` label sites) despite `LABEL_SEP` existing (`clave-types/src/lib.rs:246`).
**Drive assertion:** source probe: `grep -n '·' crates/clave/src/add.rs crates/clave/src/hook.rs` — today non-empty (known debt); the check worth building is a literal-count pin like `plugin_config.rs`'s. Render truth: `HUMAN-ONLY: no tofu in bar rows or tab names`.
**Guard today:** `\u{...}` escapes bar-side; **nothing host-side**.
**Refs:** design lock §5.4; #40; FOOTGUNS:200-202.

### B18 — Blend rounding drifted from ratified design
**Seam:** Python preview vs Rust port — Python `round()` is round-half-to-even, `f64::round` is not, so a naive port shifts one colour channel by one and the render silently stops matching the sign-off.
**Preconditions:** any colour-blend change in `render.rs`. Reachable, not theoretical: fujiWhite faded 25% onto sumiInk3 puts blue on exactly 149.5.
**Reproduce:** replace `round_ties_even` with `round` in `Rgb::mix` — the witness goes red.
**Healthy:** `mix_rounds_ties_to_even` passes; blended channels match the ratified preview byte-for-byte.
**Broken:** one channel off by one; visually near-invisible, design-drift by definition.
**Drive assertion:** `cargo test --workspace mix_rounds_ties_to_even` (`render.rs:1861`; `round_ties_even` at `:192`). Note the original witness did not discriminate — verify a witness against the reinstated defect.
**Guard today:** `round_ties_even` in `Rgb::mix` + a discriminating witness test.
**Refs:** FOOTGUNS:205; LEDGER (design ratification).

### B19 — Render-driven announce → EMFILE on first Alt+c (C6 r13) [SANDBOX]
**Seam:** announce trigger vs render semantics — `render()` is not visibility-gated (every instance renders at least once after load), so a render-driven announce fans out across all fresh instances at once.
**Preconditions:** ~10 bar instances freshly loaded (session birth or reload); announce keyed off render.
**Reproduce:** first `Alt+c` after load on the pre-fix build: 252 announces, 460 events in the final second.
**Healthy:** announces fire only from bounded triggers — BIRTH (an instance's first-ever TabUpdate, once per lifetime) and ORGANIC (Alt+o arming exactly one).
**Broken:** zellij server panic, EMFILE (fd exhaustion; panic site `zellij-utils/src/ipc.rs:388`).
**Drive assertion:** mark `$ZLOG` line count; toggle burst; idle 60s; assert appended announce/EOF-twin volume is bounded (≈ instances per gesture, not hundreds) and the log stops growing at idle (drive-loop step 6). Server survival is its own signal.
**Guard today:** bounded triggers only (birth + organic); toggle bursts set neither flag by construction.
**Refs:** SUBSYSTEM-VALIDATION C6 rounds 12–13; FOOTGUNS:65.

### B20 — `is_active_instance` self-diagnosis poisoned announces (C6 r11–13) [SANDBOX]
**Seam:** hidden instances are event-starved (`TabUpdate` reaches only the active tab), so every hidden instance's stale tab set claims its own tab is active — "am I active" self-diagnosis is poisoned during any event burst.
**Preconditions:** multi-tab fleet; any toggle burst delivering TabUpdates broadly.
**Reproduce:** toggle burst on a build gating any emission on self-diagnosed activity.
**Healthy:** only the executor (own tab == replicated beacon, `nav_executor`, `model.rs:1752`; `elects_confirmed` `:835`) acts on a broadcast.
**Broken:** announce storm — measured ~15 announces/s for 12s; escalates to B19's EMFILE crash in the render-driven variant.
**Drive assertion:** same as B19 (bounded appended log volume, 60s quiescence). Regression tier: `a_beaconless_focus_change_never_leaves_two_nav_executors`, `a_new_tabs_birth_beacon_elects_no_executor_among_starved_bars` (`model.rs:3548`, `:3620`).
**Guard today:** the executor gate is the only trustworthy on-screen signal; the #162 licence exception was tried and removed — the rule now holds with NO exception.
**Refs:** C6 rounds 11–13; FOOTGUNS:63-64; #162.

### B21 — Ungated FocusPane fan-out
**Seam:** a broadcast nav pipe reaches every instance; if each acts, N instances emit N focus actions for one keypress.
**Preconditions:** multi-tab fleet; an Alt-nav pipe (`clave-nav` / `clave-visited`).
**Reproduce:** one nav keypress on a build where jumps are not executor-gated.
**Healthy:** exactly one `SwitchTab`/focus per keypress; zellij log stays clean.
**Broken:** a burst of 12 `Failed to focus stacked pane` lines in `zellij.log` for a single gesture.
**Drive assertion:** mark `$ZLOG`; send one nav pipe (`scripts/ct.sh` action or keybind); assert the appended log contains 0 `Failed to focus stacked pane` lines and the focused tab changed once (`scripts/ct.sh dump-layout` focus truth, or `clave dev status`).
**Guard today:** executor-gated jumps (same gate as B20).
**Refs:** C5/C6; FOOTGUNS:64; P16 in the pipe plane (the 6-target sibling failure).

### B22 — Width runaway recurs on v0.1.3: bar at ~90% on a tab in #178's failure state (#181) [FIELD, OPEN]
**Seam:** the width seek vs a bar whose identity never resolves — first ~90% runaway sighted since B2's fix, on a build where `frames_coherent()` gating (PR #120) holds `own_tab()` at `None` and #178's binds never land.
**Preconditions:** daily-driver-shaped fleet on v0.1.3; the affected tab was one whose bind never landed (#178). Whether the unbound state is causal or coincidental is THE open question.
**Reproduce:** `Repro unknown — detection only:` not yet isolated. The #178 wake-ladder harness is the natural trap: after each wake, read the seek trace for the new tab's bar and get the human width check on the visible instance.
**Healthy:** bar rests at target width (54 expanded / 30 collapsed — clave-types BAR_TARGET_COLS/COLLAPSED_TARGET_COLS) on every tab, bound or not.
**Broken:** sidebar at ~90% of the terminal, agent pane squeezed to a sliver; all three known liars report normality (`dump-layout` 33/67, `dump-screen` empty, model `cols` belief ≠ pane truth).
**Drive assertion:** QA drive phase 2 (wake ladder) gains a width assertion per wake: seek trace `cols` at rest == `tgt` for the affected bar, bounded wait ~10s; final truth `HUMAN-ONLY: screenshot`.
**Guard today:** nothing for this recurrence — B2's learned-sign fix and B3's journey assertion are in place and did not prevent it, which is what makes it a new class rather than a B2 re-run.
**Refs:** #181; #178 (same session, possibly same root); B2/#137 r2 (the shape), B4/#153 (the open mover); FOOTGUNS:157-160.
