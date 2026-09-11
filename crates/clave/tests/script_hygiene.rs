//! The drive scripts' isolation rules, enforced instead of documented.
//!
//! On 2026-09-11 the QA drive hung the maintainer's live session. Nothing was
//! wrong with clave: phase 5b fired `clave hook` directly, setting
//! `CLAVE_STATE_DIR` (so the store write landed in the sandbox, correctly) and
//! leaving `ZELLIJ_SESSION_NAME` inherited from the surrounding fleet — so the
//! snapshot push that every hook makes was aimed, precisely and by design, at
//! HIS bar. FOOTGUNS #281 had already described that trap and named
//! `scripts/ct.sh --hook` as the only sanctioned way to drive a hook.
//!
//! The rule existed. Nothing checked it. These tests are the check, because a
//! convention that only lives in a comment is one edit from being gone —
//! `clave hook`'s own guard (`hook.rs::aim_push`) now refuses a misaimed push,
//! and this is the other half: the drive must not rely on being caught.
//!
//! Source text rather than behaviour, for the same reason as
//! `clave-bar/tests/zellij_pin_tripwire.rs`: the property is about what the
//! script SAYS, and running the drive to find out requires a live sandbox.

const DRIVE: &str = include_str!("../../../scripts/qa-drive.sh");
const CT: &str = include_str!("../../../scripts/ct.sh");

/// Lines that actually run something — comments and blanks carry no risk, and
/// both scripts discuss the hazard at length in prose that must not trip this.
fn code_lines(src: &str) -> impl Iterator<Item = (usize, &str)> {
    src.lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
}

/// Lines that only REPORT. The drive narrates itself constantly, and its
/// prose is full of the word `hook` — including whole paragraphs about this
/// very hazard. None of it runs anything.
fn is_reporting(line: &str) -> bool {
    const REPORTERS: [&str; 8] = [
        "measure ", "check ", "check_", "printf ", "echo ", "cat ", "usage ", "phase ",
    ];
    REPORTERS.iter().any(|r| line.starts_with(r))
}

#[test]
fn the_drive_never_fires_a_hook_except_through_ct_sh() {
    let offenders: Vec<_> = code_lines(DRIVE)
        .filter(|(_, l)| !is_reporting(l))
        .filter(|(_, l)| l.contains(" hook ") || l.contains("clave hook"))
        .filter(|(_, l)| !l.contains("--hook"))
        .collect();
    assert!(
        offenders.is_empty(),
        "a hook fired outside `ct.sh --hook` — this is the 2026-09-11 incident's \
         exact shape: the store write lands in the sandbox and the PUSH goes to \
         whatever `ZELLIJ_SESSION_NAME` names.\n{offenders:#?}"
    );
}

#[test]
fn the_drive_scrubs_the_ambient_identity_before_it_drives_anything() {
    let scrub = DRIVE
        .find("unset ZELLIJ ZELLIJ_PANE_ID")
        .expect("the drive must scrub the inherited zellij identity");
    let export = DRIVE
        .find(r#"export ZELLIJ_SESSION_NAME="$SESSION""#)
        .expect("the drive must re-point the identity at its own sandbox");
    // The ordering IS the property: every phase below assumes the scrub has
    // already happened, and the tripwire beside it only guards a reordering
    // it can see. A phase above the scrub would inherit the fleet.
    let first_phase = DRIVE
        .find("\nphase \"")
        .expect("the drive must run at least one phase");
    assert!(
        scrub < first_phase && export < first_phase,
        "the identity scrub must come before the first phase (scrub at byte \
         {scrub}, export at {export}, first phase at {first_phase})"
    );
}

#[test]
fn ct_sh_keeps_its_own_per_call_scrub() {
    // Belt as well as braces. `ct.sh` is run directly too — by hand, and by
    // the agent protocol in QA-DRIVE.md — so it cannot lean on the drive
    // having scrubbed for it.
    assert!(
        CT.contains("unset ZELLIJ ZELLIJ_PANE_ID"),
        "ct.sh must clear the inherited zellij context itself"
    );
    assert!(
        CT.contains(r#"export ZELLIJ_SESSION_NAME="$SESSION""#),
        "ct.sh must name its target session explicitly"
    );
}

#[test]
fn the_drive_witnesses_its_own_isolation() {
    // The witness phase is what turns "we did not touch his session" from a
    // claim into a reading. If it is deleted, this test says so.
    assert!(
        DRIVE.contains("push-refused"),
        "the drive must count clave's refusals to misaim a push — that count \
         is the only evidence of isolation available without touching the \
         session we are proving we did not touch"
    );
    assert!(
        DRIVE.contains("P6b-isolation-witness"),
        "the isolation witness phase must exist"
    );
}
