# Research #122 — clave replaces the user's zellij config wholesale; what layering exists?

Sources: vendored `~/.cargo/registry/src/*/zellij-utils-0.44.3/` and `…/zellij-tile-0.44.3/`
(the only zellij crates on disk — `ls ~/.cargo/registry/src/*/ | grep ^zellij` returns
exactly those two). **`zellij-server` and the `zellij` binary crate are NOT vendored**, so
every claim about what the *server* does with a parsed `Config` is inference and is
labelled as such. All zellij citations are 0.44.3, the pinned line
(`crates/clave/tests/zellij_pin_tripwire.rs:26`). Paths below are relative to those two
crate roots or to the clave repo root.

**kdl crate version.** zellij-utils declares `kdl = "4.5.0"` (caret, `zellij-utils-0.44.3/Cargo.toml:103-106`);
the workspace lock resolves it to **4.7.1** (`Cargo.lock:1160-1162`), which is the single
`kdl` entry in the lock, and the version clave pins (`crates/clave/Cargo.toml:51`). So the
parser clave tests against *is* the parser zellij runs — but only because the caret
resolved that way; a future `cargo update` could move zellij-utils onto a newer 4.x while
clave stays pinned. The tripwire that would catch that is the pin test, not the lock.

---

## Headline

**There is a layering channel, and clave is already using its transport.** A zellij
**layout file's top-level nodes are parsed a second time as config and merged OVER the
config that `--config`/the user's config dir produced** (`input/layout.rs:1475`). clave
passes `--layout` already. So clave's entire overlay — `session_serialization false`, the
`shared_among` binds, the five `unbind`s — can move into `layout.kdl`, `--config` can be
dropped, and the user's `~/.config/zellij/config.kdl` loads normally through the branch
that `--config` currently short-circuits.

That reshapes the ticket: **merge-at-generation (Q3) is a fallback, not the only route.**
Q3 is answered in full anyway, because the layout route has one real cost — the config
hot-reload watcher (Q2) rebuilds config from the watched file *with no layout*, and would
drop the overlay mid-session.

---

## Q1 — Does zellij 0.44.3 offer any layering?

**Direct answer:** For the *config file itself*, **no**. There is exactly one config file
per run, merged over zellij's built-in defaults, and nothing else. `--config-dir` is not a
second source — it selects the same single `config.kdl` by a different route. The env vars
are clap aliases for the same two flags, not an additional layer. There is **no `include`
directive** in the KDL config format. The **layout file is the one real layering surface**,
and it is a full config layer, not a keybinds-only one.

**Evidence:**

- **`--config` wins and early-returns.** `Config::try_from(&CliArgs)` — `input/config.rs:170-198`.
  Line 171-174: `if let Some(ref path) = opts.config { … return Config::from_path(path, Some(default_config)); }`.
  `find_default_config_dir()` is only reached at line 185, below that return. Already in
  `FOOTGUNS.md:54`.
- **`--config-dir` without `--config` is not layering.** `input/config.rs:182-197`: the
  config dir (CLI, else `find_default_config_dir()`) yields exactly one path,
  `<dir>/config.kdl` (`DEFAULT_CONFIG_FILE_NAME`, `config.rs:26`); if it exists it is
  merged over built-in defaults, if not, defaults alone. **One file, never two.**
  `find_default_config_dir` returns the *first existing* of
  `[home_config_dir(), xdg_config_dir(), SYSTEM_DEFAULT_CONFIG_DIR]` (`home.rs:17-37`) —
  first-existing-wins, again not a merge.
- **Env vars are the same flags.** `pub config: Option<PathBuf>` carries
  `env = ZELLIJ_CONFIG_FILE_ENV` and `pub config_dir` carries `env = ZELLIJ_CONFIG_DIR_ENV`
  (`cli.rs:73-79`); the names are `ZELLIJ_CONFIG_FILE` / `ZELLIJ_CONFIG_DIR`
  (`consts.rs:10-11`). Clap `env` populates the *same field* the flag does, so
  `ZELLIJ_CONFIG_FILE` hits the identical early return. No extra precedence, no extra file.
