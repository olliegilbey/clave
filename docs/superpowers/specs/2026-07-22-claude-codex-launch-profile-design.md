# Claude-Codex Launch Flag — Design

_2026-07-22 · approved direction: `clave add --codex` · status: reviewed, awaiting Ollie's written-spec approval_

## Decision

Treat `claude-codex` as a launch variant of Claude Code, not as a second provider.

The local wrapper changes Claude Code's endpoint/auth/model environment, then runs ordinary Claude Code. This conversation proves the result still uses Claude's UUIDs, hooks, `--session-id`/`--resume`, cwd-scoped JSONL transcripts, and worktree semantics. Clave therefore needs only to remember and reproduce one launch choice.

## Goals

- `clave add --codex` launches the selected agent through `claude-codex`.
- Plain `clave add` launches ordinary Claude Code.
- Dormant opening and dead-session resurrection retain the last selected launch variant.
- Existing store rows default to ordinary Claude.
- Clave owns no proxy secrets, model mapping, API translation, or Codex-store logic.

## Non-goals

- No Codex CLI provider, store discovery, app-server integration, or lifecycle adapter.
- No `/model` endpoint switching.
- No shell-function execution through `zsh -lic`.
- No provider marker in the bar.
- No wrapper installer or version-probing surface.
- No agent-driven writes under the real `~/.claude/` or live Zellij interaction.

## Data model

Add one backward-compatible, host-only field to `AgentRecord`:

```rust
#[serde(default)]
pub claude_codex: bool,
```

A boolean is sufficient: this is one exceptional Claude Code launcher, not a provider taxonomy. A future native Codex provider still requires the separate multi-provider identity/lifecycle design.

Old JSON without the field loads as `false`. The field does not enter `AgentSnapshot`, `clave-types`, or the bar.

## CLI and behavior

`clave add` gains `--codex`. The immediate launch and stored value follow this matrix:

| Case | Launch | Stored value |
|---|---|---|
| New plain add | ordinary Claude | `false` |
| New `add --codex` | wrapper | `true` |
| Dormant resume via plain add | ordinary Claude | overwrite to `false` after tab creation succeeds |
| Dormant resume via `--codex` | wrapper | overwrite to `true` after tab creation succeeds |
| Live picker selection | jump to existing tab | unchanged |
| Bar-triggered dormant open | stored variant | unchanged |
| Dead-session eager resurrection | stored variant | unchanged |
| Worktree row | same rules at the row's physical worktree cwd | preserve all worktree metadata |

The existing-row merge copies the requested `claude_codex` value while preserving label, cwd, repo root, branch, worktree, recency, label source, stale state, and other earned fields except the existing intended status/tab reset.

Every clap change receives `Cli::try_parse_from` regression tests.

## Immutable spawn snapshot

Current `add` ordering starts the tab before writing the row. Resolving the variant from the store inside `spawn` would therefore race new rows and profile changes.

Carry the launch snapshot in the existing internal command:

```text
clave spawn <uuid> --name <name> --cwd <cwd> [--claude-codex]
```

One small KDL argument helper is shared by the normal and bare tab builders. It emits the existing arguments and conditionally appends only `--claude-codex`; it contains no endpoint, credential, model mapping, or shell text.

- `add` builds the immediate tab from the requested flag.
- `open` and cold-start layout generation derive the flag from the stored row.
- `spawn` executes exactly the encoded snapshot; it does not re-read the store.
- After successful tab creation, `add` persists the selected value.

Create/resume arguments remain unchanged:

- transcript absent: `--session-id <uuid> --name <name>`
- transcript present: `--resume <uuid>`

Claude transcript discovery and registered-worktree scanning remain untouched.

## Executable boundary

`claude-codex` must be a real executable, not only a zsh function. It must:

1. accept arbitrary Claude Code arguments;
2. preserve cwd;
3. configure the local proxy environment;
4. execute the Claude Code binary named by `CLAVE_CLAUDE_BIN`, falling back to `claude` only for manual use:

```sh
exec "${CLAVE_CLAUDE_BIN:-claude}" "$@"
```

Clave reuses the discovery infrastructure merged in `50fa26a`:

- existing ordinary-Claude resolution;
- new `ToolId::ClaudeCodex`;
- optional trusted `CLAVE_CLAUDE_CODEX_BIN` override;
- shared known executable locations, excluding Claude-specific nvm/local-install additions;
- absolute-path execution, resolved fresh at spawn.

Explicit `CLAVE_*_BIN` overrides retain the existing trusted-override semantics: preflight accepts the supplied path without an existence check, so a bad override may fail at exec.

For Codex launches, clave resolves both ordinary Claude and the wrapper before pane registration, then direct-executes the wrapper with child-only `CLAVE_CLAUDE_BIN=<absolute Claude path>`. This prevents a Zellij-pane PATH from selecting a different Claude version—the same failure family as the v0.1.1 incident.

Clave never reads proxy credentials or model configuration. The Claude/wrapper launcher is not shell-wrapped; the existing `/bin/sh` pane-registration double-fork remains unchanged because it closes the C7 zombie finding.

## Preflight and errors

The wrapper is optional. It does not enter normal doctor facts/catalogue, so users who never request Codex retain an unchanged clean report. Missing-wrapper failures reuse centralized remediation text.

