//! Host-testable library half of clave-bar. The wasm plugin entry point lives
//! in `main.rs` (a thin zellij adapter); this lib holds the zellij-tile-free
//! logic that unit-tests on the host — `model` (the state machine),
//! `plugin_config` (the #44 binary resolver plus its shellout guard) and
//! `render` (the 44-column row renderer, plus the preview example that is
//! driven by it so the two cannot diverge) and `card` (its two-line
//! double-height counterpart, #232).
//!
//! Why the split is load-bearing: the bin references wasm host-import shims
//! (`host_run_plugin_command`, via `focus_pane_with_id`/`run_command`/…) that
//! have no symbol on the host target, so the *binary* can never link for host
//! — only for wasm32-wasip1 (hence `test = false` on the bin in Cargo.toml).
//! These lib modules import NO zellij-tile SHIMS, so as a lib target they
//! compile and test cleanly on the host. main.rs consumes them via `use
//! clave_bar::…`. (`theme` imports zellij-tile DATA types — re-exported
//! `zellij-utils::data`, pure serde structs with host symbols — which is what
//! lets the theme mapping be host-tested instead of living in this bin.)

pub mod card;
pub mod model;
pub mod pipe;
pub mod plugin_config;
pub mod render;
pub mod theme;
