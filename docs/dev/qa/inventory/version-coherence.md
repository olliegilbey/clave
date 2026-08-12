# PATH/version coherence — per-item specs (V1–V17)

This plane is the release seam: which `clave` binary answers on PATH, which
versioned artifacts the generated KDL files reference, and which wasm actually
loads. Vocabulary (see UBIQUITOUS_LANGUAGE.md for more): the **stable install**
is `~/.local/share/clave/` — `bin/clave` (the unversioned **launcher**, owned
by the release), `bin/clave-vX.Y.Z` (versioned CLI copies), the wasm, and three
generated files: `config.kdl` + `layout.kdl` (rewritten by `just release` /
`clave setup`) and `launch.kdl` (written only by a **cold start** — a fresh
`clave` launch, the only event that picks up a release). The **sandbox** is the
per-worktree dev instance (`clave-test*` session, `~/.local/state/clave-dev*`
root). `clave_versions` / `clave_unversioned` are the RELEASE-RUNBOOK Part C
Step 0 check functions (written to `$TMPDIR/clave-release-checks.sh`). The
one-grep diagnosis for the whole plane: `grep 'clave-bar: loaded'` on
`ZLOG="${TMPDIR%/}/zellij-$(id -u)/zellij-log/zellij.log"` — every line must
report the same version AND the same `build=` tag, and the log must be
line-count-marked before launch because it is machine-shared and never
truncated. Many items here are maintainer-machine live checks by construction
(Tier 2 does not exist, #47); the drive assertions say which half automates.

### V1 — Bar shelled to bare `clave` ×7 (#44) [FIELD v0.1.1]
**Seam:** plugin → CLI shellout resolved through PATH instead of the versioned binary the session was launched with; any stale binary answering `clave` hijacks the running fleet.
**Preconditions:** a stale `clave` earlier on PATH than the launcher (v0.1.1: a `0.1.0` dev build at `~/.cargo/bin/clave`); pre-#66 bar calling `run_command(&["clave", …])` in 7 places.
**Reproduce:** historical: in a v0.1.1 session, the bar's `clave open` ran the 0.1.0 binary, which composed a tab layout pointing at `clave-bar-v0.1.0.wasm` — plugin identity is `(location, configuration)`, so a second, different bar loaded.
**Healthy:** every shellout uses `self.clave_binary` from plugin config (`crates/clave-bar/src/main.rs:434` `resolve_binary`); one loaded version in the log.
**Broken:** verbatim log: `16:27:58 [id: 1] clave-bar: loaded v0.1.1 build=dev` then `16:28:04 [id: 2] clave-bar: loaded v0.1.0 build=dev` — TWO plugin populations, duplicate sidebar, half-dead nav, CliPipe timeout flood.
**Drive assertion:** unit: literal-count pin — `main.rs` must contain exactly ONE bare `"clave"` literal (`crates/clave-bar/src/plugin_config.rs:92`; counting, because absence-asserts survived three live mutations). Live: runbook Part C Step 3 log grep, one version+tag.
**Guard today:** `clave_binary` plugin-config key; literal-count test; runbook C Step 3.
**Refs:** #44 (fixed in #66), #43; FOOTGUNS:60, :106; TESTING.md escape record row 1.

### V2 — No unversioned stable entry point (#43a) [FIELD]
**Seam:** "launch the version I just released" had no owned answer — whatever `clave` resolved to won the cold start and baked ITS version into `launch.kdl`.
**Preconditions:** a cut installed only `bin/clave-vX.Y.Z`; PATH resolution decided the launcher.
**Reproduce:** historical (same incident as V1): stale dev binary cold-started and generated a mixed-version `launch.kdl` (`clave-bar-v0.1.0.wasm` + `clave-v0.1.0`) inside a v0.1.1 install.
**Healthy:** the release installs/refreshes the unversioned launcher (`crates/clave/src/release.rs:83` `install_launcher`, staged rename) and `command -v clave` resolves inside `~/.local/share/clave/bin/`.
**Broken:** mixed-version `launch.kdl`; two bars on next cold start.
**Drive assertion:** unit: `generated_artifact_set_is_version_coherent` (`crates/clave/src/setup.rs:1470`) and `released_artifacts_exist_and_the_launcher_is_never_baked` (`release.rs:659`). Live: runbook Step 1 (`command -v clave; clave --version`) — HUMAN until #48.
**Guard today:** release owns `bin/clave`; the two coherence tests; #48 (doctor sees PATH) still open.
**Refs:** #43a; RELEASE-RUNBOOK Part C Step 1; TESTING.md escape record row 2.

### V3 — dev-install wrote `~/.cargo/bin/clave` (#43b) [FIELD]
**Seam:** the dev CLI installed under the exact name the daily surface answers to, so a working-tree build silently took over the fleet.
**Preconditions:** pre-#43b `just dev-install`; `~/.cargo/bin` on PATH ahead of the launcher (it was — nothing else ever put the launcher dir there).
**Reproduce:** historical: dev-install during daily driving; the next cold start ran the working-tree build (V1/V2's trigger).
**Healthy:** `just dev-install` installs `~/.cargo/bin/clave-dev` via staged rename (`justfile:93`, `.clave-dev.$$.tmp` → `mv -f`); `command -v clave` never names `~/.cargo/bin`.
**Broken:** `command -v clave` → `~/.cargo/bin/clave` disagreeing with the release.
**Drive assertion:** `command -v clave` must not print `~/.cargo/bin/clave`; runbook Step 1's STOP row is the live form (with the five-step ordered recovery — see V16). A surviving pre-#43b copy shadows the launcher: identify (`command -v clave; clave --version`) before removing.
**Guard today:** `clave-dev` name; runbook Step 1 STOP row.
**Refs:** #43b; FOOTGUNS:107; RELEASE-RUNBOOK Step 1 table.

### V4 — dev-install leaves sandbox config stale (#31, OPEN) [SANDBOX]
**Seam:** `just dev-install` installs wasm + CLI but never regenerates the sandbox `config.kdl` (only `dev scenario` calls `run_setup`), so a fresh `launch.kdl` pairs with an old `config.kdl` and every keybind misses.
**Preconditions:** sandbox previously set up; then `just dev-install` followed by `clave dev launch` without an intervening `clave dev scenario`.
**Reproduce:** 1. `just dev-install`. 2. `clave dev launch` (human). 3. Press any clave keybind.
**Healthy:** keybinds hit the on-screen bar (config and launch bake the same plugin identity).
**Broken:** keybind starts a SECOND bar — indistinguishable from "the fix didn't work"; it produced a false negative during #44's own live validation.
**Drive assertion:** pre-launch: both sandbox `config.kdl` and `launch.kdl` carry an identical `clave_binary` value (the #44 identity pair `just sandbox` self-checks). Post-launch second-bar count: `HUMAN-ONLY: one bar per tab`.
**Guard today:** `just sandbox` is the only path yielding a coherent sandbox; `dev-install` remains unsafe against a live `clave-test`. Issue open.
**Refs:** #31; FOOTGUNS:108; TESTING.md sandbox lifecycle notes.

### V5 — Sandbox was a machine-wide singleton (#161) [SANDBOX]
**Seam:** two agents' `just sandbox` runs clobbered one shared staging dir — the second's artifacts won, and the first's live checklist measured the WRONG build for a full round (old bug "reproduced", new fix "didn't work", both someone else's wasm). The copy even ran before the live-session guard, so a REFUSED rewire still mutated `data/`.
**Preconditions:** two concurrent agent sessions staging different branches (pre-fix).
**Reproduce:** historical; the residue that survives: `just dev-install` still hardcodes the MAIN sandbox path (`justfile:89-91`), so running it from a worktree writes the main sandbox — reason `just sandbox` is the reviewed path.
**Healthy:** session name, state/data/shim dirs all derive from one key, the worktree directory name (`crates/clave/src/sandbox.rs:32-35`, `key_for`; main checkout keeps bare `clave-test`/`clave-dev`).
**Broken:** a mixed dir — this branch's wasm beside that branch's `config.kdl`.
**Drive assertion:** `clave dev instance` prints a worktree-keyed session/root from a worktree; and drive-loop step 3 unchanged — before reading any live result, tail `$ZLOG` `clave-bar: loaded` lines and match YOUR `build=` tag (never `grep -c`: a re-run at the same HEAD leaves an older line with the same tag).
**Guard today:** per-worktree instance; step-3 build-tag proof.
**Refs:** #161 (merged); FOOTGUNS:110; TESTING.md "Your sandbox is not necessarily clave-test".

### V6 — Doctor can't see PATH (#48, OPEN)
**Seam:** `clave doctor`'s skew check compares its own version against installed artifacts only — it cannot see what `clave` resolves to on PATH, which is exactly the v0.1.1 mechanism.
**Preconditions:** any PATH shadowing incident (V1/V3 shape) with a healthy install underneath.
**Reproduce:** with a stale binary shadowing the launcher, run `clave doctor` — pre-#48 it reports "no issues" during exactly the incident the runbook hunts.
**Healthy (target state, #48):** `clave doctor --json` exits non-zero on incoherence, collapsing runbook Steps 1–3 into one unattended assertable command.
**Broken:** doctor green while `command -v clave` disagrees with the release.
**Drive assertion:** `HUMAN-ONLY: runbook Step 1 (command -v clave; clave --version; ls -l ~/.local/share/clave/bin/) — both lines verbatim`. Nothing automated exists.
**Guard today:** nothing — Steps 1–3 manual; the release-skew check that does exist is `crates/clave/src/doctor.rs:366` onward (warn-only, artifact-scoped).
**Refs:** #48 (open); FOOTGUNS:117; RELEASE-RUNBOOK "Why this is manual".

### V7 — `runtime_binary()` fell back to bare `clave`
**Seam:** the CLI's self-reference probed only for its OWN version's copy under `bin/` and silently fell back to bare `clave` — diverging from the versioned path `config.kdl` baked (#44's divergence, host-side).
**Preconditions:** the running version's `bin/clave-v<self>` copy missing (partial install, manual deletion).
**Reproduce:** remove `~/.local/share/clave/bin/clave-v$(clave --version | cut -d' ' -f2)`; run a composing command.
**Healthy:** the anomaly is loud: `binary_resolution_is_anomalous` (`crates/clave/src/release.rs:184`) warns and names the repair; `runtime_binary` (`release.rs:201`) result matches what config bakes.
**Broken:** silent divergence — generated output references a binary that PATH, not the install, resolves.
**Drive assertion:** `cargo test --workspace` (`binary_resolution_is_anomalous` cases, `release.rs:490` onward); no longer silent so the warning text is the live detector.
**Guard today:** anomaly warning naming the repair.
**Refs:** FOOTGUNS:118; #44 lineage.

### V8 — `clave-v` prefix matched `clave-vault`
**Seam:** sibling detection by `starts_with("clave-v")` — a foreign `clave-vault`/`clave-verify` in `bin/` fires the divergence warning with no real mismatch, and a wolf-crying warning trains the reader to ignore the one that matters.
**Preconditions:** any non-clave binary named `clave-v*` in `~/.local/share/clave/bin/` (or a hook command referencing one).
**Reproduce:** drop an executable named `clave-vault` into `bin/`; pre-fix, the anomaly warning fires.
**Healthy:** both sites require a DIGIT immediately after `clave-v` (`crates/clave/src/setup.rs:354`; `release.rs:184-199`).
**Broken:** spurious divergence warning / a foreign hook command treated as ours.
**Drive assertion:** `cargo test --workspace` — the pinned negative cases (`setup.rs:1263` `clave-vault hook Stop`; `release.rs` sibling cases).
**Guard today:** digit-required check, unit-pinned at both sites.
**Refs:** FOOTGUNS:119.

### V9 — Warm hook migration is impossible (v0.1.2 cut) [FIELD]
**Seam:** release-time vs process-lifetime — `clave release` writes new versioned hook commands into `~/.claude/settings.json`, but every already-running Claude keeps firing the PREVIOUS binary, so the process writing the store is the one that lacks the new field.
**Preconditions:** a release adding a store field the hooks populate (v0.1.2 instance: `AgentRecord::live_session`, #99), with agents alive across the cut.
**Reproduce:** cut while a fleet is live; prompt a live agent. Observed 2026-07-31: store moved, status changed — hooks fire — yet `live_session` stayed null on all 18 rows, and `atime` landed on the old binary.
**Healthy:** the cold restart IS the migration; release notes say plainly what a row loses by carrying a default through it. Null is ALSO the healthy value of a never-rotated row — do not read "null ×18" as "18 rows need backfilling" (that misread became a P1 once).
**Broken:** old binary read-modify-writes the new store for the whole window; any "prompt each agent to migrate" procedure written into the runbook (one was written and cut again the same hour).
**Drive assertion:** `HUMAN-ONLY / process rule: no warm-migration step exists in the runbook; post-restart, hook-written new fields populate`. Detection probe: a new-binary-only store field staying default fleet-wide while agents are live is the expected pre-restart state, not a defect.
**Guard today:** RELEASE-RUNBOOK Part B step 3 (verbatim rule); never backfill by guessing; real fix is the versioned store schema (#106).
**Refs:** FOOTGUNS:123, :194; RELEASE-RUNBOOK Part B step 3.

### V10 — Relaunching the old binary is not a rollback
**Seam:** launch semantics — `clave` *attaches* to a live session (`crates/clave/src/setup.rs:720` `launch_session`, attach-or-create), so nothing is re-read; and once the session is dead, `needs_version_refresh` (`setup.rs:709`) returns false because the old binary's own wasm still exists, so setup is skipped and `config.kdl` keeps naming the failed version.
**Preconditions:** a failed cut (any Part C STOP); the instinctive recovery attempt of just running the old versioned binary.
**Reproduce:** after a STOP, run `~/.local/share/clave/bin/clave-v<LAST-GOOD>` without step 3 below — the session that appears still carries the failed version's generated set.
**Healthy:** Rollback in order: capture evidence → human kills the session → **explicit** `~/.local/share/clave/bin/clave-v<LAST-GOOD> setup` (regenerates the set against itself) → relaunch → re-verify both file sets.
**Broken:** a "rolled back" session still loading the failed version's wasm/config.
**Drive assertion:** post-rollback: `clave_versions "${STABLE[@]}" "${LAUNCH[@]}"` prints exactly `v<LAST-GOOD>`; `clave_unversioned` prints nothing. Session kill and relaunch: HUMAN.
**Guard today:** Rollback step 3 (explicit setup) in the runbook.
**Refs:** RELEASE-RUNBOOK Rollback; FOOTGUNS:116.

### V11 — `launch.kdl` stale by design; pre-launch asserts fail closed — bitten twice
**Seam:** artifact lifecycle — only a cold start writes `launch.kdl` (`setup.rs:720`; path helper `setup.rs:50`); `just release` writes `config.kdl` + `layout.kdl` only (`write_generated`, `setup.rs:498`). Any pre-launch check including `launch.kdl` STOPs a healthy upgrade.
**Preconditions:** a version-coherence check with a `*.kdl` glob run between the cut and the cold start.
**Reproduce:** run `clave_versions` over all four KDLs right after `just release` — `launch.kdl` still names the previous version and the set reads as skew. Bitten twice: the runbook's original glob, and `scripts/sandbox-setup.sh`'s #44 identity check reading a six-day-old `launch.kdl`.
**Healthy:** Step 2 checks `$STABLE` (excludes `launch.kdl`); Step 3 asserts `$LAUNCH` only AFTER the cold start, with both checks including the zero-versions row (empty = never written, not a pass).
**Broken:** ordered STOP on a healthy cut — or, inverted, a stale pre-launch file blessed as current.
**Drive assertion:** pre-launch `clave_versions "${STABLE[@]}"` → one line == tag; post-cold-start `clave_versions "${LAUNCH[@]}"` → one line == tag, `clave_unversioned "${LAUNCH[@]}"` → empty. Cold start itself: HUMAN.
**Guard today:** Step 2/Step 3 split in the runbook; the checks live in one sourced file so the two copies cannot drift.
**Refs:** FOOTGUNS:113-114; RELEASE-RUNBOOK Steps 0/2/3.

### V12 — Version COUNT blind to unversioned refs
**Seam:** the "exactly one version" probe dedupes versioned matches — a file holding one correct versioned path PLUS a bare `clave` still reports one version and passes, while the bare reference loads a second independent plugin singleton.
**Preconditions:** any generated KDL carrying an unversioned reference at one of the three sites: `clave_binary "clave"`, a keybind's `Run "clave"`, or `file:…clave-bar.wasm` (unversioned wasm).
**Reproduce:** add `Run "clave"` to a keybind in `config.kdl`; `clave_versions` still prints one version.
**Healthy:** `clave_unversioned` (runbook Step 0) targets those three sites explicitly and prints NOTHING; reports must say the word "empty" so a silent failure and a clean pass cannot look alike.
**Broken:** any `clave_unversioned` output — as fatal in `launch.kdl` as in `config.kdl`.
**Drive assertion:** `clave_unversioned "${STABLE[@]}"` and (post-launch) `"${LAUNCH[@]}"` → both empty. Fully scriptable given the check file.
**Guard today:** the `clave_unversioned` probe + the empty-is-never-a-pass reporting rule.
**Refs:** FOOTGUNS:115; RELEASE-RUNBOOK Step 0/2/3 tables.

### V13 — `start-or-reload-plugin` without `-c clave_binary` starts a second bar
**Seam:** zellij plugin identity is `(location, configuration)` compared exactly — a reload whose config map omits `clave_binary` misses the lookup, and the miss STARTS A NEW PLUGIN rather than no-oping.
**Preconditions:** a live session with the bar loaded under a `clave_binary` config; the one sanctioned live mutation (sandbox hot-reload) issued without `-c`.
**Reproduce:** `scripts/ct.sh start-or-reload-plugin "file:$SB_DATA/clave-bar.wasm"` (no `-c`) against a live sandbox.
**Healthy:** `scripts/ct.sh start-or-reload-plugin "file:$SB_DATA/clave-bar.wasm" -c clave_binary=clave` — reload in place (sandbox value is literally `clave`; a stable session needs its versioned absolute path).
**Broken:** `Plugin {} not found, starting it instead` in `$ZLOG`, then a second bar pane — the very #44 symptom, while appearing to succeed.
**Drive assertion:** mark `$ZLOG`; reload; appended lines contain no `not found, starting it instead`; instance count after == before (a surprising instance count is a session-identity failure first).
**Guard today:** the sanctioned command spells the `-c` (TESTING.md); doc-level only.
**Refs:** FOOTGUNS:60, :120; TESTING.md "Hot-reload the sandbox bar" (with the wasm_bridge/plugin_map citation chain).

### V14 — `build=dev` / stray working-tree wasm (#109; #167 OPEN sibling)
**Seam:** two builds of the SAME version are indistinguishable by version alone — the `build=` tag is the discriminator, and the release recipe once set `CLAVE_BUILD_TAG` for the CLI but not the wasm, so every released sidebar reported `build=dev`.
**Preconditions:** a cut via a recipe that misses the tag on the wasm line, or a stray working-tree wasm copied into the stable install.
**Reproduce:** historical (#109): post-v0.1.2 log showed `clave-bar: loaded v0.1.2 build=dev` while dev-installs showed proper short-SHA tags — backwards. #167 is the same class alive in `dist-build`.
**Healthy:** both build lines in the `release` recipe export the tag (`justfile:119-120`, `git describe --tags --exact-match`); a released bar logs `build=vX.Y.Z` exactly.
**Broken:** `build=dev` on an otherwise-correct line — runbook Step 3 makes it a STOP (`dev` = untagged local build; a bare short SHA = a dev-install artifact; both are the stray-wasm case).
**Drive assertion:** post-cold-start: appended `$ZLOG` `clave-bar: loaded v` lines all report version == tag AND `build=` == the exact tag being cut. Read the tail after launch (tail-read rule, V5), never `grep -c`.
**Guard today:** fixed recipe; Step 3 table row; tail-read rule. #167 (dist-build wasm untagged) still open.
**Refs:** #109 (closed), #167 (open); RELEASE-RUNBOOK Step 3; FOOTGUNS:121.

### V15 — dist fragment executed by GitHub (#67)
**Seam:** cargo-dist generated a workflow *fragment* into `.github/workflows/`, where GitHub executes anything — the fragment failed on every push and normalised red CI.
**Preconditions:** a cargo-dist (re)generation targeting `workflows/`; recurs on dist upgrades because dist regenerates the file.
**Reproduce:** Repro unknown — detection only: a `build-wasm-setup.yml` check failing on every push with no code cause.
**Healthy:** the fragment lives at `.github/build-wasm-setup.yml` (outside `workflows/`) — verified present there on this tree.
**Broken:** `.github/workflows/build-wasm-setup.yml` exists and runs red on every push.
**Drive assertion:** `test ! -e .github/workflows/build-wasm-setup.yml && test -f .github/build-wasm-setup.yml` — scriptable, and worth running after every dist upgrade.
**Guard today:** moved out by #67; a re-check-per-dist-upgrade note, no automated pin.
**Refs:** #67; FOOTGUNS:137.

### V16 — PATH recovery in wrong order → `command not found` (v0.1.2 cut) [FIELD]
**Seam:** recovery-step ordering in the shell — uninstalling the shadowing binary before the launcher dir is on PATH (and before `hash -r`) removes the only `clave` the shell can resolve.
**Preconditions:** runbook Step 1's STOP row: `clave` resolving to `~/.cargo/bin/clave`; a healthy install underneath.
**Reproduce:** run `cargo uninstall clave` before prepending `~/.local/share/clave/bin` to PATH — the v0.1.2 cut did, on a healthy install.
**Healthy:** the five-step ordered recovery: (1) PREPEND the launcher dir in shell config (append loses to `~/.cargo/bin`); (2) reload (`exec $SHELL`); (3) confirm (`clave --version` + `command -v clave` inside the launcher dir); (4) only then `cargo uninstall clave` (or `rm`); (5) `hash -r` — both bash and zsh cache resolved paths, so without it the shell keeps targeting the deleted file.
**Broken:** `zsh: command not found: clave` on a healthy install; or step 5 skipped, so the recovery "looks broken in exactly the place it just worked".
**Drive assertion:** `HUMAN-ONLY: Step 1 recovery is the human's shell; verdict is command -v clave resolving inside ~/.local/share/clave/bin and clave --version == tag`.
**Guard today:** Step 1's ordered recovery text in the runbook; nothing mechanical.
**Refs:** RELEASE-RUNBOOK Step 1 STOP row (the ordered recovery is spelled there verbatim); docs/dev/qa/BREAKAGE-INVENTORY.md V16.

### V17 — ct.sh never once worked (`$TMPDIR` trailing slash) (PR #152)
**Seam:** macOS `$TMPDIR` carries a trailing slash and zellij's argv paths do not — the naive interpolation gives `…/T//zellij-<uid>/…`; a `-S` socket test folds the double slash and passes, but the liveness `pgrep` matches argv as TEXT and never matches. The wrapper fails CLOSED, so total breakage read as "the sandbox is not running" — it fails closed into looking correct.
**Preconditions:** macOS; any `scripts/ct.sh` invocation against a genuinely live sandbox (pre-fix).
**Reproduce:** re-introduce `"$TMPDIR/zellij-$(id -u)"` in place of the normalised form; every `ct.sh` command refuses with the not-running message while `zellij list-sessions` shows the sandbox live.
**Healthy:** `TMP="${TMPDIR:-/tmp}"; SOCKET_ROOT="${TMP%/}/zellij-$(id -u)"` (`scripts/ct.sh:82-83`), the `contract_version_*` glob (`:103` — never pin `_1`), and exactly-one-socket demanded (two contract dirs each holding the session would make the vouched path and the name-resolved target different things — refuse, don't guess).
**Broken:** every command refused on every macOS machine, since the script's first day.
**Drive assertion:** against a live sandbox: `scripts/ct.sh list-panes -t -j` succeeds; with the sandbox dead: it refuses with the explicit not-running message (exit nonzero) rather than falling through to the ambient session. Both halves scriptable given a human-launched sandbox.
**Guard today:** `${TMPDIR%/}` normalisation + glob + exactly-one check, in the script itself.
**Refs:** PR #152 (found in review, 2026-08-10); FOOTGUNS:223; TESTING.md "Agent-side sanctioned commands".
