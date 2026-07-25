# Status — clave orchestrator (#44 implemented, PR open, awaiting merge + live validation)

_2026-07-23 09:56 · repo github.com/olliegilbey/clave · branch
`fix/plugin-binary-path` (9 commits off `50fa26a`) · PR open · not yet merged_

Predecessor — read it for the pre-#44 state and the day-one incident:
- @docs/status/2026-07-22-1845-clave-orchestrator.md — v0.1.1, the field
  incident, the autonomy rulings, and the original (now-superseded) #44 spec.

## Task Overview

clave = Zellij fleet-orchestration sidebar (wasm `clave-bar` + `clave` CLI).
This session implemented **#44**: the bar shelled out to bare `clave` through
PATH at seven sites, so any stale binary could hijack a running session — the
root cause of the v0.1.1 duplicate-sidebar incident. The fix passes the resolved
absolute binary into the plugin as zellij *configuration* and the bar uses it
for every shellout.

## Where it stands

**Done:** the branch is implemented, reviewed, and pushed. Nine commits,
`50fa26a..17af37c`. All three gates green (`cargo test --workspace`,
`cargo build -p clave-bar --target wasm32-wasip1`,
`cargo clippy --workspace --all-targets -- -D warnings`).

**Open:** the PR awaits (a) green CI, (b) the maintainer's merge approval, and
(c) a `needs-live-validation` sandbox pass — see the PR's numbered steps. The
agent may execute the merge once the maintainer approves; it never merges
unasked.

## What shipped, and the one thing that made it non-trivial

The handoff spec from the previous session proposed passing the binary into the
layout's plugin node and changing nothing else. **That was unsafe as written.**
Verified against vendored zellij-utils 0.44.3 and fetched zellij-server 0.44.3:

- zellij matches a pipe's destination on `(location, configuration)` as an
  EXACT pair (`wasm_bridge.rs:1676-1686`), and a miss **launches a new plugin**
  (`:1861-1894`) rather than no-op'ing.
- Our `MessagePlugin` keybinds address the plugin by location with an empty
  configuration. Emitting `clave_binary` into the layout node *only* would have
  made every keybind miss and spawn a second bar — the very bug #44 exists to
  kill, re-triggered by Alt+c instead of by opening a tab.

So the value is emitted into **both** halves of the identity pair — the three
layout emitters AND every `MessagePlugin` keybind — and a hermetic guard test
(`keybind_and_layout_plugin_configurations_match`) proves they agree. The key
lives in `clave_types::CLAVE_BINARY_KEY` so the CLI emitter and the wasm reader
cannot drift.

Commit map:
- `35c2436`/`8930015` — emit `clave_binary` from six sites + the guard test
  (guard made deterministic after review found a HashMap-order flake).
