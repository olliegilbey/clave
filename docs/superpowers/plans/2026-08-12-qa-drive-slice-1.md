# Plan — QA drive slice 1: `qa-fleet` + `qa-drive.sh` phases 0–2 (#182)

Spec: GitHub issue #182 (ratified via the 2026-08-12 grill). Design:
docs/dev/QA-DRIVE.md (ratified). Evidence base: docs/dev/qa/ (101 classes).
This slice is the executable #178 harness; it lands with the drive
documented RED on P9 — red is the deliverable, the fix is the next PR.

## Global Constraints

- `just gates` green on every task's commit: `cargo fmt --all --check`,
  `cargo test --workspace`, `cargo build -p clave-bar --target wasm32-wasip1`,
  `cargo clippy --workspace --all-targets -- -D warnings`.
- NEVER run zellij commands against any live session. Sandbox access is
  `scripts/ct.sh` only. Do not launch or kill zellij sessions. Do not write
  `~/.local/share/clave/` or `~/.local/state/clave/`.
- No `/Users/` paths in any committed file content.
- TDD at the pre-agreed seams only: the scenario table (dev.rs `SCENARIOS` +
  `ScenarioAgent`), uuid minting, resume-target fidelity. The drive script
  tests external behaviour via store/log/ct.sh probes only — never model
  internals.
- KISS: implement the task scope only; no speculative flags, no extra
  phases, no CI wiring.
- Match surrounding code idiom and comment register; commits in the repo's
  narrative style (see `git log --oneline`), no AI attribution.

## Task 1 — Commit the QA evidence docs

Mechanical. The worktree already contains (untracked, copied from the main
checkout — do not modify content):

- `docs/dev/QA-DRIVE.md` (ratified design)
- `docs/dev/qa/` (index + 7 plane files)
- `docs/status/2026-08-11-1830-v013-part-c-drive.md`
- `docs/status/2026-08-12-1219-v013-fix-and-qa.md`

Verify `grep -rc '/Users/' docs/dev/qa/ docs/dev/QA-DRIVE.md docs/status/2026-08-1*.md`
shows zero matches per file (report any hit as BLOCKED — do not edit
evidence docs silently), then `git add` exactly these paths and commit:
`docs: the QA drive design (ratified) and the 101-class breakage inventory (#182)`.
Gates: fmt/clippy/test unaffected but run `cargo fmt --all --check` +
`cargo clippy --workspace --all-targets -- -D warnings` as a smoke pair.

## Task 2 — The `qa-fleet` scenario and the faithful rotated row

In `crates/clave/src/dev.rs`, TDD each slice red→green:

1. **`rotated: bool` field** on `ScenarioAgent` (default false, same idiom
   as `worktree`/`delete_cwd_after`).
2. **Second mint**: `scenario_rotated_uuid(n)` =
   `format!("00000000-0000-4000-8000-c85c{:08}", n + 50)` — keeps the
   `c85c` prefix (so `is_scenario_jsonl` sweeps it and `dev reset` cannot
   leak it), avoids collision with `scenario_uuid` for any scenario under
   50 agents (largest today: `tall` at 34), and keeps `uuid_tag`
   (last-8-chars) unique. Pin with a unit test beside the existing
   `scenario_uuid` pin, asserting prefix, determinism, and non-collision
   against `scenario_uuid(1..=50)`.
3. **Seeding**: for a rotated agent, `run_scenario` seeds a SECOND real
   transcript via the same `claude -p --session-id <rotated-uuid>` path at
   the same cwd, guarded by the same `seed_needed` idempotence check as the
   first. `agent_record` sets `live_session: Some(rotated)` for rotated
   agents (replacing the unconditional `None`; update the now-inaccurate
   "no transcript at all" comment). Unit test: a rotated `ScenarioAgent`
   yields a record whose `live_session` is the rotated mint; a plain one
   stays `None`.
4. **Resume fidelity**: unit test at the spawn seam — for a rotated row
   whose rotated jsonl exists, `resume_target` resolves the rotated id, not
   the minted uuid (prior art: existing `resume_target` tests in spawn.rs;
   if the jsonl-exists gate makes this need a tempdir fixture, follow the
   existing fixture idiom in dev.rs tests).
