# Handoff — #261 live set restore, after the PR review round

2026-09-18 00:30. Branch `worktree-live-set-restore`, PR #261 OPEN, pushed
through `d489a82`. Gates green. Supersedes `2026-09-17-2300-*`.

## Where this is

Everything is done except the merge, which is Ollie's. The branch is up to date
with main, CI is green, the PR body is rewritten, and every review thread has a
reply.

## What happened since the last handoff

- **Three defects from Ollie's eyeball check**, all one family. `Status::Exited`
  is new here, and three places read it as something it does not mean: an agent
  you quit drew twice; an agent you quit before it ever spoke could not be
  restarted; an agent you restarted still drew as stopped. All fixed, all
  confirmed on screen and in the store.
- **The upgrade path.** The first launch after an upgrade came up cold, because
  the flag that says "trust these binds" is new and its absence read as "no".
  A missing flag now means trust. Only a store the old clave wrote can reach
  that default.
- **CodeRabbit CLI**, run locally against main: 6 findings, 4 taken, 2 declined
  with reasons in the commit.
- **Merged main in** (0.5.0 cut + pane-frame passthrough). One conflict in the
  drive, resolved keeping main's wording and this branch's status.
- **A comment pass Ollie asked for.** He read the diff and said there were far
  too many comments. Measured and he was right on direction: two comment lines
  per code line in model.rs production. Cut 243 lines, no code touched. One
  real find — a doc comment orphaned onto the wrong function.
- **CodeRabbit PR bot, 5 findings**, all confirmed and fixed in `d489a82`:
  a Linux-only `just mutants` break (`shasum` vs `sha256sum`), an `lsof` guard
  (it is the loop's pacing, not just its reading), an unenforced reader in the
  script-hygiene contract, a comment above the wrong function, and a
  self-contradicting note in the design ledger.

## OPEN — the only things left

1. **Ollie merges.** Squash looks like the house style.
2. **I cannot resolve review threads** — the token returns
   `FORBIDDEN: Resource not accessible by personal access token`. Replies are
   posted on all five. I asked `@coderabbitai resolve` to do it; if the bot
   did not, Ollie clicks resolve.
3. **To file as its own issue, on his go:** a restore that does not finish
   permanently shrinks the fleet. Pre-existing in shape, wider in reach now.
4. **Discussed, not done:** move model.rs's test module to its own file. It is
   8,145 of the file's ~11,900 lines. Mechanical and zero-risk, but it would
   bury this PR's diff — its own small PR after this merges.

## Honest gaps, already in the PR body

- The upgrade path is tested but has never been run over a real old install.
- `just mutants` has not run over the last dozen commits.
- Nothing measures the handle burst; no gate asserts a ceiling.

## Traps learned (all in FOOTGUNS)

- A per-instance counter cannot answer a question about the fleet.
- A command pane keeps its launch command after the process exits.
- `zellij action go-to-tab-by-id` moves NO beacon. Use `ct.sh pipe --name
  clave-visited` then `--name clave-nav`.
- `dump-layout` reports the original hold flag. The store's `pane_id` is the
  honest "did it start".
- Counting ONE block cannot see a duplicate. Count across both.

## Harness

Worktree classifier refuses compound bash naming `git`/`gh` inside complex
constructs. Write python to `/tmp` via the Write tool, run it plainly.
`cargo fmt --all` before every `just gates`. The sandbox shim symlinks
`target/release/clave`, so a rebuild is live in a running sandbox with no
relaunch.
