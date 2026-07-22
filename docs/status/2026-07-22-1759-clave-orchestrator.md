# Status — clave orchestrator (v0.1.1 shipped + field incident closed + agent-autonomy foundation laid)

_2026-07-22 17:59 · repo github.com/olliegilbey/clave · main `8c87bcf` ·
tag `v0.1.1` on `ccd67fb`, released and daily-driven · tree clean_

Predecessor: @docs/status/2026-07-22-1606-clave-orchestrator.md — the v0.1.1 cut
and the **full post-release incident write-up**. Read it for the incident detail;
this file does not repeat it.

## Task Overview

clave = Zellij fleet-orchestration sidebar (wasm `clave-bar` + `clave` CLI).
This session: finished the v0.1.1 milestone, cut and released it, diagnosed and
fixed a production breakage caused by that release's *environment* (not its
code), then laid the foundation for **cloud agents to pick up issues and
self-verify without the maintainer**.

## Current State

**Shipped and merged today** (squash convention, all CI-green):
`c235311` #25→#17 label hygiene · `1526d79` #26→#23+#6 nav re-anchor +
bind-liveness + order-safe prune · `7644fd8` #27→#4 width-seek re-arm ·
`83c4430` #30→#28 Claude-key unbinds · `9f121bc` #32 v0.1.1 bump ·
`ccd67fb` #42→#41 `Alt+t` new tab · `80348e2` #46 PATH-leak docs ·
`fd1cb59` #51→#20 fugu vendored + empty-diff fix · `a24f3fa` #50 agent
contract/tiers/taxonomy · `8c87bcf` #52 artifact version-coherence test.

**Closed:** #4 #6 #17 #20 #23 #28 #41. **Released:** `v0.1.1` installed, the
maintainer is daily-driving it, every bar instance verified on one version.

**Repo is clean as of this handoff:** worktrees reduced to `doctor-install`
(active workstream), the codex worktree (not ours), and this session's
`issue-10-kdl-guardrail` home. Branches reduced to `main`,
`worktree-doctor-install`, `codex/multi-provider-design` — all merged branches
deleted locally and on origin.

## Important Discoveries

1. **The v0.1.1 field incident** (detail in the predecessor): `clave-bar` shells
   out to **bare `clave`** via PATH for `snapshot`/`open`/`bind`/`focus`/
   `touch`/`prune-tabs`/`add`. A stale `0.1.0` dev binary on PATH therefore
   served a v0.1.1 session, composed tab layouts pointing at the OLD wasm, and
   — since zellij keys plugin identity on file **location** — every opened tab
   loaded a SECOND bar. Duplicate sidebar, no shared beacon state, dead nav.
   → **#44** (root cause, fix it FIRST) and **#43** (no unversioned stable
   entry point; `dev-install` collides with the daily launcher name).
2. **`git branch --merged` lies under squash-merge.** A squash creates a new
   commit, so the branch tip is never an ancestor of `main` — every merged
   branch reports as unmerged. Verify against merged-PR head refs
   (`gh pr list --state merged --json headRefName`) instead.
