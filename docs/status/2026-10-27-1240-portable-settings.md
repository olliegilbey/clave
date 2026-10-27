# Task Pickup

You are picking up this work session from a prior agent that was making clave write machine-neutral commands into Claude's shared settings file, after the 0.5.7 cut. 0.5.7 is released and published (tag pushed, release workflow green, both machines on 0.5.7). This branch, `fix/home-relative-hooks` (worktree `.claude-worktrees/homehooks`), carries the fix.

## Orientation

- Up to 0.5.7 a cut wrote `/home/ollie/.local/share/clave/bin/clave-v0.5.7 hook <Event>` (box) and `/Users/olliegilbey/...` (Mac) into `~/.claude/settings.json`, which the maintainer's dotfiles share between both machines | Checked | `grep clave ~/dotfiles/src/.claude/settings.json`
- Hooks must stay BARE `clave`: the sandbox needs bare so its shim (first on the pane PATH) runs the build under test (qa-drive.sh phase 0 "the clave a sandbox hook resolves"); the live session resolves bare through the launch folder on PATH (#282) | Checked from source and the #282 commit | `git log -1 9204432`
- The statusLine wrap uses `"$HOME"/.local/share/clave/bin/clave`, not bare: the status line also runs in Claude sessions outside clave, where PATH may lack clave and a missing command blanks the line | Inherited, docs read | code.claude.com/docs/en/statusline ("The `command` field runs in a shell"); /hooks "Shell form" (`sh -c`)
- The maintainer's live settings.json still has the 0.5.7 absolute form until the next cut or sandbox stage; `settings_portability_checks` is red on it today, by design | Checked | `bash -c 'source scripts/qa/lib.sh; settings_portability_checks ~/.claude/settings.json'`
- Whether the cargo-dist installer path on the Mac also writes the new form | Open | `run_setup`'s single-file arm passes an absolute binary, so it should; confirm on the Mac after the next release with the same check

**Method.** Grep `script_hygiene.rs` rules before adding drive code: a literal ` hook ` in lib.sh or the selftest reads as a hand-fired hook (spell the space `[ ]` or `\x20`).

**Proof.** `just gates` green (918 passed); `bash scripts/qa/lib-selftest.sh` 0 failures; the two settings tests go red when either half is reverted to the binary path.

## Current State

- Code: `crates/clave/src/setup.rs` (`HOOK_BIN`, `STATUSLINE_BIN`, `merge_claude_settings`, tests `settings_read_the_same_on_every_machine_cut_and_stage`, `the_statusline_wrap_names_the_installed_launcher`); comments in `release.rs`, `main.rs`.
- Drive: `settings_portability_checks` in `scripts/qa/lib.sh`, selftest cases, called in `scripts/qa-drive.sh` phase 0 after `hook_registration_checks`; registered in `script_hygiene.rs`.
- Docs: FOOTGUNS ("Claude's settings.json is shared" and the amended sandbox-stage entry), UBIQUITOUS_LANGUAGE ("launcher", "versioned copy"), CONTRIBUTING ("The maintainer's setup: a Mac, and the box over ssh"), RELEASE-RUNBOOK (settings check after a cut).

## Next Steps

1. The maintainer merges the PR. The next cut (0.5.8) rewrites settings.json on each machine; then the agent runs the settings check above, and the maintainer commits his dotfiles (settings.json, statusline.sh, the .zshrc PATH line) once.
2. Optional: a QA drive on this branch shows `settings_portability_checks` green after the stage rewrites the hooks (the statusLine line stays a note until a cut).

## Context for the Work

Open asks for the maintainer: merge this PR; cut 0.5.8 when he wants it; then the dotfiles commit; the three row-status hand tests; his friend's install; the wasm benchmark decision (saved in `~/.cache/clave-perf/costprobe-save/`, recommend yes); brand icon colours stay fixed. Pushes, PRs and merges wait for his go; he asked for this PR. The agent cannot push tags (auto-mode refuses); hand him `command ssh devbox 'cd ~/code/clave && git push origin vX.Y.Z'`.

## Restart Hint

Committed on `fix/home-relative-hooks`; gates green.

## Suggested Skills

`superpowers:verification-before-completion` before claiming the next cut is clean.