Profile-specific preflight occurs only when the launch path requires it:

- `add --codex`: after ruling out a live picker jump and before worktree creation, store mutation, or tab creation;
- dormant `open`: after `AlreadyLive` and `Stale` are ruled out, preflight both ordinary Claude and the wrapper before KDL write or tab creation;
- dead-session cold start: clone the eager row, preflight its required launcher, then perform session-scoped store mutation, dead-session cleanup, layout generation, and session creation from that same snapshot;
- not when merely attaching to an already-live clave session.

Existing required-tool preflight on live attach is unchanged; only the optional wrapper check is skipped.

If the wrapper is not discoverable, fail with actionable guidance and never silently substitute ordinary Claude.

A dead-session failure leaves the store intact and creates no Zellij session. Spawn resolves every required executable before `register_pane`, preventing a missing launcher from creating an authoritative live bind for a pane that never entered Claude. A filesystem race after successful resolution remains an ordinary exec failure; eliminating it would require a larger bind-rollback design and is out of scope.

## Compatibility boundary

Clave depends only on Claude Code's existing CLI and hook/session contract plus an executable that forwards its arguments. It does not sit between Claude Code and Anthropic/CLIProxyAPI.

When Claude Code changes its server protocol, compatibility belongs to the wrapper/proxy. Clave changes only if Claude changes the documented create/resume or hook/session contract—an existing upstream risk independent of this feature.

## Verification

Implementation is TDD: write each failing test, observe failure, then implement.

### Smallest hermetic test matrix

1. **CLI — one table-driven test**
   - plain `add`;
   - `add --codex`;
   - `add --worktree --codex`;
   - internal `spawn ... --claude-codex`;
   - one unrelated command rejects `--codex`.
2. **Store — two tests**
   - old serialized row without the field defaults to `false`;
   - `false` and `true` round-trip.
3. **Merge — one strengthened regression**
   - dormant resume toggles the flag;
   - only the intended profile/status/tab fields change;
   - label, cwd, repo root, branch, worktree, recency, label source, and stale state survive.
4. **KDL — two focused tests**
   - plain and Codex layouts differ only by `--claude-codex`;
   - cold eager layout derives the flag from its row;
   - both variants pass the existing real Zellij KDL parser guardrail.
5. **Discovery/remediation — one table-driven test**
   - binary name, override variable, shared candidate directories, and missing advice;
   - no Claude-only nvm/local-install candidates;
   - optional wrapper does not appear in ordinary doctor output.
6. **Spawn boundary — one parameterized fake-executable integration test**
   - create/plain, resume/plain, create/Codex, resume/Codex;
   - assert executable, byte-identical Claude argv, cwd, and Codex child `CLAVE_CLAUDE_BIN`;
   - temporary executable overrides and synthetic temporary `CLAUDE_CONFIG_DIR` transcript marker;
   - leave `ZELLIJ_PANE_ID` unset so no real Zellij or real `~/.claude` is touched;
   - assert the launcher itself is not shell-wrapped.
7. **Worktree**
   - extend existing selected-cwd/merge regressions to prove toggling the flag changes no physical worktree cwd or metadata; no separate worktree framework.
8. **Repository gate**
   - `cargo test --workspace`;
   - `cargo build -p clave-bar --target wasm32-wasip1`;
   - `cargo clippy --workspace --all-targets -- -D warnings`.

Preflight ordering remains a code-path invariant checked by review and maintainer-run live validation; this feature does not create a new fake-Zellij harness ahead of the planned tier-2 work.

### Maintainer-run real smokes

These may run in parallel with hermetic implementation but must pass before merge/live acceptance:

1. `claude-codex --version`.
2. Short proxy inference with `--no-session-persistence`.
3. Human-driven Zellij validation: new plain/Codex agents, dormant profile switch, live jump, dead-session resurrection, and a real registered worktree.

A lane that cannot safely run is reported as unverified, never counted as passed.

## Expected repository scope

After rebasing this worktree onto `main` (`50fa26a` or later), behavioral changes should be limited to:

- `crates/clave/src/main.rs` — user/internal flags and final direct-exec selection;
- `crates/clave/src/store.rs` — persisted boolean;
- `crates/clave/src/add.rs` — selection, preflight, KDL snapshot, merge behavior;
- `crates/clave/src/open.rs` — dormant-open preflight and stored flag in KDL;
- `crates/clave/src/setup.rs` — eager-row snapshot, preflight ordering, cold-start KDL;
- `crates/clave/src/discover.rs` — optional wrapper discovery;
- `crates/clave/src/doctor.rs` — centralized remediation text.

`spawn.rs` changes only if a tiny command helper belongs there; moving execution out of `main.rs` is explicitly out of scope.

Mechanical `AgentRecord` literal updates may be required in `dev.rs`, `hook.rs`, `lsview.rs`, and KDL tests. They gain no new behavior. No `clave-types` or `clave-bar` change is expected.

## Delivery order

1. Rebase the isolated worktree onto current `main`.
2. In parallel: Ollie externalizes/smoke-tests the real wrapper while implementation uses fake executables.
3. Add the failing minimal test matrix.
4. Implement the narrow boolean/flag/KDL/discovery/preflight/direct-exec path.
5. Run repository gates and both required review lanes.
6. Provide human-run live-validation commands; do not drive Zellij.
