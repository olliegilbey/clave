# clave doctor + install flow — design

**Status: locked** (brainstormed 2026-07-21; supersedes nothing — greenfield).
Sub-project 1 of 2: this spec covers dependency detection, `clave doctor`,
preflight, first-run setup, and clave's own distribution. Sub-project 2
(README restructure + VHS demo GIF) is specced separately and depends on this
landing first, so the README documents real installer behavior.

## Goal

A fresh machine — local Mac or a bare Linux box over SSH — goes from nothing
to a running clave session with one downloaded file and one command, and when
anything is missing the tool *tells the user exactly what to do next* instead
of failing raw.

Non-goals: auto-installing dependencies (detect + guide only — halt and tell
the user what to type, never run installs for them), a Homebrew tap (later if
demand appears), Nerd Font detection (deferred — see Deferred items).

## Research grounding (2026 survey, verified against upstream source)

The design steals deliberately:

- **mise**: detect the *package manager*, not the OS — probe for
  `brew`/`apt`/`dnf`/`pacman`/`apk` directly; `brew` on Linux and `apt` in a
  container are both real. Honest fallback when nothing probes: name the tool,
  say "install it manually", link upstream.
- **pyenv**: verify before recommending (`type brew` before printing a brew
  command); URL otherwise.
- **uv**: remediation text lives *with* the error — identify → what provides
  it → indented copy-pasteable command → link. One source of copy for both
  doctor and inline failure.
