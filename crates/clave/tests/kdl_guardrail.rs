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

use std::collections::BTreeMap;
use zellij_utils::input::actions::Action;
use zellij_utils::input::config::Config;
use zellij_utils::input::layout::{Layout, Run, SplitSize, TiledPaneLayout};

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

/// Every plugin configuration map in a layout artifact, as zellij's REAL
/// parser sees it. Walks tabs AND the template (the bar pane lives in
/// `default_tab_template` for launch.kdl, and in a concrete tab node for the
/// one-shot add/open layout), recursing through `children`.
///
/// This is half of the #44 identity pair: zellij keys a pipe's destination on
/// `(location, configuration)` exactly
/// (zellij-server/src/plugins/wasm_bridge.rs:1676-1686), and a MISS spawns a
/// new plugin rather than no-op'ing (ibid. :1861-1894) — a duplicate bar.
fn layout_plugin_configs(kdl: &str, what: &str) -> Vec<BTreeMap<String, String>> {
    let layout = Layout::from_str(kdl, format!("guardrail:{what}"), None, None)
        .unwrap_or_else(|e| panic!("{what} did not parse: {e:?}\n---\n{kdl}"));

    fn walk(node: &TiledPaneLayout, out: &mut Vec<BTreeMap<String, String>>) {
        // Run::get_run_plugin (zellij-utils input/layout.rs:404) unwraps both
        // the RunPlugin and Alias arms; we only ever emit `file:` URLs, so it
        // is the RunPlugin arm in practice.
        if let Some(rp) = node.run.as_ref().and_then(Run::get_run_plugin) {
            out.push(rp.configuration.inner().clone());
        }
        for child in &node.children {
            walk(child, out);
        }
    }

    let mut out = Vec::new();
    for (_name, tiled, _floating) in &layout.tabs {
        walk(tiled, &mut out);
    }
    if let Some((tiled, _floating)) = &layout.template {
        walk(tiled, &mut out);
    }
    // #181: the two swap geometries carry their own copy of the bar pane, and a
    // copy whose configuration differs by one character is a SECOND bar the
    // first time Alt+c switches to it. They are part of the identity set.
    for (by_constraint, _name) in &layout.swap_tiled_layouts {
        for tiled in by_constraint.values() {
            walk(tiled, &mut out);
        }
    }
    out
}

/// The bar pane's `Run` in each of a layout's swap geometries. A swap REUSES
/// the running bar rather than spawning a second one — but only because
/// `apply_tiled_panes_layout_to_existing_panes` matches an existing pane on
/// `invoked_with() == run` (zellij-server 0.44.3 `tab/layout_applier.rs:1218`,
/// called from `tab/mod.rs:1093-1141`). `Run` for a plugin is
/// `(location, configuration)` and does NOT include `size`, which is exactly
/// why the two geometries may differ in width and in nothing else: any drift in
/// the plugin block demotes the match to logical position and can land the bar
/// in the wrong slot.
fn swap_bar_runs(kdl: &str, what: &str) -> Vec<Run> {
    let layout = Layout::from_str(kdl, format!("guardrail:{what}"), None, None)
        .unwrap_or_else(|e| panic!("{what} did not parse: {e:?}\n---\n{kdl}"));
    layout
        .swap_tiled_layouts
        .iter()
        .flat_map(|(by_constraint, _name)| by_constraint.values())
        .filter_map(|tiled| {
            tiled
                .children
                .iter()
                .find_map(|c| c.run.as_ref().filter(|r| r.get_run_plugin().is_some()))
                .cloned()
        })
        .collect()
}

/// The bar pane's declared size in each of a layout's swap geometries, keyed by
/// the swap layout's name, IN DECLARATION ORDER. The order is load-bearing:
/// `next_swap_layout` is relative, and zellij hides the tab's own birth layout
/// ahead of the declared pair (so the live cycle is birth → declared[0] →
/// declared[1] → birth). The first call therefore lands on the first DECLARED
/// geometry, which must be the one the tab WAS born in — the birth position is
/// the one zellij reports as `"BASE"` rather than by name, and matching its
/// width makes the bar's first (naming) switch invisible (#197).
fn swap_bar_sizes(kdl: &str, what: &str) -> Vec<(String, SplitSize)> {
    let layout = Layout::from_str(kdl, format!("guardrail:{what}"), None, None)
        .unwrap_or_else(|e| panic!("{what} did not parse: {e:?}\n---\n{kdl}"));
    layout
        .swap_tiled_layouts
        .iter()
        .map(|(by_constraint, name)| {
            let tiled = by_constraint
                .values()
                .next()
                .unwrap_or_else(|| panic!("{what}: swap layout {name:?} declares no geometry"));
            let bar = tiled
                .children
                .iter()
                .find(|c| c.run.as_ref().and_then(Run::get_run_plugin).is_some())
                .unwrap_or_else(|| panic!("{what}: swap layout {name:?} has no bar pane"));
            let size = bar.split_size.unwrap_or_else(|| {
                panic!("{what}: swap layout {name:?} sizes its bar with nothing")
            });
            (name.clone().unwrap_or_default(), size)
        })
        .collect()
}

