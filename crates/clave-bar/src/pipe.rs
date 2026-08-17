//! Which pipe deliveries are messages, and which are zellij's punctuation.
//!
//! Lives in the LIB half so it host-tests: `main.rs` is `test = false` (it
//! links wasm host imports), so the routing decision has to be a pure function
//! here or it is unmodelled — the escape pattern that killed `clave-organic`
//! (#128, TESTING.md escape record).

/// Is this delivery zellij's own end-of-stream marker rather than a caller's
/// message?
///
/// **Every `zellij pipe` from the CLI delivers TWICE**: the payload, then an
/// unconditional blank follow-up per plugin instance (#45; the `dropped … pipe
/// with empty payload` lines in the zellij log ARE that twin). A handler that
/// treats a blank payload as a press therefore fires the press twice from the
/// CLI while the keybind fires once — which is why a scripted collapse toggle
/// measured a no-op where a human measured a flip.
///
/// The source is load-bearing, not a nicety. A keybind `MessagePlugin` with no
/// payload attribute also delivers `payload: None`, and that one IS a press —
/// so blankness alone cannot separate the twin from the gesture. Only a CLI
/// blank is punctuation. It is never a real CLI message either: `zellij pipe`
/// with no `-- payload` (an empty string counts as absent) streams stdin
/// instead of sending, so a CLI caller's payload is always non-empty.
pub fn is_cli_blank_twin(from_cli: bool, payload: Option<&str>) -> bool {
    from_cli && payload.is_none_or(|p| p.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BarModel, Effect};

    /// `main.rs`'s toggle route, as far as this crate can test it: the guard,
    /// then the model call. The adapter itself cannot be exercised on the host.
    /// A toggle is observable as the mode it books for the store, so the
    /// effects it returns count the presses.
    fn deliver(
        model: &mut BarModel,
        name: &str,
        from_cli: bool,
        payload: Option<&str>,
    ) -> Vec<Effect> {
        if is_cli_blank_twin(from_cli, payload) {
            return Vec::new();
        }
        if name == "clave-toggle" {
            return model.toggle();
        }
        Vec::new()
    }

    /// The defect, pinned: one scripted `clave-toggle` used to flip the bar
    /// back to where it started, because the blank twin toggled it again.
    #[test]
    fn one_cli_toggle_and_its_blank_twin_toggle_exactly_once() {
        let mut m = BarModel::default();
        let press = deliver(&mut m, "clave-toggle", true, Some("1"));
        let twin = deliver(&mut m, "clave-toggle", true, None); // zellij's own
        assert_eq!(press, vec![Effect::PersistCollapse { collapsed: true }]);
        assert_eq!(twin, Vec::<Effect>::new(), "the twin is not a press");
        // And the mode really did land collapsed rather than round-tripping.
        let next = deliver(&mut m, "clave-toggle", true, Some("1"));
        assert_eq!(next, vec![Effect::PersistCollapse { collapsed: false }]);
    }

    /// The keybind is payload-less by construction (`setup.rs` binds Alt+c with
    /// no payload attribute), so the guard must not touch it.
    #[test]
    fn the_keybind_press_carries_no_payload_and_still_toggles() {
        let mut m = BarModel::default();
        assert_eq!(
            deliver(&mut m, "clave-toggle", false, None),
            vec![Effect::PersistCollapse { collapsed: true }],
            "Alt+c must still fire"
        );
    }

    /// Whitespace is blank: a script that pipes a trailing newline sends the
    /// twin's shape, not a message.
    #[test]
    fn whitespace_only_cli_payloads_are_punctuation_too() {
        assert!(is_cli_blank_twin(true, Some("   ")));
        assert!(is_cli_blank_twin(true, Some("\n")));
        assert!(!is_cli_blank_twin(true, Some("x")));
        assert!(!is_cli_blank_twin(false, Some("   ")), "not a CLI delivery");
    }
}