- **flutter**: grouped `[✓]/[✗]/[!]` categories; all user-facing strings
  centralized in one module (validators hold logic, one file holds copy);
  hedge honestly ("It is likely available from your package manager: …, or
  see <url>"). Anti-lesson flutter#17781: never nag about checks the command
  the user ran doesn't need.
- **neovim `:checkhealth`**: advice is a structured list, not prose.
- **Homebrew 2026 / npm 12**: print the plan; prompt only when the plan
  exceeds what the user named; **never prompt without a TTY**.
- **Anti-pattern (Docker)**: "Is the daemon running?" — a rhetorical question
  with no next action. Always tell, never ask.
- **InstallFix (Push Security, 2026)**: ad-bought fake docs pages poison the
  one-liner devs copy; Claude Code users are a named primary target. So:
  remediation for `claude` prints ONLY the official docs URL, and clave's own
  README leads with explicit binary download + attestation, not a pipe-to-shell.
- **zellij#3708**: session discovery needs `$XDG_RUNTIME_DIR`, which SSH
  logins often lack (no systemd user session) — local and SSH shells on the
  same host silently disagree. Linux-only doctor check.

## Locked decisions

| Decision | Ruling |
|---|---|
| Surface | Auto-preflight per command + standalone `clave doctor` (with `--json`) |
| Dep tiers | zellij·claude·git required; fzf·zoxide required only by `clave add` |
| Versions | Presence for all; version-parse zellij only, **warn** (never halt) off tested 0.44.3 |
| Guidance | Probed pkg-manager command + upstream URL fallback; zellij and claude are URL-only |
| Discovery | `$CLAVE_<TOOL>_BIN` override → `which_global` → curated known locations; off-PATH finds are used via absolute path (Warn, not Problem) |
| Doctor scope | External deps + clave's own setup state + environment |
| Repair | Doctor never mutates; `clave setup` is the one repair path |
| Preflight | Halts only on missing required-for-this-command deps; prints only failures |
| In-session `add` | Abort cleanly with guidance, hold the pane open (see §Preflight) |
| Distribution | cargo-dist → attested GitHub Releases; Linux = static musl (x86_64 + aarch64); **wasm embedded in release binaries** |
| First run | Bare `clave` auto-runs setup: print plan → TTY-gated single confirm → setup → launch |

## Architecture

New module `crates/clave/src/doctor.rs` (+ `doctor/copy.rs` if it grows), in
the codebase's pure-core/thin-shell idiom (`session_is_live`, `merge_hooks`):

```
gather(env) -> Facts        // ALL the IO: which_global probes, --version
                            // outputs, file existence, env vars. Thin.
diagnose(&Facts) -> Vec<Finding>   // pure — every check is a unit test
render_report(&[Finding]) -> String    // doctor's grouped view (golden-tested)
render_failures(&[Finding]) -> String  // preflight's failures-only view
```

```rust
struct Finding {
    group: Group,        // RequiredTools | AgentPicker | Setup | Environment
    severity: Severity,  // Ok | Warn | Problem
    label: String,       // "zellij 0.44.3" / "fzf not found"
    advice: Vec<String>, // neovim-style structured lines; empty when Ok
}
```

- **One copy module.** Every user-facing remediation string lives in one
  place (the flutter `user_messages` move). `render_failures` and
  `render_report` draw from the same `Finding`s, so doctor and preflight are
  incapable of drifting (the uv property, adapted: a Finding *collection*
  rather than an error enum, because doctor must gather N findings and
  continue where uv's enum returns one and stops).
- `Facts` derives `Serialize` → `clave doctor --json` is free, consistent
  with `clave ls --json` / `clave dev status`.
- Exit code: 0 clean (warnings allowed), 1 on any `Problem` (mise's rule).
- Rendering degrades to ASCII (no color, `x`/`!`/`ok`) when stdout is not a
  TTY.

### Probes

- `which` crate, **`which_global()`** — plain `which()` respects cwd, and
  clave runs in arbitrary project dirs where a `./brew` would satisfy the
  probe. `apk` additionally probed at `/sbin/apk` (commonly off PATH).
- Package-manager detection is probe-first: try `brew`, `apt-get`, `dnf`,
  `pacman`, `apk` in that order via `which_global`; distro identity is never
  consulted (`ID=fedora` doesn't imply dnf works — Silverblue). No `os_info`
  crate (shells out to `lsb_release`, removed in RHEL 9).
- Zellij version: parse `zellij --version` (`zellij 0.44.3`). Compare against
  `const TESTED_ZELLIJ: &str = "0.44.3"` (cited to the validation ledger —
  permission-cache format and pane-resize semantics are pinned to it). Any
  mismatch → Warn naming the tested version. Unparseable → Warn, never error.

### Binary discovery — beyond PATH

PATH is not ground truth. Two facts force a discovery layer:

1. **Tools live in places only interactive shells know about.** Claude Code
   alone has several legitimate homes — native installer (`~/.local/bin`),
   the local-install migration (`~/.claude/local/claude`), npm-global under a
   version manager (`~/.nvm/versions/node/*/bin`), volta, bun, homebrew —
   and several are PATH-visible only via rc-files an interactive shell
   sources. The migrate-installer's *alias* helps no spawned process at all.
2. **clave's exec contexts don't inherit the user's interactive PATH.**
   `clave spawn` execs `claude` inside a zellij pane (env = whatever the
   zellij server inherited at session creation); `clave add` runs fzf/zoxide
   from a keybind-spawned pane. A tool that works when the user types it can
   be absent where clave execs it — especially over SSH.

So every external tool resolves through one function:

```
discover(tool) -> Discovered { path, via: Override | Path | KnownLocation }
```

Resolution order (first hit wins):

1. **Explicit override**: `$CLAVE_CLAUDE_BIN` (and `CLAVE_ZELLIJ_BIN` etc. —
   same env-var pattern as `CLAVE_SESSION`/`CLAVE_DATA_DIR`). Always wins,
   never second-guessed; doctor reports it as "via $CLAVE_CLAUDE_BIN".
2. `which_global(tool)`.
3. **Curated known locations**, checked for an executable file:
   - shared: `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`,
     `~/.cargo/bin` (zoxide/zellij via cargo), `/sbin` (apk)
   - claude-specific: `~/.claude/local/claude`, newest of
     `~/.nvm/versions/node/*/bin/claude`, `~/.volta/bin/claude`,
     `~/.bun/bin/claude`
   - fzf-specific: `~/.fzf/bin` (fzf's own git-install location)

**Found off-PATH ⇒ clave uses the absolute path.** This extends the existing
`runtime_binary()` idiom (stable sessions bake absolute paths precisely so
they never depend on PATH luck): `clave spawn` execs the *discovered* claude
path (resolved fresh at each spawn, not baked into the layout — the spawn
command is replayed on resurrection and must survive reinstalls), and
`clave add` invokes discovered fzf/zoxide. The user's shell config is their
business; clave just works. Doctor still surfaces a Warn so they know:

```
! claude found at ~/.claude/local/claude — not on your PATH
  clave will use this path directly; agent tabs are unaffected.
  Your interactive shell may still need it on PATH — see
  https://code.claude.com/docs (a shell alias is not enough for
  spawned processes).
```

Doctor always prints the resolved path for every tool (multiple coexisting
installs — npm *and* native — are a real footgun Claude Code's own
`claude doctor` flags): `• claude 2.1.4 (~/.local/bin/claude)`.

## Check catalogue

Every tool check is three-state via `discover()`: **on PATH** (Ok, path
shown) / **found off-PATH** (Warn — functional, clave uses the absolute
path) / **not found** (Problem + remediation).

| Group | Check | Severity when failing |
|---|---|---|
| Required tools | zellij discoverable | Problem (off-PATH → Warn) |
| | zellij version == tested | Warn |
| | claude discoverable | Problem (off-PATH → Warn) |
| | git discoverable | Problem (off-PATH → Warn) |
| Agent picker | fzf discoverable | Problem (labeled "needed by `clave add`"; off-PATH → Warn) |
| | zoxide discoverable | Problem (same) |
| clave setup | config.kdl + layout.kdl generated in data dir | Problem → "run `clave setup`" |
| | wasm present at `wasm_path()` | Problem → "run `clave setup`" (release) / "run `just dev-install`" (dev build — see Embedding) |
| | Claude hooks merged, exactly one clave entry per event (reuses `is_clave_hook_command`) | Problem → "run `clave setup`" |
| | Zellij permission cache seeded for the wasm | Warn → "run `clave setup`" (first bar load will prompt otherwise) |
| | Version skew: dev binary ahead of newest versioned copy — **only when `<data>/bin/` exists** (maintainer machinery; end users never see it) | Warn |
| Environment | `XDG_RUNTIME_DIR` set — **Linux only** | Warn (SSH session-discovery, zellij#3708) |
| | `clave --version` line (semver + build tag) | informational, always shown |

Remediation copy (hedged, flutter-voice; package-manager line only when the
manager actually probed):

```
✗ fzf not found
  It is likely available from your package manager:

      brew install fzf

  or see https://github.com/junegunn/fzf#installation
```

- **zellij**: URL-only (`https://zellij.dev/documentation/installation` +
  the GitHub releases link). It is absent from Debian/Ubuntu/Fedora repos, so
  a probed `sudo apt-get install -y zellij` would be *wrong* advice for the
  headline dependency. Suppressing the probe branch here is deliberate.
- **claude**: URL-only (`https://code.claude.com/docs`) — InstallFix ruling.
- **git/fzf/zoxide**: probed manager command + upstream URL.

Reference output (locked shape):

```
$ clave doctor

[✓] Required tools
    • zellij 0.44.3 (/opt/homebrew/bin/zellij)
    • claude 2.1.4 (~/.local/bin/claude)
    • git 2.51.0 (/usr/bin/git)

[✗] Agent picker — needed by `clave add`
    ✗ fzf not found
      It is likely available from your package manager:

          brew install fzf

      or see https://github.com/junegunn/fzf#installation
    • zoxide 0.9.6 (~/.cargo/bin/zoxide)

[!] clave setup
    • Claude hooks merged (1 entry per event)
    ✗ layout.kdl not generated — run `clave setup`

[!] Environment
    ! XDG_RUNTIME_DIR unset — zellij session discovery is
      unreliable over SSH (zellij-org/zellij#3708)

! Doctor found issues in 3 categories.
```

## Preflight integration

Scoped to what the invoked command actually needs (flutter#17781):

| Command | Checks before running |
|---|---|
| bare `clave` (launch) | zellij, claude (+ first-run setup flow below) |
| `clave add` | fzf, zoxide, git, claude |
| `clave setup` | **nothing external** — it only writes files |
| hooks / plugin-internal / dev | nothing (hook already exits 0 unconditionally) |

- Preflight prints only failures — no clean-bill banner, no prompt, never
  blocks a non-TTY run on input.
- Preflight resolves through `discover()`, so off-PATH installs pass
  silently (clave will use the absolute path) — only *undiscoverable* tools
  halt.
- Launch checks `claude` even though zellij is the thing exec'd: the eager
  tab immediately runs `clave spawn` → `exec claude`, so a missing claude
  otherwise surfaces *inside a pane* — the worst place to read an error.
  Launch execs the *discovered* zellij path for the same reason.
- **`clave add` pane-hold**: the Alt+a keybind runs add in a floating pane
  with `close_on_exit true`, so an abort's message would flash and vanish.
  On preflight failure, add prints the guidance (noting the tool can be
  installed from another tab without leaving the session), then blocks on a
  "press Enter to close" stdin read before exiting non-zero. TTY-gated: a
  non-interactive invocation skips the hold.

## First run

Bare `clave` when `config.kdl` is absent (today: `bail!("run clave setup
first")`):

```
$ clave
First run — clave needs to prepare this machine:

  • generate session config + layout in ~/.local/share/clave/
  • register status hooks in ~/.claude/settings.json (additive — your
    existing hooks are never touched)
  • pre-seed Zellij's plugin permission cache

Proceed? [Y/n]
```

- Plan is printed always; the confirm fires **only on a TTY** (Homebrew 2026
  rule) — non-TTY proceeds, because the user's invocation of `clave` *is* the
  named intent and setup is idempotent.
- Then `run_setup()` + `launch_session()` continue in-process. `clave setup`
  remains available standalone; a machine with config present never sees any
  of this.
- Preflight (zellij, claude) runs *before* the plan, so the guidance for a
  missing dep is the first thing a fresh user sees, not a mid-setup failure.
- **Upgrade refresh** (review 2026-07-22): when config EXISTS but this
  release binary's versioned wasm is not yet on disk (a binary upgrade),
  launch auto-runs the idempotent setup with a one-line notice and no
  consent prompt — the user consented to this mutation set at first run.
  Without this, an upgraded CLI would run the old bar forever (the drift
  invariant #9 kills). Running-session immunity holds: live sessions keep
  the files baked at their launch.
- **Release setup installs the versioned CLI copy and bakes through
  `runtime_binary()`** (review 2026-07-22 + codex P2 on PR #29): a
  single-file `./clave` install copies `current_exe()` to
  `<data>/bin/clave-vX.Y.Z` — the artifact `runtime_binary()` (used by
  add/open/the eager launch layout at tab-bake time) keys on — so setup and
  runtime can never disagree about which binary agent tabs run, and the
  scp'd file is disposable after setup. Dev builds keep bare `clave` (the
  sandbox flow deliberately resolves via PATH). Baking only `current_exe`
  was not enough: launch worked but every agent tab spawned bare `clave`.

## Distribution: cargo-dist + embedded wasm

**The gap this closes:** cargo-dist ships the CLI binary, but the bar wasm
had *no* distribution path — `run_setup` hard-fails with "run `just
dev-install` first", a dev-only instruction that would greet every end user.

- **Embed the wasm in release binaries.** `option_env!("CLAVE_BAR_WASM")`
  gates an `include_bytes!` of the release-built wasm (same pattern as the
  existing `CLAVE_BUILD_TAG`). Release CI builds `clave-bar` for
  `wasm32-wasip1` first, then builds the CLI with the env var pointing at the
  artifact. `clave setup` extracts the embedded bytes to the data dir as the
  versioned `clave-bar-vX.Y.Z.wasm` (which `wasm_path()` already prefers) —
  write-if-absent, so re-runs stay idempotent and a live session's loaded
  wasm is never rewritten (the §2 immunity property holds).
- Consequences, in order of value:
  1. **The entire install is one static file.** musl CLI + embedded wasm →
     `scp clave user@box:` *is* the install. The SSH requirement is met by
     construction.
  2. CLI↔wasm version drift becomes impossible in releases — invariant #9
     (anti-drift) extended from schema to artifacts.
  3. The "wasm missing" failure class disappears for end users entirely.
- Dev builds (`cargo install` from tree, no env var): no embedded wasm;
  `run_setup`'s existing ensure keeps firing with the `just dev-install`
  message, which is *correct* audience-wise there — doctor's wasm check words
  its advice by `CLAVE_BUILD_TAG` presence (release → "run `clave setup`",
  dev → "run `just dev-install`"). Sandbox flow (`CLAVE_DATA_DIR` redirect,
  unversioned wasm) is untouched.
- **cargo-dist config**: targets `aarch64-apple-darwin`, `x86_64-apple-darwin`,
  `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` (both musl targets
  are Tier 2 with host tools; Rust 1.93's musl 1.2.5 fixed the DNS footgun).
  GitHub Artifact Attestations + checksums on. Driven off the existing
  `vX.Y.Z` tag flow — composes with `just release` (which keeps preparing the
  *maintainer's* stable machine; cargo-dist prepares everyone else's).
  cargo-dist's generated shell installer ships (it's checksummed and
  attested), but the README leads with explicit binary download +
  `gh attestation verify` — no pipe-to-shell as the headline (InstallFix).
- `cargo install --git` keeps working for contributors (no wasm, sandbox
  flow) and is documented as the contributor path, not the user path.

## Testing (TDD, per CLAUDE.md)

1. `diagnose()` unit tests: hand-built `Facts` → expected `Finding`s. Every
   check, both directions. Written first, watched failing.
2. Render golden tests: a fixed `Vec<Finding>` → exact report string
   (TTY and non-TTY variants). This is what centralizing copy buys.
3. Package-manager probe order + zellij/claude URL-only suppression: pure
   over a fake probe table.
4. Zellij version parse: `"zellij 0.44.3"`, garbage, empty → tested/warn.
4b. `discover()` resolution order: override beats PATH beats known-location;
    off-PATH hit produces `via: KnownLocation` and the Warn finding; nvm
    glob picks the newest version dir. Pure over a fake filesystem/probe
    table (the codebase's `session_is_live`-style testing).
5. Hook-check reuses `is_clave_hook_command` — already covered; add the
   doctor-side "exactly one entry" test.
6. Embedding: `wasm_path()` preference untouched (existing tests);
   new test that setup's extract is write-if-absent.
7. `cargo test --workspace` — the flag is load-bearing.

Live validation (per TESTING.md, human-driven): fresh-box first-run in the
sandbox via `clave dev`, missing-fzf abort inside a real floating pane
(pane-hold behavior), SSH box without `XDG_RUNTIME_DIR`.

## Deferred (file as issues)

1. **Nerd Font / glyph check** — clave's own glyphs (`●` `✖` `◌`) are plain
   Unicode; the only exposure is whether generated Zellij config carries
   Zellij's separator glyphs. Small factual check, then decide.
2. **Homebrew tap** — revisit on demand once releases exist.
3. **`doctor --fix`** — rejected for now (one repair path); reconsider only
   if setup grows non-idempotent steps.
4. **Interactive dependency install** ("install now? Y/n") — explicitly out;
   detect + guide is the 2026-defensible stance.