/// Every `MessagePlugin` keybind's configuration in a config artifact, as
/// zellij's REAL parser sees it. `KeybindPipe` is what a `MessagePlugin` node
/// becomes (zellij-utils kdl/mod.rs:2196-2209); `configuration: None` means an
/// EMPTY map at match time, not a wildcard (input/layout.rs:159-163).
fn keybind_pipe_configs(kdl: &str, what: &str) -> Vec<BTreeMap<String, String>> {
    let config = Config::from_kdl(kdl, None)
        .unwrap_or_else(|e| panic!("{what} did not parse: {e:?}\n---\n{kdl}"));
    let mut out = Vec::new();
    // Keybinds.0 is the public HashMap the existing unbind test already walks.
    for binds in config.keybinds.0.values() {
        for actions in binds.values() {
            for action in actions {
                if let Action::KeybindPipe { configuration, .. } = action {
                    out.push(configuration.clone().unwrap_or_default());
                }
            }
        }
    }
    out
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
        commit_ord: 0,
        last_visited: 0,
        worktree: Some("/home/o/code/clave/.claude-worktrees/ab12cd34".into()),
        label_source: LabelSource::Summary,
        tab_id: None,
        pane_id: None,
        stale: false,
        title: None,
        summary: String::new(),
        default_branch: None,
        context_tokens: None,
        context_level: None,
        live_session: None,
        buckets: Default::default(),
        model: None,
        provider: None,
        pr_number: None,
        pr_checked: 0,
        pr_branch: String::new(),
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

/// The theme passthrough (#145): `write_generated` appends the user's
/// `theme`/`themes`/`theme_dir` nodes to config.kdl, and an appended slice
/// that zellij's parser refuses would kill SESSION LAUNCH — exactly the
/// failure class this suite exists to catch. So the concatenated artifact
/// goes through the real parser, and the selection/definitions must come out
/// the other side as config values, not merely parse.
#[test]
fn config_with_theme_slice_parses_and_carries_the_selection() {
    // A realistic user config: built-in selection, an inline definition in
    // the modern styling form (transcribed shape of zellij's own theme
    // assets), and a custom definitions dir.
    let user = "theme \"kanagawa\"\n\
                themes {\n\
                    mytheme {\n\
                        text_unselected {\n\
                            base 220 215 186\n\
                            background 22 22 29\n\
                            emphasis_0 255 160 102\n\
                            emphasis_1 127 180 202\n\
                            emphasis_2 118 148 106\n\
                            emphasis_3 149 127 184\n\
                        }\n\
                    }\n\
                }\n\
                theme_dir \"/tmp/does-not-need-to-exist\"\n";
    let cfg = format!(
        "{}{}",
        setup::config_kdl("clave", WASM),
        setup::theme_slice_from(user)
    );
    let parsed = match Config::from_kdl(&cfg, None) {
        Ok(c) => c,
        Err(e) => panic!("config.kdl + theme slice failed zellij's parser: {e:?}\n---\n{cfg}"),
    };
    assert_eq!(
        parsed.options.theme.as_deref(),
        Some("kanagawa"),
        "the theme selection must survive the round-trip:\n{cfg}"
    );
    assert!(
        parsed.themes.get_theme("mytheme").is_some(),
        "the inline theme definition must survive the round-trip:\n{cfg}"
    );
    assert_eq!(
        parsed.options.theme_dir.as_deref(),
        Some(std::path::Path::new("/tmp/does-not-need-to-exist")),
        "theme_dir must survive the round-trip:\n{cfg}"
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
fn alt_a_carries_the_shared_floating_geometry() {
    // #110. A substring check would pass on `x 0` as readily as on `x "0%"`,
    // and only the second is read: x/y/width/height come out of
    // `kdl_child_string_value_for_entry` (zellij-utils kdl/mod.rs:1981-1993),
    // so an unquoted number parses fine and yields NO geometry — zellij then
    // falls back to `half_size_middle_geom` and the bug is back with a green
    // test. Assert the parsed ACTION instead: this is the same read zellij
    // does at keypress, so it cannot spell the geometry away.
    use zellij_utils::data::{BareKey, InputMode, KeyWithModifier};
    use zellij_utils::input::actions::Action;
    use zellij_utils::input::layout::PercentOrFixed;

    let config =
        Config::from_kdl(&setup::config_kdl("clave", WASM), None).expect("config.kdl must parse");
    let alt_a = KeyWithModifier::new(BareKey::Char('a')).with_alt_modifier();
    let actions = config
        .keybinds
        .get_actions_for_key_in_mode(&InputMode::Normal, &alt_a)
        .expect("Alt+a must be bound in Normal");

    let coordinates = actions
        .iter()
        .find_map(|action| match action {
            // `floating true` is what routes Run to NewFloatingPane at all
            // (kdl/mod.rs:1999) — matching this variant proves the pane still
            // floats as well as proving its size.
            Action::NewFloatingPane { coordinates, .. } => Some(coordinates.clone()),
            _ => None,
        })
        .expect("Alt+a must open a FLOATING pane")
        .expect("Alt+a's floating pane must carry explicit geometry (#110)");

    // Percents, not fixed columns: they are resolved against the viewport the
    // bar has already been subtracted from, so they follow a resize and a
    // collapse without clave recomputing anything.
    assert_eq!(
        (
            coordinates.x,
            coordinates.y,
            coordinates.width,
            coordinates.height
        ),
        (
            Some(PercentOrFixed::Percent(clave_types::FLOATING_X_PERCENT)),
            Some(PercentOrFixed::Percent(clave_types::FLOATING_Y_PERCENT)),
            Some(PercentOrFixed::Percent(clave_types::FLOATING_WIDTH_PERCENT)),
            Some(PercentOrFixed::Percent(
                clave_types::FLOATING_HEIGHT_PERCENT
            )),
        ),
        "Alt+a's geometry must be clave-types' shared floating geometry — the \
         one #7's helper pane will read too"
    );
}

#[test]
fn alt_f_pipes_the_press_to_the_bar_alone() {
    // #207 reverses the #188 trade: the sized `Run` spawned a NEW shell every
    // press, and no KDL form can size AND toggle (a bare ToggleFloatingPanes
    // slivers its first spawn; a swap_floating_layout resolves x from absolute
    // column zero and parks the shell over the bar — live probe 2026-08-17).
    // So Alt+f is a MessagePlugin pipe and the bar decides spawn/show/hide.
    // Assert the PARSED action, in normal AND locked (invariant #6; stock's
    // own Alt+f lives in `shared_except "locked"`), and assert it rides ALONE:
    // a merge that APPENDED would leave stock's ToggleFloatingPanes firing on
    // the same press, hiding what the bar just spawned.
    use zellij_utils::data::{BareKey, InputMode, KeyWithModifier};

    let base = Config::from_default_assets().expect("stock zellij 0.44.3 defaults must parse");
    let merged = Config::from_kdl(&setup::config_kdl("clave", WASM), Some(base))
        .expect("clave config.kdl must merge over stock defaults");
    let alt_f = KeyWithModifier::new(BareKey::Char('f')).with_alt_modifier();

    for mode in [InputMode::Normal, InputMode::Locked] {
        let actions = merged
            .keybinds
            .get_actions_for_key_in_mode(&mode, &alt_f)
            .unwrap_or_else(|| panic!("Alt+f must be bound in {mode:?}"));
        assert_eq!(
            actions.len(),
            1,
            "Alt+f in {mode:?} must carry clave's pipe ALONE: {actions:?}"
        );
        let Action::KeybindPipe {
            name,
            configuration,
            ..
        } = &actions[0]
        else {
            panic!("Alt+f in {mode:?} must be a MessagePlugin pipe: {actions:?}")
        };
        assert_eq!(name.as_deref(), Some("clave-shell"), "wrong pipe name");
        // The clave_binary config key is plugin IDENTITY: a bind whose
        // configuration differs from the loaded bar's makes zellij boot a
        // SECOND bar instance instead of delivering the pipe.
        assert_eq!(
            configuration
                .clone()
                .unwrap_or_default()
                .get(clave_types::CLAVE_BINARY_KEY)
                .map(String::as_str),
            Some("clave"),
            "Alt+f's pipe must carry the bar's identity configuration"
        );
    }
}
#[test]
fn layout_kdl_parses_through_real_zellij_parser() {
    assert_layout_ok(&setup::layout_kdl(BIN_ABS, WASM), "layout.kdl");
}

#[test]
fn launch_layout_kdl_parses_in_both_branches() {
    // Empty store → bar-only (template + one plain `clave` tab).
    assert_layout_ok(
        &setup::launch_layout_kdl("clave", WASM, None, false),
        "launch.kdl (empty store, bar-only)",
    );
    // Non-empty store → the eager most-recent branch, which composes in
    // `add::tab_node_bare` — a distinct code path (bare tab node, no bar pane)
    // that the empty branch never touches.
    let r = eager_record();
    assert_layout_ok(
        &setup::launch_layout_kdl(BIN_ABS, WASM, Some(&r), false),
        "launch.kdl (eager most-recent tab)",
    );
}

/// #181, the whole width mechanism in one assertion. Every layout clave emits
/// must declare BOTH geometries as swap layouts, sized as the two FIXED column
/// constants (fixed panes refuse every resize, which is what makes the bar
/// border un-draggable and the painted width exactly one of the two constants
/// the bar compares against) — and with the geometry the tab WAS born in FIRST
/// (#197), because `next_swap_layout` is relative and zellij hides the tab's
/// own birth layout ahead of the declared pair: the first call lands on the
/// first DECLARED geometry, and the bar has to spend that call to trade the
/// unnameable birth position (`"BASE"`) for a name it can compare against the
/// store. Same-width-first makes that call invisible; the opposite order made
/// every cold start flash.
#[test]
fn every_layout_declares_both_swap_geometries_birth_width_first() {
    let expected_order = |born_collapsed: bool| {
        [
            clave_types::swap_layout_name(born_collapsed),
            clave_types::swap_layout_name(!born_collapsed),
        ]
    };
    let check = |kdl: &str, what: &str, born_collapsed: bool| {
        let sizes = swap_bar_sizes(kdl, what);
        assert_eq!(
            sizes.len(),
            2,
            "{what}: expected two swap geometries, got {sizes:?}"
        );
        let names: Vec<&str> = sizes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            expected_order(born_collapsed),
            "{what}: wrong swap order"
        );
        let fixed = |s: &SplitSize| match s {
            SplitSize::Fixed(f) => *f,
            // A `Percent` bar would let the border be dragged and its painted
            // width drift off the two constants the bar reasons in — percent
            // was only ever mandatory while the deleted resize seek needed a
            // resizable pane (c8-cold-start 2026-07-18).
            SplitSize::Percent(p) => panic!("{what}: bar sized {p}%, not fixed cols"),
        };
        let (expanded, collapsed) = if born_collapsed {
            (fixed(&sizes[1].1), fixed(&sizes[0].1))
        } else {
            (fixed(&sizes[0].1), fixed(&sizes[1].1))
        };
        assert_eq!(expanded, clave_types::BAR_TARGET_COLS, "{what}");
        assert_eq!(collapsed, clave_types::COLLAPSED_TARGET_COLS, "{what}");
        // …and the two geometries must declare the SAME bar, so that switching
        // between them moves the running one instead of spawning a second.
        let runs = swap_bar_runs(kdl, what);
        assert_eq!(
            runs.len(),
            2,
            "{what}: expected two bar panes, got {runs:?}"
        );
        assert_eq!(
            runs[0], runs[1],
            "{what}: the two swap geometries declare DIFFERENT bars — a swap \
             would no longer match the running pane"
        );
    };

    check(&setup::layout_kdl(BIN_ABS, WASM), "layout.kdl", false);
    for born_collapsed in [false, true] {
        check(
            &setup::launch_layout_kdl("clave", WASM, None, born_collapsed),
            "launch.kdl",
            born_collapsed,
        );
        check(
            &add::tab_layout(BIN_ABS, WASM, "row", "u-1", "/tmp", born_collapsed),
            "one-shot tab layout",
            born_collapsed,
        );
    }
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
    let kdl = add::tab_layout(BIN_ABS, WASM, &label, "u-1", cwd, false);
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
    let raw = add::tab_layout(
        BIN_ABS,
        WASM,
        r"fix the \d regex",
        "u-1",
        "/home/o/x",
        false,
    );
    assert!(
        Layout::from_str(&raw, "guardrail:raw-backslash".into(), None, None).is_err(),
        "premise broken: zellij's KDL parser now ACCEPTS a raw backslash — re-vet the guard\n---\n{raw}"
    );
    // The guard: the same label THROUGH sanitize_label must parse clean.
    let label = add::sanitize_label(r"fix the \d regex");
    let kdl = add::tab_layout(BIN_ABS, WASM, &label, "u-1", "/home/o/x", false);
    assert_layout_ok(&kdl, "add/open tab layout (backslash-bearing label)");
    // And a backslash-bearing cwd must be REFUSED, not baked.
    assert!(add::validate_cwd(r"/home/o/we\ird").is_err());
}

#[test]
fn keybind_and_layout_plugin_configurations_match() {
    // #44: zellij resolves a pipe's destination by EXACT match on
    // (location, configuration) — a nested HashMap keyed by
    // PluginUserConfiguration (zellij-server plugins/wasm_bridge.rs:1676-1686)
    // — and a miss LAUNCHES A NEW PLUGIN (ibid. :1861-1894). So config.kdl's
    // keybinds and the layout that actually starts the bar must carry the
    // SAME configuration, or every Alt+c/Alt+j/Alt+o press spawns a second
    // sidebar: the exact v0.1.1 field failure this change exists to end.
    //
    // Targets are the layouts zellij is actually GIVEN — launch.kdl
    // (setup.rs:708-711) and the one-shot add/open tab layout (open.rs:122,
    // add.rs:726). layout.kdl is generated but never passed to zellij, so it
    // is checked as a bonus, not as the contract.
    let cfg = setup::config_kdl(BIN_ABS, WASM);
    let kb = keybind_pipe_configs(&cfg, "config.kdl");

    // Non-vacuity FIRST (adversarial review, 2026-07-22): the per-element
    // presence assert below is what pins the KEY, but the layout-vs-keybind
    // `assert_eq!` further down is satisfied when both maps are EMPTY. If the
    // keybind extraction ever silently produced nothing (a refactor, a parser
    // change), every downstream equality would pass vacuously against an empty
    // layout side. Assert the keybind set is non-empty before trusting it.
    assert!(
        !kb.is_empty(),
        "config.kdl produced no MessagePlugin keybinds — the extraction is \
         vacuous:\n{cfg}"
    );
    for c in &kb {
        assert_eq!(
            c.get(clave_types::CLAVE_BINARY_KEY).map(String::as_str),
            Some(BIN_ABS),
            "a MessagePlugin keybind lacks the {} configuration key; it will \
             address a DIFFERENT plugin than the running bar (#44):\n{cfg}",
            clave_types::CLAVE_BINARY_KEY
        );
    }

    // `keybinds.0` (walked by keybind_pipe_configs) is a HashMap<InputMode,
    // …>, so `kb`'s element order is NOT stable across runs — comparing
    // layouts against an arbitrary `kb[N]` below would only catch a mismatch
    // when that particular element happens to land there. Proven live: adding
    // an extra key to only the nav closure's MessagePlugin config and running
    // a `kb[0]`-only comparison 40 times gave 9 passes / 31 failures. An EXTRA
    // key breaks plugin identity exactly as fatally as a missing one (#44), so
    // assert the keybind side is internally coherent FIRST — every entry
    // equal to every other — before picking any one of them as the agreed
    // value the layouts are checked against.
    for c in &kb[1..] {
        assert_eq!(
            c, &kb[0],
            "config.kdl's MessagePlugin keybinds disagree with each other on \
             their configuration — some keypresses will address a DIFFERENT \
             plugin than others (#44):\n{cfg}"
        );
    }
    let agreed_keybind_config = kb[0].clone();

    let r = eager_record();
    let launch_eager = setup::launch_layout_kdl(BIN_ABS, WASM, Some(&r), false);
    let launch_empty = setup::launch_layout_kdl(BIN_ABS, WASM, None, false);
    let one_shot = add::tab_layout(BIN_ABS, WASM, "lbl", "u-1", "/home/o/x", false);
    let layout_kdl_text = setup::layout_kdl(BIN_ABS, WASM);

    for (what, text) in [
        ("launch.kdl (eager most-recent tab)", &launch_eager),
        ("launch.kdl (empty store, bar-only)", &launch_empty),
        ("add/open one-shot tab layout", &one_shot),
        (
            "layout.kdl (generated, not passed to zellij)",
            &layout_kdl_text,
        ),
    ] {
        let plugins = layout_plugin_configs(text, what);
        assert!(
            !plugins.is_empty(),
            "{what} carries no plugin pane — extraction is vacuous:\n{text}"
        );
        for p in &plugins {
            assert_eq!(
                p.get(clave_types::CLAVE_BINARY_KEY).map(String::as_str),
                Some(BIN_ABS),
                "{what}'s plugin node lacks the {} key (#44):\n{text}",
                clave_types::CLAVE_BINARY_KEY
            );
            // The pair must MATCH, not merely both be present.
            assert_eq!(
                p, &agreed_keybind_config,
                "{what}'s plugin configuration differs from config.kdl's \
                 keybind configuration — every keybind press will launch a \
                 SECOND bar (#44):\nlayout={p:?}\nkeybind={agreed_keybind_config:?}"
            );
        }
    }
}
