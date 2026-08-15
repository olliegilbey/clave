# Pipe delivery — register/bind/nav (P1–P18)

The plane between an emitter — a keybind `MessagePlugin` block, a CLI `zellij
pipe` subprocess, or the hook's fire-and-forget push — and the sidebar plugin
instances (**bar instances**, one per zellij tab) that receive it: routing,
payload arrival, and the **executor election** (exactly one instance may act on
a broadcast; it is named by the last `clave-visited` **beacon**). Vocabulary
(store, snapshot, bind, dormant row, minted uuid) is
[UBIQUITOUS_LANGUAGE.md](../../../../UBIQUITOUS_LANGUAGE.md). Two standing
facts govern every item: **every non-tty `zellij pipe` also delivers one empty
"EOF-twin" message per live instance**, logged as `clave-bar: dropped <name>
pipe with empty payload` — those lines are the only *log trace* of a broadcast
(payload arrivals log nothing) but they are corroborating telemetry, never
delivery proof: the log is user-global and a twin is unattributable to a
session, so delivery is always gated on the payload's own observable (the
store bind, the focus change — see P8's drive assertion). They appear once per
live instance per delivery, in health as well as sickness; and each CLI pipe
blocks the zellij server router ~1s. Conventions below: `ZLOG="${TMPDIR%/}/zellij-$(id
-u)/zellij-log/zellij.log"` (shared machine-wide — mark the line count before
driving, read only the appended tail); store probes go through `clave dev
status | jq '.store'`; every zellij touch goes through `scripts/ct.sh`
(never the env var — it fails open, see zellij-truth Z14).

