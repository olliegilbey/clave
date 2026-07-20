//! Zellij version-pin tripwire (issue #10 item 4).
//!
//! clave's correctness leans on OBSERVED zellij 0.44.3 semantics, not just
//! its API: the resize floor/refusal behavior the width-seek machine expects
//! (SUBSYSTEM-VALIDATION.md C6 rounds 9/17/20/21b), the fixed-pane silent
//! resize refusal that forced percent panes (C8 2026-07-18), the KDL
//! trailing-`;` terminator quirk (Task 9 C1, setup.rs config_kdl), the
//! permission-cache file format merge_permissions_kdl splices into, and
//! MessagePlugin/pipe delivery order. NONE of that is compiler-checked: a
//! `zellij-tile` bump compiles clean and ships silently-changed semantics.
//!
//! Worse, cargo makes the drift invisible: the kdl_guardrail dev-dep pins
//! `zellij-utils = "=0.44.3"`, but if zellij-tile bumped to 0.45 cargo would
//! happily hold BOTH zellij-utils versions — the plugin would run 0.45
//! semantics while the guardrail kept green-lighting templates against the
//! 0.44.3 parser. This test is the loud failure for exactly that split.
//!
//! When it fires: re-audit the call sites above against the new zellij
//! source (vendored path recipe in docs/dev/TESTING.md), re-run the live
//! validation SOP for the affected subsystems, THEN bump PINNED_ZELLIJ /
//! PINNED_KDL and the kdl_guardrail dev-dep pins together in one commit.

/// The audited zellij line. Every zellij-family crate in Cargo.lock must
/// resolve to exactly this version — one version, everywhere.
const PINNED_ZELLIJ: &str = "0.44.3";

/// The kdl line zellij-utils 0.44.3 itself parses with (its own Cargo.toml
/// pins 4.7.1). The kdl_guardrail's permission-cache check leans on this
/// exact match for its fidelity claim (dev-dep `kdl = "=4.7.1"`), and a
/// decoupled kdl bump — cargo holding a second kdl version for the dev-dep's
/// consumers — would silently validate permissions.kdl against a different
/// grammar without any zellij crate moving (fugu review 2026-07-20).
const PINNED_KDL: &str = "4.7.1";

/// Minimal Cargo.lock scan: collect the `version` of every package whose
/// `name` starts with `zellij`, plus every `kdl` entry (lock format: a
/// `[[package]]` block with `name = "..."` immediately followed by
/// `version = "..."`). No toml dep — the format is stable and the parse
/// failing loudly is itself acceptable tripwire behavior.
fn pinned_lock_versions() -> Vec<(String, String)> {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
        .expect("workspace Cargo.lock must be readable from crates/clave");
    let mut out = Vec::new();
    let mut lines = lock.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(name) = line.strip_prefix("name = \"") {
            let name = name.trim_end_matches('"');
            if !name.starts_with("zellij") && name != "kdl" {
                continue;
            }
            let ver = lines
                .peek()
                .and_then(|l| l.strip_prefix("version = \""))
                .map(|v| v.trim_end_matches('"').to_string())
                .unwrap_or_else(|| panic!("Cargo.lock: no version line after {name}"));
            out.push((name.to_string(), ver));
        }
    }
    out
}

#[test]
fn every_zellij_crate_resolves_to_the_audited_version() {
    let versions = pinned_lock_versions();
    // zellij-tile (the plugin API), zellij-utils (the parser the
    // kdl_guardrail validates against) and kdl (the grammar the permission
    // cache is checked with) must all be present…
    for required in ["zellij-tile", "zellij-utils", "kdl"] {
        assert!(
            versions.iter().any(|(n, _)| n == required),
            "{required} missing from Cargo.lock — the workspace no longer \
             depends on it; this tripwire (and the kdl_guardrail) need a \
             rethink, not deletion:\n{versions:?}"
        );
    }
    // …and every entry must sit on its single audited line. A second version
    // appearing (cargo's silent dual-version resolution) is precisely the
    // guardrail-vs-plugin split described in the header — for the zellij
    // family AND for kdl (one lock entry each, at the audited version).
    for (name, ver) in &versions {
        let pinned = if name == "kdl" {
            PINNED_KDL
        } else {
            PINNED_ZELLIJ
        };
        assert_eq!(
            ver, pinned,
            "{name} resolved to {ver}, but the audited line is {pinned}. \
             Re-audit the semantics-sensitive call sites (this file's \
             header) before bumping the audited consts and the \
             kdl_guardrail dev-dep pins together.\nAll audited entries: \
             {versions:?}"
        );
    }
    let kdl_entries = versions.iter().filter(|(n, _)| n == "kdl").count();
    assert_eq!(
        kdl_entries, 1,
        "expected exactly one kdl version in Cargo.lock, found {kdl_entries} \
         — a second kdl means the permission-cache guardrail no longer \
         parses with the grammar zellij uses:\n{versions:?}"
    );
}
