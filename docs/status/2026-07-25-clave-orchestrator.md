# Status — clave orchestrator (#44 review-complete, awaiting merge + live pass)

_2026-07-25 · repo github.com/olliegilbey/clave · branch `fix/plugin-binary-path`
(13 commits off `50fa26a`) · PR #66 open · PR #67 open and CLEAN · not merged_

Predecessors:
- @docs/status/2026-07-23-0956-clave-orchestrator.md — the #44 implementation
  session: the identity-pair mechanism, what was declined, the rulings.

## Where it stands

**#44 is implemented, eight review passes deep, and green.** Two PRs are open
and both need the maintainer:

- **#67** — `fix(ci)`: moves a cargo-dist step *fragment* out of
  `.github/workflows/`, where GitHub kept trying to run it as a workflow and
  failing at 0s on every push (it fails on `main` too — pre-existing, unrelated
  to #44). **CLEAN and ready to merge.** Merging it first makes #66's checks
  readable.
- **#66** — the #44 fix itself. `needs-live-validation` labelled. Awaiting merge
  approval and the sandbox pass.

Neither has been merged: the autonomy contract gates merges on maintainer
approval, and none was given.

## What this session changed

### 1. A verification escape — the docs were wrong, not just the run

PR #66's CI `lint` job was **failing** while the previous session reported "all
gates green". Cause: CI's lint job is `cargo fmt --all --check` **then** clippy,
but `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md` and `TESTING.md` all documented
only three gates and **never mentioned fmt at all**. Any agent following the
docs passes locally and fails CI.

Fixed both layers: rustfmt'd the three hand-edited files, added **`just gates`**
(all four commands, CI's order, one recipe), and cited it in all four docs with
the escape recorded. `.github/workflows/ci.yml` was audited end to end — fmt was
the only divergence.

### 2. Three guard escapes closed (fresh whole-branch review)

The source-text shellout guard asserted the *absence* of `"clave",`. A review
proved it blind to three mutations that survived the full gate set:

- a **byte-exact revert** of the prune-tabs site (its pre-#44 text was
  `vec!["clave".into(), …]` — no comma after the quote),
- **variable indirection** (`let cli = "clave";`),
- **disabling the feature outright** in `load()`.

It now *counts* the bare `"clave"` literal (exactly one — the PATH fallback) and
pins that `load()` still feeds its configuration to `resolve_binary`. All three
mutations verified caught, then reverted.

### 3. THE THING TO KNOW BEFORE RELEASING — `config.kdl` is live-watched

Zellij watches the `--config` file of every **running** session and hot-swaps
its keybinds in place (`zellij-server src/lib.rs:2175` → `ConfigWrittenToDisk`
`:2298` → `ScreenInstruction::Reconfigure` `screen.rs:717`, ~1s poll). The
running bar's identity is **not** swapped — it is
`initial_userspace_configuration`, fixed at load.

Since #44 the keybinds carry `clave_binary`. So regenerating `config.kdl`
against a live session re-keys its keybinds to an identity the on-screen bar
does not have; the next Alt+c/Alt+j/Alt+o misses, and zellij's response to a
miss is to **start a new plugin**. Second sidebar in every tab, dead navigation
— verbatim #43/#44, *caused by installing the fix for it*.

**Operational rule, now in CONTRIBUTING "The one leak" and the TESTING live
SOP:** any `just release` / `clave setup` / `clave dev scenario` that changes
`clave_binary` or the wasm path **requires restarting every affected session**.
Kill and relaunch before pressing any clave key. This bites hardest exactly once
— on the pre-#44 → post-#44 upgrade.

Verified independently from fetched zellij-server 0.44.3 source, not taken on
the reviewer's word.

### 4. `just sandbox` — safe sandbox validation

`scripts/sandbox-setup.sh` + `just sandbox [scenario]`. The safe replacement for
`just dev-install` in sandbox work: it never writes `~/.cargo/bin/clave` (the
name a live session's plugin shells out to — the 2026-07-22 outage) and verifies
that at the end.

The non-obvious part is the **PATH shim**. The sandbox data dir holds no
versioned CLI copy, so generation bakes bare `clave` and the bar resolves it
through PATH at runtime — which without a shim is the *stable*
`~/.cargo/bin/clave`. That binary is currently **v0.1.1 with zero occurrences of
`clave_binary`**: it predates the fix, yet reports the *same version string* as
the fix build. So the usual `clave-bar: loaded vX.Y.Z` log-grep diagnostic would
show "all one version, looks fine" while the sandbox silently ran pre-fix code
and sprouted a second bar — a false negative that reads as the fix failing.

The script also self-checks the #44 identity pair before the human spends any
time in a terminal, and **refuses to run against a live `clave-test`** (§3).

## Rulings and decisions

- **fugu is RECOMMENDED, not required** (maintainer, 2026-07-23; AGENTS.md
  updated in #66). Independent adversarial review remains required.
- **Handoffs and the validation ledger are IMMUTABLE history** — doc sweeps
  correct live SOP only, never a dated record of what was run.
- **Anomaly-predicate widening stays deferred to #48.** The review sharpened the
  case: `run_release` never prunes old versioned copies, so once two exist the
  detector goes blind in exactly its own scenario (`installed = true`, no
  warning, mismatched bake). Recorded as a comment on #48.
- **Commits this session carry Claude's signature.** 1Password's SSH agent was
  down (`ssh-add -l` → "The agent has no identities"), which blocked both
  signing and pushing; the maintainer authorised the hook's Claude-signed
  fallback while AFK. Pushes went over `gh`'s HTTPS token, because global git
  config rewrites HTTPS→SSH (`url.git@github.com:.pushinsteadof`) and would
  otherwise force the dead agent.

## Next steps

1. **Merge #67** (clean, unrelated to #44, makes #66's checks readable).
2. **Merge #66** once CI is green — needs maintainer approval.
3. **Live-validate** with `just sandbox`, then the numbered steps in #66's
   dossier. Read §3 above first: kill the session before regenerating.
4. **#47 — tier-2 real-zellij harness.** Unblocked by #44, which supplies the
   binary-injection point. First scenario is the #44 regression: one bar per tab.
   It is also the only thing that will ever properly cover the seven shellouts;
   the source-text guard is a stopgap.
5. **#48** — doctor version-coherence, now carrying the sharpened predicate gap.

## Observations not acted on

- **The toolchain is unpinned.** CI uses `dtolnay/rust-toolchain@stable`, so a
  new stable release can turn an unrelated PR red via new clippy lints or a
  rustfmt change. Local is 1.96.1 and matches today. A `rust-toolchain.toml`
  would make it deliberate; not done — no evidence it has bitten yet.
- **`layout.kdl` is generated but never handed to zellij** (only `doctor` checks
  it exists). Still worth its own issue; still not a drive-by deletion.

## Restart hint

Branch pushed, both PRs open, tree clean. The SDD ledger with the per-task
record is at `.superpowers/sdd/progress.md` (gitignored — worktree only).
Resume by checking `gh pr checks 66` and whether 1Password is back up.
