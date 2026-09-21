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

    /// **The fast band holds ONE timer.** Branch-review finding, recorded in
    /// `docs/status/2026-09-13-swarm-review-fixes.md` under `31844a3`.
    ///
    /// `model::BarModel`'s `swap_owed`
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

    /// The direction rule lives in the model and is tested there, but the
    /// boolean reaches zellij only through this arm, and `main.rs` does not
    /// link on the host. Inverting the two host calls passes every test and
    /// the wasm build, and only an eyeball catches the 0.2s lag coming back
    /// (blind review of the 2026-09-21 fix). So the mapping is pinned as
    /// text: backwards is `previous_swap_layout`, forward is `next`.
    #[test]
    fn the_swap_direction_maps_backwards_to_previous_and_forward_to_next() {
        for (arm, call) in [
            (
                "Effect::SwapWidth { backwards: true }",
                "previous_swap_layout()",
            ),
            (
                "Effect::SwapWidth { backwards: false }",
                "next_swap_layout()",
            ),
        ] {
            assert!(
                SHELL.contains(&format!("{arm} => {call}")),
                "the effect arm `{arm}` must call `{call}` in main.rs"
            );
        }
        assert_eq!(
            SHELL.matches("previous_swap_layout()").count(),
            1,
            "previous_swap_layout has exactly one call site, the effect arm"
        );
    }

    /// The shell holds no copy of "a tick is in flight". It held one until
    /// 2026-09-13, mirrored into the model, and the pair needed a test that
    /// read the two writes as adjacent lines of text to stay together. The
    /// model owns the fact now, so the property is a type instead of a
    /// string search — and this is what stops the copy coming back.
    #[test]
    fn the_shell_keeps_no_second_copy_of_the_armed_flag() {
        assert!(
            !SHELL.contains("fast_armed"),
            "the fast-tick flag is the model's (`BarModel::arm_fast_tick`); a \
             shell-side copy is one fact in two places, and the disarm that \
             forgets the other buys every later ask a second tick forever"
        );
    }

    /// The spinner is armed over what the pane SHOWS, not over the fleet. A
    /// row mid-turn below the viewport draws no spinner, so arming for it wakes
    /// the bar five times a second to animate a glyph nobody can see — the
    /// quiescence class the drive's phase 6 measures. `visible_rows` is the
    /// same call the paint makes on the next line, so the two cannot disagree
    /// about what is on screen.
    #[test]
    fn the_spinner_is_armed_over_the_visible_slice() {
        let render = fn_body("render");
        assert!(
            render.contains("arm_spinner(clave_bar::render::visible_rows("),
            "the spinner must be armed from the viewport slice, not the whole \
             row list:\n{render}"
        );
    }

    /// Every event heals the fast band, and every event advances the clock
    /// the heal is judged on. `render` alone used to tick it (#232), which
    /// made the strand window age in paints — and a stale claim is exactly
    /// what stops the paints, because the spinner is re-armed by one.
    #[test]
    fn every_event_ages_the_fast_band_and_can_drop_a_stale_claim() {
        let update = fn_body("update");
        assert!(
            update.contains("self.model.tick(wall_now());"),
            "the clock must advance on every event, or a tick armed between \
             paints carries a stamp minutes old and is judged stale at \
             once:\n{update}"
        );
        assert!(
            update.contains("expire_stale_fast_tick()"),
            "a timer expiry must be allowed to drop a claim the host never \
             honoured; leave it to the next paint and the paint never \
             comes:\n{update}"
        );
        assert!(
            update.contains("self.pending_peeks > 0 || self.term_poll_armed"),
            "the elimination needs the OTHER kinds' outstanding timers, read \
             before the legs below clear them; without it a misreported fast \
             expiry is recovered two seconds late, if at all:\n{update}"
        );
    }

    /// Every counted peek is a REAL outstanding timer.
    ///
    /// This is what makes the fast band's recovery a proof instead of a hope:
    /// a claim never outlives every outstanding timer, so some expiry always
    /// arrives to drop it. The peek leg can consume a fast expiry the host
    /// reported in its band, and the argument that the peek's own timer is
    /// still to come holds only while `pending_peeks` counts timers that were
    /// actually armed. A bare increment — a "peek pending" flag set without a
    /// `set_timeout` — silently ends the wake-up chain.
    #[test]
    fn every_counted_peek_has_a_timer_behind_it() {
        let lines: Vec<&str> = SHELL.lines().collect();
        let counted = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains("pending_peeks += 1"));
        let mut seen = 0;
        for (i, l) in counted {
            seen += 1;
            let next = lines.get(i + 1).copied().unwrap_or("");
            assert!(
                next.contains("set_timeout(PEEK_SINK_SECS)"),
                "line {} counts a peek without arming its timer:\n{l}\n{next}",
                i + 1
            );
        }
        assert!(
            seen > 0,
            "the search string went stale — no `pending_peeks += 1` in main.rs"
        );
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
