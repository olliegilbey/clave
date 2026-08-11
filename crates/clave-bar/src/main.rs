//! clave-bar — the vertical dynamic tab bar (spec §6.6). This file is a THIN
//! adapter: zellij events/pipes in → model.rs (pure, host-tested) → Effects
//! out. Keep logic out of here; if you're writing an `if` about ordering,
//! glyphs, or renames, it belongs in model.rs where it can be unit-tested.

use std::collections::BTreeMap;
use std::io::Write;

// The pure model lives in the LIB half of this crate (src/lib.rs → model.rs)
// so it host-tests without linking this bin's wasm host-import shims.
use clave_bar::model::{BarModel, Effect, PEEK_SINK_SECS, PaneMeta, TabMeta};
use clave_bar::plugin_config::resolve_binary;
use clave_bar::render::{Row, render_rows};
use zellij_tile::prelude::*;

#[derive(Default)]
struct State {
    model: BarModel,
    /// Our own plugin pane id (get_plugin_ids) — used to decide whether THIS
    /// instance sits in the active tab. There is one bar instance per tab
    /// (§6.6); render-side state converges via broadcast, but WRITE effects
    /// (RenameTab, MarkRead) run on the active-tab instance only, so N
    /// instances don't fire N duplicate renames / `clave focus` runs.
    own_plugin_id: Option<u32>,
    /// Peek-on-nav timers in flight: each armed peek starts one
    /// set_timeout(1.0); only the LAST expiry sinks the bar, so a nav burst
    /// keeps it expanded until ~1s after the final press. The ONLY timers
    /// the bar arms (#100 deleted the dormant dwell).
    pending_peeks: u32,
    /// The pane height in lines, as of the last `render` — the only place
    /// zellij tells us (#148). A click carries a line of the VIEWPORT, so
    /// mapping it back to a row needs the height the viewport was sliced at.
    /// `Default`'s 0 stands for "never rendered". If a click ever did precede
    /// the first render, `pane_height == 0` degrades `viewport_top` to 0, so
    /// the click map falls back to the pre-viewport identity mapping (line N
    /// selects row N) rather than misbehaving.
    pane_height: usize,
    /// The CLI this bar shells out to, from plugin configuration (#44).
    /// Assigned in `load()`, which zellij invokes as its own wasm export
    /// before delivering any event (`register_plugin!`, zellij-tile-0.44.3
    /// src/lib.rs:109-127 — `load`, `update`, `pipe` and `render` are separate
    /// exports, and the host instantiates through `load`). `Default`'s empty
    /// string is therefore not expected to be observable; the shellout sites
    /// still degrade to a failed `run_command` rather than misbehaving if a
    /// future zellij ever reordered that, which is why this is documented
    /// rather than asserted.
    clave_binary: String,
}

register_plugin!(State);

