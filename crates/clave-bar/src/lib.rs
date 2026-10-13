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
        // The arms moved behind the paint gate (`handle_event`); the clock
        // stays at the door, so it still advances first.
        let events = fn_body("handle_event");
        assert!(
            events.contains("expire_stale_fast_tick()"),
            "a timer expiry must be allowed to drop a claim the host never \
             honoured; leave it to the next paint and the paint never \
             comes:\n{events}"
        );
        assert!(
            events.contains("self.pending_peeks > 0 || self.term_poll_armed"),
            "the elimination needs the OTHER kinds' outstanding timers, read \
             before the legs below clear them; without it a misreported fast \
             expiry is recovered two seconds late, if at all:\n{events}"
        );
    }

    /// Zellij paints whatever `update` or `pipe` says yes to, on screen or
    /// not. Both must answer through `BarModel::paints`, or every off-screen
    /// bar paints every push again: 13 of every 14 paints, measured
    /// 2026-10-13 with 14 tabs.
    #[test]
    fn every_entry_point_answers_through_the_paint_gate() {
        for (entry, answer) in [
            ("update", "self.model.paints(wants)"),
            ("pipe", "self.model.paints(repaint)"),
        ] {
            let body = fn_body(entry);
            assert_eq!(
                last_expr(body),
                answer,
                "`{entry}` must answer through the paint gate:\n{body}"
            );
        }
        let update = fn_body("update");
        assert!(
            update.contains("let wants = self.handle_event(event);"),
            "every event must reach the model, shown or hidden: a hidden bar \
             binds its own tab's agents:\n{update}"
        );
        let pipe = fn_body("pipe");
        assert!(
            pipe.contains("self.model.tick(wall_now());"),
            "a hidden bar no longer paints, so its clock must move on the \
             pipes it still takes:\n{pipe}"
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

    /// A probe that sees a change must buy a paint. The manifest arm once
    /// dropped the answer, and a command that exited between two identical
    /// frames read "running" until some other change painted the row.
    #[test]
    fn no_probe_answer_is_dropped() {
        let mut seen = 0;
        for l in SHELL
            .lines()
            .filter(|l| l.contains("self.probe_term_facts()"))
        {
            seen += 1;
            assert!(
                l.contains("= self.probe_term_facts()") && !l.contains("let _"),
                "a probe's answer is dropped here:\n{l}"
            );
        }
        assert!(seen > 0, "the search string went stale");
        assert!(
            fn_body("handle_event").contains("changed || acted || probed"),
            "the manifest arm must paint on the probe's answer"
        );
    }

    /// A frame is news only when compared BEFORE it is applied: applied
    /// first, it always equals the model, and no frame paints. Each manifest
    /// arm must also answer with that news, or a changed frame paints only
    /// when an effect happens to run (adversarial review of #279).
    #[test]
    fn each_frame_is_compared_before_it_is_applied() {
        let events = fn_body("handle_event");
        for (differs, apply, answer) in [
            ("tab_frame_differs(", "apply_tabs(", "changed || acted\n"),
            (
                "pane_frame_differs(",
                "apply_panes(",
                "changed || acted || probed",
            ),
        ] {
            let compared = events.find(differs).expect("the diff call went stale");
            let applied = events.find(apply).expect("the apply call went stale");
            assert!(
                compared < applied,
                "`{differs}` must run before `{apply}`:\n{events}"
            );
            let rest = &events[applied..];
            let arm = &rest[..rest.find("Event::").unwrap_or(rest.len())];
            assert!(
                arm.contains(answer),
                "the arm with `{apply}` must answer `{}`:\n{arm}",
                answer.trim()
            );
        }
    }

    /// The paint gate is only as good as the word it reads. Without the
    /// subscription, every bar stays "on screen" to the model and paints
    /// every push again; without the arm's answer, a bar that comes back on
    /// screen shows its last frame until some other change paints it.
    /// `main.rs` is outside `just mutants`, so these pins are its test.
    #[test]
    fn the_bar_hears_visibility_and_paints_on_return() {
        let load = fn_body("load");
        assert!(
            load.contains("EventType::Visible,"),
            "the bar must subscribe to `Visible`:\n{load}"
        );
        let events = fn_body("handle_event");
        let at = events
            .find("Event::Visible(visible) => {")
            .expect("the Visible arm went stale");
        let arm = &events[at..];
        let arm = &arm[..arm.find("\n            }").unwrap_or(arm.len())];
        assert_eq!(
            last_expr(arm),
            "visible // a returning bar paints what it took while away",
            "the Visible arm must answer `visible`:\n{arm}"
        );
    }

    #[test]
    fn every_bar_runs_its_own_hydrate_and_every_snapshot_goes_through_the_push_gate() {
        // `run_effects` ends in a catch-all that drops the gated writes on a
        // background bar. A guarded or missing `Hydrate` arm falls into it, and
        // that bar never rebuilds a slim push again.
        let effects = fn_body("run_effects");
        let arm = effects
            .find("Effect::Hydrate => {")
            .expect("`Effect::Hydrate` needs an unguarded arm of its own");
        let catch_all = effects
            .find("_ => {} // non-active instance skips writes")
            .expect("the catch-all went stale");
        assert!(
            arm < catch_all,
            "the Hydrate arm must come before the catch-all"
        );
        // Both snapshot paths (pipe and hydrate) meet in one method, and it
        // must resolve the dormant rows before the model sees the snapshot.
        let settle = fn_body("apply_snapshot_and_settle");
        assert!(
            settle.contains("self.model.apply_push(snap, mono_ms())"),
            "a snapshot must pass the push gate:\n{settle}"
        );
        // The host sends a slim push on its own name, so a bar that does not
        // listen on that name never hears most pushes.
        let pipe = fn_body("handle_pipe");
        assert!(
            pipe.contains("clave_types::STATUS_PIPE | clave_types::SLIM_STATUS_PIPE =>"),
            "both status names must reach the snapshot arm:\n{pipe}"
        );
    }

    #[test]
    fn the_bar_takes_the_keymap_once_and_not_on_every_tab_switch() {
        let load = fn_body("load");
        assert!(
            load.contains("EventType::InitialKeybinds,"),
            "without it, every ModeUpdate carries the whole keymap:\n{load}"
        );
    }

    /// The last expression of a shell `fn`: its answer.
    fn last_expr(body: &str) -> &str {
        let inner = body.trim_end().strip_suffix('}').unwrap_or(body);
        inner.trim_end().lines().last().unwrap_or("").trim()
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
