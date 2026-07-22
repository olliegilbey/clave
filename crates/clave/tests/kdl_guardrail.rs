//! KDL real-parser guardrail (issue #10 item 1).
//!
//! clave emits KDL as plain `format!` strings — config.kdl, layout.kdl,
//! launch.kdl, the one-shot add/open tab layout, and the permission cache.
//! The existing unit tests assert those strings CONTAIN the right substrings,
//! but a substring check can't catch a structural regression: a dropped brace,
//! the load-bearing trailing-`;` after a MessagePlugin node (setup.rs config
//! comment / Task 9 C1), or a mis-nested `children` all pass `.contains` and
//! then fail at SESSION LAUNCH — the worst place to discover it (a dead
//! `attach` blocks forever; the failure is invisible until a human tries to
//! start clave).
//!
//! This suite runs every layout-shaped artifact through the EXACT parser
//! zellij 0.44.3 runs — `Layout::from_str` / `Config::from_kdl` from
//! zellij-utils 0.44.3, the same version resolved in Cargo.lock (transitive
//! via zellij-tile) — and the permission cache through the `kdl` crate pinned
//! to the same 4.7.1 line zellij parses it with. Substring tests assert
//! CONTENT; these assert VALIDITY. Both matter; neither replaces the other.
//!
//! The `=` version pins in Cargo.toml are deliberate: this test is the seed of
//! the issue #10 item 5 version tripwire. When a future zellij bump changes
//! KDL semantics, the pins force a conscious re-vet here instead of a silent
//! drift that would let a newly-invalid template through.

use clave::add;
use clave::setup;
use clave::store::{AgentRecord, LabelSource};
use clave_types::Status;

use zellij_utils::input::config::Config;
use zellij_utils::input::layout::Layout;

/// Assert a layout-shaped artifact parses through zellij's REAL layout parser.
/// The full KDL text and the parser error ride in the panic message — a
/// structural regression must point straight at the offending template, the
/// same way the existing `assert!(…, "…\n{kdl}")` unit tests do.
fn assert_layout_ok(kdl: &str, what: &str) {
    // The `path_to_raw_layout` label is only used inside error messages; `None`
    // for swap layouts and cwd matches how clave hands these files to zellij
    // (`--layout <file>` with no swap layout, cwd resolved by zellij at launch).
    if let Err(e) = Layout::from_str(kdl, format!("guardrail:{what}"), None, None) {
        panic!("{what} is not valid zellij layout KDL: {e:?}\n---\n{kdl}");
    }
}

/// Assert a config artifact parses through zellij's REAL config parser (the
/// `keybinds`/`session_serialization` document `clave setup` writes as
/// config.kdl and hands to `zellij --config`).
fn assert_config_ok(kdl: &str, what: &str) {
    if let Err(e) = Config::from_kdl(kdl, None) {
        panic!("{what} is not valid zellij config KDL: {e:?}\n---\n{kdl}");
    }
}

/// A realistic agent row for the launch-layout eager branch and any generator
/// that bakes a record. Fields mirror the shapes the live weave produces: a
/// worktree cwd, a `dir · branch · words` earned label — values that must
/// survive `sanitize_label`/`validate_cwd` and still parse.
fn eager_record() -> AgentRecord {
    AgentRecord {
        uuid: "3f2a9c1b-uuid".into(),
        cwd: "/home/o/code/clave/.claude-worktrees/ab12cd34".into(),
        repo_root: "/home/o/code/clave".into(),
        branch: "clave/ab12cd34".into(),
        label: "clave · main · fix the KDL guardrail".into(),
        status: Status::Idle,
        last_interacted: 100,
        last_visited: 0,
        worktree: Some("/home/o/code/clave/.claude-worktrees/ab12cd34".into()),
        label_source: LabelSource::Summary,
        tab_id: None,
        stale: false,
    }
}

// A versioned absolute binary path (the §2 stable-session form) and a wasm
// location with the shapes both real installs produce — the dev/sandbox bare
// `clave` form is exercised by the empty-store cases below.
const WASM: &str = "/home/o/.local/share/clave/clave-bar-v0.1.0.wasm";
const BIN_ABS: &str = "/home/o/.local/share/clave/bin/clave-v0.1.0";

#[test]
fn config_kdl_parses_through_real_zellij_parser() {
    // Both binary forms: bare `clave` (dev/sandbox PATH) and the versioned
    // absolute path (stable) — the generator is pure over the binary, so a
    // regression could hide in either interpolation.
    assert_config_ok(&setup::config_kdl("clave", WASM), "config.kdl (dev binary)");
    assert_config_ok(
        &setup::config_kdl(BIN_ABS, WASM),
        "config.kdl (stable versioned binary)",
    );
}