5. **`qa-fleet` scenario entry**: 6 dormant agents in the `SCENARIOS`
   table — one `worktree: true`; one stale (`worktree: true` +
   `delete_cwd_after: true` — the pinned caveat: a stale agent sharing a
   repo must be a worktree, dev.rs has the comment); one
   `rotated: true`; three plain. Staggered `ago_secs`, distinct repos where
   shapes demand it, no titles needed. Update the scenario-list pin test.
   All dormant (no live-style rows): phase 2 wakes them; the eager row at
   launch is whichever the human's launch line names.

Commit: `dev: the qa-fleet scenario, and the rotated row finally carries a real second transcript (#182)`.

## Task 3 — `scripts/qa-drive.sh` phases 0–2

New bash script, shellcheck-clean, executable, same defensive idiom as
`scripts/ct.sh` (fail-closed on missing instance/socket). Signature:
`scripts/qa-drive.sh <scenario>`. It assumes the human has ALREADY launched
the staged sandbox session; it never launches or kills.

Structure:

- Resolve the per-worktree instance (`clave dev instance` fields) for
  state dir, session name, log paths. Refuse to run if the instance's
  zellij session is not live (fail closed, message says what to stage).
- Helpers: `phase(name)` opens a phase; `check(desc, measured, expected)`
  prints `[<phase> <ts>] CHECK <desc>: measured=<v> expected=<v> PASS|FAIL`
  — measured values ALWAYS printed, empty printed as the word `empty`.
  First FAIL stops the run (print `PHASE <n> FAILED`, exit non-zero,
  leaving the log and sandbox intact for forensics). Every line tees into
  `<state-dir>/qa/drive-<epoch>.log`; nothing to /dev/null.
- Zellij-log mark: record the byte offset of the sandbox zellij log at
  start; all zellij-log reads are lines after the mark.
- **Phase 0 — preflight**: build tag present on the loaded wasm tail
  (post-launch, marked-read); generated `config.kdl`/`layout.kdl` coherent
  at one version and `launch.kdl` asserted only post-launch (the
  stale-by-design trap); permission cache seeded for both key forms; zero
  orphan `zellij pipe` processes (`pgrep -f`, count printed).
- **Phase 1 — baseline join**: `clave dev status` rows == scenario
  expectation (6 dormant); the eager-launch row's `tab_id` bound (the #178
  resume face); viewport geometry measured via ct.sh and RECORDED (a
  measurement, not an assertion); store↔layout join printed with
  unresolvables marked `UNRESOLVED`, never filtered; store `seq` recorded.
- **Phase 2 — bind ladder, mixed paths**: ~6 binds — wake dormant rows via
  nav pipes (`{"row":N}` pick + commit through ct.sh), plus ≥1 scripted
  create (the CLI create path). After EACH bind, bounded wait (10s poll):
  `tab_id` lands in store; dormant count decremented; expected EOF-twin
  delta exact (pipes-sent × live instances, computed and compared);
  seek-trace resting width == target, labelled `belief` in the log. Per
  rung, print a one-line budget diagnosis: binds attempted vs binds landed
  so far (the "2 then never again" signature is THE discriminator).
- Exit summary: per-phase PASS/FAIL table, log path, and the reminder that
  phases 3–7 are not yet built.

Validation for this task (no live session available to the implementer):
`shellcheck` clean; `bash -n` clean; a `--help`/usage path; fail-closed
behaviour demonstrated by running against a nonexistent instance and
capturing the refusal. Live red-run happens with the maintainer after
merge prep — NOT this task's job.

Commit: `qa: the drive script exists — phases 0–2, one log, nothing discarded (#182)`.

## Task 4 — Runbook and TESTING.md integration

- `docs/dev/RELEASE-RUNBOOK.md` Part A gains exactly one line: qa-drive
  (all built phases) green on the release candidate.
- `docs/dev/TESTING.md`: in the sandbox drive loop section, one short
  pointer paragraph to QA-DRIVE.md and `scripts/qa-drive.sh` as the
  executable form of steps it automates (do not delete the manual steps —
  they remain the fallback and the phases 3–7 source).
- Do not restructure either document.

Commit: `docs: the release gate learns to ask for a green drive (#182)`.
