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
