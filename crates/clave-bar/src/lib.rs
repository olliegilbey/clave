//! Host-testable library half of clave-bar. The wasm plugin entry point lives
//! in `main.rs` (a thin zellij adapter); this lib holds the zellij-tile-free
//! logic that unit-tests on the host — `model` (the state machine) and
//! `plugin_config` (the #44 binary resolver plus its shellout guard).
//!
//! Why the split is load-bearing: the bin references wasm host-import shims
//! (`host_run_plugin_command`, via `focus_pane_with_id`/`run_command`/…) that
//! have no symbol on the host target, so the *binary* can never link for host
//! — only for wasm32-wasip1 (hence `test = false` on the bin in Cargo.toml).
//! These lib modules import NO zellij-tile, so as a lib target they compile and
//! test cleanly on the host. main.rs consumes them via `use clave_bar::…`.

pub mod model;
pub mod plugin_config;