- **No `include`-style directive.** `Config::from_kdl` (`kdl/mod.rs:4855-4893`) dispatches
  on exactly seven top-level node names — `keybinds`, `themes`, `plugins`, `load_plugins`,
  `ui`, `env`, `web_client` — plus `Options::from_kdl` over the document's flat properties.
  There is no path that opens another file. `grep -n '"include"\|include_path' kdl/mod.rs input/config.rs`
  → no matches. **Confident negative.** If it existed it would be a node name in that
  dispatch block at `kdl/mod.rs:4864-4891`.
- **The exception that *is* layering: layout files carry config.**
  `Setup::from_cli_args` → `parse_layout_and_override_config` (`setup.rs:316`, `:598-644`)
  → `Layout::from_path_or_default(chosen_layout, layout_dir, config)` (`input/layout.rs:1460`),
  whose body does:
  ```rust
  let layout  = Layout::from_kdl(&raw_layout, …)?;
  let config  = Config::from_kdl(&raw_layout, Some(config))?; // input/layout.rs:1475
  ```
  The *same raw layout text* is fed to the config parser with the already-resolved config
  as base. Same for `from_url` (`layout.rs:1484`) and `from_stringified_layout`
  (`layout.rs:1492`).
- **The layout parser tolerates config nodes by construction.** `KdlLayoutParser::parse`
  rejects a root node only if it is not `layout` **and** is a reserved word
  (`kdl/kdl_layout_parser.rs:2461-2470`). The reserved list (`kdl_layout_parser.rs:62-90`)
  is pane/tab vocabulary — `pane`, `tab`, `plugin`, `children`, `name`, `size`, `cwd`, … —
  and contains none of `keybinds`, `themes`, `plugins`, `ui`, `env`, `web_client`,
  `load_plugins`, or any `Options` property name. The source comment at
  `kdl_layout_parser.rs:63-64` says so out loud: *"it's important that none of these words
  happens to also be a config property, otherwise they might collide."* Non-reserved root
  nodes are simply ignored by the layout parser and consumed by the config parser.
- **Not every layout consumer applies the config section.** `from_path_or_default_without_config`
  (`layout.rs:1501-1515`) parses a layout and discards its config — used by the
  available-layouts scan (`layout.rs:1263`). Inference (server not vendored): the runtime
  `zellij action new-tab --layout <f>` path is a `without_config`-class consumer, so
  layout-borne config applies **at session creation only**. clave's `add::tab_node`
  (`crates/clave/src/add.rs:132-172`) does not need it either way.

**So the ordering for a clave launch, if `--config` were dropped, is:**

```
built-in defaults  →  ~/.config/zellij/config.kdl  →  clave's layout.kdl config section  →  CLI `options`
   (config.rs:190)          (config.rs:191)                 (layout.rs:1475)                (setup.rs:328)
```

Each arrow is `Config::from_kdl(text, Some(base))`, i.e. later wins per-key. clave sits in
the last position it can occupy without CLI options — which is exactly where it wants to be.

---

## Q2 — The runtime reconfigure path

**Direct answer:** **Yes — `rebind_keys` is exactly the "add keybinds without owning the
config file" API, and it exists in 0.44.3.** A plugin holding `Permission::Reconfigure` can
send a *delta* of binds and unbinds with `write_config_to_disk: false`, mutating the running
session's keymap without touching any file. There is also the blunter `reconfigure(String, bool)`
which submits a whole stringified config. Neither is exposed on the `zellij action` CLI —
this is a plugin-only surface, so clave-bar would be the actor.

**Evidence:**

- `PluginCommand::RebindKeys { keys_to_rebind: Vec<(InputMode, KeyWithModifier, Vec<Action>)>,
  keys_to_unbind: Vec<(InputMode, KeyWithModifier)>, write_config_to_disk: bool }` —
  `data.rs:3504-3508`. Shim: `pub fn rebind_keys(keys_to_unbind, keys_to_rebind, write_config_to_disk)`,
  `zellij-tile-0.44.3/src/shim.rs:2441-2455`, doc-commented *"Rebind keys for the current user"*.
  Wire round-trip at `plugin_api/plugin_command.rs:1921-1937` and `:3723-3735`.
  **The payload shape is a delta — two lists of individual keys — not a whole keymap.**
  That the server applies it additively rather than as a replacement is *inference*
  (`zellij-server` not vendored), but there is no way to express "replace everything" in
  this payload, and the sibling `keys_to_unbind` list only makes sense against a surviving base.
