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
const SAMPLER: &str = include_str!("../../../scripts/qa/fd-sampler.sh");

/// Every script the drive is made of. The hook rule is about the WHOLE drive,
/// not about one file of it: the instrument was split out of qa-drive.sh on
/// 2026-09-12, and a rule that only read the file it was written against
/// would have stopped covering anything that moved.
const DRIVE_SOURCES: [(&str, &str); 4] = [
    ("scripts/qa-drive.sh", DRIVE),
    ("scripts/qa/lib.sh", LIB),
    ("scripts/qa/lib-selftest.sh", SELFTEST),
    // A tool #261 added. It was outside every rule here until review pointed
    // it out, which is the failure mode the paragraph above describes
    // happening a second time: the list is a list, so growing the drive does
    // not grow the guard. Anything new under scripts/qa/ belongs here.
    ("scripts/qa/fd-sampler.sh", SAMPLER),
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
        // The comment and blank filter runs BEFORE the heredoc test, and
        // that order is the guard: prose here discusses heredocs, and a `#`
        // line naming one would otherwise open a phantom body that swallows
        // every real line until its tag appeared — a guard that reads less
        // than it claims to, silently (CodeRabbit, 2026-09-13).
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        open = heredoc_tag(raw);
        out.push((i + 1, line));
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
/// very hazard.
///
/// A reporting line runs nothing ONLY while it stays literal. `check "x" "$(…)"`
/// is the drive's most common shape (127 lines of it), and the substitution runs
/// before `check` is ever called — so `measure "fired" "$("$CLAVE_BIN" hook Stop)"`
/// would narrate and fire, which is the 2026-09-11 incident exactly. A reporter
/// that substitutes is therefore not exempt, and is read like any other command.
fn is_reporting(line: &str) -> bool {
    const REPORTERS: [&str; 9] = [
        "measure ", "note ", "check ", "check_", "printf ", "echo ", "cat ", "usage ", "phase ",
    ];
    REPORTERS.iter().any(|r| line.starts_with(r))
        && !line.contains("$(")
        && !backtick_outside_single_quotes(line)
}

/// Whether a backtick on this line could open a legacy command substitution.
/// The drive writes six report lines with backticks in their prose, all inside
/// single quotes, where bash expands nothing — so the character alone cannot
/// disqualify a reporter without forcing prose churn. Outside single quotes it
/// is a second way to run a command, and gets the same treatment as `$(`.
fn backtick_outside_single_quotes(line: &str) -> bool {
    let mut in_single = false;
    for c in line.chars() {
        match c {
            '\'' => in_single = !in_single,
            '`' if !in_single => return true,
            _ => {}
        }
    }
    false
}

/// Whether this line types into a pane. `write-chars` sends text and `write`
/// sends one key code; both land on whatever pane holds focus, so both are
/// keystrokes and neither is safer than the other.
///
/// Read as tokens, not as source text. The earlier form of this check looked
/// for the literal `"$CT" write `, so `$CT write 13` and `"${CT}" write 13` —
/// the same command, quoted differently — matched nothing and could sit in any
/// phase with the test still green.
fn is_keystroke(line: &str) -> bool {
    let flat = line.replace(['"', '\''], "").replace("${CT}", "$CT");
    let tokens: Vec<&str> = flat.split_whitespace().collect();
    tokens.windows(2).any(|w| {
        (w[0].contains("$CT") || w[0].ends_with("ct.sh")) && matches!(w[1], "write" | "write-chars")
    })
}

/// The reader every guard below stands on must not be fooled by prose.
///
/// These scripts DISCUSS heredocs, and a `#` line naming one used to open a
/// phantom body: every following line was read as a heredoc body — that is,
/// as harmless — until the tag turned up, which it never does in prose. A
/// guard that silently stops reading is worse than no guard, because the
/// build stays green. Two of these tests already failed open once this
/// branch; this is the same class, in the layer under them.
#[test]
fn a_comment_about_a_heredoc_does_not_blind_the_reader() {
    let src = "\
# the usage text is a heredoc <<USAGE
\"$CT\" write-chars 'y'
cat <<USAGE
  harmless body naming clave hook Stop
USAGE
echo done
";
    let read: Vec<&str> = code_lines(src).into_iter().map(|(_, l)| l).collect();
    assert!(
        read.contains(&"\"$CT\" write-chars 'y'"),
        "a comment naming a heredoc swallowed the live line after it: {read:?}"
    );
    assert!(
        !read.iter().any(|l| l.contains("harmless body")),
        "a real heredoc body must still be skipped: {read:?}"
    );
    assert!(
        read.contains(&"echo done"),
        "the reader must resume after the real heredoc closes: {read:?}"
    );
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
    let keystrokes: Vec<_> = code.iter().filter(|(_, l)| is_keystroke(l)).collect();
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

/// Whether this line runs a session lifecycle command — one that starts or
/// ends a zellij session. Read as tokens, like `is_keystroke`, so a different
/// quoting of the same command cannot slip past.
fn is_session_lifecycle(line: &str) -> bool {
    // The first four are measured from the vendored source,
    // zellij-utils-0.44.3/src/cli.rs:361-397, and each carries a
    // visible_alias: k, d, ka, da. A denylist of the long names alone let
    // `zellij ka` — kill EVERY session on the machine, including the
    // maintainer's working fleet — through a green gate, and
    // `delete-all-sessions` was missing under both names (swarm review,
    // 2026-09-16).
    //
    // `new-session` and `a` are DELIBERATELY WIDER than that range, and were
    // mis-described as part of it until review (2026-09-17). `a` is `attach`
    // (cli.rs:306), which neither starts nor ends a session but does put a
    // drive inside one it does not own. `new-session` matches no zellij
    // subcommand at all; it is kept because the denylist costs nothing when it
    // matches nothing, and a spelling that appears later should trip. The
    // session-CREATING spellings this cannot see — bare `zellij`, `zellij
    // --session <name>`, `zellij attach -c` — are caught from the other end by
    // the `dev launch`/`just launch` window below, which is the path a drive
    // would realistically take.
    const LIFECYCLE: [&str; 5] = [
        "kill-session",
        "delete-session",
        "kill-all-sessions",
        "delete-all-sessions",
        "new-session",
    ];
    // The one-letter aliases are matched ONLY in the position after a zellij
    // invocation. As a bare denylist they would trip on ordinary shell — `d`,
    // `a` and `k` are common words and variable names.
    const LIFECYCLE_ALIAS: [&str; 5] = ["k", "d", "ka", "da", "a"];
    let flat = line.replace(['"', '\''], "");
    let tokens: Vec<&str> = flat.split_whitespace().collect();
    // A launch is the same class from the other end: `dev launch` and `just
    // launch` both create the session, and the refusal that keeps them the
    // maintainer's lives in the binary, not here.
    let launches = tokens
        .windows(2)
        .any(|w| matches!((w[0], w[1]), ("dev", "launch") | ("just", "launch")));
    let aliased = tokens.windows(2).any(|w| {
        w[0].rsplit('/').next().is_some_and(|c| c == "zellij") && LIFECYCLE_ALIAS.contains(&w[1])
    });
    launches || aliased || tokens.iter().any(|t| LIFECYCLE.contains(t))
}

#[test]
fn the_drive_never_starts_or_ends_a_session_itself() {
    // Phase 6c is the first phase that NEEDS a session boundary crossed, so it
    // is the first place where killing the sandbox from inside the drive looks
    // reasonable. It is not: session lifecycle is the maintainer's (AGENTS.md),
    // and a drive that can kill by name is one variable substitution away from
    // killing the wrong one. The phase prints the pair and waits instead.
    //
    // The detector is checked against a line that must trip it, because a
    // guard that matches nothing passes on every source including a broken
    // one — the failure mode this whole file exists to close.
    assert!(
        is_session_lifecycle(r#"zellij kill-session "$SESSION""#),
        "the detector must catch a kill"
    );
    assert!(
        is_session_lifecycle("just launch"),
        "the detector must catch a launch"
    );
    // Tokens, not substrings: a new TAB is the drive's ordinary business, and
    // a variable named for the session is not a command. Narration is not
    // tested here — `is_reporting` filters it below, as it does for hooks.
    assert!(
        !is_session_lifecycle(r#""$CT" new-tab"#),
        "a new tab is not a new session"
    );
    assert!(
        !is_session_lifecycle(r#"P6C_SESSION_GONE="no""#),
        "a variable named for the session is not a lifecycle command"
    );
    // The SHORT spellings. zellij gives every lifecycle subcommand a visible
    // alias, so a denylist of long names has no teeth: `zellij ka` kills every
    // session on the machine. Measured against
    // zellij-utils-0.44.3/src/cli.rs:361-397 (swarm review, 2026-09-16).
    for short in ["zellij ka", "zellij k mysession", "zellij da", "zellij d x"] {
        assert!(
            is_session_lifecycle(short),
            "the detector must catch the short spelling: {short}"
        );
    }
    assert!(
        is_session_lifecycle("zellij delete-all-sessions"),
        "delete-all-sessions is a lifecycle command under its long name too"
    );
    // …and only after a zellij invocation. These letters are ordinary shell.
    assert!(
        !is_session_lifecycle("ls -d /tmp"),
        "a bare one-letter token is not a zellij subcommand"
    );
    assert!(
        !is_session_lifecycle(r#"jq -r .a <<<"$STATUS""#),
        "an argument that happens to read `a` is not a lifecycle command"
    );

    let offenders: Vec<_> = DRIVE_SOURCES
        .iter()
        .flat_map(|(name, src)| {
            code_lines(src)
                .into_iter()
                .map(move |(n, l)| (format!("{name}:{n}"), l))
        })
        .filter(|(_, l)| !is_reporting(l))
        .filter(|(_, l)| is_session_lifecycle(l))
        .collect();
    assert!(
        offenders.is_empty(),
        "the drive started or ended a session itself. It prints the command \
         and waits for the human — that is what makes phase 6c's two launches \
         safe.\n{offenders:#?}"
    );
}

#[test]
fn the_relaunch_phase_reads_the_set_through_the_tested_readers() {
    // Phase 6c is the only phase that reads the store on both sides of a
    // session boundary, so it is the only one that can see what a relaunch
    // does with the store the quit left. It proves one eager tab and every
    // other row dormant (setup.rs `launch_layout_kdl`, decision of
    // 2026-09-22). The live-set restore it replaced (#261) reached a shipped
    // branch with every gate green because no test launches twice.
    assert!(
        DRIVE.contains("P6c-relaunch"),
        "the relaunch phase must exist — without it nothing but a human \
         launching twice can see what a relaunch bakes"
    );
    // The verdict itself lives in qa/lib.sh and is exercised offline, against
    // a store with two bound rows, the wrong row bound, and a stale status.
    // The phase costs two maintainer launches, so a comparison only that pair
    // of launches can try is one nobody tries — and the defect this phase
    // exists for is exactly a leg that no test ran.
    assert!(
        DRIVE.contains("relaunch_checks"),
        "phase 6c must reach its verdict through `relaunch_checks`, not a \
         comparison written inline where nothing can run it"
    );
    for reader in [
        "bound_uuids",
        "eager_candidate_uuid",
        "stale_status_uuids",
        "relaunch_checks",
    ] {
        assert!(
            LIB.contains(&format!("{reader}()")),
            "`{reader}` must live in qa/lib.sh, where the selftest reaches it"
        );
        assert!(
            SELFTEST.contains(reader),
            "`{reader}` must be covered by the offline selftest"
        );
    }
    let witness = DRIVE
        .find(r#"phase "P6b-isolation-witness""#)
        .expect("the isolation witness must exist");
    let relaunch = DRIVE
        .find(r#"phase "P6c-relaunch""#)
        .expect("the relaunch phase must exist");
    let teardown = DRIVE
        .find(r#"phase "P7-teardown""#)
        .expect("the teardown must exist");
    // Order is the property: the relaunch ends the session every earlier phase
    // reads from, and the teardown hands back the session the human is looking
    // at — which after 6c is the SECOND one.
    assert!(
        witness < relaunch && relaunch < teardown,
        "the relaunch must sit between the isolation witness and the teardown \
         (witness {witness}, relaunch {relaunch}, teardown {teardown})"
    );
}
