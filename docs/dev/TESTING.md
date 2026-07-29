# Testing clave — verification tiers, risk taxonomy, and the live SOP

clave has three tiers of verification and they are not interchangeable. Most of
this document is **Tier 3**, the live-validation SOP: hard-won method that cannot
be derived from the code. But the live SOP is the tier that *requires the
maintainer at a terminal*, and treating it as the whole strategy is how the
v0.1.1 field incident happened — a correct release, a wrong binary on `PATH`, two
sidebars, dead navigation, and not one automated test in a position to notice
(#43, #44; CONTRIBUTING "The one leak").

So: read the tiers, find your change in the **risk taxonomy**, produce what that
row demands, and record it in the PR dossier. The **escape record** after it is
the evidence the taxonomy is built from — every row is a defect that actually got
through, and the tier that would have stopped it. Then **six shapes of
green-and-worthless test**: the same evidence from the other side, about tests
that were watching while a defect walked past them. If you are adding a test
rather than choosing a tier, start there.

## The verification tiers

### Tier 1 — hermetic

Runs anywhere: no TTY, no zellij, no `claude`, no auth, no network. This is the
tier an agent completes unattended, and it is the whole of CI today
(`.github/workflows/ci.yml`: test, wasm-build, lint).

| Instrument | Where | What it holds |
|---|---|---|
| Unit tests | across both crates; **63 in `crates/clave-bar/src/model.rs`** | the bar's state machine — the pure event→effect core, superbly covered |
| Proptests | `model.rs` (`proptest` is a `clave-bar` dev-dep) | invariants over generated event sequences; extend them whenever a new branch becomes reachable |
| Real-KDL-parser guardrail | `crates/clave/tests/kdl_guardrail.rs` | every generated artifact (config/layout/launch, the one-shot tab layout, the permission cache) parsed by the **exact** zellij-utils 0.44.3 parser. Substring tests assert *content*; this asserts *validity* — a dropped brace or a missing trailing `;` otherwise fails at session launch, where a dead `attach` blocks forever |
| Version-pin tripwire | `crates/clave/tests/zellij_pin_tripwire.rs` | every zellij-family crate in `Cargo.lock` resolves to **one** version, so the guardrail can never green-light templates against a parser the plugin no longer runs |
| CLI parse pins | `Cli::try_parse_from` tests in `crates/clave/src/main.rs` | that each plugin-invoked subcommand parses the literal arguments the plugin passes. Added after the `ArgAction` escape; required for **every new surface** |
| Sandboxed subcommand e2e | `CLAVE_STATE_DIR=<scratch> cargo run -p clave -- …` | one real end-to-end run of a new subcommand against a scratch store. Do it in a **debug** build — clap's `debug_assert` only fires there |

The gate (`just gates` runs all four, in CI's order):

```bash
cargo fmt --all --check  # CI's lint job runs this BEFORE clippy
cargo test --workspace   # --workspace is load-bearing: default-members excludes clave-bar
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

A bare `cargo test` exits 0 while skipping every one of the 63 model tests. Use
`just test`.

**Mutation testing is Tier 1 but deliberately not a gate.** `just mutants` runs
`cargo-mutants` over the lines this branch changed; it is a considered act, not
something every PR pays for. Which change classes owe a run is in the taxonomy
below, and the reasoning is in *Six shapes of green-and-worthless test*.

Two ways this list has bitten:

- **`cargo fmt --all --check` was missing from every doc until 2026-07-25**, so
  an agent could run all the documented gates green and still fail CI — which is
  exactly what happened to #66 (three hand-edited files, clippy-clean,
  fmt-dirty). CI's `lint` job is `fmt` **then** `clippy`; both must pass.
- `--workspace` on **both** `test` and `clippy` — the default-members form
  silently skips the entire wasm crate.

### Tier 2 — real-zellij integration (**does not exist yet — #47**)

Blocked on **#44** (the plugin must be told which binary to call instead of
resolving bare `clave` through `PATH`), because the harness needs exactly that
injection point to aim a session at a test binary. Ruling (2026-07-22): build it
immediately after #44.

The shape, so you know what is coming and can design toward it:

- **Isolation by construction** — the primitive already exists as the
  `CLAVE_SESSION` / `CLAVE_STATE_DIR` / `CLAVE_DATA_DIR` triple. A run creates
  `clave-it-<pid>` against temp dirs, so it can never touch the maintainer's
  `clave` session. The standing safety boundary holds mechanically, not by
  discipline.
- **The pane command is injectable** — tests spawn `sleep`/`cat`, not `claude`.
  CI therefore needs zellij on `PATH` and nothing else: no Claude Code, no auth,
  no network. That is what makes it cloud-agent runnable.
- **Method** — spin the session from generated artifacts, drive it with `zellij
  action`, assert against `dump-layout` / `dump-screen`, tear down in a guard
  that runs on panic.
- **Hazards to design around** — a `zellij action` against a dead session blocks
  forever, so every call needs a liveness gate *and* a timeout or CI hangs
  instead of failing; a PTY is required (`script -q` or a pty crate); flake
  quarantine and retry policy get decided up front, not bolted on.
- **First scenarios** — one bar per tab (the #44 regression), nav after an
  `Alt+w` close (#23), bind/prune round-trip through the store (#6), tab creation
  via `clave open`, collapse toggle parity (#5).

**Until it lands, treat this list as UNGUARDED.** Nothing automated today
executes the `clave` binary against a real zellij, so every process- and
environment-seam property is unverified: which binary answers a plugin shellout,
whether one session ends up with one bar per tab, whether navigation survives a
close, whether pipe deliveries arrive, whether the generated artifact set agrees
on a single version. A change touching that seam must carry a written argument in
the PR and an adversarial reviewer — that is the only guard there is.

One cheap piece is **Tier 1 and can be done today**: the pure-function assertion
proposed on #48 that the generated artifact set references exactly one version
and that every referenced path exists. That alone would have failed on the
mixed-version `launch.kdl`.

### Tier 3 — human, at a terminal

Everything below the taxonomy. Glyph fidelity, colour, Nerd Font coverage, pane
geometry, focus, repaint, resurrection, the toggle reflow, and "does this feel
right" on the maintainer's real fleet. No tier below it can substitute; the human
is ground truth.

Two labels carry this into the process:

- **`needs-live-validation`** — merged, but a maintainer pass is owed before the
  next tag. The PR must state its live steps, numbered, sandbox-first, each with
  its expected observation.
- **`host-untestable`** — human judgement only, permanently. Nothing automated
  will ever cover it.

The standing pass for the sidebar's interaction paths is
[`LIVE-INTERACTION-CHECKLIST.md`](LIVE-INTERACTION-CHECKLIST.md) — the D28 gate-2
run, written against the SOP below. Every item there states what would make its
own observation meaningless, which is the habit the rest of this document argues
for, applied to a terminal instead of a test runner.

`main` is guaranteed green, reviewed and hermetically verified; it is **not**
guaranteed live-validated. The **tag** is the promotion event, and #49 batches
every `needs-live-validation` PR merged since the previous tag into one focused
maintainer pass per cut. That is what keeps the maintainer the *last* line of
defence rather than the first — which is precisely how the v0.1.1 cycle went
wrong.

## The risk taxonomy

Find your change's class. The right-hand column is what the PR dossier must show
before you ask for a merge.

| Change class | Examples | Required before requesting merge | Label |
|---|---|---|---|
| **Pure logic / model** | `model.rs` state machine, store transitions, `render.rs` | TDD red-first; `cargo test --workspace` (`--workspace` is load-bearing); extend proptests if a new branch is reachable; **`just mutants`, with every survivor triaged in the dossier** | — |
| **Generated artifacts** | `config.kdl` / `layout.kdl` / `launch.kdl` generation | + real-parser guardrail + version-coherence and path-existence assertions; **`just mutants` if the change adds a conditional** | — |
| **External-format parsing** | jsonl tail scanners, hook payloads | + a captured (not invented) fixture, and a dated measurement that the shape still exists in the field | — |
| **CLI surface** | new subcommand or flag | + `Cli::try_parse_from` pin + one sandboxed end-to-end run in a **debug** build (clap's `debug_assert` only fires there) | — |
| **Cross-process / IPC** | pipes, plugin shellouts, multi-writer store paths | + written argument for ordering/idempotency in the PR dossier; adversarial reviewer must attack it; tier-2 coverage once #47 lands | — |
| **Install / environment** | release mechanics, dev-install, `PATH`, doctor | + fresh-environment reasoning; assume nothing about the maintainer's machine | `needs-live-validation` |
| **Visual / UX** | glyphs, colours, widths, fonts | human judgement only | `host-untestable` |

Why each row is what it is:

- **Pure logic / model** — this is the tier that already works, and the reason it
  works is red-first discipline plus proptests. The failure mode is not weak
  coverage but *unreached* coverage: the stale width-seek anchor was pure logic
  living behind an interrupt shape the generator never produced (#4). A new
  branch without a new property is a new blind spot. This is also the row that
  owes a **mutation run**: shapes 1–4 of green-and-worthless test (below) all
  lived here, in the tier with the strongest coverage in the repo, and for
  shapes 2–4 `just mutants` is the only instrument that reports them.
- **Generated artifacts** owe a mutation run only when the change introduces a
  branch. The parser guardrail proves the artifact is valid; nothing proves the
  generator took the right branch to produce it, and a dropped condition is
  exactly the mutant that survives.
- **External-format parsing** is the row shape 5 exists for: `{"type":"summary"}`
  was parsed by production code, covered by a green test, and present in **0 of
  153** real transcripts. A hand-written fixture cannot notice that; a capture
  plus a dated measurement can.
- **Generated artifacts** — a KDL string that *contains* the right substrings can
  still be structurally invalid, and it fails at **session launch**, the worst
  possible place: a dead `attach` blocks forever and the human sees nothing. The
  parser guardrail catches invalidity; version-coherence catches the other half —
  a perfectly valid `launch.kdl` baking `v0.1.0` paths inside a `v0.1.1` session
  is what produced two sidebars (#43).
- **CLI surface** — the CLI layer had *no* coverage at all until the `ArgAction`
  escape, because the plugin is the only caller and nothing in the suite invoked
  it. clap-derive turns a bare `bool` field into a flag, so `clave collapse true`
  could never parse — and the diagnostic `debug_assert` fires only in debug
  builds, which is why the e2e run must be a debug build.
- **Cross-process / IPC** — no test in any existing tier models *arrival order*
  between two fire-and-forget subprocesses. The prune race (#6/#26) was found by
  a reviewer reasoning about ordering, not by execution, and the fix was to make
  the payload idempotent and commuting rather than to test harder. Until #47,
  written argument plus adversarial review **is** the verification.
- **Install / environment** — these changes are validated against exactly one
  machine, the maintainer's, and that machine already has state on it. The
  v0.1.1 incident was a `PATH` collision that no clean checkout would ever show.
  Reason from a fresh environment, and assume the maintainer is daily-driving:
  never `cargo install` or `just dev-install` from a working session.
- **Visual / UX** — no automated tier will ever adjudicate a glyph, a colour, or
  whether the reflow feels right. Label it and write the steps.

## The escape record

The taxonomy is derived from these, not asserted. Every row is a real defect that
reached `main` or the field.

| Escaped | Where it bit | Why no tier caught it | What catches it now |
|---|---|---|---|
| `clave-bar` shells out to bare `clave` through `PATH`, in 7 places (#44) | v0.1.1 daily driving, 2026-07-22: a stale `0.1.0` dev binary served `clave open` inside a `v0.1.1` session and composed tab layouts pointing at the old wasm → **two plugin populations**, duplicate sidebar, half-dead nav | no test executes the binary, and none runs a real zellij — the process seam is entirely unmodelled | #44 injects the absolute binary path at config-generation time; #47 (tier 2) asserts one bar per tab; #48 (`clave doctor`) asserts version coherence live |
| No unversioned stable entry point; `just dev-install` writes `~/.cargo/bin/clave` (#43) | same incident: whatever `clave` resolved to won the cold start and generated a mixed-version `launch.kdl` | the KDL guardrail asserts *validity*, nothing asserted *coherence* across the artifact set; the failure is invisible until it manifests as duplicated UI | **#43a**: the cut installs and refreshes an unversioned **launcher** at `<data>/bin/clave`, so the entry point is owned rather than inherited from `PATH`. **#43b**: `dev-install` installs `clave-dev` and collides with nothing. Plus the pure-function tests — `generated_artifact_set_is_version_coherent` (one version per artifact) and `released_artifacts_exist_and_the_launcher_is_never_baked` (every reference is an installed file, and never the floating launcher); the live five-way check is still #48 |
| clap `ArgAction` on the `collapse` bool positional (#5, PR #13) | `clave collapse true` could not parse at all; debug builds trip clap's own `debug_assert` on every parse | **nothing exercised the CLI layer** — the plugin was the only caller and no test invoked it | `Cli::try_parse_from` pin per subcommand + one sandboxed debug e2e; both now taxonomy requirements. Found by CodeRabbit CLI, an independent lane |
| Full-live-set prune payload was order-unsafe (#6, PR #26) | two fire-and-forget `clave prune-tabs` subprocesses have no arrival order; a "retain these live ids" payload landing after a new tab's bind unbinds a **live** agent, and `bind_effects` is `sent_binds`-guarded so it never re-fires → #6 double-attach via a race | model tests cover single-writer logic; nothing models cross-process arrival order | payload carries observed-**stale** ids (idempotent, commuting). Found by review, not by tests |
| Prune emission was set-change-gated (PR #26, Codex) | a close `TabUpdate` arriving before its `PaneUpdate` leaves `is_active_instance()` false, silently dropping the effect — and the gate then never retries | an event-interleaving shape the proptests never generated | emission is detection-driven and self-limiting via the store echo; `last_live_ids` deleted |
| Stale width-seek anchor (#4, PR #27, Codex) | drift gate measured against a mid-flight *emit* anchor, so the bar parked off-target (reproduced: 30→16→6, then an external 26) | pure logic **inside** a covered tier — the sim harness existed, but the proptest never generated that interrupt shape | `settle_at()` pins the anchor to the accepted rest width at every settle path, plus a pinned regression seed |
| `Alt+w` close stranded Alt-↑/↓ nav until a mouse click (#23) | live sessions only | the beacon/anchor relationship only exists once real tabs open and close | `Effect::ReanchorVisit`, executor-gated; tier 2 will assert it (#47 first scenarios) |
| `CliPipe did not complete within 1s` + empty-payload deliveries (#45) | present since the log's first line, v0.1.0 era; buried the real evidence during the v0.1.1 incident | no tier reads the zellij log; nothing asserts on pipe delivery | nothing yet — it is filed. Observability discipline (below) is the only detector |

Read the pattern before you argue with the taxonomy: **the pure state machine has
never been the problem.** Everything that escaped lived at a seam — process,
environment, event ordering, or the screen.

## Six shapes of green-and-worthless test

The escape record above is about defects that reached `main`. This section is
about the tests that were *watching* while they did. Five of these shapes were
found in a single session building the sidebar renderer (LEDGER D27, 2026-07-29);
the sixth arrived the day after that entry was written.

**Read the root before the list, because five of the six share it: the test
asserts against the implementation instead of against an independently derived
expectation.** Shapes 1–4 are *test and code agree because the test came from the
code*. Shape 5 is *test and code agree because both were written from the same
wrong belief*. Neither form can fail, because there is only one belief in the
room and the test is a second copy of it.

Shape 6 is different in kind, and it is the **only one of the six caught for
free**: CI caught it on the first push. The reason it was caught is exactly the
reason the other five are not — **CI has a genuinely different view of the
world** (no `~/.gitconfig`, no signing key), and nothing else in the suite has a
different enough view to notice the rest. That asymmetry is the whole argument
for the three habits at the end of this section: each one is an attempt to buy a
second, differently-informed opinion.

If you are about to add a test, the checklist at the end is the short form.

### 1 — It passes under *both* branches of the thing it names

**The failure.** The test names a discriminating property and then picks a
witness that does not discriminate: the assertion is satisfied by the behaviour
under test *and* by its opposite.

**The instance.** `mix_rounds_ties_to_even`
(`crates/clave-bar/src/render.rs:1085`). Colour blending was ported from the
ratified Python preview, and Python's `round()` is round-half-to-**even** while
`f64::round` is half-away-from-zero. The test asserted fujiWhite's blue channel,
`149.5 -> 150` — which is 150 under **both** modes. The port could have been
reverted to `round` and the test would have stayed green. It now asserts
waveRed's blue instead: `118 + (40 - 118) * 0.25 = 98.5`, which is **98**
ties-to-even and **99** half-away-from-zero, with that arithmetic written into
the doc-comment so the next reader can check the witness themselves.
(FOOTGUNS, "Text, glyphs, rendering" — the ties-to-even entry.)

**What does not catch it.** Coverage: the line runs. CI: it is green
everywhere. Red/green: it was born green and stayed green through the exact
regression it exists to prevent.

**What does.** Naming the rival implementation and checking the witness tells
them apart — write both numbers in the comment, as that doc-comment now does. A
witness that agrees with both candidates is proving the arithmetic, not the rule.
Mutation testing helps only where the tool's operator set happens to contain the
rival; a stdlib method swap is not in it, so this shape is the one that stays
manual.

### 2 — It goes green-and-vacuous when a constant moves

**The failure.** The test still runs and still passes, but a constant it
hard-codes has moved onto the value it used to be contrasted against, so it now
exercises none of the behaviour it was written for.

**The instance.** `BAR_TARGET_COLS` went 30 -> 44 and `COLLAPSED_TARGET_COLS` 4
-> 30 (#63). `30` had been both the old expanded target *and* the seek tests'
arbitrary "far from target" start width — and it is now the **collapsed** target.
`seek_collapses_to_the_gutter_despite_coarse_steps`
(`crates/clave-bar/src/model.rs:2046`) started its collapsed model at 30, i.e.
already converged: the drive loop turned zero times and the assertion held
trivially. Sixteen seek tests went red on that sweep and got attention; two went
green and vacuous and produced no signal at all. The start widths are now chosen
off both targets and say so — `model.rs:2029`, `:2046`, `:3477`, `:3525` each
carry a comment naming the number they no longer use and why.

**What does not catch it.** Coverage, CI, and red/green — the sweep's red tests
were the loud ones; these were silent by construction.

**What does.** The rule in FOOTGUNS ("Build, test, CI", the width-constant
entry, marked *DISCHARGED, and it recurs*): after moving a constant, **re-derive
every literal that was chosen relative to it, and audit the tests that stayed
green — not only the ones that went red.** Mechanically: mutation testing. A
mutant that makes `width_seek` return no effects survives a test whose loop never
turns.

### 3 — It stays green, stays meaningful, and silently covers *less*

**The failure.** The assertion is still true and still worth making. The path
taken to reach it has shrunk, and nothing says so.

**The instance.** `harness_newborn_converges_on_the_template_from_above`
(`crates/clave-bar/src/model.rs:3448`) started the simulator at 60. Against the
old target of 30 that drove **two** resizes — the second one past the
pre-learning ±4 slack and into the learned-step acceptance band. Against 44 it
drives **one**, so it stopped exercising the post-learning band entirely while
continuing to assert convergence, correctly. The fix picks a start and step (66,
12) that still force two, and then pins the coverage itself:

```rust
assert!(steps >= 2, "start width must drive at least two resizes, drove {steps}");
```

**What does not catch it.** Line coverage least of all — the same lines run,
just fewer times. Not CI, and not red/green in **either** direction: the suite is
green before and after the coverage shrinks.

**What does.** **Assert the path, not only the outcome.** If a test's value
depends on how many steps it takes, or which branch it goes through, make that an
assertion in the test. Mutation testing finds the second-order version: the
branch that stopped being reached shows up as a surviving mutant.

### 4 — Its name and comment claim a property it never proves

The worst of the six, because it sits exactly where the next agent looks to rule
the bug out.

**The failure.** The test's name is a stronger statement than its assertion, so
a reviewer reads the name and stops looking. Usually the assertion is
algebraically identical to something already proven a few lines above.

**The instance.** `the_two_targets_are_separated_by_more_than_the_widest_acceptance_band`
(`crates/clave-bar/src/model.rs:1976`), which carried a comment saying *"the
bands are also disjoint"*. Its two assertions — `2 * sep > MAX_LEARNABLE_STEP`
and `sep > 10` — are the same statement written twice. What it actually proves is
that neither **target** falls inside the other's band. What `Alt+c` depends on is
that no **width** is accepted for both, a strictly stronger property, and one
that 44/30 had already lost: at a learned step of 14 or more the bands overlap,
`toggle()` deliberately keeps the learned step, so `Alt+c` emits zero resizes and
the pane does not move (LEDGER D21).

**What does not catch it.** Nothing mechanical — and a human is *anti*-caught,
because the name is the reason they stop reading.

**What does.** **Drive the property through the code instead of restating it as
algebra.** `no_width_is_accepted_for_both_targets` (`model.rs:2007`) calls
`width_seek` for every `(step, cols)` in `0..=MAX_LEARNABLE_STEP` x `0..=200` and
asserts no pair is quiet for both targets; a test that restates a predicate can
drift from the predicate, a test that calls it cannot. Mutation testing then
confirmed this is the **sole** guard against a band-widening reversion (LEDGER
D19). The weak test is kept, at its own honest scope, with a doc-comment stating
exactly what it does and does not prove and pointing at the strong one.

### 5 — Its fixture pins a shape reality abandoned

**The failure.** Production code parses a line shape, the fixture contains that
shape, the test is green — and the field stopped producing it, possibly before
the code was written. Test and code agree perfectly and both are wrong about the
world.

**The instance.** `{"type":"summary"}`. `summary_from_tail`
(`crates/clave/src/hook.rs:197`) scans for it. Measured 2026-07-28: **0 of 919**
local transcripts contain one. Re-measured 2026-07-29: **0 of 153**, while
`{"type":"ai-title"}` appears in **74 of 153**. So S6 §6.4's entire "a summary
earns the label" tier had **never once fired in production**, and every test
covering it was asserting against a hand-written fixture of a shape the field no
longer emits. The row fields are retargeted to `ai_title_from_tail`
(`hook.rs:179`); the extinct line is kept as a fallback behind it, the *label*
tier is deliberately left pointing at it (retargeting every tab name in the field
is S4's call, not a side effect), and
`ai_title_beats_the_extinct_summary_line_and_the_prompt_seed` (`hook.rs:808`)
pins the precedence. (FOOTGUNS, "Claude transcripts" — and LEDGER D23.)

**What does not catch it.** Nothing in the suite, at any tier. Coverage is
total, red/green is stable, CI is green, and mutation testing would report the
parser as perfectly guarded — because it is, against a fixture nobody in the
world produces any more.

**What does.** A **liveness assertion over real samples**, habit 2 below. There
is no substitute: this shape is only visible from outside the repository.

### 6 — It passes on ambient environment it never declares

**The failure.** The test reads something it did not set — a global config file,
an environment variable, a key on the developer's keyring — so it is testing the
machine as much as the code. Tier 1's definition is *hermetic*; this is a test
that quietly is not.

**The instance.** `ensure_worktree_is_re_runnable_over_a_shared_repo`
(`crates/clave/src/dev.rs:974`) shells out to real `git` — deliberately, because
the bug lives entirely in git's own semantics and a mocked git would have agreed
with the broken code. But `git commit` needs a `user.name` and `user.email`: a
developer machine supplies them from `~/.gitconfig` and a CI runner does not.
Green locally, red on the first push. The fixture now sets identity **and**
`commit.gpgsign=false` inside the repo it creates (`dev.rs:991-1002`) — the
second one is the same failure in reverse and was queued up next, because the
maintainer signs every commit globally and a runner has no key to sign with.

**What does not catch it.** The local suite, ever. Every developer machine
shares the same ambient state, so the whole team can be green forever.

**What does.** **CI, for free — and that is the entire point of this section.**
The lesson generalises to a rule and a command. The rule: *a test that shells out
owns every input that command reads.* The command, which reproduces CI's view of
git without a push:

```bash
GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null cargo test --workspace
```

Same class, different variables: `HOME`, `PATH`, `TMPDIR`, `EDITOR`, `TZ`,
locale, the system clock, and anything a subprocess resolves for itself. If the
test reads it and did not set it, the test is not hermetic.

## Three habits, in leverage order

### 1 — Mutation testing (`just mutants`)

The only mechanical catcher of shapes 2–4 — and of shape 1 only where the rival
implementation happens to fall inside its operator set, which the stdlib
rounding-mode swap does not ("Where it does not reach", below). Shapes 1 and 5
are the two that stay manual. It removes or alters a piece of the code and
re-runs the suite; a mutant that **survives** is a line you can change while
every test keeps passing. It was used by hand three times in the D27
session and found something every time — including proving that
`no_width_is_accepted_for_both_targets` is the *sole* test standing between the
codebase and a band-widening reversion, which is exactly the kind of fact no
other instrument reports.

```bash
just mutants              # mutants in the lines this branch changed vs `main`
just mutants HEAD~3       # ...vs any other base
just mutants-file crates/clave-bar/src/render.rs   # one module, deliberately
```

Configured in [`.cargo/mutants.toml`](../../.cargo/mutants.toml). Three things
are load-bearing, and two of them are `--workspace` wearing different hats:

- **`test_workspace = true`** (config) — the same trap as `just test`.
  `default-members` excludes the wasm-only `clave-bar`, so the `cargo test` a
  mutant run defaults to skips every `model.rs` and `render.rs` test, and *every*
  `clave-bar` mutant would survive for a reason that has nothing to do with the
  code.
- **`--workspace` on the command line** (there is no config key for it) — it
  governs mutant **generation**, which follows `default-members` too. Measured on
  this tree: `cargo mutants --file crates/clave-bar/src/render.rs --list` yields
  **0** mutants and exits 0; adding `--workspace` yields **81**. Both `just`
  recipes pass it.
- **`exclude_globs`** — `crates/clave-bar/src/main.rs` sets `[[bin]] test =
  false`, so nothing in it is reachable by any test and 100% of its mutants
  survive by construction (FOOTGUNS, "Build, test, CI"). Examples are excluded
  for the same reason. A tool that reports known-meaningless survivors is a tool
  people learn to ignore.

**A surviving mutant is a finding, not a failure.** It says *this line can be
changed and nothing notices*, and there are exactly three honest responses: add
the missing assertion; delete the code, because unobservable behaviour is often
dead; or write down in the doc-comment why the survivor is expected. Never
weaken a test to make one disappear.

**`just mutants` is deliberately NOT part of `just gates`.** Gates run on every
PR and must stay fast; a mutation run is a deliberate act, and the risk taxonomy
above says which change classes owe one. It is scoped to changed lines by
default because a full run over `model.rs` alone is enormous, and a gate nobody
can afford to run is a gate nobody runs.

Where it does not reach: it mutates function bodies, return values and
operators, so it will not substitute one rounding mode for another (shape 1), and
it cannot know a fixture is extinct (shape 5).

**What the first run actually reported**, so the cost and the output shape are
known quantities rather than a promise —
`just mutants-file crates/clave-bar/src/render.rs`, 2026-07-29:

```
81 mutants tested in 5m: 2 missed, 75 caught, 3 unviable, 1 timeouts
```

Read all four buckets, they mean different things:

- **2 missed** — both `Rgb::hex` (`render.rs:108`), replaced by `String::new()`
  and by `"xyzzy".into()`, with the whole suite still green. That is correct and
  it is a real finding: `hex()` has exactly one caller, `bar-preview.rs`, which
  is an excluded example, so nothing in the suite observes it at all. Recorded,
  not fixed — the honest options are a test or a deletion, and which one is a
  design call, not a mutation-report call.
- **75 caught** — on a module of this size, that ratio is what a well-tested
  module looks like, and it is the number the goldens and the per-cell width
  assertions are buying.
- **3 unviable** — `Default::default()` substituted for `Rgb` and `(char, Rgb)`;
  `Rgb` has no `Default`, so those mutants do not compile. Noise, not signal.
- **1 timeout** — `replace + with *` inside `strip_sgr` (`render.rs:316`), an
  index arithmetic mutation that stops the scan terminating. A timeout is *not* a
  survivor: the mutant was detected, just by hanging rather than by an assertion.

**`just mutants` exits non-zero (code 3) when anything is missed.** That is the
tool reporting a finding, not the recipe being broken — which is another reason
it is not wired into `just gates`.

### 2 — Fixtures captured from reality, with a liveness assertion

Shape 5's only antidote. The discipline has three parts.

**Capture, do not invent.** A fixture for an external format is a *capture* of a
real sample, filed under `crates/clave/tests/fixtures/<source>/` (transcripts
under `transcripts/`), with a header comment recording what it was captured from
and on what date. An invented fixture encodes a belief; a capture encodes an
observation, and only one of those can be contradicted by the world.

**Assert liveness, in two halves.** A checked-in capture cannot itself go
extinct — it is frozen — so the assertion has to straddle the repository
boundary:

- *Hermetic half, in the suite:* every line shape production parses appears in
  at least one checked-in capture. This fails the day someone adds a parser for a
  shape no real sample contains.
- *Field half, dated and documented:* a re-measurement against the live
  transcript tree, with the command in the doc-comment and the **date and counts
  of the last run** recorded beside it — the form FOOTGUNS already uses:

  ```bash
  # inventory every line type the field actually emits
  grep -ho '"type":"[a-z-]*"' ~/.claude/projects/*/*.jsonl | sort | uniq -c | sort -rn
  # count transcripts carrying one specific shape
  grep -rl '"type":"summary"' --include='*.jsonl' ~/.claude/projects | wc -l
  ```

A dated measurement makes staleness *visible*: "0 of 153, 2026-07-29" is a fact a
reviewer can challenge, where an undated fixture is not. Refresh on any change
to a parser, and whenever a measurement is older than the behaviour it justifies.

**Scrub, without exception — this repository is public and transcripts contain
personal data.** A capture is committed for its **shape**, so everything that is
not shape comes out:

- keep the `type` key and only the fields the parser reads; drop the rest of the
  line rather than truncating it,
- replace every free-text value (prompts, titles, summaries) with synthetic
  text written for the test,
- strip `cwd`, `gitBranch`, absolute paths and hostnames — no home-directory path
  may survive in any form,
- replace session and user identifiers with the deterministic scenario form
  already used by `dev.rs`: `00000000-0000-4000-8000-c85c…`.

A pre-commit PII blocklist rejects private local path names in staged lines
(FOOTGUNS, "Process and tooling"). Treat a rejection as correct: genericise the
line, and keep the reason out of the commit message.

### 3 — A golden carries its derivation

**The rule: a golden's doc-comment must show how the literal follows from the
design, so a reviewer can check it against the *design* rather than against the
code that emitted it.** A golden regenerated from the renderer and reviewed
against the renderer proves only that the renderer agrees with itself — which is
shape 1 at the scale of a whole picture.

Three parts, all present in
[`render.rs`](../../crates/clave-bar/src/render.rs):

1. **The arithmetic, in prose.** `golden_bar_collapsed_at_thirty_columns`
   (`render.rs:996`) derives its column map from D16's formula rather than
   pasting what the code emits: `summary = cols - 13 - title - repo`, so at
   `title = 7, repo = 3` (D17) that is `30 - 13 - 7 - 3 = 7`.

   The `13` is every fixed cell that is neither title nor repo, and it is worth
   spelling out because it is where this arithmetic goes wrong: the **left cap
   lives inside the gutter**. `GUTTER_W = 9` spans cols 1–9 as *cap, status,
   space, rule, space, battery, space, provenance, space* — so `13` is `9`
   gutter `+ 1` space after title `+ 1` space after repo `+ 1` right margin
   `+ 1` right cap, and `Widths::min_intact_cols()` is that `13 + title + repo`
   (`23` collapsed, `27` expanded). Adding a cap **on top of** the 9 counts it
   twice and totals 31.

   The full collapsed row at `cols = 30`, checkable a line at a time against
   `render_row`:

   ```text
   cols  1–9   gutter, left cap included    9
   cols 10–16   title                       7   (D17: holds at 7 in BOTH profiles)
   col     17   space                       1
   cols 18–20   repo                        3   (D17; D18 drops the ellipsis)
   col     21   space                       1
   cols 22–28   summary                     7   = 30 - 13 - 7 - 3, the only flex cell (D9)
   col     29   right margin                1
   col     30   right cap                   1
                                           --
                                            30
   ```

   The same map at `EXPANDED`/44 is `9 + 7 + 1 + 7 + 1 + 17 + 1 + 1 = 44`: only
   `repo` and `summary` moved, which is what makes collapsed a width profile and
   not a second layout (D16).

2. **The citation.** Every choice names the lock section or LEDGER decision it
   comes from — D17 for holding `title` at 7 across both profiles, D18 for why a
   3-cell repo drops the ellipsis and truncates `"clave"` to `"cla"`.
3. **Self-checks that re-derive rather than re-read.**
   `golden_bar_at_forty_four_columns` (`render.rs:907`) was the weaker of the two
   and was strengthened to match: it now recomputes the title, repo and summary
   spans from `Widths::EXPANDED` and asserts `DESIGN_COLS - 2 - summary_start ==
   17`, so a golden regenerated from a renderer that moved a column fails **here**,
   in arithmetic traceable to lock §2, instead of being accepted as the new
   picture.

The regeneration ritual (`cargo run -p clave-bar --example bar-preview`) belongs
in the doc-comment too, along with the condition on it: regenerate only **after**
confirming the change against the lock. A golden updated to match new output,
with no derivation touched, is a golden that has stopped testing anything.

## Before you add a test — the short form

- Can this assertion pass under the **opposite** implementation? Name the rival
  and check your witness distinguishes them. (1)
- Does any literal in it depend on a constant elsewhere? Say where the number
  came from, in the test. (2)
- Does its value depend on the **path** it takes? Assert the path. (3)
- Does the name promise more than the assertion delivers? Strengthen the
  assertion or rename the test — never leave the gap. (4)
- Is the fixture a capture or an invention? If invented, what says the field
  still produces that shape, and when was that last measured? (5)
- Does it read anything it did not set — env var, global config, `PATH`, `HOME`,
  `TZ`, the clock, a subprocess's own defaults? Set it in the fixture. (6)
- Then run `just mutants` and read the survivors.

---

The rest of this document is **Tier 3 in full**: the live-validation SOP. It is
unchanged and load-bearing.

clave drives a live terminal multiplexer. **No automated test can cover live
behavior** — pane geometry, focus, glyph repaints, resurrection, the toggle
reflow. Those are real Zellij, real `claude`, real eyes on the screen. This
document is the method the maintainer and his agent use to validate that
behavior. It cannot be derived from the code; it is written down here because it
is the only way the next session knows how to work.

Read it whole before you touch anything that renders. If you are an agent, the
single most important line is the one in the next section: **you read
observability, you do not drive the terminal.**

## The interaction contract

There is a hard division of labor, and it is not negotiable:

- **The human drives all live input.** Every keypress, every session launch and
  kill, every visual observation — the human. They are the only one who can see
  the screen and the only one who should touch it.
- **The agent reads observability and never puppets the live session.** You read
  logs, the store, and `dev status`. When a session needs to be launched or
  killed, you **print the exact command for the human to run** — you do not run
  it. clave's own `dev` subcommands follow this rule by construction: `dev
  reset` prints the kill-session command rather than executing it, and the
  module never launches or kills a Zellij session itself.

Why so strict? Because the failure modes here are silent and expensive (see the
hazard below), and because the human's screen is ground truth. An agent that
"helpfully" runs a `zellij action` can hang the whole loop with no error.

## Agent-side sanctioned commands

An agent may run **exactly these**, and every one is scoped to the test session.
Nothing else touching Zellij is yours — session lifecycle is always the human's.

**Hot-reload the sandbox bar** (the one sanctioned live mutation an agent may
make — see the instrumentation recipe):

```bash
ZELLIJ_SESSION_NAME=clave-test zellij action start-or-reload-plugin \
  "file:$HOME/.local/state/clave-dev/data/clave-bar.wasm" -c clave_binary=clave
```

> The `-c` is load-bearing, not optional. A plugin's configuration is half of
> its zellij identity, and `reload_plugin` matches on `(location,
> configuration)` exactly (`zellij-server/src/plugins/wasm_bridge.rs:686-697`).
> Without it the lookup misses: `all_plugin_ids_for_plugin_location`
> (`zellij-server/src/plugins/plugin_map.rs:169-171`) returns
> `Err(PluginDoesNotExist)` for the filtered-empty case, `reload_plugin`
> propagates it (`wasm_bridge.rs:692-693`), and the error branch in
> `zellij-server/src/plugins/mod.rs:446-468` logs `"Plugin {} not found,
> starting it instead"` and starts a **new** plugin instance. So a
> configuration miss is not a silent no-op — it spawns a second bar pane,
> the very symptom of #44. If a reload looks like it did nothing, grep
> `zellij.log` for that `not found, starting it instead` warning. The
> sandbox bakes bare `clave` (#44), so `clave_binary=clave` is the value
> there; a stable session would need its versioned absolute path.
> `PluginUserConfiguration`'s `FromStr`
> (`zellij-utils/src/input/layout.rs:563-576`) is comma-separated `key=value`,
> so a path containing a comma would not survive — none of ours do.

**List sessions** (read-only, safe anywhere):

```bash
zellij list-sessions
```

**Dump the sandbox layout** (env-scoped to `clave-test` — pane geometry truth):

```bash
ZELLIJ_SESSION_NAME=clave-test zellij action dump-layout
```

> **Hazard — this blocks forever.** A `zellij action` aimed at an absent or dead
> session **blocks indefinitely and never errors**. An ungated `dump-layout`
> once hung `dev status` for minutes before a session existed. Always gate on
> liveness first. `clave dev status` does exactly this: it checks the session is
> live before it ever runs `dump-layout`, and returns an empty dump otherwise.
> When in doubt, ask `dev status` — never fire a bare `zellij action` on faith.

## The sandbox lifecycle

**The short version: `just sandbox [scenario]`.** It does steps 2–3 below
against the current working tree without installing anything to the daily
surface — the safe replacement for `just dev-install` in sandbox work. It
builds the tree, drops the wasm in the sandbox data dir, wires a **PATH shim**
so the bar reaches *this* build, regenerates `config.kdl` **and** `launch.kdl`
together, self-checks the #44 identity pair, verifies it touched neither
`~/.cargo/bin/clave` nor `~/.local/share/clave`, and prints the launch command.
It refuses to run against a live `clave-test` (see the hot-reload note below),
and it never launches — step 1 and step 4 stay the human's.

The PATH shim is load-bearing, not tidiness: the sandbox data dir holds no
versioned CLI copy, so generation bakes bare `clave` and the bar resolves it
through `PATH` at runtime. Without the shim that is the **stable**
`~/.cargo/bin/clave` — quite possibly built before the change under test, and
version strings will not give it away (a pre-#44 `0.1.1` and a post-#44 `0.1.1`
are indistinguishable to `clave --version` and to the `clave-bar: loaded`
log line).

The full reset-to-fresh cycle, done by hand. Commands marked **(human)** are
the human's to run in a **non-zellij terminal**; the rest an agent may run.

1. **Kill the running session (human):**
   ```bash
   zellij kill-session clave-test && zellij delete-session --force clave-test
   ```
   (`clave dev reset` prints this line for you before it wipes anything.)
2. **Reset the sandbox:** `clave dev reset` — wipes the sandbox root and removes
   the scenario transcripts (see *What reset removes*).
3. **Seed a scenario:** `clave dev scenario <name>` — creates the fixture world
   and prints the launch command.
4. **Launch (human, non-zellij terminal):** `clave dev launch` — attaches or
   creates the `clave-test` session against the sandbox state and data dirs.

> **After `just dev-install`, re-run step 3 (`clave dev scenario <name>`)
> before launching.** `clave dev launch` composes `launch.kdl` fresh every
> time but does NOT regenerate `config.kdl` (only `dev scenario` does —
> `dev.rs`). Since #44 both files bake `clave_binary`, so a post-#44
> `launch.kdl` beside a pre-#44 `config.kdl` makes every keybind miss and
> spawn a second bar — the exact #44 symptom, mistaken for the fix not
> working. Regenerating both together is the guard.

> **And never regenerate against a LIVE session.** Zellij watches the
> `--config` file of every running session and hot-swaps its keybinds in place
> (`zellij-server src/lib.rs:2175` → `ConfigWrittenToDisk` `:2298` →
> `ScreenInstruction::Reconfigure` `screen.rs:717`, ~1s poll), but the running
> bar keeps the plugin identity it loaded with. So a `dev scenario` (or
> `just release`) aimed at a live session re-keys its keybinds to an identity
> the on-screen bar does not have, and the next keypress **starts a second
> bar**. Kill the session first — step 1 — then regenerate, then launch. The
> two notes together give the rule: **regenerate both artifacts, always, and
> only while the session is dead.**

### Scenario catalog

The scenarios are defined in `crates/clave/src/dev.rs` and map 1:1 to the C8
steps in the validation ledger. Each seeds a **real** resumable `claude`
transcript (a few tokens via `claude -p`) plus a store row, so resurrection is
verified for real, not mocked. UUIDs are deterministic and self-identifying:
`00000000-0000-4000-8000-c85c…` (`c85c` ≈ "c8 scenario").

| Scenario | Seeds | Validates |
|---|---|---|
| `c8-cold-start` | 3 agents at staggered recency (60s / 1h / 24h ago), none worktree | Cold relaunch: most-recent agent resumes focused with history, no ENTER gates; the rest sit dormant `◌` in recency order |
| `c8-worktree` | 2 agents; one in a real `git worktree` | Dwell-open resumes the agent **in its worktree path**, store row intact |
| `c8-stale` | 2 agents; one has its cwd deleted after seeding | The staleness branch (§6.3): dwelling the row → `✗`, no tab created, the session is unaffected |

### What reset removes

`clave dev reset` removes two things and nothing else:

- **The sandbox scenario state** — `~/.local/state/clave-dev/state/` (store,
  evlog) and `~/.local/state/clave-dev/repos/` (seeded fixture repos). The
  `data/` dir **survives**: it holds the sandbox wasm and generated config, a
  build artifact installed by `just dev-install`, not scenario state — wiping
  it would break the reset → scenario → launch loop with a rebuild demand.
- **The scenario transcripts in the real `~/.claude/projects/`.** Because
  Claude's identity is deliberately *not* sandboxed, those `claude -p` seed
  transcripts land in your real Claude tree. Reset finds them by their
  `c85c`-tagged UUID prefix (the `is_scenario_jsonl` filter) and removes exactly
  those, leaving every real transcript untouched.

**Claude identity is not sandboxed** (ruling, 2026-07-18). The sandbox isolates
clave state only. `claude` runs as the real you; the seed hooks still land in
the sandbox store because they inherit `CLAVE_STATE_DIR` from their `claude`
parent process. Do not try to re-sandbox it — CLAUDE_CONFIG_DIR isolation broke
auth, and that is why the ruling exists.

## The observability map

Four windows into what actually happened. Learn all four.

- **Zellij plugin log** — where `eprintln!` from the wasm bar surfaces:
  ```
  $TMPDIR/zellij-<uid>/zellij-log/zellij.log
  ```
  **This file is shared by every Zellij session on the machine, and old entries
  linger.** Always filter by **today's date AND the build tag** — an unfiltered
  tail is a mix of every session's history and will mislead you.
- **The evlog** — `clave.log`, JSON lines, one per host-side decision. There is
  one per state dir: `~/.local/state/clave/clave.log` for stable,
  `~/.local/state/clave-dev/state/clave.log` for the sandbox.
- **`clave dev status`** — the agent's primary probe. Emits JSON:
  `session_live` (bool), `live_uuids` (parsed from the layout dump), and the
  full `store`. Liveness-gated, so it is always safe to run.
- **Env-scoped `dump-layout`** — `ZELLIJ_SESSION_NAME=clave-test zellij action
  dump-layout` — the ground truth for pane geometry and the serialized spawn
  commands. Gate on liveness first (see the hazard above).

## The instrumentation recipe

This is the debugging loop that has found real bugs (the C6 announce storms, the
C8 fixed-pane resize bug). Follow it in order:

1. **Add a temporary `eprintln!`** with a grep-able marker prefix, e.g.
   `eprintln!("CLAVE_DBG_seek cols={cols}")`.
2. **Rebuild the wasm with a fresh build tag:**
   ```bash
   CLAVE_BUILD_TAG=$(date +%m%d-%H%M%S) \
     cargo build -p clave-bar --target wasm32-wasip1 --release
   ```
3. **Copy it into the SANDBOX data dir only** — never the stable install:
   ```bash
   cp target/wasm32-wasip1/release/clave-bar.wasm \
     ~/.local/state/clave-dev/data/clave-bar.wasm
   ```
4. **Hot-reload** with the sanctioned command from above (env-scoped to
   `clave-test`).
5. **The human exercises the behavior** on screen.
6. **Read the zellij log filtered by your marker AND the build tag** — both, so
   you are reading this run and not a ghost of an earlier one.
7. **Strip the instrumentation before committing.** Temporary means temporary.

> **Caveat — hot-reload resets plugin state.** A reload reincarnates every bar
> model from scratch. That is a confound (state you were watching is gone) *and*
> a tool (it clears a parity desync — see C8 — and re-hydrates cleanly, which is
> how C9 hydration was validated). Know which one you are relying on.

## Lore discipline

Two habits, both born of expensive lessons.

**Read the C-section before you touch the subsystem.**
[`SUBSYSTEM-VALIDATION.md`](../superpowers/spikes/SUBSYSTEM-VALIDATION.md) is the
ledger of what was tried and why it failed. Every entry is a dead end someone
paid for:

- **C6 (toggle / `hide_self` reflow)** is the saga of the announce storms —
  round after round of `is_active_instance` self-diagnosis poisoning during
  event bursts, until the announce was reduced to bounded birth/organic
  triggers. It is also where `show_self()` was found (in the *vendored* Zellij
  source) to be a focus action that switches tabs. Do not reintroduce
  suppress/hide-self tricks, fixed pane sizes, or self-diagnosed announces
  without reading it first.
- **C8 (resume + resurrection)** is why serialization is off and resurrection is
  clave-owned and lazy — serialization records the discovered child process, so
  a serialized `claude --session-id` would collide against an existing jsonl.
  It is also where the fixed-pane resize bug and the collapse **parity-desync**
  bug class live. This is the section your `clave dev` scenarios exercise.

Before touching a subsystem, read its section — the forbidden approaches were
each expensive to learn.

**Never trust assumed Zellij semantics — read the vendored source.** Behavior
that "obviously" works one way has repeatedly worked another (`TabUpdate`
reaches only the active tab; `resize_pane_with_id` silently refuses fixed panes;
`show_self` is a focus action). The vendored crates are here:

```
~/.cargo/registry/src/*/zellij-tile-0.44.3/
~/.cargo/registry/src/*/zellij-utils-0.44.3/
```

`zellij-server` is not vendored but is fetchable from crates.io — several C6/C8
findings were confirmed against it. Read the source before you build on a
behavior, and record what you find in the ledger in the same commit as the
change.