impl State {
    /// Execute model effects. Gate WRITES to the active-tab instance;
    /// FocusPane is intentionally ungated (every instance computes the same
    /// target — focusing twice is idempotent, and the keybind MessagePlugin
    /// may reach instances in any order).
    fn run_effects(&mut self, effects: Vec<Effect>) {
        // Nothing to gate. render() calls this every repaint with the width
        // seek's usually-empty result, and both gates below build a pair of
        // BTreeSets to test frame coherence — so without this the hottest path
        // in the plugin pays for an election it never uses.
        if effects.is_empty() {
            return;
        }
        // TWO gate strengths (#55). `confirmed` additionally requires the last
        // TabUpdate and the last PaneUpdate to describe the same tab set — it
        // gates the effects that retry or do lasting damage from the wrong
        // instance. `presumed` is the pre-#55 position join, byte-for-byte,
        // kept for the three effects that latch at emit and so cannot survive a
        // fail-closed gate: tightening them would convert a wrong-action bug
        // into a missed-action bug. DO NOT "tidy" a presumed arm into
        // confirmed without first giving that effect a retry trigger.
        // `ReanchorVisit` was the fourth; #162 gave it that trigger by moving
        // its election into the model, which is the template for the rest —
        // decide in `model.rs`, where a test can reach the decision, and
        // consume the latch only on the branch that emits.
        // confirmed ⇒ presumed.
        let confirmed = self.model.elects_confirmed();
        let presumed = self.model.elects_presumed();
        // Bound once: several arms below take `&mut self`, so borrowing the
        // field inline would conflict. One String clone per batch is noise.
        let bin = self.clave_binary.clone();
        for e in effects {
            match e {
                Effect::FocusPane { pane_id } => {
                    // S2-proven nav: focus the terminal pane; Zellij pulls
                    // its tab forward. go_to_tab is a known dead end.
                    focus_pane_with_id(PaneId::Terminal(pane_id), false, false);
                }
                Effect::SwitchTab { position } => {
                    // 1-based, like the stock tab-bar's click handler. The
                    // keybind broadcast makes every instance execute this
                    // with the SAME position — idempotent duplicates.
                    switch_tab_to(position as u32 + 1);
                }
                // The beacon. Single-instance jumps (clicks, nav) converge the
                // other instances over this pipe channel.
                //
                // BOTH variants run unconditionally here. `AnnounceVisit` is
                // ungated by design (birth must announce before its first
                // PaneUpdate). `ReanchorVisit` is gated too — but since #162 the
                // gate is at emit time, inside the model: it elects itself
                // before producing the effect, and consumes the trigger on the
                // same branch. Re-gating it here would put the decision back
                // where no test can reach it, and a second evaluation could only
                // ever drop a beacon whose trigger has already been spent.
                // Load-bearing: `apply_tabs` and `apply_panes` are
                // `ReanchorVisit`'s ONLY emitters — the two delivered frames,
                // each electing before it emits — so this arm's safety depends
                // entirely on every emission having already passed the
                // election. An un-elected emitter anywhere else, or an arm that
                // drops what these two return, would bypass it completely.
                Effect::AnnounceVisit { tab_id } | Effect::ReanchorVisit { tab_id } => {
                    run_command(
                        &[
                            "zellij",
                            "pipe",
                            "--name",
                            "clave-visited",
                            "--",
                            &tab_id.to_string(),
                        ],
                        BTreeMap::new(),
                    );
                }
                Effect::RenameTab { tab_id, name } if presumed => {
                    rename_tab_with_id(tab_id as u64, name);
                }
                Effect::MarkRead { uuid } if presumed => {
                    // Persist the unread clear (§6.5). Fire-and-forget; the
                    // local repaint already happened in the model.
                    run_command(&[bin.as_str(), "focus", &uuid], BTreeMap::new());
                }
                Effect::Touch { tab_id } if confirmed => {
                    // The once-EVER birth stamp for a tab the store timeline
                    // has never seen (its creation moment). `clave touch`
                    // stamps host time into the STORE and pushes the snapshot
                    // that carries the new order back to every instance. The
                    // model consumes its once-ever latch only when it actually
                    // emits, so a false gate DEFERS the touch to the next
                    // coherent frame rather than losing it — and the latch
                    // stays echo-INDEPENDENT (C5 rd 4: echo-gated guards
                    // re-fired per TabUpdate → spawn storm → server fd
                    // exhaustion).
                    run_command(
                        &[bin.as_str(), "touch", &tab_id.to_string()],
                        BTreeMap::new(),
                    );
                }
                Effect::Bind { uuid, tab_id } if confirmed => {
                    // Report the uuid→tab join to the store (§6.6 Design B);
                    // `clave bind` RMWs and pushes the snapshot that carries
                    // it to every instance.
                    run_command(
                        &[bin.as_str(), "bind", &uuid, &tab_id.to_string()],
                        BTreeMap::new(),
                    );
                }
                Effect::PruneTabs { stale_ids } if confirmed => {
                    // #6/F3: report the OBSERVED-STALE ids (not the live set) so
                    // the store removes exactly those binds/timeline entries —
                    // idempotent removals commute, so two out-of-order prunes
                    // can't clobber a tab neither saw die. Executor-gated (like
                    // Bind): keeps duplicate prunes to the active bar. The model
                    // gates emission to set-changes, so this fires ~once per
                    // close, not per TabUpdate.
                    let mut argv: Vec<String> = vec![bin.clone(), "prune-tabs".into()];
                    argv.extend(stale_ids.iter().map(usize::to_string));
                    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
                    run_command(&refs, BTreeMap::new());
                }
                // #137: UNGATED on purpose — every instance rate-limits its own
                // pane, so every instance must be able to say so. Gating this to
                // the executor would hide exactly the storms that only happen on
                // the eleven bars nobody is looking at.
                Effect::StormCapped {
                    actions,
                    cols,
                    target,
                } => {
                    eprintln!(
                        "clave-bar: WIDTH SEEK CAPPED after {actions} resizes without \
                         settling — parking at cols={cols} (target {target}). This is the \
                         storm brake (#137); something is re-arming the seek faster than \
                         it can converge."
                    );
                }
                // C6 width-seek effects are SELF-targeted (round 20: every
                // instance drives only its own pane, with render feedback).
                Effect::ShrinkSelf => {
                    if let Some(own) = self.own_plugin_id {
                        resize_pane_with_id(
                            ResizeStrategy::new(Resize::Decrease, Some(Direction::Right)),
                            PaneId::Plugin(own),
                        );
                    }
                }
                Effect::GrowSelf => {
                    if let Some(own) = self.own_plugin_id {
                        resize_pane_with_id(
                            ResizeStrategy::new(Resize::Increase, Some(Direction::Right)),
                            PaneId::Plugin(own),
                        );
                    }
                }
                // §6.6 C8 dormant nav (ungated — click reaches exactly one
                // instance, nav effects are executor-only by construction,
                // and the model's `opening` guard + clave open's no-op make
                // duplicates harmless).
                Effect::ArmPeek => {
                    self.pending_peeks += 1;
                    set_timeout(PEEK_SINK_SECS);
                }
                Effect::OpenAgent { uuid } => {
                    // Task 7b′: the new tab's bar percent is derived from the
                    // REAL display width, not the reference-viewport fiction.
                    // `clave open` runs inside zellij, so it cannot read this
                    // itself — a `terminal_size()` there reports the calling
                    // pane. We can: `get_tab_info` is a synchronous host call
                    // (zellij-tile-0.44.3 shim.rs:307). Measured live before
                    // this fix, dwell-opened tabs rested at 27% against the
                    // launch tab's 28% — one column apart, visible on every
                    // tab switch. Collapse mode rides along for D36's reason.
                    // Fail-closed since #55: an incoherent frame yields None
                    // and we simply omit --display-cols, falling back to
                    // `clave open`'s own default — strictly better than
                    // measuring a DIFFERENT tab's width off a mismatched join.
                    let cols = self
                        .model
                        .own_tab()
                        .and_then(get_tab_info)
                        .map(|t| t.display_area_columns);
                    let cols_s = cols.map(|c| c.to_string());
                    let mut argv = vec![bin.as_str(), "open", &uuid];
                    if let Some(c) = cols_s.as_deref() {
                        argv.extend_from_slice(&["--display-cols", c]);
                    }
                    if self.model.collapsed {
                        argv.push("--collapsed");
                    }
                    run_command(&argv, BTreeMap::new());
                }
                Effect::PersistCollapse { collapsed } if presumed => {
                    // Issue #5: report the ABSOLUTE collapse mode to the
                    // store (the one writer); its seq-bumped push heals any
                    // instance the toggle broadcast missed. Every instance
                    // books the pending write; only the active one runs it.
                    run_command(
                        &[
                            bin.as_str(),
                            "collapse",
                            if collapsed { "true" } else { "false" },
                        ],
                        BTreeMap::new(),
                    );
                }
                _ => {} // non-active instance skips writes
            }
        }
    }