#[test]
fn config_kdl_unbinds_claude_code_keys_in_every_mode() {
    // #28 semantic guardrail: a substring check proves the `unbind` node is
    // EMITTED, but only merging it over zellij's real stock defaults — the
    // exact thing `zellij --config` does — proves the five keys are actually
    // GONE from every mode. Verified against zellij-utils 0.44.3 source:
    //   * Config::from_kdl(cfg, Some(base)) seeds `base` then merges our doc
    //     over it (kdl/mod.rs:4856-4865); clear-defaults=false keeps the base.
    //   * A top-level `unbind` node is picked up by children().get("unbind")
    //     and fed to unbind_keys_in_all_modes, which iterates Keybinds.0's
    //     modes and .remove()s each key (kdl/mod.rs:4600, 4627) — AFTER binds
    //     are merged, so it strips the stock binds.
    use zellij_utils::data::{BareKey, InputMode, KeyWithModifier};

    let base = Config::from_default_assets().expect("stock zellij 0.44.3 defaults must parse");
    let merged = Config::from_kdl(&setup::config_kdl("clave", WASM), Some(base))
        .expect("clave config.kdl must merge over stock defaults");

    // Ctrl-modified char keys, spelled as the stock binds are (default.kdl:
    // `Ctrl g/t/o/b/q`). Keybinds.0 is the public HashMap<InputMode, …> that
    // unbind_keys_in_all_modes drains — iterating it is the faithful "every
    // mode that survived the merge" check, no strum EnumIter needed.
    let unbound = [
        KeyWithModifier::new(BareKey::Char('g')).with_ctrl_modifier(),
        KeyWithModifier::new(BareKey::Char('t')).with_ctrl_modifier(),
        KeyWithModifier::new(BareKey::Char('o')).with_ctrl_modifier(),
        KeyWithModifier::new(BareKey::Char('b')).with_ctrl_modifier(),
        KeyWithModifier::new(BareKey::Char('q')).with_ctrl_modifier(),
    ];
    for (mode, binds) in &merged.keybinds.0 {
        for key in &unbound {
            assert!(
                binds.get(key).is_none(),
                "{key:?} is still bound in {mode:?} — a Claude Code key is swallowed (#28)"
            );
        }
    }

    // Non-vacuity: the fix must be SURGICAL, not a blanket clear (the #28
    // ruling keeps clear-defaults=false so stock pane/resize/scroll/move
    // remain). A stock bind we never touched must survive (Ctrl+p → Pane in
    // Normal, default.kdl:206), and clave's own shared_among Alt+a add-bind
    // must survive the merge too.
    let ctrl_p = KeyWithModifier::new(BareKey::Char('p')).with_ctrl_modifier();
    assert!(
        merged
            .keybinds
            .get_actions_for_key_in_mode(&InputMode::Normal, &ctrl_p)
            .is_some(),
        "stock Ctrl+p (pane mode) must survive — the unbind must not clear defaults"
    );
    let alt_a = KeyWithModifier::new(BareKey::Char('a')).with_alt_modifier();
    assert!(
        merged
            .keybinds
            .get_actions_for_key_in_mode(&InputMode::Normal, &alt_a)
            .is_some(),
        "clave's Alt+a add-bind must survive the merge (invariant #6)"
    );
}

#[test]
fn layout_kdl_parses_through_real_zellij_parser() {
    assert_layout_ok(&setup::layout_kdl(WASM), "layout.kdl");
}

#[test]
fn launch_layout_kdl_parses_in_both_branches() {
    // Empty store → bar-only (template + one plain `clave` tab).
    assert_layout_ok(
        &setup::launch_layout_kdl("clave", WASM, None),
        "launch.kdl (empty store, bar-only)",
    );
    // Non-empty store → the eager most-recent branch, which composes in
    // `add::tab_node_bare` — a distinct code path (bare tab node, no bar pane)
    // that the empty branch never touches.
    let r = eager_record();
    assert_layout_ok(
        &setup::launch_layout_kdl(BIN_ABS, WASM, Some(&r)),
        "launch.kdl (eager most-recent tab)",
    );
}

#[test]
fn add_tab_layout_parses_through_real_zellij_parser() {
    // The one-shot temp layout fed to `zellij action new-tab --layout` (add.rs
    // and open.rs run_open). Exercise with label/cwd values that first pass
    // through the real guards the live weave applies, so this proves the
    // POST-sanitization artifact parses, not a hand-picked clean string.
    let label = add::sanitize_label("fix \"auth\"\nflow · main");
    let cwd = "/home/o/code/clave/.claude-worktrees/ab12cd34";
    add::validate_cwd(cwd).expect("test cwd must pass validate_cwd");
    let kdl = add::tab_layout(BIN_ABS, WASM, &label, "u-1", cwd);
    assert_layout_ok(&kdl, "add/open one-shot tab layout");
}