- `b1ed0fa` — bar reads the key, all seven shellouts converted. `resolve_binary`
  + guard live in `plugin_config.rs`, not `main.rs`: the bin is `test = false`
  (can't host-link), so tests in `main.rs` would silently never run.
- `ed7c457`/`e7c5540` — `runtime_binary()` announces the divergence anomaly;
  digit-after-`clave-v` discipline added so a foreign `clave-vault` can't cry
  wolf.
- `27303d5`/`43eba7b` — hot-reload SOP gains `-c clave_binary=clave`; the
  round-7 ledger quote was wrongly rewritten, then restored with the safety
  note kept separate.
- `e6cd7cd` — design, plan, carried predecessor handoff.
- `17af37c` — whole-branch review wave (nine findings; see below).

## What was discovered

1. **`layout.kdl` is generated but never handed to zellij** — only `doctor`
   checks it exists. zellij gets `--layout launch.kdl` (`setup.rs:708-711`) and
   the one-shot `add::tab_layout`. An earlier version of the guard test targeted
   `layout.kdl` and would have stayed green while the live coupling broke.
2. **The hot-reload SOP would have silently broken** — and it is the one live
   mutation an agent may perform. A config-less reload after #44 returns
   `PluginDoesNotExist` and zellij **starts a new bar** (`mod.rs:446-468`), so
   the agent's own verification tool would spawn the bug while reporting
   success. (The first fix stated this failure mode backwards — as a silent
   exit-0 no-op — and the whole-branch review caught and corrected it.)
3. **`clave dev launch` never regenerates `config.kdl`.** `just dev-install`
   then launch pairs a post-#44 launch.kdl with a pre-#44 config.kdl → duplicate
   bar, indistinguishable from the fix not working. Now documented as a
   regenerate-first step in the sandbox lifecycle. **This will bite the live
   validation if skipped.**
4. **The seven shellouts had zero automated coverage** (the bin is
   `test = false`). A source-text guard now fails on a reverted shellout;
   verified both directions. Real coverage waits on #47's tier-2 harness.

## What was declined, and why

- **Version-skew `--version` subprocess** (issue #44's "guardrail" para) —
  deferred; a subprocess at every plugin load to detect a case the config key
  already makes near-impossible. Maintainer: "not worth doing now for a while…
  no bloat anywhere yet."
- **Basename-validating the configured value** — no threat to check for; the
  value comes from our own generator.
- **Widening `binary_resolution_is_anomalous`** to the same-version-copy-behind-
  newest-release case — real gap (old copies are never pruned, so `installed`
  can be true while the launcher is stale), deferred to **#48** where the doctor
  version-coherence work fits it.
- **Removing the vestigial `layout.kdl`** — real finding, but a drive-by
  deletion inside a production-incident fix is wrong. Worth its own issue.
- **The fugu review lane** — skipped this branch (maintainer ruling,
  2026-07-23): independent adversarial review was already thorough (six per-task
  + one whole-branch, nine real defects caught), and fugu's four model lanes are
  token-heavy against a spend limit. AGENTS.md was updated to make fugu a
  *recommendation*, not a gate.

## Rulings made this session (binding)

- **Handoffs and the validation ledger are IMMUTABLE history.** A doc-sweep may
  only correct live SOP; it must never rewrite a dated record of what was run.
  (Triggered by an implementer falsifying the round-7 ledger quote under a
  blanket "fix every occurrence" instruction.)
- **fugu is recommended, not required** (AGENTS.md updated in this PR).
- **Blanket commit approval on `fix/plugin-binary-path`** was granted so the SDD
  flow's per-task commits could proceed; the maintainer still signs (1Password).

## Process note for the next session

Four of the defects the review lanes caught originated in the **plan I wrote**,
not in implementer error: the `kb[0]` HashMap-order flake, the silently-dead
test placement, the backwards hot-reload failure mode, and the blanket doc-sweep
that falsified history. The independent lanes are load-bearing on this repo —
budget for them.

The 1Password signing agent was intermittently failing all session (a
commit-fallback system is mid-build). Symptom: `fatal: failed to write commit
object`, sometimes after offering a single-use Claude-signature fallback. If a
commit fails oddly, it is the signing path, not git — retry once, then ask the
maintainer.

## Next steps

1. **Merge #44** once CI is green and the maintainer approves the live pass.
2. **#47 — tier-2 real-zellij harness.** Unblocked by #44: this change supplies
   the binary-injection point the harness needs to aim a session at a test
   binary. First scenario is the #44 regression itself: one bar per tab.
3. **#48 — doctor version-coherence.** Fold in the deferred anomaly-predicate
   widening (same-version-copy-behind-newest-release).
4. Backlog unchanged from the predecessor: #49 release checklist +
   `needs-live-validation` batching, #31 dev-install sandbox-config regen, the
   product items (#38/#39/#40/#45/#36), and file the `layout.kdl` vestigiality
   as its own issue.

## Restart hint

Branch pushed, PR open, tree clean on the branch. The SDD ledger with the full
per-task record and the triaged Minor findings is at
`.superpowers/sdd/progress.md` (git-ignored — lives only in this worktree).
Resume by checking the PR's CI and merge state.
