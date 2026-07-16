//! clave-bar — the vertical dynamic tab bar (spec §6.6). This file is a THIN
//! adapter: zellij events/pipes in → model.rs (pure, host-tested) → Effects
//! out. Keep logic out of here; if you're writing an `if` about ordering,
//! glyphs, or renames, it belongs in model.rs where it can be unit-tested.

use std::collections::BTreeMap;

// The pure model lives in the LIB half of this crate (src/lib.rs → model.rs)
// so it host-tests without linking this bin's wasm host-import shims.
use clave_bar::model::{BarModel, Effect, PaneMeta, TabMeta};
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
    /// Every bar pane's (pane_id, x, cols) from the last PaneUpdate — the
    /// timer-paced repair retry works off this cache (round 17). Stale x
    /// is safe: move-left on a leftmost pane is a zellij no-op; a stale
    /// cols at worst re-fires one step and the learner's grow recovers.
    last_bars: Vec<(u32, Option<usize>, usize)>,
    /// Remaining timer-paced repair ticks (armed on toggle-show). A hard
    /// cap so the timer chain always terminates even if some pane never
    /// converges (non-executor instances stay armed forever by design).
    repair_timer_ticks: u8,
}

/// Seconds between timer-paced repair retries — comfortably longer than
/// zellij needs to apply a resize, so a retry means "the fire was lost",
/// not "it hasn't landed yet" (the round-16 double-fire failure).
const REPAIR_RETRY_SECS: f64 = 0.4;
/// Timer ticks armed per toggle-show (~12s of paced retries).
const REPAIR_TIMER_TICKS: u8 = 30;

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
                // C6 repair effects carry their target pane (round 16: the
                // executor heals EVERY tab's bar — pane ids are global and
                // these commands work cross-tab). Gated at the call sites.
                Effect::MoveBarLeft { pane_id } => {
                    move_pane_with_pane_id_in_direction(PaneId::Plugin(pane_id), Direction::Left);
                }
                Effect::ShrinkBar { pane_id } => {
                    resize_pane_with_id(
                        ResizeStrategy::new(Resize::Decrease, Some(Direction::Right)),
                        PaneId::Plugin(pane_id),
                    );
                }
                Effect::GrowBar { pane_id } => {
                    resize_pane_with_id(
                        ResizeStrategy::new(Resize::Increase, Some(Direction::Right)),
                        PaneId::Plugin(pane_id),
                    );
                }
                _ => {} // non-active instance skips writes
            }
        }
    }

    /// The EXECUTOR gate: is this instance's own tab the replicated beacon?
    /// The only trustworthy "am I the one on screen" signal —
    /// is_active_instance() is degenerate on hidden instances (their stale
    /// tab sets always claim their own tab is active, C3), which is exactly
    /// how round-11's repair storm started: toggle-show broadcast layout
    /// events to every instance, and hidden ones "repaired" real panes in
    /// other tabs off stale geometry, each change feeding the next.
    fn is_executor(&self) -> bool {
        self.own_tab_id()
            .is_some_and(|own| self.model.current_tab() == Some(own))
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

    /// Alt+c: hide, or re-show WITHOUT stealing focus. `show_self()` is a
    /// focus action server-side (Action::FocusPluginPaneWithId → switches to
    /// the pane's tab); on toggle-show EVERY hidden instance runs this, so N
    /// racing focus calls threw the user onto an arbitrary tab (round 14).
    /// `show_pane_with_id(.., should_focus_pane=false)` routes to
    /// UnsuppressOrExpandPane instead: restores the pane into the tab that
    /// owns it, no focus, no tab switch.
    fn toggle_hidden(&mut self) {
        let hidden = self.model.toggle();
        // TEMP round-15 trace — remove after C6 closes.
        eprintln!("clave-bar: TRACE toggle hidden={hidden}");
        if hidden {
            hide_self();
        } else {
            if let Some(own) = self.own_plugin_id {
                show_pane_with_id(PaneId::Plugin(own), false, false);
            } else {
                show_self(false); // pre-load fallback; can't happen post-load()
            }
            // Arm the paced repair-retry chain (round 17): the unsuppress
            // burst clobbers resizes fired mid-burst, and hidden tabs may
            // get no further events to chain off — the timer both re-fires
            // lost resizes and ticks event-starved panes along.
            self.repair_timer_ticks = REPAIR_TIMER_TICKS;
            set_timeout(REPAIR_RETRY_SECS);
        }
    }

    /// One pipe message → model. Split out of pipe() so early returns here
    /// can't skip the unconditional unblock (dd38ace — see pipe()).
    fn handle_pipe(&mut self, message: PipeMessage) -> bool {
        let name = message.name.as_str();
        let Some(payload) = message.payload.as_deref() else {
            // Toggle carries no payload; everything else must.
            if name == "clave-toggle" {
                self.toggle_hidden();
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
                    // Beacon only (executor election) — never reorders.
                    self.model.beacon(tab_id);
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
                self.toggle_hidden();
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
            EventType::Timer, // paced C6 repair retries (round 17)
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
                // Every bar pane's geometry, all tabs (round 16: the
                // executor heals them all): (pane_id, x, cols).
                let mut bars: Vec<(u32, Option<usize>, usize)> = Vec::new();
                self.plugin_panes.clear();
                for (tab_position, panes) in &manifest.panes {
                    for p in panes {
                        if p.is_plugin {
                            self.plugin_panes.push((*tab_position, p.id));
                            bars.push((p.id, Some(p.pane_x), p.pane_columns));
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
                self.last_bars = bars.clone(); // timer retries work off this
                self.fire_binds(); // fresh manifest → own-tab joins resolvable
                // C6 re-show repair, move phase (needs x): width steps also
                // chain via render() for the visible tab — zellij sends no
                // PaneUpdate for the plugin's own resize's effect (round
                // 10). EXECUTOR-ONLY (round 11): hidden instances' stale
                // manifests fed a repair/announce storm; the one fresh
                // instance now repairs every tab's bar instead (round 16).
                if self.is_executor() {
                    let fx = self.model.repair_tick(&bars);
                    self.run_effects(fx);
                } else if self.model.repair_armed() {
                    // TEMP round-15 trace — remove after C6 closes.
                    eprintln!(
                        "clave-bar: TRACE repair skipped (PaneUpdate) own_tab={:?} beacon={:?}",
                        self.own_tab_id(),
                        self.model.current_tab()
                    );
                }
                true
            }
            Event::Timer(_) => {
                // Paced C6 repair retry (round 17): clear in-flight guards
                // and re-fire anything the unsuppress burst clobbered.
                // Executor-only like every repair path; the chain re-arms
                // while work remains, hard-capped by repair_timer_ticks.
                let mut acted = false;
                if self.repair_timer_ticks > 0 {
                    self.repair_timer_ticks -= 1;
                    if self.model.repair_armed() {
                        if self.is_executor() {
                            let bars = self.last_bars.clone();
                            let fx = self.model.repair_retry_tick(&bars);
                            // TEMP round-17 trace — remove after C6 closes.
                            eprintln!(
                                "clave-bar: TRACE timer retry ticks_left={} fired={} bars={bars:?}",
                                self.repair_timer_ticks,
                                fx.len()
                            );
                            acted = !fx.is_empty();
                            self.run_effects(fx);
                        }
                        set_timeout(REPAIR_RETRY_SECS);
                    }
                }
                acted
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
        // C6 repair, width phase: each of our resizes triggers a repaint
        // (not a PaneUpdate), so chaining here converges within one visit
        // instead of one step per tab activation (round 10). `cols` IS the
        // pane's live width; x is unknowable here (None → width only).
        // EXECUTOR-ONLY, same as the move phase (round 11 storm).
        if let Some(own) = self.own_plugin_id
            && self.is_executor()
        {
            let fx = self.model.repair_tick(&[(own, None, cols)]);
            self.run_effects(fx);
        } else if self.model.repair_armed() {
            // TEMP round-15 trace — remove after C6 closes.
            eprintln!(
                "clave-bar: TRACE repair skipped (render) own_tab={:?} beacon={:?}",
                self.own_tab_id(),
                self.model.current_tab()
            );
        }
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
