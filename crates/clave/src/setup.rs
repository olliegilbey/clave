//! `clave setup` (§6.8/§7): make the machine ready — generated session
//! config + layout in ~/.local/share/clave/, Claude hooks merged into
//! ~/.claude/settings.json (ADDITIVELY — the file may be a dotfiles symlink;
//! we edit through it and never clobber existing hooks), and Zellij's
//! permission cache pre-seeded (grants are all-or-nothing and the in-bar
//! prompt is unanswerable — S1/S2).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Where clave's generated artifacts live. NOT the repo: these files embed
/// machine-absolute paths (the wasm location) and the repo is public.
/// `$CLAVE_DATA_DIR` overrides the whole dir (spec §6.9: the dev harness sandboxes the store).
pub fn data_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("home")?;
    let default = home.join(".local").join("share").join("clave");
    Ok(crate::env::dir_from(
        std::env::var("CLAVE_DATA_DIR").ok(),
        default,
    ))
}

/// The bar wasm this environment loads. Version-aware: a release installs
/// `clave-bar-vX.Y.Z.wasm` and every generated reference points at it, so
/// prefer the versioned artifact when it exists; the sandbox/dev install
/// keeps the unversioned `clave-bar.wasm` (working-tree builds) and falls
/// through to it. Keyed on THIS binary's version so a stable session
/// resolves the exact wasm baked into its config at launch.
pub fn wasm_path() -> Result<PathBuf> {
    let dir = data_dir()?;
    let versioned = dir.join(crate::release::versioned_wasm_name(env!(
        "CARGO_PKG_VERSION"
    )));
    Ok(if versioned.exists() {
        versioned
    } else {
        dir.join("clave-bar.wasm")
    })
}

/// Where `launch_session` writes the dynamically-composed launch layout.
/// STABLE (not pid-suffixed) and inside the data dir alongside the generated
/// config.kdl/layout.kdl: `launch_session` exec()s zellij and never returns
/// (CommandExt::exec deliberately replaces the process so zellij owns the
/// terminal), so no post-exec cleanup can run — a per-launch temp file would
/// leak forever. One overwritten file: no accumulation, and it doubles as a
/// debuggable artifact of the last cold start.
pub fn launch_layout_path(dir: &std::path::Path) -> PathBuf {
    dir.join("launch.kdl")
}

/// §6.6's exact permission set. Keep THIS list, load()'s request_permission
/// call, and the seeded cache in lockstep — a partial cache match raises the
/// unanswerable prompt and withholds everything.
pub const BAR_PERMISSIONS: [&str; 4] = [
    "ReadCliPipes",
    "ChangeApplicationState",
    "ReadApplicationState",
    "RunCommands",
];

/// The clave session config: Alt keybinds in shared_among normal+locked
/// (invariant #6 — they must fire while Claude has focus), defaults kept.
///
/// `binary` is what the `Alt a` keybind's `Run` invokes: bare `clave` (PATH
/// = the dev binary) for the sandbox/dev install, the versioned copy's
/// ABSOLUTE path for a release (§2 binary split — a stable session must never
/// fall through to `~/.cargo/bin/clave`). The caller's environment decides.
pub fn config_kdl(binary: &str, wasm: &str) -> String {
    let nav = |payload: &str| {
        // Trailing `;` after the child block is REQUIRED: zellij's KDL parser
        // rejects a `}`-closed node that isn't terminated before the enclosing
        // bind-block `}` (found live in Task 9 C1 — `zellij setup --check`
        // failed on exactly these lines while the `;`-terminated Alt+a/Alt+c
        // forms parsed fine).
        //
        // `clave_binary` must match the layout's plugin configuration exactly
        // (#44): zellij resolves this pipe's destination by (location,
        // configuration) hash-map lookup, and a miss LAUNCHES A SECOND BAR
        // (zellij-server plugins/wasm_bridge.rs:1676-1686, :1861-1894).
        format!(
            "MessagePlugin \"file:{wasm}\" {{ name \"clave-nav\"; payload \"{payload}\"; {key} \"{binary}\"; }};",
            key = clave_types::CLAVE_BINARY_KEY
        )
    };
    let mut binds = String::new();
    // The picker's geometry is clave-types' (#110), shared with #7's helper
    // pane when it lands. Without it zellij applies `half_size_middle_geom` —
    // half of a viewport the bar has ALREADY shrunk, which is the tiny pane
    // the issue reports. The values are percents of that non-bar area, and
    // they must be KDL STRINGS: `kdl_child_string_value_for_entry` is what
    // reads x/y/width/height (`zellij-utils/src/kdl/mod.rs:1981-1993`), so a
    // bare integer is read as no value and the geometry silently vanishes.
    binds.push_str(&format!(
        "        bind \"Alt a\" {{ Run \"{binary}\" \"add\" {{ floating true; close_on_exit true; \
         x \"{x}%\"; y \"{y}%\"; width \"{w}%\"; height \"{h}%\"; }}; }}\n",
        x = clave_types::FLOATING_X_PERCENT,
        y = clave_types::FLOATING_Y_PERCENT,
        w = clave_types::FLOATING_WIDTH_PERCENT,
        h = clave_types::FLOATING_HEIGHT_PERCENT,
    ));
    binds.push_str(&format!(
        "        bind \"Alt c\" {{ MessagePlugin \"file:{wasm}\" {{ name \"clave-toggle\"; {key} \"{binary}\"; }}; }}\n",
        key = clave_types::CLAVE_BINARY_KEY
    ));
    // Alt+t/Alt+w are the clave-owned tab pair. Alt+t exists because #28
    // unbound stock Ctrl+t (it swallowed Claude Code's todo view), and Ctrl+t
    // was the entry to zellij's tab mode — where `n` made a plain terminal
    // tab, the maintainer's muscle-memory path (live finding 2026-07-22). A
    // one-keystroke bind in clave's own Alt namespace is the replacement, and
    // the generated layout's default_tab_template means the new tab gets its
    // own bar pane automatically (no clave-agent row: plain tabs aren't store
    // rows, same as the base `clave` tab).
    binds.push_str("        bind \"Alt t\" { NewTab; }\n");
    binds.push_str("        bind \"Alt w\" { CloseTab; }\n");
    // Alt+↓/↑ walk the DISPLAYED list (§6.6 revised 2026-07-08: rows only
    // reorder on user commitments, never on focus — so walking the visible
    // order is stable, no ping-pong). Executor-gated in the plugin: only the
    // active instance (fresh tab set, the bar being read) computes the step.
    //
    // The two rules that make walking safe, stated for #39 (S1):
    //   R1 — a prompt always moves its row to the top. No tie, no clock, no
    //        dependence on where the tab happens to sit.
    //   R2 — closing a tab reorders nothing relative to anything else. The
    //        closed row changes glyph and keeps its rank; no untouched row
    //        overtakes another.
    // Everything else — Claude finishing, focus, clicks, these nav keys —
    // changes status or selection and never the order.
    binds.push_str(&format!(
        "        bind \"Alt j\" \"Alt Down\" {{ {} }}\n",
        nav("{\\\"dir\\\":\\\"next\\\"}")
    ));
    binds.push_str(&format!(
        "        bind \"Alt k\" \"Alt Up\" {{ {} }}\n",
        nav("{\\\"dir\\\":\\\"prev\\\"}")
    ));
    // #100 dwell-commit: Alt+Enter is the ONLY act that wakes a dormant row —
    // nav and clicks merely select (the model no-ops this without a dormant
    // selection). Bare Enter must reach the terminal (you could not talk to
    // Claude otherwise), and Alt+h/l stay stock (bar↔terminal focus).
    binds.push_str(&format!(
        "        bind \"Alt Enter\" {{ {} }}\n",
        nav("{\\\"commit\\\":true}")
    ));
    // True alt-tab (last two focused tabs) is NATIVE — server-side truth.
    // Alt+o = native ToggleTab PLUS a clave-organic pipe: the pipe arms the
    // bounded beacon announce (the newly-active instance's next TabUpdate
    // announces it — rounds 11–12: unbounded self-diagnosed announces
    // stormed; organic switches must be explicitly signalled).
    binds.push_str(&format!(
        "        bind \"Alt o\" {{ ToggleTab; MessagePlugin \"file:{wasm}\" {{ name \"clave-organic\"; {key} \"{binary}\"; }}; }}\n",
        key = clave_types::CLAVE_BINARY_KEY
    ));
    for n in 1..=9 {
        binds.push_str(&format!(
            "        bind \"Alt {n}\" {{ {} }}\n",
            nav(&format!("{{\\\"row\\\":{n}}}"))
        ));
    }
    // #28: clear-defaults=false keeps stock binds, which SWALLOW keys Claude
    // Code needs before it ever sees them — Ctrl+g (locked-mode toggle vs
    // read-input-field), Ctrl+t (tab mode vs todo view), Ctrl+o (session mode
    // vs verbose output), Ctrl+b (tmux mode vs background command) — plus
    // Ctrl+q (stock Quit: one stray press kills the whole fleet session). A
    // SINGLE top-level `unbind` node inside `keybinds` (sibling to
    // shared_among, NOT nested in it) is the verified-correct form for
    // stripping a key from ALL modes when a user config merges over defaults:
    // zellij-utils 0.44.3 Keybinds::from_kdl picks it up via
    // `children().get("unbind")` and runs unbind_keys_in_all_modes — which
    // iterates every mode in Keybinds.0 and `.remove()`s each key — AFTER the
    // bind blocks merge, so it deletes the stock binds (kdl/mod.rs:4600,4627).
    // kdl 4.7.1 KdlDocument::get returns the FIRST match ONLY (document.rs:80),
    // so all five keys MUST ride this one node — a second `unbind` would be
    // silently ignored. Surgical by design: the spec's clear-defaults=false
    // stance holds, stock pane/resize/scroll/move modes remain (#24 defers the
    // full clave-owned scheme).
    let unbinds = r#"unbind "Ctrl g" "Ctrl t" "Ctrl o" "Ctrl b" "Ctrl q""#;
    format!(
        "// GENERATED by `clave setup` — regenerate, don't hand-edit.\n\
         // §6.8 C8: resurrection is clave-owned (launch + clave open);\n\
         // zellij serialization replays DISCOVERED commands and is off.\n\
         session_serialization false\n\
         // clear-defaults=false: stock zellij behaviour stays; clave only ADDS.\n\
         keybinds clear-defaults=false {{\n\
         \x20   shared_among \"normal\" \"locked\" {{\n{binds}\x20   }}\n\
         \x20   {unbinds}\n\
         }}\n"
    )
}

