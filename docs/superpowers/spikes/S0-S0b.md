# Spike S0 + S0b — findings log

**Harness:** `spikes/s0-create-and-munge.sh` (the committed harness supersedes the
plan's Task 4 Step 1 draft: it now canonicalizes the cwd before munging and pins
`--model haiku`.)
**Status:** ✅ RUN 2026-07-01 (main session drove it against real Claude via `claude -p`).

**What's being verified:**
- **S0:** does `claude --session-id <fresh-uuid>` *create* a new session and write
  `~/.claude/projects/<munged-cwd>/<uuid>.jsonl`, rather than erroring or resuming?
- **S0b:** does `munge_cwd` compute the same directory name Claude actually writes,
  across plain / dotted / worktree cwds?

---

## S0 verdict: fresh `--session-id` creates a jsonl — ✅ PASS

- A fresh `--session-id <uuid>` **creates** the session and writes the jsonl —
  confirmed for all three cwd shapes.
- **Launch mode:** headless **`-p` (print mode) DOES persist** the jsonl; the
  interactive fallback (Step 3) was not needed.

## S0 pre-existing-uuid behavior — ✅ HARD ERROR (confirms §6.1)

Re-running `claude --session-id <same-uuid>`:
```
Error: Session ID b60387fb-…-565dfebee501 is already in use.
   exit=1
```
- Exit code **1** — it does not silently resume.
- Confirms spec §6.1 ("a UUID collision is a genuine error: surface it, don't silently
  resume"). ⇒ `clave spawn` must detect the existing jsonl (via the *canonicalized*
  join key) and use `--resume`; taking the create path on an existing session errors.
- (The harness's `before=0 after=NA` counts were an artifact of the pre-fix
  un-canonicalized path — the very bug S0b found. The collision error is the real
  result.)

## S0b verdict: munge_cwd matches disk — rule ✅ correct, INPUT must be canonicalized

Sessions were created but landed at `-private-var-…` while `munge_cwd(logical cwd)`
computed `-var-…`. The only delta: macOS `/var` → `/private/var`. Claude reads
`getcwd()` (physical path, symlinks resolved) and munges *that*.

| cwd shape | munge(logical) | munge(physical) = on disk | match |
|---|---|---|---|
| plain | `-var-folders-…-plain` | `-private-var-folders-…-plain` | physical ✅ |
| dotted (`dot.dir`) | `-var-…-dot-dir` | `-private-var-…-dot-dir` | physical ✅ |
| worktree | `-var-…-base--claude-worktrees-wt` | `-private-var-…-base--claude-worktrees-wt` | physical ✅ |

Re-verified directly: `munge_cwd(pwd -P)` equals the on-disk dir for all three (the
`~/.claude/projects/<that>` dir exists). The worktree `--` double-dash matched in both,
so the `s/[^A-Za-z0-9]/-/g` **character rule is correct and unchanged**.

**Corrected rule:** callers must `std::fs::canonicalize` the cwd (resolve symlinks to
the physical path) **before** `munge_cwd`. Folded into spec §4, `munge.rs` module doc,
and the ledger's carried-forward decisions (a hard requirement for the `clave spawn` /
`clave add` subsystem plan). `munge_cwd` itself needs no change.

**Known unknown (still uncovered):** S0b exercised ASCII paths only. Whether Claude
munges a non-ASCII cwd byte-wise or char-wise is unverified on disk. Low risk.

---

## Cost note (raised by user, 2026-07-01)

Initial concern: `claude -p` might bill the **API** (pay-per-token) rather than the
subscription. On this run **no usage credits were observed to be spent**, so it appears
to use the subscription — **treat as UNCONFIRMED**. The harness pins `--model haiku` as
a cheap default regardless. Either way this touches the **test harness only**: the clave
**product** never uses `-p` — `clave spawn` launches the interactive Claude TUI
(invariant #1), which is subscription-billed.

---

## Fallbacks (not triggered — recorded for completeness)
- S0 fail (no create / always resumes) ⇒ idempotency model wrong; revise §4/§6.1. — N/A, S0 passed.
- S0b mismatch ⇒ derive the true rule, update munge_cwd + tests + §4, re-run. — Resolved: the rule was right; the fix is canonicalizing the input (spec §4 updated).

---

## Raw harness output (first run, pre-canonicalization-fix — this is what surfaced the finding)
```
=== S0 + S0b: fresh-uuid create + munge-matches-disk ===
-- cwd=…/clave.spike/plain  uuid=d1ffc779-…
   FAIL not at computed path: …/-var-folders-…-plain/d1ffc779-….jsonl
   where did the uuid actually land? ->
     …/-private-var-folders-…-plain/d1ffc779-….jsonl
   (dot.dir and worktree identical: computed -var-…, actual -private-var-…)
=== S0: pre-existing-uuid behavior (resume vs error) ===
-- re-running with the SAME uuid:
Error: Session ID b60387fb-…-565dfebee501 is already in use.
   exit=1
```
The delta is uniformly the `/private` prefix, confirming the getcwd()-canonicalization
finding. Re-running the (now-fixed) harness munges the physical path and reports PASS.