    /// Run every effect keyed on THIS instance's tab identity (bind, birth
    /// touch). Called from EVERY arm that mutates model state from an external
    /// input — both snapshot paths, both frame kinds, `clave-register`.
    ///
    /// The single entry point is the point (#55): bind emission used to be an
    /// adapter-level call each of those arms had to remember separately, and
    /// the hydrate arm — the only path that populates `agents` at session
    /// birth — was the one that forgot (RC-B). Fail-closed inside the model,
    /// so calling it on an incoherent frame is a no-op and the next frame is
    /// the retry.
    fn settle_identity(&mut self) {
        let fx = self.model.identity_effects();
        if !fx.is_empty() {
            self.run_effects(fx);
        }
    }

    /// The ONE snapshot path — hydrate (`RunCommandResult`) and the live
    /// `clave-status` push both land here. Two call sites that each had to
    /// remember `settle_identity()` is how the hydrate came to be the only
    /// snapshot that never bound anything (RC-B).
    fn apply_snapshot_and_settle(&mut self, snap: clave_types::AgentSnapshot) {
        let fx = self.model.apply_snapshot(snap);
        self.run_effects(fx);
        self.settle_identity();
    }

    /// Alt+c (round 20, collapse-in-place): flip the width target and let
    /// the render-fed seek drive OWN pane width there. No hide_self /
    /// show_self — suppress was structurally hostile (lossy re-insert,
    /// damage flag blocks swap-layout restores, resizes emit no events for
    /// hidden panes). Every instance stays visible, hears this pipe, and
    /// converges its own pane with real feedback.
    fn toggle_collapsed(&mut self) {
        // Durability (issue #5): the broadcast flipped every instance's
        // memory, but memory alone desyncs (C8 parity family — birth after
        // toggle, reload, missed pipe). The model books the write it owes
        // the store (pending ledger) and emits PersistCollapse; run_effects
        // gates its EXECUTION to the active instance, same as MarkRead/Bind
        // — one writer per toggle, absolute value, no push storm (rd 11).
        let fx = self.model.toggle();
        self.run_effects(fx);
    }