/// The session layout: EVERY tab gets the bar via default_tab_template
/// (§6.8). Task 9 checkpoint C6 validates the template survives real use;
/// fallback = per-tab panes + a new-tab keybind with an explicit layout.
pub fn layout_kdl(binary: &str, wasm: &str) -> String {
    // split_direction="vertical" goes ON the template node, with `pane` and
    // `children` as DIRECT children (both Task 9 C1 findings):
    // 1. zellij stacks siblings horizontally (rows) by default — without
    //    vertical split the bar is a 30-ROW strip across the top;
    // 2. `children` must NOT be nested inside a wrapper pane: the empty/new-
    //    tab fill path (zellij-utils kdl_layout_parser.rs:1748, v0.44.3) only
    //    inserts the default pane at the template's TOP-LEVEL
    //    external_children_index — it does not recurse — so a nested
    //    `children` yields tabs with no terminal at all.
    // size MUST be a percent: fixed sizes (`size=30`) make zellij refuse
    // every resize on the pane (CantResizeFixedPanes) — Alt+c collapse was
    // dead in any freshly-launched session (c8-cold-start 2026-07-18; pre-C8
    // sessions were resurrected from the serialized cache, which rewrites
    // sizes as percentages — masking this). The bar's birth-armed width seek
    // converges the birth percent onto the exact template cols.
    //
    // The percent is FORMATTED from `clave_types::BAR_BIRTH_PERCENT`, which is
    // `BAR_TARGET_COLS` against S8 §3.4's 200-column reference viewport — the
    // same derivation that made 15% right for 30 columns and 19% for 38. It is
    // a HINT, not a contract: the seek corrects a bad birth either way, so a
    // stale percent costs a visible flicker on every launch, not a wrong bar —
    // S8 §3.3's own words, "the percent is a birth hint, the seek is the
    // authority". Hand-deriving it here (three format strings, three chances to
    // forget) is what §3.3 names as the acceptable-but-worse fallback; it is
    // now taken as §3.3 actually recommends (#86).
    // The plugin's configuration is HALF of zellij's identity for it: pipe
    // destinations match on (location, configuration) exactly, so this key
    // must equal what config_kdl bakes into every MessagePlugin keybind or
    // each keypress launches a second bar (#44).
    format!(
        "// GENERATED by `clave setup` — regenerate, don't hand-edit.\n\
         layout {{\n\
         \x20   default_tab_template split_direction=\"vertical\" {{\n\
         \x20       pane size=\"{pct}%\" borderless=true {{\n\
         \x20           plugin location=\"file:{wasm}\" {{\n\
         \x20               {key} \"{binary}\"\n\
         \x20           }}\n\
         \x20       }}\n\
         \x20       children\n\
         \x20   }}\n\
         \x20   tab name=\"clave\" focus=true\n\
         }}\n",
        key = clave_types::CLAVE_BINARY_KEY,
        pct = clave_types::BAR_BIRTH_PERCENT
    )
}

/// §6.8 (C8): the launch layout, composed DYNAMICALLY at session-create
/// time. Base = the bar template; store non-empty → ONE eager tab for the
/// most-recent row, baked `clave spawn` (resumes via the jsonl check).
/// Everything else surfaces as dormant bar rows (§6.6).
/// The width of the terminal `clave` is being launched FROM, or `None` when
/// there is no TTY (a script, a CI run, a piped invocation).
///
/// Correct by construction only on the launch path: `clave dev launch` and
/// `clave` run in a real terminal OUTSIDE zellij, so the controlling TTY's
/// width IS the display area the layout's percent will be resolved against.
/// The same call from a process running INSIDE a zellij pane would return the
/// PANE's width — which is why `add::tab_node` still uses the fiction and
/// LEDGER D35 records that as the remaining gap.
fn launching_terminal_cols() -> Option<usize> {
    terminal_size::terminal_size().map(|(w, _)| usize::from(w.0))
}

pub fn launch_layout_kdl(
    binary: &str,
    wasm: &str,
    most_recent: Option<&crate::store::AgentRecord>,
    collapsed: bool,
) -> String {
    launch_layout_kdl_for(
        binary,
        wasm,
        most_recent,
        launching_terminal_cols(),
        collapsed,
    )
}

/// The testable half: the display width is a PARAMETER, so the tests can pin
/// what a 280-column terminal emits without owning a TTY.
pub fn launch_layout_kdl_for(
    binary: &str,
    wasm: &str,
    most_recent: Option<&crate::store::AgentRecord>,
    display_cols: Option<usize>,
    collapsed: bool,
) -> String {
    let tab = match most_recent {
        // The label is re-sanitized for KDL safety: it can be hook-derived
        // (§6.5) and only add-time labels went through sanitize_label.
        // BARE node (no bar pane): default_tab_template wraps explicit tab
        // nodes too, so a bar-carrying node here rendered a DOUBLE bar in
        // the eager tab (live finding, c8-cold-start 2026-07-18).
        Some(r) => crate::add::tab_node_bare(
            binary,
            &crate::add::sanitize_label(&r.label),
            &r.uuid,
            &r.cwd,
        ),
        None => "    tab name=\"clave\" focus=true\n".to_string(),
    };
    // percent-sized, not size=30: fixed panes refuse resizes — see layout_kdl.
    // `clave_binary` identity requirement: see layout_kdl.
    //
    // The percent is derived from the REAL terminal width when there is one
    // (LEDGER D35). This is the single highest-leverage number in the width
    // system: zellij only resizes in whole increments, so the widths the bar
    // can ever occupy form a lattice anchored at its BIRTH. Born near the
    // target, a collapse and expand return to it on any display; born at the
    // 200-column fiction's percent, a 280-column display rests at 47 forever
    // and never reaches 54 (D34, measured). This template is also what every
    // Alt+t tab inherits, so one correct number here fixes the whole session.
    format!(
        "// GENERATED at launch — §6.8 clave-owned cold start.\n\
         layout {{\n\
         \x20   default_tab_template split_direction=\"vertical\" {{\n\
         \x20       pane size=\"{pct}%\" borderless=true {{\n\
         \x20           plugin location=\"file:{wasm}\" {{\n\
         \x20               {key} \"{binary}\"\n\
         \x20           }}\n\
         \x20       }}\n\
         \x20       children\n\
         \x20   }}\n{tab}}}\n",
        key = clave_types::CLAVE_BINARY_KEY,
        pct = display_cols.map_or(clave_types::BAR_BIRTH_PERCENT, |cols| {
            clave_types::birth_percent_for(cols, clave_types::target_cols_for(collapsed))
        })
    )
}

/// Does `zellij list-sessions -n` mention this session at all (live OR
/// EXITED)? An EXITED session must be DELETED before create: `attach
/// --create` would resurrect its serialized state, ignoring `--layout`
/// (§6.8) — replaying pre-C8 discovered commands.
pub fn session_exists(list_output: &str, name: &str) -> bool {
    list_output
        .lines()
        .any(|l| l.split_whitespace().next() == Some(name))
}