### P1 — Election starvation: nav dead session-wide (#162) [FIELD]
**Seam:** broadcast delivery (healthy) vs the model-side executor election — the payload lands on every instance and the election declines to act, so nothing runs and nothing is logged.
**Preconditions:** sandbox session, ≥2 agent tabs; the tab named by the last `clave-visited` beacon gets closed, taking the announcing bar with it. Pre-fix (< PR #171) this state was terminal.
**Reproduce:** 1. `just sandbox` + human launches a 2-agent scenario. 2. Nav onto tab 2 (`scripts/ct.sh pipe --name clave-nav -- '{"dir":"next"}'` or human `Alt+j`). 3. `scripts/ct.sh close-tab` (the focused, beacon-named tab). 4. Send one more nav pipe.
**Healthy:** the press after the close moves focus — exactly one focus change. Tier-3 pass recorded 2026-08-11 on build `e646049`.
**Broken:** NO focus change AND no `clave-nav` log line at all. EOF-twin drop lines still appear — they are a control present in health, never the evidence (this cost a day of misdiagnosis as "pipe starvation").
**Drive assertion:** record the `focus=true` tab from `scripts/ct.sh dump-layout` (structure is trustworthy; width is not), send one nav pipe, re-dump within 3s → the focused tab changed (that focus change is the gate). Record the twin delta for forensics only — the log is user-global and never a gate (P8).
**Guard today:** model-side election — `nav_executor` answers the replicated beacon alone (`crates/clave-bar/src/model.rs:1752`), refused re-anchors persist as `reanchor_owed` debt (`model.rs:538`, paid at `model.rs:1407-1416`); tests `a_beaconless_focus_change_never_leaves_two_nav_executors`, `a_new_tabs_birth_beacon_elects_no_executor_among_starved_bars`.
**Refs:** #162 (PR #171); FOOTGUNS.md "Nav is dead in the whole session…"; SUBSYSTEM-VALIDATION.md C5, 2026-08-11 findings; QA-DRIVE phase 4.

### P2 — Fallback licence elected two executors (#162 review) [NEAR-MISS]
**Seam:** executor election vs a beaconless native tab switch — zellij delivers frames only to the newly active bar, so a "navigate on local truth" licence armed on one frame necessarily outlives the focus that earned it.
**Preconditions:** two bars that each witnessed a stranded beacon (`stranded_witnessed`), then a native switch (mouse on zellij's own tab bar) that emits no beacon.
**Reproduce:** caught as a red unit test before shipping; live shape: arm both bars via a close, switch tabs with the mouse, press nav once.
**Healthy:** one press → exactly one focus change; the first press after any beaconless switch walks from the tab the user left (accepted cost), still single-executor.
**Broken:** probe verbatim (C5): `own=Some(11) executor=Some(11) nav=[SwitchTab { position: 1 }, …]` AND `own=Some(12) executor=Some(12) nav=[SwitchTab { position: 0 }, …]` — two executors, two targets, one keypress.
**Drive assertion:** QA-DRIVE phase 4's single-executor check: per nav pipe, exactly one focus change in `dump-layout`, never two in succession without a press.
**Guard today:** the licence is removed entirely — `nav_executor` is the beacon and nothing else; regression test named under P1.
**Refs:** FOOTGUNS.md:64 ("A licence tried and removed, #162"); SUBSYSTEM-VALIDATION.md C5 "The nav fallback that was written instead, and removed".

### P3 — `clave-organic` dead: payload-less pipe unreachable (#128, #100) [FIELD]
**Seam:** a keybind `MessagePlugin` with no `payload` attribute delivers `payload = None`, so the payload-present match in `handle_pipe` is unreachable for it — the guard branch must route by NAME.
**Preconditions:** a dormant row selected (⏎), then the user leaves via `Alt+o` (bound to `ToggleTab; MessagePlugin clave-organic`, payload-less).
**Reproduce:** 1. Select a dormant row (`Alt+N`). 2. `Alt+o` away (human — the gesture is a native keybind; scripting a payload-less CLI pipe is itself the P6 stdin trap). 3. Wait ~18s. 4. `Alt+Enter`.
**Healthy:** the commit refuses — the organic beacon re-anchored and cleared the departed bar's cursor; no `dropped clave-organic pipe with empty payload` line (organic is handled by name before the drop log).
**Broken:** `Alt+Enter` 18s after the switch opened the pre-switch selection; the drop line fired for `clave-organic`.
**Drive assertion:** HUMAN-ONLY: LIVE-INTERACTION-CHECKLIST §5's beacon-gap item (Alt+o then Alt+Enter, commit must refuse). Log corroboration: zero `dropped clave-organic` lines after the gesture.
**Guard today:** the payload-less branch matches names — toggle AND organic (`crates/clave-bar/src/main.rs:308-334`); checklist §5 is the standing detector.
**Refs:** #100, PR #128 rounds 2–3; TESTING.md escape record; FOOTGUNS.md payload-less `MessagePlugin` entry.

### P4 — Tab close strands Alt-nav (#23) [FIELD]
**Seam:** the nav anchor (the beacon's `current_tab`) vs zellij's post-close focus, which no surviving instance observes (`TabUpdate` reaches only the active tab's instance).
**Preconditions:** any session with ≥2 tabs; close one.
**Reproduce:** 1. Open 2+ tabs. 2. Close the focused tab (`Alt+w` live; `scripts/ct.sh close-tab` scripted — non-last). 3. Press `Alt+↑/↓` / send a nav pipe.
**Healthy:** nav answers immediately after the close — one focus change per press.
**Broken:** nav dead until a bar row is mouse-clicked (the click re-seeds the anchor).
**Drive assertion:** QA-DRIVE phase 3: after `close-tab`, one nav pipe → focused tab changes in `dump-layout` within 3s.
**Guard today:** `Effect::ReanchorVisit`, executor-gated, with the #162 retry trigger on either delivered frame (`crates/clave-bar/src/model.rs:69`, debt payment `:1407-1416`); listed as a #47 tier-2 first scenario.
**Refs:** #23; TESTING.md escape record; SUBSYSTEM-VALIDATION.md C5.

### P5 — Organic announce fanned out one pipe per instance (#128 drive) [FIELD]
**Seam:** a broadcast-triggered announce running on N instances × the ~1s per-CLI-pipe router block — self-clearing per instance bounds repeats, not fan-out.
**Preconditions:** N bar instances (N tabs); the organic pipe is a broadcast, so every instance's next TabUpdate saw the armed flag.
**Reproduce:** (pre-fix build) press `Alt+o` with several tabs open; measure keyboard latency.
**Healthy:** exactly one announce per gesture — twin delta after one `Alt+o` == 1 pipe × live instances; residual ~1s single-pipe floor (#141).
**Broken:** one `zellij pipe` subprocess per bar; measured ~2s frozen keyboard per press (2026-08-02).
**Drive assertion:** after one organic gesture (human), record the `dropped clave-visited pipe with empty payload` delta appended to `$ZLOG` for forensics — healthy reads 1×live instances, fan-out's tell is N×instances, but the user-global log makes the delta a hint, never a gate (P8). Freeze feel stays HUMAN.
**Guard today:** gate at EMIT, in the model — the announce fires from `apply_tabs` only on the birth/organic branches, one emitting instance, trigger consumed only on the emitting branch (`crates/clave-bar/src/model.rs:1244-1270`).
**Refs:** FOOTGUNS.md "Each of those 1s CliPipe timeouts BLOCKS the server router"; PR #128 round 3; #141.

### P6 — Payload-less `zellij pipe` streams stdin, wedges the CLI-pipe lane (#128 drive) [SANDBOX]
**Seam:** the zellij CLI pipe client vs script timeouts — with no `--` payload (an empty string `-- ''` counts as absent) the client streams stdin; killing it leaves the server holding a half-open stream.
**Preconditions:** any scripted drive; a pipe invocation missing its payload plus a timeout-kill. Cumulative: each killed client degrades the lane further (seconds → minutes per pipe).
**Reproduce:** SANDBOX ONLY — no recovery short of relaunching the session. 1. Run a payload-less `zellij pipe --name clave-toggle` at the sandbox; it blocks on stdin. 2. Kill it. 3. Send a normal pipe: it queues forever while `zellij action` keeps working.
**Healthy:** every pipe carries an explicit non-empty payload; pipes complete within the ~1s window.
**Broken:** later pipes hang indefinitely; the session's whole CLI-pipe lane reads as "the bar stopped unblocking pipes".
**Drive assertion:** prevention, not provocation: QA-DRIVE phase 0 asserts zero orphaned `zellij pipe` processes (`pgrep -fl 'zellij pipe'` empty). A hung client is un-hung by feeding its stdin an EOF, never killed.
**Guard today:** none mechanical — explicit-payload discipline only.
**Refs:** FOOTGUNS.md "`zellij pipe --name X` with no `--` payload STREAMS stdin"; #128 drive 2026-08-01/04.

### P7 — Hook push spun 2 days at full core (#140) [FIELD]
**Seam:** the hook's fire-and-forget `zellij pipe` grandchild has no timeout; the hook's own timeout reaps the hook, not the grandchild.
**Preconditions:** any hook event whose pipe loses its unblock (a wedged lane, P6, is one route).
**Reproduce:** Repro unknown — detection only: `pgrep -fl 'zellij pipe'` — any client older than seconds is the signature (found 2026-08-04 at 3155 CPU-minutes, spinning against the maintainer's fleet); `$ZLOG` shows `unknown message` storms + 1s `CliPipe` stalls.
**Healthy:** no `zellij pipe` process outlives its ~1s window; a laggy fleet warrants this `ps` check before any deeper debugging.
**Broken:** an orphaned `zellij pipe --name clave-status` at full core for days, hammering the router.
**Drive assertion:** QA-DRIVE phase 0 preflight probes `pgrep -f 'zellij pipe'` twice ~2s apart and fails only on a pid present in both probes — a healthy in-flight client from any session lives inside its ~1s window and matches at most once, so machine-wide pipe quiescence is not a precondition. On a persistent pid, print the `pgrep -fl` line for forensics; empty must be the measured word, not silence.
**Guard today:** nothing — `push_snapshot` spawns without waiting and without a bound (`crates/clave/src/hook.rs:565-582`); the preflight is the only detector.
**Refs:** #140; FOOTGUNS.md "A `clave-status` hook push can spin FOREVER".

### P8 — CliPipe 1s timeout + EOF-twin noise (#45) [FIELD]
**Seam:** zellij's CLI-pipe protocol itself — every non-tty `zellij pipe` sends its payload then an unconditional `payload: None` twin per instance, and the action logs an ERROR-level 1s timeout.
**Preconditions:** any broadcast; present since the log's first retained line (v0.1.0 era).
**Reproduce:** send any CLI pipe; grep `$ZLOG` for `Action CliPipe did not complete within 1s timeout` and `dropped <name> pipe with empty payload`.
**Healthy:** exactly one drop line per live instance per delivery, plus the timeout ERROR — this IS the healthy signature. Volume reference: 53 timeouts / 131 drops in a 200-line window of normal use.
**Broken:** the class's damage is diagnostic, not functional: ERROR-level noise buried the real evidence twice (the v0.1.1 incident; #162's day-long misdiagnosis).
**Drive assertion:** repurpose, don't suppress: after k pipes with N live instances, drop-line delta == k×N — recorded, not asserted (the log is user-global and a drop line is unattributable to a session; FOOTGUNS "The zellij log is USER-GLOBAL"), so the delta is delivery forensics, never a gate. Read it only as a hint, and only under conditions the drive cannot verify from inside: every other clave session and pipe emitter on the machine quiescent, and the drive's own pipes serialised so k and N are known. Then a shortfall hints at a delivery genuinely missed and an excess at an unaccounted emitter — outside those conditions the delta measures the machine, not the drive.
**Guard today:** the twin is discarded before routing (`clave_bar::pipe::is_cli_blank_twin`, #197 — it used to reach `clave-toggle`'s payload-less arm and fire a second press, so a scripted collapse toggle was a no-op while the keybind worked). The drop log is still unconditional and latency is still unmeasured (issue open) — the twins are load-bearing as *diagnostics*, so never "fix" them silently; the gate is always the payload's own observable (the store bind). The drop count is now one line per CLI delivery per instance for EVERY pipe name, `clave-toggle` included; before #197 the toggle's twin was silently consumed instead.
**Refs:** #45, #197; FOOTGUNS.md "Every `zellij pipe` also delivers…" and the double-press entry beside it.

### P9 — Wake binds never land beyond early tabs (#178) [FIELD, OPEN]
**Seam:** the bar's bind leg — `bind_effects` emission vs `apply_bind` arrival vs prune traffic. CLI (open/spawn/resume), hooks, and register broadcasts were all proven healthy from store + clave.log + EOF-twin counts.
**Preconditions:** one session, ≥3 sequential wakes of dormant rows. First two wakes bound fine (tab_id 0, 1); every later wake failed. v0.1.3 wasm — regression window opens at PR #120 (bind_sent budget + `frames_coherent` gating), #162/PR #171 in the same neighborhood. Deterministic across retries.
**Reproduce:** 1. `just sandbox` with a ≥4-dormant-row scenario (QA-DRIVE's `qa-fleet`). 2. Wake rows sequentially: `scripts/ct.sh pipe --name clave-nav -- '{"row":N}'` then `… -- '{"commit":true}'`. 3. After each, probe the store.
**Healthy:** within a bounded wait per wake: `clave dev status | jq '.store.agents["<uuid>"].tab_id'` non-null; the row renders as an agent chip; dormant count drops on every instance's next snapshot.
**Broken:** `tab_id: null` while the transcript appends; the tab renders as a plain terminal row; the dormant row stays suppressed per instance (`is_dormant`'s `pane_live` suppression, `crates/clave-bar/src/model.rs:1064-1072`, turned permanent); no `bind-evict` evlog lines; register twin counts exact. Side-finding: re-waking claims the conversation off Claude remote control (`--resume` behaviour, R4).
**Drive assertion:** QA-DRIVE phase 2 wake ladder: after EACH wake, `tab_id` bound within 5s, else FAIL; register twin delta recorded per wake (not asserted — user-global log, see P8).
**Guard today:** **nothing** — the v0.1.3 no-go; the cut is held on it.
**Refs:** #178; docs/status/2026-08-11-1830-v013-part-c-drive.md; `model.rs:923` (`bind_effects`), `:800` (`frames_coherent`), `:409` (`bind_sent`).

### P10 — `sent_binds` permanent latch (#55/PR #120)
**Seam:** the bar-local emission ledger vs corrective re-emission — inserted at emit, never removed, never reset by `apply_snapshot`.
**Preconditions:** any mis-bind (the reachable one is B15's residual: a position-preserving permutation — close the lowest tab and create one in the same window).
**Reproduce:** Repro unknown — detection only: a store bind pointing at the wrong tab that never corrects for the life of the plugin instance (no re-emission however many frames arrive).
**Healthy:** re-emission permitted when the store's `seq` has strictly advanced past our own send, capped at `BIND_MAX_TRIES` (4) per (uuid, target) episode; a quiescent store costs zero subprocesses.
**Broken:** the wrong row forever — and do NOT re-gate on the pipe echo: that is the C5 round-4 shape that re-fired per TabUpdate and exhausted the server's fds.
**Drive assertion:** QA-DRIVE phase 3: after churn, every store bind's `tab_id` is in the live tab set and no two agents share one; `bind-evict` count in the evlog is zero.
**Guard today:** `bind_sent` seq-gated budget (`crates/clave-bar/src/model.rs:409`, `BIND_MAX_TRIES` `:339`, emit rule `:984-994`), unit-tested.
**Refs:** FOOTGUNS.md "[FIXED] `sent_binds` was a permanent latch"; #55, PR #120.

### P11 — `fire_binds` never ran post-hydrate
**Seam:** the hydrate snapshot (the `RunCommandResult` arm — the only thing populating `agents` at session birth) vs four bind call sites that each had to remember a second call.
**Preconditions:** cold start with agents in the store; the eager (auto-resumed) tab.
**Reproduce:** cold start a seeded sandbox; pre-fix, the eager tab was frequently never bound — `bind_effects` looped an empty `agents` for zero iterations, silently.
**Healthy:** the eager tab is bound shortly after hydration; its row shows an agent chip, not a terminal row.
**Broken:** the eager cold-start tab unbound with no error anywhere.
**Drive assertion:** after launch (QA-DRIVE phase 1), bounded wait: the eager row's `tab_id` is non-null in `clave dev status`.
**Guard today:** one entry point — `identity_effects()` (`crates/clave-bar/src/model.rs:867-897`), called from every arm taking external input, both snapshot paths folded in; RELEASE-RUNBOOK Step 4 checks the class live.
**Refs:** FOOTGUNS.md "[FIXED] `fire_binds()` was called after…".

### P12 — Full-live-set prune unbound a live agent (#6, PR #26)
**Seam:** two fire-and-forget `clave prune-tabs` subprocesses have no arrival order across processes.
**Preconditions:** a prune computed before a new tab's bind, landing after it.
**Reproduce:** Repro unknown — detection only (found by a reviewer reasoning about ordering, never executed): a "retain only these live ids" payload landing late unbinds a live agent, and P10's old latch meant it never re-fired.
**Healthy:** the payload carries observed-**STALE** ids — idempotent and commuting, so a late prune can only re-remove ids already judged dead.
**Broken:** live agent unbound and never rebound (glyphless live row; #6's double-attach corollary).
**Drive assertion:** QA-DRIVE phase 3: after interleaved close/create churn, every agent with a live tab is still bound (store↔`dump-layout` join).
**Guard today:** `Effect::PruneTabs { stale_ids }` (`crates/clave-bar/src/model.rs:75-84`, ordering commentary `:1307-1318`); store-side test `prune_tabs_removes_listed_stale_ids_order_safe_and_change_gated` (`crates/clave/src/store.rs:911`).
**Refs:** #6, PR #26; TESTING.md escape record ("written argument plus adversarial review IS the verification" for this class until #47).

### P13 — Set-change-gated prune silently dropped (PR #26)
**Seam:** event interleaving — a close's `TabUpdate` arriving before its `PaneUpdate` leaves the election false, so a set-change-gated emission drops the prune and the gate never retries.
**Preconditions:** exactly that frame order on a close; proptests never generated the shape.
**Reproduce:** Repro unknown — detection only: a closed tab's id surviving in store binds/`tab_order` indefinitely.
**Healthy:** prune emission is detection-driven — re-derived from current state on every `identity_effects()` pass, self-limited by the store echo, so a dropped emission re-emits on the next frame.
**Broken:** the effect dropped once and never retried; stale bind persists until session recreate.
**Drive assertion:** QA-DRIVE phase 3: after any `close-tab`, bounded wait: the closed tab's id is absent from every store bind (`clave dev status | jq '.store.agents[].tab_id'`).
**Guard today:** `prune_effect` emitted from `identity_effects`, not `apply_tabs` (`crates/clave-bar/src/model.rs:1343-1364`); `last_live_ids` deleted.
**Refs:** PR #26 (Codex lane); FOOTGUNS.md "Prune emission must NOT be set-change-gated".

### P14 — Registers never replay; late bar can't join (C5 r6)
**Seam:** fire-and-forget `clave-register` pipes vs instances born after the broadcast — pipes have no replay, so a late-loaded bar could never join an already-spawned agent.
**Preconditions:** an agent spawned, then a new tab (or plugin reload) creating a fresh bar instance.
**Reproduce:** (pre-Design-B) spawn one agent, open a new tab: the new tab's bar shows a permanently glyphless row; with two agents, walking alternated 6↔5 forever (crossed per-instance orders).
**Healthy:** every instance, however late, renders glyph/label/order from the store snapshot's uuid→tab_id bind; registers matter only for uuid-jump focus and computing new binds.
**Broken:** glyphless row on late instances; walk loop between two tabs.
**Drive assertion:** after a wake, create a tab (`scripts/ct.sh new-tab`), then assert the bind is in the store snapshot (`clave dev status`) — the store is the single source every instance hydrates from. Glyph presence on the newest bar stays HUMAN (eyeball checkpoint).
**Guard today:** Design B — the STORE binds (`clave bind`, `crates/clave/src/store.rs:389` `apply_bind`); bar `sort_key`/joins read the snapshot only.
**Refs:** SUBSYSTEM-VALIDATION.md C5 round 6.

### P15 — Fire-and-forget deltas diverged per instance (C5 r5)
**Seam:** per-instance timeline copies fed by pipe deltas — under spinup congestion some instances miss some echoes, each bar sorts its own diverged rows, and each landing computes the next step from ITS order.
**Preconditions:** several tabs; birth-touch echoes as deltas (pre-fix architecture).
**Reproduce:** (pre-fix) launch multi-tab, walk: oscillation forever (prev from tab 2 → 3, prev from 3 → 2), row order changed on walk step 2.
**Healthy:** order identical on every bar; a full walk visits each live tab exactly once per lap and terminates.
**Broken:** permanent oscillation — the general law: fire-and-forget deltas with no reconciliation always eventually diverge.
**Drive assertion:** QA-DRIVE phase 4: walk the live ring both directions via nav pipes, dump `focus=true` after each press → the visit sequence covers each live tab once per lap, no repeats before wrap.
**Guard today:** the timeline lives in the store (`tab_order`, `crates/clave/src/store.rs:170-181` — renamed from `tab_timeline`), locked RMW max-merge, and instances REPLACE their copy from each seq-gated snapshot.
**Refs:** SUBSYSTEM-VALIDATION.md C5 rounds 5–6.

### P16 — Stale-active fallback raced 6 SwitchTab targets (C5 r2)
**Seam:** hidden instances' frozen tab sets each claim their own tab is active, so an ungated walk fans divergent `SwitchTab` targets from every instance.
**Preconditions:** several tabs including instances event-starved since before the newest tab existed.
**Reproduce:** (pre-fix) one walk press with hidden stale instances → six divergent targets (`0,0,2,0,3,1` observed 16:15:44, 2026-07-08), all executed in ~50ms; transit landings announced and trashed recency.
**Healthy:** exactly one execution per press — the executor gate holds.
**Broken:** multiple focus changes per press; the agent tab sinks to the bottom (the user's original sighting).
**Drive assertion:** same probe as P2: one nav pipe → exactly one focus change in `dump-layout`, repeated across a full phase-4 walk.
**Guard today:** the executor gate — `nav_executor` (`crates/clave-bar/src/model.rs:1752`); row jumps executor-gated; the beacon is the only "on screen" signal ever trusted.
**Refs:** SUBSYSTEM-VALIDATION.md C5 round 2; FOOTGUNS.md `is_active_instance` entry.

### P17 — `InputReceived` subprocess-per-keystroke → EMFILE crash (C5 r4)
**Seam:** `EventType::InputReceived` carries no payload (all `EventType` variants are payload-free by construction), so it cannot tell a nav keybind from pane input — every keystroke spawned touch subprocesses, and echo-dependent guards re-fired under congestion.
**Preconditions:** 3+ tabs plus active walking (pre-fix build).
**Reproduce:** (pre-fix) walk a 3-tab session: server panic "Too many open files" — EMFILE at zellij-utils `ipc.rs:388` (`try_clone_stream().unwrap()`); tabs also reordered on focus.
**Healthy:** no `InputReceived` subscription at all (`crates/clave-bar/src/main.rs:466-473`, the "NO InputReceived" comment); an idle session generates zero subprocesses.
**Broken:** zellij server dead; session lost.
**Drive assertion:** QA-DRIVE phase 6 quiescence: idle 60s → evlog line count, `$ZLOG` tail, and store `seq` all flat. A functional pass with a growing idle log is a FAIL.
**Guard today:** the event is removed; terminal-input commitment was redesigned (store ordinals); drive-loop step 6 is the standing anti-storm assertion.
**Refs:** SUBSYSTEM-VALIDATION.md C5 round 4; FOOTGUNS.md `EventType::InputReceived` entry.

### P18 — `go_to_tab(pos+1)` silent no-op from plugin
**Seam:** the plugin-side tab-jump API — a 0-/1-based mismatch (ruled out as client-context by driving it from a keybind); it does nothing and reports nothing.
**Preconditions:** any plugin-initiated tab jump routed through `go_to_tab`.
**Reproduce:** swap a jump site to `go_to_tab(position+1)`, rebuild, click a row: nothing happens, no error, no log line.
**Healthy:** jumps go through `focus_pane_with_id(PaneId::Terminal(id), false, false)` — zellij pulls the pane's tab forward (`crates/clave-bar/src/main.rs:87-88`).
**Broken:** clicks/jumps silently dead while everything else works.
**Drive assertion:** covered by every focus-change assertion in phases 3–4 (a reintroduction makes those fail); no dedicated probe.
**Guard today:** the call-site comment "go_to_tab is a known dead end" (`main.rs:87`); FOOTGUNS entry. Note `main.rs` is `test = false` — nothing hermetic can pin this.
**Refs:** FOOTGUNS.md "`go_to_tab(position+1)` is a silent no-op".
