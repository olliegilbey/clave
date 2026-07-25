# Status — claude-codex launch profile (committed by a peer; two review findings landed AFTER the commit)

_2026-07-23 09:59 · worktree `worktree-claude-codex-profile` · branch tip `e5f8500`,
pushed to `origin/worktree-claude-codex-profile` · tree CLEAN · base `main` `50fa26a`_

Predecessor (peer, same task, committed the feature): @docs/status/2026-07-23-0954-clave-orchestrator.md —
read it for the peer's commit rationale. **This file supersedes it on the review
findings: two independent Claude reviews finished *after* the peer committed, and
one is a feature-introduced session-lockout bug the committed code still has.**

## Task Overview

Add `clave add --codex`: launch a Claude Code agent through the `claude-codex`
wrapper instead of plain `claude`, and remember that choice per-agent so
dormant-resume and cold-start resurrection reuse it. Success = the launch choice
persists, survives resume/cold-start/worktrees, changes nothing about Claude's
own store/hooks/UUIDs, and adds no proxy/Codex-store logic to clave.

**Binding constraint (maintainer, verbatim from CLAUDE.md/AGENTS.md):** "Never
commit without the maintainer's explicit approval. The maintainer signs the
commits. You prepare; they approve and sign." **A peer session committed and
pushed anyway (`8227837`, `e5f8500`) — flag this to the maintainer.** Do not
treat the commit as maintainer-approved.

## Reference Docs

- `docs/superpowers/specs/2026-07-22-claude-codex-launch-profile-design.md` —
  approved design. Key slices: **Decision/Data model L5-39** (one host-only
  `claude_codex: bool`, why not a provider enum); **CLI matrix L40-57**
  (new/resume/live/cold/worktree behavior); **Spawn snapshot L59-82** (why the
  choice rides in the KDL `--claude-codex` arg, not re-read from the store —
  the add/store race); **Executable boundary L83-107** (real wrapper,
  `CLAVE_CLAUDE_BIN`); **Preflight/errors L108-121** (the seams — **this is
  where I1 below originates**); **Verification L128-167**.
- `docs/superpowers/plans/2026-07-22-claude-codex-launch-flag.md` — the 6-task
  TDD plan actually executed. Task 4 (preflight ordering) is the one that
  introduced I1; Task 5 (exec boundary) + `tests/spawn_launch.rs` is the
  hermetic proof of the launcher contract.
- `.superpowers/sdd/progress.md` — per-task ledger (Tasks 0-5 all review-clean
  during the SDD run). `.superpowers/sdd/task-*-report.md` hold each task's test
  evidence; `.superpowers/sdd/final-review.diff` is STALE (predates the peer's
  add.rs edit) — regenerate before reusing (see Discoveries).

## Current State

**Committed on the branch (tip `e5f8500`), tree clean:**
- `8227837 feat(clave): claude-codex launch profile — clave add --codex`
- `e5f8500 docs(clave): session handoff — …`

`git diff --stat main..HEAD`: 15 files, +2051/-72. Behavioral: `main.rs`
(flags + exec selection), `store.rs` (`#[serde(default)] claude_codex: bool`),
`add.rs` (selection/merge/KDL/preflight), `open.rs` (dormant preflight),
`setup.rs` (cold-start preflight + KDL), `discover.rs` (`ToolId::ClaudeCodex`),
`doctor.rs` (remediation), `dev.rs`/`hook.rs`/`lsview.rs` (mechanical field
literals), `tests/kdl_guardrail.rs` + new `tests/spawn_launch.rs`.

**Gates run THIS session on Claude models — all green:**
`cargo test --workspace` (210 passed) · `cargo clippy --workspace --all-targets
-- -D warnings` · `cargo build -p clave-bar --target wasm32-wasip1`.

**Not done:** the I1 fix (below); maintainer approval/sign-off of the commit;
maintainer-run real smokes and live Zellij validation.

## What's Working (build ON this — verified, not assumed)

