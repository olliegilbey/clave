# Handoff — #114: whose session is this (config passthrough + keymap legibility)

## Task Overview

Resolve **[#114](https://github.com/olliegilbey/clave/issues/114)** — *"Whose session is this — restore the user's zellij config, and make clave's keybinds legible"*. Open, unassigned, **zero open blockers**, on the wayfinder map [#115](https://github.com/olliegilbey/clave/issues/115) for v0.1.3. Labels: `bug`, `enhancement`, `bar`, `wayfinder:grilling`.

It carries three things under one question:

1. **clave discards the user's zellij config** — fix by moving clave's overlay into the layout file and dropping `--config`.
2. **The zellij bar** — decide whether clave's generated layout carries `tab-bar` / `status-bar` panes.
3. **clave's keybinds must be visible** — clave-bar renders its own hint row from `ModeUpdate`. Absorbs #110 part 3 (the keybind-ownership table).

**Success:** part 1 designed and built (it is the load-bearing half and the one with a verified route); parts 2 and 3 designed, built if scope allows. Ticket type is `wayfinder:grilling` — **the design conversation comes first**, then the build. The map carries execution, so build tickets close by merged PR.

**Constraint from the maintainer:** part 1's known cost (below) is accepted as a documented limitation; the `rebind_keys` repair is wanted but explicitly **not** a precondition.

## Reference Docs

Read #114's body first — it is long and current, and it is the spec. Then, only as needed:

- **`docs/superpowers/research/2026-07-31-zellij-config-passthrough.md`** — the full #122 research, 365 lines. **Not on `main`** — it lives on branch `research/zellij-config-passthrough` (`git show research/zellij-config-passthrough:docs/superpowers/research/2026-07-31-zellij-config-passthrough.md`). Q3's section is the node-by-node merge rules, only needed if the layout route is abandoned.
- **[#122](https://github.com/olliegilbey/clave/issues/122)** (closed) — the compressed four answers are in the closing comment; the probe results are in the comment above it. Reading the two comments is usually enough, and cheaper than the doc.
- **[#117](https://github.com/olliegilbey/clave/issues/117)** (closed) — tooltip mechanism + guardrail coverage. Its resolution comment covers parts 2 and 3.
- `crates/clave/src/setup.rs:63-212` — `config_kdl` (the overlay being moved) and `layout_kdl`. The comments here are load-bearing; read them before editing.
- `crates/clave/src/setup.rs:800-835` — the `exec` site where `--config` is passed.
- `crates/clave/tests/kdl_guardrail.rs:1-100` — the real-parser harness and its helpers; `:359` is `keybind_and_layout_plugin_configurations_match`.
- `docs/dev/TESTING.md` — risk taxonomy. Part 1 is a generated-artifact change.

## Current State

**Nothing implemented.** This was a wayfinding session — research and scoping only. Working tree is clean; the only untracked files are prior `docs/status/` handoffs.

Done this session:

- #114 **rewritten** — retitled, rescoped, and now carrying the full spec for all three parts including the verified route, the accepted cost, and the build specifics.
- #122 **created, researched, closed**. Findings on `research/zellij-config-passthrough` (pushed, not merged, no PR).
- #110 **corrected** — its part 2 was recorded as user error; it was this bug. Part 1 (floating-pane geometry) untouched and still its own ticket.
- **PR #126 merged** — FOOTGUNS.md entry for the `--config` discard trap, on `main`.
- Map #115 updated: #122 in Decisions-so-far, #114 rescope noted, and a Notes rule about not editing the body (see Context to Preserve).

## What's Working

**The route for part 1 is verified, not inferred — build on it directly.**

A layout file's root nodes are parsed a second time as config and merged over the resolved config: `Config::from_kdl(&raw_layout, Some(config))` at `zellij-utils-0.44.3/src/input/layout.rs:1475`. So clave can drop `--config`, put `keybinds` / `unbind` / `session_serialization` as **root nodes in the layout file**, and the user's `~/.config/zellij/config.kdl` loads through zellij's normal path. **No KDL-merging code is needed** — zellij performs the merge itself with correct semantics.

This was proven in-process, not reasoned about. A scratch probe ran against the same pinned parser `kdl_guardrail.rs` already uses (`zellij-utils = 0.44.3`, exact `=` pin), with a hostile user config (`keybinds clear-defaults=true`, `default_mode "locked"`, `pane_frames false`, per-mode `Ctrl h`). Seven assertions, seven passes:

```
LAYOUT PARSE:           ok
ALT-T IN NORMAL:        Some([NewTab { … }])          ← clave's bind landed
CTRL-Q IN NORMAL:       None                          ← clave's unbind applied
USER default_mode:      Some(Locked)                  ← user's setting survived
USER pane_frames:       Some(false)                   ← user's setting survived
USER Ctrl-h IN LOCKED:  Some([SwitchToMode Normal])   ← user's bind survived
session_serialization:  Some(false)                   ← clave's setting landed
```

The probe file was deleted (it was scratch), but **its shape is the guardrail test the build owes** — recreate it in `kdl_guardrail.rs` as a real assertion. The full listing is in the #122 comment titled *"Hinge verified in-process"*. Note `Keybinds` has no `get_keybinds_for_mode` accessor — reach through the public tuple field: `config.keybinds.0.get(&InputMode::Normal)`.

**Also working and worth reusing:**

- `crates/clave/tests/kdl_guardrail.rs` is a mature real-parser harness — it already runs every generated artifact through `Layout::from_str` / `Config::from_kdl` from the pinned zellij-utils. Extend it; do not build a parallel test path.
- Part 3 is cheap and independent of everything else. clave-bar's current subscription set is at `crates/clave-bar/src/main.rs:425-431`; adding `EventType::ModeUpdate` gives `get_mode_keybinds()`, `KeyWithModifier` implements `Display` for key chips, and the labels are clave's own knowledge. Pure `render_rows` work under the existing model/render split. `render()` at `main.rs:598` currently ignores its `rows: usize` parameter, so a bottom-pinned hint row is available geometry.
- The maintainer engaged closely with this and endorsed the direction. The framing that landed was plain-language cause-and-effect, not mechanism-first.

## Important Discoveries

**The bug, and why it hid for months.** `zellij --config <path>` early-returns past the user's config: `Config::try_from` at `zellij-utils-0.44.3/src/input/config.rs:170-186` returns `Config::from_path(path, Some(default_config))` before ever reaching `find_default_config_dir()`. clave passes `--config` at `crates/clave/src/setup.rs:827-832`. So every clave session runs zellij's built-in defaults plus clave's overlay, and the user's keybinds, `default_mode`, `pane_frames` and `ui` are discarded. It stayed invisible because the one setting anyone would notice — `theme` — is supplied independently by Ghostty (`~/.config/ghostty/config: theme = "Kanagawa Wave"`), and zellij's built-in default theme renders through the terminal's ANSI palette. Themes are also a partial exception in code: `apply_themes_to_config` (`zellij-utils/src/setup.rs:318-341`) still reads `~/.config/zellij/themes/`, but the *selection* lives in `options`, which comes from the `--config` file — so a theme name is inert while its definition sits loaded.

**It already caused one misdiagnosis.** #110 part 2 recorded `Ctrl h` → `r` → `+` as user error because `Ctrl h` is stock zellij's `SwitchToMode "Move"`. The maintainer's own config binds `Ctrl h` to the locked/normal toggle. Do not repeat this: if a user's zellij keybind behaves like stock zellij inside clave, that is this bug. Now in `FOOTGUNS.md` (PR #126).

**The accepted cost of the layout route.** The config hot-reload watcher builds a fresh `CliArgs::default()` with only `.config` set — **no layout** (`input/config.rs:495-500`). So a user editing their own zellij config mid-session drops clave's *entire* overlay from that session: all 14 `Alt` binds, the five unbinds, `session_serialization false`. It fails **silently** — the bar keeps rendering, because the plugin is already loaded and only the keybinds routing into it vanish. "Alt+j stopped working" is the symptom. The `Ctrl q` unbind reverting is the dangerous part: stock `Ctrl q` is Quit, and one stray press kills the whole fleet (#28). The README note must say *why* you relaunch, not just that you should.

Note the hazard **moves rather than accumulates**: today the watched file is clave's own generated `config.kdl`, rewritten by every `clave setup` and `just release` — that is `FOOTGUNS.md`'s existing cold-restart entry. On the layout route it becomes the user's config, which changes far less often. On that route the existing hot-swap-under-a-running-bar hazard *disappears*.

**Approaches ruled out, with reasons — do not revisit:**

- **Contributing clave's keys into zellij's status-bar tooltips.** Impossible. The bundled `status-bar.wasm` carries hardcoded English labels (`Move focus`, `Split down`, `New tab`, `Session Manager`, `Toggle Floating`) and no generic-action rendering — confirmed by direct inspection of the binary, on top of #117's reading. clave's binds ride in every `ModeUpdate` as data but would never render as labelled hints. A *different* plugin can occupy that same `size=1` pane; that is the only live variant.
- **Config-file layering.** Does not exist. One config file per run over built-ins; `--config-dir` selects the same single `config.kdl` by another route; the env vars are clap aliases for those same two flags; there is no `include` directive (`Config::from_kdl` dispatches seven node names and opens no file, `kdl/mod.rs:4855-4893`).
- **Text-concatenating the user's config with clave's.** Unsafe. `KdlDocument::get` returns the **first** match only (kdl 4.7.1 `document.rs:80`, already recorded at `setup.rs:144`), so a user config carrying its own `keybinds` node would silently swallow clave's block and every `Alt` bind would vanish. Merge-at-generation would need real KDL-node surgery — it is the documented fallback in the research doc, not the plan.
- **A `clave setup --no-status-bar` flag / a new clave config file.** Considered and dropped once passthrough was found: the user's own config is the right place for their preferences, so inventing a parallel surface is the wrong shape.

**Build specifics established:**

- **Emit explicit `normal {}` / `locked {}` blocks, not only `shared_among`.** A user's per-mode `normal { bind "Alt j" }` beats a `shared_among` block *regardless of document order* — shared blocks parse in phase 1, per-mode in phase 2 (`kdl/mod.rs:4554-4599`). *(Inherited from the research agent; not personally re-verified.)*
- **Three generation sites**, all in-repo: `setup::layout_kdl`, `setup::launch_layout_kdl_for`, and `add::tab_node` — one-shot `zellij action new-tab --layout` files do **not** pass through `default_tab_template` (`add.rs:116-122`).
- **`keybind_and_layout_plugin_configurations_match`** (`kdl_guardrail.rs:359`) collects every plugin configuration from every generated layout and asserts each carries `clave_binary`. A restored status-bar pane makes it fail loudly — that is the tripwire working, not a bug. Filter to the clave `file:` location and positively assert the status-bar pane. Spelling matters: written as the bare alias `plugin location="status-bar"`, the node parses as `RunPluginOrAlias::Alias` with `run_plugin = None` and the guardrail **silently skips it** (`layout.rs:404-411`). Both spellings work at runtime; pick one deliberately.
- **`PluginCommand::RebindKeys { keys_to_rebind, keys_to_unbind, write_config_to_disk }`** (`data.rs:3504-3508`) is the eventual repair — a delta, no file write. Costs `Permission::Reconfigure`, which clave-bar does not hold, and the permission cache is all-or-nothing per plugin. **Never pass `write_config_to_disk: true`** — it rewrites clave's own config and stamps it AUTOGENERATED.
- Stock zellij ships `tab-bar` (top) and `status-bar` (bottom) as one-row panes (`zellij-utils-0.44.3/assets/layouts/default.kdl`); clave emits neither, so both are currently absent — clave never removed them, it simply never emitted them. Vertical budget is one row each, taken from the terminal pane, not the sidebar: the bar sits as a horizontal sibling below the vertical bar/terminal split, so the templates need one extra nesting level. The sidebar's row math is untouched. `compact-bar` is a third option. Plugin identity is a non-issue (#117).

**Verification boundary.** Everything above about parsing and merging is proven against the real pinned parser. What is **not** proven is the server-side *application* of the merged config at runtime — `zellij-server` is not vendored. Live validation is owed for that half.

## Next Steps

1. **Claim #114** — `gh issue edit 114 --add-assignee @me`, before any work. An open unassigned ticket is unclaimed; concurrent sessions rely on this.
2. **Run the design conversation** (it is a `wayfinder:grilling` ticket). The open questions are genuinely open: does the horizontal `tab-bar` earn a row when the sidebar *is* the tab list? Status-bar, compact-bar, or neither? Where does the hint row sit relative to #116's dormant-selected `⏎` hint, which is further content for the same row? Record the resolution as a comment on #114.
3. **Build part 1** — move the overlay into the three layout generators, drop `--config` from the exec, add the guardrail test in the probe's shape, write the README limitation note. This is the load-bearing half.
4. **Build part 3** — the hint row, plus #110 part 3's ownership table generated from the same `ModeInfo` data.
5. **Part 2** per the design decision.

**Open questions / blockers:** none blocking. Live validation of the runtime half is owed and is the maintainer's to run (see Context to Preserve).

Where work stopped — the maintainer's last instruction, verbatim:

> so, just fix things by adding comments, rather than by editing the body - for this scenario.

Earlier in the session, the two steers he gave on #114 itself, verbatim:

> okay, to drive this, what must I do exactly?

> okay, fair, yeah, we can look at that permission request, sounds sensible, but also, it's not the end of the world, can just add to the readme about the niggle that updating zellij config will require a clave relaunch. But fixing better will be better.

And the exchange that reframed the whole ticket — his question, which was the one that found the bug:

> So, how does a person have a zellij config of their own while running clave?

## Context to Preserve

- **Never run zellij commands against his session.** Ollie dog-foods clave daily and the session you are in is his live working fleet. Against `clave-test` you may run `zellij action` freely (`ZELLIJ_SESSION_NAME=clave-test …`, staged with `just sandbox`); against his session you run nothing, **not even a read**. Launching or killing any session is his, as is `just release` and anything writing `~/.local/share/clave/`. Print those commands and let him run them. See `AGENTS.md` and `docs/dev/TESTING.md` § the sandbox drive loop.
- **Do not edit the map (#115) issue body — post a comment.** Several agents work that board at once and the body is last-write-wins: whoever saves second silently erases the other, no conflict, no notification. Four entries were lost this way on 2026-08-01, twice within an hour, including edits that had just been pushed and verified. Comments are append-only. The map's Notes now says this, and a salvage comment holds what was lost, flagged as archaeology-to-verify rather than fact.
- **Response style:** lead with the outcome; plain cause-and-effect over mechanism-first; short sentences between tool calls, consolidation at the end. He asks "give it to me plain and straight" when it drifts technical — that is a style correction, not a request to simplify the work.
- **Report faithfully.** He checks. Twice this session he caught real gaps by asking a simple question — separate what is verified from what is inherited or inferred, and say which is which.
- The four gates a PR must show green: `cargo fmt --all --check`, `cargo test --workspace`, `cargo build -p clave-bar --target wasm32-wasip1`, `cargo clippy --workspace --all-targets -- -D warnings` — or `just gates`. `lint` is a required check, so red fmt/clippy blocks merge. Use cargo mutants.
- The GLYPH rule applies to non-ASCII in Rust source; stage explicit paths, never `git add -A`.

## Restart Hint

Tree is clean on `main` (only untracked prior handoffs); safe to `/clear`. Note other sessions switch this shared checkout's branch mid-flight — run `git branch --show-current` before assuming. Start by claiming #114 and reading its body; it is the spec.
