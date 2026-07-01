# Spike S0 + S0b — findings log

**Harness:** `spikes/s0-create-and-munge.sh`
**Status:** PENDING — this is the template; the harness has not been run yet. It
launches real Claude sessions (network + tokens), so it is run interactively by
a human with a live terminal, not by an agent.

**What's being verified:**
- **S0:** does `claude --session-id <fresh-uuid>` *create* a new session and
  write `~/.claude/projects/<munged-cwd>/<uuid>.jsonl`, rather than erroring or
  resuming? The idempotency model in spec §6.1 (`clave spawn` checks for the
  jsonl, creates on absence, `--resume`s on presence) rests entirely on this.
- **S0b:** does our `munge_cwd` helper (Task 3;
  `cargo run -q -p clave --example munge -- <path>`) compute the same
  directory name Claude actually writes, across plain / dotted / worktree cwds?

---

## S0 verdict: fresh `--session-id` creates a jsonl

**Result:** PENDING — fill in after interactive run

- Does a fresh `--session-id <uuid>` create the jsonl? (PASS/FAIL)
- Which launch mode actually persisted: `-p` (headless print mode, Step 2) or
  the interactive fallback (Step 3)? Record this — it doesn't change
  `clave spawn` (which always launches interactively) but affects whether this
  harness is reproducible as written.
- Per-cwd results (plain / `dot.dir` / worktree): PASS or FAIL for each, with
  the actual jsonl path observed.

## S0 pre-existing-uuid behavior: resume vs error

**Result:** PENDING — fill in after interactive run

- Re-running `claude --session-id <same-uuid>` a second time: did the jsonl
  grow (silently resumed) or did the command error / exit non-zero (hard
  error)?
- `BEFORE` line count: PENDING
- `AFTER` line count: PENDING
- Exit code of the second invocation: PENDING
- **Does this confirm or contradict spec §6.1's stance** ("a UUID collision is
  a genuine error: surface it, don't silently resume")? PENDING

## S0b verdict: munge_cwd matches disk

**Result:** PENDING — fill in after interactive run

| cwd shape | computed dir (munge) | actual dir (disk) | match? |
|---|---|---|---|
| plain (`$ROOT/plain`) | | | PENDING |
| dotted (`$ROOT/dot.dir`) | | | PENDING |
| worktree (`$ROOT/base/.claude-worktrees/wt`) | | | PENDING |

- If any mismatched: paste the actual dir name from the harness's
  `grep -rl "$UUID" "$PROJECTS"` output here, and state the corrected rule.
- **Known unknown (not covered by this spike):** S0b covers ASCII paths only.
  The `café → caf-` unit test pins *our* per-`char` munging rule, but whether
  Claude munges non-ASCII cwds byte-wise or char-wise is unverified against
  disk. Low risk — cwds are rarely non-ASCII — but if a future mismatch shows
  up, start here.

---

## Fallbacks if FAIL

- **S0 fails** (no create / always resumes on a fresh uuid) ⇒ the idempotency
  model in spec §6.1 is wrong — **stop and revise §4/§6.1 before writing
  `clave spawn`.**
- **S0b mismatch** ⇒ derive the true rule from the observed dir name, update
  `munge_cwd` + its unit tests + spec §4, and re-run this spike.

---

## Raw harness output

PENDING — paste the full stdout of `./spikes/s0-create-and-munge.sh` here
(and, if Step 3's interactive fallback was needed, that transcript too).