#[test]
fn permissions_kdl_is_well_formed_in_both_branches() {
    // No public string entry point exists for zellij's PermissionCache parse,
    // so validate WELL-FORMEDNESS with the `kdl` crate pinned to the exact
    // 4.7.1 line zellij deserializes the cache with (Cargo.toml comment): same
    // grammar, same acceptance/rejection, so a malformed grant fails here.
    let seeded = setup::merge_permissions_kdl("", WASM);
    seeded.parse::<kdl::KdlDocument>().unwrap_or_else(|e| {
        panic!("seeded-empty permissions.kdl is malformed: {e}\n---\n{seeded}")
    });

    // Merge-with-existing: another plugin's node must survive and the whole
    // document stay well-formed (the merge is line surgery, not structured
    // emit — exactly where a brace-balance bug would hide).
    let existing = "\"file:/other/plugin.wasm\" {\n    ReadCliPipes\n}\n";
    let merged = setup::merge_permissions_kdl(existing, WASM);
    merged
        .parse::<kdl::KdlDocument>()
        .unwrap_or_else(|e| panic!("merged permissions.kdl is malformed: {e}\n---\n{merged}"));

    // Non-vacuity guard (fugu review 2026-07-20): the layout/config paths
    // prove their parsers reject broken input, but nothing proved THIS parse
    // entry point can fail — a misused call would leave the two positive
    // checks above silently vacuous. Truncate the merged doc's final closing
    // brace: exactly the brace-balance break the line-surgery merge (keyed on
    // `ends_with('}')`, setup.rs) could produce.
    let truncated = merged.trim_end().strip_suffix('}').map(str::to_string);
    let truncated = truncated.expect("merged permissions.kdl must end with a node-closing `}`");
    assert!(
        truncated.parse::<kdl::KdlDocument>().is_err(),
        "kdl parser accepted a brace-truncated permissions.kdl — the \
         well-formedness check is vacuous:\n{truncated}"
    );
}

#[test]
fn guardrail_rejects_broken_layout_kdl() {
    // Non-vacuity proof: without this, a future parser-API misuse that accepts
    // everything (wrong entry point, swallowed error) would let the whole
    // suite pass while validating nothing. Two deliberately-broken artifacts
    // the real parser MUST reject:

    // 1. Unclosed brace — the canonical structural break a dropped `}` in a
    //    template would produce.
    let unclosed = "layout {\n    pane {\n";
    assert!(
        Layout::from_str(unclosed, "guardrail:unclosed".into(), None, None).is_err(),
        "parser accepted an unclosed-brace layout — the guardrail is vacuous:\n{unclosed}"
    );

    // 2. The load-bearing trailing-`;` quirk (setup.rs config comment / Task 9
    //    C1) VIOLATED — by mutating the REAL config_kdl output, not a
    //    hand-written document. A synthetic `keybinds { bind … }` snippet gets
    //    rejected for an unrelated reason (`Invalid mode: 'bind'` — binds must
    //    sit inside a mode node), which would leave the quirk claim untested.
    //    Differential pair: the unmutated artifact parses; the same artifact
    //    with ONE `}`-closed child node un-terminated (`; }` for `; };`) must
    //    be rejected — if it parses, the trailing `;` is no longer
    //    load-bearing, the setup.rs comment is stale, and we must know.
    //    The first `}; }` in the document is the Alt+a Run bind (fugu review
    //    2026-07-20): the quirk is about ANY `}`-closed child node inside a
    //    bind block, so un-terminating that one is as load-bearing as the
    //    MessagePlugin lines Task 9 C1 originally caught it on.
    let real = setup::config_kdl("clave", WASM);
    assert_config_ok(&real, "trailing-`;` differential twin (unmutated)");
    let broken = real.replacen("}; }", "} }", 1);
    assert_ne!(
        broken, real,
        "mutation did not apply — config_kdl no longer emits the `}}; }}` \
         quirk shape; update this test alongside the template"
    );
    assert!(
        Config::from_kdl(&broken, None).is_err(),
        "parser accepted a config with a `}}`-closed child node missing its \
         trailing `;` — the trailing-`;` quirk (setup.rs) may no longer be \
         load-bearing:\n{broken}"
    );
}

#[test]
fn backslash_label_is_guarded_through_real_parser() {
    // Fugu 2026-07-21 (pre-v0.1.0, HIGH): backslash is KDL's escape
    // introducer — `\d` is not a valid escape, so a raw backslash in a label
    // must FAIL zellij's parser. Tripwire premise first: if a future zellij
    // kdl accepts it, this assert flips and the guard can be reconsidered.
    let raw = add::tab_layout(BIN_ABS, WASM, r"fix the \d regex", "u-1", "/home/o/x");
    assert!(
        Layout::from_str(&raw, "guardrail:raw-backslash".into(), None, None).is_err(),
        "premise broken: zellij's KDL parser now ACCEPTS a raw backslash — re-vet the guard\n---\n{raw}"
    );
    // The guard: the same label THROUGH sanitize_label must parse clean.
    let label = add::sanitize_label(r"fix the \d regex");
    let kdl = add::tab_layout(BIN_ABS, WASM, &label, "u-1", "/home/o/x");
    assert_layout_ok(&kdl, "add/open tab layout (backslash-bearing label)");
    // And a backslash-bearing cwd must be REFUSED, not baked.
    assert!(add::validate_cwd(r"/home/o/we\ird").is_err());
}
