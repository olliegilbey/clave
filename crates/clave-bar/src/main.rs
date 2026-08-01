//! clave-bar — the vertical dynamic tab bar (spec §6.6). This file is a THIN
//! adapter: zellij events/pipes in → model.rs (pure, host-tested) → Effects
//! out. Keep logic out of here; if you're writing an `if` about ordering,
//! glyphs, or renames, it belongs in model.rs where it can be unit-tested.

use std::collections::BTreeMap;

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
    /// Raw pane rows kept so we can locate our own plugin pane per tab.
    plugin_panes: Vec<(usize, u32)>, // (tab_position, plugin pane id)
    /// The last TabUpdate, verbatim — is_active_instance reads it (rows()
    /// is display-ordered, so it can't answer "is position P active").
    last_tabs: Vec<TabMeta>,
    /// Peek-on-nav timers in flight: each armed peek starts one
    /// set_timeout(1.0); only the LAST expiry sinks the bar, so a nav burst
    /// keeps it expanded until ~1s after the final press. The ONLY timers
    /// the bar arms (#100 deleted the dormant dwell).
    pending_peeks: u32,
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
    /// Is THIS instance the one living in the currently-active tab?
    fn is_active_instance(&self) -> bool {
        let Some(own) = self.own_plugin_id else {
            return false;
        };
        // Find our tab position via our plugin pane id, then check active.
        self.plugin_panes
            .iter()
            .find(|(_, id)| *id == own)
            .map(|(pos, _)| *pos)
            .and_then(|pos| self.model_tab_active_at(pos))
            .unwrap_or(false)
    }

    /// tab_id of the tab hosting OUR pane, from the latest local data.
    /// Trustworthy exactly when it matters: the executor check compares it
    /// to the replicated current_tab, and only the truly-active instance
    /// (fresh TabUpdate/PaneUpdate) can match.
    fn own_tab_id(&self) -> Option<usize> {
        let own = self.own_plugin_id?;
        let pos = self
            .plugin_panes
            .iter()
            .find(|(_, id)| *id == own)
            .map(|(pos, _)| *pos)?;
        self.last_tabs
            .iter()
            .find(|t| t.position == pos)
            .map(|t| t.tab_id)
    }

    fn model_tab_active_at(&self, position: usize) -> Option<bool> {
        // rows() is display-ordered; go through the raw tabs instead.
        // (model exposes rows; keep a tiny helper here off the same data we
        // fed it — the last TabUpdate.)
        self.last_tabs
            .iter()
            .find(|t| t.position == position)
            .map(|t| t.active)
    }

    /// Execute model effects. Gate WRITES to the active-tab instance;
    /// FocusPane is intentionally ungated (every instance computes the same
    /// target — focusing twice is idempotent, and the keybind MessagePlugin
    /// may reach instances in any order).
    fn run_effects(&mut self, effects: Vec<Effect>) {
        let active = self.is_active_instance();
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
                Effect::AnnounceVisit { tab_id } => {
                    // Single-instance jumps (clicks) converge the other
                    // instances over the pipe channel.
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
                Effect::ReanchorVisit { tab_id } if active => {
                    // #23: same clave-visited beacon as AnnounceVisit, but
                    // GATED to the active instance — a toggle burst delivers the
                    // fresh set to every bar (doc:371-394), so an ungated
                    // re-anchor would be a per-instance beacon war (round-13
                    // EMFILE class). Accepted trade (see model apply_tabs): a
                    // transiently-false active check drops the reseed and nav
                    // stays stranded until a click — narrow, and storm-free.
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
                Effect::RenameTab { tab_id, name } if active => {
                    rename_tab_with_id(tab_id as u64, name);
                }
                Effect::MarkRead { uuid } if active => {
                    // Persist the unread clear (§6.5). Fire-and-forget; the
                    // local repaint already happened in the model.
                    run_command(&[bin.as_str(), "focus", &uuid], BTreeMap::new());
                }
                Effect::Bind { uuid, tab_id } if active => {
                    // Report the uuid→tab join to the store (§6.6 Design B);
                    // `clave bind` RMWs and pushes the snapshot that carries
                    // it to every instance.
                    run_command(
                        &[bin.as_str(), "bind", &uuid, &tab_id.to_string()],
                        BTreeMap::new(),
                    );
                }
                Effect::PruneTabs { stale_ids } if active => {
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
                    let cols = self
                        .own_tab_id()
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
                Effect::PersistCollapse { collapsed } if active => {
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

    /// §6.6 Design B bootstrap: only the ACTIVE instance reports binds — its
    /// manifest is the fresh one; a hidden instance's stale positions would
    /// bind agents to the wrong tabs.
    fn fire_binds(&mut self) {
        if self.is_active_instance()
            && let Some(own) = self.own_tab_id()
        {
            let fx = self.model.bind_effects(own);
            if !fx.is_empty() {
                self.run_effects(fx);
            }
        }
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
            // Toggle carries no payload; everything else must.
            if name == "clave-toggle" {
                self.toggle_collapsed();
                return true;
            }
            eprintln!("clave-bar: dropped {name} pipe with empty payload");
            return false;
        };
        match name {
            "clave-status" => match serde_json::from_str(payload) {
                Ok(snap) => {
                    let fx = self.model.apply_snapshot(snap);
                    self.run_effects(fx);
                    self.fire_binds(); // a new agent row may need its bind
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
                    self.fire_binds(); // the join input just landed
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
                // Alt+o's bind: ToggleTab + this pipe. Arms ONE announce on
                // the next TabUpdate (which steady-state zellij delivers
                // only to the newly-active instance — C3).
                self.model.set_organic_pending();
                false
            }
            // NO clave-touch/clave-touch-pane arms: tab order now travels
            // INSIDE clave-status snapshots (store tab_timeline, §6.6) —
            // fire-and-forget pipe deltas diverged per instance (C5 rd 5).
            "clave-nav" => {
                // Row jumps and dir walks need a FRESH tab set — only the
                // active instance has one. Executor = the instance whose own
                // tab is the replicated beacon (converged via visited pipes).
                let executor = self
                    .own_tab_id()
                    .filter(|own| self.model.current_tab() == Some(*own));
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
        self.own_plugin_id = Some(get_plugin_ids().plugin_id);
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
                        let fx = self.model.apply_snapshot(snap);
                        self.run_effects(fx);
                        // RC-B (#55): this is the arm that FIRST populates
                        // `agents`, and it was the one arm that did not fire
                        // binds. TabUpdate/PaneUpdate normally arrive before
                        // the snapshot result — permissions land, the frames
                        // flow, then `clave snapshot` returns — so their own
                        // `fire_binds()` (below, :491/:511) ran against an
                        // EMPTY agent list and bound nothing. Nothing else
                        // arrives until a frame changes, so the eager
                        // cold-start tab stayed unbound and its first prompt
                        // never moved it. Same gate as every other call site
                        // (active instance + resolvable own tab), and
                        // `bind_effects`' guard is last-SENT per (uuid, tab),
                        // so a re-fire is silent rather than a storm.
                        self.fire_binds();
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
                self.last_tabs = metas.clone();
                let fx = self.model.apply_tabs(metas);
                // NO beacon announce here (round 11): TabUpdate announces
                // were poisoned by design — a hidden instance's stale set
                // always claims its own tab is active (C3), and toggle
                // bursts deliver TabUpdates to ALL instances, so they
                // warred over the beacon (~15 pipes/s storm). The beacon is
                // announced from render() instead — the one signal only the
                // on-screen bar receives. This block only fires the
                // one-time BIRTH touch for a tab the timeline has never
                // seen (its creation moment; `clave touch` stamps time).
                if let Some(active_id) = self.last_tabs.iter().find(|t| t.active).map(|t| t.tab_id)
                    && self.is_active_instance()
                    && self.model.needs_birth_touch(active_id)
                {
                    // Once-EVER per instance/tab, snapshot-aware but
                    // echo-INDEPENDENT (C5 rd 4: echo-gated guards re-fired
                    // per TabUpdate → spawn storm → server fd exhaustion).
                    // `clave touch` stamps host time into the STORE and
                    // pushes the snapshot that carries the new order back
                    // to every instance.
                    run_command(
                        &[self.clave_binary.as_str(), "touch", &active_id.to_string()],
                        BTreeMap::new(),
                    );
                }
                self.run_effects(fx);
                self.fire_binds(); // fresh tab set → own-tab joins resolvable
                true
            }
            Event::PaneUpdate(manifest) => {
                let mut metas = Vec::new();
                self.plugin_panes.clear();
                for (tab_position, panes) in &manifest.panes {
                    for p in panes {
                        if p.is_plugin {
                            self.plugin_panes.push((*tab_position, p.id));
                        }
                        metas.push(PaneMeta {
                            tab_position: *tab_position,
                            pane_id: p.id,
                            is_plugin: p.is_plugin,
                            is_focused: p.is_focused,
                        });
                    }
                }
                self.model.apply_panes(metas);
                self.fire_binds(); // fresh manifest → own-tab joins resolvable
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
                    let fx = self.model.click(line as usize);
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

    fn render(&mut self, _rows: usize, cols: usize) {
        // NO announce here (round 12): render is NOT visibility-gated
        // either (every instance renders at least once after load) — the
        // render announce EMFILE-crashed the server. Announces now fire
        // only from apply_tabs on bounded triggers (birth / clave-organic).
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
        let rows: Vec<Row> = self.model.rows().into_iter().map(|(_, row)| row).collect();
        for line in render_rows(&rows, cols, self.model.widths()) {
            println!("{line}");
        }
    }
}

// NOTE: no `fn main()` — register_plugin! supplies the wasm entry point (a
// second one is E0428; confirmed in foundation Task 1).
