//! clave-bar — the vertical dynamic tab bar (spec §6.6). This file is a THIN
//! adapter: zellij events/pipes in → model.rs (pure, host-tested) → Effects
//! out. Keep logic out of here; if you're writing an `if` about ordering,
//! glyphs, or renames, it belongs in model.rs where it can be unit-tested.

use std::collections::BTreeMap;

// The pure model lives in the LIB half of this crate (src/lib.rs → model.rs)
// so it host-tests without linking this bin's wasm host-import shims.
use clave_bar::model::{
    BarModel, DWELL_SECS, Effect, PEEK_SINK_SECS, PaneMeta, TIMER_KIND_CUTOFF_SECS, TabMeta,
};
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
    /// keeps it expanded until ~1s after the final press.
    pending_peeks: u32,
    /// Dwell timers in flight (§6.6 C8): each dormant-row landing arms one
    /// set_timeout(DWELL_SECS). All share one duration, so they fire in arm
    /// order — FIFO gen matching is exact.
    pending_dwells: std::collections::VecDeque<u64>,
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
                Effect::RenameTab { tab_id, name } if active => {
                    rename_tab_with_id(tab_id as u64, name);
                }
                Effect::MarkRead { uuid } if active => {
                    // Persist the unread clear (§6.5). Fire-and-forget; the
                    // local repaint already happened in the model.
                    run_command(&["clave", "focus", &uuid], BTreeMap::new());
                }
                Effect::Bind { uuid, tab_id } if active => {
                    // Report the uuid→tab join to the store (§6.6 Design B);
                    // `clave bind` RMWs and pushes the snapshot that carries
                    // it to every instance.
                    run_command(
                        &["clave", "bind", &uuid, &tab_id.to_string()],
                        BTreeMap::new(),
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
                Effect::ArmDwell { r#gen } => {
                    self.pending_dwells.push_back(r#gen);
                    set_timeout(DWELL_SECS);
                }
                Effect::ArmPeek => {
                    self.pending_peeks += 1;
                    set_timeout(PEEK_SINK_SECS);
                }
                Effect::OpenAgent { uuid } => {
                    run_command(&["clave", "open", &uuid], BTreeMap::new());
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
        self.model.toggle();
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
                let fx = self.model.nav(payload, executor);
                if fx.is_empty() {
                    return false; // non-executor, or unresolvable payload
                }
                self.run_effects(fx);
                true // the beacon moved → active-row highlight repaint
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
    fn load(&mut self, _config: BTreeMap<String, String>) {
        // Version marker for the hot-reload workflow (`zellij action
        // start-or-reload-plugin`): stamp the build so the zellij log tells
        // you WHICH wasm produced a trace. Set by the rebuild recipe via
        // CLAVE_BUILD_TAG; "dev" means an untagged local build.
        eprintln!(
            "clave-bar: loaded v{} build={}",
            env!("CARGO_PKG_VERSION"),
            option_env!("CLAVE_BUILD_TAG").unwrap_or("dev")
        );
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
                run_command(&["clave", "snapshot"], BTreeMap::new());
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
                    run_command(&["clave", "touch", &active_id.to_string()], BTreeMap::new());
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
            Event::Timer(elapsed) => {
                // TWO timer kinds share this event; Timer carries the ELAPSED
                // sleep (≈ requested duration, v0.44.3 zellij_exports.rs:2462)
                // — 0.4s dwells and 0.9s peek sinks split cleanly at the
                // cutoff.
                if elapsed < TIMER_KIND_CUTOFF_SECS {
                    let Some(r#gen) = self.pending_dwells.pop_front() else {
                        return false;
                    };
                    let fx = self.model.dwell_expired(r#gen);
                    let fired = !fx.is_empty();
                    self.run_effects(fx);
                    fired // repaint: the row flips to ↻
                } else {
                    // One expiry per armed peek; only the LAST sinks (nav
                    // burst = one visible expand, one sink). peek_expired() is
                    // false when a toggle already cancelled the peek — no
                    // repaint.
                    self.pending_peeks = self.pending_peeks.saturating_sub(1);
                    self.pending_peeks == 0 && self.model.peek_expired()
                }
            }
            Event::Mouse(Mouse::LeftClick(line, _col)) => {
                // §6.6: rows are mouse-clickable. line is the rendered row.
                if line >= 0 {
                    let fx = self.model.click(line as usize);
                    self.run_effects(fx);
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
        // One line per tab, display-ordered. Active row inverted (SGR 7);
        // agent rows get their state glyph; plain tabs a 2-space gutter so
        // names align. Truncate to the pane width (raw ANSI is S1-proven).
        for row in self.model.rows() {
            let gutter = match row.glyph {
                Some((glyph, colour)) => format!("\u{1b}[{colour}m{glyph}\u{1b}[0m "),
                None => "  ".to_string(),
            };
            // Clamp the NAME to what's left after the 2-cell gutter, with a
            // trailing … (char-boundary safe; labels can be multibyte).
            let budget = cols.saturating_sub(3); // gutter + margin
            let name: String = if row.name.chars().count() > budget {
                let mut n: String = row.name.chars().take(budget.saturating_sub(1)).collect();
                n.push('…');
                n
            } else {
                row.name.clone()
            };
            if row.active {
                println!("{gutter}\u{1b}[7m{name}\u{1b}[0m");
            } else {
                println!("{gutter}{name}");
            }
        }
    }
}

// NOTE: no `fn main()` — register_plugin! supplies the wasm entry point (a
// second one is E0428; confirmed in foundation Task 1).
