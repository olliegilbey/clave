//! clave-bar — the Zellij WASM plugin that renders the agent sidebar.
//! S1 scope: consume `clave-status` snapshots and render colored glyphs.
//! S2 scope: map uuid → pane_id (from `clave-register`) → live tab position
//! (from `PaneManifest`) and `go_to_tab` on a `clave-nav {uuid}` message.

use std::collections::BTreeMap;
use zellij_tile::prelude::*;
use clave_types::{Agent, AgentSnapshot, Register, Status};

#[derive(Default)]
struct State {
    /// Highest snapshot seq applied so far (stale messages are discarded).
    seq: u64,
    agents: Vec<Agent>,
    /// uuid → pane_id, learned from `clave-register` messages (spec §6.1).
    uuid_to_pane: BTreeMap<String, u32>,
    /// pane_id → tab position, rebuilt from every `PaneManifest` (spec §6.6/S2).
    pane_to_tab: BTreeMap<u32, usize>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ReadCliPipes, // receive status/register/nav pipes
            // PaneUpdate delivers the PaneManifest (Zellij pane+tab state), which is
            // gated behind ReadApplicationState — without it the subscription silently
            // never fires and pane_to_tab stays empty, so every clave-nav no-ops.
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState, // call go_to_tab
        ]);
        // PaneUpdate delivers the PaneManifest we use for pane→tab resolution.
        subscribe(&[EventType::PaneUpdate]);
    }

    fn update(&mut self, event: Event) -> bool {
        if let Event::PaneUpdate(manifest) = event {
            // manifest.panes: tab_position -> panes in that tab.
            self.pane_to_tab.clear();
            for (tab_index, panes) in manifest.panes {
                for p in panes {
                    // Terminal and plugin panes have SEPARATE id spaces in Zellij
                    // (PaneId::Terminal(u32) vs Plugin(u32)). The `$ZELLIJ_PANE_ID`
                    // we register with (Step 4) is a TERMINAL id, so a plugin pane
                    // sharing that number would false-match. Map terminal panes only.
                    if !p.is_plugin {
                        self.pane_to_tab.insert(p.id, tab_index);
                    }
                }
            }
        }
        false // no repaint needed for join-map bookkeeping
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        match message.name.as_str() {
            "clave-status" => {
                let Some(payload) = message.payload else { return false };
                let Ok(snap) = serde_json::from_str::<AgentSnapshot>(&payload) else {
                    return false;
                };
                // Full-replace + monotonic seq (spec §5): apply only strictly-newer
                // snapshots; discard stale/out-of-order without repainting.
                if snap.seq <= self.seq {
                    return false;
                }
                self.seq = snap.seq;
                self.agents = snap.agents;
                true // request a re-render
            }
            "clave-register" => {
                let Some(payload) = message.payload else { return false };
                if let Ok(reg) = serde_json::from_str::<Register>(&payload) {
                    self.uuid_to_pane.insert(reg.uuid, reg.pane_id);
                }
                false
            }
            "clave-nav" => {
                // Payload: {"uuid":"..."} — jump focus to that agent's tab.
                let Some(payload) = message.payload else { return false };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload) else {
                    return false;
                };
                let Some(uuid) = v.get("uuid").and_then(|u| u.as_str()) else {
                    return false;
                };
                if let Some(pane) = self.uuid_to_pane.get(uuid) {
                    if let Some(tab) = self.pane_to_tab.get(pane) {
                        // NOTE: confirm go_to_tab indexing during the spike —
                        // PaneManifest tab keys and go_to_tab may differ by 1.
                        go_to_tab((*tab as u32) + 1);
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        for a in &self.agents {
            // One glyph; the FONT COLOUR encodes state (spec §6.5). Raw ANSI SGR
            // codes — Zellij interprets escape sequences in plugin output.
            let (glyph, color) = match a.status {
                Status::NeedsYou => ('●', 31), // red
                Status::Working => ('●', 33),  // amber / yellow
                Status::Done => ('●', 32),     // green
                Status::Idle => ('●', 90),     // dim (bright black)
                Status::Failed => ('✖', 31),   // red cross
            };
            println!("\u{1b}[{color}m{glyph}\u{1b}[0m {}", a.label);
        }
    }
}

// NOTE: no `fn main()` here — on zellij-tile 0.44 `register_plugin!` expands to
// its OWN `main` (the wasm entry point) alongside the plugin exports
// (load/update/render/pipe). A second `main` is a duplicate-definition error
// (E0428). Confirmed in Task 1: the macro is the sole source of `main`.
