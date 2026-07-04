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
                Effect::RenameTab { tab_id, name } if active => {
                    rename_tab_with_id(tab_id as u64, name);
                }
                Effect::MarkRead { uuid } if active => {
                    // Persist the unread clear (§6.5). Fire-and-forget; the
                    // local repaint already happened in the model.
                    run_command(&["clave", "focus", &uuid], BTreeMap::new());
                }
                _ => {} // non-active instance skips writes
            }
        }
    }

    /// One pipe message → model. Split out of pipe() so early returns here
    /// can't skip the unconditional unblock (dd38ace — see pipe()).
    fn handle_pipe(&mut self, message: PipeMessage) -> bool {
        let name = message.name.as_str();
        let Some(payload) = message.payload.as_deref() else {
            // Toggle carries no payload; everything else must.
            if name == "clave-toggle" {
                let hidden = self.model.toggle();
                if hidden {
                    hide_self()
                } else {
                    show_self(false)
                }
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
                    true // a row may just have gained its glyph
                }
                Err(e) => {
                    eprintln!("clave-bar: bad clave-register payload: {e}");
                    false
                }
            },
            "clave-nav" => {
                match self.model.nav(payload) {
                    Some(fx) => self.run_effects(vec![fx]),
                    None => eprintln!("clave-bar: unresolvable clave-nav {payload:?}"),
                }
                false // focus change repaints via TabUpdate anyway
            }
            "clave-toggle" => {
                let hidden = self.model.toggle();
                if hidden {
                    hide_self()
                } else {
                    show_self(false)
                }
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
        ]);
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
                self.run_effects(fx);
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
                true
            }
            Event::Mouse(Mouse::LeftClick(line, _col)) => {
                // §6.6: rows are mouse-clickable. line is the rendered row.
                if line >= 0 {
                    if let Some(fx) = self.model.click(line as usize) {
                        self.run_effects(vec![fx]);
                    }
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