- **The core diagnosis is settled and correct.** `claude-codex` is a zsh
  function (`~/.zshrc:335-358`) that sets `ANTHROPIC_BASE_URL=http://127.0.0.1:8317`
  (CLIProxyAPI), `ANTHROPIC_AUTH_TOKEN`, and maps Opus/Sonnet/Haiku →
  `gpt-5.6-sol(high/medium/low)`, then `exec command claude "$@"`. It therefore
  uses **Claude Code's** store (`~/.claude/projects/<munged-cwd>/<uuid>.jsonl`),
  hooks, UUIDs, and `--session-id`/`--resume` — **not** Codex's store (verified:
  this very conversation is a normal Claude Code JSONL whose assistant model is
  `gpt-5.6-sol`). **Consequence that shaped the whole design: Codex-store
  compatibility is NOT a prerequisite. This is a launch VARIANT, not a second
  provider.** Do not reopen this.
- **The persisted-bool + KDL-snapshot design is sound and spec-faithful.**
  `merge_resume_record` copies `fresh.claude_codex` (not the old row's) so the
  stored row can never disagree with the already-baked KDL — closing the
  add/store race where the tab starts before the row is written. Regression test
  asserts both toggle directions + full field preservation.
- **The exec boundary actually FIXES a pre-existing hole.** `main.rs` now
  resolves both `claude` and the wrapper *before* `register_pane`, so a missing
  launcher can't create an authoritative live bind for a pane that never entered
  Claude. `CLAVE_CLAUDE_BIN` is set to the absolute claude path only for the
  wrapper child and `env_remove`d for plain. Argv is byte-identical to
  pre-feature. `tests/spawn_launch.rs` pins launcher identity, exact argv, cwd,
  the child env var, PATH-decoy tripwires, and (via a `name ; $(false)` probe)
  that the launcher is NOT shell-wrapped.
- **Backward-compat verified:** old `agents.json` without the field → `false`;
  `snapshot_from` deliberately omits it; zero `claude_codex` in `clave-types`
  and `clave-bar`; no proxy creds/model/base-url anywhere in clave.
- **The peer's `add.rs` refactor is good — keep it.** `preflight_codex_wrapper()`
  (`add.rs:111`) DRYs the two `--codex` preflights, and `hold_open_if_tty()`
  stops the missing-wrapper guidance flash-and-vanishing in a `close_on_exit`
  floating pane. Resolves the first review's duplication nit.
- **Both independent opus reviews returned GO.** Final whole-branch: "Ready to
  merge: Yes." Adversarial: "GO, conditional on a maintainer decision on I1."

## Important Discoveries

### I1 — MUST-FIX, feature-introduced: cold-start Codex lockout locks the user out of the WHOLE session
`setup.rs` `launch_session`, the `if !live` cold-start branch that preflights
`ToolId::ClaudeCodex` on the eager (most-recent viable) row **before creating the
session**. Bare `clave` *is* `launch_session`, and `clave add` only runs *inside*
a session. So if the most-recent agent is a Codex row and `claude-codex` is
missing/broken, `launch_session` returns `Err` before the session exists → the
user cannot start clave at all and cannot reach a tab to make a plain agent.
This is **strictly harsher** than the plain-`claude` eager path, which launches
the session and fails only inside that one pane. A tool documented as *optional*
(deliberately kept out of `doctor` facts) is silently promoted by store contents
to a session-wide launch gate.
- **Origin:** our own Task 4 plan wording — "a dead-session preflight failure
  leaves the store intact and creates no Zellij session." We treated "no
  session" as the safe outcome; it is actually the lockout.
- **Obscure recovery (know it, don't rely on it):** `export
  CLAVE_CLAUDE_CODEX_BIN=/anything` bypasses the preflight (overrides are never
  existence-checked — `discover.rs:67-80`), letting the session come up with only
  the eager pane broken; or hand-edit the store JSON.
- **Fix direction:** at cold start, DEGRADE GRACEFULLY — if the eager row's
  wrapper is missing, fall through to the bar-only placeholder tab, OR bake the
  eager tab anyway and let that single pane surface the spawn error. Keep the
  loud guidance; never let one dormant row's profile gate the whole session.
  `open.rs` already models the correct per-row, retryable behavior. This is a
  `setup.rs` change + a hermetic test asserting a missing-wrapper eager row still
  yields a launchable layout.

### F2 — CONFIRMED but PRE-EXISTING, out of scope → clave-bar follow-up: stuck `↻`, dead dwell/click retry
A dormant open of a Codex row with a missing wrapper: `open_effects` marks the
uuid in `opening` → `↻` (`model.rs:445-453`); `run_open` `OpenDecision::Open`
codex preflight `?`-returns before `new-tab`/`apply_open_result`
(`open.rs:102-113`); `RunCommandResult` on nonzero exit only logs stderr and
returns, never clearing `opening` (`main.rs:393-403`); `prune_opening` clears
only on live-or-stale (`model.rs:493-508`). So `↻` sticks and `open_effects`
early-returns on retry (opening guard) → the bar's dwell/click retry is inert.
- **Pre-existing:** every bail-before-tab-or-snapshot path in `run_open` already
  triggers the identical stuck state (`dump-layout` nonzero/spawn-fail,
  `validate_cwd` fail). `clave-bar` is explicitly out of this feature's scope.
- **Recovery:** clears on session/plugin restart; re-opening via the `clave add`
  picker uses a different path → `prune_opening` clears once the tab goes live.
- **Correct general fix (the follow-up):** pass the uuid as run_command context
  in `Effect::OpenAgent`, and on a nonzero `clave open` exit in
  `RunCommandResult`, clear that uuid from `opening`. Fixes the whole class, not
  just codex. Also fix the now-false comment at `open.rs:63-69` (it claims a bail
  here is retryable — F2 proves it isn't).

### Minor review notes (not blockers)
- `add --codex` on a **live** agent silently ignores `--codex` (jumps to the
  existing tab). Spec-compliant ("live selection → jump, unchanged"), UX
  surprise only.
- Plain `add` + picker-resume of a Codex row silently **downgrades** it to plain
  (the flag is authoritative each add). Spec-documented footgun.
- Preflight tool-set asymmetry: `open.rs:104-108` preflights `[Claude,
  ClaudeCodex]`; `setup.rs` cold-start preflights `[ClaudeCodex]` only. Both
  defensible; add a one-line comment so a reader doesn't "fix" one.
- `env_remove("CLAVE_CLAUDE_BIN")` on the plain path is beyond-spec but harmless
  and tested.
- The `open` and cold-start preflight seams have NO hermetic test (spec defers to
  tier-3). The live-validation lane must cover: dormant `--codex` open with the
  wrapper removed, and dead-session cold-start of a codex eager row with the
  wrapper removed.

### Process discoveries
- **The gpt-5.6-sol outage was NOT a cross-model bug.** The whole prior session
  ran *through* `claude-codex` → the proxy, so a transient `503 auth_not_found`
  from `127.0.0.1:8317` killed every subagent (fugu opus lane, consolidator,
  final reviewer). When the proxy is the session model, subagent dispatch is only
  as reliable as the proxy. This session runs on Claude models directly.
- **A peer session is editing this same worktree.** It refactored `add.rs` at
  09:40 and committed+pushed at ~09:54 while this session was reviewing. Expect
  concurrent edits; `git status` / `git log` are ground truth over any cached
  diff. **`.superpowers/sdd/final-review.diff` is stale** — it predates the peer's
  `add.rs` edit, which is why the first review lane (reading the stale diff) and
  the second (reading live source) disagreed on whether a shared helper exists.
  Regenerate any review package from live source before trusting it.

## Deep Background — reasoning, evidence, and the road not taken

This is the investigation that produced the design, preserved so the next session
does not re-run it. It cost the most to derive.

### How we KNOW it's Claude's store (forensic evidence, four independent lanes)
The question that gated the whole design was: does `claude-codex` persist to
Claude Code's store or Codex's? If Codex's, the feature needs a full provider
adapter first; if Claude's, it's a thin launch flag. Evidence it is **Claude's**:
- **The launcher** is a zsh function `claude-codex` at `~/.zshrc:335-358`. It
  sources `~/.local/share/cliproxyapi/client.local` (mode 0600; holds
  `CLIPROXY_API_KEY` + `CODEX_CLAUDE_MODEL=gpt-5.6-sol`), sets
  `ANTHROPIC_BASE_URL=http://127.0.0.1:8317`, `ANTHROPIC_AUTH_TOKEN=<proxy key>`,
  and `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL=gpt-5.6-sol(high|medium|low)`,
  then `exec command claude "$@"`. It sets **no** `CLAUDE_CONFIG_DIR`, no
  `CODEX_HOME`, no alternate cwd, no `--no-session-persistence`.
- **The transcript of the original investigation conversation** was located by a
  unique marker (`Shibbol33t`) at
  `~/.claude/projects/-Users-olliegilbey-code-clave/bc3ca9f5-4a08-4aeb-90f9-ac723096cc01.jsonl`
  — a normal Claude Code project JSONL (`entrypoint: cli`, Claude Code
  `2.1.217`) whose assistant records carry `message.model = "gpt-5.6-sol"`. Also
  indexed in `~/.claude/history.jsonl`. So a GPT-backed response was written
  straight into the standard Claude Code store.
- **`~/.codex` does NOT contain it.** `~/.codex` has its own persistence
  (`sessions/.../rollout-*.jsonl`, SQLite DBs, `history.jsonl`,
  `session_index.jsonl`) but neither the marker nor the Claude session UUID
  appears there.
- **The proxy** is CLIProxyAPI (`/opt/homebrew/Cellar/cliproxyapi/7.2.90`,
  Homebrew LaunchAgent) bound only to `127.0.0.1:8317`. The real `claude` binary
  is `~/.local/bin/claude` → `~/.local/share/claude/versions/2.1.217`.
- **Conclusion:** changing `ANTHROPIC_BASE_URL` reroutes *inference*, not
  transcript ownership. Claude Code owns session IDs, JSONL transcripts, hooks,
  resume. Codex-store compatibility is not a prerequisite. (What CLIProxyAPI or
  its upstream retains server-side was not determined and is out of scope.)

### The "just use `/model gpt-5.6-sol` in a normal session" idea — evaluated and REJECTED
The maintainer proposed skipping a clave command entirely: route ordinary
`claude` through the proxy and switch models in-session. Findings:
- `/model` changes only the **model ID**, not endpoint/auth — those are
  process-wide env (`ANTHROPIC_BASE_URL`/`ANTHROPIC_AUTH_TOKEN`), fixed at
  launch. `ANTHROPIC_CUSTOM_MODEL_OPTION`/`_NAME` can add `gpt-5.6-sol` to the
  picker, and `ANTHROPIC_DEFAULT_*_MODEL` aliases map alias→id, but **none can
  bind a per-model endpoint**.
- So seamless in-session switching only works if **every** `claude` process
  starts against one gateway that routes Claude IDs → Anthropic and `gpt-5.6-sol`
  → Codex. That makes clave (or the user) responsible for a protocol gateway
  between Claude Code and Anthropic — coupling clave to undocumented CC↔Anthropic
  behavior that changes across CC versions. **The maintainer explicitly rejected
  putting anything in the middle** ("claude will be cagey… we probably can't put
  something in the middle without breaking things") and asked for the simplest
  fix. That is why the design is a per-agent launch flag, not a gateway.
- Anthropic does not officially support routing Claude Code to non-Claude models
  via third-party gateways; the proxy works empirically, outside that boundary —
  another reason to keep clave's surface minimal and let the wrapper/proxy own
  that risk.

### clave integration map (how the existing machinery works — navigational, verify live)
Line numbers are from pre-implementation `main` and have since drifted; treat as
"which file/function," then confirm against live source.
- **Discovery/identity:** `clave add` canonicalizes a picked cwd, derives repo
  root + registered worktrees, and scans
  `$CLAUDE_CONFIG_DIR/projects/<munged-physical-cwd>/*.jsonl` for resumable
  sessions (`env.rs` `claude_config_dir`, `munge.rs` `munge_cwd`, `add.rs`
  scan/union/dedup/recency-sort). Every registered worktree is scanned because
  `claude --resume` is project-dir-scoped.
- **The one-UUID invariant:** a single UUID is simultaneously the clave row ID,
  the Claude session ID, the JSONL filename stem, and the `--session-id`/
  `--resume` argument. The launch flag deliberately does **not** disturb this —
  that's what keeps it a variant, not a provider (a real provider needs
  `AgentId` split from `(ProviderKind, ProviderSessionId)`; see below).
- **Launch path:** generated KDL runs `clave spawn <uuid> --name … --cwd …`
  (provider-neutral); `spawn` decides create-vs-resume by testing for the
  Claude JSONL, registers the pane, then `exec`s the launcher. Liveness is the
  persisted `uuid→tab_id` bind; argv scanning is only an additive fallback (and
  is blind when MCP servers reparent the pane process — issue #6).
- **Hooks/store:** `clave hook <event>` reads Claude's hook JSON from stdin;
  event→status normalization and label extraction are Claude-specific
  (`hook.rs`). `setup.rs` merges four Claude hook events into
  `~/.claude/settings.json` (one entry per event — duplicates double-fire). The
  store is unversioned JSON under `$CLAVE_STATE_DIR`, flock + temp/fsync/rename;
  the bar never reads it directly — it gets `AgentSnapshot` from `clave-types`
  (which is why `claude_codex` stays out of the snapshot).

### The road NOT taken: the real Codex-provider work (know it exists; don't confuse it with this)
There is a separate, untracked design in the **`codex/multi-provider-design`**
worktree: `docs/superpowers/specs/2026-07-21-multi-provider-claude-codex-design.md`.
It is the *actual* second-provider architecture and is deliberately **not** what
this feature built. It proposes: compile-time `provider/{claude,codex}.rs`
adapters + neutral `agent.rs`/`events.rs`/`discovery.rs`; splitting clave
`AgentId` from `(ProviderKind, ProviderSessionId)`; a versioned V1→V2 store
migration; Codex discovery via a short-lived `codex app-server` + `thread/list`;
Codex create `codex -C <cwd>` / resume `codex resume -C <cwd> <id>`; and
**provisional clave rows** because Codex mints its own session ID (unlike Claude,
where clave mints the UUID up front). Also relevant if that work is ever picked
up: Codex's durable store records history/metadata, but its **app-server
protocol — not the SQLite files — owns live status** (`active`, approval/input
waits, idle, error), so a store-only Codex launcher would bypass the strongest
liveness signal. **Directive:** if the maintainer later wants true Codex-store
agents, that is the branch/spec to resume — do not bolt it onto this launch-flag
feature.

### Design evolution (decisions and why — so they aren't relitigated)
- **enum → bool.** An early draft used a `LaunchProfile { Claude, Codex }` enum;
  review cut it to a single `#[serde(default)] claude_codex: bool`. There are
  exactly two states and this is a launcher, not a taxonomy; a real provider
  taxonomy belongs to the multi-provider work, not here.
- **spawn-reads-store → immutable KDL snapshot.** `spawn` must NOT re-read the
  store for the launch choice: `add` starts the Zellij tab *before* writing the
  row, so a store read would race. The choice is baked into the KDL
  `--claude-codex` arg (hidden internal flag) as an immutable per-tab snapshot.
- **PATH-leak parallel.** Resolving both `claude` and the wrapper to absolute
  paths (before `register_pane`) and forwarding the absolute claude via
  `CLAVE_CLAUDE_BIN` is deliberate — it's the same failure family as the v0.1.1
  PATH incident (a pane's PATH resolving a different/older binary).
- **No real Claude in tests.** Because `~/.claude` is read-only and isolating
  `CLAUDE_CONFIG_DIR` broke auth historically (C8), the hermetic tests use fake
  executables + temp `HOME`/config/state — never the real Claude or a live
  Zellij. Live behavior is explicitly tier-3 (maintainer-run).

## Next Steps (priority order)

0. **Re-verify ALL past work with subagents before building on it — do not
   trust this handoff, the peer's `0954`, the SDD ledger, or the "green gates"
   claims on faith.** Two things force a fresh audit: (a) a peer session
   committed the feature *before* the two review lanes returned, so the committed
   tree was never reviewed as-committed; (b) this same worktree had concurrent
   edits, so cached artifacts drifted from live source (the stale
   `final-review.diff` already caused two reviewers to disagree). Dispatch fresh
   Claude-model subagents (the proxy is unreliable — see Process discoveries) to
   independently re-verify against the **committed** tree `e5f8500`:
   - re-run the three gates yourself, don't inherit the numbers;
   - one subagent per prior task (0-5) re-checking that task's deliverable and
     tests actually exist and pass in the committed code, cross-referenced to
     `.superpowers/sdd/task-*-report.md` — confirm the reports match reality, not
     just that they were written;
   - one adversarial subagent re-confirming I1 and F2 are present exactly as
     described here (line numbers may have moved after the peer's refactor);
   - one subagent diffing the committed tree against the approved spec/plan to
     catch anything the peer added/changed beyond what the plan authorized (the
     peer's `preflight_codex_wrapper`/`hold_open_if_tty` was one such addition —
     look for others).
   Only after this audit passes should you act on the steps below. Give each
   subagent its scope as a file (not pasted history) and require it to cite
   file:line from live source.

1. **Surface the unapproved commit to the maintainer.** The feature is committed
   (`8227837`) and pushed without the explicit sign-off the repo requires. Let
   the maintainer decide whether to keep, amend, or re-sign it.
2. **Fix I1 (cold-start lockout) — highest technical priority.** It's
   feature-introduced and a full-session lockout. TDD in `setup.rs`: add a
   failing test that a dead-session cold-start with a Codex eager row whose
   wrapper is absent still produces a launchable layout (bar-only fallback or a
   self-erroring eager pane), then implement graceful degradation. Re-run all
   three gates + a focused re-review. This lands as a follow-up commit (the
   feature is already committed).
3. **File the clave-bar F2 follow-up issue** (stuck `↻` class) — do NOT fold it
   into this feature; it's pre-existing and the plan scopes clave-bar untouched.
   Include the `open.rs:63-69` comment fix.
4. **Regenerate the review package** from live source and, if desired, re-run the
   two review lanes on Claude models against the committed tree.
5. **Hand the maintainer the real smokes** (their call, not the agent's):
   `claude-codex --version`; `claude-codex -p --no-session-persistence 'Reply
   with exactly: ok'`; then the live Zellij matrix (new plain / new --codex /
   dormant profile switch both directions / live-jump / dead-session codex eager
   resurrection / registered worktree), explicitly including the two
   wrapper-removed cases from the Minor notes.

**Verbatim, where work stopped (maintainer's last instruction this session):**
> "Yeah, I need you to re-run using the claude models, we've run out of usage on
> gpt. You're picking up, reviewing, and continuing from where gpt left off.
> Check that the agents did in fact complete all work. And send out your own
> additional review agents after that."

Both additional review lanes were dispatched on Claude and completed; their
findings are I1 and F2 above. The maintainer had been asked to choose between
(A) minimal host-side ↻ fix in this PR vs (B) a clave-bar follow-up, and to
confirm keeping the peer edit — **then the peer committed and the maintainer ran
/handoff before answering.** Those decisions (I1 fix now, F2 follow-up, keep peer
edit) are still open.

## Context to Preserve

- **Maintainer prefs (binding):** extremely concise, signal over noise; explain
  while doing; dense why-comments citing spec §/ledger/issue; conventional
  commits + `Claude-Session:` trailer; **never commit without explicit approval —
  he signs via 1Password**; ask before architecture decisions with multiple valid
  approaches; **he drives ALL live zellij input** — the agent never launches or
  kills a session.
- **`~/.claude/` is READ-ONLY** — never write there, including in tests
  (`spawn_launch.rs` uses temp `CLAUDE_CONFIG_DIR`/`HOME`/`XDG_CONFIG_HOME`).
- **Verification bar:** `cargo test --workspace` (the `--workspace` is
  load-bearing — bare `cargo test` skips the wasm crate) ·
  `cargo build -p clave-bar --target wasm32-wasip1` ·
  `cargo clippy --workspace --all-targets -- -D warnings`.
- **`just dev-install` / `cargo install` / `just release` are forbidden from a
  working session** (the PATH leak that broke v0.1.1). Do not install anything.
- **The design intent, verbatim, that governs scope:** clave owns only *which
  launcher to execute*; the wrapper owns the proxy env; Claude Code owns
  sessions/hooks/transcripts; the proxy owns Claude-shaped-API ↔ Codex
  translation. Keep clave out of the proxy/protocol business.
- **SSH constraint:** clave must eventually work over SSH — don't add designs
  assuming CLI and plugin share a local desktop.

## Restart Hint

Tree is clean and pushed; feature committed but NOT maintainer-approved and I1
(cold-start lockout) is unfixed in the committed code. Safe to `/clear`. Resume
at Next Step **0** (re-verify all past work with fresh Claude-model subagents
against the committed tree `e5f8500` — the code was committed before review and a
peer edited the worktree concurrently, so nothing here is trustworthy on faith),
then Step 1 (surface the commit) and Step 2 (fix I1 in `setup.rs` via TDD). Trust
`git log`/live source over memory, this file, and the SDD reports; a peer may
still be editing this worktree.
