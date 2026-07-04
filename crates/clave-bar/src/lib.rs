//! Host-testable library half of clave-bar. The wasm plugin entry point lives
//! in `main.rs` (a thin zellij adapter); this lib exists ONLY so the pure
//! `model` logic unit-tests on the host.
//!
//! Why the split is load-bearing: the bin references wasm host-import shims
//! (`host_run_plugin_command`, via `focus_pane_with_id`/`run_command`/…) that
//! have no symbol on the host target, so the *binary* can never link for host
//! — only for wasm32-wasip1. `model.rs` imports NO zellij-tile, so as a lib
//! target it compiles and tests cleanly on the host. main.rs consumes it via
//! `use clave_bar::model::…`.

pub mod model;