/// Is `cmd` a clave hook registration for `event` (any binary path form)?
/// Matches `<bin> hook <EVENT>` where `<bin>`'s basename is `clave` or
/// `clave-vX.Y.Z` — bare PATH `clave`, an absolute versioned copy, or an
/// older version's absolute path all count. A foreign `my-bell`/`notify hook
/// Stop` does NOT (its basename isn't ours), so replace-on-version-change
/// never touches a user's own hook. The `clave-v` check requires a DIGIT
/// immediately after the prefix (not just `starts_with`) — a foreign tool
/// named `clave-vault` or `clave-verify` shares the textual prefix with our
/// versioned binary name but is not ours, and must never be absorbed or
/// rewritten by merge_hooks.
pub fn is_clave_hook_command(cmd: &str, event: &str) -> bool {
    let Some(bin) = cmd.strip_suffix(&format!(" hook {event}")) else {
        return false;
    };
    matches!(
        std::path::Path::new(bin).file_name().and_then(|n| n.to_str()),
        Some(name) if name == "clave"
            || name.strip_prefix("clave-v").is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
    )
}

/// The §6.5 state machine's input events — hook registration AND doctor's
/// exactly-one-entry check key off the same list.
pub const HOOK_EVENTS: [&str; 4] = ["UserPromptSubmit", "Stop", "Notification", "SessionEnd"];

/// Merge clave's hook registrations into a settings.json value, keyed on
/// `clave_bin` (the command path to bake — bare `clave` for dev, the
/// versioned copy's absolute path for a release).
///
/// Replace-on-version-change (§2): an existing clave hook entry — ANY prior
/// version's command path — is REPLACED in place by `clave_bin`'s, never
/// duplicated; a same-version re-run is idempotent (no change). Non-clave
/// entries are never touched (the never-clobber invariant, §6.5). Returns
/// whether anything changed.
pub fn merge_hooks(settings: &mut serde_json::Value, clave_bin: &str) -> bool {
    let mut changed = false;
    let hooks = settings
        .as_object_mut()
        .map(|o| o.entry("hooks").or_insert_with(|| serde_json::json!({})))
        .expect("settings.json root must be an object");
    for ev in HOOK_EVENTS {
        let cmd = format!("{clave_bin} hook {ev}");
        let want = serde_json::json!(cmd);
        let arr = hooks
            .as_object_mut()
            .expect("hooks must be an object")
            .entry(ev)
            .or_insert_with(|| serde_json::json!([]));
        let entries = arr.as_array_mut().expect("hook event must be an array");
        // Keep EXACTLY ONE clave hook per event (review 2026-07-22, Fix 4):
        // rewrite the FIRST clave match (any version's path) to the current
        // command — a version cut MUST NOT leave the prior release's hook
        // behind — and REMOVE every subsequent clave match, because Claude
        // fires ALL matching hooks and duplicates double-fire. Only OUR
        // command strings are eligible; user hooks pass through untouched.
        let mut found = false;
        for e in entries.iter_mut() {
            let Some(hs) = e.get_mut("hooks").and_then(|v| v.as_array_mut()) else {
                continue;
            };
            hs.retain_mut(|h| {
                let ours = h
                    .get("command")
                    .and_then(|v| v.as_str())
                    .is_some_and(|c| is_clave_hook_command(c, ev));
                if !ours {
                    return true; // foreign hook — never touch
                }
                if found {
                    changed = true; // a duplicate clave hook — drop it
                    return false;
                }
                found = true;
                if h["command"] != want {
                    h["command"] = want.clone();
                    changed = true;
                }
                true
            });
        }
        // Drop entry objects whose hooks array emptied out (all its hooks were
        // duplicate clave matches we removed) — never leave a hollow
        // {"hooks": []}. Foreign-bearing entries can't empty (foreign is kept).
        let before = entries.len();
        entries.retain(|e| {
            e.get("hooks")
                .and_then(|v| v.as_array())
                .is_none_or(|a| !a.is_empty())
        });
        if entries.len() != before {
            changed = true;
        }
        if !found {
            entries.push(serde_json::json!({
                "hooks": [ { "type": "command", "command": cmd } ]
            }));
            changed = true;
        }
    }
    changed
}

/// Zellij's permission cache location (verified on this machine in S1).
pub fn permissions_cache_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("home")?;
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/Caches/org.Zellij-Contributors.Zellij/permissions.kdl"))
    } else {
        Ok(home.join(".cache/zellij/permissions.kdl"))
    }
}

/// Merge our grant into the cache text, preserving everyone else's entries.
/// Format (verified against zellij 0.44.3 PermissionCache::to_string): one
/// quoted-location node per plugin, children = PermissionType names. We
/// remove any existing clave-bar nodes (both key forms) then append fresh
/// ones — replace-not-accumulate keeps re-runs idempotent even when the
/// permission set changes (the S2 lesson).
pub fn merge_permissions_kdl(existing: &str, wasm_abs: &str) -> String {
    let keys = [format!("\"file:{wasm_abs}\""), format!("\"{wasm_abs}\"")];
    let mut out = String::new();
    let mut skipping = false;
    for line in existing.lines() {
        let t = line.trim_start();
        if !skipping && keys.iter().any(|k| t.starts_with(k.as_str())) {
            skipping = true; // drop this node…
        }
        if skipping {
            if t.trim_end().ends_with('}') {
                skipping = false; // …through its closing brace
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    for key in keys {
        out.push_str(&format!("{key} {{\n"));
        for p in BAR_PERMISSIONS {
            out.push_str(&format!("    {p}\n"));
        }
        out.push_str("}\n");
    }
    out
}

/// Is our grant present in the permission-cache text? Same key form
/// merge_permissions_kdl writes — doctor never guesses a second format.
pub fn permissions_seeded(existing: &str, wasm_abs: &str) -> bool {
    existing.contains(&format!("\"file:{wasm_abs}\""))
}

/// The generation weave shared by `clave setup` (dev/sandbox) and `clave
/// release` (stable): write config.kdl + layout.kdl baking `binary` into
/// commands and `wasm` into plugin locations, merge the clave hooks
/// (replace-on-version-change, keyed on `binary`), and seed the permission
/// cache for `wasm`. Idempotent by construction — every part merges.
///
/// The two callers differ ONLY in what they pass: dev = (`"clave"`,
/// unversioned wasm); release = (versioned CLI absolute path, versioned
/// wasm). Everything version-shaped stays in the caller (§2).
pub fn write_generated(dir: &std::path::Path, binary: &str, wasm: &str) -> Result<()> {
    std::fs::write(dir.join("config.kdl"), config_kdl(binary, wasm))?;
    std::fs::write(dir.join("layout.kdl"), layout_kdl(binary, wasm))?;

    // Hooks: read-merge-write $CLAUDE_CONFIG_DIR/settings.json. The path may
    // be a symlink into a dotfiles repo — fs::read/write follow it, which is
    // exactly what we want (§6.5). Routed via claude_config_dir() (not
    // hardcoded home) so `clave dev`'s sandbox (§6.9) merges hooks into its
    // OWN settings.json — else sandbox sessions get no clave hooks and
    // scenario agents never report status. Env unset ⇒ ~/.claude, unchanged.
    let settings_path = crate::env::claude_config_dir()?.join("settings.json");
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?; // fresh box: ~/.claude may not exist yet
    }
    let mut settings: serde_json::Value = match std::fs::read(&settings_path) {
        Ok(b) => serde_json::from_slice(&b).context("parsing ~/.claude/settings.json")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e).context("reading settings.json"),
    };
    if merge_hooks(&mut settings, binary) {
        std::fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)?;
        println!("hooks merged into {}", settings_path.display());
    } else {
        println!("hooks already registered");
    }

    // Permissions pre-seed (§7): merge, preserving other plugins.
    let cache = permissions_cache_path()?;
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::read_to_string(&cache).unwrap_or_default();
    std::fs::write(&cache, merge_permissions_kdl(&existing, wasm))?;
    println!("permissions seeded in {}", cache.display());
    Ok(())
}

/// Write the embedded wasm as the VERSIONED artifact — which wasm_path()
/// already prefers — if absent (spec §Distribution). Write-if-absent keeps
/// re-runs idempotent and honors running-session immunity (§2).
pub fn extract_embedded(dir: &Path, bytes: &[u8], version: &str) -> Result<PathBuf> {
    let dest = dir.join(crate::release::versioned_wasm_name(version));
    if !dest.exists() {
        std::fs::write(&dest, bytes)
            .with_context(|| format!("extracting embedded wasm to {}", dest.display()))?;
    }
    Ok(dest)
}