- `PluginCommand::Reconfigure(String, bool) // String -> stringified configuration, bool -> save configuration`
  — `data.rs:3448`; shim `pub fn reconfigure(new_config, save_configuration_file)`,
  `shim.rs:1735-1741`, *"Change configuration for the current user"*.
- **Permission gate:** `Permission::Reconfigure` / `PermissionType::Reconfigure`,
  `data.rs:1073`, display name *"Change Zellij runtime configuration"* (`data.rs:1101`).
  clave-bar does **not** currently hold it — `BAR_PERMISSIONS` is
  `["ReadCliPipes", "ChangeApplicationState", "ReadApplicationState", "RunCommands"]`
  (`crates/clave/src/setup.rs:56-61`). Adding it is a **breaking permission-cache change**:
  zellij permissions are all-or-nothing per plugin and a partial cache match raises the
  unanswerable prompt (`FOOTGUNS.md:46`). Both `permissions.kdl` key forms must be
  re-seeded in the same change.
- **Result events exist:** `Event::ConfigWasWrittenToDisk` (`data.rs:1005`) and
  `Event::FailedToWriteConfigToDisk(Option<String>)` (`data.rs:1000`) — these are the
  ack/nack for the `write_config_to_disk: true` variant. There is **no** event named for a
  successful in-memory rebind; the observable is `Event::ModeUpdate(ModeInfo)`
  (`data.rs:946`), whose `keybinds: KeybindsVec` is a straight dump of the merged table
  (`input/keybinds.rs:95-105`), plus the one-shot `Event::InitialKeybinds(KeybindsVec)`
  (`data.rs:1028`). **This is what makes a self-healing loop testable**: the bar can *see*
  whether its own binds are present and re-assert them.
- **The write-to-disk variant is a footgun for clave specifically.** If it writes, it writes
  to `Config::config_file_path(opts)` (`input/config.rs:273-283`), which with `--config`
  present is **clave's generated `config.kdl`** — and `Config::write_config_to_disk`
  (`config.rs:287-330`) backs the old file up to `config.kdl.bak` and prepends
  `// THIS FILE WAS AUTOGENERATED BY ZELLIJ …` (`config.rs:436-438`). A clave-generated
  artifact silently becoming a zellij-generated artifact would defeat every guardrail test
  in `crates/clave/tests/kdl_guardrail.rs`. **Always pass `write_config_to_disk: false`.**

**And the reason this matters even if clave never calls it — the config watcher:**

`watch_config_file_changes` (`input/config.rs:442-513`) polls the config file at 1 s
(`config.rs:469`) and on create/modify does:

```rust
let mut cli_args_for_config = CliArgs::default();
cli_args_for_config.config = Some(PathBuf::from(&config_file_path));   // config.rs:495-496
if let Ok(new_config) = Setup::from_cli_args(&cli_args_for_config) { … } // config.rs:497-500
```

**A fresh `CliArgs::default()` — no `--layout`.** So the reload path reconstructs config
from the watched file plus built-in defaults and *the default layout*, never clave's
layout. Whatever the server then does with `new_config` (inference — not vendored;
`FOOTGUNS.md:53` records empirically that it hot-swaps keybinds into running sessions),
**a layout-borne overlay is absent from it.**

Concretely: if clave moves its overlay into the layout and drops `--config`, the watched
file becomes `~/.config/zellij/config.kdl`, and **the user editing their own zellij config
while a clave session is live would wipe every clave bind from that session.** That is the
one real cost of the layout route, and `rebind_keys` is its repair: subscribe to
`ModeUpdate`, notice clave's binds have vanished from `get_mode_keybinds()`, re-assert them
with `write_config_to_disk: false`. Fully testable in `model.rs` as a pure
`keybinds_present → Option<RebindDelta>` function.

---

## Q3 — What safe merge-at-generation requires

