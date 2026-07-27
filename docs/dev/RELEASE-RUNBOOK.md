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
| **agent** | Part A and Part D. Reads logs, the store, `clave dev status`. **Prints** every zellij command; runs none of them. Never runs `just release`. |
| **maintainer** | The tag, `just release`, every keypress in Part C, killing a session, and the go/no-go. **The tag is pushed only after the go** — Part B. |

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

3. **Run Part C.** Do not continue past it without a go.

4. **On a go — publish the tag:**

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
| `clave` resolves to `~/.cargo/bin/clave` | **STOP.** This is the v0.1.1 mechanism. A dev build will cold-start and bake its own version into `launch.kdl` | fix PATH, then restart Step 1 |
| versions disagree anywhere | **STOP** | do not launch a session; report and diagnose |

### Step 2 — the generated artifact set agrees on one version

Two checks, and **both must pass** — (a) counts versions, (b) catches the
references (a) is structurally blind to. `*.kdl` is *not* used: it would glob in
`launch.kdl`, which `just release` never rewrites (it is written by `clave` at
cold start, Step 3), so on any upgrade it still names the PREVIOUS version and
(a) would report two versions and order a STOP on a healthy cut.

**You type:**

```bash
cd ~/.local/share/clave
# (a) which versions the RELEASE-generated files reference — launch.kdl excluded
grep -rhoE 'clave-bar-v[0-9]+\.[0-9]+\.[0-9]+\.wasm|clave-v[0-9]+\.[0-9]+\.[0-9]+' \
  config.kdl layout.kdl ~/.config/zellij/config.kdl 2>/dev/null | sort -u
# (b) any UNVERSIONED reference — must print NOTHING
grep -nE 'clave_binary "clave"|Run "clave"|file:[^"]*clave-bar\.wasm' \
  config.kdl layout.kdl ~/.config/zellij/config.kdl 2>/dev/null
```

(b) matches the three places a version can go missing — the `clave_binary`
plugin-config value, the `Run` command in a keybind, and the `file:` plugin
location (setup.rs `config_kdl`/`layout_kdl`) — rather than searching for a
bare `clave` anywhere, which would hit every line: the data dir is *itself*
`~/.local/share/clave/`.

**Look at:** how many distinct versions (a) prints, and whether (b) prints
anything at all.

**Report back:** (a)'s full sorted list, and (b)'s output or "empty".

| What you see | Conclusion | Next |
|---|---|---|
| (a) exactly one version == the tag, **and** (b) empty | the artifact set is coherent | Step 3 |
| (a) two or more versions | **STOP — this is the incident.** Two plugin locations = two bar instances | report; do not launch |
| (b) prints any line | **STOP** — an unversioned reference in a *stable* KDL loads a second, independent singleton, and (a) cannot see it: a file holding both a correct versioned path and a bare `clave` still reports exactly one version | report |

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

**Then you type** (a *new* session — this is the only thing that picks up a
release):

```bash
clave
```

**Look at:** the sidebar. **Exactly one bar per tab.** The classic failure is
two sidebars in one tab, or a bar that appears twice at different widths.

**Then, in another pane, you type:**

```bash
ZLOG=$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log
tail -n +$(( $(cat "$TMPDIR/clave-release-logmark") + 1 )) "$ZLOG" \
  | grep "clave-bar: loaded v" | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+' | sort -u
```

**Report back:** the count of sidebars you can see, and that command's output.

| What you see | Conclusion | Next |
|---|---|---|
| one sidebar, one version in the log, == the tag | **the cut is coherent** | Step 4 |
| one sidebar but **two versions** in the log | two instances, one may be zero-width or off-screen. Still a failure | **STOP**, report |
| two sidebars | the #43/#44 failure mode, live | **STOP**, go to Rollback |
| no output | the bar never loaded — or the mark was taken after the launch. Report `tail -n 20 "$ZLOG"` | **STOP**, report |

**Also assert `launch.kdl` now, not in Step 2.** It is written by `clave` during
the cold start you just did, so this is the first moment it can be right:

```bash
grep -ohE 'clave-bar-v[0-9.]+\.wasm|clave-v[0-9.]+' \
  ~/.local/share/clave/launch.kdl | sort -u
```

One version, == the tag. Anything else is the same STOP as Step 2 (a).

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

**Look at:** the exit status. `run_doctor` exits **1** if any finding is a
`Severity::Problem` and 0 otherwise (doctor.rs) — that status, not the prose,
is the verdict.

**Report back:** the full output *and* the exit line.

| What you see | Conclusion | Next |
|---|---|---|
| `exit=0` | doctor agrees the install is sound | Part C returns **go** |
| `exit=1` | **STOP** — a Problem-severity finding. Report the marked lines and go to Rollback | **STOP** |

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
   grep -rhoE 'clave-bar-v[0-9.]+\.wasm|clave-v[0-9.]+' \
     ~/.local/share/clave/*.kdl | sort -u
   ```

2. **Kill the failed session** — yours to run, never an agent's (AGENTS.md).
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

4. **Relaunch and re-verify** with Step 2 (a) and (b) — they must now print
   `<LAST-GOOD>` and nothing, respectively. `launch.kdl` is rewritten on this
   cold start.

   ```bash
   ~/.local/share/clave/bin/clave-v<LAST-GOOD>
   ```

5. **Delete the unpushed tag** (`git tag -d vX.Y.Z`) and file the finding with
   step 1's output. A tag is cheap; a mis-diagnosed outage is not.

---

## Part D — after a good cut (agent)

1. Update the release issue with Part C's reported results — including the
   steps that passed, not only the failures.
2. Open issues for anything Step 4 surfaced that is not already tracked.
3. Write the handoff (`docs/status/`), per AGENTS.md.

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
