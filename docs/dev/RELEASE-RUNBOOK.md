# RELEASE-RUNBOOK.md — cutting a version, and proving it

**Every release cut ends with an interactive live test driven by the
maintainer. There is no exception and no automated substitute** — Tier 2 does
not exist (#47), so nothing automated crosses the process/environment seam, and
that seam is exactly where the v0.1.1 breakage lived (#43, #44).

This runbook exists to make that live session **short and decisive**. Everything
an agent can settle beforehand is settled in Part A; the human's session is
Part C, and every step there says what to type, what to look at, and what to
report back.

Read [TESTING.md](TESTING.md) first — the interaction contract, the observability
map and the sanctioned-command list are binding here and are not repeated.

**Roles, restated because this is where they matter most:**

| Who | Does |
|---|---|
| **agent** | Part A, the QA-drive gate (once the maintainer has launched its sandbox session), and Part D. Reads logs, the store, `clave dev status`. During the gate, runs **sandbox-scoped** zellij actions through `scripts/ct.sh` only; **prints** launch and kill commands, and every other zellij command; never launches or kills a session. Never runs `just release`. |
| **maintainer** | The tag, `just release`, launching the QA drive's sandbox session, every keypress in Part C, killing a session, and the go/no-go. **The tag is pushed only after the go** — Part B. |

---

## Part A — before the tag (agent, unattended)

Nothing here touches a live session. All of it must pass before a tag exists.

1. **`main` is green at the commit you intend to tag.**

   ```bash
   just gates          # CI's four commands, in CI's order
   gh run list --branch main --limit 1
   ```

2. **The pre-tag blocker set is closed.** As of 2026-07-25 that is **#43a**
   (the release owns an unversioned launcher), **#44** (landed in #66), **#48**
   (doctor version-coherence), **#43b** (`dev-install` no longer writes the
   daily launcher name). Cutting without #43a **deterministically reproduces the
   double-sidebar incident** — see Part C step 3 for why.
3. **The version in `Cargo.toml` matches the tag you are about to push.** The
   `clave release` gate enforces this, but finding out here is cheaper.
4. **Record the *current* state, so Part C has a baseline to compare against:**

   ```bash
   clave --version; command -v clave
   ls ~/.local/share/clave/bin/
   clave doctor --json          # once #48 lands; until then, `clave doctor`
   ```

   Paste that into the release issue. A live test with no "before" is guesswork.

---

## The QA-drive gate — after Part A, before the tag (agent drives, maintainer launches)

This one cannot live in Part A, because Part A is unattended and the drive needs
a live sandbox session — and session lifecycle is never the agent's. It is still
a **pre-tag** gate: a red drive means there is nothing worth tagging.

**`qa-drive` (all built phases) is green on the release candidate.** The agent
stages the sandbox and prints the launch line; **the maintainer launches the
session** and hands it back; the agent then runs the drive and reports the
per-phase table with measured values and the kill pair. The two eyeball
checkpoints are the **maintainer's**: the agent requests them, the maintainer
looks and returns the observations, the agent records them verbatim
(TESTING.md owns visual observation). The protocol and the phase spine are in
[QA-DRIVE.md](QA-DRIVE.md); the loop it scripts is TESTING.md's sandbox drive
loop.

---

## Part B — the cut (maintainer, watched)

> **The tag stays LOCAL until Part C returns a go.** `git push origin vX.Y.Z`
> triggers `release.yml` and publishes the GitHub release immediately, so a
> defect Part C finds after a push is already public. The gate does not need a
> pushed tag: `clave release` reads `git tag --points-at HEAD` (release.rs
> `release_gate`), which a local tag satisfies.

1. **Tag locally.**

   ```bash
   git tag vX.Y.Z
   ```

2. **Cut.** This is the step that actually puts the new version on this
   machine, and **Part C is meaningless without it** — until `just release`
   runs, `~/.local/share/clave/` still holds the *previous* version's artifacts
   and Step 1 would validate the old cut.

   ```bash
   just release
   ```

   It builds the wasm and the embedding CLI, then `clave release` installs the
   versioned artifacts and rewrites `config.kdl` and `layout.kdl` (setup.rs
   `write_generated`). It does **not** write `launch.kdl` — that is `clave`'s
   own cold-start job, and Step 3 is where it gets checked.
   **`just release` is the maintainer's command, always. An agent never runs
   it.**

3. **If the release adds a store field the hooks populate, the COLD RESTART is
   the migration. There is no warm path — do not invent one.** `clave release`
   writes the new versioned hook commands into `~/.claude/settings.json`, but
   the agents already running keep firing the *previous* binary, which has no
   such field. Prompting them to "warm" the rows therefore cannot work: the
   process doing the writing is the one that lacks the field. This was tried on
   the v0.1.2 cut and cut again the same hour — FOOTGUNS has the evidence.

   Say plainly in the release notes what a row loses by carrying a default
   through the restart, and move on. Attempting a backfill is worse than the
   default: the store cannot distinguish "never populated" from "correctly
   equal to the default", so any backfill is a guess, and a wrong guess is
   unrecoverable where a default merely falls back.

   *v0.1.2 instance:* `AgentRecord::live_session` (#99). A row that was
   `/clear`ed since its last resurrection resumes its minted uuid once, so it
   reopens the pre-clear conversation. Rows that never rotated are unaffected —
   null is their correct value. Nothing is destroyed either way: a stranded
   conversation stays reachable via `claude --resume <id>` from its directory,
   just not through clave. The general fix is a versioned store schema (#106).

4. **Run Part C.** Do not continue past it without a go.

5. **On a go — publish the tag:**

   ```bash
   git push origin vX.Y.Z
   ```

   Then watch `release.yml`. #35 owns the CI-pipeline checklist — all four
   targets build, attestations verify, checksums publish.

   **On a no-go:** `git tag -d vX.Y.Z` and follow Rollback. A tag that was
   never pushed costs nothing to delete; a published one costs a retraction.

---

## Part C — the interactive live test (maintainer drives every step)

> **Before you start:** this test is about *version coherence across a cold
> start*, because that is the failure this project has actually shipped. It is
> not a feature tour.

### Step 0 — write the checks once, to a file

Steps 2 and 3 run the **same two checks against different files**. Defining them
once is not tidiness: the first draft of this runbook duplicated them, and the
copy that guarding `launch.kdl` was missing the unversioned probe entirely. Two
copies drift; one cannot.

They go to a **file**, not just your shell, because Step 3 runs its checks from
a *second pane* — a different shell, which would not inherit them.

**You type:**

```bash
cat > "$TMPDIR/clave-release-checks.sh" <<'CHECKS'
STABLE=("$HOME/.local/share/clave/config.kdl"
        "$HOME/.local/share/clave/layout.kdl"
        "$HOME/.config/zellij/config.kdl")
LAUNCH=("$HOME/.local/share/clave/launch.kdl")

# Every clave version a generated file references, deduped, one per line.
# The second grep is load-bearing: without it a coherent set prints BOTH
# `clave-bar-vX.Y.Z.wasm` and `clave-vX.Y.Z` — two lines for one version —
# and reads as skew.
clave_versions() {
  grep -rhoE 'clave-bar-v[0-9]+\.[0-9]+\.[0-9]+\.wasm|clave-v[0-9]+\.[0-9]+\.[0-9]+' "$@" 2>/dev/null \
    | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+' | sort -u
}

# Every UNVERSIONED clave reference. ANY output is a failure.
# Targets the three sites a version can go missing — the `clave_binary`
# plugin-config value, a keybind's `Run`, and the `file:` plugin location
# (setup.rs config_kdl/layout_kdl) — rather than bare `clave`, which would
# match every line: the data dir is itself ~/.local/share/clave/.
clave_unversioned() {
  grep -nE 'clave_binary "clave"|Run "clave"|file:[^"]*clave-bar\.wasm' "$@" 2>/dev/null
}
CHECKS
source "$TMPDIR/clave-release-checks.sh"
```

**In every new shell or pane this runbook sends you to, source it first:**

```bash
source "$TMPDIR/clave-release-checks.sh"
```

### Step 1 — prove what is on PATH

**You type:**

```bash
command -v clave; clave --version
ls -l ~/.local/share/clave/bin/
```

**Look at:** whether `clave` resolves to a release artifact or to something
else — a `cargo install` dev build, a shim, a stale copy.

**Report back:** both lines verbatim, plus the `bin/` listing.

| What you see | Conclusion | Next |
|---|---|---|
| `clave` resolves inside `~/.local/share/clave/bin/` and its version == the tag | coherent | Step 2 |
| `clave` resolves to `~/.cargo/bin/clave` | **STOP.** This is the v0.1.1 mechanism: whatever lives at that name will cold-start and bake ITS version into `launch.kdl`. Do not stop at "but that one is fine" — on the v0.1.2 cut it was byte-identical to the `clave-v0.1.1` release artifact, and it was still wrong, because nothing updates it and it outranks the launcher forever. Provenance is not the question; which path wins is | **In this order, or you are left with no `clave` at all:** (1) prepend `~/.local/share/clave/bin` to PATH *in your shell config* — **prepend, not append**: after `~/.cargo/bin` it loses and `command -v clave` still picks the stale one. Nothing else ever put it there, because `~/.cargo/bin` was already on PATH and answered to the name; (2) reload (`exec $SHELL`) — an un-reloaded shell removes the binary it is still resolving to; (3) confirm the launcher runs (`clave --version`, and that `command -v clave` now points inside `~/.local/share/clave/bin`); (4) only then `cargo uninstall clave` (registered even when the metadata version is stale — `rm` it if cargo does not know it); (5) `hash -r`. Both bash and zsh cache resolved command paths, so without it this shell keeps targeting the file you just deleted and the recovery looks broken in exactly the place it just worked. Reversing (1) and (4) is how the v0.1.2 cut produced `zsh: command not found: clave` on a healthy install. Then restart Step 1 |
| versions disagree anywhere | **STOP** | do not launch a session; report and diagnose |

### Step 2 — the generated artifact set agrees on one version

Both checks must pass. `$STABLE` deliberately omits `launch.kdl`: `just release`
never rewrites it (`write_generated` writes `config.kdl` and `layout.kdl` only) —
`clave` does, at cold start — so on any upgrade it still names the PREVIOUS
version and would report skew on a healthy cut. It gets checked in Step 3, once
it has been written.

**You type:**

```bash
clave_versions "${STABLE[@]}"       # expect: exactly one line, == the tag
clave_unversioned "${STABLE[@]}"    # expect: nothing at all
```

**Look at:** how many lines the first prints, and whether the second prints
anything.

**Report back:** both outputs verbatim — say "empty" if a command printed
nothing, so a silent failure and a clean pass cannot look alike in your report.

| What you see | Conclusion | Next |
|---|---|---|
| one version == the tag, **and** nothing from `clave_unversioned` | the artifact set is coherent | Step 3 |
| **zero** versions | **STOP.** The generated files are missing, unreadable, or malformed — `just release` did not run, or did not finish. Never read an empty result as a pass | report; do not launch |
| one version, but **≠ the tag** | **STOP.** The cut did not land; you are about to validate the previous release | re-run `just release` |
| two or more versions | **STOP — this is the incident.** Two plugin locations = two bar instances | report; do not launch |
| any output from `clave_unversioned` | **STOP** — an unversioned reference in a *stable* KDL loads a second, independent singleton, and the version count cannot see it: a file holding both a correct versioned path and a bare `clave` still reports exactly one version | report |

> **Empty is never a pass.** Both checks are greps: `clave_unversioned` printing
> nothing is the *good* outcome, while `clave_versions` printing nothing is a
> failure. Same-looking output, opposite meanings — which is why the step asks
> you to report the word "empty" rather than a blank line.

### Step 3 — the cold start, and the double-sidebar check

This is the step the whole runbook exists for.

**First, mark the log — BEFORE you launch anything.** The zellij log is shared
by every session on the machine and old entries linger (TESTING.md,
observability map), so the only sound filter is *lines appended after this
launch*. A date filter is not enough: on a normal release day it returns the
previous version's loads too and fails a coherent cut, and a midnight crossing
returns nothing at all.

```bash
ZLOG=$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log
{ wc -l < "$ZLOG" 2>/dev/null || echo 0; } | tee "$TMPDIR/clave-release-logmark"
```

(The `|| echo 0` is for a box that has never run zellij — no log file yet, so
every line is new.)

**Then prove there is no live session to attach to.** A cold start is the only
thing that picks up a release, and `clave` *attaches* to a live session rather
than cold-starting — the same mechanism that makes relaunching a failed binary
useless in Rollback. Attaching would validate the OLD session and append no new
log lines, which the table below would read as "the bar never loaded".

```bash
zellij list-sessions -n 2>/dev/null | grep -v EXITED
```

Expect **no `clave` line**. If one is there, kill it before continuing —
**yours to run, never an agent's** (TESTING.md, "the human drives all live
input"):

```bash
zellij kill-session clave
```

**Then you type:**

```bash
clave
```

**Look at:** the sidebar. **Exactly one bar per tab.** The classic failure is
two sidebars in one tab, or a bar that appears twice at different widths.

**Then, in another pane, you type:**

```bash
source "$TMPDIR/clave-release-checks.sh"     # new pane, new shell
ZLOG=$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log
tail -n +$(( $(cat "$TMPDIR/clave-release-logmark") + 1 )) "$ZLOG" \
  | grep "clave-bar: loaded v"
```

That's the full log line, `build=` field and all — read it, do not just count
versions. If you want the deduplicated version list too:

```bash
tail -n +$(( $(cat "$TMPDIR/clave-release-logmark") + 1 )) "$ZLOG" \
  | grep "clave-bar: loaded v" | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+' | sort -u
```

**Report back:** the count of sidebars you can see, and the full log lines —
not just the deduplicated version.

| What you see | Conclusion | Next |
|---|---|---|
| one sidebar, one version in the log, == the tag, and `build=` is **exactly the tag being cut** (`vX.Y.Z` — never `dev`, and never a bare commit SHA: a short SHA is a `dev-install` artifact from the working tree, the same stray-wasm case as `dev`) | **the cut is coherent** | Step 4 |
| one sidebar but **two versions** in the log | two instances, one may be zero-width or off-screen. Still a failure | **STOP**, report |
| two sidebars | the #43/#44 failure mode, live | **STOP**, go to Rollback |
| `build=dev` on an otherwise-correct line | **STOP.** A released bar reporting `dev` means the wasm that loaded was built without `CLAVE_BUILD_TAG` — either `just release` did not run the fixed recipe (#109), or something copied a stray working-tree wasm into the stable install. The version count alone cannot see this: version matches, tag does not, and this is exactly the "two builds of the same version" case the `build=` field exists to catch (FOOTGUNS.md) | report; do not go |
| no output | the bar never loaded — or the mark was taken after the launch. Report `tail -n 20 "$ZLOG"` | **STOP**, report |

**Also assert `launch.kdl` now, not in Step 2.** It is written by `clave` during
the cold start you just did, so this is the first moment it can be right — and
it gets **both** Step 0 checks, not just the version count. An unversioned
reference here is exactly as fatal as one in `config.kdl`, and the version count
alone cannot see it:

```bash
clave_versions "${LAUNCH[@]}"       # expect: exactly one line, == the tag
clave_unversioned "${LAUNCH[@]}"    # expect: nothing at all
```

Same verdicts as Step 2's table, **including the zero-versions row** — an empty
`clave_versions` here means `launch.kdl` was never written or is malformed, not
that all is well.

### Step 4 — navigation and binding actually work

Two populations of bars that don't share pipe state look fine until you
navigate. This step is what catches that.

**You do:** `Alt+j` and `Alt+k` a few times; open a new agent tab; switch away
and back.

**Look at:** does the highlighted row track the focused tab, does the new tab
get a status glyph rather than staying blank, does the row order behave.

**Report back:** which of those three worked and which didn't.

| What you see | Conclusion |
|---|---|
| all three behave | nav and bind are coherent |
| new tab never gets a glyph | bind is not landing — suspect the eager cold-start tab (RC-B in the dossier) |
| highlight lags or jumps to the wrong row | suspect frame coherence (RC-A) — a known open defect, **not** necessarily a release regression |

> RC-A and RC-B are tracked as S0 (#55) and are *pre-existing*. Note them, but
> they do not by themselves fail a cut.

### Step 5 — the doctor agrees

**You type:**

```bash
clave doctor; echo "exit=$?"
```

**Look at:** the exit status. That status, not the prose, is the verdict.

**Report back:** the full output *and* the exit line.

| What you see | Conclusion | Next |
|---|---|---|
| `exit=0` | doctor agrees the install is sound | Part C returns **go** |
| **any nonzero exit** | **STOP** | report and go to Rollback |

`exit=1` specifically means a `Severity::Problem` finding (`run_doctor`,
doctor.rs). Any *other* nonzero status is the doctor itself failing to run — 127
for a missing binary, 101 for a panic — and that is a worse signal, not a
lesser one: the check the release depends on did not execute. The condition is
`exit != 0`, never `exit == 1`.

Once #48 lands this becomes the cheap version of Steps 1–3: `clave doctor --json`
exits non-zero on incoherence, so it can be asserted on unattended. **Until then
the doctor cannot see PATH**, so it will report "no issues" during exactly the
incident this runbook is hunting — which is why Steps 1–3 are manual.

---

## Rollback

If any step says STOP.

> **Relaunching the old binary is NOT a rollback**, and this is the trap. Two
> reasons: `clave` attaches to the *live* session rather than cold-starting, so
> nothing is re-read; and even after that session is dead, `config.kdl` still
> names the failed version — the old binary sees its own wasm already present,
> so `needs_version_refresh` returns false (setup.rs) and setup is skipped. The
> generated files must be regenerated explicitly.

1. **Capture the evidence before touching anything.** It is gone once the
   session dies.

   ```bash
   ZLOG=$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log
   tail -n +$(( $(cat "$TMPDIR/clave-release-logmark") + 1 )) "$ZLOG" | tail -40
   clave_versions "${STABLE[@]}" "${LAUNCH[@]}"     # the skew, as it stands
   clave_unversioned "${STABLE[@]}" "${LAUNCH[@]}"
   ```

   Here `launch.kdl` is *included* deliberately — Step 2 excludes it because it
   is expected to lag, but a post-mortem wants the whole picture.

2. **Kill the failed session** — yours to run, never an agent's (TESTING.md).
   Regeneration is pointless while a session holds the old files open, and a
   live session would just be reattached.

   ```bash
   zellij kill-session clave
   ```

3. **Regenerate the stable config from the last-good release.** `clave setup`
   rewrites `config.kdl`, `layout.kdl` and the hooks against whichever binary
   runs it, so invoking the last-good versioned copy points the whole generated
   set back at itself:

   ```bash
   ~/.local/share/clave/bin/clave-v<LAST-GOOD> setup
   ```

4. **Relaunch and re-verify.** `launch.kdl` is rewritten on this cold start, so
   check it too — both checks, both file sets, and they must now print
   `v<LAST-GOOD>` and nothing.

   ```bash
   ~/.local/share/clave/bin/clave-v<LAST-GOOD>
   # then, in another pane:
   clave_versions "${STABLE[@]}" "${LAUNCH[@]}"
   clave_unversioned "${STABLE[@]}" "${LAUNCH[@]}"
   ```

5. **Delete the unpushed tag** (`git tag -d vX.Y.Z`) and file the finding with
   step 1's output. A tag is cheap; a mis-diagnosed outage is not.

---

## Part D — after a good cut (agent)

1. Update the release issue with Part C's reported results — including the
   steps that passed, not only the failures.
2. Open issues for anything Step 4 surfaced that is not already tracked.
3. Write the session handoff into `docs/status/` and include it in the PR.

---

## Why this is manual, and what would make it not

Every check above is a process/environment-seam assertion: what is on PATH, what
a *different* process baked into a file, what a *third* process loaded. Tier 1
cannot reach any of it, and **Tier 2 does not exist** (#47). Until it does, this
runbook is the control.

The two changes that would shrink Part C most:

- **#48** — `clave doctor --json` with a non-zero exit collapses Steps 1, 2 and
  5 into one assertable command an agent can run unattended.
- **#47** — a real-zellij harness in an isolated session would let Step 3's
  double-sidebar check run in CI, which is the single highest-value automation
  left in the project.