3. **Diagnosis shortcut, now documented:** the bar logs `clave-bar: loaded
   vX.Y.Z` at every load. One grep over zellij's log (`$TMPDIR/zellij-<uid>/
   zellij-log/zellij.log`) tells you instantly whether plugin versions are
   mixed. This found the incident in minutes.
4. **The fugu harness had a silently-passing lane** (#20, fixed in the vendored
   copy): the codex lane hardcoded `git diff <base>...HEAD` regardless of
   `type`, so reviewing UNCOMMITTED work diffed nothing and returned "nothing
   significant". `diffExpr()` now derives the range from `type`, and both codex
   and gemini report an EMPTY diff as a finding. **The maintainer's own global
   copy outside this repo still has the bug** — worth backporting.
5. **External review lanes keep earning their place.** CodeRabbit CLI and Codex
   caught FIVE real defects this session that the implementer agents, the
   adversarial reviewer agents, and the principal pass all missed: the
   cross-process prune race, the prune drop-under-lag, the stale width-seek
   anchor, the artifact-coherence union hole, and (historically) the clap
   `ArgAction` bug. This is why AGENTS.md now *requires* two review lanes.
6. **The maintainer's git hooks are mid-build and can fail a push.**
   `~/.config/git/hooks/pre-push` (made executable 2026-07-22 17:33) replays
   stdin through a heredoc; when `REFS` is empty the heredoc still emits one
   blank line, the loop runs with all variables empty, and it executes
   `git rev-list ..` → `fatal: '..' is outside repository`. Fix:
   `[ -z "$local_sha" ] && continue`. Separately, `gpg.ssh.program` is his
   `sign-with-fallback.sh` wrapper; a transient 1Password agent error aborts
   the commit with an explicit **"Do NOT retry this as Claude"** — obey it and
   ask him.

## Rulings made this session (binding)

- **Autonomy contract** — an agent may implement a tracked issue end to end,
  run the review gauntlet, and gate on green CI. It **asks before merging, and
  may execute the merge once approved.** Six things it never does are
  enumerated in AGENTS.md, including the `dev-install` prohibition.
- **`main` is guaranteed green + reviewed + hermetically verified; it is NOT
  guaranteed live-validated.** The **tag is the promotion event.** Human
  validation is batched into one pass per cut (#49), not per PR. The maintainer
  is the LAST line of defence, not the first.
- **Build the tier-2 real-zellij harness right after #44** (#47).
- **`.claude/settings.json` is tracked** — a clone (including a cloud agent)
  gets the same plugin setup.
- **Handoffs remain tracked** and ride the next PR (#22 ruling, unchanged).

## New in the repo (read these before working)

- **`AGENTS.md`** — tracked, and the entry point for any agent. The autonomy
  contract, required review lanes, verification tiers in brief, handoff duty.
  Cloud agents are told to skip fugu's external CLI lanes (not installed, need
  interactive auth) — **a lane that did not run is not a lane that passed.**
- **`docs/dev/TESTING.md`** — restructured: three verification tiers, the **risk
  taxonomy** (change class → required evidence → label), an **escape record**,
  then the original live SOP intact.
- **`.github/PULL_REQUEST_TEMPLATE.md`** — the verification dossier, including
  an explicit "could NOT be verified, and why".
- **`.claude/`** — the vendored fugu harness (workflow + command + README) and
  `settings.json`.
- **`CLAUDE.md` corrected** — it used to declare `just dev-install`
  unconditionally sanctioned. That instruction is what broke production.

## Next Steps

1. **#44 first** — the bar's PATH-resolved shellouts. Root cause of the
   incident; recurs the moment any agent builds a dev CLI. Preferred fix
   (design note on the issue): pass the absolute binary path into the plugin via
   its layout `configuration` block, plus a version-skew refusal, guarded by a
   KDL guardrail test in the #10/#28 style.
2. **#43** — an unversioned stable entry point so "launch what I just released"
   has an answer; consider renaming the dev binary `clave-dev` (note: `clave dev
   launch` and the sandbox config both bake bare `clave`, so it must thread
   through config generation).
3. **#47** tier-2 harness → **#48** doctor coherence → **#49** release checklist.
4. Product backlog: **#38** status glyph fidelity (red lingers, amber never
   appears), **#39** row ordering vs the §6.6 commitment-gated design (decide in
   the #24 brainstorm), **#40** Nerd Font portability, **#31** dev-install
   sandbox-config gap, **#45** pre-existing pipe noise, **#24** the UI epic
   (heavily pre-designed in its comments — read them all).
5. Backport the fugu `diffExpr` fix to the maintainer's global copy, and fix his
   `pre-push` hook (Discovery 6).

**Where work stopped — verbatim (maintainer):** "Yeah, let's do it." — approving
the merge of #50/#51/#52, the repo cleanup, and this handoff. All three merged,
cleanup executed, this file is the last act.

## Context to Preserve

- **User prefs (binding):** extremely concise, signal over noise; explain while
  doing; dense why-comments citing spec §/ledger/issue; conventional commits +
  `Claude-Session:` trailer; **never commit without explicit approval** (he
  signs via 1Password); ask before architecture decisions with multiple valid
  approaches; he drives ALL live zellij input.
- **Verification bar:** `cargo test --workspace` (load-bearing) ·
  `cargo build -p clave-bar --target wasm32-wasip1` · `cargo clippy
  --workspace --all-targets -- -D warnings` (**`--workspace` matters** — the
  default-members form skips the wasm crate; CI uses `--workspace`) ·
  rustfmt on NEW code only, never full-file reflow of model.rs/store.rs.
- **Pre-commit PII blocklist** rejects private local path names in staged
  lines — genericize them. It fired twice this session.
- **`~/.claude/` is READ-ONLY**; orphaned-cwd sessions: recreate the dir at the
  same absolute path, never relocate jsonls.
- **SSH constraint:** clave must eventually work over SSH — reject designs
  assuming the CLI and plugin share a local desktop.
- Merge mechanics: auto-merge is DISABLED and all review threads must be
  resolved; sequence is resolve threads → `gh pr update-branch` → wait for CI →
  `gh pr merge --squash`.

## Restart Hint

Everything merged, released, and cleaned; tree clean, branches pruned, `v0.1.1`
live. Safe to /clear. Resume at **#44** — read AGENTS.md first, then
CONTRIBUTING's "The one leak" and TESTING.md's risk taxonomy, which are new and
are now the operating contract.
