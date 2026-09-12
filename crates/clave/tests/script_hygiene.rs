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
const LIB: &str = include_str!("../../../scripts/qa/lib.sh");
const SELFTEST: &str = include_str!("../../../scripts/qa/lib-selftest.sh");

/// Every script the drive is made of. The hook rule is about the WHOLE drive,
/// not about one file of it: the instrument was split out of qa-drive.sh on
/// 2026-09-12, and a rule that only read the file it was written against
/// would have stopped covering anything that moved.
const DRIVE_SOURCES: [(&str, &str); 3] = [
    ("scripts/qa-drive.sh", DRIVE),
    ("scripts/qa/lib.sh", LIB),
    ("scripts/qa/lib-selftest.sh", SELFTEST),
];

/// Lines that actually run something — comments, blanks and heredoc bodies
/// carry no risk, and both scripts discuss the hazard at length in prose that
/// must not trip this. The usage text is a heredoc and names `clave hook`
/// twice, which is the point of it.
fn code_lines(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut open: Option<String> = None;
    for (i, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if let Some(tag) = &open {
            if line == tag {
                open = None;
            }
            continue;
        }
        open = heredoc_tag(raw);
        if !line.is_empty() && !line.starts_with('#') {
            out.push((i + 1, line));
        }
    }
    out
}

/// The tag a line opens a heredoc with, if it opens one. Quoted or not,
/// `<<-` or not; a bare word only, so a left shift is not mistaken for one.
fn heredoc_tag(line: &str) -> Option<String> {
    let tag = line
        .split("<<")
        .nth(1)?
        .trim_start_matches('-')
        .trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .to_string();
    let wordy = !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    wordy.then_some(tag)
}

/// Lines that only REPORT. The drive narrates itself constantly, and its
/// prose is full of the word `hook` — including whole paragraphs about this
/// very hazard. None of it runs anything.
fn is_reporting(line: &str) -> bool {
    const REPORTERS: [&str; 9] = [
        "measure ", "note ", "check ", "check_", "printf ", "echo ", "cat ", "usage ", "phase ",
    ];
    REPORTERS.iter().any(|r| line.starts_with(r))
}

#[test]
fn the_drive_never_fires_a_hook_except_through_ct_sh() {
    let offenders: Vec<_> = DRIVE_SOURCES
        .iter()
        .flat_map(|(name, src)| {
            code_lines(src)
                .into_iter()
                .map(move |(n, l)| (format!("{name}:{n}"), l))
        })
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

#[test]
fn a_keystroke_only_reaches_a_pane_the_drive_proved_is_a_shell() {
    // `write-chars` goes to whatever pane holds focus, and at a claude prompt
    // it SUBMITS A TURN. Phase 5c needs one — a real command in a real shell
    // is the only way to drive the OS-facts pipeline — so the allowlist that
    // guards it is checked here rather than trusted: a keystroke added
    // anywhere else in the drive fails this test.
    let code = code_lines(DRIVE);
    let line_of = |needle: &str| -> usize {
        code.iter()
            .find(|(_, l)| l.contains(needle))
            .map(|(n, _)| *n)
            .unwrap_or_else(|| panic!("the drive must contain `{needle}`"))
    };
    let keystrokes: Vec<_> = code
        .iter()
        .filter(|(_, l)| l.contains("write-chars") || l.contains("\"$CT\" write "))
        .collect();
    assert!(
        !keystrokes.is_empty(),
        "phase 5c types to drive the OS-facts pipeline; if that is gone, so is \
         the only evidence the pipeline delivers"
    );

    // Two properties, both about WHERE a keystroke can appear. Line numbers
    // rather than byte offsets: the prose above the guard names `write-chars`
    // (it explains the hazard), so a byte search finds a comment.
    //
    // 1. Every keystroke is inside phase 5c, the one phase that decides
    //    whether the focused pane is a shell.
    let phase_start = line_of(r#"phase "P5c-term-facts""#);
    let phase_end = line_of(r#"phase "P6-quiescence""#);
    let strays: Vec<_> = keystrokes
        .iter()
        .filter(|(n, _)| *n < phase_start || *n > phase_end)
        .collect();
    assert!(
        strays.is_empty(),
        "a keystroke outside phase 5c (lines {phase_start}-{phase_end}). At a \
         claude prompt `write-chars` SUBMITS A TURN, so a keystroke anywhere \
         else needs its own guard and this test updated deliberately.\n{strays:#?}"
    );

    // 2. Nothing types before the allowlist has answered. The typing itself
    //    lives in one helper, so the sites that matter are the CALLS: they
    //    must all sit after the `P5C_WRITABLE` gate. (Text order is the proxy
    //    for run order that bash gives us; the helper's own body is exempt
    //    because it only runs when called.)
    let allowlist = line_of(r#"zsh | bash | sh | fish | dash) P5C_WRITABLE="yes" ;;"#);
    let gate = line_of(r#"if [[ "$P5C_WRITABLE" != "yes" ]]; then"#);
    assert!(
        allowlist < gate && gate < phase_end,
        "the allowlist must be decided before the gate that reads it \
         (allowlist {allowlist}, gate {gate})"
    );
    let helper = line_of("p5c_leg() {");
    let helper_end = code
        .iter()
        .find(|(n, l)| *n > helper && *l == "}")
        .map(|(n, _)| *n)
        .expect("the keystroke helper must close");
    let early: Vec<_> = keystrokes
        .iter()
        .filter(|(n, _)| *n < helper || *n > helper_end)
        .filter(|(n, _)| *n < gate)
        .collect();
    assert!(
        early.is_empty(),
        "a keystroke outside the helper and before the shell gate at line \
         {gate}\n{early:#?}"
    );
    let calls: Vec<_> = code
        .iter()
        .filter(|(_, l)| l.starts_with("p5c_leg "))
        .collect();
    assert!(
        !calls.is_empty() && calls.iter().all(|(n, _)| *n > gate),
        "every typing leg must be called after the shell gate at line \
         {gate}\n{calls:#?}"
    );
    assert!(
        DRIVE.contains("P5c-term-facts"),
        "the terminal-facts witness phase must exist — it is the only \
         automated evidence that the OS-facts pipeline delivers at all"
    );
}
