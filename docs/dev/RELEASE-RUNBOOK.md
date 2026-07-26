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
| **maintainer** | The tag, `just release`, every keypress in Part C, and the go/no-go. |

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

## Part B — the tag (maintainer, watched)

```bash
git tag vX.Y.Z && git push origin vX.Y.Z
```

Then watch `release.yml`. #35 owns the CI-pipeline checklist — all four targets
build, attestations verify, checksums publish. **`just release` is the
maintainer's command, always. An agent never runs it.**

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

**You type:**
```bash
grep -rhoE 'clave-bar-v[0-9]+\.[0-9]+\.[0-9]+\.wasm|clave-v[0-9]+\.[0-9]+\.[0-9]+' \
  ~/.local/share/clave/*.kdl ~/.config/zellij/config.kdl 2>/dev/null | sort -u
```

**Look at:** how many distinct versions appear.

**Report back:** the full sorted list.

| What you see | Conclusion | Next |
|---|---|---|
| exactly one version, == the tag | the artifact set is coherent | Step 3 |
| two or more versions | **STOP — this is the incident.** Two plugin locations = two bar instances | report; do not launch |
| a bare `clave` or an unversioned `clave-bar.wasm` in a *stable* KDL | **STOP** — an unversioned reference loads a second, independent singleton | report |

### Step 3 — the cold start, and the double-sidebar check

This is the step the whole runbook exists for.

**You type** (a *new* session — this is the only thing that picks up a release):
```bash
clave
```

**Look at:** the sidebar. **Exactly one bar per tab.** The classic failure is
two sidebars in one tab, or a bar that appears twice at different widths.

**Then, in another pane, you type:**
```bash
grep "clave-bar: loaded v" $TMPDIR/zellij-$(id -u)/zellij-log/zellij.log \
  | grep "$(date +%Y-%m-%d)" | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+' | sort -u
```

**Report back:** the count of sidebars you can see, and that command's output.

| What you see | Conclusion | Next |
|---|---|---|
| one sidebar, one version in the log, == the tag | **the cut is coherent** | Step 4 |
| one sidebar but **two versions** in the log | two instances, one may be zero-width or off-screen. Still a failure | **STOP**, report |
| two sidebars | the #43/#44 failure mode, live | **STOP**, go to Rollback |
| log filter returns nothing | the bar never loaded, or you filtered the wrong day | report the unfiltered last 20 lines |

> The log file is shared by every zellij session on the machine and old entries
> linger — that is why the date filter is not optional (TESTING.md,
> observability map).

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
clave doctor
```

**Report back:** the full output.

Once #48 lands this becomes the cheap version of Steps 1–3: `clave doctor --json`
exits non-zero on incoherence, so it can be asserted on unattended. **Until then
the doctor cannot see PATH**, so it will report "no issues" during exactly the
incident this runbook is hunting — which is why Steps 1–3 are manual.

---

## Rollback

If any step says STOP:

1. **Do not keep using the session.** Note what you saw first — the evidence is
   gone once you kill it.
2. Relaunch from the last known-good versioned binary by absolute path:
   ```bash
   ~/.local/share/clave/bin/clave-v<LAST-GOOD>
   ```
3. Capture, before anything else overwrites it:
   ```bash
   grep "clave-bar: loaded v" $TMPDIR/zellij-$(id -u)/zellij-log/zellij.log \
     | grep "$(date +%Y-%m-%d)" | tail -40
   ```
4. File the finding with that output. A tag is cheap; a mis-diagnosed outage is
   not.

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