/// Install the running exe as the versioned CLI copy `<data>/bin/clave-vX.Y.Z`
/// (codex P2 on PR #29, 2026-07-22). Baking current_exe into config/hooks was
/// not enough: `runtime_binary()` — which add/open/the eager launch layout
/// bake into agent-tab commands — keys on this copy's EXISTENCE and fell back
/// to bare `clave`, so a `./clave` single-file install launched fine but every
/// agent tab failed to spawn. Installing the copy converges the single-file
/// install with `just release`'s model: one versioned artifact, every baked
/// reference absolute, and the scp'd file becomes disposable after setup.
/// Write-if-absent for the same reason as the wasm (running-session immunity).
pub fn install_cli_copy(dir: &Path, exe: &Path, version: &str) -> Result<PathBuf> {
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let dest = bin_dir.join(crate::release::versioned_cli_name(version));
    if !dest.exists() {
        std::fs::copy(exe, &dest)
            .with_context(|| format!("installing CLI copy to {}", dest.display()))?;
    }
    Ok(dest)
}

/// `clave setup` — the DEV/sandbox machine prep: generate against bare
/// `clave` (PATH = the dev binary) and the unversioned working-tree wasm.
/// Stable machines are prepared by `just release` (→ `run_release`), which
/// bakes versioned paths instead. Idempotent.
pub fn run_setup() -> Result<()> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    // Fix 7 (review 2026-07-22): extract the embedded wasm FIRST and
    // unconditionally — write-if-absent on the VERSIONED name. Previously we
    // extracted only when wasm_path() reported nothing; but wasm_path() falls
    // back to a stale unversioned clave-bar.wasm, so a leftover sandbox wasm
    // made wasm.exists() true and the release's versioned wasm never landed —
    // config got baked against the stale file. Extracting before resolving
    // both fixes that and removes the old nested branch.
    // Set only on the release single-file path; consumed after write_generated
    // so the launcher is the LAST thing this function creates (see below).
    let mut launcher_src: Option<PathBuf> = None;
    let binary = if let Some(bytes) = crate::release::embedded_wasm() {
        extract_embedded(&dir, bytes, env!("CARGO_PKG_VERSION"))?;
        // Release single-file install: install the versioned CLI copy, then
        // bake through runtime_binary() — the SAME resolution add/open/launch
        // use at tab-bake time, so setup and runtime can never disagree about
        // which binary agent tabs run (codex P2 on PR #29, 2026-07-22).
        // current_exe canonicalized so the copy survives a symlinked invoker.
        match std::env::current_exe().and_then(std::fs::canonicalize).ok() {
            Some(exe) => {
                let copy = install_cli_copy(&dir, &exe, env!("CARGO_PKG_VERSION"))?;
                // #43a: the single-file install owns the unversioned entry
                // point too. install_cli_copy's contract is that "the scp'd
                // file becomes disposable after setup" — that was only ever
                // true of BAKED references; without a launcher the operator
                // still had nothing to type, which is the whole defect.
                //
                // Deferred to AFTER write_generated (adversarial review
                // 2026-07-27) so this path holds the same invariant
                // run_release states one function away: "the launcher must
                // never come to exist for a cut whose generation failed".
                // Installing it here instead left a failed setup — realistically
                // a parse error on a hand-edited settings.json, one `?` below —
                // with bin/clave pointing at the NEW version while config.kdl
                // still described the OLD one. The operator then types `clave`,
                // which is the entire point of #43a, and gets a binary whose
                // launch layout disagrees with its own config: #43 reproduced
                // by the fix for #43.
                launcher_src = Some(copy);
                crate::release::runtime_binary()
            }
            // Unresolvable current_exe: bare `clave` beats refusing setup.
            None => "clave".to_string(),
        }
    } else {
        // Dev/sandbox: bare `clave` deliberately — PATH resolves to the
        // freshly cargo-installed dev binary, which is what should run there.
        "clave".to_string()
    };
    let wasm = wasm_path()?; // prefers the versioned artifact just extracted
    anyhow::ensure!(
        wasm.exists(),
        "{} missing — run `just dev-install` first (it builds the sandbox wasm here)",
        wasm.display()
    );
    let wasm_str = wasm.to_str().context("wasm path")?;
    write_generated(&dir, &binary, wasm_str)?;

    // #43a, LAST — same ordering as run_release (release.rs). Every prefix of
    // this function now leaves <data>/bin/clave naming a version whose
    // config.kdl and versioned copy agree, so a failure anywhere above is
    // recoverable by re-running setup rather than leaving a launcher that
    // points somewhere the generated set does not describe.
    //
    // Refresh semantics mean an OLDER single-file binary's setup repoints the
    // launcher backwards. Accepted: running a specific binary's `setup` is an
    // explicit "install this one", and #48's doctor is where cross-artifact
    // skew gets reported.
    if let Some(copy) = launcher_src {
        crate::release::install_launcher(&dir.join("bin"), &copy)?;
    }
    Ok(())
}

/// Does `zellij list-sessions -n` output show `name` as a LIVE session?
/// EXITED is not live: attaching resurrects a fresh session whose tab_ids
/// restart from scratch.
pub fn session_is_live(list_output: &str, name: &str) -> bool {
    list_output
        .lines()
        .any(|l| l.split_whitespace().next() == Some(name) && !l.contains("EXITED"))
}

/// §6.8 eager-launch selection: the most-recent agent row whose cwd still
/// EXISTS on disk. The cwd-existence filter mirrors `clave open`'s staleness
/// branch (§6.3/§6.8): a deleted worktree as the most-recent row would bake a
/// cold-start tab whose `clave spawn` dies at canonicalize — so skip it and
/// fall through to the next viable row (none viable → None → bar-only layout).
pub fn eager_row(store: &crate::store::Store) -> Option<&crate::store::AgentRecord> {
    store
        .agents
        .values()
        .filter(|r| std::path::Path::new(&r.cwd).is_dir())
        // TIE-BREAK (accepted): last_interacted is second-resolution, so two
        // rows touched in the same second tie. `agents` is a BTreeMap and
        // `max_by_key` keeps the LAST max seen, so ties resolve by uuid order
        // — arbitrary but harmless: both rows are equally "most recent", and
        // whichever loses is a live bar row a keystroke away. No wall-clock
        // sub-second precision is worth carrying to make this deterministic.
        .max_by_key(|r| r.last_interacted)
}

/// First-run consent (spec §First run): the plan prints ALWAYS; the prompt
/// fires only on a TTY — never prompt without one (Homebrew 2026). Pure
/// over (tty, read line) so the gate is unit-testable.
pub fn confirm_proceed(is_tty: bool, input: Option<&str>) -> bool {
    if !is_tty {
        return true; // invoking `clave` IS the named intent; setup is idempotent
    }
    matches!(
        input.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("") | Some("y") | Some("yes") | None
    )
}

pub fn first_run_plan(data_dir: &Path, settings_path: &Path) -> String {
    format!(
        "First run — clave needs to prepare this machine:\n\
         \n\
         \x20 • generate session config + layout in {}\n\
         \x20 • register status hooks in {} (additive — your existing hooks are never touched)\n\
         \x20 • pre-seed Zellij's plugin permission cache\n",
        data_dir.display(),
        settings_path.display()
    )
}

/// Should launch re-run setup because a release binary was UPGRADED (review
/// 2026-07-22, Fix 3)? launch_session only ran setup on a MISSING config, so
/// an upgraded release binary (new version, embedded wasm) with existing
/// config never extracted its new versioned wasm and kept launching the OLD
/// bar — exactly the CLI/bar drift invariant #9 exists to kill. True only
/// when config already exists (first run is the other branch's job), this is
/// a release build, and THIS version's wasm has not been extracted yet.
/// Running-session immunity holds automatically: live sessions reference the
/// old files baked into their config, untouched by re-running setup.
pub fn needs_version_refresh(
    config_exists: bool,
    has_embedded: bool,
    versioned_wasm_exists: bool,
) -> bool {
    config_exists && has_embedded && !versioned_wasm_exists
}

