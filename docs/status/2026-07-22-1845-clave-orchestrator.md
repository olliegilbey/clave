# Status — clave orchestrator (doctor/install merged · next task is #44, spec below)

_2026-07-22 18:45 · repo github.com/olliegilbey/clave · main `50fa26a` ·
tag `v0.1.1` released and daily-driven · tree clean_

Predecessors — read the first, skim the second only if you need incident detail:
- @docs/status/2026-07-22-1759-clave-orchestrator.md — the day's work: v0.1.1,
  the field incident, the autonomy rulings, what landed in the repo.
- @docs/status/2026-07-22-1606-clave-orchestrator.md — the full incident
  write-up (root causes, diagnosis method).

## Task Overview

clave = Zellij fleet-orchestration sidebar (wasm `clave-bar` + `clave` CLI).
Current goal: **close the PATH-resolution hole that broke production (#44)**,
then build the tier-2 test harness (#47) so agents can self-verify without the
maintainer. Success = the bar can never invoke a different `clave` than the one
that launched the session, and an agent can prove that automatically.

## Current State

`main` = `50fa26a`, tree clean, nothing uncommitted.

Merged since the last handoff: **PR #29** — `clave doctor` + install flow
(discovery, preflight, first-run, single-file distribution), +4832/−59 across 22
files, 25 commits squashed. It had been through CodeRabbit and Codex rounds; 0
unresolved threads at merge. Closed #34; refs #43/#31/#35/#36/#37.

Verified locally post-merge: `cargo test --workspace` green, `cargo clippy
--workspace --all-targets -- -D warnings` clean.

Repo hygiene done: worktrees reduced to the main checkout, the codex worktrees,
and this session's `issue-10-kdl-guardrail` home (detached at `ccd67fb`, now
STALE — see Restart Hint). All merged branches deleted local + origin.
`.claude/worktrees/` is gitignored.

## Reference Docs

- `docs/dev/TESTING.md` — **new structure**: verification tiers, the risk
  taxonomy (change class → required evidence → label), the escape record, then
  the original live SOP. Read the taxonomy before choosing what to test.
- `AGENTS.md` — the autonomy contract. **Read this first**, it is the operating
  agreement (what you may do unsupervised, the six things you never do).
- `CONTRIBUTING.md` §"The one leak" — the PATH hazard, mechanism and rule.
- `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` — C-sections; read the one
  for any subsystem you touch. Every forbidden path there was expensive.

## Next Steps

### 1. #44 — stop the bar shelling out to bare `clave` (SPEC)

**Problem.** `crates/clave-bar/src/main.rs` invokes the CLI unqualified at seven
sites — lines **143** (`focus`), **150** (`bind`), **162** (`prune-tabs`), **198**
(`open`), **207** (`add`), **390** (`snapshot`), **444** (`touch`). PATH decides
which binary answers. On 2026-07-22 a stale `0.1.0` dev build on PATH served a
v0.1.1 session's `clave open`, composed tab layouts pointing at the OLD wasm,
and — because zellij keys plugin identity on file **location** — every opened
tab loaded a SECOND bar: duplicate sidebar, no shared beacon state, dead nav.

**The seam already exists, twice over:**
- `crates/clave-bar/src/main.rs:342` — `fn load(&mut self, _config: BTreeMap<String, String>)`.
  The plugin **already receives a configuration map and throws it away.**
- `crates/clave/src/release.rs:73` — `pub fn runtime_binary()` (added by #29)
  resolves the versioned CLI copy `<data>/bin/clave-vX.Y.Z`, falling back to
  bare `clave`. `add`/`open`/the eager launch layout already bake it into
  agent-tab commands, so setup and runtime agree about tab commands — but the
  bar→CLI hop was never covered.

**Implementation.**
1. Emit the resolved absolute binary into the plugin's layout node as a
   configuration key, in `crates/clave/src/setup.rs` where the plugin pane is
   generated (`layout_kdl`, and the launch-layout builder). KDL shape:
   `plugin location="file:<wasm>" { clave_binary "<abs path>"; }`. **Verify the
   exact child-node syntax and the `load` delivery against vendored
   zellij-tile 0.44.3 before writing** — repo rule: never trust an assumed
   zellij behaviour. Note the KDL trailing-`;` gotcha already documented in
   `config_kdl`.
2. Store it in `State` at `load()`; replace all seven `"clave"` literals with it.
3. **Fallback must be loud, not silent.** If the key is absent (old layout,
   hand-edited config), log at a level that reaches the zellij log and then use
   `"clave"` — silence is what made this invisible for hours. Same treatment for
   #29's deliberate *"unresolvable current_exe → bare `clave`"* fallback in
   `setup.rs`: keep the fallback, make it announce itself.
4. Consider a version-skew guard: the bar already logs `clave-bar: loaded
   vX.Y.Z` at load; compare against the configured binary's version and log
   loudly on mismatch (cheap belt-and-braces, catches any future ambient path).

**Tests (risk class: generated artifacts + cross-process → taxonomy requires
real-parser guardrail + coherence assertions).**
- Extend `crates/clave/tests/kdl_guardrail.rs`: the generated layout parses
  through the REAL parser **and** its plugin node carries `clave_binary` equal
  to the versioned artifact.
- Extend the existing `generated_artifact_set_is_version_coherent` test in
  `setup.rs` so the binary key participates in the per-artifact version check.
- Note the existing test asserts each artifact carries exactly ONE version —
  that invariant holds for release-shaped inputs; dev/sandbox generation uses
  bare `clave` + unversioned wasm, so do not extend the assertion to dev inputs
  without deciding what coherence means there.

**Gates:** `cargo test --workspace` · `cargo build -p clave-bar --target
wasm32-wasip1` · `cargo clippy --workspace --all-targets -- -D warnings`
(`--workspace` matters — the default-members form skips the wasm crate; CI uses
`--workspace`).

**Live-validation (label `needs-live-validation`):** maintainer only. Sandbox:
`clave dev launch`, open rows, confirm exactly one bar per tab and that
`grep 'clave-bar: loaded' "$TMPDIR"/zellij-*/zellij-log/zellij.log` reports a
single version.

### 2. #47 — tier-2 real-zellij harness (unblocked by #44)

Isolated `clave-it-<pid>` sessions via the `CLAVE_SESSION`/`CLAVE_STATE_DIR`/
`CLAVE_DATA_DIR` triple, so it can never touch the maintainer's session. The
pane command is injectable — spawn `sleep`, not `claude`, so CI needs zellij but
no Claude Code, no auth, no network. Hazards recorded on the issue: `zellij
action` against a dead session **blocks forever** (timeout every call), needs a
PTY, decide flake/quarantine policy up front.

### 3. Then

**#48** doctor version-coherence (the doctor now exists post-#29 — fold the
check in) · **#49** release checklist + `needs-live-validation` batching ·
**#31** dev-install sandbox-config regen · **live-validate the #29 install flow
before the next tag** (it touches the surface that broke today) · product
backlog **#38** status glyph fidelity, **#39** row ordering (decide inside the
**#24** brainstorm), **#40** Nerd Font portability, **#45** pipe noise, **#36**
(`good first issue`, ideal first cloud-agent task).

**Where work stopped — verbatim (maintainer):** "Will you take over, and merge
that and see how things go from there" — PR #29 was merged, main verified green,
the doctor worktree and branch removed. Then: "add anything needed to the
handoff so that the next agent has context to begin, including the specs for
what to do next. And should we get off this worktree?"

## Important Discoveries

Beyond the predecessors' incident detail:

1. **PR #29 fixed the CLI→tab hop, not the bar→CLI hop.** `runtime_binary()`
   makes `add`/`open`/launch bake absolute versioned paths, but the bar still
   resolves through PATH — so the incident chain survives until #44. Do not
   assume #29 closed it.
2. **`git branch --merged` lies under squash-merge** — the tip is never an
   ancestor of main, so every merged branch reports unmerged. Verify with
   `gh pr list --state merged --json headRefName`.
3. **One-grep version diagnosis:** the bar logs `clave-bar: loaded vX.Y.Z` at
   every load; zellij's log is at `$TMPDIR/zellij-<uid>/zellij-log/zellij.log`
   on macOS (NOT `~/Library/Caches`). Mixed versions there = the #44 failure in
   progress. This found the incident in minutes.
4. **The maintainer's git hooks are mid-build** (he has an agent fixing them).
   Two failure modes seen: `sign-with-fallback.sh` surfacing a transient
   1Password agent error with **"Do NOT retry this as Claude"** — obey it, stop
   and ask; and `pre-push` running `git rev-list ..` when its stdin replay is
   empty (`fatal: '..' is outside repository`). If a push fails oddly from a
   worktree, suspect the hook, not git.
5. **Merge mechanics:** auto-merge is DISABLED, all review threads must be
   resolved, branch must be current. Sequence: resolve threads (GraphQL
   `resolveReviewThread`) → `gh pr update-branch` → wait for CI → `gh pr merge
   --squash`.

## Context to Preserve

- **User prefs (binding):** extremely concise, signal over noise; explain while
  doing; dense why-comments citing spec §/ledger/issue; conventional commits +
  `Claude-Session:` trailer; **never commit without explicit approval** (he
  signs via 1Password); ask before architecture decisions with multiple valid
  approaches; he drives ALL live zellij input.
- **Never run `just dev-install` or `cargo install` while he may be daily-driving**
  — it writes `~/.cargo/bin/clave`, the name the live fleet's plugin shells out
  to. This caused the outage. Restore with
  `cp ~/.local/share/clave/bin/clave-vX.Y.Z ~/.cargo/bin/clave`.
- **`just release` is his**, watched live. `~/.claude/` is READ-ONLY.
- **Pre-commit PII blocklist** rejects private local path names in staged lines
  — genericize (`~/…`, `$TMPDIR/…`). It fired twice.
- **Review requirement:** the vendored fugu harness (`.claude/commands/
  fugu-review.md`) **plus** an independent adversarial reviewer. In a cloud
  container do NOT opt into `cli_reviewers` (the external CLIs are absent and
  need interactive auth) — **a lane that did not run is not a lane that passed**;
  state which lanes actually executed in the PR dossier.
- **SSH constraint:** clave must eventually work over SSH — reject designs
  assuming the CLI and plugin share a local desktop.
- Outstanding off-repo: backport the fugu `diffExpr` fix to the maintainer's
  global copy (repo copy is fixed, his is not).

## Restart Hint

Tree clean, main green, nothing uncommitted — safe to /clear.

**Do not resume in `.claude/worktrees/issue-10-kdl-guardrail`** — it is detached
at `ccd67fb`, four merges behind, so an agent reading code there sees pre-#29
state. Work from the main checkout `/Users/olliegilbey/code/clave`, or cut a
fresh worktree for #44 (`git worktree add .claude/worktrees/issue-44-plugin-binary
-b fix/plugin-binary-path origin/main`). The stale worktree can be removed once
this session is cleared — it is only alive because the session's cwd points at
it. Start by reading `AGENTS.md`, then the #44 spec above.
