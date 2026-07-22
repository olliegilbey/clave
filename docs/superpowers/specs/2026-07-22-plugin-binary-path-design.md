# Design — the bar calls the binary it belongs to (#44)

_2026-07-22 · issue #44 · supersedes the implementation sketch in
`docs/status/2026-07-22-1845-clave-orchestrator.md` §1, which is unsafe as
written (see "Why the obvious fix breaks" below)._

## Problem

`crates/clave-bar/src/main.rs` invokes the CLI **unqualified** at seven sites —
`focus` (:143), `bind` (:150), `prune-tabs` (:162), `open` (:198), `collapse`
(:207), `snapshot` (:390), `touch` (:444). `PATH` decides which binary answers.

On 2026-07-22 a stale `0.1.0` build at `~/.cargo/bin/clave` served a `v0.1.1`
session's `clave open`. That binary composed tab layouts pointing at
`clave-bar-v0.1.0.wasm`; zellij keys plugin identity on file **location**, so
every tab opened that way loaded a *second* bar — duplicate sidebar, two
plugin populations with no shared beacon state, dead navigation
(#43/#44; CONTRIBUTING "The one leak").

`clave setup`/`just release` already bake the absolute versioned path into
keybind `Run` commands, and #29's `release::runtime_binary()` extended that to
`add`/`open`/launch tab commands. The **bar→CLI hop was never covered.**

## Why the obvious fix breaks

The handoff spec proposed passing the binary into the plugin's layout node as
configuration, and changing nothing else. Verified against vendored
zellij-utils 0.44.3, that **reintroduces the very bug it fixes**:

- `input/layout.rs:519-524` — `impl PartialEq for RunPlugin` compares
  `(location, configuration)` as a **pair**, not location alone.
- `cli.rs:711`, upstream's own words: *"the same plugin with different
  configuration is considered a different plugin for the purposes of
  determining the pipe destination."*
- `cli.rs:708` — a plugin that is specified but **not running is launched**. A
  destination miss is not a silent no-op; it spawns another instance.

Our keybinds address the plugin by location with no configuration
(`setup.rs:77,84,114`). They carry an **empty** configuration map today —
`PluginUserConfiguration::new` (`layout.rs:532-548`) strips `name`, `payload`,
`cwd`, `title`, `launch_new`, `skip_cache` and seven more after collection, so
the `name`/`payload` children we emit never survive into configuration. Empty
map on both sides is why Alt+c / Alt+o / clave-nav work.

Adding configuration to the layout node **only** would make the two sides
disagree, and each keybind press would launch a duplicate bar.

## Design

The absolute binary is emitted into **both** sides of the identity pair,
identically. It reaches the emitters by two existing routes, and both already
agree per environment:

- **Generation time** — `write_generated(dir, binary, wasm)` hands one `binary`
  to `config_kdl` *and* `layout_kdl`, so the pair cannot diverge. Its callers
  supply it: `release.rs:197` passes the versioned copy's absolute path,
  `setup.rs:503` passes the dev/sandbox binary.
- **Runtime** — `launch_layout_kdl` (`setup.rs:693`) and `add::tab_node` resolve
  it through `release::runtime_binary()`, which returns the versioned copy iff
  it is installed in the current data dir, else bare `clave` (`release.rs:63`).

Both routes yield the versioned absolute path in a stable install and bare
`clave` in dev/sandbox, so every emitted `clave_binary` in a given environment
carries the same value.

### Generation (`crates/clave`)

| Emitter | Change |
|---|---|
| `setup::config_kdl(binary, wasm)` | add `clave_binary "{binary}";` to each `MessagePlugin` block (`:77`, `:84`, `:114`) |
| `setup::layout_kdl(wasm)` → `layout_kdl(binary, wasm)` | plugin node gains a child block where it has none today; `write_generated:395` updated |
| `setup::launch_layout_kdl(binary, …)` | same child; `binary` already a parameter |
| `add::tab_node(binary, wasm, …)` | same child; `binary` already a parameter |

KDL shape is the **child-node** form, not the property form:

```kdl
plugin location="file:<wasm>" {
    clave_binary "<abs path>"
}
```

`kdl_layout_parser.rs:362-389` reads child nodes as configuration (node name =
key, first entry = value, decoded and unquoted). The property form
(`plugin location="…" clave_binary="…"`) must **not** be used: `:357`
stringifies via `KdlValue`'s `Display`, which re-emits the surrounding double
quotes into the value.

The key name lives in one shared constant, not six string literals. There are
**six** emitting sites for it — `config_kdl` alone has three `MessagePlugin`
format strings (`setup.rs:77` nav, `:84` Alt+c, `:114` Alt+o) plus
`layout_kdl:177`, `launch_layout_kdl:215`, `add::tab_node:109` — and the key's
entire purpose is that all six agree. A constant removes the only realistic
drift (a typo) at zero cost. A KDL builder abstraction would be over-building
for six short strings and is **not** proposed.

`clave_binary` is collision-free — absent from both reserved lists
(`kdl_layout_parser.rs:144-148`: `location`, `_allow_exec_host_cmd`, `path`;
and `layout.rs:532-548`, quoted above).

### The bar (`crates/clave-bar`)

- A **pure** `fn resolve_binary(config: &BTreeMap<String, String>) -> Option<String>`
  — `Some` iff the key is present and non-empty. `Option`, not a
  `(String, bool)` tuple: the bool would exist only to drive one `eprintln!`,
  and `Option` additionally distinguishes *key absent* (warn) from *key present
  with the value `clave`* (the legitimate dev/sandbox case, do not warn). An
  empty-string value is treated as absent — `run_command(&["", "open", …])` is
  a worse failure than the fallback. Pure, so it unit-tests on the host with no
  wasm target and no zellij.
- `load()` — currently `_config` (`main.rs:342`), i.e. the plugin already
  receives a configuration map and discards it — calls `resolve_binary`, stores
  `State.clave_binary`, and logs loudly via `unwrap_or_else` on the `None` arm.
- The seven `"clave"` literals become `self.clave_binary`.

### Error handling

- **Bar, key absent** (a pre-#44 layout, a hand-edited config): loud
  `eprintln!` naming the cause, then fall back to `"clave"`. The fallback stays
  — refusing to act would strand a session — but it never stays silent.
  Silence is what hid the field incident for hours.
- **`release::runtime_binary()`, resolving bare `clave` anyway**: the precise
  divergence condition is *resolving bare `clave` while `data_dir()/bin/`
  contains a `clave-v*` copy*. Announce there — one placement covering
  generation, launch and tab-bake (see "The new invariant"). Also announce
  `setup.rs:489`'s unresolvable `current_exe`, an unexpected failure worth
  naming either way. Dev/sandbox bare `clave` with no versioned copy present is
  correct by design (`baked_binary`'s documented contract) and stays quiet —
  warning on every sandbox launch would train the reader to ignore it.

### The hot-reload SOP must change with it

This is the same identity rule biting the one live mutation an agent is allowed
to perform. `docs/dev/TESTING.md:215` and
`docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md:295` document:

```sh
ZELLIJ_SESSION_NAME=clave-test zellij action start-or-reload-plugin "file:<wasm>"
```

`reload_plugin` (`zellij-server/src/plugins/wasm_bridge.rs:686-697`) resolves
targets with the same `(location, configuration)` match, then loops over the
result. With no `-c`, the command's configuration is empty
(`cli.rs:1370-1374`); once the sandbox layout carries `clave_binary "clave"`,
the running plugin's key is `{clave_binary: "clave"}`. **Zero matches, empty
loop, silent no-op, exit 0** — the agent sees success and validates stale wasm.
That is the failure class the escape record exists to prevent, and it would
bite during this PR's own live-validation pass.

The SOP becomes:

```sh
ZELLIJ_SESSION_NAME=clave-test zellij action start-or-reload-plugin \
  "file:<wasm>" -c clave_binary=clave
```

`PluginUserConfiguration`'s `FromStr` (`layout.rs:563-576`) is comma-separated
`key=value`; values rejoin on `=`, so a path containing `=` survives but one
containing `,` would not — bare `clave` in the sandbox never does. Both docs
are updated **in this PR**, with a why-comment recording that the configuration
is half of plugin identity, so a reload must present the same one.

## The new invariant

**`config.kdl`'s keybind configuration and the *running* plugin's configuration
must be identical, or every keybind launches a second bar.**

Two routes must therefore agree, not one:

1. **Within generation** — `write_generated` passes one `binary` to both
   `config_kdl` and `layout_kdl`, and both files are always regenerated
   together (maintainer ruling, 2026-07-22). Cannot diverge.
2. **Across generation and launch** — the plugin a keybind actually addresses is
   the one launched from `launch.kdl`, composed at launch time from
   `runtime_binary()`. `config.kdl` was written earlier, at setup/release time.

Route 2 mostly holds because #29 made it hold on purpose: `setup.rs:476-489`
bakes through `runtime_binary()` — *"the SAME resolution add/open/launch use at
tab-bake time, so setup and runtime can never disagree"* (codex P2, PR #29) —
and `release.rs:197` passes the versioned copy it just installed, which is what
`runtime_binary()` then resolves. In dev/sandbox both sides are bare `clave`.

It does **not** hold in two cases, and neither is caught by any hermetic test,
because each file is internally coherent — they were written by different
processes at different times:

1. **The version-skewed launcher** (the likelier one, and the #43/#44 field
   scenario itself). `runtime_binary()` probes for
   `data_dir()/bin/clave-v{CARGO_PKG_VERSION}` — *the launching binary's own
   version* (`release.rs:71-79`). `baked_binary`'s own comment
   (`release.rs:57-62`) records that a stable session is cold-started by typing
   `clave`, which resolves on PATH to the **dev** binary. So a `0.1.0` PATH
   binary launching a `v0.1.1` install probes for `clave-v0.1.0`, finds
   nothing, and bakes bare `clave` into `launch.kdl` — while `config.kdl`
   carries `/…/bin/clave-v0.1.1`. Mismatch, and every keybind press launches a
   duplicate bar.
2. **Unresolvable `current_exe`** (`setup.rs:489`) falls back to bare `clave`
   without installing a versioned copy. If a previous install already left one,
   `config.kdl` gets `clave` while launch resolves the versioned path.

Two things make case 1 less alarming than it reads, stated fairly: `wasm_path()`
(`setup.rs:29-39`) is keyed the same way, so the *location* half of the identity
pair already diverges in that scenario — this is shape-identical to the existing
bug, not a new class; and once a session launches correctly the fix is
self-healing, because the bar then invokes the absolute versioned binary whose
own `runtime_binary()` resolves correctly for `add`/`open`.

**Mitigation — one placement, not several.** The anomaly detection belongs
inside `release::runtime_binary()`: *resolved bare `clave` while
`data_dir()/bin/` contains a `clave-v*` copy* → announce, naming both. That
single site covers generation (`setup.rs:503`), launch (`setup.rs:693`), and
tab-bake (`add.rs`, `open.rs`) at once, instead of a hand-placed warning in
`setup` that misses the two paths where the divergence actually occurs.

Guarded by a test where it can be, because the failure is otherwise invisible
until a human presses Alt+c in a live session.

## Testing

Risk class straddles three taxonomy rows — *generated artifacts*,
*cross-process/IPC*, and *install/environment* — so the dossier owes all three,
and the PR carries `needs-live-validation`.

1. **Invariant guard** (new, `tests/kdl_guardrail.rs`): parse `config.kdl`
   through `Config::from_kdl` and the layouts zellij is **actually given**
   through `Layout::from_str`, extract the `KeybindPipe` configuration and the
   `RunPlugin` configuration, assert **equal**. Semantic, through the real
   parsers — not a substring check.

   The targets are `launch_layout_kdl` (both the eager-row and empty-store
   branches) and `add::tab_layout` — **not** `layout_kdl`. `layout.kdl` is
   written at `setup.rs:395` but never handed to zellij: `launch_session`
   passes `--config config.kdl --layout launch.kdl` (`setup.rs:708-711`), and
   later tabs come from the one-shot `add::tab_layout` via `zellij action
   new-tab --layout` (`open.rs:122`, `add.rs:726`). Only `doctor` reads
   `layout.kdl`, and only to check it exists. Testing `layout_kdl` alone would
   stay green while the live coupling broke. It is included as a bonus target,
   not the target.

   The assertion must **not** be a bare `assert_eq!` of the two maps: that is
   satisfied when both sides are empty, so a future zellij adding
   `clave_binary` to the `PluginUserConfiguration::new` strip list
   (`layout.rs:530-546`) would silently drop it from both sides, keep the test
   green, and revert the bar to PATH forever — the exact failure this design
   exists to prevent. Assert the key is **present with the expected value**
   first, then assert equality.
2. **Structural**: existing guardrail tests extend to the new `binary`
   argument, proving the added child block still parses.
3. **Version coherence**: `generated_artifact_set_is_version_coherent` picks up
   `layout.kdl` for free — it carries only the wasm path as a version bearer
   today, and now carries the versioned binary too.
4. **Bar units**: `resolve_binary` with the key present, and absent (→ `"clave"`
   plus the fallback flag).

Gates: `cargo test --workspace` · `cargo build -p clave-bar --target
wasm32-wasip1` · `cargo clippy --workspace --all-targets -- -D warnings`.

## Deliberately out of scope

- **Version-skew guard** (issue #44's "guardrail" paragraph; handoff spec item
  4). Shelling `<binary> --version` at every plugin load adds a subprocess and
  a new `RunCommandResult` routing path to detect a case the configuration key
  already makes near-impossible. Declined for now (maintainer, 2026-07-22:
  "isn't worth doing now for a while… we don't want bloat anywhere yet").
- **The two `zellij` shellouts** (`main.rs:105`, `:126` — the `clave-visited`
  beacon) also resolve through `PATH`. Different hazard, different blast
  radius; not part of #44.
- **Validating the `clave_binary` value's basename** (rejecting anything not
  `clave`/`clave-v*`). Raised in review as a one-`if` reuse of
  `is_clave_hook_command`'s existing predicate. Declined: the value comes from
  our own generator, not from user input, and review confirmed it is not a new
  trust boundary (the file is local-user-owned and the bar already holds
  `RunCommands`). Adds a check without a threat to check for — against the
  "nothing unnecessary" bar.
- **Removing `layout.kdl`.** Review established it is written
  (`setup.rs:395`) but never handed to zellij — only `doctor` checks it exists.
  That makes it vestigial or a manual-use convenience, and either way it is
  out of scope here. Worth its own issue rather than a drive-by deletion in a
  fix for a production incident.
- **Dropping the plugin selector from keybinds** so pipes broadcast to all
  plugins (`cli.rs:708`) would remove the lockstep invariant permanently, but
  trades a hermetically-guarded invariant for an unverifiable behavioural
  change to the daily-driver keys. Available as a follow-up if the coupling
  ever hurts.

## Verification — confirmed at the implementation level

`zellij-server` is not vendored locally, but the crate was fetched and read
(adversarial review, 2026-07-22). The claim is **confirmed in the lookup
itself**, not inferred from doc comments:

- `plugins/wasm_bridge.rs:1676-1686` — the plugin table is
  `HashMap<RunPluginLocation, HashMap<PluginUserConfiguration, Vec<(PluginId, ClientId)>>>`,
  resolved with `.get(location).and_then(|m| m.get(configuration))`. **Exact
  hash-map match on the configuration.** Empty ≠ `{clave_binary: …}`.
- `plugins/wasm_bridge.rs:1861-1894` — on a miss the server calls `load_plugin`
  and `ScreenInstruction::AddPlugin`. **A miss spawns a new bar**, it is not a
  no-op.
- `plugins/plugin_map.rs:186` — the running plugin's half of the key is
  `initial_userspace_configuration`, taken verbatim from the layout
  (`input/plugins.rs:55`). No injected keys, no mutation.
- `route.rs:1619-1624` — `launch_new: false` does not relax the match; the flag
  only injects a `_zellij_id` to *force* uniqueness when true.
- `layout.rs:490-499` — `caller_cwd` injection is alias-only ("we do this only
  for an alias"); a `file:` URL never takes that branch, so it cannot perturb
  our equality.

The child-node-vs-property distinction was also confirmed empirically by
compiling a probe against zellij-utils 0.44.3: the property form yields the
value `"\"/data/bin/clave-v0.1.0\""` — quotes and all — while the child-node
form yields the clean path.

`needs-live-validation` still applies, but for the ordinary reason (this is an
install/environment change on the surface that broke v0.1.1), not because the
mechanism is unproven.