/// Bare `clave`: attach-or-create the dedicated session with OUR config +
/// a DYNAMIC layout (§6.8 C8: eager most-recent tab; serialization is off,
/// so a dead session is deleted, never resurrected).
pub fn launch_session() -> Result<()> {
    // Preflight BEFORE anything (spec §Preflight): zellij because we exec
    // it; claude because the eager tab's spawn would otherwise fail INSIDE
    // a pane — the worst place to read an error.
    crate::doctor::preflight(
        &[
            crate::discover::ToolId::Zellij,
            crate::discover::ToolId::Claude,
        ],
        "clave can't start — missing required tools:",
    )?;
    let dir = data_dir()?;
    let config = dir.join("config.kdl");
    let config_exists = config.exists();
    if !config_exists {
        // First run (spec §First run): plan → TTY-gated consent → setup.
        use std::io::IsTerminal;
        let settings = crate::env::claude_config_dir()?.join("settings.json");
        println!("{}", first_run_plan(&dir, &settings));
        let is_tty = std::io::stdin().is_terminal();
        let input = if is_tty {
            print!("Proceed? [Y/n] ");
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            Some(line)
        } else {
            None
        };
        anyhow::ensure!(
            confirm_proceed(is_tty, input.as_deref()),
            "aborted — run `clave setup` when ready"
        );
        run_setup()?;
    } else {
        // Upgrade-refresh (review 2026-07-22, Fix 3): a release binary whose
        // config exists but whose THIS-version wasm is not yet extracted was
        // just upgraded — re-run setup so the new versioned wasm lands and the
        // bar can never go stale. Idempotent, no consent prompt (the user
        // consented at first run; this is the same mutation set).
        let versioned = dir.join(crate::release::versioned_wasm_name(env!(
            "CARGO_PKG_VERSION"
        )));
        if needs_version_refresh(
            config_exists,
            crate::release::embedded_wasm().is_some(),
            versioned.exists(),
        ) {
            println!(
                "clave {}: refreshing setup for this version",
                env!("CARGO_PKG_VERSION")
            );
            run_setup()?;
        }
    }
    // Discovered once, used for every zellij invocation in this launch —
    // an off-PATH zellij (e.g. ~/.cargo/bin over SSH) still works (spec
    // §Discovery: found off-PATH ⇒ use the absolute path).
    let zellij = crate::discover::discover(crate::discover::ToolId::Zellij)
        .map(|d| d.path)
        .unwrap_or_else(|| std::path::PathBuf::from("zellij")); // preflight guarantees Some
    let session = crate::env::session_name();
    let list = std::process::Command::new(&zellij)
        .args(["list-sessions", "-n"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let live = session_is_live(&list, &session);
    if !live {
        // §6.6 hygiene: tab_ids are SESSION-scoped — drop the previous
        // session's timeline + binds before a CREATE.
        crate::store::clear_session_order(&crate::store::store_paths()?)?;
        if session_exists(&list, &session) {
            // Dead-but-serialized (pre-C8 state, or zellij's own cache):
            // delete so attach --create builds from OUR layout. Best-effort —
            // launch must NOT die on a cleanup failure — but a SILENT failure
            // would let attach resurrect the exact pre-C8 serialized state C8
            // kills, invisibly. So capture the status and log any failure.
            match std::process::Command::new(&zellij)
                .args(["delete-session", "--force", &session])
                .status()
            {
                Ok(s) if s.success() => {}
                Ok(s) => crate::evlog::log_event(
                    "launch",
                    &format!(
                        "delete-session {session} exited {s} — attach may resurrect pre-C8 state"
                    ),
                ),
                Err(e) => crate::evlog::log_event(
                    "launch",
                    &format!(
                        "delete-session {session} did not run: {e} — attach may resurrect pre-C8 state"
                    ),
                ),
            }
        }
    }
    // Compose the launch layout from the store (eager most-recent, §6.8).
    // Harmless when live (attach ignores --layout for an existing session).
    let store = crate::store::read_store(&crate::store::store_paths()?)?;
    let most_recent = eager_row(&store);
    // Guard the eager row's cwd before it's baked into the launch layout
    // (add::validate_cwd) — a `"`/control char would emit malformed KDL and
    // the whole session would fail to create.
    if let Some(r) = most_recent {
        crate::add::validate_cwd(&r.cwd)?;
    }
    let wasm = wasm_path()?;
    // Bake the environment's clave into the eager tab's spawn: the versioned
    // copy's absolute path in a stable session (immune to a newer PATH
    // `clave`), bare `clave` in the dev/sandbox one (§2 binary split).
    let binary = crate::release::runtime_binary();
    // `store.collapsed` is LOAD-BEARING here, not decoration (D36): the mode
    // persists across a launch, so a fleet left collapsed must be BORN at 30 or
    // it arrives at 54 and visibly shrinks — the jank D35 exists to remove.
    let layout_text = launch_layout_kdl(
        &binary,
        wasm.to_str().context("wasm path")?,
        most_recent,
        store.collapsed,
    );
    // STABLE path in the data dir, not a pid-suffixed temp file: the exec()
    // below never returns, so nothing here can clean up — a unique-per-launch
    // file would leak one KDL forever. Overwrite the one file each launch.
    let layout = launch_layout_path(&dir);
    std::fs::write(&layout, layout_text)?;
    crate::evlog::log_event(
        "launch",
        &format!(
            "session={session} live={live} eager={:?}",
            most_recent.map(|r| r.uuid.as_str())
        ),
    );
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(&zellij)
        .arg("--config")
        .arg(&config)
        .arg("--layout")
        .arg(&layout)
        .args(["attach", "--create", &session])
        .exec();
    Err(anyhow::anyhow!("exec zellij failed: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_is_live_reads_zellij_list_sessions() {
        // `zellij list-sessions -n` lines: `<name> [Created …]`, with
        // ` (EXITED - attach to resurrect)` appended for dead sessions.
        // EXITED counts as NOT live: attach resurrects a FRESH session whose
        // tab_ids restart, so the stale timeline must be cleared then too.
        let out = "chat [Created 5h ago]\nclave [Created 2m ago]\n";
        assert!(session_is_live(out, "clave"));
        let out = "clave [Created 2h ago] (EXITED - attach to resurrect)\n";
        assert!(!session_is_live(out, "clave"));
        // No sessions at all (list-sessions exits non-zero → empty string).
        assert!(!session_is_live("", "clave"));
        // Name must match the whole first token, not a prefix.
        assert!(!session_is_live("clave-dev [Created 1m ago]\n", "clave"));
    }

    #[test]
    fn generated_bar_panes_are_percent_sized_not_fixed() {
        // zellij 0.44.3 REFUSES resize_pane_with_id on fixed-size panes
        // (CantResizeFixedPanes, tiled_pane_grid.rs) — a `size=30` bar can
        // never collapse (Alt+c dead, live finding c8-cold-start
        // 2026-07-18). Percent sizes are flexible; the bar's birth-armed
        // width seek converges to the exact template cols. Historically
        // masked: pre-C8 sessions were resurrected from zellij's serialized
        // cache, which rewrites sizes as percentages.
        for kdl in [
            layout_kdl("clave", "/w.wasm"),
            launch_layout_kdl_for("clave", "/w.wasm", None, None, false),
            crate::add::tab_layout("clave", "/w.wasm", "l", "u", "/c", None, false),
        ] {
            // All THREE generators carry the one derived percent (S8 §3.3):
            // `BAR_BIRTH_PERCENT` is `BAR_TARGET_COLS` against §3.4's
            // 200-column reference viewport, so a newborn bar is essentially
            // on target and does not flicker its way there. Formatted from the
            // constant, not restated as `22%` — a restatement is exactly the
            // skew this hoist removed, and `clave-types`'
            // `birth_percent_is_derived_from_the_bar_target` pins the value
            // itself in the one place it is defined.
            let want = format!("size=\"{}%\"", clave_types::BAR_BIRTH_PERCENT);
            assert!(
                kdl.contains(&want),
                "bar pane must be percent-sized at {want}:\n{kdl}"
            );
            assert!(
                !kdl.contains("size=30"),
                "fixed size resurrects the FIXED! bug:\n{kdl}"
            );
        }
    }

    #[test]
    fn launch_layout_path_is_stable_and_in_data_dir() {
        // Fix: launch_session exec()s zellij and never returns, so no cleanup
        // runs — a pid-suffixed temp path leaked one KDL per launch. The path
        // must be STABLE (same every launch → overwrite, no accumulation) and
        // live in the data dir beside config.kdl/layout.kdl.
        let dir = std::path::Path::new("/data/clave");
        let p = launch_layout_path(dir);
        assert_eq!(p, dir.join("launch.kdl"));
        assert_eq!(launch_layout_path(dir), p); // deterministic, no pid suffix
        assert!(
            !p.to_string_lossy()
                .contains(&std::process::id().to_string())
        );
    }

    /// LEDGER D35 — the launch percent comes from the REAL terminal.
    ///
    /// Every other test passes `None` for the width, so they take the fallback
    /// deliberately and prove nothing about the behaviour that matters — they
    /// would keep passing if the width were ignored entirely. This one supplies
    /// real widths so the derivation is actually exercised.
    ///
    /// **No test may call `launch_layout_kdl` itself** (PR #90, Codex P1): the
    /// wrapper reads the TTY, so a percent assertion behind it passes under
    /// `cargo test` — no TTY — and fails from Ollie's interactive zellij shell,
    /// where 142 columns emits 38% and not `BAR_BIRTH_PERCENT`. The
    /// parameterised form exists precisely so the gate is width-independent.
    #[test]
    fn the_launch_percent_is_derived_from_the_real_terminal_width() {
        // 54/280 = 19.29%, not expressible; 19% floors to 53, inside the band.
        let wide = launch_layout_kdl_for("clave", "/w.wasm", None, Some(280), false);
        assert!(wide.contains("size=\"19%\""), "{wide}");
        // 54/120 = 45% exactly.
        let narrow = launch_layout_kdl_for("clave", "/w.wasm", None, Some(120), false);
        assert!(narrow.contains("size=\"45%\""), "{narrow}");
        // The two must actually DIFFER, or the parameter is decorative — the
        // shape a single-width assertion cannot see.
        assert_ne!(
            wide, narrow,
            "the display width did not reach the emitted layout"
        );
        // D36, found live: the collapse mode PERSISTS across a launch, so the
        // birth must be computed against the target the bar will actually seek.
        // On the 95-column window this was found on, the expanded computation
        // put 57% of the terminal under the sidebar before it shrank away.
        let expanded_95 = launch_layout_kdl_for("clave", "/w.wasm", None, Some(95), false);
        let collapsed_95 = launch_layout_kdl_for("clave", "/w.wasm", None, Some(95), true);
        assert!(expanded_95.contains("size=\"57%\""), "{expanded_95}");
        assert!(collapsed_95.contains("size=\"32%\""), "{collapsed_95}");
        assert_ne!(
            expanded_95, collapsed_95,
            "the collapse mode did not reach the emitted layout"
        );

        // No TTY (a script, CI): the 200-column fiction, still a valid layout.
        let headless = launch_layout_kdl_for("clave", "/w.wasm", None, None, false);
        assert!(
            headless.contains(&format!("size=\"{}%\"", clave_types::BAR_BIRTH_PERCENT)),
            "{headless}"
        );
    }

    #[test]
    fn launch_layout_is_bar_only_when_store_empty() {
        // §6.8 cold start, empty store: today's behavior — template + one
        // plain tab, no agent tabs.
        let kdl = launch_layout_kdl_for("clave", "/w.wasm", None, None, false);
        assert!(kdl.contains("default_tab_template"));
        assert!(kdl.contains("tab name=\"clave\" focus=true"));
        assert!(!kdl.contains("\"spawn\""));
    }

    #[test]
    fn launch_layout_eager_loads_only_the_most_recent_row() {
        // §6.8: eagerness of exactly ONE — the most-recent agent resumes
        // focused at launch; every other row stays dormant in the bar.
        let mut r = crate::store::AgentRecord {
            uuid: "u-recent".into(),
            cwd: "/repo/.claude-worktrees/ab".into(), // worktree row: bake ITS cwd
            repo_root: "/repo".into(),
            branch: "main".into(),
            label: "repo · main".into(),
            status: clave_types::Status::Idle,
            last_interacted: 100,
            commit_ord: 0,
            last_visited: 0,
            worktree: Some("/repo/.claude-worktrees/ab".into()),
            label_source: crate::store::LabelSource::FirstPrompt,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
        };
        let kdl = launch_layout_kdl_for("clave", "/w.wasm", Some(&r), None, false);
        assert!(kdl.contains("default_tab_template")); // native new-tabs still barred
        assert!(kdl.contains("\"spawn\" \"u-recent\""));
        assert!(kdl.contains("cwd=\"/repo/.claude-worktrees/ab\""));
        // The eager tab replaces the plain placeholder tab entirely.
        assert!(!kdl.contains("tab name=\"clave\" focus=true"));
        // DOUBLE-BAR regression guard (live finding, 2026-07-18): the
        // template wraps explicit tab nodes too, so the ONLY bar pane in
        // the whole layout must be the template's — the eager tab node is
        // BARE.
        assert_eq!(kdl.matches("plugin location").count(), 1);
        r.label = "x".into(); // silence unused-mut if needed
    }

    #[test]
    fn eager_row_skips_rows_whose_cwd_vanished() {
        // §6.8: the most-recent row is only viable if its cwd still EXISTS —
        // a deleted worktree as most-recent would bake a tab whose spawn dies
        // at canonicalize. Skip it, fall through to the next viable row.
        use crate::store::{AgentRecord, LabelSource, Store};
        let live_dir = std::env::temp_dir().join(format!("clave-eager-{}", std::process::id()));
        std::fs::create_dir_all(&live_dir).unwrap();
        let mk = |uuid: &str, cwd: &str, li: u64| AgentRecord {
            uuid: uuid.into(),
            cwd: cwd.into(),
            repo_root: String::new(),
            branch: String::new(),
            label: uuid.into(),
            status: clave_types::Status::Idle,
            last_interacted: li,
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
        };
        // Most-recent row's cwd is GONE; the older row's cwd exists.
        let mut store = Store::default();
        store
            .agents
            .insert("gone".into(), mk("gone", "/no/such/clave/eager/dir", 200));
        store
            .agents
            .insert("live".into(), mk("live", live_dir.to_str().unwrap(), 100));
        assert_eq!(eager_row(&store).map(|r| r.uuid.as_str()), Some("live"));
        // Every row's cwd missing → None → bar-only layout.
        let mut none = Store::default();
        none.agents
            .insert("gone".into(), mk("gone", "/no/such/clave/eager/dir", 200));
        assert!(eager_row(&none).is_none());
        let _ = std::fs::remove_dir_all(&live_dir);
    }

    #[test]
    fn session_exists_vs_live_distinguish_exited() {
        let out = "clave [Created 2h ago] (EXITED - attach to resurrect)\nother [Created 1m ago]\n";
        assert!(session_exists(out, "clave"));
        assert!(!session_is_live(out, "clave")); // existing fn, unchanged
        assert!(!session_exists(out, "missing"));
    }

    #[test]
    fn hooks_merge_is_additive_and_idempotent() {
        // Existing user hook MUST survive (§6.5: never clobber).
        let mut v: serde_json::Value = serde_json::json!({
            "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "my-bell" } ] } ] }
        });
        assert!(merge_hooks(&mut v, "clave"));
        let stops = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stops.len(), 2); // user's + ours
        assert_eq!(stops[0]["hooks"][0]["command"], "my-bell");
        assert_eq!(stops[1]["hooks"][0]["command"], "clave hook Stop");
        // Every event we need is registered.
        for ev in ["UserPromptSubmit", "Notification", "SessionEnd"] {
            assert!(v["hooks"][ev].as_array().is_some(), "{ev} missing");
        }
        // Second run: no change, no duplicates.
        assert!(!merge_hooks(&mut v, "clave"));
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn permissions_merge_seeds_both_key_forms_and_preserves_others() {
        let existing = "\"file:/other/plugin.wasm\" {\n    ReadCliPipes\n}\n";
        let merged = merge_permissions_kdl(existing, "/data/clave-bar.wasm");
        // Other plugin untouched:
        assert!(merged.contains("/other/plugin.wasm"));
        // Both key forms present (S1/S2: key form matters):
        assert!(merged.contains("\"file:/data/clave-bar.wasm\""));
        assert!(merged.contains("\"/data/clave-bar.wasm\""));
        // The EXACT §6.6 set under each:
        for p in BAR_PERMISSIONS {
            assert!(
                merged.matches(p).count() >= 2,
                "{p} missing from a key form"
            );
        }
        // Idempotent: re-merging replaces our blocks, not duplicates them.
        let again = merge_permissions_kdl(&merged, "/data/clave-bar.wasm");
        assert_eq!(again.matches("file:/data/clave-bar.wasm").count(), 1);
    }

    #[test]
    fn permissions_seeded_detects_our_grant() {
        let seeded = merge_permissions_kdl("", "/data/clave-bar.wasm");
        assert!(permissions_seeded(&seeded, "/data/clave-bar.wasm"));
        assert!(!permissions_seeded("", "/data/clave-bar.wasm"));
        assert!(!permissions_seeded(&seeded, "/other/clave-bar.wasm"));
    }

    #[test]
    fn config_disables_session_serialization() {
        // §6.8 (2026-07-17, C8): resurrection is clave-owned; a serialized
        // session would replay discovered `claude --session-id` commands
        // (create-collision) or mid-tool-call children (pty.rs ppid-priority
        // discovery, v0.44.3 source-verified).
        let kdl = config_kdl("clave", "/w.wasm");
        assert!(kdl.contains("session_serialization false"));
    }

    #[test]
    fn config_unbinds_claude_code_colliding_keys() {
        // #28: clear-defaults=false keeps stock zellij binds, which SWALLOW
        // keys Claude Code needs — Ctrl+g (locked-mode toggle vs
        // read-input-field), Ctrl+t (tab mode vs todo view), Ctrl+o (session
        // mode vs verbose output), Ctrl+b (tmux mode vs background command) —
        // plus Ctrl+q (stock Quit: one stray press kills the whole fleet).
        // The verified-correct form (zellij-utils 0.44.3 kdl/mod.rs:4600) is a
        // SINGLE top-level `unbind` node inside `keybinds`, sibling to
        // shared_among — `children().get("unbind")` fetches it and
        // unbind_keys_in_all_modes (kdl/mod.rs:4627) strips each key from every
        // mode after the merge-over-defaults. kdl 4.7.1 KdlDocument::get
        // (document.rs:80) returns the FIRST match only, so ALL keys must ride
        // that one node — two `unbind` nodes would silently drop the second.
        let cfg = config_kdl("clave", "/w.wasm");
        assert_eq!(
            cfg.matches("unbind").count(),
            1,
            "keys must ride exactly ONE unbind node (get() takes the first only):\n{cfg}"
        );
        for key in ["Ctrl g", "Ctrl t", "Ctrl o", "Ctrl b", "Ctrl q"] {
            assert!(
                cfg.contains(&format!("\"{key}\"")),
                "{key} not unbound:\n{cfg}"
            );
        }
    }

    #[test]
    fn generated_kdl_carries_the_wasm_path_and_alt_keys() {
        let cfg = config_kdl("clave", "/data/clave-bar.wasm");
        for key in [
            "Alt a",
            "Alt c",
            "Alt t",
            "Alt w",
            "Alt j",
            "Alt k",
            "Alt 1",
            "Alt 9",
            "Alt Enter",
        ] {
            assert!(
                cfg.contains(&format!("bind \"{key}\"")) || cfg.contains(&format!("\"{key}\"")),
                "{key} unbound"
            );
        }
        assert!(cfg.contains("shared_among \"normal\" \"locked\"")); // invariant #6
        assert!(cfg.contains("clave-nav") && cfg.contains("clave-toggle"));
        let lay = layout_kdl("clave", "/data/clave-bar.wasm");
        assert!(lay.contains("default_tab_template"));
        assert!(lay.contains("file:/data/clave-bar.wasm"));
        // Regression (Task 9 C1): without the vertical wrapper the bar is a
        // 30-ROW strip on top, not a 30-col LEFT column.
        assert!(lay.contains("split_direction=\"vertical\""));
    }

    #[test]
    fn generation_bakes_the_binary_passed_by_the_caller() {
        // §2 binary split: stable bakes the versioned copy's ABSOLUTE path,
        // sandbox keeps bare `clave` (PATH = the dev binary). The generation
        // fns are pure over the binary — the caller's environment decides.
        let abs = "/home/o/.local/share/clave/bin/clave-v0.1.0";
        let cfg = config_kdl(abs, "/w.wasm");
        // The keybind `Run` invokes the passed binary, not bare `clave`.
        assert!(cfg.contains(&format!("Run \"{abs}\" \"add\"")));
        assert!(!cfg.contains("Run \"clave\" \"add\""));
        // Sandbox generation keeps bare `clave`.
        assert!(config_kdl("clave", "/w.wasm").contains("Run \"clave\" \"add\""));
        // The launch layout's eager-tab spawn bakes the same binary as its
        // pane command (resurrection re-execs the SAME clave).
        let r = crate::store::AgentRecord {
            uuid: "u".into(),
            cwd: "/c".into(),
            repo_root: "/c".into(),
            branch: "main".into(),
            label: "l".into(),
            status: clave_types::Status::Idle,
            last_interacted: 0,
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: crate::store::LabelSource::FirstPrompt,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
        };
        let lay = launch_layout_kdl_for(abs, "/w.wasm", Some(&r), None, false);
        assert!(lay.contains(&format!("command=\"{abs}\"")));
        assert!(!lay.contains("command=\"clave\""));
    }

    #[test]
    fn is_clave_hook_command_matches_our_paths_not_foreign() {
        // Bare PATH clave, an absolute versioned copy, and an older version's
        // path all count as OURS (replace-on-version applies).
        assert!(is_clave_hook_command("clave hook Stop", "Stop"));
        assert!(is_clave_hook_command(
            "/home/o/.local/share/clave/bin/clave-v0.1.0 hook Stop",
            "Stop"
        ));
        assert!(is_clave_hook_command(
            "/opt/clave-v0.0.9 hook Notification",
            "Notification"
        ));
        // Wrong event, or a foreign tool, or a non-hook command: NOT ours.
        assert!(!is_clave_hook_command("clave hook Stop", "Notification"));
        assert!(!is_clave_hook_command("my-bell", "Stop"));
        assert!(!is_clave_hook_command("/x/notify hook Stop", "Stop"));
        assert!(!is_clave_hook_command("clave add", "Stop"));
        // A foreign tool merely PREFIXED with "clave-v" (clave-vault,
        // clave-verify) is NOT ours — the basename check must require a
        // DIGIT immediately after "clave-v", not just the substring.
        assert!(!is_clave_hook_command("clave-vault hook Stop", "Stop"));
        assert!(!is_clave_hook_command(
            "/usr/local/bin/clave-verify hook Stop",
            "Stop"
        ));
    }

    #[test]
    fn merge_hooks_dedupes_duplicate_clave_entries_keeping_one() {
        // Fix 4 (review 2026-07-22): merge_hooks rewrote EVERY matching clave
        // hook in place but never removed duplicates — Claude fires ALL
        // matching hooks, so two clave Stop entries double-fire. Per event,
        // keep exactly ONE clave hook (rewrite the first, drop the rest);
        // never touch a foreign hook in the same array.
        let mut v: serde_json::Value = serde_json::json!({
            "hooks": {
                "Stop": [
                    { "hooks": [ { "type": "command", "command": "clave hook Stop" } ] },
                    { "hooks": [ { "type": "command", "command": "/old/path/clave-v0.0.9 hook Stop" } ] },
                    { "hooks": [ { "type": "command", "command": "my-bell hook Stop" } ] }
                ]
            }
        });
        assert!(merge_hooks(&mut v, "clave")); // changed: the duplicate was removed
        let stops = v["hooks"]["Stop"].as_array().unwrap();
        // Exactly one surviving clave Stop entry, at the current command.
        let clave: Vec<_> = stops
            .iter()
            .filter(|e| {
                e["hooks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|h| is_clave_hook_command(h["command"].as_str().unwrap_or(""), "Stop"))
            })
            .collect();
        assert_eq!(clave.len(), 1);
        assert_eq!(clave[0]["hooks"][0]["command"], "clave hook Stop");
        // The foreign my-bell entry survives untouched.
        assert!(
            stops
                .iter()
                .any(|e| e["hooks"][0]["command"] == "my-bell hook Stop")
        );
        // Idempotent second run: nothing left to dedupe.
        assert!(!merge_hooks(&mut v, "clave"));
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn merge_hooks_leaves_a_foreign_clave_v_prefixed_hook_untouched() {
        // Regression: `clave-vault`/`clave-verify` share the "clave-v" prefix
        // with our versioned binary name (clave-vN.N.N) but are unrelated
        // tools — merge_hooks must never absorb or rewrite their entry.
        let mut v: serde_json::Value = serde_json::json!({
            "hooks": {
                "Stop": [
                    { "hooks": [ { "type": "command", "command": "clave-vault hook Stop" } ] }
                ]
            }
        });
        assert!(merge_hooks(&mut v, "clave")); // registers ours fresh, doesn't replace theirs
        let stops = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stops.len(), 2); // their entry + ours, not a rewrite
        assert_eq!(stops[0]["hooks"][0]["command"], "clave-vault hook Stop"); // untouched
        assert_eq!(stops[1]["hooks"][0]["command"], "clave hook Stop");
    }

    #[test]
    fn extract_embedded_is_write_if_absent() {
        let dir = tempfile::tempdir().unwrap();
        let p = extract_embedded(dir.path(), b"wasm-bytes", "0.1.0").unwrap();
        assert_eq!(p.file_name().unwrap(), "clave-bar-v0.1.0.wasm");
        assert_eq!(std::fs::read(&p).unwrap(), b"wasm-bytes");
        // Second call must NOT rewrite — a live session's loaded wasm is
        // never touched (§2 running-session immunity).
        extract_embedded(dir.path(), b"DIFFERENT", "0.1.0").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"wasm-bytes");
    }

    #[test]
    fn needs_version_refresh_only_when_release_wasm_is_stale() {
        // Fix 3 (review 2026-07-22): an upgraded release binary with existing
        // config must re-extract its NEW versioned wasm, else it launches the
        // OLD bar forever (invariant #9 drift). True ONLY for
        // (config present, embedded, this version's wasm absent).
        assert!(needs_version_refresh(true, true, false));
        // Wasm already current → no refresh (idempotent, no needless setup).
        assert!(!needs_version_refresh(true, true, true));
        // Dev build (no embedded wasm) → the sandbox flow owns wasm placement.
        assert!(!needs_version_refresh(true, false, false));
        // First run (no config) is handled by the other branch, never here.
        assert!(!needs_version_refresh(false, true, false));
        assert!(!needs_version_refresh(false, false, false));
        assert!(!needs_version_refresh(false, true, true));
    }

    #[test]
    fn install_cli_copy_is_write_if_absent_under_bin() {
        // Codex P2 (PR #29): the versioned CLI copy is what runtime_binary()
        // keys on — a single-file install must create it or every agent tab
        // bakes bare `clave`. Write-if-absent mirrors the wasm extraction
        // (running-session immunity).
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("clave-download");
        std::fs::write(&exe, b"binary-v1").unwrap();
        let dest = install_cli_copy(dir.path(), &exe, "0.1.0").unwrap();
        assert_eq!(dest, dir.path().join("bin").join("clave-v0.1.0"));
        assert_eq!(std::fs::read(&dest).unwrap(), b"binary-v1");
        // Second install with different content must NOT rewrite.
        std::fs::write(&exe, b"binary-v2").unwrap();
        install_cli_copy(dir.path(), &exe, "0.1.0").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"binary-v1");
    }

    #[test]
    fn confirm_proceed_tty_gating_and_default_yes() {
        // Never prompt without a TTY (Homebrew 2026 rule) → proceed.
        assert!(confirm_proceed(false, None));
        // TTY: empty/y/Y/yes proceed; n/no abort.
        for yes in ["", "y", "Y", "yes", " y "] {
            assert!(confirm_proceed(true, Some(yes)), "{yes:?}");
        }
        for no in ["n", "N", "no", "nope"] {
            assert!(!confirm_proceed(true, Some(no)), "{no:?}");
        }
    }

    #[test]
    fn first_run_plan_names_the_three_mutations() {
        let s = first_run_plan(
            Path::new("/home/u/.local/share/clave"),
            Path::new("/home/u/.claude/settings.json"),
        );
        assert!(s.contains("First run"));
        assert!(s.contains("/home/u/.local/share/clave"));
        assert!(s.contains("/home/u/.claude/settings.json"));
        assert!(s.contains("additive"));
        assert!(s.contains("permission cache"));
    }

    #[test]
    fn merge_hooks_replaces_a_prior_version_in_place() {
        // §2 replace-on-version-change: a version cut rewrites the existing
        // clave hook command to the new path — never a duplicate, never the
        // stale one left behind, and a user's own hook untouched.
        let old = "/home/o/.local/share/clave/bin/clave-v0.1.0";
        let new = "/home/o/.local/share/clave/bin/clave-v0.2.0";
        let mut v: serde_json::Value = serde_json::json!({
            "hooks": {
                "Stop": [
                    { "hooks": [ { "type": "command", "command": "my-bell" } ] },
                    { "hooks": [ { "type": "command", "command": format!("{old} hook Stop") } ] }
                ]
            }
        });
        assert!(merge_hooks(&mut v, new)); // changed
        let stops = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stops.len(), 2); // user's + ours, NOT three
        assert_eq!(stops[0]["hooks"][0]["command"], "my-bell"); // foreign untouched
        assert_eq!(stops[1]["hooks"][0]["command"], format!("{new} hook Stop"));
        // The other events were absent → freshly registered at the new path.
        assert_eq!(
            v["hooks"]["Notification"][0]["hooks"][0]["command"],
            format!("{new} hook Notification")
        );
        // Same-version re-run: idempotent (no change, no duplicate).
        assert!(!merge_hooks(&mut v, new));
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
    }

    /// Pull the `X.Y.Z` out of a `clave-bar-vX.Y.Z.wasm` or `clave-vX.Y.Z`
    /// basename. Test-only: it exists to let
    /// `generated_artifact_set_is_version_coherent` compare versions found
    /// in generated KDL text, not to be a general path parser. Returns None
    /// for a bare/unversioned reference (dev's `"clave"`, an unversioned
    /// `clave-bar.wasm`) — those carry no version to compare, so the caller
    /// only compares the `Some(_)` results.
    fn extract_version(path: &str) -> Option<String> {
        let name = std::path::Path::new(path).file_name()?.to_str()?;
        let v = match name.strip_prefix("clave-bar-v") {
            Some(rest) => rest.strip_suffix(".wasm")?,
            None => name.strip_prefix("clave-v")?,
        };
        // Well-formed guard: must be dot-separated all-digit segments
        // (rejects "clave-vault"/"clave-verify" style false positives, and
        // any truncated/malformed match) — same shape as
        // is_clave_hook_command's digit-after-prefix check above.
        let well_formed = !v.is_empty()
            && v.split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        well_formed.then(|| v.to_string())
    }

    /// Every distinct version referenced in `text` via a
    /// `clave-bar-vX.Y.Z.wasm` or `clave-vX.Y.Z` path. Tokenizes on
    /// everything that can't appear inside a bare path (KDL's quotes,
    /// braces, the `file:` scheme colon, whitespace) so `"file:/d/clave-
    /// bar-v0.1.1.wasm"` and `Run "/d/bin/clave-v0.1.1" "add"` both yield
    /// their embedded version.
    fn versions_in(text: &str) -> std::collections::BTreeSet<String> {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/')))
            .filter_map(extract_version)
            .collect()
    }

    #[test]
    fn generated_artifact_set_is_version_coherent() {
        // Regression for the #43/#44 field incident (2026-07-22): a stale
        // dev binary — still built at 0.1.0 — ran the cold start and wrote
        // launch.kdl baking `clave-bar-v0.1.0.wasm`/`clave-v0.1.0`, while
        // `just release` had already regenerated config.kdl/layout.kdl at
        // v0.1.1. Zellij keys plugin IDENTITY on file location, so the two
        // wasm paths loaded as two SEPARATE plugin instances — a duplicate
        // sidebar in every tab and dead navigation (no shared beacon/pipe
        // state, #43). Nothing checked that config_kdl/layout_kdl/
        // launch_layout_kdl agreed on a version before this test; it's a
        // pure-function, hermetic check (no filesystem — generation is pure
        // over its (binary, wasm) args per write_generated's doc comment) so
        // a mismatched caller is caught at test time, not in production.
        //
        // Does NOT cover: two different BINARIES generating different files
        // across separate runs (e.g. a dev cargo-install racing a release
        // install) — that's a runtime/doctor check, tracked as #47.
        let wasm = "/data/clave/clave-bar-v0.1.1.wasm";
        let binary = "/data/clave/bin/clave-v0.1.1";
        let r = crate::store::AgentRecord {
            uuid: "u".into(),
            cwd: "/c".into(),
            repo_root: "/c".into(),
            branch: "main".into(),
            label: "l".into(),
            status: clave_types::Status::Idle,
            last_interacted: 0,
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: crate::store::LabelSource::FirstPrompt,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
        };
        let cfg = config_kdl(binary, wasm);
        let lay = layout_kdl(binary, wasm);
        // The launch layout is composed at launch time and takes the eager
        // agent row — synthesize one so the eager-tab's baked `command=`
        // (the version-bearing binary reference) is present to check too.
        let launch = launch_layout_kdl_for(binary, wasm, Some(&r), None, false);

        // Check PER ARTIFACT, not over the union (Codex, PR #52): flattening
        // first would let an artifact that lost its versioned reference
        // entirely slip through, because the other two still contribute the
        // same singleton. That regression is not benign — an unversioned
        // `clave-bar.wasm` beside a versioned one is a DIFFERENT FILE, and
        // zellij keys plugin identity on location, so it produces the exact
        // two-plugin split this test exists to prevent. Each artifact must
        // therefore carry a version, and they must all agree.
        let artifacts = [("config", &cfg), ("layout", &lay), ("launch", &launch)];
        let mut agreed: Option<String> = None;
        for (name, text) in artifacts {
            let found = versions_in(text);
            assert_eq!(
                found.len(),
                1,
                "{name}.kdl must carry exactly ONE version reference \
                 (an unversioned or mixed path loads a second, independent \
                 plugin instance — #43/#44), found {found:?}:\n{text}"
            );
            let v = found.into_iter().next().expect("checked len == 1");
            match &agreed {
                None => agreed = Some(v),
                Some(prev) => assert_eq!(
                    &v, prev,
                    "generated artifact set must agree on ONE version \
                     (#43/#44: the field incident was launch.kdl at v0.1.0 \
                     while config/layout were v0.1.1); {name}.kdl disagrees:\
                     \nconfig={cfg}\nlayout={lay}\nlaunch={launch}"
                ),
            }
        }
        // Vacuity guard: the loop above only proves agreement if it ran.
        assert!(agreed.is_some(), "no versioned reference found at all");
    }
}
