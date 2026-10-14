//! The QA instrument's own tests, run in the ordinary gate.
//!
//! `scripts/qa/lib.sh` turns zellij's log into the numbers every drive phase
//! asserts on — which instance learned what, how many bars this sandbox has,
//! what happened since a mark. That is logic, and logic belongs in a test
//! rather than in a session the maintainer has to launch by hand: before this,
//! a wrong `sed` expression could only be found by asking him for a sandbox.
//!
//! The assertions live in bash next to the code they cover
//! (`scripts/qa/lib-selftest.sh`) because a Rust reimplementation of the
//! parsing would be a second thing to keep right. This test is the gate hook:
//! it runs that script and fails with its output.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root is two levels above this crate")
}

#[test]
fn the_qa_instrument_passes_its_own_selftest() {
    let root = repo_root();
    let script = root.join("scripts/qa/lib-selftest.sh");
    let out = Command::new("bash")
        .arg(&script)
        .current_dir(&root)
        .output()
        .expect("bash must be available to run the QA selftest");
    assert!(
        out.status.success(),
        "scripts/qa/lib-selftest.sh failed ({}):\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

// The slim push checks (phases 5, 6a and 6c) count bar log lines by a
// fragment held in qa/lib.sh. A fragment that drifts from the bar's text
// counts nothing, and most of those checks pass on nothing: "no bar asked"
// is true of a line that no longer exists. So each fragment is held to the
// text the bar prints, here in the gate.
const LIB: &str = include_str!("../../../scripts/qa/lib.sh");
const BAR_SHELL: &str = include_str!("../../clave-bar/src/main.rs");

/// The value of a `NAME='…'` assignment in qa/lib.sh.
fn lib_fragment(name: &str) -> &'static str {
    let head = format!("{name}='");
    let line = LIB
        .lines()
        .find(|l| l.starts_with(&head))
        .unwrap_or_else(|| panic!("qa/lib.sh must assign {name}"));
    let value = &line[head.len()..];
    &value[..value.find('\'').expect("the fragment must close its quote")]
}

#[test]
fn the_slim_push_fragments_match_what_the_bar_prints() {
    // The bar's own format strings, pinned first, then rendered the way the
    // bar renders them (main.rs `pipe`, the CLI blank twin; the status arm).
    // Each ends its fixed words with the build tag: the drive's `build_`
    // readers keep a line only when the tag is the sandbox's, because a bar
    // in another session can carry the same `[id: N]` (ids restart per
    // zellij server).
    for text in [
        r#""clave-bar: dropped {} pipe with empty payload build={BUILD}","#,
        r#"eprintln!("clave-bar: bad {name} payload build={BUILD}: {e}");"#,
        r#"eprintln!("clave-bar: hydrate ask build={BUILD}");"#,
        r#"eprintln!("clave-bar: hydrate answer seq={} build={BUILD}", snap.seq);"#,
        r#""clave-bar: run_command failed build={BUILD}: {}","#,
    ] {
        assert!(
            BAR_SHELL.contains(text),
            "the bar no longer prints {text}; move qa/lib.sh with it"
        );
    }
    let t = "cafe123";
    let twin = |pipe: &str| format!("clave-bar: dropped {pipe} pipe with empty payload build={t}");
    let slim = twin(clave_types::SLIM_STATUS_PIPE);
    let whole = twin(clave_types::STATUS_PIPE);
    assert!(slim.contains(lib_fragment("SLIM_TWIN")));
    assert!(whole.contains(lib_fragment("WHOLE_TWIN")));
    // The whole name is a prefix of the slim one, so this is the half that
    // can go wrong: a whole-twin fragment that also counts every slim push.
    assert!(!slim.contains(lib_fragment("WHOLE_TWIN")));
    for pipe in [clave_types::STATUS_PIPE, clave_types::SLIM_STATUS_PIPE] {
        let bad = format!("clave-bar: bad {pipe} payload build={t}: expected value");
        assert!(bad.contains(lib_fragment("BAD_STATUS")), "{bad}");
    }
    assert!(format!("clave-bar: hydrate ask build={t}").contains(lib_fragment("HYDRATE_ASK")));
    assert!(
        format!("clave-bar: hydrate answer seq=41 build={t}")
            .contains(lib_fragment("HYDRATE_ANSWER"))
    );
    assert!(
        format!("clave-bar: run_command failed build={t}: boom")
            .contains(lib_fragment("RUN_FAILED"))
    );
}

#[test]
fn the_refusal_push_is_a_slim_push_the_bar_parses() {
    // Phase 6a pipes this to every bar. If the bar cannot parse it, each bar
    // logs a bad payload and asks nothing, and the leg fails for the wrong
    // reason. If it parsed as whole, no bar would refuse it.
    let line = LIB
        .lines()
        .find(|l| l.contains(r#"printf '{"seq":%s"#))
        .expect("qa/lib.sh `refusal_payload` must print the push");
    let start = line.find('\'').expect("the printf format opens a quote") + 1;
    let format = &line[start..start + line[start..].find('\'').expect("and closes it")];
    let payload = format.replace("%s", "8");
    let snap: clave_types::AgentSnapshot =
        serde_json::from_str(&payload).expect("the bar's status arm must parse the refusal push");
    assert_eq!(snap.seq, 8);
    assert!(snap.agents.is_empty());
    assert_eq!(snap.dormant, clave_types::DormantRows::Omitted { tag: 0 });
}