**Direct answer:** Text concatenation is unsafe and KDL-node-level merging is required —
but note that **the layout route (Q1) avoids this problem entirely**, because zellij itself
performs the merge, in the parser, with the correct semantics. Merge-at-generation is the
fallback for a world where the layout channel is rejected. If taken, it is a
kdl-4.7.1-document rewrite with the node-by-node rules below.

**Why concatenation fails:** every consumer in `Config::from_kdl` is
`kdl_config.get(<name>)` (`kdl/mod.rs:4864, 4867, 4872, 4876, 4880, 4884, 4888`), and
`KdlDocument::get` is `self.nodes.iter().find(|n| n.name().value() == name)` —
**first match only** (`kdl-4.7.1/src/document.rs:79-82`; already recorded at
`crates/clave/src/setup.rs:144`). A user config with its own `keybinds` node followed by
clave's appended `keybinds` block means clave's block is *never parsed*: no Alt binds, no
unbinds, and no error. Silent.

### Node-by-node merge rules

Parse the user's config with `kdl = "=4.7.1"` into a `KdlDocument`, then:

| Node | Rule | Why |
| --- | --- | --- |
| `keybinds` | **Merge into the user's existing node.** Locate the first `keybinds` node; if absent, append one. Never append a second. | `kdl/mod.rs:4864` + `document.rs:79-82`. |
| `keybinds` → `clear-defaults` prop | **Do not set, do not clear.** Leave the user's value untouched. | `kdl/mod.rs:4548-4553`: truthy → base keybinds are thrown away and parsing starts from `Keybinds::default()`. Setting it would delete the user's scheme; clearing it would resurrect stock binds the user deliberately removed. Either way clave's own binds survive, because they are inserted after. |
| clave's `shared_among "normal" "locked"` block | **Append as a child of `keybinds`, but be aware it loses to per-mode blocks.** | `Keybinds::from_kdl` runs *all* `shared`/`shared_except`/`shared_among` blocks first (`kdl/mod.rs:4554-4587`), *then* all per-mode blocks (`:4588-4599`) — regardless of document order. A user `normal { bind "Alt j" … }` therefore **overwrites** clave's shared `Alt j`. **If clave must win unconditionally, emit per-mode blocks (`normal { … }`, `locked { … }`) appended after the user's, not a `shared_among` block.** Within a phase, later insert wins (`:4613`, `HashMap::insert`), and two blocks naming the same mode hit the same map (`:4639-4656`). |
| per-mode `clear-defaults` | **Never emit.** | `kdl/mod.rs:4652-4655` calls `.clear()` on that mode's whole map at the point the block is reached — it would erase the user's binds for that mode. |
| `unbind` (clave's five keys) | **Merge into the user's single global `unbind` node if one exists; else append exactly one.** | Global unbind is read via `kdl_keybinds.children().and_then(|c| c.get("unbind"))` — **first match only** (`kdl/mod.rs:4600-4602`), applied last, across every mode (`:4627-4638`). A second `unbind` node is silently dead. This is the trap `setup.rs:144-148` already documents, now doubled: it applies to the user's node too. |
| stray nodes inside a mode block | **Only `bind` and `unbind` are legal.** Anything else is a hard parse error `"Unknown keybind instruction: '<name>'"`. | `kdl/mod.rs:4532-4539`. |
| `session_serialization false` (an `Options` property) | **Replace the user's node if present; else append.** Options are flat top-level nodes read with `document.get(name)` (`kdl/mod.rs:2656-2700` and the `kdl_property_first_arg_as_*` macros at `:2305-2320`) — first match only, same trap. | `Options::merge` is `other.<field>.or(self.<field>)` for every field (`input/options.rs:319-400`), so a `None` never clobbers a `Some`; but two `session_serialization` nodes in one document means only the first is seen. |
| `themes`, `plugins`, `ui`, `env`, `web_client` | **Leave alone** — clave emits none. | Merged additively if present (`kdl/mod.rs:4867-4891`; `Themes::merge` `input/theme.rs:60-66`, `PluginAliases::merge` `input/plugins.rs:22-24`, `UiConfig::merge` `input/theme.rs:18-22`). |
| `load_plugins` | **Never emit, and be aware it is destructive.** `config.background_plugins = load_plugins` — **assignment, not merge** (`kdl/mod.rs:4876-4879`). The only non-merging node in the whole config parser. | |

### Failure modes needing guardrail tests

1. **User config absent** — `~/.config/zellij/config.kdl` does not exist. Emit today's
   generated file unchanged. Must not create the user's config as a side effect
   (`home::try_create_home_config_dir()` at `config.rs:278` is zellij's business, not clave's).
2. **User config malformed** — it will not parse. **Fail loudly and fall back to the
   clave-only config**, with a `clave doctor` warning naming the file and the kdl error.
   Silently shipping a half-merged config is the worse outcome, and a malformed config
   means the user's zellij is already broken outside clave.
3. **User config already zellij-autogenerated** — first line
   `// THIS FILE WAS AUTOGENERATED BY ZELLIJ, THE PREVIOUS FILE AT THIS LOCATION WAS COPIED TO: …`
   (`config.rs:436-438`), which happens whenever any plugin calls `reconfigure`/`rebind_keys`
   with `write_config_to_disk: true`. It parses fine; it is just a comment. But such a file
   was serialized by `Config::to_string` (`kdl/mod.rs:4894-4926`), which **always writes
   `keybinds clear-defaults=true`** (`:4897-4898`) — so the merged result carries
   `clear-defaults=true` and the whole document is a full keymap. Handling: honour it,
   don't touch the flag, append clave's per-mode blocks. Guardrail: a fixture of a
   `Config::to_string` output merged and re-parsed, asserting clave's binds survive.
4. **User `keybinds` node with per-mode blocks that shadow clave's keys** — the
   `shared_among`-loses-to-per-mode ordering above. Test: fixture binds `Alt j` in `normal`,
   assert the merged config resolves `Alt j` in `normal` to clave's `KeybindPipe`.
5. **User already has a global `unbind`** — assert the merged document contains exactly one
   `unbind` node under `keybinds` and that it carries all five clave keys plus the user's.
6. **Two `keybinds` nodes / two `session_serialization` nodes in the output** — assert
   count == 1 for each, via `KdlDocument::nodes().iter().filter(…).count()`. This is the
   assertion that directly encodes the `document.rs:79-82` trap.
7. **Round-trip through the real parser** — every merged artifact must go through
   `zellij_utils::input::config::Config::from_kdl`, as `crates/clave/tests/kdl_guardrail.rs`
   already does for the generated ones, and the resulting `Keybinds` must be asserted
   semantically (`get_actions_for_key_in_mode`, `input/keybinds.rs:28-36`) rather than by
   string matching.
8. **Staleness** — the merge input is a file clave does not own. The user edits their
   config; clave's `config.kdl` is now a merge of a stale copy. Needs a re-merge trigger
   (`clave setup` at minimum) and a doctor check comparing mtimes. **This failure mode does
   not exist on the layout route**, which merges at parse time, every launch.

---

## Q4 — What clave must keep owning, and does zellij's merge honour it?

**Direct answer:** All three survive, and clave wins every contest — **on both routes** —
because in every merge in this codebase **the later source wins per key**, and clave is
always later. The one thing that can beat clave is a *user per-mode block* against a
*clave `shared_among` block*, which is an ordering artefact inside a single `keybinds`
node, not a config-level precedence rule. `keybinds clear-defaults=true` in the user's
config is **harmless** to clave.

**The merge chain, and who wins:**

- `Config::merge(&mut self, other)` — `input/config.rs:264-272`: **`other` wins** for every
  sub-struct. (Note: not actually on the launch path — `Config::from_kdl` does the
  equivalent inline, `kdl/mod.rs:4859-4891`, with the *file being parsed* in the `other`
  position.)
- `Keybinds::merge` — `input/keybinds.rs:106-116`: per input mode, per key,
  `input_mode_keybinds.insert(other_action, other_action_keybinds)` — **last writer wins,
  key by key**. Modes and keys the other side doesn't mention are untouched. There is no
  "conflict" concept.
- `Options::merge` — `input/options.rs:319-400`: `other.<f>.or(self.<f>)` for every field.
  **A `Some` in the later source always wins; a `None` never clobbers.**
- `Themes::merge` (`input/theme.rs:60-66`), `PluginAliases::merge` (`input/plugins.rs:22-24`),
  `UiConfig::merge` (`input/theme.rs:18-22`): same shape — later wins per entry.

**Item by item:**

| clave-owned | Where today | Contest | Outcome |
| --- | --- | --- | --- |
| `session_serialization false` (§6.8 C8) | `crates/clave/src/setup.rs:154` | user sets `session_serialization true` | **clave wins.** `Options::merge` with clave's `Some(false)` in the `other` position (`options.rs:347`). On the layout route this is `config.options.merge(layout_options)` at `kdl/mod.rs:4860`. Only a CLI `options` flag could beat it (`setup.rs:328`), and clave passes none. |
| `MessagePlugin` binds + `clave_binary` identity (#44) | `crates/clave/src/setup.rs:82-131` | user binds `Alt j` themselves | **clave wins if clave's block is later *and in the same phase*.** Clave's binds are in `shared_among "normal" "locked"`, which is phase 1 (`kdl/mod.rs:4554-4587`); a user's `normal { bind "Alt j" … }` runs in phase 2 (`:4588-4599`) and overwrites it. **This is the one real loss, and it is a phase artefact, not a file-precedence one.** Fix, on either route: emit per-mode `normal { … }` / `locked { … }` blocks instead of `shared_among`, so clave lands in phase 2 after the user's. `clave_binary` identity is unaffected either way — the whole `MessagePlugin` block including the config key is the bind's value, inserted or not as a unit. |
| the five `unbind` keys (#28) | `crates/clave/src/setup.rs:149` | user re-binds `Ctrl q` | **clave wins.** `unbind_keys_in_all_modes` runs *last*, after every bind block in the node (`kdl/mod.rs:4600-4602`, `:4627-4638`), and `.remove()`s the key from every mode present. **Provided clave's unbind is the first `unbind` child of the `keybinds` node** — `children().get("unbind")` is first-match-only. On the layout route this is trivially true (clave's layout has its own `keybinds` node). On the merge route it is failure mode 5 above. |
| — | — | user sets `keybinds clear-defaults=true` | **No effect on clave.** The flag is evaluated when *that file's* `keybinds` node is parsed (`kdl/mod.rs:4548-4553`), discarding the base it was handed at that moment. On the layout route, that moment is the user's config parse, strictly before the layout merge — clave's binds are inserted into the surviving table afterwards. On the merge route, clave's blocks live inside the same node and are appended after, so they are inserted into the freshly-cleared table. Either way clave's binds are present. What the user loses is stock zellij binds, which is what they asked for. |

**One asymmetry worth stating:** on the layout route clave gains the user's `pane_frames`,
`default_mode`, `ui`, `keybinds` and `theme` for free (all merged from their config before
the layout layer). That is the whole point of the ticket. `default_mode "locked"` in
particular is safe because clave's binds are already `shared_among "normal" "locked"`
(`crates/clave/src/setup.rs:157`) — a `locked`-default user gets working Alt navigation
immediately, which they do **not** get today.

**Theme, precisely — what `apply_themes_to_config` does and does not rescue.**
`zellij-utils-0.44.3/src/setup.rs:322-341`: it merges `get_default_themes()` into
`config.themes` (`:332`), then resolves a theme *directory* from
`config_options.theme_dir` **or** `get_theme_dir(cli_args.config_dir.or_else(find_default_config_dir))`
(`:334-337`) and merges every theme definition found there (`:338-340`). So with clave's
`--config` and no `--config-dir`, `find_default_config_dir()` *is* consulted and
`~/.config/zellij/themes/*.kdl` **are** loaded. What it rescues: the user's custom theme
*definitions*. What it does not rescue: the **selection** — `theme`, `theme_dark`,
`theme_light` are `Options` fields (`kdl/mod.rs:2678-2683`) read from the `--config` file,
which today is clave's, which sets none. So the user's `theme "kanagawa"` is inert even
though `kanagawa.kdl` was loaded, and zellij falls back to the built-in `default` theme
(`Config::theme_config`, `input/config.rs:202-207`), which renders through the terminal's
own ANSI palette — the coincidence that masked this for months (`FOOTGUNS.md:54`).
On the layout route the selection comes from the user's config and this stops being a
special case. **On the merge route, `theme`/`theme_dark`/`theme_light` must be carried
across explicitly** — they are `Options` nodes and fall under the `session_serialization`
row of the Q3 table.

---

## What this means for #114 and the implementation

**Recommended route: move the overlay into the layout, drop `--config`.**

1. `crates/clave/src/setup.rs:827-833` drops `--config` and its argument.
2. `config_kdl` (`setup.rs:70-161`) stops being the launch artefact; its body — the
   `session_serialization false` line, the `keybinds` node, the `unbind` node — moves into
   `layout_kdl` (`setup.rs:166-212`) and `launch_layout_kdl_for` (`setup.rs:248…`) as
   **sibling root nodes of the `layout` node**, not inside it. `kdl_layout_parser.rs:2461-2470`
   permits exactly that; `kdl_layout_parser.rs:62-90` proves none of those names collide.
3. Change `shared_among "normal" "locked"` to explicit `normal { … }` + `locked { … }`
   blocks, so clave's binds land in phase 2 and beat a user's per-mode block
   (`kdl/mod.rs:4554-4599`). Costs a duplicated bind list; buys unconditional precedence.
4. `crates/clave/tests/kdl_guardrail.rs` must now parse the layout artefacts **twice** —
   once as a `Layout` and once as a `Config` — mirroring `input/layout.rs:1467-1475`.
   `keybind_and_layout_plugin_configurations_match` (`kdl_guardrail.rs:359`) currently reads
   the keybind side out of `config.kdl`; it reads it out of the layout instead, and the
   `clave_binary` identity check (`kdl_guardrail.rs:431-450`) is unchanged in substance.
5. **New guardrail:** assert the generated layout's config section, parsed with the real
   `Config::from_kdl` against a **non-trivial fixture user config** (`clear-defaults=true`,
   a conflicting `normal { bind "Alt j" }`, `default_mode "locked"`, `session_serialization true`,
   `theme "kanagawa"`), yields clave's `Alt j` action, the five keys absent from every mode,
   `session_serialization == Some(false)`, `theme == Some("kanagawa")`, and
   `default_mode == Some(Locked)`. That single test pins every claim in Q4.
6. **Accept or mitigate the watcher gap (Q2).** Minimum: document it — editing
   `~/.config/zellij/config.kdl` while a clave session is live drops clave's binds until
   relaunch, a `FOOTGUNS.md` entry. Better: clave-bar subscribes to `ModeUpdate`, and
   re-asserts via `rebind_keys(…, write_config_to_disk: false)` when its binds go missing.
   That needs `Permission::Reconfigure` added to `BAR_PERMISSIONS` (`setup.rs:56-61`) **and**
   both `permissions.kdl` key forms re-seeded in the same change (`FOOTGUNS.md:46`), which
   is a cold-restart-for-every-session event.
7. **Upgrade discipline changes shape.** Today `clave setup` rewrites the watched
   `config.kdl` and zellij hot-swaps the new `clave_binary` into live sessions — which is
   the mechanism behind `FOOTGUNS.md:53`'s double-bar hazard. On the layout route the
   overlay is read only at session creation, so that hazard *disappears* and the
   already-mandated "kill and relaunch after `just release`" becomes the only path, not a
   race against a poller.

**Worth confirming live** (print, don't run — commands for Ollie):

```bash
# 1. Does a layout with a root-level config section parse and take effect?
zellij setup --check --layout ~/.local/share/clave/layout.kdl

# 2. Belt and braces on the same file as pure config (should ERROR on the `layout` node —
#    proving the two parsers are genuinely independent, which is the whole mechanism):
zellij --config ~/.local/share/clave/layout.kdl setup --check

# 3. After a prototype: in a SANDBOX session only, confirm the user's keybinds are live
#    (Ctrl h should toggle locked/normal) and `Alt j` still navigates the bar.
```

Item 1 is the load-bearing one. Everything above says it must work; it has not been run.
