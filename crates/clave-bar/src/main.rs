//! clave-bar — the Zellij WASM plugin that renders the agent sidebar.
//! Task 1 is a MINIMAL valid plugin: it proves the binary→wasm32-wasip1
//! toolchain and the `register_plugin!` wiring. Real rendering arrives in S1.

use std::collections::BTreeMap;
use zellij_tile::prelude::*;

#[derive(Default)]
struct State;

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {}
    fn update(&mut self, _event: Event) -> bool {
        false
    }
    fn render(&mut self, _rows: usize, _cols: usize) {
        print!("clave-bar");
    }
}

// NOTE (deviates from the brief): on zellij-tile 0.44, `register_plugin!`
// itself expands to a `fn main() { ... }` wasm entry point. Keeping an empty
// `fn main` here collided with it (E0428: `main` redefined). Removed — the
// macro is the sole source of `main` on this crate version.
