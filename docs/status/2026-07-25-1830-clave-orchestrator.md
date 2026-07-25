# Status — clave orchestrator (#43a/#43b implemented, PR open, live pass owed)

_2026-07-25 · repo github.com/olliegilbey/clave · branch
`fix/release-owns-the-launcher` (off `fd13c26`, i.e. post-#44) · PR open · not
merged_

Predecessors:
- @docs/status/2026-07-25-clave-orchestrator.md — the #44 review-complete state.
  #44 has since merged as `fd13c26`, which is this branch's base.
- @docs/status/2026-07-22-1845-clave-orchestrator.md — the field incident itself.

## Task overview

**#43** — the other two halves of the v0.1.1 outage. #44 stopped the bar
*resolving* `clave` through `PATH`; #43 is about who owns the *name*.

- **#43a** — the cut installs no unversioned entry point, so "how do I launch
  the version I just released?" has no answer and whatever `clave` resolves to
  on `PATH` wins the cold start.
- **#43b** — `just dev-install` wrote `~/.cargo/bin/clave`, the daily launcher's
  exact name. That is the file that won, at version 0.1.0, inside a v0.1.1
  session.

## What shipped

**#43a — the release owns a launcher.** `clave release` now installs and
*refreshes* `<data>/bin/clave` as a copy of the versioned copy it just
installed. `<data>/bin` becomes the directory an operator puts on `PATH`. Three
properties are deliberate and tested:

- **Refresh, not write-if-absent.** The opposite of `install_cli_copy` /
  `extract_embedded`, and for a reason that is the inverse of theirs: those are
  write-if-absent because a live session is loading those exact files (§2
  running-session immunity). *Nothing* loads the launcher — it is only typed —
  so a stale one would simply relocate #43a into the data dir.
- **Installed by rename, never `fs::copy` over the live name.** Copy truncates
  the existing inode, and that inode can be a running process image: ETXTBSY on
  Linux (a half-installed release), a live text segment on macOS. `rename`
  swaps the directory entry only.
- **Typed, never baked.** Every generated reference stays versioned. An
  unversioned plugin location is a *different plugin identity* to zellij — the
  #43 duplicate-sidebar shape, re-entered through the fix.

The single-file install path (`clave setup` from a release binary, #29) installs
the launcher too; without it, `install_cli_copy`'s claim that "the scp'd file
becomes disposable after setup" was only ever true of baked references.

**#43b — `just dev-install` installs `clave-dev`.** `cargo install --path`
names the binary after the package, so the recipe now builds (`--locked` kept)
and stages + `mv`s to `~/.cargo/bin/clave-dev`. The `mv` is the same
running-inode reasoning as the launcher.

**Naming** (UBIQUITOUS_LANGUAGE §4 gained three rows): **launcher** =
`<data>/bin/clave`, typed, refreshed per cut. **versioned copy** =
`<data>/bin/clave-vX.Y.Z`, baked, immutable. **dev binary** =
`~/.cargo/bin/clave-dev`. The three no longer share a name; that is the fix.

## Tier-1 coverage added (`crates/clave/src/release.rs`)

| Test | Asserts |
|---|---|
| `release_artifacts_names_the_versioned_pair_and_the_unversioned_launcher` | the pure kernel's three destinations; the launcher carries no version and sits beside the versioned copy |
| `install_launcher_refreshes_on_every_cut` | second install overwrites (refresh semantics), mode 0755, and `bin/` is left with no staging debris |
| `install_launcher_replaces_the_directory_entry_rather_than_truncating` | the inode changes across a refresh — the running-process property |
| `the_installed_launcher_actually_runs` | the installed file executes and produces its output |
| `the_launcher_is_never_read_as_a_versioned_copy` | `binary_resolution_is_anomalous` does not fire on a bare `clave` sibling, but still fires when a versioned copy sits beside it |
| `launcher_hint_names_the_bin_dir_and_the_shadow_that_caused_the_outage` | the printed hint names the `PATH` dir and the stale `~/.cargo/bin/clave` |
| `released_artifacts_exist_and_the_launcher_is_never_baked` | #48's cheap companion from the install side: every path the generated KDL references is a file a cut installs, and none of them is the launcher |

`generated_artifact_set_is_version_coherent` (setup.rs, from PR #52) already
covers the *version-agreement* half of #48's companion; this branch adds the
*path-existence* half plus the never-bake-the-floating-name rule. The live
five-way coherence check remains #48.

## What is NOT verified

`run_release` end to end. It gates on a clean tree at an exact `vX.Y.Z` tag, and
an agent must never cut a release or write `~/.local/share/clave/`. So the
orchestration — gate → install versioned pair → regenerate → install launcher →
print hint — is composed of tested units but has never executed as a whole.
**The maintainer's next cut is the first real run of this code.** The PR carries
`needs-live-validation` and lists what to look for.

`just dev-install` was likewise not executed (it writes `~/.cargo/bin`); the
recipe was validated by `just --dump` only.

## Follow-ups worth filing

1. **The sandbox still resolves bare `clave` through a `PATH` shim.**
   `run_setup`'s dev branch bakes `"clave"`, and `just sandbox` supplies that
   name from `$SANDBOX/shim`. Now that the dev binary has its own name, the
   sandbox could bake `clave-dev` directly and delete the shim — one fewer
   PATH-shaped mechanism in the environment that PATH broke.
2. **`clave doctor` says nothing about the launcher.** It reads `<data>/bin` for
   `installed_releases` (the launcher is correctly ignored there) but never
   reports whether a launcher exists, which version it is a copy of, or whether
   it is the `clave` that `PATH` actually resolves. That is #48's territory and
   is now the highest-value part of it.
3. **A stale `~/.cargo/bin/clave` still wins on `PATH`.** Nothing can delete it
   for the operator; the release now prints the instruction. Doctor should
   assert it (also #48).
