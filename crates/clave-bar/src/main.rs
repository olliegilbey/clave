//! clave-bar — the Zellij WASM plugin that renders the agent sidebar.
//! S1 scope: consume the authoritative `clave-status` snapshot (full-replace +
//! monotonic seq, spec §5) and render one colored status glyph per agent,
//! including for NON-focused rows, without stealing focus.

use std::collections::BTreeMap;
use zellij_tile::prelude::*;
use clave_types::{Agent, AgentSnapshot, Status};

#[derive(Default)]
struct State {
    /// Highest snapshot seq applied so far (stale messages are discarded).
    seq: u64,
    agents: Vec<Agent>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        // We only need to receive `zellij pipe` messages for S1.
        request_permission(&[PermissionType::ReadCliPipes]);
    }

    fn update(&mut self, _event: Event) -> bool {
        false
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        if message.name != "clave-status" {
            return false;
        }
        let Some(payload) = message.payload else {
            return false;
        };
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