    /// One pipe message → model. Split out of pipe() so early returns here
    /// can't skip the unconditional unblock (dd38ace — see pipe()).
    fn handle_pipe(&mut self, message: PipeMessage) -> bool {
        let name = message.name.as_str();
        let Some(payload) = message.payload.as_deref() else {
            // Toggle and organic carry no payload; everything else must.
            // A keybind MessagePlugin without a payload attribute delivers
            // payload=None, so a payload-less pipe NEVER reaches the named
            // match below — clave-organic sat dead there and the Alt+o
            // beacon never announced (#128 live check, 2026-08-01: the
            // departed bar kept cursor AND current_tab==own indefinitely,
            // so a commit 18s after the switch still opened the old
            // selection).
            match name {
                "clave-toggle" => {
                    self.toggle_collapsed();
                    return true;
                }
                "clave-organic" => {
                    // Arms the bounded announce AND spends any dormant
                    // selection (#100) — the selection drop must repaint.
                    self.model.set_organic_pending();
                    return true;
                }
                _ => {
                    eprintln!("clave-bar: dropped {name} pipe with empty payload");
                    return false;
                }
            }
        };
        match name {
            "clave-status" => match serde_json::from_str(payload) {
                Ok(snap) => {
                    self.apply_snapshot_and_settle(snap);
                    true
                }
                Err(e) => {
                    eprintln!("clave-bar: bad clave-status payload: {e}");
                    false
                }
            },
            "clave-register" => match serde_json::from_str::<clave_types::Register>(payload) {
                Ok(reg) => {
                    self.model.register(reg.uuid, reg.pane_id);
                    self.settle_identity(); // the join input just landed
                    true // a row may just have gained its glyph
                }
                Err(e) => {
                    eprintln!("clave-bar: bad clave-register payload: {e}");
                    false
                }
            },
            "clave-visited" => match payload.trim().parse::<usize>() {
                Ok(tab_id) => {
                    // Beacon (executor election — never reorders) + peek:
                    // a collapsed bar expands while the user navigates and
                    // sinks ~1s after the last nav (timer per peek; the
                    // Event::Timer arm below sinks only when the count of
                    // pending timers drains to zero).
                    if self.model.visited(tab_id) {
                        self.pending_peeks += 1;
                        set_timeout(PEEK_SINK_SECS); // user-tuned: 1.0 felt a touch long
                    }
                    true // active-row highlight may move
                }
                Err(e) => {
                    eprintln!("clave-bar: bad clave-visited payload: {e}");
                    false
                }
            },
            "clave-organic" => {
                // Alt+o's bind: ToggleTab + this pipe. Normally payload-less
                // (handled above); kept here so a payload-carrying variant
                // behaves identically rather than silently diverging.
                self.model.set_organic_pending();
                true
            }
            // NO clave-touch/clave-touch-pane arms: tab order now travels
            // INSIDE clave-status snapshots (store tab_timeline, §6.6) —
            // fire-and-forget pipe deltas diverged per instance (C5 rd 5).
            "clave-nav" => {
                // Exactly one instance may act on the press. The rule lives in
                // the model (`nav_executor`) and is the beacon alone: the
                // instance whose own tab is the one the last broadcast named.
                // Nothing local qualifies anyone, because a frozen instance's
                // own frames claim it is active (FOOTGUNS.md:64) — #162's
                // stranded beacon is answered by re-seeding it from either
                // delivered frame, not by letting the stranded instance nav on
                // local truth. Fail-closed: a dropped Alt+j is a repeatable
                // keypress, a jump to the wrong tab is not.
                let executor = self.model.nav_executor();
                let is_executor = executor.is_some();
                let fx = self.model.nav(payload, executor);
                let acted = !fx.is_empty();
                self.run_effects(fx);
                // A dormant landing is now a pure selection (#100) — zero
                // effects, but the ⏎ affordance and highlight must paint, so
                // the executor (the visible bar) repaints unconditionally.
                acted || is_executor
            }
            "clave-toggle" => {
                self.toggle_collapsed();
                true
            }
            other => {
                eprintln!("clave-bar: unknown pipe {other:?}");
                false
            }
        }
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, config: BTreeMap<String, String>) {
        // Version marker for the hot-reload workflow (`zellij action
        // start-or-reload-plugin`): stamp the build so the zellij log tells
        // you WHICH wasm produced a trace. Set by the rebuild recipe via
        // CLAVE_BUILD_TAG; "dev" means an untagged local build.
        eprintln!(
            "clave-bar: loaded v{} build={}",
            env!("CARGO_PKG_VERSION"),
            option_env!("CLAVE_BUILD_TAG").unwrap_or("dev")
        );
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
        // D37: gate the width seek HERE, not when the snapshot is requested.
        // `load()` only ASKS for permission; the grant arrives later as an
        // event, and zellij renders this pane before then — so a gate set in
        // the `PermissionRequestResult` arm is set AFTER the first render has
        // already seeked on the assumed-expanded default. That is the first
        // fix for this failing live and the reason it failed: the ordering,
        // not the gate. Nothing before hydration may move the pane, and
        // `load()` is the only point that precedes every render.
        self.model.await_hydration();
        // §6.6 permission set — EXACTLY these four; grants are all-or-nothing
        // per plugin and the prompt is unanswerable in the bar pane, so
        // `clave setup` pre-seeds permissions.kdl with THIS set (both key
        // forms). Changing this list without changing the seed hangs every
        // pipe (this re-bit S2 — see the ledger).
        request_permission(&[
            PermissionType::ReadCliPipes,           // receive the clave-* pipes
            PermissionType::ChangeApplicationState, // focus_pane / rename_tab / hide_self
            PermissionType::ReadApplicationState,   // TabUpdate + PaneUpdate truth
            PermissionType::RunCommands,            // hydrate (clave snapshot) + clave focus
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::Mouse,
            EventType::RunCommandResult,
            EventType::PermissionRequestResult,
            EventType::Timer, // peek-on-nav sink (set_timeout per peek)
                              // NO InputReceived: it fires for EVERY keystroke INCLUDING the
                              // nav keybinds themselves (C5 round 4: each walk press touched
                              // the departing tab and the touch-spawn storm exhausted the
                              // server's fds). Plain tabs order by birth only — shell-command
                              // touches are parked (§6.6).
        ]);
        // Stock tab-bar pattern: an unselectable pane receives clicks
        // directly (no focus-stealing first click) and MoveFocus skips it —
        // nothing the bar does needs focus (clicks, pipes, hide_self).
        set_selectable(false);
        // `own_plugin_id` stays for ShrinkSelf/GrowSelf's resize_pane_with_id;
        // the model gets the same id because identity resolution lives there
        // now — this file is `test = false`, and RC-A shipped precisely
        // because the frame join was written where nothing could assert on it.
        let id = get_plugin_ids().plugin_id;
        self.own_plugin_id = Some(id);
        self.model.set_own_pane(id);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(_) => {
                // Permissions just landed (pre-seeded → immediate): hydrate
                // from the store via `clave snapshot` (was spike S5). The
                // result arrives as RunCommandResult below; the seq gate
                // makes any race with live pushes benign (§5).
                run_command(&[self.clave_binary.as_str(), "snapshot"], BTreeMap::new());
                false
            }
            Event::RunCommandResult(exit, stdout, stderr, _ctx) => {
                // Only `clave snapshot` produces stdout we care about; the
                // `clave focus` fire-and-forgets also land here — ignore
                // anything that doesn't parse as a snapshot.
                if exit != Some(0) {
                    eprintln!(
                        "clave-bar: run_command failed: {}",
                        String::from_utf8_lossy(&stderr)
                    );
                    return false;
                }
                match serde_json::from_slice(&stdout) {
                    Ok(snap) => {
                        // RC-B (#55): this is the arm that FIRST populates
                        // `agents`. TabUpdate/PaneUpdate normally arrive
                        // before the snapshot result — permissions land, the
                        // frames flow, then `clave snapshot` returns — so
                        // their own settle ran against an EMPTY agent list and
                        // bound nothing. Nothing else arrives until a frame
                        // changes, so the eager cold-start tab stayed unbound
                        // and its first prompt never moved it. Both snapshot
                        // paths now go through one method, so this cannot
                        // diverge from the `clave-status` arm again.
                        self.apply_snapshot_and_settle(snap);
                        true
                    }
                    Err(_) => false, // not a snapshot (e.g. clave focus) — fine
                }
            }
            Event::TabUpdate(tabs) => {
                let metas: Vec<TabMeta> = tabs
                    .iter()
                    .map(|t| TabMeta {
                        tab_id: t.tab_id,
                        position: t.position,
                        name: t.name.clone(),
                        active: t.active,
                    })
                    .collect();
                let fx = self.model.apply_tabs(metas);
                // NO beacon announce here (round 11): TabUpdate announces
                // were poisoned by design — a hidden instance's stale set
                // always claims its own tab is active (C3), and toggle
                // bursts deliver TabUpdates to ALL instances, so they
                // warred over the beacon (~15 pipes/s storm). The beacon is
                // announced from render() instead — the one signal only the
                // on-screen bar receives. The one-time BIRTH touch used to
                // live inline here; it is now `Effect::Touch`, emitted by
                // `identity_effects` (#55) — it was untestable in this file
                // and, living only in this arm, had no trigger to retry after
                // the one TabUpdate a close delivers.
                self.run_effects(fx);
                // Ordering note: the touch now runs AFTER PruneTabs rather
                // than before. Both are fire-and-forget subprocesses with no
                // arrival-order guarantee either way, and their payloads are
                // disjoint id sets (prune removes observed-dead ids, touch
                // stamps a live one), so they commute.
                self.settle_identity(); // fresh tab set → own-tab joins resolvable
                true
            }
            Event::PaneUpdate(manifest) => {
                // EVERY pane goes into `metas`, plugin ones included: the
                // model's `own_tab_position` locates our own plugin pane in
                // it, and the coherence witness reads all of them (a tab
                // without a bar must not make the witness fail forever).
                let mut metas = Vec::new();
                for (tab_position, panes) in &manifest.panes {
                    for p in panes {
                        metas.push(PaneMeta {
                            tab_position: *tab_position,
                            pane_id: p.id,
                            is_plugin: p.is_plugin,
                            is_focused: p.is_focused,
                        });
                    }
                }
                // The pane frame is the second retry for a stranded beacon
                // (#162): a close's TabUpdate arriving first refuses the
                // re-anchor off the still-stale manifest, and THIS frame is the
                // one that ends that disagreement. Dropping what it returns
                // would restore the bug — nav dead for the session whenever the
                // tab that closed was the one holding the beacon.
                let fx = self.model.apply_panes(metas);
                self.run_effects(fx);
                self.settle_identity(); // fresh manifest → own-tab joins resolvable
                true
            }
            Event::Timer(_elapsed) => {
                // Peek sinks are the ONLY timers the bar arms (#100 deleted
                // the dormant dwell, so the two-kind classify_timer split
                // went with it). One expiry per armed peek; only the LAST
                // sinks (nav burst = one visible expand, one sink).
                // peek_expired() is false when a toggle already cancelled
                // the peek — no repaint.
                self.pending_peeks = self.pending_peeks.saturating_sub(1);
                self.pending_peeks == 0 && self.model.peek_expired()
            }
            Event::Mouse(Mouse::LeftClick(line, _col)) => {
                // §6.6: rows are mouse-clickable. line is the rendered row.
                // Repaint: a dormant click is a pure selection (#100) — no
                // effects, but the ⏎ affordance and highlight must paint.
                if line >= 0 {
                    let fx = self.model.click(line as usize, self.pane_height);
                    self.run_effects(fx);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        // A CLI pipe blocks its caller until unblocked; capture the id BEFORE
        // the message moves. Keybind/plugin sources carry no pipe id.
        let cli_pipe_id = match &message.source {
            PipeSource::Cli(id) => Some(id.clone()),
            _ => None,
        };
        let repaint = self.handle_pipe(message);
        // UNCONDITIONAL unblock (dd38ace): even a malformed payload must not
        // leave `zellij pipe` hanging until Zellij's 1s server timeout.
        if let Some(id) = cli_pipe_id {
            unblock_cli_pipe_input(&id);
        }
        repaint
    }

    fn render(&mut self, rows: usize, cols: usize) {
        // NO announce here (round 12): render is NOT visibility-gated
        // either (every instance renders at least once after load) — the
        // render announce EMFILE-crashed the server. No beacon originates in
        // render: they fire from apply_tabs (birth / clave-organic) and
        // apply_panes (the #162 reanchor debt) — both frame-witnessed — and
        // from the click/nav landings, which still emit AnnounceVisit.
        // C6 width seek (round 20, collapse-in-place): each of our resizes
        // triggers a repaint with the new cols (round 10) — this render
        // chain is the seek's feedback loop. SELF-targeted and ungated:
        // every instance is always visible and drives only its own pane.
        let fx = self.model.width_seek(cols);
        self.run_effects(fx);
        // One line per row, display-ordered. Everything visual — the column
        // arithmetic, the palette, the fade, the truncation — lives in
        // `render_rows` (design-lock; LEDGER D4/D5). This file stays zellij
        // plumbing: the profile comes from the model so it cannot drift from
        // the width the seek above is chasing (D16), and `cols` is whatever
        // zellij actually gave us rather than the target.
        // #148: `rows` is the pane HEIGHT in lines. The renderer slices the row
        // list to it (the viewport); a bar that printed past the bottom drew
        // rows the user could reach with nav keys and never see. Remembered
        // because the mouse-click map needs the same height, and Mouse events
        // do not carry it.
        self.pane_height = rows;
        let list: Vec<Row> = self.model.rows().into_iter().map(|(_, row)| row).collect();
        let lines = render_rows(&list, cols, rows, self.model.widths());
        // Final review 2026-08-11: emit the frame WITHOUT a trailing newline.
        // Once the viewport (#148) slices to exactly `rows` lines, the pane is
        // full at steady state; a trailing newline after the bottom row would
        // scroll the pane's own grid by one, eating the top row and shifting
        // every click map by one line. `std::io::Stdout` line-buffers on `\n`
        // regardless of tty-ness, so the last (newline-less) line needs an
        // explicit flush to reach the host.
        print!("{}", lines.join("\n"));
        let _ = std::io::stdout().flush();
    }
}

// NOTE: no `fn main()` — register_plugin! supplies the wasm entry point (a
// second one is E0428; confirmed in foundation Task 1).
