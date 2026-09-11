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

/// Invariants of the wasm shell (`main.rs`) that no unit test can reach.
///
/// The bin does not link on the host — its `set_timeout`/`next_swap_layout`
/// calls resolve to the zellij host import `host_run_plugin_command`, which
/// has no symbol here — so the shell's timer discipline has nothing watching
/// it. `include_str!` needs no linker. These live in the LIB's own tests and
/// not in `tests/`: an integration test makes cargo build the bin so the test
/// could exec it, and that build is the link failure `test = false` exists to
/// avoid. The precedent for reading an artifact as text is
/// `crates/clave/tests/zellij_pin_tripwire.rs`.
#[cfg(test)]
mod shell_text {
    const SHELL: &str = include_str!("main.rs");

    /// **The fast band holds ONE timer.** `model::BarModel`'s `swap_owed`
    /// asks for two fast ticks when one was already in flight at the instant
    /// of a switch ask, and that count is exact only while there is exactly
    /// one fast timer to count. `Event::Timer` carries the elapsed seconds
    /// and nothing else, so 0.15 and 0.2 are the same reading: while the
    /// spinner and the width cooldown each armed their own, a cooldown expiry
    /// disarmed a spinner whose frame was still pending, the next paint armed
    /// a SECOND frame, and two pending frames spend both owed ticks inside
    /// one 0.15s deafness — the bug the count was added to prevent, restored
    /// by the mechanism meant to fix it.
    #[test]
    fn the_fast_tick_is_armed_from_exactly_one_place() {
        let arms = SHELL.matches("set_timeout(FAST_TICK_SECS)").count();
        assert_eq!(
            arms, 1,
            "the fast band must hold ONE timer; found {arms} \
             set_timeout(FAST_TICK_SECS) call sites in main.rs"
        );
        let funnel = fn_body("arm_fast_tick");
        assert!(
            funnel.contains("set_timeout(FAST_TICK_SECS)"),
            "the arming call left `arm_fast_tick`, so a caller can now arm a \
             second timer past its already-armed guard:\n{funnel}"
        );
    }

    /// The shell's flag and the model's mirror are one fact in two places. A
    /// disarm that forgot the mirror would buy every later ask a second tick
    /// forever — twice the deafness on a fleet that stopped spinning — and an
    /// arm that forgot it would buy none, which is the early judgement the
    /// count exists to prevent.
    #[test]
    fn the_armed_flag_and_its_mirror_are_written_together() {
        let lines: Vec<&str> = SHELL.lines().collect();
        let edges: Vec<(usize, &str)> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains("self.fast_armed = "))
            .map(|(i, l)| (i, *l))
            .collect();
        assert_eq!(
            edges.len(),
            2,
            "expected exactly the arm and the disarm, found {edges:?}"
        );
        for (at, line) in edges {
            let armed = line.contains("= true");
            let mirror = format!("set_fast_tick_armed({armed})");
            assert!(
                lines[at + 1].contains(&mirror),
                "`{}` is not followed by `{mirror}` — the model's mirror has \
                 drifted from the shell's timer",
                line.trim()
            );
        }
    }

    /// The body of a `fn` in the shell, signature to the next one at the same
    /// indentation. Crude on purpose: it only has to be sharp enough to say
    /// which function a line of text sits in.
    fn fn_body(name: &str) -> &'static str {
        let at = SHELL
            .find(&format!("fn {name}("))
            .unwrap_or_else(|| panic!("main.rs has no `fn {name}`"));
        let rest = &SHELL[at..];
        let end = rest.find("\n    fn ").unwrap_or(rest.len());
        &rest[..end]
    }
}
