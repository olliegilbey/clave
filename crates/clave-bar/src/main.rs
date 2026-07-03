//! clave-bar — the Zellij WASM plugin that renders the agent sidebar.
//! S1 scope: consume `clave-status` snapshots and render colored glyphs.
//! S2 scope: map uuid → pane_id (from `clave-register`) and, on a `clave-nav`
//! message, jump focus straight to that agent's pane with `focus_pane_with_id`
//! (Zellij pulls the pane's tab forward — no PaneManifest / tab-index needed).

use std::collections::BTreeMap;
use zellij_tile::prelude::*;
use clave_types::{Agent, AgentSnapshot, Register, Status};

#[derive(Default)]
struct State {
    /// Highest snapshot seq applied so far (stale messages are discarded).
    seq: u64,
    agents: Vec<Agent>,
    /// uuid → pane_id, learned from `clave-register` messages (spec §6.1). This
    /// is the entire join: on nav, `focus_pane_with_id(Terminal(pane_id))` pulls
    /// the pane's tab forward, so no PaneManifest-derived pane→tab map is needed
    /// (S2 finding — see the nav arm below).
    uuid_to_pane: BTreeMap<String, u32>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        // ReadCliPipes: receive the status/register/nav pipes.
        // ChangeApplicationState: call focus_pane_with_id (moves focus).
        // NB (S2 finding): Zellij permission grants are ALL-OR-NOTHING per
        // plugin. The pre-seeded permissions.kdl cache must grant this EXACT set
        // (or a superset) under the plugin's location key, or Zellij raises a
        // prompt — unanswerable in a narrow bar pane — and withholds ALL of
        // them, which then hangs every `zellij pipe`. `clave setup` must seed it.
        request_permission(&[
            PermissionType::ReadCliPipes,
            PermissionType::ChangeApplicationState,
        ]);
    }

    fn update(&mut self, _event: Event) -> bool {
        // We subscribe to no events — the bar is driven entirely by pushed pipes
        // (spec §5/§11). Kept as the trait requires it.
        false
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        // A CLI pipe (`zellij pipe`) BLOCKS its input side until a plugin
        // unblocks it — capture the pipe_id now so we can release it after
        // handling (see the unblock call at the end of this fn). Keybind-sourced
        // messages (the production nav trigger, spec §6.6) carry no pipe_id.
        let cli_pipe_id = match &message.source {
            PipeSource::Cli(id) => Some(id.clone()),
            _ => None,
        };
        let repaint = match message.name.as_str() {
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
                // Payload: {"uuid":"..."} — jump focus to that agent's pane.
                let Some(payload) = message.payload else { return false };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload) else {
                    return false;
                };
                let Some(uuid) = v.get("uuid").and_then(|u| u.as_str()) else {
                    return false;
                };
                if let Some(pane) = self.uuid_to_pane.get(uuid) {
                    // Focus the registered TERMINAL pane directly by id — Zellij
                    // brings its containing tab forward automatically. S2 finding:
                    // this replaces the planned go_to_tab(tab_index) approach,
                    // which was called with the right value yet was a silent
                    // no-op (0- vs 1-based tab-index mismatch) AND required a
                    // PaneManifest→pane→tab map we no longer keep.
                    focus_pane_with_id(PaneId::Terminal(*pane), false, false);
                }
                false
            }
            _ => false,
        };
        // Release the blocked input side of the CLI pipe so `zellij pipe` RETURNS
        // instead of hanging on its bidirectional connection. (Production nav is a
        // keybind, not a CLI pipe, so this only affects the spike's shell driver.)
        if let Some(id) = cli_pipe_id {
            unblock_cli_pipe_input(&id);
        }
        repaint
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
